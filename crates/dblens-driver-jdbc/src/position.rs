//! 从达梦的错误信息中找出出错位置。达梦不单独返回位置，只在信息里写
//! `第 2 行, 第 13 列[FROM]附近出现错误`。实测列号在第一行和其他行之间差 1，不可靠，
//! 所以在该行里找方括号中的词，取离列号最近的一处；方括号为空（语句意外结束）时按列号取，超出行尾取行尾。

use dblens_core::{Error, char_position};

/// 解析 `第 L 行, 第 C 列[T]`，返回行号、列号和词。没有列号时返回 None。
fn parse(message: &str) -> Option<(usize, usize, &str)> {
    let rest = message.strip_prefix('第')?.trim_start();
    let (line, rest) = rest.split_once('行')?;
    let rest = rest.trim_start().strip_prefix(',')?.trim_start().strip_prefix('第')?.trim_start();
    let (col, rest) = rest.split_once('列')?;
    let token = rest.strip_prefix('[').and_then(|r| r.split_once(']')).map_or("", |(t, _)| t);
    Some((line.trim().parse().ok()?, col.trim().parse().ok()?, token))
}

/// 出错位置在 `sql` 中的字节偏移。
fn locate(sql: &str, message: &str) -> Option<usize> {
    let (line, col, token) = parse(message)?;
    let mut start = 0;
    for (i, text) in sql.split('\n').enumerate() {
        if i + 1 == line {
            let text = text.trim_end_matches('\r');
            let col = col.saturating_sub(1).min(text.len());
            if token.is_empty() {
                return Some(start + col);
            }
            let lower = text.to_ascii_lowercase();
            let token = token.to_ascii_lowercase();
            return lower
                .match_indices(&token)
                .map(|(at, _)| at)
                .min_by_key(|at| at.abs_diff(col))
                .map(|at| start + at);
        }
        start += text.len() + 1;
    }
    None
}

/// 给 SQL 错误补上出错位置（字符偏移，从 1 开始）。
pub(crate) fn with_position(e: Error, sql: &str) -> Error {
    match e {
        Error::Sql { code, message, position: None } => {
            let position = locate(sql, &message).map(|b| char_position(sql, b));
            Error::Sql { code, message, position }
        }
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(sql: &str, message: &str) -> Option<String> {
        locate(sql, message).map(|b| sql[b..].to_string())
    }

    /// 信息取自达梦 DM8 的实测。
    #[test]
    fn locates_from_message() {
        let msg = |l: u32, c: u32, t: &str| format!("第 {l} 行, 第 {c} 列[{t}]附近出现错误: \n语法分析出错");
        assert_eq!(at("SELECT 'ab', FROM t", &msg(1, 13, "FROM")).as_deref(), Some("FROM t"));
        assert_eq!(at("SELECT '数洞', FROM t", &msg(1, 17, "FROM")).as_deref(), Some("FROM t"));
        assert_eq!(at("SELECT 1,\n  '数洞', FROM t", &msg(2, 13, "FROM")).as_deref(), Some("FROM t"));
        // 语句意外结束：方括号为空，按列号取，超出行尾取行尾
        assert_eq!(at("SELECT 1 FROM DUAL WHERE", &msg(1, 25, "")).as_deref(), Some(""));
        assert_eq!(at("SELECT 1 FROM\n DUAL WHERE x =", &msg(2, 16, "")).as_deref(), Some(""));
        // 同一行有多处时取离列号最近的；大小写不敏感
        assert_eq!(at("select a from b, from c", &msg(1, 18, "FROM")).as_deref(), Some("from c"));
        // 没有列号、行号超出、找不到词
        assert_eq!(at("SELECT x FROM DUAL", "第1 行附近出现错误:\n无效的列名[X]"), None);
        assert_eq!(at("SELECT 1", &msg(3, 1, "X")), None);
        assert_eq!(at("SELECT 1", &msg(1, 1, "WHERE")), None);
    }

    #[test]
    fn fills_position_of_sql_errors_only() {
        let sql = "SELECT '数洞', FROM t";
        let e = Error::Sql {
            code: "-2007".into(),
            message: "第 1 行, 第 17 列[FROM]附近出现错误".into(),
            position: None,
        };
        let Error::Sql { position, .. } = with_position(e, sql) else { panic!() };
        assert_eq!(position, Some(14));
        assert_eq!(with_position(Error::Cancelled, sql), Error::Cancelled);
    }
}
