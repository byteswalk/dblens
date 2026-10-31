use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;
use std::time::Duration;

use dblens_core::{Error, Result, msg};
use percent_encoding::percent_decode_str;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DriverId {
    Mysql,
    Postgres,
    Oracle,
    Dameng,
    #[serde(rename = "sqlserver")]
    SqlServer,
    Informix,
}

/// 建立连接所需的参数。
#[derive(Clone, PartialEq, Eq)]
pub struct ConnectParams {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub password: String,
    /// MySQL：默认库；PG：数据库名；Oracle：服务名；达梦：可为空；SQL Server：登录后的初始库，可为空。
    pub database: Option<String>,
    /// 驱动特有的选项。
    pub options: BTreeMap<String, String>,
    pub connect_timeout: Duration,
}

impl fmt::Debug for ConnectParams {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ConnectParams")
            .field("host", &self.host)
            .field("port", &self.port)
            .field("user", &self.user)
            .field("password", &"***")
            .field("database", &self.database)
            .field("options", &self.options)
            .field("connect_timeout", &self.connect_timeout)
            .finish()
    }
}

impl ConnectParams {
    pub const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

    /// 解析 `scheme://user:pass@host:port/database?k=v`。主要用于测试和命令行。
    /// 支持的 scheme：mysql、mariadb、postgres、postgresql、oracle、dm、dameng、sqlserver、mssql、gbase、informix。
    /// 选项 `connect_timeout` 的单位为秒。
    pub fn from_url(s: &str) -> Result<(DriverId, ConnectParams)> {
        let u = url::Url::parse(s).map_err(|e| Error::connect(msg!("params.invalid_url", error = e)))?;
        let (id, default_port) = match u.scheme() {
            "mysql" | "mariadb" => (DriverId::Mysql, 3306),
            "postgres" | "postgresql" => (DriverId::Postgres, 5432),
            "oracle" => (DriverId::Oracle, 1521),
            "dm" | "dameng" => (DriverId::Dameng, 5236),
            "sqlserver" | "mssql" => (DriverId::SqlServer, 1433),
            "gbase" | "informix" => (DriverId::Informix, 9088),
            other => return Err(Error::connect(msg!("params.unsupported_scheme", scheme = other))),
        };
        let decode = |v: &str| percent_decode_str(v).decode_utf8_lossy().into_owned();
        let host = u.host_str().ok_or_else(|| Error::connect(msg!("params.missing_host")))?.to_string();
        let database = Some(decode(u.path().trim_start_matches('/'))).filter(|d| !d.is_empty());
        let options: BTreeMap<String, String> =
            u.query_pairs().map(|(k, v)| (k.into_owned(), v.into_owned())).collect();
        let connect_timeout = match options.get("connect_timeout") {
            Some(v) => Duration::from_secs(
                v.parse().map_err(|_| Error::connect(msg!("params.invalid_timeout", value = v)))?,
            ),
            None => Self::DEFAULT_CONNECT_TIMEOUT,
        };
        Ok((
            id,
            ConnectParams {
                host,
                port: u.port().unwrap_or(default_port),
                user: decode(u.username()),
                password: decode(u.password().unwrap_or("")),
                database,
                options,
                connect_timeout,
            },
        ))
    }
}

/// 连接的加密方式，来自连接选项 `tls`：
/// - `disable`（默认）：不加密；
/// - `require`：加密，但不校验服务器证书（适合自签名证书的内网库）；
/// - `verify`：加密并校验证书链和主机名；可用选项 `tls_ca` 指定额外信任的 CA 证书文件（PEM）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TlsMode {
    Disable,
    Require,
    Verify { ca_file: Option<PathBuf> },
}

impl ConnectParams {
    pub fn tls_mode(&self) -> Result<TlsMode> {
        let ca_file = self.options.get("tls_ca").filter(|v| !v.is_empty()).map(PathBuf::from);
        match self.options.get("tls").map(|v| v.trim().to_ascii_lowercase()).as_deref() {
            None | Some("" | "disable" | "off" | "false") => Ok(TlsMode::Disable),
            Some("require") => Ok(TlsMode::Require),
            Some("verify") => Ok(TlsMode::Verify { ca_file }),
            Some(other) => Err(Error::invalid(msg!("params.invalid_tls", value = other))),
        }
    }

    /// 不支持 TLS 的驱动调用：要求了加密时报错，而不是悄悄用明文连接。
    pub fn reject_tls(&self, driver: &str) -> Result<()> {
        match self.tls_mode()? {
            TlsMode::Disable => Ok(()),
            _ => Err(Error::invalid(msg!("params.tls_unsupported", driver = driver))),
        }
    }
}

