//! 三种数据库的 JDBC 适配：URL 拼接、部件查找、连接。

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use dblens_core::{DEFAULT_LOB_PREVIEW_BYTES, Error, Result, msg};
use dblens_driver::blocking::{BlockingConnection, into_async};
use dblens_driver::{Capabilities, ConnectParams, Connection, Driver, DriverId};

use crate::conn::JdbcConnection;
use crate::error::{Flavor, expect, protocol};
use crate::frame::{Req, Resp};
use crate::process::bridge;

/// 这些值拼进 URL，分隔符会变成新的设置项，所以只收普通名字。
fn plain(v: &str, field: &str) -> Result<String> {
    if v.is_empty() || !v.chars().all(|c| c.is_alphanumeric() || "._-".contains(c)) {
        return Err(Error::invalid(msg!("gbase.bad_option", field = field, value = v)));
    }
    Ok(v.to_string())
}

pub(crate) fn jdbc_url(p: &ConnectParams) -> Result<String> {
    let server = p.options.get("server_name").map(String::as_str).unwrap_or("");
    if server.is_empty() {
        return Err(Error::invalid(msg!("gbase.server_name_required")));
    }
    let server = plain(server, "server_name")?;
    let locale = plain(p.options.get("db_locale").map_or("zh_CN.utf8", String::as_str), "db_locale")?;
    let db = plain(p.database.as_deref().unwrap_or("sysmaster"), "database")?;
    Ok(format!(
        "jdbc:gbasedbt-sqli://{}:{}/{db}:GBASEDBTSERVER={server};CLIENT_LOCALE=zh_CN.utf8;DB_LOCALE={locale};SQLMODE=gbase;DELIMIDENT=y",
        p.host, p.port
    ))
}

pub(crate) fn dameng_url(p: &ConnectParams) -> String {
    format!("jdbc:dm://{}:{}", p.host, p.port)
}

pub(crate) fn oracle_url(p: &ConnectParams) -> Result<String> {
    if let Some(sid) = p.options.get("sid").filter(|s| !s.is_empty()) {
        return Ok(format!("jdbc:oracle:thin:@{}:{}:{}", p.host, p.port, plain(sid, "sid")?));
    }
    let service = plain(p.database.as_deref().unwrap_or(""), "database")?;
    Ok(format!("jdbc:oracle:thin:@//{}:{}/{service}", p.host, p.port))
}

/// 程序自带的目录：Windows 和压缩包在程序旁边，deb 在 `../lib/<产品>/`，macOS 的 app 在 `../Resources/`。
fn bundled() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    [dir.join("clients"), dir.join("../lib/dblens/clients"), dir.join("../Resources/clients")]
        .into_iter()
        .find(|d| d.is_dir())
}

struct Parts {
    java: PathBuf,
    jar: PathBuf,
    jdbc_dir: PathBuf,
}

/// 目录里有没有这个驱动的 jar。文件名带版本号，按前缀找。
fn has_jar(dir: &Path, prefix: &str) -> bool {
    std::fs::read_dir(dir).is_ok_and(|entries| {
        entries.flatten().any(|e| {
            let name = e.file_name().to_string_lossy().to_ascii_lowercase();
            name.starts_with(prefix) && name.ends_with(".jar")
        })
    })
}

