//! 元数据查询的实库测试。设置对应的环境变量时运行，否则直接通过（与驱动一致性测试相同的约定）：
//! DBLENS_TEST_MYSQL_URL、DBLENS_TEST_MARIADB_URL、DBLENS_TEST_PG_URL、DBLENS_TEST_ORACLE_URL、DBLENS_TEST_DM_URL；
//! 国产库第一批：DBLENS_TEST_TIDB_URL、DBLENS_TEST_OCEANBASE_URL、DBLENS_TEST_OPENGAUSS_URL、
//! DBLENS_TEST_OPENGAUSS_A_URL、DBLENS_TEST_KINGBASE_URL、DBLENS_TEST_KINGBASE_PG_URL；
//! SQL Server：DBLENS_TEST_SQLSERVER_URL（库写测试库，如 `/dblens_test`；元数据查询在连接的当前库中执行）。
//!
//! 每个家族先执行 `tests/fixtures/<家族>.sql` 建测试对象，再用全部元数据查询读回来逐项核对。
//! 兼容库沿用所属家族的脚本，跳过不支持的段、替换写法不同的段（`run_script_with`）。

use std::time::Instant;

use dblens_core::schema::{RoutineKind, Table};
use dblens_core::{LogicalType, Value};
use dblens_dialect::meta::{self, MetaArgs, MetaKey, MetaRows, TableRows};
use dblens_dialect::{Dialect, Family, Variant};
use dblens_driver::{ConnectParams, Connection, Driver, DriverId, collect_all};

struct Live {
    conn: Box<dyn Connection>,
    d: Dialect,
}

async fn fetch(conn: &mut Box<dyn Connection>, sql: &str, params: &[Value]) -> MetaRows {
    let mut stream = conn.execute(sql, params).await.unwrap_or_else(|e| panic!("{e}\n{sql}"));
    let events = collect_all(stream.as_mut(), 10_000).await.unwrap_or_else(|e| panic!("{e}\n{sql}"));
    MetaRows::from_events(events).unwrap()
}

async fn connect(env: &str) -> Option<Live> {
    let url = std::env::var(env).ok()?;
    let (id, params) = ConnectParams::from_url(&url).unwrap();
    let driver: Box<dyn Driver> = match id {
        DriverId::Mysql => Box::new(dblens_driver_mysql::MySqlDriver),
        DriverId::Postgres => Box::new(dblens_driver_postgres::PgDriver),
        DriverId::Oracle => Box::new(dblens_driver_oracle::OracleDriver),
        DriverId::Dameng => Box::new(dblens_driver_dameng::DmDriver),
        DriverId::SqlServer => Box::new(dblens_driver_sqlserver::SqlServerDriver),
    };
    let mut conn = driver.connect(&params).await.unwrap();
    let info = conn.server_info().await.unwrap();
    let mut d = Dialect::detect(id, &info);
    if let Some(q) = d.meta_query(MetaKey::VariantProbe, &MetaArgs::default()).unwrap() {
        let probe = fetch(&mut conn, &q.sql, &q.params).await;
        d = d.refine(&probe);
    }
    Some(Live { conn, d })
}

fn args(schema: &str) -> MetaArgs {
    MetaArgs { schema: Some(schema.into()), ..Default::default() }
}

impl Live {
    async fn rows(&mut self, key: MetaKey, a: &MetaArgs) -> MetaRows {
        let q = self
            .d
            .meta_query(key, a)
            .unwrap()
            .unwrap_or_else(|| panic!("{:?} 没有 {key:?} 模板", self.d.variant()));
        fetch(&mut self.conn, &q.sql, &q.params).await
    }

    /// 按 `-- @@ 标签` 分段逐条执行建测试对象的脚本。
    async fn run_script(&mut self, text: &str) {
        self.run_script_with(text, &[], &[]).await;
    }

    /// 同 `run_script`，但跳过 `skip` 中的段，`replace` 中的段改为执行给出的语句。
    async fn run_script_with(&mut self, text: &str, skip: &[&str], replace: &[(&str, &str)]) {
        let mut sections: Vec<(String, String)> = Vec::new();
        for line in text.lines() {
            if let Some(label) = line.strip_prefix("-- @@") {
                sections.push((label.trim().to_string(), String::new()));
            } else if let Some((_, last)) = sections.last_mut() {
                last.push_str(line);
                last.push('\n');
            }
        }
        for (label, sql) in sections {
            if skip.contains(&label.as_str()) {
                continue;
            }
            let sql = replace.iter().find(|(l, _)| *l == label).map_or(sql.trim(), |(_, r)| r.trim());
            if sql.is_empty() {
                continue;
            }
            let mut stream = self.conn.execute(sql, &[]).await.unwrap_or_else(|e| panic!("{e}\n{sql}"));
            collect_all(stream.as_mut(), 1000).await.unwrap_or_else(|e| panic!("{e}\n{sql}"));
        }
    }

    /// 取表明细：一类对象一条查询，共六条。
    async fn tables(&mut self, schema: &str, table: Option<&str>) -> Vec<Table> {
        let a = MetaArgs { table: table.map(Into::into), ..args(schema) };
        let rows = TableRows {
            tables: self.rows(MetaKey::Tables, &a).await,
            columns: self.rows(MetaKey::Columns, &a).await,
            keys: self.rows(MetaKey::Keys, &a).await,
            checks: self.rows(MetaKey::Checks, &a).await,
            indexes: self.rows(MetaKey::Indexes, &a).await,
            triggers: self.rows(MetaKey::Triggers, &a).await,
        };
        meta::tables(&self.d, &rows).unwrap()
    }

