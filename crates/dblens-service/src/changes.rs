//! 表数据编辑（M2a 设计文档第 5 节）：由改动集生成语句，预览或在会话中执行。

use dblens_core::{Error, Result, msg};
use dblens_dialect::{ChangeStatement, RowChange};
use serde::Deserialize;

use crate::profile::Profile;
use crate::service::Service;
use crate::session::{ApplyOutcome, SessionId};

/// 前端提交的改动集。
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ChangeSet {
    pub schema: String,
    pub table: String,
    pub changes: Vec<RowChange>,
}

impl Service {
    async fn change_statements(&self, profile: &Profile, set: &ChangeSet) -> Result<Vec<ChangeStatement>> {
        let dialect = self.metadata.dialect(profile).await?;
        let table = self
            .metadata
            .tables(profile, &set.schema, Some(&set.table))
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| {
                Error::not_found(msg!(
                    "noun.object",
                    kind = "table",
                    name = format!("{}.{}", set.schema, set.table)
                ))
            })?;
        dialect.change_statements(&set.schema, &table, &set.changes)
    }

    /// 预览：生成的语句（`preview` 为值写成字面量的 SQL），不执行。
    pub async fn preview_changes(&self, profile_id: &str, set: &ChangeSet) -> Result<Vec<ChangeStatement>> {
        let profile = self.profile(profile_id)?;
        self.change_statements(&profile, set).await
    }

    /// 在会话中执行改动集（一个事务，失败整体回滚）。
    pub async fn apply_changes(&self, session: SessionId, set: &ChangeSet) -> Result<ApplyOutcome> {
        let info = self.sessions.info(session)?;
        let profile = self.profile(&info.profile_id)?;
        let statements = self.change_statements(&profile, set).await?;
        self.sessions.apply(session, statements).await
    }
}
