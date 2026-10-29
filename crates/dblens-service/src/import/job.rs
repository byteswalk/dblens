//! 导入任务（M2b 设计文档第 5 节）：另开连接，读文件、转换、按批写入；整体一个事务或分批提交，
//! 分批时出错可停止或跳过（跳过时把出错的批逐行重试，失败的行写入出错报告）。

use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use dblens_core::schema::{Column, PrimaryKey, Table};
use dblens_core::{ColumnType, Error, LogicalType, Result, Value, msg};
use dblens_dialect::{Dialect, ImportStatement, WriteMode};
use dblens_driver::{Connection, ExecEvent};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

use super::file::{FileOptions, FileReader, FileRow};
use super::text;
use super::values::{EmptyAs, convert};
use crate::history::NewHistory;
use crate::policy::{Decision, ExecContext, classify};
use crate::profile::{Profile, driver_name, variant_code};
use crate::service::Service;
use crate::task::{FileSource, TaskFailure, TaskHandle, TaskKind, TaskPhase};

/// 每批行数的范围与默认值。
pub const DEFAULT_BATCH: usize = 1000;
const BATCH_RANGE: std::ops::RangeInclusive<usize> = 100..=100_000;

/// 新建表的一列。
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct NewColumn {
    pub name: String,
    pub logical: LogicalType,
    #[serde(default = "yes")]
    pub nullable: bool,
}

fn yes() -> bool {
    true
}

/// 新建表的定义（表名、schema 在请求中）。
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct NewTable {
    pub columns: Vec<NewColumn>,
    #[serde(default)]
    pub primary_key: Vec<String>,
}

