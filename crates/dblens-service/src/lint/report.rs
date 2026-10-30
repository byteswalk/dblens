//! HTML 检查报告（M7a）。用 minijinja 渲染，标签走词条。

use dblens_core::{Error, Lang, Result, msg};
use dblens_lint::{Section, Severity, Verdict};
use minijinja::value::Serde;
use minijinja::{AutoEscape, Environment};
use serde_json::{Value, json};

use super::LintRun;

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

fn labels(lang: Lang) -> Value {
    json!({
        "title": msg!("lint.report.title").render(lang),
        "object": msg!("lint.report.object").render(lang),
        "ruleset": msg!("lint.report.ruleset").render(lang),
        "generated": msg!("lint.report.generated").render(lang),
        "reviewer": msg!("lint.report.reviewer").render(lang),
        "pass_rate": msg!("lint.report.pass_rate").render(lang),
        "summary": msg!("lint.report.summary").render(lang),
        "clause": msg!("lint.report.clause").render(lang),
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
        "pass": verdict_label(Verdict::Pass, lang),
        "fail": verdict_label(Verdict::Fail, lang),
        "unknown": verdict_label(Verdict::Unknown, lang),
        "manual": verdict_label(Verdict::Manual, lang),
        "not_applicable": verdict_label(Verdict::NotApplicable, lang),
    })
}

/// 把条款结果连同说明组织成报告需要的形状。
fn rows(run: &LintRun, lang: Lang) -> Vec<Value> {
    run.results
        .iter()
        .map(|r| {
            let meta = run.clauses.iter().find(|c| c.id == r.clause);
            let section = meta.map(|m| section_label(m.section, lang)).unwrap_or_default();
            let text = meta.map(|m| m.text.clone()).unwrap_or_else(|| r.clause.clone());
            let severity = meta.map(|m| severity_label(m.severity, lang)).unwrap_or_default();
            let effective = r.effective();
            let reason = r
                .review
                .as_ref()
                .map(|rv| rv.note.clone())
                .filter(|n| !n.is_empty())
                .or_else(|| r.reason.clone());
            json!({
                "section": section,
                "text": text,
                "severity": severity,
                "verdict": verdict_label(effective, lang),
                "verdict_slug": verdict_slug(effective),
                "reason": reason,
                "issues": r.issues.iter().map(|i| json!({
                    "object": i.object,
                    "message": i.message,
                    "suggestion": i.suggestion,
                })).collect::<Vec<_>>(),
            })
        })
        .collect()
}

pub(super) fn html(run: &LintRun, reviewer: Option<&str>, lang: Lang) -> Result<String> {
    let summary = run.summary();
    let generated = chrono::DateTime::from_timestamp_millis(run.created_at)
        .map(|t| t.with_timezone(&chrono::Local).format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_default();
    let rows = rows(run, lang);
    let has_issues = rows.iter().any(|r| r["issues"].as_array().is_some_and(|a| !a.is_empty()));
    let ctx = json!({
        "lb": labels(lang),
        "target": run.target,
        "ruleset_name": run.ruleset_name,
        "generated": generated,
        "reviewer": reviewer,
        "summary": {
            "pass": summary.pass,
            "fail": summary.fail,
            "unknown": summary.unknown,
            "manual": summary.manual,
            "not_applicable": summary.not_applicable,
            "issues": summary.issues,
            "pass_rate": summary.pass_rate,
        },
        "rows": rows,
        "has_issues": has_issues,
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
