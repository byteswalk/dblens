//! 设计规范检查接口（M7a）：规范库、发起检查、人工结论、导出报告。

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use dblens_api::{ApiConfig, router};
use dblens_service::{DefaultPolicy, DriverRegistry, HistoryLog, SecretBackend, Secrets, Service, Store};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

fn app() -> Router {
    let store = Arc::new(Store::open_in_memory().unwrap());
    let secrets = Arc::new(Secrets::with_backend(store.clone(), SecretBackend::Vault));
    let registry = Arc::new(DriverRegistry::with_drivers(store.clone(), vec![]));
    let history = HistoryLog::inline(store.clone());
    let service = Arc::new(Service::with_history(store, secrets, registry, Arc::new(DefaultPolicy), history));
    router(service, ApiConfig { token: "t".into(), allowed_origins: vec![], local_files: true })
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

async fn raw(app: &Router, uri: &str) -> (StatusCode, String, String) {
    let req = Request::builder()
        .method("GET")
        .uri(uri)
        .header(header::AUTHORIZATION, "Bearer t")
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let ct = resp.headers().get(header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).unwrap_or("").to_string();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    (status, ct, String::from_utf8_lossy(&bytes).to_string())
}

fn column(name: &str, native: &str, logical: Value) -> Value {
    json!({ "name": name, "ordinal": 0, "data_type": { "native": native, "logical": logical },
            "nullable": false, "default": null, "comment": null, "auto_increment": false, "generated": null })
}

async fn new_model(app: &Router, dir: &std::path::Path) -> String {
    let body = json!({ "path": dir.join("m"), "name": "m", "database": { "variant": "postgres" } });
    let (st, m) = call(app, "POST", "/api/models", Some(body)).await;
    assert_eq!(st, StatusCode::CREATED, "{m}");
    m["id"].as_str().unwrap().to_string()
}

#[tokio::test]
async fn builtin_rulesets_are_listed_and_readable() {
    let app = app();
    let (st, v) = call(&app, "GET", "/api/lint/rulesets", None).await;
    assert_eq!(st, StatusCode::OK);
    let ids: Vec<&str> = v.as_array().unwrap().iter().map(|r| r["id"].as_str().unwrap()).collect();
    assert!(ids.contains(&"general") && ids.contains(&"strict-oltp") && ids.contains(&"analytical"), "{v}");
    assert!(v.as_array().unwrap().iter().all(|r| r["builtin"] == true));

    let (st, set) = call(&app, "GET", "/api/lint/rulesets/general", None).await;
    assert_eq!(st, StatusCode::OK);
    assert!(!set["clauses"].as_array().unwrap().is_empty(), "{set}");

    let (st, rules) = call(&app, "GET", "/api/lint/rules", None).await;
    assert_eq!(st, StatusCode::OK);
    assert!(rules.as_array().unwrap().iter().any(|r| r["id"] == "table.comment"));
}

#[tokio::test]
async fn edit_reset_and_custom_rulesets() {
    let app = app();
    // 改内置：关掉第一条，确认列表标记为已改，恢复默认后复原
    let (_, mut set) = call(&app, "GET", "/api/lint/rulesets/general", None).await;
    set["clauses"][0]["enabled"] = json!(false);
    let (st, _) = call(&app, "PUT", "/api/lint/rulesets/general", Some(set)).await;
    assert_eq!(st, StatusCode::NO_CONTENT);
    let (_, list) = call(&app, "GET", "/api/lint/rulesets", None).await;
    let general = list.as_array().unwrap().iter().find(|r| r["id"] == "general").unwrap();
    assert_eq!(general["modified"], true);
    let (st, back) = call(&app, "POST", "/api/lint/rulesets/general/reset", None).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(back["clauses"][0]["enabled"], true, "{back}");

    // 导出 YAML、再导入成自定义规范，然后删除
    let (st, ct, yaml) = raw(&app, "/api/lint/rulesets/general/export").await;
    assert_eq!(st, StatusCode::OK);
    assert!(ct.contains("yaml"), "{ct}");
    let (st, v) = call(&app, "POST", "/api/lint/rulesets/import", Some(json!({ "yaml": yaml }))).await;
    assert_eq!(st, StatusCode::CREATED, "{v}");
    let id = v["id"].as_str().unwrap().to_string();
    let (_, list) = call(&app, "GET", "/api/lint/rulesets", None).await;
    assert!(list.as_array().unwrap().iter().any(|r| r["id"] == id && r["builtin"] == false));
    let (st, _) = call(&app, "DELETE", &format!("/api/lint/rulesets/{id}"), None).await;
    assert_eq!(st, StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn run_a_check_review_and_report() {
    let app = app();
    let dir = tempfile::tempdir().unwrap();
    let m = new_model(&app, dir.path()).await;
    // 一张缺注释、缺主键的表：注释规则会报问题，对象类规则对模型报无法判断
    let draft = json!({
        "name": "orders", "comment": null,
        "columns": [column("id", "integer", json!({ "kind": "int", "bits": 32 }))],
        "primary_key": null, "uniques": [], "foreign_keys": [], "checks": [],
        "indexes": [], "triggers": [], "partition": null
    });
    let save = json!({ "schema": "public", "name": null, "draft": draft, "fingerprint": null });
    let (st, v) = call(&app, "POST", &format!("/api/models/{m}/tables"), Some(save)).await;
    assert_eq!(st, StatusCode::OK, "{v}");

    let req = json!({ "ruleset": "general", "source": { "kind": "model", "model": m, "schema": "public" } });
    let (st, v) = call(&app, "POST", "/api/lint/runs", Some(req)).await;
    assert_eq!(st, StatusCode::CREATED, "{v}");
    let task = v["id"].as_str().unwrap().to_string();

    let mut run_id = String::new();
    for _ in 0..500 {
        let (_, t) = call(&app, "GET", &format!("/api/tasks/{task}"), None).await;
        if t["state"] != "running" {
            assert_eq!(t["state"], "done", "{t}");
            assert_eq!(t["kind"], "lint");
            run_id = t["lint_run"].as_str().unwrap().to_string();
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    assert!(!run_id.is_empty(), "task did not finish");

    let (st, run) = call(&app, "GET", &format!("/api/lint/runs/{run_id}"), None).await;
    assert_eq!(st, StatusCode::OK);
    let results = run["results"].as_array().unwrap();
    // 表注释条款应未通过
    let comment = results.iter().find(|r| r["clause"] == "table.comment").unwrap();
    assert_eq!(comment["verdict"], "fail", "{comment}");
    // 视图等对象规则对模型报无法判断
    assert!(results.iter().any(|r| r["verdict"] == "unknown"), "{run}");

    // 对一条无法判断的条款填人工结论
    let unknown = results.iter().find(|r| r["verdict"] == "unknown").unwrap();
    let clause = unknown["clause"].as_str().unwrap();
    let (st, after) = call(
        &app,
        "PUT",
        &format!("/api/lint/runs/{run_id}/clauses/{clause}"),
        Some(json!({ "verdict": "not_applicable", "note": "本模型不涉及" })),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{after}");
    let reviewed = after["results"].as_array().unwrap().iter().find(|r| r["clause"] == clause).unwrap();
    assert_eq!(reviewed["review"]["verdict"], "not_applicable");

    // 不能对已通过的条款填人工结论
    if let Some(passed) = results.iter().find(|r| r["verdict"] == "pass") {
        let c = passed["clause"].as_str().unwrap();
        let (st, _) = call(
            &app,
            "PUT",
            &format!("/api/lint/runs/{run_id}/clauses/{c}"),
            Some(json!({ "verdict": "pass", "note": "" })),
        )
        .await;
        assert_eq!(st, StatusCode::BAD_REQUEST);
    }

    // HTML 报告
    let (st, ct, html) =
        raw(&app, &format!("/api/lint/runs/{run_id}/report?format=html&reviewer=张三")).await;
    assert_eq!(st, StatusCode::OK);
    assert!(ct.contains("text/html"), "{ct}");
    assert!(html.contains("public.orders") || html.contains("orders"), "report missing target");
    assert!(html.contains("张三"), "report missing reviewer");

    // 删除检查记录
    let (st, _) = call(&app, "DELETE", &format!("/api/lint/runs/{run_id}"), None).await;
    assert_eq!(st, StatusCode::NO_CONTENT);
    let (st, _) = call(&app, "GET", &format!("/api/lint/runs/{run_id}"), None).await;
    assert_eq!(st, StatusCode::NOT_FOUND);
}
