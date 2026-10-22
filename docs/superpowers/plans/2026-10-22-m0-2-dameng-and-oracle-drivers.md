# M0-2 达梦与 Oracle 驱动 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 实现达梦（DM8，经 DPI）和 Oracle（经 `oracle` crate / ODPI-C）两个驱动。两者在运行时加载用户本机的客户端库，并通过与 MySQL/PG 相同的 9 项一致性测试。

**Architecture:**
- 两个底层客户端库都是同步阻塞接口。在 `dblens-driver` 中新增"阻塞适配器"：每个连接一个专用线程加命令通道，对外仍是异步的 `Connection`，结果集的状态全部保存在工作线程的栈上。
- 新增"原生库查找"：按"设置中的目录 → 环境变量 → 常见安装位置"的顺序找到客户端库。找不到时返回 `DriverUnavailable`，并附上安装提示。
- 达梦的 FFI 绑定用 bindgen 从官方头文件生成一次，然后提交到仓库；生成的代码通过 `libloading` 在运行时加载 `dmdpi`。
- 达梦部分以一个**技术验证任务**开头，它是是否改走 ODBC 的决策点。

**Tech Stack:** Rust 1.99、tokio、libloading 0.9、bindgen-cli 0.73（只在生成绑定时用）、oracle 0.6.3（chrono 特性）、chrono。

**Spec:** `docs/superpowers/specs/2026-10-22-dblens-design.md`（第 5.1–5.3、8、9、12 节）。总览：`docs/superpowers/plans/2026-10-22-m0-overview.md`。

## Global Constraints

- 沿用 M0-1 的全部约束：仓库根目录 `E:\VibeCoding\16数据库管理工具\dblens\`、Rust 1.99.0、edition 2024、依赖版本统一在工作区声明、错误一律转换为 `dblens_core::Error`、中文注释和文案、提交时附加 `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`。
- **客户端库一律在运行时加载，不在编译期链接**：没装达梦或 Oracle 客户端的机器也必须能编译、能启动；该驱动报 `DriverUnavailable` 即可。
- **客户端库不随仓库分发**：仓库中不得提交任何 `dmdpi`、`oci` 等二进制文件。达梦头文件也不提交，只提交由它生成的 `ffi.rs`。
- **集成测试使用的环境变量**：`DBLENS_TEST_DM_URL`（如 `dm://SYSDBA:密码@主机:5236`）、`DBLENS_TEST_ORACLE_URL`（如 `oracle://用户:密码@主机:1521/FREEPDB1`）。客户端库目录通过 URL 查询参数指定：达梦用 `dpi_lib_dir`，Oracle 用 `oracle_client_lib_dir`；没有指定时按查找顺序自动寻找。
- **完成标准**：Task 5（Oracle）和 Task 4（达梦）必须在真实数据库上跑通一致性测试，并且没有"跳过"；单元测试不能代替。
- **前置条件**（执行到对应任务前由用户准备）：
  - Task 3：本机安装达梦客户端（提供 `DM_HOME/include/DPI.h`、`DPIext.h`、`DPItypes.h` 和 `DM_HOME/bin/dmdpi.dll`）；安装 LLVM，供 bindgen 使用（`winget install LLVM.LLVM`）；准备一个可连接的 DM8 测试库。
  - Task 5：本机解压 Oracle Instant Client（Basic 或 Basic Light，Windows x64），例如放到 `C:\oracle\instantclient_23_x`；准备一个 Oracle 测试库（Oracle Database 23ai Free 即可），并建好测试用户。

## 已核实的外部事实（编写本计划时查证）

- `dmdpi` 导出了 `dpi_cancel`、`dpi_kill_con`、`dpi_server_is_alive` 等 180 个函数（从 dmPython 2.6.1 的 Windows wheel 中读取导出表得到）。dmPython 的源码里 `cancel` 被注释掉了，**`dpi_cancel` 能否从其他线程安全调用，要在 Task 3 中验证**。
- dmPython 的调用方式（Gitee DamengDB/dmPython，木兰宽松许可证 v2）：
  - 先用 `dpi_alloc_env` → `dpi_alloc_con` 分配句柄。
  - 登录前用 `dpi_set_con_attr` 设置 `DSQL_ATTR_LOCAL_CODE`（值 `PG_UTF8`）和 `DSQL_ATTR_LOGIN_PORT`，再调用 `dpi_login(hcon, 主机, 用户, 密码)`。
  - 执行 SQL：`dpi_exec_direct`，或 `dpi_prepare` + `dpi_bind_param` + `dpi_exec`。
  - 读取结果：`dpi_number_columns(hstmt, udint2*)`、`dpi_desc_column(hstmt, 列号, name, buflen, &namelen, &sql_type:sdint2, &prec:ulength, &scale:sdint2, &nullable:sdint2)`、`dpi_fetch(hstmt, ulength*)`、`dpi_get_data(hstmt, 列号, c类型, buf, buflen, slength*)`。
  - 其他：`dpi_row_count(hstmt, sdint8*)`；错误信息用 `dpi_get_diag_rec(句柄类型, 句柄, 1, &code:sdint4, msg, buflen, &len:sdint2)`。
- `oracle` 0.6.3：
  - `Connection` 是 Send + Sync，`break_execution(&self)` 可以从其他线程打断正在执行的查询。
  - `InitParams::oracle_client_lib_dir` 用来指定客户端库目录，进程内只能初始化一次。
  - `Statement::into_result_set` 返回一个自己持有语句的 `ResultSet<'static, Row>`。
  - `is_query()`、`is_plsql()`、`is_ddl()` 判断语句类型；`OracleType` 实现了 `Display`。
- 已有纯 Rust 的达梦和 Oracle 驱动（`dm-pure`、`tokio-dameng`、`oracle-rs`），但都出现不到一年，下载量很小，本期不采用。`oracle-rs` 记为以后支持龙芯平台的候选。

---

## File Structure

```
Cargo.toml                                    增加 libloading、oracle 依赖
crates/dblens-driver/
├── Cargo.toml                                tokio 改为常规依赖（sync 特性）
├── src/lib.rs                                导出 blocking、native 模块
├── src/blocking.rs                           阻塞适配器：BlockingConnection/BlockingExec → 异步 Connection
├── src/native.rs                             原生库查找：NativeLib、locate、locate_with
└── tests/blocking.rs                         适配器测试（内存假驱动）
crates/dblens-driver-dameng/
├── Cargo.toml
├── dpi/wrapper.h                             bindgen 输入（只 include 三个官方头文件）
├── dpi/README.md                             重新生成 ffi.rs 的命令
├── src/lib.rs                                DmDriver
├── src/ffi.rs                                bindgen 生成（DmDpi 动态加载结构体 + 类型 + 常量）
├── src/load.rs                               查找并加载 dmdpi，按目录缓存
├── src/handles.rs                            Env/Con/Stmt 的 RAII 封装、诊断信息、取数
├── src/types.rs                              DmType 分类、类型名、文本转 Value、参数转文本
├── src/conn.rs                               DmConnection（BlockingConnection）、DmExec（BlockingExec）
├── src/spike.rs                              技术验证测试（#[ignore]，手动运行）
└── tests/conformance.rs
crates/dblens-driver-oracle/
├── Cargo.toml
├── src/lib.rs                                OracleDriver、客户端初始化、连接串
├── src/conn.rs                               OracleConnection（BlockingConnection）、OraExec、DBMS_OUTPUT
├── src/value.rs                              列信息、值映射、参数绑定、数字规范化
├── src/error.rs                              map_err
└── tests/conformance.rs                      一致性测试 + DBMS_OUTPUT 测试
docs/notes/dameng-dpi-spike.md                技术验证结论
.github/workflows/ci.yml                      增加 Oracle 集成测试任务
```

---

### Task 1: 阻塞适配器（`dblens-driver::blocking`）

**Files:**
- Modify: `Cargo.toml`（工作区依赖）、`crates/dblens-driver/Cargo.toml`、`crates/dblens-driver/src/lib.rs`
- Create: `crates/dblens-driver/src/blocking.rs`
- Test: `crates/dblens-driver/tests/blocking.rs`

**Interfaces:**
- Consumes: M0-1 的 `Connection`、`ExecStream`、`ExecEvent`、`CancelHandle`、`ServerInfo`
- Produces:
  - `pub trait BlockingExec { fn next(&mut self, max_rows: usize) -> Result<Option<ExecEvent>>; }`
  - `pub trait BlockingConnection: Send + 'static`，方法：`execute<'a>(&'a mut self, sql: &'a str, params: &'a [Value]) -> Result<Box<dyn BlockingExec + 'a>>`、`set_autocommit(&mut self, bool)`、`commit`、`rollback`、`ping`、`server_info`、`close(self: Box<Self>)`
  - `pub fn into_async<C: BlockingConnection>(conn: C, cancel: CancelHandle, thread_name: &str) -> Result<Box<dyn Connection>>`

- [ ] **Step 1: 调整依赖**

在根目录 `Cargo.toml` 的 `[workspace.dependencies]` 中追加（按字母序插入）：
```toml
libloading = "0.9.0"
oracle = { version = "0.6.3", features = ["chrono"] }
```

把 `crates/dblens-driver/Cargo.toml` 的 `[features]` 和 tokio 依赖改为：
```toml
[features]
conformance = []

[dependencies]
async-trait.workspace = true
dblens-core.workspace = true
futures.workspace = true
percent-encoding.workspace = true
serde.workspace = true
tokio.workspace = true
url.workspace = true
```
（删除 `tokio = { workspace = true, optional = true }` 和 `conformance = ["dep:tokio"]`。阻塞适配器要用 `tokio::sync::oneshot`。）

- [ ] **Step 2: 写失败的测试**

`crates/dblens-driver/tests/blocking.rs`：
```rust
//! 用内存假驱动验证阻塞适配器：专用线程、分批取数、错误传递、取消、提前丢弃执行流、事务命令转发。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use dblens_core::{Error, Result, Value};
use dblens_driver::blocking::{BlockingConnection, BlockingExec, into_async};
use dblens_driver::{CancelHandle, ColumnMeta, ExecEvent, ServerInfo, collect_all};

struct Fake {
    log: Arc<Mutex<Vec<String>>>,
    cancel_flag: Arc<AtomicBool>,
}

struct FakeExec {
    rows_left: Vec<Vec<Value>>,
    started: bool,
    ended: bool,
}

impl BlockingExec for FakeExec {
    fn next(&mut self, max_rows: usize) -> Result<Option<ExecEvent>> {
        if !self.started {
            self.started = true;
            return Ok(Some(ExecEvent::ResultStart { columns: vec![ColumnMeta::new("n", "INT")] }));
        }
        if !self.rows_left.is_empty() {
            let take = max_rows.min(self.rows_left.len());
            return Ok(Some(ExecEvent::Rows(self.rows_left.drain(..take).collect())));
        }
        if !self.ended {
            self.ended = true;
            return Ok(Some(ExecEvent::ResultEnd));
        }
        Ok(None)
    }
}

impl BlockingConnection for Fake {
    fn execute<'a>(&'a mut self, sql: &'a str, _params: &'a [Value]) -> Result<Box<dyn BlockingExec + 'a>> {
        let thread = std::thread::current().name().unwrap_or("").to_string();
        self.log.lock().unwrap().push(format!("{thread}:{sql}"));
        if sql == "fail" {
            return Err(Error::Sql { code: "1".into(), message: "坏语句".into(), position: None });
        }
        if sql == "sleep" {
            let started = Instant::now();
            while !self.cancel_flag.swap(false, Ordering::SeqCst) {
                assert!(started.elapsed() < Duration::from_secs(5), "取消没有到达");
                std::thread::sleep(Duration::from_millis(10));
            }
            return Err(Error::Cancelled);
        }
        let n: i64 = sql.strip_prefix("rows:").expect("测试 SQL 格式为 rows:N").parse().unwrap();
        Ok(Box::new(FakeExec { rows_left: (1..=n).map(|i| vec![Value::Int(i)]).collect(), started: false, ended: false }))
    }
    fn set_autocommit(&mut self, on: bool) -> Result<()> {
        self.log.lock().unwrap().push(format!("autocommit={on}"));
        Ok(())
    }
    fn commit(&mut self) -> Result<()> {
        self.log.lock().unwrap().push("commit".into());
        Ok(())
    }
    fn rollback(&mut self) -> Result<()> {
        self.log.lock().unwrap().push("rollback".into());
        Ok(())
    }
    fn ping(&mut self) -> Result<()> {
        Ok(())
    }
    fn server_info(&mut self) -> Result<ServerInfo> {
        Ok(ServerInfo { product: "Fake".into(), version: "1".into() })
    }
    fn close(self: Box<Self>) -> Result<()> {
        self.log.lock().unwrap().push("close".into());
        Ok(())
    }
}

fn setup() -> (Box<dyn dblens_driver::Connection>, Arc<Mutex<Vec<String>>>) {
    let log = Arc::new(Mutex::new(Vec::new()));
    let flag = Arc::new(AtomicBool::new(false));
    let cancel = {
        let flag = flag.clone();
        CancelHandle::new(move || {
            let flag = flag.clone();
            async move {
                flag.store(true, Ordering::SeqCst);
                Ok(())
            }
        })
    };
    let conn = into_async(Fake { log: log.clone(), cancel_flag: flag }, cancel, "dblens-test-worker").unwrap();
    (conn, log)
}

#[tokio::test]
async fn streams_rows_in_batches_on_worker_thread() {
    let (mut conn, log) = setup();
    let mut stream = conn.execute("rows:5", &[]).await.unwrap();
    let events = collect_all(stream.as_mut(), 2).await.unwrap();
    let sizes: Vec<usize> =
        events.iter().filter_map(|e| if let ExecEvent::Rows(r) = e { Some(r.len()) } else { None }).collect();
    assert_eq!(sizes, vec![2, 2, 1]);
    assert_eq!(events.last(), Some(&ExecEvent::ResultEnd));
    assert_eq!(log.lock().unwrap()[0], "dblens-test-worker:rows:5");
}

#[tokio::test]
async fn execute_error_propagates_and_connection_survives() {
    let (mut conn, _) = setup();
    let err = conn.execute("fail", &[]).await.err().expect("应返回错误");
    assert!(matches!(err, Error::Sql { .. }), "{err:?}");
    conn.ping().await.unwrap();
    let mut stream = conn.execute("rows:1", &[]).await.unwrap();
    assert_eq!(collect_all(stream.as_mut(), 10).await.unwrap().len(), 3);
}

#[tokio::test]
async fn cancel_interrupts_running_statement() {
    let (mut conn, _) = setup();
    let handle = conn.cancel_handle();
    let canceller = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(200)).await;
        handle.cancel().await
    });
    let started = Instant::now();
    let err = conn.execute("sleep", &[]).await.err().expect("应被取消");
    canceller.await.unwrap().unwrap();
    assert_eq!(err, Error::Cancelled);
    assert!(started.elapsed() < Duration::from_secs(2));
    conn.ping().await.unwrap();
}

#[tokio::test]
async fn dropping_stream_early_allows_next_execute() {
    let (mut conn, _) = setup();
    {
        let mut stream = conn.execute("rows:10", &[]).await.unwrap();
        assert!(matches!(stream.next(3).await.unwrap(), Some(ExecEvent::ResultStart { .. })));
    }
    let mut stream = conn.execute("rows:3", &[]).await.unwrap();
    let rows: usize = collect_all(stream.as_mut(), 100)
        .await
        .unwrap()
        .iter()
        .map(|e| if let ExecEvent::Rows(r) = e { r.len() } else { 0 })
        .sum();
    assert_eq!(rows, 3);
}

#[tokio::test]
async fn transaction_calls_are_forwarded() {
    let (mut conn, log) = setup();
    conn.set_autocommit(false).await.unwrap();
    conn.commit().await.unwrap();
    conn.rollback().await.unwrap();
    assert_eq!(conn.server_info().await.unwrap().product, "Fake");
    conn.close().await.unwrap();
    assert_eq!(*log.lock().unwrap(), vec!["autocommit=false", "commit", "rollback", "close"]);
}
```

