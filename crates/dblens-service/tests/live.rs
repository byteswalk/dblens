//! 服务层的实库测试。设置对应的环境变量时运行，否则直接通过：
//! DBLENS_TEST_MYSQL_URL、DBLENS_TEST_MARIADB_URL、DBLENS_TEST_PG_URL、DBLENS_TEST_ORACLE_URL、DBLENS_TEST_DM_URL。
//!
//! 走一遍完整流程：保存连接配置（密码放本地密码库）→ 测试连接 → 打开会话执行 → 浏览元数据 →
//! 会话中执行 DDL 后元数据缓存失效 → 取消长查询；查询工作台用到的执行计划、出错位置、切换 schema。

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use dblens_core::{Error, Value};
use dblens_dialect::meta::MetaKey;
use dblens_driver::{ConnectParams, DriverId};
use dblens_service::{
    DefaultPolicy, DriverRegistry, Environment, ExecRequest, ProfileInput, SecretBackend, Secrets, Service,
    SessionEvent, SessionId, Store,
};

struct Live {
    _dir: tempfile::TempDir,
    service: Service,
    profile_id: String,
    driver: DriverId,
    /// 测试表所在的 schema。
    schema: String,
}

const LIB_OPTIONS: [&str; 2] = ["oracle_client_lib_dir", "dpi_lib_dir"];

async fn setup(env: &str) -> Option<Live> {
    let url = std::env::var(env).ok()?;
    let (driver, params) = ConnectParams::from_url(&url).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(Store::open(dir.path()).unwrap());
    // 测试不写系统钥匙串
    let secrets = Arc::new(Secrets::with_backend(store.clone(), SecretBackend::Vault));
    secrets.init_vault("测试主密码").unwrap();
    let registry = Arc::new(DriverRegistry::new(store.clone()));
    for option in LIB_OPTIONS {
        if let Some(lib) = params.options.get(option) {
            registry.set_lib_dir(driver, Some(Path::new(lib))).unwrap();
        }
    }
    let service = Service::with_parts(store, secrets, registry, Arc::new(DefaultPolicy));
    let mut options = params.options.clone();
    options.retain(|k, _| !LIB_OPTIONS.contains(&k.as_str()));
    let schema = match driver {
        DriverId::Mysql => params.database.clone().unwrap(),
        DriverId::Postgres => "public".to_string(),
        DriverId::Oracle | DriverId::Dameng => params.user.to_uppercase(),
    };
    let input = ProfileInput {
        name: env.to_string(),
        driver,
        host: params.host.clone(),
        port: params.port,
        user: params.user.clone(),
        database: params.database.clone(),
        options,
        environment: Environment::Test,
        color: None,
        connect_timeout_secs: 10,
    };
    let summary = service.test_connection(input.clone(), &params.password).await.unwrap();
    eprintln!("{env}：{} {:?} {:?}", summary.variant_name.current(), summary.version_code, summary.version);
    let profile = service.create_profile(input, Some(&params.password)).unwrap();
    Some(Live { _dir: dir, service, profile_id: profile.id, driver, schema })
}

impl Live {
    fn dual(&self, expr: &str) -> String {
        match self.driver {
            DriverId::Oracle | DriverId::Dameng => format!("SELECT {expr} FROM DUAL"),
            DriverId::Mysql | DriverId::Postgres => format!("SELECT {expr}"),
        }
    }

    /// 提交请求并取完全部事件，不检查失败。
    async fn submit(&self, id: SessionId, req: ExecRequest) -> Vec<SessionEvent> {
        let sql = req.sql.clone();
        self.service.sessions.submit(id, req).await.unwrap();
        let mut events = Vec::new();
        while let Some(ev) = self.service.sessions.next(id, 500).await.unwrap() {
            events.push(ev);
        }
        assert_eq!(events.last(), Some(&SessionEvent::Done), "{sql}");
        events
    }

    /// 执行并取完全部事件。
    async fn run(&self, id: SessionId, sql: &str) -> Vec<SessionEvent> {
        let events = self.submit(id, ExecRequest::new(sql)).await;
        if let Some(SessionEvent::Failed { error, .. }) =
            events.iter().find(|e| matches!(e, SessionEvent::Failed { .. }))
        {
            panic!("{sql} 执行失败：{error}");
        }
        events
    }
}

fn first_value(events: &[SessionEvent]) -> Value {
    events
        .iter()
        .find_map(|e| if let SessionEvent::Rows { rows } = e { Some(rows[0][0].clone()) } else { None })
        .expect("没有返回行")
}

