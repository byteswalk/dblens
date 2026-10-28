//! 模型（M4a）：模型目录的读写、打开的模型。

mod file;
mod store;

pub use file::{ModelDatabase, ModelInfo, SchemaRenames};
pub use store::{FileError, ModelStore, ModelSummary, ModelTableView, ModelWrite, SchemaTables, TableEntry};
