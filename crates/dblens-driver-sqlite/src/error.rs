use dblens_core::{Error, msg};
use rusqlite::ffi::ErrorCode;

/// SQLite 错误码的名字（`SQLITE_CONSTRAINT_UNIQUE` 这类扩展码只给主码的名字，原文里有细节）。
fn code_name(code: ErrorCode) -> &'static str {
    match code {
        ErrorCode::ConstraintViolation => "SQLITE_CONSTRAINT",
        ErrorCode::DatabaseBusy => "SQLITE_BUSY",
        ErrorCode::DatabaseLocked => "SQLITE_LOCKED",
        ErrorCode::ReadOnly => "SQLITE_READONLY",
        ErrorCode::TypeMismatch => "SQLITE_MISMATCH",
        ErrorCode::TooBig => "SQLITE_TOOBIG",
        ErrorCode::DiskFull => "SQLITE_FULL",
        ErrorCode::DatabaseCorrupt => "SQLITE_CORRUPT",
        ErrorCode::SystemIoFailure => "SQLITE_IOERR",
        ErrorCode::PermissionDenied => "SQLITE_PERM",
        ErrorCode::OperationAborted => "SQLITE_ABORT",
        ErrorCode::SchemaChanged => "SQLITE_SCHEMA",
        ErrorCode::ApiMisuse => "SQLITE_MISUSE",
        ErrorCode::ParameterOutOfRange => "SQLITE_RANGE",
        _ => "SQLITE_ERROR",
    }
}

pub(crate) fn map_err(e: rusqlite::Error) -> Error {
    match e {
        rusqlite::Error::SqliteFailure(f, message) => {
            let message = message.unwrap_or_else(|| f.to_string());
            match f.code {
                ErrorCode::OperationInterrupted => Error::Cancelled,
                ErrorCode::NotADatabase => Error::connect(msg!("sqlite.not_a_database", error = message)),
                ErrorCode::CannotOpen => Error::connect(msg!("sqlite.cannot_open", error = message)),
                code => Error::Sql { code: code_name(code).into(), message, position: None },
            }
        }
        // 语法错误带出错位置（字节偏移，执行语句的地方再换成字符偏移）
        rusqlite::Error::SqlInputError { msg, offset, .. } => Error::Sql {
            code: "SQLITE_ERROR".into(),
            message: msg,
            position: u32::try_from(offset).ok().map(|o| o + 1),
        },
        rusqlite::Error::InvalidParameterCount(given, expected) => {
            Error::invalid(msg!("sqlite.param_count", given = given, expected = expected))
        }
        other => Error::Sql { code: "SQLITE_ERROR".into(), message: other.to_string(), position: None },
    }
}
