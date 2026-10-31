//! 比对前把两边的表整理成可以直接比较的形式（M4a 设计文档 5.3 节）：
//! 外键对本 schema 的引用写成"不带 schema"；目标中改了名的表按源的名字引用；
//! 按选项去掉注释、按目标的顺序排列共有的列、对齐系统生成的约束名和索引名；
//! 写法不同但含义相同的默认值和类型按目标的写法。

use std::collections::BTreeMap;

use dblens_core::schema::{Column, ForeignKey, Table};

use super::CompareOptions;
use crate::design::diff::same_type;
use crate::{Dialect, Family};

/// 外键引用本 schema 时去掉 schema，两边各自的 schema 名不同时也能比较。
fn own_schema_refs(t: &mut Table, schema: &str) {
    for f in &mut t.foreign_keys {
        if f.ref_schema.as_deref().is_some_and(|s| s == schema || s.is_empty()) {
            f.ref_schema = None;
        }
    }
}

/// 只有源有的表（要新建）：整理外键的 schema。
pub(super) fn source_only(t: &Table, schema: &str) -> Table {
    let mut t = t.clone();
    own_schema_refs(&mut t, schema);
    t
}

/// 整理一对表，返回（源，目标）；目标的名字换成源的名字（表改名在表这一层单独处理）。
/// `table_map`：目标的表名 → 源的表名（配上对的表）；`columns`：源的列名 → 目标的列名。
#[allow(clippy::too_many_arguments)]
pub(super) fn align(
    d: &Dialect,
    source: &Table,
    target: &Table,
    source_schema: &str,
    target_schema: &str,
    table_map: &BTreeMap<String, String>,
    columns: &BTreeMap<String, String>,
    opts: &CompareOptions,
) -> (Table, Table) {
    let mut s = source.clone();
    let mut t = target.clone();
    own_schema_refs(&mut s, source_schema);
    own_schema_refs(&mut t, target_schema);
    default_actions(d, &mut s);
    default_actions(d, &mut t);
    t.name = s.name.clone();
    for f in &mut t.foreign_keys {
        if f.ref_schema.is_none()
            && let Some(n) = table_map.get(&f.ref_table)
        {
            f.ref_table = n.clone();
        }
    }
    if opts.case_insensitive {
        // 名字不区分大小写时，目标中的列名、引用的表名按源的写法
        for c in &mut t.columns {
            if let Some(x) = s.columns.iter().find(|x| x.name.eq_ignore_ascii_case(&c.name)) {
                c.name = x.name.clone();
            }
        }
    }
    if opts.ignore_comments {
        for x in [&mut s, &mut t] {
            x.comment = None;
            x.columns.iter_mut().for_each(|c| c.comment = None);
        }
    }
    // 源的列名 → 目标的列名（改名）；目标中的对应列
    let target_of = |c: &Column| columns.get(&c.name).cloned().unwrap_or_else(|| c.name.clone());
    for c in &mut s.columns {
        let name = target_of(c);
        if let Some(tc) = t.columns.iter().find(|x| x.name == name) {
            if c.default.is_some() && tc.default.is_some() && same_default(d, &c.default, &tc.default) {
                c.default = tc.default.clone();
            }
            if same_type(&c.data_type.native, &tc.data_type.native)
                || same_type(
                    &canonical_type(d, &c.data_type.native),
                    &canonical_type(d, &tc.data_type.native),
                )
            {
                c.data_type = tc.data_type.clone();
            }
        }
    }
    if opts.ignore_column_order {
        // 共有的列按目标中的顺序，只有源有的列放在最后（按源中的顺序）
        let pos = |c: &Column| t.columns.iter().position(|x| x.name == target_of(c)).unwrap_or(usize::MAX);
        s.columns.sort_by_key(|c| pos(c));
    }
    if opts.ignore_generated_names {
        align_names(d, &mut s, &t);
    }
    (s, t)
}

