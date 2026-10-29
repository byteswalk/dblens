//! 结构迁移（M5a 设计文档第 3.2、6 节）：读源和目标的结构，按迁移方案转换、检查、生成脚本；
//! 作为任务逐条执行（某张表建表失败时跳过依赖它的语句），最后写迁移报告（markdown）。转换本身在 `dblens_dialect::migrate`。

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::sync::Arc;
use std::time::Instant;

use dblens_core::schema::{RoutineKind, Sequence, Table};
use dblens_core::{Error, Lang, Msg, Result, current_lang, msg};
use dblens_dialect::meta::{MetaArgs, MetaKey};
use dblens_dialect::migrate::{
    ExistingAction, MigrationDraft, MigrationInput, MigrationSettings, MigrationStatement, NoteLevel,
    StatementKind, migrate, migration_script, script_text,
};
use dblens_dialect::{DdlKind, Dialect, Variant};
use serde::{Deserialize, Serialize};

use crate::design::Run;
use crate::profile::Profile;
use crate::service::Service;
use crate::task::{TaskFailure, TaskHandle, TaskKind, TaskPhase};

/// 迁移的一端：连接和 schema。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MigrationEnd {
    pub profile: String,
    pub schema: String,
}

/// 迁移方案（M5a 设计文档 3.3 节）：可另存为 JSON 再打开。`tables`、`sequences` 为 None 时迁移源 schema 的全部表、序列；
/// `existing`：目标中已有同名表时的处理方式（按源表名）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MigrationPlan {
    pub source: MigrationEnd,
    #[serde(default)]
    pub tables: Option<Vec<String>>,
    /// 迁移的序列；None 为全部。
    #[serde(default)]
    pub sequences: Option<Vec<String>>,
    pub target: MigrationEnd,
    #[serde(default)]
    pub settings: MigrationSettings,
    #[serde(default)]
    pub existing: BTreeMap<String, ExistingAction>,
}

/// 不迁移的对象（视图、例程、触发器、同义词）。触发器的 `table` 是所属的表。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OtherObject {
    pub kind: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub table: Option<String>,
}

/// 预览：转换结果、脚本、开始前检查的问题、执行策略的结论。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MigrationPreview {
    pub source_variant: Variant,
    pub target_variant: Variant,
    /// 迁移范围内的源表（界面对照显示源类型）。
    pub source_tables: Vec<Table>,
    pub source_sequences: Vec<Sequence>,
    pub draft: MigrationDraft,
    pub others: Vec<OtherObject>,
    pub statements: Vec<MigrationStatement>,
    pub text: String,
    /// 不能开始的原因；为空才能开始。
    pub problems: Vec<Msg>,
    pub denied: Option<Msg>,
    pub confirm: Vec<Msg>,
}

/// 读出的两端。
struct Loaded {
    source: Profile,
    target: Profile,
    source_dialect: Dialect,
    target_dialect: Dialect,
    tables: Vec<Table>,
    sequences: Vec<Sequence>,
    target_tables: Vec<Table>,
    target_sequences: Vec<String>,
    others: Vec<OtherObject>,
}

/// 开始前检查（不读库的部分）：有不支持的项、同名表没有选择处理方式、没有要迁移的对象、源和目标相同。
fn problems(plan: &MigrationPlan, draft: &MigrationDraft) -> Vec<Msg> {
    let mut out = Vec::new();
    if plan.source == plan.target {
        out.push(msg!("migrate.same_target"));
    }
    if draft.tables.is_empty() && draft.sequences.is_empty() {
        out.push(msg!("migrate.nothing"));
    }
    let blocked = draft.notes.iter().filter(|n| n.level == NoteLevel::Unsupported).count();
    if blocked > 0 {
        out.push(msg!("migrate.unsupported_left", count = blocked));
    }
    let unchosen: Vec<&str> = draft
        .tables
        .iter()
        .filter(|t| t.exists && !plan.existing.contains_key(&t.source))
        .map(|t| t.source.as_str())
        .collect();
    if !unchosen.is_empty() {
        out.push(msg!("migrate.existing_unchosen", tables = unchosen.join(", ")));
    }
    out
}

