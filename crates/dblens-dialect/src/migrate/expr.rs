//! 迁移中的表达式：跨家族时只转换"简单表达式"（列名、字面量、比较、逻辑运算、IN、BETWEEN、IS NULL、LIKE），
//! 以及常见的默认值写法（字面量、布尔、当前时间或日期、UUID 函数、序列的 nextval）。
//!
//! 先把源文本切成记号并规整：去掉 PG 的类型转换（`::text`）、MySQL 的字符集前缀（`_utf8mb4'x'`），
//! 把 PG 的 `= ANY (ARRAY[…])` / `<> ALL (ARRAY[…])` 还原成 `IN (…)` / `NOT IN (…)`。

use std::collections::BTreeMap;

use dblens_core::{ColumnType, LogicalType, Value};

use crate::{Dialect, Family};

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    /// 不加引号的词（列名、关键字、函数名），保留原样。
    Word(String),
    /// 加引号的标识符（引号已去掉）。
    Quoted(String),
    /// 字符串字面量（已还原转义）。
    Str(String),
    Num(String),
    /// 运算符和标点：`( ) , . = <> != < > <= >= + - * / % [ ] ::`
    Op(&'static str),
}

const OPS: &[&str] = &[
    "::", "<>", "!=", "<=", ">=", "||", "(", ")", ",", ".", "=", "<", ">", "+", "-", "*", "/", "%", "[", "]",
];

fn tokenize(family: Family, s: &str) -> Option<Vec<Tok>> {
    let b: Vec<char> = s.chars().collect();
    let mut i = 0;
    let mut out = Vec::new();
    while i < b.len() {
        let c = b[i];
        if c.is_whitespace() {
            i += 1;
        } else if c == '\'' {
            let mut v = String::new();
            i += 1;
            loop {
                match b.get(i) {
                    None => return None,
                    Some('\'') if b.get(i + 1) == Some(&'\'') => {
                        v.push('\'');
                        i += 2;
                    }
                    Some('\'') => {
                        i += 1;
                        break;
                    }
                    Some('\\') if family == Family::MySql => {
                        v.push(*b.get(i + 1)?);
                        i += 2;
                    }
                    Some(ch) => {
                        v.push(*ch);
                        i += 1;
                    }
                }
            }
            out.push(Tok::Str(v));
        } else if c == '"' || (c == '`' && family == Family::MySql) {
            let end = b[i + 1..].iter().position(|&x| x == c)? + i + 1;
            out.push(Tok::Quoted(b[i + 1..end].iter().collect()));
            i = end + 1;
        } else if c.is_ascii_digit() {
            let start = i;
            while i < b.len() && (b[i].is_ascii_digit() || b[i] == '.') {
                i += 1;
            }
            if i < b.len() && (b[i] == 'e' || b[i] == 'E') {
                i += 1;
                if i < b.len() && (b[i] == '+' || b[i] == '-') {
                    i += 1;
                }
                while i < b.len() && b[i].is_ascii_digit() {
                    i += 1;
                }
            }
            out.push(Tok::Num(b[start..i].iter().collect()));
        } else if c.is_alphabetic() || c == '_' {
            let start = i;
            while i < b.len() && (b[i].is_alphanumeric() || b[i] == '_' || b[i] == '$' || b[i] == '#') {
                i += 1;
            }
            out.push(Tok::Word(b[start..i].iter().collect()));
        } else {
            let rest: String = b[i..b.len().min(i + 2)].iter().collect();
            let op = OPS.iter().find(|o| rest.starts_with(**o))?;
            out.push(Tok::Op(op));
            i += op.chars().count();
        }
    }
    Some(normalize(out))
}

fn is_kw(t: &Tok, kw: &str) -> bool {
    matches!(t, Tok::Word(w) if w.eq_ignore_ascii_case(kw))
}

