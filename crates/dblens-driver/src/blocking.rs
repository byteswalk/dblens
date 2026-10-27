//! 把同步（阻塞）驱动包装成异步 `Connection`：每个连接一个专用线程，通过命令通道交互。
//! 达梦（DPI）、Oracle（ODPI-C）以及以后的 JDBC 桥都用这个适配器。
//! 结果集等借用连接的状态只存在于工作线程的栈上，因此不需要自引用结构。

use std::sync::mpsc;
use std::thread;

use async_trait::async_trait;
use dblens_core::{Error, Result, Value, msg};
use tokio::sync::oneshot;

use crate::{CancelHandle, Connection, ExecEvent, ExecStream, ServerInfo};

/// 同步的执行流。
pub trait BlockingExec {
    fn next(&mut self, max_rows: usize) -> Result<Option<ExecEvent>>;
}

/// 同步的连接。所有方法都只在该连接的专用线程上调用。
pub trait BlockingConnection: Send + 'static {
    fn execute<'a>(&'a mut self, sql: &'a str, params: &'a [Value]) -> Result<Box<dyn BlockingExec + 'a>>;
    /// 大字段（长文本、二进制、LOB）每个值最多读取的字节数，超出的部分截断为预览（`Value::Lob`）。
    /// 新连接为 `DEFAULT_LOB_PREVIEW_BYTES`；导出时放宽（M1c3）。只影响之后执行的语句。
    fn set_lob_limit(&mut self, bytes: usize);
    fn set_autocommit(&mut self, on: bool) -> Result<()>;
    fn commit(&mut self) -> Result<()>;
    fn rollback(&mut self) -> Result<()>;
    fn ping(&mut self) -> Result<()>;
    fn server_info(&mut self) -> Result<ServerInfo>;
    fn close(self: Box<Self>) -> Result<()>;
}

type Reply<T> = oneshot::Sender<Result<T>>;

enum Cmd {
    Execute {
        sql: String,
        params: Vec<Value>,
        reply: Reply<()>,
    },
    Next {
        max_rows: usize,
        reply: Reply<Option<ExecEvent>>,
    },
    /// 异步侧提前丢弃了执行流。
    EndExec,
    /// 不需要回复：只改变之后执行的语句。
    SetLobLimit {
        bytes: usize,
    },
    SetAutocommit {
        on: bool,
        reply: Reply<()>,
    },
    Commit {
        reply: Reply<()>,
    },
    Rollback {
        reply: Reply<()>,
    },
    Ping {
        reply: Reply<()>,
    },
    ServerInfo {
        reply: Reply<ServerInfo>,
    },
    Close {
        reply: Reply<()>,
    },
}

impl Cmd {
    /// 执行流尚未结束时收到其他命令：回复错误。正常使用下异步侧的借用规则会阻止这种情况。
    fn reject(self) {
        let busy = || Error::internal(msg!("driver.result_unread"));
        match self {
            Cmd::Execute { reply, .. }
            | Cmd::SetAutocommit { reply, .. }
            | Cmd::Commit { reply }
            | Cmd::Rollback { reply }
            | Cmd::Ping { reply }
            | Cmd::Close { reply } => {
                let _ = reply.send(Err(busy()));
            }
            Cmd::Next { reply, .. } => {
                let _ = reply.send(Err(busy()));
            }
            Cmd::ServerInfo { reply } => {
                let _ = reply.send(Err(busy()));
            }
            Cmd::EndExec | Cmd::SetLobLimit { .. } => {}
        }
    }
}

fn run_worker<C: BlockingConnection>(mut conn: C, rx: mpsc::Receiver<Cmd>) {
    while let Ok(cmd) = rx.recv() {
        match cmd {
            Cmd::Execute { sql, params, reply } => match conn.execute(&sql, &params) {
                Err(e) => {
                    let _ = reply.send(Err(e));
                }
                Ok(mut exec) => {
                    let _ = reply.send(Ok(()));
                    // 执行流存续期间只处理 Next / EndExec
                    while let Ok(cmd) = rx.recv() {
                        match cmd {
                            Cmd::Next { max_rows, reply } => {
                                let result = exec.next(max_rows);
                                let finished = !matches!(result, Ok(Some(_)));
                                let _ = reply.send(result);
                                if finished {
                                    break;
                                }
                            }
                            Cmd::EndExec => break,
                            other => other.reject(),
                        }
                    }
                }
            },
            // 执行流结束后到达的多余命令
            Cmd::Next { reply, .. } => {
                let _ = reply.send(Ok(None));
            }
            Cmd::EndExec => {}
            Cmd::SetLobLimit { bytes } => conn.set_lob_limit(bytes),
            Cmd::SetAutocommit { on, reply } => {
                let _ = reply.send(conn.set_autocommit(on));
            }
            Cmd::Commit { reply } => {
                let _ = reply.send(conn.commit());
            }
            Cmd::Rollback { reply } => {
                let _ = reply.send(conn.rollback());
            }
            Cmd::Ping { reply } => {
                let _ = reply.send(conn.ping());
            }
            Cmd::ServerInfo { reply } => {
                let _ = reply.send(conn.server_info());
            }
            Cmd::Close { reply } => {
                let _ = reply.send(Box::new(conn).close());
                return;
            }
        }
    }
    // 发送端全部释放：连接在这里随 conn 一起释放
}

