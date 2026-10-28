//! 文档导出（M4c 设计文档）：数据字典和设计文档，docx / html / markdown / xlsx。

mod content;
mod labels;
mod template;

pub use content::{
    BuildOptions, DocColumn, DocConstraint, DocContent, DocDiagram, DocIndex, DocInfo, DocKey, DocKind,
    DocReference, DocRelation, DocSchema, DocStats, DocTable, DocTrigger, DocUnreadable, DocView,
    SourceSchema, SourceTable, build, png_size,
};
pub use labels::label;
pub use template::{TextFormat, check, render};
