use dblens_core::{Error, Msg};
use oracle::ErrorKind;

pub(crate) fn map_err(e: oracle::Error) -> Error {
    let Some(db) = e.db_error() else {
        return Error::internal(e.to_string());
    };
    let message = db.message().to_string();
    if matches!(e.kind(), ErrorKind::DpiError) {
        // DPI-1047：找不到或无法加载 Oracle 客户端库
        if message.contains("DPI-1047") {
            return Error::DriverUnavailable { hint: Msg::raw(message) };
        }
        return Error::internal(message);
    }
    match db.code() {
        // ORA-01013：用户请求取消当前操作
        1013 => Error::Cancelled,
        1017 | 28000 | 28001 => Error::Auth { message: Msg::raw(message) },
        12154 | 12170 | 12505 | 12514 | 12541 | 12543 => Error::connect(message),
        code => Error::Sql {
            code: format!("ORA-{code:05}"),
            message,
            // 这里是字节偏移，执行语句的地方再换成字符偏移（见 `Error::position_bytes_to_chars`）
            position: (db.offset() > 0).then(|| db.offset() + 1),
        },
    }
}
