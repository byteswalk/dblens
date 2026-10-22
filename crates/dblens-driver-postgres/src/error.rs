use dblens_core::Error;
use tokio_postgres::error::ErrorPosition;

pub(crate) fn map_err(e: tokio_postgres::Error) -> Error {
    if let Some(db) = e.as_db_error() {
        let code = db.code().code();
        return match code {
            // query_canceled
            "57014" => Error::Cancelled,
            // invalid_password / invalid_authorization_specification
            "28P01" | "28000" => Error::Auth { message: db.message().to_string() },
            _ => Error::Sql {
                code: code.to_string(),
                message: db.message().to_string(),
                position: match db.position() {
                    Some(ErrorPosition::Original(p)) => Some(*p),
                    _ => None,
                },
            },
        };
    }
    if e.is_closed() {
        return Error::connect(format!("连接已断开：{e}"));
    }
    Error::connect(e.to_string())
}
