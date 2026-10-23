//! 方言层：不做任何 IO，只负责各数据库的语法差异——标识符与字面量、语句拆分、类型映射、SQL 生成。

mod dml;
pub mod ident;
pub mod literal;
mod split;
pub mod types;
mod variant;

use dblens_core::{LogicalType, Result, Value};
use dblens_driver::{DriverId, ServerInfo};

pub use dml::DmlTemplate;
pub use split::{SqlStatement, split_statements};
pub use variant::{Family, Variant};

/// 某个具体数据库的方言。只包含变体信息，可以随意复制；所有方法都是纯计算。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Dialect {
    variant: Variant,
}

impl Dialect {
    pub fn new(variant: Variant) -> Self {
        Self { variant }
    }

    /// 根据驱动和服务器版本识别兼容库。
    pub fn detect(driver: DriverId, info: &ServerInfo) -> Self {
        Self::new(Variant::detect(driver, info))
    }

    /// 还没有服务器信息时使用该驱动的基础方言。
    pub fn for_driver(driver: DriverId) -> Self {
        Self::new(Variant::base(driver))
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

    pub fn logical_type(&self, native: &str) -> LogicalType {
        types::logical_type(self.family(), native)
    }

    pub fn native_type(&self, logical: &LogicalType) -> String {
        types::native_type(self.family(), logical)
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
}
