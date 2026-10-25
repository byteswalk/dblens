//! 新版本提示（设计文档第 10 节）：浏览器模式的服务只提示有新版本，不会自动替换自己。
//! 读取的是桌面版同一个 `latest.json`，只用其中的 `version` 和 `notes`。

use std::time::Duration;

use serde::Deserialize;

/// 默认更新源，与桌面版 `tauri.conf.json` 中的相同。
pub const DEFAULT_URL: &str = "https://github.com/byteswalk/dblens/releases/latest/download/latest.json";
/// 发布页。
pub const RELEASES_PAGE: &str = "https://github.com/byteswalk/dblens/releases";

#[derive(Debug, Deserialize)]
struct Latest {
    version: String,
    #[serde(default)]
    notes: Option<String>,
}

/// 比 `current` 新的版本。版本号按语义化版本比较，允许带前缀 `v`。
fn newer(current: &str, latest: &str) -> Result<bool, String> {
    let parse = |v: &str| {
        semver::Version::parse(v.trim().trim_start_matches('v')).map_err(|e| format!("无效的版本号 {v}：{e}"))
    };
    Ok(parse(latest)? > parse(current)?)
}

/// 新版本的提示文字；`latest.json` 中的版本不比当前新时返回 `None`。
pub fn notice(current: &str, latest_json: &[u8]) -> Result<Option<String>, String> {
    let latest: Latest =
        serde_json::from_slice(latest_json).map_err(|e| format!("latest.json 格式不对：{e}"))?;
    if !newer(current, &latest.version)? {
        return Ok(None);
    }
    let mut text = format!("发现新版本 {}（当前 {current}），下载地址：{RELEASES_PAGE}", latest.version);
    if let Some(notes) = latest.notes.as_deref().map(str::trim).filter(|n| !n.is_empty()) {
        text.push_str("\n更新说明：");
        text.push_str(notes);
    }
    Ok(Some(text))
}

/// 读取更新源并返回提示文字。
pub async fn check(url: &str, current: &str) -> Result<Option<String>, String> {
    let client =
        reqwest::Client::builder().timeout(Duration::from_secs(15)).build().map_err(|e| e.to_string())?;
    let resp = client.get(url).send().await.map_err(|e| format!("无法访问更新源 {url}：{e}"))?;
    if !resp.status().is_success() {
        return Err(format!("更新源 {url} 返回 {}", resp.status()));
    }
    let body = resp.bytes().await.map_err(|e| e.to_string())?;
    notice(current, &body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_semantic_versions() {
        assert_eq!(newer("0.1.0", "0.2.0"), Ok(true));
        assert_eq!(newer("0.1.0", "v0.1.1"), Ok(true));
        assert_eq!(newer("0.10.0", "0.9.0"), Ok(false), "按数字比较，不按字符串");
        assert_eq!(newer("0.1.0", "0.1.0"), Ok(false));
        assert_eq!(newer("0.1.0", "0.2.0-beta.1"), Ok(true));
        assert!(newer("0.1.0", "abc").unwrap_err().contains("无效的版本号"));
    }

    #[test]
    fn notice_text() {
        let json = r#"{"version":"0.2.0","notes":" 修复若干问题 ","pub_date":"2026-11-01T00:00:00Z","platforms":{}}"#.as_bytes();
        assert_eq!(
            notice("0.1.0", json).unwrap().unwrap(),
            format!("发现新版本 0.2.0（当前 0.1.0），下载地址：{RELEASES_PAGE}\n更新说明：修复若干问题")
        );
        assert_eq!(notice("0.2.0", json).unwrap(), None);
        assert!(notice("0.1.0", b"{}").unwrap_err().contains("格式不对"));
    }
}
