//! 对象操作（M1c2 设计文档第 3–5 节）：查看 DDL、生成 SQL、外键关系、数据标签的 SQL。
//! 取数都经元数据连接池和缓存；SQL 的拼装在 `dblens-dialect`。

use dblens_core::schema::Table;
use dblens_core::{Error, Result, msg};
use dblens_dialect::meta::{MetaArgs, MetaKey};
use dblens_dialect::{DdlKind, Family, Relation, ddl_text, normalize_ddl, relations};
use serde::{Deserialize, Serialize};

use crate::metadata::MetadataService;
use crate::profile::Profile;

/// 生成 SQL 的种类。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GenerateKind {
    Select,
    Insert,
    Update,
    Delete,
    Join,
}

/// 生成 SQL 的对象种类：视图和物化视图只能生成 SELECT。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObjectKind {
    #[default]
    Table,
    View,
    MaterializedView,
}

/// 数据标签的排序：一列，升序或降序。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DataOrder {
    pub column: String,
    #[serde(default)]
    pub desc: bool,
}

fn not_found(kind: &str, schema: &str, name: &str) -> Error {
    Error::not_found(msg!("noun.object", kind = kind, name = format!("{schema}.{name}")))
}

/// 程序源码查询（`MetaKey::RoutineSource`）用的种类名。
fn routine_kind(kind: DdlKind) -> Option<&'static str> {
    match kind {
        DdlKind::Function => Some("FUNCTION"),
        DdlKind::Procedure => Some("PROCEDURE"),
        DdlKind::Package => Some("PACKAGE"),
        DdlKind::PackageBody => Some("PACKAGE BODY"),
        _ => None,
    }
}

impl MetadataService {
    /// 对象的 DDL（见 `Dialect::ddl_query`）。
    pub async fn ddl(&self, profile: &Profile, schema: &str, kind: DdlKind, name: &str) -> Result<String> {
        let dialect = self.dialect(profile).await?;
        if let Some(q) = dialect.ddl_query(kind, schema, name)? {
            let args = MetaArgs {
                schema: Some(schema.into()),
                name: Some(name.into()),
                kind: Some(kind.as_str().into()),
                ..Default::default()
            };
            let rows = self.run(profile, MetaKey::Ddl, &args, |_| Ok(Some(q))).await?.unwrap_or_default();
            let mut text = ddl_text(&rows).ok_or_else(|| not_found(kind.as_str(), schema, name))?;
            // DBMS_METADATA 取表时不含索引和注释，用结构模型补上
            if kind == DdlKind::Table && matches!(dialect.family(), Family::Oracle | Family::Dameng) {
                let t = self.table(profile, schema, name).await?;
                let extras = dialect.table_extras(schema, &t, false)?;
                if !extras.is_empty() {
                    text = text.trim_end().to_string();
                    if !text.ends_with(';') {
                        text.push(';');
                    }
                    text += &extras;
                    text.push('\n');
                }
            }
            return Ok(text);
        }
        if let Some(rk) = routine_kind(kind) {
            let text = self.source(profile, schema, MetaKey::RoutineSource, name, Some(rk)).await?;
            return text.map(|t| normalize_ddl(&t)).ok_or_else(|| not_found(kind.as_str(), schema, name));
        }
        // PG 系：用结构模型拼装
        match kind {
            DdlKind::Table => {
                let t = self.table(profile, schema, name).await?;
                dialect.table_ddl(schema, &t)
            }
            DdlKind::View | DdlKind::MaterializedView => {
                let key = if kind == DdlKind::View { MetaKey::Views } else { MetaKey::MaterializedViews };
                let v = self
                    .views(profile, schema, key)
                    .await?
                    .into_iter()
                    .find(|v| v.name == name)
                    .ok_or_else(|| not_found(kind.as_str(), schema, name))?;
                let definition = match &v.definition {
                    Some(d) => d.clone(),
                    None => self
                        .source(profile, schema, MetaKey::ViewDefinition, name, None)
                        .await?
                        .unwrap_or_default(),
                };
                dialect.view_ddl(schema, &v, &definition, kind == DdlKind::MaterializedView)
            }
            DdlKind::Sequence => {
                let s = self
                    .sequences(profile, schema)
                    .await?
                    .into_iter()
                    .find(|s| s.name == name)
                    .ok_or_else(|| not_found(kind.as_str(), schema, name))?;
                Ok(dialect.sequence_ddl(schema, &s))
            }
            _ => Err(Error::invalid(msg!("ddl.unsupported", kind = kind.as_str()))),
        }
    }

