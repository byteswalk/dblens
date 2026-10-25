//! 会话（设计文档 5.3 节）：每个 SQL 编辑器标签页独占一个连接，由一个后台任务（actor）持有。
//! 执行是拉取式的：调用方每调用一次 `next` 取一个事件，结果集按批读取，读多少取多少。
//! 每条语句执行前都经过 `Policy`；需要确认时脚本停下，调用方确认后用 `confirmed = true` 重新提交。

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use dblens_core::{Error, Result, Row};
use dblens_dialect::{Dialect, Variant};
use dblens_driver::{CancelHandle, ColumnMeta, Connection, ExecEvent};
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, oneshot};

use crate::metadata::MetadataCache;
use crate::policy::{Decision, ExecContext, Outcome, Policy, StatementInfo, StatementKind, classify};

pub type SessionId = u64;

/// 执行过程中依次产生的事件，由 `SessionManager::next` 逐个取出。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SessionEvent {
    /// 开始执行第 `index` 条语句（从 0 开始）；`start`/`end` 是它在提交的 SQL 中的字节偏移。
    StatementStart {
        index: usize,
        start: usize,
        end: usize,
        info: StatementInfo,
    },
    ResultStart {
        columns: Vec<ColumnMeta>,
    },
    Rows {
        rows: Vec<Row>,
    },
    ResultEnd,
    Affected {
        count: u64,
    },
    Message {
        text: String,
    },
    StatementEnd {
        index: usize,
        elapsed_ms: u64,
    },
    /// 这条语句需要确认；脚本停在这里，后面的语句都没有执行。
    ConfirmRequired {
        index: usize,
        start: usize,
        reason: String,
    },
    /// 这条语句失败（或被拒绝、被取消）；脚本停在这里。
    Failed {
        index: usize,
        error: Error,
    },
    /// 整个脚本结束。之后 `next` 返回 None。
    Done,
}

/// 会话的当前状态。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionInfo {
    pub id: SessionId,
    pub profile_id: String,
    pub variant: Variant,
    pub autocommit: bool,
    /// 手动提交模式下，是否有尚未提交的修改。
    pub in_transaction: bool,
    /// 是否正在执行脚本。
    pub busy: bool,
    /// 连接已断开；要先 `reconnect` 才能继续执行。
    pub broken: bool,
}

enum Command {
    Execute { sql: String, confirmed: bool, reply: oneshot::Sender<Result<()>> },
    Next { max_rows: usize, reply: oneshot::Sender<Result<Option<SessionEvent>>> },
    SetAutocommit { on: bool, reply: oneshot::Sender<Result<()>> },
    Commit { reply: oneshot::Sender<Result<()>> },
    Rollback { reply: oneshot::Sender<Result<()>> },
    Replace { conn: Box<dyn Connection>, dialect: Dialect, reply: oneshot::Sender<Result<()>> },
    Close { commit: bool, reply: oneshot::Sender<Result<()>> },
}

struct Handle {
    tx: mpsc::Sender<Command>,
    cancel: Arc<Mutex<CancelHandle>>,
    info: Arc<Mutex<SessionInfo>>,
}

/// 管理全部会话。会话之间互不影响，每个会话的命令按顺序处理。
pub struct SessionManager {
    policy: Arc<dyn Policy>,
    metadata: Arc<MetadataCache>,
    next_id: AtomicU64,
    sessions: Mutex<HashMap<SessionId, Handle>>,
}

fn gone() -> Error {
    Error::internal("会话任务已退出")
}

impl SessionManager {
    pub fn new(policy: Arc<dyn Policy>, metadata: Arc<MetadataCache>) -> Self {
        Self { policy, metadata, next_id: AtomicU64::new(1), sessions: Mutex::new(HashMap::new()) }
    }

    fn handle<T>(&self, id: SessionId, f: impl FnOnce(&Handle) -> T) -> Result<T> {
        let sessions = self.sessions.lock().map_err(|_| Error::internal("会话表锁已损坏"))?;
        sessions.get(&id).map(f).ok_or_else(|| Error::not_found(format!("会话 {id}")))
    }

