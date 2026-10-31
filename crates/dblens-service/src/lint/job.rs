//! 规范库的增删改查、发起检查、人工结论、导出报告。

use std::sync::Arc;

use dblens_core::schema::Schema;
use dblens_core::{Error, Result, current_lang, msg};
use dblens_dialect::meta::{MetaArgs, MetaKey};
use dblens_lint::{
    ClauseResult, Exception, LintInput, RuleInfo, Ruleset, apply, builtin, builtin_ids, check, diff,
    rule_infos, validate,
};

use super::report::{self, Report};
use super::report_office;
use super::{
    ClauseMeta, LintRequest, LintRun, LintRunSummary, LintSource, ReviewInput, RulesetSummary, WaiveInput,
    is_reviewable,
};
use crate::service::Service;
use crate::task::{TaskKind, TaskPhase};

/// 报告格式。`Checklist` 是 Excel 的自查表。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReportFormat {
    Html,
    Docx,
    Xlsx,
    Checklist,
}

impl ReportFormat {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "html" => Some(Self::Html),
            "docx" => Some(Self::Docx),
            "xlsx" => Some(Self::Xlsx),
            "checklist" => Some(Self::Checklist),
            _ => None,
        }
    }

    fn file_name(self, short: &str) -> String {
        match self {
            Self::Html => format!("lint-report-{short}.html"),
            Self::Docx => format!("lint-report-{short}.docx"),
            Self::Xlsx => format!("lint-report-{short}.xlsx"),
            Self::Checklist => format!("lint-checklist-{short}.xlsx"),
        }
    }

    pub fn content_type(self) -> &'static str {
        match self {
            Self::Html => "text/html; charset=utf-8",
            Self::Docx => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
            Self::Xlsx | Self::Checklist => {
                "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
            }
        }
    }
}

impl Service {
    /// 解析一份规范：内置规范套上用户差异，自定义规范直接读。文字按当前语言。
    pub fn lint_ruleset(&self, id: &str) -> Result<Ruleset> {
        let lang = current_lang();
        if builtin_ids().iter().any(|b| b == id) {
            let base = builtin(id, lang).expect("内置编号必然存在");
            Ok(match self.store.lint_overrides(id)? {
                Some(ov) => apply(base, &ov),
                None => base,
            })
        } else {
            self.store.lint_custom(id)?.ok_or_else(|| Error::not_found(msg!("lint.ruleset_missing", id = id)))
        }
    }

    /// 规范库列表：内置在前，自定义在后。
    pub fn lint_rulesets(&self) -> Result<Vec<RulesetSummary>> {
        let lang = current_lang();
        let modified = self.store.lint_modified_builtins()?;
        let mut out = Vec::new();
        for id in builtin_ids() {
            let base = builtin(&id, lang).expect("内置编号必然存在");
            let is_modified = modified.contains(&id);
            let set = if is_modified {
                match self.store.lint_overrides(&id)? {
                    Some(ov) => apply(base, &ov),
                    None => base,
                }
            } else {
                base
            };
            out.push(RulesetSummary {
                id: set.id,
                name: set.name,
                builtin: true,
                modified: is_modified,
                clause_count: set.clauses.len(),
            });
        }
        for row in self.store.lint_customs()? {
            let set = self.store.lint_custom(&row.id)?;
            let count = set.map(|s| s.clauses.len()).unwrap_or(0);
            out.push(RulesetSummary {
                id: row.id,
                name: row.name,
                builtin: false,
                modified: false,
                clause_count: count,
            });
        }
        Ok(out)
    }

    /// 新建自定义规范，返回它的编号。
    pub fn create_lint_ruleset(&self, mut set: Ruleset) -> Result<String> {
        set.builtin = false;
        set.id = uuid::Uuid::new_v4().simple().to_string();
        validate(&set)?;
        self.store.save_lint_custom(&set)?;
        Ok(set.id)
    }

    /// 保存改动：内置规范存差异，自定义规范存完整内容。
    pub fn update_lint_ruleset(&self, id: &str, mut set: Ruleset) -> Result<()> {
        validate(&set)?;
        let lang = current_lang();
        if builtin_ids().iter().any(|b| b == id) {
            let base = builtin(id, lang).expect("内置编号必然存在");
            let overrides = diff(&base, &set);
            if overrides.is_empty() {
                self.store.delete_lint_overrides(id)?;
            } else {
                self.store.save_lint_overrides(id, &base.name, &overrides)?;
            }
        } else {
            self.store
                .lint_custom(id)?
                .ok_or_else(|| Error::not_found(msg!("lint.ruleset_missing", id = id)))?;
            set.id = id.to_string();
            set.builtin = false;
            self.store.save_lint_custom(&set)?;
        }
        Ok(())
    }

