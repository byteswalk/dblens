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
use dblens_dialect::{DdlKind, Direction};
use dblens_driver::{ConnectParams, DriverId};
use dblens_service::{
    DataOrder, DefaultPolicy, DriverRegistry, Environment, ExecRequest, ExportFormat, ExportOptions,
    ExportRequest, ExportSource, ExportState, ExportStatus, GenerateKind, ObjectKind, ProfileInput,
    SecretBackend, Secrets, Service, SessionEvent, SessionId, Store,
};

struct Live {
    _dir: tempfile::TempDir,
    service: Arc<Service>,
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
    let service = Arc::new(Service::with_parts(store, secrets, registry, Arc::new(DefaultPolicy)));
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

/// 对象操作（M1c2）：在测试 schema 中建父子表、视图、序列、函数，查看 DDL、生成 SQL、取外键关系，最后删除。
async fn objects_flow(env: &str) {
    let Some(live) = setup(env).await else { return };
    let s = &live.service;
    let profile = s.profile(&live.profile_id).unwrap();
    let info = s.open_session(&live.profile_id).await.unwrap();
    let id = info.id;
    let upper = matches!(live.driver, DriverId::Oracle | DriverId::Dameng);
    let n = |name: &str| if upper { name.to_uppercase() } else { name.to_string() };
    let drops: &[&str] = match live.driver {
        DriverId::Oracle => &[
            "BEGIN EXECUTE IMMEDIATE 'DROP VIEW c2_v'; EXCEPTION WHEN OTHERS THEN NULL; END;",
            "BEGIN EXECUTE IMMEDIATE 'DROP TABLE c2_child'; EXCEPTION WHEN OTHERS THEN NULL; END;",
            "BEGIN EXECUTE IMMEDIATE 'DROP TABLE c2_parent'; EXCEPTION WHEN OTHERS THEN NULL; END;",
            "BEGIN EXECUTE IMMEDIATE 'DROP SEQUENCE c2_seq'; EXCEPTION WHEN OTHERS THEN NULL; END;",
            "BEGIN EXECUTE IMMEDIATE 'DROP FUNCTION c2_f'; EXCEPTION WHEN OTHERS THEN NULL; END;",
        ],
        DriverId::Mysql => {
            &["DROP VIEW IF EXISTS c2_v", "DROP TABLE IF EXISTS c2_child", "DROP TABLE IF EXISTS c2_parent"]
        }
        _ => &[
            "DROP VIEW IF EXISTS c2_v",
            "DROP TABLE IF EXISTS c2_child",
            "DROP TABLE IF EXISTS c2_parent",
            "DROP SEQUENCE IF EXISTS c2_seq",
            "DROP FUNCTION IF EXISTS c2_f",
        ],
    };
    for sql in drops {
        live.submit(id, ExecRequest::new(*sql)).await;
    }
    let creates: Vec<&str> = match live.driver {
        DriverId::Mysql => vec![
            "CREATE TABLE c2_parent (id INT PRIMARY KEY, code VARCHAR(10) NOT NULL UNIQUE COMMENT '编码') COMMENT='父表'",
            "CREATE TABLE c2_child (id INT PRIMARY KEY, parent_id INT, qty INT, CONSTRAINT c2_fk FOREIGN KEY (parent_id) REFERENCES c2_parent (id), INDEX c2_idx (qty))",
            "CREATE VIEW c2_v AS SELECT id, code FROM c2_parent",
        ],
        DriverId::Postgres => vec![
            "CREATE TABLE c2_parent (id INT PRIMARY KEY, code VARCHAR(10) NOT NULL UNIQUE)",
            "COMMENT ON TABLE c2_parent IS '父表'",
            "COMMENT ON COLUMN c2_parent.code IS '编码'",
            "CREATE TABLE c2_child (id INT GENERATED BY DEFAULT AS IDENTITY PRIMARY KEY, parent_id INT, qty INT CHECK (qty > 0), CONSTRAINT c2_fk FOREIGN KEY (parent_id) REFERENCES c2_parent (id) ON DELETE CASCADE)",
            "CREATE INDEX c2_idx ON c2_child (qty DESC)",
            "CREATE VIEW c2_v AS SELECT id, code FROM c2_parent",
            "CREATE SEQUENCE c2_seq START 10 INCREMENT 2",
            "CREATE FUNCTION c2_f() RETURNS int LANGUAGE sql AS 'SELECT 1'",
        ],
        _ => vec![
            "CREATE TABLE c2_parent (id INT PRIMARY KEY, code VARCHAR(10) NOT NULL UNIQUE)",
            "COMMENT ON TABLE c2_parent IS '父表'",
            "CREATE TABLE c2_child (id INT PRIMARY KEY, parent_id INT, qty INT CHECK (qty > 0), CONSTRAINT c2_fk FOREIGN KEY (parent_id) REFERENCES c2_parent (id))",
            "CREATE INDEX c2_idx ON c2_child (qty)",
            "CREATE VIEW c2_v AS SELECT id, code FROM c2_parent",
            "CREATE SEQUENCE c2_seq START WITH 10 INCREMENT BY 2",
            "CREATE OR REPLACE FUNCTION c2_f RETURN INT IS BEGIN RETURN 1; END;",
        ],
    };
    for sql in creates {
        live.run(id, sql).await;
    }
    let schema = live.schema.as_str();
    let ddl = |kind: DdlKind, name: String| {
        let profile = &profile;
        async move { s.metadata.ddl(profile, schema, kind, &name).await }
    };

    let child = ddl(DdlKind::Table, n("c2_child")).await.unwrap();
    let low = child.to_lowercase();
    assert!(
        low.contains("c2_fk") && low.contains("foreign key") && low.contains("c2_parent"),
        "{env}：{child}"
    );
    assert!(low.contains("c2_idx") || live.driver == DriverId::Dameng, "{env}：索引 {child}");
    let parent = ddl(DdlKind::Table, n("c2_parent")).await.unwrap();
    assert!(parent.contains("父表"), "{env}：表注释 {parent}");
    let view = ddl(DdlKind::View, n("c2_v")).await.unwrap();
    assert!(view.to_lowercase().contains("c2_parent"), "{env}：{view}");
    if live.driver != DriverId::Mysql {
        let seq = ddl(DdlKind::Sequence, n("c2_seq")).await.unwrap();
        assert!(seq.to_lowercase().contains("c2_seq"), "{env}：{seq}");
        let f = ddl(DdlKind::Function, n("c2_f")).await.unwrap();
        assert!(f.to_lowercase().contains("c2_f"), "{env}：{f}");
    } else {
        assert!(matches!(ddl(DdlKind::Synonym, n("c2_v")).await, Err(Error::Invalid { .. })));
    }
    assert!(ddl(DdlKind::Table, n("c2_no_such")).await.is_err(), "{env}：不存在的表");

    // 生成 SQL
    let child_name = n("c2_child");
    let generate =
        |kind: GenerateKind, object_kind: ObjectKind, object: String, reference: Option<String>| {
            let profile = &profile;
            let schema = schema.to_string();
            async move {
                let r = reference.as_deref().map(|t| (schema.as_str(), t));
                s.metadata.generate(profile, &schema, &object, object_kind, kind, r).await
            }
        };
    let select = generate(GenerateKind::Select, ObjectKind::Table, child_name.clone(), None).await.unwrap();
    assert!(select.to_lowercase().contains("parent_id"), "{select}");
    let update = generate(GenerateKind::Update, ObjectKind::Table, child_name.clone(), None).await.unwrap();
    assert!(update.starts_with("UPDATE"), "{update}");
    let join = generate(GenerateKind::Join, ObjectKind::Table, child_name.clone(), Some(n("c2_parent")))
        .await
        .unwrap();
    assert!(join.contains("JOIN") && join.contains(" t2 ON "), "{join}");
    let r = generate(GenerateKind::Insert, ObjectKind::View, n("c2_v"), None).await;
    assert!(matches!(r, Err(Error::Invalid { .. })), "视图只能生成 SELECT");
    // 生成的 SELECT 能执行
    live.run(id, &select).await;

    let rels = s.metadata.relations(&profile, schema, &n("c2_parent")).await.unwrap();
    assert!(
        rels.iter().any(|r| r.direction == Direction::In && r.table.eq_ignore_ascii_case("c2_child")),
        "{env}：{rels:?}"
    );

    // 数据标签的 SQL 能执行
    let order = DataOrder { column: n("qty"), desc: true };
    let data =
        s.metadata.data_sql(&profile, schema, &child_name, Some("1 = 1"), Some(&order), false).await.unwrap();
    live.run(id, &data).await;
    let count = s.metadata.data_sql(&profile, schema, &child_name, None, None, true).await.unwrap();
    live.run(id, &count).await;

    for sql in drops {
        live.submit(id, ExecRequest::new(*sql)).await;
    }
    s.sessions.close(id, false).await.unwrap();
}

#[tokio::test]
async fn mysql_objects() {
    objects_flow("DBLENS_TEST_MYSQL_URL").await;
}

#[tokio::test]
async fn mariadb_objects() {
    objects_flow("DBLENS_TEST_MARIADB_URL").await;
}

#[tokio::test]
async fn postgres_objects() {
    objects_flow("DBLENS_TEST_PG_URL").await;
}

#[tokio::test]
async fn oracle_objects() {
    objects_flow("DBLENS_TEST_ORACLE_URL").await;
}

#[tokio::test]
async fn dameng_objects() {
    objects_flow("DBLENS_TEST_DM_URL").await;
}

/// 运维查询（M1c3）：逐个执行。`allowed_failures` 是测试库上已知会失败的（如没有安装 pg_stat_statements）。
async fn ops_flow(env: &str, allowed_failures: &[&str]) {
    let Some(live) = setup(env).await else { return };
    let s = &live.service;
    let id = s.open_session(&live.profile_id).await.unwrap().id;
    let mut failed = Vec::new();
    for q in s.ops(&live.profile_id).unwrap() {
        let events = live.submit(id, ExecRequest::new(q.sql.clone())).await;
        if let Some(SessionEvent::Failed { error, .. }) =
            events.iter().find(|e| matches!(e, SessionEvent::Failed { .. }))
        {
            eprintln!("{env} {}：{error}", q.key);
            failed.push(q.key.as_str());
        } else {
            assert!(events.iter().any(|e| matches!(e, SessionEvent::ResultStart { .. })), "{env} {}", q.key);
        }
    }
    assert!(failed.iter().all(|k| allowed_failures.contains(k)), "{env}：{failed:?}");
    s.sessions.close(id, false).await.unwrap();
}

#[tokio::test]
async fn mysql_ops() {
    ops_flow("DBLENS_TEST_MYSQL_URL", &[]).await;
}

#[tokio::test]
async fn mariadb_ops() {
    ops_flow("DBLENS_TEST_MARIADB_URL", &[]).await;
}

#[tokio::test]
async fn postgres_ops() {
    // 测试库没有安装 pg_stat_statements 扩展
    ops_flow("DBLENS_TEST_PG_URL", &["slow_sql"]).await;
}

#[tokio::test]
async fn oracle_ops() {
    ops_flow("DBLENS_TEST_ORACLE_URL", &[]).await;
}

#[tokio::test]
async fn dameng_ops() {
    ops_flow("DBLENS_TEST_DM_URL", &[]).await;
}

/// 导出（M1c3）：一行含中文和超过 64 KB 的大字段，四种格式都写出完整内容；另导出多行核对行数。
async fn export_flow(env: &str) {
    let Some(live) = setup(env).await else { return };
    let s = &live.service;
    let dir = tempfile::tempdir().unwrap();
    let (big, rows_sql, rows) = match live.driver {
        DriverId::Mysql => (
            "SELECT 1 AS id, '中文' AS name, REPEAT('数', 100000) AS txt, CHAR_LENGTH(REPEAT('数', 100000)) AS len",
            "WITH RECURSIVE t (n) AS (SELECT 1 UNION ALL SELECT n + 1 FROM t WHERE n < 990) SELECT n FROM t",
            990,
        ),
        DriverId::Postgres => (
            "SELECT 1 AS id, '中文' AS name, repeat('数', 100000) AS txt, 100000 AS len",
            "SELECT g AS n FROM generate_series(1, 2500) AS g",
            2500,
        ),
        DriverId::Oracle | DriverId::Dameng => (
            "SELECT 1 AS \"id\", '中文' AS \"name\", RPAD(TO_CLOB('数'), 100000, '数') AS \"txt\", \
             LENGTH(RPAD(TO_CLOB('数'), 100000, '数')) AS \"len\" FROM DUAL",
            "SELECT LEVEL AS \"n\" FROM DUAL CONNECT BY LEVEL <= 2500",
            2500,
        ),
    };
    let export = |sql: &str, format: ExportFormat, name: &str| {
        let path = dir.path().join(name);
        let req = ExportRequest {
            profile_id: live.profile_id.clone(),
            source: ExportSource::Sql(sql.to_string()),
            format,
            options: ExportOptions { bom: false, table: "c3_t".into(), ..Default::default() },
            path: Some(path.clone()),
            file_name: name.to_string(),
        };
        async move {
            let id = s.start_export(req, true).await.unwrap();
            let st = loop {
                let st = s.exports.status(&id).unwrap();
                if st.state != ExportState::Running {
                    break st;
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            };
            (st, path)
        }
    };
    let done = |st: &ExportStatus| assert_eq!(st.state, ExportState::Done, "{env}：{st:?}");

    // CSV：数据库报告的字符数与写出的一致
    let (st, path) = export(big, ExportFormat::Csv, "big.csv").await;
    done(&st);
    assert_eq!((st.rows, st.warnings.lob_truncated), (1, 0), "{env}");
    let mut csv = csv::Reader::from_path(&path).unwrap();
    let record = csv.records().next().unwrap().unwrap();
    let len: usize = record[3].parse().unwrap();
    assert!(len * 3 > 64 * 1024, "{env}：大字段要超过 64 KB，实际 {len} 个字符");
    assert_eq!((&record[1], record[2].chars().count()), ("中文", len), "{env}");

    // JSON
    let (st, path) = export(big, ExportFormat::Json, "big.json").await;
    done(&st);
    let v: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(v[0]["txt"].as_str().unwrap().chars().count(), len, "{env}");

    // INSERT：完整的大字段写在一条语句里
    let (st, path) = export(big, ExportFormat::Insert, "big.sql").await;
    done(&st);
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.starts_with("INSERT INTO c3_t ("), "{env}：{}", &text[..80]);
    assert_eq!(text.matches('数').count(), len, "{env}");

    // xlsx：单元格上限 32767 个字符，截断并报警告
    let (st, path) = export(big, ExportFormat::Xlsx, "big.xlsx").await;
    done(&st);
    assert_eq!(st.warnings.cell_truncated, 1, "{env}");
    assert_eq!(&std::fs::read(&path).unwrap()[..2], b"PK");

    // 多行
    let (st, path) = export(rows_sql, ExportFormat::Json, "rows.json").await;
    done(&st);
    let v: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!((st.rows, v.as_array().unwrap().len()), (rows, rows as usize), "{env}");
}

#[tokio::test]
async fn mysql_export() {
    export_flow("DBLENS_TEST_MYSQL_URL").await;
}

#[tokio::test]
async fn mariadb_export() {
    export_flow("DBLENS_TEST_MARIADB_URL").await;
}

#[tokio::test]
async fn postgres_export() {
    export_flow("DBLENS_TEST_PG_URL").await;
}

#[tokio::test]
async fn oracle_export() {
    export_flow("DBLENS_TEST_ORACLE_URL").await;
}

#[tokio::test]
async fn dameng_export() {
    export_flow("DBLENS_TEST_DM_URL").await;
}
