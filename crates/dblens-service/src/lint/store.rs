//! 规范库和检查记录的本地存储（表 `lint_rulesets`、`lint_runs`，store 版本 5）。

use dblens_core::{Error, Result, msg};
use dblens_lint::{Overrides, Ruleset};
use rusqlite::{OptionalExtension, params};

use super::{LintRun, LintRunSummary};
use crate::store::Store;

/// 自定义规范的一行。
pub(crate) struct CustomRow {
    pub id: String,
    pub name: String,
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

impl Store {
    /// 内置规范的用户差异；没有改动时为 None。
    pub(crate) fn lint_overrides(&self, id: &str) -> Result<Option<Overrides>> {
        let body: Option<String> = self.with(|c| {
            c.query_row("SELECT body FROM lint_rulesets WHERE id = ?1 AND builtin = 1", params![id], |r| {
                r.get(0)
            })
            .optional()
        })?;
        body.map(|b| serde_json::from_str(&b).map_err(|e| Error::internal(msg!("store.db_error", error = e))))
            .transpose()
    }

    /// 自定义规范的完整内容。
    pub(crate) fn lint_custom(&self, id: &str) -> Result<Option<Ruleset>> {
        let body: Option<String> = self.with(|c| {
            c.query_row("SELECT body FROM lint_rulesets WHERE id = ?1 AND builtin = 0", params![id], |r| {
                r.get(0)
            })
            .optional()
        })?;
        body.map(|b| serde_json::from_str(&b).map_err(|e| Error::internal(msg!("store.db_error", error = e))))
            .transpose()
    }

    /// 全部自定义规范（id 和名字），按名字排序。
    pub(crate) fn lint_customs(&self) -> Result<Vec<CustomRow>> {
        self.with(|c| {
            let mut stmt = c.prepare("SELECT id, name FROM lint_rulesets WHERE builtin = 0 ORDER BY name")?;
            let rows = stmt
                .query_map([], |r| Ok(CustomRow { id: r.get(0)?, name: r.get(1)? }))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })
    }

    /// 哪些内置规范有用户差异。
    pub(crate) fn lint_modified_builtins(&self) -> Result<Vec<String>> {
        self.with(|c| {
            let mut stmt = c.prepare("SELECT id FROM lint_rulesets WHERE builtin = 1")?;
            let rows = stmt.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<Vec<String>>>()?;
            Ok(rows)
        })
    }

    pub(crate) fn save_lint_overrides(&self, id: &str, name: &str, ov: &Overrides) -> Result<()> {
        let body =
            serde_json::to_string(ov).map_err(|e| Error::internal(msg!("store.db_error", error = e)))?;
        self.with(|c| {
            c.execute(
                "INSERT INTO lint_rulesets (id, name, builtin, body, updated_at) VALUES (?1, ?2, 1, ?3, ?4)
                 ON CONFLICT(id) DO UPDATE SET name = ?2, body = ?3, updated_at = ?4",
                params![id, name, body, now_ms()],
            )?;
            Ok(())
        })
    }

    pub(crate) fn save_lint_custom(&self, set: &Ruleset) -> Result<()> {
        let body =
            serde_json::to_string(set).map_err(|e| Error::internal(msg!("store.db_error", error = e)))?;
        self.with(|c| {
            c.execute(
                "INSERT INTO lint_rulesets (id, name, builtin, body, updated_at) VALUES (?1, ?2, 0, ?3, ?4)
                 ON CONFLICT(id) DO UPDATE SET name = ?2, body = ?3, updated_at = ?4",
                params![set.id, set.name, body, now_ms()],
            )?;
            Ok(())
        })
    }

    /// 删除内置规范的用户差异（恢复默认）。
    pub(crate) fn delete_lint_overrides(&self, id: &str) -> Result<()> {
        self.with(|c| {
            c.execute("DELETE FROM lint_rulesets WHERE id = ?1 AND builtin = 1", params![id])?;
            Ok(())
        })
    }

    /// 删除自定义规范。返回是否删到了。
    pub(crate) fn delete_lint_custom(&self, id: &str) -> Result<bool> {
        let n =
            self.with(|c| c.execute("DELETE FROM lint_rulesets WHERE id = ?1 AND builtin = 0", params![id]))?;
        Ok(n > 0)
    }

    pub(crate) fn save_lint_run(&self, run: &LintRun) -> Result<()> {
        let body =
            serde_json::to_string(run).map_err(|e| Error::internal(msg!("store.db_error", error = e)))?;
        self.with(|c| {
            c.execute(
                "INSERT INTO lint_runs (id, created_at, target, ruleset_name, body) VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(id) DO UPDATE SET body = ?5",
                params![run.id, run.created_at, run.target, run.ruleset_name, body],
            )?;
            Ok(())
        })
    }

    pub(crate) fn lint_run_record(&self, id: &str) -> Result<LintRun> {
        let body: Option<String> = self.with(|c| {
            c.query_row("SELECT body FROM lint_runs WHERE id = ?1", params![id], |r| r.get(0)).optional()
        })?;
        let body = body.ok_or_else(|| Error::not_found(msg!("lint.run_missing", id = id)))?;
        serde_json::from_str(&body).map_err(|e| Error::internal(msg!("store.db_error", error = e)))
    }

    pub(crate) fn lint_run_summaries(&self) -> Result<Vec<LintRunSummary>> {
        self.with(|c| {
            let mut stmt = c.prepare(
                "SELECT id, created_at, target, ruleset_name FROM lint_runs ORDER BY created_at DESC",
            )?;
            let rows = stmt
                .query_map([], |r| {
                    Ok(LintRunSummary {
                        id: r.get(0)?,
                        created_at: r.get(1)?,
                        target: r.get(2)?,
                        ruleset_name: r.get(3)?,
                    })
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })
    }

    pub(crate) fn delete_lint_run(&self, id: &str) -> Result<()> {
        self.with(|c| {
            c.execute("DELETE FROM lint_runs WHERE id = ?1", params![id])?;
            Ok(())
        })
    }
}
