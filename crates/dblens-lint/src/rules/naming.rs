//! 命名规则。

use dblens_core::schema::{RoutineKind, Table};
use dblens_core::{LogicalType, Msg, msg};
use dblens_dialect::Family;
use dblens_dialect::ident::{is_reserved_word, needs_quote};

use super::index::backs_constraint;
use super::{Check, Ctx, Finding, ParamDef, ParamDefault, Params, RuleDef, upper};
use crate::pinyin;

const TARGETS: &[&str] = &["all", "table", "column", "index", "other", "schema"];
const PREFIX_TARGETS: &[&str] =
    &["primary_key", "unique", "index", "sequence", "procedure", "function", "view"];

pub(super) const RULES: &[RuleDef] = &[
    RuleDef { id: "name.charset", params: &[], fix: Some(|| msg!("lint.fix.rename")), check: charset },
    RuleDef {
        id: "name.max_length",
        params: &[
            ParamDef { key: "target", default: ParamDefault::Choice(TARGETS) },
            ParamDef { key: "max", default: ParamDefault::Num(40) },
        ],
        fix: Some(|| msg!("lint.fix.shorten")),
        check: max_length,
    },
    RuleDef {
        id: "name.case",
        params: &[ParamDef { key: "case", default: ParamDefault::Choice(&["auto", "upper", "lower"]) }],
        fix: Some(|| msg!("lint.fix.rename")),
        check: case,
    },
    RuleDef { id: "name.reserved", params: &[], fix: Some(|| msg!("lint.fix.rename")), check: reserved },
    RuleDef { id: "name.quoted", params: &[], fix: Some(|| msg!("lint.fix.rename")), check: quoted },
    RuleDef {
        id: "name.pinyin",
        params: &[],
        fix: Some(|| msg!("lint.fix.english_name")),
        check: pinyin_name,
    },
    RuleDef {
        id: "name.prefix",
        params: &[
            ParamDef { key: "target", default: ParamDefault::Choice(PREFIX_TARGETS) },
            ParamDef { key: "prefix", default: ParamDefault::List(&[]) },
        ],
        fix: Some(|| msg!("lint.fix.rename")),
        check: prefix,
    },
    RuleDef {
        id: "name.temporal_suffix",
        params: &[
            ParamDef { key: "date", default: ParamDefault::List(&["DATE"]) },
            ParamDef { key: "time", default: ParamDefault::List(&["TIME"]) },
        ],
        fix: Some(|| msg!("lint.fix.rename")),
        check: temporal_suffix,
    },
    RuleDef {
        id: "name.required_prefix",
        params: &[
            ParamDef { key: "target", default: ParamDefault::Choice(TARGETS) },
            ParamDef { key: "prefix", default: ParamDefault::Text("") },
        ],
        fix: Some(|| msg!("lint.fix.rename")),
        check: required_prefix,
    },
];

struct Named {
    object: String,
    table: Option<String>,
    name: String,
}

fn of_table(t: &Table) -> Named {
    Named { object: t.name.clone(), table: Some(t.name.clone()), name: t.name.clone() }
}

fn of_child(t: &Table, name: &str) -> Named {
    Named { object: format!("{}.{name}", t.name), table: Some(t.name.clone()), name: name.to_string() }
}

fn of_object(name: &str) -> Named {
    Named { object: name.to_string(), table: None, name: name.to_string() }
}

/// 主键和唯一约束的名字，再加上不是约束自带的索引。
fn key_names(t: &Table) -> Vec<Named> {
    let keys = t.primary_key.iter().filter_map(|k| k.name.as_deref());
    let uniques = t.uniques.iter().filter_map(|u| u.name.as_deref());
    let indexes = t.indexes.iter().filter(|i| !backs_constraint(t, i)).map(|i| i.name.as_str());
    keys.chain(uniques).chain(indexes).filter(|n| !is_generated(n)).map(|n| of_child(t, n)).collect()
}

/// MySQL 的主键索引固定叫 PRIMARY，不是用户起的名字。
fn is_generated(name: &str) -> bool {
    name.eq_ignore_ascii_case("PRIMARY")
}

fn others(ctx: &Ctx) -> Vec<Named> {
    let s = &ctx.input.schema;
    let views = s.views.iter().chain(&s.materialized_views).map(|v| v.name.as_str());
    let sequences = s.sequences.iter().map(|q| q.name.as_str());
    let routines = s.routines.iter().map(|r| r.name.as_str());
    views.chain(sequences).chain(routines).map(of_object).collect()
}

