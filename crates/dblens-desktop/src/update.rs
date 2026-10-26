//! 自动更新（设计文档第 10 节）：
//! - 启动后在后台检查更新源（静态的 `latest.json`）；有新版本时提示版本号和更新说明，用户确认后下载、校验签名、安装、重启。
//! - 菜单"帮助"中可以手动检查，也可以关闭启动时的自动检查（Windows、Linux 的菜单由页面绘制，经 `shell` 的命令调用这里）。
//! - 更新源默认是 GitHub Releases；数据目录下的 `update.json` 可以改成内网地址。
//! - Linux 上只有 AppImage 能自动安装；deb、rpm 只提示新版本，由用户自己下载。
//!
//! 更新包用 updater 自带的密钥对签名，公钥在 `tauri.conf.json` 中，私钥只在发布时使用（见 `docs/notes/m0-4d-release.md`）。

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::menu::{CheckMenuItem, CheckMenuItemBuilder, MenuItemBuilder, Submenu, SubmenuBuilder};
use tauri::{App, AppHandle, Manager, Wry};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
use tauri_plugin_updater::{Update, UpdaterExt};

use crate::json_file;

/// 发布页，Linux 的 deb、rpm 包从这里手动下载新版本。
pub const RELEASES_PAGE: &str = "https://github.com/byteswalk/dblens/releases";
const MENU_CHECK: &str = "check_update";
const MENU_AUTO: &str = "auto_check_update";
const MENU_ABOUT: &str = "about";
/// 启动后等这么久再自动检查，不和启动抢资源。
const STARTUP_DELAY: Duration = Duration::from_secs(5);
const TIMEOUT: Duration = Duration::from_secs(30);

/// 数据目录下的 `update.json`。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct UpdateSettings {
    /// 启动后在后台检查更新。
    pub auto_check: bool,
    /// 更新源（`latest.json` 的地址）。为空时用 `tauri.conf.json` 中的默认地址；内网部署时改成内网地址。
    pub endpoint: Option<String>,
}

impl Default for UpdateSettings {
    fn default() -> Self {
        Self { auto_check: true, endpoint: None }
    }
}

/// 当前的安装方式能否自动安装更新：Windows（NSIS）和 macOS 可以；Linux 上只有 AppImage 可以。
pub fn can_self_update(os: &str, appimage: bool) -> bool {
    os != "linux" || appimage
}

/// 发现新版本时的提示文字。
pub fn offer_text(version: &str, current: &str, notes: Option<&str>, self_update: bool) -> String {
    let mut text = format!("发现新版本 {version}（当前 {current}）。");
    if let Some(notes) = notes.map(str::trim).filter(|n| !n.is_empty()) {
        text.push_str("\n\n更新说明：\n");
        text.push_str(notes);
    }
    if self_update {
        text.push_str("\n\n现在下载并安装吗？安装完成后程序会自动重启。");
    } else {
        text.push_str(&format!("\n\n请到 {RELEASES_PAGE} 下载新版本。"));
    }
    text
}

/// 把更新插件的英文错误转成给用户看的说明，原文附在后面便于排查。
pub fn error_text(err: &str) -> String {
    let lower = err.to_lowercase();
    let hint = if lower.contains("signature") {
        "安装包签名校验失败，已取消安装。安装包可能被篡改，或更新源提供的签名不对"
    } else if lower.contains("secure protocol") {
        "更新源必须使用 https 地址"
    } else if lower.contains("could not fetch")
        || lower.contains("error sending request")
        || lower.contains("timed out")
    {
        "无法连接更新源，请检查网络或 update.json 中的地址"
    } else {
        return err.to_string();
    };
    format!("{hint}（{err}）")
}

struct UpdateState {
    path: PathBuf,
    settings: Mutex<UpdateSettings>,
    auto_item: CheckMenuItem<Wry>,
}

