//! 主窗口（设计文档 7.2 节；M1 设计文档 1.2、2.2 节）：
//! - 首次启动 1440×900 居中，屏幕小时取工作区的 90%，最小 1024×640；
//! - 窗口置顶（Ctrl+Shift+T）、恢复默认窗口位置和大小；
//! - 退出时保存位置、大小、是否最大化和置顶状态；保存的位置已不在任何显示器上时，改用主屏幕的默认位置。
//!
//! 标题栏由页面自绘（M1）：Windows、Linux 去掉系统标题栏，菜单也由页面绘制，经 [`crate::shell`] 的命令调用这里；
//! macOS 保留系统的红黄绿按钮（标题栏叠加在页面上），菜单仍用屏幕顶部的系统菜单栏。
//!
//! 页面由进程内的接口服务提供（与 `/api` 同源）。访问令牌经初始化脚本注入页面；
//! 窗口只允许停留在这个地址下，防止令牌被注入到其他网站的页面中。
//!
//! 快捷键：Windows 上键盘焦点在页面（WebView2）里时，按键不经过窗口的消息循环，菜单快捷键不会触发（实测）。
//! 所以置顶快捷键用系统热键实现，只在窗口获得焦点时注册、失去焦点时注销，不影响其他程序。

use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use tauri::menu::{
    CheckMenuItem, CheckMenuItemBuilder, MenuBuilder, MenuItemBuilder, Submenu, SubmenuBuilder,
};
use tauri::{
    App, AppHandle, Manager, PhysicalPosition, PhysicalSize, WebviewUrl, WebviewWindow, WebviewWindowBuilder,
    WindowEvent, Wry,
};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutEvent, ShortcutState};

use crate::geometry::{self, MIN_SIZE, Rect};
use crate::window_state::{self, WindowState};
use crate::{shell, update};

pub const MAIN: &str = "main";
const MENU_ON_TOP: &str = "always_on_top";
const MENU_RESET: &str = "reset_window";
const MENU_QUIT: &str = "quit";
/// "窗口置顶"的快捷键。没有用 Ctrl+Alt+T：它在 Linux 桌面上是打开终端的系统快捷键。
pub const ON_TOP_SHORTCUT: &str = "CmdOrCtrl+Shift+T";

/// 移动、缩放后等这么久再记录位置：最大化时移动和缩放事件先于"已最大化"标志到达，立即记录会把最大化后的大小当成普通大小。
const SETTLE: Duration = Duration::from_millis(300);

/// 运行期间的窗口状态，退出时写回文件。
struct Tracked {
    path: PathBuf,
    state: Mutex<WindowState>,
    /// macOS 系统菜单里的"窗口置顶"勾选项；其他平台没有系统菜单。
    on_top_item: Option<CheckMenuItem<Wry>>,
    /// 每次移动、缩放加一；延迟记录时编号已变，说明之后又有新事件，这次就不记了。
    generation: AtomicU64,
}

/// 页面脚本运行前执行的初始化脚本：注入访问令牌。
pub fn init_script(token: &str) -> String {
    format!("window.__DBLENS__ = Object.freeze({{ token: {} }});", serde_json::Value::from(token))
}

/// 窗口只能停留在接口服务的地址下（`base` 形如 `http://127.0.0.1:端口/`）。
pub fn allowed_navigation(base: &str, url: &str) -> bool {
    url.starts_with(base)
}

fn on_top_shortcut() -> Shortcut {
    ON_TOP_SHORTCUT.parse().expect("快捷键格式")
}

fn to_rect(p: PhysicalPosition<i32>, s: PhysicalSize<u32>) -> Rect {
    Rect { x: p.x, y: p.y, width: s.width, height: s.height }
}

fn apply_rect(win: &WebviewWindow, r: Rect) -> tauri::Result<()> {
    win.set_size(PhysicalSize::new(r.width, r.height))?;
    win.set_position(PhysicalPosition::new(r.x, r.y))
}

/// 主屏幕（取不到时用窗口当前所在的屏幕）上的默认位置和大小。
fn default_rect(win: &WebviewWindow) -> tauri::Result<Option<Rect>> {
    let monitor = match win.primary_monitor()? {
        Some(m) => Some(m),
        None => win.current_monitor()?,
    };
    Ok(monitor.map(|m| {
        let a = m.work_area();
        geometry::default_rect(to_rect(a.position, a.size), m.scale_factor())
    }))
}

pub fn reset(win: &WebviewWindow) -> tauri::Result<()> {
    win.unmaximize()?;
    if let Some(r) = default_rect(win)? {
        apply_rect(win, r)?;
    }
    Ok(())
}

