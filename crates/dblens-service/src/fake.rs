//! 测试用的假驱动：按 SQL 文本给出预设的结果，并记录收到的调用。
//!
//! - `ROWS n`：返回一个结果集，n 行，每行一个整数；
//! - `FAIL`：SQL 错误；`FAILAT n`：带出错位置 n 的 SQL 错误；`DISCONNECT`：连接错误；
//! - `SELECT current_schema()`：当前 schema（初始为 public），`SET search_path TO "x"` 切换它；
//! - `SLEEP`：一直等到被取消，然后返回 `Cancelled`；
//! - 其他：影响 1 行。

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use dblens_core::{Error, Result, Value};
use dblens_driver::{
    CancelHandle, Capabilities, ColumnMeta, ConnectParams, Connection, Driver, DriverId, ExecEvent,
    ExecStream, ServerInfo,
};
use tokio::sync::Notify;

pub type Log = Arc<Mutex<Vec<String>>>;

pub struct FakeDriver {
    pub log: Log,
}

#[async_trait]
impl Driver for FakeDriver {
    fn id(&self) -> DriverId {
        DriverId::Postgres
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            catalogs: true,
            schemas: true,
            transactional_ddl: true,
            cancel: true,
            explain: false,
            multiple_result_sets: false,
        }
    }

    async fn connect(&self, p: &ConnectParams) -> Result<Box<dyn Connection>> {
        if p.password == "wrong" {
            return Err(Error::Auth { message: "密码错误".into() });
        }
        self.log.lock().unwrap().push("connect".into());
        Ok(Box::new(FakeConn::new(self.log.clone())))
    }
}

pub struct FakeConn {
    log: Log,
    schema: String,
    cancelled: Arc<AtomicBool>,
    wake: Arc<Notify>,
}

impl FakeConn {
    pub fn new(log: Log) -> Self {
        Self {
            log,
            schema: "public".into(),
            cancelled: Arc::new(AtomicBool::new(false)),
            wake: Arc::new(Notify::new()),
        }
    }
}

struct FakeStream {
    events: VecDeque<ExecEvent>,
    rows_left: i64,
    next_value: i64,
    sleep: Option<(Arc<AtomicBool>, Arc<Notify>)>,
}

#[async_trait]
impl ExecStream for FakeStream {
    async fn next(&mut self, max_rows: usize) -> Result<Option<ExecEvent>> {
        if let Some((flag, wake)) = &self.sleep {
            while !flag.swap(false, Ordering::SeqCst) {
                wake.notified().await;
            }
            self.sleep = None;
            return Err(Error::Cancelled);
        }
        if let Some(ev) = self.events.pop_front() {
            return Ok(Some(ev));
        }
        if self.rows_left > 0 {
            let n = (max_rows as i64).min(self.rows_left);
            let rows = (0..n).map(|k| vec![Value::Int(self.next_value + k)]).collect();
            self.next_value += n;
            self.rows_left -= n;
            if self.rows_left == 0 {
                self.events.push_back(ExecEvent::ResultEnd);
            }
            return Ok(Some(ExecEvent::Rows(rows)));
        }
        Ok(None)
    }
}

#[async_trait]
impl Connection for FakeConn {
    fn set_lob_limit(&mut self, bytes: usize) {
        self.log.lock().unwrap().push(format!("lob_limit {bytes}"));
    }

    async fn execute<'a>(&'a mut self, sql: &'a str, _: &'a [Value]) -> Result<Box<dyn ExecStream + 'a>> {
        let sql = sql.trim();
        self.log.lock().unwrap().push(format!("execute {sql}"));
        // 导出只接受查询语句：`SELECT ROWS n`、`SELECT SLEEP`、`SELECT FAIL` 与去掉 SELECT 的语句相同
        let sql = sql
            .strip_prefix("SELECT ")
            .filter(|s| s.starts_with("ROWS ") || matches!(*s, "SLEEP" | "FAIL"))
            .unwrap_or(sql);
        let mut stream = FakeStream { events: VecDeque::new(), rows_left: 0, next_value: 1, sleep: None };
        match sql {
            "FAIL" => {
                return Err(Error::Sql {
                    code: "42000".into(), message: "语法错误".into(), position: None
                });
            }
            "DISCONNECT" => return Err(Error::connect("连接中断")),
            "SELECT current_schema()" => {
                stream
                    .events
                    .push_back(ExecEvent::ResultStart { columns: vec![ColumnMeta::new("s", "text")] });
                stream.events.push_back(ExecEvent::Rows(vec![vec![Value::String(self.schema.clone())]]));
                stream.events.push_back(ExecEvent::ResultEnd);
            }
            _ if sql.starts_with("FAILAT ") => {
                return Err(Error::Sql {
                    code: "42601".into(),
                    message: "语法错误".into(),
                    position: sql[7..].parse().ok(),
                });
            }
            _ if sql.starts_with("SET search_path TO ") => {
                self.schema = sql[19..].trim_matches('"').to_string();
                stream.events.push_back(ExecEvent::Affected(0));
            }
            "SLEEP" => stream.sleep = Some((self.cancelled.clone(), self.wake.clone())),
            // 表数据编辑的冲突：没有影响任何行
            _ if sql.contains("zero_rows") => stream.events.push_back(ExecEvent::Affected(0)),
            _ => match sql.strip_prefix("ROWS ").and_then(|n| n.parse::<i64>().ok()) {
                Some(n) => {
                    stream
                        .events
                        .push_back(ExecEvent::ResultStart { columns: vec![ColumnMeta::new("n", "int4")] });
                    stream.rows_left = n;
                    if n == 0 {
                        stream.events.push_back(ExecEvent::ResultEnd);
                    }
                }
                None => stream.events.push_back(ExecEvent::Affected(1)),
            },
        }
        Ok(Box::new(stream))
    }

    fn cancel_handle(&self) -> CancelHandle {
        let (flag, wake, log) = (self.cancelled.clone(), self.wake.clone(), self.log.clone());
        CancelHandle::new(move || {
            let (flag, wake, log) = (flag.clone(), wake.clone(), log.clone());
            async move {
                log.lock().unwrap().push("cancel".into());
                flag.store(true, Ordering::SeqCst);
                wake.notify_one();
                Ok(())
            }
        })
    }

    async fn set_autocommit(&mut self, on: bool) -> Result<()> {
        self.log.lock().unwrap().push(format!("autocommit {on}"));
        Ok(())
    }

    async fn commit(&mut self) -> Result<()> {
        self.log.lock().unwrap().push("commit".into());
        Ok(())
    }

    async fn rollback(&mut self) -> Result<()> {
        self.log.lock().unwrap().push("rollback".into());
        Ok(())
    }

    async fn ping(&mut self) -> Result<()> {
        Ok(())
    }

    async fn server_info(&mut self) -> Result<ServerInfo> {
        Ok(ServerInfo { product: "PostgreSQL".into(), version: "PostgreSQL 17.0 (fake)".into() })
    }

    async fn close(self: Box<Self>) -> Result<()> {
        self.log.lock().unwrap().push("close".into());
        Ok(())
    }
}