    /// 服务器报告的每个保留字都必须被 `ident` 加引号。
    async fn check_reserved_words(&mut self) {
        let rows = self.rows(MetaKey::ReservedWords, &MetaArgs::default()).await;
        let words = meta::reserved_words(&rows).unwrap();
        assert!(words.len() > 50, "{:?} 只取到 {} 个保留字", self.d.variant(), words.len());
        let lower = matches!(self.d.family(), Family::MySql | Family::Postgres);
        let unquoted: Vec<&String> = words
            .iter()
            .filter(|w| {
                let name = if lower { w.to_ascii_lowercase() } else { w.to_string() };
                self.d.ident(&name) == name
            })
            .collect();
        assert!(unquoted.is_empty(), "{:?} 这些保留字没有加引号：{unquoted:?}", self.d.variant());
    }
}

fn table<'a>(tables: &'a [Table], name: &str) -> &'a Table {
    tables.iter().find(|t| t.name == name).unwrap_or_else(|| panic!("没有表 {name}"))
}

fn logical(t: &Table, column: &str) -> LogicalType {
    t.columns
        .iter()
        .find(|c| c.name == column)
        .unwrap_or_else(|| panic!("{} 没有列 {column}", t.name))
        .data_type
        .logical
        .clone()
}

fn has(text: &str, needle: &str) -> bool {
    text.to_ascii_lowercase().contains(&needle.to_ascii_lowercase())
}

/// 测试对象的命名：MySQL/PG 用小写原名；Oracle/达梦用大写并加 `META_` 前缀（列名只转大写）。
/// `triggers`、`routines` 为 false 时该库没有这类对象（TiDB），不检查；`descending` 为 false 时
/// 该库建索引时忽略 DESC（TiDB、OceanBase）。
#[derive(Clone, Copy)]
struct Fx {
    schema: &'static str,
    upper: bool,
    triggers: bool,
    routines: bool,
    descending: bool,
}

impl Fx {
    fn lower(schema: &'static str) -> Fx {
        Fx { schema, upper: false, triggers: true, routines: true, descending: true }
    }

    fn upper(schema: &'static str) -> Fx {
        Fx { schema, upper: true, triggers: true, routines: true, descending: true }
    }

    fn obj(&self, name: &str) -> String {
        if self.upper { format!("META_{}", name.to_ascii_uppercase()) } else { name.to_string() }
    }

    fn col(&self, name: &str) -> String {
        if self.upper { name.to_ascii_uppercase() } else { name.to_string() }
    }
}