fn names(ctx: &Ctx, target: &str) -> Vec<Named> {
    let tables = ctx.tables();
    let table_names = || tables.iter().map(of_table).collect::<Vec<_>>();
    let columns = || {
        tables.iter().flat_map(|t| t.columns.iter().map(move |c| of_child(t, &c.name))).collect::<Vec<_>>()
    };
    let indexes = || tables.iter().flat_map(key_names).collect::<Vec<_>>();
    match target {
        "table" => table_names(),
        "column" => columns(),
        "index" => indexes(),
        "other" => others(ctx),
        "schema" => vec![of_object(&ctx.input.schema.name)],
        _ => {
            let mut all = table_names();
            all.extend(columns());
            all.extend(indexes());
            all.extend(others(ctx));
            all
        }
    }
}

fn report(named: Vec<Named>, bad: impl Fn(&str) -> Option<Msg>) -> Check {
    Check::Findings(
        named
            .into_iter()
            .filter_map(|n| bad(&n.name).map(|message| Finding { object: n.object, table: n.table, message }))
            .collect(),
    )
}

fn charset(ctx: &Ctx, _: &Params) -> Check {
    report(names(ctx, "all"), |name| {
        let ok = name.starts_with(|c: char| c.is_ascii_alphabetic())
            && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
        (!ok).then(|| msg!("lint.issue.charset"))
    })
}

fn max_length(ctx: &Ctx, p: &Params) -> Check {
    let max = p.num("max") as usize;
    report(names(ctx, &p.text("target")), |name| {
        let len = name.chars().count();
        (len > max).then(|| msg!("lint.issue.too_long", len = len, max = max))
    })
}

fn case(ctx: &Ctx, p: &Params) -> Check {
    let wanted = match p.text("case").as_str() {
        "upper" => true,
        "lower" => false,
        // 按数据库的习惯：Oracle、达梦大写，MySQL、PostgreSQL 小写，SQL Server 不查
        _ => match ctx.family() {
            Family::Oracle | Family::Dameng => true,
            Family::MySql | Family::Postgres | Family::Informix => false,
            Family::SqlServer => return Check::NotApplicable,
        },
    };
    report(names(ctx, "all"), |name| {
        if wanted {
            name.chars().any(|c| c.is_ascii_lowercase()).then(|| msg!("lint.issue.not_upper"))
        } else {
            name.chars().any(|c| c.is_ascii_uppercase()).then(|| msg!("lint.issue.not_lower"))
        }
    })
}

fn reserved(ctx: &Ctx, _: &Params) -> Check {
    let family = ctx.family();
    report(names(ctx, "all"), |name| is_reserved_word(family, name).then(|| msg!("lint.issue.reserved")))
}

fn quoted(ctx: &Ctx, _: &Params) -> Check {
    let family = ctx.family();
    report(names(ctx, "all"), |name| {
        (needs_quote(family, name) && !is_reserved_word(family, name)).then(|| msg!("lint.issue.quoted"))
    })
}

fn pinyin_name(ctx: &Ctx, _: &Params) -> Check {
    let mut named = names(ctx, "table");
    named.extend(names(ctx, "column"));
    report(named, |name| pinyin::looks_like_pinyin(name).then(|| msg!("lint.issue.pinyin")))
}

fn prefix(ctx: &Ctx, p: &Params) -> Check {
    let prefixes: Vec<String> = p.list("prefix").iter().map(|s| upper(s)).collect();
    if prefixes.is_empty() {
        return Check::NotApplicable;
    }
    let shown = prefixes.join(", ");
    let s = &ctx.input.schema;
    let target = p.text("target");
    let named: Vec<Named> = match target.as_str() {
        "primary_key" => ctx
            .tables()
            .iter()
            .filter_map(|t| t.primary_key.as_ref()?.name.as_deref().map(|n| of_child(t, n)))
            .filter(|n| !is_generated(&n.name))
            .collect(),
        "unique" => ctx.tables().iter().flat_map(unique_names).collect(),
        "index" => ctx
            .tables()
            .iter()
            .flat_map(|t| {
                t.indexes
                    .iter()
                    .filter(|i| !i.unique && !backs_constraint(t, i))
                    .map(move |i| of_child(t, &i.name))
            })
            .collect(),
        "sequence" => s.sequences.iter().map(|q| of_object(&q.name)).collect(),
        "procedure" => routines(ctx, RoutineKind::Procedure),
        "function" => routines(ctx, RoutineKind::Function),
        _ => s.views.iter().chain(&s.materialized_views).map(|v| of_object(&v.name)).collect(),
    };
    report(named, |name| {
        let name = upper(name);
        (!prefixes.iter().any(|x| name.starts_with(x))).then(|| msg!("lint.issue.prefix", prefix = shown))
    })
}

