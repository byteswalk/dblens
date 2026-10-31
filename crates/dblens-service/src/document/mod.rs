//! 文档导出（M4c 设计文档）：数据字典和设计文档，docx / html / markdown / xlsx。

mod content;
mod docx;
mod job;
mod labels;
mod template;
mod xlsx;

pub use content::{
    BuildOptions, DocColumn, DocConstraint, DocContent, DocDiagram, DocIndex, DocInfo, DocKey, DocKind,
    DocReference, DocRelation, DocSchema, DocStats, DocTable, DocTrigger, DocUnreadable, DocView,
    SourceSchema, SourceTable, build, png_size,
};
pub(crate) use docx::{Cell, assemble};
pub use docx::{builtin_reference, check_reference, write as write_docx};
pub use job::{DiagramImage, DocFormat, DocSource, DocumentProblem, DocumentRequest};
pub use labels::label;
pub use template::{TextFormat, check, render};
pub use xlsx::write as write_xlsx;
pub(crate) use xlsx::{Sheet, xlsx_error};
