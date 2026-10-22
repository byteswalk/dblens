use async_trait::async_trait;
use dblens_core::{Result, Row};
use serde::{Deserialize, Serialize};

/// 结果集列的元信息。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ColumnMeta {
    pub name: String,
    /// 数据库原生类型名。
    pub type_name: String,
    pub nullable: Option<bool>,
    pub precision: Option<u32>,
    pub scale: Option<i32>,
    /// 来源表和列，能取到时填写，供数据编辑使用。
    pub source_table: Option<String>,
    pub source_column: Option<String>,
}

impl ColumnMeta {
    pub fn new(name: impl Into<String>, type_name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            type_name: type_name.into(),
            nullable: None,
            precision: None,
            scale: None,
            source_table: None,
            source_column: None,
        }
    }
}

/// 执行过程中依次产生的事件。
#[derive(Debug, Clone, PartialEq)]
pub enum ExecEvent {
    ResultStart { columns: Vec<ColumnMeta> },
    Rows(Vec<Row>),
    ResultEnd,
    Affected(u64),
    Message(String),
}

/// 拉取式的执行流：调用方调用 `next` 时才继续读取游标。
#[async_trait]
pub trait ExecStream: Send {
    /// 取下一个事件。`max_rows` 是单个 `Rows` 事件最多包含的行数。返回 `None` 表示执行全部结束。
    async fn next(&mut self, max_rows: usize) -> Result<Option<ExecEvent>>;
}

/// 读完整个执行流。只适合结果较小的场景（测试、元数据查询）。
pub async fn collect_all(stream: &mut (dyn ExecStream + '_), max_rows: usize) -> Result<Vec<ExecEvent>> {
    let mut events = Vec::new();
    while let Some(event) = stream.next(max_rows).await? {
        events.push(event);
    }
    Ok(events)
}
