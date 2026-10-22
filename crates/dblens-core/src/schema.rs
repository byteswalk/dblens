//! 统一结构模型。层级：Catalog → Schema → 各类对象。
//! Oracle/达梦没有 Catalog 层；MySQL 的 database 同时充当 Schema。

use serde::{Deserialize, Serialize};

use crate::ColumnType;

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Catalog {
    pub name: String,
    #[serde(default)]
    pub schemas: Vec<Schema>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Schema {
    pub name: String,
    #[serde(default)]
    pub tables: Vec<Table>,
    #[serde(default)]
    pub views: Vec<View>,
    #[serde(default)]
    pub materialized_views: Vec<View>,
    #[serde(default)]
    pub sequences: Vec<Sequence>,
    #[serde(default)]
    pub routines: Vec<Routine>,
    #[serde(default)]
    pub synonyms: Vec<Synonym>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Table {
    pub name: String,
    #[serde(default)]
    pub comment: Option<String>,
    #[serde(default)]
    pub columns: Vec<Column>,
    #[serde(default)]
    pub primary_key: Option<PrimaryKey>,
    #[serde(default)]
    pub uniques: Vec<UniqueConstraint>,
    #[serde(default)]
    pub foreign_keys: Vec<ForeignKey>,
    #[serde(default)]
    pub checks: Vec<CheckConstraint>,
    #[serde(default)]
    pub indexes: Vec<Index>,
    #[serde(default)]
    pub triggers: Vec<Trigger>,
    /// 分区信息的文本描述，首期只读。
    #[serde(default)]
    pub partition: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Column {
    pub name: String,
    /// 列序号，从 1 开始。
    pub ordinal: u32,
    pub data_type: ColumnType,
    pub nullable: bool,
    /// 默认值表达式的原文。
    #[serde(default)]
    pub default: Option<String>,
    #[serde(default)]
    pub comment: Option<String>,
    #[serde(default)]
    pub auto_increment: bool,
    /// 生成列的表达式。
    #[serde(default)]
    pub generated: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PrimaryKey {
    pub name: Option<String>,
    pub columns: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UniqueConstraint {
    pub name: Option<String>,
    pub columns: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ForeignKey {
    pub name: Option<String>,
    pub columns: Vec<String>,
    pub ref_schema: Option<String>,
    pub ref_table: String,
    pub ref_columns: Vec<String>,
    pub on_delete: Option<String>,
    pub on_update: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CheckConstraint {
    pub name: Option<String>,
    pub expression: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Index {
    pub name: String,
    pub columns: Vec<IndexColumn>,
    pub unique: bool,
    /// 索引方法或类型，如 BTREE、HASH、NORMAL、BITMAP。
    pub method: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IndexColumn {
    /// 列名；函数索引时为表达式原文。
    pub name: String,
    pub descending: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Trigger {
    pub name: String,
    /// BEFORE / AFTER / INSTEAD OF
    pub timing: Option<String>,
    /// INSERT / UPDATE / DELETE 等
    pub events: Vec<String>,
    pub definition: Option<String>,
}

/// 视图和物化视图共用。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct View {
    pub name: String,
    pub comment: Option<String>,
    pub definition: Option<String>,
    #[serde(default)]
    pub columns: Vec<Column>,
}

/// 序列。数值用字符串保存，因为 Oracle 序列上限可达 28 位。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Sequence {
    pub name: String,
    pub start: Option<String>,
    pub increment: Option<String>,
    pub min_value: Option<String>,
    pub max_value: Option<String>,
    pub cycle: bool,
    pub cache: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RoutineKind {
    Procedure,
    Function,
    Package,
    PackageBody,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Routine {
    pub name: String,
    pub kind: RoutineKind,
    pub source: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Synonym {
    pub name: String,
    pub target_schema: Option<String>,
    pub target_name: String,
    pub db_link: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ColumnType, LogicalType};

    fn sample_schema() -> Schema {
        let id = Column {
            name: "id".into(),
            ordinal: 1,
            data_type: ColumnType { native: "NUMBER(10)".into(), logical: LogicalType::Decimal { precision: Some(10), scale: Some(0) } },
            nullable: false,
            default: None,
            comment: Some("主键".into()),
            auto_increment: false,
            generated: None,
        };
        let name = Column {
            name: "name".into(),
            ordinal: 2,
            data_type: ColumnType { native: "VARCHAR2(100 CHAR)".into(), logical: LogicalType::VarChar { len: Some(100) } },
            nullable: true,
            default: Some("'无名'".into()),
            comment: Some("姓名".into()),
            auto_increment: false,
            generated: None,
        };
        let table = Table {
            name: "person".into(),
            comment: Some("人员".into()),
            columns: vec![id, name],
            primary_key: Some(PrimaryKey { name: Some("pk_person".into()), columns: vec!["id".into()] }),
            uniques: vec![UniqueConstraint { name: Some("uk_person_name".into()), columns: vec!["name".into()] }],
            foreign_keys: vec![ForeignKey {
                name: Some("fk_person_dept".into()),
                columns: vec!["dept_id".into()],
                ref_schema: None,
                ref_table: "dept".into(),
                ref_columns: vec!["id".into()],
                on_delete: Some("CASCADE".into()),
                on_update: None,
            }],
            checks: vec![CheckConstraint { name: Some("ck_id".into()), expression: "id > 0".into() }],
            indexes: vec![Index {
                name: "idx_person_name".into(),
                columns: vec![IndexColumn { name: "name".into(), descending: true }],
                unique: false,
                method: Some("NORMAL".into()),
            }],
            triggers: vec![Trigger { name: "trg".into(), timing: Some("BEFORE".into()), events: vec!["INSERT".into()], definition: None }],
            partition: None,
        };
        Schema {
            name: "HR".into(),
            tables: vec![table],
            views: vec![View { name: "v_person".into(), comment: None, definition: Some("select * from person".into()), columns: vec![] }],
            materialized_views: vec![],
            sequences: vec![Sequence {
                name: "seq_person".into(),
                start: Some("1".into()),
                increment: Some("1".into()),
                min_value: Some("1".into()),
                max_value: Some("9999999999999999999999999999".into()),
                cycle: false,
                cache: Some(20),
            }],
            routines: vec![Routine { name: "pkg_hr".into(), kind: RoutineKind::Package, source: None }],
            synonyms: vec![Synonym { name: "emp".into(), target_schema: Some("SCOTT".into()), target_name: "EMP".into(), db_link: None }],
        }
    }

    #[test]
    fn schema_round_trips_through_json() {
        let schema = sample_schema();
        let json = serde_json::to_string(&schema).unwrap();
        let back: Schema = serde_json::from_str(&json).unwrap();
        assert_eq!(back, schema);
    }

    #[test]
    fn logical_type_uses_snake_case_kind() {
        let json = serde_json::to_value(LogicalType::VarChar { len: Some(100) }).unwrap();
        assert_eq!(json, serde_json::json!({"kind":"var_char","len":100}));
        assert_eq!(serde_json::to_value(LogicalType::Json).unwrap(), serde_json::json!({"kind":"json"}));
    }

    #[test]
    fn missing_collections_default_to_empty() {
        let t: Table = serde_json::from_str(r#"{"name":"t"}"#).unwrap();
        assert!(t.columns.is_empty() && t.indexes.is_empty() && t.primary_key.is_none());
        let s: Schema = serde_json::from_str(r#"{"name":"s"}"#).unwrap();
        assert!(s.tables.is_empty() && s.synonyms.is_empty());
    }
}