    pub fn delete_lint_ruleset(&self, id: &str) -> Result<()> {
        if !self.store.delete_lint_custom(id)? {
            return Err(Error::not_found(msg!("lint.ruleset_missing", id = id)));
        }
        Ok(())
    }

    /// 恢复内置规范的默认值：删掉用户差异，返回默认规范。
    pub fn reset_lint_ruleset(&self, id: &str) -> Result<Ruleset> {
        if !builtin_ids().iter().any(|b| b == id) {
            return Err(Error::invalid(msg!("lint.not_builtin")));
        }
        self.store.delete_lint_overrides(id)?;
        self.lint_ruleset(id)
    }

    /// 导入 YAML，存为自定义规范，返回编号。
    pub fn import_lint_ruleset(&self, yaml: &str) -> Result<String> {
        let set: Ruleset =
            serde_saphyr::from_str(yaml).map_err(|e| Error::invalid(msg!("lint.yaml_parse", error = e)))?;
        self.create_lint_ruleset(set)
    }

    /// 导出规范为 YAML。
    pub fn export_lint_ruleset(&self, id: &str) -> Result<String> {
        let set = self.lint_ruleset(id)?;
        serde_saphyr::to_string(&set).map_err(|e| Error::internal(msg!("lint.yaml_parse", error = e)))
    }

    /// 全部规则的编号和参数定义，供界面显示说明和默认值。
    pub fn lint_rule_catalog(&self) -> Vec<RuleInfo> {
        rule_infos()
    }

    pub fn lint_runs(&self) -> Result<Vec<LintRunSummary>> {
        self.store.lint_run_summaries()
    }

    pub fn lint_run(&self, id: &str) -> Result<LintRun> {
        self.store.lint_run_record(id)
    }

    pub fn delete_lint_run(&self, id: &str) -> Result<()> {
        self.store.delete_lint_run(id)
    }

    /// 填写人工结论；只允许填在无法判断或待人工确认的条款上。
    pub fn review_lint_clause(&self, run_id: &str, clause: &str, input: ReviewInput) -> Result<LintRun> {
        let mut run = self.store.lint_run_record(run_id)?;
        let result = run
            .results
            .iter_mut()
            .find(|r| r.clause == clause)
            .ok_or_else(|| Error::not_found(msg!("lint.run_missing", id = clause)))?;
        if !is_reviewable(result.verdict) {
            return Err(Error::invalid(msg!("lint.clause_not_reviewable")));
        }
        result.review = Some(dblens_lint::Review { verdict: input.verdict, note: input.note });
        self.store.save_lint_run(&run)?;
        Ok(run)
    }

    /// 豁免一个对象：写进规范，对这次结果立即生效，以后的检查也跳过它。
    pub fn waive_lint_issue(&self, run_id: &str, clause: &str, input: WaiveInput) -> Result<LintRun> {
        let reason = input.reason.trim();
        if reason.is_empty() {
            return Err(Error::invalid(msg!("lint.exception_empty")));
        }
        let mut run = self.store.lint_run_record(run_id)?;
        if !lint_clause_mut(&mut run, clause)?.waive(&input.object, reason) {
            return Err(Error::not_found(msg!("lint.issue_missing", object = input.object)));
        }
        let mut set = self.lint_ruleset(&run.ruleset_id)?;
        set.exceptions.push(Exception {
            clause: clause.to_string(),
            object: input.object,
            reason: reason.to_string(),
        });
        self.update_lint_ruleset(&run.ruleset_id, set)?;
        self.store.save_lint_run(&run)?;
        Ok(run)
    }

    /// 撤销豁免：从规范里删掉例外，恢复这次结果里的问题。规范已删除时只恢复结果。
    pub fn unwaive_lint_issue(&self, run_id: &str, clause: &str, object: &str) -> Result<LintRun> {
        let mut run = self.store.lint_run_record(run_id)?;
        if !lint_clause_mut(&mut run, clause)?.unwaive(object) {
            return Err(Error::not_found(msg!("lint.issue_missing", object = object)));
        }
        match self.lint_ruleset(&run.ruleset_id) {
            Ok(mut set) => {
                set.exceptions.retain(|e| !(e.clause == clause && e.object == object));
                self.update_lint_ruleset(&run.ruleset_id, set)?;
            }
            Err(Error::NotFound { .. }) => {}
            Err(e) => return Err(e),
        }
        self.store.save_lint_run(&run)?;
        Ok(run)
    }

