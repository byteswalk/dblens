//! 元数据查询的实库测试。设置对应的环境变量时运行，否则直接通过（与驱动一致性测试相同的约定）：
//! DBLENS_TEST_MYSQL_URL、DBLENS_TEST_MARIADB_URL、DBLENS_TEST_PG_URL、DBLENS_TEST_ORACLE_URL、DBLENS_TEST_DM_URL；
//! 国产库第一批：DBLENS_TEST_TIDB_URL、DBLENS_TEST_OCEANBASE_URL、DBLENS_TEST_OPENGAUSS_URL、
//! DBLENS_TEST_OPENGAUSS_A_URL、DBLENS_TEST_KINGBASE_URL、DBLENS_TEST_KINGBASE_PG_URL；
//! SQL Server：DBLENS_TEST_SQLSERVER_URL（库写测试库，如 `/dblens_test`；元数据查询在连接的当前库中执行）；
//! GBase 8s：DBLENS_TEST_GBASE_URL，另需 DBLENS_JAVA、DBLENS_BRIDGE_JAR。
//!
//! 每个家族先执行 `tests/fixtures/<家族>.sql` 建测试对象，再用全部元数据查询读回来逐项核对。
//! 兼容库沿用所属家族的脚本，跳过不支持的段、替换写法不同的段（`run_script_with`）。

use std::time::Instant;

use dblens_core::schema::{RoutineKind, Table};
use dblens_core::{LogicalType, Value};
use dblens_dialect::meta::{self, MetaArgs, MetaKey, MetaRows, TableRows};
use dblens_dialect::{Compat, Dialect, Family, Variant};
use dblens_driver::{ConnectParams, Connection, Driver, DriverId, collect_all};

struct Live {
    conn: Box<dyn Connection>,
    d: Dialect,
    /// 服务器报告的版本串（Oracle 是 banner，能看出 XE 版）。
    banner: String,
}

async fn fetch(conn: &mut Box<dyn Connection>, sql: &str, params: &[Value]) -> MetaRows {
    let mut stream = conn.execute(sql, params).await.unwrap_or_else(|e| panic!("{e}\n{sql}"));
    let events = collect_all(stream.as_mut(), 10_000).await.unwrap_or_else(|e| panic!("{e}\n{sql}"));
    MetaRows::from_events(events).unwrap()
}

async fn connect(env: &str) -> Option<Live> {
    Some(connect_url(&std::env::var(env).ok()?).await)
}

async fn connect_url(url: &str) -> Live {
    let (id, params) = ConnectParams::from_url(url).unwrap();
    let driver: Box<dyn Driver> = match id {
        DriverId::Mysql => Box::new(dblens_driver_mysql::MySqlDriver),
        DriverId::Postgres => Box::new(dblens_driver_postgres::PgDriver),
        DriverId::Oracle => Box::new(dblens_driver_oracle::OracleDriver),
        DriverId::Dameng => Box::new(dblens_driver_jdbc::DamengJdbcDriver),
        DriverId::SqlServer => Box::new(dblens_driver_sqlserver::SqlServerDriver),
        DriverId::Informix => Box::new(dblens_driver_jdbc::GbaseDriver),
        DriverId::Sqlite => Box::new(dblens_driver_sqlite::SqliteDriver),
    };
    let mut conn = driver.connect(&params).await.unwrap();
    let info = conn.server_info().await.unwrap();
    let mut d = Dialect::detect(id, &info);
    if let Some(q) = d.meta_query(MetaKey::VariantProbe, &MetaArgs::default()).unwrap() {
        let probe = fetch(&mut conn, &q.sql, &q.params).await;
        d = d.refine(&probe);
    }
    for sql in d.session_setup() {
        let mut stream = conn.execute(sql, &[]).await.unwrap();
        collect_all(stream.as_mut(), 1).await.unwrap();
    }
    Live { conn, d, banner: info.version }
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
        // MySQL 8.0.13 以前没有 KEYWORDS 表，查询按设计返回空
        let old_mysql = self.d.variant() == Variant::MySql && self.d.version().is_some_and(|v| v < 80013);
        assert!(
            words.len() > 50 || (old_mysql && words.is_empty()),
            "{:?} 只取到 {} 个保留字",
            self.d.variant(),
            words.len()
        );
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
    /// dept.id 是自增列。Oracle 12c 以前没有标识列。
    identity: bool,
    /// 夹具里小写的列名存成大写。达梦大小写不敏感的库按书写的原样存。
    upper_cols: bool,
    /// 检查约束能读回。MySQL 8.0.16 以前只解析不保存。
    checks: bool,
    /// 有生成列 name_upper（人大金仓 SQL Server 模式没有生成列）。
    generated: bool,
}