把 `crates/dblens-driver/src/lib.rs` 改为：
```rust
//! 驱动接口：所有数据库驱动都实现这里的 `Driver` / `Connection` / `ExecStream`。

pub mod blocking;
mod cancel;
mod exec;
pub mod native;
mod params;
mod traits;

#[cfg(feature = "conformance")]
pub mod conformance;

pub use cancel::CancelHandle;
pub use exec::{ColumnMeta, ExecEvent, ExecStream, collect_all};
pub use params::{Capabilities, ConnectParams, DriverId, ServerInfo};
pub use traits::{Connection, Driver};
```
同时创建空文件 `crates/dblens-driver/src/native.rs`，内容只有一行 `//! 原生客户端库查找（Task 2 实现）。`，以便本任务能编译。

- [ ] **Step 3: 运行测试，确认失败**

Run: `cargo test -p dblens-driver --test blocking`
Expected: 编译失败，提示 `blocking` 模块不存在（或找不到 `into_async`）。

- [ ] **Step 4: 实现 `blocking.rs`**

`crates/dblens-driver/src/blocking.rs`：
```rust
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
    Execute { sql: String, params: Vec<Value>, reply: Reply<()> },
    Next { max_rows: usize, reply: Reply<Option<ExecEvent>> },
    /// 异步侧提前丢弃了执行流。
    EndExec,
    SetAutocommit { on: bool, reply: Reply<()> },
    Commit { reply: Reply<()> },
    Rollback { reply: Reply<()> },
    Ping { reply: Reply<()> },
    ServerInfo { reply: Reply<ServerInfo> },
    Close { reply: Reply<()> },
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
```

- [ ] **Step 5: 运行测试，确认通过**

Run: `cargo test -p dblens-driver --all-features`
Expected: 新增的 5 个 blocking 测试 PASS；M0-1 原有测试仍然 PASS。

Run: `cargo clippy -p dblens-driver --all-features --all-targets -- -D warnings`
Expected: 没有警告。

- [ ] **Step 6: 提交**

```bash
git add Cargo.toml Cargo.lock crates/dblens-driver
git commit -m "feat(driver): 阻塞驱动适配器（专用线程 + 命令通道）" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 2: 原生库查找（`dblens-driver::native`）

**Files:**
- Modify: `crates/dblens-driver/src/native.rs`

**Interfaces:**
- Consumes: `dblens_core::Error`
- Produces:
  - `pub enum NativeLib { OracleClient, DamengDpi }`，带 `pub fn file_name(self) -> &'static str`
  - `pub fn locate(lib: NativeLib, configured: Option<&Path>) -> Result<PathBuf>`（返回库**所在目录**）
  - `pub fn locate_with(lib, configured: Option<&Path>, env: &dyn Fn(&str) -> Option<OsString>, common: &[PathBuf]) -> Result<PathBuf>`（测试用：可注入环境变量和常见目录）
  - `pub fn expand_common(patterns: &[&str]) -> Vec<PathBuf>`（以 `*` 结尾的模式按前缀展开为同级子目录，版本号大的排在前面）

- [ ] **Step 1: 写失败的测试**

把 `crates/dblens-driver/src/native.rs` 的内容替换为下面的测试模块（实现放在 Step 3 中，写在测试模块之前）：
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("dblens-native-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn put_lib(dir: &Path, lib: NativeLib) {
        fs::create_dir_all(dir).unwrap();
        fs::write(dir.join(lib.file_name()), b"").unwrap();
    }

    fn no_env(_: &str) -> Option<OsString> {
        None
    }

    #[test]
    fn configured_dir_is_the_only_place_searched() {
        let root = temp("configured");
        let good = root.join("good");
        put_lib(&good, NativeLib::DamengDpi);
        assert_eq!(locate_with(NativeLib::DamengDpi, Some(good.as_path()), &no_env, &[]).unwrap(), good);

        // 指定了目录但里面没有库：即使环境变量里能找到，也要报错
        let empty = root.join("empty");
        fs::create_dir_all(&empty).unwrap();
        let env = |k: &str| (k == "DM_HOME").then(|| good.clone().into_os_string());
        let err = locate_with(NativeLib::DamengDpi, Some(empty.as_path()), &env, &[]).unwrap_err();
        assert!(matches!(err, Error::DriverUnavailable { .. }), "{err:?}");
    }

    #[test]
    fn home_variable_subdirectories() {
        let home = temp("home");
        put_lib(&home.join("bin"), NativeLib::DamengDpi);
        let env = |k: &str| (k == "DM_HOME").then(|| home.clone().into_os_string());
        assert_eq!(locate_with(NativeLib::DamengDpi, None, &env, &[]).unwrap(), home.join("bin"));
    }

    #[test]
    fn search_path_variable() {
        let root = temp("path");
        let a = root.join("a");
        let b = root.join("b");
        fs::create_dir_all(&a).unwrap();
        put_lib(&b, NativeLib::OracleClient);
        let joined = std::env::join_paths([&a, &b]).unwrap();
        let var = if cfg!(windows) {
            "PATH"
        } else if cfg!(target_os = "macos") {
            "DYLD_LIBRARY_PATH"
        } else {
            "LD_LIBRARY_PATH"
        };
        let env = move |k: &str| (k == var).then(|| joined.clone());
        assert_eq!(locate_with(NativeLib::OracleClient, None, &env, &[]).unwrap(), b);
    }

    #[test]
    fn common_dirs_and_prefix_expansion_prefers_newest() {
        let root = temp("common");
        put_lib(&root.join("instantclient_19_8"), NativeLib::OracleClient);
        put_lib(&root.join("instantclient_23_5"), NativeLib::OracleClient);
        let pattern = format!("{}*", root.join("instantclient_").display());
        let dirs = expand_common(&[pattern.as_str()]);
        assert_eq!(dirs, vec![root.join("instantclient_23_5"), root.join("instantclient_19_8")]);
        assert_eq!(locate_with(NativeLib::OracleClient, None, &no_env, &dirs).unwrap(), root.join("instantclient_23_5"));
    }

    #[test]
    fn not_found_hint_lists_tried_dirs() {
        let root = temp("missing");
        let err = locate_with(NativeLib::DamengDpi, None, &no_env, &[root.clone()]).unwrap_err();
        let Error::DriverUnavailable { hint } = err else { panic!("应为 DriverUnavailable") };
        assert!(hint.contains(NativeLib::DamengDpi.file_name()), "{hint}");
        assert!(hint.contains(&root.display().to_string()), "{hint}");
    }
}
```

- [ ] **Step 2: 运行测试，确认失败**

Run: `cargo test -p dblens-driver --lib native`
Expected: 编译失败，提示找不到 `NativeLib`、`locate_with` 等。

- [ ] **Step 3: 实现**

在 `native.rs` 的测试模块之前写入：
```rust
//! 查找运行时动态加载的原生客户端库（Oracle Instant Client、达梦 DPI）。
//! 查找顺序：设置中指定的目录（指定后只查这一处）→ 专属环境变量 → 库搜索路径变量 → 常见安装位置。

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use dblens_core::{Error, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeLib {
    OracleClient,
    DamengDpi,
}

impl NativeLib {
    /// 当前平台上的库文件名。
    pub fn file_name(self) -> &'static str {
        match self {
            NativeLib::OracleClient => {
                if cfg!(windows) {
                    "oci.dll"
                } else if cfg!(target_os = "macos") {
                    "libclntsh.dylib"
                } else {
                    "libclntsh.so"
                }
            }
            NativeLib::DamengDpi => {
                if cfg!(windows) {
                    "dmdpi.dll"
                } else if cfg!(target_os = "macos") {
                    "libdmdpi.dylib"
                } else {
                    "libdmdpi.so"
                }
            }
        }
    }

    fn display_name(self) -> &'static str {
        match self {
            NativeLib::OracleClient => "Oracle 客户端库",
            NativeLib::DamengDpi => "达梦 DPI 库",
        }
    }

    fn install_hint(self) -> &'static str {
        match self {
            NativeLib::OracleClient => {
                "请安装 Oracle Instant Client（Basic 或 Basic Light），然后在连接设置中指定其目录，或设置 ORACLE_HOME / PATH"
            }
            NativeLib::DamengDpi => {
                "请安装达梦客户端（含 DPI 驱动），然后在连接设置中指定 dmdpi 所在目录，或设置 DM_HOME"
            }
        }
    }

    /// （环境变量，其下要查找的子目录）
    fn home_vars(self) -> &'static [(&'static str, &'static [&'static str])] {
        match self {
            NativeLib::OracleClient => &[("ORACLE_HOME", &["", "bin", "lib"])],
            NativeLib::DamengDpi => &[("DM_HOME", &["bin", "drivers/dpi"])],
        }
    }

    /// 常见安装位置。以 `*` 结尾表示按前缀匹配同级的所有子目录。
    fn common_patterns(self) -> &'static [&'static str] {
        match self {
            NativeLib::OracleClient => {
                if cfg!(windows) {
                    &["C:\\oracle\\instantclient*", "C:\\instantclient*", "D:\\oracle\\instantclient*"]
                } else if cfg!(target_os = "macos") {
                    &["/opt/oracle/instantclient*"]
                } else {
                    &[
                        "/opt/oracle/instantclient*",
                        "/usr/lib/oracle/23/client64/lib",
                        "/usr/lib/oracle/21/client64/lib",
                        "/usr/lib/oracle/19/client64/lib",
                    ]
                }
            }
            NativeLib::DamengDpi => {
                if cfg!(windows) {
                    &["C:\\dmdbms\\bin", "D:\\dmdbms\\bin", "C:\\dmdbms\\drivers\\dpi"]
                } else if cfg!(target_os = "macos") {
                    &[]
                } else {
                    &["/opt/dmdbms/bin", "/home/dmdba/dmdbms/bin", "/dm8/bin"]
                }
            }
        }
    }
}

fn search_path_var() -> &'static str {
    if cfg!(windows) {
        "PATH"
    } else if cfg!(target_os = "macos") {
        "DYLD_LIBRARY_PATH"
    } else {
        "LD_LIBRARY_PATH"
    }
}

/// 查找库所在的目录。
pub fn locate(lib: NativeLib, configured: Option<&Path>) -> Result<PathBuf> {
    let common = expand_common(lib.common_patterns());
    locate_with(lib, configured, &|k| std::env::var_os(k), &common)
}

/// 可注入环境变量和常见目录的查找，供测试使用。
pub fn locate_with(
    lib: NativeLib,
    configured: Option<&Path>,
    env: &dyn Fn(&str) -> Option<OsString>,
    common: &[PathBuf],
) -> Result<PathBuf> {
    let candidates: Vec<PathBuf> = match configured {
        Some(dir) => vec![dir.to_path_buf()],
        None => {
            let mut c = Vec::new();
            for (var, subdirs) in lib.home_vars() {
                if let Some(home) = env(var) {
                    c.extend(subdirs.iter().map(|s| PathBuf::from(&home).join(s)));
                }
            }
            if let Some(paths) = env(search_path_var()) {
                c.extend(std::env::split_paths(&paths));
            }
            c.extend(common.iter().cloned());
            c
        }
    };
    candidates.iter().find(|d| d.join(lib.file_name()).is_file()).cloned().ok_or_else(|| {
        let tried: Vec<String> = candidates.iter().map(|p| p.display().to_string()).collect();
        Error::DriverUnavailable {
            hint: format!(
                "未找到{}（{}）。{}。已查找：{}",
                lib.display_name(),
                lib.file_name(),
                lib.install_hint(),
                tried.join("；")
            ),
        }
    })
}

/// 展开常见安装位置。`前缀*` 匹配同级目录中名字以该前缀开头的子目录，按名称倒序（新版本在前）。
pub fn expand_common(patterns: &[&str]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for pattern in patterns {
        let Some(prefix) = pattern.strip_suffix('*') else {
            out.push(PathBuf::from(pattern));
            continue;
        };
        let prefix = Path::new(prefix);
        let (Some(parent), Some(stem)) = (prefix.parent(), prefix.file_name()) else { continue };
        let stem = stem.to_string_lossy().into_owned();
        let Ok(entries) = std::fs::read_dir(parent) else { continue };
        let mut found: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.is_dir() && p.file_name().is_some_and(|n| n.to_string_lossy().starts_with(&stem)))
            .collect();
        found.sort();
        found.reverse();
        out.extend(found);
    }
    out
}
```

- [ ] **Step 4: 运行测试，确认通过**

Run: `cargo test -p dblens-driver --lib native && cargo clippy -p dblens-driver --all-features --all-targets -- -D warnings`
Expected: 5 个 native 测试 PASS，没有警告。

- [ ] **Step 5: 提交**

```bash
git add crates/dblens-driver
git commit -m "feat(driver): 原生客户端库查找（设置目录、环境变量、常见安装位置）" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 3: 达梦 DPI 绑定与技术验证（决策点）

