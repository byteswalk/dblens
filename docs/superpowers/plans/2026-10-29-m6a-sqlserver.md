# M6a SQL Server：连接、浏览、查询 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 支持 SQL Server 2017 及以上：连接（SQL 账号、Windows 账号、当前 Windows 用户；实例名；TLS 和信任服务器证书）、像 SSMS 一样按"连接 → 库 → 架构 → 对象"浏览、按 `GO` 分批执行 T-SQL（多结果集、PRINT 消息、错误行号、取消）、结果编辑、运维查询、执行计划、生产环境危险语句确认；表设计器、导入、文档导出、比对和迁移留给 M6b，入口置灰。

**Architecture:** 新驱动 `dblens-driver-sqlserver` 基于 tiberius 0.13（打了补丁：结果流带出影响行数和 INFO 消息，取消后按包丢弃到确认为止），后台任务独占连接；方言层加 `Family::SqlServer`（方括号、`@P1`、`N''`、`GO` 分批、类型、元数据和运维模板、危险语句）；服务层的元数据连接池和缓存按"连接 + 库"区分，接口加 `catalog` 参数和库列表；前端把库名和架构合成一个字符串在各处传递，编辑器用 grammars-v4 的 T-SQL 语法做检查和补全。

**Tech Stack:** Rust（tiberius 0.13 + tokio-util compat、sqlparser 的 MsSqlDialect、insta）；React 19 + TypeScript、antd 6、Monaco、antlr-ng / antlr4-c3、sql-formatter（transactsql）、Vitest。

**Spec:** `docs/superpowers/specs/2026-10-29-m6a-sqlserver-design.md`

## Global Constraints

- 支持 SQL Server 2017 及以上；日常实测 2022（Docker），另跑 2025（Win11 上的 Express）。
- 认证：SQL 账号（全部平台）；指定 Windows 账号、当前 Windows 用户只在 Windows 上可用（其他平台报 `sqlserver.windows_auth_only`，不静默改用别的方式）。
- 对象树像 SSMS：连接 → 数据库 → 架构 → 对象；系统库排在后面并标注。其他驱动没有库这一层，树不变。
- 脚本按单独一行的 `GO` 分批（可带次数，上限 1000；字符串、方括号、注释里的不算），每批整段发给服务器；后端和前端用同一套规则。
- 生成的 SQL 一律带架构名；字符串字面量写成 `N'…'`。
- M6b 的功能（表设计器、导入、文档导出、比对、保存为模型、迁移）：后端对 SQL Server 报 `sqlserver.later`，前端入口置灰并提示，迁移和比对的来源、目标列表不出现 SQL Server。
- 不做兼容产品的识别（没有 `VariantProbe`）。
- 依赖只用正式发布版，跟随最新；新版发布满 7 天再升级（有意固定版本的记入 `scripts/check-deps.mjs` 的 `HOLD`）。tiberius 的 `sspi-rs`（非 Windows 的 Windows 认证）依赖预发布的 sha1，不启用。
- 前端给用户看的文字都用词条（中英文），英文区分单复数；后端错误和提示用 `msg!` 词条（`src` 中的中文字面量只能出现在 `#[cfg(test)]` 之后，单独的测试文件中也不能有）；日志不翻译。
- 代码注释用中文；文件 UTF-8、LF 换行；前端代码风格：无分号、单引号、两格缩进。
- cargo 命令加 `--config "source.mirror.registry='sparse+https://rsproxy.cn/index/'"`（clippy 写在 `clippy` 之后），并设 `CARGO_INCREMENTAL=0`；后端测试在 `DBLENS_LANG=en` 下运行。
- 实库测试一律串行（`--test-threads=1`），不加 `--include-ignored`（会跑达梦的 spike 测试并卡住）。
- 前端工具用 PowerShell 执行（`npm`/`npx` 在本机不可用，用 `pnpm exec`）。
- 测试数据库的密码只在命令行中以内联环境变量传入，不写进仓库、报告或提交信息。

## 与设计文档的差异（写计划时实测后调整）

- 按库取元数据：不用模板函数 `{{ catalog() }}` 拼三段名，改为元数据连接池按"连接 + 库"区分、连接时指定库（`Profile::in_catalog`）；模板只查当前库的 `sys.*` 视图，库名不进入 SQL 文本。
- TLS 三档：关闭（`EncryptionLevel::Off`，登录报文仍加密，服务器强制加密时由服务器决定）、必须（`Required`，不校验证书）、校验证书（`Required`，打开"信任服务器证书"时不校验，否则用指定的 CA）。DBLens 的 TLS 选项没有"优先"。
- Windows 认证只在 Windows 上可用（见上）。表单在所有平台都显示这两项，附说明。
- 取消：tiberius 0.13 能发 attention 包，但原来的清理按令牌读，结果集读到一半时会读乱，下一条查询得到空结果。补丁改为按包丢弃，直到收到带 attention 标志的 DONE；取消后连接保留，会话状态都还在。
- 危险语句：sqlparser 解析不了的批（过程式代码、没有分号分隔的多条语句）按关键字扫描（DROP、TRUNCATE、DELETE、UPDATE 及 SQL Server 特有的几个），不含 MERGE。
- 运维页面与其他库相同（会话、运行中、锁、存储、大表、慢 SQL），没有单独的"服务器参数"。
- 浏览表数据与其他库相同：执行 SELECT 后流式分批读取，不生成 `OFFSET … FETCH`。
- 索引的包含列和筛选条件在元数据中列出，生成 DDL 时不写（M6b 再补）。

## 执行说明

任务 1–5 的代码都已在临时副本中逐任务实现并通过检查，执行时按顺序重放。新文件给出全文，修改的文件给出 diff（基于上一个任务完成后的状态，可直接 `git apply`）。例外：
- `vendor/tiberius/` 从 crates.io 的 0.13.0 原样复制后套用给出的补丁（约 200 行），不逐个列出上游文件；
- `web/grammars/tsql/*.g4` 从 grammars-v4 的固定提交下载，给出校验和；
- `Cargo.lock`、`web/pnpm-lock.yaml` 由 cargo / pnpm 生成，不给 diff。

写计划时已运行：全部后端测试和前端测试；13 个实库目标的串行回归（含 SQL Server 2022）；SQL Server 2025 Express 上的 Windows 认证；浏览器中的连接、库树、`GO` 脚本、编辑、危险语句确认、执行计划、DDL、ER 图、运维、连接表单。

## 文件结构

```text
Cargo.toml（tiberius、tokio-util，[patch.crates-io] tiberius）; vendor/tiberius/（补丁）; vendor/PATCHES.md
crates/dblens-driver-sqlserver/{Cargo.toml, src/{lib,actor,conn,exec,types,error}.rs, tests/conformance.rs}（新增）
crates/dblens-driver/{Cargo.toml, src/params.rs}
crates/dblens-dialect/sql/{sqlserver.sql, ops/sqlserver.sql}（新增）
crates/dblens-dialect/src/{variant,ident,literal,ddl,dml,types,split,session,ops,lib}.rs, src/meta/{mod,template}.rs,
  src/design/{catalog,rebuild,sql}.rs, src/migrate/{data,expr,mod,names,script,types}.rs, src/compare/normalize.rs
crates/dblens-dialect/tests/{meta_snapshots.rs, meta_live.rs, fixtures/sqlserver.sql, snapshots/meta_snapshots__sqlserver.snap}
crates/dblens-service/src/{policy,profile,metadata,registry,service,session,changes,compare,design}.rs,
  src/{document/job,import/job,migrate/mod,model/ops}.rs; tests/{live,migrate_live}.rs
crates/dblens-api/src/{routes,lib}.rs, tests/live.rs; crates/dblens-core/locales/app.json
web/grammars/tsql/（新增）; web/scripts/gen-tsql.mjs（新增）; web/package.json; .gitignore
web/src/features/sql/tsql/{parser.ts, parser.test.ts}（新增）; web/src/features/sql/{types,parsers,dialects,completion,functions,provider,metadata}.ts
web/src/features/{tree,connections,editor,data,er,import,compare,migrate,model}/…; web/src/shared/{api,dbIcons}.ts; web/src/locales/{zh-CN,en}/{connections,editor,tree}.json
docs/notes/{m6a-sqlserver.md（新增）, m0-4b-api.md}; docs/superpowers/specs/2026-10-22-dblens-design.md
```

---

### Task 1: 依赖复查

按依赖原则（M1b2 设计文档 3.1 节）复查前端和后端的全部直接依赖。写计划时可升级的是 `i18next` 26.5.5 和 `react-i18next` 17.1.2；2026-10-30 执行时复查结果为 26.5.6 和 17.1.3（下面的 diff 是写计划时的版本，以执行当天的复查结果为准）。

**Files:**
- Modify: `web/package.json`

- [ ] **Step 1: 运行复查脚本**

本机经代理上网，Node 的 `fetch` 默认不读 `HTTPS_PROXY`，必须先设 `NODE_USE_ENV_PROXY=1`，否则全部查询失败，最后仍显示"0 个可升级"。

```powershell
$env:PATH = "D:\Environments\nvm4w\nodejs;$env:PATH"; $env:NODE_USE_ENV_PROXY = '1'; $env:HTTPS_PROXY = 'http://127.0.0.1:6789'; node scripts/check-deps.mjs
```

Expected：输出中没有"查询失败"；以执行当天的结果为准，输出中 `->` 的都升级（npm 用 `pnpm add 包@版本`，Cargo 改工作区 `Cargo.toml` 后 `cargo update -p 包`）。写计划时的升级如下。

- [ ] **Step 2: 修改 `web/package.json`**

```diff
--- a/web/package.json
+++ b/web/package.json
@@ -27,11 +27,11 @@
     "antlr4ng": "3.0.16",
     "dt-sql-parser": "4.5.1",
     "html-to-image": "1.11.11",
-    "i18next": "26.5.4",
+    "i18next": "26.5.5",
     "monaco-editor": "0.57.0",
     "react": "19.3.1",
     "react-dom": "19.3.1",
-    "react-i18next": "17.1.0",
+    "react-i18next": "17.1.2",
     "react-resizable-panels": "4.14.8",
     "simple-icons": "16.35.0",
     "sql-formatter": "15.9.0",
```

- [ ] **Step 3: 更新锁文件**

在 `web` 目录执行 `pnpm install` 更新 `pnpm-lock.yaml`。

- [ ] **Step 4: 检查**

```bash
export CARGO_INCREMENTAL=0
cargo fmt --all -- --check
cargo clippy --config "source.mirror.registry='sparse+https://rsproxy.cn/index/'" --workspace --all-targets -- -D warnings
DBLENS_LANG=en cargo test --config "source.mirror.registry='sparse+https://rsproxy.cn/index/'" --workspace
```

在 PowerShell 中执行（bash 里 `cd web` 会触发坏掉的 fnm 钩子，`&&` 后面的命令会被跳过）：

```powershell
$env:PATH = "D:\Environments\nvm4w\nodejs;$env:PATH"; Set-Location web; pnpm install; pnpm typecheck; pnpm test; pnpm build
```

Expected: 全部通过。

- [ ] **Step 5: 提交**

```bash
git add web/package.json web/pnpm-lock.yaml
git commit -m "chore: M6a 依赖复查（i18next 26.5.5、react-i18next 17.1.2）" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 2: 方言层：SQL Server 家族

- `variant.rs`：`Family::SqlServer`、`Variant::SqlServer`（显示名 "SQL Server"）。
- `ident.rs`：方括号引用（`]` 写成 `]]`）、`@P{n}` 占位符、保留字表 `SQLSERVER_RESERVED`；`literal.rs`：`N'…'`、`0x…`、布尔 `1`/`0`、日期时间写成 `CAST('…' AS datetime2)` 等 ISO 形式，分页 `OFFSET … ROWS FETCH NEXT … ROWS ONLY`。
- `split.rs`：`GO` 分批（单独一行、不区分大小写、可带次数 `GO n`（上限 `MAX_GO_COUNT` = 1000）和行尾 `--` 注释；字符串、方括号、可嵌套的 `/* */` 里的不算；每批保留起始行号）。
- `ddl.rs`：视图和同义词的 DDL 查询；建表语句（`IDENTITY(1,1)`、计算列 `AS (…)`、约束、索引），注释用 `sp_addextendedproperty`、与触发器之间用 `GO` 分隔；`dml.rs`：`INSERT … DEFAULT VALUES`。
- `types.rs`：逻辑类型与原生类型的映射（时间精度最大 7）；`design/catalog.rs`：类型目录和能力；`design/sql.rs`、`migrate/*`、`compare/normalize.rs`：SqlServer 分支（设计器和迁移报 `sqlserver.later`）。
- `session.rs`、`lib.rs`：`explain_cleanup`（执行计划后执行 `SET SHOWPLAN_ALL OFF`）；`ops.rs`：运维模板。
- `meta/mod.rs`：`MetaKey::Catalogs`、`MetaArgs.catalog`；`meta/template.rs`；模板 `sql/sqlserver.sql`（catalogs、schemas、tables、views、sequences、routines、synonyms、columns、keys、checks、indexes、triggers、view_definition、routine_source、referencing_keys、dependent_views、grants、table_storage）和 `sql/ops/sqlserver.sql`；快照测试。
- `dblens-service/src/policy.rs`：`classify_batch`（MsSqlDialect 拆成语句，取最危险的一条；解析不了时跳过字符串、方括号和注释扫描关键字）；KILL、SHUTDOWN、DBCC、RESTORE、ALTER DATABASE 一律确认。
- 后端词条加 `sqlserver.later`。

**Files:**
- Modify: `crates/dblens-core/locales/app.json`
- Create: `crates/dblens-dialect/sql/ops/sqlserver.sql`
- Create: `crates/dblens-dialect/sql/sqlserver.sql`
- Modify: `crates/dblens-dialect/src/compare/normalize.rs`
- Modify: `crates/dblens-dialect/src/ddl.rs`
- Modify: `crates/dblens-dialect/src/design/catalog.rs`
- Modify: `crates/dblens-dialect/src/design/rebuild.rs`
- Modify: `crates/dblens-dialect/src/design/sql.rs`
- Modify: `crates/dblens-dialect/src/dml.rs`
- Modify: `crates/dblens-dialect/src/ident.rs`
- Modify: `crates/dblens-dialect/src/lib.rs`
- Modify: `crates/dblens-dialect/src/literal.rs`
- Modify: `crates/dblens-dialect/src/meta/mod.rs`
- Modify: `crates/dblens-dialect/src/meta/template.rs`
- Modify: `crates/dblens-dialect/src/migrate/data.rs`
- Modify: `crates/dblens-dialect/src/migrate/expr.rs`
- Modify: `crates/dblens-dialect/src/migrate/mod.rs`
- Modify: `crates/dblens-dialect/src/migrate/names.rs`
- Modify: `crates/dblens-dialect/src/migrate/script.rs`
- Modify: `crates/dblens-dialect/src/migrate/types.rs`
- Modify: `crates/dblens-dialect/src/ops.rs`
- Modify: `crates/dblens-dialect/src/session.rs`
- Modify: `crates/dblens-dialect/src/split.rs`
- Modify: `crates/dblens-dialect/src/types.rs`
- Modify: `crates/dblens-dialect/src/variant.rs`
- Modify: `crates/dblens-dialect/tests/meta_snapshots.rs`
- Create: `crates/dblens-dialect/tests/snapshots/meta_snapshots__sqlserver.snap`
- Modify: `crates/dblens-service/src/policy.rs`

**Interfaces:**
- Consumes：现有的 `Dialect`、元数据模板机制、`Policy`。
- Produces（`dblens_dialect`）：`Family::SqlServer`、`Variant::SqlServer`、`MetaKey::Catalogs`、`MetaArgs { catalog: Option<String>, .. }`、`split::MAX_GO_COUNT`、`Dialect::explain_cleanup(&self) -> Option<&'static str>`、`session::explain_cleanup(Family)`；
  `dblens_service::policy` 对 `Family::SqlServer` 的分类。

- [ ] **Step 1: 修改 `crates/dblens-dialect/tests/meta_snapshots.rs`**

```diff
--- a/crates/dblens-dialect/tests/meta_snapshots.rs
+++ b/crates/dblens-dialect/tests/meta_snapshots.rs
@@ -5,7 +5,7 @@ use dblens_dialect::meta::{MetaArgs, MetaKey};
 use dblens_dialect::{Dialect, Family, Variant};
 
 /// 有自己的模板覆盖文件的变体（PolarDB、GaussDB、HighGo 沿用家族模板）。
-const VARIANTS: [Variant; 9] = [
+const VARIANTS: [Variant; 10] = [
     Variant::MySql,
     Variant::MariaDb,
     Variant::TiDb,
@@ -15,11 +15,13 @@ const VARIANTS: [Variant; 9] = [
     Variant::OpenGauss,
     Variant::Oracle,
     Variant::Dameng,
+    Variant::SqlServer,
 ];
 
 #[test]
 fn rendered_metadata_queries() {
     let full = MetaArgs {
+        catalog: None,
         schema: Some("HR".into()),
         table: Some("EMP".into()),
         name: Some("EMP".into()),
@@ -44,14 +46,18 @@ fn every_family_covers_the_common_keys() {
         let d = Dialect::new(v);
         for key in MetaKey::ALL {
             let optional = match key {
+                // 只有 SQL Server 有数据库这一层
+                MetaKey::Catalogs => v.family() != Family::SqlServer,
                 MetaKey::MaterializedViews
                 | MetaKey::Synonyms
                 | MetaKey::Sequences
                 | MetaKey::VariantProbe => true,
                 MetaKey::DependentViews | MetaKey::Grants => v.family() == Family::MySql,
                 MetaKey::TriggerDdl => v != Variant::Oracle,
-                // OceanBase 没有 information_schema.KEYWORDS
-                MetaKey::ReservedWords => matches!(v, Variant::MariaDb | Variant::OceanBase),
+                // OceanBase 没有 information_schema.KEYWORDS；SQL Server 没有可查的保留字表（用 ident.rs 的清单）
+                MetaKey::ReservedWords => {
+                    matches!(v, Variant::MariaDb | Variant::OceanBase | Variant::SqlServer)
+                }
                 _ => false,
             };
             assert!(optional || d.has_meta(key), "{v:?} 缺少 {key:?} 模板");
@@ -63,6 +69,7 @@ fn every_family_covers_the_common_keys() {
 #[test]
 fn legacy_version_branches_render() {
     let full = MetaArgs {
+        catalog: None,
         schema: Some("HR".into()),
         table: Some("EMP".into()),
         name: Some("EMP".into()),
```

- [ ] **Step 2: 新建 `crates/dblens-dialect/tests/snapshots/meta_snapshots__sqlserver.snap`**

```text
---
source: crates/dblens-dialect/tests/meta_snapshots.rs
expression: out
---
-- [catalogs] [String("%员工%")]
-- Online databases the login can open; master/tempdb/model/msdb (database_id <= 4) are system.
SELECT d.name AS name,
       CAST(CASE WHEN d.database_id <= 4 THEN 1 ELSE 0 END AS bit) AS is_system
FROM sys.databases d
WHERE d.state = 0
  AND HAS_DBACCESS(d.name) = 1
  AND UPPER(d.name) LIKE UPPER(@P1) ESCAPE '!'
ORDER BY CASE WHEN d.database_id <= 4 THEN 1 ELSE 0 END, d.name

-- [schemas] []
-- sys, INFORMATION_SCHEMA, guest and the fixed database-role schemas (db_owner … db_denydatawriter) are system.
SELECT s.name AS name,
       CAST(CASE WHEN s.name IN ('sys', 'INFORMATION_SCHEMA', 'guest') OR s.schema_id BETWEEN 16384 AND 16399
                 THEN 1 ELSE 0 END AS bit) AS is_system
FROM sys.schemas s
ORDER BY s.name

-- [tables] [String("HR"), String("EMP"), String("%员工%"), String("%员工%")]
SELECT t.name AS name,
       CAST(ep.value AS nvarchar(max)) AS comment,
       CAST(NULL AS nvarchar(max)) AS partition
FROM sys.tables t
LEFT JOIN sys.extended_properties ep
       ON ep.class = 1 AND ep.major_id = t.object_id AND ep.minor_id = 0 AND ep.name = 'MS_Description'
WHERE t.schema_id = SCHEMA_ID(@P1)
  AND t.is_ms_shipped = 0
  AND t.name = @P2
  AND (UPPER(t.name) LIKE UPPER(@P3) ESCAPE '!'
       OR UPPER(CAST(ep.value AS nvarchar(max))) LIKE UPPER(@P4) ESCAPE '!')
ORDER BY t.name

-- [views] [String("HR"), String("%员工%"), String("%员工%")]
SELECT v.name AS name,
       CAST(ep.value AS nvarchar(max)) AS comment
FROM sys.views v
LEFT JOIN sys.extended_properties ep
       ON ep.class = 1 AND ep.major_id = v.object_id AND ep.minor_id = 0 AND ep.name = 'MS_Description'
WHERE v.schema_id = SCHEMA_ID(@P1)
  AND v.is_ms_shipped = 0
  AND (UPPER(v.name) LIKE UPPER(@P2) ESCAPE '!'
       OR UPPER(CAST(ep.value AS nvarchar(max))) LIKE UPPER(@P3) ESCAPE '!')
ORDER BY v.name

-- [sequences] [String("HR"), String("%员工%")]
-- start_value etc. are sql_variant: cast to text.
SELECT s.name AS name,
       CAST(s.start_value AS nvarchar(40)) AS start_value,
       CAST(s.increment AS nvarchar(40)) AS increment,
       CAST(s.minimum_value AS nvarchar(40)) AS min_value,
       CAST(s.maximum_value AS nvarchar(40)) AS max_value,
       s.is_cycling AS cycle,
       CASE WHEN s.is_cached = 1 THEN s.cache_size END AS cache
FROM sys.sequences s
WHERE s.schema_id = SCHEMA_ID(@P1)
  AND UPPER(s.name) LIKE UPPER(@P2) ESCAPE '!'
ORDER BY s.name

-- [routines] [String("HR"), String("%员工%"), String("%员工%")]
-- P/PC = procedures (T-SQL / CLR); FN/IF/TF/FS/FT = scalar, inline, table-valued and CLR functions.
SELECT o.name AS name,
       CASE WHEN o.type IN ('P', 'PC') THEN 'PROCEDURE' ELSE 'FUNCTION' END AS kind,
       CAST(ep.value AS nvarchar(max)) AS comment
FROM sys.objects o
LEFT JOIN sys.extended_properties ep
       ON ep.class = 1 AND ep.major_id = o.object_id AND ep.minor_id = 0 AND ep.name = 'MS_Description'
WHERE o.schema_id = SCHEMA_ID(@P1)
  AND o.type IN ('P', 'PC', 'FN', 'IF', 'TF', 'FS', 'FT')
  AND o.is_ms_shipped = 0
  AND (UPPER(o.name) LIKE UPPER(@P2) ESCAPE '!'
       OR UPPER(CAST(ep.value AS nvarchar(max))) LIKE UPPER(@P3) ESCAPE '!')
ORDER BY o.name

-- [synonyms] [String("HR"), String("%员工%")]
-- base_object_name is [server].[database].[schema].[object] with the leading parts optional.
SELECT s.name AS name,
       PARSENAME(s.base_object_name, 2) AS target_schema,
       PARSENAME(s.base_object_name, 1) AS target_name,
       PARSENAME(s.base_object_name, 4) AS db_link
FROM sys.synonyms s
WHERE s.schema_id = SCHEMA_ID(@P1)
  AND UPPER(s.name) LIKE UPPER(@P2) ESCAPE '!'
ORDER BY s.name

-- [columns] [String("HR"), String("EMP")]
-- type_text rebuilds the declared type: n(var)char lengths are in bytes in sys.columns (halved), -1 is max.
SELECT o.name AS table_name,
       c.name AS name,
       CAST(ROW_NUMBER() OVER (PARTITION BY o.object_id ORDER BY c.column_id) AS int) AS ordinal,
       ty.name + CASE
         WHEN ty.name IN ('varchar', 'char', 'varbinary', 'binary')
           THEN '(' + CASE WHEN c.max_length = -1 THEN 'max' ELSE CAST(c.max_length AS varchar(10)) END + ')'
         WHEN ty.name IN ('nvarchar', 'nchar')
           THEN '(' + CASE WHEN c.max_length = -1 THEN 'max' ELSE CAST(c.max_length / 2 AS varchar(10)) END + ')'
         WHEN ty.name IN ('decimal', 'numeric')
           THEN '(' + CAST(c.precision AS varchar(10)) + ',' + CAST(c.scale AS varchar(10)) + ')'
         WHEN ty.name IN ('datetime2', 'time', 'datetimeoffset')
           THEN '(' + CAST(c.scale AS varchar(10)) + ')'
         ELSE '' END AS type_text,
       c.is_nullable AS nullable,
       dc.definition AS default_value,
       CAST(ep.value AS nvarchar(max)) AS comment,
       c.is_identity AS auto_increment,
       cc.definition AS generated
FROM sys.objects o
JOIN sys.columns c ON c.object_id = o.object_id
JOIN sys.types ty ON ty.user_type_id = c.user_type_id
LEFT JOIN sys.default_constraints dc ON dc.object_id = c.default_object_id
LEFT JOIN sys.computed_columns cc ON cc.object_id = c.object_id AND cc.column_id = c.column_id
LEFT JOIN sys.extended_properties ep
       ON ep.class = 1 AND ep.major_id = c.object_id AND ep.minor_id = c.column_id AND ep.name = 'MS_Description'
WHERE o.schema_id = SCHEMA_ID(@P1)
  AND o.type IN ('U', 'V')
  AND o.is_ms_shipped = 0
  AND o.name = @P2
ORDER BY o.name, c.column_id

-- [keys] [String("HR"), String("EMP"), String("HR"), String("EMP")]
-- Primary keys and unique constraints (through their index), then foreign keys.
SELECT t.name AS table_name,
       kc.name AS constraint_name,
       CASE kc.type WHEN 'PK' THEN 'P' ELSE 'U' END AS constraint_type,
       c.name AS column_name,
       CAST(ic.key_ordinal AS int) AS position,
       CAST(NULL AS sysname) AS ref_schema,
       CAST(NULL AS sysname) AS ref_table,
       CAST(NULL AS sysname) AS ref_column,
       CAST(NULL AS nvarchar(20)) AS on_delete,
       CAST(NULL AS nvarchar(20)) AS on_update
FROM sys.key_constraints kc
JOIN sys.tables t ON t.object_id = kc.parent_object_id
JOIN sys.index_columns ic ON ic.object_id = kc.parent_object_id AND ic.index_id = kc.unique_index_id
JOIN sys.columns c ON c.object_id = ic.object_id AND c.column_id = ic.column_id
WHERE t.schema_id = SCHEMA_ID(@P1)
  AND ic.is_included_column = 0
  AND t.name = @P2
UNION ALL
SELECT t.name,
       fk.name,
       'F',
       c.name,
       CAST(fkc.constraint_column_id AS int),
       SCHEMA_NAME(rt.schema_id),
       rt.name,
       rc.name,
       REPLACE(fk.delete_referential_action_desc, '_', ' '),
       REPLACE(fk.update_referential_action_desc, '_', ' ')
FROM sys.foreign_keys fk
JOIN sys.tables t ON t.object_id = fk.parent_object_id
JOIN sys.foreign_key_columns fkc ON fkc.constraint_object_id = fk.object_id
JOIN sys.columns c ON c.object_id = fkc.parent_object_id AND c.column_id = fkc.parent_column_id
JOIN sys.tables rt ON rt.object_id = fk.referenced_object_id
JOIN sys.columns rc ON rc.object_id = fkc.referenced_object_id AND rc.column_id = fkc.referenced_column_id
WHERE t.schema_id = SCHEMA_ID(@P3)
  AND t.name = @P4
ORDER BY 1, 2, 5

-- [checks] [String("HR"), String("EMP")]
SELECT t.name AS table_name,
       ck.name AS constraint_name,
       ck.definition AS expression
FROM sys.check_constraints ck
JOIN sys.tables t ON t.object_id = ck.parent_object_id
WHERE t.schema_id = SCHEMA_ID(@P1)
  AND t.name = @P2
ORDER BY t.name, ck.name

-- [indexes] [String("HR"), String("EMP")]
-- Key columns only (INCLUDE columns are skipped); heaps (index_id 0) and indexes backing PK/UNIQUE are excluded.
-- method is CLUSTERED / NONCLUSTERED / CLUSTERED COLUMNSTORE / …
SELECT t.name AS table_name,
       i.name AS index_name,
       i.is_unique AS is_unique,
       i.type_desc AS method,
       c.name AS column_expr,
       CAST(ic.key_ordinal AS int) AS position,
       ic.is_descending_key AS descending
FROM sys.indexes i
JOIN sys.tables t ON t.object_id = i.object_id
JOIN sys.index_columns ic ON ic.object_id = i.object_id AND ic.index_id = i.index_id
JOIN sys.columns c ON c.object_id = ic.object_id AND c.column_id = ic.column_id
WHERE t.schema_id = SCHEMA_ID(@P1)
  AND i.index_id > 0
  AND i.is_primary_key = 0
  AND i.is_unique_constraint = 0
  AND i.is_hypothetical = 0
  AND ic.is_included_column = 0
  AND ic.key_ordinal > 0
  AND t.name = @P2
ORDER BY t.name, i.name, ic.key_ordinal

-- [triggers] [String("HR"), String("EMP")]
-- DML triggers on tables and views; definition is the full CREATE TRIGGER text.
SELECT o.name AS table_name,
       tr.name AS name,
       CASE WHEN tr.is_instead_of_trigger = 1 THEN 'INSTEAD OF' ELSE 'AFTER' END AS timing,
       (SELECT STRING_AGG(te.type_desc, ',') FROM sys.trigger_events te WHERE te.object_id = tr.object_id) AS events,
       m.definition AS definition
FROM sys.triggers tr
JOIN sys.objects o ON o.object_id = tr.parent_id
LEFT JOIN sys.sql_modules m ON m.object_id = tr.object_id
WHERE o.schema_id = SCHEMA_ID(@P1)
  AND tr.parent_class = 1
  AND tr.is_ms_shipped = 0
  AND o.name = @P2
ORDER BY o.name, tr.name

-- [view_definition] [String("HR"), String("EMP")]
-- The module text is the full CREATE VIEW statement.
SELECT m.definition AS definition
FROM sys.views v
JOIN sys.sql_modules m ON m.object_id = v.object_id
WHERE v.schema_id = SCHEMA_ID(@P1)
  AND v.name = @P2

-- [routine_source] [String("HR"), String("EMP"), String("FUNCTION")]
-- One row per routine: the full CREATE PROCEDURE / FUNCTION text.
SELECT 1 AS line,
       m.definition AS text
FROM sys.objects o
JOIN sys.sql_modules m ON m.object_id = o.object_id
WHERE o.schema_id = SCHEMA_ID(@P1)
  AND o.name = @P2
  AND (CASE WHEN o.type IN ('P', 'PC') THEN 'PROCEDURE' ELSE 'FUNCTION' END) = @P3

-- [referencing_keys] [String("HR"), String("EMP")]
-- Foreign keys of OTHER tables that reference this table.
SELECT SCHEMA_NAME(t.schema_id) AS table_schema,
       t.name AS table_name,
       fk.name AS constraint_name,
       c.name AS column_name,
       CAST(fkc.constraint_column_id AS int) AS position,
       rc.name AS ref_column,
       REPLACE(fk.delete_referential_action_desc, '_', ' ') AS on_delete,
       REPLACE(fk.update_referential_action_desc, '_', ' ') AS on_update
FROM sys.foreign_keys fk
JOIN sys.tables t ON t.object_id = fk.parent_object_id
JOIN sys.foreign_key_columns fkc ON fkc.constraint_object_id = fk.object_id
JOIN sys.columns c ON c.object_id = fkc.parent_object_id AND c.column_id = fkc.parent_column_id
JOIN sys.columns rc ON rc.object_id = fkc.referenced_object_id AND rc.column_id = fkc.referenced_column_id
WHERE fk.referenced_object_id = OBJECT_ID(QUOTENAME(@P1) + '.' + QUOTENAME(@P2))
  AND fk.parent_object_id <> fk.referenced_object_id
ORDER BY 1, 2, 3, 5

-- [dependent_views] [String("HR"), String("EMP")]
-- Views whose definition references this table (schema-bound or not).
SELECT DISTINCT SCHEMA_NAME(v.schema_id) AS schema_name,
       v.name AS name,
       'VIEW' AS kind
FROM sys.sql_expression_dependencies d
JOIN sys.views v ON v.object_id = d.referencing_id
WHERE d.referenced_id = OBJECT_ID(QUOTENAME(@P1) + '.' + QUOTENAME(@P2))
ORDER BY 1, 2

-- [grants] [String("HR"), String("EMP")]
-- Object-level permissions granted (G) or granted WITH GRANT OPTION (W); denials are not grants.
SELECT pr.name AS grantee,
       p.permission_name AS privilege,
       CAST(CASE WHEN p.state = 'W' THEN 1 ELSE 0 END AS bit) AS grantable
FROM sys.database_permissions p
JOIN sys.database_principals pr ON pr.principal_id = p.grantee_principal_id
WHERE p.class = 1
  AND p.minor_id = 0
  AND p.state IN ('G', 'W')
  AND p.major_id = OBJECT_ID(QUOTENAME(@P1) + '.' + QUOTENAME(@P2))
ORDER BY 1, 2

-- [table_storage] [String("HR"), String("EMP")]
-- tablespace = filegroup of the heap / clustered index; row_count from partition metadata (no scan).
SELECT CAST(NULL AS nvarchar(10)) AS engine,
       CAST(NULL AS nvarchar(10)) AS charset,
       CAST(NULL AS nvarchar(10)) AS collation,
       (SELECT TOP 1 ds.name FROM sys.indexes i JOIN sys.data_spaces ds ON ds.data_space_id = i.data_space_id
        WHERE i.object_id = t.object_id AND i.index_id IN (0, 1)) AS tablespace,
       (SELECT SUM(p.rows) FROM sys.partitions p WHERE p.object_id = t.object_id AND p.index_id IN (0, 1)) AS row_count
FROM sys.tables t
WHERE t.schema_id = SCHEMA_ID(@P1)
  AND t.name = @P2
```

- [ ] **Step 3: 修改 `crates/dblens-core/locales/app.json`**

```diff
--- a/crates/dblens-core/locales/app.json
+++ b/crates/dblens-core/locales/app.json
@@ -298,6 +298,7 @@
   "session.disconnected": {"zh-CN": "连接已断开", "en": "The connection is closed"},
   "session.explain_single": {"zh-CN": "执行计划只能针对一条语句", "en": "An execution plan can only be shown for a single statement"},
   "session.task_gone": {"zh-CN": "会话任务已退出", "en": "The session task has exited"},
+  "sqlserver.later": {"zh-CN": "SQL Server 的这项功能将在后续版本支持", "en": "This feature is not yet available for SQL Server"},
   "sqlfile.denied": {"zh-CN": "第 %{line} 行的语句被执行策略拒绝，没有执行任何语句：%{reason}\n%{text}", "en": "The statement at line %{line} was denied by the execution policy; nothing was executed: %{reason}\n%{text}"},
   "sqlfile.history": {"zh-CN": "执行 SQL 文件 %{file}：%{done} 条，失败 %{failed} 条", "en": "Run SQL file %{file}: %{done} statements, %{failed} failed"},
   "sqlfile.log_failed": {"zh-CN": "第 %{n} 条语句失败（第 %{line} 行）：", "en": "Statement %{n} failed (line %{line}):"},
```

- [ ] **Step 4: 新建 `crates/dblens-dialect/sql/ops/sqlserver.sql`**

```sql
-- SQL Server ops queries (M6a). Dynamic management views need VIEW SERVER STATE (VIEW SERVER PERFORMANCE STATE on
-- 2022+); without it the database's own permission error is shown.

-- [sessions]
-- name.zh-CN: 会话列表
-- name.en: Sessions
-- category: session
SELECT s.session_id, s.login_name AS [user], s.host_name AS host, DB_NAME(s.database_id) AS db,
       s.program_name AS application, s.status AS state,
       DATEDIFF(SECOND, COALESCE(r.start_time, s.last_request_start_time), SYSDATETIME()) AS seconds,
       LEFT(t.text, 200) AS current_sql,
       'KILL ' + CAST(s.session_id AS varchar(10)) AS kill_sql
FROM sys.dm_exec_sessions s
LEFT JOIN sys.dm_exec_requests r ON r.session_id = s.session_id
OUTER APPLY sys.dm_exec_sql_text(r.sql_handle) t
WHERE s.is_user_process = 1
ORDER BY s.last_request_start_time DESC

-- [running]
-- name.zh-CN: 正在执行的语句
-- name.en: Running Statements
-- category: session
SELECT r.session_id, s.login_name AS [user], s.host_name AS host, DB_NAME(r.database_id) AS db,
       DATEDIFF(SECOND, r.start_time, SYSDATETIME()) AS seconds,
       r.status, r.wait_type, r.blocking_session_id,
       LEFT(t.text, 500) AS current_sql,
       'KILL ' + CAST(r.session_id AS varchar(10)) AS kill_sql
FROM sys.dm_exec_requests r
JOIN sys.dm_exec_sessions s ON s.session_id = r.session_id
OUTER APPLY sys.dm_exec_sql_text(r.sql_handle) t
WHERE s.is_user_process = 1 AND r.session_id <> @@SPID
ORDER BY r.start_time

-- [locks]
-- name.zh-CN: 锁等待
-- name.en: Lock Waits
-- category: lock
SELECT w.session_id AS waiting_session, LEFT(wt.text, 200) AS waiting_sql,
       w.blocking_session_id AS blocking_session, LEFT(bt.text, 200) AS blocking_sql,
       w.wait_type, w.wait_time / 1000 AS wait_seconds, w.wait_resource,
       'KILL ' + CAST(w.blocking_session_id AS varchar(10)) AS kill_sql
FROM sys.dm_exec_requests w
OUTER APPLY sys.dm_exec_sql_text(w.sql_handle) wt
LEFT JOIN sys.dm_exec_connections bc ON bc.session_id = w.blocking_session_id
OUTER APPLY sys.dm_exec_sql_text(bc.most_recent_sql_handle) bt
WHERE w.blocking_session_id <> 0
ORDER BY w.wait_time DESC

-- [storage]
-- name.zh-CN: 各库大小
-- name.en: Database Sizes
-- category: storage
SELECT d.name AS db,
       CAST(SUM(CASE WHEN f.type = 0 THEN f.size END) * 8 / 1024.0 AS decimal(18, 2)) AS data_mb,
       CAST(SUM(CASE WHEN f.type = 1 THEN f.size END) * 8 / 1024.0 AS decimal(18, 2)) AS log_mb,
       CAST(SUM(f.size) * 8 / 1024.0 AS decimal(18, 2)) AS size_mb
FROM sys.databases d
JOIN sys.master_files f ON f.database_id = d.database_id
GROUP BY d.name
ORDER BY SUM(f.size) DESC

-- [largest_tables]
-- name.zh-CN: 占用最大的表（当前库）
-- name.en: Largest Tables (current database)
-- category: storage
SELECT TOP 20 SCHEMA_NAME(t.schema_id) AS [schema], t.name AS table_name,
       SUM(CASE WHEN ps.index_id IN (0, 1) THEN ps.row_count END) AS est_rows,
       CAST(SUM(CASE WHEN ps.index_id IN (0, 1) THEN ps.used_page_count END) * 8 / 1024.0 AS decimal(18, 2)) AS data_mb,
       CAST(SUM(CASE WHEN ps.index_id > 1 THEN ps.used_page_count END) * 8 / 1024.0 AS decimal(18, 2)) AS index_mb,
       CAST(SUM(ps.reserved_page_count) * 8 / 1024.0 AS decimal(18, 2)) AS total_mb
FROM sys.dm_db_partition_stats ps
JOIN sys.tables t ON t.object_id = ps.object_id
WHERE t.is_ms_shipped = 0
GROUP BY t.schema_id, t.name
ORDER BY SUM(ps.reserved_page_count) DESC

-- [slow_sql]
-- name.zh-CN: 慢 SQL
-- name.en: Slow SQL
-- category: sql
SELECT TOP 20 LEFT(t.text, 300) AS sql_text, qs.execution_count AS executions,
       CAST(qs.total_elapsed_time / qs.execution_count / 1000.0 AS decimal(18, 2)) AS avg_ms,
       CAST(qs.max_elapsed_time / 1000.0 AS decimal(18, 2)) AS max_ms,
       CAST(qs.total_elapsed_time / 1000.0 AS decimal(18, 2)) AS total_ms,
       qs.total_rows AS [rows]
FROM sys.dm_exec_query_stats qs
CROSS APPLY sys.dm_exec_sql_text(qs.sql_handle) t
ORDER BY qs.total_elapsed_time / qs.execution_count DESC
```

- [ ] **Step 5: 新建 `crates/dblens-dialect/sql/sqlserver.sql`**

```sql
-- SQL Server metadata queries (catalog views only). Base: SQL Server 2017+ (STRING_AGG); tested on 2022.
-- Every query runs in the CURRENT database: the service switches with USE [catalog] first (catalogs lists them).
-- Booleans are returned as bit. Microsoft-shipped objects are excluded.
-- Comments are the MS_Description extended property.

-- [catalogs]
-- Online databases the login can open; master/tempdb/model/msdb (database_id <= 4) are system.
SELECT d.name AS name,
       CAST(CASE WHEN d.database_id <= 4 THEN 1 ELSE 0 END AS bit) AS is_system
FROM sys.databases d
WHERE d.state = 0
  AND HAS_DBACCESS(d.name) = 1
{% if filter %}
  AND UPPER(d.name) LIKE UPPER({{ bind("filter") }}) ESCAPE '!'
{% endif %}
ORDER BY CASE WHEN d.database_id <= 4 THEN 1 ELSE 0 END, d.name

-- [schemas]
-- sys, INFORMATION_SCHEMA, guest and the fixed database-role schemas (db_owner … db_denydatawriter) are system.
SELECT s.name AS name,
       CAST(CASE WHEN s.name IN ('sys', 'INFORMATION_SCHEMA', 'guest') OR s.schema_id BETWEEN 16384 AND 16399
                 THEN 1 ELSE 0 END AS bit) AS is_system
FROM sys.schemas s
ORDER BY s.name

-- [tables]
SELECT t.name AS name,
       CAST(ep.value AS nvarchar(max)) AS comment,
       CAST(NULL AS nvarchar(max)) AS partition
FROM sys.tables t
LEFT JOIN sys.extended_properties ep
       ON ep.class = 1 AND ep.major_id = t.object_id AND ep.minor_id = 0 AND ep.name = 'MS_Description'
WHERE t.schema_id = SCHEMA_ID({{ bind("schema") }})
  AND t.is_ms_shipped = 0
{% if table %}
  AND t.name = {{ bind("table") }}
{% endif %}
{% if filter %}
  AND (UPPER(t.name) LIKE UPPER({{ bind("filter") }}) ESCAPE '!'
       OR UPPER(CAST(ep.value AS nvarchar(max))) LIKE UPPER({{ bind("filter") }}) ESCAPE '!')
{% endif %}
ORDER BY t.name

-- [views]
SELECT v.name AS name,
       CAST(ep.value AS nvarchar(max)) AS comment
FROM sys.views v
LEFT JOIN sys.extended_properties ep
       ON ep.class = 1 AND ep.major_id = v.object_id AND ep.minor_id = 0 AND ep.name = 'MS_Description'
WHERE v.schema_id = SCHEMA_ID({{ bind("schema") }})
  AND v.is_ms_shipped = 0
{% if filter %}
  AND (UPPER(v.name) LIKE UPPER({{ bind("filter") }}) ESCAPE '!'
       OR UPPER(CAST(ep.value AS nvarchar(max))) LIKE UPPER({{ bind("filter") }}) ESCAPE '!')
{% endif %}
ORDER BY v.name

-- [sequences]
-- start_value etc. are sql_variant: cast to text.
SELECT s.name AS name,
       CAST(s.start_value AS nvarchar(40)) AS start_value,
       CAST(s.increment AS nvarchar(40)) AS increment,
       CAST(s.minimum_value AS nvarchar(40)) AS min_value,
       CAST(s.maximum_value AS nvarchar(40)) AS max_value,
       s.is_cycling AS cycle,
       CASE WHEN s.is_cached = 1 THEN s.cache_size END AS cache
FROM sys.sequences s
WHERE s.schema_id = SCHEMA_ID({{ bind("schema") }})
{% if filter %}
  AND UPPER(s.name) LIKE UPPER({{ bind("filter") }}) ESCAPE '!'
{% endif %}
ORDER BY s.name

-- [routines]
-- P/PC = procedures (T-SQL / CLR); FN/IF/TF/FS/FT = scalar, inline, table-valued and CLR functions.
SELECT o.name AS name,
       CASE WHEN o.type IN ('P', 'PC') THEN 'PROCEDURE' ELSE 'FUNCTION' END AS kind,
       CAST(ep.value AS nvarchar(max)) AS comment
FROM sys.objects o
LEFT JOIN sys.extended_properties ep
       ON ep.class = 1 AND ep.major_id = o.object_id AND ep.minor_id = 0 AND ep.name = 'MS_Description'
WHERE o.schema_id = SCHEMA_ID({{ bind("schema") }})
  AND o.type IN ('P', 'PC', 'FN', 'IF', 'TF', 'FS', 'FT')
  AND o.is_ms_shipped = 0
{% if filter %}
  AND (UPPER(o.name) LIKE UPPER({{ bind("filter") }}) ESCAPE '!'
       OR UPPER(CAST(ep.value AS nvarchar(max))) LIKE UPPER({{ bind("filter") }}) ESCAPE '!')
{% endif %}
ORDER BY o.name

-- [synonyms]
-- base_object_name is [server].[database].[schema].[object] with the leading parts optional.
SELECT s.name AS name,
       PARSENAME(s.base_object_name, 2) AS target_schema,
       PARSENAME(s.base_object_name, 1) AS target_name,
       PARSENAME(s.base_object_name, 4) AS db_link
FROM sys.synonyms s
WHERE s.schema_id = SCHEMA_ID({{ bind("schema") }})
{% if filter %}
  AND UPPER(s.name) LIKE UPPER({{ bind("filter") }}) ESCAPE '!'
{% endif %}
ORDER BY s.name

-- [columns]
-- type_text rebuilds the declared type: n(var)char lengths are in bytes in sys.columns (halved), -1 is max.
SELECT o.name AS table_name,
       c.name AS name,
       CAST(ROW_NUMBER() OVER (PARTITION BY o.object_id ORDER BY c.column_id) AS int) AS ordinal,
       ty.name + CASE
         WHEN ty.name IN ('varchar', 'char', 'varbinary', 'binary')
           THEN '(' + CASE WHEN c.max_length = -1 THEN 'max' ELSE CAST(c.max_length AS varchar(10)) END + ')'
         WHEN ty.name IN ('nvarchar', 'nchar')
           THEN '(' + CASE WHEN c.max_length = -1 THEN 'max' ELSE CAST(c.max_length / 2 AS varchar(10)) END + ')'
         WHEN ty.name IN ('decimal', 'numeric')
           THEN '(' + CAST(c.precision AS varchar(10)) + ',' + CAST(c.scale AS varchar(10)) + ')'
         WHEN ty.name IN ('datetime2', 'time', 'datetimeoffset')
           THEN '(' + CAST(c.scale AS varchar(10)) + ')'
         ELSE '' END AS type_text,
       c.is_nullable AS nullable,
       dc.definition AS default_value,
       CAST(ep.value AS nvarchar(max)) AS comment,
       c.is_identity AS auto_increment,
       cc.definition AS generated
FROM sys.objects o
JOIN sys.columns c ON c.object_id = o.object_id
JOIN sys.types ty ON ty.user_type_id = c.user_type_id
LEFT JOIN sys.default_constraints dc ON dc.object_id = c.default_object_id
LEFT JOIN sys.computed_columns cc ON cc.object_id = c.object_id AND cc.column_id = c.column_id
LEFT JOIN sys.extended_properties ep
       ON ep.class = 1 AND ep.major_id = c.object_id AND ep.minor_id = c.column_id AND ep.name = 'MS_Description'
WHERE o.schema_id = SCHEMA_ID({{ bind("schema") }})
  AND o.type IN ('U', 'V')
  AND o.is_ms_shipped = 0
{% if table %}
  AND o.name = {{ bind("table") }}
{% endif %}
ORDER BY o.name, c.column_id

-- [keys]
-- Primary keys and unique constraints (through their index), then foreign keys.
SELECT t.name AS table_name,
       kc.name AS constraint_name,
       CASE kc.type WHEN 'PK' THEN 'P' ELSE 'U' END AS constraint_type,
       c.name AS column_name,
       CAST(ic.key_ordinal AS int) AS position,
       CAST(NULL AS sysname) AS ref_schema,
       CAST(NULL AS sysname) AS ref_table,
       CAST(NULL AS sysname) AS ref_column,
       CAST(NULL AS nvarchar(20)) AS on_delete,
       CAST(NULL AS nvarchar(20)) AS on_update
FROM sys.key_constraints kc
JOIN sys.tables t ON t.object_id = kc.parent_object_id
JOIN sys.index_columns ic ON ic.object_id = kc.parent_object_id AND ic.index_id = kc.unique_index_id
JOIN sys.columns c ON c.object_id = ic.object_id AND c.column_id = ic.column_id
WHERE t.schema_id = SCHEMA_ID({{ bind("schema") }})
  AND ic.is_included_column = 0
{% if table %}
  AND t.name = {{ bind("table") }}
{% endif %}
UNION ALL
SELECT t.name,
       fk.name,
       'F',
       c.name,
       CAST(fkc.constraint_column_id AS int),
       SCHEMA_NAME(rt.schema_id),
       rt.name,
       rc.name,
       REPLACE(fk.delete_referential_action_desc, '_', ' '),
       REPLACE(fk.update_referential_action_desc, '_', ' ')
FROM sys.foreign_keys fk
JOIN sys.tables t ON t.object_id = fk.parent_object_id
JOIN sys.foreign_key_columns fkc ON fkc.constraint_object_id = fk.object_id
JOIN sys.columns c ON c.object_id = fkc.parent_object_id AND c.column_id = fkc.parent_column_id
JOIN sys.tables rt ON rt.object_id = fk.referenced_object_id
JOIN sys.columns rc ON rc.object_id = fkc.referenced_object_id AND rc.column_id = fkc.referenced_column_id
WHERE t.schema_id = SCHEMA_ID({{ bind("schema") }})
{% if table %}
  AND t.name = {{ bind("table") }}
{% endif %}
ORDER BY 1, 2, 5

-- [checks]
SELECT t.name AS table_name,
       ck.name AS constraint_name,
       ck.definition AS expression
FROM sys.check_constraints ck
JOIN sys.tables t ON t.object_id = ck.parent_object_id
WHERE t.schema_id = SCHEMA_ID({{ bind("schema") }})
{% if table %}
  AND t.name = {{ bind("table") }}
{% endif %}
ORDER BY t.name, ck.name

-- [indexes]
-- Key columns only (INCLUDE columns are skipped); heaps (index_id 0) and indexes backing PK/UNIQUE are excluded.
-- method is CLUSTERED / NONCLUSTERED / CLUSTERED COLUMNSTORE / …
SELECT t.name AS table_name,
       i.name AS index_name,
       i.is_unique AS is_unique,
       i.type_desc AS method,
       c.name AS column_expr,
       CAST(ic.key_ordinal AS int) AS position,
       ic.is_descending_key AS descending
FROM sys.indexes i
JOIN sys.tables t ON t.object_id = i.object_id
JOIN sys.index_columns ic ON ic.object_id = i.object_id AND ic.index_id = i.index_id
JOIN sys.columns c ON c.object_id = ic.object_id AND c.column_id = ic.column_id
WHERE t.schema_id = SCHEMA_ID({{ bind("schema") }})
  AND i.index_id > 0
  AND i.is_primary_key = 0
  AND i.is_unique_constraint = 0
  AND i.is_hypothetical = 0
  AND ic.is_included_column = 0
  AND ic.key_ordinal > 0
{% if table %}
  AND t.name = {{ bind("table") }}
{% endif %}
ORDER BY t.name, i.name, ic.key_ordinal

-- [triggers]
-- DML triggers on tables and views; definition is the full CREATE TRIGGER text.
SELECT o.name AS table_name,
       tr.name AS name,
       CASE WHEN tr.is_instead_of_trigger = 1 THEN 'INSTEAD OF' ELSE 'AFTER' END AS timing,
       (SELECT STRING_AGG(te.type_desc, ',') FROM sys.trigger_events te WHERE te.object_id = tr.object_id) AS events,
       m.definition AS definition
FROM sys.triggers tr
JOIN sys.objects o ON o.object_id = tr.parent_id
LEFT JOIN sys.sql_modules m ON m.object_id = tr.object_id
WHERE o.schema_id = SCHEMA_ID({{ bind("schema") }})
  AND tr.parent_class = 1
  AND tr.is_ms_shipped = 0
{% if table %}
  AND o.name = {{ bind("table") }}
{% endif %}
ORDER BY o.name, tr.name

-- [view_definition]
-- The module text is the full CREATE VIEW statement.
SELECT m.definition AS definition
FROM sys.views v
JOIN sys.sql_modules m ON m.object_id = v.object_id
WHERE v.schema_id = SCHEMA_ID({{ bind("schema") }})
  AND v.name = {{ bind("name") }}

-- [routine_source]
-- One row per routine: the full CREATE PROCEDURE / FUNCTION text.
SELECT 1 AS line,
       m.definition AS text
FROM sys.objects o
JOIN sys.sql_modules m ON m.object_id = o.object_id
WHERE o.schema_id = SCHEMA_ID({{ bind("schema") }})
  AND o.name = {{ bind("name") }}
{% if kind %}
  AND (CASE WHEN o.type IN ('P', 'PC') THEN 'PROCEDURE' ELSE 'FUNCTION' END) = {{ bind("kind") }}
{% endif %}

-- [referencing_keys]
-- Foreign keys of OTHER tables that reference this table.
SELECT SCHEMA_NAME(t.schema_id) AS table_schema,
       t.name AS table_name,
       fk.name AS constraint_name,
       c.name AS column_name,
       CAST(fkc.constraint_column_id AS int) AS position,
       rc.name AS ref_column,
       REPLACE(fk.delete_referential_action_desc, '_', ' ') AS on_delete,
       REPLACE(fk.update_referential_action_desc, '_', ' ') AS on_update
FROM sys.foreign_keys fk
JOIN sys.tables t ON t.object_id = fk.parent_object_id
JOIN sys.foreign_key_columns fkc ON fkc.constraint_object_id = fk.object_id
JOIN sys.columns c ON c.object_id = fkc.parent_object_id AND c.column_id = fkc.parent_column_id
JOIN sys.columns rc ON rc.object_id = fkc.referenced_object_id AND rc.column_id = fkc.referenced_column_id
WHERE fk.referenced_object_id = OBJECT_ID(QUOTENAME({{ bind("schema") }}) + '.' + QUOTENAME({{ bind("table") }}))
  AND fk.parent_object_id <> fk.referenced_object_id
ORDER BY 1, 2, 3, 5

-- [dependent_views]
-- Views whose definition references this table (schema-bound or not).
SELECT DISTINCT SCHEMA_NAME(v.schema_id) AS schema_name,
       v.name AS name,
       'VIEW' AS kind
FROM sys.sql_expression_dependencies d
JOIN sys.views v ON v.object_id = d.referencing_id
WHERE d.referenced_id = OBJECT_ID(QUOTENAME({{ bind("schema") }}) + '.' + QUOTENAME({{ bind("table") }}))
ORDER BY 1, 2

-- [grants]
-- Object-level permissions granted (G) or granted WITH GRANT OPTION (W); denials are not grants.
SELECT pr.name AS grantee,
       p.permission_name AS privilege,
       CAST(CASE WHEN p.state = 'W' THEN 1 ELSE 0 END AS bit) AS grantable
FROM sys.database_permissions p
JOIN sys.database_principals pr ON pr.principal_id = p.grantee_principal_id
WHERE p.class = 1
  AND p.minor_id = 0
  AND p.state IN ('G', 'W')
  AND p.major_id = OBJECT_ID(QUOTENAME({{ bind("schema") }}) + '.' + QUOTENAME({{ bind("table") }}))
ORDER BY 1, 2

-- [table_storage]
-- tablespace = filegroup of the heap / clustered index; row_count from partition metadata (no scan).
SELECT CAST(NULL AS nvarchar(10)) AS engine,
       CAST(NULL AS nvarchar(10)) AS charset,
       CAST(NULL AS nvarchar(10)) AS collation,
       (SELECT TOP 1 ds.name FROM sys.indexes i JOIN sys.data_spaces ds ON ds.data_space_id = i.data_space_id
        WHERE i.object_id = t.object_id AND i.index_id IN (0, 1)) AS tablespace,
       (SELECT SUM(p.rows) FROM sys.partitions p WHERE p.object_id = t.object_id AND p.index_id IN (0, 1)) AS row_count
FROM sys.tables t
WHERE t.schema_id = SCHEMA_ID({{ bind("schema") }})
  AND t.name = {{ bind("table") }}
```

- [ ] **Step 6: 修改 `crates/dblens-dialect/src/compare/normalize.rs`**

```diff
--- a/crates/dblens-dialect/src/compare/normalize.rs
+++ b/crates/dblens-dialect/src/compare/normalize.rs
@@ -259,7 +259,8 @@ fn same_check(a: &str, b: &str) -> bool {
 }
 
 /// 名字是否像库自动生成的：Oracle、达梦的 `SYS_C…`、`CONS…`、`INDEX…`；MySQL 的 `PRIMARY`、`<表>_ibfk_N`、
-/// `<表>_chk_N`；PG 家族的 `<表>_pkey`、`<表>_…_key`、`<表>_…_fkey`、`<表>_…_check`。
+/// `<表>_chk_N`；PG 家族的 `<表>_pkey`、`<表>_…_key`、`<表>_…_fkey`、`<表>_…_check`；
+/// SQL Server 的 `PK__表__哈希`、`UQ__…`、`FK__…`、`CK__…`、`DF__…`。
 pub(super) fn is_generated(d: &Dialect, table: &str, name: &str) -> bool {
     let upper = name.to_ascii_uppercase();
     let digits = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_digit());
@@ -277,6 +278,7 @@ pub(super) fn is_generated(d: &Dialect, table: &str, name: &str) -> bool {
                 || n.strip_prefix(&format!("{t}_ibfk_")).is_some_and(digits)
                 || n.strip_prefix(&format!("{t}_chk_")).is_some_and(digits)
         }
+        Family::SqlServer => ["PK__", "UQ__", "FK__", "CK__", "DF__"].iter().any(|p| upper.starts_with(p)),
         Family::Postgres => {
             let prefix = format!("{}_", table.to_ascii_lowercase());
             let n = name.to_ascii_lowercase();
```

- [ ] **Step 7: 修改 `crates/dblens-dialect/src/ddl.rs`**

```diff
--- a/crates/dblens-dialect/src/ddl.rs
+++ b/crates/dblens-dialect/src/ddl.rs
@@ -114,6 +114,27 @@ impl Dialect {
                 | DdlKind::Procedure => Ok(None),
                 _ => Err(unsupported()),
             },
+            // SQL Server：视图的模块定义就是完整的 CREATE VIEW；同义词由系统视图拼出；表、序列用结构模型，函数、过程用程序源码。
+            // 名字用两段名交给 OBJECT_ID（在当前库中查找，调用方先切到对象所在的库）
+            Family::SqlServer => {
+                let two_part =
+                    Value::String(format!("{}.{}", self.quote_ident(schema), self.quote_ident(name)));
+                match kind {
+                    DdlKind::Table | DdlKind::Sequence | DdlKind::Function | DdlKind::Procedure => Ok(None),
+                    DdlKind::View => query(
+                        "SELECT m.definition AS ddl FROM sys.sql_modules m WHERE m.object_id = OBJECT_ID(@P1)".into(),
+                        vec![two_part],
+                    ),
+                    DdlKind::Synonym => query(
+                        "SELECT N'CREATE SYNONYM ' + QUOTENAME(SCHEMA_NAME(s.schema_id)) + N'.' + QUOTENAME(s.name) \
+                         + N' FOR ' + s.base_object_name + N';' AS ddl \
+                         FROM sys.synonyms s WHERE s.object_id = OBJECT_ID(@P1)"
+                            .into(),
+                        vec![two_part],
+                    ),
+                    _ => Err(unsupported()),
+                }
+            }
         }
     }
 
@@ -121,7 +142,7 @@ impl Dialect {
         self.literal(&Value::String(text.into()))
     }
 
-    /// PG 系的建表语句：列、约束、分区，之后是索引、触发器和注释（`table_extras`），每条以 `;` 结束。
+    /// PG 系、SQL Server 的建表语句：列、约束、分区，之后是索引、触发器和注释（`table_extras`），每条以 `;` 结束。
     pub fn table_ddl(&self, schema: &str, t: &Table) -> Result<String> {
         let mut out = self.table_create(schema, t);
         out += &self.table_extras(schema, t, true)?;
@@ -134,7 +155,19 @@ impl Dialect {
         let mut items: Vec<String> = Vec::new();
         for c in &t.columns {
             let mut s = format!("{} {}", self.ident(&c.name), c.data_type.native);
-            if let Some(g) = &c.generated {
+            if self.family() == Family::SqlServer {
+                // SQL Server 的计算列没有类型和可空性：`名 AS (表达式)`
+                if let Some(g) = &c.generated {
+                    items.push(format!("{} AS ({})", self.ident(&c.name), strip_parens(g)));
+                    continue;
+                }
+                if c.auto_increment {
+                    s += " IDENTITY(1,1)";
+                }
+                if let Some(d) = &c.default {
+                    s += &format!(" DEFAULT {d}");
+                }
+            } else if let Some(g) = &c.generated {
                 s += &format!(" GENERATED ALWAYS AS ({g}) STORED");
             } else if let Some(d) = &c.default {
                 s += &format!(" DEFAULT {d}");
@@ -215,10 +248,40 @@ impl Dialect {
                 cols.join(", ")
             );
         }
+        let sqlserver = self.family() == Family::SqlServer;
         for tr in t.triggers.iter().filter(|_| triggers) {
             if let Some(d) = &tr.definition {
-                out += &format!("\n{};", d.trim_end_matches(';').trim());
+                if sqlserver {
+                    // CREATE TRIGGER 必须是批中的第一条语句
+                    out += &format!("\nGO\n{}\nGO", d.trim());
+                } else {
+                    out += &format!("\n{};", d.trim_end_matches(';').trim());
+                }
+            }
+        }
+        if sqlserver {
+            // SQL Server 的注释是扩展属性 MS_Description
+            let property = |text: &str, column: Option<&str>| -> Result<String> {
+                let mut s = format!(
+                    "\nEXEC sys.sp_addextendedproperty N'MS_Description', {}, N'SCHEMA', {}, N'TABLE', {}",
+                    self.comment_literal(text)?,
+                    self.comment_literal(schema)?,
+                    self.comment_literal(&t.name)?
+                );
+                if let Some(col) = column {
+                    s += &format!(", N'COLUMN', {}", self.comment_literal(col)?);
+                }
+                Ok(s + ";")
+            };
+            if let Some(c) = &t.comment {
+                out += &property(c, None)?;
+            }
+            for c in &t.columns {
+                if let Some(cm) = &c.comment {
+                    out += &property(cm, Some(&c.name))?;
+                }
             }
+            return Ok(out);
         }
         if let Some(c) = &t.comment {
             out += &format!("\nCOMMENT ON TABLE {q} IS {};", self.comment_literal(c)?);
@@ -266,6 +329,33 @@ impl Dialect {
     }
 }
 
+/// 去掉表达式最外层成对的括号（SQL Server 的定义文本自带括号，如 `([a]+[b])`）。
+fn strip_parens(e: &str) -> &str {
+    let e = e.trim();
+    match e.strip_prefix('(').and_then(|r| r.strip_suffix(')')) {
+        // 只有首尾的括号互相配对时才去掉：`(a)+(b)` 不能变成 `a)+(b`
+        Some(inner) if balanced(inner) => inner,
+        _ => e,
+    }
+}
+
+fn balanced(s: &str) -> bool {
+    let mut depth = 0i32;
+    for c in s.chars() {
+        match c {
+            '(' => depth += 1,
+            ')' => {
+                depth -= 1;
+                if depth < 0 {
+                    return false;
+                }
+            }
+            _ => {}
+        }
+    }
+    depth == 0
+}
+
 /// 数据库返回的 DDL 文本：统一换行，去掉首尾空白。
 pub fn normalize_ddl(text: &str) -> String {
     text.replace("\r\n", "\n").trim().to_string() + "\n"
@@ -419,6 +509,65 @@ COMMENT ON COLUMN public.\"Orders\".code IS '编码 ''A''';
         );
     }
 
+    #[test]
+    fn sqlserver_queries_and_table() {
+        let d = Dialect::new(Variant::SqlServer);
+        let v = d.ddl_query(DdlKind::View, "dbo", "v]1").unwrap().unwrap();
+        assert!(v.sql.contains("sys.sql_modules"), "{}", v.sql);
+        assert_eq!(v.params, [Value::String("[dbo].[v]]1]".into())]);
+        assert!(d.ddl_query(DdlKind::Synonym, "dbo", "s").unwrap().unwrap().sql.contains("CREATE SYNONYM"));
+        assert_eq!(d.ddl_query(DdlKind::Table, "dbo", "t").unwrap(), None);
+        assert_eq!(d.ddl_query(DdlKind::Procedure, "dbo", "p").unwrap(), None);
+        assert!(d.ddl_query(DdlKind::Package, "dbo", "p").is_err());
+
+        let mut id = col("id", "int");
+        id.nullable = false;
+        id.auto_increment = true;
+        let mut name = col("name", "nvarchar(50)");
+        name.default = Some("(N'x')".into());
+        name.comment = Some("名称".into());
+        let mut total = col("total", "int");
+        total.generated = Some("([id]*(2))".into());
+        let t = Table {
+            name: "Orders".into(),
+            comment: Some("订单".into()),
+            columns: vec![id, name, total],
+            primary_key: Some(PrimaryKey { name: Some("PK_Orders".into()), columns: vec!["id".into()] }),
+            indexes: vec![Index {
+                name: "IX_name".into(),
+                columns: vec![IndexColumn { name: "name".into(), descending: false }],
+                unique: false,
+                method: Some("NONCLUSTERED".into()),
+            }],
+            triggers: vec![Trigger {
+                name: "trg".into(),
+                timing: None,
+                events: vec![],
+                definition: Some("CREATE TRIGGER trg ON dbo.Orders AFTER INSERT AS SELECT 1".into()),
+            }],
+            ..Default::default()
+        };
+        assert_eq!(
+            d.table_ddl("dbo", &t).unwrap(),
+            "CREATE TABLE dbo.Orders (
+    id int IDENTITY(1,1) NOT NULL,
+    name nvarchar(50) DEFAULT (N'x'),
+    total AS ([id]*(2)),
+    CONSTRAINT PK_Orders PRIMARY KEY (id)
+);
+
+CREATE INDEX IX_name ON dbo.Orders (name);
+GO
+CREATE TRIGGER trg ON dbo.Orders AFTER INSERT AS SELECT 1
+GO
+EXEC sys.sp_addextendedproperty N'MS_Description', N'订单', N'SCHEMA', N'dbo', N'TABLE', N'Orders';
+EXEC sys.sp_addextendedproperty N'MS_Description', N'名称', N'SCHEMA', N'dbo', N'TABLE', N'Orders', N'COLUMN', N'name';
+"
+        );
+        assert_eq!(strip_parens("(a)+(b)"), "(a)+(b)");
+        assert_eq!(strip_parens(" ((a)) "), "(a)");
+    }
+
     #[test]
     fn extras_without_triggers() {
         let mut c = col("ID", "NUMBER");
```

- [ ] **Step 8: 修改 `crates/dblens-dialect/src/design/catalog.rs`**

```diff
--- a/crates/dblens-dialect/src/design/catalog.rs
+++ b/crates/dblens-dialect/src/design/catalog.rs
@@ -154,6 +154,33 @@ const DAMENG_TYPES: &[TypeEntry] = &[
     t("TIMESTAMP WITH TIME ZONE", L),
 ];
 
+/// SQL Server：长度可写 MAX 的类型在设计器里按长度填写（M6b 再细化）。
+const SQLSERVER_TYPES: &[TypeEntry] = &[
+    t("BIT", N),
+    t("TINYINT", N),
+    t("SMALLINT", N),
+    t("INT", N),
+    t("BIGINT", N),
+    t("DECIMAL", P),
+    t("NUMERIC", P),
+    t("MONEY", N),
+    t("REAL", N),
+    t("FLOAT", N),
+    t("CHAR", L),
+    t("VARCHAR", L),
+    t("NCHAR", L),
+    t("NVARCHAR", L),
+    t("BINARY", L),
+    t("VARBINARY", L),
+    t("DATE", N),
+    t("TIME", L),
+    t("DATETIME", N),
+    t("DATETIME2", L),
+    t("DATETIMEOFFSET", L),
+    t("UNIQUEIDENTIFIER", N),
+    t("XML", N),
+];
+
 impl Dialect {
     pub fn design_caps(&self) -> DesignCaps {
         let v = self.version().unwrap_or(u32::MAX);
@@ -208,6 +235,15 @@ impl Dialect {
                 index_methods: vec!["NORMAL", "BITMAP"],
                 types: if self.family() == Family::Oracle { ORACLE_TYPES } else { DAMENG_TYPES }.to_vec(),
             },
+            Family::SqlServer => DesignCaps {
+                reorder: false,
+                checks: true,
+                descending_index: true,
+                on_delete: vec!["NO ACTION", "CASCADE", "SET NULL", "SET DEFAULT"],
+                on_update: vec!["NO ACTION", "CASCADE", "SET NULL", "SET DEFAULT"],
+                index_methods: vec!["NONCLUSTERED", "CLUSTERED"],
+                types: SQLSERVER_TYPES.to_vec(),
+            },
         }
     }
 }
```

- [ ] **Step 9: 修改 `crates/dblens-dialect/src/design/rebuild.rs`**

```diff
--- a/crates/dblens-dialect/src/design/rebuild.rs
+++ b/crates/dblens-dialect/src/design/rebuild.rs
@@ -75,7 +75,7 @@ impl Dialect {
             Family::MySql => 64,
             Family::Postgres => 63,
             Family::Oracle if self.version().unwrap_or(u32::MAX) < 120200 => 30,
-            Family::Oracle | Family::Dameng => 128,
+            Family::Oracle | Family::Dameng | Family::SqlServer => 128,
         }
     }
 
@@ -276,7 +276,8 @@ impl Dialect {
                     "ALTER TABLE {q} MODIFY ({} GENERATED BY DEFAULT AS IDENTITY (START WITH LIMIT VALUE))",
                     self.ident(&c.name)
                 )),
-                Family::MySql | Family::Dameng => {}
+                // SQL Server 在 IDENTITY_INSERT 下写入显式值时也会推进标识种子
+                Family::MySql | Family::Dameng | Family::SqlServer => {}
             }
         }
         if !mysql {
```

- [ ] **Step 10: 修改 `crates/dblens-dialect/src/design/sql.rs`**

```diff
--- a/crates/dblens-dialect/src/design/sql.rs
+++ b/crates/dblens-dialect/src/design/sql.rs
@@ -53,7 +53,7 @@ impl Dialect {
                         out.push(msg!("design.unsupported.serial", column = to.name));
                     }
                 }
-                Family::Oracle | Family::Dameng => {
+                Family::Oracle | Family::Dameng | Family::SqlServer => {
                     if generated {
                         out.push(msg!("design.unsupported.generated", column = to.name));
                     }
@@ -72,6 +72,8 @@ impl Dialect {
             Family::MySql => self.mysql_design(schema, table, &diff.changes),
             Family::Postgres => self.pg_design(schema, table, &diff.changes),
             Family::Oracle | Family::Dameng => self.oracle_design(schema, table, &diff.changes),
+            // 表设计器的 SQL Server 支持在 M6b
+            Family::SqlServer => Err(dblens_core::Error::invalid(dblens_core::msg!("sqlserver.later"))),
         }
     }
 
```

- [ ] **Step 11: 修改 `crates/dblens-dialect/src/dml.rs`**

```diff
--- a/crates/dblens-dialect/src/dml.rs
+++ b/crates/dblens-dialect/src/dml.rs
@@ -50,7 +50,9 @@ impl Dialect {
         let sql = if cols.is_empty() {
             match (self.family(), table.columns.iter().find(|c| c.generated.is_none())) {
                 (Family::MySql, _) => format!("INSERT INTO {target} () VALUES ()"),
-                (Family::Postgres | Family::Dameng, _) => format!("INSERT INTO {target} DEFAULT VALUES"),
+                (Family::Postgres | Family::Dameng | Family::SqlServer, _) => {
+                    format!("INSERT INTO {target} DEFAULT VALUES")
+                }
                 (Family::Oracle, Some(first)) => {
                     format!("INSERT INTO {target} ({}) VALUES (DEFAULT)", self.ident(&first.name))
                 }
```

- [ ] **Step 12: 修改 `crates/dblens-dialect/src/ident.rs`**

```diff
--- a/crates/dblens-dialect/src/ident.rs
+++ b/crates/dblens-dialect/src/ident.rs
@@ -7,11 +7,12 @@ pub fn quote_ident(family: Family, name: &str) -> String {
     match family {
         Family::MySql => format!("`{}`", name.replace('`', "``")),
         Family::Postgres | Family::Oracle | Family::Dameng => format!("\"{}\"", name.replace('"', "\"\"")),
+        Family::SqlServer => format!("[{}]", name.replace(']', "]]")),
     }
 }
 
 /// 只在必要时加引号：含特殊字符、以数字开头、是保留字，
-/// 或者大小写与数据库的默认折叠方向不一致（PG 折叠为小写，Oracle/达梦折叠为大写）。
+/// 或者大小写与数据库的默认折叠方向不一致（PG 折叠为小写，Oracle/达梦折叠为大写；MySQL、SQL Server 不区分）。
 pub fn ident(family: Family, name: &str) -> String {
     if needs_quote(family, name) { quote_ident(family, name) } else { name.to_string() }
 }
@@ -22,13 +23,14 @@ pub fn placeholder(family: Family, index: usize) -> String {
         Family::Postgres => format!("${index}"),
         Family::Oracle => format!(":{index}"),
         Family::MySql | Family::Dameng => "?".to_string(),
+        Family::SqlServer => format!("@P{index}"),
     }
 }
 
 fn needs_quote(family: Family, name: &str) -> bool {
     let Some(first) = name.chars().next() else { return true };
     let first_ok = match family {
-        Family::MySql | Family::Postgres => first.is_ascii_alphabetic() || first == '_',
+        Family::MySql | Family::Postgres | Family::SqlServer => first.is_ascii_alphabetic() || first == '_',
         Family::Oracle | Family::Dameng => first.is_ascii_alphabetic(),
     };
     if !first_ok {
@@ -39,7 +41,7 @@ fn needs_quote(family: Family, name: &str) -> bool {
         return true;
     }
     let case_ok = match family {
-        Family::MySql => true,
+        Family::MySql | Family::SqlServer => true,
         Family::Postgres => !name.chars().any(|c| c.is_ascii_uppercase()),
         Family::Oracle | Family::Dameng => !name.chars().any(|c| c.is_ascii_lowercase()),
     };
@@ -53,8 +55,197 @@ fn is_reserved(family: Family, upper: &str) -> bool {
     }
     RESERVED.binary_search(&upper).is_ok()
         || (family == Family::Dameng && DAMENG_RESERVED.binary_search(&upper).is_ok())
+        || (family == Family::SqlServer && SQLSERVER_RESERVED.binary_search(&upper).is_ok())
 }
 
+/// SQL Server 的保留字（微软文档"Reserved Keywords"中 SQL Server 一栏），不在 `RESERVED` 中的部分也要加引号。按字节序排列。
+const SQLSERVER_RESERVED: &[&str] = &[
+    "ADD",
+    "ALL",
+    "ALTER",
+    "AND",
+    "ANY",
+    "AS",
+    "ASC",
+    "AUTHORIZATION",
+    "BACKUP",
+    "BEGIN",
+    "BETWEEN",
+    "BREAK",
+    "BROWSE",
+    "BULK",
+    "BY",
+    "CASCADE",
+    "CASE",
+    "CHECK",
+    "CHECKPOINT",
+    "CLOSE",
+    "CLUSTERED",
+    "COALESCE",
+    "COLLATE",
+    "COLUMN",
+    "COMMIT",
+    "COMPUTE",
+    "CONSTRAINT",
+    "CONTAINS",
+    "CONTAINSTABLE",
+    "CONTINUE",
+    "CONVERT",
+    "CREATE",
+    "CROSS",
+    "CURRENT",
+    "CURRENT_DATE",
+    "CURRENT_TIME",
+    "CURRENT_TIMESTAMP",
+    "CURRENT_USER",
+    "CURSOR",
+    "DATABASE",
+    "DBCC",
+    "DEALLOCATE",
+    "DECLARE",
+    "DEFAULT",
+    "DELETE",
+    "DENY",
+    "DESC",
+    "DISK",
+    "DISTINCT",
+    "DISTRIBUTED",
+    "DOUBLE",
+    "DROP",
+    "DUMP",
+    "ELSE",
+    "END",
+    "ERRLVL",
+    "ESCAPE",
+    "EXCEPT",
+    "EXEC",
+    "EXECUTE",
+    "EXISTS",
+    "EXIT",
+    "EXTERNAL",
+    "FETCH",
+    "FILE",
+    "FILLFACTOR",
+    "FOR",
+    "FOREIGN",
+    "FREETEXT",
+    "FREETEXTTABLE",
+    "FROM",
+    "FULL",
+    "FUNCTION",
+    "GOTO",
+    "GRANT",
+    "GROUP",
+    "HAVING",
+    "HOLDLOCK",
+    "IDENTITY",
+    "IDENTITYCOL",
+    "IDENTITY_INSERT",
+    "IF",
+    "IN",
+    "INDEX",
+    "INNER",
+    "INSERT",
+    "INTERSECT",
+    "INTO",
+    "IS",
+    "JOIN",
+    "KEY",
+    "KILL",
+    "LEFT",
+    "LIKE",
+    "LINENO",
+    "LOAD",
+    "MERGE",
+    "NATIONAL",
+    "NOCHECK",
+    "NONCLUSTERED",
+    "NOT",
+    "NULL",
+    "NULLIF",
+    "OF",
+    "OFF",
+    "OFFSETS",
+    "ON",
+    "OPEN",
+    "OPENDATASOURCE",
+    "OPENQUERY",
+    "OPENROWSET",
+    "OPENXML",
+    "OPTION",
+    "OR",
+    "ORDER",
+    "OUTER",
+    "OVER",
+    "PERCENT",
+    "PIVOT",
+    "PLAN",
+    "PRECISION",
+    "PRIMARY",
+    "PRINT",
+    "PROC",
+    "PROCEDURE",
+    "PUBLIC",
+    "RAISERROR",
+    "READ",
+    "READTEXT",
+    "RECONFIGURE",
+    "REFERENCES",
+    "REPLICATION",
+    "RESTORE",
+    "RESTRICT",
+    "RETURN",
+    "REVERT",
+    "REVOKE",
+    "RIGHT",
+    "ROLLBACK",
+    "ROWCOUNT",
+    "ROWGUIDCOL",
+    "RULE",
+    "SAVE",
+    "SCHEMA",
+    "SECURITYAUDIT",
+    "SELECT",
+    "SEMANTICKEYPHRASETABLE",
+    "SEMANTICSIMILARITYDETAILSTABLE",
+    "SEMANTICSIMILARITYTABLE",
+    "SESSION_USER",
+    "SET",
+    "SETUSER",
+    "SHUTDOWN",
+    "SOME",
+    "STATISTICS",
+    "SYSTEM_USER",
+    "TABLE",
+    "TABLESAMPLE",
+    "TEXTSIZE",
+    "THEN",
+    "TO",
+    "TOP",
+    "TRAN",
+    "TRANSACTION",
+    "TRIGGER",
+    "TRUNCATE",
+    "TRY_CONVERT",
+    "TSEQUAL",
+    "UNION",
+    "UNIQUE",
+    "UNPIVOT",
+    "UPDATE",
+    "UPDATETEXT",
+    "USE",
+    "USER",
+    "VALUES",
+    "VARYING",
+    "VIEW",
+    "WAITFOR",
+    "WHEN",
+    "WHERE",
+    "WHILE",
+    "WITH",
+    "WRITETEXT",
+];
+
 /// PostgreSQL、MySQL（8.0/8.4）、Oracle、达梦以及国产库第一批（TiDB 等）的保留字与特殊值函数名（如 CURRENT_USER、SYSDATE）的并集，按字节序排列。
 /// 多加引号不会出错，所以宁多勿少；个别词只在部分方言中保留（如 PUBLIC），由 `is_reserved` 按家族豁免。新增方言时要补充。
 const RESERVED: &[&str] = &[
@@ -612,6 +803,20 @@ mod tests {
         assert_eq!(placeholder(Family::Postgres, 3), "$3");
         assert_eq!(placeholder(Family::Oracle, 3), ":3");
         assert_eq!(placeholder(Family::Dameng, 3), "?");
+        assert_eq!(placeholder(Family::SqlServer, 3), "@P3");
+    }
+
+    #[test]
+    fn sqlserver_brackets() {
+        assert_eq!(quote_ident(Family::SqlServer, "a]b"), "[a]]b]");
+        assert_eq!(ident(Family::SqlServer, "Orders"), "Orders");
+        assert_eq!(ident(Family::SqlServer, "_x"), "_x");
+        assert_eq!(ident(Family::SqlServer, "order"), "[order]");
+        assert_eq!(ident(Family::SqlServer, "PUBLIC"), "[PUBLIC]");
+        assert_eq!(ident(Family::SqlServer, "IDENTITY_INSERT"), "[IDENTITY_INSERT]");
+        assert_eq!(ident(Family::SqlServer, "x; DROP TABLE t"), "[x; DROP TABLE t]");
+        assert_eq!(ident(Family::SqlServer, "数洞"), "[数洞]");
+        assert!(SQLSERVER_RESERVED.windows(2).all(|w| w[0] < w[1]));
     }
 
     #[test]
@@ -668,7 +873,7 @@ mod tests {
             matches!(cs.next(), Some(c) if c.is_ascii_alphabetic() || c == '_')
                 && cs.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
         };
-        for f in [Family::MySql, Family::Postgres, Family::Oracle, Family::Dameng] {
+        for f in [Family::MySql, Family::Postgres, Family::Oracle, Family::Dameng, Family::SqlServer] {
             for name in
                 ["a", "A", "a b", "a\"b", "a`b", "x;--", "数洞", "", "_", "1a", "$x", "ORDER", "order"]
             {
```

- [ ] **Step 13: 修改 `crates/dblens-dialect/src/lib.rs`**

```diff
--- a/crates/dblens-dialect/src/lib.rs
+++ b/crates/dblens-dialect/src/lib.rs
@@ -155,6 +155,11 @@ impl Dialect {
         session::explain(self.family(), sql)
     }
 
+    /// 生成执行计划之后无论成败都要执行的语句（见 `session::explain`）。
+    pub fn explain_cleanup(&self) -> Option<&'static str> {
+        session::explain_cleanup(self.family())
+    }
+
     /// 查询当前 schema 的语句，结果是一行一列。
     pub fn current_schema_sql(&self) -> &'static str {
         session::current_schema(self.family())
```

- [ ] **Step 14: 修改 `crates/dblens-dialect/src/literal.rs`**

```diff
--- a/crates/dblens-dialect/src/literal.rs
+++ b/crates/dblens-dialect/src/literal.rs
@@ -22,6 +22,14 @@ pub fn literal(family: Family, v: &Value) -> Result<String> {
         Value::Json(j) => string_literal(family, &j.to_string())?,
         Value::Uuid(u) => string_literal(family, &u.to_string())?,
         Value::Bytes(b) => bytes_literal(family, b)?,
+        // SQL Server 没有 DATE '…' 这类字面量；ISO 8601 文本经 CAST 转换，不受 DATEFORMAT、语言设置影响
+        Value::Date(d) if family == Family::SqlServer => format!("CAST('{}' AS DATE)", d.format("%Y-%m-%d")),
+        Value::Time(t) if family == Family::SqlServer => {
+            format!("CAST('{}' AS TIME)", t.format("%H:%M:%S%.f"))
+        }
+        Value::DateTime(dt) if family == Family::SqlServer => {
+            format!("CAST('{}' AS DATETIME2)", dt.format("%Y-%m-%dT%H:%M:%S%.f"))
+        }
         Value::Date(d) => format!("DATE '{}'", d.format("%Y-%m-%d")),
         Value::Time(t) => {
             // Oracle 没有 TIME 类型，按 'HH24:MI:SS.FF6' 文本写出（与 types.rs 的存储约定一致，固定 6 位小数）
@@ -39,6 +47,9 @@ pub fn literal(family: Family, v: &Value) -> Result<String> {
             Family::Oracle | Family::Dameng => {
                 format!("TIMESTAMP '{}'", dt.format("%Y-%m-%d %H:%M:%S%.f %:z"))
             }
+            Family::SqlServer => {
+                format!("CAST('{}' AS DATETIMEOFFSET)", dt.format("%Y-%m-%dT%H:%M:%S%.f%:z"))
+            }
         },
         // 错误信息只报类型，不回显值本身（大字段或未知类型的文本可能很长或很敏感）
         Value::Lob(_) => return Err(Error::invalid(msg!("literal.lob"))),
@@ -53,7 +64,7 @@ fn float_literal(family: Family, f: f64) -> Result<String> {
         return match family {
             Family::Postgres => Ok("'NaN'::float8".to_string()),
             Family::Oracle => Ok("BINARY_DOUBLE_NAN".to_string()),
-            Family::MySql | Family::Dameng => Err(Error::invalid(msg!("literal.nan"))),
+            Family::MySql | Family::Dameng | Family::SqlServer => Err(Error::invalid(msg!("literal.nan"))),
         };
     }
     if f.is_infinite() {
@@ -62,7 +73,9 @@ fn float_literal(family: Family, f: f64) -> Result<String> {
             (Family::Postgres, false) => Ok("'-Infinity'::float8".to_string()),
             (Family::Oracle, true) => Ok("BINARY_DOUBLE_INFINITY".to_string()),
             (Family::Oracle, false) => Ok("-BINARY_DOUBLE_INFINITY".to_string()),
-            (Family::MySql | Family::Dameng, _) => Err(Error::invalid(msg!("literal.infinity"))),
+            (Family::MySql | Family::Dameng | Family::SqlServer, _) => {
+                Err(Error::invalid(msg!("literal.infinity")))
+            }
         };
     }
     // 普通区间用十进制写法，避免指数形式在各库之间的解析差异；其余用科学计数法保留精度
@@ -106,7 +119,8 @@ fn is_decimal_text(s: &str) -> bool {
 const ORACLE_LITERAL_MAX: usize = 4000;
 
 /// 字符串字面量。MySQL 含反斜杠或 NUL 时用十六进制 + utf8mb4 形式，不受 sql_mode 影响；
-/// PostgreSQL 含反斜杠时用 E'' 形式；Oracle 超过 4000 字节时拆成 TO_CLOB 拼接；NUL 在非 MySQL 方言中报错。
+/// PostgreSQL 含反斜杠时用 E'' 形式；Oracle 超过 4000 字节时拆成 TO_CLOB 拼接；SQL Server 一律用 N''（Unicode）；
+/// NUL 在非 MySQL 方言中报错。
 fn string_literal(family: Family, s: &str) -> Result<String> {
     if s.contains('\0') && family != Family::MySql {
         return Err(Error::invalid(msg!("literal.nul")));
@@ -120,6 +134,7 @@ fn string_literal(family: Family, s: &str) -> Result<String> {
             format!("E'{}'", s.replace('\\', "\\\\").replace('\'', "''"))
         }
         Family::Postgres => quoted(s),
+        Family::SqlServer => format!("N{}", quoted(s)),
         Family::Oracle => {
             let chunks = oracle_chunks(s, ORACLE_LITERAL_MAX);
             if let [only] = chunks.as_slice() {
@@ -178,6 +193,8 @@ fn bytes_literal(family: Family, b: &[u8]) -> Result<String> {
         // 达梦的 0x 字面量不能为空，空值写成空字符串由数据库隐式转换
         Family::Dameng if b.is_empty() => "''".to_string(),
         Family::Dameng => format!("0x{hex}"),
+        // SQL Server 的 0x 可以为空
+        Family::SqlServer => format!("0x{hex}"),
     })
 }
 
@@ -185,11 +202,13 @@ fn bytes_literal(family: Family, b: &[u8]) -> Result<String> {
 /// 仅适用于自身不含 LIMIT/FETCH 的查询。
 /// 子句另起一行，原查询末尾的行注释不会吞掉它。
 /// 采用追加而非包装：允许重复列名，ORDER BY 仍然有效（MariaDB 会忽略派生表内的 ORDER BY）。
-/// Oracle 需要 12c 及以上；11g 的 ROWNUM 分页尚未实现。
+/// Oracle 需要 12c 及以上；11g 的 ROWNUM 分页尚未实现。SQL Server 的 OFFSET 要求查询带 ORDER BY。
 pub fn paginate(family: Family, sql: &str, offset: u64, limit: u64) -> String {
     let inner = sql.trim_start().trim_end_matches(|c: char| c == ';' || c.is_whitespace());
     match family {
-        Family::Oracle => format!("{inner}\nOFFSET {offset} ROWS FETCH NEXT {limit} ROWS ONLY"),
+        Family::Oracle | Family::SqlServer => {
+            format!("{inner}\nOFFSET {offset} ROWS FETCH NEXT {limit} ROWS ONLY")
+        }
         Family::MySql | Family::Postgres | Family::Dameng => {
             format!("{inner}\nLIMIT {limit} OFFSET {offset}")
         }
@@ -411,5 +430,30 @@ mod tests {
             "SELECT * FROM t -- 注释\nOFFSET 0 ROWS FETCH NEXT 50 ROWS ONLY"
         );
         assert!(paginate(Family::Dameng, "SELECT 1", 0, 1).ends_with("\nLIMIT 1 OFFSET 0"));
+        assert_eq!(
+            paginate(Family::SqlServer, "SELECT * FROM t ORDER BY id;", 10, 5),
+            "SELECT * FROM t ORDER BY id\nOFFSET 10 ROWS FETCH NEXT 5 ROWS ONLY"
+        );
+    }
+
+    #[test]
+    fn sqlserver_literals() {
+        let f = Family::SqlServer;
+        assert_eq!(lit(f, Value::Bool(true)), "1");
+        assert_eq!(lit(f, Value::String("it's 数洞".into())), "N'it''s 数洞'");
+        assert_eq!(lit(f, Value::Bytes(vec![0x01, 0xab])), "0x01AB");
+        assert_eq!(lit(f, Value::Bytes(vec![])), "0x");
+        let d = NaiveDate::from_ymd_opt(2024, 2, 29).unwrap();
+        assert_eq!(lit(f, Value::Date(d)), "CAST('2024-02-29' AS DATE)");
+        let t = NaiveTime::from_hms_micro_opt(13, 45, 30, 123456).unwrap();
+        assert_eq!(lit(f, Value::Time(t)), "CAST('13:45:30.123456' AS TIME)");
+        assert_eq!(
+            lit(f, Value::DateTime(d.and_hms_opt(1, 2, 3).unwrap())),
+            "CAST('2024-02-29T01:02:03' AS DATETIME2)"
+        );
+        let tz = FixedOffset::east_opt(8 * 3600).unwrap().with_ymd_and_hms(2024, 2, 29, 13, 45, 30).unwrap();
+        assert_eq!(lit(f, Value::DateTimeTz(tz)), "CAST('2024-02-29T13:45:30+08:00' AS DATETIMEOFFSET)");
+        assert!(literal(f, &Value::Float(f64::NAN)).is_err());
+        assert!(literal(f, &Value::String("a\0b".into())).is_err());
     }
 }
```

- [ ] **Step 15: 修改 `crates/dblens-dialect/src/meta/mod.rs`**

```diff
--- a/crates/dblens-dialect/src/meta/mod.rs
+++ b/crates/dblens-dialect/src/meta/mod.rs
@@ -21,6 +21,9 @@ pub use rows::{MetaRow, MetaRows};
 #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
 #[serde(rename_all = "snake_case")]
 pub enum MetaKey {
+    /// 列出数据库（只有 SQL Server：连接 → 数据库 → 架构）。参数：filter。
+    /// 其余查询都在当前库中执行，服务层先切到所选的库。
+    Catalogs,
     /// 列出 schema（MySQL 为 database）。参数：filter。
     Schemas,
     /// 列出表（含分区父表）。参数：schema、filter、table（只取这一张）。
@@ -58,7 +61,8 @@ pub enum MetaKey {
 }
 
 impl MetaKey {
-    pub const ALL: [MetaKey; 21] = [
+    pub const ALL: [MetaKey; 22] = [
+        MetaKey::Catalogs,
         MetaKey::Schemas,
         MetaKey::Tables,
         MetaKey::Views,
@@ -94,6 +98,7 @@ impl MetaKey {
 
     pub fn as_str(self) -> &'static str {
         match self {
+            MetaKey::Catalogs => "catalogs",
             MetaKey::Schemas => "schemas",
             MetaKey::Tables => "tables",
             MetaKey::Views => "views",
@@ -123,6 +128,8 @@ impl MetaKey {
 /// 查询参数。`filter` 是用户在对象树过滤框里输入的原文（按名字或注释包含匹配，不区分大小写）。
 #[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
 pub struct MetaArgs {
+    /// 所在的数据库（SQL Server）。不进模板：服务层执行前先切到这个库。
+    pub catalog: Option<String>,
     pub schema: Option<String>,
     pub table: Option<String>,
     pub name: Option<String>,
```

- [ ] **Step 16: 修改 `crates/dblens-dialect/src/meta/template.rs`**

```diff
--- a/crates/dblens-dialect/src/meta/template.rs
+++ b/crates/dblens-dialect/src/meta/template.rs
@@ -23,8 +23,9 @@ const KINGBASE: &str = include_str!("../../sql/kingbase.sql");
 const OPENGAUSS: &str = include_str!("../../sql/opengauss.sql");
 const TIDB: &str = include_str!("../../sql/tidb.sql");
 const OCEANBASE: &str = include_str!("../../sql/oceanbase.sql");
+const SQLSERVER: &str = include_str!("../../sql/sqlserver.sql");
 
-pub(crate) const ALL_VARIANTS: [Variant; 12] = [
+pub(crate) const ALL_VARIANTS: [Variant; 13] = [
     Variant::MySql,
     Variant::MariaDb,
     Variant::TiDb,
@@ -37,6 +38,7 @@ pub(crate) const ALL_VARIANTS: [Variant; 12] = [
     Variant::HighGo,
     Variant::Oracle,
     Variant::Dameng,
+    Variant::SqlServer,
 ];
 
 /// 把模板文件按 `-- [key]` 行拆成 key → 模板正文。
@@ -67,6 +69,7 @@ fn base_file(family: Family) -> &'static str {
         Family::Postgres => POSTGRES,
         Family::Oracle => ORACLE,
         Family::Dameng => DAMENG,
+        Family::SqlServer => SQLSERVER,
     }
 }
 
```

- [ ] **Step 17: 修改 `crates/dblens-dialect/src/migrate/data.rs`**

```diff
--- a/crates/dblens-dialect/src/migrate/data.rs
+++ b/crates/dblens-dialect/src/migrate/data.rs
@@ -124,15 +124,15 @@ pub fn keyset_params(last: &[Value]) -> Vec<Value> {
     (0..last.len()).flat_map(|i| last[..=i].iter().cloned()).collect()
 }
 
-/// 达梦写入自增列前后的开关；其他库为 None（标识列都是 BY DEFAULT，可以直接写）。
+/// 达梦、SQL Server 写入自增列前后的开关；其他库为 None（标识列都是 BY DEFAULT，可以直接写）。
 pub fn identity_insert_sql(d: &Dialect, schema: &str, table: &str, on: bool) -> Option<String> {
-    (d.family() == Family::Dameng).then(|| {
+    matches!(d.family(), Family::Dameng | Family::SqlServer).then(|| {
         format!("SET IDENTITY_INSERT {} {}", d.qualified(Some(schema), table), if on { "ON" } else { "OFF" })
     })
 }
 
-/// 迁完数据后把自增列推到当前最大值之后。MySQL 系写入显式值时会自动推进计数器，达梦在 `IDENTITY_INSERT` 下也会，
-/// 都返回 None。
+/// 迁完数据后把自增列推到当前最大值之后。MySQL 系写入显式值时会自动推进计数器，达梦、SQL Server 在
+/// `IDENTITY_INSERT` 下也会，都返回 None。
 pub fn identity_reset_sql(d: &Dialect, schema: &str, table: &str, column: &str) -> Result<Option<String>> {
     let q = d.qualified(Some(schema), table);
     let c = d.ident(column);
@@ -145,12 +145,13 @@ pub fn identity_reset_sql(d: &Dialect, schema: &str, table: &str, column: &str)
         Family::Oracle => Some(format!(
             "ALTER TABLE {q} MODIFY ({c} GENERATED BY DEFAULT AS IDENTITY (START WITH LIMIT VALUE))"
         )),
-        Family::MySql | Family::Dameng => None,
+        Family::MySql | Family::Dameng | Family::SqlServer => None,
     })
 }
 
 /// 读序列的下一个值的查询（一行一列 `n`）：PG 系由 `last_value`、`is_called` 算出；Oracle、达梦取 `LAST_NUMBER`
-/// （带缓存时可能比实际的下一个值大，作为起点是安全的）。MySQL 系没有序列，返回 None。
+/// （带缓存时可能比实际的下一个值大，作为起点是安全的）；SQL Server 由 `current_value` 加步长算出（从未取过值时是起始值）。
+/// MySQL 系没有序列，返回 None。
 pub fn sequence_next_sql(d: &Dialect, schema: &str, name: &str) -> Result<Option<String>> {
     Ok(match d.family() {
         Family::Postgres => Some(format!(
@@ -162,11 +163,17 @@ pub fn sequence_next_sql(d: &Dialect, schema: &str, name: &str) -> Result<Option
             d.literal(&Value::String(schema.to_string()))?,
             d.literal(&Value::String(name.to_string()))?
         )),
+        Family::SqlServer => Some(format!(
+            "SELECT CASE WHEN last_used_value IS NULL THEN CAST(start_value AS bigint) \
+             ELSE CAST(current_value AS bigint) + CAST(increment AS bigint) END AS n \
+             FROM sys.sequences WHERE object_id = OBJECT_ID({})",
+            d.literal(&Value::String(d.qualified(Some(schema), name)))?
+        )),
         Family::MySql => None,
     })
 }
 
-/// 把目标序列推到 `next`：PG 系 `setval`；Oracle、达梦删掉后按原定义以 `next` 起始重建。
+/// 把目标序列推到 `next`：PG 系 `setval`；SQL Server `ALTER SEQUENCE … RESTART`；Oracle、达梦删掉后按原定义以 `next` 起始重建。
 pub fn sequence_restart_sql(d: &Dialect, schema: &str, seq: &Sequence, next: &str) -> Result<Vec<String>> {
     let q = d.qualified(Some(schema), &seq.name);
     Ok(match d.family() {
@@ -175,6 +182,7 @@ pub fn sequence_restart_sql(d: &Dialect, schema: &str, seq: &Sequence, next: &st
             let s = Sequence { start: Some(next.to_string()), ..seq.clone() };
             vec![format!("DROP SEQUENCE {q}"), sequence_sql(d, schema, &s)]
         }
+        Family::SqlServer => vec![format!("ALTER SEQUENCE {q} RESTART WITH {next}")],
         Family::MySql => vec![],
     })
 }
```

- [ ] **Step 18: 修改 `crates/dblens-dialect/src/migrate/expr.rs`**

```diff
--- a/crates/dblens-dialect/src/migrate/expr.rs
+++ b/crates/dblens-dialect/src/migrate/expr.rs
@@ -308,6 +308,7 @@ pub(super) fn convert_default(
                 .map(|s| Some(format!("nextval({s})")))
                 .map_err(|_| ()),
             Family::Oracle | Family::Dameng => Ok(Some(format!("{}.NEXTVAL", tgt.ident(name)))),
+            Family::SqlServer => Ok(Some(format!("NEXT VALUE FOR {}", tgt.ident(name)))),
             Family::MySql => Err(()),
         };
     }
```

- [ ] **Step 19: 修改 `crates/dblens-dialect/src/migrate/mod.rs`**

```diff
--- a/crates/dblens-dialect/src/migrate/mod.rs
+++ b/crates/dblens-dialect/src/migrate/mod.rs
@@ -316,7 +316,8 @@ impl Context<'_> {
             })
     }
 
-    /// 约束名：系统生成的名字（MySQL 的主键 `PRIMARY`、Oracle 的 `SYS_C…`、达梦的 `CONS…`）不带过去，由目标库生成。
+    /// 约束名：系统生成的名字（MySQL 的主键 `PRIMARY`、Oracle 的 `SYS_C…`、达梦的 `CONS…`、SQL Server 的 `PK__…` 等）
+    /// 不带过去，由目标库生成。
     fn constraint(&self, name: &Option<String>) -> Option<String> {
         let n = text(name)?;
         let digits = |p: &str| {
@@ -326,6 +327,7 @@ impl Context<'_> {
             Family::MySql => n.eq_ignore_ascii_case("PRIMARY"),
             Family::Oracle => digits("SYS_C"),
             Family::Dameng => digits("CONS"),
+            Family::SqlServer => ["PK__", "UQ__", "FK__", "CK__", "DF__"].iter().any(|p| n.starts_with(p)),
             Family::Postgres => false,
         };
         (!system).then(|| self.case.apply(n))
```

- [ ] **Step 20: 修改 `crates/dblens-dialect/src/migrate/names.rs`**

```diff
--- a/crates/dblens-dialect/src/migrate/names.rs
+++ b/crates/dblens-dialect/src/migrate/names.rs
@@ -18,12 +18,12 @@ pub enum NameCase {
 }
 
 impl NameCase {
-    /// 目标库的习惯：Oracle、达梦折叠为大写，PG 系（含兼容 Oracle 的模式）折叠为小写，MySQL 系保持。
+    /// 目标库的习惯：Oracle、达梦折叠为大写，PG 系（含兼容 Oracle 的模式）折叠为小写，MySQL 系、SQL Server 保持。
     pub fn for_target(target: &Dialect) -> NameCase {
         match target.family() {
             Family::Oracle | Family::Dameng => NameCase::Upper,
             Family::Postgres => NameCase::Lower,
-            Family::MySql => NameCase::Keep,
+            Family::MySql | Family::SqlServer => NameCase::Keep,
         }
     }
 
@@ -36,7 +36,7 @@ impl NameCase {
     }
 }
 
-/// 标识符长度上限（MySQL 按字符，其余按字节）：MySQL 64，PG 系 63，Oracle 12.2 起 128、之前 30，达梦 128。
+/// 标识符长度上限（MySQL 按字符，其余按字节）：MySQL 64，PG 系 63，Oracle 12.2 起 128、之前 30，达梦、SQL Server 128。
 pub fn max_ident_len(target: &Dialect) -> usize {
     match target.family() {
         Family::MySql => 64,
@@ -48,7 +48,7 @@ pub fn max_ident_len(target: &Dialect) -> usize {
                 128
             }
         }
-        Family::Dameng => 128,
+        Family::Dameng | Family::SqlServer => 128,
     }
 }
 
```

- [ ] **Step 21: 修改 `crates/dblens-dialect/src/migrate/script.rs`**

```diff
--- a/crates/dblens-dialect/src/migrate/script.rs
+++ b/crates/dblens-dialect/src/migrate/script.rs
@@ -213,7 +213,7 @@ fn design(tgt: &Dialect, schema: &str, table: &str, change: TableChange) -> Resu
 fn drop_sql(tgt: &Dialect, schema: &str, name: &str) -> String {
     let q = tgt.qualified(Some(schema), name);
     match tgt.family() {
-        Family::MySql => format!("DROP TABLE {q}"),
+        Family::MySql | Family::SqlServer => format!("DROP TABLE {q}"),
         Family::Postgres | Family::Dameng => format!("DROP TABLE {q} CASCADE"),
         Family::Oracle => format!("DROP TABLE {q} CASCADE CONSTRAINTS"),
     }
```

- [ ] **Step 22: 修改 `crates/dblens-dialect/src/migrate/types.rs`**

```diff
--- a/crates/dblens-dialect/src/migrate/types.rs
+++ b/crates/dblens-dialect/src/migrate/types.rs
@@ -135,11 +135,13 @@ pub(super) fn identity_supported(tgt: &Dialect) -> bool {
     !(tgt.family() == Family::Oracle && tgt.version().is_some_and(|v| v < 120000))
 }
 
-/// 外键动作在目标库中是否可用：Oracle、达梦只有 ON DELETE CASCADE / SET NULL；MySQL（InnoDB）不支持 SET DEFAULT。
+/// 外键动作在目标库中是否可用：Oracle、达梦只有 ON DELETE CASCADE / SET NULL；MySQL（InnoDB）不支持 SET DEFAULT；
+/// SQL Server 没有 RESTRICT。
 pub(super) fn fk_action_supported(tgt: &Dialect, what: &str, action: &str) -> bool {
     match tgt.family() {
         Family::Oracle | Family::Dameng => what == "DELETE" && (action == "CASCADE" || action == "SET NULL"),
         Family::MySql => action != "SET DEFAULT",
+        Family::SqlServer => action != "RESTRICT",
         Family::Postgres => true,
     }
 }
```

- [ ] **Step 23: 修改 `crates/dblens-dialect/src/ops.rs`**

```diff
--- a/crates/dblens-dialect/src/ops.rs
+++ b/crates/dblens-dialect/src/ops.rs
@@ -25,6 +25,7 @@ const DAMENG: &str = include_str!("../sql/ops/dameng.sql");
 const TIDB: &str = include_str!("../sql/ops/tidb.sql");
 const OCEANBASE: &str = include_str!("../sql/ops/oceanbase.sql");
 const OPENGAUSS: &str = include_str!("../sql/ops/opengauss.sql");
+const SQLSERVER: &str = include_str!("../sql/ops/sqlserver.sql");
 
 /// 运维查询的分类。
 #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
@@ -115,6 +116,7 @@ fn base_file(family: Family) -> &'static str {
         Family::Postgres => POSTGRES,
         Family::Oracle => ORACLE,
         Family::Dameng => DAMENG,
+        Family::SqlServer => SQLSERVER,
     }
 }
 
```

- [ ] **Step 24: 修改 `crates/dblens-dialect/src/session.rs`**

```diff
--- a/crates/dblens-dialect/src/session.rs
+++ b/crates/dblens-dialect/src/session.rs
@@ -1,11 +1,14 @@
 //! 会话级 SQL：执行计划、读取和切换当前 schema。
+//! SQL Server 不能在会话中切换默认架构，会话的"当前 schema"是当前数据库（`USE`），与 MySQL 的库相同。
 
 use crate::{Family, ident};
 
 /// 生成一条语句的执行计划要依次执行的语句。最后一条的结果集就是计划：
 /// - MySQL 系、PG 系：`EXPLAIN <语句>`；
 /// - 达梦：`EXPLAIN FOR <语句>`，返回计划表的各行（`EXPLAIN <语句>` 经 DPI 执行不返回结果集）；
-/// - Oracle：`EXPLAIN PLAN FOR <语句>` 写入计划表，再用 `DBMS_XPLAN.DISPLAY` 读出文本。
+/// - Oracle：`EXPLAIN PLAN FOR <语句>` 写入计划表，再用 `DBMS_XPLAN.DISPLAY` 读出文本；
+/// - SQL Server：`SET SHOWPLAN_ALL ON`（必须单独成批）后执行语句得到估算计划的表格；之后必须执行
+///   `explain_cleanup` 的语句恢复，否则会话之后的语句都只返回计划。
 ///
 /// 都不会真正执行语句（不用 `EXPLAIN ANALYZE`）。
 pub fn explain(family: Family, sql: &str) -> Vec<String> {
@@ -16,15 +19,22 @@ pub fn explain(family: Family, sql: &str) -> Vec<String> {
         ],
         Family::Dameng => vec![format!("EXPLAIN FOR {sql}")],
         Family::MySql | Family::Postgres => vec![format!("EXPLAIN {sql}")],
+        Family::SqlServer => vec!["SET SHOWPLAN_ALL ON".to_string(), sql.to_string()],
     }
 }
 
+/// 生成执行计划之后无论成败都要执行的语句。
+pub fn explain_cleanup(family: Family) -> Option<&'static str> {
+    (family == Family::SqlServer).then_some("SET SHOWPLAN_ALL OFF")
+}
+
 /// 查询当前 schema 的语句，结果是一行一列；MySQL 没有选库时为 NULL。
 pub fn current_schema(family: Family) -> &'static str {
     match family {
         Family::MySql => "SELECT DATABASE()",
         Family::Postgres => "SELECT current_schema()",
         Family::Oracle | Family::Dameng => "SELECT SYS_CONTEXT('USERENV', 'CURRENT_SCHEMA') FROM DUAL",
+        Family::SqlServer => "SELECT DB_NAME()",
     }
 }
 
@@ -32,7 +42,7 @@ pub fn current_schema(family: Family) -> &'static str {
 pub fn set_schema(family: Family, schema: &str) -> String {
     let name = ident::quote_ident(family, schema);
     match family {
-        Family::MySql => format!("USE {name}"),
+        Family::MySql | Family::SqlServer => format!("USE {name}"),
         Family::Postgres => format!("SET search_path TO {name}"),
         Family::Oracle | Family::Dameng => format!("ALTER SESSION SET CURRENT_SCHEMA = {name}"),
     }
@@ -65,6 +75,11 @@ mod tests {
         assert_eq!(set_schema(Family::Oracle, "HR"), "ALTER SESSION SET CURRENT_SCHEMA = \"HR\"");
         assert_eq!(set_schema(Family::Dameng, "x\"y"), "ALTER SESSION SET CURRENT_SCHEMA = \"x\"\"y\"");
         assert!(current_schema(Family::Oracle).contains("CURRENT_SCHEMA"));
+        assert_eq!(set_schema(Family::SqlServer, "a]b"), "USE [a]]b]");
+        assert_eq!(current_schema(Family::SqlServer), "SELECT DB_NAME()");
+        assert_eq!(explain(Family::SqlServer, "SELECT 1"), ["SET SHOWPLAN_ALL ON", "SELECT 1"]);
+        assert_eq!(explain_cleanup(Family::SqlServer), Some("SET SHOWPLAN_ALL OFF"));
+        assert_eq!(explain_cleanup(Family::Postgres), None);
         assert!(may_change_schema("USE") && may_change_schema("ALTER") && !may_change_schema("SELECT"));
     }
 }
```

- [ ] **Step 25: 修改 `crates/dblens-dialect/src/split.rs`**

```diff
--- a/crates/dblens-dialect/src/split.rs
+++ b/crates/dblens-dialect/src/split.rs
@@ -1,6 +1,10 @@
 //! SQL 语句拆分：处理字符串、注释、PG 的 $$ 引号、MySQL 的 DELIMITER 命令，
 //! 以及 Oracle/达梦的 PL/SQL 块和 `/` 结束符。保留每条语句在原文中的字节范围和起始行号，便于定位出错位置。
 //!
+//! SQL Server 不按 `;` 拆分，而是按单独一行的 `GO` 分批（与 SSMS、sqlcmd 一致），每批整段交给服务器：
+//! `GO` 不区分大小写，前后可有空白，可带次数（`GO 5` 把该批重复 5 次，上限 `MAX_GO_COUNT`）和行尾 `--` 注释；
+//! 字符串、`[…]`、`"…"` 标识符和（可嵌套的）块注释里的 `GO` 不算。
+//!
 //! 可以分块喂入（`StreamSplitter`，执行 SQL 文件用）：跨块保留结束符、PL/SQL 块、字符串和注释的状态，
 //! 从上次停下的位置继续扫描。块末尾不足以判断的结构（被切开的结束符、未闭合的字符串或注释、
 //! 未读到换行的行首命令、PL/SQL 块开头的关键字）等下一块再判断，文本结束时按结束处理；
@@ -18,9 +22,13 @@
 //! - `q''` 的定界符必须是 ASCII 字符。
 //! - 达梦是否支持 `q''` 字符串尚未验证。
 //! - 分块喂入时，跨越多块的单个字符串、注释或 `$$` 函数体，每来一块都从它的开头重新查找结尾。
+//! - SQL Server 不识别 sqlcmd 的其他命令（`:setvar`、`:r` 等），按 SQL 原样发给服务器。
 
 use crate::Family;
 
+/// `GO n` 的次数上限，防止误写的大数把一批执行成千上万次。
+pub const MAX_GO_COUNT: usize = 1000;
+
 #[derive(Debug, Clone, PartialEq, Eq)]
 pub struct SqlStatement {
     /// 语句文本：不含结束符，已去掉首尾空白。
@@ -167,7 +175,8 @@ impl StreamSplitter {
                 Step::No => {}
             }
             let rest = &self.buf.as_bytes()[self.i..];
-            if !self.block {
+            // SQL Server 只按 GO 分批
+            if !self.block && self.family != Family::SqlServer {
                 if rest.starts_with(&self.delimiter) {
                     let end = self.i;
                     self.i += self.delimiter.len();
@@ -228,7 +237,7 @@ impl StreamSplitter {
         self.block = false;
     }
 
-    /// 行首命令：MySQL 的 `DELIMITER xx`，Oracle/达梦单独一行的 `/`。
+    /// 行首命令：MySQL 的 `DELIMITER xx`，Oracle/达梦单独一行的 `/`，SQL Server 单独一行的 `GO [n]`。
     /// 行还没读完时，能确定不是命令就返回 `No`，否则等下一块。
     fn line_command(&mut self, eof: bool) -> Step {
         let b = self.buf.as_bytes();
@@ -264,6 +273,26 @@ impl StreamSplitter {
                 }
                 Step::No
             }
+            Family::SqlServer => {
+                if !complete {
+                    let n = head.len().min(2);
+                    let maybe = head[..n].eq_ignore_ascii_case(&b"go"[..n])
+                        && head.get(2).is_none_or(|c| !is_ident_byte(*c));
+                    return if maybe { Step::Wait } else { Step::No };
+                }
+                let Some(count) = go_count(line) else { return Step::No };
+                let before = self.out.len();
+                self.finish_stmt(self.i);
+                // 这一批出现 count 次（count 为 0 时不执行）
+                if self.out.len() > before
+                    && let Some(batch) = self.out.pop()
+                {
+                    self.out.extend(std::iter::repeat_n(batch, count));
+                }
+                self.i = next_line;
+                self.stmt_start = next_line;
+                Step::Done
+            }
             _ if self.plsql() => {
                 if !complete {
                     let maybe = head.first().is_none_or(|&c| c == b'/')
@@ -311,7 +340,7 @@ impl StreamSplitter {
                     None if !eof => return Step::Wait,
                     c => matches!(c, Some(b'!' | b'+')),
                 };
-                let nested = self.family == Family::Postgres;
+                let nested = matches!(self.family, Family::Postgres | Family::SqlServer);
                 let mut depth = 1;
                 let mut j = i + 2;
                 while j < b.len() && depth > 0 {
@@ -355,6 +384,9 @@ impl StreamSplitter {
             scan_quoted(b, i + 1, b'"', self.family == Family::MySql, eof)
         } else if c == b'`' && self.family == Family::MySql {
             scan_quoted(b, i + 1, b'`', false, eof)
+        } else if c == b'[' && self.family == Family::SqlServer {
+            // 方括号标识符，`]]` 表示一个 `]`
+            scan_quoted(b, i + 1, b']', false, eof)
         } else if matches!(c, b'n' | b'N') && oracle && !prev_is_ident {
             // Oracle 的 nq'X…X' 字符串（national 字符集的 q-quote）
             if need(3) && b.get(i + 1).is_none_or(|c| matches!(c, b'q' | b'Q')) {
@@ -398,6 +430,27 @@ impl StreamSplitter {
     }
 }
 
+/// 一行是否是 `GO [n] [-- 注释]`：是则返回次数（没写为 1，超过上限按上限）。
+fn go_count(line: &[u8]) -> Option<usize> {
+    let line = line.trim_ascii();
+    if line.len() < 2 || !line[..2].eq_ignore_ascii_case(b"go") {
+        return None;
+    }
+    let mut rest = &line[2..];
+    if rest.first().is_some_and(|c| !c.is_ascii_whitespace() && *c != b'-') {
+        return None;
+    }
+    rest = rest.trim_ascii_start();
+    let digits = rest.iter().take_while(|c| c.is_ascii_digit()).count();
+    let count = if digits == 0 {
+        1
+    } else {
+        std::str::from_utf8(&rest[..digits]).ok()?.parse::<usize>().unwrap_or(MAX_GO_COUNT).min(MAX_GO_COUNT)
+    };
+    rest = rest[digits..].trim_ascii_start();
+    (rest.is_empty() || rest.starts_with(b"--")).then_some(count)
+}
+
 fn newlines(b: &[u8]) -> usize {
     b.iter().filter(|&&c| c == b'\n').count()
 }
@@ -690,6 +743,48 @@ mod tests {
         );
     }
 
+    #[test]
+    fn sqlserver_batches_split_on_go_lines() {
+        let sql = "CREATE TABLE t (id int);\nINSERT INTO t VALUES (1); SELECT 1\n  go  \n\
+                   CREATE PROCEDURE p AS\nBEGIN\n  SELECT 'GO';\n  /* GO\n /* GO */ GO */\nEND\nGo -- 结束\n\
+                   SELECT [go\nGO], \"x\nGO\" FROM t\nGO";
+        let s = split_statements(sql, Family::SqlServer);
+        let t: Vec<&str> = s.iter().map(|s| s.text.as_str()).collect();
+        assert_eq!(
+            t,
+            [
+                "CREATE TABLE t (id int);\nINSERT INTO t VALUES (1); SELECT 1",
+                "CREATE PROCEDURE p AS\nBEGIN\n  SELECT 'GO';\n  /* GO\n /* GO */ GO */\nEND",
+                "SELECT [go\nGO], \"x\nGO\" FROM t",
+            ]
+        );
+        assert_eq!((s[0].line, s[1].line, s[2].line), (1, 4, 11));
+        for st in &s {
+            assert_eq!(&sql[st.start..st.end], st.text);
+        }
+    }
+
+    #[test]
+    fn sqlserver_go_count_and_lookalikes() {
+        assert_eq!(
+            texts("INSERT INTO t DEFAULT VALUES\nGO 3\nSELECT 1", Family::SqlServer),
+            [
+                "INSERT INTO t DEFAULT VALUES",
+                "INSERT INTO t DEFAULT VALUES",
+                "INSERT INTO t DEFAULT VALUES",
+                "SELECT 1"
+            ]
+        );
+        assert_eq!(texts("SELECT 1\nGO 0\nSELECT 2", Family::SqlServer), ["SELECT 2"]);
+        assert_eq!(texts("SELECT 1\nGO 99999999999999999999999", Family::SqlServer).len(), MAX_GO_COUNT);
+        // 不是 GO 命令：GOTO、GO 后跟别的、行中间的 GO
+        assert_eq!(texts("GOTO x\nSELECT 1 GO\nGO x\nGO;", Family::SqlServer).len(), 1);
+        assert_eq!(texts("SELECT 1;\nSELECT 2;", Family::SqlServer), ["SELECT 1;\nSELECT 2;"]);
+        assert_eq!(go_count(b"go--x"), Some(1));
+        assert_eq!(go_count(b"  GO 2 -- x "), Some(2));
+        assert_eq!(go_count(b"GOx"), None);
+    }
+
     #[test]
     fn mysql_bare_delimiter_is_plain_statement() {
         assert_eq!(texts("DELIMITER\nSELECT 1;", Family::MySql), ["DELIMITER\nSELECT 1"]);
@@ -705,7 +800,7 @@ mod tests {
         let sql = "-- 注释;\nSELECT '数据;\\' FROM t; /* 块 */ # 井\nDELIMITER //\n\
                    SELECT $$体;$$, $a$标;$a$, q'[中;]', nq'<x>' FROM DUAL//\nDELIMITER ;\n\
                    SELECT \"双;\", `反;` ;\nBEGIN NULL; END;\n/\n\u{3000}SELECT 1 -- 行尾\u{feff}";
-        let families = [Family::MySql, Family::Postgres, Family::Oracle, Family::Dameng];
+        let families = [Family::MySql, Family::Postgres, Family::Oracle, Family::Dameng, Family::SqlServer];
         let ends = sql.char_indices().map(|(i, _)| i).chain(std::iter::once(sql.len()));
         for end in ends {
             let prefix = &sql[..end];
@@ -737,7 +832,8 @@ mod tests {
         out
     }
 
-    const FAMILIES: [Family; 4] = [Family::MySql, Family::Postgres, Family::Oracle, Family::Dameng];
+    const FAMILIES: [Family; 5] =
+        [Family::MySql, Family::Postgres, Family::Oracle, Family::Dameng, Family::SqlServer];
 
     /// 上面各测试用过的文本，加上结构可能恰好被切开的例子。
     const SAMPLES: &[&str] = &[
@@ -777,6 +873,7 @@ mod tests {
         "SELECT 1;\r\nSELECT 2;\r\n/\r\nCREATE OR REPLACE\r\n  PACKAGE pk AS x INT; END;\r\n/\r\n",
         "INSERT INTO t VALUES ('a''b', E'\\\\', $q$x$q$, 'c'); -- 末尾注释",
         "SELECT 1 /*+ hint */; SELECT /*! 2 */; #",
+        "SELECT 1\nGO\nSELECT [a]]\nGO], N'b\nGO' /* x /* GO */\nGO */\n  go 2 -- c\nSELECT 3\ngo",
     ];
 
     #[test]
```

- [ ] **Step 26: 修改 `crates/dblens-dialect/src/types.rs`**

```diff
--- a/crates/dblens-dialect/src/types.rs
+++ b/crates/dblens-dialect/src/types.rs
@@ -2,7 +2,7 @@
 //! `timestamp(6) with time zone`），逻辑类型用于跨库映射和设计器。
 //!
 //! 约定：
-//! - 原生 → 逻辑时，没有显式精度的时间类型补上源库的默认精度（PG/Oracle/达梦 6，MySQL 0），
+//! - 原生 → 逻辑时，没有显式精度的时间类型补上源库的默认精度（PG/Oracle/达梦 6，MySQL 0，SQL Server 7），
 //!   这样跨库时目标库不会因为默认精度不同而悄悄改变精度。
 //! - 逻辑 → 原生时，按目标库的上限钳制精度、长度和小数位，保证生成的 DDL 在目标库上可执行。
 
@@ -226,6 +226,35 @@ pub fn logical_type(family: Family, native: &str) -> LogicalType {
             "TIMESTAMP" | "DATETIME" => L::Timestamp { tz, precision: prec(6) },
             _ => L::Other,
         },
+        // SQL Server 的 (MAX) 长度解析为 None
+        Family::SqlServer => match n {
+            "BIT" => L::Bool,
+            // TINYINT 是 0–255
+            "TINYINT" => L::UInt { bits: 8 },
+            "SMALLINT" => L::Int { bits: 16 },
+            "INT" => L::Int { bits: 32 },
+            "BIGINT" => L::Int { bits: 64 },
+            "REAL" => L::Float { bits: 32 },
+            "FLOAT" => L::Float { bits: if len.is_some_and(|p| p <= 24) { 32 } else { 64 } },
+            "DECIMAL" | "NUMERIC" => decimal(),
+            "MONEY" => L::Decimal { precision: Some(19), scale: Some(4) },
+            "SMALLMONEY" => L::Decimal { precision: Some(10), scale: Some(4) },
+            "CHAR" | "NCHAR" => L::Char { len },
+            "VARCHAR" | "NVARCHAR" => L::VarChar { len },
+            "TEXT" | "NTEXT" | "XML" => L::Text,
+            "BINARY" => L::Binary { len },
+            "VARBINARY" => L::VarBinary { len },
+            "IMAGE" => L::Blob,
+            "DATE" => L::Date,
+            "TIME" => L::Time { precision: prec(7) },
+            "DATETIME2" => L::Timestamp { tz: false, precision: prec(7) },
+            "DATETIME" => L::Timestamp { tz: false, precision: Some(3) },
+            "SMALLDATETIME" => L::Timestamp { tz: false, precision: Some(0) },
+            "DATETIMEOFFSET" => L::Timestamp { tz: true, precision: prec(7) },
+            "UNIQUEIDENTIFIER" => L::Uuid,
+            // ROWVERSION / TIMESTAMP（行版本）、空间类型、HIERARCHYID、SQL_VARIANT 等没有对应逻辑类型
+            _ => L::Other,
+        },
     }
 }
 
@@ -236,9 +265,13 @@ fn with_len(name: &str, len: Option<u32>) -> String {
     }
 }
 
-/// 时间精度按目标库上限钳制：Oracle 最高 9，其余最高 6。
+/// 时间精度按目标库上限钳制：Oracle 最高 9，SQL Server 最高 7，其余最高 6。
 fn with_precision(family: Family, name: &str, p: Option<u8>) -> String {
-    let max = if family == Family::Oracle { 9 } else { 6 };
+    let max = match family {
+        Family::Oracle => 9,
+        Family::SqlServer => 7,
+        _ => 6,
+    };
     with_len(name, p.map(|v| u32::from(v.min(max))))
 }
 
@@ -249,6 +282,8 @@ fn decimal_native(family: Family, precision: Option<u32>, scale: Option<i32>) ->
         Family::Postgres => ("NUMERIC", "NUMERIC", 1000),
         Family::Oracle => ("NUMBER", "NUMBER", 38),
         Family::Dameng => ("DEC", "DEC", 38),
+        // SQL Server 的 DECIMAL 不写参数是 (18,0)，无限定的源类型给 (38,10)
+        Family::SqlServer => ("DECIMAL", "DECIMAL(38,10)", 38),
     };
     // 精度 0 在各库都无效，按无限定处理
     let precision = precision.filter(|&p| p > 0);
@@ -281,6 +316,7 @@ fn decimal_native(family: Family, precision: Option<u32>, scale: Option<i32>) ->
             Family::Postgres => "NUMERIC".into(),
             Family::Oracle => "NUMBER".into(),
             Family::Dameng => "DEC".into(),
+            Family::SqlServer => "DECIMAL(38,10)".into(),
         };
     }
     if family == Family::MySql {
@@ -491,6 +527,44 @@ pub fn native_type(family: Family, logical: &LogicalType) -> String {
             L::Json | L::Other => "CLOB".into(),
             L::Uuid => "VARCHAR(36)".into(),
         },
+        // 字符一律用 Unicode 的 N 类型：NCHAR/NVARCHAR 上限 4000 字符，超出用 NVARCHAR(MAX)；二进制上限 8000 字节
+        Family::SqlServer => match logical {
+            L::Bool => "BIT".into(),
+            // TINYINT 无符号，有符号 8 位用 SMALLINT
+            L::Int { bits } => match bits {
+                0..=16 => "SMALLINT",
+                17..=32 => "INT",
+                _ => "BIGINT",
+            }
+            .into(),
+            L::UInt { bits } => match bits {
+                0..=8 => "TINYINT",
+                9..=16 => "INT",
+                17..=32 => "BIGINT",
+                _ => "DECIMAL(20,0)",
+            }
+            .into(),
+            L::Float { bits } => if *bits <= 32 { "REAL" } else { "FLOAT" }.into(),
+            L::Decimal { precision, scale } => decimal_native(family, *precision, *scale),
+            L::Char { len } => match len.unwrap_or(1) {
+                n if n > 4000 => "NVARCHAR(MAX)".into(),
+                n => format!("NCHAR({n})"),
+            },
+            L::VarChar { len: Some(n) } if *n <= 4000 => format!("NVARCHAR({n})"),
+            L::VarChar { .. } | L::Text | L::Json | L::Other => "NVARCHAR(MAX)".into(),
+            L::Binary { len } => match len.unwrap_or(1) {
+                n if n > 8000 => "VARBINARY(MAX)".into(),
+                n => format!("BINARY({n})"),
+            },
+            L::VarBinary { len: Some(n) } if *n <= 8000 => format!("VARBINARY({n})"),
+            L::VarBinary { .. } | L::Blob => "VARBINARY(MAX)".into(),
+            L::Date => "DATE".into(),
+            L::Time { precision } => with_precision(family, "TIME", *precision),
+            L::Timestamp { tz: false, precision } => with_precision(family, "DATETIME2", *precision),
+            L::Timestamp { tz: true, precision } => with_precision(family, "DATETIMEOFFSET", *precision),
+            L::Interval => "NVARCHAR(100)".into(),
+            L::Uuid => "UNIQUEIDENTIFIER".into(),
+        },
     }
 }
 
@@ -499,7 +573,8 @@ mod tests {
     use super::*;
     use LogicalType as L;
 
-    const FAMILIES: [Family; 4] = [Family::MySql, Family::Postgres, Family::Oracle, Family::Dameng];
+    const FAMILIES: [Family; 5] =
+        [Family::MySql, Family::Postgres, Family::Oracle, Family::Dameng, Family::SqlServer];
 
     fn dec(p: Option<u32>, s: Option<i32>) -> L {
         L::Decimal { precision: p, scale: s }
@@ -1107,6 +1182,50 @@ mod tests {
         assert_eq!(native_type(Family::MySql, &L::Char { len: Some(500) }), "VARCHAR(500)");
     }
 
+    #[test]
+    fn sqlserver_round_trip() {
+        let f = Family::SqlServer;
+        for (native, back) in [
+            ("bit", "BIT"),
+            ("tinyint", "TINYINT"),
+            ("smallint", "SMALLINT"),
+            ("int", "INT"),
+            ("bigint", "BIGINT"),
+            ("real", "REAL"),
+            ("float", "FLOAT"),
+            ("decimal(10,2)", "DECIMAL(10,2)"),
+            ("money", "DECIMAL(19,4)"),
+            ("nchar(10)", "NCHAR(10)"),
+            // 非 Unicode 的字符类型统一成 N 类型
+            ("varchar(100)", "NVARCHAR(100)"),
+            ("nvarchar(4000)", "NVARCHAR(4000)"),
+            ("nvarchar(max)", "NVARCHAR(MAX)"),
+            ("varchar(max)", "NVARCHAR(MAX)"),
+            ("ntext", "NVARCHAR(MAX)"),
+            ("binary(16)", "BINARY(16)"),
+            ("varbinary(max)", "VARBINARY(MAX)"),
+            ("image", "VARBINARY(MAX)"),
+            ("date", "DATE"),
+            ("time(3)", "TIME(3)"),
+            ("datetime2(7)", "DATETIME2(7)"),
+            ("datetime", "DATETIME2(3)"),
+            ("smalldatetime", "DATETIME2(0)"),
+            ("datetimeoffset(7)", "DATETIMEOFFSET(7)"),
+            ("uniqueidentifier", "UNIQUEIDENTIFIER"),
+        ] {
+            assert_eq!(native_type(f, &logical_type(f, native)), back, "{native}");
+        }
+        assert_eq!(logical_type(f, "geography"), L::Other);
+        assert_eq!(logical_type(f, "rowversion"), L::Other);
+        // 有符号 8 位放不进 TINYINT
+        assert_eq!(native_type(f, &L::Int { bits: 8 }), "SMALLINT");
+        assert_eq!(native_type(f, &L::VarChar { len: Some(5000) }), "NVARCHAR(MAX)");
+        assert_eq!(native_type(f, &dec(None, None)), "DECIMAL(38,10)");
+        assert_eq!(native_type(f, &ts(true, Some(9))), "DATETIMEOFFSET(7)");
+        // 从 PG 来的 timestamp 到 SQL Server 是 DATETIME2(6)
+        assert_eq!(native_type(f, &logical_type(Family::Postgres, "timestamp")), "DATETIME2(6)");
+    }
+
     #[test]
     fn malformed_input_never_panics() {
         let inputs = [
```

- [ ] **Step 27: 修改 `crates/dblens-dialect/src/variant.rs`**

```diff
--- a/crates/dblens-dialect/src/variant.rs
+++ b/crates/dblens-dialect/src/variant.rs
@@ -15,6 +15,8 @@ pub enum Family {
     Oracle,
     #[serde(rename = "dameng")]
     Dameng,
+    #[serde(rename = "sqlserver")]
+    SqlServer,
 }
 
 /// 兼容模式：人大金仓、openGauss 可以按 Oracle 的规则解释 SQL（空串即 NULL、`NUMBER`、`VARCHAR2`、
@@ -75,6 +77,8 @@ pub enum Variant {
     Oracle,
     #[serde(rename = "dameng")]
     Dameng,
+    #[serde(rename = "sqlserver")]
+    SqlServer,
 }
 
 impl Variant {
@@ -90,6 +94,7 @@ impl Variant {
             | Variant::HighGo => Family::Postgres,
             Variant::Oracle => Family::Oracle,
             Variant::Dameng => Family::Dameng,
+            Variant::SqlServer => Family::SqlServer,
         }
     }
 
@@ -108,6 +113,7 @@ impl Variant {
             Variant::HighGo => msg!("variant.highgo"),
             Variant::Oracle => Msg::raw("Oracle"),
             Variant::Dameng => msg!("variant.dameng"),
+            Variant::SqlServer => Msg::raw("SQL Server"),
         }
     }
 
```

- [ ] **Step 28: 修改 `crates/dblens-service/src/policy.rs`**

```diff
--- a/crates/dblens-service/src/policy.rs
+++ b/crates/dblens-service/src/policy.rs
@@ -1,12 +1,15 @@
 //! 执行策略（设计文档 7.7 节）：所有执行路径都要先问 `Policy`。个人版的 `DefaultPolicy` 只在生产环境的连接上，
-//! 对不带 WHERE 的 UPDATE/DELETE、DROP、TRUNCATE 要求确认；团队版以后用同一接口实现授权、审计等。
+//! 对不带 WHERE 的 UPDATE/DELETE、DROP、TRUNCATE 要求确认（SQL Server 另加 KILL、SHUTDOWN、DBCC、RESTORE、
+//! ALTER DATABASE）；团队版以后用同一接口实现授权、审计等。
+//!
+//! SQL Server 按 `GO` 分批执行，一批里可以有多条语句：分析时取批中最危险的一条（见 `classify_batch`）。
 
 use async_trait::async_trait;
 use dblens_core::{Msg, msg};
 use dblens_dialect::{Dialect, Family};
 use serde::{Deserialize, Serialize};
 use sqlparser::ast::{ObjectName, Statement};
-use sqlparser::dialect::{GenericDialect, MySqlDialect, PostgreSqlDialect};
+use sqlparser::dialect::{GenericDialect, MsSqlDialect, MySqlDialect, PostgreSqlDialect};
 use sqlparser::parser::Parser;
 
 use crate::profile::Environment;
@@ -98,6 +101,11 @@ impl Policy for DefaultPolicy {
                 msg!("policy.prod_no_where", keyword = stmt.keyword)
             }
             "DROP" | "TRUNCATE" => msg!("policy.prod_statement", keyword = stmt.keyword),
+            "KILL" | "SHUTDOWN" | "DBCC" | "RESTORE" | "ALTER DATABASE"
+                if ctx.dialect.family() == Family::SqlServer =>
+            {
+                msg!("policy.prod_statement", keyword = stmt.keyword)
+            }
             _ => return Decision::Allow,
         };
         Decision::Confirm { reason }
@@ -161,8 +169,11 @@ fn from_ast(stmt: &Statement) -> (Option<bool>, Vec<ObjectRef>) {
 }
 
 /// 分析一条语句。MySQL、PG 家族用 sqlparser 解析；Oracle/达梦（PL/SQL 语法 sqlparser 支持不全）
-/// 和解析失败时只看首个关键字，此时无法判断是否带 WHERE。
+/// 和解析失败时只看首个关键字，此时无法判断是否带 WHERE。SQL Server 的一批见 `classify_batch`。
 pub fn classify(dialect: &Dialect, sql: &str) -> StatementInfo {
+    if dialect.family() == Family::SqlServer {
+        return classify_batch(sql);
+    }
     let keyword = first_keyword(sql);
     // PL/SQL 匿名块
     let kind = if keyword == "BEGIN" && matches!(dialect.family(), Family::Oracle | Family::Dameng) {
@@ -173,7 +184,7 @@ pub fn classify(dialect: &Dialect, sql: &str) -> StatementInfo {
     let parsed = match dialect.family() {
         Family::MySql => Parser::parse_sql(&MySqlDialect {}, sql).ok(),
         Family::Postgres => Parser::parse_sql(&PostgreSqlDialect {}, sql).ok(),
-        Family::Oracle | Family::Dameng => None,
+        Family::Oracle | Family::Dameng | Family::SqlServer => None,
     };
     let (has_where, objects) = match parsed.as_deref() {
         Some([stmt]) => from_ast(stmt),
@@ -191,6 +202,123 @@ pub fn classify(dialect: &Dialect, sql: &str) -> StatementInfo {
     StatementInfo { kind, keyword, has_where, objects }
 }
 
+/// SQL Server 中执行前要确认的关键字（另有 `ALTER DATABASE`）。
+const SQLSERVER_DANGEROUS: [&str; 4] = ["KILL", "SHUTDOWN", "DBCC", "RESTORE"];
+
+/// SQL Server 的一批：用 sqlparser 的 MsSQL 方言拆成语句，取第一条危险的语句（不带 WHERE 的 UPDATE/DELETE、DROP、
+/// TRUNCATE、`SQLSERVER_DANGEROUS`、ALTER DATABASE）作为结果；没有危险语句时，有 DDL 则按 DDL（元数据缓存要失效），
+/// 否则按第一条。所有 DDL 涉及的对象都收集起来。
+/// 解析不了（`IF`、`WHILE`、`BEGIN…END` 等过程式代码，或没有用分号分隔的多条语句）时，跳过字符串、方括号和注释扫描关键字：出现危险关键字时
+/// 以它为关键字、`has_where` 为 None，宁可多问一次。
+fn classify_batch(sql: &str) -> StatementInfo {
+    let first = first_keyword(sql);
+    let Ok(stmts) = Parser::parse_sql(&MsSqlDialect {}, sql) else {
+        let words = batch_words(sql);
+        let found = words.iter().enumerate().find_map(|(i, w)| match w.as_str() {
+            "DROP" | "TRUNCATE" | "DELETE" | "UPDATE" => Some(w.clone()),
+            "ALTER" if words.get(i + 1).is_some_and(|n| n == "DATABASE") => Some("ALTER DATABASE".into()),
+            w if SQLSERVER_DANGEROUS.contains(&w) => Some(w.to_string()),
+            _ => None,
+        });
+        let keyword = found.unwrap_or(first);
+        let kind = kind_of_keyword(keyword.split(' ').next().unwrap_or_default());
+        return StatementInfo { kind, keyword, has_where: None, objects: vec![] };
+    };
+    let infos: Vec<StatementInfo> = stmts
+        .iter()
+        .map(|s| {
+            let text = s.to_string();
+            let mut keyword = first_keyword(&text);
+            if keyword == "ALTER" && text.to_uppercase().split_whitespace().nth(1) == Some("DATABASE") {
+                keyword = "ALTER DATABASE".into();
+            }
+            let (has_where, objects) = from_ast(s);
+            StatementInfo {
+                kind: kind_of_keyword(keyword.split(' ').next().unwrap_or_default()),
+                keyword,
+                has_where,
+                objects,
+            }
+        })
+        .collect();
+    let objects: Vec<ObjectRef> = infos.iter().flat_map(|i| i.objects.clone()).collect();
+    let dangerous = |i: &&StatementInfo| match i.keyword.as_str() {
+        "UPDATE" | "DELETE" => i.has_where != Some(true),
+        "DROP" | "TRUNCATE" | "ALTER DATABASE" => true,
+        k => SQLSERVER_DANGEROUS.contains(&k),
+    };
+    let pick = infos
+        .iter()
+        .find(dangerous)
+        .or_else(|| infos.iter().find(|i| i.kind == StatementKind::Ddl))
+        .or(infos.first());
+    match pick {
+        Some(i) => StatementInfo { objects, ..i.clone() },
+        None => StatementInfo { kind: kind_of_keyword(&first), keyword: first, has_where: None, objects },
+    }
+}
+
+/// T-SQL 文本中字符串、方括号和双引号标识符、注释以外的单词（大写）。
+fn batch_words(sql: &str) -> Vec<String> {
+    let mut words = Vec::new();
+    let mut chars = sql.chars().peekable();
+    let mut word = String::new();
+    let flush = |word: &mut String, words: &mut Vec<String>| {
+        if !word.is_empty() {
+            words.push(std::mem::take(word).to_uppercase());
+        }
+    };
+    while let Some(c) = chars.next() {
+        match c {
+            '\'' | '"' | '[' => {
+                flush(&mut word, &mut words);
+                let close = if c == '[' { ']' } else { c };
+                while let Some(d) = chars.next() {
+                    if d == close {
+                        // 两个结束符表示一个字符
+                        if chars.peek() == Some(&close) {
+                            chars.next();
+                        } else {
+                            break;
+                        }
+                    }
+                }
+            }
+            '-' if chars.peek() == Some(&'-') => {
+                flush(&mut word, &mut words);
+                for d in chars.by_ref() {
+                    if d == '\n' {
+                        break;
+                    }
+                }
+            }
+            '/' if chars.peek() == Some(&'*') => {
+                flush(&mut word, &mut words);
+                chars.next();
+                let mut depth = 1;
+                while depth > 0 {
+                    match chars.next() {
+                        Some('*') if chars.peek() == Some(&'/') => {
+                            chars.next();
+                            depth -= 1;
+                        }
+                        Some('/') if chars.peek() == Some(&'*') => {
+                            chars.next();
+                            depth += 1;
+                        }
+                        Some(_) => {}
+                        None => break,
+                    }
+                }
+            }
+            c if c.is_alphanumeric() || c == '_' || c == '@' || c == '#' => word.push(c),
+            _ => flush(&mut word, &mut words),
+        }
+    }
+    flush(&mut word, &mut words);
+    words
+}
+
 #[cfg(test)]
 mod tests {
     use dblens_dialect::Variant;
@@ -239,6 +367,46 @@ mod tests {
         assert_eq!(classify(&pg(), "# c\nSELECT 1").keyword, "SELECT");
     }
 
+    #[test]
+    fn sqlserver_batches_pick_the_most_dangerous_statement() {
+        let ms = Dialect::new(Variant::SqlServer);
+        let s = classify(&ms, "SELECT 1; DELETE FROM dbo.t WHERE id = 1; UPDATE t SET a = 1");
+        assert_eq!((s.keyword.as_str(), s.has_where), ("UPDATE", Some(false)));
+        let s = classify(&ms, "SELECT 1; DELETE FROM t WHERE id = 1");
+        assert_eq!((s.kind, s.keyword.as_str()), (StatementKind::Dql, "SELECT"));
+        // 不写分号的多条语句 sqlparser 解析不了，按关键字扫描，无法判断 WHERE（生产环境上会多问一次）
+        let s = classify(&ms, "SELECT 1\nDELETE FROM t WHERE id = 1");
+        assert_eq!((s.kind, s.keyword.as_str(), s.has_where), (StatementKind::Dml, "DELETE", None));
+        let s = classify(&ms, "SELECT 1; CREATE TABLE dbo.t2 (id int); DROP TABLE [dbo].[t3]");
+        assert_eq!(s.keyword, "DROP");
+        assert_eq!(s.objects.iter().map(|o| o.name.as_str()).collect::<Vec<_>>(), ["t2", "t3"]);
+        let s = classify(&ms, "SELECT 1; CREATE TABLE t2 (id int)");
+        assert_eq!(s.kind, StatementKind::Ddl);
+        assert_eq!(classify(&ms, "ALTER DATABASE x SET OFFLINE").keyword, "ALTER DATABASE");
+        // 解析不了的过程式代码：按关键字扫描，字符串和注释里的不算
+        let s = classify(&ms, "IF 1 = 1 BEGIN PRINT 'DROP TABLE t' /* DELETE */ -- TRUNCATE\nEND");
+        assert_eq!(s.keyword, "IF");
+        let s = classify(&ms, "IF OBJECT_ID('t') IS NOT NULL BEGIN DBCC CHECKDB WITH NO_INFOMSGS END");
+        assert_eq!((s.keyword.as_str(), s.has_where), ("DBCC", None));
+        let s = classify(&ms, "WHILE 1 = 0 BEGIN DELETE FROM [t]] x] WHERE 1 = 1 END");
+        assert_eq!((s.keyword.as_str(), s.has_where), ("DELETE", None));
+        assert_eq!(batch_words("SELECT [a]]b], N'c''d', \"e\" /* x /* y */ z */ f"), ["SELECT", "N", "F"]);
+    }
+
+    #[tokio::test]
+    async fn sqlserver_admin_statements_need_confirmation_on_production() {
+        let ms = Dialect::new(Variant::SqlServer);
+        let c = ExecContext { profile_id: "p".into(), environment: Environment::Prod, dialect: ms };
+        for sql in ["KILL 53", "SHUTDOWN", "DBCC SHRINKDATABASE (x)", "RESTORE DATABASE x FROM DISK = 'a'"] {
+            let d = DefaultPolicy.before_execute(&c, &classify(&ms, sql)).await;
+            assert!(matches!(d, Decision::Confirm { .. }), "{sql}");
+        }
+        // 其他库的 KILL 不在此列
+        let my = Dialect::new(Variant::MySql);
+        let c = ExecContext { profile_id: "p".into(), environment: Environment::Prod, dialect: my };
+        assert_eq!(DefaultPolicy.before_execute(&c, &classify(&my, "KILL 53")).await, Decision::Allow);
+    }
+
     fn ctx(environment: Environment) -> ExecContext {
         ExecContext { profile_id: "p".into(), environment, dialect: pg() }
     }
```

- [ ] **Step 29: 检查**

```bash
export CARGO_INCREMENTAL=0
cargo fmt --all -- --check
cargo clippy --config "source.mirror.registry='sparse+https://rsproxy.cn/index/'" --workspace --all-targets -- -D warnings
DBLENS_LANG=en cargo test --config "source.mirror.registry='sparse+https://rsproxy.cn/index/'" --workspace
```

Expected: 全部测试通过（GO 分批、标识符和字面量、类型、DDL、危险语句的单元测试；`meta_snapshots__sqlserver.snap`）。快照文件随本任务给出；改了模板时用 `INSTA_UPDATE=always` 重新生成并逐个审查差异。

- [ ] **Step 30: 提交**

```bash
git add crates/dblens-core/locales/app.json crates/dblens-dialect/sql/ops/sqlserver.sql crates/dblens-dialect/sql/sqlserver.sql crates/dblens-dialect/src/compare/normalize.rs crates/dblens-dialect/src/ddl.rs crates/dblens-dialect/src/design/catalog.rs crates/dblens-dialect/src/design/rebuild.rs crates/dblens-dialect/src/design/sql.rs crates/dblens-dialect/src/dml.rs crates/dblens-dialect/src/ident.rs crates/dblens-dialect/src/lib.rs crates/dblens-dialect/src/literal.rs crates/dblens-dialect/src/meta/mod.rs crates/dblens-dialect/src/meta/template.rs crates/dblens-dialect/src/migrate/data.rs crates/dblens-dialect/src/migrate/expr.rs crates/dblens-dialect/src/migrate/mod.rs crates/dblens-dialect/src/migrate/names.rs crates/dblens-dialect/src/migrate/script.rs crates/dblens-dialect/src/migrate/types.rs crates/dblens-dialect/src/ops.rs crates/dblens-dialect/src/session.rs crates/dblens-dialect/src/split.rs crates/dblens-dialect/src/types.rs crates/dblens-dialect/src/variant.rs crates/dblens-dialect/tests/meta_snapshots.rs crates/dblens-dialect/tests/snapshots/meta_snapshots__sqlserver.snap crates/dblens-service/src/policy.rs
git commit -m "feat(dialect): SQL Server 家族（标识符、字面量、GO 分批、类型、元数据和运维模板、危险语句）" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 3: 驱动、服务层与接口

- `vendor/tiberius`：crates.io 的 0.13.0 加补丁（见 Step 1–2 和 `vendor/PATCHES.md`）。工作区 `Cargo.toml`：`tiberius = { version = "0.13", default-features = false, features = ["tds80", "rustls", "chrono", "sql-browser-tokio", "winauth"] }`、`tokio-util`（compat），`[patch.crates-io] tiberius = { path = "vendor/tiberius" }`，`vendor/tiberius` 加进 `exclude`。
- `dblens-driver`：`DriverId::SqlServer`（序列化为 `sqlserver`，与前端一致）、URL 前缀 `sqlserver://` / `mssql://`、默认端口 1433。
- `dblens-driver-sqlserver`（新）：`SqlServerDriver`、`SqlServerConnection`。后台任务独占 `Client<Compat<TcpStream>>`，命令走 mpsc、结果走有界通道（256）；无参数时 `simple_query` 整批执行（`USE`、临时表留在会话里），有参数时 `query`（sp_executesql）；结果集后面的 DONE 结束结果集，否则是影响行数；INFO 是消息；取消用 `Notify`（执行开始时 `notified().enable()`）；批内行号换算成错误位置；手动提交时 `BEGIN TRANSACTION`，提交、回滚前看 `@@TRANCOUNT`。选项：`auth=sql|windows|integrated`、`instance`（填了就经 SQL Browser 查端口）、`trust_server_certificate`。
- `dblens-service`：`Profile::in_catalog`、`Profile::scope`（SQL Server 为"连接 id + \u{1f} + 库名"）；元数据连接池和缓存按 scope 区分，`scope_of` 认前缀；`MetadataService::catalogs`；`Service::feature_profile`（M6b 的功能对 SQL Server 报 `sqlserver.later`），设计器、导入、文档导出、比对、迁移、模型的入口改用它；会话执行完执行计划后执行 `explain_cleanup`；`ChangeSet.catalog`（serde 默认值，表结构从该库读取）；注册表加 SQL Server；Cargo 特性 `sqlserver`。
- `dblens-api`：元数据接口加 `?catalog=`，新接口 `GET /profiles/{id}/catalogs`。
- 实库测试：`meta_live`（`fixtures/sqlserver.sql`）、`service/tests/live.rs` 的 `sqlserver_*`（流程、工作台、取消、对象、运维、导出、库和分批与 M6b 拦截、SQL 文件、编辑）、api 的 `sqlserver` 和 `sqlserver_catalogs`、迁移测试的分支；驱动一致性测试（含多结果集、影响行数、PRINT、取消后连接可用、Windows 认证）。
- 后端词条加 `sqlserver.bad_auth`、`sqlserver.connection_lost`、`sqlserver.windows_auth_only`。

**Files:**
- Create（复制自 crates.io，再套补丁）：`vendor/tiberius/`
- Modify: `Cargo.toml`
- Modify: `crates/dblens-api/src/lib.rs`
- Modify: `crates/dblens-api/src/routes.rs`
- Modify: `crates/dblens-api/tests/live.rs`
- Modify: `crates/dblens-core/locales/app.json`
- Modify: `crates/dblens-dialect/Cargo.toml`
- Modify: `crates/dblens-dialect/src/variant.rs`
- Create: `crates/dblens-dialect/tests/fixtures/sqlserver.sql`
- Modify: `crates/dblens-dialect/tests/meta_live.rs`
- Create: `crates/dblens-driver-sqlserver/Cargo.toml`
- Create: `crates/dblens-driver-sqlserver/src/actor.rs`
- Create: `crates/dblens-driver-sqlserver/src/conn.rs`
- Create: `crates/dblens-driver-sqlserver/src/error.rs`
- Create: `crates/dblens-driver-sqlserver/src/exec.rs`
- Create: `crates/dblens-driver-sqlserver/src/lib.rs`
- Create: `crates/dblens-driver-sqlserver/src/types.rs`
- Create: `crates/dblens-driver-sqlserver/tests/conformance.rs`
- Modify: `crates/dblens-driver/Cargo.toml`
- Modify: `crates/dblens-driver/src/params.rs`
- Modify: `crates/dblens-service/Cargo.toml`
- Modify: `crates/dblens-service/src/changes.rs`
- Modify: `crates/dblens-service/src/compare.rs`
- Modify: `crates/dblens-service/src/design.rs`
- Modify: `crates/dblens-service/src/document/job.rs`
- Modify: `crates/dblens-service/src/import/job.rs`
- Modify: `crates/dblens-service/src/metadata.rs`
- Modify: `crates/dblens-service/src/migrate/mod.rs`
- Modify: `crates/dblens-service/src/model/ops.rs`
- Modify: `crates/dblens-service/src/profile.rs`
- Modify: `crates/dblens-service/src/registry.rs`
- Modify: `crates/dblens-service/src/service.rs`
- Modify: `crates/dblens-service/src/session.rs`
- Modify: `crates/dblens-service/tests/live.rs`
- Modify: `crates/dblens-service/tests/migrate_live.rs`
- Modify: `vendor/PATCHES.md`

**Interfaces:**
- Consumes：任务 2 的 `Family::SqlServer`、`MetaKey::Catalogs`、`MetaArgs.catalog`、`explain_cleanup`。
- Produces：`DriverId::SqlServer`；`dblens_driver_sqlserver::{SqlServerDriver, SqlServerConnection}`；`Profile::in_catalog(&self, Option<&str>) -> Profile`、`Profile::scope(&self) -> String`；
  `MetadataService::catalogs(&self, &Profile, filter: Option<&str>) -> Result<Vec<SchemaInfo>>`；接口 `GET /profiles/{id}/catalogs?filter=` → `[{name, is_system}]`、元数据接口的 `catalog` 参数。

- [ ] **Step 1: 复制 tiberius 0.13.0 的原版**

`$CARGO_HOME/registry/src/` 下有 crates.io 的源码（本机是 `D:/Environments/cargo/registry/src/rsproxy.cn-*`）。没有时在临时目录建一个只依赖 `tiberius = "=0.13.0"` 的空项目执行 `cargo fetch`。

```bash
UP=$(ls -d "$CARGO_HOME"/registry/src/*/tiberius-0.13.0 | head -1)
mkdir -p vendor/tiberius
cp -r "$UP"/. vendor/tiberius/
rm -f vendor/tiberius/.cargo-ok
```

然后删去 `vendor/tiberius/Cargo.toml` 中全部 `[[example]]`、`[[test]]`、`[[bench]]` 段（上游的测试和示例不在工作区构建，见 `vendor/PATCHES.md` 开头）。

- [ ] **Step 2: 套用补丁**

在仓库根目录用 `git apply` 套用下面的补丁（或逐处手改）：

```diff
--- a/vendor/tiberius/src/client/connection.rs
+++ b/vendor/tiberius/src/client/connection.rs
@@ -431,7 +431,7 @@
     /// The Attention message carries no payload, so it is written to the wire
     /// as a single end-of-message packet. Draining the acknowledgement leaves
     /// the connection clean and ready to be reused for further queries.
-    pub(crate) async fn cancel_request(&mut self) -> crate::Result<TokenDone> {
+    pub(crate) async fn cancel_request(&mut self) -> crate::Result<()> {
         // Consistency with the other write paths (`send`,
         // `send_sensitive_login`, `flush_stream`): if a previous multi-packet
         // write was interrupted, the connection is already desynced with a
@@ -457,7 +457,45 @@
         self.flush_sink().await?;
         self.poisoned = false;
 
-        TokenStream::new(self).flush_done_attention().await
+        self.drain_to_attention_ack().await
+    }
+
+    /// DBLens 补丁：取消时按包丢弃剩下的响应，直到某条响应以带 DONE_ATTN 的 DONE 令牌结束。
+    ///
+    /// 原来的做法按令牌读到当前响应的末尾为止：结果集读到一半被丢弃时，缓冲区停在令牌中间，而且服务器的
+    /// 取消确认常在单独的一条响应中，留在连接上被下一个请求读到（那次请求就得到空结果）。确认是响应的最后
+    /// 一个令牌，所以只看每条响应末尾的 13 个字节（DONE、DONEPROC、DONEINPROC 都是 13 字节，状态位 0x0020）。
+    async fn drain_to_attention_ack(&mut self) -> crate::Result<()> {
+        const DONE_LEN: usize = 13;
+        const DONE_ATTN: u16 = 0x0020;
+        self.buf.truncate(0);
+        let mut tail: Vec<u8> = Vec::with_capacity(DONE_LEN * 2);
+        loop {
+            let packet = match self.try_next().await? {
+                Some(p) => p,
+                None => {
+                    self.poisoned = true;
+                    return Err(crate::Error::Protocol(
+                        "Never got a DONE token acknowledging the Attention signal.".into(),
+                    ));
+                }
+            };
+            let last = packet.is_last();
+            let (_, payload) = packet.into_parts();
+            tail.extend_from_slice(&payload);
+            if tail.len() > DONE_LEN {
+                tail.drain(..tail.len() - DONE_LEN);
+            }
+            if last {
+                let acked = tail.len() == DONE_LEN
+                    && matches!(tail[0], 0xFD..=0xFF)
+                    && u16::from_le_bytes([tail[1], tail[2]]) & DONE_ATTN != 0;
+                if acked {
+                    return Ok(());
+                }
+                tail.clear();
+            }
+        }
     }
 
     /// Cleans the packet stream from previous use. It is important to use the
--- a/vendor/tiberius/src/result.rs
+++ b/vendor/tiberius/src/result.rs
@@ -1,4 +1,4 @@
-pub use crate::tds::stream::{CommandItem, QueryItem, ResultMetadata};
+pub use crate::tds::stream::{CommandItem, InfoMessage, QueryItem, ResultMetadata};
 use crate::{
     client::Connection,
     error::Error,
--- a/vendor/tiberius/src/tds/codec/token/token_done.rs
+++ b/vendor/tiberius/src/tds/codec/token/token_done.rs
@@ -67,10 +67,17 @@
     /// `true` when the server has set the `DONE_ATTN` status bit, indicating
     /// this DONE token acknowledges a client Attention signal (MS-TDS
     /// section 2.2.7.6).
+    // DBLens 补丁：取消改为按包查找确认（Connection::drain_to_attention_ack），这里不再使用
+    #[allow(dead_code)]
     pub(crate) fn is_attention(&self) -> bool {
         self.status.contains(DoneStatus::Attention)
     }
 
+    /// DBLens 补丁：是否设置了 DONE_COUNT（行数有效）。
+    pub(crate) fn has_count(&self) -> bool {
+        self.status.contains(DoneStatus::Count)
+    }
+
     pub(crate) fn rows(&self) -> u64 {
         // The row count is only meaningful when the DONE_COUNT status bit is
         // set (MS-TDS §2.2.7.6); otherwise the field is not a valid count.
--- a/vendor/tiberius/src/tds/stream/query.rs
+++ b/vendor/tiberius/src/tds/stream/query.rs
@@ -126,6 +126,9 @@
 
             match item {
                 Some(ReceivedToken::NewResultset(_)) => break,
+                // DBLens 补丁：结果集之前的行数和 INFO 消息也要交给调用方，不能跳过
+                Some(ReceivedToken::Info(_)) => break,
+                Some(ReceivedToken::Done(done) | ReceivedToken::DoneInProc(done)) if done.has_count() => break,
                 Some(_) => {
                     self.token_stream.try_next().await?;
                 }
@@ -246,7 +249,7 @@
         let s = self.try_filter_map(|item| async {
             match item {
                 QueryItem::Row(row) => Ok(Some(row)),
-                QueryItem::Metadata(_) => Ok(None),
+                _ => Ok(None),
             }
         });
 
@@ -292,6 +295,7 @@
                 // a result set on the fly rather than discard a leading row.
                 current.get_or_insert_with(Vec::new).push(row);
             }
+            QueryItem::Done(_) | QueryItem::Info(_) => {}
         }
     }
 
@@ -330,6 +334,25 @@
     Row(Row),
     /// Information of the upcoming row data.
     Metadata(ResultMetadata),
+    /// DBLens 补丁：语句结束，带影响（或返回）的行数（DONE / DONEINPROC 设置了 DONE_COUNT）。
+    Done(u64),
+    /// DBLens 补丁：服务器的 INFO 消息（`PRINT`、级别 10 及以下的 `RAISERROR`）。
+    Info(InfoMessage),
+}
+
+/// DBLens 补丁：服务器的 INFO 消息。
+#[derive(Debug, Clone)]
+pub struct InfoMessage {
+    /// 消息号。
+    pub number: u32,
+    /// 级别（10 及以下为提示）。
+    pub class: u8,
+    /// 消息文本。
+    pub message: String,
+    /// 发出消息的过程名（批中直接执行时为空）。
+    pub procedure: String,
+    /// 批内行号。
+    pub line: u32,
 }
 
 impl QueryItem {
@@ -343,8 +366,8 @@
     /// Returns a reference to the metadata, if the item is of a correct variant.
     pub fn as_metadata(&self) -> Option<&ResultMetadata> {
         match self {
-            QueryItem::Row(_) => None,
             QueryItem::Metadata(ref metadata) => Some(metadata),
+            _ => None,
         }
     }
 
@@ -352,15 +375,15 @@
     pub fn as_row(&self) -> Option<&Row> {
         match self {
             QueryItem::Row(ref row) => Some(row),
-            QueryItem::Metadata(_) => None,
+            _ => None,
         }
     }
 
     /// Returns the metadata, if the item is of a correct variant.
     pub fn into_metadata(self) -> Option<ResultMetadata> {
         match self {
-            QueryItem::Row(_) => None,
             QueryItem::Metadata(metadata) => Some(metadata),
+            _ => None,
         }
     }
 
@@ -368,7 +391,7 @@
     pub fn into_row(self) -> Option<Row> {
         match self {
             QueryItem::Row(row) => Some(row),
-            QueryItem::Metadata(_) => None,
+            _ => None,
         }
     }
 }
@@ -423,6 +446,17 @@
 
                     Poll::Ready(Some(Ok(QueryItem::Row(row))))
                 }
+                // DBLens 补丁：带行数的 DONE / DONEINPROC，以及 INFO 消息
+                ReceivedToken::Done(done) | ReceivedToken::DoneInProc(done) if done.has_count() => {
+                    Poll::Ready(Some(Ok(QueryItem::Done(done.rows()))))
+                }
+                ReceivedToken::Info(info) => Poll::Ready(Some(Ok(QueryItem::Info(InfoMessage {
+                    number: info.number,
+                    class: info.class,
+                    message: info.message,
+                    procedure: info.procedure,
+                    line: info.line,
+                })))),
                 _ => continue,
             };
         }
--- a/vendor/tiberius/src/tds/stream/token.rs
+++ b/vendor/tiberius/src/tds/stream/token.rs
@@ -218,6 +218,8 @@
     /// acknowledging DONE token (with the `DONE_ATTN` status bit set) is
     /// received, per MS-TDS section 2.2.1.6. Returns that DONE token so the
     /// connection is left clean and ready for reuse.
+    // DBLens 补丁：取消改为按包查找确认（Connection::drain_to_attention_ack），这里不再使用
+    #[allow(dead_code)]
     pub(crate) async fn flush_done_attention(self) -> crate::Result<TokenDone> {
         let mut stream = self.try_unfold();
 
```

- [ ] **Step 3: 修改 `crates/dblens-api/tests/live.rs`**

```diff
--- a/crates/dblens-api/tests/live.rs
+++ b/crates/dblens-api/tests/live.rs
@@ -1,7 +1,7 @@
 //! 接口的实库测试。设置对应的环境变量时运行，否则直接通过：
 //! DBLENS_TEST_MYSQL_URL、DBLENS_TEST_MARIADB_URL、DBLENS_TEST_PG_URL、DBLENS_TEST_ORACLE_URL、DBLENS_TEST_DM_URL；
 //! 国产库第一批：DBLENS_TEST_TIDB_URL、DBLENS_TEST_OCEANBASE_URL、DBLENS_TEST_OPENGAUSS_URL、
-//! DBLENS_TEST_OPENGAUSS_A_URL、DBLENS_TEST_KINGBASE_URL、DBLENS_TEST_KINGBASE_PG_URL。
+//! DBLENS_TEST_OPENGAUSS_A_URL、DBLENS_TEST_KINGBASE_URL、DBLENS_TEST_KINGBASE_PG_URL；DBLENS_TEST_SQLSERVER_URL。
 //!
 //! 通过 REST 保存连接配置（MySQL、MariaDB、PG 的测试库开了 TLS，要求走 TLS）、测试连接、浏览元数据、打开会话，
 //! 再通过 WebSocket 分批取回 900 行结果。
@@ -145,6 +145,10 @@ async fn flow(env: &str, tls: bool) {
             "WITH RECURSIVE t (n) AS (SELECT 1 UNION ALL SELECT n + 1 FROM t WHERE n < 900) SELECT n FROM t ORDER BY n"
         }
         DriverId::Oracle | DriverId::Dameng => "SELECT LEVEL FROM DUAL CONNECT BY LEVEL <= 900 ORDER BY 1",
+        DriverId::SqlServer => {
+            "SELECT TOP (900) ROW_NUMBER() OVER (ORDER BY (SELECT NULL)) AS n \
+             FROM sys.all_columns a CROSS JOIN sys.all_columns b ORDER BY n"
+        }
     };
     ws.send(Message::Text(json!({"type": "exec", "sql": sql, "max_rows": 400}).to_string().into()))
         .await
@@ -179,6 +183,50 @@ async fn flow(env: &str, tls: bool) {
     assert_eq!(st, StatusCode::NO_CONTENT);
 }
 
+#[tokio::test]
+async fn sqlserver() {
+    flow("DBLENS_TEST_SQLSERVER_URL", true).await;
+}
+
+/// SQL Server 的数据库层：库列表；按库取架构和对象（`catalog` 参数）；M6b 之前的功能返回 400。
+#[tokio::test]
+async fn sqlserver_catalogs() {
+    let Ok(url) = std::env::var("DBLENS_TEST_SQLSERVER_URL") else { return };
+    let (driver, params) = ConnectParams::from_url(&url).unwrap();
+    let dir = tempfile::tempdir().unwrap();
+    let store = Arc::new(Store::open(dir.path()).unwrap());
+    let secrets = Arc::new(Secrets::with_backend(store.clone(), SecretBackend::Vault));
+    secrets.init_vault("主密码").unwrap();
+    let registry = Arc::new(DriverRegistry::new(store.clone()));
+    let service = Arc::new(Service::with_parts(store, secrets, registry, Arc::new(DefaultPolicy)));
+    let app = router(service, ApiConfig { token: TOKEN.into(), allowed_origins: vec![], local_files: false });
+    let profile = json!({
+        "name": "mssql", "driver": driver, "host": params.host, "port": params.port, "user": params.user,
+        "database": params.database, "options": params.options, "environment": "test",
+    });
+    let (st, p) =
+        call(&app, "POST", "/api/profiles", Some(json!({"profile": profile, "password": params.password})))
+            .await;
+    assert_eq!(st, StatusCode::CREATED, "{p}");
+    let id = p["id"].as_str().unwrap();
+    let (st, cats) = call(&app, "GET", &format!("/api/profiles/{id}/catalogs"), None).await;
+    assert_eq!(st, StatusCode::OK, "{cats}");
+    assert!(
+        cats.as_array().unwrap().iter().any(|c| c["name"] == "master" && c["is_system"] == true),
+        "{cats}"
+    );
+    let (st, schemas) = call(&app, "GET", &format!("/api/profiles/{id}/schemas?catalog=master"), None).await;
+    assert_eq!(st, StatusCode::OK, "{schemas}");
+    assert!(schemas.as_array().unwrap().iter().any(|s| s["name"] == "dbo"), "{schemas}");
+    let (st, t) =
+        call(&app, "GET", &format!("/api/profiles/{id}/schemas/sys/objects?catalog=msdb&kind=views"), None)
+            .await;
+    assert_eq!(st, StatusCode::OK, "{t}");
+    let (st, e) =
+        call(&app, "GET", &format!("/api/profiles/{id}/design/table?schema=dbo&name=t"), None).await;
+    assert_eq!(st, StatusCode::BAD_REQUEST, "{e}");
+}
+
 #[tokio::test]
 async fn mysql() {
     flow("DBLENS_TEST_MYSQL_URL", true).await;
```

- [ ] **Step 4: 新建 `crates/dblens-dialect/tests/fixtures/sqlserver.sql`**

```sql
-- @@ drop_objects
DROP SYNONYM IF EXISTS dblens_meta.syn_emp;
DROP VIEW IF EXISTS dblens_meta.v_emp;
DROP PROCEDURE IF EXISTS dblens_meta.p_meta;
DROP FUNCTION IF EXISTS dblens_meta.f_meta;
DROP TABLE IF EXISTS dblens_meta.emp;
DROP TABLE IF EXISTS dblens_meta.dept;
DROP TABLE IF EXISTS dblens_meta.[Mixed Case];
DROP TABLE IF EXISTS dblens_meta.ds_t;
DROP SEQUENCE IF EXISTS dblens_meta.seq_meta;
DROP SCHEMA IF EXISTS dblens_meta
-- @@ create_schema
CREATE SCHEMA dblens_meta
-- @@ dept
CREATE TABLE dblens_meta.dept (id int IDENTITY(1,1) PRIMARY KEY, name nvarchar(50) NOT NULL UNIQUE, note nvarchar(max))
-- @@ dept_c
EXEC sys.sp_addextendedproperty N'MS_Description', N'部门表', N'SCHEMA', N'dblens_meta', N'TABLE', N'dept'
-- @@ seq_meta
CREATE SEQUENCE dblens_meta.seq_meta AS bigint START WITH 100 INCREMENT BY 5 MAXVALUE 99999 CYCLE CACHE 10
-- @@ emp
CREATE TABLE dblens_meta.emp (
  id bigint PRIMARY KEY,
  dept_id int,
  manager_id bigint,
  name nvarchar(100) NOT NULL CONSTRAINT df_emp_name DEFAULT N'无名',
  salary decimal(10,2) CONSTRAINT ck_emp_salary CHECK (salary >= 0),
  created datetime2(6) DEFAULT SYSDATETIME(),
  bio nvarchar(max),
  photo varbinary(max),
  name_upper AS UPPER(name),
  CONSTRAINT fk_emp_dept FOREIGN KEY (dept_id) REFERENCES dblens_meta.dept(id) ON DELETE CASCADE,
  CONSTRAINT fk_emp_mgr FOREIGN KEY (manager_id) REFERENCES dblens_meta.emp(id),
  CONSTRAINT uq_emp_dept_name UNIQUE (dept_id, name))
-- @@ emp_c1
EXEC sys.sp_addextendedproperty N'MS_Description', N'员工表 employees', N'SCHEMA', N'dblens_meta', N'TABLE', N'emp'
-- @@ emp_c2
EXEC sys.sp_addextendedproperty N'MS_Description', N'员工姓名', N'SCHEMA', N'dblens_meta', N'TABLE', N'emp', N'COLUMN', N'name'
-- @@ idx_created
CREATE INDEX idx_emp_created ON dblens_meta.emp (created DESC)
-- @@ idx_salary
CREATE INDEX idx_emp_salary ON dblens_meta.emp (salary) INCLUDE (bio) WHERE salary > 1000
-- @@ trg
CREATE TRIGGER dblens_meta.trg_emp_aiu ON dblens_meta.emp AFTER INSERT, UPDATE AS BEGIN SET NOCOUNT ON; END
-- @@ v_emp
CREATE VIEW dblens_meta.v_emp AS SELECT e.id, e.name, d.name AS dept_name FROM dblens_meta.emp e LEFT JOIN dblens_meta.dept d ON d.id = e.dept_id
-- @@ p_meta
CREATE PROCEDURE dblens_meta.p_meta @a int AS BEGIN SET NOCOUNT ON; END
-- @@ f_meta
CREATE FUNCTION dblens_meta.f_meta(@a int) RETURNS int AS BEGIN RETURN @a + 1; END
-- @@ syn_emp
CREATE SYNONYM dblens_meta.syn_emp FOR dblens_meta.emp
-- @@ mixed
CREATE TABLE dblens_meta.[Mixed Case] ([数洞] nvarchar(10), [select] int NOT NULL PRIMARY KEY)
-- @@ ds_t
CREATE TABLE dblens_meta.ds_t (
  c_bit bit,
  c_tinyint tinyint,
  c_money money,
  c_varchar_max varchar(max),
  c_nchar nchar(4),
  c_time time(3),
  c_dto datetimeoffset(7),
  c_uid uniqueidentifier,
  c_xml xml,
  c_geo geography,
  c_rv rowversion)
-- @@ rows
INSERT INTO dblens_meta.dept (name) VALUES (N'研发'), (N'市场')
```

- [ ] **Step 5: 修改 `crates/dblens-dialect/tests/meta_live.rs`**

```diff
--- a/crates/dblens-dialect/tests/meta_live.rs
+++ b/crates/dblens-dialect/tests/meta_live.rs
@@ -1,7 +1,8 @@
 //! 元数据查询的实库测试。设置对应的环境变量时运行，否则直接通过（与驱动一致性测试相同的约定）：
 //! DBLENS_TEST_MYSQL_URL、DBLENS_TEST_MARIADB_URL、DBLENS_TEST_PG_URL、DBLENS_TEST_ORACLE_URL、DBLENS_TEST_DM_URL；
 //! 国产库第一批：DBLENS_TEST_TIDB_URL、DBLENS_TEST_OCEANBASE_URL、DBLENS_TEST_OPENGAUSS_URL、
-//! DBLENS_TEST_OPENGAUSS_A_URL、DBLENS_TEST_KINGBASE_URL、DBLENS_TEST_KINGBASE_PG_URL。
+//! DBLENS_TEST_OPENGAUSS_A_URL、DBLENS_TEST_KINGBASE_URL、DBLENS_TEST_KINGBASE_PG_URL；
+//! SQL Server：DBLENS_TEST_SQLSERVER_URL（库写测试库，如 `/dblens_test`；元数据查询在连接的当前库中执行）。
 //!
 //! 每个家族先执行 `tests/fixtures/<家族>.sql` 建测试对象，再用全部元数据查询读回来逐项核对。
 //! 兼容库沿用所属家族的脚本，跳过不支持的段、替换写法不同的段（`run_script_with`）。
@@ -33,6 +34,7 @@ async fn connect(env: &str) -> Option<Live> {
         DriverId::Postgres => Box::new(dblens_driver_postgres::PgDriver),
         DriverId::Oracle => Box::new(dblens_driver_oracle::OracleDriver),
         DriverId::Dameng => Box::new(dblens_driver_dameng::DmDriver),
+        DriverId::SqlServer => Box::new(dblens_driver_sqlserver::SqlServerDriver),
     };
     let mut conn = driver.connect(&params).await.unwrap();
     let info = conn.server_info().await.unwrap();
@@ -307,6 +309,124 @@ async fn mariadb_metadata() {
     assert!(seq.iter().any(|q| q.name == "seq_meta"), "{seq:?}");
 }
 
+/// SQL Server 没有 BEFORE 触发器，自引用外键不能级联，不按 `check_common` 检查，逐项核对。
+#[tokio::test]
+async fn sqlserver_metadata() {
+    let Some(mut live) = connect("DBLENS_TEST_SQLSERVER_URL").await else { return };
+    assert_eq!(live.d.variant(), Variant::SqlServer);
+    live.run_script(include_str!("fixtures/sqlserver.sql")).await;
+    let s = "dblens_meta";
+
+    let catalogs = meta::schemas(&live.rows(MetaKey::Catalogs, &MetaArgs::default()).await).unwrap();
+    assert!(catalogs.iter().any(|c| c.name == "master" && c.is_system), "{catalogs:?}");
+    assert!(catalogs.iter().any(|c| !c.is_system), "{catalogs:?}");
+    let schemas = meta::schemas(&live.rows(MetaKey::Schemas, &MetaArgs::default()).await).unwrap();
+    assert!(schemas.iter().any(|x| x.name == s && !x.is_system), "{schemas:?}");
+    assert!(schemas.iter().any(|x| x.name == "sys" && x.is_system), "{schemas:?}");
+    assert!(schemas.iter().any(|x| x.name == "db_owner" && x.is_system), "{schemas:?}");
+
+    let by_comment = MetaArgs { filter: Some("员工".into()), ..args(s) };
+    let hit = meta::objects(MetaKey::Tables, &live.rows(MetaKey::Tables, &by_comment).await).unwrap();
+    assert_eq!(hit.iter().map(|o| o.name.as_str()).collect::<Vec<_>>(), ["emp"]);
+
+    let all = live.tables(s, None).await;
+    let dept = table(&all, "dept");
+    assert_eq!(dept.comment.as_deref(), Some("部门表"));
+    assert!(dept.columns[0].auto_increment && !dept.columns[0].nullable, "{:?}", dept.columns[0]);
+    assert_eq!(dept.primary_key.as_ref().unwrap().columns, ["id"]);
+    assert_eq!(dept.uniques[0].columns, ["name"]);
+
+    let emp = table(&all, "emp");
+    let col = |n: &str| emp.columns.iter().find(|c| c.name == n).unwrap_or_else(|| panic!("emp 没有列 {n}"));
+    let name = col("name");
+    assert!(!name.nullable && name.default.as_deref().unwrap().contains("无名"), "{name:?}");
+    assert_eq!(name.comment.as_deref(), Some("员工姓名"));
+    assert_eq!(name.data_type.native, "nvarchar(100)");
+    assert!(has(col("name_upper").generated.as_deref().unwrap(), "upper"));
+    assert_eq!(col("salary").data_type.native, "decimal(10,2)");
+    assert_eq!(col("created").data_type.logical, LogicalType::Timestamp { tz: false, precision: Some(6) });
+    assert_eq!(col("bio").data_type.native, "nvarchar(max)");
+    let fk = emp.foreign_keys.iter().find(|f| f.ref_table == "dept").unwrap();
+    assert_eq!((fk.columns.clone(), fk.on_delete.as_deref()), (vec!["dept_id".to_string()], Some("CASCADE")));
+    assert_eq!(fk.ref_schema.as_deref(), Some(s));
+    let mgr = emp.foreign_keys.iter().find(|f| f.ref_table == "emp").unwrap();
+    assert_eq!(mgr.on_delete.as_deref(), Some("NO ACTION"));
+    assert_eq!(emp.uniques[0].columns, ["dept_id", "name"]);
+    assert!(emp.checks.iter().any(|c| has(&c.expression, "salary")), "{:?}", emp.checks);
+    let created = emp.indexes.iter().find(|i| i.name == "idx_emp_created").unwrap();
+    assert!(
+        created.columns[0].descending && created.method.as_deref() == Some("NONCLUSTERED"),
+        "{created:?}"
+    );
+    // INCLUDE 列不算索引列
+    let salary = emp.indexes.iter().find(|i| i.name == "idx_emp_salary").unwrap();
+    assert_eq!(salary.columns.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["salary"]);
+    let trg = &emp.triggers[0];
+    assert_eq!(trg.timing.as_deref(), Some("AFTER"));
+    assert!(
+        trg.events.contains(&"INSERT".to_string()) && trg.events.contains(&"UPDATE".to_string()),
+        "{trg:?}"
+    );
+    assert!(has(trg.definition.as_deref().unwrap(), "create trigger"));
+    let one = live.tables(s, Some("emp")).await;
+    assert_eq!(&one[0], emp);
+
+    let mixed = table(&all, "Mixed Case");
+    assert_eq!(live.d.qualified(Some(s), &mixed.name), "dblens_meta.[Mixed Case]");
+    assert!(mixed.columns.iter().any(|c| c.name == "数洞") && mixed.primary_key.is_some());
+
+    let ds = table(&all, "ds_t");
+    assert_eq!(logical(ds, "c_bit"), LogicalType::Bool);
+    assert_eq!(logical(ds, "c_tinyint"), LogicalType::UInt { bits: 8 });
+    assert_eq!(logical(ds, "c_money"), LogicalType::Decimal { precision: Some(19), scale: Some(4) });
+    assert_eq!(logical(ds, "c_varchar_max"), LogicalType::VarChar { len: None });
+    assert_eq!(logical(ds, "c_nchar"), LogicalType::Char { len: Some(4) });
+    assert_eq!(logical(ds, "c_time"), LogicalType::Time { precision: Some(3) });
+    assert_eq!(logical(ds, "c_dto"), LogicalType::Timestamp { tz: true, precision: Some(7) });
+    assert_eq!(logical(ds, "c_uid"), LogicalType::Uuid);
+    assert_eq!(logical(ds, "c_geo"), LogicalType::Other);
+
+    let d = live.d;
+    let columns = live.rows(MetaKey::Columns, &args(s)).await;
+    let views =
+        meta::views(&d, MetaKey::Views, &live.rows(MetaKey::Views, &args(s)).await, &columns).unwrap();
+    assert_eq!(views.iter().find(|v| v.name == "v_emp").unwrap().columns.len(), 3);
+    let def = MetaArgs { name: Some("v_emp".into()), ..args(s) };
+    let text = meta::source_text(MetaKey::ViewDefinition, &live.rows(MetaKey::ViewDefinition, &def).await)
+        .unwrap()
+        .unwrap();
+    assert!(has(&text, "create view") && has(&text, "dept"), "{text}");
+
+    let routines = meta::routines(&live.rows(MetaKey::Routines, &args(s)).await).unwrap();
+    assert!(routines.iter().any(|r| r.name == "p_meta" && r.kind == RoutineKind::Procedure), "{routines:?}");
+    assert!(routines.iter().any(|r| r.name == "f_meta" && r.kind == RoutineKind::Function), "{routines:?}");
+    let src = MetaArgs { name: Some("f_meta".into()), kind: Some("FUNCTION".into()), ..args(s) };
+    let text = meta::source_text(MetaKey::RoutineSource, &live.rows(MetaKey::RoutineSource, &src).await)
+        .unwrap()
+        .unwrap();
+    assert!(has(&text, "create function"), "{text}");
+
+    let seq = meta::sequences(&live.rows(MetaKey::Sequences, &args(s)).await).unwrap();
+    let q = seq.iter().find(|q| q.name == "seq_meta").unwrap();
+    assert_eq!(
+        (q.start.as_deref(), q.increment.as_deref(), q.cycle, q.cache),
+        (Some("100"), Some("5"), true, Some(10))
+    );
+    let syn = meta::synonyms(&live.rows(MetaKey::Synonyms, &args(s)).await).unwrap();
+    assert_eq!(syn.iter().find(|x| x.name == "syn_emp").unwrap().target_name, "emp");
+
+    let t = |table: &str| MetaArgs { table: Some(table.into()), ..args(s) };
+    let refs =
+        meta::referencing_keys(&live.rows(MetaKey::ReferencingKeys, &t("dept")).await, s, "dept").unwrap();
+    assert!(refs.iter().any(|r| r.table == "emp"), "{refs:?}");
+    let deps = meta::dependent_views(&live.rows(MetaKey::DependentViews, &t("emp")).await).unwrap();
+    assert!(deps.iter().any(|v| v.name == "v_emp"), "{deps:?}");
+    meta::grants(&live.rows(MetaKey::Grants, &t("emp")).await).unwrap();
+    let storage = meta::table_storage(&live.rows(MetaKey::TableStorage, &t("dept")).await).unwrap();
+    assert_eq!(storage.row_count, Some(2), "{storage:?}");
+    assert!(!live.d.has_meta(MetaKey::MaterializedViews) && !live.d.has_meta(MetaKey::ReservedWords));
+}
+
 #[tokio::test]
 async fn postgres_metadata() {
     let Some(mut live) = connect("DBLENS_TEST_PG_URL").await else { return };
```

- [ ] **Step 6: 新建 `crates/dblens-driver-sqlserver/tests/conformance.rs`**

```rust
//! 一致性测试，加上 SQL Server 特有的：一批多个结果集、影响行数与 PRINT 消息、批内错误的位置、读一半丢弃后连接仍可用、
//! 手动提交时批里自己提交、Windows 认证。
//! `DBLENS_TEST_SQLSERVER_URL`（如 `sqlserver://sa:密码@192.168.139.106:1433/master`）；
//! Windows 认证：`DBLENS_TEST_SQLSERVER_WINAUTH_URL`（`?auth=windows`，用户名写 `机器名\用户`）、
//! `DBLENS_TEST_SQLSERVER_INTEGRATED_URL`（`?auth=integrated`，在 Windows 上运行）。

use chrono::{FixedOffset, NaiveDate, TimeZone};
use dblens_core::{Error, Value};
use dblens_driver::conformance::{Fixture, load};
use dblens_driver::{ConnectParams, Connection, Driver, ExecEvent, collect_all};
use dblens_driver_sqlserver::SqlServerDriver;

fn fixture(params: ConnectParams) -> Fixture {
    let day = NaiveDate::from_ymd_opt(2024, 2, 29).unwrap();
    Fixture {
        driver: Box::new(SqlServerDriver),
        params,
        select_one: "SELECT 1",
        gen_rows: |n| {
            format!(
                "SELECT TOP ({n}) ROW_NUMBER() OVER (ORDER BY (SELECT NULL)) AS n FROM sys.all_columns a CROSS JOIN sys.all_columns b ORDER BY n"
            )
        },
        long_running: "WAITFOR DELAY '00:00:20'",
        large_text: "SELECT REPLICATE(CAST('a' AS varchar(max)), 100000)",
        create_table: |t| format!("CREATE TABLE {t} (id INT PRIMARY KEY, name NVARCHAR(100))"),
        drop_table_if_exists: |t| format!("DROP TABLE IF EXISTS {t}"),
        insert_with_params: |t| format!("INSERT INTO {t} (id, name) VALUES (@P1, @P2)"),
        select_name_by_id: |t| format!("SELECT name FROM {t} WHERE id = @P1"),
        count_rows: |t| format!("SELECT COUNT(*) FROM {t}"),
        missing_table_query: "SELECT * FROM dblens_no_such_table",
        type_cases: vec![
            ("SELECT CAST(NULL AS int)", Value::Null),
            ("SELECT CAST(1 AS bit)", Value::Bool(true)),
            ("SELECT CAST(255 AS tinyint)", Value::Int(255)),
            ("SELECT CAST(42 AS int)", Value::Int(42)),
            ("SELECT CAST(42 AS bigint)", Value::Int(42)),
            ("SELECT CAST(1.5 AS float)", Value::Float(1.5)),
            ("SELECT CAST(1.5 AS real)", Value::Float(1.5)),
            ("SELECT CAST(123.4500 AS decimal(10,4))", Value::Decimal("123.4500".into())),
            ("SELECT CAST(-0.05 AS numeric(5,2))", Value::Decimal("-0.05".into())),
            (
                "SELECT CAST(12345678901234567890123456789012345678 AS decimal(38,0))",
                Value::Decimal("12345678901234567890123456789012345678".into()),
            ),
            ("SELECT CAST(12.34 AS money)", Value::Decimal("12.3400".into())),
            ("SELECT N'数洞'", Value::String("数洞".into())),
            ("SELECT CAST('abc' AS varchar(10))", Value::String("abc".into())),
            ("SELECT CAST('2024-02-29' AS date)", Value::Date(day)),
            (
                "SELECT CAST('13:45:30.1234567' AS time)",
                Value::Time(chrono::NaiveTime::from_hms_nano_opt(13, 45, 30, 123_456_700).unwrap()),
            ),
            (
                "SELECT CAST('2024-02-29T13:45:30.123456' AS datetime2(6))",
                Value::DateTime(day.and_hms_micro_opt(13, 45, 30, 123_456).unwrap()),
            ),
            (
                "SELECT CAST('2024-02-29T13:45:30' AS datetime)",
                Value::DateTime(day.and_hms_opt(13, 45, 30).unwrap()),
            ),
            (
                "SELECT CAST('2024-02-29T13:45:30+08:00' AS datetimeoffset)",
                Value::DateTimeTz(
                    FixedOffset::east_opt(8 * 3600)
                        .unwrap()
                        .with_ymd_and_hms(2024, 2, 29, 13, 45, 30)
                        .unwrap(),
                ),
            ),
            ("SELECT 0x010203", Value::Bytes(vec![1, 2, 3])),
            (
                "SELECT CAST('a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11' AS uniqueidentifier)",
                Value::Uuid("a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11".parse().unwrap()),
            ),
            ("SELECT CAST('<a>1</a>' AS xml)", Value::String("<a>1</a>".into())),
        ],
    }
}

dblens_driver::conformance_tests!("DBLENS_TEST_SQLSERVER_URL", fixture);

async fn connect(env: &str) -> Option<Box<dyn Connection>> {
    let f = load(env, fixture)?;
    Some(SqlServerDriver.connect(&f.params).await.expect("连接测试库失败"))
}

async fn run(c: &mut dyn Connection, sql: &str) -> Result<Vec<ExecEvent>, Error> {
    let mut s = c.execute(sql, &[]).await?;
    collect_all(s.as_mut(), 1000).await
}

fn kinds(events: &[ExecEvent]) -> Vec<String> {
    events
        .iter()
        .map(|e| match e {
            ExecEvent::ResultStart { columns } => format!("start{}", columns.len()),
            ExecEvent::Rows(r) => format!("rows{}", r.len()),
            ExecEvent::ResultEnd => "end".into(),
            ExecEvent::Affected(n) => format!("affected{n}"),
            ExecEvent::Message(m) => format!("msg:{m}"),
        })
        .collect()
}

#[tokio::test]
async fn batch_with_results_counts_and_messages() {
    let Some(mut c) = connect("DBLENS_TEST_SQLSERVER_URL").await else { return };
    let events = run(
        c.as_mut(),
        "CREATE TABLE #t (id int)\nINSERT INTO #t VALUES (1), (2)\nPRINT N'完成'\nSELECT id FROM #t ORDER BY id\nSELECT 'x' AS a, 2 AS b WHERE 1 = 0\nUPDATE #t SET id = id + 1",
    )
    .await
    .unwrap();
    assert_eq!(
        kinds(&events),
        ["affected2", "msg:完成", "start1", "rows2", "end", "start2", "end", "affected2"]
    );
    // 临时表留在会话中（按批发送，不经 sp_executesql）
    assert_eq!(kinds(&run(c.as_mut(), "SELECT COUNT(*) FROM #t").await.unwrap()), ["start1", "rows1", "end"]);
    let events = run(c.as_mut(), "USE tempdb").await.unwrap();
    assert!(events.iter().all(|e| matches!(e, ExecEvent::Message(_))), "{events:?}");
    let db = run(c.as_mut(), "SELECT DB_NAME()").await.unwrap();
    assert!(matches!(&db[1], ExecEvent::Rows(r) if r[0][0] == Value::String("tempdb".into())), "{db:?}");
}

#[tokio::test]
async fn error_reports_code_and_line_and_keeps_earlier_results() {
    let Some(mut c) = connect("DBLENS_TEST_SQLSERVER_URL").await else { return };
    let sql = "SELECT 1\n\n  SELECT * FROM dblens_missing_table";
    let mut s = c.execute(sql, &[]).await.unwrap();
    let mut events = Vec::new();
    let err = loop {
        match s.next(100).await {
            Ok(Some(e)) => events.push(e),
            Ok(None) => panic!("期望出错：{events:?}"),
            Err(e) => break e,
        }
    };
    drop(s);
    assert_eq!(kinds(&events), ["start1", "rows1", "end"]);
    match err {
        Error::Sql { code, position, .. } => {
            assert_eq!(code, "208");
            assert_eq!(position, Some(sql.find("SELECT *").unwrap() as u32 + 1));
        }
        other => panic!("{other:?}"),
    }
    c.ping().await.unwrap();
}

#[tokio::test]
async fn abandoned_stream_leaves_connection_usable() {
    let Some(mut c) = connect("DBLENS_TEST_SQLSERVER_URL").await else { return };
    let mut s = c
        .execute("SELECT TOP (200000) a.object_id FROM sys.all_columns a CROSS JOIN sys.all_columns b", &[])
        .await
        .unwrap();
    assert!(matches!(s.next(10).await.unwrap(), Some(ExecEvent::ResultStart { .. })));
    assert!(matches!(s.next(10).await.unwrap(), Some(ExecEvent::Rows(_))));
    drop(s);
    let started = std::time::Instant::now();
    assert_eq!(kinds(&run(c.as_mut(), "SELECT 2").await.unwrap()), ["start1", "rows1", "end"]);
    assert!(started.elapsed() < std::time::Duration::from_secs(5), "{:?}", started.elapsed());
}

#[tokio::test]
async fn manual_commit_survives_commit_inside_the_batch() {
    let Some(mut c) = connect("DBLENS_TEST_SQLSERVER_URL").await else { return };
    run(c.as_mut(), "DROP TABLE IF EXISTS dblens_conf_tx2; CREATE TABLE dblens_conf_tx2 (id int)")
        .await
        .unwrap();
    c.set_autocommit(false).await.unwrap();
    run(c.as_mut(), "INSERT INTO dblens_conf_tx2 VALUES (1)\nCOMMIT").await.unwrap();
    // 事务已在批里提交，再提交不报错
    c.commit().await.unwrap();
    run(c.as_mut(), "INSERT INTO dblens_conf_tx2 VALUES (2)").await.unwrap();
    c.rollback().await.unwrap();
    c.set_autocommit(true).await.unwrap();
    let events = run(c.as_mut(), "SELECT COUNT(*) FROM dblens_conf_tx2").await.unwrap();
    assert!(matches!(&events[1], ExecEvent::Rows(r) if r[0][0] == Value::Int(1)), "{events:?}");
    run(c.as_mut(), "DROP TABLE dblens_conf_tx2").await.unwrap();
}

#[tokio::test]
async fn wrong_password_is_auth_error() {
    let Some(f) = load("DBLENS_TEST_SQLSERVER_URL", fixture) else { return };
    let params = ConnectParams { password: "dblens-wrong-password".into(), ..f.params };
    assert!(matches!(SqlServerDriver.connect(&params).await.err(), Some(Error::Auth { .. })));
}

async fn windows_login(env: &str) {
    let Some(mut c) = connect(env).await else { return };
    let events = run(c.as_mut(), "SELECT SUSER_SNAME()").await.unwrap();
    assert!(
        matches!(&events[1], ExecEvent::Rows(r) if matches!(&r[0][0], Value::String(s) if s.contains('\\'))),
        "{events:?}"
    );
}

#[tokio::test]
async fn windows_account_login() {
    windows_login("DBLENS_TEST_SQLSERVER_WINAUTH_URL").await;
}

#[tokio::test]
async fn integrated_login() {
    windows_login("DBLENS_TEST_SQLSERVER_INTEGRATED_URL").await;
}
```

- [ ] **Step 7: 修改 `crates/dblens-service/tests/live.rs`**

```diff
--- a/crates/dblens-service/tests/live.rs
+++ b/crates/dblens-service/tests/live.rs
@@ -40,6 +40,8 @@ struct Live {
     compat: Compat,
     /// 测试表所在的 schema。
     schema: String,
+    /// 连接的库（SQL Server 的会话"当前 schema"是库）。
+    database: Option<String>,
 }
 
 const LIB_OPTIONS: [&str; 2] = ["oracle_client_lib_dir", "dpi_lib_dir"];
@@ -65,7 +67,9 @@ async fn setup(env: &str) -> Option<Live> {
         DriverId::Mysql => params.database.clone().unwrap(),
         DriverId::Postgres => "public".to_string(),
         DriverId::Oracle | DriverId::Dameng => params.user.to_uppercase(),
+        DriverId::SqlServer => "dbo".to_string(),
     };
+    let database = params.database.clone();
     let input = ProfileInput {
         name: env.to_string(),
         driver,
@@ -83,14 +87,14 @@ async fn setup(env: &str) -> Option<Live> {
     eprintln!("{env}：{} {:?} {:?}", summary.variant_name.current(), summary.version_code, summary.version);
     let profile = service.create_profile(input, Some(&params.password)).unwrap();
     let (variant, compat) = (summary.variant, summary.compat);
-    Some(Live { _dir: dir, service, profile_id: profile.id, driver, variant, compat, schema })
+    Some(Live { _dir: dir, service, profile_id: profile.id, driver, variant, compat, schema, database })
 }
 
 impl Live {
     fn dual(&self, expr: &str) -> String {
         match self.driver {
             DriverId::Oracle | DriverId::Dameng => format!("SELECT {expr} FROM DUAL"),
-            DriverId::Mysql | DriverId::Postgres => format!("SELECT {expr}"),
+            DriverId::Mysql | DriverId::Postgres | DriverId::SqlServer => format!("SELECT {expr}"),
         }
     }
 
@@ -189,13 +193,17 @@ async fn workbench_flow(env: &str) {
     let info = s.open_session(&live.profile_id).await.unwrap();
     let id = info.id;
     let schema = info.schema.expect("应读到当前 schema");
-    assert!(schema.eq_ignore_ascii_case(&live.schema), "{schema}");
+    // SQL Server 的会话"当前 schema"是当前库
+    let expected =
+        if live.driver == DriverId::SqlServer { live.database.clone().unwrap() } else { live.schema.clone() };
+    assert!(schema.eq_ignore_ascii_case(&expected), "{schema}");
 
     // 切换 schema 再切回
     let other = match live.driver {
         DriverId::Mysql => "information_schema",
         DriverId::Postgres => "pg_catalog",
         DriverId::Oracle | DriverId::Dameng => "SYS",
+        DriverId::SqlServer => "master",
     };
     s.sessions.set_schema(id, other).await.unwrap();
     assert_eq!(s.sessions.info(id).unwrap().schema.as_deref(), Some(other));
@@ -292,6 +300,69 @@ async fn cancel_flow(env: &str, slow: &str) {
     s.sessions.close(id, false).await.unwrap();
 }
 
+#[tokio::test]
+async fn sqlserver_flow() {
+    full_flow("DBLENS_TEST_SQLSERVER_URL").await;
+}
+
+#[tokio::test]
+async fn sqlserver_workbench() {
+    workbench_flow("DBLENS_TEST_SQLSERVER_URL").await;
+}
+
+#[tokio::test]
+async fn sqlserver_cancel() {
+    cancel_flow("DBLENS_TEST_SQLSERVER_URL", "WAITFOR DELAY '00:00:30'").await;
+}
+
+#[tokio::test]
+async fn sqlserver_objects() {
+    objects_flow("DBLENS_TEST_SQLSERVER_URL").await;
+}
+
+#[tokio::test]
+async fn sqlserver_ops() {
+    ops_flow("DBLENS_TEST_SQLSERVER_URL", &[]).await;
+}
+
+#[tokio::test]
+async fn sqlserver_export() {
+    export_flow("DBLENS_TEST_SQLSERVER_URL").await;
+}
+
+/// SQL Server：库列表、按库浏览（元数据连接以所选库为初始库）、执行计划后会话恢复正常、
+/// 一批里的多个结果集、M6b 之前的功能被拒绝。
+#[tokio::test]
+async fn sqlserver_catalogs_batches_and_gating() {
+    let Some(live) = setup("DBLENS_TEST_SQLSERVER_URL").await else { return };
+    let s = &live.service;
+    let profile = s.profile(&live.profile_id).unwrap();
+    let catalogs = s.metadata.catalogs(&profile, None).await.unwrap();
+    assert!(catalogs.iter().any(|c| c.name == "master" && c.is_system), "{catalogs:?}");
+    let in_master = profile.in_catalog(Some("master"));
+    let tables = s.metadata.objects(&in_master, "dbo", MetaKey::Tables, None).await.unwrap();
+    assert!(
+        tables.iter().any(|t| t.name == "spt_monitor" || t.name.starts_with("spt_")) || tables.is_empty()
+    );
+    let schemas = s.metadata.schemas(&in_master, None).await.unwrap();
+    assert!(schemas.iter().any(|x| x.name == "dbo"), "{schemas:?}");
+
+    let id = s.open_session(&live.profile_id).await.unwrap().id;
+    // 执行计划之后，同一会话的语句照常执行（SHOWPLAN 已关掉）
+    let events = live.submit(id, ExecRequest { explain: true, ..ExecRequest::new("SELECT 1 AS a") }).await;
+    assert!(!events.iter().any(|e| matches!(e, SessionEvent::Failed { .. })), "{events:?}");
+    let v = first_value(&live.run(id, "SELECT 42").await);
+    assert_eq!(v, Value::Int(42));
+    // 用 GO 分成两批，第一批里有两个结果集
+    let events = live.run(id, "SELECT 1 AS a\nSELECT 2 AS b\nGO\nSELECT 3 AS c").await;
+    let starts = events.iter().filter(|e| matches!(e, SessionEvent::ResultStart { .. })).count();
+    assert_eq!(starts, 3, "{events:?}");
+    s.sessions.close(id, false).await.unwrap();
+
+    assert!(matches!(s.design_table(&live.profile_id, "dbo", "t").await, Err(Error::Invalid { .. })));
+    s.delete_profile(&live.profile_id).await.unwrap();
+}
+
 #[tokio::test]
 async fn mysql_flow() {
     full_flow("DBLENS_TEST_MYSQL_URL").await;
@@ -349,6 +420,13 @@ async fn objects_flow(env: &str) {
         DriverId::Mysql => {
             &["DROP VIEW IF EXISTS c2_v", "DROP TABLE IF EXISTS c2_child", "DROP TABLE IF EXISTS c2_parent"]
         }
+        DriverId::SqlServer => &[
+            "DROP VIEW IF EXISTS c2_v",
+            "DROP TABLE IF EXISTS c2_child",
+            "DROP TABLE IF EXISTS c2_parent",
+            "DROP SEQUENCE IF EXISTS c2_seq",
+            "DROP FUNCTION IF EXISTS c2_f",
+        ],
         _ => &[
             "DROP VIEW IF EXISTS c2_v",
             "DROP TABLE IF EXISTS c2_child",
@@ -376,6 +454,15 @@ async fn objects_flow(env: &str) {
             "CREATE SEQUENCE c2_seq START 10 INCREMENT 2",
             "CREATE FUNCTION c2_f() RETURNS int LANGUAGE sql AS 'SELECT 1'",
         ],
+        DriverId::SqlServer => vec![
+            "CREATE TABLE c2_parent (id INT PRIMARY KEY, code NVARCHAR(10) NOT NULL UNIQUE)",
+            "EXEC sys.sp_addextendedproperty N'MS_Description', N'父表', N'SCHEMA', N'dbo', N'TABLE', N'c2_parent'",
+            "CREATE TABLE c2_child (id INT IDENTITY PRIMARY KEY, parent_id INT, qty INT CHECK (qty > 0), CONSTRAINT c2_fk FOREIGN KEY (parent_id) REFERENCES c2_parent (id) ON DELETE CASCADE)",
+            "CREATE INDEX c2_idx ON c2_child (qty DESC)",
+            "CREATE VIEW c2_v AS SELECT id, code FROM c2_parent",
+            "CREATE SEQUENCE c2_seq START WITH 10 INCREMENT BY 2",
+            "CREATE FUNCTION c2_f() RETURNS int AS BEGIN RETURN 1 END",
+        ],
         _ => vec![
             "CREATE TABLE c2_parent (id INT PRIMARY KEY, code VARCHAR(10) NOT NULL UNIQUE)",
             "COMMENT ON TABLE c2_parent IS '父表'",
@@ -554,6 +641,12 @@ async fn export_flow(env: &str) {
             "SELECT LEVEL AS \"n\" FROM DUAL CONNECT BY LEVEL <= 2500",
             2500,
         ),
+        DriverId::SqlServer => (
+            "SELECT 1 AS id, N'中文' AS name, REPLICATE(CAST(N'数' AS nvarchar(max)), 100000) AS txt, \
+             LEN(REPLICATE(CAST(N'数' AS nvarchar(max)), 100000)) AS len",
+            "SELECT TOP (2500) ROW_NUMBER() OVER (ORDER BY (SELECT NULL)) AS n FROM sys.all_columns a CROSS JOIN sys.all_columns b",
+            2500,
+        ),
     };
     let export = |sql: &str, format: ExportFormat, name: &str| {
         let path = dir.path().join(name);
@@ -687,10 +780,17 @@ async fn editing_flow(env: &str) {
             "CREATE TABLE m2_t (id INT IDENTITY(1, 1) PRIMARY KEY, code VARCHAR(10) NOT NULL UNIQUE, \
              name VARCHAR(20), qty INT DEFAULT 5, price DECIMAL(10, 2))"
         }
+        DriverId::SqlServer => {
+            "CREATE TABLE m2_t (id INT IDENTITY(1, 1) PRIMARY KEY, code NVARCHAR(10) NOT NULL UNIQUE, \
+             name NVARCHAR(20), qty INT DEFAULT 5, price DECIMAL(10, 2))"
+        }
     };
     live.run(id, create).await;
     live.run(id, "CREATE TABLE m2_nokey (a INT)").await;
-    for (code, name, qty) in [("A", "'甲'", 1), ("B", "NULL", 2), ("C", "'丙'", 3)] {
+    // SQL Server 的非 Unicode 字面量按库的排序规则转码，中文要写 N''
+    let (jia, bing) =
+        if live.driver == DriverId::SqlServer { ("N'甲'", "N'丙'") } else { ("'甲'", "'丙'") };
+    for (code, name, qty) in [("A", jia, 1), ("B", "NULL", 2), ("C", bing, 3)] {
         live.run(
             id,
             &format!("INSERT INTO m2_t (code, name, qty, price) VALUES ('{code}', {name}, {qty}, 1.50)"),
@@ -701,7 +801,12 @@ async fn editing_flow(env: &str) {
     let id_of = |code: &str| one(format!("SELECT id FROM m2_t WHERE code = '{code}'"));
     let (a, b, c) = (id_of("A").await, id_of("B").await, id_of("C").await);
     let map = |pairs: Vec<(&str, Value)>| pairs.into_iter().map(|(k, v)| (n(k), v)).collect();
-    let set = |changes: Vec<RowChange>| ChangeSet { schema: live.schema.clone(), table: n("m2_t"), changes };
+    let set = |changes: Vec<RowChange>| ChangeSet {
+        catalog: None,
+        schema: live.schema.clone(),
+        table: n("m2_t"),
+        changes,
+    };
 
     // 一次提交：修改 A、删除 B、新增 D（id、qty 留空，用自增和默认值）
     let changes = set(vec![
@@ -748,7 +853,8 @@ async fn editing_flow(env: &str) {
 
     // 冲突：另一个会话先改了 C 的 name，再按旧值提交，整体回滚
     let other = s.open_session(&live.profile_id).await.unwrap().id;
-    live.run(other, "UPDATE m2_t SET name = '别人' WHERE code = 'C'").await;
+    let n_prefix = if live.driver == DriverId::SqlServer { "N" } else { "" };
+    live.run(other, &format!("UPDATE m2_t SET name = {n_prefix}'别人' WHERE code = 'C'")).await;
     s.sessions.close(other, true).await.unwrap();
     let conflict = set(vec![
         RowChange::Update {
@@ -781,6 +887,7 @@ async fn editing_flow(env: &str) {
 
     // 没有主键或唯一键的表不能编辑
     let nokey = ChangeSet {
+        catalog: None,
         schema: live.schema.clone(),
         table: n("m2_nokey"),
         changes: vec![RowChange::Insert { values: map(vec![("a", Value::Int(1))]) }],
@@ -1066,12 +1173,23 @@ async fn sql_file_flow(env: &str) {
              CREATE OR REPLACE PROCEDURE m2b_p IS\nBEGIN\n  INSERT INTO m2b_f VALUES (1);\n  INSERT INTO m2b_f VALUES (2);\nEND;\n/\n\
              BEGIN m2b_p; END;\n/\nINSERT INTO m2b_f VALUES (3);\nDROP PROCEDURE m2b_p;\n"
         }
+        // SQL Server：按 GO 分批，CREATE PROCEDURE 单独一批
+        DriverId::SqlServer => {
+            "DROP TABLE IF EXISTS m2b_f\nDROP PROCEDURE IF EXISTS m2b_p\nCREATE TABLE m2b_f (a INT)\nGO\n\
+             CREATE PROCEDURE m2b_p AS\nBEGIN\n  INSERT INTO m2b_f VALUES (1);\n  INSERT INTO m2b_f VALUES (2);\nEND\nGO\n\
+             EXEC m2b_p\nINSERT INTO m2b_f VALUES (3)\nGO\nDROP PROCEDURE m2b_p\n"
+        }
     };
     let request = |sql: &str, continue_on_error: bool| SqlFileRequest {
         profile_id: live.profile_id.clone(),
         source: upload(s, "脚本.sql", sql),
         encoding: Default::default(),
-        schema: Some(live.schema.clone()),
+        // SQL Server 的"当前 schema"是库
+        schema: Some(if live.driver == DriverId::SqlServer {
+            live.database.clone().unwrap()
+        } else {
+            live.schema.clone()
+        }),
         continue_on_error,
         transaction: false,
     };
@@ -1086,12 +1204,21 @@ async fn sql_file_flow(env: &str) {
         "{env}"
     );
 
-    // 出错时继续：失败的一条记下行号，其余照常执行
-    let script =
-        "INSERT INTO m2b_f VALUES (4);\nINSERT INTO m2b_nope VALUES (1);\nINSERT INTO m2b_f VALUES (5);\n";
+    // 出错时继续：失败的一条记下行号，其余照常执行（SQL Server 按 GO 分批，失败的一批从第 3 行开始）
+    let (script, failed_line) = if live.driver == DriverId::SqlServer {
+        (
+            "INSERT INTO m2b_f VALUES (4)\nGO\nINSERT INTO m2b_nope VALUES (1)\nGO\nINSERT INTO m2b_f VALUES (5)\n",
+            3,
+        )
+    } else {
+        (
+            "INSERT INTO m2b_f VALUES (4);\nINSERT INTO m2b_nope VALUES (1);\nINSERT INTO m2b_f VALUES (5);\n",
+            2,
+        )
+    };
     let st = wait_task(s, &s.start_sql_file(request(script, true), false).await.unwrap()).await;
     assert_eq!((st.state, st.progress.statements_failed), (TaskState::Done, 1), "{env}：{st:?}");
-    assert_eq!(st.failures[0].line, 2, "{env}");
+    assert_eq!(st.failures[0].line, failed_line, "{env}");
     // 出错时停止
     let st = wait_task(s, &s.start_sql_file(request(script, false), false).await.unwrap()).await;
     assert_eq!((st.state, st.progress.statements_done), (TaskState::Failed, 2), "{env}：{st:?}");
@@ -1110,6 +1237,16 @@ async fn sql_file_flow(env: &str) {
     s.sessions.close(id, false).await.unwrap();
 }
 
+#[tokio::test]
+async fn sqlserver_sql_file() {
+    sql_file_flow("DBLENS_TEST_SQLSERVER_URL").await;
+}
+
+#[tokio::test]
+async fn sqlserver_editing() {
+    editing_flow("DBLENS_TEST_SQLSERVER_URL").await;
+}
+
 #[tokio::test]
 async fn mysql_sql_file() {
     sql_file_flow("DBLENS_TEST_MYSQL_URL").await;
@@ -1167,6 +1304,8 @@ async fn design_flow(env: &str) {
         DriverId::Postgres => ["bigint", "varchar(20)", "numeric(10,2)", "varchar(40)"],
         DriverId::Oracle => ["NUMBER(19)", "VARCHAR2(20)", "NUMBER(10,2)", "VARCHAR2(40)"],
         DriverId::Dameng => ["BIGINT", "VARCHAR(20)", "DECIMAL(10,2)", "VARCHAR(40)"],
+        // 表设计器的 SQL Server 支持在 M6b，这里只为穷尽分支
+        DriverId::SqlServer => ["BIGINT", "NVARCHAR(20)", "DECIMAL(10,2)", "NVARCHAR(40)"],
     };
     let col = |name: &str, native: &str| Column {
         name: n(name),
@@ -1291,6 +1430,7 @@ async fn design_flow(env: &str) {
         DriverId::Oracle | DriverId::Dameng => &[
             "CREATE OR REPLACE TRIGGER m3_trg BEFORE INSERT ON m3_t FOR EACH ROW BEGIN :NEW.remark := 'trg'; END;",
         ],
+        DriverId::SqlServer => &[],
     };
     for sql in trigger {
         live.run(id, sql).await;
@@ -1323,7 +1463,7 @@ async fn design_flow(env: &str) {
         DriverId::Mysql => "INT",
         DriverId::Postgres => "integer",
         DriverId::Oracle => "NUMBER(10)",
-        DriverId::Dameng => "INT",
+        DriverId::Dameng | DriverId::SqlServer => "INT",
     };
     d.columns[4].data_type.native = number.into();
     let req = request(Some("m3_t"), d.clone(), Renames::default(), true, Some(t.fingerprint.clone()));
@@ -1658,7 +1798,7 @@ async fn model_flow(env: &str) {
     let other: Option<&'static str> = match live.driver {
         DriverId::Postgres => Some("m4_b"),
         DriverId::Mysql => Some("dblens_test_m4b"),
-        DriverId::Oracle | DriverId::Dameng => None,
+        DriverId::Oracle | DriverId::Dameng | DriverId::SqlServer => None,
     };
     let created = match other {
         Some(o) => {
```

- [ ] **Step 8: 修改 `crates/dblens-service/tests/migrate_live.rs`**

```diff
--- a/crates/dblens-service/tests/migrate_live.rs
+++ b/crates/dblens-service/tests/migrate_live.rs
@@ -68,6 +68,7 @@ async fn setup(envs: [&str; 2]) -> Option<(Arc<Service>, Vec<End>, tempfile::Tem
             DriverId::Mysql => params.database.clone().unwrap(),
             DriverId::Postgres => "public".to_string(),
             DriverId::Oracle | DriverId::Dameng => params.user.to_uppercase(),
+            DriverId::SqlServer => "dbo".to_string(),
         };
         let input = ProfileInput {
             name: env.to_string(),
@@ -126,6 +127,7 @@ fn drops(end: &End, names: &[String], seq: &str) -> Vec<String> {
             }
             DriverId::Dameng | DriverId::Postgres => format!("DROP TABLE IF EXISTS \"{n}\" CASCADE"),
             DriverId::Mysql => format!("DROP TABLE IF EXISTS `{n}`"),
+            DriverId::SqlServer => format!("DROP TABLE IF EXISTS [{n}]"),
         })
         .collect();
     out.push(match end.driver {
@@ -134,6 +136,7 @@ fn drops(end: &End, names: &[String], seq: &str) -> Vec<String> {
         ),
         DriverId::Dameng | DriverId::Postgres => format!("DROP SEQUENCE IF EXISTS \"{seq}\""),
         DriverId::Mysql => "SELECT 1".into(),
+        DriverId::SqlServer => format!("DROP SEQUENCE IF EXISTS [{seq}]"),
     });
     out
 }
@@ -143,6 +146,8 @@ fn source_ddl(end: &End, p: &str) -> Vec<String> {
     let opengauss = end.variant == Variant::OpenGauss;
     let n = ROWS;
     match end.driver {
+        // SQL Server 的迁移在 M6b，路线中没有它
+        DriverId::SqlServer => vec![],
         DriverId::Mysql => vec![
             format!(
                 "CREATE TABLE {p}_cust (id BIGINT AUTO_INCREMENT PRIMARY KEY, name VARCHAR(50) NOT NULL DEFAULT 'n/a' COMMENT 'name', \
```

- [ ] **Step 9: 修改 `Cargo.toml`**

```diff
--- a/Cargo.toml
+++ b/Cargo.toml
@@ -22,6 +22,7 @@ dblens-driver-dameng = { path = "crates/dblens-driver-dameng" }
 dblens-driver-mysql = { path = "crates/dblens-driver-mysql" }
 dblens-driver-oracle = { path = "crates/dblens-driver-oracle" }
 dblens-driver-postgres = { path = "crates/dblens-driver-postgres" }
+dblens-driver-sqlserver = { path = "crates/dblens-driver-sqlserver" }
 
 argon2 = "0.6.0"
 async-trait = "0.1.92"
@@ -63,10 +64,13 @@ tauri-plugin-global-shortcut = "2.4.1"
 tauri-plugin-updater = "2.14.0"
 tempfile = "3.27.0"
 thiserror = "2.0.21"
+# SQL Server：替换为 vendor/ 中打了补丁的版本（见 vendor/PATCHES.md）
+tiberius = { version = "0.13.0", default-features = false, features = ["tds80", "rustls", "chrono", "sql-browser-tokio", "winauth"] }
 tokio = { version = "1.54.1", features = ["rt-multi-thread", "macros", "sync", "time"] }
 tokio-postgres = { version = "0.7.18", features = ["with-chrono-0_4", "with-serde_json-1", "with-uuid-1"] }
 tokio-postgres-rustls = "0.14.0"
 tokio-tungstenite = "0.30.0"
+tokio-util = { version = "0.7.20", features = ["compat"] }
 tower = { version = "0.5.3", features = ["util"] }
 tracing = "0.1.44"
 tracing-appender = "0.2.5"
@@ -76,7 +80,8 @@ uuid = { version = "1.28.0", features = ["serde", "v4"] }
 webpki-roots = "1.0.11"
 zip = { version = "9.0.5", default-features = false, features = ["deflate-flate2"] }
 
-# openGauss 的 SHA256 认证：替换为 vendor/ 中打了补丁的版本（见 vendor/PATCHES.md）
+# 打了补丁的第三方库（见 vendor/PATCHES.md）：openGauss 的 SHA256 认证；SQL Server 的影响行数和 INFO 消息
 [patch.crates-io]
 postgres-protocol = { path = "vendor/postgres-protocol" }
+tiberius = { path = "vendor/tiberius" }
 tokio-postgres = { path = "vendor/tokio-postgres" }
```

- [ ] **Step 10: 修改 `crates/dblens-api/src/lib.rs`**

```diff
--- a/crates/dblens-api/src/lib.rs
+++ b/crates/dblens-api/src/lib.rs
@@ -75,6 +75,7 @@ pub fn router(service: Arc<Service>, config: ApiConfig) -> Router {
         )
         .route("/profiles/{id}/password", put(routes::set_password))
         .route("/profiles/{id}/refresh", post(routes::refresh))
+        .route("/profiles/{id}/catalogs", get(routes::catalogs))
         .route("/profiles/{id}/schemas", get(routes::schemas))
         .route("/profiles/{id}/schemas/{schema}/objects", get(routes::objects))
         .route("/profiles/{id}/schemas/{schema}/tables", get(routes::tables))
```

- [ ] **Step 11: 修改 `crates/dblens-api/src/routes.rs`**

```diff
--- a/crates/dblens-api/src/routes.rs
+++ b/crates/dblens-api/src/routes.rs
@@ -238,12 +238,29 @@ pub struct FilterQuery {
     pub filter: Option<String>,
 }
 
-pub async fn schemas(
+/// 所在的数据库（SQL Server 的连接 → 数据库 → 架构）。不给时用连接配置里的初始库。
+#[derive(Deserialize)]
+pub struct CatalogQuery {
+    pub catalog: Option<String>,
+}
+
+/// 数据库列表（只有 SQL Server 有这一层，其他驱动返回空数组）。
+pub async fn catalogs(
     State(s): State<AppState>,
     Path(id): Path<String>,
     Query(q): Query<FilterQuery>,
 ) -> ApiResult<Json<Vec<SchemaInfo>>> {
     let p = s.service.profile(&id)?;
+    Ok(Json(s.service.metadata.catalogs(&p, q.filter.as_deref()).await?))
+}
+
+pub async fn schemas(
+    State(s): State<AppState>,
+    Path(id): Path<String>,
+    Query(c): Query<CatalogQuery>,
+    Query(q): Query<FilterQuery>,
+) -> ApiResult<Json<Vec<SchemaInfo>>> {
+    let p = s.service.profile(&id)?.in_catalog(c.catalog.as_deref());
     Ok(Json(s.service.metadata.schemas(&p, q.filter.as_deref()).await?))
 }
 
@@ -257,9 +274,10 @@ pub struct ObjectsQuery {
 pub async fn objects(
     State(s): State<AppState>,
     Path((id, schema)): Path<(String, String)>,
+    Query(c): Query<CatalogQuery>,
     Query(q): Query<ObjectsQuery>,
 ) -> ApiResult<Json<Vec<ObjectInfo>>> {
-    let p = s.service.profile(&id)?;
+    let p = s.service.profile(&id)?.in_catalog(c.catalog.as_deref());
     let kind = q.kind.unwrap_or(MetaKey::Tables);
     Ok(Json(s.service.metadata.objects(&p, &schema, kind, q.filter.as_deref()).await?))
 }
@@ -273,9 +291,10 @@ pub struct TablesQuery {
 pub async fn tables(
     State(s): State<AppState>,
     Path((id, schema)): Path<(String, String)>,
+    Query(c): Query<CatalogQuery>,
     Query(q): Query<TablesQuery>,
 ) -> ApiResult<Json<Vec<Table>>> {
-    let p = s.service.profile(&id)?;
+    let p = s.service.profile(&id)?.in_catalog(c.catalog.as_deref());
     Ok(Json(s.service.metadata.tables(&p, &schema, q.table.as_deref()).await?))
 }
 
@@ -288,9 +307,10 @@ pub struct ViewsQuery {
 pub async fn views(
     State(s): State<AppState>,
     Path((id, schema)): Path<(String, String)>,
+    Query(c): Query<CatalogQuery>,
     Query(q): Query<ViewsQuery>,
 ) -> ApiResult<Json<Vec<View>>> {
-    let p = s.service.profile(&id)?;
+    let p = s.service.profile(&id)?.in_catalog(c.catalog.as_deref());
     let kind = q.kind.unwrap_or(MetaKey::Views);
     if !matches!(kind, MetaKey::Views | MetaKey::MaterializedViews) {
         return Err(Error::invalid(msg!("api.bad_view_kind")).into());
@@ -301,24 +321,27 @@ pub async fn views(
 pub async fn sequences(
     State(s): State<AppState>,
     Path((id, schema)): Path<(String, String)>,
+    Query(c): Query<CatalogQuery>,
 ) -> ApiResult<Json<Vec<Sequence>>> {
-    let p = s.service.profile(&id)?;
+    let p = s.service.profile(&id)?.in_catalog(c.catalog.as_deref());
     Ok(Json(s.service.metadata.sequences(&p, &schema).await?))
 }
 
 pub async fn routines(
     State(s): State<AppState>,
     Path((id, schema)): Path<(String, String)>,
+    Query(c): Query<CatalogQuery>,
 ) -> ApiResult<Json<Vec<Routine>>> {
-    let p = s.service.profile(&id)?;
+    let p = s.service.profile(&id)?.in_catalog(c.catalog.as_deref());
     Ok(Json(s.service.metadata.routines(&p, &schema).await?))
 }
 
 pub async fn synonyms(
     State(s): State<AppState>,
     Path((id, schema)): Path<(String, String)>,
+    Query(c): Query<CatalogQuery>,
 ) -> ApiResult<Json<Vec<Synonym>>> {
-    let p = s.service.profile(&id)?;
+    let p = s.service.profile(&id)?.in_catalog(c.catalog.as_deref());
     Ok(Json(s.service.metadata.synonyms(&p, &schema).await?))
 }
 
@@ -334,9 +357,10 @@ pub struct SourceQuery {
 pub async fn source(
     State(s): State<AppState>,
     Path((id, schema)): Path<(String, String)>,
+    Query(c): Query<CatalogQuery>,
     Query(q): Query<SourceQuery>,
 ) -> ApiResult<Json<Option<String>>> {
-    let p = s.service.profile(&id)?;
+    let p = s.service.profile(&id)?.in_catalog(c.catalog.as_deref());
     Ok(Json(s.service.metadata.source(&p, &schema, q.kind, &q.name, q.routine_kind.as_deref()).await?))
 }
 
@@ -359,9 +383,10 @@ pub struct DdlQuery {
 pub async fn ddl(
     State(s): State<AppState>,
     Path((id, schema)): Path<(String, String)>,
+    Query(c): Query<CatalogQuery>,
     Query(q): Query<DdlQuery>,
 ) -> ApiResult<Json<Value>> {
-    let p = s.service.profile(&id)?;
+    let p = s.service.profile(&id)?.in_catalog(c.catalog.as_deref());
     let ddl = s.service.metadata.ddl(&p, &schema, q.kind, &q.name).await?;
     Ok(Json(json!({ "ddl": ddl })))
 }
@@ -382,9 +407,10 @@ pub struct GenerateQuery {
 pub async fn generate(
     State(s): State<AppState>,
     Path((id, schema)): Path<(String, String)>,
+    Query(c): Query<CatalogQuery>,
     Query(q): Query<GenerateQuery>,
 ) -> ApiResult<Json<Value>> {
-    let p = s.service.profile(&id)?;
+    let p = s.service.profile(&id)?.in_catalog(c.catalog.as_deref());
     let reference = q.ref_table.as_deref().map(|t| (q.ref_schema.as_deref().unwrap_or(&schema), t));
     let sql = s.service.metadata.generate(&p, &schema, &q.object, q.object_kind, q.kind, reference).await?;
     Ok(Json(json!({ "sql": sql })))
@@ -398,9 +424,10 @@ pub struct RelationsQuery {
 pub async fn relations(
     State(s): State<AppState>,
     Path((id, schema)): Path<(String, String)>,
+    Query(c): Query<CatalogQuery>,
     Query(q): Query<RelationsQuery>,
 ) -> ApiResult<Json<Vec<Relation>>> {
-    let p = s.service.profile(&id)?;
+    let p = s.service.profile(&id)?.in_catalog(c.catalog.as_deref());
     Ok(Json(s.service.metadata.relations(&p, &schema, &q.table).await?))
 }
 
@@ -418,9 +445,10 @@ pub struct DataSqlBody {
 pub async fn data_sql(
     State(s): State<AppState>,
     Path((id, schema)): Path<(String, String)>,
+    Query(c): Query<CatalogQuery>,
     Json(b): Json<DataSqlBody>,
 ) -> ApiResult<Json<Value>> {
-    let p = s.service.profile(&id)?;
+    let p = s.service.profile(&id)?.in_catalog(c.catalog.as_deref());
     let filter = b.filter.as_deref().map(str::trim).filter(|f| !f.is_empty());
     let sql = s.service.metadata.data_sql(&p, &schema, &b.object, filter, b.order.as_ref(), b.count).await?;
     Ok(Json(json!({ "sql": sql })))
```

- [ ] **Step 12: 修改 `crates/dblens-core/locales/app.json`**

```diff
--- a/crates/dblens-core/locales/app.json
+++ b/crates/dblens-core/locales/app.json
@@ -298,7 +298,10 @@
   "session.disconnected": {"zh-CN": "连接已断开", "en": "The connection is closed"},
   "session.explain_single": {"zh-CN": "执行计划只能针对一条语句", "en": "An execution plan can only be shown for a single statement"},
   "session.task_gone": {"zh-CN": "会话任务已退出", "en": "The session task has exited"},
+  "sqlserver.bad_auth": {"zh-CN": "不认识的认证方式：%{value}", "en": "Unknown authentication method: %{value}"},
+  "sqlserver.connection_lost": {"zh-CN": "与 SQL Server 的连接已断开", "en": "The connection to SQL Server was lost"},
   "sqlserver.later": {"zh-CN": "SQL Server 的这项功能将在后续版本支持", "en": "This feature is not yet available for SQL Server"},
+  "sqlserver.windows_auth_only": {"zh-CN": "Windows 认证只能在 Windows 上使用", "en": "Windows authentication is only available on Windows"},
   "sqlfile.denied": {"zh-CN": "第 %{line} 行的语句被执行策略拒绝，没有执行任何语句：%{reason}\n%{text}", "en": "The statement at line %{line} was denied by the execution policy; nothing was executed: %{reason}\n%{text}"},
   "sqlfile.history": {"zh-CN": "执行 SQL 文件 %{file}：%{done} 条，失败 %{failed} 条", "en": "Run SQL file %{file}: %{done} statements, %{failed} failed"},
   "sqlfile.log_failed": {"zh-CN": "第 %{n} 条语句失败（第 %{line} 行）：", "en": "Statement %{n} failed (line %{line}):"},
```

- [ ] **Step 13: 修改 `crates/dblens-dialect/Cargo.toml`**

```diff
--- a/crates/dblens-dialect/Cargo.toml
+++ b/crates/dblens-dialect/Cargo.toml
@@ -17,6 +17,7 @@ dblens-driver-dameng.workspace = true
 dblens-driver-mysql.workspace = true
 dblens-driver-oracle.workspace = true
 dblens-driver-postgres.workspace = true
+dblens-driver-sqlserver.workspace = true
 insta.workspace = true
 serde_json.workspace = true
 tokio.workspace = true
```

- [ ] **Step 14: 修改 `crates/dblens-dialect/src/variant.rs`**

```diff
--- a/crates/dblens-dialect/src/variant.rs
+++ b/crates/dblens-dialect/src/variant.rs
@@ -124,6 +124,7 @@ impl Variant {
             DriverId::Postgres => Variant::Postgres,
             DriverId::Oracle => Variant::Oracle,
             DriverId::Dameng => Variant::Dameng,
+            DriverId::SqlServer => Variant::SqlServer,
         }
     }
 
```

- [ ] **Step 15: 新建 `crates/dblens-driver-sqlserver/Cargo.toml`**

```toml
[package]
name = "dblens-driver-sqlserver"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
publish.workspace = true

[dependencies]
async-trait.workspace = true
chrono.workspace = true
dblens-core.workspace = true
dblens-driver.workspace = true
futures.workspace = true
tiberius.workspace = true
tokio = { workspace = true, features = ["net"] }
tokio-util.workspace = true
uuid.workspace = true

[dev-dependencies]
dblens-driver = { workspace = true, features = ["conformance"] }
tokio.workspace = true
```

- [ ] **Step 16: 新建 `crates/dblens-driver-sqlserver/src/actor.rs`**

```rust
//! 持有 tiberius 连接的后台任务（见 lib.rs 开头的说明）。
//!
//! 一批的结果按顺序变成 `Item`：结果集（列 → 行 → 结束）、影响行数、服务器消息。结果集之后的 DONE 带的是返回的行数，
//! 不当作影响行数。读取方丢弃了执行流（不再读）时，向服务器发取消并丢掉剩下的结果，连接继续可用。

use std::pin::Pin;
use std::sync::Arc;

use dblens_core::{Error, Result, Row, Value};
use dblens_driver::ColumnMeta;
use futures::TryStreamExt;
use tiberius::{ColumnType, QueryItem, ToSql};
use tokio::sync::futures::Notified;
use tokio::sync::{Notify, mpsc, oneshot};

use crate::TdsClient;
use crate::error::{map_err, server_line, with_line};
use crate::types::{Param, cell_value, type_name};

pub(crate) enum Item {
    Start(Vec<ColumnMeta>),
    Row(Row),
    End,
    Affected(u64),
    Message(String),
}

pub(crate) type Reply<T> = oneshot::Sender<Result<T>>;

pub(crate) enum Cmd {
    Exec {
        sql: String,
        params: Vec<Value>,
        lob_limit: usize,
        out: mpsc::Sender<Result<Item>>,
    },
    /// 执行一批不返回结果的语句（事务控制、ping）。
    Batch(&'static str, Reply<()>),
    Version(Reply<String>),
    Close(Reply<()>),
}

/// 一批执行到哪里停下。
enum Flow {
    Done,
    /// 用户取消：通知服务器，并向读取方报 `Cancelled`。
    Cancelled,
    /// 读取方丢弃了执行流：通知服务器停下。
    Abandoned,
    Failed(Error),
}

pub(crate) async fn run(mut client: TdsClient, mut rx: mpsc::Receiver<Cmd>, cancel: Arc<Notify>) {
    while let Some(cmd) = rx.recv().await {
        match cmd {
            Cmd::Exec { sql, params, lob_limit, out } => {
                let cancelled = cancel.notified();
                tokio::pin!(cancelled);
                // 先登记，执行开始后的取消都能收到；没有在执行时的取消（notify_waiters）不会留到下一次
                cancelled.as_mut().enable();
                match stream(&mut client, &sql, &params, lob_limit, &out, cancelled).await {
                    Flow::Done => {}
                    Flow::Cancelled => {
                        let _ = client.cancel_query().await;
                        let _ = out.send(Err(Error::Cancelled)).await;
                    }
                    Flow::Abandoned => {
                        let _ = client.cancel_query().await;
                    }
                    Flow::Failed(e) => {
                        let _ = out.send(Err(e)).await;
                    }
                }
            }
            Cmd::Batch(sql, reply) => {
                let _ = reply.send(batch(&mut client, sql).await);
            }
            Cmd::Version(reply) => {
                let _ = reply.send(version(&mut client).await);
            }
            Cmd::Close(reply) => {
                let _ = reply.send(client.close().await.map_err(map_err));
                return;
            }
        }
    }
    let _ = client.close().await;
}

fn sql_error(e: tiberius::error::Error, sql: &str) -> Error {
    let line = server_line(&e);
    with_line(map_err(e), sql, line)
}

/// 执行一批并读完结果（只用于不返回结果集的内部语句）。
async fn batch(client: &mut TdsClient, sql: &str) -> Result<()> {
    let stream = client.simple_query(sql).await.map_err(map_err)?;
    stream.into_results().await.map(|_| ()).map_err(map_err)
}

async fn version(client: &mut TdsClient) -> Result<String> {
    let row =
        client.simple_query("SELECT @@VERSION").await.map_err(map_err)?.into_row().await.map_err(map_err)?;
    let text: Option<&str> = row.as_ref().and_then(|r| r.get(0));
    // @@VERSION 有多行（版权、版本、操作系统），第一行是产品和版本号
    Ok(text.unwrap_or_default().lines().next().unwrap_or_default().trim().to_string())
}

/// 把一项交给读取方；等待期间收到取消或读取方已不在时返回对应的停止原因。
async fn send(
    out: &mpsc::Sender<Result<Item>>,
    item: Item,
    cancelled: Pin<&mut Notified<'_>>,
) -> Option<Flow> {
    tokio::select! {
        biased;
        _ = cancelled => Some(Flow::Cancelled),
        r = out.send(Ok(item)) => r.is_err().then_some(Flow::Abandoned),
    }
}

async fn stream(
    client: &mut TdsClient,
    sql: &str,
    params: &[Value],
    lob_limit: usize,
    out: &mpsc::Sender<Result<Item>>,
    mut cancelled: Pin<&mut Notified<'_>>,
) -> Flow {
    let params = match params.iter().map(Param::new).collect::<Result<Vec<_>>>() {
        Ok(p) => p,
        Err(e) => return Flow::Failed(e),
    };
    let refs: Vec<&dyn ToSql> = params.iter().map(|p| p as &dyn ToSql).collect();
    // 没有参数时按批发送：USE、SET、临时表等留在会话中；有参数时经 sp_executesql
    let started = async {
        if refs.is_empty() { client.simple_query(sql).await } else { client.query(sql, &refs).await }
    };
    let mut rows = tokio::select! {
        biased;
        _ = cancelled.as_mut() => return Flow::Cancelled,
        r = started => match r {
            Ok(s) => s,
            Err(e) => return Flow::Failed(sql_error(e, sql)),
        },
    };
    let mut types: Vec<ColumnType> = Vec::new();
    let mut in_set = false;
    loop {
        let next = tokio::select! {
            biased;
            _ = cancelled.as_mut() => return Flow::Cancelled,
            _ = out.closed() => return Flow::Abandoned,
            r = rows.try_next() => r,
        };
        let item = match next {
            Err(e) => {
                if in_set && let Some(f) = send(out, Item::End, cancelled.as_mut()).await {
                    return f;
                }
                return Flow::Failed(sql_error(e, sql));
            }
            Ok(None) => {
                if in_set && let Some(f) = send(out, Item::End, cancelled.as_mut()).await {
                    return f;
                }
                return Flow::Done;
            }
            Ok(Some(QueryItem::Metadata(m))) => {
                if in_set && let Some(f) = send(out, Item::End, cancelled.as_mut()).await {
                    return f;
                }
                in_set = true;
                types = m.columns().iter().map(|c| c.column_type()).collect();
                Item::Start(
                    m.columns()
                        .iter()
                        .map(|c| ColumnMeta::new(c.name(), type_name(c.column_type())))
                        .collect(),
                )
            }
            Ok(Some(QueryItem::Row(r))) => {
                let row: Result<Row> =
                    r.into_iter().zip(&types).map(|(data, ty)| cell_value(&data, *ty, lob_limit)).collect();
                match row {
                    Ok(row) => Item::Row(row),
                    Err(e) => return Flow::Failed(e),
                }
            }
            // 结果集之后的行数是返回的行数，结果集到此结束
            Ok(Some(QueryItem::Done(_))) if in_set => {
                in_set = false;
                Item::End
            }
            Ok(Some(QueryItem::Done(n))) => Item::Affected(n),
            Ok(Some(QueryItem::Info(info))) => Item::Message(info.message),
        };
        if let Some(f) = send(out, item, cancelled.as_mut()).await {
            return f;
        }
    }
}
```

- [ ] **Step 17: 新建 `crates/dblens-driver-sqlserver/src/conn.rs`**

```rust
use std::sync::Arc;

use async_trait::async_trait;
use dblens_core::{DEFAULT_LOB_PREVIEW_BYTES, Error, Result, Value, msg};
use dblens_driver::{CancelHandle, Connection, ExecStream, ServerInfo};
use tokio::sync::{Notify, mpsc, oneshot};
use tokio::task::JoinHandle;

use crate::TdsClient;
use crate::actor::{self, Cmd, Reply};
use crate::exec::MsExec;

/// 一批结果在通道中最多缓存的项数（行、消息等）；读取方不取时后台任务就等待。
const BUFFERED_ITEMS: usize = 256;

pub struct SqlServerConnection {
    cmds: mpsc::Sender<Cmd>,
    cancel: Arc<Notify>,
    task: JoinHandle<()>,
    autocommit: bool,
    in_tx: bool,
    lob_limit: usize,
}

fn gone() -> Error {
    Error::connect(msg!("sqlserver.connection_lost"))
}

impl SqlServerConnection {
    pub(crate) fn spawn(client: TdsClient) -> Self {
        let (cmds, rx) = mpsc::channel(1);
        let cancel = Arc::new(Notify::new());
        let task = tokio::spawn(actor::run(client, rx, cancel.clone()));
        Self { cmds, cancel, task, autocommit: true, in_tx: false, lob_limit: DEFAULT_LOB_PREVIEW_BYTES }
    }

    async fn request<T>(&self, make: impl FnOnce(Reply<T>) -> Cmd) -> Result<T> {
        let (tx, rx) = oneshot::channel();
        self.cmds.send(make(tx)).await.map_err(|_| gone())?;
        rx.await.map_err(|_| gone())?
    }

    async fn batch(&self, sql: &'static str) -> Result<()> {
        self.request(|r| Cmd::Batch(sql, r)).await
    }

    /// 手动提交模式下，在第一批之前开启事务。
    async fn ensure_tx(&mut self) -> Result<()> {
        if !self.autocommit && !self.in_tx {
            self.batch("BEGIN TRANSACTION").await?;
            self.in_tx = true;
        }
        Ok(())
    }

    /// 结束事务。批里可能已经自己提交或回滚了，所以先看 `@@TRANCOUNT`。
    async fn end_tx(&mut self, sql: &'static str) -> Result<()> {
        if self.in_tx {
            self.batch(sql).await?;
            self.in_tx = false;
        }
        Ok(())
    }
}

#[async_trait]
impl Connection for SqlServerConnection {
    async fn execute<'a>(
        &'a mut self,
        sql: &'a str,
        params: &'a [Value],
    ) -> Result<Box<dyn ExecStream + 'a>> {
        self.ensure_tx().await?;
        let (out, rx) = mpsc::channel(BUFFERED_ITEMS);
        let cmd = Cmd::Exec { sql: sql.to_string(), params: params.to_vec(), lob_limit: self.lob_limit, out };
        self.cmds.send(cmd).await.map_err(|_| gone())?;
        Ok(Box::new(MsExec::new(rx)))
    }

    fn cancel_handle(&self) -> CancelHandle {
        let cancel = self.cancel.clone();
        CancelHandle::new(move || {
            // 只通知正在执行的那一批；没有在执行时什么也不做
            cancel.notify_waiters();
            async { Ok(()) }
        })
    }

    fn set_lob_limit(&mut self, bytes: usize) {
        self.lob_limit = bytes;
    }

    async fn set_autocommit(&mut self, on: bool) -> Result<()> {
        if on {
            self.end_tx("IF @@TRANCOUNT > 0 COMMIT TRANSACTION").await?;
        }
        self.autocommit = on;
        Ok(())
    }

    async fn commit(&mut self) -> Result<()> {
        self.end_tx("IF @@TRANCOUNT > 0 COMMIT TRANSACTION").await
    }

    async fn rollback(&mut self) -> Result<()> {
        self.end_tx("IF @@TRANCOUNT > 0 ROLLBACK TRANSACTION").await
    }

    async fn ping(&mut self) -> Result<()> {
        self.batch("SELECT 1").await
    }

    async fn server_info(&mut self) -> Result<ServerInfo> {
        let version = self.request(Cmd::Version).await?;
        Ok(ServerInfo { product: "SQL Server".into(), version })
    }

    async fn close(self: Box<Self>) -> Result<()> {
        let this = *self;
        let (tx, rx) = oneshot::channel();
        let sent = this.cmds.send(Cmd::Close(tx)).await.is_ok();
        let result = if sent { rx.await.unwrap_or(Ok(())) } else { Ok(()) };
        let _ = this.task.await;
        result
    }
}
```

- [ ] **Step 18: 新建 `crates/dblens-driver-sqlserver/src/error.rs`**

```rust
use dblens_core::{Error, Msg, char_position};
use tiberius::error::Error as TdsError;

/// 登录失败（密码错误、账号不存在或被禁用、无权访问默认库时也报这个）。
const LOGIN_FAILED: u32 = 18456;
/// 无法打开登录所请求的数据库。
const CANNOT_OPEN_DATABASE: u32 = 4060;

pub(crate) fn map_err(e: TdsError) -> Error {
    match e {
        TdsError::Server(ref t) => match t.code() {
            LOGIN_FAILED | CANNOT_OPEN_DATABASE => Error::Auth { message: Msg::raw(t.message()) },
            code => Error::Sql { code: code.to_string(), message: t.message().to_string(), position: None },
        },
        TdsError::Io { .. } | TdsError::Tls(_) | TdsError::Routing { .. } => Error::connect(e.to_string()),
        other => Error::internal(other.to_string()),
    }
}

/// 服务器报出的批内行号（从 1 开始），没有时为 None。
pub(crate) fn server_line(e: &TdsError) -> Option<u32> {
    match e {
        TdsError::Server(t) if t.line() > 0 => Some(t.line()),
        _ => None,
    }
}

/// 语法错误（102 `Incorrect syntax near 'x'.`、156 `… near the keyword 'FROM'.`）中出错处的原文。
/// 消息的语言随登录名的默认语言，所以按错误号判断，取引号里的部分。
fn near<'a>(code: &str, message: &'a str) -> Option<&'a str> {
    if !matches!(code, "102" | "156") {
        return None;
    }
    let (_, rest) = message.split_once('\'')?;
    rest.rsplit_once('\'').map(|(token, _)| token).filter(|t| !t.is_empty())
}

/// 把批内行号换算成在批文本中的位置（字符偏移，从 1 开始）：语法错误能在该行找到出错处的原文时指向它，
/// 否则指向该行第一个非空白字符。
pub(crate) fn with_line(e: Error, sql: &str, line: Option<u32>) -> Error {
    match (e, line) {
        (Error::Sql { code, message, position: None }, Some(line)) => {
            let start: usize = sql.split_inclusive('\n').take(line as usize - 1).map(str::len).sum();
            let start = start.min(sql.len());
            let text = sql[start..].split('\n').next().unwrap_or_default();
            let indent = text.len() - text.trim_start_matches([' ', '\t']).len();
            let at = near(&code, &message)
                .and_then(|token| text.to_ascii_lowercase().find(&token.to_ascii_lowercase()))
                .unwrap_or(indent);
            Error::Sql { code, message, position: Some(char_position(sql, start + at)) }
        }
        (e, _) => e,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sql_error(message: &str) -> Error {
        Error::Sql { code: "102".into(), message: message.into(), position: None }
    }

    #[test]
    fn line_becomes_position() {
        let sql = "SELECT '数洞'\n  SELECT * FROM nope";
        // 第 2 行的 SELECT 前面有 11 个字符、一个换行、两个空格，位置从 1 开始
        let e = with_line(sql_error("Invalid object name 'nope2'."), sql, Some(2));
        assert!(matches!(e, Error::Sql { position: Some(15), .. }), "{e:?}");
        assert!(matches!(with_line(sql_error("x"), sql, None), Error::Sql { position: None, .. }));
        assert!(matches!(with_line(sql_error("x"), sql, Some(9)), Error::Sql { position: Some(_), .. }));
    }

    #[test]
    fn syntax_error_points_at_the_token() {
        let sql = "SELECT '数洞', FROM t";
        let e = with_line(sql_error("Incorrect syntax near the keyword 'FROM'."), sql, Some(1));
        let Error::Sql { position: Some(p), .. } = e else { panic!("{e:?}") };
        assert_eq!(p as usize, sql[..sql.find("FROM").unwrap()].chars().count() + 1);
        assert_eq!(near("102", "Incorrect syntax near ')'."), Some(")"));
        assert_eq!(near("208", "Invalid object name 'x'."), None);
    }

    #[test]
    fn syntax_error_in_other_languages() {
        // 服务器的消息语言随登录名的默认语言（中文 Windows 上装的实例默认是简体中文）
        let sql = "SELECT '数洞', FROM t";
        let e = with_line(
            Error::Sql {
                code: "156".into(),
                message: "关键字 'FROM' 附近有语法错误。".into(),
                position: None,
            },
            sql,
            Some(1),
        );
        let Error::Sql { position: Some(p), .. } = e else { panic!("{e:?}") };
        assert_eq!(p as usize, sql[..sql.find("FROM").unwrap()].chars().count() + 1);
        assert_eq!(near("102", "')' 附近有语法错误。"), Some(")"));
    }
}
```

- [ ] **Step 19: 新建 `crates/dblens-driver-sqlserver/src/exec.rs`**

```rust
use async_trait::async_trait;
use dblens_core::Result;
use dblens_driver::{ExecEvent, ExecStream};
use tokio::sync::mpsc;

use crate::actor::Item;

/// 执行流：从后台任务的通道读取。连续的行合并成一个 `Rows` 事件：凑满 `max_rows` 行，或遇到结果集结束等
/// 其他事件为止（分批取数据的调用方以不满一批判断结果读完了）。丢弃时通道关闭，后台任务据此通知服务器停下。
pub(crate) struct MsExec {
    rx: mpsc::Receiver<Result<Item>>,
    /// 合并行时多读到的下一项。
    pending: Option<Result<Item>>,
}

impl MsExec {
    pub(crate) fn new(rx: mpsc::Receiver<Result<Item>>) -> Self {
        Self { rx, pending: None }
    }
}

#[async_trait]
impl ExecStream for MsExec {
    async fn next(&mut self, max_rows: usize) -> Result<Option<ExecEvent>> {
        let item = match self.pending.take() {
            Some(item) => item,
            None => match self.rx.recv().await {
                Some(item) => item,
                None => return Ok(None),
            },
        };
        Ok(Some(match item? {
            Item::Start(columns) => ExecEvent::ResultStart { columns },
            Item::End => ExecEvent::ResultEnd,
            Item::Affected(n) => ExecEvent::Affected(n),
            Item::Message(m) => ExecEvent::Message(m),
            Item::Row(first) => {
                let mut rows = vec![first];
                while rows.len() < max_rows.max(1) {
                    match self.rx.recv().await {
                        Some(Ok(Item::Row(r))) => rows.push(r),
                        Some(other) => {
                            self.pending = Some(other);
                            break;
                        }
                        None => break,
                    }
                }
                ExecEvent::Rows(rows)
            }
        }))
    }
}
```

- [ ] **Step 20: 新建 `crates/dblens-driver-sqlserver/src/lib.rs`**

```rust
//! SQL Server 驱动（tiberius，TDS 协议；M6a 设计文档第 3 节）。
//!
//! tiberius 的取消（`Client::cancel_query`）要独占 `Client`，而结果流在读取期间一直借用它，所以连接由一个后台任务
//! （`actor`）持有：执行、事务、取消都通过消息交给它，结果经有界通道流回，读取方不取时后台任务就等待。
//!
//! 连接选项：
//! - `auth`：`sql`（默认，SQL 账号）、`windows`（指定 Windows 账号，用户名写 `域或机器名\用户`，NTLM）、
//!   `integrated`（当前 Windows 用户）。两种 Windows 认证都只在 Windows 上可用（其他平台的 NTLM 实现 sspi
//!   依赖预发布版本的库，没有采用）；
//! - `instance`：命名实例（如 `SQLEXPRESS`），经 SQL Browser（UDP 1434）查端口，此时不用 `port`；
//! - `tls`：`disable`（只加密登录）、`require`（加密，不校验证书）、`verify`（校验证书，`tls_ca` 可指定 CA）；
//! - `trust_server_certificate`：`true` 时一律不校验证书（同 SSMS 的同名选项）。

mod actor;
mod conn;
mod error;
mod exec;
mod types;

use async_trait::async_trait;
use dblens_core::{Error, Result, msg};
use dblens_driver::{Capabilities, ConnectParams, Connection, Driver, DriverId, TlsMode};
use tiberius::{AuthMethod, Client, Config, EncryptionLevel, SqlBrowser};
use tokio::net::TcpStream;
use tokio_util::compat::{Compat, TokioAsyncWriteCompatExt};

pub use conn::SqlServerConnection;

use crate::error::map_err;

pub(crate) type TdsClient = Client<Compat<TcpStream>>;

pub struct SqlServerDriver;

#[async_trait]
impl Driver for SqlServerDriver {
    fn id(&self) -> DriverId {
        DriverId::SqlServer
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            catalogs: true,
            schemas: true,
            transactional_ddl: true,
            cancel: true,
            explain: true,
            multiple_result_sets: true,
        }
    }

    async fn connect(&self, p: &ConnectParams) -> Result<Box<dyn Connection>> {
        let cfg = config(p)?;
        let named = option(p, "instance").is_some();
        let client = match tokio::time::timeout(p.connect_timeout, open(cfg.clone(), named)).await {
            Err(_) => return Err(Error::Timeout),
            Ok(Err(tiberius::error::Error::Routing { host, port })) => {
                // Azure 等会把连接重定向到另一个地址：按返回的地址重连一次
                let mut cfg = cfg;
                cfg.host(host);
                cfg.port(port);
                tokio::time::timeout(p.connect_timeout, open(cfg, false))
                    .await
                    .map_err(|_| Error::Timeout)?
                    .map_err(map_err)?
            }
            Ok(r) => r.map_err(map_err)?,
        };
        Ok(Box::new(SqlServerConnection::spawn(client)))
    }
}

/// 建立 TCP 连接并登录。`named` 时先经 SQL Browser 查出命名实例的端口。
async fn open(cfg: Config, named: bool) -> tiberius::Result<TdsClient> {
    let tcp = if named {
        TcpStream::connect_named(&cfg).await?
    } else {
        TcpStream::connect(cfg.get_addr()).await?
    };
    tcp.set_nodelay(true)?;
    Client::connect(cfg, tcp.compat_write()).await
}

fn option<'a>(p: &'a ConnectParams, key: &str) -> Option<&'a str> {
    p.options.get(key).map(|v| v.trim()).filter(|v| !v.is_empty())
}

fn config(p: &ConnectParams) -> Result<Config> {
    let mut cfg = Config::new();
    cfg.host(&p.host);
    cfg.application_name("DBLens");
    // 命名实例的端口由 SQL Browser 给出，不设端口（设了会直接连这个端口）
    match option(p, "instance") {
        Some(instance) => cfg.instance_name(instance),
        None => cfg.port(p.port),
    }
    if let Some(db) = p.database.as_deref().filter(|d| !d.is_empty()) {
        cfg.database(db);
    }
    cfg.authentication(match option(p, "auth").unwrap_or("sql") {
        "sql" => AuthMethod::sql_server(&p.user, &p.password),
        #[cfg(windows)]
        "windows" => AuthMethod::windows(&p.user, &p.password),
        #[cfg(windows)]
        "integrated" => AuthMethod::Integrated,
        #[cfg(not(windows))]
        "windows" | "integrated" => return Err(Error::invalid(msg!("sqlserver.windows_auth_only"))),
        other => return Err(Error::invalid(msg!("sqlserver.bad_auth", value = other))),
    });
    let trust = option(p, "trust_server_certificate").is_some_and(|v| v.eq_ignore_ascii_case("true"));
    match p.tls_mode()? {
        // 登录报文总要加密；服务器强制加密时由服务器决定是否加密其余流量
        TlsMode::Disable => {
            cfg.encryption(EncryptionLevel::Off);
            cfg.trust_cert();
        }
        TlsMode::Require => {
            cfg.encryption(EncryptionLevel::Required);
            cfg.trust_cert();
        }
        TlsMode::Verify { ca_file } => {
            cfg.encryption(EncryptionLevel::Required);
            if trust {
                cfg.trust_cert();
            } else if let Some(ca) = ca_file {
                cfg.trust_cert_ca(ca.to_string_lossy());
            }
        }
    }
    Ok(cfg)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    fn params(options: &[(&str, &str)]) -> ConnectParams {
        ConnectParams {
            host: "db.example".into(),
            port: 1433,
            user: "sa".into(),
            password: "p".into(),
            database: Some("app".into()),
            options: options.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect::<BTreeMap<_, _>>(),
            connect_timeout: ConnectParams::DEFAULT_CONNECT_TIMEOUT,
        }
    }

    #[test]
    fn config_from_options() {
        let cfg = config(&params(&[])).unwrap();
        assert_eq!(cfg.get_addr(), "db.example:1433");
        // 命名实例先连 SQL Browser 的 1434 端口
        let cfg = config(&params(&[("instance", "SQLEXPRESS")])).unwrap();
        assert_eq!(cfg.get_addr(), "db.example:1434");
        assert!(config(&params(&[("auth", "kerberos")])).is_err());
        assert!(config(&params(&[("tls", "bogus")])).is_err());
        assert!(config(&params(&[("tls", "verify"), ("trust_server_certificate", "true")])).is_ok());
        for auth in ["windows", "integrated"] {
            assert_eq!(config(&params(&[("auth", auth)])).is_ok(), cfg!(windows), "{auth}");
        }
    }
}
```

- [ ] **Step 21: 新建 `crates/dblens-driver-sqlserver/src/types.rs`**

```rust
//! TDS 值与 DBLens 值的互相转换（M6a 设计文档 3.5 节）。

use chrono::{DateTime, FixedOffset, NaiveDate, NaiveDateTime, NaiveTime};
use dblens_core::{Error, Result, Value, msg};
use tiberius::numeric::Numeric;
use tiberius::{ColumnData, ColumnType, FromSql, ToSql};

/// 结果列的类型名（TDS 只给出线上的类型，不含长度，也分不出 varchar 与 varchar(max) 等）。
pub(crate) fn type_name(t: ColumnType) -> &'static str {
    use ColumnType as T;
    match t {
        T::Null => "null",
        T::Bit | T::Bitn => "bit",
        T::Int1 => "tinyint",
        T::Int2 => "smallint",
        T::Int4 | T::Intn => "int",
        T::Int8 => "bigint",
        T::Float4 => "real",
        T::Float8 | T::Floatn => "float",
        T::Money => "money",
        T::Money4 => "smallmoney",
        T::Datetime4 => "smalldatetime",
        T::Datetime | T::Datetimen => "datetime",
        T::Daten => "date",
        T::Timen => "time",
        T::Datetime2 => "datetime2",
        T::DatetimeOffsetn => "datetimeoffset",
        T::Guid => "uniqueidentifier",
        T::Decimaln | T::Numericn => "decimal",
        T::BigVarBin => "varbinary",
        T::BigBinary => "binary",
        T::BigVarChar => "varchar",
        T::BigChar => "char",
        T::NVarchar => "nvarchar",
        T::NChar => "nchar",
        T::Xml => "xml",
        T::Udt => "udt",
        T::Text => "text",
        T::Image => "image",
        T::NText => "ntext",
        T::SSVariant => "sql_variant",
    }
}

/// 精确小数的文本：整数部分和补足小数位的小数部分（tiberius 自己的格式在小数位为 0 时会多出 `.0`）。
pub(crate) fn numeric_text(n: Numeric) -> String {
    let scale = usize::from(n.scale());
    let digits = n.value().unsigned_abs().to_string();
    let sign = if n.value() < 0 { "-" } else { "" };
    if scale == 0 {
        return format!("{sign}{digits}");
    }
    let padded = format!("{digits:0>width$}", width = scale + 1);
    let (int, frac) = padded.split_at(padded.len() - scale);
    format!("{sign}{int}.{frac}")
}

fn chrono_value<'a, T: FromSql<'a>>(data: &'a ColumnData<'static>, wrap: fn(T) -> Value) -> Result<Value> {
    T::from_sql(data).map(|v| v.map_or(Value::Null, wrap)).map_err(|e| Error::internal(e.to_string()))
}

/// 一个单元格的值。`ty` 用来区分 money（线上是浮点）；大字段按 `lob_limit` 截断为预览。
pub(crate) fn cell_value(data: &ColumnData<'static>, ty: ColumnType, lob_limit: usize) -> Result<Value> {
    use ColumnData as D;
    Ok(match data {
        D::U8(v) => v.map_or(Value::Null, |x| Value::Int(i64::from(x))),
        D::I16(v) => v.map_or(Value::Null, |x| Value::Int(i64::from(x))),
        D::I32(v) => v.map_or(Value::Null, |x| Value::Int(i64::from(x))),
        D::I64(v) => v.map_or(Value::Null, Value::Int),
        D::F32(v) => v.map_or(Value::Null, |x| Value::Float(f64::from(x))),
        // money / smallmoney 在线上是按 4 位小数换算的浮点，按精确小数显示
        D::F64(v) if matches!(ty, ColumnType::Money | ColumnType::Money4) => {
            v.map_or(Value::Null, |x| Value::Decimal(format!("{x:.4}")))
        }
        D::F64(v) => v.map_or(Value::Null, Value::Float),
        D::Bit(v) => v.map_or(Value::Null, Value::Bool),
        D::String(v) => v.as_ref().map_or(Value::Null, |s| Value::String(s.to_string()).cap_large(lob_limit)),
        D::Guid(v) => v.map_or(Value::Null, Value::Uuid),
        D::Binary(v) => v.as_ref().map_or(Value::Null, |b| Value::Bytes(b.to_vec()).cap_large(lob_limit)),
        D::Numeric(v) => v.map_or(Value::Null, |n| Value::Decimal(numeric_text(n))),
        D::Xml(v) => v.as_ref().map_or(Value::Null, |x| Value::String(x.to_string()).cap_large(lob_limit)),
        D::Date(_) => chrono_value::<NaiveDate>(data, Value::Date)?,
        D::Time(_) => chrono_value::<NaiveTime>(data, Value::Time)?,
        D::DateTime(_) | D::SmallDateTime(_) | D::DateTime2(_) => {
            chrono_value::<NaiveDateTime>(data, Value::DateTime)?
        }
        D::DateTimeOffset(_) => chrono_value::<DateTime<FixedOffset>>(data, Value::DateTimeTz)?,
    })
}

/// 绑定参数。时间类型按 tiberius 的映射（date、time、datetime2、datetimeoffset）。
pub(crate) struct Param(pub(crate) Value);

impl Param {
    /// 检查值能否作为参数（大字段预览不能写回）。
    pub(crate) fn new(v: &Value) -> Result<Self> {
        match v {
            Value::Lob(_) => Err(Error::invalid(msg!("literal.lob"))),
            v => Ok(Self(v.clone())),
        }
    }
}

/// 精确小数文本 → TDS 的 Numeric；超出 38 位或不是数字时返回 None（改用字符串，由服务器转换）。
fn parse_numeric(s: &str) -> Option<Numeric> {
    let s = s.trim();
    let (neg, body) = match s.strip_prefix('-') {
        Some(b) => (true, b),
        None => (false, s.strip_prefix('+').unwrap_or(s)),
    };
    let (int, frac) = body.split_once('.').unwrap_or((body, ""));
    if int.is_empty() && frac.is_empty() || !(int.bytes().chain(frac.bytes())).all(|c| c.is_ascii_digit()) {
        return None;
    }
    if int.len() + frac.len() > 38 {
        return None;
    }
    let value: i128 = format!("{int}{frac}").parse().ok()?;
    Some(Numeric::new_with_scale(if neg { -value } else { value }, u8::try_from(frac.len()).ok()?))
}

impl ToSql for Param {
    fn to_sql(&self) -> ColumnData<'_> {
        match &self.0 {
            // 类型未知的 NULL 用 nvarchar，服务器可以隐式转换成任何类型
            Value::Null | Value::Lob(_) => ColumnData::String(None),
            Value::Bool(b) => ColumnData::Bit(Some(*b)),
            Value::Int(i) => ColumnData::I64(Some(*i)),
            Value::UInt(u) => match i64::try_from(*u) {
                Ok(i) => ColumnData::I64(Some(i)),
                Err(_) => ColumnData::Numeric(Some(Numeric::new_with_scale(i128::from(*u), 0))),
            },
            Value::Float(f) => ColumnData::F64(Some(*f)),
            Value::Decimal(s) => match parse_numeric(s) {
                Some(n) => ColumnData::Numeric(Some(n)),
                None => ColumnData::String(Some(s.as_str().into())),
            },
            Value::String(s) | Value::Interval(s) => ColumnData::String(Some(s.as_str().into())),
            Value::Json(j) => ColumnData::String(Some(j.to_string().into())),
            Value::Uuid(u) => ColumnData::Guid(Some(*u)),
            Value::Bytes(b) => ColumnData::Binary(Some(b.as_slice().into())),
            Value::Date(d) => d.to_sql(),
            Value::Time(t) => t.to_sql(),
            Value::DateTime(dt) => dt.to_sql(),
            Value::DateTimeTz(dt) => dt.to_sql(),
            Value::Other { text, .. } => ColumnData::String(Some(text.as_str().into())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numeric_text_pads_and_signs() {
        let n = |v: i128, s: u8| numeric_text(Numeric::new_with_scale(v, s));
        assert_eq!(n(12345, 2), "123.45");
        assert_eq!(n(-5, 3), "-0.005");
        assert_eq!(n(42, 0), "42");
        assert_eq!(n(0, 2), "0.00");
        assert_eq!(n(-100, 0), "-100");
    }

    #[test]
    fn decimal_params_become_numeric() {
        let n = parse_numeric("-123.4500").unwrap();
        assert_eq!((n.value(), n.scale()), (-1234500, 4));
        assert_eq!(parse_numeric(".5").map(|n| (n.value(), n.scale())), Some((5, 1)));
        assert!(parse_numeric("1e5").is_none());
        assert!(parse_numeric("NaN").is_none());
        assert!(parse_numeric(".").is_none());
        assert!(parse_numeric(&"9".repeat(39)).is_none());
        assert!(matches!(Param(Value::Decimal("1e5".into())).to_sql(), ColumnData::String(Some(_))));
        assert!(matches!(Param(Value::UInt(u64::MAX)).to_sql(), ColumnData::Numeric(Some(_))));
        assert!(
            Param::new(&Value::Lob(dblens_core::LobPreview {
                binary: false,
                preview: String::new(),
                total_len: 1,
                truncated: true
            }))
            .is_err()
        );
    }

    #[test]
    fn cells_convert_and_money_is_decimal() {
        assert_eq!(
            cell_value(&ColumnData::F64(Some(12.5)), ColumnType::Money, 10).unwrap(),
            Value::Decimal("12.5000".into())
        );
        assert_eq!(
            cell_value(&ColumnData::F64(Some(1.5)), ColumnType::Floatn, 10).unwrap(),
            Value::Float(1.5)
        );
        assert_eq!(cell_value(&ColumnData::I32(None), ColumnType::Intn, 10).unwrap(), Value::Null);
        assert_eq!(cell_value(&ColumnData::U8(Some(255)), ColumnType::Int1, 10).unwrap(), Value::Int(255));
        let long = ColumnData::String(Some("x".repeat(20).into()));
        assert!(matches!(cell_value(&long, ColumnType::NVarchar, 10).unwrap(), Value::Lob(_)));
        let d = NaiveDate::from_ymd_opt(2024, 2, 29).unwrap();
        let ColumnData::Date(date) = d.to_sql() else { panic!("date") };
        assert_eq!(cell_value(&ColumnData::Date(date), ColumnType::Daten, 10).unwrap(), Value::Date(d));
    }
}
```

- [ ] **Step 22: 修改 `crates/dblens-driver/Cargo.toml`**

```diff
--- a/crates/dblens-driver/Cargo.toml
+++ b/crates/dblens-driver/Cargo.toml
@@ -18,4 +18,5 @@ tokio.workspace = true
 url.workspace = true
 
 [dev-dependencies]
+serde_json.workspace = true
 tokio.workspace = true
```

- [ ] **Step 23: 修改 `crates/dblens-driver/src/params.rs`**

```diff
--- a/crates/dblens-driver/src/params.rs
+++ b/crates/dblens-driver/src/params.rs
@@ -14,6 +14,8 @@ pub enum DriverId {
     Postgres,
     Oracle,
     Dameng,
+    #[serde(rename = "sqlserver")]
+    SqlServer,
 }
 
 /// 建立连接所需的参数。
@@ -23,7 +25,7 @@ pub struct ConnectParams {
     pub port: u16,
     pub user: String,
     pub password: String,
-    /// MySQL：默认库；PG：数据库名；Oracle：服务名；达梦：可为空。
+    /// MySQL：默认库；PG：数据库名；Oracle：服务名；达梦：可为空；SQL Server：登录后的初始库，可为空。
     pub database: Option<String>,
     /// 驱动特有的选项。
     pub options: BTreeMap<String, String>,
@@ -48,7 +50,7 @@ impl ConnectParams {
     pub const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
 
     /// 解析 `scheme://user:pass@host:port/database?k=v`。主要用于测试和命令行。
-    /// 支持的 scheme：mysql、mariadb、postgres、postgresql、oracle、dm、dameng。
+    /// 支持的 scheme：mysql、mariadb、postgres、postgresql、oracle、dm、dameng、sqlserver、mssql。
     /// 选项 `connect_timeout` 的单位为秒。
     pub fn from_url(s: &str) -> Result<(DriverId, ConnectParams)> {
         let u = url::Url::parse(s).map_err(|e| Error::connect(msg!("params.invalid_url", error = e)))?;
@@ -57,6 +59,7 @@ impl ConnectParams {
             "postgres" | "postgresql" => (DriverId::Postgres, 5432),
             "oracle" => (DriverId::Oracle, 1521),
             "dm" | "dameng" => (DriverId::Dameng, 5236),
+            "sqlserver" | "mssql" => (DriverId::SqlServer, 1433),
             other => return Err(Error::connect(msg!("params.unsupported_scheme", scheme = other))),
         };
         let decode = |v: &str| percent_decode_str(v).decode_utf8_lossy().into_owned();
@@ -179,9 +182,21 @@ mod tests {
         assert_eq!((id, p.port, p.database), (DriverId::Postgres, 5432, None));
         assert_eq!(ConnectParams::from_url("oracle://u:p@h/ORCLPDB1").unwrap().1.port, 1521);
         assert_eq!(ConnectParams::from_url("dm://SYSDBA:p@h").unwrap().0, DriverId::Dameng);
+        let (id, p) = ConnectParams::from_url("mssql://sa:p@h/app?auth=windows").unwrap();
+        assert_eq!((id, p.port, p.options["auth"].as_str()), (DriverId::SqlServer, 1433, "windows"));
         assert_eq!(ConnectParams::from_url("dm://SYSDBA:p@h").unwrap().1.port, 5236);
     }
 
+    #[test]
+    fn driver_ids_match_web_names() {
+        let names: Vec<String> =
+            [DriverId::Mysql, DriverId::Postgres, DriverId::Oracle, DriverId::Dameng, DriverId::SqlServer]
+                .iter()
+                .map(|d| serde_json::to_string(d).unwrap())
+                .collect();
+        assert_eq!(names, ["\"mysql\"", "\"postgres\"", "\"oracle\"", "\"dameng\"", "\"sqlserver\""]);
+    }
+
     #[test]
     fn connect_timeout_option() {
         let (_, p) = ConnectParams::from_url("mysql://u:p@h/db?connect_timeout=3").unwrap();
```

- [ ] **Step 24: 修改 `crates/dblens-service/Cargo.toml`**

```diff
--- a/crates/dblens-service/Cargo.toml
+++ b/crates/dblens-service/Cargo.toml
@@ -6,11 +6,12 @@ rust-version.workspace = true
 publish.workspace = true
 
 [features]
-default = ["mysql", "postgres", "oracle", "dameng"]
+default = ["mysql", "postgres", "oracle", "dameng", "sqlserver"]
 mysql = ["dep:dblens-driver-mysql"]
 postgres = ["dep:dblens-driver-postgres"]
 oracle = ["dep:dblens-driver-oracle"]
 dameng = ["dep:dblens-driver-dameng"]
+sqlserver = ["dep:dblens-driver-sqlserver"]
 # 公开测试用的假驱动 `fake`
 testing = []
 
@@ -29,6 +30,7 @@ dblens-driver-dameng = { workspace = true, optional = true }
 dblens-driver-mysql = { workspace = true, optional = true }
 dblens-driver-oracle = { workspace = true, optional = true }
 dblens-driver-postgres = { workspace = true, optional = true }
+dblens-driver-sqlserver = { workspace = true, optional = true }
 directories.workspace = true
 encoding_rs.workspace = true
 encoding_rs_io.workspace = true
```

- [ ] **Step 25: 修改 `crates/dblens-service/src/changes.rs`**

```diff
--- a/crates/dblens-service/src/changes.rs
+++ b/crates/dblens-service/src/changes.rs
@@ -11,6 +11,9 @@ use crate::session::{ApplyOutcome, SessionId};
 /// 前端提交的改动集。
 #[derive(Debug, Clone, PartialEq, Deserialize)]
 pub struct ChangeSet {
+    /// 所在的库（SQL Server）：表结构从这个库读取。
+    #[serde(default)]
+    pub catalog: Option<String>,
     pub schema: String,
     pub table: String,
     pub changes: Vec<RowChange>,
@@ -18,6 +21,7 @@ pub struct ChangeSet {
 
 impl Service {
     async fn change_statements(&self, profile: &Profile, set: &ChangeSet) -> Result<Vec<ChangeStatement>> {
+        let profile = &profile.in_catalog(set.catalog.as_deref());
         let dialect = self.metadata.dialect(profile).await?;
         let table = self
             .metadata
```

- [ ] **Step 26: 修改 `crates/dblens-service/src/compare.rs`**

```diff
--- a/crates/dblens-service/src/compare.rs
+++ b/crates/dblens-service/src/compare.rs
@@ -266,7 +266,7 @@ impl Service {
                 })
             }
             CompareSideRef::Database { profile, schema } => {
-                let profile = self.profile(profile)?;
+                let profile = self.feature_profile(profile)?;
                 let dialect = self.metadata.dialect(&profile).await?;
                 // 跳过元数据缓存，以库中最新的结构为准
                 let args = MetaArgs { schema: Some(schema.clone()), ..Default::default() };
```

- [ ] **Step 27: 修改 `crates/dblens-service/src/design.rs`**

```diff
--- a/crates/dblens-service/src/design.rs
+++ b/crates/dblens-service/src/design.rs
@@ -180,7 +180,7 @@ impl Service {
 
     /// 打开设计器：原表（直接从库读取）、存储属性和行数估计、指纹。
     pub async fn design_table(&self, profile_id: &str, schema: &str, name: &str) -> Result<DesignTable> {
-        let profile = self.profile(profile_id)?;
+        let profile = self.feature_profile(profile_id)?;
         let table = self.fresh_table(&profile, schema, name).await?;
         let storage =
             meta::table_storage(&*self.fresh_rows(&profile, MetaKey::TableStorage, schema, name).await?)?;
@@ -217,7 +217,7 @@ impl Service {
     }
 
     async fn prepare_design(&self, profile_id: &str, req: &DesignRequest) -> Result<Prepared> {
-        let profile = self.profile(profile_id)?;
+        let profile = self.feature_profile(profile_id)?;
         let dialect = self.metadata.dialect(&profile).await?;
         let mut draft = req.draft.clone();
         if let Some(name) = &req.name {
```

- [ ] **Step 28: 修改 `crates/dblens-service/src/document/job.rs`**

```diff
--- a/crates/dblens-service/src/document/job.rs
+++ b/crates/dblens-service/src/document/job.rs
@@ -204,7 +204,7 @@ impl Service {
                 }
             }
             DocSource::Database { profile, schema } => {
-                let p = self.profile(profile)?;
+                let p = self.feature_profile(profile)?;
                 // 跳过元数据缓存，以库中最新的结构为准
                 let args = MetaArgs { schema: Some(schema.clone()), ..Default::default() };
                 for key in MetaKey::TABLE_DETAIL.into_iter().chain([MetaKey::Views]) {
```

- [ ] **Step 29: 修改 `crates/dblens-service/src/import/job.rs`**

```diff
--- a/crates/dblens-service/src/import/job.rs
+++ b/crates/dblens-service/src/import/job.rs
@@ -264,7 +264,7 @@ impl Service {
         table: &str,
         def: &NewTable,
     ) -> Result<String> {
-        let profile = self.profile(profile_id)?;
+        let profile = self.feature_profile(profile_id)?;
         let dialect = self.metadata.dialect(&profile).await?;
         Ok(dialect.create_table_sql(schema, &new_table(&dialect, table, def)?))
     }
@@ -285,7 +285,7 @@ impl Service {
 
     /// 提交导入任务，返回任务 id。提交时检查连接、文件、目标表、映射和执行策略；执行在后台进行。
     pub async fn start_import(self: &Arc<Self>, req: ImportRequest, allow_path: bool) -> Result<String> {
-        let profile = self.profile(&req.profile_id)?;
+        let profile = self.feature_profile(&req.profile_id)?;
         let password = self.secrets.get(&profile.id)?.unwrap_or_default();
         let dialect = self.metadata.dialect(&profile).await?;
         let (table, create_sql) = match &req.create {
```

- [ ] **Step 30: 修改 `crates/dblens-service/src/metadata.rs`**

```diff
--- a/crates/dblens-service/src/metadata.rs
+++ b/crates/dblens-service/src/metadata.rs
@@ -1,5 +1,6 @@
 //! 元数据浏览（设计文档 5.3、6.4 节）：每个连接配置一个小连接池（默认 2 个连接），不会被编辑器里正在跑的
 //! 长查询堵住；查询结果按（连接配置、查询种类、参数）缓存，手动刷新或执行 DDL 后失效。
+//! 有数据库这一层的驱动（SQL Server）按库分开连接池和缓存（`Profile::scope`），查询在连接的初始库中执行。
 
 use std::collections::HashMap;
 use std::sync::{Arc, Mutex};
@@ -12,13 +13,14 @@ use dblens_driver::{Connection, collect_all};
 use tokio::sync::Semaphore;
 
 use crate::policy::ObjectRef;
-use crate::profile::Profile;
+use crate::profile::{Profile, scope_of};
 use crate::registry::DriverRegistry;
 use crate::secret::Secrets;
 
 /// 每个连接配置用于浏览元数据的最大连接数。
 pub const METADATA_POOL_SIZE: usize = 2;
 
+/// （`Profile::scope`、查询种类、参数）。
 type CacheKey = (String, MetaKey, MetaArgs);
 
 /// 元数据查询结果的缓存。
@@ -42,10 +44,10 @@ impl MetadataCache {
         }
     }
 
-    /// 丢掉一条缓存的查询结果（下次重新查询）。
+    /// 丢掉一条缓存的查询结果（这个连接配置的所有库中的，下次重新查询）。
     pub fn forget(&self, profile_id: &str, key: MetaKey, args: &MetaArgs) {
         if let Ok(mut e) = self.entries.lock() {
-            e.remove(&(profile_id.to_string(), key, args.clone()));
+            e.retain(|(p, k, a), _| !(scope_of(p, profile_id) && *k == key && a == args));
         }
     }
 
@@ -62,14 +64,14 @@ impl MetadataCache {
     pub fn invalidate(&self, profile_id: &str, schema: Option<&str>) {
         let Ok(mut e) = self.entries.lock() else { return };
         e.retain(|(p, key, args), _| {
-            if p != profile_id {
+            if !scope_of(p, profile_id) {
                 return true;
             }
             match schema {
                 None => false,
                 Some(s) => {
                     let same = args.schema.as_deref().is_some_and(|a| a.eq_ignore_ascii_case(s));
-                    !(same || *key == MetaKey::Schemas)
+                    !(same || matches!(key, MetaKey::Schemas | MetaKey::Catalogs))
                 }
             }
         });
@@ -119,14 +121,17 @@ impl MetadataService {
             .clone())
     }
 
-    /// 关闭某个连接配置的元数据连接并清掉缓存（连接配置被修改或删除时调用）。
+    /// 关闭某个连接配置（所有库）的元数据连接并清掉缓存（连接配置被修改或删除时调用）。
     pub async fn forget(&self, profile_id: &str) -> Result<()> {
-        let pool = self
-            .pools
-            .lock()
-            .map_err(|_| Error::internal(msg!("internal.lock_poisoned", lock = "pools")))?
-            .remove(profile_id);
-        if let Some(pool) = pool {
+        let pools: Vec<Arc<Pool>> = {
+            let mut all = self
+                .pools
+                .lock()
+                .map_err(|_| Error::internal(msg!("internal.lock_poisoned", lock = "pools")))?;
+            let keys: Vec<String> = all.keys().filter(|k| scope_of(k, profile_id)).cloned().collect();
+            keys.iter().filter_map(|k| all.remove(k)).collect()
+        };
+        for pool in pools {
             let idle: Vec<_> = pool.idle.lock().map(|mut v| v.drain(..).collect()).unwrap_or_default();
             for (conn, _) in idle {
                 let _ = conn.close().await;
@@ -154,11 +159,11 @@ impl MetadataService {
         args: &MetaArgs,
         build: impl FnOnce(&Dialect) -> Result<Option<MetaQuery>>,
     ) -> Result<Option<Arc<MetaRows>>> {
-        let cache_key = (profile.id.clone(), key, args.clone());
+        let cache_key = (profile.scope(), key, args.clone());
         if let Some(rows) = self.cache.get(&cache_key) {
             return Ok(Some(rows));
         }
-        let pool = self.pool(&profile.id)?;
+        let pool = self.pool(&profile.scope())?;
         let _permit =
             pool.permits.acquire().await.map_err(|_| Error::internal(msg!("metadata.pool_closed")))?;
         let idle = pool
@@ -218,7 +223,7 @@ impl MetadataService {
 
     /// 连接的方言（取一次连接上的识别结果）。
     pub async fn dialect(&self, profile: &Profile) -> Result<Dialect> {
-        let pool = self.pool(&profile.id)?;
+        let pool = self.pool(&profile.scope())?;
         if let Some((_, d)) = pool
             .idle
             .lock()
@@ -238,6 +243,15 @@ impl MetadataService {
         Ok(dialect)
     }
 
+    /// 数据库列表（只有 SQL Server 有这一层，其他驱动返回空）。
+    pub async fn catalogs(&self, profile: &Profile, filter: Option<&str>) -> Result<Vec<SchemaInfo>> {
+        let args = MetaArgs { filter: filter.map(Into::into), ..Default::default() };
+        match self.rows(profile, MetaKey::Catalogs, &args).await? {
+            Some(rows) => meta::schemas(&rows),
+            None => Ok(vec![]),
+        }
+    }
+
     pub async fn schemas(&self, profile: &Profile, filter: Option<&str>) -> Result<Vec<SchemaInfo>> {
         let args = MetaArgs { filter: filter.map(Into::into), ..Default::default() };
         meta::schemas(&*self.rows_or_empty(profile, MetaKey::Schemas, &args).await?)
```

- [ ] **Step 31: 修改 `crates/dblens-service/src/migrate/mod.rs`**

```diff
--- a/crates/dblens-service/src/migrate/mod.rs
+++ b/crates/dblens-service/src/migrate/mod.rs
@@ -176,8 +176,8 @@ fn problems(plan: &MigrationPlan, draft: &MigrationDraft) -> Vec<Msg> {
 impl Service {
     /// 读两端的结构。`fresh`：跳过元数据缓存（开始执行前以库中最新的结构为准）。
     async fn load_migration(&self, plan: &MigrationPlan, fresh: bool) -> Result<Loaded> {
-        let source = self.profile(&plan.source.profile)?;
-        let target = self.profile(&plan.target.profile)?;
+        let source = self.feature_profile(&plan.source.profile)?;
+        let target = self.feature_profile(&plan.target.profile)?;
         let source_dialect = self.metadata.dialect(&source).await?;
         let target_dialect = self.metadata.dialect(&target).await?;
         let schemas = self.metadata.schemas(&target, None).await?;
@@ -483,8 +483,8 @@ impl Service {
 
     /// 作为任务执行迁移记录，返回任务 id。结束后刷新目标的元数据缓存、保存记录、写迁移报告。
     async fn launch(self: &Arc<Self>, record: MigrationRecord) -> Result<String> {
-        let source = self.profile(&record.plan.source.profile)?;
-        let target = self.profile(&record.plan.target.profile)?;
+        let source = self.feature_profile(&record.plan.source.profile)?;
+        let target = self.feature_profile(&record.plan.target.profile)?;
         let source_dialect = self.metadata.dialect(&source).await?;
         let target_dialect = self.metadata.dialect(&target).await?;
         let source_password = self.secrets.get(&source.id)?.unwrap_or_default();
```

- [ ] **Step 32: 修改 `crates/dblens-service/src/model/ops.rs`**

```diff
--- a/crates/dblens-service/src/model/ops.rs
+++ b/crates/dblens-service/src/model/ops.rs
@@ -92,7 +92,7 @@ impl Service {
 
     /// 反向生成：作为任务执行，返回任务 id；结束后模型已打开，任务状态的 `model` 是模型编号。
     pub async fn start_reverse(self: &Arc<Self>, req: ReverseRequest) -> Result<String> {
-        let profile = self.profile(&req.profile_id)?;
+        let profile = self.feature_profile(&req.profile_id)?;
         if req.schemas.is_empty() {
             return Err(Error::invalid(msg!("model.no_schemas")));
         }
```

- [ ] **Step 33: 修改 `crates/dblens-service/src/profile.rs`**

```diff
--- a/crates/dblens-service/src/profile.rs
+++ b/crates/dblens-service/src/profile.rs
@@ -68,6 +68,14 @@ pub struct ProfileInput {
     pub product: Option<Variant>,
 }
 
+/// `Profile::scope` 中连接配置 id 与库名的分隔符。
+pub(crate) const SCOPE_SEP: char = '\u{1f}';
+
+/// 范围是否属于这个连接配置（任何库）。
+pub(crate) fn scope_of(scope: &str, profile_id: &str) -> bool {
+    scope.strip_prefix(profile_id).is_some_and(|rest| rest.is_empty() || rest.starts_with(SCOPE_SEP))
+}
+
 fn default_timeout() -> u64 {
     ConnectParams::DEFAULT_CONNECT_TIMEOUT.as_secs()
 }
@@ -85,6 +93,28 @@ pub struct Profile {
 }
 
 impl Profile {
+    /// 在某个数据库中工作的同一连接配置（SQL Server 的连接 → 数据库 → 架构）：连接时以这个库为初始库。
+    /// 没给库名时原样返回。
+    pub fn in_catalog(&self, catalog: Option<&str>) -> Profile {
+        match catalog.filter(|c| !c.is_empty()) {
+            Some(c) => {
+                let mut p = self.clone();
+                p.input.database = Some(c.to_string());
+                p
+            }
+            None => self.clone(),
+        }
+    }
+
+    /// 元数据连接池和缓存的范围：通常就是连接配置 id；有数据库这一层的驱动（SQL Server）按库分开，
+    /// 写成 `id` + `\u{1f}` + 库名（见 `scope_of`）。
+    pub fn scope(&self) -> String {
+        match (self.input.driver, &self.input.database) {
+            (DriverId::SqlServer, Some(db)) => format!("{}{SCOPE_SEP}{db}", self.id),
+            _ => self.id.clone(),
+        }
+    }
+
     /// 用给定密码组成连接参数。原生库目录由驱动注册表另外补上。
     pub fn params(&self, password: String) -> ConnectParams {
         let p = &self.input;
@@ -106,6 +136,7 @@ pub(crate) fn driver_name(d: DriverId) -> &'static str {
         DriverId::Postgres => "postgres",
         DriverId::Oracle => "oracle",
         DriverId::Dameng => "dameng",
+        DriverId::SqlServer => "sqlserver",
     }
 }
 
@@ -115,6 +146,7 @@ fn parse_driver(s: &str) -> rusqlite::Result<DriverId> {
         "postgres" => Ok(DriverId::Postgres),
         "oracle" => Ok(DriverId::Oracle),
         "dameng" => Ok(DriverId::Dameng),
+        "sqlserver" => Ok(DriverId::SqlServer),
         other => {
             Err(rusqlite::Error::InvalidColumnType(2, format!("driver={other}"), rusqlite::types::Type::Text))
         }
```

- [ ] **Step 34: 修改 `crates/dblens-service/src/registry.rs`**

```diff
--- a/crates/dblens-service/src/registry.rs
+++ b/crates/dblens-service/src/registry.rs
@@ -56,7 +56,7 @@ fn native_lib(id: DriverId) -> Option<(NativeLib, &'static str)> {
     match id {
         DriverId::Oracle => Some((NativeLib::OracleClient, "oracle_client_lib_dir")),
         DriverId::Dameng => Some((NativeLib::DamengDpi, "dpi_lib_dir")),
-        DriverId::Mysql | DriverId::Postgres => None,
+        DriverId::Mysql | DriverId::Postgres | DriverId::SqlServer => None,
     }
 }
 
@@ -70,6 +70,7 @@ fn display_name(id: DriverId) -> Msg {
         DriverId::Postgres => Msg::raw("PostgreSQL"),
         DriverId::Oracle => Msg::raw("Oracle"),
         DriverId::Dameng => msg!("variant.dameng"),
+        DriverId::SqlServer => Msg::raw("SQL Server"),
     }
 }
 
@@ -90,6 +91,8 @@ impl DriverRegistry {
             Box::new(dblens_driver_oracle::OracleDriver),
             #[cfg(feature = "dameng")]
             Box::new(dblens_driver_dameng::DmDriver),
+            #[cfg(feature = "sqlserver")]
+            Box::new(dblens_driver_sqlserver::SqlServerDriver),
         ];
         Self::with_drivers(store, drivers)
     }
@@ -132,7 +135,7 @@ impl DriverRegistry {
     }
 
     pub fn statuses(&self) -> Result<Vec<DriverStatus>> {
-        [DriverId::Mysql, DriverId::Postgres, DriverId::Oracle, DriverId::Dameng]
+        [DriverId::Mysql, DriverId::Postgres, DriverId::Oracle, DriverId::Dameng, DriverId::SqlServer]
             .into_iter()
             .map(|id| self.status(id))
             .collect()
@@ -263,7 +266,9 @@ mod tests {
         let (_, r) = registry();
         let s = r.status(DriverId::Postgres).unwrap();
         assert!(s.compiled && s.available && !s.needs_native_lib && s.hint.is_none());
-        assert_eq!(r.statuses().unwrap().len(), 4);
+        assert_eq!(r.statuses().unwrap().len(), 5);
+        let ms = r.status(DriverId::SqlServer).unwrap();
+        assert!(ms.compiled && ms.available && !ms.needs_native_lib, "{ms:?}");
         assert!(matches!(r.set_lib_dir(DriverId::Mysql, None), Err(Error::Invalid { .. })));
     }
 
```

- [ ] **Step 35: 修改 `crates/dblens-service/src/service.rs`**

```diff
--- a/crates/dblens-service/src/service.rs
+++ b/crates/dblens-service/src/service.rs
@@ -3,8 +3,9 @@
 use std::path::Path;
 use std::sync::Arc;
 
-use dblens_core::{Lang, Result, set_default_lang};
+use dblens_core::{Error, Lang, Result, msg, set_default_lang};
 use dblens_dialect::{Dialect, OpsQuery, Variant};
+use dblens_driver::DriverId;
 
 use crate::export::ExportManager;
 use crate::history::HistoryLog;
@@ -102,6 +103,16 @@ impl Service {
         self.store.profile(id)
     }
 
+    /// 写入类功能（表设计器、导入、文档导出、结构对比、迁移、逆向成模型）用的连接配置：SQL Server 在 M6b 之前
+    /// 不支持这些功能，在入口处拒绝，不会生成错误的 SQL。
+    pub(crate) fn feature_profile(&self, id: &str) -> Result<Profile> {
+        let p = self.profile(id)?;
+        if p.input.driver == DriverId::SqlServer {
+            return Err(Error::invalid(msg!("sqlserver.later")));
+        }
+        Ok(p)
+    }
+
     /// 新建连接配置；给出密码时一并保存。
     pub fn create_profile(&self, input: ProfileInput, password: Option<&str>) -> Result<Profile> {
         let p = self.store.create_profile(input)?;
```

- [ ] **Step 36: 修改 `crates/dblens-service/src/session.rs`**

```diff
--- a/crates/dblens-service/src/session.rs
+++ b/crates/dblens-service/src/session.rs
@@ -864,6 +864,13 @@ impl Actor {
                     break;
                 }
             }
+            // SQL Server 的执行计划打开了会话级开关，无论成败都要关掉，否则之后的语句都只返回计划
+            if req.explain
+                && let Some(cleanup) = ctx.dialect.explain_cleanup()
+                && let Ok(mut stream) = conn.execute(cleanup, &[]).await
+            {
+                while let Ok(Some(_)) = stream.next(100).await {}
+            }
             let outcome = match &result {
                 Ok(()) => Outcome::Succeeded { affected },
                 Err(Error::Cancelled) => Outcome::Cancelled,
```

- [ ] **Step 37: 修改 `vendor/PATCHES.md`**

```diff
--- a/vendor/PATCHES.md
+++ b/vendor/PATCHES.md
@@ -1,6 +1,6 @@
 # 打了补丁的第三方库
 
-工作区 `Cargo.toml` 的 `[patch.crates-io]` 用这里的副本替换 crates.io 上的同名库。副本是 crates.io 发布包的原样内容（只删去了 `Cargo.toml` 中的 `[[test]]`、`[[bench]]`，发布包里没有对应的文件），再加上下面列出的改动。目录在 `[workspace] exclude` 中，不是工作区成员，clippy 不检查。
+工作区 `Cargo.toml` 的 `[patch.crates-io]` 用这里的副本替换 crates.io 上的同名库。副本是 crates.io 发布包的原样内容（只删去了 `Cargo.toml` 中的 `[[test]]`、`[[bench]]`、`[[example]]`），再加上下面列出的改动。目录在 `[workspace] exclude` 中，不是工作区成员，clippy 不检查。
 
 ## openGauss 的 SHA256 认证
 
@@ -15,11 +15,21 @@ openGauss 用认证请求码 10（与 PostgreSQL 的 SASL 相同）发它自己
 
 已知的告警：`tokio-postgres/src/client.rs` 在 Windows 上有一个 unused import（上游代码，只在 unix 下使用），不影响构建。
 
+## SQL Server 的影响行数、服务器消息和取消
+
+tiberius 的结果流（`QueryStream`）只给出结果集和行：语句的影响行数（DONE / DONEINPROC 令牌中的行数）和服务器的 INFO 消息
+（`PRINT`、级别 10 及以下的 `RAISERROR`）都被丢掉，而执行 SQL 时要显示它们（M6a 设计文档 3.4 节）。另外，结果集读到一半时取消，
+连接会错位（见下表最后一项）。
+
+| 库 | 上游版本 | 改动 |
+|---|---|---|
+| `tiberius` | 0.13.0 | `src/tds/stream/query.rs`：`QueryItem` 加 `Done(u64)`（设置了 DONE_COUNT 的 DONE、DONEINPROC）和 `Info(InfoMessage)`，`poll_next` 产出它们；`forward_to_metadata` 遇到这两种令牌时停下（不跳过）；其余对 `QueryItem` 的 `match` 补上新分支。新增 `InfoMessage` 结构，`src/result.rs` 导出它。`src/tds/codec/token/token_done.rs` 加 `has_count()`。`src/client/connection.rs`：`cancel_request` 发出取消后改为按包丢弃剩下的响应，直到某条响应以带 DONE_ATTN 的 DONE 令牌结束（新增 `drain_to_attention_ack`，返回 `()`）；原来按令牌读到当前响应末尾，结果集读到一半丢弃时确认会留在连接上，下一个请求得到空结果。原来的 `flush_done_attention`、`is_attention` 不再使用，标了 `allow(dead_code)` |
+
 ## 升级步骤
 
-每个里程碑开头的依赖复查发现这两个库有新版本时：
+每个里程碑开头的依赖复查发现这些库有新版本时：
 
 1. 从 crates.io 下载新版本（`cargo download` 或取 `$CARGO_HOME/registry/src/*/<库>-<版本>`），替换 `vendor/<库>/` 中除 `Cargo.toml` 改动以外的全部文件；
-2. 按上表重新套用改动（`gauss.rs` 整个文件复制过去；其余是几行的插入）；
+2. 按上表重新套用改动（`gauss.rs` 整个文件复制过去；其余是几行的插入或分支）；
 3. 更新上表中的上游版本；
-4. 运行 `CARGO_TARGET_DIR=target cargo test --manifest-path vendor/postgres-protocol/Cargo.toml --lib authentication`（构建目录用工作区的 `target`，不要在 vendor 下生成），再用 `DBLENS_TEST_OPENGAUSS_SHA_URL` 跑 PG 驱动的一致性测试。
+4. 运行 `CARGO_TARGET_DIR=target cargo test --manifest-path vendor/postgres-protocol/Cargo.toml --lib authentication`（构建目录用工作区的 `target`，不要在 vendor 下生成），再用 `DBLENS_TEST_OPENGAUSS_SHA_URL` 跑 PG 驱动的一致性测试；tiberius 用 `DBLENS_TEST_SQLSERVER_URL` 跑 SQL Server 驱动的一致性测试（其中多结果集、影响行数和 PRINT 消息的用例覆盖补丁）。
```

- [ ] **Step 38: 更新锁文件**

`cargo build` 会更新 `Cargo.lock`。

- [ ] **Step 39: 检查**

```bash
export CARGO_INCREMENTAL=0
cargo fmt --all -- --check
cargo clippy --config "source.mirror.registry='sparse+https://rsproxy.cn/index/'" --workspace --all-targets -- -D warnings
DBLENS_LANG=en cargo test --config "source.mirror.registry='sparse+https://rsproxy.cn/index/'" --workspace
```

实库测试（SQL Server）：测试虚拟机 192.168.139.106 上的 Docker 容器 `mssql2022`（`mcr.microsoft.com/mssql/server:2022-latest`，1433，`--restart unless-stopped`）。sa 密码在 root 的 `/root/dblens-test/creds.env`（`MSSQL_SA=`），读进 shell 变量后做 URL 编码，以内联环境变量传入，不写进任何文件：

```bash
H=192.168.139.106
P=$(ssh -i ~/.ssh/nexa_linux_vm -o IdentitiesOnly=yes nexa@$H 'sudo grep "^MSSQL_SA=" /root/dblens-test/creds.env | cut -d= -f2-')
ENC=$(python -c "import sys, urllib.parse; print(urllib.parse.quote(sys.argv[1], safe=''))" "$P")
DBLENS_TEST_SQLSERVER_URL="sqlserver://sa:$ENC@$H:1433/dblens_test" DBLENS_LANG=en CARGO_INCREMENTAL=0 \
  cargo test --config "source.mirror.registry='sparse+https://rsproxy.cn/index/'" --workspace -- --test-threads=1 sqlserver
```

测试库 `dblens_test` 由测试自己建（不存在时）。全部实库测试串行运行（`--test-threads=1`），不加 `--include-ignored`。

回归：再带上原有 12 个实库目标的环境变量（MySQL、MariaDB、PG、Oracle、达梦，以及 TiDB、OceanBase、openGauss 三个、人大金仓两个；密码读法同上）跑一遍 `cargo test … --workspace -- --test-threads=1`。
- OceanBase 在虚拟机开机后要几分钟才能连上（"Server is initializing"），连接被拒或报这个错时等它启动完再重跑。
- 达梦偶发"网络通讯失败"，失败时单独重跑该用例。

Windows 认证（设计文档 10 节，只能在 Windows 上运行）：Win11 测试虚拟机 192.168.139.103 上的 SQL Server Express（实例 `SQLEXPRESS`，固定端口 1433），本地账号 `dbltest` 已映射为登录名。密码在虚拟机的 `C:\ProgramData\dblens-test\creds.txt`（只有管理员可读）。
- 指定账号（NTLM，从开发机连）：`DBLENS_TEST_SQLSERVER_WINAUTH_URL="sqlserver://DESKTOP-F4O6PL4%5Cdbltest:<编码后的密码>@192.168.139.103:1433/dblens_test?auth=windows&trust_server_certificate=true"`；
- 当前用户（在虚拟机上以 `dbltest` 身份运行）：`DBLENS_TEST_SQLSERVER_INTEGRATED_URL="sqlserver://localhost:1433/dblens_test?auth=integrated&trust_server_certificate=true"`。
没设置时这两个测试跳过。

Expected: 全部测试通过；SQL Server 的实库测试（驱动一致性、元数据、服务、接口）全部通过；回归无失败。

- [ ] **Step 40: 提交**

```bash
git add Cargo.lock Cargo.toml crates/dblens-api/src/lib.rs crates/dblens-api/src/routes.rs crates/dblens-api/tests/live.rs crates/dblens-core/locales/app.json crates/dblens-dialect/Cargo.toml crates/dblens-dialect/src/variant.rs crates/dblens-dialect/tests/fixtures/sqlserver.sql crates/dblens-dialect/tests/meta_live.rs crates/dblens-driver-sqlserver/Cargo.toml crates/dblens-driver-sqlserver/src/actor.rs crates/dblens-driver-sqlserver/src/conn.rs crates/dblens-driver-sqlserver/src/error.rs crates/dblens-driver-sqlserver/src/exec.rs crates/dblens-driver-sqlserver/src/lib.rs crates/dblens-driver-sqlserver/src/types.rs crates/dblens-driver-sqlserver/tests/conformance.rs crates/dblens-driver/Cargo.toml crates/dblens-driver/src/params.rs crates/dblens-service/Cargo.toml crates/dblens-service/src/changes.rs crates/dblens-service/src/compare.rs crates/dblens-service/src/design.rs crates/dblens-service/src/document/job.rs crates/dblens-service/src/import/job.rs crates/dblens-service/src/metadata.rs crates/dblens-service/src/migrate/mod.rs crates/dblens-service/src/model/ops.rs crates/dblens-service/src/profile.rs crates/dblens-service/src/registry.rs crates/dblens-service/src/service.rs crates/dblens-service/src/session.rs crates/dblens-service/tests/live.rs crates/dblens-service/tests/migrate_live.rs vendor/PATCHES.md vendor/tiberius
git commit -m "feat: SQL Server 驱动（tiberius）、按库浏览元数据、M6b 功能拦截" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 4: 前端：连接、库树、T-SQL 编辑器、M6b 入口置灰

- `shared/api.ts`：`DriverId` 加 `sqlserver`；库名和架构合成一个字符串在各处传递：`joinCatalog`、`splitCatalog`、`schemaLabel`（给人看的 `库.架构`）、`sessionSchema`（SQL Server 的会话"当前 schema"是库）、`exportSource`（三段名的 SELECT）、`supportsWrites`；元数据方法带 `catalog`，`catalogs()`；改动集带 `catalog`（后端在任务 3）。
- 连接：产品列表、图标（MS，`#cc2927`）、认证方式（SQL 账号 / Windows 账号 / 当前 Windows 用户，附说明）、实例名、信任服务器证书；`needsCredentials`。
- 树：库节点 `d`、系统分组 `g` 带库；右键菜单中 M6b 的项置灰并提示（`writable`、`later` 写成函数声明，菜单在渲染时就调用它们）；DDL 对话框和只读 ER 图的标题用 `schemaLabel`；ER 图按架构名比较外键目标。
- 编辑器：`sql` 语言、`transactsql` 格式化；工具栏的库下拉框；执行前 `USE [库]`；数据标签、行数统计、SQL 文件对话框按库执行。
- `sql/tsql/parser.ts`（新）：按 `GO` 切批再交给 antlr-ng 生成的解析器检查，补全（三段名、别名、`[…]`）；`scripts/gen-tsql.mjs` 生成解析器到 `src/features/sql/tsql/gen/`（不提交，`.gitignore`），`package.json` 的 `gen`/`dev`/`build`/`test`/`typecheck` 先跑它；`grammars/tsql/` 放上游语法和来源说明。
- 比对、迁移、模型对话框的连接列表按 `supportsWrites` 过滤；词条（`connections`、`editor`、`tree`）。

**Files:**
- Create（下载，见下）：`web/grammars/tsql/TSqlLexer.g4`、`web/grammars/tsql/TSqlParser.g4`
- Modify: `.gitignore`
- Create: `web/grammars/tsql/README.md`
- Modify: `web/package.json`
- Create: `web/scripts/gen-tsql.mjs`
- Modify: `web/src/features/compare/CompareView.tsx`
- Modify: `web/src/features/connections/ConnectionDialog.tsx`
- Modify: `web/src/features/connections/form.test.ts`
- Modify: `web/src/features/connections/form.ts`
- Modify: `web/src/features/connections/products.ts`
- Modify: `web/src/features/data/DataView.tsx`
- Modify: `web/src/features/data/count.ts`
- Modify: `web/src/features/editor/QueryView.tsx`
- Modify: `web/src/features/editor/Toolbar.tsx`
- Modify: `web/src/features/editor/dialect.ts`
- Modify: `web/src/features/editor/monaco.ts`
- Modify: `web/src/features/editor/runner.ts`
- Modify: `web/src/features/er/ErView.tsx`
- Modify: `web/src/features/er/graph.test.ts`
- Modify: `web/src/features/er/graph.ts`
- Modify: `web/src/features/import/SqlFileDialog.tsx`
- Modify: `web/src/features/migrate/MigrateView.tsx`
- Modify: `web/src/features/model/ModelDialogs.tsx`
- Modify: `web/src/features/sql/completion.ts`
- Modify: `web/src/features/sql/dialects.test.ts`
- Modify: `web/src/features/sql/dialects.ts`
- Modify: `web/src/features/sql/functions.ts`
- Modify: `web/src/features/sql/metadata.test.ts`
- Modify: `web/src/features/sql/metadata.ts`
- Modify: `web/src/features/sql/parsers.ts`
- Modify: `web/src/features/sql/provider.ts`
- Create: `web/src/features/sql/tsql/parser.test.ts`
- Create: `web/src/features/sql/tsql/parser.ts`
- Modify: `web/src/features/sql/types.ts`
- Modify: `web/src/features/tree/DdlDialog.tsx`
- Modify: `web/src/features/tree/ObjectTree.tsx`
- Modify: `web/src/features/tree/nodes.test.ts`
- Modify: `web/src/features/tree/nodes.ts`
- Modify: `web/src/locales/en/connections.json`
- Modify: `web/src/locales/en/editor.json`
- Modify: `web/src/locales/en/tree.json`
- Modify: `web/src/locales/zh-CN/connections.json`
- Modify: `web/src/locales/zh-CN/editor.json`
- Modify: `web/src/locales/zh-CN/tree.json`
- Modify: `web/src/shared/api.test.ts`
- Modify: `web/src/shared/api.ts`
- Modify: `web/src/shared/dbIcons.test.ts`
- Modify: `web/src/shared/dbIcons.ts`

**Interfaces:**
- Consumes：任务 3 的接口。
- Produces：`joinCatalog(catalog, schema)`、`splitCatalog(schema) -> {catalog?, schema}`、`schemaLabel(schema)`、`sessionSchema(schema)`、`exportSource(schema, name)`、`supportsWrites(driver)`、`api.catalogs(id, filter?)`；`tsql: SqlParser`。

- [ ] **Step 1: 下载 T-SQL 语法**

```bash
R=https://raw.githubusercontent.com/antlr/grammars-v4/afbe4ed460d091afd7158b5cbd4ef63e64523e21/sql/tsql
mkdir -p web/grammars/tsql
curl -sSfL -o web/grammars/tsql/TSqlLexer.g4 $R/TSqlLexer.g4
curl -sSfL -o web/grammars/tsql/TSqlParser.g4 $R/TSqlParser.g4
sha256sum web/grammars/tsql/*.g4
```

Expected（LF 换行）：
```text
c2ea8c633c26d46347138bc00b9374ba6ab327ad99a48c22ba01cc494be018b0 web/grammars/tsql/TSqlLexer.g4
1a7e935763af4b8e81d40111f8c309d834378435f263ac67c2e5e900c7e648c2 web/grammars/tsql/TSqlParser.g4
```

- [ ] **Step 2: 修改 `web/src/features/connections/form.test.ts`**

```diff
--- a/web/src/features/connections/form.test.ts
+++ b/web/src/features/connections/form.test.ts
@@ -53,6 +53,19 @@ describe('连接表单', () => {
     expect(o.options).toEqual({ sid: 'ORCL' })
   })
 
+  it('SQL Server：认证方式、实例名、信任证书放进 options，当前 Windows 用户不要求用户名（M6a）', () => {
+    const f = { ...switchProduct(emptyForm(), 'sqlserver'), name: 'ms', sqlAuth: 'windows' as const, user: 'HOST\\u', instance: 'SQLEXPRESS', trustCert: true, tls: 'require' as const }
+    expect(f.port).toBe(1433)
+    const input = toProfileInput(f)
+    expect(input.options).toEqual({ auth: 'windows', instance: 'SQLEXPRESS', trust_server_certificate: 'true', tls: 'require' })
+    const back = fromProfile({ id: 'x', ...input }, false)
+    expect([back.sqlAuth, back.instance, back.trustCert, back.extra]).toEqual(['windows', 'SQLEXPRESS', true, ''])
+    const integrated = { ...f, sqlAuth: 'integrated' as const, user: '' }
+    expect(validate(integrated).user).toBeUndefined()
+    expect(validate({ ...f, user: '' }).user).toBeDefined()
+    expect(toProfileInput({ ...f, sqlAuth: 'sql' }).options?.auth).toBeUndefined()
+  })
+
   it('从已保存的配置还原表单，已知选项拆回各自的字段', () => {
     const p: Profile = {
       id: 'p1',
@@ -138,10 +151,10 @@ describe('连接表单', () => {
   })
 
   it('驱动名称、字段名、校验信息按调用时的界面语言翻译', async () => {
-    expect(DRIVER_IDS.map(driverLabel)).toEqual(['MySQL 系', 'PostgreSQL 系', 'Oracle', '达梦 DM'])
+    expect(DRIVER_IDS.map(driverLabel)).toEqual(['MySQL 系', 'PostgreSQL 系', 'Oracle', '达梦 DM', 'SQL Server'])
     try {
       await i18n.changeLanguage('en')
-      expect(DRIVER_IDS.map(driverLabel)).toEqual(['MySQL family', 'PostgreSQL family', 'Oracle', 'DM (Dameng)'])
+      expect(DRIVER_IDS.map(driverLabel)).toEqual(['MySQL family', 'PostgreSQL family', 'Oracle', 'DM (Dameng)', 'SQL Server'])
       expect(databaseLabel('oracle')).toBe('Service Name')
       expect(validate(emptyForm()).name).toBe('Enter a connection name')
       expect(validate({ ...emptyForm(), extra: 'bad' }).extra).toBe('Unrecognized line: bad (expected key=value)')
```

- [ ] **Step 3: 修改 `web/src/features/er/graph.test.ts`**

```diff
--- a/web/src/features/er/graph.test.ts
+++ b/web/src/features/er/graph.test.ts
@@ -74,6 +74,17 @@ describe('ER 图的节点和连线', () => {
     expect(orders.height).toBe(nodeHeight(6, 1))
   })
 
+  it('SQL Server 的图：schema 是"库 + 架构"，外键写的架构名照样算图内', () => {
+    const sample2 = sample().map((t) => ({
+      ...t,
+      table: { ...t.table, foreign_keys: t.table.foreign_keys.map((k) => (k.ref_schema ? k : { ...k, ref_schema: 'dbo' })) },
+    }))
+    const g = buildGraph(sample2, 'keys', 'salesdbo')
+    expect(g.edges.map((e) => e.id)).toEqual(['fk:orders:0', 'fk:orders:1', 'lk:items:0'])
+    expect(g.nodes[1].data.external).toEqual(['→ crm.regions'])
+    expect(g.nodes[0].data.columns.map((c) => c.name)).toEqual(['id', 'code'])
+  })
+
   it('主题图中没有的表列在节点上；只显示键列、只显示表名', () => {
     const [customers, orders] = sample()
     const g = buildGraph([orders], 'columns', 'public')
```

- [ ] **Step 4: 修改 `web/src/features/sql/dialects.test.ts`**

```diff
--- a/web/src/features/sql/dialects.test.ts
+++ b/web/src/features/sql/dialects.test.ts
@@ -26,6 +26,10 @@ describe('sqlDialect', () => {
     expect(sqlDialect('mysql', 'tidb', 'oracle')).toMatchObject({ dialect: 'mysql' })
   })
 
+  it('SQL Server 用 T-SQL 语法（标准档）', () => {
+    expect(sqlDialect('sqlserver', 'sqlserver')).toEqual({ dialect: 'tsql', strict: true, grammar: 'T-SQL' })
+  })
+
   it('变体未知时按驱动选解析器，按兼容档处理', () => {
     expect(sqlDialect('oracle', null)).toEqual({ dialect: 'plsql', strict: false, grammar: 'Oracle' })
     expect(sqlDialect('mysql')).toEqual({ dialect: 'mysql', strict: false, grammar: 'MySQL' })
```

- [ ] **Step 5: 修改 `web/src/features/sql/metadata.test.ts`**

```diff
--- a/web/src/features/sql/metadata.test.ts
+++ b/web/src/features/sql/metadata.test.ts
@@ -44,6 +44,21 @@ describe('metaReader', () => {
     expect(await m.schemas()).toEqual(['HR'])
   })
 
+  it('SQL Server：在会话的当前库中取架构和对象（M6a）', async () => {
+    const api = fakeApi()
+    const calls: string[] = []
+    api.schemas = (_id, _f, catalog) => {
+      calls.push(`schemas ${catalog}`)
+      return Promise.resolve([{ name: 'dbo', is_system: false }])
+    }
+    const m = metaReader(new QueryClient(), api, 'p1', 'sqlserver', 'sales')
+    await m.objects('dbo')
+    expect(api.calls).toEqual(['objects sales\u001fdbo tables', 'objects sales\u001fdbo views', 'synonyms sales\u001fdbo', 'sequences sales\u001fdbo'])
+    expect(await m.schemas()).toEqual(['dbo'])
+    expect(calls).toEqual(['schemas sales'])
+    expect(defaultSchema('sqlserver', 'sa', 'sales')).toBe('dbo')
+  })
+
   it('MySQL 不取同义词和序列', async () => {
     const api = fakeApi()
     await metaReader(new QueryClient(), api, 'p1', 'mysql').objects('db')
@@ -66,7 +81,7 @@ describe('metaReader', () => {
       const api = fakeApi()
       api.table = () => new Promise(() => {})
       api.schemas = () => Promise.reject(new Error('locked'))
-      const m = metaReader(new QueryClient(), api, 'p1', 'mysql', 100)
+      const m = metaReader(new QueryClient(), api, 'p1', 'mysql', null, 100)
       const cols = m.columns('db', 't')
       await vi.advanceTimersByTimeAsync(101)
       await expect(cols).resolves.toEqual([])
```

- [ ] **Step 6: 新建 `web/src/features/sql/tsql/parser.test.ts`**

```ts
import { describe, expect, it } from 'vitest'

import { tsql } from './parser'

/** `|` 表示光标。 */
function at(text: string) {
  const offset = text.indexOf('|')
  return tsql.suggest(text.replace('|', ''), offset)
}

describe('T-SQL 语法检查', () => {
  it('常见的 T-SQL 批没有问题', () => {
    const sql = `SELECT TOP (10) e.id, d.[name] FROM dbo.emp AS e JOIN [dbo].[dept] d ON d.id = e.dept_id WHERE e.salary > 100 ORDER BY e.id
GO
DECLARE @n INT = 5
INSERT INTO #t (id) VALUES (@n)
UPDATE t SET v = 1 OUTPUT inserted.id WHERE id = @n
GO 3
CREATE PROCEDURE dbo.p @a INT AS
BEGIN
  SET NOCOUNT ON;
  IF @a > 0 SELECT 1 ELSE PRINT N'负数'
END
GO
SELECT id FROM t ORDER BY id OFFSET 10 ROWS FETCH NEXT 5 ROWS ONLY;
MERGE INTO t USING s ON t.id = s.id WHEN MATCHED THEN UPDATE SET t.v = s.v;`
    expect(tsql.validate(sql, 'zh_CN')).toEqual([])
  })

  it('标出出错的位置；位置换算成全文中的行列', () => {
    const issues = tsql.validate('SELECT 1\nGO\nSELECT * FROM t WHERE', 'zh_CN')
    expect(issues).toHaveLength(1)
    expect(issues[0]).toMatchObject({ startLine: 3, incomplete: true })
    expect(tsql.validate('SELECT 1\nSELECT * FROM t WHERE a = = 1', 'zh_CN')[0]).toMatchObject({ startLine: 2, near: '=' })
  })
})

describe('T-SQL 分批', () => {
  it('单独一行的 GO 分批，字符串、注释、方括号里的 GO 不算', () => {
    const sql = "SELECT 'GO'\n/* GO */\nSELECT [GO]\n  go 2 -- 两次\nSELECT 3\nGOTO x"
    const second = sql.indexOf('SELECT 3')
    expect(tsql.statementAt(sql, 0)).toEqual({ start: 0, end: sql.indexOf('[GO]') + 4 })
    expect(tsql.statementAt(sql, second + 2)).toEqual({ start: second, end: sql.length })
  })

  it('一批里的多条语句是同一个执行单位', () => {
    const sql = 'SELECT 1\nSELECT 2\nGO\nSELECT 3'
    expect(tsql.statementAt(sql, sql.indexOf('2'))).toEqual({ start: 0, end: sql.indexOf('\nGO') })
  })
})

describe('T-SQL 补全', () => {
  it('FROM 后要表，表名前的限定名、三段名、别名', () => {
    const s = at('SELECT * FROM |')
    expect(s.want).toContain('table')
    const q = at('SELECT * FROM dbo.|')
    expect(q.qualifier).toEqual(['dbo'])
    const t = at('SELECT e.| FROM sales.dbo.emp e JOIN [hr].[dept] AS d ON 1 = 1')
    expect(t.qualifier).toEqual(['e'])
    expect(t.tables).toEqual([
      { schema: 'dbo', name: 'emp', alias: 'e' },
      { schema: 'hr', name: 'dept', alias: 'd' },
    ])
  })

  it('语句开头给关键字', () => {
    expect(at('|').keywords).toEqual(expect.arrayContaining(['SELECT', 'INSERT', 'DECLARE']))
  })
})
```

- [ ] **Step 7: 修改 `web/src/features/tree/nodes.test.ts`**

```diff
--- a/web/src/features/tree/nodes.test.ts
+++ b/web/src/features/tree/nodes.test.ts
@@ -1,5 +1,6 @@
 import { describe, expect, it } from 'vitest'
 
+import { joinCatalog } from '../../shared/api'
 import i18n from '../../shared/i18n'
 import {
   type NodeKey,
@@ -35,6 +36,26 @@ describe('对象树节点', () => {
     expect(() => decodeKey('x\u0001y')).toThrow('无法识别')
   })
 
+  it('SQL Server 的库节点和库中的架构（M6a）', () => {
+    const s = joinCatalog('销售', 'dbo')
+    for (const k of [
+      { type: 'd', profileId: 'p', catalog: '销售' },
+      { type: 'g', profileId: 'p', catalog: '销售' },
+      { type: 'g', profileId: 'p' },
+      { type: 's', profileId: 'p', schema: s },
+    ] as NodeKey[]) {
+      expect(decodeKey(encodeKey(k))).toEqual(k)
+    }
+    expect(categoriesFor('sqlserver').map((c) => c.kind)).toEqual(['tables', 'views', 'routines', 'sequences', 'synonyms'])
+    expect(qualifiedName({ type: 'o', profileId: 'p', schema: s, kind: 'tables', name: 't' })).toBe('销售.dbo.t')
+    const db: NodeKey = { type: 'd', profileId: 'p', catalog: '销售' }
+    expect(isWithin({ type: 's', profileId: 'p', schema: s }, db)).toBe(true)
+    expect(isWithin({ type: 's', profileId: 'p', schema: joinCatalog('其他', 'dbo') }, db)).toBe(false)
+    expect(isWithin({ type: 's', profileId: 'p', schema: s }, { type: 'g', profileId: 'p', catalog: '销售' })).toBe(true)
+    expect(isWithin({ type: 's', profileId: 'p', schema: s }, { type: 'g', profileId: 'p' })).toBe(false)
+    expect(isWithin(db, { type: 'c', profileId: 'p' })).toBe(true)
+  })
+
   it('过滤按名称或注释，不区分大小写', () => {
     expect(matches('', 'orders')).toBe(true)
     expect(matches('ORD', 'orders')).toBe(true)
```

- [ ] **Step 8: 修改 `web/src/shared/api.test.ts`**

```diff
--- a/web/src/shared/api.test.ts
+++ b/web/src/shared/api.test.ts
@@ -1,6 +1,17 @@
 import { describe, expect, it } from 'vitest'
 
-import { ApiError, type FetchFn, createApi, errorText } from './api'
+import {
+  ApiError,
+  type FetchFn,
+  createApi,
+  errorText,
+  exportSource,
+  joinCatalog,
+  schemaLabel,
+  sessionSchema,
+  splitCatalog,
+  supportsWrites,
+} from './api'
 import i18n from './i18n'
 
 function fakeFetch(status: number, body: unknown) {
@@ -54,6 +65,37 @@ describe('createApi', () => {
     })
   })
 
+  it('SQL Server 的合成 schema：架构进路径，库进 catalog 参数（M6a）', async () => {
+    const { fn, calls } = fakeFetch(200, [])
+    const api = createApi('tok', fn)
+    const s = joinCatalog('销售', 'dbo')
+    expect(splitCatalog(s)).toEqual({ catalog: '销售', schema: 'dbo' })
+    expect(splitCatalog('dbo')).toEqual({ schema: 'dbo' })
+    expect([schemaLabel(s), schemaLabel('hr'), sessionSchema(s), sessionSchema('hr')]).toEqual(['销售.dbo', 'hr', '销售', 'hr'])
+    await api.objects('p', s, 'tables')
+    expect(calls[0].url).toBe('/api/profiles/p/schemas/dbo/objects?kind=tables&catalog=%E9%94%80%E5%94%AE')
+    await api.catalogs('p')
+    expect(calls[1].url).toBe('/api/profiles/p/catalogs')
+    await api.schemas('p', undefined, 'master')
+    expect(calls[2].url).toBe('/api/profiles/p/schemas?catalog=master')
+    await api.dataSql('p', s, { object: 't' }).catch(() => {})
+    expect(calls[3].url).toBe('/api/profiles/p/schemas/dbo/data-sql?catalog=%E9%94%80%E5%94%AE')
+    await api.previewChanges('p', { schema: s, table: 't', changes: [] })
+    expect(JSON.parse(calls[4].init?.body as string)).toEqual({ schema: 'dbo', catalog: '销售', table: 't', changes: [] })
+    await api.setSchema(1, s).catch(() => {})
+    expect(JSON.parse(calls[5].init?.body as string)).toEqual({ schema: '销售' })
+    await api.refresh('p', s)
+    expect(JSON.parse(calls[6].init?.body as string)).toEqual({ schema: 'dbo' })
+  })
+
+  it('导出 SQL Server 别的库中的表按三段名写成查询', () => {
+    expect(exportSource('hr', 't')).toEqual({ data: { schema: 'hr', object: 't', filter: undefined, order: undefined } })
+    expect(exportSource(joinCatalog('a]b', 'dbo'), 'order', 'id > 1', { column: 'id', desc: true })).toEqual({
+      sql: 'SELECT * FROM [a]]b].[dbo].[order] WHERE (id > 1) ORDER BY [id] DESC',
+    })
+    expect([supportsWrites('sqlserver'), supportsWrites('postgres')]).toEqual([false, true])
+  })
+
   it('204 没有响应体', async () => {
     const { fn, calls } = fakeFetch(204, null)
     await expect(createApi('tok', fn).deleteProfile('a/b')).resolves.toBeNull()
```

- [ ] **Step 9: 修改 `web/src/shared/dbIcons.test.ts`**

```diff
--- a/web/src/shared/dbIcons.test.ts
+++ b/web/src/shared/dbIcons.test.ts
@@ -16,6 +16,7 @@ const VARIANTS = [
   'highgo',
   'oracle',
   'dameng',
+  'sqlserver',
 ]
 
 describe('数据库图标', () => {
```

- [ ] **Step 10: 修改 `.gitignore`**

```diff
--- a/.gitignore
+++ b/.gitignore
@@ -9,6 +9,8 @@ node_modules/
 /web/dist/
 # 生成的 PL/SQL 解析器（web/scripts/gen-plsql.mjs）
 /web/src/features/sql/plsql/gen/
+# 生成的 T-SQL 解析器（web/scripts/gen-tsql.mjs）
+/web/src/features/sql/tsql/gen/
 # tauri-build 生成的权限描述
 /crates/dblens-desktop/gen/
 # tauri-build 按 build.rs 中的命令列表自动生成的权限定义
```

- [ ] **Step 11: 新建 `web/grammars/tsql/README.md`**

```markdown
# T-SQL 语法（SQL Server）

来源：[antlr/grammars-v4](https://github.com/antlr/grammars-v4) 的 `sql/tsql`，提交 `afbe4ed460d091afd7158b5cbd4ef63e64523e21`（2026-10-17，与 PL/SQL 语法相同；`sql/tsql` 最后一次改动是 2026-07-05）。许可证 MIT（见 `.g4` 文件头）。

| 文件 | 上游路径 |
|---|---|
| `TSqlLexer.g4`、`TSqlParser.g4` | `sql/tsql/` |

这里的文件与上游一致，不直接修改；语法与目标语言无关，没有基类文件。`web/scripts/gen-tsql.mjs` 用 `antlr-ng` 生成 TypeScript，输出到 `src/features/sql/tsql/gen/`（不提交，`pnpm dev` / `build` / `test` / `typecheck` 前自动生成，输入不变时跳过）。

更新上游版本：按依赖原则（只取发布满 7 天的提交），替换这两个文件并更新上面的提交号。
```

- [ ] **Step 12: 修改 `web/package.json`**

```diff
--- a/web/package.json
+++ b/web/package.json
@@ -5,11 +5,11 @@
   "type": "module",
   "packageManager": "pnpm@11.8.0",
   "scripts": {
-    "gen": "node scripts/gen-plsql.mjs",
-    "dev": "node scripts/gen-plsql.mjs && vite",
-    "build": "node scripts/gen-plsql.mjs && tsc --noEmit && vite build",
-    "test": "node scripts/gen-plsql.mjs && vitest run",
-    "typecheck": "node scripts/gen-plsql.mjs && tsc --noEmit",
+    "gen": "node scripts/gen-plsql.mjs && node scripts/gen-tsql.mjs",
+    "dev": "node scripts/gen-plsql.mjs && node scripts/gen-tsql.mjs && vite",
+    "build": "node scripts/gen-plsql.mjs && node scripts/gen-tsql.mjs && tsc --noEmit && vite build",
+    "test": "node scripts/gen-plsql.mjs && node scripts/gen-tsql.mjs && vitest run",
+    "typecheck": "node scripts/gen-plsql.mjs && node scripts/gen-tsql.mjs && tsc --noEmit",
     "tauri": "cd .. && tauri",
     "package": "cd .. && tauri build --config crates/dblens-desktop/tauri.bundle.json"
   },
```

- [ ] **Step 13: 新建 `web/scripts/gen-tsql.mjs`**

```js
// 生成 T-SQL 解析器（SQL Server；M6a 设计文档第 8 节）：grammars/tsql 中的上游语法，原样用 antlr-ng 生成 TypeScript
// 到 src/features/sql/tsql/gen/。生成结果不提交；输入不变时跳过。
//
// 用法：node scripts/gen-tsql.mjs [--force]

import { execFileSync } from 'node:child_process'
import { createHash } from 'node:crypto'
import { existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const web = join(dirname(fileURLToPath(import.meta.url)), '..')
const src = join(web, 'grammars/tsql')
const out = join(web, 'src/features/sql/tsql/gen')
const antlrNg = join(web, 'node_modules/antlr-ng')

const FILES = ['TSqlLexer.g4', 'TSqlParser.g4']

function inputsHash() {
  const h = createHash('sha256')
  h.update(readFileSync(fileURLToPath(import.meta.url)))
  h.update(readFileSync(join(antlrNg, 'package.json')))
  for (const f of FILES) h.update(readFileSync(join(src, f)))
  return h.digest('hex')
}

function main() {
  const hash = inputsHash()
  const stamp = join(out, '.inputs-hash')
  if (!process.argv.includes('--force') && existsSync(stamp) && readFileSync(stamp, 'utf8') === hash) return
  const started = Date.now()
  rmSync(out, { recursive: true, force: true })
  mkdirSync(out, { recursive: true })
  for (const f of FILES) writeFileSync(join(out, f), readFileSync(join(src, f), 'utf8'))
  execFileSync(
    process.execPath,
    [join(antlrNg, 'dist/cli/runner.js'), '-Dlanguage=TypeScript', '--generate-listener', 'false', '-o', '.', ...FILES],
    { cwd: out, stdio: ['ignore', 'ignore', 'pipe'] },
  )
  // 生成的代码不按本项目的 tsconfig 检查（未使用的变量等）
  for (const f of ['TSqlLexer.ts', 'TSqlParser.ts']) {
    const p = join(out, f)
    writeFileSync(p, `// @ts-nocheck\n${readFileSync(p, 'utf8')}`)
  }
  writeFileSync(stamp, hash)
  console.log(`已生成 T-SQL 解析器（${((Date.now() - started) / 1000).toFixed(1)} 秒）`)
}

if (process.argv[1] === fileURLToPath(import.meta.url)) main()
```

- [ ] **Step 14: 修改 `web/src/features/compare/CompareView.tsx`**

```diff
--- a/web/src/features/compare/CompareView.tsx
+++ b/web/src/features/compare/CompareView.tsx
@@ -21,6 +21,7 @@ import type {
   SyncRequest,
   TableStatus,
 } from '../../shared/api'
+import { supportsWrites } from '../../shared/api'
 import { desktop } from '../../shared/shell'
 import { withVault } from '../connections/vault'
 import { useQueryActions } from '../editor/actions'
@@ -115,7 +116,7 @@ function SidePicker({ label, value, profiles, onChange }: {
           placeholder={t('model.compare.pickProfile')}
           value={value?.kind === 'database' ? value.profile : undefined}
           onChange={(p) => onChange({ kind: 'database', profile: p, schema: '' })}
-          options={profiles.map((p) => ({ value: p.id, label: p.name }))}
+          options={profiles.filter((p) => supportsWrites(p.driver)).map((p) => ({ value: p.id, label: p.name }))}
         />
       )}
       <Select
```

- [ ] **Step 15: 修改 `web/src/features/connections/ConnectionDialog.tsx`**

```diff
--- a/web/src/features/connections/ConnectionDialog.tsx
+++ b/web/src/features/connections/ConnectionDialog.tsx
@@ -18,6 +18,7 @@ import {
   driverNote,
   emptyForm,
   fromProfile,
+  needsCredentials,
   serverText,
   supportsTls,
   serverNotes,
@@ -165,10 +166,31 @@ export default function ConnectionDialog() {
           <InputNumber style={{ width: 110 }} min={1} max={65535} value={form.port} onChange={(v) => set('port', v ?? 0)} />
         </Space.Compact>
       </Field>
-      <Field label={t('connections.general.user')} error={errors.user}>
-        <Input value={form.user} onChange={(e) => set('user', e.target.value)} autoComplete="off" />
-      </Field>
-      <Field label={t('connections.general.password')}>
+      {form.driver === 'sqlserver' && (
+        <Field label={t('connections.sqlserver.auth')}>
+          <Radio.Group
+            value={form.sqlAuth}
+            onChange={(e) => set('sqlAuth', e.target.value)}
+            options={(['sql', 'windows', 'integrated'] as const).map((value) => ({ value, label: t(`connections.sqlserver.authOption.${value}`) }))}
+          />
+          {form.sqlAuth !== 'sql' && (
+            <div className="muted" style={{ marginTop: 4 }}>
+              {t(form.sqlAuth === 'windows' ? 'connections.sqlserver.windowsHint' : 'connections.sqlserver.integratedHint')}
+            </div>
+          )}
+        </Field>
+      )}
+      {needsCredentials(form) && (
+        <Field label={t('connections.general.user')} error={errors.user}>
+          <Input
+            value={form.user}
+            onChange={(e) => set('user', e.target.value)}
+            autoComplete="off"
+            placeholder={form.driver === 'sqlserver' && form.sqlAuth === 'windows' ? 'HOST\\user' : undefined}
+          />
+        </Field>
+      )}
+      {needsCredentials(form) && <Field label={t('connections.general.password')}>
         <Space.Compact style={{ width: '100%' }}>
           <Input.Password
             value={form.password}
@@ -180,7 +202,7 @@ export default function ConnectionDialog() {
         <Checkbox checked={form.savePassword} onChange={(e) => set('savePassword', e.target.checked)} style={{ marginTop: 4 }}>
           {t('connections.general.savePassword')}
         </Checkbox>
-      </Field>
+      </Field>}
       <Field label={databaseLabel(form.driver)} error={errors.database}>
         <Input
           value={form.database}
@@ -224,6 +246,13 @@ export default function ConnectionDialog() {
           <Input value={form.tlsCa} onChange={(e) => set('tlsCa', e.target.value)} placeholder={t('connections.tls.caPlaceholder')} />
         </Field>
       )}
+      {form.driver === 'sqlserver' && (
+        <Field label={t('connections.sqlserver.trustCert')}>
+          <Checkbox checked={form.trustCert} onChange={(e) => set('trustCert', e.target.checked)}>
+            {t('connections.sqlserver.trustCertHint')}
+          </Checkbox>
+        </Field>
+      )}
     </>
   ) : (
     <Alert type="info" showIcon message={t('connections.tls.unsupported', { driver: driverLabel(form.driver) })} />
@@ -234,6 +263,11 @@ export default function ConnectionDialog() {
       <Field label={t('connections.advanced.timeout')} error={errors.timeoutSecs}>
         <InputNumber min={1} max={600} value={form.timeoutSecs} onChange={(v) => set('timeoutSecs', v ?? 0)} />
       </Field>
+      {form.driver === 'sqlserver' && (
+        <Field label={t('connections.sqlserver.instance')}>
+          <Input value={form.instance} onChange={(e) => set('instance', e.target.value)} placeholder={t('connections.sqlserver.instancePlaceholder')} />
+        </Field>
+      )}
       {form.driver === 'oracle' && (
         <Field label="SID">
           <Input value={form.oracleSid} onChange={(e) => set('oracleSid', e.target.value)} placeholder={t('connections.advanced.sidPlaceholder')} />
```

- [ ] **Step 16: 修改 `web/src/features/connections/form.ts`**

```diff
--- a/web/src/features/connections/form.ts
+++ b/web/src/features/connections/form.ts
@@ -7,6 +7,9 @@ import { type ProductId, baseProduct, product, productLabel, productOf } from '.
 
 export type TlsMode = 'disable' | 'require' | 'verify'
 
+/** SQL Server 的认证方式：SQL 账号、指定 Windows 账号、当前 Windows 用户（服务端进程的身份）。 */
+export type SqlServerAuth = 'sql' | 'windows' | 'integrated'
+
 export interface ConnectionForm {
   name: string
   /** 选的产品；驱动由产品决定。 */
@@ -26,13 +29,17 @@ export interface ConnectionForm {
   color: string
   tls: TlsMode
   tlsCa: string
+  /** 只用于 SQL Server：认证方式、命名实例、信任服务器证书。 */
+  sqlAuth: SqlServerAuth
+  instance: string
+  trustCert: boolean
   timeoutSecs: number
   /** 其他驱动参数，一行一个 `键=值`。 */
   extra: string
 }
 
 /** 新建连接时可选的驱动，按界面上的顺序。 */
-export const DRIVER_IDS: DriverId[] = ['mysql', 'postgres', 'oracle', 'dameng']
+export const DRIVER_IDS: DriverId[] = ['mysql', 'postgres', 'oracle', 'dameng', 'sqlserver']
 
 /** 驱动的显示名称（如「MySQL 系」）。 */
 export function driverLabel(driver: DriverId): string {
@@ -44,16 +51,21 @@ export function driverNote(driver: DriverId): string {
   return t(`connections.driver.${driver}.note`)
 }
 
-export const DEFAULT_PORTS: Record<DriverId, number> = { mysql: 3306, postgres: 5432, oracle: 1521, dameng: 5236 }
+export const DEFAULT_PORTS: Record<DriverId, number> = { mysql: 3306, postgres: 5432, oracle: 1521, dameng: 5236, sqlserver: 1433 }
 
 /** 「数据库」一栏在各驱动下的含义。 */
 export function databaseLabel(driver: DriverId): string {
   return t(`connections.databaseLabel.${driver}`)
 }
 
-/** 只有 MySQL 系、PG 系支持 TLS 选项（M0-4b）。 */
+/** MySQL 系、PG 系（M0-4b）和 SQL Server 支持 TLS 选项。 */
 export function supportsTls(driver: DriverId): boolean {
-  return driver === 'mysql' || driver === 'postgres'
+  return driver === 'mysql' || driver === 'postgres' || driver === 'sqlserver'
+}
+
+/** 当前 Windows 用户认证不需要填用户名和密码。 */
+export function needsCredentials(f: Pick<ConnectionForm, 'driver' | 'sqlAuth'>): boolean {
+  return !(f.driver === 'sqlserver' && f.sqlAuth === 'integrated')
 }
 
 export function emptyForm(id: ProductId = 'mysql'): ConnectionForm {
@@ -73,6 +85,9 @@ export function emptyForm(id: ProductId = 'mysql'): ConnectionForm {
     color: '',
     tls: 'disable',
     tlsCa: '',
+    sqlAuth: 'sql',
+    instance: '',
+    trustCert: false,
     timeoutSecs: 10,
     extra: '',
   }
@@ -91,7 +106,7 @@ export function switchProduct(f: ConnectionForm, id: ProductId): ConnectionForm
   return { ...switchDriver(f, next.driver), product: id, port }
 }
 
-const KNOWN_OPTIONS = ['tls', 'tls_ca', 'sid']
+const KNOWN_OPTIONS = ['tls', 'tls_ca', 'sid', 'auth', 'instance', 'trust_server_certificate']
 
 export function fromProfile(p: Profile, hasPassword: boolean): ConnectionForm {
   const opts = p.options ?? {}
@@ -115,6 +130,9 @@ export function fromProfile(p: Profile, hasPassword: boolean): ConnectionForm {
     color: p.color ?? '',
     tls,
     tlsCa: opts.tls_ca ?? '',
+    sqlAuth: opts.auth === 'windows' || opts.auth === 'integrated' ? opts.auth : 'sql',
+    instance: opts.instance ?? '',
+    trustCert: opts.trust_server_certificate === 'true',
     timeoutSecs: p.connect_timeout_secs ?? 10,
     extra,
   }
@@ -131,6 +149,11 @@ export function toProfileInput(f: ConnectionForm): ProfileInput {
     if (f.tls === 'verify' && f.tlsCa.trim()) options.tls_ca = f.tlsCa.trim()
   }
   if (f.driver === 'oracle' && f.oracleSid.trim()) options.sid = f.oracleSid.trim()
+  if (f.driver === 'sqlserver') {
+    if (f.sqlAuth !== 'sql') options.auth = f.sqlAuth
+    if (f.instance.trim()) options.instance = f.instance.trim()
+    if (f.trustCert) options.trust_server_certificate = 'true'
+  }
   return {
     name: f.name.trim(),
     driver: f.driver,
@@ -152,7 +175,7 @@ export function validate(f: ConnectionForm): Partial<Record<keyof ConnectionForm
   if (!f.name.trim()) e.name = t('connections.validate.nameRequired')
   if (!f.host.trim()) e.host = t('connections.validate.hostRequired')
   if (!Number.isInteger(f.port) || f.port < 1 || f.port > 65535) e.port = t('connections.validate.portRange')
-  if (!f.user.trim()) e.user = t('connections.validate.userRequired')
+  if (needsCredentials(f) && !f.user.trim()) e.user = t('connections.validate.userRequired')
   if (f.driver === 'oracle' && !f.database.trim() && !f.oracleSid.trim()) {
     e.database = t('connections.validate.serviceOrSidRequired')
   }
```

- [ ] **Step 17: 修改 `web/src/features/connections/products.ts`**

```diff
--- a/web/src/features/connections/products.ts
+++ b/web/src/features/connections/products.ts
@@ -14,6 +14,7 @@ export type ProductId =
   | 'opengauss'
   | 'oracle'
   | 'dameng'
+  | 'sqlserver'
 
 export interface Product {
   id: ProductId
@@ -32,6 +33,7 @@ export const PRODUCTS: Product[] = [
   { id: 'opengauss', driver: 'postgres', port: 5432 },
   { id: 'oracle', driver: 'oracle', port: 1521 },
   { id: 'dameng', driver: 'dameng', port: 5236 },
+  { id: 'sqlserver', driver: 'sqlserver', port: 1433 },
 ]
 
 export function product(id: ProductId): Product {
```

- [ ] **Step 18: 修改 `web/src/features/data/DataView.tsx`**

```diff
--- a/web/src/features/data/DataView.tsx
+++ b/web/src/features/data/DataView.tsx
@@ -23,7 +23,7 @@ import { useTranslation } from 'react-i18next'
 
 import { useApi } from '../../app/context'
 import DbIcon from '../../shared/DbIcon'
-import { type ChangeSet, type Profile, errorText } from '../../shared/api'
+import { type ChangeSet, type Profile, errorText, exportSource, schemaLabel, supportsWrites } from '../../shared/api'
 import ResultsPanel from '../results/ResultsPanel'
 import { useQueryActions } from '../editor/actions'
 import { runnerFor, useTabRuntime } from '../editor/runner'
@@ -120,7 +120,7 @@ export default function DataView({ tab, profiles }: { tab: QueryTab; profiles: P
     setCounting(true)
     try {
       const sql = await api.dataSql(tab.profileId, data.schema, { object: data.object, filter: data.filter, count: true })
-      setTotal(await countRows(api, tab.profileId, sql))
+      setTotal(await countRows(api, tab.profileId, sql, data.schema))
     } catch (e) {
       message.error((e as Error).message)
     } finally {
@@ -190,7 +190,7 @@ export default function DataView({ tab, profiles }: { tab: QueryTab; profiles: P
       <div className="row toolbar">
         {profile && <DbIcon driver={profile.driver} variant={rt.session?.variant ?? profile.variant} />}
         <span>{profile?.name}</span>
-        <span className="muted">{data.schema}</span>
+        <span className="muted">{schemaLabel(data.schema)}</span>
         <span className="sep" />
         <Input
           size="small"
@@ -226,9 +226,9 @@ export default function DataView({ tab, profiles }: { tab: QueryTab; profiles: P
           onClick={() =>
             useExportDialog.getState().open({
               profileId: tab.profileId,
-              source: { data: { schema: data.schema, object: data.object, filter: data.filter, order: data.order } },
+              source: exportSource(data.schema, data.object, data.filter, data.order),
               baseName: data.object,
-              table: `${data.schema}.${data.object}`,
+              table: `${schemaLabel(data.schema)}.${data.object}`,
               inTransaction: false,
             })
           }
@@ -239,7 +239,7 @@ export default function DataView({ tab, profiles }: { tab: QueryTab; profiles: P
           <Button
             size="small"
             icon={<IconUpload size={14} />}
-            disabled={!profile}
+            disabled={!profile || !supportsWrites(profile.driver)}
             onClick={() =>
               useImportDialogs
                 .getState()
@@ -253,7 +253,7 @@ export default function DataView({ tab, profiles }: { tab: QueryTab; profiles: P
           <Button
             size="small"
             icon={<IconTableOptions size={14} />}
-            disabled={!profile}
+            disabled={!profile || !supportsWrites(profile.driver)}
             onClick={() => useTabs.getState().openDesign(tab.profileId, { schema: data.schema, table: data.object })}
           >
             {t('editor.data.design')}
```

- [ ] **Step 19: 修改 `web/src/features/data/count.ts`**

```diff
--- a/web/src/features/data/count.ts
+++ b/web/src/features/data/count.ts
@@ -1,13 +1,16 @@
 // 数据标签的"统计行数"：另开一个会话执行 COUNT(*)，取第一行第一列后关闭。
 // 不用标签自己的会话：它可能还有没读完的结果（服务端在等续取），会话正忙。
 
-import { type Api, errorText } from '../../shared/api'
+import { type Api, errorText, splitCatalog } from '../../shared/api'
 import { ExecSocket, cellText } from '../../shared/exec'
 import { t } from '../../shared/i18n'
 import { withVault } from '../connections/vault'
 
-export async function countRows(api: Api, profileId: string, sql: string): Promise<string> {
+/** `schema`：对象所在的 schema；SQL Server 的合成 schema 先把会话切到其中的库。 */
+export async function countRows(api: Api, profileId: string, sql: string, schema?: string): Promise<string> {
   const session = await withVault(api, () => api.openSession(profileId))
+  const catalog = schema === undefined ? undefined : splitCatalog(schema).catalog
+  if (catalog !== undefined) await api.setSchema(session.id, schema!)
   let socket: ExecSocket | null = null
   try {
     return await new Promise<string>((resolve, reject) => {
```

- [ ] **Step 20: 修改 `web/src/features/editor/QueryView.tsx`**

```diff
--- a/web/src/features/editor/QueryView.tsx
+++ b/web/src/features/editor/QueryView.tsx
@@ -162,7 +162,12 @@ export default function QueryView({ tab, profiles }: { tab: QueryTab; profiles:
       .catch(() => {})
       .then(() => qc.invalidateQueries({ queryKey: metaKey(profile.id) }))
   }, [rt.running, rt.exec, profile, api, qc])
-  const meta = useMemo(() => (profile ? metaReader(qc, api, profile.id, profile.driver) : null), [qc, api, profile?.id, profile?.driver])
+  // SQL Server：会话的当前 schema 是库，补全在这个库的架构中查找
+  const catalog = profile?.driver === 'sqlserver' ? (rt.session?.schema ?? profile.database) : null
+  const meta = useMemo(
+    () => (profile ? metaReader(qc, api, profile.id, profile.driver, catalog) : null),
+    [qc, api, profile?.id, profile?.driver, catalog],
+  )
   const sql = useMemo(
     () =>
       profile && meta
@@ -170,7 +175,10 @@ export default function QueryView({ tab, profiles }: { tab: QueryTab; profiles:
             driver: profile.driver,
             variant: rt.session?.variant ?? profile.variant,
             compat: rt.session?.compat,
-            schema: rt.session?.schema ?? defaultSchema(profile.driver, profile.user, profile.database),
+            schema:
+              profile.driver === 'sqlserver'
+                ? defaultSchema(profile.driver, profile.user, profile.database)
+                : (rt.session?.schema ?? defaultSchema(profile.driver, profile.user, profile.database)),
             meta,
           }
         : null,
```

- [ ] **Step 21: 修改 `web/src/features/editor/Toolbar.tsx`**

```diff
--- a/web/src/features/editor/Toolbar.tsx
+++ b/web/src/features/editor/Toolbar.tsx
@@ -56,9 +56,11 @@ export default function Toolbar({ tab, rt, runner, profiles }: ToolbarProps) {
   const api = useApi()
   const { t } = useTranslation()
   const { message, modal } = App.useApp()
+  const driver = profiles.find((p) => p.id === tab.profileId)?.driver
+  // SQL Server 的会话按库切换（USE），下拉框列出库
   const { data: schemas } = useQuery({
-    queryKey: ['schema-list', tab.profileId],
-    queryFn: () => api.schemas(tab.profileId),
+    queryKey: ['schema-list', tab.profileId, driver],
+    queryFn: () => (driver === 'sqlserver' ? api.catalogs(tab.profileId) : api.schemas(tab.profileId)),
   })
   const guard = (p: Promise<unknown>) => p.catch((e: unknown) => message.error(e instanceof Error ? e.message : String(e)))
   const s = rt.session
@@ -115,7 +117,7 @@ export default function Toolbar({ tab, rt, runner, profiles }: ToolbarProps) {
         size="small"
         variant="filled"
         showSearch
-        placeholder="schema"
+        placeholder={driver === 'sqlserver' ? t('editor.toolbar.database') : 'schema'}
         value={s?.schema ?? undefined}
         onChange={(v: string) => guard(runner.setSchema(v))}
         disabled={rt.running}
```

- [ ] **Step 22: 修改 `web/src/features/editor/dialect.ts`**

```diff
--- a/web/src/features/editor/dialect.ts
+++ b/web/src/features/editor/dialect.ts
@@ -6,8 +6,11 @@ import { type FormatOptionsWithLanguage, format } from 'sql-formatter'
 import type { Compat, DriverId } from '../../shared/api'
 import { t } from '../../shared/i18n'
 
-/** Monaco 的语言：MySQL 系、PG 系用 Monaco 自带的规则，Oracle、达梦和 PG 系的 Oracle 兼容模式用 plsql（monaco.ts 注册）。 */
-export function editorLanguage(driver: DriverId, compat?: Compat | null): 'mysql' | 'pgsql' | 'plsql' {
+/**
+ * Monaco 的语言：MySQL 系、PG 系用 Monaco 自带的规则，Oracle、达梦和 PG 系的 Oracle 兼容模式用 plsql（monaco.ts 注册），
+ * SQL Server 用 Monaco 自带的 sql（按 T-SQL 的关键字）。
+ */
+export function editorLanguage(driver: DriverId, compat?: Compat | null): 'mysql' | 'pgsql' | 'plsql' | 'sql' {
   if (driver === 'postgres' && compat === 'oracle') return 'plsql'
   switch (driver) {
     case 'mysql':
@@ -17,6 +20,8 @@ export function editorLanguage(driver: DriverId, compat?: Compat | null): 'mysql
     case 'oracle':
     case 'dameng':
       return 'plsql'
+    case 'sqlserver':
+      return 'sql'
   }
 }
 
@@ -38,6 +43,8 @@ export function formatterLanguage(driver: DriverId, variant?: string | null): Fo
     case 'oracle':
     case 'dameng':
       return 'plsql'
+    case 'sqlserver':
+      return 'transactsql'
   }
 }
 
@@ -63,6 +70,7 @@ const VARIANT_NAMES: Record<string, string> = {
   gaussdb: 'GaussDB',
   highgo: 'HighGo',
   oracle: 'Oracle',
+  sqlserver: 'SQL Server',
 }
 
 /** 变体的简短名称（状态栏、对象树用）；达梦的名称随界面语言。不认识的变体返回 undefined。 */
```

- [ ] **Step 23: 修改 `web/src/features/editor/monaco.ts`**

```diff
--- a/web/src/features/editor/monaco.ts
+++ b/web/src/features/editor/monaco.ts
@@ -1,5 +1,5 @@
 // Monaco 的初始化（M1 设计文档 2.1 节）：从本地打包的 monaco-editor 加载（不走 CDN，离线可用），
-// 只用编辑器自身的 worker；注册 PL/SQL 高亮（Oracle、达梦）和跟随界面主题的两套配色。
+// 只用编辑器自身的 worker；注册 PL/SQL 高亮（Oracle、达梦）和跟随界面主题的两套配色。SQL Server 用自带的 sql 语言。
 // 只由懒加载的工作区引入，不进入首屏。
 
 import { loader } from '@monaco-editor/react'
```

- [ ] **Step 24: 修改 `web/src/features/editor/runner.ts`**

```diff
--- a/web/src/features/editor/runner.ts
+++ b/web/src/features/editor/runner.ts
@@ -6,7 +6,7 @@
 
 import { create } from 'zustand'
 
-import { type Api, ApiError, type SessionInfo } from '../../shared/api'
+import { type Api, ApiError, type SessionInfo, splitCatalog } from '../../shared/api'
 import {
   DEFAULT_MAX_ROWS,
   type ExecOptions,
@@ -144,6 +144,9 @@ export class Runner {
       const tab = useTabs.getState().tabs.find((x) => x.id === this.tabId)
       if (!tab) throw new Error(t('editor.runner.tabClosed'))
       session = await withVault(this.api, () => this.api.openSession(tab.profileId))
+      // SQL Server 的数据标签：对象可能在别的库中，先把会话切过去
+      const catalog = tab.data && splitCatalog(tab.data.schema).catalog
+      if (catalog !== undefined) session = await this.api.setSchema(session.id, tab.data!.schema)
     }
     this.patch({ session, error: null })
     this.socket = new ExecSocket(
```

- [ ] **Step 25: 修改 `web/src/features/er/ErView.tsx`**

```diff
--- a/web/src/features/er/ErView.tsx
+++ b/web/src/features/er/ErView.tsx
@@ -31,7 +31,7 @@ import { type KeyboardEvent, useCallback, useEffect, useMemo, useState } from 'r
 import { useTranslation } from 'react-i18next'
 
 import { useApi } from '../../app/context'
-import { type DesignTableModel, type ErDiagram, type ErDisplay, type ErPosition, type ErTable, DEFAULT_DIAGRAM } from '../../shared/api'
+import { type DesignTableModel, type ErDiagram, type ErDisplay, type ErPosition, type ErTable, DEFAULT_DIAGRAM, schemaLabel } from '../../shared/api'
 import { useSetting } from '../../shared/settings'
 import { desktop } from '../../shared/shell'
 import { THEME_KEY, parseThemePref, resolveTheme, useSystemDark } from '../../shared/theme'
@@ -315,7 +315,7 @@ function Canvas({ tab }: { tab: QueryTab }) {
   const isTopic = tableList !== 'all'
   const title = isModel
     ? `${modelQ.data?.info.name ?? ''} / ${schema} · ${diagramName === DEFAULT_DIAGRAM ? t('er.defaultDiagram') : diagramName}`
-    : `${schema} · ${t('er.readOnly')}`
+    : `${schemaLabel(schema)} · ${t('er.readOnly')}`
   return (
     <div className="er-view" data-er-tab={tab.id} onKeyDown={onKeyDown} tabIndex={-1}>
       <div className="row toolbar" style={{ gap: 6, flexWrap: 'wrap' }}>
```

- [ ] **Step 26: 修改 `web/src/features/er/graph.ts`**

```diff
--- a/web/src/features/er/graph.ts
+++ b/web/src/features/er/graph.ts
@@ -3,7 +3,7 @@
 
 import { graphlib, layout as dagreLayout } from '@dagrejs/dagre'
 
-import type { DesignTableModel, ErDisplay, ErPosition, ErTable } from '../../shared/api'
+import { type DesignTableModel, type ErDisplay, type ErPosition, type ErTable, splitCatalog } from '../../shared/api'
 
 export const NODE_WIDTH = 240
 export const HEADER_HEIGHT = 30
@@ -85,8 +85,10 @@ export function nodeHeight(columns: number, external: number): number {
   return HEADER_HEIGHT + columns * ROW_HEIGHT + external * EXTERNAL_HEIGHT + 6
 }
 
-/** 表和外键换算成节点和连线。`schema` 是图所在的 schema（外键不写 schema 时指它）。 */
-export function buildGraph(tables: ErTable[], display: ErDisplay, schema: string): Graph {
+/** 表和外键换算成节点和连线。`schema` 是图所在的 schema（外键不写 schema 时指它）；SQL Server 的"库 + 架构"
+ * 只取架构名与外键比较。 */
+export function buildGraph(tables: ErTable[], display: ErDisplay, graphSchema: string): Graph {
+  const schema = splitCatalog(graphSchema).schema
   const names = new Set(tables.map((t) => t.table.name))
   const nodes: ErNode[] = []
   const edges: ErEdge[] = []
```

- [ ] **Step 27: 修改 `web/src/features/import/SqlFileDialog.tsx`**

```diff
--- a/web/src/features/import/SqlFileDialog.tsx
+++ b/web/src/features/import/SqlFileDialog.tsx
@@ -7,7 +7,7 @@ import { useEffect, useState } from 'react'
 import { useTranslation } from 'react-i18next'
 
 import { useApi } from '../../app/context'
-import type { TextEncoding } from '../../shared/api'
+import { type TextEncoding, sessionSchema } from '../../shared/api'
 import { withVault } from '../connections/vault'
 import FileField, { type PickedFile } from './FileField'
 import { useImportDialogs } from './dialog'
@@ -31,16 +31,16 @@ export default function SqlFileDialog() {
   useEffect(() => {
     if (!target) return
     setFile(null)
-    setSchema(target.schema)
+    setSchema(target.schema === null ? null : sessionSchema(target.schema))
     setError(null)
   }, [target])
 
   const { data: profiles = [] } = useQuery({ queryKey: ['profiles'], queryFn: () => api.profiles() })
   const profile = profiles.find((p) => p.id === profileId)
   const schemas = useQuery({
-    queryKey: ['schemas', profileId],
-    queryFn: () => withVault(api, () => api.schemas(profileId)),
-    enabled: !!target,
+    queryKey: ['schemas', profileId, profile?.driver],
+    queryFn: () => withVault(api, () => (profile?.driver === 'sqlserver' ? api.catalogs(profileId) : api.schemas(profileId))),
+    enabled: !!target && !!profile,
   })
 
   const submit = async () => {
```

- [ ] **Step 28: 修改 `web/src/features/migrate/MigrateView.tsx`**

```diff
--- a/web/src/features/migrate/MigrateView.tsx
+++ b/web/src/features/migrate/MigrateView.tsx
@@ -22,6 +22,7 @@ import {
   type Profile,
   type TaskStatus,
   errorText,
+  supportsWrites,
 } from '../../shared/api'
 import { desktop } from '../../shared/shell'
 import { withVault } from '../connections/vault'
@@ -69,7 +70,7 @@ function EndPicker({ label, value, profiles, onChange }: {
         placeholder={t('migrate.pickProfile')}
         value={value.profile || undefined}
         onChange={(profile) => onChange({ profile, schema: '' })}
-        options={profiles.map((p) => ({ value: p.id, label: p.name }))}
+        options={profiles.filter((p) => supportsWrites(p.driver)).map((p) => ({ value: p.id, label: p.name }))}
       />
       <Select
         size="small"
```

- [ ] **Step 29: 修改 `web/src/features/model/ModelDialogs.tsx`**

```diff
--- a/web/src/features/model/ModelDialogs.tsx
+++ b/web/src/features/model/ModelDialogs.tsx
@@ -9,7 +9,7 @@ import { useTranslation } from 'react-i18next'
 
 import { useApi } from '../../app/context'
 import { useUi } from '../../app/ui'
-import { type Compat, type TaskStatus, errorText } from '../../shared/api'
+import { type Compat, type TaskStatus, errorText, supportsWrites } from '../../shared/api'
 import { desktop } from '../../shared/shell'
 import { withVault } from '../connections/vault'
 import { PRODUCTS, type ProductId, productLabel } from '../connections/products'
@@ -204,7 +204,7 @@ function ReverseDialog() {
               setProfileId(v)
               setSchemas([])
             }}
-            options={profiles.map((p) => ({ value: p.id, label: p.name }))}
+            options={profiles.filter((p) => supportsWrites(p.driver)).map((p) => ({ value: p.id, label: p.name }))}
           />
         </Form.Item>
         <Form.Item label={t('model.dialog.schemas')}>
```

- [ ] **Step 30: 修改 `web/src/features/sql/completion.ts`**

```diff
--- a/web/src/features/sql/completion.ts
+++ b/web/src/features/sql/completion.ts
@@ -28,7 +28,7 @@ const ORDER: Record<ItemKind, string> = {
   keyword: '4',
 }
 
-/** 名称需要时加引号：含特殊字符、是 PL/SQL 中的非大写名称或 PG 中的非小写名称。 */
+/** 名称需要时加引号：含特殊字符、是 PL/SQL 中的非大写名称或 PG 中的非小写名称；SQL Server 用方括号。 */
 export function quoteIdent(name: string, d: Dialect): string {
   const plain = /^[A-Za-z_][A-Za-z0-9_$#]*$/.test(name)
   switch (d) {
@@ -38,6 +38,8 @@ export function quoteIdent(name: string, d: Dialect): string {
       return plain && name === name.toLowerCase() ? name : `"${name.replaceAll('"', '""')}"`
     case 'plsql':
       return plain && name === name.toUpperCase() ? name : `"${name.replaceAll('"', '""')}"`
+    case 'tsql':
+      return plain ? name : `[${name.replaceAll(']', ']]')}]`
   }
 }
 
```

- [ ] **Step 31: 修改 `web/src/features/sql/dialects.ts`**

```diff
--- a/web/src/features/sql/dialects.ts
+++ b/web/src/features/sql/dialects.ts
@@ -11,10 +11,10 @@ export interface SqlDialect {
   /** 标准档：语法错误按错误（红色）标出。 */
   strict: boolean
   /** 检查所用语法的名称，兼容档的提示中说明。 */
-  grammar: 'MySQL' | 'PostgreSQL' | 'Oracle'
+  grammar: 'MySQL' | 'PostgreSQL' | 'Oracle' | 'T-SQL'
 }
 
-const STRICT = new Set(['mysql', 'mariadb', 'postgres', 'oracle'])
+const STRICT = new Set(['mysql', 'mariadb', 'postgres', 'oracle', 'sqlserver'])
 
 export function sqlDialect(driver: DriverId, variant?: string | null, compat?: Compat | null): SqlDialect {
   const strict = !!variant && STRICT.has(variant)
@@ -27,5 +27,7 @@ export function sqlDialect(driver: DriverId, variant?: string | null, compat?: C
     case 'oracle':
     case 'dameng':
       return { dialect: 'plsql', strict, grammar: 'Oracle' }
+    case 'sqlserver':
+      return { dialect: 'tsql', strict, grammar: 'T-SQL' }
   }
 }
```

- [ ] **Step 32: 修改 `web/src/features/sql/functions.ts`**

```diff
--- a/web/src/features/sql/functions.ts
+++ b/web/src/features/sql/functions.ts
@@ -18,10 +18,17 @@ LAST_DAY LEAD LEAST LENGTH LISTAGG LOWER LPAD LTRIM MAX MIN MOD MONTHS_BETWEEN N
 REGEXP_SUBSTR REPLACE ROUND ROW_NUMBER RPAD RTRIM SUBSTR SUM SYS_GUID SYSDATE SYSTIMESTAMP TO_CHAR TO_DATE TO_NUMBER
 TO_TIMESTAMP TRIM TRUNC UPPER`
 
+const TSQL = `ABS AVG CAST CEILING CHARINDEX COALESCE CONCAT CONCAT_WS CONVERT COUNT COUNT_BIG DATEADD DATEDIFF DATEFROMPARTS
+DATENAME DATEPART DAY DENSE_RANK EOMONTH FLOOR FORMAT GETDATE GETUTCDATE GREATEST IIF ISJSON ISNULL JSON_QUERY JSON_VALUE LAG
+LEAD LEAST LEFT LEN LOWER LTRIM MAX MIN MONTH NEWID NTILE NULLIF OBJECT_ID PATINDEX RANK REPLACE REPLICATE RIGHT ROUND
+ROW_NUMBER RTRIM SCOPE_IDENTITY STRING_AGG STRING_SPLIT STUFF SUBSTRING SUM SYSDATETIME SYSDATETIMEOFFSET TRANSLATE TRIM
+TRY_CAST TRY_CONVERT UPPER YEAR`
+
 const LISTS: Record<Dialect, string[]> = {
   mysql: MYSQL.split(/\s+/),
   postgres: POSTGRES.split(/\s+/),
   plsql: PLSQL.split(/\s+/),
+  tsql: TSQL.split(/\s+/),
 }
 
 export function builtinFunctions(d: Dialect): string[] {
```

- [ ] **Step 33: 修改 `web/src/features/sql/metadata.ts`**

```diff
--- a/web/src/features/sql/metadata.ts
+++ b/web/src/features/sql/metadata.ts
@@ -6,7 +6,7 @@
 
 import type { QueryClient } from '@tanstack/react-query'
 
-import type { Api, DriverId } from '../../shared/api'
+import { type Api, type DriverId, joinCatalog } from '../../shared/api'
 
 export interface MetaObject {
   name: string
@@ -49,21 +49,30 @@ function within<T>(p: Promise<T>, ms: number, fallback: T): Promise<T> {
   })
 }
 
-export function metaReader(qc: QueryClient, api: MetaApi, profileId: string, driver: DriverId, waitMs = 1500): MetaReader {
+/**
+ * `catalog`：SQL Server 会话的当前库。读取的架构都在这个库中（与对象树的合成 schema 相同，缓存也共用）。
+ */
+export function metaReader(
+  qc: QueryClient,
+  api: MetaApi,
+  profileId: string,
+  driver: DriverId,
+  catalog?: string | null,
+  waitMs = 1500,
+): MetaReader {
   const get = <T>(key: unknown[], fn: () => Promise<T>, fallback: T) =>
     within(qc.fetchQuery({ queryKey: key, queryFn: fn, staleTime: Infinity }), waitMs, fallback)
-  const plsql = driver === 'oracle' || driver === 'dameng'
-  return {
-    schemas: () => get([...metaKey(profileId), '#schemas'], async () => (await api.schemas(profileId)).map((s) => s.name), []),
-    objects: (schema) =>
+  const withSynonyms = driver === 'oracle' || driver === 'dameng' || driver === 'sqlserver'
+  const inner = { schemas: () => get([...metaKey(profileId), '#schemas', catalog ?? ''], async () => (await api.schemas(profileId, undefined, catalog ?? undefined)).map((s) => s.name), []),
+    objects: (schema: string) =>
       get(
         [...metaKey(profileId, schema), 'objects'],
         async () => {
           const [tables, views, synonyms, sequences] = await Promise.all([
             api.objects(profileId, schema, 'tables'),
             api.objects(profileId, schema, 'views'),
-            plsql ? api.synonyms(profileId, schema) : Promise.resolve([]),
-            plsql ? api.sequences(profileId, schema) : Promise.resolve([]),
+            withSynonyms ? api.synonyms(profileId, schema) : Promise.resolve([]),
+            withSynonyms ? api.sequences(profileId, schema) : Promise.resolve([]),
           ])
           return [
             ...tables.map((o): MetaObject => ({ name: o.name, kind: 'table', comment: o.comment })),
@@ -74,7 +83,7 @@ export function metaReader(qc: QueryClient, api: MetaApi, profileId: string, dri
         },
         [],
       ),
-    columns: (schema, table) =>
+    columns: (schema: string, table: string) =>
       get(
         [...metaKey(profileId, schema), 'columns', table],
         async () =>
@@ -85,13 +94,22 @@ export function metaReader(qc: QueryClient, api: MetaApi, profileId: string, dri
           })),
         [],
       ),
-    functions: (schema) =>
+    functions: (schema: string) =>
       get(
         [...metaKey(profileId, schema), 'functions'],
         async () => (await api.routines(profileId, schema)).filter((r) => r.kind === 'function').map((r) => r.name),
         [],
       ),
   }
+  if (driver !== 'sqlserver' || !catalog) return inner
+  // SQL Server：补全按架构名取元数据，请求时带上当前库
+  const at = (schema: string) => joinCatalog(catalog, schema)
+  return {
+    schemas: inner.schemas,
+    objects: (schema) => inner.objects(at(schema)),
+    columns: (schema, table) => inner.columns(at(schema), table),
+    functions: (schema) => inner.functions(at(schema)),
+  }
 }
 
 /** 按名称找：先找完全相同的，再不分大小写（语句里写的 emp 对应 Oracle 中的 EMP）。 */
@@ -100,7 +118,10 @@ export function findName<T extends { name: string } | string>(items: T[], name:
   return items.find((x) => n(x) === name) ?? items.find((x) => n(x).toUpperCase() === name.toUpperCase())
 }
 
-/** 没有打开会话时的当前 schema：MySQL 系是连接配置里的库，PG 系是 public，Oracle 和达梦是用户名（大写）。 */
+/**
+ * 补全用的当前 schema：MySQL 系是连接配置里的库，PG 系是 public，Oracle 和达梦是用户名（大写），
+ * SQL Server 是 dbo（会话的当前库另作 `metaReader` 的 catalog）。
+ */
 export function defaultSchema(driver: DriverId, user: string, database?: string | null): string | null {
   switch (driver) {
     case 'mysql':
@@ -110,5 +131,7 @@ export function defaultSchema(driver: DriverId, user: string, database?: string
     case 'oracle':
     case 'dameng':
       return user.toUpperCase()
+    case 'sqlserver':
+      return 'dbo'
   }
 }
```

- [ ] **Step 34: 修改 `web/src/features/sql/parsers.ts`**

```diff
--- a/web/src/features/sql/parsers.ts
+++ b/web/src/features/sql/parsers.ts
@@ -1,4 +1,5 @@
-// 按语法族懒加载解析器：每种是一个单独的块，用到时才下载（MySQL、PG 各约 0.35 MB，PL/SQL 约 1.4 MB，gzip 后）。
+// 按语法族懒加载解析器：每种是一个单独的块，用到时才下载（MySQL、PG 各约 0.35 MB，PL/SQL 约 1.4 MB，gzip 后；
+// T-SQL 与 PL/SQL 相近）。
 
 import type { Dialect, SqlParser } from './types'
 
@@ -17,6 +18,9 @@ export function loadParser(d: Dialect): Promise<SqlParser> {
       case 'plsql':
         p = import('./plsql/parser').then((m) => m.plsql)
         break
+      case 'tsql':
+        p = import('./tsql/parser').then((m) => m.tsql)
+        break
     }
     // 加载失败时不缓存，下次重试
     p.catch(() => loaded.delete(d))
```

- [ ] **Step 35: 修改 `web/src/features/sql/provider.ts`**

```diff
--- a/web/src/features/sql/provider.ts
+++ b/web/src/features/sql/provider.ts
@@ -1,4 +1,4 @@
-// Monaco 的 SQL 补全（M1b2 设计文档第 4 节）。三种语言（mysql、pgsql、plsql）共用一个补全提供者；
+// Monaco 的 SQL 补全（M1b2 设计文档第 4 节）。四种语言（mysql、pgsql、plsql、sql）共用一个补全提供者；
 // 每个编辑器模型绑定它所在标签页的上下文（连接、变体、当前 schema、元数据），提供者按模型找到上下文。
 
 import type { Compat, DriverId } from '../../shared/api'
@@ -89,5 +89,5 @@ let registered = false
 export function registerSqlProviders() {
   if (registered) return
   registered = true
-  for (const lang of ['mysql', 'pgsql', 'plsql']) monaco.languages.registerCompletionItemProvider(lang, provider)
+  for (const lang of ['mysql', 'pgsql', 'plsql', 'sql']) monaco.languages.registerCompletionItemProvider(lang, provider)
 }
```

- [ ] **Step 36: 新建 `web/src/features/sql/tsql/parser.ts`**

```ts
// SQL Server 的语言服务（M6a 设计文档第 8 节）：grammars-v4 的 T-SQL 语法生成的解析器，加 antlr4-c3 分析光标处
// 可以出现的规则和关键字。做法同 PL/SQL（plsql/parser.ts）：先分批再逐批解析，没改动的批用缓存；先 SLL 后 LL。
// 分批规则与后端 dblens-dialect 的 split.rs 一致：单独一行的 `GO [次数]`（行尾可有注释）结束一批；
// 字符串、`[…]`、`"…"` 和注释里的 GO 由词法分析排除。"光标处的语句"就是光标所在的一批。

import { CodeCompletionCore } from 'antlr4-c3'
import {
  type ATNSimulator,
  BaseErrorListener,
  CharStream,
  CommonTokenStream,
  PredictionMode,
  type Recognizer,
  Token,
} from 'antlr4ng'

import { type ParseIssue, type SqlParser, type Suggest, type TableRef, type Want, pickStatement } from '../types'
import { TSqlLexer } from './gen/TSqlLexer'
import { TSqlParser, type Tsql_fileContext } from './gen/TSqlParser'

const P = TSqlParser

/** 把 ANTLR 的报错转成 ParseIssue（行列相对于被解析的那一批）。 */
class IssueCollector extends BaseErrorListener {
  issues: ParseIssue[] = []
  constructor(private readonly lastWord: () => Token | null) {
    super()
  }

  override syntaxError<S extends Token, T extends ATNSimulator>(
    _r: Recognizer<T>,
    offending: S | null,
    line: number,
    column: number,
    msg: string,
  ): void {
    const last = this.lastWord()
    if (offending && (offending.type === Token.EOF || offending.tokenIndex === last?.tokenIndex)) {
      this.issues.push({ ...tokenRange(last, line, column), incomplete: true })
    } else if (offending) {
      this.issues.push({ ...tokenRange(offending, line, column), near: offending.text ?? '' })
    } else {
      const near = /at: '(.*)'$/s.exec(msg)?.[1] ?? ''
      const first = near.split('\n')[0]
      this.issues.push({ startLine: line, startColumn: column + 1, endLine: line, endColumn: column + 1 + Math.max(first.length, 1), near: first })
    }
  }
}

function tokenRange(t: Token | null, line: number, column: number) {
  if (!t || t.type === Token.EOF) return { startLine: line, startColumn: column + 1, endLine: line, endColumn: column + 2 }
  const lines = (t.text ?? '').split('\n')
  const endLine = t.line + lines.length - 1
  const endColumn = lines.length > 1 ? lines[lines.length - 1].length + 1 : t.column + 1 + lines[0].length
  return { startLine: t.line, startColumn: t.column + 1, endLine, endColumn }
}

function lex(sql: string): Token[] {
  const lexer = new TSqlLexer(CharStream.fromString(sql))
  lexer.removeErrorListeners()
  const stream = new CommonTokenStream(lexer)
  stream.fill()
  return stream.getTokens()
}

const isCode = (t: Token) => t.channel === Token.DEFAULT_CHANNEL && t.type !== Token.EOF

interface Parsed {
  tree: Tsql_fileContext
  tokens: Token[]
  parser: TSqlParser
  /** 只保留第一处：一批中后面的报错多是连带的。 */
  issue: ParseIssue | null
}

const cache = new Map<string, Parsed>()
const CACHE_LIMIT = 500

/** 解析一批（或任意文本）。 */
export function parseText(text: string): Parsed {
  const hit = cache.get(text)
  if (hit) {
    cache.delete(text)
    cache.set(text, hit)
    return hit
  }
  let tokens: Token[] = []
  const lastWord = () => [...tokens].reverse().find(isCode) ?? null
  const lexErrors = new IssueCollector(lastWord)
  const lexer = new TSqlLexer(CharStream.fromString(text))
  lexer.removeErrorListeners()
  lexer.addErrorListener(lexErrors)
  const stream = new CommonTokenStream(lexer)
  stream.fill()
  tokens = stream.getTokens()
  const parser = new TSqlParser(stream)
  const errors = new IssueCollector(lastWord)
  parser.removeErrorListeners()
  parser.addErrorListener(errors)
  parser.interpreter.predictionMode = PredictionMode.SLL
  let tree = parser.tsql_file()
  if (errors.issues.length) {
    errors.issues = []
    stream.seek(0)
    parser.reset()
    parser.interpreter.predictionMode = PredictionMode.LL
    tree = parser.tsql_file()
  }
  const parsed = { tree, tokens, parser, issue: lexErrors.issues[0] ?? errors.issues[0] ?? null }
  cache.set(text, parsed)
  if (cache.size > CACHE_LIMIT) cache.delete(cache.keys().next().value as string)
  return parsed
}

/**
 * 一批在全文中的位置：[start, end) 为 UTF-16 偏移（不含批前后的空白和注释），line / column 为起点（行从 1、列从 0 开始）；
 * `limit` 是这一批能延伸到的位置（下一个 GO 行的开头或全文末尾），光标在 [start, limit] 中都算在这一批里。
 */
export interface Piece {
  start: number
  end: number
  line: number
  column: number
  limit: number
}

/** 第 i 个 token 是否是批的结束：单独一行的 GO，后面同一行只能有次数（注释在隐藏通道上）。 */
function isGoLine(tokens: Token[], i: number): boolean {
  const t = tokens[i]
  if (t.type !== P.GO) return false
  const sameLine = (j: number) => tokens[j] && tokens[j].line === t.line && isCode(tokens[j])
  for (let j = i - 1; j >= 0 && tokens[j].line === t.line; j--) if (sameLine(j)) return false
  const after: Token[] = []
  for (let j = i + 1; j < tokens.length && tokens[j].line === t.line; j++) if (sameLine(j)) after.push(tokens[j])
  return after.length === 0 || (after.length === 1 && after[0].type === P.DECIMAL)
}

/** 分批：GO 行不含在批中；没有代码的批跳过。 */
export function split(tokens: Token[]): Piece[] {
  const out: Piece[] = []
  let first: Token | null = null
  let last: Token | null = null
  const close = (limit: number) => {
    if (first && last) out.push({ start: first.start, end: last.stop + 1, line: first.line, column: first.column, limit })
    first = null
    last = null
  }
  for (let i = 0; i < tokens.length; i++) {
    const t = tokens[i]
    if (!isCode(t)) continue
    if (isGoLine(tokens, i)) {
      close(t.start)
      // 跳过同一行的次数
      while (tokens[i + 1] && tokens[i + 1].line === t.line) i++
      continue
    }
    if (!first) first = t
    last = t
  }
  close(tokens[tokens.length - 1]?.start ?? 0)
  return out
}

function shift(i: ParseIssue, p: Piece): ParseIssue {
  const col = (line: number, c: number) => (line === 1 ? c + p.column : c)
  return {
    ...i,
    startLine: i.startLine + p.line - 1,
    startColumn: col(i.startLine, i.startColumn),
    endLine: i.endLine + p.line - 1,
    endColumn: col(i.endLine, i.endColumn),
  }
}

const WORD = /^[\p{L}_#@["]/u

/** 光标对应的 token 下标（c3 从这里给候选）：光标在词中或紧贴词尾时是这个词，否则是光标后的第一个 token。 */
export function caretTokenIndex(tokens: Token[], offset: number): number {
  for (const t of tokens) {
    if (t.channel !== Token.DEFAULT_CHANNEL && t.type !== Token.EOF) continue
    if (t.type === Token.EOF || t.start >= offset) return t.tokenIndex
    if (offset <= t.stop + 1 && WORD.test(t.text ?? '')) return t.tokenIndex
  }
  return tokens.length - 1
}

/** 去掉方括号或双引号。 */
export function unquote(s: string): string {
  if (s.startsWith('[') && s.endsWith(']')) return s.slice(1, -1).replace(/\]\]/g, ']')
  if (s.startsWith('"') && s.endsWith('"')) return s.slice(1, -1).replace(/""/g, '"')
  return s
}

const isName = (t: Token | undefined) => !!t && isCode(t) && WORD.test(t.text ?? '') && !(t.text ?? '').startsWith('@')

function qualifierBefore(code: Token[], pos: number): string[] {
  const out: string[] = []
  let i = pos - 1
  while (i >= 1 && code[i].type === P.DOT && isName(code[i - 1])) {
    out.unshift(unquote(code[i - 1].text ?? ''))
    i -= 2
  }
  return out
}

/** 表名后面的这些词不是别名。 */
const NOT_ALIAS = new Set(
  'WHERE JOIN ON INNER LEFT RIGHT FULL OUTER CROSS APPLY GROUP ORDER HAVING UNION EXCEPT INTERSECT SET VALUES WHEN OPTION FOR PIVOT UNPIVOT AS WITH OUTPUT SELECT TABLESAMPLE DEFAULT GO'.split(' '),
)
/** 这些词后面是表名。 */
const BEFORE_TABLE = new Set([P.FROM, P.JOIN, P.INTO, P.UPDATE, P.TABLE, P.MERGE])

/** 按词扫描语句中的表和别名（三段名 `库.架构.表` 取后两段）。不依赖语法树，写到一半的语句也能识别。 */
export function scanTables(code: Token[]): TableRef[] {
  const out: TableRef[] = []
  const seen = new Set<string>()
  for (let i = 0; i < code.length; i++) {
    if (!BEFORE_TABLE.has(code[i].type)) continue
    let j = i + 1
    for (;;) {
      if (!isName(code[j])) break
      const parts = [unquote(code[j].text ?? '')]
      j++
      while (code[j]?.type === P.DOT && isName(code[j + 1])) {
        parts.push(unquote(code[j + 1].text ?? ''))
        j += 2
      }
      let alias: string | undefined
      if (code[j]?.type === P.AS && isName(code[j + 1])) j++
      if (isName(code[j]) && !NOT_ALIAS.has((code[j].text ?? '').toUpperCase())) alias = unquote(code[j++].text ?? '')
      const name = parts[parts.length - 1]
      if (parts.length <= 3 && !NOT_ALIAS.has(name.toUpperCase())) {
        const ref: TableRef = parts.length >= 2 ? { schema: parts[parts.length - 2], name } : { name }
        if (alias) ref.alias = alias
        const key = JSON.stringify(ref)
        if (!seen.has(key)) {
          seen.add(key)
          out.push(ref)
        }
      }
      if (code[i].type === P.FROM && code[j]?.type === P.COMMA) {
        j++
        continue
      }
      break
    }
  }
  return out
}

const PREFERRED = new Map<number, Want[]>([
  [P.RULE_full_table_name, ['table', 'view']],
  [P.RULE_table_name, ['table', 'view']],
  [P.RULE_full_column_name, ['column']],
  [P.RULE_scalar_function_name, ['function']],
  // 只用来挡住标识符位置上大量的非保留关键字，不产生补全
  [P.RULE_id_, []],
  [P.RULE_keyword, []],
])

const KEYWORD = /^[A-Z][A-Z0-9_]*$/

export const tsql: SqlParser = {
  validate(sql) {
    const out: ParseIssue[] = []
    for (const p of split(lex(sql))) {
      const { issue } = parseText(sql.slice(p.start, p.end))
      if (issue) out.push(shift(issue, p))
    }
    return out
  },

  suggest(sql, offset): Suggest {
    // 光标所在的一批；光标在批之外（如 GO 之后的空行）时是新的空批
    const piece = split(lex(sql)).find((p) => p.start <= offset && offset <= p.limit) ?? { start: offset, limit: offset }
    const { tree, tokens, parser } = parseText(sql.slice(piece.start, piece.limit))
    const caret = offset - piece.start
    const idx = caretTokenIndex(tokens, caret)
    const core = new CodeCompletionCore(parser)
    core.preferredRules = new Set(PREFERRED.keys())
    const cands = core.collectCandidates(idx, tree)
    const want = new Set<Want>()
    for (const rule of cands.rules.keys()) for (const w of PREFERRED.get(rule) ?? []) want.add(w)
    const keywords: string[] = []
    for (const t of cands.tokens.keys()) {
      const name = parser.vocabulary.getLiteralName(t)?.replace(/^'|'$/g, '')
      if (name && KEYWORD.test(name)) keywords.push(name)
    }
    const code = tokens.filter(isCode)
    const pos = code.findIndex((t) => t.tokenIndex >= idx)
    return { keywords, want: [...want], qualifier: qualifierBefore(code, pos < 0 ? code.length : pos), tables: scanTables(code) }
  },

  statementAt(sql, offset) {
    const ranges = split(lex(sql)).map((p) => ({ start: p.start, end: p.end }))
    return pickStatement(ranges, offset)
  },
}
```

- [ ] **Step 37: 修改 `web/src/features/sql/types.ts`**

```diff
--- a/web/src/features/sql/types.ts
+++ b/web/src/features/sql/types.ts
@@ -1,7 +1,7 @@
 // SQL 语言服务（M1b2 设计文档第 4 节）的公共类型：worker 与主线程之间传递的数据。
 
-/** 解析器的语法族：MySQL 系、PG 系用 dt-sql-parser，Oracle 和达梦用 PL/SQL 语法。 */
-export type Dialect = 'mysql' | 'postgres' | 'plsql'
+/** 解析器的语法族：MySQL 系、PG 系用 dt-sql-parser，Oracle 和达梦用 PL/SQL 语法，SQL Server 用 T-SQL 语法。 */
+export type Dialect = 'mysql' | 'postgres' | 'plsql' | 'tsql'
 
 /** 错误文字的语言（dt-sql-parser 的取值）。 */
 export type ParserLocale = 'zh_CN' | 'en_US'
```

- [ ] **Step 38: 修改 `web/src/features/tree/DdlDialog.tsx`**

```diff
--- a/web/src/features/tree/DdlDialog.tsx
+++ b/web/src/features/tree/DdlDialog.tsx
@@ -7,7 +7,7 @@ import { App, Button, Modal, Spin } from 'antd'
 import { useTranslation } from 'react-i18next'
 
 import { useApi } from '../../app/context'
-import type { DdlKind, DriverId } from '../../shared/api'
+import { type DdlKind, type DriverId, schemaLabel } from '../../shared/api'
 import { useSetting } from '../../shared/settings'
 import { THEME_KEY, parseThemePref, resolveTheme, useSystemDark } from '../../shared/theme'
 import { useQueryActions } from '../editor/actions'
@@ -42,7 +42,7 @@ export default function DdlDialog({ target, onClose }: { target: DdlTarget; onCl
     <Modal
       open
       width={820}
-      title={t('tree.ddl.title', { name: `${schema}.${name}` })}
+      title={t('tree.ddl.title', { name: `${schemaLabel(schema)}.${name}` })}
       onCancel={onClose}
       footer={[
         <Button
```

- [ ] **Step 39: 修改 `web/src/features/tree/ObjectTree.tsx`**

```diff
--- a/web/src/features/tree/ObjectTree.tsx
+++ b/web/src/features/tree/ObjectTree.tsx
@@ -3,11 +3,13 @@
 // 右键菜单（M1a）：连接 —— 编辑、复制、删除、刷新；其他节点 —— 复制名称、刷新。
 // 对象节点另有（M1c2）："查看数据"（数据标签，也可双击打开）、"查看 DDL"（弹出只读预览）、
 // "生成 SQL"（插入到当前编辑器）。
+// SQL Server（M6a）：连接下先列库，库下是架构；表设计器、导入、文档导出、比对、保存为模型、迁移的入口置灰。
 
 import { App, Dropdown, Input, type MenuProps, Tree } from 'antd'
 import type { DataNode, EventDataNode } from 'antd/es/tree'
 import {
   IconColumns,
+  IconDatabase,
   IconEye,
   IconFolder,
   IconFunction,
@@ -31,7 +33,16 @@ import { useImportDialogs } from '../import/dialog'
 import { useDocExport } from '../document/dialog'
 import { emptyPlan } from '../migrate/plan'
 import { useModelDialogs } from '../model/dialogs'
-import type { Api, Environment, Profile, RoutineInfo } from '../../shared/api'
+import {
+  type Api,
+  type Environment,
+  type Profile,
+  type RoutineInfo,
+  exportSource,
+  joinCatalog,
+  schemaLabel,
+  supportsWrites,
+} from '../../shared/api'
 import DbIcon from '../../shared/DbIcon'
 import { profileKind } from '../connections/form'
 import { withVault } from '../connections/vault'
@@ -95,16 +106,28 @@ const CATEGORY_ICON: Record<CategoryKind, ReactNode> = {
 async function loadChildren(api: Api, profiles: Profile[], k: NodeKey): Promise<Loaded['items']> {
   switch (k.type) {
     case 'c':
+    case 'd':
     case 'g': {
-      const schemas = await api.schemas(k.profileId)
+      const driver = profiles.find((p) => p.id === k.profileId)?.driver
+      // SQL Server：连接下是库（系统库排在后面），库下才是架构
+      if (k.type === 'c' && driver === 'sqlserver') {
+        return (await api.catalogs(k.profileId)).map((c) => ({
+          key: encodeKey({ type: 'd', profileId: k.profileId, catalog: c.name }),
+          label: c.name,
+          extra: c.is_system ? 'system' : undefined,
+          leaf: false,
+        }))
+      }
+      const catalog = k.type === 'd' || k.type === 'g' ? k.catalog : undefined
+      const schemas = await api.schemas(k.profileId, undefined, catalog)
       const pick = schemas.filter((s) => s.is_system === (k.type === 'g'))
       const items: Loaded['items'] = pick.map((s) => ({
-        key: encodeKey({ type: 's', profileId: k.profileId, schema: s.name }),
+        key: encodeKey({ type: 's', profileId: k.profileId, schema: catalog === undefined ? s.name : joinCatalog(catalog, s.name) }),
         label: s.name,
         leaf: false,
       }))
-      if (k.type === 'c' && schemas.some((s) => s.is_system)) {
-        items.push({ key: encodeKey({ type: 'g', profileId: k.profileId }), label: '', leaf: false })
+      if (k.type !== 'g' && schemas.some((s) => s.is_system)) {
+        items.push({ key: encodeKey({ type: 'g', profileId: k.profileId, catalog }), label: '', leaf: false })
       }
       return items
     }
@@ -195,9 +218,14 @@ export default function ObjectTree() {
     [api, profiles, qc],
   )
 
-  /** 刷新一个节点：清后端元数据缓存，丢掉它下面已加载和已展开的内容，再重新取。系统分组按整个连接刷新。 */
+  /** 刷新一个节点：清后端元数据缓存，丢掉它下面已加载和已展开的内容，再重新取。系统分组按所在的连接或库刷新。 */
   const refresh = async (node: NodeKey) => {
-    const k: NodeKey = node.type === 'g' ? { type: 'c', profileId: node.profileId } : node
+    const k: NodeKey =
+      node.type === 'g'
+        ? node.catalog === undefined
+          ? { type: 'c', profileId: node.profileId }
+          : { type: 'd', profileId: node.profileId, catalog: node.catalog }
+        : node
     const key = encodeKey(k)
     await api.refresh(k.profileId, 'schema' in k ? k.schema : null).catch(() => {})
     // 补全用的元数据一并失效
@@ -234,9 +262,9 @@ export default function ObjectTree() {
       return items.map((i) => {
         const k = decodeKey(i.key)
         const icon =
-          k.type === 's' ? <IconSchema size={16} /> : k.type === 'g' || k.type === 'k' ? <IconFolder size={16} /> : k.type === 'o' ? CATEGORY_ICON[k.kind] : i.pk ? <IconKey size={16} color="var(--warn)" /> : <IconColumns size={16} />
+          k.type === 'd' ? <IconDatabase size={16} /> : k.type === 's' ? <IconSchema size={16} /> : k.type === 'g' || k.type === 'k' ? <IconFolder size={16} /> : k.type === 'o' ? CATEGORY_ICON[k.kind] : i.pk ? <IconKey size={16} color="var(--warn)" /> : <IconColumns size={16} />
         const label = k.type === 'k' ? categoryLabel(k.kind) : k.type === 'g' ? t('tree.node.system') : i.label
-        const extra = i.routineKind ? routineKindLabel(i.routineKind) : i.extra
+        const extra = i.routineKind ? routineKindLabel(i.routineKind) : i.extra === 'system' ? t('tree.node.system') : i.extra
         const count = k.type === 'k' && loaded[i.key] && !loaded[i.key].error ? loaded[i.key].items.length : null
         return {
           key: i.key,
@@ -295,19 +323,29 @@ export default function ObjectTree() {
           ]
         : []
 
+  // 下面几个是函数声明而不是箭头函数常量：上面的 menuItems 在渲染时就会调用它们
+  /** 连接是否支持写入类功能（M6b 之前的 SQL Server 不支持）；不支持时这些菜单项置灰并说明。 */
+  function writable(profileId: string) {
+    return supportsWrites(profiles.find((p) => p.id === profileId)?.driver ?? 'mysql')
+  }
+  function later(key: string, label: string, enabled: boolean) {
+    return enabled ? { key, label } : { key, label: <span title={t('tree.menu.sqlServerLater')}>{label}</span>, disabled: true }
+  }
+
   /** schema 节点：新建表、导入到新表、执行 SQL 文件，后面接分隔线。 */
   function schemaItems(k: NodeKey): NonNullable<MenuProps['items']> {
     if (k.type !== 's') return []
+    const w = writable(k.profileId)
     return [
-      { key: 'new-table', label: t('tree.menu.newTable') },
-      { key: 'import-new', label: t('tree.menu.importNew') },
+      later('new-table', t('tree.menu.newTable'), w),
+      later('import-new', t('tree.menu.importNew'), w),
       { key: 'sql-file', label: t('tree.menu.sqlFile') },
       { type: 'divider' },
       { key: 'er', label: t('tree.menu.er') },
-      { key: 'compare-model', label: t('tree.menu.compareModel') },
-      { key: 'save-model', label: t('tree.menu.saveAsModel') },
-      { key: 'export-doc', label: t('tree.menu.exportDoc') },
-      { key: 'migrate', label: t('tree.menu.migrate') },
+      later('compare-model', t('tree.menu.compareModel'), w),
+      later('save-model', t('tree.menu.saveAsModel'), w),
+      later('export-doc', t('tree.menu.exportDoc'), w),
+      later('migrate', t('tree.menu.migrate'), w),
       { type: 'divider' },
     ]
   }
@@ -317,9 +355,10 @@ export default function ObjectTree() {
     if (k.type !== 'o') return []
     const items: NonNullable<MenuProps['items']> = []
     const target = targetKind(k.kind)
+    const w = writable(k.profileId)
     if (target) items.push({ key: 'view-data', label: t('tree.menu.viewData') })
-    if (target === 'table') items.push({ key: 'design', label: t('tree.menu.design') })
-    if (target === 'table') items.push({ key: 'import', label: t('tree.menu.import') })
+    if (target === 'table') items.push(later('design', t('tree.menu.design'), w))
+    if (target === 'table') items.push(later('import', t('tree.menu.import'), w))
     if (target) items.push({ key: 'export-data', label: t('tree.menu.exportData') })
     if (ddlKind(k.kind, k.routineKind)) items.push({ key: 'ddl', label: t('tree.menu.viewDdl') })
     if (target) {
@@ -355,7 +394,7 @@ export default function ObjectTree() {
       else if (key === 'design' && k.type === 'o') useTabs.getState().openDesign(k.profileId, { schema: k.schema, table: k.name })
       else if (key === 'new-table' && k.type === 's') useTabs.getState().openDesign(k.profileId, { schema: k.schema, table: null })
       else if (key === 'er' && k.type === 's') {
-        useTabs.getState().openEr({ kind: 'database', profile: k.profileId, schema: k.schema }, `${k.schema} · ER`)
+        useTabs.getState().openEr({ kind: 'database', profile: k.profileId, schema: k.schema }, `${schemaLabel(k.schema)} · ER`)
       } else if (key === 'compare-model' && k.type === 's') {
         useTabs.getState().openCompare({ source: null, target: { kind: 'database', profile: k.profileId, schema: k.schema } })
       } else if (key === 'save-model' && k.type === 's') useModelDialogs.getState().openReverse({ profileId: k.profileId, schema: k.schema })
@@ -370,9 +409,9 @@ export default function ObjectTree() {
       } else if (key === 'export-data' && k.type === 'o') {
         useExportDialog.getState().open({
           profileId: k.profileId,
-          source: { data: { schema: k.schema, object: k.name } },
+          source: exportSource(k.schema, k.name),
           baseName: k.name,
-          table: `${k.schema}.${k.name}`,
+          table: `${schemaLabel(k.schema)}.${k.name}`,
           inTransaction: false,
         })
       }
```

- [ ] **Step 40: 修改 `web/src/features/tree/nodes.ts`**

```diff
--- a/web/src/features/tree/nodes.ts
+++ b/web/src/features/tree/nodes.ts
@@ -1,7 +1,8 @@
 // 对象树的节点结构（设计文档 7.3 节）：连接 → schema → 分类（表、视图……）→ 对象 →（表的）列。
+// SQL Server 多一层库（M6a）：连接 → 库 → 架构 → …，架构节点的 schema 是库和架构合成的（见 api.ts 的 joinCatalog）。
 // 懒加载：展开时才向后端取下一层。这里只放纯逻辑，单独测试。
 
-import type { DriverId, ObjectKind, RoutineInfo } from '../../shared/api'
+import { type DriverId, type ObjectKind, type RoutineInfo, schemaLabel, splitCatalog } from '../../shared/api'
 import { t } from '../../shared/i18n'
 
 export type CategoryKind = ObjectKind | 'routines' | 'sequences' | 'synonyms'
@@ -33,23 +34,27 @@ export function categoriesFor(driver: DriverId): Category[] {
       ? ['tables', 'views', 'routines']
       : driver === 'postgres'
         ? ['tables', 'views', 'materialized_views', 'routines', 'sequences']
-        : ['tables', 'views', 'materialized_views', 'routines', 'sequences', 'synonyms']
+        : driver === 'sqlserver'
+          ? ['tables', 'views', 'routines', 'sequences', 'synonyms']
+          : ['tables', 'views', 'materialized_views', 'routines', 'sequences', 'synonyms']
   return kinds.map((kind) => ({ kind, label: categoryLabel(kind) }))
 }
 
 /**
  * 节点的键。各段用 `\u0001` 连接（数据库对象名里不会出现），开头是节点类型：
  * - `c` 连接：c、profileId
+ * - `d` 库（SQL Server）：d、profileId、catalog
  * - `s` schema：s、profileId、schema
- * - `g` 系统 schema 分组：g、profileId
+ * - `g` 系统 schema 分组：g、profileId，库中的分组另有 catalog
  * - `k` 分类：k、profileId、schema、kind
  * - `o` 对象：o、profileId、schema、kind、name，程序另有 routineKind（包和包体同名，靠它区分）
  * - `l` 列：l、profileId、schema、table、column
  */
 export type NodeKey =
   | { type: 'c'; profileId: string }
+  | { type: 'd'; profileId: string; catalog: string }
   | { type: 's'; profileId: string; schema: string }
-  | { type: 'g'; profileId: string }
+  | { type: 'g'; profileId: string; catalog?: string }
   | { type: 'k'; profileId: string; schema: string; kind: CategoryKind }
   | {
       type: 'o'
@@ -66,8 +71,11 @@ const SEP = '\u0001'
 export function encodeKey(k: NodeKey): string {
   switch (k.type) {
     case 'c':
-    case 'g':
       return [k.type, k.profileId].join(SEP)
+    case 'g':
+      return [k.type, k.profileId, ...(k.catalog === undefined ? [] : [k.catalog])].join(SEP)
+    case 'd':
+      return [k.type, k.profileId, k.catalog].join(SEP)
     case 's':
       return [k.type, k.profileId, k.schema].join(SEP)
     case 'k':
@@ -85,7 +93,9 @@ export function decodeKey(s: string): NodeKey {
     case 'c':
       return { type, profileId }
     case 'g':
-      return { type, profileId }
+      return a === undefined ? { type, profileId } : { type, profileId, catalog: a }
+    case 'd':
+      return { type, profileId, catalog: a }
     case 's':
       return { type, profileId, schema: a }
     case 'k':
@@ -120,15 +130,17 @@ export function routineKindLabel(kind: RoutineInfo['kind']): string {
   return key ? t(`tree.routineKind.${key}`) : kind
 }
 
-/** 复制名称时用的限定名：schema.对象，列为 表.列。名字原样输出，不加引号。 */
+/** 复制名称时用的限定名：schema.对象（SQL Server 为 库.架构.对象），列为 表.列。名字原样输出，不加引号。 */
 export function qualifiedName(k: NodeKey): string {
   switch (k.type) {
     case 'o':
-      return `${k.schema}.${k.name}`
+      return `${schemaLabel(k.schema)}.${k.name}`
     case 'l':
       return `${k.table}.${k.column}`
     case 's':
-      return k.schema
+      return schemaLabel(k.schema)
+    case 'd':
+      return k.catalog
     default:
       return ''
   }
@@ -137,11 +149,15 @@ export function qualifiedName(k: NodeKey): string {
 /** child 是否在 parent 之下（不含 parent 自己）。刷新 parent 时，它下面已加载的内容都要丢掉。 */
 export function isWithin(child: NodeKey, parent: NodeKey): boolean {
   if (child.profileId !== parent.profileId) return false
+  const catalogOf = (c: NodeKey) =>
+    c.type === 'd' || c.type === 'g' ? c.catalog : 'schema' in c ? splitCatalog(c.schema).catalog : undefined
   switch (parent.type) {
     case 'c':
       return child.type !== 'c'
+    case 'd':
+      return child.type !== 'c' && child.type !== 'd' && catalogOf(child) === parent.catalog
     case 'g':
-      return child.type === 's'
+      return child.type === 's' && catalogOf(child) === parent.catalog
     case 's':
       return (child.type === 'k' || child.type === 'o' || child.type === 'l') && child.schema === parent.schema
     case 'k':
```

- [ ] **Step 41: 修改 `web/src/locales/en/connections.json`**

```diff
--- a/web/src/locales/en/connections.json
+++ b/web/src/locales/en/connections.json
@@ -15,6 +15,10 @@
     "dameng": {
       "label": "DM (Dameng)",
       "note": "Requires the DM DPI client library"
+    },
+    "sqlserver": {
+      "label": "SQL Server",
+      "note": "SQL logins and Windows authentication; browse by database after connecting"
     }
   },
   "product": {
@@ -50,13 +54,18 @@
     "dameng": {
       "label": "Dameng DM",
       "note": "Requires the Dameng DPI client library"
+    },
+    "sqlserver": {
+      "label": "SQL Server",
+      "note": "Default port 1433; set a named instance under Advanced to look up the port through SQL Browser"
     }
   },
   "databaseLabel": {
     "mysql": "Database",
     "postgres": "Database",
     "oracle": "Service Name",
-    "dameng": "Schema"
+    "dameng": "Schema",
+    "sqlserver": "Default database"
   },
   "validate": {
     "nameRequired": "Enter a connection name",
@@ -136,5 +145,19 @@
     "again": "Confirm Password",
     "masterRequired": "Enter the master password",
     "mismatch": "The passwords do not match"
+  },
+  "sqlserver": {
+    "auth": "Authentication",
+    "authOption": {
+      "sql": "SQL login",
+      "windows": "Windows account",
+      "integrated": "Current Windows user"
+    },
+    "windowsHint": "Write the user name as DOMAIN-or-HOST\\user (NTLM); available on Windows only",
+    "integratedHint": "Logs in as the Windows account running the DBLens service, without a user name or password; in browser mode that is the server's account, not the browser machine's. Available on Windows only",
+    "instance": "Instance",
+    "instancePlaceholder": "Optional, e.g. SQLEXPRESS; the port is ignored when set",
+    "trustCert": "Server certificate",
+    "trustCertHint": "Trust the server certificate (no validation, like the SSMS option)"
   }
 }
```

- [ ] **Step 42: 修改 `web/src/locales/en/editor.json`**

```diff
--- a/web/src/locales/en/editor.json
+++ b/web/src/locales/en/editor.json
@@ -20,7 +20,8 @@
     "inTransaction": "Uncommitted transaction",
     "format": "Format (Shift+Alt+F)",
     "explain": "Explain Plan",
-    "favorite": "Add to Favorites (Ctrl+D)"
+    "favorite": "Add to Favorites (Ctrl+D)",
+    "database": "database"
   },
   "switchProfile": {
     "title": "Switch Connection",
```

- [ ] **Step 43: 修改 `web/src/locales/en/tree.json`**

```diff
--- a/web/src/locales/en/tree.json
+++ b/web/src/locales/en/tree.json
@@ -46,7 +46,8 @@
     "exportData": "Export Data…",
     "newTable": "New table",
     "exportDoc": "Export Document…",
-    "migrate": "Migrate To…"
+    "migrate": "Migrate To…",
+    "sqlServerLater": "This feature is not yet available for SQL Server"
   },
   "duplicate": {
     "name": "{{name}} copy",
```

- [ ] **Step 44: 修改 `web/src/locales/zh-CN/connections.json`**

```diff
--- a/web/src/locales/zh-CN/connections.json
+++ b/web/src/locales/zh-CN/connections.json
@@ -15,6 +15,10 @@
     "dameng": {
       "label": "达梦 DM",
       "note": "需要达梦 DPI 客户端库"
+    },
+    "sqlserver": {
+      "label": "SQL Server",
+      "note": "支持 SQL 账号和 Windows 认证；连接后按库浏览"
     }
   },
   "product": {
@@ -50,13 +54,18 @@
     "dameng": {
       "label": "达梦 DM",
       "note": "需要达梦 DPI 客户端库"
+    },
+    "sqlserver": {
+      "label": "SQL Server",
+      "note": "默认端口 1433；命名实例在「高级」中填写，此时经 SQL Browser 查端口"
     }
   },
   "databaseLabel": {
     "mysql": "数据库",
     "postgres": "数据库",
     "oracle": "服务名",
-    "dameng": "模式"
+    "dameng": "模式",
+    "sqlserver": "默认库"
   },
   "validate": {
     "nameRequired": "填写连接名称",
@@ -136,5 +145,19 @@
     "again": "再输入一次",
     "masterRequired": "输入主密码",
     "mismatch": "两次输入的主密码不一致"
+  },
+  "sqlserver": {
+    "auth": "认证",
+    "authOption": {
+      "sql": "SQL 账号",
+      "windows": "Windows 账号",
+      "integrated": "当前 Windows 用户"
+    },
+    "windowsHint": "用户名写成 域或机器名\\用户（NTLM）；只能在 Windows 上使用",
+    "integratedHint": "用运行 DBLens 服务的 Windows 账号登录，不需要用户名和密码；浏览器访问时是服务端的账号，不是浏览器所在电脑的账号。只能在 Windows 上使用",
+    "instance": "实例名",
+    "instancePlaceholder": "可选，如 SQLEXPRESS；填写后忽略端口",
+    "trustCert": "服务器证书",
+    "trustCertHint": "信任服务器证书（不校验，同 SSMS 的同名选项）"
   }
 }
```

- [ ] **Step 45: 修改 `web/src/locales/zh-CN/editor.json`**

```diff
--- a/web/src/locales/zh-CN/editor.json
+++ b/web/src/locales/zh-CN/editor.json
@@ -20,7 +20,8 @@
     "inTransaction": "有未提交的事务",
     "format": "格式化（Shift+Alt+F）",
     "explain": "执行计划",
-    "favorite": "收藏（Ctrl+D）"
+    "favorite": "收藏（Ctrl+D）",
+    "database": "库"
   },
   "switchProfile": {
     "title": "切换连接",
```

- [ ] **Step 46: 修改 `web/src/locales/zh-CN/tree.json`**

```diff
--- a/web/src/locales/zh-CN/tree.json
+++ b/web/src/locales/zh-CN/tree.json
@@ -46,7 +46,8 @@
     "exportData": "导出数据…",
     "newTable": "新建表",
     "exportDoc": "导出文档…",
-    "migrate": "迁移到…"
+    "migrate": "迁移到…",
+    "sqlServerLater": "SQL Server 的这项功能将在后续版本支持"
   },
   "duplicate": {
     "name": "{{name}} 副本",
```

- [ ] **Step 47: 修改 `web/src/shared/api.ts`**

```diff
--- a/web/src/shared/api.ts
+++ b/web/src/shared/api.ts
@@ -4,9 +4,58 @@
 import type { CellValue } from './exec'
 import { currentLang, t } from './i18n'
 
-export type DriverId = 'mysql' | 'postgres' | 'oracle' | 'dameng'
+export type DriverId = 'mysql' | 'postgres' | 'oracle' | 'dameng' | 'sqlserver'
 export type Environment = 'none' | 'dev' | 'test' | 'prod'
 
+/**
+ * SQL Server 有库这一层（连接 → 库 → 架构）。界面上把库和架构合成一个 schema 字符串传递（对象树、数据标签等），
+ * 请求后端时拆开：架构进路径，库进 `catalog` 参数。分隔符不会出现在库名中。
+ */
+export const CATALOG_SEP = '\u001f'
+
+export function joinCatalog(catalog: string, schema: string): string {
+  return `${catalog}${CATALOG_SEP}${schema}`
+}
+
+export function splitCatalog(schema: string): { catalog?: string; schema: string } {
+  const i = schema.indexOf(CATALOG_SEP)
+  return i < 0 ? { schema } : { catalog: schema.slice(0, i), schema: schema.slice(i + 1) }
+}
+
+/** 给人看的 schema：带库时写成 `库.架构`。 */
+export function schemaLabel(schema: string): string {
+  const s = splitCatalog(schema)
+  return s.catalog === undefined ? s.schema : `${s.catalog}.${s.schema}`
+}
+
+/** 会话的"当前 schema"：SQL Server 是当前库（USE），其余是 schema。合成的 schema 取其中的库。 */
+export function sessionSchema(schema: string): string {
+  return splitCatalog(schema).catalog ?? schema
+}
+
+/**
+ * 导出表数据的来源。SQL Server 的合成 schema（库不是连接的初始库）：导出在新连接上执行，按三段名写成查询。
+ */
+export function exportSource(
+  schema: string,
+  object: string,
+  filter?: string,
+  order?: { column: string; desc: boolean } | null,
+): ExportSource {
+  const s = splitCatalog(schema)
+  if (s.catalog === undefined) return { data: { schema, object, filter, order } }
+  const q = (n: string) => `[${n.replaceAll(']', ']]')}]`
+  let sql = `SELECT * FROM ${q(s.catalog)}.${q(s.schema)}.${q(object)}`
+  if (filter?.trim()) sql += ` WHERE (${filter})`
+  if (order) sql += ` ORDER BY ${q(order.column)} ${order.desc ? 'DESC' : 'ASC'}`
+  return { sql }
+}
+
+/** M6b 之前 SQL Server 不支持的功能（表设计器、导入、文档导出、结构对比、迁移、保存为模型）。 */
+export function supportsWrites(driver: DriverId): boolean {
+  return driver !== 'sqlserver'
+}
+
 /** 服务端的错误对象（dblens_core::Error），按 kind 区分。 */
 export interface ErrorBody {
   kind: string
@@ -251,11 +300,18 @@ export type RowChange =
   | { kind: 'delete'; key: Record<string, CellValue> }
 
 export interface ChangeSet {
+  /** 可以是合成的 schema（SQL Server），请求时拆出库。 */
   schema: string
   table: string
   changes: RowChange[]
 }
 
+/** 改动集发给后端时拆出库（SQL Server）。 */
+function changeBody(set: ChangeSet) {
+  const s = splitCatalog(set.schema)
+  return { ...set, schema: s.schema, catalog: s.catalog ?? null }
+}
+
 /** 改动集的执行结果：失败时已整体回滚，`failed.index` 是失败的改动序号。 */
 export interface ApplyOutcome {
   applied: number
@@ -998,7 +1054,9 @@ export function createApi(token: string, fetchFn: FetchFn = (i, o) => fetch(i, o
     `${m(id)}/diagrams/${encodeURIComponent(schema)}/${encodeURIComponent(name)}`
   const mt = (id: string, schema: string, name: string) =>
     `${m(id)}/tables/${encodeURIComponent(schema)}/${encodeURIComponent(name)}`
-  const s = (id: string, schema: string) => `${p(id)}/schemas/${encodeURIComponent(schema)}`
+  const s = (id: string, schema: string) => `${p(id)}/schemas/${encodeURIComponent(splitCatalog(schema).schema)}`
+  /** 合成的 schema 中的库，放进查询参数。 */
+  const cat = (schema: string) => splitCatalog(schema).catalog
   const q = (params: Record<string, string | undefined>) => {
     const entries = Object.entries(params).filter((e): e is [string, string] => e[1] !== undefined && e[1] !== '')
     return entries.length ? `?${new URLSearchParams(entries)}` : ''
@@ -1024,16 +1082,23 @@ export function createApi(token: string, fetchFn: FetchFn = (i, o) => fetch(i, o
     deleteProfile: (id: string) => request<null>('DELETE', p(id)),
     testConnection: (profile: ProfileInput, password: string) =>
       request<ServerSummary>('POST', '/profiles/test', { profile, password }),
-    schemas: (id: string, filter?: string) => request<SchemaInfo[]>('GET', `${p(id)}/schemas${q({ filter })}`),
+    /** 库列表（只有 SQL Server 有这一层，其他驱动为空）。 */
+    catalogs: (id: string, filter?: string) => request<SchemaInfo[]>('GET', `${p(id)}/catalogs${q({ filter })}`),
+    /** `catalog`：SQL Server 中列出哪个库的架构，不给时是连接的初始库。 */
+    schemas: (id: string, filter?: string, catalog?: string) =>
+      request<SchemaInfo[]>('GET', `${p(id)}/schemas${q({ filter, catalog })}`),
     objects: (id: string, schema: string, kind: ObjectKind, filter?: string) =>
-      request<ObjectInfo[]>('GET', `${s(id, schema)}/objects${q({ kind, filter })}`),
+      request<ObjectInfo[]>('GET', `${s(id, schema)}/objects${q({ kind, filter, catalog: cat(schema) })}`),
     table: async (id: string, schema: string, table: string) =>
-      (await request<TableInfo[]>('GET', `${s(id, schema)}/tables${q({ table })}`))[0] ?? null,
-    sequences: (id: string, schema: string) => request<SequenceInfo[]>('GET', `${s(id, schema)}/sequences`),
-    routines: (id: string, schema: string) => request<RoutineInfo[]>('GET', `${s(id, schema)}/routines`),
-    synonyms: (id: string, schema: string) => request<SynonymInfo[]>('GET', `${s(id, schema)}/synonyms`),
+      (await request<TableInfo[]>('GET', `${s(id, schema)}/tables${q({ table, catalog: cat(schema) })}`))[0] ?? null,
+    sequences: (id: string, schema: string) =>
+      request<SequenceInfo[]>('GET', `${s(id, schema)}/sequences${q({ catalog: cat(schema) })}`),
+    routines: (id: string, schema: string) =>
+      request<RoutineInfo[]>('GET', `${s(id, schema)}/routines${q({ catalog: cat(schema) })}`),
+    synonyms: (id: string, schema: string) =>
+      request<SynonymInfo[]>('GET', `${s(id, schema)}/synonyms${q({ catalog: cat(schema) })}`),
     ddl: async (id: string, schema: string, kind: DdlKind, name: string) =>
-      (await request<{ ddl: string }>('GET', `${s(id, schema)}/ddl${q({ kind, name })}`)).ddl,
+      (await request<{ ddl: string }>('GET', `${s(id, schema)}/ddl${q({ kind, name, catalog: cat(schema) })}`)).ddl,
     generate: async (id: string, schema: string, g: GenerateParams) =>
       (
         await request<{ sql: string }>(
@@ -1042,17 +1107,19 @@ export function createApi(token: string, fetchFn: FetchFn = (i, o) => fetch(i, o
             object: g.object,
             object_kind: g.objectKind,
             kind: g.kind,
-            ref_schema: g.ref?.schema,
+            ref_schema: g.ref && splitCatalog(g.ref.schema).schema,
             ref_table: g.ref?.table,
+            catalog: cat(schema),
           })}`,
         )
       ).sql,
     relations: (id: string, schema: string, table: string) =>
-      request<Relation[]>('GET', `${s(id, schema)}/relations${q({ table })}`),
+      request<Relation[]>('GET', `${s(id, schema)}/relations${q({ table, catalog: cat(schema) })}`),
     dataSql: async (id: string, schema: string, d: DataSqlParams) =>
-      (await request<{ sql: string }>('POST', `${s(id, schema)}/data-sql`, d)).sql,
+      (await request<{ sql: string }>('POST', `${s(id, schema)}/data-sql${q({ catalog: cat(schema) })}`, d)).sql,
     /** 清掉后端的元数据缓存；不给 schema 时清整个连接的。 */
-    refresh: (id: string, schema: string | null) => request<null>('POST', `${p(id)}/refresh`, { schema }),
+    refresh: (id: string, schema: string | null) =>
+      request<null>('POST', `${p(id)}/refresh`, { schema: schema === null ? null : splitCatalog(schema).schema }),
     history: (p: HistoryParams = {}) =>
       request<HistoryItem[]>(
         'GET',
@@ -1073,9 +1140,9 @@ export function createApi(token: string, fetchFn: FetchFn = (i, o) => fetch(i, o
     deleteFavorite: (id: string) => request<null>('DELETE', `/favorites/${encodeURIComponent(id)}`),
     ops: (id: string) => request<OpsQuery[]>('GET', `${p(id)}/ops`),
     previewChanges: (id: string, set: ChangeSet) =>
-      request<{ index: number; sql: string }[]>('POST', `${p(id)}/changes/preview`, set),
+      request<{ index: number; sql: string }[]>('POST', `${p(id)}/changes/preview`, changeBody(set)),
     applyChanges: (sessionId: number, set: ChangeSet) =>
-      request<ApplyOutcome>('POST', `/sessions/${sessionId}/changes`, set),
+      request<ApplyOutcome>('POST', `/sessions/${sessionId}/changes`, changeBody(set)),
     startExport: async (req: ExportRequest) => (await request<{ id: string }>('POST', '/exports', req)).id,
     exportStatus: (id: string) => request<ExportStatus>('GET', `/exports/${encodeURIComponent(id)}`),
     /** 进行中的导出取消；已结束的删除记录。 */
@@ -1201,7 +1268,9 @@ export function createApi(token: string, fetchFn: FetchFn = (i, o) => fetch(i, o
     commit: (id: number) => request<SessionInfo>('POST', `/sessions/${id}/commit`),
     rollback: (id: number) => request<SessionInfo>('POST', `/sessions/${id}/rollback`),
     reconnect: (id: number) => request<SessionInfo>('POST', `/sessions/${id}/reconnect`),
-    setSchema: (id: number, schema: string) => request<SessionInfo>('POST', `/sessions/${id}/schema`, { schema }),
+    /** 切换会话的当前 schema；合成的 schema（SQL Server）切到其中的库。 */
+    setSchema: (id: number, schema: string) =>
+      request<SessionInfo>('POST', `/sessions/${id}/schema`, { schema: sessionSchema(schema) }),
     /** 会话的 WebSocket 地址。浏览器的 WebSocket 不能带请求头，令牌和语言放在查询参数里。 */
     wsUrl: (sessionId: number, loc: { protocol: string; host: string } = window.location) =>
       `${loc.protocol === 'https:' ? 'wss' : 'ws'}://${loc.host}/api/sessions/${sessionId}/ws?token=${encodeURIComponent(token)}&lang=${currentLang()}`,
```

- [ ] **Step 48: 修改 `web/src/shared/dbIcons.ts`**

```diff
--- a/web/src/shared/dbIcons.ts
+++ b/web/src/shared/dbIcons.ts
@@ -33,6 +33,8 @@ export const VARIANT_ICONS: Record<string, DbIconDef> = {
   highgo: mark('HG', '#0091d5'),
   oracle: mark('ORA', '#c74634'),
   dameng: mark('DM', '#c8102e'),
+  // simple-icons 没有微软的产品图标
+  sqlserver: mark('MS', '#cc2927'),
 }
 
 /** 还没连接过、不知道具体变体时，按驱动大类。 */
@@ -41,6 +43,7 @@ const DRIVER_ICONS: Record<DriverId, string> = {
   postgres: 'postgres',
   oracle: 'oracle',
   dameng: 'dameng',
+  sqlserver: 'sqlserver',
 }
 
 export function dbIcon(driver: DriverId, variant?: string | null): DbIconDef {
```

- [ ] **Step 49: 检查**

```bash
export CARGO_INCREMENTAL=0
cargo fmt --all -- --check
cargo clippy --config "source.mirror.registry='sparse+https://rsproxy.cn/index/'" --workspace --all-targets -- -D warnings
DBLENS_LANG=en cargo test --config "source.mirror.registry='sparse+https://rsproxy.cn/index/'" --workspace
```

在 PowerShell 中执行（bash 里 `cd web` 会触发坏掉的 fnm 钩子，`&&` 后面的命令会被跳过）：

```powershell
$env:PATH = "D:\Environments\nvm4w\nodejs;$env:PATH"; Set-Location web; pnpm install; pnpm typecheck; pnpm test; pnpm build
```

Expected: 全部通过（前端新增 T-SQL 解析、库节点、ER 图的测试）。

- [ ] **Step 50: 浏览器实测**

浏览器实测（设计文档 10 节）：用 `dblens-server --listen 127.0.0.1:<端口> --data-dir <临时目录> --web-dir web/dist --no-update-check` 启动浏览器模式，按 `DBLENS_TEST_SQLSERVER_URL` 建一个"生产"环境的 SQL Server 连接（可用接口 `POST /api/profiles`，驱动名 `sqlserver`）。
1. 展开连接：列出各库，系统库标"系统"；展开 `dblens_test` → 架构 → 表。
2. 新建查询，库选 `dblens_test`，执行带 `PRINT`、两个 SELECT、`GO`、`GO 2` 和一条查不存在的表的脚本：结果 1–4 各自显示，消息页有 PRINT 的内容和各批耗时，错误标在原文的行上。
3. 打开表数据：中文正常；改一个单元格提交，预览的语句带 `N''` 和原值条件，生产环境要求确认；不带 WHERE 的 UPDATE 要求确认。
4. 执行计划显示 SHOWPLAN 的表格；之后正常执行得到的是数据而不是计划。
5. 右键架构和表：页面不崩溃，设计表、导入、比对、另存为模型、导出文档、迁移置灰；查看 DDL 的标题是 `库.架构.表`；"生成 SQL → INSERT"跳过计算列；ER 图画出外键连线、没有误报的图外引用。
6. 运维的会话列表能执行；新建连接选 SQL Server：认证方式切换时用户名密码随之隐藏，SSL/TLS 页有"信任服务器证书"。

- [ ] **Step 51: 提交**

```bash
git add .gitignore web/grammars/tsql/README.md web/grammars/tsql/TSqlLexer.g4 web/grammars/tsql/TSqlParser.g4 web/package.json web/scripts/gen-tsql.mjs web/src/features/compare/CompareView.tsx web/src/features/connections/ConnectionDialog.tsx web/src/features/connections/form.test.ts web/src/features/connections/form.ts web/src/features/connections/products.ts web/src/features/data/DataView.tsx web/src/features/data/count.ts web/src/features/editor/QueryView.tsx web/src/features/editor/Toolbar.tsx web/src/features/editor/dialect.ts web/src/features/editor/monaco.ts web/src/features/editor/runner.ts web/src/features/er/ErView.tsx web/src/features/er/graph.test.ts web/src/features/er/graph.ts web/src/features/import/SqlFileDialog.tsx web/src/features/migrate/MigrateView.tsx web/src/features/model/ModelDialogs.tsx web/src/features/sql/completion.ts web/src/features/sql/dialects.test.ts web/src/features/sql/dialects.ts web/src/features/sql/functions.ts web/src/features/sql/metadata.test.ts web/src/features/sql/metadata.ts web/src/features/sql/parsers.ts web/src/features/sql/provider.ts web/src/features/sql/tsql/parser.test.ts web/src/features/sql/tsql/parser.ts web/src/features/sql/types.ts web/src/features/tree/DdlDialog.tsx web/src/features/tree/ObjectTree.tsx web/src/features/tree/nodes.test.ts web/src/features/tree/nodes.ts web/src/locales/en/connections.json web/src/locales/en/editor.json web/src/locales/en/tree.json web/src/locales/zh-CN/connections.json web/src/locales/zh-CN/editor.json web/src/locales/zh-CN/tree.json web/src/shared/api.test.ts web/src/shared/api.ts web/src/shared/dbIcons.test.ts web/src/shared/dbIcons.ts
git commit -m "feat(web): SQL Server 连接、按库浏览、T-SQL 语法检查与补全、M6b 功能入口置灰" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 5: 文档

- `docs/notes/m6a-sqlserver.md`（新）：结构、与设计文档不一致的地方、取消实验的结论、已知限制、验证。
- `docs/notes/m0-4b-api.md`：库列表接口、`catalog` 参数、改动集的 `catalog`。
- `vendor/PATCHES.md`：补上 `token.rs` 的改动位置。
- 总设计文档：国产库第二批取消、M6 拆分，M6a 完成。

**Files:**
- Modify: `docs/notes/m0-4b-api.md`
- Create: `docs/notes/m6a-sqlserver.md`
- Modify: `docs/superpowers/specs/2026-10-22-dblens-design.md`
- Modify: `vendor/PATCHES.md`

- [ ] **Step 1: 修改 `docs/notes/m0-4b-api.md`**

```diff
--- a/docs/notes/m0-4b-api.md
+++ b/docs/notes/m0-4b-api.md
@@ -87,7 +87,8 @@
 
 | 方法 | 路径 | 说明 |
 |---|---|---|
-| GET | `/profiles/{id}/schemas?filter=` | schema 列表 |
+| GET | `/profiles/{id}/catalogs?filter=` | 数据库列表（M6a 加入，只有 SQL Server 有这一层，其他驱动返回空数组）：`[{name, is_system}]`；有权访问的在线库，系统库排在后面 |
+| GET | `/profiles/{id}/schemas?filter=&catalog=` | schema 列表 |
 | GET | `…/schemas/{schema}/objects?kind=tables\|views\|materialized_views&filter=` | 对象树节点 |
 | GET | `…/schemas/{schema}/tables?table=` | 表明细；不给 `table` 时取整个 schema |
 | GET | `…/schemas/{schema}/views?kind=views\|materialized_views` | 视图及其列 |
@@ -102,6 +103,8 @@
 | POST | `/profiles/{id}/refresh` | 刷新元数据缓存，`{"schema": "…" \| null}` |
 | GET | `/profiles/{id}/ops` | 运维查询（M1c3）：`[{key, name, category, sql}]`，`name` 按请求的语言，`category` 为 `session` / `lock` / `storage` / `sql`。按连接上次识别出的变体选模板，没连接过时按驱动；不连接数据库 |
 
+- SQL Server（M6a）：上面的元数据接口都可加 `catalog=库名`，在该库中查询（元数据连接按库区分）；不给时用连接的默认库。
+
 ### 会话
 
 | 方法 | 路径 | 说明 |
@@ -147,7 +150,7 @@
 | POST | `/profiles/{id}/changes/preview` | 预览改动集生成的语句，返回 `[{index, sql}]`（`sql` 中的值写成字面量），不执行 |
 | POST | `/sessions/{id}/changes` | 在会话中执行改动集，返回 `{applied, failed}`。失败时已整体回滚，`failed` 为 `{index, error}`（仍是 200）；会话正忙时 409 |
 
-- 改动集：`{schema, table, changes: [...]}`，每个改动按 `kind` 区分：
+- 改动集：`{schema, table, catalog?, changes: [...]}`（`catalog` 为 SQL Server 的库名，M6a 加入），每个改动按 `kind` 区分：
   - `{"kind": "update", "key": {列: 值}, "original": {列: 原值}, "values": {列: 新值}}`；
   - `{"kind": "insert", "values": {列: 值}}`；
   - `{"kind": "delete", "key": {列: 值}}`。
```

- [ ] **Step 2: 新建 `docs/notes/m6a-sqlserver.md`**

````markdown
# M6a SQL Server：连接、浏览、查询

依据：设计文档 `docs/superpowers/specs/2026-10-29-m6a-sqlserver-design.md`。

## 结构

```text
crates/dblens-driver-sqlserver/       新驱动（tiberius 0.13）
  src/lib.rs     URL 解析成 tiberius 的 Config（认证、实例名、TLS、信任服务器证书）、连接（Azure 重定向重连一次）
  src/actor.rs   后台任务独占连接，命令走 mpsc，结果走有界通道（256）；取消用 Notify；
                 无参数时整批 simple_query（USE、临时表留在会话里），有参数时 sp_executesql
  src/conn.rs    Connection 实现：手动提交时 BEGIN TRANSACTION，提交/回滚前看 @@TRANCOUNT
  src/exec.rs    执行流：连续的行合并成一批
  src/types.rs   列类型和值的转换
  src/error.rs   错误号、批内行号换算成位置
vendor/tiberius/                      tiberius 0.13.0 的补丁（见 vendor/PATCHES.md）：
                                      QueryItem::Done / Info（影响行数和 PRINT 消息不丢）、取消后丢弃到 attention 确认为止
crates/dblens-dialect/
  src/variant.rs ident.rs literal.rs  SqlServer 家族：方括号、@P1 占位符、N'' 字符串、0x 二进制、OFFSET FETCH
  src/split.rs   GO 分批（单独一行、可带次数和行尾注释；字符串、方括号、嵌套注释里的不算；次数上限 1000）
  src/ddl.rs types.rs design/catalog.rs migrate/*  DDL 生成、类型映射；表设计器和迁移的入口报"暂不支持"
  sql/sqlserver.sql  元数据模板（含 catalogs）；sql/ops/sqlserver.sql 运维查询
crates/dblens-service/
  src/profile.rs   in_catalog、scope（连接 id + 库名）
  src/metadata.rs  元数据池和缓存按 scope 区分；catalogs()
  src/policy.rs    classify_batch（危险语句）
  src/service.rs   feature_profile：M6b 的功能对 SQL Server 报 sqlserver.later
  src/session.rs   执行计划的收尾语句（SET SHOWPLAN_ALL OFF）
crates/dblens-api/src/routes.rs  元数据接口加 ?catalog=，GET /profiles/{id}/catalogs
web/grammars/tsql/                    grammars-v4 的 T-SQL 语法（固定提交，MIT）
web/scripts/gen-tsql.mjs              生成解析器到 src/features/sql/tsql/gen/（不提交）
web/src/features/sql/tsql/parser.ts   GO 分批、语法检查、补全（三段名、别名）
web/src/features/tree/                库 → 架构 → 对象
web/src/shared/api.ts                 库名和架构合成一个字符串（joinCatalog / splitCatalog / schemaLabel）、supportsWrites
web/src/features/connections/         认证方式、实例名、信任服务器证书
```

## 与设计文档不一致的地方

- 按库取元数据不用模板函数 `{{ catalog() }}` 拼三段名，而是元数据连接池按库区分，连接时指定库（`in_catalog`）；
  模板里只写 `sys.tables` 这类当前库的视图，库名不进入 SQL 文本。
- TLS 只有三档：关闭（`EncryptionLevel::Off`，登录报文仍加密）、必须（`Required`，不校验证书）、校验证书
  （`Required`，打开"信任服务器证书"时不校验，否则用指定的 CA）。DBLens 的 TLS 选项本来就没有"优先"。
- Windows 认证（指定账号和当前用户）只在 Windows 上可用：tiberius 在其他平台走 sspi-rs，其依赖 sha1 只有预发布版，
  不符合依赖原则。其他平台选了会报 `sqlserver.windows_auth_only`。表单在所有平台都显示这两项并加提示。
- 过程式代码等解析不了的批，按关键字扫描时只看 DROP、TRUNCATE、DELETE、UPDATE 和 SQL Server 特有的几个，不含 MERGE
  （与其他库一致，MERGE 按 DML 处理）。没有分号分隔的多条语句 sqlparser 也解析不了，同样走关键字扫描，宁可多问。
- 运维页面与其他库相同（会话、运行中、锁、存储、大表、慢 SQL），没有单独的"服务器参数"查询。
- 浏览表数据与其他库相同：执行 SELECT 后流式分批读取，不生成 `OFFSET … FETCH` 分页。
- 索引的包含列和筛选条件在元数据里列出，但生成 DDL 时不写（M6b 的表设计器再补）。
- 取消：tiberius 0.13 能发 attention 包，但原有的清理按令牌读，遇到被中断的结果集会读乱，下一条查询拿到空结果。
  补丁改为按包丢弃，直到收到带 attention 标志的 DONE 为止。取消后连接保留，`USE` 过的库、临时表、未提交的事务都还在。

## 已知限制

- Windows 认证只在 Windows 上可用（见上）。
- M6b 的功能（表设计器、导入、文档导出、结构对比、迁移、模型）对 SQL Server 置灰，后端报"暂不支持"。
- geometry、geography、hierarchyid、sql_variant 等类型不解析，按二进制或文本显示。
- 编辑器的标签选的架构只影响补全；不带架构的名称按登录用户的默认架构解析（SQL Server 不能在会话中切换默认架构）。
- 刷新只带架构名，同一连接中其他库的同名架构的缓存也一并失效（多刷新，不会少刷新）。
- 服务器的消息语言随登录名的默认语言（中文 Windows 上装的实例默认是简体中文）。语法错误的位置按错误号（102、156）取引号里的原文，不依赖语言；其他错误指向出错的行首。

## 验证

- 后端：fmt、clippy 通过；全部测试在 `DBLENS_LANG=en` 下通过。
  - 方言层：`GO` 分批（字符串、注释、方括号里的 GO，`GO n`、次数上限、行尾注释、行号）、标识符和字面量、类型往返、DDL、危险语句分类、元数据模板快照。
  - 驱动：错误行号换算成位置（含中文消息）。
- 实库（串行）：
  - SQL Server 2022（测试虚拟机 192.168.139.106 上的 Docker）：驱动一致性 17 个（多结果集、影响行数与 PRINT、批内错误位置、读一半丢弃后连接可用、取消、批里自己提交、大字段）、元数据、服务（流程、工作台和执行计划、取消、对象、运维、导出、库和分批与 M6b 拦截、SQL 文件、编辑）、接口。
  - SQL Server 2025 Express（Win11 测试虚拟机 192.168.139.103，中文系统）：上面的 SQL Server 测试全部通过；指定 Windows 账号（NTLM，从开发机连）和当前 Windows 用户（在虚拟机上运行）的登录。
  - 回归：原有 12 个实库目标加 SQL Server 2022 一起串行跑全部后端测试，796 个通过（约 9 分钟；达梦偶发的"网络通讯失败"重跑后通过，OceanBase 要等虚拟机开机后初始化完）。
  - SQL Server 2017 没有实测（镜像下载太慢）。
- 前端：tsc、Vitest 286 个（T-SQL 的分批、检查、补全，库节点，合成的 schema，ER 图的外键）、打包通过。
- 浏览器（生产环境的 SQL Server 2022 连接）：库树（系统库标注）；带 PRINT、`GO`、`GO 2` 和错误语句的脚本（4 个结果、消息、错误行）；表数据中文、编辑提交（`N''`、原值条件、生产确认）；不带 WHERE 的 UPDATE 确认；执行计划及之后正常执行；架构和表的右键菜单（M6b 的项置灰）；DDL；生成 INSERT（跳过计算列）；ER 图的外键连线；运维的会话列表；新建连接的认证方式和 TLS 页。
  浏览器实测中发现并修正：驱动名序列化不一致导致界面新建连接被拒、右键菜单崩溃、DDL 和 ER 标题显示内部分隔符、ER 图的外键被当成图外引用。
- 未验证：桌面程序（Tauri）中的 SQL Server 连接；命名实例经 SQL Browser 查端口（测试实例固定了 1433）；Azure SQL 的重定向。
````

- [ ] **Step 3: 修改 `docs/superpowers/specs/2026-10-22-dblens-design.md`**

```diff
--- a/docs/superpowers/specs/2026-10-22-dblens-design.md
+++ b/docs/superpowers/specs/2026-10-22-dblens-design.md
@@ -337,6 +337,8 @@ trait Policy: Send + Sync {
 - 2026-10-28 M5 拆成两份（用户确认）：M5a 结构迁移；M5b 数据迁移（分批迁数据、断点续传、行数和抽样校验，插在 M5a 的建表和建约束之间）。
 - M5a 已完成（设计 `docs/superpowers/specs/2026-10-28-m5a-structure-migration-design.md`，详细计划 `docs/superpowers/plans/2026-10-28-m5a-structure-migration.md`，说明 `docs/notes/m5a-structure-migration.md`）。
 - M5b 已完成（设计 `docs/superpowers/specs/2026-10-28-m5b-data-migration-design.md`，详细计划 `docs/superpowers/plans/2026-10-29-m5b-data-migration.md`，说明 `docs/notes/m5b-data-migration.md`）。M5 全部完成。
+- 2026-10-29 用户决定：国产库第二批取消，兼容 MySQL / PG / Oracle 协议的产品不做产品识别；先做 SQL Server，分两份：M6a 连接、浏览、查询；M6b 表设计器、导入、文档导出、比对、迁移。
+- M6a 已完成（设计 `docs/superpowers/specs/2026-10-29-m6a-sqlserver-design.md`，详细计划 `docs/superpowers/plans/2026-10-29-m6a-sqlserver.md`，说明 `docs/notes/m6a-sqlserver.md`）。M6b 尚未开始。
 
 ## 12. 风险与待验证项
 
```

- [ ] **Step 4: 修改 `vendor/PATCHES.md`**

```diff
--- a/vendor/PATCHES.md
+++ b/vendor/PATCHES.md
@@ -23,7 +23,7 @@ tiberius 的结果流（`QueryStream`）只给出结果集和行：语句的影
 
 | 库 | 上游版本 | 改动 |
 |---|---|---|
-| `tiberius` | 0.13.0 | `src/tds/stream/query.rs`：`QueryItem` 加 `Done(u64)`（设置了 DONE_COUNT 的 DONE、DONEINPROC）和 `Info(InfoMessage)`，`poll_next` 产出它们；`forward_to_metadata` 遇到这两种令牌时停下（不跳过）；其余对 `QueryItem` 的 `match` 补上新分支。新增 `InfoMessage` 结构，`src/result.rs` 导出它。`src/tds/codec/token/token_done.rs` 加 `has_count()`。`src/client/connection.rs`：`cancel_request` 发出取消后改为按包丢弃剩下的响应，直到某条响应以带 DONE_ATTN 的 DONE 令牌结束（新增 `drain_to_attention_ack`，返回 `()`）；原来按令牌读到当前响应末尾，结果集读到一半丢弃时确认会留在连接上，下一个请求得到空结果。原来的 `flush_done_attention`、`is_attention` 不再使用，标了 `allow(dead_code)` |
+| `tiberius` | 0.13.0 | `src/tds/stream/query.rs`：`QueryItem` 加 `Done(u64)`（设置了 DONE_COUNT 的 DONE、DONEINPROC）和 `Info(InfoMessage)`，`poll_next` 产出它们；`forward_to_metadata` 遇到这两种令牌时停下（不跳过）；其余对 `QueryItem` 的 `match` 补上新分支。新增 `InfoMessage` 结构，`src/result.rs` 导出它。`src/tds/codec/token/token_done.rs` 加 `has_count()`。`src/client/connection.rs`：`cancel_request` 发出取消后改为按包丢弃剩下的响应，直到某条响应以带 DONE_ATTN 的 DONE 令牌结束（新增 `drain_to_attention_ack`，返回 `()`）；原来按令牌读到当前响应末尾，结果集读到一半丢弃时确认会留在连接上，下一个请求得到空结果。原来的 `flush_done_attention`（`src/tds/stream/token.rs`）、`is_attention` 不再使用，标了 `allow(dead_code)` |
 
 ## 升级步骤
 
```

- [ ] **Step 5: 检查**

检查文档中的路径和接口与代码一致；按执行时的实际结果更新"验证"一节（测试数、实库与浏览器实测的内容）。

Expected: 无。

- [ ] **Step 6: 提交**

```bash
git add docs/notes/m0-4b-api.md docs/notes/m6a-sqlserver.md docs/superpowers/specs/2026-10-22-dblens-design.md vendor/PATCHES.md
git commit -m "docs: M6a 说明文档、接口与总设计文档更新" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```
