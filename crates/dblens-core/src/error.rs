use std::fmt;

use serde::{Deserialize, Serialize};

use crate::Msg;

/// DBLens 统一错误类型。各层都用它向上传递错误，不允许吞掉。
/// 给用户看的文字字段都是 `Msg`，序列化时按当前语言渲染（M1i 设计文档第 5 节）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Error {
    Connect {
        message: Msg,
    },
    Auth {
        message: Msg,
    },
    /// 数据库返回的 SQL 错误。`code` 是数据库原生错误码（如 `1146`、`42P01`、`ORA-00942`），
    /// `message` 是数据库原文（不翻译），
    /// `position` 是出错位置在 SQL 文本中的字符偏移（从 1 开始），数据库没给时为 None。
    Sql {
        code: String,
        message: String,
        position: Option<u32>,
    },
    Timeout,
    Cancelled,
    DriverUnavailable {
        hint: Msg,
    },
    /// 输入或数据本身不合法（参数错误、值无法表示为字面量等），由调用方修正后重试。
    Invalid {
        message: Msg,
    },
    /// 要找的对象（连接配置、会话等）不存在。`what` 是对象的名词短语，如"会话 7"。
    NotFound {
        what: Msg,
    },
    /// 会话正在执行其他语句。
    Busy {
        message: Msg,
    },
    /// 本地密码库已加锁，需要先输入主密码。
    Locked,
    /// 被执行策略拒绝。
    Denied {
        reason: Msg,
    },
    Internal {
        message: Msg,
    },
}

impl Error {
    /// 整条错误文字：类别前缀加细节，如"连接失败：…"。
    pub fn to_msg(&self) -> Msg {
        match self {
            Error::Connect { message } => crate::msg!("error.kind.connect", message = message),
            Error::Auth { message } => crate::msg!("error.kind.auth", message = message),
            Error::Sql { code, message, .. } => crate::msg!("error.kind.sql", code = code, message = message),
            Error::Timeout => crate::msg!("error.kind.timeout"),
            Error::Cancelled => crate::msg!("error.kind.cancelled"),
            Error::DriverUnavailable { hint } => crate::msg!("error.kind.driver_unavailable", hint = hint),
            Error::Invalid { message } => crate::msg!("error.kind.invalid", message = message),
            Error::NotFound { what } => crate::msg!("error.kind.not_found", what = what),
            Error::Busy { message } => crate::msg!("error.kind.busy", message = message),
            Error::Locked => crate::msg!("error.kind.locked"),
            Error::Denied { reason } => crate::msg!("error.kind.denied", reason = reason),
            Error::Internal { message } => crate::msg!("error.kind.internal", message = message),
        }
    }
}

/// 按当前语言显示（日志、命令行）。
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_msg().current())
    }
}

impl std::error::Error for Error {}

/// `sql` 中的字节偏移（从 0 开始）换算成 `Error::Sql::position` 用的字符偏移（从 1 开始）。
/// 偏移落在字符中间时按该字符算，超出末尾时取末尾之后的位置。
pub fn char_position(sql: &str, byte: usize) -> u32 {
    let mut end = byte.min(sql.len());
    while !sql.is_char_boundary(end) {
        end -= 1;
    }
    (sql[..end].chars().count() + 1) as u32
}

impl Error {
    /// 按字节报告出错位置的数据库（如 Oracle）：把 `Sql` 错误的位置（从 1 开始的字节偏移）换成字符偏移。
    pub fn position_bytes_to_chars(self, sql: &str) -> Self {
        match self {
            Error::Sql { code, message, position: Some(p) } => Error::Sql {
                code,
                message,
                position: Some(char_position(sql, (p as usize).saturating_sub(1))),
            },
            other => other,
        }
    }

    pub fn connect(message: impl Into<Msg>) -> Self {
        Self::Connect { message: message.into() }
    }

    pub fn internal(message: impl Into<Msg>) -> Self {
        Self::Internal { message: message.into() }
    }

    pub fn invalid(message: impl Into<Msg>) -> Self {
        Self::Invalid { message: message.into() }
    }

    pub fn not_found(what: impl Into<Msg>) -> Self {
        Self::NotFound { what: what.into() }
    }
}

pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Lang, with_lang};

    #[test]
    fn display_follows_language_and_contains_code() {
        let e = Error::Sql { code: "1146".into(), message: "表不存在".into(), position: None };
        assert_eq!(with_lang(Lang::ZhCn, || e.to_string()), "SQL 错误 [1146]：表不存在");
        assert_eq!(with_lang(Lang::En, || e.to_string()), "SQL error [1146]: 表不存在");
        assert_eq!(with_lang(Lang::ZhCn, || Error::Cancelled.to_string()), "执行已取消");
        assert_eq!(with_lang(Lang::En, || Error::Cancelled.to_string()), "Execution cancelled");
        let nf = Error::not_found(crate::msg!("noun.session", id = 7));
        assert_eq!(with_lang(Lang::ZhCn, || nf.to_string()), "找不到会话 7");
        assert_eq!(with_lang(Lang::En, || nf.to_string()), "Not found: session 7");
    }

    #[test]
    fn serializes_with_kind_tag() {
        let e = Error::Sql { code: "42P01".into(), message: "x".into(), position: Some(15) };
        let json = serde_json::to_value(&e).unwrap();
        assert_eq!(json, serde_json::json!({"kind":"sql","code":"42P01","message":"x","position":15}));
        assert_eq!(serde_json::to_value(Error::Timeout).unwrap(), serde_json::json!({"kind":"timeout"}));
        let nf = Error::not_found(crate::msg!("noun.profile", id = "x"));
        assert_eq!(
            with_lang(Lang::ZhCn, || serde_json::to_value(&nf).unwrap()),
            serde_json::json!({"kind":"not_found","what":"连接配置 x"})
        );
        assert_eq!(
            with_lang(Lang::En, || serde_json::to_value(&nf).unwrap()),
            serde_json::json!({"kind":"not_found","what":"connection profile x"})
        );
        assert_eq!(serde_json::to_value(Error::Locked).unwrap(), serde_json::json!({"kind":"locked"}));
    }

    #[test]
    fn byte_positions_become_char_positions() {
        let sql = "SELECT '数洞', FROM t";
        let from = sql.find("FROM").unwrap();
        assert_eq!(char_position(sql, from), "SELECT '数洞', ".chars().count() as u32 + 1);
        assert_eq!(char_position(sql, from + 1), char_position(sql, from) + 1);
        // 落在汉字中间按该汉字算；超出末尾取末尾之后
        assert_eq!(char_position(sql, sql.find('洞').unwrap() + 1), 10);
        assert_eq!(char_position(sql, 1000), sql.chars().count() as u32 + 1);
        let e = Error::Sql { code: "ORA-00936".into(), message: "x".into(), position: Some(from as u32 + 1) };
        let Error::Sql { position, .. } = e.position_bytes_to_chars(sql) else { panic!() };
        assert_eq!(position, Some(14));
        assert_eq!(Error::Timeout.position_bytes_to_chars(sql), Error::Timeout);
    }
}
