//! 历史和收藏接口（M1c1）。

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use dblens_api::{ApiConfig, router};
use dblens_service::{
    DefaultPolicy, DriverRegistry, HistoryLog, NewHistory, SecretBackend, Secrets, Service, Store,
};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

fn app() -> (Router, Arc<Store>) {
    let store = Arc::new(Store::open_in_memory().unwrap());
    let secrets = Arc::new(Secrets::with_backend(store.clone(), SecretBackend::Vault));
    let registry = Arc::new(DriverRegistry::with_drivers(store.clone(), vec![]));
    let history = HistoryLog::inline(store.clone());
    let service =
        Arc::new(Service::with_history(store.clone(), secrets, registry, Arc::new(DefaultPolicy), history));
    (router(service, ApiConfig { token: "t".into(), allowed_origins: vec![], local_files: false }), store)
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

fn entry(sql: &str, profile: &str, error: Option<&str>) -> NewHistory {
    NewHistory {
        at: 1_700_000_000_000,
        profile_id: profile.into(),
        profile_name: "开发库".into(),
        driver: "mysql".into(),
        variant: Some("mariadb".into()),
        schema: None,
        sql: sql.into(),
        elapsed_ms: Some(2),
        rows: None,
        error: error.map(str::to_string),
    }
}

fn sqls(v: &Value) -> Vec<&str> {
    v.as_array().unwrap().iter().map(|i| i["sql"].as_str().unwrap()).collect()
}

#[tokio::test]
async fn history_query_delete_and_clear() {
    let (app, store) = app();
    store
        .add_history(
            &[
                entry("SELECT 1", "p1", None),
                entry("SELECT 2", "p2", Some("已取消")),
                entry("select 3", "p1", None),
            ],
            100,
        )
        .unwrap();
    let (st, all) = call(&app, "GET", "/api/history", None).await;
    assert_eq!((st, sqls(&all)), (StatusCode::OK, vec!["select 3", "SELECT 2", "SELECT 1"]));
    assert_eq!(all[0]["profile_name"], "开发库");
    assert_eq!(all[0]["variant"], "mariadb");
    assert_eq!(all[0]["error"], Value::Null);
    let id = all[0]["id"].as_i64().unwrap();

    let q = |uri: &'static str| call(&app, "GET", uri, None);
    assert_eq!(sqls(&q("/api/history?q=SELECT%203").await.1), ["select 3"]);
    assert_eq!(sqls(&q("/api/history?profile=p2").await.1), ["SELECT 2"]);
    assert_eq!(sqls(&q("/api/history?failed=true").await.1), ["SELECT 2"]);
    assert_eq!(
        sqls(&call(&app, "GET", &format!("/api/history?before={id}&limit=1"), None).await.1),
        ["SELECT 2"]
    );

    assert_eq!(call(&app, "DELETE", &format!("/api/history/{id}"), None).await.0, StatusCode::NO_CONTENT);
    let (st, body) = call(&app, "DELETE", &format!("/api/history/{id}"), None).await;
    assert_eq!((st, body["kind"].as_str()), (StatusCode::NOT_FOUND, Some("not_found")));
    assert_eq!(call(&app, "DELETE", "/api/history", None).await.0, StatusCode::NO_CONTENT);
    assert_eq!(q("/api/history").await.1, json!([]));
}

#[tokio::test]
async fn favorites_crud_and_move() {
    let (app, _) = app();
    let (st, folder) =
        call(&app, "POST", "/api/favorites", Some(json!({ "kind": "folder", "name": "日报" }))).await;
    assert_eq!(
        (st, folder["kind"].as_str(), folder["parent_id"].clone()),
        (StatusCode::CREATED, Some("folder"), Value::Null)
    );
    let fid = folder["id"].as_str().unwrap().to_string();
    let (st, item) = call(
        &app,
        "POST",
        "/api/favorites",
        Some(json!({ "kind": "item", "name": "当天订单", "sql": "SELECT * FROM orders", "driver": "mysql", "parent_id": fid })),
    )
    .await;
    assert_eq!((st, item["position"].as_i64()), (StatusCode::CREATED, Some(0)));
    let iid = item["id"].as_str().unwrap().to_string();

    let (st, body) =
        call(&app, "POST", "/api/favorites", Some(json!({ "kind": "item", "name": "空" }))).await;
    assert_eq!((st, body["kind"].as_str()), (StatusCode::BAD_REQUEST, Some("invalid")), "收藏必须有 SQL");

    let (st, moved) = call(
        &app,
        "PUT",
        &format!("/api/favorites/{iid}"),
        Some(json!({ "name": "订单", "note": "每天看", "move": { "parent_id": null, "position": 0 } })),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(
        (moved["name"].as_str(), moved["note"].as_str(), moved["parent_id"].clone()),
        (Some("订单"), Some("每天看"), Value::Null)
    );

    let (st, body) = call(
        &app,
        "PUT",
        &format!("/api/favorites/{fid}"),
        Some(json!({ "move": { "parent_id": fid, "position": 0 } })),
    )
    .await;
    assert_eq!((st, body["kind"].as_str()), (StatusCode::BAD_REQUEST, Some("invalid")), "不能移进自己");

    let (_, all) = call(&app, "GET", "/api/favorites", None).await;
    let names: Vec<_> = all
        .as_array()
        .unwrap()
        .iter()
        .map(|f| (f["name"].as_str().unwrap(), f["position"].as_i64().unwrap()))
        .collect();
    assert_eq!(names, [("订单", 0), ("日报", 1)]);

    assert_eq!(call(&app, "DELETE", &format!("/api/favorites/{fid}"), None).await.0, StatusCode::NO_CONTENT);
    assert_eq!(call(&app, "GET", "/api/favorites", None).await.1.as_array().unwrap().len(), 1);
    let (st, body) = call(&app, "DELETE", &format!("/api/favorites/{fid}"), None).await;
    assert_eq!((st, body["kind"].as_str()), (StatusCode::NOT_FOUND, Some("not_found")));
}
