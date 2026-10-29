//! 结构迁移（M5a）的实库测试：源库和目标库的环境变量都设置时运行，否则直接通过（变量见 `live.rs`）。
//!
//! 在源库建一套测试结构（自增主键、默认值、注释、唯一约束、检查约束、索引、外键），迁到目标库，
//! 读回目标结构，与迁移草稿中的目标结构比对应无差异；最后清理两边。表名带源库的代号，不同路线互不干扰。
//! 人大金仓开发版最多 10 个连接，与其他实库测试一样要串行运行（`--test-threads=1`）。

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use dblens_core::LogicalType;
use dblens_core::schema::Table;
use dblens_dialect::{Dialect, Variant};
use dblens_driver::{ConnectParams, DriverId};
use dblens_service::{
    DefaultPolicy, DriverRegistry, Environment, ExecRequest, MigrationEnd, MigrationPlan, ProfileInput,
    SecretBackend, Secrets, Service, SessionEvent, Store, TaskState, TaskStatus,
};

const LIB_OPTIONS: [&str; 2] = ["oracle_client_lib_dir", "dpi_lib_dir"];

struct End {
    profile: String,
    driver: DriverId,
    variant: Variant,
    schema: String,
}

/// 一个服务中建两个连接配置；任一环境变量没有设置时返回 None。
async fn setup(envs: [&str; 2]) -> Option<(Arc<Service>, Vec<End>, tempfile::TempDir)> {
    let urls: Vec<String> = envs.iter().map(|e| std::env::var(e).ok()).collect::<Option<_>>()?;
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(Store::open(dir.path()).unwrap());
    let secrets = Arc::new(Secrets::with_backend(store.clone(), SecretBackend::Vault));
    secrets.init_vault("测试主密码").unwrap();
    let registry = Arc::new(DriverRegistry::new(store.clone()));
    let service = Arc::new(Service::with_parts(store, secrets, registry.clone(), Arc::new(DefaultPolicy)));
    let mut ends = Vec::new();
    for (env, url) in envs.iter().zip(urls) {
        let (driver, params) = ConnectParams::from_url(&url).unwrap();
        for option in LIB_OPTIONS {
            if let Some(lib) = params.options.get(option) {
                registry.set_lib_dir(driver, Some(Path::new(lib))).unwrap();
            }
        }
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
            product: None,
        };
        let summary = service.test_connection(input.clone(), &params.password).await.unwrap();
        let profile = service.create_profile(input, Some(&params.password)).unwrap();
        ends.push(End { profile: profile.id, driver, variant: summary.variant, schema });
    }
    Some((service, ends, dir))
}

/// 在一端执行若干语句；`tolerate` 为 true 时忽略失败（清理用）。
async fn run(s: &Service, end: &End, sqls: &[String], tolerate: bool) {
    let id = s.open_session(&end.profile).await.unwrap().id;
    for sql in sqls {
        s.sessions.submit(id, ExecRequest::new(sql)).await.unwrap();
        while let Some(ev) = s.sessions.next(id, 500).await.unwrap() {
            if let SessionEvent::Failed { error, .. } = &ev {
                assert!(tolerate, "{sql} 执行失败：{error}");
            }
        }
    }
    s.sessions.close(id, true).await.unwrap();
}

/// 删除测试表（子表在前）。
fn drops(end: &End, names: &[String]) -> Vec<String> {
    names
        .iter()
        .map(|n| match end.driver {
            DriverId::Oracle => {
                format!("BEGIN EXECUTE IMMEDIATE 'DROP TABLE \"{n}\" CASCADE CONSTRAINTS'; EXCEPTION WHEN OTHERS THEN NULL; END;")
            }
            DriverId::Dameng => format!("DROP TABLE IF EXISTS \"{n}\" CASCADE"),
            DriverId::Postgres => format!("DROP TABLE IF EXISTS \"{n}\" CASCADE"),
            DriverId::Mysql => format!("DROP TABLE IF EXISTS `{n}`"),
        })
        .collect()
}

