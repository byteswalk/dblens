//! 全应用唯一的 Java 进程：第一次连接时启动，退出后下一次连接重新启动。
//! 主程序退出时 stdin 关闭，Java 端随之退出，不留孤儿进程。

use std::collections::HashMap;
use std::io::{BufReader, BufWriter};
use std::path::PathBuf;
use std::process::{ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{SyncSender, sync_channel};
use std::sync::{Arc, Mutex};

use dblens_core::{Error, Result, msg};

use crate::frame::{Req, encode_req, read_frame, write_frame};

type Pending = Arc<Mutex<HashMap<u64, SyncSender<(u8, Vec<u8>)>>>>;

pub(crate) struct Bridge {
    stdin: Mutex<BufWriter<ChildStdin>>,
    pending: Pending,
    next: AtomicU64,
    dead: Arc<AtomicBool>,
}

impl Bridge {
    fn start(java: &PathBuf, jar: &PathBuf) -> Result<Arc<Bridge>> {
        let mut cmd = Command::new(java);
        cmd.args(["-Xss512k", "-XX:+UseSerialGC", "-Dfile.encoding=UTF-8", "-jar"])
            .arg(jar)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            // 桌面版没有控制台，不要弹黑窗
            cmd.creation_flags(0x0800_0000);
        }
        let mut child = cmd.spawn().map_err(|e| Error::DriverUnavailable {
            hint: msg!("jdbc.start_failed", path = java.display(), error = e),
        })?;
        let stdin = child.stdin.take().expect("已设置 piped");
        let stdout = child.stdout.take().expect("已设置 piped");
        let stderr = child.stderr.take().expect("已设置 piped");
        let pending: Pending = Arc::default();
        let dead = Arc::new(AtomicBool::new(false));
        let bridge = Arc::new(Bridge {
            stdin: Mutex::new(BufWriter::new(stdin)),
            pending: pending.clone(),
            next: AtomicU64::new(1),
            dead: dead.clone(),
        });
        std::thread::Builder::new()
            .name("dblens-bridge-log".into())
            .spawn(move || {
                use std::io::BufRead;
                for line in BufReader::new(stderr).lines().map_while(std::result::Result::ok) {
                    tracing::warn!(target: "bridge", "{line}");
                }
            })
            .map_err(|e| Error::internal(msg!("driver.worker_spawn_failed", error = e)))?;
        std::thread::Builder::new()
            .name("dblens-bridge-read".into())
            .spawn(move || {
                let mut r = BufReader::new(stdout);
                while let Ok((id, kind, body)) = read_frame(&mut r) {
                    if let Some(tx) = pending.lock().expect("pending 锁").remove(&id) {
                        let _ = tx.send((kind, body));
                    }
                }
                dead.store(true, Ordering::Relaxed);
                // 丢弃发送端，所有等待者收到断开
                pending.lock().expect("pending 锁").clear();
                let _ = child.wait();
            })
            .map_err(|e| Error::internal(msg!("driver.worker_spawn_failed", error = e)))?;
        Ok(bridge)
    }

    /// 发一条请求并等响应，返回（类型，内容）。在连接的专用线程上调用。
    pub(crate) fn call(&self, req: &Req) -> Result<(u8, Vec<u8>)> {
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        let frame = encode_req(id, req)?;
        let (tx, rx) = sync_channel(1);
        self.pending.lock().expect("pending 锁").insert(id, tx);
        let sent = {
            let mut w = self.stdin.lock().expect("stdin 锁");
            write_frame(&mut *w, &frame)
        };
        if sent.is_err() {
            self.pending.lock().expect("pending 锁").remove(&id);
            return Err(Error::connect(msg!("jdbc.bridge_exited")));
        }
        rx.recv().map_err(|_| Error::connect(msg!("jdbc.bridge_exited")))
    }

    fn alive(&self) -> bool {
        !self.dead.load(Ordering::Relaxed)
    }
}

static BRIDGE: Mutex<Option<Arc<Bridge>>> = Mutex::new(None);

/// 取得运行中的进程，没有或已退出时启动。
pub(crate) fn bridge(java: &PathBuf, jar: &PathBuf) -> Result<Arc<Bridge>> {
    let mut slot =
        BRIDGE.lock().map_err(|_| Error::internal(msg!("internal.lock_poisoned", lock = "bridge")))?;
    if let Some(b) = slot.as_ref().filter(|b| b.alive()) {
        return Ok(b.clone());
    }
    let b = Bridge::start(java, jar)?;
    *slot = Some(b.clone());
    Ok(b)
}
