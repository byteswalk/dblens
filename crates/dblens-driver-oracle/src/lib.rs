//! Oracle 驱动：基于 `oracle` crate（ODPI-C），运行时加载 Oracle Instant Client。

mod conn;
mod error;
mod value;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use dblens_core::{Error, Msg, Result, msg};
use dblens_driver::blocking::into_async;
use dblens_driver::native::{self, NativeLib};
use dblens_driver::{CancelHandle, Capabilities, ConnectParams, Connection, Driver, DriverId};

use crate::conn::OracleConnection;
use crate::error::map_err;

pub struct OracleDriver;

/// Oracle 客户端库在进程内只能初始化一次；只记录成功的初始化，失败后可以换目录重试。
static CLIENT_DIR: Mutex<Option<PathBuf>> = Mutex::new(None);

fn init_client(p: &ConnectParams) -> Result<()> {
    let mut guard = CLIENT_DIR
        .lock()
        .map_err(|_| Error::internal(msg!("internal.lock_poisoned", lock = "oracle_client_init")))?;
    let configured = p.options.get("oracle_client_lib_dir").map(PathBuf::from);
    if let Some(loaded) = guard.as_ref() {
        // 进程内无法换用另一个目录的客户端库：明确报错，而不是悄悄沿用已加载的那个
        return match configured {
            Some(dir) if dir != *loaded => Err(Error::invalid(msg!(
                "oracle.client_already_loaded",
                loaded = loaded.display(),
                dir = dir.display()
            ))),
            _ => Ok(()),
        };
    }
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
/// 主机、服务名、SID 中不允许出现会改变连接描述符结构的字符。
fn check_part(what: Msg, v: &str) -> Result<()> {
    if v.is_empty()
        || v.chars()
            .any(|c| matches!(c, '(' | ')' | '=' | '/' | '?' | '@' | ' ' | '"' | '\'') || c.is_control())
    {
        return Err(Error::invalid(msg!("oracle.bad_chars", what = what, value = v)));
    }
    Ok(())
}

pub(crate) fn connect_string(p: &ConnectParams) -> Result<String> {
    // 不足 1 秒的超时按 1 秒处理，避免变成 0（表示不超时）
    let timeout = p.connect_timeout.as_secs().max(1);
    let host = p.host.trim_start_matches('[').trim_end_matches(']');
    if host.contains(':') {
        // IPv6 地址：只允许十六进制数字、冒号和点
        if !host.chars().all(|c| c.is_ascii_hexdigit() || c == ':' || c == '.') {
            return Err(Error::invalid(msg!("oracle.bad_ipv6", host = host)));
        }
    } else {
        check_part(msg!("oracle.part.host"), host)?;
    }
    let addr_host = if host.contains(':') { format!("[{host}]") } else { host.to_string() };
    if let Some(sid) = p.options.get("sid") {
        check_part(Msg::raw("SID"), sid)?;
        return Ok(format!(
            "(DESCRIPTION=(CONNECT_TIMEOUT={timeout})(ADDRESS=(PROTOCOL=TCP)(HOST={host})(PORT={}))(CONNECT_DATA=(SID={sid})))",
            p.port
        ));
    }
    let service = p.database.as_deref().ok_or_else(|| Error::connect(msg!("oracle.service_required")))?;
    check_part(msg!("oracle.part.service"), service)?;
    Ok(format!("//{addr_host}:{}/{service}?connect_timeout={timeout}", p.port))
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
        p.reject_tls("Oracle")?;
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
        .map_err(|e| Error::internal(msg!("internal.task_failed", task = "connect", error = e)))??;
        let conn = Arc::new(conn);
        let cancel = {
            let conn = conn.clone();
            CancelHandle::new(move || {
                let conn = conn.clone();
                async move {
                    tokio::task::spawn_blocking(move || conn.break_execution())
                        .await
                        .map_err(|e| {
                            Error::internal(msg!("internal.task_failed", task = "cancel", error = e))
                        })?
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

    #[test]
    fn connect_string_rejects_injection_and_handles_ipv6_and_short_timeout() {
        let (_, mut p) = ConnectParams::from_url("oracle://u:p@db:1521/FREEPDB1").unwrap();
        p.connect_timeout = std::time::Duration::from_millis(300);
        assert_eq!(connect_string(&p).unwrap(), "//db:1521/FREEPDB1?connect_timeout=1");
        p.host = "::1".into();
        assert_eq!(connect_string(&p).unwrap(), "//[::1]:1521/FREEPDB1?connect_timeout=1");
        p.host = "[fe80::1]".into();
        assert!(connect_string(&p).unwrap().starts_with("//[fe80::1]:1521/"));
        p.host = "fe80::1%eth0".into();
        assert!(matches!(connect_string(&p), Err(Error::Invalid { .. })));
        p.host = "db".into();
        p.options.insert("sid".into(), "ORCL)(HOST=evil".into());
        assert!(matches!(connect_string(&p), Err(Error::Invalid { .. })));
        p.options.clear();
        p.database = Some("svc?x=1".into());
        assert!(matches!(connect_string(&p), Err(Error::Invalid { .. })));
        p.database = Some("svc.example.com".into());
        assert!(connect_string(&p).is_ok());
    }
}
