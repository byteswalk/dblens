//! 接口测试：服务层换成假驱动（见 `dblens_service::fake`），不需要真实数据库。

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use dblens_api::{ApiConfig, router};
use dblens_service::fake::{FakeDriver, Log};
use dblens_service::{DefaultPolicy, DriverRegistry, SecretBackend, Secrets, Service, Store};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

const TOKEN: &str = "test-token";

fn app() -> (Router, Arc<Service>) {
    let store = Arc::new(Store::open_in_memory().unwrap());
    let secrets = Arc::new(Secrets::with_backend(store.clone(), SecretBackend::Vault));
    secrets.init_vault("master").unwrap();
    let log: Log = Arc::default();
    let registry = Arc::new(DriverRegistry::with_drivers(store.clone(), vec![Box::new(FakeDriver { log })]));
    let service = Arc::new(Service::with_parts(store, secrets, registry, Arc::new(DefaultPolicy)));
    let config = ApiConfig { token: TOKEN.into(), allowed_origins: vec!["tauri://localhost".into()] };
    (router(service.clone(), config), service)
}

async fn call(app: &Router, method: &str, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
    let mut req =
        Request::builder().method(method).uri(uri).header(header::AUTHORIZATION, format!("Bearer {TOKEN}"));
    if body.is_some() {
        req = req.header(header::CONTENT_TYPE, "application/json");
    }
    let req = req.body(body.map_or_else(Body::empty, |b| Body::from(b.to_string()))).unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let json = if bytes.is_empty() { Value::Null } else { serde_json::from_slice(&bytes).unwrap() };
    (status, json)
}

fn profile_json(name: &str) -> Value {
    json!({"name": name, "driver": "postgres", "host": "db.local", "port": 5432, "user": "app", "environment": "prod"})
}

#[tokio::test]
async fn token_and_origin_are_checked() {
    let (app, _) = app();
    let send = |req: Request<Body>| {
        let app = app.clone();
        async move { app.oneshot(req).await.unwrap().status() }
    };
    let get = |auth: Option<&str>, origin: Option<&str>| {
        let mut r = Request::get("/api/profiles").header(header::HOST, "127.0.0.1:8742");
        if let Some(a) = auth {
            r = r.header(header::AUTHORIZATION, a);
        }
        if let Some(o) = origin {
            r = r.header(header::ORIGIN, o);
        }
        r.body(Body::empty()).unwrap()
    };
    assert_eq!(send(get(None, None)).await, StatusCode::UNAUTHORIZED);
    assert_eq!(send(get(Some("Bearer nope"), None)).await, StatusCode::UNAUTHORIZED);
    assert_eq!(send(get(Some("Bearer test-token"), None)).await, StatusCode::OK);
    assert_eq!(
        send(get(Some("Bearer test-token"), Some("https://evil.example"))).await,
        StatusCode::FORBIDDEN
    );
    assert_eq!(send(get(Some("Bearer test-token"), Some("http://127.0.0.1:8742"))).await, StatusCode::OK);
    assert_eq!(send(get(Some("Bearer test-token"), Some("tauri://localhost"))).await, StatusCode::OK);
    let by_query = Request::get("/api/profiles?token=test-token").body(Body::empty()).unwrap();
    assert_eq!(send(by_query).await, StatusCode::OK);
}

#[tokio::test]
async fn profile_crud_and_errors() {
    let (app, _) = app();
    let (st, p) = call(
        &app,
        "POST",
        "/api/profiles",
        Some(json!({"profile": profile_json("生产库"), "password": "pw"})),
    )
    .await;
    assert_eq!(st, StatusCode::CREATED, "{p}");
    let id = p["id"].as_str().unwrap().to_string();
    assert_eq!(p["environment"], "prod");
    let (_, list) = call(&app, "GET", "/api/profiles", None).await;
    assert_eq!(list.as_array().unwrap().len(), 1);

    let mut changed = profile_json("改名");
    changed["port"] = json!(5433);
    let (st, p) = call(&app, "PUT", &format!("/api/profiles/{id}"), Some(changed)).await;
    assert_eq!((st, p["port"].clone()), (StatusCode::OK, json!(5433)));
    let (st, _) =
        call(&app, "PUT", &format!("/api/profiles/{id}/password"), Some(json!({"password": null}))).await;
    assert_eq!(st, StatusCode::NO_CONTENT);

    let (st, e) = call(
        &app,
        "POST",
        "/api/profiles",
        Some(json!({"profile": {"name": "", "driver": "mysql", "host": "h", "port": 1, "user": "u"}})),
    )
    .await;
    assert_eq!((st, e["kind"].clone()), (StatusCode::BAD_REQUEST, json!("invalid")));

    let (st, _) = call(&app, "DELETE", &format!("/api/profiles/{id}"), None).await;
    assert_eq!(st, StatusCode::NO_CONTENT);
    let (st, e) = call(&app, "GET", &format!("/api/profiles/{id}"), None).await;
    assert_eq!((st, e["kind"].clone()), (StatusCode::NOT_FOUND, json!("not_found")));
}