**Files:**
- Create: `crates/dblens-driver-dameng/Cargo.toml`、`dpi/wrapper.h`、`dpi/README.md`
- Create: `crates/dblens-driver-dameng/src/{lib.rs,ffi.rs,load.rs,handles.rs,spike.rs}`
- Create: `docs/notes/dameng-dpi-spike.md`

**Interfaces:**
- Consumes: Task 2 的 `native::locate`、`NativeLib::DamengDpi`
- Produces（crate 内部）：
  - `ffi::DmDpi`（bindgen 动态加载结构体，方法名即 DPI 函数名）以及头文件中的类型别名（`dhenv`、`dhcon`、`dhstmt`、`dhandle`、`sdint2`、`udint2`、`sdint4`、`sdint8`、`slength`、`ulength`、`sdbyte`、`dpointer`、`DPIRETURN`）和常量（`DSQL_*`、`PG_*`）
  - `load::load(params: &ConnectParams) -> Result<Arc<DmDpi>>`
  - `handles::succeeded(rt: DPIRETURN) -> bool`
  - `handles::Env`、`handles::Con`（`new`、`set_attr_int`、`login`、`commit`、`rollback`、`set_autocommit`、`dpi()`、`h`）
  - `handles::Stmt<'c>`（`new`、`exec_direct`、`prepare`、`exec`、`num_columns`、`describe`、`fetch`、`get_data`、`row_count`、`h`、`check`）
  - `handles::ColDesc { name: String, sql_type: i32, precision: u64, scale: i32, nullable: bool }`

**本任务是决策点**：Step 6 的四项验证中，"连接与中文"和"取消"任一项失败，就停止执行本计划，把结论写进笔记交给用户，并改写 Task 4 为 ODBC 方案（`odbc-api` crate）。其余两项（LOB、错误码）失败只需记录，在 Task 4 中处理。

- [ ] **Step 1: 确认前置条件**

```powershell
Test-Path "$env:DM_HOME\include\DPI.h"; Test-Path "$env:DM_HOME\include\DPIext.h"; Test-Path "$env:DM_HOME\include\DPItypes.h"; Test-Path "$env:DM_HOME\bin\dmdpi.dll"; clang --version
```
Expected: 四个 `True`，并且能看到 clang 版本。缺少任何一项，就暂停并请用户按 Global Constraints 中的前置条件安装。

安装 bindgen 命令行工具（只在生成绑定时需要）：
```bash
cargo install bindgen-cli --version 0.73.2 --locked
```

- [ ] **Step 2: 创建 crate 骨架和 bindgen 输入**

`crates/dblens-driver-dameng/Cargo.toml`：
```toml
[package]
name = "dblens-driver-dameng"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
publish.workspace = true

[dependencies]
async-trait.workspace = true
chrono.workspace = true
dblens-core.workspace = true
dblens-driver.workspace = true
libloading.workspace = true
tokio.workspace = true

[dev-dependencies]
dblens-driver = { workspace = true, features = ["conformance"] }
tokio.workspace = true
```

`crates/dblens-driver-dameng/dpi/wrapper.h`：
```c
/* bindgen 输入：只包含达梦官方 DPI 头文件。头文件本身不提交，来自 DM_HOME/include。 */
#include "DPI.h"
#include "DPIext.h"
#include "DPItypes.h"
```

`crates/dblens-driver-dameng/dpi/README.md`：
````markdown
# 达梦 DPI 绑定

`src/ffi.rs` 由 bindgen 从达梦官方头文件生成，生成后提交到仓库，平时编译不需要头文件和 libclang。
达梦客户端升级、需要新的 DPI 函数时，按下面的命令重新生成（需要 LLVM 和 `cargo install bindgen-cli --version 0.73.2`）：

```bash
bindgen crates/dblens-driver-dameng/dpi/wrapper.h \
  --dynamic-loading DmDpi \
  --allowlist-function "dpi_.*" \
  --allowlist-var "DSQL_.*|PG_.*" \
  --no-layout-tests \
  --raw-line "#![allow(non_upper_case_globals, non_camel_case_types, non_snake_case, dead_code, clippy::all)]" \
  -o crates/dblens-driver-dameng/src/ffi.rs \
  -- -I "$DM_HOME/include"
```

生成所用的达梦客户端版本记录在 `docs/notes/dameng-dpi-spike.md`。
````

- [ ] **Step 3: 生成 `ffi.rs` 并检查**

运行 `dpi/README.md` 中的 bindgen 命令（PowerShell 下把 `$DM_HOME` 写成 `$env:DM_HOME`，续行符改为反引号或写成一行）。

检查生成结果：
```bash
grep -c "pub unsafe fn dpi_" crates/dblens-driver-dameng/src/ffi.rs
grep -E "pub struct DmDpi|pub unsafe fn (dpi_alloc_env|dpi_login|dpi_exec_direct|dpi_cancel|dpi_get_data|dpi_desc_column|dpi_get_diag_rec)\b" crates/dblens-driver-dameng/src/ffi.rs
grep -E "pub const (DSQL_SUCCESS|DSQL_SUCCESS_WITH_INFO|DSQL_NO_DATA|DSQL_NULL_DATA|DSQL_HANDLE_DBC|DSQL_HANDLE_STMT|DSQL_ATTR_LOCAL_CODE|DSQL_ATTR_LOGIN_PORT|DSQL_ATTR_LOGIN_TIMEOUT|DSQL_ATTR_AUTOCOMMIT|PG_UTF8|DSQL_C_NCHAR|DSQL_C_BINARY|DSQL_PARAM_INPUT|DSQL_VARCHAR|DSQL_VARBINARY)\b" crates/dblens-driver-dameng/src/ffi.rs
```
Expected: 函数数量 ≥ 100；`DmDpi` 结构体和列出的 7 个函数都存在；列出的 16 个常量都存在。

**如果某个函数或常量的名字与此不同**（例如 bindgen 把宏定义识别成了别的名字），以 `ffi.rs` 为准，在本任务和 Task 4 的代码中统一替换。

- [ ] **Step 4: 实现加载与句柄封装**

`crates/dblens-driver-dameng/src/lib.rs`（本任务的临时版本，Task 4 再补充驱动本体）：
```rust
//! 达梦驱动：通过 DPI（运行时加载 dmdpi）访问 DM8。

#[allow(clippy::all)]
mod ffi;
mod handles;
mod load;

#[cfg(test)]
mod spike;
```

`crates/dblens-driver-dameng/src/load.rs`：
```rust
//! 查找并加载 dmdpi。同一目录只加载一次，进程内共享。

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use dblens_core::{Error, Result};
use dblens_driver::ConnectParams;
use dblens_driver::native::{self, NativeLib};

use crate::ffi::DmDpi;

static LOADED: Mutex<Vec<(PathBuf, Arc<DmDpi>)>> = Mutex::new(Vec::new());

/// 按连接参数中的 `dpi_lib_dir` 选项（或自动查找）加载 DPI 库。
pub(crate) fn load(params: &ConnectParams) -> Result<Arc<DmDpi>> {
    let configured = params.options.get("dpi_lib_dir").map(PathBuf::from);
    let dir = native::locate(NativeLib::DamengDpi, configured.as_deref())?;
    let mut loaded = LOADED.lock().map_err(|_| Error::internal("DPI 加载锁已损坏"))?;
    if let Some((_, dpi)) = loaded.iter().find(|(d, _)| *d == dir) {
        return Ok(dpi.clone());
    }
    let path = dir.join(NativeLib::DamengDpi.file_name());
    let library = open_library(&path)?;
    let dpi = unsafe { DmDpi::from_library(library) }.map_err(|e| unavailable(&path, e))?;
    let dpi = Arc::new(dpi);
    loaded.push((dir, dpi.clone()));
    Ok(dpi)
}

fn unavailable(path: &Path, e: impl std::fmt::Display) -> Error {
    Error::DriverUnavailable { hint: format!("加载 {} 失败：{e}", path.display()) }
}

#[cfg(windows)]
fn open_library(path: &Path) -> Result<libloading::Library> {
    use libloading::os::windows::{LOAD_WITH_ALTERED_SEARCH_PATH, Library};
    // 让 dmdpi.dll 的依赖（dmcalc.dll 等）从它所在的目录加载
    unsafe { Library::load_with_flags(path, LOAD_WITH_ALTERED_SEARCH_PATH) }
        .map(Into::into)
        .map_err(|e| unavailable(path, e))
}

#[cfg(unix)]
fn open_library(path: &Path) -> Result<libloading::Library> {
    // Linux 上 libdmdpi.so 的依赖要靠 LD_LIBRARY_PATH 找到，结论见技术验证笔记
    unsafe { libloading::Library::new(path) }.map_err(|e| unavailable(path, e))
}
```

`crates/dblens-driver-dameng/src/handles.rs`：
```rust
//! DPI 句柄的 RAII 封装。头文件中的整数类型在不同版本间可能有差异，
//! 因此传参一律用 `as _` 转换；输出参数的局部变量类型若与 ffi.rs 不一致，以 ffi.rs 为准调整。

use std::ffi::CString;
use std::sync::Arc;

use dblens_core::{Error, Result};

use crate::ffi::*;

/// 文本数据每次读取的块大小。
const CHUNK: usize = 64 * 1024;

pub(crate) fn succeeded(rt: DPIRETURN) -> bool {
    let rt = rt as i64;
    rt == DSQL_SUCCESS as i64 || rt == DSQL_SUCCESS_WITH_INFO as i64
}

fn is(rt: DPIRETURN, code: i64) -> bool {
    rt as i64 == code
}

fn cstring(s: &str) -> Result<CString> {
    CString::new(s).map_err(|_| Error::internal("字符串中不能包含 \\0 字符"))
}

/// 读取句柄上的第一条诊断记录，转换为错误。
pub(crate) fn diag(dpi: &DmDpi, handle_type: i64, handle: dhandle) -> Error {
    let mut code: sdint4 = 0;
    let mut msg = vec![0u8; 4096];
    let mut len: sdint2 = 0;
    let rt = unsafe {
        dpi.dpi_get_diag_rec(handle_type as _, handle, 1, &mut code, msg.as_mut_ptr() as _, msg.len() as _, &mut len)
    };
    if !succeeded(rt) {
        return Error::internal("调用 DPI 失败，且无法读取错误信息");
    }
    msg.truncate((len.max(0) as usize).min(msg.len()));
    let message = String::from_utf8_lossy(&msg).trim_end_matches('\0').trim().to_string();
    Error::Sql { code: code.to_string(), message, position: None }
}

pub(crate) struct Env {
    pub(crate) dpi: Arc<DmDpi>,
    h: dhenv,
}

impl Env {
    pub(crate) fn new(dpi: Arc<DmDpi>) -> Result<Self> {
        let mut h: dhenv = std::ptr::null_mut();
        let rt = unsafe { dpi.dpi_alloc_env(&mut h) };
        if !succeeded(rt) {
            return Err(Error::internal("dpi_alloc_env 失败"));
        }
        Ok(Env { dpi, h })
    }
}

impl Drop for Env {
    fn drop(&mut self) {
        unsafe { self.dpi.dpi_free_env(self.h) };
    }
}

// 句柄只在单个工作线程上使用（取消除外，见 conn.rs），可以跨线程移交
unsafe impl Send for Env {}

pub(crate) struct Con {
    env: Arc<Env>,
    pub(crate) h: dhcon,
    logged_in: bool,
}

unsafe impl Send for Con {}

impl Con {
    pub(crate) fn new(env: Arc<Env>) -> Result<Self> {
        let mut h: dhcon = std::ptr::null_mut();
        let rt = unsafe { env.dpi.dpi_alloc_con(env.h, &mut h) };
        if !succeeded(rt) {
            return Err(diag(&env.dpi, DSQL_HANDLE_ENV as i64, env.h as dhandle));
        }
        Ok(Con { env, h, logged_in: false })
    }

    pub(crate) fn dpi(&self) -> &DmDpi {
        &self.env.dpi
    }

    pub(crate) fn dpi_arc(&self) -> Arc<DmDpi> {
        self.env.dpi.clone()
    }

    pub(crate) fn check(&self, rt: DPIRETURN) -> Result<()> {
        if succeeded(rt) { Ok(()) } else { Err(diag(self.dpi(), DSQL_HANDLE_DBC as i64, self.h as dhandle)) }
    }

    /// 设置整数型连接属性（DPI 约定：整数值直接放在指针参数里，长度传 0）。
    pub(crate) fn set_attr_int(&self, attr: i64, value: i64) -> Result<()> {
        let rt = unsafe { self.dpi().dpi_set_con_attr(self.h, attr as _, value as usize as dpointer, 0) };
        self.check(rt)
    }

    pub(crate) fn login(&mut self, host: &str, user: &str, password: &str) -> Result<()> {
        let (host, user, password) = (cstring(host)?, cstring(user)?, cstring(password)?);
        let rt = unsafe {
            self.dpi().dpi_login(self.h, host.as_ptr() as _, user.as_ptr() as _, password.as_ptr() as _)
        };
        self.check(rt)?;
        self.logged_in = true;
        Ok(())
    }

    pub(crate) fn set_autocommit(&self, on: bool) -> Result<()> {
        self.set_attr_int(DSQL_ATTR_AUTOCOMMIT as i64, i64::from(on))
    }

    pub(crate) fn commit(&self) -> Result<()> {
        let rt = unsafe { self.dpi().dpi_commit(self.h) };
        self.check(rt)
    }

    pub(crate) fn rollback(&self) -> Result<()> {
        let rt = unsafe { self.dpi().dpi_rollback(self.h) };
        self.check(rt)
    }
}

impl Drop for Con {
    fn drop(&mut self) {
        unsafe {
            if self.logged_in {
                self.dpi().dpi_logout(self.h);
            }
            self.dpi().dpi_free_con(self.h);
        }
    }
}

/// 结果集列的描述。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ColDesc {
    pub(crate) name: String,
    pub(crate) sql_type: i32,
    pub(crate) precision: u64,
    pub(crate) scale: i32,
    pub(crate) nullable: bool,
}

pub(crate) struct Stmt<'c> {
    con: &'c Con,
    pub(crate) h: dhstmt,
}

impl<'c> Stmt<'c> {
    pub(crate) fn new(con: &'c Con) -> Result<Self> {
        let mut h: dhstmt = std::ptr::null_mut();
        let rt = unsafe { con.dpi().dpi_alloc_stmt(con.h, &mut h) };
        con.check(rt)?;
        Ok(Stmt { con, h })
    }

    pub(crate) fn check(&self, rt: DPIRETURN) -> Result<()> {
        if succeeded(rt) { Ok(()) } else { Err(diag(self.con.dpi(), DSQL_HANDLE_STMT as i64, self.h as dhandle)) }
    }

    pub(crate) fn exec_direct(&self, sql: &str) -> Result<()> {
        let sql = cstring(sql)?;
        let rt = unsafe { self.con.dpi().dpi_exec_direct(self.h, sql.as_ptr() as _) };
        self.check(rt)
    }

    pub(crate) fn prepare(&self, sql: &str) -> Result<()> {
        let sql = cstring(sql)?;
        let rt = unsafe { self.con.dpi().dpi_prepare(self.h, sql.as_ptr() as _) };
        self.check(rt)
    }

    pub(crate) fn exec(&self) -> Result<()> {
        let rt = unsafe { self.con.dpi().dpi_exec(self.h) };
        self.check(rt)
    }

    /// 绑定一个输入参数。`buf` 和 `len` 必须活到 `exec` 返回之后。
    pub(crate) fn bind_param(
        &self,
        index: usize,
        c_type: i64,
        sql_type: i64,
        buf: &mut [u8],
        len: &mut slength,
    ) -> Result<()> {
        let precision = buf.len().max(1);
        let rt = unsafe {
            self.con.dpi().dpi_bind_param(
                self.h,
                index as _,
                DSQL_PARAM_INPUT as _,
                c_type as _,
                sql_type as _,
                precision as _,
                0,
                buf.as_mut_ptr() as dpointer,
                buf.len() as _,
                len,
            )
        };
        self.check(rt)
    }

    pub(crate) fn num_columns(&self) -> Result<usize> {
        let mut n: udint2 = 0;
        let rt = unsafe { self.con.dpi().dpi_number_columns(self.h, &mut n) };
        self.check(rt)?;
        Ok(n as usize)
    }

    /// 描述第 `index` 列（从 1 开始）。
    pub(crate) fn describe(&self, index: usize) -> Result<ColDesc> {
        let mut name = vec![0u8; 256];
        let mut name_len: sdint2 = 0;
        let mut sql_type: sdint2 = 0;
        let mut precision: ulength = 0;
        let mut scale: sdint2 = 0;
        let mut nullable: sdint2 = 0;
        let rt = unsafe {
            self.con.dpi().dpi_desc_column(
                self.h,
                index as _,
                name.as_mut_ptr() as _,
                name.len() as _,
                &mut name_len,
                &mut sql_type,
                &mut precision,
                &mut scale,
                &mut nullable,
            )
        };
        self.check(rt)?;
        name.truncate((name_len.max(0) as usize).min(name.len()));
        Ok(ColDesc {
            name: String::from_utf8_lossy(&name).trim_end_matches('\0').to_string(),
            sql_type: sql_type as i32,
            precision: precision as u64,
            scale: scale as i32,
            nullable: nullable != 0,
        })
    }

    /// 取下一行；没有更多行时返回 false。
    pub(crate) fn fetch(&self) -> Result<bool> {
        let mut rows: ulength = 0;
        let rt = unsafe { self.con.dpi().dpi_fetch(self.h, &mut rows) };
        if is(rt, DSQL_NO_DATA as i64) {
            return Ok(false);
        }
        self.check(rt)?;
        Ok(true)
    }

    /// 读取当前行第 `index` 列（从 1 开始）。返回 None 表示 NULL；
    /// 否则返回（读到的字节，数据库报告的总字节数）。读满 `limit` 字节后不再继续读。
    pub(crate) fn get_data(&self, index: usize, binary: bool, limit: usize) -> Result<Option<(Vec<u8>, Option<u64>)>> {
        let c_type = if binary { DSQL_C_BINARY as i64 } else { DSQL_C_NCHAR as i64 };
        let mut out = Vec::new();
        let mut total: Option<u64> = None;
        let mut buf = vec![0u8; CHUNK];
        loop {
            let mut len: slength = 0;
            let rt = unsafe {
                self.con.dpi().dpi_get_data(
                    self.h,
                    index as _,
                    c_type as _,
                    buf.as_mut_ptr() as dpointer,
                    buf.len() as _,
                    &mut len,
                )
            };
            if is(rt, DSQL_NO_DATA as i64) {
                break;
            }
            self.check(rt)?;
            if len as i64 == DSQL_NULL_DATA as i64 {
                return Ok(None);
            }
            // 文本以 \0 结尾，缓冲区写满时有效数据要扣掉结尾符
            let room = if binary { buf.len() } else { buf.len() - 1 };
            if total.is_none() && len >= 0 {
                total = Some(len as u64);
            }
            let got = if len < 0 || len as usize > room { room } else { len as usize };
            out.extend_from_slice(&buf[..got]);
            if is(rt, DSQL_SUCCESS as i64) || out.len() > limit {
                break;
            }
        }
        Ok(Some((out, total)))
    }

    pub(crate) fn row_count(&self) -> Result<u64> {
        let mut n: sdint8 = 0;
        let rt = unsafe { self.con.dpi().dpi_row_count(self.h, &mut n) };
        self.check(rt)?;
        Ok(n.max(0) as u64)
    }
}

impl Drop for Stmt<'_> {
    fn drop(&mut self) {
        unsafe { self.con.dpi().dpi_free_stmt(self.h) };
    }
}
```

