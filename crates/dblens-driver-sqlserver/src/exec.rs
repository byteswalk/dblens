use async_trait::async_trait;
use dblens_core::Result;
use dblens_driver::{ExecEvent, ExecStream};
use tokio::sync::mpsc;

use crate::actor::Item;

/// 执行流：从后台任务的通道读取。连续的行合并成一个 `Rows` 事件：凑满 `max_rows` 行，或遇到结果集结束等
/// 其他事件为止（分批取数据的调用方以不满一批判断结果读完了）。丢弃时通道关闭，后台任务据此通知服务器停下。
pub(crate) struct MsExec {
    rx: mpsc::Receiver<Result<Item>>,
    /// 合并行时多读到的下一项。
    pending: Option<Result<Item>>,
}

impl MsExec {
    pub(crate) fn new(rx: mpsc::Receiver<Result<Item>>) -> Self {
        Self { rx, pending: None }
    }
}

#[async_trait]
impl ExecStream for MsExec {
    async fn next(&mut self, max_rows: usize) -> Result<Option<ExecEvent>> {
        let item = match self.pending.take() {
            Some(item) => item,
            None => match self.rx.recv().await {
                Some(item) => item,
                None => return Ok(None),
            },
        };
        Ok(Some(match item? {
            Item::Start(columns) => ExecEvent::ResultStart { columns },
            Item::End => ExecEvent::ResultEnd,
            Item::Affected(n) => ExecEvent::Affected(n),
            Item::Message(m) => ExecEvent::Message(m),
            Item::Row(first) => {
                let mut rows = vec![first];
                while rows.len() < max_rows.max(1) {
                    match self.rx.recv().await {
                        Some(Ok(Item::Row(r))) => rows.push(r),
                        Some(other) => {
                            self.pending = Some(other);
                            break;
                        }
                        None => break,
                    }
                }
                ExecEvent::Rows(rows)
            }
        }))
    }
}
