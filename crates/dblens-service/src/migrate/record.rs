//! 迁移记录（M5b 设计文档 3.3 节）：方案快照、脚本、每张表的搬运参数和进度，存在数据目录的 `migrations/<id>.json`，
//! 每批提交后更新（先写临时文件再改名）。没有完成的记录在迁移标签中列出，可继续或放弃。

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use dblens_core::schema::{Column, Sequence};
use dblens_core::{Error, Result, Value, msg};
use dblens_dialect::migrate::MigrationStatement;
use serde::{Deserialize, Serialize};

use super::{MigrationPlan, OtherObject};

/// 迁移进行到哪一步。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MigrationPhase {
    Structure,
    Data,
    Constraints,
    Sequences,
    Verify,
    Done,
}

/// 一张表的搬运参数（开始时由迁移草稿算出，续传时不再读结构）。
/// `columns`：读的源列与写的目标列一一对应（目标的生成列不写）；`key`：源主键列在 `columns` 中的位置（为空时不能续传）；
/// `identity`：目标的自增列。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TableCopy {
    pub source: String,
    pub target: String,
    pub source_columns: Vec<String>,
    pub target_columns: Vec<Column>,
    pub key: Vec<usize>,
    pub filter: Option<String>,
    pub identity: Option<String>,
    /// 开始前清空目标表（清空后导入、没有主键的表重迁）。
    pub replace: bool,
    /// 追加到已有数据：校验时加上原有的行数。
    pub append: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TableState {
    Pending,
    Copying,
    Done,
    Failed,
    /// 只迁结构，或建表失败、不迁数据。
    Skipped,
}

/// 抽样比对中不一致的一处。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mismatch {
    pub key: String,
    pub column: String,
    pub source: String,
    pub target: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Verify {
    pub source_rows: u64,
    pub target_rows: u64,
    /// 应有的目标行数（追加时加上原有的行数，减去跳过的行）。
    pub expected_rows: u64,
    pub sampled: u64,
    pub mismatches: Vec<Mismatch>,
}

impl Verify {
    pub fn ok(&self) -> bool {
        self.target_rows == self.expected_rows && self.mismatches.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TableProgress {
    pub state: TableState,
    pub rows: u64,
    pub skipped: u64,
    /// 最后提交的一行的源主键值。
    pub last_key: Option<Vec<Value>>,
    /// 追加前目标表原有的行数。
    pub base_rows: Option<u64>,
    pub estimate: Option<u64>,
    pub error: Option<String>,
    pub elapsed_ms: u64,
    pub verify: Option<Verify>,
}

impl TableProgress {
    pub fn new(state: TableState, estimate: Option<u64>) -> Self {
        Self {
            state,
            rows: 0,
            skipped: 0,
            last_key: None,
            base_rows: None,
            estimate,
            error: None,
            elapsed_ms: 0,
            verify: None,
        }
    }
}

/// 跳过的一行。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkippedRow {
    pub table: String,
    pub key: String,
    pub error: String,
}

/// 序列和自增的调整结果。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Adjustment {
    pub object: String,
    pub sql: String,
    pub error: Option<String>,
}

/// 跳过的行至多记这么多条，超出只计数。
pub const MAX_SKIPPED_ROWS: usize = 1000;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MigrationRecord {
    pub id: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub plan: MigrationPlan,
    /// 两端的显示名（`连接 / schema`）。
    pub source_name: String,
    pub target_name: String,
    pub phase: MigrationPhase,
    pub statements: Vec<MigrationStatement>,
    /// 执行成功的语句（序号）；失败的语句继续时再执行。
    pub done: BTreeSet<usize>,
    /// 失败的语句：序号 → 错误。
    pub statement_errors: BTreeMap<usize, String>,
    /// 建表失败的源表：不迁数据，依赖它的语句跳过。
    pub failed_tables: BTreeSet<String>,
    pub copies: Vec<TableCopy>,
    pub tables: BTreeMap<String, TableProgress>,
    /// 迁移的序列：（源序列名，目标序列）。
    pub sequences: Vec<(String, Sequence)>,
    pub skipped_rows: Vec<SkippedRow>,
    pub adjustments: Vec<Adjustment>,
    /// 映射提示（开始时按界面语言写成文字）。
    pub notes: Vec<String>,
    pub others: Vec<OtherObject>,
    pub finished: bool,
}

pub fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64)
}

/// 未完成的迁移（列表用）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MigrationSummary {
    pub id: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub source: String,
    pub target: String,
    pub phase: MigrationPhase,
    pub tables_done: usize,
    pub tables_total: usize,
    pub running: bool,
}

/// 迁移记录的存放处。`running`：正在执行的记录（不能同时继续两次）。
pub struct MigrationRecords {
    dir: Mutex<PathBuf>,
    running: Mutex<HashSet<String>>,
}

impl Default for MigrationRecords {
    fn default() -> Self {
        Self { dir: Mutex::new(std::env::temp_dir().join("dblens-migrations")), running: Mutex::default() }
    }
}

fn io(e: impl std::fmt::Display) -> Error {
    Error::internal(msg!("migrate.record_io", error = e.to_string()))
}

fn lock_error() -> Error {
    Error::internal(msg!("internal.lock_poisoned", lock = "migrations"))
}