- [ ] **Step 5: 编写技术验证测试**

`crates/dblens-driver-dameng/src/spike.rs`：
```rust
//! 达梦 DPI 技术验证。手动运行：
//! `cargo test -p dblens-driver-dameng spike -- --ignored --nocapture --test-threads=1`
//! 需要环境变量 DBLENS_TEST_DM_URL。结论写入 docs/notes/dameng-dpi-spike.md。

use std::sync::Arc;
use std::time::{Duration, Instant};

use dblens_driver::ConnectParams;

use crate::ffi::*;
use crate::handles::{Con, Env, Stmt, succeeded};
use crate::load;

fn connect() -> Con {
    let url = std::env::var("DBLENS_TEST_DM_URL").expect("请设置 DBLENS_TEST_DM_URL");
    let (_, p): (_, ConnectParams) = ConnectParams::from_url(&url).unwrap();
    let dpi = load::load(&p).expect("加载 dmdpi 失败");
    let env = Arc::new(Env::new(dpi).unwrap());
    let mut con = Con::new(env).unwrap();
    con.set_attr_int(DSQL_ATTR_LOCAL_CODE as i64, PG_UTF8 as i64).expect("设置 UTF-8 失败");
    con.set_attr_int(DSQL_ATTR_LOGIN_PORT as i64, i64::from(p.port)).unwrap();
    con.login(&p.host, &p.user, &p.password).expect("登录失败");
    con
}

fn first_text(stmt: &Stmt<'_>) -> String {
    assert!(stmt.fetch().unwrap(), "没有返回行");
    let (bytes, _) = stmt.get_data(1, false, usize::MAX).unwrap().expect("值为 NULL");
    String::from_utf8(bytes).unwrap()
}

#[test]
#[ignore]
fn spike_1_connect_and_unicode() {
    let con = connect();
    let stmt = Stmt::new(&con).unwrap();
    stmt.exec_direct("SELECT '数洞', 1, SYSDATE FROM DUAL").unwrap();
    let n = stmt.num_columns().unwrap();
    for i in 1..=n {
        println!("列 {i}：{:?}", stmt.describe(i).unwrap());
    }
    assert_eq!(first_text(&stmt), "数洞");
    let v = Stmt::new(&con).unwrap();
    v.exec_direct("SELECT TOP 1 BANNER FROM V$VERSION").unwrap();
    println!("服务器版本：{}", first_text(&v));
}

#[test]
#[ignore]
fn spike_2_cancel_from_another_thread() {
    let con = connect();
    let stmt = Stmt::new(&con).unwrap();
    let (dpi, h) = (con.dpi_arc(), stmt.h as usize);
    let canceller = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(500));
        let rt = unsafe { dpi.dpi_cancel(h as dhstmt) };
        println!("dpi_cancel 返回 {}（成功：{}）", rt as i64, succeeded(rt));
    });
    let started = Instant::now();
    let result = stmt.exec_direct("SELECT COUNT(*) FROM SYSOBJECTS a, SYSOBJECTS b, SYSOBJECTS c");
    canceller.join().unwrap();
    println!("执行结果：{result:?}，耗时 {:?}", started.elapsed());
    assert!(result.is_err(), "查询应被取消");
    assert!(started.elapsed() < Duration::from_secs(5), "取消耗时过长");
    // 取消后连接应仍可用
    let again = Stmt::new(&con).unwrap();
    again.exec_direct("SELECT 1").unwrap();
    assert_eq!(first_text(&again), "1");
}

#[test]
#[ignore]
fn spike_3_large_clob_preview() {
    let con = connect();
    let stmt = Stmt::new(&con).unwrap();
    // 如果这条 SQL 在当前 DM 版本上不可用，换成任意返回 10 万个字符的 CLOB 的写法，并记入笔记
    stmt.exec_direct("SELECT RPAD(TO_CLOB('a'), 100000, 'a') FROM DUAL").unwrap();
    println!("列类型：{:?}", stmt.describe(1).unwrap());
    assert!(stmt.fetch().unwrap());
    let (bytes, total) = stmt.get_data(1, false, 64 * 1024).unwrap().unwrap();
    println!("预览读取 {} 字节，数据库报告总长 {:?}", bytes.len(), total);
    assert!(bytes.len() >= 64 * 1024);
}

#[test]
#[ignore]
fn spike_4_error_code_format() {
    let con = connect();
    let stmt = Stmt::new(&con).unwrap();
    let err = stmt.exec_direct("SELECT * FROM dblens_no_such_table").unwrap_err();
    println!("不存在的表：{err:?}");
    let bad = Stmt::new(&con).unwrap();
    println!("语法错误：{:?}", bad.exec_direct("SELEC 1").unwrap_err());
}
```

- [ ] **Step 6: 运行技术验证**

```powershell
$env:DBLENS_TEST_DM_URL = "dm://用户:密码@主机:5236"
cargo test -p dblens-driver-dameng spike -- --ignored --nocapture --test-threads=1
```
Expected：
- `spike_1`：能连接，中文能正确往返，打印出各列描述和服务器版本；
- `spike_2`：约 0.5 秒后查询返回错误，连接仍可用；
- `spike_3`：读满 64KB 预览后停止，打印出数据库报告的总长；
- `spike_4`：打印出两类错误的错误码和信息。

如果某项失败，先排查是不是 `handles.rs` 中参数类型或常量用错了，再下结论。

- [ ] **Step 7: 记录结论**

`docs/notes/dameng-dpi-spike.md`（把 `<…>` 的位置换成实际观察到的内容，**不允许保留尖括号**）：
```markdown
# 达梦 DPI 技术验证结论（日期：<YYYY-MM-DD>）

- 达梦客户端版本（生成 ffi.rs 所用的头文件）：<DM_HOME 中的版本信息>
- 测试库：<V$VERSION 的 BANNER>
- 客户端平台：Windows x64

| 验证项 | 结果 | 观察 |
|---|---|---|
| 连接与中文（PG_UTF8） | <通过/失败> | <列描述输出、中文往返情况> |
| 从其他线程 dpi_cancel | <通过/失败> | <dpi_cancel 返回值、被取消语句的错误码、耗时、取消后连接是否可用> |
| CLOB 分块读取 | <通过/失败> | <首次 get_data 报告的总长是字节还是字符，或是 NO_TOTAL> |
| 错误码格式 | <通过/失败> | <不存在的表、语法错误各自的错误码> |

## 结论

<继续使用 DPI，或改走 ODBC 及原因>

## 对 Task 4 的影响

<例如：被取消语句的错误码是多少、CLOB 总长的单位、需要调整的类型或常量>
```

- [ ] **Step 8: 提交**

```bash
git add crates/dblens-driver-dameng docs/notes/dameng-dpi-spike.md
git commit -m "feat(dameng): DPI 绑定、句柄封装与技术验证" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

**决策点**：把笔记中的结论告诉用户。如果结论是"改走 ODBC"，停止执行后续达梦任务，先修订 Task 4。

---

### Task 4: 达梦驱动

**Files:**
- Create: `crates/dblens-driver-dameng/src/types.rs`、`src/conn.rs`、`tests/conformance.rs`
- Modify: `crates/dblens-driver-dameng/src/lib.rs`

**Interfaces:**
- Consumes: Task 1 的 `blocking::{BlockingConnection, BlockingExec, into_async}`；Task 3 的 `load`、`handles`、`ffi`
- Produces: `dblens_driver_dameng::DmDriver`（实现 `Driver`，`id()` 为 `DriverId::Dameng`）

- [ ] **Step 1: 写失败的单元测试（类型转换，不需要数据库）**

`crates/dblens-driver-dameng/src/types.rs` 末尾：
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{NaiveDate, NaiveTime};

    #[test]
    fn parses_text_by_type() {
        assert_eq!(text_value(DmType::Int, "-42".into()), Value::Int(-42));
        assert_eq!(text_value(DmType::Dec, "123.4500".into()), Value::Decimal("123.4500".into()));
        assert_eq!(text_value(DmType::Dec, ".5".into()), Value::Decimal("0.5".into()));
        assert_eq!(text_value(DmType::Dec, "-.5".into()), Value::Decimal("-0.5".into()));
        assert_eq!(text_value(DmType::Float, "1.5".into()), Value::Float(1.5));
        assert_eq!(text_value(DmType::Bool, "1".into()), Value::Bool(true));
        assert_eq!(text_value(DmType::Date, "2024-02-29".into()), Value::Date(NaiveDate::from_ymd_opt(2024, 2, 29).unwrap()));
        assert_eq!(text_value(DmType::Time, "13:45:30".into()), Value::Time(NaiveTime::from_hms_opt(13, 45, 30).unwrap()));
        assert_eq!(
            text_value(DmType::Timestamp, "2024-02-29 13:45:30.123456".into()),
            Value::DateTime(NaiveDate::from_ymd_opt(2024, 2, 29).unwrap().and_hms_micro_opt(13, 45, 30, 123456).unwrap())
        );
        assert_eq!(text_value(DmType::Char, "数洞".into()), Value::String("数洞".into()));
        assert_eq!(text_value(DmType::Interval, "INTERVAL '1' DAY".into()), Value::Interval("INTERVAL '1' DAY".into()));
        // 解析失败时保留原文
        assert_eq!(text_value(DmType::Int, "abc".into()), Value::Other { type_name: "INT".into(), text: "abc".into() });
    }

    #[test]
    fn param_data_conversion() {
        assert_eq!(param_data(&Value::Null).unwrap(), ParamData::Null);
        assert_eq!(param_data(&Value::Int(7)).unwrap(), ParamData::Text(b"7".to_vec()));
        assert_eq!(param_data(&Value::Bool(true)).unwrap(), ParamData::Text(b"1".to_vec()));
        assert_eq!(param_data(&Value::String("数".into())).unwrap(), ParamData::Text("数".as_bytes().to_vec()));
        assert_eq!(param_data(&Value::Bytes(vec![1, 2])).unwrap(), ParamData::Binary(vec![1, 2]));
        assert_eq!(
            param_data(&Value::DateTime(NaiveDate::from_ymd_opt(2024, 2, 29).unwrap().and_hms_opt(1, 2, 3).unwrap())).unwrap(),
            ParamData::Text(b"2024-02-29 01:02:03".to_vec())
        );
        assert!(param_data(&Value::Other { type_name: "x".into(), text: "y".into() }).is_err());
    }

    #[test]
    fn lob_values_keep_total_length() {
        let text = "a".repeat(70_000);
        match bytes_value(DmType::Clob, text.into_bytes(), Some(100_000), 65_536) {
            Value::Lob(lob) => {
                assert!(!lob.binary && lob.truncated);
                assert_eq!((lob.preview.len(), lob.total_len), (65_536, 100_000));
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(bytes_value(DmType::Binary, vec![1, 2, 3], Some(3), 65_536), Value::Bytes(vec![1, 2, 3]));
    }
}
```