    /// 用一个已建立的连接创建会话（连接由调用方通过 `DriverRegistry::connect` 建立）。
    pub fn open(
        &self,
        profile_id: &str,
        environment: crate::profile::Environment,
        conn: Box<dyn Connection>,
        dialect: Dialect,
    ) -> Result<SessionId> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let info = Arc::new(Mutex::new(SessionInfo {
            id,
            profile_id: profile_id.to_string(),
            variant: dialect.variant(),
            autocommit: true,
            in_transaction: false,
            busy: false,
            broken: false,
        }));
        let cancel = Arc::new(Mutex::new(conn.cancel_handle()));
        let (tx, rx) = mpsc::channel(16);
        let actor = Actor {
            conn: Some(conn),
            ctx: ExecContext { profile_id: profile_id.to_string(), environment, dialect },
            policy: self.policy.clone(),
            metadata: self.metadata.clone(),
            info: info.clone(),
            cancel: cancel.clone(),
            rx,
        };
        tokio::spawn(actor.run());
        self.sessions
            .lock()
            .map_err(|_| Error::internal("会话表锁已损坏"))?
            .insert(id, Handle { tx, cancel, info });
        Ok(id)
    }

    async fn call<T>(
        &self,
        id: SessionId,
        make: impl FnOnce(oneshot::Sender<Result<T>>) -> Command,
    ) -> Result<T> {
        let tx = self.handle(id, |h| h.tx.clone())?;
        let (reply, rx) = oneshot::channel();
        tx.send(make(reply)).await.map_err(|_| gone())?;
        rx.await.map_err(|_| gone())?
    }

    pub fn info(&self, id: SessionId) -> Result<SessionInfo> {
        self.handle(id, |h| {
            h.info.lock().map(|i| i.clone()).map_err(|_| Error::internal("会话状态锁已损坏"))
        })?
    }

    pub fn list(&self) -> Result<Vec<SessionInfo>> {
        let sessions = self.sessions.lock().map_err(|_| Error::internal("会话表锁已损坏"))?;
        let mut out: Vec<SessionInfo> =
            sessions.values().filter_map(|h| h.info.lock().ok().map(|i| i.clone())).collect();
        out.sort_by_key(|i| i.id);
        Ok(out)
    }

    /// 提交一段 SQL（可以有多条语句）。之后用 `next` 取事件，直到 `Done`。会话正忙时报 `Busy`。
    pub async fn execute(&self, id: SessionId, sql: &str, confirmed: bool) -> Result<()> {
        let sql = sql.to_string();
        self.call(id, |reply| Command::Execute { sql, confirmed, reply }).await
    }

    /// 取下一个事件；`max_rows` 是一个 `Rows` 事件最多包含的行数。没有在执行的脚本时返回 None。
    pub async fn next(&self, id: SessionId, max_rows: usize) -> Result<Option<SessionEvent>> {
        self.call(id, |reply| Command::Next { max_rows: max_rows.max(1), reply }).await
    }

    /// 取消正在执行的语句。可以在会话忙碌时调用；没有在执行时什么也不做。
    pub async fn cancel(&self, id: SessionId) -> Result<()> {
        let cancel = self.handle(id, |h| h.cancel.clone())?;
        let handle = cancel.lock().map_err(|_| Error::internal("取消句柄锁已损坏"))?.clone();
        handle.cancel().await
    }

    pub async fn set_autocommit(&self, id: SessionId, on: bool) -> Result<()> {
        self.call(id, |reply| Command::SetAutocommit { on, reply }).await
    }

    pub async fn commit(&self, id: SessionId) -> Result<()> {
        self.call(id, |reply| Command::Commit { reply }).await
    }

    pub async fn rollback(&self, id: SessionId) -> Result<()> {
        self.call(id, |reply| Command::Rollback { reply }).await
    }

    /// 换上一个新建立的连接（连接断开后由用户确认重连）。原连接上未提交的修改已经丢失。
    pub async fn replace_connection(
        &self,
        id: SessionId,
        conn: Box<dyn Connection>,
        dialect: Dialect,
    ) -> Result<()> {
        self.call(id, |reply| Command::Replace { conn, dialect, reply }).await
    }

    /// 关闭会话。有未提交的修改时按 `commit` 提交或回滚。
    pub async fn close(&self, id: SessionId, commit: bool) -> Result<()> {
        let result = self.call(id, |reply| Command::Close { commit, reply }).await;
        self.sessions.lock().map_err(|_| Error::internal("会话表锁已损坏"))?.remove(&id);
        result
    }
}

