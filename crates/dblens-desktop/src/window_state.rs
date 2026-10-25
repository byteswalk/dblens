//! 窗口状态的保存与读取：数据目录下的 `window.json`。

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::geometry::Rect;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct WindowState {
    /// 未最大化时的位置和大小；`None` 表示用默认位置。
    pub rect: Option<Rect>,
    pub maximized: bool,
    pub always_on_top: bool,
}

/// 读取保存的状态。文件不存在或内容损坏时用默认状态（损坏时记一条警告），不影响启动。
/// 允许开头有 UTF-8 BOM（用记事本等工具改过的文件）。
pub fn load(path: &Path) -> WindowState {
    match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice(bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(&bytes))
            .unwrap_or_else(|e| {
                tracing::warn!("窗口状态文件 {} 无法解析，改用默认状态：{e}", path.display());
                WindowState::default()
            }),
        Err(_) => WindowState::default(),
    }
}

/// 先写临时文件再改名，避免写到一半退出时留下损坏的文件。
pub fn save(path: &Path, state: &WindowState) -> std::io::Result<()> {
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(state)?)?;
    std::fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_fallbacks() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("window.json");
        assert_eq!(load(&path), WindowState::default(), "文件不存在");

        let s = WindowState {
            rect: Some(Rect { x: -1200, y: 50, width: 1440, height: 900 }),
            maximized: true,
            always_on_top: true,
        };
        save(&path, &s).unwrap();
        assert_eq!(load(&path), s);
        save(&path, &WindowState::default()).unwrap();
        assert_eq!(load(&path), WindowState::default(), "覆盖保存");

        std::fs::write(&path, "\u{feff}{\"maximized\": true}").unwrap();
        assert_eq!(load(&path), WindowState { maximized: true, ..Default::default() }, "带 BOM");
        std::fs::write(&path, "{ broken").unwrap();
        assert_eq!(load(&path), WindowState::default(), "内容损坏");
        std::fs::write(&path, r#"{"always_on_top": true}"#).unwrap();
        assert_eq!(
            load(&path),
            WindowState { always_on_top: true, ..Default::default() },
            "缺少的字段取默认值"
        );
    }
}