#[tokio::test]
async fn secrets_drivers_and_test_connection() {
    let (app, _) = app();
    let (_, s) = call(&app, "GET", "/api/secrets", None).await;
    assert_eq!((s["backend"].clone(), s["unlocked"].clone()), (json!("vault"), json!(true)));
    call(&app, "POST", "/api/secrets/lock", None).await;
    let (st, e) =
        call(&app, "POST", "/api/profiles", Some(json!({"profile": profile_json("x"), "password": "pw"})))
            .await;
    assert_eq!((st, e["kind"].clone()), (StatusCode::LOCKED, json!("locked")));
    let (st, e) = call(&app, "POST", "/api/secrets/unlock", Some(json!({"master": "wrong"}))).await;
    assert_eq!((st, e["kind"].clone()), (StatusCode::UNPROCESSABLE_ENTITY, json!("auth")));
    let (st, _) = call(&app, "POST", "/api/secrets/unlock", Some(json!({"master": "master"}))).await;
    assert_eq!(st, StatusCode::OK);

    let (_, d) = call(&app, "GET", "/api/drivers", None).await;
    let pg = d.as_array().unwrap().iter().find(|x| x["id"] == "postgres").unwrap();
    assert_eq!(pg["available"], true);
    let (st, e) =
        call(&app, "PUT", "/api/drivers/dameng/lib-dir", Some(json!({"path": "relative/dir"}))).await;
    assert_eq!((st, e["kind"].clone()), (StatusCode::BAD_REQUEST, json!("invalid")));

    let (st, summary) = call(
        &app,
        "POST",
        "/api/profiles/test",
        Some(json!({"profile": profile_json("t"), "password": "pw"})),
    )
    .await;
    assert_eq!((st, summary["variant"].clone()), (StatusCode::OK, json!("postgres")));
    let (st, e) = call(
        &app,
        "POST",
        "/api/profiles/test",
        Some(json!({"profile": profile_json("t"), "password": "wrong"})),
    )
    .await;
    assert_eq!((st, e["kind"].clone()), (StatusCode::UNPROCESSABLE_ENTITY, json!("auth")));
}

#[tokio::test]
async fn session_lifecycle_over_rest() {
    let (app, _) = app();
    let (_, p) =
        call(&app, "POST", "/api/profiles", Some(json!({"profile": profile_json("p"), "password": "pw"})))
            .await;
    let (st, s) = call(&app, "POST", "/api/sessions", Some(json!({"profile_id": p["id"]}))).await;
    assert_eq!(st, StatusCode::CREATED, "{s}");
    let sid = s["id"].as_u64().unwrap();
    let (_, s) =
        call(&app, "POST", &format!("/api/sessions/{sid}/autocommit"), Some(json!({"on": false}))).await;
    assert_eq!(s["autocommit"], false);
    let (st, _) = call(&app, "POST", &format!("/api/sessions/{sid}/commit"), None).await;
    assert_eq!(st, StatusCode::OK);
    let (_, list) = call(&app, "GET", "/api/sessions", None).await;
    assert_eq!(list.as_array().unwrap().len(), 1);
    let (st, _) = call(&app, "DELETE", &format!("/api/sessions/{sid}?commit=false"), None).await;
    assert_eq!(st, StatusCode::NO_CONTENT);
    let (st, _) = call(&app, "GET", &format!("/api/sessions/{sid}"), None).await;
    assert_eq!(st, StatusCode::NOT_FOUND);
}