/// 各家族共有的检查。返回整个 schema 的表明细，供各家族再做专门检查。
async fn check_common(live: &mut Live, fx: Fx) -> Vec<Table> {
    let s = fx.schema;
    let (dept_name, emp_name) = (fx.obj("dept"), fx.obj("emp"));

    let schemas = meta::schemas(&live.rows(MetaKey::Schemas, &MetaArgs::default()).await).unwrap();
    assert!(schemas.iter().any(|x| x.name == s && !x.is_system), "{schemas:?}");
    assert!(schemas.iter().any(|x| x.is_system), "应识别出系统 schema");

    let list = meta::objects(MetaKey::Tables, &live.rows(MetaKey::Tables, &args(s)).await).unwrap();
    let names: Vec<&str> = list.iter().map(|o| o.name.as_str()).collect();
    assert!(names.contains(&dept_name.as_str()) && names.contains(&emp_name.as_str()), "{names:?}");
    // “员工”只出现在 emp 的表注释里，不在任何表名里
    let by_comment = MetaArgs { filter: Some("员工".into()), ..args(s) };
    let hit = meta::objects(MetaKey::Tables, &live.rows(MetaKey::Tables, &by_comment).await).unwrap();
    assert_eq!(hit.iter().map(|o| o.name.as_str()).collect::<Vec<_>>(), [emp_name.as_str()]);
    let none = MetaArgs { filter: Some("zz_no_such_100%".into()), ..args(s) };
    assert!(live.rows(MetaKey::Tables, &none).await.is_empty());

    let start = Instant::now();
    let all = live.tables(s, None).await;
    eprintln!("{:?} 整个 schema 的表明细：{} 张表，{:?}", live.d.variant(), all.len(), start.elapsed());

    let dept = table(&all, &dept_name);
    assert_eq!(dept.comment.as_deref(), Some("部门表"));
    let id = &dept.columns[0];
    assert!(id.name == fx.col("id") && id.auto_increment && !id.nullable, "{id:?}");
    assert_eq!(dept.primary_key.as_ref().unwrap().columns, [fx.col("id")]);
    assert_eq!(dept.uniques[0].columns, [fx.col("name")]);

    let emp = table(&all, &emp_name);
    let col = |n: &str| {
        emp.columns.iter().find(|c| c.name == fx.col(n)).unwrap_or_else(|| panic!("emp 没有列 {n}"))
    };
    let name = col("name");
    assert!(!name.nullable && name.default.as_deref().unwrap().contains("无名"), "{name:?}");
    assert_eq!(name.comment.as_deref(), Some("员工姓名"));
    assert!(has(col("name_upper").generated.as_deref().unwrap(), "name"));
    assert!(col("created").default.is_some());
    let fk = emp.foreign_keys.iter().find(|f| f.ref_table == dept_name).unwrap();
    assert_eq!((fk.columns.clone(), fk.on_delete.as_deref()), (vec![fx.col("dept_id")], Some("CASCADE")));
    assert_eq!(fk.ref_columns, [fx.col("id")]);
    let mgr = emp.foreign_keys.iter().find(|f| f.ref_table == emp_name).unwrap();
    assert_eq!(mgr.on_delete.as_deref(), Some("SET NULL"));
    assert_eq!(emp.uniques[0].columns, [fx.col("dept_id"), fx.col("name")]);
    assert!(emp.checks.iter().any(|c| has(&c.expression, "salary")), "{:?}", emp.checks);
    let created =
        emp.indexes.iter().find(|i| i.columns[0].name == fx.col("created")).expect("created 上的索引");
    assert_eq!(created.columns[0].descending, fx.descending);
    if fx.triggers {
        let trg = emp.triggers.iter().find(|t| t.timing.as_deref() == Some("BEFORE")).expect("BEFORE 触发器");
        assert!(trg.events.contains(&"INSERT".to_string()), "{trg:?}");
    }

    // 单表明细与整个 schema 中的同一张表一致
    let one = live.tables(s, Some(&emp_name)).await;
    assert_eq!(one.len(), 1);
    assert_eq!(&one[0], emp);

    // OceanBase 默认 lower_case_table_names = 1，表名存成小写
    let mixed = all.iter().find(|t| has(&t.name, "Mixed Case")).expect("表 Mixed Case");
    assert!(live.d.qualified(Some(s), &mixed.name).ends_with(&live.d.quote_ident(&mixed.name)));
    assert!(mixed.columns.iter().any(|c| c.name == "数洞"));
    assert!(mixed.columns.iter().any(|c| c.name == "select") && mixed.primary_key.is_some());

    let d = live.d;
    let columns = live.rows(MetaKey::Columns, &args(s)).await;
    let views =
        meta::views(&d, MetaKey::Views, &live.rows(MetaKey::Views, &args(s)).await, &columns).unwrap();
    let v = views.iter().find(|v| v.name == fx.obj("v_emp")).unwrap();
    assert_eq!(v.columns.len(), 3, "{v:?}");
    let def = MetaArgs { name: Some(fx.obj("v_emp")), ..args(s) };
    let text = meta::source_text(MetaKey::ViewDefinition, &live.rows(MetaKey::ViewDefinition, &def).await)
        .unwrap()
        .unwrap();
    assert!(has(&text, "dept"), "{text}");

    if fx.routines {
        let routines = meta::routines(&live.rows(MetaKey::Routines, &args(s)).await).unwrap();
        assert!(
            routines.iter().any(|r| r.name == fx.obj("p_meta") && r.kind == RoutineKind::Procedure),
            "{routines:?}"
        );
        assert!(
            routines.iter().any(|r| r.name == fx.obj("f_meta") && r.kind == RoutineKind::Function),
            "{routines:?}"
        );
        let src = MetaArgs { name: Some(fx.obj("f_meta")), kind: Some("FUNCTION".into()), ..args(s) };
        let text = meta::source_text(MetaKey::RoutineSource, &live.rows(MetaKey::RoutineSource, &src).await)
            .unwrap()
            .unwrap();
        assert!(has(&text, "function"), "{text}");
    }

    if live.d.has_meta(MetaKey::ReservedWords) {
        live.check_reserved_words().await;
    }
    all
}

async fn check_mysql_family(live: &mut Live, fx: Fx) -> Vec<Table> {
    let all = check_common(live, fx).await;
    let emp = table(&all, "emp");
    assert_eq!(emp.columns.iter().find(|c| c.name == "name").unwrap().default.as_deref(), Some("'无名'"));
    assert!(table(&all, "part_t").partition.as_deref().unwrap().starts_with("RANGE"));
    let ds = table(&all, "ds_t");
    assert_eq!(logical(ds, "c_tinyint1"), LogicalType::Bool);
    assert_eq!(logical(ds, "c_bigint_u"), LogicalType::UInt { bits: 64 });
    assert_eq!(logical(ds, "c_decimal"), LogicalType::Decimal { precision: Some(65), scale: Some(30) });
    assert_eq!(logical(ds, "c_datetime6"), LogicalType::Timestamp { tz: false, precision: Some(6) });
    assert!(!live.d.has_meta(MetaKey::MaterializedViews) && !live.d.has_meta(MetaKey::Synonyms));
    all
}

#[tokio::test]
async fn mysql_metadata() {
    let Some(mut live) = connect("DBLENS_TEST_MYSQL_URL").await else { return };
    assert_eq!(live.d.variant(), Variant::MySql, "不是 PolarDB");
    live.run_script(include_str!("fixtures/mysql.sql")).await;
    live.run_script(include_str!("fixtures/mysql_only.sql")).await;
    let all = check_mysql_family(&mut live, Fx::lower("dblens_meta")).await;
    let func = table(&all, "emp").indexes.iter().find(|i| i.name == "idx_emp_upper_name").unwrap();
    assert!(has(&func.columns[0].name, "upper"), "{func:?}");
    assert_eq!(logical(table(&all, "ds_t"), "c_json"), LogicalType::Json);
    assert!(!live.d.has_meta(MetaKey::Sequences));
}

