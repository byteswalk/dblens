//! 表和主键规则。

use dblens_core::schema::{Column, Table};
use dblens_core::{LogicalType, Msg, msg};
use dblens_dialect::Family;

use super::{Check, Ctx, Finding, ParamDef, ParamDefault, Params, RuleDef, same_name, upper};

pub(super) const RULES: &[RuleDef] = &[
    RuleDef {
        id: "table.primary_key",
        params: &[],
        fix: Some(|| msg!("lint.fix.add_primary_key")),
        check: primary_key,
    },
    RuleDef {
        id: "table.no_primary_key",
        params: &[],
        fix: Some(|| msg!("lint.fix.drop_primary_key")),
        check: no_primary_key,
    },
    RuleDef { id: "table.comment", params: &[], fix: Some(|| msg!("lint.fix.add_comment")), check: comment },
    RuleDef {
        id: "table.max_columns",
        params: &[ParamDef { key: "max", default: ParamDefault::Num(200) }],
        fix: Some(|| msg!("lint.fix.split_table")),
        check: max_columns,
    },
    RuleDef {
        id: "table.char_bytes",
        params: &[
            ParamDef { key: "max", default: ParamDefault::Num(8092) },
            ParamDef { key: "bytes_per_char", default: ParamDefault::Num(1) },
        ],
        fix: Some(|| msg!("lint.fix.split_table")),
        check: char_bytes,
    },
    RuleDef {
        id: "table.required_columns",
        params: &[ParamDef { key: "fields", default: ParamDefault::List(&[]) }],
        fix: Some(|| msg!("lint.fix.add_column")),
        check: required_columns,
    },
    RuleDef {
        id: "table.backup_leftover",
        params: &[ParamDef { key: "suffixes", default: ParamDefault::List(&["_BAK", "_BACKUP", "_OLD"]) }],
        fix: Some(|| msg!("lint.fix.drop_backup")),
        check: backup_leftover,
    },
    RuleDef {
        id: "table.reserved_columns",
        params: &[],
        fix: Some(|| msg!("lint.fix.drop_reserved")),
        check: reserved_columns,
    },
    RuleDef {
        id: "table.all_nullable",
        params: &[],
        fix: Some(|| msg!("lint.fix.not_null")),
        check: all_nullable,
    },
    RuleDef {
        id: "table.row_count",
        params: &[ParamDef { key: "max", default: ParamDefault::Num(20_000_000) }],
        fix: None,
        check: |_, _| Check::Unknown(msg!("lint.unknown.needs_data")),
    },
    RuleDef {
        id: "pk.auto_increment",
        params: &[],
        fix: Some(|| msg!("lint.fix.ordered_key")),
        check: pk_auto_increment,
    },
    RuleDef {
        id: "pk.auto_increment_step",
        params: &[ParamDef { key: "step", default: ParamDefault::Num(1) }],
        fix: None,
        check: pk_step,
    },
    RuleDef { id: "pk.on_update_timestamp", params: &[], fix: None, check: pk_on_update },
];

fn per_table(ctx: &Ctx, bad: impl Fn(&Table) -> Option<Msg>) -> Check {
    Check::Findings(ctx.tables().iter().filter_map(|t| bad(t).map(|m| Finding::table(t, m))).collect())
}

fn primary_key(ctx: &Ctx, _: &Params) -> Check {
    per_table(ctx, |t| t.primary_key.is_none().then(|| msg!("lint.issue.no_primary_key")))
}

fn no_primary_key(ctx: &Ctx, _: &Params) -> Check {
    per_table(ctx, |t| t.primary_key.is_some().then(|| msg!("lint.issue.has_primary_key")))
}

fn comment(ctx: &Ctx, _: &Params) -> Check {
    per_table(ctx, |t| {
        t.comment.as_deref().is_none_or(|c| c.trim().is_empty()).then(|| msg!("lint.issue.no_table_comment"))
    })
}

fn max_columns(ctx: &Ctx, p: &Params) -> Check {
    let max = p.num("max") as usize;
    per_table(ctx, |t| {
        (t.columns.len() >= max)
            .then(|| msg!("lint.issue.too_many_columns", count = t.columns.len(), max = max))
    })
}

fn char_bytes(ctx: &Ctx, p: &Params) -> Check {
    let (max, per_char) = (p.num("max"), p.num("bytes_per_char").max(1));
    per_table(ctx, |t| {
        let total: u64 = t
            .columns
            .iter()
            .filter_map(|c| match c.data_type.logical {
                LogicalType::Char { len } | LogicalType::VarChar { len } => len,
                _ => None,
            })
            .map(|len| u64::from(len) * per_char)
            .sum();
        (total > max).then(|| msg!("lint.issue.char_bytes", total = total, max = max))
    })
}

