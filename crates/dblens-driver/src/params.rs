use std::collections::BTreeMap;
use std::fmt;
use std::time::Duration;

use dblens_core::{Error, Result};
use percent_encoding::percent_decode_str;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DriverId {
    Mysql,
    Postgres,
    Oracle,
    Dameng,
}

/// 建立连接所需的参数。
#[derive(Clone, PartialEq, Eq)]
pub struct ConnectParams {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub password: String,
    /// MySQL：默认库；PG：数据库名；Oracle：服务名；达梦：可为空。
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
    /// 支持的 scheme：mysql、mariadb、postgres、postgresql、oracle、dm、dameng。
    /// 选项 `connect_timeout` 的单位为秒。
    pub fn from_url(s: &str) -> Result<(DriverId, ConnectParams)> {
        let u = url::Url::parse(s).map_err(|e| Error::connect(format!("无效的连接 URL：{e}")))?;
        let (id, default_port) = match u.scheme() {
            "mysql" | "mariadb" => (DriverId::Mysql, 3306),
            "postgres" | "postgresql" => (DriverId::Postgres, 5432),
            "oracle" => (DriverId::Oracle, 1521),
            "dm" | "dameng" => (DriverId::Dameng, 5236),
            other => return Err(Error::connect(format!("不支持的连接协议：{other}"))),
        };
        let decode = |v: &str| percent_decode_str(v).decode_utf8_lossy().into_owned();
        let host = u.host_str().ok_or_else(|| Error::connect("连接 URL 缺少主机名"))?.to_string();
        let database = Some(decode(u.path().trim_start_matches('/'))).filter(|d| !d.is_empty());
        let options: BTreeMap<String, String> =
            u.query_pairs().map(|(k, v)| (k.into_owned(), v.into_owned())).collect();
        let connect_timeout = match options.get("connect_timeout") {
            Some(v) => Duration::from_secs(
                v.parse().map_err(|_| Error::connect(format!("connect_timeout 不是有效的秒数：{v}")))?,
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
        assert_eq!(ConnectParams::from_url("dm://SYSDBA:p@h").unwrap().1.port, 5236);
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
