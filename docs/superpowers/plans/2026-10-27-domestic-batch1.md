# 国产库适配第一批 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** TiDB、OceanBase 社区版（MySQL 模式）、openGauss（A、PG 兼容模式，含 SHA256 认证）、人大金仓 KingbaseES V9R2（Oracle、PG 模式）与 MySQL、PostgreSQL 一样支持全部功能，新建连接按产品选。

**Architecture:** 留在现有的 MySQL、PG 家族中：`Dialect` 增加兼容模式，连上后用探测查询读出模式和所基于的 PG 版本；差异写在按变体的元数据、运维模板覆盖文件里，DDL / DML / 拆分只在少数地方按变体或模式分叉；openGauss 的 SHA256 认证通过复制到 `vendor/` 并打补丁的 `tokio-postgres`、`postgres-protocol` 实现。

**Tech Stack:** Rust（axum、tokio、tokio-postgres、mysql_async、minijinja、insta、hmac / sha1 / sha2）；React 19 + TypeScript、antd 6、Monaco、Vitest。

**Spec:** `docs/superpowers/specs/2026-10-27-domestic-batch1-design.md`

## Global Constraints

- 四个库支持全部功能：连接、对象树、查询（拆分、出错位置、执行计划）、运维查询、数据编辑、导入导出、表设计器。
- 人大金仓、openGauss 的 Oracle 兼容模式与 PG 模式都覆盖；不做人大金仓的 MySQL / SQL Server 兼容版、openGauss 的 B / C 模式、OceanBase 的 Oracle 模式、国密 SM3 认证、跨库 SQL 转换。
- 连上后以识别出的变体为准；所选产品只用于显示和默认值。
- `vendor/` 中的补丁只改认证部分，改动和升级步骤记在 `vendor/PATCHES.md`。
- 依赖只用正式发布版，跟随最新；新版发布满 7 天再升级。
- 前端给用户看的文字都用词条（中英文），英文区分单复数；后端错误文字用 `msg!` 词条；日志不翻译。
- 代码注释用中文（SQL 模板沿用英文注释）；文件 UTF-8、LF 换行；前端代码风格：无分号、单引号、两格缩进。
- cargo 命令加 `--config "source.mirror.registry='sparse+https://rsproxy.cn/index/'"`（clippy 写在 `clippy` 之后），并设 `CARGO_INCREMENTAL=0`；后端测试在 `DBLENS_LANG=en` 下运行。
- 前端工具用 PowerShell 执行（`npm`/`npx` 在本机不可用，用 `pnpm exec`）。
- 测试数据库的密码只在命令行中以内联环境变量传入，不写进仓库、报告或提交信息。

## 与设计文档的差异（写计划时实测后调整）

- 所基于的 PG 版本由探测查询读出，`ServerInfo` 不加字段。
- openGauss 的 MD5_SHA256 不实现：服务端的 MD5 摘要包含存储串末尾的迭代次数后缀，客户端收不到；驱动报出明确的错误。
- 兼容库类型不另查 `pg_type`：用 tokio-postgres 已解析的类型信息（域取基础类型）加类型名对应。
- 同义词与包在两种模式下都列出。openGauss 读不到序列的缓存大小；OceanBase 的全文索引不列出。
- TiDB 删除或更换主键一律重建；检查约束按只解析不执行提示。
- 插入时有默认值的列留空不写：M2a 已有，不改。
- 编辑器在 Oracle 兼容模式下按 PL/SQL 处理；在 Oracle 模式中写 `$$` 函数时编辑器选当前语句可能不准（后端拆分正确）。
- 人大金仓开发版授权最多 10 个连接、不能建物化视图；openGauss 的慢 SQL 需要 MONADMIN。

## 执行说明

任务 2–12 的代码都已在临时副本中逐任务实现并通过检查（含原有 5 个库与新增 7 个目标的实库测试），执行时按顺序重放。新文件给出全文，修改的文件给出 diff（基于上一个任务完成后的状态，可直接 `git apply`）。任务 3 中复制自上游的文件不给全文，给复制命令和相对上游的 diff。`Cargo.lock` 的变化由 cargo 生成；`web/pnpm-lock.yaml` 不变。

浏览器实测在执行时做（设计文档 9.3 节）：每个产品新建连接一次（默认端口、提示、测试结果中的兼容模式），对象树、查询（含 Oracle 模式的过程块）、数据编辑、设计器各操作一遍。

## 文件结构

```text
vendor/{tokio-postgres, postgres-protocol}/, vendor/PATCHES.md; Cargo.toml; .gitignore   openGauss 的 SHA256 认证（新增）
crates/dblens-dialect/src/{variant.rs, lib.rs, split.rs, ident.rs, changes.rs, import.rs, ops.rs, meta/template.rs, meta/assemble.rs, design/{catalog.rs, sql.rs, postgres.rs, rebuild.rs}}
crates/dblens-dialect/sql/{tidb.sql, oceanbase.sql, kingbase.sql, opengauss.sql}, sql/ops/{tidb.sql, oceanbase.sql, opengauss.sql}（新增）; sql/postgres.sql
crates/dblens-dialect/tests/{meta_live.rs, meta_snapshots.rs, fixtures/opengauss.sql, snapshots/}
crates/dblens-driver-postgres/src/types.rs, tests/conformance.rs; crates/dblens-driver-mysql/src/error.rs, tests/conformance.rs
crates/dblens-service/src/{registry.rs, session.rs, sqlfile.rs, profile.rs, store.rs}, tests/live.rs; crates/dblens-api/tests/live.rs
crates/dblens-core/locales/app.json
web/src/features/connections/{products.ts（新增）, form.ts, ConnectionDialog.tsx}; features/sql/{dialects.ts, provider.ts, checker.ts}; features/editor/{dialect.ts, QueryView.tsx}; features/data/DataView.tsx; features/design/OptionsPane.tsx; shared/api.ts; locales/{zh-CN,en}/{connections.json, design.json, editor.json}
docs/notes/{domestic-batch1.md, m0-4b-api.md}; docs/superpowers/specs/2026-10-22-dblens-design.md
```

---

### Task 1: 依赖复查

按依赖原则（M1b2 设计文档 3.1 节）复查前端和后端的全部直接依赖。本里程碑没有新增直接依赖；任务 3 复制进 `vendor/` 的 `postgres-protocol` 多依赖 `sha1 = "0.11"`（`Cargo.lock` 中已有）。

**Files:** 视复查结果而定（`web/package.json`、`Cargo.toml` 及锁文件）。

- [ ] **Step 1: 运行复查脚本**

本机经代理上网，Node 的 `fetch` 默认不读 `HTTPS_PROXY`，必须先设 `NODE_USE_ENV_PROXY=1`，否则全部查询失败，最后仍显示"0 个可升级"。

```powershell
$env:PATH = "D:\Environments\nvm4w\nodejs;$env:PATH"; $env:NODE_USE_ENV_PROXY = '1'; $env:HTTPS_PROXY = 'http://127.0.0.1:6789'; node scripts/check-deps.mjs
```

Expected：输出中没有"查询失败"。2026-10-27 写计划时：共 80 个直接依赖，0 个可升级。`tokio-postgres` 0.7.18、`postgres-protocol` 0.6.12 是 crates.io 上的最新版；以执行当天的结果为准。如果这两个库有新版本，先按 `vendor/PATCHES.md`（任务 3 建立）的步骤处理。

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

Expected：全部通过。

- [ ] **Step 3: 提交（有改动时）**

```bash
git add web/package.json web/pnpm-lock.yaml Cargo.toml Cargo.lock
git commit -m "chore: 依赖复查：升级到发布满 7 天的最新正式版" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 2: 方言：兼容模式与所基于的 PG 版本

- `variant.rs`：`Compat::{Native, Oracle}`（serde `native` / `oracle`）；`Compat::from_mode`（人大金仓的 `oracle`、openGauss 的 `A` 是 Oracle 模式，其余按原生）、`Compat::is_adapted_mode`（`oracle`、`a`、`pg`）。
- `Dialect` 加 `compat` 字段；`with_compat`（只对人大金仓、openGauss 生效）、`compat()`、`oracle_mode()`。
- `refine`：人大金仓、openGauss 读探测结果的 `mode`、`version_num` 两列；`version_num` 是 `server_version_num`，换成 `parse_version` 的编码（10 起 `major*10000 + minor`），替换由产品版本号解析出的版本（人大金仓的 `V009R002` 会被解析成 9.2）。
- 新增覆盖文件 `sql/kingbase.sql`、`sql/opengauss.sql`，先只有 `variant_probe`；`meta/template.rs` 登记两个文件，模板上下文加布尔值 `oracle_mode`（`render` 多一个参数）。
- 服务层：`DriverRegistry::connect` 记下探测到的模式名；`ServerSummary` 加 `compat`、`mode`、`unadapted_mode`。

**Files:**
- Create: `crates/dblens-dialect/sql/kingbase.sql`
- Create: `crates/dblens-dialect/sql/opengauss.sql`
- Modify: `crates/dblens-dialect/src/lib.rs`
- Modify: `crates/dblens-dialect/src/meta/template.rs`
- Modify: `crates/dblens-dialect/src/variant.rs`
- Modify: `crates/dblens-service/src/registry.rs`

**Interfaces:**
- Produces：
  - `dblens_dialect::Compat::{Native, Oracle}`、`Compat::from_mode(&str) -> Compat`、`Compat::is_adapted_mode(&str) -> bool`；
  - `Dialect::with_compat(self, Compat) -> Self`、`compat(&self) -> Compat`、`oracle_mode(&self) -> bool`；
  - crate 内：`meta::render(key, template, family, version, oracle_mode: bool, args)`；
  - `dblens_service::ServerSummary { …, compat: Compat, mode: Option<String>, unadapted_mode: bool }`。

- [ ] **Step 1: 新建 `crates/dblens-dialect/sql/kingbase.sql`**

```sql
-- KingbaseES overrides on top of postgres.sql (only keys that differ). Tested on V9R2C16 in Oracle and PG mode.

-- [variant_probe]
-- Compatibility mode (oracle / pg / mysql / sqlserver) and the PostgreSQL version the build is based on.
SELECT current_setting('database_mode') AS mode,
       current_setting('server_version_num') AS version_num
```

- [ ] **Step 2: 新建 `crates/dblens-dialect/sql/opengauss.sql`**

```sql
-- openGauss overrides on top of postgres.sql (only keys that differ). Based on PG 9.2; tested on 7.0.0-RC1
-- with databases in A (Oracle-compatible) and PG compatibility.

-- [variant_probe]
-- Compatibility of the current database (A / B / C / PG) and the PostgreSQL version the build is based on.
SELECT datcompatibility AS mode,
       current_setting('server_version_num') AS version_num
FROM pg_catalog.pg_database
WHERE datname = current_database()
```

- [ ] **Step 3: 修改 `crates/dblens-dialect/src/lib.rs`**

```diff
--- a/crates/dblens-dialect/src/lib.rs
+++ b/crates/dblens-dialect/src/lib.rs
@@ -27,29 +27,38 @@ pub use import::{ImportStatement, MAX_PARAMS_PER_STATEMENT, MAX_ROWS_PER_STATEME
 pub use ops::{OpsCategory, OpsQuery};
 pub use session::may_change_schema;
 pub use split::{SqlStatement, StreamSplitter, split_statements};
-pub use variant::{Family, Variant};
+pub use variant::{Compat, Family, Variant};
 pub use version::parse_version;
 
 use meta::{MetaArgs, MetaKey, MetaQuery, MetaRows};
 
-/// 某个具体数据库的方言：变体和服务器版本。可以随意复制；所有方法都是纯计算。
+/// 某个具体数据库的方言：变体、服务器版本和兼容模式。可以随意复制；所有方法都是纯计算。
 #[derive(Debug, Clone, Copy, PartialEq, Eq)]
 pub struct Dialect {
     variant: Variant,
-    /// 编码后的服务器版本（见 `parse_version`），未知时为 None。
+    /// 编码后的服务器版本（见 `parse_version`），未知时为 None。人大金仓、openGauss 是所基于的 PG 版本。
     version: Option<u32>,
+    compat: Compat,
 }
 
 impl Dialect {
     /// 版本未知的方言：元数据模板按最新版本渲染。
     pub fn new(variant: Variant) -> Self {
-        Self { variant, version: None }
+        Self { variant, version: None, compat: Compat::Native }
     }
 
     pub fn with_version(self, version: Option<u32>) -> Self {
         Self { version, ..self }
     }
 
+    /// 指定兼容模式；只有人大金仓、openGauss 会是 `Oracle`，其他变体忽略。
+    pub fn with_compat(self, compat: Compat) -> Self {
+        match self.variant {
+            Variant::Kingbase | Variant::OpenGauss => Self { compat, ..self },
+            _ => self,
+        }
+    }
+
     /// 根据驱动和服务器版本识别兼容库和版本号。
     pub fn detect(driver: DriverId, info: &ServerInfo) -> Self {
         Self::new(Variant::detect(driver, info)).with_version(parse_version(&info.version))
@@ -60,15 +69,34 @@ impl Dialect {
         Self::new(Variant::base(driver))
     }
 
-    /// 用 `MetaKey::VariantProbe` 的查询结果细化变体：目前只用于从 MySQL 中识别出 PolarDB
-    /// （`SHOW VARIABLES LIKE 'polardb_version'` 有结果）。
+    /// 用 `MetaKey::VariantProbe` 的查询结果细化方言：
+    /// - MySQL：`SHOW VARIABLES LIKE 'polardb_version'` 有结果即为 PolarDB；
+    /// - 人大金仓、openGauss：`mode` 列给出兼容模式，`version_num` 列（`server_version_num`）给出所基于的
+    ///   PG 版本，替换由产品版本号解析出的版本（人大金仓的 `V009R002` 会被解析成 9.2）。
     pub fn refine(self, probe: &MetaRows) -> Self {
         match self.variant {
             Variant::MySql if !probe.is_empty() => Self { variant: Variant::PolarDb, ..self },
+            Variant::Kingbase | Variant::OpenGauss => match probe.iter().next() {
+                Some(row) => {
+                    let compat = Compat::from_mode(&row.str("mode").unwrap_or_default());
+                    let version = row.int("version_num").and_then(pg_version_num).or(self.version);
+                    Self { version, compat, ..self }
+                }
+                None => self,
+            },
             _ => self,
         }
     }
 
+    pub fn compat(&self) -> Compat {
+        self.compat
+    }
+
+    /// 人大金仓、openGauss 的 Oracle 兼容模式。
+    pub fn oracle_mode(&self) -> bool {
+        self.compat == Compat::Oracle
+    }
+
     pub fn version(&self) -> Option<u32> {
         self.version
     }
@@ -145,11 +173,22 @@ impl Dialect {
     pub fn meta_query(&self, key: MetaKey, args: &MetaArgs) -> Result<Option<MetaQuery>> {
         match meta::sections(self.variant).get(key.as_str()) {
             None => Ok(None),
-            Some(t) => meta::render(key, t, self.family(), self.version.unwrap_or(u32::MAX), args).map(Some),
+            Some(t) => {
+                let version = self.version.unwrap_or(u32::MAX);
+                meta::render(key, t, self.family(), version, self.oracle_mode(), args).map(Some)
+            }
         }
     }
 }
 
+/// 把 PG 的 `server_version_num`（10 起为 `major*10000 + minor`，之前为 `major*10000 + minor*100 + patch`）
+/// 换成 `parse_version` 的编码。
+fn pg_version_num(n: i64) -> Option<u32> {
+    let n = u32::try_from(n).ok()?;
+    let major = n / 10000;
+    Some(if major >= 10 { major * 10000 + (n % 10000).min(99) * 100 } else { n })
+}
+
 /// 跨模块回归：各模块生成的 SQL 经 `Dialect::split` 必须仍是原样的一条语句。
 #[cfg(test)]
 mod tests {
@@ -251,6 +290,48 @@ mod tests {
         assert_eq!(Dialect::new(Variant::MariaDb).refine(&probe).variant(), Variant::MariaDb);
     }
 
+    fn probe(mode: &str, version_num: &str) -> MetaRows {
+        MetaRows::new(
+            vec!["mode".into(), "version_num".into()],
+            vec![vec![Value::String(mode.into()), Value::String(version_num.into())]],
+        )
+    }
+
+    #[test]
+    fn refine_reads_compat_mode_and_pg_version() {
+        let kb = Dialect::new(Variant::Kingbase).with_version(Some(90200));
+        let d = kb.refine(&probe("oracle", "120001"));
+        assert_eq!((d.compat(), d.version()), (Compat::Oracle, Some(120100)));
+        assert!(d.oracle_mode());
+        assert_eq!(kb.refine(&probe("pg", "120001")).compat(), Compat::Native);
+        let og = Dialect::new(Variant::OpenGauss).with_version(Some(70000));
+        let d = og.refine(&probe("A", "90204"));
+        assert_eq!((d.compat(), d.version()), (Compat::Oracle, Some(90204)));
+        assert_eq!(og.refine(&probe("PG", "90204")).compat(), Compat::Native);
+        // 未适配的模式按原生处理；没有探测结果时保持原样
+        assert_eq!(og.refine(&probe("B", "90204")).compat(), Compat::Native);
+        assert_eq!(og.refine(&MetaRows::default()), og);
+        // 其他变体不受探测结果影响
+        let pg = Dialect::new(Variant::Postgres).with_version(Some(180000));
+        assert_eq!(pg.refine(&probe("oracle", "120001")), pg);
+        assert_eq!(pg.with_compat(Compat::Oracle).compat(), Compat::Native);
+    }
+
+    #[test]
+    fn compat_modes() {
+        assert!(
+            Compat::is_adapted_mode("A")
+                && Compat::is_adapted_mode("pg")
+                && Compat::is_adapted_mode("ORACLE")
+        );
+        assert!(!Compat::is_adapted_mode("B") && !Compat::is_adapted_mode("mysql"));
+        assert_eq!(pg_version_num(120001), Some(120100));
+        assert_eq!(pg_version_num(180006), Some(180600));
+        assert_eq!(pg_version_num(90204), Some(90204));
+        assert!(Dialect::new(Variant::Kingbase).has_meta(MetaKey::VariantProbe));
+        assert!(Dialect::new(Variant::OpenGauss).has_meta(MetaKey::VariantProbe));
+    }
+
     #[test]
     fn meta_query_is_none_when_the_dialect_lacks_the_key() {
         let probe =
```

- [ ] **Step 4: 修改 `crates/dblens-dialect/src/meta/template.rs`**

```diff
--- a/crates/dblens-dialect/src/meta/template.rs
+++ b/crates/dblens-dialect/src/meta/template.rs
@@ -19,6 +19,8 @@ const MARIADB: &str = include_str!("../../sql/mariadb.sql");
 const POSTGRES: &str = include_str!("../../sql/postgres.sql");
 const ORACLE: &str = include_str!("../../sql/oracle.sql");
 const DAMENG: &str = include_str!("../../sql/dameng.sql");
+const KINGBASE: &str = include_str!("../../sql/kingbase.sql");
+const OPENGAUSS: &str = include_str!("../../sql/opengauss.sql");
 
 pub(crate) const ALL_VARIANTS: [Variant; 12] = [
     Variant::MySql,
@@ -69,6 +71,8 @@ fn base_file(family: Family) -> &'static str {
 fn override_file(variant: Variant) -> Option<&'static str> {
     match variant {
         Variant::MariaDb => Some(MARIADB),
+        Variant::Kingbase => Some(KINGBASE),
+        Variant::OpenGauss => Some(OPENGAUSS),
         _ => None,
     }
 }
@@ -106,12 +110,14 @@ pub(crate) fn like_pattern(filter: &str) -> String {
     p
 }
 
-/// 渲染一个模板。`version` 是编码后的版本号，未知时传 `u32::MAX`（按最新版本处理）。
+/// 渲染一个模板。`version` 是编码后的版本号，未知时传 `u32::MAX`（按最新版本处理）；
+/// `oracle_mode` 是人大金仓、openGauss 的 Oracle 兼容模式。
 pub(crate) fn render(
     key: MetaKey,
     template: &str,
     family: Family,
     version: u32,
+    oracle_mode: bool,
     args: &MetaArgs,
 ) -> Result<MetaQuery> {
     let names: Arc<Mutex<Vec<String>>> = Arc::default();
@@ -140,6 +146,7 @@ pub(crate) fn render(
             kind => args.kind.is_some(),
             filter => args.filter.is_some(),
             version => version,
+            oracle_mode => oracle_mode,
         })
         .map_err(|e| Error::internal(msg!("meta.template_render", key = k, error = e)))?;
     let params = names
@@ -193,16 +200,17 @@ mod tests {
 
     #[test]
     fn renders_placeholders_per_family_and_binds_in_order() {
-        let q = render(MetaKey::Tables, T, Family::Postgres, 90000, &args()).unwrap();
+        let q = render(MetaKey::Tables, T, Family::Postgres, 90000, false, &args()).unwrap();
         assert_eq!(q.sql, "SELECT x FROM t WHERE s = $1\n  AND n = $2\n  AND new = 1");
         assert_eq!(q.params, vec![Value::String("hr".into()), Value::String("emp".into())]);
-        let q = render(MetaKey::Tables, T, Family::Oracle, 80000, &args()).unwrap();
+        let q = render(MetaKey::Tables, T, Family::Oracle, 80000, false, &args()).unwrap();
         assert_eq!(q.sql, "SELECT x FROM t WHERE s = :1\n  AND n = :2");
         let q = render(
             MetaKey::Tables,
             T,
             Family::MySql,
             80000,
+            false,
             &MetaArgs { schema: Some("hr".into()), ..Default::default() },
         )
         .unwrap();
@@ -218,23 +226,25 @@ mod tests {
             "WHERE x LIKE {{ bind(\"filter\") }} OR y LIKE {{ bind(\"filter\") }}",
             Family::Oracle,
             0,
+            false,
             &a,
         )
         .unwrap();
         assert_eq!(q.sql, "WHERE x LIKE :1 OR y LIKE :2");
         assert_eq!(q.params, vec![Value::String("%a!_b%".into()), Value::String("%a!_b%".into())]);
         // 上下文里的 table 只是布尔值，模板不能借它把参数值拼进 SQL
-        let q = render(MetaKey::Tables, "{{ table }}", Family::MySql, 0, &args()).unwrap();
+        let q = render(MetaKey::Tables, "{{ table }}", Family::MySql, 0, false, &args()).unwrap();
         assert_eq!(q.sql, "True");
         // 未定义的变量直接报错，而不是悄悄渲染成空串
-        assert!(render(MetaKey::Tables, "{{ schema_name }}", Family::MySql, 0, &args()).is_err());
-        assert!(render(MetaKey::Tables, "{{ bind(\"owner\") }}", Family::MySql, 0, &args()).is_err());
+        assert!(render(MetaKey::Tables, "{{ schema_name }}", Family::MySql, 0, false, &args()).is_err());
+        assert!(render(MetaKey::Tables, "{{ bind(\"owner\") }}", Family::MySql, 0, false, &args()).is_err());
     }
 
     #[test]
     fn missing_arguments_bind_null() {
         let q =
-            render(MetaKey::Tables, "{{ bind(\"table\") }}", Family::MySql, 0, &MetaArgs::default()).unwrap();
+            render(MetaKey::Tables, "{{ bind(\"table\") }}", Family::MySql, 0, false, &MetaArgs::default())
+                .unwrap();
         assert_eq!(q.params, vec![Value::Null]);
     }
 }
```

- [ ] **Step 5: 修改 `crates/dblens-dialect/src/variant.rs`**

```diff
--- a/crates/dblens-dialect/src/variant.rs
+++ b/crates/dblens-dialect/src/variant.rs
@@ -17,6 +17,35 @@ pub enum Family {
     Dameng,
 }
 
+/// 兼容模式：人大金仓、openGauss 可以按 Oracle 的规则解释 SQL（空串即 NULL、`NUMBER`、`VARCHAR2`、
+/// 带时间的 `DATE`、Oracle 风格的过程块）。其他变体恒为 `Native`。
+#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
+pub enum Compat {
+    /// 产品自己的语法（人大金仓、openGauss 的 PG 模式也算）。
+    #[default]
+    #[serde(rename = "native")]
+    Native,
+    /// 人大金仓的 `database_mode = oracle`、openGauss 的 `DBCOMPATIBILITY 'A'`。
+    #[serde(rename = "oracle")]
+    Oracle,
+}
+
+impl Compat {
+    /// 由探测到的模式名得出兼容模式：人大金仓的 `oracle`、openGauss 的 `A` 是 Oracle 模式，其余按原生处理。
+    pub fn from_mode(mode: &str) -> Compat {
+        if mode.eq_ignore_ascii_case("oracle") || mode.eq_ignore_ascii_case("a") {
+            Compat::Oracle
+        } else {
+            Compat::Native
+        }
+    }
+
+    /// 已适配的模式名：openGauss 的 B、C 和人大金仓的 mysql、sqlserver 等返回 false。
+    pub fn is_adapted_mode(mode: &str) -> bool {
+        ["oracle", "a", "pg"].iter().any(|m| mode.eq_ignore_ascii_case(m))
+    }
+}
+
 /// 具体的数据库产品。兼容库复用所属家族的规则，只在个别地方覆盖。
 #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
 pub enum Variant {
```

- [ ] **Step 6: 修改 `crates/dblens-service/src/registry.rs`**

```diff
--- a/crates/dblens-service/src/registry.rs
+++ b/crates/dblens-service/src/registry.rs
@@ -8,7 +8,7 @@ use std::sync::Arc;
 
 use dblens_core::{Error, Msg, Result, msg};
 use dblens_dialect::meta::{MetaArgs, MetaKey, MetaRows};
-use dblens_dialect::{Dialect, Variant};
+use dblens_dialect::{Compat, Dialect, Variant};
 use dblens_driver::native::{self, NativeLib};
 use dblens_driver::{ConnectParams, Connection, Driver, DriverId, collect_all};
 use serde::{Deserialize, Serialize};
@@ -42,8 +42,14 @@ pub struct ServerSummary {
     pub version: String,
     pub variant: Variant,
     pub variant_name: Msg,
-    /// 编码后的版本号（见 `dblens_dialect::parse_version`）。
+    /// 编码后的版本号（见 `dblens_dialect::parse_version`）。人大金仓、openGauss 是所基于的 PG 版本。
     pub version_code: Option<u32>,
+    /// 兼容模式（人大金仓、openGauss 可能是 Oracle 模式）。
+    pub compat: Compat,
+    /// 探测到的兼容模式名（人大金仓的 `database_mode`、openGauss 当前库的 `datcompatibility`）。
+    pub mode: Option<String>,
+    /// 模式名不在已适配的范围内（如 openGauss 的 B 模式），界面据此提示。
+    pub unadapted_mode: bool,
 }
 
 fn native_lib(id: DriverId) -> Option<(NativeLib, &'static str)> {
@@ -187,33 +193,47 @@ impl DriverRegistry {
         Ok(params)
     }
 
-    /// 连接并识别方言：`server_info` 识别兼容库和版本，再用 `variant_probe` 细化（如 PolarDB）。
+    /// 连接并识别方言：`server_info` 识别兼容库和版本，再用 `variant_probe` 细化
+    /// （PolarDB；人大金仓、openGauss 的兼容模式和所基于的 PG 版本）。
     pub async fn connect(
         &self,
         profile: &Profile,
         password: String,
     ) -> Result<(Box<dyn Connection>, Dialect)> {
+        self.connect_probed(profile, password).await.map(|(conn, dialect, _)| (conn, dialect))
+    }
+
+    /// 同 `connect`，另外返回探测到的兼容模式名（只有人大金仓、openGauss 有）。
+    async fn connect_probed(
+        &self,
+        profile: &Profile,
+        password: String,
+    ) -> Result<(Box<dyn Connection>, Dialect, Option<String>)> {
         let params = self.connect_params(profile, password)?;
         let driver = self.driver(profile.input.driver)?;
         let mut conn = driver.connect(&params).await?;
         let info = conn.server_info().await?;
         let mut dialect = Dialect::detect(driver.id(), &info);
+        let mut mode = None;
         if let Some(q) = dialect.meta_query(MetaKey::VariantProbe, &MetaArgs::default())? {
             let mut stream = conn.execute(&q.sql, &q.params).await?;
             let rows = MetaRows::from_events(collect_all(stream.as_mut(), 100).await?)?;
             drop(stream);
+            if matches!(dialect.variant(), Variant::Kingbase | Variant::OpenGauss) {
+                mode = rows.iter().next().and_then(|r| r.str("mode"));
+            }
             dialect = dialect.refine(&rows);
         }
         // 记下识别结果供界面显示图标；写不进去不影响连接
         if let Err(e) = self.store.set_profile_variant(&profile.id, dialect.variant()) {
             tracing::warn!("记录连接 {} 的数据库变体失败：{e}", profile.id);
         }
-        Ok((conn, dialect))
+        Ok((conn, dialect, mode))
     }
 
     /// 测试连接：连上、读出服务器信息后立即断开。
     pub async fn test_connection(&self, profile: &Profile, password: String) -> Result<ServerSummary> {
-        let (mut conn, dialect) = self.connect(profile, password).await?;
+        let (mut conn, dialect, mode) = self.connect_probed(profile, password).await?;
         let info = conn.server_info().await?;
         conn.close().await?;
         Ok(ServerSummary {
@@ -222,6 +242,9 @@ impl DriverRegistry {
             variant: dialect.variant(),
             variant_name: dialect.variant().display_name(),
             version_code: dialect.version(),
+            compat: dialect.compat(),
+            unadapted_mode: mode.as_deref().is_some_and(|m| !Compat::is_adapted_mode(m)),
+            mode,
         })
     }
 }
```

- [ ] **Step 7: 检查**

```bash
export CARGO_INCREMENTAL=0
cargo fmt --all
cargo clippy --config "source.mirror.registry='sparse+https://rsproxy.cn/index/'" --workspace --all-targets -- -D warnings
DBLENS_LANG=en cargo test --config "source.mirror.registry='sparse+https://rsproxy.cn/index/'" --workspace
```

Expected: 全部测试通过（dialect 新增 2 个）。

- [ ] **Step 8: 提交**

```bash
git add crates/dblens-dialect/sql/kingbase.sql crates/dblens-dialect/sql/opengauss.sql crates/dblens-dialect/src/lib.rs crates/dblens-dialect/src/meta/template.rs crates/dblens-dialect/src/variant.rs crates/dblens-service/src/registry.rs
git commit -m "feat(dialect): 人大金仓、openGauss 的兼容模式与所基于的 PG 版本" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 3: 驱动：openGauss 的 SHA256 认证与兼容库类型

- 把 crates.io 上的 `tokio-postgres` 0.7.18、`postgres-protocol` 0.6.12 复制到 `vendor/`（`Cargo.toml` 去掉 `[[test]]`、`[[bench]]`），工作区 `Cargo.toml` 加 `exclude = ["vendor"]` 和 `[patch.crates-io]`；`.gitignore` 忽略 `vendor/*/Cargo.lock`。
- 补丁（`vendor/PATCHES.md`）：
  - `postgres-protocol`：新文件 `authentication/gauss.rs`（解析请求码 10 的 openGauss 报文、按 RFC5802 的 openGauss 写法算证明；没有迭代次数时用服务器签名在 10000、2048 中找出）；`AuthenticationSaslBody::data()`；依赖 `sha1 = "0.11"`。
  - `tokio-postgres`：`connect_raw.rs` 的 `authenticate` 中，请求码 10 的报文首字节为 0 时走 openGauss 的 SHA256（或 MD5 存储时走 MD5）；请求码 11 开头的认证（openGauss 的 MD5_SHA256）报出明确的错误。
- 算法已用 Python 对 openGauss 7.0.0-RC1 实测（SHA256 成功；MD5_SHA256 服务端摘要包含迭代次数后缀，无法通过，见设计偏差）。
- PG 驱动（`types.rs`）：兼容库类型按标准类型读写——域类型取基础类型（人大金仓 Oracle 模式的 `date` 是 timestamp 上的域）；按类型名对应 `clob`、`nclob`、`nvarchar2`、`varchar2`、`long`（文本）、`blob`、`raw`（字节）、`number`、`smalldatetime`、`binary_double`、`binary_float`；`int1`、`tinyint` 一个字节；参数编码同样按标准类型（DATE 值写进 timestamp 列时补零点）。`int16`（openGauss 的序列参数）在任务 4 加入。
- 驱动一致性测试：MySQL 加 `tidb`、`oceanbase`；PG 拆出 `pg_cases()` 与 `oracle_mode_cases()`（不测 `'NaN'::numeric`，`DATE` 带时间，加 NUMBER、VARCHAR2、CLOB、Oracle 风格的 DATE），加 `opengauss`、`opengauss_sha`、`opengauss_a`、`kingbase`、`kingbase_pg`。

**Files:**
- Create: `vendor/tokio-postgres/`、`vendor/postgres-protocol/`（复制自 crates.io，见下面的步骤）
- Modify: `.gitignore`
- Modify: `Cargo.toml`
- Modify: `crates/dblens-driver-mysql/tests/conformance.rs`
- Modify: `crates/dblens-driver-postgres/src/types.rs`
- Modify: `crates/dblens-driver-postgres/tests/conformance.rs`
- Create: `vendor/PATCHES.md`
- Create: `vendor/postgres-protocol/src/authentication/gauss.rs`

**Interfaces:**
- Consumes：无。
- Produces：`postgres_protocol::authentication::gauss::{Request::{Sha256 { salt, token, server_signature, iterations }, Md5 { salt }}, parse_request(&[u8]) -> Option<Request>, sha256_proof(password, salt, token, server_signature, iterations) -> Option<String>}`；`AuthenticationSaslBody::data(&self) -> &[u8]`。

- [ ] **Step 1: 复制上游源码**

`$CARGO_HOME/registry/src/` 下有 crates.io 的源码（本机是 `D:/Environments/cargo/registry/src/rsproxy.cn-*`；没有时先 `cargo fetch`）：

```bash
UP=$(ls -d "$CARGO_HOME"/registry/src/*/ | head -1)
mkdir -p vendor/tokio-postgres
cp -r "$UP/tokio-postgres-0.7.18"/{src,Cargo.toml,LICENSE-MIT,LICENSE-APACHE,README.md} vendor/tokio-postgres/
mkdir -p vendor/postgres-protocol
cp -r "$UP/postgres-protocol-0.6.12"/{src,Cargo.toml,LICENSE-MIT,LICENSE-APACHE,README.md} vendor/postgres-protocol/
```

- [ ] **Step 2: 修改 `vendor/postgres-protocol/Cargo.toml`（相对上游）**

```diff
--- a/vendor/postgres-protocol/Cargo.toml
+++ b/vendor/postgres-protocol/Cargo.toml
@@ -63,6 +63,9 @@
 [dependencies.rand]
 version = "0.10"
 
+[dependencies.sha1]
+version = "0.11"
+
 [dependencies.sha2]
 version = "0.11"
 
```

- [ ] **Step 3: 修改 `vendor/postgres-protocol/src/authentication/mod.rs`（相对上游）**

```diff
--- a/vendor/postgres-protocol/src/authentication/mod.rs
+++ b/vendor/postgres-protocol/src/authentication/mod.rs
@@ -2,6 +2,7 @@
 use crate::hex::LowerHexWrapper;
 use md5::{Digest, Md5};
 
+pub mod gauss;
 pub mod sasl;
 
 /// Hashes authentication information in a way suitable for use in response
```

- [ ] **Step 4: 修改 `vendor/postgres-protocol/src/message/backend.rs`（相对上游）**

```diff
--- a/vendor/postgres-protocol/src/message/backend.rs
+++ b/vendor/postgres-protocol/src/message/backend.rs
@@ -360,6 +360,12 @@
     pub fn mechanisms(&self) -> SaslMechanisms<'_> {
         SaslMechanisms(&self.0)
     }
+
+    /// The raw body (DBLens patch): openGauss sends its SHA256 request with the same code as SASL.
+    #[inline]
+    pub fn data(&self) -> &[u8] {
+        &self.0
+    }
 }
 
 pub struct SaslMechanisms<'a>(&'a [u8]);
```

- [ ] **Step 5: 修改 `vendor/tokio-postgres/Cargo.toml`（相对上游）**

```diff
--- a/vendor/tokio-postgres/Cargo.toml
+++ b/vendor/tokio-postgres/Cargo.toml
@@ -74,13 +74,7 @@
 path = "src/lib.rs"
 test = false
 
-[[test]]
-name = "test"
-path = "tests/test/main.rs"
-
-[[bench]]
-name = "bench"
-path = "benches/bench.rs"
+
 harness = false
 
 [dependencies.async-trait]
```

- [ ] **Step 6: 修改 `vendor/tokio-postgres/src/connect_raw.rs`（相对上游）**

```diff
--- a/vendor/tokio-postgres/src/connect_raw.rs
+++ b/vendor/tokio-postgres/src/connect_raw.rs
@@ -191,8 +191,49 @@
             let output = authentication::md5_hash(user.as_bytes(), pass, body.salt());
             authenticate_password(stream, output.as_bytes()).await?;
         }
+        // DBLens patch (vendor/PATCHES.md): openGauss sends its SHA256 request with the SASL code.
+        Some(Message::AuthenticationSasl(body))
+            if authentication::gauss::parse_request(body.data()).is_some() =>
+        {
+            can_skip_channel_binding(config)?;
+            let pass = config
+                .password
+                .as_ref()
+                .ok_or_else(|| Error::config("password missing".into()))?;
+            let output = match authentication::gauss::parse_request(body.data()) {
+                Some(authentication::gauss::Request::Sha256 {
+                    salt,
+                    token,
+                    server_signature,
+                    iterations,
+                }) => authentication::gauss::sha256_proof(
+                    pass,
+                    &salt,
+                    &token,
+                    server_signature.as_deref(),
+                    iterations,
+                )
+                .ok_or_else(|| {
+                    Error::authentication("openGauss SHA256: password does not match".into())
+                })?,
+                Some(authentication::gauss::Request::Md5 { salt }) => {
+                    authentication::md5_hash(user.as_bytes(), pass, salt)
+                }
+                None => unreachable!("checked by the match guard"),
+            };
+            authenticate_password(stream, output.as_bytes()).await?;
+        }
         Some(Message::AuthenticationSasl(body)) => {
             authenticate_sasl(stream, body, config).await?;
+        }
+        // DBLens patch: openGauss MD5_SHA256 (md5 in pg_hba, password stored as SHA256 only). The server
+        // digests its stored string including an iteration suffix the client never receives, so this
+        // cannot succeed; fail with a clear message instead of "unexpected message".
+        Some(Message::AuthenticationSaslContinue(_)) => {
+            return Err(Error::authentication(
+                "openGauss md5 authentication of a password stored as SHA256 is not supported;                  use the sha256 method in pg_hba.conf"
+                    .into(),
+            ));
         }
         Some(Message::AuthenticationKerberosV5)
         | Some(Message::AuthenticationScmCredential)
```

- [ ] **Step 7: 修改 `crates/dblens-driver-mysql/tests/conformance.rs`**

```diff
--- a/crates/dblens-driver-mysql/tests/conformance.rs
+++ b/crates/dblens-driver-mysql/tests/conformance.rs
@@ -70,3 +70,14 @@ mod mariadb {
     use super::*;
     dblens_driver::conformance_tests!("DBLENS_TEST_MARIADB_URL", mariadb_fixture);
 }
+
+mod tidb {
+    use super::*;
+    dblens_driver::conformance_tests!("DBLENS_TEST_TIDB_URL", mysql_fixture);
+}
+
+/// OceanBase 的 MySQL 模式；用户名写成 `用户@租户`（URL 中 `@` 写成 `%40`）。
+mod oceanbase {
+    use super::*;
+    dblens_driver::conformance_tests!("DBLENS_TEST_OCEANBASE_URL", mysql_fixture);
+}
```

- [ ] **Step 8: 修改 `crates/dblens-driver-postgres/tests/conformance.rs`**

```diff
--- a/crates/dblens-driver-postgres/tests/conformance.rs
+++ b/crates/dblens-driver-postgres/tests/conformance.rs
@@ -4,7 +4,7 @@ use dblens_driver::ConnectParams;
 use dblens_driver::conformance::Fixture;
 use dblens_driver_postgres::PgDriver;
 
-fn fixture(params: ConnectParams) -> Fixture {
+fn base(params: ConnectParams, type_cases: Vec<(&'static str, Value)>) -> Fixture {
     Fixture {
         driver: Box::new(PgDriver),
         params,
@@ -18,45 +18,106 @@ fn fixture(params: ConnectParams) -> Fixture {
         select_name_by_id: |t| format!("SELECT name FROM {t} WHERE id = $1"),
         count_rows: |t| format!("SELECT COUNT(*) FROM {t}"),
         missing_table_query: "SELECT * FROM dblens_no_such_table",
-        type_cases: vec![
-            ("SELECT NULL::text", Value::Null),
-            ("SELECT true", Value::Bool(true)),
-            ("SELECT 42::int4", Value::Int(42)),
-            ("SELECT 42::int8", Value::Int(42)),
-            ("SELECT 1.5::float8", Value::Float(1.5)),
-            ("SELECT 123.4500::numeric(10,4)", Value::Decimal("123.4500".into())),
-            (
-                "SELECT 12345678901234567890.123456789::numeric",
-                Value::Decimal("12345678901234567890.123456789".into()),
-            ),
-            ("SELECT 'NaN'::numeric", Value::Decimal("NaN".into())),
-            ("SELECT '数洞'::text", Value::String("数洞".into())),
-            ("SELECT DATE '2024-02-29'", Value::Date(NaiveDate::from_ymd_opt(2024, 2, 29).unwrap())),
-            (
-                "SELECT TIMESTAMP '2024-02-29 13:45:30.123456'",
-                Value::DateTime(
-                    NaiveDate::from_ymd_opt(2024, 2, 29)
-                        .unwrap()
-                        .and_hms_micro_opt(13, 45, 30, 123456)
-                        .unwrap(),
-                ),
-            ),
-            (
-                "SELECT TIMESTAMPTZ '2024-02-29 13:45:30+08'",
-                Value::DateTimeTz(
-                    FixedOffset::east_opt(0).unwrap().with_ymd_and_hms(2024, 2, 29, 5, 45, 30).unwrap(),
-                ),
+        type_cases,
+    }
+}
+
+/// PostgreSQL 的类型用例；人大金仓、openGauss 的 PG 模式也全部适用。
+fn pg_cases() -> Vec<(&'static str, Value)> {
+    vec![
+        ("SELECT NULL::text", Value::Null),
+        ("SELECT true", Value::Bool(true)),
+        ("SELECT 42::int4", Value::Int(42)),
+        ("SELECT 42::int8", Value::Int(42)),
+        ("SELECT 1.5::float8", Value::Float(1.5)),
+        ("SELECT 123.4500::numeric(10,4)", Value::Decimal("123.4500".into())),
+        (
+            "SELECT 12345678901234567890.123456789::numeric",
+            Value::Decimal("12345678901234567890.123456789".into()),
+        ),
+        ("SELECT 'NaN'::numeric", Value::Decimal("NaN".into())),
+        ("SELECT '数洞'::text", Value::String("数洞".into())),
+        ("SELECT DATE '2024-02-29'", Value::Date(NaiveDate::from_ymd_opt(2024, 2, 29).unwrap())),
+        (
+            "SELECT TIMESTAMP '2024-02-29 13:45:30.123456'",
+            Value::DateTime(
+                NaiveDate::from_ymd_opt(2024, 2, 29).unwrap().and_hms_micro_opt(13, 45, 30, 123456).unwrap(),
             ),
-            ("SELECT '\\x010203'::bytea", Value::Bytes(vec![1, 2, 3])),
-            ("SELECT '{\"a\":1}'::jsonb", Value::Json(serde_json::json!({"a": 1}))),
-            (
-                "SELECT 'a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11'::uuid",
-                Value::Uuid("a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11".parse().unwrap()),
+        ),
+        (
+            "SELECT TIMESTAMPTZ '2024-02-29 13:45:30+08'",
+            Value::DateTimeTz(
+                FixedOffset::east_opt(0).unwrap().with_ymd_and_hms(2024, 2, 29, 5, 45, 30).unwrap(),
             ),
-            ("SELECT INTERVAL '1 day 02:03:04'", Value::Interval("0 mons 1 days 02:03:04".into())),
-            ("SELECT 'x'::char(1)::\"char\"", Value::Other { type_name: "char".into(), text: "x".into() }),
-        ],
-    }
+        ),
+        ("SELECT '\\x010203'::bytea", Value::Bytes(vec![1, 2, 3])),
+        ("SELECT '{\"a\":1}'::jsonb", Value::Json(serde_json::json!({"a": 1}))),
+        (
+            "SELECT 'a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11'::uuid",
+            Value::Uuid("a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11".parse().unwrap()),
+        ),
+        ("SELECT INTERVAL '1 day 02:03:04'", Value::Interval("0 mons 1 days 02:03:04".into())),
+        ("SELECT 'x'::char(1)::\"char\"", Value::Other { type_name: "char".into(), text: "x".into() }),
+    ]
+}
+
+/// Oracle 兼容模式（人大金仓、openGauss）：numeric 不接受 'NaN'，`DATE` 带时间；另测 Oracle 风格的类型。
+fn oracle_mode_cases() -> Vec<(&'static str, Value)> {
+    let day = NaiveDate::from_ymd_opt(2024, 2, 29).unwrap();
+    let mut cases: Vec<_> = pg_cases()
+        .into_iter()
+        .filter(|(sql, _)| !sql.contains("'NaN'"))
+        .map(|(sql, v)| match v {
+            Value::Date(d) => (sql, Value::DateTime(d.and_hms_opt(0, 0, 0).unwrap())),
+            v => (sql, v),
+        })
+        .collect();
+    let dt = day.and_hms_opt(13, 45, 30).unwrap();
+    cases.extend([
+        ("SELECT CAST(1.5 AS NUMBER(10,2))", Value::Decimal("1.50".into())),
+        ("SELECT CAST('数洞' AS VARCHAR2(10))", Value::String("数洞".into())),
+        ("SELECT CAST('数洞' AS CLOB)", Value::String("数洞".into())),
+        // Oracle 模式的 DATE 带时间
+        ("SELECT CAST(TIMESTAMP '2024-02-29 13:45:30' AS DATE)", Value::DateTime(dt)),
+    ]);
+    cases
+}
+
+fn pg_fixture(params: ConnectParams) -> Fixture {
+    base(params, pg_cases())
+}
+
+fn oracle_mode_fixture(params: ConnectParams) -> Fixture {
+    base(params, oracle_mode_cases())
 }
 
-dblens_driver::conformance_tests!("DBLENS_TEST_PG_URL", fixture);
+mod postgres {
+    use super::*;
+    dblens_driver::conformance_tests!("DBLENS_TEST_PG_URL", pg_fixture);
+}
+
+mod opengauss {
+    use super::*;
+    dblens_driver::conformance_tests!("DBLENS_TEST_OPENGAUSS_URL", pg_fixture);
+}
+
+/// 只存 SHA256 密码的账号：openGauss 自己的认证消息（vendor/PATCHES.md）。
+mod opengauss_sha {
+    use super::*;
+    dblens_driver::conformance_tests!("DBLENS_TEST_OPENGAUSS_SHA_URL", pg_fixture);
+}
+
+mod opengauss_a {
+    use super::*;
+    dblens_driver::conformance_tests!("DBLENS_TEST_OPENGAUSS_A_URL", oracle_mode_fixture);
+}
+
+mod kingbase {
+    use super::*;
+    dblens_driver::conformance_tests!("DBLENS_TEST_KINGBASE_URL", oracle_mode_fixture);
+}
+
+mod kingbase_pg {
+    use super::*;
+    dblens_driver::conformance_tests!("DBLENS_TEST_KINGBASE_PG_URL", pg_fixture);
+}
```

- [ ] **Step 9: 修改 `.gitignore`**

```diff
--- a/.gitignore
+++ b/.gitignore
@@ -13,3 +13,6 @@ node_modules/
 /crates/dblens-desktop/gen/
 # tauri-build 按 build.rs 中的命令列表自动生成的权限定义
 /crates/dblens-desktop/permissions/autogenerated/
+
+# 单独测试 vendor/ 中的库时生成
+/vendor/*/Cargo.lock
```

- [ ] **Step 10: 修改 `Cargo.toml`**

```diff
--- a/Cargo.toml
+++ b/Cargo.toml
@@ -1,6 +1,8 @@
 [workspace]
 resolver = "3"
 members = ["crates/*"]
+# 打了补丁的第三方库（见 vendor/PATCHES.md），不作为工作区成员检查
+exclude = ["vendor"]
 
 [workspace.package]
 version = "0.1.0"
@@ -70,3 +72,8 @@ tracing-subscriber = "0.3.23"
 url = "2.5.8"
 uuid = { version = "1.28.0", features = ["serde", "v4"] }
 webpki-roots = "1.0.11"
+
+# openGauss 的 SHA256 认证：替换为 vendor/ 中打了补丁的版本（见 vendor/PATCHES.md）
+[patch.crates-io]
+postgres-protocol = { path = "vendor/postgres-protocol" }
+tokio-postgres = { path = "vendor/tokio-postgres" }
```

- [ ] **Step 11: 修改 `crates/dblens-driver-postgres/src/types.rs`**

```diff
--- a/crates/dblens-driver-postgres/src/types.rs
+++ b/crates/dblens-driver-postgres/src/types.rs
@@ -2,7 +2,7 @@ use bytes::BytesMut;
 use chrono::{DateTime, NaiveDate, NaiveDateTime, NaiveTime, Utc};
 use dblens_core::{Error, Msg, Result, Value, msg};
 use tokio_postgres::Row;
-use tokio_postgres::types::{FromSql, IsNull, ToSql, Type, to_sql_checked};
+use tokio_postgres::types::{FromSql, IsNull, Kind, ToSql, Type, to_sql_checked};
 
 type BoxError = Box<dyn std::error::Error + Sync + Send>;
 
@@ -118,6 +118,61 @@ impl<'a> FromSql<'a> for RawValue<'a> {
     }
 }
 
+/// 兼容库自定义的类型对应的标准类型：域类型取基础类型（人大金仓 Oracle 模式的 `date` 是 timestamp 上的域，
+/// `blob`、`clob` 也是域）；openGauss 的 `clob`、`nvarchar2` 等是独立的类型，二进制格式与对应的标准类型相同。
+/// `int1`、`tinyint` 没有对应的标准类型，原样返回，由 `decode_as` 单独处理。
+fn standard_type(ty: &Type) -> Option<Type> {
+    if let Kind::Domain(base) = ty.kind() {
+        return Some(standard_type(base).unwrap_or_else(|| base.clone()));
+    }
+    match ty.name() {
+        "clob" | "nclob" | "nvarchar2" | "varchar2" | "long" => Some(Type::TEXT),
+        "blob" | "raw" => Some(Type::BYTEA),
+        "number" => Some(Type::NUMERIC),
+        "smalldatetime" => Some(Type::TIMESTAMP),
+        "binary_double" => Some(Type::FLOAT8),
+        "binary_float" => Some(Type::FLOAT4),
+        "int1" | "tinyint" => Some(ty.clone()),
+        _ => None,
+    }
+}
+
+/// 按 `standard_type` 给出的类型解码二进制值。
+fn decode_as(ty: &Type, raw: &[u8]) -> std::result::Result<Value, BoxError> {
+    macro_rules! from {
+        ($t:ty, $f:expr) => {
+            <$t as FromSql>::from_sql(ty, raw).map($f)?
+        };
+    }
+    Ok(match *ty {
+        Type::BOOL => from!(bool, Value::Bool),
+        Type::INT2 => from!(i16, |v| Value::Int(v.into())),
+        Type::INT4 => from!(i32, |v| Value::Int(v.into())),
+        Type::INT8 => from!(i64, Value::Int),
+        Type::FLOAT4 => from!(f32, |v| Value::Float(v.into())),
+        Type::FLOAT8 => from!(f64, Value::Float),
+        Type::NUMERIC => Value::Decimal(decode_numeric(raw)?),
+        Type::TEXT | Type::VARCHAR | Type::BPCHAR => from!(String, Value::String),
+        Type::BYTEA => Value::Bytes(raw.to_vec()),
+        Type::DATE => from!(NaiveDate, Value::Date),
+        Type::TIME => from!(NaiveTime, Value::Time),
+        Type::TIMESTAMP => from!(NaiveDateTime, Value::DateTime),
+        Type::TIMESTAMPTZ => from!(DateTime<Utc>, |v| Value::DateTimeTz(v.fixed_offset())),
+        // openGauss 的 int1 是 0–255，人大金仓的 tinyint 是 -128–127，都是 1 个字节
+        _ if raw.len() == 1 && ty.name() == "int1" => Value::Int(raw[0].into()),
+        _ if raw.len() == 1 && ty.name() == "tinyint" => Value::Int((raw[0] as i8).into()),
+        _ => Value::Other { type_name: ty.name().to_string(), text: other_text(raw) },
+    })
+}
+
+/// 不认识的类型：枚举、xml、citext 等类型的二进制格式就是文本，其余显示为十六进制。
+fn other_text(raw: &[u8]) -> String {
+    match std::str::from_utf8(raw) {
+        Ok(s) => s.to_string(),
+        Err(_) => format!("\\x{}", raw.iter().map(|b| format!("{b:02x}")).collect::<String>()),
+    }
+}
+
 /// 读取一行中第 `idx` 列的值。
 pub(crate) fn pg_value(row: &Row, idx: usize) -> Result<Value> {
     let ty = row.columns()[idx].type_().clone();
@@ -150,14 +205,22 @@ pub(crate) fn pg_value(row: &Row, idx: usize) -> Result<Value> {
         Type::INTERVAL => get!(PgInterval, |v| Value::Interval(v.0)),
         _ => {
             let raw = row.try_get::<_, Option<RawValue>>(idx).map_err(fail)?;
-            raw.map(|r| Value::Other {
-                type_name: ty.name().to_string(),
-                // 枚举、xml、citext 等类型的二进制格式就是文本
-                text: match std::str::from_utf8(r.0) {
-                    Ok(s) => s.to_string(),
-                    Err(_) => format!("\\x{}", r.0.iter().map(|b| format!("{b:02x}")).collect::<String>()),
-                },
-            })
+            match (raw, standard_type(&ty)) {
+                (None, _) => None,
+                (Some(r), Some(base)) => Some(decode_as(&base, r.0).map_err(|e| {
+                    let cause =
+                        e.downcast_ref::<Error>().map_or_else(|| Msg::raw(e.to_string()), Error::to_msg);
+                    Error::internal(msg!(
+                        "pg.read_column_failed",
+                        column = idx + 1,
+                        ty = ty.name(),
+                        error = cause
+                    ))
+                })?),
+                (Some(r), None) => {
+                    Some(Value::Other { type_name: ty.name().to_string(), text: other_text(r.0) })
+                }
+            }
         }
     };
     Ok(value.unwrap_or(Value::Null))
```

- [ ] **Step 12: 新建 `vendor/PATCHES.md`**

```markdown
# 打了补丁的第三方库

工作区 `Cargo.toml` 的 `[patch.crates-io]` 用这里的副本替换 crates.io 上的同名库。副本是 crates.io 发布包的原样内容（只删去了 `Cargo.toml` 中的 `[[test]]`、`[[bench]]`，发布包里没有对应的文件），再加上下面列出的改动。目录在 `[workspace] exclude` 中，不是工作区成员，clippy 不检查。

## openGauss 的 SHA256 认证

openGauss 用认证请求码 10（与 PostgreSQL 的 SASL 相同）发它自己的 SHA256 认证，上游的 tokio-postgres 解析失败（"error parsing response from server"）。算法与报文格式见 `postgres-protocol/src/authentication/gauss.rs` 开头的说明。

| 库 | 上游版本 | 改动 |
|---|---|---|
| `postgres-protocol` | 0.6.12 | 新增 `src/authentication/gauss.rs`（解析请求、计算证明，含单元测试）；`src/authentication/mod.rs` 加 `pub mod gauss;`；`src/message/backend.rs` 中 `AuthenticationSaslBody` 加 `data()`；`Cargo.toml` 加依赖 `sha1 = "0.11"` |
| `tokio-postgres` | 0.7.18 | `src/connect_raw.rs` 的 `authenticate`：请求码 10 的报文是 openGauss 格式时走 SHA256（或 MD5 存储时走 MD5）；请求码 11 开头的认证（openGauss 的 MD5_SHA256）直接报错说明不支持 |

不支持的情况：pg_hba 配成 md5、而密码只存了 SHA256 时，openGauss 发请求码 11（MD5_SHA256）。服务端计算 MD5 摘要时包含了存储串末尾的迭代次数后缀，客户端收不到这个后缀，无法通过认证（openGauss 自己的 libpq 也是按不含后缀计算）。补丁让这种情况报出明确的错误，提示改用 sha256。

已知的告警：`tokio-postgres/src/client.rs` 在 Windows 上有一个 unused import（上游代码，只在 unix 下使用），不影响构建。

## 升级步骤

每个里程碑开头的依赖复查发现这两个库有新版本时：

1. 从 crates.io 下载新版本（`cargo download` 或取 `$CARGO_HOME/registry/src/*/<库>-<版本>`），替换 `vendor/<库>/` 中除 `Cargo.toml` 改动以外的全部文件；
2. 按上表重新套用改动（`gauss.rs` 整个文件复制过去；其余是几行的插入）；
3. 更新上表中的上游版本；
4. 运行 `cargo test --manifest-path vendor/postgres-protocol/Cargo.toml --lib authentication`，再用 `DBLENS_TEST_OPENGAUSS_SHA_URL` 跑 PG 驱动的一致性测试。
```

- [ ] **Step 13: 新建 `vendor/postgres-protocol/src/authentication/gauss.rs`**

```rust
//! openGauss password authentication (DBLens patch, see `vendor/PATCHES.md`).
//!
//! openGauss reuses authentication request code 10 (`AUTH_REQ_SHA256`, the same code PostgreSQL uses for
//! SASL) for its own RFC 5802-style exchange. The request body starts with a 4-byte password storage
//! method (a small integer, so its first byte is 0), while a PostgreSQL SASL body starts with a mechanism
//! name; that first byte tells them apart.
//!
//! Body layout for the SHA256 storage method (`2`):
//! - protocol 3.0 (what tokio-postgres sends): 64 hex chars of salt, 8 hex chars of token and the 64-hex
//!   server signature; the iteration count is not sent, so it is found by checking the server signature;
//! - protocol 3.51 and later: salt, token and a 4-byte iteration count.
//!
//! Algorithm (openGauss `RFC5802Algorithm`): K = PBKDF2-HMAC-SHA1(password, salt, iterations, 32);
//! ServerKey = HMAC-SHA256(K, "Sever Key") (sic); ClientKey = HMAC-SHA256(K, "Client Key");
//! StoredKey = SHA256(ClientKey); proof = HMAC-SHA256(StoredKey, token) XOR ClientKey, sent as lowercase
//! hex in a password message. The server signature is HMAC-SHA256(ServerKey, token).
use hmac::{Hmac, KeyInit, Mac};
use sha1::Sha1;
use sha2::{Digest, Sha256};

/// Password stored as plain text: handled like SHA256.
const PLAIN_PASSWORD: i32 = 0;
/// Password stored as MD5: the body carries a 4-byte MD5 salt.
const MD5_PASSWORD: i32 = 1;
/// Password stored as SHA256.
const SHA256_PASSWORD: i32 = 2;

/// Iteration counts tried, in order, when the server does not send one (protocol 3.0): the current
/// openGauss default, then the default of older releases.
const ITERATIONS: [u32; 2] = [10000, 2048];

/// A parsed openGauss authentication request (code 10).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Request {
    /// Password stored as SHA256 (or plain text).
    Sha256 {
        /// 64 hex chars.
        salt: String,
        /// 8 hex chars.
        token: String,
        /// Sent by servers speaking protocol 3.0.
        server_signature: Option<String>,
        /// Sent by servers speaking protocol 3.51 and later.
        iterations: Option<u32>,
    },
    /// Password stored as MD5: answered like a PostgreSQL MD5 request.
    Md5 {
        /// The MD5 salt.
        salt: [u8; 4],
    },
}

/// Parses the body of an authentication request with code 10 when it is an openGauss request; returns
/// `None` for a PostgreSQL SASL body (which starts with a mechanism name) or an unknown storage method.
pub fn parse_request(body: &[u8]) -> Option<Request> {
    if body.len() < 4 || body[0] != 0 {
        return None;
    }
    let method = i32::from_be_bytes([body[0], body[1], body[2], body[3]]);
    let rest = &body[4..];
    match method {
        PLAIN_PASSWORD | SHA256_PASSWORD if rest.len() >= 72 => {
            let salt = std::str::from_utf8(&rest[..64]).ok()?.to_string();
            let token = std::str::from_utf8(&rest[64..72]).ok()?.to_string();
            let tail = &rest[72..];
            let (server_signature, iterations) = if tail.len() >= 64 {
                (Some(std::str::from_utf8(&tail[..64]).ok()?.to_string()), None)
            } else if tail.len() >= 4 {
                (None, Some(u32::from_be_bytes([tail[0], tail[1], tail[2], tail[3]])))
            } else {
                (None, None)
            };
            Some(Request::Sha256 { salt, token, server_signature, iterations })
        }
        MD5_PASSWORD if rest.len() >= 4 => Some(Request::Md5 { salt: [rest[0], rest[1], rest[2], rest[3]] }),
        _ => None,
    }
}

fn hmac_sha256(key: &[u8], data: &[u8]) -> [u8; 32] {
    let mut mac = Hmac::<Sha256>::new_from_slice(key).expect("HMAC is able to accept all key sizes");
    mac.update(data);
    mac.finalize().into_bytes().into()
}

/// PBKDF2-HMAC-SHA1 producing 32 bytes (two SHA1 blocks).
fn pbkdf2_sha1(password: &[u8], salt: &[u8], iterations: u32) -> [u8; 32] {
    let prf = Hmac::<Sha1>::new_from_slice(password).expect("HMAC is able to accept all key sizes");
    let mut out = [0u8; 32];
    for (block, chunk) in out.chunks_mut(20).enumerate() {
        let mut mac = prf.clone();
        mac.update(salt);
        mac.update(&(block as u32 + 1).to_be_bytes());
        let mut u: [u8; 20] = mac.finalize().into_bytes().into();
        let mut t = u;
        for _ in 1..iterations {
            let mut mac = prf.clone();
            mac.update(&u);
            u = mac.finalize().into_bytes().into();
            t.iter_mut().zip(u.iter()).for_each(|(a, b)| *a ^= b);
        }
        chunk.copy_from_slice(&t[..chunk.len()]);
    }
    out
}

fn from_hex(text: &str) -> Option<Vec<u8>> {
    if text.len() % 2 != 0 {
        return None;
    }
    (0..text.len()).step_by(2).map(|i| u8::from_str_radix(text.get(i..i + 2)?, 16).ok()).collect()
}

fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// The server and client keys for a password, salt and iteration count.
fn keys(password: &[u8], salt: &[u8], iterations: u32) -> ([u8; 32], [u8; 32]) {
    let k = pbkdf2_sha1(password, salt, iterations);
    (hmac_sha256(&k, b"Sever Key"), hmac_sha256(&k, b"Client Key"))
}

/// The proof to send for an SHA256 request, as lowercase hex. Without an iteration count the candidates in
/// `ITERATIONS` are checked against the server signature. Returns `None` when the salt or token is not hex
/// or no candidate matches the server signature.
pub fn sha256_proof(
    password: &[u8],
    salt: &str,
    token: &str,
    server_signature: Option<&str>,
    iterations: Option<u32>,
) -> Option<String> {
    let salt = from_hex(salt)?;
    let token = from_hex(token)?;
    let (_, client_key) = match (iterations, server_signature) {
        (Some(n), _) => keys(password, &salt, n),
        (None, Some(signature)) => ITERATIONS.iter().find_map(|&n| {
            let (server_key, client_key) = keys(password, &salt, n);
            (to_hex(&hmac_sha256(&server_key, &token)) == signature.to_ascii_lowercase())
                .then_some((server_key, client_key))
        })?,
        (None, None) => keys(password, &salt, ITERATIONS[0]),
    };
    let stored_key: [u8; 32] = Sha256::digest(client_key).into();
    let signature = hmac_sha256(&stored_key, &token);
    let proof: Vec<u8> = signature.iter().zip(client_key.iter()).map(|(a, b)| a ^ b).collect();
    Some(to_hex(&proof))
}

#[cfg(test)]
mod test {
    use super::*;

    const SALT: &str = "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff";
    // Expected values computed with the reference algorithm and checked against openGauss 7.0.0-RC1.
    const SIG_10000: &str = "b01a9f7346d6a37e43b4e1e976de4750ee7c8d394bfa329287aed4d6cce189fd";
    const PROOF_10000: &str = "48447ffe6e19554775a9a7c1e7e2bd8ef1b656b11ee2e4c790e23cc7a693469e";
    const SIG_2048: &str = "fd51d5b00e8cd9927d44dd6391180b54aadcab0c224da9fd25afb49c2ce31d38";
    const PROOF_2048: &str = "b4d07ffa1ae0c5143768f8bed55079f4ec7ef7fc95954839c661a02a97f80694";

    #[test]
    fn proof_with_iterations_or_server_signature() {
        let pw = b"Gauss-Test-123";
        assert_eq!(sha256_proof(pw, SALT, "0a1b2c3d", None, Some(10000)).as_deref(), Some(PROOF_10000));
        assert_eq!(sha256_proof(pw, SALT, "0a1b2c3d", Some(SIG_10000), None).as_deref(), Some(PROOF_10000));
        assert_eq!(sha256_proof(pw, SALT, "0a1b2c3d", Some(SIG_2048), None).as_deref(), Some(PROOF_2048));
        assert_eq!(sha256_proof(b"wrong", SALT, "0a1b2c3d", Some(SIG_10000), None), None);
        assert_eq!(sha256_proof(pw, "zz", "0a1b2c3d", None, Some(10000)), None);
    }

    #[test]
    fn parses_requests_and_leaves_sasl_alone() {
        let mut v30 = vec![0, 0, 0, 2];
        v30.extend_from_slice(SALT.as_bytes());
        v30.extend_from_slice(b"0a1b2c3d");
        v30.extend_from_slice(SIG_10000.as_bytes());
        assert_eq!(
            parse_request(&v30),
            Some(Request::Sha256 {
                salt: SALT.into(),
                token: "0a1b2c3d".into(),
                server_signature: Some(SIG_10000.into()),
                iterations: None,
            })
        );
        let mut v351 = vec![0, 0, 0, 2];
        v351.extend_from_slice(SALT.as_bytes());
        v351.extend_from_slice(b"0a1b2c3d");
        v351.extend_from_slice(&10000u32.to_be_bytes());
        assert!(matches!(parse_request(&v351), Some(Request::Sha256 { iterations: Some(10000), .. })));
        assert_eq!(parse_request(&[0, 0, 0, 1, 9, 8, 7, 6]), Some(Request::Md5 { salt: [9, 8, 7, 6] }));
        assert_eq!(parse_request(b"SCRAM-SHA-256\0\0"), None);
        assert_eq!(parse_request(&[0, 0, 0, 3, 1]), None);
    }
}
```

- [ ] **Step 14: 检查**

```bash
export CARGO_INCREMENTAL=0
cargo fmt --all
cargo clippy --config "source.mirror.registry='sparse+https://rsproxy.cn/index/'" --workspace --all-targets -- -D warnings
DBLENS_LANG=en cargo test --config "source.mirror.registry='sparse+https://rsproxy.cn/index/'" --workspace
```

```bash
cargo test --config "source.mirror.registry='sparse+https://rsproxy.cn/index/'" --manifest-path vendor/postgres-protocol/Cargo.toml --lib authentication
rm -f vendor/postgres-protocol/Cargo.lock
```

实库测试：在上面的 `cargo test` 前以内联环境变量给出原有 5 个库的 `DBLENS_TEST_MYSQL_URL`、`DBLENS_TEST_MARIADB_URL`、`DBLENS_TEST_PG_URL`、`DBLENS_TEST_ORACLE_URL`、`DBLENS_TEST_DM_URL`，以及第一批的 `DBLENS_TEST_TIDB_URL`、`DBLENS_TEST_OCEANBASE_URL`（用户名写成 `admin%40test`）、`DBLENS_TEST_OPENGAUSS_URL`、`DBLENS_TEST_OPENGAUSS_A_URL`（库 `dblens_test_a`）、`DBLENS_TEST_OPENGAUSS_SHA_URL`（账号 `admin_sha`）、`DBLENS_TEST_KINGBASE_URL`（54321）、`DBLENS_TEST_KINGBASE_PG_URL`（54322）。新目标的密码在测试虚拟机 root 的 `/root/dblens-test/creds.env`，用 `ssh … 'sudo cat …'` 读进当前 shell 的变量，不写进任何文件。
- 人大金仓的开发版授权最多 10 个连接：它的测试用 `-- kingbase --test-threads=1` 单独串行运行。
- 达梦的测试与其他测试同在一个进程里运行时偶发"网络通讯失败"，失败时用 `-- dameng --test-threads=1` 单独重跑。

Expected: 全部测试通过（`gauss` 2 个；PG 驱动一致性测试 6 个目标共 60 个，MySQL 4 个目标共 40 个）。上游的 `client.rs` 在 Windows 上有一个 unused import 告警（vendor 不是工作区成员，不影响 `-D warnings`）。

- [ ] **Step 15: 提交**

```bash
git add .gitignore Cargo.lock Cargo.toml crates/dblens-driver-mysql/tests/conformance.rs crates/dblens-driver-postgres/src/types.rs crates/dblens-driver-postgres/tests/conformance.rs vendor/PATCHES.md vendor/postgres-protocol vendor/tokio-postgres
git commit -m "feat(driver): openGauss 的 SHA256 认证与兼容库类型的读取" -m "tokio-postgres、postgres-protocol 复制到 vendor/ 并打补丁（见 vendor/PATCHES.md）。驱动一致性测试增加 TiDB、OceanBase、openGauss（PG、A 模式、纯 SHA256 账号）、人大金仓（Oracle、PG 模式）。" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 4: 方言：国产库的元数据模板

覆盖文件（只放差异，规则不变：同名 key 替换，正文为空表示没有这种查询）：
- `tidb.sql`：`checks` 从 `TIDB_CHECK_CONSTRAINTS` 读；`tables` 的分区用 LEFT JOIN（相关子查询匹配多行时全是 NULL）；`sequences`；`columns` 认出 `AUTO_RANDOM`；`table_storage` 加 `clustered`。
- `oceanbase.sql`：`checks` 不看版本；`indexes` 去掉全文索引的隐藏列、`descending` 恒为 0；`reserved_words` 为空；`tables` 的分区用 LEFT JOIN（`CREATE_OPTIONS` 为 NULL）。
- `kingbase.sql`：`schemas`（隐藏 sys、sys_*、dbms_* 等）、`synonyms`（`sys_synonym`）、`routines` / `routine_source`（加上 `_pkg` 中的包，源码由规范和包体拼出）。
- `opengauss.sql`：`schemas`、`tables`（分区从 `pg_partition`）、`sequences`（`pg_sequence_parameters`）、`columns`（`adgencol`）、`keys` / `indexes` / `referencing_keys`（`generate_series` 代替 LATERAL + WITH ORDINALITY）、`checks` / `triggers`（去掉 `relispartition`）、`grants`（子查询中 `aclexplode`）、`synonyms`（`pg_synonym`）、`routines` / `routine_source`（排除聚合、包内函数，加上 `gs_package` 中的包）。
- `postgres.sql` 的 `columns`：去掉与空串的比较（Oracle 兼容模式下 `''` 是 NULL）：`attidentity IN ('a', 'd')`（10 起）、`attgenerated = 's'`。
- `ident.rs`：保留字补上 TiDB、人大金仓、openGauss 报告的词。
- `TableStorage.clustered: Option<bool>`（只有 TiDB 的模板有这一列）；PG 驱动读 `int16`。
- 元数据实库测试：`run_script_with`（跳过、替换脚本中的段）；`Fx` 加 `triggers`、`routines`、`descending`；`check_pg_family` + `PgFx`（物化视图、序列缓存）；新目标的测试，`fixtures/opengauss.sql`；Oracle 模式的同义词和包（`check_oracle_mode_objects`）。快照测试覆盖 9 个变体。

快照文件（`tests/snapshots/`）随本任务给出；改了模板时用 `INSTA_UPDATE=always` 重新生成并逐个审查差异。

**Files:**
- Modify: `crates/dblens-dialect/sql/kingbase.sql`
- Create: `crates/dblens-dialect/sql/oceanbase.sql`
- Modify: `crates/dblens-dialect/sql/opengauss.sql`
- Modify: `crates/dblens-dialect/sql/postgres.sql`
- Create: `crates/dblens-dialect/sql/tidb.sql`
- Modify: `crates/dblens-dialect/src/design/rebuild.rs`
- Modify: `crates/dblens-dialect/src/ident.rs`
- Modify: `crates/dblens-dialect/src/meta/assemble.rs`
- Modify: `crates/dblens-dialect/src/meta/template.rs`
- Create: `crates/dblens-dialect/tests/fixtures/opengauss.sql`
- Modify: `crates/dblens-dialect/tests/meta_live.rs`
- Modify: `crates/dblens-dialect/tests/meta_snapshots.rs`
- Create: `crates/dblens-dialect/tests/snapshots/meta_snapshots__kingbase.snap`
- Create: `crates/dblens-dialect/tests/snapshots/meta_snapshots__oceanbase.snap`
- Create: `crates/dblens-dialect/tests/snapshots/meta_snapshots__opengauss.snap`
- Modify: `crates/dblens-dialect/tests/snapshots/meta_snapshots__postgres.snap`
- Create: `crates/dblens-dialect/tests/snapshots/meta_snapshots__tidb.snap`
- Modify: `crates/dblens-driver-postgres/src/types.rs`

**Interfaces:**
- Consumes：任务 2 的 `oracle_mode`、任务 3 的驱动。
- Produces：`dblens_dialect::meta::TableStorage.clustered: Option<bool>`。

- [ ] **Step 1: 新建 `crates/dblens-dialect/tests/fixtures/opengauss.sql`**

```sql
-- openGauss（PG 与 A 兼容模式通用）：postgres.sql 的改写版。没有 IDENTITY 列，改用 serial；存储过程用 openGauss 的写法；
-- 分区在建表时一并定义，LIST 分区键不能是 text；没有 EXCLUDE 约束；numeric 的小数位不能为负。
-- @@ drop_schema
DROP SCHEMA IF EXISTS dblens_meta CASCADE
-- @@ create_schema
CREATE SCHEMA dblens_meta
-- @@ dept
CREATE TABLE dblens_meta.dept (id serial PRIMARY KEY, name varchar(50) NOT NULL UNIQUE, note text)
-- @@ dept_c1
COMMENT ON TABLE dblens_meta.dept IS '部门表'
-- @@ dept_c2
COMMENT ON COLUMN dblens_meta.dept.id IS '部门编号'
-- @@ dept_c3
COMMENT ON COLUMN dblens_meta.dept.name IS '部门名称'
-- @@ seq_meta
CREATE SEQUENCE dblens_meta.seq_meta START 100 INCREMENT 5 MAXVALUE 99999 CYCLE CACHE 10
-- @@ emp
CREATE TABLE dblens_meta.emp (
  id bigint PRIMARY KEY,
  dept_id integer,
  manager_id bigint,
  name varchar(100) NOT NULL DEFAULT '无名',
  salary numeric(10,2) CONSTRAINT ck_emp_salary CHECK (salary >= 0),
  created timestamp(6) DEFAULT CURRENT_TIMESTAMP,
  bio text,
  photo bytea,
  tags text[],
  serial_no serial,
  name_upper text GENERATED ALWAYS AS (upper(name)) STORED,
  CONSTRAINT fk_emp_dept FOREIGN KEY (dept_id) REFERENCES dblens_meta.dept(id) ON DELETE CASCADE,
  CONSTRAINT fk_emp_mgr FOREIGN KEY (manager_id) REFERENCES dblens_meta.emp(id) ON DELETE SET NULL ON UPDATE NO ACTION,
  CONSTRAINT uq_emp_dept_name UNIQUE (dept_id, name))
-- @@ emp_c1
COMMENT ON TABLE dblens_meta.emp IS '员工表 employees'
-- @@ emp_c2
COMMENT ON COLUMN dblens_meta.emp.name IS '员工姓名'
-- @@ idx_created
CREATE INDEX idx_emp_created ON dblens_meta.emp (created DESC)
-- @@ idx_upper
CREATE INDEX idx_emp_upper ON dblens_meta.emp (lower(name), dept_id)
-- @@ idx_gin
CREATE INDEX idx_emp_tags ON dblens_meta.emp USING gin (tags)
-- @@ idx_partial
CREATE INDEX idx_emp_partial ON dblens_meta.emp (salary) WHERE salary > 1000
-- @@ trg_fn
CREATE FUNCTION dblens_meta.trg_emp() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RETURN NEW; END $$
-- @@ trg
CREATE TRIGGER trg_emp_biu BEFORE INSERT OR UPDATE ON dblens_meta.emp FOR EACH ROW EXECUTE FUNCTION dblens_meta.trg_emp()
-- @@ trg2
CREATE TRIGGER trg_emp_trunc AFTER TRUNCATE ON dblens_meta.emp FOR EACH STATEMENT EXECUTE FUNCTION dblens_meta.trg_emp()
-- @@ v_emp
CREATE VIEW dblens_meta.v_emp AS SELECT e.id, e.name, d.name AS dept_name FROM dblens_meta.emp e LEFT JOIN dblens_meta.dept d ON d.id = e.dept_id
-- @@ v_emp_c
COMMENT ON VIEW dblens_meta.v_emp IS '员工视图'
-- @@ mv_dept
CREATE MATERIALIZED VIEW dblens_meta.mv_dept AS SELECT d.id, d.name, count(e.id) AS cnt FROM dblens_meta.dept d LEFT JOIN dblens_meta.emp e ON e.dept_id = d.id GROUP BY d.id, d.name
-- @@ mv_dept_c
COMMENT ON MATERIALIZED VIEW dblens_meta.mv_dept IS '部门统计物化视图'
-- @@ mv_dept_cc
COMMENT ON COLUMN dblens_meta.mv_dept.cnt IS '员工数'
-- @@ p_meta
CREATE PROCEDURE dblens_meta.p_meta(a integer) AS BEGIN NULL; END;
-- @@ f_meta
CREATE FUNCTION dblens_meta.f_meta(a integer) RETURNS integer LANGUAGE sql AS $$ SELECT a + 1 $$
-- @@ f_meta2
CREATE FUNCTION dblens_meta.f_meta(a text, b integer) RETURNS text LANGUAGE sql AS $$ SELECT a || b::text $$
-- @@ f_meta_c
COMMENT ON FUNCTION dblens_meta.f_meta(integer) IS '加一函数'
-- @@ agg_fn
CREATE FUNCTION dblens_meta.agg_sfunc(integer, integer) RETURNS integer LANGUAGE sql AS $$ SELECT $1 + $2 $$
-- @@ agg
CREATE AGGREGATE dblens_meta.sum_meta(integer) (SFUNC = dblens_meta.agg_sfunc, STYPE = integer)
-- @@ part_t
CREATE TABLE dblens_meta.part_t (id integer, created date NOT NULL, v text) PARTITION BY RANGE (created) (PARTITION part_t_2024 VALUES LESS THAN ('2025-01-01'), PARTITION part_t_2025 VALUES LESS THAN ('2026-01-01'))
-- @@ part_h
CREATE TABLE dblens_meta.part_h (id integer) PARTITION BY HASH (id) (PARTITION part_h0, PARTITION part_h1)
-- @@ part_l
CREATE TABLE dblens_meta.part_l (id integer, region varchar(10)) PARTITION BY LIST (region) (PARTITION part_l_a VALUES ('a'))
-- @@ inh_parent
CREATE TABLE dblens_meta.inh_parent (id integer)
-- @@ inh_child
CREATE TABLE dblens_meta.inh_child (extra integer) INHERITS (dblens_meta.inh_parent)
-- @@ temp_t
CREATE TEMP TABLE meta_tmp_probe (id integer)
-- @@ unlogged_t
CREATE UNLOGGED TABLE dblens_meta.unlogged_t (id integer)
-- @@ quoted
CREATE TABLE dblens_meta."Mixed Case" ("select" integer PRIMARY KEY, "数洞" text, "Id" integer)
-- @@ uidx
CREATE UNIQUE INDEX uidx_dept_note ON dblens_meta.dept (note) WHERE note IS NOT NULL
-- @@ enum_ty
CREATE TYPE dblens_meta.mood AS ENUM ('sad','ok','happy')
-- @@ dom_ty
CREATE DOMAIN dblens_meta.posint AS integer CHECK (VALUE > 0)
-- @@ ds_t
CREATE TABLE dblens_meta.ds_t (
  c_smallint smallint, c_int integer, c_bigint bigint,
  c_numeric numeric, c_numeric_p numeric(10,2),
  c_real real, c_double double precision, c_float3 float(3),
  c_money money,
  c_char char(10), c_char1 char, c_varchar varchar(255), c_varchar_n varchar, c_text text, c_name name, c_citext_like "char",
  c_bytea bytea,
  c_bool boolean,
  c_date date, c_time time, c_time3 time(3), c_timetz time with time zone, c_ts timestamp, c_ts6 timestamp(6), c_ts3 timestamp(3), c_tstz timestamptz, c_tstz3 timestamp(3) with time zone,
  c_interval interval, c_interval_ds interval day to second(3), c_interval_ym interval year to month, c_interval_f interval(2),
  c_uuid uuid, c_json json, c_jsonb jsonb, c_xml xml,
  c_inet inet, c_cidr cidr, c_macaddr macaddr,
  c_bit bit(1), c_bit8 bit(8), c_varbit bit varying(16),
  c_tsvector tsvector, c_tsquery tsquery,
  c_point point, c_box box, c_int4range int4range, c_tstzrange tstzrange,
  c_int_arr integer[], c_text_arr text[], c_int_arr2 integer[][], c_varchar_arr varchar(20)[], c_ts_arr timestamp(3)[], c_numeric_arr numeric(8,3)[],
  c_enum dblens_meta.mood, c_domain dblens_meta.posint, c_enum_arr dblens_meta.mood[],
  c_oid oid, c_regclass regclass, c_serial serial, c_bigserial bigserial, c_smallserial smallserial)
```

- [ ] **Step 2: 修改 `crates/dblens-dialect/tests/meta_live.rs`**

```diff
--- a/crates/dblens-dialect/tests/meta_live.rs
+++ b/crates/dblens-dialect/tests/meta_live.rs
@@ -1,7 +1,10 @@
 //! 元数据查询的实库测试。设置对应的环境变量时运行，否则直接通过（与驱动一致性测试相同的约定）：
-//! DBLENS_TEST_MYSQL_URL、DBLENS_TEST_MARIADB_URL、DBLENS_TEST_PG_URL、DBLENS_TEST_ORACLE_URL、DBLENS_TEST_DM_URL。
+//! DBLENS_TEST_MYSQL_URL、DBLENS_TEST_MARIADB_URL、DBLENS_TEST_PG_URL、DBLENS_TEST_ORACLE_URL、DBLENS_TEST_DM_URL；
+//! 国产库第一批：DBLENS_TEST_TIDB_URL、DBLENS_TEST_OCEANBASE_URL、DBLENS_TEST_OPENGAUSS_URL、
+//! DBLENS_TEST_OPENGAUSS_A_URL、DBLENS_TEST_KINGBASE_URL、DBLENS_TEST_KINGBASE_PG_URL。
 //!
 //! 每个家族先执行 `tests/fixtures/<家族>.sql` 建测试对象，再用全部元数据查询读回来逐项核对。
+//! 兼容库沿用所属家族的脚本，跳过不支持的段、替换写法不同的段（`run_script_with`）。
 
 use std::time::Instant;
 
@@ -57,17 +60,25 @@ impl Live {
 
     /// 按 `-- @@ 标签` 分段逐条执行建测试对象的脚本。
     async fn run_script(&mut self, text: &str) {
-        let mut sections: Vec<String> = Vec::new();
+        self.run_script_with(text, &[], &[]).await;
+    }
+
+    /// 同 `run_script`，但跳过 `skip` 中的段，`replace` 中的段改为执行给出的语句。
+    async fn run_script_with(&mut self, text: &str, skip: &[&str], replace: &[(&str, &str)]) {
+        let mut sections: Vec<(String, String)> = Vec::new();
         for line in text.lines() {
-            if line.starts_with("-- @@") {
-                sections.push(String::new());
-            } else if let Some(last) = sections.last_mut() {
+            if let Some(label) = line.strip_prefix("-- @@") {
+                sections.push((label.trim().to_string(), String::new()));
+            } else if let Some((_, last)) = sections.last_mut() {
                 last.push_str(line);
                 last.push('\n');
             }
         }
-        for sql in sections {
-            let sql = sql.trim();
+        for (label, sql) in sections {
+            if skip.contains(&label.as_str()) {
+                continue;
+            }
+            let sql = replace.iter().find(|(l, _)| *l == label).map_or(sql.trim(), |(_, r)| r.trim());
             if sql.is_empty() {
                 continue;
             }
@@ -126,13 +137,26 @@ fn has(text: &str, needle: &str) -> bool {
 }
 
 /// 测试对象的命名：MySQL/PG 用小写原名；Oracle/达梦用大写并加 `META_` 前缀（列名只转大写）。
+/// `triggers`、`routines` 为 false 时该库没有这类对象（TiDB），不检查；`descending` 为 false 时
+/// 该库建索引时忽略 DESC（TiDB、OceanBase）。
 #[derive(Clone, Copy)]
 struct Fx {
     schema: &'static str,
     upper: bool,
+    triggers: bool,
+    routines: bool,
+    descending: bool,
 }
 
 impl Fx {
+    fn lower(schema: &'static str) -> Fx {
+        Fx { schema, upper: false, triggers: true, routines: true, descending: true }
+    }
+
+    fn upper(schema: &'static str) -> Fx {
+        Fx { schema, upper: true, triggers: true, routines: true, descending: true }
+    }
+
     fn obj(&self, name: &str) -> String {
         if self.upper { format!("META_{}", name.to_ascii_uppercase()) } else { name.to_string() }
     }
@@ -190,16 +214,19 @@ async fn check_common(live: &mut Live, fx: Fx) -> Vec<Table> {
     assert!(emp.checks.iter().any(|c| has(&c.expression, "salary")), "{:?}", emp.checks);
     let created =
         emp.indexes.iter().find(|i| i.columns[0].name == fx.col("created")).expect("created 上的索引");
-    assert!(created.columns[0].descending);
-    let trg = emp.triggers.iter().find(|t| t.timing.as_deref() == Some("BEFORE")).expect("BEFORE 触发器");
-    assert!(trg.events.contains(&"INSERT".to_string()), "{trg:?}");
+    assert_eq!(created.columns[0].descending, fx.descending);
+    if fx.triggers {
+        let trg = emp.triggers.iter().find(|t| t.timing.as_deref() == Some("BEFORE")).expect("BEFORE 触发器");
+        assert!(trg.events.contains(&"INSERT".to_string()), "{trg:?}");
+    }
 
     // 单表明细与整个 schema 中的同一张表一致
     let one = live.tables(s, Some(&emp_name)).await;
     assert_eq!(one.len(), 1);
     assert_eq!(&one[0], emp);
 
-    let mixed = all.iter().find(|t| t.name.ends_with("Mixed Case")).expect("表 Mixed Case");
+    // OceanBase 默认 lower_case_table_names = 1，表名存成小写
+    let mixed = all.iter().find(|t| has(&t.name, "Mixed Case")).expect("表 Mixed Case");
     assert!(live.d.qualified(Some(s), &mixed.name).ends_with(&live.d.quote_ident(&mixed.name)));
     assert!(mixed.columns.iter().any(|c| c.name == "数洞"));
     assert!(mixed.columns.iter().any(|c| c.name == "select") && mixed.primary_key.is_some());
@@ -216,20 +243,22 @@ async fn check_common(live: &mut Live, fx: Fx) -> Vec<Table> {
         .unwrap();
     assert!(has(&text, "dept"), "{text}");
 
-    let routines = meta::routines(&live.rows(MetaKey::Routines, &args(s)).await).unwrap();
-    assert!(
-        routines.iter().any(|r| r.name == fx.obj("p_meta") && r.kind == RoutineKind::Procedure),
-        "{routines:?}"
-    );
-    assert!(
-        routines.iter().any(|r| r.name == fx.obj("f_meta") && r.kind == RoutineKind::Function),
-        "{routines:?}"
-    );
-    let src = MetaArgs { name: Some(fx.obj("f_meta")), kind: Some("FUNCTION".into()), ..args(s) };
-    let text = meta::source_text(MetaKey::RoutineSource, &live.rows(MetaKey::RoutineSource, &src).await)
-        .unwrap()
-        .unwrap();
-    assert!(has(&text, "function"), "{text}");
+    if fx.routines {
+        let routines = meta::routines(&live.rows(MetaKey::Routines, &args(s)).await).unwrap();
+        assert!(
+            routines.iter().any(|r| r.name == fx.obj("p_meta") && r.kind == RoutineKind::Procedure),
+            "{routines:?}"
+        );
+        assert!(
+            routines.iter().any(|r| r.name == fx.obj("f_meta") && r.kind == RoutineKind::Function),
+            "{routines:?}"
+        );
+        let src = MetaArgs { name: Some(fx.obj("f_meta")), kind: Some("FUNCTION".into()), ..args(s) };
+        let text = meta::source_text(MetaKey::RoutineSource, &live.rows(MetaKey::RoutineSource, &src).await)
+            .unwrap()
+            .unwrap();
+        assert!(has(&text, "function"), "{text}");
+    }
 
     if live.d.has_meta(MetaKey::ReservedWords) {
         live.check_reserved_words().await;
@@ -237,8 +266,7 @@ async fn check_common(live: &mut Live, fx: Fx) -> Vec<Table> {
     all
 }
 
-async fn check_mysql_family(live: &mut Live) -> Vec<Table> {
-    let fx = Fx { schema: "dblens_meta", upper: false };
+async fn check_mysql_family(live: &mut Live, fx: Fx) -> Vec<Table> {
     let all = check_common(live, fx).await;
     let emp = table(&all, "emp");
     assert_eq!(emp.columns.iter().find(|c| c.name == "name").unwrap().default.as_deref(), Some("'无名'"));
@@ -258,7 +286,7 @@ async fn mysql_metadata() {
     assert_eq!(live.d.variant(), Variant::MySql, "不是 PolarDB");
     live.run_script(include_str!("fixtures/mysql.sql")).await;
     live.run_script(include_str!("fixtures/mysql_only.sql")).await;
-    let all = check_mysql_family(&mut live).await;
+    let all = check_mysql_family(&mut live, Fx::lower("dblens_meta")).await;
     let func = table(&all, "emp").indexes.iter().find(|i| i.name == "idx_emp_upper_name").unwrap();
     assert!(has(&func.columns[0].name, "upper"), "{func:?}");
     assert_eq!(logical(table(&all, "ds_t"), "c_json"), LogicalType::Json);
@@ -271,7 +299,7 @@ async fn mariadb_metadata() {
     assert_eq!(live.d.variant(), Variant::MariaDb);
     live.run_script(include_str!("fixtures/mysql.sql")).await;
     live.run_script(include_str!("fixtures/mariadb_only.sql")).await;
-    let all = check_mysql_family(&mut live).await;
+    let all = check_mysql_family(&mut live, Fx::lower("dblens_meta")).await;
     // MariaDB 的 JSON 是带检查约束的 longtext，这条隐式检查约束不应出现在 checks 中
     assert!(table(&all, "ds_t").checks.is_empty(), "{:?}", table(&all, "ds_t").checks);
     assert!(!live.d.has_meta(MetaKey::ReservedWords), "MariaDB 的 KEYWORDS 不区分是否保留");
@@ -283,8 +311,32 @@ async fn mariadb_metadata() {
 async fn postgres_metadata() {
     let Some(mut live) = connect("DBLENS_TEST_PG_URL").await else { return };
     live.run_script(include_str!("fixtures/postgres.sql")).await;
-    let fx = Fx { schema: "dblens_meta", upper: false };
-    let all = check_common(&mut live, fx).await;
+    check_pg_family(&mut live, PgFx::FULL).await;
+}
+
+/// PG 家族各库支持的程度：物化视图（人大金仓开发版的授权不允许建）、序列的缓存大小（openGauss 读不到）。
+#[derive(Clone, Copy)]
+struct PgFx {
+    materialized_views: bool,
+    sequence_cache: bool,
+}
+
+impl PgFx {
+    const FULL: PgFx = PgFx { materialized_views: true, sequence_cache: true };
+}
+
+/// PG 家族的检查：先做共有的检查，再核对 PG 特有的对象。
+async fn check_pg_family(live: &mut Live, pg: PgFx) {
+    let schemas = meta::schemas(&live.rows(MetaKey::Schemas, &MetaArgs::default()).await).unwrap();
+    let builtin = match live.d.variant() {
+        Variant::Kingbase => "sys_catalog",
+        Variant::OpenGauss => "dbe_perf",
+        _ => "pg_catalog",
+    };
+    assert!(schemas.iter().any(|x| x.name == builtin && x.is_system), "{schemas:?}");
+    assert!(schemas.iter().any(|x| x.name == "public" && !x.is_system), "{schemas:?}");
+    let fx = Fx::lower("dblens_meta");
+    let all = check_common(live, fx).await;
     let s = fx.schema;
 
     assert!(
@@ -310,16 +362,20 @@ async fn postgres_metadata() {
     let views =
         meta::views(&d, MetaKey::Views, &live.rows(MetaKey::Views, &args(s)).await, &columns).unwrap();
     assert_eq!(views.iter().find(|v| v.name == "v_emp").unwrap().comment.as_deref(), Some("员工视图"));
-    let mv_rows = live.rows(MetaKey::MaterializedViews, &args(s)).await;
-    let mv = meta::views(&d, MetaKey::MaterializedViews, &mv_rows, &columns).unwrap();
-    assert_eq!(mv[0].columns.iter().find(|c| c.name == "cnt").unwrap().comment.as_deref(), Some("员工数"));
+    if pg.materialized_views {
+        let mv_rows = live.rows(MetaKey::MaterializedViews, &args(s)).await;
+        let mv = meta::views(&d, MetaKey::MaterializedViews, &mv_rows, &columns).unwrap();
+        let cnt = mv[0].columns.iter().find(|c| c.name == "cnt").unwrap();
+        assert_eq!(cnt.comment.as_deref(), Some("员工数"));
+    }
 
     let seq = meta::sequences(&live.rows(MetaKey::Sequences, &args(s)).await).unwrap();
     assert_eq!(seq.len(), 1, "serial/identity 自带的序列不列出：{seq:?}");
     let q = &seq[0];
+    let cache = pg.sequence_cache.then_some(10);
     assert_eq!(
         (q.start.as_deref(), q.increment.as_deref(), q.cycle, q.cache),
-        (Some("100"), Some("5"), true, Some(10))
+        (Some("100"), Some("5"), true, cache)
     );
 
     let routines = meta::routines(&live.rows(MetaKey::Routines, &args(s)).await).unwrap();
@@ -329,14 +385,15 @@ async fn postgres_metadata() {
         .unwrap()
         .unwrap();
     assert_eq!(text.matches("CREATE OR REPLACE FUNCTION").count(), 2, "两个重载：{text}");
-    assert!(!live.d.has_meta(MetaKey::Synonyms));
+    // 人大金仓、openGauss 有同义词（check_oracle_mode_objects）
+    assert_eq!(live.d.has_meta(MetaKey::Synonyms), live.d.variant() != Variant::Postgres);
 }
 
 #[tokio::test]
 async fn oracle_metadata() {
     let Some(mut live) = connect("DBLENS_TEST_ORACLE_URL").await else { return };
     live.run_script(include_str!("fixtures/oracle.sql")).await;
-    let fx = Fx { schema: "DBLENS_TEST", upper: true };
+    let fx = Fx::upper("DBLENS_TEST");
     let all = check_common(&mut live, fx).await;
     let s = fx.schema;
 
@@ -369,7 +426,7 @@ async fn oracle_metadata() {
 async fn dameng_metadata() {
     let Some(mut live) = connect("DBLENS_TEST_DM_URL").await else { return };
     live.run_script(include_str!("fixtures/dameng.sql")).await;
-    let fx = Fx { schema: "DBLENS_TEST", upper: true };
+    let fx = Fx::upper("DBLENS_TEST");
     let all = check_common(&mut live, fx).await;
     let s = fx.schema;
 
@@ -399,3 +456,151 @@ async fn dameng_metadata() {
     let mv = meta::views(&d, MetaKey::MaterializedViews, &mv_rows, &columns).unwrap();
     assert_eq!(mv.iter().find(|v| v.name == "META_MV_DEPT").unwrap().columns.len(), 2);
 }
+
+/// MySQL 脚本里 TiDB 不支持的段：触发器、存储过程和函数、全文索引；ds_t 去掉 GEOMETRY 列。
+const TIDB_SKIP: [&str; 4] = ["trg", "proc", "func", "idx_fulltext"];
+
+fn without_geometry(script: &str) -> String {
+    let ds = script.lines().find(|l| l.starts_with("CREATE TABLE dblens_meta.ds_t")).unwrap();
+    ds.replace(", c_geometry GEOMETRY", "")
+}
+
+#[tokio::test]
+async fn tidb_metadata() {
+    let Some(mut live) = connect("DBLENS_TEST_TIDB_URL").await else { return };
+    assert_eq!(live.d.variant(), Variant::TiDb);
+    // CHECK 约束默认只解析不保存，打开后才能读回
+    live.run_script(
+        "-- @@ check_on
+SET GLOBAL tidb_enable_check_constraint = ON",
+    )
+    .await;
+    let script = include_str!("fixtures/mysql.sql");
+    let ds = without_geometry(script);
+    live.run_script_with(script, &TIDB_SKIP, &[("ds_t", &ds)]).await;
+    let fx = Fx { triggers: false, routines: false, descending: false, ..Fx::lower("dblens_meta") };
+    live.run_script(TIDB_ONLY).await;
+    let all = check_mysql_family(&mut live, fx).await;
+    assert!(table(&all, "emp").triggers.is_empty());
+
+    let ar = table(&all, "ar_t");
+    assert!(ar.columns[0].auto_increment && !ar.columns[1].auto_increment, "AUTO_RANDOM：{ar:?}");
+    let seq = meta::sequences(&live.rows(MetaKey::Sequences, &args("dblens_meta")).await).unwrap();
+    let q = seq.iter().find(|q| q.name == "seq_meta").unwrap();
+    assert_eq!(
+        (q.start.as_deref(), q.increment.as_deref(), q.cycle, q.cache),
+        (Some("100"), Some("5"), true, Some(10))
+    );
+    for (t, clustered) in [("ar_t", true), ("nc_t", false)] {
+        let a = MetaArgs { table: Some(t.into()), ..args("dblens_meta") };
+        let storage = meta::table_storage(&live.rows(MetaKey::TableStorage, &a).await).unwrap();
+        assert_eq!(storage.clustered, Some(clustered), "{t}");
+    }
+}
+
+/// 只在 TiDB 上建的对象：AUTO_RANDOM 主键、非聚簇主键、序列。
+const TIDB_ONLY: &str = "-- @@ ar_t
+CREATE TABLE dblens_meta.ar_t (id BIGINT AUTO_RANDOM PRIMARY KEY, v INT)
+-- @@ nc_t
+CREATE TABLE dblens_meta.nc_t (id INT PRIMARY KEY NONCLUSTERED, v INT)
+-- @@ seq
+CREATE SEQUENCE dblens_meta.seq_meta START 100 INCREMENT 5 CACHE 10 CYCLE
+";
+
+#[tokio::test]
+async fn oceanbase_metadata() {
+    let Some(mut live) = connect("DBLENS_TEST_OCEANBASE_URL").await else { return };
+    assert_eq!(live.d.variant(), Variant::OceanBase);
+    let script = include_str!("fixtures/mysql.sql");
+    let ds = without_geometry(script);
+    live.run_script_with(script, &[], &[("ds_t", &ds)]).await;
+    let all = check_mysql_family(&mut live, Fx { descending: false, ..Fx::lower("dblens_meta") }).await;
+    // 全文索引在 STATISTICS 中只有隐藏列（__doc_id_*、__word_segment_*），去掉后不列出
+    let emp = table(&all, "emp");
+    assert!(emp.indexes.iter().all(|i| i.columns.iter().all(|c| !c.name.starts_with("__"))), "{emp:?}");
+}
+
+#[tokio::test]
+async fn opengauss_metadata() {
+    let Some(mut live) = connect("DBLENS_TEST_OPENGAUSS_URL").await else { return };
+    assert_eq!((live.d.variant(), live.d.oracle_mode()), (Variant::OpenGauss, false));
+    live.run_script(include_str!("fixtures/opengauss.sql")).await;
+    check_pg_family(&mut live, PgFx { sequence_cache: false, ..PgFx::FULL }).await;
+}
+
+#[tokio::test]
+async fn opengauss_a_metadata() {
+    let Some(mut live) = connect("DBLENS_TEST_OPENGAUSS_A_URL").await else { return };
+    assert_eq!((live.d.variant(), live.d.oracle_mode()), (Variant::OpenGauss, true));
+    live.run_script(include_str!("fixtures/opengauss.sql")).await;
+    check_pg_family(&mut live, PgFx { sequence_cache: false, ..PgFx::FULL }).await;
+    check_oracle_mode_objects(&mut live).await;
+}
+
+/// 人大金仓沿用 PG 的脚本：开发版授权不允许建物化视图；PG 模式不接受负的小数位。
+async fn kingbase_metadata(env: &str, oracle_mode: bool) {
+    let Some(mut live) = connect(env).await else { return };
+    assert_eq!((live.d.variant(), live.d.oracle_mode()), (Variant::Kingbase, oracle_mode));
+    let script = include_str!("fixtures/postgres.sql");
+    let ds =
+        script.lines().skip_while(|l| !l.starts_with("CREATE TABLE dblens_meta.ds_t")).collect::<Vec<_>>();
+    let ds = ds
+        .join(
+            "
+",
+        )
+        .replace(", c_numeric_neg numeric(5,-2)", "");
+    let skip = ["mv_dept", "mv_dept_c", "mv_dept_cc"];
+    live.run_script_with(script, &skip, &[("ds_t", &ds)]).await;
+    check_pg_family(&mut live, PgFx { materialized_views: false, ..PgFx::FULL }).await;
+    if oracle_mode {
+        check_oracle_mode_objects(&mut live).await;
+    }
+}
+
+#[tokio::test]
+async fn kingbase_oracle_metadata() {
+    kingbase_metadata("DBLENS_TEST_KINGBASE_URL", true).await;
+}
+
+#[tokio::test]
+async fn kingbase_pg_metadata() {
+    kingbase_metadata("DBLENS_TEST_KINGBASE_PG_URL", false).await;
+}
+
+/// Oracle 兼容模式（人大金仓、openGauss 的 A 模式）：同义词与包。
+const ORACLE_MODE_ONLY: &str = "-- @@ syn
+CREATE OR REPLACE SYNONYM dblens_meta.syn_emp FOR dblens_meta.emp
+-- @@ pkg
+CREATE OR REPLACE PACKAGE dblens_meta.pkg_meta AS FUNCTION f(a integer) RETURN integer; END pkg_meta;
+-- @@ pkg_body
+CREATE OR REPLACE PACKAGE BODY dblens_meta.pkg_meta AS FUNCTION f(a integer) RETURN integer IS BEGIN RETURN a + 1; END; END pkg_meta;
+";
+
+async fn check_oracle_mode_objects(live: &mut Live) {
+    live.run_script(ORACLE_MODE_ONLY).await;
+    let s = "dblens_meta";
+    let syn = meta::synonyms(&live.rows(MetaKey::Synonyms, &args(s)).await).unwrap();
+    assert!(
+        syn.iter()
+            .any(|x| x.name == "syn_emp" && x.target_schema.as_deref() == Some(s) && x.target_name == "emp"),
+        "{syn:?}"
+    );
+    let routines = meta::routines(&live.rows(MetaKey::Routines, &args(s)).await).unwrap();
+    for kind in [RoutineKind::Package, RoutineKind::PackageBody] {
+        assert!(routines.iter().any(|r| r.name == "pkg_meta" && r.kind == kind), "{routines:?}");
+    }
+    assert!(!routines.iter().any(|r| r.name == "f"), "包里的函数不单独列出：{routines:?}");
+    for (kind, head) in
+        [("PACKAGE", "CREATE OR REPLACE PACKAGE "), ("PACKAGE BODY", "CREATE OR REPLACE PACKAGE BODY ")]
+    {
+        let a = MetaArgs { name: Some("pkg_meta".into()), kind: Some(kind.into()), ..args(s) };
+        let text = meta::source_text(MetaKey::RoutineSource, &live.rows(MetaKey::RoutineSource, &a).await)
+            .unwrap()
+            .unwrap_or_else(|| panic!("没有 {kind} 的源码"));
+        assert!(
+            text.starts_with(head) && has(&text, "RETURN integer") && text.trim_end().ends_with(';'),
+            "{text}"
+        );
+    }
+}
```

- [ ] **Step 3: 修改 `crates/dblens-dialect/tests/meta_snapshots.rs`**

```diff
--- a/crates/dblens-dialect/tests/meta_snapshots.rs
+++ b/crates/dblens-dialect/tests/meta_snapshots.rs
@@ -2,10 +2,20 @@
 //! 改动模板时在快照差异里审查生成的 SQL。新增或改动快照后用 `INSTA_UPDATE=always cargo test` 重新生成并审查。
 
 use dblens_dialect::meta::{MetaArgs, MetaKey};
-use dblens_dialect::{Dialect, Variant};
+use dblens_dialect::{Dialect, Family, Variant};
 
-const VARIANTS: [Variant; 5] =
-    [Variant::MySql, Variant::MariaDb, Variant::Postgres, Variant::Oracle, Variant::Dameng];
+/// 有自己的模板覆盖文件的变体（PolarDB、GaussDB、HighGo 沿用家族模板）。
+const VARIANTS: [Variant; 9] = [
+    Variant::MySql,
+    Variant::MariaDb,
+    Variant::TiDb,
+    Variant::OceanBase,
+    Variant::Postgres,
+    Variant::Kingbase,
+    Variant::OpenGauss,
+    Variant::Oracle,
+    Variant::Dameng,
+];
 
 #[test]
 fn rendered_metadata_queries() {
@@ -38,9 +48,10 @@ fn every_family_covers_the_common_keys() {
                 | MetaKey::Synonyms
                 | MetaKey::Sequences
                 | MetaKey::VariantProbe => true,
-                MetaKey::DependentViews | MetaKey::Grants => matches!(v, Variant::MySql | Variant::MariaDb),
+                MetaKey::DependentViews | MetaKey::Grants => v.family() == Family::MySql,
                 MetaKey::TriggerDdl => v != Variant::Oracle,
-                MetaKey::ReservedWords => v == Variant::MariaDb,
+                // OceanBase 没有 information_schema.KEYWORDS
+                MetaKey::ReservedWords => matches!(v, Variant::MariaDb | Variant::OceanBase),
                 _ => false,
             };
             assert!(optional || d.has_meta(key), "{v:?} 缺少 {key:?} 模板");
@@ -61,6 +72,8 @@ fn legacy_version_branches_render() {
     for (v, old) in [
         (Variant::MySql, 50744),
         (Variant::Postgres, 100000),
+        (Variant::Kingbase, 120100),
+        (Variant::OpenGauss, 90204),
         (Variant::Oracle, 110204),
         (Variant::Dameng, 80000),
     ] {
```

- [ ] **Step 4: 新建 `crates/dblens-dialect/tests/snapshots/meta_snapshots__kingbase.snap`**

```text
---
source: crates/dblens-dialect/tests/meta_snapshots.rs
expression: out
---
-- [schemas] []
-- Built-in schemas besides pg_* and information_schema (sys, sys_*, dbms_*, ...); Oracle mode adds sys and wmsys.
SELECT n.nspname AS name,
       (n.nspname LIKE 'pg!_%' ESCAPE '!' OR n.nspname LIKE 'sys!_%' ESCAPE '!' OR n.nspname LIKE 'dbms!_%' ESCAPE '!'
        OR n.nspname LIKE 'kdb!_%' ESCAPE '!'
        OR n.nspname IN ('information_schema', 'sys', 'anon', 'perf', 'src_restrict', 'sysaudit', 'sysmac',
                         'wmsys', 'xlog_record_read')) AS is_system
FROM pg_catalog.pg_namespace n
ORDER BY n.nspname

-- [tables] [String("HR"), String("EMP"), String("%员工%"), String("%员工%")]
SELECT c.relname AS name,
       pg_catalog.obj_description(c.oid, 'pg_class') AS comment,
       CASE WHEN c.relkind = 'p' THEN pg_catalog.pg_get_partkeydef(c.oid) END AS partition
FROM pg_catalog.pg_class c
JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace
WHERE n.nspname = $1
  AND c.relkind IN ('r', 'p')
  AND NOT c.relispartition
  AND c.relpersistence <> 't'
  AND c.relname = $2
  AND (UPPER(c.relname) LIKE UPPER($3) ESCAPE '!'
       OR UPPER(pg_catalog.obj_description(c.oid, 'pg_class')) LIKE UPPER($4) ESCAPE '!')
ORDER BY c.relname

-- [views] [String("HR"), String("%员工%"), String("%员工%")]
SELECT c.relname AS name,
       pg_catalog.obj_description(c.oid, 'pg_class') AS comment
FROM pg_catalog.pg_class c
JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace
WHERE n.nspname = $1
  AND c.relkind = 'v'
  AND (UPPER(c.relname) LIKE UPPER($2) ESCAPE '!'
       OR UPPER(pg_catalog.obj_description(c.oid, 'pg_class')) LIKE UPPER($3) ESCAPE '!')
ORDER BY c.relname

-- [materialized_views] [String("HR"), String("%员工%"), String("%员工%")]
SELECT c.relname AS name,
       pg_catalog.obj_description(c.oid, 'pg_class') AS comment
FROM pg_catalog.pg_class c
JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace
WHERE n.nspname = $1
  AND c.relkind = 'm'
  AND (UPPER(c.relname) LIKE UPPER($2) ESCAPE '!'
       OR UPPER(pg_catalog.obj_description(c.oid, 'pg_class')) LIKE UPPER($3) ESCAPE '!')
ORDER BY c.relname

-- [sequences] [String("HR"), String("%员工%"), String("%员工%")]
-- Sequences owned by a column (serial 'a' / identity 'i') are implementation detail of the column and are hidden.
SELECT c.relname AS name,
       s.seqstart AS start_value,
       s.seqincrement AS increment,
       s.seqmin AS min_value,
       s.seqmax AS max_value,
       s.seqcycle AS cycle,
       s.seqcache AS cache
FROM pg_catalog.pg_class c
JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace
JOIN pg_catalog.pg_sequence s ON s.seqrelid = c.oid
WHERE n.nspname = $1
  AND c.relkind = 'S'
  AND NOT EXISTS (SELECT 1 FROM pg_catalog.pg_depend d
                  WHERE d.classid = 'pg_catalog.pg_class'::regclass AND d.objid = c.oid
                    AND d.refclassid = 'pg_catalog.pg_class'::regclass AND d.deptype IN ('a', 'i'))
  AND (UPPER(c.relname) LIKE UPPER($2) ESCAPE '!'
       OR UPPER(pg_catalog.obj_description(c.oid, 'pg_class')) LIKE UPPER($3) ESCAPE '!')
ORDER BY c.relname

-- [routines] [String("HR"), String("%员工%"), String("%员工%"), String("HR"), String("%员工%")]
-- Functions and procedures as in postgres.sql, plus packages (pg_catalog._pkg; their routines are not in
-- pg_proc). Every package is listed as PACKAGE and PACKAGE BODY.
SELECT r.name, r.kind, r.comment
FROM (
  SELECT p.proname AS name,
         CASE p.prokind WHEN 'p' THEN 'PROCEDURE' ELSE 'FUNCTION' END AS kind,
         MAX(pg_catalog.obj_description(p.oid, 'pg_proc')) AS comment
  FROM pg_catalog.pg_proc p
  JOIN pg_catalog.pg_namespace n ON n.oid = p.pronamespace
  WHERE n.nspname = $1
    AND p.prokind IN ('f', 'p')
    AND (UPPER(p.proname) LIKE UPPER($2) ESCAPE '!'
         OR UPPER(pg_catalog.obj_description(p.oid, 'pg_proc')) LIKE UPPER($3) ESCAPE '!')
  GROUP BY p.proname, 2
  UNION ALL
  SELECT k.pkgname, x.kind, NULL
  FROM pg_catalog._pkg k
  JOIN pg_catalog.pg_namespace n ON n.oid = k.pkgnamespace
  CROSS JOIN (SELECT 'PACKAGE' AS kind UNION ALL SELECT 'PACKAGE BODY') x
  WHERE n.nspname = $4
    AND UPPER(k.pkgname) LIKE UPPER($5) ESCAPE '!'
) r
ORDER BY r.name, r.kind

-- [synonyms] [String("HR"), String("%员工%")]
-- Oracle-style synonyms (both compatibility modes have them); sys_synonym is a view over the catalog.
SELECT s.synname AS name,
       s.refobjnspname AS target_schema,
       s.refobjname AS target_name,
       NULL AS db_link
FROM pg_catalog.sys_synonym s
JOIN pg_catalog.pg_namespace n ON n.oid = s.synnamespace
WHERE n.nspname = $1
  AND UPPER(s.synname) LIKE UPPER($2) ESCAPE '!'
ORDER BY s.synname

-- [columns] [String("HR"), String("EMP")]
-- No comparisons with '' anywhere: in the Oracle compatibility mode of KingbaseES / openGauss '' is NULL.
SELECT c.relname AS table_name,
       a.attname AS name,
       (ROW_NUMBER() OVER (PARTITION BY c.oid ORDER BY a.attnum))::int AS ordinal,
       pg_catalog.format_type(a.atttypid, a.atttypmod) AS type_text,
       NOT a.attnotnull AS nullable,
       CASE WHEN a.attgenerated <> 's' THEN pg_catalog.pg_get_expr(d.adbin, d.adrelid) END AS default_value,
       pg_catalog.col_description(c.oid, a.attnum) AS comment,
       (a.attidentity IN ('a', 'd') OR (d.adbin IS NOT NULL AND pg_catalog.pg_get_expr(d.adbin, d.adrelid) LIKE 'nextval(%'
            AND EXISTS (SELECT 1 FROM pg_catalog.pg_depend dp
                        WHERE dp.refclassid = 'pg_catalog.pg_class'::regclass AND dp.refobjid = c.oid
                          AND dp.refobjsubid = a.attnum AND dp.deptype = 'a'
                          AND dp.classid = 'pg_catalog.pg_class'::regclass))) AS auto_increment,
       CASE WHEN a.attgenerated = 's' THEN pg_catalog.pg_get_expr(d.adbin, d.adrelid) END AS generated
FROM pg_catalog.pg_class c
JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace
JOIN pg_catalog.pg_attribute a ON a.attrelid = c.oid AND a.attnum > 0 AND NOT a.attisdropped
LEFT JOIN pg_catalog.pg_attrdef d ON d.adrelid = a.attrelid AND d.adnum = a.attnum
WHERE n.nspname = $1
  AND c.relkind IN ('r', 'p', 'v', 'm')
  AND NOT c.relispartition
  AND c.relpersistence <> 't'
  AND c.relname = $2
ORDER BY c.relname, a.attnum

-- [keys] [String("HR"), String("EMP")]
SELECT c.relname AS table_name,
       con.conname AS constraint_name,
       CASE con.contype WHEN 'p' THEN 'P' WHEN 'u' THEN 'U' ELSE 'F' END AS constraint_type,
       a.attname AS column_name,
       k.pos::int AS position,
       CASE WHEN con.contype = 'f' THEN rn.nspname END AS ref_schema,
       CASE WHEN con.contype = 'f' THEN rc.relname END AS ref_table,
       ra.attname AS ref_column,
       CASE WHEN con.contype = 'f' THEN
         CASE con.confdeltype WHEN 'c' THEN 'CASCADE' WHEN 'n' THEN 'SET NULL' WHEN 'd' THEN 'SET DEFAULT'
                              WHEN 'r' THEN 'RESTRICT' ELSE 'NO ACTION' END END AS on_delete,
       CASE WHEN con.contype = 'f' THEN
         CASE con.confupdtype WHEN 'c' THEN 'CASCADE' WHEN 'n' THEN 'SET NULL' WHEN 'd' THEN 'SET DEFAULT'
                              WHEN 'r' THEN 'RESTRICT' ELSE 'NO ACTION' END END AS on_update
FROM pg_catalog.pg_constraint con
JOIN pg_catalog.pg_class c ON c.oid = con.conrelid
JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace
CROSS JOIN LATERAL pg_catalog.unnest(con.conkey) WITH ORDINALITY AS k(attnum, pos)
JOIN pg_catalog.pg_attribute a ON a.attrelid = con.conrelid AND a.attnum = k.attnum
LEFT JOIN pg_catalog.pg_class rc ON rc.oid = con.confrelid
LEFT JOIN pg_catalog.pg_namespace rn ON rn.oid = rc.relnamespace
LEFT JOIN pg_catalog.pg_attribute ra ON ra.attrelid = con.confrelid AND ra.attnum = con.confkey[k.pos]
WHERE n.nspname = $1
  AND con.contype IN ('p', 'u', 'f')
  AND c.relkind IN ('r', 'p')
  AND NOT c.relispartition
  AND c.relpersistence <> 't'
  AND c.relname = $2
ORDER BY c.relname, con.conname, k.pos

-- [checks] [String("HR"), String("EMP")]
-- contype 'c' only: PG 18 not-null constraints are contype 'n' and never match.
SELECT c.relname AS table_name,
       con.conname AS constraint_name,
       pg_catalog.pg_get_expr(con.conbin, con.conrelid) AS expression
FROM pg_catalog.pg_constraint con
JOIN pg_catalog.pg_class c ON c.oid = con.conrelid
JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace
WHERE n.nspname = $1
  AND con.contype = 'c'
  AND c.relkind IN ('r', 'p')
  AND NOT c.relispartition
  AND c.relpersistence <> 't'
  AND c.relname = $2
ORDER BY c.relname, con.conname

-- [indexes] [String("HR"), String("EMP")]
-- Key columns only (INCLUDE columns, pos > indnkeyatts, are skipped). Indexes backing PK/UNIQUE constraints are excluded
-- (not those backing FKs: pg_constraint.conindid of an FK points at the *referenced* unique index, so filter contype).
SELECT c.relname AS table_name,
       ic.relname AS index_name,
       i.indisunique AS is_unique,
       UPPER(am.amname) AS method,
       CASE WHEN k.attnum = 0 THEN pg_catalog.pg_get_indexdef(i.indexrelid, k.pos::int, true) ELSE a.attname END AS column_expr,
       k.pos::int AS position,
       ((i.indoption[k.pos - 1] & 1) = 1) AS descending
FROM pg_catalog.pg_index i
JOIN pg_catalog.pg_class c ON c.oid = i.indrelid
JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace
JOIN pg_catalog.pg_class ic ON ic.oid = i.indexrelid
JOIN pg_catalog.pg_am am ON am.oid = ic.relam
CROSS JOIN LATERAL pg_catalog.unnest(i.indkey::int2[]) WITH ORDINALITY AS k(attnum, pos)
LEFT JOIN pg_catalog.pg_attribute a ON a.attrelid = i.indrelid AND a.attnum = k.attnum AND k.attnum > 0
WHERE n.nspname = $1
  AND c.relkind IN ('r', 'p', 'm')
  AND NOT c.relispartition
  AND c.relpersistence <> 't'
  AND k.pos <= i.indnkeyatts
  AND NOT EXISTS (SELECT 1 FROM pg_catalog.pg_constraint cc
                  WHERE cc.conindid = i.indexrelid AND cc.contype IN ('p', 'u') AND cc.conrelid = i.indrelid)
  AND c.relname = $2
ORDER BY c.relname, ic.relname, k.pos

-- [triggers] [String("HR"), String("EMP")]
-- tgisinternal excludes FK / constraint-implementation triggers. definition = full CREATE TRIGGER via pg_get_triggerdef.
SELECT c.relname AS table_name,
       t.tgname AS name,
       CASE WHEN (t.tgtype & 64) <> 0 THEN 'INSTEAD OF' WHEN (t.tgtype & 2) <> 0 THEN 'BEFORE' ELSE 'AFTER' END AS timing,
       concat_ws(',',
         CASE WHEN (t.tgtype & 4) <> 0 THEN 'INSERT' END,
         CASE WHEN (t.tgtype & 16) <> 0 THEN 'UPDATE' END,
         CASE WHEN (t.tgtype & 8) <> 0 THEN 'DELETE' END,
         CASE WHEN (t.tgtype & 32) <> 0 THEN 'TRUNCATE' END) AS events,
       pg_catalog.pg_get_triggerdef(t.oid, true) AS definition
FROM pg_catalog.pg_trigger t
JOIN pg_catalog.pg_class c ON c.oid = t.tgrelid
JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace
WHERE n.nspname = $1
  AND NOT t.tgisinternal
  AND c.relkind IN ('r', 'p', 'v')
  AND NOT c.relispartition
  AND c.relpersistence <> 't'
  AND c.relname = $2
ORDER BY c.relname, t.tgname

-- [view_definition] [String("HR"), String("EMP")]
SELECT pg_catalog.pg_get_viewdef(c.oid, true) AS definition
FROM pg_catalog.pg_class c
JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace
WHERE n.nspname = $1
  AND c.relname = $2
  AND c.relkind IN ('v', 'm')

-- [routine_source] [String("HR"), String("EMP"), String("FUNCTION"), String("HR"), String("EMP"), String("FUNCTION"), String("HR"), String("EMP"), String("FUNCTION")]
-- As postgres.sql for functions and procedures; a package is rebuilt from the stored spec / body text
-- (pkgspecsrc / pkgbodysrc hold what follows AS, without the final semicolon).
SELECT r.line, r.text
FROM (
  SELECT (ROW_NUMBER() OVER (ORDER BY pg_catalog.pg_get_function_identity_arguments(p.oid), p.oid))::int AS line,
         pg_catalog.pg_get_functiondef(p.oid) || E'\n' AS text
  FROM pg_catalog.pg_proc p
  JOIN pg_catalog.pg_namespace n ON n.oid = p.pronamespace
  WHERE n.nspname = $1
    AND p.proname = $2
    AND p.prokind IN ('f', 'p')
    AND p.prokind = CASE $3::text WHEN 'PROCEDURE' THEN 'p' WHEN 'FUNCTION' THEN 'f' END
  UNION ALL
  SELECT 1, 'CREATE OR REPLACE PACKAGE ' || pg_catalog.quote_ident(n.nspname) || '.' || pg_catalog.quote_ident(k.pkgname)
            || ' AS' || k.pkgspecsrc || ';' || E'\n'
  FROM pg_catalog._pkg k
  JOIN pg_catalog.pg_namespace n ON n.oid = k.pkgnamespace
  WHERE n.nspname = $4
    AND k.pkgname = $5
    AND $6::text = 'PACKAGE'
  UNION ALL
  SELECT 1, 'CREATE OR REPLACE PACKAGE BODY ' || pg_catalog.quote_ident(n.nspname) || '.' || pg_catalog.quote_ident(k.pkgname)
            || ' AS' || k.pkgbodysrc || ';' || E'\n'
  FROM pg_catalog._pkg k
  JOIN pg_catalog.pg_namespace n ON n.oid = k.pkgnamespace
  WHERE n.nspname = $7
    AND k.pkgname = $8
    AND k.pkgbodysrc IS NOT NULL
    AND $9::text = 'PACKAGE BODY'
) r
ORDER BY r.line

-- [reserved_words] []
-- catcode R = reserved, T = reserved (can be function or type name). C (col_name) and U are not reserved.
SELECT word FROM pg_catalog.pg_get_keywords() WHERE catcode IN ('R', 'T') ORDER BY word

-- [variant_probe] []
-- Compatibility mode (oracle / pg / mysql / sqlserver) and the PostgreSQL version the build is based on.
SELECT current_setting('database_mode') AS mode,
       current_setting('server_version_num') AS version_num

-- [referencing_keys] [String("HR"), String("EMP")]
-- Table designer rebuild: foreign keys of OTHER tables that reference this table.
SELECT n.nspname AS table_schema,
       c.relname AS table_name,
       con.conname AS constraint_name,
       a.attname AS column_name,
       k.pos::int AS position,
       ra.attname AS ref_column,
       CASE con.confdeltype WHEN 'c' THEN 'CASCADE' WHEN 'n' THEN 'SET NULL' WHEN 'd' THEN 'SET DEFAULT'
                         WHEN 'r' THEN 'RESTRICT' ELSE 'NO ACTION' END AS on_delete,
       CASE con.confupdtype WHEN 'c' THEN 'CASCADE' WHEN 'n' THEN 'SET NULL' WHEN 'd' THEN 'SET DEFAULT'
                         WHEN 'r' THEN 'RESTRICT' ELSE 'NO ACTION' END AS on_update
FROM pg_catalog.pg_constraint con
JOIN pg_catalog.pg_class c ON c.oid = con.conrelid
JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace
JOIN pg_catalog.pg_class t ON t.oid = con.confrelid
JOIN pg_catalog.pg_namespace tn ON tn.oid = t.relnamespace
CROSS JOIN LATERAL pg_catalog.unnest(con.conkey) WITH ORDINALITY AS k(attnum, pos)
JOIN pg_catalog.pg_attribute a ON a.attrelid = con.conrelid AND a.attnum = k.attnum
JOIN pg_catalog.pg_attribute ra ON ra.attrelid = con.confrelid AND ra.attnum = con.confkey[k.pos]
WHERE con.contype = 'f'
  AND tn.nspname = $1
  AND t.relname = $2
  AND con.conrelid <> con.confrelid
ORDER BY n.nspname, c.relname, con.conname, k.pos

-- [dependent_views] [String("HR"), String("EMP")]
-- Views / materialized views whose rewrite rule depends on this table (PG cannot drop the table under them).
SELECT DISTINCT vn.nspname AS schema_name,
       v.relname AS name,
       CASE v.relkind WHEN 'm' THEN 'MATERIALIZED VIEW' ELSE 'VIEW' END AS kind
FROM pg_catalog.pg_depend d
JOIN pg_catalog.pg_rewrite r ON r.oid = d.objid
JOIN pg_catalog.pg_class v ON v.oid = r.ev_class
JOIN pg_catalog.pg_namespace vn ON vn.oid = v.relnamespace
JOIN pg_catalog.pg_class t ON t.oid = d.refobjid
JOIN pg_catalog.pg_namespace tn ON tn.oid = t.relnamespace
WHERE d.classid = 'pg_catalog.pg_rewrite'::regclass
  AND d.refclassid = 'pg_catalog.pg_class'::regclass
  AND tn.nspname = $1
  AND t.relname = $2
  AND v.oid <> t.oid
ORDER BY 1, 2

-- [grants] [String("HR"), String("EMP")]
-- Privileges granted on the table to others (the owner's implicit privileges are skipped). grantee 0 = PUBLIC.
SELECT CASE WHEN a.grantee = 0 THEN 'PUBLIC' ELSE pg_catalog.pg_get_userbyid(a.grantee) END AS grantee,
       a.privilege_type AS privilege,
       a.is_grantable AS grantable
FROM pg_catalog.pg_class c
JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace
CROSS JOIN LATERAL pg_catalog.aclexplode(c.relacl) a
WHERE n.nspname = $1
  AND c.relname = $2
  AND a.grantee <> c.relowner
ORDER BY 1, 2

-- [table_storage] [String("HR"), String("EMP")]
-- reltuples is -1 (PG 14+) or 0 until the table is analyzed.
SELECT NULL::text AS engine,
       NULL::text AS charset,
       NULL::text AS collation,
       ts.spcname AS tablespace,
       c.reltuples::bigint AS row_count
FROM pg_catalog.pg_class c
JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace
LEFT JOIN pg_catalog.pg_tablespace ts ON ts.oid = c.reltablespace
WHERE n.nspname = $1
  AND c.relname = $2
  AND c.relkind IN ('r', 'p')
```

- [ ] **Step 5: 新建 `crates/dblens-dialect/tests/snapshots/meta_snapshots__oceanbase.snap`**

```text
---
source: crates/dblens-dialect/tests/meta_snapshots.rs
expression: out
---
-- [schemas] []
SELECT s.SCHEMA_NAME AS name,
       CASE WHEN s.SCHEMA_NAME IN ('mysql', 'sys', 'information_schema', 'performance_schema') THEN 1 ELSE 0 END AS is_system
FROM information_schema.SCHEMATA s
ORDER BY s.SCHEMA_NAME

-- [tables] [String("HR"), String("EMP"), String("%员工%"), String("%员工%")]
-- Partition from a join: CREATE_OPTIONS is NULL even for partitioned tables.
SELECT t.TABLE_NAME AS name,
       NULLIF(t.TABLE_COMMENT, '') AS comment,
       CASE WHEN p.PARTITION_METHOD IS NOT NULL
            THEN CONCAT(p.PARTITION_METHOD, ' (', COALESCE(p.PARTITION_EXPRESSION, ''), ')')
       END AS `partition`
FROM information_schema.TABLES t
LEFT JOIN information_schema.PARTITIONS p
  ON p.TABLE_SCHEMA = t.TABLE_SCHEMA AND p.TABLE_NAME = t.TABLE_NAME AND p.PARTITION_ORDINAL_POSITION = 1
WHERE t.TABLE_SCHEMA = ?
  AND t.TABLE_TYPE = 'BASE TABLE'
  AND t.TABLE_NAME = ?
  AND (UPPER(t.TABLE_NAME) LIKE UPPER(?) ESCAPE '!'
       OR UPPER(COALESCE(t.TABLE_COMMENT, '')) LIKE UPPER(?) ESCAPE '!')
ORDER BY t.TABLE_NAME

-- [views] [String("HR"), String("%员工%")]
SELECT v.TABLE_NAME AS name, NULL AS comment
FROM information_schema.VIEWS v
WHERE v.TABLE_SCHEMA = ?
  AND UPPER(v.TABLE_NAME) LIKE UPPER(?) ESCAPE '!'
ORDER BY v.TABLE_NAME

-- [routines] [String("HR"), String("%员工%"), String("%员工%")]
SELECT r.ROUTINE_NAME AS name, r.ROUTINE_TYPE AS kind, NULLIF(r.ROUTINE_COMMENT, '') AS comment
FROM information_schema.ROUTINES r
WHERE r.ROUTINE_SCHEMA = ?
  AND r.ROUTINE_TYPE IN ('PROCEDURE', 'FUNCTION')
  AND (UPPER(r.ROUTINE_NAME) LIKE UPPER(?) ESCAPE '!'
       OR UPPER(COALESCE(r.ROUTINE_COMMENT, '')) LIKE UPPER(?) ESCAPE '!')
ORDER BY r.ROUTINE_NAME, r.ROUTINE_TYPE

-- [columns] [String("HR"), String("EMP")]
-- Covers tables and views. default_value: expression defaults (EXTRA has DEFAULT_GENERATED) as raw text,
-- numeric/bit/year literals raw, all other literals SQL-quoted ('无名'), so the text is valid DDL.
SELECT c.TABLE_NAME AS table_name,
       c.COLUMN_NAME AS name,
       c.ORDINAL_POSITION AS ordinal,
       c.COLUMN_TYPE AS type_text,
       CASE WHEN c.IS_NULLABLE = 'YES' THEN 1 ELSE 0 END AS nullable,
       CASE
         WHEN c.COLUMN_DEFAULT IS NULL THEN NULL
         WHEN c.GENERATION_EXPRESSION <> '' THEN NULL
         WHEN c.EXTRA LIKE '%DEFAULT\_GENERATED%' THEN c.COLUMN_DEFAULT
         WHEN c.DATA_TYPE IN ('tinyint', 'smallint', 'mediumint', 'int', 'bigint', 'decimal', 'float', 'double', 'year', 'bit') THEN c.COLUMN_DEFAULT
         ELSE CONCAT('''', REPLACE(REPLACE(c.COLUMN_DEFAULT, '\\', '\\\\'), '''', ''''''), '''')
       END AS default_value,
       NULLIF(c.COLUMN_COMMENT, '') AS comment,
       CASE WHEN c.EXTRA LIKE '%auto\_increment%' THEN 1 ELSE 0 END AS auto_increment,
       NULLIF(c.GENERATION_EXPRESSION, '') AS `generated`
FROM information_schema.COLUMNS c
WHERE c.TABLE_SCHEMA = ?
  AND c.TABLE_NAME = ?
ORDER BY c.TABLE_NAME, c.ORDINAL_POSITION

-- [keys] [String("HR"), String("EMP")]
SELECT tc.TABLE_NAME AS table_name,
       tc.CONSTRAINT_NAME AS constraint_name,
       CASE tc.CONSTRAINT_TYPE WHEN 'PRIMARY KEY' THEN 'P' WHEN 'UNIQUE' THEN 'U' ELSE 'F' END AS constraint_type,
       k.COLUMN_NAME AS column_name,
       k.ORDINAL_POSITION AS position,
       k.REFERENCED_TABLE_SCHEMA AS ref_schema,
       k.REFERENCED_TABLE_NAME AS ref_table,
       k.REFERENCED_COLUMN_NAME AS ref_column,
       rc.DELETE_RULE AS on_delete,
       rc.UPDATE_RULE AS on_update
FROM information_schema.TABLE_CONSTRAINTS tc
JOIN information_schema.KEY_COLUMN_USAGE k
  ON k.CONSTRAINT_SCHEMA = tc.CONSTRAINT_SCHEMA
 AND k.TABLE_NAME = tc.TABLE_NAME
 AND k.CONSTRAINT_NAME = tc.CONSTRAINT_NAME
LEFT JOIN information_schema.REFERENTIAL_CONSTRAINTS rc
  ON tc.CONSTRAINT_TYPE = 'FOREIGN KEY'
 AND rc.CONSTRAINT_SCHEMA = tc.CONSTRAINT_SCHEMA
 AND rc.TABLE_NAME = tc.TABLE_NAME
 AND rc.CONSTRAINT_NAME = tc.CONSTRAINT_NAME
WHERE tc.TABLE_SCHEMA = ?
  AND tc.CONSTRAINT_TYPE IN ('PRIMARY KEY', 'UNIQUE', 'FOREIGN KEY')
  AND (tc.CONSTRAINT_TYPE <> 'UNIQUE'
       OR NOT EXISTS (SELECT 1 FROM information_schema.STATISTICS s2
                       WHERE s2.TABLE_SCHEMA = tc.TABLE_SCHEMA AND s2.TABLE_NAME = tc.TABLE_NAME
                         AND s2.INDEX_NAME = tc.CONSTRAINT_NAME AND s2.EXPRESSION IS NOT NULL))
  AND tc.TABLE_NAME = ?
ORDER BY tc.TABLE_NAME, tc.CONSTRAINT_NAME, k.ORDINAL_POSITION

-- [checks] [String("HR"), String("EMP")]
SELECT tc.TABLE_NAME AS table_name,
       tc.CONSTRAINT_NAME AS constraint_name,
       cc.CHECK_CLAUSE AS expression
FROM information_schema.TABLE_CONSTRAINTS tc
JOIN information_schema.CHECK_CONSTRAINTS cc
  ON cc.CONSTRAINT_SCHEMA = tc.CONSTRAINT_SCHEMA
 AND cc.CONSTRAINT_NAME = tc.CONSTRAINT_NAME
WHERE tc.TABLE_SCHEMA = ?
  AND tc.CONSTRAINT_TYPE = 'CHECK'
  AND tc.TABLE_NAME = ?
ORDER BY tc.TABLE_NAME, tc.CONSTRAINT_NAME

-- [indexes] [String("HR"), String("EMP")]
-- Non-unique indexes only (as in mysql.sql). A FULLTEXT index lists only hidden columns
-- (__doc_id_*, __word_segment_*), so with those left out it does not appear. Index columns are always
-- stored ascending (DESC is accepted and ignored).
SELECT s.TABLE_NAME AS table_name,
       s.INDEX_NAME AS index_name,
       0 AS is_unique,
       s.INDEX_TYPE AS method,
       s.COLUMN_NAME AS column_expr,
       s.SEQ_IN_INDEX AS position,
       0 AS descending
FROM information_schema.STATISTICS s
WHERE s.TABLE_SCHEMA = ?
  AND s.NON_UNIQUE = 1
  AND s.COLUMN_NAME NOT LIKE '!_!_%' ESCAPE '!'
  AND s.TABLE_NAME = ?
ORDER BY s.TABLE_NAME, s.INDEX_NAME, s.SEQ_IN_INDEX

-- [triggers] [String("HR"), String("EMP")]
-- definition: a CREATE TRIGGER statement rebuilt from TRIGGERS (FOLLOWS/PRECEDES order and DEFINER are not included).
-- MySQL allows exactly one event per trigger, so events is that single event.
SELECT t.EVENT_OBJECT_TABLE AS table_name,
       t.TRIGGER_NAME AS name,
       t.ACTION_TIMING AS timing,
       t.EVENT_MANIPULATION AS events,
       CONCAT('CREATE TRIGGER `', REPLACE(t.TRIGGER_NAME, '`', '``'), '` ', t.ACTION_TIMING, ' ', t.EVENT_MANIPULATION,
              ' ON `', REPLACE(t.EVENT_OBJECT_TABLE, '`', '``'), '` FOR EACH ROW ', t.ACTION_STATEMENT) AS definition
FROM information_schema.TRIGGERS t
WHERE t.EVENT_OBJECT_SCHEMA = ?
  AND t.EVENT_OBJECT_TABLE = ?
ORDER BY t.EVENT_OBJECT_TABLE, t.TRIGGER_NAME

-- [view_definition] [String("HR"), String("EMP")]
SELECT v.VIEW_DEFINITION AS definition
FROM information_schema.VIEWS v
WHERE v.TABLE_SCHEMA = ?
  AND v.TABLE_NAME = ?

-- [routine_source] [String("HR"), String("EMP"), String("FUNCTION")]
-- SHOW CREATE cannot take bound params, so the CREATE statement is rebuilt from ROUTINES + PARAMETERS
-- (params, RETURNS, DETERMINISTIC, SQL SECURITY, COMMENT, body). One row, line = 1. Character-set clauses are not reproduced.
SELECT 1 AS line,
       CONCAT('CREATE ', r.ROUTINE_TYPE, ' `', REPLACE(r.ROUTINE_NAME, '`', '``'), '`(',
              COALESCE((SELECT GROUP_CONCAT(
                                 CONCAT(CASE WHEN r.ROUTINE_TYPE = 'PROCEDURE' THEN CONCAT(COALESCE(p.PARAMETER_MODE, 'IN'), ' ') ELSE '' END,
                                        '`', REPLACE(p.PARAMETER_NAME, '`', '``'), '` ', p.DTD_IDENTIFIER)
                                 ORDER BY p.ORDINAL_POSITION SEPARATOR ', ')
                          FROM information_schema.PARAMETERS p
                         WHERE p.SPECIFIC_SCHEMA = r.ROUTINE_SCHEMA AND p.SPECIFIC_NAME = r.SPECIFIC_NAME
                           AND p.ROUTINE_TYPE = r.ROUTINE_TYPE AND p.ORDINAL_POSITION > 0), ''),
              ')',
              CASE WHEN r.ROUTINE_TYPE = 'FUNCTION' THEN CONCAT(' RETURNS ', r.DTD_IDENTIFIER) ELSE '' END,
              CHAR(10), '    ', r.SQL_DATA_ACCESS,
              CASE WHEN r.IS_DETERMINISTIC = 'YES' THEN ' DETERMINISTIC' ELSE '' END,
              ' SQL SECURITY ', r.SECURITY_TYPE,
              CASE WHEN r.ROUTINE_COMMENT <> '' THEN CONCAT(' COMMENT ''', REPLACE(REPLACE(r.ROUTINE_COMMENT, '\\', '\\\\'), '''', ''''''), '''') ELSE '' END,
              CHAR(10), COALESCE(r.ROUTINE_DEFINITION, '')) AS text
FROM information_schema.ROUTINES r
WHERE r.ROUTINE_SCHEMA = ?
  AND r.ROUTINE_NAME = ?
  AND r.ROUTINE_TYPE = ?

-- [variant_probe] []
SHOW VARIABLES LIKE 'polardb_version'

-- [referencing_keys] [String("HR"), String("EMP")]
-- Table designer rebuild: foreign keys of OTHER tables that reference this table (dropped and recreated around the swap).
SELECT k.TABLE_SCHEMA AS table_schema,
       k.TABLE_NAME AS table_name,
       k.CONSTRAINT_NAME AS constraint_name,
       k.COLUMN_NAME AS column_name,
       k.ORDINAL_POSITION AS position,
       k.REFERENCED_COLUMN_NAME AS ref_column,
       rc.DELETE_RULE AS on_delete,
       rc.UPDATE_RULE AS on_update
FROM information_schema.KEY_COLUMN_USAGE k
JOIN information_schema.REFERENTIAL_CONSTRAINTS rc
  ON rc.CONSTRAINT_SCHEMA = k.CONSTRAINT_SCHEMA
 AND rc.TABLE_NAME = k.TABLE_NAME
 AND rc.CONSTRAINT_NAME = k.CONSTRAINT_NAME
WHERE k.REFERENCED_TABLE_SCHEMA = ?
  AND k.REFERENCED_TABLE_NAME = ?
  AND NOT (k.TABLE_SCHEMA = k.REFERENCED_TABLE_SCHEMA AND k.TABLE_NAME = k.REFERENCED_TABLE_NAME)
ORDER BY k.TABLE_SCHEMA, k.TABLE_NAME, k.CONSTRAINT_NAME, k.ORDINAL_POSITION

-- [table_storage] [String("HR"), String("EMP")]
-- Table designer: engine / charset / collation kept by a rebuild, and the row estimate shown in the designer.
SELECT t.ENGINE AS engine,
       (SELECT MAX(c.CHARACTER_SET_NAME) FROM information_schema.COLLATION_CHARACTER_SET_APPLICABILITY c
         WHERE c.COLLATION_NAME = t.TABLE_COLLATION) AS charset,
       t.TABLE_COLLATION AS collation,
       NULL AS tablespace,
       t.TABLE_ROWS AS row_count
FROM information_schema.TABLES t
WHERE t.TABLE_SCHEMA = ?
  AND t.TABLE_NAME = ?
```

- [ ] **Step 6: 新建 `crates/dblens-dialect/tests/snapshots/meta_snapshots__opengauss.snap`**

```text
---
source: crates/dblens-dialect/tests/meta_snapshots.rs
expression: out
---
-- [schemas] []
-- Built-in schemas besides pg_* and information_schema (dbe_*, pkg_*, db4ai, ...).
SELECT n.nspname AS name,
       (n.nspname LIKE 'pg!_%' ESCAPE '!' OR n.nspname LIKE 'dbe!_%' ESCAPE '!' OR n.nspname LIKE 'pkg!_%' ESCAPE '!'
        OR n.nspname IN ('information_schema', 'blockchain', 'coverage', 'cstore', 'db4ai', 'snapshot',
                         'sqladvisor', 'xmltype')) AS is_system
FROM pg_catalog.pg_namespace n
ORDER BY n.nspname

-- [tables] [String("HR"), String("EMP"), String("%员工%"), String("%员工%")]
-- Partitions live in pg_partition, not pg_class: a partitioned table is relkind r with parttype p.
SELECT c.relname AS name,
       pg_catalog.obj_description(c.oid, 'pg_class') AS comment,
       CASE WHEN c.parttype = 'p' THEN
         (SELECT CASE p.partstrategy WHEN 'r' THEN 'RANGE' WHEN 'l' THEN 'LIST' WHEN 'h' THEN 'HASH'
                                     WHEN 'i' THEN 'INTERVAL' ELSE 'UNKNOWN' END
                 || ' (' || (SELECT pg_catalog.string_agg(a.attname, ', ' ORDER BY a.attnum)
                               FROM pg_catalog.pg_attribute a
                              WHERE a.attrelid = c.oid AND a.attnum = ANY (p.partkey::int2[])) || ')'
            FROM pg_catalog.pg_partition p
           WHERE p.parentid = c.oid AND p.parttype = 'r')
       END AS partition
FROM pg_catalog.pg_class c
JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace
WHERE n.nspname = $1
  AND c.relkind IN ('r', 'p')
  AND c.relpersistence <> 't'
  AND c.relname = $2
  AND (UPPER(c.relname) LIKE UPPER($3) ESCAPE '!'
       OR UPPER(pg_catalog.obj_description(c.oid, 'pg_class')) LIKE UPPER($4) ESCAPE '!')
ORDER BY c.relname

-- [views] [String("HR"), String("%员工%"), String("%员工%")]
SELECT c.relname AS name,
       pg_catalog.obj_description(c.oid, 'pg_class') AS comment
FROM pg_catalog.pg_class c
JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace
WHERE n.nspname = $1
  AND c.relkind = 'v'
  AND (UPPER(c.relname) LIKE UPPER($2) ESCAPE '!'
       OR UPPER(pg_catalog.obj_description(c.oid, 'pg_class')) LIKE UPPER($3) ESCAPE '!')
ORDER BY c.relname

-- [materialized_views] [String("HR"), String("%员工%"), String("%员工%")]
SELECT c.relname AS name,
       pg_catalog.obj_description(c.oid, 'pg_class') AS comment
FROM pg_catalog.pg_class c
JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace
WHERE n.nspname = $1
  AND c.relkind = 'm'
  AND (UPPER(c.relname) LIKE UPPER($2) ESCAPE '!'
       OR UPPER(pg_catalog.obj_description(c.oid, 'pg_class')) LIKE UPPER($3) ESCAPE '!')
ORDER BY c.relname

-- [sequences] [String("HR"), String("%员工%"), String("%员工%")]
-- No pg_sequence: parameters come from pg_sequence_parameters, which has no cache size.
SELECT c.relname AS name,
       (pg_catalog.pg_sequence_parameters(c.oid)).start_value AS start_value,
       (pg_catalog.pg_sequence_parameters(c.oid)).increment AS increment,
       (pg_catalog.pg_sequence_parameters(c.oid)).minimum_value AS min_value,
       (pg_catalog.pg_sequence_parameters(c.oid)).maximum_value AS max_value,
       (pg_catalog.pg_sequence_parameters(c.oid)).cycle_option AS cycle,
       NULL::bigint AS cache
FROM pg_catalog.pg_class c
JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace
WHERE n.nspname = $1
  AND c.relkind = 'S'
  AND NOT EXISTS (SELECT 1 FROM pg_catalog.pg_depend d
                  WHERE d.classid = 'pg_catalog.pg_class'::regclass AND d.objid = c.oid
                    AND d.refclassid = 'pg_catalog.pg_class'::regclass AND d.deptype IN ('a', 'i'))
  AND (UPPER(c.relname) LIKE UPPER($2) ESCAPE '!'
       OR UPPER(pg_catalog.obj_description(c.oid, 'pg_class')) LIKE UPPER($3) ESCAPE '!')
ORDER BY c.relname

-- [routines] [String("HR"), String("%员工%"), String("%员工%"), String("HR"), String("%员工%")]
-- Functions and procedures (aggregates are prokind f too, proisagg filters them; routines of packages have
-- propackageid set), plus packages from gs_package (A compatibility only). Every package is listed as
-- PACKAGE and PACKAGE BODY.
SELECT r.name, r.kind, r.comment
FROM (
  SELECT p.proname AS name,
         CASE p.prokind WHEN 'p' THEN 'PROCEDURE' ELSE 'FUNCTION' END AS kind,
         MAX(pg_catalog.obj_description(p.oid, 'pg_proc')) AS comment
  FROM pg_catalog.pg_proc p
  JOIN pg_catalog.pg_namespace n ON n.oid = p.pronamespace
  WHERE n.nspname = $1
    AND p.prokind IN ('f', 'p') AND NOT p.proisagg AND NOT p.proiswindow AND p.propackageid = 0
    AND (UPPER(p.proname) LIKE UPPER($2) ESCAPE '!'
         OR UPPER(pg_catalog.obj_description(p.oid, 'pg_proc')) LIKE UPPER($3) ESCAPE '!')
  GROUP BY p.proname, 2
  UNION ALL
  SELECT k.pkgname, x.kind, NULL
  FROM pg_catalog.gs_package k
  JOIN pg_catalog.pg_namespace n ON n.oid = k.pkgnamespace
  CROSS JOIN (SELECT 'PACKAGE' AS kind UNION ALL SELECT 'PACKAGE BODY') x
  WHERE n.nspname = $4
    AND UPPER(k.pkgname) LIKE UPPER($5) ESCAPE '!'
) r
ORDER BY r.name, r.kind

-- [synonyms] [String("HR"), String("%员工%")]
SELECT s.synname AS name,
       s.synobjschema AS target_schema,
       s.synobjname AS target_name,
       NULL AS db_link
FROM pg_catalog.pg_synonym s
JOIN pg_catalog.pg_namespace n ON n.oid = s.synnamespace
WHERE n.nspname = $1
  AND UPPER(s.synname) LIKE UPPER($2) ESCAPE '!'
ORDER BY s.synname

-- [columns] [String("HR"), String("EMP")]
-- No identity columns (serial only); a generated column has pg_attrdef.adgencol = 's'.
SELECT c.relname AS table_name,
       a.attname AS name,
       (ROW_NUMBER() OVER (PARTITION BY c.oid ORDER BY a.attnum))::int AS ordinal,
       pg_catalog.format_type(a.atttypid, a.atttypmod) AS type_text,
       NOT a.attnotnull AS nullable,
       CASE WHEN d.adgencol = 's' THEN NULL ELSE pg_catalog.pg_get_expr(d.adbin, d.adrelid) END AS default_value,
       pg_catalog.col_description(c.oid, a.attnum) AS comment,
       ((d.adbin IS NOT NULL AND pg_catalog.pg_get_expr(d.adbin, d.adrelid) LIKE 'nextval(%'
            AND EXISTS (SELECT 1 FROM pg_catalog.pg_depend dp
                        WHERE dp.refclassid = 'pg_catalog.pg_class'::regclass AND dp.refobjid = c.oid
                          AND dp.refobjsubid = a.attnum AND dp.deptype = 'a'
                          AND dp.classid = 'pg_catalog.pg_class'::regclass))) AS auto_increment,
       CASE WHEN d.adgencol = 's' THEN pg_catalog.pg_get_expr(d.adbin, d.adrelid) END AS generated
FROM pg_catalog.pg_class c
JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace
JOIN pg_catalog.pg_attribute a ON a.attrelid = c.oid AND a.attnum > 0 AND NOT a.attisdropped
LEFT JOIN pg_catalog.pg_attrdef d ON d.adrelid = a.attrelid AND d.adnum = a.attnum
WHERE n.nspname = $1
  AND c.relkind IN ('r', 'p', 'v', 'm')
  AND c.relpersistence <> 't'
  AND c.relname = $2
ORDER BY c.relname, a.attnum

-- [keys] [String("HR"), String("EMP")]
-- No LATERAL / WITH ORDINALITY: key positions come from generate_series in a subquery.
SELECT c.relname AS table_name,
       con.conname AS constraint_name,
       CASE con.contype WHEN 'p' THEN 'P' WHEN 'u' THEN 'U' ELSE 'F' END AS constraint_type,
       a.attname AS column_name,
       con.pos::int AS position,
       CASE WHEN con.contype = 'f' THEN rn.nspname END AS ref_schema,
       CASE WHEN con.contype = 'f' THEN rc.relname END AS ref_table,
       ra.attname AS ref_column,
       CASE WHEN con.contype = 'f' THEN
         CASE con.confdeltype WHEN 'c' THEN 'CASCADE' WHEN 'n' THEN 'SET NULL' WHEN 'd' THEN 'SET DEFAULT'
                              WHEN 'r' THEN 'RESTRICT' ELSE 'NO ACTION' END END AS on_delete,
       CASE WHEN con.contype = 'f' THEN
         CASE con.confupdtype WHEN 'c' THEN 'CASCADE' WHEN 'n' THEN 'SET NULL' WHEN 'd' THEN 'SET DEFAULT'
                              WHEN 'r' THEN 'RESTRICT' ELSE 'NO ACTION' END END AS on_update
FROM (SELECT x.*, pg_catalog.generate_series(1, pg_catalog.array_length(x.conkey, 1)) AS pos
        FROM pg_catalog.pg_constraint x) con
JOIN pg_catalog.pg_class c ON c.oid = con.conrelid
JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace
JOIN pg_catalog.pg_attribute a ON a.attrelid = con.conrelid AND a.attnum = con.conkey[con.pos]
LEFT JOIN pg_catalog.pg_class rc ON rc.oid = con.confrelid
LEFT JOIN pg_catalog.pg_namespace rn ON rn.oid = rc.relnamespace
LEFT JOIN pg_catalog.pg_attribute ra ON ra.attrelid = con.confrelid AND ra.attnum = con.confkey[con.pos]
WHERE n.nspname = $1
  AND con.contype IN ('p', 'u', 'f')
  AND c.relkind IN ('r', 'p')
  AND c.relpersistence <> 't'
  AND c.relname = $2
ORDER BY c.relname, con.conname, con.pos

-- [checks] [String("HR"), String("EMP")]
SELECT c.relname AS table_name,
       con.conname AS constraint_name,
       pg_catalog.pg_get_expr(con.conbin, con.conrelid) AS expression
FROM pg_catalog.pg_constraint con
JOIN pg_catalog.pg_class c ON c.oid = con.conrelid
JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace
WHERE n.nspname = $1
  AND con.contype = 'c'
  AND c.relkind IN ('r', 'p')
  AND c.relpersistence <> 't'
  AND c.relname = $2
ORDER BY c.relname, con.conname

-- [indexes] [String("HR"), String("EMP")]
-- No LATERAL / WITH ORDINALITY: key positions come from generate_series over indnkeyatts.
SELECT c.relname AS table_name,
       ic.relname AS index_name,
       i.indisunique AS is_unique,
       UPPER(am.amname) AS method,
       CASE WHEN i.indkey[i.pos - 1] = 0 THEN pg_catalog.pg_get_indexdef(i.indexrelid, i.pos::int, true) ELSE a.attname END AS column_expr,
       i.pos::int AS position,
       ((i.indoption[i.pos - 1] & 1) = 1) AS descending
FROM (SELECT x.*, pg_catalog.generate_series(1, x.indnkeyatts) AS pos FROM pg_catalog.pg_index x) i
JOIN pg_catalog.pg_class c ON c.oid = i.indrelid
JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace
JOIN pg_catalog.pg_class ic ON ic.oid = i.indexrelid
JOIN pg_catalog.pg_am am ON am.oid = ic.relam
LEFT JOIN pg_catalog.pg_attribute a ON a.attrelid = i.indrelid AND a.attnum = i.indkey[i.pos - 1] AND i.indkey[i.pos - 1] > 0
WHERE n.nspname = $1
  AND c.relkind IN ('r', 'p', 'm')
  AND c.relpersistence <> 't'
  AND NOT EXISTS (SELECT 1 FROM pg_catalog.pg_constraint cc
                  WHERE cc.conindid = i.indexrelid AND cc.contype IN ('p', 'u') AND cc.conrelid = i.indrelid)
  AND c.relname = $2
ORDER BY c.relname, ic.relname, i.pos

-- [triggers] [String("HR"), String("EMP")]
SELECT c.relname AS table_name,
       t.tgname AS name,
       CASE WHEN (t.tgtype & 64) <> 0 THEN 'INSTEAD OF' WHEN (t.tgtype & 2) <> 0 THEN 'BEFORE' ELSE 'AFTER' END AS timing,
       concat_ws(',',
         CASE WHEN (t.tgtype & 4) <> 0 THEN 'INSERT' END,
         CASE WHEN (t.tgtype & 16) <> 0 THEN 'UPDATE' END,
         CASE WHEN (t.tgtype & 8) <> 0 THEN 'DELETE' END,
         CASE WHEN (t.tgtype & 32) <> 0 THEN 'TRUNCATE' END) AS events,
       pg_catalog.pg_get_triggerdef(t.oid, true) AS definition
FROM pg_catalog.pg_trigger t
JOIN pg_catalog.pg_class c ON c.oid = t.tgrelid
JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace
WHERE n.nspname = $1
  AND NOT t.tgisinternal
  AND c.relkind IN ('r', 'p', 'v')
  AND c.relpersistence <> 't'
  AND c.relname = $2
ORDER BY c.relname, t.tgname

-- [view_definition] [String("HR"), String("EMP")]
SELECT pg_catalog.pg_get_viewdef(c.oid, true) AS definition
FROM pg_catalog.pg_class c
JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace
WHERE n.nspname = $1
  AND c.relname = $2
  AND c.relkind IN ('v', 'm')

-- [routine_source] [String("HR"), String("EMP"), String("FUNCTION"), String("HR"), String("EMP"), String("FUNCTION"), String("HR"), String("EMP"), String("FUNCTION")]
-- A package is rebuilt from gs_package: pkgspecsrc / pkgbodydeclsrc start with "PACKAGE DECLARE", which is
-- dropped, and end without the final semicolon. (substring, not regexp_replace with '': in A compatibility
-- '' is NULL.)
SELECT r.line, r.text
FROM (
  SELECT (ROW_NUMBER() OVER (ORDER BY pg_catalog.pg_get_function_identity_arguments(p.oid), p.oid))::int AS line,
         pg_catalog.pg_get_functiondef(p.oid) || E'\n' AS text
  FROM pg_catalog.pg_proc p
  JOIN pg_catalog.pg_namespace n ON n.oid = p.pronamespace
  WHERE n.nspname = $1
    AND p.proname = $2
    AND p.prokind IN ('f', 'p') AND NOT p.proisagg AND NOT p.proiswindow AND p.propackageid = 0
    AND p.prokind = CASE $3::text WHEN 'PROCEDURE' THEN 'p' WHEN 'FUNCTION' THEN 'f' END
  UNION ALL
  SELECT 1, 'CREATE OR REPLACE PACKAGE ' || pg_catalog.quote_ident(n.nspname) || '.' || pg_catalog.quote_ident(k.pkgname)
            || ' AS' || substring(k.pkgspecsrc FROM '^\s*PACKAGE\s+DECLARE(.*)$') || ';' || E'\n'
  FROM pg_catalog.gs_package k
  JOIN pg_catalog.pg_namespace n ON n.oid = k.pkgnamespace
  WHERE n.nspname = $4
    AND k.pkgname = $5
    AND $6::text = 'PACKAGE'
  UNION ALL
  SELECT 1, 'CREATE OR REPLACE PACKAGE BODY ' || pg_catalog.quote_ident(n.nspname) || '.' || pg_catalog.quote_ident(k.pkgname)
            || ' AS' || substring(k.pkgbodydeclsrc FROM '^\s*PACKAGE\s+DECLARE(.*)$') || ';' || E'\n'
  FROM pg_catalog.gs_package k
  JOIN pg_catalog.pg_namespace n ON n.oid = k.pkgnamespace
  WHERE n.nspname = $7
    AND k.pkgname = $8
    AND substring(k.pkgbodydeclsrc FROM '^\s*PACKAGE\s+DECLARE(.*)$') IS NOT NULL
    AND $9::text = 'PACKAGE BODY'
) r
ORDER BY r.line

-- [reserved_words] []
-- catcode R = reserved, T = reserved (can be function or type name). C (col_name) and U are not reserved.
SELECT word FROM pg_catalog.pg_get_keywords() WHERE catcode IN ('R', 'T') ORDER BY word

-- [variant_probe] []
-- Compatibility of the current database (A / B / C / PG) and the PostgreSQL version the build is based on.
SELECT datcompatibility AS mode,
       current_setting('server_version_num') AS version_num
FROM pg_catalog.pg_database
WHERE datname = current_database()

-- [referencing_keys] [String("HR"), String("EMP")]
SELECT n.nspname AS table_schema,
       c.relname AS table_name,
       con.conname AS constraint_name,
       a.attname AS column_name,
       con.pos::int AS position,
       ra.attname AS ref_column,
       CASE con.confdeltype WHEN 'c' THEN 'CASCADE' WHEN 'n' THEN 'SET NULL' WHEN 'd' THEN 'SET DEFAULT'
                         WHEN 'r' THEN 'RESTRICT' ELSE 'NO ACTION' END AS on_delete,
       CASE con.confupdtype WHEN 'c' THEN 'CASCADE' WHEN 'n' THEN 'SET NULL' WHEN 'd' THEN 'SET DEFAULT'
                         WHEN 'r' THEN 'RESTRICT' ELSE 'NO ACTION' END AS on_update
FROM (SELECT x.*, pg_catalog.generate_series(1, pg_catalog.array_length(x.conkey, 1)) AS pos
        FROM pg_catalog.pg_constraint x) con
JOIN pg_catalog.pg_class c ON c.oid = con.conrelid
JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace
JOIN pg_catalog.pg_class t ON t.oid = con.confrelid
JOIN pg_catalog.pg_namespace tn ON tn.oid = t.relnamespace
JOIN pg_catalog.pg_attribute a ON a.attrelid = con.conrelid AND a.attnum = con.conkey[con.pos]
JOIN pg_catalog.pg_attribute ra ON ra.attrelid = con.confrelid AND ra.attnum = con.confkey[con.pos]
WHERE con.contype = 'f'
  AND tn.nspname = $1
  AND t.relname = $2
  AND con.conrelid <> con.confrelid
ORDER BY n.nspname, c.relname, con.conname, con.pos

-- [dependent_views] [String("HR"), String("EMP")]
-- Views / materialized views whose rewrite rule depends on this table (PG cannot drop the table under them).
SELECT DISTINCT vn.nspname AS schema_name,
       v.relname AS name,
       CASE v.relkind WHEN 'm' THEN 'MATERIALIZED VIEW' ELSE 'VIEW' END AS kind
FROM pg_catalog.pg_depend d
JOIN pg_catalog.pg_rewrite r ON r.oid = d.objid
JOIN pg_catalog.pg_class v ON v.oid = r.ev_class
JOIN pg_catalog.pg_namespace vn ON vn.oid = v.relnamespace
JOIN pg_catalog.pg_class t ON t.oid = d.refobjid
JOIN pg_catalog.pg_namespace tn ON tn.oid = t.relnamespace
WHERE d.classid = 'pg_catalog.pg_rewrite'::regclass
  AND d.refclassid = 'pg_catalog.pg_class'::regclass
  AND tn.nspname = $1
  AND t.relname = $2
  AND v.oid <> t.oid
ORDER BY 1, 2

-- [grants] [String("HR"), String("EMP")]
-- No LATERAL: aclexplode runs in a subquery.
SELECT CASE WHEN (c.acl).grantee = 0 THEN 'PUBLIC' ELSE pg_catalog.pg_get_userbyid((c.acl).grantee) END AS grantee,
       (c.acl).privilege_type AS privilege,
       (c.acl).is_grantable AS grantable
FROM (SELECT x.relname, x.relnamespace, x.relowner, pg_catalog.aclexplode(x.relacl) AS acl
        FROM pg_catalog.pg_class x) c
JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace
WHERE n.nspname = $1
  AND c.relname = $2
  AND (c.acl).grantee <> c.relowner
ORDER BY 1, 2

-- [table_storage] [String("HR"), String("EMP")]
-- reltuples is -1 (PG 14+) or 0 until the table is analyzed.
SELECT NULL::text AS engine,
       NULL::text AS charset,
       NULL::text AS collation,
       ts.spcname AS tablespace,
       c.reltuples::bigint AS row_count
FROM pg_catalog.pg_class c
JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace
LEFT JOIN pg_catalog.pg_tablespace ts ON ts.oid = c.reltablespace
WHERE n.nspname = $1
  AND c.relname = $2
  AND c.relkind IN ('r', 'p')
```

- [ ] **Step 7: 修改 `crates/dblens-dialect/tests/snapshots/meta_snapshots__postgres.snap`**

```diff
--- a/crates/dblens-dialect/tests/snapshots/meta_snapshots__postgres.snap
+++ b/crates/dblens-dialect/tests/snapshots/meta_snapshots__postgres.snap
@@ -81,20 +81,20 @@ GROUP BY p.proname, 2
 ORDER BY p.proname, 2
 
 -- [columns] [String("HR"), String("EMP")]
+-- No comparisons with '' anywhere: in the Oracle compatibility mode of KingbaseES / openGauss '' is NULL.
 SELECT c.relname AS table_name,
        a.attname AS name,
        (ROW_NUMBER() OVER (PARTITION BY c.oid ORDER BY a.attnum))::int AS ordinal,
        pg_catalog.format_type(a.atttypid, a.atttypmod) AS type_text,
        NOT a.attnotnull AS nullable,
-       CASE WHEN a.attgenerated = '' THEN pg_catalog.pg_get_expr(d.adbin, d.adrelid) END AS default_value,
+       CASE WHEN a.attgenerated <> 's' THEN pg_catalog.pg_get_expr(d.adbin, d.adrelid) END AS default_value,
        pg_catalog.col_description(c.oid, a.attnum) AS comment,
-       (a.attidentity <> ''
-        OR (d.adbin IS NOT NULL AND pg_catalog.pg_get_expr(d.adbin, d.adrelid) LIKE 'nextval(%'
+       (a.attidentity IN ('a', 'd') OR (d.adbin IS NOT NULL AND pg_catalog.pg_get_expr(d.adbin, d.adrelid) LIKE 'nextval(%'
             AND EXISTS (SELECT 1 FROM pg_catalog.pg_depend dp
                         WHERE dp.refclassid = 'pg_catalog.pg_class'::regclass AND dp.refobjid = c.oid
                           AND dp.refobjsubid = a.attnum AND dp.deptype = 'a'
                           AND dp.classid = 'pg_catalog.pg_class'::regclass))) AS auto_increment,
-       CASE WHEN a.attgenerated <> '' THEN pg_catalog.pg_get_expr(d.adbin, d.adrelid) END AS generated
+       CASE WHEN a.attgenerated = 's' THEN pg_catalog.pg_get_expr(d.adbin, d.adrelid) END AS generated
 FROM pg_catalog.pg_class c
 JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace
 JOIN pg_catalog.pg_attribute a ON a.attrelid = c.oid AND a.attnum > 0 AND NOT a.attisdropped
```

- [ ] **Step 8: 新建 `crates/dblens-dialect/tests/snapshots/meta_snapshots__tidb.snap`**

```text
---
source: crates/dblens-dialect/tests/meta_snapshots.rs
expression: out
---
-- [schemas] []
SELECT s.SCHEMA_NAME AS name,
       CASE WHEN s.SCHEMA_NAME IN ('mysql', 'sys', 'information_schema', 'performance_schema') THEN 1 ELSE 0 END AS is_system
FROM information_schema.SCHEMATA s
ORDER BY s.SCHEMA_NAME

-- [tables] [String("HR"), String("EMP"), String("%员工%"), String("%员工%")]
-- Partition from a join: a correlated subquery over information_schema returns NULL for every row once
-- more than one table matches (TiDB 8.5).
SELECT t.TABLE_NAME AS name,
       NULLIF(t.TABLE_COMMENT, '') AS comment,
       CASE WHEN p.PARTITION_METHOD IS NOT NULL
            THEN CONCAT(p.PARTITION_METHOD, ' (', COALESCE(p.PARTITION_EXPRESSION, ''), ')')
       END AS `partition`
FROM information_schema.TABLES t
LEFT JOIN information_schema.PARTITIONS p
  ON p.TABLE_SCHEMA = t.TABLE_SCHEMA AND p.TABLE_NAME = t.TABLE_NAME AND p.PARTITION_ORDINAL_POSITION = 1
WHERE t.TABLE_SCHEMA = ?
  AND t.TABLE_TYPE = 'BASE TABLE'
  AND t.TABLE_NAME = ?
  AND (UPPER(t.TABLE_NAME) LIKE UPPER(?) ESCAPE '!'
       OR UPPER(COALESCE(t.TABLE_COMMENT, '')) LIKE UPPER(?) ESCAPE '!')
ORDER BY t.TABLE_NAME

-- [views] [String("HR"), String("%员工%")]
SELECT v.TABLE_NAME AS name, NULL AS comment
FROM information_schema.VIEWS v
WHERE v.TABLE_SCHEMA = ?
  AND UPPER(v.TABLE_NAME) LIKE UPPER(?) ESCAPE '!'
ORDER BY v.TABLE_NAME

-- [sequences] [String("HR"), String("%员工%")]
-- TiDB supports CREATE SEQUENCE (MySQL has no sequences); CACHE is 1/0 and the cache size is CACHE_VALUE.
SELECT s.SEQUENCE_NAME AS name,
       s.START AS start_value,
       s.INCREMENT AS increment,
       s.MIN_VALUE AS min_value,
       s.MAX_VALUE AS max_value,
       s.CYCLE AS cycle,
       CASE WHEN s.CACHE = 1 THEN s.CACHE_VALUE END AS cache
FROM information_schema.SEQUENCES s
WHERE s.SEQUENCE_SCHEMA = ?
  AND UPPER(s.SEQUENCE_NAME) LIKE UPPER(?) ESCAPE '!'
ORDER BY s.SEQUENCE_NAME

-- [routines] [String("HR"), String("%员工%"), String("%员工%")]
SELECT r.ROUTINE_NAME AS name, r.ROUTINE_TYPE AS kind, NULLIF(r.ROUTINE_COMMENT, '') AS comment
FROM information_schema.ROUTINES r
WHERE r.ROUTINE_SCHEMA = ?
  AND r.ROUTINE_TYPE IN ('PROCEDURE', 'FUNCTION')
  AND (UPPER(r.ROUTINE_NAME) LIKE UPPER(?) ESCAPE '!'
       OR UPPER(COALESCE(r.ROUTINE_COMMENT, '')) LIKE UPPER(?) ESCAPE '!')
ORDER BY r.ROUTINE_NAME, r.ROUTINE_TYPE

-- [columns] [String("HR"), String("EMP")]
-- As mysql.sql, plus AUTO_RANDOM: it is not in EXTRA; the table's TIDB_ROW_ID_SHARDING_INFO is
-- PK_AUTO_RANDOM_BITS=n and the column is its (single-column, clustered) primary key.
SELECT c.TABLE_NAME AS table_name,
       c.COLUMN_NAME AS name,
       c.ORDINAL_POSITION AS ordinal,
       c.COLUMN_TYPE AS type_text,
       CASE WHEN c.IS_NULLABLE = 'YES' THEN 1 ELSE 0 END AS nullable,
       CASE
         WHEN c.COLUMN_DEFAULT IS NULL THEN NULL
         WHEN c.GENERATION_EXPRESSION <> '' THEN NULL
         WHEN c.EXTRA LIKE '%DEFAULT\_GENERATED%' THEN c.COLUMN_DEFAULT
         WHEN c.DATA_TYPE IN ('tinyint', 'smallint', 'mediumint', 'int', 'bigint', 'decimal', 'float', 'double', 'year', 'bit') THEN c.COLUMN_DEFAULT
         ELSE CONCAT('''', REPLACE(REPLACE(c.COLUMN_DEFAULT, '\\', '\\\\'), '''', ''''''), '''')
       END AS default_value,
       NULLIF(c.COLUMN_COMMENT, '') AS comment,
       CASE WHEN c.EXTRA LIKE '%auto\_increment%'
              OR (c.COLUMN_KEY = 'PRI' AND t.TIDB_ROW_ID_SHARDING_INFO LIKE 'PK!_AUTO!_RANDOM!_BITS%' ESCAPE '!')
            THEN 1 ELSE 0 END AS auto_increment,
       NULLIF(c.GENERATION_EXPRESSION, '') AS `generated`
FROM information_schema.COLUMNS c
JOIN information_schema.TABLES t ON t.TABLE_SCHEMA = c.TABLE_SCHEMA AND t.TABLE_NAME = c.TABLE_NAME
WHERE c.TABLE_SCHEMA = ?
  AND c.TABLE_NAME = ?
ORDER BY c.TABLE_NAME, c.ORDINAL_POSITION

-- [keys] [String("HR"), String("EMP")]
SELECT tc.TABLE_NAME AS table_name,
       tc.CONSTRAINT_NAME AS constraint_name,
       CASE tc.CONSTRAINT_TYPE WHEN 'PRIMARY KEY' THEN 'P' WHEN 'UNIQUE' THEN 'U' ELSE 'F' END AS constraint_type,
       k.COLUMN_NAME AS column_name,
       k.ORDINAL_POSITION AS position,
       k.REFERENCED_TABLE_SCHEMA AS ref_schema,
       k.REFERENCED_TABLE_NAME AS ref_table,
       k.REFERENCED_COLUMN_NAME AS ref_column,
       rc.DELETE_RULE AS on_delete,
       rc.UPDATE_RULE AS on_update
FROM information_schema.TABLE_CONSTRAINTS tc
JOIN information_schema.KEY_COLUMN_USAGE k
  ON k.CONSTRAINT_SCHEMA = tc.CONSTRAINT_SCHEMA
 AND k.TABLE_NAME = tc.TABLE_NAME
 AND k.CONSTRAINT_NAME = tc.CONSTRAINT_NAME
LEFT JOIN information_schema.REFERENTIAL_CONSTRAINTS rc
  ON tc.CONSTRAINT_TYPE = 'FOREIGN KEY'
 AND rc.CONSTRAINT_SCHEMA = tc.CONSTRAINT_SCHEMA
 AND rc.TABLE_NAME = tc.TABLE_NAME
 AND rc.CONSTRAINT_NAME = tc.CONSTRAINT_NAME
WHERE tc.TABLE_SCHEMA = ?
  AND tc.CONSTRAINT_TYPE IN ('PRIMARY KEY', 'UNIQUE', 'FOREIGN KEY')
  AND (tc.CONSTRAINT_TYPE <> 'UNIQUE'
       OR NOT EXISTS (SELECT 1 FROM information_schema.STATISTICS s2
                       WHERE s2.TABLE_SCHEMA = tc.TABLE_SCHEMA AND s2.TABLE_NAME = tc.TABLE_NAME
                         AND s2.INDEX_NAME = tc.CONSTRAINT_NAME AND s2.EXPRESSION IS NOT NULL))
  AND tc.TABLE_NAME = ?
ORDER BY tc.TABLE_NAME, tc.CONSTRAINT_NAME, k.ORDINAL_POSITION

-- [checks] [String("HR"), String("EMP")]
-- CHECK constraints are kept only while tidb_enable_check_constraint is ON (7.2+; with it OFF they are
-- parsed and dropped). They are not in TABLE_CONSTRAINTS; TIDB_CHECK_CONSTRAINTS carries the table name.
SELECT c.TABLE_NAME AS table_name,
       c.CONSTRAINT_NAME AS constraint_name,
       c.CHECK_CLAUSE AS expression
FROM information_schema.TIDB_CHECK_CONSTRAINTS c
WHERE c.CONSTRAINT_SCHEMA = ?
  AND c.TABLE_NAME = ?
ORDER BY c.TABLE_NAME, c.CONSTRAINT_NAME

-- [indexes] [String("HR"), String("EMP")]
-- Non-unique indexes only: every unique index is already a UNIQUE constraint in `keys`.
-- Exception (8.0.13+): a unique index containing a functional part has no KEY_COLUMN_USAGE rows,
-- so it would vanish from `keys`; it is kept here instead.
SELECT s.TABLE_NAME AS table_name,
       s.INDEX_NAME AS index_name,
       CASE WHEN s.NON_UNIQUE = 0 THEN 1 ELSE 0 END AS is_unique,
       s.INDEX_TYPE AS method,
       COALESCE(s.COLUMN_NAME, s.EXPRESSION) AS column_expr,
       s.SEQ_IN_INDEX AS position,
       CASE WHEN s.COLLATION = 'D' THEN 1 ELSE 0 END AS descending
FROM information_schema.STATISTICS s
WHERE s.TABLE_SCHEMA = ?
  AND (s.NON_UNIQUE = 1
       OR EXISTS (SELECT 1 FROM information_schema.STATISTICS s2
                   WHERE s2.TABLE_SCHEMA = s.TABLE_SCHEMA AND s2.TABLE_NAME = s.TABLE_NAME
                     AND s2.INDEX_NAME = s.INDEX_NAME AND s2.EXPRESSION IS NOT NULL))
  AND s.TABLE_NAME = ?
ORDER BY s.TABLE_NAME, s.INDEX_NAME, s.SEQ_IN_INDEX

-- [triggers] [String("HR"), String("EMP")]
-- definition: a CREATE TRIGGER statement rebuilt from TRIGGERS (FOLLOWS/PRECEDES order and DEFINER are not included).
-- MySQL allows exactly one event per trigger, so events is that single event.
SELECT t.EVENT_OBJECT_TABLE AS table_name,
       t.TRIGGER_NAME AS name,
       t.ACTION_TIMING AS timing,
       t.EVENT_MANIPULATION AS events,
       CONCAT('CREATE TRIGGER `', REPLACE(t.TRIGGER_NAME, '`', '``'), '` ', t.ACTION_TIMING, ' ', t.EVENT_MANIPULATION,
              ' ON `', REPLACE(t.EVENT_OBJECT_TABLE, '`', '``'), '` FOR EACH ROW ', t.ACTION_STATEMENT) AS definition
FROM information_schema.TRIGGERS t
WHERE t.EVENT_OBJECT_SCHEMA = ?
  AND t.EVENT_OBJECT_TABLE = ?
ORDER BY t.EVENT_OBJECT_TABLE, t.TRIGGER_NAME

-- [view_definition] [String("HR"), String("EMP")]
SELECT v.VIEW_DEFINITION AS definition
FROM information_schema.VIEWS v
WHERE v.TABLE_SCHEMA = ?
  AND v.TABLE_NAME = ?

-- [routine_source] [String("HR"), String("EMP"), String("FUNCTION")]
-- SHOW CREATE cannot take bound params, so the CREATE statement is rebuilt from ROUTINES + PARAMETERS
-- (params, RETURNS, DETERMINISTIC, SQL SECURITY, COMMENT, body). One row, line = 1. Character-set clauses are not reproduced.
SELECT 1 AS line,
       CONCAT('CREATE ', r.ROUTINE_TYPE, ' `', REPLACE(r.ROUTINE_NAME, '`', '``'), '`(',
              COALESCE((SELECT GROUP_CONCAT(
                                 CONCAT(CASE WHEN r.ROUTINE_TYPE = 'PROCEDURE' THEN CONCAT(COALESCE(p.PARAMETER_MODE, 'IN'), ' ') ELSE '' END,
                                        '`', REPLACE(p.PARAMETER_NAME, '`', '``'), '` ', p.DTD_IDENTIFIER)
                                 ORDER BY p.ORDINAL_POSITION SEPARATOR ', ')
                          FROM information_schema.PARAMETERS p
                         WHERE p.SPECIFIC_SCHEMA = r.ROUTINE_SCHEMA AND p.SPECIFIC_NAME = r.SPECIFIC_NAME
                           AND p.ROUTINE_TYPE = r.ROUTINE_TYPE AND p.ORDINAL_POSITION > 0), ''),
              ')',
              CASE WHEN r.ROUTINE_TYPE = 'FUNCTION' THEN CONCAT(' RETURNS ', r.DTD_IDENTIFIER) ELSE '' END,
              CHAR(10), '    ', r.SQL_DATA_ACCESS,
              CASE WHEN r.IS_DETERMINISTIC = 'YES' THEN ' DETERMINISTIC' ELSE '' END,
              ' SQL SECURITY ', r.SECURITY_TYPE,
              CASE WHEN r.ROUTINE_COMMENT <> '' THEN CONCAT(' COMMENT ''', REPLACE(REPLACE(r.ROUTINE_COMMENT, '\\', '\\\\'), '''', ''''''), '''') ELSE '' END,
              CHAR(10), COALESCE(r.ROUTINE_DEFINITION, '')) AS text
FROM information_schema.ROUTINES r
WHERE r.ROUTINE_SCHEMA = ?
  AND r.ROUTINE_NAME = ?
  AND r.ROUTINE_TYPE = ?

-- [reserved_words] []
SELECT k.WORD AS word
FROM information_schema.KEYWORDS k
WHERE k.RESERVED = 1
ORDER BY k.WORD

-- [variant_probe] []
SHOW VARIABLES LIKE 'polardb_version'

-- [referencing_keys] [String("HR"), String("EMP")]
-- Table designer rebuild: foreign keys of OTHER tables that reference this table (dropped and recreated around the swap).
SELECT k.TABLE_SCHEMA AS table_schema,
       k.TABLE_NAME AS table_name,
       k.CONSTRAINT_NAME AS constraint_name,
       k.COLUMN_NAME AS column_name,
       k.ORDINAL_POSITION AS position,
       k.REFERENCED_COLUMN_NAME AS ref_column,
       rc.DELETE_RULE AS on_delete,
       rc.UPDATE_RULE AS on_update
FROM information_schema.KEY_COLUMN_USAGE k
JOIN information_schema.REFERENTIAL_CONSTRAINTS rc
  ON rc.CONSTRAINT_SCHEMA = k.CONSTRAINT_SCHEMA
 AND rc.TABLE_NAME = k.TABLE_NAME
 AND rc.CONSTRAINT_NAME = k.CONSTRAINT_NAME
WHERE k.REFERENCED_TABLE_SCHEMA = ?
  AND k.REFERENCED_TABLE_NAME = ?
  AND NOT (k.TABLE_SCHEMA = k.REFERENCED_TABLE_SCHEMA AND k.TABLE_NAME = k.REFERENCED_TABLE_NAME)
ORDER BY k.TABLE_SCHEMA, k.TABLE_NAME, k.CONSTRAINT_NAME, k.ORDINAL_POSITION

-- [table_storage] [String("HR"), String("EMP")]
-- As mysql.sql, plus whether the primary key is clustered (TIDB_PK_TYPE): a clustered primary key cannot
-- be dropped or changed in place, the table designer rebuilds the table instead.
SELECT t.ENGINE AS engine,
       (SELECT MAX(c.CHARACTER_SET_NAME) FROM information_schema.COLLATION_CHARACTER_SET_APPLICABILITY c
         WHERE c.COLLATION_NAME = t.TABLE_COLLATION) AS charset,
       t.TABLE_COLLATION AS collation,
       NULL AS tablespace,
       t.TABLE_ROWS AS row_count,
       CASE WHEN t.TIDB_PK_TYPE = 'CLUSTERED' THEN 1 ELSE 0 END AS clustered
FROM information_schema.TABLES t
WHERE t.TABLE_SCHEMA = ?
  AND t.TABLE_NAME = ?
```

- [ ] **Step 9: 修改 `crates/dblens-dialect/sql/kingbase.sql`**

```diff
--- a/crates/dblens-dialect/sql/kingbase.sql
+++ b/crates/dblens-dialect/sql/kingbase.sql
@@ -4,3 +4,99 @@
 -- Compatibility mode (oracle / pg / mysql / sqlserver) and the PostgreSQL version the build is based on.
 SELECT current_setting('database_mode') AS mode,
        current_setting('server_version_num') AS version_num
+
+-- [schemas]
+-- Built-in schemas besides pg_* and information_schema (sys, sys_*, dbms_*, ...); Oracle mode adds sys and wmsys.
+SELECT n.nspname AS name,
+       (n.nspname LIKE 'pg!_%' ESCAPE '!' OR n.nspname LIKE 'sys!_%' ESCAPE '!' OR n.nspname LIKE 'dbms!_%' ESCAPE '!'
+        OR n.nspname LIKE 'kdb!_%' ESCAPE '!'
+        OR n.nspname IN ('information_schema', 'sys', 'anon', 'perf', 'src_restrict', 'sysaudit', 'sysmac',
+                         'wmsys', 'xlog_record_read')) AS is_system
+FROM pg_catalog.pg_namespace n
+ORDER BY n.nspname
+
+-- [synonyms]
+-- Oracle-style synonyms (both compatibility modes have them); sys_synonym is a view over the catalog.
+SELECT s.synname AS name,
+       s.refobjnspname AS target_schema,
+       s.refobjname AS target_name,
+       NULL AS db_link
+FROM pg_catalog.sys_synonym s
+JOIN pg_catalog.pg_namespace n ON n.oid = s.synnamespace
+WHERE n.nspname = {{ bind("schema") }}
+{% if filter %}
+  AND UPPER(s.synname) LIKE UPPER({{ bind("filter") }}) ESCAPE '!'
+{% endif %}
+ORDER BY s.synname
+
+-- [routines]
+-- Functions and procedures as in postgres.sql, plus packages (pg_catalog._pkg; their routines are not in
+-- pg_proc). Every package is listed as PACKAGE and PACKAGE BODY.
+SELECT r.name, r.kind, r.comment
+FROM (
+  SELECT p.proname AS name,
+         CASE p.prokind WHEN 'p' THEN 'PROCEDURE' ELSE 'FUNCTION' END AS kind,
+         MAX(pg_catalog.obj_description(p.oid, 'pg_proc')) AS comment
+  FROM pg_catalog.pg_proc p
+  JOIN pg_catalog.pg_namespace n ON n.oid = p.pronamespace
+  WHERE n.nspname = {{ bind("schema") }}
+    AND p.prokind IN ('f', 'p')
+{% if filter %}
+    AND (UPPER(p.proname) LIKE UPPER({{ bind("filter") }}) ESCAPE '!'
+         OR UPPER(pg_catalog.obj_description(p.oid, 'pg_proc')) LIKE UPPER({{ bind("filter") }}) ESCAPE '!')
+{% endif %}
+  GROUP BY p.proname, 2
+  UNION ALL
+  SELECT k.pkgname, x.kind, NULL
+  FROM pg_catalog._pkg k
+  JOIN pg_catalog.pg_namespace n ON n.oid = k.pkgnamespace
+  CROSS JOIN (SELECT 'PACKAGE' AS kind UNION ALL SELECT 'PACKAGE BODY') x
+  WHERE n.nspname = {{ bind("schema") }}
+{% if filter %}
+    AND UPPER(k.pkgname) LIKE UPPER({{ bind("filter") }}) ESCAPE '!'
+{% endif %}
+) r
+ORDER BY r.name, r.kind
+
+-- [routine_source]
+-- As postgres.sql for functions and procedures; a package is rebuilt from the stored spec / body text
+-- (pkgspecsrc / pkgbodysrc hold what follows AS, without the final semicolon).
+SELECT r.line, r.text
+FROM (
+  SELECT (ROW_NUMBER() OVER (ORDER BY pg_catalog.pg_get_function_identity_arguments(p.oid), p.oid))::int AS line,
+         pg_catalog.pg_get_functiondef(p.oid) || E'\n' AS text
+  FROM pg_catalog.pg_proc p
+  JOIN pg_catalog.pg_namespace n ON n.oid = p.pronamespace
+  WHERE n.nspname = {{ bind("schema") }}
+    AND p.proname = {{ bind("name") }}
+    AND p.prokind IN ('f', 'p')
+{% if kind %}
+    AND p.prokind = CASE {{ bind("kind") }}::text WHEN 'PROCEDURE' THEN 'p' WHEN 'FUNCTION' THEN 'f' END
+{% endif %}
+  UNION ALL
+  SELECT 1, 'CREATE OR REPLACE PACKAGE ' || pg_catalog.quote_ident(n.nspname) || '.' || pg_catalog.quote_ident(k.pkgname)
+            || ' AS' || k.pkgspecsrc || ';' || E'\n'
+  FROM pg_catalog._pkg k
+  JOIN pg_catalog.pg_namespace n ON n.oid = k.pkgnamespace
+  WHERE n.nspname = {{ bind("schema") }}
+    AND k.pkgname = {{ bind("name") }}
+{% if kind %}
+    AND {{ bind("kind") }}::text = 'PACKAGE'
+{% else %}
+    AND 1 = 0
+{% endif %}
+  UNION ALL
+  SELECT 1, 'CREATE OR REPLACE PACKAGE BODY ' || pg_catalog.quote_ident(n.nspname) || '.' || pg_catalog.quote_ident(k.pkgname)
+            || ' AS' || k.pkgbodysrc || ';' || E'\n'
+  FROM pg_catalog._pkg k
+  JOIN pg_catalog.pg_namespace n ON n.oid = k.pkgnamespace
+  WHERE n.nspname = {{ bind("schema") }}
+    AND k.pkgname = {{ bind("name") }}
+    AND k.pkgbodysrc IS NOT NULL
+{% if kind %}
+    AND {{ bind("kind") }}::text = 'PACKAGE BODY'
+{% else %}
+    AND 1 = 0
+{% endif %}
+) r
+ORDER BY r.line
```

- [ ] **Step 10: 新建 `crates/dblens-dialect/sql/oceanbase.sql`**

```sql
-- OceanBase (MySQL mode) overrides on top of mysql.sql (only keys that differ). Tested on 4.4.2.1;
-- VERSION() reports 5.7.25, but CHECK constraints are supported and listed like MySQL 8.0.16+.
-- reserved_words is empty (removed): there is no information_schema.KEYWORDS; ident.rs has the built-in list.

-- [checks]
SELECT tc.TABLE_NAME AS table_name,
       tc.CONSTRAINT_NAME AS constraint_name,
       cc.CHECK_CLAUSE AS expression
FROM information_schema.TABLE_CONSTRAINTS tc
JOIN information_schema.CHECK_CONSTRAINTS cc
  ON cc.CONSTRAINT_SCHEMA = tc.CONSTRAINT_SCHEMA
 AND cc.CONSTRAINT_NAME = tc.CONSTRAINT_NAME
WHERE tc.TABLE_SCHEMA = {{ bind("schema") }}
  AND tc.CONSTRAINT_TYPE = 'CHECK'
{% if table %}
  AND tc.TABLE_NAME = {{ bind("table") }}
{% endif %}
ORDER BY tc.TABLE_NAME, tc.CONSTRAINT_NAME

-- [indexes]
-- Non-unique indexes only (as in mysql.sql). A FULLTEXT index lists only hidden columns
-- (__doc_id_*, __word_segment_*), so with those left out it does not appear. Index columns are always
-- stored ascending (DESC is accepted and ignored).
SELECT s.TABLE_NAME AS table_name,
       s.INDEX_NAME AS index_name,
       0 AS is_unique,
       s.INDEX_TYPE AS method,
       s.COLUMN_NAME AS column_expr,
       s.SEQ_IN_INDEX AS position,
       0 AS descending
FROM information_schema.STATISTICS s
WHERE s.TABLE_SCHEMA = {{ bind("schema") }}
  AND s.NON_UNIQUE = 1
  AND s.COLUMN_NAME NOT LIKE '!_!_%' ESCAPE '!'
{% if table %}
  AND s.TABLE_NAME = {{ bind("table") }}
{% endif %}
ORDER BY s.TABLE_NAME, s.INDEX_NAME, s.SEQ_IN_INDEX

-- [reserved_words]

-- [tables]
-- Partition from a join: CREATE_OPTIONS is NULL even for partitioned tables.
SELECT t.TABLE_NAME AS name,
       NULLIF(t.TABLE_COMMENT, '') AS comment,
       CASE WHEN p.PARTITION_METHOD IS NOT NULL
            THEN CONCAT(p.PARTITION_METHOD, ' (', COALESCE(p.PARTITION_EXPRESSION, ''), ')')
       END AS `partition`
FROM information_schema.TABLES t
LEFT JOIN information_schema.PARTITIONS p
  ON p.TABLE_SCHEMA = t.TABLE_SCHEMA AND p.TABLE_NAME = t.TABLE_NAME AND p.PARTITION_ORDINAL_POSITION = 1
WHERE t.TABLE_SCHEMA = {{ bind("schema") }}
  AND t.TABLE_TYPE = 'BASE TABLE'
{% if table %}
  AND t.TABLE_NAME = {{ bind("table") }}
{% endif %}
{% if filter %}
  AND (UPPER(t.TABLE_NAME) LIKE UPPER({{ bind("filter") }}) ESCAPE '!'
       OR UPPER(COALESCE(t.TABLE_COMMENT, '')) LIKE UPPER({{ bind("filter") }}) ESCAPE '!')
{% endif %}
ORDER BY t.TABLE_NAME
```

- [ ] **Step 11: 修改 `crates/dblens-dialect/sql/opengauss.sql`**

```diff
--- a/crates/dblens-dialect/sql/opengauss.sql
+++ b/crates/dblens-dialect/sql/opengauss.sql
@@ -1,5 +1,7 @@
 -- openGauss overrides on top of postgres.sql (only keys that differ). Based on PG 9.2; tested on 7.0.0-RC1
--- with databases in A (Oracle-compatible) and PG compatibility.
+-- with databases in A (Oracle-compatible) and PG compatibility. Compared with postgres.sql: no LATERAL /
+-- WITH ORDINALITY, no relispartition (partitions are in pg_partition), no pg_sequence, pg_get_partkeydef,
+-- attidentity or attgenerated.
 
 -- [variant_probe]
 -- Compatibility of the current database (A / B / C / PG) and the PostgreSQL version the build is based on.
@@ -7,3 +9,309 @@ SELECT datcompatibility AS mode,
        current_setting('server_version_num') AS version_num
 FROM pg_catalog.pg_database
 WHERE datname = current_database()
+
+-- [schemas]
+-- Built-in schemas besides pg_* and information_schema (dbe_*, pkg_*, db4ai, ...).
+SELECT n.nspname AS name,
+       (n.nspname LIKE 'pg!_%' ESCAPE '!' OR n.nspname LIKE 'dbe!_%' ESCAPE '!' OR n.nspname LIKE 'pkg!_%' ESCAPE '!'
+        OR n.nspname IN ('information_schema', 'blockchain', 'coverage', 'cstore', 'db4ai', 'snapshot',
+                         'sqladvisor', 'xmltype')) AS is_system
+FROM pg_catalog.pg_namespace n
+ORDER BY n.nspname
+
+-- [tables]
+-- Partitions live in pg_partition, not pg_class: a partitioned table is relkind r with parttype p.
+SELECT c.relname AS name,
+       pg_catalog.obj_description(c.oid, 'pg_class') AS comment,
+       CASE WHEN c.parttype = 'p' THEN
+         (SELECT CASE p.partstrategy WHEN 'r' THEN 'RANGE' WHEN 'l' THEN 'LIST' WHEN 'h' THEN 'HASH'
+                                     WHEN 'i' THEN 'INTERVAL' ELSE 'UNKNOWN' END
+                 || ' (' || (SELECT pg_catalog.string_agg(a.attname, ', ' ORDER BY a.attnum)
+                               FROM pg_catalog.pg_attribute a
+                              WHERE a.attrelid = c.oid AND a.attnum = ANY (p.partkey::int2[])) || ')'
+            FROM pg_catalog.pg_partition p
+           WHERE p.parentid = c.oid AND p.parttype = 'r')
+       END AS partition
+FROM pg_catalog.pg_class c
+JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace
+WHERE n.nspname = {{ bind("schema") }}
+  AND c.relkind IN ('r', 'p')
+  AND c.relpersistence <> 't'
+{% if table %}
+  AND c.relname = {{ bind("table") }}
+{% endif %}
+{% if filter %}
+  AND (UPPER(c.relname) LIKE UPPER({{ bind("filter") }}) ESCAPE '!'
+       OR UPPER(pg_catalog.obj_description(c.oid, 'pg_class')) LIKE UPPER({{ bind("filter") }}) ESCAPE '!')
+{% endif %}
+ORDER BY c.relname
+
+-- [sequences]
+-- No pg_sequence: parameters come from pg_sequence_parameters, which has no cache size.
+SELECT c.relname AS name,
+       (pg_catalog.pg_sequence_parameters(c.oid)).start_value AS start_value,
+       (pg_catalog.pg_sequence_parameters(c.oid)).increment AS increment,
+       (pg_catalog.pg_sequence_parameters(c.oid)).minimum_value AS min_value,
+       (pg_catalog.pg_sequence_parameters(c.oid)).maximum_value AS max_value,
+       (pg_catalog.pg_sequence_parameters(c.oid)).cycle_option AS cycle,
+       NULL::bigint AS cache
+FROM pg_catalog.pg_class c
+JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace
+WHERE n.nspname = {{ bind("schema") }}
+  AND c.relkind = 'S'
+  AND NOT EXISTS (SELECT 1 FROM pg_catalog.pg_depend d
+                  WHERE d.classid = 'pg_catalog.pg_class'::regclass AND d.objid = c.oid
+                    AND d.refclassid = 'pg_catalog.pg_class'::regclass AND d.deptype IN ('a', 'i'))
+{% if filter %}
+  AND (UPPER(c.relname) LIKE UPPER({{ bind("filter") }}) ESCAPE '!'
+       OR UPPER(pg_catalog.obj_description(c.oid, 'pg_class')) LIKE UPPER({{ bind("filter") }}) ESCAPE '!')
+{% endif %}
+ORDER BY c.relname
+
+-- [columns]
+-- No identity columns (serial only); a generated column has pg_attrdef.adgencol = 's'.
+SELECT c.relname AS table_name,
+       a.attname AS name,
+       (ROW_NUMBER() OVER (PARTITION BY c.oid ORDER BY a.attnum))::int AS ordinal,
+       pg_catalog.format_type(a.atttypid, a.atttypmod) AS type_text,
+       NOT a.attnotnull AS nullable,
+       CASE WHEN d.adgencol = 's' THEN NULL ELSE pg_catalog.pg_get_expr(d.adbin, d.adrelid) END AS default_value,
+       pg_catalog.col_description(c.oid, a.attnum) AS comment,
+       ((d.adbin IS NOT NULL AND pg_catalog.pg_get_expr(d.adbin, d.adrelid) LIKE 'nextval(%'
+            AND EXISTS (SELECT 1 FROM pg_catalog.pg_depend dp
+                        WHERE dp.refclassid = 'pg_catalog.pg_class'::regclass AND dp.refobjid = c.oid
+                          AND dp.refobjsubid = a.attnum AND dp.deptype = 'a'
+                          AND dp.classid = 'pg_catalog.pg_class'::regclass))) AS auto_increment,
+       CASE WHEN d.adgencol = 's' THEN pg_catalog.pg_get_expr(d.adbin, d.adrelid) END AS generated
+FROM pg_catalog.pg_class c
+JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace
+JOIN pg_catalog.pg_attribute a ON a.attrelid = c.oid AND a.attnum > 0 AND NOT a.attisdropped
+LEFT JOIN pg_catalog.pg_attrdef d ON d.adrelid = a.attrelid AND d.adnum = a.attnum
+WHERE n.nspname = {{ bind("schema") }}
+  AND c.relkind IN ('r', 'p', 'v', 'm')
+  AND c.relpersistence <> 't'
+{% if table %}
+  AND c.relname = {{ bind("table") }}
+{% endif %}
+ORDER BY c.relname, a.attnum
+
+-- [keys]
+-- No LATERAL / WITH ORDINALITY: key positions come from generate_series in a subquery.
+SELECT c.relname AS table_name,
+       con.conname AS constraint_name,
+       CASE con.contype WHEN 'p' THEN 'P' WHEN 'u' THEN 'U' ELSE 'F' END AS constraint_type,
+       a.attname AS column_name,
+       con.pos::int AS position,
+       CASE WHEN con.contype = 'f' THEN rn.nspname END AS ref_schema,
+       CASE WHEN con.contype = 'f' THEN rc.relname END AS ref_table,
+       ra.attname AS ref_column,
+       CASE WHEN con.contype = 'f' THEN
+         CASE con.confdeltype WHEN 'c' THEN 'CASCADE' WHEN 'n' THEN 'SET NULL' WHEN 'd' THEN 'SET DEFAULT'
+                              WHEN 'r' THEN 'RESTRICT' ELSE 'NO ACTION' END END AS on_delete,
+       CASE WHEN con.contype = 'f' THEN
+         CASE con.confupdtype WHEN 'c' THEN 'CASCADE' WHEN 'n' THEN 'SET NULL' WHEN 'd' THEN 'SET DEFAULT'
+                              WHEN 'r' THEN 'RESTRICT' ELSE 'NO ACTION' END END AS on_update
+FROM (SELECT x.*, pg_catalog.generate_series(1, pg_catalog.array_length(x.conkey, 1)) AS pos
+        FROM pg_catalog.pg_constraint x) con
+JOIN pg_catalog.pg_class c ON c.oid = con.conrelid
+JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace
+JOIN pg_catalog.pg_attribute a ON a.attrelid = con.conrelid AND a.attnum = con.conkey[con.pos]
+LEFT JOIN pg_catalog.pg_class rc ON rc.oid = con.confrelid
+LEFT JOIN pg_catalog.pg_namespace rn ON rn.oid = rc.relnamespace
+LEFT JOIN pg_catalog.pg_attribute ra ON ra.attrelid = con.confrelid AND ra.attnum = con.confkey[con.pos]
+WHERE n.nspname = {{ bind("schema") }}
+  AND con.contype IN ('p', 'u', 'f')
+  AND c.relkind IN ('r', 'p')
+  AND c.relpersistence <> 't'
+{% if table %}
+  AND c.relname = {{ bind("table") }}
+{% endif %}
+ORDER BY c.relname, con.conname, con.pos
+
+-- [checks]
+SELECT c.relname AS table_name,
+       con.conname AS constraint_name,
+       pg_catalog.pg_get_expr(con.conbin, con.conrelid) AS expression
+FROM pg_catalog.pg_constraint con
+JOIN pg_catalog.pg_class c ON c.oid = con.conrelid
+JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace
+WHERE n.nspname = {{ bind("schema") }}
+  AND con.contype = 'c'
+  AND c.relkind IN ('r', 'p')
+  AND c.relpersistence <> 't'
+{% if table %}
+  AND c.relname = {{ bind("table") }}
+{% endif %}
+ORDER BY c.relname, con.conname
+
+-- [indexes]
+-- No LATERAL / WITH ORDINALITY: key positions come from generate_series over indnkeyatts.
+SELECT c.relname AS table_name,
+       ic.relname AS index_name,
+       i.indisunique AS is_unique,
+       UPPER(am.amname) AS method,
+       CASE WHEN i.indkey[i.pos - 1] = 0 THEN pg_catalog.pg_get_indexdef(i.indexrelid, i.pos::int, true) ELSE a.attname END AS column_expr,
+       i.pos::int AS position,
+       ((i.indoption[i.pos - 1] & 1) = 1) AS descending
+FROM (SELECT x.*, pg_catalog.generate_series(1, x.indnkeyatts) AS pos FROM pg_catalog.pg_index x) i
+JOIN pg_catalog.pg_class c ON c.oid = i.indrelid
+JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace
+JOIN pg_catalog.pg_class ic ON ic.oid = i.indexrelid
+JOIN pg_catalog.pg_am am ON am.oid = ic.relam
+LEFT JOIN pg_catalog.pg_attribute a ON a.attrelid = i.indrelid AND a.attnum = i.indkey[i.pos - 1] AND i.indkey[i.pos - 1] > 0
+WHERE n.nspname = {{ bind("schema") }}
+  AND c.relkind IN ('r', 'p', 'm')
+  AND c.relpersistence <> 't'
+  AND NOT EXISTS (SELECT 1 FROM pg_catalog.pg_constraint cc
+                  WHERE cc.conindid = i.indexrelid AND cc.contype IN ('p', 'u') AND cc.conrelid = i.indrelid)
+{% if table %}
+  AND c.relname = {{ bind("table") }}
+{% endif %}
+ORDER BY c.relname, ic.relname, i.pos
+
+-- [triggers]
+SELECT c.relname AS table_name,
+       t.tgname AS name,
+       CASE WHEN (t.tgtype & 64) <> 0 THEN 'INSTEAD OF' WHEN (t.tgtype & 2) <> 0 THEN 'BEFORE' ELSE 'AFTER' END AS timing,
+       concat_ws(',',
+         CASE WHEN (t.tgtype & 4) <> 0 THEN 'INSERT' END,
+         CASE WHEN (t.tgtype & 16) <> 0 THEN 'UPDATE' END,
+         CASE WHEN (t.tgtype & 8) <> 0 THEN 'DELETE' END,
+         CASE WHEN (t.tgtype & 32) <> 0 THEN 'TRUNCATE' END) AS events,
+       pg_catalog.pg_get_triggerdef(t.oid, true) AS definition
+FROM pg_catalog.pg_trigger t
+JOIN pg_catalog.pg_class c ON c.oid = t.tgrelid
+JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace
+WHERE n.nspname = {{ bind("schema") }}
+  AND NOT t.tgisinternal
+  AND c.relkind IN ('r', 'p', 'v')
+  AND c.relpersistence <> 't'
+{% if table %}
+  AND c.relname = {{ bind("table") }}
+{% endif %}
+ORDER BY c.relname, t.tgname
+
+-- [referencing_keys]
+SELECT n.nspname AS table_schema,
+       c.relname AS table_name,
+       con.conname AS constraint_name,
+       a.attname AS column_name,
+       con.pos::int AS position,
+       ra.attname AS ref_column,
+       CASE con.confdeltype WHEN 'c' THEN 'CASCADE' WHEN 'n' THEN 'SET NULL' WHEN 'd' THEN 'SET DEFAULT'
+                         WHEN 'r' THEN 'RESTRICT' ELSE 'NO ACTION' END AS on_delete,
+       CASE con.confupdtype WHEN 'c' THEN 'CASCADE' WHEN 'n' THEN 'SET NULL' WHEN 'd' THEN 'SET DEFAULT'
+                         WHEN 'r' THEN 'RESTRICT' ELSE 'NO ACTION' END AS on_update
+FROM (SELECT x.*, pg_catalog.generate_series(1, pg_catalog.array_length(x.conkey, 1)) AS pos
+        FROM pg_catalog.pg_constraint x) con
+JOIN pg_catalog.pg_class c ON c.oid = con.conrelid
+JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace
+JOIN pg_catalog.pg_class t ON t.oid = con.confrelid
+JOIN pg_catalog.pg_namespace tn ON tn.oid = t.relnamespace
+JOIN pg_catalog.pg_attribute a ON a.attrelid = con.conrelid AND a.attnum = con.conkey[con.pos]
+JOIN pg_catalog.pg_attribute ra ON ra.attrelid = con.confrelid AND ra.attnum = con.confkey[con.pos]
+WHERE con.contype = 'f'
+  AND tn.nspname = {{ bind("schema") }}
+  AND t.relname = {{ bind("table") }}
+  AND con.conrelid <> con.confrelid
+ORDER BY n.nspname, c.relname, con.conname, con.pos
+
+-- [grants]
+-- No LATERAL: aclexplode runs in a subquery.
+SELECT CASE WHEN (c.acl).grantee = 0 THEN 'PUBLIC' ELSE pg_catalog.pg_get_userbyid((c.acl).grantee) END AS grantee,
+       (c.acl).privilege_type AS privilege,
+       (c.acl).is_grantable AS grantable
+FROM (SELECT x.relname, x.relnamespace, x.relowner, pg_catalog.aclexplode(x.relacl) AS acl
+        FROM pg_catalog.pg_class x) c
+JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace
+WHERE n.nspname = {{ bind("schema") }}
+  AND c.relname = {{ bind("table") }}
+  AND (c.acl).grantee <> c.relowner
+ORDER BY 1, 2
+
+-- [synonyms]
+SELECT s.synname AS name,
+       s.synobjschema AS target_schema,
+       s.synobjname AS target_name,
+       NULL AS db_link
+FROM pg_catalog.pg_synonym s
+JOIN pg_catalog.pg_namespace n ON n.oid = s.synnamespace
+WHERE n.nspname = {{ bind("schema") }}
+{% if filter %}
+  AND UPPER(s.synname) LIKE UPPER({{ bind("filter") }}) ESCAPE '!'
+{% endif %}
+ORDER BY s.synname
+
+-- [routines]
+-- Functions and procedures (aggregates are prokind f too, proisagg filters them; routines of packages have
+-- propackageid set), plus packages from gs_package (A compatibility only). Every package is listed as
+-- PACKAGE and PACKAGE BODY.
+SELECT r.name, r.kind, r.comment
+FROM (
+  SELECT p.proname AS name,
+         CASE p.prokind WHEN 'p' THEN 'PROCEDURE' ELSE 'FUNCTION' END AS kind,
+         MAX(pg_catalog.obj_description(p.oid, 'pg_proc')) AS comment
+  FROM pg_catalog.pg_proc p
+  JOIN pg_catalog.pg_namespace n ON n.oid = p.pronamespace
+  WHERE n.nspname = {{ bind("schema") }}
+    AND p.prokind IN ('f', 'p') AND NOT p.proisagg AND NOT p.proiswindow AND p.propackageid = 0
+{% if filter %}
+    AND (UPPER(p.proname) LIKE UPPER({{ bind("filter") }}) ESCAPE '!'
+         OR UPPER(pg_catalog.obj_description(p.oid, 'pg_proc')) LIKE UPPER({{ bind("filter") }}) ESCAPE '!')
+{% endif %}
+  GROUP BY p.proname, 2
+  UNION ALL
+  SELECT k.pkgname, x.kind, NULL
+  FROM pg_catalog.gs_package k
+  JOIN pg_catalog.pg_namespace n ON n.oid = k.pkgnamespace
+  CROSS JOIN (SELECT 'PACKAGE' AS kind UNION ALL SELECT 'PACKAGE BODY') x
+  WHERE n.nspname = {{ bind("schema") }}
+{% if filter %}
+    AND UPPER(k.pkgname) LIKE UPPER({{ bind("filter") }}) ESCAPE '!'
+{% endif %}
+) r
+ORDER BY r.name, r.kind
+
+-- [routine_source]
+-- A package is rebuilt from gs_package: pkgspecsrc / pkgbodydeclsrc start with "PACKAGE DECLARE", which is
+-- dropped, and end without the final semicolon. (substring, not regexp_replace with '': in A compatibility
+-- '' is NULL.)
+SELECT r.line, r.text
+FROM (
+  SELECT (ROW_NUMBER() OVER (ORDER BY pg_catalog.pg_get_function_identity_arguments(p.oid), p.oid))::int AS line,
+         pg_catalog.pg_get_functiondef(p.oid) || E'\n' AS text
+  FROM pg_catalog.pg_proc p
+  JOIN pg_catalog.pg_namespace n ON n.oid = p.pronamespace
+  WHERE n.nspname = {{ bind("schema") }}
+    AND p.proname = {{ bind("name") }}
+    AND p.prokind IN ('f', 'p') AND NOT p.proisagg AND NOT p.proiswindow AND p.propackageid = 0
+{% if kind %}
+    AND p.prokind = CASE {{ bind("kind") }}::text WHEN 'PROCEDURE' THEN 'p' WHEN 'FUNCTION' THEN 'f' END
+{% endif %}
+  UNION ALL
+  SELECT 1, 'CREATE OR REPLACE PACKAGE ' || pg_catalog.quote_ident(n.nspname) || '.' || pg_catalog.quote_ident(k.pkgname)
+            || ' AS' || substring(k.pkgspecsrc FROM '^\s*PACKAGE\s+DECLARE(.*)$') || ';' || E'\n'
+  FROM pg_catalog.gs_package k
+  JOIN pg_catalog.pg_namespace n ON n.oid = k.pkgnamespace
+  WHERE n.nspname = {{ bind("schema") }}
+    AND k.pkgname = {{ bind("name") }}
+{% if kind %}
+    AND {{ bind("kind") }}::text = 'PACKAGE'
+{% else %}
+    AND 1 = 0
+{% endif %}
+  UNION ALL
+  SELECT 1, 'CREATE OR REPLACE PACKAGE BODY ' || pg_catalog.quote_ident(n.nspname) || '.' || pg_catalog.quote_ident(k.pkgname)
+            || ' AS' || substring(k.pkgbodydeclsrc FROM '^\s*PACKAGE\s+DECLARE(.*)$') || ';' || E'\n'
+  FROM pg_catalog.gs_package k
+  JOIN pg_catalog.pg_namespace n ON n.oid = k.pkgnamespace
+  WHERE n.nspname = {{ bind("schema") }}
+    AND k.pkgname = {{ bind("name") }}
+    AND substring(k.pkgbodydeclsrc FROM '^\s*PACKAGE\s+DECLARE(.*)$') IS NOT NULL
+{% if kind %}
+    AND {{ bind("kind") }}::text = 'PACKAGE BODY'
+{% else %}
+    AND 1 = 0
+{% endif %}
+) r
+ORDER BY r.line
```

- [ ] **Step 12: 修改 `crates/dblens-dialect/sql/postgres.sql`**

```diff
--- a/crates/dblens-dialect/sql/postgres.sql
+++ b/crates/dblens-dialect/sql/postgres.sql
@@ -100,25 +100,25 @@ GROUP BY p.proname, 2
 ORDER BY p.proname, 2
 
 -- [columns]
+-- No comparisons with '' anywhere: in the Oracle compatibility mode of KingbaseES / openGauss '' is NULL.
 SELECT c.relname AS table_name,
        a.attname AS name,
        (ROW_NUMBER() OVER (PARTITION BY c.oid ORDER BY a.attnum))::int AS ordinal,
        pg_catalog.format_type(a.atttypid, a.atttypmod) AS type_text,
        NOT a.attnotnull AS nullable,
 {% if version >= 120000 %}
-       CASE WHEN a.attgenerated = '' THEN pg_catalog.pg_get_expr(d.adbin, d.adrelid) END AS default_value,
+       CASE WHEN a.attgenerated <> 's' THEN pg_catalog.pg_get_expr(d.adbin, d.adrelid) END AS default_value,
 {% else %}
        pg_catalog.pg_get_expr(d.adbin, d.adrelid) AS default_value,
 {% endif %}
        pg_catalog.col_description(c.oid, a.attnum) AS comment,
-       (a.attidentity <> ''
-        OR (d.adbin IS NOT NULL AND pg_catalog.pg_get_expr(d.adbin, d.adrelid) LIKE 'nextval(%'
+       ({% if version >= 100000 %}a.attidentity IN ('a', 'd') OR {% endif %}(d.adbin IS NOT NULL AND pg_catalog.pg_get_expr(d.adbin, d.adrelid) LIKE 'nextval(%'
             AND EXISTS (SELECT 1 FROM pg_catalog.pg_depend dp
                         WHERE dp.refclassid = 'pg_catalog.pg_class'::regclass AND dp.refobjid = c.oid
                           AND dp.refobjsubid = a.attnum AND dp.deptype = 'a'
                           AND dp.classid = 'pg_catalog.pg_class'::regclass))) AS auto_increment,
 {% if version >= 120000 %}
-       CASE WHEN a.attgenerated <> '' THEN pg_catalog.pg_get_expr(d.adbin, d.adrelid) END AS generated
+       CASE WHEN a.attgenerated = 's' THEN pg_catalog.pg_get_expr(d.adbin, d.adrelid) END AS generated
 {% else %}
        NULL::text AS generated
 {% endif %}
```

- [ ] **Step 13: 新建 `crates/dblens-dialect/sql/tidb.sql`**

```sql
-- TiDB overrides on top of mysql.sql (only keys that differ). Tested on v8.5.8; VERSION() reports 8.0.11.

-- [checks]
-- CHECK constraints are kept only while tidb_enable_check_constraint is ON (7.2+; with it OFF they are
-- parsed and dropped). They are not in TABLE_CONSTRAINTS; TIDB_CHECK_CONSTRAINTS carries the table name.
SELECT c.TABLE_NAME AS table_name,
       c.CONSTRAINT_NAME AS constraint_name,
       c.CHECK_CLAUSE AS expression
FROM information_schema.TIDB_CHECK_CONSTRAINTS c
WHERE c.CONSTRAINT_SCHEMA = {{ bind("schema") }}
{% if table %}
  AND c.TABLE_NAME = {{ bind("table") }}
{% endif %}
ORDER BY c.TABLE_NAME, c.CONSTRAINT_NAME

-- [tables]
-- Partition from a join: a correlated subquery over information_schema returns NULL for every row once
-- more than one table matches (TiDB 8.5).
SELECT t.TABLE_NAME AS name,
       NULLIF(t.TABLE_COMMENT, '') AS comment,
       CASE WHEN p.PARTITION_METHOD IS NOT NULL
            THEN CONCAT(p.PARTITION_METHOD, ' (', COALESCE(p.PARTITION_EXPRESSION, ''), ')')
       END AS `partition`
FROM information_schema.TABLES t
LEFT JOIN information_schema.PARTITIONS p
  ON p.TABLE_SCHEMA = t.TABLE_SCHEMA AND p.TABLE_NAME = t.TABLE_NAME AND p.PARTITION_ORDINAL_POSITION = 1
WHERE t.TABLE_SCHEMA = {{ bind("schema") }}
  AND t.TABLE_TYPE = 'BASE TABLE'
{% if table %}
  AND t.TABLE_NAME = {{ bind("table") }}
{% endif %}
{% if filter %}
  AND (UPPER(t.TABLE_NAME) LIKE UPPER({{ bind("filter") }}) ESCAPE '!'
       OR UPPER(COALESCE(t.TABLE_COMMENT, '')) LIKE UPPER({{ bind("filter") }}) ESCAPE '!')
{% endif %}
ORDER BY t.TABLE_NAME

-- [sequences]
-- TiDB supports CREATE SEQUENCE (MySQL has no sequences); CACHE is 1/0 and the cache size is CACHE_VALUE.
SELECT s.SEQUENCE_NAME AS name,
       s.START AS start_value,
       s.INCREMENT AS increment,
       s.MIN_VALUE AS min_value,
       s.MAX_VALUE AS max_value,
       s.CYCLE AS cycle,
       CASE WHEN s.CACHE = 1 THEN s.CACHE_VALUE END AS cache
FROM information_schema.SEQUENCES s
WHERE s.SEQUENCE_SCHEMA = {{ bind("schema") }}
{% if filter %}
  AND UPPER(s.SEQUENCE_NAME) LIKE UPPER({{ bind("filter") }}) ESCAPE '!'
{% endif %}
ORDER BY s.SEQUENCE_NAME

-- [columns]
-- As mysql.sql, plus AUTO_RANDOM: it is not in EXTRA; the table's TIDB_ROW_ID_SHARDING_INFO is
-- PK_AUTO_RANDOM_BITS=n and the column is its (single-column, clustered) primary key.
SELECT c.TABLE_NAME AS table_name,
       c.COLUMN_NAME AS name,
       c.ORDINAL_POSITION AS ordinal,
       c.COLUMN_TYPE AS type_text,
       CASE WHEN c.IS_NULLABLE = 'YES' THEN 1 ELSE 0 END AS nullable,
       CASE
         WHEN c.COLUMN_DEFAULT IS NULL THEN NULL
         WHEN c.GENERATION_EXPRESSION <> '' THEN NULL
         WHEN c.EXTRA LIKE '%DEFAULT\_GENERATED%' THEN c.COLUMN_DEFAULT
         WHEN c.DATA_TYPE IN ('tinyint', 'smallint', 'mediumint', 'int', 'bigint', 'decimal', 'float', 'double', 'year', 'bit') THEN c.COLUMN_DEFAULT
         ELSE CONCAT('''', REPLACE(REPLACE(c.COLUMN_DEFAULT, '\\', '\\\\'), '''', ''''''), '''')
       END AS default_value,
       NULLIF(c.COLUMN_COMMENT, '') AS comment,
       CASE WHEN c.EXTRA LIKE '%auto\_increment%'
              OR (c.COLUMN_KEY = 'PRI' AND t.TIDB_ROW_ID_SHARDING_INFO LIKE 'PK!_AUTO!_RANDOM!_BITS%' ESCAPE '!')
            THEN 1 ELSE 0 END AS auto_increment,
       NULLIF(c.GENERATION_EXPRESSION, '') AS `generated`
FROM information_schema.COLUMNS c
JOIN information_schema.TABLES t ON t.TABLE_SCHEMA = c.TABLE_SCHEMA AND t.TABLE_NAME = c.TABLE_NAME
WHERE c.TABLE_SCHEMA = {{ bind("schema") }}
{% if table %}
  AND c.TABLE_NAME = {{ bind("table") }}
{% endif %}
ORDER BY c.TABLE_NAME, c.ORDINAL_POSITION

-- [table_storage]
-- As mysql.sql, plus whether the primary key is clustered (TIDB_PK_TYPE): a clustered primary key cannot
-- be dropped or changed in place, the table designer rebuilds the table instead.
SELECT t.ENGINE AS engine,
       (SELECT MAX(c.CHARACTER_SET_NAME) FROM information_schema.COLLATION_CHARACTER_SET_APPLICABILITY c
         WHERE c.COLLATION_NAME = t.TABLE_COLLATION) AS charset,
       t.TABLE_COLLATION AS collation,
       NULL AS tablespace,
       t.TABLE_ROWS AS row_count,
       CASE WHEN t.TIDB_PK_TYPE = 'CLUSTERED' THEN 1 ELSE 0 END AS clustered
FROM information_schema.TABLES t
WHERE t.TABLE_SCHEMA = {{ bind("schema") }}
  AND t.TABLE_NAME = {{ bind("table") }}
```

- [ ] **Step 14: 修改 `crates/dblens-dialect/src/design/rebuild.rs`**

```diff
--- a/crates/dblens-dialect/src/design/rebuild.rs
+++ b/crates/dblens-dialect/src/design/rebuild.rs
@@ -375,6 +375,7 @@ mod tests {
                 collation: Some("utf8mb4_0900_ai_ci".into()),
                 tablespace: None,
                 row_count: Some(10),
+                clustered: None,
             },
             triggers: vec![trigger.into()],
         }
```

- [ ] **Step 15: 修改 `crates/dblens-dialect/src/ident.rs`**

```diff
--- a/crates/dblens-dialect/src/ident.rs
+++ b/crates/dblens-dialect/src/ident.rs
@@ -55,7 +55,7 @@ fn is_reserved(family: Family, upper: &str) -> bool {
         || (family == Family::Dameng && DAMENG_RESERVED.binary_search(&upper).is_ok())
 }
 
-/// PostgreSQL、MySQL（8.0/8.4）、Oracle、达梦保留字与特殊值函数名（如 CURRENT_USER、SYSDATE）的并集，按字节序排列。
+/// PostgreSQL、MySQL（8.0/8.4）、Oracle、达梦以及国产库第一批（TiDB 等）的保留字与特殊值函数名（如 CURRENT_USER、SYSDATE）的并集，按字节序排列。
 /// 多加引号不会出错，所以宁多勿少；个别词只在部分方言中保留（如 PUBLIC），由 `is_reserved` 按家族豁免。新增方言时要补充。
 const RESERVED: &[&str] = &[
     "ACCESS",
@@ -73,6 +73,7 @@ const RESERVED: &[&str] = &[
     "ASENSITIVE",
     "ASYMMETRIC",
     "AUDIT",
+    "AUTHID",
     "AUTHORIZATION",
     "BEFORE",
     "BETWEEN",
@@ -81,6 +82,7 @@ const RESERVED: &[&str] = &[
     "BLOB",
     "BOTH",
     "BREAK",
+    "BUCKETS",
     "BY",
     "CALL",
     "CASCADE",
@@ -97,15 +99,18 @@ const RESERVED: &[&str] = &[
     "COLUMN",
     "COLUMN_VALUE",
     "COMMENT",
+    "COMPACT",
     "COMPRESS",
     "CONCURRENTLY",
     "CONDITION",
     "CONNECT",
+    "CONNECT_BY_ROOT",
     "CONSTRAINT",
     "CONTINUE",
     "CONVERT",
     "CREATE",
     "CROSS",
+    "CSN",
     "CUBE",
     "CUME_DIST",
     "CURRENT",
@@ -131,6 +136,7 @@ const RESERVED: &[&str] = &[
     "DEFERRABLE",
     "DELAYED",
     "DELETE",
+    "DELTAMERGE",
     "DENSE_RANK",
     "DESC",
     "DESCRIBE",
@@ -146,10 +152,12 @@ const RESERVED: &[&str] = &[
     "ELSE",
     "ELSEIF",
     "EMPTY",
+    "ENCKEY",
     "ENCLOSED",
     "END",
     "ESCAPED",
     "EXCEPT",
+    "EXCLUDED",
     "EXCLUSIVE",
     "EXISTS",
     "EXIT",
@@ -175,8 +183,10 @@ const RESERVED: &[&str] = &[
     "GRANT",
     "GROUP",
     "GROUPING",
+    "GROUPPARENT",
     "GROUPS",
     "HAVING",
+    "HDFSDIRECTORY",
     "HIGH_PRIORITY",
     "HOUR_MICROSECOND",
     "HOUR_MINUTE",
@@ -186,6 +196,7 @@ const RESERVED: &[&str] = &[
     "IF",
     "IGNORE",
     "ILIKE",
+    "IMCSTORED",
     "IMMEDIATE",
     "IN",
     "INCREMENT",
@@ -224,6 +235,7 @@ const RESERVED: &[&str] = &[
     "LEADING",
     "LEAVE",
     "LEFT",
+    "LESS",
     "LEVEL",
     "LIBRARY",
     "LIKE",
@@ -261,6 +273,7 @@ const RESERVED: &[&str] = &[
     "NESTED_TABLE_ID",
     "NOAUDIT",
     "NOCOMPRESS",
+    "NOCYCLE",
     "NOT",
     "NOTNULL",
     "NOWAIT",
@@ -291,6 +304,7 @@ const RESERVED: &[&str] = &[
     "PARTITION",
     "PCTFREE",
     "PERCENT_RANK",
+    "PERFORMANCE",
     "PLACING",
     "PRECISION",
     "PRIMARY",
@@ -308,8 +322,10 @@ const RESERVED: &[&str] = &[
     "READ_WRITE",
     "REAL",
     "RECURSIVE",
+    "RECYCLEBIN",
     "REFERENCES",
     "REGEXP",
+    "REJECT",
     "RELEASE",
     "RENAME",
     "REPEAT",
@@ -332,13 +348,16 @@ const RESERVED: &[&str] = &[
     "SCHEMAS",
     "SECOND_MICROSECOND",
     "SELECT",
+    "SELF",
     "SENSITIVE",
     "SEPARATOR",
     "SESSION",
+    "SESSIONTIMEZONE",
     "SESSION_USER",
     "SET",
     "SHARE",
     "SHOW",
+    "SHRINK",
     "SIGNAL",
     "SIMILAR",
     "SIZE",
@@ -364,10 +383,13 @@ const RESERVED: &[&str] = &[
     "SYSDATE",
     "SYSTEM",
     "SYSTEM_USER",
+    "SYSTIMESTAMP",
     "TABLE",
     "TABLESAMPLE",
     "TERMINATED",
     "THEN",
+    "TIDB_CURRENT_TSO",
+    "TIMECAPSULE",
     "TINYBLOB",
     "TINYINT",
     "TINYTEXT",
@@ -378,10 +400,12 @@ const RESERVED: &[&str] = &[
     "TRUE",
     "UID",
     "UNDO",
+    "UNIMCSTORED",
     "UNION",
     "UNIQUE",
     "UNLOCK",
     "UNSIGNED",
+    "UNTIL",
     "UPDATE",
     "USAGE",
     "USE",
@@ -399,6 +423,7 @@ const RESERVED: &[&str] = &[
     "VARIADIC",
     "VARYING",
     "VERBOSE",
+    "VERIFY",
     "VIEW",
     "VIRTUAL",
     "WHEN",
```

- [ ] **Step 16: 修改 `crates/dblens-dialect/src/meta/assemble.rs`**

```diff
--- a/crates/dblens-dialect/src/meta/assemble.rs
+++ b/crates/dblens-dialect/src/meta/assemble.rs
@@ -354,6 +354,8 @@ pub struct TableStorage {
     pub collation: Option<String>,
     pub tablespace: Option<String>,
     pub row_count: Option<i64>,
+    /// 主键是否聚簇（只有 TiDB 报告）：聚簇主键不能直接删除或更换，表设计器改为重建。
+    pub clustered: Option<bool>,
 }
 
 /// `referencing_keys` 的结果：`ref_schema` / `ref_table` 是被引用的这张表。
@@ -434,6 +436,7 @@ pub fn table_storage(rows: &MetaRows) -> Result<TableStorage> {
             collation: r.text("collation"),
             tablespace: r.text("tablespace"),
             row_count: r.int("row_count").filter(|n| *n >= 0),
+            clustered: r.columns().iter().any(|c| c == "clustered").then(|| r.flag("clustered")),
         })
         .unwrap_or_default())
 }
```

- [ ] **Step 17: 修改 `crates/dblens-dialect/src/meta/template.rs`**

```diff
--- a/crates/dblens-dialect/src/meta/template.rs
+++ b/crates/dblens-dialect/src/meta/template.rs
@@ -21,6 +21,8 @@ const ORACLE: &str = include_str!("../../sql/oracle.sql");
 const DAMENG: &str = include_str!("../../sql/dameng.sql");
 const KINGBASE: &str = include_str!("../../sql/kingbase.sql");
 const OPENGAUSS: &str = include_str!("../../sql/opengauss.sql");
+const TIDB: &str = include_str!("../../sql/tidb.sql");
+const OCEANBASE: &str = include_str!("../../sql/oceanbase.sql");
 
 pub(crate) const ALL_VARIANTS: [Variant; 12] = [
     Variant::MySql,
@@ -71,6 +73,8 @@ fn base_file(family: Family) -> &'static str {
 fn override_file(variant: Variant) -> Option<&'static str> {
     match variant {
         Variant::MariaDb => Some(MARIADB),
+        Variant::TiDb => Some(TIDB),
+        Variant::OceanBase => Some(OCEANBASE),
         Variant::Kingbase => Some(KINGBASE),
         Variant::OpenGauss => Some(OPENGAUSS),
         _ => None,
```

- [ ] **Step 18: 修改 `crates/dblens-driver-postgres/src/types.rs`**

```diff
--- a/crates/dblens-driver-postgres/src/types.rs
+++ b/crates/dblens-driver-postgres/src/types.rs
@@ -120,7 +120,8 @@ impl<'a> FromSql<'a> for RawValue<'a> {
 
 /// 兼容库自定义的类型对应的标准类型：域类型取基础类型（人大金仓 Oracle 模式的 `date` 是 timestamp 上的域，
 /// `blob`、`clob` 也是域）；openGauss 的 `clob`、`nvarchar2` 等是独立的类型，二进制格式与对应的标准类型相同。
-/// `int1`、`tinyint` 没有对应的标准类型，原样返回，由 `decode_as` 单独处理。
+/// `int1`、`tinyint`、`int16`（16 字节整数，openGauss 的序列参数用它）没有对应的标准类型，原样返回，
+/// 由 `decode_as` 单独处理。
 fn standard_type(ty: &Type) -> Option<Type> {
     if let Kind::Domain(base) = ty.kind() {
         return Some(standard_type(base).unwrap_or_else(|| base.clone()));
@@ -132,7 +133,7 @@ fn standard_type(ty: &Type) -> Option<Type> {
         "smalldatetime" => Some(Type::TIMESTAMP),
         "binary_double" => Some(Type::FLOAT8),
         "binary_float" => Some(Type::FLOAT4),
-        "int1" | "tinyint" => Some(ty.clone()),
+        "int1" | "tinyint" | "int16" => Some(ty.clone()),
         _ => None,
     }
 }
@@ -161,6 +162,13 @@ fn decode_as(ty: &Type, raw: &[u8]) -> std::result::Result<Value, BoxError> {
         // openGauss 的 int1 是 0–255，人大金仓的 tinyint 是 -128–127，都是 1 个字节
         _ if raw.len() == 1 && ty.name() == "int1" => Value::Int(raw[0].into()),
         _ if raw.len() == 1 && ty.name() == "tinyint" => Value::Int((raw[0] as i8).into()),
+        _ if ty.name() == "int16" => match <[u8; 16]>::try_from(raw) {
+            Ok(b) => match i64::try_from(i128::from_be_bytes(b)) {
+                Ok(v) => Value::Int(v),
+                Err(_) => Value::Decimal(i128::from_be_bytes(b).to_string()),
+            },
+            Err(_) => Value::Other { type_name: ty.name().to_string(), text: other_text(raw) },
+        },
         _ => Value::Other { type_name: ty.name().to_string(), text: other_text(raw) },
     })
 }
```

- [ ] **Step 19: 检查**

```bash
export CARGO_INCREMENTAL=0
cargo fmt --all
cargo clippy --config "source.mirror.registry='sparse+https://rsproxy.cn/index/'" --workspace --all-targets -- -D warnings
DBLENS_LANG=en cargo test --config "source.mirror.registry='sparse+https://rsproxy.cn/index/'" --workspace
```

实库测试：在上面的 `cargo test` 前以内联环境变量给出原有 5 个库的 `DBLENS_TEST_MYSQL_URL`、`DBLENS_TEST_MARIADB_URL`、`DBLENS_TEST_PG_URL`、`DBLENS_TEST_ORACLE_URL`、`DBLENS_TEST_DM_URL`，以及第一批的 `DBLENS_TEST_TIDB_URL`、`DBLENS_TEST_OCEANBASE_URL`（用户名写成 `admin%40test`）、`DBLENS_TEST_OPENGAUSS_URL`、`DBLENS_TEST_OPENGAUSS_A_URL`（库 `dblens_test_a`）、`DBLENS_TEST_OPENGAUSS_SHA_URL`（账号 `admin_sha`）、`DBLENS_TEST_KINGBASE_URL`（54321）、`DBLENS_TEST_KINGBASE_PG_URL`（54322）。新目标的密码在测试虚拟机 root 的 `/root/dblens-test/creds.env`，用 `ssh … 'sudo cat …'` 读进当前 shell 的变量，不写进任何文件。
- 人大金仓的开发版授权最多 10 个连接：它的测试用 `-- kingbase --test-threads=1` 单独串行运行。
- 达梦的测试与其他测试同在一个进程里运行时偶发"网络通讯失败"，失败时用 `-- dameng --test-threads=1` 单独重跑。

Expected: 全部测试通过（元数据实库测试 11 个；快照 9 个，新增 4 个）。

- [ ] **Step 20: 提交**

```bash
git add crates/dblens-dialect/sql/kingbase.sql crates/dblens-dialect/sql/oceanbase.sql crates/dblens-dialect/sql/opengauss.sql crates/dblens-dialect/sql/postgres.sql crates/dblens-dialect/sql/tidb.sql crates/dblens-dialect/src/design/rebuild.rs crates/dblens-dialect/src/ident.rs crates/dblens-dialect/src/meta/assemble.rs crates/dblens-dialect/src/meta/template.rs crates/dblens-dialect/tests/fixtures/opengauss.sql crates/dblens-dialect/tests/meta_live.rs crates/dblens-dialect/tests/meta_snapshots.rs crates/dblens-dialect/tests/snapshots/meta_snapshots__kingbase.snap crates/dblens-dialect/tests/snapshots/meta_snapshots__oceanbase.snap crates/dblens-dialect/tests/snapshots/meta_snapshots__opengauss.snap crates/dblens-dialect/tests/snapshots/meta_snapshots__postgres.snap crates/dblens-dialect/tests/snapshots/meta_snapshots__tidb.snap crates/dblens-driver-postgres/src/types.rs
git commit -m "feat(dialect): 国产库第一批的元数据模板" -m "新增 tidb.sql、oceanbase.sql、kingbase.sql、opengauss.sql 覆盖文件：系统 schema、分区、检查约束、序列、AUTO_RANDOM 与聚簇主键、同义词与包；PG 模板去掉与空串的比较（Oracle 兼容模式下空串即 NULL）；补充保留字。元数据实库测试覆盖新目标。" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 5: 出错位置与 Oracle 兼容模式下的语句拆分

- MySQL 驱动 `error.rs`：`locate` 认得两种写法：`near '…' at line N`（OceanBase 在后面另起一行附节点和追踪号，行号只取数字）和 TiDB 的 `line N column M near "…"`。
- `split.rs`：`StreamSplitter::with_oracle_blocks()`（只对 PG 家族生效）：识别 PL/SQL 块和独占一行的 `/`，块中遇到 `$$` 时按 PG 的写法在 `;` 处结束；`split_with`。
- `Dialect::split` 与新加的 `Dialect::splitter()` 按兼容模式；执行 SQL 文件用 `dialect.splitter()`。

**Files:**
- Modify: `crates/dblens-dialect/src/lib.rs`
- Modify: `crates/dblens-dialect/src/split.rs`
- Modify: `crates/dblens-driver-mysql/src/error.rs`
- Modify: `crates/dblens-service/src/sqlfile.rs`

**Interfaces:**
- Consumes：任务 2 的 `oracle_mode`。
- Produces：`StreamSplitter::with_oracle_blocks(self) -> Self`、`Dialect::splitter(&self) -> StreamSplitter`。

- [ ] **Step 1: 修改 `crates/dblens-dialect/src/lib.rs`**

```diff
--- a/crates/dblens-dialect/src/lib.rs
+++ b/crates/dblens-dialect/src/lib.rs
@@ -26,6 +26,7 @@ pub use dml::DmlTemplate;
 pub use import::{ImportStatement, MAX_PARAMS_PER_STATEMENT, MAX_ROWS_PER_STATEMENT, WriteMode};
 pub use ops::{OpsCategory, OpsQuery};
 pub use session::may_change_schema;
+use split::split_with;
 pub use split::{SqlStatement, StreamSplitter, split_statements};
 pub use variant::{Compat, Family, Variant};
 pub use version::parse_version;
@@ -138,7 +139,13 @@ impl Dialect {
     }
 
     pub fn split(&self, sql: &str) -> Vec<SqlStatement> {
-        split_statements(sql, self.family())
+        split_with(self.splitter(), sql)
+    }
+
+    /// 分块拆分用的拆分器：人大金仓、openGauss 的 Oracle 兼容模式另外识别 PL/SQL 块。
+    pub fn splitter(&self) -> StreamSplitter {
+        let s = StreamSplitter::new(self.family());
+        if self.oracle_mode() { s.with_oracle_blocks() } else { s }
     }
 
     /// 生成执行计划要依次执行的语句，最后一条的结果集是计划（见 `session::explain`）。
@@ -317,6 +324,14 @@ mod tests {
         assert_eq!(pg.with_compat(Compat::Oracle).compat(), Compat::Native);
     }
 
+    #[test]
+    fn split_follows_compat_mode() {
+        let sql = "CREATE PROCEDURE p AS BEGIN NULL; END;\n/\nSELECT 1";
+        let kb = Dialect::new(Variant::Kingbase);
+        assert_eq!(kb.with_compat(Compat::Oracle).split(sql).len(), 2);
+        assert_eq!(kb.split(sql).len(), 3);
+    }
+
     #[test]
     fn compat_modes() {
         assert!(
```

- [ ] **Step 2: 修改 `crates/dblens-dialect/src/split.rs`**

```diff
--- a/crates/dblens-dialect/src/split.rs
+++ b/crates/dblens-dialect/src/split.rs
@@ -34,7 +34,11 @@ pub struct SqlStatement {
 
 /// 按家族的语法规则拆分多条语句。
 pub fn split_statements(sql: &str, family: Family) -> Vec<SqlStatement> {
-    let mut s = StreamSplitter::new(family);
+    split_with(StreamSplitter::new(family), sql)
+}
+
+/// 用给定的拆分器拆分整段文本（`Dialect::split` 用它带上兼容模式）。
+pub(crate) fn split_with(mut s: StreamSplitter, sql: &str) -> Vec<SqlStatement> {
     let mut out = s.feed(sql);
     out.extend(s.finish());
     out
@@ -44,6 +48,8 @@ pub fn split_statements(sql: &str, family: Family) -> Vec<SqlStatement> {
 /// 语句的位置和行号是在全部已喂入文本中的。
 pub struct StreamSplitter {
     family: Family,
+    /// PG 家族的 Oracle 兼容模式（人大金仓、openGauss）：另外识别 PL/SQL 块和独占一行的 `/`。
+    oracle_blocks: bool,
     /// 尚未拆完的文本，从全文的 `base` 处开始：比当前语句的开头多留几个字节，供向前查看（标识符、行首）。
     buf: String,
     base: usize,
@@ -81,6 +87,7 @@ impl StreamSplitter {
     pub fn new(family: Family) -> Self {
         Self {
             family,
+            oracle_blocks: false,
             buf: String::new(),
             base: 0,
             i: 0,
@@ -110,8 +117,14 @@ impl StreamSplitter {
         self.out
     }
 
+    /// PG 家族在 Oracle 兼容模式下的拆分器：PL/SQL 块（`CREATE PROCEDURE … END;` 后跟独占一行的 `/`）
+    /// 与 `$$` 引号都识别。块中出现 `$$` 时按 PG 的写法处理，语句在 `;` 处结束。
+    pub fn with_oracle_blocks(self) -> Self {
+        Self { oracle_blocks: self.family == Family::Postgres, ..self }
+    }
+
     fn plsql(&self) -> bool {
-        matches!(self.family, Family::Oracle | Family::Dameng)
+        matches!(self.family, Family::Oracle | Family::Dameng) || self.oracle_blocks
     }
 
     /// 丢掉当前语句之前的文本（留几个字节供向前查看），内存只与当前语句的长度有关。
@@ -251,7 +264,7 @@ impl StreamSplitter {
                 }
                 Step::No
             }
-            Family::Oracle | Family::Dameng => {
+            _ if self.plsql() => {
                 if !complete {
                     let maybe = head.first().is_none_or(|&c| c == b'/')
                         && head.iter().skip(1).all(u8::is_ascii_whitespace);
@@ -365,7 +378,11 @@ impl StreamSplitter {
             match dollar_tag_len(b, i, eof) {
                 Tag::Wait => return Step::Wait,
                 Tag::No => return Step::No,
-                Tag::Len(len) => scan_dollar(b, i, len, eof),
+                Tag::Len(len) => {
+                    // Oracle 兼容模式下 `CREATE FUNCTION … AS $$ … $$` 是 PG 的写法，不按 PL/SQL 块等 `/`
+                    self.block = false;
+                    scan_dollar(b, i, len, eof)
+                }
             }
         } else {
             return Step::No;
@@ -581,6 +598,28 @@ mod tests {
         );
     }
 
+    /// 人大金仓、openGauss 的 Oracle 兼容模式：PL/SQL 块以独占一行的 `/` 结束，`$$` 写法仍在 `;` 处结束。
+    #[test]
+    fn postgres_oracle_blocks() {
+        let oracle = |sql: &str| -> Vec<String> {
+            split_with(StreamSplitter::new(Family::Postgres).with_oracle_blocks(), sql)
+                .into_iter()
+                .map(|s| s.text)
+                .collect()
+        };
+        let proc = "CREATE OR REPLACE PROCEDURE p AS BEGIN INSERT INTO t VALUES (1); END;";
+        let sql = format!("{proc}\n/\nSELECT 1;\nSELECT 2");
+        assert_eq!(oracle(&sql), [proc, "SELECT 1", "SELECT 2"]);
+        assert!(texts(&sql, Family::Postgres).len() > 3, "PG 模式不认 PL/SQL 块");
+        let func = "CREATE FUNCTION f() RETURNS int AS $$ BEGIN RETURN 1; END; $$ LANGUAGE plpgsql";
+        assert_eq!(oracle(&format!("{func};\nSELECT 2;")), [func, "SELECT 2"]);
+        let block = "DECLARE n int; BEGIN n := 1; END;";
+        assert_eq!(oracle(&format!("{block}\n/\n")), [block]);
+        // 只在 PG 家族上生效
+        let my = StreamSplitter::new(Family::MySql).with_oracle_blocks();
+        assert!(!my.plsql());
+    }
+
     #[test]
     fn postgres_dollar_quotes_and_strings() {
         let f = "CREATE FUNCTION f() RETURNS int AS $$ BEGIN RETURN 1; END; $$ LANGUAGE plpgsql; SELECT 1";
```

- [ ] **Step 3: 修改 `crates/dblens-driver-mysql/src/error.rs`**

```diff
--- a/crates/dblens-driver-mysql/src/error.rs
+++ b/crates/dblens-driver-mysql/src/error.rs
@@ -18,12 +18,12 @@ pub(crate) fn map_err(e: mysql_async::Error) -> Error {
     }
 }
 
-/// 从语法错误信息 `… near 'FROM t' at line 2` 找出出错位置：`near` 后面是从出错处开始的原文（可能被截断），
-/// 在第 2 行起找它的第一行；为空表示语句意外结束，位置在末尾。返回字节偏移。
+/// 从语法错误信息找出出错位置：`near` 后面是从出错处开始的原文（可能被截断），在所报的行里找它的第一行；
+/// 为空表示语句意外结束，位置在末尾。返回字节偏移。认得两种写法：
+/// - MySQL、MariaDB、OceanBase：`… near 'FROM t' at line 2`（OceanBase 在后面另起一行附上节点和追踪号）；
+/// - TiDB：`… line 2 column 21 near "FROM t" `。
 fn locate(sql: &str, message: &str) -> Option<usize> {
-    let (head, line) = message.rsplit_once("' at line ")?;
-    let line: usize = line.trim().parse().ok()?;
-    let near = &head[head.find(" near '")? + 7..];
+    let (line, near) = mysql_style(message).or_else(|| tidb_style(message))?;
     let line_start = sql.split_inclusive('\n').take(line.saturating_sub(1)).map(str::len).sum::<usize>();
     let snippet = near.split('\n').next().unwrap_or_default();
     if snippet.is_empty() {
@@ -32,6 +32,24 @@ fn locate(sql: &str, message: &str) -> Option<usize> {
     sql.get(line_start..)?.find(snippet).map(|at| line_start + at)
 }
 
+/// `near '…' at line N`：返回行号和 near 后的原文。
+fn mysql_style(message: &str) -> Option<(usize, &str)> {
+    let (head, tail) = message.rsplit_once("' at line ")?;
+    let digits = tail.find(|c: char| !c.is_ascii_digit()).unwrap_or(tail.len());
+    let line = tail[..digits].parse().ok()?;
+    Some((line, &head[head.find(" near '")? + 7..]))
+}
+
+/// TiDB 的 `line N column M near "…" `：near 后的原文到最后一个双引号为止。
+fn tidb_style(message: &str) -> Option<(usize, &str)> {
+    let at = message.find(" line ")?;
+    let rest = &message[at + 6..];
+    let digits = rest.find(|c: char| !c.is_ascii_digit())?;
+    let line = rest[..digits].parse().ok()?;
+    let near = &rest[rest.find(" near \"")? + 7..];
+    Some((line, &near[..near.rfind('"')?]))
+}
+
 /// 给语法错误补上出错位置（字符偏移，从 1 开始）。
 pub(crate) fn with_position(e: Error, sql: &str) -> Error {
     match e {
@@ -77,6 +95,14 @@ mod tests {
         assert_eq!(at(sql, &syntax("FROM t\nWHERE x", 2)).as_deref(), Some("FROM t\nWHERE x"));
         assert_eq!(at("SELECT 1 FROM DUAL WHERE", &syntax("", 1)).as_deref(), Some(""));
         assert_eq!(at("SELECT 1", "Table 'x' doesn't exist"), None);
+        // OceanBase 4.4 与 TiDB 8.5 的实测信息
+        let ob = format!(
+            "{}\n[127.0.0.1:2882] [2026-10-28 01:14:03.030007] [YB427F000001-00065EDC1E3E1735-0-0]",
+            syntax("FROM t", 1)
+        );
+        assert_eq!(at(sql, &ob).as_deref(), Some("FROM t\nWHERE x"));
+        let tidb = "You have an error in your SQL syntax; check the manual that corresponds to your TiDB version for the right syntax to use line 2 column 10 near \"FROM t\nWHERE x\" ";
+        assert_eq!(at(sql, tidb).as_deref(), Some("FROM t\nWHERE x"));
         let e = Error::Sql { code: "1064".into(), message: syntax("FROM t", 1), position: None };
         let Error::Sql { position, .. } = with_position(e, "SELECT '数洞', FROM t") else { panic!() };
         assert_eq!(position, Some(14));
```

- [ ] **Step 4: 修改 `crates/dblens-service/src/sqlfile.rs`**

```diff
--- a/crates/dblens-service/src/sqlfile.rs
+++ b/crates/dblens-service/src/sqlfile.rs
@@ -113,7 +113,7 @@ impl Statements {
     fn open(file: &SourceFile, encoding: TextEncoding, dialect: Dialect) -> Self {
         Self {
             rx: spawn_text(file.path.clone(), encoding),
-            splitter: Some(StreamSplitter::new(dialect.family())),
+            splitter: Some(dialect.splitter()),
             ready: VecDeque::new(),
         }
     }
```

- [ ] **Step 5: 检查**

```bash
export CARGO_INCREMENTAL=0
cargo fmt --all
cargo clippy --config "source.mirror.registry='sparse+https://rsproxy.cn/index/'" --workspace --all-targets -- -D warnings
DBLENS_LANG=en cargo test --config "source.mirror.registry='sparse+https://rsproxy.cn/index/'" --workspace
```

Expected: 全部测试通过（mysql 驱动、dialect 各新增断言）。

- [ ] **Step 6: 提交**

```bash
git add crates/dblens-dialect/src/lib.rs crates/dblens-dialect/src/split.rs crates/dblens-driver-mysql/src/error.rs crates/dblens-service/src/sqlfile.rs
git commit -m "feat: TiDB、OceanBase 的出错位置；Oracle 兼容模式下的语句拆分" -m "出错位置认得 OceanBase 带追踪号的报错和 TiDB 的 line/column 写法；人大金仓、openGauss 的 Oracle 兼容模式识别 PL/SQL 块和独占一行的 /，$$ 写法照常。" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 6: 方言：TiDB、OceanBase、openGauss 的运维查询

新的覆盖文件（`ops.rs` 登记）：
- `sql/ops/tidb.sql`：`locks` 从 `DATA_LOCK_WAITS` 与 `CLUSTER_TIDB_TRX` 读。
- `sql/ops/oceanbase.sql`：`locks` 从 `GV$OB_LOCKS`（等待的一行 `BLOCK = 1`、`ID1` 是所等的事务）与 `GV$OB_PROCESSLIST` 读；`slow_sql` 从 `GV$OB_PLAN_CACHE_PLAN_STAT` 读（微秒）。
- `sql/ops/opengauss.sql`：`sessions`、`running`（没有 `backend_type`、`wait_event_type`，有 `waiting`、`enqueue`）、`locks`（`pg_locks` 自连接，没有 `pg_blocking_pids`）、`slow_sql`（`dbe_perf.statement`，需要 MONADMIN）。

**Files:**
- Modify: `crates/dblens-dialect/sql/ops/mysql.sql`
- Create: `crates/dblens-dialect/sql/ops/oceanbase.sql`
- Create: `crates/dblens-dialect/sql/ops/opengauss.sql`
- Modify: `crates/dblens-dialect/sql/ops/postgres.sql`
- Create: `crates/dblens-dialect/sql/ops/tidb.sql`
- Modify: `crates/dblens-dialect/src/ops.rs`

**Interfaces:**
- Consumes：无。

- [ ] **Step 1: 修改 `crates/dblens-dialect/sql/ops/mysql.sql`**

```diff
--- a/crates/dblens-dialect/sql/ops/mysql.sql
+++ b/crates/dblens-dialect/sql/ops/mysql.sql
@@ -1,5 +1,5 @@
--- MySQL ops queries (M1c3). Also used by TiDB, OceanBase and PolarDB, which may lack some of the views:
--- the database's own error is shown in that case. Timers in performance_schema are picoseconds.
+-- MySQL ops queries (M1c3). Also used by PolarDB; TiDB and OceanBase override what they lack (tidb.sql,
+-- oceanbase.sql). A database that still lacks a view shows its own error. Timers in performance_schema are picoseconds.
 
 -- [sessions]
 -- name.zh-CN: 会话列表
```

- [ ] **Step 2: 新建 `crates/dblens-dialect/sql/ops/oceanbase.sql`**

```sql
-- OceanBase (MySQL mode) overrides of the MySQL ops queries. There is neither the sys schema nor
-- performance_schema statement digests: locks come from GV$OB_LOCKS (a waiting row has BLOCK = 1, ID1 is the
-- transaction it waits for) and slow SQL from the plan cache statistics. Times there are microseconds.

-- [locks]
-- name.zh-CN: 锁等待
-- name.en: Lock Waits
-- category: lock
SELECT w.SESSION_ID AS waiting_pid, LEFT(wp.INFO, 200) AS waiting_sql,
       h.SESSION_ID AS blocking_pid, LEFT(hp.INFO, 200) AS blocking_sql,
       w.TYPE AS lock_type, w.CTIME AS wait_seconds,
       CONCAT('KILL ', h.SESSION_ID) AS kill_sql
FROM oceanbase.GV$OB_LOCKS w
LEFT JOIN oceanbase.GV$OB_LOCKS h ON h.TRANS_ID = w.ID1 AND h.BLOCK = 0
LEFT JOIN oceanbase.GV$OB_PROCESSLIST wp ON wp.ID = w.SESSION_ID
LEFT JOIN oceanbase.GV$OB_PROCESSLIST hp ON hp.ID = h.SESSION_ID
WHERE w.BLOCK = 1
ORDER BY w.CTIME DESC

-- [slow_sql]
-- name.zh-CN: 慢 SQL
-- name.en: Slow SQL
-- category: sql
SELECT LEFT(p.QUERY_SQL, 300) AS sql_text, p.EXECUTIONS AS executions,
       ROUND(p.AVG_EXE_USEC / 1000, 2) AS avg_ms,
       ROUND(p.SLOWEST_EXE_USEC / 1000, 2) AS max_ms,
       ROUND(p.ELAPSED_TIME / 1000, 2) AS total_ms,
       p.ROWS_PROCESSED AS rows_processed, p.LAST_ACTIVE_TIME AS last_seen
FROM oceanbase.GV$OB_PLAN_CACHE_PLAN_STAT p
ORDER BY p.AVG_EXE_USEC DESC
LIMIT 20
```

- [ ] **Step 3: 新建 `crates/dblens-dialect/sql/ops/opengauss.sql`**

```sql
-- openGauss overrides of the PostgreSQL ops queries. pg_stat_activity has no backend_type / wait_event_type
-- (it has waiting / enqueue), there is no pg_blocking_pids, and statement statistics are in dbe_perf.statement
-- (microseconds) instead of pg_stat_statements.

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
WHERE a.usename IS NOT NULL
ORDER BY a.query_start NULLS LAST

-- [running]
-- name.zh-CN: 正在执行的语句
-- name.en: Running Statements
-- category: session
SELECT a.pid, a.usename AS user, a.client_addr::text AS host, a.datname AS db,
       ROUND(EXTRACT(EPOCH FROM now() - a.query_start)) AS seconds,
       a.waiting, a.enqueue, LEFT(a.query, 500) AS current_sql,
       'SELECT pg_terminate_backend(' || a.pid || ')' AS kill_sql
FROM pg_stat_activity a
WHERE a.usename IS NOT NULL AND a.state = 'active' AND a.pid <> pg_backend_pid()
ORDER BY a.query_start

-- [locks]
-- name.zh-CN: 锁等待
-- name.en: Lock Waits
-- category: lock
SELECT w.pid AS waiting_pid, LEFT(wa.query, 200) AS waiting_sql,
       h.pid AS blocking_pid, LEFT(ha.query, 200) AS blocking_sql,
       w.locktype AS lock_type, w.relation::regclass::text AS locked_object,
       ROUND(EXTRACT(EPOCH FROM now() - wa.query_start)) AS wait_seconds,
       'SELECT pg_terminate_backend(' || h.pid || ')' AS kill_sql
FROM pg_locks w
JOIN pg_locks h ON h.granted AND NOT w.granted AND h.pid <> w.pid AND h.locktype = w.locktype
               AND h.database IS NOT DISTINCT FROM w.database
               AND h.relation IS NOT DISTINCT FROM w.relation
               AND h.transactionid IS NOT DISTINCT FROM w.transactionid
JOIN pg_stat_activity wa ON wa.pid = w.pid
JOIN pg_stat_activity ha ON ha.pid = h.pid
ORDER BY wait_seconds DESC

-- [slow_sql]
-- name.zh-CN: 慢 SQL
-- name.en: Slow SQL
-- category: sql
SELECT s.user_name AS user, LEFT(s.query, 300) AS sql_text, s.n_calls AS executions,
       ROUND(s.total_elapse_time / s.n_calls / 1000.0, 2) AS avg_ms,
       ROUND(s.max_elapse_time / 1000.0, 2) AS max_ms,
       ROUND(s.total_elapse_time / 1000.0, 2) AS total_ms,
       s.n_returned_rows AS rows_returned, s.last_updated AS last_seen
FROM dbe_perf.statement s
WHERE s.n_calls > 0
ORDER BY s.total_elapse_time / s.n_calls DESC
LIMIT 20
```

- [ ] **Step 4: 修改 `crates/dblens-dialect/sql/ops/postgres.sql`**

```diff
--- a/crates/dblens-dialect/sql/ops/postgres.sql
+++ b/crates/dblens-dialect/sql/ops/postgres.sql
@@ -1,5 +1,5 @@
--- PostgreSQL ops queries (M1c3). Also used by KingbaseES, openGauss, GaussDB and HighGo, which may name
--- some columns differently: the database's own error is shown in that case.
+-- PostgreSQL ops queries (M1c3). Also used by KingbaseES, GaussDB and HighGo; openGauss overrides what it
+-- names differently (opengauss.sql). A database that still lacks a view shows its own error.
 -- slow_sql needs the pg_stat_statements extension (PostgreSQL 13+ column names).
 
 -- [sessions]
```

- [ ] **Step 5: 新建 `crates/dblens-dialect/sql/ops/tidb.sql`**

```sql
-- TiDB overrides of the MySQL ops queries. There is no sys schema; lock waits come from DATA_LOCK_WAITS
-- (pessimistic transactions) with the transactions of both sides from CLUSTER_TIDB_TRX.

-- [locks]
-- name.zh-CN: 锁等待
-- name.en: Lock Waits
-- category: lock
SELECT wt.SESSION_ID AS waiting_pid, LEFT(wt.CURRENT_SQL_DIGEST_TEXT, 200) AS waiting_sql,
       bt.SESSION_ID AS blocking_pid, LEFT(bt.CURRENT_SQL_DIGEST_TEXT, 200) AS blocking_sql,
       w.KEY AS locked_key,
       TIMESTAMPDIFF(SECOND, wt.WAITING_START_TIME, NOW()) AS wait_seconds,
       CONCAT('KILL ', bt.SESSION_ID) AS kill_sql
FROM information_schema.DATA_LOCK_WAITS w
LEFT JOIN information_schema.CLUSTER_TIDB_TRX wt ON wt.ID = w.TRX_ID
LEFT JOIN information_schema.CLUSTER_TIDB_TRX bt ON bt.ID = w.CURRENT_HOLDING_TRX_ID
ORDER BY wait_seconds DESC
```

- [ ] **Step 6: 修改 `crates/dblens-dialect/src/ops.rs`**

```diff
--- a/crates/dblens-dialect/src/ops.rs
+++ b/crates/dblens-dialect/src/ops.rs
@@ -22,6 +22,9 @@ const MARIADB: &str = include_str!("../sql/ops/mariadb.sql");
 const POSTGRES: &str = include_str!("../sql/ops/postgres.sql");
 const ORACLE: &str = include_str!("../sql/ops/oracle.sql");
 const DAMENG: &str = include_str!("../sql/ops/dameng.sql");
+const TIDB: &str = include_str!("../sql/ops/tidb.sql");
+const OCEANBASE: &str = include_str!("../sql/ops/oceanbase.sql");
+const OPENGAUSS: &str = include_str!("../sql/ops/opengauss.sql");
 
 /// 运维查询的分类。
 #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
@@ -118,6 +121,9 @@ fn base_file(family: Family) -> &'static str {
 fn override_file(variant: Variant) -> Option<&'static str> {
     match variant {
         Variant::MariaDb => Some(MARIADB),
+        Variant::TiDb => Some(TIDB),
+        Variant::OceanBase => Some(OCEANBASE),
+        Variant::OpenGauss => Some(OPENGAUSS),
         _ => None,
     }
 }
@@ -187,11 +193,19 @@ mod tests {
         let locks = |qs: &[OpsQuery]| qs.iter().find(|q| q.key == "locks").unwrap().sql.clone();
         assert_ne!(locks(my), locks(maria));
         assert_eq!(maria[2].key, "locks");
-        // 兼容库用所属家族的模板
-        assert_eq!(Dialect::new(Variant::TiDb).ops_queries(), my);
+        // 没有覆盖文件的兼容库用所属家族的模板；有覆盖文件的只替换其中的查询，顺序不变
+        assert_eq!(Dialect::new(Variant::PolarDb).ops_queries(), my);
         assert_eq!(
             Dialect::new(Variant::Kingbase).ops_queries(),
             Dialect::new(Variant::Postgres).ops_queries()
         );
+        let keys = |qs: &[OpsQuery]| qs.iter().map(|q| q.key.clone()).collect::<Vec<_>>();
+        for v in [Variant::TiDb, Variant::OceanBase] {
+            assert_eq!(keys(Dialect::new(v).ops_queries()), keys(my), "{v:?}");
+            assert_ne!(locks(Dialect::new(v).ops_queries()), locks(my), "{v:?}");
+        }
+        let og = Dialect::new(Variant::OpenGauss).ops_queries();
+        assert_eq!(keys(og), keys(Dialect::new(Variant::Postgres).ops_queries()));
+        assert!(og.iter().all(|q| !q.sql.contains("backend_type") && !q.sql.contains("pg_blocking_pids")));
     }
 }
```

- [ ] **Step 7: 检查**

```bash
export CARGO_INCREMENTAL=0
cargo fmt --all
cargo clippy --config "source.mirror.registry='sparse+https://rsproxy.cn/index/'" --workspace --all-targets -- -D warnings
DBLENS_LANG=en cargo test --config "source.mirror.registry='sparse+https://rsproxy.cn/index/'" --workspace
```

Expected: 全部测试通过。

- [ ] **Step 8: 提交**

```bash
git add crates/dblens-dialect/sql/ops/mysql.sql crates/dblens-dialect/sql/ops/oceanbase.sql crates/dblens-dialect/sql/ops/opengauss.sql crates/dblens-dialect/sql/ops/postgres.sql crates/dblens-dialect/sql/ops/tidb.sql crates/dblens-dialect/src/ops.rs
git commit -m "feat(dialect): TiDB、OceanBase、openGauss 的运维查询" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 7: 国产库的数据编辑与导入

- PG 驱动：参数先按 `standard_type` 换成标准类型再编码（人大金仓 Oracle 模式的 DATE 列收到日期值时补零点，按 timestamp 发出）。
- `import.rs`：openGauss 的按主键更新用 `ON DUPLICATE KEY UPDATE 列 = EXCLUDED.列`（只映射了主键列时 `NOTHING`）。
- `changes.rs`：Oracle 兼容模式中定位行时原值为空串的写成 `IS NULL`。

**Files:**
- Modify: `crates/dblens-dialect/src/changes.rs`
- Modify: `crates/dblens-dialect/src/import.rs`
- Modify: `crates/dblens-driver-postgres/src/types.rs`

**Interfaces:**
- Consumes：任务 2 的 `oracle_mode`、任务 3 的 `standard_type`。

- [ ] **Step 1: 修改 `crates/dblens-dialect/src/changes.rs`**

```diff
--- a/crates/dblens-dialect/src/changes.rs
+++ b/crates/dblens-dialect/src/changes.rs
@@ -97,10 +97,12 @@ impl<'a> Builder<'a> {
         Ok(())
     }
 
-    /// `列 = 值`；值为 NULL 时 `列 IS NULL`（不占参数）。
+    /// `列 = 值`；值为 NULL 时 `列 IS NULL`（不占参数）。Oracle 兼容模式（人大金仓、openGauss）中空串就是 NULL，
+    /// 也写成 `IS NULL`。
     fn equals(&mut self, col: &str, v: &Value) -> Result<()> {
         self.text(&self.dialect.ident(col));
-        if matches!(v, Value::Null) {
+        let empty = matches!(v, Value::String(s) if s.is_empty());
+        if matches!(v, Value::Null) || (empty && self.dialect.oracle_mode()) {
             self.text(" IS NULL");
             Ok(())
         } else {
@@ -248,7 +250,7 @@ mod tests {
     use dblens_core::schema::{PrimaryKey, UniqueConstraint};
 
     use super::*;
-    use crate::Variant;
+    use crate::{Compat, Variant};
 
     fn col(name: &str, logical: LogicalType) -> Column {
         Column {
@@ -326,6 +328,22 @@ mod tests {
         );
     }
 
+    #[test]
+    fn empty_string_is_null_in_oracle_mode() {
+        // 定位行的原值中有空串（只在没有主键、按全部列定位等情况下出现）
+        let change = RowChange::Update {
+            key: map(&[("id", Value::Int(1))]),
+            original: map(&[("name", s(""))]),
+            values: map(&[("name", s("新"))]),
+        };
+        let kb = Dialect::new(Variant::Kingbase);
+        let oracle = kb.with_compat(Compat::Oracle);
+        let st = oracle.change_statements("public", &orders(), std::slice::from_ref(&change)).unwrap();
+        assert_eq!(st[0].sql, "UPDATE public.orders SET name = $1 WHERE id = $2 AND name IS NULL");
+        let st = kb.change_statements("public", &orders(), &[change]).unwrap();
+        assert_eq!(st[0].sql, "UPDATE public.orders SET name = $1 WHERE id = $2 AND name = $3");
+    }
+
     #[test]
     fn order_is_delete_update_insert_and_placeholders_follow_dialect() {
         let changes = [
```

- [ ] **Step 2: 修改 `crates/dblens-dialect/src/import.rs`**

```diff
--- a/crates/dblens-dialect/src/import.rs
+++ b/crates/dblens-dialect/src/import.rs
@@ -7,7 +7,7 @@ use dblens_core::schema::{Column, Table};
 use dblens_core::{Error, LogicalType, Result, Value, msg};
 use serde::{Deserialize, Serialize};
 
-use crate::{Dialect, Family};
+use crate::{Dialect, Family, Variant};
 
 /// 每条语句至多包含的行数。
 pub const MAX_ROWS_PER_STATEMENT: usize = 500;
@@ -180,6 +180,16 @@ impl Binder<'_> {
                 };
                 self.sql.push_str(&format!(" ON DUPLICATE KEY UPDATE {}", sets.join(", ")));
             }
+            // openGauss 没有 ON CONFLICT，用它自己的 ON DUPLICATE KEY UPDATE（可引用 EXCLUDED）
+            _ if self.dialect.variant() == Variant::OpenGauss => {
+                self.sql.push_str(" ON DUPLICATE KEY UPDATE ");
+                if others.is_empty() {
+                    self.sql.push_str("NOTHING");
+                } else {
+                    let sets: Vec<String> = others.iter().map(|n| format!("{n} = EXCLUDED.{n}")).collect();
+                    self.sql.push_str(&sets.join(", "));
+                }
+            }
             _ => {
                 let keys: Vec<&str> = key.iter().map(|&k| names[k].as_str()).collect();
                 self.sql.push_str(&format!(" ON CONFLICT ({}) DO ", keys.join(", ")));
@@ -337,6 +347,12 @@ mod tests {
             ["INSERT INTO public.orders (id, name, price) VALUES ($1, $2, $3::text::numeric), ($4, $5, $6) \
               ON CONFLICT (id) DO UPDATE SET name = EXCLUDED.name, price = EXCLUDED.price"]
         );
+        assert_eq!(
+            sql(Variant::OpenGauss, "public", WriteMode::Upsert),
+            [
+                "INSERT INTO public.orders (id, name, price) VALUES ($1, $2, $3::text::numeric), ($4, $5, $6) ON DUPLICATE KEY UPDATE name = EXCLUDED.name, price = EXCLUDED.price"
+            ]
+        );
         // Oracle、达梦：一行一条 MERGE
         let ora = sql(Variant::Oracle, "HR", WriteMode::Upsert);
         assert_eq!(ora.len(), 2);
```

- [ ] **Step 3: 修改 `crates/dblens-driver-postgres/src/types.rs`**

```diff
--- a/crates/dblens-driver-postgres/src/types.rs
+++ b/crates/dblens-driver-postgres/src/types.rs
@@ -240,6 +240,10 @@ pub(crate) struct PgParam<'a>(pub &'a Value);
 
 impl ToSql for PgParam<'_> {
     fn to_sql(&self, ty: &Type, out: &mut BytesMut) -> std::result::Result<IsNull, BoxError> {
+        // 兼容库的域类型和自定义类型按对应的标准类型编码（人大金仓 Oracle 模式的 date 是 timestamp 上的域）
+        if let Some(base) = standard_type(ty).filter(|b| b != ty) {
+            return self.to_sql(&base, out);
+        }
         match self.0 {
             Value::Null => Ok(IsNull::Yes),
             Value::Bool(b) => b.to_sql(ty, out),
@@ -260,6 +264,7 @@ impl ToSql for PgParam<'_> {
             Value::Decimal(_) if *ty == Type::NUMERIC => Err(numeric_param_hint()),
             Value::String(s) | Value::Decimal(s) | Value::Interval(s) => s.as_str().to_sql(ty, out),
             Value::Bytes(b) => b.as_slice().to_sql(ty, out),
+            Value::Date(d) if *ty == Type::TIMESTAMP => d.and_time(chrono::NaiveTime::MIN).to_sql(ty, out),
             Value::Date(d) => d.to_sql(ty, out),
             Value::Time(t) => t.to_sql(ty, out),
             Value::DateTime(dt) => dt.to_sql(ty, out),
```

- [ ] **Step 4: 检查**

```bash
export CARGO_INCREMENTAL=0
cargo fmt --all
cargo clippy --config "source.mirror.registry='sparse+https://rsproxy.cn/index/'" --workspace --all-targets -- -D warnings
DBLENS_LANG=en cargo test --config "source.mirror.registry='sparse+https://rsproxy.cn/index/'" --workspace
```

Expected: 全部测试通过（dialect 新增 1 个、断言 1 处）。

- [ ] **Step 5: 提交**

```bash
git add crates/dblens-dialect/src/changes.rs crates/dblens-dialect/src/import.rs crates/dblens-driver-postgres/src/types.rs
git commit -m "feat: 国产库的数据编辑与导入" -m "PG 驱动的参数按兼容库类型对应的标准类型编码（人大金仓 Oracle 模式的 DATE）；openGauss 的按主键更新用 ON DUPLICATE KEY UPDATE；Oracle 兼容模式下定位行时空串按 IS NULL。" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 8: 方言：国产库的表设计器

- `catalog.rs`：TiDB 检查约束按只解析不执行、不建降序索引、索引方法只有 BTREE；OceanBase 执行检查约束、不建降序索引、索引方法 BTREE / FULLTEXT；openGauss 没有 BRIN、SPGIST；人大金仓、openGauss 的 Oracle 模式另有 NUMBER、VARCHAR2、NVARCHAR2、CLOB、BLOB、DATE。
- `sql.rs`：TiDB 删除主键时 `design.unsupported.clustered_pk`（重建）。
- `postgres.rs`：openGauss 的自增列建成 `serial` / `bigserial` / `smallserial`；已有列改成自增时建归这一列所有的序列（从现有最大值之后开始）并设默认值，取消自增去掉默认值。
- `oceanbase.sql` 的 `triggers`：`ACTION_STATEMENT` 是 CREATE 之后的整条语句，定义写成 `CONCAT('CREATE ', ACTION_STATEMENT)`（重建表时重建触发器用到）；快照随之更新。

后端词条加 `design.unsupported.clustered_pk`。

**Files:**
- Modify: `crates/dblens-core/locales/app.json`
- Modify: `crates/dblens-dialect/sql/oceanbase.sql`
- Modify: `crates/dblens-dialect/src/design/catalog.rs`
- Modify: `crates/dblens-dialect/src/design/postgres.rs`
- Modify: `crates/dblens-dialect/src/design/sql.rs`
- Modify: `crates/dblens-dialect/tests/snapshots/meta_snapshots__oceanbase.snap`

**Interfaces:**
- Consumes：任务 4 的 OceanBase 模板。

- [ ] **Step 1: 修改 `crates/dblens-dialect/tests/snapshots/meta_snapshots__oceanbase.snap`**

```diff
--- a/crates/dblens-dialect/tests/snapshots/meta_snapshots__oceanbase.snap
+++ b/crates/dblens-dialect/tests/snapshots/meta_snapshots__oceanbase.snap
@@ -126,14 +126,13 @@ WHERE s.TABLE_SCHEMA = ?
 ORDER BY s.TABLE_NAME, s.INDEX_NAME, s.SEQ_IN_INDEX
 
 -- [triggers] [String("HR"), String("EMP")]
--- definition: a CREATE TRIGGER statement rebuilt from TRIGGERS (FOLLOWS/PRECEDES order and DEFINER are not included).
--- MySQL allows exactly one event per trigger, so events is that single event.
+-- ACTION_STATEMENT holds the whole statement after CREATE ("TRIGGER name BEFORE INSERT ON t FOR EACH ROW ..."),
+-- not just the body.
 SELECT t.EVENT_OBJECT_TABLE AS table_name,
        t.TRIGGER_NAME AS name,
        t.ACTION_TIMING AS timing,
        t.EVENT_MANIPULATION AS events,
-       CONCAT('CREATE TRIGGER `', REPLACE(t.TRIGGER_NAME, '`', '``'), '` ', t.ACTION_TIMING, ' ', t.EVENT_MANIPULATION,
-              ' ON `', REPLACE(t.EVENT_OBJECT_TABLE, '`', '``'), '` FOR EACH ROW ', t.ACTION_STATEMENT) AS definition
+       CONCAT('CREATE ', t.ACTION_STATEMENT) AS definition
 FROM information_schema.TRIGGERS t
 WHERE t.EVENT_OBJECT_SCHEMA = ?
   AND t.EVENT_OBJECT_TABLE = ?
```

- [ ] **Step 2: 修改 `crates/dblens-core/locales/app.json`**

```diff
--- a/crates/dblens-core/locales/app.json
+++ b/crates/dblens-core/locales/app.json
@@ -42,6 +42,7 @@
   "design.type_empty": {"zh-CN": "类型不能为空", "en": "Type is required"},
   "design.unknown_column": {"zh-CN": "列 %{column} 不存在", "en": "Column %{column} does not exist"},
   "design.unnamed_constraint": {"zh-CN": "约束没有名字，无法删除", "en": "The constraint has no name and cannot be dropped"},
+  "design.unsupported.clustered_pk": {"zh-CN": "TiDB 的聚簇主键不能直接删除或更换", "en": "TiDB cannot drop or replace a clustered primary key in place"},
   "design.unsupported.generated": {"zh-CN": "该库不能修改已有列 %{column} 的生成表达式", "en": "This database cannot change the generation expression of existing column %{column}"},
   "design.unsupported.identity": {"zh-CN": "该库不能改变已有列 %{column} 的自增属性", "en": "This database cannot change the auto-increment of existing column %{column}"},
   "design.unsupported.reorder": {"zh-CN": "该库不能调整已有列的顺序或在指定位置加列", "en": "This database cannot reorder existing columns or add a column at a given position"},
```

- [ ] **Step 3: 修改 `crates/dblens-dialect/sql/oceanbase.sql`**

```diff
--- a/crates/dblens-dialect/sql/oceanbase.sql
+++ b/crates/dblens-dialect/sql/oceanbase.sql
@@ -59,3 +59,18 @@ WHERE t.TABLE_SCHEMA = {{ bind("schema") }}
        OR UPPER(COALESCE(t.TABLE_COMMENT, '')) LIKE UPPER({{ bind("filter") }}) ESCAPE '!')
 {% endif %}
 ORDER BY t.TABLE_NAME
+
+-- [triggers]
+-- ACTION_STATEMENT holds the whole statement after CREATE ("TRIGGER name BEFORE INSERT ON t FOR EACH ROW ..."),
+-- not just the body.
+SELECT t.EVENT_OBJECT_TABLE AS table_name,
+       t.TRIGGER_NAME AS name,
+       t.ACTION_TIMING AS timing,
+       t.EVENT_MANIPULATION AS events,
+       CONCAT('CREATE ', t.ACTION_STATEMENT) AS definition
+FROM information_schema.TRIGGERS t
+WHERE t.EVENT_OBJECT_SCHEMA = {{ bind("schema") }}
+{% if table %}
+  AND t.EVENT_OBJECT_TABLE = {{ bind("table") }}
+{% endif %}
+ORDER BY t.EVENT_OBJECT_TABLE, t.TRIGGER_NAME
```

- [ ] **Step 4: 修改 `crates/dblens-dialect/src/design/catalog.rs`**

```diff
--- a/crates/dblens-dialect/src/design/catalog.rs
+++ b/crates/dblens-dialect/src/design/catalog.rs
@@ -103,6 +103,10 @@ const POSTGRES_TYPES: &[TypeEntry] = &[
     t("xml", N),
 ];
 
+/// 人大金仓、openGauss 的 Oracle 兼容模式另外可选的 Oracle 风格类型（DATE 带时间）。
+const PG_ORACLE_MODE_TYPES: &[TypeEntry] =
+    &[t("NUMBER", P), t("VARCHAR2", L), t("NVARCHAR2", L), t("CLOB", N), t("BLOB", N), t("DATE", N)];
+
 const ORACLE_TYPES: &[TypeEntry] = &[
     t("NUMBER", P),
     t("INTEGER", N),
@@ -156,16 +160,27 @@ impl Dialect {
         match self.family() {
             Family::MySql => DesignCaps {
                 reorder: true,
-                // MySQL 8.0.16、MariaDB 10.2 起执行检查约束
+                // MySQL 8.0.16、MariaDB 10.2 起执行检查约束；OceanBase 报告 5.7 但执行检查约束；
+                // TiDB 默认只解析不执行（tidb_enable_check_constraint 关闭）
                 checks: match self.variant() {
                     Variant::MariaDb => v >= 100200,
+                    Variant::OceanBase => true,
+                    Variant::TiDb => false,
                     _ => v >= 80016,
                 },
-                // 8.0 起支持降序索引
-                descending_index: self.variant() == Variant::MariaDb || v >= 80000,
+                // 8.0 起支持降序索引；TiDB、OceanBase 接受 DESC 但按升序建
+                descending_index: match self.variant() {
+                    Variant::MariaDb => true,
+                    Variant::TiDb | Variant::OceanBase => false,
+                    _ => v >= 80000,
+                },
                 on_delete: vec!["RESTRICT", "CASCADE", "SET NULL", "NO ACTION"],
                 on_update: vec!["RESTRICT", "CASCADE", "SET NULL", "NO ACTION"],
-                index_methods: vec!["BTREE", "HASH", "FULLTEXT", "SPATIAL"],
+                index_methods: match self.variant() {
+                    Variant::TiDb => vec!["BTREE"],
+                    Variant::OceanBase => vec!["BTREE", "FULLTEXT"],
+                    _ => vec!["BTREE", "HASH", "FULLTEXT", "SPATIAL"],
+                },
                 types: MYSQL_TYPES.to_vec(),
             },
             Family::Postgres => DesignCaps {
@@ -174,8 +189,15 @@ impl Dialect {
                 descending_index: true,
                 on_delete: vec!["NO ACTION", "RESTRICT", "CASCADE", "SET NULL", "SET DEFAULT"],
                 on_update: vec!["NO ACTION", "RESTRICT", "CASCADE", "SET NULL", "SET DEFAULT"],
-                index_methods: vec!["BTREE", "HASH", "GIN", "GIST", "BRIN", "SPGIST"],
-                types: POSTGRES_TYPES.to_vec(),
+                index_methods: match self.variant() {
+                    Variant::OpenGauss => vec!["BTREE", "HASH", "GIN", "GIST"],
+                    _ => vec!["BTREE", "HASH", "GIN", "GIST", "BRIN", "SPGIST"],
+                },
+                types: if self.oracle_mode() {
+                    POSTGRES_TYPES.iter().chain(PG_ORACLE_MODE_TYPES).cloned().collect()
+                } else {
+                    POSTGRES_TYPES.to_vec()
+                },
             },
             Family::Oracle | Family::Dameng => DesignCaps {
                 reorder: false,
@@ -194,6 +216,8 @@ impl Dialect {
 mod tests {
     use super::*;
 
+    use crate::Compat;
+
     #[test]
     fn caps_follow_family_and_version() {
         let mysql57 = Dialect::new(Variant::MySql).with_version(Some(50744)).design_caps();
@@ -209,6 +233,15 @@ mod tests {
         assert!(ora.on_update.is_empty());
         let varchar2 = ora.types.iter().find(|t| t.name == "VARCHAR2").unwrap();
         assert_eq!((varchar2.params, varchar2.char_unit), (TypeParams::Length, true));
+        let tidb = Dialect::new(Variant::TiDb).with_version(Some(80011)).design_caps();
+        assert!(tidb.reorder && !tidb.checks && !tidb.descending_index && tidb.index_methods == ["BTREE"]);
+        let ob = Dialect::new(Variant::OceanBase).with_version(Some(50725)).design_caps();
+        assert!(ob.checks && !ob.descending_index);
+        let kb = Dialect::new(Variant::Kingbase).with_compat(Compat::Oracle).design_caps();
+        assert!(kb.types.iter().any(|t| t.name == "VARCHAR2") && kb.types.iter().any(|t| t.name == "jsonb"));
+        let kb_pg = Dialect::new(Variant::Kingbase).design_caps();
+        assert!(!kb_pg.types.iter().any(|t| t.name == "VARCHAR2"));
+        assert!(!Dialect::new(Variant::OpenGauss).design_caps().index_methods.contains(&"BRIN"));
         let dm = Dialect::new(Variant::Dameng).design_caps();
         assert!(dm.types.iter().any(|t| t.name == "VARCHAR" && t.char_unit));
     }
```

- [ ] **Step 5: 修改 `crates/dblens-dialect/src/design/postgres.rs`**

```diff
--- a/crates/dblens-dialect/src/design/postgres.rs
+++ b/crates/dblens-dialect/src/design/postgres.rs
@@ -1,5 +1,5 @@
 //! PG 家族（PostgreSQL、金仓、openGauss、GaussDB、瀚高）的设计器 DDL（M3 设计文档 4.3 节）。
-//! 每项改动一条或几条语句，整体放在一个事务中执行。
+//! 每项改动一条或几条语句，整体放在一个事务中执行。openGauss 没有标识列，自增列用 serial 和序列。
 
 use dblens_core::Result;
 use dblens_core::schema::{Column, Index, Table};
@@ -7,11 +7,24 @@ use dblens_core::schema::{Column, Index, Table};
 use super::TableChange as C;
 use super::diff::{same_type, text};
 use super::sql::{DesignStatement, stmt};
-use crate::Dialect;
+use crate::{Dialect, Variant};
+
+/// openGauss 建表时自增列的类型：按整数宽度取 serial 系列，其他类型按 bigserial。
+fn serial_type(native: &str) -> &'static str {
+    match native.trim().to_ascii_lowercase().as_str() {
+        "smallint" | "int2" => "smallserial",
+        "integer" | "int" | "int4" => "serial",
+        _ => "bigserial",
+    }
+}
 
 impl Dialect {
     /// 新列的定义：类型、生成表达式 / 默认值 / 标识列、NOT NULL。注释另用 COMMENT ON。
     fn pg_column(&self, c: &Column) -> String {
+        if c.auto_increment && self.variant() == Variant::OpenGauss {
+            let s = format!("{} {}", self.ident(&c.name), serial_type(&c.data_type.native));
+            return if c.nullable { s } else { s + " NOT NULL" };
+        }
         let mut s = format!("{} {}", self.ident(&c.name), c.data_type.native);
         if let Some(g) = text(&c.generated) {
             s += &format!(" GENERATED ALWAYS AS ({g}) STORED");
@@ -102,7 +115,29 @@ impl Dialect {
                 None => out.push(alter("DROP DEFAULT".into())),
             }
         }
-        if from.auto_increment != to.auto_increment {
+        if from.auto_increment != to.auto_increment && self.variant() == Variant::OpenGauss {
+            // openGauss：建一个归这一列所有的序列作默认值，从现有最大值之后开始；取消自增只去掉默认值
+            if to.auto_increment {
+                let (schema, table) = q.rsplit_once('.').map_or(("", q), |(s, t)| (s, t));
+                let seq_name = format!("{}_{}_seq", table.trim_matches('"'), to.name);
+                let seq = if schema.is_empty() {
+                    self.ident(&seq_name)
+                } else {
+                    format!("{schema}.{}", self.ident(&seq_name))
+                };
+                out.push(format!("CREATE SEQUENCE {seq} OWNED BY {q}.{col}"));
+                out.push(format!(
+                    "SELECT pg_catalog.setval({}, COALESCE(MAX({col}), 0) + 1, false) FROM {q}",
+                    self.text_literal(&seq)?
+                ));
+                out.push(alter(format!(
+                    "SET DEFAULT pg_catalog.nextval({}::regclass)",
+                    self.text_literal(&seq)?
+                )));
+            } else {
+                out.push(alter("DROP DEFAULT".into()));
+            }
+        } else if from.auto_increment != to.auto_increment {
             out.push(alter(
                 if to.auto_increment { "ADD GENERATED BY DEFAULT AS IDENTITY" } else { "DROP IDENTITY" }
                     .into(),
```

- [ ] **Step 6: 修改 `crates/dblens-dialect/src/design/sql.rs`**

```diff
--- a/crates/dblens-dialect/src/design/sql.rs
+++ b/crates/dblens-dialect/src/design/sql.rs
@@ -7,7 +7,7 @@ use serde::Serialize;
 
 use super::diff::{is_expression, text};
 use super::{Diff, TableChange as C};
-use crate::{Dialect, Family};
+use crate::{Dialect, Family, Variant};
 
 /// 一条生成的语句。`changes` 是它对应的变更序号（一项变更也可能生成几条语句）。
 #[derive(Debug, Clone, PartialEq, Eq, Serialize)]
@@ -32,6 +32,12 @@ impl Dialect {
         if !self.design_caps().reorder && !diff.appended {
             out.push(msg!("design.unsupported.reorder"));
         }
+        // TiDB 的主键默认是聚簇索引，不能直接删除或更换
+        if self.variant() == Variant::TiDb
+            && diff.changes.iter().any(|c| matches!(c, C::DropPrimaryKey { .. }))
+        {
+            out.push(msg!("design.unsupported.clustered_pk"));
+        }
         for ch in &diff.changes {
             let C::AlterColumn { from, to } = ch else { continue };
             let generated = text(&from.generated) != text(&to.generated);
```

- [ ] **Step 7: 检查**

```bash
export CARGO_INCREMENTAL=0
cargo fmt --all
cargo clippy --config "source.mirror.registry='sparse+https://rsproxy.cn/index/'" --workspace --all-targets -- -D warnings
DBLENS_LANG=en cargo test --config "source.mirror.registry='sparse+https://rsproxy.cn/index/'" --workspace
```

Expected: 全部测试通过（dialect 设计器能力的断言增加）。

- [ ] **Step 8: 提交**

```bash
git add crates/dblens-core/locales/app.json crates/dblens-dialect/sql/oceanbase.sql crates/dblens-dialect/src/design/catalog.rs crates/dblens-dialect/src/design/postgres.rs crates/dblens-dialect/src/design/sql.rs crates/dblens-dialect/tests/snapshots/meta_snapshots__oceanbase.snap
git commit -m "feat(dialect): 国产库的表设计器" -m "能力表按变体与兼容模式给出；openGauss 的自增列用 serial 和序列；TiDB 更换主键时重建；OceanBase 的触发器定义按它的 ACTION_STATEMENT 读取。" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 9: 服务层：连接配置记录新建时选的产品

- `ProfileInput.product: Option<Variant>`（`#[serde(default)]`）；存储迁移版本 4：`ALTER TABLE profiles ADD COLUMN product TEXT`；读写和修改都带上 `product`。
- 实库测试中 `ProfileInput` 的字面量补 `product: None`。

**Files:**
- Modify: `crates/dblens-service/src/profile.rs`
- Modify: `crates/dblens-service/src/store.rs`
- Modify: `crates/dblens-service/tests/live.rs`

**Interfaces:**
- Produces：`dblens_service::ProfileInput.product: Option<Variant>`。

- [ ] **Step 1: 修改 `crates/dblens-service/tests/live.rs`**

```diff
--- a/crates/dblens-service/tests/live.rs
+++ b/crates/dblens-service/tests/live.rs
@@ -66,6 +66,7 @@ async fn setup(env: &str) -> Option<Live> {
         environment: Environment::Test,
         color: None,
         connect_timeout_secs: 10,
+        product: None,
     };
     let summary = service.test_connection(input.clone(), &params.password).await.unwrap();
     eprintln!("{env}：{} {:?} {:?}", summary.variant_name.current(), summary.version_code, summary.version);
```

- [ ] **Step 2: 修改 `crates/dblens-service/src/profile.rs`**

```diff
--- a/crates/dblens-service/src/profile.rs
+++ b/crates/dblens-service/src/profile.rs
@@ -62,6 +62,10 @@ pub struct ProfileInput {
     pub color: Option<String>,
     #[serde(default = "default_timeout")]
     pub connect_timeout_secs: u64,
+    /// 新建连接时选的产品（如人大金仓），只用于显示和默认值；连上后以识别出的变体为准。
+    /// 没有时按驱动显示。
+    #[serde(default)]
+    pub product: Option<Variant>,
 }
 
 fn default_timeout() -> u64 {
@@ -145,6 +149,9 @@ fn from_row(r: &Row) -> rusqlite::Result<Profile> {
     let options: String = r.get(7)?;
     let environment: String = r.get(8)?;
     let variant: Option<String> = r.get(11)?;
+    let product: Option<String> = r.get(12)?;
+    let parse =
+        |code: Option<String>| code.and_then(|v| serde_json::from_value(serde_json::Value::String(v)).ok());
     Ok(Profile {
         id: r.get(0)?,
         input: ProfileInput {
@@ -158,8 +165,9 @@ fn from_row(r: &Row) -> rusqlite::Result<Profile> {
             environment: Environment::parse(&environment),
             color: r.get(9)?,
             connect_timeout_secs: u64::try_from(r.get::<_, i64>(10)?).unwrap_or(0),
+            product: parse(product),
         },
-        variant: variant.and_then(|v| serde_json::from_value(serde_json::Value::String(v)).ok()),
+        variant: parse(variant),
     })
 }
 
@@ -170,7 +178,8 @@ fn now() -> String {
 impl Store {
     pub fn list_profiles(&self) -> Result<Vec<Profile>> {
         self.with(|c| {
-            let mut st = c.prepare(&format!("SELECT {COLUMNS}, variant FROM profiles ORDER BY name, id"))?;
+            let mut st =
+                c.prepare(&format!("SELECT {COLUMNS}, variant, product FROM profiles ORDER BY name, id"))?;
             st.query_map([], from_row)?.collect()
         })
     }
@@ -178,7 +187,7 @@ impl Store {
     pub fn profile(&self, id: &str) -> Result<Profile> {
         self.with(|c| {
             c.query_row(
-                &format!("SELECT {COLUMNS}, variant FROM profiles WHERE id = ?1"),
+                &format!("SELECT {COLUMNS}, variant, product FROM profiles WHERE id = ?1"),
                 params![id],
                 from_row,
             )
@@ -196,8 +205,8 @@ impl Store {
         self.with(|c| {
             c.execute(
                 &format!(
-                    "INSERT INTO profiles ({COLUMNS}, created_at, updated_at)
-                          VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?12)"
+                    "INSERT INTO profiles ({COLUMNS}, product, created_at, updated_at)
+                          VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?13, ?12, ?12)"
                 ),
                 params![
                     p.id,
@@ -211,7 +220,8 @@ impl Store {
                     i.environment.as_str(),
                     i.color,
                     i.connect_timeout_secs as i64,
-                    ts
+                    ts,
+                    i.product.map(variant_code)
                 ],
             )
         })?;
@@ -227,6 +237,7 @@ impl Store {
                 // SET 中引用的列是修改前的值：驱动、主机、端口都没变才保留识别出的变体
                 "UPDATE profiles SET name = ?2, driver = ?3, host = ?4, port = ?5, user = ?6, database = ?7,
                         options = ?8, environment = ?9, color = ?10, connect_timeout_secs = ?11, updated_at = ?12,
+                        product = ?13,
                         variant = CASE WHEN driver = ?3 AND host = ?4 AND port = ?5 THEN variant END
                  WHERE id = ?1",
                 params![
@@ -241,7 +252,8 @@ impl Store {
                     i.environment.as_str(),
                     i.color,
                     i.connect_timeout_secs as i64,
-                    now()
+                    now(),
+                    i.product.map(variant_code)
                 ],
             )
         })?;
@@ -289,6 +301,7 @@ pub(crate) mod tests {
             environment: Environment::Prod,
             color: Some("#d4380d".into()),
             connect_timeout_secs: 5,
+            product: None,
         }
     }
 
@@ -315,6 +328,21 @@ pub(crate) mod tests {
         assert!(matches!(s.update_profile(&a.id, input("x")), Err(Error::NotFound { .. })));
     }
 
+    #[test]
+    fn product_is_stored_and_updated() {
+        let s = Store::open_in_memory().unwrap();
+        let p = s.create_profile(ProfileInput { product: Some(Variant::OpenGauss), ..input("p") }).unwrap();
+        assert_eq!(s.profile(&p.id).unwrap().input.product, Some(Variant::OpenGauss));
+        let q =
+            s.update_profile(&p.id, ProfileInput { product: Some(Variant::Kingbase), ..input("p") }).unwrap();
+        assert_eq!(q.input.product, Some(Variant::Kingbase));
+        assert_eq!(s.update_profile(&p.id, input("p")).unwrap().input.product, None);
+        // 请求中没有 product 时按没有处理
+        let json =
+            serde_json::json!({"name": "x", "driver": "postgres", "host": "h", "port": 5432, "user": "u"});
+        assert_eq!(serde_json::from_value::<ProfileInput>(json).unwrap().product, None);
+    }
+
     #[test]
     fn detected_variant_survives_edits_but_not_address_changes() {
         let s = Store::open_in_memory().unwrap();
```

- [ ] **Step 3: 修改 `crates/dblens-service/src/store.rs`**

```diff
--- a/crates/dblens-service/src/store.rs
+++ b/crates/dblens-service/src/store.rs
@@ -73,6 +73,8 @@ const MIGRATIONS: &[&str] = &[
         updated_at INTEGER NOT NULL
     );
     CREATE INDEX favorites_parent ON favorites(parent_id, position);",
+    // 版本 4：新建连接时选的产品（国产库第一批），没有时按驱动显示
+    "ALTER TABLE profiles ADD COLUMN product TEXT;",
 ];
 
 pub(crate) fn db_err(e: rusqlite::Error) -> Error {
```

- [ ] **Step 4: 检查**

```bash
export CARGO_INCREMENTAL=0
cargo fmt --all
cargo clippy --config "source.mirror.registry='sparse+https://rsproxy.cn/index/'" --workspace --all-targets -- -D warnings
DBLENS_LANG=en cargo test --config "source.mirror.registry='sparse+https://rsproxy.cn/index/'" --workspace
```

Expected: 全部测试通过（service 新增 1 个）。

- [ ] **Step 5: 提交**

```bash
git add crates/dblens-service/src/profile.rs crates/dblens-service/src/store.rs crates/dblens-service/tests/live.rs
git commit -m "feat(service): 连接配置记录新建时选的产品" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 10: 服务层、接口层的实库测试覆盖国产库第一批

- `dblens-service/tests/live.rs`：`Live` 记下变体和兼容模式；`adapt`（openGauss 把 `INT GENERATED BY DEFAULT AS IDENTITY` 换成 `SERIAL`）；`domestic_flows!` 为 6 个目标生成 flow、workbench、cancel、objects、ops、export、editing、import、sql_file、design 十个测试；TiDB 的 SQL 文件不用存储过程，Oracle 兼容模式用 Oracle 风格的过程块和 `/`；设计器测试中 TiDB、OceanBase 不用降序索引，TiDB 不建触发器、自增值只要求大于原最大值。
- `dblens-api/tests/live.rs`：`flow(env, tls)`，TLS 只对开了 TLS 的 MySQL、MariaDB、PG 测试库要求；加 6 个目标。
- 测试库上的准备（不在仓库中）：openGauss 的 `admin` 授予 MONADMIN（`ALTER USER admin MONADMIN`）。

**Files:**
- Modify: `crates/dblens-api/tests/live.rs`
- Modify: `crates/dblens-service/tests/live.rs`

**Interfaces:**
- Consumes：任务 2–9 的全部改动。

- [ ] **Step 1: 修改 `crates/dblens-api/tests/live.rs`**

```diff
--- a/crates/dblens-api/tests/live.rs
+++ b/crates/dblens-api/tests/live.rs
@@ -1,7 +1,9 @@
 //! 接口的实库测试。设置对应的环境变量时运行，否则直接通过：
-//! DBLENS_TEST_MYSQL_URL、DBLENS_TEST_MARIADB_URL、DBLENS_TEST_PG_URL、DBLENS_TEST_ORACLE_URL、DBLENS_TEST_DM_URL。
+//! DBLENS_TEST_MYSQL_URL、DBLENS_TEST_MARIADB_URL、DBLENS_TEST_PG_URL、DBLENS_TEST_ORACLE_URL、DBLENS_TEST_DM_URL；
+//! 国产库第一批：DBLENS_TEST_TIDB_URL、DBLENS_TEST_OCEANBASE_URL、DBLENS_TEST_OPENGAUSS_URL、
+//! DBLENS_TEST_OPENGAUSS_A_URL、DBLENS_TEST_KINGBASE_URL、DBLENS_TEST_KINGBASE_PG_URL。
 //!
-//! 通过 REST 保存连接配置（MySQL 系和 PG 走 TLS）、测试连接、浏览元数据、打开会话，
+//! 通过 REST 保存连接配置（MySQL、MariaDB、PG 的测试库开了 TLS，要求走 TLS）、测试连接、浏览元数据、打开会话，
 //! 再通过 WebSocket 分批取回 900 行结果。
 
 use std::sync::Arc;
@@ -42,7 +44,8 @@ async fn call(app: &Router, method: &str, uri: &str, body: Option<Value>) -> (St
     (status, body)
 }
 
-async fn flow(env: &str) {
+/// `tls`：测试库开了 TLS，连接时要求走 TLS（国产库的测试库没开）。
+async fn flow(env: &str, tls: bool) {
     let Ok(url) = std::env::var(env) else { return };
     let (driver, params) = ConnectParams::from_url(&url).unwrap();
     let dir = tempfile::tempdir().unwrap();
@@ -69,7 +72,7 @@ async fn flow(env: &str) {
         .filter(|(k, _)| !LIB_OPTIONS.contains(&k.as_str()))
         .map(|(k, v)| (k.clone(), json!(v)))
         .collect();
-    if matches!(driver, DriverId::Mysql | DriverId::Postgres) {
+    if tls {
         options.insert("tls".into(), json!("require"));
     }
     let profile = json!({
@@ -178,25 +181,55 @@ async fn flow(env: &str) {
 
 #[tokio::test]
 async fn mysql() {
-    flow("DBLENS_TEST_MYSQL_URL").await;
+    flow("DBLENS_TEST_MYSQL_URL", true).await;
 }
 
 #[tokio::test]
 async fn mariadb() {
-    flow("DBLENS_TEST_MARIADB_URL").await;
+    flow("DBLENS_TEST_MARIADB_URL", true).await;
 }
 
 #[tokio::test]
 async fn postgres() {
-    flow("DBLENS_TEST_PG_URL").await;
+    flow("DBLENS_TEST_PG_URL", true).await;
 }
 
 #[tokio::test]
 async fn oracle() {
-    flow("DBLENS_TEST_ORACLE_URL").await;
+    flow("DBLENS_TEST_ORACLE_URL", false).await;
 }
 
 #[tokio::test]
 async fn dameng() {
-    flow("DBLENS_TEST_DM_URL").await;
+    flow("DBLENS_TEST_DM_URL", false).await;
+}
+
+#[tokio::test]
+async fn tidb() {
+    flow("DBLENS_TEST_TIDB_URL", false).await;
+}
+
+#[tokio::test]
+async fn oceanbase() {
+    flow("DBLENS_TEST_OCEANBASE_URL", false).await;
+}
+
+#[tokio::test]
+async fn opengauss() {
+    flow("DBLENS_TEST_OPENGAUSS_URL", false).await;
+}
+
+#[tokio::test]
+async fn opengauss_a() {
+    flow("DBLENS_TEST_OPENGAUSS_A_URL", false).await;
+}
+
+#[tokio::test]
+async fn kingbase() {
+    flow("DBLENS_TEST_KINGBASE_URL", false).await;
+}
+
+#[tokio::test]
+async fn kingbase_pg() {
+    flow("DBLENS_TEST_KINGBASE_PG_URL", false).await;
 }
```

- [ ] **Step 2: 修改 `crates/dblens-service/tests/live.rs`**

```diff
--- a/crates/dblens-service/tests/live.rs
+++ b/crates/dblens-service/tests/live.rs
@@ -1,5 +1,7 @@
 //! 服务层的实库测试。设置对应的环境变量时运行，否则直接通过：
-//! DBLENS_TEST_MYSQL_URL、DBLENS_TEST_MARIADB_URL、DBLENS_TEST_PG_URL、DBLENS_TEST_ORACLE_URL、DBLENS_TEST_DM_URL。
+//! DBLENS_TEST_MYSQL_URL、DBLENS_TEST_MARIADB_URL、DBLENS_TEST_PG_URL、DBLENS_TEST_ORACLE_URL、DBLENS_TEST_DM_URL；
+//! 国产库第一批（文件末尾的 `domestic_flows!`）：DBLENS_TEST_TIDB_URL、DBLENS_TEST_OCEANBASE_URL、
+//! DBLENS_TEST_OPENGAUSS_URL、DBLENS_TEST_OPENGAUSS_A_URL、DBLENS_TEST_KINGBASE_URL、DBLENS_TEST_KINGBASE_PG_URL。
 //!
 //! 走一遍完整流程：保存连接配置（密码放本地密码库）→ 测试连接 → 打开会话执行 → 浏览元数据 →
 //! 会话中执行 DDL 后元数据缓存失效 → 取消长查询；查询工作台用到的执行计划、出错位置、切换 schema。
@@ -12,7 +14,7 @@ use dblens_core::schema::{CheckConstraint, Column, Index, IndexColumn, PrimaryKe
 use dblens_core::{ColumnType, Error, LogicalType, Value};
 use dblens_dialect::design::{Renames, TableChange, diff_table};
 use dblens_dialect::meta::{MetaArgs, MetaKey};
-use dblens_dialect::{DdlKind, Direction, RowChange, WriteMode};
+use dblens_dialect::{Compat, DdlKind, Direction, RowChange, Variant, WriteMode};
 use dblens_driver::{ConnectParams, DriverId};
 use dblens_service::{
     ChangeSet, ColumnMapping, DEFAULT_BATCH, DataOrder, DefaultPolicy, DesignRequest, DriverRegistry,
@@ -27,6 +29,9 @@ struct Live {
     service: Arc<Service>,
     profile_id: String,
     driver: DriverId,
+    /// 识别出的变体与兼容模式：兼容库沿用所属家族的测试语句，个别写法由 `adapt` 改写。
+    variant: Variant,
+    compat: Compat,
     /// 测试表所在的 schema。
     schema: String,
 }
@@ -71,7 +76,8 @@ async fn setup(env: &str) -> Option<Live> {
     let summary = service.test_connection(input.clone(), &params.password).await.unwrap();
     eprintln!("{env}：{} {:?} {:?}", summary.variant_name.current(), summary.version_code, summary.version);
     let profile = service.create_profile(input, Some(&params.password)).unwrap();
-    Some(Live { _dir: dir, service, profile_id: profile.id, driver, schema })
+    let (variant, compat) = (summary.variant, summary.compat);
+    Some(Live { _dir: dir, service, profile_id: profile.id, driver, variant, compat, schema })
 }
 
 impl Live {
@@ -82,8 +88,17 @@ impl Live {
         }
     }
 
+    /// 把家族通用的测试语句改成兼容库的写法：openGauss 没有 IDENTITY 列，改用 serial。
+    fn adapt(&self, sql: &str) -> String {
+        match self.variant {
+            Variant::OpenGauss => sql.replace("INT GENERATED BY DEFAULT AS IDENTITY", "SERIAL"),
+            _ => sql.to_string(),
+        }
+    }
+
     /// 提交请求并取完全部事件，不检查失败。
-    async fn submit(&self, id: SessionId, req: ExecRequest) -> Vec<SessionEvent> {
+    async fn submit(&self, id: SessionId, mut req: ExecRequest) -> Vec<SessionEvent> {
+        req.sql = self.adapt(&req.sql);
         let sql = req.sql.clone();
         self.service.sessions.submit(id, req).await.unwrap();
         let mut events = Vec::new();
@@ -1018,11 +1033,22 @@ async fn sql_file_flow(env: &str) {
     let Some(live) = setup(env).await else { return };
     let s = &live.service;
     let script = match live.driver {
+        // TiDB 没有存储过程
+        DriverId::Mysql if live.variant == Variant::TiDb => {
+            "DROP TABLE IF EXISTS m2b_f;\nCREATE TABLE m2b_f (a INT);\nINSERT INTO m2b_f VALUES (1);\n\
+             INSERT INTO m2b_f VALUES (2);\nINSERT INTO m2b_f VALUES (3);\n"
+        }
         DriverId::Mysql => {
             "DROP TABLE IF EXISTS m2b_f;\nDROP PROCEDURE IF EXISTS m2b_p;\nCREATE TABLE m2b_f (a INT);\n\
              DELIMITER $$\nCREATE PROCEDURE m2b_p() BEGIN INSERT INTO m2b_f VALUES (1); INSERT INTO m2b_f VALUES (2); END$$\n\
              DELIMITER ;\nCALL m2b_p();\nINSERT INTO m2b_f VALUES (3);\nDROP PROCEDURE m2b_p;\n"
         }
+        // 人大金仓、openGauss 的 Oracle 兼容模式：Oracle 风格的过程块，以独占一行的 `/` 结束
+        DriverId::Postgres if live.compat == Compat::Oracle => {
+            "DROP TABLE IF EXISTS m2b_f;\nCREATE TABLE m2b_f (a INT);\n\
+             CREATE OR REPLACE PROCEDURE m2b_p AS\nBEGIN\n  INSERT INTO m2b_f VALUES (1);\n  INSERT INTO m2b_f VALUES (2);\nEND;\n/\n\
+             CALL m2b_p();\nINSERT INTO m2b_f VALUES (3);\nDROP PROCEDURE m2b_p;\n"
+        }
         DriverId::Postgres => {
             "DROP TABLE IF EXISTS m2b_f;\nCREATE TABLE m2b_f (a INT);\n\
              CREATE OR REPLACE FUNCTION m2b_p() RETURNS void AS $$ BEGIN INSERT INTO m2b_f VALUES (1); INSERT INTO m2b_f VALUES (2); END; $$ LANGUAGE plpgsql;\n\
@@ -1216,7 +1242,11 @@ async fn design_flow(env: &str) {
     d.columns.push(extra);
     d.indexes = vec![Index {
         name: n("ix_m3_remark"),
-        columns: vec![IndexColumn { name: n("remark"), descending: true }],
+        // TiDB、OceanBase 不建降序索引（design_caps 的 descending_index）
+        columns: vec![IndexColumn {
+            name: n("remark"),
+            descending: !matches!(live.variant, Variant::TiDb | Variant::OceanBase),
+        }],
         unique: false,
         method: None,
     }];
@@ -1243,6 +1273,8 @@ async fn design_flow(env: &str) {
     assert!(left.is_empty(), "{env}：读回后仍有差异 {left:?}");
 
     let trigger: &[&str] = match live.driver {
+        // TiDB 没有触发器
+        DriverId::Mysql if live.variant == Variant::TiDb => &[],
         DriverId::Mysql => {
             &["CREATE TRIGGER m3_trg BEFORE INSERT ON m3_t FOR EACH ROW SET NEW.remark = 'trg'"]
         }
@@ -1325,7 +1357,7 @@ async fn design_flow(env: &str) {
         "{env}"
     );
     assert_eq!(after.checks.len(), draft.checks.len(), "{env}");
-    assert_eq!(after.triggers.len(), 1, "{env}：触发器重建");
+    assert_eq!(after.triggers.len(), trigger.len().min(1), "{env}：触发器重建");
     let one = |sql: String| async move { text(&first_value(&live.run(id, &sql).await)) };
     assert_eq!(one("SELECT COUNT(*) FROM m3_t".into()).await.as_deref(), Some("3"), "{env}");
     assert_eq!(
@@ -1335,8 +1367,17 @@ async fn design_flow(env: &str) {
     );
     // 自增接续、触发器生效
     live.run(id, "INSERT INTO m3_t (code, amount) VALUES ('d', 4)").await;
-    assert_eq!(one("SELECT MAX(id) FROM m3_t".into()).await.as_deref(), Some("4"), "{env}");
-    assert_eq!(one("SELECT remark FROM m3_t WHERE code = 'd'".into()).await.as_deref(), Some("trg"), "{env}");
+    let max: i64 = one("SELECT MAX(id) FROM m3_t".into()).await.unwrap().parse().unwrap();
+    // TiDB 按批分配自增值，新值大于原有的最大值即可
+    if live.variant == Variant::TiDb {
+        assert!(max > 3, "{env}：{max}");
+    } else {
+        assert_eq!(max, 4, "{env}");
+    }
+    if !trigger.is_empty() {
+        let remark = one("SELECT remark FROM m3_t WHERE code = 'd'".into()).await;
+        assert_eq!(remark.as_deref(), Some("trg"), "{env}");
+    }
     // 授权恢复（MySQL 的授权按表名记录，不受影响）
     if live.driver != DriverId::Mysql {
         let profile = s.profile(&live.profile_id).unwrap();
@@ -1394,3 +1435,66 @@ async fn oracle_design() {
 async fn dameng_design() {
     design_flow("DBLENS_TEST_DM_URL").await;
 }
+
+/// MySQL 驱动的取消测试用的耗时查询（SLEEP() 被 KILL QUERY 打断时返回 1 而不报错）。
+const MYSQL_SLOW: &str = "SELECT COUNT(*) FROM information_schema.columns a, information_schema.columns b, information_schema.columns c";
+
+/// 国产库第一批：与 MySQL、PostgreSQL 走同样的流程。`$ops` 是允许失败的运维查询。
+macro_rules! domestic_flows {
+    ($($name:ident: $env:literal, slow = $slow:expr, ops = $ops:expr;)*) => {$(
+        mod $name {
+            use super::*;
+
+            #[tokio::test]
+            async fn flow() {
+                full_flow($env).await;
+            }
+            #[tokio::test]
+            async fn workbench() {
+                workbench_flow($env).await;
+            }
+            #[tokio::test]
+            async fn cancel() {
+                cancel_flow($env, $slow).await;
+            }
+            #[tokio::test]
+            async fn objects() {
+                objects_flow($env).await;
+            }
+            #[tokio::test]
+            async fn ops() {
+                ops_flow($env, $ops).await;
+            }
+            #[tokio::test]
+            async fn export() {
+                export_flow($env).await;
+            }
+            #[tokio::test]
+            async fn editing() {
+                editing_flow($env).await;
+            }
+            #[tokio::test]
+            async fn import() {
+                import_flow($env).await;
+            }
+            #[tokio::test]
+            async fn sql_file() {
+                sql_file_flow($env).await;
+            }
+            #[tokio::test]
+            async fn design() {
+                design_flow($env).await;
+            }
+        }
+    )*};
+}
+
+domestic_flows! {
+    tidb: "DBLENS_TEST_TIDB_URL", slow = MYSQL_SLOW, ops = &[];
+    oceanbase: "DBLENS_TEST_OCEANBASE_URL", slow = MYSQL_SLOW, ops = &[];
+    opengauss: "DBLENS_TEST_OPENGAUSS_URL", slow = "SELECT pg_sleep(20)", ops = &[];
+    opengauss_a: "DBLENS_TEST_OPENGAUSS_A_URL", slow = "SELECT pg_sleep(20)", ops = &[];
+    // 人大金仓：与 PG 测试库一样没有加载 sys_stat_statements；开发版授权最多 10 个连接，要串行运行
+    kingbase: "DBLENS_TEST_KINGBASE_URL", slow = "SELECT pg_sleep(20)", ops = &["slow_sql"];
+    kingbase_pg: "DBLENS_TEST_KINGBASE_PG_URL", slow = "SELECT pg_sleep(20)", ops = &["slow_sql"];
+}
```

- [ ] **Step 3: 检查**

```bash
export CARGO_INCREMENTAL=0
cargo fmt --all
cargo clippy --config "source.mirror.registry='sparse+https://rsproxy.cn/index/'" --workspace --all-targets -- -D warnings
DBLENS_LANG=en cargo test --config "source.mirror.registry='sparse+https://rsproxy.cn/index/'" --workspace
```

实库测试：在上面的 `cargo test` 前以内联环境变量给出原有 5 个库的 `DBLENS_TEST_MYSQL_URL`、`DBLENS_TEST_MARIADB_URL`、`DBLENS_TEST_PG_URL`、`DBLENS_TEST_ORACLE_URL`、`DBLENS_TEST_DM_URL`，以及第一批的 `DBLENS_TEST_TIDB_URL`、`DBLENS_TEST_OCEANBASE_URL`（用户名写成 `admin%40test`）、`DBLENS_TEST_OPENGAUSS_URL`、`DBLENS_TEST_OPENGAUSS_A_URL`（库 `dblens_test_a`）、`DBLENS_TEST_OPENGAUSS_SHA_URL`（账号 `admin_sha`）、`DBLENS_TEST_KINGBASE_URL`（54321）、`DBLENS_TEST_KINGBASE_PG_URL`（54322）。新目标的密码在测试虚拟机 root 的 `/root/dblens-test/creds.env`，用 `ssh … 'sudo cat …'` 读进当前 shell 的变量，不写进任何文件。
- 人大金仓的开发版授权最多 10 个连接：它的测试用 `-- kingbase --test-threads=1` 单独串行运行。
- 达梦的测试与其他测试同在一个进程里运行时偶发"网络通讯失败"，失败时用 `-- dameng --test-threads=1` 单独重跑。

Expected: 全部测试通过：服务层实库测试新增 60 个，接口层新增 6 个。

- [ ] **Step 4: 提交**

```bash
git add crates/dblens-api/tests/live.rs crates/dblens-service/tests/live.rs
git commit -m "test: 服务层、接口层的实库测试覆盖国产库第一批" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 11: 前端：新建连接按产品选；编辑器按兼容模式切换

- 后端 `SessionInfo.compat`。
- `shared/api.ts`：`Compat`；`ProfileInput.product`；`ServerSummary` 的 `compat`、`mode`、`unadapted_mode`；`SessionInfo.compat`；`TableStorage.clustered`。
- `features/connections/products.ts`（新）：9 个产品、驱动和默认端口；`productOf`（不认识或与驱动对不上的值按驱动）、`productLabel`、`productNote`。
- `form.ts`：表单带 `product`；`emptyForm(product)`、`switchProduct`（驱动跟着换，端口还是上一个产品的默认值时换成新产品的）、`fromProfile` / `toProfileInput` 带产品；`serverNotes`（兼容模式、未适配的模式、所选产品与识别结果不一致；选标准库时不算不一致）。
- `ConnectionDialog.tsx`：左侧列出产品（图标按产品），说明用产品的，测试成功时显示 `serverNotes`。
- 编辑器：`sqlDialect(driver, variant, compat)` 与 `editorLanguage(driver, compat)`：PG 家族的 Oracle 兼容模式按 PL/SQL（兼容档）；`QueryView`、补全和语法检查的上下文带 `compat`。
- 数据标签：Oracle 兼容模式下工具栏提示"此库空串按 NULL 存储"；设计器选项页显示聚簇主键。
- 词条：`connections.product.*`、`connections.dialog.modeOracle` / `modeNative` / `modeUnadapted` / `productMismatch`，`driverType` 改为"数据库"；`design.option.clustered`；`editor.data.edit.emptyIsNull`。

**Files:**
- Modify: `crates/dblens-service/src/session.rs`
- Modify: `web/src/features/connections/ConnectionDialog.tsx`
- Modify: `web/src/features/connections/form.test.ts`
- Modify: `web/src/features/connections/form.ts`
- Create: `web/src/features/connections/products.ts`
- Modify: `web/src/features/data/DataView.tsx`
- Modify: `web/src/features/design/OptionsPane.tsx`
- Modify: `web/src/features/editor/QueryView.tsx`
- Modify: `web/src/features/editor/dialect.ts`
- Modify: `web/src/features/editor/runner.test.ts`
- Modify: `web/src/features/sql/checker.ts`
- Modify: `web/src/features/sql/dialects.test.ts`
- Modify: `web/src/features/sql/dialects.ts`
- Modify: `web/src/features/sql/provider.ts`
- Modify: `web/src/locales/en/connections.json`
- Modify: `web/src/locales/en/design.json`
- Modify: `web/src/locales/en/editor.json`
- Modify: `web/src/locales/zh-CN/connections.json`
- Modify: `web/src/locales/zh-CN/design.json`
- Modify: `web/src/locales/zh-CN/editor.json`
- Modify: `web/src/shared/api.ts`

**Interfaces:**
- Consumes：任务 2 的 `ServerSummary`、任务 9 的 `product`。
- Produces：`features/connections/products.ts` 的 `ProductId`、`PRODUCTS`、`product`、`productOf`、`productLabel`、`productNote`；`form.ts` 的 `switchProduct`、`serverNotes`。

- [ ] **Step 1: 修改 `web/src/features/connections/form.test.ts`**

```diff
--- a/web/src/features/connections/form.test.ts
+++ b/web/src/features/connections/form.test.ts
@@ -8,8 +8,10 @@ import {
   driverLabel,
   emptyForm,
   fromProfile,
+  serverNotes,
   serverText,
   switchDriver,
+  switchProduct,
   toProfileInput,
   validate,
 } from './form'
@@ -44,6 +46,7 @@ describe('连接表单', () => {
       environment: 'none',
       color: null,
       connect_timeout_secs: 10,
+      product: 'postgres',
     })
     const o = toProfileInput({ ...emptyForm('oracle'), name: 'o', user: 'u', oracleSid: 'ORCL', tls: 'require' })
     expect(o.options).toEqual({ sid: 'ORCL' })
@@ -84,8 +87,44 @@ describe('连接表单', () => {
     expect(validate({ ...ok, extra: 'a=1\nbad' }).extra).toContain('bad')
   })
 
+  it('按产品选：驱动和默认端口跟着换，手改过的端口保留；配置记下产品，旧配置按驱动还原', () => {
+    let f = switchProduct(emptyForm(), 'tidb')
+    expect([f.driver, f.port]).toEqual(['mysql', 4000])
+    f = switchProduct(f, 'kingbase')
+    expect([f.driver, f.port]).toEqual(['postgres', 54321])
+    f = switchProduct({ ...f, port: 6000 }, 'opengauss')
+    expect([f.driver, f.port, toProfileInput(f).product]).toEqual(['postgres', 6000, 'opengauss'])
+    const p = { id: 'p', name: 'n', driver: 'mysql' as const, host: 'h', port: 2881, user: 'u' }
+    expect(fromProfile({ ...p, product: 'oceanbase' }, false).product).toBe('oceanbase')
+    expect(fromProfile(p, false).product).toBe('mysql')
+    // 与驱动对不上的值按驱动处理
+    expect(fromProfile({ ...p, product: 'kingbase' }, false).product).toBe('mysql')
+  })
+
+  it('测试连接的说明：兼容模式、未适配的模式、所选产品与识别结果不一致', () => {
+    const s = { product: 'PostgreSQL', version: 'v', variant: 'kingbase', variant_name: '人大金仓', version_code: 120100 }
+    const oracle = { ...s, compat: 'oracle' as const, mode: 'oracle', unadapted_mode: false }
+    expect(serverNotes(oracle, 'kingbase')).toEqual([{ text: expect.stringContaining('兼容模式：oracle'), warn: false }])
+    // 选标准库而识别出兼容库不算不一致
+    expect(serverNotes(oracle, 'postgres')).toHaveLength(1)
+    const b = { ...s, variant: 'opengauss', variant_name: 'openGauss', compat: 'native' as const, mode: 'B', unadapted_mode: true }
+    expect(serverNotes(b, 'kingbase').map((n) => n.warn)).toEqual([true, true])
+    expect(serverNotes(b, 'kingbase')[1].text).toBe('选的是 人大金仓，实际识别为 openGauss，按识别结果处理')
+    const pg = { ...s, variant: 'postgres', variant_name: 'PostgreSQL', compat: 'native' as const, mode: null, unadapted_mode: false }
+    expect(serverNotes(pg, 'postgres')).toEqual([])
+  })
+
   it('服务器版本说明不重复产品名', () => {
-    const s = (product: string, version: string) => ({ product, version, variant: 'x', variant_name: 'x', version_code: null })
+    const s = (product: string, version: string) => ({
+      product,
+      version,
+      variant: 'x',
+      variant_name: 'x',
+      version_code: null,
+      compat: 'native' as const,
+      mode: null,
+      unadapted_mode: false,
+    })
     expect(serverText(s('PostgreSQL', 'PostgreSQL 18.6 on x86_64'))).toBe('PostgreSQL 18.6 on x86_64')
     expect(serverText(s('MySQL', '9.7.4'))).toBe('MySQL 9.7.4')
   })
```

- [ ] **Step 2: 修改 `web/src/features/editor/runner.test.ts`**

```diff
--- a/web/src/features/editor/runner.test.ts
+++ b/web/src/features/editor/runner.test.ts
@@ -28,6 +28,7 @@ const session: SessionInfo = {
   id: 7,
   profile_id: 'p',
   variant: 'postgres',
+  compat: 'native',
   autocommit: true,
   in_transaction: false,
   busy: false,
```

- [ ] **Step 3: 修改 `web/src/features/sql/dialects.test.ts`**

```diff
--- a/web/src/features/sql/dialects.test.ts
+++ b/web/src/features/sql/dialects.test.ts
@@ -20,6 +20,12 @@ describe('sqlDialect', () => {
     expect(sqlDialect(driver, variant)).toMatchObject({ dialect, strict })
   })
 
+  it('人大金仓、openGauss 的 Oracle 兼容模式按 PL/SQL 处理（兼容档）', () => {
+    expect(sqlDialect('postgres', 'kingbase', 'oracle')).toEqual({ dialect: 'plsql', strict: false, grammar: 'Oracle' })
+    expect(sqlDialect('postgres', 'opengauss', 'native')).toMatchObject({ dialect: 'postgres' })
+    expect(sqlDialect('mysql', 'tidb', 'oracle')).toMatchObject({ dialect: 'mysql' })
+  })
+
   it('变体未知时按驱动选解析器，按兼容档处理', () => {
     expect(sqlDialect('oracle', null)).toEqual({ dialect: 'plsql', strict: false, grammar: 'Oracle' })
     expect(sqlDialect('mysql')).toEqual({ dialect: 'mysql', strict: false, grammar: 'MySQL' })
```

- [ ] **Step 4: 修改 `crates/dblens-service/src/session.rs`**

```diff
--- a/crates/dblens-service/src/session.rs
+++ b/crates/dblens-service/src/session.rs
@@ -10,7 +10,7 @@ use std::sync::{Arc, Mutex};
 use std::time::{Instant, SystemTime, UNIX_EPOCH};
 
 use dblens_core::{Error, Msg, Result, Row, Value, msg};
-use dblens_dialect::{ChangeStatement, Dialect, SqlStatement, Variant, may_change_schema};
+use dblens_dialect::{ChangeStatement, Compat, Dialect, SqlStatement, Variant, may_change_schema};
 use dblens_driver::{CancelHandle, ColumnMeta, Connection, ExecEvent};
 use serde::{Deserialize, Serialize};
 use tokio::sync::{mpsc, oneshot};
@@ -77,6 +77,8 @@ pub struct SessionInfo {
     pub id: SessionId,
     pub profile_id: String,
     pub variant: Variant,
+    /// 兼容模式（人大金仓、openGauss 可能是 Oracle 模式），编辑器据此选语法和拆分规则。
+    pub compat: Compat,
     pub autocommit: bool,
     /// 手动提交模式下，是否有尚未提交的修改。
     pub in_transaction: bool,
@@ -275,6 +277,7 @@ impl SessionManager {
             id,
             profile_id: profile_id.to_string(),
             variant: dialect.variant(),
+            compat: dialect.compat(),
             autocommit: true,
             in_transaction: false,
             busy: false,
```

- [ ] **Step 5: 修改 `web/src/features/connections/ConnectionDialog.tsx`**

```diff
--- a/web/src/features/connections/ConnectionDialog.tsx
+++ b/web/src/features/connections/ConnectionDialog.tsx
@@ -13,7 +13,6 @@ import DbIcon from '../../shared/DbIcon'
 import type { DriverId, Environment, ServerSummary } from '../../shared/api'
 import {
   type ConnectionForm,
-  DRIVER_IDS,
   databaseLabel,
   driverLabel,
   driverNote,
@@ -21,10 +20,12 @@ import {
   fromProfile,
   serverText,
   supportsTls,
-  switchDriver,
+  serverNotes,
+  switchProduct,
   toProfileInput,
   validate,
 } from './form'
+import { PRODUCTS, productLabel, productNote } from './products'
 import { withVault } from './vault'
 
 type Errors = ReturnType<typeof validate>
@@ -272,25 +273,25 @@ export default function ConnectionDialog() {
           <div className="muted" style={{ marginBottom: 6 }}>
             {t('connections.dialog.driverType')}
           </div>
-          {DRIVER_IDS.map((id) => (
+          {PRODUCTS.map((p) => (
             <div
-              key={id}
+              key={p.id}
               className="row"
-              onClick={() => setForm((f) => switchDriver(f, id))}
+              onClick={() => setForm((f) => switchProduct(f, p.id))}
               style={{
                 gap: 8,
-                padding: '6px 10px',
+                padding: '5px 10px',
                 borderRadius: 4,
                 cursor: 'pointer',
-                background: form.driver === id ? 'var(--sel)' : undefined,
+                background: form.product === p.id ? 'var(--sel)' : undefined,
               }}
             >
-              <DbIcon driver={id} size={16} />
-              {driverLabel(id)}
+              <DbIcon driver={p.driver} variant={p.id} size={16} />
+              {productLabel(p.id)}
             </div>
           ))}
           <div className="muted" style={{ marginTop: 12, lineHeight: 1.5 }}>
-            {driverNote(form.driver)}
+            {productNote(form.product) || driverNote(form.driver)}
           </div>
         </div>
         <Form className="grow" layout="horizontal" labelCol={{ flex: '100px' }} labelAlign="left">
@@ -309,6 +310,14 @@ export default function ConnectionDialog() {
                 showIcon
                 icon={<IconCircleCheck size={16} />}
                 message={t('connections.dialog.testOk', { server: serverText(result.ok), variant: result.ok.variant_name })}
+                description={
+                  serverNotes(result.ok, form.product).length > 0 &&
+                  serverNotes(result.ok, form.product).map((n) => (
+                    <div key={n.text} style={n.warn ? { color: 'var(--warn)' } : undefined}>
+                      {n.text}
+                    </div>
+                  ))
+                }
               />
             ) : (
               <Alert type="error" showIcon message={result.error} />
```

- [ ] **Step 6: 修改 `web/src/features/connections/form.ts`**

```diff
--- a/web/src/features/connections/form.ts
+++ b/web/src/features/connections/form.ts
@@ -2,11 +2,14 @@
 
 import type { DriverId, Environment, Profile, ProfileInput, ServerSummary } from '../../shared/api'
 import { t } from '../../shared/i18n'
+import { type ProductId, product, productLabel, productOf } from './products'
 
 export type TlsMode = 'disable' | 'require' | 'verify'
 
 export interface ConnectionForm {
   name: string
+  /** 选的产品；驱动由产品决定。 */
+  product: ProductId
   driver: DriverId
   host: string
   port: number
@@ -52,12 +55,14 @@ export function supportsTls(driver: DriverId): boolean {
   return driver === 'mysql' || driver === 'postgres'
 }
 
-export function emptyForm(driver: DriverId = 'mysql'): ConnectionForm {
+export function emptyForm(id: ProductId = 'mysql'): ConnectionForm {
+  const p = product(id)
   return {
     name: '',
-    driver,
+    product: id,
+    driver: p.driver,
     host: '127.0.0.1',
-    port: DEFAULT_PORTS[driver],
+    port: p.port,
     user: '',
     password: '',
     savePassword: true,
@@ -78,6 +83,13 @@ export function switchDriver(f: ConnectionForm, driver: DriverId): ConnectionFor
   return { ...f, driver, port, tls: supportsTls(driver) ? f.tls : 'disable', tlsCa: supportsTls(driver) ? f.tlsCa : '' }
 }
 
+/** 换产品：驱动跟着换；端口还是上一个产品的默认值，就换成新产品的默认值。 */
+export function switchProduct(f: ConnectionForm, id: ProductId): ConnectionForm {
+  const next = product(id)
+  const port = f.port === product(f.product).port ? next.port : f.port
+  return { ...switchDriver(f, next.driver), product: id, port }
+}
+
 const KNOWN_OPTIONS = ['tls', 'tls_ca', 'sid']
 
 export function fromProfile(p: Profile, hasPassword: boolean): ConnectionForm {
@@ -89,6 +101,7 @@ export function fromProfile(p: Profile, hasPassword: boolean): ConnectionForm {
   const tls = opts.tls === 'require' || opts.tls === 'verify' ? opts.tls : 'disable'
   return {
     name: p.name,
+    product: productOf(p.driver, p.product),
     driver: p.driver,
     host: p.host,
     port: p.port,
@@ -128,6 +141,7 @@ export function toProfileInput(f: ConnectionForm): ProfileInput {
     environment: f.environment,
     color: f.color.trim() || null,
     connect_timeout_secs: f.timeoutSecs,
+    product: f.product,
   }
 }
 
@@ -157,3 +171,18 @@ export function validate(f: ConnectionForm): Partial<Record<keyof ConnectionForm
 export function serverText(s: ServerSummary): string {
   return s.version.startsWith(s.product) ? s.version : `${s.product} ${s.version}`
 }
+
+/** 测试连接成功后另外显示的说明：兼容模式、未适配的模式、所选产品与识别结果不一致。 */
+export function serverNotes(s: ServerSummary, chosen: ProductId): { text: string; warn: boolean }[] {
+  const out: { text: string; warn: boolean }[] = []
+  if (s.mode) {
+    const key = s.unadapted_mode ? 'connections.dialog.modeUnadapted' : s.compat === 'oracle' ? 'connections.dialog.modeOracle' : 'connections.dialog.modeNative'
+    out.push({ text: t(key, { mode: s.mode }), warn: s.unadapted_mode })
+  }
+  const detected = productOf(product(chosen).driver, s.variant)
+  // 选的是标准库（如 PostgreSQL）而识别出兼容库时不算不一致：较早的连接都按驱动选的
+  if (chosen !== detected && chosen !== product(chosen).driver) {
+    out.push({ text: t('connections.dialog.productMismatch', { chosen: productLabel(chosen), detected: s.variant_name }), warn: true })
+  }
+  return out
+}
```

- [ ] **Step 7: 新建 `web/src/features/connections/products.ts`**

```ts
// 新建连接时按产品选（国产库第一批设计 4.3 节）：每个产品对应一个驱动、一个默认端口。
// 选择记在连接配置的 `product` 上；连上后以识别出的变体为准。

import type { DriverId } from '../../shared/api'
import { t } from '../../shared/i18n'

export type ProductId =
  | 'mysql'
  | 'mariadb'
  | 'tidb'
  | 'oceanbase'
  | 'postgres'
  | 'kingbase'
  | 'opengauss'
  | 'oracle'
  | 'dameng'

export interface Product {
  id: ProductId
  driver: DriverId
  port: number
}

/** 界面上的顺序：按驱动分组，标准库在前。 */
export const PRODUCTS: Product[] = [
  { id: 'mysql', driver: 'mysql', port: 3306 },
  { id: 'mariadb', driver: 'mysql', port: 3306 },
  { id: 'tidb', driver: 'mysql', port: 4000 },
  { id: 'oceanbase', driver: 'mysql', port: 2881 },
  { id: 'postgres', driver: 'postgres', port: 5432 },
  { id: 'kingbase', driver: 'postgres', port: 54321 },
  { id: 'opengauss', driver: 'postgres', port: 5432 },
  { id: 'oracle', driver: 'oracle', port: 1521 },
  { id: 'dameng', driver: 'dameng', port: 5236 },
]

export function product(id: ProductId): Product {
  return PRODUCTS.find((p) => p.id === id) ?? PRODUCTS[0]
}

/** 只有驱动、没有记下产品（较早保存的连接）时对应的产品。 */
export function baseProduct(driver: DriverId): ProductId {
  return driver
}

/** 连接配置上记的产品；不认识的值按驱动处理。 */
export function productOf(driver: DriverId, saved?: string | null): ProductId {
  const p = PRODUCTS.find((x) => x.id === saved && x.driver === driver)
  return p ? p.id : baseProduct(driver)
}

export function productLabel(id: ProductId): string {
  return t(`connections.product.${id}.label`)
}

/** 产品的说明：默认端口以外的登录提示、需要的客户端库等；没有时为空串。 */
export function productNote(id: ProductId): string {
  return t(`connections.product.${id}.note`, { defaultValue: '' })
}
```

- [ ] **Step 8: 修改 `web/src/features/data/DataView.tsx`**

```diff
--- a/web/src/features/data/DataView.tsx
+++ b/web/src/features/data/DataView.tsx
@@ -65,6 +65,8 @@ export default function DataView({ tab, profiles }: { tab: QueryTab; profiles: P
   )
   const readOnly = !isTable || !info?.key
   const readOnlyReason = !isTable ? t('editor.data.edit.viewReadOnly') : info && !info.key ? t('editor.data.edit.noKey') : null
+  // 人大金仓、openGauss 的 Oracle 兼容模式：空串存进去就是 NULL
+  const emptyIsNull = rt.session?.compat === 'oracle'
   const edit = useEdits((s) => s.tabs[tab.id]) ?? emptyEdit
   const failedRow = useEdits((s) => s.failed[tab.id] ?? null)
   const setEdit = (next: typeof edit) => useEdits.getState().set(tab.id, next)
@@ -294,6 +296,7 @@ export default function DataView({ tab, profiles }: { tab: QueryTab; profiles: P
                 {t('editor.data.edit.discard')}
               </Button>
               <span className="grow" />
+              {emptyIsNull && <span className="muted">{t('editor.data.edit.emptyIsNull')}</span>}
               <Button size="small" type="primary" disabled={!pending || readOnly} onClick={() => void commit()}>
                 {t('editor.data.edit.commit', { count: pending })}
               </Button>
```

- [ ] **Step 9: 修改 `web/src/features/design/OptionsPane.tsx`**

```diff
--- a/web/src/features/design/OptionsPane.tsx
+++ b/web/src/features/design/OptionsPane.tsx
@@ -29,6 +29,7 @@ export default function OptionsPane({ profileId, schema, isNew, draft, storage,
     storage?.charset && t('design.option.charset', { value: storage.charset }),
     storage?.collation && t('design.option.collation', { value: storage.collation }),
     storage?.tablespace && t('design.option.tablespace', { value: storage.tablespace }),
+    storage?.clustered && t('design.option.clustered'),
   ].filter(Boolean)
   return (
     <div className="design-pane design-options">
```

- [ ] **Step 10: 修改 `web/src/features/editor/QueryView.tsx`**

```diff
--- a/web/src/features/editor/QueryView.tsx
+++ b/web/src/features/editor/QueryView.tsx
@@ -82,7 +82,7 @@ export default function QueryView({ tab, profiles }: { tab: QueryTab; profiles:
               if (!sql) {
                 const all = model.getValue()
                 const r = profile
-                  ? await sqlClient.statementAt(sqlDialect(profile.driver, variant).dialect, all, cursor)
+                  ? await sqlClient.statementAt(sqlDialect(profile.driver, variant, rt.session?.compat).dialect, all, cursor)
                   : null
                 sql = r?.ok && r.value ? all.slice(r.value.start, r.value.end) : all
               }
@@ -169,11 +169,12 @@ export default function QueryView({ tab, profiles }: { tab: QueryTab; profiles:
         ? {
             driver: profile.driver,
             variant: rt.session?.variant ?? profile.variant,
+            compat: rt.session?.compat,
             schema: rt.session?.schema ?? defaultSchema(profile.driver, profile.user, profile.database),
             meta,
           }
         : null,
-    [profile, meta, rt.session?.variant, rt.session?.schema],
+    [profile, meta, rt.session?.variant, rt.session?.compat, rt.session?.schema],
   )
   const confirm = !rt.running && !rt.declined ? rt.exec.confirm : null
   const sub = rt.submission
@@ -198,7 +199,7 @@ export default function QueryView({ tab, profiles }: { tab: QueryTab; profiles:
         <Panel id="editor" minSize={80}>
           <SqlEditor
             path={tab.id}
-            language={profile ? editorLanguage(profile.driver) : 'sql'}
+            language={profile ? editorLanguage(profile.driver, rt.session?.compat) : 'sql'}
             defaultValue={tab.sql}
             marks={marks}
             sql={sql}
```

- [ ] **Step 11: 修改 `web/src/features/editor/dialect.ts`**

```diff
--- a/web/src/features/editor/dialect.ts
+++ b/web/src/features/editor/dialect.ts
@@ -3,11 +3,12 @@
 
 import { type FormatOptionsWithLanguage, format } from 'sql-formatter'
 
-import type { DriverId } from '../../shared/api'
+import type { Compat, DriverId } from '../../shared/api'
 import { t } from '../../shared/i18n'
 
-/** Monaco 的语言：MySQL 系、PG 系用 Monaco 自带的规则，Oracle 和达梦用 plsql（monaco.ts 注册）。 */
-export function editorLanguage(driver: DriverId): 'mysql' | 'pgsql' | 'plsql' {
+/** Monaco 的语言：MySQL 系、PG 系用 Monaco 自带的规则，Oracle、达梦和 PG 系的 Oracle 兼容模式用 plsql（monaco.ts 注册）。 */
+export function editorLanguage(driver: DriverId, compat?: Compat | null): 'mysql' | 'pgsql' | 'plsql' {
+  if (driver === 'postgres' && compat === 'oracle') return 'plsql'
   switch (driver) {
     case 'mysql':
       return 'mysql'
```

- [ ] **Step 12: 修改 `web/src/features/sql/checker.ts`**

```diff
--- a/web/src/features/sql/checker.ts
+++ b/web/src/features/sql/checker.ts
@@ -38,7 +38,7 @@ async function check(model: Model) {
   if ((timeouts.get(model) ?? 0) >= MAX_TIMEOUTS) return clear({ ...NO_SYNTAX, status: 'stopped' })
   const text = model.getValue()
   if (text.length > MAX_CHARS) return clear({ ...NO_SYNTAX, status: 'skipped' })
-  const d = sqlDialect(ctx.driver, ctx.variant)
+  const d = sqlDialect(ctx.driver, ctx.variant, ctx.compat)
   if (sqlClient.unavailable(d.dialect)) return clear(NO_SYNTAX)
 
   const version = model.getVersionId()
```

- [ ] **Step 13: 修改 `web/src/features/sql/dialects.ts`**

```diff
--- a/web/src/features/sql/dialects.ts
+++ b/web/src/features/sql/dialects.ts
@@ -1,8 +1,9 @@
 // 驱动 / 变体 → 解析器与档位（M1b2 设计文档 3.2 节）。
 // 标准库的语法错误标红；兼容库（在 MySQL / PG / Oracle 语法上有扩展）只标黄"可能有误"，避免误报显得刺眼。
 // 变体未知（还没连接过）时按驱动取解析器，按兼容档处理。
+// 人大金仓、openGauss 的 Oracle 兼容模式按 PL/SQL 的语法（兼容档）、关键字和语句拆分处理。
 
-import type { DriverId } from '../../shared/api'
+import type { Compat, DriverId } from '../../shared/api'
 import type { Dialect } from './types'
 
 export interface SqlDialect {
@@ -15,8 +16,9 @@ export interface SqlDialect {
 
 const STRICT = new Set(['mysql', 'mariadb', 'postgres', 'oracle'])
 
-export function sqlDialect(driver: DriverId, variant?: string | null): SqlDialect {
+export function sqlDialect(driver: DriverId, variant?: string | null, compat?: Compat | null): SqlDialect {
   const strict = !!variant && STRICT.has(variant)
+  if (driver === 'postgres' && compat === 'oracle') return { dialect: 'plsql', strict: false, grammar: 'Oracle' }
   switch (driver) {
     case 'mysql':
       return { dialect: 'mysql', strict, grammar: 'MySQL' }
```

- [ ] **Step 14: 修改 `web/src/features/sql/provider.ts`**

```diff
--- a/web/src/features/sql/provider.ts
+++ b/web/src/features/sql/provider.ts
@@ -1,7 +1,7 @@
 // Monaco 的 SQL 补全（M1b2 设计文档第 4 节）。三种语言（mysql、pgsql、plsql）共用一个补全提供者；
 // 每个编辑器模型绑定它所在标签页的上下文（连接、变体、当前 schema、元数据），提供者按模型找到上下文。
 
-import type { DriverId } from '../../shared/api'
+import type { Compat, DriverId } from '../../shared/api'
 import { monaco } from '../editor/monaco'
 import { type ItemKind, completionItems, lightSuggest } from './completion'
 import { sqlDialect } from './dialects'
@@ -11,6 +11,8 @@ import { sqlClient } from './service'
 export interface EditorSqlContext {
   driver: DriverId
   variant: string | null | undefined
+  /** 会话的兼容模式；没有会话时为空。 */
+  compat?: Compat | null
   /** 当前 schema：会话的，没有会话时按连接配置推断。 */
   schema: string | null
   meta: MetaReader
@@ -59,7 +61,7 @@ const provider: monaco.languages.CompletionItemProvider = {
   async provideCompletionItems(model, position) {
     const ctx = contextOf(model)
     if (!ctx) return { suggestions: [] }
-    const { dialect } = sqlDialect(ctx.driver, ctx.variant)
+    const { dialect } = sqlDialect(ctx.driver, ctx.variant, ctx.compat)
     const text = model.getValue()
     const offset = model.getOffsetAt(position)
     // 解析器不可用或超时：退回轻量补全
```

- [ ] **Step 15: 修改 `web/src/locales/en/connections.json`**

```diff
--- a/web/src/locales/en/connections.json
+++ b/web/src/locales/en/connections.json
@@ -17,6 +17,41 @@
       "note": "Requires the DM DPI client library"
     }
   },
+  "product": {
+    "mysql": {
+      "label": "MySQL"
+    },
+    "mariadb": {
+      "label": "MariaDB"
+    },
+    "tidb": {
+      "label": "TiDB",
+      "note": "Default port 4000; no stored procedures or triggers"
+    },
+    "oceanbase": {
+      "label": "OceanBase",
+      "note": "MySQL mode; user name is user@tenant (e.g. root@sys); through OBProxy the port is usually 2883"
+    },
+    "postgres": {
+      "label": "PostgreSQL"
+    },
+    "kingbase": {
+      "label": "KingbaseES",
+      "note": "Default port 54321; Oracle and PG compatibility modes are detected on connect"
+    },
+    "opengauss": {
+      "label": "openGauss",
+      "note": "SHA256 authentication supported; A (Oracle) and PG compatibility are detected per database"
+    },
+    "oracle": {
+      "label": "Oracle",
+      "note": "Requires Oracle Instant Client"
+    },
+    "dameng": {
+      "label": "Dameng DM",
+      "note": "Requires the Dameng DPI client library"
+    }
+  },
   "databaseLabel": {
     "mysql": "Database",
     "postgres": "Database",
@@ -50,12 +85,16 @@
     "test": "Test Connection",
     "cancel": "Cancel",
     "save": "Save",
-    "driverType": "Database Type",
+    "driverType": "Database",
     "created": "Connection created",
     "saved": "Connection saved",
     "testOk": "Connected: {{server}}, detected as {{variant}}",
     "tabGeneral": "General",
-    "tabAdvanced": "Advanced"
+    "tabAdvanced": "Advanced",
+    "modeOracle": "Compatibility mode: {{mode}} (Oracle rules: empty string is NULL, DATE has a time)",
+    "modeNative": "Compatibility mode: {{mode}}",
+    "modeUnadapted": "Compatibility mode {{mode}} is not supported yet; native rules are used and some features may fail",
+    "productMismatch": "{{chosen}} was selected but the server is {{detected}}; DBLens follows the detected server"
   },
   "general": {
     "name": "Name",
```

- [ ] **Step 16: 修改 `web/src/locales/en/design.json`**

```diff
--- a/web/src/locales/en/design.json
+++ b/web/src/locales/en/design.json
@@ -88,7 +88,8 @@
     "engine": "Engine: {{value}}",
     "charset": "Charset: {{value}}",
     "collation": "Collation: {{value}}",
-    "tablespace": "Tablespace: {{value}}"
+    "tablespace": "Tablespace: {{value}}",
+    "clustered": "Clustered primary key (changing it rebuilds the table)"
   },
   "sql": {
     "nothing": "No changes",
```

- [ ] **Step 17: 修改 `web/src/locales/en/editor.json`**

```diff
--- a/web/src/locales/en/editor.json
+++ b/web/src/locales/en/editor.json
@@ -112,7 +112,8 @@
       "cancel": "Cancel",
       "applied_one": "{{count}} statement committed",
       "applied_other": "{{count}} statements committed",
-      "noSession": "The data has not been read yet; refresh first"
+      "noSession": "The data has not been read yet; refresh first",
+      "emptyIsNull": "This database stores empty strings as NULL"
     },
     "import": "Import",
     "design": "Design"
```

- [ ] **Step 18: 修改 `web/src/locales/zh-CN/connections.json`**

```diff
--- a/web/src/locales/zh-CN/connections.json
+++ b/web/src/locales/zh-CN/connections.json
@@ -17,6 +17,41 @@
       "note": "需要达梦 DPI 客户端库"
     }
   },
+  "product": {
+    "mysql": {
+      "label": "MySQL"
+    },
+    "mariadb": {
+      "label": "MariaDB"
+    },
+    "tidb": {
+      "label": "TiDB",
+      "note": "默认端口 4000；没有存储过程、触发器"
+    },
+    "oceanbase": {
+      "label": "OceanBase",
+      "note": "MySQL 模式；用户名写成 用户@租户（如 root@sys），经 OBProxy 连接时端口一般是 2883"
+    },
+    "postgres": {
+      "label": "PostgreSQL"
+    },
+    "kingbase": {
+      "label": "人大金仓",
+      "note": "KingbaseES，默认端口 54321；Oracle 兼容模式与 PG 模式连接后自动识别"
+    },
+    "opengauss": {
+      "label": "openGauss",
+      "note": "支持 SHA256 认证；A（Oracle 兼容）与 PG 兼容模式按库自动识别"
+    },
+    "oracle": {
+      "label": "Oracle",
+      "note": "需要 Oracle Instant Client"
+    },
+    "dameng": {
+      "label": "达梦 DM",
+      "note": "需要达梦 DPI 客户端库"
+    }
+  },
   "databaseLabel": {
     "mysql": "数据库",
     "postgres": "数据库",
@@ -50,12 +85,16 @@
     "test": "测试连接",
     "cancel": "取消",
     "save": "保存",
-    "driverType": "数据库类型",
+    "driverType": "数据库",
     "created": "连接已创建",
     "saved": "连接已保存",
     "testOk": "连接成功：{{server}}，识别为 {{variant}}",
     "tabGeneral": "常规",
-    "tabAdvanced": "高级"
+    "tabAdvanced": "高级",
+    "modeOracle": "兼容模式：{{mode}}（按 Oracle 规则：空串即 NULL，DATE 带时间）",
+    "modeNative": "兼容模式：{{mode}}",
+    "modeUnadapted": "兼容模式 {{mode}} 尚未适配，按原生规则处理，部分功能可能出错",
+    "productMismatch": "选的是 {{chosen}}，实际识别为 {{detected}}，按识别结果处理"
   },
   "general": {
     "name": "名称",
```

- [ ] **Step 19: 修改 `web/src/locales/zh-CN/design.json`**

```diff
--- a/web/src/locales/zh-CN/design.json
+++ b/web/src/locales/zh-CN/design.json
@@ -88,7 +88,8 @@
     "engine": "引擎 {{value}}",
     "charset": "字符集 {{value}}",
     "collation": "排序规则 {{value}}",
-    "tablespace": "表空间 {{value}}"
+    "tablespace": "表空间 {{value}}",
+    "clustered": "主键为聚簇索引（更换主键时重建表）"
   },
   "sql": {
     "nothing": "没有改动",
```

- [ ] **Step 20: 修改 `web/src/locales/zh-CN/editor.json`**

```diff
--- a/web/src/locales/zh-CN/editor.json
+++ b/web/src/locales/zh-CN/editor.json
@@ -109,7 +109,8 @@
       "execute": "执行",
       "cancel": "取消",
       "applied": "已提交 {{count}} 条语句",
-      "noSession": "数据还没有读取，请先刷新"
+      "noSession": "数据还没有读取，请先刷新",
+      "emptyIsNull": "此库空串按 NULL 存储"
     },
     "import": "导入",
     "design": "设计表"
```

- [ ] **Step 21: 修改 `web/src/shared/api.ts`**

```diff
--- a/web/src/shared/api.ts
+++ b/web/src/shared/api.ts
@@ -84,6 +84,8 @@ export interface ProfileInput {
   environment?: Environment
   color?: string | null
   connect_timeout_secs?: number
+  /** 新建连接时选的产品（如 kingbase）；没有时按驱动显示。连上后以识别出的变体为准。 */
+  product?: string | null
 }
 
 export interface Profile extends ProfileInput {
@@ -98,8 +100,16 @@ export interface ServerSummary {
   variant: string
   variant_name: string
   version_code: number | null
+  /** 兼容模式：人大金仓、openGauss 可能是 oracle。 */
+  compat: Compat
+  /** 探测到的兼容模式名（人大金仓的 database_mode、openGauss 当前库的 DBCOMPATIBILITY）。 */
+  mode: string | null
+  /** 模式名不在已适配的范围内（如 openGauss 的 B 模式）。 */
+  unadapted_mode: boolean
 }
 
+export type Compat = 'native' | 'oracle'
+
 export interface SecretStatus {
   backend: 'keyring' | 'vault'
   vault_initialized: boolean
@@ -110,6 +120,7 @@ export interface SessionInfo {
   id: number
   profile_id: string
   variant: string
+  compat: Compat
   autocommit: boolean
   in_transaction: boolean
   busy: boolean
@@ -519,6 +530,8 @@ export interface DesignTableResult {
     charset: string | null
     collation: string | null
     tablespace: string | null
+    /** 主键是否聚簇（只有 TiDB 报告）。 */
+    clustered?: boolean | null
     row_count: number | null
   }
   fingerprint: string
```

- [ ] **Step 22: 检查**

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

Expected: 全部通过（前端新增 3 个测试）。

- [ ] **Step 23: 提交**

```bash
git add crates/dblens-service/src/session.rs web/src/features/connections/ConnectionDialog.tsx web/src/features/connections/form.test.ts web/src/features/connections/form.ts web/src/features/connections/products.ts web/src/features/data/DataView.tsx web/src/features/design/OptionsPane.tsx web/src/features/editor/QueryView.tsx web/src/features/editor/dialect.ts web/src/features/editor/runner.test.ts web/src/features/sql/checker.ts web/src/features/sql/dialects.test.ts web/src/features/sql/dialects.ts web/src/features/sql/provider.ts web/src/locales/en/connections.json web/src/locales/en/design.json web/src/locales/en/editor.json web/src/locales/zh-CN/connections.json web/src/locales/zh-CN/design.json web/src/locales/zh-CN/editor.json web/src/shared/api.ts
git commit -m "feat(web): 新建连接按产品选；编辑器按兼容模式切换" -m "连接对话框列出 MySQL、MariaDB、TiDB、OceanBase、PostgreSQL、人大金仓、openGauss、Oracle、达梦，带默认端口和提示；测试连接显示兼容模式与识别结果不一致的提示；会话信息带兼容模式，Oracle 兼容模式下编辑器按 PL/SQL 的语法、补全和拆分；数据标签提示空串按 NULL 存储；设计器显示 TiDB 的聚簇主键。" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 12: 文档

- `docs/notes/domestic-batch1.md`：结构、各库的差异、与设计文档不一致的地方、测试库（不含密码）、验证。
- `docs/notes/m0-4b-api.md`：连接配置的 `product`、测试连接返回的兼容模式、会话的 `compat`。
- 总设计文档：第一批完成；第二、三批的状态。

**Files:**
- Create: `docs/notes/domestic-batch1.md`
- Modify: `docs/notes/m0-4b-api.md`
- Modify: `docs/superpowers/specs/2026-10-22-dblens-design.md`

- [ ] **Step 1: 新建 `docs/notes/domestic-batch1.md`**

````markdown
# 国产库适配第一批

依据：设计文档 `docs/superpowers/specs/2026-10-27-domestic-batch1-design.md`。

第一批是 TiDB、OceanBase 社区版（MySQL 模式）、openGauss（A、PG 兼容模式）、人大金仓 KingbaseES V9R2（Oracle、PG 模式），功能与 MySQL、PostgreSQL 相同。

## 结构

```text
crates/dblens-dialect/
  src/variant.rs      Compat（Native / Oracle）
  src/lib.rs          Dialect 带兼容模式；refine 读探测结果（模式、server_version_num）；split / splitter 按模式
  src/split.rs        StreamSplitter::with_oracle_blocks：PG 家族的 Oracle 兼容模式识别 PL/SQL 块和独占一行的 /
  sql/tidb.sql、oceanbase.sql、kingbase.sql、opengauss.sql   元数据模板的覆盖文件
  sql/ops/tidb.sql、oceanbase.sql、opengauss.sql             运维查询的覆盖文件
  src/design/catalog.rs、sql.rs、postgres.rs                  设计器的能力表、TiDB 主键、openGauss 的 serial
  src/import.rs、changes.rs                                   openGauss 的按主键更新；Oracle 模式下空串按 IS NULL 定位
crates/dblens-driver-postgres/src/types.rs   兼容库类型（域、clob、int1、int16 等）按标准类型读写
crates/dblens-driver-mysql/src/error.rs      OceanBase、TiDB 报错的出错位置
crates/dblens-service/src/registry.rs        测试连接返回兼容模式；profile.rs、store.rs 记录产品（迁移版本 4）
vendor/tokio-postgres、vendor/postgres-protocol   openGauss 的 SHA256 认证补丁（vendor/PATCHES.md）
web/src/features/connections/products.ts      按产品新建连接
web/src/features/sql/dialects.ts、editor/dialect.ts   Oracle 兼容模式按 PL/SQL 的语法、补全、拆分、高亮
```

## 各库的差异

TiDB（v8.5.8，`VERSION()` 报 8.0.11）：
- 没有存储过程、触发器、全文索引；有序列（`information_schema.SEQUENCES`）。
- CHECK 约束只在 `tidb_enable_check_constraint` 打开时保存，且不在 `TABLE_CONSTRAINTS` 里，改从 `TIDB_CHECK_CONSTRAINTS` 读。
- 对 `information_schema` 做相关子查询，匹配多行时结果全是 NULL；表的分区改用 LEFT JOIN 读取。
- `AUTO_RANDOM` 不在 `EXTRA` 里，由表的 `TIDB_ROW_ID_SHARDING_INFO` 判断；表存储信息带 `clustered`（`TIDB_PK_TYPE`）。
- 接受 `DESC` 但按升序建索引；聚簇主键不能删除或更换。
- 锁等待从 `DATA_LOCK_WAITS` 与 `CLUSTER_TIDB_TRX` 读；报错格式为 `line N column M near "…"`。
- 自增值按批分配，重建表后新值大于原最大值，但不一定紧接着。

OceanBase 社区版（4.4.2.1，MySQL 模式，`VERSION()` 报 5.7.25）：
- 执行 CHECK 约束，按 MySQL 8.0.16+ 的方式读取。
- `lower_case_table_names = 1`，表名存成小写。
- 分区表的 `CREATE_OPTIONS` 为 NULL，分区改用 LEFT JOIN 读取。
- 全文索引在 `STATISTICS` 中只有隐藏列（`__doc_id_*`、`__word_segment_*`），去掉后不列出。
- 接受 `DESC` 但按升序建索引。
- `TRIGGERS.ACTION_STATEMENT` 是 `CREATE` 之后的整条语句，不只是触发器体。
- 没有 `information_schema.KEYWORDS`、sys schema 和 `performance_schema` 的语句摘要：保留字用内置列表，锁从 `GV$OB_LOCKS`、慢 SQL 从 `GV$OB_PLAN_CACHE_PLAN_STAT` 读。
- 报错信息后另起一行附上节点和追踪号。

openGauss（7.0.0-RC1，基于 PG 9.2.4）：
- 认证：远程登录常用 SHA256（请求码 10，与 PG 的 SASL 相同），见 `vendor/PATCHES.md`。
- 没有 IDENTITY 列：设计器的自增列用 `serial` / `bigserial` / `smallserial`，已有列改成自增时建序列并设默认值。
- 没有 `LATERAL`、`WITH ORDINALITY`、`relispartition`、`pg_sequence`、`pg_get_partkeydef`、`attidentity`、`attgenerated`：
  - 分区在 `pg_partition` 中，分区表在 `pg_class` 里是 `relkind = 'r'`、`parttype = 'p'`；
  - 生成列由 `pg_attrdef.adgencol = 's'` 标记；
  - 序列参数用 `pg_sequence_parameters`，读不到缓存大小。
- 聚合函数的 `prokind` 也是 `f`，仍要用 `proisagg` 排除；包里的函数 `propackageid` 不为 0。
- 同义词在 `pg_synonym`，包在 `gs_package`（只有 A 模式能建）。
- 没有 `ON CONFLICT`，按主键更新用 `ON DUPLICATE KEY UPDATE … = EXCLUDED.…`。
- `pg_stat_activity` 没有 `backend_type`、`wait_event_type`；没有 `pg_blocking_pids`；语句统计在 `dbe_perf.statement`（需要 MONADMIN 权限）。
- LIST 分区键不能是 text；没有 EXCLUDE 约束；numeric 的小数位不能为负。
- 新建库默认是 A（Oracle 兼容）模式。

人大金仓 KingbaseES（V9R2C16，基于 PG 12）：
- 版本字符串 `V009R002C016` 会被解析成 9.2，改用 `server_version_num`（120001）。
- 两种模式的系统目录都是 PG 的，PG 模板大部分可直接用。
- Oracle 模式下空串就是 NULL：PG 模板中与 `''` 的比较全部改写（`attidentity IN ('a', 'd')`、`attgenerated = 's'`）。
- Oracle 模式的 `DATE` 是 timestamp 上的域，`NUMBER` 读出来就是 numeric；`'NaN'::numeric` 不被接受。
- 同义词在 `sys_synonym`，包在 `_pkg`（`pkgspecsrc` / `pkgbodysrc`）。
- 开发版授权：最多 10 个连接，不能建物化视图。
- 没有加载 `sys_stat_statements`，慢 SQL 查询报错（与 PG 测试库没有 `pg_stat_statements` 相同）。

## 与设计文档不一致的地方

- 所基于的 PG 版本（4.2 节）由探测查询一并读出（`current_setting('server_version_num')`），`ServerInfo` 没有加字段。
- openGauss 的 MD5_SHA256（请求码 11）不实现（5.1 节）：服务端计算 MD5 摘要时包含了存储串末尾的迭代次数后缀，客户端收不到这个后缀，无法通过认证（openGauss 自己的 libpq 也是按不含后缀计算）。驱动遇到时报出明确的错误，提示把 pg_hba 改为 sha256。
- 兼容库类型（5.2 节）不另查 `pg_type`：tokio-postgres 解析未知类型时已经查过，域类型取基础类型，其余按类型名（clob、nvarchar2、blob、raw、number、int1、tinyint、int16、smalldatetime 等）对应到标准类型；参数也按对应的标准类型编码。
- 同义词与包（6.1 节）在两种模式下都列出：两个库的目录在 PG 模式下也有同义词，包在 PG 模式下为空。
- openGauss 读不到序列的缓存大小。
- OceanBase 的全文索引不列出（只有隐藏列）。
- TiDB 删除或更换主键一律重建（设计文档写"聚簇主键"）：默认建的主键都是聚簇的，非聚簇主键重建也正确，只是更慢。选项页显示是否聚簇。
- TiDB 的检查约束在设计器中按"只解析不执行"提示，不看 `tidb_enable_check_constraint` 的当前值。
- 插入时有默认值的列留空就不写（8.1 节的兜底），M2a 时已经是这个规则，没有改动。
- 编辑器（7.1 节）：Oracle 兼容模式下按 PL/SQL 的语法（兼容档，只标黄）、关键字补全和拆分处理。在 Oracle 模式里用 `$$` 写 PG 风格的函数时，编辑器的"执行当前语句"可能选错范围；后端执行（含执行 SQL 文件）两种写法都正确拆分。
- 运维查询中 OceanBase 的锁等待按文档写成，测试库上只验证了能执行，没有构造锁等待核对内容。
- 所选产品与识别结果不一致的提示：选的是标准库（MySQL、PostgreSQL、Oracle、达梦）时不提示，较早保存的连接都是按驱动选的。

## 测试库

在测试虚拟机 192.168.139.106 上（密码在虚拟机 root 的 `/root/dblens-test/creds.env`，不进仓库；运行测试时以环境变量传入）：

| 目标 | 端口 | 部署 |
|---|---|---|
| TiDB v8.5.8 | 4000 | tiup 镜像站的官方二进制，单个 tidb-server（`-store unistore -path /data/tidb`），systemd `tidb`，`WorkingDirectory=/data/tidb`（慢日志用相对路径） |
| OceanBase CE 4.4.2.1 | 2881 | `quay.io/oceanbase/oceanbase-ce:4.4.2-lts`，`MODE=mini`，必须加 `OB_SERVER_IP=127.0.0.1` 和 `--ulimit nofile=65535:65535`，租户 `test` |
| openGauss 7.0.0-RC1 | 5433 | Docker Hub 的 `opengauss/opengauss:7.0.0-RC1`（经华为云 SWR 同步副本拉取；quay.io 的 7.0.0-RC3 镜像缺动态库）；`password_encryption_type = 1`，远程认证 sha256；库 `dblens_test`（PG）、`dblens_test_a`（A）；账号 `admin`（MONADMIN）、`admin_sha`（只存 SHA256）、`admin_m5s`（pg_hba 为 md5，用于确认不支持的报错） |
| 人大金仓 V9R2C16 | 54321 / 54322 | 官方 DEB 包，开发版授权（365 天）；Oracle 模式 systemd `kingbased`，PG 模式第二个实例 `kingbased-pg`（`initdb -m pg`） |

实库测试的环境变量见设计文档 9.1 节。人大金仓的测试要串行运行（`--test-threads=1`，授权限制 10 个连接）；TiDB 的元数据测试会把 `tidb_enable_check_constraint` 设为 ON。

## 验证

- 后端：fmt、clippy 通过；离线测试全部通过。
  - dialect：兼容模式探测与版本、Oracle 模式拆分、各变体元数据模板的快照、旧版本分支能渲染、设计器能力表、openGauss 的按主键更新、Oracle 模式的空串定位。
  - 驱动：openGauss SHA256 证明（固定输入）、请求报文解析、TiDB 和 OceanBase 的出错位置。
- 实库（原有 5 个库与新增 7 个目标）：驱动一致性、元数据、服务层全部流程（会话、工作台、取消、对象操作、运维、导出、编辑、导入、执行 SQL 文件、表设计器）、接口层。
  - 达梦和其他测试同在一个进程里运行时偶发"网络通讯失败"，单独运行通过（与以前的记录相同）。
- 前端：tsc、Vitest、打包通过。
````

- [ ] **Step 2: 修改 `docs/notes/m0-4b-api.md`**

```diff
--- a/docs/notes/m0-4b-api.md
+++ b/docs/notes/m0-4b-api.md
@@ -68,7 +68,7 @@
 | PUT | `/profiles/{id}` | 修改，请求体为配置本身 |
 | DELETE | `/profiles/{id}` | 删除，返回 204 |
 | PUT | `/profiles/{id}/password` | 设置或清除密码，`{"password": "…" \| null}` |
-| POST | `/profiles/test` | 测试连接，`{"profile": {…}, "password": "…"}`，返回服务器信息和识别出的方言 |
+| POST | `/profiles/test` | 测试连接，`{"profile": {…}, "password": "…"}`，返回服务器信息和识别出的方言；人大金仓、openGauss 另有 `compat`（`native` / `oracle`）、`mode`（探测到的兼容模式名）、`unadapted_mode`（国产库第一批加入） |
 
 配置的字段：
 - 必填：`name`、`driver`（`mysql` / `postgres` / `oracle` / `dameng`）、`host`、`port`、`user`；
@@ -77,7 +77,8 @@
   - `options`：键值对，如 `tls`、`tls_ca`、`sid`；
   - `environment`：`none` / `dev` / `test` / `prod`；
   - `color`；
-  - `connect_timeout_secs`。
+  - `connect_timeout_secs`；
+  - `product`：新建连接时选的产品（如 `kingbase`、`opengauss`、`tidb`），只用于显示，连上后以识别出的变体为准；没有时按驱动显示（国产库第一批加入）。
 - 只读：`variant`，上次连接成功时识别出的数据库变体（如 `kingbase`），没连接过时为 null（M1i 加入）。
   - 由服务端在连接成功时写入，修改配置时忽略请求中的值。
   - 主机、端口或驱动变了则清空。
@@ -107,7 +108,7 @@
 |---|---|---|
 | POST | `/sessions` | 打开会话，`{"profile_id": "…"}`，返回 201 和会话信息 |
 | GET | `/sessions` | 全部会话 |
-| GET | `/sessions/{id}` | `{id, profile_id, variant, autocommit, in_transaction, busy, broken, schema}`；`schema` 是当前 schema（M1b1 加入） |
+| GET | `/sessions/{id}` | `{id, profile_id, variant, compat, autocommit, in_transaction, busy, broken, schema}`；`schema` 是当前 schema（M1b1 加入）；`compat` 是兼容模式（国产库第一批加入） |
 | DELETE | `/sessions/{id}?commit=false` | 关闭；有未提交的修改时按 `commit` 提交或回滚 |
 | POST | `/sessions/{id}/autocommit` | 切换自动提交，`{"on": bool}` |
 | POST | `/sessions/{id}/commit` | 提交 |
```

- [ ] **Step 3: 修改 `docs/superpowers/specs/2026-10-22-dblens-design.md`**

```diff
--- a/docs/superpowers/specs/2026-10-22-dblens-design.md
+++ b/docs/superpowers/specs/2026-10-22-dblens-design.md
@@ -329,6 +329,7 @@ trait Policy: Send + Sync {
 - M2b 已完成（设计 `docs/superpowers/specs/2026-10-27-m2b-import-design.md`，详细计划 `docs/superpowers/plans/2026-10-27-m2b-import.md`，说明 `docs/notes/m2b-import.md`），M2 完成。
 - M3 已完成（设计 `docs/superpowers/specs/2026-10-27-m3-table-designer-design.md`，详细计划 `docs/superpowers/plans/2026-10-27-m3-table-designer.md`，说明 `docs/notes/m3-table-designer.md`）。
 - 2026-10-27 用户决定：国产库适配（OceanBase、人大金仓、PolarDB、TiDB、TDSQL、GaussDB、GoldenDB、GBase）作为 M3 之后的单独里程碑，分三批：有 Docker 或免费版的；只有云服务的；需要新驱动或新架构的（GBase 8s、OceanBase 的 Oracle 模式）。
+- 国产库第一批已完成：TiDB、OceanBase 社区版（MySQL 模式）、openGauss（A、PG 兼容模式，含 SHA256 认证）、人大金仓（Oracle、PG 模式）（设计 `docs/superpowers/specs/2026-10-27-domestic-batch1-design.md`，详细计划 `docs/superpowers/plans/2026-10-27-domestic-batch1.md`，说明 `docs/notes/domestic-batch1.md`）。第二批（PolarDB、TDSQL、GaussDB，只有云服务）、第三批（GBase 8s、OceanBase 的 Oracle 模式）尚未开始；GoldenDB 没有可用的测试环境。
 
 ## 12. 风险与待验证项
 
```

- [ ] **Step 4: 检查**

检查文档中的路径和接口与代码一致。

Expected: 无。

- [ ] **Step 5: 提交**

```bash
git add docs/notes/domestic-batch1.md docs/notes/m0-4b-api.md docs/superpowers/specs/2026-10-22-dblens-design.md
git commit -m "docs: 国产库第一批的说明文档、接口说明与进度" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```