fn required_columns(ctx: &Ctx, p: &Params) -> Check {
    // 每一项是一组可互换的字段名，用 | 分隔
    let fields = p.list("fields");
    if fields.is_empty() {
        return Check::NotApplicable;
    }
    let mut findings = vec![];
    for t in ctx.tables() {
        for field in &fields {
            let found =
                field.split('|').any(|name| t.columns.iter().any(|c| same_name(&c.name, name.trim())));
            if !found {
                findings.push(Finding::table(t, msg!("lint.issue.missing_column", column = field)));
            }
        }
    }
    Check::Findings(findings)
}

/// 名字以 6 或 8 位日期结尾，如 `ORDERS_20230313`、`ORDERS_XF20230313`。
fn ends_with_date(name: &str) -> bool {
    let digits = name.chars().rev().take_while(char::is_ascii_digit).count();
    (digits == 6 || digits == 8) && digits < name.len()
}

fn backup_leftover(ctx: &Ctx, p: &Params) -> Check {
    let suffixes: Vec<String> = p.list("suffixes").iter().map(|s| upper(s)).collect();
    per_table(ctx, |t| {
        let name = upper(&t.name);
        (suffixes.iter().any(|s| name.ends_with(s)) || ends_with_date(&name))
            .then(|| msg!("lint.issue.backup_table"))
    })
}

/// 预留字段：`RESERVED`、`RESERVE1`、`SPARE2`、`EXT1`、`ATTR3` 这类没有业务含义的名字。
fn is_reserved_column(name: &str) -> bool {
    let name = upper(name);
    let base = name.trim_end_matches(|c: char| c.is_ascii_digit()).trim_end_matches('_');
    let numbered = base.len() < name.len();
    matches!(base, "RESERVED" | "RESERVE" | "RSRV" | "SPARE")
        || (numbered && matches!(base, "EXT" | "EXTEND" | "ATTR" | "ATTRIBUTE" | "FIELD" | "COL" | "BAK"))
}

fn reserved_columns(ctx: &Ctx, _: &Params) -> Check {
    let mut findings = vec![];
    for t in ctx.tables() {
        for c in t.columns.iter().filter(|c| is_reserved_column(&c.name)) {
            findings.push(Finding::column(t, c, msg!("lint.issue.reserved_column")));
        }
    }
    Check::Findings(findings)
}

fn all_nullable(ctx: &Ctx, _: &Params) -> Check {
    per_table(ctx, |t| {
        (!t.columns.is_empty() && t.columns.iter().all(|c| c.nullable))
            .then(|| msg!("lint.issue.all_nullable"))
    })
}

fn pk_columns(t: &Table) -> impl Iterator<Item = &Column> {
    let keys = t.primary_key.iter().flat_map(|k| k.columns.iter());
    keys.filter_map(|k| t.columns.iter().find(|c| same_name(&c.name, k)))
}

fn pk_auto_increment(ctx: &Ctx, _: &Params) -> Check {
    let mut findings = vec![];
    for t in ctx.tables() {
        for c in pk_columns(t).filter(|c| c.auto_increment) {
            findings.push(Finding::column(t, c, msg!("lint.issue.pk_auto_increment")));
        }
    }
    Check::Findings(findings)
}

fn pk_step(ctx: &Ctx, _: &Params) -> Check {
    if ctx.tables().iter().any(|t| pk_columns(t).any(|c| c.auto_increment)) {
        Check::Unknown(msg!("lint.unknown.no_metadata"))
    } else {
        Check::Findings(vec![])
    }
}

/// `ON UPDATE` 只有 MySQL 有，现有元数据没有取。
fn pk_on_update(ctx: &Ctx, _: &Params) -> Check {
    if ctx.family() != Family::MySql {
        return Check::NotApplicable;
    }
    let timestamp_key = ctx
        .tables()
        .iter()
        .any(|t| pk_columns(t).any(|c| matches!(c.data_type.logical, LogicalType::Timestamp { .. })));
    if timestamp_key { Check::Unknown(msg!("lint.unknown.no_metadata")) } else { Check::Findings(vec![]) }
}

#[cfg(test)]
mod tests {
    use dblens_core::LogicalType;
    use serde_json::json;

    use super::*;
    use crate::rules::Outcome;
    use crate::rules::testing::*;

