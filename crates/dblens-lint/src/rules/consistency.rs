//! 跨表一致性规则。都是启发式的，只给建议。

use std::collections::{BTreeMap, BTreeSet};

use dblens_core::msg;

use super::{Check, Ctx, Finding, Params, RuleDef, same_name, upper};

pub(super) const RULES: &[RuleDef] = &[
    RuleDef {
        id: "consistency.same_name",
        params: &[],
        fix: Some(|| msg!("lint.fix.same_name")),
        check: same_name_types,
    },
    RuleDef {
        id: "consistency.synonym",
        params: &[],
        fix: Some(|| msg!("lint.fix.synonym")),
        check: synonym,
    },
    RuleDef {
        id: "consistency.fk_type",
        params: &[],
        fix: Some(|| msg!("lint.fix.fk_type")),
        check: fk_type,
    },
];

/// 类型比较不看大小写和空格。
fn type_key(native: &str) -> String {
    upper(native).split_whitespace().collect()
}

/// 同一个字段名在不同的表里类型不同。
fn same_name_types(ctx: &Ctx, _: &Params) -> Check {
    let mut by_name: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for t in ctx.tables() {
        for c in &t.columns {
            by_name.entry(upper(&c.name)).or_default().insert(type_key(&c.data_type.native));
        }
    }
    let findings = by_name
        .into_iter()
        .filter(|(_, types)| types.len() > 1)
        .map(|(name, types)| {
            let shown = types.into_iter().collect::<Vec<_>>().join(", ");
            Finding::object(&name, msg!("lint.issue.same_name_types", types = shown))
        })
        .collect();
    Check::Findings(findings)
}

/// 注释相同而字段名不同，可能是同一含义起了两个名字。
fn synonym(ctx: &Ctx, _: &Params) -> Check {
    let mut by_comment: BTreeMap<&str, Vec<String>> = BTreeMap::new();
    for t in ctx.tables() {
        for c in &t.columns {
            let Some(comment) = c.comment.as_deref().map(str::trim).filter(|x| x.chars().count() >= 2) else {
                continue;
            };
            let names = by_comment.entry(comment).or_default();
            let name = upper(&c.name);
            if !names.contains(&name) {
                names.push(name);
            }
        }
    }
    let findings = by_comment
        .into_iter()
        .filter(|(_, names)| names.len() > 1)
        .map(|(comment, names)| {
            Finding::object(comment, msg!("lint.issue.synonym", columns = names.join(", ")))
        })
        .collect();
    Check::Findings(findings)
}

/// 外键两端的字段类型不一致。引用别的模式的外键不查。
fn fk_type(ctx: &Ctx, _: &Params) -> Check {
    let schema = &ctx.input.schema.name;
    let mut findings = vec![];
    for t in ctx.tables() {
        for fk in
            t.foreign_keys.iter().filter(|fk| fk.ref_schema.as_deref().is_none_or(|s| same_name(s, schema)))
        {
            let Some(parent) = ctx.tables().iter().find(|p| same_name(&p.name, &fk.ref_table)) else {
                continue;
            };
            for (name, ref_name) in fk.columns.iter().zip(&fk.ref_columns) {
                let child = t.columns.iter().find(|c| same_name(&c.name, name));
                let target = parent.columns.iter().find(|c| same_name(&c.name, ref_name));
                if let (Some(c), Some(r)) = (child, target)
                    && type_key(&c.data_type.native) != type_key(&r.data_type.native)
                {
                    let message = msg!(
                        "lint.issue.fk_type",
                        table = parent.name,
                        column = r.name,
                        own = c.data_type.native,
                        other = r.data_type.native
                    );
                    findings.push(Finding::column(t, c, message));
                }
            }
        }
    }
    Check::Findings(findings)
}

#[cfg(test)]
mod tests {
    use dblens_core::schema::ForeignKey;
    use serde_json::json;

    use crate::rules::testing::*;

    #[test]
    fn same_name_reports_differing_types() {
        let i = tables(vec![
            table("a", vec![varchar("code", 10), int("id")]),
            table("b", vec![varchar("CODE", 20), int("id")]),
        ]);
        assert_eq!(objects(&i, "consistency.same_name", json!({})), ["CODE"]);
    }

    #[test]
    fn synonym_groups_columns_by_comment() {
        let named = |name: &str, comment: &str| {
            let mut c = int(name);
            c.comment = Some(comment.into());
            c
        };
        let i = tables(vec![
            table("a", vec![named("cust_no", "customer"), named("x", "n")]),
            table("b", vec![named("customer_id", "customer"), named("y", "n")]),
            table("c", vec![named("cust_no", "customer")]),
        ]);
        assert_eq!(objects(&i, "consistency.synonym", json!({})), ["customer"]);
    }

    #[test]
    fn fk_type_compares_both_ends() {
        let fk = |ref_schema: Option<&str>| ForeignKey {
            name: None,
            columns: vec!["pid".into()],
            ref_schema: ref_schema.map(str::to_string),
            ref_table: "p".into(),
            ref_columns: vec!["id".into()],
            on_delete: None,
            on_update: None,
        };
        let parent = table("p", vec![int("id")]);
        let mut bad = table("c", vec![varchar("pid", 10)]);
        bad.foreign_keys = vec![fk(None)];
        assert_eq!(
            objects(&tables(vec![parent.clone(), bad.clone()]), "consistency.fk_type", json!({})),
            ["c.pid"]
        );
        bad.foreign_keys = vec![fk(Some("other"))];
        assert!(objects(&tables(vec![parent.clone(), bad]), "consistency.fk_type", json!({})).is_empty());
        let mut good = table("c", vec![int("pid")]);
        good.foreign_keys = vec![fk(None)];
        assert!(objects(&tables(vec![parent, good]), "consistency.fk_type", json!({})).is_empty());
    }
}
