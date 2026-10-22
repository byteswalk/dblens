use std::sync::Arc;

use async_trait::async_trait;
use dblens_core::{DEFAULT_LOB_PREVIEW_BYTES, Result, Row};
use dblens_driver::{ExecEvent, ExecStream};
use mysql_async::prelude::Protocol;
use mysql_async::{Column, QueryResult};

use crate::error::map_err;
use crate::value::{column_meta, to_value};

enum State {
    /// 位于两个结果集之间，下一步检查是否还有结果集。
    Between,
    InRows,
    /// 当前结果集的行已读完，下一步发出 ResultEnd。
    EndPending,
}

/// MySQL 执行流。`QueryResult` 在一个结果集读完后会自动跳到下一个结果集，
/// `columns()` 为 None 表示已没有更多结果集；列为空的结果集是 OK 包（影响行数）。
pub(crate) struct MyExec<'a, P: Protocol> {
    result: QueryResult<'a, 'static, P>,
    columns: Arc<[Column]>,
    state: State,
}

impl<'a, P: Protocol> MyExec<'a, P> {
    pub(crate) fn new(result: QueryResult<'a, 'static, P>) -> Self {
        Self { result, columns: Arc::from(Vec::new()), state: State::Between }
    }
}

fn convert_row(row: mysql_async::Row, columns: &[Column]) -> Row {
    row.unwrap()
        .into_iter()
        .zip(columns)
        .map(|(v, c)| to_value(v, c).cap_large(DEFAULT_LOB_PREVIEW_BYTES))
        .collect()
}

#[async_trait]
impl<'a, P> ExecStream for MyExec<'a, P>
where
    P: Protocol + Send + Sync + 'a,
{
    async fn next(&mut self, max_rows: usize) -> Result<Option<ExecEvent>> {
        match self.state {
            State::Between => {
                let Some(columns) = self.result.columns() else {
                    return Ok(None);
                };
                if columns.is_empty() {
                    let affected = self.result.affected_rows();
                    // 消费这个 OK 结果，推进到下一个结果集
                    self.result.next().await.map_err(map_err)?;
                    return Ok(Some(ExecEvent::Affected(affected)));
                }
                let metas = columns.iter().map(column_meta).collect();
                self.columns = columns;
                self.state = State::InRows;
                Ok(Some(ExecEvent::ResultStart { columns: metas }))
            }
            State::InRows => {
                let mut rows = Vec::new();
                while rows.len() < max_rows.max(1) {
                    match self.result.next().await.map_err(map_err)? {
                        Some(row) => rows.push(convert_row(row, &self.columns)),
                        None => {
                            self.state = State::EndPending;
                            break;
                        }
                    }
                }
                if rows.is_empty() {
                    self.state = State::Between;
                    Ok(Some(ExecEvent::ResultEnd))
                } else {
                    Ok(Some(ExecEvent::Rows(rows)))
                }
            }
            State::EndPending => {
                self.state = State::Between;
                Ok(Some(ExecEvent::ResultEnd))
            }
        }
    }
}