- [ ] **Step 2: 运行测试，确认失败**

Run: `cargo test -p dblens-driver-dameng --lib types`
Expected: 编译失败，提示找不到 `text_value`、`DmType`。

- [ ] **Step 3: 实现 `types.rs`**

在 `types.rs` 的测试模块之前写入：
```rust
//! 达梦类型分类、类型名、文本到 Value 的转换，以及参数转换。
//! DPI 的 SQL 类型常量来自 ffi.rs，类型宽度不确定，统一按 i64 比较。

use chrono::{DateTime, NaiveDate, NaiveDateTime, NaiveTime};
use dblens_core::{DEFAULT_LOB_PREVIEW_BYTES, Error, Result, Value};

use crate::ffi::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DmType {
    Int,
    Dec,
    Float,
    Bool,
    Date,
    Time,
    Timestamp,
    TimestampTz,
    Interval,
    Char,
    Clob,
    Binary,
    Blob,
    Other,
}

impl DmType {
    pub(crate) fn is_binary(self) -> bool {
        matches!(self, DmType::Binary | DmType::Blob)
    }
}

macro_rules! any_of {
    ($t:expr; $($c:ident),+ $(,)?) => { false $(|| $t as i64 == $c as i64)+ };
}

pub(crate) fn classify(t: i32) -> DmType {
    if any_of!(t; DSQL_TINYINT, DSQL_SMALLINT, DSQL_INT, DSQL_BIGINT) {
        DmType::Int
    } else if any_of!(t; DSQL_DEC) {
        DmType::Dec
    } else if any_of!(t; DSQL_FLOAT, DSQL_DOUBLE) {
        DmType::Float
    } else if any_of!(t; DSQL_BIT, DSQL_BOOLEAN) {
        DmType::Bool
    } else if any_of!(t; DSQL_DATE) {
        DmType::Date
    } else if any_of!(t; DSQL_TIME, DSQL_TIME_TZ) {
        DmType::Time
    } else if any_of!(t; DSQL_TIMESTAMP) {
        DmType::Timestamp
    } else if any_of!(t; DSQL_TIMESTAMP_TZ) {
        DmType::TimestampTz
    } else if any_of!(t; DSQL_INTERVAL_YEAR, DSQL_INTERVAL_MONTH, DSQL_INTERVAL_DAY, DSQL_INTERVAL_HOUR,
        DSQL_INTERVAL_MINUTE, DSQL_INTERVAL_SECOND, DSQL_INTERVAL_YEAR_TO_MONTH, DSQL_INTERVAL_DAY_TO_HOUR,
        DSQL_INTERVAL_DAY_TO_MINUTE, DSQL_INTERVAL_DAY_TO_SECOND, DSQL_INTERVAL_HOUR_TO_MINUTE,
        DSQL_INTERVAL_HOUR_TO_SECOND, DSQL_INTERVAL_MINUTE_TO_SECOND)
    {
        DmType::Interval
    } else if any_of!(t; DSQL_CHAR, DSQL_VARCHAR, DSQL_ROWID) {
        DmType::Char
    } else if any_of!(t; DSQL_CLOB) {
        DmType::Clob
    } else if any_of!(t; DSQL_BINARY, DSQL_VARBINARY) {
        DmType::Binary
    } else if any_of!(t; DSQL_BLOB) {
        DmType::Blob
    } else {
        DmType::Other
    }
}

macro_rules! name_of {
    ($t:expr; $($c:ident => $n:literal),+ $(,)?) => {{ $( if $t as i64 == $c as i64 { return $n.to_string(); } )+ }};
}

/// 原生类型名，用于结果集列信息。
pub(crate) fn type_name(t: i32) -> String {
    name_of!(t;
        DSQL_TINYINT => "TINYINT", DSQL_SMALLINT => "SMALLINT", DSQL_INT => "INT", DSQL_BIGINT => "BIGINT",
        DSQL_DEC => "DEC", DSQL_FLOAT => "FLOAT", DSQL_DOUBLE => "DOUBLE", DSQL_BIT => "BIT",
        DSQL_BOOLEAN => "BOOLEAN", DSQL_DATE => "DATE", DSQL_TIME => "TIME", DSQL_TIME_TZ => "TIME WITH TIME ZONE",
        DSQL_TIMESTAMP => "TIMESTAMP", DSQL_TIMESTAMP_TZ => "TIMESTAMP WITH TIME ZONE", DSQL_CHAR => "CHAR",
        DSQL_VARCHAR => "VARCHAR", DSQL_CLOB => "CLOB", DSQL_BINARY => "BINARY", DSQL_VARBINARY => "VARBINARY",
        DSQL_BLOB => "BLOB", DSQL_ROWID => "ROWID",
    );
    if classify(t) == DmType::Interval { "INTERVAL".to_string() } else { format!("TYPE({t})") }
}

fn kind_name(kind: DmType) -> &'static str {
    match kind {
        DmType::Int => "INT",
        DmType::Dec => "DEC",
        DmType::Float => "DOUBLE",
        DmType::Bool => "BIT",
        DmType::Date => "DATE",
        DmType::Time => "TIME",
        DmType::Timestamp => "TIMESTAMP",
        DmType::TimestampTz => "TIMESTAMP WITH TIME ZONE",
        DmType::Interval => "INTERVAL",
        DmType::Char => "VARCHAR",
        DmType::Clob => "CLOB",
        DmType::Binary => "VARBINARY",
        DmType::Blob => "BLOB",
        DmType::Other => "UNKNOWN",
    }
}

fn normalize_number(s: &str) -> String {
    if let Some(rest) = s.strip_prefix("-.") {
        format!("-0.{rest}")
    } else if let Some(rest) = s.strip_prefix('.') {
        format!("0.{rest}")
    } else {
        s.to_string()
    }
}

/// 把 DPI 以文本形式（DSQL_C_NCHAR）返回的值按类型解析为 Value。解析失败时保留原文。
pub(crate) fn text_value(kind: DmType, text: String) -> Value {
    let other = |text: String| Value::Other { type_name: kind_name(kind).to_string(), text };
    match kind {
        DmType::Int => text.parse().map(Value::Int).unwrap_or_else(|_| other(text)),
        DmType::Dec => Value::Decimal(normalize_number(&text)),
        DmType::Float => text.parse().map(Value::Float).unwrap_or_else(|_| other(text)),
        DmType::Bool => Value::Bool(text == "1" || text.eq_ignore_ascii_case("true")),
        DmType::Date => NaiveDate::parse_from_str(&text, "%Y-%m-%d").map(Value::Date).unwrap_or_else(|_| other(text)),
        DmType::Time => NaiveTime::parse_from_str(&text, "%H:%M:%S%.f").map(Value::Time).unwrap_or_else(|_| other(text)),
        DmType::Timestamp => NaiveDateTime::parse_from_str(&text, "%Y-%m-%d %H:%M:%S%.f")
            .map(Value::DateTime)
            .unwrap_or_else(|_| other(text)),
        DmType::TimestampTz => DateTime::parse_from_str(&text, "%Y-%m-%d %H:%M:%S%.f %:z")
            .map(Value::DateTimeTz)
            .unwrap_or_else(|_| other(text)),
        DmType::Interval => Value::Interval(text),
        DmType::Char | DmType::Clob => Value::String(text),
        DmType::Binary | DmType::Blob | DmType::Other => other(text),
    }
}

/// 把读到的字节转换为 Value。`total` 是数据库报告的总长度，用于大字段预览。
pub(crate) fn bytes_value(kind: DmType, bytes: Vec<u8>, total: Option<u64>, limit: usize) -> Value {
    let value = if kind.is_binary() {
        Value::Bytes(bytes)
    } else {
        text_value(kind, String::from_utf8_lossy(&bytes).into_owned())
    };
    match value.cap_large(limit) {
        Value::Lob(mut lob) => {
            if let Some(total) = total {
                lob.total_len = lob.total_len.max(total);
            }
            Value::Lob(lob)
        }
        other => other,
    }
}

/// 默认的大字段预览长度，供 conn.rs 使用。
pub(crate) const PREVIEW: usize = DEFAULT_LOB_PREVIEW_BYTES;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ParamData {
    Null,
    Text(Vec<u8>),
    Binary(Vec<u8>),
}

/// 参数统一以文本或二进制绑定，由数据库做隐式转换。
pub(crate) fn param_data(v: &Value) -> Result<ParamData> {
    let text = |s: String| Ok(ParamData::Text(s.into_bytes()));
    match v {
        Value::Null => Ok(ParamData::Null),
        Value::Bool(b) => text(if *b { "1".into() } else { "0".into() }),
        Value::Int(i) => text(i.to_string()),
        Value::UInt(u) => text(u.to_string()),
        Value::Float(f) => text(f.to_string()),
        Value::Decimal(s) | Value::String(s) | Value::Interval(s) => text(s.clone()),
        Value::Bytes(b) => Ok(ParamData::Binary(b.clone())),
        Value::Date(d) => text(d.format("%Y-%m-%d").to_string()),
        Value::Time(t) => text(t.format("%H:%M:%S%.f").to_string()),
        Value::DateTime(dt) => text(dt.format("%Y-%m-%d %H:%M:%S%.f").to_string()),
        Value::DateTimeTz(dt) => text(dt.format("%Y-%m-%d %H:%M:%S%.f %:z").to_string()),
        Value::Json(j) => text(j.to_string()),
        Value::Uuid(u) => text(u.to_string()),
        Value::Lob(_) | Value::Other { .. } => Err(Error::internal(format!("该类型的值不能作为参数绑定：{v:?}"))),
    }
}
```

把 `lib.rs` 的模块列表改为（驱动本体在 Step 5 加入）：
```rust
#[allow(clippy::all)]
mod ffi;
mod handles;
mod load;
mod types;

#[cfg(test)]
mod spike;
```

- [ ] **Step 4: 运行单元测试，确认通过**

Run: `cargo test -p dblens-driver-dameng --lib types`
Expected: 3 个测试 PASS。

- [ ] **Step 5: 写失败的一致性测试**

`crates/dblens-driver-dameng/tests/conformance.rs`：
```rust
use chrono::{NaiveDate, NaiveTime};
use dblens_core::Value;
use dblens_driver::ConnectParams;
use dblens_driver::conformance::Fixture;
use dblens_driver_dameng::DmDriver;

fn fixture(params: ConnectParams) -> Fixture {
    Fixture {
        driver: Box::new(DmDriver),
        params,
        select_one: "SELECT 1",
        gen_rows: |n| format!("SELECT LEVEL FROM DUAL CONNECT BY LEVEL <= {n} ORDER BY 1"),
        long_running: "SELECT COUNT(*) FROM SYSOBJECTS a, SYSOBJECTS b, SYSOBJECTS c",
        // 与技术验证 spike_3 使用同一写法；如果当时改过，这里同步修改
        large_text: "SELECT RPAD(TO_CLOB('a'), 100000, 'a') FROM DUAL",
        create_table: |t| format!("CREATE TABLE {t} (id INT PRIMARY KEY, name VARCHAR(100))"),
        drop_table_if_exists: |t| format!("DROP TABLE IF EXISTS {t}"),
        insert_with_params: |t| format!("INSERT INTO {t} (id, name) VALUES (?, ?)"),
        select_name_by_id: |t| format!("SELECT name FROM {t} WHERE id = ?"),
        count_rows: |t| format!("SELECT COUNT(*) FROM {t}"),
        missing_table_query: "SELECT * FROM dblens_no_such_table",
        type_cases: vec![
            ("SELECT NULL", Value::Null),
            ("SELECT CAST(42 AS INT)", Value::Int(42)),
            ("SELECT CAST(42 AS BIGINT)", Value::Int(42)),
            ("SELECT CAST(1.5 AS DOUBLE)", Value::Float(1.5)),
            ("SELECT CAST(123.45 AS DEC(10,4))", Value::Decimal("123.4500".into())),
            ("SELECT '数洞'", Value::String("数洞".into())),
            ("SELECT DATE '2024-02-29'", Value::Date(NaiveDate::from_ymd_opt(2024, 2, 29).unwrap())),
            (
                "SELECT TIMESTAMP '2024-02-29 13:45:30.123456'",
                Value::DateTime(
                    NaiveDate::from_ymd_opt(2024, 2, 29).unwrap().and_hms_micro_opt(13, 45, 30, 123456).unwrap(),
                ),
            ),
            ("SELECT TIME '13:45:30'", Value::Time(NaiveTime::from_hms_opt(13, 45, 30).unwrap())),
            ("SELECT 0x010203", Value::Bytes(vec![1, 2, 3])),
        ],
    }
}

dblens_driver::conformance_tests!("DBLENS_TEST_DM_URL", fixture);
```

