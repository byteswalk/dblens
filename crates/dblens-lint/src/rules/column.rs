//! 字段规则。

use dblens_core::schema::{Column, Table};
use dblens_core::{LogicalType, Msg, msg};
use dblens_dialect::Family;

use super::{Check, Ctx, Finding, ParamDef, ParamDefault, Params, RuleDef, same_name, upper};

pub(super) const RULES: &[RuleDef] = &[
    RuleDef { id: "column.comment", params: &[], fix: Some(|| msg!("lint.fix.add_comment")), check: comment },
    RuleDef {
        id: "column.comment_differs",
        params: &[],
        fix: Some(|| msg!("lint.fix.add_comment")),
        check: comment_differs,
    },
    RuleDef {
        id: "column.date_as_string",
        params: &[],
        fix: Some(|| msg!("lint.fix.date_type")),
        check: date_as_string,
    },
    RuleDef {
        id: "column.integer_as_decimal",
        params: &[],
        fix: Some(|| msg!("lint.fix.integer_type")),
        check: integer_as_decimal,
    },
    RuleDef {
        id: "column.decimal_precision",
        params: &[],
        fix: Some(|| msg!("lint.fix.decimal_precision")),
        check: decimal_precision,
    },
    RuleDef {
        id: "column.money_float",
        params: &[ParamDef {
            key: "words",
            default: ParamDefault::List(&["AMOUNT", "AMT", "PRICE", "MONEY", "FEE", "COST", "BALANCE"]),
        }],
        fix: Some(|| msg!("lint.fix.money_type")),
        check: money_float,
    },
    RuleDef { id: "column.char_type", params: &[], fix: Some(|| msg!("lint.fix.varchar")), check: char_type },
    RuleDef { id: "column.long", params: &[], fix: Some(|| msg!("lint.fix.long_type")), check: long_type },
    RuleDef { id: "column.enum", params: &[], fix: Some(|| msg!("lint.fix.enum_type")), check: enum_type },
    RuleDef { id: "column.lob", params: &[], fix: Some(|| msg!("lint.fix.lob_table")), check: lob },
    RuleDef {
        id: "column.varchar_max",
        params: &[
            ParamDef { key: "max_len", default: ParamDefault::Num(4000) },
            ParamDef { key: "max_count", default: ParamDefault::Num(3) },
        ],
        fix: Some(|| msg!("lint.fix.shorten_varchar")),
        check: varchar_max,
    },
    RuleDef {
        id: "column.no_auto_increment",
        params: &[],
        fix: Some(|| msg!("lint.fix.no_auto_increment")),
        check: no_auto_increment,
    },
];

fn per_column(ctx: &Ctx, bad: impl Fn(&Table, &Column) -> Option<Msg>) -> Check {
    let mut findings = vec![];
    for t in ctx.tables() {
        for c in &t.columns {
            if let Some(m) = bad(t, c) {
                findings.push(Finding::column(t, c, m));
            }
        }
    }
    Check::Findings(findings)
}

fn native(c: &Column) -> String {
    upper(&c.data_type.native)
}

fn comment(ctx: &Ctx, _: &Params) -> Check {
    per_column(ctx, |_, c| {
        c.comment.as_deref().is_none_or(|x| x.trim().is_empty()).then(|| msg!("lint.issue.no_column_comment"))
    })
}

fn comment_differs(ctx: &Ctx, _: &Params) -> Check {
    per_column(ctx, |_, c| {
        c.comment
            .as_deref()
            .is_some_and(|x| same_name(x.trim(), &c.name))
            .then(|| msg!("lint.issue.comment_is_name"))
    })
}

/// 字段名表明是日期或时间。
fn is_temporal_name(name: &str) -> bool {
    let name = upper(name);
    ["_DATE", "_TIME", "_DATETIME", "_TIMESTAMP"].iter().any(|s| name.ends_with(s))
        || matches!(name.as_str(), "DATE" | "TIME" | "DATETIME" | "TIMESTAMP")
}

