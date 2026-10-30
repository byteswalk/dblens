//! 持有 tiberius 连接的后台任务（见 lib.rs 开头的说明）。
//!
//! 一批的结果按顺序变成 `Item`：结果集（列 → 行 → 结束）、影响行数、服务器消息。结果集之后的 DONE 带的是返回的行数，
//! 不当作影响行数。读取方丢弃了执行流（不再读）时，向服务器发取消并丢掉剩下的结果，连接继续可用。

use std::pin::Pin;
use std::sync::Arc;

use dblens_core::{Error, Result, Row, Value};
use dblens_driver::ColumnMeta;
use futures::TryStreamExt;
use tiberius::{ColumnType, QueryItem, ToSql};
use tokio::sync::futures::Notified;
use tokio::sync::{Notify, mpsc, oneshot};

use crate::TdsClient;
use crate::error::{map_err, server_line, with_line};
use crate::types::{Param, cell_value, type_name};

pub(crate) enum Item {
    Start(Vec<ColumnMeta>),
    Row(Row),
    End,
    Affected(u64),
    Message(String),
}

pub(crate) type Reply<T> = oneshot::Sender<Result<T>>;

pub(crate) enum Cmd {
    Exec {
        sql: String,
        params: Vec<Value>,
        lob_limit: usize,
        out: mpsc::Sender<Result<Item>>,
    },
    /// 执行一批不返回结果的语句（事务控制、ping）。
    Batch(&'static str, Reply<()>),
    Version(Reply<String>),
    Close(Reply<()>),
}

/// 一批执行到哪里停下。
enum Flow {
    Done,
    /// 用户取消：通知服务器，并向读取方报 `Cancelled`。
    Cancelled,
    /// 读取方丢弃了执行流：通知服务器停下。
    Abandoned,
    Failed(Error),
}

pub(crate) async fn run(mut client: TdsClient, mut rx: mpsc::Receiver<Cmd>, cancel: Arc<Notify>) {
    while let Some(cmd) = rx.recv().await {
        match cmd {
            Cmd::Exec { sql, params, lob_limit, out } => {
                let cancelled = cancel.notified();
                tokio::pin!(cancelled);
                // 先登记，执行开始后的取消都能收到；没有在执行时的取消（notify_waiters）不会留到下一次
                cancelled.as_mut().enable();
                match stream(&mut client, &sql, &params, lob_limit, &out, cancelled).await {
                    Flow::Done => {}
                    Flow::Cancelled => {
                        let _ = client.cancel_query().await;
                        let _ = out.send(Err(Error::Cancelled)).await;
                    }
                    Flow::Abandoned => {
                        let _ = client.cancel_query().await;
                    }
                    Flow::Failed(e) => {
                        let _ = out.send(Err(e)).await;
                    }
                }
            }
            Cmd::Batch(sql, reply) => {
                let _ = reply.send(batch(&mut client, sql).await);
            }
            Cmd::Version(reply) => {
                let _ = reply.send(version(&mut client).await);
            }
            Cmd::Close(reply) => {
                let _ = reply.send(client.close().await.map_err(map_err));
                return;
            }
        }
    }
    let _ = client.close().await;
}

fn sql_error(e: tiberius::error::Error, sql: &str) -> Error {
    let line = server_line(&e);
    with_line(map_err(e), sql, line)
}

/// 执行一批并读完结果（只用于不返回结果集的内部语句）。
async fn batch(client: &mut TdsClient, sql: &str) -> Result<()> {
    let stream = client.simple_query(sql).await.map_err(map_err)?;
    stream.into_results().await.map(|_| ()).map_err(map_err)
}

async fn version(client: &mut TdsClient) -> Result<String> {
    let row =
        client.simple_query("SELECT @@VERSION").await.map_err(map_err)?.into_row().await.map_err(map_err)?;
    let text: Option<&str> = row.as_ref().and_then(|r| r.get(0));
    // @@VERSION 有多行（版权、版本、操作系统），第一行是产品和版本号
    Ok(text.unwrap_or_default().lines().next().unwrap_or_default().trim().to_string())
}

/// 把一项交给读取方；等待期间收到取消或读取方已不在时返回对应的停止原因。
async fn send(
    out: &mpsc::Sender<Result<Item>>,
    item: Item,
    cancelled: Pin<&mut Notified<'_>>,
) -> Option<Flow> {
    tokio::select! {
        biased;
        _ = cancelled => Some(Flow::Cancelled),
        r = out.send(Ok(item)) => r.is_err().then_some(Flow::Abandoned),
    }
}

async fn stream(
    client: &mut TdsClient,
    sql: &str,
    params: &[Value],
    lob_limit: usize,
    out: &mpsc::Sender<Result<Item>>,
    mut cancelled: Pin<&mut Notified<'_>>,
) -> Flow {
    let params = match params.iter().map(Param::new).collect::<Result<Vec<_>>>() {
        Ok(p) => p,
        Err(e) => return Flow::Failed(e),
    };
    let refs: Vec<&dyn ToSql> = params.iter().map(|p| p as &dyn ToSql).collect();
    // 没有参数时按批发送：USE、SET、临时表等留在会话中；有参数时经 sp_executesql
    let started = async {
        if refs.is_empty() { client.simple_query(sql).await } else { client.query(sql, &refs).await }
    };
    let mut rows = tokio::select! {
        biased;
        _ = cancelled.as_mut() => return Flow::Cancelled,
        r = started => match r {
            Ok(s) => s,
            Err(e) => return Flow::Failed(sql_error(e, sql)),
        },
    };
    let mut types: Vec<ColumnType> = Vec::new();
    let mut in_set = false;
    loop {
        let next = tokio::select! {
            biased;
            _ = cancelled.as_mut() => return Flow::Cancelled,
            _ = out.closed() => return Flow::Abandoned,
            r = rows.try_next() => r,
        };
        let item = match next {
            Err(e) => {
                if in_set && let Some(f) = send(out, Item::End, cancelled.as_mut()).await {
                    return f;
                }
                return Flow::Failed(sql_error(e, sql));
            }
            Ok(None) => {
                if in_set && let Some(f) = send(out, Item::End, cancelled.as_mut()).await {
                    return f;
                }
                return Flow::Done;
            }
            Ok(Some(QueryItem::Metadata(m))) => {
                if in_set && let Some(f) = send(out, Item::End, cancelled.as_mut()).await {
                    return f;
                }
                in_set = true;
                types = m.columns().iter().map(|c| c.column_type()).collect();
                Item::Start(
                    m.columns()
                        .iter()
                        .map(|c| ColumnMeta::new(c.name(), type_name(c.column_type())))
                        .collect(),
                )
            }
            Ok(Some(QueryItem::Row(r))) => {
                let row: Result<Row> =
                    r.into_iter().zip(&types).map(|(data, ty)| cell_value(&data, *ty, lob_limit)).collect();
                match row {
                    Ok(row) => Item::Row(row),
                    Err(e) => return Flow::Failed(e),
                }
            }
            // 结果集之后的行数是返回的行数，结果集到此结束
            Ok(Some(QueryItem::Done(_))) if in_set => {
                in_set = false;
                Item::End
            }
            Ok(Some(QueryItem::Done(n))) => Item::Affected(n),
            Ok(Some(QueryItem::Info(info))) => Item::Message(info.message),
        };
        if let Some(f) = send(out, item, cancelled.as_mut()).await {
            return f;
        }
    }
}
