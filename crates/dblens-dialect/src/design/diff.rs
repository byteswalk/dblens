//! 草稿的校验与比对（M3 设计文档第 3 节）。与方言无关：哪些变更库不支持由方言另行判断。

use std::collections::{BTreeSet, HashMap};

use dblens_core::schema::{
    CheckConstraint, Column, ForeignKey, Index, IndexColumn, PrimaryKey, Table, UniqueConstraint,
};
use dblens_core::{Error, Result, msg};

use super::{DesignIssue, Diff, IssueSection as S, Renames, TableChange as C};

/// 索引列是不是表达式（如 `lower(name)`）：表达式不检查列是否存在，也不随列改名。
pub(crate) fn is_expression(name: &str) -> bool {
    name.contains('(') || name.contains(' ')
}

/// 校验草稿；返回全部错误（空表示可以生成 DDL）。
pub fn validate(draft: &Table) -> Vec<DesignIssue> {
    let mut out = Vec::new();
    let mut push = |section, row, field, message| out.push(DesignIssue::new(section, row, field, message));
    if draft.name.trim().is_empty() {
        push(S::Table, None, Some("name"), msg!("design.name_empty"));
    }
    if draft.columns.is_empty() {
        push(S::Columns, None, None, msg!("design.no_columns"));
    }
    let mut seen = BTreeSet::new();
    for (i, c) in draft.columns.iter().enumerate() {
        if c.name.trim().is_empty() {
            push(S::Columns, Some(i), Some("name"), msg!("design.name_empty"));
        } else if !seen.insert(c.name.as_str()) {
            push(S::Columns, Some(i), Some("name"), msg!("design.duplicate", name = c.name));
        }
        if c.data_type.native.trim().is_empty() {
            push(S::Columns, Some(i), Some("type"), msg!("design.type_empty"));
        }
    }
    let has = |n: &str| draft.columns.iter().any(|c| c.name == n);
    let mut columns = |section, row, cols: &[String], expressions: bool| {
        if cols.is_empty() {
            push(section, row, Some("columns"), msg!("design.columns_empty"));
        }
        let mut seen = BTreeSet::new();
        for c in cols {
            if expressions && is_expression(c) {
                continue;
            }
            if !has(c) {
                push(section, row, Some("columns"), msg!("design.unknown_column", column = c));
            } else if !seen.insert(c.as_str()) {
                push(section, row, Some("columns"), msg!("design.duplicate_column", column = c));
            }
        }
    };
    if let Some(pk) = &draft.primary_key {
        columns(S::PrimaryKey, None, &pk.columns, false);
    }
    for (i, u) in draft.uniques.iter().enumerate() {
        columns(S::Uniques, Some(i), &u.columns, false);
    }
    for (i, f) in draft.foreign_keys.iter().enumerate() {
        columns(S::ForeignKeys, Some(i), &f.columns, false);
    }
    for (i, x) in draft.indexes.iter().enumerate() {
        let names: Vec<String> = x.columns.iter().map(|c| c.name.clone()).collect();
        columns(S::Indexes, Some(i), &names, true);
    }
    for (i, f) in draft.foreign_keys.iter().enumerate() {
        if f.ref_table.trim().is_empty() {
            push(S::ForeignKeys, Some(i), Some("ref_table"), msg!("design.ref_table_empty"));
        } else if f.ref_columns.is_empty() || f.ref_columns.len() != f.columns.len() {
            push(S::ForeignKeys, Some(i), Some("ref_columns"), msg!("design.ref_columns_count"));
        }
    }
    for (i, c) in draft.checks.iter().enumerate() {
        if c.expression.trim().is_empty() {
            push(S::Checks, Some(i), Some("expression"), msg!("design.expression_empty"));
        }
    }
    let mut indexes = BTreeSet::new();
    for (i, x) in draft.indexes.iter().enumerate() {
        if x.name.trim().is_empty() {
            push(S::Indexes, Some(i), Some("name"), msg!("design.name_empty"));
        } else if !indexes.insert(x.name.as_str()) {
            push(S::Indexes, Some(i), Some("name"), msg!("design.duplicate", name = x.name));
        }
    }
    // 约束名在表内不能重复（未命名的由数据库取名）
    let mut names = BTreeSet::new();
    let named = [(S::PrimaryKey, None, draft.primary_key.as_ref().and_then(|k| k.name.as_deref()))]
        .into_iter()
        .chain(draft.uniques.iter().enumerate().map(|(i, u)| (S::Uniques, Some(i), u.name.as_deref())))
        .chain(
            draft.foreign_keys.iter().enumerate().map(|(i, f)| (S::ForeignKeys, Some(i), f.name.as_deref())),
        )
        .chain(draft.checks.iter().enumerate().map(|(i, c)| (S::Checks, Some(i), c.name.as_deref())));
    for (section, row, name) in named {
        if let Some(n) = name.filter(|n| !n.trim().is_empty())
            && !names.insert(n)
        {
            push(section, row, Some("name"), msg!("design.duplicate", name = n));
        }
    }
    out
}