struct Actor {
    /// 断开后仍保留（关闭时尝试回滚）；`Replace` 换成新连接。
    conn: Option<Box<dyn Connection>>,
    ctx: ExecContext,
    policy: Arc<dyn Policy>,
    metadata: Arc<MetadataCache>,
    info: Arc<Mutex<SessionInfo>>,
    cancel: Arc<Mutex<CancelHandle>>,
    rx: mpsc::Receiver<Command>,
}

fn reply<T>(tx: oneshot::Sender<Result<T>>, value: Result<T>) {
    // 调用方已经不等结果（如请求被中断）时丢弃
    let _ = tx.send(value);
}

fn is_disconnect(e: &Error) -> bool {
    matches!(e, Error::Connect { .. })
}

impl Actor {
    fn update(&self, f: impl FnOnce(&mut SessionInfo)) {
        if let Ok(mut i) = self.info.lock() {
            f(&mut i);
        }
    }

    fn snapshot(&self) -> SessionInfo {
        self.info.lock().map(|i| i.clone()).unwrap_or_else(|p| p.into_inner().clone())
    }

    fn note_error<T>(&self, r: &Result<T>) {
        if let Err(e) = r
            && is_disconnect(e)
        {
            self.update(|i| i.broken = true);
        }
    }

    /// 空闲时的命令循环。
    async fn run(mut self) {
        while let Some(cmd) = self.rx.recv().await {
            match cmd {
                Command::Execute { sql, confirmed, reply: tx } => {
                    if self.snapshot().broken || self.conn.is_none() {
                        reply(tx, Err(Error::connect("连接已断开，请先重新连接")));
                        continue;
                    }
                    reply(tx, Ok(()));
                    self.update(|i| i.busy = true);
                    let closed = self.run_script(&sql, confirmed).await;
                    self.update(|i| i.busy = false);
                    if let Some((commit, tx)) = closed {
                        reply(tx, self.close(commit).await);
                        return;
                    }
                }
                Command::Next { reply: tx, .. } => reply(tx, Ok(None)),
                Command::SetAutocommit { on, reply: tx } => {
                    let r = match self.conn.as_mut() {
                        Some(c) => c.set_autocommit(on).await,
                        None => Err(Error::connect("连接已断开")),
                    };
                    if r.is_ok() {
                        // 切回自动提交时驱动会提交未提交的修改
                        self.update(|i| {
                            i.autocommit = on;
                            if on {
                                i.in_transaction = false;
                            }
                        });
                    }
                    self.note_error(&r);
                    reply(tx, r);
                }
                Command::Commit { reply: tx } => {
                    let r = match self.conn.as_mut() {
                        Some(c) => c.commit().await,
                        None => Err(Error::connect("连接已断开")),
                    };
                    if r.is_ok() {
                        self.update(|i| i.in_transaction = false);
                    }
                    self.note_error(&r);
                    reply(tx, r);
                }
                Command::Rollback { reply: tx } => {
                    let r = match self.conn.as_mut() {
                        Some(c) => c.rollback().await,
                        None => Err(Error::connect("连接已断开")),
                    };
                    if r.is_ok() {
                        self.update(|i| i.in_transaction = false);
                    }
                    self.note_error(&r);
                    reply(tx, r);
                }
                Command::Replace { conn, dialect, reply: tx } => {
                    if let Some(old) = self.conn.take() {
                        let _ = old.close().await;
                    }
                    if let Ok(mut c) = self.cancel.lock() {
                        *c = conn.cancel_handle();
                    }
                    self.conn = Some(conn);
                    self.ctx.dialect = dialect;
                    self.update(|i| {
                        i.broken = false;
                        i.autocommit = true;
                        i.in_transaction = false;
                        i.variant = dialect.variant();
                    });
                    reply(tx, Ok(()));
                }
                Command::Close { commit, reply: tx } => {
                    reply(tx, self.close(commit).await);
                    return;
                }
            }
        }
        // 管理器被丢弃：尽量回滚并断开
        let _ = self.close(false).await;
    }

