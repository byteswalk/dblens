//! Word、Excel 检查报告和 Excel 自查表，复用 M4c 文档导出的写出代码。

use dblens_core::Result;
use rust_xlsxwriter::Workbook;

use super::report::{Report, Row, labels};
use crate::document::{Cell, Sheet, assemble, xlsx_error};

fn text(s: &str) -> Cell {
    Cell::Text(s.to_string())
}

fn opt(s: &Option<String>) -> Cell {
    Cell::Text(s.clone().unwrap_or_default())
}

/// 封面信息：标签和值。
fn cover(r: &Report, l: &impl Fn(&str) -> String) -> Vec<(String, String)> {
    let mut items = vec![
        (l("object"), r.target.to_string()),
        (l("ruleset"), r.ruleset_name.to_string()),
        (l("generated"), r.generated.clone()),
    ];
    if let Some(who) = r.reviewer.filter(|w| !w.is_empty()) {
        items.push((l("reviewer"), who.to_string()));
    }
    if let Some(rate) = r.pass_rate() {
        items.push((l("pass_rate"), rate));
    }
    items
}

/// 各结论的计数。通过率之外的计数并列给出，避免只看通过率偏乐观。
fn counts(r: &Report, l: &impl Fn(&str) -> String) -> Vec<(String, u32)> {
    let s = &r.summary;
    vec![
        (l("pass"), s.pass),
        (l("fail"), s.fail),
        (l("unknown"), s.unknown),
        (l("manual"), s.manual),
        (l("not_applicable"), s.not_applicable),
        (l("issues"), s.issues),
        (l("waived"), s.waived),
    ]
}

/// 自查表的说明列：原因、问题、豁免写在一格里。
fn note(row: &Row, l: &impl Fn(&str) -> String) -> String {
    let mut parts: Vec<String> = row.reason.clone().into_iter().collect();
    parts.extend(row.issues.iter().map(|i| format!("{}: {}", i.object, i.message)));
    let waived = l("waived");
    parts.extend(row.waived.iter().map(|w| format!("{waived} {}: {}", w.issue.object, w.reason)));
    parts.join("; ")
}

pub(super) fn docx(r: &Report) -> Result<Vec<u8>> {
    let lb = labels(r.lang);
    let l = |k: &str| lb[k].as_str().unwrap_or_default().to_string();
    assemble(None, r.lang, |b| {
        b.para(Some("title"), &l("title"));
        for (k, v) in cover(r, &l) {
            b.para(None, &format!("{k}: {v}"));
        }

        b.heading(1, &l("summary"), None);
        let rows =
            counts(r, &l).into_iter().map(|(k, n)| vec![Cell::Text(k), Cell::Text(n.to_string())]).collect();
        b.table(&[l("verdict"), l("count")], &[3, 1], rows);

        b.heading(1, &l("clause_results"), None);
        let rows = r
            .rows
            .iter()
            .map(|c| {
                vec![text(&c.section), text(&c.text), text(&c.severity), text(&c.verdict), opt(&c.reason)]
            })
            .collect();
        b.table(
            &[l("section"), l("clause"), l("severity"), l("verdict"), l("reason")],
            &[2, 7, 2, 2, 5],
            rows,
        );

        b.heading(1, &l("issue_details"), None);
        let failed: Vec<&Row> = r.rows.iter().filter(|c| !c.issues.is_empty()).collect();
        if failed.is_empty() {
            b.para(None, &l("no_issues"));
        }
        for c in failed {
            b.heading(2, &format!("{} · {}", c.section, c.text), None);
            let rows = c
                .issues
                .iter()
                .map(|i| vec![text(&i.object), text(&i.message), opt(&i.suggestion)])
                .collect();
            b.table(&[l("object_col"), l("message"), l("suggestion")], &[4, 5, 5], rows);
        }

        if r.rows.iter().any(|c| c.manual) {
            b.heading(1, &l("manual_list"), None);
            let rows = r
                .rows
                .iter()
                .filter(|c| c.manual)
                .map(|c| vec![text(&c.text), text(&c.verdict), opt(&c.reason)])
                .collect();
            b.table(&[l("clause"), l("verdict"), l("reason")], &[6, 2, 6], rows);
        }

        if r.rows.iter().any(|c| !c.waived.is_empty()) {
            b.heading(1, &l("waived_list"), None);
            let rows = r
                .rows
                .iter()
                .flat_map(|c| {
                    c.waived.iter().map(|w| {
                        vec![text(&c.text), text(&w.issue.object), text(&w.issue.message), text(&w.reason)]
                    })
                })
                .collect();
            b.table(&[l("clause"), l("object_col"), l("message"), l("waive_reason")], &[5, 4, 5, 4], rows);
        }
    })
}

