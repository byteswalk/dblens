use dblens_core::Error;

/// MySQL 错误码：查询被 KILL QUERY 中断。
const ER_QUERY_INTERRUPTED: u16 = 1317;

pub(crate) fn map_err(e: mysql_async::Error) -> Error {
    match e {
        mysql_async::Error::Server(ref se) => match se.code {
            ER_QUERY_INTERRUPTED => Error::Cancelled,
            // 1044 无库权限，1045 密码错误，1698 用户被拒绝
            1044 | 1045 | 1698 => Error::Auth { message: se.message.clone() },
            code => Error::Sql { code: code.to_string(), message: se.message.clone(), position: None },
        },
        mysql_async::Error::Io(_) | mysql_async::Error::Driver(_) | mysql_async::Error::Url(_) => {
            Error::connect(e.to_string())
        }
        mysql_async::Error::Other(_) => Error::internal(e.to_string()),
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
}
