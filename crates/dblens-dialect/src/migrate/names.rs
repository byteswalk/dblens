//! 迁移中的名字：大小写转换、长度上限、重名检查。

use std::collections::BTreeMap;

use dblens_core::msg;
use serde::{Deserialize, Serialize};

use super::{MigrationNote, NoteLevel, ObjectKind, SequenceMapping, TableMapping};
use crate::{Dialect, Family};

/// 名字的大小写转换。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NameCase {
    Keep,
    Lower,
    Upper,
}

impl NameCase {
    /// 目标库的习惯：Oracle、达梦折叠为大写，PG 系（含兼容 Oracle 的模式）折叠为小写，MySQL 系、SQL Server 保持。
    pub fn for_target(target: &Dialect) -> NameCase {
        match target.family() {
            Family::Oracle | Family::Dameng => NameCase::Upper,
            Family::Postgres | Family::Informix => NameCase::Lower,
            Family::MySql | Family::SqlServer => NameCase::Keep,
        }
    }

    pub fn apply(self, name: &str) -> String {
        match self {
            NameCase::Keep => name.to_string(),
            NameCase::Lower => name.to_lowercase(),
            NameCase::Upper => name.to_uppercase(),
        }
    }
}

/// 标识符长度上限（MySQL 按字符，其余按字节）：MySQL 64，PG 系 63，Oracle 12.2 起 128、之前 30，达梦、SQL Server 128。
pub fn max_ident_len(target: &Dialect) -> usize {
    match target.family() {
        Family::MySql => 64,
        Family::Postgres => 63,
        Family::Oracle => {
            if target.version().is_some_and(|v| v < 120200) {
                30
            } else {
                128
            }
        }
        Family::Dameng | Family::SqlServer | Family::Informix => 128,
    }
}

fn too_long(target: &Dialect, name: &str) -> bool {
    let len = if target.family() == Family::MySql { name.chars().count() } else { name.len() };
    len > max_ident_len(target)
}

/// 检查重名和超长，结果写成不支持的提示。表和序列在同一 schema 中不能重名；PG、Oracle、达梦的索引名也在 schema 中唯一。
pub(super) fn check(
    target: &Dialect,
    tables: &[TableMapping],
    sequences: &[SequenceMapping],
    notes: &mut Vec<MigrationNote>,
) {
    let note = |kind, object: &str, column: Option<&str>, item: Option<&str>, message| MigrationNote {
        kind,
        object: object.to_string(),
        column: column.map(Into::into),
        item: item.map(Into::into),
        level: NoteLevel::Unsupported,
        message,
    };
    // schema 中的名字（不区分大小写）→ 第一次出现的源对象
    let mut seen: BTreeMap<String, String> = BTreeMap::new();
    let mut claim = |name: &str, owner: &str| -> Option<String> {
        let key = name.to_lowercase();
        match seen.get(&key) {
            Some(first) => Some(first.clone()),
            None => {
                seen.insert(key, owner.to_string());
                None
            }
        }
    };
    let schema_wide_indexes = target.family() != Family::MySql;
    for t in tables {
        let name = &t.target.name;
        if too_long(target, name) {
            notes.push(note(
                ObjectKind::Table,
                &t.source,
                None,
                None,
                msg!("migrate.name_too_long", name = name),
            ));
        }
        if let Some(first) = claim(name, &t.source) {
            notes.push(note(
                ObjectKind::Table,
                &t.source,
                None,
                None,
                msg!("migrate.name_conflict", name = name, other = first),
            ));
        }
        let mut cols: BTreeMap<String, &str> = BTreeMap::new();
        for (m, c) in t.columns.iter().filter_map(|m| m.target.as_ref().map(|c| (m, c))) {
            if too_long(target, c) {
                notes.push(note(
                    ObjectKind::Table,
                    &t.source,
                    Some(&m.source),
                    None,
                    msg!("migrate.name_too_long", name = c),
                ));
            }
            if let Some(first) = cols.insert(c.to_lowercase(), &m.source) {
                notes.push(note(
                    ObjectKind::Table,
                    &t.source,
                    Some(&m.source),
                    None,
                    msg!("migrate.name_conflict", name = c, other = first),
                ));
            }
        }
        let mut items: Vec<&str> = t.target.indexes.iter().map(|i| i.name.as_str()).collect();
        items.extend(t.target.primary_key.iter().filter_map(|k| k.name.as_deref()));
        items.extend(t.target.uniques.iter().filter_map(|k| k.name.as_deref()));
        items.extend(t.target.foreign_keys.iter().filter_map(|k| k.name.as_deref()));
        items.extend(t.target.checks.iter().filter_map(|k| k.name.as_deref()));
        for i in items {
            if too_long(target, i) {
                notes.push(note(
                    ObjectKind::Table,
                    &t.source,
                    None,
                    Some(i),
                    msg!("migrate.name_too_long", name = i),
                ));
            }
        }
        if schema_wide_indexes {
            for i in &t.target.indexes {
                if let Some(first) = claim(&i.name, &t.source) {
                    notes.push(note(
                        ObjectKind::Table,
                        &t.source,
                        None,
                        Some(&i.name),
                        msg!("migrate.name_conflict", name = i.name, other = first),
                    ));
                }
            }
        }
    }
    for s in sequences {
        let name = &s.target.name;
        if too_long(target, name) {
            notes.push(note(
                ObjectKind::Sequence,
                &s.source,
                None,
                None,
                msg!("migrate.name_too_long", name = name),
            ));
        }
        if let Some(first) = claim(name, &s.source) {
            notes.push(note(
                ObjectKind::Sequence,
                &s.source,
                None,
                None,
                msg!("migrate.name_conflict", name = name, other = first),
            ));
        }
    }
}
