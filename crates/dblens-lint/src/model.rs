//! 规范和检查结果的数据结构。

use std::collections::HashSet;

use dblens_core::{Error, Result, msg};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::rules;

/// 一份规范：一组条款。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Ruleset {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub builtin: bool,
    pub clauses: Vec<Clause>,
}

/// 条款是给人看的要求，决定结论和级别；规则是挂在条款下的检查。
/// 没有挂规则的是人工条款。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Clause {
    pub id: String,
    pub section: Section,
    pub text: String,
    pub enabled: bool,
    pub severity: Severity,
    /// 自动查不全时，写明还要人工确认什么。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manual_note: Option<String>,
    #[serde(default)]
    pub rules: Vec<RuleRef>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuleRef {
    pub id: String,
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub params: Map<String, Value>,
    /// 适用的数据库家族；空表示全部。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub databases: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Section {
    Naming,
    Table,
    PrimaryKey,
    Column,
    Index,
    Constraint,
    Consistency,
    Distribution,
    Design,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Error,
    Warning,
    Info,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    Pass,
    Fail,
    Unknown,
    Manual,
    NotApplicable,
}

/// 规则发现的一个问题。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Issue {
    /// 对象的显示名，如 `ORDERS.AMOUNT`。
    pub object: String,
    /// 所在的表，用于定位到表设计器。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub table: Option<String>,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suggestion: Option<String>,
}

/// 人工结论。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Review {
    pub verdict: Verdict,
    #[serde(default)]
    pub note: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ClauseResult {
    pub clause: String,
    /// 自动结论。
    pub verdict: Verdict,
    /// 无法判断或要人工确认的原因。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default)]
    pub issues: Vec<Issue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review: Option<Review>,
}

impl ClauseResult {
    /// 最终结论：人工填过的以人工为准。
    pub fn effective(&self) -> Verdict {
        self.review.as_ref().map_or(self.verdict, |r| r.verdict)
    }

    /// 只有自动查不出结论的条款可以填人工结论。
    pub fn reviewable(&self) -> bool {
        matches!(self.verdict, Verdict::Unknown | Verdict::Manual)
    }
}

/// 结果统计。通过率只算通过和未通过，其余单独计数。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Summary {
    pub pass: u32,
    pub fail: u32,
    pub unknown: u32,
    pub manual: u32,
    pub not_applicable: u32,
    pub issues: u32,
    /// 百分比，保留一位小数；没有可计算的条款时为 None。
    pub pass_rate: Option<f64>,
}

pub fn summarize(results: &[ClauseResult]) -> Summary {
    let mut s = Summary::default();
    for r in results {
        match r.effective() {
            Verdict::Pass => s.pass += 1,
            Verdict::Fail => s.fail += 1,
            Verdict::Unknown => s.unknown += 1,
            Verdict::Manual => s.manual += 1,
            Verdict::NotApplicable => s.not_applicable += 1,
        }
        s.issues += r.issues.len() as u32;
    }
    let total = s.pass + s.fail;
    s.pass_rate = (total > 0).then(|| (f64::from(s.pass) * 1000.0 / f64::from(total)).round() / 10.0);
    s
}

/// 保存或导入规范前的校验。
pub fn validate(ruleset: &Ruleset) -> Result<()> {
    if ruleset.name.trim().is_empty() {
        return Err(Error::invalid(msg!("lint.ruleset_name_empty")));
    }
    let mut ids = HashSet::new();
    for clause in &ruleset.clauses {
        if clause.id.trim().is_empty() || clause.text.trim().is_empty() {
            return Err(Error::invalid(msg!("lint.clause_empty")));
        }
        if !ids.insert(clause.id.as_str()) {
            return Err(Error::invalid(msg!("lint.clause_duplicate", id = clause.id)));
        }
        for rule in &clause.rules {
            if !rules::exists(&rule.id) {
                return Err(Error::invalid(msg!("lint.rule_unknown", id = rule.id)));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn result(verdict: Verdict) -> ClauseResult {
        ClauseResult { clause: "c".into(), verdict, reason: None, issues: vec![], review: None }
    }

    #[test]
    fn pass_rate_counts_pass_and_fail_only() {
        let s = summarize(&[
            result(Verdict::Pass),
            result(Verdict::Pass),
            result(Verdict::Fail),
            result(Verdict::Unknown),
            result(Verdict::Manual),
            result(Verdict::NotApplicable),
        ]);
        assert_eq!((s.pass, s.fail, s.unknown, s.manual, s.not_applicable), (2, 1, 1, 1, 1));
        assert_eq!(s.pass_rate, Some(66.7));
    }

    #[test]
    fn pass_rate_is_none_without_decided_clauses() {
        assert_eq!(summarize(&[result(Verdict::Manual)]).pass_rate, None);
    }

    #[test]
    fn review_overrides_the_automatic_verdict() {
        let mut r = result(Verdict::Manual);
        r.review = Some(Review { verdict: Verdict::Pass, note: "ok".into() });
        let s = summarize(&[r]);
        assert_eq!((s.pass, s.manual), (1, 0));
    }

    #[test]
    fn validate_rejects_unknown_rules_and_duplicates() {
        let clause = |id: &str, rule: &str| Clause {
            id: id.into(),
            section: Section::Table,
            text: "t".into(),
            enabled: true,
            severity: Severity::Error,
            manual_note: None,
            rules: vec![RuleRef { id: rule.into(), params: Map::new(), databases: vec![] }],
        };
        let set = |clauses| Ruleset { id: "x".into(), name: "n".into(), builtin: false, clauses };
        assert!(validate(&set(vec![clause("a", "table.comment")])).is_ok());
        assert!(validate(&set(vec![clause("a", "no.such")])).is_err());
        assert!(validate(&set(vec![clause("a", "table.comment"), clause("a", "table.comment")])).is_err());
    }
}
