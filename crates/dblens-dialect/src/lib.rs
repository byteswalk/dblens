//! 方言层：不做任何 IO，只负责各数据库的语法差异——标识符与字面量、语句拆分、类型映射、SQL 生成。

mod browse;
mod changes;
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
pub use split::{SqlStatement, StreamSplitter, split_statements};
pub use variant::{Family, Variant};
pub use version::parse_version;

use meta::{MetaArgs, MetaKey, MetaQuery, MetaRows};

/// 某个具体数据库的方言：变体和服务器版本。可以随意复制；所有方法都是纯计算。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Dialect {
    variant: Variant,
    /// 编码后的服务器版本（见 `parse_version`），未知时为 None。
    version: Option<u32>,
}

impl Dialect {
    /// 版本未知的方言：元数据模板按最新版本渲染。
    pub fn new(variant: Variant) -> Self {
        Self { variant, version: None }
    }

    pub fn with_version(self, version: Option<u32>) -> Self {
        Self { version, ..self }
    }

    /// 根据驱动和服务器版本识别兼容库和版本号。
    pub fn detect(driver: DriverId, info: &ServerInfo) -> Self {
        Self::new(Variant::detect(driver, info)).with_version(parse_version(&info.version))
    }

    /// 还没有服务器信息时使用该驱动的基础方言。
    pub fn for_driver(driver: DriverId) -> Self {
        Self::new(Variant::base(driver))
    }

    /// 用 `MetaKey::VariantProbe` 的查询结果细化变体：目前只用于从 MySQL 中识别出 PolarDB
    /// （`SHOW VARIABLES LIKE 'polardb_version'` 有结果）。
    pub fn refine(self, probe: &MetaRows) -> Self {
        match self.variant {
            Variant::MySql if !probe.is_empty() => Self { variant: Variant::PolarDb, ..self },
            _ => self,
        }
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
        split_statements(sql, self.family())
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
            Some(t) => meta::render(key, t, self.family(), self.version.unwrap_or(u32::MAX), args).map(Some),
        }
    }
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
