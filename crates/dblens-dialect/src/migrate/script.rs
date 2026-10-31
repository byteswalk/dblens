//! 迁移脚本（M5a 设计文档第 6 节）：第一步建序列、建表（列、主键、注释），删除重建的表先删；
//! 第二步建索引、唯一约束、检查约束，最后建外键。建表和约束的语句复用表设计器的 DDL 生成。

use std::collections::BTreeMap;

use dblens_core::Result;
use dblens_core::schema::{Index, Sequence, Table};
use serde::{Deserialize, Serialize};

use super::MigrationDraft;
use crate::design::{Diff, TableChange};
use crate::{Dialect, Family};

/// 目标中已有同名表时的处理方式。追加数据、清空后导入（M5b）不动结构。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExistingAction {
    Skip,
    Recreate,
    /// 不建表，只建目标表中缺少的索引和约束。
    MissingOnly,
    Append,
    Replace,
}

impl ExistingAction {
    /// 是否迁数据：跳过、只建缺的索引和约束不迁。
    pub fn copies_data(self) -> bool {
        matches!(self, ExistingAction::Recreate | ExistingAction::Append | ExistingAction::Replace)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StatementKind {
    DropTable,
    Sequence,
    CreateTable,
    Comment,
    Index,
    Unique,
    Check,
    ForeignKey,
}

/// 一条迁移语句。`object` 是源对象名；`requires`：这些源表建表失败时跳过本语句。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MigrationStatement {
    pub phase: u8,
    pub kind: StatementKind,
    pub object: String,
    pub requires: Vec<String>,
    pub sql: String,
}

/// 生成迁移脚本。`existing`：目标中已有同名表的处理方式（按源表名，缺省为跳过）；`target_tables`：目标中已有的表。
pub fn migration_script(
    tgt: &Dialect,
    schema: &str,
    draft: &MigrationDraft,
    existing: &BTreeMap<String, ExistingAction>,
    target_tables: &[Table],
) -> Result<Vec<MigrationStatement>> {
    let mut first = Vec::new();
    let mut second = Vec::new();
    let mut fks = Vec::new();
    let stmt = |phase, kind, object: &str, requires: Vec<String>, sql: String| MigrationStatement {
        phase,
        kind,
        object: object.to_string(),
        requires,
        sql,
    };
    for s in draft.sequences.iter().filter(|s| !s.exists) {
        first.push(stmt(1, StatementKind::Sequence, &s.source, vec![], sequence_sql(tgt, schema, &s.target)));
    }
    for t in &draft.tables {
        let action = if t.exists {
            existing.get(&t.source).copied().unwrap_or(ExistingAction::Skip)
        } else {
            ExistingAction::Recreate
        };
        if matches!(action, ExistingAction::Skip | ExistingAction::Append | ExistingAction::Replace) {
            continue;
        }
        let present = target_tables.iter().find(|x| x.name.eq_ignore_ascii_case(&t.target.name));
        let own = vec![t.source.clone()];
        if action == ExistingAction::Recreate {
            if t.exists {
                first.push(stmt(
                    1,
                    StatementKind::DropTable,
                    &t.source,
                    vec![],
                    drop_sql(tgt, schema, &t.target.name),
                ));
            }
            let mut base = Table {
                uniques: vec![],
                foreign_keys: vec![],
                checks: vec![],
                indexes: vec![],
                ..t.target.clone()
            };
            if tgt.family() == Family::SqlServer {
                for c in &mut base.columns {
                    c.default = c.default.take().map(|d| qualify_next_value(tgt, schema, d));
                }
            }
            for (i, s) in
                design(tgt, schema, "", TableChange::CreateTable { table: base })?.into_iter().enumerate()
            {
                let kind = if i == 0 { StatementKind::CreateTable } else { StatementKind::Comment };
                first.push(stmt(1, kind, &t.source, if i == 0 { vec![] } else { own.clone() }, s));
            }
        }
        // 只建缺的：已有的表中有相同列（不区分大小写）的索引、约束不再建
        let missing_only = action == ExistingAction::MissingOnly;
        let cols_of = |i: &Index| i.columns.iter().map(|c| c.name.clone()).collect::<Vec<_>>();
        let (indexes, keys): (Vec<Vec<String>>, Vec<Vec<String>>) = match present.filter(|_| missing_only) {
            Some(p) => (
                p.indexes.iter().map(cols_of).collect(),
                p.uniques
                    .iter()
                    .map(|u| u.columns.clone())
                    .chain(p.indexes.iter().filter(|i| i.unique).map(cols_of))
                    .collect(),
            ),
            None => (vec![], vec![]),
        };
        let has = |list: &[Vec<String>], cols: &[String]| list.iter().any(|x| same_cols(x, cols));
        let name = &t.target.name;
        for x in &t.target.indexes {
            if has(&indexes, &cols_of(x)) {
                continue;
            }
            for s in design(tgt, schema, name, TableChange::AddIndex { index: x.clone() })? {
                second.push(stmt(2, StatementKind::Index, &t.source, own.clone(), s));
            }
        }
        for u in &t.target.uniques {
            if has(&keys, &u.columns) {
                continue;
            }
            for s in design(tgt, schema, name, TableChange::AddUnique { unique: u.clone() })? {
                second.push(stmt(2, StatementKind::Unique, &t.source, own.clone(), s));
            }
        }
        for c in &t.target.checks {
            if missing_only {
                let named = c.name.as_ref().is_some_and(|n| {
                    present.is_some_and(|p| {
                        p.checks.iter().any(|x| x.name.as_ref().is_some_and(|m| m.eq_ignore_ascii_case(n)))
                    })
                });
                // 没有名字的检查约束无法判断是否已有，不建
                if named || c.name.is_none() {
                    continue;
                }
            }
            for s in design(tgt, schema, name, TableChange::AddCheck { check: c.clone() })? {
                second.push(stmt(2, StatementKind::Check, &t.source, own.clone(), s));
            }
        }
        for f in &t.target.foreign_keys {
            let exists = missing_only
                && present.is_some_and(|p| {
                    p.foreign_keys.iter().any(|x| {
                        same_cols(&x.columns, &f.columns) && x.ref_table.eq_ignore_ascii_case(&f.ref_table)
                    })
                });
            if exists {
                continue;
            }
            let mut requires = own.clone();
            if let Some(r) =
                draft.tables.iter().find(|x| x.target.name == f.ref_table && x.source != t.source)
            {
                requires.push(r.source.clone());
            }
            for s in design(tgt, schema, name, TableChange::AddForeignKey { key: f.clone() })? {
                fks.push(stmt(2, StatementKind::ForeignKey, &t.source, requires.clone(), s));
            }
        }
    }
    first.extend(second);
    first.extend(fks);
    Ok(first)
}

/// 脚本全文：每条语句按目标的写法结束（`Family::terminate`），两步之间空一行。
pub fn script_text(target: Family, statements: &[MigrationStatement]) -> String {
    let part = |phase| {
        statements
            .iter()
            .filter(|s| s.phase == phase)
            .map(|s| target.terminate(&s.sql))
            .collect::<Vec<_>>()
            .join("\n")
    };
    [part(1), part(2)].into_iter().filter(|p| !p.is_empty()).collect::<Vec<_>>().join("\n\n")
}

fn same_cols(a: &[String], b: &[String]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.eq_ignore_ascii_case(y))
}

