//! 结构比对（M4a 设计文档第 5 节）：两组表（模型或数据库的一个 schema）配对后逐表比对，
//! 结果描述"怎样修改目标才与源一致"。不做任何 IO。
//!
//! 表的配对：名字相同、调用方给出的改名（模型记下的、用户确认的推测），其余按列名的重合程度推测改名，
//! 推测的改名只作为建议列出，不采用。配上对的表用 M3 的 `diff_table` 比对。

mod apply;
mod normalize;
mod sync;

use std::collections::BTreeMap;

use dblens_core::Result;
use dblens_core::schema::Table;
use serde::{Deserialize, Serialize};

use crate::Dialect;
use crate::design::diff::same_type;
use crate::design::{Renames, TableChange, diff_table};

pub use apply::apply_changes;
pub use sync::{SyncScript, SyncStep, sync_rebuild_tables, sync_script};

/// 已采用的改名：源的名字 → 目标的名字。表的改名按表名，列的改名按（源的）表名分组。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompareRenames {
    #[serde(default)]
    pub tables: BTreeMap<String, String>,
    #[serde(default)]
    pub columns: BTreeMap<String, BTreeMap<String, String>>,
}

/// 比对选项。`case_insensitive`：表名、列名不区分大小写（MySQL 家族的 `lower_case_table_names` 不为 0 时）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompareOptions {
    #[serde(default)]
    pub ignore_comments: bool,
    #[serde(default)]
    pub ignore_column_order: bool,
    #[serde(default = "yes")]
    pub ignore_generated_names: bool,
    #[serde(default)]
    pub case_insensitive: bool,
}

fn yes() -> bool {
    true
}

impl Default for CompareOptions {
    fn default() -> Self {
        Self {
            ignore_comments: false,
            ignore_column_order: false,
            ignore_generated_names: true,
            case_insensitive: false,
        }
    }
}

/// 比对的两边：表和所在 schema 的名字（外键引用本 schema 时按两边各自的名字换算）。
pub struct CompareSide<'a> {
    pub schema: &'a str,
    pub tables: &'a [Table],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TableStatus {
    /// 只有源有：要在目标中新建。
    Added,
    /// 只有目标有：要从目标中删除。
    Removed,
    Renamed,
    Changed,
    Same,
}

/// 一项可以勾选的变更。`id` 在整个比对结果中唯一，勾选和同步都按它。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SchemaItem {
    pub id: u32,
    pub change: SchemaChange,
}

/// 表这一层的变更（建表、删表、改名）和表内的变更（M3 的变更列表）。
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SchemaChange {
    DropTable {
        name: String,
    },
    RenameTable {
        from: String,
        to: String,
    },
    /// 表内的变更；新建表是其中的 `CreateTable`。
    Table {
        change: Box<TableChange>,
    },
}

/// 一张表的比对结果。`source` / `target` 是两边的表名（新增时没有目标，删除时没有源）。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TableDiff {
    pub status: TableStatus,
    pub source: Option<String>,
    pub target: Option<String>,
    pub items: Vec<SchemaItem>,
    /// 新列都在最后、保留的列相对顺序不变（不能指定列位置的库据此判断要不要重建）。
    pub appended: bool,
}

/// 推测的改名：`table` 为 None 时是表的改名，否则是该表（源的名字）中列的改名。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RenameSuggestion {
    pub table: Option<String>,
    pub source: String,
    pub target: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SchemaDiff {
    pub tables: Vec<TableDiff>,
    pub suggestions: Vec<RenameSuggestion>,
}

impl SchemaDiff {
    /// 按编号找变更，并给出它所在的表。
    pub fn item(&self, id: u32) -> Option<(&TableDiff, &SchemaItem)> {
        self.tables.iter().find_map(|t| t.items.iter().find(|i| i.id == id).map(|i| (t, i)))
    }

