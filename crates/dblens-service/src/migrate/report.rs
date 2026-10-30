//! 迁移报告（M5a 设计文档第 5.2 节、M5b 设计文档第 5 节）：由迁移记录写成 markdown——结果、语句汇总、每张表的数据和校验、
//! 跳过的行、序列和自增的调整、失败的语句、映射提示、不迁移的对象及其源库 DDL、脚本。

use std::fmt::Write as _;

use dblens_core::{Error, Lang, Msg, Result, msg};
use dblens_dialect::migrate::script_text;
use dblens_dialect::{DdlKind, Family, Variant};

use super::record::{MigrationRecord, TableState};
use crate::profile::Profile;
use crate::service::Service;

/// 表格单元格中的竖线和换行。
fn cell(s: &str) -> String {
    s.replace('|', "\\|").replace(['\r', '\n'], " ")
}

impl Service {
    /// 写迁移报告。`outcome` 是这次执行的结果；`source` 用来取不迁移的对象的 DDL。
    pub(crate) async fn migration_report(
        &self,
        r: &MigrationRecord,
        source: &Profile,
        outcome: &Result<()>,
        lang: Lang,
    ) -> String {
        let t = |m: Msg| m.render(lang);
        // 脚本按目标的写法结束；连接已删除时按分号
        let target_family = self
            .profile(&r.plan.target.profile)
            .map_or(Family::MySql, |p| Variant::base(p.input.driver).family());
        let mut md = String::new();
        let _ = writeln!(
            md,
            "# {}\n",
            t(msg!("migrate.report.title", source = &r.source_name, target = &r.target_name))
        );
        let state = match outcome {
            Ok(()) if r.finished => t(msg!("migrate.report.finished")),
            Ok(()) => t(msg!("migrate.report.unfinished")),
            Err(Error::Cancelled) => t(msg!("migrate.report.cancelled")),
            Err(e) => t(msg!("migrate.report.failed", error = e.to_string())),
        };
        let _ = writeln!(md, "{state}\n");
        let _ = writeln!(
            md,
            "{}\n",
            t(msg!(
                "migrate.report.statements",
                done = r.done.len(),
                failed = r.statement_errors.len(),
                total = r.statements.len()
            ))
        );
        if !r.copies.is_empty() {
            let _ = writeln!(md, "## {}\n", t(msg!("migrate.report.data")));
            let _ =
                writeln!(md, "| {} |\n|---|---|---|---|---|---|---|", t(msg!("migrate.report.data_header")));
            for c in &r.copies {
                let Some(p) = r.tables.get(&c.source) else { continue };
                let verify = match (&p.verify, p.state) {
                    (Some(v), _) if v.ok() => t(msg!("migrate.report.verify_ok", sampled = v.sampled)),
                    (Some(v), _) => t(msg!(
                        "migrate.report.verify_bad",
                        expected = v.expected_rows,
                        target = v.target_rows,
                        mismatches = v.mismatches.len()
                    )),
                    (None, TableState::Failed | TableState::Skipped) => p.error.clone().unwrap_or_default(),
                    (None, _) => String::new(),
                };
                let source_rows = p.verify.as_ref().map(|v| v.source_rows.to_string()).unwrap_or_default();
                let _ = writeln!(
                    md,
                    "| {} → {} | {} | {} | {} | {} | {:.1} s | {} |",
                    cell(&c.source),
                    cell(&c.target),
                    t(msg!("migrate.report.state", state = format!("{:?}", p.state).to_lowercase())),
                    source_rows,
                    p.rows,
                    p.skipped,
                    p.elapsed_ms as f64 / 1000.0,
                    cell(&verify)
                );
            }
            md.push('\n');
            for c in &r.copies {
                let Some(v) = r.tables.get(&c.source).and_then(|p| p.verify.as_ref()) else { continue };
                if v.mismatches.is_empty() {
                    continue;
                }
                let _ = writeln!(md, "### {}\n", t(msg!("migrate.report.mismatches", table = &c.source)));
                for m in &v.mismatches {
                    if m.column.is_empty() {
                        let _ = writeln!(md, "- {}: {}", m.key, t(msg!("migrate.report.row_missing")));
                    } else {
                        let _ = writeln!(md, "- {} · {}: `{}` → `{}`", m.key, m.column, m.source, m.target);
                    }
                }
                md.push('\n');
            }
        }
        if !r.skipped_rows.is_empty() {
            let _ = writeln!(md, "## {}\n", t(msg!("migrate.report.skipped_rows")));
            for s in &r.skipped_rows {
                let _ = writeln!(md, "- {} ({}): {}", s.table, s.key, s.error);
            }
            md.push('\n');
        }
        if !r.adjustments.is_empty() {
            let _ = writeln!(md, "## {}\n", t(msg!("migrate.report.adjustments")));
            for a in &r.adjustments {
                match &a.error {
                    None => {
                        let _ = writeln!(md, "- {}: `{}`", a.object, a.sql);
                    }
                    Some(e) => {
                        let _ = writeln!(md, "- {}: `{}` — {}", a.object, a.sql, e);
                    }
                }
            }
            md.push('\n');
        }
        if !r.statement_errors.is_empty() {
            let _ = writeln!(md, "## {}\n", t(msg!("migrate.report.failures")));
            for (i, e) in &r.statement_errors {
                if let Some(s) = r.statements.get(*i) {
                    let _ = writeln!(md, "```sql\n{}\n```\n\n{e}\n", s.sql);
                }
            }
        }
        if !r.notes.is_empty() {
            let _ = writeln!(md, "## {}\n", t(msg!("migrate.report.notes")));
            for n in &r.notes {
                let _ = writeln!(md, "- {n}");
            }
            md.push('\n');
        }
        if !r.others.is_empty() {
            let _ = writeln!(md, "## {}\n", t(msg!("migrate.report.others")));
            for o in &r.others {
                let _ = writeln!(md, "### {} {}\n", o.kind, o.name);
                let ddl = match (o.kind.as_str(), &o.table) {
                    ("trigger", Some(_)) => o.definition.clone(),
                    (kind, _) => match DdlKind::parse(kind) {
                        Some(k) => self.metadata.ddl(source, &r.plan.source.schema, k, &o.name).await.ok(),
                        None => None,
                    },
                };
                match ddl {
                    Some(d) => {
                        let _ = writeln!(md, "```sql\n{}\n```\n", d.trim());
                    }
                    None => {
                        let _ = writeln!(md, "{}\n", t(msg!("migrate.report.no_ddl")));
                    }
                }
            }
        }
        let _ = writeln!(
            md,
            "## {}\n\n```sql\n{}\n```",
            t(msg!("migrate.report.script")),
            script_text(target_family, &r.statements)
        );
        md
    }
}