#[tokio::test]
async fn mariadb_metadata() {
    let Some(mut live) = connect("DBLENS_TEST_MARIADB_URL").await else { return };
    assert_eq!(live.d.variant(), Variant::MariaDb);
    live.run_script(include_str!("fixtures/mysql.sql")).await;
    live.run_script(include_str!("fixtures/mariadb_only.sql")).await;
    let all = check_mysql_family(&mut live, Fx::lower("dblens_meta")).await;
    // MariaDB 的 JSON 是带检查约束的 longtext，这条隐式检查约束不应出现在 checks 中
    assert!(table(&all, "ds_t").checks.is_empty(), "{:?}", table(&all, "ds_t").checks);
    assert!(!live.d.has_meta(MetaKey::ReservedWords), "MariaDB 的 KEYWORDS 不区分是否保留");
    let seq = meta::sequences(&live.rows(MetaKey::Sequences, &args("dblens_meta")).await).unwrap();
    assert!(seq.iter().any(|q| q.name == "seq_meta"), "{seq:?}");
}

/// SQL Server 没有 BEFORE 触发器，自引用外键不能级联，不按 `check_common` 检查，逐项核对。
#[tokio::test]
async fn sqlserver_metadata() {
    let Some(mut live) = connect("DBLENS_TEST_SQLSERVER_URL").await else { return };
    assert_eq!(live.d.variant(), Variant::SqlServer);
    live.run_script(include_str!("fixtures/sqlserver.sql")).await;
    let s = "dblens_meta";

    let catalogs = meta::schemas(&live.rows(MetaKey::Catalogs, &MetaArgs::default()).await).unwrap();
    assert!(catalogs.iter().any(|c| c.name == "master" && c.is_system), "{catalogs:?}");
    assert!(catalogs.iter().any(|c| !c.is_system), "{catalogs:?}");
    let schemas = meta::schemas(&live.rows(MetaKey::Schemas, &MetaArgs::default()).await).unwrap();
    assert!(schemas.iter().any(|x| x.name == s && !x.is_system), "{schemas:?}");
    assert!(schemas.iter().any(|x| x.name == "sys" && x.is_system), "{schemas:?}");
    assert!(schemas.iter().any(|x| x.name == "db_owner" && x.is_system), "{schemas:?}");

    let by_comment = MetaArgs { filter: Some("员工".into()), ..args(s) };
    let hit = meta::objects(MetaKey::Tables, &live.rows(MetaKey::Tables, &by_comment).await).unwrap();
    assert_eq!(hit.iter().map(|o| o.name.as_str()).collect::<Vec<_>>(), ["emp"]);

    let all = live.tables(s, None).await;
    let dept = table(&all, "dept");
    assert_eq!(dept.comment.as_deref(), Some("部门表"));
    assert!(dept.columns[0].auto_increment && !dept.columns[0].nullable, "{:?}", dept.columns[0]);
    assert_eq!(dept.primary_key.as_ref().unwrap().columns, ["id"]);
    assert_eq!(dept.uniques[0].columns, ["name"]);

    let emp = table(&all, "emp");
    let col = |n: &str| emp.columns.iter().find(|c| c.name == n).unwrap_or_else(|| panic!("emp 没有列 {n}"));
    let name = col("name");
    assert!(!name.nullable && name.default.as_deref().unwrap().contains("无名"), "{name:?}");
    assert_eq!(name.comment.as_deref(), Some("员工姓名"));
    assert_eq!(name.data_type.native, "nvarchar(100)");
    assert!(has(col("name_upper").generated.as_deref().unwrap(), "upper"));
    assert_eq!(col("salary").data_type.native, "decimal(10,2)");
    assert_eq!(col("created").data_type.logical, LogicalType::Timestamp { tz: false, precision: Some(6) });
    assert_eq!(col("bio").data_type.native, "nvarchar(max)");
    let fk = emp.foreign_keys.iter().find(|f| f.ref_table == "dept").unwrap();
    assert_eq!((fk.columns.clone(), fk.on_delete.as_deref()), (vec!["dept_id".to_string()], Some("CASCADE")));
    assert_eq!(fk.ref_schema.as_deref(), Some(s));
    let mgr = emp.foreign_keys.iter().find(|f| f.ref_table == "emp").unwrap();
    assert_eq!(mgr.on_delete.as_deref(), Some("NO ACTION"));
    assert_eq!(emp.uniques[0].columns, ["dept_id", "name"]);
    assert!(emp.checks.iter().any(|c| has(&c.expression, "salary")), "{:?}", emp.checks);
    let created = emp.indexes.iter().find(|i| i.name == "idx_emp_created").unwrap();
    assert!(
        created.columns[0].descending && created.method.as_deref() == Some("NONCLUSTERED"),
        "{created:?}"
    );
    // INCLUDE 列不算索引列
    let salary = emp.indexes.iter().find(|i| i.name == "idx_emp_salary").unwrap();
    assert_eq!(salary.columns.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["salary"]);
    let trg = &emp.triggers[0];
    assert_eq!(trg.timing.as_deref(), Some("AFTER"));
    assert!(
        trg.events.contains(&"INSERT".to_string()) && trg.events.contains(&"UPDATE".to_string()),
        "{trg:?}"
    );
    assert!(has(trg.definition.as_deref().unwrap(), "create trigger"));
    let one = live.tables(s, Some("emp")).await;
    assert_eq!(&one[0], emp);

    let mixed = table(&all, "Mixed Case");
    assert_eq!(live.d.qualified(Some(s), &mixed.name), "dblens_meta.[Mixed Case]");
    assert!(mixed.columns.iter().any(|c| c.name == "数洞") && mixed.primary_key.is_some());

    let ds = table(&all, "ds_t");
    assert_eq!(logical(ds, "c_bit"), LogicalType::Bool);
    assert_eq!(logical(ds, "c_tinyint"), LogicalType::UInt { bits: 8 });
    assert_eq!(logical(ds, "c_money"), LogicalType::Decimal { precision: Some(19), scale: Some(4) });
    assert_eq!(logical(ds, "c_varchar_max"), LogicalType::VarChar { len: None });
    assert_eq!(logical(ds, "c_nchar"), LogicalType::Char { len: Some(4) });
    assert_eq!(logical(ds, "c_time"), LogicalType::Time { precision: Some(3) });
    assert_eq!(logical(ds, "c_dto"), LogicalType::Timestamp { tz: true, precision: Some(7) });
    assert_eq!(logical(ds, "c_uid"), LogicalType::Uuid);
    assert_eq!(logical(ds, "c_geo"), LogicalType::Other);

    let d = live.d;
    let columns = live.rows(MetaKey::Columns, &args(s)).await;
    let views =
        meta::views(&d, MetaKey::Views, &live.rows(MetaKey::Views, &args(s)).await, &columns).unwrap();
    assert_eq!(views.iter().find(|v| v.name == "v_emp").unwrap().columns.len(), 3);
    let def = MetaArgs { name: Some("v_emp".into()), ..args(s) };
    let text = meta::source_text(MetaKey::ViewDefinition, &live.rows(MetaKey::ViewDefinition, &def).await)
        .unwrap()
        .unwrap();
    assert!(has(&text, "create view") && has(&text, "dept"), "{text}");

    let routines = meta::routines(&live.rows(MetaKey::Routines, &args(s)).await).unwrap();
    assert!(routines.iter().any(|r| r.name == "p_meta" && r.kind == RoutineKind::Procedure), "{routines:?}");
    assert!(routines.iter().any(|r| r.name == "f_meta" && r.kind == RoutineKind::Function), "{routines:?}");
    let src = MetaArgs { name: Some("f_meta".into()), kind: Some("FUNCTION".into()), ..args(s) };
    let text = meta::source_text(MetaKey::RoutineSource, &live.rows(MetaKey::RoutineSource, &src).await)
        .unwrap()
        .unwrap();
    assert!(has(&text, "create function"), "{text}");

    let seq = meta::sequences(&live.rows(MetaKey::Sequences, &args(s)).await).unwrap();
    let q = seq.iter().find(|q| q.name == "seq_meta").unwrap();
    assert_eq!(
        (q.start.as_deref(), q.increment.as_deref(), q.cycle, q.cache),
        (Some("100"), Some("5"), true, Some(10))
    );
    let syn = meta::synonyms(&live.rows(MetaKey::Synonyms, &args(s)).await).unwrap();
    assert_eq!(syn.iter().find(|x| x.name == "syn_emp").unwrap().target_name, "emp");

    let t = |table: &str| MetaArgs { table: Some(table.into()), ..args(s) };
    let refs =
        meta::referencing_keys(&live.rows(MetaKey::ReferencingKeys, &t("dept")).await, s, "dept").unwrap();
    assert!(refs.iter().any(|r| r.table == "emp"), "{refs:?}");
    let deps = meta::dependent_views(&live.rows(MetaKey::DependentViews, &t("emp")).await).unwrap();
    assert!(deps.iter().any(|v| v.name == "v_emp"), "{deps:?}");
    meta::grants(&live.rows(MetaKey::Grants, &t("emp")).await).unwrap();
    let storage = meta::table_storage(&live.rows(MetaKey::TableStorage, &t("dept")).await).unwrap();
    assert_eq!(storage.row_count, Some(2), "{storage:?}");
    assert!(!live.d.has_meta(MetaKey::MaterializedViews) && !live.d.has_meta(MetaKey::ReservedWords));
}