Run: `cargo test -p dblens-driver-dameng --test conformance`
Expected: 编译失败，提示找不到 `DmDriver`。

- [ ] **Step 6: 实现 `conn.rs` 和驱动入口**

`crates/dblens-driver-dameng/src/conn.rs`：
```rust
//! 达梦连接（在专用线程上运行的同步实现）与执行流。

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use dblens_core::{Error, Result, Row, Value};
use dblens_driver::blocking::{BlockingConnection, BlockingExec};
use dblens_driver::{CancelHandle, ColumnMeta, ExecEvent, ServerInfo};

use crate::ffi::*;
use crate::handles::{ColDesc, Con, Stmt, diag, succeeded};
use crate::types::{DmType, PREVIEW, ParamData, bytes_value, classify, param_data, type_name};

/// 正在执行的语句句柄，供取消使用。
#[derive(Clone, Copy)]
struct StmtPtr(dhstmt);
unsafe impl Send for StmtPtr {}

type Current = Arc<Mutex<Option<StmtPtr>>>;

pub(crate) struct DmConnection {
    con: Con,
    current: Current,
    cancelled: Arc<AtomicBool>,
}

impl DmConnection {
    pub(crate) fn new(con: Con) -> Self {
        DmConnection { con, current: Arc::new(Mutex::new(None)), cancelled: Arc::new(AtomicBool::new(false)) }
    }

    /// 取消句柄：加锁后对当前语句调用 dpi_cancel。持锁期间工作线程不会释放该语句。
    pub(crate) fn cancel_handle(&self) -> CancelHandle {
        let (dpi, current, cancelled) = (self.con.dpi_arc(), self.current.clone(), self.cancelled.clone());
        CancelHandle::new(move || {
            let (dpi, current, cancelled) = (dpi.clone(), current.clone(), cancelled.clone());
            async move {
                tokio::task::spawn_blocking(move || -> Result<()> {
                    let guard = current.lock().map_err(|_| Error::internal("取消锁已损坏"))?;
                    if let Some(ptr) = *guard {
                        cancelled.store(true, Ordering::SeqCst);
                        let rt = unsafe { dpi.dpi_cancel(ptr.0) };
                        if !succeeded(rt) {
                            return Err(diag(&dpi, DSQL_HANDLE_STMT as i64, ptr.0 as dhandle));
                        }
                    }
                    Ok(())
                })
                .await
                .map_err(|e| Error::internal(format!("取消任务异常退出：{e}")))?
            }
        })
    }

    fn set_current(&self, ptr: Option<dhstmt>) {
        if let Ok(mut guard) = self.current.lock() {
            *guard = ptr.map(StmtPtr);
        }
    }

    /// 执行期间出错时，如果期间发出过取消，就报告为"已取消"。
    fn map_exec_err(&self, e: Error) -> Error {
        if self.cancelled.swap(false, Ordering::SeqCst) { Error::Cancelled } else { e }
    }

    fn query_text(&self, sql: &str) -> Result<String> {
        let stmt = Stmt::new(&self.con)?;
        stmt.exec_direct(sql)?;
        if !stmt.fetch()? {
            return Err(Error::internal(format!("{sql} 没有返回结果")));
        }
        let (bytes, _) = stmt.get_data(1, false, usize::MAX)?.unwrap_or_default();
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }
}

/// 绑定参数用到的缓冲区，必须活到语句执行完毕。
struct ParamBuffers {
    data: Vec<Vec<u8>>,
    lens: Vec<slength>,
}

fn bind_params(stmt: &Stmt<'_>, params: &[Value]) -> Result<ParamBuffers> {
    let mut bufs = ParamBuffers { data: Vec::with_capacity(params.len()), lens: vec![0; params.len()] };
    let mut kinds = Vec::with_capacity(params.len());
    for v in params {
        let (c_type, sql_type, bytes, null) = match param_data(v)? {
            ParamData::Null => (DSQL_C_NCHAR as i64, DSQL_VARCHAR as i64, vec![0u8], true),
            ParamData::Text(b) => (DSQL_C_NCHAR as i64, DSQL_VARCHAR as i64, b, false),
            ParamData::Binary(b) => (DSQL_C_BINARY as i64, DSQL_VARBINARY as i64, b, false),
        };
        kinds.push((c_type, sql_type, null));
        bufs.data.push(bytes);
    }
    // data 与 lens 都已按最终长度分配、不会再扩容；整体移动 ParamBuffers 也不会改变堆上地址，
    // 因此绑定时交给 DPI 的指针在执行期间保持有效
    let ParamBuffers { data, lens } = &mut bufs;
    for (i, (c_type, sql_type, null)) in kinds.into_iter().enumerate() {
        lens[i] = if null { DSQL_NULL_DATA as slength } else { data[i].len() as slength };
        stmt.bind_param(i + 1, c_type, sql_type, &mut data[i], &mut lens[i])?;
    }
    Ok(bufs)
}

enum State {
    Rows,
    Done,
}

pub(crate) struct DmExec<'a> {
    stmt: Stmt<'a>,
    current: &'a Current,
    _params: ParamBuffers,
    columns: Vec<(ColDesc, DmType)>,
    pending: VecDeque<ExecEvent>,
    state: State,
}

impl DmExec<'_> {
    fn read_row(&self) -> Result<Row> {
        let mut row = Vec::with_capacity(self.columns.len());
        for (i, (_, kind)) in self.columns.iter().enumerate() {
            let value = match self.stmt.get_data(i + 1, kind.is_binary(), PREVIEW)? {
                None => Value::Null,
                Some((bytes, total)) => bytes_value(*kind, bytes, total, PREVIEW),
            };
            row.push(value);
        }
        Ok(row)
    }
}

impl BlockingExec for DmExec<'_> {
    fn next(&mut self, max_rows: usize) -> Result<Option<ExecEvent>> {
        if let Some(event) = self.pending.pop_front() {
            return Ok(Some(event));
        }
        match self.state {
            State::Done => Ok(None),
            State::Rows => {
                let mut rows = Vec::new();
                let mut ended = false;
                while rows.len() < max_rows.max(1) {
                    if !self.stmt.fetch()? {
                        ended = true;
                        break;
                    }
                    rows.push(self.read_row()?);
                }
                if ended {
                    self.state = State::Done;
                    if rows.is_empty() {
                        return Ok(Some(ExecEvent::ResultEnd));
                    }
                    self.pending.push_back(ExecEvent::ResultEnd);
                }
                Ok(Some(ExecEvent::Rows(rows)))
            }
        }
    }
}

impl Drop for DmExec<'_> {
    fn drop(&mut self) {
        // 先从"当前语句"中移除，再由字段析构释放语句句柄
        if let Ok(mut guard) = self.current.lock() {
            *guard = None;
        }
    }
}

impl BlockingConnection for DmConnection {
    fn execute<'a>(&'a mut self, sql: &'a str, params: &'a [Value]) -> Result<Box<dyn BlockingExec + 'a>> {
        self.cancelled.store(false, Ordering::SeqCst);
        let stmt = Stmt::new(&self.con)?;
        self.set_current(Some(stmt.h));
        let run = || -> Result<ParamBuffers> {
            if params.is_empty() {
                stmt.exec_direct(sql)?;
                Ok(ParamBuffers { data: Vec::new(), lens: Vec::new() })
            } else {
                stmt.prepare(sql)?;
                let bufs = bind_params(&stmt, params)?;
                stmt.exec()?;
                Ok(bufs)
            }
        };
        let bufs = match run() {
            Ok(bufs) => bufs,
            Err(e) => {
                self.set_current(None);
                return Err(self.map_exec_err(e));
            }
        };
        let describe = || -> Result<(Vec<(ColDesc, DmType)>, VecDeque<ExecEvent>, State)> {
            let n = stmt.num_columns()?;
            let mut pending = VecDeque::new();
            let mut columns = Vec::with_capacity(n);
            if n == 0 {
                pending.push_back(ExecEvent::Affected(stmt.row_count()?));
                return Ok((columns, pending, State::Done));
            }
            for i in 1..=n {
                let desc = stmt.describe(i)?;
                let kind = classify(desc.sql_type);
                columns.push((desc, kind));
            }
            let metas = columns
                .iter()
                .map(|(d, _)| ColumnMeta {
                    name: d.name.clone(),
                    type_name: type_name(d.sql_type),
                    nullable: Some(d.nullable),
                    precision: (d.precision > 0).then_some(d.precision as u32),
                    scale: Some(d.scale),
                    source_table: None,
                    source_column: None,
                })
                .collect();
            pending.push_back(ExecEvent::ResultStart { columns: metas });
            Ok((columns, pending, State::Rows))
        };
        let (columns, pending, state) = match describe() {
            Ok(v) => v,
            Err(e) => {
                // 语句句柄即将随 stmt 释放，必须先从"当前语句"中移除，否则取消会作用到已释放的句柄
                self.set_current(None);
                return Err(e);
            }
        };
        Ok(Box::new(DmExec { stmt, current: &self.current, _params: bufs, columns, pending, state }))
    }

    fn set_autocommit(&mut self, on: bool) -> Result<()> {
        self.con.set_autocommit(on)
    }

    fn commit(&mut self) -> Result<()> {
        self.con.commit()
    }

    fn rollback(&mut self) -> Result<()> {
        self.con.rollback()
    }

    fn ping(&mut self) -> Result<()> {
        self.query_text("SELECT 1").map(|_| ())
    }

    fn server_info(&mut self) -> Result<ServerInfo> {
        Ok(ServerInfo { product: "DM".into(), version: self.query_text("SELECT TOP 1 BANNER FROM V$VERSION")? })
    }

    fn close(self: Box<Self>) -> Result<()> {
        // 句柄在析构时登出并释放
        Ok(())
    }
}
```

把 `crates/dblens-driver-dameng/src/lib.rs` 改为：
```rust
//! 达梦驱动：通过 DPI（运行时加载 dmdpi）访问 DM8。

mod conn;
#[allow(clippy::all)]
mod ffi;
mod handles;
mod load;
mod types;

#[cfg(test)]
mod spike;

use std::sync::Arc;

use async_trait::async_trait;
use dblens_core::{Error, Result};
use dblens_driver::blocking::into_async;
use dblens_driver::{Capabilities, ConnectParams, Connection, Driver, DriverId};

use crate::conn::DmConnection;
use crate::ffi::*;
use crate::handles::{Con, Env};

pub struct DmDriver;

fn connect_blocking(p: &ConnectParams) -> Result<DmConnection> {
    let dpi = load::load(p)?;
    let env = Arc::new(Env::new(dpi)?);
    let mut con = Con::new(env)?;
    con.set_attr_int(DSQL_ATTR_LOCAL_CODE as i64, PG_UTF8 as i64)?;
    con.set_attr_int(DSQL_ATTR_LOGIN_PORT as i64, i64::from(p.port))?;
    con.set_attr_int(DSQL_ATTR_LOGIN_TIMEOUT as i64, p.connect_timeout.as_secs() as i64)?;
    // 登录失败统一报告为连接错误（认证失败的错误码待技术验证后细分）
    con.login(&p.host, &p.user, &p.password).map_err(|e| match e {
        Error::Sql { code, message, .. } => Error::connect(format!("[{code}] {message}")),
        other => other,
    })?;
    con.set_autocommit(true)?;
    Ok(DmConnection::new(con))
}

#[async_trait]
impl Driver for DmDriver {
    fn id(&self) -> DriverId {
        DriverId::Dameng
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            catalogs: false,
            schemas: true,
            transactional_ddl: false,
            cancel: true,
            explain: true,
            multiple_result_sets: false,
        }
    }

    async fn connect(&self, p: &ConnectParams) -> Result<Box<dyn Connection>> {
        let p = p.clone();
        let conn = tokio::task::spawn_blocking(move || connect_blocking(&p))
            .await
            .map_err(|e| Error::internal(format!("连接任务异常退出：{e}")))??;
        let cancel = conn.cancel_handle();
        into_async(conn, cancel, "dblens-dameng")
    }
}
```

- [ ] **Step 7: 运行一致性测试**

```powershell
$env:DBLENS_TEST_DM_URL = "dm://用户:密码@主机:5236"
cargo test -p dblens-driver-dameng --test conformance -- --nocapture --test-threads=1
```
Expected: 9 个测试全部 PASS，输出中没有"跳过"。

排查规则：
- 先对照技术验证笔记中的"对 Task 4 的影响"。
- 只修改驱动代码；夹具中的测试 SQL 如果不是合法的达梦语法，可以换成等价写法，但**不修改期望值**。
- 修改夹具 SQL 时，在旁边写注释说明原因。

然后按并行方式连跑 3 次，确认稳定：
```powershell
1..3 | ForEach-Object { cargo test -q -p dblens-driver-dameng --test conformance -- --nocapture 2>&1 | Select-String "test result|跳过|FAILED" }
```

- [ ] **Step 8: clippy 与提交**

Run: `cargo clippy -p dblens-driver-dameng --all-targets -- -D warnings`
Expected: 没有警告（`ffi.rs` 已整体放行 clippy）。

```bash
git add crates/dblens-driver-dameng
git commit -m "feat(dameng): 达梦驱动（DPI、流式取数、取消、事务、LOB 预览）" -m "一致性测试 9 项已在真实 DM8 上通过。" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 5: Oracle 驱动

**Files:**
- Create: `crates/dblens-driver-oracle/Cargo.toml`
- Create: `crates/dblens-driver-oracle/src/{lib.rs,conn.rs,value.rs,error.rs}`
- Test: `crates/dblens-driver-oracle/tests/conformance.rs`

**Interfaces:**
- Consumes: Task 1 的 `blocking`；Task 2 的 `native::locate`、`NativeLib::OracleClient`
- Produces: `dblens_driver_oracle::OracleDriver`（实现 `Driver`，`id()` 为 `DriverId::Oracle`）

- [ ] **Step 1: 创建 crate 并写失败的单元测试**

`crates/dblens-driver-oracle/Cargo.toml`：
```toml
[package]
name = "dblens-driver-oracle"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
publish.workspace = true

[dependencies]
async-trait.workspace = true
chrono.workspace = true
dblens-core.workspace = true
dblens-driver.workspace = true
oracle.workspace = true
tokio.workspace = true

