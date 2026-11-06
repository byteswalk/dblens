//! SQLite 系统目录里没有的信息，从 `sqlite_schema.sql` 保存的建对象原文里读：检查约束、生成列表达式、
//! 约束名、表达式索引的列、触发器的时机和事件。
//!
//! 只做轻量的扫描（引号、注释、括号层级），表达式按原文切出，保留用户的写法。读不懂的写法只是缺这几项。

use std::collections::HashMap;

use dblens_core::schema::{CheckConstraint, Table};

/// 一段原文中顶层（不在括号内）的记号。
#[derive(Debug, Clone, PartialEq)]
enum Tok<'a> {
    /// 关键字或不带引号的名字。
    Word(&'a str),
    /// 带引号的名字（"x"、`x`、[x]），已去掉引号。
    Name(String),
    /// 括号及其内容：内容的原文（不含两侧括号）。
    Group(&'a str),
    Str,
    Comma,
    Other,
}

impl Tok<'_> {
    fn is(&self, kw: &str) -> bool {
        matches!(self, Tok::Word(w) if w.eq_ignore_ascii_case(kw))
    }

    fn name(&self) -> Option<String> {
        match self {
            Tok::Word(w) => Some((*w).to_string()),
            Tok::Name(n) => Some(n.clone()),
            _ => None,
        }
    }
}

/// 从 `i` 开始跳过一个带引号的部分，返回结束后的位置；不是引号开头时返回 None。
fn skip_quoted(b: &[u8], i: usize) -> Option<usize> {
    let close = match b[i] {
        b'\'' => b'\'',
        b'"' => b'"',
        b'`' => b'`',
        b'[' => b']',
        _ => return None,
    };
    let mut j = i + 1;
    while j < b.len() {
        if b[j] == close {
            // 引号重写两次表示转义（方括号不能转义）
            if close != b']' && b.get(j + 1) == Some(&close) {
                j += 2;
                continue;
            }
            return Some(j + 1);
        }
        j += 1;
    }
    Some(b.len())
}

/// 跳过注释，返回结束后的位置。
fn skip_comment(b: &[u8], i: usize) -> Option<usize> {
    if b[i..].starts_with(b"--") {
        return Some(b[i..].iter().position(|&c| c == b'\n').map_or(b.len(), |p| i + p + 1));
    }
    if b[i..].starts_with(b"/*") {
        return Some(b[i + 2..].windows(2).position(|w| w == b"*/").map_or(b.len(), |p| i + 2 + p + 2));
    }
    None
}

fn unquote(raw: &str) -> String {
    let inner = &raw[1..raw.len().saturating_sub(1).max(1)];
    match raw.as_bytes()[0] {
        b'"' => inner.replace("\"\"", "\""),
        b'`' => inner.replace("``", "`"),
        _ => inner.to_string(),
    }
}

/// 把原文切成顶层记号。
fn tokens(s: &str) -> Vec<Tok<'_>> {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if c.is_ascii_whitespace() {
            i += 1;
        } else if let Some(j) = skip_comment(b, i) {
            i = j;
        } else if let Some(j) = skip_quoted(b, i) {
            out.push(if c == b'\'' { Tok::Str } else { Tok::Name(unquote(&s[i..j])) });
            i = j;
        } else if c == b'(' {
            let end = close_paren(b, i);
            out.push(Tok::Group(&s[i + 1..end.saturating_sub(1).max(i + 1)]));
            i = end;
        } else if c == b',' {
            out.push(Tok::Comma);
            i += 1;
        } else if c.is_ascii_alphanumeric() || c == b'_' || c >= 0x80 {
            let start = i;
            while i < b.len()
                && (b[i].is_ascii_alphanumeric() || b[i] == b'_' || b[i] == b'$' || b[i] >= 0x80)
            {
                i += 1;
            }
            out.push(Tok::Word(&s[start..i]));
        } else {
            out.push(Tok::Other);
            i += 1;
        }
    }
    out
}