pub(super) fn xlsx(r: &Report) -> Result<Vec<u8>> {
    let lb = labels(r.lang);
    let l = |k: &str| lb[k].as_str().unwrap_or_default().to_string();
    let mut book = Workbook::new();
    let bold = rust_xlsxwriter::Format::new().set_bold();
    let mut truncated = 0;

    {
        let ws = book.add_worksheet().set_name(l("summary")).map_err(xlsx_error)?;
        let mut sh = Sheet::new(ws, &mut truncated);
        sh.header(&[l("item"), l("result")], &bold)?;
        for (k, v) in cover(r, &l) {
            sh.cell(0, &k)?;
            sh.cell(1, &v)?;
            sh.next();
        }
        for (k, n) in counts(r, &l) {
            sh.cell(0, &k)?;
            sh.number(1, n as usize)?;
            sh.next();
        }
        sh.finish()?;
    }

    {
        let ws = book.add_worksheet().set_name(l("clause_results")).map_err(xlsx_error)?;
        let mut sh = Sheet::new(ws, &mut truncated);
        let headers =
            [l("section"), l("clause"), l("severity"), l("verdict"), l("issues"), l("waived"), l("reason")];
        sh.header(&headers, &bold)?;
        for c in &r.rows {
            sh.cell(0, &c.section)?;
            sh.cell(1, &c.text)?;
            sh.cell(2, &c.severity)?;
            sh.cell(3, &c.verdict)?;
            sh.number(4, c.issues.len())?;
            sh.number(5, c.waived.len())?;
            sh.cell(6, c.reason.as_deref().unwrap_or_default())?;
            sh.next();
        }
        sh.finish()?;
    }

    {
        let ws = book.add_worksheet().set_name(l("issue_details")).map_err(xlsx_error)?;
        let mut sh = Sheet::new(ws, &mut truncated);
        let headers = [l("section"), l("clause"), l("object_col"), l("message"), l("suggestion")];
        sh.header(&headers, &bold)?;
        for c in &r.rows {
            for i in &c.issues {
                sh.cell(0, &c.section)?;
                sh.cell(1, &c.text)?;
                sh.cell(2, &i.object)?;
                sh.cell(3, &i.message)?;
                sh.cell(4, i.suggestion.as_deref().unwrap_or_default())?;
                sh.next();
            }
        }
        sh.finish()?;
    }

    {
        let ws = book.add_worksheet().set_name(l("manual_list")).map_err(xlsx_error)?;
        let mut sh = Sheet::new(ws, &mut truncated);
        sh.header(&[l("clause"), l("verdict"), l("reason")], &bold)?;
        for c in r.rows.iter().filter(|c| c.manual) {
            sh.cell(0, &c.text)?;
            sh.cell(1, &c.verdict)?;
            sh.cell(2, c.reason.as_deref().unwrap_or_default())?;
            sh.next();
        }
        sh.finish()?;
    }

    {
        let ws = book.add_worksheet().set_name(l("waived_list")).map_err(xlsx_error)?;
        let mut sh = Sheet::new(ws, &mut truncated);
        sh.header(&[l("clause"), l("object_col"), l("message"), l("waive_reason")], &bold)?;
        for c in &r.rows {
            for w in &c.waived {
                sh.cell(0, &c.text)?;
                sh.cell(1, &w.issue.object)?;
                sh.cell(2, &w.issue.message)?;
                sh.cell(3, &w.reason)?;
                sh.next();
            }
        }
        sh.finish()?;
    }

    book.save_to_buffer().map_err(xlsx_error)
}

/// 自查表：序号、分类、检查项、约束、结果、说明、检查时间、检查人。
pub(super) fn checklist(r: &Report) -> Result<Vec<u8>> {
    let lb = labels(r.lang);
    let l = |k: &str| lb[k].as_str().unwrap_or_default().to_string();
    let mut book = Workbook::new();
    let bold = rust_xlsxwriter::Format::new().set_bold();
    let mut truncated = 0;
    let ws = book.add_worksheet().set_name(l("checklist")).map_err(xlsx_error)?;
    let mut sh = Sheet::new(ws, &mut truncated);
    let headers = [
        l("no"),
        l("section"),
        l("item"),
        l("constraint"),
        l("result"),
        l("reason"),
        l("checked_at"),
        l("reviewer"),
    ];
    sh.header(&headers, &bold)?;
    for (i, c) in r.rows.iter().enumerate() {
        sh.number(0, i + 1)?;
        sh.cell(1, &c.section)?;
        sh.cell(2, &c.text)?;
        sh.cell(3, &c.constraint)?;
        sh.cell(4, &c.verdict)?;
        sh.cell(5, &note(c, &l))?;
        sh.cell(6, &r.generated)?;
        sh.cell(7, r.reviewer.unwrap_or_default())?;
        sh.next();
    }
    sh.finish()?;
    book.save_to_buffer().map_err(xlsx_error)
}