[dev-dependencies]
dblens-driver = { workspace = true, features = ["conformance"] }
tokio.workspace = true
```

`crates/dblens-driver-oracle/src/lib.rs`（本步骤的临时版本）：
```rust
//! Oracle 驱动：基于 `oracle` crate（ODPI-C），运行时加载 Oracle Instant Client。

mod error;
mod value;
```

`crates/dblens-driver-oracle/src/value.rs` 末尾：
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_leading_dot_numbers() {
        assert_eq!(normalize_number(".5"), "0.5");
        assert_eq!(normalize_number("-.25"), "-0.25");
        assert_eq!(normalize_number("42"), "42");
        assert_eq!(normalize_number("-3.1"), "-3.1");
    }

    #[test]
    fn rejects_unbindable_values() {
        assert!(to_binds(&[Value::Int(1), Value::String("x".into()), Value::Null]).is_ok());
        assert!(to_binds(&[Value::Other { type_name: "x".into(), text: "y".into() }]).is_err());
    }
}
```

- [ ] **Step 2: 运行测试，确认失败**

Run: `cargo test -p dblens-driver-oracle --lib`
Expected: 编译失败，提示找不到 `normalize_number`、`to_binds`。注意：首次编译 `oracle` crate 会用 MSVC 编译 ODPI-C 的 C 源码，耗时较长，这是正常的。

- [ ] **Step 3: 实现 `error.rs` 和 `value.rs`**

`crates/dblens-driver-oracle/src/error.rs`：
```rust
use dblens_core::Error;
use oracle::ErrorKind;

pub(crate) fn map_err(e: oracle::Error) -> Error {
    let Some(db) = e.db_error() else {
        return Error::internal(e.to_string());
    };
    let message = db.message().to_string();
    if matches!(e.kind(), ErrorKind::DpiError) {
        // DPI-1047：找不到或无法加载 Oracle 客户端库
        if message.contains("DPI-1047") {
            return Error::DriverUnavailable { hint: message };
        }
        return Error::internal(message);
    }
    match db.code() {
        // ORA-01013：用户请求取消当前操作
        1013 => Error::Cancelled,
        1017 | 28000 | 28001 => Error::Auth { message },
        12154 | 12170 | 12505 | 12514 | 12541 | 12543 => Error::connect(message),
        code => Error::Sql {
            code: format!("ORA-{code:05}"),
            message,
            position: (db.offset() > 0).then(|| db.offset() + 1),
        },
    }
}
```

`crates/dblens-driver-oracle/src/value.rs`（测试模块之前）：
```rust
//! Oracle 列信息、值映射与参数绑定。

use std::io::Read;

use chrono::{DateTime, FixedOffset, NaiveDateTime};
use dblens_core::{DEFAULT_LOB_PREVIEW_BYTES, Error, Result, Value};
use dblens_driver::ColumnMeta;
use oracle::sql_type::{Blob, Clob, Lob, OracleType, ToSql};
use oracle::{ColumnInfo, SqlValue};

use crate::error::map_err;

/// ODPI-C 返回的数字文本可能省略整数部分的 0（如 ".5"），补齐以便阅读和比较。
pub(crate) fn normalize_number(s: &str) -> String {
    if let Some(rest) = s.strip_prefix("-.") {
        format!("-0.{rest}")
    } else if let Some(rest) = s.strip_prefix('.') {
        format!("0.{rest}")
    } else {
        s.to_string()
    }
}

pub(crate) fn column_meta(info: &ColumnInfo) -> ColumnMeta {
    let (precision, scale) = match info.oracle_type() {
        OracleType::Number(p, s) => ((*p > 0).then_some(u32::from(*p)), Some(i32::from(*s))),
        _ => (None, None),
    };
    ColumnMeta {
        name: info.name().to_string(),
        type_name: info.oracle_type().to_string(),
        nullable: Some(info.nullable()),
        precision,
        scale,
        source_table: None,
        source_column: None,
    }
}

/// `complete` 为 false 时，`bytes` 比预览长度多读了几个字节，cap_large 一定会把它转成预览。
fn lob_preview(binary: bool, bytes: Vec<u8>, total: u64, complete: bool) -> Value {
    let value = if binary { Value::Bytes(bytes) } else { Value::String(String::from_utf8_lossy(&bytes).into_owned()) };
    if complete {
        return value;
    }
    match value.cap_large(DEFAULT_LOB_PREVIEW_BYTES) {
        Value::Lob(mut lob) => {
            lob.total_len = total;
            Value::Lob(lob)
        }
        other => other,
    }
}

/// 读取 LOB 的前 N 字节作为预览。CLOB 的 size() 单位是字符，BLOB 是字节。
fn read_lob(mut lob: impl Read + Lob, binary: bool) -> Result<Value> {
    let total = lob.size().map_err(map_err)?;
    let mut bytes = Vec::new();
    let want = DEFAULT_LOB_PREVIEW_BYTES as u64 + 4;
    (&mut lob).take(want).read_to_end(&mut bytes).map_err(|e| Error::internal(format!("读取 LOB 失败：{e}")))?;
    let complete = (bytes.len() as u64) < want;
    Ok(lob_preview(binary, bytes, total, complete))
}

pub(crate) fn oracle_value(v: &SqlValue) -> Result<Value> {
    if v.is_null().map_err(map_err)? {
        return Ok(Value::Null);
    }
    let ty = v.oracle_type().map_err(map_err)?.clone();
    let value = match ty {
        OracleType::Varchar2(_)
        | OracleType::NVarchar2(_)
        | OracleType::Char(_)
        | OracleType::NChar(_)
        | OracleType::Long
        | OracleType::Rowid => Value::String(v.get::<String>().map_err(map_err)?),
        OracleType::Number(_, _) | OracleType::Float(_) => {
            Value::Decimal(normalize_number(&v.get::<String>().map_err(map_err)?))
        }
        OracleType::Int64 => Value::Int(v.get().map_err(map_err)?),
        OracleType::UInt64 => Value::UInt(v.get().map_err(map_err)?),
        OracleType::BinaryFloat | OracleType::BinaryDouble => Value::Float(v.get().map_err(map_err)?),
        OracleType::Date | OracleType::Timestamp(_) => Value::DateTime(v.get::<NaiveDateTime>().map_err(map_err)?),
        OracleType::TimestampTZ(_) | OracleType::TimestampLTZ(_) => {
            Value::DateTimeTz(v.get::<DateTime<FixedOffset>>().map_err(map_err)?)
        }
        OracleType::IntervalDS(_, _) | OracleType::IntervalYM(_) => Value::Interval(v.to_string()),
        OracleType::Raw(_) | OracleType::LongRaw => Value::Bytes(v.get::<Vec<u8>>().map_err(map_err)?),
        OracleType::CLOB | OracleType::NCLOB => read_lob(v.get::<Clob>().map_err(map_err)?, false)?,
        OracleType::BLOB => read_lob(v.get::<Blob>().map_err(map_err)?, true)?,
        OracleType::Boolean => Value::Bool(v.get().map_err(map_err)?),
        other => Value::Other { type_name: other.to_string(), text: v.to_string() },
    };
    Ok(value)
}

/// 把参数转换为可绑定的值。
pub(crate) fn to_binds(params: &[Value]) -> Result<Vec<Box<dyn ToSql>>> {
    params
        .iter()
        .map(|v| -> Result<Box<dyn ToSql>> {
            Ok(match v {
                Value::Null => Box::new(None::<String>),
                Value::Bool(b) => Box::new(*b),
                Value::Int(i) => Box::new(*i),
                Value::UInt(u) => Box::new(*u),
                Value::Float(f) => Box::new(*f),
                Value::Decimal(s) | Value::String(s) | Value::Interval(s) => Box::new(s.clone()),
                Value::Bytes(b) => Box::new(b.clone()),
                Value::Date(d) => Box::new(*d),
                Value::Time(t) => Box::new(t.to_string()),
                Value::DateTime(dt) => Box::new(*dt),
                Value::DateTimeTz(dt) => Box::new(*dt),
                Value::Json(j) => Box::new(j.to_string()),
                Value::Uuid(u) => Box::new(u.to_string()),
                Value::Lob(_) | Value::Other { .. } => {
                    return Err(Error::internal(format!("该类型的值不能作为参数绑定：{v:?}")));
                }
            })
        })
        .collect()
}
```

> 注：`ToSql`、`Lob` 等的导入路径如果与 0.6.3 实际导出的不同（例如 `oracle::sql_type::ToSql` 与 `oracle::ToSql`），以编译器提示为准调整 `use` 语句。

- [ ] **Step 4: 运行单元测试，确认通过**

Run: `cargo test -p dblens-driver-oracle --lib`
Expected: 2 个测试 PASS。

- [ ] **Step 5: 写失败的一致性测试**

`crates/dblens-driver-oracle/tests/conformance.rs`：
```rust
use chrono::{FixedOffset, NaiveDate, TimeZone};
use dblens_core::Value;
use dblens_driver::conformance::Fixture;
use dblens_driver::{ConnectParams, Driver, ExecEvent, collect_all};
use dblens_driver_oracle::OracleDriver;

fn fixture(params: ConnectParams) -> Fixture {
    Fixture {
        driver: Box::new(OracleDriver),
        params,
        select_one: "SELECT 1 FROM DUAL",
        gen_rows: |n| format!("SELECT LEVEL FROM DUAL CONNECT BY LEVEL <= {n} ORDER BY 1"),
        long_running: "SELECT COUNT(*) FROM all_objects a, all_objects b, all_objects c",
        large_text: "SELECT RPAD(TO_CLOB('a'), 100000, 'a') FROM DUAL",
        create_table: |t| format!("CREATE TABLE {t} (id NUMBER(10) PRIMARY KEY, name VARCHAR2(100))"),
        // 用 PL/SQL 块兼容 23ai 之前的版本，同时覆盖 DBMS_OUTPUT 的收取路径
        drop_table_if_exists: |t| {
            format!(
                "BEGIN EXECUTE IMMEDIATE 'DROP TABLE {t}'; EXCEPTION WHEN OTHERS THEN IF SQLCODE != -942 THEN RAISE; END IF; END;"
            )
        },
        insert_with_params: |t| format!("INSERT INTO {t} (id, name) VALUES (:1, :2)"),
        select_name_by_id: |t| format!("SELECT name FROM {t} WHERE id = :1"),
        count_rows: |t| format!("SELECT COUNT(*) FROM {t}"),
        missing_table_query: "SELECT * FROM dblens_no_such_table",
        type_cases: vec![
            ("SELECT NULL FROM DUAL", Value::Null),
            ("SELECT 42 FROM DUAL", Value::Decimal("42".into())),
            ("SELECT 123.45 FROM DUAL", Value::Decimal("123.45".into())),
            ("SELECT 0.5 FROM DUAL", Value::Decimal("0.5".into())),
            ("SELECT CAST(1.5 AS BINARY_DOUBLE) FROM DUAL", Value::Float(1.5)),
            ("SELECT '数洞' FROM DUAL", Value::String("数洞".into())),
            (
                "SELECT DATE '2024-02-29' FROM DUAL",
                Value::DateTime(NaiveDate::from_ymd_opt(2024, 2, 29).unwrap().and_hms_opt(0, 0, 0).unwrap()),
            ),
            (
                "SELECT TIMESTAMP '2024-02-29 13:45:30.123456' FROM DUAL",
                Value::DateTime(
                    NaiveDate::from_ymd_opt(2024, 2, 29).unwrap().and_hms_micro_opt(13, 45, 30, 123456).unwrap(),
                ),
            ),
            (
                "SELECT TIMESTAMP '2024-02-29 13:45:30 +08:00' FROM DUAL",
                Value::DateTimeTz(FixedOffset::east_opt(8 * 3600).unwrap().with_ymd_and_hms(2024, 2, 29, 13, 45, 30).unwrap()),
            ),
            ("SELECT HEXTORAW('010203') FROM DUAL", Value::Bytes(vec![1, 2, 3])),
            ("SELECT TO_CLOB('abc') FROM DUAL", Value::String("abc".into())),
        ],
    }
}

dblens_driver::conformance_tests!("DBLENS_TEST_ORACLE_URL", fixture);

/// PL/SQL 中 DBMS_OUTPUT 的输出应作为 Message 事件返回。
#[tokio::test]
async fn dbms_output_becomes_message() {
    let Ok(url) = std::env::var("DBLENS_TEST_ORACLE_URL") else {
        eprintln!("跳过：未设置环境变量 DBLENS_TEST_ORACLE_URL");
        return;
    };
    let (_, params) = ConnectParams::from_url(&url).unwrap();
    let mut conn = OracleDriver.connect(&params).await.unwrap();
    let mut stream = conn
        .execute("BEGIN DBMS_OUTPUT.PUT_LINE('你好'); DBMS_OUTPUT.PUT_LINE('数洞'); END;", &[])
        .await
        .unwrap();
    let events = collect_all(stream.as_mut(), 100).await.unwrap();
    assert_eq!(
        events,
        vec![ExecEvent::Message("你好".into()), ExecEvent::Message("数洞".into()), ExecEvent::Affected(1)]
    );
}
```

> 说明：PL/SQL 块执行后 `row_count()` 返回 1，这是 ODPI-C 的约定。如果实测为 0，把期望中的 `Affected(1)` 改为实测值，并在旁边写注释说明。

Run: `cargo test -p dblens-driver-oracle --test conformance`
Expected: 编译失败，提示找不到 `OracleDriver`。

- [ ] **Step 6: 实现 `conn.rs` 和驱动入口**

