//! 索引规则。

use dblens_core::schema::{Index, Table};
use dblens_core::{LogicalType, Msg, msg};

use super::{Check, Ctx, Finding, ParamDef, ParamDefault, Params, RuleDef, same_name, upper};

pub(super) const RULES: &[RuleDef] = &[
    RuleDef {
        id: "index.max_count",
        params: &[ParamDef { key: "max", default: ParamDefault::Num(5) }],
        fix: Some(|| msg!("lint.fix.drop_index")),
        check: max_count,
    },
    RuleDef {
        id: "index.max_columns",
        params: &[ParamDef { key: "max", default: ParamDefault::Num(5) }],
        fix: Some(|| msg!("lint.fix.drop_index")),
        check: max_columns,
    },
    RuleDef {
        id: "index.redundant",
        params: &[],
        fix: Some(|| msg!("lint.fix.drop_index")),
        check: redundant,
    },
    RuleDef {
        id: "index.on_lob",
        params: &[ParamDef { key: "max_len", default: ParamDefault::Num(4000) }],
        fix: Some(|| msg!("lint.fix.drop_index")),
        check: on_lob,
    },
    RuleDef { id: "index.function", params: &[], fix: Some(|| msg!("lint.fix.drop_index")), check: function },
    RuleDef { id: "index.bitmap", params: &[], fix: Some(|| msg!("lint.fix.drop_index")), check: bitmap },
    RuleDef {
        id: "index.every_column",
        params: &[],
        fix: Some(|| msg!("lint.fix.drop_index")),
        check: every_column,
    },
    RuleDef {
        id: "index.fk_without_index",
        params: &[],
        fix: Some(|| msg!("lint.fix.add_index")),
        check: fk_without_index,
    },
    RuleDef { id: "index.none", params: &[], fix: Some(|| msg!("lint.fix.drop_index")), check: none },
    RuleDef { id: "index.local_partition", params: &[], fix: None, check: local_partition },
    RuleDef {
        id: "index.low_selectivity",
        params: &[],
        fix: None,
        check: |_, _| Check::Unknown(msg!("lint.unknown.needs_data")),
    },
];

/// 索引是主键或唯一约束自带的。
pub(super) fn backs_constraint(t: &Table, i: &Index) -> bool {
    if i.name.eq_ignore_ascii_case("PRIMARY") {
        return true;
    }
    let pk = t.primary_key.as_ref().and_then(|k| k.name.as_deref());
    pk.is_some_and(|n| same_name(n, &i.name))
        || t.uniques.iter().any(|u| u.name.as_deref().is_some_and(|n| same_name(n, &i.name)))
}

fn columns(i: &Index) -> Vec<String> {
    i.columns.iter().map(|c| upper(&c.name)).collect()
}

fn key_columns(names: &[String]) -> Vec<String> {
    names.iter().map(|c| upper(c)).collect()
}

/// B 树类索引才谈得上"被覆盖"，全文、空间这类不比较。
fn is_btree(i: &Index) -> bool {
    i.method.as_deref().is_none_or(|m| {
        matches!(
            upper(m).as_str(),
            "BTREE" | "NORMAL" | "CLUSTERED" | "NONCLUSTERED" | "UNIQUE" | "NONUNIQUE"
        )
    })
}

fn is_function(t: &Table, i: &Index) -> bool {
    i.columns.iter().any(|ic| !t.columns.iter().any(|c| same_name(&c.name, &ic.name)))
}

fn per_index(ctx: &Ctx, bad: impl Fn(&Table, &Index) -> Option<Msg>) -> Check {
    let mut findings = vec![];
    for t in ctx.tables() {
        for i in &t.indexes {
            if let Some(m) = bad(t, i) {
                findings.push(Finding::index(t, i, m));
            }
        }
    }
    Check::Findings(findings)
}

fn max_count(ctx: &Ctx, p: &Params) -> Check {
    let max = p.num("max") as usize;
    let mut findings = vec![];
    for t in ctx.tables() {
        let count = t.indexes.iter().filter(|i| !backs_constraint(t, i)).count();
        if count > max {
            findings.push(Finding::table(t, msg!("lint.issue.too_many_indexes", count = count, max = max)));
        }
    }
    Check::Findings(findings)
}

fn max_columns(ctx: &Ctx, p: &Params) -> Check {
    let max = p.num("max") as usize;
    per_index(ctx, |_, i| {
        (i.columns.len() > max).then(|| msg!("lint.issue.index_columns", count = i.columns.len(), max = max))
    })
}