/// 外键的默认动作写成"没有"：库读回 `NO ACTION`（MySQL 家族的 `RESTRICT` 与之相同），模型中通常不写。
fn default_actions(d: &Dialect, t: &mut Table) {
    let default = |a: &Option<String>| {
        a.as_deref().is_some_and(|a| {
            a.eq_ignore_ascii_case("NO ACTION")
                || (d.family() == Family::MySql && a.eq_ignore_ascii_case("RESTRICT"))
        })
    };
    for f in &mut t.foreign_keys {
        if default(&f.on_delete) {
            f.on_delete = None;
        }
        if default(&f.on_update) {
            f.on_update = None;
        }
    }
}

/// 去掉库加的类型转换（`'abc'::character varying`）和多余的括号后比较默认值。
fn same_default(d: &Dialect, a: &Option<String>, b: &Option<String>) -> bool {
    let norm = |v: &Option<String>| -> String {
        let mut s = v.as_deref().unwrap_or_default().trim().to_string();
        if d.family() == Family::Postgres {
            while let Some(i) = s.rfind("::") {
                let tail = &s[i + 2..];
                if tail.chars().all(|c| c.is_ascii_alphanumeric() || " _()[],\"".contains(c)) {
                    s.truncate(i);
                    s = s.trim().to_string();
                } else {
                    break;
                }
            }
        }
        while s.starts_with('(') && s.ends_with(')') && balanced(&s[1..s.len() - 1]) {
            s = s[1..s.len() - 1].trim().to_string();
        }
        s.to_ascii_lowercase()
    };
    norm(a) == norm(b)
}

fn balanced(s: &str) -> bool {
    let mut depth = 0i32;
    for c in s.chars() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth < 0 {
                    return false;
                }
            }
            _ => {}
        }
    }
    depth == 0
}

/// 同义的类型名换成同一种写法（`int` 与 `integer`、`int4` 等），用于判断两边的类型是否相同。
fn canonical_type(d: &Dialect, native: &str) -> String {
    let lower = native.trim().to_ascii_lowercase();
    let (base, rest) = match lower.find('(') {
        Some(i) => (lower[..i].trim().to_string(), lower[i..].to_string()),
        None => (lower.clone(), String::new()),
    };
    let base = match (d.family(), base.as_str()) {
        (Family::Postgres, "int" | "int4") => "integer",
        (Family::Postgres, "int8") => "bigint",
        (Family::Postgres, "int2") => "smallint",
        (Family::Postgres, "bool") => "boolean",
        (Family::Postgres, "float8") => "double precision",
        (Family::Postgres, "float4") => "real",
        (Family::Postgres, "varchar") => "character varying",
        (Family::Postgres, "char" | "bpchar") => "character",
        (Family::Postgres, "decimal") => "numeric",
        (Family::Postgres, "timestamp without time zone") => "timestamp",
        (Family::Postgres, "timestamptz") => "timestamp with time zone",
        (Family::MySql, "integer") => "int",
        (Family::MySql, "bool" | "boolean") => "tinyint(1)",
        (Family::MySql, "dec" | "numeric") => "decimal",
        (Family::Oracle | Family::Dameng, "integer" | "int") => "number(38)",
        (_, b) => b,
    }
    .to_string();
    // MySQL 8 的整数类型不报显示宽度：int(11) 与 int 相同
    let rest = if d.family() == Family::MySql
        && matches!(base.as_str(), "tinyint" | "smallint" | "mediumint" | "int" | "bigint")
        && !rest.starts_with("(1)")
    {
        rest.trim_start_matches(|c: char| c == '(' || c.is_ascii_digit() || c == ')').to_string()
    } else {
        rest
    };
    if base.contains('(') { base } else { format!("{base}{rest}") }
}

