use dblens_core::{Error, Msg, char_position};

/// MySQL 错误码：查询被 KILL QUERY 中断。
const ER_QUERY_INTERRUPTED: u16 = 1317;

pub(crate) fn map_err(e: mysql_async::Error) -> Error {
    match e {
        mysql_async::Error::Server(ref se) => match se.code {
            ER_QUERY_INTERRUPTED => Error::Cancelled,
            // 1044 无库权限，1045 密码错误，1698 用户被拒绝
            1044 | 1045 | 1698 => Error::Auth { message: Msg::raw(&se.message) },
            code => Error::Sql { code: code.to_string(), message: se.message.clone(), position: None },
        },
        mysql_async::Error::Io(_) | mysql_async::Error::Driver(_) | mysql_async::Error::Url(_) => {
            Error::connect(e.to_string())
        }
        mysql_async::Error::Other(_) => Error::internal(e.to_string()),
    }
}

/// 从语法错误信息找出出错位置：`near` 后面是从出错处开始的原文（可能被截断），在所报的行里找它的第一行；
/// 为空表示语句意外结束，位置在末尾。返回字节偏移。认得两种写法：
/// - MySQL、MariaDB、OceanBase：`… near 'FROM t' at line 2`（OceanBase 在后面另起一行附上节点和追踪号）；
/// - TiDB：`… line 2 column 21 near "FROM t" `。
fn locate(sql: &str, message: &str) -> Option<usize> {
    let (line, near) = mysql_style(message).or_else(|| tidb_style(message))?;
    let line_start = sql.split_inclusive('\n').take(line.saturating_sub(1)).map(str::len).sum::<usize>();
    let snippet = near.split('\n').next().unwrap_or_default();
    if snippet.is_empty() {
        return Some(sql.trim_end().len());
    }
    sql.get(line_start..)?.find(snippet).map(|at| line_start + at)
}

/// `near '…' at line N`：返回行号和 near 后的原文。
fn mysql_style(message: &str) -> Option<(usize, &str)> {
    let (head, tail) = message.rsplit_once("' at line ")?;
    let digits = tail.find(|c: char| !c.is_ascii_digit()).unwrap_or(tail.len());
    let line = tail[..digits].parse().ok()?;
    Some((line, &head[head.find(" near '")? + 7..]))
}

/// TiDB 的 `line N column M near "…" `：near 后的原文到最后一个双引号为止。
fn tidb_style(message: &str) -> Option<(usize, &str)> {
    let at = message.find(" line ")?;
    let rest = &message[at + 6..];
    let digits = rest.find(|c: char| !c.is_ascii_digit())?;
    let line = rest[..digits].parse().ok()?;
    let near = &rest[rest.find(" near \"")? + 7..];
    Some((line, &near[..near.rfind('"')?]))
}

/// 给语法错误补上出错位置（字符偏移，从 1 开始）。
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
    use mysql_async::ServerError;

    fn server(code: u16, message: &str) -> mysql_async::Error {
        mysql_async::Error::Server(ServerError { code, message: message.into(), state: "HY000".into() })
    }

    #[test]
    fn maps_server_errors() {
        assert_eq!(map_err(server(1317, "Query execution was interrupted")), Error::Cancelled);
        assert!(matches!(map_err(server(1045, "Access denied")), Error::Auth { .. }));
        assert_eq!(
            map_err(server(1146, "Table 'x' doesn't exist")),
            Error::Sql { code: "1146".into(), message: "Table 'x' doesn't exist".into(), position: None }
        );
    }

    /// 信息取自 MySQL 9.7 和 MariaDB 12.3 的实测。
    #[test]
    fn locates_syntax_errors() {
        let syntax = |near: &str, line: u32| {
            format!(
                "You have an error in your SQL syntax; check the manual that corresponds to your MySQL server version for the right syntax to use near '{near}' at line {line}"
            )
        };
        let at = |sql: &str, msg: &str| locate(sql, msg).map(|b| sql[b..].to_string());
        let sql = "SELECT '数洞', FROM t";
        assert_eq!(at(sql, &syntax("FROM t", 1)).as_deref(), Some("FROM t"));
        let sql = "SELECT 1,\n  'it''s', FROM t\nWHERE x";
        assert_eq!(at(sql, &syntax("FROM t\nWHERE x", 2)).as_deref(), Some("FROM t\nWHERE x"));
        assert_eq!(at("SELECT 1 FROM DUAL WHERE", &syntax("", 1)).as_deref(), Some(""));
        assert_eq!(at("SELECT 1", "Table 'x' doesn't exist"), None);
        // OceanBase 4.4 与 TiDB 8.5 的实测信息
        let ob = format!(
            "{}\n[127.0.0.1:2882] [2026-10-28 01:14:03.030007] [YB427F000001-00065EDC1E3E1735-0-0]",
            syntax("FROM t", 1)
        );
        assert_eq!(at(sql, &ob).as_deref(), Some("FROM t\nWHERE x"));
        let tidb = "You have an error in your SQL syntax; check the manual that corresponds to your TiDB version for the right syntax to use line 2 column 10 near \"FROM t\nWHERE x\" ";
        assert_eq!(at(sql, tidb).as_deref(), Some("FROM t\nWHERE x"));
        let e = Error::Sql { code: "1064".into(), message: syntax("FROM t", 1), position: None };
        let Error::Sql { position, .. } = with_position(e, "SELECT '数洞', FROM t") else { panic!() };
        assert_eq!(position, Some(14));
    }
}