    /// 关闭连接；有未提交的修改时按 `commit` 提交或回滚。
    async fn close(&mut self, commit: bool) -> Result<()> {
        let Some(mut conn) = self.conn.take() else { return Ok(()) };
        let state = self.snapshot();
        let mut result = Ok(());
        if state.in_transaction && !state.broken {
            result = if commit { conn.commit().await } else { conn.rollback().await };
        }
        let closed = conn.close().await;
        result.and(closed)
    }
}

/// 执行期间等来的请求。
enum Req {
    Next(usize, oneshot::Sender<Result<Option<SessionEvent>>>),
    Close(bool, oneshot::Sender<Result<()>>),
}

fn busy() -> Error {
    Error::Busy { message: "正在执行，请等待完成或先取消".into() }
}

/// 执行期间处理命令：`Next` 时队列里有事件就直接交出，队列空了才把请求交给执行循环去取数据；
/// 其他命令一律报 `Busy`。管理器被丢弃时按回滚关闭。
async fn take_request(rx: &mut mpsc::Receiver<Command>, queue: &mut VecDeque<SessionEvent>) -> Req {
    loop {
        let Some(cmd) = rx.recv().await else {
            let (tx, _) = oneshot::channel();
            return Req::Close(false, tx);
        };
        match cmd {
            Command::Next { max_rows, reply: tx } => match queue.pop_front() {
                Some(ev) => reply(tx, Ok(Some(ev))),
                None => return Req::Next(max_rows, tx),
            },
            Command::Close { commit, reply: tx } => return Req::Close(commit, tx),
            Command::Execute { reply: tx, .. } => reply(tx, Err(busy())),
            Command::SetAutocommit { reply: tx, .. }
            | Command::Commit { reply: tx }
            | Command::Rollback { reply: tx }
            | Command::Replace { reply: tx, .. } => reply(tx, Err(busy())),
        }
    }
}

fn convert(ev: ExecEvent, affected: &mut Option<u64>) -> SessionEvent {
    match ev {
        ExecEvent::ResultStart { columns } => SessionEvent::ResultStart { columns },
        ExecEvent::Rows(rows) => SessionEvent::Rows { rows },
        ExecEvent::ResultEnd => SessionEvent::ResultEnd,
        ExecEvent::Affected(count) => {
            *affected = Some(affected.unwrap_or(0) + count);
            SessionEvent::Affected { count }
        }
        ExecEvent::Message(text) => SessionEvent::Message { text },
    }
}