/// 规整：去掉类型转换和字符集前缀，还原 `ANY (ARRAY[…])`。
fn normalize(toks: Vec<Tok>) -> Vec<Tok> {
    const STOP: &[&str] = &["AND", "OR", "NOT", "IN", "IS", "BETWEEN", "LIKE"];
    let mut out: Vec<Tok> = Vec::new();
    let mut i = 0;
    while i < toks.len() {
        match &toks[i] {
            Tok::Op("::") => {
                i += 1;
                while matches!(toks.get(i), Some(Tok::Word(w)) if !STOP.iter().any(|s| w.eq_ignore_ascii_case(s)))
                {
                    i += 1;
                }
                if toks.get(i) == Some(&Tok::Op("(")) {
                    while i < toks.len() && toks[i] != Tok::Op(")") {
                        i += 1;
                    }
                    i += 1;
                }
                while toks.get(i) == Some(&Tok::Op("[")) && toks.get(i + 1) == Some(&Tok::Op("]")) {
                    i += 2;
                }
            }
            Tok::Word(w) if w.starts_with('_') && matches!(toks.get(i + 1), Some(Tok::Str(_))) => i += 1,
            _ => {
                out.push(toks[i].clone());
                i += 1;
            }
        }
    }
    any_array(out)
}

/// `= ANY (ARRAY[a, b])` → `IN (a, b)`；`<> ALL (ARRAY[a, b])` → `NOT IN (a, b)`（括号可以多套几层）。
fn any_array(toks: Vec<Tok>) -> Vec<Tok> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < toks.len() {
        let neg = match (&toks[i], toks.get(i + 1)) {
            (Tok::Op("="), Some(t)) if is_kw(t, "ANY") => Some(false),
            (Tok::Op("<>" | "!="), Some(t)) if is_kw(t, "ALL") => Some(true),
            _ => None,
        };
        if let Some((items, next)) = neg.and_then(|_| array_items(&toks, i + 2)) {
            if neg == Some(true) {
                out.push(Tok::Word("NOT".into()));
            }
            out.push(Tok::Word("IN".into()));
            out.push(Tok::Op("("));
            out.extend(items.iter().cloned());
            out.push(Tok::Op(")"));
            i = next;
            continue;
        }
        out.push(toks[i].clone());
        i += 1;
    }
    out
}

/// 从 `start` 起的 `(…(ARRAY[元素])…)`：返回元素的记号和之后的位置。
fn array_items(toks: &[Tok], start: usize) -> Option<(&[Tok], usize)> {
    let mut j = start;
    while toks.get(j) == Some(&Tok::Op("(")) {
        j += 1;
    }
    let depth = j - start;
    if depth == 0 || !toks.get(j).is_some_and(|t| is_kw(t, "ARRAY")) || toks.get(j + 1) != Some(&Tok::Op("["))
    {
        return None;
    }
    let end = j + 2 + toks[j + 2..].iter().position(|t| *t == Tok::Op("]"))?;
    let after = end + 1;
    (0..depth).all(|k| toks.get(after + k) == Some(&Tok::Op(")"))).then(|| (&toks[j + 2..end], after + depth))
}

/// 简单表达式中允许的关键字。
const KEYWORDS: &[&str] = &["AND", "OR", "NOT", "IN", "IS", "NULL", "BETWEEN", "LIKE", "TRUE", "FALSE"];