/// `(` 在 `i` 处，返回配对的 `)` 之后的位置（没有配对时为末尾）。
fn close_paren(b: &[u8], i: usize) -> usize {
    let mut depth = 0;
    let mut j = i;
    while j < b.len() {
        if let Some(k) = skip_comment(b, j).or_else(|| skip_quoted(b, j)) {
            j = k;
            continue;
        }
        match b[j] {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return j + 1;
                }
            }
            _ => {}
        }
        j += 1;
    }
    b.len()
}

/// 按顶层逗号切开括号内的列表，返回各项的原文（去掉两侧空白）。
fn split_list(s: &str) -> Vec<&str> {
    let b = s.as_bytes();
    let (mut out, mut start, mut i) = (Vec::new(), 0, 0);
    while i < b.len() {
        if let Some(k) = skip_comment(b, i).or_else(|| skip_quoted(b, i)) {
            i = k;
            continue;
        }
        match b[i] {
            b'(' => {
                i = close_paren(b, i);
                continue;
            }
            b',' => {
                out.push(s[start..i].trim());
                start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    out.push(s[start..].trim());
    out.retain(|x| !x.is_empty());
    out
}

/// 括号列表里的名字（`(a, "b" DESC)` → a、b）。
fn names_in(group: &str) -> Vec<String> {
    split_list(group).iter().filter_map(|item| tokens(item).first().and_then(Tok::name)).collect()
}

/// 从建表语句读出的约束名、检查约束和生成列表达式。
#[derive(Debug, Default, PartialEq)]
pub(crate) struct CreateTable {
    pub primary_key: Option<String>,
    /// 唯一约束：列 → 名字。
    pub uniques: Vec<(Vec<String>, String)>,
    /// 外键：列 → 名字。
    pub foreign_keys: Vec<(Vec<String>, String)>,
    pub checks: Vec<CheckConstraint>,
    /// 列名（小写）→ 生成表达式。
    pub generated: HashMap<String, String>,
}

/// 解析 `CREATE TABLE`。不是建表语句（如 `CREATE VIRTUAL TABLE`）或读不懂时返回 None。
pub(crate) fn parse_create_table(sql: &str) -> Option<CreateTable> {
    let toks = tokens(sql);
    if !toks.first()?.is("CREATE") || toks.iter().take(4).any(|t| t.is("VIRTUAL")) {
        return None;
    }
    let body = toks.iter().find_map(|t| if let Tok::Group(g) = t { Some(*g) } else { None })?;
    let mut out = CreateTable::default();
    for item in split_list(body) {
        let t = tokens(item);
        let table_level = ["CONSTRAINT", "PRIMARY", "UNIQUE", "CHECK", "FOREIGN"]
            .iter()
            .any(|k| t.first().is_some_and(|x| x.is(k)));
        let column = if table_level { None } else { t.first().and_then(Tok::name) };
        let mut name: Option<String> = None;
        let mut k = usize::from(column.is_some());
        while k < t.len() {
            let tok = &t[k];
            let group_after = |n: usize| match t.get(n) {
                Some(Tok::Group(g)) => Some(*g),
                _ => None,
            };
            if tok.is("CONSTRAINT") {
                name = t.get(k + 1).and_then(Tok::name);
                k += 2;
                continue;
            }
            if tok.is("CHECK") {
                if let Some(g) = group_after(k + 1) {
                    out.checks.push(CheckConstraint { name: name.take(), expression: g.trim().to_string() });
                    k += 2;
                    continue;
                }
            } else if tok.is("AS") {
                if let (Some(col), Some(g)) = (&column, group_after(k + 1)) {
                    out.generated.insert(col.to_lowercase(), g.trim().to_string());
                    k += 2;
                    continue;
                }
            } else if tok.is("PRIMARY") {
                if let Some(n) = name.take() {
                    out.primary_key = Some(n);
                }
            } else if tok.is("UNIQUE") {
                let cols = match (&column, group_after(k + 1)) {
                    (Some(c), _) => Some(vec![c.clone()]),
                    (None, Some(g)) => Some(names_in(g)),
                    _ => None,
                };
                if let (Some(n), Some(cols)) = (name.take(), cols) {
                    out.uniques.push((cols, n));
                }
            } else if tok.is("FOREIGN") || (tok.is("REFERENCES") && column.is_some()) {
                let cols = match (&column, group_after(k + 2)) {
                    (Some(c), _) => Some(vec![c.clone()]),
                    (None, Some(g)) => Some(names_in(g)),
                    _ => None,
                };
                if let (Some(n), Some(cols)) = (name.take(), cols) {
                    out.foreign_keys.push((cols, n));
                }
            } else if ["NOT", "NULL", "DEFAULT", "COLLATE", "GENERATED"].iter().any(|kw| tok.is(kw)) {
                // 约束名只作用于紧跟着的那个约束（`CONSTRAINT nn NOT NULL` 的名字不能落到后面的约束上）
                name = None;
            }
            k += 1;
        }
    }
    Some(out)
}

/// `CREATE INDEX … ON 表 (列…)` 中第 `position` 个（从 1 开始）索引列的原文，去掉 COLLATE、ASC、DESC。
pub(crate) fn index_column(sql: &str, position: usize) -> Option<String> {
    let toks = tokens(sql);
    let on = toks.iter().position(|t| t.is("ON"))?;
    let group = toks[on..].iter().find_map(|t| if let Tok::Group(g) = t { Some(*g) } else { None })?;
    let item = *split_list(group).get(position.checked_sub(1)?)?;
    let mut text = item.trim().to_string();
    for suffix in ["ASC", "DESC"] {
        if text.len() > suffix.len() && text[text.len() - suffix.len()..].eq_ignore_ascii_case(suffix) {
            let head = &text[..text.len() - suffix.len()];
            if head.ends_with(|c: char| c.is_whitespace() || c == ')') {
                text = head.trim_end().to_string();
            }
        }
    }
    let upper = text.to_ascii_uppercase();
    if let Some(p) = upper.rfind(" COLLATE ") {
        text.truncate(p);
    }
    Some(text.trim().to_string())
}

/// 触发器的时机（BEFORE、AFTER、INSTEAD OF；不写时是 BEFORE）和事件（INSERT、UPDATE、DELETE）。
pub(crate) fn trigger_header(sql: &str) -> (String, Vec<String>) {
    let toks = tokens(sql);
    let head = toks.iter().position(|t| t.is("BEGIN")).map_or(&toks[..], |p| &toks[..p]);
    let timing = if head.iter().any(|t| t.is("INSTEAD")) {
        "INSTEAD OF"
    } else if head.iter().any(|t| t.is("AFTER")) {
        "AFTER"
    } else {
        "BEFORE"
    };
    let events = ["INSERT", "UPDATE", "DELETE"]
        .iter()
        .find(|e| head.iter().any(|t| t.is(e)))
        .map(|e| vec![(*e).to_string()])
        .unwrap_or_default();
    (timing.to_string(), events)
}

/// 用建表语句补全组装好的表：检查约束、生成列表达式、约束名。系统目录给的分组键（`pk`、`u:…`、`fk:…`）
/// 换成建表语句里写的名字，没写名字的清空。
pub(crate) fn complete_table(table: &mut Table, create_sql: Option<&str>) {
    let parsed = create_sql.and_then(parse_create_table).unwrap_or_default();
    let same = |a: &[String], b: &[String]| {
        a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.eq_ignore_ascii_case(y))
    };
    if let Some(pk) = table.primary_key.as_mut() {
        pk.name = parsed.primary_key.clone();
    }
    for u in &mut table.uniques {
        u.name = parsed.uniques.iter().find(|(cols, _)| same(cols, &u.columns)).map(|(_, n)| n.clone());
    }
    for f in &mut table.foreign_keys {
        f.name = parsed.foreign_keys.iter().find(|(cols, _)| same(cols, &f.columns)).map(|(_, n)| n.clone());
        // SQLite 不写动作时报 NO ACTION
        for a in [&mut f.on_delete, &mut f.on_update] {
            if a.as_deref() == Some("NO ACTION") {
                *a = None;
            }
        }
    }
    table.checks = parsed.checks;
    for c in &mut table.columns {
        if c.generated.is_some()
            && let Some(expr) = parsed.generated.get(&c.name.to_lowercase())
        {
            c.generated = Some(expr.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_constraints_from_create_table() {
        let sql = r#"CREATE TABLE "订单 (x)" (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  code TEXT CONSTRAINT uq_code UNIQUE NOT NULL,
  qty INT CHECK (qty > 0) DEFAULT 1, -- 数量, (注释里的括号)
  price REAL CONSTRAINT ck_price CHECK (price >= 0 AND price < 1e9),
  total REAL GENERATED ALWAYS AS (qty * price) STORED,
  "Half" REAL AS (price / 2),
  cust INT CONSTRAINT fk_cust REFERENCES cust (id) ON DELETE CASCADE,
  a INT, b INT,
  CONSTRAINT uq_ab UNIQUE (a, "b"),
  CONSTRAINT fk_ab FOREIGN KEY (a, b) REFERENCES other (x, y),
  CHECK (a <> ',' OR b IS NULL)
)"#;
        let t = parse_create_table(sql).unwrap();
        assert_eq!(t.primary_key, None);
        assert_eq!(
            t.uniques,
            [
                (vec!["code".to_string()], "uq_code".to_string()),
                (vec!["a".into(), "b".into()], "uq_ab".into())
            ]
        );
        assert_eq!(
            t.foreign_keys,
            [
                (vec!["cust".to_string()], "fk_cust".to_string()),
                (vec!["a".into(), "b".into()], "fk_ab".into())
            ]
        );
        let checks: Vec<(Option<&str>, &str)> =
            t.checks.iter().map(|c| (c.name.as_deref(), c.expression.as_str())).collect();
        assert_eq!(
            checks,
            [
                (None, "qty > 0"),
                (Some("ck_price"), "price >= 0 AND price < 1e9"),
                (None, "a <> ',' OR b IS NULL")
            ]
        );
        assert_eq!(t.generated["total"], "qty * price");
        assert_eq!(t.generated["half"], "price / 2");
        assert_eq!(parse_create_table("CREATE VIRTUAL TABLE f USING fts5(a, b)"), None);
        let pk = parse_create_table(
            "CREATE TABLE t (a INT, b INT, CONSTRAINT pk_t PRIMARY KEY (a, b)) WITHOUT ROWID",
        );
        assert_eq!(pk.unwrap().primary_key.as_deref(), Some("pk_t"));
    }

    #[test]
    fn index_columns_and_trigger_headers() {
        let sql = "CREATE UNIQUE INDEX \"ix (1)\" ON t (lower(name) COLLATE NOCASE DESC, \"Code\" ASC, (a + b)) WHERE a > 0";
        assert_eq!(index_column(sql, 1).as_deref(), Some("lower(name)"));
        assert_eq!(index_column(sql, 2).as_deref(), Some("\"Code\""));
        assert_eq!(index_column(sql, 3).as_deref(), Some("(a + b)"));
        assert_eq!(index_column(sql, 4), None);
        assert_eq!(
            trigger_header(
                "CREATE TRIGGER t1 AFTER UPDATE OF a ON t BEGIN INSERT INTO log VALUES ('BEFORE DELETE'); END"
            ),
            ("AFTER".to_string(), vec!["UPDATE".to_string()])
        );
        assert_eq!(
            trigger_header("CREATE TRIGGER v1 INSTEAD OF INSERT ON v BEGIN SELECT 1; END"),
            ("INSTEAD OF".to_string(), vec!["INSERT".to_string()])
        );
        assert_eq!(trigger_header("CREATE TRIGGER d DELETE ON t BEGIN SELECT 1; END").0, "BEFORE");
    }
}
