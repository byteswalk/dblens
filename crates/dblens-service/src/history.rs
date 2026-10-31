//! 执行历史（M1c1 设计文档第 3、4 节）：每条执行过的语句一条记录，保存在本地库，超出上限时删除最旧的。
//! 写入不阻塞执行：会话把记录交给 `HistoryLog`，由后台线程批量写库。

use std::borrow::Cow;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, SyncSender, TrySendError, sync_channel};

use dblens_core::{Error, Result, msg};
use dblens_dialect::privilege::redact_sql;
use rusqlite::{Row, params, params_from_iter};
use serde::{Deserialize, Serialize};

use crate::store::Store;

/// 设置：值为 `off` 时不记录历史（已有记录保留）。
pub const HISTORY_ENABLED_SETTING: &str = "ui.history.enabled";
/// 设置：保留的条数。
pub const HISTORY_LIMIT_SETTING: &str = "ui.history.limit";
pub const DEFAULT_HISTORY_LIMIT: u64 = 10_000;
const MIN_HISTORY_LIMIT: u64 = 100;
const MAX_HISTORY_LIMIT: u64 = 1_000_000;

/// 一条要写入的历史。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewHistory {
    /// 执行开始时间，Unix 毫秒。
    pub at: i64,
    pub profile_id: String,
    /// 执行时的连接名（连接删除后仍能显示）。
    pub profile_name: String,
    pub driver: String,
    pub variant: Option<String>,
    /// 执行时的当前 schema。
    pub schema: Option<String>,
    /// 这一条语句的原文（不含结束符）。
    pub sql: String,
    pub elapsed_ms: Option<u64>,
    /// 查询取回的行数，或增删改影响的行数；不知道时为 None。
    pub rows: Option<u64>,
    /// 失败、被拒绝或被取消时的错误文字；成功时为 None。
    pub error: Option<String>,
}

/// 查询返回的一条历史。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HistoryItem {
    pub id: i64,
    #[serde(flatten)]
    pub entry: NewHistory,
}

/// 历史查询条件：按时间倒序分页。
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct HistoryQuery {
    /// 在 SQL 中做不区分大小写的包含匹配。
    #[serde(default)]
    pub q: Option<String>,
    /// 只看这个连接的。
    #[serde(default)]
    pub profile: Option<String>,
    /// 只看有错误的（失败、被拒绝、被取消）。
    #[serde(default)]
    pub failed: bool,
    /// 上一页最后一条的 id，返回比它旧的。
    #[serde(default)]
    pub before: Option<i64>,
    /// 每页条数，默认 100，最大 500。
    #[serde(default)]
    pub limit: Option<u32>,
}

const COLUMNS: &str = "at, profile_id, profile_name, driver, variant, schema, sql, elapsed_ms, rows, error";

fn from_row(r: &Row) -> rusqlite::Result<HistoryItem> {
    Ok(HistoryItem {
        id: r.get(0)?,
        entry: NewHistory {
            at: r.get(1)?,
            profile_id: r.get(2)?,
            profile_name: r.get(3)?,
            driver: r.get(4)?,
            variant: r.get(5)?,
            schema: r.get(6)?,
            sql: r.get(7)?,
            elapsed_ms: r.get::<_, Option<i64>>(8)?.map(|v| v as u64),
            rows: r.get::<_, Option<i64>>(9)?.map(|v| v as u64),
            error: r.get(10)?,
        },
    })
}

