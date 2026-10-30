//! 元数据读取：生成查询（SQL 和绑定值），把查询结果行组装成结构模型。不做 IO，查询由服务层执行。
//!
//! 每个家族的查询返回同一套规范化的列（见 `sql/*.sql` 文件头），差异全部写在 SQL 里，组装代码各家族共用。

mod assemble;
pub mod coltype;
mod rows;
pub(crate) mod template;

use serde::{Deserialize, Serialize};

use dblens_core::Value;

pub use assemble::{
    DependentView, Grant, ObjectInfo, ReferencingKey, SchemaInfo, TableRows, TableStorage, dependent_views,
    grants, objects, referencing_keys, reserved_words, routines, schemas, sequences, source_text, synonyms,
    table_storage, tables, trigger_ddl, views,
};
pub use rows::{MetaRow, MetaRows};

/// 元数据查询的种类。每个 key 对应模板文件里的一段 `-- [key]`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MetaKey {
    /// 列出数据库（只有 SQL Server：连接 → 数据库 → 架构）。参数：filter。
    /// 其余查询都在当前库中执行，服务层先切到所选的库。
    Catalogs,
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
    /// 由数据库给出的对象 DDL（不在模板文件中，由 `Dialect::ddl_query` 生成）。参数：schema、name、kind。
    Ddl,
    /// 以下供表设计器重建表用，参数：schema、table。其他表指向这张表的外键。
    ReferencingKeys,
    /// 依赖这张表的视图和物化视图（MySQL 系没有：视图按名字引用表）。
    DependentViews,
    /// 这张表上授予他人的权限（MySQL 系没有：授权按表名记录，重建不受影响）。
    Grants,
    /// 存储属性（引擎、字符集、表空间）和行数估计。
    TableStorage,
    /// 触发器的完整 DDL（只有 Oracle：`triggers` 的 definition 只有触发器体）。
    TriggerDdl,
    // 以下是退路（只有达梦）：旧版 DM8 缺少 ALL_MVIEWS、ALL_IND_EXPRESSIONS 时用，见 `fallback`。不在 `ALL` 中。
    TablesBasic,
    MaterializedViewsBasic,
    IndexesBasic,
    ViewDefinitionBasic,
}

impl MetaKey {
    pub const ALL: [MetaKey; 22] = [
        MetaKey::Catalogs,
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
        MetaKey::ReferencingKeys,
        MetaKey::DependentViews,
        MetaKey::Grants,
        MetaKey::TableStorage,
        MetaKey::TriggerDdl,
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

    /// 这个查询报系统视图不存在时改用的查询。
    pub fn fallback(self) -> Option<MetaKey> {
        match self {
            MetaKey::Tables => Some(MetaKey::TablesBasic),
            MetaKey::MaterializedViews => Some(MetaKey::MaterializedViewsBasic),
            MetaKey::Indexes => Some(MetaKey::IndexesBasic),
            MetaKey::ViewDefinition => Some(MetaKey::ViewDefinitionBasic),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            MetaKey::Catalogs => "catalogs",
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
            MetaKey::Ddl => "ddl",
            MetaKey::ReferencingKeys => "referencing_keys",
            MetaKey::DependentViews => "dependent_views",
            MetaKey::Grants => "grants",
            MetaKey::TableStorage => "table_storage",
            MetaKey::TriggerDdl => "trigger_ddl",
            MetaKey::TablesBasic => "tables_basic",
            MetaKey::MaterializedViewsBasic => "materialized_views_basic",
            MetaKey::IndexesBasic => "indexes_basic",
            MetaKey::ViewDefinitionBasic => "view_definition_basic",
        }
    }
}

/// 查询参数。`filter` 是用户在对象树过滤框里输入的原文（按名字或注释包含匹配，不区分大小写）。
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MetaArgs {
    /// 所在的数据库（SQL Server）。不进模板：服务层执行前先切到这个库。
    pub catalog: Option<String>,
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