fn redundant(ctx: &Ctx, _: &Params) -> Check {
    let mut findings = vec![];
    for t in ctx.tables() {
        let usable: Vec<&Index> = t
            .indexes
            .iter()
            .filter(|i| is_btree(i) && !is_function(t, i) && !backs_constraint(t, i))
            .collect();
        let pk = t.primary_key.as_ref().map(|k| key_columns(&k.columns));
        for (n, i) in usable.iter().enumerate() {
            let cols = columns(i);
            let covered_by = usable.iter().enumerate().find_map(|(m, other)| {
                if m == n {
                    return None;
                }
                let others = columns(other);
                // 完全相同时只报后面那个；唯一索引同时是约束，不算被更长的索引覆盖
                let covered = if others == cols { m < n } else { !i.unique && others.starts_with(&cols) };
                covered.then_some(other.name.as_str())
            });
            if let Some(name) = covered_by {
                findings.push(Finding::index(t, i, msg!("lint.issue.index_covered", by = name)));
            } else if pk.as_ref() == Some(&cols) {
                findings.push(Finding::index(t, i, msg!("lint.issue.index_is_primary_key")));
            }
        }
    }
    Check::Findings(findings)
}

fn on_lob(ctx: &Ctx, p: &Params) -> Check {
    let max_len = p.num("max_len") as u32;
    per_index(ctx, |t, i| {
        let big =
            i.columns.iter().filter_map(|ic| t.columns.iter().find(|c| same_name(&c.name, &ic.name))).find(
                |c| match c.data_type.logical {
                    LogicalType::Text | LogicalType::Blob => true,
                    LogicalType::VarChar { len: Some(n) } => n >= max_len,
                    _ => false,
                },
            );
        big.map(|c| msg!("lint.issue.index_on_lob", column = c.name))
    })
}

fn function(ctx: &Ctx, _: &Params) -> Check {
    per_index(ctx, |t, i| is_function(t, i).then(|| msg!("lint.issue.function_index")))
}

fn bitmap(ctx: &Ctx, _: &Params) -> Check {
    per_index(ctx, |_, i| {
        i.method
            .as_deref()
            .is_some_and(|m| m.eq_ignore_ascii_case("BITMAP"))
            .then(|| msg!("lint.issue.bitmap_index"))
    })
}

fn every_column(ctx: &Ctx, _: &Params) -> Check {
    let mut findings = vec![];
    for t in ctx.tables() {
        let keys = t.primary_key.as_ref().map(|k| key_columns(&k.columns)).unwrap_or_default();
        let plain: Vec<_> = t.columns.iter().filter(|c| !keys.contains(&upper(&c.name))).collect();
        let indexed = |name: &str| {
            t.indexes.iter().any(|i| i.columns.len() == 1 && same_name(&i.columns[0].name, name))
        };
        if plain.len() >= 3 && plain.iter().all(|c| indexed(&c.name)) {
            findings.push(Finding::table(t, msg!("lint.issue.every_column_indexed")));
        }
    }
    Check::Findings(findings)
}

fn fk_without_index(ctx: &Ctx, _: &Params) -> Check {
    let mut findings = vec![];
    for t in ctx.tables() {
        for fk in &t.foreign_keys {
            let wanted = key_columns(&fk.columns);
            let leads = |cols: Vec<String>| cols.starts_with(&wanted);
            let covered = t.indexes.iter().any(|i| leads(columns(i)))
                || t.primary_key.iter().any(|k| leads(key_columns(&k.columns)))
                || t.uniques.iter().any(|u| leads(key_columns(&u.columns)));
            if !covered {
                let name = fk.name.as_deref().unwrap_or(&fk.ref_table);
                let object = format!("{}.{name}", t.name);
                let message = msg!("lint.issue.fk_without_index", columns = fk.columns.join(", "));
                findings.push(Finding { object, table: Some(t.name.clone()), message });
            }
        }
    }
    Check::Findings(findings)
}

fn none(ctx: &Ctx, _: &Params) -> Check {
    per_index(ctx, |_, _| Some(msg!("lint.issue.has_index")))
}

/// 分区信息只有文字描述，判断不了索引是不是本地索引。
fn local_partition(ctx: &Ctx, _: &Params) -> Check {
    if ctx.tables().iter().any(|t| t.partition.is_some()) {
        Check::Unknown(msg!("lint.unknown.no_metadata"))
    } else {
        Check::Findings(vec![])
    }
}

#[cfg(test)]
mod tests {
    use dblens_core::LogicalType;
    use dblens_core::schema::{ForeignKey, PrimaryKey, Table};
    use serde_json::json;

    use super::*;
    use crate::rules::Outcome;
    use crate::rules::testing::*;

    fn with_indexes(indexes: Vec<Index>) -> Table {
        let cols = ["a", "b", "c", "d"].iter().map(|n| int(n)).collect();
        let mut t = table("t", cols);
        t.indexes = indexes;
        t
    }

    #[test]
    fn max_count_ignores_constraint_indexes() {
        let mut t = with_indexes(vec![index("PRIMARY", &["a"]), index("i1", &["b"]), index("i2", &["c"])]);
        t.primary_key = pk(&["a"]);
        assert_eq!(objects(&tables(vec![t.clone()]), "index.max_count", json!({"max": 1})), ["t"]);
        assert!(objects(&tables(vec![t]), "index.max_count", json!({"max": 2})).is_empty());
    }

