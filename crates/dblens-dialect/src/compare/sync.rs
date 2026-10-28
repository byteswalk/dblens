//! 同步脚本（M4a 设计文档 6.1 节）：按勾选的变更修改目标 schema。
//! 顺序：删外键 → 删表 → 表改名 → 逐表修改（不支持的走 M3 重建）→ 建表（不带外键）→ 加外键。
//!
//! 要重建的表先用 `sync_rebuild_tables` 找出来，调用方读取它们的重建依赖信息后传给 `sync_script`。

use std::collections::{BTreeMap, BTreeSet};

use dblens_core::schema::Table;
use dblens_core::{Msg, Result};
use serde::Serialize;

use super::{SchemaChange, SchemaDiff, TableDiff, apply_changes};
use crate::Dialect;
use crate::design::{Diff, Rebuild, RebuildContext, RebuildOutcome, Renames, TableChange as C};

/// 脚本的一段：一组语句，或一张表的重建。`items` 是这一段对应的变更编号。
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SyncStep {
    Sql { sql: String, items: Vec<u32> },
    Rebuild { table: String, rebuild: Rebuild, items: Vec<u32> },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SyncScript {
    pub steps: Vec<SyncStep>,
    /// 不能重建的表及原因（这些表的变更没有生成语句）。
    pub refused: Vec<(String, Msg)>,
    pub warnings: Vec<Msg>,
    pub transactional: bool,
}

/// 勾选的变更：（编号，变更）。
type Picks<'a> = Vec<(u32, &'a C)>;

/// 一张表中被勾选的变更（不含表这一层的建、删、改名）。
fn picked<'a>(t: &'a TableDiff, selected: &BTreeSet<u32>) -> Picks<'a> {
    t.items
        .iter()
        .filter(|i| selected.contains(&i.id))
        .filter_map(|i| match &i.change {
            SchemaChange::Table { change } => Some((i.id, change.as_ref())),
            _ => None,
        })
        .collect()
}

fn is_fk(c: &C) -> bool {
    matches!(c, C::DropForeignKey { .. } | C::AddForeignKey { .. })
}

/// 勾选的变更要不要重建：库不支持（M3 的 `design_unsupported`）时重建。
fn needs_rebuild(d: &Dialect, t: &TableDiff, changes: &[C]) -> bool {
    if changes.is_empty() || matches!(changes.first(), Some(C::CreateTable { .. })) {
        return false;
    }
    let reorder = changes.iter().any(|c| matches!(c, C::MoveColumn { .. } | C::AddColumn { .. }));
    let diff = Diff { changes: changes.to_vec(), appended: !reorder || t.appended };
    !d.design_unsupported(&diff).is_empty()
}

/// 勾选的变更中要重建的表（目标中的名字，改名后的名字）。
pub fn sync_rebuild_tables(d: &Dialect, diff: &SchemaDiff, selected: &BTreeSet<u32>) -> Vec<String> {
    diff.tables
        .iter()
        .filter(|t| t.target.is_some() && t.source.is_some())
        .filter(|t| {
            let changes: Vec<C> = picked(t, selected).into_iter().map(|(_, c)| c.clone()).collect();
            needs_rebuild(d, t, &changes)
        })
        .filter_map(|t| t.source.clone())
        .collect()
}

