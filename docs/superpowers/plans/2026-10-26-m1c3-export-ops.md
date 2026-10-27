# M1c3 导出与运维查询 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 查询结果和数据标签可以导出为 CSV、xlsx、INSERT、JSON（后端重新执行，读完整大字段）；新增按方言的运维查询侧栏。

**Architecture:** 驱动连接增加大字段读取上限；`dblens-service` 的导出任务另开连接执行、边读边写文件，前端轮询进度，桌面版写到"另存为"选的路径，浏览器模式写临时文件后下载。运维查询是 `dblens-dialect` 中按方言的 SQL 模板，接口按连接的变体列出，前端点击后新开标签执行。

**Tech Stack:** Rust（axum、tokio、csv、rust_xlsxwriter、Tauri 的 dialog 插件）；React 19 + TypeScript、antd 6（Modal、notification）、zustand、@tanstack/react-query 5、Vitest。

**Spec:** `docs/superpowers/specs/2026-10-26-m1c3-export-ops-design.md`

## Global Constraints

- 导出只接受单条查询语句；另开连接执行，不占用标签的会话，看不到标签中未提交的修改；大字段每个值最多读 256 MB，超出的截断并计入警告。
- 同时进行的导出任务最多 4 个；已结束的任务保留 1 小时。
- 只有桌面程序的本机服务接受保存路径（绝对路径、所在目录存在）；浏览器模式写临时文件后下载，下载后删除。
- 结束会话的语句（`kill_sql` 列）只生成，不执行。运维查询缺权限或缺插件时显示数据库的报错。
- 依赖只用正式发布版，跟随最新；新版发布满 7 天再升级。本里程碑新增 `csv` 1.4.0、`rust_xlsxwriter` 0.100.0。
- 前端给用户看的文字都用词条（中英文），英文区分单复数；后端错误文字用 `msg!` 词条；日志不翻译。
- 代码注释用中文；文件 UTF-8、LF 换行；前端代码风格：无分号、单引号、两格缩进。
- cargo 命令加 `--config "source.mirror.registry='sparse+https://rsproxy.cn/index/'"`（clippy 写在 `clippy` 之后），并设 `CARGO_INCREMENTAL=0`；后端测试在 `DBLENS_LANG=en` 下运行。
- 前端工具用 PowerShell 执行（`npm`/`npx` 在本机不可用，用 `pnpm exec`）。
- 测试数据库的密码只在命令行中以内联环境变量传入，不写进仓库、报告或提交信息。

## 与设计文档的差异（写计划时实测后调整）

- 导出任务直接用 `registry.connect` 打开连接，不经过会话管理：会话的事件接口是给界面用的，导出不需要确认、事务和 schema 跟踪。照常写执行历史。
- 运维查询的列表接口不连接数据库：按连接上次识别出的变体选模板，没连接过时按驱动。
- 临时目录在服务启动时清空（设计文档写的是退出时），也能清掉异常退出留下的文件。
- 取消的竞态：取消先置标志再取取消句柄，任务存好句柄后先检查标志。

## 执行说明

任务 2–8 的代码都已在临时副本中逐任务实现并通过检查（含 5 个实库和浏览器实测），执行时按顺序重放。新文件给出全文，修改的文件给出 diff（基于上一个任务完成后的状态，可直接 `git apply`）。`Cargo.lock` 在任务 4 中随新依赖由 cargo 自动更新，计划中不列出；`web/pnpm-lock.yaml` 不变（任务 1 有升级时除外）。

## 文件结构

```text
crates/dblens-driver/src/{traits.rs, blocking.rs, conformance.rs}     大字段读取上限
crates/dblens-driver-{mysql,postgres,oracle,dameng}/src/            各驱动按上限读取
crates/dblens-dialect/sql/ops/*.sql; src/ops.rs                     运维查询模板（新增）
crates/dblens-service/src/export/{mod.rs, write.rs, job.rs}         导出（新增）
crates/dblens-service/src/{service.rs, lib.rs, fake.rs}; tests/live.rs   Service::ops、组装、假驱动、实库测试
crates/dblens-api/src/exports.rs; src/lib.rs; tests/api.rs          接口（新增）
crates/dblens-desktop/src/shell.rs; build.rs; capabilities/main.json   另存为、在文件夹中显示
crates/dblens-core/locales/app.json                                 后端词条
docs/notes/m0-4b-api.md                                             接口说明
web/src/shared/{api.ts, shell.ts}                                    接口客户端、原生能力
web/src/features/export/                                            导出对话框（新增）
web/src/features/ops/                                               运维查询侧栏（新增）
web/src/features/{editor,results,data}/                             导出入口、打开后执行
web/src/app/Shell.tsx; web/src/locales/                             侧栏、词条
docs/notes/m1c3-export-ops.md                                       说明文档（新增）
```

---

### Task 1: 依赖复查

按依赖原则（M1b2 设计文档 3.1 节）复查前端和后端的全部直接依赖。本里程碑新增的两个依赖（`csv`、`rust_xlsxwriter`）在任务 4 中加入，版本见任务 4。

**Files:** 视复查结果而定（`web/package.json`、`Cargo.toml` 及锁文件）。

- [ ] **Step 1: 运行复查脚本**

```powershell
$env:PATH = "D:\Environments\nvm4w\nodejs;$env:PATH"; $env:NPM_REGISTRY = 'https://registry.npmmirror.com'; node scripts/check-deps.mjs
```

Expected：2026-10-26 写计划时：共 75 个直接依赖，0 个可升级（当天唯一可升级的 i18next 26.5.3 已在 M1c2 执行时升级）。以执行当天的结果为准。crates.io 偶尔"查询失败：fetch failed"，重跑即可。

- [ ] **Step 2: 升级（有可升级项时）**

输出中 `->` 的都升级：npm 用 `pnpm add 包@版本`（在 `web` 目录），Cargo 改工作区 `Cargo.toml` 后执行 `cargo update -p 包`。然后跑全量检查：

```bash
export CARGO_INCREMENTAL=0
cargo fmt --all
cargo clippy --config "source.mirror.registry='sparse+https://rsproxy.cn/index/'" --workspace --all-targets -- -D warnings
DBLENS_LANG=en cargo test --config "source.mirror.registry='sparse+https://rsproxy.cn/index/'" --workspace
```

在 PowerShell 中执行（bash 里 `cd web` 会触发坏掉的 fnm 钩子，`&&` 后面的命令会被跳过）：

```powershell
$env:PATH = "D:\Environments\nvm4w\nodejs;$env:PATH"; Set-Location web; pnpm typecheck; pnpm test; pnpm build
```

Expected：全部通过。结果记进任务 8 的说明文档（"依赖"一节）。

- [ ] **Step 3: 提交（有改动时）**

```bash
git add web/package.json web/pnpm-lock.yaml Cargo.toml Cargo.lock
git commit -m "chore: 依赖复查：升级到发布满 7 天的最新正式版" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 2: 驱动：大字段的读取上限

现在 4 个驱动读大字段时都只取前 64 KB（`DEFAULT_LOB_PREVIEW_BYTES`），导出会被截断。`Connection` 增加 `set_lob_limit`：
- 新连接仍为 64 KB；导出任务的连接放宽（任务 4）。
- MySQL、PG 读出整个值后按上限 `cap_large`；Oracle、达梦分块读取 LOB，读到上限为止。
- 阻塞适配器（Oracle、达梦）用不需要回复的命令转发，先释放被丢弃的执行流。
- 一致性测试增加 `lob_limit_reads_full_value`：放宽后读出 100000 字节的完整文本，改回默认后又是预览。

**Files:**
- Modify: `crates/dblens-driver-dameng/src/conn.rs`
- Modify: `crates/dblens-driver-dameng/src/types.rs`
- Modify: `crates/dblens-driver-mysql/src/conn.rs`
- Modify: `crates/dblens-driver-mysql/src/exec.rs`
- Modify: `crates/dblens-driver-mysql/src/lib.rs`
- Modify: `crates/dblens-driver-oracle/src/conn.rs`
- Modify: `crates/dblens-driver-oracle/src/value.rs`
- Modify: `crates/dblens-driver-postgres/src/conn.rs`
- Modify: `crates/dblens-driver-postgres/src/exec.rs`
- Modify: `crates/dblens-driver-postgres/src/lib.rs`
- Modify: `crates/dblens-driver/src/blocking.rs`
- Modify: `crates/dblens-driver/src/conformance.rs`
- Modify: `crates/dblens-driver/src/traits.rs`
- Modify: `crates/dblens-driver/tests/blocking.rs`
- Modify: `crates/dblens-driver/tests/fake_driver.rs`
- Modify: `crates/dblens-service/src/fake.rs`

**Interfaces:**
- Produces（`dblens_driver`）：
  - `Connection::set_lob_limit(&mut self, bytes: usize)`（同步方法，只影响之后执行的语句）；
  - `BlockingConnection::set_lob_limit(&mut self, bytes: usize)`；
  - 测试用的假连接（`dblens-driver/tests`、`dblens-service/src/fake.rs`）都实现它；服务层的假连接把它记进日志 `lob_limit <字节数>`。

- [ ] **Step 1: 修改 `crates/dblens-driver/tests/blocking.rs`**

```diff
--- a/crates/dblens-driver/tests/blocking.rs
+++ b/crates/dblens-driver/tests/blocking.rs
@@ -38,6 +38,8 @@ impl BlockingExec for FakeExec {
 }
 
 impl BlockingConnection for Fake {
+    fn set_lob_limit(&mut self, _bytes: usize) {}
+
     fn execute<'a>(&'a mut self, sql: &'a str, _params: &'a [Value]) -> Result<Box<dyn BlockingExec + 'a>> {
         let thread = std::thread::current().name().unwrap_or("").to_string();
         self.log.lock().unwrap().push(format!("{thread}:{sql}"));
```

- [ ] **Step 2: 修改 `crates/dblens-driver/tests/fake_driver.rs`**

```diff
--- a/crates/dblens-driver/tests/fake_driver.rs
+++ b/crates/dblens-driver/tests/fake_driver.rs
@@ -58,6 +58,8 @@ impl Connection for FakeConn {
             started: false,
         }))
     }
+    fn set_lob_limit(&mut self, _bytes: usize) {}
+
     fn cancel_handle(&self) -> CancelHandle {
         let counter = self.cancelled.clone();
         CancelHandle::new(move || {
```

- [ ] **Step 3: 修改 `crates/dblens-driver-dameng/src/conn.rs`**

```diff
--- a/crates/dblens-driver-dameng/src/conn.rs
+++ b/crates/dblens-driver-dameng/src/conn.rs
@@ -11,7 +11,7 @@ use dblens_driver::{CancelHandle, ColumnMeta, ExecEvent, ServerInfo};
 use crate::ffi::*;
 use crate::handles::{ColDesc, Con, Stmt, succeeded};
 use crate::position::with_position;
-use crate::types::{DmType, PREVIEW, ParamData, bytes_value, classify, param_data, type_name, with_scale};
+use crate::types::{DmType, ParamData, bytes_value, classify, param_data, type_name, with_scale};
 
 /// 语句被 dpi_cancel 中断时 DM 返回的错误码（「操作被取消」，见技术验证笔记）。
 const CANCELLED_CODE: &str = "-6515";
@@ -41,6 +41,7 @@ pub(crate) struct DmConnection {
     cancelled: Arc<AtomicBool>,
     /// 当前是否为自动提交；连接建立时已开启。
     autocommit: bool,
+    lob_limit: usize,
 }
 
 impl DmConnection {
@@ -50,6 +51,7 @@ impl DmConnection {
             current: Arc::new(Mutex::new(None)),
             cancelled: Arc::new(AtomicBool::new(false)),
             autocommit: true,
+            lob_limit: dblens_core::DEFAULT_LOB_PREVIEW_BYTES,
         }
     }
 
@@ -156,15 +158,18 @@ pub(crate) struct DmExec<'a> {
     columns: Columns,
     pending: VecDeque<ExecEvent>,
     state: State,
+    lob_limit: usize,
 }
 
 impl DmExec<'_> {
     fn read_row(&self) -> Result<Row> {
         let mut row = Vec::with_capacity(self.columns.len());
         for (i, (desc, kind)) in self.columns.iter().enumerate() {
-            let value = match self.stmt.get_data(i + 1, kind.is_binary(), PREVIEW)? {
+            let value = match self.stmt.get_data(i + 1, kind.is_binary(), self.lob_limit)? {
                 None => Value::Null,
-                Some((bytes, total)) => with_scale(bytes_value(*kind, bytes, total, PREVIEW), desc.scale),
+                Some((bytes, total)) => {
+                    with_scale(bytes_value(*kind, bytes, total, self.lob_limit), desc.scale)
+                }
             };
             row.push(value);
         }
@@ -214,6 +219,10 @@ impl Drop for DmExec<'_> {
 }
 
 impl BlockingConnection for DmConnection {
+    fn set_lob_limit(&mut self, bytes: usize) {
+        self.lob_limit = bytes;
+    }
+
     fn execute<'a>(&'a mut self, sql: &'a str, params: &'a [Value]) -> Result<Box<dyn BlockingExec + 'a>> {
         self.cancelled.store(false, Ordering::SeqCst);
         let stmt = Stmt::new(&self.con)?;
@@ -282,6 +291,7 @@ impl BlockingConnection for DmConnection {
             columns,
             pending,
             state,
+            lob_limit: self.lob_limit,
         }))
     }
 
```

- [ ] **Step 4: 修改 `crates/dblens-driver-dameng/src/types.rs`**

```diff
--- a/crates/dblens-driver-dameng/src/types.rs
+++ b/crates/dblens-driver-dameng/src/types.rs
@@ -4,7 +4,7 @@
 use base64::Engine;
 use base64::engine::general_purpose::STANDARD;
 use chrono::{DateTime, NaiveDate, NaiveDateTime, NaiveTime};
-use dblens_core::{DEFAULT_LOB_PREVIEW_BYTES, Error, LobPreview, Result, Value, msg};
+use dblens_core::{Error, LobPreview, Result, Value, msg};
 
 use crate::ffi::*;
 
@@ -208,7 +208,6 @@ pub(crate) fn bytes_value(kind: DmType, bytes: Vec<u8>, total: Option<u64>, limi
 }
 
 /// 默认的大字段预览长度，供 conn.rs 使用。
