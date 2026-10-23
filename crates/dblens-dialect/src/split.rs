//! SQL 语句拆分：处理字符串、注释、PG 的 $$ 引号、MySQL 的 DELIMITER 命令，
//! 以及 Oracle/达梦的 PL/SQL 块和 `/` 结束符。保留每条语句在原文中的字节范围，便于定位出错位置。

use crate::Family;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SqlStatement {
    /// 语句文本：不含结束符，已去掉首尾空白。
    pub text: String,
    /// 在原文中的字节范围，满足 `&sql[start..end] == text`。
    pub start: usize,
    pub end: usize,
}

/// 按家族的语法规则拆分多条语句。
pub fn split_statements(sql: &str, family: Family) -> Vec<SqlStatement> {
    let mut s = Splitter {
        sql,
        b: sql.as_bytes(),
        family,
        i: 0,
        stmt_start: 0,
        has_code: false,
        block: false,
        delimiter: b";".to_vec(),
        out: Vec::new(),
    };
    s.run();
    s.out
}

struct Splitter<'a> {
    sql: &'a str,
    b: &'a [u8],
    family: Family,
    i: usize,
    stmt_start: usize,
    /// 当前语句是否已出现注释以外的内容。
    has_code: bool,
    /// 是否处于 PL/SQL 块中（块内 `;` 不拆分）。
    block: bool,
    delimiter: Vec<u8>,
    out: Vec<SqlStatement>,
}

fn is_ident_byte(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_' || c >= 0x80
}

impl Splitter<'_> {
    fn plsql(&self) -> bool {
        matches!(self.family, Family::Oracle | Family::Dameng)
    }

    fn run(&mut self) {
        while self.i < self.b.len() {
            if (self.i == 0 || self.b[self.i - 1] == b'\n') && self.line_command() {
                continue;
            }
            if self.skip_comment() || self.skip_string() {
                continue;
            }
            if !self.block && self.b[self.i..].starts_with(&self.delimiter) {
                let end = self.i;
                self.i += self.delimiter.len();
                self.finish(end);
                continue;
            }
            if !self.b[self.i].is_ascii_whitespace() {
                self.mark_code();
            }
            self.i += 1;
        }
        self.finish(self.b.len());
    }

    /// 遇到第一段非注释内容时记下，并判断是否是 PL/SQL 块的开头。
    fn mark_code(&mut self) {
        if !self.has_code {
            self.has_code = true;
            self.block = self.plsql() && starts_plsql(&self.b[self.i..]);
        }
    }

    /// 结束当前语句（范围到 `end` 为止），并从 `self.i` 开始新语句。
    fn finish(&mut self, end: usize) {
        if self.has_code {
            let mut s = self.stmt_start;
            let mut e = end;
            while s < e && self.b[s].is_ascii_whitespace() {
                s += 1;
            }
            while e > s && self.b[e - 1].is_ascii_whitespace() {
                e -= 1;
            }
            if s < e {
                self.out.push(SqlStatement { text: self.sql[s..e].to_string(), start: s, end: e });
            }
        }
        self.stmt_start = self.i;
        self.has_code = false;
        self.block = false;
    }

    /// 行首命令：MySQL 的 `DELIMITER xx`，Oracle/达梦单独一行的 `/`。处理了就返回 true。
    fn line_command(&mut self) -> bool {
        let mut j = self.i;
        while j < self.b.len() && matches!(self.b[j], b' ' | b'\t') {
            j += 1;
        }
        let line_end = self.b[j..].iter().position(|&c| c == b'\n').map_or(self.b.len(), |p| j + p);
        let line = self.b[j..line_end].trim_ascii_end();
        let next_line = (line_end + 1).min(self.b.len());
        match self.family {
            Family::MySql
                if line.len() > 9
                    && line[..9].eq_ignore_ascii_case(b"delimiter")
                    && line[9].is_ascii_whitespace() =>
            {
                let new = line[9..].trim_ascii();
                if new.is_empty() {
                    return false;
                }
                self.finish(self.i);
                self.delimiter = new.to_vec();
                self.i = next_line;
                self.stmt_start = next_line;
                true
            }
            Family::Oracle | Family::Dameng if line == b"/" => {
                self.finish(self.i);
                self.i = next_line;
                self.stmt_start = next_line;
                true
            }
            _ => false,
        }
    }

    fn skip_comment(&mut self) -> bool {
        let b = self.b;
        let i = self.i;
        let next = b.get(i + 1).copied();
        let line_end = |from: usize| b[from..].iter().position(|&c| c == b'\n').map_or(b.len(), |p| from + p);
        if b[i] == b'-' && next == Some(b'-') {
            // MySQL 要求 `--` 后跟空白或行尾才是注释
            if self.family == Family::MySql
                && !matches!(b.get(i + 2), None | Some(b' ' | b'\t' | b'\r' | b'\n'))
            {
                return false;
            }
            self.i = line_end(i);
            return true;
        }
        if b[i] == b'#' && self.family == Family::MySql {
            self.i = line_end(i);
            return true;
        }
        if b[i] == b'/' && next == Some(b'*') {
            // MySQL 的 /*! … */ 与 Oracle 的 /*+ … */ 提示属于语句内容
            if matches!(b.get(i + 2), Some(b'!' | b'+')) {
                self.mark_code();
            }
            let nested = self.family == Family::Postgres;
            let mut depth = 1;
            let mut j = i + 2;
            while j < b.len() && depth > 0 {
                if b[j] == b'*' && b.get(j + 1) == Some(&b'/') {
                    depth -= 1;
                    j += 2;
                } else if nested && b[j] == b'/' && b.get(j + 1) == Some(&b'*') {
                    depth += 1;
                    j += 2;
                } else {
                    j += 1;
                }
            }
            self.i = j.min(b.len());
            return true;
        }
        false
    }

    fn skip_string(&mut self) -> bool {
        let b = self.b;
        let i = self.i;
        let c = b[i];
        let prev_is_ident = i > 0 && is_ident_byte(b[i - 1]);
        let end = if c == b'\'' {
            let pg_escape = self.family == Family::Postgres
                && i > 0
                && matches!(b[i - 1], b'E' | b'e')
                && (i < 2 || !is_ident_byte(b[i - 2]));
            Some(scan_quoted(b, i + 1, b'\'', self.family == Family::MySql || pg_escape))
        } else if c == b'"' {
            Some(scan_quoted(b, i + 1, b'"', self.family == Family::MySql))
        } else if c == b'`' && self.family == Family::MySql {
            Some(scan_quoted(b, i + 1, b'`', false))
        } else if matches!(c, b'q' | b'Q')
            && self.family == Family::Oracle
            && b.get(i + 1) == Some(&b'\'')
            && !prev_is_ident
        {
            Some(scan_q_quote(b, i + 2))
        } else if c == b'$' && self.family == Family::Postgres && !prev_is_ident {
            dollar_tag_len(b, i).map(|len| scan_dollar(b, i, len))
        } else {
            None
        };
        match end {
            Some(e) => {
                self.mark_code();
                self.i = e;
                true
            }
            None => false,
        }
    }
}

