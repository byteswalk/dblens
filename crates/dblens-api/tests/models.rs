//! 模型与比对接口（M4a）。

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use dblens_api::{ApiConfig, router};
use dblens_service::{DefaultPolicy, DriverRegistry, HistoryLog, SecretBackend, Secrets, Service, Store};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

fn app(local_files: bool) -> Router {
    let store = Arc::new(Store::open_in_memory().unwrap());
    let secrets = Arc::new(Secrets::with_backend(store.clone(), SecretBackend::Vault));
    let registry = Arc::new(DriverRegistry::with_drivers(store.clone(), vec![]));
    let history = HistoryLog::inline(store.clone());
    let service = Arc::new(Service::with_history(store, secrets, registry, Arc::new(DefaultPolicy), history));
    // 迁移记录放在单独的临时目录（默认目录可能有其他测试留下的记录）
    service.migrations.set_dir(tempfile::tempdir().unwrap().keep().join("migrations"));
    router(service, ApiConfig { token: "t".into(), allowed_origins: vec![], local_files })
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

fn column(name: &str) -> Value {
    json!({ "name": name, "ordinal": 0, "data_type": { "native": "integer", "logical": { "kind": "other" } },
            "nullable": false, "default": null, "comment": null, "auto_increment": false, "generated": null })
}

fn draft(name: &str, cols: &[&str]) -> Value {
    json!({ "name": name, "comment": null, "columns": cols.iter().map(|c| column(c)).collect::<Vec<_>>(),
            "primary_key": { "name": null, "columns": [cols[0]] }, "uniques": [], "foreign_keys": [],
            "checks": [], "indexes": [], "triggers": [], "partition": null })
}

async fn new_model(app: &Router, dir: &std::path::Path, name: &str) -> String {
    let body = json!({ "path": dir.join(name), "name": name, "database": { "variant": "postgres" } });
    let (st, m) = call(app, "POST", "/api/models", Some(body)).await;
    assert_eq!(st, StatusCode::CREATED, "{m}");
    m["id"].as_str().unwrap().to_string()
}

#[tokio::test]
async fn models_need_local_files() {
    let app = app(false);
    let (st, v) = call(&app, "GET", "/api/models", None).await;
    assert_eq!(st, StatusCode::FORBIDDEN, "{v}");
    let (st, _) = call(&app, "POST", "/api/models/open", Some(json!({ "path": "/tmp/x" }))).await;
    assert_eq!(st, StatusCode::FORBIDDEN);
    let (st, _) =
        call(&app, "POST", "/api/compare/script", Some(json!({ "path": "/tmp/x.sql", "text": "" }))).await;
    assert_eq!(st, StatusCode::FORBIDDEN);
    let (st, _) = call(&app, "GET", "/api/models/x/diagrams?schema=s", None).await;
    assert_eq!(st, StatusCode::FORBIDDEN);
    let (st, _) =
        call(&app, "POST", "/api/files/image", Some(json!({ "path": "/tmp/x.png", "data": "" }))).await;
    assert_eq!(st, StatusCode::FORBIDDEN);
    let (st, _) = call(
        &app,
        "POST",
        "/api/documents/templates/export",
        Some(json!({ "format": "html", "path": "/tmp/t.html" })),
    )
    .await;
    assert_eq!(st, StatusCode::FORBIDDEN);
    // 浏览器模式：本机路径作为检查出的问题
    let req = json!({ "source": { "kind": "database", "profile": "p", "schema": "s" }, "kind": "dictionary",
                      "format": "html", "template": "/tmp/t.html" });
    let (st, v) = call(&app, "POST", "/api/documents/check", Some(req)).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(v["problems"][0]["field"], "template", "{v}");
}

/// ER 图与逻辑外键（M4b）。
#[tokio::test]
async fn diagrams_and_logical_keys() {
    let app = app(true);
    let dir = tempfile::tempdir().unwrap();
    let m = new_model(&app, dir.path(), "m").await;
    let save = |name: Option<&str>, d: Value| json!({ "schema": "public", "name": name, "draft": d, "fingerprint": null });
    for t in ["a", "b"] {
        let (st, v) =
            call(&app, "POST", &format!("/api/models/{m}/tables"), Some(save(None, draft(t, &["id", "x"]))))
                .await;
        assert_eq!(st, StatusCode::OK, "{v}");
    }
    // 默认图：没有文件时也能读，第一次保存建文件
    let (_, list) = call(&app, "GET", &format!("/api/models/{m}/diagrams?schema=public"), None).await;
    assert_eq!(list[0]["name"], "_all");
    let (st, v) = call(&app, "GET", &format!("/api/models/{m}/diagrams/public/_all"), None).await;
    assert_eq!((st, &v["fingerprint"]), (StatusCode::OK, &Value::Null), "{v}");
    assert_eq!(v["tables"].as_array().unwrap().len(), 2);
    let mut d = v["diagram"].clone();
    d["positions"] = json!({ "a": { "x": 10, "y": 20 } });
    let (st, r) = call(
        &app,
        "PUT",
        &format!("/api/models/{m}/diagrams/public/_all"),
        Some(json!({ "diagram": d, "fingerprint": null })),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{r}");
    let (st, _) = call(
        &app,
        "PUT",
        &format!("/api/models/{m}/diagrams/public/_all"),
        Some(json!({ "diagram": d, "fingerprint": null })),
    )
    .await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
    // 主题图
    let body = json!({ "schema": "public", "name": "t1", "tables": ["a"] });
    let (st, _) = call(&app, "POST", &format!("/api/models/{m}/diagrams"), Some(body)).await;
    assert_eq!(st, StatusCode::CREATED);
    let (st, _) = call(
        &app,
        "POST",
        &format!("/api/models/{m}/diagrams/public/t1/rename"),
        Some(json!({ "to": "t2" })),
    )
    .await;
    assert_eq!(st, StatusCode::NO_CONTENT);
    let (_, v) = call(&app, "GET", &format!("/api/models/{m}/diagrams/public/t2"), None).await;
    assert_eq!(v["diagram"]["tables"], json!(["a"]));
    let (st, _) = call(&app, "DELETE", &format!("/api/models/{m}/diagrams/public/t2"), None).await;
    assert_eq!(st, StatusCode::NO_CONTENT);
    // 逻辑外键
    let key = json!({ "columns": ["x"], "ref_table": "b", "ref_columns": ["id"], "comment": "c" });
    let (st, r) = call(
        &app,
        "POST",
        &format!("/api/models/{m}/tables/public/a/logical-keys"),
        Some(json!({ "key": key })),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{r}");
    let (_, v) = call(&app, "GET", &format!("/api/models/{m}/diagrams/public/_all"), None).await;
    let a = v["tables"].as_array().unwrap().iter().find(|t| t["table"]["name"] == "a").unwrap().clone();
    assert_eq!(a["logical_keys"][0]["ref_table"], "b");
    let bad = json!({ "key": { "columns": ["x"], "ref_table": "b", "ref_columns": ["nope"] } });
    let (st, _) =
        call(&app, "POST", &format!("/api/models/{m}/tables/public/a/logical-keys"), Some(bad)).await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
    let fp = r["fingerprint"].as_str().unwrap();
    let (st, _) = call(
        &app,
        "DELETE",
        &format!("/api/models/{m}/tables/public/a/logical-keys/0?fingerprint={fp}"),
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    // 导出图片只写 .svg、.png
    let png = dir.path().join("er.png");
    let (st, _) =
        call(&app, "POST", "/api/files/image", Some(json!({ "path": png, "data": "iVBORw0K" }))).await;
    assert_eq!(st, StatusCode::NO_CONTENT);
    assert_eq!(std::fs::read(&png).unwrap(), [0x89, b'P', b'N', b'G', 0x0d, 0x0a]);
    let (st, _) =
        call(&app, "POST", "/api/files/image", Some(json!({ "path": dir.path().join("x.exe"), "data": "" })))
            .await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn save_script_only_writes_sql_files() {
    let app = app(true);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sync.SQL");
    let (st, _) =
        call(&app, "POST", "/api/compare/script", Some(json!({ "path": path, "text": "SELECT 1;" }))).await;
    assert_eq!(st, StatusCode::NO_CONTENT);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "SELECT 1;");
    let other = dir.path().join("x.txt");
    let (st, _) =
        call(&app, "POST", "/api/compare/script", Some(json!({ "path": other, "text": "x" }))).await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
    assert!(!other.exists());
}

#[tokio::test]
async fn edit_compare_and_write_back() {
    let app = app(true);
    let dir = tempfile::tempdir().unwrap();
    let a = new_model(&app, dir.path(), "a").await;
    let b = new_model(&app, dir.path(), "b").await;
    let (_, list) = call(&app, "GET", "/api/models", None).await;
    assert_eq!(list["open"].as_array().unwrap().len(), 2);
    assert_eq!(list["recent"].as_array().unwrap().len(), 2);

    // 在 a 中新建表、改名；b 中有一张旧表
    let save = |name: Option<&str>, d: Value, fp: Option<&str>| json!({ "schema": "public", "name": name, "draft": d, "fingerprint": fp });
    let (st, v) = call(
        &app,
        "POST",
        &format!("/api/models/{a}/tables"),
        Some(save(None, draft("t", &["id", "x"]), None)),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let (st, _) =
        call(&app, "POST", &format!("/api/models/{b}/tables"), Some(save(None, draft("old", &["id"]), None)))
            .await;
    assert_eq!(st, StatusCode::OK);
    let (st, _) = call(
        &app,
        "POST",
        &format!("/api/models/{a}/tables/public/t/rename"),
        Some(json!({ "to": "items" })),
    )
    .await;
    assert_eq!(st, StatusCode::NO_CONTENT);
    let (_, tables) = call(&app, "GET", &format!("/api/models/{a}/tables"), None).await;
    assert_eq!(tables[0]["name"], "items");
    let (st, t) = call(&app, "GET", &format!("/api/models/{a}/tables/public/items"), None).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(t["table"]["columns"][1]["name"], "x");
    // 旧指纹保存：预览标出冲突
    let req = save(Some("items"), draft("items", &["id", "x", "y"]), Some("stale"));
    let (_, p) = call(&app, "POST", &format!("/api/models/{a}/tables/preview"), Some(req)).await;
    assert_eq!(p["conflict"], true);

    // a → b：新增 items、删除 old
    let side = |m: &str| json!({ "kind": "model", "model": m, "schema": "public" });
    let cmp = json!({ "source": side(&a), "target": side(&b) });
    let (st, r) = call(&app, "POST", "/api/compare", Some(cmp.clone())).await;
    assert_eq!(st, StatusCode::OK, "{r}");
    let ids: Vec<u64> = r["diff"]["tables"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|t| t["items"].as_array().unwrap().iter().map(|i| i["id"].as_u64().unwrap()))
        .collect();
    assert_eq!(ids.len(), 2);
    let sync = json!({ "compare": cmp, "selected": ids, "fingerprint": r["fingerprint"] });
    let (_, p) = call(&app, "POST", "/api/compare/preview", Some(sync.clone())).await;
    assert_eq!(p["writes"].as_array().unwrap().len(), 2, "{p}");
    let (st, done) = call(&app, "POST", "/api/compare/sync", Some(sync)).await;
    assert_eq!((st, &done["task"]), (StatusCode::OK, &Value::Null), "{done}");
    let (_, tables) = call(&app, "GET", &format!("/api/models/{b}/tables"), None).await;
    let names: Vec<&str> = tables.as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["items"]);

    let (st, _) = call(&app, "DELETE", &format!("/api/models/{b}"), None).await;
    assert_eq!(st, StatusCode::NO_CONTENT);
    let (st, _) = call(&app, "GET", &format!("/api/models/{b}"), None).await;
    assert_eq!(st, StatusCode::NOT_FOUND);
}

/// 文档导出（M4c）：检查、导出内置模板、模型导出数据字典后下载。
#[tokio::test]
async fn documents() {
    let app = app(true);
    let dir = tempfile::tempdir().unwrap();
    let m = new_model(&app, dir.path(), "m").await;
    let save = json!({ "schema": "public", "name": null, "draft": draft("orders", &["id", "x"]), "fingerprint": null });
    let (st, v) = call(&app, "POST", &format!("/api/models/{m}/tables"), Some(save)).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let source = json!({ "kind": "model", "model": m, "schemas": ["public"] });
    // xlsx 只能用于数据字典
    let req = json!({ "source": source, "kind": "design", "format": "xlsx" });
    let (_, v) = call(&app, "POST", "/api/documents/check", Some(req.clone())).await;
    assert_eq!(v["problems"][0]["field"], "format", "{v}");
    let (st, _) = call(&app, "POST", "/api/documents", Some(req)).await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
    // 导出内置模板
    let path = dir.path().join("t.md");
    let (st, _) = call(
        &app,
        "POST",
        "/api/documents/templates/export",
        Some(json!({ "format": "markdown", "path": path })),
    )
    .await;
    assert_eq!(st, StatusCode::NO_CONTENT);
    assert!(std::fs::read_to_string(&path).unwrap().contains("{{ info.title }}"));
    // 用这份模板导出数据字典，写到本机路径
    let out = dir.path().join("dict.md");
    let req = json!({ "source": source, "kind": "dictionary", "format": "markdown", "template": path,
                      "info": { "title": "订单库" }, "output": out });
    let (st, v) = call(&app, "POST", "/api/documents", Some(req)).await;
    assert_eq!(st, StatusCode::CREATED, "{v}");
    let id = v["id"].as_str().unwrap().to_string();
    for _ in 0..500 {
        let (_, t) = call(&app, "GET", &format!("/api/tasks/{id}"), None).await;
        if t["state"] != "running" {
            assert_eq!(t["state"], "done", "{t}");
            assert_eq!(t["kind"], "document");
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    let md = std::fs::read_to_string(&out).unwrap();
    assert!(
        md.starts_with(
            "# 订单库
"
        ) && md.contains("public.orders"),
        "{md}"
    );
}

/// 结构迁移（M5a）：方案能解析；连接不存在时预览和开始都报 404。
#[tokio::test]
async fn migrations_need_existing_profiles() {
    let app = app(true);
    let plan = json!({ "source": { "profile": "nope", "schema": "s" }, "target": { "profile": "nope2", "schema": "t" },
                       "settings": { "options": { "name_case": "upper" }, "type_rules": [{ "source": "CLOB", "target": "TEXT" }] },
                       "existing": { "a": "missing_only" } });
    for path in ["/api/migrations/preview", "/api/migrations"] {
        let (st, v) = call(&app, "POST", path, Some(plan.clone())).await;
        assert_eq!(st, StatusCode::NOT_FOUND, "{path} {v}");
    }
    // 没有完成的迁移：一开始为空；不存在的记录继续、放弃都报 404
    let (st, v) = call(&app, "GET", "/api/migrations", None).await;
    assert_eq!((st, v), (StatusCode::OK, json!([])));
    let (st, _) = call(&app, "POST", "/api/migrations/abc123/resume", None).await;
    assert_eq!(st, StatusCode::NOT_FOUND);
    let (st, _) = call(&app, "DELETE", "/api/migrations/abc123", None).await;
    assert_eq!(st, StatusCode::NOT_FOUND);
}