fn unique_names(t: &Table) -> Vec<Named> {
    let mut out: Vec<String> = t.uniques.iter().filter_map(|u| u.name.clone()).collect();
    for i in t.indexes.iter().filter(|i| i.unique && !is_generated(&i.name)) {
        let is_pk = t.primary_key.as_ref().and_then(|k| k.name.as_deref()) == Some(i.name.as_str());
        if !is_pk && !out.iter().any(|n| n.eq_ignore_ascii_case(&i.name)) {
            out.push(i.name.clone());
        }
    }
    out.iter().map(|n| of_child(t, n)).collect()
}

fn routines(ctx: &Ctx, kind: RoutineKind) -> Vec<Named> {
    ctx.input.schema.routines.iter().filter(|r| r.kind == kind).map(|r| of_object(&r.name)).collect()
}

fn temporal_suffix(ctx: &Ctx, p: &Params) -> Check {
    let date: Vec<String> = p.list("date").iter().map(|s| upper(s)).collect();
    let time: Vec<String> = p.list("time").iter().map(|s| upper(s)).collect();
    // Oracle、达梦的 DATE 带时分秒，两种后缀都算对
    let loose = matches!(ctx.family(), Family::Oracle | Family::Dameng);
    let mut findings = vec![];
    for t in ctx.tables() {
        for c in &t.columns {
            let wanted = match c.data_type.logical {
                LogicalType::Date => &date,
                LogicalType::Timestamp { .. } => &time,
                _ => continue,
            };
            let name = upper(&c.name);
            let ok = wanted.iter().any(|s| name.ends_with(s))
                || (loose && date.iter().chain(&time).any(|s| name.ends_with(s)));
            if !ok {
                findings.push(Finding::column(
                    t,
                    c,
                    msg!("lint.issue.temporal_suffix", suffix = wanted.join(", ")),
                ));
            }
        }
    }
    Check::Findings(findings)
}

fn required_prefix(ctx: &Ctx, p: &Params) -> Check {
    let wanted = upper(&p.text("prefix"));
    if wanted.is_empty() {
        return Check::NotApplicable;
    }
    report(names(ctx, &p.text("target")), |name| {
        (!upper(name).starts_with(&wanted)).then(|| msg!("lint.issue.prefix", prefix = wanted))
    })
}

#[cfg(test)]
mod tests {
    use dblens_core::LogicalType;
    use dblens_core::schema::{PrimaryKey, Schema, Sequence, Table, View};
    use serde_json::json;

    use super::*;
    use crate::rules::Outcome;
    use crate::rules::testing::*;

    fn named(name: &str) -> Table {
        table(name, vec![])
    }

    #[test]
    fn charset_accepts_plain_names_only() {
        let i = tables(vec![named("orders"), named("order-items"), named("1st"), named("_x")]);
        assert_eq!(objects(&i, "name.charset", json!({})), ["order-items", "1st", "_x"]);
    }

    #[test]
    fn max_length_applies_to_the_target_only() {
        let long = "c".repeat(41);
        let i = tables(vec![table(&"t".repeat(41), vec![int(&long)])]);
        let by = |target| objects(&i, "name.max_length", json!({"target": target, "max": 40})).len();
        assert_eq!((by("table"), by("column"), by("index"), by("all")), (1, 1, 0, 2));
        assert!(objects(&i, "name.max_length", json!({"target": "table", "max": 41})).is_empty());
    }

    #[test]
    fn schema_length_checks_the_schema_name() {
        let s = Schema { name: "s".repeat(31), ..Default::default() };
        let i = input(s, Family::Oracle);
        assert_eq!(objects(&i, "name.max_length", json!({"target": "schema", "max": 30})).len(), 1);
    }