-pub(crate) const PREVIEW: usize = DEFAULT_LOB_PREVIEW_BYTES;
 
 #[derive(Debug, Clone, PartialEq, Eq)]
 pub(crate) enum ParamData {
```

- [ ] **Step 5: 修改 `crates/dblens-driver-mysql/src/conn.rs`**

```diff
--- a/crates/dblens-driver-mysql/src/conn.rs
+++ b/crates/dblens-driver-mysql/src/conn.rs
@@ -11,6 +11,7 @@ use crate::value::to_mysql_param;
 pub struct MySqlConnection {
     pub(crate) conn: Conn,
     pub(crate) opts: Opts,
+    pub(crate) lob_limit: usize,
 }
 
 #[async_trait]
@@ -23,15 +24,19 @@ impl Connection for MySqlConnection {
         if params.is_empty() {
             // 文本协议，支持一次返回多个结果集（如 CALL）
             let result = self.conn.query_iter(sql).await.map_err(|e| with_position(map_err(e), sql))?;
-            Ok(Box::new(MyExec::new(result)))
+            Ok(Box::new(MyExec::new(result, self.lob_limit)))
         } else {
             let values = params.iter().map(to_mysql_param).collect::<Result<Vec<_>>>()?;
             let result =
                 self.conn.exec_iter(sql, values).await.map_err(|e| with_position(map_err(e), sql))?;
-            Ok(Box::new(MyExec::new(result)))
+            Ok(Box::new(MyExec::new(result, self.lob_limit)))
         }
     }
 
+    fn set_lob_limit(&mut self, bytes: usize) {
+        self.lob_limit = bytes;
+    }
+
     /// 另开一个连接执行 `KILL QUERY <线程号>`。
     fn cancel_handle(&self) -> CancelHandle {
         let opts = self.opts.clone();
```

- [ ] **Step 6: 修改 `crates/dblens-driver-mysql/src/exec.rs`**

```diff
--- a/crates/dblens-driver-mysql/src/exec.rs
+++ b/crates/dblens-driver-mysql/src/exec.rs
@@ -1,7 +1,7 @@
 use std::sync::Arc;
 
 use async_trait::async_trait;
-use dblens_core::{DEFAULT_LOB_PREVIEW_BYTES, Result, Row};
+use dblens_core::{Result, Row};
 use dblens_driver::{ExecEvent, ExecStream};
 use mysql_async::prelude::Protocol;
 use mysql_async::{Column, QueryResult};
@@ -23,20 +23,17 @@ pub(crate) struct MyExec<'a, P: Protocol> {
     result: QueryResult<'a, 'static, P>,
     columns: Arc<[Column]>,
     state: State,
+    lob_limit: usize,
 }
 
 impl<'a, P: Protocol> MyExec<'a, P> {
-    pub(crate) fn new(result: QueryResult<'a, 'static, P>) -> Self {
-        Self { result, columns: Arc::from(Vec::new()), state: State::Between }
+    pub(crate) fn new(result: QueryResult<'a, 'static, P>, lob_limit: usize) -> Self {
+        Self { result, columns: Arc::from(Vec::new()), state: State::Between, lob_limit }
     }
 }
 
-fn convert_row(row: mysql_async::Row, columns: &[Column]) -> Row {
-    row.unwrap()
-        .into_iter()
-        .zip(columns)
-        .map(|(v, c)| to_value(v, c).cap_large(DEFAULT_LOB_PREVIEW_BYTES))
-        .collect()
+fn convert_row(row: mysql_async::Row, columns: &[Column], lob_limit: usize) -> Row {
+    row.unwrap().into_iter().zip(columns).map(|(v, c)| to_value(v, c).cap_large(lob_limit)).collect()
 }
 
 #[async_trait]
@@ -65,7 +62,7 @@ where
                 let mut rows = Vec::new();
                 while rows.len() < max_rows.max(1) {
                     match self.result.next().await.map_err(map_err)? {
-                        Some(row) => rows.push(convert_row(row, &self.columns)),
+                        Some(row) => rows.push(convert_row(row, &self.columns, self.lob_limit)),
                         None => {
                             self.state = State::EndPending;
                             break;
```

- [ ] **Step 7: 修改 `crates/dblens-driver-mysql/src/lib.rs`**

```diff
--- a/crates/dblens-driver-mysql/src/lib.rs
+++ b/crates/dblens-driver-mysql/src/lib.rs
@@ -76,7 +76,7 @@ impl Driver for MySqlDriver {
                 Error::connect(msg!("driver.connect_timeout", seconds = p.connect_timeout.as_secs()))
             })?
             .map_err(map_err)?;
-        Ok(Box::new(MySqlConnection { conn, opts }))
+        Ok(Box::new(MySqlConnection { conn, opts, lob_limit: dblens_core::DEFAULT_LOB_PREVIEW_BYTES }))
     }
 }
 
```

- [ ] **Step 8: 修改 `crates/dblens-driver-oracle/src/conn.rs`**

```diff
--- a/crates/dblens-driver-oracle/src/conn.rs
+++ b/crates/dblens-driver-oracle/src/conn.rs
@@ -19,11 +19,12 @@ pub(crate) struct OracleConnection {
     conn: Arc<oracle::Connection>,
     /// 自行维护的自动提交标志（`oracle::Connection::set_autocommit` 需要 &mut，而连接要与取消句柄共享）。
     autocommit: bool,
+    lob_limit: usize,
 }
 
 impl OracleConnection {
     pub(crate) fn new(conn: Arc<oracle::Connection>) -> Self {
-        OracleConnection { conn, autocommit: true }
+        OracleConnection { conn, autocommit: true, lob_limit: dblens_core::DEFAULT_LOB_PREVIEW_BYTES }
     }
 }
 
@@ -51,15 +52,13 @@ fn drain_output(conn: &oracle::Connection) -> Result<Vec<String>> {
 pub(crate) struct OraExec {
     pending: VecDeque<ExecEvent>,
     rows: Option<ResultSet<'static, OraRow>>,
+    lob_limit: usize,
 }
 
-fn convert_row(row: &OraRow) -> Result<Row> {
+fn convert_row(row: &OraRow, lob_limit: usize) -> Result<Row> {
     // CLOB/NCLOB/BLOB 在 oracle_value 中已按预览读取（得到 Value::Lob，cap_large 对其原样放行）；
     // 这里再统一截断 LONG、LONG RAW 和超长 VARCHAR2 等非 LOB 的大值，与 MySQL、PG、达梦驱动一致
-    row.sql_values()
-        .iter()
-        .map(|v| oracle_value(v).map(|v| v.cap_large(dblens_core::DEFAULT_LOB_PREVIEW_BYTES)))
-        .collect()
+    row.sql_values().iter().map(|v| oracle_value(v, lob_limit).map(|v| v.cap_large(lob_limit))).collect()
 }
 
 impl BlockingExec for OraExec {
@@ -74,7 +73,7 @@ impl BlockingExec for OraExec {
         let mut ended = false;
         while rows.len() < max_rows.max(1) {
             match rs.next() {
-                Some(row) => rows.push(convert_row(&row.map_err(map_err)?)?),
+                Some(row) => rows.push(convert_row(&row.map_err(map_err)?, self.lob_limit)?),
                 None => {
                     ended = true;
                     break;
@@ -93,6 +92,10 @@ impl BlockingExec for OraExec {
 }
 
 impl BlockingConnection for OracleConnection {
+    fn set_lob_limit(&mut self, bytes: usize) {
+        self.lob_limit = bytes;
+    }
+
     fn execute<'a>(&'a mut self, sql: &'a str, params: &'a [Value]) -> Result<Box<dyn BlockingExec + 'a>> {
         let binds = to_binds(params)?;
         let refs: Vec<&dyn ToSql> = binds.iter().map(|b| b.as_ref()).collect();
@@ -112,6 +115,7 @@ impl BlockingConnection for OracleConnection {
             return Ok(Box::new(OraExec {
                 pending: VecDeque::from([ExecEvent::ResultStart { columns }]),
                 rows: Some(rs),
+                lob_limit: self.lob_limit,
             }));
         }
         if let Err(e) = stmt.execute(&refs) {
@@ -127,7 +131,7 @@ impl BlockingConnection for OracleConnection {
             self.conn.commit().map_err(map_err)?;
         }
         pending.push_back(ExecEvent::Affected(affected));
-        Ok(Box::new(OraExec { pending, rows: None }))
+        Ok(Box::new(OraExec { pending, rows: None, lob_limit: self.lob_limit }))
     }
 
     fn set_autocommit(&mut self, on: bool) -> Result<()> {
```

- [ ] **Step 9: 修改 `crates/dblens-driver-oracle/src/value.rs`**

```diff
--- a/crates/dblens-driver-oracle/src/value.rs
+++ b/crates/dblens-driver-oracle/src/value.rs
@@ -3,7 +3,7 @@
 use std::io::Read;
 
 use chrono::{DateTime, FixedOffset, NaiveDateTime};
-use dblens_core::{DEFAULT_LOB_PREVIEW_BYTES, Error, Result, Value, msg};
+use dblens_core::{Error, Result, Value, msg};
 use dblens_driver::ColumnMeta;
 use oracle::sql_type::{Blob, Clob, Lob, Nclob, OracleType, ToSql};
 use oracle::{ColumnInfo, SqlValue};
@@ -38,7 +38,7 @@ pub(crate) fn column_meta(info: &ColumnInfo) -> ColumnMeta {
 }
 
 /// `complete` 为 false 时，`bytes` 超过了预览长度，cap_large 一定会把它转成预览。
-fn lob_preview(binary: bool, bytes: Vec<u8>, total: u64, complete: bool) -> Value {
+fn lob_preview(binary: bool, bytes: Vec<u8>, total: u64, complete: bool, limit: usize) -> Value {
     let value = if binary {
         Value::Bytes(bytes)
     } else {
@@ -47,7 +47,7 @@ fn lob_preview(binary: bool, bytes: Vec<u8>, total: u64, complete: bool) -> Valu
     if complete {
         return value;
     }
-    match value.cap_large(DEFAULT_LOB_PREVIEW_BYTES) {
+    match value.cap_large(limit) {
         Value::Lob(mut lob) => {
             lob.total_len = total;
             Value::Lob(lob)
@@ -65,12 +65,12 @@ const LOB_READ_CHUNK: usize = 16 * 1024;
 /// 因此 CLOB/NCLOB 预览的 `LobPreview.total_len` 是 UTF-16 代码单元数（字符数），而 BLOB 和
 /// `cap_large` 产生的预览（以及 MySQL、PG、达梦驱动）是字节数。各驱动的单位尚未统一，
 /// 使用方不得假定 `total_len` 在不同驱动之间单位相同。
-fn read_lob(mut lob: impl Read + Lob, binary: bool) -> Result<Value> {
+fn read_lob(mut lob: impl Read + Lob, binary: bool, limit: usize) -> Result<Value> {
     let total = lob.size().map_err(map_err)?;
     let mut bytes = Vec::new();
     let mut chunk = vec![0u8; LOB_READ_CHUNK];
     // 读到超过预览长度或读完为止；每次都是整块读取，字符 LOB 由 crate 保证只返回完整字符
-    while bytes.len() <= DEFAULT_LOB_PREVIEW_BYTES {
+    while bytes.len() <= limit {
         let n =
             lob.read(&mut chunk).map_err(|e| Error::internal(msg!("driver.lob_read_failed", error = e)))?;
         if n == 0 {
@@ -85,11 +85,12 @@ fn read_lob(mut lob: impl Read + Lob, binary: bool) -> Result<Value> {
     } else {
         String::from_utf8_lossy(&bytes).encode_utf16().count() as u64
     };
-    let complete = read_units == total && bytes.len() <= DEFAULT_LOB_PREVIEW_BYTES;
-    Ok(lob_preview(binary, bytes, total, complete))
+    let complete = read_units == total && bytes.len() <= limit;
+    Ok(lob_preview(binary, bytes, total, complete, limit))
 }
 
-pub(crate) fn oracle_value(v: &SqlValue) -> Result<Value> {
+/// `lob_limit`：CLOB/NCLOB/BLOB 最多读取的字节数（见 `Connection::set_lob_limit`）。
+pub(crate) fn oracle_value(v: &SqlValue, lob_limit: usize) -> Result<Value> {
     if v.is_null().map_err(map_err)? {
         return Ok(Value::Null);
     }
@@ -115,10 +116,10 @@ pub(crate) fn oracle_value(v: &SqlValue) -> Result<Value> {
         }
         OracleType::IntervalDS(_, _) | OracleType::IntervalYM(_) => Value::Interval(v.to_string()),
         OracleType::Raw(_) | OracleType::LongRaw => Value::Bytes(v.get::<Vec<u8>>().map_err(map_err)?),
-        OracleType::CLOB => read_lob(v.get::<Clob>().map_err(map_err)?, false)?,
+        OracleType::CLOB => read_lob(v.get::<Clob>().map_err(map_err)?, false, lob_limit)?,
         // NCLOB 在 oracle crate 中是独立类型，v.get::<Clob>() 会报类型转换错误
-        OracleType::NCLOB => read_lob(v.get::<Nclob>().map_err(map_err)?, false)?,
-        OracleType::BLOB => read_lob(v.get::<Blob>().map_err(map_err)?, true)?,
+        OracleType::NCLOB => read_lob(v.get::<Nclob>().map_err(map_err)?, false, lob_limit)?,
+        OracleType::BLOB => read_lob(v.get::<Blob>().map_err(map_err)?, true, lob_limit)?,
         OracleType::Boolean => Value::Bool(v.get().map_err(map_err)?),
         other => Value::Other { type_name: other.to_string(), text: v.to_string() },
     };
```

- [ ] **Step 10: 修改 `crates/dblens-driver-postgres/src/conn.rs`**

```diff
--- a/crates/dblens-driver-postgres/src/conn.rs
+++ b/crates/dblens-driver-postgres/src/conn.rs
@@ -17,6 +17,7 @@ pub struct PgConnection {
     pub(crate) in_tx: bool,
     /// 加密连接时的 TLS 连接器；取消请求也要用它建立连接（服务器可能只接受加密连接）。
     pub(crate) tls: Option<tokio_postgres_rustls::MakeRustlsConnect>,
+    pub(crate) lob_limit: usize,
 }
 
 impl PgConnection {
@@ -57,10 +58,14 @@ impl Connection for PgConnection {
             let columns =
                 stmt.columns().iter().map(|c| ColumnMeta::new(c.name(), c.type_().name())).collect();
             let rows = self.client.query_raw(&stmt, args()).await.map_err(map_err)?;
-            Ok(Box::new(PgExec::rows(&mut self.notices, columns, rows)))
+            Ok(Box::new(PgExec::rows(&mut self.notices, columns, rows, self.lob_limit)))
         }
     }
 
+    fn set_lob_limit(&mut self, bytes: usize) {
+        self.lob_limit = bytes;
+    }
+
     fn cancel_handle(&self) -> CancelHandle {
         let token = self.client.cancel_token();
         let tls = self.tls.clone();
```

- [ ] **Step 11: 修改 `crates/dblens-driver-postgres/src/exec.rs`**

```diff
--- a/crates/dblens-driver-postgres/src/exec.rs
+++ b/crates/dblens-driver-postgres/src/exec.rs
@@ -23,24 +23,26 @@ pub(crate) struct PgExec<'a> {
     notices: &'a mut UnboundedReceiver<String>,
     rows: Option<Pin<Box<RowStream>>>,
     state: State,
+    lob_limit: usize,
 }
 
 impl<'a> PgExec<'a> {
     pub(crate) fn affected(notices: &'a mut UnboundedReceiver<String>, n: u64) -> Self {
-        Self { notices, rows: None, state: State::Affected(n) }
+        Self { notices, rows: None, state: State::Affected(n), lob_limit: DEFAULT_LOB_PREVIEW_BYTES }
     }
 
     pub(crate) fn rows(
         notices: &'a mut UnboundedReceiver<String>,
         columns: Vec<ColumnMeta>,
         rows: RowStream,
+        lob_limit: usize,
     ) -> Self {
-        Self { notices, rows: Some(Box::pin(rows)), state: State::Start(columns) }
+        Self { notices, rows: Some(Box::pin(rows)), state: State::Start(columns), lob_limit }
     }
 }
 
-fn convert_row(row: &tokio_postgres::Row) -> Result<Row> {
-    (0..row.len()).map(|i| pg_value(row, i).map(|v| v.cap_large(DEFAULT_LOB_PREVIEW_BYTES))).collect()
+fn convert_row(row: &tokio_postgres::Row, lob_limit: usize) -> Result<Row> {
+    (0..row.len()).map(|i| pg_value(row, i).map(|v| v.cap_large(lob_limit))).collect()
 }
 
 #[async_trait]
@@ -61,7 +63,7 @@ impl ExecStream for PgExec<'_> {
                 let mut ended = false;
                 while rows.len() < max_rows.max(1) {
                     match stream.next().await {
-                        Some(row) => rows.push(convert_row(&row.map_err(map_err)?)?),
+                        Some(row) => rows.push(convert_row(&row.map_err(map_err)?, self.lob_limit)?),
                         None => {
                             ended = true;
                             break;
```

- [ ] **Step 12: 修改 `crates/dblens-driver-postgres/src/lib.rs`**

```diff
--- a/crates/dblens-driver-postgres/src/lib.rs
+++ b/crates/dblens-driver-postgres/src/lib.rs
@@ -84,6 +84,14 @@ impl Driver for PgDriver {
                 (client, spawn_connection(connection))
             }
         };
-        Ok(Box::new(PgConnection { client, notices, conn_task, autocommit: true, in_tx: false, tls }))
+        Ok(Box::new(PgConnection {
+            client,
+            notices,
+            conn_task,
+            autocommit: true,
+            in_tx: false,
+            tls,
+            lob_limit: dblens_core::DEFAULT_LOB_PREVIEW_BYTES,
+        }))
     }
 }
```

- [ ] **Step 13: 修改 `crates/dblens-driver/src/blocking.rs`**

```diff
--- a/crates/dblens-driver/src/blocking.rs
+++ b/crates/dblens-driver/src/blocking.rs
@@ -19,6 +19,9 @@ pub trait BlockingExec {
 /// 同步的连接。所有方法都只在该连接的专用线程上调用。
 pub trait BlockingConnection: Send + 'static {
     fn execute<'a>(&'a mut self, sql: &'a str, params: &'a [Value]) -> Result<Box<dyn BlockingExec + 'a>>;
+    /// 大字段（长文本、二进制、LOB）每个值最多读取的字节数，超出的部分截断为预览（`Value::Lob`）。
+    /// 新连接为 `DEFAULT_LOB_PREVIEW_BYTES`；导出时放宽（M1c3）。只影响之后执行的语句。
+    fn set_lob_limit(&mut self, bytes: usize);
     fn set_autocommit(&mut self, on: bool) -> Result<()>;
     fn commit(&mut self) -> Result<()>;
     fn rollback(&mut self) -> Result<()>;
@@ -41,6 +44,10 @@ enum Cmd {
     },
     /// 异步侧提前丢弃了执行流。
     EndExec,
+    /// 不需要回复：只改变之后执行的语句。
+    SetLobLimit {
+        bytes: usize,
+    },
     SetAutocommit {
         on: bool,
         reply: Reply<()>,
@@ -81,7 +88,7 @@ impl Cmd {
             Cmd::ServerInfo { reply } => {
                 let _ = reply.send(Err(busy()));
             }
-            Cmd::EndExec => {}
+            Cmd::EndExec | Cmd::SetLobLimit { .. } => {}
         }
     }
 }
@@ -117,6 +124,7 @@ fn run_worker<C: BlockingConnection>(mut conn: C, rx: mpsc::Receiver<Cmd>) {
                 let _ = reply.send(Ok(None));
             }
             Cmd::EndExec => {}
+            Cmd::SetLobLimit { bytes } => conn.set_lob_limit(bytes),
             Cmd::SetAutocommit { on, reply } => {
                 let _ = reply.send(conn.set_autocommit(on));
             }
@@ -196,6 +204,12 @@ impl Connection for AsyncConnection {
         self.cancel.clone()
     }
 
+    fn set_lob_limit(&mut self, bytes: usize) {
+        // 与 request 一样先释放被丢弃的执行流；发送失败说明工作线程已退出，下一条命令会报告
+        let _ = self.tx.send(Cmd::EndExec);
+        let _ = self.tx.send(Cmd::SetLobLimit { bytes });
+    }
+
     async fn set_autocommit(&mut self, on: bool) -> Result<()> {
         self.request(|reply| Cmd::SetAutocommit { on, reply }).await
     }
```

- [ ] **Step 14: 修改 `crates/dblens-driver/src/conformance.rs`**

```diff
--- a/crates/dblens-driver/src/conformance.rs
+++ b/crates/dblens-driver/src/conformance.rs
@@ -202,6 +202,19 @@ pub async fn large_text_becomes_lob_preview(f: &Fixture) {
     }
 }
 
+pub async fn lob_limit_reads_full_value(f: &Fixture) {
+    let mut c = connect(f).await;
+    c.set_lob_limit(1024 * 1024);
+    match single_value(&run(c.as_mut(), f.large_text, &[]).await.expect("执行失败")) {
+        Value::String(s) => assert_eq!(s.len(), 100_000),
+        other => panic!("期望完整的文本，实际：{other:?}"),
+    }
+    // 改回默认后又是预览
+    c.set_lob_limit(dblens_core::DEFAULT_LOB_PREVIEW_BYTES);
+    let v = single_value(&run(c.as_mut(), f.large_text, &[]).await.expect("执行失败"));
+    assert!(matches!(v, Value::Lob(_)), "{v:?}");
+}
+
 pub async fn type_round_trip(f: &Fixture) {
     let mut c = connect(f).await;
     for (sql, expected) in &f.type_cases {
@@ -266,6 +279,12 @@ macro_rules! conformance_tests {
                 }
             }
             #[tokio::test]
+            async fn lob_limit_reads_full_value() {
+                if let Some(f) = $crate::conformance::load($env, $make) {
+                    $crate::conformance::lob_limit_reads_full_value(&f).await;
+                }
+            }
+            #[tokio::test]
             async fn type_round_trip() {
                 if let Some(f) = $crate::conformance::load($env, $make) {
                     $crate::conformance::type_round_trip(&f).await;
```

- [ ] **Step 15: 修改 `crates/dblens-driver/src/traits.rs`**

```diff
--- a/crates/dblens-driver/src/traits.rs
+++ b/crates/dblens-driver/src/traits.rs
@@ -19,6 +19,9 @@ pub trait Connection: Send {
     -> Result<Box<dyn ExecStream + 'a>>;
     /// 返回可以从其他任务调用的取消句柄。
     fn cancel_handle(&self) -> CancelHandle;
+    /// 大字段（长文本、二进制、LOB）每个值最多读取的字节数，超出的部分截断为预览（`Value::Lob`）。
+    /// 新连接为 `DEFAULT_LOB_PREVIEW_BYTES`；导出时放宽（M1c3）。只影响之后执行的语句。
+    fn set_lob_limit(&mut self, bytes: usize);
     /// 切换自动提交。从手动切回自动时，未提交的事务会被提交（与 JDBC 行为一致）。
     async fn set_autocommit(&mut self, on: bool) -> Result<()>;
     async fn commit(&mut self) -> Result<()>;
```

- [ ] **Step 16: 修改 `crates/dblens-service/src/fake.rs`**

```diff
--- a/crates/dblens-service/src/fake.rs
+++ b/crates/dblens-service/src/fake.rs
@@ -104,6 +104,10 @@ impl ExecStream for FakeStream {
 
 #[async_trait]
 impl Connection for FakeConn {
+    fn set_lob_limit(&mut self, bytes: usize) {
+        self.log.lock().unwrap().push(format!("lob_limit {bytes}"));
+    }
+
     async fn execute<'a>(&'a mut self, sql: &'a str, _: &'a [Value]) -> Result<Box<dyn ExecStream + 'a>> {
         let sql = sql.trim();
         self.log.lock().unwrap().push(format!("execute {sql}"));
```

- [ ] **Step 17: 检查**

```bash
export CARGO_INCREMENTAL=0
cargo fmt --all
cargo clippy --config "source.mirror.registry='sparse+https://rsproxy.cn/index/'" --workspace --all-targets -- -D warnings
DBLENS_LANG=en cargo test --config "source.mirror.registry='sparse+https://rsproxy.cn/index/'" --workspace
```

实库测试：在上面的 `cargo test` 前以内联环境变量给出 `DBLENS_TEST_MYSQL_URL`、`DBLENS_TEST_MARIADB_URL`、`DBLENS_TEST_PG_URL`、`DBLENS_TEST_ORACLE_URL`、`DBLENS_TEST_DM_URL`（密码不写进任何文件）。

Expected: 全部测试通过；4 个驱动 crate 的 `lob_limit_reads_full_value` 在 5 个实库上通过（MySQL、MariaDB 共用 MySQL 驱动）。

- [ ] **Step 18: 提交**

```bash
git add crates/dblens-driver-dameng/src/conn.rs crates/dblens-driver-dameng/src/types.rs crates/dblens-driver-mysql/src/conn.rs crates/dblens-driver-mysql/src/exec.rs crates/dblens-driver-mysql/src/lib.rs crates/dblens-driver-oracle/src/conn.rs crates/dblens-driver-oracle/src/value.rs crates/dblens-driver-postgres/src/conn.rs crates/dblens-driver-postgres/src/exec.rs crates/dblens-driver-postgres/src/lib.rs crates/dblens-driver/src/blocking.rs crates/dblens-driver/src/conformance.rs crates/dblens-driver/src/traits.rs crates/dblens-driver/tests/blocking.rs crates/dblens-driver/tests/fake_driver.rs crates/dblens-service/src/fake.rs
git commit -m "feat(driver): 连接可设置大字段的读取上限（导出时读完整内容）" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 3: 方言：运维查询模板

运维查询放在 `dblens-dialect/sql/ops/<方言>.sql`，按 `-- [key]` 分段，段首注释写中英文名称和分类（`session` / `lock` / `storage` / `sql`），解析时保持文件中的顺序。
- 每种数据库 6 项：`sessions`、`running`、`locks`、`storage`、`largest_tables`、`slow_sql`；会话和锁的查询带 `kill_sql` 列。
- MariaDB 没有 sys 库，`mariadb.sql` 只覆盖 `locks`（原位替换）；其余兼容库用所属家族的模板。
- Oracle、达梦的列别名加双引号，保持小写列名。

**Files:**
- Create: `crates/dblens-dialect/sql/ops/dameng.sql`
- Create: `crates/dblens-dialect/sql/ops/mariadb.sql`
- Create: `crates/dblens-dialect/sql/ops/mysql.sql`
- Create: `crates/dblens-dialect/sql/ops/oracle.sql`
- Create: `crates/dblens-dialect/sql/ops/postgres.sql`
- Modify: `crates/dblens-dialect/src/lib.rs`
- Modify: `crates/dblens-dialect/src/meta/mod.rs`
- Modify: `crates/dblens-dialect/src/meta/template.rs`
- Create: `crates/dblens-dialect/src/ops.rs`

**Interfaces:**
- Produces（`dblens_dialect`）：
  - `OpsCategory::{Session, Lock, Storage, Sql}`（serde snake_case）；
  - `OpsQuery { key, name_zh, name_en, category, sql }`；
  - `Dialect::ops_queries(&self) -> &'static [OpsQuery]`；
  - `meta::template::ALL_VARIANTS` 改为 `pub(crate)`。

- [ ] **Step 1: 新建 `crates/dblens-dialect/sql/ops/dameng.sql`**

```
-- DM (Dameng) ops queries (M1c3). They read V$ dynamic views and DBA_ views: users without the DBA role
-- (or equivalent grants) get the database's own error. Aliases are quoted to keep lower-case column names.

-- [sessions]
-- name.zh-CN: 会话列表
-- name.en: Sessions
-- category: session
SELECT s.SESS_ID AS "sess_id", s.USER_NAME AS "username", s.CLNT_IP AS "host", s.APPNAME AS "program",
       s.STATE AS "state", DATEDIFF(SS, s.LAST_RECV_TIME, SYSDATE) AS "seconds",
       SUBSTR(s.SQL_TEXT, 1, 200) AS "current_sql",
       'SP_CLOSE_SESSION(' || s.SESS_ID || ')' AS "kill_sql"
FROM V$SESSIONS s
ORDER BY 6 DESC

-- [running]
-- name.zh-CN: 正在执行的语句
-- name.en: Running Statements
-- category: session
SELECT s.SESS_ID AS "sess_id", s.USER_NAME AS "username", s.CLNT_IP AS "host",
       DATEDIFF(SS, s.LAST_RECV_TIME, SYSDATE) AS "seconds", SUBSTR(s.SQL_TEXT, 1, 500) AS "current_sql",
       'SP_CLOSE_SESSION(' || s.SESS_ID || ')' AS "kill_sql"
FROM V$SESSIONS s
WHERE s.STATE = 'ACTIVE' AND s.SESS_ID <> SESSID()
ORDER BY 4 DESC

-- [locks]
-- name.zh-CN: 锁等待
-- name.en: Lock Waits
-- category: lock
SELECT ws.SESS_ID AS "waiting_sess", SUBSTR(ws.SQL_TEXT, 1, 200) AS "waiting_sql",
       bs.SESS_ID AS "blocking_sess", SUBSTR(bs.SQL_TEXT, 1, 200) AS "blocking_sql",
       w.WAIT_TIME AS "wait_ms",
       'SP_CLOSE_SESSION(' || bs.SESS_ID || ')' AS "kill_sql"
FROM V$TRXWAIT w
JOIN V$SESSIONS ws ON ws.TRX_ID = w.ID
JOIN V$SESSIONS bs ON bs.TRX_ID = w.WAIT_FOR_ID
ORDER BY w.WAIT_TIME DESC

-- [storage]
-- name.zh-CN: 表空间使用
-- name.en: Tablespace Usage
-- category: storage
SELECT d.TABLESPACE_NAME AS "tablespace",
       ROUND(SUM(d.BYTES) / 1048576, 2) AS "total_mb",
       ROUND((SUM(d.BYTES) - NVL(MAX(f.FREE_BYTES), 0)) / 1048576, 2) AS "used_mb",
       ROUND((SUM(d.BYTES) - NVL(MAX(f.FREE_BYTES), 0)) * 100 / SUM(d.BYTES), 2) AS "used_pct",
       ROUND(SUM(CASE WHEN d.AUTOEXTENSIBLE = 'YES' THEN d.MAXBYTES ELSE d.BYTES END) / 1048576, 2) AS "max_mb"
FROM DBA_DATA_FILES d
LEFT JOIN (SELECT TABLESPACE_NAME, SUM(BYTES) AS FREE_BYTES FROM DBA_FREE_SPACE GROUP BY TABLESPACE_NAME) f
       ON f.TABLESPACE_NAME = d.TABLESPACE_NAME
GROUP BY d.TABLESPACE_NAME
ORDER BY 4 DESC

-- [largest_tables]
-- name.zh-CN: 占用最大的表
-- name.en: Largest Tables
-- category: storage
SELECT x.OWNER AS "owner", x.TABLE_NAME AS "table_name",
       ROUND(SUM(CASE WHEN x.KIND = 'T' THEN x.BYTES ELSE 0 END) / 1048576, 2) AS "data_mb",
       ROUND(SUM(CASE WHEN x.KIND = 'I' THEN x.BYTES ELSE 0 END) / 1048576, 2) AS "index_mb",
       ROUND(SUM(x.BYTES) / 1048576, 2) AS "total_mb"
FROM (
  SELECT s.OWNER, s.SEGMENT_NAME AS TABLE_NAME, 'T' AS KIND, s.BYTES
  FROM DBA_SEGMENTS s WHERE s.SEGMENT_TYPE LIKE 'TABLE%'
  UNION ALL
  SELECT i.TABLE_OWNER, i.TABLE_NAME, 'I', s.BYTES
  FROM DBA_SEGMENTS s JOIN DBA_INDEXES i ON i.OWNER = s.OWNER AND i.INDEX_NAME = s.SEGMENT_NAME
  WHERE s.SEGMENT_TYPE LIKE 'INDEX%'
) x
GROUP BY x.OWNER, x.TABLE_NAME
ORDER BY SUM(x.BYTES) DESC
LIMIT 20

-- [slow_sql]
-- name.zh-CN: 慢 SQL
-- name.en: Slow SQL
-- category: sql
SELECT l.SESS_ID AS "sess_id", SUBSTR(l.SQL_TEXT, 1, 300) AS "sql_text", l.EXEC_TIME AS "elapsed_ms",
       l.FINISH_TIME AS "finished_at"
FROM V$LONG_EXEC_SQLS l
ORDER BY l.EXEC_TIME DESC
LIMIT 20
```

- [ ] **Step 2: 新建 `crates/dblens-dialect/sql/ops/mariadb.sql`**

```
-- MariaDB overrides of the MySQL ops queries (M1c3). MariaDB has no sys schema; lock waits come from
-- the InnoDB tables in information_schema.

-- [locks]
-- name.zh-CN: 锁等待
-- name.en: Lock Waits
-- category: lock
SELECT r.trx_mysql_thread_id AS waiting_pid, LEFT(r.trx_query, 200) AS waiting_sql,
       b.trx_mysql_thread_id AS blocking_pid, LEFT(b.trx_query, 200) AS blocking_sql,
       l.lock_table AS locked_table,
       TIMESTAMPDIFF(SECOND, r.trx_wait_started, NOW()) AS wait_seconds,
       CONCAT('KILL ', b.trx_mysql_thread_id) AS kill_sql
FROM information_schema.INNODB_LOCK_WAITS w
JOIN information_schema.INNODB_TRX r ON r.trx_id = w.requesting_trx_id
JOIN information_schema.INNODB_TRX b ON b.trx_id = w.blocking_trx_id
LEFT JOIN information_schema.INNODB_LOCKS l ON l.lock_id = w.requested_lock_id
ORDER BY wait_seconds DESC
```

- [ ] **Step 3: 新建 `crates/dblens-dialect/sql/ops/mysql.sql`**

```
-- MySQL ops queries (M1c3). Also used by TiDB, OceanBase and PolarDB, which may lack some of the views:
-- the database's own error is shown in that case. Timers in performance_schema are picoseconds.

-- [sessions]
-- name.zh-CN: 会话列表
-- name.en: Sessions
-- category: session
SELECT p.ID AS id, p.USER AS user, p.HOST AS host, p.DB AS db, p.COMMAND AS command,
       p.TIME AS seconds, p.STATE AS state, LEFT(p.INFO, 200) AS current_sql,
       CONCAT('KILL ', p.ID) AS kill_sql
FROM information_schema.PROCESSLIST p
ORDER BY p.TIME DESC

-- [running]
-- name.zh-CN: 正在执行的语句
-- name.en: Running Statements
-- category: session
SELECT p.ID AS id, p.USER AS user, p.HOST AS host, p.DB AS db, p.TIME AS seconds, p.STATE AS state,
       LEFT(p.INFO, 500) AS current_sql, CONCAT('KILL ', p.ID) AS kill_sql
FROM information_schema.PROCESSLIST p
WHERE p.COMMAND NOT IN ('Sleep', 'Daemon', 'Binlog Dump') AND p.ID <> CONNECTION_ID()
ORDER BY p.TIME DESC

-- [locks]
-- name.zh-CN: 锁等待
-- name.en: Lock Waits
-- category: lock
SELECT w.waiting_pid, LEFT(w.waiting_query, 200) AS waiting_sql,
       w.blocking_pid, LEFT(w.blocking_query, 200) AS blocking_sql,
       w.locked_table, w.wait_age_secs AS wait_seconds,
       CONCAT('KILL ', w.blocking_pid) AS kill_sql
FROM sys.innodb_lock_waits w
ORDER BY w.wait_age_secs DESC

-- [storage]
-- name.zh-CN: 各库大小
-- name.en: Database Sizes
-- category: storage
SELECT t.TABLE_SCHEMA AS db, COUNT(*) AS tables,
       ROUND(SUM(t.DATA_LENGTH) / 1048576, 2) AS data_mb,
       ROUND(SUM(t.INDEX_LENGTH) / 1048576, 2) AS index_mb,
       ROUND(SUM(t.DATA_LENGTH + t.INDEX_LENGTH) / 1048576, 2) AS total_mb
FROM information_schema.TABLES t
WHERE t.TABLE_TYPE = 'BASE TABLE'
GROUP BY t.TABLE_SCHEMA
ORDER BY total_mb DESC

-- [largest_tables]
-- name.zh-CN: 占用最大的表
-- name.en: Largest Tables
-- category: storage
SELECT t.TABLE_SCHEMA AS db, t.TABLE_NAME AS table_name, t.TABLE_ROWS AS est_rows,
       ROUND(t.DATA_LENGTH / 1048576, 2) AS data_mb,
       ROUND(t.INDEX_LENGTH / 1048576, 2) AS index_mb,
       ROUND((t.DATA_LENGTH + t.INDEX_LENGTH) / 1048576, 2) AS total_mb
FROM information_schema.TABLES t
WHERE t.TABLE_TYPE = 'BASE TABLE'
ORDER BY t.DATA_LENGTH + t.INDEX_LENGTH DESC
LIMIT 20

-- [slow_sql]
-- name.zh-CN: 慢 SQL
-- name.en: Slow SQL
-- category: sql
SELECT d.SCHEMA_NAME AS db, LEFT(d.DIGEST_TEXT, 300) AS sql_text, d.COUNT_STAR AS executions,
       ROUND(d.AVG_TIMER_WAIT / 1000000000, 2) AS avg_ms,
       ROUND(d.MAX_TIMER_WAIT / 1000000000, 2) AS max_ms,
       ROUND(d.SUM_TIMER_WAIT / 1000000000, 2) AS total_ms,
       d.SUM_ROWS_EXAMINED AS rows_examined, d.LAST_SEEN AS last_seen
FROM performance_schema.events_statements_summary_by_digest d
WHERE d.DIGEST_TEXT IS NOT NULL
ORDER BY d.AVG_TIMER_WAIT DESC
LIMIT 20
```

- [ ] **Step 4: 新建 `crates/dblens-dialect/sql/ops/oracle.sql`**

```
-- Oracle ops queries (M1c3). They read V$ and DBA_ views: users without SELECT_CATALOG_ROLE (or
-- equivalent grants) get the database's own error. Aliases are quoted to keep lower-case column names.

-- [sessions]
-- name.zh-CN: 会话列表
-- name.en: Sessions
-- category: session
SELECT s.SID AS "sid", s.SERIAL# AS "serial", s.USERNAME AS "username", s.MACHINE AS "host",
       s.PROGRAM AS "program", s.STATUS AS "status", s.LAST_CALL_ET AS "seconds",
       SUBSTR(q.SQL_TEXT, 1, 200) AS "current_sql",
       'ALTER SYSTEM KILL SESSION ''' || s.SID || ',' || s.SERIAL# || ''' IMMEDIATE' AS "kill_sql"
FROM V$SESSION s
LEFT JOIN V$SQL q ON q.SQL_ID = s.SQL_ID AND q.CHILD_NUMBER = s.SQL_CHILD_NUMBER
WHERE s.TYPE = 'USER'
ORDER BY s.LAST_CALL_ET DESC

-- [running]
-- name.zh-CN: 正在执行的语句
-- name.en: Running Statements
-- category: session
SELECT s.SID AS "sid", s.SERIAL# AS "serial", s.USERNAME AS "username", s.MACHINE AS "host",
       s.LAST_CALL_ET AS "seconds", s.EVENT AS "wait_event", SUBSTR(q.SQL_TEXT, 1, 500) AS "current_sql",
       'ALTER SYSTEM KILL SESSION ''' || s.SID || ',' || s.SERIAL# || ''' IMMEDIATE' AS "kill_sql"
FROM V$SESSION s
LEFT JOIN V$SQL q ON q.SQL_ID = s.SQL_ID AND q.CHILD_NUMBER = s.SQL_CHILD_NUMBER
WHERE s.TYPE = 'USER' AND s.STATUS = 'ACTIVE' AND s.SID <> SYS_CONTEXT('USERENV', 'SID')
ORDER BY s.LAST_CALL_ET DESC

-- [locks]
-- name.zh-CN: 锁等待
-- name.en: Lock Waits
-- category: lock
SELECT w.SID AS "waiting_sid", SUBSTR(wq.SQL_TEXT, 1, 200) AS "waiting_sql",
       b.SID AS "blocking_sid", b.SERIAL# AS "blocking_serial", SUBSTR(bq.SQL_TEXT, 1, 200) AS "blocking_sql",
       o.OWNER || '.' || o.OBJECT_NAME AS "locked_object", w.SECONDS_IN_WAIT AS "wait_seconds",
       'ALTER SYSTEM KILL SESSION ''' || b.SID || ',' || b.SERIAL# || ''' IMMEDIATE' AS "kill_sql"
FROM V$SESSION w
JOIN V$SESSION b ON b.SID = w.BLOCKING_SESSION
LEFT JOIN V$SQL wq ON wq.SQL_ID = w.SQL_ID AND wq.CHILD_NUMBER = w.SQL_CHILD_NUMBER
LEFT JOIN V$SQL bq ON bq.SQL_ID = b.PREV_SQL_ID AND bq.CHILD_NUMBER = b.PREV_CHILD_NUMBER
LEFT JOIN ALL_OBJECTS o ON o.OBJECT_ID = w.ROW_WAIT_OBJ#
WHERE w.BLOCKING_SESSION IS NOT NULL
ORDER BY w.SECONDS_IN_WAIT DESC

-- [storage]
-- name.zh-CN: 表空间使用
-- name.en: Tablespace Usage
-- category: storage
SELECT d.TABLESPACE_NAME AS "tablespace",
       ROUND(SUM(d.BYTES) / 1048576, 2) AS "total_mb",
       ROUND((SUM(d.BYTES) - NVL(MAX(f.FREE_BYTES), 0)) / 1048576, 2) AS "used_mb",
       ROUND((SUM(d.BYTES) - NVL(MAX(f.FREE_BYTES), 0)) * 100 / SUM(d.BYTES), 2) AS "used_pct",
       ROUND(SUM(CASE WHEN d.AUTOEXTENSIBLE = 'YES' THEN d.MAXBYTES ELSE d.BYTES END) / 1048576, 2) AS "max_mb"
FROM DBA_DATA_FILES d
LEFT JOIN (SELECT TABLESPACE_NAME, SUM(BYTES) AS FREE_BYTES FROM DBA_FREE_SPACE GROUP BY TABLESPACE_NAME) f
       ON f.TABLESPACE_NAME = d.TABLESPACE_NAME
GROUP BY d.TABLESPACE_NAME
ORDER BY 4 DESC

-- [largest_tables]
-- name.zh-CN: 占用最大的表
-- name.en: Largest Tables
-- category: storage
SELECT * FROM (
  SELECT x.OWNER AS "owner", x.TABLE_NAME AS "table_name",
         ROUND(SUM(CASE WHEN x.KIND = 'T' THEN x.BYTES ELSE 0 END) / 1048576, 2) AS "data_mb",
         ROUND(SUM(CASE WHEN x.KIND = 'I' THEN x.BYTES ELSE 0 END) / 1048576, 2) AS "index_mb",
         ROUND(SUM(x.BYTES) / 1048576, 2) AS "total_mb"
  FROM (
    SELECT s.OWNER, s.SEGMENT_NAME AS TABLE_NAME, 'T' AS KIND, s.BYTES
    FROM DBA_SEGMENTS s WHERE s.SEGMENT_TYPE LIKE 'TABLE%'
    UNION ALL
    SELECT i.TABLE_OWNER, i.TABLE_NAME, 'I', s.BYTES
    FROM DBA_SEGMENTS s JOIN DBA_INDEXES i ON i.OWNER = s.OWNER AND i.INDEX_NAME = s.SEGMENT_NAME
    WHERE s.SEGMENT_TYPE LIKE 'INDEX%'
  ) x
  GROUP BY x.OWNER, x.TABLE_NAME
  ORDER BY SUM(x.BYTES) DESC
) WHERE ROWNUM <= 20

-- [slow_sql]
-- name.zh-CN: 慢 SQL
-- name.en: Slow SQL
-- category: sql
SELECT * FROM (
  SELECT q.SQL_ID AS "sql_id", q.PARSING_SCHEMA_NAME AS "schema", SUBSTR(q.SQL_TEXT, 1, 300) AS "sql_text",
         q.EXECUTIONS AS "executions", ROUND(q.ELAPSED_TIME / 1000 / q.EXECUTIONS, 2) AS "avg_ms",
         ROUND(q.ELAPSED_TIME / 1000, 2) AS "total_ms", q.BUFFER_GETS AS "buffer_gets",
         q.LAST_ACTIVE_TIME AS "last_seen"
  FROM V$SQL q
  WHERE q.EXECUTIONS > 0
  ORDER BY q.ELAPSED_TIME / q.EXECUTIONS DESC
) WHERE ROWNUM <= 20
```

- [ ] **Step 5: 新建 `crates/dblens-dialect/sql/ops/postgres.sql`**

```
-- PostgreSQL ops queries (M1c3). Also used by KingbaseES, openGauss, GaussDB and HighGo, which may name
-- some columns differently: the database's own error is shown in that case.
-- slow_sql needs the pg_stat_statements extension (PostgreSQL 13+ column names).

-- [sessions]
-- name.zh-CN: 会话列表
-- name.en: Sessions
-- category: session
SELECT a.pid, a.usename AS user, a.client_addr::text AS host, a.datname AS db,
       a.application_name AS application, a.state,
       ROUND(EXTRACT(EPOCH FROM now() - a.query_start)) AS seconds,
       LEFT(a.query, 200) AS current_sql,
       'SELECT pg_terminate_backend(' || a.pid || ')' AS kill_sql
FROM pg_stat_activity a
WHERE a.backend_type = 'client backend'
ORDER BY a.query_start NULLS LAST

-- [running]
-- name.zh-CN: 正在执行的语句
-- name.en: Running Statements
-- category: session
SELECT a.pid, a.usename AS user, a.client_addr::text AS host, a.datname AS db,
       ROUND(EXTRACT(EPOCH FROM now() - a.query_start)) AS seconds,
       a.wait_event_type, a.wait_event, LEFT(a.query, 500) AS current_sql,
       'SELECT pg_terminate_backend(' || a.pid || ')' AS kill_sql
FROM pg_stat_activity a
WHERE a.backend_type = 'client backend' AND a.state = 'active' AND a.pid <> pg_backend_pid()
ORDER BY a.query_start

-- [locks]
-- name.zh-CN: 锁等待
-- name.en: Lock Waits
-- category: lock
SELECT w.pid AS waiting_pid, LEFT(w.query, 200) AS waiting_sql,
       b.pid AS blocking_pid, LEFT(b.query, 200) AS blocking_sql,
       ROUND(EXTRACT(EPOCH FROM now() - w.query_start)) AS wait_seconds,
       'SELECT pg_terminate_backend(' || b.pid || ')' AS kill_sql
FROM pg_stat_activity w
CROSS JOIN LATERAL unnest(pg_blocking_pids(w.pid)) AS bp(pid)
JOIN pg_stat_activity b ON b.pid = bp.pid
ORDER BY wait_seconds DESC

-- [storage]
-- name.zh-CN: 各库大小
-- name.en: Database Sizes
-- category: storage
SELECT d.datname AS db, pg_size_pretty(pg_database_size(d.datname)) AS size,
       ROUND(pg_database_size(d.datname) / 1048576.0, 2) AS size_mb
FROM pg_database d
WHERE NOT d.datistemplate AND has_database_privilege(d.datname, 'CONNECT')
ORDER BY pg_database_size(d.datname) DESC

-- [largest_tables]
-- name.zh-CN: 占用最大的表
-- name.en: Largest Tables
-- category: storage
SELECT n.nspname AS schema, c.relname AS table_name, c.reltuples::bigint AS est_rows,
       ROUND(pg_table_size(c.oid) / 1048576.0, 2) AS data_mb,
       ROUND(pg_indexes_size(c.oid) / 1048576.0, 2) AS index_mb,
       ROUND(pg_total_relation_size(c.oid) / 1048576.0, 2) AS total_mb
FROM pg_class c
JOIN pg_namespace n ON n.oid = c.relnamespace
WHERE c.relkind IN ('r', 'p', 'm')
  AND n.nspname NOT IN ('pg_catalog', 'information_schema') AND n.nspname NOT LIKE 'pg_toast%'
ORDER BY pg_total_relation_size(c.oid) DESC
LIMIT 20

-- [slow_sql]
-- name.zh-CN: 慢 SQL
-- name.en: Slow SQL
-- category: sql
SELECT LEFT(s.query, 300) AS sql_text, s.calls AS executions,
       ROUND(s.mean_exec_time::numeric, 2) AS avg_ms,
       ROUND(s.max_exec_time::numeric, 2) AS max_ms,
       ROUND(s.total_exec_time::numeric, 2) AS total_ms,
       s.rows
FROM pg_stat_statements s
ORDER BY s.mean_exec_time DESC
LIMIT 20
```

- [ ] **Step 6: 修改 `crates/dblens-dialect/src/lib.rs`**

```diff
--- a/crates/dblens-dialect/src/lib.rs
+++ b/crates/dblens-dialect/src/lib.rs
@@ -6,6 +6,7 @@ mod dml;
 pub mod ident;
 pub mod literal;
 pub mod meta;
+mod ops;
 mod session;
 mod split;
 pub mod types;
@@ -18,6 +19,7 @@ use dblens_driver::{DriverId, ServerInfo};
 pub use browse::{Direction, Relation, relations};
 pub use ddl::{DdlKind, ddl_text, normalize_ddl};
 pub use dml::DmlTemplate;
+pub use ops::{OpsCategory, OpsQuery};
 pub use session::may_change_schema;
 pub use split::{SqlStatement, split_statements};
 pub use variant::{Family, Variant};
```

- [ ] **Step 7: 修改 `crates/dblens-dialect/src/meta/mod.rs`**

```diff
--- a/crates/dblens-dialect/src/meta/mod.rs
+++ b/crates/dblens-dialect/src/meta/mod.rs
@@ -4,7 +4,7 @@
 
 mod assemble;
 mod rows;
-mod template;
+pub(crate) mod template;
 
 use serde::{Deserialize, Serialize};
 
```

- [ ] **Step 8: 修改 `crates/dblens-dialect/src/meta/template.rs`**

```diff
--- a/crates/dblens-dialect/src/meta/template.rs
+++ b/crates/dblens-dialect/src/meta/template.rs
@@ -20,7 +20,7 @@ const POSTGRES: &str = include_str!("../../sql/postgres.sql");
 const ORACLE: &str = include_str!("../../sql/oracle.sql");
 const DAMENG: &str = include_str!("../../sql/dameng.sql");
 
-const ALL_VARIANTS: [Variant; 12] = [
+pub(crate) const ALL_VARIANTS: [Variant; 12] = [
     Variant::MySql,
     Variant::MariaDb,
     Variant::TiDb,
```

- [ ] **Step 9: 新建 `crates/dblens-dialect/src/ops.rs`**

````rust
//! 运维查询（M1c3 设计文档第 5 节）：每个家族一个 `sql/ops/<家族>.sql`，兼容库可以有只含差异 key 的覆盖文件。
//! 文件按 `-- [key]` 行分段，段首的注释行写名称和分类：
//!
//! ```sql
//! -- [sessions]
//! -- name.zh-CN: 会话列表
//! -- name.en: Sessions
//! -- category: session
//! SELECT …
//! ```
//!
//! 查询没有参数；会话、锁的查询带 `kill_sql` 列（结束会话的语句，只生成不执行）。

use std::sync::OnceLock;

use serde::Serialize;

use crate::{Dialect, Family, Variant};

const MYSQL: &str = include_str!("../sql/ops/mysql.sql");
const MARIADB: &str = include_str!("../sql/ops/mariadb.sql");
const POSTGRES: &str = include_str!("../sql/ops/postgres.sql");
const ORACLE: &str = include_str!("../sql/ops/oracle.sql");
const DAMENG: &str = include_str!("../sql/ops/dameng.sql");

/// 运维查询的分类。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OpsCategory {
    Session,
    Lock,
    Storage,
    Sql,
}

impl OpsCategory {
    fn parse(s: &str) -> Option<Self> {
        match s {
            "session" => Some(Self::Session),
            "lock" => Some(Self::Lock),
            "storage" => Some(Self::Storage),
            "sql" => Some(Self::Sql),
            _ => None,
        }
    }
}

/// 一条运维查询。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpsQuery {
    pub key: String,
    pub name_zh: String,
    pub name_en: String,
    pub category: OpsCategory,
    pub sql: String,
}

/// 把模板文件拆成运维查询，保持文件中的顺序。段首注释缺名称或分类时 panic：模板随程序编译，由单元测试保证完整。
pub(crate) fn parse(text: &str) -> Vec<OpsQuery> {
    let mut out = Vec::new();
    let mut current: Option<OpsQuery> = None;
    let finish = |q: OpsQuery, out: &mut Vec<OpsQuery>| {
        assert!(!q.name_zh.is_empty() && !q.name_en.is_empty(), "运维查询 {} 缺名称", q.key);
        out.push(OpsQuery { sql: q.sql.trim().to_string(), ..q });
    };
    for line in text.lines() {
        let t = line.trim();
        if let Some(key) = t.strip_prefix("-- [").and_then(|r| r.strip_suffix(']')) {
            if let Some(q) = current.take() {
                finish(q, &mut out);
            }
            current = Some(OpsQuery {
                key: key.trim().to_string(),
                name_zh: String::new(),
                name_en: String::new(),
                category: OpsCategory::Session,
                sql: String::new(),
            });
            continue;
        }
        let Some(q) = current.as_mut() else { continue };
        if q.sql.is_empty() {
            if let Some(v) = t.strip_prefix("-- name.zh-CN:") {
                q.name_zh = v.trim().to_string();
                continue;
            }
            if let Some(v) = t.strip_prefix("-- name.en:") {
                q.name_en = v.trim().to_string();
                continue;
            }
            if let Some(v) = t.strip_prefix("-- category:") {
                q.category = OpsCategory::parse(v.trim())
                    .unwrap_or_else(|| panic!("运维查询 {} 的分类不认识：{v}", q.key));
                continue;
            }
            if t.is_empty() {
                continue;
            }
        }
        q.sql.push_str(line);
        q.sql.push('\n');
    }
    if let Some(q) = current {
        finish(q, &mut out);
    }
    out
}

fn base_file(family: Family) -> &'static str {
    match family {
        Family::MySql => MYSQL,
        Family::Postgres => POSTGRES,
        Family::Oracle => ORACLE,
        Family::Dameng => DAMENG,
    }
}

fn override_file(variant: Variant) -> Option<&'static str> {
    match variant {
        Variant::MariaDb => Some(MARIADB),
        _ => None,
    }
}

/// 某个变体的运维查询：家族的模板，变体覆盖文件中的同名 key 原位替换。
fn queries(variant: Variant) -> Vec<OpsQuery> {
    let mut list = parse(base_file(variant.family()));
    if let Some(o) = override_file(variant) {
        for q in parse(o) {
            match list.iter_mut().find(|x| x.key == q.key) {
                Some(slot) => *slot = q,
                None => list.push(q),
            }
        }
    }
    list
}

impl Dialect {
    /// 这个方言可用的运维查询（M1c3）。
    pub fn ops_queries(&self) -> &'static [OpsQuery] {
        static CACHE: OnceLock<Vec<(Variant, Vec<OpsQuery>)>> = OnceLock::new();
        let all = CACHE
            .get_or_init(|| crate::meta::template::ALL_VARIANTS.iter().map(|&v| (v, queries(v))).collect());
        &all.iter().find(|(v, _)| *v == self.variant()).expect("每个变体都有运维查询").1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEYS: [&str; 6] = ["sessions", "running", "locks", "storage", "largest_tables", "slow_sql"];

    #[test]
    fn every_variant_has_all_queries() {
        for &v in &crate::meta::template::ALL_VARIANTS {
            let qs = Dialect::new(v).ops_queries();
            assert_eq!(qs.iter().map(|q| q.key.as_str()).collect::<Vec<_>>(), KEYS, "{v:?}");
            for q in qs {
                assert!(!q.sql.is_empty() && !q.sql.contains("-- ["), "{v:?} {}", q.key);
                if matches!(q.category, OpsCategory::Session | OpsCategory::Lock) {
                    assert!(q.sql.contains("kill_sql"), "{v:?} {} 缺 kill_sql 列", q.key);
                }
            }
        }
    }

    #[test]
    fn parse_keeps_order_and_header() {
        let qs = parse(
            "-- 文件说明\n\n-- [b]\n-- name.zh-CN: 乙\n-- name.en: B\n-- category: lock\nSELECT 2\n-- 正文里的注释\nFROM t\n\n-- [a]\n-- name.zh-CN: 甲\n-- name.en: A\n-- category: sql\nSELECT 1\n",
        );
        assert_eq!(qs.len(), 2);
        assert_eq!(
            (qs[0].key.as_str(), qs[0].name_zh.as_str(), qs[0].category),
            ("b", "乙", OpsCategory::Lock)
        );
        assert_eq!(qs[0].sql, "SELECT 2\n-- 正文里的注释\nFROM t");
        assert_eq!((qs[1].key.as_str(), qs[1].name_en.as_str(), qs[1].sql.as_str()), ("a", "A", "SELECT 1"));
    }

    #[test]
    fn mariadb_overrides_in_place() {
        let my = Dialect::new(Variant::MySql).ops_queries();
        let maria = Dialect::new(Variant::MariaDb).ops_queries();
        let locks = |qs: &[OpsQuery]| qs.iter().find(|q| q.key == "locks").unwrap().sql.clone();
        assert_ne!(locks(my), locks(maria));
        assert_eq!(maria[2].key, "locks");
        // 兼容库用所属家族的模板
        assert_eq!(Dialect::new(Variant::TiDb).ops_queries(), my);
        assert_eq!(
            Dialect::new(Variant::Kingbase).ops_queries(),
            Dialect::new(Variant::Postgres).ops_queries()
        );
    }
}
````

- [ ] **Step 10: 检查**

```bash
export CARGO_INCREMENTAL=0
cargo fmt --all
cargo clippy --config "source.mirror.registry='sparse+https://rsproxy.cn/index/'" --workspace --all-targets -- -D warnings
DBLENS_LANG=en cargo test --config "source.mirror.registry='sparse+https://rsproxy.cn/index/'" --workspace
```

Expected: 全部测试通过（dialect 新增 3 个：每个变体都有全部 6 项且会话、锁带 kill_sql；解析保持顺序；MariaDB 原位覆盖、兼容库回退）。SQL 本身在任务 4 的实库测试中逐个执行。

- [ ] **Step 11: 提交**

```bash
git add crates/dblens-dialect/sql/ops/dameng.sql crates/dblens-dialect/sql/ops/mariadb.sql crates/dblens-dialect/sql/ops/mysql.sql crates/dblens-dialect/sql/ops/oracle.sql crates/dblens-dialect/sql/ops/postgres.sql crates/dblens-dialect/src/lib.rs crates/dblens-dialect/src/meta/mod.rs crates/dblens-dialect/src/meta/template.rs crates/dblens-dialect/src/ops.rs
git commit -m "feat(dialect): 运维查询模板：会话、正在执行的语句、锁等待、存储、最大的表、慢 SQL" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 4: 服务层：导出任务与运维查询列表

- `export/write.rs`：四种格式的写入器（`RowWriter`：`begin` 写表头、`rows` 分批、`finish` 返回警告）。CSV 用 `csv` 库，xlsx 用 `rust_xlsxwriter` 的低内存模式，INSERT 用方言的字面量，JSON 用 `serde_json` 流式写出。
- `export/job.rs`：`Service::start_export` 检查请求（单条查询、路径、任务数）后在后台另开连接（`registry.connect`，大字段上限 256 MB）执行、写文件，记一条历史；`ExportManager` 保存状态，支持取消、浏览器模式取走临时文件、过期清理。
- `Service::ops(profile_id)`：按连接上次识别出的变体选运维查询模板，没连接过时按驱动，不连接数据库。
- `Service` 增加 `exports`、`history`；`Service::open` 把临时目录设为数据目录下的 `exports/`（启动时清空）。
- 新增依赖 `csv = "1.4.0"`、`rust_xlsxwriter = { version = "0.100.0", default-features = false, features = ["constant_memory", "chrono"] }`（0.101.0 发布未满 7 天），服务层另加 `base64`（工作区已有）。`Cargo.lock` 由 cargo 自动更新。
- 假驱动：`SELECT ROWS n`、`SELECT SLEEP`、`SELECT FAIL` 与去掉 SELECT 的语句相同（导出只接受查询）。
- 实库测试：导出（大字段、中文、四种格式、行数）和运维查询（逐个执行，PG 的慢 SQL 允许失败）。`Live.service` 改为 `Arc<Service>`。

**Files:**
- Modify: `Cargo.toml`
- Modify: `crates/dblens-core/locales/app.json`
- Modify: `crates/dblens-service/Cargo.toml`
- Create: `crates/dblens-service/src/export/job.rs`
- Create: `crates/dblens-service/src/export/mod.rs`
- Create: `crates/dblens-service/src/export/write.rs`
- Modify: `crates/dblens-service/src/fake.rs`
- Modify: `crates/dblens-service/src/lib.rs`
- Modify: `crates/dblens-service/src/service.rs`
- Modify: `crates/dblens-service/tests/live.rs`

**Interfaces:**
- Consumes：任务 2 的 `set_lob_limit`；任务 3 的 `Dialect::ops_queries`；已有的 `MetadataService::data_sql`、`policy::classify`、`HistoryLog`。
- Produces（`dblens_service`）：
  - `ExportFormat::{Csv, Xlsx, Insert, Json}`、`ExportOptions { delimiter, header, bom, null_text, table, rows_per_insert }`（`#[serde(default)]`）、`ExportWarnings { lob_truncated, cell_truncated }`；
  - `ExportSource::{Sql(String), Data { schema, object, filter, order }}`、`ExportRequest { profile_id, source, format, options, path, file_name }`；
  - `ExportState::{Running, Done, Failed, Cancelled}`、`ExportStatus { id, state, rows, warnings, error: Option<Error>, file_name, path }`；
  - `Service::start_export(self: &Arc<Self>, req, allow_path: bool) -> Result<String>`；
  - `ExportManager::{status, cancel_or_remove, take_file, set_dir}`；`MAX_RUNNING_EXPORTS = 4`、`EXPORT_LOB_LIMIT = 256 MB`；
  - `Service::ops(&self, profile_id) -> Result<&'static [OpsQuery]>`；
  - 后端词条 `export.*`。

- [ ] **Step 1: 修改 `crates/dblens-service/tests/live.rs`**

```diff
--- a/crates/dblens-service/tests/live.rs
+++ b/crates/dblens-service/tests/live.rs
@@ -13,13 +13,14 @@ use dblens_dialect::meta::MetaKey;
 use dblens_dialect::{DdlKind, Direction};
 use dblens_driver::{ConnectParams, DriverId};
 use dblens_service::{
-    DataOrder, DefaultPolicy, DriverRegistry, Environment, ExecRequest, GenerateKind, ObjectKind,
-    ProfileInput, SecretBackend, Secrets, Service, SessionEvent, SessionId, Store,
+    DataOrder, DefaultPolicy, DriverRegistry, Environment, ExecRequest, ExportFormat, ExportOptions,
+    ExportRequest, ExportSource, ExportState, ExportStatus, GenerateKind, ObjectKind, ProfileInput,
+    SecretBackend, Secrets, Service, SessionEvent, SessionId, Store,
 };
 
 struct Live {
     _dir: tempfile::TempDir,
-    service: Service,
+    service: Arc<Service>,
     profile_id: String,
     driver: DriverId,
     /// 测试表所在的 schema。
@@ -42,7 +43,7 @@ async fn setup(env: &str) -> Option<Live> {
             registry.set_lib_dir(driver, Some(Path::new(lib))).unwrap();
         }
     }
-    let service = Service::with_parts(store, secrets, registry, Arc::new(DefaultPolicy));
+    let service = Arc::new(Service::with_parts(store, secrets, registry, Arc::new(DefaultPolicy)));
     let mut options = params.options.clone();
     options.retain(|k, _| !LIB_OPTIONS.contains(&k.as_str()));
     let schema = match driver {
@@ -457,3 +458,158 @@ async fn oracle_objects() {
 async fn dameng_objects() {
     objects_flow("DBLENS_TEST_DM_URL").await;
 }
+
+/// 运维查询（M1c3）：逐个执行。`allowed_failures` 是测试库上已知会失败的（如没有安装 pg_stat_statements）。
+async fn ops_flow(env: &str, allowed_failures: &[&str]) {
+    let Some(live) = setup(env).await else { return };
+    let s = &live.service;
+    let id = s.open_session(&live.profile_id).await.unwrap().id;
+    let mut failed = Vec::new();
+    for q in s.ops(&live.profile_id).unwrap() {
+        let events = live.submit(id, ExecRequest::new(q.sql.clone())).await;
+        if let Some(SessionEvent::Failed { error, .. }) =
+            events.iter().find(|e| matches!(e, SessionEvent::Failed { .. }))
+        {
+            eprintln!("{env} {}：{error}", q.key);
+            failed.push(q.key.as_str());
+        } else {
+            assert!(events.iter().any(|e| matches!(e, SessionEvent::ResultStart { .. })), "{env} {}", q.key);
+        }
+    }
+    assert!(failed.iter().all(|k| allowed_failures.contains(k)), "{env}：{failed:?}");
+    s.sessions.close(id, false).await.unwrap();
+}
+
+#[tokio::test]
+async fn mysql_ops() {
+    ops_flow("DBLENS_TEST_MYSQL_URL", &[]).await;
+}
+
+#[tokio::test]
+async fn mariadb_ops() {
+    ops_flow("DBLENS_TEST_MARIADB_URL", &[]).await;
+}
+
+#[tokio::test]
+async fn postgres_ops() {
+    // 测试库没有安装 pg_stat_statements 扩展
+    ops_flow("DBLENS_TEST_PG_URL", &["slow_sql"]).await;
+}
+
+#[tokio::test]
+async fn oracle_ops() {
+    ops_flow("DBLENS_TEST_ORACLE_URL", &[]).await;
+}
+
+#[tokio::test]
+async fn dameng_ops() {
+    ops_flow("DBLENS_TEST_DM_URL", &[]).await;
+}
+
+/// 导出（M1c3）：一行含中文和超过 64 KB 的大字段，四种格式都写出完整内容；另导出多行核对行数。
+async fn export_flow(env: &str) {
+    let Some(live) = setup(env).await else { return };
+    let s = &live.service;
+    let dir = tempfile::tempdir().unwrap();
+    let (big, rows_sql, rows) = match live.driver {
+        DriverId::Mysql => (
+            "SELECT 1 AS id, '中文' AS name, REPEAT('数', 100000) AS txt, CHAR_LENGTH(REPEAT('数', 100000)) AS len",
+            "WITH RECURSIVE t (n) AS (SELECT 1 UNION ALL SELECT n + 1 FROM t WHERE n < 990) SELECT n FROM t",
+            990,
+        ),
+        DriverId::Postgres => (
+            "SELECT 1 AS id, '中文' AS name, repeat('数', 100000) AS txt, 100000 AS len",
+            "SELECT g AS n FROM generate_series(1, 2500) AS g",
+            2500,
+        ),
+        DriverId::Oracle | DriverId::Dameng => (
+            "SELECT 1 AS \"id\", '中文' AS \"name\", RPAD(TO_CLOB('数'), 100000, '数') AS \"txt\", \
+             LENGTH(RPAD(TO_CLOB('数'), 100000, '数')) AS \"len\" FROM DUAL",
+            "SELECT LEVEL AS \"n\" FROM DUAL CONNECT BY LEVEL <= 2500",
+            2500,
+        ),
+    };
+    let export = |sql: &str, format: ExportFormat, name: &str| {
+        let path = dir.path().join(name);
+        let req = ExportRequest {
+            profile_id: live.profile_id.clone(),
+            source: ExportSource::Sql(sql.to_string()),
+            format,
+            options: ExportOptions { bom: false, table: "c3_t".into(), ..Default::default() },
+            path: Some(path.clone()),
+            file_name: name.to_string(),
+        };
+        async move {
+            let id = s.start_export(req, true).await.unwrap();
+            let st = loop {
+                let st = s.exports.status(&id).unwrap();
+                if st.state != ExportState::Running {
+                    break st;
+                }
+                tokio::time::sleep(Duration::from_millis(50)).await;
+            };
+            (st, path)
+        }
+    };
+    let done = |st: &ExportStatus| assert_eq!(st.state, ExportState::Done, "{env}：{st:?}");
+
+    // CSV：数据库报告的字符数与写出的一致
+    let (st, path) = export(big, ExportFormat::Csv, "big.csv").await;
+    done(&st);
+    assert_eq!((st.rows, st.warnings.lob_truncated), (1, 0), "{env}");
+    let mut csv = csv::Reader::from_path(&path).unwrap();
+    let record = csv.records().next().unwrap().unwrap();
+    let len: usize = record[3].parse().unwrap();
+    assert!(len * 3 > 64 * 1024, "{env}：大字段要超过 64 KB，实际 {len} 个字符");
+    assert_eq!((&record[1], record[2].chars().count()), ("中文", len), "{env}");
+
+    // JSON
+    let (st, path) = export(big, ExportFormat::Json, "big.json").await;
+    done(&st);
+    let v: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
+    assert_eq!(v[0]["txt"].as_str().unwrap().chars().count(), len, "{env}");
+
+    // INSERT：完整的大字段写在一条语句里
+    let (st, path) = export(big, ExportFormat::Insert, "big.sql").await;
+    done(&st);
+    let text = std::fs::read_to_string(&path).unwrap();
+    assert!(text.starts_with("INSERT INTO c3_t ("), "{env}：{}", &text[..80]);
+    assert_eq!(text.matches('数').count(), len, "{env}");
+
+    // xlsx：单元格上限 32767 个字符，截断并报警告
+    let (st, path) = export(big, ExportFormat::Xlsx, "big.xlsx").await;
+    done(&st);
+    assert_eq!(st.warnings.cell_truncated, 1, "{env}");
+    assert_eq!(&std::fs::read(&path).unwrap()[..2], b"PK");
+
+    // 多行
+    let (st, path) = export(rows_sql, ExportFormat::Json, "rows.json").await;
+    done(&st);
+    let v: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
+    assert_eq!((st.rows, v.as_array().unwrap().len()), (rows, rows as usize), "{env}");
+}
+
+#[tokio::test]
+async fn mysql_export() {
+    export_flow("DBLENS_TEST_MYSQL_URL").await;
+}
+
+#[tokio::test]
+async fn mariadb_export() {
+    export_flow("DBLENS_TEST_MARIADB_URL").await;
+}
+
+#[tokio::test]
+async fn postgres_export() {
+    export_flow("DBLENS_TEST_PG_URL").await;
+}
+
+#[tokio::test]
+async fn oracle_export() {
+    export_flow("DBLENS_TEST_ORACLE_URL").await;
+}
+
+#[tokio::test]
+async fn dameng_export() {
+    export_flow("DBLENS_TEST_DM_URL").await;
+}
```

- [ ] **Step 2: 修改 `Cargo.toml`**

```diff
--- a/Cargo.toml
+++ b/Cargo.toml
@@ -28,6 +28,7 @@ base64 = "0.23.1"
 bytes = "1.12.1"
 chacha20poly1305 = "0.11.0"
 chrono = { version = "0.4.45", default-features = false, features = ["std", "clock", "serde"] }
+csv = "1.4.0"
 directories = "6.0.0"
 futures = "0.3.34"
 http-body-util = "0.1.5"
@@ -40,6 +41,7 @@ oracle = { version = "0.6.3", features = ["chrono"] }
 percent-encoding = "2.3.2"
 reqwest = { version = "0.13.5", default-features = false, features = ["rustls"] }
 rusqlite = { version = "0.41.0", features = ["bundled"] }
+rust_xlsxwriter = { version = "0.100.0", default-features = false, features = ["constant_memory", "chrono"] }
 rust-i18n = "4.2.4"
 rustls = { version = "0.23.46", default-features = false, features = ["aws_lc_rs", "std", "tls12"] }
 semver = "1.0.28"
```

- [ ] **Step 3: 修改 `crates/dblens-core/locales/app.json`**

```diff
--- a/crates/dblens-core/locales/app.json
+++ b/crates/dblens-core/locales/app.json
@@ -40,6 +40,16 @@
   "error.kind.not_found": {"zh-CN": "找不到%{what}", "en": "Not found: %{what}"},
   "error.kind.sql": {"zh-CN": "SQL 错误 [%{code}]：%{message}", "en": "SQL error [%{code}]: %{message}"},
   "error.kind.timeout": {"zh-CN": "执行超时", "en": "Execution timed out"},
+  "export.bad_delimiter": {"zh-CN": "CSV 分隔符只能是逗号、制表符或分号", "en": "The CSV delimiter must be a comma, tab or semicolon"},
+  "export.bad_path": {"zh-CN": "保存路径无效：%{path}（需要绝对路径，且所在目录存在）", "en": "Invalid save path: %{path} (it must be absolute and its folder must exist)"},
+  "export.binary_cell": {"zh-CN": "二进制 %{bytes} 字节", "en": "binary, %{bytes} bytes"},
+  "export.no_file": {"zh-CN": "这个导出任务没有可下载的文件", "en": "This export has no file to download"},
+  "export.no_table": {"zh-CN": "INSERT 语句需要目标表名", "en": "INSERT statements need a target table name"},
+  "export.not_found": {"zh-CN": "导出任务 %{id} 不存在或已过期", "en": "Export %{id} does not exist or has expired"},
+  "export.not_query": {"zh-CN": "只能导出单条查询语句的结果", "en": "Only the result of a single query can be exported"},
+  "export.path_not_allowed": {"zh-CN": "浏览器模式不能指定保存路径", "en": "A save path cannot be given in browser mode"},
+  "export.too_many": {"zh-CN": "同时最多进行 %{max} 个导出任务，请等其他任务完成", "en": "At most %{max} exports can run at once; wait for one to finish"},
+  "export.write_failed": {"zh-CN": "写导出文件失败：%{error}", "en": "Failed to write the export file: %{error}"},
   "favorite.folder_has_no_sql": {"zh-CN": "文件夹不能有 SQL", "en": "A folder has no SQL"},
   "favorite.move_into_self": {"zh-CN": "不能把文件夹移到它自己或它的子文件夹中", "en": "Cannot move a folder into itself or one of its subfolders"},
   "favorite.name_required": {"zh-CN": "名称不能为空", "en": "The name is required"},
```

- [ ] **Step 4: 修改 `crates/dblens-service/Cargo.toml`**

```diff
--- a/crates/dblens-service/Cargo.toml
+++ b/crates/dblens-service/Cargo.toml
@@ -17,8 +17,10 @@ testing = []
 [dependencies]
 argon2.workspace = true
 async-trait.workspace = true
+base64.workspace = true
 chacha20poly1305.workspace = true
 chrono.workspace = true
+csv.workspace = true
 dblens-core.workspace = true
 dblens-dialect.workspace = true
 dblens-driver.workspace = true
@@ -29,6 +31,7 @@ dblens-driver-postgres = { workspace = true, optional = true }
 directories.workspace = true
 keyring.workspace = true
 rusqlite.workspace = true
+rust_xlsxwriter.workspace = true
 serde.workspace = true
 serde_json.workspace = true
 sqlparser.workspace = true
@@ -37,4 +40,5 @@ tracing.workspace = true
 uuid.workspace = true
 
 [dev-dependencies]
+csv.workspace = true
 tempfile.workspace = true
```

- [ ] **Step 5: 新建 `crates/dblens-service/src/export/job.rs`**

```rust
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
```

- [ ] **Step 6: 新建 `crates/dblens-service/src/export/mod.rs`**

```rust
//! 导出查询结果（M1c3 设计文档第 3 节）。

mod job;
mod write;

pub use job::{
    EXPORT_LOB_LIMIT, ExportManager, ExportRequest, ExportSource, ExportState, ExportStatus,
    MAX_RUNNING_EXPORTS,
};
pub use write::{ExportFormat, ExportOptions, ExportWarnings};
```

- [ ] **Step 7: 新建 `crates/dblens-service/src/export/write.rs`**

```rust
//! 导出文件的写入（M1c3 设计文档 3.4 节）：CSV、xlsx、INSERT、JSON。只管把列和行写进文件，不涉及数据库。
//! 每种格式实现 `RowWriter`：先 `begin` 写表头，再分批 `rows`，最后 `finish`。

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use dblens_core::{Error, Result, Row, Value, msg};
use dblens_dialect::Dialect;
use dblens_driver::ColumnMeta;
use rust_xlsxwriter::{Format, Workbook, XlsxError};
use serde::{Deserialize, Serialize};

/// 导出格式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExportFormat {
    Csv,
    Xlsx,
    Insert,
    Json,
}

impl ExportFormat {
    pub fn extension(self) -> &'static str {
        match self {
            Self::Csv => "csv",
            Self::Xlsx => "xlsx",
            Self::Insert => "sql",
            Self::Json => "json",
        }
    }
}

/// 导出选项。只有对应格式用到的字段起作用。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ExportOptions {
    /// CSV 的分隔符：`,`、`\t` 或 `;`。
    pub delimiter: char,
    /// CSV 是否写表头。
    pub header: bool,
    /// CSV 是否在开头写 UTF-8 BOM（Excel 直接打开中文不乱码）。
    pub bom: bool,
    /// CSV 中 NULL 写成的文字。
    pub null_text: String,
    /// INSERT 的目标表（原样写入，需要时由用户加引号）。
    pub table: String,
    /// INSERT 每条语句的行数；大于 1 时多行合并成一条。
    pub rows_per_insert: usize,
}

impl Default for ExportOptions {
    fn default() -> Self {
        Self {
            delimiter: ',',
            header: true,
            bom: true,
            null_text: String::new(),
            table: "table_name".into(),
            rows_per_insert: 1,
        }
    }
}

/// 导出中出现的、不中断导出的问题。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct ExportWarnings {
    /// 超过读取上限、只写出了前一部分的大字段个数。
    pub lob_truncated: u64,
    /// xlsx 中超过 32767 个字符被截断的单元格个数。
    pub cell_truncated: u64,
}

pub trait RowWriter: Send {
    fn begin(&mut self, columns: &[ColumnMeta]) -> Result<()>;
    fn rows(&mut self, rows: &[Row]) -> Result<()>;
    /// 写完并关闭文件，返回过程中的警告。
    fn finish(self: Box<Self>) -> Result<ExportWarnings>;
}

/// 为 `path` 新建对应格式的写入器。INSERT 的值按 `dialect` 的写法生成。
pub fn writer(
    format: ExportFormat,
    options: &ExportOptions,
    path: &Path,
    dialect: Dialect,
) -> Result<Box<dyn RowWriter>> {
    Ok(match format {
        ExportFormat::Csv => Box::new(CsvWriter::new(path, options)?),
        ExportFormat::Xlsx => Box::new(XlsxWriter::new(path)?),
        ExportFormat::Insert => Box::new(InsertWriter::new(path, options, dialect)?),
        ExportFormat::Json => Box::new(JsonWriter::new(path)?),
    })
}

fn io_error(e: impl std::fmt::Display) -> Error {
    Error::internal(msg!("export.write_failed", error = e))
}

fn create(path: &Path) -> Result<BufWriter<File>> {
    Ok(BufWriter::with_capacity(256 * 1024, File::create(path).map_err(io_error)?))
}

/// 单元格的文本（与前端结果表格的显示一致）；NULL 为 None。二进制写成 base64，大字段写出已读取的部分。
pub fn cell_text(v: &Value, warnings: &mut ExportWarnings) -> Option<String> {
    Some(match v {
        Value::Null => return None,
        Value::Bool(b) => b.to_string(),
        Value::Int(i) => i.to_string(),
        Value::UInt(u) => u.to_string(),
        Value::Float(f) => f.to_string(),
        Value::Decimal(s) | Value::String(s) | Value::Interval(s) => s.clone(),
        Value::Bytes(b) => STANDARD.encode(b),
        Value::Date(d) => d.format("%Y-%m-%d").to_string(),
        Value::Time(t) => t.format("%H:%M:%S%.f").to_string(),
        Value::DateTime(dt) => dt.format("%Y-%m-%d %H:%M:%S%.f").to_string(),
        Value::DateTimeTz(dt) => dt.format("%Y-%m-%d %H:%M:%S%.f%:z").to_string(),
        Value::Json(j) => j.to_string(),
        Value::Uuid(u) => u.to_string(),
        Value::Lob(lob) => {
            if lob.truncated {
                warnings.lob_truncated += 1;
            }
            lob.preview.clone()
        }
        Value::Other { text, .. } => text.clone(),
    })
}

// ---- CSV ----

struct CsvWriter {
    out: csv::Writer<BufWriter<File>>,
    header: bool,
    null_text: String,
    warnings: ExportWarnings,
}

impl CsvWriter {
    fn new(path: &Path, options: &ExportOptions) -> Result<Self> {
        let delimiter = u8::try_from(options.delimiter)
            .ok()
            .filter(|d| matches!(d, b',' | b'\t' | b';'))
            .ok_or_else(|| Error::invalid(msg!("export.bad_delimiter")))?;
        let mut file = create(path)?;
        if options.bom {
            file.write_all("\u{feff}".as_bytes()).map_err(io_error)?;
        }
        Ok(Self {
            out: csv::WriterBuilder::new().delimiter(delimiter).from_writer(file),
            header: options.header,
            null_text: options.null_text.clone(),
            warnings: ExportWarnings::default(),
        })
    }
}

impl RowWriter for CsvWriter {
    fn begin(&mut self, columns: &[ColumnMeta]) -> Result<()> {
        if self.header {
            self.out.write_record(columns.iter().map(|c| c.name.as_str())).map_err(io_error)?;
        }
        Ok(())
    }

    fn rows(&mut self, rows: &[Row]) -> Result<()> {
        for row in rows {
            let fields: Vec<String> = row
                .iter()
                .map(|v| cell_text(v, &mut self.warnings).unwrap_or_else(|| self.null_text.clone()))
                .collect();
            self.out.write_record(&fields).map_err(io_error)?;
        }
        Ok(())
    }

    fn finish(mut self: Box<Self>) -> Result<ExportWarnings> {
        self.out.flush().map_err(io_error)?;
        Ok(self.warnings)
    }
}

// ---- xlsx ----

/// 单个工作表的行数上限（1048576 行，其中一行是表头）。
const XLSX_MAX_ROWS: u32 = 1_048_576;
/// 单元格最多 32767 个字符。
const XLSX_MAX_CHARS: usize = 32_767;

struct XlsxWriter {
    path: std::path::PathBuf,
    book: Workbook,
    columns: Vec<String>,
    /// 当前工作表的下标和下一行的行号。
    sheet: usize,
    row: u32,
    header: Format,
    date: Format,
    time: Format,
    datetime: Format,
    warnings: ExportWarnings,
}

fn xlsx_error(e: XlsxError) -> Error {
    io_error(e)
}

impl XlsxWriter {
    fn new(path: &Path) -> Result<Self> {
        // 先建一个空文件：路径不可写时立即报错，而不是写完全部数据后才发现
        File::create(path).map_err(io_error)?;
        Ok(Self {
            path: path.to_path_buf(),
            book: Workbook::new(),
            columns: Vec::new(),
            sheet: 0,
            row: 0,
            header: Format::new().set_bold(),
            date: Format::new().set_num_format("yyyy-mm-dd"),
            time: Format::new().set_num_format("hh:mm:ss"),
            datetime: Format::new().set_num_format("yyyy-mm-dd hh:mm:ss"),
            warnings: ExportWarnings::default(),
        })
    }

    /// 新开一个工作表（低内存模式：按行顺序写入，写过的行落到临时文件），写上表头。
    fn new_sheet(&mut self) -> Result<()> {
        self.book.add_worksheet_with_constant_memory().map_err(xlsx_error)?;
        self.sheet = self.book.worksheets_mut().len() - 1;
        let ws = self.book.worksheet_from_index(self.sheet).map_err(xlsx_error)?;
        for (i, name) in self.columns.iter().enumerate() {
            ws.write_string_with_format(0, i as u16, name.as_str(), &self.header).map_err(xlsx_error)?;
        }
        self.row = 1;
        Ok(())
    }
}

impl RowWriter for XlsxWriter {
    fn begin(&mut self, columns: &[ColumnMeta]) -> Result<()> {
        self.columns = columns.iter().map(|c| c.name.clone()).collect();
        self.new_sheet()
    }

    fn rows(&mut self, rows: &[Row]) -> Result<()> {
        for row in rows {
            if self.row >= XLSX_MAX_ROWS {
                self.new_sheet()?;
            }
            let r = self.row;
            let ws = self.book.worksheet_from_index(self.sheet).map_err(xlsx_error)?;
            for (i, v) in row.iter().enumerate() {
                let c = i as u16;
                match v {
                    Value::Null => {}
                    Value::Bool(b) => {
                        ws.write_boolean(r, c, *b).map_err(xlsx_error)?;
                    }
                    Value::Int(n) => {
                        ws.write_number(r, c, *n as f64).map_err(xlsx_error)?;
                    }
                    Value::UInt(n) => {
                        ws.write_number(r, c, *n as f64).map_err(xlsx_error)?;
                    }
                    Value::Float(f) if f.is_finite() => {
                        ws.write_number(r, c, *f).map_err(xlsx_error)?;
                    }
                    Value::Date(d) => {
                        ws.write_datetime_with_format(r, c, d, &self.date).map_err(xlsx_error)?;
                    }
                    Value::Time(t) => {
                        ws.write_datetime_with_format(r, c, t, &self.time).map_err(xlsx_error)?;
                    }
                    Value::DateTime(dt) => {
                        ws.write_datetime_with_format(r, c, dt, &self.datetime).map_err(xlsx_error)?;
                    }
                    Value::Bytes(b) => {
                        ws.write_string(r, c, msg!("export.binary_cell", bytes = b.len()).current())
                            .map_err(xlsx_error)?;
                    }
                    Value::Lob(lob) if lob.binary => {
                        ws.write_string(r, c, msg!("export.binary_cell", bytes = lob.total_len).current())
                            .map_err(xlsx_error)?;
                    }
                    // 精确小数写成文本，避免经过浮点丢精度
                    other => {
                        let mut text = cell_text(other, &mut self.warnings).unwrap_or_default();
                        if text.chars().count() > XLSX_MAX_CHARS {
                            text = text.chars().take(XLSX_MAX_CHARS).collect();
                            self.warnings.cell_truncated += 1;
                        }
                        ws.write_string(r, c, text).map_err(xlsx_error)?;
                    }
                }
            }
            self.row += 1;
        }
        Ok(())
    }

    fn finish(mut self: Box<Self>) -> Result<ExportWarnings> {
        if self.book.worksheets_mut().is_empty() {
            self.new_sheet()?;
        }
        self.book.save(&self.path).map_err(xlsx_error)?;
        Ok(self.warnings)
    }
}

// ---- INSERT ----

struct InsertWriter {
    out: BufWriter<File>,
    dialect: Dialect,
    table: String,
    per_statement: usize,
    columns: String,
    /// 当前语句已写的行数。
    pending: usize,
    warnings: ExportWarnings,
}

impl InsertWriter {
    fn new(path: &Path, options: &ExportOptions, dialect: Dialect) -> Result<Self> {
        let table = options.table.trim();
        if table.is_empty() {
            return Err(Error::invalid(msg!("export.no_table")));
        }
        Ok(Self {
            out: create(path)?,
            dialect,
            table: table.to_string(),
            per_statement: options.rows_per_insert.max(1),
            columns: String::new(),
            pending: 0,
            warnings: ExportWarnings::default(),
        })
    }

    fn value(&mut self, v: &Value) -> Result<String> {
        match v {
            // 已读到的部分照样写出，记一次截断
            Value::Lob(lob) => {
                if lob.truncated {
                    self.warnings.lob_truncated += 1;
                }
                if lob.binary {
                    let bytes = STANDARD.decode(&lob.preview).unwrap_or_default();
                    self.dialect.literal(&Value::Bytes(bytes))
                } else {
                    self.dialect.literal(&Value::String(lob.preview.clone()))
                }
            }
            Value::Other { text, .. } => self.dialect.literal(&Value::String(text.clone())),
            other => self.dialect.literal(other),
        }
    }

    /// 结束当前的多行语句。
    fn close_statement(&mut self) -> Result<()> {
        if self.pending > 0 {
            self.out.write_all(b";\n").map_err(io_error)?;
            self.pending = 0;
        }
        Ok(())
    }
}

impl RowWriter for InsertWriter {
    fn begin(&mut self, columns: &[ColumnMeta]) -> Result<()> {
        self.columns = columns.iter().map(|c| self.dialect.ident(&c.name)).collect::<Vec<_>>().join(", ");
        Ok(())
    }

    fn rows(&mut self, rows: &[Row]) -> Result<()> {
        for row in rows {
            let values = row.iter().map(|v| self.value(v)).collect::<Result<Vec<_>>>()?.join(", ");
            let line = if self.per_statement == 1 {
                format!("INSERT INTO {} ({}) VALUES ({values});\n", self.table, self.columns)
            } else if self.pending == 0 {
                format!("INSERT INTO {} ({}) VALUES\n  ({values})", self.table, self.columns)
            } else {
                format!(",\n  ({values})")
            };
            self.out.write_all(line.as_bytes()).map_err(io_error)?;
            if self.per_statement > 1 {
                self.pending += 1;
                if self.pending == self.per_statement {
                    self.close_statement()?;
                }
            }
        }
        Ok(())
    }

    fn finish(mut self: Box<Self>) -> Result<ExportWarnings> {
        self.close_statement()?;
        self.out.flush().map_err(io_error)?;
        Ok(self.warnings)
    }
}

// ---- JSON ----

struct JsonWriter {
    out: BufWriter<File>,
    /// 列名；重复的依次加 `_2`、`_3`。
    keys: Vec<String>,
    first: bool,
    warnings: ExportWarnings,
}

impl JsonWriter {
    fn new(path: &Path) -> Result<Self> {
        Ok(Self { out: create(path)?, keys: Vec::new(), first: true, warnings: ExportWarnings::default() })
    }
}

/// 重复的列名依次加后缀 `_2`、`_3`，保证 JSON 对象的键唯一。
pub fn unique_keys(columns: &[ColumnMeta]) -> Vec<String> {
    let mut keys: Vec<String> = Vec::with_capacity(columns.len());
    for c in columns {
        let mut key = c.name.clone();
        let mut n = 1;
        while keys.contains(&key) {
            n += 1;
            key = format!("{}_{n}", c.name);
        }
        keys.push(key);
    }
    keys
}

fn json_value(v: &Value, warnings: &mut ExportWarnings) -> serde_json::Value {
    use serde_json::Value as J;
    match v {
        Value::Null => J::Null,
        Value::Bool(b) => J::Bool(*b),
        Value::Int(i) => J::from(*i),
        Value::UInt(u) => J::from(*u),
        Value::Float(f) => serde_json::Number::from_f64(*f).map(J::Number).unwrap_or(J::Null),
        Value::Json(j) => j.clone(),
        other => cell_text(other, warnings).map(J::String).unwrap_or(J::Null),
    }
}

impl RowWriter for JsonWriter {
    fn begin(&mut self, columns: &[ColumnMeta]) -> Result<()> {
        self.keys = unique_keys(columns);
        self.out.write_all(b"[").map_err(io_error)?;
        Ok(())
    }

    fn rows(&mut self, rows: &[Row]) -> Result<()> {
        for row in rows {
            let obj: serde_json::Map<String, serde_json::Value> = self
                .keys
                .iter()
                .cloned()
                .zip(row.iter().map(|v| json_value(v, &mut self.warnings)))
                .collect();
            self.out.write_all(if self.first { b"\n  " } else { b",\n  " }).map_err(io_error)?;
            self.first = false;
            serde_json::to_writer(&mut self.out, &obj).map_err(io_error)?;
        }
        Ok(())
    }

    fn finish(mut self: Box<Self>) -> Result<ExportWarnings> {
        if self.keys.is_empty() && self.first {
            // 没有结果集时也写出合法的 JSON
            self.out.write_all(b"[").map_err(io_error)?;
        }
        self.out.write_all(if self.first { b"]\n" } else { b"\n]\n" }).map_err(io_error)?;
        self.out.flush().map_err(io_error)?;
        Ok(self.warnings)
    }
}

#[cfg(test)]
mod tests {
    use chrono::NaiveDate;
    use dblens_core::LobPreview;
    use dblens_dialect::Variant;

    use super::*;

    fn cols(names: &[&str]) -> Vec<ColumnMeta> {
        names.iter().map(|n| ColumnMeta::new(*n, "text")).collect()
    }

    fn sample() -> Vec<Row> {
        vec![
            vec![Value::Int(1), Value::String("a,\"b\"\n中文".into()), Value::Null],
            vec![
                Value::Int(2),
                Value::Bytes(vec![0, 255]),
                Value::Date(NaiveDate::from_ymd_opt(2026, 10, 26).unwrap()),
            ],
        ]
    }

    fn run(
        format: ExportFormat,
        options: &ExportOptions,
        columns: &[ColumnMeta],
        rows: &[Row],
    ) -> (Vec<u8>, ExportWarnings) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(format!("out.{}", format.extension()));
        let mut w = writer(format, options, &path, Dialect::new(Variant::Postgres)).unwrap();
        w.begin(columns).unwrap();
        // 分两批写，检查跨批次的状态
        let (first, rest) = rows.split_at(rows.len().min(1));
        w.rows(first).unwrap();
        w.rows(rest).unwrap();
        let warnings = w.finish().unwrap();
        (std::fs::read(&path).unwrap(), warnings)
    }

    #[test]
    fn csv_escapes_and_bom() {
        let (bytes, _) =
            run(ExportFormat::Csv, &ExportOptions::default(), &cols(&["id", "name", "d"]), &sample());
        let text = String::from_utf8(bytes).unwrap();
        assert_eq!(text, "\u{feff}id,name,d\n1,\"a,\"\"b\"\"\n中文\",\n2,AP8=,2026-10-26\n");
    }

    #[test]
    fn csv_options() {
        let options = ExportOptions {
            delimiter: '\t',
            header: false,
            bom: false,
            null_text: "NULL".into(),
            ..Default::default()
        };
        let (bytes, _) = run(ExportFormat::Csv, &options, &cols(&["id", "name", "d"]), &sample()[..1]);
        assert_eq!(String::from_utf8(bytes).unwrap(), "1\t\"a,\"\"b\"\"\n中文\"\tNULL\n");
        let bad = ExportOptions { delimiter: '|', ..Default::default() };
        let dir = tempfile::tempdir().unwrap();
        assert!(
            writer(ExportFormat::Csv, &bad, &dir.path().join("x.csv"), Dialect::new(Variant::MySql)).is_err()
        );
    }

    #[test]
    fn insert_single_and_multi_row() {
        let options = ExportOptions { table: "public.t".into(), ..Default::default() };
        let (bytes, _) = run(ExportFormat::Insert, &options, &cols(&["id", "name", "d"]), &sample());
        assert_eq!(
            String::from_utf8(bytes).unwrap(),
            "INSERT INTO public.t (id, name, d) VALUES (1, 'a,\"b\"\n中文', NULL);\n\
             INSERT INTO public.t (id, name, d) VALUES (2, decode('00FF','hex'), DATE '2026-10-26');\n"
        );
        let options = ExportOptions { table: "t".into(), rows_per_insert: 2, ..Default::default() };
        let rows: Vec<Row> = (1..=3).map(|i| vec![Value::Int(i)]).collect();
        let (bytes, _) = run(ExportFormat::Insert, &options, &cols(&["id"]), &rows);
        assert_eq!(
            String::from_utf8(bytes).unwrap(),
            "INSERT INTO t (id) VALUES\n  (1),\n  (2);\nINSERT INTO t (id) VALUES\n  (3);\n"
        );
    }

    #[test]
    fn json_types_and_duplicate_keys() {
        let rows = vec![vec![
            Value::Int(1),
            Value::Decimal("1.50".into()),
            Value::Null,
            Value::Json(serde_json::json!({"k": 1})),
        ]];
        let (bytes, _) =
            run(ExportFormat::Json, &ExportOptions::default(), &cols(&["id", "id", "n", "j"]), &rows);
        let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v, serde_json::json!([{"id": 1, "id_2": "1.50", "n": null, "j": {"k": 1}}]));
        let (bytes, _) = run(ExportFormat::Json, &ExportOptions::default(), &cols(&["id"]), &[]);
        assert_eq!(serde_json::from_slice::<serde_json::Value>(&bytes).unwrap(), serde_json::json!([]));
    }

    #[test]
    fn truncated_lob_is_counted() {
        let lob =
            Value::Lob(LobPreview { binary: false, preview: "abc".into(), total_len: 9, truncated: true });
        let (bytes, warnings) = run(
            ExportFormat::Csv,
            &ExportOptions { bom: false, ..Default::default() },
            &cols(&["c"]),
            &[vec![lob.clone()], vec![lob]],
        );
        assert_eq!(String::from_utf8(bytes).unwrap(), "c\nabc\nabc\n");
        assert_eq!(warnings.lob_truncated, 2);
    }

    #[test]
    fn xlsx_writes_and_truncates_long_text() {
        let long = "长".repeat(XLSX_MAX_CHARS + 5);
        let rows = vec![vec![Value::Int(1), Value::String(long)], vec![Value::Float(1.5), Value::Null]];
        let (bytes, warnings) = run(ExportFormat::Xlsx, &ExportOptions::default(), &cols(&["n", "s"]), &rows);
        assert_eq!(&bytes[..2], b"PK", "xlsx 是 zip 文件");
        assert_eq!(warnings.cell_truncated, 1);
    }
}
```

- [ ] **Step 8: 修改 `crates/dblens-service/src/fake.rs`**

```diff
--- a/crates/dblens-service/src/fake.rs
+++ b/crates/dblens-service/src/fake.rs
@@ -111,6 +111,11 @@ impl Connection for FakeConn {
     async fn execute<'a>(&'a mut self, sql: &'a str, _: &'a [Value]) -> Result<Box<dyn ExecStream + 'a>> {
         let sql = sql.trim();
         self.log.lock().unwrap().push(format!("execute {sql}"));
+        // 导出只接受查询语句：`SELECT ROWS n`、`SELECT SLEEP`、`SELECT FAIL` 与去掉 SELECT 的语句相同
+        let sql = sql
+            .strip_prefix("SELECT ")
+            .filter(|s| s.starts_with("ROWS ") || matches!(*s, "SLEEP" | "FAIL"))
+            .unwrap_or(sql);
         let mut stream = FakeStream { events: VecDeque::new(), rows_left: 0, next_value: 1, sleep: None };
         match sql {
             "FAIL" => {
```

- [ ] **Step 9: 修改 `crates/dblens-service/src/lib.rs`**

```diff
--- a/crates/dblens-service/src/lib.rs
+++ b/crates/dblens-service/src/lib.rs
@@ -1,5 +1,6 @@
 //! 服务层：连接配置、密码、驱动注册表、会话、元数据缓存、执行策略。供 `dblens-api` 使用，自身不处理 HTTP。
 
+mod export;
 /// 测试用的假驱动，供本 crate 和 `dblens-api` 的测试使用（feature `testing`）。
 #[cfg(any(test, feature = "testing"))]
 pub mod fake;
@@ -15,6 +16,10 @@ mod service;
 mod session;
 mod store;
 
+pub use export::{
+    EXPORT_LOB_LIMIT, ExportFormat, ExportManager, ExportOptions, ExportRequest, ExportSource, ExportState,
+    ExportStatus, ExportWarnings, MAX_RUNNING_EXPORTS,
+};
 pub use favorite::{Favorite, FavoriteKind, FavoriteMove, FavoritePatch, NewFavorite};
 pub use history::{
     DEFAULT_HISTORY_LIMIT, HISTORY_ENABLED_SETTING, HISTORY_LIMIT_SETTING, HistoryItem, HistoryLog,
```

- [ ] **Step 10: 修改 `crates/dblens-service/src/service.rs`**

```diff
--- a/crates/dblens-service/src/service.rs
+++ b/crates/dblens-service/src/service.rs
@@ -4,7 +4,9 @@ use std::path::Path;
 use std::sync::Arc;
 
 use dblens_core::{Lang, Result, set_default_lang};
+use dblens_dialect::{Dialect, OpsQuery, Variant};
 
+use crate::export::ExportManager;
 use crate::history::HistoryLog;
 use crate::metadata::{MetadataCache, MetadataService};
 use crate::policy::{DefaultPolicy, Policy};
@@ -23,6 +25,9 @@ pub struct Service {
     pub registry: Arc<DriverRegistry>,
     pub sessions: SessionManager,
     pub metadata: MetadataService,
+    /// 导出任务（M1c3）。
+    pub exports: ExportManager,
+    pub(crate) history: HistoryLog,
 }
 
 impl Service {
@@ -33,6 +38,7 @@ impl Service {
         let registry = Arc::new(DriverRegistry::new(store.clone()));
         let service = Self::with_parts(store, secrets, registry, Arc::new(DefaultPolicy));
         service.apply_lang_setting()?;
+        service.exports.set_dir(data_dir.join("exports"));
         Ok(service)
     }
 
@@ -64,8 +70,10 @@ impl Service {
     ) -> Self {
         let cache = Arc::new(MetadataCache::new());
         Self {
-            sessions: SessionManager::new(policy, cache.clone(), history),
+            sessions: SessionManager::new(policy, cache.clone(), history.clone()),
             metadata: MetadataService::new(registry.clone(), secrets.clone(), cache),
+            exports: ExportManager::default(),
+            history,
             store,
             secrets,
             registry,
@@ -123,6 +131,13 @@ impl Service {
         self.registry.test_connection(&p, password.to_string()).await
     }
 
+    /// 连接可用的运维查询（M1c3）：按上次连接时识别出的变体选模板，没连接过时按驱动。不需要连接数据库。
+    pub fn ops(&self, profile_id: &str) -> Result<&'static [OpsQuery]> {
+        let p = self.store.profile(profile_id)?;
+        let variant = p.variant.unwrap_or_else(|| Variant::base(p.input.driver));
+        Ok(Dialect::new(variant).ops_queries())
+    }
+
     /// 为编辑器标签页打开一个会话（独占一个连接）。
     pub async fn open_session(&self, profile_id: &str) -> Result<SessionInfo> {
         let p = self.store.profile(profile_id)?;
```

- [ ] **Step 11: 检查**

```bash
export CARGO_INCREMENTAL=0
cargo fmt --all
cargo clippy --config "source.mirror.registry='sparse+https://rsproxy.cn/index/'" --workspace --all-targets -- -D warnings
DBLENS_LANG=en cargo test --config "source.mirror.registry='sparse+https://rsproxy.cn/index/'" --workspace
```

实库测试：在上面的 `cargo test` 前以内联环境变量给出 `DBLENS_TEST_MYSQL_URL`、`DBLENS_TEST_MARIADB_URL`、`DBLENS_TEST_PG_URL`、`DBLENS_TEST_ORACLE_URL`、`DBLENS_TEST_DM_URL`（密码不写进任何文件）。

Expected: 全部测试通过（服务层新增写入器 6 个、导出任务 4 个）；`*_export` 和 `*_ops` 在 5 个实库上通过。

- [ ] **Step 12: 提交**

```bash
git add Cargo.lock Cargo.toml crates/dblens-core/locales/app.json crates/dblens-service/Cargo.toml crates/dblens-service/src/export/job.rs crates/dblens-service/src/export/mod.rs crates/dblens-service/src/export/write.rs crates/dblens-service/src/fake.rs crates/dblens-service/src/lib.rs crates/dblens-service/src/service.rs crates/dblens-service/tests/live.rs
git commit -m "feat(service): 导出任务（CSV、xlsx、INSERT、JSON，另开连接读完整大字段）与运维查询列表" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 5: 接口与桌面原生能力

- 接口：`POST /exports`、`GET/DELETE /exports/{id}`、`GET /exports/{id}/file`（流式传输，传完或中断后删除临时文件；`Content-Disposition` 带 RFC 5987 文件名）、`GET /profiles/{id}/ops`（名称按请求语言）。
- `ApiConfig` 增加 `local_files`：桌面程序为 true（导出可以写到页面给的路径），`dblens-server` 为 false。各处构造都要补上这个字段。
- `dblens-api` 的 tokio 多开 `fs`、`io-util` 特性。
- 桌面：`save_dialog`（tauri-plugin-dialog 的另存为）、`reveal_in_folder`（Windows 用 `explorer /select,"路径"`，macOS `open -R`，其他 `xdg-open` 所在目录）；`build.rs` 和 `capabilities/main.json` 放行这两个命令。
- 接口说明补充导出和运维查询。

**Files:**
- Modify: `crates/dblens-api/Cargo.toml`
- Create: `crates/dblens-api/src/exports.rs`
- Modify: `crates/dblens-api/src/lib.rs`
- Modify: `crates/dblens-api/tests/api.rs`
- Modify: `crates/dblens-api/tests/library.rs`
- Modify: `crates/dblens-api/tests/live.rs`
- Modify: `crates/dblens-api/tests/settings.rs`
- Modify: `crates/dblens-api/tests/web.rs`
- Modify: `crates/dblens-desktop/build.rs`
- Modify: `crates/dblens-desktop/capabilities/main.json`
- Modify: `crates/dblens-desktop/src/main.rs`
- Modify: `crates/dblens-desktop/src/shell.rs`
- Modify: `crates/dblens-server/src/main.rs`
- Modify: `docs/notes/m0-4b-api.md`

**Interfaces:**
- Consumes：任务 4 的 `start_export`、`ExportManager`、`Service::ops`。
- Produces：
  - HTTP：见上；下载链接用 `?token=` 带令牌；
  - `ApiConfig { token, allowed_origins, local_files }`；
  - Tauri 命令 `save_dialog(default_name, filters: [{name, extensions}]) -> Option<String>`、`reveal_in_folder(path) -> Result<(), String>`。

- [ ] **Step 1: 修改 `crates/dblens-api/tests/api.rs`**

```diff
--- a/crates/dblens-api/tests/api.rs
+++ b/crates/dblens-api/tests/api.rs
@@ -24,7 +24,11 @@ fn app() -> (Router, Arc<Service>) {
     let log: Log = Arc::default();
     let registry = Arc::new(DriverRegistry::with_drivers(store.clone(), vec![Box::new(FakeDriver { log })]));
     let service = Arc::new(Service::with_parts(store, secrets, registry, Arc::new(DefaultPolicy)));
-    let config = ApiConfig { token: TOKEN.into(), allowed_origins: vec!["tauri://localhost".into()] };
+    let config = ApiConfig {
+        token: TOKEN.into(),
+        allowed_origins: vec!["tauri://localhost".into()],
+        local_files: false,
+    };
     (router(service.clone(), config), service)
 }
 
@@ -439,3 +443,79 @@ async fn websocket_events_follow_lang_param() {
         assert_eq!(confirm["reason"], want);
     }
 }
+
+// ---- 导出与运维查询（M1c3） ----
+
+#[tokio::test]
+async fn export_then_download() {
+    let (app, service) = app();
+    let dir = tempfile::tempdir().unwrap();
+    service.exports.set_dir(dir.path().join("exports"));
+    let (st, p) =
+        call(&app, "POST", "/api/profiles", Some(json!({"profile": profile_json("p"), "password": "pw"})))
+            .await;
+    assert_eq!(st, StatusCode::CREATED);
+    let pid = p["id"].as_str().unwrap();
+    let body = |sql: &str, path: Option<&str>| {
+        json!({"profile_id": pid, "source": {"sql": sql}, "format": "csv", "options": {"bom": false},
+               "path": path, "file_name": "订单.csv"})
+    };
+    // 不是查询、浏览器模式给了路径：400
+    let (st, e) = call(&app, "POST", "/api/exports", Some(body("DELETE FROM t", None))).await;
+    assert_eq!((st, e["kind"].clone()), (StatusCode::BAD_REQUEST, json!("invalid")));
+    let abs = dir.path().join("x.csv");
+    let (st, _) = call(&app, "POST", "/api/exports", Some(body("SELECT ROWS 3", abs.to_str()))).await;
+    assert_eq!(st, StatusCode::BAD_REQUEST);
+
+    let (st, created) = call(&app, "POST", "/api/exports", Some(body("SELECT ROWS 3", None))).await;
+    assert_eq!(st, StatusCode::CREATED, "{created}");
+    let id = created["id"].as_str().unwrap().to_string();
+    let status = loop {
+        let (_, s) = call(&app, "GET", &format!("/api/exports/{id}"), None).await;
+        if s["state"] != "running" {
+            break s;
+        }
+        tokio::time::sleep(Duration::from_millis(10)).await;
+    };
+    assert_eq!((status["state"].clone(), status["rows"].clone()), (json!("done"), json!(3)), "{status}");
+    // 浏览器的下载链接用查询参数带令牌
+    let req = Request::get(format!("/api/exports/{id}/file?token={TOKEN}")).body(Body::empty()).unwrap();
+    let resp = app.clone().oneshot(req).await.unwrap();
+    assert_eq!(resp.status(), StatusCode::OK);
+    let cd = resp.headers()[header::CONTENT_DISPOSITION].to_str().unwrap().to_string();
+    assert!(cd.ends_with("filename*=UTF-8''%E8%AE%A2%E5%8D%95.csv"), "{cd}");
+    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
+    assert_eq!(&bytes[..], b"n\n1\n2\n3\n");
+    assert_eq!(std::fs::read_dir(dir.path().join("exports")).unwrap().count(), 0, "下载后删除临时文件");
+    let (st, _) = call(&app, "GET", &format!("/api/exports/{id}"), None).await;
+    assert_eq!(st, StatusCode::NOT_FOUND);
+}
+
+#[tokio::test]
+async fn ops_follow_language() {
+    let (app, _) = app();
+    let (_, p) =
+        call(&app, "POST", "/api/profiles", Some(json!({"profile": profile_json("p"), "password": "pw"})))
+            .await;
+    let uri = format!("/api/profiles/{}/ops", p["id"].as_str().unwrap());
+    let get = |lang: &'static str| {
+        let req = Request::get(&uri)
+            .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
+            .header(header::ACCEPT_LANGUAGE, lang)
+            .body(Body::empty())
+            .unwrap();
+        let app = app.clone();
+        async move {
+            let resp = app.oneshot(req).await.unwrap();
+            assert_eq!(resp.status(), StatusCode::OK);
+            serde_json::from_slice::<Value>(&resp.into_body().collect().await.unwrap().to_bytes()).unwrap()
+        }
+    };
+    let zh = get("zh-CN").await;
+    assert_eq!(
+        (zh[0]["key"].clone(), zh[0]["name"].clone(), zh[0]["category"].clone()),
+        (json!("sessions"), json!("会话列表"), json!("session"))
+    );
+    assert!(zh[0]["sql"].as_str().unwrap().contains("pg_stat_activity"), "没连接过时按驱动选 PG 的模板");
+    assert_eq!(get("en").await[0]["name"], "Sessions");
+}
```

- [ ] **Step 2: 修改 `crates/dblens-api/tests/library.rs`**

```diff
--- a/crates/dblens-api/tests/library.rs
+++ b/crates/dblens-api/tests/library.rs
@@ -20,7 +20,7 @@ fn app() -> (Router, Arc<Store>) {
     let history = HistoryLog::inline(store.clone());
     let service =
         Arc::new(Service::with_history(store.clone(), secrets, registry, Arc::new(DefaultPolicy), history));
-    (router(service, ApiConfig { token: "t".into(), allowed_origins: vec![] }), store)
+    (router(service, ApiConfig { token: "t".into(), allowed_origins: vec![], local_files: false }), store)
 }
 
 async fn call(app: &Router, method: &str, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
```

- [ ] **Step 3: 修改 `crates/dblens-api/tests/live.rs`**

```diff
--- a/crates/dblens-api/tests/live.rs
+++ b/crates/dblens-api/tests/live.rs
@@ -51,7 +51,7 @@ async fn flow(env: &str) {
     secrets.init_vault("主密码").unwrap();
     let registry = Arc::new(DriverRegistry::new(store.clone()));
     let service = Arc::new(Service::with_parts(store, secrets, registry, Arc::new(DefaultPolicy)));
-    let app = router(service, ApiConfig { token: TOKEN.into(), allowed_origins: vec![] });
+    let app = router(service, ApiConfig { token: TOKEN.into(), allowed_origins: vec![], local_files: false });
 
     for option in LIB_OPTIONS {
         if let Some(lib) = params.options.get(option) {
```

- [ ] **Step 4: 修改 `crates/dblens-api/tests/settings.rs`**

```diff
--- a/crates/dblens-api/tests/settings.rs
+++ b/crates/dblens-api/tests/settings.rs
@@ -20,7 +20,7 @@ fn app() -> Router {
         vec![Box::new(FakeDriver { log: Arc::default() })],
     ));
     let service = Arc::new(Service::with_parts(store, secrets, registry, Arc::new(DefaultPolicy)));
-    router(service, ApiConfig { token: "t".into(), allowed_origins: vec![] })
+    router(service, ApiConfig { token: "t".into(), allowed_origins: vec![], local_files: false })
 }
 
 async fn call(app: &Router, method: &str, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
@@ -61,7 +61,7 @@ async fn only_ui_settings_are_visible() {
     let secrets = Arc::new(Secrets::with_backend(store.clone(), SecretBackend::Vault));
     let registry = Arc::new(DriverRegistry::with_drivers(store.clone(), vec![]));
     let service = Arc::new(Service::with_parts(store, secrets, registry, Arc::new(DefaultPolicy)));
-    let app = router(service, ApiConfig { token: "t".into(), allowed_origins: vec![] });
+    let app = router(service, ApiConfig { token: "t".into(), allowed_origins: vec![], local_files: false });
     assert_eq!(call(&app, "GET", "/api/settings", None).await.1, json!({ "ui.theme": "dark" }));
 }
 
```

- [ ] **Step 5: 修改 `crates/dblens-api/tests/web.rs`**

```diff
--- a/crates/dblens-api/tests/web.rs
+++ b/crates/dblens-api/tests/web.rs
@@ -19,7 +19,7 @@ fn app(dir: &std::path::Path) -> Router {
         vec![Box::new(FakeDriver { log: Arc::default() })],
     ));
     let service = Arc::new(Service::with_parts(store, secrets, registry, Arc::new(DefaultPolicy)));
-    let config = ApiConfig { token: "t".into(), allowed_origins: vec![] };
+    let config = ApiConfig { token: "t".into(), allowed_origins: vec![], local_files: false };
     with_web(router(service, config), dir.to_path_buf())
 }
 
```

- [ ] **Step 6: 修改 `crates/dblens-api/Cargo.toml`**

```diff
--- a/crates/dblens-api/Cargo.toml
+++ b/crates/dblens-api/Cargo.toml
@@ -14,7 +14,7 @@ dblens-service.workspace = true
 futures.workspace = true
 serde.workspace = true
 serde_json.workspace = true
-tokio.workspace = true
+tokio = { workspace = true, features = ["fs", "io-util"] }
 tracing.workspace = true
 uuid.workspace = true
 
```

- [ ] **Step 7: 新建 `crates/dblens-api/src/exports.rs`**

```rust
//! 导出与运维查询接口（M1c3 设计文档 3.1、5.2 节）。

use axum::Json;
use axum::body::{Body, Bytes};
use axum::extract::{Path, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use dblens_core::{Lang, current_lang};
use dblens_dialect::OpsCategory;
use dblens_service::{ExportRequest, ExportStatus};
use serde::Serialize;
use serde_json::{Value, json};
use tokio::io::AsyncReadExt;

use crate::AppState;
use crate::error::ApiResult;

/// 提交导出任务。只有桌面程序的本机服务（`ApiConfig::local_files`）接受保存路径。
pub async fn create(
    State(s): State<AppState>,
    Json(req): Json<ExportRequest>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    let id = s.service.start_export(req, s.config.local_files).await?;
    Ok((StatusCode::CREATED, Json(json!({ "id": id }))))
}

pub async fn status(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult<Json<ExportStatus>> {
    Ok(Json(s.service.exports.status(&id)?))
}

/// 进行中的任务取消；已结束的任务删除记录和临时文件。
pub async fn cancel_or_remove(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult<StatusCode> {
    s.service.exports.cancel_or_remove(&id).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// 传完（或连接中断）后删除临时文件。
struct RemoveOnDrop(std::path::PathBuf);

impl Drop for RemoveOnDrop {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// `Content-Disposition` 的文件名：RFC 5987 的 `filename*`，非 ASCII 字符按 UTF-8 百分号编码；另给一个 ASCII 的后备名。
fn disposition(name: &str) -> HeaderValue {
    let encoded: String = name
        .bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect();
    let ascii: String =
        name.chars().map(|c| if c.is_ascii_alphanumeric() || "-._".contains(c) { c } else { '_' }).collect();
    HeaderValue::from_str(&format!("attachment; filename=\"{ascii}\"; filename*=UTF-8''{encoded}"))
        .unwrap_or_else(|_| HeaderValue::from_static("attachment"))
}

/// 浏览器模式下载导出的文件（链接中用 `?token=` 带令牌）。下载后任务不再存在。
pub async fn file(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult<Response> {
    let (path, name) = s.service.exports.take_file(&id)?;
    let guard = RemoveOnDrop(path.clone());
    let f = tokio::fs::File::open(&path)
        .await
        .map_err(|e| dblens_core::Error::internal(dblens_core::msg!("export.write_failed", error = e)))?;
    // 文件和删除守卫一起放在流的状态里：流结束或被丢弃时先关闭文件，再删除
    let stream = futures::stream::unfold(Some((f, guard)), |state| async move {
        let (mut f, guard) = state?;
        let mut buf = vec![0u8; 64 * 1024];
        match f.read(&mut buf).await {
            Ok(0) => None,
            Ok(n) => {
                buf.truncate(n);
                Some((Ok::<_, std::io::Error>(Bytes::from(buf)), Some((f, guard))))
            }
            Err(e) => Some((Err(e), None)),
        }
    });
    let mut resp = Body::from_stream(stream).into_response();
    resp.headers_mut().insert(header::CONTENT_TYPE, HeaderValue::from_static("application/octet-stream"));
    resp.headers_mut().insert(header::CONTENT_DISPOSITION, disposition(&name));
    Ok(resp)
}

#[derive(Serialize)]
pub struct OpsItem {
    key: &'static str,
    /// 按请求的语言。
    name: &'static str,
    category: OpsCategory,
    sql: &'static str,
}

/// 连接可用的运维查询。
pub async fn ops(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult<Json<Vec<OpsItem>>> {
    let en = current_lang() == Lang::En;
    let items = s
        .service
        .ops(&id)?
        .iter()
        .map(|q| OpsItem {
            key: q.key.as_str(),
            name: if en { q.name_en.as_str() } else { q.name_zh.as_str() },
            category: q.category,
            sql: q.sql.as_str(),
        })
        .collect();
    Ok(Json(items))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disposition_encodes_non_ascii() {
        assert_eq!(
            disposition("订单 2026.csv").to_str().unwrap(),
            "attachment; filename=\"___2026.csv\"; filename*=UTF-8''%E8%AE%A2%E5%8D%95%202026.csv"
        );
    }
}
```

- [ ] **Step 8: 修改 `crates/dblens-api/src/lib.rs`**

```diff
--- a/crates/dblens-api/src/lib.rs
+++ b/crates/dblens-api/src/lib.rs
@@ -3,6 +3,7 @@
 
 mod auth;
 mod error;
+mod exports;
 mod lang;
 mod routes;
 mod web;
@@ -27,6 +28,9 @@ pub struct ApiConfig {
     pub token: String,
     /// 除同源外还允许的 Origin，如桌面模式的 `tauri://localhost`、前端开发服务器 `http://localhost:5173`。
     pub allowed_origins: Vec<String>,
+    /// 页面和服务在同一台机器上（桌面程序）：导出可以直接写到页面给出的路径。
+    /// 浏览器模式（`dblens-server`）页面可能在另一台机器上，为 false，导出一律写临时文件后下载。
+    pub local_files: bool,
 }
 
 /// 生成随机访问令牌（64 个十六进制字符）。
@@ -76,6 +80,10 @@ pub fn router(service: Arc<Service>, config: ApiConfig) -> Router {
         .route("/profiles/{id}/schemas/{schema}/generate", get(routes::generate))
         .route("/profiles/{id}/schemas/{schema}/relations", get(routes::relations))
         .route("/profiles/{id}/schemas/{schema}/data-sql", post(routes::data_sql))
+        .route("/profiles/{id}/ops", get(exports::ops))
+        .route("/exports", post(exports::create))
+        .route("/exports/{id}", get(exports::status).delete(exports::cancel_or_remove))
+        .route("/exports/{id}/file", get(exports::file))
         .route("/sessions", get(routes::sessions).post(routes::open_session))
         .route("/sessions/{id}", get(routes::session).delete(routes::close_session))
         .route("/sessions/{id}/autocommit", post(routes::set_autocommit))
```

- [ ] **Step 9: 修改 `crates/dblens-desktop/build.rs`**

```diff
--- a/crates/dblens-desktop/build.rs
+++ b/crates/dblens-desktop/build.rs
@@ -1,6 +1,11 @@
 fn main() {
     // 为页面可调用的命令生成 allow-* 权限，供 capabilities/main.json 引用
-    let manifest =
-        tauri_build::AppManifest::new().commands(&["shell_info", "window_action", "set_auto_check_update"]);
+    let manifest = tauri_build::AppManifest::new().commands(&[
+        "shell_info",
+        "window_action",
+        "set_auto_check_update",
+        "save_dialog",
+        "reveal_in_folder",
+    ]);
     tauri_build::try_build(tauri_build::Attributes::new().app_manifest(manifest)).expect("tauri-build 失败");
 }
```

- [ ] **Step 10: 修改 `crates/dblens-desktop/capabilities/main.json`**

```diff
--- a/crates/dblens-desktop/capabilities/main.json
+++ b/crates/dblens-desktop/capabilities/main.json
@@ -1,7 +1,7 @@
 {
   "$schema": "../gen/schemas/desktop-schema.json",
   "identifier": "main",
-  "description": "主窗口：页面来自进程内接口服务（http://127.0.0.1:随机端口）。只放行自绘标题栏需要的三个命令和事件监听，业务仍走 HTTP 和 WebSocket",
+  "description": "主窗口：页面来自进程内接口服务（http://127.0.0.1:随机端口）。只放行自绘标题栏需要的三个命令、导出用的另存为对话框和在文件夹中显示，以及事件监听，业务仍走 HTTP 和 WebSocket",
   "windows": ["main"],
   "local": false,
   "remote": {
@@ -12,6 +12,8 @@
     "core:event:allow-unlisten",
     "allow-shell-info",
     "allow-window-action",
-    "allow-set-auto-check-update"
+    "allow-set-auto-check-update",
+    "allow-save-dialog",
+    "allow-reveal-in-folder"
   ]
 }
```

- [ ] **Step 11: 修改 `crates/dblens-desktop/src/main.rs`**

```diff
--- a/crates/dblens-desktop/src/main.rs
+++ b/crates/dblens-desktop/src/main.rs
@@ -53,10 +53,15 @@ fn run(data_dir: PathBuf) -> Result<(), String> {
         .invoke_handler(tauri::generate_handler![
             shell::shell_info,
             shell::window_action,
-            shell::set_auto_check_update
+            shell::set_auto_check_update,
+            shell::save_dialog,
+            shell::reveal_in_folder
         ])
         .setup(move |app| {
-            let mut api = router(service, ApiConfig { token: token.clone(), allowed_origins: Vec::new() });
+            let mut api = router(
+                service,
+                ApiConfig { token: token.clone(), allowed_origins: Vec::new(), local_files: true },
+            );
             match web_dir(app.path().resource_dir().ok()) {
                 Some(dir) => {
                     tracing::info!("前端页面目录 {}", dir.display());
```

- [ ] **Step 12: 修改 `crates/dblens-desktop/src/shell.rs`**

```diff
--- a/crates/dblens-desktop/src/shell.rs
+++ b/crates/dblens-desktop/src/shell.rs
@@ -1,14 +1,17 @@
 //! 页面可以调用的原生能力（M1 设计文档 2.2 节）。
 //!
 //! 业务仍然只走 HTTP 和 WebSocket；自绘标题栏需要的窗口操作，以及原本在原生菜单里的几项（置顶、恢复默认位置、
-//! 检查更新、关于、退出），通过这里的三个命令提供。`capabilities/main.json` 只放行这三个命令和事件监听，
-//! 且只对 `http://127.0.0.1:*` 上的本地页面生效。
+//! 检查更新、关于、退出），通过这里的三个命令提供；导出（M1c3）另有"另存为"对话框和"在文件夹中显示"两个命令。
+//! `capabilities/main.json` 只放行这五个命令和事件监听，且只对 `http://127.0.0.1:*` 上的本地页面生效。
 //!
 //! 窗口状态变化（最大化、置顶、自动检查更新）后，通过事件 [`EVENT`] 把新的 [`ShellInfo`] 推给页面。
 
+use std::path::Path;
+
 use dblens_core::msg;
 use serde::{Deserialize, Serialize};
 use tauri::{AppHandle, Emitter, Manager};
+use tauri_plugin_dialog::DialogExt;
 
 use crate::{update, window};
 
@@ -104,6 +107,51 @@ pub fn set_auto_check_update(app: AppHandle, on: bool) {
     notify(&app);
 }
 
+/// "另存为"对话框的文件类型。
+#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
+pub struct FileFilter {
+    pub name: String,
+    pub extensions: Vec<String>,
+}
+
+/// 系统"另存为"对话框（导出时选择保存位置）。用户取消时返回 None。
+#[tauri::command]
+pub async fn save_dialog(app: AppHandle, default_name: String, filters: Vec<FileFilter>) -> Option<String> {
+    let mut dialog = app.dialog().file().set_file_name(default_name);
+    for f in &filters {
+        let exts: Vec<&str> = f.extensions.iter().map(String::as_str).collect();
+        dialog = dialog.add_filter(&f.name, &exts);
+    }
+    // 在异步命令的线程上等待对话框，不阻塞主线程
+    dialog.blocking_save_file().and_then(|p| p.into_path().ok()).map(|p| p.display().to_string())
+}
+
+/// 在系统的文件管理器中显示文件（导出完成后"打开所在文件夹"）。
+#[tauri::command]
+pub fn reveal_in_folder(path: String) -> Result<(), String> {
+    reveal(Path::new(&path)).map_err(|e| e.to_string())
+}
+
+#[cfg(target_os = "windows")]
+fn reveal(path: &Path) -> std::io::Result<()> {
+    use std::os::windows::process::CommandExt;
+    // explorer 要求 /select, 后面的路径自己加引号，不能交给标准库按参数转义
+    std::process::Command::new("explorer").raw_arg(format!("/select,\"{}\"", path.display())).spawn()?;
+    Ok(())
+}
+
+#[cfg(target_os = "macos")]
+fn reveal(path: &Path) -> std::io::Result<()> {
+    std::process::Command::new("open").arg("-R").arg(path).spawn()?;
+    Ok(())
+}
+
+#[cfg(not(any(target_os = "windows", target_os = "macos")))]
+fn reveal(path: &Path) -> std::io::Result<()> {
+    std::process::Command::new("xdg-open").arg(path.parent().unwrap_or(path)).spawn()?;
+    Ok(())
+}
+
 #[cfg(test)]
 mod tests {
     use super::*;
```

- [ ] **Step 13: 修改 `crates/dblens-server/src/main.rs`**

```diff
--- a/crates/dblens-server/src/main.rs
+++ b/crates/dblens-server/src/main.rs
@@ -132,7 +132,8 @@ async fn run(args: Args) -> Result<(), String> {
             }
         });
     }
-    let mut app = router(service, ApiConfig { token, allowed_origins: args.allowed_origins });
+    let mut app =
+        router(service, ApiConfig { token, allowed_origins: args.allowed_origins, local_files: false });
     if let Some(dir) = web_dir {
         app = with_web(app, dir);
     }
```

- [ ] **Step 14: 修改 `docs/notes/m0-4b-api.md`**

```diff
--- a/docs/notes/m0-4b-api.md
+++ b/docs/notes/m0-4b-api.md
@@ -99,6 +99,7 @@
 | GET | `…/schemas/{schema}/relations?table=` | 表的外键关系（M1c2 加入）：`[{direction: out\|in, name, schema, table, local_columns, other_columns}]`，`in` 只含同一 schema 中引用它的表 |
 | POST | `…/schemas/{schema}/data-sql` | 数据标签的 SQL（M1c2 加入），`{"object", "filter"?, "order"?: {"column", "desc"}, "count"?}` → `{"sql": "…"}`；`filter` 为空白时不加 WHERE，`count` 为 true 时生成 `SELECT COUNT(*)` |
 | POST | `/profiles/{id}/refresh` | 刷新元数据缓存，`{"schema": "…" \| null}` |
+| GET | `/profiles/{id}/ops` | 运维查询（M1c3）：`[{key, name, category, sql}]`，`name` 按请求的语言，`category` 为 `session` / `lock` / `storage` / `sql`。按连接上次识别出的变体选模板，没连接过时按驱动；不连接数据库 |
 
 ### 会话
 
@@ -138,6 +139,21 @@
 
 每条：`{id, parent_id, kind, name, sql, note, driver, variant, position, created_at, updated_at}`。收藏必须有 SQL，文件夹没有；上级必须是文件夹；不能把文件夹移到它自己或它的下级中（都返回 `invalid`）。
 
+### 导出（M1c3 加入）
+
+| 方法 | 路径 | 说明 |
+|---|---|---|
+| POST | `/exports` | 提交导出任务，返回 201 和 `{"id"}`。请求体：`{profile_id, source, format, options?, path?, file_name}` |
+| GET | `/exports/{id}` | `{id, state, rows, warnings: {lob_truncated, cell_truncated}, error, file_name, path}`；`state` 为 `running` / `done` / `failed` / `cancelled`，`error` 是错误对象 |
+| DELETE | `/exports/{id}` | 进行中时取消；已结束时删除任务记录和临时文件。204 |
+| GET | `/exports/{id}/file?token=…` | 浏览器模式下载（`Content-Disposition` 带文件名）。下载后删除临时文件，任务不再存在 |
+
+- `source`：`{"sql": "…"}`（必须是单条查询语句，否则 400）或 `{"data": {schema, object, filter?, order?}}`（由数据标签的 SQL 生成）。
+- `format`：`csv` / `xlsx` / `insert` / `json`。`options`：`delimiter`（`,` / `	` / `;`）、`header`、`bom`、`null_text`、`table`（INSERT 的目标表）、`rows_per_insert`。
+- `path`：只有桌面程序的本机服务（`ApiConfig::local_files`）接受，必须是绝对路径且所在目录存在；浏览器模式给出时 400。不给时写到数据目录的 `exports/` 下，完成后下载。
+- 同时进行的任务最多 4 个，超出时 409（`busy`）。已结束的任务保留 1 小时。
+- 导出另开一个连接执行，大字段每个值最多读 256 MB；照常记入执行历史。
+
 ## WebSocket：`GET /api/sessions/{id}/ws?token=…&lang=…`
 
 `lang` 可选，决定事件中文字的语言（见"语言"一节）。
```

- [ ] **Step 15: 检查**

```bash
export CARGO_INCREMENTAL=0
cargo fmt --all
cargo clippy --config "source.mirror.registry='sparse+https://rsproxy.cn/index/'" --workspace --all-targets -- -D warnings
DBLENS_LANG=en cargo test --config "source.mirror.registry='sparse+https://rsproxy.cn/index/'" --workspace
```

Expected: 全部测试通过（接口新增 `export_then_download`、`ops_follow_language` 和文件名编码的单元测试）。

- [ ] **Step 16: 提交**

```bash
git add crates/dblens-api/Cargo.toml crates/dblens-api/src/exports.rs crates/dblens-api/src/lib.rs crates/dblens-api/tests/api.rs crates/dblens-api/tests/library.rs crates/dblens-api/tests/live.rs crates/dblens-api/tests/settings.rs crates/dblens-api/tests/web.rs crates/dblens-desktop/build.rs crates/dblens-desktop/capabilities/main.json crates/dblens-desktop/src/main.rs crates/dblens-desktop/src/shell.rs crates/dblens-server/src/main.rs docs/notes/m0-4b-api.md
git commit -m "feat(api): 导出任务与运维查询接口；桌面版的另存为对话框和在文件夹中显示" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 6: 前端：导出

- 接口客户端：`startExport`、`exportStatus`、`cancelExport`、`exportFileUrl`、`ops`；`shared/shell.ts` 增加 `saveDialog`、`revealInFolder`。
- 导出对话框（挂在外壳上）：选格式，CSV 有分隔符、NULL 写法、表头、BOM，INSERT 有目标表和每条语句的行数；标签有未提交的事务时提示。桌面版点导出后弹"另存为"。
- 提交后右下角通知轮询进度（每秒一次），可取消；完成后显示行数和警告，桌面版有"打开所在文件夹"，浏览器模式用带令牌的链接触发下载。
- 入口：结果面板的结果集标签右侧（执行计划的结果除外）导出那条语句的原文；数据标签工具栏导出当前对象、过滤和排序，默认目标表为 `schema.对象`。
- 词条 `results.export.*`。

**Files:**
- Modify: `web/src/app/Shell.tsx`
- Modify: `web/src/features/data/DataView.tsx`
- Modify: `web/src/features/editor/QueryView.tsx`
- Create: `web/src/features/export/ExportDialog.tsx`
- Create: `web/src/features/export/dialog.ts`
- Create: `web/src/features/export/options.test.ts`
- Create: `web/src/features/export/options.ts`
- Modify: `web/src/features/results/ResultsPanel.tsx`
- Modify: `web/src/locales/en/results.json`
- Modify: `web/src/locales/zh-CN/results.json`
- Modify: `web/src/shared/api.ts`
- Modify: `web/src/shared/shell.ts`

**Interfaces:**
- Consumes：任务 5 的接口和 Tauri 命令。
- Produces：
  - `shared/api.ts`：`ExportFormat`、`ExportOptions`、`ExportSource`、`ExportRequest`、`ExportStatus`、`OpsQuery`；
  - `features/export/options.ts`：`FORMATS`、`EXTENSION`、`defaultOptions(table?)`、`defaultFileName(base, format, now?)`、`fileFilter`、`baseName`；
  - `features/export/dialog.ts`：`useExportDialog`、`ExportDraft { profileId, source, baseName, table?, inTransaction }`；
  - `ResultsPanel` 新增可选的 `onExport(stmt)`。

- [ ] **Step 1: 新建 `web/src/features/export/options.test.ts`**

```ts
import { describe, expect, it } from 'vitest'

import { baseName, defaultFileName, defaultOptions, fileFilter } from './options'

describe('导出选项', () => {
  it('默认选项：逗号分隔、带表头和 BOM，NULL 写成空，INSERT 每条一行', () => {
    expect(defaultOptions()).toEqual({
      delimiter: ',',
      header: true,
      bom: true,
      null_text: '',
      table: 'table_name',
      rows_per_insert: 1,
    })
    expect(defaultOptions('public.orders').table).toBe('public.orders')
  })

  it('默认文件名：对象名加时间，去掉不能用的字符', () => {
    const now = new Date(2026, 9, 26, 9, 5, 3)
    expect(defaultFileName('orders', 'csv', now)).toBe('orders_20261026_090503.csv')
    expect(defaultFileName('订单 明细', 'insert', now)).toBe('订单_明细_20261026_090503.sql')
    expect(defaultFileName('a/b:c', 'xlsx', now)).toBe('a_b_c_20261026_090503.xlsx')
    expect(defaultFileName('', 'json', now)).toBe('export_20261026_090503.json')
  })

  it('另存为的文件类型和路径中的文件名', () => {
    expect(fileFilter('insert', 'INSERT')).toEqual({ name: 'INSERT', extensions: ['sql'] })
    expect(baseName('D:\\导出\\a.csv')).toBe('a.csv')
    expect(baseName('/home/u/a.json')).toBe('a.json')
  })
})
```

- [ ] **Step 2: 修改 `web/src/app/Shell.tsx`**

```diff
--- a/web/src/app/Shell.tsx
+++ b/web/src/app/Shell.tsx
@@ -11,6 +11,7 @@ import { Group, Panel, type PanelImperativeHandle, Separator, useDefaultLayout }
 import { useQueryActions } from '../features/editor/actions'
 import { type EditorCommand, sendCommand } from '../features/editor/commands'
 import { useTabs } from '../features/editor/tabs'
+import ExportDialog from '../features/export/ExportDialog'
 import FavoriteDialog from '../features/favorites/FavoriteDialog'
 import FavoritesPanel from '../features/favorites/FavoritesPanel'
 import HistoryPanel from '../features/history/HistoryPanel'
@@ -155,6 +156,7 @@ export default function Shell() {
       </div>
       <StatusBar />
       <FavoriteDialog />
+      <ExportDialog />
     </div>
   )
 }
```

- [ ] **Step 3: 修改 `web/src/features/data/DataView.tsx`**

```diff
--- a/web/src/features/data/DataView.tsx
+++ b/web/src/features/data/DataView.tsx
@@ -2,7 +2,7 @@
 // 工具栏：连接和 schema（只显示）、WHERE 过滤、刷新、停止、统计行数；表格沿用查询标签的结果面板，点列头排序。
 // SELECT 由后端按方言生成（data-sql），经这个标签自己的会话执行（沿用 runner：续取、取消、行数上限）。
 
-import { IconPlayerStop, IconRefresh, IconSum } from '@tabler/icons-react'
+import { IconDownload, IconPlayerStop, IconRefresh, IconSum } from '@tabler/icons-react'
 import { Alert, App, Button, Input, Tooltip } from 'antd'
 import { useEffect, useState } from 'react'
 import { useTranslation } from 'react-i18next'
@@ -12,6 +12,7 @@ import DbIcon from '../../shared/DbIcon'
 import type { Profile } from '../../shared/api'
 import ResultsPanel from '../results/ResultsPanel'
 import { runnerFor, useTabRuntime } from '../editor/runner'
+import { useExportDialog } from '../export/dialog'
 import { type DataTarget, type QueryTab, useTabs } from '../editor/tabs'
 import { countRows } from './count'
 import { nextOrder } from './sort'
@@ -97,6 +98,22 @@ export default function DataView({ tab, profiles }: { tab: QueryTab; profiles: P
         <Button size="small" icon={<IconSum size={14} />} loading={counting} disabled={!profile} onClick={() => void count()}>
           {t('editor.data.count')}
         </Button>
+        <Button
+          size="small"
+          icon={<IconDownload size={14} />}
+          disabled={!profile}
+          onClick={() =>
+            useExportDialog.getState().open({
+              profileId: tab.profileId,
+              source: { data: { schema: data.schema, object: data.object, filter: data.filter, order: data.order } },
+              baseName: data.object,
+              table: `${data.schema}.${data.object}`,
+              inTransaction: false,
+            })
+          }
+        >
+          {t('results.export.button')}
+        </Button>
         {total !== null && <span className="muted">{t('editor.data.total', { count: Number(total) })}</span>}
       </div>
       {!profile && <Alert type="error" banner message={t('editor.query.profileDeleted')} />}
```

- [ ] **Step 4: 修改 `web/src/features/editor/QueryView.tsx`**

```diff
--- a/web/src/features/editor/QueryView.tsx
+++ b/web/src/features/editor/QueryView.tsx
@@ -11,6 +11,7 @@ import { useApi } from '../../app/context'
 import { type Profile, errorText } from '../../shared/api'
 import { layoutStorage } from '../../shared/settings'
 import ResultsPanel from '../results/ResultsPanel'
+import { useExportDialog } from '../export/dialog'
 import { useFavoriteDialog } from '../favorites/dialog'
 import { sqlDialect } from '../sql/dialects'
 import { defaultSchema, metaKey, metaReader } from '../sql/metadata'
@@ -202,7 +203,20 @@ export default function QueryView({ tab, profiles }: { tab: QueryTab; profiles:
         </Panel>
         <Separator className="splitter-h" />
         <Panel id="results" defaultSize="40%" minSize={60}>
-          <ResultsPanel rt={rt} onFetchMore={() => runner.fetchMore()} />
+          <ResultsPanel
+            rt={rt}
+            onFetchMore={() => runner.fetchMore()}
+            onExport={(i) => {
+              // 导出这条语句的原文（取自执行时的全文）
+              const s = rt.exec.statements[i]
+              useExportDialog.getState().open({
+                profileId: tab.profileId,
+                source: { sql: rt.source.slice(s.start, s.end) },
+                baseName: 'export',
+                inTransaction: !!rt.session?.in_transaction,
+              })
+            }}
+          />
         </Panel>
       </Group>
       <ConfirmDialog
```

- [ ] **Step 5: 新建 `web/src/features/export/ExportDialog.tsx`**

```tsx
// 导出对话框（M1c3 设计文档 4.2 节）：选格式和选项；桌面版再弹系统"另存为"。提交后右下角的通知显示进度、可取消，
// 完成后桌面版可以打开所在文件夹，浏览器模式自动下载。通知在对话框关闭后继续轮询（对话框挂在外壳上，不卸载）。

import { Alert, App, Button, Checkbox, Form, Input, InputNumber, Modal, Radio, Select } from 'antd'
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { useApi } from '../../app/context'
import { type ExportFormat, type ExportOptions, errorText } from '../../shared/api'
import { desktop } from '../../shared/shell'
import { withVault } from '../connections/vault'
import { useExportDialog } from './dialog'
import { FORMATS, baseName, defaultFileName, defaultOptions, fileFilter } from './options'

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms))

export default function ExportDialog() {
  const api = useApi()
  const { t } = useTranslation()
  const { notification } = App.useApp()
  const { draft, close } = useExportDialog()
  // 格式和选项在两次导出之间保留（目标表除外）
  const [format, setFormat] = useState<ExportFormat>('csv')
  const [options, setOptions] = useState<ExportOptions>(defaultOptions())
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  useEffect(() => {
    if (!draft) return
    setOptions((o) => ({ ...o, table: draft.table ?? 'table_name' }))
    setError(null)
  }, [draft])
  const set = (patch: Partial<ExportOptions>) => setOptions((o) => ({ ...o, ...patch }))

  /** 轮询任务直到结束，用同一个 key 的通知显示进度和结果。 */
  const track = async (id: string, name: string) => {
    const key = `export-${id}`
    await sleep(300)
    for (;;) {
      const st = await api.exportStatus(id).catch(() => null)
      if (!st) return
      if (st.state === 'running') {
        notification.open({
          key,
          message: t('results.export.running', { name }),
          description: t('results.export.progress', { count: st.rows }),
          duration: 0,
          actions: (
            <Button size="small" onClick={() => void api.cancelExport(id)}>
              {t('results.export.cancel')}
            </Button>
          ),
        })
        await sleep(1000)
        continue
      }
      if (st.state === 'done') {
        const lines = [t('results.export.doneRows', { count: st.rows })]
        if (st.warnings.lob_truncated) lines.push(t('results.export.lobTruncated', { count: st.warnings.lob_truncated }))
        if (st.warnings.cell_truncated) lines.push(t('results.export.cellTruncated', { count: st.warnings.cell_truncated }))
        const path = st.path
        notification.success({
          key,
          message: t('results.export.done', { name }),
          description: lines.map((l) => <div key={l}>{l}</div>),
          duration: path ? 0 : 8,
          actions:
            desktop && path ? (
              <Button size="small" onClick={() => void desktop?.revealInFolder(path)}>
                {t('results.export.reveal')}
              </Button>
            ) : undefined,
        })
        if (!path) {
          // 浏览器模式：下载（下载后服务端删除临时文件和任务）
          const a = document.createElement('a')
          a.href = api.exportFileUrl(id)
          a.download = name
          a.click()
        }
      } else if (st.state === 'failed') {
        notification.error({
          key,
          message: t('results.export.failed', { name }),
          description: st.error ? errorText(st.error) : null,
          duration: 0,
        })
        void api.cancelExport(id)
      } else {
        notification.info({ key, message: t('results.export.cancelled', { name }), duration: 4 })
        void api.cancelExport(id)
      }
      return
    }
  }

  const submit = async () => {
    if (!draft) return
    let name = defaultFileName(draft.baseName, format)
    let path: string | null = null
    if (desktop) {
      path = await desktop.saveDialog(name, [fileFilter(format, t(`results.export.formats.${format}`))])
      if (!path) return
      name = baseName(path)
    }
    setBusy(true)
    setError(null)
    try {
      const req = { profile_id: draft.profileId, source: draft.source, format, options, path, file_name: name }
      const id = await withVault(api, () => api.startExport(req))
      close()
      void track(id, name)
    } catch (e) {
      setError((e as Error).message)
    } finally {
      setBusy(false)
    }
  }

  return (
    <Modal
      open={draft !== null}
      title={t('results.export.title')}
      okText={t('results.export.ok')}
      confirmLoading={busy}
      onOk={() => void submit()}
      onCancel={close}
      destroyOnHidden
    >
      {draft?.inTransaction && <Alert type="warning" showIcon message={t('results.export.uncommitted')} style={{ marginBottom: 12 }} />}
      <Form layout="vertical">
        <Form.Item label={t('results.export.format')}>
          <Radio.Group
            value={format}
            onChange={(e) => setFormat(e.target.value as ExportFormat)}
            options={FORMATS.map((f) => ({ value: f, label: t(`results.export.formats.${f}`) }))}
          />
        </Form.Item>
        {format === 'csv' && (
          <>
            <Form.Item label={t('results.export.delimiter')}>
              <Select
                value={options.delimiter}
                onChange={(delimiter) => set({ delimiter })}
                style={{ width: 160 }}
                options={[
                  { value: ',', label: t('results.export.delimiters.comma') },
                  { value: '\t', label: t('results.export.delimiters.tab') },
                  { value: ';', label: t('results.export.delimiters.semicolon') },
                ]}
              />
            </Form.Item>
            <Form.Item label={t('results.export.nullText')}>
              <Select
                value={options.null_text}
                onChange={(null_text) => set({ null_text })}
                style={{ width: 160 }}
                options={[
                  { value: '', label: t('results.export.nullEmpty') },
                  { value: 'NULL', label: 'NULL' },
                ]}
              />
            </Form.Item>
            <Checkbox checked={options.header} onChange={(e) => set({ header: e.target.checked })}>
              {t('results.export.header')}
            </Checkbox>
            <br />
            <Checkbox checked={options.bom} onChange={(e) => set({ bom: e.target.checked })}>
              {t('results.export.bom')}
            </Checkbox>
          </>
        )}
        {format === 'insert' && (
          <>
            <Form.Item label={t('results.export.table')}>
              <Input value={options.table} onChange={(e) => set({ table: e.target.value })} />
            </Form.Item>
            <Form.Item label={t('results.export.rowsPerInsert')}>
              <InputNumber
                min={1}
                max={1000}
                value={options.rows_per_insert}
                onChange={(v) => set({ rows_per_insert: v ?? 1 })}
              />
            </Form.Item>
          </>
        )}
        {format === 'xlsx' && <div className="muted">{t('results.export.xlsxNote')}</div>}
        {format === 'json' && <div className="muted">{t('results.export.jsonNote')}</div>}
      </Form>
      {error && <Alert type="error" showIcon message={error} style={{ marginTop: 12 }} />}
    </Modal>
  )
}
```

- [ ] **Step 6: 新建 `web/src/features/export/dialog.ts`**

```ts
// 导出对话框的状态：结果面板和数据标签都可以打开它。

import { create } from 'zustand'

import type { ExportSource } from '../../shared/api'

export interface ExportDraft {
  profileId: string
  source: ExportSource
  /** 默认文件名的前半部分（对象名，查询结果为 export）。 */
  baseName: string
  /** INSERT 的默认目标表。 */
  table?: string
  /** 标签有未提交的事务：提示导出看不到这些修改。 */
  inTransaction: boolean
}

interface DialogState {
  draft: ExportDraft | null
  open(draft: ExportDraft): void
  close(): void
}

export const useExportDialog = create<DialogState>()((set) => ({
  draft: null,
  open: (draft) => set({ draft }),
  close: () => set({ draft: null }),
}))
```

- [ ] **Step 7: 新建 `web/src/features/export/options.ts`**

```ts
// 导出对话框的纯逻辑（M1c3 设计文档 3.4、4.2 节）：默认选项、默认文件名、另存为对话框的文件类型。

import type { ExportFormat, ExportOptions } from '../../shared/api'

export const FORMATS: ExportFormat[] = ['csv', 'xlsx', 'insert', 'json']

export const EXTENSION: Record<ExportFormat, string> = { csv: 'csv', xlsx: 'xlsx', insert: 'sql', json: 'json' }

/** 默认选项；`table` 为 INSERT 的目标表（数据标签为对象的限定名，查询结果为占位的 `table_name`）。 */
export function defaultOptions(table = 'table_name'): ExportOptions {
  return { delimiter: ',', header: true, bom: true, null_text: '', table, rows_per_insert: 1 }
}

/** 文件名里不能出现的字符换成下划线。 */
function safe(name: string): string {
  const s = name.replace(/[\\/:*?"<>|\s]+/g, '_').replace(/^_+|_+$/g, '')
  return s || 'export'
}

/** 默认文件名：`对象名_年月日_时分秒.扩展名`。 */
export function defaultFileName(base: string, format: ExportFormat, now = new Date()): string {
  const p = (n: number) => String(n).padStart(2, '0')
  const stamp = `${now.getFullYear()}${p(now.getMonth() + 1)}${p(now.getDate())}_${p(now.getHours())}${p(now.getMinutes())}${p(now.getSeconds())}`
  return `${safe(base)}_${stamp}.${EXTENSION[format]}`
}

/** 另存为对话框的文件类型。 */
export function fileFilter(format: ExportFormat, label: string): { name: string; extensions: string[] } {
  return { name: label, extensions: [EXTENSION[format]] }
}

/** 路径中的文件名部分（Windows 和 Unix 的分隔符都认）。 */
export function baseName(path: string): string {
  return path.split(/[\\/]/).pop() ?? path
}
```

- [ ] **Step 8: 修改 `web/src/features/results/ResultsPanel.tsx`**

```diff
--- a/web/src/features/results/ResultsPanel.tsx
+++ b/web/src/features/results/ResultsPanel.tsx
@@ -1,8 +1,8 @@
 // 结果面板（设计稿第 2 部分）：结果集、失败的语句、消息各一个标签。每次执行后自动选中第一个结果集，
-// 没有结果集时选中第一个失败的语句，再没有就选"消息"。
+// 没有结果集时选中第一个失败的语句，再没有就选"消息"。结果集标签右侧有"导出"按钮（M1c3，执行计划的结果除外）。
 
-import { IconAlertCircle, IconBinaryTree, IconMessage, IconTable } from '@tabler/icons-react'
-import { Spin } from 'antd'
+import { IconAlertCircle, IconBinaryTree, IconDownload, IconMessage, IconTable } from '@tabler/icons-react'
+import { Button, Spin } from 'antd'
 import { useEffect, useState } from 'react'
 import { useTranslation } from 'react-i18next'
 
@@ -18,6 +18,8 @@ export interface ResultsPanelProps {
   /** 数据标签的排序（见 ResultGrid）。 */
   sort?: Order
   onSort?(column: string): void
+  /** 导出第 `stmt` 条语句的结果；不给时不显示导出按钮。 */
+  onExport?(stmt: number): void
 }
 
 function TabIcon({ tab, explain }: { tab: ResultTab; explain: boolean }) {
@@ -26,7 +28,7 @@ function TabIcon({ tab, explain }: { tab: ResultTab; explain: boolean }) {
   return explain ? <IconBinaryTree size={16} /> : <IconTable size={16} />
 }
 
-export default function ResultsPanel({ rt, onFetchMore, sort, onSort }: ResultsPanelProps) {
+export default function ResultsPanel({ rt, onFetchMore, sort, onSort, onExport }: ResultsPanelProps) {
   const { t } = useTranslation()
   const { exec, explain, capped, running } = rt
   const tabs = resultTabs(exec, explain, capped)
@@ -57,6 +59,17 @@ export default function ResultsPanel({ rt, onFetchMore, sort, onSort }: ResultsP
         ))}
         <span className="grow" />
         {running && <Spin size="small" style={{ marginRight: 8 }} />}
+        {onExport && current.kind === 'result' && !explain && (
+          <Button
+            type="text"
+            size="small"
+            icon={<IconDownload size={16} />}
+            onClick={() => onExport(current.stmt)}
+            style={{ marginRight: 4 }}
+          >
+            {t('results.export.button')}
+          </Button>
+        )}
       </div>
       <div className="results-body">
         {current.kind === 'result' && (
```

- [ ] **Step 9: 修改 `web/src/locales/en/results.json`**

```diff
--- a/web/src/locales/en/results.json
+++ b/web/src/locales/en/results.json
@@ -46,5 +46,46 @@
     "copyFailed": "Copy failed",
     "text": "Text",
     "hex": "Hex"
+  },
+  "export": {
+    "button": "Export",
+    "title": "Export",
+    "format": "Format",
+    "formats": {
+      "csv": "CSV",
+      "xlsx": "Excel (xlsx)",
+      "insert": "INSERT statements",
+      "json": "JSON"
+    },
+    "delimiter": "Delimiter",
+    "delimiters": {
+      "comma": "Comma",
+      "tab": "Tab",
+      "semicolon": "Semicolon"
+    },
+    "header": "Column names in the first row",
+    "bom": "Add a UTF-8 BOM (so Excel opens non-ASCII text correctly)",
+    "nullText": "Write NULL as",
+    "nullEmpty": "Empty",
+    "table": "Target table",
+    "rowsPerInsert": "Rows per statement",
+    "xlsxNote": "The first row holds column names; rows beyond 1048575 continue on the next sheet; cells hold at most 32767 characters.",
+    "jsonNote": "An array of objects, one per row, keyed by column name.",
+    "uncommitted": "The export runs in a new session and cannot see uncommitted changes in this tab",
+    "ok": "Export",
+    "running": "Exporting {{name}}",
+    "progress_one": "{{count}} row written",
+    "progress_other": "{{count}} rows written",
+    "cancel": "Cancel",
+    "done": "Export finished: {{name}}",
+    "doneRows_one": "{{count}} row",
+    "doneRows_other": "{{count}} rows",
+    "lobTruncated_one": "{{count}} large value exceeded 256 MB; only the first part was written",
+    "lobTruncated_other": "{{count}} large values exceeded 256 MB; only the first part was written",
+    "cellTruncated_one": "{{count}} cell exceeded 32767 characters and was truncated",
+    "cellTruncated_other": "{{count}} cells exceeded 32767 characters and were truncated",
+    "reveal": "Open Containing Folder",
+    "failed": "Export failed: {{name}}",
+    "cancelled": "Export cancelled: {{name}}"
   }
 }
```

- [ ] **Step 10: 修改 `web/src/locales/zh-CN/results.json`**

```diff
--- a/web/src/locales/zh-CN/results.json
+++ b/web/src/locales/zh-CN/results.json
@@ -36,5 +36,42 @@
     "copyFailed": "复制失败",
     "text": "文本",
     "hex": "十六进制"
+  },
+  "export": {
+    "button": "导出",
+    "title": "导出",
+    "format": "格式",
+    "formats": {
+      "csv": "CSV",
+      "xlsx": "Excel（xlsx）",
+      "insert": "INSERT 语句",
+      "json": "JSON"
+    },
+    "delimiter": "分隔符",
+    "delimiters": {
+      "comma": "逗号",
+      "tab": "制表符",
+      "semicolon": "分号"
+    },
+    "header": "第一行写列名",
+    "bom": "加 UTF-8 BOM（Excel 直接打开中文不乱码）",
+    "nullText": "NULL 写成",
+    "nullEmpty": "空",
+    "table": "目标表",
+    "rowsPerInsert": "每条语句的行数",
+    "xlsxNote": "第一行是列名；超过 1048575 行时续到下一个工作表；单元格最多 32767 个字符。",
+    "jsonNote": "对象数组，每行一个对象，键为列名。",
+    "uncommitted": "导出用新会话执行，看不到当前标签中未提交的修改",
+    "ok": "导出",
+    "running": "正在导出 {{name}}",
+    "progress": "已写出 {{count}} 行",
+    "cancel": "取消",
+    "done": "导出完成：{{name}}",
+    "doneRows": "共 {{count}} 行",
+    "lobTruncated": "{{count}} 个大字段超过 256 MB，只写出了前一部分",
+    "cellTruncated": "{{count}} 个单元格超过 32767 个字符，已截断",
+    "reveal": "打开所在文件夹",
+    "failed": "导出失败：{{name}}",
+    "cancelled": "已取消导出：{{name}}"
   }
 }
```

- [ ] **Step 11: 修改 `web/src/shared/api.ts`**

```diff
--- a/web/src/shared/api.ts
+++ b/web/src/shared/api.ts
@@ -191,6 +191,52 @@ export interface DataSqlParams {
   count?: boolean
 }
 
+/** 导出格式（M1c3）。 */
+export type ExportFormat = 'csv' | 'xlsx' | 'insert' | 'json'
+
+/** 导出选项；只有对应格式用到的字段起作用。 */
+export interface ExportOptions {
+  delimiter: ',' | '\t' | ';'
+  header: boolean
+  bom: boolean
+  null_text: string
+  table: string
+  rows_per_insert: number
+}
+
+/** 导出什么：一条查询语句，或数据标签的对象。 */
+export type ExportSource =
+  | { sql: string }
+  | { data: { schema: string; object: string; filter?: string; order?: { column: string; desc: boolean } | null } }
+
+export interface ExportRequest {
+  profile_id: string
+  source: ExportSource
+  format: ExportFormat
+  options: ExportOptions
+  /** 桌面版"另存为"选出的路径；浏览器模式不给。 */
+  path: string | null
+  file_name: string
+}
+
+export interface ExportStatus {
+  id: string
+  state: 'running' | 'done' | 'failed' | 'cancelled'
+  rows: number
+  warnings: { lob_truncated: number; cell_truncated: number }
+  error: ErrorBody | null
+  file_name: string
+  path: string | null
+}
+
+/** 一条运维查询（M1c3）。 */
+export interface OpsQuery {
+  key: string
+  name: string
+  category: 'session' | 'lock' | 'storage' | 'sql'
+  sql: string
+}
+
 export interface SequenceInfo {
   name: string
 }
@@ -364,6 +410,13 @@ export function createApi(token: string, fetchFn: FetchFn = (i, o) => fetch(i, o
     updateFavorite: (id: string, patch: FavoritePatch) =>
       request<Favorite>('PUT', `/favorites/${encodeURIComponent(id)}`, patch),
     deleteFavorite: (id: string) => request<null>('DELETE', `/favorites/${encodeURIComponent(id)}`),
+    ops: (id: string) => request<OpsQuery[]>('GET', `${p(id)}/ops`),
+    startExport: async (req: ExportRequest) => (await request<{ id: string }>('POST', '/exports', req)).id,
+    exportStatus: (id: string) => request<ExportStatus>('GET', `/exports/${encodeURIComponent(id)}`),
+    /** 进行中的导出取消；已结束的删除记录。 */
+    cancelExport: (id: string) => request<null>('DELETE', `/exports/${encodeURIComponent(id)}`),
+    /** 浏览器模式的下载地址：链接不能带请求头，令牌放在查询参数里。 */
+    exportFileUrl: (id: string) => `/api/exports/${encodeURIComponent(id)}/file?token=${encodeURIComponent(token)}`,
     openSession: (profileId: string) => request<SessionInfo>('POST', '/sessions', { profile_id: profileId }),
     session: (id: number) => request<SessionInfo>('GET', `/sessions/${id}`),
     /** 关闭会话；有未提交的修改时按 commit 提交或回滚。 */
```

- [ ] **Step 12: 修改 `web/src/shared/shell.ts`**

```diff
--- a/web/src/shared/shell.ts
+++ b/web/src/shared/shell.ts
@@ -1,4 +1,5 @@
-// 桌面外壳的原生能力（M1 设计文档 2.2 节）：自绘标题栏的窗口操作，以及置顶、恢复默认位置、检查更新、关于、退出。
+// 桌面外壳的原生能力（M1 设计文档 2.2 节）：自绘标题栏的窗口操作，以及置顶、恢复默认位置、检查更新、关于、退出；
+// 导出（M1c3）用的"另存为"对话框和"在文件夹中显示"。
 // 只有这里调用 Tauri；其他代码通过这里的函数使用。浏览器模式（dblens-server）没有 Tauri，desktop 为 null。
 
 import { invoke } from '@tauri-apps/api/core'
@@ -32,6 +33,9 @@ export interface DesktopShell {
   setAutoCheckUpdate(on: boolean): Promise<void>
   /** 订阅状态变化，返回取消订阅的函数。 */
   onChange(cb: (info: ShellInfo) => void): Promise<() => void>
+  /** 系统"另存为"对话框；用户取消时为 null。 */
+  saveDialog(defaultName: string, filters: { name: string; extensions: string[] }[]): Promise<string | null>
+  revealInFolder(path: string): Promise<void>
 }
 
 /** 页面是否运行在桌面程序里：Tauri 会在页面脚本运行前注入 `__TAURI_INTERNALS__`。 */
@@ -45,6 +49,8 @@ function createDesktopShell(): DesktopShell {
     action: (action) => invoke('window_action', { action }),
     setAutoCheckUpdate: (on) => invoke('set_auto_check_update', { on }),
     onChange: async (cb) => listen<ShellInfo>(SHELL_EVENT, (e) => cb(e.payload)),
+    saveDialog: (defaultName, filters) => invoke<string | null>('save_dialog', { defaultName, filters }),
+    revealInFolder: (path) => invoke('reveal_in_folder', { path }),
   }
 }
 
```

- [ ] **Step 13: 检查**

在 PowerShell 中执行（bash 里 `cd web` 会触发坏掉的 fnm 钩子，`&&` 后面的命令会被跳过）：

```powershell
$env:PATH = "D:\Environments\nvm4w\nodejs;$env:PATH"; Set-Location web; pnpm typecheck; pnpm test; pnpm build
```

Expected: tsc 无输出；32 个测试文件、204 个测试通过；打包成功。

- [ ] **Step 14: 提交**

```bash
git add web/src/app/Shell.tsx web/src/features/data/DataView.tsx web/src/features/editor/QueryView.tsx web/src/features/export/ExportDialog.tsx web/src/features/export/dialog.ts web/src/features/export/options.test.ts web/src/features/export/options.ts web/src/features/results/ResultsPanel.tsx web/src/locales/en/results.json web/src/locales/zh-CN/results.json web/src/shared/api.ts web/src/shared/shell.ts
git commit -m "feat(web): 导出：结果面板和数据标签的导出对话框，进度通知，桌面版另存为和打开所在文件夹" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 7: 前端：运维查询侧栏

- 侧栏按分类列出当前连接的运维查询；连接依次取侧栏下拉框选的、对象树选中的、当前标签的、第一个。
- 点击用 `openSql(sql, profileId, { run: true })` 新开标签；`tabs.ts` 的 `markRun` / `takeRun` 记下"打开后执行"（不随标签保存），查询标签第一次显示时取走标记并执行全文。
- 右键：在新标签中打开（不执行）、复制 SQL。没有连接时引导新建。
- 替换外壳中"即将推出"的占位（删除词条 `app.side.comingSoon`）；词条 `app.ops.*`。

**Files:**
- Modify: `web/src/app/Shell.tsx`
- Modify: `web/src/features/editor/QueryView.tsx`
- Modify: `web/src/features/editor/actions.tsx`
- Modify: `web/src/features/editor/tabs.test.ts`
- Modify: `web/src/features/editor/tabs.ts`
- Create: `web/src/features/ops/OpsPanel.tsx`
- Create: `web/src/features/ops/ops.test.ts`
- Create: `web/src/features/ops/ops.ts`
- Modify: `web/src/locales/en/app.json`
- Modify: `web/src/locales/zh-CN/app.json`

**Interfaces:**
- Consumes：任务 6 的 `api.ops`、`OpsQuery`。
- Produces：
  - `features/ops/ops.ts`：`CATEGORIES`、`groupByCategory(list)`、`pickProfile(profiles, ...candidates)`；
  - `features/editor/tabs.ts`：`markRun(tabId)`、`takeRun(tabId)`；
  - `useQueryActions().openSql(sql, profileId?, { run }?)`。

- [ ] **Step 1: 修改 `web/src/features/editor/tabs.test.ts`**

```diff
--- a/web/src/features/editor/tabs.test.ts
+++ b/web/src/features/editor/tabs.test.ts
@@ -2,7 +2,7 @@ import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
 
 import i18n from '../../shared/i18n'
 import { useSettings } from '../../shared/settings'
-import { TABS_KEY, parseTabs, persistTabs, useTabs } from './tabs'
+import { TABS_KEY, markRun, parseTabs, persistTabs, takeRun, useTabs } from './tabs'
 
 describe('标签页', () => {
   beforeEach(() => useTabs.getState().restore(undefined))
@@ -108,6 +108,17 @@ describe('数据标签', () => {
   })
 })
 
+describe('打开后执行', () => {
+  it('标记只取走一次，不随标签保存', () => {
+    const id = useTabs.getState().open('p', 'SELECT 1')
+    markRun(id)
+    const saved = JSON.stringify({ tabs: useTabs.getState().tabs })
+    expect(saved).not.toContain('run')
+    expect(takeRun(id)).toBe(true)
+    expect(takeRun(id)).toBe(false)
+  })
+})
+
 describe('保存标签', () => {
   beforeEach(() => vi.useFakeTimers())
   afterEach(() => vi.useRealTimers())
```

- [ ] **Step 2: 新建 `web/src/features/ops/ops.test.ts`**

```ts
import { describe, expect, it } from 'vitest'

import type { OpsQuery, Profile } from '../../shared/api'
import { groupByCategory, pickProfile } from './ops'

const q = (key: string, category: OpsQuery['category']): OpsQuery => ({ key, name: key, category, sql: 'SELECT 1' })
const profile = (id: string) => ({ id }) as Profile

describe('运维查询侧栏', () => {
  it('按固定的分类顺序分组，组内保持原顺序，空分类不出现', () => {
    const groups = groupByCategory([q('slow', 'sql'), q('sessions', 'session'), q('running', 'session'), q('size', 'storage')])
    expect(groups.map((g) => [g.category, g.items.map((i) => i.key)])).toEqual([
      ['session', ['sessions', 'running']],
      ['storage', ['size']],
      ['sql', ['slow']],
    ])
  })

  it('连接：依次取侧栏选的、对象树选中的、当前标签的，已删除的跳过，最后取第一个', () => {
    const ps = [profile('a'), profile('b'), profile('c')]
    expect(pickProfile(ps, null, 'b', 'c')).toBe('b')
    expect(pickProfile(ps, 'gone', undefined, 'c')).toBe('c')
    expect(pickProfile(ps, null, null, null)).toBe('a')
    expect(pickProfile([], 'a')).toBeNull()
  })
})
```

- [ ] **Step 3: 修改 `web/src/app/Shell.tsx`**

```diff
--- a/web/src/app/Shell.tsx
+++ b/web/src/app/Shell.tsx
@@ -2,7 +2,7 @@
 // 侧栏宽度经 useDefaultLayout 存进设置（ui.layout.*），下次启动恢复。
 // 没有查询标签时工作区显示欢迎页；工作区（Monaco、结果表格）懒加载。
 
-import { Button, Empty, Spin } from 'antd'
+import { Button, Spin } from 'antd'
 import { IconDatabase, IconFileCode, IconHistory, IconPlus, IconSettings, IconStar, IconTool } from '@tabler/icons-react'
 import { Suspense, lazy, useEffect, useRef } from 'react'
 import { useTranslation } from 'react-i18next'
@@ -15,6 +15,7 @@ import ExportDialog from '../features/export/ExportDialog'
 import FavoriteDialog from '../features/favorites/FavoriteDialog'
 import FavoritesPanel from '../features/favorites/FavoritesPanel'
 import HistoryPanel from '../features/history/HistoryPanel'
+import OpsPanel from '../features/ops/OpsPanel'
 import ObjectTree from '../features/tree/ObjectTree'
 import { layoutStorage } from '../shared/settings'
 import StatusBar from './StatusBar'
@@ -37,11 +38,10 @@ const SIDES: { key: SideKey; icon: typeof IconDatabase }[] = [
 ]
 
 function SideContent({ side }: { side: SideKey }) {
-  const { t } = useTranslation()
   if (side === 'db') return <ObjectTree />
   if (side === 'history') return <HistoryPanel />
   if (side === 'favorites') return <FavoritesPanel />
-  return <Empty style={{ marginTop: 48 }} description={t('app.side.comingSoon')} image={Empty.PRESENTED_IMAGE_SIMPLE} />
+  return <OpsPanel />
 }
 
 function Welcome() {
```

- [ ] **Step 4: 修改 `web/src/features/editor/QueryView.tsx`**

```diff
--- a/web/src/features/editor/QueryView.tsx
+++ b/web/src/features/editor/QueryView.tsx
@@ -24,7 +24,7 @@ import { editorLanguage, formatSql } from './dialect'
 import { errorMarks } from './markers'
 import { runnerFor, useTabRuntime } from './runner'
 import SqlEditor, { type MonacoEditor } from './SqlEditor'
-import { type QueryTab, useTabs } from './tabs'
+import { type QueryTab, takeRun, useTabs } from './tabs'
 
 /** 会改变对象结构的语句（按首个关键字）。 */
 const DDL = new Set(['CREATE', 'ALTER', 'DROP', 'RENAME', 'TRUNCATE', 'COMMENT'])
@@ -118,6 +118,11 @@ export default function QueryView({ tab, profiles }: { tab: QueryTab; profiles:
     [runner, profile, rt.session?.variant, message, t, tab.id],
   )
 
+  // 运维查询打开的标签：打开后立即执行全文
+  useEffect(() => {
+    if (takeRun(tab.id)) void runner.run(tab.sql, 0)
+  }, [tab.id]) // 只在标签第一次显示时检查
+
   // 协议层面的错误（如上一段还没执行完）用全局提示（M1 设计文档第 5 节），同时记在"消息"里
   const protocolError = rt.exec.protocolError
   useEffect(() => {
```

- [ ] **Step 5: 修改 `web/src/features/editor/actions.tsx`**

```diff
--- a/web/src/features/editor/actions.tsx
+++ b/web/src/features/editor/actions.tsx
@@ -8,7 +8,7 @@ import { useApi } from '../../app/context'
 import { useUi } from '../../app/ui'
 import { sendInsert } from './commands'
 import { disposeRunner, useRuntime } from './runner'
-import { useTabs } from './tabs'
+import { markRun, useTabs } from './tabs'
 
 export function useQueryActions() {
   const api = useApi()
@@ -28,8 +28,8 @@ export function useQueryActions() {
     else useUi.getState().openConnection(null)
   }
 
-  /** 用给定的 SQL 新开标签：连接已删除或没给时，依次用对象树中选中的、当前标签的、第一个连接。 */
-  const openSql = (sql: string, profileId?: string) => {
+  /** 用给定的 SQL 新开标签：连接已删除或没给时，依次用对象树中选中的、当前标签的、第一个连接。`run` 为 true 时打开后立即执行。 */
+  const openSql = (sql: string, profileId?: string, { run = false } = {}) => {
     const { tabs, active } = useTabs.getState()
     const exists = (id?: string | null) => (id && profiles?.some((p) => p.id === id) ? id : undefined)
     const id =
@@ -37,8 +37,10 @@ export function useQueryActions() {
       exists(useUi.getState().selectedProfile) ??
       exists(tabs.find((x) => x.id === active)?.profileId) ??
       profiles?.[0]?.id
-    if (id) useTabs.getState().open(id, sql)
-    else useUi.getState().openConnection(null)
+    if (id) {
+      const tabId = useTabs.getState().open(id, sql)
+      if (run) markRun(tabId)
+    } else useUi.getState().openConnection(null)
   }
 
   /** 插入到当前编辑器的光标处；当前不是查询标签（没有标签或是数据标签）时新开一个。 */
```

- [ ] **Step 6: 修改 `web/src/features/editor/tabs.ts`**

```diff
--- a/web/src/features/editor/tabs.ts
+++ b/web/src/features/editor/tabs.ts
@@ -138,6 +138,18 @@ export const useTabs = create<TabsState>()((set, get) => ({
   restore: (raw) => set(parseTabs(raw)),
 }))
 
+/** 打开后要立即执行的标签（运维查询，M1c3）。不随标签保存：重新打开页面时不会再执行。 */
+const pendingRuns = new Set<string>()
+
+export function markRun(tabId: string) {
+  pendingRuns.add(tabId)
+}
+
+/** 取走"打开后执行"的标记：有标记时返回 true，只返回一次。 */
+export function takeRun(tabId: string): boolean {
+  return pendingRuns.delete(tabId)
+}
+
 let stopPersist: (() => void) | null = null
 
 /** 标签有变化时延迟保存到设置（输入 SQL 时不必每个字都保存）。重复调用不会重复订阅。返回取消订阅的函数。 */
```

- [ ] **Step 7: 新建 `web/src/features/ops/OpsPanel.tsx`**

```tsx
// 运维查询侧栏（M1c3 设计文档 5.3 节）：按当前连接的方言列出会话、锁、存储、SQL 统计等查询。
// 点击用该连接新开标签并执行；右键在新标签中打开（不执行）、复制 SQL。结束会话的语句在结果的 kill_sql 列，只生成不执行。

import { App, Button, Dropdown, Empty, Select } from 'antd'
import { IconPlayerPlay } from '@tabler/icons-react'
import { useQuery } from '@tanstack/react-query'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

import { useApi } from '../../app/context'
import { useUi } from '../../app/ui'
import DbIcon from '../../shared/DbIcon'
import type { OpsQuery } from '../../shared/api'
import { useQueryActions } from '../editor/actions'
import { useTabs } from '../editor/tabs'
import { groupByCategory, pickProfile } from './ops'

export default function OpsPanel() {
  const api = useApi()
  const { t, i18n } = useTranslation()
  const { message } = App.useApp()
  const { openSql } = useQueryActions()
  const { data: profiles = [] } = useQuery({ queryKey: ['profiles'], queryFn: () => api.profiles() })
  const selected = useUi((s) => s.selectedProfile)
  const openConnection = useUi((s) => s.openConnection)
  const activeProfile = useTabs((s) => s.tabs.find((x) => x.id === s.active)?.profileId)
  const [chosen, setChosen] = useState<string | null>(null)
  const profileId = pickProfile(profiles, chosen, selected, activeProfile)
  const { data = [], error } = useQuery({
    queryKey: ['ops', profileId, i18n.language],
    queryFn: () => api.ops(profileId!),
    enabled: profileId !== null,
  })

  if (!profileId) {
    return (
      <Empty style={{ marginTop: 48 }} description={t('app.ops.noProfile')} image={Empty.PRESENTED_IMAGE_SIMPLE}>
        <Button type="primary" onClick={() => openConnection(null)}>
          {t('app.ops.newConnection')}
        </Button>
      </Empty>
    )
  }

  const run = (q: OpsQuery, execute: boolean) => openSql(q.sql, profileId, { run: execute })
  return (
    <div className="hist">
      <div className="hist-filter">
        <Select
          size="small"
          value={profileId}
          onChange={setChosen}
          options={profiles.map((p) => ({
            value: p.id,
            label: (
              <span>
                <DbIcon driver={p.driver} variant={p.variant} /> {p.name}
              </span>
            ),
          }))}
        />
        <div className="muted" style={{ fontSize: 'var(--fs-xs)' }}>
          {t('app.ops.hint')}
        </div>
      </div>
      <div className="hist-list">
        {error && <div className="err" style={{ padding: 12 }}>{(error as Error).message}</div>}
        {groupByCategory(data).map((g) => (
          <div key={g.category}>
            <div className="hist-day">{t(`app.ops.category.${g.category}`)}</div>
            {g.items.map((q) => (
              <Dropdown
                key={q.key}
                trigger={['contextMenu']}
                menu={{
                  items: [
                    { key: 'open', label: t('app.ops.openNoRun') },
                    { key: 'copy', label: t('app.ops.copy') },
                  ],
                  onClick: ({ key }) => {
                    if (key === 'open') run(q, false)
                    else
                      void navigator.clipboard.writeText(q.sql).then(() => message.success(t('app.ops.copied')))
                  },
                }}
              >
                <div className="hist-item row" style={{ gap: 8, cursor: 'pointer' }} onClick={() => run(q, true)} title={q.sql}>
                  <IconPlayerPlay size={14} className="muted" />
                  <span>{q.name}</span>
                </div>
              </Dropdown>
            ))}
          </div>
        ))}
      </div>
    </div>
  )
}
```

- [ ] **Step 8: 新建 `web/src/features/ops/ops.ts`**

```ts
// 运维查询侧栏的纯逻辑（M1c3 设计文档 5.3 节）：按分类分组、选用哪个连接。

import type { OpsQuery, Profile } from '../../shared/api'

export const CATEGORIES: OpsQuery['category'][] = ['session', 'lock', 'storage', 'sql']

/** 按分类分组，分类按固定顺序，组内保持服务端给的顺序；空分类不出现。 */
export function groupByCategory(list: OpsQuery[]): { category: OpsQuery['category']; items: OpsQuery[] }[] {
  return CATEGORIES.map((category) => ({ category, items: list.filter((q) => q.category === category) })).filter(
    (g) => g.items.length > 0,
  )
}

/** 用哪个连接：用户在侧栏选的，否则对象树中选中的，否则当前标签的，否则第一个。已删除的不算。 */
export function pickProfile(
  profiles: Profile[],
  ...candidates: (string | null | undefined)[]
): string | null {
  for (const id of candidates) if (id && profiles.some((p) => p.id === id)) return id
  return profiles[0]?.id ?? null
}
```

- [ ] **Step 9: 修改 `web/src/locales/en/app.json`**

```diff
--- a/web/src/locales/en/app.json
+++ b/web/src/locales/en/app.json
@@ -29,8 +29,7 @@
     "db": "Database",
     "history": "History",
     "favorites": "Favorites",
-    "ops": "Ops Queries",
-    "comingSoon": "Coming soon"
+    "ops": "Ops Queries"
   },
   "welcome": {
     "newConnection": "New Connection",
@@ -135,5 +134,19 @@
     "edit": "Edit",
     "delete": "Delete",
     "deleteFolderConfirm": "Delete the folder \"{{name}}\" and everything in it?"
+  },
+  "ops": {
+    "hint": "Click to run in a new tab. In session and lock results, the kill_sql column holds the statement that ends the session; copy it and run it yourself.",
+    "category": {
+      "session": "Sessions",
+      "lock": "Locks",
+      "storage": "Storage",
+      "sql": "SQL Statistics"
+    },
+    "openNoRun": "Open in New Tab (Do Not Run)",
+    "copy": "Copy SQL",
+    "copied": "Copied",
+    "noProfile": "No connections yet",
+    "newConnection": "New Connection"
   }
 }
```

- [ ] **Step 10: 修改 `web/src/locales/zh-CN/app.json`**

```diff
--- a/web/src/locales/zh-CN/app.json
+++ b/web/src/locales/zh-CN/app.json
@@ -29,8 +29,7 @@
     "db": "数据库",
     "history": "历史",
     "favorites": "收藏",
-    "ops": "运维查询",
-    "comingSoon": "即将推出"
+    "ops": "运维查询"
   },
   "welcome": {
     "newConnection": "新建连接",
@@ -133,5 +132,19 @@
     "edit": "编辑",
     "delete": "删除",
     "deleteFolderConfirm": "删除文件夹“{{name}}”及其中的全部内容？"
+  },
+  "ops": {
+    "hint": "点击在新标签中执行。会话和锁的结果中 kill_sql 列是结束会话的语句，复制后自行执行。",
+    "category": {
+      "session": "会话",
+      "lock": "锁",
+      "storage": "存储",
+      "sql": "SQL 统计"
+    },
+    "openNoRun": "在新标签中打开（不执行）",
+    "copy": "复制 SQL",
+    "copied": "已复制",
+    "noProfile": "还没有连接",
+    "newConnection": "新建连接"
   }
 }
```

- [ ] **Step 11: 检查**

在 PowerShell 中执行（bash 里 `cd web` 会触发坏掉的 fnm 钩子，`&&` 后面的命令会被跳过）：

```powershell
$env:PATH = "D:\Environments\nvm4w\nodejs;$env:PATH"; Set-Location web; pnpm typecheck; pnpm test; pnpm build
```

Expected: tsc 无输出；33 个测试文件、207 个测试通过；打包成功。

- [ ] **Step 12: 提交**

```bash
git add web/src/app/Shell.tsx web/src/features/editor/QueryView.tsx web/src/features/editor/actions.tsx web/src/features/editor/tabs.test.ts web/src/features/editor/tabs.ts web/src/features/ops/OpsPanel.tsx web/src/features/ops/ops.test.ts web/src/features/ops/ops.ts web/src/locales/en/app.json web/src/locales/zh-CN/app.json
git commit -m "feat(web): 运维查询侧栏：按方言列出会话、锁、存储、SQL 统计，点击在新标签中执行" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 8: 说明文档与实测

写 M1c3 说明文档，M1 设计文档记录 M1 全部完成。然后用浏览器模式和桌面程序实测。若任务 1 的复查有升级，在说明文档的"依赖"一节补上。

**Files:**
- Create: `docs/notes/m1c3-export-ops.md`
- Modify: `docs/superpowers/specs/2026-10-25-m1-workbench-design.md`

- [ ] **Step 1: 新建 `docs/notes/m1c3-export-ops.md`**

````markdown
# M1c3 导出与运维查询

依据：设计文档 `docs/superpowers/specs/2026-10-26-m1c3-export-ops-design.md`。M1c 的第三份，也是 M1 的最后一份（M1c1 历史与收藏、M1c2 对象操作已完成）。

## 结构

```text
crates/dblens-driver/src/{traits.rs, blocking.rs, conformance.rs}
                  Connection::set_lob_limit：大字段的读取上限；阻塞适配器转发；一致性测试 lob_limit_reads_full_value
crates/dblens-driver-{mysql,postgres,oracle,dameng}/src/   各驱动按连接上的上限读取大字段（原来写死 64 KB）
crates/dblens-dialect/
  sql/ops/<方言>.sql   运维查询模板：-- [key] 分段，段首注释写中英文名称和分类；mariadb.sql 只覆盖 locks
  src/ops.rs          解析模板（保持顺序）、Dialect::ops_queries、兼容库回退到所属家族
crates/dblens-service/src/export/
  write.rs            四种格式的写入（RowWriter）：CSV（csv 库）、xlsx（rust_xlsxwriter 低内存模式）、INSERT（方言字面量）、JSON
  job.rs              导出任务：Service::start_export、ExportManager（状态、取消、下载、过期清理）
crates/dblens-service/src/service.rs   Service::ops（运维查询列表）、exports、history
crates/dblens-api/src/exports.rs       /exports、/exports/{id}、/exports/{id}/file、/profiles/{id}/ops
crates/dblens-desktop/src/shell.rs     save_dialog（另存为）、reveal_in_folder（在文件夹中显示）
web/src/
  features/export/   导出对话框、默认选项和文件名、进度通知
  features/ops/      运维查询侧栏
  features/editor/   打开后执行的标记（markRun / takeRun）、结果面板和数据标签的导出入口
```

## 导出

- 任务另开一个连接（`registry.connect`），不经过会话管理：
  - 连接上设置大字段读取上限 256 MB，执行、每批读 1000 行、写文件，结束后关闭；
  - 只导出第一个结果集；
  - 照常写一条执行历史（行数为写出的行数）。

  设计文档写的是"经 SessionManager"。直接用连接更简单：会话的事件接口是给界面用的，导出不需要确认、事务和 schema 跟踪。
- 只接受单条查询语句：方言拆分后只有一条，且语句分析为查询（DQL）。数据标签的导出由后端用 `data_sql` 生成 SELECT。
- 取消：
  - 先置取消标志，再取连接的取消句柄；任务存好句柄后先检查标志。这样无论取消在连接前还是执行中到达，都能结束。实现时测试发现过这个竞态。
  - 读取循环每批检查一次标志：语句执行完、正在读取时，数据库层面的取消可能不起作用。
- 失败、取消时删除已写的文件。用户在"另存为"中选了已有文件时，原文件会被覆盖，取消后不会恢复。
- 浏览器模式：
  - 临时文件放在数据目录的 `exports/` 下，服务启动时清空。设计文档写的是退出时清空，启动时清空效果相同，也能清掉异常退出留下的文件。
  - 下载用流式响应，传完或连接中断后删除文件。
  - 前端轮询到完成后，用带令牌的链接触发下载。
- 错误：任务状态中保存的是错误对象，序列化时按请求的语言渲染。
- 格式细节（单元测试覆盖）：
  - CSV：RFC 4180 转义；可选 BOM；NULL 写成空或 `NULL`。
  - xlsx：表头加粗；数字、日期、时间按类型写入；精确小数写成文本（避免经过浮点）；单元格超过 32767 个字符时截断，计入警告；1048575 行后续到下一个工作表。
  - INSERT：值用方言的字面量（PG 的二进制为 `decode('…','hex')`）；可以多行合并成一条。
  - JSON：重复的列名加后缀 `_2`、`_3`；数字写成 JSON 数字，精确小数写成字符串。

## 运维查询

- 每种数据库 6 项：会话列表、正在执行的语句、锁等待、存储（Oracle、达梦为表空间使用，MySQL、PG 为各库大小）、占用最大的表、慢 SQL。
- 会话、锁的结果带 `kill_sql` 列：MySQL 系 `KILL id`，PG 系 `pg_terminate_backend`，Oracle `ALTER SYSTEM KILL SESSION 'sid,serial#' IMMEDIATE`，达梦 `SP_CLOSE_SESSION`。
- 列表接口不连接数据库：按连接上次识别出的变体选模板，没连接过时按驱动。设计文档没有写明，这样打开侧栏不会触发连接（也不会弹密码库解锁）。
- 侧栏：点击用 `openSql(..., { run: true })` 新开标签；查询标签第一次显示时取走"打开后执行"的标记并执行全文。标记不随标签保存，重新打开页面不会再执行。
- 测试库上的结果：
  - Oracle、达梦的测试账号能读 V$ 和 DBA_ 视图，6 项都能执行；
  - PostgreSQL 没有安装 pg_stat_statements 扩展，"慢 SQL"报"关系 pg_stat_statements 不存在"（按设计显示数据库的报错）；
  - 其余全部通过。

## 依赖

- 新增 `csv` 1.4.0（MIT / Unlicense）。
- 新增 `rust_xlsxwriter` 0.100.0（MIT / Apache-2.0，特性 `constant_memory`、`chrono`）。写计划时最新的 0.101.0 发布未满 7 天。
- `dblens-api` 的 tokio 多开 `fs`、`io-util` 特性（下载时流式读文件）。

## 验证

- 后端：fmt、clippy 通过；全部测试在 `DBLENS_LANG=en` 下对 5 个实库通过。
  - 驱动一致性测试：放宽上限后读出 100000 字节的大字段，改回默认后又是预览（4 个驱动，MySQL 和 MariaDB 各一次）。
  - 导出实库测试（5 个库）：一行含中文和 100000 个字符的大字段，CSV、JSON、INSERT 都写出完整内容（字符数与数据库报告的一致）；xlsx 截断并报一个警告；另导出 2500 行（MySQL 系 990 行）核对行数。
  - 运维查询实库测试（5 个库）：逐个执行全部 6 项，只允许 PG 的慢 SQL 失败。
  - 有一次全量运行中运维查询的实库测试失败一次，之后连续 12 次通过，判断与 M1c1 记录的情况相同：多个测试同时连达梦时偶发连接失败。
- 前端：tsc、Vitest、打包通过。
- 浏览器实测（浏览器模式，5 个库建测试数据）：
  - 中文界面：运维查询侧栏按分类列出；点"会话列表"新开标签并执行，结果带 `kill_sql` 列。结果面板导出 CSV：完成通知显示行数，下载链接返回正确的内容。数据标签加过滤 `parent_id = 2` 后导出 INSERT：默认目标表为 `public.c2_child`，文件名为对象名加时间，400 条语句。
  - 英文界面：侧栏和导出对话框的文字；侧栏跟随当前标签的连接，切到 Oracle（生产环境）执行"表空间使用"。
  - 内置浏览器不能真正下载文件：实测时拦截了下载链接的点击，用请求取回文件内容核对。
- 未验证：桌面版的"另存为"对话框和"打开所在文件夹"需要在桌面程序中手动操作一次（系统对话框不能由自动化点击）。
````

- [ ] **Step 2: 修改 `docs/superpowers/specs/2026-10-25-m1-workbench-design.md`**

```diff
--- a/docs/superpowers/specs/2026-10-25-m1-workbench-design.md
+++ b/docs/superpowers/specs/2026-10-25-m1-workbench-design.md
@@ -159,6 +159,7 @@ M1b 在写计划时（2026-10-25，用户确认）再拆成两份：
 - 2026-10-26 M1c 再拆成三份（用户确认）：M1c1 历史与收藏；M1c2 对象操作（查看 DDL、生成 SQL、关联查询、"查看数据"标签）；M1c3 导出与运维查询。
 - M1c1 已完成（设计 `docs/superpowers/specs/2026-10-26-m1c1-history-favorites-design.md`，详细计划 `docs/superpowers/plans/2026-10-26-m1c1-history-favorites.md`，说明 `docs/notes/m1c1-history-favorites.md`）。
 - M1c2 已完成（设计 `docs/superpowers/specs/2026-10-26-m1c2-object-actions-design.md`，详细计划 `docs/superpowers/plans/2026-10-26-m1c2-object-actions.md`，说明 `docs/notes/m1c2-object-actions.md`）。
+- M1c3 已完成（设计 `docs/superpowers/specs/2026-10-26-m1c3-export-ops-design.md`，详细计划 `docs/superpowers/plans/2026-10-26-m1c3-export-ops.md`，说明 `docs/notes/m1c3-export-ops.md`）。M1 全部完成。
 
 右键菜单中的"查看 DDL"和"生成 SQL"依赖 M1c 的后端，M1a 先不显示这两项。
 
```

- [ ] **Step 3: 检查**

全量回归：后端对 5 个实库，前端测试和打包：

```bash
export CARGO_INCREMENTAL=0
cargo fmt --all
cargo clippy --config "source.mirror.registry='sparse+https://rsproxy.cn/index/'" --workspace --all-targets -- -D warnings
DBLENS_LANG=en cargo test --config "source.mirror.registry='sparse+https://rsproxy.cn/index/'" --workspace
```

实库测试：在上面的 `cargo test` 前以内联环境变量给出 `DBLENS_TEST_MYSQL_URL`、`DBLENS_TEST_MARIADB_URL`、`DBLENS_TEST_PG_URL`、`DBLENS_TEST_ORACLE_URL`、`DBLENS_TEST_DM_URL`（密码不写进任何文件）。

在 PowerShell 中执行（bash 里 `cd web` 会触发坏掉的 fnm 钩子，`&&` 后面的命令会被跳过）：

```powershell
$env:PATH = "D:\Environments\nvm4w\nodejs;$env:PATH"; Set-Location web; pnpm typecheck; pnpm test; pnpm build
```

浏览器模式实测（用新的数据目录启动 `dblens-server --web-dir web/dist`）：
1. 经接口建 5 个测试连接（密码只经环境变量传给建连接的脚本），经 WebSocket 建测试数据（父子表，子表 1200 行；MySQL 系 990 行）。
2. 中文界面，运维查询侧栏：按"会话 / 锁 / 存储 / SQL 统计"列出；点"会话列表"新开标签并执行，结果带 `kill_sql` 列。
3. 结果面板点"导出"，CSV：完成通知显示行数，浏览器下载文件，内容正确。
4. PG 数据标签加过滤 `parent_id = 2`，导出 INSERT：默认目标表 `public.c2_child`，文件名为对象名加时间，400 条语句。
5. 切到 English：侧栏和导出对话框为英文；侧栏下拉框切到 Oracle，执行"Tablespace Usage"。
6. 删除测试数据和测试连接（系统凭据中的密码随之删除），停掉服务。

桌面程序实测（需要用户在系统对话框中点击）：导出时弹出"另存为"，文件写到所选位置；完成通知中的"打开所在文件夹"打开资源管理器并选中文件。

Expected: 全部通过。

- [ ] **Step 4: 提交**

```bash
git add docs/notes/m1c3-export-ops.md docs/superpowers/specs/2026-10-25-m1-workbench-design.md
git commit -m "docs: M1c3 说明文档；M1 设计文档记录 M1 完成" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```
