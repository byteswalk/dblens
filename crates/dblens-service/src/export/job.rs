//! 导出任务（M1c3 设计文档 3.1、3.2 节）：后台另开一个连接，重新执行查询，边读边写文件；前端轮询进度。
//! 桌面版直接写到用户选的路径；浏览器模式写到临时目录，完成后下载。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use dblens_core::{Error, Result, msg};
use dblens_driver::{CancelHandle, ExecEvent};
use serde::{Deserialize, Serialize};

use super::write::{ExportFormat, ExportOptions, ExportWarnings, writer};
use crate::history::NewHistory;
use crate::objects::DataOrder;
use crate::policy::{StatementKind, classify};
use crate::profile::{driver_name, variant_code};
use crate::service::Service;

/// 同时进行的导出任务上限。
pub const MAX_RUNNING_EXPORTS: usize = 4;
/// 导出时每个大字段最多读取的字节数。
pub const EXPORT_LOB_LIMIT: usize = 256 * 1024 * 1024;
/// 每批读取的行数。
const BATCH: usize = 1000;
/// 已结束的任务保留多久。
const KEEP: Duration = Duration::from_secs(3600);

/// 导出什么：一条查询语句，或数据标签的对象（由 `data_sql` 生成 SELECT）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExportSource {
    Sql(String),
    Data {
        schema: String,
        object: String,
        #[serde(default)]
        filter: Option<String>,
        #[serde(default)]
        order: Option<DataOrder>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ExportRequest {
    pub profile_id: String,
    pub source: ExportSource,
    pub format: ExportFormat,
    #[serde(default)]
    pub options: ExportOptions,
    /// 桌面版由"另存为"对话框选出的绝对路径；不给时写到临时目录，完成后下载。
    #[serde(default)]
    pub path: Option<PathBuf>,
    /// 下载时的文件名。
    pub file_name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExportState {
    Running,
    Done,
    Failed,
    Cancelled,
}

/// 任务的状态，`GET /api/exports/{id}` 原样返回。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExportStatus {
    pub id: String,
    pub state: ExportState,
    /// 已写出的行数。
    pub rows: u64,
    pub warnings: ExportWarnings,
    /// 失败的原因；序列化时按请求的语言渲染。
    pub error: Option<Error>,
    pub file_name: String,
    /// 写到用户指定的路径时为该路径；临时文件不返回路径。
    pub path: Option<PathBuf>,
}

struct Job {
    status: ExportStatus,
    rows: Arc<AtomicU64>,
    cancelled: Arc<AtomicBool>,
    cancel: Arc<Mutex<Option<CancelHandle>>>,
    /// 实际写入的文件。
    file: PathBuf,
    temp: bool,
    finished: Option<Instant>,
}

/// 全部导出任务。
pub struct ExportManager {
    dir: Mutex<PathBuf>,
    jobs: Mutex<HashMap<String, Job>>,
}

fn lock_error() -> Error {
    Error::internal(msg!("internal.lock_poisoned", lock = "exports"))
}

fn not_found(id: &str) -> Error {
    Error::not_found(msg!("export.not_found", id = id))
}

impl Default for ExportManager {
    fn default() -> Self {
        Self::new(std::env::temp_dir().join("dblens-exports"))
    }
}

impl ExportManager {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir: Mutex::new(dir), jobs: Mutex::new(HashMap::new()) }
    }

    /// 临时文件放在 `dir` 下；上次运行留下的临时文件一并删除。
    pub fn set_dir(&self, dir: PathBuf) {
        if dir.exists() {
            let _ = std::fs::remove_dir_all(&dir);
        }
        if let Ok(mut d) = self.dir.lock() {
            *d = dir;
        }
    }

    fn temp_path(&self, id: &str, format: ExportFormat) -> Result<PathBuf> {
        let dir = self.dir.lock().map_err(|_| lock_error())?.clone();
        std::fs::create_dir_all(&dir).map_err(|e| Error::internal(msg!("export.write_failed", error = e)))?;
        Ok(dir.join(format!("{id}.{}", format.extension())))
    }

    /// 删除结束超过一小时的任务和它们的临时文件。
    fn purge(jobs: &mut HashMap<String, Job>) {
        jobs.retain(|_, j| {
            let keep = j.finished.is_none_or(|t| t.elapsed() < KEEP);
            if !keep && j.temp {
                let _ = std::fs::remove_file(&j.file);
            }
            keep
        });
    }

    pub fn status(&self, id: &str) -> Result<ExportStatus> {
        let mut jobs = self.jobs.lock().map_err(|_| lock_error())?;
        Self::purge(&mut jobs);
        let j = jobs.get(id).ok_or_else(|| not_found(id))?;
        Ok(ExportStatus { rows: j.rows.load(Ordering::Relaxed), ..j.status.clone() })
    }

    /// 进行中的任务：取消（之后状态为 `cancelled`）。已结束的任务：删除记录和临时文件。
    pub async fn cancel_or_remove(&self, id: &str) -> Result<()> {
        let handle = {
            let mut jobs = self.jobs.lock().map_err(|_| lock_error())?;
            let j = jobs.get(id).ok_or_else(|| not_found(id))?;
            if j.status.state == ExportState::Running {
                j.cancelled.store(true, Ordering::SeqCst);
                j.cancel.lock().map_err(|_| lock_error())?.clone()
            } else {
                let j = jobs.remove(id).expect("刚取到");
                if j.temp {
                    let _ = std::fs::remove_file(&j.file);
                }
                return Ok(());
            }
        };
        if let Some(h) = handle {
            // 语句已经执行完、正在读取时，数据库层面的取消可能没有效果，由读取循环检查取消标志
            let _ = h.cancel().await;
        }
        Ok(())
    }

    /// 浏览器模式下载用：已完成的临时文件的路径和文件名。下载后由调用方删除文件、移除任务。
    pub fn take_file(&self, id: &str) -> Result<(PathBuf, String)> {
        let mut jobs = self.jobs.lock().map_err(|_| lock_error())?;
        let j = jobs.get(id).ok_or_else(|| not_found(id))?;
        if j.status.state != ExportState::Done || !j.temp {
            return Err(Error::invalid(msg!("export.no_file")));
        }
        let j = jobs.remove(id).expect("刚取到");
        Ok((j.file, j.status.file_name))
    }

    fn running(&self) -> Result<usize> {
        Ok(self
            .jobs
            .lock()
            .map_err(|_| lock_error())?
            .values()
            .filter(|j| j.status.state == ExportState::Running)
            .count())
    }

    fn finish(&self, id: &str, state: ExportState, warnings: ExportWarnings, error: Option<Error>) {
        if let Ok(mut jobs) = self.jobs.lock()
            && let Some(j) = jobs.get_mut(id)
        {
            j.status.state = state;
            j.status.warnings = warnings;
            j.status.error = error;
            j.finished = Some(Instant::now());
            if state != ExportState::Done {
                let _ = std::fs::remove_file(&j.file);
            }
        }
    }
}