impl Fx {
    fn lower(schema: &'static str) -> Fx {
        Fx {
            schema,
            upper: false,
            triggers: true,
            routines: true,
            descending: true,
            identity: true,
            upper_cols: false,
            checks: true,
            generated: true,
        }
    }

    fn upper(schema: &'static str) -> Fx {
        Fx {
            schema,
            upper: true,
            triggers: true,
            routines: true,
            descending: true,
            identity: true,
            upper_cols: true,
            checks: true,
            generated: true,
        }
    }

    fn obj(&self, name: &str) -> String {
        if self.upper { format!("META_{}", name.to_ascii_uppercase()) } else { name.to_string() }
    }

    fn col(&self, name: &str) -> String {
        if self.upper_cols { name.to_ascii_uppercase() } else { name.to_string() }
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
    assert!(id.name == fx.col("id") && id.auto_increment == fx.identity && !id.nullable, "{id:?}");
    assert_eq!(dept.primary_key.as_ref().unwrap().columns, [fx.col("id")]);
    assert_eq!(dept.uniques[0].columns, [fx.col("name")]);

    let emp = table(&all, &emp_name);
    let col = |n: &str| {
        emp.columns.iter().find(|c| c.name == fx.col(n)).unwrap_or_else(|| panic!("emp 没有列 {n}"))
    };
    let name = col("name");
    assert!(!name.nullable && name.default.as_deref().unwrap().contains("无名"), "{name:?}");
    assert_eq!(name.comment.as_deref(), Some("员工姓名"));
    if fx.generated {
        assert!(has(col("name_upper").generated.as_deref().unwrap(), "name"));
    }
    assert!(col("created").default.is_some());
    let fk = emp.foreign_keys.iter().find(|f| f.ref_table == dept_name).unwrap();
    assert_eq!((fk.columns.clone(), fk.on_delete.as_deref()), (vec![fx.col("dept_id")], Some("CASCADE")));
    assert_eq!(fk.ref_columns, [fx.col("id")]);
    let mgr = emp.foreign_keys.iter().find(|f| f.ref_table == emp_name).unwrap();
    assert_eq!(mgr.on_delete.as_deref(), Some("SET NULL"));
    // 人大金仓 MySQL 模式的 serial 按 MySQL 的含义带 UNIQUE，唯一约束不止一个
    let uk = [fx.col("dept_id"), fx.col("name")];
    assert!(emp.uniques.iter().any(|u| u.columns == uk), "{:?}", emp.uniques);
    assert_eq!(emp.checks.iter().any(|c| has(&c.expression, "salary")), fx.checks, "{:?}", emp.checks);
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
    let script = include_str!("fixtures/mysql.sql");
    // 5.7：没有函数索引、表达式默认值（8.0.13）和检查约束（8.0.16），DESC 按升序建
    if live.d.version().is_some_and(|v| v < 80000) {
        let def = script.lines().find(|l| l.starts_with("CREATE TABLE dblens_meta.def_t")).unwrap();
        let def = def.replace("DEFAULT (UUID())", "DEFAULT 'x'");
        live.run_script_with(script, &[], &[("def_t", &def)]).await;
        let fx = Fx { descending: false, checks: false, ..Fx::lower("dblens_meta") };
        let all = check_mysql_family(&mut live, fx).await;
        assert_eq!(logical(table(&all, "ds_t"), "c_json"), LogicalType::Json);
        return;
    }
    live.run_script(script).await;
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
    let script = include_str!("fixtures/postgres.sql");
    // 负的小数位数 PG 15 起才有
    if live.d.version().is_some_and(|v| v < 150000) {
        live.run_script(&script.replace(", c_numeric_neg numeric(5,-2)", "")).await;
    } else {
        live.run_script(script).await;
    }
    check_pg_family(&mut live, PgFx::FULL).await;
}

/// PG 家族各库支持的程度：物化视图（人大金仓开发版的授权不允许建）、序列的缓存大小（openGauss 读不到）。
#[derive(Clone, Copy)]
struct PgFx {
    materialized_views: bool,
    sequence_cache: bool,
    /// 人大金仓 SQL Server 模式：不建按 PG 类型写的数据类型表 ds_t（类型名的含义不同），没有生成列。
    sqlserver: bool,
}

impl PgFx {
    const FULL: PgFx = PgFx { materialized_views: true, sequence_cache: true, sqlserver: false };
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
    let fx = Fx { generated: !pg.sqlserver, ..Fx::lower("dblens_meta") };
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

