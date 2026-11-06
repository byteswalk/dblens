use dblens_core::{Error, Msg, msg};
use oracle::ErrorKind;

pub(crate) fn map_err(e: oracle::Error) -> Error {
    let Some(db) = e.db_error() else {
        return Error::internal(e.to_string());
    };
    let message = db.message().to_string();
    if matches!(e.kind(), ErrorKind::DpiError) {
        // DPI-1047：找不到或无法加载 Oracle 客户端库
        if message.contains("DPI-1047") {
            return Error::DriverUnavailable { hint: client_hint(message) };
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

/// Linux 上 `libclntsh.so` 的依赖（如 `libnnz.so`）按系统库搜索路径找，只在连接设置里指定目录不够，
/// 要在启动 DBLENS 前把目录加入 `LD_LIBRARY_PATH` 或用 ldconfig 注册。2026-11-06 在麒麟 ARM64 上实测。
fn client_hint(message: String) -> Msg {
    if cfg!(all(unix, not(target_os = "macos"))) && message.contains("cannot open shared object file") {
        msg!("oracle.linux_library_path", error = message)
    } else {
        Msg::raw(message)
    }
}
