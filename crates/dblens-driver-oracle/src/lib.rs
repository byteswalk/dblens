//! Oracle 驱动：基于 `oracle` crate（ODPI-C），运行时加载 Oracle Instant Client。

mod conn;
mod error;
mod value;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use dblens_core::{Error, Result};
use dblens_driver::blocking::into_async;
use dblens_driver::native::{self, NativeLib};
use dblens_driver::{CancelHandle, Capabilities, ConnectParams, Connection, Driver, DriverId};

use crate::conn::OracleConnection;
use crate::error::map_err;

pub struct OracleDriver;

/// Oracle 客户端库在进程内只能初始化一次；只记录成功的初始化，失败后可以换目录重试。
static CLIENT_DIR: Mutex<Option<PathBuf>> = Mutex::new(None);

fn init_client(p: &ConnectParams) -> Result<()> {
    let mut guard = CLIENT_DIR.lock().map_err(|_| Error::internal("Oracle 客户端初始化锁已损坏"))?;
    if guard.is_some() {
        return Ok(());
    }
    let configured = p.options.get("oracle_client_lib_dir").map(PathBuf::from);
    let dir = native::locate(NativeLib::OracleClient, configured.as_deref())?;
    oracle::InitParams::new()
        .oracle_client_lib_dir(dir.as_os_str())
        .map_err(map_err)?
        .init()
        .map_err(map_err)?;
    *guard = Some(dir);
    Ok(())
}

/// 生成 Easy Connect 连接串；指定了 `sid` 选项时改用完整描述符。
pub(crate) fn connect_string(p: &ConnectParams) -> Result<String> {
    let timeout = p.connect_timeout.as_secs();
    if let Some(sid) = p.options.get("sid") {
        return Ok(format!(
            "(DESCRIPTION=(CONNECT_TIMEOUT={timeout})(ADDRESS=(PROTOCOL=TCP)(HOST={})(PORT={}))(CONNECT_DATA=(SID={sid})))",
            p.host, p.port
        ));
    }
    let service = p
        .database
        .as_deref()
        .ok_or_else(|| Error::connect("Oracle 连接需要服务名（URL 路径部分）或 sid 选项"))?;
    Ok(format!("//{}:{}/{service}?connect_timeout={timeout}", p.host, p.port))
}

#[async_trait]
impl Driver for OracleDriver {
    fn id(&self) -> DriverId {
        DriverId::Oracle
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            catalogs: false,
            schemas: true,
            transactional_ddl: false,
            cancel: true,
            explain: true,
            multiple_result_sets: false,
        }
    }

    async fn connect(&self, p: &ConnectParams) -> Result<Box<dyn Connection>> {
        let p = p.clone();
        let conn = tokio::task::spawn_blocking(move || -> Result<oracle::Connection> {
            init_client(&p)?;
            let conn = oracle::Connector::new(p.user.as_str(), p.password.as_str(), connect_string(&p)?)
                .connect()
                .map_err(map_err)?;
            conn.execute("BEGIN DBMS_OUTPUT.ENABLE(NULL); END;", &[]).map_err(map_err)?;
            Ok(conn)
        })
        .await
        .map_err(|e| Error::internal(format!("连接任务异常退出：{e}")))??;
        let conn = Arc::new(conn);
        let cancel = {
            let conn = conn.clone();
            CancelHandle::new(move || {
                let conn = conn.clone();
                async move {
                    tokio::task::spawn_blocking(move || conn.break_execution())
                        .await
                        .map_err(|e| Error::internal(format!("取消任务异常退出：{e}")))?
                        .map_err(map_err)
                }
            })
        };
        into_async(OracleConnection::new(conn), cancel, "dblens-oracle")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_connect_strings() {
        let (_, p) = ConnectParams::from_url("oracle://u:p@db:1521/FREEPDB1?connect_timeout=5").unwrap();
        assert_eq!(connect_string(&p).unwrap(), "//db:1521/FREEPDB1?connect_timeout=5");
        let (_, p) = ConnectParams::from_url("oracle://u:p@db:1521?sid=ORCL").unwrap();
        assert!(connect_string(&p).unwrap().contains("(SID=ORCL)"));
        let (_, p) = ConnectParams::from_url("oracle://u:p@db:1521").unwrap();
        assert!(connect_string(&p).is_err());
    }
}