#[tokio::test]
async fn postgres_metadata() {
    let Some(mut live) = connect("DBLENS_TEST_PG_URL").await else { return };
    live.run_script(include_str!("fixtures/postgres.sql")).await;
    check_pg_family(&mut live, PgFx::FULL).await;
}

/// PG 家族各库支持的程度：物化视图（人大金仓开发版的授权不允许建）、序列的缓存大小（openGauss 读不到）。
#[derive(Clone, Copy)]
struct PgFx {
    materialized_views: bool,
    sequence_cache: bool,
}

impl PgFx {
    const FULL: PgFx = PgFx { materialized_views: true, sequence_cache: true };
}

/// PG 家族的检查：先做共有的检查，再核对 PG 特有的对象。
async fn check_pg_family(live: &mut Live, pg: PgFx) {
    let schemas = meta::schemas(&live.rows(MetaKey::Schemas, &MetaArgs::default()).await).unwrap();
    let builtin = match live.d.variant() {
        Variant::Kingbase => "sys_catalog",
        Variant::OpenGauss => "dbe_perf",
        _ => "pg_catalog",
    };
    assert!(schemas.iter().any(|x| x.name == builtin && x.is_system), "{schemas:?}");
    assert!(schemas.iter().any(|x| x.name == "public" && !x.is_system), "{schemas:?}");
    let fx = Fx::lower("dblens_meta");
    let all = check_common(live, fx).await;
    let s = fx.schema;

    assert!(
        table(&all, "emp").columns.iter().find(|c| c.name == "serial_no").unwrap().auto_increment,
        "serial 列"
    );
    let list = meta::objects(MetaKey::Tables, &live.rows(MetaKey::Tables, &args(s)).await).unwrap();
    assert!(!list.iter().any(|o| o.name == "part_t_2024"), "分区子表不列出");
    assert_eq!(table(&all, "part_t").partition.as_deref(), Some("RANGE (created)"));
    let func = table(&all, "emp").indexes.iter().find(|i| i.name == "idx_emp_upper").unwrap();
    assert!(func.columns[0].name.contains("lower") && func.columns[1].name == "dept_id");

    let ds = table(&all, "ds_t");
    assert_eq!(logical(ds, "c_numeric_p"), LogicalType::Decimal { precision: Some(10), scale: Some(2) });
    assert_eq!(logical(ds, "c_varchar"), LogicalType::VarChar { len: Some(255) });
    assert_eq!(logical(ds, "c_ts"), LogicalType::Timestamp { tz: false, precision: Some(6) });
    assert_eq!(logical(ds, "c_tstz3"), LogicalType::Timestamp { tz: true, precision: Some(3) });
    assert_eq!(logical(ds, "c_jsonb"), LogicalType::Json);
    assert_eq!(logical(ds, "c_int_arr"), LogicalType::Other);

    let d = live.d;
    let columns = live.rows(MetaKey::Columns, &args(s)).await;
    let views =
        meta::views(&d, MetaKey::Views, &live.rows(MetaKey::Views, &args(s)).await, &columns).unwrap();
    assert_eq!(views.iter().find(|v| v.name == "v_emp").unwrap().comment.as_deref(), Some("员工视图"));
    if pg.materialized_views {
        let mv_rows = live.rows(MetaKey::MaterializedViews, &args(s)).await;
        let mv = meta::views(&d, MetaKey::MaterializedViews, &mv_rows, &columns).unwrap();
        let cnt = mv[0].columns.iter().find(|c| c.name == "cnt").unwrap();
        assert_eq!(cnt.comment.as_deref(), Some("员工数"));
    }

    let seq = meta::sequences(&live.rows(MetaKey::Sequences, &args(s)).await).unwrap();
    assert_eq!(seq.len(), 1, "serial/identity 自带的序列不列出：{seq:?}");
    let q = &seq[0];
    let cache = pg.sequence_cache.then_some(10);
    assert_eq!(
        (q.start.as_deref(), q.increment.as_deref(), q.cycle, q.cache),
        (Some("100"), Some("5"), true, cache)
    );

    let routines = meta::routines(&live.rows(MetaKey::Routines, &args(s)).await).unwrap();
    assert!(!routines.iter().any(|r| r.name == "sum_meta"), "聚合函数不列出");
    let src = MetaArgs { name: Some("f_meta".into()), kind: Some("FUNCTION".into()), ..args(s) };
    let text = meta::source_text(MetaKey::RoutineSource, &live.rows(MetaKey::RoutineSource, &src).await)
        .unwrap()
        .unwrap();
    assert_eq!(text.matches("CREATE OR REPLACE FUNCTION").count(), 2, "两个重载：{text}");
    // 人大金仓、openGauss 有同义词（check_oracle_mode_objects）
    assert_eq!(live.d.has_meta(MetaKey::Synonyms), live.d.variant() != Variant::Postgres);
}