async fn full_flow(env: &str) {
    let Some(live) = setup(env).await else { return };
    let s = &live.service;
    let profile = s.profile(&live.profile_id).unwrap();

    // 会话：执行查询，结果按批取回
    let info = s.open_session(&live.profile_id).await.unwrap();
    let id = info.id;
    let v = first_value(&live.run(id, &live.dual("1 + 1")).await);
    assert!(matches!(&v, Value::Int(2) | Value::Decimal(_)), "{v:?}");

    // 元数据：schema 列表与表列表（之后会被缓存）
    let schemas = s.metadata.schemas(&profile, None).await.unwrap();
    assert!(schemas.iter().any(|x| x.name.eq_ignore_ascii_case(&live.schema)), "{schemas:?}");
    let table = match live.driver {
        DriverId::Oracle | DriverId::Dameng => "DBLENS_SVC_T".to_string(),
        _ => "dblens_svc_t".to_string(),
    };
    let qualified = format!("{}.{}", live.schema, table);
    let drop = format!("DROP TABLE {qualified}");
    // 上次中断留下的表
    s.sessions.execute(id, &drop, false).await.unwrap();
    while s.sessions.next(id, 10).await.unwrap().is_some() {}
    let before = s.metadata.objects(&profile, &live.schema, MetaKey::Tables, None).await.unwrap();
    assert!(!before.iter().any(|o| o.name == table));

    // 会话中执行 DDL：带 schema 的对象只让这个 schema 的缓存失效，再次浏览能看到新表
    live.run(id, &format!("CREATE TABLE {qualified} (id INT PRIMARY KEY, name VARCHAR(20))")).await;
    let after = s.metadata.objects(&profile, &live.schema, MetaKey::Tables, None).await.unwrap();
    assert!(after.iter().any(|o| o.name == table), "DDL 后缓存应已失效");
    let detail = s.metadata.tables(&profile, &live.schema, Some(&table)).await.unwrap();
    assert_eq!(detail[0].columns.len(), 2);
    assert!(detail[0].primary_key.is_some());

    // 手动提交：修改后有未提交的事务，回滚后数据不在
    s.sessions.set_autocommit(id, false).await.unwrap();
    live.run(id, &format!("INSERT INTO {qualified} (id, name) VALUES (1, 'a')")).await;
    assert!(s.sessions.info(id).unwrap().in_transaction);
    s.sessions.rollback(id).await.unwrap();
    s.sessions.set_autocommit(id, true).await.unwrap();
    let n = first_value(&live.run(id, &format!("SELECT COUNT(*) FROM {qualified}")).await);
    assert!(matches!(&n, Value::Int(0)) || n == Value::Decimal("0".into()), "{n:?}");

    live.run(id, &drop).await;
    s.sessions.close(id, false).await.unwrap();
    s.delete_profile(&live.profile_id).await.unwrap();
}

/// 查询工作台（M1b）：当前 schema、执行计划、出错位置。
async fn workbench_flow(env: &str) {
    let Some(live) = setup(env).await else { return };
    let s = &live.service;
    let info = s.open_session(&live.profile_id).await.unwrap();
    let id = info.id;
    let schema = info.schema.expect("应读到当前 schema");
    assert!(schema.eq_ignore_ascii_case(&live.schema), "{schema}");

    // 切换 schema 再切回
    let other = match live.driver {
        DriverId::Mysql => "information_schema",
        DriverId::Postgres => "pg_catalog",
        DriverId::Oracle | DriverId::Dameng => "SYS",
    };
    s.sessions.set_schema(id, other).await.unwrap();
    assert_eq!(s.sessions.info(id).unwrap().schema.as_deref(), Some(other));
    s.sessions.set_schema(id, &schema).await.unwrap();
    assert_eq!(s.sessions.info(id).unwrap().schema, Some(schema.clone()));

    // 执行计划：最后有结果集，不报错
    let sql = format!(
        "{};
{}",
        live.dual("1"),
        live.dual("2")
    );
    let cursor = Some(sql.rfind("SELECT").unwrap());
    let events =
        live.submit(id, ExecRequest { explain: true, cursor, ..ExecRequest::new(sql.clone()) }).await;
    assert!(!events.iter().any(|e| matches!(e, SessionEvent::Failed { .. })), "{events:?}");
    let rows: usize =
        events.iter().map(|e| if let SessionEvent::Rows { rows } = e { rows.len() } else { 0 }).sum();
    assert!(rows > 0, "执行计划没有返回行：{events:?}");
    eprintln!("{env} 执行计划：{events:?}");

    // 出错位置：中文之后的语法错误，换算后指向 FROM（PG 直接给位置；Oracle 按字节给；MySQL 系、达梦从错误信息中找）
    let bad = "SELECT '数洞', FROM t";
    let events = live.submit(id, ExecRequest::new(bad)).await;
    let Some(SessionEvent::Failed { error, offset, .. }) =
        events.iter().find(|e| matches!(e, SessionEvent::Failed { .. }))
    else {
        panic!("{events:?}")
    };
    eprintln!("{env} 出错位置：{offset:?} {error}");
    let o = offset.expect("应有出错位置");
    assert!(bad[o..].starts_with("FROM"), "位置 {o} 指向 {:?}", &bad[o..]);
    s.sessions.close(id, false).await.unwrap();
}

