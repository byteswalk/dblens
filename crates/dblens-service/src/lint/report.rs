//! 检查报告：把检查记录整理成报告数据，HTML 用 minijinja 渲染，标签走词条。
//! Word、Excel 见 `report_office.rs`。

use dblens_core::{Error, Lang, Result, msg};
use dblens_lint::{Issue, Section, Severity, Summary, Verdict, WaivedIssue};
use minijinja::value::Serde;
use minijinja::{AutoEscape, Environment};
use serde::Serialize;
use serde_json::{Value, json};

use super::{LintRun, is_reviewable};

const TEMPLATE: &str = include_str!("report.html.j2");

fn verdict_label(v: Verdict, lang: Lang) -> String {
    match v {
        Verdict::Pass => msg!("lint.verdict.pass"),
        Verdict::Fail => msg!("lint.verdict.fail"),
        Verdict::Unknown => msg!("lint.verdict.unknown"),
        Verdict::Manual => msg!("lint.verdict.manual"),
        Verdict::NotApplicable => msg!("lint.verdict.not_applicable"),
    }
    .render(lang)
}

fn verdict_slug(v: Verdict) -> &'static str {
    match v {
        Verdict::Pass => "pass",
        Verdict::Fail => "fail",
        Verdict::Unknown => "unknown",
        Verdict::Manual => "manual",
        Verdict::NotApplicable => "not_applicable",
    }
}

fn severity_label(s: Severity, lang: Lang) -> String {
    match s {
        Severity::Error => msg!("lint.severity.error"),
        Severity::Warning => msg!("lint.severity.warning"),
        Severity::Info => msg!("lint.severity.info"),
    }
    .render(lang)
}

/// 自查表的约束列：错误为强制，警告为建议，提示为参考。
fn constraint_label(s: Severity, lang: Lang) -> String {
    match s {
        Severity::Error => msg!("lint.report.constraint_required"),
        Severity::Warning => msg!("lint.report.constraint_recommended"),
        Severity::Info => msg!("lint.report.constraint_reference"),
    }
    .render(lang)
}

fn section_label(s: Section, lang: Lang) -> String {
    match s {
        Section::Naming => msg!("lint.section.naming"),
        Section::Table => msg!("lint.section.table"),
        Section::PrimaryKey => msg!("lint.section.primary_key"),
        Section::Column => msg!("lint.section.column"),
        Section::Index => msg!("lint.section.index"),
        Section::Constraint => msg!("lint.section.constraint"),
        Section::Consistency => msg!("lint.section.consistency"),
        Section::Distribution => msg!("lint.section.distribution"),
        Section::Design => msg!("lint.section.design"),
    }
    .render(lang)
}

/// 报告用到的全部标签，HTML 和 Office 共用。
pub(super) fn labels(lang: Lang) -> Value {
    json!({
        "title": msg!("lint.report.title").render(lang),
        "object": msg!("lint.report.object").render(lang),
        "ruleset": msg!("lint.report.ruleset").render(lang),
        "generated": msg!("lint.report.generated").render(lang),
        "reviewer": msg!("lint.report.reviewer").render(lang),
        "pass_rate": msg!("lint.report.pass_rate").render(lang),
        "summary": msg!("lint.report.summary").render(lang),
        "count": msg!("lint.report.count").render(lang),
        "clause": msg!("lint.report.clause").render(lang),
        "section": msg!("lint.report.section").render(lang),
        "severity": msg!("lint.report.severity").render(lang),
        "verdict": msg!("lint.report.verdict").render(lang),
        "issues": msg!("lint.report.issues").render(lang),
        "reason": msg!("lint.report.reason").render(lang),
        "object_col": msg!("lint.report.object_col").render(lang),
        "message": msg!("lint.report.message").render(lang),
        "suggestion": msg!("lint.report.suggestion").render(lang),
        "manual_list": msg!("lint.report.manual_list").render(lang),
        "no_issues": msg!("lint.report.no_issues").render(lang),
        "clause_results": msg!("lint.report.clause_results").render(lang),
        "issue_details": msg!("lint.report.issue_details").render(lang),
        "waived": msg!("lint.report.waived").render(lang),
        "waived_list": msg!("lint.report.waived_list").render(lang),
        "waive_reason": msg!("lint.report.waive_reason").render(lang),
        "no": msg!("lint.report.no").render(lang),
        "item": msg!("lint.report.item").render(lang),
        "constraint": msg!("lint.report.constraint").render(lang),
        "result": msg!("lint.report.result").render(lang),
        "checked_at": msg!("lint.report.checked_at").render(lang),
        "checklist": msg!("lint.report.checklist").render(lang),
        "pass": verdict_label(Verdict::Pass, lang),
        "fail": verdict_label(Verdict::Fail, lang),
        "unknown": verdict_label(Verdict::Unknown, lang),
        "manual": verdict_label(Verdict::Manual, lang),
        "not_applicable": verdict_label(Verdict::NotApplicable, lang),
    })
}

