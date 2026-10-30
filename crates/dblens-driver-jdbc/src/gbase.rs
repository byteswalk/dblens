//! GBase 8s：URL 拼接、部件查找、登录错误。

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use dblens_core::{DEFAULT_LOB_PREVIEW_BYTES, Error, Msg, Result, msg};
use dblens_driver::blocking::{BlockingConnection, into_async};
use dblens_driver::{Capabilities, ConnectParams, Connection, Driver, DriverId};

use crate::conn::JdbcConnection;
use crate::error::{expect, protocol};
use crate::frame::{Req, Resp};
use crate::process::bridge;

const DRIVER_CLASS: &str = "com.gbasedbt.jdbc.Driver";
const JDBC_JAR: &str = "gbasedbtjdbc.jar";

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
    let db = p.database.as_deref().unwrap_or("sysmaster");
    let db = plain(db, "database")?;
    Ok(format!(
        "jdbc:gbasedbt-sqli://{}:{}/{db}:GBASEDBTSERVER={server};CLIENT_LOCALE=zh_CN.utf8;DB_LOCALE={locale};SQLMODE=gbase;DELIMIDENT=y",
        p.host, p.port
    ))
}

pub(crate) fn login_error(code: i32, message: String) -> Error {
    match code {
        -951 | -952 => Error::Auth { message: Msg::raw(format!("[{code}] {message}")) },
        _ => Error::connect(format!("[{code}] {message}")),
    }
}

/// 程序自带的目录：`<程序目录>/clients`。Linux 的 deb 和 macOS 的 app 由打包任务放到同样的相对位置。
fn bundled() -> Option<PathBuf> {
    Some(std::env::current_exe().ok()?.parent()?.join("clients"))
}

struct Parts {
    java: PathBuf,
    jar: PathBuf,
    jdbc_dir: PathBuf,
}

fn locate(p: &ConnectParams) -> Result<Parts> {
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
    if !jdbc_dir.join(JDBC_JAR).is_file() {
        return Err(Error::DriverUnavailable { hint: msg!("gbase.jar_missing", dir = jdbc_dir.display()) });
    }
    Ok(Parts { java, jar, jdbc_dir })
}

fn connect_blocking(p: &ConnectParams) -> Result<JdbcConnection> {
    p.reject_tls("GBase 8s")?;
    let url = jdbc_url(p)?;
    let parts = locate(p)?;
    let bridge = bridge(&parts.java, &parts.jar)?;
    let (k, b) = bridge.call(&Req::Connect {
        driver_class: DRIVER_CLASS.into(),
        url,
        user: p.user.clone(),
        password: p.password.clone(),
        jar_dir: parts.jdbc_dir.display().to_string(),
        timeout_secs: p.connect_timeout.as_secs() as u32,
    })?;
    match expect(k, &b, 0) {
        Ok(Resp::Connected(id)) => {
            let mut conn = JdbcConnection { bridge, id };
            conn.set_lob_limit(DEFAULT_LOB_PREVIEW_BYTES);
            Ok(conn)
        }
        Ok(_) => Err(protocol()),
        Err(Error::Sql { code, message, .. }) => Err(login_error(code.parse().unwrap_or(0), message)),
        Err(e) => Err(e),
    }
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
        let p = p.clone();
        let conn = tokio::task::spawn_blocking(move || connect_blocking(&p))
            .await
            .map_err(|e| Error::internal(e.to_string()))??;
        let cancel = conn.cancel_handle();
        into_async(conn, cancel, "dblens-jdbc")
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
        assert!(matches!(login_error(-951, "x".into()), Error::Auth { .. }));
        assert!(matches!(login_error(-952, "x".into()), Error::Auth { .. }));
        assert!(matches!(login_error(-908, "x".into()), Error::Connect { .. }));
    }
}