impl Service {
    /// 读两端的结构。`fresh`：跳过元数据缓存（开始执行前以库中最新的结构为准）。
    async fn load_migration(&self, plan: &MigrationPlan, fresh: bool) -> Result<Loaded> {
        let source = self.profile(&plan.source.profile)?;
        let target = self.profile(&plan.target.profile)?;
        let source_dialect = self.metadata.dialect(&source).await?;
        let target_dialect = self.metadata.dialect(&target).await?;
        let schemas = self.metadata.schemas(&target, None).await?;
        if !schemas.iter().any(|s| s.name == plan.target.schema) {
            return Err(Error::not_found(msg!(
                "noun.object",
                kind = "schema",
                name = format!("{}.{}", target.input.name, plan.target.schema)
            )));
        }
        if fresh {
            for (p, schema) in [(&source, &plan.source.schema), (&target, &plan.target.schema)] {
                let args = MetaArgs { schema: Some(schema.clone()), ..Default::default() };
                for key in MetaKey::TABLE_DETAIL.into_iter().chain([MetaKey::Sequences]) {
                    self.metadata.cache().forget(&p.id, key, &args);
                }
            }
        }
        let wanted = |name: &str| plan.tables.as_ref().is_none_or(|t| t.iter().any(|x| x == name));
        let all = self.metadata.tables(&source, &plan.source.schema, None).await?;
        let tables: Vec<Table> = all.into_iter().filter(|t| wanted(&t.name)).collect();
        let sequences = if source_dialect.has_meta(MetaKey::Sequences) {
            let wanted = |name: &str| plan.sequences.as_ref().is_none_or(|t| t.iter().any(|x| x == name));
            let all = self.metadata.sequences(&source, &plan.source.schema).await?;
            all.into_iter().filter(|s| wanted(&s.name)).collect()
        } else {
            vec![]
        };
        let target_tables = self.metadata.tables(&target, &plan.target.schema, None).await?;
        let target_sequences = if target_dialect.has_meta(MetaKey::Sequences) {
            self.metadata.sequences(&target, &plan.target.schema).await?.into_iter().map(|s| s.name).collect()
        } else {
            vec![]
        };
        let others = self.other_objects(&source, &source_dialect, &plan.source.schema, &tables).await?;
        Ok(Loaded {
            source,
            target,
            source_dialect,
            target_dialect,
            tables,
            sequences,
            target_tables,
            target_sequences,
            others,
        })
    }

    /// 不迁移的对象：视图、物化视图、函数、过程、包、同义词，以及迁移的表上的触发器。
    async fn other_objects(
        &self,
        profile: &Profile,
        dialect: &Dialect,
        schema: &str,
        tables: &[Table],
    ) -> Result<Vec<OtherObject>> {
        let mut out = Vec::new();
        let obj = |kind: DdlKind, name: String| OtherObject { kind: kind.as_str().into(), name, table: None };
        for (key, kind) in
            [(MetaKey::Views, DdlKind::View), (MetaKey::MaterializedViews, DdlKind::MaterializedView)]
        {
            if dialect.has_meta(key) {
                out.extend(
                    self.metadata.views(profile, schema, key).await?.into_iter().map(|v| obj(kind, v.name)),
                );
            }
        }
        if dialect.has_meta(MetaKey::Routines) {
            for r in self.metadata.routines(profile, schema).await? {
                let kind = match r.kind {
                    RoutineKind::Procedure => DdlKind::Procedure,
                    RoutineKind::Function => DdlKind::Function,
                    RoutineKind::Package => DdlKind::Package,
                    RoutineKind::PackageBody => DdlKind::PackageBody,
                };
                out.push(obj(kind, r.name));
            }
        }
        if dialect.has_meta(MetaKey::Synonyms) {
            out.extend(
                self.metadata
                    .synonyms(profile, schema)
                    .await?
                    .into_iter()
                    .map(|s| obj(DdlKind::Synonym, s.name)),
            );
        }
        for t in tables {
            for tr in &t.triggers {
                out.push(OtherObject {
                    kind: "trigger".into(),
                    name: tr.name.clone(),
                    table: Some(t.name.clone()),
                });
            }
        }
        Ok(out)
    }

