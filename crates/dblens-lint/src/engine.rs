//! 遍历条款，运行规则，判定结论。

use dblens_core::schema::Schema;
use dblens_core::{Lang, msg};
use dblens_dialect::Family;

use crate::model::{Clause, ClauseResult, CleanVerdict, Exception, Issue, RuleRef, Ruleset, Verdict};
use crate::rules::{self, Ctx, Outcome};

/// 一次检查的输入。模型文件只有表，`objects_known` 为 false，
/// 视图、序列、存储过程这些规则就报无法判断。
#[derive(Debug, Clone)]
pub struct LintInput {
    pub schema: Schema,
    pub family: Family,
    pub objects_known: bool,
}

/// 规范里 `databases` 用的家族名。
pub fn family_key(family: Family) -> &'static str {
    match family {
        Family::MySql => "mysql",
        Family::Postgres => "postgres",
        Family::Oracle => "oracle",
        Family::Dameng => "dameng",
        Family::SqlServer => "sqlserver",
        Family::Informix => "informix",
    }
}

/// 逐条检查启用的条款。每开始一条调用一次 `on_clause`，用于显示进度。
pub fn check(
    input: &LintInput,
    ruleset: &Ruleset,
    lang: Lang,
    mut on_clause: impl FnMut(),
) -> Vec<ClauseResult> {
    let ctx = Ctx { input };
    ruleset
        .clauses
        .iter()
        .filter(|c| c.enabled)
        .map(|c| {
            on_clause();
            check_clause(&ctx, c, &ruleset.exceptions, lang)
        })
        .collect()
}

fn applies(rule: &RuleRef, family: Family) -> bool {
    rule.databases.is_empty() || rule.databases.iter().any(|d| d == "all" || d == family_key(family))
}

fn check_clause(ctx: &Ctx, clause: &Clause, exceptions: &[Exception], lang: Lang) -> ClauseResult {
    let note = clause.manual_note.clone().filter(|n| !n.trim().is_empty());
    let mut result = ClauseResult {
        clause: clause.id.clone(),
        verdict: Verdict::Manual,
        reason: note.clone(),
        issues: vec![],
        waived: vec![],
        clean: None,
        review: None,
    };
    if clause.rules.is_empty() {
        return result;
    }
    let mut all_skipped = true;
    let mut unknown: Option<String> = None;
    for rule in &clause.rules {
        if !applies(rule, ctx.input.family) {
            continue;
        }
        match rules::run(ctx, rule) {
            Outcome::NotApplicable => {}
            Outcome::Unknown(reason) => {
                all_skipped = false;
                unknown.get_or_insert_with(|| reason.render(lang));
            }
            Outcome::Findings(findings, fix) => {
                all_skipped = false;
                let suggestion = fix.map(|f| f().render(lang));
                result.issues.extend(findings.into_iter().map(|f| Issue {
                    object: f.object,
                    table: f.table,
                    message: f.message.render(lang),
                    suggestion: suggestion.clone(),
                }));
            }
        }
    }
    // 判定顺序：未通过 → 全部不涉及 → 无法判断 → 待人工确认 → 通过
    let (verdict, reason) = if all_skipped {
        (Verdict::NotApplicable, Some(msg!("lint.not_applicable").render(lang)))
    } else if unknown.is_some() {
        (Verdict::Unknown, unknown)
    } else if note.is_some() {
        (Verdict::Manual, note)
    } else {
        (Verdict::Pass, None)
    };
    result.clean = Some(CleanVerdict { verdict, reason });
    for e in exceptions.iter().filter(|e| e.clause == clause.id) {
        result.waive(&e.object, &e.reason);
    }
    result.settle();
    result
}

#[cfg(test)]
mod tests {
    use dblens_core::schema::Table;
    use serde_json::Map;

    use super::*;
    use crate::model::{Section, Severity};

    fn input(tables: Vec<Table>) -> LintInput {
        let schema = Schema { name: "s".into(), tables, ..Default::default() };
        LintInput { schema, family: Family::Postgres, objects_known: true }
    }

    fn rule(id: &str, databases: &[&str]) -> RuleRef {
        RuleRef {
            id: id.into(),
            params: Map::new(),
            databases: databases.iter().map(|d| d.to_string()).collect(),
        }
    }

