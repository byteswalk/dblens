//! MySQL 系驱动（MySQL、MariaDB，以及兼容 MySQL 协议的 TiDB、OceanBase、PolarDB 等）。

mod conn;
mod error;
mod exec;
mod value;

use async_trait::async_trait;
use dblens_core::{Error, Result};
use dblens_driver::{Capabilities, ConnectParams, Connection, Driver, DriverId};
use mysql_async::{Conn, Opts, OptsBuilder};

pub use conn::MySqlConnection;

use crate::error::map_err;

pub struct MySqlDriver;

#[async_trait]
impl Driver for MySqlDriver {
    fn id(&self) -> DriverId {
        DriverId::Mysql
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            catalogs: false,
            schemas: true,
            transactional_ddl: false,
            cancel: true,
            explain: true,
            multiple_result_sets: true,
        }
    }

    async fn connect(&self, p: &ConnectParams) -> Result<Box<dyn Connection>> {
        let opts: Opts = OptsBuilder::default()
            .ip_or_hostname(p.host.clone())
            .tcp_port(p.port)
            .user(Some(p.user.clone()))
            .pass(Some(p.password.clone()))
            .db_name(p.database.clone())
            .into();
        let conn = tokio::time::timeout(p.connect_timeout, Conn::new(opts.clone()))
            .await
            .map_err(|_| Error::connect(format!("连接超时（{} 秒）", p.connect_timeout.as_secs())))?
            .map_err(map_err)?;
        Ok(Box::new(MySqlConnection { conn, opts }))
    }
}