/// 去掉首尾空白，空串当作没有。
fn text(v: &Option<String>) -> Option<&str> {
    v.as_deref().map(str::trim).filter(|s| !s.is_empty())
}

/// 类型文本比较：不区分大小写，忽略多余空白和括号、逗号两侧的空白。
fn same_type(a: &str, b: &str) -> bool {
    let norm = |s: &str| {
        let s = s.split_whitespace().collect::<Vec<_>>().join(" ").to_ascii_uppercase();
        s.replace(" (", "(").replace("( ", "(").replace(" )", ")").replace(", ", ",").replace(" ,", ",")
    };
    norm(a) == norm(b)
}

/// 列定义是否相同（不比较名字和序号）。
pub(crate) fn same_column(a: &Column, b: &Column) -> bool {
    same_type(&a.data_type.native, &b.data_type.native)
        && a.nullable == b.nullable
        && text(&a.default) == text(&b.default)
        && text(&a.comment) == text(&b.comment)
        && a.auto_increment == b.auto_increment
        && text(&a.generated) == text(&b.generated)
}

/// 外键动作：NO ACTION 与不写相同。
fn action(a: &Option<String>) -> Option<String> {
    text(a).map(|s| s.to_ascii_uppercase()).filter(|s| s != "NO ACTION")
}

/// 两项按名字配对；草稿中没有名字的项按内容找原表中同样没有名字的项。
/// 返回（要删除的原有项，要新增的草稿项）：内容不同的一对两边都出现。
fn pair<'a, T>(
    orig: &'a [T],
    draft: &'a [T],
    name: impl Fn(&T) -> Option<&str>,
    same: impl Fn(&T, &T) -> bool,
) -> (Vec<&'a T>, Vec<&'a T>) {
    let mut used = vec![false; orig.len()];
    let mut added = Vec::new();
    let mut dropped = Vec::new();
    for d in draft {
        let found = orig.iter().enumerate().position(|(i, o)| {
            !used[i]
                && match name(d) {
                    Some(n) => name(o) == Some(n),
                    None => name(o).is_none() && same(o, d),
                }
        });
        match found {
            Some(i) => {
                used[i] = true;
                if !same(&orig[i], d) {
                    dropped.push(&orig[i]);
                    added.push(d);
                }
            }
            None => added.push(d),
        }
    }
    dropped.extend(orig.iter().enumerate().filter(|(i, _)| !used[*i]).map(|(_, o)| o));
    (dropped, added)
}

