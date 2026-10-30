# M6b SQL Server：表设计器、导入、文档导出、比对与模型、迁移 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让 SQL Server 的写入类功能与其他库同等可用：表设计器（变更语句、可回滚、需要时重建）、导入（参数上限、多行 MERGE、自增列）、文档导出、模型与比对（同步脚本带 GO）、作为来源和目标的结构与数据迁移；取消 M6a 的置灰。

**Architecture:** 方言层新增 SQL Server 的设计器变更语句（`design/sqlserver.rs`），批量写入按方言限制参数、用多行 MERGE 和直接写 NULL，脚本按目标结束语句（SQL Server 后接 `GO`），迁移的序列、默认值和表达式补上 SQL Server 的写法；服务层的写入类请求加可选的 `catalog`，用 `Profile::in_catalog` 连到该库；前端在请求边界把合成的 schema 拆成库和架构，schema 下拉框按库列出。

**Tech Stack:** Rust（tiberius、insta）；React 19 + TypeScript、antd 6、TanStack Query、Vitest。

**Spec:** `docs/superpowers/specs/2026-10-29-m6b-sqlserver-design.md`

## Global Constraints

- SQL Server 2017 及以上；日常实测 2022（Docker），兼容实测 2025 Express（Win11 测试虚拟机）。
- 默认值约束：设计器建的命名为 `DF_表名_列名`；删除时在执行时从 `sys.default_constraints` 查名字，不改公共模型。
- SQL Server 的设计器改动在一个事务中执行，任一条失败整体回滚。
- SQL Server 一次请求最多 2100 个参数：每条写入语句至多 2000 个参数、500 行。
- 给 SQL Server 的脚本每条语句后接单独一行 `GO`；在 DBLens 中直接执行时语句逐条发送。
- 写入类请求加可选的 `catalog`（其他库不填，行为不变）；前端界面中仍用合成的 schema，在请求边界拆开。
- 迁移实库路线：SQL Server 与 MySQL、PG、Oracle、达梦互迁 8 条，另加以 SQL Server 为目标的续传、跳过各 1 条。
- 依赖只用正式发布版，跟随最新；新版发布满 7 天再升级（有意固定版本的记入 `scripts/check-deps.mjs` 的 `HOLD`）。
- 前端给用户看的文字都用词条（中英文）；后端错误和提示用 `msg!` 词条（`src` 中的中文字面量只能出现在 `#[cfg(test)]` 之后，单独的测试文件中也不能有）；日志不翻译。
- 代码注释用中文；文件 UTF-8、LF 换行；前端代码风格：无分号、单引号、两格缩进。
- cargo 命令加 `--config "source.mirror.registry='sparse+https://rsproxy.cn/index/'"`（clippy 写在 `clippy` 之后），并设 `CARGO_INCREMENTAL=0`；后端测试在 `DBLENS_LANG=en` 下运行。
- 不同的仓库副本不要共用 `CARGO_TARGET_DIR`（cargo 按相对路径和修改时间判断是否要重编，会用到别的副本编出的库）。
- 实库测试一律串行（`--test-threads=1`），不加 `--include-ignored`（会跑达梦的 spike 测试并卡住）。
- 前端工具用 PowerShell 执行（`npm`/`npx` 在本机不可用，用 `pnpm exec`）。
- 测试数据库的密码只在命令行中以内联环境变量传入，不写进仓库、报告或提交信息。

## 与设计文档的差异（写计划时实测后调整）

- 库名的传递（设计文档第 6 节，写计划时补充）：前端界面中仍用合成的 schema，只在请求后端和另存迁移方案时拆开；比对标签存在本机界面状态中，不拆。
- SQL Server 的写入把 NULL 直接写进语句（导入、迁移、表数据编辑）：驱动按 nvarchar 传类型未知的 NULL，写进 varbinary 列会报错。
- 删除默认值约束用 `sp_executesql` 执行拼好的语句（`EXEC (…)` 中不能调用 `QUOTENAME`）。
- 重建后把新表上按临时表名命名的默认值约束改回按原表名命名；依赖视图执行 `sp_refreshview`。
- `NONCLUSTERED` 视为默认的索引方法（比对、设计器）。
- 迁移：序列写成 `AS BIGINT … NO CACHE / NO CYCLE`，超出范围的边界不写；布尔默认值写 1、0；时间和 UUID 默认值的 SQL Server 写法；
  来源表达式中的 `[列]`、`N'…'`；`NEXT VALUE FOR` 补 schema；时间按目标精度截断后再写（所有目标）。
- 导入的 `MERGE` 不更新自增列；反向生成模型时 SQL Server 的架构必须在同一个库中。
- 任务划分：方言层的写入、脚本、迁移改动与服务层的 `catalog` 改的是同一批文件（脚本函数的签名、各请求的结构），合为一个后端任务。

## 执行说明

任务 2–5 的代码都已在临时副本中逐任务实现并通过检查（每个任务提交后单独跑过 fmt、clippy 和全部测试），执行时按顺序重放。新文件给出全文，修改的文件给出 diff（基于上一个任务完成后的状态，可直接 `git apply`）。`Cargo.lock`、`web/pnpm-lock.yaml` 没有变化。

写计划时已运行：全部后端测试和前端测试；13 个实库目标的串行回归（含 SQL Server 2022 和 10 条 SQL Server 迁移路线）；SQL Server 2025 Express 上的 SQL Server 测试；浏览器中的设计器（读原表、预览、执行）、比对、迁移预览和 schema 下拉框。

## 文件结构

```text
crates/dblens-dialect/src/design/{sqlserver.rs（新增）, mod.rs, sql.rs, rebuild.rs, diff.rs}, snapshots/
crates/dblens-dialect/src/{import.rs, changes.rs, variant.rs, lib.rs}, compare/sync.rs, migrate/{script.rs, expr.rs, tests.rs}, migrate/snapshots/
crates/dblens-service/src/{service.rs, design.rs, compare.rs, import/job.rs, document/job.rs, model/ops.rs}, migrate/{mod.rs, run.rs, report.rs, values.rs}
crates/dblens-service/tests/{live.rs, migrate_live.rs}; crates/dblens-api/src/{design.rs, tasks.rs}, tests/live.rs; crates/dblens-core/locales/app.json
web/src/shared/{api.ts, api.test.ts, schemaOptions.ts（新增）}
web/src/features/{tree/ObjectTree.tsx, data/DataView.tsx, compare/CompareView.tsx, migrate/{MigrateView.tsx, plan.ts}, model/ModelDialogs.tsx,
  design/{summary.ts, summary.test.ts, DesignView.tsx, SaveDialog.tsx}, import/ImportDialog.tsx, document/ExportDocDialog.tsx}
web/src/locales/{zh-CN,en}/{tree.json, model.json}
docs/notes/{m6b-sqlserver.md（新增）, m0-4b-api.md}; docs/superpowers/specs/2026-10-22-dblens-design.md
```

---

### Task 1: 依赖复查

按依赖原则（M1b2 设计文档 3.1 节）复查前端和后端的全部直接依赖。写计划时 88 个直接依赖都不用升级。

**Files:** 视复查结果而定（`web/package.json`、`Cargo.toml` 及锁文件）。

- [ ] **Step 1: 运行复查脚本**

本机经代理上网，Node 的 `fetch` 默认不读 `HTTPS_PROXY`，必须先设 `NODE_USE_ENV_PROXY=1`，否则全部查询失败，最后仍显示"0 个可升级"。

```powershell
$env:PATH = "D:\Environments\nvm4w\nodejs;$env:PATH"; $env:NODE_USE_ENV_PROXY = '1'; $env:HTTPS_PROXY = 'http://127.0.0.1:6789'; node scripts/check-deps.mjs
```

Expected：输出中没有"查询失败"，以执行当天的结果为准。

- [ ] **Step 2: 升级（有可升级项时）**

输出中 `->` 的都升级：npm 用 `pnpm add 包@版本`（在 `web` 目录），Cargo 改工作区 `Cargo.toml` 后执行 `cargo update -p 包`。然后跑全量检查：

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

Expected：全部通过。

- [ ] **Step 3: 提交（有改动时）**

```bash
git add web/package.json web/pnpm-lock.yaml Cargo.toml Cargo.lock
git commit -m "chore: M6b 依赖复查" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 2: 方言层：SQL Server 表设计器的变更语句与重建

- `design/sqlserver.rs`（新）：列定义（计算列 `名 AS (表达式)`、`IDENTITY(1,1)`、带名字的默认值 `CONSTRAINT DF_表_列 DEFAULT …`、总是写明 `NULL` / `NOT NULL`）；
  删除默认值约束的 T-SQL（执行时从 `sys.default_constraints` 查名字，拼好后 `sp_executesql`）；注释（扩展属性 `MS_Description`，已有更新、没有添加、清空删除）；
  `sp_rename` 改列名、索引名；索引 `CREATE [UNIQUE] [CLUSTERED|NONCLUSTERED] INDEX`、`DROP INDEX 名 ON 表`；`sqlserver_design` 把各类变更生成为语句。
- `design/sql.rs`：`design_sql` 的 SQL Server 分支；`design_transactional` 对 SQL Server 为真；`rename_table_sql`（MySQL 新名带 schema、SQL Server `sp_rename`、其他 `RENAME TO`）；
  "每条生成的语句只拆出一条"的测试加 SQL Server。
- `design/rebuild.rs`：存储子句 `ON [文件组]`；只在复制自增列时打开 `IDENTITY_INSERT`（达梦、SQL Server）；改回原名用 `rename_table_sql`；
  SQL Server 再把新表上的默认值约束改成按原表名命名；依赖视图 `sp_refreshview`。
- `design/diff.rs`：`NONCLUSTERED` 视为默认的索引方法。

**Files:**
- Modify: `crates/dblens-dialect/src/design/diff.rs`
- Modify: `crates/dblens-dialect/src/design/mod.rs`
- Modify: `crates/dblens-dialect/src/design/rebuild.rs`
- Create: `crates/dblens-dialect/src/design/snapshots/dblens_dialect__design__rebuild__tests__rebuild_sqlserver.snap`
- Create: `crates/dblens-dialect/src/design/snapshots/dblens_dialect__design__sqlserver__tests__sqlserver_alter.snap`
- Create: `crates/dblens-dialect/src/design/snapshots/dblens_dialect__design__sqlserver__tests__sqlserver_create.snap`
- Create: `crates/dblens-dialect/src/design/snapshots/dblens_dialect__design__sqlserver__tests__sqlserver_everything.snap`
- Modify: `crates/dblens-dialect/src/design/sql.rs`
- Create: `crates/dblens-dialect/src/design/sqlserver.rs`

**Interfaces:**
- Consumes：现有的 `Dialect`（`ident`、`qualified`、`literal`、`index_items`、`fk_clause`、`check_clause`、`constraint_name`、`required_name`）。
- Produces：`Dialect::design_sql` 对 SQL Server 可用；`Dialect::design_transactional()` 对 SQL Server 为 true；`pub(crate) fn rename_table_sql(&self, schema, from, to) -> Result<String>`；
  `design::sqlserver::df_name(table, column) -> String`。

- [ ] **Step 1: 修改 `crates/dblens-dialect/src/design/diff.rs`**

```diff
--- a/crates/dblens-dialect/src/design/diff.rs
+++ b/crates/dblens-dialect/src/design/diff.rs
@@ -331,9 +331,12 @@ pub fn diff_table(original: Option<&Table>, draft: &Table, renames: &Renames) ->
             .collect(),
         ..x.clone()
     };
-    // 索引方法：不写与各库的默认方法（BTREE、NORMAL）相同
+    // 索引方法：不写与各库的默认方法（BTREE、NORMAL、SQL Server 的 NONCLUSTERED）相同
     let method = |x: &Index| {
-        x.method.as_deref().map(str::to_ascii_uppercase).filter(|m| !matches!(m.as_str(), "BTREE" | "NORMAL"))
+        x.method
+            .as_deref()
+            .map(str::to_ascii_uppercase)
+            .filter(|m| !matches!(m.as_str(), "BTREE" | "NORMAL" | "NONCLUSTERED"))
     };
     let same_index =
         |x: &Index, y: &Index| x.columns == y.columns && x.unique == y.unique && method(x) == method(y);
@@ -444,6 +447,9 @@ mod tests {
         draft.foreign_keys[0].on_delete = None;
         draft.comment = Some(" 订单 ".into());
         assert!(diff(&t, &draft, &Renames::default()).is_empty());
+        // SQL Server 的非聚集索引是默认方法
+        draft.indexes[0].method = Some("NONCLUSTERED".into());
+        assert!(diff(&t, &draft, &Renames::default()).is_empty());
     }
 
     #[test]
```

- [ ] **Step 2: 修改 `crates/dblens-dialect/src/design/mod.rs`**

```diff
--- a/crates/dblens-dialect/src/design/mod.rs
+++ b/crates/dblens-dialect/src/design/mod.rs
@@ -8,6 +8,7 @@ mod oracle;
 mod postgres;
 mod rebuild;
 mod sql;
+pub(crate) mod sqlserver;
 
 use std::collections::BTreeMap;
 
```

- [ ] **Step 3: 修改 `crates/dblens-dialect/src/design/rebuild.rs`**

```diff
--- a/crates/dblens-dialect/src/design/rebuild.rs
+++ b/crates/dblens-dialect/src/design/rebuild.rs
@@ -95,7 +95,7 @@ impl Dialect {
         format!("{}_{}{SUFFIX}", &table[..end], short_hash(table))
     }
 
-    /// 建表语句末尾的存储属性：MySQL 的引擎、字符集、排序规则，其他库的表空间。
+    /// 建表语句末尾的存储属性：MySQL 的引擎、字符集、排序规则，SQL Server 的文件组，其他库的表空间。
     fn storage_clause(&self, s: &TableStorage) -> String {
         match self.family() {
             Family::MySql => {
@@ -111,6 +111,9 @@ impl Dialect {
                 }
                 out
             }
+            Family::SqlServer => {
+                s.tablespace.as_ref().map(|t| format!(" ON {}", self.ident(t))).unwrap_or_default()
+            }
             _ => s.tablespace.as_ref().map(|t| format!(" TABLESPACE {}", self.ident(t))).unwrap_or_default(),
         }
     }
@@ -199,13 +202,14 @@ impl Dialect {
                 src
             });
         }