/// 转换简单表达式：列名换成目标名并按目标方言引用，字符串按目标方言写字面量。不是简单表达式时返回 None。
/// `columns`：源列名 → 目标列名。
pub fn convert_expr(
    src: &Dialect,
    tgt: &Dialect,
    text: &str,
    columns: &BTreeMap<String, String>,
) -> Option<String> {
    let toks = tokenize(src.family(), text)?;
    let find = |name: &str, quoted: bool| -> Option<&String> {
        columns.get(name).or_else(|| {
            if quoted {
                None
            } else {
                columns.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v)
            }
        })
    };
    let mut parts: Vec<String> = Vec::new();
    for t in &toks {
        let s = match t {
            Tok::Word(w) if KEYWORDS.iter().any(|k| w.eq_ignore_ascii_case(k)) => {
                let up = w.to_ascii_uppercase();
                match up.as_str() {
                    // Oracle、达梦没有布尔字面量
                    "TRUE" | "FALSE" if matches!(tgt.family(), Family::Oracle | Family::Dameng) => {
                        (if up == "TRUE" { "1" } else { "0" }).to_string()
                    }
                    _ => up,
                }
            }
            Tok::Word(w) => tgt.ident(find(w, false)?),
            Tok::Quoted(q) => tgt.ident(find(q, true)?),
            Tok::Str(v) => tgt.literal(&Value::String(v.clone())).ok()?,
            Tok::Num(n) => n.clone(),
            Tok::Op(
                o @ ("(" | ")" | "," | "=" | "<>" | "!=" | "<" | ">" | "<=" | ">=" | "+" | "-" | "*" | "/"),
            ) => (if *o == "!=" { "<>" } else { o }).to_string(),
            Tok::Op(_) => return None,
        };
        parts.push(s);
    }
    if parts.is_empty() {
        return None;
    }
    // 拼接：左括号之后、右括号和逗号之前不加空格
    let mut out = String::new();
    for (i, p) in parts.iter().enumerate() {
        let prev = i.checked_sub(1).map(|j| parts[j].as_str());
        if i > 0 && prev != Some("(") && p != ")" && p != "," {
            out.push(' ');
        }
        out.push_str(p);
    }
    Some(out)
}

/// 去掉成对包住全部内容的括号。
fn strip_parens(mut toks: &[Tok]) -> &[Tok] {
    while toks.len() >= 2 && toks[0] == Tok::Op("(") && toks[toks.len() - 1] == Tok::Op(")") {
        let mut depth = 0;
        let whole = toks.iter().enumerate().all(|(i, t)| {
            match t {
                Tok::Op("(") => depth += 1,
                Tok::Op(")") => depth -= 1,
                _ => {}
            }
            depth > 0 || i == toks.len() - 1
        });
        if !whole {
            break;
        }
        toks = &toks[1..toks.len() - 1];
    }
    toks
}

/// 函数调用 `名字()` 或 `名字(数字)`：返回名字（大写）。
fn call(toks: &[Tok]) -> Option<String> {
    match toks {
        [Tok::Word(w)] => Some(w.to_ascii_uppercase()),
        [Tok::Word(w), Tok::Op("("), Tok::Op(")")]
        | [Tok::Word(w), Tok::Op("("), Tok::Num(_), Tok::Op(")")] => Some(w.to_ascii_uppercase()),
        _ => None,
    }
}

/// 序列 nextval 的目标序列名：PG 的 `nextval('s')`（可带 schema）、Oracle/达梦的 `s.NEXTVAL`、MariaDB 的 `nextval(s)`。
pub(super) fn nextval_target(text: &str) -> Option<String> {
    let family = if text.contains('`') { Family::MySql } else { Family::Postgres };
    let toks = tokenize(family, text)?;
    let toks = strip_parens(&toks);
    let last = |name: &str| name.rsplit('.').next().unwrap_or(name).trim_matches('"').to_string();
    match toks {
        [Tok::Word(f), Tok::Op("("), Tok::Str(s), Tok::Op(")")] if f.eq_ignore_ascii_case("nextval") => {
            Some(last(s))
        }
        [Tok::Word(f), Tok::Op("("), Tok::Word(s) | Tok::Quoted(s), Tok::Op(")")]
            if f.eq_ignore_ascii_case("nextval") =>
        {
            Some(s.clone())
        }
        [.., Tok::Word(s) | Tok::Quoted(s), Tok::Op("."), Tok::Word(n) | Tok::Quoted(n)]
            if n.eq_ignore_ascii_case("NEXTVAL") =>
        {
            Some(s.clone())
        }
        _ => None,
    }
}

