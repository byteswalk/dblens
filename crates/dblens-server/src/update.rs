//! 新版本提示（设计文档第 10 节）：浏览器模式的服务只提示有新版本，不会自动替换自己。
//! 读取的是桌面版同一个 `latest.json`，只用其中的 `version` 和 `notes`。

use std::time::Duration;

use dblens_core::msg;
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
        semver::Version::parse(v.trim().trim_start_matches('v'))
            .map_err(|e| msg!("server.bad_version", version = v, error = e).current())
    };
    Ok(parse(latest)? > parse(current)?)
}

/// 新版本的提示文字；`latest.json` 中的版本不比当前新时返回 `None`。
pub fn notice(current: &str, latest_json: &[u8]) -> Result<Option<String>, String> {
    let latest: Latest = serde_json::from_slice(latest_json)
        .map_err(|e| msg!("server.bad_latest_json", error = e).current())?;
    if !newer(current, &latest.version)? {
        return Ok(None);
    }
    let mut text =
        msg!("server.new_version", version = latest.version, current = current, url = RELEASES_PAGE)
            .current();
    if let Some(notes) = latest.notes.as_deref().map(str::trim).filter(|n| !n.is_empty()) {
        text.push('\n');
        text.push_str(&msg!("update.notes").current());
        text.push_str(notes);
    }
    Ok(Some(text))
}

/// 读取更新源并返回提示文字。
pub async fn check(url: &str, current: &str) -> Result<Option<String>, String> {
    let client =
        reqwest::Client::builder().timeout(Duration::from_secs(15)).build().map_err(|e| e.to_string())?;
    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| msg!("server.update_unreachable", url = url, error = e).current())?;
    if !resp.status().is_success() {
        return Err(msg!("server.update_status", url = url, status = resp.status()).current());
    }
    let body = resp.bytes().await.map_err(|e| e.to_string())?;
    notice(current, &body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use dblens_core::{Lang, with_lang};

    #[test]
    fn compares_semantic_versions() {
        assert_eq!(newer("0.1.0", "0.2.0"), Ok(true));
        assert_eq!(newer("0.1.0", "v0.1.1"), Ok(true));
        assert_eq!(newer("0.10.0", "0.9.0"), Ok(false), "按数字比较，不按字符串");
        assert_eq!(newer("0.1.0", "0.1.0"), Ok(false));
        assert_eq!(newer("0.1.0", "0.2.0-beta.1"), Ok(true));
        assert!(with_lang(Lang::ZhCn, || newer("0.1.0", "abc").unwrap_err()).contains("无效的版本号"));
    }

    #[test]
    fn notice_text() {
        let json = r#"{"version":"0.2.0","notes":" 修复若干问题 ","pub_date":"2026-11-01T00:00:00Z","platforms":{}}"#.as_bytes();
        assert_eq!(
            with_lang(Lang::ZhCn, || notice("0.1.0", json)).unwrap().unwrap(),
            format!("发现新版本 0.2.0（当前 0.1.0），下载地址：{RELEASES_PAGE}\n更新说明：修复若干问题")
        );
        assert_eq!(
            with_lang(Lang::En, || notice("0.1.0", json)).unwrap().unwrap(),
            format!(
                "Version 0.2.0 is available (current 0.1.0); download it from {RELEASES_PAGE}\nRelease notes: 修复若干问题"
            )
        );
        assert_eq!(notice("0.2.0", json).unwrap(), None);
        assert!(with_lang(Lang::ZhCn, || notice("0.1.0", b"{}")).unwrap_err().contains("格式不对"));
    }
}
