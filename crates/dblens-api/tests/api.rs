//! 接口测试：服务层换成假驱动（见 `dblens_service::fake`），不需要真实数据库。

use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use dblens_api::{ApiConfig, router};
use dblens_service::fake::{FakeDriver, Log};
use dblens_service::{DefaultPolicy, DriverRegistry, SecretBackend, Secrets, Service, Store};
use futures::{SinkExt, StreamExt};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tokio_tungstenite::tungstenite::Message;
use tower::ServiceExt;

const TOKEN: &str = "test-token";

fn app() -> (Router, Arc<Service>) {
    let store = Arc::new(Store::open_in_memory().unwrap());
    let secrets = Arc::new(Secrets::with_backend(store.clone(), SecretBackend::Vault));
    secrets.init_vault("master").unwrap();
    let log: Log = Arc::default();
    let registry = Arc::new(DriverRegistry::with_drivers(store.clone(), vec![Box::new(FakeDriver { log })]));
    let service = Arc::new(Service::with_parts(store, secrets, registry, Arc::new(DefaultPolicy)));
    let config = ApiConfig {
        token: TOKEN.into(),
        allowed_origins: vec!["tauri://localhost".into()],
        local_files: false,
    };
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

// ---- WebSocket ----

type Ws = tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// 启动真实的服务，打开一个会话并连上它的 WebSocket。
async fn ws_session() -> (Ws, Arc<Service>, u64) {
    ws_open(dblens_service::Environment::Dev, "").await
}

/// 同上，可指定连接的环境和 WebSocket 地址上附加的查询参数（如 `&lang=en`）。
async fn ws_open(env: dblens_service::Environment, query: &str) -> (Ws, Arc<Service>, u64) {
    let (app, service) = app();
    let profile =
        service.create_profile(serde_json::from_value(profile_json("p")).unwrap(), Some("pw")).unwrap();
    let mut input = profile.input.clone();
    input.environment = env;
    service.update_profile(&profile.id, input).await.unwrap();
    let sid = service.open_session(&profile.id).await.unwrap().id;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { dblens_api::serve(listener, app).await.unwrap() });
    let url = format!("ws://{addr}/api/sessions/{sid}/ws?token={TOKEN}{query}");
    let (ws, _) = tokio_tungstenite::connect_async(url).await.unwrap();
    (ws, service, sid)
}

async fn send(ws: &mut Ws, v: Value) {
    ws.send(Message::Text(v.to_string().into())).await.unwrap();
}

async fn recv(ws: &mut Ws) -> Value {
    loop {
        let msg = tokio::time::timeout(Duration::from_secs(5), ws.next())
            .await
            .expect("5 秒内没有消息")
            .unwrap()
            .unwrap();
        if let Message::Text(t) = msg {
            return serde_json::from_str(&t).unwrap();
        }
    }
}

async fn quiet(ws: &mut Ws) -> bool {
    tokio::time::timeout(Duration::from_millis(300), ws.next()).await.is_err()
}

#[tokio::test]
async fn rows_wait_for_fetch_more() {
    let (mut ws, _, _) = ws_session().await;
    send(&mut ws, json!({"type": "exec", "sql": "ROWS 5", "max_rows": 2})).await;
    assert_eq!(recv(&mut ws).await["type"], "statement_start");
    assert_eq!(recv(&mut ws).await["type"], "result_start");
    let rows = recv(&mut ws).await;
    assert_eq!((rows["type"].clone(), rows["rows"].as_array().unwrap().len()), (json!("rows"), 2));
    assert!(quiet(&mut ws).await, "没有 fetch_more 时不再推送");
    send(&mut ws, json!({"type": "fetch_more", "max_rows": 2})).await;
    assert_eq!(recv(&mut ws).await["rows"].as_array().unwrap().len(), 2);
    send(&mut ws, json!({"type": "fetch_more", "max_rows": 10})).await;
    assert_eq!(recv(&mut ws).await["rows"].as_array().unwrap().len(), 1);
    // 不满一批：结果集已取完，后面的事件不用再 fetch_more
    let rest: Vec<Value> = vec![recv(&mut ws).await, recv(&mut ws).await, recv(&mut ws).await];
    assert_eq!(
        rest.iter().map(|v| v["type"].as_str().unwrap()).collect::<Vec<_>>(),
        ["result_end", "statement_end", "done"]
    );
}

