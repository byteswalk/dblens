//! 接口的实库测试。设置对应的环境变量时运行，否则直接通过：
//! DBLENS_TEST_MYSQL_URL、DBLENS_TEST_MARIADB_URL、DBLENS_TEST_PG_URL、DBLENS_TEST_ORACLE_URL、DBLENS_TEST_DM_URL。
//!
//! 通过 REST 保存连接配置（MySQL 系和 PG 走 TLS）、测试连接、浏览元数据、打开会话，
//! 再通过 WebSocket 分批取回 900 行结果。

use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use dblens_api::{ApiConfig, router};
use dblens_driver::{ConnectParams, DriverId};
use dblens_service::{DefaultPolicy, DriverRegistry, SecretBackend, Secrets, Service, Store};
use futures::{SinkExt, StreamExt};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tokio_tungstenite::tungstenite::Message;
use tower::ServiceExt;

const TOKEN: &str = "live-token";
const LIB_OPTIONS: [&str; 2] = ["oracle_client_lib_dir", "dpi_lib_dir"];

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
    (status, if bytes.is_empty() { Value::Null } else { serde_json::from_slice(&bytes).unwrap() })
}

async fn flow(env: &str) {
    let Ok(url) = std::env::var(env) else { return };
    let (driver, params) = ConnectParams::from_url(&url).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(Store::open(dir.path()).unwrap());
    let secrets = Arc::new(Secrets::with_backend(store.clone(), SecretBackend::Vault));
    secrets.init_vault("主密码").unwrap();
    let registry = Arc::new(DriverRegistry::new(store.clone()));
    let service = Arc::new(Service::with_parts(store, secrets, registry, Arc::new(DefaultPolicy)));
    let app = router(service, ApiConfig { token: TOKEN.into(), allowed_origins: vec![] });

    for option in LIB_OPTIONS {
        if let Some(lib) = params.options.get(option) {
            let path = format!(
                "/api/drivers/{}/lib-dir",
                if driver == DriverId::Oracle { "oracle" } else { "dameng" }
            );
            let (st, s) = call(&app, "PUT", &path, Some(json!({"path": lib}))).await;
            assert_eq!((st, s["available"].clone()), (StatusCode::OK, json!(true)), "{s}");
        }
    }
    let mut options: serde_json::Map<String, Value> = params
        .options
        .iter()
        .filter(|(k, _)| !LIB_OPTIONS.contains(&k.as_str()))
        .map(|(k, v)| (k.clone(), json!(v)))
        .collect();
    if matches!(driver, DriverId::Mysql | DriverId::Postgres) {
        options.insert("tls".into(), json!("require"));
    }
    let profile = json!({
        "name": env, "driver": driver, "host": params.host, "port": params.port, "user": params.user,
        "database": params.database, "options": options, "environment": "test",
    });
    let (st, summary) = call(
        &app,
        "POST",
        "/api/profiles/test",
        Some(json!({"profile": profile, "password": params.password})),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{summary}");
    eprintln!("{env}：{}", summary["variant_name"]);
    let (st, p) =
        call(&app, "POST", "/api/profiles", Some(json!({"profile": profile, "password": params.password})))
            .await;
    assert_eq!(st, StatusCode::CREATED, "{p}");
    let id = p["id"].as_str().unwrap().to_string();

    let (st, schemas) = call(&app, "GET", &format!("/api/profiles/{id}/schemas"), None).await;
    assert_eq!(st, StatusCode::OK, "{schemas}");
    assert!(schemas.as_array().unwrap().iter().any(|s| s["is_system"] == true));

    let (st, s) = call(&app, "POST", "/api/sessions", Some(json!({"profile_id": id}))).await;
    assert_eq!(st, StatusCode::CREATED, "{s}");
    let sid = s["id"].as_u64().unwrap();

    // WebSocket：900 行，每批 400 行
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(dblens_api::serve(listener, app.clone()));
    let (mut ws, _) =
        tokio_tungstenite::connect_async(format!("ws://{addr}/api/sessions/{sid}/ws?token={TOKEN}"))
            .await
            .unwrap();
    let sql = match driver {
        DriverId::Postgres => "SELECT g FROM generate_series(1, 900) AS g ORDER BY g",
        DriverId::Mysql => {
            "WITH RECURSIVE t (n) AS (SELECT 1 UNION ALL SELECT n + 1 FROM t WHERE n < 900) SELECT n FROM t ORDER BY n"
        }
        DriverId::Oracle | DriverId::Dameng => "SELECT LEVEL FROM DUAL CONNECT BY LEVEL <= 900 ORDER BY 1",
    };
    ws.send(Message::Text(json!({"type": "exec", "sql": sql, "max_rows": 400}).to_string().into()))
        .await
        .unwrap();
    let mut batches = Vec::new();
    loop {
        let msg = tokio::time::timeout(Duration::from_secs(20), ws.next()).await.unwrap().unwrap().unwrap();
        let Message::Text(t) = msg else { continue };
        let v: Value = serde_json::from_str(&t).unwrap();
        match v["type"].as_str().unwrap() {
            "rows" => {
                let n = v["rows"].as_array().unwrap().len();
                batches.push(n);
                if n == 400 {
                    ws.send(Message::Text(json!({"type": "fetch_more", "max_rows": 400}).to_string().into()))
                        .await
                        .unwrap();
                }
            }
            "failed" | "error" => panic!("{v}"),
            "done" => break,
            _ => {}
        }
    }
    assert_eq!(batches, [400, 400, 100], "{env}");
    drop(ws);
    server.abort();

    let (st, _) = call(&app, "DELETE", &format!("/api/sessions/{sid}"), None).await;
    assert_eq!(st, StatusCode::NO_CONTENT);
    let (st, _) = call(&app, "DELETE", &format!("/api/profiles/{id}"), None).await;
    assert_eq!(st, StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn mysql() {
    flow("DBLENS_TEST_MYSQL_URL").await;
}

#[tokio::test]
async fn mariadb() {
    flow("DBLENS_TEST_MARIADB_URL").await;
}

#[tokio::test]
async fn postgres() {
    flow("DBLENS_TEST_PG_URL").await;
}

#[tokio::test]
async fn oracle() {
    flow("DBLENS_TEST_ORACLE_URL").await;
}

#[tokio::test]
async fn dameng() {
    flow("DBLENS_TEST_DM_URL").await;
}
