//! 把变更应用到表结构上（M4a 设计文档 6.2 节）：同步的目标是模型时，按勾选的变更改写模型中的表。
//! 与 `diff_table` 互逆：`apply_changes(原表, diff_table(原表, 草稿))` 与草稿再比对没有差异。

use dblens_core::schema::{Column, Table};
use dblens_core::{Error, Result, msg};

use crate::design::TableChange as C;

/// 依次应用变更。变更引用的列、约束在表中找不到时返回 `invalid`。
pub fn apply_changes(table: &Table, changes: &[C]) -> Result<Table> {
    let mut t = table.clone();
    for ch in changes {
        apply_one(&mut t, ch)?;
    }
    for (i, c) in t.columns.iter_mut().enumerate() {
        c.ordinal = (i + 1) as u32;
    }
    Ok(t)
}

fn missing(what: &str) -> Error {
    Error::invalid(msg!("compare.apply_missing", name = what))
}

fn position(t: &Table, name: &str) -> Result<usize> {
    t.columns.iter().position(|c| c.name == name).ok_or_else(|| missing(name))
}

/// 放在 `after` 之后（None 为最前）。
fn insert_after(t: &mut Table, column: Column, after: &Option<String>) -> Result<()> {
    let at = match after {
        None => 0,
        Some(a) => position(t, a)? + 1,
    };
    t.columns.insert(at, column);
    Ok(())
}

fn rename_refs(t: &mut Table, from: &str, to: &str) {
    let fix =
        |cols: &mut Vec<String>| cols.iter_mut().filter(|c| *c == from).for_each(|c| *c = to.to_string());
    if let Some(pk) = &mut t.primary_key {
        fix(&mut pk.columns);
    }
    t.uniques.iter_mut().for_each(|u| fix(&mut u.columns));
    let own = t.name.clone();
    for f in &mut t.foreign_keys {
        fix(&mut f.columns);
        if f.ref_table == own && f.ref_schema.is_none() {
            fix(&mut f.ref_columns);
        }
    }
    for x in &mut t.indexes {
        x.columns.iter_mut().filter(|c| c.name == from).for_each(|c| c.name = to.to_string());
    }
}

fn apply_one(t: &mut Table, ch: &C) -> Result<()> {
    match ch {
        C::CreateTable { table } => *t = table.clone(),
        C::SetTableComment { comment } => t.comment = comment.clone(),
        C::AddColumn { column, after } => insert_after(t, column.clone(), after)?,
        C::DropColumn { name } => {
            let i = position(t, name)?;
            t.columns.remove(i);
        }
        C::RenameColumn { from, to, .. } => {
            let i = position(t, from)?;
            t.columns[i].name = to.clone();
            rename_refs(t, from, to);
        }
        C::AlterColumn { to, .. } => {
            let i = position(t, &to.name)?;
            t.columns[i] = Column { ordinal: t.columns[i].ordinal, ..to.clone() };
        }
        C::MoveColumn { column, after } => {
            let i = position(t, &column.name)?;
            let c = t.columns.remove(i);
            insert_after(t, c, after)?;
        }
        C::DropPrimaryKey { .. } => t.primary_key = None,
        C::AddPrimaryKey { key } => t.primary_key = Some(key.clone()),
        C::DropUnique { unique } => {
            let i = t
                .uniques
                .iter()
                .position(|u| u.name == unique.name && (unique.name.is_some() || u.columns == unique.columns))
                .ok_or_else(|| missing(unique.name.as_deref().unwrap_or("UNIQUE")))?;
            t.uniques.remove(i);
        }
        C::AddUnique { unique } => t.uniques.push(unique.clone()),
        C::DropCheck { check } => {
            let i = t
                .checks
                .iter()
                .position(|c| {
                    c.name == check.name && (check.name.is_some() || c.expression == check.expression)
                })
                .ok_or_else(|| missing(check.name.as_deref().unwrap_or("CHECK")))?;
            t.checks.remove(i);
        }
        C::AddCheck { check } => t.checks.push(check.clone()),
        C::DropForeignKey { key } => {
            let i = t
                .foreign_keys
                .iter()
                .position(|f| f.name == key.name && (key.name.is_some() || f.columns == key.columns))
                .ok_or_else(|| missing(key.name.as_deref().unwrap_or("FOREIGN KEY")))?;
            t.foreign_keys.remove(i);
        }
        C::AddForeignKey { key } => t.foreign_keys.push(key.clone()),
        C::DropIndex { index } => {
            let i =
                t.indexes.iter().position(|x| x.name == index.name).ok_or_else(|| missing(&index.name))?;
            t.indexes.remove(i);
        }
        C::AddIndex { index } => t.indexes.push(index.clone()),
        C::RenameIndex { from, index } => {
            let i = t.indexes.iter().position(|x| &x.name == from).ok_or_else(|| missing(from))?;
            t.indexes[i] = index.clone();
        }
    }
    Ok(())
}
