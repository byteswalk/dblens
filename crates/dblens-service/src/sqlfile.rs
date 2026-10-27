//! 执行 SQL 文件（M2b 设计文档第 6 节）：先扫描一遍（拆分、按执行策略检查），需要时等用户确认，
//! 再另开连接逐条执行；出错时停止或继续，可选整体一个事务。边执行边写执行日志。

use std::collections::VecDeque;
use std::io::{Read, Write};
use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use dblens_core::{Error, Result, msg};
use dblens_dialect::{Dialect, SqlStatement, StreamSplitter};
use serde::Deserialize;
use tokio::sync::mpsc;

use crate::history::NewHistory;
use crate::import::{TextEncoding, exec, open_text};
use crate::policy::{Decision, ExecContext, classify};
use crate::profile::{Environment, Profile, driver_name, variant_code};
use crate::service::Service;
use crate::task::{
    ConfirmItem, FileSource, SourceFile, TaskConfirm, TaskFailure, TaskHandle, TaskKind, TaskPhase,
};

/// 每次读取并拆分的文本大小。
const CHUNK: usize = 1024 * 1024;
/// 确认框中列出的语句条数。
const CONFIRM_ITEMS: usize = 20;
/// 日志中语句文本的长度上限、失败列表中语句开头的长度。
const LOG_TEXT: usize = 2000;
const HEAD_TEXT: usize = 200;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct SqlFileRequest {
    pub profile_id: String,
    pub source: FileSource,
    #[serde(default)]
    pub encoding: TextEncoding,
    /// 执行前切换到的 schema；不给时用连接的默认 schema。
    #[serde(default)]
    pub schema: Option<String>,
    /// 出错时继续执行下一条（默认停止）。
    #[serde(default)]
    pub continue_on_error: bool,
    /// 整体一个事务（此时固定为出错时停止）。
    #[serde(default)]
    pub transaction: bool,
}

fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64)
}

fn head(text: &str, n: usize) -> String {
    match text.char_indices().nth(n) {
        Some((k, _)) => format!("{}…", &text[..k]),
        None => text.to_string(),
    }
}

/// 在阻塞线程中读文件、转码，按块交出文本（附带已读字节数和文件大小）；任务丢掉接收端时停止。
fn spawn_text(
    path: std::path::PathBuf,
    encoding: TextEncoding,
) -> mpsc::Receiver<Result<(String, u64, u64)>> {
    let (tx, rx) = mpsc::channel(2);
    tokio::task::spawn_blocking(move || {
        let file = match open_text(&path, encoding) {
            Ok(f) => f,
            Err(e) => {
                let _ = tx.blocking_send(Err(e));
                return;
            }
        };
        let (mut reader, bytes, total) = (file.reader, file.bytes, file.total);
        let mut pending: Vec<u8> = Vec::new();
        let mut buf = vec![0u8; CHUNK];
        loop {
            let n = match reader.read(&mut buf) {
                Ok(n) => n,
                Err(e) => {
                    let _ = tx.blocking_send(Err(Error::invalid(msg!("import.read_failed", error = e))));
                    return;
                }
            };
            pending.extend_from_slice(&buf[..n]);
            // 转码后是 UTF-8；块末尾被切开的字符留到下一块
            let valid = match std::str::from_utf8(&pending) {
                Ok(s) => s.len(),
                Err(e) => e.valid_up_to(),
            };
            let tail = pending.split_off(valid);
            let text = String::from_utf8(std::mem::replace(&mut pending, tail)).unwrap_or_default();
            let read = bytes.load(std::sync::atomic::Ordering::Relaxed).min(total);
            if (!text.is_empty() || n == 0) && tx.blocking_send(Ok((text, read, total))).is_err() {
                return;
            }
            if n == 0 {
                return;
            }
        }
    });
    rx
}

/// 文件中的语句：逐块读取、拆分，按需取下一条。
struct Statements {
    rx: mpsc::Receiver<Result<(String, u64, u64)>>,
    /// 文件读完后为 None。
    splitter: Option<StreamSplitter>,
    ready: VecDeque<SqlStatement>,
}