/// 比对原表与草稿。`original` 为 None 表示新建表。草稿应已校验过（`validate`）。
/// `renames` 中的名字对不上时返回 `invalid`。
pub fn diff_table(original: Option<&Table>, draft: &Table, renames: &Renames) -> Result<Diff> {
    let Some(orig) = original else {
        return Ok(Diff { changes: vec![C::CreateTable { table: draft.clone() }], appended: true });
    };
    // 原名 → 新名（保留下来的列）
    let mut new_of: HashMap<&str, &str> = HashMap::new();
    for c in &draft.columns {
        let old = renames.columns.get(&c.name).map_or(c.name.as_str(), String::as_str);
        if renames.columns.contains_key(&c.name) && !orig.columns.iter().any(|o| o.name == old) {
            return Err(Error::invalid(msg!("design.bad_rename", name = c.name)));
        }
        if orig.columns.iter().any(|o| o.name == old) && new_of.insert(old, &c.name).is_some() {
            return Err(Error::invalid(msg!("design.bad_rename", name = c.name)));
        }
    }
    let map = |n: &String| new_of.get(n.as_str()).map_or(n.clone(), |s| s.to_string());
    let map_all = |cols: &[String]| cols.iter().map(map).collect::<Vec<_>>();

    let mut drops = Vec::new();
    let mut cols = Vec::new();
    let mut adds = Vec::new();

    // 列：删除、改名、修改、新增
    for o in &orig.columns {
        if !new_of.contains_key(o.name.as_str()) {
            cols.push(C::DropColumn { name: o.name.clone() });
        }
    }
    for o in &orig.columns {
        if let Some(&n) = new_of.get(o.name.as_str())
            && n != o.name
        {
            cols.push(C::RenameColumn { from: o.name.clone(), to: n.to_string() });
        }
    }
    for o in &orig.columns {
        let Some(&n) = new_of.get(o.name.as_str()) else { continue };
        let d = draft.columns.iter().find(|c| c.name == n).expect("配对的列在草稿中");
        if !same_column(o, d) {
            cols.push(C::AlterColumn { from: Column { name: n.to_string(), ..o.clone() }, to: d.clone() });
        }
    }
    let is_new = |c: &Column| !new_of.values().any(|&n| n == c.name);
    for (i, c) in draft.columns.iter().enumerate() {
        if is_new(c) {
            let after = i.checked_sub(1).map(|p| draft.columns[p].name.clone());
            cols.push(C::AddColumn { column: c.clone(), after });
        }
    }
    // 列顺序：先按"保留的列保持原顺序、新列插到草稿中的位置"排出当前顺序，再逐个挪到草稿中的位置
    let mut current: Vec<String> =
        orig.columns.iter().filter_map(|o| new_of.get(o.name.as_str()).map(|n| n.to_string())).collect();
    let kept = current.clone();
    for (i, c) in draft.columns.iter().enumerate() {
        if is_new(c) {
            let at = match i.checked_sub(1) {
                None => 0,
                Some(p) => {
                    current.iter().position(|x| *x == draft.columns[p].name).map_or(current.len(), |k| k + 1)
                }
            };
            current.insert(at, c.name.clone());
        }
    }
    for (i, c) in draft.columns.iter().enumerate() {
        if current[i] != c.name {
            let from = current.iter().position(|x| *x == c.name).expect("草稿的列都在当前顺序中");
            let name = current.remove(from);
            current.insert(i, name);
            let after = i.checked_sub(1).map(|p| draft.columns[p].name.clone());
            cols.push(C::MoveColumn { column: c.clone(), after });
        }
    }
    // 新列都追加在最后、保留的列相对顺序不变时，不能指定列位置的库也能照草稿的顺序得到结果
    let appended = {
        let mut expect = kept;
        expect.extend(draft.columns.iter().filter(|c| is_new(c)).map(|c| c.name.clone()));
        expect.iter().eq(draft.columns.iter().map(|c| &c.name))
    };

    // 主键
    let opk =
        orig.primary_key.as_ref().map(|k| PrimaryKey { name: k.name.clone(), columns: map_all(&k.columns) });
    let same_pk = |a: &PrimaryKey, b: &PrimaryKey| {
        a.columns == b.columns && (a.name.is_none() || b.name.is_none() || a.name == b.name)
    };
    match (&opk, &draft.primary_key) {
        (Some(a), Some(b)) if same_pk(a, b) => {}
        (Some(_), Some(b)) => {
            drops.push(C::DropPrimaryKey { key: orig.primary_key.clone().expect("有主键") });
            adds.push(C::AddPrimaryKey { key: b.clone() });
        }
        (Some(_), None) => {
            drops.push(C::DropPrimaryKey { key: orig.primary_key.clone().expect("有主键") })
        }
        (None, Some(b)) => adds.push(C::AddPrimaryKey { key: b.clone() }),
        (None, None) => {}
    }

    // 唯一约束、检查约束：先把原表中引用的列换成新名再比较
    let ouniq: Vec<UniqueConstraint> = orig
        .uniques
        .iter()
        .map(|u| UniqueConstraint { name: u.name.clone(), columns: map_all(&u.columns) })
        .collect();
    let (d, a) = pair(&ouniq, &draft.uniques, |u| u.name.as_deref(), |x, y| x.columns == y.columns);
    let original_of = |u: &UniqueConstraint| {
        orig.uniques[ouniq.iter().position(|x| std::ptr::eq(x, u)).expect("同一个切片")].clone()
    };
    drops.extend(d.into_iter().map(|u| C::DropUnique { unique: original_of(u) }));
    adds.extend(a.into_iter().map(|u| C::AddUnique { unique: u.clone() }));

    let (d, a) = pair(
        &orig.checks,
        &draft.checks,
        |c| c.name.as_deref(),
        |x: &CheckConstraint, y| x.expression.trim() == y.expression.trim(),
    );
    drops.extend(d.into_iter().map(|c| C::DropCheck { check: c.clone() }));
    adds.extend(a.into_iter().map(|c| C::AddCheck { check: c.clone() }));

    // 外键：指向本表自己的外键，引用列也随改名
    let self_ref = |f: &ForeignKey| f.ref_table == orig.name;
    let ofk: Vec<ForeignKey> = orig
        .foreign_keys
        .iter()
        .map(|f| ForeignKey {
            columns: map_all(&f.columns),
            ref_columns: if self_ref(f) { map_all(&f.ref_columns) } else { f.ref_columns.clone() },
            ..f.clone()
        })
        .collect();
    let same_fk = |x: &ForeignKey, y: &ForeignKey| {
        x.columns == y.columns
            && x.ref_table == y.ref_table
            && text(&x.ref_schema) == text(&y.ref_schema)
            && x.ref_columns == y.ref_columns
            && action(&x.on_delete) == action(&y.on_delete)
            && action(&x.on_update) == action(&y.on_update)
    };
    let (d, a) = pair(&ofk, &draft.foreign_keys, |f| f.name.as_deref(), same_fk);
    let fk_drops: Vec<C> = d
        .into_iter()
        .map(|f| C::DropForeignKey {
            key: orig.foreign_keys[ofk.iter().position(|x| std::ptr::eq(x, f)).expect("同一个切片")].clone(),
        })
        .collect();
    let fk_adds: Vec<C> = a.into_iter().map(|f| C::AddForeignKey { key: f.clone() }).collect();

    // 索引：按（经改名换算的）原名配对；只改了名字时改名，内容不同时先删后建
    let map_index = |x: &Index| Index {
        columns: x
            .columns
            .iter()
            .map(|c| IndexColumn {
                name: if is_expression(&c.name) { c.name.clone() } else { map(&c.name) },
                descending: c.descending,
            })
            .collect(),
        ..x.clone()
    };
    let same_index = |x: &Index, y: &Index| {
        x.columns == y.columns
            && x.unique == y.unique
            && x.method.as_deref().map(str::to_ascii_uppercase)
                == y.method.as_deref().map(str::to_ascii_uppercase)
    };
    let mut used = vec![false; orig.indexes.len()];
    let mut renames_ix = Vec::new();
    for x in &draft.indexes {
        let old = renames.indexes.get(&x.name).map_or(x.name.as_str(), String::as_str);
        match orig.indexes.iter().position(|o| o.name == old) {
            Some(i) if !used[i] => {
                used[i] = true;
                if !same_index(&map_index(&orig.indexes[i]), x) {
                    drops.push(C::DropIndex { index: orig.indexes[i].clone() });
                    adds.push(C::AddIndex { index: x.clone() });
                } else if old != x.name {
                    renames_ix.push(C::RenameIndex { from: old.to_string(), to: x.name.clone() });
                }
            }
            _ => adds.push(C::AddIndex { index: x.clone() }),
        }
    }
    for (i, o) in orig.indexes.iter().enumerate() {
        if !used[i] {
            drops.push(C::DropIndex { index: o.clone() });
        }
    }

    let mut changes = fk_drops;
    changes.extend(drops);
    changes.extend(cols);
    changes.extend(adds);
    changes.extend(renames_ix);
    changes.extend(fk_adds);
    if text(&orig.comment) != text(&draft.comment) {
        changes.push(C::SetTableComment { comment: text(&draft.comment).map(str::to_string) });
    }
    Ok(Diff { changes, appended })
}