#[tokio::test]
async fn oracle_metadata() {
    let Some(mut live) = connect("DBLENS_TEST_ORACLE_URL").await else { return };
    live.run_script(include_str!("fixtures/oracle.sql")).await;
    let fx = Fx::upper("DBLENS_TEST");
    let all = check_common(&mut live, fx).await;
    let s = fx.schema;

    let emp = table(&all, "META_EMP");
    let col = |n: &str| emp.columns.iter().find(|c| c.name == n).unwrap_or_else(|| panic!("没有列 {n}"));
    // Oracle 在 LONG 类型的默认值后面存了空白，组装时要去掉
    assert_eq!(col("NAME").default.as_deref(), Some("'无名'"));
    assert!(col("SEQ_NO").auto_increment && col("SEQ_NO").default.is_none(), "标识列");
    assert!(!emp.columns.iter().any(|c| c.name.starts_with("SYS_")), "隐藏的系统列不列出");
    assert_eq!(logical(emp, "NAME"), LogicalType::VarChar { len: Some(100) });
    assert!(emp.checks.iter().all(|c| !c.expression.ends_with("IS NOT NULL")), "{:?}", emp.checks);
    assert!(table(&all, "META_PART_T").partition.as_deref().unwrap().starts_with("RANGE"));

    let seq = meta::sequences(&live.rows(MetaKey::Sequences, &args(s)).await).unwrap();
    assert!(seq.iter().all(|q| !q.name.starts_with("ISEQ1584")), "标识列自带的序列不列出：{seq:?}");
    let q = seq.iter().find(|q| q.name == "META_SEQ").unwrap();
    assert_eq!((q.increment.as_deref(), q.cycle), (Some("5"), true));
    let syn = meta::synonyms(&live.rows(MetaKey::Synonyms, &args(s)).await).unwrap();
    assert!(syn.iter().any(|x| x.name == "META_SYN_EMP" && x.target_name == "META_EMP"), "{syn:?}");
    let routines = meta::routines(&live.rows(MetaKey::Routines, &args(s)).await).unwrap();
    assert!(routines.iter().any(|r| r.kind == RoutineKind::PackageBody), "{routines:?}");
    let d = live.d;
    let columns = live.rows(MetaKey::Columns, &args(s)).await;
    let mv_rows = live.rows(MetaKey::MaterializedViews, &args(s)).await;
    let mv = meta::views(&d, MetaKey::MaterializedViews, &mv_rows, &columns).unwrap();
    assert!(!mv.is_empty());
}

