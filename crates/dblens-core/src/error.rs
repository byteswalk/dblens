use serde::{Deserialize, Serialize};

/// DBLens 统一错误类型。各层都用它向上传递错误，不允许吞掉。
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Error {
    #[error("连接失败：{message}")]
    Connect { message: String },
    #[error("认证失败：{message}")]
    Auth { message: String },
    /// 数据库返回的 SQL 错误。`code` 是数据库原生错误码（如 `1146`、`42P01`、`ORA-00942`），
    /// `position` 是出错位置在 SQL 文本中的字符偏移（从 1 开始），数据库没给时为 None。
    #[error("SQL 错误 [{code}]：{message}")]
    Sql { code: String, message: String, position: Option<u32> },
    #[error("执行超时")]
    Timeout,
    #[error("执行已取消")]
    Cancelled,
    #[error("驱动不可用：{hint}")]
    DriverUnavailable { hint: String },
    #[error("内部错误：{message}")]
    Internal { message: String },
}

impl Error {
    pub fn connect(message: impl Into<String>) -> Self {
        Self::Connect { message: message.into() }
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::Internal { message: message.into() }
    }
}

pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_is_chinese_and_contains_code() {
        let e = Error::Sql { code: "1146".into(), message: "表不存在".into(), position: None };
        assert_eq!(e.to_string(), "SQL 错误 [1146]：表不存在");
        assert_eq!(Error::Cancelled.to_string(), "执行已取消");
    }

    #[test]
    fn serializes_with_kind_tag() {
        let e = Error::Sql { code: "42P01".into(), message: "x".into(), position: Some(15) };
        let json = serde_json::to_value(&e).unwrap();
        assert_eq!(json, serde_json::json!({"kind":"sql","code":"42P01","message":"x","position":15}));
        assert_eq!(serde_json::to_value(Error::Timeout).unwrap(), serde_json::json!({"kind":"timeout"}));
    }
}