/// 文件的第 `source` 列（从 0 开始）写入表的 `target` 列。
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ColumnMapping {
    pub source: usize,
    pub target: String,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Transaction {
    /// 整体一个事务。
    Whole,
    /// 分批提交。
    #[default]
    Batch,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OnError {
    #[default]
    Stop,
    Skip,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ImportRequest {
    pub profile_id: String,
    pub source: FileSource,
    pub file: FileOptions,
    pub schema: String,
    pub table: String,
    /// 新建表；不给时导入到已有表。
    #[serde(default)]
    pub create: Option<NewTable>,
    pub mapping: Vec<ColumnMapping>,
    pub mode: WriteMode,
    #[serde(default)]
    pub transaction: Transaction,
    #[serde(default = "default_batch")]
    pub batch_size: usize,
    #[serde(default)]
    pub on_error: OnError,
    /// 空单元格在文本列写空字符串（默认写 NULL）。
    #[serde(default)]
    pub empty_as_string: bool,
}

fn default_batch() -> usize {
    DEFAULT_BATCH
}

/// 新建表的结构模型：类型按方言转成原生类型，主键列不可为空。
pub fn new_table(dialect: &Dialect, name: &str, def: &NewTable) -> Result<Table> {
    if def.columns.is_empty() {
        return Err(Error::invalid(msg!("import.no_columns")));
    }
    for k in &def.primary_key {
        if !def.columns.iter().any(|c| &c.name == k) {
            return Err(Error::invalid(msg!("changes.unknown_column", column = k)));
        }
    }
    let columns = def
        .columns
        .iter()
        .enumerate()
        .map(|(n, c)| Column {
            name: c.name.clone(),
            ordinal: n as u32 + 1,
            data_type: ColumnType { native: dialect.native_type(&c.logical), logical: c.logical.clone() },
            nullable: c.nullable && !def.primary_key.contains(&c.name),
            default: None,
            comment: None,
            auto_increment: false,
            generated: None,
        })
        .collect();
    Ok(Table {
        name: name.to_string(),
        columns,
        primary_key: (!def.primary_key.is_empty())
            .then(|| PrimaryKey { name: None, columns: def.primary_key.clone() }),
        ..Default::default()
    })
}

fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64)
}

/// 执行一条语句，返回影响的行数（结果集读完丢弃）。
pub(crate) async fn exec(conn: &mut Box<dyn Connection>, sql: &str, params: &[Value]) -> Result<u64> {
    let mut stream = conn.execute(sql, params).await?;
    let mut affected = 0;
    while let Some(ev) = stream.next(100).await? {
        if let ExecEvent::Affected(n) = ev {
            affected += n;
        }
    }
    Ok(affected)
}

/// 读文件的线程交来的一批行。
struct Batch {
    rows: Vec<FileRow>,
    read: (u64, u64),
    warnings: u64,
}

/// 在阻塞线程中读文件，按批交给任务；任务丢掉接收端时停止。
fn spawn_reader(mut reader: FileReader, size: usize) -> mpsc::Receiver<Result<Batch>> {
    let (tx, rx) = mpsc::channel(2);
    tokio::task::spawn_blocking(move || {
        loop {
            let mut rows = Vec::with_capacity(size);
            let result = loop {
                match reader.next_row() {
                    Ok(Some(r)) => {
                        rows.push(r);
                        if rows.len() == size {
                            break Ok(());
                        }
                    }
                    Ok(None) => break Ok(()),
                    Err(e) => break Err(e),
                }
            };
            let w = reader.warnings();
            let last = rows.len() < size;
            let msg = result.map(|()| Batch {
                rows,
                read: reader.progress(),
                warnings: w.ragged_rows + w.bad_encoding + w.error_cells,
            });
            let failed = msg.is_err();
            let empty = msg.as_ref().is_ok_and(|b| b.rows.is_empty());
            if (!empty && tx.blocking_send(msg).is_err()) || failed || last {
                break;
            }
        }
    });
    rx
}

/// 出错报告：UTF-8 带 BOM 的 CSV，列为行号、错误、文件的原始各列。
struct Report {
    out: csv::Writer<std::fs::File>,
    rows: u64,
}

impl Report {
    fn create(path: &std::path::Path, columns: &[String]) -> Result<Self> {
        use std::io::Write;
        let io = |e: std::io::Error| Error::internal(msg!("export.write_failed", error = e));
        let mut f = std::fs::File::create(path).map_err(io)?;
        f.write_all("\u{feff}".as_bytes()).map_err(io)?;
        let mut out = csv::Writer::from_writer(f);
        let mut head = vec![msg!("import.report_line").current(), msg!("import.report_error").current()];
        head.extend(columns.iter().cloned());
        out.write_record(&head).map_err(|e| Error::internal(msg!("export.write_failed", error = e)))?;
        Ok(Self { out, rows: 0 })
    }

    fn write(&mut self, row: &FileRow, error: &Error) -> Result<()> {
        let mut rec = vec![row.line.to_string(), error.to_string()];
        rec.extend(row.values.iter().map(text));
        self.out.write_record(&rec).map_err(|e| Error::internal(msg!("export.write_failed", error = e)))?;
        self.rows += 1;
        Ok(())
    }
}

/// 一次导入的固定参数。
struct Plan {
    profile: Profile,
    dialect: Dialect,
    schema: String,
    table: Table,
    /// 新建表时的建表语句。
    create_sql: Option<String>,
    /// 映射：文件列下标、表列。
    sources: Vec<usize>,
    columns: Vec<String>,
    mode: WriteMode,
    transaction: Transaction,
    batch: usize,
    on_error: OnError,
    empty: EmptyAs,
    file_columns: Vec<String>,
    report_name: String,
}

impl Plan {
    /// 一行转换成要写入的值。
    fn values(&self, row: &FileRow) -> Result<Vec<Value>> {
        self.sources
            .iter()
            .zip(&self.columns)
            .map(|(&k, name)| {
                let col = self.table.columns.iter().find(|c| &c.name == name).expect("映射已检查");
                convert(row.values.get(k).unwrap_or(&Value::Null), col, self.empty)
            })
            .collect()
    }

    fn statements(&self, rows: &[Vec<Value>]) -> Result<Vec<ImportStatement>> {
        self.dialect.import_statements(&self.schema, &self.table, &self.columns, rows, self.mode)
    }
}

impl Service {
    /// 新建表的建表语句预览。
    pub async fn import_ddl(
        &self,
        profile_id: &str,
        schema: &str,
        table: &str,
        def: &NewTable,
    ) -> Result<String> {
        let profile = self.profile(profile_id)?;
        let dialect = self.metadata.dialect(&profile).await?;
        Ok(dialect.create_table_sql(schema, &new_table(&dialect, table, def)?))
    }

    /// 预览文件。`allow_path` 为 false（浏览器模式）时不接受本机路径。
    pub async fn preview_import(
        &self,
        source: &FileSource,
        opts: &FileOptions,
        allow_path: bool,
    ) -> Result<super::FilePreview> {
        let file = self.tasks.resolve(source, allow_path, false)?;
        let opts = opts.clone();
        tokio::task::spawn_blocking(move || super::preview_file(&file.path, &file.name, &opts))
            .await
            .map_err(|e| Error::internal(msg!("import.read_failed", error = e)))?
    }

    /// 提交导入任务，返回任务 id。提交时检查连接、文件、目标表、映射和执行策略；执行在后台进行。
    pub async fn start_import(self: &Arc<Self>, req: ImportRequest, allow_path: bool) -> Result<String> {
        let profile = self.profile(&req.profile_id)?;
        let password = self.secrets.get(&profile.id)?.unwrap_or_default();
        let dialect = self.metadata.dialect(&profile).await?;
        let (table, create_sql) = match &req.create {
            Some(def) => {
                let t = new_table(&dialect, &req.table, def)?;
                let sql = dialect.create_table_sql(&req.schema, &t);
                (t, Some(sql))
            }
            None => {
                let t =
                    self.metadata.tables(&profile, &req.schema, Some(&req.table)).await?.into_iter().next();
                let t = t.ok_or_else(|| {
                    Error::not_found(msg!(
                        "noun.object",
                        kind = "table",
                        name = format!("{}.{}", req.schema, req.table)
                    ))
                })?;
                (t, None)
            }
        };
        if req.create.is_some() && req.mode != WriteMode::Append {
            return Err(Error::invalid(msg!("import.new_table_append")));
        }
        let columns: Vec<String> = req.mapping.iter().map(|m| m.target.clone()).collect();
        let sources: Vec<usize> = req.mapping.iter().map(|m| m.source).collect();
        if columns.iter().enumerate().any(|(k, c)| columns[..k].contains(c)) {
            return Err(Error::invalid(msg!("import.duplicate_target")));
        }
        // 列、生成列、按主键更新的条件：用空行检查一次
        dialect.import_statements(&req.schema, &table, &columns, &[], req.mode)?;
        // 执行策略：拒绝时不开始；需要确认的由界面在开始前确认
        let ctx =
            ExecContext { profile_id: profile.id.clone(), environment: profile.input.environment, dialect };
        let policy = self.sessions.policy();
        let mut checks =
            vec![format!("INSERT INTO {} VALUES (1)", dialect.qualified(Some(&req.schema), &req.table))];
        checks.extend(create_sql.clone());
        if req.mode == WriteMode::Replace {
            checks.push(dialect.delete_all_sql(&req.schema, &req.table));
        }
        for sql in &checks {
            if let Decision::Deny { reason } = policy.before_execute(&ctx, &classify(&dialect, sql)).await {
                return Err(Error::Denied { reason });
            }
        }
        let file = self.tasks.resolve(&req.source, allow_path, false)?;
        let opened = {
            let (path, name, opts) = (file.path.clone(), file.name.clone(), req.file.clone());
            tokio::task::spawn_blocking(move || FileReader::open(&path, &name, &opts))
                .await
                .map_err(|e| Error::internal(msg!("import.read_failed", error = e)))??
        };
        if let Some(bad) = sources.iter().find(|&&k| k >= opened.columns().len()) {
            return Err(Error::invalid(msg!("import.bad_source_column", n = bad + 1)));
        }
        let task = self.tasks.create(TaskKind::Import)?;
        // 文件交给任务：任务结束时删除上传的文件
        let file = self.tasks.resolve(&req.source, allow_path, true)?;
        let stem = file.name.rsplit_once('.').map_or(file.name.as_str(), |(s, _)| s).to_string();
        let plan = Plan {
            profile,
            dialect,
            schema: req.schema.clone(),
            table,
            create_sql,
            sources,
            columns,
            // 清空后导入固定为整体一个事务
            transaction: if req.mode == WriteMode::Replace { Transaction::Whole } else { req.transaction },
            mode: req.mode,
            batch: req.batch_size.clamp(*BATCH_RANGE.start(), *BATCH_RANGE.end()),
            on_error: req.on_error,
            empty: if req.empty_as_string { EmptyAs::EmptyString } else { EmptyAs::Null },
            file_columns: opened.columns().to_vec(),
            report_name: format!("{stem}-errors.csv"),
        };
        let id = task_id(&task);
        let service = self.clone();
        tokio::spawn(async move {
            let at = now_ms();
            let started = Instant::now();
            let result = service.run_import(&task, &plan, opened, password).await;
            let written = {
                let mut n = 0;
                task.update(|s| n = s.progress.rows_written);
                n
            };
            service.history.record(NewHistory {
                at,
                profile_id: plan.profile.id.clone(),
                profile_name: plan.profile.input.name.clone(),
                driver: driver_name(plan.profile.input.driver).to_string(),
                variant: Some(variant_code(plan.dialect.variant())),
                schema: Some(plan.schema.clone()),
                sql: format!(
                    "-- {}",
                    msg!("import.history", file = file.name, table = plan.table.name).current()
                ),
                elapsed_ms: Some(started.elapsed().as_millis() as u64),
                rows: Some(written),
                error: result.as_ref().err().map(ToString::to_string),
            });
            if plan.create_sql.is_some() {
                service.metadata.cache().invalidate(&plan.profile.id, Some(&plan.schema));
            }
            file.release();
            task.finish(result);
        });
        Ok(id)
    }

    async fn run_import(
        &self,
        task: &TaskHandle,
        plan: &Plan,
        reader: FileReader,
        password: String,
    ) -> Result<()> {
        let (mut conn, _) = self.registry.connect(&plan.profile, password).await?;
        task.set_cancel(Some(conn.cancel_handle()))?;
        let mut created = false;
        let outcome = async {
            if let Some(sql) = &plan.create_sql {
                task.update(|s| s.phase = TaskPhase::Creating);
                exec(&mut conn, sql, &[]).await?;
                created = true;
            }
            conn.set_autocommit(false).await?;
            let result = self.write_rows(task, plan, &mut conn, reader).await;
            if result.is_err() {
                let _ = conn.rollback().await;
            }
            let restore = conn.set_autocommit(true).await;
            result.and(restore)
        }
        .await;
        // 整体一个事务失败或取消：删掉本次新建的表，相当于什么都没发生
        if outcome.is_err() && created && plan.transaction == Transaction::Whole {
            let drop = format!("DROP TABLE {}", plan.dialect.qualified(Some(&plan.schema), &plan.table.name));
            let _ = exec(&mut conn, &drop, &[]).await;
        }
        let _ = task.set_cancel(None);
        let _ = conn.close().await;
        if task.cancelled() { Err(Error::Cancelled) } else { outcome }
    }

    async fn write_rows(
        &self,
        task: &TaskHandle,
        plan: &Plan,
        conn: &mut Box<dyn Connection>,
        reader: FileReader,
    ) -> Result<()> {
        if plan.mode == WriteMode::Replace {
            task.update(|s| s.phase = TaskPhase::Clearing);
            exec(conn, &plan.dialect.delete_all_sql(&plan.schema, &plan.table.name), &[]).await?;
        }
        task.update(|s| s.phase = TaskPhase::Writing);
        let whole = plan.transaction == Transaction::Whole;
        let skip = !whole && plan.on_error == OnError::Skip;
        let mut report = None;
        if skip {
            let path = task.report_path("csv", plan.report_name.clone())?;
            report = Some((Report::create(&path, &plan.file_columns)?, path));
        }
        let mut rx = spawn_reader(reader, plan.batch);
        while let Some(batch) = rx.recv().await {
            if task.cancelled() {
                return Err(Error::Cancelled);
            }
            let batch = batch?;
            task.update(|s| {
                s.progress.read = batch.read.0;
                s.progress.total = batch.read.1;
                s.progress.rows_read += batch.rows.len() as u64;
                s.warnings = batch.warnings;
            });
            // 先转换：失败的行在跳过模式下直接写入报告，否则停止
            let mut good: Vec<(&FileRow, Vec<Value>)> = Vec::with_capacity(batch.rows.len());
            for row in &batch.rows {
                match plan.values(row) {
                    Ok(v) => good.push((row, v)),
                    Err(e) if skip => skip_row(task, &mut report, row, &e)?,
                    Err(e) => {
                        task.update(|s| s.error_line = Some(row.line));
                        return Err(e);
                    }
                }
            }
            let values: Vec<Vec<Value>> = good.iter().map(|(_, v)| v.clone()).collect();
            let mut failed = None;
            for st in plan.statements(&values)? {
                if let Err(e) = exec(conn, &st.sql, &st.params).await {
                    failed = Some((st.rows.start, e));
                    break;
                }
            }
            match failed {
                None => {
                    if !whole {
                        conn.commit().await?;
                    }
                    let last = good.last().map(|(r, _)| r.line);
                    task.update(|s| {
                        s.progress.rows_written += good.len() as u64;
                        if !whole && last.is_some() {
                            s.committed_line = last;
                        }
                    });
                }
                Some((first, e)) if !skip => {
                    task.update(|s| s.error_line = Some(good[first].0.line));
                    return Err(e);
                }
                Some(_) => {
                    // 跳过模式：回滚这一批，逐行重试，每行单独提交
                    conn.rollback().await?;
                    for (row, v) in &good {
                        if task.cancelled() {
                            return Err(Error::Cancelled);
                        }
                        let mut result = Ok(());
                        for st in plan.statements(std::slice::from_ref(v))? {
                            result = exec(conn, &st.sql, &st.params).await.map(|_| ());
                            if result.is_err() {
                                break;
                            }
                        }
                        match result {
                            Ok(()) => {
                                conn.commit().await?;
                                task.update(|s| {
                                    s.progress.rows_written += 1;
                                    s.committed_line = Some(row.line);
                                });
                            }
                            Err(e) => {
                                conn.rollback().await?;
                                if matches!(e, Error::Cancelled | Error::Connect { .. }) {
                                    return Err(e);
                                }
                                skip_row(task, &mut report, row, &e)?;
                            }
                        }
                    }
                }
            }
        }
        if whole {
            conn.commit().await?;
        }
        // 没有跳过的行时不留报告
        if let Some((mut r, path)) = report {
            r.out.flush().map_err(|e| Error::internal(msg!("export.write_failed", error = e)))?;
            if r.rows == 0 {
                drop(r);
                let _ = std::fs::remove_file(path);
                task.update(|s| s.report = None);
            }
        }
        Ok(())
    }
}

fn skip_row(
    task: &TaskHandle,
    report: &mut Option<(Report, std::path::PathBuf)>,
    row: &FileRow,
    e: &Error,
) -> Result<()> {
    if let Some((r, _)) = report {
        r.write(row, e)?;
    }
    task.update(|s| s.progress.rows_skipped += 1);
    task.fail(TaskFailure { index: 0, line: row.line, error: e.clone(), text: String::new() }, String::new());
    Ok(())
}

fn task_id(task: &TaskHandle) -> String {
    let mut id = String::new();
    task.update(|s| id = s.id.clone());
    id
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use crate::fake::{FakeDriver, Log};
    use crate::history::{HistoryLog, HistoryQuery};
    use crate::policy::DefaultPolicy;
    use crate::profile::tests::input;
    use crate::registry::DriverRegistry;
    use crate::secret::{SecretBackend, Secrets};
    use crate::store::Store;
    use crate::task::{TaskState, TaskStatus};

    use super::*;

    /// 假驱动的服务（历史直接写入）和一个保存了密码的连接。
    fn service() -> (Arc<Service>, String, Log, tempfile::TempDir) {
        let store = Arc::new(Store::open_in_memory().unwrap());
        let secrets = Arc::new(Secrets::with_backend(store.clone(), SecretBackend::Vault));
        secrets.init_vault("master").unwrap();
        let log: Log = Arc::default();
        let registry = Arc::new(DriverRegistry::with_drivers(
            store.clone(),
            vec![Box::new(FakeDriver { log: log.clone() })],
        ));
        let history = HistoryLog::inline(store.clone());
        let s = Service::with_history(store, secrets, registry, Arc::new(DefaultPolicy), history);
        let dir = tempfile::tempdir().unwrap();
        s.tasks.set_dirs(dir.path().join("tasks"), dir.path().join("imports"));
        let p = s.create_profile(input("pg"), Some("pw")).unwrap();
        (Arc::new(s), p.id, log, dir)
    }

    /// 上传一个 CSV：列 id、name，`n` 行，第 `bad` 行（数据行序号，从 1 开始）的 name 为 `FAIL`。
    fn upload(s: &Service, n: usize, bad: &[usize]) -> String {
        let mut text = String::from("id,name\n");
        for k in 1..=n {
            let name = if bad.contains(&k) { "FAIL".to_string() } else { format!("名{k}") };
            text += &format!("{k},{name}\n");
        }
        let (id, path) = s.tasks.begin_upload().unwrap();
        std::fs::write(&path, &text).unwrap();
        s.tasks.finish_upload(&id, "数据.csv", text.len() as u64).unwrap();
        id
    }

    /// 导入到新表 t(id BIGINT 主键, name VARCHAR(20))。
    fn request(profile_id: &str, upload: &str) -> ImportRequest {
        ImportRequest {
            profile_id: profile_id.into(),
            source: FileSource::Upload { upload: upload.into() },
            file: FileOptions::csv(),
            schema: "public".into(),
            table: "t".into(),
            create: Some(NewTable {
                columns: vec![
                    NewColumn { name: "id".into(), logical: LogicalType::Int { bits: 64 }, nullable: false },
                    NewColumn {
                        name: "name".into(),
                        logical: LogicalType::VarChar { len: Some(20) },
                        nullable: true,
                    },
                ],
                primary_key: vec!["id".into()],
            }),
            mapping: vec![
                ColumnMapping { source: 0, target: "id".into() },
                ColumnMapping { source: 1, target: "name".into() },
            ],
            mode: WriteMode::Append,
            transaction: Transaction::Whole,
            batch_size: DEFAULT_BATCH,
            on_error: OnError::Stop,
            empty_as_string: false,
        }
    }

    async fn wait(s: &Service, id: &str) -> TaskStatus {
        for _ in 0..1000 {
            let st = s.tasks.status(id).unwrap();
            if !matches!(st.state, TaskState::Running | TaskState::Confirm) {
                return st;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("任务没有结束");
    }

    fn executed(log: &Log) -> Vec<String> {
        log.lock().unwrap().iter().filter(|l| !l.starts_with("lob_limit")).cloned().collect()
    }

    #[tokio::test]
    async fn whole_transaction_creates_table_and_writes_in_statements_of_500() {
        let (s, p, log, dir) = service();
        let up = upload(&s, 1200, &[]);
        let id = s.start_import(request(&p, &up), false).await.unwrap();
        let st = wait(&s, &id).await;
        assert_eq!(
            (st.state, st.progress.rows_read, st.progress.rows_written),
            (TaskState::Done, 1200, 1200),
            "{st:?}"
        );
        let log = executed(&log);
        let pos = |prefix: &str| {
            log.iter().position(|l| l.starts_with(prefix)).unwrap_or_else(|| panic!("{prefix}：{log:?}"))
        };
        assert!(pos("execute CREATE TABLE public.t") < pos("autocommit false"));
        // 1200 行：500 + 500 + 200，一次提交
        assert_eq!(
            log.iter().filter(|l| l.starts_with("execute INSERT INTO public.t (id, name) VALUES")).count(),
            3
        );
        assert_eq!(log.iter().filter(|l| *l == "commit").count(), 1);
        assert!(pos("commit") < pos("autocommit true"));
        assert!(st.report.is_none());
        let h = s.store.history(&HistoryQuery::default()).unwrap();
        assert_eq!(h[0].entry.rows, Some(1200));
        assert!(h[0].entry.sql.starts_with("-- "), "{}", h[0].entry.sql);
        // 上传的文件交给任务后删除
        assert_eq!(std::fs::read_dir(dir.path().join("imports")).unwrap().count(), 0);
    }

    #[tokio::test]
    async fn whole_transaction_failure_rolls_back_and_drops_new_table() {
        let (s, p, log, _dir) = service();
        let up = upload(&s, 700, &[600]);
        let id = s.start_import(request(&p, &up), false).await.unwrap();
        let st = wait(&s, &id).await;
        assert_eq!(st.state, TaskState::Failed);
        assert!(matches!(st.error, Some(Error::Sql { .. })));
        // 出错的语句包含第 501–700 行（文件第 502–701 行），报这一组的第一行
        assert_eq!(st.error_line, Some(502));
        let log = executed(&log);
        assert!(log.contains(&"rollback".to_string()));
        assert!(log.iter().any(|l| l == "execute DROP TABLE public.t"), "{log:?}");
        assert!(!log.contains(&"commit".to_string()));
    }

    #[tokio::test]
    async fn batches_stop_on_error_and_keep_committed_rows() {
        let (s, p, log, _dir) = service();
        let up = upload(&s, 1500, &[1200]);
        let req = ImportRequest { transaction: Transaction::Batch, ..request(&p, &up) };
        let st = wait(&s, &s.start_import(req, false).await.unwrap()).await;
        assert_eq!(st.state, TaskState::Failed);
        assert_eq!((st.progress.rows_written, st.committed_line), (1000, Some(1001)));
        assert_eq!(st.error_line, Some(1002));
        let log = executed(&log);
        assert_eq!(log.iter().filter(|l| *l == "commit").count(), 1);
        assert!(!log.iter().any(|l| l.starts_with("execute DROP")), "分批模式保留新建的表");
    }

    #[tokio::test]
    async fn batches_skip_failed_rows_into_a_report() {
        let (s, p, _, _dir) = service();
        let mut text = String::from("id,name\n1,a\nx,b\n3,FAIL\n4,d\n");
        text += "";
        let (up, path) = s.tasks.begin_upload().unwrap();
        std::fs::write(&path, &text).unwrap();
        s.tasks.finish_upload(&up, "订单.csv", 0).unwrap();
        let req =
            ImportRequest { transaction: Transaction::Batch, on_error: OnError::Skip, ..request(&p, &up) };
        let st = wait(&s, &s.start_import(req, false).await.unwrap()).await;
        assert_eq!(st.state, TaskState::Done, "{st:?}");
        // 第 3 行转换失败（x 不是整数），第 4 行写入失败；其余两行写入
        assert_eq!((st.progress.rows_written, st.progress.rows_skipped), (2, 2));
        assert_eq!(st.failures.iter().map(|f| f.line).collect::<Vec<_>>(), [3, 4]);
        assert_eq!(st.report.as_deref(), Some("订单-errors.csv"));
        let (file, _) = s.tasks.report(&st.id).unwrap();
        let report = std::fs::read_to_string(file).unwrap();
        let lines: Vec<&str> = report.lines().collect();
        assert_eq!(lines.len(), 3, "{report}");
        assert!(lines[0].starts_with("\u{feff}Line,Error,id,name"), "{report}");
        assert!(lines[1].starts_with("3,") && lines[1].ends_with(",x,b"), "{report}");
        assert!(lines[2].starts_with("4,") && lines[2].ends_with(",3,FAIL"), "{report}");
    }

    #[tokio::test]
    async fn cancel_rolls_back() {
        let (s, p, log, _dir) = service();
        let mut text = String::from("id,name\n");
        text += "1,SLEEP\n";
        let (up, path) = s.tasks.begin_upload().unwrap();
        std::fs::write(&path, &text).unwrap();
        s.tasks.finish_upload(&up, "a.csv", 0).unwrap();
        let id = s.start_import(request(&p, &up), false).await.unwrap();
        tokio::time::sleep(Duration::from_millis(50)).await;
        s.tasks.cancel_or_remove(&id).await.unwrap();
        let st = wait(&s, &id).await;
        assert_eq!(st.state, TaskState::Cancelled);
        let log = executed(&log);
        assert!(log.contains(&"cancel".to_string()) && log.contains(&"rollback".to_string()), "{log:?}");
        assert!(log.iter().any(|l| l == "execute DROP TABLE public.t"));
    }

    #[tokio::test]
    async fn rejects_bad_requests() {
        let (s, p, _, dir) = service();
        let up = upload(&s, 3, &[]);
        let bad = |req: ImportRequest| {
            let s = s.clone();
            async move { s.start_import(req, false).await }
        };
        let dup = ImportRequest {
            mapping: vec![
                ColumnMapping { source: 0, target: "id".into() },
                ColumnMapping { source: 1, target: "id".into() },
            ],
            ..request(&p, &up)
        };
        assert!(matches!(bad(dup).await, Err(Error::Invalid { .. })), "同一表列映射两次");
        let far = ImportRequest {
            mapping: vec![ColumnMapping { source: 5, target: "id".into() }],
            ..request(&p, &up)
        };
        assert!(matches!(bad(far).await, Err(Error::Invalid { .. })), "文件没有第 6 列");
        let upsert = ImportRequest { mode: WriteMode::Upsert, ..request(&p, &up) };
        assert!(matches!(bad(upsert).await, Err(Error::Invalid { .. })), "新建表只能追加");
        let local =
            ImportRequest { source: FileSource::Path { path: dir.path().join("a.csv") }, ..request(&p, &up) };
        assert!(matches!(bad(local).await, Err(Error::Invalid { .. })), "浏览器模式不接受路径");
        // 检查失败时上传的文件还在，可以改了再提交
        assert!(s.start_import(request(&p, &up), false).await.is_ok());
    }

    #[tokio::test]
    async fn ddl_and_preview() {
        let (s, p, _, _dir) = service();
        let def = request(&p, "x").create.unwrap();
        assert_eq!(
            s.import_ddl(&p, "public", "导入", &def).await.unwrap(),
            "CREATE TABLE public.\"导入\" (\n    id BIGINT NOT NULL,\n    name VARCHAR(20),\n    PRIMARY KEY (id)\n)"
        );
        let up = upload(&s, 3, &[]);
        let pv =
            s.preview_import(&FileSource::Upload { upload: up }, &FileOptions::csv(), false).await.unwrap();
        assert_eq!((pv.columns.len(), pv.rows.len()), (2, 3));
        assert_eq!(pv.types[0], LogicalType::Int { bits: 64 });
    }
}
