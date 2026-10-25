//! 达梦驱动：通过 DPI（运行时加载 dmdpi）访问 DM8。

mod conn;
#[allow(clippy::all)]
mod ffi;
mod handles;
mod load;
mod types;

#[cfg(test)]
mod spike;

use async_trait::async_trait;
use dblens_core::{Error, Result};
use dblens_driver::blocking::into_async;
use dblens_driver::{Capabilities, ConnectParams, Connection, Driver, DriverId};

use crate::conn::DmConnection;
use crate::ffi::*;
use crate::handles::{Con, Env, Stmt};

pub struct DmDriver;

/// 登录错误：-2501（用户名或密码错误，用户不存在时也是这个码，实测 DM8）报告为认证失败，其余为连接失败。
fn login_error(e: Error) -> Error {
    match e {
        Error::Sql { code, message, .. } if code == "-2501" => {
            Error::Auth { message: format!("[{code}] {message}") }
        }
        Error::Sql { code, message, .. } => Error::connect(format!("[{code}] {message}")),
        other => other,
    }
}

fn connect_blocking(p: &ConnectParams) -> Result<DmConnection> {
    p.reject_tls("达梦")?;
    let dpi = load::load(p)?;
    // 每个连接使用自己的环境句柄，不在连接之间共享
    let env = Env::new(dpi)?;
    let mut con = Con::new(env)?;
    // 必须在登录前设置：列名、数据、错误信息因此都是 UTF-8
    con.set_attr_int(DSQL_ATTR_LOCAL_CODE as i64, PG_UTF8 as i64)?;
    con.set_attr_int(DSQL_ATTR_LOGIN_PORT as i64, i64::from(p.port))?;
    con.set_attr_int(DSQL_ATTR_LOGIN_TIMEOUT as i64, p.connect_timeout.as_secs() as i64)?;
    con.login(&p.host, &p.user, &p.password).map_err(login_error)?;
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
    fn wrong_password_is_auth_error() {
        let sql = |code: &str| Error::Sql { code: code.into(), message: "m".into(), position: None };
        assert!(matches!(login_error(sql("-2501")), Error::Auth { message } if message == "[-2501] m"));
        assert!(matches!(login_error(sql("-70028")), Error::Connect { .. }));
        assert_eq!(login_error(Error::Timeout), Error::Timeout);
    }
}
