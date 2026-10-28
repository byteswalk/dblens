//! 模型（M4a）：模型目录的读写、打开的模型。

mod diagram;
mod file;
mod ops;
mod store;

pub use diagram::{
    DEFAULT_DIAGRAM, Diagram, DiagramDisplay, DiagramSummary, DiagramTable, DiagramTables, DiagramView,
    DiagramWarning, Position,
};
pub use file::{LogicalKey, ModelDatabase, ModelInfo, SchemaRenames};
#[cfg(test)]
pub(crate) use ops::tests;
pub use ops::{ModelDesignPreview, ModelDesignRequest, NewModel, RECENT_MODELS_SETTING, ReverseRequest};
pub use store::{FileError, ModelStore, ModelSummary, ModelTableView, ModelWrite, SchemaTables, TableEntry};
