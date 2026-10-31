//! 迁移（M5a 结构、M5b 数据）的实库测试：源库和目标库的环境变量都设置时运行，否则直接通过（变量见 `live.rs`）。
//!
//! 在源库建一套测试结构（自增主键、默认值、注释、唯一约束、检查约束、索引、外键）并放数据（1000 行的表、各种类型的表、
//! 推进过的序列），迁到目标库：读回目标结构与迁移草稿比对；行数和抽样逐值比对（迁移自己的校验）都一致；
//! 目标上插入新行时自增不冲突，序列接着源序列往下走；最后清理两边。表名带源库的代号，不同路线互不干扰。
//! 另有续传（迁到一半取消再继续）和跳过出错行的测试。
//! 人大金仓开发版最多 10 个连接，与其他实库测试一样要串行运行（`--test-threads=1`）。

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use dblens_core::schema::Table;
use dblens_core::{LogicalType, Value};
use dblens_dialect::migrate::{ColumnOverride, ExistingAction};
use dblens_dialect::{Dialect, Variant};
use dblens_driver::{ConnectParams, DriverId};
use dblens_service::{
    DataOptions, DefaultPolicy, DriverRegistry, Environment, ExecRequest, MigrationEnd, MigrationPlan,
    OnError, ProfileInput, SecretBackend, Secrets, Service, SessionEvent, Store, TableState, TaskState,
    TaskStatus,
};

const LIB_OPTIONS: [&str; 2] = ["oracle_client_lib_dir", "jdbc_dir"];

/// 大表的行数。MySQL 的递归 CTE 默认至多 1000 层。
const ROWS: u64 = 1000;

struct End {
    profile: String,
    driver: DriverId,
    variant: Variant,
    /// 所在的库：SQL Server 是连接的库，其他库不填。
    catalog: Option<String>,
    schema: String,
}

impl End {
    fn upper(&self) -> bool {
        matches!(self.driver, DriverId::Oracle | DriverId::Dameng)
    }

