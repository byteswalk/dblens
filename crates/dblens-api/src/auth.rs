//! 令牌与 Origin 校验（设计文档 4.1 节）。
//!
//! - 每个请求都要带令牌：`Authorization: Bearer <令牌>`；浏览器的 WebSocket 不能设置请求头，
//!   所以也接受查询参数 `token=<令牌>`。
//! - 带 `Origin` 头的请求（即浏览器发出的请求），Origin 必须与服务同源，或在允许列表中，防止其他网页借用户的浏览器访问本服务。
//!   不带 Origin 的请求（命令行工具等）只校验令牌。

use axum::extract::{Request, State};
use axum::http::{HeaderMap, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use dblens_core::Error;

use crate::AppState;
use crate::error::ApiError;

/// 逐字节比较且耗时与内容无关，避免按响应时间猜出令牌。
fn same_token(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn token_from(headers: &HeaderMap, query: Option<&str>) -> Option<String> {
    if let Some(v) = headers.get(header::AUTHORIZATION).and_then(|v| v.to_str().ok())
        && let Some(t) = v.strip_prefix("Bearer ")
    {
        return Some(t.trim().to_string());
    }
    query?.split('&').find_map(|kv| kv.strip_prefix("token=")).map(str::to_string)
}

fn origin_allowed(headers: &HeaderMap, allowed: &[String]) -> bool {
    let Some(origin) = headers.get(header::ORIGIN).and_then(|v| v.to_str().ok()) else { return true };
    if allowed.iter().any(|a| a == origin) {
        return true;
    }
    // 同源：Origin 的主机部分与 Host 头一致
    let host = headers.get(header::HOST).and_then(|v| v.to_str().ok()).unwrap_or("");
    let origin_host = origin.split_once("://").map_or("", |(_, rest)| rest);
    !host.is_empty() && origin_host == host
}

pub async fn guard(State(state): State<AppState>, request: Request, next: Next) -> Response {
    let headers = request.headers();
    if !origin_allowed(headers, &state.config.allowed_origins) {
        return ApiError(Error::Denied { reason: "不允许的来源（Origin）".into() }).into_response();
    }
    match token_from(headers, request.uri().query()) {
        Some(t) if same_token(&t, &state.config.token) => next.run(request).await,
        _ => (
            axum::http::StatusCode::UNAUTHORIZED,
            axum::Json(Error::Auth { message: "缺少或错误的访问令牌".into() }),
        )
            .into_response(),
    }
}

#[cfg(test)]
mod tests {
    use axum::http::HeaderValue;

    use super::*;

    #[test]
    fn token_from_header_or_query() {
        let mut h = HeaderMap::new();
        assert_eq!(token_from(&h, Some("a=1&token=abc")), Some("abc".into()));
        h.insert(header::AUTHORIZATION, HeaderValue::from_static("Bearer xyz"));
        assert_eq!(token_from(&h, Some("token=abc")), Some("xyz".into()), "请求头优先");
        assert_eq!(token_from(&HeaderMap::new(), None), None);
        assert!(same_token("abc", "abc") && !same_token("abc", "abd") && !same_token("abc", "ab"));
    }

    #[test]
    fn origin_rules() {
        let allowed = vec!["tauri://localhost".to_string()];
        let mut h = HeaderMap::new();
        assert!(origin_allowed(&h, &allowed), "没有 Origin 的请求只看令牌");
        h.insert(header::HOST, HeaderValue::from_static("127.0.0.1:8742"));
        h.insert(header::ORIGIN, HeaderValue::from_static("http://127.0.0.1:8742"));
        assert!(origin_allowed(&h, &allowed), "同源");
        h.insert(header::ORIGIN, HeaderValue::from_static("tauri://localhost"));
        assert!(origin_allowed(&h, &allowed), "在允许列表中");
        h.insert(header::ORIGIN, HeaderValue::from_static("https://evil.example"));
        assert!(!origin_allowed(&h, &allowed));
        h.insert(header::ORIGIN, HeaderValue::from_static("http://127.0.0.1:9999"));
        assert!(!origin_allowed(&h, &allowed), "端口不同不算同源");
    }
}
