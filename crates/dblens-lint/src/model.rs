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
    /// 例外：某个对象不适用某条条款。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exceptions: Vec<Exception>,
}

/// 例外。按条款编号和对象名精确匹配，对象在该条款下的问题一起豁免。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Exception {
    pub clause: String,
    pub object: String,
    pub reason: String,
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

/// 被例外豁免的问题。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WaivedIssue {
    #[serde(flatten)]
    pub issue: Issue,
    pub reason: String,
}

/// 条款没有问题时的结论，豁免掉全部问题后回到它。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CleanVerdict {
    pub verdict: Verdict,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub waived: Vec<WaivedIssue>,
    /// 没有问题时的结论。M7a 的旧记录没有，按通过算。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clean: Option<CleanVerdict>,
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

    /// 豁免一个对象的问题，重判结论。返回是否有问题被豁免。
    pub fn waive(&mut self, object: &str, reason: &str) -> bool {
        let (hit, rest): (Vec<_>, Vec<_>) =
            std::mem::take(&mut self.issues).into_iter().partition(|i| i.object == object);
        self.issues = rest;
        let found = !hit.is_empty();
        self.waived.extend(hit.into_iter().map(|issue| WaivedIssue { issue, reason: reason.to_string() }));
        self.settle();
        found
    }

    /// 撤销一个对象的豁免，重判结论。返回是否撤销到了。
    pub fn unwaive(&mut self, object: &str) -> bool {
        let (hit, rest): (Vec<_>, Vec<_>) =
            std::mem::take(&mut self.waived).into_iter().partition(|w| w.issue.object == object);
        self.waived = rest;
        let found = !hit.is_empty();
        self.issues.extend(hit.into_iter().map(|w| w.issue));
        self.settle();
        found
    }

    /// 按当前的问题重判：有问题就未通过，否则回到没有问题时的结论。
    pub(crate) fn settle(&mut self) {
        if self.issues.is_empty() {
            let clean = self.clean.clone();
            self.verdict = clean.as_ref().map_or(Verdict::Pass, |c| c.verdict);
            self.reason = clean.and_then(|c| c.reason);
        } else {
            self.verdict = Verdict::Fail;
            self.reason = None;
            // 人工结论只属于无法判断、待人工确认的条款
            self.review = None;
        }
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
    /// 已豁免的问题数，不计入 `issues`。
    pub waived: u32,
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
        s.waived += r.waived.len() as u32;
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
    // 不查条款是否存在：内置规范换版本后，旧例外留着无害
    let mut targets = HashSet::new();
    for e in &ruleset.exceptions {
        if e.clause.trim().is_empty() || e.object.trim().is_empty() || e.reason.trim().is_empty() {
            return Err(Error::invalid(msg!("lint.exception_empty")));
        }
        if !targets.insert((e.clause.as_str(), e.object.as_str())) {
            return Err(Error::invalid(msg!("lint.exception_duplicate", object = e.object)));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn result(verdict: Verdict) -> ClauseResult {
        ClauseResult {
            clause: "c".into(),
            verdict,
            reason: None,
            issues: vec![],
            waived: vec![],
            clean: None,
            review: None,
        }
    }

    fn issue(object: &str) -> Issue {
        Issue { object: object.into(), table: None, message: "m".into(), suggestion: None }
    }

    #[test]
    fn waiving_all_issues_falls_back_to_the_clean_verdict() {
        let mut r = result(Verdict::Fail);
        r.issues = vec![issue("a"), issue("a"), issue("b")];
        r.clean = Some(CleanVerdict { verdict: Verdict::Manual, reason: Some("note".into()) });
        assert!(r.waive("a", "why"));
        assert_eq!((r.verdict, r.issues.len(), r.waived.len()), (Verdict::Fail, 1, 2));
        assert!(!r.waive("missing", "why"));
        assert!(r.waive("b", "why"));
        assert_eq!(r.verdict, Verdict::Manual);
        assert_eq!(r.reason.as_deref(), Some("note"));
        let s = summarize(std::slice::from_ref(&r));
        assert_eq!((s.issues, s.waived, s.manual), (0, 3, 1));
    }

    #[test]
    fn unwaiving_restores_the_failure_and_drops_the_review() {
        let mut r = result(Verdict::Fail);
        r.issues = vec![issue("a")];
        r.clean = Some(CleanVerdict { verdict: Verdict::Manual, reason: None });
        r.waive("a", "why");
        r.review = Some(Review { verdict: Verdict::Pass, note: String::new() });
        assert!(r.unwaive("a"));
        assert_eq!((r.verdict, r.issues.len(), r.waived.len()), (Verdict::Fail, 1, 0));
        assert_eq!(r.review, None);
        assert!(!r.unwaive("a"));
    }

    #[test]
    fn old_records_without_a_clean_verdict_pass_when_cleared() {
        let mut r = result(Verdict::Fail);
        r.issues = vec![issue("a")];
        r.waive("a", "why");
        assert_eq!(r.verdict, Verdict::Pass);
    }

    #[test]
    fn validate_checks_exceptions() {
        let exception = |object: &str, reason: &str| Exception {
            clause: "c".into(),
            object: object.into(),
            reason: reason.into(),
        };
        let set = |exceptions| Ruleset {
            id: "x".into(),
            name: "n".into(),
            builtin: false,
            clauses: vec![],
            exceptions,
        };
        assert!(validate(&set(vec![exception("a", "r"), exception("b", "r")])).is_ok());
        assert!(validate(&set(vec![exception("a", " ")])).is_err());
        assert!(validate(&set(vec![exception("a", "r"), exception("a", "r2")])).is_err());
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
        let set = |clauses| Ruleset {
            id: "x".into(),
            name: "n".into(),
            builtin: false,
            clauses,
            exceptions: vec![],
        };
        assert!(validate(&set(vec![clause("a", "table.comment")])).is_ok());
        assert!(validate(&set(vec![clause("a", "no.such")])).is_err());
        assert!(validate(&set(vec![clause("a", "table.comment"), clause("a", "table.comment")])).is_err());
    }
}
