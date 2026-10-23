//! 方言层：不做任何 IO，只负责各数据库的语法差异——标识符与字面量、语句拆分、类型映射、SQL 生成。

pub mod ident;
pub mod literal;
mod split;
mod variant;

pub use split::{SqlStatement, split_statements};
pub use variant::{Family, Variant};