#[tokio::test]
async fn mysql_workbench() {
    workbench_flow("DBLENS_TEST_MYSQL_URL").await;
}

#[tokio::test]
async fn mariadb_workbench() {
    workbench_flow("DBLENS_TEST_MARIADB_URL").await;
}

#[tokio::test]
async fn postgres_workbench() {
    workbench_flow("DBLENS_TEST_PG_URL").await;
}

#[tokio::test]
async fn oracle_workbench() {
    workbench_flow("DBLENS_TEST_ORACLE_URL").await;
}

#[tokio::test]
async fn dameng_workbench() {
    workbench_flow("DBLENS_TEST_DM_URL").await;
}

/// 会话忙碌时取消长查询，会话之后照常可用。
async fn cancel_flow(env: &str, slow: &str) {
    let Some(live) = setup(env).await else { return };
    let s = Arc::new(live.service);
    let id = s.open_session(&live.profile_id).await.unwrap().id;
    s.sessions.execute(id, slow, false).await.unwrap();
    assert!(matches!(s.sessions.next(id, 10).await.unwrap(), Some(SessionEvent::StatementStart { .. })));
    // 后台一直取事件（列信息可能先于结果到达），主任务稍后取消
    let fetch = {
        let s = s.clone();
        tokio::spawn(async move {
            let mut events = Vec::new();
            while let Some(ev) = s.sessions.next(id, 10).await.unwrap() {
                events.push(ev);
            }
            events
        })
    };
    tokio::time::sleep(Duration::from_millis(500)).await;
    s.sessions.cancel(id).await.unwrap();
    let events =
        tokio::time::timeout(Duration::from_secs(8), fetch).await.expect("取消后 8 秒内应结束").unwrap();
    assert!(
        events.iter().any(|e| matches!(e, SessionEvent::Failed { error: Error::Cancelled, .. })),
        "{events:?}"
    );
    s.sessions.execute(id, "SELECT 1", false).await.unwrap();
    let mut ok = false;
    while let Some(ev) = s.sessions.next(id, 10).await.unwrap() {
        ok |= matches!(ev, SessionEvent::Rows { .. });
    }
    assert!(ok);
    s.sessions.close(id, false).await.unwrap();
}

#[tokio::test]
async fn mysql_flow() {
    full_flow("DBLENS_TEST_MYSQL_URL").await;
}

#[tokio::test]
async fn mariadb_flow() {
    full_flow("DBLENS_TEST_MARIADB_URL").await;
}

#[tokio::test]
async fn postgres_flow() {
    full_flow("DBLENS_TEST_PG_URL").await;
}

#[tokio::test]
async fn oracle_flow() {
    full_flow("DBLENS_TEST_ORACLE_URL").await;
}

#[tokio::test]
async fn dameng_flow() {
    full_flow("DBLENS_TEST_DM_URL").await;
}

#[tokio::test]
async fn postgres_cancel() {
    cancel_flow("DBLENS_TEST_PG_URL", "SELECT pg_sleep(20)").await;
}

#[tokio::test]
async fn mysql_cancel() {
    // SLEEP() 被 KILL QUERY 打断时返回 1 而不报错，所以用一个真正耗时的查询（与驱动一致性测试相同）
    let slow = "SELECT COUNT(*) FROM information_schema.columns a, information_schema.columns b, information_schema.columns c";
    cancel_flow("DBLENS_TEST_MYSQL_URL", slow).await;
}