impl Statements {
    fn open(file: &SourceFile, encoding: TextEncoding, dialect: Dialect) -> Self {
        Self {
            rx: spawn_text(file.path.clone(), encoding),
            splitter: Some(StreamSplitter::new(dialect.family())),
            ready: VecDeque::new(),
        }
    }

    /// 下一条语句；同时更新任务的已读字节。
    async fn next(&mut self, task: &TaskHandle) -> Result<Option<SqlStatement>> {
        loop {
            if let Some(st) = self.ready.pop_front() {
                return Ok(Some(st));
            }
            let Some(splitter) = self.splitter.as_mut() else { return Ok(None) };
            if task.cancelled() {
                return Err(Error::Cancelled);
            }
            if let Some(chunk) = self.rx.recv().await {
                let (text, read, total) = chunk?;
                task.update(|s| {
                    s.progress.read = read;
                    s.progress.total = total;
                });
                if !text.is_empty() {
                    self.ready.extend(splitter.feed(&text));
                    continue;
                }
            }
            // 文件读完（空块或读取线程结束）
            if let Some(sp) = self.splitter.take() {
                self.ready.extend(sp.finish());
            }
        }
    }
}

struct Run {
    profile: Profile,
    dialect: Dialect,
    req: SqlFileRequest,
    file: SourceFile,
}

impl Service {
    /// 提交执行 SQL 文件的任务，返回任务 id。
    pub async fn start_sql_file(self: &Arc<Self>, req: SqlFileRequest, allow_path: bool) -> Result<String> {
        let profile = self.profile(&req.profile_id)?;
        let password = self.secrets.get(&profile.id)?.unwrap_or_default();
        let dialect = self.metadata.dialect(&profile).await?;
        self.tasks.resolve(&req.source, allow_path, false)?;
        let task = self.tasks.create(TaskKind::SqlFile)?;
        let file = self.tasks.resolve(&req.source, allow_path, true)?;
        let mut id = String::new();
        task.update(|s| id = s.id.clone());
        let run = Run { profile, dialect, req, file };
        let service = self.clone();
        tokio::spawn(async move {
            let at = now_ms();
            let started = Instant::now();
            let result = service.run_sql_file(&task, &run, password, started).await;
            let (mut done, mut failed) = (0, 0);
            task.update(|s| (done, failed) = (s.progress.statements_done, s.progress.statements_failed));
            service.history.record(NewHistory {
                at,
                profile_id: run.profile.id.clone(),
                profile_name: run.profile.input.name.clone(),
                driver: driver_name(run.profile.input.driver).to_string(),
                variant: Some(variant_code(run.dialect.variant())),
                schema: run.req.schema.clone(),
                sql: format!(
                    "-- {}",
                    msg!("sqlfile.history", file = run.file.name, done = done, failed = failed).current()
                ),
                elapsed_ms: Some(started.elapsed().as_millis() as u64),
                rows: None,
                error: result.as_ref().err().map(ToString::to_string),
            });
            // 文件可能建了或删了对象：清掉这个连接的元数据缓存
            service.metadata.cache().invalidate(&run.profile.id, None);
            run.file.release();
            task.finish(result);
        });
        Ok(id)
    }