/// 转换列的默认值。`Ok(None)`：没有默认值（源为 NULL）；`Err(())`：认不出，不迁。
/// `column`：目标列的类型；`sequences`：源序列名（小写）→ 目标序列名。
pub(super) fn convert_default(
    src: &Dialect,
    tgt: &Dialect,
    text: &str,
    column: &ColumnType,
    sequences: &BTreeMap<String, String>,
) -> Result<Option<String>, ()> {
    use LogicalType as L;
    let fam = tgt.family();
    if let Some(seq) = nextval_target(text) {
        let name = sequences.get(&seq.to_lowercase()).ok_or(())?;
        return match fam {
            Family::Postgres => tgt
                .literal(&Value::String(tgt.ident(name)))
                .map(|s| Some(format!("nextval({s})")))
                .map_err(|_| ()),
            Family::Oracle | Family::Dameng => Ok(Some(format!("{}.NEXTVAL", tgt.ident(name)))),
            Family::MySql => Err(()),
        };
    }
    let toks = tokenize(src.family(), text).ok_or(())?;
    let toks = strip_parens(&toks);
    let logical = &column.logical;
    let oracle_like = matches!(fam, Family::Oracle | Family::Dameng);
    let boolean = |v: bool| -> String {
        if oracle_like || (fam == Family::Postgres && *logical != L::Bool) {
            (if v { "1" } else { "0" }).into()
        } else {
            (if v { "TRUE" } else { "FALSE" }).into()
        }
    };
    let today = || -> Result<Option<String>, ()> {
        Ok(Some(match (fam, logical) {
            (Family::MySql, L::Date) => "(CURRENT_DATE)".into(),
            (Family::Postgres, L::Date) => "CURRENT_DATE".into(),
            (Family::Oracle, L::Timestamp { .. } | L::Date) => "TRUNC(SYSDATE)".into(),
            (Family::Dameng, L::Date) => "CURDATE()".into(),
            _ => return Err(()),
        }))
    };
    let now = || -> Result<Option<String>, ()> {
        Ok(Some(match (fam, logical) {
            (Family::MySql, L::Timestamp { precision: Some(p), .. }) if *p > 0 => {
                format!("CURRENT_TIMESTAMP({p})")
            }
            (Family::MySql, L::Timestamp { .. }) => "CURRENT_TIMESTAMP".into(),
            (Family::Postgres, L::Timestamp { tz: false, .. }) => "LOCALTIMESTAMP".into(),
            (Family::Postgres, L::Timestamp { tz: true, .. }) => "CURRENT_TIMESTAMP".into(),
            (Family::Oracle | Family::Dameng, L::Timestamp { tz: false, precision: Some(0) })
                if column.native.trim().eq_ignore_ascii_case("DATE") =>
            {
                "SYSDATE".into()
            }
            (Family::Oracle, L::Timestamp { tz: false, .. }) => "LOCALTIMESTAMP".into(),
            (Family::Oracle, L::Timestamp { tz: true, .. }) => "SYSTIMESTAMP".into(),
            (Family::Dameng, L::Timestamp { .. }) => "CURRENT_TIMESTAMP".into(),
            (_, L::Date) => return today(),
            _ => return Err(()),
        }))
    };
    match toks {
        [] => Err(()),
        [Tok::Word(w)] if w.eq_ignore_ascii_case("NULL") => Ok(None),
        [Tok::Num(n)] if *logical == L::Bool && (n == "0" || n == "1") => Ok(Some(boolean(n == "1"))),
        [Tok::Num(n)] => Ok(Some(n.clone())),
        [Tok::Op("-"), Tok::Num(n)] => Ok(Some(format!("-{n}"))),
        [Tok::Word(w)] if w.eq_ignore_ascii_case("TRUE") || w.eq_ignore_ascii_case("FALSE") => {
            Ok(Some(boolean(w.eq_ignore_ascii_case("TRUE"))))
        }
        // MySQL 的位串 b'1'
        [Tok::Word(b), Tok::Str(s)] if b.eq_ignore_ascii_case("b") && (s == "0" || s == "1") => {
            Ok(Some(boolean(s == "1")))
        }
        [Tok::Str(s)] => {
            let value = Value::String(s.clone());
            match (fam, logical) {
                (Family::Oracle | Family::Dameng, L::Date | L::Timestamp { .. }) => {
                    // 只有日期的写成 DATE 字面量，带时间的写成 TIMESTAMP 字面量
                    let kw = if s.trim().len() <= 10 { "DATE" } else { "TIMESTAMP" };
                    tgt.literal(&value).map(|l| Some(format!("{kw} {l}"))).map_err(|_| ())
                }
                (_, L::Bool) if ["t", "true", "1"].contains(&s.to_lowercase().as_str()) => {
                    Ok(Some(boolean(true)))
                }
                (_, L::Bool) if ["f", "false", "0"].contains(&s.to_lowercase().as_str()) => {
                    Ok(Some(boolean(false)))
                }
                _ => tgt.literal(&value).map(Some).map_err(|_| ()),
            }
        }
        _ => match call(toks).as_deref() {
            Some(
                "CURRENT_TIMESTAMP"
                | "LOCALTIMESTAMP"
                | "NOW"
                | "SYSDATE"
                | "SYSTIMESTAMP"
                | "GETDATE"
                | "STATEMENT_TIMESTAMP"
                | "TRANSACTION_TIMESTAMP"
                | "CLOCK_TIMESTAMP"
                | "LOCALTIME"
                | "CURRENT_TIME"
                | "PG_SYSTIMESTAMP",
            ) => {
                if *logical == L::Date {
                    today()
                } else {
                    now()
                }
            }
            Some("CURRENT_DATE" | "CURDATE") => today(),
            Some("UUID" | "GEN_RANDOM_UUID" | "SYS_GUID" | "NEWID" | "UUID_GENERATE_V4") => {
                match tgt.variant() {
                    crate::Variant::MySql => Ok(Some("(UUID())".into())),
                    crate::Variant::Postgres => Ok(Some("gen_random_uuid()".into())),
                    crate::Variant::Oracle => Ok(Some("SYS_GUID()".into())),
                    _ => Err(()),
                }
            }
            _ => {
                // Oracle 的 TRUNC(SYSDATE)
                match toks {
                    [Tok::Word(f), Tok::Op("("), Tok::Word(a), Tok::Op(")")]
                        if f.eq_ignore_ascii_case("TRUNC") && a.eq_ignore_ascii_case("SYSDATE") =>
                    {
                        today()
                    }
                    _ => Err(()),
                }
            }
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Variant;

    fn d(v: Variant) -> Dialect {
        Dialect::new(v)
    }

    fn cols(names: &[(&str, &str)]) -> BTreeMap<String, String> {
        names.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect()
    }

    fn ty(dialect: Variant, native: &str) -> ColumnType {
        ColumnType { native: native.into(), logical: d(dialect).logical_type(native) }
    }

    #[test]
    fn simple_expressions_convert_names_and_literals() {
        let c = cols(&[("qty", "QTY"), ("status", "STATUS")]);
        let pg = d(Variant::Postgres);
        let ora = d(Variant::Oracle);
        assert_eq!(convert_expr(&pg, &ora, "(qty > 0)", &c).as_deref(), Some("(QTY > 0)"));
        assert_eq!(
            convert_expr(
                &pg,
                &ora,
                "((status)::text = ANY ((ARRAY['a'::character varying, 'b''c'::character varying])::text[]))",
                &c
            )
            .as_deref(),
            Some("((STATUS) IN ('a', 'b''c'))")
        );
        assert_eq!(
            convert_expr(&pg, &ora, "status <> ALL (ARRAY['x'])", &c).as_deref(),
            Some("STATUS NOT IN ('x')")
        );
        let my = d(Variant::MySql);
        let c2 = cols(&[("qty", "qty")]);
        assert_eq!(
            convert_expr(&my, &pg, "((`qty` between 1 and 10) and (`qty` <> _utf8mb4'x'))", &c2).as_deref(),
            Some("((qty BETWEEN 1 AND 10) AND (qty <> 'x'))")
        );
        assert_eq!(
            convert_expr(&ora, &pg, "\"QTY\" IS NOT NULL", &cols(&[("QTY", "qty")])).as_deref(),
            Some("qty IS NOT NULL")
        );
        assert_eq!(
            convert_expr(&pg, &ora, "flag = true", &cols(&[("flag", "FLAG")])).as_deref(),
            Some("FLAG = 1")
        );
    }

    #[test]
    fn functions_and_unknown_names_are_not_simple() {
        let c = cols(&[("name", "name")]);
        let (pg, my) = (d(Variant::Postgres), d(Variant::MySql));
        assert_eq!(convert_expr(&pg, &my, "length(name) > 0", &c), None);
        assert_eq!(convert_expr(&pg, &my, "other > 0", &c), None);
        assert_eq!(convert_expr(&pg, &my, "name || 'x' = 'y'", &c), None);
        assert_eq!(convert_expr(&pg, &my, "", &c), None);
    }

    #[test]
    fn defaults_convert_between_families() {
        let none = BTreeMap::new();
        let conv = |s: Variant, t: Variant, text: &str, native: &str| {
            convert_default(&d(s), &d(t), text, &ty(t, native), &none)
        };
        use Variant::*;
        assert_eq!(
            conv(Postgres, MySql, "'abc'::character varying", "VARCHAR(10)"),
            Ok(Some("'abc'".into()))
        );
        assert_eq!(conv(Postgres, Oracle, "0", "NUMBER(10)"), Ok(Some("0".into())));
        assert_eq!(conv(Oracle, Postgres, "-1.5 ", "NUMERIC(5,1)"), Ok(Some("-1.5".into())));
        assert_eq!(conv(Oracle, Postgres, "NULL", "INTEGER"), Ok(None));
        assert_eq!(conv(Postgres, Oracle, "true", "NUMBER(1)"), Ok(Some("1".into())));
        assert_eq!(conv(MySql, Postgres, "b'1'", "BOOLEAN"), Ok(Some("TRUE".into())));
        assert_eq!(conv(Postgres, MySql, "now()", "DATETIME(6)"), Ok(Some("CURRENT_TIMESTAMP(6)".into())));
        assert_eq!(conv(Oracle, Postgres, "SYSDATE", "TIMESTAMP(0)"), Ok(Some("LOCALTIMESTAMP".into())));
        assert_eq!(conv(MySql, Oracle, "CURRENT_TIMESTAMP", "DATE"), Ok(Some("SYSDATE".into())));
        assert_eq!(
            conv(Postgres, Oracle, "CURRENT_TIMESTAMP", "TIMESTAMP(6) WITH TIME ZONE"),
            Ok(Some("SYSTIMESTAMP".into()))
        );
        assert_eq!(conv(Postgres, Dameng, "CURRENT_DATE", "DATE"), Ok(Some("CURDATE()".into())));
        assert_eq!(conv(Oracle, MySql, "trunc(sysdate)", "DATE"), Ok(Some("(CURRENT_DATE)".into())));
        assert_eq!(
            conv(Postgres, Oracle, "'2020-01-02'::date", "DATE"),
            Ok(Some("DATE '2020-01-02'".into()))
        );
        assert_eq!(conv(Postgres, MySql, "gen_random_uuid()", "CHAR(36)"), Ok(Some("(UUID())".into())));
        assert_eq!(conv(Postgres, MySql, "upper('x')", "VARCHAR(10)"), Err(()));
        assert_eq!(conv(Postgres, MySql, "nextval('s'::regclass)", "BIGINT"), Err(()));
    }

    #[test]
    fn nextval_defaults_use_the_target_sequence() {
        let seqs = cols(&[("order_seq", "ORDER_SEQ")]);
        let pg = d(Variant::Postgres);
        let ora = d(Variant::Oracle);
        assert_eq!(nextval_target("nextval('public.order_seq'::regclass)").as_deref(), Some("order_seq"));
        assert_eq!(nextval_target("\"APP\".\"ORDER_SEQ\".\"NEXTVAL\"").as_deref(), Some("ORDER_SEQ"));
        assert_eq!(nextval_target("\"APP\".\"ORDER_SEQ\".NEXTVAL").as_deref(), Some("ORDER_SEQ"));
        assert_eq!(
            convert_default(
                &pg,
                &ora,
                "nextval('order_seq'::regclass)",
                &ty(Variant::Oracle, "NUMBER(19)"),
                &seqs
            ),
            Ok(Some("ORDER_SEQ.NEXTVAL".into()))
        );
        let back = cols(&[("order_seq", "order_seq")]);
        assert_eq!(
            convert_default(&ora, &pg, "ORDER_SEQ.NEXTVAL", &ty(Variant::Postgres, "BIGINT"), &back),
            Ok(Some("nextval('order_seq')".into()))
        );
    }
}
