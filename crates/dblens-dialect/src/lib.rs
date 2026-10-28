//! 方言层：不做任何 IO，只负责各数据库的语法差异——标识符与字面量、语句拆分、类型映射、SQL 生成。

mod browse;
mod changes;
pub mod compare;
mod ddl;
pub mod design;
mod dml;
pub mod ident;
mod import;
pub mod literal;
pub mod meta;
mod ops;
mod session;
mod split;
pub mod types;
mod variant;
mod version;

use dblens_core::{LogicalType, Result, Value};
use dblens_driver::{DriverId, ServerInfo};

pub use browse::{Direction, Relation, relations};
pub use changes::{ChangeStatement, RowChange, row_key};
pub use ddl::{DdlKind, ddl_text, normalize_ddl};
pub use dml::DmlTemplate;
pub use import::{ImportStatement, MAX_PARAMS_PER_STATEMENT, MAX_ROWS_PER_STATEMENT, WriteMode};
pub use ops::{OpsCategory, OpsQuery};
pub use session::may_change_schema;
use split::split_with;
pub use split::{SqlStatement, StreamSplitter, split_statements};
pub use variant::{Compat, Family, Variant};
pub use version::parse_version;

use meta::{MetaArgs, MetaKey, MetaQuery, MetaRows};

/// 某个具体数据库的方言：变体、服务器版本和兼容模式。可以随意复制；所有方法都是纯计算。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Dialect {
    variant: Variant,
    /// 编码后的服务器版本（见 `parse_version`），未知时为 None。人大金仓、openGauss 是所基于的 PG 版本。
    version: Option<u32>,
    compat: Compat,
}

impl Dialect {
    /// 版本未知的方言：元数据模板按最新版本渲染。
    pub fn new(variant: Variant) -> Self {
        Self { variant, version: None, compat: Compat::Native }
    }

    pub fn with_version(self, version: Option<u32>) -> Self {
        Self { version, ..self }
    }

    /// 指定兼容模式；只有人大金仓、openGauss 会是 `Oracle`，其他变体忽略。
    pub fn with_compat(self, compat: Compat) -> Self {
        match self.variant {
            Variant::Kingbase | Variant::OpenGauss => Self { compat, ..self },
            _ => self,
        }
    }

    /// 根据驱动和服务器版本识别兼容库和版本号。
    pub fn detect(driver: DriverId, info: &ServerInfo) -> Self {
        Self::new(Variant::detect(driver, info)).with_version(parse_version(&info.version))
    }

    /// 还没有服务器信息时使用该驱动的基础方言。
    pub fn for_driver(driver: DriverId) -> Self {
        Self::new(Variant::base(driver))
    }

    /// 用 `MetaKey::VariantProbe` 的查询结果细化方言：
    /// - MySQL：`SHOW VARIABLES LIKE 'polardb_version'` 有结果即为 PolarDB；
    /// - 人大金仓、openGauss：`mode` 列给出兼容模式，`version_num` 列（`server_version_num`）给出所基于的
    ///   PG 版本，替换由产品版本号解析出的版本（人大金仓的 `V009R002` 会被解析成 9.2）。
    pub fn refine(self, probe: &MetaRows) -> Self {
        match self.variant {
            Variant::MySql if !probe.is_empty() => Self { variant: Variant::PolarDb, ..self },
            Variant::Kingbase | Variant::OpenGauss => match probe.iter().next() {
                Some(row) => {
                    let compat = Compat::from_mode(&row.str("mode").unwrap_or_default());
                    let version = row.int("version_num").and_then(pg_version_num).or(self.version);
                    Self { version, compat, ..self }
                }
                None => self,
            },
            _ => self,
        }
    }

    pub fn compat(&self) -> Compat {
        self.compat
    }

    /// 人大金仓、openGauss 的 Oracle 兼容模式。
    pub fn oracle_mode(&self) -> bool {
        self.compat == Compat::Oracle
    }

    pub fn version(&self) -> Option<u32> {
        self.version
    }

    pub fn variant(&self) -> Variant {
        self.variant
    }

    pub fn family(&self) -> Family {
        self.variant.family()
    }

    pub fn quote_ident(&self, name: &str) -> String {
        ident::quote_ident(self.family(), name)
    }

    pub fn ident(&self, name: &str) -> String {
        ident::ident(self.family(), name)
    }

    /// `schema.name`，两部分各自按需加引号；schema 为空串时与 `None` 相同。
    pub fn qualified(&self, schema: Option<&str>, name: &str) -> String {
        match schema {
            Some(s) if !s.is_empty() => format!("{}.{}", self.ident(s), self.ident(name)),
            _ => self.ident(name),
        }
    }

    pub fn placeholder(&self, index: usize) -> String {
        ident::placeholder(self.family(), index)
    }

    pub fn literal(&self, value: &Value) -> Result<String> {
        literal::literal(self.family(), value)
    }

    pub fn paginate(&self, sql: &str, offset: u64, limit: u64) -> String {
        literal::paginate(self.family(), sql, offset, limit)
    }

    pub fn split(&self, sql: &str) -> Vec<SqlStatement> {
        split_with(self.splitter(), sql)
    }