/// 系统生成的约束名、索引名：内容相同、至少一边是生成的名字时，源用目标的名字（同步时不必改名）。
fn align_names(d: &Dialect, s: &mut Table, t: &Table) {
    let generated = |n: &str| is_generated(d, &t.name, n);
    // 主键
    if let (Some(sp), Some(tp)) = (&mut s.primary_key, &t.primary_key)
        && sp.columns == tp.columns
        && sp.name != tp.name
        && (sp.name.as_deref().is_none_or(generated) || tp.name.as_deref().is_none_or(generated))
    {
        sp.name = tp.name.clone();
    }
    for u in &mut s.uniques {
        if let Some(x) = t.uniques.iter().find(|x| x.columns == u.columns)
            && u.name != x.name
            && (u.name.as_deref().is_none_or(generated) || x.name.as_deref().is_none_or(generated))
        {
            u.name = x.name.clone();
        }
    }
    for c in &mut s.checks {
        if let Some(x) = t.checks.iter().find(|x| same_check(&x.expression, &c.expression))
            && c.name != x.name
            && (c.name.as_deref().is_none_or(generated) || x.name.as_deref().is_none_or(generated))
        {
            c.name = x.name.clone();
            c.expression = x.expression.clone();
        }
    }
    for f in &mut s.foreign_keys {
        if let Some(x) = t.foreign_keys.iter().find(|x| same_fk_target(x, f))
            && f.name != x.name
            && (f.name.as_deref().is_none_or(generated) || x.name.as_deref().is_none_or(generated))
        {
            f.name = x.name.clone();
        }
    }
    for i in &mut s.indexes {
        if let Some(x) = t.indexes.iter().find(|x| x.columns == i.columns && x.unique == i.unique)
            && i.name != x.name
            && (generated(&i.name) || generated(&x.name))
        {
            i.name = x.name.clone();
        }
    }
}

fn same_fk_target(a: &ForeignKey, b: &ForeignKey) -> bool {
    a.columns == b.columns
        && a.ref_table == b.ref_table
        && a.ref_columns == b.ref_columns
        && a.ref_schema == b.ref_schema
}

/// 检查约束的表达式：库读回时常加括号、改大小写，去掉空白和最外层括号后不区分大小写比较。
fn same_check(a: &str, b: &str) -> bool {
    let norm = |s: &str| {
        let mut s: String = s.chars().filter(|c| !c.is_whitespace()).collect::<String>().to_ascii_lowercase();
        while s.starts_with('(') && s.ends_with(')') && balanced(&s[1..s.len() - 1]) {
            s = s[1..s.len() - 1].to_string();
        }
        s.replace(['`', '"'], "")
    };
    norm(a) == norm(b)
}

/// 名字是否像库自动生成的：Oracle、达梦的 `SYS_C…`、`CONS…`、`INDEX…`；MySQL 的 `PRIMARY`、`<表>_ibfk_N`、
/// `<表>_chk_N`；PG 家族的 `<表>_pkey`、`<表>_…_key`、`<表>_…_fkey`、`<表>_…_check`；
/// SQL Server 的 `PK__表__哈希`、`UQ__…`、`FK__…`、`CK__…`、`DF__…`。
pub(super) fn is_generated(d: &Dialect, table: &str, name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    let digits = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_digit());
    match d.family() {
        Family::Oracle | Family::Dameng => {
            upper.strip_prefix("SYS_C").is_some_and(digits)
                || upper.starts_with("SYS_")
                || upper.strip_prefix("CONS").is_some_and(digits)
                || upper.strip_prefix("INDEX").is_some_and(digits)
        }
        Family::MySql => {
            let t = table.to_ascii_lowercase();
            let n = name.to_ascii_lowercase();
            upper == "PRIMARY"
                || n.strip_prefix(&format!("{t}_ibfk_")).is_some_and(digits)
                || n.strip_prefix(&format!("{t}_chk_")).is_some_and(digits)
        }
        Family::SqlServer => ["PK__", "UQ__", "FK__", "CK__", "DF__"].iter().any(|p| upper.starts_with(p)),
        // 系统生成的约束名形如 u123_45、r123_45
        Family::Informix => false,
        Family::Postgres => {
            let prefix = format!("{}_", table.to_ascii_lowercase());
            let n = name.to_ascii_lowercase();
            n.starts_with(&prefix)
                && ["_pkey", "_key", "_fkey", "_check", "_idx", "_excl"].iter().any(|s| n.ends_with(s))
        }
    }
}
