//! 迁移的执行（M5b 设计文档 3.2、3.3、第 5 节）：按迁移记录依次建表、迁数据（按表并行）、建索引和约束、
//! 调整序列和自增、校验；每一步之后保存记录，继续时从记录的步骤接着做。

use std::sync::{Arc, Mutex};
use std::time::Instant;

use dblens_core::{Error, Result, Value, msg};
use dblens_dialect::migrate::StatementKind;
use dblens_dialect::migrate::data::{TableRead, identity_reset_sql, sequence_next_sql, sequence_restart_sql};
use dblens_dialect::{Compat, Dialect, Family};
use dblens_driver::{Connection, ExecEvent};
use serde::Serialize;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;

use super::copy::copy_table;
use super::record::{
    Adjustment, MigrationPhase, MigrationRecord, Mismatch, TableCopy, TableState, Verify, now_ms,
};
use super::values::{key_text, normalize, to_target};
use crate::design::Run;
use crate::import::exec;
use crate::profile::Profile;
use crate::service::Service;
use crate::task::{TaskFailure, TaskHandle};

/// 任务状态中的迁移进度（`TaskStatus::migration`）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MigrationProgress {
    pub record: String,
    pub phase: MigrationPhase,
    pub tables: Vec<TableView>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TableView {
    pub source: String,
    pub target: String,
    pub state: TableState,
    pub rows: u64,
    pub skipped: u64,
    pub estimate: Option<u64>,
    pub elapsed_ms: u64,
    pub error: Option<String>,
    pub verify: Option<Verify>,
}

/// 执行中共用的上下文（每张表的搬运在各自的任务中进行，所以都是可以复制的）。
#[derive(Clone)]
pub(crate) struct Ctx {
    pub service: Arc<Service>,
    pub task: TaskHandle,
    pub record: Arc<Mutex<MigrationRecord>>,
    pub source: Profile,
    pub target: Profile,
    pub source_dialect: Dialect,
    pub target_dialect: Dialect,
    pub source_password: String,
    pub target_password: String,
}

fn lock_error() -> Error {
    Error::internal(msg!("internal.lock_poisoned", lock = "migration"))
}

impl Ctx {
    pub fn with<R>(&self, f: impl FnOnce(&mut MigrationRecord) -> R) -> Result<R> {
        Ok(f(&mut *self.record.lock().map_err(|_| lock_error())?))
    }

    pub fn snapshot(&self) -> Result<MigrationRecord> {
        self.with(|r| r.clone())
    }

    /// 保存记录，并更新任务状态中的进度。
    pub fn save(&self) -> Result<()> {
        let r = self.with(|r| {
            r.updated_at = now_ms();
            r.clone()
        })?;
        self.service.migrations.save(&r)?;
        self.task.update(|s| s.migration = Some(progress(&r)));
        Ok(())
    }

    pub fn source_schema(&self) -> Result<String> {
        self.with(|r| r.plan.source.schema.clone())
    }

    pub fn target_schema(&self) -> Result<String> {
        self.with(|r| r.plan.target.schema.clone())
    }

    pub async fn connect_source(&self) -> Result<Box<dyn Connection>> {
        let (mut c, _) = self.service.registry.connect(&self.source, self.source_password.clone()).await?;
        exec(&mut c, &self.source_dialect.set_schema_sql(&self.source_schema()?), &[]).await?;
        Ok(c)
    }

    pub async fn connect_target(&self) -> Result<Box<dyn Connection>> {
        let (mut c, _) = self.service.registry.connect(&self.target, self.target_password.clone()).await?;
        exec(&mut c, &self.target_dialect.set_schema_sql(&self.target_schema()?), &[]).await?;
        Ok(c)
    }

    fn run(&self) -> Run<'_> {
        Run {
            service: &self.service,
            task: &self.task,
            profile: &self.target,
            dialect: self.target_dialect,
            schema: "",
        }
    }
}

pub fn progress(r: &MigrationRecord) -> MigrationProgress {
    MigrationProgress {
        record: r.id.clone(),
        phase: r.phase,
        tables: r
            .copies
            .iter()
            .filter_map(|c| {
                let p = r.tables.get(&c.source)?;
                Some(TableView {
                    source: c.source.clone(),
                    target: c.target.clone(),
                    state: p.state,
                    rows: p.rows,
                    skipped: p.skipped,
                    estimate: p.estimate,
                    elapsed_ms: p.elapsed_ms,
                    error: p.error.clone(),
                    verify: p.verify.clone(),
                })
            })
            .collect(),
    }
}