/// 用表设计器的生成得到一项变更的语句（去掉结尾的分号）。
fn design(tgt: &Dialect, schema: &str, table: &str, change: TableChange) -> Result<Vec<String>> {
    let diff = Diff { changes: vec![change], appended: true };
    Ok(tgt
        .design_sql(schema, table, &diff)?
        .into_iter()
        .map(|s| s.sql.trim().trim_end_matches(';').trim_end().to_string())
        .collect())
}

/// SQL Server 的默认值 `NEXT VALUE FOR 序列` 补上 schema：不切换当前 schema，一段名按用户的默认架构解析。
fn qualify_next_value(tgt: &Dialect, schema: &str, default: String) -> String {
    match default.strip_prefix("NEXT VALUE FOR ") {
        Some(seq) if !seq.contains('.') => format!("NEXT VALUE FOR {}.{seq}", tgt.ident(schema)),
        _ => default,
    }
}

fn drop_sql(tgt: &Dialect, schema: &str, name: &str) -> String {
    let q = tgt.qualified(Some(schema), name);
    match tgt.family() {
        Family::MySql | Family::SqlServer | Family::Informix => format!("DROP TABLE {q}"),
        Family::Postgres | Family::Dameng => format!("DROP TABLE {q} CASCADE"),
        Family::Oracle => format!("DROP TABLE {q} CASCADE CONSTRAINTS"),
    }
}

/// 序列：PG 系用 `NO CYCLE`；Oracle、达梦用 `NOCYCLE`，缓存小于 2 时写 `NOCACHE`。
pub(super) fn sequence_sql(tgt: &Dialect, schema: &str, s: &Sequence) -> String {
    if tgt.family() == Family::Postgres {
        return tgt.sequence_ddl(schema, s).trim().trim_end_matches(';').to_string();
    }
    let sqlserver = tgt.family() == Family::SqlServer;
    let mut out = format!("CREATE SEQUENCE {}", tgt.qualified(Some(schema), &s.name));
    if sqlserver {
        out += " AS BIGINT";
    }
    for (label, v) in [
        ("START WITH", &s.start),
        ("INCREMENT BY", &s.increment),
        ("MINVALUE", &s.min_value),
        ("MAXVALUE", &s.max_value),
    ] {
        if let Some(v) = v.as_deref().map(str::trim).filter(|v| !v.is_empty()) {
            // SQL Server 的序列是 BIGINT：超出范围的边界（如 Oracle 的 28 个 9）不写，取类型的边界
            if sqlserver && v.parse::<i64>().is_err() {
                continue;
            }
            out += &format!("\n    {label} {v}");
        }
    }
    // SQL Server 写 NO CACHE / NO CYCLE，Oracle、达梦写 NOCACHE / NOCYCLE
    let no = if sqlserver { "NO " } else { "NO" };
    out += &match s.cache {
        Some(c) if c >= 2 => format!("\n    CACHE {c}"),
        _ => format!("\n    {no}CACHE"),
    };
    out += &if s.cycle { "\n    CYCLE".to_string() } else { format!("\n    {no}CYCLE") };
    out
}