    /// 扫描：统计语句数，按执行策略检查。有被拒绝的语句时报错；需要确认时等用户回答。
    async fn scan_sql_file(&self, task: &TaskHandle, run: &Run) -> Result<()> {
        task.update(|s| s.phase = TaskPhase::Scanning);
        let ctx = ExecContext {
            profile_id: run.profile.id.clone(),
            environment: run.profile.input.environment,
            dialect: run.dialect,
        };
        let policy = self.sessions.policy();
        let mut total = 0u64;
        let mut confirm: Vec<ConfirmItem> = Vec::new();
        let mut more = 0u64;
        let mut denied: Option<(u64, String, dblens_core::Msg)> = None;
        let mut statements = Statements::open(&run.file, run.req.encoding, run.dialect);
        while let Some(st) = statements.next(task).await? {
            total += 1;
            match policy.before_execute(&ctx, &classify(&run.dialect, &st.text)).await {
                Decision::Allow => {}
                Decision::Confirm { reason } if confirm.len() < CONFIRM_ITEMS => {
                    confirm.push(ConfirmItem {
                        line: st.line as u64,
                        text: head(&st.text, HEAD_TEXT),
                        reason,
                    });
                }
                Decision::Confirm { .. } => more += 1,
                Decision::Deny { reason } => {
                    denied = Some((st.line as u64, head(&st.text, HEAD_TEXT), reason));
                    break;
                }
            }
        }
        task.update(|s| s.progress.statements_total = total);
        if let Some((line, text, reason)) = denied {
            task.update(|s| s.error_line = Some(line));
            return Err(Error::invalid(msg!("sqlfile.denied", line = line, text = text, reason = reason)));
        }
        if (!confirm.is_empty() || run.profile.input.environment == Environment::Prod)
            && !task.ask(TaskConfirm { items: confirm, more }).await
        {
            return Err(Error::Cancelled);
        }
        Ok(())
    }