    if !pg.sqlserver {
        let ds = table(&all, "ds_t");
        assert_eq!(logical(ds, "c_numeric_p"), LogicalType::Decimal { precision: Some(10), scale: Some(2) });
        assert_eq!(logical(ds, "c_varchar"), LogicalType::VarChar { len: Some(255) });
        assert_eq!(logical(ds, "c_ts"), LogicalType::Timestamp { tz: false, precision: Some(6) });
        assert_eq!(logical(ds, "c_tstz3"), LogicalType::Timestamp { tz: true, precision: Some(3) });
        assert_eq!(logical(ds, "c_jsonb"), LogicalType::Json);
        assert_eq!(logical(ds, "c_int_arr"), LogicalType::Other);
    }

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

/// 11g 的写法：没有标识列、`DEFAULT ON NULL`、隐藏列，默认值不能引用序列。
const ORACLE11_DEPT: &str = "CREATE TABLE META_DEPT (
  id NUMBER NOT NULL,
  name VARCHAR2(50) NOT NULL,
  CONSTRAINT META_DEPT_PK PRIMARY KEY (id),
  CONSTRAINT META_DEPT_NAME_UK UNIQUE (name)
)";
const ORACLE11_EMP: &str = "CREATE TABLE META_EMP (
  id NUMBER(19) NOT NULL,
  dept_id NUMBER,
  manager_id NUMBER(19),
  name VARCHAR2(100 CHAR) DEFAULT '无名' NOT NULL,
  salary NUMBER(10,2),
  created TIMESTAMP(6) DEFAULT CURRENT_TIMESTAMP,
  notes CLOB,
  photo BLOB,
  name_upper VARCHAR2(100 CHAR) GENERATED ALWAYS AS (UPPER(name)) VIRTUAL,
  status VARCHAR2(10) DEFAULT 'NEW',
  flag NUMBER(1) DEFAULT 0 NOT NULL,
  CONSTRAINT META_EMP_PK PRIMARY KEY (id),
  CONSTRAINT META_EMP_DEPT_FK FOREIGN KEY (dept_id) REFERENCES META_DEPT (id) ON DELETE CASCADE,
  CONSTRAINT META_EMP_MGR_FK FOREIGN KEY (manager_id) REFERENCES META_EMP (id) ON DELETE SET NULL,
  CONSTRAINT META_EMP_UK UNIQUE (dept_id, name),
  CONSTRAINT META_EMP_SAL_CK CHECK (salary >= 0),
  CONSTRAINT META_EMP_FLAG_CK CHECK (flag IN (0, 1))
)";
/// 11g 没有的：标识列、23ai 的类型；和虚拟列同表达式的函数索引报 ORA-54018。
const ORACLE11_SKIP: [&str; 3] = ["ds_23", "idt", "idx_upper"];
/// XE 版没有分区和位图索引。
const ORACLE_XE_SKIP: [&str; 3] = ["part", "part_list", "idx_bitmap"];

