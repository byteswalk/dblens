//! 表设计器（M3 设计文档第 3–5 节）：前端编辑一份草稿表，这里校验草稿、与原表比对出变更列表，
//! 再按方言生成 DDL；库不支持的改动改用重建表的脚本。不做任何 IO。

mod catalog;
pub(crate) mod diff;
mod mysql;
mod oracle;
mod postgres;
mod rebuild;
mod sql;

use std::collections::BTreeMap;

use dblens_core::schema::{CheckConstraint, Column, ForeignKey, Index, PrimaryKey, Table, UniqueConstraint};
use dblens_core::{Msg, msg};
use serde::{Deserialize, Serialize};

use crate::Dialect;

pub use catalog::{DesignCaps, TypeEntry, TypeParams};
pub use diff::{diff_table, validate};
pub use rebuild::{Rebuild, RebuildContext, RebuildOutcome, RebuildStep};
pub use sql::DesignStatement;
#[cfg(test)]
pub(crate) use sql::fixtures;

/// 草稿中改了名的项：新名 → 原名。前端用每一项的 `origin` 跟踪原来的名字，发送时整理成这个映射。
/// 约束改名一律先删后建，不需要映射。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Renames {
    #[serde(default)]
    pub columns: BTreeMap<String, String>,
    #[serde(default)]
    pub indexes: BTreeMap<String, String>,
}

/// 一项结构变更。列的修改发生在改名之后：`AlterColumn.from` 的名字已换成新名。
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TableChange {
    /// 新建表：整张表（含约束、索引、注释）一次生成。
    CreateTable {
        table: Table,
    },
    SetTableComment {
        comment: Option<String>,
    },
    /// `after` 为草稿中前一列的名字，None 表示放在最前。
    AddColumn {
        column: Column,
        after: Option<String>,
    },
    DropColumn {
        name: String,
    },
    /// `column` 是草稿中的完整定义（旧版 MySQL 只能用 `CHANGE` 带上定义改名）。
    RenameColumn {
        from: String,
        to: String,
        column: Column,
    },
    AlterColumn {
        from: Column,
        to: Column,
    },
    /// 调整已有列的位置：`column` 是草稿中的完整定义。
    MoveColumn {
        column: Column,
        after: Option<String>,
    },
    DropPrimaryKey {
        key: PrimaryKey,
    },
    AddPrimaryKey {
        key: PrimaryKey,
    },
    DropUnique {
        unique: UniqueConstraint,
    },
    AddUnique {
        unique: UniqueConstraint,
    },
    DropCheck {
        check: CheckConstraint,
    },
    AddCheck {
        check: CheckConstraint,
    },
    DropForeignKey {
        key: ForeignKey,
    },
    AddForeignKey {
        key: ForeignKey,
    },
    DropIndex {
        index: Index,
    },
    AddIndex {
        index: Index,
    },
    /// `index` 是草稿中的索引（不支持改名的旧版本先删后建）。
    RenameIndex {
        from: String,
        index: Index,
    },
}

/// 比对结果。`appended`：新列都在最后、保留的列相对顺序不变——不能指定列位置的库据此判断要不要重建。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Diff {
    pub changes: Vec<TableChange>,
    pub appended: bool,
}

/// 校验错误出现在设计器的哪个部分。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum IssueSection {
    Table,
    Columns,
    PrimaryKey,
    Indexes,
    ForeignKeys,
    Uniques,
    Checks,
}

/// 一处校验错误：`row` 是该部分中的行号（从 0 开始），`field` 是字段名（与前端的列名一致）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DesignIssue {
    pub section: IssueSection,
    pub row: Option<usize>,
    pub field: Option<&'static str>,
    pub message: Msg,
}

impl DesignIssue {
    pub(crate) fn new(
        section: IssueSection,
        row: Option<usize>,
        field: Option<&'static str>,
        message: Msg,
    ) -> Self {
        Self { section, row, field, message }
    }
}

impl Dialect {
    /// 整理前端发来的草稿：按原生类型文本重新解析逻辑类型；主键列一律 NOT NULL。
    /// 返回提示（如"主键列 id 将设为 NOT NULL"）。
    pub fn prepare_draft(&self, draft: &mut Table) -> Vec<Msg> {
        let mut notes = Vec::new();
        let pk: Vec<String> = draft.primary_key.as_ref().map(|k| k.columns.clone()).unwrap_or_default();
        for c in &mut draft.columns {
            c.data_type.native = c.data_type.native.trim().to_string();
            c.data_type.logical = self.logical_type(&c.data_type.native);
            if pk.contains(&c.name) && c.nullable {
                c.nullable = false;
                notes.push(msg!("design.pk_not_null", column = c.name));
            }
        }
        for (i, c) in draft.columns.iter_mut().enumerate() {
            c.ordinal = i as u32 + 1;
        }
        notes
    }
}

#[cfg(test)]
mod tests {
    use dblens_core::schema::PrimaryKey;
    use dblens_core::{ColumnType, LogicalType};

    use super::*;
    use crate::Variant;

    #[test]
    fn prepare_draft_parses_types_and_forces_pk_not_null() {
        let col = |name: &str, native: &str| Column {
            name: name.into(),
            ordinal: 9,
            data_type: ColumnType { native: native.into(), logical: LogicalType::Other },
            nullable: true,
            default: None,
            comment: None,
            auto_increment: false,
            generated: None,
        };
        let mut t = Table {
            name: "t".into(),
            columns: vec![col("id", " bigint "), col("name", "varchar(20)")],
            primary_key: Some(PrimaryKey { name: None, columns: vec!["id".into()] }),
            ..Default::default()
        };
        let notes = Dialect::new(Variant::Postgres).prepare_draft(&mut t);
        assert_eq!(notes, vec![msg!("design.pk_not_null", column = "id")]);
        assert_eq!(t.columns[0].data_type.native, "bigint");
        assert_eq!(t.columns[0].data_type.logical, LogicalType::Int { bits: 64 });
        assert_eq!(t.columns[1].data_type.logical, LogicalType::VarChar { len: Some(20) });
        assert!(!t.columns[0].nullable && t.columns[1].nullable);
        assert_eq!((t.columns[0].ordinal, t.columns[1].ordinal), (1, 2));
    }
}
