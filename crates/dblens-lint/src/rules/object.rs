//! 外键、触发器、视图等数据库对象的规则，以及现有元数据取不到的规则。

use dblens_core::msg;
use dblens_core::schema::RoutineKind;
use dblens_dialect::Family;

use super::{Check, Ctx, Finding, Params, RuleDef};

pub(super) const RULES: &[RuleDef] = &[
    RuleDef {
        id: "constraint.foreign_key",
        params: &[],
        fix: Some(|| msg!("lint.fix.drop_foreign_key")),
        check: foreign_key,
    },
    RuleDef {
        id: "constraint.trigger",
        params: &[],
        fix: Some(|| msg!("lint.fix.drop_trigger")),
        check: trigger,
    },
    RuleDef { id: "constraint.view", params: &[], fix: Some(|| msg!("lint.fix.review_object")), check: view },
    RuleDef {
        id: "constraint.function",
        params: &[],
        fix: Some(|| msg!("lint.fix.review_object")),
        check: function,
    },
    RuleDef {
        id: "constraint.procedure",
        params: &[],
        fix: Some(|| msg!("lint.fix.review_object")),
        check: procedure,
    },
    RuleDef {
        id: "constraint.sequence",
        params: &[],
        fix: Some(|| msg!("lint.fix.review_object")),
        check: sequence,
    },
    RuleDef {
        id: "constraint.dblink",
        params: &[],
        fix: Some(|| msg!("lint.fix.review_object")),
        check: dblink,
    },
    RuleDef { id: "db.charset", params: &[], fix: None, check: no_metadata },
    RuleDef { id: "db.engine", params: &[], fix: None, check: no_metadata },
    RuleDef { id: "db.tablespace", params: &[], fix: None, check: no_metadata },
    RuleDef { id: "db.grants", params: &[], fix: None, check: no_metadata },
    RuleDef { id: "db.event", params: &[], fix: None, check: no_metadata },
    RuleDef { id: "db.temporary_table", params: &[], fix: None, check: no_metadata },
];

fn no_metadata(_: &Ctx, _: &Params) -> Check {
    Check::Unknown(msg!("lint.unknown.no_metadata"))
}

/// 模型文件只有表，查不了其他对象。
fn need_objects(ctx: &Ctx, check: impl FnOnce() -> Vec<Finding>) -> Check {
    if ctx.input.objects_known {
        Check::Findings(check())
    } else {
        Check::Unknown(msg!("lint.unknown.model_objects"))
    }
}

fn foreign_key(ctx: &Ctx, _: &Params) -> Check {
    let mut findings = vec![];
    for t in ctx.tables() {
        for fk in &t.foreign_keys {
            let name = fk.name.as_deref().unwrap_or(&fk.ref_table);
            let object = format!("{}.{name}", t.name);
            let message = msg!("lint.issue.foreign_key", table = fk.ref_table);
            findings.push(Finding { object, table: Some(t.name.clone()), message });
        }
    }
    Check::Findings(findings)
}

fn trigger(ctx: &Ctx, _: &Params) -> Check {
    need_objects(ctx, || {
        let mut findings = vec![];
        for t in ctx.tables() {
            for g in &t.triggers {
                let object = format!("{}.{}", t.name, g.name);
                findings.push(Finding {
                    object,
                    table: Some(t.name.clone()),
                    message: msg!("lint.issue.trigger"),
                });
            }
        }
        findings
    })
}

fn view(ctx: &Ctx, _: &Params) -> Check {
    need_objects(ctx, || {
        let s = &ctx.input.schema;
        s.views
            .iter()
            .chain(&s.materialized_views)
            .map(|v| Finding::object(&v.name, msg!("lint.issue.view")))
            .collect()
    })
}

fn routines(ctx: &Ctx, kind: RoutineKind) -> Check {
    need_objects(ctx, || {
        let found = ctx.input.schema.routines.iter().filter(|r| r.kind == kind);
        found.map(|r| Finding::object(&r.name, msg!("lint.issue.routine"))).collect()
    })
}

fn function(ctx: &Ctx, _: &Params) -> Check {
    routines(ctx, RoutineKind::Function)
}

fn procedure(ctx: &Ctx, _: &Params) -> Check {
    routines(ctx, RoutineKind::Procedure)
}

