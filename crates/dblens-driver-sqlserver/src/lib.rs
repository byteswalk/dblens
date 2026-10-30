//! SQL Server 驱动（tiberius，TDS 协议；M6a 设计文档第 3 节）。
//!
//! tiberius 的取消（`Client::cancel_query`）要独占 `Client`，而结果流在读取期间一直借用它，所以连接由一个后台任务
//! （`actor`）持有：执行、事务、取消都通过消息交给它，结果经有界通道流回，读取方不取时后台任务就等待。
//!
//! 连接选项：
//! - `auth`：`sql`（默认，SQL 账号）、`windows`（指定 Windows 账号，用户名写 `域或机器名\用户`，NTLM）、
//!   `integrated`（当前 Windows 用户）。两种 Windows 认证都只在 Windows 上可用（其他平台的 NTLM 实现 sspi
//!   依赖预发布版本的库，没有采用）；
//! - `instance`：命名实例（如 `SQLEXPRESS`），经 SQL Browser（UDP 1434）查端口，此时不用 `port`；
//! - `tls`：`disable`（只加密登录）、`require`（加密，不校验证书）、`verify`（校验证书，`tls_ca` 可指定 CA）；
//! - `trust_server_certificate`：`true` 时一律不校验证书（同 SSMS 的同名选项）。

mod actor;
mod conn;
mod error;
mod exec;
mod types;

use async_trait::async_trait;
use dblens_core::{Error, Result, msg};
use dblens_driver::{Capabilities, ConnectParams, Connection, Driver, DriverId, TlsMode};
use tiberius::{AuthMethod, Client, Config, EncryptionLevel, SqlBrowser};
use tokio::net::TcpStream;
use tokio_util::compat::{Compat, TokioAsyncWriteCompatExt};

pub use conn::SqlServerConnection;

use crate::error::map_err;

pub(crate) type TdsClient = Client<Compat<TcpStream>>;

pub struct SqlServerDriver;

#[async_trait]
impl Driver for SqlServerDriver {
    fn id(&self) -> DriverId {
        DriverId::SqlServer
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            catalogs: true,
            schemas: true,
            transactional_ddl: true,
            cancel: true,
            explain: true,
            multiple_result_sets: true,
        }
    }

    async fn connect(&self, p: &ConnectParams) -> Result<Box<dyn Connection>> {
        let cfg = config(p)?;
        let named = option(p, "instance").is_some();
        let client = match tokio::time::timeout(p.connect_timeout, open(cfg.clone(), named)).await {
            Err(_) => return Err(Error::Timeout),
            Ok(Err(tiberius::error::Error::Routing { host, port })) => {
                // Azure 等会把连接重定向到另一个地址：按返回的地址重连一次
                let mut cfg = cfg;
                cfg.host(host);
                cfg.port(port);
                tokio::time::timeout(p.connect_timeout, open(cfg, false))
                    .await
                    .map_err(|_| Error::Timeout)?
                    .map_err(map_err)?
            }
            Ok(r) => r.map_err(map_err)?,
        };
        Ok(Box::new(SqlServerConnection::spawn(client)))
    }
}

/// 建立 TCP 连接并登录。`named` 时先经 SQL Browser 查出命名实例的端口。
async fn open(cfg: Config, named: bool) -> tiberius::Result<TdsClient> {
    let tcp = if named {
        TcpStream::connect_named(&cfg).await?
    } else {
        TcpStream::connect(cfg.get_addr()).await?
    };
    tcp.set_nodelay(true)?;
    Client::connect(cfg, tcp.compat_write()).await
}

fn option<'a>(p: &'a ConnectParams, key: &str) -> Option<&'a str> {
    p.options.get(key).map(|v| v.trim()).filter(|v| !v.is_empty())
}

fn config(p: &ConnectParams) -> Result<Config> {
    let mut cfg = Config::new();
    cfg.host(&p.host);
    cfg.application_name("DBLens");
    // 命名实例的端口由 SQL Browser 给出，不设端口（设了会直接连这个端口）
    match option(p, "instance") {
        Some(instance) => cfg.instance_name(instance),
        None => cfg.port(p.port),
    }
    if let Some(db) = p.database.as_deref().filter(|d| !d.is_empty()) {
        cfg.database(db);
    }
    cfg.authentication(match option(p, "auth").unwrap_or("sql") {
        "sql" => AuthMethod::sql_server(&p.user, &p.password),
        #[cfg(windows)]
        "windows" => AuthMethod::windows(&p.user, &p.password),
        #[cfg(windows)]
        "integrated" => AuthMethod::Integrated,
        #[cfg(not(windows))]
        "windows" | "integrated" => return Err(Error::invalid(msg!("sqlserver.windows_auth_only"))),
        other => return Err(Error::invalid(msg!("sqlserver.bad_auth", value = other))),
    });
    let trust = option(p, "trust_server_certificate").is_some_and(|v| v.eq_ignore_ascii_case("true"));
    match p.tls_mode()? {
        // 登录报文总要加密；服务器强制加密时由服务器决定是否加密其余流量
        TlsMode::Disable => {
            cfg.encryption(EncryptionLevel::Off);
            cfg.trust_cert();
        }
        TlsMode::Require => {
            cfg.encryption(EncryptionLevel::Required);
            cfg.trust_cert();
        }
        TlsMode::Verify { ca_file } => {
            cfg.encryption(EncryptionLevel::Required);
            if trust {
                cfg.trust_cert();
            } else if let Some(ca) = ca_file {
                cfg.trust_cert_ca(ca.to_string_lossy());
            }
        }
    }
    Ok(cfg)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    fn params(options: &[(&str, &str)]) -> ConnectParams {
        ConnectParams {
            host: "db.example".into(),
            port: 1433,
            user: "sa".into(),
            password: "p".into(),
            database: Some("app".into()),
            options: options.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect::<BTreeMap<_, _>>(),
            connect_timeout: ConnectParams::DEFAULT_CONNECT_TIMEOUT,
        }
    }

    #[test]
    fn config_from_options() {
        let cfg = config(&params(&[])).unwrap();
        assert_eq!(cfg.get_addr(), "db.example:1433");
        // 命名实例先连 SQL Browser 的 1434 端口
        let cfg = config(&params(&[("instance", "SQLEXPRESS")])).unwrap();
        assert_eq!(cfg.get_addr(), "db.example:1434");
        assert!(config(&params(&[("auth", "kerberos")])).is_err());
        assert!(config(&params(&[("tls", "bogus")])).is_err());
        assert!(config(&params(&[("tls", "verify"), ("trust_server_certificate", "true")])).is_ok());
        for auth in ["windows", "integrated"] {
            assert_eq!(config(&params(&[("auth", auth)])).is_ok(), cfg!(windows), "{auth}");
        }
    }
}
