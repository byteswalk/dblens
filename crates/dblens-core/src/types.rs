use serde::{Deserialize, Serialize};

/// 与具体数据库无关的逻辑类型，用于跨库类型映射和设计器。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LogicalType {
    Bool,
    Int { bits: u8 },
    UInt { bits: u8 },
    Float { bits: u8 },
    Decimal { precision: Option<u32>, scale: Option<i32> },
    Char { len: Option<u32> },
    VarChar { len: Option<u32> },
    Text,
    Binary { len: Option<u32> },
    VarBinary { len: Option<u32> },
    Blob,
    Date,
    Time { precision: Option<u8> },
    Timestamp { tz: bool, precision: Option<u8> },
    Interval,
    Json,
    Uuid,
    Other,
}

/// 列类型：同时保存原生类型文本（展示、同库 DDL 用）和逻辑类型（跨库映射用）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ColumnType {
    pub native: String,
    pub logical: LogicalType,
}
