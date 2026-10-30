use dblens_core::{Error, Result, msg};

use crate::frame::{Resp, decode_resp};

/// 把响应转成值：错误帧转 `Error::Sql`。
pub(crate) fn expect(kind: u8, body: &[u8], columns: usize) -> Result<Resp> {
    match decode_resp(kind, body, columns)? {
        // -213：语句被用户中断
        Resp::Error { code: -213, .. } => Err(Error::Cancelled),
        Resp::Error { code, message, .. } => {
            Err(Error::Sql { code: code.to_string(), message, position: None })
        }
        other => Ok(other),
    }
}

pub(crate) fn protocol() -> Error {
    Error::connect(msg!("jdbc.bad_frame"))
}
