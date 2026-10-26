//! 页面可以调用的原生能力（M1 设计文档 2.2 节）。
//!
//! 业务仍然只走 HTTP 和 WebSocket；自绘标题栏需要的窗口操作，以及原本在原生菜单里的几项（置顶、恢复默认位置、
//! 检查更新、关于、退出），通过这里的三个命令提供。`capabilities/main.json` 只放行这三个命令和事件监听，
//! 且只对 `http://127.0.0.1:*` 上的本地页面生效。
//!
//! 窗口状态变化（最大化、置顶、自动检查更新）后，通过事件 [`EVENT`] 把新的 [`ShellInfo`] 推给页面。

use dblens_core::msg;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};

use crate::{update, window};

/// 状态变化事件名，载荷为 [`ShellInfo`]。
pub const EVENT: &str = "dblens://shell";

/// 标题栏需要的状态。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ShellInfo {
    /// `windows` / `macos` / `linux`。macOS 上页面要给系统的红黄绿按钮留位置。
    pub platform: &'static str,
    pub version: String,
    pub maximized: bool,
    pub always_on_top: bool,
    pub auto_check_update: bool,
}

/// 页面发起的窗口操作。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WindowAction {
    Minimize,
    ToggleMaximize,
    Close,
    /// 鼠标在标题栏空白处按下：开始拖动窗口。
    StartDragging,
    ToggleAlwaysOnTop,
    /// 恢复默认窗口位置和大小。
    ResetPosition,
    CheckUpdate,
    About,
    Quit,
}

pub fn info(app: &AppHandle) -> ShellInfo {
    let maximized = app.get_webview_window(window::MAIN).and_then(|w| w.is_maximized().ok()).unwrap_or(false);
    ShellInfo {
        platform: std::env::consts::OS,
        version: app.package_info().version.to_string(),
        maximized,
        always_on_top: window::always_on_top(app),
        auto_check_update: update::auto_check(app),
    }
}

/// 把最新状态推给页面。
pub fn notify(app: &AppHandle) {
    if let Err(e) = app.emit(EVENT, info(app)) {
        tracing::warn!("推送窗口状态失败：{e}");
    }
}

#[tauri::command]
pub fn shell_info(app: AppHandle) -> ShellInfo {
    info(&app)
}

#[tauri::command]
pub fn window_action(app: AppHandle, action: WindowAction) -> Result<(), String> {
    let win = app.get_webview_window(window::MAIN).ok_or_else(|| msg!("desktop.no_main_window").current())?;
    let result = match action {
        WindowAction::Minimize => win.minimize(),
        WindowAction::ToggleMaximize => {
            if win.is_maximized().map_err(|e| e.to_string())? {
                win.unmaximize()
            } else {
                win.maximize()
            }
        }
        WindowAction::Close => win.close(),
        WindowAction::StartDragging => win.start_dragging(),
        WindowAction::ToggleAlwaysOnTop => window::toggle_always_on_top(&app),
        WindowAction::ResetPosition => window::reset(&win),
        WindowAction::CheckUpdate => {
            tauri::async_runtime::spawn(update::check(app.clone(), true));
            Ok(())
        }
        WindowAction::About => {
            update::about(&app);
            Ok(())
        }
        WindowAction::Quit => {
            window::quit(&app);
            Ok(())
        }
    };
    result.map_err(|e| e.to_string())
}

#[tauri::command]
pub fn set_auto_check_update(app: AppHandle, on: bool) {
    update::set_auto_check(&app, on);
    notify(&app);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn actions_use_snake_case() {
        let parse = |s: &str| serde_json::from_value::<WindowAction>(serde_json::Value::from(s));
        assert_eq!(parse("toggle_maximize").unwrap(), WindowAction::ToggleMaximize);
        assert_eq!(parse("start_dragging").unwrap(), WindowAction::StartDragging);
        assert_eq!(parse("toggle_always_on_top").unwrap(), WindowAction::ToggleAlwaysOnTop);
        assert!(parse("ToggleMaximize").is_err());
        assert!(parse("format_disk").is_err(), "未知操作直接拒绝");
    }

    #[test]
    fn info_serializes_for_the_page() {
        let i = ShellInfo {
            platform: "windows",
            version: "0.1.0".into(),
            maximized: false,
            always_on_top: true,
            auto_check_update: true,
        };
        assert_eq!(
            serde_json::to_value(&i).unwrap(),
            serde_json::json!({"platform":"windows","version":"0.1.0","maximized":false,"always_on_top":true,"auto_check_update":true})
        );
    }
}