#[tokio::test]
async fn offsets_are_utf16_and_errors_are_reported() {
    let (mut ws, _, _) = ws_session().await;
    let sql = "INSERT INTO t VALUES ('数洞😀'); ROWS 0";
    send(&mut ws, json!({"type": "exec", "sql": sql})).await;
    let mut starts = Vec::new();
    loop {
        let v = recv(&mut ws).await;
        if v["type"] == "statement_start" {
            starts.push(v["start"].as_u64().unwrap() as usize);
        }
        if v["type"] == "done" {
            break;
        }
    }
    let second_bytes = sql.find("ROWS").unwrap();
    assert_eq!(starts[1], sql[..second_bytes].encode_utf16().count());
    assert_ne!(starts[1], second_bytes);

    send(&mut ws, json!({"type": "nonsense"})).await;
    let e = recv(&mut ws).await;
    assert_eq!((e["type"].clone(), e["error"]["kind"].clone()), (json!("error"), json!("invalid")));
}

#[tokio::test]
async fn cancel_over_websocket() {
    let (mut ws, _, _) = ws_session().await;
    send(&mut ws, json!({"type": "exec", "sql": "SLEEP"})).await;
    assert_eq!(recv(&mut ws).await["type"], "statement_start");
    tokio::time::sleep(Duration::from_millis(100)).await;
    send(&mut ws, json!({"type": "cancel"})).await;
    let failed = recv(&mut ws).await;
    assert_eq!(
        (failed["type"].clone(), failed["error"]["kind"].clone()),
        (json!("failed"), json!("cancelled"))
    );
    assert_eq!(recv(&mut ws).await["type"], "done");
}

#[tokio::test]
async fn cancel_while_waiting_for_fetch_more_discards_the_rest() {
    let (mut ws, _, _) = ws_session().await;
    send(&mut ws, json!({"type": "exec", "sql": "ROWS 10", "max_rows": 2})).await;
    for _ in 0..3 {
        recv(&mut ws).await;
    }
    assert!(quiet(&mut ws).await, "在等 fetch_more");
    send(&mut ws, json!({"type": "cancel"})).await;
    let mut types = Vec::new();
    loop {
        let v = recv(&mut ws).await;
        types.push(v["type"].as_str().unwrap().to_string());
        if v["type"] == "done" {
            break;
        }
    }
    assert!(!types.iter().any(|t| t == "rows"), "剩下的行被丢掉：{types:?}");
    // 会话照常可用
    send(&mut ws, json!({"type": "exec", "sql": "ROWS 1"})).await;
    assert_eq!(recv(&mut ws).await["type"], "statement_start");
}

