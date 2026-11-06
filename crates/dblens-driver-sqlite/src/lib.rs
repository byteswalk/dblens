//! SQLite 驱动：基于 rusqlite（程序自带 SQLite 源码静态编译），每个连接一个专用线程。
//!
//! 连接参数：`database` 是数据库文件的绝对路径；选项 `read_only=1` 只读打开，`create=1` 文件不存在时新建。
//! 主机、端口、用户名、密码不使用。

mod conn;
mod error;
mod value;

use std::path::{Path, PathBuf};
use std::time::Duration;

use async_trait::async_trait;
use dblens_core::{Error, Result, msg};
use dblens_driver::blocking::into_async;
use dblens_driver::{CancelHandle, Capabilities, ConnectParams, Connection, Driver, DriverId};
use rusqlite::OpenFlags;

use crate::conn::SqliteConnection;
use crate::error::map_err;

pub struct SqliteDriver;

/// 别的连接（元数据、另一个会话）正在写时，最多等这么久再报“数据库被锁”。
const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

/// 选项的布尔值：`1`、`true`、`yes`、`on` 为真。
fn flag(p: &ConnectParams, key: &str) -> bool {
    p.options
        .get(key)
        .is_some_and(|v| matches!(v.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on"))
}

/// 连接参数中的数据库文件：必须是绝对路径，不新建时文件必须已存在。
pub(crate) fn file_path(p: &ConnectParams) -> Result<PathBuf> {
    let raw = p.database.as_deref().map(str::trim).filter(|s| !s.is_empty());
    let path = PathBuf::from(raw.ok_or_else(|| Error::invalid(msg!("sqlite.file_required")))?);
    if !path.is_absolute() {
        return Err(Error::invalid(msg!("sqlite.file_not_absolute", file = path.display())));
    }
    if path.is_dir() {
        return Err(Error::invalid(msg!("sqlite.file_is_dir", file = path.display())));
    }
    if !flag(p, "create") && !path.exists() {
        return Err(Error::connect(msg!("sqlite.file_missing", file = path.display())));
    }
    Ok(path)
}

fn open(path: &Path, read_only: bool, create: bool) -> Result<rusqlite::Connection> {
    // 不打开 URI 解析：文件名里的 `?`、`#` 按普通字符处理
    let flags = if read_only {
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX
    } else if create {
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE | OpenFlags::SQLITE_OPEN_NO_MUTEX
    } else {
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX
    };
    let conn = rusqlite::Connection::open_with_flags(path, flags).map_err(map_err)?;
    conn.busy_timeout(BUSY_TIMEOUT).map_err(map_err)?;
    // 打开文件不读内容，不是数据库（或已加密）的文件到第一次读时才报错：这里先读一次
    conn.query_row("SELECT count(*) FROM sqlite_schema", [], |_| Ok(())).map_err(map_err)?;
    conn.execute_batch("PRAGMA foreign_keys = ON").map_err(map_err)?;
    Ok(conn)
}

#[async_trait]
impl Driver for SqliteDriver {
    fn id(&self) -> DriverId {
        DriverId::Sqlite
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            catalogs: false,
            schemas: true,
            transactional_ddl: true,
            cancel: true,
            explain: true,
            multiple_result_sets: false,
        }
    }

    async fn connect(&self, p: &ConnectParams) -> Result<Box<dyn Connection>> {
        p.reject_tls("SQLite")?;
        let path = file_path(p)?;
        let (read_only, create) = (flag(p, "read_only"), flag(p, "create"));
        let conn = tokio::task::spawn_blocking(move || open(&path, read_only, create))
            .await
            .map_err(|e| Error::internal(msg!("internal.task_failed", task = "connect", error = e)))??;
        let interrupt = conn.get_interrupt_handle();
        let cancel = CancelHandle::new(move || {
            interrupt.interrupt();
            async { Ok(()) }
        });
        into_async(SqliteConnection::new(conn), cancel, "dblens-sqlite")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params(database: Option<&str>, options: &[(&str, &str)]) -> ConnectParams {
        ConnectParams {
            host: String::new(),
            port: 0,
            user: String::new(),
            password: String::new(),
            database: database.map(str::to_string),
            options: options.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
            connect_timeout: ConnectParams::DEFAULT_CONNECT_TIMEOUT,
        }
    }

    #[test]
    fn file_path_rules() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("a.db");
        let m = missing.to_str().unwrap();
        assert!(matches!(file_path(&params(None, &[])), Err(Error::Invalid { .. })));
        assert!(matches!(file_path(&params(Some("rel/a.db"), &[])), Err(Error::Invalid { .. })));
        assert!(matches!(file_path(&params(Some(m), &[])), Err(Error::Connect { .. })));
        assert_eq!(file_path(&params(Some(m), &[("create", "1")])).unwrap(), missing);
        let d = dir.path().to_str().unwrap();
        assert!(matches!(file_path(&params(Some(d), &[("create", "1")])), Err(Error::Invalid { .. })));
    }
}