#[tokio::test]
async fn dameng_metadata() {
    let Some(mut live) = connect("DBLENS_TEST_DM_URL").await else { return };
    live.run_script(include_str!("fixtures/dameng.sql")).await;
    let fx = Fx::upper("DBLENS_TEST");
    let all = check_common(&mut live, fx).await;
    let s = fx.schema;

    assert!(table(&all, "META_PART_T").partition.as_deref().unwrap().starts_with("RANGE"));
    let list = meta::objects(MetaKey::Tables, &live.rows(MetaKey::Tables, &args(s)).await).unwrap();
    assert!(
        !list.iter().any(|o| o.name.starts_with("BM$_") || o.name.starts_with("MTAB$_")),
        "内部存储表不列出"
    );
    let emp = table(&all, "META_EMP");
    assert!(emp.indexes.iter().any(|i| i.columns.iter().any(|c| has(&c.name, "upper"))), "函数索引");
    assert!(emp.indexes.iter().all(|i| i.name != "META_PK_EMP"), "主键背后的索引不列出");

    let seq = meta::sequences(&live.rows(MetaKey::Sequences, &args(s)).await).unwrap();
    let q = seq.iter().find(|q| q.name == "META_SEQ_META").unwrap();
    assert_eq!(
        (q.start.as_deref(), q.increment.as_deref(), q.cycle, q.cache),
        (Some("100"), Some("5"), true, Some(10))
    );
    let syn = meta::synonyms(&live.rows(MetaKey::Synonyms, &args(s)).await).unwrap();
    assert!(syn.iter().any(|x| x.name == "META_SYN_EMP" && x.target_name == "META_EMP"), "{syn:?}");
    let routines = meta::routines(&live.rows(MetaKey::Routines, &args(s)).await).unwrap();
    assert!(routines.iter().any(|r| r.name == "META_PKG_META" && r.kind == RoutineKind::PackageBody));
    let d = live.d;
    let columns = live.rows(MetaKey::Columns, &args(s)).await;
    let mv_rows = live.rows(MetaKey::MaterializedViews, &args(s)).await;
    let mv = meta::views(&d, MetaKey::MaterializedViews, &mv_rows, &columns).unwrap();
    assert_eq!(mv.iter().find(|v| v.name == "META_MV_DEPT").unwrap().columns.len(), 2);
}

/// MySQL 脚本里 TiDB 不支持的段：触发器、存储过程和函数、全文索引；ds_t 去掉 GEOMETRY 列。
const TIDB_SKIP: [&str; 4] = ["trg", "proc", "func", "idx_fulltext"];

fn without_geometry(script: &str) -> String {
    let ds = script.lines().find(|l| l.starts_with("CREATE TABLE dblens_meta.ds_t")).unwrap();
    ds.replace(", c_geometry GEOMETRY", "")
}

#[tokio::test]
async fn tidb_metadata() {
    let Some(mut live) = connect("DBLENS_TEST_TIDB_URL").await else { return };
    assert_eq!(live.d.variant(), Variant::TiDb);
    // CHECK 约束默认只解析不保存，打开后才能读回
    live.run_script(
        "-- @@ check_on
SET GLOBAL tidb_enable_check_constraint = ON",
    )
    .await;
    let script = include_str!("fixtures/mysql.sql");
    let ds = without_geometry(script);
    live.run_script_with(script, &TIDB_SKIP, &[("ds_t", &ds)]).await;
    let fx = Fx { triggers: false, routines: false, descending: false, ..Fx::lower("dblens_meta") };
    live.run_script(TIDB_ONLY).await;
    let all = check_mysql_family(&mut live, fx).await;
    assert!(table(&all, "emp").triggers.is_empty());

    let ar = table(&all, "ar_t");
    assert!(ar.columns[0].auto_increment && !ar.columns[1].auto_increment, "AUTO_RANDOM：{ar:?}");
    let seq = meta::sequences(&live.rows(MetaKey::Sequences, &args("dblens_meta")).await).unwrap();
    let q = seq.iter().find(|q| q.name == "seq_meta").unwrap();
    assert_eq!(
        (q.start.as_deref(), q.increment.as_deref(), q.cycle, q.cache),
        (Some("100"), Some("5"), true, Some(10))
    );
    for (t, clustered) in [("ar_t", true), ("nc_t", false)] {
        let a = MetaArgs { table: Some(t.into()), ..args("dblens_meta") };
        let storage = meta::table_storage(&live.rows(MetaKey::TableStorage, &a).await).unwrap();
        assert_eq!(storage.clustered, Some(clustered), "{t}");
    }
}

/// 只在 TiDB 上建的对象：AUTO_RANDOM 主键、非聚簇主键、序列。
const TIDB_ONLY: &str = "-- @@ ar_t
CREATE TABLE dblens_meta.ar_t (id BIGINT AUTO_RANDOM PRIMARY KEY, v INT)
-- @@ nc_t
CREATE TABLE dblens_meta.nc_t (id INT PRIMARY KEY NONCLUSTERED, v INT)
-- @@ seq
CREATE SEQUENCE dblens_meta.seq_meta START 100 INCREMENT 5 CACHE 10 CYCLE
";

