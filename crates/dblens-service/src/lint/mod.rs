//! 设计规范检查（M7a 设计文档）：规范库的增删改查、发起检查、保存结果、人工结论、导出报告。
//! 规则引擎在 `dblens-lint`，本模块负责取结构、起任务、存本地库。

mod job;
mod report;
mod store;

use dblens_lint::{ClauseResult, Section, Severity, Verdict};
use serde::{Deserialize, Serialize};

pub use job::ReportFormat;

/// 检查对象：模型的一个 schema，或连接的一个 schema。与比对、文档导出一致。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LintSource {
    Model {
        model: String,
        schema: String,
    },
    Database {
        profile: String,
        #[serde(default)]
        catalog: Option<String>,
        schema: String,
    },
}

/// 发起检查：选规范、选对象。
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct LintRequest {
    pub ruleset: String,
    pub source: LintSource,
}

/// 规范库列表里的一条。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RulesetSummary {
    pub id: String,
    pub name: String,
    pub builtin: bool,
    /// 是否有用户改动（内置规范）。
    pub modified: bool,
    pub clause_count: usize,
}

/// 人工结论。
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ReviewInput {
    pub verdict: Verdict,
    #[serde(default)]
    pub note: String,
}

/// 条款的说明信息，随检查结果一起存下来，报告和界面分组用。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ClauseMeta {
    pub id: String,
    pub section: Section,
    pub text: String,
    pub severity: Severity,
}

/// 一次检查的完整记录。存进 `lint_runs.body`，之后改规范不影响它。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LintRun {
    pub id: String,
    pub created_at: i64,
    /// 检查对象的显示名。
    pub target: String,
    pub ruleset_id: String,
    pub ruleset_name: String,
    /// 数据库家族；模型检查时取模型的家族。
    pub family: String,
    /// 条款说明，与 `results` 一一对应。
    pub clauses: Vec<ClauseMeta>,
    pub results: Vec<ClauseResult>,
}

impl LintRun {
    pub fn summary(&self) -> dblens_lint::Summary {
        dblens_lint::summarize(&self.results)
    }
}

/// 检查记录列表里的一条。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LintRunSummary {
    pub id: String,
    pub created_at: i64,
    pub target: String,
    pub ruleset_name: String,
}

/// 条款结论属于"待人工填写"的两种。
pub(crate) fn is_reviewable(verdict: Verdict) -> bool {
    matches!(verdict, Verdict::Unknown | Verdict::Manual)
}