/// 报告里的一条条款。
#[derive(Debug, Serialize)]
pub(super) struct Row {
    pub section: String,
    pub text: String,
    pub severity: String,
    pub constraint: String,
    pub verdict: String,
    pub verdict_slug: &'static str,
    pub reason: Option<String>,
    /// 自动查不出结论，进人工确认清单。
    pub manual: bool,
    pub issues: Vec<Issue>,
    pub waived: Vec<WaivedIssue>,
}

/// 各种格式共用的报告数据。
#[derive(Debug)]
pub(super) struct Report<'a> {
    pub lang: Lang,
    pub target: &'a str,
    pub ruleset_name: &'a str,
    pub reviewer: Option<&'a str>,
    pub generated: String,
    pub summary: Summary,
    pub rows: Vec<Row>,
}

impl<'a> Report<'a> {
    pub(super) fn new(run: &'a LintRun, reviewer: Option<&'a str>, lang: Lang) -> Self {
        let generated = chrono::DateTime::from_timestamp_millis(run.created_at)
            .map(|t| t.with_timezone(&chrono::Local).format("%Y-%m-%d %H:%M").to_string())
            .unwrap_or_default();
        let rows = run
            .results
            .iter()
            .map(|r| {
                let meta = run.clauses.iter().find(|c| c.id == r.clause);
                let effective = r.effective();
                let reason = r
                    .review
                    .as_ref()
                    .map(|rv| rv.note.clone())
                    .filter(|n| !n.is_empty())
                    .or_else(|| r.reason.clone());
                Row {
                    section: meta.map(|m| section_label(m.section, lang)).unwrap_or_default(),
                    text: meta.map(|m| m.text.clone()).unwrap_or_else(|| r.clause.clone()),
                    severity: meta.map(|m| severity_label(m.severity, lang)).unwrap_or_default(),
                    constraint: meta.map(|m| constraint_label(m.severity, lang)).unwrap_or_default(),
                    verdict: verdict_label(effective, lang),
                    verdict_slug: verdict_slug(effective),
                    reason,
                    manual: is_reviewable(r.verdict),
                    issues: r.issues.clone(),
                    waived: r.waived.clone(),
                }
            })
            .collect();
        Self {
            lang,
            target: &run.target,
            ruleset_name: &run.ruleset_name,
            reviewer,
            generated,
            summary: run.summary(),
            rows,
        }
    }

    pub(super) fn pass_rate(&self) -> Option<String> {
        self.summary.pass_rate.map(|r| format!("{r}%"))
    }
}

pub(super) fn html(report: &Report) -> Result<String> {
    let ctx = json!({
        "lb": labels(report.lang),
        "target": report.target,
        "ruleset_name": report.ruleset_name,
        "generated": report.generated,
        "reviewer": report.reviewer,
        "summary": report.summary,
        "rows": report.rows,
        "has_issues": report.rows.iter().any(|r| !r.issues.is_empty()),
        "has_manual": report.rows.iter().any(|r| r.manual),
        "has_waived": report.rows.iter().any(|r| !r.waived.is_empty()),
    });

    let mut env = Environment::new();
    env.set_auto_escape_callback(|_| AutoEscape::Html);
    env.add_template("report", TEMPLATE).map_err(render_err)?;
    let tmpl = env.get_template("report").map_err(render_err)?;
    tmpl.render(Serde(&ctx)).map_err(render_err)
}

fn render_err(e: minijinja::Error) -> Error {
    Error::internal(msg!("lint.yaml_parse", error = e))
}
