//! 表设计器（M3 设计文档第 6 节）：读取原表、预览改动生成的 DDL（或重建脚本）、作为任务执行。
//! 执行时用草稿重新生成语句，不接受前端传来的 SQL；执行前核对原表没有被别人改过（`fingerprint`）。

use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use dblens_core::schema::Table;
use dblens_core::{Error, Msg, Result, msg};
use dblens_dialect::Dialect;
use dblens_dialect::design::{
    DesignCaps, DesignIssue, DesignStatement, Rebuild, RebuildContext, RebuildOutcome, RebuildStep, Renames,
    TableChange, diff_table, validate,
};
use dblens_dialect::meta::{self, MetaArgs, MetaKey, MetaRows, TableStorage};
use dblens_driver::{Connection, collect_all};
use serde::{Deserialize, Serialize};

use crate::history::NewHistory;
use crate::import::exec;
use crate::policy::{Decision, ExecContext, classify};
use crate::profile::{Profile, driver_name, variant_code};
use crate::service::Service;
use crate::task::{TaskFailure, TaskHandle, TaskKind, TaskPhase};

/// 预览和执行的请求。
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct DesignRequest {
    pub schema: String,
    /// 已有表的名字；新建表时不给。
    #[serde(default)]
    pub name: Option<String>,
    pub draft: Table,
    #[serde(default)]
    pub renames: Renames,
    /// 用户在预览中手动选择了重建。
    #[serde(default)]
    pub rebuild: bool,
    /// 打开设计器时原表的指纹；执行前核对。
    #[serde(default)]
    pub fingerprint: Option<String>,
}

/// 打开设计器时读到的原表。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DesignTable {
    pub table: Table,
    pub storage: TableStorage,
    pub fingerprint: String,
}

/// 预览中的一条语句：普通 SQL，或由程序执行的核对行数（`sql` 为空）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PreviewStatement {
    #[serde(flatten)]
    pub step: RebuildStep,
    /// 对应的变更序号（重建脚本中为空）。
    pub changes: Vec<usize>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RebuildInfo {
    /// 方言确定不支持的改动：不为空时必须重建。
    pub reasons: Vec<Msg>,
    /// 这次预览是否用重建方式。
    pub active: bool,
    /// 不能重建的原因。
    pub refused: Option<Msg>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DesignPreview {
    pub errors: Vec<DesignIssue>,
    pub changes: Vec<TableChange>,
    pub statements: Vec<PreviewStatement>,
    pub rebuild: RebuildInfo,
    /// 提示：主键列改为 NOT NULL、重建期间不要写入等。
    pub warnings: Vec<Msg>,
    /// 整体在一个事务中执行（PG 家族）。
    pub transactional: bool,
    /// 被执行策略拒绝的原因（有则不能执行）。
    pub denied: Option<Msg>,
    /// 需要用户确认的原因（生产环境或危险语句）。
    pub confirm: Vec<Msg>,
    /// 原表已被修改（与请求中的指纹不同）。
    pub conflict: bool,
}

/// 生成好的执行计划。
enum Plan {
    Alter(Vec<DesignStatement>),
    Rebuild(Rebuild),
}

impl Plan {
    fn parts(self) -> Vec<Part> {
        match self {
            Plan::Alter(statements) => statements.into_iter().map(|s| Part::Sql(s.sql)).collect(),
            Plan::Rebuild(r) => vec![Part::Rebuild(r)],
        }
    }
}

/// 执行器依次执行的一段：一条语句，或一张表的重建（M4a 的同步脚本由多段组成）。
pub(crate) enum Part {
    Sql(String),
    Rebuild(Rebuild),
}

impl Part {
    /// 这一段的语句文本（核对行数写成两条 `SELECT COUNT(*)`）。
    pub(crate) fn texts(&self) -> Vec<String> {
        match self {
            Part::Sql(sql) => vec![sql.clone()],
            Part::Rebuild(r) => r
                .steps
                .iter()
                .map(|s| match s {
                    RebuildStep::Sql { sql } => sql.clone(),
                    RebuildStep::CountCheck { source, target } => {
                        format!("SELECT COUNT(*) FROM {source}; SELECT COUNT(*) FROM {target}")
                    }
                })
                .collect(),
        }
    }

    pub(crate) fn len(parts: &[Part]) -> u64 {
        parts.iter().map(|p| p.texts().len() as u64).sum()
    }
}

struct Prepared {
    profile: Profile,
    dialect: Dialect,
    preview: DesignPreview,
    plan: Option<Plan>,
    /// 其他表指向本表的外键所在的 schema（执行后这些 schema 的缓存也失效）。
    related: Vec<String>,
}

fn fingerprint(t: &Table) -> String {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    serde_json::to_string(t).unwrap_or_default().hash(&mut h);
    format!("{:016x}", h.finish())
}

fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64)
}