    fn clause(rules: Vec<RuleRef>, note: Option<&str>) -> Clause {
        Clause {
            id: "c".into(),
            section: Section::Table,
            text: "t".into(),
            enabled: true,
            severity: Severity::Warning,
            manual_note: note.map(str::to_string),
            rules,
        }
    }

    fn verdict(tables: Vec<Table>, c: Clause) -> ClauseResult {
        let set = Ruleset {
            id: "r".into(),
            name: "r".into(),
            builtin: false,
            clauses: vec![c],
            exceptions: vec![],
        };
        check(&input(tables), &set, Lang::En, || {}).remove(0)
    }

    fn table(comment: Option<&str>) -> Table {
        Table { name: "t".into(), comment: comment.map(str::to_string), ..Default::default() }
    }

    #[test]
    fn clause_without_rules_needs_manual_review() {
        assert_eq!(verdict(vec![], clause(vec![], None)).verdict, Verdict::Manual);
    }

    #[test]
    fn passing_rules_give_pass() {
        let r = verdict(vec![table(Some("x"))], clause(vec![rule("table.comment", &[])], None));
        assert_eq!(r.verdict, Verdict::Pass);
    }

    #[test]
    fn a_finding_wins_over_unknown_and_note() {
        let c = clause(vec![rule("db.charset", &[]), rule("table.comment", &[])], Some("n"));
        let r = verdict(vec![table(None)], c);
        assert_eq!(r.verdict, Verdict::Fail);
        assert_eq!(r.issues.len(), 1);
        assert!(r.issues[0].suggestion.is_some());
    }

    #[test]
    fn all_rules_filtered_by_database_is_not_applicable() {
        let r = verdict(vec![table(None)], clause(vec![rule("table.comment", &["mysql"])], None));
        assert_eq!(r.verdict, Verdict::NotApplicable);
    }

    #[test]
    fn unknown_wins_over_pass_and_note() {
        let c = clause(vec![rule("table.comment", &[]), rule("db.charset", &[])], Some("n"));
        let r = verdict(vec![table(Some("x"))], c);
        assert_eq!(r.verdict, Verdict::Unknown);
        assert!(r.reason.is_some());
    }

    #[test]
    fn passing_rules_with_a_note_still_need_review() {
        let r = verdict(vec![table(Some("x"))], clause(vec![rule("table.comment", &[])], Some("n")));
        assert_eq!(r.verdict, Verdict::Manual);
        assert_eq!(r.reason.as_deref(), Some("n"));
    }

    #[test]
    fn disabled_clauses_are_skipped_and_progress_is_reported() {
        let mut off = clause(vec![], None);
        off.enabled = false;
        let set = Ruleset {
            id: "r".into(),
            name: "r".into(),
            builtin: false,
            clauses: vec![off, clause(vec![], None)],
            exceptions: vec![],
        };
        let mut seen = 0;
        let results = check(&input(vec![]), &set, Lang::En, || seen += 1);
        assert_eq!((results.len(), seen), (1, 1));
    }

    fn with_exception(object: &str, c: Clause) -> ClauseResult {
        let exception = Exception { clause: "c".into(), object: object.into(), reason: "why".into() };
        let set = Ruleset {
            id: "r".into(),
            name: "r".into(),
            builtin: false,
            clauses: vec![c],
            exceptions: vec![exception],
        };
        check(&input(vec![table(None)]), &set, Lang::En, || {}).remove(0)
    }

    #[test]
    fn an_exception_waives_the_issue_and_rejudges_the_clause() {
        let r = with_exception("t", clause(vec![rule("table.comment", &[])], None));
        assert_eq!(r.verdict, Verdict::Pass);
        assert_eq!((r.issues.len(), r.waived.len()), (0, 1));
        assert_eq!(r.waived[0].reason, "why");
    }

    #[test]
    fn a_waived_clause_keeps_its_unknown_rules() {
        let c = clause(vec![rule("db.charset", &[]), rule("table.comment", &[])], None);
        assert_eq!(with_exception("t", c).verdict, Verdict::Unknown);
    }

    #[test]
    fn an_exception_for_another_object_changes_nothing() {
        let r = with_exception("other", clause(vec![rule("table.comment", &[])], None));
        assert_eq!(r.verdict, Verdict::Fail);
        assert!(r.waived.is_empty());
    }
}