#[cfg(test)]
mod tests {
    use dblens_core::{ColumnType, LogicalType};

    use super::*;

    fn col(name: &str, native: &str) -> Column {
        Column {
            name: name.into(),
            ordinal: 0,
            data_type: ColumnType { native: native.into(), logical: LogicalType::Other },
            nullable: true,
            default: None,
            comment: None,
            auto_increment: false,
            generated: None,
        }
    }

    fn table() -> Table {
        Table {
            name: "orders".into(),
            comment: Some("订单".into()),
            columns: vec![col("id", "BIGINT"), col("code", "VARCHAR(20)"), col("amount", "DECIMAL(10,2)")],
            primary_key: Some(PrimaryKey { name: Some("pk_orders".into()), columns: vec!["id".into()] }),
            uniques: vec![UniqueConstraint { name: Some("uk_code".into()), columns: vec!["code".into()] }],
            checks: vec![CheckConstraint {
                name: Some("ck_amount".into()),
                expression: "amount >= 0".into(),
            }],
            foreign_keys: vec![ForeignKey {
                name: Some("fk_parent".into()),
                columns: vec!["code".into()],
                ref_schema: None,
                ref_table: "parent".into(),
                ref_columns: vec!["code".into()],
                on_delete: Some("NO ACTION".into()),
                on_update: None,
            }],
            indexes: vec![Index {
                name: "ix_amount".into(),
                columns: vec![IndexColumn { name: "amount".into(), descending: false }],
                unique: false,
                method: Some("BTREE".into()),
            }],
            ..Default::default()
        }
    }