impl Service {
    pub async fn design_caps(&self, profile_id: &str) -> Result<DesignCaps> {
        let profile = self.profile(profile_id)?;
        Ok(self.metadata.dialect(&profile).await?.design_caps())
    }

    /// 元数据查询，跳过缓存（设计器要以库中最新的结构为准）。
    async fn fresh_rows(
        &self,
        profile: &Profile,
        key: MetaKey,
        schema: &str,
        table: &str,
    ) -> Result<Arc<MetaRows>> {
        let args = MetaArgs { schema: Some(schema.into()), table: Some(table.into()), ..Default::default() };
        self.metadata.cache().forget(&profile.id, key, &args);
        Ok(self.metadata.rows(profile, key, &args).await?.unwrap_or_default())
    }

    pub(crate) async fn fresh_table(&self, profile: &Profile, schema: &str, name: &str) -> Result<Table> {
        let args = MetaArgs { schema: Some(schema.into()), table: Some(name.into()), ..Default::default() };
        for key in MetaKey::TABLE_DETAIL {
            self.metadata.cache().forget(&profile.id, key, &args);
        }
        self.metadata.tables(profile, schema, Some(name)).await?.into_iter().next().ok_or_else(|| {
            Error::not_found(msg!("noun.object", kind = "table", name = format!("{schema}.{name}")))
        })
    }

    /// 打开设计器：原表（直接从库读取）、存储属性和行数估计、指纹。
    pub async fn design_table(&self, profile_id: &str, schema: &str, name: &str) -> Result<DesignTable> {
        let profile = self.feature_profile(profile_id)?;
        let table = self.fresh_table(&profile, schema, name).await?;
        let storage =
            meta::table_storage(&*self.fresh_rows(&profile, MetaKey::TableStorage, schema, name).await?)?;
        Ok(DesignTable { fingerprint: fingerprint(&table), table, storage })
    }

    /// 重建脚本要用的依赖信息。
    pub(crate) async fn rebuild_context(
        &self,
        profile: &Profile,
        dialect: &Dialect,
        schema: &str,
        t: &Table,
    ) -> Result<RebuildContext> {
        let name = &t.name;
        let rows = |key| self.fresh_rows(profile, key, schema, name);
        let mut ctx = RebuildContext {
            referencing: meta::referencing_keys(&*rows(MetaKey::ReferencingKeys).await?, schema, name)?,
            storage: meta::table_storage(&*rows(MetaKey::TableStorage).await?)?,
            ..Default::default()
        };
        if dialect.has_meta(MetaKey::DependentViews) {
            ctx.views = meta::dependent_views(&*rows(MetaKey::DependentViews).await?)?;
        }
        if dialect.has_meta(MetaKey::Grants) {
            ctx.grants = meta::grants(&*rows(MetaKey::Grants).await?)?;
        }
        ctx.triggers = if dialect.has_meta(MetaKey::TriggerDdl) {
            meta::trigger_ddl(&*rows(MetaKey::TriggerDdl).await?)?
        } else {
            t.triggers.iter().filter_map(|tr| tr.definition.clone()).collect()
        };
        Ok(ctx)
    }