impl Store {
    /// 写入一批历史，然后删除超出 `limit` 条的最旧记录。
    pub fn add_history(&self, entries: &[NewHistory], limit: u64) -> Result<()> {
        self.with(|c| {
            let tx = c.transaction()?;
            {
                let mut st = tx.prepare(&format!(
                    "INSERT INTO history ({COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)"
                ))?;
                for e in entries {
                    st.execute(params![
                        e.at,
                        e.profile_id,
                        e.profile_name,
                        e.driver,
                        e.variant,
                        e.schema,
                        e.sql,
                        e.elapsed_ms.map(|v| v as i64),
                        e.rows.map(|v| v as i64),
                        e.error
                    ])?;
                }
            }
            tx.execute(
                "DELETE FROM history WHERE id <= (SELECT id FROM history ORDER BY id DESC LIMIT 1 OFFSET ?1)",
                params![limit as i64],
            )?;
            tx.commit()
        })
    }

    pub fn history(&self, q: &HistoryQuery) -> Result<Vec<HistoryItem>> {
        let mut sql = format!("SELECT id, {COLUMNS} FROM history WHERE 1 = 1");
        let mut args: Vec<rusqlite::types::Value> = Vec::new();
        if let Some(text) = q.q.as_deref().filter(|t| !t.is_empty()) {
            args.push(text.to_lowercase().into());
            sql += &format!(" AND instr(lower(sql), ?{}) > 0", args.len());
        }
        if let Some(p) = &q.profile {
            args.push(p.clone().into());
            sql += &format!(" AND profile_id = ?{}", args.len());
        }
        if q.failed {
            sql += " AND error IS NOT NULL";
        }
        if let Some(before) = q.before {
            args.push(before.into());
            sql += &format!(" AND id < ?{}", args.len());
        }
        let limit = q.limit.unwrap_or(100).clamp(1, 500);
        sql += &format!(" ORDER BY id DESC LIMIT {limit}");
        self.with(|c| {
            let mut st = c.prepare(&sql)?;
            st.query_map(params_from_iter(args), from_row)?.collect()
        })
    }

    pub fn delete_history(&self, id: i64) -> Result<()> {
        let n = self.with(|c| c.execute("DELETE FROM history WHERE id = ?1", params![id]))?;
        if n == 0 {
            return Err(Error::not_found(msg!("noun.history", id = id)));
        }
        Ok(())
    }

    pub fn clear_history(&self) -> Result<()> {
        self.with(|c| c.execute("DELETE FROM history", []))?;
        Ok(())
    }

    /// 历史设置：是否记录、保留条数（不合法时用默认值，超出范围时取边界）。
    pub fn history_settings(&self) -> Result<(bool, u64)> {
        let enabled = self.setting(HISTORY_ENABLED_SETTING)?.as_deref() != Some("off");
        let limit = self
            .setting(HISTORY_LIMIT_SETTING)?
            .and_then(|v| v.trim().parse::<u64>().ok())
            .map_or(DEFAULT_HISTORY_LIMIT, |v| v.clamp(MIN_HISTORY_LIMIT, MAX_HISTORY_LIMIT));
        Ok((enabled, limit))
    }
}

/// 写历史的入口，由会话共用。克隆后指向同一个后台写入线程。
#[derive(Clone)]
pub struct HistoryLog {
    inner: Inner,
}

#[derive(Clone)]
enum Inner {
    /// 不记录（测试中不关心历史时用）。
    Off,
    /// 后台线程写入：有界队列，满了丢弃并记日志。
    Thread(SyncSender<NewHistory>),
    /// 直接在调用方写入（测试用，写完立即可查）。
    Inline(Arc<Store>),
}

/// 后台队列的容量。正常情况下写入很快，积压到这个数量说明本地库出了问题。
const QUEUE: usize = 1024;
/// 每批最多写入的条数。
const BATCH: usize = 200;

impl HistoryLog {
    pub fn off() -> Self {
        Self { inner: Inner::Off }
    }

    pub fn inline(store: Arc<Store>) -> Self {
        Self { inner: Inner::Inline(store) }
    }

