//! 查找并加载 dmdpi。同一目录只加载一次，进程内共享。

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use dblens_core::{Error, Result};
use dblens_driver::ConnectParams;
use dblens_driver::native::{self, NativeLib};

use crate::ffi::DmDpi;

static LOADED: Mutex<Vec<(PathBuf, Arc<DmDpi>)>> = Mutex::new(Vec::new());

/// 按连接参数中的 `dpi_lib_dir` 选项（或自动查找）加载 DPI 库。
pub(crate) fn load(params: &ConnectParams) -> Result<Arc<DmDpi>> {
    let configured = params.options.get("dpi_lib_dir").map(PathBuf::from);
    let dir = native::locate(NativeLib::DamengDpi, configured.as_deref())?;
    let mut loaded = LOADED.lock().map_err(|_| Error::internal("DPI 加载锁已损坏"))?;
    if let Some((_, dpi)) = loaded.iter().find(|(d, _)| *d == dir) {
        return Ok(dpi.clone());
    }
    let path = dir.join(NativeLib::DamengDpi.file_name());
    warm_up_winsock();
    let library = open_library(&path)?;
    let dpi = unsafe { DmDpi::from_library(library) }.map_err(|e| unavailable(&path, e))?;
    // 缺少函数时直接返回、不缓存：用户换一个目录（或升级客户端）后可以重试
    verify_functions(&dpi, &path)?;
    let dpi = Arc::new(dpi);
    loaded.push((dir, dpi.clone()));
    Ok(dpi)
}

/// dmdpi 在 DLL 初始化例程里（持有 Windows 加载器锁时）调用 getaddrinfo。如果这时另一个线程正在进程内
/// 第一次创建 socket，ws2_32 会持有自己的目录锁、再去加载协议提供程序 DLL 而等待加载器锁，两边互相等待
/// （实测：达梦连接与 PostgreSQL 连接同时发起时整个进程卡死）。所以加载前先在本线程把 Winsock 的协议
/// 目录和名字解析初始化好，让 dmdpi 初始化时不再触发 DLL 加载。其他平台没有这个问题。
fn warm_up_winsock() {
    #[cfg(windows)]
    {
        use std::net::{ToSocketAddrs, UdpSocket};
        let _ = UdpSocket::bind("127.0.0.1:0");
        let _ = UdpSocket::bind("[::1]:0");
        let _ = "localhost:0".to_socket_addrs();
    }
}

/// 本 crate 调用的全部 DPI 函数。bindgen 生成的方法在函数缺失时会 panic，
/// 所以加载时先检查，缺失时报告驱动不可用。新用到一个 DPI 函数时，要加到这里。
macro_rules! required_functions {
    ($($name:ident),* $(,)?) => {
        fn missing_functions(dpi: &DmDpi) -> Vec<&'static str> {
            let mut missing = Vec::new();
            $(
                if dpi.$name.is_err() {
                    missing.push(stringify!($name));
                }
            )*
            missing
        }
    };
}

required_functions!(
    dpi_alloc_env,
    dpi_free_env,
    dpi_alloc_con,
    dpi_free_con,
    dpi_alloc_stmt,
    dpi_free_stmt,
    dpi_set_con_attr,
    dpi_login,
    dpi_logout,
    dpi_commit,
    dpi_rollback,
    dpi_exec_direct,
    dpi_prepare,
    dpi_exec,
    dpi_bind_param,
    dpi_number_columns,
    dpi_desc_column,
    dpi_fetch,
    dpi_get_data,
    dpi_row_count,
    dpi_get_diag_rec,
    dpi_cancel,
);

fn verify_functions(dpi: &DmDpi, path: &Path) -> Result<()> {
    let missing = missing_functions(dpi);
    if missing.is_empty() {
        return Ok(());
    }
    Err(unavailable(path, format!("缺少 DPI 函数 {}，达梦客户端版本可能过旧", missing.join("、"))))
}

fn unavailable(path: &Path, e: impl std::fmt::Display) -> Error {
    Error::DriverUnavailable { hint: format!("加载 {} 失败：{e}", path.display()) }
}

#[cfg(windows)]
pub(crate) fn open_library(path: &Path) -> Result<libloading::Library> {
    use std::ffi::OsString;
    use std::os::windows::ffi::{OsStrExt, OsStringExt};

    use libloading::os::windows::{LOAD_WITH_ALTERED_SEARCH_PATH, Library};

    // 让 dmdpi.dll 的依赖（dmcalc.dll 等）从它所在的目录加载。
    // 这个标志要求路径用反斜杠：用户填 `E:/dmdbms/drivers/dpi` 时依赖会找不到（错误 126），所以先统一分隔符
    let wide: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .map(|c| if c == u16::from(b'/') { u16::from(b'\\') } else { c })
        .collect();
    let path = PathBuf::from(OsString::from_wide(&wide));
    unsafe { Library::load_with_flags(&path, LOAD_WITH_ALTERED_SEARCH_PATH) }
        .map(Into::into)
        .map_err(|e| unavailable(&path, e))
}

#[cfg(unix)]
pub(crate) fn open_library(path: &Path) -> Result<libloading::Library> {
    // Linux 上 libdmdpi.so 的依赖要靠 LD_LIBRARY_PATH 找到，结论见技术验证笔记
    unsafe { libloading::Library::new(path) }.map_err(|e| unavailable(path, e))
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn missing_functions_are_reported_as_driver_unavailable() {
        // 用一个肯定不含 DPI 函数的系统 DLL 模拟版本不符的客户端
        let library = unsafe { libloading::Library::new("kernel32.dll") }.unwrap();
        let dpi = unsafe { DmDpi::from_library(library) }.unwrap();
        assert_eq!(missing_functions(&dpi).len(), 22);
        let path = Path::new(r"C:\fake\dmdpi.dll");
        match verify_functions(&dpi, path) {
            Err(Error::DriverUnavailable { hint }) => {
                assert!(hint.contains(r"C:\fake\dmdpi.dll"), "{hint}");
                assert!(hint.contains("dpi_cancel") && hint.contains("dpi_alloc_env"), "{hint}");
            }
            other => panic!("应报告驱动不可用：{other:?}"),
        }
    }
}
