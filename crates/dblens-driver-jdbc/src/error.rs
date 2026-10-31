use dblens_core::{Error, Msg, Result, msg};

use crate::frame::{Resp, decode_resp};

/// 各数据库在错误码上的差异。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Flavor {
    Gbase,
    Dameng,
    Oracle,
}

impl Flavor {
    fn error(self, code: i32, sqlstate: &str, message: String) -> Error {
        match self {
            // -213：语句被用户中断
            Flavor::Gbase if code == -213 => Error::Cancelled,
            // -6515：操作被取消
            Flavor::Dameng if code == -6515 || sqlstate == "HY008" => Error::Cancelled,
            // ORA-01013：用户请求取消当前操作
            Flavor::Oracle if code == 1013 => Error::Cancelled,
            Flavor::Oracle => Error::Sql { code: format!("ORA-{code:05}"), message, position: None },
            _ => Error::Sql { code: code.to_string(), message, position: None },
        }
    }

    /// 登录时的错误：账号问题转 `Error::Auth`，不重试；其余是连接失败。
    pub(crate) fn login_error(self, code: i32, message: String) -> Error {
        let auth = match self {
            Flavor::Gbase => matches!(code, -951 | -952),
            Flavor::Dameng => code == -2501,
            Flavor::Oracle => matches!(code, 1017 | 28000 | 28001),
        };
        if auth {
            Error::Auth { message: Msg::raw(format!("[{code}] {message}")) }
        } else {
            Error::connect(format!("[{code}] {message}"))
        }
    }
}

/// 把响应转成值：错误帧按数据库的规则转成 `Error`。
pub(crate) fn expect(kind: u8, body: &[u8], columns: usize, flavor: Flavor) -> Result<Resp> {
    match decode_resp(kind, body, columns)? {
        Resp::Error { code, sqlstate, message } => Err(flavor.error(code, &sqlstate, message)),
        other => Ok(other),
    }
}

pub(crate) fn protocol() -> Error {
    Error::connect(msg!("jdbc.bad_frame"))
}