/// 记下未最大化、未最小化时的位置和大小，作为下次启动的位置。
fn remember(win: &WebviewWindow, tracked: &Tracked) -> tauri::Result<()> {
    if win.is_maximized()? || win.is_minimized()? {
        return Ok(());
    }
    let r = to_rect(win.outer_position()?, win.inner_size()?);
    tracked.state.lock().unwrap().rect = Some(r);
    Ok(())
}

/// 移动、缩放后延迟记录位置，连续的事件只记最后一次。
fn remember_later(win: &WebviewWindow) {
    let generation = win.state::<Tracked>().generation.fetch_add(1, Ordering::SeqCst) + 1;
    let win = win.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(SETTLE).await;
        let tracked = win.state::<Tracked>();
        if tracked.generation.load(Ordering::SeqCst) == generation {
            let _ = remember(&win, &tracked);
        }
    });
}

fn save(win: &WebviewWindow, tracked: &Tracked) {
    // 退出时状态已稳定，补记一次，免得最后一次移动还没来得及记录
    let _ = remember(win, tracked);
    let mut s = *tracked.state.lock().unwrap();
    s.maximized = win.is_maximized().unwrap_or(false);
    if let Err(e) = window_state::save(&tracked.path, &s) {
        tracing::warn!("无法保存窗口状态到 {}：{e}", tracked.path.display());
    }
}

pub fn always_on_top(app: &AppHandle) -> bool {
    app.state::<Tracked>().state.lock().unwrap().always_on_top
}

/// 设置置顶，同步系统菜单的勾选状态，并通知页面。
fn set_always_on_top(app: &AppHandle, on: bool) -> tauri::Result<()> {
    if let Some(win) = app.get_webview_window(MAIN) {
        win.set_always_on_top(on)?;
    }
    let tracked = app.state::<Tracked>();
    tracked.state.lock().unwrap().always_on_top = on;
    if let Some(item) = &tracked.on_top_item {
        item.set_checked(on)?;
    }
    shell::notify(app);
    Ok(())
}

pub fn toggle_always_on_top(app: &AppHandle) -> tauri::Result<()> {
    set_always_on_top(app, !always_on_top(app))
}

/// 保存窗口状态后退出。
pub fn quit(app: &AppHandle) {
    if let Some(win) = app.get_webview_window(MAIN) {
        save(&win, &app.state::<Tracked>());
    }
    app.exit(0);
}

/// 快捷键：切换置顶。
pub fn on_shortcut(app: &AppHandle, shortcut: &Shortcut, event: ShortcutEvent) {
    if event.state != ShortcutState::Pressed || *shortcut != on_top_shortcut() {
        return;
    }
    if let Err(e) = toggle_always_on_top(app) {
        tracing::warn!("切换窗口置顶失败：{e}");
    }
}

fn on_menu(app: &AppHandle, id: &str) -> tauri::Result<()> {
    if update::on_menu(app, id) {
        return Ok(());
    }
    let Some(win) = app.get_webview_window(MAIN) else { return Ok(()) };
    match id {
        // 点击时系统菜单已自动切换了勾选状态，以它为准
        MENU_ON_TOP => {
            let on = app.state::<Tracked>().on_top_item.as_ref().map_or(Ok(false), |i| i.is_checked())?;
            set_always_on_top(app, on)?
        }
        MENU_RESET => reset(&win)?,
        MENU_QUIT => quit(app),
        _ => {}
    }
    Ok(())
}

/// 窗口获得焦点时注册快捷键，失去焦点时注销。注册失败（被其他程序占用）只记日志，菜单仍可用。
fn focus_changed(app: &AppHandle, focused: bool) {
    let gs = app.global_shortcut();
    let shortcut = on_top_shortcut();
    let result = if focused {
        if gs.is_registered(shortcut) { Ok(()) } else { gs.register(shortcut) }
    } else if gs.is_registered(shortcut) {
        gs.unregister(shortcut)
    } else {
        Ok(())
    };
    if let Err(e) = result {
        tracing::warn!("快捷键 {ON_TOP_SHORTCUT} {}失败：{e}", if focused { "注册" } else { "注销" });
    }
}

/// macOS 的系统菜单："窗口"和"帮助"（`help`）。其他平台的菜单由页面绘制。
fn mac_menu(
    app: &App,
    saved: &WindowState,
    help: &Submenu<Wry>,
) -> tauri::Result<(tauri::menu::Menu<Wry>, CheckMenuItem<Wry>)> {
    let on_top = CheckMenuItemBuilder::with_id(MENU_ON_TOP, "窗口置顶")
        .checked(saved.always_on_top)
        .accelerator(ON_TOP_SHORTCUT)
        .build(app)?;
    let window_menu = SubmenuBuilder::new(app, "窗口")
        .item(&on_top)
        .item(&MenuItemBuilder::with_id(MENU_RESET, "恢复默认窗口位置和大小").build(app)?)
        .separator()
        .item(&MenuItemBuilder::with_id(MENU_QUIT, "退出").build(app)?)
        .build()?;
    Ok((MenuBuilder::new(app).item(&window_menu).item(help).build()?, on_top))
}