    async fn prepare_design(&self, profile_id: &str, req: &DesignRequest) -> Result<Prepared> {
        let profile = self.feature_profile(profile_id)?;
        let dialect = self.metadata.dialect(&profile).await?;
        let mut draft = req.draft.clone();
        if let Some(name) = &req.name {
            // 已有表不能改名
            draft.name = name.clone();
        }
        let mut warnings = dialect.prepare_draft(&mut draft);
        let original = match &req.name {
            Some(n) => Some(self.fresh_table(&profile, &req.schema, n).await?),
            None => None,
        };
        let conflict = match (&original, &req.fingerprint) {
            (Some(t), Some(fp)) => &fingerprint(t) != fp,
            _ => false,
        };
        let mut preview = DesignPreview {
            errors: validate(&draft),
            changes: vec![],
            statements: vec![],
            rebuild: RebuildInfo { reasons: vec![], active: false, refused: None },
            warnings: vec![],
            transactional: dialect.design_transactional(),
            denied: None,
            confirm: vec![],
            conflict,
        };
        let mut prepared =
            Prepared { profile, dialect, preview: preview.clone(), plan: None, related: vec![] };
        if !preview.errors.is_empty() {
            prepared.preview = preview;
            return Ok(prepared);
        }
        let diff = diff_table(original.as_ref(), &draft, &req.renames)?;
        preview.rebuild.reasons = dialect.design_unsupported(&diff);
        preview.changes = diff.changes.clone();
        let rebuild = original.is_some()
            && !diff.changes.is_empty()
            && (req.rebuild || !preview.rebuild.reasons.is_empty());
        let plan = match (&original, rebuild) {
            _ if diff.changes.is_empty() => None,
            (Some(orig), true) => {
                preview.rebuild.active = true;
                let ctx = self.rebuild_context(&prepared.profile, &dialect, &req.schema, orig).await?;
                prepared.related = ctx.referencing.iter().map(|r| r.schema.clone()).collect();
                match dialect.rebuild(&req.schema, orig, &draft, &req.renames, &ctx)? {
                    RebuildOutcome::Refused { reason } => {
                        preview.rebuild.refused = Some(reason);
                        None
                    }
                    RebuildOutcome::Script(r) => {
                        warnings.extend(r.warnings.iter().cloned());
                        preview.statements = r
                            .steps
                            .iter()
                            .map(|s| PreviewStatement { step: s.clone(), changes: vec![] })
                            .collect();
                        Some(Plan::Rebuild(r))
                    }
                }
            }
            _ => {
                let statements = dialect.design_sql(&req.schema, &draft.name, &diff)?;
                preview.statements = statements
                    .iter()
                    .map(|s| PreviewStatement {
                        step: RebuildStep::Sql { sql: s.sql.clone() },
                        changes: s.changes.clone(),
                    })
                    .collect();
                Some(Plan::Alter(statements))
            }
        };
        preview.warnings = warnings;
        let sqls: Vec<String> = preview
            .statements
            .iter()
            .filter_map(|s| match &s.step {
                RebuildStep::Sql { sql } => Some(sql.clone()),
                RebuildStep::CountCheck { .. } => None,
            })
            .collect();
        (preview.denied, preview.confirm) = self.check_policy(&prepared.profile, dialect, &sqls).await;
        prepared.preview = preview;
        prepared.plan = plan;
        Ok(prepared)
    }

