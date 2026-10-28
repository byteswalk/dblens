//! 文档导出（M4c 设计文档）：数据字典和设计文档，docx / html / markdown / xlsx。

mod content;

pub use content::{
    BuildOptions, DocColumn, DocConstraint, DocContent, DocDiagram, DocIndex, DocInfo, DocKey, DocKind,
    DocReference, DocRelation, DocSchema, DocStats, DocTable, DocTrigger, DocUnreadable, DocView,
    SourceSchema, SourceTable, build, png_size,
};