    async fn run_sql_file(
        &self,
        task: &TaskHandle,
        run: &Run,
        password: String,
        started: Instant,
    ) -> Result<()> {
        self.scan_sql_file(task, run).await?;
        task.update(|s| s.phase = TaskPhase::Executing);
        let stem = run.file.name.rsplit_once('.').map_or(run.file.name.as_str(), |(s, _)| s);
        let path = task.report_path("txt", format!("{stem}-log.txt"))?;
        let io = |e: std::io::Error| Error::internal(msg!("export.write_failed", error = e));
        let mut log = std::io::BufWriter::new(std::fs::File::create(&path).map_err(io)?);
        writeln!(
            log,
            "{}",
            msg!(
                "sqlfile.log_head",
                connection = run.profile.input.name,
                schema = run.req.schema.as_deref().unwrap_or("-"),
                file = run.file.name,
                at = chrono::Local::now().format("%Y-%m-%d %H:%M:%S")
            )
            .current()
        )
        .map_err(io)?;

        let (mut conn, _) = self.registry.connect(&run.profile, password).await?;
        task.set_cancel(Some(conn.cancel_handle()))?;
        let whole = run.req.transaction;
        let stop = whole || !run.req.continue_on_error;
        let outcome: Result<()> = async {
            if let Some(schema) = &run.req.schema {
                exec(&mut conn, &run.dialect.set_schema_sql(schema), &[]).await?;
            }
            if whole {
                conn.set_autocommit(false).await?;
            }
            let mut index = 0u64;
            let mut first_error: Option<Error> = None;
            let mut statements = Statements::open(&run.file, run.req.encoding, run.dialect);
            while let Some(st) = statements.next(task).await? {
                let n = index;
                index += 1;
                match exec(&mut conn, &st.text, &[]).await {
                    Ok(affected) => task.update(|s| {
                        s.progress.statements_done += 1;
                        s.progress.affected += affected;
                    }),
                    Err(e) => {
                        if task.cancelled() {
                            return Err(Error::Cancelled);
                        }
                        let line = st.line as u64;
                        writeln!(
                            log,
                            "
{}
{}
{}",
                            msg!("sqlfile.log_failed", n = n + 1, line = line).current(),
                            e,
                            head(&st.text, LOG_TEXT)
                        )
                        .map_err(io)?;
                        task.update(|s| {
                            s.progress.statements_done += 1;
                            s.progress.statements_failed += 1;
                        });
                        let failure =
                            TaskFailure { index: n, line, error: e.clone(), text: head(&st.text, HEAD_TEXT) };
                        task.fail(failure, st.text);
                        if stop {
                            task.update(|s| s.error_line = Some(line));
                            first_error = Some(e);
                            break;
                        }
                    }
                }
            }
            match first_error {
                Some(e) => Err(e),
                None => {
                    if whole {
                        conn.commit().await?;
                    }
                    Ok(())
                }
            }
        }
        .await;
        if whole {
            if outcome.is_err() {
                let _ = conn.rollback().await;
            }
            let _ = conn.set_autocommit(true).await;
        }
        let _ = task.set_cancel(None);
        let _ = conn.close().await;
        let outcome = if task.cancelled() { Err(Error::Cancelled) } else { outcome };
        let (mut done, mut failed, mut affected) = (0, 0, 0);
        task.update(|s| {
            (done, failed, affected) =
                (s.progress.statements_done, s.progress.statements_failed, s.progress.affected)
        });
        writeln!(
            log,
            "\n{}",
            msg!(
                "sqlfile.log_summary",
                ok = done - failed,
                failed = failed,
                affected = affected,
                seconds = format!("{:.1}", started.elapsed().as_secs_f64())
            )
            .current()
        )
        .map_err(io)?;
        if let Err(e) = &outcome {
            writeln!(log, "{e}").map_err(io)?;
        }
        log.flush().map_err(io)?;
        outcome
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use crate::fake::{FakeDriver, Log};
    use crate::history::{HistoryLog, HistoryQuery};
    use crate::policy::DefaultPolicy;
    use crate::profile::ProfileInput;
    use crate::profile::tests::input;
    use crate::registry::DriverRegistry;
    use crate::secret::{SecretBackend, Secrets};
    use crate::store::Store;
    use crate::task::{TaskState, TaskStatus};

    use super::*;

    /// 假驱动的服务，以及开发环境、生产环境各一个连接。
    fn service() -> (Arc<Service>, String, String, Log, tempfile::TempDir) {
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
        let prod = s.create_profile(input("prod"), Some("pw")).unwrap();
        (Arc::new(s), dev.id, prod.id, log, dir)
    }

    fn upload(s: &Service, sql: &str) -> FileSource {
        let (id, path) = s.tasks.begin_upload().unwrap();
        std::fs::write(&path, sql).unwrap();
        s.tasks.finish_upload(&id, "脚本.sql", sql.len() as u64).unwrap();
        FileSource::Upload { upload: id }
    }

    fn request(profile_id: &str, source: FileSource) -> SqlFileRequest {
        SqlFileRequest {
            profile_id: profile_id.into(),
            source,
            encoding: TextEncoding::Auto,
            schema: None,
            continue_on_error: false,
            transaction: false,
        }
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

    const SCRIPT: &str =
        "CREATE TABLE a (x int);\nINSERT INTO a VALUES (1);\nFAIL;\n-- 注释\nINSERT INTO a VALUES (2);\n";

    #[tokio::test]
    async fn continue_on_error_runs_everything_and_logs_failures() {
        let (s, dev, _, log, _dir) = service();
        let req = SqlFileRequest {
            continue_on_error: true,
            schema: Some("sales".into()),
            ..request(&dev, upload(&s, SCRIPT))
        };
        let id = s.start_sql_file(req, false).await.unwrap();
        let st = wait(&s, &id).await;
        assert_eq!(st.state, TaskState::Done, "{st:?}");
        let p = &st.progress;
        assert_eq!((p.statements_total, p.statements_done, p.statements_failed, p.affected), (4, 4, 1, 3));
        assert_eq!((st.failures[0].index, st.failures[0].line), (2, 3));
        assert_eq!(s.tasks.statement(&id, 0).unwrap(), "FAIL");
        assert_eq!(executed(&log)[0], "SET search_path TO \"sales\"");
        let (file, name) = s.tasks.report(&id).unwrap();
        assert_eq!(name, "脚本-log.txt");
        let text = std::fs::read_to_string(file).unwrap();
        assert!(
            text.contains("Statement 3 failed (line 3)") && text.contains("3 succeeded, 1 failed"),
            "{text}"
        );
        let h = s.store.history(&HistoryQuery::default()).unwrap();
        assert!(h[0].entry.sql.contains("4 statements, 1 failed"), "{}", h[0].entry.sql);
    }

    #[tokio::test]
    async fn stop_on_error_and_whole_transaction() {
        let (s, dev, _, log, _dir) = service();
        let st = wait(&s, &s.start_sql_file(request(&dev, upload(&s, SCRIPT)), false).await.unwrap()).await;
        assert_eq!((st.state, st.error_line, st.progress.statements_done), (TaskState::Failed, Some(3), 3));
        assert!(matches!(st.error, Some(Error::Sql { .. })));
        assert!(!executed(&log).iter().any(|l| l.ends_with("VALUES (2)")), "出错后停止");
        // 整体一个事务：出错回滚；成功时提交
        log.lock().unwrap().clear();
        let req = SqlFileRequest {
            transaction: true,
            continue_on_error: true,
            ..request(&dev, upload(&s, SCRIPT))
        };
        let st = wait(&s, &s.start_sql_file(req, false).await.unwrap()).await;
        assert_eq!(st.state, TaskState::Failed, "整体事务固定为出错时停止");
        let l = log.lock().unwrap().clone();
        assert!(
            l.contains(&"autocommit false".into())
                && l.contains(&"rollback".into())
                && !l.contains(&"commit".into())
        );
        log.lock().unwrap().clear();
        let req = SqlFileRequest { transaction: true, ..request(&dev, upload(&s, "SELECT 1;\nSELECT 2")) };
        let st = wait(&s, &s.start_sql_file(req, false).await.unwrap()).await;
        assert_eq!(st.state, TaskState::Done);
        assert!(log.lock().unwrap().contains(&"commit".into()));
    }

    #[tokio::test]
    async fn production_asks_before_running() {
        let (s, _, prod, log, _dir) = service();
        let script = "INSERT INTO a VALUES (1);\nDROP TABLE a;\nDELETE FROM a;\n";
        let id = s.start_sql_file(request(&prod, upload(&s, script)), false).await.unwrap();
        let st = wait(&s, &id).await;
        assert_eq!(st.state, TaskState::Confirm);
        let c = st.confirm.unwrap();
        assert_eq!(c.items.iter().map(|i| i.line).collect::<Vec<_>>(), [2, 3]);
        assert_eq!(st.progress.statements_total, 3);
        assert!(executed(&log).is_empty(), "确认前不执行");
        s.tasks.confirm(&id, true).unwrap();
        assert_eq!(wait(&s, &id).await.state, TaskState::Done);
        assert_eq!(executed(&log).len(), 3);
        // 回答"否"：不执行任何语句
        log.lock().unwrap().clear();
        let id = s.start_sql_file(request(&prod, upload(&s, "SELECT 1")), false).await.unwrap();
        assert_eq!(wait(&s, &id).await.state, TaskState::Confirm, "生产环境总要确认");
        s.tasks.confirm(&id, false).unwrap();
        assert_eq!(wait(&s, &id).await.state, TaskState::Cancelled);
        assert!(executed(&log).is_empty());
    }

    #[tokio::test]
    async fn large_file_spans_chunks_and_cancel_works() {
        let (s, dev, _, log, _dir) = service();
        let script: String = (0..60_000).map(|n| format!("INSERT INTO t VALUES ({n}, '数{n}');\n")).collect();
        assert!(script.len() > 2 * CHUNK);
        let st = wait(&s, &s.start_sql_file(request(&dev, upload(&s, &script)), false).await.unwrap()).await;
        assert_eq!((st.state, st.progress.statements_done), (TaskState::Done, 60_000));
        assert_eq!(st.progress.read, st.progress.total);
        assert_eq!(executed(&log).last().unwrap(), "INSERT INTO t VALUES (59999, '数59999')");
        let id =
            s.start_sql_file(request(&dev, upload(&s, "SELECT 1; SLEEP; SELECT 2")), false).await.unwrap();
        tokio::time::sleep(Duration::from_millis(50)).await;
        s.tasks.cancel_or_remove(&id).await.unwrap();
        let st = wait(&s, &id).await;
        assert_eq!((st.state, st.progress.statements_done), (TaskState::Cancelled, 1));
    }
}
