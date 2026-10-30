//! 结构比对与同步（M4a 设计文档第 5、6 节）：读取两边（模型或库的一个 schema）、比对、
//! 生成同步脚本并作为任务执行，或把变更写回模型。比对本身在 `dblens_dialect::compare`。

use std::collections::{BTreeMap, BTreeSet};
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use dblens_core::schema::Table;
use dblens_core::{Error, Msg, Result, msg};
use dblens_dialect::Dialect;
use dblens_dialect::compare::{
    CompareOptions, CompareRenames, CompareSide, SchemaChange, SchemaDiff, SyncScript, SyncStep,
    apply_changes, compare_schemas, sync_rebuild_tables, sync_script,
};
use dblens_dialect::design::{RebuildContext, TableChange};
use dblens_dialect::meta::{MetaArgs, MetaKey};
use serde::{Deserialize, Serialize};

use crate::design::{Part, Run};
use crate::model::{FileError, ModelWrite, SchemaRenames};
use crate::profile::Profile;
use crate::service::Service;
use crate::task::{TaskKind, TaskPhase};

/// 比对的一边：模型的一个 schema，或连接的一个 schema。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CompareSideRef {
    Model { model: String, schema: String },
    Database { profile: String, schema: String },
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct CompareRequest {
    pub source: CompareSideRef,
    pub target: CompareSideRef,
    #[serde(default)]
    pub options: CompareOptions,
    /// 用户确认采用的推测改名（模型中记下的改名自动采用，不必在这里给出）。
    #[serde(default)]
    pub renames: CompareRenames,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CompareResult {
    pub diff: SchemaDiff,
    /// 实际采用的改名（模型记下的加上请求中的）。
    pub renames: CompareRenames,
    /// 两边文件有错、没有参与比对的表。
    pub source_errors: Vec<FileError>,
    pub target_errors: Vec<FileError>,
    /// 两边的表（界面中对照显示）。
    pub source_tables: Vec<Table>,
    pub target_tables: Vec<Table>,
    /// 比对结果的指纹：同步前重新比对，结果不同则拒绝。
    pub fingerprint: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct SyncRequest {
    pub compare: CompareRequest,
    pub selected: BTreeSet<u32>,
    /// 预览时比对结果的指纹。
    #[serde(default)]
    pub fingerprint: Option<String>,
}

/// 写回模型时要改的一个表文件。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ModelWriteInfo {
    /// 写入后的表名（删除时为被删的表）。
    pub name: String,
    /// 改名前的名字。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
    pub delete: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SyncPreview {
    /// 目标是库：同步脚本；目标是模型：None。
    pub script: Option<SyncScript>,
    /// 脚本全文（复制、在查询标签中打开、另存为）。
    pub text: String,
    /// 目标是模型：要写的文件。
    pub writes: Vec<ModelWriteInfo>,
    pub denied: Option<Msg>,
    pub confirm: Vec<Msg>,
    pub fingerprint: String,
}

/// 同步的结果：目标是库时为任务 id；目标是模型时已经写完。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SyncStarted {
    pub task: Option<String>,
}

/// 读出的一边。
struct Side {
    dialect: Dialect,
    schema: String,
    tables: Vec<Table>,
    fingerprints: BTreeMap<String, String>,
    errors: Vec<FileError>,
    /// 模型中记下的改名。
    renames: Option<SchemaRenames>,
    profile: Option<Profile>,
}

/// 比对的全部结果（预览和执行共用）。
struct Compared {
    target: Side,
    result: CompareResult,
}

/// 已应用的改名：（表名，列名）；列名为 None 时是表的改名。
type Cleared = Vec<(String, Option<String>)>;

fn hash_of<T: Serialize>(v: &T) -> String {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    serde_json::to_string(v).unwrap_or_default().hash(&mut h);
    format!("{:016x}", h.finish())
}

/// 合并改名：模型是源时记下的改名（新名 → 原名）就是源 → 目标；模型是目标时反过来。
/// 只保留两边都有的表和列（记录过时的不报错）。
fn effective_renames(source: &Side, target: &Side, requested: &CompareRenames) -> CompareRenames {
    let mut out = CompareRenames::default();
    if let Some(r) = &source.renames {
        out.tables.extend(r.tables.clone());
        out.columns.extend(r.columns.clone());
    }
    if let Some(r) = &target.renames {
        let inverse = |m: &BTreeMap<String, String>| -> BTreeMap<String, String> {
            m.iter().map(|(k, v)| (v.clone(), k.clone())).collect()
        };
        out.tables.extend(inverse(&r.tables));
        for (t, cols) in &r.columns {
            let source_table = r.tables.get(t).cloned().unwrap_or_else(|| t.clone());
            out.columns.entry(source_table).or_default().extend(inverse(cols));
        }
    }
    out.tables.extend(requested.tables.clone());
    for (t, cols) in &requested.columns {
        out.columns.entry(t.clone()).or_default().extend(cols.clone());
    }
    let find = |tables: &[Table], name: &str| tables.iter().find(|t| t.name == name).cloned();
    out.tables.retain(|s, t| find(&source.tables, s).is_some() && find(&target.tables, t).is_some());
    let renames_tables = out.tables.clone();
    out.columns.retain(|s, cols| {
        let target_name = renames_tables.get(s).cloned().unwrap_or_else(|| s.clone());
        let (Some(st), Some(tt)) = (find(&source.tables, s), find(&target.tables, &target_name)) else {
            return false;
        };
        cols.retain(|a, b| {
            st.columns.iter().any(|c| &c.name == a)
                && tt.columns.iter().any(|c| &c.name == b)
                && !st.columns.iter().any(|c| &c.name == b)
        });
        !cols.is_empty()
    });
    out
}

/// 同步脚本全文：每条语句以 `;` 结尾，各段之间空一行。
fn script_text(script: &SyncScript) -> String {
    let parts: Vec<String> = script
        .steps
        .iter()
        .map(|s| part_of(s).texts().into_iter().map(|t| format!("{t};")).collect::<Vec<_>>().join("\n"))
        .collect();
    parts.join("\n\n")
}

fn part_of(step: &SyncStep) -> Part {
    match step {
        SyncStep::Sql { sql, .. } => Part::Sql(sql.clone()),
        SyncStep::Rebuild { rebuild, .. } => Part::Rebuild(rebuild.clone()),
    }
}

/// 写回模型：勾选的变更换算成表文件的写入、删除；同时得出已应用的改名（从模型的改名记录中删去）。
fn model_writes(
    diff: &SchemaDiff,
    selected: &BTreeSet<u32>,
    target: &[Table],
) -> Result<(Vec<ModelWrite>, Cleared)> {
    let mut writes = Vec::new();
    let mut cleared = Vec::new();
    for t in &diff.tables {
        let picked: Vec<&SchemaChange> =
            t.items.iter().filter(|i| selected.contains(&i.id)).map(|i| &i.change).collect();
        if picked.is_empty() {
            continue;
        }
        let mut changes = Vec::new();
        let mut rename = None;
        for c in picked {
            match c {
                SchemaChange::DropTable { name } => {
                    writes.push(ModelWrite::Delete { name: name.clone() });
                }
                SchemaChange::RenameTable { from, to } => {
                    rename = Some(to.clone());
                    cleared.push((from.clone(), None));
                }
                SchemaChange::Table { change } => match change.as_ref() {
                    TableChange::CreateTable { table } => {
                        writes.push(ModelWrite::Put { table: Box::new(table.clone()), path_of: None });
                    }
                    other => {
                        if let TableChange::RenameColumn { from, .. } = other {
                            cleared.push((t.target.clone().unwrap_or_default(), Some(from.clone())));
                        }
                        changes.push(other.clone());
                    }
                },
            }
        }
        let Some(target_name) = &t.target else { continue };
        if changes.is_empty() && rename.is_none() {
            continue;
        }
        let original = target.iter().find(|x| &x.name == target_name).cloned().unwrap_or_default();
        let mut table = apply_changes(&original, &changes)?;
        if let Some(to) = rename {
            table.name = to;
        }
        writes.push(ModelWrite::Put { table: Box::new(table), path_of: Some(target_name.clone()) });
    }
    Ok((writes, cleared))
}

/// 同步到库后，源模型中已应用的改名：表的改名按源的名字（新名），列的改名按源的表名和列名。
fn applied_renames(diff: &SchemaDiff, selected: &BTreeSet<u32>) -> Cleared {
    let mut out = Vec::new();
    for t in &diff.tables {
        for i in t.items.iter().filter(|i| selected.contains(&i.id)) {
            match &i.change {
                SchemaChange::RenameTable { to, .. } => out.push((to.clone(), None)),
                SchemaChange::Table { change } => {
                    if let TableChange::RenameColumn { to, .. } = change.as_ref() {
                        out.push((t.source.clone().unwrap_or_default(), Some(to.clone())));
                    }
                }
                SchemaChange::DropTable { .. } => {}
            }
        }
    }
    out
}

impl Service {
    async fn load_side(&self, side: &CompareSideRef) -> Result<Side> {
        match side {
            CompareSideRef::Model { model, schema } => {
                let s = self.models.schema_tables(model, schema)?;
                Ok(Side {
                    dialect: self.models.dialect(model)?,
                    schema: schema.clone(),
                    fingerprints: s.tables.iter().map(|(t, fp)| (t.name.clone(), fp.clone())).collect(),
                    tables: s.tables.into_iter().map(|(t, _)| t).collect(),
                    errors: s.errors,
                    renames: Some(self.models.renames(model, schema)?),
                    profile: None,
                })
            }
            CompareSideRef::Database { profile, schema } => {
                let profile = self.feature_profile(profile)?;
                let dialect = self.metadata.dialect(&profile).await?;
                // 跳过元数据缓存，以库中最新的结构为准
                let args = MetaArgs { schema: Some(schema.clone()), ..Default::default() };
                for key in MetaKey::TABLE_DETAIL {
                    self.metadata.cache().forget(&profile.id, key, &args);
                }
                let tables = self.metadata.tables(&profile, schema, None).await?;
                Ok(Side {
                    dialect,
                    schema: schema.clone(),
                    tables: tables.into_iter().map(|t| Table { triggers: vec![], ..t }).collect(),
                    fingerprints: BTreeMap::new(),
                    errors: vec![],
                    renames: None,
                    profile: Some(profile),
                })
            }
        }
    }

    async fn run_compare(&self, req: &CompareRequest) -> Result<Compared> {
        let source = self.load_side(&req.source).await?;
        let target = self.load_side(&req.target).await?;
        if source.dialect.family() != target.dialect.family() {
            return Err(Error::invalid(msg!(
                "compare.family_mismatch",
                source = format!("{:?}", source.dialect.variant()),
                target = format!("{:?}", target.dialect.variant())
            )));
        }
        let renames = effective_renames(&source, &target, &req.renames);
        // 生成的语句在目标上执行，按目标的方言比对
        let diff = compare_schemas(
            &target.dialect,
            &CompareSide { schema: &source.schema, tables: &source.tables },
            &CompareSide { schema: &target.schema, tables: &target.tables },
            &renames,
            &req.options,
        )?;
        let result = CompareResult {
            fingerprint: hash_of(&(&diff, &target.fingerprints)),
            diff,
            renames,
            source_errors: source.errors.clone(),
            target_errors: target.errors.clone(),
            source_tables: source.tables.clone(),
            target_tables: target.tables.clone(),
        };
        Ok(Compared { target, result })
    }

    pub async fn compare(&self, req: &CompareRequest) -> Result<CompareResult> {
        Ok(self.run_compare(req).await?.result)
    }

    /// 目标是库时的同步脚本：要重建的表先读取依赖信息。
    async fn sync_script_of(&self, c: &Compared, selected: &BTreeSet<u32>) -> Result<SyncScript> {
        let (d, profile) = (&c.target.dialect, c.target.profile.as_ref().expect("database target"));
        let mut contexts = BTreeMap::<String, RebuildContext>::new();
        for name in sync_rebuild_tables(d, &c.result.diff, selected) {
            let target_name = c
                .result
                .diff
                .tables
                .iter()
                .find(|t| t.source.as_deref() == Some(&name))
                .and_then(|t| t.target.clone())
                .unwrap_or_else(|| name.clone());
            let t = self.fresh_table(profile, &c.target.schema, &target_name).await?;
            contexts.insert(name, self.rebuild_context(profile, d, &c.target.schema, &t).await?);
        }
        sync_script(d, &c.target.schema, &c.result.diff, selected, &c.target.tables, &contexts)
    }

    async fn prepare_sync(&self, req: &SyncRequest) -> Result<(Compared, SyncPreview)> {
        let c = self.run_compare(&req.compare).await?;
        let mut preview = SyncPreview {
            script: None,
            text: String::new(),
            writes: vec![],
            denied: None,
            confirm: vec![],
            fingerprint: c.result.fingerprint.clone(),
        };
        match &c.target.profile {
            Some(profile) => {
                let script = self.sync_script_of(&c, &req.selected).await?;
                preview.text = script_text(&script);
                let texts: Vec<String> = script.steps.iter().flat_map(|s| part_of(s).texts()).collect();
                (preview.denied, preview.confirm) =
                    self.check_policy(profile, c.target.dialect, &texts).await;
                preview.script = Some(script);
            }
            None => {
                let (writes, _) = model_writes(&c.result.diff, &req.selected, &c.target.tables)?;
                preview.writes = writes
                    .iter()
                    .map(|w| match w {
                        ModelWrite::Put { table, path_of } => ModelWriteInfo {
                            name: table.name.clone(),
                            from: path_of.clone().filter(|p| p != &table.name),
                            delete: false,
                        },
                        ModelWrite::Delete { name } => {
                            ModelWriteInfo { name: name.clone(), from: None, delete: true }
                        }
                    })
                    .collect();
            }
        }
        Ok((c, preview))
    }

    pub async fn preview_sync(&self, req: &SyncRequest) -> Result<SyncPreview> {
        Ok(self.prepare_sync(req).await?.1)
    }

    /// 同步：重新比对，结果与预览时不同则拒绝。目标是库时作为任务执行（结束后刷新缓存、删去源模型中已应用的改名）；
    /// 目标是模型时直接写回。
    pub async fn start_sync(self: &Arc<Self>, req: SyncRequest) -> Result<SyncStarted> {
        if req.selected.is_empty() {
            return Err(Error::invalid(msg!("design.no_changes")));
        }
        let (c, preview) = self.prepare_sync(&req).await?;
        if req.fingerprint.as_deref().is_some_and(|fp| fp != preview.fingerprint) {
            return Err(Error::invalid(msg!("compare.changed")));
        }
        let CompareSideRef::Database { .. } = &req.compare.target else {
            let CompareSideRef::Model { model, schema } = &req.compare.target else { unreachable!() };
            let (writes, cleared) = model_writes(&c.result.diff, &req.selected, &c.target.tables)?;
            self.models.write_back(model, schema, &c.target.fingerprints, writes, &cleared)?;
            return Ok(SyncStarted { task: None });
        };
        let script = preview.script.expect("database target");
        if let Some((table, reason)) = script.refused.first() {
            return Err(Error::invalid(msg!("compare.rebuild_refused", table = table, reason = reason)));
        }
        if let Some(reason) = preview.denied {
            return Err(Error::Denied { reason });
        }
        let profile = c.target.profile.clone().expect("database target");
        let password = self.secrets.get(&profile.id)?.unwrap_or_default();
        let parts: Vec<Part> = script.steps.iter().map(part_of).collect();
        let task = self.tasks.create(TaskKind::Sync)?;
        let mut id = String::new();
        task.update(|s| {
            id = s.id.clone();
            s.phase = TaskPhase::Executing;
            s.progress.statements_total = Part::len(&parts);
        });
        let cleared = match &req.compare.source {
            CompareSideRef::Model { model, schema } => {
                Some((model.clone(), schema.clone(), applied_renames(&c.result.diff, &req.selected)))
            }
            CompareSideRef::Database { .. } => None,
        };
        let service = self.clone();
        let (dialect, schema) = (c.target.dialect, c.target.schema.clone());
        tokio::spawn(async move {
            let run = Run { service: &service, task: &task, profile: &profile, dialect, schema: &schema };
            let mut result = run.execute(parts, password).await;
            service.metadata.cache().invalidate(&profile.id, Some(&schema));
            if let (Ok(()), Some((model, mschema, list))) = (&result, cleared) {
                result = service.models.clear_renames(&model, &mschema, &list);
            }
            task.finish(result);
        });
        Ok(SyncStarted { task: Some(id) })
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::time::Duration;

    use dblens_core::schema::{Column, PrimaryKey};
    use dblens_core::{ColumnType, LogicalType};
    use dblens_dialect::compare::TableStatus;
    use dblens_dialect::design::{Rebuild, RebuildStep};
    use dblens_dialect::{Compat, Variant};

    use super::*;
    use crate::fake::{FakeDriver, Log};
    use crate::history::HistoryLog;
    use crate::model::ModelDatabase;
    use crate::policy::DefaultPolicy;
    use crate::profile::tests::input;
    use crate::registry::DriverRegistry;
    use crate::secret::{SecretBackend, Secrets};
    use crate::store::Store;
    use crate::task::{TaskState, TaskStatus};

    pub(crate) fn service() -> (Arc<Service>, String, Log, tempfile::TempDir) {
        let store = Arc::new(Store::open_in_memory().unwrap());
        let secrets = Arc::new(Secrets::with_backend(store.clone(), SecretBackend::Vault));
        secrets.init_vault("master").unwrap();
        let log: Log = Arc::default();
        let registry = Arc::new(DriverRegistry::with_drivers(
            store.clone(),
            vec![Box::new(FakeDriver { log: log.clone() })],
        ));
        let s = Service::with_history(
            store.clone(),
            secrets,
            registry,
            Arc::new(DefaultPolicy),
            HistoryLog::inline(store),
        );
        let dir = tempfile::tempdir().unwrap();
        s.tasks.set_dirs(dir.path().join("tasks"), dir.path().join("imports"));
        let p = s.create_profile(input("dev"), Some("pw")).unwrap();
        (Arc::new(s), p.id, log, dir)
    }

    async fn wait(s: &Service, id: &str) -> TaskStatus {
        for _ in 0..1000 {
            let st = s.tasks.status(id).unwrap();
            if st.state != TaskState::Running {
                return st;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("任务没有结束");
    }

    fn table(name: &str, cols: &[&str]) -> Table {
        Table {
            name: name.into(),
            columns: cols
                .iter()
                .enumerate()
                .map(|(i, c)| Column {
                    name: (*c).into(),
                    ordinal: i as u32 + 1,
                    data_type: ColumnType {
                        native: "integer".into(),
                        logical: LogicalType::Int { bits: 32 },
                    },
                    nullable: i > 0,
                    default: None,
                    comment: None,
                    auto_increment: false,
                    generated: None,
                })
                .collect(),
            primary_key: Some(PrimaryKey { name: None, columns: vec![cols[0].into()] }),
            ..Default::default()
        }
    }

    fn model(s: &Service, dir: &std::path::Path, name: &str, tables: Vec<Table>) -> String {
        let db = ModelDatabase { variant: Variant::Postgres, compat: Compat::Native, version: None };
        s.models.create_from(&dir.join(name), name, db, &[("public".into(), tables)]).unwrap().id
    }

    fn side(model: &str) -> CompareSideRef {
        CompareSideRef::Model { model: model.into(), schema: "public".into() }
    }

    fn all(diff: &SchemaDiff) -> BTreeSet<u32> {
        diff.tables.iter().flat_map(|t| t.items.iter().map(|i| i.id)).collect()
    }

    /// 源模型记下了改名：按改名配对；写回目标模型后两边没有差异。
    #[tokio::test]
    async fn model_to_model_uses_recorded_renames_and_writes_back() {
        let (s, _, _, dir) = service();
        let base = vec![table("customers", &["id", "name"]), table("old_items", &["id"])];
        let a = model(&s, dir.path(), "a", base.clone());
        let b = model(&s, dir.path(), "b", base);
        s.models.rename_table(&a, "public", "customers", "clients").unwrap();
        s.models
            .save_table(&a, "public", None, table("orders", &["id", "client_id"]), &BTreeMap::new(), None)
            .unwrap();
        s.models.delete_table(&a, "public", "old_items", None).unwrap();
        let req = CompareRequest {
            source: side(&a),
            target: side(&b),
            options: CompareOptions::default(),
            renames: CompareRenames::default(),
        };
        let r = s.compare(&req).await.unwrap();
        assert_eq!(r.renames.tables, BTreeMap::from([("clients".into(), "customers".into())]));
        let status: Vec<_> = r.diff.tables.iter().map(|t| (t.source.clone(), t.status)).collect();
        assert!(status.contains(&(Some("clients".into()), TableStatus::Renamed)), "{status:?}");
        assert!(status.contains(&(Some("orders".into()), TableStatus::Added)), "{status:?}");
        assert!(status.contains(&(None, TableStatus::Removed)), "{status:?}");
        let sync =
            SyncRequest { compare: req.clone(), selected: all(&r.diff), fingerprint: Some(r.fingerprint) };
        let p = s.preview_sync(&sync).await.unwrap();
        assert!(p.script.is_none());
        assert!(p.writes.contains(&ModelWriteInfo {
            name: "clients".into(),
            from: Some("customers".into()),
            delete: false
        }));
        assert!(p.writes.contains(&ModelWriteInfo { name: "old_items".into(), from: None, delete: true }));
        assert_eq!(s.start_sync(sync).await.unwrap(), SyncStarted { task: None });
        let again = s.compare(&req).await.unwrap();
        assert_eq!(again.diff.changed(), 0, "{:?}", again.diff);
    }

    /// 预览之后目标被改了：拒绝同步。
    #[tokio::test]
    async fn sync_refuses_when_target_changed() {
        let (s, _, _, dir) = service();
        let a = model(&s, dir.path(), "a", vec![table("t", &["id", "x"])]);
        let b = model(&s, dir.path(), "b", vec![table("t", &["id"])]);
        let req = CompareRequest {
            source: side(&a),
            target: side(&b),
            options: CompareOptions::default(),
            renames: CompareRenames::default(),
        };
        let r = s.compare(&req).await.unwrap();
        let sync = SyncRequest { compare: req, selected: all(&r.diff), fingerprint: Some(r.fingerprint) };
        s.models
            .save_table(&b, "public", Some("t"), table("t", &["id", "y"]), &BTreeMap::new(), None)
            .unwrap();
        let err = s.start_sync(sync).await.unwrap_err();
        assert!(err.to_string().contains("changed"), "{err}");
    }

    /// 执行器依次执行多段：MySQL（不在事务中）在第二段的重建删除旧表之前失败，没有执行的只列出后面的段。
    #[tokio::test]
    async fn executor_runs_parts_and_lists_remaining() {
        let (s, dev, log, _dir) = service();
        let profile = s.profile(&dev).unwrap();
        let task = s.tasks.create(TaskKind::Sync).unwrap();
        let mut id = String::new();
        task.update(|st| id = st.id.clone());
        let sql = |s: &str| RebuildStep::Sql { sql: s.into() };
        let rebuild = Rebuild {
            steps: vec![sql("CREATE TABLE t_new"), sql("FAIL"), sql("DROP TABLE t")],
            drop_at: 2,
            cleanup: "DROP TABLE t_new".into(),
            warnings: vec![],
        };
        let parts = vec![Part::Sql("ALTER A".into()), Part::Rebuild(rebuild), Part::Sql("ALTER B".into())];
        let run = Run {
            service: &s,
            task: &task,
            profile: &profile,
            dialect: Dialect::new(Variant::MySql),
            schema: "shop",
        };
        let r = run.execute(parts, "pw".into()).await;
        task.finish(r);
        let st = s.tasks.status(&id).unwrap();
        assert_eq!(st.state, TaskState::Failed);
        assert_eq!(st.failures[0].index, 2);
        assert_eq!(st.remaining.as_deref(), Some("ALTER B;"));
        let l = log.lock().unwrap().clone();
        assert!(l.contains(&"execute DROP TABLE t_new".to_string()), "{l:?}");
        assert!(!l.contains(&"execute ALTER B".to_string()), "{l:?}");
        let _ = wait(&s, &id).await;
    }
}