    fn diff(orig: &Table, draft: &Table, renames: &Renames) -> Vec<C> {
        diff_table(Some(orig), draft, renames).unwrap().changes
    }

    fn kinds(changes: &[C]) -> Vec<String> {
        changes
            .iter()
            .map(|c| serde_json::to_value(c).unwrap()["kind"].as_str().unwrap().to_string())
            .collect()
    }

    #[test]
    fn unchanged_table_has_no_changes() {
        let t = table();
        let d = diff_table(Some(&t), &t, &Renames::default()).unwrap();
        assert!(d.changes.is_empty());
        assert!(d.appended);
        // 类型的大小写、空白不同不算改动；NO ACTION 与不写相同
        let mut draft = t.clone();
        draft.columns[2].data_type.native = "decimal( 10, 2 )".into();
        draft.foreign_keys[0].on_delete = None;
        draft.comment = Some(" 订单 ".into());
        assert!(diff(&t, &draft, &Renames::default()).is_empty());
    }

    #[test]
    fn new_table_is_one_create() {
        let t = table();
        let d = diff_table(None, &t, &Renames::default()).unwrap();
        assert_eq!(d.changes, vec![C::CreateTable { table: t }]);
    }

    #[test]
    fn column_changes_in_order() {
        let t = table();
        let mut draft = t.clone();
        draft.columns.retain(|c| c.name != "amount");
        draft.indexes.clear();
        draft.checks.clear();
        draft.columns[1].data_type.native = "VARCHAR(50)".into();
        draft.columns[1].nullable = false;
        draft.columns.push(col("note", "TEXT"));
        let changes = diff(&t, &draft, &Renames::default());
        assert_eq!(
            kinds(&changes),
            ["drop_check", "drop_index", "drop_column", "alter_column", "add_column"]
        );
        let C::AlterColumn { from, to } = &changes[3] else { panic!() };
        assert_eq!(
            (from.data_type.native.as_str(), to.data_type.native.as_str()),
            ("VARCHAR(20)", "VARCHAR(50)")
        );
        assert_eq!(changes[4], C::AddColumn { column: col("note", "TEXT"), after: Some("code".into()) });
    }