    /// 分块拆分用的拆分器：人大金仓、openGauss 的 Oracle 兼容模式另外识别 PL/SQL 块。
    pub fn splitter(&self) -> StreamSplitter {
        let s = StreamSplitter::new(self.family());
        if self.oracle_mode() { s.with_oracle_blocks() } else { s }
    }

    /// 生成执行计划要依次执行的语句，最后一条的结果集是计划（见 `session::explain`）。
    pub fn explain(&self, sql: &str) -> Vec<String> {
        session::explain(self.family(), sql)
    }

    /// 查询当前 schema 的语句，结果是一行一列。
    pub fn current_schema_sql(&self) -> &'static str {
        session::current_schema(self.family())
    }

    /// 切换当前 schema 的语句。
    pub fn set_schema_sql(&self, schema: &str) -> String {
        session::set_schema(self.family(), schema)
    }

    pub fn logical_type(&self, native: &str) -> LogicalType {
        types::logical_type(self.family(), native)
    }

    pub fn native_type(&self, logical: &LogicalType) -> String {
        types::native_type(self.family(), logical)
    }

    /// 该方言是否有这种元数据查询（如 MySQL 没有物化视图和同义词）。
    pub fn has_meta(&self, key: MetaKey) -> bool {
        meta::sections(self.variant).contains_key(key.as_str())
    }

    /// 生成元数据查询；该方言没有这种查询时返回 None。
    pub fn meta_query(&self, key: MetaKey, args: &MetaArgs) -> Result<Option<MetaQuery>> {
        match meta::sections(self.variant).get(key.as_str()) {
            None => Ok(None),
            Some(t) => {
                let version = self.version.unwrap_or(u32::MAX);
                meta::render(key, t, self.family(), version, self.oracle_mode(), args).map(Some)
            }
        }
    }
}

/// 把 PG 的 `server_version_num`（10 起为 `major*10000 + minor`，之前为 `major*10000 + minor*100 + patch`）
/// 换成 `parse_version` 的编码。
fn pg_version_num(n: i64) -> Option<u32> {
    let n = u32::try_from(n).ok()?;
    let major = n / 10000;
    Some(if major >= 10 { major * 10000 + (n % 10000).min(99) * 100 } else { n })
}

/// 跨模块回归：各模块生成的 SQL 经 `Dialect::split` 必须仍是原样的一条语句。
#[cfg(test)]
mod tests {
    use dblens_core::schema::{Column, PrimaryKey, Table};
    use dblens_core::{ColumnType, LogicalType, Value};

    use super::*;

    const VARIANTS: [Variant; 4] = [Variant::MySql, Variant::Postgres, Variant::Oracle, Variant::Dameng];

    fn col(name: &str, comment: &str, auto: bool) -> Column {
        Column {
            name: name.into(),
            ordinal: 0,
            data_type: ColumnType {
                native: "VARCHAR(10)".into(),
                logical: LogicalType::VarChar { len: Some(10) },
            },
            nullable: true,
            default: None,
            comment: Some(comment.into()),
            auto_increment: auto,
            generated: None,
        }
    }

    /// 名字里带分号、注释符、`$$`、`q'[`、独占一行的 `/` 和中文的表。
    fn nasty_table() -> Table {
        let names = ["a;b", "c--d", "e$$f", "g q'[h", "i\n/\nj", "数洞;--列", "k`l\"m'n"];
        let mut columns: Vec<Column> =
            names.iter().map(|n| col(n, "注释;--/*$$ q'[x]' \n/\n DELIMITER //", false)).collect();
        columns.push(col("auto;id", "", true));
        Table {
            name: "t;--$$q'[\n/\n表".into(),
            columns,
            primary_key: Some(PrimaryKey { name: None, columns: vec!["a;b".into(), "i\n/\nj".into()] }),
            ..Default::default()
        }
    }

    fn assert_single(d: &Dialect, sql: &str, what: &str) {
        let parts = d.split(sql);
        assert_eq!(parts.len(), 1, "{:?} {what} 被拆成 {} 条：{sql}", d.variant(), parts.len());
        assert_eq!(parts[0].text, sql, "{:?} {what}", d.variant());
        assert_eq!(&sql[parts[0].start..parts[0].end], sql, "{:?} {what} 范围", d.variant());
    }

    #[test]
    fn generated_dml_splits_into_one_statement() {
        let t = nasty_table();
        for v in VARIANTS {
            let d = Dialect::new(v);
            for schema in [None, Some("s;--"), Some("hr\n/\n")] {
                let select = d.select_sql(schema, &t);
                assert_single(&d, &select, "select");
                assert_single(&d, &d.paginate(&select, 20, 10), "select 分页");
                assert_single(&d, &d.insert_sql(schema, &t).sql, "insert");
                assert_single(&d, &d.update_sql(schema, &t).unwrap().sql, "update");
                assert_single(&d, &d.delete_sql(schema, &t).unwrap().sql, "delete");
            }
        }
    }