#[tokio::test]
async fn oceanbase_metadata() {
    let Some(mut live) = connect("DBLENS_TEST_OCEANBASE_URL").await else { return };
    assert_eq!(live.d.variant(), Variant::OceanBase);
    let script = include_str!("fixtures/mysql.sql");
    let ds = without_geometry(script);
    live.run_script_with(script, &[], &[("ds_t", &ds)]).await;
    let all = check_mysql_family(&mut live, Fx { descending: false, ..Fx::lower("dblens_meta") }).await;
    // 全文索引在 STATISTICS 中只有隐藏列（__doc_id_*、__word_segment_*），去掉后不列出
    let emp = table(&all, "emp");
    assert!(emp.indexes.iter().all(|i| i.columns.iter().all(|c| !c.name.starts_with("__"))), "{emp:?}");
}

#[tokio::test]
async fn opengauss_metadata() {
    let Some(mut live) = connect("DBLENS_TEST_OPENGAUSS_URL").await else { return };
    // 这个目标也可以指向 GaussDB：两者用同一份查询
    assert!(matches!(live.d.variant(), Variant::OpenGauss | Variant::GaussDb), "{:?}", live.d.variant());
    assert!(!live.d.oracle_mode());
    let script = include_str!("fixtures/opengauss.sql");
    if live.d.variant() == Variant::GaussDb {
        live.run_script(&script.replace("EXECUTE FUNCTION", "EXECUTE PROCEDURE")).await;
    } else {
        live.run_script(script).await;
    }
    check_pg_family(&mut live, PgFx { sequence_cache: false, ..PgFx::FULL }).await;
}

#[tokio::test]
async fn opengauss_a_metadata() {
    let Some(mut live) = connect("DBLENS_TEST_OPENGAUSS_A_URL").await else { return };
    assert_eq!((live.d.variant(), live.d.oracle_mode()), (Variant::OpenGauss, true));
    live.run_script(include_str!("fixtures/opengauss.sql")).await;
    check_pg_family(&mut live, PgFx { sequence_cache: false, ..PgFx::FULL }).await;
    check_oracle_mode_objects(&mut live).await;
}

/// 人大金仓沿用 PG 的脚本：开发版授权不允许建物化视图；PG 模式不接受负的小数位。
async fn kingbase_metadata(env: &str, oracle_mode: bool) {
    let Some(mut live) = connect(env).await else { return };
    assert_eq!((live.d.variant(), live.d.oracle_mode()), (Variant::Kingbase, oracle_mode));
    let script = include_str!("fixtures/postgres.sql");
    let ds =
        script.lines().skip_while(|l| !l.starts_with("CREATE TABLE dblens_meta.ds_t")).collect::<Vec<_>>();
    let ds = ds
        .join(
            "
",
        )
        .replace(", c_numeric_neg numeric(5,-2)", "");
    let skip = ["mv_dept", "mv_dept_c", "mv_dept_cc"];
    live.run_script_with(script, &skip, &[("ds_t", &ds)]).await;
    check_pg_family(&mut live, PgFx { materialized_views: false, ..PgFx::FULL }).await;
    if oracle_mode {
        check_oracle_mode_objects(&mut live).await;
    }
}

#[tokio::test]
async fn kingbase_oracle_metadata() {
    kingbase_metadata("DBLENS_TEST_KINGBASE_URL", true).await;
}

#[tokio::test]
async fn kingbase_pg_metadata() {
    kingbase_metadata("DBLENS_TEST_KINGBASE_PG_URL", false).await;
}

/// Oracle 兼容模式（人大金仓、openGauss 的 A 模式）：同义词与包。
const ORACLE_MODE_ONLY: &str = "-- @@ syn
CREATE OR REPLACE SYNONYM dblens_meta.syn_emp FOR dblens_meta.emp
-- @@ pkg
CREATE OR REPLACE PACKAGE dblens_meta.pkg_meta AS FUNCTION f(a integer) RETURN integer; END pkg_meta;
-- @@ pkg_body
CREATE OR REPLACE PACKAGE BODY dblens_meta.pkg_meta AS FUNCTION f(a integer) RETURN integer IS BEGIN RETURN a + 1; END; END pkg_meta;
";

async fn check_oracle_mode_objects(live: &mut Live) {
    live.run_script(ORACLE_MODE_ONLY).await;
    let s = "dblens_meta";
    let syn = meta::synonyms(&live.rows(MetaKey::Synonyms, &args(s)).await).unwrap();
    assert!(
        syn.iter()
            .any(|x| x.name == "syn_emp" && x.target_schema.as_deref() == Some(s) && x.target_name == "emp"),
        "{syn:?}"
    );
    let routines = meta::routines(&live.rows(MetaKey::Routines, &args(s)).await).unwrap();
    for kind in [RoutineKind::Package, RoutineKind::PackageBody] {
        assert!(routines.iter().any(|r| r.name == "pkg_meta" && r.kind == kind), "{routines:?}");
    }
    assert!(!routines.iter().any(|r| r.name == "f"), "包里的函数不单独列出：{routines:?}");
    for (kind, head) in
        [("PACKAGE", "CREATE OR REPLACE PACKAGE "), ("PACKAGE BODY", "CREATE OR REPLACE PACKAGE BODY ")]
    {
        let a = MetaArgs { name: Some("pkg_meta".into()), kind: Some(kind.into()), ..args(s) };
        let text = meta::source_text(MetaKey::RoutineSource, &live.rows(MetaKey::RoutineSource, &a).await)
            .unwrap()
            .unwrap_or_else(|| panic!("没有 {kind} 的源码"));
        assert!(
            text.starts_with(head) && has(&text, "RETURN integer") && text.trim_end().ends_with(';'),
            "{text}"
        );
    }
}