    /// 发起检查，返回任务编号。结果存进 `lint_runs`，任务状态里给出记录编号。
    pub async fn start_lint(self: &Arc<Self>, req: LintRequest) -> Result<String> {
        let lang = current_lang();
        let set = self.lint_ruleset(&req.ruleset)?;
        let enabled = set.clauses.iter().filter(|c| c.enabled).count();
        let task = self.tasks.create(TaskKind::Lint)?;
        let mut id = String::new();
        task.update(|s| {
            id = s.id.clone();
            s.phase = TaskPhase::Scanning;
            s.progress.total = enabled as u64;
        });
        let service = self.clone();
        tokio::spawn(async move {
            let run = async {
                let (input, target) = service.load_lint_input(&req.source).await?;
                if task.cancelled() {
                    return Err(Error::Cancelled);
                }
                task.update(|s| s.phase = TaskPhase::Executing);
                let family = dblens_lint::family_key(input.family).to_string();
                let results = check(&input, &set, lang, || task.update(|s| s.progress.read += 1));
                let clauses: Vec<ClauseMeta> = results
                    .iter()
                    .filter_map(|r| set.clauses.iter().find(|c| c.id == r.clause))
                    .map(|c| ClauseMeta {
                        id: c.id.clone(),
                        section: c.section,
                        text: c.text.clone(),
                        severity: c.severity,
                    })
                    .collect();
                let record = LintRun {
                    id: uuid::Uuid::new_v4().simple().to_string(),
                    created_at: chrono::Utc::now().timestamp_millis(),
                    target,
                    ruleset_id: set.id.clone(),
                    ruleset_name: set.name.clone(),
                    family,
                    clauses,
                    results,
                };
                service.store.save_lint_run(&record)?;
                task.update(|s| {
                    s.lint_run = Some(record.id.clone());
                    s.phase = TaskPhase::Finished;
                });
                Ok(())
            };
            let result = run.await;
            task.finish(result);
        });
        Ok(id)
    }

    /// 取结构，连同对象的显示名。
    async fn load_lint_input(&self, source: &LintSource) -> Result<(LintInput, String)> {
        match source {
            LintSource::Database { profile, catalog, schema } => {
                let profile = self.scoped_profile(profile, catalog.as_deref())?;
                let dialect = self.metadata.dialect(&profile).await?;
                let args = MetaArgs { schema: Some(schema.clone()), ..Default::default() };
                for key in MetaKey::TABLE_DETAIL {
                    self.metadata.cache().forget(&profile.id, key, &args);
                }
                let tables = self.metadata.tables(&profile, schema, None).await?;
                let mut views =
                    self.metadata.views(&profile, schema, MetaKey::Views).await.unwrap_or_default();
                let mviews = self
                    .metadata
                    .views(&profile, schema, MetaKey::MaterializedViews)
                    .await
                    .unwrap_or_default();
                views.extend(mviews);
                let sequences = self.metadata.sequences(&profile, schema).await.unwrap_or_default();
                let routines = self.metadata.routines(&profile, schema).await.unwrap_or_default();
                let synonyms = self.metadata.synonyms(&profile, schema).await.unwrap_or_default();
                let model = Schema {
                    name: schema.clone(),
                    tables,
                    views,
                    materialized_views: vec![],
                    sequences,
                    routines,
                    synonyms,
                };
                let input = LintInput { schema: model, family: dialect.family(), objects_known: true };
                let place = catalog.as_ref().map_or_else(|| schema.clone(), |c| format!("{c}.{schema}"));
                let target = format!("{} · {place}", profile.input.name);
                Ok((input, target))
            }
            LintSource::Model { model, schema } => {
                let st = self.models.schema_tables(model, schema)?;
                let tables = st.tables.into_iter().map(|(t, _)| t).collect();
                let family = self.models.dialect(model)?.family();
                let name = self.models.summary_of(model)?.info.name;
                let model_schema = Schema { name: schema.clone(), tables, ..Default::default() };
                let input = LintInput { schema: model_schema, family, objects_known: false };
                let target = format!("{name} · {schema}");
                Ok((input, target))
            }
        }
    }

    /// 导出报告，返回字节和下载文件名。
    pub fn lint_report(
        &self,
        run_id: &str,
        reviewer: Option<String>,
        format: ReportFormat,
    ) -> Result<(Vec<u8>, String)> {
        let run = self.store.lint_run_record(run_id)?;
        let report = Report::new(&run, reviewer.as_deref(), current_lang());
        let bytes = match format {
            ReportFormat::Html => report::html(&report)?.into_bytes(),
            ReportFormat::Docx => report_office::docx(&report)?,
            ReportFormat::Xlsx => report_office::xlsx(&report)?,
            ReportFormat::Checklist => report_office::checklist(&report)?,
        };
        let short: String = run.id.chars().take(8).collect();
        Ok((bytes, format.file_name(&short)))
    }
}

fn lint_clause_mut<'a>(run: &'a mut LintRun, clause: &str) -> Result<&'a mut ClauseResult> {
    run.results
        .iter_mut()
        .find(|r| r.clause == clause)
        .ok_or_else(|| Error::not_found(msg!("lint.run_missing", id = clause)))
}