    #[test]
    fn primary_key_rules_are_opposites() {
        let mut with = table("a", vec![int("id")]);
        with.primary_key = pk(&["id"]);
        let i = tables(vec![with, table("b", vec![int("id")])]);
        assert_eq!(objects(&i, "table.primary_key", json!({})), ["b"]);
        assert_eq!(objects(&i, "table.no_primary_key", json!({})), ["a"]);
    }

    #[test]
    fn comment_must_not_be_blank() {
        let mut ok = table("a", vec![]);
        ok.comment = Some("x".into());
        let mut blank = table("b", vec![]);
        blank.comment = Some("  ".into());
        let i = tables(vec![ok, blank, table("c", vec![])]);
        assert_eq!(objects(&i, "table.comment", json!({})), ["b", "c"]);
    }

    #[test]
    fn max_columns_must_stay_below_the_limit() {
        let i = tables(vec![table("a", vec![int("x"), int("y")]), table("b", vec![int("x")])]);
        assert_eq!(objects(&i, "table.max_columns", json!({"max": 2})), ["a"]);
    }

    #[test]
    fn char_bytes_adds_character_columns_only() {
        let cols = vec![varchar("a", 5000), varchar("b", 3093), col("t", "text", LogicalType::Text)];
        let i = tables(vec![table("big", cols), table("small", vec![varchar("a", 8092)])]);
        assert_eq!(objects(&i, "table.char_bytes", json!({})), ["big"]);
        assert_eq!(objects(&i, "table.char_bytes", json!({"bytes_per_char": 3})).len(), 2);
    }

    #[test]
    fn required_columns_accept_alternatives() {
        let i = tables(vec![
            table("a", vec![int("CREATE_TIME")]),
            table("b", vec![int("created_at")]),
            table("c", vec![]),
        ]);
        let got = objects(&i, "table.required_columns", json!({"fields": ["create_time|created_at"]}));
        assert_eq!(got, ["c"]);
        assert!(matches!(outcome(&i, "table.required_columns", json!({})), Outcome::NotApplicable));
    }

    #[test]
    fn backup_leftover_matches_suffixes_and_dates() {
        let names = ["orders", "orders_bak", "ORDERS_XF20230313", "orders_202301", "t123"];
        let i = tables(names.iter().map(|n| table(n, vec![])).collect());
        let got = objects(&i, "table.backup_leftover", json!({}));
        assert_eq!(got, ["orders_bak", "ORDERS_XF20230313", "orders_202301"]);
    }

    #[test]
    fn reserved_columns_need_a_number_when_the_word_is_common() {
        let cols = ["reserved1", "RESERVE_2", "spare", "ext1", "ext", "attr3", "field", "user_field1"];
        let i = tables(vec![table("t", cols.iter().map(|n| int(n)).collect())]);
        let got = objects(&i, "table.reserved_columns", json!({}));
        assert_eq!(got, ["t.reserved1", "t.RESERVE_2", "t.spare", "t.ext1", "t.attr3"]);
    }

    #[test]
    fn all_nullable_needs_every_column_nullable() {
        let mut a = int("a");
        a.nullable = true;
        let i = tables(vec![table("x", vec![a.clone()]), table("y", vec![a, int("b")]), table("z", vec![])]);
        assert_eq!(objects(&i, "table.all_nullable", json!({})), ["x"]);
    }

    #[test]
    fn auto_increment_key_is_flagged_and_step_is_unknown() {
        let mut id = int("id");
        id.auto_increment = true;
        let mut t = table("a", vec![id]);
        t.primary_key = pk(&["id"]);
        let i = tables(vec![t]);
        assert_eq!(objects(&i, "pk.auto_increment", json!({})), ["a.id"]);
        assert!(matches!(outcome(&i, "pk.auto_increment_step", json!({})), Outcome::Unknown(_)));
        let none = tables(vec![table("b", vec![int("id")])]);
        assert!(objects(&none, "pk.auto_increment_step", json!({})).is_empty());
    }

    #[test]
    fn on_update_key_is_unknown_on_mysql_only() {
        let ts = LogicalType::Timestamp { tz: false, precision: None };
        let mut t = table("a", vec![col("ts", "timestamp", ts)]);
        t.primary_key = pk(&["ts"]);
        let mysql = input(schema(vec![t.clone()]), Family::MySql);
        assert!(matches!(outcome(&mysql, "pk.on_update_timestamp", json!({})), Outcome::Unknown(_)));
        let pg = input(schema(vec![t]), Family::Postgres);
        assert!(matches!(outcome(&pg, "pk.on_update_timestamp", json!({})), Outcome::NotApplicable));
    }
}