/// 在新线程上运行同步连接，返回异步的 `Connection`。
pub fn into_async<C: BlockingConnection>(
    conn: C,
    cancel: CancelHandle,
    thread_name: &str,
) -> Result<Box<dyn Connection>> {
    let (tx, rx) = mpsc::channel();
    thread::Builder::new()
        .name(thread_name.to_string())
        .spawn(move || run_worker(conn, rx))
        .map_err(|e| Error::internal(msg!("driver.worker_spawn_failed", error = e)))?;
    Ok(Box::new(AsyncConnection { tx, cancel }))
}

fn worker_gone() -> Error {
    Error::connect(msg!("driver.worker_gone"))
}

async fn call<T: Send>(tx: &mpsc::Sender<Cmd>, make: impl FnOnce(Reply<T>) -> Cmd) -> Result<T> {
    let (reply, rx) = oneshot::channel();
    tx.send(make(reply)).map_err(|_| worker_gone())?;
    rx.await.map_err(|_| worker_gone())?
}

struct AsyncConnection {
    tx: mpsc::Sender<Cmd>,
    cancel: CancelHandle,
}

impl AsyncConnection {
    /// 发送普通命令。先发 `EndExec`，释放被丢弃的 `execute` future 留下的执行流：
    /// 那条语句仍会在工作线程上执行，若不释放，工作线程会一直停在执行流循环里，之后所有命令都会被拒绝。
    /// 工作线程空闲时 `EndExec` 是空操作；发送失败说明工作线程已退出，由随后的 `call` 报告。
    async fn request<T: Send>(&self, make: impl FnOnce(Reply<T>) -> Cmd) -> Result<T> {
        let _ = self.tx.send(Cmd::EndExec);
        call(&self.tx, make).await
    }
}

#[async_trait]
impl Connection for AsyncConnection {
    async fn execute<'a>(
        &'a mut self,
        sql: &'a str,
        params: &'a [Value],
    ) -> Result<Box<dyn ExecStream + 'a>> {
        let (sql, params) = (sql.to_string(), params.to_vec());
        self.request(|reply| Cmd::Execute { sql, params, reply }).await?;
        Ok(Box::new(AsyncExec { tx: &self.tx, finished: false }))
    }

    fn cancel_handle(&self) -> CancelHandle {
        self.cancel.clone()
    }

    fn set_lob_limit(&mut self, bytes: usize) {
        // 与 request 一样先释放被丢弃的执行流；发送失败说明工作线程已退出，下一条命令会报告
        let _ = self.tx.send(Cmd::EndExec);
        let _ = self.tx.send(Cmd::SetLobLimit { bytes });
    }

    async fn set_autocommit(&mut self, on: bool) -> Result<()> {
        self.request(|reply| Cmd::SetAutocommit { on, reply }).await
    }

    async fn commit(&mut self) -> Result<()> {
        self.request(|reply| Cmd::Commit { reply }).await
    }

    async fn rollback(&mut self) -> Result<()> {
        self.request(|reply| Cmd::Rollback { reply }).await
    }

    async fn ping(&mut self) -> Result<()> {
        self.request(|reply| Cmd::Ping { reply }).await
    }

    async fn server_info(&mut self) -> Result<ServerInfo> {
        self.request(|reply| Cmd::ServerInfo { reply }).await
    }

    async fn close(self: Box<Self>) -> Result<()> {
        self.request(|reply| Cmd::Close { reply }).await
    }
}

struct AsyncExec<'a> {
    tx: &'a mpsc::Sender<Cmd>,
    finished: bool,
}

#[async_trait]
impl ExecStream for AsyncExec<'_> {
    async fn next(&mut self, max_rows: usize) -> Result<Option<ExecEvent>> {
        if self.finished {
            return Ok(None);
        }
        let result = call(self.tx, |reply| Cmd::Next { max_rows, reply }).await;
        if !matches!(result, Ok(Some(_))) {
            self.finished = true;
        }
        result
    }
}

impl Drop for AsyncExec<'_> {
    fn drop(&mut self) {
        if !self.finished {
            let _ = self.tx.send(Cmd::EndExec);
        }
    }
}
