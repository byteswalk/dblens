//! 错误到 HTTP 响应的映射：响应体就是 `dblens_core::Error` 的 JSON（`{"kind": …, …}`），前端按 `kind` 处理。

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use dblens_core::Error;

/// 处理函数的错误类型。
#[derive(Debug)]
pub struct ApiError(pub Error);

impl From<Error> for ApiError {
    fn from(e: Error) -> Self {
        Self(e)
    }
}

pub type ApiResult<T> = std::result::Result<T, ApiError>;

/// 状态码只用来区分大类，细节看响应体的 `kind`：
/// - 400 输入不合法；404 对象不存在；409 会话正忙或已取消；423 密码库已锁定；403 被执行策略拒绝；
/// - 422 数据库一侧的问题（连接、认证、SQL 错误、超时、驱动不可用）；500 程序内部错误。
pub fn status_of(e: &Error) -> StatusCode {
    match e {
        Error::Invalid { .. } => StatusCode::BAD_REQUEST,
        Error::NotFound { .. } => StatusCode::NOT_FOUND,
        Error::Busy { .. } | Error::Cancelled => StatusCode::CONFLICT,
        Error::Locked => StatusCode::LOCKED,
        Error::Denied { .. } => StatusCode::FORBIDDEN,
        Error::Connect { .. }
        | Error::Auth { .. }
        | Error::Sql { .. }
        | Error::Timeout
        | Error::DriverUnavailable { .. } => StatusCode::UNPROCESSABLE_ENTITY,
        Error::Internal { .. } => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = status_of(&self.0);
        if status == StatusCode::INTERNAL_SERVER_ERROR {
            tracing::error!("{}", self.0);
        }
        (status, Json(self.0)).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_codes() {
        assert_eq!(status_of(&Error::not_found("x")), StatusCode::NOT_FOUND);
        assert_eq!(status_of(&Error::invalid("x")), StatusCode::BAD_REQUEST);
        assert_eq!(status_of(&Error::Locked), StatusCode::LOCKED);
        assert_eq!(status_of(&Error::Busy { message: "x".into() }), StatusCode::CONFLICT);
        assert_eq!(status_of(&Error::Auth { message: "x".into() }), StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(status_of(&Error::internal("x")), StatusCode::INTERNAL_SERVER_ERROR);
    }
}