/// 检查用户给的保存路径：必须是绝对路径，父目录必须存在。
fn check_path(path: &Path) -> Result<()> {
    let ok = path.is_absolute() && path.parent().is_some_and(Path::is_dir) && !path.is_dir();
    if ok { Ok(()) } else { Err(Error::invalid(msg!("export.bad_path", path = path.display()))) }
}

fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64)
}

impl Service {
    /// 提交导出任务，返回任务 id。`allow_path` 为 false（浏览器模式）时不接受保存路径。
    /// 提交时检查：连接配置、密码库、语句是否为单条查询、任务数、路径；执行在后台进行。
    pub async fn start_export(self: &Arc<Self>, req: ExportRequest, allow_path: bool) -> Result<String> {
        let profile = self.profile(&req.profile_id)?;
        let password = self.secrets.get(&profile.id)?.unwrap_or_default();
        let dialect = self.metadata.dialect(&profile).await?;
        let sql = match &req.source {
            ExportSource::Sql(sql) => {
                let statements = dialect.split(sql);
                let [one] = statements.as_slice() else {
                    return Err(Error::invalid(msg!("export.not_query")));
                };
                if classify(&dialect, &one.text).kind != StatementKind::Dql {
                    return Err(Error::invalid(msg!("export.not_query")));
                }
                one.text.clone()
            }
            ExportSource::Data { schema, object, filter, order } => {
                let filter = filter.as_deref().map(str::trim).filter(|f| !f.is_empty());
                self.metadata.data_sql(&profile, schema, object, filter, order.as_ref(), false).await?
            }
        };
        if req.path.is_some() && !allow_path {
            return Err(Error::invalid(msg!("export.path_not_allowed")));
        }
        if let Some(p) = &req.path {
            check_path(p)?;
        }
        if self.exports.running()? >= MAX_RUNNING_EXPORTS {
            return Err(Error::Busy { message: msg!("export.too_many", max = MAX_RUNNING_EXPORTS) });
        }
        let id = uuid::Uuid::new_v4().simple().to_string();
        let (file, temp) = match &req.path {
            Some(p) => (p.clone(), false),
            None => (self.exports.temp_path(&id, req.format)?, true),
        };
        // 先建文件：不可写时在提交时就报错
        let mut out = writer(req.format, &req.options, &file, dialect)?;
        let rows = Arc::new(AtomicU64::new(0));
        let cancelled = Arc::new(AtomicBool::new(false));
        let cancel = Arc::new(Mutex::new(None));
        let status = ExportStatus {
            id: id.clone(),
            state: ExportState::Running,
            rows: 0,
            warnings: ExportWarnings::default(),
            error: None,
            file_name: req.file_name.clone(),
            path: req.path.clone(),
        };
        self.exports.jobs.lock().map_err(|_| lock_error())?.insert(
            id.clone(),
            Job {
                status,
                rows: rows.clone(),
                cancelled: cancelled.clone(),
                cancel: cancel.clone(),
                file,
                temp,
                finished: None,
            },
        );

        let service = self.clone();
        let job = id.clone();
        tokio::spawn(async move {
            let at = now_ms();
            let started = Instant::now();
            let result = async {
                let (mut conn, _) = service.registry.connect(&profile, password).await?;
                conn.set_lob_limit(EXPORT_LOB_LIMIT);
                *cancel.lock().map_err(|_| lock_error())? = Some(conn.cancel_handle());
                let outcome = async {
                    // 取消先置标志再取句柄：句柄存好之前到达的取消在这里发现
                    if cancelled.load(Ordering::SeqCst) {
                        return Err(Error::Cancelled);
                    }
                    let mut stream = conn.execute(&sql, &[]).await?;
                    let mut begun = false;
                    // 只导出第一个结果集
                    while let Some(ev) = stream.next(BATCH).await? {
                        if cancelled.load(Ordering::SeqCst) {
                            return Err(Error::Cancelled);
                        }
                        match ev {
                            ExecEvent::ResultStart { columns } if !begun => {
                                out.begin(&columns)?;
                                begun = true;
                            }
                            ExecEvent::Rows(batch) if begun => {
                                out.rows(&batch)?;
                                rows.fetch_add(batch.len() as u64, Ordering::Relaxed);
                            }
                            ExecEvent::ResultEnd if begun => break,
                            _ => {}
                        }
                    }
                    Ok(())
                }
                .await;
                *cancel.lock().map_err(|_| lock_error())? = None;
                let _ = conn.close().await;
                outcome?;
                out.finish()
            }
            .await;
            let result = if cancelled.load(Ordering::SeqCst) { Err(Error::Cancelled) } else { result };
            let n = rows.load(Ordering::Relaxed);
            service.history.record(NewHistory {
                at,
                profile_id: profile.id.clone(),
                profile_name: profile.input.name.clone(),
                driver: driver_name(profile.input.driver).to_string(),
                variant: Some(variant_code(dialect.variant())),
                schema: None,
                sql,
                elapsed_ms: Some(started.elapsed().as_millis() as u64),
                rows: Some(n),
                error: result.as_ref().err().map(ToString::to_string),
            });
            match result {
                Ok(warnings) => service.exports.finish(&job, ExportState::Done, warnings, None),
                Err(Error::Cancelled) => {
                    service.exports.finish(&job, ExportState::Cancelled, ExportWarnings::default(), None)
                }
                Err(e) => {
                    service.exports.finish(&job, ExportState::Failed, ExportWarnings::default(), Some(e))
                }
            }
        });
        Ok(id)
    }
}

