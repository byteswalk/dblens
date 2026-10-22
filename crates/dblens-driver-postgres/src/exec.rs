use std::pin::Pin;

use async_trait::async_trait;
use dblens_core::{DEFAULT_LOB_PREVIEW_BYTES, Result, Row};
use dblens_driver::{ColumnMeta, ExecEvent, ExecStream};
use futures::StreamExt;
use tokio::sync::mpsc::UnboundedReceiver;
use tokio_postgres::RowStream;

use crate::error::map_err;
use crate::types::pg_value;

enum State {
    Affected(u64),
    Start(Vec<ColumnMeta>),
    InRows,
    EndPending,
    Done,
}

/// PG 执行流。每次 `next` 时先把执行期间收到的 notice 作为 Message 发出。
pub(crate) struct PgExec<'a> {
    notices: &'a mut UnboundedReceiver<String>,
    rows: Option<Pin<Box<RowStream>>>,
    state: State,
}

impl<'a> PgExec<'a> {
    pub(crate) fn affected(notices: &'a mut UnboundedReceiver<String>, n: u64) -> Self {
        Self { notices, rows: None, state: State::Affected(n) }
    }

    pub(crate) fn rows(
        notices: &'a mut UnboundedReceiver<String>,
        columns: Vec<ColumnMeta>,
        rows: RowStream,
    ) -> Self {
        Self { notices, rows: Some(Box::pin(rows)), state: State::Start(columns) }
    }
}

fn convert_row(row: &tokio_postgres::Row) -> Result<Row> {
    (0..row.len()).map(|i| pg_value(row, i).map(|v| v.cap_large(DEFAULT_LOB_PREVIEW_BYTES))).collect()
}

#[async_trait]
impl ExecStream for PgExec<'_> {
    async fn next(&mut self, max_rows: usize) -> Result<Option<ExecEvent>> {
        if let Ok(message) = self.notices.try_recv() {
            return Ok(Some(ExecEvent::Message(message)));
        }
        match std::mem::replace(&mut self.state, State::Done) {
            State::Affected(n) => Ok(Some(ExecEvent::Affected(n))),
            State::Start(columns) => {
                self.state = State::InRows;
                Ok(Some(ExecEvent::ResultStart { columns }))
            }
            State::InRows => {
                let stream = self.rows.as_mut().expect("InRows 状态下必有结果流");
                let mut rows = Vec::new();
                let mut ended = false;
                while rows.len() < max_rows.max(1) {
                    match stream.next().await {
                        Some(row) => rows.push(convert_row(&row.map_err(map_err)?)?),
                        None => {
                            ended = true;
                            break;
                        }
                    }
                }
                if rows.is_empty() {
                    Ok(Some(ExecEvent::ResultEnd))
                } else {
                    self.state = if ended { State::EndPending } else { State::InRows };
                    Ok(Some(ExecEvent::Rows(rows)))
                }
            }
            State::EndPending => Ok(Some(ExecEvent::ResultEnd)),
            State::Done => Ok(None),
        }
    }
}