    async fn prepare_migration(
        &self,
        plan: &MigrationPlan,
        fresh: bool,
    ) -> Result<(Loaded, MigrationPreview)> {
        let l = self.load_migration(plan, fresh).await?;
        let input = MigrationInput {
            source: l.source_dialect,
            target: l.target_dialect,
            tables: &l.tables,
            sequences: &l.sequences,
            target_tables: &l.target_tables,
            target_sequences: &l.target_sequences,
        };
        let draft = migrate(&input, &plan.settings);
        let statements = migration_script(
            &l.target_dialect,
            &plan.target.schema,
            &draft,
            &plan.existing,
            &l.target_tables,
        )?;
        let sqls: Vec<String> = statements.iter().map(|s| s.sql.clone()).collect();
        let (denied, confirm) = self.check_policy(&l.target, l.target_dialect, &sqls).await;
        let preview = MigrationPreview {
            source_variant: l.source_dialect.variant(),
            target_variant: l.target_dialect.variant(),
            source_tables: l.tables.clone(),
            source_sequences: l.sequences.clone(),
            problems: problems(plan, &draft),
            text: script_text(&statements),
            draft,
            others: l.others.clone(),
            statements,
            denied,
            confirm,
        };
        Ok((l, preview))
    }

    /// 预览迁移：读结构（经元数据缓存）、转换、检查、生成脚本。不执行。
    pub async fn preview_migration(&self, plan: &MigrationPlan) -> Result<MigrationPreview> {
        Ok(self.prepare_migration(plan, false).await?.1)
    }

    /// 开始迁移：以库中最新的结构重新转换，检查不通过则拒绝；作为任务执行，返回任务 id。
    pub async fn start_migration(self: &Arc<Self>, plan: MigrationPlan) -> Result<String> {
        let (l, preview) = self.prepare_migration(&plan, true).await?;
        if let Some(p) = preview.problems.first() {
            return Err(Error::invalid(p.clone()));
        }
        if let Some(reason) = preview.denied.clone() {
            return Err(Error::Denied { reason });
        }
        let password = self.secrets.get(&l.target.id)?.unwrap_or_default();
        let task = self.tasks.create(TaskKind::Migrate)?;
        let mut id = String::new();
        task.update(|s| {
            id = s.id.clone();
            s.phase = TaskPhase::Executing;
            s.progress.statements_total = preview.statements.len() as u64;
        });
        let lang = current_lang();
        let service = self.clone();
        tokio::spawn(async move {
            let started = Instant::now();
            let run = Run {
                service: &service,
                task: &task,
                profile: &l.target,
                dialect: l.target_dialect,
                schema: &plan.target.schema,
            };
            let outcome = run_statements(&run, &preview.statements, password).await;
            service.metadata.cache().invalidate(&l.target.id, Some(&plan.target.schema));
            let report = service.migration_report(&plan, &l, &preview, &task, &outcome, started, lang).await;
            let result = match (outcome, report) {
                (Err(e), _) | (Ok(_), Err(e)) => Err(e),
                (Ok(_), Ok(())) => Ok(()),
            };
            task.finish(result);
        });
        Ok(id)
    }