fn date_as_string(ctx: &Ctx, _: &Params) -> Check {
    per_column(ctx, |_, c| {
        let text = matches!(c.data_type.logical, LogicalType::Char { .. } | LogicalType::VarChar { .. });
        (text && is_temporal_name(&c.name)).then(|| msg!("lint.issue.date_as_string"))
    })
}

fn integer_as_decimal(ctx: &Ctx, _: &Params) -> Check {
    // Oracle 的整数就是 NUMBER(p, 0)
    if ctx.family() == Family::Oracle {
        return Check::NotApplicable;
    }
    per_column(ctx, |_, c| {
        matches!(c.data_type.logical, LogicalType::Decimal { scale: Some(0), .. })
            .then(|| msg!("lint.issue.integer_as_decimal"))
    })
}

fn decimal_precision(ctx: &Ctx, _: &Params) -> Check {
    per_column(ctx, |_, c| {
        matches!(c.data_type.logical, LogicalType::Decimal { precision: None, .. })
            .then(|| msg!("lint.issue.decimal_precision"))
    })
}

fn money_float(ctx: &Ctx, p: &Params) -> Check {
    let words: Vec<String> = p.list("words").iter().map(|s| upper(s)).collect();
    per_column(ctx, |_, c| {
        let name = upper(&c.name);
        let money = name.split('_').any(|part| words.iter().any(|w| w == part));
        (money && matches!(c.data_type.logical, LogicalType::Float { .. }))
            .then(|| msg!("lint.issue.money_float"))
    })
}

fn char_type(ctx: &Ctx, _: &Params) -> Check {
    // SQL Server 存中文要用 NVARCHAR，不报
    let national = ctx.family() != Family::SqlServer;
    per_column(ctx, |_, c| {
        let name = native(c);
        let bad = matches!(c.data_type.logical, LogicalType::Char { .. })
            || (national && (name.starts_with("NCHAR") || name.starts_with("NVARCHAR")));
        bad.then(|| msg!("lint.issue.char_type", native = c.data_type.native))
    })
}

fn long_type(ctx: &Ctx, _: &Params) -> Check {
    per_column(ctx, |_, c| {
        let name = native(c);
        (name == "LONG" || name.starts_with("LONG RAW")).then(|| msg!("lint.issue.long_type"))
    })
}

fn enum_type(ctx: &Ctx, _: &Params) -> Check {
    per_column(ctx, |_, c| {
        let name = native(c);
        (name.starts_with("ENUM") || name.starts_with("SET(")).then(|| msg!("lint.issue.enum_type"))
    })
}

fn lob(ctx: &Ctx, _: &Params) -> Check {
    per_column(ctx, |_, c| {
        matches!(c.data_type.logical, LogicalType::Text | LogicalType::Blob)
            .then(|| msg!("lint.issue.lob", native = c.data_type.native))
    })
}

fn varchar_max(ctx: &Ctx, p: &Params) -> Check {
    let (max_len, max_count) = (p.num("max_len") as u32, p.num("max_count") as usize);
    let mut findings = vec![];
    for t in ctx.tables() {
        let count = t
            .columns
            .iter()
            .filter(|c| matches!(c.data_type.logical, LogicalType::VarChar { len: Some(n) } if n >= max_len))
            .count();
        if count > max_count {
            let message = msg!("lint.issue.varchar_max", count = count, len = max_len, max = max_count);
            findings.push(Finding::table(t, message));
        }
    }
    Check::Findings(findings)
}

fn no_auto_increment(ctx: &Ctx, _: &Params) -> Check {
    per_column(ctx, |_, c| c.auto_increment.then(|| msg!("lint.issue.auto_increment")))
}

#[cfg(test)]
mod tests {
    use dblens_core::LogicalType;
    use serde_json::json;

    use super::*;
    use crate::rules::Outcome;
    use crate::rules::testing::*;