/// 查询结果的第一行第一列。
pub(crate) async fn scalar(
    conn: &mut Box<dyn Connection>,
    sql: &str,
    params: &[Value],
) -> Result<Option<Value>> {
    let mut stream = conn.execute(sql, params).await?;
    let mut out = None;
    while let Some(ev) = stream.next(10).await? {
        if let (ExecEvent::Rows(rows), None) = (ev, &out) {
            out = rows.into_iter().next().and_then(|r| r.into_iter().next());
        }
    }
    Ok(out)
}

pub(crate) fn as_u64(v: Option<Value>) -> u64 {
    match v {
        Some(Value::Int(n)) => n.max(0) as u64,
        Some(Value::UInt(n)) => n,
        Some(Value::Decimal(s) | Value::String(s)) => {
            s.trim().split('.').next().and_then(|x| x.parse().ok()).unwrap_or(0)
        }
        Some(Value::Float(f)) => f.max(0.0) as u64,
        _ => 0,
    }
}

/// 依次执行各步。已经做完的步骤跳过；出错（含取消）时记录已保存，可以继续。
pub(crate) async fn run(ctx: &Ctx) -> Result<()> {
    let phase = ctx.with(|r| r.phase)?;
    if phase <= MigrationPhase::Structure {
        statements(ctx, 1).await?;
        set_phase(ctx, MigrationPhase::Data)?;
    }
    if phase <= MigrationPhase::Data {
        data(ctx).await?;
        set_phase(ctx, MigrationPhase::Constraints)?;
    }
    if phase <= MigrationPhase::Constraints {
        statements(ctx, 2).await?;
        set_phase(ctx, MigrationPhase::Sequences)?;
    }
    if phase <= MigrationPhase::Sequences {
        adjust(ctx).await?;
        set_phase(ctx, MigrationPhase::Verify)?;
    }
    if phase <= MigrationPhase::Verify {
        verify(ctx).await?;
        set_phase(ctx, MigrationPhase::Done)?;
    }
    // 有搬运失败的表时没有完成，修改后可以继续
    ctx.with(|r| r.finished = !r.tables.values().any(|t| t.state == TableState::Failed))?;
    ctx.save()
}

fn set_phase(ctx: &Ctx, phase: MigrationPhase) -> Result<()> {
    ctx.with(|r| r.phase = phase)?;
    ctx.save()
}

/// 执行某一步的语句：已成功的跳过；建表失败的表，依赖它的语句跳过；失败的语句记下后继续。
async fn statements(ctx: &Ctx, phase: u8) -> Result<()> {
    let todo: Vec<usize> = ctx.with(|r| {
        r.statements
            .iter()
            .enumerate()
            .filter(|(i, s)| s.phase == phase && !r.done.contains(i))
            .map(|(i, _)| i)
            .collect()
    })?;
    if todo.is_empty() {
        return Ok(());
    }
    let mut conn = ctx.connect_target().await?;
    ctx.task.set_cancel(Some(conn.cancel_handle()))?;
    let run = ctx.run();
    let result = async {
        for i in todo {
            if ctx.task.cancelled() {
                return Err(Error::Cancelled);
            }
            let (s, blocked) = ctx.with(|r| {
                let s = r.statements[i].clone();
                let blocked = s.requires.iter().any(|t| r.failed_tables.contains(t));
                (s, blocked)
            })?;
            if blocked {
                ctx.task.update(|st| st.progress.statements_done += 1);
                continue;
            }
            match run.sql(&mut conn, &s.sql).await {
                Ok(_) => {
                    ctx.with(|r| {
                        r.done.insert(i);
                        r.statement_errors.remove(&i);
                    })?;
                    ctx.task.update(|st| st.progress.statements_done += 1);
                }
                Err(e) => {
                    if ctx.task.cancelled() {
                        return Err(Error::Cancelled);
                    }
                    ctx.with(|r| {
                        if matches!(s.kind, StatementKind::CreateTable | StatementKind::DropTable) {
                            r.failed_tables.insert(s.object.clone());
                            if let Some(t) = r.tables.get_mut(&s.object) {
                                t.state = TableState::Skipped;
                                t.error = Some(e.to_string());
                            }
                        }
                        r.statement_errors.insert(i, e.to_string());
                    })?;
                    let failure =
                        TaskFailure { index: i as u64, line: i as u64 + 1, error: e, text: s.sql.clone() };
                    ctx.task.fail(failure, s.sql.clone());
                    ctx.task.update(|st| {
                        st.progress.statements_done += 1;
                        st.progress.statements_failed += 1;
                    });
                }
            }
            ctx.save()?;
        }
        Ok(())
    }
    .await;
    let _ = ctx.task.set_cancel(None);
    let _ = conn.close().await;
    result
}