    #[test]
    fn max_columns_limits_composite_indexes() {
        let i = tables(vec![with_indexes(vec![index("i", &["a", "b", "c"])])]);
        assert_eq!(objects(&i, "index.max_columns", json!({"max": 2})), ["t.i"]);
        assert!(objects(&i, "index.max_columns", json!({"max": 3})).is_empty());
    }

    #[test]
    fn redundant_finds_prefixes_and_duplicates() {
        let i = tables(vec![with_indexes(vec![
            index("ab", &["a", "b"]),
            index("a", &["A"]),
            index("ab2", &["a", "b"]),
            index("c", &["c"]),
        ])]);
        assert_eq!(objects(&i, "index.redundant", json!({})), ["t.a", "t.ab2"]);
    }

    #[test]
    fn redundant_keeps_unique_prefixes_and_other_methods() {
        let mut unique = index("a", &["a"]);
        unique.unique = true;
        let mut text = index("b", &["b"]);
        text.method = Some("FULLTEXT".into());
        let i = tables(vec![with_indexes(vec![
            unique,
            text,
            index("ab", &["a", "b"]),
            index("bc", &["b", "c"]),
        ])]);
        assert!(objects(&i, "index.redundant", json!({})).is_empty());
    }

    #[test]
    fn redundant_reports_an_index_equal_to_the_primary_key() {
        let mut t = with_indexes(vec![index("i", &["a"])]);
        t.primary_key = Some(PrimaryKey { name: Some("pk_t".into()), columns: vec!["a".into()] });
        assert_eq!(objects(&tables(vec![t]), "index.redundant", json!({})), ["t.i"]);
    }

    #[test]
    fn on_lob_checks_text_blob_and_long_varchar() {
        let mut t =
            table("t", vec![col("a", "text", LogicalType::Text), varchar("b", 4000), varchar("c", 100)]);
        t.indexes = vec![index("ia", &["a"]), index("ib", &["b"]), index("ic", &["c"])];
        assert_eq!(objects(&tables(vec![t]), "index.on_lob", json!({})), ["t.ia", "t.ib"]);
    }

    #[test]
    fn function_index_has_a_key_that_is_not_a_column() {
        let i = tables(vec![with_indexes(vec![index("f", &["lower(a)"]), index("p", &["a"])])]);
        assert_eq!(objects(&i, "index.function", json!({})), ["t.f"]);
    }

    #[test]
    fn bitmap_goes_by_method() {
        let mut b = index("b", &["a"]);
        b.method = Some("BITMAP".into());
        let i = tables(vec![with_indexes(vec![b, index("n", &["b"])])]);
        assert_eq!(objects(&i, "index.bitmap", json!({})), ["t.b"]);
    }

    #[test]
    fn every_column_needs_at_least_three_plain_columns() {
        let mut t = table("t", vec![int("id"), int("a"), int("b"), int("c")]);
        t.primary_key = pk(&["id"]);
        t.indexes = vec![index("ia", &["a"]), index("ib", &["b"]), index("ic", &["c"])];
        assert_eq!(objects(&tables(vec![t.clone()]), "index.every_column", json!({})), ["t"]);
        t.indexes.pop();
        assert!(objects(&tables(vec![t]), "index.every_column", json!({})).is_empty());
    }

    #[test]
    fn fk_needs_a_leading_index() {
        let fk = ForeignKey {
            name: Some("fk1".into()),
            columns: vec!["a".into()],
            ref_schema: None,
            ref_table: "p".into(),
            ref_columns: vec!["id".into()],
            on_delete: None,
            on_update: None,
        };
        let mut bad = with_indexes(vec![index("i", &["b", "a"])]);
        bad.foreign_keys = vec![fk.clone()];
        assert_eq!(objects(&tables(vec![bad]), "index.fk_without_index", json!({})), ["t.fk1"]);
        let mut good = with_indexes(vec![index("i", &["a", "b"])]);
        good.foreign_keys = vec![fk.clone()];
        assert!(objects(&tables(vec![good]), "index.fk_without_index", json!({})).is_empty());
        let mut key = with_indexes(vec![]);
        key.primary_key = pk(&["a", "b"]);
        key.foreign_keys = vec![fk];
        assert!(objects(&tables(vec![key]), "index.fk_without_index", json!({})).is_empty());
    }

    #[test]
    fn none_flags_any_index() {
        let i = tables(vec![with_indexes(vec![index("i", &["a"])])]);
        assert_eq!(objects(&i, "index.none", json!({})), ["t.i"]);
    }

    #[test]
    fn local_partition_is_unknown_only_with_partitions() {
        let mut t = with_indexes(vec![]);
        assert!(objects(&tables(vec![t.clone()]), "index.local_partition", json!({})).is_empty());
        t.partition = Some("RANGE".into());
        assert!(matches!(outcome(&tables(vec![t]), "index.local_partition", json!({})), Outcome::Unknown(_)));
    }
}