fn locate(p: &ConnectParams, jar_prefix: &str) -> Result<Parts> {
    let base = bundled();
    let java_name = if cfg!(windows) { "java.exe" } else { "java" };
    let env_path = |k: &str| std::env::var_os(k).map(PathBuf::from);
    let java = env_path("DBLENS_JAVA")
        .or_else(|| base.as_ref().map(|b| b.join("jre/bin").join(java_name)))
        .filter(|f| f.is_file())
        .ok_or_else(|| Error::DriverUnavailable { hint: msg!("jdbc.runtime_missing") })?;
    let jar = env_path("DBLENS_BRIDGE_JAR")
        .or_else(|| base.as_ref().map(|b| b.join("bridge/dblens-bridge.jar")))
        .filter(|f| f.is_file())
        .ok_or_else(|| Error::DriverUnavailable { hint: msg!("jdbc.runtime_missing") })?;
    // 指定目录后只查这一处，和原生客户端的规则相同
    let jdbc_dir = match p.options.get("jdbc_dir").filter(|d| !d.is_empty()) {
        Some(d) if Path::new(d).is_absolute() => PathBuf::from(d),
        Some(d) => {
            return Err(Error::DriverUnavailable {
                hint: msg!("native.dir_not_absolute", lib = "JDBC", dir = d),
            });
        }
        None => base.map(|b| b.join("jdbc")).unwrap_or_default(),
    };
    if !has_jar(&jdbc_dir, jar_prefix) {
        return Err(Error::DriverUnavailable {
            hint: msg!("jdbc.jar_missing", dir = jdbc_dir.display(), jar = jar_prefix),
        });
    }
    Ok(Parts { java, jar, jdbc_dir })
}

/// 一种数据库经 JDBC 连接要用到的东西。
struct Spec {
    class: &'static str,
    jar_prefix: &'static str,
    flavor: Flavor,
    url: String,
}

fn connect_blocking(p: &ConnectParams, spec: Spec) -> Result<JdbcConnection> {
    let parts = locate(p, spec.jar_prefix)?;
    let bridge = bridge(&parts.java, &parts.jar)?;
    let (k, b) = bridge.call(&Req::Connect {
        driver_class: spec.class.into(),
        url: spec.url,
        user: p.user.clone(),
        password: p.password.clone(),
        jar_dir: parts.jdbc_dir.display().to_string(),
        timeout_secs: p.connect_timeout.as_secs() as u32,
    })?;
    match expect(k, &b, 0, spec.flavor) {
        Ok(Resp::Connected(id)) => {
            let mut conn = JdbcConnection { bridge, id, flavor: spec.flavor };
            conn.set_lob_limit(DEFAULT_LOB_PREVIEW_BYTES);
            Ok(conn)
        }
        Ok(_) => Err(protocol()),
        Err(Error::Sql { code, message, .. }) => {
            let code = code.trim_start_matches("ORA-").parse().unwrap_or(0);
            Err(spec.flavor.login_error(code, message))
        }
        Err(e) => Err(e),
    }
}

async fn connect_async(
    p: &ConnectParams,
    make: impl FnOnce(&ConnectParams) -> Result<JdbcConnection> + Send + 'static,
) -> Result<Box<dyn Connection>> {
    let p = p.clone();
    let conn =
        tokio::task::spawn_blocking(move || make(&p)).await.map_err(|e| Error::internal(e.to_string()))??;
    let cancel = conn.cancel_handle();
    into_async(conn, cancel, "dblens-jdbc")
}

pub struct GbaseDriver;

#[async_trait]
impl Driver for GbaseDriver {
    fn id(&self) -> DriverId {
        DriverId::Informix
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            catalogs: true,
            schemas: true,
            transactional_ddl: true,
            cancel: true,
            explain: false,
            multiple_result_sets: false,
        }
    }

    async fn connect(&self, p: &ConnectParams) -> Result<Box<dyn Connection>> {
        connect_async(p, |p| {
            p.reject_tls("GBase 8s")?;
            let spec = Spec {
                class: "com.gbasedbt.jdbc.Driver",
                jar_prefix: "gbasedbtjdbc",
                flavor: Flavor::Gbase,
                url: jdbc_url(p)?,
            };
            connect_blocking(p, spec)
        })
        .await
    }
}

/// 达梦的 JDBC 版：不需要 DPI，所以 macOS 和没装客户端的 Linux 也能用。
pub struct DamengJdbcDriver;