/// 创建主窗口：恢复保存的状态，注入令牌，按平台设置标题栏和菜单，挂上窗口事件。
/// `help` 是"帮助"菜单，只在 macOS 上使用。
pub fn create_main(
    app: &App,
    base: &str,
    token: &str,
    state_path: PathBuf,
    help: &Submenu<Wry>,
) -> tauri::Result<()> {
    let saved = window_state::load(&state_path);
    let allowed = base.to_string();
    let mut builder =
        WebviewWindowBuilder::new(app, MAIN, WebviewUrl::External(base.parse().expect("接口地址")))
            .title("DBLens 数洞")
            .min_inner_size(MIN_SIZE.0, MIN_SIZE.1)
            .visible(false)
            .initialization_script(init_script(token))
            .on_navigation(move |url| {
                let ok = allowed_navigation(&allowed, url.as_str());
                if !ok {
                    tracing::warn!("已阻止窗口跳转到 {url}");
                }
                ok
            });
    let mut on_top_item = None;
    if cfg!(target_os = "macos") {
        let (menu, item) = mac_menu(app, &saved, help)?;
        on_top_item = Some(item);
        builder = builder.menu(menu);
        // 标题栏叠加在页面上：保留红黄绿按钮，其余由页面绘制（这两个方法只在 macOS 上存在）
        #[cfg(target_os = "macos")]
        {
            builder = builder.title_bar_style(tauri::TitleBarStyle::Overlay).hidden_title(true);
        }
    } else {
        builder = builder.decorations(false);
    }
    let win = builder.build()?;

    let work_areas: Vec<Rect> = win
        .available_monitors()?
        .iter()
        .map(|m| to_rect(m.work_area().position, m.work_area().size))
        .collect();
    match saved.rect {
        Some(r) if geometry::is_reachable(r, &work_areas) => apply_rect(&win, r)?,
        Some(r) => {
            tracing::info!("保存的窗口位置 {r:?} 已不在任何显示器上，移回主屏幕");
            reset(&win)?;
        }
        None => reset(&win)?,
    }
    if saved.maximized {
        win.maximize()?;
    }
    win.set_always_on_top(saved.always_on_top)?;
    win.show()?;

    let tracked =
        Tracked { path: state_path, state: Mutex::new(saved), on_top_item, generation: AtomicU64::new(0) };
    remember(&win, &tracked)?;
    app.manage(tracked);
    app.on_menu_event(|app, event| {
        if let Err(e) = on_menu(app, event.id().as_ref()) {
            tracing::warn!("菜单操作失败：{e}");
        }
    });
    let w = win.clone();
    win.on_window_event(move |event| {
        let tracked = w.state::<Tracked>();
        match event {
            WindowEvent::Moved(_) => remember_later(&w),
            WindowEvent::Resized(_) => {
                remember_later(&w);
                // 最大化、还原都会触发缩放，页面据此切换"最大化 / 还原"按钮
                shell::notify(w.app_handle());
            }
            WindowEvent::Focused(focused) => focus_changed(w.app_handle(), *focused),
            WindowEvent::CloseRequested { .. } => save(&w, &tracked),
            _ => {}
        }
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_is_injected_as_js_string() {
        assert_eq!(init_script("ab\"c"), r#"window.__DBLENS__ = Object.freeze({ token: "ab\"c" });"#);
    }

    #[test]
    fn navigation_stays_on_api_origin() {
        let base = "http://127.0.0.1:50000/";
        assert!(allowed_navigation(base, "http://127.0.0.1:50000/"));
        assert!(allowed_navigation(base, "http://127.0.0.1:50000/query?x=1"));
        assert!(!allowed_navigation(base, "http://127.0.0.1:500001/"));
        assert!(!allowed_navigation(base, "https://evil.example/"));
        assert!(!allowed_navigation(base, "http://127.0.0.1:50000.evil.example/"));
    }

    #[test]
    fn on_top_shortcut_parses() {
        let s = on_top_shortcut();
        assert!(s.mods.contains(tauri_plugin_global_shortcut::Modifiers::SHIFT));
        assert_eq!(s.key, tauri_plugin_global_shortcut::Code::KeyT);
    }
}
