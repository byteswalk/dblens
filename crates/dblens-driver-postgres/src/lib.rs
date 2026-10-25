//! PostgreSQL 系驱动（PostgreSQL，以及兼容 PG 协议的人大金仓、openGauss、瀚高等）。

mod conn;
mod error;
mod exec;
mod tls;
mod types;

use async_trait::async_trait;
use dblens_core::Result;
use dblens_driver::{Capabilities, ConnectParams, Connection, Driver, DriverId};
use futures::StreamExt;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::mpsc::{self, UnboundedReceiver};
use tokio::task::JoinHandle;
use tokio_postgres::config::SslMode;
use tokio_postgres::{AsyncMessage, NoTls};

pub use conn::PgConnection;

use crate::error::map_err;

pub struct PgDriver;

/// 驱动连接任务，同时把服务器发来的 notice 转发给执行流。加密与否时连接的类型不同，所以写成泛型。
fn spawn_connection<S, T>(
    mut connection: tokio_postgres::Connection<S, T>,
) -> (UnboundedReceiver<String>, JoinHandle<()>)
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    T: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
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
    (notices, conn_task)
}

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
        let tls = tls::connector(&p.tls_mode()?)?;
        cfg.ssl_mode(if tls.is_some() { SslMode::Require } else { SslMode::Disable });
        let (client, (notices, conn_task)) = match &tls {
            None => {
                let (client, connection) = cfg.connect(NoTls).await.map_err(map_err)?;
                (client, spawn_connection(connection))
            }
            Some(t) => {
                let (client, connection) = cfg.connect(t.clone()).await.map_err(map_err)?;
                (client, spawn_connection(connection))
            }
        };
        Ok(Box::new(PgConnection { client, notices, conn_task, autocommit: true, in_tx: false, tls }))
    }
}
