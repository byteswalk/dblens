//! 本地数据：用户数据目录下的一个 SQLite 文件（设计文档 4.3 节）。表结构用 `PRAGMA user_version` 记录版本，
//! 启动时依次执行尚未执行的迁移。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use dblens_core::{Error, Result, msg};
use rusqlite::{Connection, OptionalExtension, params};

/// 第 N 个元素把表结构从版本 N 升到 N+1。只能追加，不能修改已发布的迁移。
const MIGRATIONS: &[&str] = &[
    // 版本 1：设置、连接配置、本地加密保存的密码
    "CREATE TABLE settings (
        key   TEXT PRIMARY KEY,
        value TEXT NOT NULL
    );
    CREATE TABLE profiles (
        id                   TEXT PRIMARY KEY,
        name                 TEXT NOT NULL,
        driver               TEXT NOT NULL,
        host                 TEXT NOT NULL,
        port                 INTEGER NOT NULL,
        user                 TEXT NOT NULL,
        database             TEXT,
        options              TEXT NOT NULL,
        environment          TEXT NOT NULL,
        color                TEXT,
        connect_timeout_secs INTEGER NOT NULL,
        created_at           TEXT NOT NULL,
        updated_at           TEXT NOT NULL
    );
    CREATE TABLE vault (
        id           INTEGER PRIMARY KEY CHECK (id = 1),
        salt         BLOB NOT NULL,
        check_nonce  BLOB NOT NULL,
        check_cipher BLOB NOT NULL
    );
    CREATE TABLE secrets (
        profile_id TEXT PRIMARY KEY REFERENCES profiles(id) ON DELETE CASCADE,
        nonce      BLOB NOT NULL,
        cipher     BLOB NOT NULL
    );",
];

pub(crate) fn db_err(e: rusqlite::Error) -> Error {
    Error::internal(msg!("store.db_error", error = e))
}

/// 默认的数据目录：Windows 为 `%APPDATA%\DBLens`，macOS 为 `~/Library/Application Support/DBLens`，
/// Linux 为 `~/.local/share/DBLens`。
pub fn default_data_dir() -> Result<PathBuf> {
    directories::BaseDirs::new()
        .map(|d| d.data_dir().join("DBLens"))
        .ok_or_else(|| Error::internal(msg!("store.no_data_dir")))
}

pub struct Store {
    conn: Mutex<Connection>,
}

impl Store {
    /// 打开（必要时创建）数据目录下的 `dblens.db`，并执行迁移。
    pub fn open(data_dir: &Path) -> Result<Self> {
        std::fs::create_dir_all(data_dir).map_err(|e| {
            Error::internal(msg!("store.create_dir_failed", dir = data_dir.display(), error = e))
        })?;
        Self::init(Connection::open(data_dir.join("dblens.db")).map_err(db_err)?)
    }

    /// 内存数据库，供测试使用。
    pub fn open_in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory().map_err(db_err)?)
    }

    fn init(mut conn: Connection) -> Result<Self> {
        conn.pragma_update(None, "foreign_keys", true).map_err(db_err)?;
        let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0)).map_err(db_err)?;
        let version = usize::try_from(version).unwrap_or(usize::MAX);
        if version > MIGRATIONS.len() {
            return Err(Error::internal(msg!(
                "store.too_new",
                version = version,
                supported = MIGRATIONS.len()
            )));
        }
        for (i, sql) in MIGRATIONS.iter().enumerate().skip(version) {
            let tx = conn.transaction().map_err(db_err)?;
            tx.execute_batch(sql).map_err(db_err)?;
            tx.pragma_update(None, "user_version", i as i64 + 1).map_err(db_err)?;
            tx.commit().map_err(db_err)?;
        }
        Ok(Self { conn: Mutex::new(conn) })
    }

    /// 在数据库连接上执行一段操作。操作都很快，直接持锁执行。
    pub(crate) fn with<T>(&self, f: impl FnOnce(&mut Connection) -> rusqlite::Result<T>) -> Result<T> {
        let mut conn =
            self.conn.lock().map_err(|_| Error::internal(msg!("internal.lock_poisoned", lock = "store")))?;
        f(&mut conn).map_err(db_err)
    }

    pub fn setting(&self, key: &str) -> Result<Option<String>> {
        self.with(|c| {
            c.query_row("SELECT value FROM settings WHERE key = ?1", params![key], |r| r.get(0)).optional()
        })
    }

    /// 全部设置，按键排序。
    pub fn settings(&self) -> Result<BTreeMap<String, String>> {
        self.with(|c| {
            let mut stmt = c.prepare("SELECT key, value FROM settings ORDER BY key")?;
            let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
            rows.collect()
        })
    }

    /// 写入设置；`None` 删除该项。
    pub fn set_setting(&self, key: &str, value: Option<&str>) -> Result<()> {
        self.with(|c| {
            match value {
                Some(v) => c.execute(
                    "INSERT INTO settings (key, value) VALUES (?1, ?2)
                     ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                    params![key, v],
                )?,
                None => c.execute("DELETE FROM settings WHERE key = ?1", params![key])?,
            };
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_round_trip() {
        let s = Store::open_in_memory().unwrap();
        assert_eq!(s.setting("a").unwrap(), None);
        s.set_setting("a", Some("1")).unwrap();
        s.set_setting("a", Some("2")).unwrap();
        assert_eq!(s.setting("a").unwrap().as_deref(), Some("2"));
        s.set_setting("b", Some("x")).unwrap();
        let all = s.settings().unwrap();
        assert_eq!(all.into_iter().collect::<Vec<_>>(), [("a".into(), "2".into()), ("b".into(), "x".into())]);
        s.set_setting("a", None).unwrap();
        assert_eq!(s.setting("a").unwrap(), None);
    }

    #[test]
    fn migrations_run_once_and_survive_reopen() {
        let dir = tempfile::tempdir().unwrap();
        {
            let s = Store::open(dir.path()).unwrap();
            s.set_setting("k", Some("v")).unwrap();
        }
        let s = Store::open(dir.path()).unwrap();
        assert_eq!(s.setting("k").unwrap().as_deref(), Some("v"));
        let version: i64 = s.with(|c| c.pragma_query_value(None, "user_version", |r| r.get(0))).unwrap();
        assert_eq!(version, MIGRATIONS.len() as i64);
    }

    #[test]
    fn newer_database_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        {
            let c = Connection::open(dir.path().join("dblens.db")).unwrap();
            c.pragma_update(None, "user_version", 99).unwrap();
        }
        let err = Store::open(dir.path()).err().unwrap().to_msg().render(dblens_core::Lang::ZhCn);
        assert!(err.contains("升级"), "{err}");
    }

    #[test]
    fn default_data_dir_ends_with_app_name() {
        assert!(default_data_dir().unwrap().ends_with("DBLens"));
    }
}
