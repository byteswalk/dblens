//! 桌面模式入口（设计文档 4.1 节）：进程内启动接口服务，只绑定 127.0.0.1 的随机端口，
//! 每次启动生成新的访问令牌，经 Tauri 注入页面。前端只通过 HTTP 和 WebSocket 调用业务；
//! Tauri 只负责窗口、菜单等原生能力。

// 发布版不弹出控制台窗口
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod geometry;
mod json_file;
mod logging;
mod shell;
mod update;
mod window;
mod window_state;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use dblens_api::{ApiConfig, new_token, router, serve, with_web};
use dblens_core::msg;
use dblens_service::{Service, default_data_dir};
use tauri::Manager;

/// 前端页面目录，按顺序查找：
/// 1. 环境变量 `DBLENS_WEB_DIR`；
/// 2. 安装包中的资源目录 `web/`（打包在 M0-4d 中配置）；
/// 3. 调试构建时，源码树中的 `web/dist`。
fn web_dir(resource_dir: Option<PathBuf>) -> Option<PathBuf> {
    let dev = cfg!(debug_assertions).then(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../web/dist"));
    std::env::var_os("DBLENS_WEB_DIR")
        .map(PathBuf::from)
        .into_iter()
        .chain(resource_dir.map(|d| d.join("web")))
        .chain(dev)
        .find(|d| d.join("index.html").is_file())
}

fn run(data_dir: PathBuf) -> Result<(), String> {
    let service = Arc::new(Service::open(&data_dir).map_err(|e| e.to_string())?);
    let token = new_token();
    // 先在当前线程绑定端口，窗口创建时地址已经确定
    let listener = std::net::TcpListener::bind("127.0.0.1:0")
        .map_err(|e| msg!("desktop.bind_failed", error = e).current())?;
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    let addr = listener.local_addr().map_err(|e| e.to_string())?;
    let base = format!("http://{addr}/");
    tracing::info!("接口服务地址 {base}，数据目录 {}", data_dir.display());

    tauri::Builder::default()
        .plugin(tauri_plugin_global_shortcut::Builder::new().with_handler(window::on_shortcut).build())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .invoke_handler(tauri::generate_handler![
            shell::shell_info,
            shell::window_action,
            shell::set_auto_check_update,
            shell::save_dialog,
            shell::reveal_in_folder
        ])
        .setup(move |app| {
            let mut api = router(
                service,
                ApiConfig { token: token.clone(), allowed_origins: Vec::new(), local_files: true },
            );
            match web_dir(app.path().resource_dir().ok()) {
                Some(dir) => {
                    tracing::info!("前端页面目录 {}", dir.display());
                    api = with_web(api, dir);
                }
                None => tracing::error!("没有找到前端页面（web/dist），请先在 web/ 下执行 pnpm build"),
            }
            tauri::async_runtime::spawn(async move {
                let result = match tokio::net::TcpListener::from_std(listener) {
                    Ok(l) => serve(l, api).await,
                    Err(e) => Err(e),
                };
                if let Err(e) = result {
                    tracing::error!("接口服务异常退出：{e}");
                }
            });
            let help = update::init(app, data_dir.join("update.json"))?;
            window::create_main(app, &base, &token, data_dir.join("window.json"), &help)?;
            Ok(())
        })
        .run(tauri::generate_context!())
        .map_err(|e| e.to_string())
}

/// 数据目录：环境变量 `DBLENS_DATA_DIR`（开发和测试时用，避免碰到真实数据）优先，否则用默认的用户数据目录。
fn data_dir() -> dblens_core::Result<PathBuf> {
    match std::env::var_os("DBLENS_DATA_DIR") {
        Some(d) => Ok(PathBuf::from(d)),
        None => default_data_dir(),
    }
}

fn main() {
    let data_dir = match data_dir() {
        Ok(d) => d,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    };
    let _guard = match logging::init(&data_dir.join("logs")) {
        Ok(g) => Some(g),
        Err(e) => {
            eprintln!("日志初始化失败：{e}");
            None
        }
    };
    tracing::info!("DBLens {} 启动", env!("CARGO_PKG_VERSION"));
    if let Err(e) = run(data_dir) {
        tracing::error!("启动失败：{e}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn web_dir_prefers_resource_dir_and_needs_index_html() {
        let tmp = tempfile::tempdir().unwrap();
        let res = tmp.path().join("res");
        std::fs::create_dir_all(res.join("web")).unwrap();
        // 资源目录中还没有 index.html：调试构建回退到源码树的 web/dist（存在时），否则找不到
        let found = web_dir(Some(res.clone()));
        assert_ne!(found, Some(res.join("web")));
        std::fs::write(res.join("web").join("index.html"), "x").unwrap();
        assert_eq!(web_dir(Some(res.clone())), Some(res.join("web")));
    }
}
