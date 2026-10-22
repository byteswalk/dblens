//! PostgreSQL 系驱动（PostgreSQL，以及兼容 PG 协议的人大金仓、openGauss、瀚高等）。

mod conn;
mod error;
mod exec;
mod types;

use async_trait::async_trait;
use dblens_core::Result;
use dblens_driver::{Capabilities, ConnectParams, Connection, Driver, DriverId};
use futures::StreamExt;
use tokio::sync::mpsc;
use tokio_postgres::{AsyncMessage, NoTls};

pub use conn::PgConnection;

use crate::error::map_err;

pub struct PgDriver;

#[async_trait]
impl Driver for PgDriver {
    fn id(&self) -> DriverId {
        DriverId::Postgres
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            catalogs: true,
            schemas: true,
            transactional_ddl: true,
            cancel: true,
            explain: true,
            multiple_result_sets: false,
        }
    }

    async fn connect(&self, p: &ConnectParams) -> Result<Box<dyn Connection>> {
        let mut cfg = tokio_postgres::Config::new();
        cfg.host(&p.host)
            .port(p.port)
            .user(&p.user)
            .password(&p.password)
            .dbname(p.database.as_deref().unwrap_or("postgres"))
            .connect_timeout(p.connect_timeout)
            .application_name("DBLens");
        let (client, mut connection) = cfg.connect(NoTls).await.map_err(map_err)?;

        // 驱动连接任务，同时把服务器发来的 notice 转发给执行流
        let (tx, notices) = mpsc::unbounded_channel();
        let conn_task = tokio::spawn(async move {
            let mut messages = futures::stream::poll_fn(move |cx| connection.poll_message(cx));
            while let Some(message) = messages.next().await {
                match message {
                    Ok(AsyncMessage::Notice(n)) => {
                        let _ = tx.send(format!("{}: {}", n.severity(), n.message()));
                    }
                    Ok(_) => {}
                    Err(_) => break,
                }
            }
        });

        Ok(Box::new(PgConnection { client, notices, conn_task, autocommit: true, in_tx: false }))
    }
}