    /// 执行策略：被拒绝的语句不能执行（返回第一个原因）；需要确认的列出原因（生产环境总要确认）。
    pub(crate) async fn check_policy(
        &self,
        profile: &Profile,
        dialect: Dialect,
        sqls: &[String],
    ) -> (Option<Msg>, Vec<Msg>) {
        let ctx =
            ExecContext { profile_id: profile.id.clone(), environment: profile.input.environment, dialect };
        let policy = self.sessions.policy();
        let (mut denied, mut confirm) = (None, Vec::new());
        for sql in sqls {
            match policy.before_execute(&ctx, &classify(&dialect, sql)).await {
                Decision::Allow => {}
                Decision::Confirm { reason } => {
                    if !confirm.contains(&reason) {
                        confirm.push(reason);
                    }
                }
                Decision::Deny { reason } => {
                    denied.get_or_insert(reason);
                }
            }
        }
        (denied, confirm)
    }

    /// 预览：校验错误、变更列表、生成的语句或重建脚本、提示、执行策略的结论。不执行。
    pub async fn preview_design(&self, profile_id: &str, req: &DesignRequest) -> Result<DesignPreview> {
        Ok(self.prepare_design(profile_id, req).await?.preview)
    }

    /// 执行：作为任务在单独的连接上运行，返回任务 id。有校验错误、被拒绝、原表已被修改、没有改动时报错。
    pub async fn start_design(self: &Arc<Self>, profile_id: &str, req: DesignRequest) -> Result<String> {
        let prepared = self.prepare_design(profile_id, &req).await?;
        let p = &prepared.preview;
        if !p.errors.is_empty() {
            return Err(Error::invalid(msg!("design.has_errors")));
        }
        if p.conflict {
            return Err(Error::invalid(msg!("design.conflict")));
        }
        if let Some(reason) = &p.rebuild.refused {
            return Err(Error::invalid(reason.clone()));
        }
        if let Some(reason) = &p.denied {
            return Err(Error::Denied { reason: reason.clone() });
        }
        let Some(plan) = prepared.plan else {
            return Err(Error::invalid(msg!("design.no_changes")));
        };
        let password = self.secrets.get(&prepared.profile.id)?.unwrap_or_default();
        let task = self.tasks.create(TaskKind::Design)?;
        let mut id = String::new();
        let parts = plan.parts();
        let total = Part::len(&parts);
        task.update(|s| {
            id = s.id.clone();
            s.phase = TaskPhase::Executing;
            s.progress.statements_total = total;
        });
        let service = self.clone();
        let (profile, dialect, schema) = (prepared.profile, prepared.dialect, req.schema);
        let mut schemas = prepared.related;
        schemas.push(schema.clone());
        tokio::spawn(async move {
            let run = Run { service: &service, task: &task, profile: &profile, dialect, schema: &schema };
            let result = run.execute(parts, password).await;
            for s in &schemas {
                service.metadata.cache().invalidate(&profile.id, Some(s));
            }
            task.finish(result);
        });
        Ok(id)
    }
}

/// 一次执行：表设计器的改动、比对后的同步脚本。
pub(crate) struct Run<'a> {
    pub service: &'a Service,
    pub task: &'a TaskHandle,
    pub profile: &'a Profile,
    pub dialect: Dialect,
    pub schema: &'a str,
}

