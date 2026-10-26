use async_trait::async_trait;
use dblens_core::{Error, Result, Value};
use dblens_driver::{CancelHandle, Connection, ExecStream, ServerInfo};
use mysql_async::prelude::Queryable;
use mysql_async::{Conn, Opts};

use crate::error::{map_err, with_position};
use crate::exec::MyExec;
use crate::value::to_mysql_param;

pub struct MySqlConnection {
    pub(crate) conn: Conn,
    pub(crate) opts: Opts,
}

#[async_trait]
impl Connection for MySqlConnection {
    async fn execute<'a>(
        &'a mut self,
        sql: &'a str,
        params: &'a [Value],
    ) -> Result<Box<dyn ExecStream + 'a>> {
        if params.is_empty() {
            // 文本协议，支持一次返回多个结果集（如 CALL）
            let result = self.conn.query_iter(sql).await.map_err(|e| with_position(map_err(e), sql))?;
            Ok(Box::new(MyExec::new(result)))
        } else {
            let values = params.iter().map(to_mysql_param).collect::<Result<Vec<_>>>()?;
            let result =
                self.conn.exec_iter(sql, values).await.map_err(|e| with_position(map_err(e), sql))?;
            Ok(Box::new(MyExec::new(result)))
        }
    }

    /// 另开一个连接执行 `KILL QUERY <线程号>`。
    fn cancel_handle(&self) -> CancelHandle {
        let opts = self.opts.clone();
        let id = self.conn.id();
        CancelHandle::new(move || {
            let opts = opts.clone();
            async move {
                let mut killer = Conn::new(opts).await.map_err(map_err)?;
                killer.query_drop(format!("KILL QUERY {id}")).await.map_err(map_err)?;
                killer.disconnect().await.map_err(map_err)?;
                Ok(())
            }
        })
    }

    async fn set_autocommit(&mut self, on: bool) -> Result<()> {
        let sql = if on { "SET autocommit = 1" } else { "SET autocommit = 0" };
        self.conn.query_drop(sql).await.map_err(map_err)
    }

    async fn commit(&mut self) -> Result<()> {
        self.conn.query_drop("COMMIT").await.map_err(map_err)
    }

    async fn rollback(&mut self) -> Result<()> {
        self.conn.query_drop("ROLLBACK").await.map_err(map_err)
    }

    async fn ping(&mut self) -> Result<()> {
        self.conn.ping().await.map_err(map_err)
    }

    async fn server_info(&mut self) -> Result<ServerInfo> {
        let version: Option<String> = self.conn.query_first("SELECT VERSION()").await.map_err(map_err)?;
        Ok(ServerInfo {
            product: "MySQL".into(),
            version: version.ok_or_else(|| Error::internal("SELECT VERSION() 没有返回结果"))?,
        })
    }

    async fn close(self: Box<Self>) -> Result<()> {
        self.conn.disconnect().await.map_err(map_err)
    }
}