impl Actor {
    /// 执行一段脚本，按 `Next` 请求逐个交出事件。执行中途会话被关闭时返回关闭请求。
    async fn run_script(
        &mut self,
        sql: &str,
        confirmed: bool,
    ) -> Option<(bool, oneshot::Sender<Result<()>>)> {
        let Actor { conn, ctx, policy, metadata, info, rx, .. } = self;
        let conn = conn.as_mut()?;
        let set = |f: &dyn Fn(&mut SessionInfo)| {
            if let Ok(mut i) = info.lock() {
                f(&mut i);
            }
        };
        let mut queue: VecDeque<SessionEvent> = VecDeque::new();
        // 一个等着数据的 Next 请求：队列空时由执行循环取数据后回复
        let mut pending: Option<(usize, oneshot::Sender<Result<Option<SessionEvent>>>)> = None;

        'script: for (index, st) in ctx.dialect.split(sql).into_iter().enumerate() {
            let stmt = classify(&ctx.dialect, &st.text);
            match policy.before_execute(ctx, &stmt).await {
                Decision::Allow => {}
                Decision::Confirm { .. } if confirmed => {}
                Decision::Confirm { reason } => {
                    queue.push_back(SessionEvent::ConfirmRequired { index, start: st.start, reason });
                    break 'script;
                }
                Decision::Deny { reason } => {
                    queue.push_back(SessionEvent::Failed { index, error: Error::Denied { reason } });
                    break 'script;
                }
            }
            queue.push_back(SessionEvent::StatementStart {
                index,
                start: st.start,
                end: st.end,
                info: stmt.clone(),
            });
            // 惰性执行：调用方取走前面的事件、再来要数据时才真正开始执行。这样 StatementStart 能先送到，
            // 调用方在语句执行期间随时可以取消
            loop {
                let (m, tx) = match pending.take() {
                    Some(p) => p,
                    None => match take_request(rx, &mut queue).await {
                        Req::Next(m, tx) => (m, tx),
                        Req::Close(commit, tx) => return Some((commit, tx)),
                    },
                };
                match queue.pop_front() {
                    Some(ev) => reply(tx, Ok(Some(ev))),
                    None => {
                        pending = Some((m, tx));
                        break;
                    }
                }
            }
            let started = Instant::now();
            let mut affected = None;
            let result: Result<()> = match conn.execute(&st.text, &[]).await {
                Err(e) => Err(e),
                Ok(mut stream) => loop {
                    let (max_rows, tx) = match pending.take() {
                        Some((m, tx)) => match queue.pop_front() {
                            Some(ev) => {
                                reply(tx, Ok(Some(ev)));
                                continue;
                            }
                            None => (m, tx),
                        },
                        None => match take_request(rx, &mut queue).await {
                            Req::Next(m, tx) => (m, tx),
                            Req::Close(commit, tx) => return Some((commit, tx)),
                        },
                    };
                    match stream.next(max_rows).await {
                        Ok(Some(ev)) => reply(tx, Ok(Some(convert(ev, &mut affected)))),
                        Ok(None) => {
                            pending = Some((max_rows, tx));
                            break Ok(());
                        }
                        Err(e) => {
                            pending = Some((max_rows, tx));
                            break Err(e);
                        }
                    }
                },
            };
            let outcome = match &result {
                Ok(()) => Outcome::Succeeded { affected },
                Err(Error::Cancelled) => Outcome::Cancelled,
                Err(e) => Outcome::Failed { message: e.to_string() },
            };
            policy.after_execute(ctx, &stmt, &outcome).await;
            match result {
                Ok(()) => {
                    queue.push_back(SessionEvent::StatementEnd {
                        index,
                        elapsed_ms: started.elapsed().as_millis() as u64,
                    });
                    if stmt.kind == StatementKind::Ddl {
                        metadata.invalidate_objects(&ctx.profile_id, &stmt.objects);
                    }
                    let autocommit = info.lock().map(|i| i.autocommit).unwrap_or(true);
                    match (stmt.kind, stmt.keyword.as_str()) {
                        (StatementKind::Dml, _) if !autocommit => set(&|i| i.in_transaction = true),
                        (StatementKind::Tcl, "COMMIT" | "ROLLBACK") => set(&|i| i.in_transaction = false),
                        _ => {}
                    }
                }
                Err(error) => {
                    if is_disconnect(&error) {
                        set(&|i| i.broken = true);
                    }
                    queue.push_back(SessionEvent::Failed { index, error });
                    break 'script;
                }
            }
        }
        queue.push_back(SessionEvent::Done);