    #[test]
    fn case_auto_follows_the_database() {
        let s = || schema(vec![named("Orders")]);
        assert_eq!(objects(&input(s(), Family::Postgres), "name.case", json!({})).len(), 1);
        assert_eq!(objects(&input(s(), Family::Oracle), "name.case", json!({})).len(), 1);
        let upper_ok = input(schema(vec![named("ORDERS")]), Family::Oracle);
        assert!(objects(&upper_ok, "name.case", json!({})).is_empty());
        let sql = input(s(), Family::SqlServer);
        assert!(matches!(outcome(&sql, "name.case", json!({})), Outcome::NotApplicable));
        assert_eq!(objects(&sql, "name.case", json!({"case": "upper"})).len(), 1);
    }

    #[test]
    fn reserved_and_quoted_do_not_double_report() {
        let i = input(schema(vec![named("order"), named("orders"), named("Orders")]), Family::Postgres);
        assert_eq!(objects(&i, "name.reserved", json!({})), ["order"]);
        assert_eq!(objects(&i, "name.quoted", json!({})), ["Orders"]);
    }

    #[test]
    fn pinyin_flags_tables_and_columns() {
        let i = tables(vec![table("xingming_biao", vec![int("shouji"), int("user_name")])]);
        assert_eq!(objects(&i, "name.pinyin", json!({})), ["xingming_biao", "xingming_biao.shouji"]);
    }

    #[test]
    fn prefix_skips_generated_primary_and_constraint_indexes() {
        let mut t = named("t");
        t.primary_key = Some(PrimaryKey { name: Some("PRIMARY".into()), columns: vec!["a".into()] });
        t.indexes = vec![index("PRIMARY", &["a"]), index("IX_T", &["b"]), index("t_b", &["b"])];
        let i = tables(vec![t]);
        assert_eq!(objects(&i, "name.prefix", json!({"target": "index", "prefix": ["IX_"]})), ["t.t_b"]);
        assert!(objects(&i, "name.prefix", json!({"target": "primary_key", "prefix": ["PK_"]})).is_empty());
    }

    #[test]
    fn prefix_is_case_insensitive_and_covers_objects() {
        let mut s = schema(vec![]);
        s.views = vec![View { name: "v_a".into(), comment: None, definition: None, columns: vec![] }];
        s.sequences = vec![Sequence {
            name: "ids".into(),
            start: None,
            increment: None,
            min_value: None,
            max_value: None,
            cycle: false,
            cache: None,
        }];
        let i = input(s, Family::Postgres);
        assert!(objects(&i, "name.prefix", json!({"target": "view", "prefix": ["V_"]})).is_empty());
        assert_eq!(objects(&i, "name.prefix", json!({"target": "sequence", "prefix": ["SEQ_"]})), ["ids"]);
    }

    #[test]
    fn prefix_without_a_value_is_not_applicable() {
        let i = tables(vec![named("t")]);
        assert!(matches!(outcome(&i, "name.prefix", json!({"target": "view"})), Outcome::NotApplicable));
    }

    #[test]
    fn temporal_suffix_checks_date_and_timestamp_columns() {
        let ts = LogicalType::Timestamp { tz: false, precision: None };
        let cols = vec![
            col("start_date", "date", LogicalType::Date),
            col("start_day", "date", LogicalType::Date),
            col("create_time", "timestamp", ts.clone()),
            col("create_at", "timestamp", ts.clone()),
            int("amount"),
        ];
        let i = tables(vec![table("t", cols)]);
        assert_eq!(objects(&i, "name.temporal_suffix", json!({})), ["t.start_day", "t.create_at"]);
        assert_eq!(objects(&i, "name.temporal_suffix", json!({"time": ["TIME", "AT"]})), ["t.start_day"]);
        let oracle = input(
            schema(vec![table("t", vec![col("create_time", "date", LogicalType::Date)])]),
            Family::Oracle,
        );
        assert!(objects(&oracle, "name.temporal_suffix", json!({})).is_empty());
    }

    #[test]
    fn required_prefix_is_off_until_configured() {
        let i = tables(vec![named("orders"), named("app_orders")]);
        assert!(matches!(
            outcome(&i, "name.required_prefix", json!({"target": "table"})),
            Outcome::NotApplicable
        ));
        let got = objects(&i, "name.required_prefix", json!({"target": "table", "prefix": "app_"}));
        assert_eq!(got, ["orders"]);
    }
}