#[cfg(test)]
mod tests {
    use crate::fake::{FakeDriver, Log};
    use crate::history::{HistoryLog, HistoryQuery};
    use crate::policy::DefaultPolicy;
    use crate::profile::tests::input;
    use crate::registry::DriverRegistry;
    use crate::secret::{SecretBackend, Secrets};
    use crate::store::Store;

    use super::*;

    /// 假驱动的服务（历史直接写入，写完即可查）和一个保存了密码的连接。
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
        s.exports.set_dir(dir.path().join("exports"));
        let p = s.create_profile(input("pg"), Some("pw")).unwrap();
        (Arc::new(s), p.id, log, dir)
    }

    fn request(profile_id: &str, sql: &str, format: ExportFormat) -> ExportRequest {
        ExportRequest {
            profile_id: profile_id.into(),
            source: ExportSource::Sql(sql.into()),
            format,
            options: ExportOptions::default(),
            path: None,
            file_name: "out".into(),
        }
    }

    /// 等任务结束。
    async fn wait(s: &Service, id: &str) -> ExportStatus {
        for _ in 0..500 {
            let st = s.exports.status(id).unwrap();
            if st.state != ExportState::Running {
                return st;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("导出没有结束");
    }

    #[tokio::test]
    async fn csv_to_temp_file_then_download() {
        let (s, p, log, _dir) = service();
        let id = s.start_export(request(&p, "SELECT ROWS 2500", ExportFormat::Csv), false).await.unwrap();
        let st = wait(&s, &id).await;
        assert_eq!((st.state, st.rows, st.path.clone()), (ExportState::Done, 2500, None), "{st:?}");
        let (file, name) = s.exports.take_file(&id).unwrap();
        assert_eq!(name, "out");
        let text = std::fs::read_to_string(&file).unwrap();
        assert_eq!(text.lines().count(), 2501, "表头加 2500 行");
        assert!(text.starts_with("\u{feff}n\n1\n"));
        // 下载后任务不再存在
        assert!(matches!(s.exports.status(&id), Err(Error::NotFound { .. })));
        assert!(log.lock().unwrap().contains(&format!("lob_limit {EXPORT_LOB_LIMIT}")));
        let history = s.store.history(&HistoryQuery::default()).unwrap();
        assert_eq!((history[0].entry.sql.as_str(), history[0].entry.rows), ("SELECT ROWS 2500", Some(2500)));
    }

    #[tokio::test]
    async fn json_to_given_path() {
        let (s, p, _, dir) = service();
        let path = dir.path().join("rows.json");
        let req =
            ExportRequest { path: Some(path.clone()), ..request(&p, "SELECT ROWS 3", ExportFormat::Json) };
        let id = s.start_export(req, true).await.unwrap();
        let st = wait(&s, &id).await;
        assert_eq!((st.state, st.path.as_deref()), (ExportState::Done, Some(path.as_path())));
        let v: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(v, serde_json::json!([{"n": 1}, {"n": 2}, {"n": 3}]));
        assert!(s.exports.take_file(&id).is_err(), "用户指定的路径不经下载");
        // 删除任务记录不删除用户的文件
        s.exports.cancel_or_remove(&id).await.unwrap();
        assert!(path.exists());
    }

    #[tokio::test]
    async fn only_single_queries_and_valid_paths() {
        let (s, p, _, dir) = service();
        for sql in ["DELETE FROM t", "SELECT ROWS 1; SELECT ROWS 2", ""] {
            let r = s.start_export(request(&p, sql, ExportFormat::Csv), false).await;
            assert!(matches!(r, Err(Error::Invalid { .. })), "{sql}：{r:?}");
        }
        let with_path = |path: &str| ExportRequest {
            path: Some(PathBuf::from(path)),
            ..request(&p, "SELECT ROWS 1", ExportFormat::Csv)
        };
        let absolute = dir.path().join("a.csv");
        let r = s.start_export(with_path(absolute.to_str().unwrap()), false).await;
        assert!(matches!(r, Err(Error::Invalid { .. })), "浏览器模式不接受路径");
        for bad in ["relative.csv", dir.path().join("no/such/dir/a.csv").to_str().unwrap()] {
            assert!(
                matches!(s.start_export(with_path(bad), true).await, Err(Error::Invalid { .. })),
                "{bad}"
            );
        }
    }

    #[tokio::test]
    async fn cancel_failure_and_limit() {
        let (s, p, _, _dir) = service();
        // 取消：任务为 cancelled，临时文件删除
        let id = s.start_export(request(&p, "SELECT SLEEP", ExportFormat::Csv), false).await.unwrap();
        tokio::time::sleep(Duration::from_millis(50)).await;
        s.exports.cancel_or_remove(&id).await.unwrap();
        assert_eq!(wait(&s, &id).await.state, ExportState::Cancelled);
        // 失败：带上错误
        let id = s.start_export(request(&p, "SELECT FAIL", ExportFormat::Xlsx), false).await.unwrap();
        let st = wait(&s, &id).await;
        assert!(matches!((st.state, &st.error), (ExportState::Failed, Some(Error::Sql { .. }))), "{st:?}");
        assert!(s.exports.take_file(&id).is_err());
        // 同时进行的任务数
        let mut ids = Vec::new();
        for _ in 0..MAX_RUNNING_EXPORTS {
            ids.push(s.start_export(request(&p, "SELECT SLEEP", ExportFormat::Csv), false).await.unwrap());
        }
        let r = s.start_export(request(&p, "SELECT ROWS 1", ExportFormat::Csv), false).await;
        assert!(matches!(r, Err(Error::Busy { .. })), "{r:?}");
        for id in &ids {
            s.exports.cancel_or_remove(id).await.unwrap();
            wait(&s, id).await;
        }
        let files = std::fs::read_dir(_dir.path().join("exports")).unwrap().count();
        assert_eq!(files, 0, "取消和失败的任务不留文件");
    }
}