    #[test]
    fn literals_embedded_in_select_split_into_one_statement() {
        let texts = ["\n/\n", "DELIMITER //", "$$;$$", "\\", "q'[;]'", "数洞;--中文", "'; DROP TABLE t; --"];
        for v in VARIANTS {
            let d = Dialect::new(v);
            let mut all = Vec::new();
            for s in texts {
                let lit = d.literal(&Value::String(s.into())).unwrap();
                assert_single(&d, &format!("SELECT {lit}"), s);
                all.push(lit);
            }
            assert_single(&d, &format!("SELECT {}", all.join(", ")), "全部");
        }
    }

    #[test]
    fn detect_parses_server_version() {
        use dblens_driver::{DriverId, ServerInfo};
        let info = ServerInfo { product: "MySQL".into(), version: "8.0.16".into() };
        let d = Dialect::detect(DriverId::Mysql, &info);
        assert_eq!((d.variant(), d.version()), (Variant::MySql, Some(80016)));
        assert_eq!(Dialect::new(Variant::Oracle).version(), None);
        assert_eq!(Dialect::new(Variant::Oracle).with_version(Some(110200)).version(), Some(110200));
        assert_eq!(Variant::PolarDb.family(), Family::MySql);
    }
    #[test]
    fn refine_finds_polardb() {
        let d = Dialect::new(Variant::MySql).with_version(Some(80016));
        let probe = MetaRows::new(
            vec!["Variable_name".into(), "Value".into()],
            vec![vec![Value::String("polardb_version".into()), Value::String("8.0.2.2.20".into())]],
        );
        assert_eq!(d.refine(&probe).variant(), Variant::PolarDb);
        assert_eq!(d.refine(&probe).version(), Some(80016));
        assert_eq!(d.refine(&MetaRows::default()).variant(), Variant::MySql);
        assert_eq!(Dialect::new(Variant::MariaDb).refine(&probe).variant(), Variant::MariaDb);
    }

    fn probe(mode: &str, version_num: &str) -> MetaRows {
        MetaRows::new(
            vec!["mode".into(), "version_num".into()],
            vec![vec![Value::String(mode.into()), Value::String(version_num.into())]],
        )
    }

    #[test]
    fn refine_reads_compat_mode_and_pg_version() {
        let kb = Dialect::new(Variant::Kingbase).with_version(Some(90200));
        let d = kb.refine(&probe("oracle", "120001"));
        assert_eq!((d.compat(), d.version()), (Compat::Oracle, Some(120100)));
        assert!(d.oracle_mode());
        assert_eq!(kb.refine(&probe("pg", "120001")).compat(), Compat::Native);
        let og = Dialect::new(Variant::OpenGauss).with_version(Some(70000));
        let d = og.refine(&probe("A", "90204"));
        assert_eq!((d.compat(), d.version()), (Compat::Oracle, Some(90204)));
        assert_eq!(og.refine(&probe("PG", "90204")).compat(), Compat::Native);
        // 未适配的模式按原生处理；没有探测结果时保持原样
        assert_eq!(og.refine(&probe("B", "90204")).compat(), Compat::Native);
        assert_eq!(og.refine(&MetaRows::default()), og);
        // 其他变体不受探测结果影响
        let pg = Dialect::new(Variant::Postgres).with_version(Some(180000));
        assert_eq!(pg.refine(&probe("oracle", "120001")), pg);
        assert_eq!(pg.with_compat(Compat::Oracle).compat(), Compat::Native);
    }

    #[test]
    fn split_follows_compat_mode() {
        let sql = "CREATE PROCEDURE p AS BEGIN NULL; END;\n/\nSELECT 1";
        let kb = Dialect::new(Variant::Kingbase);
        assert_eq!(kb.with_compat(Compat::Oracle).split(sql).len(), 2);
        assert_eq!(kb.split(sql).len(), 3);
    }

    #[test]
    fn compat_modes() {
        assert!(
            Compat::is_adapted_mode("A")
                && Compat::is_adapted_mode("pg")
                && Compat::is_adapted_mode("ORACLE")
        );
        assert!(!Compat::is_adapted_mode("B") && !Compat::is_adapted_mode("mysql"));
        assert_eq!(pg_version_num(120001), Some(120100));
        assert_eq!(pg_version_num(180006), Some(180600));
        assert_eq!(pg_version_num(90204), Some(90204));
        assert!(Dialect::new(Variant::Kingbase).has_meta(MetaKey::VariantProbe));
        assert!(Dialect::new(Variant::OpenGauss).has_meta(MetaKey::VariantProbe));
    }

    #[test]
    fn meta_query_is_none_when_the_dialect_lacks_the_key() {
        let probe =
            Dialect::new(Variant::MySql).meta_query(MetaKey::VariantProbe, &MetaArgs::default()).unwrap();
        assert!(probe.unwrap().sql.contains("polardb_version"));
        assert!(Dialect::new(Variant::PolarDb).has_meta(MetaKey::VariantProbe));
        assert!(!Dialect::new(Variant::Postgres).has_meta(MetaKey::VariantProbe));
        let none =
            Dialect::new(Variant::Postgres).meta_query(MetaKey::VariantProbe, &MetaArgs::default()).unwrap();
        assert!(none.is_none());
    }
}