    /// 启动后台写入线程。所有 `HistoryLog` 都被丢弃后线程退出。
    pub fn start(store: Arc<Store>) -> Self {
        let (tx, rx) = sync_channel(QUEUE);
        std::thread::Builder::new()
            .name("dblens-history".into())
            .spawn(move || writer(&store, &rx))
            .map(|_| Self { inner: Inner::Thread(tx) })
            .unwrap_or_else(|e| {
                tracing::warn!("无法启动历史写入线程，本次运行不记录历史：{e}");
                Self::off()
            })
    }

    /// 记录一条。不阻塞；失败只记日志。账号类语句里的密码在这里脱敏，所有入口都经过这里。
    pub fn record(&self, mut entry: NewHistory) {
        if let Cow::Owned(sql) = redact_sql(&entry.sql) {
            entry.sql = sql;
        }
        if let Some(Cow::Owned(error)) = entry.error.as_deref().map(redact_sql) {
            entry.error = Some(error);
        }
        match &self.inner {
            Inner::Off => {}
            Inner::Thread(tx) => match tx.try_send(entry) {
                Ok(()) => {}
                Err(TrySendError::Full(_)) => tracing::warn!("历史写入队列已满，丢弃一条历史"),
                Err(TrySendError::Disconnected(_)) => tracing::warn!("历史写入线程已退出，丢弃一条历史"),
            },
            Inner::Inline(store) => write_batch(store, &[entry]),
        }
    }
}

fn writer(store: &Store, rx: &Receiver<NewHistory>) {
    while let Ok(first) = rx.recv() {
        let mut batch = vec![first];
        while batch.len() < BATCH {
            match rx.try_recv() {
                Ok(e) => batch.push(e),
                Err(_) => break,
            }
        }
        write_batch(store, &batch);
    }
}

