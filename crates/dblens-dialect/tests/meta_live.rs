//! 元数据查询的实库测试。设置对应的环境变量时运行，否则直接通过（与驱动一致性测试相同的约定）：
//! DBLENS_TEST_MYSQL_URL、DBLENS_TEST_MARIADB_URL、DBLENS_TEST_PG_URL、DBLENS_TEST_ORACLE_URL、DBLENS_TEST_DM_URL。
//!
//! 每个家族先执行 `tests/fixtures/<家族>.sql` 建测试对象，再用全部元数据查询读回来逐项核对。

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
        let mut sections: Vec<String> = Vec::new();
        for line in text.lines() {
            if line.starts_with("-- @@") {
                sections.push(String::new());
            } else if let Some(last) = sections.last_mut() {
                last.push_str(line);
                last.push('\n');
            }
        }
        for sql in sections {
            let sql = sql.trim();
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
#[derive(Clone, Copy)]
struct Fx {
    schema: &'static str,
    upper: bool,
}

impl Fx {
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
    assert!(created.columns[0].descending);
    let trg = emp.triggers.iter().find(|t| t.timing.as_deref() == Some("BEFORE")).expect("BEFORE 触发器");
    assert!(trg.events.contains(&"INSERT".to_string()), "{trg:?}");

    // 单表明细与整个 schema 中的同一张表一致
    let one = live.tables(s, Some(&emp_name)).await;
    assert_eq!(one.len(), 1);
    assert_eq!(&one[0], emp);

    let mixed = all.iter().find(|t| t.name.ends_with("Mixed Case")).expect("表 Mixed Case");
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

    if live.d.has_meta(MetaKey::ReservedWords) {
        live.check_reserved_words().await;
    }
    all
}

async fn check_mysql_family(live: &mut Live) -> Vec<Table> {
    let fx = Fx { schema: "dblens_meta", upper: false };
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
    let all = check_mysql_family(&mut live).await;
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
    let all = check_mysql_family(&mut live).await;
    // MariaDB 的 JSON 是带检查约束的 longtext，这条隐式检查约束不应出现在 checks 中
    assert!(table(&all, "ds_t").checks.is_empty(), "{:?}", table(&all, "ds_t").checks);
    assert!(!live.d.has_meta(MetaKey::ReservedWords), "MariaDB 的 KEYWORDS 不区分是否保留");
    let seq = meta::sequences(&live.rows(MetaKey::Sequences, &args("dblens_meta")).await).unwrap();
    assert!(seq.iter().any(|q| q.name == "seq_meta"), "{seq:?}");
}

#[tokio::test]
async fn postgres_metadata() {
    let Some(mut live) = connect("DBLENS_TEST_PG_URL").await else { return };
    live.run_script(include_str!("fixtures/postgres.sql")).await;
    let fx = Fx { schema: "dblens_meta", upper: false };
    let all = check_common(&mut live, fx).await;
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
    let mv_rows = live.rows(MetaKey::MaterializedViews, &args(s)).await;
    let mv = meta::views(&d, MetaKey::MaterializedViews, &mv_rows, &columns).unwrap();
    assert_eq!(mv[0].columns.iter().find(|c| c.name == "cnt").unwrap().comment.as_deref(), Some("员工数"));

    let seq = meta::sequences(&live.rows(MetaKey::Sequences, &args(s)).await).unwrap();
    assert_eq!(seq.len(), 1, "serial/identity 自带的序列不列出：{seq:?}");
    let q = &seq[0];
    assert_eq!(
        (q.start.as_deref(), q.increment.as_deref(), q.cycle, q.cache),
        (Some("100"), Some("5"), true, Some(10))
    );

    let routines = meta::routines(&live.rows(MetaKey::Routines, &args(s)).await).unwrap();
    assert!(!routines.iter().any(|r| r.name == "sum_meta"), "聚合函数不列出");
    let src = MetaArgs { name: Some("f_meta".into()), kind: Some("FUNCTION".into()), ..args(s) };
    let text = meta::source_text(MetaKey::RoutineSource, &live.rows(MetaKey::RoutineSource, &src).await)
        .unwrap()
        .unwrap();
    assert_eq!(text.matches("CREATE OR REPLACE FUNCTION").count(), 2, "两个重载：{text}");
    assert!(!live.d.has_meta(MetaKey::Synonyms));
}

#[tokio::test]
async fn oracle_metadata() {
    let Some(mut live) = connect("DBLENS_TEST_ORACLE_URL").await else { return };
    live.run_script(include_str!("fixtures/oracle.sql")).await;
    let fx = Fx { schema: "DBLENS_TEST", upper: true };
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
    let fx = Fx { schema: "DBLENS_TEST", upper: true };
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
