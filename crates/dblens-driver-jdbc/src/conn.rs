use std::sync::Arc;

use dblens_core::{Result, Value};
use dblens_driver::blocking::{BlockingConnection, BlockingExec};
use dblens_driver::{CancelHandle, ColumnMeta, ExecEvent, ServerInfo};

use crate::error::{Flavor, expect, protocol};
use crate::frame::{Column, Req, Resp};
use crate::process::Bridge;

pub(crate) struct JdbcConnection {
    pub(crate) bridge: Arc<Bridge>,
    pub(crate) id: u64,
    pub(crate) flavor: Flavor,
}

impl JdbcConnection {
    /// 执行一条不关心结果的语句。
    pub(crate) fn run(&self, sql: &str) -> Result<()> {
        let (k, b) = self.bridge.call(&Req::Execute { conn: self.id, sql: sql.into(), params: vec![] })?;
        expect(k, &b, 0, self.flavor)?;
        self.simple(Req::EndExec { conn: self.id })
    }

    fn simple(&self, req: Req) -> Result<()> {
        let (k, b) = self.bridge.call(&req)?;
        match expect(k, &b, 0, self.flavor)? {
            Resp::Ok => Ok(()),
            _ => Err(protocol()),
        }
    }

    pub(crate) fn cancel_handle(&self) -> CancelHandle {
        let (bridge, id, flavor) = (self.bridge.clone(), self.id, self.flavor);
        CancelHandle::new(move || {
            let bridge = bridge.clone();
            async move {
                tokio::task::spawn_blocking(move || {
                    let (k, b) = bridge.call(&Req::Cancel { conn: id })?;
                    expect(k, &b, 0, flavor).map(|_| ())
                })
                .await
                .map_err(|e| dblens_core::Error::internal(e.to_string()))?
            }
        })
    }
}

fn meta(c: Column) -> ColumnMeta {
    ColumnMeta {
        nullable: c.nullable,
        precision: c.precision,
        scale: c.scale,
        ..ColumnMeta::new(c.name, c.type_name)
    }
}

enum Stage {
    Start(Vec<ColumnMeta>),
    Rows(usize),
    End,
    Affected(u64),
    Done,
}

struct Exec<'a> {
    conn: &'a JdbcConnection,
    stage: Stage,
}

impl BlockingExec for Exec<'_> {
    fn next(&mut self, max_rows: usize) -> Result<Option<ExecEvent>> {
        match std::mem::replace(&mut self.stage, Stage::Done) {
            Stage::Start(columns) => {
                self.stage = Stage::Rows(columns.len());
                Ok(Some(ExecEvent::ResultStart { columns }))
            }
            Stage::Rows(n) => {
                let (k, b) =
                    self.conn.bridge.call(&Req::Fetch { conn: self.conn.id, max_rows: max_rows as u32 })?;
                let Resp::Rows { done, rows } = expect(k, &b, n, self.conn.flavor)? else {
                    return Err(protocol());
                };
                self.stage = if done { Stage::End } else { Stage::Rows(n) };
                if rows.is_empty() {
                    self.stage = Stage::Done;
                    return Ok(Some(ExecEvent::ResultEnd));
                }
                Ok(Some(ExecEvent::Rows(rows)))
            }
            Stage::End => Ok(Some(ExecEvent::ResultEnd)),
            Stage::Affected(n) => Ok(Some(ExecEvent::Affected(n))),
            Stage::Done => Ok(None),
        }
    }
}

impl Drop for Exec<'_> {
    fn drop(&mut self) {
        // 提前丢弃时关掉游标，下一条语句才能执行
        if matches!(self.stage, Stage::Start(_) | Stage::Rows(_)) {
            let _ = self.conn.bridge.call(&Req::EndExec { conn: self.conn.id });
        }
    }
}

impl BlockingConnection for JdbcConnection {
    fn execute<'a>(&'a mut self, sql: &'a str, params: &'a [Value]) -> Result<Box<dyn BlockingExec + 'a>> {
        let (k, b) =
            self.bridge.call(&Req::Execute { conn: self.id, sql: sql.into(), params: params.to_vec() })?;
        let stage = match expect(k, &b, 0, self.flavor)? {
            Resp::Result(cols) => Stage::Start(cols.into_iter().map(meta).collect()),
            Resp::Affected(n) => Stage::Affected(n),
            _ => return Err(protocol()),
        };
        Ok(Box::new(Exec { conn: self, stage }))
    }

    fn set_lob_limit(&mut self, bytes: usize) {
        let _ = self.simple(Req::SetLobLimit { conn: self.id, bytes: bytes as u64 });
    }

    fn set_autocommit(&mut self, on: bool) -> Result<()> {
        self.simple(Req::SetAutocommit { conn: self.id, on })
    }

    fn commit(&mut self) -> Result<()> {
        self.simple(Req::Commit { conn: self.id })
    }

    fn rollback(&mut self) -> Result<()> {
        self.simple(Req::Rollback { conn: self.id })
    }

    fn ping(&mut self) -> Result<()> {
        self.simple(Req::Ping { conn: self.id })
    }

    fn server_info(&mut self) -> Result<ServerInfo> {
        let (k, b) = self.bridge.call(&Req::ServerInfo { conn: self.id })?;
        match expect(k, &b, 0, self.flavor)? {
            Resp::ServerInfo { product, version } => Ok(ServerInfo { product, version }),
            _ => Err(protocol()),
        }
    }

    fn close(self: Box<Self>) -> Result<()> {
        self.simple(Req::Close { conn: self.id })
    }
}