    /// 一张表的明细；不存在时报 `not_found`。
    async fn table(&self, profile: &Profile, schema: &str, name: &str) -> Result<Table> {
        self.tables(profile, schema, Some(name))
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| not_found("table", schema, name))
    }

    /// 表、视图或物化视图（当作只有列的表）。
    async fn object(&self, profile: &Profile, schema: &str, object: &str, kind: ObjectKind) -> Result<Table> {
        let key = match kind {
            ObjectKind::Table => return self.table(profile, schema, object).await,
            ObjectKind::View => MetaKey::Views,
            ObjectKind::MaterializedView => MetaKey::MaterializedViews,
        };
        let v = self
            .views(profile, schema, key)
            .await?
            .into_iter()
            .find(|v| v.name == object)
            .ok_or_else(|| not_found("view", schema, object))?;
        Ok(Table { name: v.name, comment: v.comment, columns: v.columns, ..Default::default() })
    }

    /// 表 `table` 的外键关系：它引用的表，以及同一 schema 中引用它的表。
    pub async fn relations(&self, profile: &Profile, schema: &str, table: &str) -> Result<Vec<Relation>> {
        let t = self.table(profile, schema, table).await?;
        let all = self.tables(profile, schema, None).await?;
        Ok(relations(schema, &t, &all))
    }

    /// 生成 SQL。`reference` 只用于关联查询：给出时只连接与这张表相关的外键，否则连接本表引用的全部表。
    pub async fn generate(
        &self,
        profile: &Profile,
        schema: &str,
        object: &str,
        object_kind: ObjectKind,
        kind: GenerateKind,
        reference: Option<(&str, &str)>,
    ) -> Result<String> {
        if object_kind != ObjectKind::Table && kind != GenerateKind::Select {
            return Err(Error::invalid(msg!("generate.select_only")));
        }
        let dialect = self.dialect(profile).await?;
        let t = self.object(profile, schema, object, object_kind).await?;
        let needs_pk = || Error::invalid(msg!("generate.needs_primary_key", table = object));
        match kind {
            GenerateKind::Select => Ok(dialect.select_sql(Some(schema), &t)),
            GenerateKind::Insert => Ok(dialect.insert_sql(Some(schema), &t).sql),
            GenerateKind::Update => dialect.update_sql(Some(schema), &t).map(|d| d.sql).ok_or_else(needs_pk),
            GenerateKind::Delete => dialect.delete_sql(Some(schema), &t).map(|d| d.sql).ok_or_else(needs_pk),
            GenerateKind::Join => {
                let all = self.relations(profile, schema, object).await?;
                let picked: Vec<Relation> = match reference {
                    Some((rs, rt)) => all.into_iter().filter(|r| r.schema == rs && r.table == rt).collect(),
                    None => {
                        all.into_iter().filter(|r| r.direction == dblens_dialect::Direction::Out).collect()
                    }
                };
                if picked.is_empty() {
                    return Err(Error::invalid(msg!("generate.no_relations", table = object)));
                }
                Ok(dialect.join_sql(schema, object, &picked))
            }
        }
    }

    /// 数据标签的 SQL（见 `Dialect::data_sql`）。
    pub async fn data_sql(
        &self,
        profile: &Profile,
        schema: &str,
        object: &str,
        filter: Option<&str>,
        order: Option<&DataOrder>,
        count: bool,
    ) -> Result<String> {
        let dialect = self.dialect(profile).await?;
        Ok(dialect.data_sql(schema, object, filter, order.map(|o| (o.column.as_str(), o.desc)), count))
    }
}
