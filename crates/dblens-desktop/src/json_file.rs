//! 数据目录下的小型 JSON 设置文件（窗口状态、更新设置等）的读写。

use std::path::Path;

use serde::Serialize;
use serde::de::DeserializeOwned;

/// 读取设置。文件不存在或内容损坏时用默认值（损坏时记一条警告），不影响启动。
/// 允许开头有 UTF-8 BOM（用记事本等工具改过的文件）。
pub fn load<T: DeserializeOwned + Default>(path: &Path) -> T {
    match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice(bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(&bytes))
            .unwrap_or_else(|e| {
                tracing::warn!("设置文件 {} 无法解析，改用默认值：{e}", path.display());
                T::default()
            }),
        Err(_) => T::default(),
    }
}

/// 先写临时文件再改名，避免写到一半退出时留下损坏的文件。
pub fn save<T: Serialize>(path: &Path, value: &T) -> std::io::Result<()> {
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(value)?)?;
    std::fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use serde::Deserialize;

    use super::*;

    #[derive(Debug, Default, PartialEq, Serialize, Deserialize)]
    #[serde(default)]
    struct Demo {
        a: bool,
        b: Option<String>,
    }

    #[test]
    fn round_trip_and_fallbacks() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("demo.json");
        assert_eq!(load::<Demo>(&path), Demo::default(), "文件不存在");

        let v = Demo { a: true, b: Some("x".into()) };
        save(&path, &v).unwrap();
        assert_eq!(load::<Demo>(&path), v);
        save(&path, &Demo::default()).unwrap();
        assert_eq!(load::<Demo>(&path), Demo::default(), "覆盖保存");

        std::fs::write(&path, "\u{feff}{\"a\": true}").unwrap();
        assert_eq!(load::<Demo>(&path), Demo { a: true, b: None }, "带 BOM、缺少的字段取默认值");
        std::fs::write(&path, "{ broken").unwrap();
        assert_eq!(load::<Demo>(&path), Demo::default(), "内容损坏");
    }
}