/// 源库的测试结构：`p` 为表名前缀（Oracle、达梦用大写）。
fn source_ddl(end: &End, p: &str) -> Vec<String> {
    let opengauss = end.variant == Variant::OpenGauss;
    match end.driver {
        DriverId::Mysql => vec![
            format!(
                "CREATE TABLE {p}_cust (id BIGINT AUTO_INCREMENT PRIMARY KEY, name VARCHAR(50) NOT NULL DEFAULT 'n/a' COMMENT 'name', \
                 active TINYINT(1) DEFAULT 1, created DATETIME DEFAULT CURRENT_TIMESTAMP, amount DECIMAL(12,2) DEFAULT 0, note TEXT, \
                 CONSTRAINT {p}_ck_amount CHECK (amount >= 0), CONSTRAINT {p}_uk_name UNIQUE (name)) COMMENT = 'customers'"
            ),
            format!("CREATE INDEX {p}_ix_created ON {p}_cust (created)"),
            format!(
                "CREATE TABLE {p}_ord (id INT PRIMARY KEY, cust_id BIGINT, qty INT, status VARCHAR(10) DEFAULT 'new', \
                 CONSTRAINT {p}_ck_qty CHECK (qty > 0), \
                 CONSTRAINT {p}_fk_cust FOREIGN KEY (cust_id) REFERENCES {p}_cust (id) ON DELETE CASCADE)"
            ),
        ],
        DriverId::Postgres => vec![
            format!(
                "CREATE TABLE {p}_cust (id {}, name VARCHAR(50) NOT NULL DEFAULT 'n/a', \
                 active BOOLEAN DEFAULT true, created TIMESTAMP DEFAULT CURRENT_TIMESTAMP, amount NUMERIC(12,2) DEFAULT 0, note TEXT, \
                 CONSTRAINT {p}_ck_amount CHECK (amount >= 0), CONSTRAINT {p}_uk_name UNIQUE (name))",
                if opengauss {
                    "BIGSERIAL PRIMARY KEY"
                } else {
                    "BIGINT GENERATED BY DEFAULT AS IDENTITY PRIMARY KEY"
                }
            ),
            format!("COMMENT ON TABLE {p}_cust IS 'customers'"),
            format!("COMMENT ON COLUMN {p}_cust.name IS 'name'"),
            format!("CREATE INDEX {p}_ix_created ON {p}_cust (created)"),
            format!(
                "CREATE TABLE {p}_ord (id INT PRIMARY KEY, cust_id BIGINT, qty INT, status VARCHAR(10) DEFAULT 'new', \
                 CONSTRAINT {p}_ck_qty CHECK (qty > 0), \
                 CONSTRAINT {p}_fk_cust FOREIGN KEY (cust_id) REFERENCES {p}_cust (id) ON DELETE CASCADE)"
            ),
        ],
        DriverId::Oracle | DriverId::Dameng => {
            // 达梦的外键要求两边类型相同
            let (id, created, cust_id) = if end.driver == DriverId::Oracle {
                ("NUMBER(19) GENERATED BY DEFAULT AS IDENTITY", "DATE DEFAULT SYSDATE", "NUMBER(19)")
            } else {
                ("BIGINT IDENTITY(1, 1)", "TIMESTAMP DEFAULT SYSDATE", "BIGINT")
            };
            vec![
                format!(
                    "CREATE TABLE {p}_CUST (ID {id} PRIMARY KEY, NAME VARCHAR2(50) DEFAULT 'n/a' NOT NULL, \
                     ACTIVE NUMBER(1) DEFAULT 1, CREATED {created}, AMOUNT NUMBER(12,2) DEFAULT 0, NOTE CLOB, \
                     CONSTRAINT {p}_CK_AMOUNT CHECK (AMOUNT >= 0), CONSTRAINT {p}_UK_NAME UNIQUE (NAME))"
                ),
                format!("COMMENT ON TABLE {p}_CUST IS 'customers'"),
                format!("COMMENT ON COLUMN {p}_CUST.NAME IS 'name'"),
                format!("CREATE INDEX {p}_IX_CREATED ON {p}_CUST (CREATED)"),
                format!(
                    "CREATE TABLE {p}_ORD (ID NUMBER(10) PRIMARY KEY, CUST_ID {cust_id}, QTY NUMBER(10), STATUS VARCHAR2(10) DEFAULT 'new', \
                     CONSTRAINT {p}_CK_QTY CHECK (QTY > 0), \
                     CONSTRAINT {p}_FK_CUST FOREIGN KEY (CUST_ID) REFERENCES {p}_CUST (ID) ON DELETE CASCADE)"
                ),
            ]
        }
    }
}

/// 源库中的表名（Oracle、达梦为大写）。
fn names(end: &End, p: &str) -> Vec<String> {
    let up = matches!(end.driver, DriverId::Oracle | DriverId::Dameng);
    ["ord", "cust"]
        .iter()
        .map(|t| if up { format!("{p}_{t}").to_uppercase() } else { format!("{p}_{t}") })
        .collect()
}

