//! 后台任务（M2b 设计文档第 8 节）：导入和执行 SQL 文件。状态、取消、等待确认、报告文件、过期清理；
//! 以及浏览器模式上传的文件。导出任务（M1c3）仍用自己的 `ExportManager`。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use dblens_core::{Error, Msg, Result, msg};
use dblens_driver::CancelHandle;
use serde::{Deserialize, Serialize};
use tokio::sync::Notify;

/// 同时进行的任务上限。
pub const MAX_RUNNING_TASKS: usize = 4;
/// 已结束的任务、上传后没有使用的文件保留多久。
const KEEP: Duration = Duration::from_secs(3600);
/// 结果中保留的失败条数。
pub const MAX_FAILURES: usize = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskKind {
    Import,
    SqlFile,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskState {
    Running,
    /// 等待用户确认（SQL 文件中有需要确认的语句）。
    Confirm,
    Done,
    Failed,
    Cancelled,
}

/// 任务进行到哪一步。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskPhase {
    Preparing,
    Scanning,
    Creating,
    Clearing,
    Writing,
    Executing,
    Finished,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct TaskProgress {
    /// 已读字节和文件大小（Excel 为已读行数和总行数）。
    pub read: u64,
    pub total: u64,
    pub rows_read: u64,
    pub rows_written: u64,
    pub rows_skipped: u64,
    pub statements_done: u64,
    pub statements_total: u64,
    pub statements_failed: u64,
    /// SQL 文件：影响的行数合计。
    pub affected: u64,
}

/// 一处失败：导入出错的行，或 SQL 文件中失败的语句（`index` 为语句序号，从 0 开始）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TaskFailure {
    pub index: u64,
    pub line: u64,
    pub error: Error,
    /// 语句开头（至多 200 字符）；导入为空。
    pub text: String,
}

/// 需要确认的语句。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ConfirmItem {
    pub line: u64,
    pub text: String,
    pub reason: Msg,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TaskConfirm {
    /// 至多 20 条；`more` 是其余的条数。
    pub items: Vec<ConfirmItem>,
    pub more: u64,
}

/// 任务的状态，`GET /api/tasks/{id}` 原样返回。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TaskStatus {
    pub id: String,
    pub kind: TaskKind,
    pub state: TaskState,
    pub phase: TaskPhase,
    pub progress: TaskProgress,
    pub warnings: u64,
    /// 失败的原因；序列化时按请求的语言渲染。
    pub error: Option<Error>,
    /// 导入：出错的文件行号；分批提交时已提交的最后一行。
    pub error_line: Option<u64>,
    pub committed_line: Option<u64>,
    /// 至多 100 条。
    pub failures: Vec<TaskFailure>,
    pub confirm: Option<TaskConfirm>,
    /// 有报告文件（出错报告或执行日志）时为下载用的文件名。
    pub report: Option<String>,
}

struct Entry {
    status: Mutex<TaskStatus>,
    cancelled: AtomicBool,
    cancel: Mutex<Option<CancelHandle>>,
    answer: Mutex<Option<bool>>,
    answered: Notify,
    report: Mutex<Option<PathBuf>>,
    /// 失败语句的完整文本（与 `failures` 对应）。
    texts: Mutex<Vec<String>>,
    finished: Mutex<Option<Instant>>,
}

fn lock_error() -> Error {
    Error::internal(msg!("internal.lock_poisoned", lock = "tasks"))
}

fn not_found(id: &str) -> Error {
    Error::not_found(msg!("task.not_found", id = id))
}

/// 任务一侧的句柄：更新状态、检查取消、等待确认、写报告。
#[derive(Clone)]
pub(crate) struct TaskHandle {
    id: String,
    dir: PathBuf,
    entry: Arc<Entry>,
}

impl TaskHandle {
    pub fn update(&self, f: impl FnOnce(&mut TaskStatus)) {
        if let Ok(mut s) = self.entry.status.lock() {
            f(&mut s);
        }
    }

    pub fn cancelled(&self) -> bool {
        self.entry.cancelled.load(Ordering::SeqCst)
    }

    /// 存好连接的取消句柄（None 为清除）。先置标志再取句柄：存好之前到达的取消在这里发现。
    pub fn set_cancel(&self, h: Option<CancelHandle>) -> Result<()> {
        *self.entry.cancel.lock().map_err(|_| lock_error())? = h;
        if self.cancelled() { Err(Error::Cancelled) } else { Ok(()) }
    }

