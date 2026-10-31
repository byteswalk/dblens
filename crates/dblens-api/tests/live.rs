//! 接口的实库测试。设置对应的环境变量时运行，否则直接通过：
//! DBLENS_TEST_MYSQL_URL、DBLENS_TEST_MARIADB_URL、DBLENS_TEST_PG_URL、DBLENS_TEST_ORACLE_URL、DBLENS_TEST_DM_URL；
//! 国产库第一批：DBLENS_TEST_TIDB_URL、DBLENS_TEST_OCEANBASE_URL、DBLENS_TEST_OPENGAUSS_URL、
//! DBLENS_TEST_OPENGAUSS_A_URL、DBLENS_TEST_KINGBASE_URL、DBLENS_TEST_KINGBASE_PG_URL；DBLENS_TEST_SQLSERVER_URL。
//!
//! 通过 REST 保存连接配置（MySQL、MariaDB、PG 的测试库开了 TLS，要求走 TLS）、测试连接、浏览元数据、打开会话，
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
const LIB_OPTIONS: [&str; 2] = ["oracle_client_lib_dir", "jdbc_dir"];

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
    // 参数解析失败时 axum 返回纯文本
    let body = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes)
            .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&bytes).into()))
    };
    (status, body)
}

/// `tls`：测试库开了 TLS，连接时要求走 TLS（国产库的测试库没开）。
async fn flow(env: &str, tls: bool) {
    let Ok(url) = std::env::var(env) else { return };
    let (driver, params) = ConnectParams::from_url(&url).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(Store::open(dir.path()).unwrap());
    let secrets = Arc::new(Secrets::with_backend(store.clone(), SecretBackend::Vault));
    secrets.init_vault("主密码").unwrap();
    let registry = Arc::new(DriverRegistry::new(store.clone()));
    let service = Arc::new(Service::with_parts(store, secrets, registry, Arc::new(DefaultPolicy)));
    let app = router(service, ApiConfig { token: TOKEN.into(), allowed_origins: vec![], local_files: false });

    // 达梦的驱动目录是连接选项，不是全局设置
    let lib_options = if driver == DriverId::Dameng { [].as_slice() } else { LIB_OPTIONS.as_slice() };
    for option in lib_options {
        if let Some(lib) = params.options.get(*option) {
            let path = format!(
                "/api/drivers/{}/lib-dir",
                match driver {
                    DriverId::Oracle => "oracle",
                    _ => "informix",
                }
            );
            let (st, s) = call(&app, "PUT", &path, Some(json!({"path": lib}))).await;
            assert_eq!((st, s["available"].clone()), (StatusCode::OK, json!(true)), "{s}");
        }
    }
    let mut options: serde_json::Map<String, Value> = params
        .options
        .iter()
        .filter(|(k, _)| !lib_options.contains(&k.as_str()))
        .map(|(k, v)| (k.clone(), json!(v)))
        .collect();
    if tls {
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

    // 对象操作（M1c2）：取数和拼装在服务层的实库测试中覆盖，这里只看路由、参数和状态码
    let schema = s["schema"].as_str().unwrap();
    let base = format!("/api/profiles/{id}/schemas/{schema}");
    let (st, d) = call(
        &app,
        "POST",
        &format!("{base}/data-sql"),
        Some(json!({
            "object": "c2_x", "filter": " ", "order": {"column": "id", "desc": true},
        })),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{d}");
    let sql = d["sql"].as_str().unwrap().to_lowercase();
    assert!(sql.contains("c2_x") && sql.contains("desc") && !sql.contains("where"), "{sql}");
    let (st, d) =
        call(&app, "POST", &format!("{base}/data-sql"), Some(json!({"object": "c2_x", "count": true}))).await;
    assert!(st == StatusCode::OK && d["sql"].as_str().unwrap().to_lowercase().contains("count(*)"), "{d}");
    // 不存在的表：PG 系由结构模型拼装时 404；其余由数据库报错（422）
    let (st, e) = call(&app, "GET", &format!("{base}/ddl?kind=table&name=c2_no_such"), None).await;
    assert!(st.is_client_error(), "{st} {e}");
    let (st, e) = call(&app, "GET", &format!("{base}/relations?table=c2_no_such"), None).await;
    assert_eq!(st, StatusCode::NOT_FOUND, "{e}");
    let (st, e) =
        call(&app, "GET", &format!("{base}/generate?object=c2_v&object_kind=view&kind=insert"), None).await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "{e}");
    let (st, _) = call(&app, "GET", &format!("{base}/ddl?kind=bogus&name=x"), None).await;
    assert_eq!(st, StatusCode::BAD_REQUEST);

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
        DriverId::SqlServer => {
            "SELECT TOP (900) ROW_NUMBER() OVER (ORDER BY (SELECT NULL)) AS n \
             FROM sys.all_columns a CROSS JOIN sys.all_columns b ORDER BY n"
        }
        DriverId::Informix => {
            "SELECT FIRST 900 ROW_NUMBER() OVER (ORDER BY a.tabid, b.tabid) AS n              FROM systables a, systables b ORDER BY n"
        }
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
async fn gbase8s() {
    flow("DBLENS_TEST_GBASE_URL", false).await;
}

#[tokio::test]
async fn sqlserver() {
    flow("DBLENS_TEST_SQLSERVER_URL", true).await;
}

/// SQL Server 的数据库层：库列表；按库取架构和对象（`catalog` 参数）；M6b 之前的功能返回 400。
#[tokio::test]
async fn sqlserver_catalogs() {
    let Ok(url) = std::env::var("DBLENS_TEST_SQLSERVER_URL") else { return };
    let (driver, params) = ConnectParams::from_url(&url).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(Store::open(dir.path()).unwrap());
    let secrets = Arc::new(Secrets::with_backend(store.clone(), SecretBackend::Vault));
    secrets.init_vault("主密码").unwrap();
    let registry = Arc::new(DriverRegistry::new(store.clone()));
    let service = Arc::new(Service::with_parts(store, secrets, registry, Arc::new(DefaultPolicy)));
    let app = router(service, ApiConfig { token: TOKEN.into(), allowed_origins: vec![], local_files: false });
    let profile = json!({
        "name": "mssql", "driver": driver, "host": params.host, "port": params.port, "user": params.user,
        "database": params.database, "options": params.options, "environment": "test",
    });
    let (st, p) =
        call(&app, "POST", "/api/profiles", Some(json!({"profile": profile, "password": params.password})))
            .await;
    assert_eq!(st, StatusCode::CREATED, "{p}");
    let id = p["id"].as_str().unwrap();
    let (st, cats) = call(&app, "GET", &format!("/api/profiles/{id}/catalogs"), None).await;
    assert_eq!(st, StatusCode::OK, "{cats}");
    assert!(
        cats.as_array().unwrap().iter().any(|c| c["name"] == "master" && c["is_system"] == true),
        "{cats}"
    );
    let (st, schemas) = call(&app, "GET", &format!("/api/profiles/{id}/schemas?catalog=master"), None).await;
    assert_eq!(st, StatusCode::OK, "{schemas}");
    assert!(schemas.as_array().unwrap().iter().any(|s| s["name"] == "dbo"), "{schemas}");
    let (st, t) =
        call(&app, "GET", &format!("/api/profiles/{id}/schemas/sys/objects?catalog=msdb&kind=views"), None)
            .await;
    assert_eq!(st, StatusCode::OK, "{t}");
    // M6b 起表设计器可用：按库读不存在的表是 404
    let (st, e) = call(
        &app,
        "GET",
        &format!("/api/profiles/{id}/design/table?catalog=master&schema=dbo&name=dblens_no_such"),
        None,
    )
    .await;
    assert_eq!(st, StatusCode::NOT_FOUND, "{e}");
}

#[tokio::test]
async fn mysql() {
    flow("DBLENS_TEST_MYSQL_URL", true).await;
}

#[tokio::test]
async fn mariadb() {
    flow("DBLENS_TEST_MARIADB_URL", true).await;
}

#[tokio::test]
async fn postgres() {
    flow("DBLENS_TEST_PG_URL", true).await;
}

#[tokio::test]
async fn oracle() {
    flow("DBLENS_TEST_ORACLE_URL", false).await;
}

#[tokio::test]
async fn dameng() {
    flow("DBLENS_TEST_DM_URL", false).await;
}

#[tokio::test]
async fn tidb() {
    flow("DBLENS_TEST_TIDB_URL", false).await;
}

#[tokio::test]
async fn oceanbase() {
    flow("DBLENS_TEST_OCEANBASE_URL", false).await;
}

#[tokio::test]
async fn opengauss() {
    flow("DBLENS_TEST_OPENGAUSS_URL", false).await;
}

#[tokio::test]
async fn opengauss_a() {
    flow("DBLENS_TEST_OPENGAUSS_A_URL", false).await;
}

#[tokio::test]
async fn kingbase() {
    flow("DBLENS_TEST_KINGBASE_URL", false).await;
}

#[tokio::test]
async fn kingbase_pg() {
    flow("DBLENS_TEST_KINGBASE_PG_URL", false).await;
}