/// 驱动能力，供上层决定界面和行为。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Capabilities {
    /// 是否有 Catalog 层（PG 的 database）。
    pub catalogs: bool,
    pub schemas: bool,
    /// DDL 能否在事务中回滚。
    pub transactional_ddl: bool,
    pub cancel: bool,
    pub explain: bool,
    /// 一次执行能否返回多个结果集（如 MySQL 的 CALL）。
    pub multiple_result_sets: bool,
}

/// 服务器信息。`version` 保存数据库返回的原始版本字符串，供方言层识别兼容库。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerInfo {
    pub product: String,
    pub version: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_tls_option() {
        let tls = |url: &str| ConnectParams::from_url(url).unwrap().1.tls_mode();
        assert_eq!(tls("postgres://u@h/db").unwrap(), TlsMode::Disable);
        assert_eq!(tls("postgres://u@h/db?tls=disable").unwrap(), TlsMode::Disable);
        assert_eq!(tls("postgres://u@h/db?tls=REQUIRE").unwrap(), TlsMode::Require);
        assert_eq!(tls("postgres://u@h/db?tls=verify").unwrap(), TlsMode::Verify { ca_file: None });
        assert_eq!(
            tls("postgres://u@h/db?tls=verify&tls_ca=%2Fetc%2Fca.pem").unwrap(),
            TlsMode::Verify { ca_file: Some(PathBuf::from("/etc/ca.pem")) }
        );
        assert!(matches!(tls("postgres://u@h/db?tls=maybe"), Err(Error::Invalid { .. })));
        let (_, p) = ConnectParams::from_url("oracle://u@h/svc?tls=require").unwrap();
        assert!(matches!(p.reject_tls("Oracle"), Err(Error::Invalid { .. })));
        let (_, p) = ConnectParams::from_url("oracle://u@h/svc").unwrap();
        assert!(p.reject_tls("Oracle").is_ok());
    }

    #[test]
    fn parses_mysql_url_with_encoded_password() {
        let (id, p) =
            ConnectParams::from_url("mysql://root:p%40ss%3Aw@db.local:3307/app?charset=utf8mb4").unwrap();
        assert_eq!(id, DriverId::Mysql);
        assert_eq!(
            (p.host.as_str(), p.port, p.user.as_str(), p.password.as_str()),
            ("db.local", 3307, "root", "p@ss:w")
        );
        assert_eq!(p.database.as_deref(), Some("app"));
        assert_eq!(p.options.get("charset").map(String::as_str), Some("utf8mb4"));
        assert_eq!(p.connect_timeout, ConnectParams::DEFAULT_CONNECT_TIMEOUT);
    }

    #[test]
    fn default_ports_and_empty_database() {
        let (id, p) = ConnectParams::from_url("postgres://u@h").unwrap();
        assert_eq!((id, p.port, p.database), (DriverId::Postgres, 5432, None));
        assert_eq!(ConnectParams::from_url("oracle://u:p@h/ORCLPDB1").unwrap().1.port, 1521);
        assert_eq!(ConnectParams::from_url("dm://SYSDBA:p@h").unwrap().0, DriverId::Dameng);
        let (id, p) = ConnectParams::from_url("mssql://sa:p@h/app?auth=windows").unwrap();
        assert_eq!((id, p.port, p.options["auth"].as_str()), (DriverId::SqlServer, 1433, "windows"));
        assert_eq!(ConnectParams::from_url("dm://SYSDBA:p@h").unwrap().1.port, 5236);
    }

    #[test]
    fn driver_ids_match_web_names() {
        let names: Vec<String> =
            [DriverId::Mysql, DriverId::Postgres, DriverId::Oracle, DriverId::Dameng, DriverId::SqlServer]
                .iter()
                .map(|d| serde_json::to_string(d).unwrap())
                .collect();
        assert_eq!(names, ["\"mysql\"", "\"postgres\"", "\"oracle\"", "\"dameng\"", "\"sqlserver\""]);
    }

    #[test]
    fn connect_timeout_option() {
        let (_, p) = ConnectParams::from_url("mysql://u:p@h/db?connect_timeout=3").unwrap();
        assert_eq!(p.connect_timeout, Duration::from_secs(3));
    }

    #[test]
    fn rejects_unknown_scheme() {
        let err = ConnectParams::from_url("redis://h").unwrap_err();
        assert!(matches!(err, dblens_core::Error::Connect { .. }));
    }

    #[test]
    fn debug_hides_password() {
        let (_, p) = ConnectParams::from_url("mysql://u:secret@h/db").unwrap();
        let s = format!("{p:?}");
        assert!(!s.contains("secret") && s.contains("***"), "{s}");
    }
}
