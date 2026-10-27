//! 前端静态文件服务的测试：页面不需要令牌，接口仍然需要；单页应用的路径回退到 index.html。

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use dblens_api::{ApiConfig, router, with_web};
use dblens_service::fake::FakeDriver;
use dblens_service::{DefaultPolicy, DriverRegistry, SecretBackend, Secrets, Service, Store};
use http_body_util::BodyExt;
use tower::ServiceExt;

fn app(dir: &std::path::Path) -> Router {
    let store = Arc::new(Store::open_in_memory().unwrap());
    let secrets = Arc::new(Secrets::with_backend(store.clone(), SecretBackend::Vault));
    let registry = Arc::new(DriverRegistry::with_drivers(
        store.clone(),
        vec![Box::new(FakeDriver { log: Arc::default() })],
    ));
    let service = Arc::new(Service::with_parts(store, secrets, registry, Arc::new(DefaultPolicy)));
    let config = ApiConfig { token: "t".into(), allowed_origins: vec![], local_files: false };
    with_web(router(service, config), dir.to_path_buf())
}

async fn get(app: &Router, uri: &str) -> (StatusCode, String, String, String) {
    let resp = app.clone().oneshot(Request::get(uri).body(Body::empty()).unwrap()).await.unwrap();
    let header_of = |name| {
        resp.headers()
            .get(name)
            .map(|v: &header::HeaderValue| v.to_str().unwrap().to_string())
            .unwrap_or_default()
    };
    let (ct, cache) = (header_of(header::CONTENT_TYPE), header_of(header::CACHE_CONTROL));
    let status = resp.status();
    let body = String::from_utf8(resp.into_body().collect().await.unwrap().to_bytes().to_vec()).unwrap();
    (status, ct, cache, body)
}

#[tokio::test]
async fn serves_pages_without_token_and_keeps_api_protected() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("index.html"), "<html>首页</html>").unwrap();
    std::fs::create_dir(dir.path().join("assets")).unwrap();
    std::fs::write(dir.path().join("assets").join("app-1a2b.js"), "console.log(1)").unwrap();
    let app = app(dir.path());

    let (st, ct, cache, body) = get(&app, "/").await;
    assert_eq!(
        (st, ct.as_str(), cache.as_str(), body.as_str()),
        (StatusCode::OK, "text/html; charset=utf-8", "no-cache", "<html>首页</html>")
    );
    let (st, ct, cache, _) = get(&app, "/assets/app-1a2b.js").await;
    assert_eq!(
        (st, ct.as_str(), cache.as_str()),
        (StatusCode::OK, "text/javascript; charset=utf-8", "public, max-age=31536000, immutable")
    );
    // 单页应用的前端路由
    let (st, _, _, body) = get(&app, "/connections/abc").await;
    assert_eq!((st, body.as_str()), (StatusCode::OK, "<html>首页</html>"));
    // 带扩展名的文件不存在就是 404，不回退
    assert_eq!(get(&app, "/assets/missing.js").await.0, StatusCode::NOT_FOUND);
    // 跳出目录的路径
    assert_eq!(get(&app, "/../Cargo.toml").await.0, StatusCode::NOT_FOUND);
    assert_eq!(get(&app, "/assets/..%2f..%2fCargo.toml").await.0, StatusCode::NOT_FOUND);
    // 接口仍然要令牌；/api 下不存在的路径不回退到页面
    assert_eq!(get(&app, "/api/profiles").await.0, StatusCode::UNAUTHORIZED);
    assert_eq!(get(&app, "/api/profiles?token=t").await.0, StatusCode::OK);
    assert_eq!(get(&app, "/api/nope?token=t").await.0, StatusCode::NOT_FOUND);
    assert_eq!(get(&app, "/api").await.0, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn missing_web_dir_is_404_not_a_crash() {
    let dir = tempfile::tempdir().unwrap();
    let app = app(&dir.path().join("does-not-exist"));
    assert_eq!(get(&app, "/").await.0, StatusCode::NOT_FOUND);
    assert_eq!(get(&app, "/x").await.0, StatusCode::NOT_FOUND);
}