#[tokio::test]
async fn disconnect_mid_result_frees_the_session() {
    let (mut ws, service, sid) = ws_session().await;
    send(&mut ws, json!({"type": "exec", "sql": "ROWS 10", "max_rows": 1})).await;
    for _ in 0..3 {
        recv(&mut ws).await;
    }
    drop(ws);
    for _ in 0..50 {
        if !service.sessions.info(sid).unwrap().busy {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("断开后会话应恢复空闲");
}

#[tokio::test]
async fn websocket_requires_token() {
    let (app, service) = app();
    let profile =
        service.create_profile(serde_json::from_value(profile_json("p")).unwrap(), Some("pw")).unwrap();
    let sid = service.open_session(&profile.id).await.unwrap().id;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { dblens_api::serve(listener, app).await.unwrap() });
    assert!(tokio_tungstenite::connect_async(format!("ws://{addr}/api/sessions/{sid}/ws")).await.is_err());
    assert!(
        tokio_tungstenite::connect_async(format!("ws://{addr}/api/sessions/999/ws?token={TOKEN}"))
            .await
            .is_err()
    );
}

/// 取到 done 为止的全部事件。
async fn until_done(ws: &mut Ws) -> Vec<Value> {
    let mut out = Vec::new();
    loop {
        let v = recv(ws).await;
        let done = v["type"] == "done";
        out.push(v);
        if done {
            return out;
        }
    }
}

#[tokio::test]
async fn exec_options_over_websocket() {
    let (mut ws, _, _) = ws_session().await;
    let utf16 = |s: &str| s.encode_utf16().count();

    // 出错继续：失败的语句带 UTF-16 的出错位置，后面的语句照常执行
    let sql = "SELECT '数洞😀'; FAILAT 3; ROWS 0";
    send(&mut ws, json!({"type": "exec", "sql": sql, "continue_on_error": true})).await;
    let events = until_done(&mut ws).await;
    let failed = events.iter().find(|e| e["type"] == "failed").unwrap();
    let at = sql.find("FAILAT").unwrap() + 2;
    assert_eq!(failed["offset"], json!(utf16(&sql[..at])));
    let starts: Vec<&Value> = events.iter().filter(|e| e["type"] == "statement_start").collect();
    assert_eq!(starts.iter().map(|e| e["total"].clone()).collect::<Vec<_>>(), [json!(3), json!(3), json!(3)]);

    // 光标：UTF-16 偏移，只执行所在的那条
    let cursor = utf16(&sql[..sql.find("ROWS").unwrap()]) + 1;
    send(&mut ws, json!({"type": "exec", "sql": sql, "cursor": cursor})).await;
    let events = until_done(&mut ws).await;
    let starts: Vec<&Value> = events.iter().filter(|e| e["type"] == "statement_start").collect();
    assert_eq!((starts.len(), starts[0]["total"].clone()), (1, json!(1)));
    assert_eq!(starts[0]["start"], json!(utf16(&sql[..sql.find("ROWS").unwrap()])));

    // 执行计划：多条语句时报错，会话不受影响
    send(&mut ws, json!({"type": "exec", "sql": sql, "explain": true})).await;
    let e = recv(&mut ws).await;
    assert_eq!((e["type"].clone(), e["error"]["kind"].clone()), (json!("error"), json!("invalid")));
    send(&mut ws, json!({"type": "exec", "sql": "ROWS 1", "explain": true})).await;
    let events = until_done(&mut ws).await;
    assert_eq!(events[0]["type"], "statement_start");
    assert_eq!(events[0]["info"]["keyword"], "EXPLAIN");
}

#[tokio::test]
async fn schema_over_rest() {
    let (app, _) = app();
    let (_, p) =
        call(&app, "POST", "/api/profiles", Some(json!({"profile": profile_json("p"), "password": "pw"})))
            .await;
    let (_, s) = call(&app, "POST", "/api/sessions", Some(json!({"profile_id": p["id"]}))).await;
    assert_eq!(s["schema"], "public", "打开会话时读取当前 schema");
    let sid = s["id"].as_u64().unwrap();
    let (st, s) =
        call(&app, "POST", &format!("/api/sessions/{sid}/schema"), Some(json!({"schema": "hr"}))).await;
    assert_eq!((st, s["schema"].clone()), (StatusCode::OK, json!("hr")));
}

// ---- 语言 ----

/// 带 `Accept-Language` 的 GET 请求；`token` 为 false 时不带令牌。
async fn get_lang(app: &Router, uri: &str, lang: &str, token: bool) -> Value {
    let mut req = Request::builder().uri(uri).header(header::ACCEPT_LANGUAGE, lang);
    if token {
        req = req.header(header::AUTHORIZATION, format!("Bearer {TOKEN}"));
    }
    let resp = app.clone().oneshot(req.body(Body::empty()).unwrap()).await.unwrap();
    serde_json::from_slice(&resp.into_body().collect().await.unwrap().to_bytes()).unwrap()
}

#[tokio::test]
async fn errors_follow_accept_language() {
    let (app, _) = app();
    let zh = get_lang(&app, "/api/profiles/x", "zh-CN,zh;q=0.9,en;q=0.8", true).await;
    assert_eq!((zh["kind"].clone(), zh["what"].clone()), (json!("not_found"), json!("连接配置 x")));
    let en = get_lang(&app, "/api/profiles/x", "en-US,en;q=0.9", true).await;
    assert_eq!(en["what"], "connection profile x");
    // 令牌检查在语言协商之内：令牌错误也按请求语言
    let en = get_lang(&app, "/api/profiles", "en", false).await;
    assert_eq!(
        (en["kind"].clone(), en["message"].clone()),
        (json!("auth"), json!("Missing or wrong access token"))
    );
}

#[tokio::test]
async fn websocket_events_follow_lang_param() {
    for (query, want) in [("&lang=en", "Production: DROP statement"), ("&lang=zh-CN", "生产环境：DROP 语句")]
    {
        let (mut ws, _, _) = ws_open(dblens_service::Environment::Prod, query).await;
        send(&mut ws, json!({"type": "exec", "sql": "DROP TABLE t"})).await;
        let events = until_done(&mut ws).await;
        let confirm = events.iter().find(|e| e["type"] == "confirm_required").expect("应要求确认");
        assert_eq!(confirm["reason"], want);
    }
}

// ---- 导出与运维查询（M1c3） ----

#[tokio::test]
async fn export_then_download() {
    let (app, service) = app();
    let dir = tempfile::tempdir().unwrap();
    service.exports.set_dir(dir.path().join("exports"));
    let (st, p) =
        call(&app, "POST", "/api/profiles", Some(json!({"profile": profile_json("p"), "password": "pw"})))
            .await;
    assert_eq!(st, StatusCode::CREATED);
    let pid = p["id"].as_str().unwrap();
    let body = |sql: &str, path: Option<&str>| {
        json!({"profile_id": pid, "source": {"sql": sql}, "format": "csv", "options": {"bom": false},
               "path": path, "file_name": "订单.csv"})
    };
    // 不是查询、浏览器模式给了路径：400
    let (st, e) = call(&app, "POST", "/api/exports", Some(body("DELETE FROM t", None))).await;
    assert_eq!((st, e["kind"].clone()), (StatusCode::BAD_REQUEST, json!("invalid")));
    let abs = dir.path().join("x.csv");
    let (st, _) = call(&app, "POST", "/api/exports", Some(body("SELECT ROWS 3", abs.to_str()))).await;
    assert_eq!(st, StatusCode::BAD_REQUEST);

    let (st, created) = call(&app, "POST", "/api/exports", Some(body("SELECT ROWS 3", None))).await;
    assert_eq!(st, StatusCode::CREATED, "{created}");
    let id = created["id"].as_str().unwrap().to_string();
    let status = loop {
        let (_, s) = call(&app, "GET", &format!("/api/exports/{id}"), None).await;
        if s["state"] != "running" {
            break s;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    };
    assert_eq!((status["state"].clone(), status["rows"].clone()), (json!("done"), json!(3)), "{status}");
    // 浏览器的下载链接用查询参数带令牌
    let req = Request::get(format!("/api/exports/{id}/file?token={TOKEN}")).body(Body::empty()).unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let cd = resp.headers()[header::CONTENT_DISPOSITION].to_str().unwrap().to_string();
    assert!(cd.ends_with("filename*=UTF-8''%E8%AE%A2%E5%8D%95.csv"), "{cd}");
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(&bytes[..], b"n\n1\n2\n3\n");
    assert_eq!(std::fs::read_dir(dir.path().join("exports")).unwrap().count(), 0, "下载后删除临时文件");
    let (st, _) = call(&app, "GET", &format!("/api/exports/{id}"), None).await;
    assert_eq!(st, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn ops_follow_language() {
    let (app, _) = app();
    let (_, p) =
        call(&app, "POST", "/api/profiles", Some(json!({"profile": profile_json("p"), "password": "pw"})))
            .await;
    let uri = format!("/api/profiles/{}/ops", p["id"].as_str().unwrap());
    let get = |lang: &'static str| {
        let req = Request::get(&uri)
            .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
            .header(header::ACCEPT_LANGUAGE, lang)
            .body(Body::empty())
            .unwrap();
        let app = app.clone();
        async move {
            let resp = app.oneshot(req).await.unwrap();
            assert_eq!(resp.status(), StatusCode::OK);
            serde_json::from_slice::<Value>(&resp.into_body().collect().await.unwrap().to_bytes()).unwrap()
        }
    };
    let zh = get("zh-CN").await;
    assert_eq!(
        (zh[0]["key"].clone(), zh[0]["name"].clone(), zh[0]["category"].clone()),
        (json!("sessions"), json!("会话列表"), json!("session"))
    );
    assert!(zh[0]["sql"].as_str().unwrap().contains("pg_stat_activity"), "没连接过时按驱动选 PG 的模板");
    assert_eq!(get("en").await[0]["name"], "Sessions");
}