impl Run<'_> {
    fn record(&self, sql: &str, at: i64, started: Instant, rows: Option<u64>, error: Option<&Error>) {
        self.service.history.record(NewHistory {
            at,
            profile_id: self.profile.id.clone(),
            profile_name: self.profile.input.name.clone(),
            driver: driver_name(self.profile.input.driver).to_string(),
            variant: Some(variant_code(self.dialect.variant())),
            schema: Some(self.schema.to_string()),
            sql: sql.to_string(),
            elapsed_ms: Some(started.elapsed().as_millis() as u64),
            rows,
            error: error.map(ToString::to_string),
        });
    }

    /// 执行一条语句并记入历史。
    pub(crate) async fn sql(&self, conn: &mut Box<dyn Connection>, sql: &str) -> Result<u64> {
        let (at, started) = (now_ms(), Instant::now());
        let result = exec(conn, sql, &[]).await;
        self.record(sql, at, started, result.as_ref().ok().copied(), result.as_ref().err());
        result
    }

    async fn count(&self, conn: &mut Box<dyn Connection>, table: &str) -> Result<i64> {
        let sql = format!("SELECT COUNT(*) AS n FROM {table}");
        let mut stream = conn.execute(&sql, &[]).await?;
        let rows = MetaRows::from_events(collect_all(stream.as_mut(), 10).await?)?;
        Ok(rows.iter().next().and_then(|r| r.int("n")).unwrap_or(-1))
    }

    /// 第 `index` 步失败：记下失败的语句和没有执行的语句。
    fn failed(&self, index: usize, text: &str, error: &Error, rest: &[String]) {
        let failure = TaskFailure {
            index: index as u64,
            line: index as u64 + 1,
            error: error.clone(),
            text: text.into(),
        };
        self.task.fail(failure, text.into());
        let rest = rest.join(";\n");
        self.task.update(|s| s.remaining = (!rest.is_empty()).then(|| rest + ";"));
    }

    /// 在新连接上依次执行各段。PG 家族在一个事务中执行；其他库失败时列出没有执行的语句。
    pub(crate) async fn execute(&self, parts: Vec<Part>, password: String) -> Result<()> {
        let (mut conn, _) = self.service.registry.connect(self.profile, password).await?;
        self.task.set_cancel(Some(conn.cancel_handle()))?;
        let transactional = self.dialect.design_transactional();
        let outcome = async {
            exec(&mut conn, &self.dialect.set_schema_sql(self.schema), &[]).await?;
            if transactional {
                conn.set_autocommit(false).await?;
            }
            let texts: Vec<Vec<String>> = parts.iter().map(Part::texts).collect();
            let mut offset = 0;
            for (pi, part) in parts.iter().enumerate() {
                // PG 回滚全部，没有执行的语句不必再列出
                let later: Vec<String> = if transactional { vec![] } else { texts[pi + 1..].concat() };
                match part {
                    Part::Sql(sql) => {
                        if self.task.cancelled() {
                            return Err(Error::Cancelled);
                        }
                        if let Err(e) = self.sql(&mut conn, sql).await {
                            let e = if self.task.cancelled() { Error::Cancelled } else { e };
                            self.failed(offset, sql, &e, &later);
                            return Err(e);
                        }
                        self.task.update(|s| s.progress.statements_done += 1);
                    }
                    Part::Rebuild(r) => self.rebuild(&mut conn, r, transactional, offset, &later).await?,
                }
                offset += texts[pi].len();
            }
            if transactional {
                conn.commit().await?;
            }
            Ok(())
        }
        .await;
        if outcome.is_err() && transactional {
            let _ = conn.rollback().await;
        }
        let _ = conn.close().await;
        outcome
    }

    /// 一张表的重建。`offset`：第一步在整个脚本中的序号；`later`：后面各段的语句（失败时列为没有执行）。
    async fn rebuild(
        &self,
        conn: &mut Box<dyn Connection>,
        r: &Rebuild,
        transactional: bool,
        offset: usize,
        later: &[String],
    ) -> Result<()> {
        let texts = Part::Rebuild(r.clone()).texts();
        for (i, step) in r.steps.iter().enumerate() {
            if i == r.drop_at {
                // 从删除旧表起不能取消
                self.task.set_cancel(None)?;
            }
            let before_drop = i < r.drop_at;
            if before_drop && self.task.cancelled() {
                self.cleanup(conn, r, transactional).await;
                return Err(Error::Cancelled);
            }
            let result = match step {
                RebuildStep::Sql { sql } => self.sql(conn, sql).await.map(|_| ()),
                RebuildStep::CountCheck { source, target } => {
                    async {
                        let (a, b) = (self.count(conn, source).await?, self.count(conn, target).await?);
                        if a == b {
                            Ok(())
                        } else {
                            Err(Error::invalid(msg!("design.count_mismatch", source = a, target = b)))
                        }
                    }
                    .await
                }
            };
            if let Err(e) = result {
                let e = if before_drop && self.task.cancelled() { Error::Cancelled } else { e };
                // 删除旧表之前失败时这张表的改动已撤销，只列出后面各段
                let rest = match (transactional, before_drop) {
                    (true, _) => vec![],
                    (false, true) => later.to_vec(),
                    (false, false) => [&texts[i + 1..], later].concat(),
                };
                self.failed(offset + i, &texts[i], &e, &rest);
                if before_drop {
                    self.cleanup(conn, r, transactional).await;
                }
                return Err(e);
            }
            self.task.update(|s| s.progress.statements_done += 1);
        }
        Ok(())
    }

    /// 删除旧表之前失败或取消：删除新表（PG 由回滚撤销，不必删除）。
    async fn cleanup(&self, conn: &mut Box<dyn Connection>, r: &Rebuild, transactional: bool) {
        if !transactional {
            let _ = self.sql(conn, &r.cleanup).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use dblens_core::schema::{Column, PrimaryKey};
    use dblens_core::{ColumnType, LogicalType};
    use dblens_dialect::Variant;

    use crate::fake::{FakeDriver, Log};
    use crate::history::{HistoryLog, HistoryQuery};
    use crate::policy::DefaultPolicy;
    use crate::profile::tests::input;
    use crate::profile::{Environment, ProfileInput};
    use crate::registry::DriverRegistry;
    use crate::secret::{SecretBackend, Secrets};
    use crate::store::Store;
    use crate::task::{TaskState, TaskStatus};

    use super::*;

    fn service() -> (Arc<Service>, String, Log, tempfile::TempDir) {
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
        let dev = s
            .create_profile(ProfileInput { environment: Environment::Dev, ..input("dev") }, Some("pw"))
            .unwrap();
        (Arc::new(s), dev.id, log, dir)
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

    fn executed(log: &Log) -> Vec<String> {
        log.lock().unwrap().iter().filter_map(|l| l.strip_prefix("execute ").map(str::to_string)).collect()
    }

    fn new_table(name: &str) -> DesignRequest {
        let col = |name: &str, native: &str| Column {
            name: name.into(),
            ordinal: 0,
            data_type: ColumnType { native: native.into(), logical: LogicalType::Other },
            nullable: true,
            default: None,
            comment: Some("说明".into()),
            auto_increment: false,
            generated: None,
        };
        DesignRequest {
            schema: "public".into(),
            name: None,
            draft: Table {
                name: name.into(),
                columns: vec![col("id", "bigint"), col("名称", "varchar(20)")],
                primary_key: Some(PrimaryKey { name: None, columns: vec!["id".into()] }),
                ..Default::default()
            },
            renames: Renames::default(),
            rebuild: false,
            fingerprint: None,
        }
    }

    fn sql_of(p: &DesignPreview) -> Vec<String> {
        p.statements
            .iter()
            .map(|s| match &s.step {
                RebuildStep::Sql { sql } => sql.clone(),
                RebuildStep::CountCheck { .. } => panic!("新建表不会核对行数"),
            })
            .collect()
    }

    #[tokio::test]
    async fn preview_new_table() {
        let (s, dev, _log, _dir) = service();
        let p = s.preview_design(&dev, &new_table("t")).await.unwrap();
        assert!(p.errors.is_empty() && p.transactional && !p.conflict && p.denied.is_none());
        assert!(matches!(p.changes[..], [TableChange::CreateTable { .. }]));
        let sql = sql_of(&p);
        assert!(sql[0].starts_with("CREATE TABLE public.t (\n    id bigint NOT NULL,"), "{}", sql[0]);
        assert_eq!(sql.len(), 3, "建表加两条列注释");
        assert_eq!(p.warnings, vec![msg!("design.pk_not_null", column = "id")]);
        // 校验错误：不生成语句，不能执行
        let mut bad = new_table("");
        bad.draft.columns[1].data_type.native = String::new();
        let p = s.preview_design(&dev, &bad).await.unwrap();
        assert_eq!(p.errors.len(), 2);
        assert!(p.statements.is_empty());
        assert!(matches!(s.start_design(&dev, bad).await, Err(Error::Invalid { .. })));
    }

    #[tokio::test]
    async fn apply_runs_in_one_transaction_and_records_history() {
        let (s, dev, log, _dir) = service();
        let id = s.start_design(&dev, new_table("t")).await.unwrap();
        let st = wait(&s, &id).await;
        assert_eq!(st.state, TaskState::Done, "{st:?}");
        assert_eq!((st.progress.statements_done, st.progress.statements_total), (3, 3));
        let l = log.lock().unwrap().clone();
        let at = |x: &str| l.iter().position(|e| e == x).unwrap_or_else(|| panic!("{x} 不在 {l:?}"));
        assert!(at("execute SET search_path TO \"public\"") < at("autocommit false"));
        assert!(at("autocommit false") < at("commit"));
        assert_eq!(s.store.history(&HistoryQuery::default()).unwrap().len(), 3);
    }

    /// 直接执行一份重建计划（MySQL：不在事务中）。删除旧表之前失败要删除新表；之后失败列出没有执行的语句。
    #[tokio::test]
    async fn rebuild_failure_before_and_after_dropping_the_old_table() {
        let (s, dev, log, _dir) = service();
        let profile = s.profile(&dev).unwrap();
        let sql = |s: &str| RebuildStep::Sql { sql: s.into() };
        let count = || RebuildStep::CountCheck { source: "t".into(), target: "t_new".into() };
        let run = |steps: Vec<RebuildStep>| {
            let (s, profile) = (s.clone(), profile.clone());
            async move {
                let task = s.tasks.create(TaskKind::Design).unwrap();
                let mut id = String::new();
                task.update(|st| id = st.id.clone());
                let plan =
                    Rebuild { steps, drop_at: 3, cleanup: "DROP TABLE t_new".into(), warnings: vec![] };
                let run = Run {
                    service: &s,
                    task: &task,
                    profile: &profile,
                    dialect: Dialect::new(Variant::MySql),
                    schema: "shop",
                };
                let r = run.execute(vec![Part::Rebuild(plan)], "pw".into()).await;
                task.finish(r);
                s.tasks.status(&id).unwrap()
            }
        };

        let st = run(vec![sql("CREATE TABLE t_new"), sql("FAIL"), count(), sql("DROP TABLE t")]).await;
        assert_eq!(st.state, TaskState::Failed);
        assert_eq!(st.remaining, None, "删除旧表之前失败：旧表完好，不列出剩余语句");
        let ex = executed(&log);
        assert_eq!(ex.last().unwrap(), "DROP TABLE t_new", "{ex:?}");
        assert!(!ex.contains(&"DROP TABLE t".to_string()));

        log.lock().unwrap().clear();
        let st = run(vec![
            sql("CREATE TABLE t_new"),
            sql("INSERT INTO t_new SELECT * FROM t"),
            count(),
            sql("DROP TABLE t"),
            sql("FAIL"),
            sql("CREATE INDEX a ON t (x)"),
            sql("CREATE INDEX b ON t (y)"),
        ])
        .await;
        assert_eq!(st.state, TaskState::Failed);
        assert_eq!(st.failures[0].index, 4);
        assert_eq!(st.remaining.as_deref(), Some("CREATE INDEX a ON t (x);\nCREATE INDEX b ON t (y);"));
        let ex = executed(&log);
        assert!(ex.contains(&"SELECT COUNT(*) AS n FROM t_new".to_string()), "{ex:?}");
        assert!(!ex.contains(&"DROP TABLE t_new".to_string()), "删除旧表之后不再删除新表");
    }
}
