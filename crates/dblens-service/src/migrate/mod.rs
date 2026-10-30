//! 结构和数据迁移（M5a 设计文档第 3.2、6 节，M5b 设计文档）：读源和目标的结构，按迁移方案转换、检查、生成脚本；
//! 开始时建迁移记录（`record`），作为任务依次建表、迁数据、建约束、调整序列和自增、校验（`run`、`copy`），
//! 最后写迁移报告（`report`）。没有完成的迁移可以继续或放弃。结构的转换在 `dblens_dialect::migrate`。

mod copy;
mod record;
mod report;
mod run;
mod values;

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};

use dblens_core::schema::{RoutineKind, Sequence, Table};
use dblens_core::{Error, Msg, Result, current_lang, msg, scope_lang};
use dblens_dialect::meta::{self, MetaArgs, MetaKey};
use dblens_dialect::migrate::{
    ExistingAction, MigrationDraft, MigrationInput, MigrationSettings, MigrationStatement, NoteLevel,
    migrate, migration_script, script_text,
};
use dblens_dialect::{DdlKind, Dialect, Variant};
use serde::{Deserialize, Serialize};

use crate::import::OnError;
use crate::profile::Profile;
use crate::service::Service;
use crate::task::{TaskKind, TaskPhase};
pub use record::{MigrationPhase, MigrationRecords, MigrationSummary, Mismatch, TableState, Verify};
use record::{MigrationRecord, TableCopy, TableProgress, now_ms};
use run::Ctx;
pub use run::{MigrationProgress, TableView};

/// 迁移的一端：连接和 schema。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MigrationEnd {
    pub profile: String,
    /// 所在的库（SQL Server，M6b 设计文档第 6 节）；其他库不填。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub catalog: Option<String>,
    pub schema: String,
}

/// 迁移的模式（M5b 设计文档 3.1 节）：结构和数据、只迁结构、只迁数据（目标表必须已存在）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MigrationMode {
    #[default]
    All,
    Structure,
    Data,
}

/// 迁数据的选项：每批行数（100–100000）、并行表数（1–4）、出错时停止或跳过、每张表抽样比对的行数（0 为不抽样，至多 10000）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DataOptions {
    #[serde(default = "default_batch")]
    pub batch_size: usize,
    #[serde(default = "default_parallel")]
    pub parallel: usize,
    #[serde(default)]
    pub on_error: OnError,
    #[serde(default)]
    pub sample: u64,
}

fn default_batch() -> usize {
    1000
}

fn default_parallel() -> usize {
    2
}

impl Default for DataOptions {
    fn default() -> Self {
        Self { batch_size: default_batch(), parallel: default_parallel(), on_error: OnError::Stop, sample: 0 }
    }
}

/// 迁移方案（M5a 设计文档 3.3 节）：可另存为 JSON 再打开。`tables`、`sequences` 为 None 时迁移源 schema 的全部表、序列；
/// `existing`：目标中已有同名表时的处理方式（按源表名）。`mode`、`data` 见 M5b 设计文档。
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
    pub mode: MigrationMode,
    #[serde(default)]
    pub settings: MigrationSettings,
    #[serde(default)]
    pub data: DataOptions,
    #[serde(default)]
    pub existing: BTreeMap<String, ExistingAction>,
}

/// 不迁移的对象（视图、例程、触发器、同义词）。触发器的 `table` 是所属的表，`definition` 是它的定义。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OtherObject {
    pub kind: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub table: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub definition: Option<String>,
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
    if plan.mode == MigrationMode::Data {
        let missing: Vec<&str> =
            draft.tables.iter().filter(|t| !t.exists).map(|t| t.source.as_str()).collect();
        if !missing.is_empty() {
            out.push(msg!("migrate.data_needs_table", tables = missing.join(", ")));
        }
    }
    let d = &plan.data;
    if !(100..=100_000).contains(&d.batch_size) || !(1..=4).contains(&d.parallel) || d.sample > 10_000 {
        out.push(msg!("migrate.bad_data_options"));
    }
    out
}

