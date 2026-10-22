use async_trait::async_trait;
use dblens_core::{Result, Value};
use dblens_driver::{CancelHandle, ColumnMeta, Connection, ExecStream, ServerInfo};
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::task::JoinHandle;
use tokio_postgres::{Client, NoTls};

use crate::error::map_err;
use crate::exec::PgExec;
use crate::types::PgParam;

pub struct PgConnection {
    pub(crate) client: Client,
    pub(crate) notices: UnboundedReceiver<String>,
    pub(crate) conn_task: JoinHandle<()>,
    pub(crate) autocommit: bool,
    pub(crate) in_tx: bool,
}

impl PgConnection {
    /// 手动提交模式下，在第一条语句前开启事务。
    async fn ensure_tx(&mut self) -> Result<()> {
        if !self.autocommit && !self.in_tx {
            self.client.batch_execute("BEGIN").await.map_err(map_err)?;
            self.in_tx = true;
        }
        Ok(())
    }

    async fn end_tx(&mut self, sql: &str) -> Result<()> {
        if self.in_tx {
            self.client.batch_execute(sql).await.map_err(map_err)?;
            self.in_tx = false;
        }
        Ok(())
    }
}

#[async_trait]
impl Connection for PgConnection {
    async fn execute<'a>(
        &'a mut self,
        sql: &'a str,
        params: &'a [Value],
    ) -> Result<Box<dyn ExecStream + 'a>> {
        self.ensure_tx().await?;
        let stmt = self.client.prepare(sql).await.map_err(map_err)?;
        let pg_params: Vec<PgParam<'a>> = params.iter().map(PgParam).collect();
        // &PgParam 实现了 ToSql，因而也实现了 BorrowToSql；它是 Send，可以跨 await 持有
        let args = || pg_params.iter();
        if stmt.columns().is_empty() {
            let n = self.client.execute_raw(&stmt, args()).await.map_err(map_err)?;
            Ok(Box::new(PgExec::affected(&mut self.notices, n)))
        } else {
            let columns =
                stmt.columns().iter().map(|c| ColumnMeta::new(c.name(), c.type_().name())).collect();
            let rows = self.client.query_raw(&stmt, args()).await.map_err(map_err)?;
            Ok(Box::new(PgExec::rows(&mut self.notices, columns, rows)))
        }
    }

    fn cancel_handle(&self) -> CancelHandle {
        let token = self.client.cancel_token();
        CancelHandle::new(move || {
            let token = token.clone();
            async move { token.cancel_query(NoTls).await.map_err(map_err) }
        })
    }

    async fn set_autocommit(&mut self, on: bool) -> Result<()> {
        if on {
            self.end_tx("COMMIT").await?;
        }
        self.autocommit = on;
        Ok(())
    }

    async fn commit(&mut self) -> Result<()> {
        self.end_tx("COMMIT").await
    }

    async fn rollback(&mut self) -> Result<()> {
        self.end_tx("ROLLBACK").await
    }

    async fn ping(&mut self) -> Result<()> {
        self.client.batch_execute("SELECT 1").await.map_err(map_err)
    }

    async fn server_info(&mut self) -> Result<ServerInfo> {
        let row = self.client.query_one("SELECT version()", &[]).await.map_err(map_err)?;
        Ok(ServerInfo { product: "PostgreSQL".into(), version: row.get(0) })
    }

    async fn close(self: Box<Self>) -> Result<()> {
        let this = *self;
        drop(this.client);
        // 连接任务在 Client 释放后会自行结束
        let _ = this.conn_task.await;
        Ok(())
    }
}
