//! 请求语言（M1i 设计文档第 5.2 节）：查询参数 `lang` 优先（WebSocket 用），其次 `Accept-Language` 头。
//! 整个请求在该语言下处理，响应里的文字（错误、提示）在序列化时按它渲染；两者都没有时用默认语言。

use axum::extract::Request;
use axum::http::header;
use axum::middleware::Next;
use axum::response::Response;
use dblens_core::{Lang, scope_lang};

pub fn request_lang(request: &Request) -> Option<Lang> {
    let from_query = request
        .uri()
        .query()
        .and_then(|q| q.split('&').find_map(|kv| kv.strip_prefix("lang=")))
        .filter(|v| !v.is_empty())
        .map(Lang::from_tag);
    from_query.or_else(|| {
        request
            .headers()
            .get(header::ACCEPT_LANGUAGE)
            .and_then(|v| v.to_str().ok())
            .and_then(Lang::from_accept_language)
    })
}

pub async fn negotiate(request: Request, next: Next) -> Response {
    match request_lang(&request) {
        Some(lang) => scope_lang(lang, next.run(request)).await,
        None => next.run(request).await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;

    fn req(uri: &str, accept: Option<&str>) -> Request {
        let mut b = Request::builder().uri(uri);
        if let Some(a) = accept {
            b = b.header(header::ACCEPT_LANGUAGE, a);
        }
        b.body(Body::empty()).unwrap()
    }

    #[test]
    fn query_param_wins_over_header() {
        assert_eq!(request_lang(&req("/api/x?token=t&lang=en", Some("zh-CN"))), Some(Lang::En));
        assert_eq!(request_lang(&req("/api/x?lang=zh-CN", None)), Some(Lang::ZhCn));
        assert_eq!(request_lang(&req("/api/x", Some("zh-CN,zh;q=0.9"))), Some(Lang::ZhCn));
        assert_eq!(request_lang(&req("/api/x", Some("en-US"))), Some(Lang::En));
        assert_eq!(request_lang(&req("/api/x?lang=", None)), None);
        assert_eq!(request_lang(&req("/api/x", None)), None);
    }
}