/// 扫描引号字符串，返回结束引号之后的位置；没有结束引号时返回文本末尾。
fn scan_quoted(b: &[u8], mut j: usize, quote: u8, backslash: bool) -> usize {
    while j < b.len() {
        let c = b[j];
        if backslash && c == b'\\' {
            j += 2;
            continue;
        }
        if c == quote {
            if b.get(j + 1) == Some(&quote) {
                j += 2;
                continue;
            }
            return j + 1;
        }
        j += 1;
    }
    b.len()
}

/// Oracle 的 q'X…X' 字符串，`start` 指向定界符 X。
fn scan_q_quote(b: &[u8], start: usize) -> usize {
    let Some(&open) = b.get(start) else { return b.len() };
    let close = match open {
        b'[' => b']',
        b'{' => b'}',
        b'(' => b')',
        b'<' => b'>',
        c => c,
    };
    let mut j = start + 1;
    while j + 1 < b.len() {
        if b[j] == close && b[j + 1] == b'\'' {
            return j + 2;
        }
        j += 1;
    }
    b.len()
}

/// PG 的 `$标签$` 或 `$$`：返回标签长度（含两端的 `$`），不是标签时返回 None。
fn dollar_tag_len(b: &[u8], i: usize) -> Option<usize> {
    let mut j = i + 1;
    if b.get(j) == Some(&b'$') {
        return Some(2);
    }
    if !b.get(j).is_some_and(|c| c.is_ascii_alphabetic() || *c == b'_') {
        return None;
    }
    while j < b.len() && (b[j].is_ascii_alphanumeric() || b[j] == b'_') {
        j += 1;
    }
    (b.get(j) == Some(&b'$')).then_some(j - i + 1)
}