#[tokio::test]
async fn oracle_metadata() {
    let Some(mut live) = connect("DBLENS_TEST_ORACLE_URL").await else { return };
    let v11 = live.d.oracle_release().is_some_and(|v| v < 120100);
    let xe = live.banner.contains("Express Edition");
    let fixture = include_str!("fixtures/oracle.sql");
    if v11 {
        let skip: Vec<&str> =
            ORACLE11_SKIP.iter().chain(if xe { &ORACLE_XE_SKIP[..] } else { &[] }).copied().collect();
        live.run_script_with(fixture, &skip, &[("dept", ORACLE11_DEPT), ("emp", ORACLE11_EMP)]).await;
    } else {
        live.run_script(fixture).await;
    }
    let fx = Fx { identity: !v11, ..Fx::upper("DBLENS_TEST") };
    let all = check_common(&mut live, fx).await;
    let s = fx.schema;

    let emp = table(&all, "META_EMP");
    let col = |n: &str| emp.columns.iter().find(|c| c.name == n).unwrap_or_else(|| panic!("没有列 {n}"));
    // Oracle 在 LONG 类型的默认值后面存了空白，组装时要去掉
    assert_eq!(col("NAME").default.as_deref(), Some("'无名'"));
    if !v11 {
        assert!(col("SEQ_NO").auto_increment && col("SEQ_NO").default.is_none(), "标识列");
    }
    if !xe {
        assert!(table(&all, "META_PART_T").partition.as_deref().unwrap().starts_with("RANGE"));
    }
    assert!(!emp.columns.iter().any(|c| c.name.starts_with("SYS_")), "隐藏的系统列不列出");
    assert_eq!(logical(emp, "NAME"), LogicalType::VarChar { len: Some(100) });
    assert!(emp.checks.iter().all(|c| !c.expression.ends_with("IS NOT NULL")), "{:?}", emp.checks);

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
    // 夹具不带 schema 名，对象建在连接账号同名的 schema 下
    let url = std::env::var("DBLENS_TEST_DM_URL").unwrap();
    let user = ConnectParams::from_url(&url).unwrap().1.user.to_ascii_uppercase();
    let sensitive = fetch(&mut live.conn, "SELECT SF_GET_CASE_SENSITIVE_FLAG() AS f FROM DUAL", &[]).await;
    let upper_cols = sensitive.iter().next().and_then(|r| r.str("f")).as_deref() == Some("1");
    let fx = Fx { upper_cols, ..Fx::upper(Box::leak(user.into_boxed_str())) };
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

/// openGauss 的两个目标也可以指向 GaussDB：两者用同一份查询。夹具的触发器写 `EXECUTE PROCEDURE`，
/// openGauss 6.0 及以前和 GaussDB 都不认 `EXECUTE FUNCTION`。GaussDB 不支持表继承和域；
/// 表默认用 Ustore 存储，不支持 GIN 索引和物化视图。
async fn opengauss_family(env: &str, oracle_mode: bool) {
    let Some(mut live) = connect(env).await else { return };
    assert!(matches!(live.d.variant(), Variant::OpenGauss | Variant::GaussDb), "{:?}", live.d.variant());
    assert_eq!(live.d.oracle_mode(), oracle_mode);
    let script = include_str!("fixtures/opengauss.sql");
    let gauss = live.d.variant() == Variant::GaussDb;
    if gauss {
        let script = script.replace(", c_domain dblens_meta.posint", "");
        let skip = ["inh_child", "dom_ty", "idx_gin", "mv_dept", "mv_dept_c", "mv_dept_cc"];
        live.run_script_with(&script, &skip, &[]).await;
    } else if live.banner.contains("openGauss 5.") {
        // openGauss 5.0 不支持表继承
        live.run_script_with(script, &["inh_child"], &[]).await;
    } else {
        live.run_script(script).await;
    }
    check_pg_family(&mut live, PgFx { sequence_cache: false, materialized_views: !gauss, ..PgFx::FULL })
        .await;
    if oracle_mode {
        check_oracle_mode_objects(&mut live).await;
    }
}

#[tokio::test]
async fn opengauss_metadata() {
    opengauss_family("DBLENS_TEST_OPENGAUSS_URL", false).await;
}

#[tokio::test]
async fn opengauss_a_metadata() {
    opengauss_family("DBLENS_TEST_OPENGAUSS_A_URL", true).await;
}

/// 人大金仓沿用 PG 的脚本：开发版授权不允许建物化视图；PG 模式不接受负的小数位。
async fn kingbase_metadata(env: &str, oracle_mode: bool) {
    let Some(mut live) = connect(env).await else { return };
    assert_eq!((live.d.variant(), live.d.oracle_mode()), (Variant::Kingbase, oracle_mode));
    let mut script = include_str!("fixtures/postgres.sql").to_string();
    // SQL Server 模式：自增列写 IDENTITY，timestamp 是 rowversion，没有 text[] 这种数组写法；
    // 数据类型表 ds_t 按 PG 类型写，不建
    let sqlserver = live.d.compat() == Compat::SqlServer;
    if sqlserver {
        script = script
            .replace("GENERATED BY DEFAULT AS IDENTITY", "IDENTITY(1, 1)")
            .replace("created timestamp(6) DEFAULT", "created datetime2(6) DEFAULT")
            .replace("  tags text[],\n", "")
            .replace("  name_upper text GENERATED ALWAYS AS (upper(name)) STORED,\n", "");
    }
    let script = script.as_str();
    let ds =
        script.lines().skip_while(|l| !l.starts_with("CREATE TABLE dblens_meta.ds_t")).collect::<Vec<_>>();
    let ds = ds
        .join(
            "
",
        )
        .replace(", c_numeric_neg numeric(5,-2)", "");
    let mut skip = vec!["mv_dept", "mv_dept_c", "mv_dept_cc"];
    if sqlserver {
        skip.extend(["ds_t", "idx_gin"]);
    }
    live.run_script_with(script, &skip, &[("ds_t", &ds)]).await;
    check_pg_family(&mut live, PgFx { materialized_views: false, sqlserver, ..PgFx::FULL }).await;
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

#[tokio::test]
async fn gbase8s_metadata() {
    let Some(mut live) = connect("DBLENS_TEST_GBASE_URL").await else { return };
    assert_eq!(live.d.variant(), Variant::Gbase8s);
    live.run_script(include_str!("fixtures/gbase8s.sql")).await;
    let s = "admin";

    let catalogs = meta::schemas(&live.rows(MetaKey::Catalogs, &MetaArgs::default()).await).unwrap();
    assert!(catalogs.iter().any(|c| c.name == "sysmaster" && c.is_system), "{catalogs:?}");
    assert!(catalogs.iter().any(|c| c.name == "dblens_test" && !c.is_system), "{catalogs:?}");
    let schemas = meta::schemas(&live.rows(MetaKey::Schemas, &MetaArgs::default()).await).unwrap();
    assert!(schemas.iter().any(|x| x.name == s), "{schemas:?}");

    let by_comment = MetaArgs { filter: Some("员工".into()), ..args(s) };
    let hit = meta::objects(MetaKey::Tables, &live.rows(MetaKey::Tables, &by_comment).await).unwrap();
    assert_eq!(hit.iter().map(|o| o.name.as_str()).collect::<Vec<_>>(), ["dm_emp"]);

    let all = live.tables(s, None).await;
    let dept = table(&all, "dm_dept");
    assert_eq!(dept.comment.as_deref(), Some("部门表"));
    assert!(dept.columns[0].auto_increment && !dept.columns[0].nullable, "{:?}", dept.columns[0]);
    assert_eq!(dept.primary_key.as_ref().unwrap().columns, ["id"]);
    assert_eq!(dept.uniques[0].columns, ["name"]);

    let emp = table(&all, "dm_emp");
    let col = |n: &str| emp.columns.iter().find(|c| c.name == n).unwrap_or_else(|| panic!("没有列 {n}"));
    let name = col("name");
    assert!(!name.nullable && name.default.as_deref().unwrap().contains("无名"), "{name:?}");
    assert_eq!(name.comment.as_deref(), Some("员工姓名"));
    assert_eq!(name.data_type.native, "VARCHAR(100)");
    assert_eq!(col("id").data_type.native, "BIGSERIAL");
    assert!(col("id").auto_increment);
    assert_eq!(col("salary").data_type.native, "DECIMAL(10,2)");
    assert_eq!(col("active").data_type.logical, LogicalType::Bool);
    assert_eq!(col("created").data_type.native, "DATETIME YEAR TO FRACTION(3)");
    assert_eq!(col("clock").data_type.native, "DATETIME HOUR TO SECOND");
    assert_eq!(col("bio").data_type.logical, LogicalType::Text);
    assert_eq!(col("photo").data_type.logical, LogicalType::Blob);
    let fk = emp.foreign_keys.iter().find(|f| f.ref_table == "dm_dept").unwrap();
    assert_eq!((fk.columns.clone(), fk.on_delete.as_deref()), (vec!["dept_id".to_string()], Some("CASCADE")));
    assert_eq!(fk.ref_schema.as_deref(), Some(s));
    assert_eq!(fk.ref_columns, ["id"]);
    let mgr = emp.foreign_keys.iter().find(|f| f.ref_table == "dm_emp").unwrap();
    assert_eq!(mgr.on_delete.as_deref(), Some("NO ACTION"));
    assert_eq!(emp.uniques[0].columns, ["dept_id", "name"]);
    // 检查约束超过一块时要拼完整
    assert!(emp.checks.iter().any(|c| has(&c.expression, "99999999")), "{:?}", emp.checks);
    let idx = emp.indexes.iter().find(|i| i.name == "idx_dm_emp").unwrap();
    assert_eq!(idx.columns.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["created", "name"]);
    assert!(idx.columns[0].descending && !idx.columns[1].descending, "{idx:?}");
    assert_eq!(emp.triggers[0].name, "dm_trg");
    assert_eq!(emp.triggers[0].events, ["UPDATE"]);
    let one = live.tables(s, Some("dm_emp")).await;
    assert_eq!(&one[0], emp);

    let d = live.d;
    let columns = live.rows(MetaKey::Columns, &args(s)).await;
    let views =
        meta::views(&d, MetaKey::Views, &live.rows(MetaKey::Views, &args(s)).await, &columns).unwrap();
    assert_eq!(views.iter().find(|v| v.name == "dm_v_emp").unwrap().columns.len(), 3);
    let def = MetaArgs { name: Some("dm_v_emp".into()), ..args(s) };
    let text = meta::source_text(MetaKey::ViewDefinition, &live.rows(MetaKey::ViewDefinition, &def).await)
        .unwrap()
        .unwrap();
    assert!(has(&text, "create view") && has(&text, "dm_dept"), "{text}");

    let routines = meta::routines(&live.rows(MetaKey::Routines, &args(s)).await).unwrap();
    let kind = |n: &str| routines.iter().find(|r| r.name == n).unwrap().kind;
    assert_eq!(kind("dm_p_meta"), RoutineKind::Procedure);
    assert_eq!(kind("dm_f_meta"), RoutineKind::Function);
    let src = MetaArgs { name: Some("dm_p_meta".into()), kind: Some("PROCEDURE".into()), ..args(s) };
    let text = meta::source_text(MetaKey::RoutineSource, &live.rows(MetaKey::RoutineSource, &src).await)
        .unwrap()
        .unwrap();
    assert!(has(&text, "end procedure"), "{text}");

    let seqs = meta::sequences(&live.rows(MetaKey::Sequences, &args(s)).await).unwrap();
    let seq = seqs.iter().find(|x| x.name == "dm_seq").unwrap();
    assert_eq!(seq.start.as_deref(), Some("100"));
    assert!(seq.cycle);
    let syns = meta::synonyms(&live.rows(MetaKey::Synonyms, &args(s)).await).unwrap();
    assert_eq!(syns.iter().find(|x| x.name == "dm_syn_emp").unwrap().target_name, "dm_emp");

    let storage = MetaArgs { table: Some("dm_emp".into()), ..args(s) };
    meta::table_storage(&live.rows(MetaKey::TableStorage, &storage).await).unwrap();

    for q in live.d.ops_queries() {
        fetch(&mut live.conn, &q.sql, &[]).await;
    }
}

/// SQLite 不需要外部服务：每次都在临时文件上跑。
#[tokio::test]
async fn sqlite_metadata() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("meta.db").to_string_lossy().replace('\\', "/");
    let mut live = connect_url(&format!("sqlite:///{}?create=1", file.trim_start_matches('/'))).await;
    assert_eq!((live.d.variant(), live.d.family()), (Variant::Sqlite, Family::Sqlite));
    live.run_script(include_str!("fixtures/sqlite.sql")).await;
    let s = "main";

    let schemas = meta::schemas(&live.rows(MetaKey::Schemas, &MetaArgs::default()).await).unwrap();
    assert_eq!(schemas.iter().map(|x| x.name.as_str()).collect::<Vec<_>>(), ["main", "aux"]);
    let tables = meta::objects(MetaKey::Tables, &live.rows(MetaKey::Tables, &args(s)).await).unwrap();
    assert_eq!(
        tables.iter().map(|o| o.name.as_str()).collect::<Vec<_>>(),
        ["dept", "emp", "kv"],
        "sqlite_sequence 不列出"
    );
    let filtered = MetaArgs { filter: Some("EM".into()), ..args(s) };
    assert_eq!(
        meta::objects(MetaKey::Tables, &live.rows(MetaKey::Tables, &filtered).await).unwrap().len(),
        1
    );
    let aux = live.tables("aux", None).await;
    assert_eq!(aux[0].name, "other");
    assert!(aux[0].columns[0].auto_increment);

    let all = live.tables(s, None).await;
    let dept = table(&all, "dept");
    assert!(dept.columns[0].auto_increment, "{:?}", dept.columns[0]);
    assert_eq!(
        dept.primary_key.as_ref().map(|p| (p.name.clone(), p.columns.clone())),
        Some((None, vec!["id".into()]))
    );
    assert_eq!(dept.uniques[0].name.as_deref(), Some("uq_dept_name"));
    assert_eq!(dept.uniques[0].columns, ["name"]);
    assert_eq!(
        dept.checks.iter().map(|c| (c.name.clone(), c.expression.as_str())).collect::<Vec<_>>(),
        [(None, "budget >= 0")]
    );
    assert_eq!(logical(dept, "budget"), LogicalType::Decimal { precision: Some(12), scale: Some(2) });
    assert_eq!(dept.columns[2].default.as_deref(), Some("0"));

    let emp = table(&all, "emp");
    let col = |n: &str| emp.columns.iter().find(|c| c.name == n).unwrap_or_else(|| panic!("没有列 {n}"));
    assert!(!col("id").auto_increment, "复合主键里的 INTEGER 不是 rowid 别名");
    assert_eq!(emp.primary_key.as_ref().unwrap().name.as_deref(), Some("pk_emp"));
    assert_eq!(emp.primary_key.as_ref().unwrap().columns, ["id", "seq"]);
    assert_eq!(col("name").default.as_deref(), Some("'无名'"));
    assert!(!col("name").nullable && col("salary").nullable);
    assert_eq!(col("hired").default.as_deref(), Some("date('now')"));
    assert_eq!(logical(emp, "active"), LogicalType::Bool);
    assert_eq!(logical(emp, "hired"), LogicalType::Date);
    assert_eq!(logical(emp, "updated"), LogicalType::Timestamp { tz: false, precision: None });
    assert_eq!(logical(emp, "doc"), LogicalType::Json);
    assert_eq!(logical(emp, "photo"), LogicalType::Blob);
    assert_eq!(logical(emp, "salary"), LogicalType::Float { bits: 64 });
    assert_eq!(logical(emp, "untyped"), LogicalType::Other);
    assert_eq!(col("name_upper").generated.as_deref(), Some("upper(name)"));
    assert_eq!(col("double_salary").generated.as_deref(), Some("salary * 2"));
    assert_eq!(col("name").generated, None);
    let fk = &emp.foreign_keys[0];
    assert_eq!(
        (
            fk.name.as_deref(),
            fk.ref_table.as_str(),
            fk.ref_columns.clone(),
            fk.on_delete.as_deref(),
            fk.on_update.as_deref()
        ),
        (Some("fk_emp_dept"), "dept", vec!["id".to_string()], Some("CASCADE"), None)
    );
    assert_eq!(fk.ref_schema.as_deref(), Some(s));
    assert_eq!(
        (emp.uniques[0].name.clone(), emp.uniques[0].columns.clone()),
        (None, vec!["dept_id".into(), "name".into()])
    );
    assert_eq!(emp.checks.iter().map(|c| c.name.as_deref()).collect::<Vec<_>>(), [Some("ck_salary")]);
    let idx = &emp.indexes[0];
    assert_eq!(idx.name, "idx_emp_name");
    assert_eq!(
        idx.columns.iter().map(|c| (c.name.as_str(), c.descending)).collect::<Vec<_>>(),
        [("lower(name)", true), ("hired", false)]
    );
    assert!(!idx.unique);
    let trg = &emp.triggers[0];
    assert_eq!(
        (trg.name.as_str(), trg.timing.as_deref(), trg.events.clone()),
        ("trg_emp_au", Some("AFTER"), vec!["UPDATE".to_string()])
    );
    assert!(trg.definition.as_deref().unwrap().contains("CASE WHEN"));

    let kv = table(&all, "kv");
    assert!(!kv.columns[0].auto_increment && kv.primary_key.is_some());
    let one = live.tables(s, Some("emp")).await;
    assert_eq!(&one[0], emp);

    let d = live.d;
    let columns = live.rows(MetaKey::Columns, &args(s)).await;
    let views =
        meta::views(&d, MetaKey::Views, &live.rows(MetaKey::Views, &args(s)).await, &columns).unwrap();
    assert_eq!(views[0].columns.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["id", "name", "dept"]);
    let def = MetaArgs { name: Some("v_emp".into()), ..args(s) };
    let text = meta::source_text(MetaKey::ViewDefinition, &live.rows(MetaKey::ViewDefinition, &def).await)
        .unwrap()
        .unwrap();
    assert!(text.starts_with("CREATE VIEW v_emp"), "{text}");
    let q = d.ddl_query(dblens_dialect::DdlKind::Table, s, "emp").unwrap().unwrap();
    let ddl = dblens_dialect::ddl_text(&fetch(&mut live.conn, &q.sql, &q.params).await).unwrap();
    assert!(
        ddl.starts_with("CREATE TABLE \"emp\"")
            && ddl.contains("CREATE INDEX idx_emp_name")
            && ddl.contains("CREATE TRIGGER"),
        "{ddl}"
    );
    for key in [MetaKey::Sequences, MetaKey::Routines, MetaKey::Synonyms, MetaKey::MaterializedViews] {
        assert!(!d.has_meta(key), "{key:?}");
    }
    for q in d.ops_queries() {
        fetch(&mut live.conn, &q.sql, &[]).await;
    }
}