impl Service {
    /// 读两端的结构。`fresh`：跳过元数据缓存（开始执行前以库中最新的结构为准）。
    async fn load_migration(&self, plan: &MigrationPlan, fresh: bool) -> Result<Loaded> {
        let source = self.scoped_profile(&plan.source.profile, plan.source.catalog.as_deref())?;
        let target = self.scoped_profile(&plan.target.profile, plan.target.catalog.as_deref())?;
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
        let obj = |kind: DdlKind, name: String| OtherObject {
            kind: kind.as_str().into(),
            name,
            table: None,
            definition: None,
        };
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
                    definition: tr.definition.clone(),
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
            text: script_text(l.target_dialect.family(), &statements),
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

    /// 开始迁移：以库中最新的结构重新转换，检查不通过则拒绝；建迁移记录，作为任务执行，返回任务 id。
    pub async fn start_migration(self: &Arc<Self>, plan: MigrationPlan) -> Result<String> {
        let (l, preview) = self.prepare_migration(&plan, true).await?;
        if let Some(p) = preview.problems.first() {
            return Err(Error::invalid(p.clone()));
        }
        if let Some(reason) = preview.denied.clone() {
            return Err(Error::Denied { reason });
        }
        let record = self.new_record(plan, &l, preview).await?;
        self.migrations.save(&record)?;
        self.launch(record).await
    }

    /// 由预览建迁移记录：每张要迁数据的表的搬运参数和行数估计。
    async fn new_record(
        &self,
        plan: MigrationPlan,
        l: &Loaded,
        preview: MigrationPreview,
    ) -> Result<MigrationRecord> {
        let lang = current_lang();
        let mut copies = Vec::new();
        let mut tables = BTreeMap::new();
        for t in &preview.draft.tables {
            let action =
                if t.exists { plan.existing.get(&t.source).copied() } else { Some(ExistingAction::Recreate) };
            if plan.mode == MigrationMode::Structure || !action.is_some_and(ExistingAction::copies_data) {
                continue;
            }
            let Some(source) = l.tables.iter().find(|x| x.name == t.source) else { continue };
            let ov = plan.settings.tables.get(&t.source);
            let mut source_columns = Vec::new();
            let mut target_columns = Vec::new();
            for m in &t.columns {
                let Some(col) =
                    m.target.as_ref().and_then(|n| t.target.columns.iter().find(|c| &c.name == n))
                else {
                    continue;
                };
                if col.generated.is_some() {
                    continue;
                }
                source_columns.push(m.source.clone());
                target_columns.push(col.clone());
            }
            let pk = source.primary_key.as_ref().map(|k| k.columns.clone()).unwrap_or_default();
            let key: Option<Vec<usize>> =
                pk.iter().map(|k| source_columns.iter().position(|c| c == k)).collect();
            let structure_only = ov.is_some_and(|o| o.structure_only);
            let estimate = if structure_only {
                None
            } else {
                self.row_estimate(&l.source, &plan.source.schema, &t.source).await
            };
            tables.insert(
                t.source.clone(),
                TableProgress::new(
                    if structure_only { TableState::Skipped } else { TableState::Pending },
                    estimate,
                ),
            );
            copies.push(TableCopy {
                source: t.source.clone(),
                target: t.target.name.clone(),
                identity: target_columns.iter().find(|c| c.auto_increment).map(|c| c.name.clone()),
                source_columns,
                target_columns,
                key: key.unwrap_or_default(),
                filter: ov.and_then(|o| o.filter.clone()).filter(|f| !f.trim().is_empty()),
                replace: action == Some(ExistingAction::Replace),
                append: action == Some(ExistingAction::Append),
            });
        }
        let sequences = if plan.mode == MigrationMode::Structure {
            vec![]
        } else {
            preview
                .draft
                .sequences
                .iter()
                .filter(|s| !s.exists)
                .map(|s| (s.source.clone(), s.target.clone()))
                .collect()
        };
        let notes = preview
            .draft
            .notes
            .iter()
            .filter(|n| n.level != NoteLevel::Info)
            .map(|n| {
                let at = [Some(n.object.as_str()), n.column.as_deref()]
                    .into_iter()
                    .flatten()
                    .collect::<Vec<_>>()
                    .join(".");
                let item = n.item.as_deref().map(|i| format!(" [{i}]")).unwrap_or_default();
                format!("{at}{item}: {}", n.message.render(lang))
            })
            .collect();
        let now = now_ms();
        Ok(MigrationRecord {
            id: uuid::Uuid::new_v4().simple().to_string(),
            created_at: now,
            updated_at: now,
            source_name: format!("{} / {}", l.source.input.name, plan.source.schema),
            target_name: format!("{} / {}", l.target.input.name, plan.target.schema),
            plan,
            phase: MigrationPhase::Structure,
            statements: preview.statements,
            done: BTreeSet::new(),
            statement_errors: BTreeMap::new(),
            failed_tables: BTreeSet::new(),
            copies,
            tables,
            sequences,
            skipped_rows: vec![],
            adjustments: vec![],
            notes,
            others: preview.others,
            finished: false,
        })
    }

    /// 元数据中的行数估计（读不出时为 None）。
    async fn row_estimate(&self, profile: &Profile, schema: &str, table: &str) -> Option<u64> {
        let args = MetaArgs { schema: Some(schema.into()), table: Some(table.into()), ..Default::default() };
        let rows = self.metadata.rows(profile, MetaKey::TableStorage, &args).await.ok()??;
        meta::table_storage(&rows).ok()?.row_count.and_then(|n| u64::try_from(n).ok())
    }

    /// 继续一个没有完成的迁移：有失败的表时回到迁数据这一步。
    pub async fn resume_migration(self: &Arc<Self>, id: &str) -> Result<String> {
        let mut record = self.migrations.load(id)?;
        if record.finished {
            return Err(Error::invalid(msg!("migrate.already_finished")));
        }
        if record.tables.values().any(|t| t.state == TableState::Failed) {
            record.phase = record.phase.min(MigrationPhase::Data);
        }
        self.launch(record).await
    }

    /// 没有完成的迁移。
    pub fn unfinished_migrations(&self) -> Result<Vec<MigrationSummary>> {
        self.migrations.unfinished()
    }

    /// 放弃一个迁移：删除记录，不动目标库。
    pub fn discard_migration(&self, id: &str) -> Result<()> {
        self.migrations.delete(id)
    }

    /// 作为任务执行迁移记录，返回任务 id。结束后刷新目标的元数据缓存、保存记录、写迁移报告。
    async fn launch(self: &Arc<Self>, record: MigrationRecord) -> Result<String> {
        let source =
            self.scoped_profile(&record.plan.source.profile, record.plan.source.catalog.as_deref())?;
        let target =
            self.scoped_profile(&record.plan.target.profile, record.plan.target.catalog.as_deref())?;
        let source_dialect = self.metadata.dialect(&source).await?;
        let target_dialect = self.metadata.dialect(&target).await?;
        let source_password = self.secrets.get(&source.id)?.unwrap_or_default();
        let target_password = self.secrets.get(&target.id)?.unwrap_or_default();
        if !self.migrations.begin(&record.id) {
            return Err(Error::invalid(msg!("migrate.running")));
        }
        let task = match self.tasks.create(TaskKind::Migrate) {
            Ok(t) => t,
            Err(e) => {
                self.migrations.end(&record.id);
                return Err(e);
            }
        };
        let mut id = String::new();
        let total =
            record.statements.iter().enumerate().filter(|(i, _)| !record.done.contains(i)).count() as u64;
        task.update(|s| {
            id = s.id.clone();
            s.phase = TaskPhase::Executing;
            s.progress.statements_total = total;
            s.migration = Some(run::progress(&record));
        });
        let lang = current_lang();
        let record_id = record.id.clone();
        let ctx = Ctx {
            service: self.clone(),
            task: task.clone(),
            record: Arc::new(Mutex::new(record)),
            source,
            target,
            source_dialect,
            target_dialect,
            source_password,
            target_password,
        };
        // 错误文字、报告都用开始迁移时的界面语言
        tokio::spawn(scope_lang(lang, async move {
            let outcome = run::run(&ctx).await;
            let _ = ctx.save();
            let target_schema = ctx.target_schema().unwrap_or_default();
            ctx.service.metadata.cache().invalidate(&ctx.target.id, Some(&target_schema));
            let mut result = outcome;
            if let Ok(r) = ctx.snapshot() {
                let md = ctx.service.migration_report(&r, &ctx.source, &result, lang).await;
                let written = task.report_path("md", format!("migration-{target_schema}.md")).and_then(|p| {
                    std::fs::write(&p, md)
                        .map_err(|e| Error::internal(msg!("export.write_failed", error = e)))
                });
                if result.is_ok() {
                    result = written;
                }
            }
            ctx.service.migrations.end(&record_id);
            task.finish(result);
        }));
        Ok(id)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use dblens_core::Lang;
    use dblens_dialect::migrate::{MigrationNote, ObjectKind, TableMapping};

    use super::*;

    fn end(p: &str, s: &str) -> MigrationEnd {
        MigrationEnd { profile: p.into(), catalog: None, schema: s.into() }
    }

    pub(crate) fn plan() -> MigrationPlan {
        MigrationPlan {
            source: end("a", "s"),
            tables: None,
            sequences: None,
            target: end("b", "t"),
            mode: MigrationMode::All,
            settings: MigrationSettings::default(),
            data: DataOptions::default(),
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
        // 只迁数据：目标中没有的表不能迁
        p.mode = MigrationMode::Data;
        let got = keys(problems(&p, &draft));
        assert!(got.len() == 1 && got[0].contains(": c"), "{got:?}");
        // 数据选项超出范围
        p.mode = MigrationMode::All;
        p.data.parallel = 9;
        assert_eq!(problems(&p, &draft).len(), 1);
    }
}