#[cfg(test)]
mod tests {
    use std::io::{Cursor, Read};

    use dblens_core::Lang;
    use dblens_lint::{ClauseResult, CleanVerdict, Issue, Section, Severity, Verdict};

    use super::super::report::html;
    use super::super::{ClauseMeta, LintRun};
    use super::*;

    fn run() -> LintRun {
        let meta = |id: &str, section, text: &str, severity| ClauseMeta {
            id: id.into(),
            section,
            text: text.into(),
            severity,
        };
        let result = |id: &str, verdict| ClauseResult {
            clause: id.into(),
            verdict,
            reason: None,
            issues: vec![],
            waived: vec![],
            clean: Some(CleanVerdict { verdict: Verdict::Pass, reason: None }),
            review: None,
        };
        let issue = |object: &str| Issue {
            object: object.into(),
            table: None,
            message: format!("{object} 缺注释"),
            suggestion: Some("补上注释".into()),
        };
        let mut failed = result("a", Verdict::Fail);
        failed.issues = vec![issue("orders"), issue("tmp_old")];
        failed.waive("tmp_old", "临时表，下月删除");
        let mut manual = result("b", Verdict::Manual);
        manual.reason = Some("请确认范式".into());
        LintRun {
            id: "r".into(),
            created_at: 0,
            target: "demo · public".into(),
            ruleset_id: "general".into(),
            ruleset_name: "通用".into(),
            family: "postgres".into(),
            clauses: vec![
                meta("a", Section::Table, "表要有注释", Severity::Error),
                meta("b", Section::Design, "符合第三范式", Severity::Warning),
                meta("c", Section::Index, "索引数量", Severity::Info),
            ],
            results: vec![failed, manual, result("c", Verdict::Pass)],
        }
    }

    fn part(bytes: &[u8], name: &str) -> String {
        let mut z = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
        let mut s = String::new();
        z.by_name(name).unwrap().read_to_string(&mut s).unwrap();
        s
    }

    #[test]
    fn html_lists_manual_clauses_and_waivers() {
        let run = run();
        let page = html(&Report::new(&run, Some("张三"), Lang::ZhCn)).unwrap();
        for s in ["人工确认清单", "请确认范式", "豁免清单", "临时表，下月删除", "tmp_old", "张三"]
        {
            assert!(page.contains(s), "{s}");
        }
    }

    #[test]
    fn docx_has_every_section_and_no_field_update_prompt() {
        let run = run();
        let bytes = docx(&Report::new(&run, Some("张三"), Lang::ZhCn)).unwrap();
        let doc = part(&bytes, "word/document.xml");
        for s in
            ["数据库设计规范检查报告", "条款结论", "orders 缺注释", "人工确认清单", "豁免清单", "张三", "50%"]
        {
            assert!(doc.contains(s), "{s}");
        }
        assert!(!part(&bytes, "word/settings.xml").contains("updateFields"));
    }

    #[test]
    fn xlsx_has_five_sheets() {
        let run = run();
        let bytes = xlsx(&Report::new(&run, None, Lang::ZhCn)).unwrap();
        let book = part(&bytes, "xl/workbook.xml");
        for name in ["统计", "条款结论", "问题明细", "人工确认清单", "豁免清单"] {
            assert!(book.contains(&format!("name=\"{name}\"")), "{book}");
        }
        let strings = part(&bytes, "xl/sharedStrings.xml");
        assert!(strings.contains("orders 缺注释") && strings.contains("临时表，下月删除"));
    }

    #[test]
    fn checklist_columns_follow_the_design() {
        let run = run();
        let bytes = checklist(&Report::new(&run, Some("张三"), Lang::ZhCn)).unwrap();
        assert!(part(&bytes, "xl/workbook.xml").contains("name=\"自查表\""));
        let strings = part(&bytes, "xl/sharedStrings.xml");
        let order = ["序号", "分类", "检查项", "约束", "结果", "说明", "检查时间", "检查人"];
        let at: Vec<usize> = order.iter().map(|h| strings.find(h).unwrap_or_else(|| panic!("{h}"))).collect();
        assert!(at.windows(2).all(|w| w[0] < w[1]), "{strings}");
        for s in ["强制", "建议", "参考", "orders: orders 缺注释", "已豁免 tmp_old: 临时表，下月删除", "张三"]
        {
            assert!(strings.contains(s), "{s}");
        }
    }
}
