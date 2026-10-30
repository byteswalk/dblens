//! 达梦驱动：通过 DPI（运行时加载 dmdpi）访问 DM8。

mod conn;
#[allow(clippy::all)]
mod ffi;
mod handles;
mod load;
mod position;
mod types;

#[cfg(test)]
mod spike;

use std::net::{TcpStream, ToSocketAddrs};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use dblens_core::{Error, Msg, Result};
use dblens_driver::blocking::into_async;
use dblens_driver::{Capabilities, ConnectParams, Connection, Driver, DriverId};

use crate::conn::DmConnection;
use crate::ffi::*;
use crate::handles::{Con, Env, Stmt};

pub struct DmDriver;

/// 登录错误：-2501（用户名或密码错误，用户不存在时也是这个码，实测 DM8）报告为认证失败，其余为连接失败。
/// DPI 登录时只等服务器首包约 1 秒，超过就报 -70019 或 -70028。这个等待不受 LOGIN_TIMEOUT、CONNECTION_TIMEOUT
/// 和 dm_svc.conf 的超时项影响（用不回应的假服务器实测）。经 Wi-Fi 连接时约四分之一的登录会这样失败，
/// 而同时 TCP 是通的。此时还没有登录，重试没有副作用。
const LOGIN_ATTEMPTS: u32 = 5;
const LOGIN_RETRY_DELAY: Duration = Duration::from_millis(300);

/// 直接建一条 TCP 连接，返回结果和耗时，只用于写日志。
fn probe_tcp(p: &ConnectParams) -> String {
    let addr = if p.host.contains(':') { p.host.clone() } else { format!("{}:{}", p.host, p.port) };
    let started = Instant::now();
    let result = addr.to_socket_addrs().map_err(|e| e.to_string()).and_then(|mut addrs| {
        let addr = addrs.next().ok_or_else(|| "地址解析为空".to_string())?;
        TcpStream::connect_timeout(&addr, Duration::from_secs(3)).map_err(|e| e.to_string())
    });
    let ms = started.elapsed().as_millis();
    match result {
        Ok(_) => format!("通（{ms} ms）"),
        Err(e) => format!("不通（{ms} ms）：{e}"),
    }
}

fn is_transient_login_code(code: &str) -> bool {
    matches!(code, "-70019" | "-70028")
}

fn login_error(e: Error) -> Error {
    match e {
        Error::Sql { code, message, .. } if code == "-2501" => {
            Error::Auth { message: Msg::raw(format!("[{code}] {message}")) }
        }
        Error::Sql { code, message, .. } => Error::connect(format!("[{code}] {message}")),
        other => other,
    }
}

fn connect_blocking(p: &ConnectParams) -> Result<DmConnection> {
    p.reject_tls("达梦")?;
    let dpi = load::load(p)?;
    let started = Instant::now();
    tracing::info!(host = %p.host, port = p.port, user = %p.user, timeout_secs = p.connect_timeout.as_secs(), "达梦：开始连接");
    let mut attempt = 1;
    let con = loop {
        // 每个连接使用自己的环境句柄，不在连接之间共享；重试时也换新句柄
        let env = Env::new(dpi.clone())?;
        let mut con = Con::new(env)?;
        // 必须在登录前设置：列名、数据、错误信息因此都是 UTF-8
        con.set_attr_int(DSQL_ATTR_LOCAL_CODE as i64, PG_UTF8 as i64)?;
        con.set_attr_int(DSQL_ATTR_LOGIN_PORT as i64, i64::from(p.port))?;
        con.set_attr_int(DSQL_ATTR_LOGIN_TIMEOUT as i64, p.connect_timeout.as_secs() as i64)?;
        let login_started = Instant::now();
        match con.login(&p.host, &p.user, &p.password) {
            Ok(()) => {
                tracing::info!(attempt, elapsed_ms = started.elapsed().as_millis() as u64, "达梦：连接成功");
                break con;
            }
            Err(e) => {
                let login_ms = login_started.elapsed().as_millis() as u64;
                let transient = matches!(&e, Error::Sql { code, .. } if is_transient_login_code(code));
                // 记下 TCP 探测结果：能区分是网络不通，还是 TCP 通而 DPI 登录失败
                let tcp = if transient { probe_tcp(p) } else { "未探测".to_string() };
                tracing::warn!(attempt, login_ms, tcp = %tcp, error = %e, "达梦：登录失败");
                if !transient || attempt >= LOGIN_ATTEMPTS {
                    return Err(login_error(e));
                }
                attempt += 1;
                std::thread::sleep(LOGIN_RETRY_DELAY);
            }
        }
    };
    con.set_autocommit(true)?;
    // 连接参数里的 database 在达梦中表示模式：登录后切换当前模式。名字按原样（区分大小写）加引号
    if let Some(schema) = p.database.as_deref().filter(|s| !s.is_empty()) {
        Stmt::new(&con)?.exec_direct(&format!("SET SCHEMA \"{}\"", schema.replace('"', "\"\"")))?;
    }
    Ok(DmConnection::new(con))
}

#[async_trait]
impl Driver for DmDriver {
    fn id(&self) -> DriverId {
        DriverId::Dameng
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
        let conn = tokio::task::spawn_blocking(move || connect_blocking(&p))
            .await
            .map_err(|e| Error::internal(format!("连接任务异常退出：{e}")))??;
        let cancel = conn.cancel_handle();
        into_async(conn, cancel, "dblens-dameng")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_network_errors_are_retried_on_login() {
        assert!(is_transient_login_code("-70019") && is_transient_login_code("-70028"));
        // 密码错误不能重试：多试几次可能锁定账号
        assert!(!is_transient_login_code("-2501"));
    }

    #[test]
    fn wrong_password_is_auth_error() {
        let sql = |code: &str| Error::Sql { code: code.into(), message: "m".into(), position: None };
        assert!(
            matches!(login_error(sql("-2501")), Error::Auth { message } if message == Msg::raw("[-2501] m"))
        );
        assert!(matches!(login_error(sql("-70028")), Error::Connect { .. }));
        assert_eq!(login_error(Error::Timeout), Error::Timeout);
    }
}