    /// 有差异的表的数量。
    pub fn changed(&self) -> usize {
        self.tables.iter().filter(|t| t.status != TableStatus::Same).count()
    }
}

/// 推测表改名的门槛：列名的 Jaccard 系数。
const TABLE_SIMILARITY: f64 = 0.7;
/// 推测表改名时两表列数之差的上限。
const TABLE_COLUMN_SLACK: usize = 2;

fn same_name(a: &str, b: &str, opts: &CompareOptions) -> bool {
    if opts.case_insensitive { a.eq_ignore_ascii_case(b) } else { a == b }
}

/// 比对两组表。`renames` 中的名字对不上时返回 `invalid`。
pub fn compare_schemas(
    d: &Dialect,
    source: &CompareSide,
    target: &CompareSide,
    renames: &CompareRenames,
    opts: &CompareOptions,
) -> Result<SchemaDiff> {
    // 表的配对：源的下标 → 目标的下标
    let mut pairs: Vec<(Option<usize>, Option<usize>)> = Vec::new();
    let mut used = vec![false; target.tables.len()];
    let mut renamed = vec![false; source.tables.len()];
    for (si, s) in source.tables.iter().enumerate() {
        let want = renames.tables.get(&s.name).map(|t| (t.as_str(), true));
        let (name, by_rename) = want.unwrap_or((s.name.as_str(), false));
        let found =
            target.tables.iter().enumerate().position(|(ti, t)| !used[ti] && same_name(&t.name, name, opts));
        match found {
            Some(ti) => {
                used[ti] = true;
                renamed[si] = by_rename && !same_name(&s.name, &target.tables[ti].name, opts);
                pairs.push((Some(si), Some(ti)));
            }
            None => pairs.push((Some(si), None)),
        }
    }
    pairs.extend(used.iter().enumerate().filter(|(_, u)| !**u).map(|(ti, _)| (None, Some(ti))));

    let suggestions = suggest_tables(source.tables, target.tables, &pairs, opts);
    let table_map: BTreeMap<String, String> = pairs
        .iter()
        .filter_map(|p| match p {
            (Some(si), Some(ti)) => Some((target.tables[*ti].name.clone(), source.tables[*si].name.clone())),
            _ => None,
        })
        .collect();

    let mut id = 0u32;
    let mut next = |change: SchemaChange| {
        id += 1;
        SchemaItem { id, change }
    };
    let mut tables = Vec::new();
    let mut column_suggestions = Vec::new();
    for (si, ti) in pairs {
        let s = si.map(|i| &source.tables[i]);
        let t = ti.map(|i| &target.tables[i]);
        let diff = match (s, t) {
            (Some(s), None) => {
                let s = normalize::source_only(s, source.schema);
                let table = TableChange::CreateTable { table: s.clone() };
                TableDiff {
                    status: TableStatus::Added,
                    source: Some(s.name.clone()),
                    target: None,
                    items: vec![next(SchemaChange::Table { change: Box::new(table) })],
                    appended: true,
                }
            }
            (None, Some(t)) => TableDiff {
                status: TableStatus::Removed,
                source: None,
                target: Some(t.name.clone()),
                items: vec![next(SchemaChange::DropTable { name: t.name.clone() })],
                appended: true,
            },
            (Some(s), Some(t)) => {
                let cols = renames.columns.get(&s.name).cloned().unwrap_or_default();
                let (s2, t2) =
                    normalize::align(d, s, t, source.schema, target.schema, &table_map, &cols, opts);
                let r = Renames { columns: cols.clone(), indexes: BTreeMap::new() };
                let diff = diff_table(Some(&t2), &s2, &r)?;
                column_suggestions.extend(suggest_columns(&s2, &t2, &diff.changes));
                let is_renamed = renamed[si.expect("配对")];
                let mut items = Vec::new();
                if is_renamed {
                    items.push(next(SchemaChange::RenameTable { from: t.name.clone(), to: s.name.clone() }));
                }
                items.extend(
                    diff.changes
                        .into_iter()
                        .map(|change| next(SchemaChange::Table { change: Box::new(change) })),
                );
                let status = match (is_renamed, items.is_empty()) {
                    (true, _) => TableStatus::Renamed,
                    (false, true) => TableStatus::Same,
                    (false, false) => TableStatus::Changed,
                };
                TableDiff {
                    status,
                    source: Some(s.name.clone()),
                    target: Some(t.name.clone()),
                    items,
                    appended: diff.appended,
                }
            }
            (None, None) => unreachable!("至少一边有表"),
        };
        tables.push(diff);
    }
    let mut suggestions = suggestions;
    suggestions.extend(column_suggestions);
    Ok(SchemaDiff { tables, suggestions })
}