/// 迁数据：没有完成的表按并行数同时搬运。某张表失败只标记这张表；取消时等正在搬运的表停下后返回。
async fn data(ctx: &Ctx) -> Result<()> {
    let (copies, parallel) = ctx.with(|r| {
        let copies: Vec<TableCopy> = r
            .copies
            .iter()
            .filter(|c| {
                r.tables.get(&c.source).is_some_and(|t| {
                    matches!(t.state, TableState::Pending | TableState::Copying | TableState::Failed)
                })
            })
            .cloned()
            .collect();
        (copies, r.plan.data.parallel.clamp(1, 4))
    })?;
    let sem = Arc::new(Semaphore::new(parallel));
    let mut set = JoinSet::new();
    for c in copies {
        let permit = sem.clone().acquire_owned().await.map_err(|_| lock_error())?;
        if ctx.task.cancelled() {
            break;
        }
        let ctx = ctx.clone();
        set.spawn(async move {
            let _permit = permit;
            let started = Instant::now();
            let result = copy_table(&ctx, &c).await;
            let _ = ctx.with(|r| {
                if let Some(t) = r.tables.get_mut(&c.source) {
                    t.elapsed_ms += started.elapsed().as_millis() as u64;
                    match &result {
                        Ok(()) => {
                            t.state = TableState::Done;
                            t.error = None;
                        }
                        // 取消时保持"迁移中"，继续时从断点接着做
                        Err(Error::Cancelled) => {}
                        Err(e) => {
                            t.state = TableState::Failed;
                            t.error = Some(e.to_string());
                        }
                    }
                }
            });
            let _ = ctx.save();
            result
        });
    }
    let mut cancelled = ctx.task.cancelled();
    while let Some(r) = set.join_next().await {
        if matches!(r, Ok(Err(Error::Cancelled))) {
            cancelled = true;
        }
    }
    if cancelled { Err(Error::Cancelled) } else { Ok(()) }
}

/// 调整自增列和序列：自增推到当前最大值之后，序列推到源序列的位置。出错只记下。
async fn adjust(ctx: &Ctx) -> Result<()> {
    let (copies, sequences, source_schema, target_schema) = ctx.with(|r| {
        let copies: Vec<TableCopy> = r
            .copies
            .iter()
            .filter(|c| {
                c.identity.is_some() && r.tables.get(&c.source).is_some_and(|t| t.state == TableState::Done)
            })
            .cloned()
            .collect();
        (copies, r.sequences.clone(), r.plan.source.schema.clone(), r.plan.target.schema.clone())
    })?;
    if copies.is_empty() && sequences.is_empty() {
        return Ok(());
    }
    let (sd, td) = (ctx.source_dialect, ctx.target_dialect);
    let mut tgt = ctx.connect_target().await?;
    let mut src = if sequences.is_empty() { None } else { Some(ctx.connect_source().await?) };
    let mut out = Vec::new();
    for c in &copies {
        let column = c.identity.as_deref().unwrap_or_default();
        if let Some(sql) = identity_reset_sql(&td, &target_schema, &c.target, column)? {
            let error = exec(&mut tgt, &sql, &[]).await.err().map(|e| e.to_string());
            out.push(Adjustment { object: format!("{}.{column}", c.target), sql, error });
        }
    }
    for (name, seq) in &sequences {
        let (Some(conn), Some(q)) = (src.as_mut(), sequence_next_sql(&sd, &source_schema, name)?) else {
            continue;
        };
        let next = match scalar(conn, &q, &[]).await {
            Ok(v) => as_u64(v),
            Err(e) => {
                out.push(Adjustment { object: seq.name.clone(), sql: q, error: Some(e.to_string()) });
                continue;
            }
        };
        for sql in sequence_restart_sql(&td, &target_schema, seq, &next.max(1).to_string())? {
            let error = exec(&mut tgt, &sql, &[]).await.err().map(|e| e.to_string());
            let failed = error.is_some();
            out.push(Adjustment { object: seq.name.clone(), sql, error });
            if failed {
                break;
            }
        }
    }
    let _ = tgt.close().await;
    if let Some(c) = src {
        let _ = c.close().await;
    }
    ctx.with(|r| r.adjustments = out)?;
    ctx.save()
}