async fn wait(s: &Service, id: &str) -> TaskStatus {
    for _ in 0..1200 {
        let st = s.tasks.status(id).unwrap();
        if !matches!(st.state, TaskState::Running | TaskState::Confirm) {
            return st;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("任务没有结束");
}

/// 源 → 目标迁移一次并核对。`p`：表名前缀（小写）。
async fn migrate_flow(src_env: &str, tgt_env: &str, p: &str) {
    let Some((s, ends, _dir)) = setup([src_env, tgt_env]).await else { return };
    let (src, tgt) = (&ends[0], &ends[1]);
    let route = format!("{src_env} → {tgt_env}");
    let up = matches!(src.driver, DriverId::Oracle | DriverId::Dameng);
    let prefix = if up { p.to_uppercase() } else { p.to_string() };
    let src_names = names(src, &prefix);
    let mut tgt_names = names(tgt, p);
    // MySQL 区分表名大小写：清理时大小写两种都删
    if tgt.driver == DriverId::Mysql {
        tgt_names.extend(names(tgt, p).iter().map(|n| n.to_uppercase()).collect::<Vec<_>>());
    }
    run(&s, src, &drops(src, &src_names), true).await;
    run(&s, tgt, &drops(tgt, &tgt_names), true).await;
    run(&s, src, &source_ddl(src, &prefix), false).await;

    let plan = MigrationPlan {
        source: MigrationEnd { profile: src.profile.clone(), schema: src.schema.clone() },
        tables: Some(src_names.iter().rev().cloned().collect()),
        sequences: Some(vec![]),
        target: MigrationEnd { profile: tgt.profile.clone(), schema: tgt.schema.clone() },
        settings: Default::default(),
        existing: BTreeMap::new(),
    };
    let preview = s.preview_migration(&plan).await.unwrap();
    let notes: Vec<String> = preview
        .draft
        .notes
        .iter()
        .map(|n| format!("{:?} {}.{:?} {:?}: {}", n.level, n.object, n.column, n.item, n.message.current()))
        .collect();
    eprintln!("{route} 提示：\n{}\n脚本：\n{}", notes.join("\n"), preview.text);
    assert!(preview.problems.is_empty(), "{route}：{:?}", preview.problems);
    assert_eq!(preview.draft.tables.len(), 2, "{route}");
    let id = s.start_migration(plan).await.unwrap();
    let st = wait(&s, &id).await;
    let failures: Vec<String> = st.failures.iter().map(|f| format!("{}\n  {}", f.text, f.error)).collect();
    assert_eq!(st.state, TaskState::Done, "{route}：{st:?}");
    assert!(failures.is_empty(), "{route} 失败的语句：\n{}", failures.join("\n"));
    let (report, _) = s.tasks.report(&id).unwrap();
    assert!(std::fs::read_to_string(report).unwrap().contains("```sql"), "{route}");

    // 读回目标结构，与草稿中的目标结构比对
    let tp = s.profile(&tgt.profile).unwrap();
    let dialect: Dialect = s.metadata.dialect(&tp).await.unwrap();
    let mut actual = Vec::new();
    for t in &preview.draft.tables {
        let mut got: Vec<Table> = s.metadata.tables(&tp, &tgt.schema, Some(&t.target.name)).await.unwrap();
        assert_eq!(got.len(), 1, "{route}：{} 没有建成", t.target.name);
        actual.push(Table { triggers: vec![], ..got.remove(0) });
    }
    for (t, got) in preview.draft.tables.iter().zip(&actual) {
        let (want, have) = (shape(&dialect, &t.target, &t.target), shape(&dialect, got, &t.target));
        assert_eq!(want, have, "{route}：{} 读回后与草稿不同", t.target.name);
    }

    run(&s, tgt, &drops(tgt, &tgt_names), true).await;
    run(&s, src, &drops(src, &src_names), true).await;
}

/// 表结构的要点（核对用）：列（名字、逻辑类型、可空、有无默认值、自增、注释）、主键、唯一约束、
/// 草稿中有的索引（库为外键自动建的索引不算）、外键、检查约束的个数、表注释。默认值和表达式的写法会被库规整，不比文本。
fn shape(d: &Dialect, t: &Table, draft: &Table) -> Vec<String> {
    let lower = |v: &[String]| v.iter().map(|x| x.to_lowercase()).collect::<Vec<_>>().join(",");
    let mut out = vec![format!("comment {:?}", t.comment)];
    for c in &t.columns {
        // NUMBER(p) 与 NUMBER(p,0) 相同
        let logical = match d.logical_type(&c.data_type.native) {
            LogicalType::Decimal { precision: Some(p), scale: None } => {
                LogicalType::Decimal { precision: Some(p), scale: Some(0) }
            }
            l => l,
        };
        out.push(format!(
            "col {} {:?} null={} default={} auto={} comment={:?}",
            c.name.to_lowercase(),
            logical,
            c.nullable,
            c.default.is_some() && !c.auto_increment,
            c.auto_increment,
            c.comment
        ));
    }
    out.push(format!("pk {:?}", t.primary_key.as_ref().map(|k| lower(&k.columns))));
    let mut uniques: Vec<String> = t.uniques.iter().map(|u| lower(&u.columns)).collect();
    uniques.sort();
    out.push(format!("unique {uniques:?}"));
    let wanted: Vec<String> = draft
        .indexes
        .iter()
        .map(|i| lower(&i.columns.iter().map(|c| c.name.clone()).collect::<Vec<_>>()))
        .collect();
    let mut indexes: Vec<String> = t
        .indexes
        .iter()
        .map(|i| lower(&i.columns.iter().map(|c| c.name.clone()).collect::<Vec<_>>()))
        .filter(|i| wanted.contains(i))
        .collect();
    indexes.sort();
    out.push(format!("index {indexes:?}"));
    let mut fks: Vec<String> = t
        .foreign_keys
        .iter()
        .map(|f| {
            format!(
                "{} -> {}({}) {:?}",
                lower(&f.columns),
                f.ref_table.to_lowercase(),
                lower(&f.ref_columns),
                f.on_delete
            )
        })
        .collect();
    fks.sort();
    out.push(format!("fk {fks:?}"));
    out.push(format!("checks {}", t.checks.len()));
    out
}

/// 每个源库迁到 MySQL、PG、Oracle、达梦（不迁到自己），另加信创常见路线。
macro_rules! routes {
    ($($name:ident: $src:literal => $tgt:literal, $p:literal;)*) => {
        $(
            #[tokio::test]
            async fn $name() {
                migrate_flow($src, $tgt, $p).await;
            }
        )*
    };
}

routes! {
    mysql_to_pg: "DBLENS_TEST_MYSQL_URL" => "DBLENS_TEST_PG_URL", "m5my";
    mysql_to_oracle: "DBLENS_TEST_MYSQL_URL" => "DBLENS_TEST_ORACLE_URL", "m5my";
    mysql_to_dameng: "DBLENS_TEST_MYSQL_URL" => "DBLENS_TEST_DM_URL", "m5my";
    mysql_to_tidb: "DBLENS_TEST_MYSQL_URL" => "DBLENS_TEST_TIDB_URL", "m5my";
    mysql_to_oceanbase: "DBLENS_TEST_MYSQL_URL" => "DBLENS_TEST_OCEANBASE_URL", "m5my";
    mariadb_to_mysql: "DBLENS_TEST_MARIADB_URL" => "DBLENS_TEST_MYSQL_URL", "m5ma";
    mariadb_to_pg: "DBLENS_TEST_MARIADB_URL" => "DBLENS_TEST_PG_URL", "m5ma";
    mariadb_to_oracle: "DBLENS_TEST_MARIADB_URL" => "DBLENS_TEST_ORACLE_URL", "m5ma";
    mariadb_to_dameng: "DBLENS_TEST_MARIADB_URL" => "DBLENS_TEST_DM_URL", "m5ma";
    pg_to_mysql: "DBLENS_TEST_PG_URL" => "DBLENS_TEST_MYSQL_URL", "m5pg";
    pg_to_oracle: "DBLENS_TEST_PG_URL" => "DBLENS_TEST_ORACLE_URL", "m5pg";
    pg_to_dameng: "DBLENS_TEST_PG_URL" => "DBLENS_TEST_DM_URL", "m5pg";
    oracle_to_mysql: "DBLENS_TEST_ORACLE_URL" => "DBLENS_TEST_MYSQL_URL", "m5or";
    oracle_to_pg: "DBLENS_TEST_ORACLE_URL" => "DBLENS_TEST_PG_URL", "m5or";
    oracle_to_dameng: "DBLENS_TEST_ORACLE_URL" => "DBLENS_TEST_DM_URL", "m5or";
    oracle_to_kingbase: "DBLENS_TEST_ORACLE_URL" => "DBLENS_TEST_KINGBASE_URL", "m5or";
    oracle_to_opengauss: "DBLENS_TEST_ORACLE_URL" => "DBLENS_TEST_OPENGAUSS_A_URL", "m5or";
    dameng_to_mysql: "DBLENS_TEST_DM_URL" => "DBLENS_TEST_MYSQL_URL", "m5dm";
    dameng_to_pg: "DBLENS_TEST_DM_URL" => "DBLENS_TEST_PG_URL", "m5dm";
    dameng_to_oracle: "DBLENS_TEST_DM_URL" => "DBLENS_TEST_ORACLE_URL", "m5dm";
    tidb_to_mysql: "DBLENS_TEST_TIDB_URL" => "DBLENS_TEST_MYSQL_URL", "m5ti";
    tidb_to_pg: "DBLENS_TEST_TIDB_URL" => "DBLENS_TEST_PG_URL", "m5ti";
    tidb_to_oracle: "DBLENS_TEST_TIDB_URL" => "DBLENS_TEST_ORACLE_URL", "m5ti";
    tidb_to_dameng: "DBLENS_TEST_TIDB_URL" => "DBLENS_TEST_DM_URL", "m5ti";
    oceanbase_to_mysql: "DBLENS_TEST_OCEANBASE_URL" => "DBLENS_TEST_MYSQL_URL", "m5ob";
    oceanbase_to_pg: "DBLENS_TEST_OCEANBASE_URL" => "DBLENS_TEST_PG_URL", "m5ob";
    oceanbase_to_oracle: "DBLENS_TEST_OCEANBASE_URL" => "DBLENS_TEST_ORACLE_URL", "m5ob";
    oceanbase_to_dameng: "DBLENS_TEST_OCEANBASE_URL" => "DBLENS_TEST_DM_URL", "m5ob";
    opengauss_to_mysql: "DBLENS_TEST_OPENGAUSS_URL" => "DBLENS_TEST_MYSQL_URL", "m5og";
    opengauss_to_pg: "DBLENS_TEST_OPENGAUSS_URL" => "DBLENS_TEST_PG_URL", "m5og";
    opengauss_to_oracle: "DBLENS_TEST_OPENGAUSS_URL" => "DBLENS_TEST_ORACLE_URL", "m5og";
    opengauss_to_dameng: "DBLENS_TEST_OPENGAUSS_URL" => "DBLENS_TEST_DM_URL", "m5og";
    opengauss_a_to_mysql: "DBLENS_TEST_OPENGAUSS_A_URL" => "DBLENS_TEST_MYSQL_URL", "m5oa";
    opengauss_a_to_pg: "DBLENS_TEST_OPENGAUSS_A_URL" => "DBLENS_TEST_PG_URL", "m5oa";
    opengauss_a_to_oracle: "DBLENS_TEST_OPENGAUSS_A_URL" => "DBLENS_TEST_ORACLE_URL", "m5oa";
    opengauss_a_to_dameng: "DBLENS_TEST_OPENGAUSS_A_URL" => "DBLENS_TEST_DM_URL", "m5oa";
    kingbase_to_mysql: "DBLENS_TEST_KINGBASE_URL" => "DBLENS_TEST_MYSQL_URL", "m5kb";
    kingbase_to_pg: "DBLENS_TEST_KINGBASE_URL" => "DBLENS_TEST_PG_URL", "m5kb";
    kingbase_to_oracle: "DBLENS_TEST_KINGBASE_URL" => "DBLENS_TEST_ORACLE_URL", "m5kb";
    kingbase_to_dameng: "DBLENS_TEST_KINGBASE_URL" => "DBLENS_TEST_DM_URL", "m5kb";
    kingbase_pg_to_mysql: "DBLENS_TEST_KINGBASE_PG_URL" => "DBLENS_TEST_MYSQL_URL", "m5kp";
    kingbase_pg_to_pg: "DBLENS_TEST_KINGBASE_PG_URL" => "DBLENS_TEST_PG_URL", "m5kp";
    kingbase_pg_to_oracle: "DBLENS_TEST_KINGBASE_PG_URL" => "DBLENS_TEST_ORACLE_URL", "m5kp";
    kingbase_pg_to_dameng: "DBLENS_TEST_KINGBASE_PG_URL" => "DBLENS_TEST_DM_URL", "m5kp";
}