fn scan_dollar(b: &[u8], i: usize, tag_len: usize) -> usize {
    let tag = &b[i..i + tag_len];
    let body = i + tag_len;
    b[body..].windows(tag_len).position(|w| w == tag).map_or(b.len(), |p| body + p + tag_len)
}

/// 语句开头是否是 PL/SQL 块（Oracle/达梦）。
fn starts_plsql(s: &[u8]) -> bool {
    let words = leading_words(s, 6);
    let w = |k: usize| words.get(k).map(String::as_str).unwrap_or("");
    match w(0) {
        "DECLARE" | "BEGIN" => true,
        "CREATE" => {
            let mut k = 1;
            if w(1) == "OR" && w(2) == "REPLACE" {
                k = 3;
            }
            if matches!(w(k), "EDITIONABLE" | "NONEDITIONABLE" | "EDITIONING") {
                k += 1;
            }
            matches!(w(k), "PROCEDURE" | "FUNCTION" | "PACKAGE" | "TRIGGER" | "TYPE" | "LIBRARY" | "CLASS")
        }
        _ => false,
    }
}

fn leading_words(s: &[u8], n: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut j = 0;
    while out.len() < n {
        while j < s.len() && s[j].is_ascii_whitespace() {
            j += 1;
        }
        let start = j;
        while j < s.len() && (s[j].is_ascii_alphanumeric() || s[j] == b'_') {
            j += 1;
        }
        if start == j {
            break;
        }
        out.push(String::from_utf8_lossy(&s[start..j]).to_ascii_uppercase());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 拆分并校验偏移量与文本一致，返回各语句文本。
    fn texts(sql: &str, family: Family) -> Vec<String> {
        let out = split_statements(sql, family);
        for s in &out {
            assert_eq!(&sql[s.start..s.end], s.text, "偏移量与文本不一致：{s:?}");
        }
        out.into_iter().map(|s| s.text).collect()
    }

    #[test]
    fn basic_and_trailing() {
        assert_eq!(texts("SELECT 1; SELECT 2", Family::MySql), ["SELECT 1", "SELECT 2"]);
        assert_eq!(texts("  SELECT 1 ;\n\n", Family::Postgres), ["SELECT 1"]);
        assert!(texts("", Family::MySql).is_empty());
        assert!(texts(";;; \n ;", Family::Oracle).is_empty());
        assert!(texts("-- 只有注释\n/* 还有 */", Family::Postgres).is_empty());
    }

    #[test]
    fn offsets_are_byte_positions() {
        let sql = "SELECT '数洞;'; SELECT 2";
        let out = split_statements(sql, Family::MySql);
        assert_eq!(out.len(), 2);
        assert_eq!((out[0].start, out[0].end), (0, 16));
        assert_eq!(out[1].text, "SELECT 2");
        assert_eq!(out[1].start, 18);
    }

    #[test]
    fn mysql_quotes_and_escapes() {
        assert_eq!(
            texts("SELECT 'a;b'; SELECT \"c;d\"; SELECT `e;f`", Family::MySql),
            ["SELECT 'a;b'", "SELECT \"c;d\"", "SELECT `e;f`"]
        );
        assert_eq!(
            texts("SELECT 'it''s; ok', 'a\\';b'; SELECT 2", Family::MySql),
            ["SELECT 'it''s; ok', 'a\\';b'", "SELECT 2"]
        );
    }

    #[test]
    fn mysql_comments() {
        assert_eq!(
            texts("-- x;\nSELECT 1; # y;\nSELECT 2 /* ; */;", Family::MySql),
            ["-- x;\nSELECT 1", "# y;\nSELECT 2 /* ; */"]
        );
        // MySQL 中 `--` 后不跟空白就不是注释
        assert_eq!(texts("SELECT 5--2; SELECT 1", Family::MySql), ["SELECT 5--2", "SELECT 1"]);
    }

    #[test]
    fn mysql_delimiter() {
        let sql =
            "DELIMITER $$\nCREATE PROCEDURE p() BEGIN SELECT 1; SELECT 2; END$$\nDELIMITER ;\nSELECT 3;";
        assert_eq!(
            texts(sql, Family::MySql),
            ["CREATE PROCEDURE p() BEGIN SELECT 1; SELECT 2; END", "SELECT 3"]
        );
        // DELIMITER 前未结束的语句也会被结束
        assert_eq!(texts("SELECT 0\ndelimiter //\nSELECT 1//", Family::MySql), ["SELECT 0", "SELECT 1"]);
    }

    #[test]
    fn postgres_dollar_quotes_and_strings() {
        let f = "CREATE FUNCTION f() RETURNS int AS $$ BEGIN RETURN 1; END; $$ LANGUAGE plpgsql; SELECT 1";
        assert_eq!(texts(f, Family::Postgres).len(), 2);
        assert_eq!(texts("DO $body$ BEGIN PERFORM 1; END $body$; SELECT 2", Family::Postgres).len(), 2);
        assert_eq!(texts("SELECT $1; SELECT 2", Family::Postgres), ["SELECT $1", "SELECT 2"]);
        assert_eq!(texts("SELECT E'a\\';b'; SELECT 2", Family::Postgres), ["SELECT E'a\\';b'", "SELECT 2"]);
        // 普通字符串里反斜杠不转义
        assert_eq!(texts("SELECT 'a\\'; SELECT 2", Family::Postgres), ["SELECT 'a\\'", "SELECT 2"]);
        assert_eq!(texts("SELECT \"a;b\" FROM t; SELECT 2", Family::Postgres).len(), 2);
    }

    #[test]
    fn postgres_nested_comments_and_dash_comments() {
        assert_eq!(
            texts("/* a /* b */ ; */ SELECT 1; SELECT 2", Family::Postgres),
            ["/* a /* b */ ; */ SELECT 1", "SELECT 2"]
        );
        // PG 中 `--` 总是注释，会吃掉行内的分号
        assert_eq!(texts("SELECT 5--2;\nSELECT 1", Family::Postgres), ["SELECT 5--2;\nSELECT 1"]);
    }

    #[test]
    fn oracle_plsql_blocks_and_slash() {
        let sql = "BEGIN\n  NULL;\n  NULL;\nEND;\n/\nSELECT 1 FROM DUAL;";
        assert_eq!(texts(sql, Family::Oracle), ["BEGIN\n  NULL;\n  NULL;\nEND;", "SELECT 1 FROM DUAL"]);
        let proc = "CREATE OR REPLACE PROCEDURE p IS\nBEGIN\n  NULL;\nEND;\n/\n";
        assert_eq!(texts(proc, Family::Oracle), ["CREATE OR REPLACE PROCEDURE p IS\nBEGIN\n  NULL;\nEND;"]);
        let ed = "create or replace editionable function f return number is begin return 1; end;\n/";
        assert_eq!(texts(ed, Family::Oracle).len(), 1);
        assert_eq!(
            texts("CREATE TABLE t (a INT);\nINSERT INTO t VALUES (1);", Family::Oracle),
            ["CREATE TABLE t (a INT)", "INSERT INTO t VALUES (1)"]
        );
        assert_eq!(
            texts("SELECT 1 FROM DUAL\n/\nSELECT 2 FROM DUAL\n  /  \n", Family::Oracle),
            ["SELECT 1 FROM DUAL", "SELECT 2 FROM DUAL"]
        );
        assert_eq!(texts("SELECT 4 / 2 FROM DUAL;", Family::Oracle), ["SELECT 4 / 2 FROM DUAL"]);
    }

    #[test]
    fn oracle_q_quote() {
        assert_eq!(
            texts("SELECT q'[a;b]' FROM DUAL; SELECT Q'#c;d#' FROM DUAL", Family::Oracle),
            ["SELECT q'[a;b]' FROM DUAL", "SELECT Q'#c;d#' FROM DUAL"]
        );
    }

    #[test]
    fn dameng_uses_plsql_rules() {
        let sql = "DECLARE\n  x INT;\nBEGIN\n  x := 1;\nEND;\n/\nSELECT 1;";
        assert_eq!(texts(sql, Family::Dameng), ["DECLARE\n  x INT;\nBEGIN\n  x := 1;\nEND;", "SELECT 1"]);
    }

    #[test]
    fn unterminated_string_runs_to_end() {
        assert_eq!(texts("SELECT 'abc; SELECT 2", Family::MySql), ["SELECT 'abc; SELECT 2"]);
    }
}
