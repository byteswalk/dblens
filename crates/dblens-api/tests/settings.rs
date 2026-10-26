//! 设置接口：界面状态（主题、面板大小、打开的标签页等）存进本地库。

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use dblens_api::{ApiConfig, router};
use dblens_service::fake::FakeDriver;
use dblens_service::{DefaultPolicy, DriverRegistry, SecretBackend, Secrets, Service, Store};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

fn app() -> Router {
    let store = Arc::new(Store::open_in_memory().unwrap());
    let secrets = Arc::new(Secrets::with_backend(store.clone(), SecretBackend::Vault));
    let registry = Arc::new(DriverRegistry::with_drivers(
        store.clone(),
        vec![Box::new(FakeDriver { log: Arc::default() })],
    ));
    let service = Arc::new(Service::with_parts(store, secrets, registry, Arc::new(DefaultPolicy)));
    router(service, ApiConfig { token: "t".into(), allowed_origins: vec![] })
}

async fn call(app: &Router, method: &str, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
    let mut req = Request::builder().method(method).uri(uri).header(header::AUTHORIZATION, "Bearer t");
    if body.is_some() {
        req = req.header(header::CONTENT_TYPE, "application/json");
    }
    let req = req.body(body.map_or_else(Body::empty, |b| Body::from(b.to_string()))).unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    (status, if bytes.is_empty() { Value::Null } else { serde_json::from_slice(&bytes).unwrap() })
}

async fn put(app: &Router, key: &str, value: Value) -> (StatusCode, Value) {
    call(app, "PUT", &format!("/api/settings/{key}"), Some(json!({ "value": value }))).await
}

#[tokio::test]
async fn read_write_and_delete_settings() {
    let app = app();
    assert_eq!(call(&app, "GET", "/api/settings", None).await, (StatusCode::OK, json!({})));
    assert_eq!(put(&app, "ui.theme", json!("dark")).await.0, StatusCode::NO_CONTENT);
    assert_eq!(put(&app, "ui.layout", json!("{\"side\":270}")).await.0, StatusCode::NO_CONTENT);
    assert_eq!(
        call(&app, "GET", "/api/settings", None).await,
        (StatusCode::OK, json!({ "ui.layout": "{\"side\":270}", "ui.theme": "dark" }))
    );
    assert_eq!(put(&app, "ui.theme", Value::Null).await.0, StatusCode::NO_CONTENT, "null 删除");
    assert_eq!(call(&app, "GET", "/api/settings", None).await.1, json!({ "ui.layout": "{\"side\":270}" }));
}

#[tokio::test]
async fn only_ui_settings_are_visible() {
    let store = Arc::new(Store::open_in_memory().unwrap());
    store.set_setting("native.dpi_lib_dir", Some("/opt/dm")).unwrap();
    store.set_setting("ui.theme", Some("dark")).unwrap();
    let secrets = Arc::new(Secrets::with_backend(store.clone(), SecretBackend::Vault));
    let registry = Arc::new(DriverRegistry::with_drivers(store.clone(), vec![]));
    let service = Arc::new(Service::with_parts(store, secrets, registry, Arc::new(DefaultPolicy)));
    let app = router(service, ApiConfig { token: "t".into(), allowed_origins: vec![] });
    assert_eq!(call(&app, "GET", "/api/settings", None).await.1, json!({ "ui.theme": "dark" }));
}

#[tokio::test]
async fn rejects_bad_keys_and_huge_values() {
    let app = app();
    let long = format!("ui.{}", "x".repeat(62));
    for key in ["UI.theme", "ui.a%20b", long.as_str(), "native.dpi_lib_dir", "theme"] {
        let (st, body) = put(&app, key, json!("1")).await;
        assert_eq!((st, body["kind"].as_str()), (StatusCode::BAD_REQUEST, Some("invalid")), "{key}");
    }
    let (st, body) = put(&app, "ui.big", json!("x".repeat(64 * 1024 + 1))).await;
    assert_eq!((st, body["kind"].as_str()), (StatusCode::BAD_REQUEST, Some("invalid")));
    assert_eq!(call(&app, "GET", "/api/settings", None).await.1, json!({}), "被拒绝的都没有写入");
}
