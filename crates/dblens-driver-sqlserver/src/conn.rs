use std::sync::Arc;

use async_trait::async_trait;
use dblens_core::{DEFAULT_LOB_PREVIEW_BYTES, Error, Result, Value, msg};
use dblens_driver::{CancelHandle, Connection, ExecStream, ServerInfo};
use tokio::sync::{Notify, mpsc, oneshot};
use tokio::task::JoinHandle;

use crate::TdsClient;
use crate::actor::{self, Cmd, Reply};
use crate::exec::MsExec;

/// 一批结果在通道中最多缓存的项数（行、消息等）；读取方不取时后台任务就等待。
const BUFFERED_ITEMS: usize = 256;

pub struct SqlServerConnection {
    cmds: mpsc::Sender<Cmd>,
    cancel: Arc<Notify>,
    task: JoinHandle<()>,
    autocommit: bool,
    in_tx: bool,
    lob_limit: usize,
}

fn gone() -> Error {
    Error::connect(msg!("sqlserver.connection_lost"))
}

impl SqlServerConnection {
    pub(crate) fn spawn(client: TdsClient) -> Self {
        let (cmds, rx) = mpsc::channel(1);
        let cancel = Arc::new(Notify::new());
        let task = tokio::spawn(actor::run(client, rx, cancel.clone()));
        Self { cmds, cancel, task, autocommit: true, in_tx: false, lob_limit: DEFAULT_LOB_PREVIEW_BYTES }
    }

    async fn request<T>(&self, make: impl FnOnce(Reply<T>) -> Cmd) -> Result<T> {
        let (tx, rx) = oneshot::channel();
        self.cmds.send(make(tx)).await.map_err(|_| gone())?;
        rx.await.map_err(|_| gone())?
    }

    async fn batch(&self, sql: &'static str) -> Result<()> {
        self.request(|r| Cmd::Batch(sql, r)).await
    }

    /// 手动提交模式下，在第一批之前开启事务。
    async fn ensure_tx(&mut self) -> Result<()> {
        if !self.autocommit && !self.in_tx {
            self.batch("BEGIN TRANSACTION").await?;
            self.in_tx = true;
        }
        Ok(())
    }

    /// 结束事务。批里可能已经自己提交或回滚了，所以先看 `@@TRANCOUNT`。
    async fn end_tx(&mut self, sql: &'static str) -> Result<()> {
        if self.in_tx {
            self.batch(sql).await?;
            self.in_tx = false;
        }
        Ok(())
    }
}

#[async_trait]
impl Connection for SqlServerConnection {
    async fn execute<'a>(
        &'a mut self,
        sql: &'a str,
        params: &'a [Value],
    ) -> Result<Box<dyn ExecStream + 'a>> {
        self.ensure_tx().await?;
        let (out, rx) = mpsc::channel(BUFFERED_ITEMS);
        let cmd = Cmd::Exec { sql: sql.to_string(), params: params.to_vec(), lob_limit: self.lob_limit, out };
        self.cmds.send(cmd).await.map_err(|_| gone())?;
        Ok(Box::new(MsExec::new(rx)))
    }

    fn cancel_handle(&self) -> CancelHandle {
        let cancel = self.cancel.clone();
        CancelHandle::new(move || {
            // 只通知正在执行的那一批；没有在执行时什么也不做
            cancel.notify_waiters();
            async { Ok(()) }
        })
    }

    fn set_lob_limit(&mut self, bytes: usize) {
        self.lob_limit = bytes;
    }

    async fn set_autocommit(&mut self, on: bool) -> Result<()> {
        if on {
            self.end_tx("IF @@TRANCOUNT > 0 COMMIT TRANSACTION").await?;
        }
        self.autocommit = on;
        Ok(())
    }

    async fn commit(&mut self) -> Result<()> {
        self.end_tx("IF @@TRANCOUNT > 0 COMMIT TRANSACTION").await
    }

    async fn rollback(&mut self) -> Result<()> {
        self.end_tx("IF @@TRANCOUNT > 0 ROLLBACK TRANSACTION").await
    }

    async fn ping(&mut self) -> Result<()> {
        self.batch("SELECT 1").await
    }

    async fn server_info(&mut self) -> Result<ServerInfo> {
        let version = self.request(Cmd::Version).await?;
        Ok(ServerInfo { product: "SQL Server".into(), version })
    }

    async fn close(self: Box<Self>) -> Result<()> {
        let this = *self;
        let (tx, rx) = oneshot::channel();
        let sent = this.cmds.send(Cmd::Close(tx)).await.is_ok();
        let result = if sent { rx.await.unwrap_or(Ok(())) } else { Ok(()) };
        let _ = this.task.await;
        result
    }
}