/// 弹出消息框，等用户关闭后返回是否点了确认按钮。
async fn dialog(
    app: &AppHandle,
    kind: MessageDialogKind,
    text: String,
    buttons: MessageDialogButtons,
) -> bool {
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog().message(text).title("DBLens 数洞").kind(kind).buttons(buttons).show(move |ok| {
        let _ = tx.send(ok);
    });
    rx.await.unwrap_or(false)
}

async fn info(app: &AppHandle, text: String) {
    dialog(app, MessageDialogKind::Info, text, MessageDialogButtons::Ok).await;
}

async fn find_update(app: &AppHandle) -> Result<Option<Update>, String> {
    let endpoint = app.state::<UpdateState>().settings.lock().unwrap().endpoint.clone();
    let mut builder = app.updater_builder().timeout(TIMEOUT);
    if let Some(e) = endpoint.filter(|e| !e.trim().is_empty()) {
        let url = e.trim().parse().map_err(|err| format!("update.json 中的更新源地址无效（{e}）：{err}"))?;
        builder = builder.endpoints(vec![url]).map_err(|err| err.to_string())?;
    }
    builder.build().map_err(|e| e.to_string())?.check().await.map_err(|e| e.to_string())
}

async fn install(app: &AppHandle, update: Update) -> Result<(), String> {
    let mut downloaded = 0usize;
    let mut last_logged = 0usize;
    update
        .download_and_install(
            |chunk, total| {
                downloaded += chunk;
                if downloaded - last_logged >= 10 * 1024 * 1024 {
                    last_logged = downloaded;
                    tracing::info!("已下载 {downloaded} / {total:?} 字节");
                }
            },
            || tracing::info!("下载完成，开始安装"),
        )
        .await
        .map_err(|e| e.to_string())?;
    tracing::info!("更新已安装，重启");
    app.restart()
}

/// 检查更新。`manual` 为 true 表示用户从菜单发起：没有新版本或出错时也要告诉用户；自动检查出错只记日志。
pub async fn check(app: AppHandle, manual: bool) {
    let update = match find_update(&app).await {
        Ok(u) => u,
        Err(e) => {
            tracing::warn!("检查更新失败：{e}");
            if manual {
                dialog(
                    &app,
                    MessageDialogKind::Error,
                    format!("检查更新失败：{}", error_text(&e)),
                    MessageDialogButtons::Ok,
                )
                .await;
            }
            return;
        }
    };
    let Some(update) = update else {
        tracing::info!("已是最新版本");
        if manual {
            info(&app, format!("已是最新版本（{}）。", app.package_info().version)).await;
        }
        return;
    };
    tracing::info!("发现新版本 {}（当前 {}）", update.version, update.current_version);
    let self_update = can_self_update(std::env::consts::OS, std::env::var_os("APPIMAGE").is_some());
    let text = offer_text(&update.version, &update.current_version, update.body.as_deref(), self_update);
    if !self_update {
        info(&app, text).await;
        return;
    }
    let buttons = MessageDialogButtons::OkCancelCustom("下载并安装".into(), "以后再说".into());
    if !dialog(&app, MessageDialogKind::Info, text, buttons).await {
        return;
    }
    if let Err(e) = install(&app, update).await {
        tracing::error!("安装更新失败：{e}");
        let text = format!("安装更新失败：{}", error_text(&e));
        dialog(&app, MessageDialogKind::Error, text, MessageDialogButtons::Ok).await;
    }
}

/// 读取更新设置，生成"帮助"菜单；开启了自动检查时，启动后在后台检查一次。
pub fn init(app: &App, path: PathBuf) -> tauri::Result<Submenu<Wry>> {
    let settings: UpdateSettings = json_file::load(&path);
    let auto_item = CheckMenuItemBuilder::with_id(MENU_AUTO, "启动时自动检查更新")
        .checked(settings.auto_check)
        .build(app)?;
    let menu = SubmenuBuilder::new(app, "帮助")
        .item(&MenuItemBuilder::with_id(MENU_CHECK, "检查更新…").build(app)?)
        .item(&auto_item)
        .separator()
        .item(&MenuItemBuilder::with_id(MENU_ABOUT, "关于 DBLens").build(app)?)
        .build()?;
    if settings.auto_check {
        let handle = app.handle().clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(STARTUP_DELAY).await;
            check(handle, false).await;
        });
    }
    app.manage(UpdateState { path, settings: Mutex::new(settings), auto_item });
    Ok(menu)
}

