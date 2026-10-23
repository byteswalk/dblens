//! 方言层：不做任何 IO，只负责各数据库的语法差异——标识符与字面量、语句拆分、类型映射、SQL 生成。

mod dml;
pub mod ident;
pub mod literal;
mod split;
pub mod types;
mod variant;

use dblens_core::{LogicalType, Result, Value};
use dblens_driver::{DriverId, ServerInfo};

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

    /// `schema.name`，两部分各自按需加引号。
    pub fn qualified(&self, schema: Option<&str>, name: &str) -> String {
        match schema {
            Some(s) => format!("{}.{}", self.ident(s), self.ident(name)),
            None => self.ident(name),
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
