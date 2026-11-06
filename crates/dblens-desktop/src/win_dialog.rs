//! Windows 上能输入新文件名的"打开"对话框。
//!
//! 系统"打开"对话框默认要求文件已存在；"另存为"对话框能输入新名字，但选中已有文件时会问"是否替换"，
//! 对"打开已有库或新建库"来说两者都不对。对话框插件（rfd）没有开放这两个选项，这里直接调用
//! `IFileOpenDialog`，去掉"文件必须存在"，保留"路径必须存在"。

use std::path::Path;

use windows::Win32::Foundation::HWND;
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoCreateInstance, CoInitializeEx,
    CoTaskMemFree, CoUninitialize,
};
use windows::Win32::UI::Shell::Common::COMDLG_FILTERSPEC;
use windows::Win32::UI::Shell::{
    FILEOPENDIALOGOPTIONS, FOS_FILEMUSTEXIST, FOS_FORCEFILESYSTEM, FileOpenDialog, IFileOpenDialog,
    IShellItem, SHCreateItemFromParsingName, SIGDN_FILESYSPATH,
};
use windows::core::{HSTRING, PCWSTR};

use crate::shell::FileFilter;

/// 对话框的初始状态。
pub struct Options {
    pub filters: Vec<FileFilter>,
    /// 打开时所在的目录（不存在时由系统决定）。
    pub directory: Option<String>,
    /// 预填的文件名。
    pub file_name: Option<String>,
    /// 用户输入的名字没有扩展名时补上的扩展名（不带点）。
    pub default_extension: Option<String>,
}

/// 显示对话框，返回选中或输入的完整路径；用户取消时返回 None。
///
/// COM 对话框要在单线程套间里运行，这里开一个新线程，调用方的线程不受影响。
pub fn pick_file_allow_new(owner: Option<isize>, opts: Options) -> Option<String> {
    std::thread::spawn(move || unsafe {
        let init = CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE);
        let result = show(owner.map(|h| HWND(h as *mut _)), &opts);
        if init.is_ok() {
            CoUninitialize();
        }
        match result {
            Ok(path) => path,
            Err(e) => {
                tracing::warn!("文件对话框打开失败：{e}");
                None
            }
        }
    })
    .join()
    .unwrap_or(None)
}

/// 筛选器里的扩展名转成对话框的通配符：`["db","sqlite"]` → `*.db;*.sqlite`，`*` → `*.*`。
fn spec(extensions: &[String]) -> String {
    extensions
        .iter()
        .map(|e| if e == "*" { "*.*".to_string() } else { format!("*.{e}") })
        .collect::<Vec<_>>()
        .join(";")
}

unsafe fn show(owner: Option<HWND>, opts: &Options) -> windows::core::Result<Option<String>> {
    unsafe {
        let dialog: IFileOpenDialog = CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER)?;
        let flags = dialog.GetOptions()?;
        dialog.SetOptions(FILEOPENDIALOGOPTIONS((flags.0 & !FOS_FILEMUSTEXIST.0) | FOS_FORCEFILESYSTEM.0))?;

        // 名字和通配符的字符串要活到对话框关闭
        let texts: Vec<(HSTRING, HSTRING)> = opts
            .filters
            .iter()
            .map(|f| (HSTRING::from(f.name.as_str()), HSTRING::from(spec(&f.extensions))))
            .collect();
        let specs: Vec<COMDLG_FILTERSPEC> = texts
            .iter()
            .map(|(name, spec)| COMDLG_FILTERSPEC {
                pszName: PCWSTR(name.as_ptr()),
                pszSpec: PCWSTR(spec.as_ptr()),
            })
            .collect();
        if !specs.is_empty() {
            dialog.SetFileTypes(&specs)?;
            dialog.SetFileTypeIndex(1)?;
        }
        if let Some(ext) = opts.default_extension.as_deref().filter(|e| !e.is_empty()) {
            dialog.SetDefaultExtension(&HSTRING::from(ext))?;
        }
        if let Some(dir) = opts.directory.as_deref().filter(|d| Path::new(d).is_dir()) {
            // 目录无法解析时忽略，由系统决定打开位置
            if let Ok(item) = SHCreateItemFromParsingName::<_, _, IShellItem>(&HSTRING::from(dir), None) {
                dialog.SetFolder(&item)?;
            }
        }
        if let Some(name) = opts.file_name.as_deref().filter(|n| !n.is_empty()) {
            dialog.SetFileName(&HSTRING::from(name))?;
        }

        // 用户取消时 Show 返回 ERROR_CANCELLED
        if dialog.Show(owner).is_err() {
            return Ok(None);
        }
        let raw = dialog.GetResult()?.GetDisplayName(SIGDN_FILESYSPATH)?;
        let path = raw.to_string().ok();
        CoTaskMemFree(Some(raw.0 as *const _));
        Ok(path)
    }
}

#[cfg(test)]
mod tests {
    use super::spec;

    #[test]
    fn spec_joins_extensions_as_wildcards() {
        assert_eq!(spec(&["db".into(), "sqlite".into()]), "*.db;*.sqlite");
        assert_eq!(spec(&["*".into()]), "*.*");
    }
}