    /// 进入等待确认，直到用户回答（取消算作否）。
    pub async fn ask(&self, confirm: TaskConfirm) -> bool {
        let notified = self.entry.answered.notified();
        self.update(|s| {
            s.state = TaskState::Confirm;
            s.confirm = Some(confirm);
        });
        if self.entry.answer.lock().map(|a| a.is_none()).unwrap_or(false) && !self.cancelled() {
            notified.await;
        }
        let yes = self.entry.answer.lock().ok().and_then(|a| *a).unwrap_or(false) && !self.cancelled();
        self.update(|s| {
            s.state = TaskState::Running;
            s.confirm = None;
        });
        yes
    }

    /// 报告文件的路径（任务目录下，以任务 id 命名）。`name` 是下载用的文件名。
    pub fn report_path(&self, extension: &str, name: String) -> Result<PathBuf> {
        std::fs::create_dir_all(&self.dir)
            .map_err(|e| Error::internal(msg!("export.write_failed", error = e)))?;
        let path = self.dir.join(format!("{}.{extension}", self.id));
        *self.entry.report.lock().map_err(|_| lock_error())? = Some(path.clone());
        self.update(|s| s.report = Some(name));
        Ok(path)
    }

    /// 记一处失败（前 100 条带完整文本）。
    pub fn fail(&self, failure: TaskFailure, full_text: String) {
        let mut keep = false;
        self.update(|s| {
            if s.failures.len() < MAX_FAILURES {
                s.failures.push(failure);
                keep = true;
            }
        });
        if keep && let Ok(mut t) = self.entry.texts.lock() {
            t.push(full_text);
        }
    }

    /// 结束：设置最终状态。失败或取消时没有报告文件的话不保留报告名。
    pub fn finish(&self, result: Result<()>) {
        self.update(|s| {
            s.phase = TaskPhase::Finished;
            s.confirm = None;
            match result {
                Ok(()) => s.state = TaskState::Done,
                Err(Error::Cancelled) => s.state = TaskState::Cancelled,
                Err(e) => {
                    s.state = TaskState::Failed;
                    s.error = Some(e);
                }
            }
        });
        if let Ok(mut f) = self.entry.finished.lock() {
            *f = Some(Instant::now());
        }
    }
}

/// 上传的文件。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UploadInfo {
    pub id: String,
    pub name: String,
    pub size: u64,
}

struct Upload {
    info: UploadInfo,
    path: PathBuf,
    at: Instant,
}

/// 导入、执行 SQL 文件的文件来源：上传的文件（`{"upload": id}`），或本机路径（`{"path": …}`，只有桌面程序接受）。
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(untagged)]
pub enum FileSource {
    Upload { upload: String },
    Path { path: PathBuf },
}

/// 解析好的文件：路径、原文件名，以及任务结束后要删除的上传文件。
#[derive(Debug, Clone)]
pub(crate) struct SourceFile {
    pub path: PathBuf,
    pub name: String,
    pub upload: bool,
}