impl MigrationRecords {
    pub fn set_dir(&self, dir: PathBuf) {
        if let Ok(mut d) = self.dir.lock() {
            *d = dir;
        }
    }

    fn path(&self, id: &str) -> Result<PathBuf> {
        // 记录编号只有十六进制字符，不会跑出目录
        if id.is_empty() || !id.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(Error::not_found(msg!("migrate.not_found", id = id)));
        }
        Ok(self.dir.lock().map_err(|_| lock_error())?.join(format!("{id}.json")))
    }

    pub fn save(&self, r: &MigrationRecord) -> Result<()> {
        let path = self.path(&r.id)?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(io)?;
        }
        let tmp = path.with_extension("json.tmp");
        let text = serde_json::to_string(r).map_err(io)?;
        std::fs::write(&tmp, text).map_err(io)?;
        std::fs::rename(&tmp, &path).map_err(io)
    }

    pub fn load(&self, id: &str) -> Result<MigrationRecord> {
        let path = self.path(id)?;
        let text = std::fs::read_to_string(&path)
            .map_err(|_| Error::not_found(msg!("migrate.not_found", id = id)))?;
        serde_json::from_str(&text).map_err(io)
    }

    pub fn delete(&self, id: &str) -> Result<()> {
        if self.is_running(id) {
            return Err(Error::invalid(msg!("migrate.running")));
        }
        let path = self.path(id)?;
        std::fs::remove_file(&path).map_err(|_| Error::not_found(msg!("migrate.not_found", id = id)))
    }

    /// 没有完成的迁移，最近更新的在前。读不出的文件跳过。
    pub fn unfinished(&self) -> Result<Vec<MigrationSummary>> {
        let dir = self.dir.lock().map_err(|_| lock_error())?.clone();
        let Ok(entries) = std::fs::read_dir(&dir) else { return Ok(vec![]) };
        let mut out = Vec::new();
        for e in entries.flatten() {
            let p = e.path();
            if p.extension().is_none_or(|x| x != "json") {
                continue;
            }
            let Some(r) = std::fs::read_to_string(&p)
                .ok()
                .and_then(|t| serde_json::from_str::<MigrationRecord>(&t).ok())
            else {
                continue;
            };
            if r.finished {
                continue;
            }
            out.push(MigrationSummary {
                running: self.is_running(&r.id),
                id: r.id,
                created_at: r.created_at,
                updated_at: r.updated_at,
                source: r.source_name,
                target: r.target_name,
                phase: r.phase,
                tables_done: r
                    .tables
                    .values()
                    .filter(|t| matches!(t.state, TableState::Done | TableState::Skipped))
                    .count(),
                tables_total: r.tables.len(),
            });
        }
        out.sort_by_key(|s| std::cmp::Reverse(s.updated_at));
        Ok(out)
    }

    pub fn is_running(&self, id: &str) -> bool {
        self.running.lock().map(|r| r.contains(id)).unwrap_or(false)
    }

    /// 标记开始执行；已经在执行时返回 false。
    pub fn begin(&self, id: &str) -> bool {
        self.running.lock().map(|mut r| r.insert(id.to_string())).unwrap_or(false)
    }

    pub fn end(&self, id: &str) {
        if let Ok(mut r) = self.running.lock() {
            r.remove(id);
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::migrate::tests::plan;

    pub(crate) fn record(id: &str) -> MigrationRecord {
        MigrationRecord {
            id: id.into(),
            created_at: 1,
            updated_at: 1,
            plan: plan(),
            source_name: "a / s".into(),
            target_name: "b / t".into(),
            phase: MigrationPhase::Data,
            statements: vec![],
            done: BTreeSet::new(),
            statement_errors: BTreeMap::new(),
            failed_tables: BTreeSet::new(),
            copies: vec![],
            tables: BTreeMap::from([
                ("x".to_string(), TableProgress::new(TableState::Done, Some(3))),
                ("y".to_string(), TableProgress::new(TableState::Copying, None)),
            ]),
            sequences: vec![],
            skipped_rows: vec![],
            adjustments: vec![],
            notes: vec![],
            others: vec![],
            finished: false,
        }
    }

    #[test]
    fn records_round_trip_and_list_unfinished() {
        let dir = tempfile::tempdir().unwrap();
        let store = MigrationRecords::default();
        store.set_dir(dir.path().join("migrations"));
        let mut r = record("a1");
        r.tables.get_mut("y").unwrap().last_key = Some(vec![Value::Int(7), Value::String("k".into())]);
        store.save(&r).unwrap();
        assert_eq!(store.load("a1").unwrap(), r);
        let mut done = record("b2");
        done.finished = true;
        store.save(&done).unwrap();
        let list = store.unfinished().unwrap();
        assert_eq!(
            list.iter().map(|s| (s.id.as_str(), s.tables_done, s.tables_total)).collect::<Vec<_>>(),
            [("a1", 1, 2)]
        );
        assert!(store.begin("a1") && !store.begin("a1"));
        assert!(store.delete("a1").is_err());
        store.end("a1");
        store.delete("a1").unwrap();
        assert!(store.load("a1").is_err());
        assert!(store.load("../x").is_err());
    }
}