    #[test]
    fn rename_follows_into_constraints_and_indexes() {
        let t = table();
        let mut draft = t.clone();
        draft.columns[1].name = "order_code".into();
        draft.columns[2].name = "total".into();
        draft.uniques[0].columns = vec!["order_code".into()];
        draft.foreign_keys[0].columns = vec!["order_code".into()];
        draft.indexes[0].columns[0].name = "total".into();
        draft.checks[0].expression = "total >= 0".into();
        let renames = Renames {
            columns: [("order_code".into(), "code".into()), ("total".into(), "amount".into())].into(),
            ..Default::default()
        };
        let changes = diff(&t, &draft, &renames);
        // 检查约束的表达式是文本，改了就先删后建；其余随改名不算改动
        assert_eq!(kinds(&changes), ["drop_check", "rename_column", "rename_column", "add_check"]);
        assert_eq!(changes[1], C::RenameColumn { from: "code".into(), to: "order_code".into() });
    }

    #[test]
    fn rename_and_alter_together() {
        let t = table();
        let mut draft = t.clone();
        draft.columns[2].name = "total".into();
        draft.columns[2].comment = Some("合计".into());
        draft.indexes[0].columns[0].name = "total".into();
        draft.checks[0].expression = "amount >= 0".into();
        draft.checks.clear();
        let renames = Renames { columns: [("total".into(), "amount".into())].into(), ..Default::default() };
        let changes = diff(&t, &draft, &renames);
        assert_eq!(kinds(&changes), ["drop_check", "rename_column", "alter_column"]);
        let C::AlterColumn { from, to } = &changes[2] else { panic!() };
        assert_eq!(from.name, "total", "修改发生在改名之后");
        assert_eq!(to.comment.as_deref(), Some("合计"));
    }

    #[test]
    fn bad_renames_are_rejected() {
        let t = table();
        let mut draft = t.clone();
        draft.columns[1].name = "x".into();
        let r = Renames { columns: [("x".into(), "nope".into())].into(), ..Default::default() };
        assert!(diff_table(Some(&t), &draft, &r).is_err());
        // 两个草稿列配到同一个原列
        let r = Renames { columns: [("x".into(), "id".into())].into(), ..Default::default() };
        assert!(diff_table(Some(&t), &draft, &r).is_err());
    }