impl SourceFile {
    /// 任务结束：删除上传的文件（本机文件不动）。
    pub fn release(&self) {
        if self.upload {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

/// 全部后台任务和上传的文件。
pub struct TaskManager {
    /// 报告文件的目录、上传文件的目录。
    dirs: Mutex<(PathBuf, PathBuf)>,
    tasks: Mutex<HashMap<String, Arc<Entry>>>,
    uploads: Mutex<HashMap<String, Upload>>,
}

impl Default for TaskManager {
    fn default() -> Self {
        let tmp = std::env::temp_dir();
        Self::new(tmp.join("dblens-tasks"), tmp.join("dblens-imports"))
    }
}

impl TaskManager {
    pub fn new(reports: PathBuf, uploads: PathBuf) -> Self {
        Self {
            dirs: Mutex::new((reports, uploads)),
            tasks: Mutex::new(HashMap::new()),
            uploads: Mutex::new(HashMap::new()),
        }
    }

    /// 设定目录；上次运行留下的报告和上传文件一并删除。
    pub fn set_dirs(&self, reports: PathBuf, uploads: PathBuf) {
        for d in [&reports, &uploads] {
            if d.exists() {
                let _ = std::fs::remove_dir_all(d);
            }
        }
        if let Ok(mut d) = self.dirs.lock() {
            *d = (reports, uploads);
        }
    }

    fn dirs(&self) -> Result<(PathBuf, PathBuf)> {
        Ok(self.dirs.lock().map_err(|_| lock_error())?.clone())
    }

    /// 删除结束超过一小时的任务和报告、上传后超过一小时没有使用的文件。
    fn purge(&self) {
        if let Ok(mut tasks) = self.tasks.lock() {
            tasks.retain(|_, e| {
                let old = e.finished.lock().ok().and_then(|f| *f).is_some_and(|t| t.elapsed() >= KEEP);
                if old && let Some(p) = e.report.lock().ok().and_then(|r| r.clone()) {
                    let _ = std::fs::remove_file(p);
                }
                !old
            });
        }
        if let Ok(mut ups) = self.uploads.lock() {
            ups.retain(|_, u| {
                let old = u.at.elapsed() >= KEEP;
                if old {
                    let _ = std::fs::remove_file(&u.path);
                }
                !old
            });
        }
    }

    /// 新上传的文件：分配 id 和保存路径，由调用方写入，写完后调用 `finish_upload`。
    pub fn begin_upload(&self) -> Result<(String, PathBuf)> {
        self.purge();
        let (_, dir) = self.dirs()?;
        std::fs::create_dir_all(&dir)
            .map_err(|e| Error::internal(msg!("import.upload_failed", error = e)))?;
        let id = uuid::Uuid::new_v4().simple().to_string();
        let path = dir.join(&id);
        Ok((id, path))
    }

    pub fn finish_upload(&self, id: &str, name: &str, size: u64) -> Result<UploadInfo> {
        let (_, dir) = self.dirs()?;
        let info = UploadInfo { id: id.to_string(), name: name.to_string(), size };
        self.uploads
            .lock()
            .map_err(|_| lock_error())?
            .insert(id.to_string(), Upload { info: info.clone(), path: dir.join(id), at: Instant::now() });
        Ok(info)
    }

    /// 解析文件来源。`take` 为 true 时上传的文件交给任务（任务结束时删除），否则留着（预览用）。
    /// 浏览器模式（`allow_path` 为 false）不接受本机路径。
    pub(crate) fn resolve(&self, src: &FileSource, allow_path: bool, take: bool) -> Result<SourceFile> {
        match src {
            FileSource::Upload { upload } => {
                let mut ups = self.uploads.lock().map_err(|_| lock_error())?;
                let u =
                    ups.get(upload).ok_or_else(|| Error::not_found(msg!("import.no_upload", id = upload)))?;
                let file = SourceFile { path: u.path.clone(), name: u.info.name.clone(), upload: true };
                if take {
                    ups.remove(upload);
                }
                Ok(file)
            }
            FileSource::Path { path } => {
                if !allow_path {
                    return Err(Error::invalid(msg!("export.path_not_allowed")));
                }
                if !path.is_absolute() || !path.is_file() {
                    return Err(Error::invalid(msg!("import.bad_path", path = path.display())));
                }
                let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                Ok(SourceFile { path: path.clone(), name, upload: false })
            }
        }
    }

    /// 新建任务（检查同时进行的任务数）。
    pub(crate) fn create(&self, kind: TaskKind) -> Result<TaskHandle> {
        self.purge();
        let mut tasks = self.tasks.lock().map_err(|_| lock_error())?;
        let running = tasks
            .values()
            .filter(|e| {
                e.status.lock().is_ok_and(|s| matches!(s.state, TaskState::Running | TaskState::Confirm))
            })
            .count();
        if running >= MAX_RUNNING_TASKS {
            return Err(Error::Busy { message: msg!("task.too_many", max = MAX_RUNNING_TASKS) });
        }
        let id = uuid::Uuid::new_v4().simple().to_string();
        let entry = Arc::new(Entry {
            status: Mutex::new(TaskStatus {
                id: id.clone(),
                kind,
                state: TaskState::Running,
                phase: TaskPhase::Preparing,
                progress: TaskProgress::default(),
                warnings: 0,
                error: None,
                error_line: None,
                committed_line: None,
                failures: Vec::new(),
                confirm: None,
                report: None,
            }),
            cancelled: AtomicBool::new(false),
            cancel: Mutex::new(None),
            answer: Mutex::new(None),
            answered: Notify::new(),
            report: Mutex::new(None),
            texts: Mutex::new(Vec::new()),
            finished: Mutex::new(None),
        });
        tasks.insert(id.clone(), entry.clone());
        Ok(TaskHandle { id, dir: self.dirs()?.0, entry })
    }

    fn get(&self, id: &str) -> Result<Arc<Entry>> {
        self.tasks.lock().map_err(|_| lock_error())?.get(id).cloned().ok_or_else(|| not_found(id))
    }

    pub fn status(&self, id: &str) -> Result<TaskStatus> {
        self.purge();
        Ok(self.get(id)?.status.lock().map_err(|_| lock_error())?.clone())
    }

    /// 回答确认。
    pub fn confirm(&self, id: &str, yes: bool) -> Result<()> {
        let e = self.get(id)?;
        if e.status.lock().map_err(|_| lock_error())?.state != TaskState::Confirm {
            return Err(Error::invalid(msg!("task.not_waiting")));
        }
        *e.answer.lock().map_err(|_| lock_error())? = Some(yes);
        // 立即回到进行中：前端轮询时不会再看到确认框
        if let Ok(mut s) = e.status.lock() {
            s.state = TaskState::Running;
            s.confirm = None;
        }
        e.answered.notify_one();
        Ok(())
    }

    /// 进行中的任务：取消。已结束的任务：删除记录和报告文件。
    pub async fn cancel_or_remove(&self, id: &str) -> Result<()> {
        let e = self.get(id)?;
        let state = e.status.lock().map_err(|_| lock_error())?.state;
        if matches!(state, TaskState::Running | TaskState::Confirm) {
            e.cancelled.store(true, Ordering::SeqCst);
            e.answered.notify_one();
            let handle = e.cancel.lock().map_err(|_| lock_error())?.clone();
            if let Some(h) = handle {
                let _ = h.cancel().await;
            }
            return Ok(());
        }
        self.tasks.lock().map_err(|_| lock_error())?.remove(id);
        if let Some(p) = e.report.lock().map_err(|_| lock_error())?.take() {
            let _ = std::fs::remove_file(p);
        }
        Ok(())
    }

    /// 已结束任务的报告文件：路径和下载用的文件名。
    pub fn report(&self, id: &str) -> Result<(PathBuf, String)> {
        let e = self.get(id)?;
        let s = e.status.lock().map_err(|_| lock_error())?.clone();
        let path = e.report.lock().map_err(|_| lock_error())?.clone();
        match (s.state, path, s.report) {
            (TaskState::Done | TaskState::Failed | TaskState::Cancelled, Some(p), Some(name))
                if p.exists() =>
            {
                Ok((p, name))
            }
            _ => Err(Error::invalid(msg!("task.no_report"))),
        }
    }

    /// 桌面版：把报告复制到用户选的路径。
    pub fn save_report(&self, id: &str, to: &Path) -> Result<()> {
        let (from, _) = self.report(id)?;
        std::fs::copy(from, to).map_err(|e| Error::internal(msg!("export.write_failed", error = e)))?;
        Ok(())
    }

    /// 前 100 处失败中第 `n` 条的完整语句。
    pub fn statement(&self, id: &str, n: usize) -> Result<String> {
        self.get(id)?
            .texts
            .lock()
            .map_err(|_| lock_error())?
            .get(n)
            .cloned()
            .ok_or_else(|| Error::not_found(msg!("task.no_statement", n = n)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manager(dir: &tempfile::TempDir) -> TaskManager {
        let m = TaskManager::default();
        m.set_dirs(dir.path().join("tasks"), dir.path().join("imports"));
        m
    }

    #[tokio::test]
    async fn confirm_cancel_and_report() {
        let dir = tempfile::tempdir().unwrap();
        let m = manager(&dir);
        let h = m.create(TaskKind::SqlFile).unwrap();
        let id = h.id.clone();
        assert!(m.confirm(&id, true).is_err(), "没有在等待确认");
        // 等待确认：回答"是"
        let asking = tokio::spawn({
            let h = h.clone();
            async move { h.ask(TaskConfirm { items: vec![], more: 0 }).await }
        });
        while m.status(&id).unwrap().state != TaskState::Confirm {
            tokio::task::yield_now().await;
        }
        m.confirm(&id, true).unwrap();
        assert!(asking.await.unwrap());
        assert_eq!(m.status(&id).unwrap().state, TaskState::Running);
        // 报告和失败
        let path = h.report_path("log", "run.log".into()).unwrap();
        std::fs::write(&path, "log").unwrap();
        let e = || Error::invalid(msg!("task.not_waiting"));
        for n in 0..(MAX_FAILURES + 5) {
            h.fail(
                TaskFailure { index: n as u64, line: 1, error: e(), text: String::new() },
                format!("S{n}"),
            );
        }
        assert!(m.report(&id).is_err(), "进行中不能下载");
        h.finish(Ok(()));
        let st = m.status(&id).unwrap();
        assert_eq!((st.state, st.failures.len()), (TaskState::Done, MAX_FAILURES));
        assert_eq!(m.statement(&id, 99).unwrap(), "S99");
        assert!(m.statement(&id, 100).is_err());
        assert_eq!(m.report(&id).unwrap().1, "run.log");
        let copy = dir.path().join("copy.log");
        m.save_report(&id, &copy).unwrap();
        assert_eq!(std::fs::read_to_string(copy).unwrap(), "log");
        // 删除已结束的任务连同报告
        m.cancel_or_remove(&id).await.unwrap();
        assert!(!path.exists());
        assert!(matches!(m.status(&id), Err(Error::NotFound { .. })));
    }

    #[tokio::test]
    async fn cancel_while_waiting_answers_no() {
        let dir = tempfile::tempdir().unwrap();
        let m = manager(&dir);
        let h = m.create(TaskKind::SqlFile).unwrap();
        let id = h.id.clone();
        let asking = tokio::spawn({
            let h = h.clone();
            async move { h.ask(TaskConfirm { items: vec![], more: 0 }).await }
        });
        while m.status(&id).unwrap().state != TaskState::Confirm {
            tokio::task::yield_now().await;
        }
        m.cancel_or_remove(&id).await.unwrap();
        assert!(!asking.await.unwrap());
        assert!(h.cancelled());
        assert!(matches!(h.set_cancel(None), Err(Error::Cancelled)));
    }

    #[test]
    fn uploads_and_paths() {
        let dir = tempfile::tempdir().unwrap();
        let m = manager(&dir);
        let (id, path) = m.begin_upload().unwrap();
        std::fs::write(&path, "a,b\n").unwrap();
        m.finish_upload(&id, "数据.csv", 4).unwrap();
        let src = FileSource::Upload { upload: id.clone() };
        // 预览不取走，任务取走
        assert_eq!(m.resolve(&src, false, false).unwrap().name, "数据.csv");
        let f = m.resolve(&src, false, true).unwrap();
        assert!(m.resolve(&src, false, false).is_err(), "已交给任务");
        f.release();
        assert!(!path.exists());
        // 本机路径：只有桌面程序接受，必须是存在的文件
        let local = FileSource::Path { path: path.clone() };
        assert!(matches!(m.resolve(&local, false, true), Err(Error::Invalid { .. })));
        assert!(matches!(m.resolve(&local, true, true), Err(Error::Invalid { .. })), "文件不存在");
        let real = dir.path().join("x.sql");
        std::fs::write(&real, "SELECT 1").unwrap();
        let f = m.resolve(&FileSource::Path { path: real.clone() }, true, true).unwrap();
        assert_eq!((f.name.as_str(), f.upload), ("x.sql", false));
        f.release();
        assert!(real.exists(), "本机文件不删除");
        // JSON 形式
        let v: FileSource = serde_json::from_str(r#"{"upload":"abc"}"#).unwrap();
        assert_eq!(v, FileSource::Upload { upload: "abc".into() });
    }

    #[test]
    fn running_limit() {
        let dir = tempfile::tempdir().unwrap();
        let m = manager(&dir);
        let hs: Vec<TaskHandle> =
            (0..MAX_RUNNING_TASKS).map(|_| m.create(TaskKind::Import).unwrap()).collect();
        assert!(matches!(m.create(TaskKind::Import), Err(Error::Busy { .. })));
        hs[0].finish(Err(Error::Cancelled));
        assert_eq!(m.status(&hs[0].id).unwrap().state, TaskState::Cancelled);
        assert!(m.create(TaskKind::Import).is_ok());
    }
}