-        let dameng_identity =
-            self.family() == Family::Dameng && draft.columns.iter().any(|c| c.auto_increment);
-        if dameng_identity {
+        // 达梦、SQL Server 往自增列写显式值要打开 IDENTITY_INSERT；自增列是新加的（不复制）时不能打开
+        let identity_insert = matches!(self.family(), Family::Dameng | Family::SqlServer)
+            && draft.columns.iter().any(|c| c.auto_increment && old_of(&c.name).is_some());
+        if identity_insert {
             sql(format!("SET IDENTITY_INSERT {q_tmp} ON"));
         }
         sql(format!("INSERT INTO {q_tmp} ({}) SELECT {} FROM {q}", targets.join(", "), sources.join(", ")));
-        if dameng_identity {
+        if identity_insert {
             sql(format!("SET IDENTITY_INSERT {q_tmp} OFF"));
         }
 
@@ -228,11 +232,23 @@ impl Dialect {
         let drop_at = steps.len();
         let mut sql = |s: String| steps.push(RebuildStep::Sql { sql: s });
         sql(format!("DROP TABLE {q}"));
-        sql(if mysql {
-            format!("ALTER TABLE {q_tmp} RENAME TO {q}")
-        } else {
-            format!("ALTER TABLE {q_tmp} RENAME TO {}", self.ident(name))
-        });
+        sql(self.rename_table_sql(schema, &tmp, name)?);
+        // SQL Server：新表上的默认值约束按临时表名命名（约束名在 schema 内不能重复），改回按原表名命名，
+        // 否则下次重建同一张表时会重名
+        if self.family() == Family::SqlServer {
+            for c in draft
+                .columns
+                .iter()
+                .filter(|c| c.default.is_some() && c.generated.is_none() && !c.auto_increment)
+            {
+                let from = self.qualified(Some(schema), &super::sqlserver::df_name(&tmp, &c.name));
+                sql(format!(
+                    "EXEC sp_rename {}, {}, N'OBJECT'",
+                    self.literal(&Value::String(from))?,
+                    self.literal(&Value::String(super::sqlserver::df_name(name, &c.name)))?
+                ));
+            }
+        }
 
         // 6. 约束、索引、外键、触发器
         let mut changes = Vec::new();
@@ -295,10 +311,18 @@ impl Dialect {
             }
         }
         let mut warnings = vec![msg!("design.rebuild.no_writes")];
-        if matches!(self.family(), Family::Oracle | Family::Dameng) && !ctx.views.is_empty() {
+        if matches!(self.family(), Family::Oracle | Family::Dameng | Family::SqlServer)
+            && !ctx.views.is_empty()
+        {
             for v in &ctx.views {
-                let kind = if v.materialized { "MATERIALIZED VIEW" } else { "VIEW" };
-                sql(format!("ALTER {kind} {} COMPILE", self.qualified(Some(&v.schema), &v.name)));
+                let qv = self.qualified(Some(&v.schema), &v.name);
+                if self.family() == Family::SqlServer {
+                    // 视图按名字引用表，刷新它保存的列信息
+                    sql(format!("EXEC sp_refreshview {}", self.literal(&Value::String(qv))?));
+                } else {
+                    let kind = if v.materialized { "MATERIALIZED VIEW" } else { "VIEW" };
+                    sql(format!("ALTER {kind} {qv} COMPILE"));
+                }
             }
             warnings.push(msg!("design.rebuild.views_recompiled", views = view_list()));
         }
@@ -413,6 +437,13 @@ mod tests {
                 "VARCHAR(50)",
                 "CREATE OR REPLACE TRIGGER trg BEFORE INSERT ON orders FOR EACH ROW BEGIN NULL; END;",
             ),
+            (
+                Variant::SqlServer,
+                "dbo",
+                ["bigint", "nvarchar(20)", "decimal(10,2)"],
+                "nvarchar(50)",
+                "CREATE TRIGGER dbo.trg ON dbo.orders AFTER INSERT AS BEGIN SET NOCOUNT ON; END",
+            ),
         ];
         for (v, schema, types, varchar, trigger) in cases {
             let d = Dialect::new(v);
@@ -422,7 +453,7 @@ mod tests {
             if v != Variant::MySql {
                 c.storage = TableStorage { tablespace: Some("USERS".into()), ..Default::default() };
             }
-            if matches!(v, Variant::Oracle | Variant::Dameng) {
+            if matches!(v, Variant::Oracle | Variant::Dameng | Variant::SqlServer) {
                 c.views = vec![DependentView {
                     schema: schema.into(),
                     name: "v_orders".into(),
```

- [ ] **Step 4: 新建 `crates/dblens-dialect/src/design/snapshots/dblens_dialect__design__rebuild__tests__rebuild_sqlserver.snap`**

```text
---
source: crates/dblens-dialect/src/design/rebuild.rs
expression: render(&out)
---
CREATE TABLE dbo.orders_dblens_new (
    amount decimal(10,2) CONSTRAINT DF_orders_dblens_new_amount DEFAULT 0 NULL,
    id bigint IDENTITY(1,1) NOT NULL,
    order_code nvarchar(50) NULL
) ON USERS
EXEC sys.sp_addextendedproperty N'MS_Description', N'订单', N'SCHEMA', N'dbo', N'TABLE', N'orders_dblens_new'
EXEC sys.sp_addextendedproperty N'MS_Description', N'金额', N'SCHEMA', N'dbo', N'TABLE', N'orders_dblens_new', N'COLUMN', N'amount'
SET IDENTITY_INSERT dbo.orders_dblens_new ON
INSERT INTO dbo.orders_dblens_new (amount, id, order_code) SELECT amount, id, CAST(code AS nvarchar(50)) FROM dbo.orders
SET IDENTITY_INSERT dbo.orders_dblens_new OFF
-- COUNT dbo.orders = dbo.orders_dblens_new
ALTER TABLE dbo.items DROP CONSTRAINT fk_items_order
▶ DROP TABLE dbo.orders
EXEC sp_rename N'dbo.orders_dblens_new', N'orders'
EXEC sp_rename N'dbo.DF_orders_dblens_new_amount', N'DF_orders_amount', N'OBJECT'
ALTER TABLE dbo.orders ADD CONSTRAINT pk_orders PRIMARY KEY (id)
ALTER TABLE dbo.orders ADD CONSTRAINT uk_code UNIQUE (order_code)
ALTER TABLE dbo.orders ADD CONSTRAINT ck_amount CHECK (amount >= 0)
CREATE INDEX ix_amount ON dbo.orders (amount DESC)
ALTER TABLE dbo.orders ADD CONSTRAINT fk_parent FOREIGN KEY (order_code) REFERENCES dbo.parent (code) ON DELETE CASCADE
CREATE TRIGGER dbo.trg ON dbo.orders AFTER INSERT AS BEGIN SET NOCOUNT ON; END;
ALTER TABLE dbo.items ADD CONSTRAINT fk_items_order FOREIGN KEY (order_code) REFERENCES dbo.orders (order_code) ON DELETE CASCADE
GRANT SELECT ON dbo.orders TO report
EXEC sp_refreshview N'dbo.v_orders'
-- cleanup: DROP TABLE dbo.orders_dblens_new
```

- [ ] **Step 5: 新建 `crates/dblens-dialect/src/design/snapshots/dblens_dialect__design__sqlserver__tests__sqlserver_alter.snap`**

```text
---
source: crates/dblens-dialect/src/design/sqlserver.rs
expression: "render(&d.design_sql(\"dbo\", \"orders\",\n&alter_only(&t, \"decimal(12,2)\")).unwrap())"
---
[0] ALTER TABLE dbo.orders ADD CONSTRAINT DF_orders_code DEFAULT 'x' FOR code
[0] IF EXISTS (SELECT 1 FROM sys.extended_properties WHERE class = 1 AND major_id = OBJECT_ID(N'dbo.orders') AND minor_id = COLUMNPROPERTY(OBJECT_ID(N'dbo.orders'), N'code', 'ColumnId') AND name = N'MS_Description')
    EXEC sys.sp_updateextendedproperty N'MS_Description', N'编号', N'SCHEMA', N'dbo', N'TABLE', N'orders', N'COLUMN', N'code';
ELSE
    EXEC sys.sp_addextendedproperty N'MS_Description', N'编号', N'SCHEMA', N'dbo', N'TABLE', N'orders', N'COLUMN', N'code';
[1] ALTER TABLE dbo.orders ALTER COLUMN amount decimal(12,2) NOT NULL
[1] DECLARE @df sysname = (SELECT dc.name FROM sys.default_constraints dc JOIN sys.columns c ON c.object_id = dc.parent_object_id AND c.column_id = dc.parent_column_id WHERE dc.parent_object_id = OBJECT_ID(N'dbo.orders') AND c.name = N'amount');
IF @df IS NOT NULL
BEGIN
    DECLARE @ddl nvarchar(max) = N'ALTER TABLE dbo.orders DROP CONSTRAINT ' + QUOTENAME(@df);
    EXEC sp_executesql @ddl;
END;
[1] IF EXISTS (SELECT 1 FROM sys.extended_properties WHERE class = 1 AND major_id = OBJECT_ID(N'dbo.orders') AND minor_id = COLUMNPROPERTY(OBJECT_ID(N'dbo.orders'), N'amount', 'ColumnId') AND name = N'MS_Description')
    EXEC sys.sp_dropextendedproperty N'MS_Description', N'SCHEMA', N'dbo', N'TABLE', N'orders', N'COLUMN', N'amount';
```

- [ ] **Step 6: 新建 `crates/dblens-dialect/src/design/snapshots/dblens_dialect__design__sqlserver__tests__sqlserver_create.snap`**

```text
---
source: crates/dblens-dialect/src/design/sqlserver.rs
expression: "render(&d.design_sql(\"dbo\", \"\", &diff).unwrap())"
---
[0] CREATE TABLE dbo.orders (
    id bigint IDENTITY(1,1) NOT NULL,
    code nvarchar(20) NULL,
    amount decimal(10,2) CONSTRAINT DF_orders_amount DEFAULT 0 NULL,
    CONSTRAINT pk_orders PRIMARY KEY (id),
    CONSTRAINT uk_code UNIQUE (code),
    CONSTRAINT fk_parent FOREIGN KEY (code) REFERENCES dbo.parent (code) ON DELETE CASCADE,
    CONSTRAINT ck_amount CHECK (amount >= 0)
)
[0] CREATE INDEX ix_amount ON dbo.orders (amount DESC)
[0] EXEC sys.sp_addextendedproperty N'MS_Description', N'订单', N'SCHEMA', N'dbo', N'TABLE', N'orders'
[0] EXEC sys.sp_addextendedproperty N'MS_Description', N'金额', N'SCHEMA', N'dbo', N'TABLE', N'orders', N'COLUMN', N'amount'
```

- [ ] **Step 7: 新建 `crates/dblens-dialect/src/design/snapshots/dblens_dialect__design__sqlserver__tests__sqlserver_everything.snap`**

```text
---
source: crates/dblens-dialect/src/design/sqlserver.rs
expression: "render(&d.design_sql(\"dbo\", \"orders\", &diff).unwrap())"
---
[0] ALTER TABLE dbo.orders DROP CONSTRAINT fk_parent
[1] ALTER TABLE dbo.orders DROP CONSTRAINT pk_orders
[2] ALTER TABLE dbo.orders DROP CONSTRAINT ck_amount
[3] DROP INDEX ix_amount ON dbo.orders
[4] DECLARE @df sysname = (SELECT dc.name FROM sys.default_constraints dc JOIN sys.columns c ON c.object_id = dc.parent_object_id AND c.column_id = dc.parent_column_id WHERE dc.parent_object_id = OBJECT_ID(N'dbo.orders') AND c.name = N'amount');
IF @df IS NOT NULL
BEGIN
    DECLARE @ddl nvarchar(max) = N'ALTER TABLE dbo.orders DROP CONSTRAINT ' + QUOTENAME(@df);
    EXEC sp_executesql @ddl;
END;
[4] ALTER TABLE dbo.orders DROP COLUMN amount
[5] EXEC sp_rename N'dbo.orders.code', N'order_code', N'COLUMN'
[6] ALTER TABLE dbo.orders ALTER COLUMN order_code nvarchar(50) NOT NULL
[7] ALTER TABLE dbo.orders ADD note nvarchar(50) NULL
[8] ALTER TABLE dbo.orders ADD CONSTRAINT pk_orders PRIMARY KEY (id, order_code)
[9] ALTER TABLE dbo.orders ADD CONSTRAINT uk_note UNIQUE (note)
[10] CREATE INDEX ix_note ON dbo.orders (note)
[11] ALTER TABLE dbo.orders ADD CONSTRAINT fk_parent FOREIGN KEY (order_code) REFERENCES dbo.parent (code)
[12] IF EXISTS (SELECT 1 FROM sys.extended_properties WHERE class = 1 AND major_id = OBJECT_ID(N'dbo.orders') AND minor_id = 0 AND name = N'MS_Description')
    EXEC sys.sp_updateextendedproperty N'MS_Description', N'订单表', N'SCHEMA', N'dbo', N'TABLE', N'orders';
ELSE
    EXEC sys.sp_addextendedproperty N'MS_Description', N'订单表', N'SCHEMA', N'dbo', N'TABLE', N'orders';
```

- [ ] **Step 8: 修改 `crates/dblens-dialect/src/design/sql.rs`**

```diff
--- a/crates/dblens-dialect/src/design/sql.rs
+++ b/crates/dblens-dialect/src/design/sql.rs
@@ -21,9 +21,9 @@ pub(crate) fn stmt(change: usize, sql: String) -> DesignStatement {
 }
 
 impl Dialect {
-    /// 整个改动能否放在一个事务中执行：只有 PG 家族的 DDL 可以回滚。
+    /// 整个改动能否放在一个事务中执行：PG 家族和 SQL Server 的 DDL 可以回滚。
     pub fn design_transactional(&self) -> bool {
-        self.family() == Family::Postgres
+        matches!(self.family(), Family::Postgres | Family::SqlServer)
     }
 
     /// 库确定不支持的改动及原因；不为空时改用重建表。
@@ -72,11 +72,22 @@ impl Dialect {
             Family::MySql => self.mysql_design(schema, table, &diff.changes),
             Family::Postgres => self.pg_design(schema, table, &diff.changes),
             Family::Oracle | Family::Dameng => self.oracle_design(schema, table, &diff.changes),
-            // 表设计器的 SQL Server 支持在 M6b
-            Family::SqlServer => Err(dblens_core::Error::invalid(dblens_core::msg!("sqlserver.later"))),
+            Family::SqlServer => self.sqlserver_design(schema, table, &diff.changes),
         }
     }
 
+    /// 表改名：MySQL `ALTER TABLE … RENAME TO 带 schema 的新名`；SQL Server `sp_rename`；其他库 `RENAME TO 新名`。
+    pub(crate) fn rename_table_sql(&self, schema: &str, from: &str, to: &str) -> Result<String> {
+        let q = self.qualified(Some(schema), from);
+        Ok(match self.family() {
+            Family::MySql => format!("ALTER TABLE {q} RENAME TO {}", self.qualified(Some(schema), to)),
+            Family::SqlServer => {
+                format!("EXEC sp_rename {}, {}", self.text_literal(&q)?, self.text_literal(to)?)
+            }
+            _ => format!("ALTER TABLE {q} RENAME TO {}", self.ident(to)),
+        })
+    }
+
     pub(crate) fn idents(&self, cols: &[String]) -> String {
         cols.iter().map(|c| self.ident(c)).collect::<Vec<_>>().join(", ")
     }
@@ -316,7 +327,14 @@ mod tests {
     #[test]
     fn every_generated_statement_splits_into_one() {
         let types = ["BIGINT", "VARCHAR(20)", "DECIMAL(10,2)"];
-        for v in [Variant::MySql, Variant::MariaDb, Variant::Postgres, Variant::Oracle, Variant::Dameng] {
+        for v in [
+            Variant::MySql,
+            Variant::MariaDb,
+            Variant::Postgres,
+            Variant::Oracle,
+            Variant::Dameng,
+            Variant::SqlServer,
+        ] {
             let d = Dialect::new(v);
             let mut t = table(types);
             // 注释、默认值里带分号、注释符和 `/`
```

- [ ] **Step 9: 新建 `crates/dblens-dialect/src/design/sqlserver.rs`**

```rust
//! SQL Server 的设计器 DDL（M6b 设计文档第 3 节）。DDL 可以回滚，全部语句在一个事务中执行。
//! 默认值是有名字的约束：设计器建的命名为 `DF_表_列`；删除时在执行时从 `sys.default_constraints` 查名字（库自动起的名字带随机后缀，
//! 各库不同）。改名用 `sp_rename`，注释是扩展属性 `MS_Description`。

use dblens_core::Result;
use dblens_core::schema::{Column, Index, Table};

use super::TableChange as C;
use super::diff::{same_type, text};
use super::sql::{DesignStatement, stmt};
use crate::Dialect;

/// 约束名的最大长度。
const MAX_NAME: usize = 128;

/// 设计器建的默认值约束的名字：`DF_表_列`，超长时截断。
pub(crate) fn df_name(table: &str, column: &str) -> String {
    format!("DF_{table}_{column}").chars().take(MAX_NAME).collect()
}

/// 计算列的表达式去掉一层外括号（库中读出的是 `([a]+[b])`，写回时再加一层）。
fn strip_outer(e: &str) -> &str {
    let e = e.trim();
    match e.strip_prefix('(').and_then(|s| s.strip_suffix(')')) {
        Some(inner) if balanced(inner) => inner,
        _ => e,
    }
}

fn balanced(s: &str) -> bool {
    let mut depth = 0i32;
    for ch in s.chars() {
        match ch {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth < 0 {
                    return false;
                }
            }
            _ => {}
        }
    }
    depth == 0
}

impl Dialect {
    /// 列定义：计算列写 `名 AS (表达式)`；否则类型、自增、带名字的默认值、可空性（总是写明，不依赖会话的 ANSI_NULL_DFLT 设置）。
    fn ms_column(&self, table: &str, c: &Column) -> String {
        let name = self.ident(&c.name);
        if let Some(g) = text(&c.generated) {
            return format!("{name} AS ({})", strip_outer(g));
        }
        let mut s = format!("{name} {}", c.data_type.native);
        if c.auto_increment {
            s += " IDENTITY(1,1)";
        } else if let Some(d) = text(&c.default) {
            s += &format!(" CONSTRAINT {} DEFAULT {d}", self.ident(&df_name(table, &c.name)));
        }
        s += if c.nullable { " NULL" } else { " NOT NULL" };
        s
    }

    /// 删除列上的默认值约束（没有时什么也不做）：执行时按列查出约束名，拼成语句后用 `sp_executesql` 执行
    /// （`EXEC (…)` 的括号里只能拼字面量和变量，不能调用 `QUOTENAME`）。
    fn ms_drop_default(&self, schema: &str, table: &str, column: &str) -> Result<String> {
        let q = self.qualified(Some(schema), table);
        Ok(format!(
            "DECLARE @df sysname = (SELECT dc.name FROM sys.default_constraints dc \
             JOIN sys.columns c ON c.object_id = dc.parent_object_id AND c.column_id = dc.parent_column_id \
             WHERE dc.parent_object_id = OBJECT_ID({}) AND c.name = {});\n\
             IF @df IS NOT NULL\nBEGIN\n    \
             DECLARE @ddl nvarchar(max) = {} + QUOTENAME(@df);\n    \
             EXEC sp_executesql @ddl;\nEND;",
            self.text_literal(&q)?,
            self.text_literal(column)?,
            self.text_literal(&format!("ALTER TABLE {q} DROP CONSTRAINT "))?,
        ))
    }

    /// 表或列的注释（扩展属性 `MS_Description`）：有注释时已存在则更新、不存在则添加；没有注释时存在则删除。
    fn ms_comment(
        &self,
        schema: &str,
        table: &str,
        column: Option<&str>,
        comment: Option<&str>,
    ) -> Result<String> {
        let object = self.text_literal(&self.qualified(Some(schema), table))?;
        let minor = match column {
            Some(c) => format!("COLUMNPROPERTY(OBJECT_ID({object}), {}, 'ColumnId')", self.text_literal(c)?),
            None => "0".into(),
        };
        let mut target =
            format!("N'SCHEMA', {}, N'TABLE', {}", self.text_literal(schema)?, self.text_literal(table)?);
        if let Some(c) = column {
            target += &format!(", N'COLUMN', {}", self.text_literal(c)?);
        }
        let exists = format!(
            "IF EXISTS (SELECT 1 FROM sys.extended_properties WHERE class = 1 AND major_id = OBJECT_ID({object}) \
             AND minor_id = {minor} AND name = N'MS_Description')"
        );
        Ok(match comment {
            Some(text) => {
                let value = self.text_literal(text)?;
                format!(
                    "{exists}\n    EXEC sys.sp_updateextendedproperty N'MS_Description', {value}, {target};\n\
                     ELSE\n    EXEC sys.sp_addextendedproperty N'MS_Description', {value}, {target};"
                )
            }
            None => format!("{exists}\n    EXEC sys.sp_dropextendedproperty N'MS_Description', {target};"),
        })
    }

    /// 新建对象上的注释：一定不存在，直接添加。
    fn ms_add_comment(
        &self,
        schema: &str,
        table: &str,
        column: Option<&str>,
        comment: &str,
    ) -> Result<String> {
        let mut s = format!(
            "EXEC sys.sp_addextendedproperty N'MS_Description', {}, N'SCHEMA', {}, N'TABLE', {}",
            self.text_literal(comment)?,
            self.text_literal(schema)?,
            self.text_literal(table)?
        );
        if let Some(c) = column {
            s += &format!(", N'COLUMN', {}", self.text_literal(c)?);
        }
        Ok(s)
    }

    /// `sp_rename`：`what` 为 `COLUMN` 或 `INDEX`，`object` 是 `[架构].[表].[名]`。
    fn ms_rename(&self, object: &str, to: &str, what: &str) -> Result<String> {
        Ok(format!("EXEC sp_rename {}, {}, N'{what}'", self.text_literal(object)?, self.text_literal(to)?))
    }

    fn ms_index(&self, schema: &str, table: &str, x: &Index) -> String {
        let method = x
            .method
            .as_deref()
            .map(str::to_ascii_uppercase)
            .filter(|m| m == "CLUSTERED" || m == "NONCLUSTERED")
            .map(|m| format!("{m} "))
            .unwrap_or_default();
        format!(
            "CREATE {}{method}INDEX {} ON {} ({})",
            if x.unique { "UNIQUE " } else { "" },
            self.ident(&x.name),
            self.qualified(Some(schema), table),
            self.index_items(x)
        )
    }

    fn ms_create(&self, schema: &str, t: &Table) -> Result<Vec<String>> {
        let q = self.qualified(Some(schema), &t.name);
        let mut items: Vec<String> = t.columns.iter().map(|c| self.ms_column(&t.name, c)).collect();
        if let Some(pk) = &t.primary_key {
            items.push(format!(
                "{}PRIMARY KEY ({})",
                self.constraint_name(&pk.name),
                self.idents(&pk.columns)
            ));
        }
        for u in &t.uniques {
            items.push(format!("{}UNIQUE ({})", self.constraint_name(&u.name), self.idents(&u.columns)));
        }
        for f in &t.foreign_keys {
            items.push(format!("{}{}", self.constraint_name(&f.name), self.fk_clause(schema, f)));
        }
        for c in &t.checks {
            items.push(self.check_clause(c));
        }
        let mut out = vec![format!("CREATE TABLE {q} (\n    {}\n)", items.join(",\n    "))];
        for x in &t.indexes {
            out.push(self.ms_index(schema, &t.name, x));
        }
        if let Some(c) = text(&t.comment) {
            out.push(self.ms_add_comment(schema, &t.name, None, c)?);
        }
        for c in &t.columns {
            if let Some(cm) = text(&c.comment) {
                out.push(self.ms_add_comment(schema, &t.name, Some(&c.name), cm)?);
            }
        }
        Ok(out)
    }

    /// 修改已有列。自增和计算列的修改由 `design_unsupported` 改为重建。
    fn ms_alter_column(&self, schema: &str, table: &str, from: &Column, to: &Column) -> Result<Vec<String>> {
        let q = self.qualified(Some(schema), table);
        let col = self.ident(&to.name);
        let mut out = Vec::new();
        if !same_type(&from.data_type.native, &to.data_type.native) || from.nullable != to.nullable {
            // 可空性必须写明，否则按会话设置可能变成可空
            out.push(format!(
                "ALTER TABLE {q} ALTER COLUMN {col} {}{}",
                to.data_type.native,
                if to.nullable { " NULL" } else { " NOT NULL" }
            ));
        }
        if text(&from.default) != text(&to.default) && to.generated.is_none() && !to.auto_increment {
            if text(&from.default).is_some() {
                out.push(self.ms_drop_default(schema, table, &to.name)?);
            }
            if let Some(d) = text(&to.default) {
                out.push(format!(
                    "ALTER TABLE {q} ADD CONSTRAINT {} DEFAULT {d} FOR {col}",
                    self.ident(&df_name(table, &to.name))
                ));
            }
        }
        if text(&from.comment) != text(&to.comment) {
            out.push(self.ms_comment(schema, table, Some(&to.name), text(&to.comment))?);
        }
        Ok(out)
    }

    pub(crate) fn sqlserver_design(
        &self,
        schema: &str,
        table: &str,
        changes: &[C],
    ) -> Result<Vec<DesignStatement>> {
        let q = self.qualified(Some(schema), table);
        let alter = |clause: String| format!("ALTER TABLE {q} {clause}");
        let drop_constraint = |name: &Option<String>| -> Result<String> {
            Ok(alter(format!("DROP CONSTRAINT {}", self.ident(self.required_name(name)?))))
        };
        let member = |name: &str| format!("{q}.{}", self.ident(name));
        let mut out = Vec::new();
        for (i, ch) in changes.iter().enumerate() {
            let sqls: Vec<String> = match ch {
                C::CreateTable { table } => self.ms_create(schema, table)?,
                C::SetTableComment { comment } => {
                    vec![self.ms_comment(schema, table, None, text(comment))?]
                }
                C::AddColumn { column, .. } => {
                    let mut v = vec![alter(format!("ADD {}", self.ms_column(table, column)))];
                    if let Some(cm) = text(&column.comment) {
                        v.push(self.ms_add_comment(schema, table, Some(&column.name), cm)?);
                    }
                    v
                }
                C::DropColumn { name } => {
                    vec![
                        self.ms_drop_default(schema, table, name)?,
                        alter(format!("DROP COLUMN {}", self.ident(name))),
                    ]
                }
                C::RenameColumn { from, to, .. } => vec![self.ms_rename(&member(from), to, "COLUMN")?],
                C::AlterColumn { from, to } => self.ms_alter_column(schema, table, from, to)?,
                // 由 design_unsupported 改为重建，不会走到这里
                C::MoveColumn { .. } => vec![],
                C::DropPrimaryKey { key } => vec![drop_constraint(&key.name)?],
                C::AddPrimaryKey { key } => vec![alter(format!(
                    "ADD {}PRIMARY KEY ({})",
                    self.constraint_name(&key.name),
                    self.idents(&key.columns)
                ))],
                C::DropUnique { unique } => vec![drop_constraint(&unique.name)?],
                C::AddUnique { unique } => vec![alter(format!(
                    "ADD {}UNIQUE ({})",
                    self.constraint_name(&unique.name),
                    self.idents(&unique.columns)
                ))],
                C::DropCheck { check } => vec![drop_constraint(&check.name)?],
                C::AddCheck { check } => vec![alter(format!("ADD {}", self.check_clause(check)))],
                C::DropForeignKey { key } => vec![drop_constraint(&key.name)?],
                C::AddForeignKey { key } => {
                    vec![alter(format!(
                        "ADD {}{}",
                        self.constraint_name(&key.name),
                        self.fk_clause(schema, key)
                    ))]
                }
                C::DropIndex { index } => vec![format!("DROP INDEX {} ON {q}", self.ident(&index.name))],
                C::AddIndex { index } => vec![self.ms_index(schema, table, index)],
                C::RenameIndex { from, index } => {
                    vec![self.ms_rename(&member(from), &index.name, "INDEX")?]
                }
            };
            out.extend(sqls.into_iter().map(|s| stmt(i, s)));
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::super::sql::fixtures::*;
    use super::*;
    use crate::Variant;
    use crate::design::{Renames, diff_table};

    fn appended(mut diff: crate::design::Diff) -> crate::design::Diff {
        // 新列插在中间会改为重建；这里把 note 放到最后，覆盖其余每种变更
        for c in &mut diff.changes {
            if let C::AddColumn { after, .. } = c {
                *after = Some("order_code".into());
            }
        }
        diff
    }

    #[test]
    fn sqlserver() {
        let d = Dialect::new(Variant::SqlServer);
        let t = table(["bigint", "nvarchar(20)", "decimal(10,2)"]);
        let diff = diff_table(None, &t, &Renames::default()).unwrap();
        insta::assert_snapshot!("sqlserver_create", render(&d.design_sql("dbo", "", &diff).unwrap()));
        let diff = appended(everything(&t, "nvarchar(50)"));
        insta::assert_snapshot!(
            "sqlserver_everything",
            render(&d.design_sql("dbo", "orders", &diff).unwrap())
        );
        insta::assert_snapshot!(
            "sqlserver_alter",
            render(&d.design_sql("dbo", "orders", &alter_only(&t, "decimal(12,2)")).unwrap())
        );
        assert!(d.design_transactional());
    }

    #[test]
    fn names_and_expressions() {
        assert_eq!(df_name("orders", "amount"), "DF_orders_amount");
        assert_eq!(df_name(&"t".repeat(200), "c").chars().count(), MAX_NAME);
        assert_eq!(strip_outer("([a]+[b])"), "[a]+[b]");
        assert_eq!(strip_outer("(a)+(b)"), "(a)+(b)");
        let d = Dialect::new(Variant::SqlServer);
        let mut c = col("total", "int");
        c.generated = Some("([a]*(2))".into());
        assert_eq!(d.ms_column("t", &c), "total AS ([a]*(2))");
        // 名字中的单引号在字符串字面量中转义
        let s = d.ms_drop_default("dbo", "o'k", "c").unwrap();
        assert!(s.contains("OBJECT_ID(N'dbo.[o''k]')"), "{s}");
        assert!(
            s.contains(
                "DECLARE @ddl nvarchar(max) = N'ALTER TABLE dbo.[o''k] DROP CONSTRAINT ' + QUOTENAME(@df);"
            ),
            "{s}"
        );
    }
}
```

- [ ] **Step 10: 检查**

```bash
export CARGO_INCREMENTAL=0
cargo fmt --all -- --check
cargo clippy --config "source.mirror.registry='sparse+https://rsproxy.cn/index/'" --workspace --all-targets -- -D warnings
DBLENS_LANG=en cargo test --config "source.mirror.registry='sparse+https://rsproxy.cn/index/'" --workspace
```

Expected: 全部测试通过（新增 SQL Server 的建表、全部变更、改列、重建快照，名字与表达式的单元测试）。快照文件随本任务给出。

- [ ] **Step 11: 提交**

```bash
git add crates/dblens-dialect/src/design/diff.rs crates/dblens-dialect/src/design/mod.rs crates/dblens-dialect/src/design/rebuild.rs crates/dblens-dialect/src/design/snapshots/dblens_dialect__design__rebuild__tests__rebuild_sqlserver.snap crates/dblens-dialect/src/design/snapshots/dblens_dialect__design__sqlserver__tests__sqlserver_alter.snap crates/dblens-dialect/src/design/snapshots/dblens_dialect__design__sqlserver__tests__sqlserver_create.snap crates/dblens-dialect/src/design/snapshots/dblens_dialect__design__sqlserver__tests__sqlserver_everything.snap crates/dblens-dialect/src/design/sql.rs crates/dblens-dialect/src/design/sqlserver.rs
git commit -m "feat(dialect): SQL Server 表设计器的变更语句与重建" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 3: 后端：导入、比对与模型、文档导出、迁移（含库名的传递和实库测试）

- 方言层：
  - `import.rs`：`Dialect::max_params()`（SQL Server 2000）；SQL Server 的按主键更新一批一条多行 `MERGE`（不更新自增列，结尾分号）；SQL Server 的 NULL 直接写进语句；
  - `changes.rs`：表数据编辑中 SQL Server 的 NULL 直接写进语句；
  - `variant.rs`：`Family::terminate`；`lib.rs`：`Dialect::schema_switch_sql`；`compare/sync.rs`：表改名用 `rename_table_sql`；
  - `migrate/script.rs`：`script_text(target: Family, …)`；序列的 SQL Server 写法；默认值 `NEXT VALUE FOR` 补 schema；
  - `migrate/expr.rs`：SQL Server 的布尔、时间、UUID 默认值；来源表达式中的 `[列]`、`N'…'`；`SYSDATETIME`、`SYSDATETIMEOFFSET`；
  - `migrate/tests.rs`：PG → SQL Server 的快照。
- 服务层：`Service::scoped_profile(id, catalog)` 替代 `feature_profile`（不再拦截，删去词条 `sqlserver.later`）；设计器、导入、比对、文档导出、迁移、反向生成模型的请求加可选的 `catalog`；
  设计器和迁移执行前用 `schema_switch_sql`；导入映射了自增列时包上 `IDENTITY_INSERT`；`migrate/values.rs` 时间按目标精度截断；比对和迁移的脚本按目标结束语句。
- 接口：`/design/table?catalog=`，建表语句预览的 `catalog`。
- 实库测试：设计器、导入（含自增列）、模型与比对、文档导出加 SQL Server；M6a 的"拦截"断言改为功能可用；迁移的 SQL Server 源数据与 10 条路线。

**Files:**
- Modify: `crates/dblens-api/src/design.rs`
- Modify: `crates/dblens-api/src/tasks.rs`
- Modify: `crates/dblens-api/tests/live.rs`
- Modify: `crates/dblens-core/locales/app.json`
- Modify: `crates/dblens-dialect/src/changes.rs`
- Modify: `crates/dblens-dialect/src/compare/sync.rs`
- Modify: `crates/dblens-dialect/src/import.rs`
- Modify: `crates/dblens-dialect/src/lib.rs`
- Modify: `crates/dblens-dialect/src/migrate/expr.rs`
- Modify: `crates/dblens-dialect/src/migrate/script.rs`
- Create: `crates/dblens-dialect/src/migrate/snapshots/dblens_dialect__migrate__tests__pg_to_sqlserver.snap`
- Modify: `crates/dblens-dialect/src/migrate/tests.rs`
- Modify: `crates/dblens-dialect/src/variant.rs`
- Modify: `crates/dblens-service/src/compare.rs`
- Modify: `crates/dblens-service/src/design.rs`
- Modify: `crates/dblens-service/src/document/job.rs`
- Modify: `crates/dblens-service/src/import/job.rs`
- Modify: `crates/dblens-service/src/migrate/mod.rs`
- Modify: `crates/dblens-service/src/migrate/report.rs`
- Modify: `crates/dblens-service/src/migrate/run.rs`
- Modify: `crates/dblens-service/src/migrate/values.rs`
- Modify: `crates/dblens-service/src/model/ops.rs`
- Modify: `crates/dblens-service/src/service.rs`
- Modify: `crates/dblens-service/tests/live.rs`
- Modify: `crates/dblens-service/tests/migrate_live.rs`

**Interfaces:**
- Consumes：任务 2 的 `design_sql`、`rename_table_sql`、`df_name`。
- Produces：`Family::terminate(self, sql: &str) -> String`、`Dialect::schema_switch_sql(&self, schema: &str) -> Option<String>`、`Dialect::max_params(&self) -> usize`、
  `migrate::script_text(target: Family, statements: &[MigrationStatement]) -> String`；`Service::scoped_profile(&self, id: &str, catalog: Option<&str>) -> Result<Profile>`、
  `Service::design_table(&self, profile_id, catalog: Option<&str>, schema, name)`、`Service::import_ddl(&self, profile_id, catalog: Option<&str>, schema, table, def)`；
  请求结构的 `catalog: Option<String>`：`DesignRequest`、`ImportRequest`、`CompareSideRef::Database`、`DocSource::Database`、`MigrationEnd`、`ReverseRequest`。

- [ ] **Step 1: 修改 `crates/dblens-api/tests/live.rs`**

```diff
--- a/crates/dblens-api/tests/live.rs
+++ b/crates/dblens-api/tests/live.rs
@@ -222,9 +222,15 @@ async fn sqlserver_catalogs() {
         call(&app, "GET", &format!("/api/profiles/{id}/schemas/sys/objects?catalog=msdb&kind=views"), None)
             .await;
     assert_eq!(st, StatusCode::OK, "{t}");
-    let (st, e) =
-        call(&app, "GET", &format!("/api/profiles/{id}/design/table?schema=dbo&name=t"), None).await;
-    assert_eq!(st, StatusCode::BAD_REQUEST, "{e}");
+    // M6b 起表设计器可用：按库读不存在的表是 404
+    let (st, e) = call(
+        &app,
+        "GET",
+        &format!("/api/profiles/{id}/design/table?catalog=master&schema=dbo&name=dblens_no_such"),
+        None,
+    )
+    .await;
+    assert_eq!(st, StatusCode::NOT_FOUND, "{e}");
 }
 
 #[tokio::test]
```

- [ ] **Step 2: 修改 `crates/dblens-service/tests/live.rs`**

```diff
--- a/crates/dblens-service/tests/live.rs
+++ b/crates/dblens-service/tests/live.rs
@@ -91,6 +91,11 @@ async fn setup(env: &str) -> Option<Live> {
 }
 
 impl Live {
+    /// M6b 的功能请求中的库名：SQL Server 是连接的库，其他库不填。
+    fn catalog(&self) -> Option<String> {
+        if self.driver == DriverId::SqlServer { self.database.clone() } else { None }
+    }
+
     fn dual(&self, expr: &str) -> String {
         match self.driver {
             DriverId::Oracle | DriverId::Dameng => format!("SELECT {expr} FROM DUAL"),
@@ -333,7 +338,7 @@ async fn sqlserver_export() {
 /// SQL Server：库列表、按库浏览（元数据连接以所选库为初始库）、执行计划后会话恢复正常、
 /// 一批里的多个结果集、M6b 之前的功能被拒绝。
 #[tokio::test]
-async fn sqlserver_catalogs_batches_and_gating() {
+async fn sqlserver_catalogs_and_batches() {
     let Some(live) = setup("DBLENS_TEST_SQLSERVER_URL").await else { return };
     let s = &live.service;
     let profile = s.profile(&live.profile_id).unwrap();
@@ -359,7 +364,9 @@ async fn sqlserver_catalogs_batches_and_gating() {
     assert_eq!(starts, 3, "{events:?}");
     s.sessions.close(id, false).await.unwrap();
 
-    assert!(matches!(s.design_table(&live.profile_id, "dbo", "t").await, Err(Error::Invalid { .. })));
+    // M6b 起写入类功能不再拦截：读不存在的表是"找不到"
+    let missing = s.design_table(&live.profile_id, live.catalog().as_deref(), "dbo", "dblens_no_such").await;
+    assert!(matches!(missing, Err(Error::NotFound { .. })), "{missing:?}");
     s.delete_profile(&live.profile_id).await.unwrap();
 }
 
@@ -968,6 +975,10 @@ async fn import_flow(env: &str) {
         DriverId::Oracle => {
             "CREATE TABLE m2b_t (id NUMBER(10) PRIMARY KEY, name VARCHAR2(40), price NUMBER(10, 2), on_day DATE, note VARCHAR2(40))"
         }
+        // 非 Unicode 的 VARCHAR 在默认排序规则下存不了中文
+        DriverId::SqlServer => {
+            "CREATE TABLE m2b_t (id INT PRIMARY KEY, name NVARCHAR(40), price DECIMAL(10, 2), on_day DATE, note NVARCHAR(40))"
+        }
         _ => {
             "CREATE TABLE m2b_t (id INT PRIMARY KEY, name VARCHAR(40), price DECIMAL(10, 2), on_day DATE, note VARCHAR(40))"
         }
@@ -980,6 +991,7 @@ async fn import_flow(env: &str) {
     };
     let request = |source: FileSource, mode: WriteMode| ImportRequest {
         profile_id: live.profile_id.clone(),
+        catalog: live.catalog(),
         source,
         file: FileOptions::csv(),
         schema: live.schema.clone(),
@@ -1109,6 +1121,28 @@ async fn import_flow(env: &str) {
     let gone = live.submit(id, ExecRequest::new("SELECT COUNT(*) FROM m2b_new")).await;
     assert!(gone.iter().any(|e| matches!(e, SessionEvent::Failed { .. })), "{env}：新建的表应已删除");
 
+    // 自增列（M6b）：达梦、SQL Server 往自增列写显式值要打开 IDENTITY_INSERT；追加和按主键更新都要能写
+    if matches!(live.driver, DriverId::Dameng | DriverId::SqlServer) {
+        live.submit(id, ExecRequest::new("DROP TABLE IF EXISTS m2b_id")).await;
+        let text_type = if live.driver == DriverId::SqlServer { "NVARCHAR(40)" } else { "VARCHAR(40)" };
+        live.run(id, &format!("CREATE TABLE m2b_id (id INT IDENTITY(1,1) PRIMARY KEY, name {text_type})"))
+            .await;
+        let id_req = |source: FileSource, mode: WriteMode| ImportRequest {
+            table: n("m2b_id"),
+            mapping: mapping(&["id", "name"]),
+            ..request(source, mode)
+        };
+        let st = run(id_req(upload(s, "i.csv", "编号,名称\n5,五\n9,九\n"), WriteMode::Append)).await;
+        assert_eq!((st.state, st.progress.rows_written), (TaskState::Done, 2), "{env}：{st:?}");
+        let st = run(id_req(upload(s, "u.csv", "编号,名称\n9,玖\n12,十二\n"), WriteMode::Upsert)).await;
+        assert_eq!(st.state, TaskState::Done, "{env}：{st:?}");
+        assert_eq!(one("SELECT name FROM m2b_id WHERE id = 9".into()).await.as_deref(), Some("玖"), "{env}");
+        // 写入显式值后自增接着最大值往下走
+        live.run(id, "INSERT INTO m2b_id (name) VALUES ('x')").await;
+        assert_eq!(one("SELECT MAX(id) FROM m2b_id".into()).await.as_deref(), Some("13"), "{env}");
+        live.submit(id, ExecRequest::new("DROP TABLE m2b_id")).await;
+    }
+
     for sql in &drop {
         live.submit(id, ExecRequest::new(*sql)).await;
     }
@@ -1140,6 +1174,11 @@ async fn dameng_import() {
     import_flow("DBLENS_TEST_DM_URL").await;
 }
 
+#[tokio::test]
+async fn sqlserver_import() {
+    import_flow("DBLENS_TEST_SQLSERVER_URL").await;
+}
+
 /// 执行 SQL 文件（M2b）：各方言的程序体写法（MySQL 的 DELIMITER、PG 的 $$、Oracle/达梦的 PL/SQL 块加 `/`），
 /// 出错时停止和继续。
 async fn sql_file_flow(env: &str) {
@@ -1304,7 +1343,6 @@ async fn design_flow(env: &str) {
         DriverId::Postgres => ["bigint", "varchar(20)", "numeric(10,2)", "varchar(40)"],
         DriverId::Oracle => ["NUMBER(19)", "VARCHAR2(20)", "NUMBER(10,2)", "VARCHAR2(40)"],
         DriverId::Dameng => ["BIGINT", "VARCHAR(20)", "DECIMAL(10,2)", "VARCHAR(40)"],
-        // 表设计器的 SQL Server 支持在 M6b，这里只为穷尽分支
         DriverId::SqlServer => ["BIGINT", "NVARCHAR(20)", "DECIMAL(10,2)", "NVARCHAR(40)"],
     };
     let col = |name: &str, native: &str| Column {
@@ -1339,13 +1377,18 @@ async fn design_flow(env: &str) {
         }],
         ..Default::default()
     };
-    let request = |name: Option<&str>,
-                   draft: Table,
-                   renames: Renames,
-                   rebuild: bool,
-                   fingerprint: Option<String>| {
-        DesignRequest { schema: live.schema.clone(), name: name.map(n), draft, renames, rebuild, fingerprint }
-    };
+    let request =
+        |name: Option<&str>, draft: Table, renames: Renames, rebuild: bool, fingerprint: Option<String>| {
+            DesignRequest {
+                catalog: live.catalog(),
+                schema: live.schema.clone(),
+                name: name.map(n),
+                draft,
+                renames,
+                rebuild,
+                fingerprint,
+            }
+        };
     let apply = |req: DesignRequest| async move {
         let task = s.start_design(&live.profile_id, req).await.unwrap();
         wait_task(s, &task).await
@@ -1363,7 +1406,8 @@ async fn design_flow(env: &str) {
         live.run(id, &format!("INSERT INTO m3_t (code, amount, note) VALUES ('{code}', {amount}, '{note}')"))
             .await;
     }
-    live.run(id, "CREATE TABLE m3_items (item_id INT PRIMARY KEY, order_code VARCHAR(20))").await;
+    // 外键两端的类型要相同（SQL Server 的 NVARCHAR 不能引用 VARCHAR）
+    live.run(id, &format!("CREATE TABLE m3_items (item_id INT PRIMARY KEY, order_code {})", types[1])).await;
     live.run(
         id,
         "ALTER TABLE m3_items ADD CONSTRAINT fk_m3_items FOREIGN KEY (order_code) REFERENCES m3_t (code)",
@@ -1372,7 +1416,8 @@ async fn design_flow(env: &str) {
     live.run(id, "INSERT INTO m3_items VALUES (1, 'a')").await;
     live.run(id, "CREATE VIEW m3_v AS SELECT id, code FROM m3_t").await;
     // 2. 修改：note 改名为 remark 并加长，amount 不可空、改默认值和注释，换索引，加列 extra，改表注释
-    let t = s.design_table(&live.profile_id, &live.schema, &n("m3_t")).await.unwrap();
+    let t =
+        s.design_table(&live.profile_id, live.catalog().as_deref(), &live.schema, &n("m3_t")).await.unwrap();
     assert_eq!(t.table.columns.len(), 4, "{env}");
     assert!(t.table.columns[0].auto_increment, "{env}：{:?}", t.table.columns[0]);
     let mut d = t.table.clone();
@@ -1402,7 +1447,11 @@ async fn design_flow(env: &str) {
     assert!(p.errors.is_empty() && !p.rebuild.active && p.rebuild.reasons.is_empty(), "{env}：{p:?}");
     let st = apply(req).await;
     assert_eq!(st.state, TaskState::Done, "{env}：{st:?}");
-    let after = s.design_table(&live.profile_id, &live.schema, &n("m3_t")).await.unwrap().table;
+    let after = s
+        .design_table(&live.profile_id, live.catalog().as_deref(), &live.schema, &n("m3_t"))
+        .await
+        .unwrap()
+        .table;
     // 读回的结构与草稿一致（默认值的写法各库不同，只比较是否有默认值）
     let left = diff_table(Some(&after), &d, &Renames::default()).unwrap().changes;
     let left: Vec<_> = left
@@ -1437,7 +1486,8 @@ async fn design_flow(env: &str) {
     }
 
     // 3. 原表被别人改过：拒绝执行
-    let t = s.design_table(&live.profile_id, &live.schema, &n("m3_t")).await.unwrap();
+    let t =
+        s.design_table(&live.profile_id, live.catalog().as_deref(), &live.schema, &n("m3_t")).await.unwrap();
     live.run(id, "ALTER TABLE m3_t ADD tmp_col INT").await;
     let mut d = t.table.clone();
     d.comment = Some("冲突".into());
@@ -1455,7 +1505,8 @@ async fn design_flow(env: &str) {
     if live.driver != DriverId::Mysql {
         live.run(id, "GRANT SELECT ON m3_t TO PUBLIC").await;
     }
-    let t = s.design_table(&live.profile_id, &live.schema, &n("m3_t")).await.unwrap();
+    let t =
+        s.design_table(&live.profile_id, live.catalog().as_deref(), &live.schema, &n("m3_t")).await.unwrap();
     let mut d = t.table.clone();
     let amount = d.columns.remove(2);
     d.columns.insert(0, amount);
@@ -1483,7 +1534,11 @@ async fn design_flow(env: &str) {
     assert!(p.rebuild.refused.is_none(), "{env}：{p:?}");
     let st = apply(req).await;
     assert_eq!(st.state, TaskState::Done, "{env}：{st:?}");
-    let after = s.design_table(&live.profile_id, &live.schema, &n("m3_t")).await.unwrap().table;
+    let after = s
+        .design_table(&live.profile_id, live.catalog().as_deref(), &live.schema, &n("m3_t"))
+        .await
+        .unwrap()
+        .table;
     let names: Vec<&str> = after.columns.iter().map(|c| c.name.as_str()).collect();
     assert_eq!(
         names,
@@ -1531,24 +1586,40 @@ async fn design_flow(env: &str) {
             MetaArgs { schema: Some(live.schema.clone()), table: Some(n("m3_t")), ..Default::default() };
         let rows = s.metadata.rows(&profile, MetaKey::Grants, &args).await.unwrap().unwrap();
         let grants = dblens_dialect::meta::grants(&rows).unwrap();
-        assert!(grants.iter().any(|g| g.grantee == "PUBLIC" && g.privilege == "SELECT"), "{env}：{grants:?}");
+        // SQL Server 的角色名是小写的 public
+        assert!(
+            grants.iter().any(|g| g.grantee.eq_ignore_ascii_case("PUBLIC") && g.privilege == "SELECT"),
+            "{env}：{grants:?}"
+        );
     }
     // 其他表指向本表的外键还在
-    let items = s.design_table(&live.profile_id, &live.schema, &n("m3_items")).await.unwrap().table;
+    let items = s
+        .design_table(&live.profile_id, live.catalog().as_deref(), &live.schema, &n("m3_items"))
+        .await
+        .unwrap()
+        .table;
     assert_eq!(items.foreign_keys.len(), 1, "{env}");
     if live.driver != DriverId::Postgres {
         assert_eq!(one("SELECT COUNT(*) FROM m3_v".into()).await.as_deref(), Some("4"), "{env}：视图仍可用");
     }
 
-    // 5. 中途失败：加列后检查约束出错。PG 全部回滚；其他库已执行的保留
-    let t = s.design_table(&live.profile_id, &live.schema, &n("m3_t")).await.unwrap();
+    // 5. 中途失败：加列后检查约束出错。PG、SQL Server 全部回滚；其他库已执行的保留
+    let t =
+        s.design_table(&live.profile_id, live.catalog().as_deref(), &live.schema, &n("m3_t")).await.unwrap();
     let mut d = t.table.clone();
     d.columns.push(col("z", types[1]));
     d.checks.push(CheckConstraint { name: Some(n("ck_m3_bad")), expression: "no_such_column > 0".into() });
     let st = apply(request(Some("m3_t"), d, Renames::default(), false, Some(t.fingerprint))).await;
     assert_eq!(st.state, TaskState::Failed, "{env}：{st:?}");
-    let cols = s.design_table(&live.profile_id, &live.schema, &n("m3_t")).await.unwrap().table.columns.len();
-    assert_eq!(cols, if live.driver == DriverId::Postgres { 5 } else { 6 }, "{env}");
+    let cols = s
+        .design_table(&live.profile_id, live.catalog().as_deref(), &live.schema, &n("m3_t"))
+        .await
+        .unwrap()
+        .table
+        .columns
+        .len();
+    let rolled_back = matches!(live.driver, DriverId::Postgres | DriverId::SqlServer);
+    assert_eq!(cols, if rolled_back { 5 } else { 6 }, "{env}");
 
     cleanup().await;
     if live.driver == DriverId::Postgres {
@@ -1582,6 +1653,11 @@ async fn dameng_design() {
     design_flow("DBLENS_TEST_DM_URL").await;
 }
 
+#[tokio::test]
+async fn sqlserver_design() {
+    design_flow("DBLENS_TEST_SQLSERVER_URL").await;
+}
+
 /// MySQL 驱动的取消测试用的耗时查询（SLEEP() 被 KILL QUERY 打断时返回 1 而不报错）。
 /// 只看 `m4_` 开头的表（同一 schema 中其他测试的表会并发变化）。
 fn m4_items(r: &CompareResult) -> BTreeSet<u32> {
@@ -1647,6 +1723,7 @@ async fn model_flow(env: &str) {
     // 1. 反向生成
     let dir = tempfile::tempdir().unwrap();
     let req = ReverseRequest {
+        catalog: live.catalog(),
         profile_id: live.profile_id.clone(),
         schemas: vec![live.schema.clone()],
         path: dir.path().join("model").to_string_lossy().into(),
@@ -1704,7 +1781,11 @@ async fn model_flow(env: &str) {
     );
 
     // 3. 模型 → 库：按记下的改名配对
-    let db = CompareSideRef::Database { profile: live.profile_id.clone(), schema: schema.clone() };
+    let db = CompareSideRef::Database {
+        profile: live.profile_id.clone(),
+        catalog: live.catalog(),
+        schema: schema.clone(),
+    };
     let md = CompareSideRef::Model { model: m.clone(), schema: schema.clone() };
     let compare = |source: &CompareSideRef, target: &CompareSideRef| CompareRequest {
         source: source.clone(),
@@ -1759,6 +1840,7 @@ async fn model_flow(env: &str) {
 
     // 5. ER 图（M4b）：同步后的库再反向生成一次，默认图中外键连线的列对应
     let req = ReverseRequest {
+        catalog: live.catalog(),
         profile_id: live.profile_id.clone(),
         schemas: vec![schema.clone()],
         path: dir.path().join("model2").to_string_lossy().into(),
@@ -1777,7 +1859,11 @@ async fn model_flow(env: &str) {
     // 文档导出（M4c）：库的这个 schema 导出 markdown 数据字典，表和外键（链接到被引用的表）都在
     let out = dir.path().join("dict.md");
     let req = DocumentRequest {
-        source: DocSource::Database { profile: live.profile_id.clone(), schema: schema.clone() },
+        source: DocSource::Database {
+            profile: live.profile_id.clone(),
+            catalog: live.catalog(),
+            schema: schema.clone(),
+        },
         tables: None,
         kind: DocKind::Dictionary,
         format: DocFormat::Markdown,
@@ -1814,7 +1900,11 @@ async fn model_flow(env: &str) {
     if created {
         let o = other.unwrap();
         cleanup(Some(o)).await;
-        let b = CompareSideRef::Database { profile: live.profile_id.clone(), schema: o.into() };
+        let b = CompareSideRef::Database {
+            profile: live.profile_id.clone(),
+            catalog: live.catalog(),
+            schema: o.into(),
+        };
         let r = s.compare(&compare(&db, &b)).await.unwrap();
         assert!(m4_status(&r).iter().all(|(_, st)| *st == TableStatus::Added), "{env}：{:?}", m4_status(&r));
         let sync = SyncRequest { compare: compare(&db, &b), selected: m4_items(&r), fingerprint: None };
@@ -1855,6 +1945,11 @@ async fn dameng_model() {
     model_flow("DBLENS_TEST_DM_URL").await;
 }
 
+#[tokio::test]
+async fn sqlserver_model() {
+    model_flow("DBLENS_TEST_SQLSERVER_URL").await;
+}
+
 const MYSQL_SLOW: &str = "SELECT COUNT(*) FROM information_schema.columns a, information_schema.columns b, information_schema.columns c";
 
 /// 国产库第一批：与 MySQL、PostgreSQL 走同样的流程。`$ops` 是允许失败的运维查询。
```

- [ ] **Step 3: 修改 `crates/dblens-service/tests/migrate_live.rs`**

```diff
--- a/crates/dblens-service/tests/migrate_live.rs
+++ b/crates/dblens-service/tests/migrate_live.rs
@@ -31,6 +31,8 @@ struct End {
     profile: String,
     driver: DriverId,
     variant: Variant,
+    /// 所在的库：SQL Server 是连接的库，其他库不填。
+    catalog: Option<String>,
     schema: String,
 }
 
@@ -85,7 +87,8 @@ async fn setup(envs: [&str; 2]) -> Option<(Arc<Service>, Vec<End>, tempfile::Tem
         };
         let summary = service.test_connection(input.clone(), &params.password).await.unwrap();
         let profile = service.create_profile(input, Some(&params.password)).unwrap();
-        ends.push(End { profile: profile.id, driver, variant: summary.variant, schema });
+        let catalog = if driver == DriverId::SqlServer { params.database.clone() } else { None };
+        ends.push(End { profile: profile.id, driver, variant: summary.variant, catalog, schema });
     }
     Some((service, ends, dir))
 }
@@ -146,8 +149,44 @@ fn source_ddl(end: &End, p: &str) -> Vec<String> {
     let opengauss = end.variant == Variant::OpenGauss;
     let n = ROWS;
     match end.driver {
-        // SQL Server 的迁移在 M6b，路线中没有它
-        DriverId::SqlServer => vec![],
+        DriverId::SqlServer => vec![
+            format!(
+                "CREATE TABLE {p}_cust (id BIGINT IDENTITY(1,1) PRIMARY KEY, name NVARCHAR(50) NOT NULL DEFAULT 'n/a', \
+                 active BIT DEFAULT 1, created DATETIME2 DEFAULT SYSDATETIME(), amount DECIMAL(12,2) DEFAULT 0, note NVARCHAR(MAX), \
+                 CONSTRAINT {p}_ck_amount CHECK (amount >= 0), CONSTRAINT {p}_uk_name UNIQUE (name))"
+            ),
+            format!(
+                "EXEC sp_addextendedproperty N'MS_Description', N'customers', N'SCHEMA', N'dbo', N'TABLE', N'{p}_cust'"
+            ),
+            format!(
+                "EXEC sp_addextendedproperty N'MS_Description', N'name', N'SCHEMA', N'dbo', N'TABLE', N'{p}_cust', N'COLUMN', N'name'"
+            ),
+            format!("CREATE INDEX {p}_ix_created ON {p}_cust (created)"),
+            format!(
+                "CREATE TABLE {p}_ord (id INT PRIMARY KEY, cust_id BIGINT, qty INT, status NVARCHAR(10) DEFAULT 'new', \
+                 CONSTRAINT {p}_ck_qty CHECK (qty > 0), \
+                 CONSTRAINT {p}_fk_cust FOREIGN KEY (cust_id) REFERENCES {p}_cust (id) ON DELETE CASCADE)"
+            ),
+            format!(
+                "CREATE TABLE {p}_types (id INT PRIMARY KEY, txt NVARCHAR(100), big NVARCHAR(MAX), amount DECIMAL(12,2), ratio FLOAT, \
+                 flag BIT, day DATE, ts DATETIME2(3), bin VARBINARY(16))"
+            ),
+            format!("CREATE SEQUENCE {p}_seq START WITH 1"),
+            format!(
+                "DECLARE @i INT = 0, @n BIGINT; WHILE @i < 5 BEGIN SET @n = NEXT VALUE FOR {p}_seq; SET @i += 1; END"
+            ),
+            format!(
+                "WITH s(n) AS (SELECT 1 UNION ALL SELECT n + 1 FROM s WHERE n < {n}) \
+                 INSERT INTO {p}_cust (name, amount, note) SELECT CONCAT('name', n), n * 1.5, CONCAT(N'备注', n) FROM s OPTION (MAXRECURSION 0)"
+            ),
+            format!(
+                "INSERT INTO {p}_ord (id, cust_id, qty, status) VALUES (1, 1, 2, 'paid'), (2, 3, 1, 'new')"
+            ),
+            format!(
+                "INSERT INTO {p}_types VALUES (1, N'中文 abc', REPLICATE(N'y', 3000), 1234.5, 0.25, 1, '2026-01-02', '2026-01-02 03:04:05.678', 0x0102FF), \
+                 (2, N'x''q', N'z', -0.01, NULL, 0, NULL, NULL, 0x), (3, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL)"
+            ),
+        ],
         DriverId::Mysql => vec![
             format!(
                 "CREATE TABLE {p}_cust (id BIGINT AUTO_INCREMENT PRIMARY KEY, name VARCHAR(50) NOT NULL DEFAULT 'n/a' COMMENT 'name', \
@@ -325,10 +364,18 @@ async fn cleanup(s: &Service, src: &End, tgt: &End, p: &str, names: (&[String],
 
 fn plan(src: &End, tgt: &End, tables: &[String], seq: Option<&str>, data: DataOptions) -> MigrationPlan {
     MigrationPlan {
-        source: MigrationEnd { profile: src.profile.clone(), schema: src.schema.clone() },
+        source: MigrationEnd {
+            profile: src.profile.clone(),
+            catalog: src.catalog.clone(),
+            schema: src.schema.clone(),
+        },
         tables: Some(tables.iter().rev().cloned().collect()),
         sequences: Some(seq.into_iter().map(String::from).collect()),
-        target: MigrationEnd { profile: tgt.profile.clone(), schema: tgt.schema.clone() },
+        target: MigrationEnd {
+            profile: tgt.profile.clone(),
+            catalog: tgt.catalog.clone(),
+            schema: tgt.schema.clone(),
+        },
         mode: Default::default(),
         settings: Default::default(),
         data,
@@ -406,6 +453,7 @@ async fn migrate_flow(src_env: &str, tgt_env: &str, p: &str) {
         let tseq = dialect.qualified(Some(&tgt.schema), &preview.draft.sequences[0].target.name);
         let sql = match tgt.driver {
             DriverId::Postgres => format!("SELECT nextval('{tseq}')"),
+            DriverId::SqlServer => format!("SELECT NEXT VALUE FOR {tseq}"),
             _ => format!("SELECT {tseq}.NEXTVAL FROM DUAL"),
         };
         let next = number(run(&s, tgt, &[sql], false).await);
@@ -547,7 +595,7 @@ fn shape(d: &Dialect, t: &Table, draft: &Table) -> Vec<String> {
     out
 }
 
-/// 每个源库迁到 MySQL、PG、Oracle、达梦（不迁到自己），另加信创常见路线。
+/// 每个源库迁到 MySQL、PG、Oracle、达梦（不迁到自己），另加信创常见路线；SQL Server 与这四个库互迁（M6b）。
 macro_rules! routes {
     ($($name:ident: $src:literal => $tgt:literal, $p:literal;)*) => {
         $(
@@ -604,6 +652,14 @@ routes! {
     kingbase_pg_to_pg: "DBLENS_TEST_KINGBASE_PG_URL" => "DBLENS_TEST_PG_URL", "m5kp";
     kingbase_pg_to_oracle: "DBLENS_TEST_KINGBASE_PG_URL" => "DBLENS_TEST_ORACLE_URL", "m5kp";
     kingbase_pg_to_dameng: "DBLENS_TEST_KINGBASE_PG_URL" => "DBLENS_TEST_DM_URL", "m5kp";
+    sqlserver_to_mysql: "DBLENS_TEST_SQLSERVER_URL" => "DBLENS_TEST_MYSQL_URL", "m6ms";
+    sqlserver_to_pg: "DBLENS_TEST_SQLSERVER_URL" => "DBLENS_TEST_PG_URL", "m6ms";
+    sqlserver_to_oracle: "DBLENS_TEST_SQLSERVER_URL" => "DBLENS_TEST_ORACLE_URL", "m6ms";
+    sqlserver_to_dameng: "DBLENS_TEST_SQLSERVER_URL" => "DBLENS_TEST_DM_URL", "m6ms";
+    mysql_to_sqlserver: "DBLENS_TEST_MYSQL_URL" => "DBLENS_TEST_SQLSERVER_URL", "m6my";
+    pg_to_sqlserver: "DBLENS_TEST_PG_URL" => "DBLENS_TEST_SQLSERVER_URL", "m6pg";
+    oracle_to_sqlserver: "DBLENS_TEST_ORACLE_URL" => "DBLENS_TEST_SQLSERVER_URL", "m6or";
+    dameng_to_sqlserver: "DBLENS_TEST_DM_URL" => "DBLENS_TEST_SQLSERVER_URL", "m6dm";
 }
 
 /// 续传和跳过：每种目标家族各一条。
@@ -627,4 +683,6 @@ special! {
     skip_mysql_to_oracle: skip_flow, "DBLENS_TEST_MYSQL_URL" => "DBLENS_TEST_ORACLE_URL", "m5sm";
     skip_oracle_to_pg: skip_flow, "DBLENS_TEST_ORACLE_URL" => "DBLENS_TEST_PG_URL", "m5so";
     skip_pg_to_dameng: skip_flow, "DBLENS_TEST_PG_URL" => "DBLENS_TEST_DM_URL", "m5sd";
+    resume_pg_to_sqlserver: resume_flow, "DBLENS_TEST_PG_URL" => "DBLENS_TEST_SQLSERVER_URL", "m6rp";
+    skip_mysql_to_sqlserver: skip_flow, "DBLENS_TEST_MYSQL_URL" => "DBLENS_TEST_SQLSERVER_URL", "m6sm";
 }
```

- [ ] **Step 4: 修改 `crates/dblens-api/src/design.rs`**

```diff
--- a/crates/dblens-api/src/design.rs
+++ b/crates/dblens-api/src/design.rs
@@ -17,6 +17,8 @@ pub async fn caps(State(s): State<AppState>, Path(id): Path<String>) -> ApiResul
 
 #[derive(Deserialize)]
 pub struct TableQuery {
+    #[serde(default)]
+    catalog: Option<String>,
     schema: String,
     name: String,
 }
@@ -27,7 +29,7 @@ pub async fn table(
     Path(id): Path<String>,
     Query(q): Query<TableQuery>,
 ) -> ApiResult<Json<DesignTable>> {
-    Ok(Json(s.service.design_table(&id, &q.schema, &q.name).await?))
+    Ok(Json(s.service.design_table(&id, q.catalog.as_deref(), &q.schema, &q.name).await?))
 }
 
 pub async fn preview(
```

- [ ] **Step 5: 修改 `crates/dblens-api/src/tasks.rs`**

```diff
--- a/crates/dblens-api/src/tasks.rs
+++ b/crates/dblens-api/src/tasks.rs
@@ -83,6 +83,8 @@ pub async fn preview(State(s): State<AppState>, Json(b): Json<PreviewBody>) -> A
 
 #[derive(Deserialize)]
 pub struct DdlBody {
+    #[serde(default)]
+    catalog: Option<String>,
     schema: String,
     table: String,
     create: NewTable,
@@ -94,7 +96,7 @@ pub async fn ddl(
     Path(id): Path<String>,
     Json(b): Json<DdlBody>,
 ) -> ApiResult<Json<Value>> {
-    let sql = s.service.import_ddl(&id, &b.schema, &b.table, &b.create).await?;
+    let sql = s.service.import_ddl(&id, b.catalog.as_deref(), &b.schema, &b.table, &b.create).await?;
     Ok(Json(json!({ "sql": sql })))
 }
 
```

- [ ] **Step 6: 修改 `crates/dblens-core/locales/app.json`**

```diff
--- a/crates/dblens-core/locales/app.json
+++ b/crates/dblens-core/locales/app.json
@@ -300,7 +300,6 @@
   "session.task_gone": {"zh-CN": "会话任务已退出", "en": "The session task has exited"},
   "sqlserver.bad_auth": {"zh-CN": "不认识的认证方式：%{value}", "en": "Unknown authentication method: %{value}"},
   "sqlserver.connection_lost": {"zh-CN": "与 SQL Server 的连接已断开", "en": "The connection to SQL Server was lost"},
-  "sqlserver.later": {"zh-CN": "SQL Server 的这项功能将在后续版本支持", "en": "This feature is not yet available for SQL Server"},
   "sqlserver.windows_auth_only": {"zh-CN": "Windows 认证只能在 Windows 上使用", "en": "Windows authentication is only available on Windows"},
   "sqlfile.denied": {"zh-CN": "第 %{line} 行的语句被执行策略拒绝，没有执行任何语句：%{reason}\n%{text}", "en": "The statement at line %{line} was denied by the execution policy; nothing was executed: %{reason}\n%{text}"},
   "sqlfile.history": {"zh-CN": "执行 SQL 文件 %{file}：%{done} 条，失败 %{failed} 条", "en": "Run SQL file %{file}: %{done} statements, %{failed} failed"},
```

- [ ] **Step 7: 修改 `crates/dblens-dialect/src/changes.rs`**

```diff
--- a/crates/dblens-dialect/src/changes.rs
+++ b/crates/dblens-dialect/src/changes.rs
@@ -87,6 +87,11 @@ impl<'a> Builder<'a> {
     }
 
     fn value(&mut self, v: &Value) -> Result<()> {
+        // SQL Server：类型未知的 NULL 参数按 nvarchar 传，写进 varbinary 等列会报"不能隐式转换"，直接写 NULL
+        if self.dialect.family() == Family::SqlServer && matches!(v, Value::Null) {
+            self.text("NULL");
+            return Ok(());
+        }
         self.params.push(v.clone());
         self.sql.push_str(&self.dialect.placeholder(self.params.len()));
         // PG 驱动不能直接绑定 NUMERIC 参数：精确小数按文本传，由数据库转换（整数列也接受 numeric 的赋值和比较）
```

- [ ] **Step 8: 修改 `crates/dblens-dialect/src/compare/sync.rs`**

```diff
--- a/crates/dblens-dialect/src/compare/sync.rs
+++ b/crates/dblens-dialect/src/compare/sync.rs
@@ -101,10 +101,8 @@ pub fn sync_script(
             match &item.change {
                 SchemaChange::DropTable { name } => drop_tables
                     .push(SyncStep::Sql { sql: format!("DROP TABLE {}", q(name)), items: vec![item.id] }),
-                SchemaChange::RenameTable { from, to } => renames.push(SyncStep::Sql {
-                    sql: format!("ALTER TABLE {} RENAME TO {}", q(from), d.ident(to)),
-                    items: vec![item.id],
-                }),
+                SchemaChange::RenameTable { from, to } => renames
+                    .push(SyncStep::Sql { sql: d.rename_table_sql(schema, from, to)?, items: vec![item.id] }),
                 SchemaChange::Table { change } if matches!(change.as_ref(), C::CreateTable { .. }) => {
                     let C::CreateTable { table } = change.as_ref() else { unreachable!() };
                     // 外键在所有表建好之后再加
```

- [ ] **Step 9: 修改 `crates/dblens-dialect/src/import.rs`**

```diff
--- a/crates/dblens-dialect/src/import.rs
+++ b/crates/dblens-dialect/src/import.rs
@@ -38,6 +38,11 @@ impl Dialect {
         format!("DELETE FROM {}", self.qualified(Some(schema), table))
     }
 
+    /// 每条写入语句至多包含的参数个数：SQL Server 一次请求最多 2100 个参数，留些余量取 2000。
+    pub fn max_params(&self) -> usize {
+        if self.family() == Family::SqlServer { 2000 } else { MAX_PARAMS_PER_STATEMENT }
+    }
+
     /// 新建表的建表语句（列、可空、主键），不带结尾的分号。
     pub fn create_table_sql(&self, schema: &str, table: &Table) -> String {
         self.table_create(schema, table).trim_end().trim_end_matches(';').to_string()
@@ -81,7 +86,7 @@ impl Dialect {
         let merge = mode == WriteMode::Upsert && matches!(self.family(), Family::Oracle | Family::Dameng);
         // Oracle、达梦的 MERGE 每行一条：多行要用 UNION ALL 拼来源，各行的 NULL 参数类型不一致时会报错
         let per_statement =
-            if merge { 1 } else { (MAX_PARAMS_PER_STATEMENT / cols.len()).clamp(1, MAX_ROWS_PER_STATEMENT) };
+            if merge { 1 } else { (self.max_params() / cols.len()).clamp(1, MAX_ROWS_PER_STATEMENT) };
         let target = self.qualified(Some(schema), &table.name);
         let names: Vec<String> = cols.iter().map(|c| self.ident(&c.name)).collect();
         let list = names.join(", ");
@@ -92,6 +97,8 @@ impl Dialect {
             let mut b = Binder { dialect: self, sql: String::new(), params: Vec::new() };
             if merge {
                 b.merge(&target, &cols, &names, &key, &rows[start])?;
+            } else if mode == WriteMode::Upsert && self.family() == Family::SqlServer {
+                b.mssql_merge(&target, &cols, &names, &key, &rows[start..end])?;
             } else {
                 b.insert(&target, &cols, &list, &rows[start..end])?;
                 if mode == WriteMode::Upsert {
@@ -116,6 +123,11 @@ impl Binder<'_> {
     /// 一个参数。PG 驱动不能直接绑定 NUMERIC 参数，精确小数按文本传、由数据库转换（与表数据编辑相同）；
     /// 时间间隔和不认识的类型也按文本传，转换成列的原生类型。
     fn param(&mut self, col: &Column, v: &Value) {
+        // SQL Server：类型未知的 NULL 参数按 nvarchar 传，写进 varbinary 等列会报"不能隐式转换"，直接写 NULL
+        if self.dialect.family() == Family::SqlServer && matches!(v, Value::Null) {
+            self.sql.push_str("NULL");
+            return;
+        }
         self.params.push(v.clone());
         self.sql.push_str(&self.dialect.placeholder(self.params.len()));
         if self.dialect.family() != Family::Postgres || matches!(v, Value::Null) {
@@ -203,6 +215,43 @@ impl Binder<'_> {
         }
     }
 
+    /// SQL Server 的按主键更新：一批一条多行 MERGE（来源是带参数的 VALUES）。自增列不能更新，不写进 UPDATE SET；
+    /// 没有可更新的列时省略 WHEN MATCHED。MERGE 必须以分号结束。
+    fn mssql_merge(
+        &mut self,
+        target: &str,
+        cols: &[&Column],
+        names: &[String],
+        key: &[usize],
+        rows: &[Vec<Value>],
+    ) -> Result<()> {
+        self.sql.push_str(&format!("MERGE INTO {target} AS t USING (VALUES "));
+        for (n, row) in rows.iter().enumerate() {
+            if n > 0 {
+                self.sql.push_str(", ");
+            }
+            self.sql.push('(');
+            self.row_values(cols, row)?;
+            self.sql.push(')');
+        }
+        let on: Vec<String> = key.iter().map(|&k| format!("t.{0} = s.{0}", names[k])).collect();
+        self.sql.push_str(&format!(") AS s ({}) ON {}", names.join(", "), on.join(" AND ")));
+        let sets: Vec<String> = (0..names.len())
+            .filter(|&k| !key.contains(&k) && !cols[k].auto_increment)
+            .map(|k| format!("t.{0} = s.{0}", names[k]))
+            .collect();
+        if !sets.is_empty() {
+            self.sql.push_str(&format!(" WHEN MATCHED THEN UPDATE SET {}", sets.join(", ")));
+        }
+        let values: Vec<String> = names.iter().map(|n| format!("s.{n}")).collect();
+        self.sql.push_str(&format!(
+            " WHEN NOT MATCHED THEN INSERT ({}) VALUES ({});",
+            names.join(", "),
+            values.join(", ")
+        ));
+        Ok(())
+    }
+
     /// Oracle、达梦的按主键更新：一行一条 MERGE。只映射了主键列时省略 WHEN MATCHED。
     fn merge(
         &mut self,
@@ -413,6 +462,39 @@ mod tests {
         let st = d.import_statements("s", &wide, &cols, &rows, WriteMode::Append).unwrap();
         assert_eq!(st.iter().map(|s| s.rows.len()).collect::<Vec<_>>(), [300, 300, 100]);
         assert!(st.iter().all(|s| s.params.len() <= MAX_PARAMS_PER_STATEMENT));
+        // SQL Server 一次请求最多 2100 个参数：100 列 → 每条 20 行
+        let ms = Dialect::new(Variant::SqlServer);
+        let st = ms.import_statements("s", &wide, &cols, &rows, WriteMode::Append).unwrap();
+        assert!(
+            st.iter().all(|s| s.rows.len() == 20),
+            "{:?}",
+            st.iter().map(|s| s.rows.len()).collect::<Vec<_>>()
+        );
+        let st = ms
+            .import_statements("s", &orders(), &names(&["id", "name", "price"]), &many, WriteMode::Upsert)
+            .unwrap();
+        assert!(st.iter().all(|s| s.params.len() <= 2000) && st.len() == 3);
+    }
+
+    #[test]
+    fn sqlserver_upsert_is_one_multi_row_merge() {
+        assert_eq!(
+            sql(Variant::SqlServer, "dbo", WriteMode::Upsert),
+            [
+                "MERGE INTO dbo.orders AS t USING (VALUES (@P1, @P2, @P3), (@P4, NULL, NULL)) AS s (id, name, price) \
+              ON t.id = s.id WHEN MATCHED THEN UPDATE SET t.name = s.name, t.price = s.price \
+              WHEN NOT MATCHED THEN INSERT (id, name, price) VALUES (s.id, s.name, s.price);"
+            ]
+        );
+        // 只映射了主键列：没有 WHEN MATCHED
+        let only = Dialect::new(Variant::SqlServer)
+            .import_statements("dbo", &orders(), &names(&["id"]), &[vec![Value::Int(1)]], WriteMode::Upsert)
+            .unwrap();
+        assert_eq!(
+            only[0].sql,
+            "MERGE INTO dbo.orders AS t USING (VALUES (@P1)) AS s (id) ON t.id = s.id \
+             WHEN NOT MATCHED THEN INSERT (id) VALUES (s.id);"
+        );
     }
 
     #[test]
```

- [ ] **Step 10: 修改 `crates/dblens-dialect/src/lib.rs`**

```diff
--- a/crates/dblens-dialect/src/lib.rs
+++ b/crates/dblens-dialect/src/lib.rs
@@ -170,6 +170,12 @@ impl Dialect {
         session::set_schema(self.family(), schema)
     }
 
+    /// 在某个 schema 中执行生成的语句前要切换的语句（表设计器、迁移）。SQL Server 的会话"当前 schema"是库，
+    /// 连接时已指定库（`Profile::in_catalog`），生成的语句都带架构名，不切换，返回 None。
+    pub fn schema_switch_sql(&self, schema: &str) -> Option<String> {
+        (self.family() != Family::SqlServer).then(|| self.set_schema_sql(schema))
+    }
+
     pub fn logical_type(&self, native: &str) -> LogicalType {
         types::logical_type(self.family(), native)
     }
```

- [ ] **Step 11: 修改 `crates/dblens-dialect/src/migrate/expr.rs`**

```diff
--- a/crates/dblens-dialect/src/migrate/expr.rs
+++ b/crates/dblens-dialect/src/migrate/expr.rs
@@ -35,6 +35,31 @@ fn tokenize(family: Family, s: &str) -> Option<Vec<Tok>> {
         let c = b[i];
         if c.is_whitespace() {
             i += 1;
+        } else if family == Family::SqlServer && (c == 'N' || c == 'n') && b.get(i + 1) == Some(&'\'') {
+            // SQL Server 的 Unicode 字符串 N'…'：去掉 N，按普通字符串读
+            i += 1;
+        } else if c == '[' && family == Family::SqlServer {
+            // SQL Server 的方括号标识符，`]]` 是一个 `]`
+            let mut v = String::new();
+            i += 1;
+            loop {
+                match b.get(i) {
+                    None => return None,
+                    Some(']') if b.get(i + 1) == Some(&']') => {
+                        v.push(']');
+                        i += 2;
+                    }
+                    Some(']') => {
+                        i += 1;
+                        break;
+                    }
+                    Some(ch) => {
+                        v.push(*ch);
+                        i += 1;
+                    }
+                }
+            }
+            out.push(Tok::Quoted(v));
         } else if c == '\'' {
             let mut v = String::new();
             i += 1;
@@ -202,8 +227,10 @@ pub fn convert_expr(
             Tok::Word(w) if KEYWORDS.iter().any(|k| w.eq_ignore_ascii_case(k)) => {
                 let up = w.to_ascii_uppercase();
                 match up.as_str() {
-                    // Oracle、达梦没有布尔字面量
-                    "TRUE" | "FALSE" if matches!(tgt.family(), Family::Oracle | Family::Dameng) => {
+                    // Oracle、达梦、SQL Server 没有布尔字面量
+                    "TRUE" | "FALSE"
+                        if matches!(tgt.family(), Family::Oracle | Family::Dameng | Family::SqlServer) =>
+                    {
                         (if up == "TRUE" { "1" } else { "0" }).to_string()
                     }
                     _ => up,
@@ -316,8 +343,9 @@ pub(super) fn convert_default(
     let toks = strip_parens(&toks);
     let logical = &column.logical;
     let oracle_like = matches!(fam, Family::Oracle | Family::Dameng);
+    // SQL Server 的 BIT 也只能写 1、0
     let boolean = |v: bool| -> String {
-        if oracle_like || (fam == Family::Postgres && *logical != L::Bool) {
+        if oracle_like || fam == Family::SqlServer || (fam == Family::Postgres && *logical != L::Bool) {
             (if v { "1" } else { "0" }).into()
         } else {
             (if v { "TRUE" } else { "FALSE" }).into()
@@ -329,6 +357,7 @@ pub(super) fn convert_default(
             (Family::Postgres, L::Date) => "CURRENT_DATE".into(),
             (Family::Oracle, L::Timestamp { .. } | L::Date) => "TRUNC(SYSDATE)".into(),
             (Family::Dameng, L::Date) => "CURDATE()".into(),
+            (Family::SqlServer, L::Date) => "CAST(GETDATE() AS DATE)".into(),
             _ => return Err(()),
         }))
     };
@@ -348,6 +377,8 @@ pub(super) fn convert_default(
             (Family::Oracle, L::Timestamp { tz: false, .. }) => "LOCALTIMESTAMP".into(),
             (Family::Oracle, L::Timestamp { tz: true, .. }) => "SYSTIMESTAMP".into(),
             (Family::Dameng, L::Timestamp { .. }) => "CURRENT_TIMESTAMP".into(),
+            (Family::SqlServer, L::Timestamp { tz: false, .. }) => "SYSDATETIME()".into(),
+            (Family::SqlServer, L::Timestamp { tz: true, .. }) => "SYSDATETIMEOFFSET()".into(),
             (_, L::Date) => return today(),
             _ => return Err(()),
         }))
@@ -390,6 +421,8 @@ pub(super) fn convert_default(
                 | "SYSDATE"
                 | "SYSTIMESTAMP"
                 | "GETDATE"
+                | "SYSDATETIME"
+                | "SYSDATETIMEOFFSET"
                 | "STATEMENT_TIMESTAMP"
                 | "TRANSACTION_TIMESTAMP"
                 | "CLOCK_TIMESTAMP"
@@ -409,6 +442,7 @@ pub(super) fn convert_default(
                     crate::Variant::MySql => Ok(Some("(UUID())".into())),
                     crate::Variant::Postgres => Ok(Some("gen_random_uuid()".into())),
                     crate::Variant::Oracle => Ok(Some("SYS_GUID()".into())),
+                    crate::Variant::SqlServer => Ok(Some("NEWID()".into())),
                     _ => Err(()),
                 }
             }
@@ -478,6 +512,20 @@ mod tests {
             convert_expr(&pg, &ora, "flag = true", &cols(&[("flag", "FLAG")])).as_deref(),
             Some("FLAG = 1")
         );
+        // SQL Server 作为来源：方括号标识符、N'' 字符串
+        let ms = d(Variant::SqlServer);
+        assert_eq!(
+            convert_expr(&ms, &pg, "([amount]>=(0))", &cols(&[("amount", "amount")])).as_deref(),
+            Some("(amount >= (0))")
+        );
+        assert_eq!(
+            convert_expr(&ms, &ora, "[status]<>N'x'", &cols(&[("status", "STATUS")])).as_deref(),
+            Some("STATUS <> 'x'")
+        );
+        assert_eq!(
+            convert_expr(&pg, &ms, "flag = true", &cols(&[("flag", "flag")])).as_deref(),
+            Some("flag = 1")
+        );
     }
 
     #[test]
@@ -505,6 +553,19 @@ mod tests {
         assert_eq!(conv(Oracle, Postgres, "-1.5 ", "NUMERIC(5,1)"), Ok(Some("-1.5".into())));
         assert_eq!(conv(Oracle, Postgres, "NULL", "INTEGER"), Ok(None));
         assert_eq!(conv(Postgres, Oracle, "true", "NUMBER(1)"), Ok(Some("1".into())));
+        // SQL Server 没有布尔字面量
+        assert_eq!(conv(MySql, SqlServer, "1", "BIT"), Ok(Some("1".into())));
+        assert_eq!(conv(Postgres, SqlServer, "true", "BIT"), Ok(Some("1".into())));
+        // 当前时间：SQL Server 作为目标和来源
+        assert_eq!(conv(Postgres, SqlServer, "now()", "DATETIME2(6)"), Ok(Some("SYSDATETIME()".into())));
+        assert_eq!(
+            conv(Postgres, SqlServer, "CURRENT_DATE", "DATE"),
+            Ok(Some("CAST(GETDATE() AS DATE)".into()))
+        );
+        assert_eq!(
+            conv(SqlServer, Postgres, "(sysdatetime())", "timestamp(6)"),
+            Ok(Some("LOCALTIMESTAMP".into()))
+        );
         assert_eq!(conv(MySql, Postgres, "b'1'", "BOOLEAN"), Ok(Some("TRUE".into())));
         assert_eq!(conv(Postgres, MySql, "now()", "DATETIME(6)"), Ok(Some("CURRENT_TIMESTAMP(6)".into())));
         assert_eq!(conv(Oracle, Postgres, "SYSDATE", "TIMESTAMP(0)"), Ok(Some("LOCALTIMESTAMP".into())));
```

- [ ] **Step 12: 修改 `crates/dblens-dialect/src/migrate/script.rs`**

```diff
--- a/crates/dblens-dialect/src/migrate/script.rs
+++ b/crates/dblens-dialect/src/migrate/script.rs
@@ -95,13 +95,18 @@ pub fn migration_script(
                     drop_sql(tgt, schema, &t.target.name),
                 ));
             }
-            let base = Table {
+            let mut base = Table {
                 uniques: vec![],
                 foreign_keys: vec![],
                 checks: vec![],
                 indexes: vec![],
                 ..t.target.clone()
             };
+            if tgt.family() == Family::SqlServer {
+                for c in &mut base.columns {
+                    c.default = c.default.take().map(|d| qualify_next_value(tgt, schema, d));
+                }
+            }
             for (i, s) in
                 design(tgt, schema, "", TableChange::CreateTable { table: base })?.into_iter().enumerate()
             {
@@ -183,13 +188,13 @@ pub fn migration_script(
     Ok(first)
 }
 
-/// 脚本全文：每条语句以 `;` 结尾，两步之间空一行。
-pub fn script_text(statements: &[MigrationStatement]) -> String {
+/// 脚本全文：每条语句按目标的写法结束（`Family::terminate`），两步之间空一行。
+pub fn script_text(target: Family, statements: &[MigrationStatement]) -> String {
     let part = |phase| {
         statements
             .iter()
             .filter(|s| s.phase == phase)
-            .map(|s| format!("{};", s.sql))
+            .map(|s| target.terminate(&s.sql))
             .collect::<Vec<_>>()
             .join("\n")
     };
@@ -210,6 +215,14 @@ fn design(tgt: &Dialect, schema: &str, table: &str, change: TableChange) -> Resu
         .collect())
 }
 
+/// SQL Server 的默认值 `NEXT VALUE FOR 序列` 补上 schema：不切换当前 schema，一段名按用户的默认架构解析。
+fn qualify_next_value(tgt: &Dialect, schema: &str, default: String) -> String {
+    match default.strip_prefix("NEXT VALUE FOR ") {
+        Some(seq) if !seq.contains('.') => format!("NEXT VALUE FOR {}.{seq}", tgt.ident(schema)),
+        _ => default,
+    }
+}
+
 fn drop_sql(tgt: &Dialect, schema: &str, name: &str) -> String {
     let q = tgt.qualified(Some(schema), name);
     match tgt.family() {
@@ -224,7 +237,11 @@ pub(super) fn sequence_sql(tgt: &Dialect, schema: &str, s: &Sequence) -> String
     if tgt.family() == Family::Postgres {
         return tgt.sequence_ddl(schema, s).trim().trim_end_matches(';').to_string();
     }
+    let sqlserver = tgt.family() == Family::SqlServer;
     let mut out = format!("CREATE SEQUENCE {}", tgt.qualified(Some(schema), &s.name));
+    if sqlserver {
+        out += " AS BIGINT";
+    }
     for (label, v) in [
         ("START WITH", &s.start),
         ("INCREMENT BY", &s.increment),
@@ -232,13 +249,19 @@ pub(super) fn sequence_sql(tgt: &Dialect, schema: &str, s: &Sequence) -> String
         ("MAXVALUE", &s.max_value),
     ] {
         if let Some(v) = v.as_deref().map(str::trim).filter(|v| !v.is_empty()) {
+            // SQL Server 的序列是 BIGINT：超出范围的边界（如 Oracle 的 28 个 9）不写，取类型的边界
+            if sqlserver && v.parse::<i64>().is_err() {
+                continue;
+            }
             out += &format!("\n    {label} {v}");
         }
     }
+    // SQL Server 写 NO CACHE / NO CYCLE，Oracle、达梦写 NOCACHE / NOCYCLE
+    let no = if sqlserver { "NO " } else { "NO" };
     out += &match s.cache {
         Some(c) if c >= 2 => format!("\n    CACHE {c}"),
-        _ => "\n    NOCACHE".into(),
+        _ => format!("\n    {no}CACHE"),
     };
-    out += if s.cycle { "\n    CYCLE" } else { "\n    NOCYCLE" };
+    out += &if s.cycle { "\n    CYCLE".to_string() } else { format!("\n    {no}CYCLE") };
     out
 }
```

- [ ] **Step 13: 新建 `crates/dblens-dialect/src/migrate/snapshots/dblens_dialect__migrate__tests__pg_to_sqlserver.snap`**

```text
---
source: crates/dblens-dialect/src/migrate/tests.rs
expression: "render(&draft, tgt)"
---
Info customers_id_seq: Sequence owned by a serial column; migrated with the identity column
Lossy customers.tags: Unrecognized type text[]; migrated as text (NVARCHAR(MAX))
Lossy customers [ix_customers_note]: Includes a large-object column the target cannot index; not migrated
Lossy customers [ix_customers_lower]: Index expression lower((email)::text) cannot be converted; not migrated
Lossy orders.pos: Unrecognized type point; migrated as text (NVARCHAR(MAX))
Lossy orders [fk_orders_product]: Referenced table products is neither migrated nor in the target; not migrated

CREATE SEQUENCE app.order_seq AS BIGINT
    START WITH 1
    INCREMENT BY 1
    MINVALUE 1
    MAXVALUE 9223372036854775807
    CACHE 20
    NO CYCLE
GO
CREATE TABLE app.customers (
    id BIGINT IDENTITY(1,1) NOT NULL,
    name NVARCHAR(50) CONSTRAINT DF_customers_name DEFAULT N'n/a' NOT NULL,
    email NVARCHAR(100) NULL,
    active BIT CONSTRAINT DF_customers_active DEFAULT 1 NULL,
    created DATETIME2(6) CONSTRAINT DF_customers_created DEFAULT SYSDATETIME() NULL,
    balance DECIMAL(12,2) CONSTRAINT DF_customers_balance DEFAULT 0 NULL,
    tags NVARCHAR(MAX) NULL,
    note NVARCHAR(MAX) NULL,
    CONSTRAINT customers_pkey PRIMARY KEY (id)
)
GO
EXEC sys.sp_addextendedproperty N'MS_Description', N'Customers', N'SCHEMA', N'app', N'TABLE', N'customers'
GO
EXEC sys.sp_addextendedproperty N'MS_Description', N'Customer name', N'SCHEMA', N'app', N'TABLE', N'customers', N'COLUMN', N'name'
GO
CREATE TABLE app.orders (
    id INT CONSTRAINT DF_orders_id DEFAULT NEXT VALUE FOR app.order_seq NOT NULL,
    customer_id BIGINT NULL,
    product_id INT NULL,
    amount DECIMAL(10,2) NULL,
    double_amount AS (amount * (2)),
    status NVARCHAR(10) CONSTRAINT DF_orders_status DEFAULT N'new' NULL,
    placed DATE CONSTRAINT DF_orders_placed DEFAULT CAST(GETDATE() AS DATE) NULL,
    code UNIQUEIDENTIFIER CONSTRAINT DF_orders_code DEFAULT NEWID() NULL,
    pos NVARCHAR(MAX) NULL,
    CONSTRAINT orders_pkey PRIMARY KEY (id)
)
GO

CREATE INDEX ix_customers_name ON app.customers (name)
GO
ALTER TABLE app.customers ADD CONSTRAINT customers_email_key UNIQUE (email)
GO
ALTER TABLE app.customers ADD CONSTRAINT ck_balance CHECK (balance >= (0))
GO
ALTER TABLE app.orders ADD CONSTRAINT ck_status CHECK ((status) IN (N'new', N'paid'))
GO
ALTER TABLE app.orders ADD CONSTRAINT fk_orders_customer FOREIGN KEY (customer_id) REFERENCES app.customers (id) ON DELETE CASCADE ON UPDATE CASCADE
GO
```

- [ ] **Step 14: 修改 `crates/dblens-dialect/src/migrate/tests.rs`**

```diff
--- a/crates/dblens-dialect/src/migrate/tests.rs
+++ b/crates/dblens-dialect/src/migrate/tests.rs
@@ -171,7 +171,7 @@ fn render(draft: &MigrationDraft, tgt: Variant) -> String {
         );
     }
     let script = migration_script(&Dialect::new(tgt), "app", draft, &BTreeMap::new(), &[]).unwrap();
-    out + "\n" + &script_text(&script)
+    out + "\n" + &script_text(tgt.family(), &script)
 }
 
 /// 不认识的类型都确认按文本迁移。
@@ -196,6 +196,7 @@ fn pg_to_each_family() {
         (Variant::Oracle, "pg_to_oracle"),
         (Variant::Dameng, "pg_to_dameng"),
         (Variant::OpenGauss, "pg_to_opengauss"),
+        (Variant::SqlServer, "pg_to_sqlserver"),
     ] {
         let draft = migrate(&input(Variant::Postgres, tgt, &tables, &sequences), &settings);
         assert!(!draft.blocked(), "{name}");
```

- [ ] **Step 15: 修改 `crates/dblens-dialect/src/variant.rs`**

```diff
--- a/crates/dblens-dialect/src/variant.rs
+++ b/crates/dblens-dialect/src/variant.rs
@@ -19,6 +19,22 @@ pub enum Family {
     SqlServer,
 }
 
+impl Family {
+    /// 脚本中一条语句的写法（M6b 设计文档 5.1 节）：SQL Server 后接单独一行 `GO`（`CREATE VIEW`、`CREATE TRIGGER` 等
+    /// 必须是批中第一条），其他库以 `;` 结尾。
+    pub fn terminate(self, sql: &str) -> String {
+        let sql = sql.trim_end();
+        match self {
+            Family::SqlServer => format!(
+                "{sql}
+GO"
+            ),
+            _ if sql.ends_with(';') => sql.to_string(),
+            _ => format!("{sql};"),
+        }
+    }
+}
+
 /// 兼容模式：人大金仓、openGauss 可以按 Oracle 的规则解释 SQL（空串即 NULL、`NUMBER`、`VARCHAR2`、
 /// 带时间的 `DATE`、Oracle 风格的过程块）。其他变体恒为 `Native`。
 #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
@@ -205,4 +221,21 @@ mod tests {
         assert_eq!(serde_json::to_string(&Variant::OpenGauss).unwrap(), "\"opengauss\"");
         assert_eq!(serde_json::to_string(&Family::MySql).unwrap(), "\"mysql\"");
     }
+
+    #[test]
+    fn script_terminators() {
+        assert_eq!(Family::MySql.terminate("SELECT 1"), "SELECT 1;");
+        assert_eq!(
+            Family::Oracle.terminate(
+                "SELECT 1 FROM DUAL;
+"
+            ),
+            "SELECT 1 FROM DUAL;"
+        );
+        assert_eq!(
+            Family::SqlServer.terminate("CREATE VIEW v AS SELECT 1"),
+            "CREATE VIEW v AS SELECT 1
+GO"
+        );
+    }
 }
```

- [ ] **Step 16: 修改 `crates/dblens-service/src/compare.rs`**

```diff
--- a/crates/dblens-service/src/compare.rs
+++ b/crates/dblens-service/src/compare.rs
@@ -7,13 +7,13 @@ use std::sync::Arc;
 
 use dblens_core::schema::Table;
 use dblens_core::{Error, Msg, Result, msg};
-use dblens_dialect::Dialect;
 use dblens_dialect::compare::{
     CompareOptions, CompareRenames, CompareSide, SchemaChange, SchemaDiff, SyncScript, SyncStep,
     apply_changes, compare_schemas, sync_rebuild_tables, sync_script,
 };
 use dblens_dialect::design::{RebuildContext, TableChange};
 use dblens_dialect::meta::{MetaArgs, MetaKey};
+use dblens_dialect::{Dialect, Family};
 use serde::{Deserialize, Serialize};
 
 use crate::design::{Part, Run};
@@ -26,8 +26,17 @@ use crate::task::{TaskKind, TaskPhase};
 #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
 #[serde(tag = "kind", rename_all = "snake_case")]
 pub enum CompareSideRef {
-    Model { model: String, schema: String },
-    Database { profile: String, schema: String },
+    Model {
+        model: String,
+        schema: String,
+    },
+    Database {
+        profile: String,
+        /// 所在的库（SQL Server）；其他库不填。
+        #[serde(default, skip_serializing_if = "Option::is_none")]
+        catalog: Option<String>,
+        schema: String,
+    },
 }
 
 #[derive(Debug, Clone, PartialEq, Deserialize)]
@@ -162,12 +171,12 @@ fn effective_renames(source: &Side, target: &Side, requested: &CompareRenames) -
     out
 }
 
-/// 同步脚本全文：每条语句以 `;` 结尾，各段之间空一行。
-fn script_text(script: &SyncScript) -> String {
+/// 同步脚本全文：每条语句按目标的写法结束（`Family::terminate`），各段之间空一行。
+fn script_text(target: Family, script: &SyncScript) -> String {
     let parts: Vec<String> = script
         .steps
         .iter()
-        .map(|s| part_of(s).texts().into_iter().map(|t| format!("{t};")).collect::<Vec<_>>().join("\n"))
+        .map(|s| part_of(s).texts().into_iter().map(|t| target.terminate(&t)).collect::<Vec<_>>().join("\n"))
         .collect();
     parts.join("\n\n")
 }
@@ -265,8 +274,8 @@ impl Service {
                     profile: None,
                 })
             }
-            CompareSideRef::Database { profile, schema } => {
-                let profile = self.feature_profile(profile)?;
+            CompareSideRef::Database { profile, catalog, schema } => {
+                let profile = self.scoped_profile(profile, catalog.as_deref())?;
                 let dialect = self.metadata.dialect(&profile).await?;
                 // 跳过元数据缓存，以库中最新的结构为准
                 let args = MetaArgs { schema: Some(schema.clone()), ..Default::default() };
@@ -354,7 +363,7 @@ impl Service {
         match &c.target.profile {
             Some(profile) => {
                 let script = self.sync_script_of(&c, &req.selected).await?;
-                preview.text = script_text(&script);
+                preview.text = script_text(c.target.dialect.family(), &script);
                 let texts: Vec<String> = script.steps.iter().flat_map(|s| part_of(s).texts()).collect();
                 (preview.denied, preview.confirm) =
                     self.check_policy(profile, c.target.dialect, &texts).await;
```

- [ ] **Step 17: 修改 `crates/dblens-service/src/design.rs`**

```diff
--- a/crates/dblens-service/src/design.rs
+++ b/crates/dblens-service/src/design.rs
@@ -26,6 +26,9 @@ use crate::task::{TaskFailure, TaskHandle, TaskKind, TaskPhase};
 /// 预览和执行的请求。
 #[derive(Debug, Clone, PartialEq, Deserialize)]
 pub struct DesignRequest {
+    /// 所在的库（SQL Server，M6b 设计文档第 6 节）；其他库不填。
+    #[serde(default, skip_serializing_if = "Option::is_none")]
+    pub catalog: Option<String>,
     pub schema: String,
     /// 已有表的名字；新建表时不给。
     #[serde(default)]
@@ -179,8 +182,14 @@ impl Service {
     }
 
     /// 打开设计器：原表（直接从库读取）、存储属性和行数估计、指纹。
-    pub async fn design_table(&self, profile_id: &str, schema: &str, name: &str) -> Result<DesignTable> {
-        let profile = self.feature_profile(profile_id)?;
+    pub async fn design_table(
+        &self,
+        profile_id: &str,
+        catalog: Option<&str>,
+        schema: &str,
+        name: &str,
+    ) -> Result<DesignTable> {
+        let profile = self.scoped_profile(profile_id, catalog)?;
         let table = self.fresh_table(&profile, schema, name).await?;
         let storage =
             meta::table_storage(&*self.fresh_rows(&profile, MetaKey::TableStorage, schema, name).await?)?;
@@ -217,7 +226,7 @@ impl Service {
     }
 
     async fn prepare_design(&self, profile_id: &str, req: &DesignRequest) -> Result<Prepared> {
-        let profile = self.feature_profile(profile_id)?;
+        let profile = self.scoped_profile(profile_id, req.catalog.as_deref())?;
         let dialect = self.metadata.dialect(&profile).await?;
         let mut draft = req.draft.clone();
         if let Some(name) = &req.name {
@@ -441,7 +450,9 @@ impl Run<'_> {
         self.task.set_cancel(Some(conn.cancel_handle()))?;
         let transactional = self.dialect.design_transactional();
         let outcome = async {
-            exec(&mut conn, &self.dialect.set_schema_sql(self.schema), &[]).await?;
+            if let Some(sql) = self.dialect.schema_switch_sql(self.schema) {
+                exec(&mut conn, &sql, &[]).await?;
+            }
             if transactional {
                 conn.set_autocommit(false).await?;
             }
@@ -611,6 +622,7 @@ mod tests {
             generated: None,
         };
         DesignRequest {
+            catalog: None,
             schema: "public".into(),
             name: None,
             draft: Table {
```

- [ ] **Step 18: 修改 `crates/dblens-service/src/document/job.rs`**

```diff
--- a/crates/dblens-service/src/document/job.rs
+++ b/crates/dblens-service/src/document/job.rs
@@ -56,7 +56,13 @@ pub enum DocSource {
     /// 打开的模型中的若干 schema。
     Model { model: String, schemas: Vec<String> },
     /// 连接的一个 schema。
-    Database { profile: String, schema: String },
+    Database {
+        profile: String,
+        /// 所在的库（SQL Server）；其他库不填。
+        #[serde(default)]
+        catalog: Option<String>,
+        schema: String,
+    },
 }
 
 /// 一张 ER 图的图片（前端画好后上传）。
@@ -203,8 +209,8 @@ impl Service {
                     done();
                 }
             }
-            DocSource::Database { profile, schema } => {
-                let p = self.feature_profile(profile)?;
+            DocSource::Database { profile, catalog, schema } => {
+                let p = self.scoped_profile(profile, catalog.as_deref())?;
                 // 跳过元数据缓存，以库中最新的结构为准
                 let args = MetaArgs { schema: Some(schema.clone()), ..Default::default() };
                 for key in MetaKey::TABLE_DETAIL.into_iter().chain([MetaKey::Views]) {
@@ -235,9 +241,10 @@ impl Service {
                 let m = self.models.summary_of(model)?;
                 Ok((format!("{} ({})", m.info.name, m.path), m.info.name))
             }
-            DocSource::Database { profile, schema } => {
+            DocSource::Database { profile, catalog, schema } => {
                 let p = self.profile(profile)?;
-                Ok((format!("{} / {schema}", p.input.name), schema.clone()))
+                let place = catalog.as_ref().map_or_else(|| schema.clone(), |c| format!("{c}.{schema}"));
+                Ok((format!("{} / {place}", p.input.name), schema.clone()))
             }
         }
     }
@@ -407,7 +414,7 @@ mod tests {
 
     fn req(format: DocFormat, kind: DocKind) -> DocumentRequest {
         DocumentRequest {
-            source: DocSource::Database { profile: "p".into(), schema: "s".into() },
+            source: DocSource::Database { profile: "p".into(), catalog: None, schema: "s".into() },
             tables: None,
             kind,
             format,
```

- [ ] **Step 19: 修改 `crates/dblens-service/src/import/job.rs`**

```diff
--- a/crates/dblens-service/src/import/job.rs
+++ b/crates/dblens-service/src/import/job.rs
@@ -6,6 +6,7 @@ use std::time::{Instant, SystemTime, UNIX_EPOCH};
 
 use dblens_core::schema::{Column, PrimaryKey, Table};
 use dblens_core::{ColumnType, Error, LogicalType, Result, Value, msg};
+use dblens_dialect::migrate::data::identity_insert_sql;
 use dblens_dialect::{Dialect, ImportStatement, WriteMode};
 use dblens_driver::{Connection, ExecEvent};
 use serde::{Deserialize, Serialize};
@@ -73,6 +74,10 @@ pub enum OnError {
 #[derive(Debug, Clone, PartialEq, Deserialize)]
 pub struct ImportRequest {
     pub profile_id: String,
+    /// 所在的库（SQL Server，M6b 设计文档第 6 节）；其他库不填。
+    #[serde(default)]
+    pub catalog: Option<String>,
+
     pub source: FileSource,
     pub file: FileOptions,
     pub schema: String,
@@ -260,11 +265,12 @@ impl Service {
     pub async fn import_ddl(
         &self,
         profile_id: &str,
+        catalog: Option<&str>,
         schema: &str,
         table: &str,
         def: &NewTable,
     ) -> Result<String> {
-        let profile = self.feature_profile(profile_id)?;
+        let profile = self.scoped_profile(profile_id, catalog)?;
         let dialect = self.metadata.dialect(&profile).await?;
         Ok(dialect.create_table_sql(schema, &new_table(&dialect, table, def)?))
     }
@@ -285,7 +291,7 @@ impl Service {
 
     /// 提交导入任务，返回任务 id。提交时检查连接、文件、目标表、映射和执行策略；执行在后台进行。
     pub async fn start_import(self: &Arc<Self>, req: ImportRequest, allow_path: bool) -> Result<String> {
-        let profile = self.feature_profile(&req.profile_id)?;
+        let profile = self.scoped_profile(&req.profile_id, req.catalog.as_deref())?;
         let password = self.secrets.get(&profile.id)?.unwrap_or_default();
         let dialect = self.metadata.dialect(&profile).await?;
         let (table, create_sql) = match &req.create {
@@ -414,12 +420,28 @@ impl Service {
                 exec(&mut conn, sql, &[]).await?;
                 created = true;
             }
+            // 达梦、SQL Server 往自增列写显式值要打开 IDENTITY_INSERT（会话级），写完关掉
+            let identity = plan
+                .columns
+                .iter()
+                .any(|name| plan.table.columns.iter().any(|c| &c.name == name && c.auto_increment));
+            let identity_insert = |on| {
+                identity
+                    .then(|| identity_insert_sql(&plan.dialect, &plan.schema, &plan.table.name, on))
+                    .flatten()
+            };
+            if let Some(sql) = identity_insert(true) {
+                exec(&mut conn, &sql, &[]).await?;
+            }
             conn.set_autocommit(false).await?;
             let result = self.write_rows(task, plan, &mut conn, reader).await;
             if result.is_err() {
                 let _ = conn.rollback().await;
             }
             let restore = conn.set_autocommit(true).await;
+            if let Some(sql) = identity_insert(false) {
+                let _ = exec(&mut conn, &sql, &[]).await;
+            }
             result.and(restore)
         }
         .await;
@@ -621,6 +643,7 @@ mod tests {
     fn request(profile_id: &str, upload: &str) -> ImportRequest {
         ImportRequest {
             profile_id: profile_id.into(),
+            catalog: None,
             source: FileSource::Upload { upload: upload.into() },
             file: FileOptions::csv(),
             schema: "public".into(),
@@ -802,7 +825,7 @@ mod tests {
         let (s, p, _, _dir) = service();
         let def = request(&p, "x").create.unwrap();
         assert_eq!(
-            s.import_ddl(&p, "public", "导入", &def).await.unwrap(),
+            s.import_ddl(&p, None, "public", "导入", &def).await.unwrap(),
             "CREATE TABLE public.\"导入\" (\n    id BIGINT NOT NULL,\n    name VARCHAR(20),\n    PRIMARY KEY (id)\n)"
         );
         let up = upload(&s, 3, &[]);
```

- [ ] **Step 20: 修改 `crates/dblens-service/src/migrate/mod.rs`**

```diff
--- a/crates/dblens-service/src/migrate/mod.rs
+++ b/crates/dblens-service/src/migrate/mod.rs
@@ -34,6 +34,9 @@ pub use run::{MigrationProgress, TableView};
 #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
 pub struct MigrationEnd {
     pub profile: String,
+    /// 所在的库（SQL Server，M6b 设计文档第 6 节）；其他库不填。
+    #[serde(default, skip_serializing_if = "Option::is_none")]
+    pub catalog: Option<String>,
     pub schema: String,
 }
 
@@ -176,8 +179,8 @@ fn problems(plan: &MigrationPlan, draft: &MigrationDraft) -> Vec<Msg> {
 impl Service {
     /// 读两端的结构。`fresh`：跳过元数据缓存（开始执行前以库中最新的结构为准）。
     async fn load_migration(&self, plan: &MigrationPlan, fresh: bool) -> Result<Loaded> {
-        let source = self.feature_profile(&plan.source.profile)?;
-        let target = self.feature_profile(&plan.target.profile)?;
+        let source = self.scoped_profile(&plan.source.profile, plan.source.catalog.as_deref())?;
+        let target = self.scoped_profile(&plan.target.profile, plan.target.catalog.as_deref())?;
         let source_dialect = self.metadata.dialect(&source).await?;
         let target_dialect = self.metadata.dialect(&target).await?;
         let schemas = self.metadata.schemas(&target, None).await?;
@@ -313,7 +316,7 @@ impl Service {
             source_tables: l.tables.clone(),
             source_sequences: l.sequences.clone(),
             problems: problems(plan, &draft),
-            text: script_text(&statements),
+            text: script_text(l.target_dialect.family(), &statements),
             draft,
             others: l.others.clone(),
             statements,
@@ -483,8 +486,10 @@ impl Service {
 
     /// 作为任务执行迁移记录，返回任务 id。结束后刷新目标的元数据缓存、保存记录、写迁移报告。
     async fn launch(self: &Arc<Self>, record: MigrationRecord) -> Result<String> {
-        let source = self.feature_profile(&record.plan.source.profile)?;
-        let target = self.feature_profile(&record.plan.target.profile)?;
+        let source =
+            self.scoped_profile(&record.plan.source.profile, record.plan.source.catalog.as_deref())?;
+        let target =
+            self.scoped_profile(&record.plan.target.profile, record.plan.target.catalog.as_deref())?;
         let source_dialect = self.metadata.dialect(&source).await?;
         let target_dialect = self.metadata.dialect(&target).await?;
         let source_password = self.secrets.get(&source.id)?.unwrap_or_default();
@@ -553,7 +558,7 @@ pub(crate) mod tests {
     use super::*;
 
     fn end(p: &str, s: &str) -> MigrationEnd {
-        MigrationEnd { profile: p.into(), schema: s.into() }
+        MigrationEnd { profile: p.into(), catalog: None, schema: s.into() }
     }
 
     pub(crate) fn plan() -> MigrationPlan {
```

- [ ] **Step 21: 修改 `crates/dblens-service/src/migrate/report.rs`**

````diff
--- a/crates/dblens-service/src/migrate/report.rs
+++ b/crates/dblens-service/src/migrate/report.rs
@@ -4,8 +4,8 @@
 use std::fmt::Write as _;
 
 use dblens_core::{Error, Lang, Msg, Result, msg};
-use dblens_dialect::DdlKind;
 use dblens_dialect::migrate::script_text;
+use dblens_dialect::{DdlKind, Family, Variant};
 
 use super::record::{MigrationRecord, TableState};
 use crate::profile::Profile;
@@ -26,6 +26,10 @@ impl Service {
         lang: Lang,
     ) -> String {
         let t = |m: Msg| m.render(lang);
+        // 脚本按目标的写法结束；连接已删除时按分号
+        let target_family = self
+            .profile(&r.plan.target.profile)
+            .map_or(Family::MySql, |p| Variant::base(p.input.driver).family());
         let mut md = String::new();
         let _ = writeln!(
             md,
@@ -158,7 +162,7 @@ impl Service {
             md,
             "## {}\n\n```sql\n{}\n```",
             t(msg!("migrate.report.script")),
-            script_text(&r.statements)
+            script_text(target_family, &r.statements)
         );
         md
     }
````

- [ ] **Step 22: 修改 `crates/dblens-service/src/migrate/run.rs`**

```diff
--- a/crates/dblens-service/src/migrate/run.rs
+++ b/crates/dblens-service/src/migrate/run.rs
@@ -93,13 +93,17 @@ impl Ctx {
 
     pub async fn connect_source(&self) -> Result<Box<dyn Connection>> {
         let (mut c, _) = self.service.registry.connect(&self.source, self.source_password.clone()).await?;
-        exec(&mut c, &self.source_dialect.set_schema_sql(&self.source_schema()?), &[]).await?;
+        if let Some(sql) = self.source_dialect.schema_switch_sql(&self.source_schema()?) {
+            exec(&mut c, &sql, &[]).await?;
+        }
         Ok(c)
     }
 
     pub async fn connect_target(&self) -> Result<Box<dyn Connection>> {
         let (mut c, _) = self.service.registry.connect(&self.target, self.target_password.clone()).await?;
-        exec(&mut c, &self.target_dialect.set_schema_sql(&self.target_schema()?), &[]).await?;
+        if let Some(sql) = self.target_dialect.schema_switch_sql(&self.target_schema()?) {
+            exec(&mut c, &sql, &[]).await?;
+        }
         Ok(c)
     }
 
```

- [ ] **Step 23: 修改 `crates/dblens-service/src/migrate/values.rs`**

```diff
--- a/crates/dblens-service/src/migrate/values.rs
+++ b/crates/dblens-service/src/migrate/values.rs
@@ -115,7 +115,15 @@ pub fn to_target(v: &Value, col: &Column) -> Result<Value> {
         (L::Decimal { .. }, Value::Float(f)) => Value::Decimal(f.to_string()),
         _ => convert(v, col, EmptyAs::Null)?,
     };
-    Ok(out)
+    // 小数秒超出目标精度时先截断：有的库写入时四舍五入（达梦），有的截断，校验按截断比较
+    Ok(match (out, logical) {
+        (Value::DateTime(t), L::Timestamp { precision, .. }) => Value::DateTime(truncate(&t, *precision)),
+        (Value::DateTimeTz(t), L::Timestamp { precision, .. }) => {
+            let nanos = truncate(&t.naive_utc(), *precision).nanosecond();
+            Value::DateTimeTz(t.with_nanosecond(nanos).unwrap_or(t))
+        }
+        (out, _) => out,
+    })
 }
 
 /// 去掉小数末尾的 0（`1.50` → `1.5`，`2.00` → `2`）。
```

- [ ] **Step 24: 修改 `crates/dblens-service/src/model/ops.rs`**

```diff
--- a/crates/dblens-service/src/model/ops.rs
+++ b/crates/dblens-service/src/model/ops.rs
@@ -33,6 +33,10 @@ pub struct NewModel {
 #[derive(Debug, Clone, PartialEq, Deserialize)]
 pub struct ReverseRequest {
     pub profile_id: String,
+    /// 所在的库（SQL Server，M6b 设计文档第 6 节）；其他库不填。
+    #[serde(default)]
+    pub catalog: Option<String>,
+
     pub schemas: Vec<String>,
     pub path: String,
     pub name: String,
@@ -92,7 +96,7 @@ impl Service {
 
     /// 反向生成：作为任务执行，返回任务 id；结束后模型已打开，任务状态的 `model` 是模型编号。
     pub async fn start_reverse(self: &Arc<Self>, req: ReverseRequest) -> Result<String> {
-        let profile = self.feature_profile(&req.profile_id)?;
+        let profile = self.scoped_profile(&req.profile_id, req.catalog.as_deref())?;
         if req.schemas.is_empty() {
             return Err(Error::invalid(msg!("model.no_schemas")));
         }
@@ -325,6 +329,7 @@ pub(crate) mod tests {
         let path = dir.path().join("rev");
         let req = ReverseRequest {
             profile_id: dev.clone(),
+            catalog: None,
             schemas: vec!["public".into()],
             path: path.to_string_lossy().into(),
             name: "rev".into(),
```

- [ ] **Step 25: 修改 `crates/dblens-service/src/service.rs`**

```diff
--- a/crates/dblens-service/src/service.rs
+++ b/crates/dblens-service/src/service.rs
@@ -3,9 +3,8 @@
 use std::path::Path;
 use std::sync::Arc;
 
-use dblens_core::{Error, Lang, Result, msg, set_default_lang};
+use dblens_core::{Lang, Result, set_default_lang};
 use dblens_dialect::{Dialect, OpsQuery, Variant};
-use dblens_driver::DriverId;
 
 use crate::export::ExportManager;
 use crate::history::HistoryLog;
@@ -103,14 +102,9 @@ impl Service {
         self.store.profile(id)
     }
 
-    /// 写入类功能（表设计器、导入、文档导出、结构对比、迁移、逆向成模型）用的连接配置：SQL Server 在 M6b 之前
-    /// 不支持这些功能，在入口处拒绝，不会生成错误的 SQL。
-    pub(crate) fn feature_profile(&self, id: &str) -> Result<Profile> {
-        let p = self.profile(id)?;
-        if p.input.driver == DriverId::SqlServer {
-            return Err(Error::invalid(msg!("sqlserver.later")));
-        }
-        Ok(p)
+    /// 在某个库中工作的连接配置（SQL Server 的表设计器、导入、比对、迁移等，M6b 设计文档第 6 节）：没给库名时就是连接配置本身。
+    pub(crate) fn scoped_profile(&self, id: &str, catalog: Option<&str>) -> Result<Profile> {
+        Ok(self.profile(id)?.in_catalog(catalog))
     }
 
     /// 新建连接配置；给出密码时一并保存。
```

- [ ] **Step 26: 检查**

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

兼容（设计文档 8 节）：Win11 测试虚拟机 192.168.139.103 上的 SQL Server 2025 Express（实例 `SQLEXPRESS`，固定端口 1433，中文环境、中文报错）。
sa 密码在虚拟机的 `C:\ProgramData\dblens-test\creds.txt`（只有管理员可读），用 `ssh bytes@192.168.139.103 "type …"` 读进变量，把 `DBLENS_TEST_SQLSERVER_URL` 指向它，
再跑一遍上面的 SQL Server 测试和迁移路线。

Expected: 全部测试通过；SQL Server 的实库测试和 10 条迁移路线全部通过；回归无失败。

- [ ] **Step 27: 提交**

```bash
git add crates/dblens-api/src/design.rs crates/dblens-api/src/tasks.rs crates/dblens-api/tests/live.rs crates/dblens-core/locales/app.json crates/dblens-dialect/src/changes.rs crates/dblens-dialect/src/compare/sync.rs crates/dblens-dialect/src/import.rs crates/dblens-dialect/src/lib.rs crates/dblens-dialect/src/migrate/expr.rs crates/dblens-dialect/src/migrate/script.rs crates/dblens-dialect/src/migrate/snapshots/dblens_dialect__migrate__tests__pg_to_sqlserver.snap crates/dblens-dialect/src/migrate/tests.rs crates/dblens-dialect/src/variant.rs crates/dblens-service/src/compare.rs crates/dblens-service/src/design.rs crates/dblens-service/src/document/job.rs crates/dblens-service/src/import/job.rs crates/dblens-service/src/migrate/mod.rs crates/dblens-service/src/migrate/report.rs crates/dblens-service/src/migrate/run.rs crates/dblens-service/src/migrate/values.rs crates/dblens-service/src/model/ops.rs crates/dblens-service/src/service.rs crates/dblens-service/tests/live.rs crates/dblens-service/tests/migrate_live.rs
git commit -m "feat: SQL Server 的导入、比对与模型、文档导出、迁移（后端）" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 4: 前端：取消置灰、按库列出 schema、脚本的 GO

- `shared/api.ts`：去掉 `supportsWrites`；`withCatalog`、`planEnds`：设计器、导入、比对与同步、迁移、反向生成模型、文档导出的请求在发出前拆开合成的 schema；
  反向生成时所选架构不在同一个库中提示 `model.dialog.oneCatalog`；`MigrationEnd` 加 `catalog?`。
- `shared/schemaOptions.ts`（新）：连接下可选的 schema，SQL Server 列出各用户库的非系统架构（值为合成的 schema，标签 `库.架构`）。
- 对象树、数据标签：去掉置灰；比对、迁移、反向生成模型的下拉框用 `schemaOptions`，连接列表不再过滤。
- 迁移方案另存时用 `planEnds` 拆开、打开时 `parsePlan` 合成。
- 设计器的脚本 `scriptOf(steps, driver)`：SQL Server 每条后接一行 `GO`。设计器标题、保存确认、导入的生产环境提示、文档标题用 `schemaLabel`。
- 词条：去掉 `tree.menu.sqlServerLater`，加 `model.dialog.oneCatalog`。

**Files:**
- Modify: `web/src/features/compare/CompareView.tsx`
- Modify: `web/src/features/data/DataView.tsx`
- Modify: `web/src/features/design/DesignView.tsx`
- Modify: `web/src/features/design/SaveDialog.tsx`
- Modify: `web/src/features/design/summary.test.ts`
- Modify: `web/src/features/design/summary.ts`
- Modify: `web/src/features/document/ExportDocDialog.tsx`
- Modify: `web/src/features/import/ImportDialog.tsx`
- Modify: `web/src/features/migrate/MigrateView.tsx`
- Modify: `web/src/features/migrate/plan.ts`
- Modify: `web/src/features/model/ModelDialogs.tsx`
- Modify: `web/src/features/tree/ObjectTree.tsx`
- Modify: `web/src/locales/en/model.json`
- Modify: `web/src/locales/en/tree.json`
- Modify: `web/src/locales/zh-CN/model.json`
- Modify: `web/src/locales/zh-CN/tree.json`
- Modify: `web/src/shared/api.test.ts`
- Modify: `web/src/shared/api.ts`
- Create: `web/src/shared/schemaOptions.ts`

**Interfaces:**
- Consumes：任务 3 的请求字段。
- Produces：`withCatalog<T extends { schema: string }>(x: T): T & { catalog?: string }`、`planEnds(plan: MigrationPlan): MigrationPlan`、
  `schemaOptions(api: Api, profile: Profile): Promise<{ value: string; label: string }[]>`、`scriptOf(steps: DesignStep[], driver: DriverId): string`。

- [ ] **Step 1: 修改 `web/src/features/design/summary.test.ts`**

```diff
--- a/web/src/features/design/summary.test.ts
+++ b/web/src/features/design/summary.test.ts
@@ -63,12 +63,14 @@ describe('变更摘要', () => {
       { kind: 'count_check' as const, source: 'old', target: 'new', changes: [] },
       { kind: 'sql' as const, sql: 'CREATE TRIGGER x BEGIN NULL; END;', changes: [] },
     ]
-    const text = scriptOf(steps, true)
+    const text = scriptOf(steps, 'oracle')
     expect(text.split('\n\n')).toEqual([
       'CREATE TABLE t (a INT);',
       expect.stringMatching(/^-- 核对行数：old 与 new/),
       'CREATE TRIGGER x BEGIN NULL; END;\n/',
     ])
-    expect(scriptOf(steps.slice(2), false)).toBe('CREATE TRIGGER x BEGIN NULL; END;')
+    expect(scriptOf(steps.slice(2), 'mysql')).toBe('CREATE TRIGGER x BEGIN NULL; END;')
+    // SQL Server：每条之后一行 GO
+    expect(scriptOf(steps.slice(2), 'sqlserver')).toBe('CREATE TRIGGER x BEGIN NULL; END;\nGO')
   })
 })
```

- [ ] **Step 2: 修改 `web/src/shared/api.test.ts`**

```diff
--- a/web/src/shared/api.test.ts
+++ b/web/src/shared/api.test.ts
@@ -10,7 +10,7 @@ import {
   schemaLabel,
   sessionSchema,
   splitCatalog,
-  supportsWrites,
+  withCatalog,
 } from './api'
 import i18n from './i18n'
 
@@ -93,7 +93,20 @@ describe('createApi', () => {
     expect(exportSource(joinCatalog('a]b', 'dbo'), 'order', 'id > 1', { column: 'id', desc: true })).toEqual({
       sql: 'SELECT * FROM [a]]b].[dbo].[order] WHERE (id > 1) ORDER BY [id] DESC',
     })
-    expect([supportsWrites('sqlserver'), supportsWrites('postgres')]).toEqual([false, true])
+  })
+
+  it('M6b 的请求把合成的 schema 拆成库和架构', async () => {
+    expect(withCatalog({ schema: 'public', table: 't' })).toEqual({ schema: 'public', table: 't' })
+    expect(withCatalog({ schema: joinCatalog('sales', 'dbo'), table: 't' })).toEqual({ schema: 'dbo', table: 't', catalog: 'sales' })
+    const { fn, calls } = fakeFetch(200, { id: 'x' })
+    const api = createApi('tok', fn)
+    const end = (schema: string) => ({ profile: 'p', schema })
+    await api.startMigration({ source: end(joinCatalog('a', 'dbo')), target: end('public') } as never)
+    const body = JSON.parse(calls[0].init?.body as string)
+    expect([body.source, body.target]).toEqual([{ profile: 'p', schema: 'dbo', catalog: 'a' }, { profile: 'p', schema: 'public' }])
+    await api.startReverse({ profile_id: 'p', schemas: [joinCatalog('a', 'dbo'), joinCatalog('a', 'hr')], path: 'x', name: 'm' })
+    expect(JSON.parse(calls[1].init?.body as string)).toMatchObject({ schemas: ['dbo', 'hr'], catalog: 'a' })
+    await expect(api.startReverse({ profile_id: 'p', schemas: [joinCatalog('a', 'dbo'), joinCatalog('b', 'dbo')], path: 'x', name: 'm' })).rejects.toThrow()
   })
 
   it('204 没有响应体', async () => {
```

- [ ] **Step 3: 修改 `web/src/features/compare/CompareView.tsx`**

```diff
--- a/web/src/features/compare/CompareView.tsx
+++ b/web/src/features/compare/CompareView.tsx
@@ -21,7 +21,7 @@ import type {
   SyncRequest,
   TableStatus,
 } from '../../shared/api'
-import { supportsWrites } from '../../shared/api'
+import { schemaOptions } from '../../shared/schemaOptions'
 import { desktop } from '../../shared/shell'
 import { withVault } from '../connections/vault'
 import { useQueryActions } from '../editor/actions'
@@ -74,16 +74,19 @@ function SidePicker({ label, value, profiles, onChange }: {
     queryFn: () => api.modelTables(model!.id),
     enabled: !!model,
   })
-  const profile = value?.kind === 'database' ? value.profile : null
+  const profile = value?.kind === 'database' ? profiles.find((p) => p.id === value.profile) : undefined
   const { data: dbSchemas = [] } = useQuery({
-    queryKey: ['schemas', profile],
-    queryFn: () => withVault(api, () => api.schemas(profile!)),
+    queryKey: ['schema-options', profile?.id],
+    queryFn: () => withVault(api, () => schemaOptions(api, profile!)),
     enabled: !!profile,
   })
   const schemas =
     value?.kind === 'model'
-      ? [...new Set([...(model?.info.schemas ?? []), ...modelTables.map((x) => x.schema)])]
-      : dbSchemas.map((s) => s.name)
+      ? [...new Set([...(model?.info.schemas ?? []), ...modelTables.map((x) => x.schema)])].map((s) => ({
+          value: s,
+          label: s,
+        }))
+      : dbSchemas
   return (
     <Space.Compact>
       <span className="compare-side-label">{label}</span>
@@ -116,7 +119,7 @@ function SidePicker({ label, value, profiles, onChange }: {
           placeholder={t('model.compare.pickProfile')}
           value={value?.kind === 'database' ? value.profile : undefined}
           onChange={(p) => onChange({ kind: 'database', profile: p, schema: '' })}
-          options={profiles.filter((p) => supportsWrites(p.driver)).map((p) => ({ value: p.id, label: p.name }))}
+          options={profiles.map((p) => ({ value: p.id, label: p.name }))}
         />
       )}
       <Select
@@ -127,7 +130,7 @@ function SidePicker({ label, value, profiles, onChange }: {
         disabled={!value}
         value={value?.schema || undefined}
         onChange={(schema) => value && onChange({ ...value, schema })}
-        options={schemas.map((s) => ({ value: s }))}
+        options={schemas}
       />
     </Space.Compact>
   )
```

- [ ] **Step 4: 修改 `web/src/features/data/DataView.tsx`**

```diff
--- a/web/src/features/data/DataView.tsx
+++ b/web/src/features/data/DataView.tsx
@@ -23,7 +23,7 @@ import { useTranslation } from 'react-i18next'
 
 import { useApi } from '../../app/context'
 import DbIcon from '../../shared/DbIcon'
-import { type ChangeSet, type Profile, errorText, exportSource, schemaLabel, supportsWrites } from '../../shared/api'
+import { type ChangeSet, type Profile, errorText, exportSource, schemaLabel } from '../../shared/api'
 import ResultsPanel from '../results/ResultsPanel'
 import { useQueryActions } from '../editor/actions'
 import { runnerFor, useTabRuntime } from '../editor/runner'
@@ -239,7 +239,7 @@ export default function DataView({ tab, profiles }: { tab: QueryTab; profiles: P
           <Button
             size="small"
             icon={<IconUpload size={14} />}
-            disabled={!profile || !supportsWrites(profile.driver)}
+            disabled={!profile}
             onClick={() =>
               useImportDialogs
                 .getState()
@@ -253,7 +253,7 @@ export default function DataView({ tab, profiles }: { tab: QueryTab; profiles: P
           <Button
             size="small"
             icon={<IconTableOptions size={14} />}
-            disabled={!profile || !supportsWrites(profile.driver)}
+            disabled={!profile}
             onClick={() => useTabs.getState().openDesign(tab.profileId, { schema: data.schema, table: data.object })}
           >
             {t('editor.data.design')}
```

- [ ] **Step 5: 修改 `web/src/features/design/DesignView.tsx`**

```diff
--- a/web/src/features/design/DesignView.tsx
+++ b/web/src/features/design/DesignView.tsx
@@ -12,7 +12,7 @@ import { useTranslation } from 'react-i18next'
 
 import { useApi } from '../../app/context'
 import { useUi } from '../../app/ui'
-import { type DesignRequest, type ModelDesignRequest, type Profile, type TaskStatus, errorText } from '../../shared/api'
+import { type DesignRequest, type ModelDesignRequest, type Profile, type TaskStatus, errorText, schemaLabel } from '../../shared/api'
 import { withVault } from '../connections/vault'
 import { useQueryActions } from '../editor/actions'
 import { type QueryTab, useTabs } from '../editor/tabs'
@@ -229,7 +229,7 @@ export default function DesignView({ tab, profiles }: { tab: QueryTab; profiles:
       <div className="row toolbar">
         <b>
           {modelQ.data && `${modelQ.data.info.name} / `}
-          {isNew ? t('design.newTable') : `${target.schema}.${target.table}`}
+          {isNew ? t('design.newTable') : `${schemaLabel(target.schema)}.${target.table}`}
         </b>
         {profile && <span className="muted">{profile.name}</span>}
         {modelQ.data?.read_only && <span className="err">{t('model.readOnly')}</span>}
@@ -344,7 +344,7 @@ export default function DesignView({ tab, profiles }: { tab: QueryTab; profiles:
                     {p.statements.length > 0 && (
                       <Alert type="info" showIcon message={t(p.transactional ? 'design.sql.transactional' : 'design.sql.autocommit')} />
                     )}
-                    <SqlBox sql={scriptOf(p.statements, profile!.driver === 'oracle' || profile!.driver === 'dameng')} driver={profile!.driver} height="calc(100vh - 360px)" />
+                    <SqlBox sql={scriptOf(p.statements, profile!.driver)} driver={profile!.driver} height="calc(100vh - 360px)" />
                   </>
                 )}
               </div>
```

- [ ] **Step 6: 修改 `web/src/features/design/SaveDialog.tsx`**

```diff
--- a/web/src/features/design/SaveDialog.tsx
+++ b/web/src/features/design/SaveDialog.tsx
@@ -7,7 +7,7 @@ import { useState } from 'react'
 import { useTranslation } from 'react-i18next'
 
 import { useApi } from '../../app/context'
-import { type DesignRequest, type Profile } from '../../shared/api'
+import { type DesignRequest, type Profile, schemaLabel } from '../../shared/api'
 import { useQueryActions } from '../editor/actions'
 import SqlBox from './SqlBox'
 import { describeChange, scriptOf } from './summary'
@@ -34,11 +34,10 @@ export default function SaveDialog({ profile, request, onRebuild, onReload, onCl
     gcTime: 0,
     retry: false,
   })
-  const plsql = profile.driver === 'oracle' || profile.driver === 'dameng'
-  const script = p ? scriptOf(p.statements, plsql) : ''
+  const script = p ? scriptOf(p.statements, profile.driver) : ''
   const blocked =
     !p || p.errors.length > 0 || p.conflict || !!p.rebuild.refused || !!p.denied || p.statements.length === 0
-  const name = `${request.schema}.${request.draft.name}`
+  const name = `${schemaLabel(request.schema)}.${request.draft.name}`
   const execute = async () => {
     setStarting(true)
     try {
```

- [ ] **Step 7: 修改 `web/src/features/design/summary.ts`**

```diff
--- a/web/src/features/design/summary.ts
+++ b/web/src/features/design/summary.ts
@@ -1,6 +1,6 @@
 // 变更列表的可读摘要（保存确认框中列出），以及复制、在查询标签中打开用的脚本文本。
 
-import type { DesignColumn, DesignStep, TableChange } from '../../shared/api'
+import type { DesignColumn, DesignStep, DriverId, TableChange } from '../../shared/api'
 import { t } from '../../shared/i18n'
 
 const list = (cols: string[]) => cols.join(', ')
@@ -68,12 +68,17 @@ function alterLines(from: DesignColumn, to: DesignColumn): string[] {
   return out
 }
 
-/** 预览中的语句拼成脚本：每条以 `;` 结束（PL/SQL 块之后加一行 `/`），由程序执行的核对行数写成注释。 */
-export function scriptOf(steps: DesignStep[], plsql: boolean): string {
+/**
+ * 预览中的语句拼成脚本：每条以 `;` 结束（Oracle、达梦的 PL/SQL 块之后加一行 `/`；SQL Server 每条之后加一行 `GO`），
+ * 由程序执行的核对行数写成注释。
+ */
+export function scriptOf(steps: DesignStep[], driver: DriverId): string {
+  const plsql = driver === 'oracle' || driver === 'dameng'
   return steps
     .map((s) => {
       if (s.kind === 'count_check') return `-- ${t('design.save.countCheck', { source: s.source, target: s.target })}`
       const sql = s.sql.trimEnd()
+      if (driver === 'sqlserver') return `${sql}\nGO`
       if (sql.endsWith(';')) return plsql ? `${sql}\n/` : sql
       return `${sql};`
     })
```

- [ ] **Step 8: 修改 `web/src/features/document/ExportDocDialog.tsx`**

```diff
--- a/web/src/features/document/ExportDocDialog.tsx
+++ b/web/src/features/document/ExportDocDialog.tsx
@@ -7,7 +7,7 @@ import { useEffect, useMemo, useRef, useState } from 'react'
 import { useTranslation } from 'react-i18next'
 
 import { useApi } from '../../app/context'
-import { DEFAULT_DIAGRAM, type DocFormat, type DocumentProblem, errorText } from '../../shared/api'
+import { DEFAULT_DIAGRAM, type DocFormat, type DocumentProblem, errorText, schemaLabel } from '../../shared/api'
 import { useSettings } from '../../shared/settings'
 import { desktop } from '../../shared/shell'
 import { withVault } from '../connections/vault'
@@ -98,7 +98,7 @@ function Dialog({ target }: { target: DocTarget }) {
         {
           schema: target.schema,
           name: DEFAULT_DIAGRAM,
-          title: target.schema,
+          title: schemaLabel(target.schema),
           description: null,
           tables: picked.map((table) => ({ table, logical_keys: [] })),
           display: large ? 'names' : 'columns',
```

- [ ] **Step 9: 修改 `web/src/features/import/ImportDialog.tsx`**

```diff
--- a/web/src/features/import/ImportDialog.tsx
+++ b/web/src/features/import/ImportDialog.tsx
@@ -21,7 +21,7 @@ import { useEffect, useMemo, useState } from 'react'
 import { useTranslation } from 'react-i18next'
 
 import { useApi } from '../../app/context'
-import type { FileOptions, ImportRequest, WriteMode } from '../../shared/api'
+import { type FileOptions, type ImportRequest, type WriteMode, schemaLabel } from '../../shared/api'
 import { cellText } from '../../shared/exec'
 import { withVault } from '../connections/vault'
 import FileField, { type PickedFile } from './FileField'
@@ -198,7 +198,7 @@ export default function ImportDialog() {
         title: t('import.options.prodTitle'),
         content: (
           <div>
-            <div>{t('import.options.prodContent', { table: `${schema}.${targetTable}`, mode: t(`import.options.modes.${mode}`) })}</div>
+            <div>{t('import.options.prodContent', { table: `${schemaLabel(schema)}.${targetTable}`, mode: t(`import.options.modes.${mode}`) })}</div>
             {mode === 'replace' && <div className="err">{t('import.options.replaceWarn')}</div>}
           </div>
         ),
```

- [ ] **Step 10: 修改 `web/src/features/migrate/MigrateView.tsx`**

```diff
--- a/web/src/features/migrate/MigrateView.tsx
+++ b/web/src/features/migrate/MigrateView.tsx
@@ -22,8 +22,9 @@ import {
   type Profile,
   type TaskStatus,
   errorText,
-  supportsWrites,
+  planEnds,
 } from '../../shared/api'
+import { schemaOptions } from '../../shared/schemaOptions'
 import { desktop } from '../../shared/shell'
 import { withVault } from '../connections/vault'
 import SqlBox from '../design/SqlBox'
@@ -56,10 +57,11 @@ function EndPicker({ label, value, profiles, onChange }: {
 }) {
   const api = useApi()
   const { t } = useTranslation()
+  const profile = profiles.find((p) => p.id === value.profile)
   const { data: schemas = [] } = useQuery({
-    queryKey: ['schemas', value.profile],
-    queryFn: () => withVault(api, () => api.schemas(value.profile)),
-    enabled: !!value.profile,
+    queryKey: ['schema-options', value.profile],
+    queryFn: () => withVault(api, () => schemaOptions(api, profile!)),
+    enabled: !!profile,
   })
   return (
     <Space.Compact>
@@ -70,7 +72,7 @@ function EndPicker({ label, value, profiles, onChange }: {
         placeholder={t('migrate.pickProfile')}
         value={value.profile || undefined}
         onChange={(profile) => onChange({ profile, schema: '' })}
-        options={profiles.filter((p) => supportsWrites(p.driver)).map((p) => ({ value: p.id, label: p.name }))}
+        options={profiles.map((p) => ({ value: p.id, label: p.name }))}
       />
       <Select
         size="small"
@@ -80,7 +82,7 @@ function EndPicker({ label, value, profiles, onChange }: {
         disabled={!value.profile}
         value={value.schema || undefined}
         onChange={(schema) => onChange({ ...value, schema })}
-        options={schemas.map((s) => ({ value: s.name }))}
+        options={schemas}
       />
     </Space.Compact>
   )
@@ -322,7 +324,7 @@ export default function MigrateView({ tab, profiles }: { tab: QueryTab; profiles
         </Button>
         <span className="grow" />
         <Button size="small" icon={<IconFolderOpen size={16} />} onClick={() => file.current?.click()}>{t('migrate.openPlan')}</Button>
-        <Button size="small" icon={<IconDeviceFloppy size={16} />} onClick={() => download('migration-plan.json', JSON.stringify(plan, null, 2), 'application/json')}>
+        <Button size="small" icon={<IconDeviceFloppy size={16} />} onClick={() => download('migration-plan.json', JSON.stringify(planEnds(plan), null, 2), 'application/json')}>
           {t('migrate.savePlan')}
         </Button>
         <input ref={file} type="file" accept=".json" hidden onChange={(e) => void openPlan(e.target.files?.[0]).finally(() => (e.target.value = ''))} />
```

- [ ] **Step 11: 修改 `web/src/features/migrate/plan.ts`**

```diff
--- a/web/src/features/migrate/plan.ts
+++ b/web/src/features/migrate/plan.ts
@@ -13,6 +13,7 @@ import type {
   NoteLevel,
   TableOverride,
 } from '../../shared/api'
+import { joinCatalog } from '../../shared/api'
 
 export const DEFAULT_DATA: DataOptions = { batch_size: 1000, parallel: 2, on_error: 'stop', sample: 0 }
 
@@ -141,11 +142,18 @@ const isEnd = (v: unknown): v is MigrationEnd =>
   typeof (v as MigrationEnd)?.profile === 'string' && typeof (v as MigrationEnd).schema === 'string'
 const isList = (v: unknown) => v === null || (Array.isArray(v) && v.every((x) => typeof x === 'string'))
 
+/** 方案文件中库和架构分开存（`planEnds`），读回时合成界面用的 schema。 */
+function joinEnd(e: MigrationEnd): MigrationEnd {
+  return { profile: e.profile, schema: e.catalog ? joinCatalog(e.catalog, e.schema) : e.schema }
+}
+
 /** 解析另存的方案；格式不对时返回 null。缺的设置项补默认值。 */
 export function parsePlan(text: string): MigrationPlan | null {
   try {
     const v = JSON.parse(text) as Partial<MigrationPlan>
     if (!isEnd(v.source) || !isEnd(v.target) || !isList(v.tables ?? null) || !isList(v.sequences ?? null)) return null
+    v.source = joinEnd(v.source)
+    v.target = joinEnd(v.target)
     const base = emptyPlan(v.source)
     const s = (v.settings ?? {}) as Partial<MigrationPlan['settings']>
     return {
```

- [ ] **Step 12: 修改 `web/src/features/model/ModelDialogs.tsx`**

```diff
--- a/web/src/features/model/ModelDialogs.tsx
+++ b/web/src/features/model/ModelDialogs.tsx
@@ -9,7 +9,8 @@ import { useTranslation } from 'react-i18next'
 
 import { useApi } from '../../app/context'
 import { useUi } from '../../app/ui'
-import { type Compat, type TaskStatus, errorText, supportsWrites } from '../../shared/api'
+import { type Compat, type TaskStatus, errorText } from '../../shared/api'
+import { schemaOptions } from '../../shared/schemaOptions'
 import { desktop } from '../../shared/shell'
 import { withVault } from '../connections/vault'
 import { PRODUCTS, type ProductId, productLabel } from '../connections/products'
@@ -146,10 +147,11 @@ function ReverseDialog() {
   useEffect(() => {
     if (!profileId && profiles.length) setProfileId(profiles[0].id)
   }, [profileId, profiles])
+  const profile = profiles.find((p) => p.id === profileId)
   const { data: all = [] } = useQuery({
-    queryKey: ['schemas', profileId],
-    queryFn: () => withVault(api, () => api.schemas(profileId!)),
-    enabled: !!profileId,
+    queryKey: ['schema-options', profileId],
+    queryFn: () => withVault(api, () => schemaOptions(api, profile!)),
+    enabled: !!profile,
   })
 
   // 执行中：轮询任务状态；完成后打开模型
@@ -204,11 +206,11 @@ function ReverseDialog() {
               setProfileId(v)
               setSchemas([])
             }}
-            options={profiles.filter((p) => supportsWrites(p.driver)).map((p) => ({ value: p.id, label: p.name }))}
+            options={profiles.map((p) => ({ value: p.id, label: p.name }))}
           />
         </Form.Item>
         <Form.Item label={t('model.dialog.schemas')}>
-          <Select mode="multiple" value={schemas} onChange={setSchemas} options={all.map((s) => ({ value: s.name }))} />
+          <Select mode="multiple" value={schemas} onChange={setSchemas} options={all} />
         </Form.Item>
         <Form.Item label={t('model.dialog.path')} extra={t('model.dialog.emptyDir')}>
           <PathField value={path} onChange={setPath} />
```

- [ ] **Step 13: 修改 `web/src/features/tree/ObjectTree.tsx`**

```diff
--- a/web/src/features/tree/ObjectTree.tsx
+++ b/web/src/features/tree/ObjectTree.tsx
@@ -3,7 +3,7 @@
 // 右键菜单（M1a）：连接 —— 编辑、复制、删除、刷新；其他节点 —— 复制名称、刷新。
 // 对象节点另有（M1c2）："查看数据"（数据标签，也可双击打开）、"查看 DDL"（弹出只读预览）、
 // "生成 SQL"（插入到当前编辑器）。
-// SQL Server（M6a）：连接下先列库，库下是架构；表设计器、导入、文档导出、比对、保存为模型、迁移的入口置灰。
+// SQL Server（M6a）：连接下先列库，库下是架构。
 
 import { App, Dropdown, Input, type MenuProps, Tree } from 'antd'
 import type { DataNode, EventDataNode } from 'antd/es/tree'
@@ -41,7 +41,6 @@ import {
   exportSource,
   joinCatalog,
   schemaLabel,
-  supportsWrites,
 } from '../../shared/api'
 import DbIcon from '../../shared/DbIcon'
 import { profileKind } from '../connections/form'
@@ -323,29 +322,19 @@ export default function ObjectTree() {
           ]
         : []
 
-  // 下面几个是函数声明而不是箭头函数常量：上面的 menuItems 在渲染时就会调用它们
-  /** 连接是否支持写入类功能（M6b 之前的 SQL Server 不支持）；不支持时这些菜单项置灰并说明。 */
-  function writable(profileId: string) {
-    return supportsWrites(profiles.find((p) => p.id === profileId)?.driver ?? 'mysql')
-  }
-  function later(key: string, label: string, enabled: boolean) {
-    return enabled ? { key, label } : { key, label: <span title={t('tree.menu.sqlServerLater')}>{label}</span>, disabled: true }
-  }
-
   /** schema 节点：新建表、导入到新表、执行 SQL 文件，后面接分隔线。 */
   function schemaItems(k: NodeKey): NonNullable<MenuProps['items']> {
     if (k.type !== 's') return []
-    const w = writable(k.profileId)
     return [
-      later('new-table', t('tree.menu.newTable'), w),
-      later('import-new', t('tree.menu.importNew'), w),
+      { key: 'new-table', label: t('tree.menu.newTable') },
+      { key: 'import-new', label: t('tree.menu.importNew') },
       { key: 'sql-file', label: t('tree.menu.sqlFile') },
       { type: 'divider' },
       { key: 'er', label: t('tree.menu.er') },
-      later('compare-model', t('tree.menu.compareModel'), w),
-      later('save-model', t('tree.menu.saveAsModel'), w),
-      later('export-doc', t('tree.menu.exportDoc'), w),
-      later('migrate', t('tree.menu.migrate'), w),
+      { key: 'compare-model', label: t('tree.menu.compareModel') },
+      { key: 'save-model', label: t('tree.menu.saveAsModel') },
+      { key: 'export-doc', label: t('tree.menu.exportDoc') },
+      { key: 'migrate', label: t('tree.menu.migrate') },
       { type: 'divider' },
     ]
   }
@@ -355,10 +344,9 @@ export default function ObjectTree() {
     if (k.type !== 'o') return []
     const items: NonNullable<MenuProps['items']> = []
     const target = targetKind(k.kind)
-    const w = writable(k.profileId)
     if (target) items.push({ key: 'view-data', label: t('tree.menu.viewData') })
-    if (target === 'table') items.push(later('design', t('tree.menu.design'), w))
-    if (target === 'table') items.push(later('import', t('tree.menu.import'), w))
+    if (target === 'table') items.push({ key: 'design', label: t('tree.menu.design') })
+    if (target === 'table') items.push({ key: 'import', label: t('tree.menu.import') })
     if (target) items.push({ key: 'export-data', label: t('tree.menu.exportData') })
     if (ddlKind(k.kind, k.routineKind)) items.push({ key: 'ddl', label: t('tree.menu.viewDdl') })
     if (target) {
```

- [ ] **Step 14: 修改 `web/src/locales/en/model.json`**

```diff
--- a/web/src/locales/en/model.json
+++ b/web/src/locales/en/model.json
@@ -31,6 +31,7 @@
     "openTitle": "Open Model",
     "createTitle": "New Model",
     "reverseTitle": "Reverse-engineer a Model from a Database",
+    "oneCatalog": "The selected schemas must be in the same database",
     "path": "Model directory",
     "pathHint": "A directory on the server's machine",
     "emptyDir": "The directory must not exist or must be empty",
```

- [ ] **Step 15: 修改 `web/src/locales/en/tree.json`**

```diff
--- a/web/src/locales/en/tree.json
+++ b/web/src/locales/en/tree.json
@@ -46,8 +46,7 @@
     "exportData": "Export Data…",
     "newTable": "New table",
     "exportDoc": "Export Document…",
-    "migrate": "Migrate To…",
-    "sqlServerLater": "This feature is not yet available for SQL Server"
+    "migrate": "Migrate To…"
   },
   "duplicate": {
     "name": "{{name}} copy",
```

- [ ] **Step 16: 修改 `web/src/locales/zh-CN/model.json`**

```diff
--- a/web/src/locales/zh-CN/model.json
+++ b/web/src/locales/zh-CN/model.json
@@ -31,6 +31,7 @@
     "openTitle": "打开模型",
     "createTitle": "新建模型",
     "reverseTitle": "从库反向生成模型",
+    "oneCatalog": "反向生成时所选的架构必须在同一个库中",
     "path": "模型目录",
     "pathHint": "服务所在机器上的目录",
     "emptyDir": "目录必须不存在或为空",
```

- [ ] **Step 17: 修改 `web/src/locales/zh-CN/tree.json`**

```diff
--- a/web/src/locales/zh-CN/tree.json
+++ b/web/src/locales/zh-CN/tree.json
@@ -46,8 +46,7 @@
     "exportData": "导出数据…",
     "newTable": "新建表",
     "exportDoc": "导出文档…",
-    "migrate": "迁移到…",
-    "sqlServerLater": "SQL Server 的这项功能将在后续版本支持"
+    "migrate": "迁移到…"
   },
   "duplicate": {
     "name": "{{name}} 副本",
```

- [ ] **Step 18: 修改 `web/src/shared/api.ts`**

```diff
--- a/web/src/shared/api.ts
+++ b/web/src/shared/api.ts
@@ -51,9 +51,40 @@ export function exportSource(
   return { sql }
 }
 
-/** M6b 之前 SQL Server 不支持的功能（表设计器、导入、文档导出、结构对比、迁移、保存为模型）。 */
-export function supportsWrites(driver: DriverId): boolean {
-  return driver !== 'sqlserver'
+/**
+ * 带 schema 的请求对象在发给后端前拆开合成的 schema：架构留在 `schema`，库放进 `catalog`（M6b 设计文档第 6 节）。
+ * 不带库时原样返回。
+ */
+export function withCatalog<T extends { schema: string }>(x: T): T & { catalog?: string } {
+  const s = splitCatalog(x.schema)
+  return s.catalog === undefined ? x : { ...x, schema: s.schema, catalog: s.catalog }
+}
+
+/** 比对的一边：连接一侧拆开合成的 schema。 */
+function compareSide(side: CompareSideRef): CompareSideRef {
+  return side.kind === 'database' ? withCatalog(side) : side
+}
+
+function compareRequest(req: CompareRequest): CompareRequest {
+  return { ...req, source: compareSide(req.source), target: compareSide(req.target) }
+}
+
+/** 迁移方案的两端拆开合成的 schema（保存的方案文件中库和架构也分开存）。 */
+export function planEnds(plan: MigrationPlan): MigrationPlan {
+  return { ...plan, source: withCatalog(plan.source), target: withCatalog(plan.target) }
+}
+
+/** 从库反向生成模型：SQL Server 的几个架构必须在同一个库中（模型中的 schema 不带库）。 */
+function reverseRequest(req: ReverseRequest): ReverseRequest & { catalog?: string } {
+  const parts = req.schemas.map(splitCatalog)
+  const catalogs = new Set(parts.map((s) => s.catalog))
+  if (catalogs.size > 1) throw new Error(t('model.dialog.oneCatalog'))
+  const catalog = parts[0]?.catalog
+  return { ...req, schemas: parts.map((s) => s.schema), ...(catalog === undefined ? {} : { catalog }) }
+}
+
+function documentRequest(req: DocumentRequest): DocumentRequest {
+  return req.source.kind === 'database' ? { ...req, source: withCatalog(req.source) } : req
 }
 
 /** 服务端的错误对象（dblens_core::Error），按 kind 区分。 */
@@ -482,7 +513,9 @@ export interface SqlFileRequest {
 /** 结构迁移（M5a）：迁移的一端。 */
 export interface MigrationEnd {
   profile: string
+  /** 界面中为合成的 schema（SQL Server 带库），请求和保存时拆成 `catalog` + `schema`。 */
   schema: string
+  catalog?: string
 }
 
 export type NameCase = 'keep' | 'lower' | 'upper'
@@ -1176,8 +1209,8 @@ export function createApi(token: string, fetchFn: FetchFn = (i, o) => fetch(i, o
       request<FilePreview>('POST', '/imports/preview', { source, file }),
     /** 新建表的建表语句预览。 */
     importDdl: async (id: string, schema: string, table: string, create: NewTable) =>
-      (await request<{ sql: string }>('POST', `${p(id)}/imports/ddl`, { schema, table, create })).sql,
-    startImport: async (req: ImportRequest) => (await request<{ id: string }>('POST', '/imports', req)).id,
+      (await request<{ sql: string }>('POST', `${p(id)}/imports/ddl`, withCatalog({ schema, table, create }))).sql,
+    startImport: async (req: ImportRequest) => (await request<{ id: string }>('POST', '/imports', withCatalog(req))).id,
     startSqlFile: async (req: SqlFileRequest) => (await request<{ id: string }>('POST', '/sql-files', req)).id,
     task: (id: string) => request<TaskStatus>('GET', `/tasks/${encodeURIComponent(id)}`),
     /** 进行中的任务取消；已结束的删除记录和报告。 */
@@ -1193,15 +1226,17 @@ export function createApi(token: string, fetchFn: FetchFn = (i, o) => fetch(i, o
       (await request<{ sql: string }>('GET', `/tasks/${encodeURIComponent(id)}/statements/${n}`)).sql,
     designCaps: (id: string) => request<DesignCaps>('GET', `${p(id)}/design/caps`),
     designTable: (id: string, schema: string, name: string) =>
-      request<DesignTableResult>('GET', `${p(id)}/design/table${q({ schema, name })}`),
-    previewDesign: (id: string, req: DesignRequest) => request<DesignPreview>('POST', `${p(id)}/design/preview`, req),
+      request<DesignTableResult>('GET', `${p(id)}/design/table${q({ ...withCatalog({ schema }), name })}`),
+    previewDesign: (id: string, req: DesignRequest) =>
+      request<DesignPreview>('POST', `${p(id)}/design/preview`, withCatalog(req)),
     applyDesign: async (id: string, req: DesignRequest) =>
-      (await request<{ id: string }>('POST', `${p(id)}/design/apply`, req)).id,
+      (await request<{ id: string }>('POST', `${p(id)}/design/apply`, withCatalog(req))).id,
     models: () => request<{ open: ModelSummary[]; recent: string[] }>('GET', '/models'),
     openModel: (path: string) => request<ModelSummary>('POST', '/models/open', { path }),
     createModel: (path: string, name: string, database: ModelDatabase) =>
       request<ModelSummary>('POST', '/models', { path, name, database }),
-    startReverse: async (req: ReverseRequest) => (await request<{ id: string }>('POST', '/models/reverse', req)).id,
+    startReverse: async (req: ReverseRequest) =>
+      (await request<{ id: string }>('POST', '/models/reverse', reverseRequest(req))).id,
     /** 重新加载并返回模型信息。 */
     model: (id: string) => request<ModelSummary>('GET', m(id)),
     closeModel: (id: string) => request<null>('DELETE', m(id)),
@@ -1234,20 +1269,23 @@ export function createApi(token: string, fetchFn: FetchFn = (i, o) => fetch(i, o
       request<{ fingerprint: string }>('DELETE', `${mt(id, schema, table)}/logical-keys/${index}`),
     /** 桌面版：把导出的图片（base64）写到"另存为"选的 .svg、.png 文件。 */
     saveImage: (path: string, data: string) => request<null>('POST', '/files/image', { path, data }),
-    startDocument: async (req: DocumentRequest) => (await request<{ id: string }>('POST', '/documents', req)).id,
+    startDocument: async (req: DocumentRequest) =>
+      (await request<{ id: string }>('POST', '/documents', documentRequest(req))).id,
     /** 开始前检查：模板语法、样式来源、输出位置。 */
     checkDocument: async (req: DocumentRequest) =>
-      (await request<{ problems: DocumentProblem[] }>('POST', '/documents/check', req)).problems,
+      (await request<{ problems: DocumentProblem[] }>('POST', '/documents/check', documentRequest(req))).problems,
     /** 把内置模板写到指定路径（桌面版）。 */
     exportDocTemplate: (format: 'html' | 'markdown', path: string) =>
       request<null>('POST', '/documents/templates/export', { format, path }),
     /** 整个 schema 的表明细（库的 ER 图）。 */
     schemaTables: (id: string, schema: string) => request<DesignTableModel[]>('GET', `${s(id, schema)}/tables`),
-    compare: (req: CompareRequest) => request<CompareResult>('POST', '/compare', req),
+    compare: (req: CompareRequest) => request<CompareResult>('POST', '/compare', compareRequest(req)),
     /** 结构迁移：读结构、转换、检查、生成脚本，不执行。 */
-    previewMigration: (plan: MigrationPlan) => request<MigrationPreview>('POST', '/migrations/preview', plan),
+    previewMigration: (plan: MigrationPlan) =>
+      request<MigrationPreview>('POST', '/migrations/preview', planEnds(plan)),
     /** 开始迁移，返回任务 id。 */
-    startMigration: async (plan: MigrationPlan) => (await request<{ id: string }>('POST', '/migrations', plan)).id,
+    startMigration: async (plan: MigrationPlan) =>
+      (await request<{ id: string }>('POST', '/migrations', planEnds(plan))).id,
     /** 没有完成的迁移。 */
     unfinishedMigrations: () => request<MigrationSummary[]>('GET', '/migrations'),
     /** 继续一个没有完成的迁移，返回任务 id。 */
@@ -1255,9 +1293,12 @@ export function createApi(token: string, fetchFn: FetchFn = (i, o) => fetch(i, o
       (await request<{ id: string }>('POST', `/migrations/${encodeURIComponent(id)}/resume`)).id,
     /** 放弃：删除迁移记录（不动目标库）。 */
     discardMigration: (id: string) => request<null>('DELETE', `/migrations/${encodeURIComponent(id)}`),
-    syncPreview: (req: SyncRequest) => request<SyncPreview>('POST', '/compare/preview', req),
+    syncPreview: (req: SyncRequest) =>
+      request<SyncPreview>('POST', '/compare/preview', { ...req, compare: compareRequest(req.compare) }),
     /** 同步：目标是库时返回任务 id；目标是模型时已经写完，返回 null。 */
-    sync: async (req: SyncRequest) => (await request<{ task: string | null }>('POST', '/compare/sync', req)).task,
+    sync: async (req: SyncRequest) =>
+      (await request<{ task: string | null }>('POST', '/compare/sync', { ...req, compare: compareRequest(req.compare) }))
+        .task,
     /** 桌面版：把同步脚本写到"另存为"选的 .sql 文件。 */
     saveScript: (path: string, text: string) => request<null>('POST', '/compare/script', { path, text }),
     openSession: (profileId: string) => request<SessionInfo>('POST', '/sessions', { profile_id: profileId }),
```

- [ ] **Step 19: 新建 `web/src/shared/schemaOptions.ts`**

```ts
// 连接下可选的 schema（比对、迁移、反向生成模型的下拉框）。SQL Server 有库这一层（M6b）：列出每个用户库的架构，
// 值是合成的 schema（`joinCatalog`），标签写成 `库.架构`；系统库和系统架构不列出。

import { type Api, type Profile, joinCatalog } from './api'

export interface SchemaOption {
  value: string
  label: string
}

export async function schemaOptions(api: Api, profile: Profile): Promise<SchemaOption[]> {
  if (profile.driver !== 'sqlserver') {
    return (await api.schemas(profile.id)).map((s) => ({ value: s.name, label: s.name }))
  }
  const catalogs = (await api.catalogs(profile.id)).filter((c) => !c.is_system)
  const lists = await Promise.all(
    catalogs.map(async (c) =>
      (await api.schemas(profile.id, undefined, c.name))
        .filter((s) => !s.is_system)
        .map((s) => ({ value: joinCatalog(c.name, s.name), label: `${c.name}.${s.name}` })),
    ),
  )
  return lists.flat()
}
```

- [ ] **Step 20: 检查**

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

Expected: 全部通过（前端新增库名拆分、迁移方案、反向生成的测试，设计器脚本的 GO）。

- [ ] **Step 21: 浏览器实测**

浏览器实测（设计文档 8 节）：用 `dblens-server --listen 127.0.0.1:<端口> --data-dir <临时目录> --web-dir web/dist --no-update-check` 启动浏览器模式，
按 `DBLENS_TEST_SQLSERVER_URL`、`DBLENS_TEST_MYSQL_URL` 建两个"测试"环境的连接（可用接口 `POST /api/profiles`）。
1. 对象树中 SQL Server 的架构和表的右键菜单没有置灰的项。
2. 表设计器打开 `dblens_test.dblens_meta.emp`（标题为 `库.架构.表`）；加一列后 SQL 预览为 `ALTER TABLE … ADD …`，提示"整体在一个事务中执行"；保存执行后列已加上；再删掉这列。
3. 结构比对：两边选 SQL Server，schema 下拉框列出 `dblens_test.dblens_meta`、`dblens_test.dbo`；同一个架构与自身比对没有差异。
4. 在 `dblens_meta` 上"迁移到…"：来源为 `dblens_test.dblens_meta`；目标选 MySQL 的 `dblens_test`，读取结构后列出表和需要处理的不支持映射（geography、rowversion）。

- [ ] **Step 22: 提交**

```bash
git add web/src/features/compare/CompareView.tsx web/src/features/data/DataView.tsx web/src/features/design/DesignView.tsx web/src/features/design/SaveDialog.tsx web/src/features/design/summary.test.ts web/src/features/design/summary.ts web/src/features/document/ExportDocDialog.tsx web/src/features/import/ImportDialog.tsx web/src/features/migrate/MigrateView.tsx web/src/features/migrate/plan.ts web/src/features/model/ModelDialogs.tsx web/src/features/tree/ObjectTree.tsx web/src/locales/en/model.json web/src/locales/en/tree.json web/src/locales/zh-CN/model.json web/src/locales/zh-CN/tree.json web/src/shared/api.test.ts web/src/shared/api.ts web/src/shared/schemaOptions.ts
git commit -m "feat(web): SQL Server 的写入类功能取消置灰，schema 下拉框按库列出" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 5: 文档

- `docs/notes/m6b-sqlserver.md`（新）：结构、与设计文档不一致的地方、已知限制、验证。
- `docs/notes/m0-4b-api.md`：写入类请求的 `catalog`。
- 总设计文档：M6b 完成，M6 全部完成。

**Files:**
- Modify: `docs/notes/m0-4b-api.md`
- Create: `docs/notes/m6b-sqlserver.md`
- Modify: `docs/superpowers/specs/2026-10-22-dblens-design.md`

- [ ] **Step 1: 修改 `docs/notes/m0-4b-api.md`**

```diff
--- a/docs/notes/m0-4b-api.md
+++ b/docs/notes/m0-4b-api.md
@@ -104,6 +104,9 @@
 | GET | `/profiles/{id}/ops` | 运维查询（M1c3）：`[{key, name, category, sql}]`，`name` 按请求的语言，`category` 为 `session` / `lock` / `storage` / `sql`。按连接上次识别出的变体选模板，没连接过时按驱动；不连接数据库 |
 
 - SQL Server（M6a）：上面的元数据接口都可加 `catalog=库名`，在该库中查询（元数据连接按库区分）；不给时用连接的默认库。
+- SQL Server（M6b）：写入类功能的请求同样可带库名（其他库不填，行为不变）：表设计器的读原表（`?catalog=`）、预览和执行（请求体的 `catalog`）；
+  导入请求和建表语句预览的 `catalog`；比对的数据库一侧 `{kind: "database", profile, catalog?, schema}`；迁移方案两端 `{profile, catalog?, schema}`；
+  反向生成模型的 `catalog`（所选 schema 都在这个库中）；文档导出的数据库来源 `{kind: "database", profile, catalog?, schema}`。
 
 ### 会话
 
```

- [ ] **Step 2: 新建 `docs/notes/m6b-sqlserver.md`**

````markdown
# M6b SQL Server：表设计器、导入、文档导出、比对与模型、迁移

依据：设计文档 `docs/superpowers/specs/2026-10-29-m6b-sqlserver-design.md`。

## 结构

```text
crates/dblens-dialect/src/design/sqlserver.rs  设计器的变更语句：带名字的默认值（DF_表_列）、执行时查名删除默认值约束、sp_rename、扩展属性注释
crates/dblens-dialect/src/design/sql.rs        SQL Server 分支、可回滚（design_transactional）、rename_table_sql
crates/dblens-dialect/src/design/rebuild.rs    重建：文件组 ON […]、IDENTITY_INSERT、sp_rename 改回原名并把默认值约束改回按原表名命名、sp_refreshview
crates/dblens-dialect/src/design/diff.rs       NONCLUSTERED 是默认的索引方法
crates/dblens-dialect/src/import.rs            每条语句的参数上限由方言给出（SQL Server 2000）、多行 MERGE、NULL 直接写进语句
crates/dblens-dialect/src/changes.rs           表数据编辑：NULL 直接写进语句
crates/dblens-dialect/src/variant.rs           Family::terminate：脚本中一条语句的写法（SQL Server 后接 GO）
crates/dblens-dialect/src/lib.rs               schema_switch_sql：SQL Server 不切换 schema
crates/dblens-dialect/src/compare/sync.rs      同步脚本的表改名用 rename_table_sql
crates/dblens-dialect/src/migrate/             序列（AS BIGINT、NO CACHE / NO CYCLE）、默认值（时间、UUID、布尔）、源表达式的方括号和 N''、
                                               NEXT VALUE FOR 带 schema、script_text 按目标结束语句
crates/dblens-service/src/service.rs           scoped_profile（替代 feature_profile，不再拦截）
crates/dblens-service/src/{design,import/job,compare,document/job,migrate/mod,model/ops}.rs  请求加 catalog
crates/dblens-service/src/import/job.rs        映射了自增列时包上 IDENTITY_INSERT
crates/dblens-service/src/migrate/values.rs    时间按目标精度截断后再写
crates/dblens-api/src/{design,tasks}.rs        读原表、建表语句预览的 catalog 参数
web/src/shared/api.ts                          withCatalog、planEnds：请求和保存的方案中拆开合成的 schema
web/src/shared/schemaOptions.ts                连接下可选的 schema（SQL Server 列出各用户库的架构）
web/src/features/{tree,data,compare,migrate,model,design,import,document}/  取消置灰、下拉框、GO、库.架构 的显示
```

## 与设计文档不一致的地方

- 库名的传递（设计文档第 6 节，写计划时补充）：前端界面中仍用合成的 schema（对象树、标签、比对标签的状态），只在请求后端和另存迁移方案时拆成
  `catalog` + `schema`；比对标签保存在本机的界面状态中，没有拆开。
- SQL Server 的反向生成模型：所选的架构必须在同一个库中（模型中的 schema 不带库），否则前端提示。
- schema 下拉框只列用户库和非系统架构（系统库 master、model、msdb、tempdb 不列）。
- 写入时的 NULL：驱动把类型未知的 NULL 参数按 nvarchar 传，写进 varbinary 等列会报"不能隐式转换"（M6a 的假设不对）。
  导入、数据迁移、表数据编辑对 SQL Server 改为把 NULL 直接写进语句，不占参数。
- 删除默认值约束的那段 T-SQL 用 `sp_executesql` 执行拼好的语句：`EXEC (…)` 的括号里只能拼字面量和变量，不能调用 `QUOTENAME`。
- 重建：新表上的默认值约束按临时表名命名（约束名在 schema 内不能重复），改回原名后再用 `sp_rename … 'OBJECT'` 改成按原表名命名，
  否则下次重建同一张表时重名。依赖本表的视图在重建后执行 `sp_refreshview`。
- 改列类型：实测 SQL Server 允许在有默认值约束、唯一约束的列上直接 `ALTER COLUMN`（加长），不必先删再建。
- 导入的按主键更新：自增列不能更新，不写进 `MERGE` 的 `UPDATE SET`。
- 迁移（实测发现后修正）：
  - 序列写成 `AS BIGINT … NO CACHE / NO CYCLE`，超出 BIGINT 的边界（如 Oracle 的 28 个 9）不写；
  - 默认值：布尔写 1、0；当前时间写 `SYSDATETIME()`、`SYSDATETIMEOFFSET()`、`CAST(GETDATE() AS DATE)`，UUID 写 `NEWID()`；
    SQL Server 作为来源时认得 `SYSDATETIME`、`SYSDATETIMEOFFSET`；
  - SQL Server 作为来源时，检查约束等表达式中的 `[列]`、`N'…'` 能转换；
  - 默认值 `NEXT VALUE FOR 序列` 补上 schema（SQL Server 不切换当前 schema）；
  - 时间值按目标列的精度截断后再写（所有目标）：达梦写入时四舍五入，与校验的截断不一致。
- 同步脚本的表改名统一由 `rename_table_sql` 生成：SQL Server 用 `sp_rename`；MySQL 的新名带 schema（原来不带，当前库不同时会把表移到别的库）。
- 比对和设计器把 `NONCLUSTERED` 视为默认的索引方法（与 BTREE、NORMAL 相同），否则同一个库反向生成的模型与库比对时每个索引都报差异。

## 已知限制

- 非 Unicode 的 `varchar`、`char` 列在默认排序规则下存不了中文（数据库的限制）；DBLens 生成的字符串列一律是 `nvarchar`。
- 改列类型时，若有索引、检查约束、计算列引用该列，由服务器报错、整体回滚；不自动删除再重建。
- 带 `SCHEMABINDING` 的视图引用的表不能重建（删表失败、整体回滚）。
- 触发器定义中不带 schema 的表名按执行者的默认架构解析（重建时原样执行触发器 DDL）。
- 索引的包含列、筛选条件不能在设计器中编辑；分区表、内存优化表、列存储索引、全文索引不支持设计。
- geometry、geography、hierarchyid、sql_variant、rowversion 列的迁移需人工处理（迁移方案中列为不支持的映射）。

## 验证

- 后端：fmt、clippy 通过；全部测试在 `DBLENS_LANG=en` 下通过（不连库 837 个）。按任务拆分的每个提交都单独跑过 fmt、clippy 和全部测试。
  - 方言层：SQL Server 的建表、全部变更、改列、重建快照；默认值约束名、计算列表达式；2000 个参数的拆分、多行 MERGE（NULL 直接写）；脚本的 GO；
    迁移的 PG → SQL Server 快照、布尔和时间默认值、来源表达式的方括号和 N''。
- 实库（串行）：
  - SQL Server 2022（测试虚拟机 192.168.139.106 上的 Docker）：设计器（加列、删列、改名、改类型、改默认值、注释、约束、索引；一次重建；中途失败整体回滚）、
    导入（追加、清空后导入、按主键更新、带自增列）、模型与比对（反向生成、改名和列变化的同步、写回模型）、文档导出、M6a 的全部测试。
  - 迁移：SQL Server ↔ MySQL、PG、Oracle、达梦 8 条路线和以 SQL Server 为目标的续传、跳过各 1 条（行数和抽样逐值比对一致，自增和序列接着往下走）。
  - 回归：原有 12 个实库目标加 SQL Server 2022 一起串行跑全部后端测试，813 个通过（约 6 分钟；达梦偶发的"网络通讯失败"重跑后通过）。
  - SQL Server 2025 Express（Win11 测试虚拟机 192.168.139.103，中文环境）：上面 SQL Server 的测试和 10 条迁移路线全部通过。
- 前端：tsc、Vitest 287 个（库名拆分、迁移方案、反向生成的库检查、设计器脚本的 GO）、打包通过。
- 浏览器：对象树中 SQL Server 的菜单不再置灰；设计器读原表（标题 `库.架构.表`）、加列的预览（一个事务）、保存执行后列已加上，再删掉；
  比对的 schema 下拉框列出 `dblens_test.dblens_meta`、`dblens_test.dbo`，同一个架构与自身比对没有差异；从对象树"迁移到…" MySQL，读取结构后列出表和需要处理的不支持映射。
- 未验证：桌面程序（Tauri）中的这些功能；文档导出和导入在浏览器中的完整操作（接口转换有单元测试，后端有实库测试）；SQL Server 2017。
````

- [ ] **Step 3: 修改 `docs/superpowers/specs/2026-10-22-dblens-design.md`**

```diff
--- a/docs/superpowers/specs/2026-10-22-dblens-design.md
+++ b/docs/superpowers/specs/2026-10-22-dblens-design.md
@@ -338,7 +338,8 @@ trait Policy: Send + Sync {
 - M5a 已完成（设计 `docs/superpowers/specs/2026-10-28-m5a-structure-migration-design.md`，详细计划 `docs/superpowers/plans/2026-10-28-m5a-structure-migration.md`，说明 `docs/notes/m5a-structure-migration.md`）。
 - M5b 已完成（设计 `docs/superpowers/specs/2026-10-28-m5b-data-migration-design.md`，详细计划 `docs/superpowers/plans/2026-10-29-m5b-data-migration.md`，说明 `docs/notes/m5b-data-migration.md`）。M5 全部完成。
 - 2026-10-29 用户决定：国产库第二批取消，兼容 MySQL / PG / Oracle 协议的产品不做产品识别；先做 SQL Server，分两份：M6a 连接、浏览、查询；M6b 表设计器、导入、文档导出、比对、迁移。
-- M6a 已完成（设计 `docs/superpowers/specs/2026-10-29-m6a-sqlserver-design.md`，详细计划 `docs/superpowers/plans/2026-10-29-m6a-sqlserver.md`，说明 `docs/notes/m6a-sqlserver.md`）。M6b 尚未开始。
+- M6a 已完成（设计 `docs/superpowers/specs/2026-10-29-m6a-sqlserver-design.md`，详细计划 `docs/superpowers/plans/2026-10-29-m6a-sqlserver.md`，说明 `docs/notes/m6a-sqlserver.md`）。
+- M6b 已完成（设计 `docs/superpowers/specs/2026-10-29-m6b-sqlserver-design.md`，详细计划 `docs/superpowers/plans/2026-10-29-m6b-sqlserver.md`，说明 `docs/notes/m6b-sqlserver.md`）。M6 全部完成，SQL Server 与其他库同等可用。
 
 ## 12. 风险与待验证项
 
```

- [ ] **Step 4: 检查**

检查文档中的路径和接口与代码一致；按执行时的实际结果更新"验证"一节（测试数、实库与浏览器实测的内容）。

Expected: 无。

- [ ] **Step 5: 提交**

```bash
git add docs/notes/m0-4b-api.md docs/notes/m6b-sqlserver.md docs/superpowers/specs/2026-10-22-dblens-design.md
git commit -m "docs: M6b 说明文档、接口与总设计文档更新" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```
