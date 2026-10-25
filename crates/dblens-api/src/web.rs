//! 前端页面（`web/` 构建出的 `dist` 目录）的静态文件服务，桌面模式和浏览器模式共用。
//!
//! - 页面与 `/api` 同源，前端调用接口不涉及跨域。
//! - 静态文件不需要令牌：页面加载之后才拿得到令牌。业务全部在 `/api` 下，仍受令牌保护。
//! - 前端是单页应用：没有扩展名的路径找不到文件时返回 `index.html`，由前端路由处理。
//!   `/api` 下不存在的路径一律 404，不回退到页面。

use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::Router;
use axum::http::{HeaderValue, StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};

/// 给接口路由加上前端页面：凡是没有匹配到接口的 GET 请求，都到 `dir` 中找文件。
pub fn with_web(app: Router, dir: PathBuf) -> Router {
    let dir = Arc::new(dir);
    app.fallback(move |uri: Uri| serve(dir.clone(), uri))
}

/// 把 URL 路径转成 `dir` 下的相对路径。含 `..`、`.`、空段、反斜杠或冒号（Windows 盘符）的路径一律拒绝。
/// 路径不做百分号解码：`%2e%2e` 只会被当成一个普通文件名，找不到就是 404。
fn safe_relative(path: &str) -> Option<PathBuf> {
    let rest = path.strip_prefix('/')?;
    let mut out = PathBuf::new();
    if rest.is_empty() {
        return Some(out);
    }
    for seg in rest.split('/') {
        if seg.is_empty() || seg == "." || seg == ".." || seg.contains(['\\', ':']) {
            return None;
        }
        out.push(seg);
    }
    Some(out)
}

fn content_type(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()).unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" | "map" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "ico" => "image/x-icon",
        "woff2" => "font/woff2",
        "woff" => "font/woff",
        "ttf" => "font/ttf",
        "wasm" => "application/wasm",
        "txt" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

fn file_response(path: &Path, bytes: Vec<u8>) -> Response {
    // Vite 输出的 assets/ 下的文件名带内容哈希，可以长期缓存；index.html 每次都要重新验证
    let cache = if path.components().next().is_some_and(|c| c.as_os_str() == "assets") {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    };
    let mut resp = bytes.into_response();
    let h = resp.headers_mut();
    h.insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type(path)));
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static(cache));
    h.insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    resp
}

async fn serve(dir: Arc<PathBuf>, uri: Uri) -> Response {
    let Some(rel) = safe_relative(uri.path()) else { return StatusCode::NOT_FOUND.into_response() };
    if rel.starts_with("api") {
        return StatusCode::NOT_FOUND.into_response();
    }
    let rel = if rel.as_os_str().is_empty() { PathBuf::from("index.html") } else { rel };
    if let Ok(bytes) = tokio::fs::read(dir.join(&rel)).await {
        return file_response(&rel, bytes);
    }
    if rel.extension().is_none()
        && let Ok(bytes) = tokio::fs::read(dir.join("index.html")).await
    {
        return file_response(Path::new("index.html"), bytes);
    }
    StatusCode::NOT_FOUND.into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_paths_leaving_the_directory() {
        assert_eq!(safe_relative("/"), Some(PathBuf::new()));
        assert_eq!(safe_relative("/assets/a.js"), Some(PathBuf::from("assets").join("a.js")));
        for bad in ["/../x", "/a/../../x", "/./a", "/a//b", "/a\\..\\b", "/C:/x", "no-slash"] {
            assert_eq!(safe_relative(bad), None, "{bad}");
        }
    }

    #[test]
    fn content_types() {
        assert_eq!(content_type(Path::new("index.html")), "text/html; charset=utf-8");
        assert_eq!(content_type(Path::new("assets/x.js")), "text/javascript; charset=utf-8");
        assert_eq!(content_type(Path::new("x.unknown")), "application/octet-stream");
    }
}