#[async_trait]
impl Driver for DamengJdbcDriver {
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
        connect_async(p, |p| {
            p.reject_tls("DM")?;
            let spec = Spec {
                class: "dm.jdbc.driver.DmDriver",
                jar_prefix: "dmjdbcdriver",
                flavor: Flavor::Dameng,
                url: dameng_url(p),
            };
            let conn = connect_blocking(p, spec)?;
            // 连接参数里的 database 在达梦中表示模式
            if let Some(schema) = p.database.as_deref().filter(|s| !s.is_empty()) {
                conn.run(&format!("SET SCHEMA \"{}\"", schema.replace('"', "\"\"")))?;
            }
            Ok(conn)
        })
        .await
    }
}

/// Oracle 的 JDBC 版：thin 驱动不需要 Instant Client。
pub struct OracleJdbcDriver;

#[async_trait]
impl Driver for OracleJdbcDriver {
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
        connect_async(p, |p| {
            p.reject_tls("Oracle")?;
            let spec = Spec {
                class: "oracle.jdbc.OracleDriver",
                jar_prefix: "ojdbc",
                flavor: Flavor::Oracle,
                url: oracle_url(p)?,
            };
            connect_blocking(p, spec)
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::time::Duration;

    fn params(opts: &[(&str, &str)]) -> ConnectParams {
        ConnectParams {
            host: "h".into(),
            port: 9088,
            user: "u".into(),
            password: "p;w".into(),
            database: Some("db".into()),
            options: opts.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect::<BTreeMap<_, _>>(),
            connect_timeout: Duration::from_secs(10),
        }
    }

    #[test]
    fn url_carries_every_required_setting_but_no_password() {
        let url = jdbc_url(&params(&[("server_name", "gbase01")])).unwrap();
        assert_eq!(
            url,
            "jdbc:gbasedbt-sqli://h:9088/db:GBASEDBTSERVER=gbase01;CLIENT_LOCALE=zh_CN.utf8;DB_LOCALE=zh_CN.utf8;SQLMODE=gbase;DELIMIDENT=y"
        );
    }

    #[test]
    fn db_locale_can_be_overridden() {
        let url = jdbc_url(&params(&[("server_name", "s"), ("db_locale", "en_US.819")])).unwrap();
        assert!(url.contains("DB_LOCALE=en_US.819;"), "{url}");
    }

    #[test]
    fn server_name_is_required() {
        assert!(matches!(jdbc_url(&params(&[])), Err(Error::Invalid { .. })));
    }

    #[test]
    fn option_values_cannot_inject_url_settings() {
        assert!(jdbc_url(&params(&[("server_name", "a;SQLMODE=oracle")])).is_err());
    }

    #[test]
    fn login_failures_are_auth_errors() {
        let f = Flavor::Gbase;
        assert!(matches!(f.login_error(-951, "x".into()), Error::Auth { .. }));
        assert!(matches!(f.login_error(-952, "x".into()), Error::Auth { .. }));
        assert!(matches!(f.login_error(-908, "x".into()), Error::Connect { .. }));
        assert!(matches!(Flavor::Oracle.login_error(1017, "x".into()), Error::Auth { .. }));
        assert!(matches!(Flavor::Dameng.login_error(-2501, "x".into()), Error::Auth { .. }));
    }

    #[test]
    fn oracle_url_uses_service_or_sid() {
        let mut p = params(&[]);
        p.database = Some("FREEPDB1".into());
        assert_eq!(oracle_url(&p).unwrap(), "jdbc:oracle:thin:@//h:9088/FREEPDB1");
        let sid = params(&[("sid", "ORCL")]);
        assert_eq!(oracle_url(&sid).unwrap(), "jdbc:oracle:thin:@h:9088:ORCL");
    }

    #[test]
    fn dameng_url_has_no_database() {
        assert_eq!(dameng_url(&params(&[])), "jdbc:dm://h:9088");
    }
}