pub fn auto_check(app: &AppHandle) -> bool {
    app.state::<UpdateState>().settings.lock().unwrap().auto_check
}

/// 开关启动时的自动检查：保存到 `update.json`，并同步 macOS 系统菜单的勾选状态。
pub fn set_auto_check(app: &AppHandle, on: bool) {
    let state = app.state::<UpdateState>();
    let mut s = state.settings.lock().unwrap();
    s.auto_check = on;
    if let Err(e) = state.auto_item.set_checked(on) {
        tracing::warn!("同步菜单勾选状态失败：{e}");
    }
    if let Err(e) = json_file::save(&state.path, &*s) {
        tracing::warn!("无法保存更新设置到 {}：{e}", state.path.display());
    }
}

/// "关于 DBLens"对话框。
pub fn about(app: &AppHandle) {
    let text = format!(
        "DBLens 数洞 {}

{RELEASES_PAGE}",
        app.package_info().version
    );
    let app = app.clone();
    tauri::async_runtime::spawn(async move { info(&app, text).await });
}

/// 处理"帮助"菜单（macOS 系统菜单）的事件；不是这个菜单的事件返回 false。
pub fn on_menu(app: &AppHandle, id: &str) -> bool {
    match id {
        MENU_CHECK => {
            tauri::async_runtime::spawn(check(app.clone(), true));
        }
        MENU_AUTO => {
            // 点击时菜单项已自动切换了勾选状态，以它为准
            let on = app.state::<UpdateState>().auto_item.is_checked().unwrap_or(true);
            set_auto_check(app, on);
        }
        MENU_ABOUT => about(app),
        _ => return false,
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_default_to_auto_check_with_builtin_endpoint() {
        assert_eq!(UpdateSettings::default(), UpdateSettings { auto_check: true, endpoint: None });
        let s: UpdateSettings =
            serde_json::from_str(r#"{"endpoint": "http://intranet/latest.json"}"#).unwrap();
        assert!(s.auto_check, "缺少的字段取默认值");
        assert_eq!(s.endpoint.as_deref(), Some("http://intranet/latest.json"));
    }

    #[test]
    fn only_appimage_can_self_update_on_linux() {
        assert!(can_self_update("windows", false));
        assert!(can_self_update("macos", false));
        assert!(can_self_update("linux", true));
        assert!(!can_self_update("linux", false));
    }

    #[test]
    fn errors_are_explained_in_chinese() {
        assert_eq!(
            error_text("The signature verification failed"),
            "安装包签名校验失败，已取消安装。安装包可能被篡改，或更新源提供的签名不对（The signature verification failed）"
        );
        let insecure = error_text("The configured updater endpoint must use a secure protocol like `https`.");
        assert!(insecure.starts_with("更新源必须使用 https 地址"), "{insecure}");
        assert_eq!(error_text("something else"), "something else");
    }

    #[test]
    fn offer_text_variants() {
        assert_eq!(
            offer_text("0.2.0", "0.1.0", Some(" 修复若干问题 \n"), true),
            "发现新版本 0.2.0（当前 0.1.0）。\n\n更新说明：\n修复若干问题\n\n现在下载并安装吗？安装完成后程序会自动重启。"
        );
        assert_eq!(
            offer_text("0.2.0", "0.1.0", Some("  "), false),
            format!("发现新版本 0.2.0（当前 0.1.0）。\n\n请到 {RELEASES_PAGE} 下载新版本。")
        );
    }
}
