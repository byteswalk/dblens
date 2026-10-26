use dblens_core::{Error, Msg, msg};
use tokio_postgres::error::ErrorPosition;

/// 我们在 `FromSql` / `ToSql` 里返回的错误被 tokio-postgres 包了一层，取回原错误。
pub(crate) fn ours(e: &tokio_postgres::Error) -> Option<Error> {
    std::error::Error::source(e)?.downcast_ref::<Error>().cloned()
}

pub(crate) fn map_err(e: tokio_postgres::Error) -> Error {
    if let Some(inner) = ours(&e) {
        return inner;
    }
    if let Some(db) = e.as_db_error() {
        let code = db.code().code();
        return match code {
            // query_canceled
            "57014" => Error::Cancelled,
            // invalid_password / invalid_authorization_specification
            "28P01" | "28000" => Error::Auth { message: Msg::raw(db.message()) },
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
        return Error::connect(msg!("driver.connection_closed", error = e));
    }
    Error::connect(e.to_string())
}