    #[test]
    fn constraints_and_indexes() {
        let t = table();
        let mut draft = t.clone();
        draft.primary_key =
            Some(PrimaryKey { name: Some("pk_orders".into()), columns: vec!["id".into(), "code".into()] });
        draft.uniques[0].columns = vec!["code".into(), "amount".into()];
        draft.foreign_keys[0].on_delete = Some("CASCADE".into());
        draft.indexes[0].name = "ix_total".into();
        draft.indexes.push(Index {
            name: "ix_lower".into(),
            columns: vec![IndexColumn { name: "lower(code)".into(), descending: false }],
            unique: false,
            method: None,
        });
        draft.uniques.push(UniqueConstraint { name: None, columns: vec!["amount".into()] });
        let renames =
            Renames { indexes: [("ix_total".into(), "ix_amount".into())].into(), ..Default::default() };
        let changes = diff(&t, &draft, &renames);
        assert_eq!(
            kinds(&changes),
            [
                "drop_foreign_key",
                "drop_primary_key",
                "drop_unique",
                "add_primary_key",
                "add_unique",
                "add_unique",
                "add_index",
                "rename_index",
                "add_foreign_key"
            ]
        );
        assert_eq!(changes[7], C::RenameIndex { from: "ix_amount".into(), to: "ix_total".into() });
        // 改了索引内容：先删后建
        let mut draft = t.clone();
        draft.indexes[0].columns[0].descending = true;
        assert_eq!(kinds(&diff(&t, &draft, &Renames::default())), ["drop_index", "add_index"]);
        // 删除主键、改表注释
        let mut draft = t.clone();
        draft.primary_key = None;
        draft.comment = None;
        let changes = diff(&t, &draft, &Renames::default());
        assert_eq!(kinds(&changes), ["drop_primary_key", "set_table_comment"]);
        assert_eq!(changes[1], C::SetTableComment { comment: None });
    }

    #[test]
    fn column_order() {
        let t = table();
        // 新列追加在最后：不需要挪
        let mut draft = t.clone();
        draft.columns.push(col("note", "TEXT"));
        let d = diff_table(Some(&t), &draft, &Renames::default()).unwrap();
        assert!(d.appended);
        assert_eq!(kinds(&d.changes), ["add_column"]);
        // 新列插在中间：指定位置的 ADD，不能指定位置的库要重建
        let mut draft = t.clone();
        draft.columns.insert(1, col("note", "TEXT"));
        let d = diff_table(Some(&t), &draft, &Renames::default()).unwrap();
        assert!(!d.appended);
        assert_eq!(d.changes, vec![C::AddColumn { column: col("note", "TEXT"), after: Some("id".into()) }]);
        // 把最后一列挪到最前：一次挪动
        let mut draft = t.clone();
        let last = draft.columns.pop().unwrap();
        draft.columns.insert(0, last);
        let d = diff_table(Some(&t), &draft, &Renames::default()).unwrap();
        assert!(!d.appended);
        assert_eq!(d.changes, vec![C::MoveColumn { column: col("amount", "DECIMAL(10,2)"), after: None }]);
        // 交换前两列
        let mut draft = t.clone();
        draft.columns.swap(0, 1);
        let d = diff_table(Some(&t), &draft, &Renames::default()).unwrap();
        assert_eq!(d.changes, vec![C::MoveColumn { column: col("code", "VARCHAR(20)"), after: None }]);
    }

    #[test]
    fn validation_reports_positions() {
        let mut t = table();
        t.name = " ".into();
        t.columns.push(col("id", ""));
        t.uniques.push(UniqueConstraint { name: Some("uk_code".into()), columns: vec!["nope".into()] });
        t.foreign_keys[0].ref_columns.push("x".into());
        t.indexes.push(Index { name: "ix_amount".into(), columns: vec![], unique: false, method: None });
        t.indexes.push(Index {
            name: "ix_expr".into(),
            columns: vec![IndexColumn { name: "lower(code)".into(), descending: false }],
            unique: false,
            method: None,
        });
        t.checks.push(CheckConstraint { name: None, expression: "".into() });
        let issues = validate(&t);
        let at: Vec<(S, Option<usize>, Option<&str>)> =
            issues.iter().map(|i| (i.section, i.row, i.field)).collect();
        assert_eq!(
            at,
            [
                (S::Table, None, Some("name")),
                (S::Columns, Some(3), Some("name")),
                (S::Columns, Some(3), Some("type")),
                (S::Uniques, Some(1), Some("columns")),
                (S::Indexes, Some(1), Some("columns")),
                (S::ForeignKeys, Some(0), Some("ref_columns")),
                (S::Checks, Some(1), Some("expression")),
                (S::Indexes, Some(1), Some("name")),
                (S::Uniques, Some(1), Some("name")),
            ]
        );
        assert!(validate(&table()).is_empty());
    }
}