/// 校验：每张迁完的表比对行数；设了抽样时按主键取最前和最后各一半逐值比对。
async fn verify(ctx: &Ctx) -> Result<()> {
    let (copies, sample) = ctx.with(|r| {
        let copies: Vec<TableCopy> = r
            .copies
            .iter()
            .filter(|c| r.tables.get(&c.source).is_some_and(|t| t.state == TableState::Done))
            .cloned()
            .collect();
        (copies, r.plan.data.sample)
    })?;
    if copies.is_empty() {
        return Ok(());
    }
    let mut src = ctx.connect_source().await?;
    let mut tgt = ctx.connect_target().await?;
    let result = async {
        for c in &copies {
            if ctx.task.cancelled() {
                return Err(Error::Cancelled);
            }
            let v = verify_table(ctx, c, &mut src, &mut tgt, sample).await?;
            ctx.with(|r| {
                if let Some(t) = r.tables.get_mut(&c.source) {
                    t.verify = Some(v);
                }
            })?;
            ctx.save()?;
        }
        Ok(())
    }
    .await;
    let _ = src.close().await;
    let _ = tgt.close().await;
    result
}

/// 抽样比对中不一致的处至多记这么多。
const MAX_MISMATCHES: usize = 20;

async fn verify_table(
    ctx: &Ctx,
    c: &TableCopy,
    src: &mut Box<dyn Connection>,
    tgt: &mut Box<dyn Connection>,
    sample: u64,
) -> Result<Verify> {
    let (sd, td) = (ctx.source_dialect, ctx.target_dialect);
    let (ss, ts) = (ctx.source_schema()?, ctx.target_schema()?);
    let key_names: Vec<String> = c.key.iter().map(|&i| c.source_columns[i].clone()).collect();
    let target_names: Vec<String> = c.target_columns.iter().map(|x| x.name.clone()).collect();
    let target_key: Vec<String> = c.key.iter().map(|&i| target_names[i].clone()).collect();
    let read = TableRead {
        schema: &ss,
        table: &c.source,
        columns: &c.source_columns,
        key: &key_names,
        filter: c.filter.as_deref(),
    };
    let target_read =
        TableRead { schema: &ts, table: &c.target, columns: &target_names, key: &target_key, filter: None };
    let source_rows = as_u64(scalar(src, &read.count(&sd), &[]).await?);
    let target_rows = as_u64(scalar(tgt, &target_read.count(&td), &[]).await?);
    let (base, skipped) = ctx
        .with(|r| r.tables.get(&c.source).map(|t| (t.base_rows.unwrap_or(0), t.skipped)).unwrap_or((0, 0)))?;
    let mut v = Verify {
        source_rows,
        target_rows,
        expected_rows: (base + source_rows).saturating_sub(skipped),
        sampled: 0,
        mismatches: vec![],
    };
    if sample == 0 || c.key.is_empty() {
        return Ok(v);
    }
    let mut rows = Vec::new();
    for (desc, n) in [(false, sample / 2), (true, sample - sample / 2)] {
        let sql = read.sample(&sd, desc, n);
        let mut stream = src.execute(&sql, &[]).await?;
        while let Some(ev) = stream.next(1000).await? {
            if let ExecEvent::Rows(r) = ev {
                rows.extend(r);
            }
        }
    }
    let mut seen = std::collections::HashSet::new();
    for row in rows {
        let key: Vec<Value> = c.key.iter().map(|&i| row[i].clone()).collect();
        if !seen.insert(key_text(&key)) {
            continue;
        }
        v.sampled += 1;
        let params: Vec<Value> = c
            .key
            .iter()
            .map(|&i| to_target(&row[i], &c.target_columns[i]))
            .collect::<Result<_>>()
            .unwrap_or_else(|_| key.clone());
        let sql = target_read.row(&td, &params);
        let mut stream = tgt.execute(&sql, &params).await?;
        let mut found = None;
        while let Some(ev) = stream.next(10).await? {
            if let (ExecEvent::Rows(r), None) = (ev, &found) {
                found = r.into_iter().next();
            }
        }
        drop(stream);
        let Some(target_row) = found else {
            if v.mismatches.len() < MAX_MISMATCHES {
                v.mismatches.push(Mismatch {
                    key: key_text(&key),
                    column: String::new(),
                    source: String::new(),
                    target: String::new(),
                });
            }
            continue;
        };
        let empty_is_null = td.family() == Family::Oracle || td.compat() == Compat::Oracle;
        for (i, col) in c.target_columns.iter().enumerate() {
            let (a, b) =
                (normalize(&row[i], col, empty_is_null), normalize(&target_row[i], col, empty_is_null));
            if a != b && v.mismatches.len() < MAX_MISMATCHES {
                v.mismatches.push(Mismatch {
                    key: key_text(&key),
                    column: col.name.clone(),
                    source: a,
                    target: b,
                });
            }
        }
    }
    Ok(v)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use dblens_dialect::Variant;
    use dblens_dialect::migrate::MigrationStatement;

    use super::*;
    use crate::compare::tests::service;
    use crate::migrate::record::tests::record;
    use crate::task::{TaskKind, TaskState};

    #[tokio::test]
    async fn statements_skip_dependents_and_resume_retries_failures() {
        let (s, profile, log, dir) = service();
        s.migrations.set_dir(dir.path().join("migrations"));
        let profile = s.profile(&profile).unwrap();
        let st = |phase, kind, object: &str, requires: &[&str], sql: &str| MigrationStatement {
            phase,
            kind,
            object: object.into(),
            requires: requires.iter().map(|r| r.to_string()).collect(),
            sql: sql.into(),
        };
        let mut r = record("c3");
        r.phase = MigrationPhase::Structure;
        r.tables = BTreeMap::new();
        r.statements = vec![
            st(1, StatementKind::CreateTable, "a", &[], "FAIL"),
            st(1, StatementKind::CreateTable, "b", &[], "CREATE TABLE b"),
            st(1, StatementKind::Comment, "a", &["a"], "COMMENT ON a"),
            st(2, StatementKind::Index, "b", &["b"], "FAIL"),
            st(2, StatementKind::ForeignKey, "b", &["b", "a"], "ALTER TABLE b ADD FK"),
            st(2, StatementKind::Unique, "b", &["b"], "ALTER TABLE b ADD UNIQUE"),
        ];
        let task = s.tasks.create(TaskKind::Migrate).unwrap();
        let ctx = Ctx {
            service: s.clone(),
            task: task.clone(),
            record: Arc::new(Mutex::new(r)),
            source: profile.clone(),
            target: profile,
            source_dialect: Dialect::new(Variant::Postgres),
            target_dialect: Dialect::new(Variant::Postgres),
            source_password: String::new(),
            target_password: String::new(),
        };
        run(&ctx).await.unwrap();
        task.finish(Ok(()));
        let r = ctx.snapshot().unwrap();
        assert_eq!(r.phase, MigrationPhase::Done);
        assert!(r.finished);
        assert_eq!(r.done, [1, 5].into_iter().collect());
        assert_eq!(r.failed_tables, ["a".to_string()].into_iter().collect());
        assert_eq!(r.statement_errors.keys().copied().collect::<Vec<_>>(), [0, 3]);
        // 记录已保存
        assert_eq!(s.migrations.load("c3").unwrap().done, r.done);
        let st = s.tasks.status(&task_id(&ctx)).unwrap();
        assert_eq!(st.state, TaskState::Done);
        assert_eq!(st.migration.as_ref().map(|m| m.phase), Some(MigrationPhase::Done));
        let log = log.lock().unwrap().clone();
        assert!(
            log.iter().any(|l| l.contains("ADD UNIQUE")) && !log.iter().any(|l| l.contains("COMMENT ON a"))
        );
    }

    fn task_id(ctx: &Ctx) -> String {
        let mut id = String::new();
        ctx.task.update(|s| id = s.id.clone());
        id
    }
}
