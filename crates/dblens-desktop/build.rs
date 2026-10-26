fn main() {
    // 为页面可调用的命令生成 allow-* 权限，供 capabilities/main.json 引用
    let manifest =
        tauri_build::AppManifest::new().commands(&["shell_info", "window_action", "set_auto_check_update"]);
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(manifest)).expect("tauri-build 失败");
}