    fn has_sequences(&self) -> bool {
        self.driver != DriverId::Mysql
    }
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
    service.migrations.set_dir(dir.path().join("migrations"));
    let mut ends = Vec::new();
    for (env, url) in envs.iter().zip(urls) {
        let (driver, params) = ConnectParams::from_url(&url).unwrap();
        // 达梦的驱动目录是连接选项，不是全局设置
        for option in LIB_OPTIONS.iter().filter(|_| driver != DriverId::Dameng) {
            if let Some(lib) = params.options.get(*option) {
                registry.set_lib_dir(driver, Some(Path::new(lib))).unwrap();
            }
        }
        let mut options = params.options.clone();
        options.retain(|k, _| driver == DriverId::Dameng || !LIB_OPTIONS.contains(&k.as_str()));
        let schema = match driver {
            DriverId::Mysql => params.database.clone().unwrap(),
            DriverId::Postgres => "public".to_string(),
            DriverId::Oracle | DriverId::Dameng => params.user.to_uppercase(),
            DriverId::SqlServer => "dbo".to_string(),
            DriverId::Informix => params.user.clone(),
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
        let catalog = if driver == DriverId::SqlServer { params.database.clone() } else { None };
        ends.push(End { profile: profile.id, driver, variant: summary.variant, catalog, schema });
    }
    Some((service, ends, dir))
}

/// 在一端执行若干语句，返回最后一个结果集的第一个值；`tolerate` 为 true 时忽略失败（清理用）。
async fn run(s: &Service, end: &End, sqls: &[String], tolerate: bool) -> Option<Value> {
    let id = s.open_session(&end.profile).await.unwrap().id;
    let mut last = None;
    for sql in sqls {
        s.sessions.submit(id, ExecRequest::new(sql)).await.unwrap();
        while let Some(ev) = s.sessions.next(id, 500).await.unwrap() {
            match &ev {
                SessionEvent::Failed { error, .. } => assert!(tolerate, "{sql} 执行失败：{error}"),
                SessionEvent::Rows { rows } => last = rows.first().and_then(|r| r.first()).cloned(),
                _ => {}
            }
        }
    }
    s.sessions.close(id, true).await.unwrap();
    last
}

fn number(v: Option<Value>) -> i64 {
    match v {
        Some(Value::Int(n)) => n,
        Some(Value::UInt(n)) => n as i64,
        Some(Value::Decimal(s) | Value::String(s)) => s.trim().split('.').next().unwrap().parse().unwrap(),
        other => panic!("不是数字：{other:?}"),
    }
}

/// 删除测试表（子表在前）和序列。
fn drops(end: &End, names: &[String], seq: &str) -> Vec<String> {
    let mut out: Vec<String> = names
        .iter()
        .map(|n| match end.driver {
            DriverId::Oracle => {
                format!("BEGIN EXECUTE IMMEDIATE 'DROP TABLE \"{n}\" CASCADE CONSTRAINTS'; EXCEPTION WHEN OTHERS THEN NULL; END;")
            }
            DriverId::Dameng | DriverId::Postgres => format!("DROP TABLE IF EXISTS \"{n}\" CASCADE"),
            DriverId::Mysql => format!("DROP TABLE IF EXISTS `{n}`"),
            DriverId::SqlServer => format!("DROP TABLE IF EXISTS [{n}]"),
            DriverId::Informix => format!("DROP TABLE IF EXISTS \"{n}\""),
        })
        .collect();
    out.push(match end.driver {
        DriverId::Oracle => format!(
            "BEGIN EXECUTE IMMEDIATE 'DROP SEQUENCE \"{seq}\"'; EXCEPTION WHEN OTHERS THEN NULL; END;"
        ),
        DriverId::Dameng | DriverId::Postgres => format!("DROP SEQUENCE IF EXISTS \"{seq}\""),
        DriverId::Mysql => "SELECT 1".into(),
        DriverId::SqlServer => format!("DROP SEQUENCE IF EXISTS [{seq}]"),
        DriverId::Informix => format!("DROP SEQUENCE IF EXISTS \"{seq}\""),
    });
    out
}

/// 源库的测试结构和数据：`p` 为表名前缀（Oracle、达梦用大写）。
fn source_ddl(end: &End, p: &str) -> Vec<String> {
    let opengauss = end.variant == Variant::OpenGauss;
    let n = ROWS;
    match end.driver {
        DriverId::Informix => vec![
            format!(
                "CREATE TABLE {p}_cust (id BIGSERIAL PRIMARY KEY CONSTRAINT {p}_pk_cust, name VARCHAR(50) DEFAULT 'n/a' NOT NULL, \
                 active BOOLEAN DEFAULT 't', created DATETIME YEAR TO SECOND DEFAULT CURRENT YEAR TO SECOND, \
                 amount DECIMAL(12,2) DEFAULT 0, note LVARCHAR(200), \
                 CHECK (amount >= 0) CONSTRAINT {p}_ck_amount, UNIQUE (name) CONSTRAINT {p}_uk_name)"
            ),
            format!("COMMENT ON TABLE {p}_cust IS 'customers'"),
            format!("COMMENT ON COLUMN {p}_cust.name IS 'name'"),
            format!("CREATE INDEX {p}_ix_created ON {p}_cust (created)"),
            format!(
                "CREATE TABLE {p}_ord (id INT PRIMARY KEY, cust_id BIGINT, qty INT, status VARCHAR(10) DEFAULT 'new', \
                 CHECK (qty > 0) CONSTRAINT {p}_ck_qty, \
                 FOREIGN KEY (cust_id) REFERENCES {p}_cust (id) ON DELETE CASCADE CONSTRAINT {p}_fk_cust)"
            ),
            format!(
                "CREATE TABLE {p}_types (id INT PRIMARY KEY, txt VARCHAR(100), big LVARCHAR(4000), amount DECIMAL(12,2), \
                 ratio FLOAT, flag BOOLEAN, day DATE, ts DATETIME YEAR TO FRACTION(3))"
            ),
            format!("CREATE SEQUENCE {p}_seq START WITH 1"),
            format!("SELECT FIRST 5 {p}_seq.NEXTVAL FROM systables"),
            format!(
                "CREATE PROCEDURE {p}_fill()\n  DEFINE i INT;\n  FOR i = 1 TO {n}\n    \
                 INSERT INTO {p}_cust (name, amount, note) VALUES ('name' || i, i * 1.5, '备注' || i);\n  \
                 END FOR;\nEND PROCEDURE"
            ),
            format!("EXECUTE PROCEDURE {p}_fill()"),
            format!("DROP PROCEDURE {p}_fill"),
            format!("INSERT INTO {p}_ord (id, cust_id, qty, status) VALUES (1, 1, 2, 'paid')"),
            format!("INSERT INTO {p}_ord (id, cust_id, qty, status) VALUES (2, 3, 1, 'new')"),
            format!(
                "INSERT INTO {p}_types VALUES (1, '中文 abc', '{}', 1234.5, 0.25, 't', '2026-01-02', '2026-01-02 03:04:05.678')",
                "y".repeat(3000)
            ),
            format!("INSERT INTO {p}_types VALUES (2, 'x''q', 'z', -0.01, NULL, 'f', NULL, NULL)"),
            format!("INSERT INTO {p}_types (id) VALUES (3)"),
        ],
        DriverId::SqlServer => vec![
            format!(
                "CREATE TABLE {p}_cust (id BIGINT IDENTITY(1,1) PRIMARY KEY, name NVARCHAR(50) NOT NULL DEFAULT 'n/a', \
                 active BIT DEFAULT 1, created DATETIME2 DEFAULT SYSDATETIME(), amount DECIMAL(12,2) DEFAULT 0, note NVARCHAR(MAX), \
                 CONSTRAINT {p}_ck_amount CHECK (amount >= 0), CONSTRAINT {p}_uk_name UNIQUE (name))"
            ),
            format!(
                "EXEC sp_addextendedproperty N'MS_Description', N'customers', N'SCHEMA', N'dbo', N'TABLE', N'{p}_cust'"
            ),
            format!(
                "EXEC sp_addextendedproperty N'MS_Description', N'name', N'SCHEMA', N'dbo', N'TABLE', N'{p}_cust', N'COLUMN', N'name'"
            ),
            format!("CREATE INDEX {p}_ix_created ON {p}_cust (created)"),
            format!(
                "CREATE TABLE {p}_ord (id INT PRIMARY KEY, cust_id BIGINT, qty INT, status NVARCHAR(10) DEFAULT 'new', \
                 CONSTRAINT {p}_ck_qty CHECK (qty > 0), \
                 CONSTRAINT {p}_fk_cust FOREIGN KEY (cust_id) REFERENCES {p}_cust (id) ON DELETE CASCADE)"
            ),
            format!(
                "CREATE TABLE {p}_types (id INT PRIMARY KEY, txt NVARCHAR(100), big NVARCHAR(MAX), amount DECIMAL(12,2), ratio FLOAT, \
                 flag BIT, day DATE, ts DATETIME2(3), bin VARBINARY(16))"
            ),
            format!("CREATE SEQUENCE {p}_seq START WITH 1"),
            format!(
                "DECLARE @i INT = 0, @n BIGINT; WHILE @i < 5 BEGIN SET @n = NEXT VALUE FOR {p}_seq; SET @i += 1; END"
            ),
            format!(
                "WITH s(n) AS (SELECT 1 UNION ALL SELECT n + 1 FROM s WHERE n < {n}) \
                 INSERT INTO {p}_cust (name, amount, note) SELECT CONCAT('name', n), n * 1.5, CONCAT(N'备注', n) FROM s OPTION (MAXRECURSION 0)"
            ),
            format!(
                "INSERT INTO {p}_ord (id, cust_id, qty, status) VALUES (1, 1, 2, 'paid'), (2, 3, 1, 'new')"
            ),
            format!(
                "INSERT INTO {p}_types VALUES (1, N'中文 abc', REPLICATE(N'y', 3000), 1234.5, 0.25, 1, '2026-01-02', '2026-01-02 03:04:05.678', 0x0102FF), \
                 (2, N'x''q', N'z', -0.01, NULL, 0, NULL, NULL, 0x), (3, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL)"
            ),
        ],
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
            format!(
                "CREATE TABLE {p}_types (id INT PRIMARY KEY, txt VARCHAR(100), big TEXT, amount DECIMAL(12,2), ratio DOUBLE, \
                 flag TINYINT(1), day DATE, ts DATETIME(3), bin VARBINARY(16))"
            ),
            format!(
                "INSERT INTO {p}_cust (name, amount, note) WITH RECURSIVE s(n) AS (SELECT 1 UNION ALL SELECT n + 1 FROM s WHERE n < {n}) \
                 SELECT CONCAT('name', n), n * 1.5, CONCAT('备注', n) FROM s"
            ),
            format!(
                "INSERT INTO {p}_ord (id, cust_id, qty, status) VALUES (1, 1, 2, 'paid'), (2, 3, 1, 'new')"
            ),
            format!(
                "INSERT INTO {p}_types VALUES (1, '中文 abc', REPEAT('y', 3000), 1234.5, 0.25, 1, '2026-01-02', '2026-01-02 03:04:05.678', X'0102FF'), \
                 (2, 'x''q', 'z', -0.01, NULL, 0, NULL, NULL, X''), (3, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL)"
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
            format!(
                "CREATE TABLE {p}_types (id INT PRIMARY KEY, txt VARCHAR(100), big TEXT, amount NUMERIC(12,2), ratio DOUBLE PRECISION, \
                 flag BOOLEAN, day DATE, ts TIMESTAMP(3), bin BYTEA)"
            ),
            format!("CREATE SEQUENCE {p}_seq START WITH 1"),
            format!("SELECT nextval('{p}_seq') FROM generate_series(1, 5)"),
            format!(
                "INSERT INTO {p}_cust (name, amount, note) SELECT 'name' || g, g * 1.5, '备注' || g FROM generate_series(1, {n}) g"
            ),
            format!(
                "INSERT INTO {p}_ord (id, cust_id, qty, status) VALUES (1, 1, 2, 'paid'), (2, 3, 1, 'new')"
            ),
            format!(
                "INSERT INTO {p}_types VALUES (1, '中文 abc', repeat('y', 3000), 1234.5, 0.25, true, DATE '2026-01-02', \
                 TIMESTAMP '2026-01-02 03:04:05.678', '\\x0102ff'::bytea), (2, 'x''q', 'z', -0.01, NULL, false, NULL, NULL, NULL), \
                 (3, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL)"
            ),
        ],
        DriverId::Oracle | DriverId::Dameng => {
            let ora = end.driver == DriverId::Oracle;
            // 达梦的外键要求两边类型相同
            let (id, created, cust_id, flag, bin, hex) = if ora {
                (
                    "NUMBER(19) GENERATED BY DEFAULT AS IDENTITY",
                    "DATE DEFAULT SYSDATE",
                    "NUMBER(19)",
                    "NUMBER(1)",
                    "RAW(16)",
                    "HEXTORAW('0102FF')",
                )
            } else {
                (
                    "BIGINT IDENTITY(1, 1)",
                    "TIMESTAMP DEFAULT SYSDATE",
                    "BIGINT",
                    "BIT",
                    "VARBINARY(16)",
                    "0x0102FF",
                )
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
                format!(
                    "CREATE TABLE {p}_TYPES (ID NUMBER(10) PRIMARY KEY, TXT VARCHAR2(100), BIG CLOB, AMOUNT NUMBER(12,2), \
                     RATIO {}, FLAG {flag}, DAY DATE, TS TIMESTAMP(3), BIN {bin})",
                    if ora { "BINARY_DOUBLE" } else { "DOUBLE" }
                ),
                format!("CREATE SEQUENCE {p}_SEQ START WITH 1"),
                format!(
                    "DECLARE n NUMBER; BEGIN FOR i IN 1..5 LOOP SELECT {p}_SEQ.NEXTVAL INTO n FROM DUAL; END LOOP; END;"
                ),
                format!(
                    "INSERT INTO {p}_CUST (NAME, AMOUNT, NOTE) SELECT 'name' || LEVEL, LEVEL * 1.5, '备注' || LEVEL FROM DUAL CONNECT BY LEVEL <= {n}"
                ),
                format!("INSERT INTO {p}_ORD (ID, CUST_ID, QTY, STATUS) VALUES (1, 1, 2, 'paid')"),
                format!("INSERT INTO {p}_ORD (ID, CUST_ID, QTY, STATUS) VALUES (2, 3, 1, 'new')"),
                format!(
                    "INSERT INTO {p}_TYPES VALUES (1, '中文 abc', RPAD('y', 3000, 'y'), 1234.5, 0.25, 1, DATE '2026-01-02', \
                     TIMESTAMP '2026-01-02 03:04:05.678', {hex})"
                ),
                format!("INSERT INTO {p}_TYPES VALUES (2, 'x''q', 'z', -0.01, NULL, 0, NULL, NULL, NULL)"),
                format!("INSERT INTO {p}_TYPES VALUES (3, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL)"),
                "COMMIT".into(),
            ]
        }
    }
}

/// 表名（子表在前；Oracle、达梦为大写）。
fn names(end: &End, p: &str) -> Vec<String> {
    ["ord", "types", "cust"]
        .iter()
        .map(|t| if end.upper() { format!("{p}_{t}").to_uppercase() } else { format!("{p}_{t}") })
        .collect()
}

fn seq_name(end: &End, p: &str) -> String {
    if end.upper() { format!("{p}_seq").to_uppercase() } else { format!("{p}_seq") }
}

async fn wait(s: &Service, id: &str) -> TaskStatus {
    for _ in 0..3000 {
        let st = s.tasks.status(id).unwrap();
        if !matches!(st.state, TaskState::Running | TaskState::Confirm) {
            return st;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("任务没有结束");
}

/// 一条路线的源和目标：建好源库的测试数据，清理目标。返回 `(源表名, 目标表名, 源序列名)`。
async fn prepare(s: &Service, src: &End, tgt: &End, p: &str) -> (Vec<String>, Vec<String>, String) {
    let prefix = if src.upper() { p.to_uppercase() } else { p.to_string() };
    let src_names = names(src, &prefix);
    let mut tgt_names = names(tgt, p);
    // MySQL 区分表名大小写：清理时大小写两种都删
    if tgt.driver == DriverId::Mysql {
        tgt_names.extend(names(tgt, p).iter().map(|n| n.to_uppercase()).collect::<Vec<_>>());
    }
    let seq = seq_name(src, &prefix);
    run(s, src, &drops(src, &src_names, &seq), true).await;
    run(s, tgt, &drops(tgt, &tgt_names, &seq_name(tgt, p)), true).await;
    let ddl: Vec<String> = source_ddl(src, &prefix)
        .into_iter()
        .filter(|sql| src.has_sequences() || !sql.contains("_seq"))
        .collect();
    run(s, src, &ddl, false).await;
    (src_names, tgt_names, seq)
}

async fn cleanup(s: &Service, src: &End, tgt: &End, p: &str, names: (&[String], &[String], &str)) {
    run(s, tgt, &drops(tgt, names.1, &seq_name(tgt, p)), true).await;
    run(s, src, &drops(src, names.0, names.2), true).await;
}

fn plan(src: &End, tgt: &End, tables: &[String], seq: Option<&str>, data: DataOptions) -> MigrationPlan {
    MigrationPlan {
        source: MigrationEnd {
            profile: src.profile.clone(),
            catalog: src.catalog.clone(),
            schema: src.schema.clone(),
        },
        tables: Some(tables.iter().rev().cloned().collect()),
        sequences: Some(seq.into_iter().map(String::from).collect()),
        target: MigrationEnd {
            profile: tgt.profile.clone(),
            catalog: tgt.catalog.clone(),
            schema: tgt.schema.clone(),
        },
        mode: Default::default(),
        settings: Default::default(),
        data,
        existing: BTreeMap::new(),
    }
}

/// 源 → 目标迁移一次并核对。`p`：表名前缀（小写）。
async fn migrate_flow(src_env: &str, tgt_env: &str, p: &str) {
    let Some((s, ends, _dir)) = setup([src_env, tgt_env]).await else { return };
    let (src, tgt) = (&ends[0], &ends[1]);
    let route = format!("{src_env} → {tgt_env}");
    let (src_names, tgt_names, seq) = prepare(&s, src, tgt, p).await;
    let with_seq = src.has_sequences() && tgt.has_sequences();
    let data = DataOptions { batch_size: 300, parallel: 2, on_error: OnError::Stop, sample: 100 };
    let plan = plan(src, tgt, &src_names, with_seq.then_some(seq.as_str()), data);
    let preview = s.preview_migration(&plan).await.unwrap();
    let notes: Vec<String> = preview
        .draft
        .notes
        .iter()
        .map(|n| format!("{:?} {}.{:?} {:?}: {}", n.level, n.object, n.column, n.item, n.message.current()))
        .collect();
    eprintln!("{route} 提示：\n{}\n脚本：\n{}", notes.join("\n"), preview.text);
    assert!(preview.problems.is_empty(), "{route}：{:?}", preview.problems);
    assert_eq!(preview.draft.tables.len(), 3, "{route}");
    let id = s.start_migration(plan).await.unwrap();
    let st = wait(&s, &id).await;
    let failures: Vec<String> = st.failures.iter().map(|f| format!("{}\n  {}", f.text, f.error)).collect();
    assert_eq!(st.state, TaskState::Done, "{route}：{st:?}");
    assert!(failures.is_empty(), "{route} 失败的语句：\n{}", failures.join("\n"));
    let (report, _) = s.tasks.report(&id).unwrap();
    let report = std::fs::read_to_string(report).unwrap();
    // 数据：每张表都迁完，行数和抽样都一致
    let m = st.migration.expect("迁移进度");
    for t in &m.tables {
        let v =
            t.verify.as_ref().unwrap_or_else(|| panic!("{route}：{} 没有校验：{t:?}\n{report}", t.source));
        assert_eq!(t.state, TableState::Done, "{route}：{t:?}");
        assert!(
            v.target_rows == v.expected_rows && v.mismatches.is_empty(),
            "{route}：{} 校验不一致：{v:?}",
            t.source
        );
    }
    let cust = m.tables.iter().find(|t| t.source.to_lowercase().ends_with("_cust")).unwrap();
    assert_eq!((cust.rows, cust.verify.as_ref().unwrap().source_rows), (ROWS, ROWS), "{route}");
    assert!(s.unfinished_migrations().unwrap().is_empty(), "{route}");

    // 读回目标结构，与草稿中的目标结构比对
    let tp = s.profile(&tgt.profile).unwrap();
    let dialect: Dialect = s.metadata.dialect(&tp).await.unwrap();
    for t in &preview.draft.tables {
        let mut got: Vec<Table> = s.metadata.tables(&tp, &tgt.schema, Some(&t.target.name)).await.unwrap();
        assert_eq!(got.len(), 1, "{route}：{} 没有建成", t.target.name);
        let got = Table { triggers: vec![], ..got.remove(0) };
        assert_eq!(
            shape(&dialect, &t.target, &t.target),
            shape(&dialect, &got, &t.target),
            "{route}：{} 读回后与草稿不同",
            t.target.name
        );
    }

    // 自增推进过：插入新行不与迁来的主键冲突；序列接着源序列往下走
    let ct =
        &preview.draft.tables.iter().find(|t| t.source.to_lowercase().ends_with("_cust")).unwrap().target;
    let (q, name) = (dialect.qualified(Some(&tgt.schema), &ct.name), dialect.ident(&ct.columns[1].name));
    run(&s, tgt, &[format!("INSERT INTO {q} ({name}) VALUES ('new')")], false).await;
    let id_col = dialect.ident(&ct.columns[0].name);
    let new_id =
        number(run(&s, tgt, &[format!("SELECT {id_col} FROM {q} WHERE {name} = 'new'")], false).await);
    assert!(new_id > ROWS as i64, "{route}：新行的主键 {new_id}");
    if with_seq {
        let tseq = dialect.qualified(Some(&tgt.schema), &preview.draft.sequences[0].target.name);
        let sql = match tgt.driver {
            DriverId::Postgres => format!("SELECT nextval('{tseq}')"),
            DriverId::Informix => format!("SELECT {tseq}.NEXTVAL FROM sysmaster:sysdual"),
            DriverId::SqlServer => format!("SELECT NEXT VALUE FOR {tseq}"),
            _ => format!("SELECT {tseq}.NEXTVAL FROM DUAL"),
        };
        let next = number(run(&s, tgt, &[sql], false).await);
        assert!(next > 5, "{route}：序列的下一个值 {next}");
    }
    cleanup(&s, src, tgt, p, (&src_names, &tgt_names, &seq)).await;
}

/// 续传：小批量迁移，迁了几批后取消；未完成的迁移列出来，继续后全部迁完，没有重复也没有遗漏。
async fn resume_flow(src_env: &str, tgt_env: &str, p: &str) {
    let Some((s, ends, _dir)) = setup([src_env, tgt_env]).await else { return };
    let (src, tgt) = (&ends[0], &ends[1]);
    let route = format!("{src_env} → {tgt_env}（续传）");
    let (src_names, tgt_names, seq) = prepare(&s, src, tgt, p).await;
    let data = DataOptions { batch_size: 100, parallel: 1, on_error: OnError::Stop, sample: 20 };
    let id = s.start_migration(plan(src, tgt, &src_names, None, data)).await.unwrap();
    // 大表迁了几批后取消
    for _ in 0..600 {
        let st = s.tasks.status(&id).unwrap();
        let copied =
            st.migration.as_ref().map(|m| m.tables.iter().map(|t| t.rows).max().unwrap_or(0)).unwrap_or(0);
        if copied >= 200 || st.state != TaskState::Running {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    s.tasks.cancel_or_remove(&id).await.unwrap();
    let st = wait(&s, &id).await;
    assert_eq!(st.state, TaskState::Cancelled, "{route}：{st:?}");
    let list = s.unfinished_migrations().unwrap();
    assert_eq!(list.len(), 1, "{route}");
    let id = s.resume_migration(&list[0].id).await.unwrap();
    let st = wait(&s, &id).await;
    assert_eq!(st.state, TaskState::Done, "{route}：{st:?}");
    for t in &st.migration.unwrap().tables {
        let v = t.verify.as_ref().unwrap();
        assert!(
            t.state == TableState::Done && v.target_rows == v.source_rows && v.mismatches.is_empty(),
            "{route}：{t:?}"
        );
    }
    assert!(s.unfinished_migrations().unwrap().is_empty(), "{route}");
    cleanup(&s, src, tgt, p, (&src_names, &tgt_names, &seq)).await;
}

/// 跳过出错的行：订单状态列改成目标上很短的类型，超长的值写不进去；选"跳过并记录"时其余行照常迁，跳过的行记进报告；
/// 再以"清空后导入"迁一次，结果相同。
async fn skip_flow(src_env: &str, tgt_env: &str, p: &str) {
    let Some((s, ends, _dir)) = setup([src_env, tgt_env]).await else { return };
    let (src, tgt) = (&ends[0], &ends[1]);
    let route = format!("{src_env} → {tgt_env}（跳过）");
    let (src_names, tgt_names, seq) = prepare(&s, src, tgt, p).await;
    let ord = src_names[0].clone();
    let short = if tgt.upper() { "VARCHAR2(3)" } else { "VARCHAR(3)" };
    let status = if src.upper() { "STATUS" } else { "status" };
    let data = DataOptions { batch_size: 1000, parallel: 2, on_error: OnError::Skip, sample: 0 };
    let mut pl = plan(src, tgt, std::slice::from_ref(&ord), None, data);
    pl.settings
        .tables
        .entry(ord.clone())
        .or_default()
        .columns
        .insert(status.into(), ColumnOverride { data_type: Some(short.into()), ..Default::default() });
    let id = s.start_migration(pl.clone()).await.unwrap();
    let st = wait(&s, &id).await;
    assert_eq!(st.state, TaskState::Done, "{route}：{st:?}");
    let t = &st.migration.unwrap().tables[0];
    // 'paid' 超长被跳过，'new' 写入
    assert_eq!((t.rows, t.skipped), (1, 1), "{route}：{t:?}");
    assert!(t.verify.as_ref().unwrap().target_rows == 1, "{route}：{t:?}");
    let (report, _) = s.tasks.report(&id).unwrap();
    assert!(std::fs::read_to_string(report).unwrap().contains(&format!("{ord} (1)")), "{route}");
    // 清空后导入：仍是 1 行
    pl.existing.insert(ord.clone(), ExistingAction::Replace);
    let id = s.start_migration(pl).await.unwrap();
    let st = wait(&s, &id).await;
    let t = &st.migration.unwrap().tables[0];
    let v = t.verify.as_ref().unwrap();
    assert_eq!((v.expected_rows, v.target_rows, t.skipped), (1, 1, 1), "{route}：{t:?}");
    cleanup(&s, src, tgt, p, (&src_names, &tgt_names, &seq)).await;
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

/// 每个源库迁到 MySQL、PG、Oracle、达梦（不迁到自己），另加信创常见路线；SQL Server 与这四个库互迁（M6b）。
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
    gbase_to_mysql: "DBLENS_TEST_GBASE_URL" => "DBLENS_TEST_MYSQL_URL", "m8gb";
    gbase_to_pg: "DBLENS_TEST_GBASE_URL" => "DBLENS_TEST_PG_URL", "m8gb";
    gbase_to_oracle: "DBLENS_TEST_GBASE_URL" => "DBLENS_TEST_ORACLE_URL", "m8gb";
    mysql_to_gbase: "DBLENS_TEST_MYSQL_URL" => "DBLENS_TEST_GBASE_URL", "m8my";
    pg_to_gbase: "DBLENS_TEST_PG_URL" => "DBLENS_TEST_GBASE_URL", "m8pg";
    sqlserver_to_mysql: "DBLENS_TEST_SQLSERVER_URL" => "DBLENS_TEST_MYSQL_URL", "m6ms";
    sqlserver_to_pg: "DBLENS_TEST_SQLSERVER_URL" => "DBLENS_TEST_PG_URL", "m6ms";
    sqlserver_to_oracle: "DBLENS_TEST_SQLSERVER_URL" => "DBLENS_TEST_ORACLE_URL", "m6ms";
    sqlserver_to_dameng: "DBLENS_TEST_SQLSERVER_URL" => "DBLENS_TEST_DM_URL", "m6ms";
    mysql_to_sqlserver: "DBLENS_TEST_MYSQL_URL" => "DBLENS_TEST_SQLSERVER_URL", "m6my";
    pg_to_sqlserver: "DBLENS_TEST_PG_URL" => "DBLENS_TEST_SQLSERVER_URL", "m6pg";
    oracle_to_sqlserver: "DBLENS_TEST_ORACLE_URL" => "DBLENS_TEST_SQLSERVER_URL", "m6or";
    dameng_to_sqlserver: "DBLENS_TEST_DM_URL" => "DBLENS_TEST_SQLSERVER_URL", "m6dm";
}

/// 续传和跳过：每种目标家族各一条。
macro_rules! special {
    ($($name:ident: $flow:ident, $src:literal => $tgt:literal, $p:literal;)*) => {
        $(
            #[tokio::test]
            async fn $name() {
                $flow($src, $tgt, $p).await;
            }
        )*
    };
}

special! {
    resume_pg_to_mysql: resume_flow, "DBLENS_TEST_PG_URL" => "DBLENS_TEST_MYSQL_URL", "m5rp";
    resume_mysql_to_oracle: resume_flow, "DBLENS_TEST_MYSQL_URL" => "DBLENS_TEST_ORACLE_URL", "m5rm";
    resume_oracle_to_dameng: resume_flow, "DBLENS_TEST_ORACLE_URL" => "DBLENS_TEST_DM_URL", "m5ro";
    resume_dameng_to_pg: resume_flow, "DBLENS_TEST_DM_URL" => "DBLENS_TEST_PG_URL", "m5rd";
    skip_pg_to_mysql: skip_flow, "DBLENS_TEST_PG_URL" => "DBLENS_TEST_MYSQL_URL", "m5sp";
    skip_mysql_to_oracle: skip_flow, "DBLENS_TEST_MYSQL_URL" => "DBLENS_TEST_ORACLE_URL", "m5sm";
    skip_oracle_to_pg: skip_flow, "DBLENS_TEST_ORACLE_URL" => "DBLENS_TEST_PG_URL", "m5so";
    skip_pg_to_dameng: skip_flow, "DBLENS_TEST_PG_URL" => "DBLENS_TEST_DM_URL", "m5sd";
    resume_pg_to_sqlserver: resume_flow, "DBLENS_TEST_PG_URL" => "DBLENS_TEST_SQLSERVER_URL", "m6rp";
    skip_mysql_to_sqlserver: skip_flow, "DBLENS_TEST_MYSQL_URL" => "DBLENS_TEST_SQLSERVER_URL", "m6sm";
}