`crates/dblens-driver-oracle/src/conn.rs`：
```rust
//! Oracle 连接（在专用线程上运行的同步实现）与执行流。

use std::collections::VecDeque;
use std::sync::Arc;

use dblens_core::{Result, Row, Value};
use dblens_driver::blocking::{BlockingConnection, BlockingExec};
use dblens_driver::{ColumnMeta, ExecEvent, ServerInfo};
use oracle::sql_type::{OracleType, ToSql};
use oracle::{ResultSet, Row as OraRow};

use crate::error::map_err;
use crate::value::{column_meta, oracle_value, to_binds};

/// 每次网络往返取回的行数。
const FETCH_ARRAY_SIZE: u32 = 500;

pub(crate) struct OracleConnection {
    conn: Arc<oracle::Connection>,
    /// 自行维护的自动提交标志（`oracle::Connection::set_autocommit` 需要 &mut，而连接要与取消句柄共享）。
    autocommit: bool,
}

impl OracleConnection {
    pub(crate) fn new(conn: Arc<oracle::Connection>) -> Self {
        OracleConnection { conn, autocommit: true }
    }
}

/// 收取 DBMS_OUTPUT 缓冲区中的全部行。
fn drain_output(conn: &oracle::Connection) -> Result<Vec<String>> {
    let mut stmt = conn.statement("BEGIN DBMS_OUTPUT.GET_LINE(:line, :status); END;").build().map_err(map_err)?;
    let mut lines = Vec::new();
    loop {
        // OracleType 作为输出参数的类型声明
        let line_type = OracleType::Varchar2(32767);
        let status_type = OracleType::Number(0, 0);
        stmt.execute_named(&[("line", &line_type as &dyn ToSql), ("status", &status_type as &dyn ToSql)])
            .map_err(map_err)?;
        let status: i32 = stmt.bind_value("status").map_err(map_err)?;
        if status != 0 {
            break;
        }
        let line: Option<String> = stmt.bind_value("line").map_err(map_err)?;
        lines.push(line.unwrap_or_default());
    }
    Ok(lines)
}

pub(crate) struct OraExec {
    pending: VecDeque<ExecEvent>,
    rows: Option<ResultSet<'static, OraRow>>,
}

fn convert_row(row: &OraRow) -> Result<Row> {
    row.sql_values().iter().map(oracle_value).collect()
}

impl BlockingExec for OraExec {
    fn next(&mut self, max_rows: usize) -> Result<Option<ExecEvent>> {
        if let Some(event) = self.pending.pop_front() {
            return Ok(Some(event));
        }
        let Some(rs) = self.rows.as_mut() else {
            return Ok(None);
        };
        let mut rows = Vec::new();
        let mut ended = false;
        while rows.len() < max_rows.max(1) {
            match rs.next() {
                Some(row) => rows.push(convert_row(&row.map_err(map_err)?)?),
                None => {
                    ended = true;
                    break;
                }
            }
        }
        if ended {
            self.rows = None;
            if rows.is_empty() {
                return Ok(Some(ExecEvent::ResultEnd));
            }
            self.pending.push_back(ExecEvent::ResultEnd);
        }
        Ok(Some(ExecEvent::Rows(rows)))
    }
}

impl BlockingConnection for OracleConnection {
    fn execute<'a>(&'a mut self, sql: &'a str, params: &'a [Value]) -> Result<Box<dyn BlockingExec + 'a>> {
        let binds = to_binds(params)?;
        let refs: Vec<&dyn ToSql> = binds.iter().map(|b| b.as_ref()).collect();
        let mut stmt = self.conn.statement(sql).fetch_array_size(FETCH_ARRAY_SIZE).build().map_err(map_err)?;
        if stmt.is_query() {
            let rs = stmt.into_result_set::<OraRow>(&refs).map_err(map_err)?;
            let columns: Vec<ColumnMeta> = rs.column_info().iter().map(column_meta).collect();
            return Ok(Box::new(OraExec {
                pending: VecDeque::from([ExecEvent::ResultStart { columns }]),
                rows: Some(rs),
            }));
        }
        stmt.execute(&refs).map_err(map_err)?;
        let affected = stmt.row_count().map_err(map_err)?;
        let mut pending: VecDeque<ExecEvent> = VecDeque::new();
        if stmt.is_plsql() {
            pending.extend(drain_output(&self.conn)?.into_iter().map(ExecEvent::Message));
        }
        if self.autocommit && !stmt.is_ddl() {
            self.conn.commit().map_err(map_err)?;
        }
        pending.push_back(ExecEvent::Affected(affected));
        Ok(Box::new(OraExec { pending, rows: None }))
    }

    fn set_autocommit(&mut self, on: bool) -> Result<()> {
        // 从手动切回自动时提交未完成的事务（与 JDBC 行为一致）
        if on && !self.autocommit {
            self.conn.commit().map_err(map_err)?;
        }
        self.autocommit = on;
        Ok(())
    }

    fn commit(&mut self) -> Result<()> {
        self.conn.commit().map_err(map_err)
    }

    fn rollback(&mut self) -> Result<()> {
        self.conn.rollback().map_err(map_err)
    }

    fn ping(&mut self) -> Result<()> {
        self.conn.ping().map_err(map_err)
    }

    fn server_info(&mut self) -> Result<ServerInfo> {
        let (_, banner) = self.conn.server_version().map_err(map_err)?;
        Ok(ServerInfo { product: "Oracle".into(), version: banner })
    }

    fn close(self: Box<Self>) -> Result<()> {
        self.conn.close().map_err(map_err)
    }
}
```

把 `crates/dblens-driver-oracle/src/lib.rs` 改为：
```rust
//! Oracle 驱动：基于 `oracle` crate（ODPI-C），运行时加载 Oracle Instant Client。

mod conn;
mod error;
mod value;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use dblens_core::{Error, Result};
use dblens_driver::blocking::into_async;
use dblens_driver::native::{self, NativeLib};
use dblens_driver::{CancelHandle, Capabilities, ConnectParams, Connection, Driver, DriverId};

use crate::conn::OracleConnection;
use crate::error::map_err;

pub struct OracleDriver;

/// Oracle 客户端库在进程内只能初始化一次；只记录成功的初始化，失败后可以换目录重试。
static CLIENT_DIR: Mutex<Option<PathBuf>> = Mutex::new(None);

fn init_client(p: &ConnectParams) -> Result<()> {
    let mut guard = CLIENT_DIR.lock().map_err(|_| Error::internal("Oracle 客户端初始化锁已损坏"))?;
    if guard.is_some() {
        return Ok(());
    }
    let configured = p.options.get("oracle_client_lib_dir").map(PathBuf::from);
    let dir = native::locate(NativeLib::OracleClient, configured.as_deref())?;
    oracle::InitParams::new().oracle_client_lib_dir(dir.as_os_str()).map_err(map_err)?.init().map_err(map_err)?;
    *guard = Some(dir);
    Ok(())
}

/// 生成 Easy Connect 连接串；指定了 `sid` 选项时改用完整描述符。
pub(crate) fn connect_string(p: &ConnectParams) -> Result<String> {
    let timeout = p.connect_timeout.as_secs();
    if let Some(sid) = p.options.get("sid") {
        return Ok(format!(
            "(DESCRIPTION=(CONNECT_TIMEOUT={timeout})(ADDRESS=(PROTOCOL=TCP)(HOST={})(PORT={}))(CONNECT_DATA=(SID={sid})))",
            p.host, p.port
        ));
    }
    let service = p.database.as_deref().ok_or_else(|| Error::connect("Oracle 连接需要服务名（URL 路径部分）或 sid 选项"))?;
    Ok(format!("//{}:{}/{service}?connect_timeout={timeout}", p.host, p.port))
}

#[async_trait]
impl Driver for OracleDriver {
    fn id(&self) -> DriverId {
        DriverId::Oracle
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            catalogs: false,
            schemas: true,
            transactional_ddl: false,
            cancel: true,
            explain: true,
            multiple_result_sets: false,
        }
    }

    async fn connect(&self, p: &ConnectParams) -> Result<Box<dyn Connection>> {
        let p = p.clone();
        let conn = tokio::task::spawn_blocking(move || -> Result<oracle::Connection> {
            init_client(&p)?;
            let conn = oracle::Connector::new(p.user.as_str(), p.password.as_str(), connect_string(&p)?)
                .connect()
                .map_err(map_err)?;
            conn.execute("BEGIN DBMS_OUTPUT.ENABLE(NULL); END;", &[]).map_err(map_err)?;
            Ok(conn)
        })
        .await
        .map_err(|e| Error::internal(format!("连接任务异常退出：{e}")))??;
        let conn = Arc::new(conn);
        let cancel = {
            let conn = conn.clone();
            CancelHandle::new(move || {
                let conn = conn.clone();
                async move {
                    tokio::task::spawn_blocking(move || conn.break_execution())
                        .await
                        .map_err(|e| Error::internal(format!("取消任务异常退出：{e}")))?
                        .map_err(map_err)
                }
            })
        };
        into_async(OracleConnection::new(conn), cancel, "dblens-oracle")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_connect_strings() {
        let (_, p) = ConnectParams::from_url("oracle://u:p@db:1521/FREEPDB1?connect_timeout=5").unwrap();
        assert_eq!(connect_string(&p).unwrap(), "//db:1521/FREEPDB1?connect_timeout=5");
        let (_, p) = ConnectParams::from_url("oracle://u:p@db:1521?sid=ORCL").unwrap();
        assert!(connect_string(&p).unwrap().contains("(SID=ORCL)"));
        let (_, p) = ConnectParams::from_url("oracle://u:p@db:1521").unwrap();
        assert!(connect_string(&p).is_err());
    }
}
```

- [ ] **Step 7: 运行一致性测试**

```powershell
$env:DBLENS_TEST_ORACLE_URL = "oracle://用户:密码@主机:1521/FREEPDB1?oracle_client_lib_dir=C:\oracle\instantclient_23_x"
cargo test -p dblens-driver-oracle -- --nocapture --test-threads=1
```
（如果 Instant Client 放在 `C:\oracle\instantclient_*` 这类常见位置，可以不写 `oracle_client_lib_dir`。）

Expected: 单元测试 3 个，一致性测试 9 个，`dbms_output_becomes_message` 1 个，全部 PASS，没有"跳过"。排查规则同 Task 4 Step 7。

再按并行方式连跑 3 次，确认稳定：
```powershell
1..3 | ForEach-Object { cargo test -q -p dblens-driver-oracle --test conformance -- --nocapture 2>&1 | Select-String "test result|跳过|FAILED" }
```

- [ ] **Step 8: clippy 与提交**

Run: `cargo clippy -p dblens-driver-oracle --all-targets -- -D warnings`
Expected: 没有警告。

```bash
git add Cargo.lock crates/dblens-driver-oracle
git commit -m "feat(oracle): Oracle 驱动（Instant Client 运行时加载、流式取数、取消、DBMS_OUTPUT、LOB 预览）" -m "一致性测试 9 项与 DBMS_OUTPUT 测试已在真实 Oracle 上通过。" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 6: CI 与文档收尾

**Files:**
- Modify: `.github/workflows/ci.yml`、`README.md`、`docs/superpowers/plans/2026-10-22-m0-overview.md`

**Interfaces:**
- Consumes: 全部 crate
- Produces: CI 中的 Oracle 集成测试；README 中新的测试环境变量；总览中的状态更新

- [ ] **Step 1: CI 增加 Oracle 集成测试任务**

在 `.github/workflows/ci.yml` 的 `jobs:` 下追加：
```yaml
  oracle:
    name: Oracle 驱动集成测试
    runs-on: ubuntu-latest
    services:
      oracle:
        image: gvenzl/oracle-free:23-slim
        env:
          ORACLE_PASSWORD: dblens
          APP_USER: dblens
          APP_USER_PASSWORD: dblens
        ports: ["1521:1521"]
        options: >-
          --health-cmd healthcheck.sh
          --health-interval 10s --health-timeout 5s --health-retries 30
    env:
      DBLENS_TEST_ORACLE_URL: oracle://dblens:dblens@127.0.0.1:1521/FREEPDB1?oracle_client_lib_dir=/opt/oracle/instantclient
    steps:
      - uses: actions/checkout@v4
      - name: 安装 Oracle Instant Client
        run: |
          sudo apt-get update && sudo apt-get install -y libaio1t64 unzip
          sudo ln -sf /usr/lib/x86_64-linux-gnu/libaio.so.1t64 /usr/lib/x86_64-linux-gnu/libaio.so.1
          curl -sSLo /tmp/ic.zip https://download.oracle.com/otn_software/linux/instantclient/instantclient-basiclite-linuxx64.zip
          sudo mkdir -p /opt/oracle && sudo unzip -q /tmp/ic.zip -d /opt/oracle
          sudo ln -sfn "$(ls -d /opt/oracle/instantclient_* | head -1)" /opt/oracle/instantclient
      - run: rustup show
      - uses: Swatinem/rust-cache@v2
      - name: 运行 Oracle 一致性测试（不允许跳过）
        run: |
          cargo test -p dblens-driver-oracle -- --nocapture 2>&1 | tee test.log
          if grep -q "跳过" test.log; then echo "有测试被跳过，环境变量未生效"; exit 1; fi
```

达梦没有可以在 CI 中自由拉取的官方镜像，达梦的集成测试只在本地运行（README 中注明）。

- [ ] **Step 2: 更新 README 的测试数据库表格**

在 README "集成测试数据库"一节的表格末尾追加两行，并在表格下方追加说明：
```markdown
| `DBLENS_TEST_DM_URL` | `dm://SYSDBA:密码@127.0.0.1:5236` |
| `DBLENS_TEST_ORACLE_URL` | `oracle://dblens:dblens@127.0.0.1:1521/FREEPDB1` |

- 达梦和 Oracle 驱动在运行时加载本机的客户端库：达梦需要安装达梦客户端（dmdpi），Oracle 需要 Instant Client。库所在目录可以用 URL 参数 `dpi_lib_dir` / `oracle_client_lib_dir` 指定，不指定时会按 `DM_HOME` / `ORACLE_HOME`、PATH 和常见安装位置自动查找。
- 达梦集成测试只能在本地运行（CI 中没有达梦镜像）；达梦 DPI 技术验证：`cargo test -p dblens-driver-dameng spike -- --ignored --nocapture --test-threads=1`。
```

- [ ] **Step 3: 更新总览**

把 `docs/superpowers/plans/2026-10-22-m0-overview.md` 子计划表中 M0-2 一行的状态改为：
`完成：详细计划 2026-10-22-m0-2-dameng-and-oracle-drivers.md；达梦 DPI 技术验证结论见 docs/notes/dameng-dpi-spike.md`

- [ ] **Step 4: 全量验证**

在设置了全部五个测试环境变量（MySQL、MariaDB、PG、DM、Oracle）的情况下：
```bash
cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace -- --nocapture 2>&1 | tee /tmp/all.log | grep -E "test result|FAILED"; grep -c "跳过" /tmp/all.log
```
Expected: fmt 和 clippy 通过；所有 `test result` 为 ok；"跳过"计数为 0（技术验证测试是 `#[ignore]`，不会被计入）。

- [ ] **Step 5: 提交**

```bash
git add .github README.md docs/superpowers/plans/2026-10-22-m0-overview.md
git commit -m "ci: Oracle 驱动集成测试；文档：达梦与 Oracle 测试环境说明" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```
