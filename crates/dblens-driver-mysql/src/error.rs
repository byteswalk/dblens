use dblens_core::{Error, Msg, char_position};
use dblens_driver::mysql_error_offset as locate;

/// MySQL 错误码：查询被 KILL QUERY 中断。
const ER_QUERY_INTERRUPTED: u16 = 1317;

pub(crate) fn map_err(e: mysql_async::Error) -> Error {
    match e {
        mysql_async::Error::Server(ref se) => match se.code {
            ER_QUERY_INTERRUPTED => Error::Cancelled,
            // TiDB 的语句被 KILL 时，部分执行路径不报 1317，而是报这个通用错误
            1105 if se.message == "context canceled" => Error::Cancelled,
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
        assert_eq!(map_err(server(1105, "context canceled")), Error::Cancelled);
        assert!(matches!(map_err(server(1105, "other")), Error::Sql { .. }));
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
