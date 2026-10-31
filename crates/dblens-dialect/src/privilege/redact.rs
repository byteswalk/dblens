//! 历史记录的尽力脱敏（M9 设计文档 9.2 节）：`CREATE`/`ALTER` 的 `USER`、`ROLE`、`LOGIN`，`SET PASSWORD`，以及
//! 带 `IDENTIFIED` 的语句，把密码字面量换成 `***`。看不懂（比如引号没闭合）时只留前两个词，宁可少记。

use std::borrow::Cow;

const MASK: &str = "***";

/// `PASSWORD` 后面的这些词是子句，不是密码。
const PASSWORD_CLAUSES: [&str; 6] = ["EXPIRE", "HISTORY", "REUSE", "REQUIRE", "LOCK", "POLICY"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Token {
    Word(usize, usize),
    Text(usize, usize),
    Symbol(usize),
}

/// 切成词、字符串和符号，跳过注释。引号没闭合时返回 None。
fn tokens(sql: &str) -> Option<Vec<Token>> {
    let b = sql.as_bytes();
    let (mut out, mut i) = (Vec::new(), 0);
    while i < b.len() {
        let c = b[i];
        if c.is_ascii_whitespace() {
            i += 1;
        } else if b[i..].starts_with(b"--") {
            i += b[i..].iter().position(|&x| x == b'\n').unwrap_or(b.len() - i);
        } else if b[i..].starts_with(b"/*") {
            i += sql[i..].find("*/").map_or(b.len() - i, |p| p + 2);
        } else if c == b'\'' || c == b'"' || c == b'`' {
            let start = i;
            i = closing(b, i, c)?;
            out.push(Token::Text(start, i));
        } else if (c == b'N' || c == b'n') && b.get(i + 1) == Some(&b'\'') {
            let start = i;
            i = closing(b, i + 1, b'\'')?;
            out.push(Token::Text(start, i));
        } else if c == b'[' {
            let end = sql[i..].find(']')? + i + 1;
            out.push(Token::Word(i, end));
            i = end;
        } else if c.is_ascii_alphanumeric() || c == b'_' || c >= 0x80 {
            let start = i;
            while i < b.len()
                && (b[i].is_ascii_alphanumeric() || matches!(b[i], b'_' | b'$' | b'#') || b[i] >= 0x80)
            {
                i += 1;
            }
            out.push(Token::Word(start, i));
        } else {
            out.push(Token::Symbol(i));
            i += 1;
        }
    }
    Some(out)
}

/// 字符串的结束位置（闭合引号之后）。双写引号和反斜杠转义都算在内。
fn closing(b: &[u8], open: usize, quote: u8) -> Option<usize> {
    let mut i = open + 1;
    while i < b.len() {
        if b[i] == b'\\' && quote == b'\'' {
            i += 2;
        } else if b[i] == quote {
            if b.get(i + 1) == Some(&quote) {
                i += 2;
            } else {
                return Some(i + 1);
            }
        } else {
            i += 1;
        }
    }
    None
}

fn word(sql: &str, t: Option<&Token>) -> Option<String> {
    match t {
        Some(Token::Word(s, e)) => Some(sql[*s..*e].to_ascii_uppercase()),
        _ => None,
    }
}

/// `SET PASSWORD` 要在语句开头，`UPDATE … SET password = …` 不算。
fn sets_password(words: &[String]) -> bool {
    matches!(words, [a, b, ..] if a == "SET" && b == "PASSWORD")
}

fn is_sensitive(sql: &str, t: &[Token]) -> bool {
    let words: Vec<String> = t.iter().map(|x| word(sql, Some(x)).unwrap_or_default()).collect();
    words.windows(2).any(|w| {
        matches!(w[0].as_str(), "CREATE" | "ALTER") && matches!(w[1].as_str(), "USER" | "ROLE" | "LOGIN")
    }) || sets_password(&words)
        || words.iter().any(|w| w == "IDENTIFIED")
}

fn value_range(t: Token) -> (usize, usize) {
    match t {
        Token::Word(s, e) | Token::Text(s, e) => (s, e),
        Token::Symbol(s) => (s, s + 1),
    }
}

/// 密码值的位置：给定关键字之后，跳过 `=`、`(`、`PASSWORD` 这类连接成分。
fn value_after(sql: &str, t: &[Token], mut i: usize) -> Option<(usize, usize)> {
    loop {
        match t.get(i)? {
            Token::Symbol(p) if matches!(sql.as_bytes()[*p], b'=' | b'(') => i += 1,
            Token::Symbol(_) => return None,
            Token::Word(..) => {
                let w = word(sql, t.get(i))?;
                if w == "PASSWORD" {
                    i += 1;
                } else if PASSWORD_CLAUSES.contains(&w.as_str()) {
                    return None;
                } else {
                    return Some(value_range(t[i]));
                }
            }
            Token::Text(..) => return Some(value_range(t[i])),
        }
    }
}

fn first_words(sql: &str) -> String {
    let words: Vec<&str> = sql.split_whitespace().take(2).collect();
    format!("{} {MASK}", words.join(" "))
}

/// 脱敏后的语句。不是账号类语句时原样返回。
pub fn redact_sql(sql: &str) -> Cow<'_, str> {
    let lower = sql.to_ascii_lowercase();
    if !["user", "role", "login", "password", "identified"].iter().any(|k| lower.contains(k)) {
        return Cow::Borrowed(sql);
    }
    let Some(t) = tokens(sql) else {
        let upper = lower.to_ascii_uppercase();
        let suspicious =
            ["IDENTIFIED", "PASSWORD", "CREATE USER", "ALTER USER", "CREATE LOGIN", "ALTER LOGIN"];
        return if suspicious.iter().any(|k| upper.contains(k)) {
            Cow::Owned(first_words(sql))
        } else {
            Cow::Borrowed(sql)
        };
    };
    if !is_sensitive(sql, &t) {
        return Cow::Borrowed(sql);
    }
    let mut ranges = Vec::new();
    for i in 0..t.len() {
        match word(sql, t.get(i)).as_deref() {
            Some("BY" | "PASSWORD" | "OLD_PASSWORD" | "REPLACE" | "AS") => {
                ranges.extend(value_after(sql, &t, i + 1));
            }
            _ => {}
        }
    }
    // SET PASSWORD [FOR 账号] = '…'：等号后面都不留
    let first: Vec<String> = t.iter().take(2).map(|x| word(sql, Some(x)).unwrap_or_default()).collect();
    if sets_password(&first) {
        let eq = t.iter().position(|x| matches!(x, Token::Symbol(p) if sql.as_bytes()[*p] == b'='));
        if let Some(eq) = eq {
            ranges
                .extend(t[eq + 1..].iter().filter(|x| matches!(x, Token::Text(..))).map(|x| value_range(*x)));
        }
    }
    ranges.sort_unstable();
    ranges.dedup();
    let (mut out, mut at) = (String::with_capacity(sql.len()), 0);
    for (s, e) in ranges {
        if s < at {
            continue;
        }
        out.push_str(&sql[at..s]);
        out.push_str(MASK);
        at = e;
    }
    out.push_str(&sql[at..]);
    Cow::Owned(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const S: &str = "S3ntinel!pw";

    #[test]
    fn masks_every_dialect_form() {
        for sql in [
            format!("CREATE USER 'u'@'%' IDENTIFIED BY '{S}'"),
            format!("create user u identified by {S}"),
            format!("CREATE USER U IDENTIFIED BY \"{S}\""),
            format!("ALTER USER 'u'@'%' IDENTIFIED WITH caching_sha2_password BY '{S}'"),
            format!("ALTER USER u IDENTIFIED BY '{S}' REPLACE '{S}'"),
            format!("CREATE ROLE u LOGIN PASSWORD '{S}'"),
            format!("ALTER ROLE u WITH PASSWORD '{S}'"),
            format!("CREATE USER u WITH PASSWORD = '{S}'"),
            format!("CREATE LOGIN [u] WITH PASSWORD = N'{S}', CHECK_POLICY = OFF"),
            format!("ALTER LOGIN [u] WITH PASSWORD = N'{S}' OLD_PASSWORD = N'{S}'"),
            format!("SET PASSWORD = '{S}'"),
            format!("SET PASSWORD FOR 'u'@'%' = PASSWORD('{S}')"),
            format!("SET PASSWORD FOR u = '{S}'"),
            format!("GRANT ALL ON *.* TO 'u'@'%' IDENTIFIED BY '{S}'"),
            format!("/* x */ CREATE USER 'u' IDENTIFIED BY 'it''s{S}'"),
        ] {
            let got = redact_sql(&sql);
            assert!(!got.contains(S), "{sql} -> {got}");
            assert!(got.contains(MASK), "{sql} -> {got}");
        }
    }

    #[test]
    fn keeps_the_rest_of_the_statement() {
        assert_eq!(
            redact_sql("CREATE LOGIN [u] WITH PASSWORD = N'x', CHECK_POLICY = OFF"),
            "CREATE LOGIN [u] WITH PASSWORD = ***, CHECK_POLICY = OFF"
        );
        assert_eq!(redact_sql("ALTER USER 'u'@'%' PASSWORD EXPIRE"), "ALTER USER 'u'@'%' PASSWORD EXPIRE");
        assert_eq!(redact_sql("ALTER USER 'u'@'%' ACCOUNT LOCK"), "ALTER USER 'u'@'%' ACCOUNT LOCK");
    }

    #[test]
    fn ordinary_statements_are_untouched() {
        for sql in [
            "SELECT password FROM users WHERE login = 'x'",
            "UPDATE users SET password = 'x'",
            "SELECT * FROM t",
            "INSERT INTO users(role) VALUES ('it''s')",
            "SELECT 'unterminated role",
        ] {
            assert!(matches!(redact_sql(sql), Cow::Borrowed(_)), "{sql}");
        }
    }

    #[test]
    fn falls_back_to_first_words_when_unparsable() {
        let sql = format!("CREATE USER u IDENTIFIED BY '{S}");
        assert_eq!(redact_sql(&sql), format!("CREATE USER {MASK}"));
    }
}