        // 交出剩下的事件，最后一个是 Done
        if let Some((_, tx)) = pending.take() {
            reply(tx, Ok(queue.pop_front()));
        }
        // 最后一个事件交出后立即返回，之后的命令（提交、回滚等）由空闲循环处理
        while !queue.is_empty() {
            let cmd = rx.recv().await?;
            match cmd {
                Command::Next { reply: tx, .. } => reply(tx, Ok(queue.pop_front())),
                Command::Close { commit, reply: tx } => return Some((commit, tx)),
                Command::Execute { reply: tx, .. } => reply(tx, Err(busy())),
                Command::SetAutocommit { reply: tx, .. }
                | Command::Commit { reply: tx }
                | Command::Rollback { reply: tx }
                | Command::Replace { reply: tx, .. } => reply(tx, Err(busy())),
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use dblens_dialect::Variant;

    use super::*;
    use crate::fake::{FakeConn, Log};
    use crate::policy::DefaultPolicy;
    use crate::profile::Environment;

    fn manager() -> (SessionManager, Arc<MetadataCache>) {
        let cache = Arc::new(MetadataCache::new());
        (SessionManager::new(Arc::new(DefaultPolicy), cache.clone()), cache)
    }

    fn open(m: &SessionManager, env: Environment) -> (SessionId, Log) {
        let log: Log = Arc::default();
        let id =
            m.open("p1", env, Box::new(FakeConn::new(log.clone())), Dialect::new(Variant::Postgres)).unwrap();
        (id, log)
    }

    /// 取完全部事件（每批最多 2 行）。
    async fn drain(m: &SessionManager, id: SessionId) -> Vec<SessionEvent> {
        let mut out = Vec::new();
        while let Some(ev) = m.next(id, 2).await.unwrap() {
            let done = ev == SessionEvent::Done;
            out.push(ev);
            if done {
                break;
            }
        }
        out
    }

    fn kinds(events: &[SessionEvent]) -> Vec<String> {
        events
            .iter()
            .map(|e| match e {
                SessionEvent::StatementStart { index, .. } => format!("start{index}"),
                SessionEvent::ResultStart { .. } => "result".into(),
                SessionEvent::Rows { rows } => format!("rows{}", rows.len()),
                SessionEvent::ResultEnd => "end".into(),
                SessionEvent::Affected { count } => format!("affected{count}"),
                SessionEvent::Message { .. } => "message".into(),
                SessionEvent::StatementEnd { index, .. } => format!("done{index}"),
                SessionEvent::ConfirmRequired { index, .. } => format!("confirm{index}"),
                SessionEvent::Failed { index, .. } => format!("failed{index}"),
                SessionEvent::Done => "DONE".into(),
            })
            .collect()
    }

    #[tokio::test]
    async fn pulls_rows_in_batches_across_statements() {
        let (m, _) = manager();
        let (id, _) = open(&m, Environment::None);
        let sql = "ROWS 5;\nUPDATE t SET a = 1 WHERE id = 1";
        m.execute(id, sql, false).await.unwrap();
        let events = drain(&m, id).await;
        assert_eq!(
            kinds(&events),
            [
                "start0",
                "result",
                "rows2",
                "rows2",
                "rows1",
                "end",
                "done0",
                "start1",
                "affected1",
                "done1",
                "DONE"
            ]
        );
        // 语句位置是字节偏移
        let SessionEvent::StatementStart { start, end, .. } = &events[7] else { panic!() };
        assert_eq!(&sql[*start..*end], "UPDATE t SET a = 1 WHERE id = 1");
        assert_eq!(m.next(id, 2).await.unwrap(), None, "脚本结束后没有事件");
        assert!(!m.info(id).unwrap().busy);
    }

    /// 语句在调用方取走 StatementStart、再来要数据时才执行，所以执行期间随时可以取消。
    #[tokio::test]
    async fn statement_runs_only_when_its_data_is_requested() {
        let (m, _) = manager();
        let (id, log) = open(&m, Environment::None);
        m.execute(id, "ROWS 1; ROWS 2", false).await.unwrap();
        assert!(matches!(m.next(id, 5).await.unwrap(), Some(SessionEvent::StatementStart { index: 0, .. })));
        assert!(!log.lock().unwrap().iter().any(|l| l.starts_with("execute")), "取走开始事件时还没执行");
        assert!(matches!(m.next(id, 5).await.unwrap(), Some(SessionEvent::ResultStart { .. })));
        assert_eq!(log.lock().unwrap().iter().filter(|l| l.starts_with("execute")).count(), 1);
        drain(&m, id).await;
        assert_eq!(log.lock().unwrap().iter().filter(|l| l.starts_with("execute")).count(), 2);
    }

    #[tokio::test]
    async fn failure_stops_the_script() {
        let (m, _) = manager();
        let (id, log) = open(&m, Environment::None);
        m.execute(id, "INSERT INTO t VALUES (1); FAIL; INSERT INTO t VALUES (2)", false).await.unwrap();
        let events = drain(&m, id).await;
        assert_eq!(kinds(&events), ["start0", "affected1", "done0", "start1", "failed1", "DONE"]);
        let SessionEvent::Failed { error, .. } = &events[4] else { panic!() };
        assert!(matches!(error, Error::Sql { code, .. } if code == "42000"));
        assert!(!log.lock().unwrap().iter().any(|l| l.contains("VALUES (2)")), "第三条没有执行");
    }

    #[tokio::test]
    async fn other_commands_are_busy_while_executing() {
        let (m, _) = manager();
        let (id, _) = open(&m, Environment::None);
        m.execute(id, "ROWS 3", false).await.unwrap();
        assert!(matches!(m.next(id, 1).await.unwrap(), Some(SessionEvent::StatementStart { .. })));
        assert!(m.info(id).unwrap().busy);
        assert!(matches!(m.commit(id).await, Err(Error::Busy { .. })));
        assert!(matches!(m.execute(id, "ROWS 1", false).await, Err(Error::Busy { .. })));
        drain(&m, id).await;
        m.commit(id).await.unwrap();
    }

    #[tokio::test]
    async fn production_requires_confirmation() {
        let (m, _) = manager();
        let (id, log) = open(&m, Environment::Prod);
        let sql = "INSERT INTO t VALUES (1); DELETE FROM t; INSERT INTO t VALUES (2)";
        m.execute(id, sql, false).await.unwrap();
        let events = drain(&m, id).await;
        assert_eq!(kinds(&events), ["start0", "affected1", "done0", "confirm1", "DONE"]);
        let SessionEvent::ConfirmRequired { start, reason, .. } = &events[3] else { panic!() };
        assert!(reason.contains("WHERE"), "{reason}");
        assert!(!log.lock().unwrap().iter().any(|l| l.contains("DELETE")));
        // 确认后从这条重新提交
        m.execute(id, &sql[*start..], true).await.unwrap();
        assert_eq!(
            kinds(&drain(&m, id).await),
            ["start0", "affected1", "done0", "start1", "affected1", "done1", "DONE"]
        );
        assert!(log.lock().unwrap().iter().any(|l| l == "execute DELETE FROM t"));
    }

    #[tokio::test]
    async fn cancel_while_waiting() {
        let (m, _) = manager();
        let (id, log) = open(&m, Environment::None);
        m.execute(id, "SLEEP", false).await.unwrap();
        assert!(matches!(m.next(id, 2).await.unwrap(), Some(SessionEvent::StatementStart { .. })));
        let m = Arc::new(m);
        let fetch = {
            let m = m.clone();
            tokio::spawn(async move { m.next(id, 2).await })
        };
        tokio::time::sleep(Duration::from_millis(50)).await;
        m.cancel(id).await.unwrap();
        let ev = tokio::time::timeout(Duration::from_secs(5), fetch).await.unwrap().unwrap().unwrap();
        assert!(matches!(ev, Some(SessionEvent::Failed { error: Error::Cancelled, .. })), "{ev:?}");
        assert_eq!(m.next(id, 2).await.unwrap(), Some(SessionEvent::Done));
        assert!(log.lock().unwrap().contains(&"cancel".to_string()));
        // 取消后会话照常可用
        m.execute(id, "ROWS 1", false).await.unwrap();
        assert_eq!(kinds(&drain(&m, id).await).last().unwrap(), "DONE");
    }

    #[tokio::test]
    async fn manual_commit_tracks_pending_changes() {
        let (m, _) = manager();
        let (id, log) = open(&m, Environment::None);
        m.set_autocommit(id, false).await.unwrap();
        m.execute(id, "SELECT 1", false).await.unwrap();
        drain(&m, id).await;
        assert!(!m.info(id).unwrap().in_transaction, "查询不算修改");
        m.execute(id, "UPDATE t SET a = 1 WHERE id = 1", false).await.unwrap();
        drain(&m, id).await;
        assert!(m.info(id).unwrap().in_transaction);
        m.commit(id).await.unwrap();
        assert!(!m.info(id).unwrap().in_transaction);
        m.execute(id, "DELETE FROM t WHERE id = 2", false).await.unwrap();
        drain(&m, id).await;
        // 关闭时有未提交的修改：按要求提交
        m.close(id, true).await.unwrap();
        let log = log.lock().unwrap();
        assert_eq!(log.iter().filter(|l| *l == "commit").count(), 2, "{log:?}");
        assert_eq!(log.last().unwrap(), "close");
        assert!(matches!(m.info(id), Err(Error::NotFound { .. })));
    }

    #[tokio::test]
    async fn disconnect_marks_session_broken_until_replaced() {
        let (m, _) = manager();
        let (id, _) = open(&m, Environment::None);
        m.execute(id, "DISCONNECT", false).await.unwrap();
        let events = drain(&m, id).await;
        assert!(matches!(&events[1], SessionEvent::Failed { error: Error::Connect { .. }, .. }));
        assert!(m.info(id).unwrap().broken);
        assert!(matches!(m.execute(id, "ROWS 1", false).await, Err(Error::Connect { .. })), "不静默重连");
        let log: Log = Arc::default();
        m.replace_connection(id, Box::new(FakeConn::new(log)), Dialect::new(Variant::Postgres))
            .await
            .unwrap();
        assert!(!m.info(id).unwrap().broken);
        m.execute(id, "ROWS 1", false).await.unwrap();
        assert_eq!(kinds(&drain(&m, id).await).last().unwrap(), "DONE");
    }

    #[tokio::test]
    async fn ddl_invalidates_metadata_cache() {
        let (m, cache) = manager();
        let (id, _) = open(&m, Environment::None);
        cache.put(("p1".into(), dblens_dialect::meta::MetaKey::Tables, Default::default()), Arc::default());
        cache.put(("p2".into(), dblens_dialect::meta::MetaKey::Tables, Default::default()), Arc::default());
        m.execute(id, "SELECT 1", false).await.unwrap();
        drain(&m, id).await;
        assert_eq!(cache.len(), 2, "查询不影响缓存");
        m.execute(id, "CREATE TABLE t (id int)", false).await.unwrap();
        drain(&m, id).await;
        assert_eq!(cache.len(), 1, "DDL 后只清本连接配置的缓存");
    }

    #[tokio::test]
    async fn close_during_execution_rolls_back() {
        let (m, _) = manager();
        let (id, log) = open(&m, Environment::None);
        m.set_autocommit(id, false).await.unwrap();
        m.execute(id, "UPDATE t SET a = 1 WHERE id = 1; ROWS 10", false).await.unwrap();
        for _ in 0..5 {
            m.next(id, 1).await.unwrap();
        }
        m.close(id, false).await.unwrap();
        let log = log.lock().unwrap();
        assert!(log.contains(&"rollback".to_string()) && log.last().unwrap() == "close", "{log:?}");
    }
}