/// 推测表的改名：没配上对的源表和目标表中，列名重合度够高、列数相近的。每张表至多一个建议，取重合度最高的。
fn suggest_tables(
    source: &[Table],
    target: &[Table],
    pairs: &[(Option<usize>, Option<usize>)],
    opts: &CompareOptions,
) -> Vec<RenameSuggestion> {
    let names = |t: &Table| -> Vec<String> {
        t.columns
            .iter()
            .map(|c| if opts.case_insensitive { c.name.to_lowercase() } else { c.name.clone() })
            .collect()
    };
    let only_source: Vec<usize> =
        pairs.iter().filter_map(|p| if let (Some(s), None) = p { Some(*s) } else { None }).collect();
    let only_target: Vec<usize> =
        pairs.iter().filter_map(|p| if let (None, Some(t)) = p { Some(*t) } else { None }).collect();
    let mut out = Vec::new();
    let mut taken = vec![false; target.len()];
    for si in only_source {
        let a = names(&source[si]);
        let best = only_target
            .iter()
            .filter(|ti| !taken[**ti])
            .filter(|ti| a.len().abs_diff(target[**ti].columns.len()) <= TABLE_COLUMN_SLACK)
            .map(|ti| (*ti, jaccard(&a, &names(&target[*ti]))))
            .filter(|(_, j)| *j >= TABLE_SIMILARITY)
            .max_by(|x, y| x.1.total_cmp(&y.1));
        if let Some((ti, _)) = best {
            taken[ti] = true;
            out.push(RenameSuggestion {
                table: None,
                source: source[si].name.clone(),
                target: target[ti].name.clone(),
            });
        }
    }
    out
}

fn jaccard(a: &[String], b: &[String]) -> f64 {
    let both = a.iter().filter(|x| b.contains(x)).count();
    let all = a.len() + b.len() - both;
    if all == 0 { 0.0 } else { both as f64 / all as f64 }
}

/// 推测列的改名：同一张表中删掉的一列（目标）和新增的一列（源），类型相同、位置相差不超过 1。
fn suggest_columns(source: &Table, target: &Table, changes: &[TableChange]) -> Vec<RenameSuggestion> {
    let mut out = Vec::new();
    let mut taken: Vec<&str> = Vec::new();
    for c in changes {
        let TableChange::DropColumn { name } = c else { continue };
        let Some(ti) = target.columns.iter().position(|x| &x.name == name) else { continue };
        let tc = &target.columns[ti];
        let found = changes.iter().find_map(|a| {
            let TableChange::AddColumn { column, .. } = a else { return None };
            let si = source.columns.iter().position(|x| x.name == column.name)?;
            (!taken.contains(&column.name.as_str())
                && si.abs_diff(ti) <= 1
                && same_type(&column.data_type.native, &tc.data_type.native))
            .then_some(column.name.as_str())
        });
        if let Some(to) = found {
            taken.push(to);
            out.push(RenameSuggestion {
                table: Some(source.name.clone()),
                source: to.to_string(),
                target: name.clone(),
            });
        }
    }
    out
}

#[cfg(test)]
mod tests;
