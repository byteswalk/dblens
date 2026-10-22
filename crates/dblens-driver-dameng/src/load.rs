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
    let library = open_library(&path)?;
    let dpi = unsafe { DmDpi::from_library(library) }.map_err(|e| unavailable(&path, e))?;
    let dpi = Arc::new(dpi);
    loaded.push((dir, dpi.clone()));
    Ok(dpi)
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