fn sequence(ctx: &Ctx, _: &Params) -> Check {
    need_objects(ctx, || {
        ctx.input
            .schema
            .sequences
            .iter()
            .map(|q| Finding::object(&q.name, msg!("lint.issue.sequence")))
            .collect()
    })
}

/// 只有 Oracle 能从同义词看到 DBLINK，别的库取不到。
fn dblink(ctx: &Ctx, _: &Params) -> Check {
    if ctx.family() != Family::Oracle {
        return Check::Unknown(msg!("lint.unknown.no_metadata"));
    }
    need_objects(ctx, || {
        let linked = ctx.input.schema.synonyms.iter().filter(|s| s.db_link.is_some());
        linked.map(|s| Finding::object(&s.name, msg!("lint.issue.dblink"))).collect()
    })
}

#[cfg(test)]
mod tests {
    use dblens_core::schema::{ForeignKey, Routine, Schema, Sequence, Synonym, Trigger, View};
    use serde_json::json;

    use super::*;
    use crate::rules::Outcome;
    use crate::rules::testing::*;

    fn rich() -> Schema {
        let mut t = table("t", vec![]);
        t.foreign_keys = vec![ForeignKey {
            name: None,
            columns: vec!["a".into()],
            ref_schema: None,
            ref_table: "p".into(),
            ref_columns: vec!["id".into()],
            on_delete: None,
            on_update: None,
        }];
        t.triggers = vec![Trigger { name: "g".into(), timing: None, events: vec![], definition: None }];
        let mut s = schema(vec![t]);
        s.views = vec![View { name: "v".into(), comment: None, definition: None, columns: vec![] }];
        s.routines = vec![
            Routine { name: "f".into(), kind: RoutineKind::Function, source: None },
            Routine { name: "p".into(), kind: RoutineKind::Procedure, source: None },
        ];
        s.sequences = vec![Sequence {
            name: "q".into(),
            start: None,
            increment: None,
            min_value: None,
            max_value: None,
            cycle: false,
            cache: None,
        }];
        s.synonyms = vec![
            Synonym {
                name: "s1".into(),
                target_schema: None,
                target_name: "x".into(),
                db_link: Some("remote".into()),
            },
            Synonym { name: "s2".into(), target_schema: None, target_name: "x".into(), db_link: None },
        ];
        s
    }

    #[test]
    fn objects_are_reported() {
        let i = input(rich(), Family::Oracle);
        assert_eq!(objects(&i, "constraint.foreign_key", json!({})), ["t.p"]);
        assert_eq!(objects(&i, "constraint.trigger", json!({})), ["t.g"]);
        assert_eq!(objects(&i, "constraint.view", json!({})), ["v"]);
        assert_eq!(objects(&i, "constraint.function", json!({})), ["f"]);
        assert_eq!(objects(&i, "constraint.procedure", json!({})), ["p"]);
        assert_eq!(objects(&i, "constraint.sequence", json!({})), ["q"]);
        assert_eq!(objects(&i, "constraint.dblink", json!({})), ["s1"]);
    }

    #[test]
    fn nothing_found_is_clean() {
        let i = tables(vec![]);
        assert!(objects(&i, "constraint.view", json!({})).is_empty());
    }

    #[test]
    fn models_cannot_answer_object_rules() {
        let mut i = input(rich(), Family::Oracle);
        i.objects_known = false;
        let ids = [
            "constraint.view",
            "constraint.function",
            "constraint.sequence",
            "constraint.dblink",
            "constraint.trigger",
        ];
        for id in ids {
            assert!(matches!(outcome(&i, id, json!({})), Outcome::Unknown(_)), "{id}");
        }
        assert_eq!(objects(&i, "constraint.foreign_key", json!({})).len(), 1);
    }

    #[test]
    fn missing_metadata_is_unknown() {
        let i = tables(vec![]);
        let ids = ["db.charset", "db.engine", "db.tablespace", "db.grants", "db.event", "db.temporary_table"];
        for id in ids.iter().chain(&["constraint.dblink"]) {
            assert!(matches!(outcome(&i, id, json!({})), Outcome::Unknown(_)), "{id}");
        }
    }
}
