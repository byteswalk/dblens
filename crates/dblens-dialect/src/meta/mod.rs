//! 元数据读取：生成查询（SQL 和绑定值），把查询结果行组装成结构模型。不做 IO，查询由服务层执行。
//!
//! 每个家族的查询返回同一套规范化的列（见 `sql/*.sql` 文件头），差异全部写在 SQL 里，组装代码各家族共用。

mod assemble;
mod rows;
mod template;

use serde::{Deserialize, Serialize};

use dblens_core::Value;

pub use assemble::{
    ObjectInfo, SchemaInfo, TableRows, objects, reserved_words, routines, schemas, sequences, source_text,
    synonyms, tables, views,
};
pub use rows::{MetaRow, MetaRows};

/// 元数据查询的种类。每个 key 对应模板文件里的一段 `-- [key]`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MetaKey {
    /// 列出 schema（MySQL 为 database）。参数：filter。
    Schemas,
    /// 列出表（含分区父表）。参数：schema、filter、table（只取这一张）。
    Tables,
    Views,
    MaterializedViews,
    Sequences,
    Routines,
    Synonyms,
    /// 以下是明细，参数：schema、table（不给则取整个 schema，一类对象一条 SQL）。
    Columns,
    Keys,
    Checks,
    Indexes,
    Triggers,
    /// 单个对象的源码。参数：schema、name（routine_source 另有 kind）。
    ViewDefinition,
    RoutineSource,
    /// 服务器的保留字，用于校验标识符引用规则。无参数。
    ReservedWords,
    /// 识别兼容库用的附加查询（如 PolarDB），见 `Dialect::refine`。无参数。
    VariantProbe,
}

impl MetaKey {
    pub const ALL: [MetaKey; 16] = [
        MetaKey::Schemas,
        MetaKey::Tables,
        MetaKey::Views,
        MetaKey::MaterializedViews,
        MetaKey::Sequences,
        MetaKey::Routines,
        MetaKey::Synonyms,
        MetaKey::Columns,
        MetaKey::Keys,
        MetaKey::Checks,
        MetaKey::Indexes,
        MetaKey::Triggers,
        MetaKey::ViewDefinition,
        MetaKey::RoutineSource,
        MetaKey::ReservedWords,
        MetaKey::VariantProbe,
    ];

    /// 取一张表或整个 schema 明细时要执行的查询，结果按同样的顺序填进 `TableRows`。
    pub const TABLE_DETAIL: [MetaKey; 6] = [
        MetaKey::Tables,
        MetaKey::Columns,
        MetaKey::Keys,
        MetaKey::Checks,
        MetaKey::Indexes,
        MetaKey::Triggers,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            MetaKey::Schemas => "schemas",
            MetaKey::Tables => "tables",
            MetaKey::Views => "views",
            MetaKey::MaterializedViews => "materialized_views",
            MetaKey::Sequences => "sequences",
            MetaKey::Routines => "routines",
            MetaKey::Synonyms => "synonyms",
            MetaKey::Columns => "columns",
            MetaKey::Keys => "keys",
            MetaKey::Checks => "checks",
            MetaKey::Indexes => "indexes",
            MetaKey::Triggers => "triggers",
            MetaKey::ViewDefinition => "view_definition",
            MetaKey::RoutineSource => "routine_source",
            MetaKey::ReservedWords => "reserved_words",
            MetaKey::VariantProbe => "variant_probe",
        }
    }
}

/// 查询参数。`filter` 是用户在对象树过滤框里输入的原文（按名字或注释包含匹配，不区分大小写）。
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MetaArgs {
    pub schema: Option<String>,
    pub table: Option<String>,
    pub name: Option<String>,
    /// routine_source 用：PROCEDURE / FUNCTION / PACKAGE / PACKAGE BODY。
    pub kind: Option<String>,
    pub filter: Option<String>,
}

/// 渲染好的元数据查询。`params` 按占位符顺序排列。
#[derive(Debug, Clone, PartialEq)]
pub struct MetaQuery {
    pub key: MetaKey,
    pub sql: String,
    pub params: Vec<Value>,
}

pub(crate) use template::{render, sections};