    /// 写迁移报告：结果汇总、失败的语句、提示、不迁移的对象及其源库 DDL。
    #[allow(clippy::too_many_arguments)]
    async fn migration_report(
        &self,
        plan: &MigrationPlan,
        l: &Loaded,
        preview: &MigrationPreview,
        task: &TaskHandle,
        outcome: &Result<Outcome>,
        started: Instant,
        lang: Lang,
    ) -> Result<()> {
        let t = |m: Msg| m.render(lang);
        let mut md = String::new();
        let src = format!("{} / {}", l.source.input.name, plan.source.schema);
        let tgt = format!("{} / {}", l.target.input.name, plan.target.schema);
        let _ = writeln!(md, "# {}\n", t(msg!("migrate.report.title", source = &src, target = &tgt)));
        let status = task.status();
        let state = match outcome {
            Ok(_) => t(msg!("migrate.report.finished")),
            Err(Error::Cancelled) => t(msg!("migrate.report.cancelled")),
            Err(e) => t(msg!("migrate.report.failed", error = e.to_string())),
        };
        let _ = writeln!(md, "{state}\n");
        let p = &status.progress;
        let skipped = outcome.as_ref().map(|o| o.skipped).unwrap_or(0);
        let _ = writeln!(
            md,
            "{}\n",
            t(msg!(
                "migrate.report.summary",
                done = p.statements_done,
                failed = p.statements_failed,
                skipped = skipped,
                total = p.statements_total,
                seconds = started.elapsed().as_secs()
            ))
        );
        if !status.failures.is_empty() {
            let _ = writeln!(md, "## {}\n", t(msg!("migrate.report.failures")));
            for f in &status.failures {
                let _ = writeln!(md, "```sql\n{}\n```\n\n{}\n", f.text, f.error);
            }
        }
        let notes: Vec<_> = preview.draft.notes.iter().filter(|n| n.level != NoteLevel::Info).collect();
        if !notes.is_empty() {
            let _ = writeln!(md, "## {}\n", t(msg!("migrate.report.notes")));
            for n in notes {
                let at = [Some(n.object.as_str()), n.column.as_deref()]
                    .into_iter()
                    .flatten()
                    .collect::<Vec<_>>()
                    .join(".");
                let item = n.item.as_deref().map(|i| format!(" [{i}]")).unwrap_or_default();
                let _ = writeln!(md, "- {at}{item}: {}", n.message.render(lang));
            }
            md.push('\n');
        }
        if !preview.others.is_empty() {
            let _ = writeln!(md, "## {}\n", t(msg!("migrate.report.others")));
            for o in &preview.others {
                let _ = writeln!(md, "### {} {}\n", o.kind, o.name);
                let ddl = match (o.kind.as_str(), &o.table) {
                    ("trigger", Some(table)) => l
                        .tables
                        .iter()
                        .find(|x| &x.name == table)
                        .and_then(|x| x.triggers.iter().find(|tr| tr.name == o.name))
                        .and_then(|tr| tr.definition.clone()),
                    (kind, _) => match DdlKind::parse(kind) {
                        Some(k) => self.metadata.ddl(&l.source, &plan.source.schema, k, &o.name).await.ok(),
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
        let _ = writeln!(md, "## {}\n\n```sql\n{}\n```", t(msg!("migrate.report.script")), preview.text);
        let path = task.report_path("md", format!("migration-{}.md", plan.target.schema))?;
        std::fs::write(&path, md).map_err(|e| Error::internal(msg!("export.write_failed", error = e)))
    }
}

/// 执行的结果：建表失败的源表、因此跳过的语句数。
#[derive(Debug, Default)]
pub(crate) struct Outcome {
    pub failed_tables: BTreeSet<String>,
    pub skipped: u64,
}

/// 逐条执行：失败的语句记下后继续；建表（或删除重建时的删表）失败的表，依赖它的语句跳过。
pub(crate) async fn run_statements(
    run: &Run<'_>,
    statements: &[MigrationStatement],
    password: String,
) -> Result<Outcome> {
    let (mut conn, _) = run.service.registry.connect(run.profile, password).await?;
    run.task.set_cancel(Some(conn.cancel_handle()))?;
    let mut out = Outcome::default();
    let result = async {
        run.sql(&mut conn, &run.dialect.set_schema_sql(run.schema)).await?;
        for (i, s) in statements.iter().enumerate() {
            if run.task.cancelled() {
                return Err(Error::Cancelled);
            }
            if s.requires.iter().any(|r| out.failed_tables.contains(r)) {
                out.skipped += 1;
                run.task.update(|st| st.progress.statements_done += 1);
                continue;
            }
            match run.sql(&mut conn, &s.sql).await {
                Ok(_) => run.task.update(|st| st.progress.statements_done += 1),
                Err(e) => {
                    if run.task.cancelled() {
                        return Err(Error::Cancelled);
                    }
                    if matches!(s.kind, StatementKind::CreateTable | StatementKind::DropTable) {
                        out.failed_tables.insert(s.object.clone());
                    }
                    let failure =
                        TaskFailure { index: i as u64, line: i as u64 + 1, error: e, text: s.sql.clone() };
                    run.task.fail(failure, s.sql.clone());
                    run.task.update(|st| {
                        st.progress.statements_done += 1;
                        st.progress.statements_failed += 1;
                    });
                }
            }
        }
        Ok(())
    }
    .await;
    let _ = conn.close().await;
    result.map(|()| out)
}

#[cfg(test)]
mod tests {
    use dblens_dialect::migrate::{MigrationNote, ObjectKind, TableMapping};

    use super::*;
    use crate::compare::tests::service;
    use crate::model::tests::wait;
    use crate::task::TaskState;

    fn end(p: &str, s: &str) -> MigrationEnd {
        MigrationEnd { profile: p.into(), schema: s.into() }
    }

    fn plan() -> MigrationPlan {
        MigrationPlan {
            source: end("a", "s"),
            tables: None,
            sequences: None,
            target: end("b", "t"),
            settings: MigrationSettings::default(),
            existing: BTreeMap::new(),
        }
    }

    fn mapping(name: &str, exists: bool) -> TableMapping {
        TableMapping {
            source: name.into(),
            target: Table { name: name.into(), ..Default::default() },
            columns: vec![],
            exists,
        }
    }

    #[test]
    fn problems_block_the_start() {
        let mut draft = MigrationDraft { tables: vec![], sequences: vec![], notes: vec![] };
        let mut p = plan();
        p.target = p.source.clone();
        let keys = |v: Vec<Msg>| v.iter().map(|m| m.render(Lang::En)).collect::<Vec<_>>();
        assert_eq!(keys(problems(&p, &draft)).len(), 2);
        draft.tables = vec![mapping("a", true), mapping("b", true), mapping("c", false)];
        draft.notes.push(MigrationNote {
            kind: ObjectKind::Table,
            object: "c".into(),
            column: Some("x".into()),
            item: None,
            level: NoteLevel::Unsupported,
            message: Msg::raw("x"),
        });
        let mut p = plan();
        p.existing.insert("a".into(), ExistingAction::Skip);
        let got = keys(problems(&p, &draft));
        assert_eq!(got.len(), 2, "{got:?}");
        assert!(got[1].contains(": b"), "{got:?}");
        p.existing.insert("b".into(), ExistingAction::Recreate);
        draft.notes.clear();
        assert!(problems(&p, &draft).is_empty());
    }

    #[tokio::test]
    async fn failed_tables_skip_their_dependents() {
        let (s, profile, log, _dir) = service();
        let profile = s.profile(&profile).unwrap();
        let task = s.tasks.create(TaskKind::Migrate).unwrap();
        let id = task.status().id;
        let st = |kind, object: &str, requires: &[&str], sql: &str| MigrationStatement {
            phase: 1,
            kind,
            object: object.into(),
            requires: requires.iter().map(|r| r.to_string()).collect(),
            sql: sql.into(),
        };
        let statements = vec![
            st(StatementKind::CreateTable, "a", &[], "FAIL"),
            st(StatementKind::CreateTable, "b", &[], "CREATE TABLE b"),
            st(StatementKind::Comment, "a", &["a"], "COMMENT ON a"),
            st(StatementKind::Index, "b", &["b"], "FAIL"),
            st(StatementKind::ForeignKey, "b", &["b", "a"], "ALTER TABLE b ADD FK"),
            st(StatementKind::Unique, "b", &["b"], "ALTER TABLE b ADD UNIQUE"),
        ];
        let run = Run {
            service: &s,
            task: &task,
            profile: &profile,
            dialect: Dialect::new(Variant::Postgres),
            schema: "t",
        };
        let out = run_statements(&run, &statements, String::new()).await.unwrap();
        task.finish(Ok(()));
        assert_eq!(out.failed_tables, BTreeSet::from(["a".to_string()]));
        assert_eq!(out.skipped, 2);
        let status = wait(&s, &id).await;
        assert_eq!(status.state, TaskState::Done);
        assert_eq!((status.progress.statements_done, status.progress.statements_failed), (6, 2));
        assert_eq!(status.failures.iter().map(|f| f.index).collect::<Vec<_>>(), [0, 3]);
        let log = log.lock().unwrap().clone();
        assert!(
            log.iter().any(|l| l.contains("ADD UNIQUE")) && !log.iter().any(|l| l.contains("COMMENT ON a"))
        );
    }
}