fn write_batch(store: &Store, batch: &[NewHistory]) {
    let result = store
        .history_settings()
        .and_then(|(enabled, limit)| if enabled { store.add_history(batch, limit) } else { Ok(()) });
    if let Err(e) = result {
        tracing::warn!("写入历史失败：{}", e.to_msg().current());
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn typed_account_statements_lose_their_passwords() {
        let store = Arc::new(Store::open_in_memory().unwrap());
        let log = HistoryLog::inline(store.clone());
        log.record(entry(
            "CREATE USER 'u'@'%' IDENTIFIED BY 'S3ntinel'",
            Some("near IDENTIFIED BY 'S3ntinel'"),
        ));
        let items = store.history(&HistoryQuery::default()).unwrap();
        assert_eq!(items[0].entry.sql, "CREATE USER 'u'@'%' IDENTIFIED BY ***");
        assert!(!items[0].entry.error.as_deref().unwrap().contains("S3ntinel"));
    }

    pub(crate) fn entry(sql: &str, error: Option<&str>) -> NewHistory {
        NewHistory {
            at: 1_700_000_000_000,
            profile_id: "p1".into(),
            profile_name: "开发库".into(),
            driver: "postgres".into(),
            variant: Some("postgres".into()),
            schema: Some("public".into()),
            sql: sql.into(),
            elapsed_ms: Some(3),
            rows: Some(1),
            error: error.map(str::to_string),
        }
    }

    fn sqls(items: &[HistoryItem]) -> Vec<&str> {
        items.iter().map(|i| i.entry.sql.as_str()).collect()
    }

    #[test]
    fn newest_first_with_paging() {
        let s = Store::open_in_memory().unwrap();
        let batch: Vec<_> = (1..=5).map(|i| entry(&format!("SELECT {i}"), None)).collect();
        s.add_history(&batch, 100).unwrap();
        let page = s.history(&HistoryQuery { limit: Some(2), ..Default::default() }).unwrap();
        assert_eq!(sqls(&page), ["SELECT 5", "SELECT 4"]);
        let next = s
            .history(&HistoryQuery { before: Some(page[1].id), limit: Some(2), ..Default::default() })
            .unwrap();
        assert_eq!(sqls(&next), ["SELECT 3", "SELECT 2"]);
        assert_eq!(page[0].entry, entry("SELECT 5", None), "字段原样保存");
    }

    #[test]
    fn search_profile_and_failed_filters() {
        let s = Store::open_in_memory().unwrap();
        let mut other = entry("select * from Orders", None);
        other.profile_id = "p2".into();
        s.add_history(
            &[entry("SELECT * FROM emp", None), entry("UPDATE emp SET a = 1", Some("已取消")), other],
            100,
        )
        .unwrap();
        let q = |q: HistoryQuery| {
            sqls(&s.history(&q).unwrap()).into_iter().map(str::to_string).collect::<Vec<_>>()
        };
        assert_eq!(
            q(HistoryQuery { q: Some("ORDERS".into()), ..Default::default() }),
            ["select * from Orders"]
        );
        assert_eq!(q(HistoryQuery { q: Some("emp".into()), ..Default::default() }).len(), 2);
        assert_eq!(
            q(HistoryQuery { profile: Some("p2".into()), ..Default::default() }),
            ["select * from Orders"]
        );
        assert_eq!(q(HistoryQuery { failed: true, ..Default::default() }), ["UPDATE emp SET a = 1"]);
        assert_eq!(
            q(HistoryQuery { q: Some("%".into()), ..Default::default() }),
            Vec::<String>::new(),
            "% 不是通配符"
        );
    }

    #[test]
    fn keeps_only_the_newest_limit() {
        let s = Store::open_in_memory().unwrap();
        for i in 1..=7 {
            s.add_history(&[entry(&format!("S{i}"), None)], 3).unwrap();
        }
        assert_eq!(sqls(&s.history(&HistoryQuery::default()).unwrap()), ["S7", "S6", "S5"]);
    }

    #[test]
    fn delete_and_clear() {
        let s = Store::open_in_memory().unwrap();
        s.add_history(&[entry("A", None), entry("B", None)], 100).unwrap();
        let items = s.history(&HistoryQuery::default()).unwrap();
        s.delete_history(items[0].id).unwrap();
        assert_eq!(sqls(&s.history(&HistoryQuery::default()).unwrap()), ["A"]);
        assert!(matches!(s.delete_history(items[0].id), Err(Error::NotFound { .. })));
        s.clear_history().unwrap();
        assert!(s.history(&HistoryQuery::default()).unwrap().is_empty());
    }

    #[test]
    fn settings_have_defaults_and_bounds() {
        let s = Store::open_in_memory().unwrap();
        assert_eq!(s.history_settings().unwrap(), (true, DEFAULT_HISTORY_LIMIT));
        s.set_setting(HISTORY_LIMIT_SETTING, Some("5")).unwrap();
        s.set_setting(HISTORY_ENABLED_SETTING, Some("off")).unwrap();
        assert_eq!(s.history_settings().unwrap(), (false, MIN_HISTORY_LIMIT));
        s.set_setting(HISTORY_LIMIT_SETTING, Some("abc")).unwrap();
        assert_eq!(s.history_settings().unwrap().1, DEFAULT_HISTORY_LIMIT);
    }

    #[test]
    fn log_respects_enabled_setting() {
        let s = Arc::new(Store::open_in_memory().unwrap());
        let log = HistoryLog::inline(s.clone());
        log.record(entry("A", None));
        s.set_setting(HISTORY_ENABLED_SETTING, Some("off")).unwrap();
        log.record(entry("B", None));
        assert_eq!(sqls(&s.history(&HistoryQuery::default()).unwrap()), ["A"]);
    }

    #[test]
    fn background_thread_writes() {
        let s = Arc::new(Store::open_in_memory().unwrap());
        let log = HistoryLog::start(s.clone());
        for i in 0..50 {
            log.record(entry(&format!("S{i}"), None));
        }
        drop(log);
        for _ in 0..200 {
            if s.history(&HistoryQuery { limit: Some(500), ..Default::default() }).unwrap().len() == 50 {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        panic!("后台线程没有写完");
    }
}