/// 生成同步脚本。`targets`：目标 schema 中的表（按目标的名字）；`contexts`：要重建的表的依赖信息
/// （按改名后的名字，见 `sync_rebuild_tables`）。
pub fn sync_script(
    d: &Dialect,
    schema: &str,
    diff: &SchemaDiff,
    selected: &BTreeSet<u32>,
    targets: &[Table],
    contexts: &BTreeMap<String, RebuildContext>,
) -> Result<SyncScript> {
    let mut drop_fks = Vec::new();
    let mut drop_tables = Vec::new();
    let mut renames = Vec::new();
    let mut alters = Vec::new();
    let mut creates = Vec::new();
    let mut add_fks = Vec::new();
    let mut refused = Vec::new();
    let mut warnings = Vec::new();
    let q = |name: &str| d.qualified(Some(schema), name);
    let sql_of = |table: &str, changes: Vec<C>| -> Result<Vec<String>> {
        let diff = Diff { changes, appended: true };
        Ok(d.design_sql(schema, table, &diff)?.into_iter().map(|s| s.sql).collect())
    };

    for t in &diff.tables {
        for item in t.items.iter().filter(|i| selected.contains(&i.id)) {
            match &item.change {
                SchemaChange::DropTable { name } => drop_tables
                    .push(SyncStep::Sql { sql: format!("DROP TABLE {}", q(name)), items: vec![item.id] }),
                SchemaChange::RenameTable { from, to } => renames.push(SyncStep::Sql {
                    sql: format!("ALTER TABLE {} RENAME TO {}", q(from), d.ident(to)),
                    items: vec![item.id],
                }),
                SchemaChange::Table { change } if matches!(change.as_ref(), C::CreateTable { .. }) => {
                    let C::CreateTable { table } = change.as_ref() else { unreachable!() };
                    // 外键在所有表建好之后再加
                    let bare = Table { foreign_keys: vec![], ..table.clone() };
                    for sql in sql_of(&table.name, vec![C::CreateTable { table: bare }])? {
                        creates.push(SyncStep::Sql { sql, items: vec![item.id] });
                    }
                    for key in &table.foreign_keys {
                        for sql in sql_of(&table.name, vec![C::AddForeignKey { key: key.clone() }])? {
                            add_fks.push(SyncStep::Sql { sql, items: vec![item.id] });
                        }
                    }
                }
                SchemaChange::Table { .. } => {}
            }
        }
        let (Some(name), Some(target)) = (&t.source, &t.target) else { continue };
        let picks = picked(t, selected);
        if picks.is_empty() {
            continue;
        }
        let changes: Vec<C> = picks.iter().map(|(_, c)| (*c).clone()).collect();
        let ids: Vec<u32> = picks.iter().map(|(id, _)| *id).collect();
        if needs_rebuild(d, t, &changes) {
            let original = targets.iter().find(|x| &x.name == target).cloned().unwrap_or_default();
            let original = Table { name: name.clone(), ..original };
            let draft = apply_changes(&original, &changes)?;
            let column_renames: BTreeMap<String, String> = changes
                .iter()
                .filter_map(|c| match c {
                    C::RenameColumn { from, to, .. } => Some((to.clone(), from.clone())),
                    _ => None,
                })
                .collect();
            let r = Renames { columns: column_renames, indexes: BTreeMap::new() };
            let ctx = contexts.get(name).cloned().unwrap_or_default();
            match d.rebuild(schema, &original, &draft, &r, &ctx)? {
                RebuildOutcome::Script(rebuild) => {
                    warnings.extend(rebuild.warnings.iter().cloned());
                    alters.push(SyncStep::Rebuild { table: name.clone(), rebuild, items: ids });
                }
                RebuildOutcome::Refused { reason } => refused.push((name.clone(), reason)),
            }
            continue;
        }
        // 一张表的改动一起生成（MySQL 换主键、既修改又挪动的列要合成一条）；外键分出去放在前后两段
        let groups: [(&mut Vec<SyncStep>, Picks); 3] = [
            (
                &mut drop_fks,
                picks.iter().filter(|(_, c)| matches!(c, C::DropForeignKey { .. })).copied().collect(),
            ),
            (&mut alters, picks.iter().filter(|(_, c)| !is_fk(c)).copied().collect()),
            (
                &mut add_fks,
                picks.iter().filter(|(_, c)| matches!(c, C::AddForeignKey { .. })).copied().collect(),
            ),
        ];
        for (out, group) in groups {
            if group.is_empty() {
                continue;
            }
            let reorder = group.iter().any(|(_, c)| matches!(c, C::MoveColumn { .. } | C::AddColumn { .. }));
            let diff = Diff {
                changes: group.iter().map(|(_, c)| (*c).clone()).collect(),
                appended: !reorder || t.appended,
            };
            for st in d.design_sql(schema, name, &diff)? {
                let items = st.changes.iter().map(|&i| group[i].0).collect();
                out.push(SyncStep::Sql { sql: st.sql, items });
            }
        }
    }
    let steps = [drop_fks, drop_tables, renames, alters, creates, add_fks].concat();
    Ok(SyncScript { steps, refused, warnings, transactional: d.design_transactional() })
}
