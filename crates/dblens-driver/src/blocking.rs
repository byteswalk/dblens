//! 把同步（阻塞）驱动包装成异步 `Connection`：每个连接一个专用线程，通过命令通道交互。
//! 达梦（DPI）、Oracle（ODPI-C）以及以后的 JDBC 桥都用这个适配器。
//! 结果集等借用连接的状态只存在于工作线程的栈上，因此不需要自引用结构。

use std::sync::mpsc;
use std::thread;

use async_trait::async_trait;
use dblens_core::{Error, Result, Value};
use tokio::sync::oneshot;

use crate::{CancelHandle, Connection, ExecEvent, ExecStream, ServerInfo};

/// 同步的执行流。
pub trait BlockingExec {
    fn next(&mut self, max_rows: usize) -> Result<Option<ExecEvent>>;
}

/// 同步的连接。所有方法都只在该连接的专用线程上调用。
pub trait BlockingConnection: Send + 'static {
    fn execute<'a>(&'a mut self, sql: &'a str, params: &'a [Value]) -> Result<Box<dyn BlockingExec + 'a>>;
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
        let busy = || Error::internal("上一条语句的结果尚未读完");
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
            Cmd::EndExec => {}
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
        .map_err(|e| Error::internal(format!("无法创建连接工作线程：{e}")))?;
    Ok(Box::new(AsyncConnection { tx, cancel }))
}

fn worker_gone() -> Error {
    Error::connect("连接工作线程已退出")
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

#[async_trait]
impl Connection for AsyncConnection {
    async fn execute<'a>(
        &'a mut self,
        sql: &'a str,
        params: &'a [Value],
    ) -> Result<Box<dyn ExecStream + 'a>> {
        let (sql, params) = (sql.to_string(), params.to_vec());
        call(&self.tx, |reply| Cmd::Execute { sql, params, reply }).await?;
        Ok(Box::new(AsyncExec { tx: &self.tx, finished: false }))
    }

    fn cancel_handle(&self) -> CancelHandle {
        self.cancel.clone()
    }

    async fn set_autocommit(&mut self, on: bool) -> Result<()> {
        call(&self.tx, |reply| Cmd::SetAutocommit { on, reply }).await
    }

    async fn commit(&mut self) -> Result<()> {
        call(&self.tx, |reply| Cmd::Commit { reply }).await
    }

    async fn rollback(&mut self) -> Result<()> {
        call(&self.tx, |reply| Cmd::Rollback { reply }).await
    }

    async fn ping(&mut self) -> Result<()> {
        call(&self.tx, |reply| Cmd::Ping { reply }).await
    }

    async fn server_info(&mut self) -> Result<ServerInfo> {
        call(&self.tx, |reply| Cmd::ServerInfo { reply }).await
    }

    async fn close(self: Box<Self>) -> Result<()> {
        call(&self.tx, |reply| Cmd::Close { reply }).await
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