    fn dec(name: &str, precision: Option<u32>, scale: Option<i32>) -> Column {
        col(name, "decimal", LogicalType::Decimal { precision, scale })
    }

    fn one(c: Vec<Column>) -> crate::engine::LintInput {
        tables(vec![table("t", c)])
    }

    #[test]
    fn comment_rules() {
        let mut none = int("a");
        none.comment = None;
        let mut same = int("b");
        same.comment = Some("B".into());
        let i = one(vec![none, same, int("c")]);
        assert_eq!(objects(&i, "column.comment", json!({})), ["t.a"]);
        assert_eq!(objects(&i, "column.comment_differs", json!({})), ["t.b"]);
    }

    #[test]
    fn date_as_string_goes_by_name_and_type() {
        let i = one(vec![
            varchar("start_date", 10),
            varchar("update_time", 20),
            varchar("name", 10),
            int("x_date"),
        ]);
        assert_eq!(objects(&i, "column.date_as_string", json!({})), ["t.start_date", "t.update_time"]);
    }

    #[test]
    fn integer_as_decimal_skips_oracle() {
        let cols = vec![dec("a", Some(10), Some(0)), dec("b", Some(10), Some(2))];
        assert_eq!(objects(&one(cols.clone()), "column.integer_as_decimal", json!({})), ["t.a"]);
        let oracle = input(schema(vec![table("t", cols)]), Family::Oracle);
        assert!(matches!(outcome(&oracle, "column.integer_as_decimal", json!({})), Outcome::NotApplicable));
    }

    #[test]
    fn decimal_needs_precision() {
        let i = one(vec![dec("a", None, None), dec("b", Some(10), Some(2))]);
        assert_eq!(objects(&i, "column.decimal_precision", json!({})), ["t.a"]);
    }

    #[test]
    fn money_float_needs_a_money_word_and_a_float() {
        let float = LogicalType::Float { bits: 64 };
        let i = one(vec![
            col("total_amount", "double", float.clone()),
            col("ratio", "double", float.clone()),
            dec("price", Some(10), Some(2)),
            col("feedback", "double", float),
        ]);
        assert_eq!(objects(&i, "column.money_float", json!({})), ["t.total_amount"]);
    }

    #[test]
    fn char_type_covers_national_types_except_sql_server() {
        let cols = vec![
            col("a", "char(2)", LogicalType::Char { len: Some(2) }),
            col("b", "nvarchar2(10)", LogicalType::VarChar { len: Some(10) }),
            varchar("c", 10),
        ];
        assert_eq!(objects(&one(cols.clone()), "column.char_type", json!({})), ["t.a", "t.b"]);
        let sql = input(schema(vec![table("t", cols)]), Family::SqlServer);
        assert_eq!(objects(&sql, "column.char_type", json!({})), ["t.a"]);
    }

    #[test]
    fn long_enum_and_lob_types() {
        let i = one(vec![
            col("a", "LONG", LogicalType::Text),
            col("b", "enum('x','y')", LogicalType::VarChar { len: None }),
            col("c", "blob", LogicalType::Blob),
            varchar("d", 10),
        ]);
        assert_eq!(objects(&i, "column.long", json!({})), ["t.a"]);
        assert_eq!(objects(&i, "column.enum", json!({})), ["t.b"]);
        assert_eq!(objects(&i, "column.lob", json!({})), ["t.a", "t.c"]);
    }

    #[test]
    fn varchar_max_reports_tables_with_too_many() {
        let i = one(vec![varchar("a", 4000), varchar("b", 4000), varchar("c", 100)]);
        assert_eq!(objects(&i, "column.varchar_max", json!({"max_count": 1})), ["t"]);
        assert!(objects(&i, "column.varchar_max", json!({"max_count": 2})).is_empty());
    }

    #[test]
    fn no_auto_increment_flags_auto_columns() {
        let mut a = int("a");
        a.auto_increment = true;
        assert_eq!(objects(&one(vec![a, int("b")]), "column.no_auto_increment", json!({})), ["t.a"]);
    }
}
