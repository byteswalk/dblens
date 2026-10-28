# M4b ER 图 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在模型的 schema 上画 ER 图（默认图加主题图，布局存回模型目录），按列画外键连线，自定义逻辑外键（不参与比对和同步），导出 SVG / PNG；连接的 schema 也能看只读的 ER 图。

**Architecture:** 服务层在模型目录中读写图文件（`diagrams/<schema>/<图名>.yaml`），逻辑外键存在表文件中、只在服务层的模型表里，比对和设计器看不到；接口在 `/models` 下；前端用 React Flow 画表节点和列级连线、dagre 自动布局、html-to-image 导出。

**Tech Stack:** Rust（axum、serde、serde-saphyr、insta）；React 19 + TypeScript、antd 6、@xyflow/react、@dagrejs/dagre、html-to-image、zustand、TanStack Query、Vitest。

**Spec:** `docs/superpowers/specs/2026-10-28-m4b-er-diagram-design.md`

## Global Constraints

- ER 图画在模型的一个 schema 上：每个 schema 有默认图 `_all`（全部表），另可建主题图；库的 schema 也能看，只读、自动布局、不保存，可另存为模型。
- 逻辑外键存在表文件中，不参与比对和同步；图上画虚线。
- 图上能做的编辑：布局、逻辑外键、主题图的表；表结构用 M3 设计器。
- 导出 SVG、PNG。
- 模型文件写法为 git 而定：字段顺序固定，不认识的字段原样保留，写文件先写临时文件再改名；保存前核对指纹。
- 模型目录是服务所在机器上的路径：只有桌面程序和只监听本机地址的浏览器模式可以使用模型。
- 依赖只用正式发布版，跟随最新；新版发布满 7 天再升级（有意固定版本的记入 `scripts/check-deps.mjs` 的 `HOLD`）。
- 前端给用户看的文字都用词条（中英文），英文区分单复数；后端错误文字用 `msg!` 词条（`src` 中的中文字面量只能出现在 `#[cfg(test)]` 之后）；日志不翻译。
- 代码注释用中文；文件 UTF-8、LF 换行；前端代码风格：无分号、单引号、两格缩进。
- cargo 命令加 `--config "source.mirror.registry='sparse+https://rsproxy.cn/index/'"`（clippy 写在 `clippy` 之后），并设 `CARGO_INCREMENTAL=0`；后端测试在 `DBLENS_LANG=en` 下运行。
- 前端工具用 PowerShell 执行（`npm`/`npx` 在本机不可用，用 `pnpm exec`）。
- 测试数据库的密码只在命令行中以内联环境变量传入，不写进仓库、报告或提交信息。

## 与设计文档的差异（写计划时实测后调整）

- `positions` 用块写法，`y` 写成 `"y"`（YAML 1.1 的布尔值），serde-saphyr 自动加引号。
- 在图上拖线只建单列的逻辑外键；复合的在表文件中手写。
- "自动布局"菜单只有横向、纵向的全部重排；没有位置的表打开、加表时自动排在已有部分的下方。
- 导出的"白底"只换画布背景；导出不内嵌网页字体。
- `html-to-image` 固定 1.11.11，依赖复查脚本加"有意不升级"的名单。
- 库图的标签绑定连接（带环境色）。

## 执行说明

任务 2–7 的代码都已在临时副本中逐任务实现并通过检查，执行时按顺序重放。新文件给出全文，修改的文件给出 diff（基于上一个任务完成后的状态，可直接 `git apply`）。`Cargo.lock` 的变化由 cargo 生成；`web/pnpm-lock.yaml` 由任务 6 的 `pnpm add` 生成。

写计划时测试虚拟机不可达：任务 5 的实库测试只编译通过、没有运行；浏览器实测只覆盖了由接口建立的模型（第 1–3 步和导出 PNG），库图、主题图、设计器联动、SVG、200 张表在执行时补测。

## 文件结构

```text
crates/dblens-service/src/model/{file.rs, store.rs, mod.rs, diagram.rs（新增）, snapshots/}; src/lib.rs; tests/live.rs
crates/dblens-api/src/{diagrams.rs（新增）, models.rs, lib.rs}, Cargo.toml, tests/models.rs
crates/dblens-core/locales/app.json
web/src/features/er/{graph.ts, graph.test.ts, TableNode.tsx, ErView.tsx, export.ts, events.ts}（新增）
web/src/features/{editor/{tabs.ts, tabs.test.ts, Workspace.tsx, actions.tsx}, model/ModelsPanel.tsx, tree/ObjectTree.tsx, design/DesignView.tsx}
web/src/{shared/api.ts, app/{StatusBar.tsx, shell.css}, locales/{zh-CN,en}/{er.json（新增）, tree.json}, locales/index.ts}; web/package.json, web/pnpm-lock.yaml; scripts/check-deps.mjs
docs/notes/{m4b-er-diagram.md（新增）, m0-4b-api.md}; docs/superpowers/specs/2026-10-22-dblens-design.md
```

---

### Task 1: 依赖复查

按依赖原则（M1b2 设计文档 3.1 节）复查前端和后端的全部直接依赖。本里程碑在任务 6 新增三个前端依赖：`@xyflow/react` 12.12.0（2026-09-24 发布，MIT）、`@dagrejs/dagre` 3.1.1（2026-08-08 发布，MIT；3.1.2、3.1.3 写计划时发布不满 7 天）、`html-to-image` 1.11.11（MIT；React Flow 文档的导出示例固定这个版本，更新的版本导出图片有问题，所以有意不升级）。

**Files:** 视复查结果而定（`web/package.json`、`Cargo.toml` 及锁文件）。

- [ ] **Step 1: 运行复查脚本**

本机经代理上网，Node 的 `fetch` 默认不读 `HTTPS_PROXY`，必须先设 `NODE_USE_ENV_PROXY=1`，否则全部查询失败，最后仍显示"0 个可升级"。

```powershell
$env:PATH = "D:\Environments\nvm4w\nodejs;$env:PATH"; $env:NODE_USE_ENV_PROXY = '1'; $env:HTTPS_PROXY = 'http://127.0.0.1:6789'; node scripts/check-deps.mjs
```

Expected：输出中没有"查询失败"。以执行当天的结果为准；`@dagrejs/dagre` 如有发布满 7 天的更新的 3.x，任务 6 中用新版本（`graphlib.Graph`、`layout` 的用法不变时）。

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

### Task 2: 服务层：模型中的逻辑外键

- `model/file.rs`：`LogicalKey { name, columns, ref_schema, ref_table, ref_columns, comment }`；表文件在 `foreign_keys` 之后写 `logical_keys`；`ModelTable.logical_keys`；`ModelTable::new(table)`（新表，原来各处的字面量改用它）。
- `model/store.rs`：`update_refs` 的回调改为修改"引用的表名、列"，真外键和逻辑外键共用；设计器保存时列改名也改本表逻辑外键的列和自引用的列；表改名时本表自引用的逻辑外键跟着改；`add_logical_key`（两边的列存在、列数一致且不为 0、名字不与本表的外键和逻辑外键重复，核对表文件的指纹）、`delete_logical_key`（按序号）。比对用的 `Table` 中没有逻辑外键，比对、同步不受影响。
- 后端词条加 `model.lk_column_missing`、`model.lk_count_mismatch`、`model.lk_name_exists`、`model.lk_not_found`。

快照文件（`snapshots/`）随本任务给出；改了生成逻辑时用 `INSTA_UPDATE=always` 重新生成并逐个审查差异。

**Files:**
- Modify: `crates/dblens-core/locales/app.json`
- Modify: `crates/dblens-service/src/lib.rs`
- Modify: `crates/dblens-service/src/model/file.rs`
- Modify: `crates/dblens-service/src/model/mod.rs`
- Modify: `crates/dblens-service/src/model/snapshots/dblens_service__model__file__tests__table_yaml.snap`
- Modify: `crates/dblens-service/src/model/store.rs`

**Interfaces:**
- Consumes：M4a 的 `ModelStore`、表文件。
- Produces（`dblens_service`）：`LogicalKey`；`ModelStore::add_logical_key(id, schema, table, key: LogicalKey, expected: Option<&str>) -> Result<String>`、`ModelStore::delete_logical_key(id, schema, table, index: usize, expected: Option<&str>) -> Result<String>`；crate 内 `ModelTable { table, logical_keys, extra, column_extra }`、`ModelTable::new`。

- [ ] **Step 1: 修改 `crates/dblens-core/locales/app.json`**

```diff
--- a/crates/dblens-core/locales/app.json
+++ b/crates/dblens-core/locales/app.json
@@ -148,6 +148,10 @@
   "model.conflict": {"zh-CN": "%{name} 的文件已被修改，请重新加载", "en": "The file for %{name} has changed; reload it"},
   "model.dir_not_empty": {"zh-CN": "目录 %{path} 不为空", "en": "Directory %{path} is not empty"},
   "model.io_failed": {"zh-CN": "读写 %{path} 失败：%{error}", "en": "Failed to access %{path}: %{error}"},
+  "model.lk_column_missing": {"zh-CN": "列 %{name} 不存在", "en": "Column %{name} does not exist"},
+  "model.lk_count_mismatch": {"zh-CN": "逻辑外键两边的列数必须相同且不为空", "en": "Both sides of a logical key must have the same, non-zero number of columns"},
+  "model.lk_name_exists": {"zh-CN": "名字 %{name} 已被本表的外键或逻辑外键使用", "en": "The name %{name} is already used by a key of this table"},
+  "model.lk_not_found": {"zh-CN": "没有第 %{index} 个逻辑外键", "en": "Logical key %{index} does not exist"},
   "model.local_only": {"zh-CN": "只有桌面程序和只监听本机地址的浏览器模式可以使用模型", "en": "Models are available only in the desktop app and in browser mode listening on a local address"},
   "model.no_schemas": {"zh-CN": "没有选择 schema", "en": "No schema selected"},
   "model.not_a_model": {"zh-CN": "%{path} 不是模型目录（没有 model.yaml）", "en": "%{path} is not a model directory (model.yaml not found)"},
```

- [ ] **Step 2: 修改 `crates/dblens-service/src/lib.rs`**

```diff
--- a/crates/dblens-service/src/lib.rs
+++ b/crates/dblens-service/src/lib.rs
@@ -44,7 +44,7 @@ pub use import::{
 };
 pub use metadata::{METADATA_POOL_SIZE, MetadataCache, MetadataService};
 pub use model::{
-    FileError, ModelDatabase, ModelDesignPreview, ModelDesignRequest, ModelInfo, ModelStore, ModelSummary,
+    FileError, LogicalKey, ModelDatabase, ModelDesignPreview, ModelDesignRequest, ModelInfo, ModelStore, ModelSummary,
     ModelTableView, ModelWrite, NewModel, RECENT_MODELS_SETTING, ReverseRequest, SchemaRenames, SchemaTables,
     TableEntry,
 };
```

- [ ] **Step 3: 修改 `crates/dblens-service/src/model/file.rs`**

```diff
--- a/crates/dblens-service/src/model/file.rs
+++ b/crates/dblens-service/src/model/file.rs
@@ -163,6 +163,8 @@ struct TableFile {
     #[serde(default, skip_serializing_if = "Vec::is_empty")]
     foreign_keys: Vec<ForeignKeyFile>,
     #[serde(default, skip_serializing_if = "Vec::is_empty")]
+    logical_keys: Vec<LogicalKey>,
+    #[serde(default, skip_serializing_if = "Vec::is_empty")]
     checks: Vec<CheckFile>,
     #[serde(default, skip_serializing_if = "Vec::is_empty")]
     indexes: Vec<IndexFile>,
@@ -172,15 +174,37 @@ struct TableFile {
     extra: Extra,
 }
 
-/// 读出的一张表：结构、文件中不认识的字段（保存时写回）。
+/// 逻辑外键（M4b）：库中没有建的关系，只在模型中记录，不参与比对和同步。
+#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
+pub struct LogicalKey {
+    #[serde(default, skip_serializing_if = "Option::is_none")]
+    pub name: Option<String>,
+    pub columns: Vec<String>,
+    #[serde(default, skip_serializing_if = "Option::is_none")]
+    pub ref_schema: Option<String>,
+    pub ref_table: String,
+    pub ref_columns: Vec<String>,
+    #[serde(default, skip_serializing_if = "Option::is_none")]
+    pub comment: Option<String>,
+}
+
+/// 读出的一张表：结构、逻辑外键、文件中不认识的字段（保存时写回）。
 #[derive(Debug, Clone, PartialEq)]
 pub struct ModelTable {
     pub table: Table,
+    pub logical_keys: Vec<LogicalKey>,
     pub extra: Extra,
     /// 每列不认识的字段，按列名。
     pub column_extra: BTreeMap<String, Extra>,
 }
 
+impl ModelTable {
+    /// 新表：没有逻辑外键和扩展字段。
+    pub fn new(table: Table) -> Self {
+        Self { table, logical_keys: vec![], extra: Extra::new(), column_extra: BTreeMap::new() }
+    }
+}
+
 fn to_file(t: &ModelTable) -> TableFile {
     let tab = &t.table;
     let key =
@@ -243,6 +267,7 @@ fn to_file(t: &ModelTable) -> TableFile {
             })
             .collect(),
         partition: tab.partition.clone(),
+        logical_keys: t.logical_keys.clone(),
         extra: t.extra.clone(),
     }
 }
@@ -317,7 +342,7 @@ fn from_file(f: TableFile, d: &Dialect) -> ModelTable {
         triggers: vec![],
         partition: f.partition,
     };
-    ModelTable { table, extra: f.extra, column_extra }
+    ModelTable { table, logical_keys: f.logical_keys, extra: f.extra, column_extra }
 }
 
 fn yaml_options() -> serde_saphyr::SerializerOptions {
@@ -477,7 +502,16 @@ mod tests {
             }],
             ..Default::default()
         };
-        ModelTable { table, extra: Extra::new(), column_extra: BTreeMap::new() }
+        let mut t = ModelTable::new(table);
+        t.logical_keys = vec![LogicalKey {
+            name: Some("lk_cust".into()),
+            columns: vec!["code".into()],
+            ref_schema: None,
+            ref_table: "customers".into(),
+            ref_columns: vec!["code".into()],
+            comment: Some("历史原因没建外键".into()),
+        }];
+        t
     }
 
     #[test]
```

- [ ] **Step 4: 修改 `crates/dblens-service/src/model/mod.rs`**

```diff
--- a/crates/dblens-service/src/model/mod.rs
+++ b/crates/dblens-service/src/model/mod.rs
@@ -4,6 +4,6 @@ mod file;
 mod ops;
 mod store;
 
-pub use file::{ModelDatabase, ModelInfo, SchemaRenames};
+pub use file::{LogicalKey, ModelDatabase, ModelInfo, SchemaRenames};
 pub use ops::{ModelDesignPreview, ModelDesignRequest, NewModel, RECENT_MODELS_SETTING, ReverseRequest};
 pub use store::{FileError, ModelStore, ModelSummary, ModelTableView, ModelWrite, SchemaTables, TableEntry};
```

- [ ] **Step 5: 修改 `crates/dblens-service/src/model/snapshots/dblens_service__model__file__tests__table_yaml.snap`**

```diff
--- a/crates/dblens-service/src/model/snapshots/dblens_service__model__file__tests__table_yaml.snap
+++ b/crates/dblens-service/src/model/snapshots/dblens_service__model__file__tests__table_yaml.snap
@@ -33,6 +33,14 @@ foreign_keys:
   ref_columns:
   - code
   on_delete: CASCADE
+logical_keys:
+- name: lk_cust
+  columns:
+  - code
+  ref_table: customers
+  ref_columns:
+  - code
+  comment: 历史原因没建外键
 checks:
 - name: ck_amount
   expression: amount >= 0
```

- [ ] **Step 6: 修改 `crates/dblens-service/src/model/store.rs`**

```diff
--- a/crates/dblens-service/src/model/store.rs
+++ b/crates/dblens-service/src/model/store.rs
@@ -12,7 +12,7 @@ use dblens_dialect::Dialect;
 use serde::Serialize;
 
 use super::file::{
-    FORMAT, ModelDatabase, ModelInfo, ModelTable, SchemaRenames, fingerprint, info_path, info_yaml, io_err,
+    FORMAT, LogicalKey, ModelDatabase, ModelInfo, ModelTable, SchemaRenames, fingerprint, info_path, info_yaml, io_err,
     parse_info, parse_table, table_path, table_yaml, unescape_name, write_atomic,
 };
 
@@ -265,12 +265,13 @@ impl OpenModel {
         Ok(fingerprint(text.as_bytes()))
     }
 
-    /// 其他表中引用本 schema 的外键交给 `fix` 修改（返回是否改了），改过的表写回文件。
+    /// 其他表中引用本 schema 的外键和逻辑外键交给 `fix` 修改（参数为引用的表名、列，返回是否改了），
+    /// 改过的表写回文件。
     fn update_refs(
         &mut self,
         schema: &str,
         skip: Option<&Path>,
-        mut fix: impl FnMut(&mut dblens_core::schema::ForeignKey) -> bool,
+        mut fix: impl FnMut(&mut String, &mut Vec<String>) -> bool,
     ) -> Result<()> {
         let mut changed = Vec::new();
         for (p, c) in &self.files {
@@ -283,7 +284,13 @@ impl OpenModel {
             let mut hit = false;
             for f in &mut t.table.foreign_keys {
                 let ref_schema = f.ref_schema.clone().unwrap_or_else(|| own.clone());
-                if ref_schema == schema && fix(f) {
+                if ref_schema == schema && fix(&mut f.ref_table, &mut f.ref_columns) {
+                    hit = true;
+                }
+            }
+            for k in &mut t.logical_keys {
+                let ref_schema = k.ref_schema.clone().unwrap_or_else(|| own.clone());
+                if ref_schema == schema && fix(&mut k.ref_table, &mut k.ref_columns) {
                     hit = true;
                 }
             }
@@ -382,7 +389,7 @@ impl ModelStore {
         for (schema, list) in tables {
             for t in list {
                 let mt =
-                    ModelTable { table: t.clone(), extra: BTreeMap::new(), column_extra: BTreeMap::new() };
+                    ModelTable::new(t.clone());
                 write_atomic(&table_path(&root, schema, &t.name), &table_yaml(&mt)?)?;
             }
         }
@@ -509,11 +516,7 @@ impl ModelStore {
                             name = format!("{schema}.{}", table.name)
                         )));
                     }
-                    let mt = ModelTable {
-                        table: table.clone(),
-                        extra: BTreeMap::new(),
-                        column_extra: BTreeMap::new(),
-                    };
+                    let mt = ModelTable::new(table.clone());
                     (table_path(&m.root, schema, &table.name), mt)
                 }
             };
@@ -525,13 +528,20 @@ impl ModelStore {
                     mt.column_extra.insert(to.clone(), x);
                 }
                 let (tn, from_c, to_c) = (tname.clone(), from.clone(), to.clone());
-                m.update_refs(schema, Some(&path), |f| {
-                    let hit = f.ref_table == tn && f.ref_columns.contains(&from_c);
+                m.update_refs(schema, Some(&path), |ref_table, ref_columns| {
+                    let hit = *ref_table == tn && ref_columns.contains(&from_c);
                     if hit {
-                        f.ref_columns.iter_mut().filter(|c| **c == from_c).for_each(|c| *c = to_c.clone());
+                        ref_columns.iter_mut().filter(|c| **c == from_c).for_each(|c| *c = to_c.clone());
                     }
                     hit
                 })?;
+                // 本表的逻辑外键：本表的列、自引用的列
+                for k in &mut mt.logical_keys {
+                    k.columns.iter_mut().filter(|c| **c == from_c).for_each(|c| *c = to_c.clone());
+                    if k.ref_table == tn && k.ref_schema.as_deref().is_none_or(|s| s == schema) {
+                        k.ref_columns.iter_mut().filter(|c| **c == from_c).for_each(|c| *c = to_c.clone());
+                    }
+                }
                 let cols = m.renames(schema).columns.entry(tname.clone()).or_default();
                 record_rename(cols, from, to);
             }
@@ -561,21 +571,26 @@ impl ModelStore {
             }
             let (old, mut mt, _) = m.read(schema, from)?;
             mt.table.name = to.to_string();
-            // 自引用的外键
+            // 自引用的外键和逻辑外键
             for f in &mut mt.table.foreign_keys {
                 if f.ref_table == from && f.ref_schema.as_deref().is_none_or(|s| s == schema) {
                     f.ref_table = to.to_string();
                 }
             }
+            for k in &mut mt.logical_keys {
+                if k.ref_table == from && k.ref_schema.as_deref().is_none_or(|s| s == schema) {
+                    k.ref_table = to.to_string();
+                }
+            }
             let new = table_path(&m.root, schema, to);
             m.write_table(schema, &new, &mt)?;
             if new != old {
                 std::fs::remove_file(&old).map_err(|e| io_err(&old, e))?;
             }
-            m.update_refs(schema, Some(&new), |f| {
-                let hit = f.ref_table == from;
+            m.update_refs(schema, Some(&new), |ref_table, _| {
+                let hit = ref_table == from;
                 if hit {
-                    f.ref_table = to.to_string();
+                    *ref_table = to.to_string();
                 }
                 hit
             })?;
@@ -590,6 +605,78 @@ impl ModelStore {
         })
     }
 
+    /// 加一个逻辑外键：两边的列存在、列数一致、名字在本表中不重复。返回表文件的新指纹。
+    pub fn add_logical_key(
+        &self,
+        id: &str,
+        schema: &str,
+        table: &str,
+        key: LogicalKey,
+        expected: Option<&str>,
+    ) -> Result<String> {
+        self.with(id, |m| {
+            m.writable()?;
+            m.refresh()?;
+            m.check_fingerprint(schema, table, expected)?;
+            let (path, mut mt, _) = m.read(schema, table)?;
+            let missing = |name: &str| Error::invalid(msg!("model.lk_column_missing", name = name));
+            if key.columns.is_empty() || key.columns.len() != key.ref_columns.len() {
+                return Err(Error::invalid(msg!("model.lk_count_mismatch")));
+            }
+            for c in &key.columns {
+                if !mt.table.columns.iter().any(|x| &x.name == c) {
+                    return Err(missing(&format!("{table}.{c}")));
+                }
+            }
+            let ref_schema = key.ref_schema.clone().unwrap_or_else(|| schema.to_string());
+            let target = if ref_schema == schema && key.ref_table == table {
+                mt.table.clone()
+            } else {
+                m.read(&ref_schema, &key.ref_table)?.1.table
+            };
+            for c in &key.ref_columns {
+                if !target.columns.iter().any(|x| &x.name == c) {
+                    return Err(missing(&format!("{}.{c}", key.ref_table)));
+                }
+            }
+            if let Some(n) = &key.name {
+                let taken = mt.logical_keys.iter().any(|k| k.name.as_ref() == Some(n))
+                    || mt.table.foreign_keys.iter().any(|k| k.name.as_ref() == Some(n));
+                if taken {
+                    return Err(Error::invalid(msg!("model.lk_name_exists", name = n)));
+                }
+            }
+            mt.logical_keys.push(key);
+            let fp = m.write_table(schema, &path, &mt)?;
+            m.refresh()?;
+            Ok(fp)
+        })
+    }
+
+    /// 删除第 `index` 个逻辑外键（从 0 开始）。返回表文件的新指纹。
+    pub fn delete_logical_key(
+        &self,
+        id: &str,
+        schema: &str,
+        table: &str,
+        index: usize,
+        expected: Option<&str>,
+    ) -> Result<String> {
+        self.with(id, |m| {
+            m.writable()?;
+            m.refresh()?;
+            m.check_fingerprint(schema, table, expected)?;
+            let (path, mut mt, _) = m.read(schema, table)?;
+            if index >= mt.logical_keys.len() {
+                return Err(Error::not_found(msg!("model.lk_not_found", index = index)));
+            }
+            mt.logical_keys.remove(index);
+            let fp = m.write_table(schema, &path, &mt)?;
+            m.refresh()?;
+            Ok(fp)
+        })
+    }
+
     /// 删除表文件，去掉它的改名记录。
     pub fn delete_table(&self, id: &str, schema: &str, name: &str, expected: Option<&str>) -> Result<()> {
         self.with(id, |m| {
@@ -647,11 +734,7 @@ impl ModelStore {
                             }
                             None => (
                                 None,
-                                ModelTable {
-                                    table: Table::default(),
-                                    extra: BTreeMap::new(),
-                                    column_extra: BTreeMap::new(),
-                                },
+                                ModelTable::new(Table::default()),
                             ),
                         };
                         let new = match &old {
@@ -892,6 +975,61 @@ mod tests {
         assert!(std::fs::read_to_string(&path).unwrap().contains("layout:"));
     }
 
+    fn lk(columns: &str, ref_table: &str, ref_columns: &str) -> LogicalKey {
+        LogicalKey {
+            name: Some(format!("lk_{columns}")),
+            columns: vec![columns.into()],
+            ref_schema: None,
+            ref_table: ref_table.into(),
+            ref_columns: vec![ref_columns.into()],
+            comment: None,
+        }
+    }
+
+    /// 逻辑外键：校验、设计器保存后保留、表和列改名时引用跟着改、删除。
+    #[test]
+    fn logical_keys_follow_renames() {
+        let dir = tempfile::tempdir().unwrap();
+        let root = dir.path().join("m");
+        let (store, id) = sample(&root);
+        let bad = [
+            lk("nope", "customers", "id"),
+            lk("cust_id", "customers", "nope"),
+            LogicalKey { ref_columns: vec![], ..lk("cust_id", "customers", "id") },
+        ];
+        for k in bad {
+            assert!(store.add_logical_key(&id, "public", "orders", k, None).is_err());
+        }
+        let fp = store.add_logical_key(&id, "public", "orders", lk("cust_id", "customers", "id"), None).unwrap();
+        let err = store.add_logical_key(&id, "public", "orders", lk("cust_id", "customers", "id"), None).unwrap_err();
+        assert!(err.to_string().contains("already used"), "{err}");
+        // 设计器保存（草稿中没有逻辑外键）后还在
+        let t = store.read_table(&id, "public", "orders").unwrap().table;
+        store.save_table(&id, "public", Some("orders"), t, &BTreeMap::new(), Some(&fp)).unwrap();
+        let text = std::fs::read_to_string(root.join("tables/public/orders.yaml")).unwrap();
+        assert!(text.contains("logical_keys:") && text.contains("ref_table: customers"), "{text}");
+        // 被引用的表、列改名
+        store.rename_table(&id, "public", "customers", "clients").unwrap();
+        let mut c = store.read_table(&id, "public", "clients").unwrap().table;
+        c.columns[0].name = "client_id".into();
+        c.primary_key = Some(PrimaryKey { name: None, columns: vec!["client_id".into()] });
+        store.save_table(&id, "public", Some("clients"), c, &[("client_id".into(), "id".into())].into(), None).unwrap();
+        // 本表的列改名
+        let mut o = store.read_table(&id, "public", "orders").unwrap().table;
+        o.columns[1].name = "client_ref".into();
+        o.foreign_keys[0].columns = vec!["client_ref".into()];
+        store.save_table(&id, "public", Some("orders"), o, &[("client_ref".into(), "cust_id".into())].into(), None).unwrap();
+        let text = std::fs::read_to_string(root.join("tables/public/orders.yaml")).unwrap();
+        let mt = super::super::file::parse_table(&text, &Dialect::new(Variant::Postgres)).unwrap();
+        assert_eq!(mt.logical_keys[0].columns, ["client_ref"]);
+        assert_eq!((mt.logical_keys[0].ref_table.as_str(), mt.logical_keys[0].ref_columns.as_slice()), ("clients", &["client_id".to_string()][..]));
+        // 比对用的表结构中没有逻辑外键
+        assert!(store.schema_tables(&id, "public").unwrap().tables.iter().all(|(t, _)| t.foreign_keys.len() <= 1));
+        store.delete_logical_key(&id, "public", "orders", 0, None).unwrap();
+        assert!(store.delete_logical_key(&id, "public", "orders", 0, None).is_err());
+        assert!(!std::fs::read_to_string(root.join("tables/public/orders.yaml")).unwrap().contains("logical_keys"));
+    }
+
     #[test]
     fn write_back_checks_fingerprints() {
         let dir = tempfile::tempdir().unwrap();
```

- [ ] **Step 7: 检查**

```bash
export CARGO_INCREMENTAL=0
cargo fmt --all
cargo clippy --config "source.mirror.registry='sparse+https://rsproxy.cn/index/'" --workspace --all-targets -- -D warnings
DBLENS_LANG=en cargo test --config "source.mirror.registry='sparse+https://rsproxy.cn/index/'" --workspace
```

Expected: 全部测试通过（service 新增 1 个，表文件的快照多一段 `logical_keys`）。

- [ ] **Step 8: 提交**

```bash
git add crates/dblens-core/locales/app.json crates/dblens-service/src/lib.rs crates/dblens-service/src/model/file.rs crates/dblens-service/src/model/mod.rs crates/dblens-service/src/model/snapshots/dblens_service__model__file__tests__table_yaml.snap crates/dblens-service/src/model/store.rs
git commit -m "feat(service): 模型中的逻辑外键" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 3: 服务层：模型中的 ER 图文件

- `model/diagram.rs`（新）：`DEFAULT_DIAGRAM = "_all"`；`DiagramTables::{All, List}`（序列化为 `all` 或表名数组）、`DiagramDisplay::{Columns, Keys, Names}`、`Position { x: i64, y: i64 }`、`Diagram { name, tables, display, positions, extra }`；`DiagramSummary`、`DiagramTable { table, logical_keys, error }`、`DiagramWarning::{BrokenLogicalKey, MissingTable}`、`DiagramView { diagram, tables, fingerprint, warnings }`。
- `ModelStore` 的方法：`diagrams`（默认图在前，没有文件时也列出）、`read_diagram`（默认图没有文件时按全部表，`fingerprint` 为 None；文件有错的表只有名字；缺的表、引用失效的逻辑外键进提示）、`save_diagram`（指纹与 `expected` 不同时拒绝；默认图的 `tables` 固定为 `all`；文件中手写的字段保留）、`create_diagram`、`rename_diagram`、`delete_diagram`（默认图不能改名、删除）。
- `store.rs`：`OpenModel` 和要用到的方法改为 `pub(super)`；表改名、删除时调用 `diagram::on_table_changed` 更新各图的表名和位置。
- 后端词条加 `model.diagram_bad_name`、`model.diagram_exists`。

快照文件（`snapshots/`）随本任务给出；改了生成逻辑时用 `INSTA_UPDATE=always` 重新生成并逐个审查差异。

**Files:**
- Modify: `crates/dblens-core/locales/app.json`
- Modify: `crates/dblens-service/src/lib.rs`
- Create: `crates/dblens-service/src/model/diagram.rs`
- Modify: `crates/dblens-service/src/model/mod.rs`
- Create: `crates/dblens-service/src/model/snapshots/dblens_service__model__diagram__tests__diagram_yaml.snap`
- Modify: `crates/dblens-service/src/model/store.rs`

**Interfaces:**
- Consumes：任务 2 的 `LogicalKey`、`ModelTable`。
- Produces（`dblens_service`）：上面的类型；`ModelStore::{diagrams(id, schema) -> Result<Vec<DiagramSummary>>, read_diagram(id, schema, name) -> Result<DiagramView>, save_diagram(id, schema, name, diagram, expected) -> Result<String>, create_diagram(id, schema, name, tables) -> Result<String>, rename_diagram(id, schema, from, to) -> Result<()>, delete_diagram(id, schema, name) -> Result<()>}`。

- [ ] **Step 1: 修改 `crates/dblens-core/locales/app.json`**

```diff
--- a/crates/dblens-core/locales/app.json
+++ b/crates/dblens-core/locales/app.json
@@ -146,6 +146,8 @@
   "model.bad_file": {"zh-CN": "文件 %{path} 有错：%{error}", "en": "File %{path} is invalid: %{error}"},
   "model.bad_path": {"zh-CN": "路径无效：%{path}", "en": "Invalid path: %{path}"},
   "model.conflict": {"zh-CN": "%{name} 的文件已被修改，请重新加载", "en": "The file for %{name} has changed; reload it"},
+  "model.diagram_bad_name": {"zh-CN": "图名 %{name} 不可用（不能为空，也不能是 _all）", "en": "The diagram name %{name} is not allowed (it must not be empty or _all)"},
+  "model.diagram_exists": {"zh-CN": "图 %{name} 已存在", "en": "Diagram %{name} already exists"},
   "model.dir_not_empty": {"zh-CN": "目录 %{path} 不为空", "en": "Directory %{path} is not empty"},
   "model.io_failed": {"zh-CN": "读写 %{path} 失败：%{error}", "en": "Failed to access %{path}: %{error}"},
   "model.lk_column_missing": {"zh-CN": "列 %{name} 不存在", "en": "Column %{name} does not exist"},
```

- [ ] **Step 2: 修改 `crates/dblens-service/src/lib.rs`**

```diff
--- a/crates/dblens-service/src/lib.rs
+++ b/crates/dblens-service/src/lib.rs
@@ -44,9 +44,10 @@ pub use import::{
 };
 pub use metadata::{METADATA_POOL_SIZE, MetadataCache, MetadataService};
 pub use model::{
-    FileError, LogicalKey, ModelDatabase, ModelDesignPreview, ModelDesignRequest, ModelInfo, ModelStore, ModelSummary,
-    ModelTableView, ModelWrite, NewModel, RECENT_MODELS_SETTING, ReverseRequest, SchemaRenames, SchemaTables,
-    TableEntry,
+    DEFAULT_DIAGRAM, Diagram, DiagramDisplay, DiagramSummary, DiagramTable, DiagramTables, DiagramView,
+    DiagramWarning, FileError, LogicalKey, ModelDatabase, ModelDesignPreview, ModelDesignRequest, ModelInfo,
+    ModelStore, ModelSummary, ModelTableView, ModelWrite, NewModel, Position, RECENT_MODELS_SETTING,
+    ReverseRequest, SchemaRenames, SchemaTables, TableEntry,
 };
 pub use objects::{DataOrder, GenerateKind, ObjectKind};
 pub use policy::{
```

- [ ] **Step 3: 新建 `crates/dblens-service/src/model/diagram.rs`**

```rust
//! ER 图（M4b 设计文档第 3、5 节）：`diagrams/<schema>/<图名>.yaml`。每个 schema 都有默认图 `_all`（全部表），
//! 没有文件时按全部表、自动布局打开；主题图只放选中的表。表改名、删除时各图跟着改。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use dblens_core::schema::Table;
use dblens_core::{Error, Result, msg};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::file::{LogicalKey, escape_name, fingerprint, io_err, unescape_name, write_atomic};
use super::store::{FileError, ModelStore, OpenModel};

/// 默认图的名字（文件 `_all.yaml`）。
pub const DEFAULT_DIAGRAM: &str = "_all";

type Extra = BTreeMap<String, serde_json::Value>;

/// 图中的表：全部（写成 `all`）或列出的表。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiagramTables {
    All,
    List(Vec<String>),
}

impl Serialize for DiagramTables {
    fn serialize<S: Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        match self {
            DiagramTables::All => s.serialize_str("all"),
            DiagramTables::List(l) => l.serialize(s),
        }
    }
}

impl<'de> Deserialize<'de> for DiagramTables {
    fn deserialize<D: Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Raw {
            Word(String),
            List(Vec<String>),
        }
        match Raw::deserialize(d)? {
            Raw::Word(w) if w == "all" => Ok(DiagramTables::All),
            Raw::Word(w) => {
                Err(serde::de::Error::custom(format!("expected `all` or a list of tables, got `{w}`")))
            }
            Raw::List(l) => Ok(DiagramTables::List(l)),
        }
    }
}

impl DiagramTables {
    fn contains(&self, name: &str) -> bool {
        match self {
            DiagramTables::All => true,
            DiagramTables::List(l) => l.iter().any(|t| t == name),
        }
    }
}

/// 表节点的显示方式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagramDisplay {
    #[default]
    Columns,
    Keys,
    Names,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Position {
    pub x: i64,
    pub y: i64,
}

/// 图文件。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Diagram {
    pub name: String,
    pub tables: DiagramTables,
    #[serde(default)]
    pub display: DiagramDisplay,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub positions: BTreeMap<String, Position>,
    #[serde(flatten)]
    pub extra: Extra,
}

impl Diagram {
    fn default_for() -> Self {
        Diagram {
            name: DEFAULT_DIAGRAM.into(),
            tables: DiagramTables::All,
            display: DiagramDisplay::Columns,
            positions: BTreeMap::new(),
            extra: Extra::new(),
        }
    }
}

/// 图列表中的一项。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DiagramSummary {
    pub schema: String,
    pub name: String,
    pub tables: DiagramTables,
}

/// 图中的一张表。文件有错时 `table` 只有名字。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DiagramTable {
    pub table: Table,
    pub logical_keys: Vec<LogicalKey>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<FileError>,
}

/// 图的提示：引用不存在的逻辑外键、图中已不存在的表。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DiagramWarning {
    BrokenLogicalKey { table: String, index: usize, name: Option<String> },
    MissingTable { name: String },
}

/// 读出的图：图（默认图没有文件时为默认值）、表、指纹（没有文件时为 None）、提示。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DiagramView {
    pub diagram: Diagram,
    pub tables: Vec<DiagramTable>,
    pub fingerprint: Option<String>,
    pub warnings: Vec<DiagramWarning>,
}

fn diagram_dir(root: &Path, schema: &str) -> PathBuf {
    root.join("diagrams").join(escape_name(schema))
}

fn diagram_path(root: &Path, schema: &str, name: &str) -> PathBuf {
    diagram_dir(root, schema).join(format!("{}.yaml", escape_name(name)))
}

fn yaml_options() -> serde_saphyr::SerializerOptions {
    let mut o = serde_saphyr::SerializerOptions::default();
    o.prefer_block_scalars = true;
    o
}

pub fn diagram_yaml(d: &Diagram) -> Result<String> {
    serde_saphyr::to_string_with_options(d, yaml_options())
        .map_err(|e| Error::internal(msg!("model.yaml_write_failed", error = e)))
}

pub fn parse_diagram(text: &str) -> std::result::Result<Diagram, String> {
    serde_saphyr::from_str::<Diagram>(text).map_err(|e| e.to_string())
}

/// 读图文件：（图，指纹）；没有文件时为 None。
fn load(root: &Path, schema: &str, name: &str) -> Result<Option<(Diagram, String)>> {
    let path = diagram_path(root, schema, name);
    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(io_err(&path, e)),
    };
    let text = String::from_utf8_lossy(&bytes);
    let d = parse_diagram(&text).map_err(|e| {
        Error::invalid(msg!("model.bad_file", path = format!("diagrams/{schema}/{name}.yaml"), error = e))
    })?;
    Ok(Some((d, fingerprint(&bytes))))
}

fn not_found(schema: &str, name: &str) -> Error {
    Error::not_found(msg!("noun.object", kind = "diagram", name = format!("{schema}.{name}")))
}

/// 一个 schema 的图文件名（不含默认图）。
fn topic_names(root: &Path, schema: &str) -> Result<Vec<String>> {
    let dir = diagram_dir(root, schema);
    let entries = match std::fs::read_dir(&dir) {
        Ok(it) => it,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Err(e) => return Err(io_err(&dir, e)),
    };
    let mut out = Vec::new();
    for e in entries {
        let p = e.map_err(|e| io_err(&dir, e))?.path();
        if p.extension().is_some_and(|x| x == "yaml")
            && let Some(stem) = p.file_stem()
        {
            let name = unescape_name(&stem.to_string_lossy());
            if name != DEFAULT_DIAGRAM {
                out.push(name);
            }
        }
    }
    out.sort();
    Ok(out)
}

/// 表改名（`to` 为 Some）或删除（None）时更新一个 schema 的各图。
pub(super) fn on_table_changed(m: &OpenModel, schema: &str, from: &str, to: Option<&str>) -> Result<()> {
    let mut names = topic_names(&m.root, schema)?;
    names.push(DEFAULT_DIAGRAM.into());
    for name in names {
        let Some((mut d, _)) = load(&m.root, schema, &name)? else { continue };
        let mut changed = false;
        if let DiagramTables::List(l) = &mut d.tables
            && let Some(i) = l.iter().position(|t| t == from)
        {
            match to {
                Some(t) => l[i] = t.to_string(),
                None => {
                    l.remove(i);
                }
            }
            changed = true;
        }
        if let Some(p) = d.positions.remove(from) {
            if let Some(t) = to {
                d.positions.insert(t.to_string(), p);
            }
            changed = true;
        }
        if changed {
            write_atomic(&diagram_path(&m.root, schema, &name), &diagram_yaml(&d)?)?;
        }
    }
    Ok(())
}

fn check_name(name: &str) -> Result<()> {
    if name.trim().is_empty() || name == DEFAULT_DIAGRAM {
        return Err(Error::invalid(msg!("model.diagram_bad_name", name = name)));
    }
    Ok(())
}

impl ModelStore {
    /// 一个 schema 的图：默认图在前（没有文件时也列出），其余按名字。
    pub fn diagrams(&self, id: &str, schema: &str) -> Result<Vec<DiagramSummary>> {
        self.with(id, |m| {
            let mut out = Vec::new();
            let default =
                load(&m.root, schema, DEFAULT_DIAGRAM)?.map_or(DiagramTables::All, |(d, _)| d.tables);
            out.push(DiagramSummary { schema: schema.into(), name: DEFAULT_DIAGRAM.into(), tables: default });
            for name in topic_names(&m.root, schema)? {
                let tables =
                    load(&m.root, schema, &name)?.map_or(DiagramTables::List(vec![]), |(d, _)| d.tables);
                out.push(DiagramSummary { schema: schema.into(), name, tables });
            }
            Ok(out)
        })
    }

    /// 读图：图中的表（文件有错的只有名字）和它们的逻辑外键，以及提示。
    pub fn read_diagram(&self, id: &str, schema: &str, name: &str) -> Result<DiagramView> {
        self.with(id, |m| {
            m.refresh()?;
            let (diagram, fp) = match load(&m.root, schema, name)? {
                Some((d, fp)) => (d, Some(fp)),
                None if name == DEFAULT_DIAGRAM => (Diagram::default_for(), None),
                None => return Err(not_found(schema, name)),
            };
            let entries: Vec<_> = m.entries().into_iter().filter(|e| e.schema == schema).collect();
            let mut tables = Vec::new();
            let mut warnings = Vec::new();
            for e in &entries {
                if !diagram.tables.contains(&e.name) {
                    continue;
                }
                match &e.error {
                    Some(err) => tables.push(DiagramTable {
                        table: Table { name: e.name.clone(), ..Default::default() },
                        logical_keys: vec![],
                        error: Some(err.clone()),
                    }),
                    None => {
                        let (_, mt, _) = m.read(schema, &e.name)?;
                        tables.push(DiagramTable {
                            table: mt.table,
                            logical_keys: mt.logical_keys,
                            error: None,
                        });
                    }
                }
            }
            if let DiagramTables::List(l) = &diagram.tables {
                for t in l.iter().filter(|t| !entries.iter().any(|e| &e.name == *t)) {
                    warnings.push(DiagramWarning::MissingTable { name: t.clone() });
                }
            }
            // 逻辑外键：本表的列、引用的表和列都要在
            for t in &tables {
                for (index, k) in t.logical_keys.iter().enumerate() {
                    let has = |tab: &Table, cols: &[String]| {
                        cols.iter().all(|c| tab.columns.iter().any(|x| &x.name == c))
                    };
                    let ref_schema = k.ref_schema.clone().unwrap_or_else(|| schema.to_string());
                    let target = m.read(&ref_schema, &k.ref_table).ok().map(|(_, mt, _)| mt.table);
                    let ok = has(&t.table, &k.columns) && target.is_some_and(|x| has(&x, &k.ref_columns));
                    if !ok {
                        warnings.push(DiagramWarning::BrokenLogicalKey {
                            table: t.table.name.clone(),
                            index,
                            name: k.name.clone(),
                        });
                    }
                }
            }
            Ok(DiagramView { diagram, tables, fingerprint: fp, warnings })
        })
    }

    /// 保存图（默认图第一次保存时建文件）：文件的指纹与 `expected` 不同时拒绝。返回新的指纹。
    pub fn save_diagram(
        &self,
        id: &str,
        schema: &str,
        name: &str,
        mut diagram: Diagram,
        expected: Option<&str>,
    ) -> Result<String> {
        self.with(id, |m| {
            m.writable()?;
            let current = load(&m.root, schema, name)?;
            if current.is_none() && name != DEFAULT_DIAGRAM {
                return Err(not_found(schema, name));
            }
            if current.as_ref().map(|(_, fp)| fp.as_str()) != expected {
                return Err(Error::invalid(msg!("model.conflict", name = format!("{schema}/{name}"))));
            }
            diagram.name = name.to_string();
            if name == DEFAULT_DIAGRAM {
                diagram.tables = DiagramTables::All;
            }
            // 文件中不认识的字段以文件为准保留
            if let Some((old, _)) = current {
                for (k, v) in old.extra {
                    diagram.extra.entry(k).or_insert(v);
                }
            }
            let text = diagram_yaml(&diagram)?;
            write_atomic(&diagram_path(&m.root, schema, name), &text)?;
            Ok(fingerprint(text.as_bytes()))
        })
    }

    /// 新建主题图。
    pub fn create_diagram(&self, id: &str, schema: &str, name: &str, tables: Vec<String>) -> Result<String> {
        self.with(id, |m| {
            m.writable()?;
            check_name(name)?;
            let path = diagram_path(&m.root, schema, name);
            if path.exists() {
                return Err(Error::invalid(msg!("model.diagram_exists", name = name)));
            }
            let d =
                Diagram { name: name.into(), tables: DiagramTables::List(tables), ..Diagram::default_for() };
            let text = diagram_yaml(&d)?;
            write_atomic(&path, &text)?;
            Ok(fingerprint(text.as_bytes()))
        })
    }

    pub fn rename_diagram(&self, id: &str, schema: &str, from: &str, to: &str) -> Result<()> {
        self.with(id, |m| {
            m.writable()?;
            check_name(from)?;
            check_name(to)?;
            let (mut d, _) = load(&m.root, schema, from)?.ok_or_else(|| not_found(schema, from))?;
            let target = diagram_path(&m.root, schema, to);
            if target.exists() {
                return Err(Error::invalid(msg!("model.diagram_exists", name = to)));
            }
            d.name = to.into();
            write_atomic(&target, &diagram_yaml(&d)?)?;
            let old = diagram_path(&m.root, schema, from);
            std::fs::remove_file(&old).map_err(|e| io_err(&old, e))
        })
    }

    pub fn delete_diagram(&self, id: &str, schema: &str, name: &str) -> Result<()> {
        self.with(id, |m| {
            m.writable()?;
            check_name(name)?;
            let path = diagram_path(&m.root, schema, name);
            if !path.exists() {
                return Err(not_found(schema, name));
            }
            std::fs::remove_file(&path).map_err(|e| io_err(&path, e))
        })
    }
}

#[cfg(test)]
mod tests {
    use dblens_core::schema::{Column, ForeignKey, PrimaryKey};
    use dblens_core::{ColumnType, LogicalType};
    use dblens_dialect::{Compat, Variant};

    use super::super::file::ModelDatabase;
    use super::*;

    fn table(name: &str, cols: &[&str]) -> Table {
        Table {
            name: name.into(),
            columns: cols
                .iter()
                .map(|c| Column {
                    name: (*c).into(),
                    ordinal: 0,
                    data_type: ColumnType { native: "integer".into(), logical: LogicalType::Other },
                    nullable: false,
                    default: None,
                    comment: None,
                    auto_increment: false,
                    generated: None,
                })
                .collect(),
            primary_key: Some(PrimaryKey { name: None, columns: vec![cols[0].into()] }),
            ..Default::default()
        }
    }

    fn sample(root: &Path) -> (ModelStore, String) {
        let store = ModelStore::default();
        let db = ModelDatabase { variant: Variant::Postgres, compat: Compat::Native, version: None };
        let mut orders = table("orders", &["id", "cust_id"]);
        orders.foreign_keys.push(ForeignKey {
            name: Some("fk_cust".into()),
            columns: vec!["cust_id".into()],
            ref_schema: None,
            ref_table: "customers".into(),
            ref_columns: vec!["id".into()],
            on_delete: None,
            on_update: None,
        });
        let tables = vec![table("customers", &["id", "code"]), orders, table("items", &["id"])];
        let id = store.create_from(root, "m", db, &[("public".into(), tables)]).unwrap().id;
        (store, id)
    }

    fn pos(x: i64, y: i64) -> Position {
        Position { x, y }
    }

    #[test]
    fn default_diagram_without_file_then_saved() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("m");
        let (store, id) = sample(&root);
        let list = store.diagrams(&id, "public").unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!((list[0].name.as_str(), &list[0].tables), (DEFAULT_DIAGRAM, &DiagramTables::All));
        let v = store.read_diagram(&id, "public", DEFAULT_DIAGRAM).unwrap();
        assert_eq!(v.fingerprint, None);
        let names: Vec<_> = v.tables.iter().map(|t| t.table.name.as_str()).collect();
        assert_eq!(names, ["customers", "items", "orders"]);
        let mut d = v.diagram;
        d.positions.insert("orders".into(), pos(120, 40));
        d.display = DiagramDisplay::Keys;
        let fp = store.save_diagram(&id, "public", DEFAULT_DIAGRAM, d.clone(), None).unwrap();
        let text = std::fs::read_to_string(root.join("diagrams/public/_all.yaml")).unwrap();
        insta::assert_snapshot!("diagram_yaml", text);
        // 用旧的指纹（没有文件时的 None）再保存被拒绝；文件中手写的字段保留
        assert!(store.save_diagram(&id, "public", DEFAULT_DIAGRAM, d.clone(), None).is_err());
        std::fs::write(root.join("diagrams/public/_all.yaml"), format!("{text}theme: dark\n")).unwrap();
        let v = store.read_diagram(&id, "public", DEFAULT_DIAGRAM).unwrap();
        assert_ne!(v.fingerprint.as_deref(), Some(fp.as_str()));
        store.save_diagram(&id, "public", DEFAULT_DIAGRAM, d, v.fingerprint.as_deref()).unwrap();
        assert!(
            std::fs::read_to_string(root.join("diagrams/public/_all.yaml")).unwrap().contains("theme: dark")
        );
    }

    #[test]
    fn topic_diagrams_follow_table_renames_and_deletes() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("m");
        let (store, id) = sample(&root);
        store.create_diagram(&id, "public", "订单/明细", vec!["orders".into(), "customers".into()]).unwrap();
        assert!(root.join("diagrams/public/订单%2F明细.yaml").exists());
        assert!(store.create_diagram(&id, "public", "订单/明细", vec![]).is_err());
        assert!(store.create_diagram(&id, "public", DEFAULT_DIAGRAM, vec![]).is_err());
        let v = store.read_diagram(&id, "public", "订单/明细").unwrap();
        assert_eq!(v.tables.len(), 2);
        let mut d = v.diagram;
        d.positions.insert("customers".into(), pos(1, 2));
        store.save_diagram(&id, "public", "订单/明细", d, v.fingerprint.as_deref()).unwrap();
        // 表改名、删除
        store.rename_table(&id, "public", "customers", "clients").unwrap();
        let v = store.read_diagram(&id, "public", "订单/明细").unwrap();
        assert_eq!(v.diagram.tables, DiagramTables::List(vec!["orders".into(), "clients".into()]));
        assert_eq!(v.diagram.positions["clients"], pos(1, 2));
        store.delete_table(&id, "public", "orders", None).unwrap();
        let v = store.read_diagram(&id, "public", "订单/明细").unwrap();
        assert_eq!(v.diagram.tables, DiagramTables::List(vec!["clients".into()]));
        // 改名、删除主题图；默认图不能改名、删除
        store.rename_diagram(&id, "public", "订单/明细", "客户").unwrap();
        let names: Vec<_> = store.diagrams(&id, "public").unwrap().into_iter().map(|d| d.name).collect();
        assert_eq!(names, [DEFAULT_DIAGRAM, "客户"]);
        assert!(store.rename_diagram(&id, "public", DEFAULT_DIAGRAM, "x").is_err());
        assert!(store.delete_diagram(&id, "public", DEFAULT_DIAGRAM).is_err());
        store.delete_diagram(&id, "public", "客户").unwrap();
        assert_eq!(store.diagrams(&id, "public").unwrap().len(), 1);
    }

    #[test]
    fn warnings_for_missing_tables_and_broken_logical_keys() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("m");
        let (store, id) = sample(&root);
        let key = LogicalKey {
            name: Some("lk_code".into()),
            columns: vec!["cust_id".into()],
            ref_schema: None,
            ref_table: "customers".into(),
            ref_columns: vec!["code".into()],
            comment: None,
        };
        store.add_logical_key(&id, "public", "orders", key, None).unwrap();
        store.create_diagram(&id, "public", "t", vec!["orders".into(), "gone".into()]).unwrap();
        let v = store.read_diagram(&id, "public", "t").unwrap();
        assert_eq!(v.tables[0].logical_keys.len(), 1);
        assert_eq!(v.warnings, [DiagramWarning::MissingTable { name: "gone".into() }]);
        // 被引用的列删掉后逻辑外键保留，列入提示
        let mut c = store.read_table(&id, "public", "customers").unwrap().table;
        c.columns.pop();
        store.save_table(&id, "public", Some("customers"), c, &BTreeMap::new(), None).unwrap();
        let v = store.read_diagram(&id, "public", DEFAULT_DIAGRAM).unwrap();
        let broken = DiagramWarning::BrokenLogicalKey {
            table: "orders".into(),
            index: 0,
            name: Some("lk_code".into()),
        };
        assert_eq!(v.warnings, [broken]);
        // 文件有错的表只有名字
        std::fs::write(root.join("tables/public/bad.yaml"), "name: [").unwrap();
        let v = store.read_diagram(&id, "public", DEFAULT_DIAGRAM).unwrap();
        let bad = v.tables.iter().find(|t| t.table.name == "bad").unwrap();
        assert!(bad.error.is_some() && bad.table.columns.is_empty());
    }
}
```

- [ ] **Step 4: 修改 `crates/dblens-service/src/model/mod.rs`**

```diff
--- a/crates/dblens-service/src/model/mod.rs
+++ b/crates/dblens-service/src/model/mod.rs
@@ -1,9 +1,14 @@
 //! 模型（M4a）：模型目录的读写、打开的模型。
 
+mod diagram;
 mod file;
 mod ops;
 mod store;
 
+pub use diagram::{
+    DEFAULT_DIAGRAM, Diagram, DiagramDisplay, DiagramSummary, DiagramTable, DiagramTables, DiagramView,
+    DiagramWarning, Position,
+};
 pub use file::{LogicalKey, ModelDatabase, ModelInfo, SchemaRenames};
 pub use ops::{ModelDesignPreview, ModelDesignRequest, NewModel, RECENT_MODELS_SETTING, ReverseRequest};
 pub use store::{FileError, ModelStore, ModelSummary, ModelTableView, ModelWrite, SchemaTables, TableEntry};
```

- [ ] **Step 5: 新建 `crates/dblens-service/src/model/snapshots/dblens_service__model__diagram__tests__diagram_yaml.snap`**

```text
---
source: crates/dblens-service/src/model/diagram.rs
expression: text
---
name: _all
tables: all
display: keys
positions:
  orders:
    x: 120
    "y": 40
```

- [ ] **Step 6: 修改 `crates/dblens-service/src/model/store.rs`**

```diff
--- a/crates/dblens-service/src/model/store.rs
+++ b/crates/dblens-service/src/model/store.rs
@@ -11,9 +11,10 @@ use dblens_core::{Error, Result, msg};
 use dblens_dialect::Dialect;
 use serde::Serialize;
 
+use super::diagram;
 use super::file::{
-    FORMAT, LogicalKey, ModelDatabase, ModelInfo, ModelTable, SchemaRenames, fingerprint, info_path, info_yaml, io_err,
-    parse_info, parse_table, table_path, table_yaml, unescape_name, write_atomic,
+    FORMAT, LogicalKey, ModelDatabase, ModelInfo, ModelTable, SchemaRenames, fingerprint, info_path,
+    info_yaml, io_err, parse_info, parse_table, table_path, table_yaml, unescape_name, write_atomic,
 };
 
 /// 打开的模型的概要。
@@ -60,8 +61,8 @@ struct Cached {
     parsed: std::result::Result<ModelTable, String>,
 }
 
-struct OpenModel {
-    root: PathBuf,
+pub(super) struct OpenModel {
+    pub(super) root: PathBuf,
     info: ModelInfo,
     read_only: bool,
     files: BTreeMap<PathBuf, Cached>,
@@ -141,7 +142,7 @@ impl OpenModel {
         self.info.database.dialect()
     }
 
-    fn writable(&self) -> Result<()> {
+    pub(super) fn writable(&self) -> Result<()> {
         if self.read_only {
             return Err(Error::invalid(msg!("model.read_only", format = self.info.format)));
         }
@@ -149,7 +150,7 @@ impl OpenModel {
     }
 
     /// 扫描 `tables/` 下的文件，修改时间或长度变了的重读，删掉的文件移出缓存。
-    fn refresh(&mut self) -> Result<()> {
+    pub(super) fn refresh(&mut self) -> Result<()> {
         let d = self.dialect();
         let dir = self.root.join("tables");
         let mut seen = Vec::new();
@@ -202,7 +203,7 @@ impl OpenModel {
             .map(|(p, _)| p.as_path())
     }
 
-    fn entries(&self) -> Vec<TableEntry> {
+    pub(super) fn entries(&self) -> Vec<TableEntry> {
         let mut out: Vec<TableEntry> = self
             .files
             .iter()
@@ -233,7 +234,7 @@ impl OpenModel {
         Ok((path, &self.files[path]))
     }
 
-    fn read(&self, schema: &str, name: &str) -> Result<(PathBuf, ModelTable, String)> {
+    pub(super) fn read(&self, schema: &str, name: &str) -> Result<(PathBuf, ModelTable, String)> {
         let (path, c) = self.cached(schema, name)?;
         match &c.parsed {
             Ok(t) => Ok((path.to_path_buf(), t.clone(), c.fingerprint.clone())),
@@ -317,7 +318,7 @@ impl OpenModel {
 }
 
 impl ModelStore {
-    fn with<T>(&self, id: &str, f: impl FnOnce(&mut OpenModel) -> Result<T>) -> Result<T> {
+    pub(super) fn with<T>(&self, id: &str, f: impl FnOnce(&mut OpenModel) -> Result<T>) -> Result<T> {
         let mut models = self.models.lock().unwrap_or_else(|e| e.into_inner());
         let m = models.get_mut(id).ok_or_else(|| not_open(id))?;
         f(m)
@@ -388,8 +389,7 @@ impl ModelStore {
         check_empty(&root)?;
         for (schema, list) in tables {
             for t in list {
-                let mt =
-                    ModelTable::new(t.clone());
+                let mt = ModelTable::new(t.clone());
                 write_atomic(&table_path(&root, schema, &t.name), &table_yaml(&mt)?)?;
             }
         }
@@ -594,6 +594,7 @@ impl ModelStore {
                 }
                 hit
             })?;
+            diagram::on_table_changed(m, schema, from, Some(to))?;
             let r = m.renames(schema);
             record_rename(&mut r.tables, from, to);
             if let Some(cols) = r.columns.remove(from) {
@@ -686,6 +687,7 @@ impl ModelStore {
             m.check_fingerprint(schema, name, expected)?;
             let path = m.find(schema, name).ok_or_else(|| table_missing(schema, name))?.to_path_buf();
             std::fs::remove_file(&path).map_err(|e| io_err(&path, e))?;
+            diagram::on_table_changed(m, schema, name, None)?;
             let r = m.renames(schema);
             r.tables.remove(name);
             r.columns.remove(name);
@@ -732,10 +734,7 @@ impl ModelStore {
                                 let (p, mt, _) = m.read(schema, n)?;
                                 (Some(p), mt)
                             }
-                            None => (
-                                None,
-                                ModelTable::new(Table::default()),
-                            ),
+                            None => (None, ModelTable::new(Table::default())),
                         };
                         let new = match &old {
                             Some(p) if path_of.as_deref() == Some(table.name.as_str()) => p.clone(),
@@ -1000,8 +999,11 @@ mod tests {
         for k in bad {
             assert!(store.add_logical_key(&id, "public", "orders", k, None).is_err());
         }
-        let fp = store.add_logical_key(&id, "public", "orders", lk("cust_id", "customers", "id"), None).unwrap();
-        let err = store.add_logical_key(&id, "public", "orders", lk("cust_id", "customers", "id"), None).unwrap_err();
+        let fp =
+            store.add_logical_key(&id, "public", "orders", lk("cust_id", "customers", "id"), None).unwrap();
+        let err = store
+            .add_logical_key(&id, "public", "orders", lk("cust_id", "customers", "id"), None)
+            .unwrap_err();
         assert!(err.to_string().contains("already used"), "{err}");
         // 设计器保存（草稿中没有逻辑外键）后还在
         let t = store.read_table(&id, "public", "orders").unwrap().table;
@@ -1013,21 +1015,41 @@ mod tests {
         let mut c = store.read_table(&id, "public", "clients").unwrap().table;
         c.columns[0].name = "client_id".into();
         c.primary_key = Some(PrimaryKey { name: None, columns: vec!["client_id".into()] });
-        store.save_table(&id, "public", Some("clients"), c, &[("client_id".into(), "id".into())].into(), None).unwrap();
+        store
+            .save_table(&id, "public", Some("clients"), c, &[("client_id".into(), "id".into())].into(), None)
+            .unwrap();
         // 本表的列改名
         let mut o = store.read_table(&id, "public", "orders").unwrap().table;
         o.columns[1].name = "client_ref".into();
         o.foreign_keys[0].columns = vec!["client_ref".into()];
-        store.save_table(&id, "public", Some("orders"), o, &[("client_ref".into(), "cust_id".into())].into(), None).unwrap();
+        store
+            .save_table(
+                &id,
+                "public",
+                Some("orders"),
+                o,
+                &[("client_ref".into(), "cust_id".into())].into(),
+                None,
+            )
+            .unwrap();
         let text = std::fs::read_to_string(root.join("tables/public/orders.yaml")).unwrap();
         let mt = super::super::file::parse_table(&text, &Dialect::new(Variant::Postgres)).unwrap();
         assert_eq!(mt.logical_keys[0].columns, ["client_ref"]);
-        assert_eq!((mt.logical_keys[0].ref_table.as_str(), mt.logical_keys[0].ref_columns.as_slice()), ("clients", &["client_id".to_string()][..]));
+        assert_eq!(
+            (mt.logical_keys[0].ref_table.as_str(), mt.logical_keys[0].ref_columns.as_slice()),
+            ("clients", &["client_id".to_string()][..])
+        );
         // 比对用的表结构中没有逻辑外键
-        assert!(store.schema_tables(&id, "public").unwrap().tables.iter().all(|(t, _)| t.foreign_keys.len() <= 1));
+        assert!(
+            store.schema_tables(&id, "public").unwrap().tables.iter().all(|(t, _)| t.foreign_keys.len() <= 1)
+        );
         store.delete_logical_key(&id, "public", "orders", 0, None).unwrap();
         assert!(store.delete_logical_key(&id, "public", "orders", 0, None).is_err());
-        assert!(!std::fs::read_to_string(root.join("tables/public/orders.yaml")).unwrap().contains("logical_keys"));
+        assert!(
+            !std::fs::read_to_string(root.join("tables/public/orders.yaml"))
+                .unwrap()
+                .contains("logical_keys")
+        );
     }
 
     #[test]
```

- [ ] **Step 7: 检查**

```bash
export CARGO_INCREMENTAL=0
cargo fmt --all
cargo clippy --config "source.mirror.registry='sparse+https://rsproxy.cn/index/'" --workspace --all-targets -- -D warnings
DBLENS_LANG=en cargo test --config "source.mirror.registry='sparse+https://rsproxy.cn/index/'" --workspace
```

Expected: 全部测试通过（service 新增 3 个，快照 1 个）。

- [ ] **Step 8: 提交**

```bash
git add crates/dblens-core/locales/app.json crates/dblens-service/src/lib.rs crates/dblens-service/src/model/diagram.rs crates/dblens-service/src/model/mod.rs crates/dblens-service/src/model/snapshots/dblens_service__model__diagram__tests__diagram_yaml.snap crates/dblens-service/src/model/store.rs
git commit -m "feat(service): 模型中的 ER 图文件" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 4: 接口：ER 图与逻辑外键，桌面程序导出图片

- `dblens-api/src/diagrams.rs`（新）：`/models/{id}/diagrams`（列表、新建）、`/models/{id}/diagrams/{schema}/{name}`（读、保存、删除）、`…/rename`、`/models/{id}/tables/{schema}/{name}/logical-keys`（加）、`…/logical-keys/{index}`（删）、`/files/image`（`{path, data}`，base64，只接受 `.svg`、`.png`）。都要求 `local_files`（`models::local_only` 改为 `pub(crate)`）。
- `dblens-api` 依赖 `base64`（工作区已有）。
- 后端词条加 `model.image_ext`、`model.image_data`。

**Files:**
- Modify: `crates/dblens-api/Cargo.toml`
- Create: `crates/dblens-api/src/diagrams.rs`
- Modify: `crates/dblens-api/src/lib.rs`
- Modify: `crates/dblens-api/src/models.rs`
- Modify: `crates/dblens-api/tests/models.rs`
- Modify: `crates/dblens-core/locales/app.json`

**Interfaces:**
- Consumes：任务 2、3 的服务层接口。

- [ ] **Step 1: 修改 `crates/dblens-api/tests/models.rs`**

```diff
--- a/crates/dblens-api/tests/models.rs
+++ b/crates/dblens-api/tests/models.rs
@@ -60,6 +60,102 @@ async fn models_need_local_files() {
     let (st, _) =
         call(&app, "POST", "/api/compare/script", Some(json!({ "path": "/tmp/x.sql", "text": "" }))).await;
     assert_eq!(st, StatusCode::FORBIDDEN);
+    let (st, _) = call(&app, "GET", "/api/models/x/diagrams?schema=s", None).await;
+    assert_eq!(st, StatusCode::FORBIDDEN);
+    let (st, _) =
+        call(&app, "POST", "/api/files/image", Some(json!({ "path": "/tmp/x.png", "data": "" }))).await;
+    assert_eq!(st, StatusCode::FORBIDDEN);
+}
+
+/// ER 图与逻辑外键（M4b）。
+#[tokio::test]
+async fn diagrams_and_logical_keys() {
+    let app = app(true);
+    let dir = tempfile::tempdir().unwrap();
+    let m = new_model(&app, dir.path(), "m").await;
+    let save = |name: Option<&str>, d: Value| json!({ "schema": "public", "name": name, "draft": d, "fingerprint": null });
+    for t in ["a", "b"] {
+        let (st, v) =
+            call(&app, "POST", &format!("/api/models/{m}/tables"), Some(save(None, draft(t, &["id", "x"]))))
+                .await;
+        assert_eq!(st, StatusCode::OK, "{v}");
+    }
+    // 默认图：没有文件时也能读，第一次保存建文件
+    let (_, list) = call(&app, "GET", &format!("/api/models/{m}/diagrams?schema=public"), None).await;
+    assert_eq!(list[0]["name"], "_all");
+    let (st, v) = call(&app, "GET", &format!("/api/models/{m}/diagrams/public/_all"), None).await;
+    assert_eq!((st, &v["fingerprint"]), (StatusCode::OK, &Value::Null), "{v}");
+    assert_eq!(v["tables"].as_array().unwrap().len(), 2);
+    let mut d = v["diagram"].clone();
+    d["positions"] = json!({ "a": { "x": 10, "y": 20 } });
+    let (st, r) = call(
+        &app,
+        "PUT",
+        &format!("/api/models/{m}/diagrams/public/_all"),
+        Some(json!({ "diagram": d, "fingerprint": null })),
+    )
+    .await;
+    assert_eq!(st, StatusCode::OK, "{r}");
+    let (st, _) = call(
+        &app,
+        "PUT",
+        &format!("/api/models/{m}/diagrams/public/_all"),
+        Some(json!({ "diagram": d, "fingerprint": null })),
+    )
+    .await;
+    assert_eq!(st, StatusCode::BAD_REQUEST);
+    // 主题图
+    let body = json!({ "schema": "public", "name": "t1", "tables": ["a"] });
+    let (st, _) = call(&app, "POST", &format!("/api/models/{m}/diagrams"), Some(body)).await;
+    assert_eq!(st, StatusCode::CREATED);
+    let (st, _) = call(
+        &app,
+        "POST",
+        &format!("/api/models/{m}/diagrams/public/t1/rename"),
+        Some(json!({ "to": "t2" })),
+    )
+    .await;
+    assert_eq!(st, StatusCode::NO_CONTENT);
+    let (_, v) = call(&app, "GET", &format!("/api/models/{m}/diagrams/public/t2"), None).await;
+    assert_eq!(v["diagram"]["tables"], json!(["a"]));
+    let (st, _) = call(&app, "DELETE", &format!("/api/models/{m}/diagrams/public/t2"), None).await;
+    assert_eq!(st, StatusCode::NO_CONTENT);
+    // 逻辑外键
+    let key = json!({ "columns": ["x"], "ref_table": "b", "ref_columns": ["id"], "comment": "c" });
+    let (st, r) = call(
+        &app,
+        "POST",
+        &format!("/api/models/{m}/tables/public/a/logical-keys"),
+        Some(json!({ "key": key })),
+    )
+    .await;
+    assert_eq!(st, StatusCode::OK, "{r}");
+    let (_, v) = call(&app, "GET", &format!("/api/models/{m}/diagrams/public/_all"), None).await;
+    let a = v["tables"].as_array().unwrap().iter().find(|t| t["table"]["name"] == "a").unwrap().clone();
+    assert_eq!(a["logical_keys"][0]["ref_table"], "b");
+    let bad = json!({ "key": { "columns": ["x"], "ref_table": "b", "ref_columns": ["nope"] } });
+    let (st, _) =
+        call(&app, "POST", &format!("/api/models/{m}/tables/public/a/logical-keys"), Some(bad)).await;
+    assert_eq!(st, StatusCode::BAD_REQUEST);
+    let fp = r["fingerprint"].as_str().unwrap();
+    let (st, _) = call(
+        &app,
+        "DELETE",
+        &format!("/api/models/{m}/tables/public/a/logical-keys/0?fingerprint={fp}"),
+        None,
+    )
+    .await;
+    assert_eq!(st, StatusCode::OK);
+    // 导出图片只写 .svg、.png
+    let png = dir.path().join("er.png");
+    let (st, _) =
+        call(&app, "POST", "/api/files/image", Some(json!({ "path": png, "data": "iVBORw0K" }))).await;
+    assert_eq!(st, StatusCode::NO_CONTENT);
+    assert_eq!(std::fs::read(&png).unwrap(), [0x89, b'P', b'N', b'G', 0x0d, 0x0a]);
+    let (st, _) =
+        call(&app, "POST", "/api/files/image", Some(json!({ "path": dir.path().join("x.exe"), "data": "" })))
+            .await;
+    assert_eq!(st, StatusCode::BAD_REQUEST);
 }
 
 #[tokio::test]
```

- [ ] **Step 2: 修改 `crates/dblens-api/Cargo.toml`**

```diff
--- a/crates/dblens-api/Cargo.toml
+++ b/crates/dblens-api/Cargo.toml
@@ -7,6 +7,7 @@ publish.workspace = true
 
 [dependencies]
 axum.workspace = true
+base64.workspace = true
 dblens-core.workspace = true
 dblens-dialect.workspace = true
 dblens-driver.workspace = true
```

- [ ] **Step 3: 新建 `crates/dblens-api/src/diagrams.rs`**

```rust
//! ER 图与逻辑外键的接口（M4b 设计文档 5.2 节），以及桌面程序导出图片时写文件。都要求 `local_files`。

use std::path::PathBuf;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use base64::Engine;
use dblens_core::{Error, msg};
use dblens_service::{Diagram, DiagramSummary, DiagramView, LogicalKey};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::AppState;
use crate::error::ApiResult;
use crate::models::local_only;

#[derive(Deserialize)]
pub struct SchemaQuery {
    schema: String,
}

pub async fn list(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<SchemaQuery>,
) -> ApiResult<Json<Vec<DiagramSummary>>> {
    local_only(&s)?;
    Ok(Json(s.service.models.diagrams(&id, &q.schema)?))
}

#[derive(Deserialize)]
pub struct NewDiagram {
    schema: String,
    name: String,
    #[serde(default)]
    tables: Vec<String>,
}

/// 新建主题图，返回 201 和 `{fingerprint}`。
pub async fn create(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Json(b): Json<NewDiagram>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    local_only(&s)?;
    let fp = s.service.models.create_diagram(&id, &b.schema, &b.name, b.tables)?;
    Ok((StatusCode::CREATED, Json(json!({ "fingerprint": fp }))))
}

pub async fn get(
    State(s): State<AppState>,
    Path((id, schema, name)): Path<(String, String, String)>,
) -> ApiResult<Json<DiagramView>> {
    local_only(&s)?;
    Ok(Json(s.service.models.read_diagram(&id, &schema, &name)?))
}

#[derive(Deserialize)]
pub struct SaveBody {
    diagram: Diagram,
    #[serde(default)]
    fingerprint: Option<String>,
}

/// 保存图，返回新的 `{fingerprint}`；文件已被修改时 400。
pub async fn save(
    State(s): State<AppState>,
    Path((id, schema, name)): Path<(String, String, String)>,
    Json(b): Json<SaveBody>,
) -> ApiResult<Json<Value>> {
    local_only(&s)?;
    let fp = s.service.models.save_diagram(&id, &schema, &name, b.diagram, b.fingerprint.as_deref())?;
    Ok(Json(json!({ "fingerprint": fp })))
}

pub async fn delete(
    State(s): State<AppState>,
    Path((id, schema, name)): Path<(String, String, String)>,
) -> ApiResult<StatusCode> {
    local_only(&s)?;
    s.service.models.delete_diagram(&id, &schema, &name)?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
pub struct RenameBody {
    to: String,
}

pub async fn rename(
    State(s): State<AppState>,
    Path((id, schema, name)): Path<(String, String, String)>,
    Json(b): Json<RenameBody>,
) -> ApiResult<StatusCode> {
    local_only(&s)?;
    s.service.models.rename_diagram(&id, &schema, &name, &b.to)?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
pub struct KeyBody {
    key: LogicalKey,
    #[serde(default)]
    fingerprint: Option<String>,
}

/// 加逻辑外键，返回表文件的新 `{fingerprint}`。
pub async fn add_key(
    State(s): State<AppState>,
    Path((id, schema, table)): Path<(String, String, String)>,
    Json(b): Json<KeyBody>,
) -> ApiResult<Json<Value>> {
    local_only(&s)?;
    let fp = s.service.models.add_logical_key(&id, &schema, &table, b.key, b.fingerprint.as_deref())?;
    Ok(Json(json!({ "fingerprint": fp })))
}

#[derive(Deserialize)]
pub struct FingerprintQuery {
    #[serde(default)]
    fingerprint: Option<String>,
}

pub async fn delete_key(
    State(s): State<AppState>,
    Path((id, schema, table, index)): Path<(String, String, String, usize)>,
    Query(q): Query<FingerprintQuery>,
) -> ApiResult<Json<Value>> {
    local_only(&s)?;
    let fp = s.service.models.delete_logical_key(&id, &schema, &table, index, q.fingerprint.as_deref())?;
    Ok(Json(json!({ "fingerprint": fp })))
}

#[derive(Deserialize)]
pub struct ImageBody {
    path: PathBuf,
    /// base64 编码的文件内容。
    data: String,
}

/// 桌面程序：把导出的 ER 图写到"另存为"选的路径（只接受 `.svg`、`.png`）。
pub async fn save_image(State(s): State<AppState>, Json(b): Json<ImageBody>) -> ApiResult<StatusCode> {
    local_only(&s)?;
    let ok =
        b.path.extension().is_some_and(|e| e.eq_ignore_ascii_case("svg") || e.eq_ignore_ascii_case("png"));
    if !ok {
        return Err(Error::invalid(msg!("model.image_ext")).into());
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(b.data.as_bytes())
        .map_err(|e| Error::invalid(msg!("model.image_data", error = e)))?;
    std::fs::write(&b.path, bytes)
        .map_err(|e| Error::internal(msg!("model.io_failed", path = b.path.display(), error = e)))?;
    Ok(StatusCode::NO_CONTENT)
}
```

- [ ] **Step 4: 修改 `crates/dblens-api/src/lib.rs`**

```diff
--- a/crates/dblens-api/src/lib.rs
+++ b/crates/dblens-api/src/lib.rs
@@ -3,6 +3,7 @@
 
 mod auth;
 mod design;
+mod diagrams;
 mod error;
 mod exports;
 mod lang;
@@ -104,6 +105,15 @@ pub fn router(service: Arc<Service>, config: ApiConfig) -> Router {
         .route("/compare/preview", post(models::sync_preview))
         .route("/compare/sync", post(models::sync))
         .route("/compare/script", post(models::save_script))
+        .route("/models/{id}/diagrams", get(diagrams::list).post(diagrams::create))
+        .route(
+            "/models/{id}/diagrams/{schema}/{name}",
+            get(diagrams::get).put(diagrams::save).delete(diagrams::delete),
+        )
+        .route("/models/{id}/diagrams/{schema}/{name}/rename", post(diagrams::rename))
+        .route("/models/{id}/tables/{schema}/{name}/logical-keys", post(diagrams::add_key))
+        .route("/models/{id}/tables/{schema}/{name}/logical-keys/{index}", delete(diagrams::delete_key))
+        .route("/files/image", post(diagrams::save_image))
         .route("/exports", post(exports::create))
         .route("/exports/{id}", get(exports::status).delete(exports::cancel_or_remove))
         .route("/exports/{id}/file", get(exports::file))
```

- [ ] **Step 5: 修改 `crates/dblens-api/src/models.rs`**

```diff
--- a/crates/dblens-api/src/models.rs
+++ b/crates/dblens-api/src/models.rs
@@ -18,7 +18,7 @@ use serde_json::{Value, json};
 use crate::AppState;
 use crate::error::ApiResult;
 
-fn local_only(s: &AppState) -> ApiResult<()> {
+pub(crate) fn local_only(s: &AppState) -> ApiResult<()> {
     if !s.config.local_files {
         return Err(Error::Denied { reason: msg!("model.local_only") }.into());
     }
```

- [ ] **Step 6: 修改 `crates/dblens-core/locales/app.json`**

```diff
--- a/crates/dblens-core/locales/app.json
+++ b/crates/dblens-core/locales/app.json
@@ -149,6 +149,8 @@
   "model.diagram_bad_name": {"zh-CN": "图名 %{name} 不可用（不能为空，也不能是 _all）", "en": "The diagram name %{name} is not allowed (it must not be empty or _all)"},
   "model.diagram_exists": {"zh-CN": "图 %{name} 已存在", "en": "Diagram %{name} already exists"},
   "model.dir_not_empty": {"zh-CN": "目录 %{path} 不为空", "en": "Directory %{path} is not empty"},
+  "model.image_data": {"zh-CN": "图片数据无效：%{error}", "en": "Invalid image data: %{error}"},
+  "model.image_ext": {"zh-CN": "图片只能保存为 .svg 或 .png 文件", "en": "Images can only be saved as .svg or .png files"},
   "model.io_failed": {"zh-CN": "读写 %{path} 失败：%{error}", "en": "Failed to access %{path}: %{error}"},
   "model.lk_column_missing": {"zh-CN": "列 %{name} 不存在", "en": "Column %{name} does not exist"},
   "model.lk_count_mismatch": {"zh-CN": "逻辑外键两边的列数必须相同且不为空", "en": "Both sides of a logical key must have the same, non-zero number of columns"},
```

- [ ] **Step 7: 检查**

```bash
export CARGO_INCREMENTAL=0
cargo fmt --all
cargo clippy --config "source.mirror.registry='sparse+https://rsproxy.cn/index/'" --workspace --all-targets -- -D warnings
DBLENS_LANG=en cargo test --config "source.mirror.registry='sparse+https://rsproxy.cn/index/'" --workspace
```

Expected: 全部测试通过（api 的 `tests/models.rs` 新增 1 个，禁止的检查多 2 处）。

- [ ] **Step 8: 提交**

```bash
git add Cargo.lock crates/dblens-api/Cargo.toml crates/dblens-api/src/diagrams.rs crates/dblens-api/src/lib.rs crates/dblens-api/src/models.rs crates/dblens-api/tests/models.rs crates/dblens-core/locales/app.json
git commit -m "feat(api): ER 图与逻辑外键的接口，桌面程序导出图片" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 5: 实库测试：反向生成后的默认 ER 图

`dblens-service/tests/live.rs` 的 `model_flow`：同步到库、写回模型之后，把这个 schema 再反向生成一次，读默认图，核对 `m4_ord` 指向 `m4_customer` 的外键列对应（`cust_id` → `id`），默认图没有文件（指纹为 None）。原来的第 5 步（库对库）改为第 6 步。

**Files:**
- Modify: `crates/dblens-service/tests/live.rs`

**Interfaces:**
- Consumes：任务 3 的 `read_diagram`、`DEFAULT_DIAGRAM`。

- [ ] **Step 1: 修改 `crates/dblens-service/tests/live.rs`**

```diff
--- a/crates/dblens-service/tests/live.rs
+++ b/crates/dblens-service/tests/live.rs
@@ -21,10 +21,10 @@ use dblens_dialect::meta::{MetaArgs, MetaKey};
 use dblens_dialect::{Compat, DdlKind, Direction, RowChange, Variant, WriteMode};
 use dblens_driver::{ConnectParams, DriverId};
 use dblens_service::{
-    ChangeSet, ColumnMapping, CompareRequest, CompareResult, CompareSideRef, DEFAULT_BATCH, DataOrder,
-    DefaultPolicy, DesignRequest, DriverRegistry, Environment, ExecRequest, ExportFormat, ExportOptions,
-    ExportRequest, ExportSource, ExportState, ExportStatus, FileOptions, FileSource, GenerateKind,
-    ImportRequest, ModelDesignRequest, NewColumn, NewTable, ObjectKind, OnError, ProfileInput,
+    ChangeSet, ColumnMapping, CompareRequest, CompareResult, CompareSideRef, DEFAULT_BATCH, DEFAULT_DIAGRAM,
+    DataOrder, DefaultPolicy, DesignRequest, DriverRegistry, Environment, ExecRequest, ExportFormat,
+    ExportOptions, ExportRequest, ExportSource, ExportState, ExportStatus, FileOptions, FileSource,
+    GenerateKind, ImportRequest, ModelDesignRequest, NewColumn, NewTable, ObjectKind, OnError, ProfileInput,
     ReverseRequest, SecretBackend, Secrets, Service, SessionEvent, SessionId, SqlFileRequest, Store,
     SyncRequest, TaskState, TaskStatus, Transaction,
 };
@@ -1616,7 +1616,24 @@ async fn model_flow(env: &str) {
     let r = s.compare(&compare(&md, &db)).await.unwrap();
     assert!(m4_items(&r).is_empty(), "{env}：{}", m4_detail(&r));
 
-    // 5. 库对库：同一个库中的第二个 schema
+    // 5. ER 图（M4b）：同步后的库再反向生成一次，默认图中外键连线的列对应
+    let req = ReverseRequest {
+        profile_id: live.profile_id.clone(),
+        schemas: vec![schema.clone()],
+        path: dir.path().join("model2").to_string_lossy().into(),
+        name: "m4b".into(),
+    };
+    let st = wait_task(s, &s.start_reverse(req).await.unwrap()).await;
+    assert_eq!(st.state, TaskState::Done, "{env}：{st:?}");
+    let m2 = st.model.unwrap();
+    let v = s.models.read_diagram(&m2, &schema, DEFAULT_DIAGRAM).unwrap();
+    assert_eq!(v.fingerprint, None, "{env}");
+    let ord = v.tables.iter().find(|t| t.table.name == n("m4_ord")).expect("m4_ord");
+    let fk = ord.table.foreign_keys.iter().find(|f| f.ref_table == n("m4_customer")).expect("外键");
+    assert_eq!((fk.columns.clone(), fk.ref_columns.clone()), (vec![n("cust_id")], vec![n("id")]), "{env}");
+    assert!(v.tables.iter().any(|t| t.table.name == n("m4_customer")), "{env}");
+
+    // 6. 库对库：同一个库中的第二个 schema
     let other: Option<&'static str> = match live.driver {
         DriverId::Postgres => Some("m4_b"),
         DriverId::Mysql => Some("dblens_test_m4b"),
```

- [ ] **Step 2: 检查**

```bash
export CARGO_INCREMENTAL=0
cargo fmt --all
cargo clippy --config "source.mirror.registry='sparse+https://rsproxy.cn/index/'" --workspace --all-targets -- -D warnings
DBLENS_LANG=en cargo test --config "source.mirror.registry='sparse+https://rsproxy.cn/index/'" --workspace
```

实库测试：在上面的 `cargo test` 前以内联环境变量给出原有 5 个库的 `DBLENS_TEST_MYSQL_URL`、`DBLENS_TEST_MARIADB_URL`、`DBLENS_TEST_PG_URL`、`DBLENS_TEST_ORACLE_URL`、`DBLENS_TEST_DM_URL`，以及国产库第一批的 `DBLENS_TEST_TIDB_URL`、`DBLENS_TEST_OCEANBASE_URL`（用户名写成 `admin%40test`）、`DBLENS_TEST_OPENGAUSS_URL`、`DBLENS_TEST_OPENGAUSS_A_URL`（库 `dblens_test_a`）、`DBLENS_TEST_OPENGAUSS_SHA_URL`（账号 `admin_sha`，库 `dblens_test_sha`）、`DBLENS_TEST_KINGBASE_URL`（54321）、`DBLENS_TEST_KINGBASE_PG_URL`（54322）。新目标的密码在测试虚拟机 root 的 `/root/dblens-test/creds.env`，用 `ssh … 'sudo cat …'` 读进当前 shell 的变量，不写进任何文件。只跑模型的测试：`cargo test … -p dblens-service --test live model -- --test-threads=1`。
- 人大金仓的开发版授权最多 10 个连接：它的测试要串行运行（`--test-threads=1`）。
- 达梦的测试与其他测试同时运行时偶发"网络通讯失败"，失败时用 `dameng_model` 单独重跑。

Expected: 全部测试通过：模型的实库测试 11 个。

- [ ] **Step 3: 提交**

```bash
git add crates/dblens-service/tests/live.rs
git commit -m "test(service): 实库测试中核对反向生成后默认 ER 图的外键" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 6: 前端：ER 图标签

- 依赖：`pnpm add @xyflow/react@12.12.0 @dagrejs/dagre@3.1.1 html-to-image@1.11.11`；`scripts/check-deps.mjs` 加"有意不升级"的名单 `HOLD`（`html-to-image`）。
- `shared/api.ts`：`LogicalKey`、`ErDisplay`、`ErPosition`、`ErDiagram`、`ErTable`、`ErWarning`、`DiagramView`、`DEFAULT_DIAGRAM`；方法 `diagrams`、`createDiagram`、`diagram`、`saveDiagram`、`deleteDiagram`、`renameDiagram`、`addLogicalKey`、`deleteLogicalKey`、`saveImage`、`schemaTables`。
- `features/editor/tabs.ts`：`ErTarget`（`{kind: 'model', path, schema, diagram}` 或 `{kind: 'database', profile, schema}`）、`QueryTab.er`、`openEr(target, title)`（同一张图只开一个）、`setEr`；保存与恢复时校验。
- `features/er/`（新）：`graph.ts`（节点和连线：每列的连接点 `列:in` / `列:out`，只显示表名时接在表头 `__table`；复合外键一条线标列数；自引用；其他 schema 和图外的表列在节点底部；逻辑外键带序号；节点尺寸；dagre 布局，保留已有位置、新表排在下方；搜索）、`TableNode.tsx`、`ErView.tsx`（工具栏：标题和"未保存"、搜索定位、显示方式、自动布局、提示、添加表、导出、重新加载、保存或"另存为模型…"；画布：拖动、拖线建逻辑外键、删除逻辑外键、双击打开设计器或数据标签、右键菜单、连线悬停提示；`colorMode` 跟随主题；导出时关掉只渲染可见区域、去掉选中标记）、`export.ts`、`events.ts`（ER 图中"新建表"打开的设计器保存后，主题图把新表加进来）。
- 查询键：模型图的数据用 `['model-tables', 模型, 'er', schema, 图]`，设计器保存、模型面板改表后一起刷新。
- 入口：模型面板 schema 下的"ER 图"节点和右键菜单；对象树 schema 右键"查看 ER 图"；设计器保存新表时通知 ER 图。
- 工作区懒加载 `ErView`；状态栏显示模型目录；`insertSql` 跳过 ER 图标签。
- 词条：新文件 `locales/{zh-CN,en}/er.json`；`tree.menu.er`。样式：`shell.css` 末尾的 ER 图部分。

**Files:**
- Modify: `scripts/check-deps.mjs`
- Modify: `web/package.json`
- Modify: `web/src/app/StatusBar.tsx`
- Modify: `web/src/app/shell.css`
- Modify: `web/src/features/design/DesignView.tsx`
- Modify: `web/src/features/editor/Workspace.tsx`
- Modify: `web/src/features/editor/actions.tsx`
- Modify: `web/src/features/editor/tabs.test.ts`
- Modify: `web/src/features/editor/tabs.ts`
- Create: `web/src/features/er/ErView.tsx`
- Create: `web/src/features/er/TableNode.tsx`
- Create: `web/src/features/er/events.ts`
- Create: `web/src/features/er/export.ts`
- Create: `web/src/features/er/graph.test.ts`
- Create: `web/src/features/er/graph.ts`
- Modify: `web/src/features/model/ModelsPanel.tsx`
- Modify: `web/src/features/tree/ObjectTree.tsx`
- Create: `web/src/locales/en/er.json`
- Modify: `web/src/locales/en/tree.json`
- Modify: `web/src/locales/index.ts`
- Create: `web/src/locales/zh-CN/er.json`
- Modify: `web/src/locales/zh-CN/tree.json`
- Modify: `web/src/shared/api.ts`

**Interfaces:**
- Consumes：任务 4 的接口；M4a 的模型面板、设计器的模型模式、`useModelDialogs().openReverse`。
- Produces：`useTabs` 的 `openEr(target, title)`、`setEr(id, target, title)`；`features/er/graph.ts` 的 `buildGraph(tables, display, schema)`、`layout(graph, positions, {direction, all})`。

- [ ] **Step 1: 修改 `web/src/features/editor/tabs.test.ts`**

```diff
--- a/web/src/features/editor/tabs.test.ts
+++ b/web/src/features/editor/tabs.test.ts
@@ -144,6 +144,23 @@ describe('打开后执行', () => {
     expect(parseTabs(JSON.stringify({ tabs: [bad] })).tabs).toEqual([])
   })
 
+  it('ER 图标签：同一张图只开一个，改名后跟着改，保存后恢复', () => {
+    useTabs.getState().restore(undefined)
+    const { openEr, setEr } = useTabs.getState()
+    const target = { kind: 'model' as const, path: 'D:/m', schema: 'public', diagram: '_all' }
+    const a = openEr(target, 'public · 全部表')
+    expect(openEr({ ...target }, 'x')).toBe(a)
+    const t1 = openEr({ ...target, diagram: '订单' }, 'public · 订单')
+    setEr(t1, { ...target, diagram: '订单2' }, 'public · 订单2')
+    const db = openEr({ kind: 'database', profile: 'p', schema: 'shop' }, 'shop · ER')
+    const tabs = useTabs.getState().tabs
+    expect(tabs.map((x) => [x.title, x.profileId])).toEqual([['public · 全部表', ''], ['public · 订单2', ''], ['shop · ER', 'p']])
+    const back = parseTabs(JSON.stringify({ tabs, active: db, seq: 1 }))
+    expect(back.tabs[1].er).toEqual({ ...target, diagram: '订单2' })
+    const bad = { id: 'x', title: 't', profileId: '', sql: '', er: { kind: 'model', path: 'D:/m', schema: 's' } }
+    expect(parseTabs(JSON.stringify({ tabs: [bad] })).tabs).toEqual([])
+  })
+
   it('标记只取走一次，不随标签保存', () => {
     const id = useTabs.getState().open('p', 'SELECT 1')
     markRun(id)
```

- [ ] **Step 2: 新建 `web/src/features/er/graph.test.ts`**

```ts
import { describe, expect, it } from 'vitest'

import type { DesignForeignKey, DesignTableModel, ErTable, LogicalKey } from '../../shared/api'
import { HEADER_HEIGHT, ROW_HEIGHT, TABLE_HANDLE, buildGraph, layout, nodeHeight, searchTables } from './graph'
import { dataUrlBase64, imageName } from './export'

const fk = (columns: string[], ref_table: string, ref_columns: string[], extra: Partial<DesignForeignKey> = {}): DesignForeignKey => ({
  name: `fk_${columns.join('_')}`,
  columns,
  ref_schema: null,
  ref_table,
  ref_columns,
  on_delete: null,
  on_update: null,
  ...extra,
})

function table(name: string, cols: string[], fks: DesignForeignKey[] = []): DesignTableModel {
  return {
    name,
    comment: null,
    columns: cols.map((c, i) => ({
      name: c,
      ordinal: i + 1,
      data_type: { native: 'integer', logical: null },
      nullable: i > 0,
      default: null,
      comment: null,
      auto_increment: false,
      generated: null,
    })),
    primary_key: { name: null, columns: [cols[0]] },
    uniques: [],
    foreign_keys: fks,
    checks: [],
    indexes: [],
    triggers: [],
    partition: null,
  }
}

const er = (t: DesignTableModel, logical_keys: LogicalKey[] = []): ErTable => ({ table: t, logical_keys })

/** customers；orders 的 (cust_id, cust_code) 复合外键指向 customers，parent_id 自引用，region_id 指向 crm.regions；
 * items 的逻辑外键指向 orders。 */
function sample(): ErTable[] {
  return [
    er(table('customers', ['id', 'code', 'name'])),
    er(
      table('orders', ['id', 'cust_id', 'cust_code', 'parent_id', 'region_id', 'note'], [
        fk(['cust_id', 'cust_code'], 'customers', ['id', 'code']),
        fk(['parent_id'], 'orders', ['id']),
        fk(['region_id'], 'regions', ['id'], { ref_schema: 'crm' }),
      ]),
    ),
    er(table('items', ['id', 'order_ref']), [{ name: 'lk_order', columns: ['order_ref'], ref_table: 'orders', ref_columns: ['id'] }]),
  ]
}

describe('ER 图的节点和连线', () => {
  it('复合外键一条线、自引用、跨 schema 列在节点上、逻辑外键为虚线', () => {
    const g = buildGraph(sample(), 'columns', 'public')
    expect(g.nodes.map((n) => n.id)).toEqual(['customers', 'orders', 'items'])
    expect(g.edges.map((e) => [e.id, e.source, e.target, e.sourceHandle, e.targetHandle, e.count, e.logical])).toEqual([
      ['fk:orders:0', 'orders', 'customers', 'cust_id:out', 'id:in', 2, false],
      ['fk:orders:1', 'orders', 'orders', 'parent_id:out', 'id:in', 1, false],
      ['lk:items:0', 'items', 'orders', 'order_ref:out', 'id:in', 1, true],
    ])
    expect(g.edges[0].title).toBe('fk_cust_id_cust_code: cust_id → id, cust_code → code')
    expect(g.edges[2].logicalIndex).toBe(0)
    const orders = g.nodes[1]
    expect(orders.data.external).toEqual(['→ crm.regions'])
    expect(orders.data.columns.find((c) => c.name === 'cust_id')).toMatchObject({ fk: true, pk: false, nullable: true })
    expect(orders.height).toBe(nodeHeight(6, 1))
  })

  it('主题图中没有的表列在节点上；只显示键列、只显示表名', () => {
    const [customers, orders] = sample()
    const g = buildGraph([orders], 'columns', 'public')
    expect(g.nodes[0].data.external).toEqual(['→ customers', '→ crm.regions'])
    expect(g.edges.map((e) => e.id)).toEqual(['fk:orders:1'])
    const keys = buildGraph([customers, orders], 'keys', 'public')
    expect(keys.nodes[0].data.columns.map((c) => c.name)).toEqual(['id', 'code'])
    expect(keys.nodes[1].data.columns.map((c) => c.name)).toEqual(['id', 'cust_id', 'cust_code', 'parent_id', 'region_id'])
    const names = buildGraph([customers, orders], 'names', 'public')
    expect(names.nodes[0].data.columns).toEqual([])
    expect(names.nodes[0].height).toBe(HEADER_HEIGHT + 6)
    expect(names.edges[0]).toMatchObject({ sourceHandle: `${TABLE_HANDLE}:out`, targetHandle: `${TABLE_HANDLE}:in` })
    expect(ROW_HEIGHT).toBeGreaterThan(0)
  })

  it('文件有错的表只有名字', () => {
    const bad: ErTable = { table: table('bad', ['id']), logical_keys: [], error: { path: 'tables/public/bad.yaml', message: 'line 1' } }
    const g = buildGraph([bad], 'columns', 'public')
    expect(g.nodes[0].data).toMatchObject({ error: 'line 1', columns: [] })
  })
})

describe('自动布局', () => {
  it('全都没有位置时用 dagre；有位置时保留，只把新表排在下方', () => {
    const g = buildGraph(sample(), 'columns', 'public')
    const auto = layout(g, {})
    expect(Object.keys(auto).sort()).toEqual(['customers', 'items', 'orders'])
    // 横向：被引用的 customers 在 orders 右边或左边，但 items → orders → customers 依次排开
    expect(new Set([auto.items.x, auto.orders.x, auto.customers.x]).size).toBe(3)
    const kept = layout(g, { customers: { x: 0, y: 0 }, orders: { x: 400, y: 0 } })
    expect(kept.customers).toEqual({ x: 0, y: 0 })
    expect(kept.orders).toEqual({ x: 400, y: 0 })
    expect(kept.items.y).toBe(nodeHeight(6, 1) + 80)
    const all = layout(g, { customers: { x: 0, y: 0 } }, { all: true, direction: 'TB' })
    expect(all.customers).not.toEqual({ x: 0, y: 0 })
  })

  it('搜索表名', () => {
    expect(searchTables(sample().map((x) => x.table), 'ORD')).toEqual(['orders'])
    expect(searchTables(sample().map((x) => x.table), ' ')).toEqual([])
  })
})

describe('导出', () => {
  it('data URL 转 base64、文件名', () => {
    expect(dataUrlBase64('data:image/png;base64,iVBOR')).toBe('iVBOR')
    expect(atob(dataUrlBase64('data:image/svg+xml;charset=utf-8,%3Csvg%3E'))).toBe('<svg>')
    expect(imageName('订单/明细', 'svg')).toBe('订单_明细.svg')
  })
})
```

- [ ] **Step 3: 修改 `scripts/check-deps.mjs`**

```diff
--- a/scripts/check-deps.mjs
+++ b/scripts/check-deps.mjs
@@ -15,6 +15,11 @@ const days = daysArg > 0 ? Number(process.argv[daysArg + 1]) : 7
 const cutoff = Date.now() - days * 86_400_000
 const npmRegistry = (process.env.NPM_REGISTRY ?? 'https://registry.npmjs.org').replace(/\/$/, '')
 
+/** 有意停在某个版本的依赖：名字 → 原因。复查时列出，不算可升级。 */
+const HOLD = {
+  'html-to-image': 'React Flow 文档的导出示例固定 1.11.11，更新的版本导出图片有问题（M4b）',
+}
+
 const isStable = (v) => /^\d+\.\d+\.\d+$/.test(v)
 const cmp = (a, b) => {
   const x = a.split('.').map(Number)
@@ -83,7 +88,9 @@ for (const r of rows) {
     console.log(`${r.eco}\t${r.name}\t${r.current}\t查询失败：${r.error}`)
     continue
   }
-  if (r.eligible && cmp(r.eligible[0], r.current) > 0) {
+  if (HOLD[r.name] && r.newest && cmp(r.newest[0], r.current) > 0) {
+    console.log(`${r.eco}\t${r.name}\t${r.current}\t${r.newest[0]} 有意不升级：${HOLD[r.name]}`)
+  } else if (r.eligible && cmp(r.eligible[0], r.current) > 0) {
     pending++
     console.log(`${r.eco}\t${r.name}\t${r.current} -> ${r.eligible[0]}\t（${r.eligible[1].slice(0, 10)} 发布）`)
   } else if (r.newest && cmp(r.newest[0], r.current) > 0) {
```

- [ ] **Step 4: 修改 `web/package.json`**

```diff
--- a/web/package.json
+++ b/web/package.json
@@ -15,15 +15,18 @@
   },
   "dependencies": {
     "@ant-design/icons": "6.3.4",
+    "@dagrejs/dagre": "3.1.1",
     "@glideapps/glide-data-grid": "6.0.3",
     "@monaco-editor/react": "4.7.0",
     "@tabler/icons-react": "3.49.0",
     "@tanstack/react-query": "5.104.5",
     "@tauri-apps/api": "2.12.4",
+    "@xyflow/react": "12.12.0",
     "antd": "6.6.7",
     "antlr4-c3": "3.4.4",
     "antlr4ng": "3.0.16",
     "dt-sql-parser": "4.5.1",
+    "html-to-image": "1.11.11",
     "i18next": "26.5.3",
     "monaco-editor": "0.57.0",
     "react": "19.3.1",
```

- [ ] **Step 5: 修改 `web/src/app/StatusBar.tsx`**

```diff
--- a/web/src/app/StatusBar.tsx
+++ b/web/src/app/StatusBar.tsx
@@ -69,7 +69,7 @@ export default function StatusBar() {
   if (active && !active.profileId) {
     return (
       <div className="row statusbar">
-        <span className="muted">{active.design?.model ?? t('model.compare.title')}</span>
+        <span className="muted">{active.design?.model ?? (active.er?.kind === 'model' ? active.er.path : t('model.compare.title'))}</span>
         <span className="grow" />
       </div>
     )
```

- [ ] **Step 6: 修改 `web/src/app/shell.css`**

```diff
--- a/web/src/app/shell.css
+++ b/web/src/app/shell.css
@@ -778,3 +778,101 @@ body {
 .compare-line.diff {
   background: var(--warn-bg);
 }
+
+/* ER 图（M4b） */
+.er-view {
+  display: flex;
+  flex-direction: column;
+  min-height: 0;
+  height: 100%;
+  outline: none;
+}
+.er-canvas {
+  position: relative;
+  min-height: 0;
+}
+.er-table {
+  background: var(--panel);
+  border: 1px solid var(--border);
+  border-radius: 4px;
+  font-size: 12px;
+  overflow: hidden;
+}
+.er-table.selected {
+  box-shadow: 0 0 0 2px var(--accent);
+}
+.er-table.error .er-table-h {
+  background: var(--danger-bg);
+}
+.er-table-h {
+  position: relative;
+  height: 30px;
+  line-height: 30px;
+  padding: 0 8px;
+  font-weight: 600;
+  background: var(--panel-2);
+  border-bottom: 1px solid var(--border);
+  overflow: hidden;
+  white-space: nowrap;
+  text-overflow: ellipsis;
+}
+.er-col {
+  position: relative;
+  display: flex;
+  gap: 6px;
+  height: 22px;
+  line-height: 22px;
+  padding: 0 8px;
+}
+.er-col-icon {
+  width: 12px;
+  flex: none;
+}
+.er-pk {
+  color: var(--warn);
+}
+.er-fk {
+  color: var(--accent);
+}
+.er-col-name {
+  flex: 1;
+  min-width: 0;
+  overflow: hidden;
+  text-overflow: ellipsis;
+  white-space: nowrap;
+}
+.er-col-type {
+  color: var(--muted);
+  font-family: var(--mono);
+  white-space: nowrap;
+}
+.er-external {
+  height: 20px;
+  line-height: 20px;
+  padding: 0 8px;
+  font-size: 11px;
+}
+.er-handle {
+  width: 6px !important;
+  height: 6px !important;
+  min-width: 0 !important;
+  opacity: 0.4;
+}
+.er-edge.logical path {
+  stroke-dasharray: 6 4;
+}
+.er-tip {
+  position: fixed;
+  z-index: 10;
+  padding: 2px 6px;
+  border-radius: 3px;
+  background: var(--panel-2);
+  border: 1px solid var(--border);
+  font-size: 12px;
+  pointer-events: none;
+}
+.er-warnings {
+  margin: 0;
+  padding-left: 18px;
+  max-width: 420px;
+}
```

- [ ] **Step 7: 修改 `web/src/features/design/DesignView.tsx`**

```diff
--- a/web/src/features/design/DesignView.tsx
+++ b/web/src/features/design/DesignView.tsx
@@ -24,6 +24,7 @@ import IndexesPane from './IndexesPane'
 import { type Draft, type Pane, emptyDraft, fromTable, isDirty, issuesOf, renamesOf, toModel } from './logic'
 import OptionsPane from './OptionsPane'
 import SaveDialog from './SaveDialog'
+import { useErEvents } from '../er/events'
 import { modelSource, profileSource } from './source'
 import SqlBox from './SqlBox'
 import { describeChange, scriptOf } from './summary'
@@ -183,6 +184,7 @@ export default function DesignView({ tab, profiles }: { tab: QueryTab; profiles:
       if (isNew) {
         useDesigns.getState().clear(tab.id)
         useTabs.getState().setDesign(tab.id, { schema: req.schema, table: req.draft.name.trim(), model: modelPath })
+        useErEvents.getState().tableCreated(tab.id, modelPath!, req.schema, req.draft.name.trim())
       } else void load()
     } catch (e) {
       message.error((e as Error).message)
```

- [ ] **Step 8: 修改 `web/src/features/editor/Workspace.tsx`**

```diff
--- a/web/src/features/editor/Workspace.tsx
+++ b/web/src/features/editor/Workspace.tsx
@@ -3,7 +3,7 @@
 // 标签多时先缩窄（悬停看全名），再放不下就用滚轮横向滚动，不显示滚动条；右端 ▾ 列出全部标签；右键菜单批量关闭。
 
 import { Dropdown, type MenuProps } from 'antd'
-import { IconArrowsDiff, IconChevronDown, IconFileCode, IconPlus, IconSchema, IconTable, IconTableOptions, IconX } from '@tabler/icons-react'
+import { IconAffiliate, IconArrowsDiff, IconChevronDown, IconFileCode, IconPlus, IconSchema, IconTable, IconTableOptions, IconX } from '@tabler/icons-react'
 import { useQuery } from '@tanstack/react-query'
 import { Spin } from 'antd'
 import { Suspense, type WheelEvent, lazy, useEffect, useRef } from 'react'
@@ -23,6 +23,8 @@ import { type QueryTab, useTabs } from './tabs'
 const DesignView = lazy(() => import('../design/DesignView'))
 // 比对标签的同步预览带 Monaco，同样懒加载
 const CompareView = lazy(() => import('../compare/CompareView'))
+// ER 图带 React Flow，打开第一张图时才加载
+const ErView = lazy(() => import('../er/ErView'))
 
 function TabTitle({ tab, active, envClass, profileName, profile }: {
   tab: QueryTab
@@ -64,7 +66,9 @@ function TabTitle({ tab, active, envClass, profileName, profile }: {
         }}
         title={`${tab.title} · ${profileName}`}
       >
-        {tab.compare ? (
+        {tab.er ? (
+          <IconAffiliate size={16} />
+        ) : tab.compare ? (
           <IconArrowsDiff size={16} />
         ) : tab.design?.model ? (
           <IconSchema size={16} />
@@ -165,7 +169,11 @@ export default function Workspace() {
         )}
       </div>
       {tab &&
-        (tab.compare ? (
+        (tab.er ? (
+          <Suspense fallback={<Spin style={{ margin: 48 }} />}>
+            <ErView key={tab.id} tab={tab} />
+          </Suspense>
+        ) : tab.compare ? (
           <Suspense fallback={<Spin style={{ margin: 48 }} />}>
             <CompareView key={tab.id} tab={tab} profiles={profiles} />
           </Suspense>
```

- [ ] **Step 9: 修改 `web/src/features/editor/actions.tsx`**

```diff
--- a/web/src/features/editor/actions.tsx
+++ b/web/src/features/editor/actions.tsx
@@ -49,7 +49,7 @@ export function useQueryActions() {
   const insertSql = (sql: string, profileId?: string) => {
     const { tabs, active } = useTabs.getState()
     const tab = tabs.find((x) => x.id === active)
-    if (tab && !tab.data && !tab.design && !tab.compare) sendInsert(sql)
+    if (tab && !tab.data && !tab.design && !tab.compare && !tab.er) sendInsert(sql)
     else openSql(sql, profileId)
   }
 
```

- [ ] **Step 10: 修改 `web/src/features/editor/tabs.ts`**

```diff
--- a/web/src/features/editor/tabs.ts
+++ b/web/src/features/editor/tabs.ts
@@ -4,6 +4,7 @@
 // 设计器标签（M3 设计文档 7.1 节）：带 design 的标签，设计一张已有的表（同一张表只开一个）或新建表；草稿不保存。
 // 设计模型中的表（M4a）时 design.model 是模型目录，标签不绑定连接（profileId 为空串）。
 // 比对标签（M4a 设计文档第 7 节）：带 compare 的标签，两边是模型或连接的一个 schema；也不绑定连接。
+// ER 图标签（M4b 设计文档 4.1 节）：带 er 的标签，模型的一张图（不绑定连接）或连接的一个 schema（只读）。
 
 import { create } from 'zustand'
 
@@ -34,6 +35,9 @@ export interface DesignTarget {
 /** 比对的一边：模型（按目录）或连接的一个 schema。 */
 export type CompareSide = { kind: 'model'; path: string; schema: string } | { kind: 'database'; profile: string; schema: string }
 
+/** ER 图标签画的图：模型（按目录）的一张图，或连接的一个 schema。 */
+export type ErTarget = { kind: 'model'; path: string; schema: string; diagram: string } | { kind: 'database'; profile: string; schema: string }
+
 /** 比对标签的两边（还没选时为 null）。 */
 export interface CompareTarget {
   source: CompareSide | null
@@ -52,6 +56,8 @@ export interface QueryTab {
   design?: DesignTarget
   /** 有值时是比对标签。 */
   compare?: CompareTarget
+  /** 有值时是 ER 图标签。 */
+  er?: ErTarget
 }
 
 const TARGET_KINDS: readonly unknown[] = ['table', 'view', 'materialized_view'] satisfies TargetKind[]
@@ -84,6 +90,12 @@ function isCompareSide(v: unknown): v is CompareSide | null {
   return s.kind === 'model' ? typeof s.path === 'string' : s.kind === 'database' && typeof s.profile === 'string'
 }
 
+function isErTarget(v: unknown): v is ErTarget {
+  const e = v as ErTarget | null
+  if (typeof e?.schema !== 'string') return false
+  return e.kind === 'model' ? typeof e.path === 'string' && typeof e.diagram === 'string' : e.kind === 'database' && typeof e.profile === 'string'
+}
+
 function isCompareTarget(v: unknown): v is CompareTarget {
   const c = v as CompareTarget | null
   return !!c && isCompareSide(c.source) && isCompareSide(c.target)
@@ -120,7 +132,8 @@ export function parseTabs(raw: string | undefined): Persisted {
         typeof t.sql === 'string' &&
         (t.data === undefined || isDataTarget(t.data)) &&
         (t.design === undefined || isDesignTarget(t.design)) &&
-        (t.compare === undefined || isCompareTarget(t.compare)),
+        (t.compare === undefined || isCompareTarget(t.compare)) &&
+        (t.er === undefined || isErTarget(t.er)),
     )
     const active = tabs.some((t) => t.id === v.active) ? (v.active as string) : (tabs[0]?.id ?? null)
     return { tabs, active, seq: typeof v.seq === 'number' && v.seq > 0 ? v.seq : tabs.length + 1 }
@@ -142,6 +155,10 @@ interface TabsState extends Persisted {
   openCompare(target: CompareTarget): string
   /** 修改比对标签的两边。 */
   setCompare(id: string, target: CompareTarget): void
+  /** 打开 ER 图：同一张图已经打开过时切换过去。`title` 是标签标题。返回标签的 id。 */
+  openEr(target: ErTarget, title: string): string
+  /** 图改名后，标签跟着改。 */
+  setEr(id: string, target: ErTarget, title: string): void
   /** 修改数据标签的过滤条件和排序。 */
   setData(id: string, patch: Pick<DataTarget, 'filter' | 'order'>): void
   close(id: string): void
@@ -205,6 +222,25 @@ export const useTabs = create<TabsState>()((set, get) => ({
     set({ tabs: [...get().tabs, tab], active: tab.id })
     return tab.id
   },
+  openEr: (target, title) => {
+    const { tabs } = get()
+    const same = tabs.find((x) => x.er && JSON.stringify(x.er) === JSON.stringify(target))
+    if (same) {
+      set({ active: same.id })
+      return same.id
+    }
+    const tab: QueryTab = {
+      id: newId(),
+      title,
+      profileId: target.kind === 'database' ? target.profile : '',
+      sql: '',
+      er: { ...target },
+    }
+    set({ tabs: [...tabs, tab], active: tab.id })
+    return tab.id
+  },
+  setEr: (id, target, title) =>
+    set({ tabs: get().tabs.map((x) => (x.id === id && x.er ? { ...x, er: { ...target }, title } : x)) }),
   setCompare: (id, target) =>
     set({ tabs: get().tabs.map((x) => (x.id === id && x.compare ? { ...x, compare: target } : x)) }),
   setData: (id, patch) =>
```

- [ ] **Step 11: 新建 `web/src/features/er/ErView.tsx`**

```tsx
// ER 图标签（M4b 设计文档第 4 节）：模型的一张图（可编辑布局、逻辑外键、主题图的表，Ctrl+S 保存），
// 或连接的一个 schema（只读、自动布局、不保存）。画布用 React Flow，布局用 dagre，导出用 html-to-image。
// 引入 React Flow，由工作区懒加载。

import {
  IconAlertTriangle,
  IconDeviceFloppy,
  IconFocusCentered,
  IconLayoutDistributeHorizontal,
  IconPhoto,
  IconPlus,
  IconRefresh,
  IconSchema,
} from '@tabler/icons-react'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import {
  Background,
  type Connection,
  Controls,
  type Edge,
  MarkerType,
  MiniMap,
  ReactFlow,
  ReactFlowProvider,
  getNodesBounds,
  useNodesState,
  useReactFlow,
} from '@xyflow/react'
import '@xyflow/react/dist/style.css'
import { Alert, App, AutoComplete, Badge, Button, Dropdown, Form, Input, Modal, Popover, Segmented, Select, Spin } from 'antd'
import { type KeyboardEvent, useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { useApi } from '../../app/context'
import { type DesignTableModel, type ErDiagram, type ErDisplay, type ErPosition, type ErTable, DEFAULT_DIAGRAM } from '../../shared/api'
import { useSetting } from '../../shared/settings'
import { desktop } from '../../shared/shell'
import { THEME_KEY, parseThemePref, resolveTheme, useSystemDark } from '../../shared/theme'
import { withVault } from '../connections/vault'
import { type QueryTab, useTabs } from '../editor/tabs'
import { useModelDialogs } from '../model/dialogs'
import { useErEvents } from './events'
import { type ImageFormat, dataUrlBase64, imageName, renderImage } from './export'
import { type Direction, TABLE_HANDLE, buildGraph, layout, searchTables } from './graph'
import TableNode, { type TableNodeType } from './TableNode'

const NODE_TYPES = { table: TableNode }
/** 默认图超过这么多张表时先只显示表名。 */
const LARGE = 300

interface Loaded {
  tables: ErTable[]
  diagram: ErDiagram
  fingerprint: string | null
  warnings: string[]
}

/** 拖线建逻辑外键时要填的名称、注释。 */
interface PendingKey {
  table: string
  column: string
  refTable: string
  refColumn: string
}

function Canvas({ tab }: { tab: QueryTab }) {
  const api = useApi()
  const { t } = useTranslation()
  const { message, modal } = App.useApp()
  const qc = useQueryClient()
  const flow = useReactFlow()
  const theme = resolveTheme(parseThemePref(useSetting(THEME_KEY)), useSystemDark())
  const target = tab.er!
  const isModel = target.kind === 'model'
  const schema = target.schema

  const modelQ = useQuery({
    queryKey: ['model-open', isModel ? target.path : null],
    queryFn: () => api.openModel((target as { path: string }).path),
    enabled: isModel,
    staleTime: Infinity,
  })
  const modelId = modelQ.data?.id
  const diagramName = isModel ? target.diagram : DEFAULT_DIAGRAM
  // 键以 model-tables 开头：设计器保存、模型面板中改表后一起刷新
  const dataQ = useQuery({
    queryKey: isModel ? ['model-tables', modelId, 'er', schema, diagramName] : ['er-db', target.profile, schema],
    queryFn: async (): Promise<Loaded> => {
      if (isModel) {
        const v = await api.diagram(modelId!, schema, diagramName)
        const warnings = v.warnings.map((w) =>
          w.kind === 'missing_table'
            ? t('er.warning.missingTable', { name: w.name })
            : t('er.warning.brokenKey', { table: w.table, name: w.name ?? `#${w.index + 1}` }),
        )
        for (const x of v.tables) if (x.error) warnings.push(t('er.warning.badFile', { name: x.table.name, message: x.error.message }))
        return { tables: v.tables, diagram: v.diagram, fingerprint: v.fingerprint, warnings }
      }
      const tables = await withVault(api, () => api.schemaTables(target.profile, schema))
      return {
        tables: tables.map((table) => ({ table, logical_keys: [] })),
        diagram: { name: DEFAULT_DIAGRAM, tables: 'all', display: tables.length > LARGE ? 'names' : 'columns' },
        fingerprint: null,
        warnings: [],
      }
    },
    enabled: !isModel || !!modelId,
  })
  const data = dataQ.data

  // 本地的编辑：位置、显示方式、主题图的表；dirty 时标题标"未保存"
  const [positions, setPositions] = useState<Record<string, ErPosition>>({})
  const [display, setDisplay] = useState<ErDisplay>('columns')
  const [tableList, setTableList] = useState<'all' | string[]>('all')
  const [fingerprint, setFingerprint] = useState<string | null>(null)
  const [dirty, setDirty] = useState(false)
  const [exporting, setExporting] = useState(false)
  const [pending, setPending] = useState<PendingKey | null>(null)
  const [adding, setAdding] = useState(false)
  const loadedFor = useRef<string | null>(null)

  // 第一次读到（或重新加载）时取文件中的布局；之后的刷新保留未保存的改动
  useEffect(() => {
    if (!data) return
    const key = `${tab.id}:${data.fingerprint ?? 'none'}`
    if (loadedFor.current === null || (!dirty && loadedFor.current !== key)) {
      setPositions(data.diagram.positions ?? {})
      const large = data.fingerprint === null && data.tables.length > LARGE
      setDisplay(large ? 'names' : data.diagram.display)
      setTableList(data.diagram.tables)
      setFingerprint(data.fingerprint)
      setDirty(false)
    }
    loadedFor.current = key
  }, [data, tab.id, dirty])

  const tables = useMemo(
    () => (data?.tables ?? []).filter((x) => tableList === 'all' || tableList.includes(x.table.name)),
    [data, tableList],
  )
  const graph = useMemo(() => buildGraph(tables, display, schema), [tables, display, schema])

  // 没有位置的表自动排
  useEffect(() => {
    if (!graph.nodes.length || graph.nodes.every((n) => positions[n.id])) return
    setPositions(layout(graph, positions))
  }, [graph, positions])

  const [nodes, setNodes, onNodesChange] = useNodesState<TableNodeType>([])
  useEffect(() => {
    setNodes((prev) =>
      graph.nodes.map((n) => ({
        id: n.id,
        type: 'table' as const,
        position: positions[n.id] ?? { x: 0, y: 0 },
        data: n.data,
        selected: prev.find((p) => p.id === n.id)?.selected ?? false,
        style: { width: n.width },
      })),
    )
  }, [graph, positions, setNodes])

  const edges: Edge[] = useMemo(
    () =>
      graph.edges.map((e) => ({
        id: e.id,
        source: e.source,
        target: e.target,
        sourceHandle: e.sourceHandle,
        targetHandle: e.targetHandle,
        type: 'smoothstep',
        label: e.count > 1 ? t('er.columns', { count: e.count }) : undefined,
        className: e.logical ? 'er-edge logical' : 'er-edge',
        markerEnd: { type: MarkerType.ArrowClosed },
        data: { title: e.title, logicalIndex: e.logicalIndex },
      })),
    [graph, t],
  )

  const markDirty = () => isModel && setDirty(true)

  const autoLayout = (direction: Direction, all: boolean) => {
    setPositions(layout(graph, positions, { direction, all }))
    markDirty()
    setTimeout(() => void flow.fitView({ padding: 0.1 }), 50)
  }

  const save = useCallback(async () => {
    if (!isModel || !modelId || !data || !dirty) return
    const diagram: ErDiagram = { ...data.diagram, display, tables: tableList, positions }
    try {
      const fp = await api.saveDiagram(modelId, schema, diagramName, diagram, fingerprint)
      setFingerprint(fp)
      setDirty(false)
      message.success(t('er.saved'))
      void qc.invalidateQueries({ queryKey: ['model-diagrams', modelId] })
      void dataQ.refetch()
    } catch (e) {
      message.error((e as Error).message)
    }
  }, [isModel, modelId, data, dirty, display, tableList, positions, api, schema, diagramName, fingerprint, message, t, qc, dataQ])

  const reload = () => {
    const go = () => {
      setDirty(false)
      loadedFor.current = null
      void dataQ.refetch()
    }
    if (dirty) modal.confirm({ title: t('er.reloadConfirm'), okType: 'danger', onOk: go })
    else go()
  }

  // 设计器保存了从这张图新建的表：主题图把它加进来
  const created = useErEvents((s) => s.created)
  useEffect(() => {
    if (!created || created.erTab !== tab.id || tableList === 'all') return
    if (!tableList.includes(created.name)) {
      setTableList([...tableList, created.name])
      setDirty(true)
    }
  }, [created])

  const onConnect = (c: Connection) => {
    if (!isModel || !c.sourceHandle || !c.targetHandle) return
    const col = (h: string) => h.replace(/:(in|out)$/, '')
    const column = col(c.sourceHandle)
    const refColumn = col(c.targetHandle)
    if (column === TABLE_HANDLE || refColumn === TABLE_HANDLE) {
      message.info(t('er.key.needColumns'))
      return
    }
    setPending({ table: c.source, column, refTable: c.target, refColumn })
  }

  const deleteKey = (e: Edge) => {
    const index = (e.data as { logicalIndex: number | null } | undefined)?.logicalIndex
    if (!isModel || index === null || index === undefined) return
    modal.confirm({
      title: t('er.key.deleteTitle'),
      content: (e.data as { title: string }).title,
      okType: 'danger',
      onOk: async () => {
        await api.deleteLogicalKey(modelId!, schema, e.source, index)
        void dataQ.refetch()
      },
    })
  }

  const openTable = (name: string) => {
    if (target.kind === 'model') useTabs.getState().openDesign('', { schema, table: name, model: target.path })
    else useTabs.getState().openData(target.profile, { schema, object: name, objectKind: 'table' })
  }

  const newTable = () => {
    if (target.kind !== 'model') return
    const id = useTabs.getState().openDesign('', { schema, table: null, model: target.path })
    useErEvents.getState().wait(id, tab.id)
  }

  const exportImage = async (format: ImageFormat, white: boolean) => {
    setExporting(true)
    // 去掉选中的标记；关掉"只渲染可见区域"，等全部节点画出来
    setNodes((ns) => ns.map((x) => ({ ...x, selected: false })))
    await new Promise((r) => setTimeout(r, 150))
    try {
      const viewport = document.querySelector<HTMLElement>(`[data-er-tab="${tab.id}"] .react-flow__viewport`)
      if (!viewport) return
      const bg = white ? '#ffffff' : getComputedStyle(document.body).getPropertyValue('--bg').trim() || '#ffffff'
      const url = await renderImage(viewport, getNodesBounds(flow.getNodes()), format, bg)
      const base = isModel ? (diagramName === DEFAULT_DIAGRAM ? schema : diagramName) : schema
      const name = imageName(base, format)
      if (desktop) {
        const path = await desktop.saveDialog(name, [{ name: format.toUpperCase(), extensions: [format] }])
        if (path) {
          await api.saveImage(path, dataUrlBase64(url))
          message.success(t('er.exported'))
        }
      } else {
        const a = document.createElement('a')
        a.href = url
        a.download = name
        a.click()
      }
    } catch (e) {
      message.error((e as Error).message)
    } finally {
      setExporting(false)
    }
  }

  const [search, setSearch] = useState('')
  const locate = (name: string) => {
    const n = flow.getNode(name)
    if (!n) return
    setNodes((ns) => ns.map((x) => ({ ...x, selected: x.id === name })))
    void flow.setCenter(n.position.x + (n.measured?.width ?? 240) / 2, n.position.y + 40, { zoom: 1, duration: 300 })
  }

  const [hover, setHover] = useState<{ x: number; y: number; text: string } | null>(null)
  const [menu, setMenu] = useState<{ x: number; y: number; items: { key: string; label: string; danger?: boolean }[]; on(key: string): void } | null>(null)

  const onKeyDown = (e: KeyboardEvent) => {
    if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 's') {
      e.preventDefault()
      void save()
    }
  }

  if (modelQ.error) return <Alert type="error" style={{ margin: 12 }} message={(modelQ.error as Error).message} />
  if (dataQ.error) return <Alert type="error" style={{ margin: 12 }} message={(dataQ.error as Error).message} action={<Button onClick={reload}>{t('er.reload')}</Button>} />
  if (!data) return <Spin style={{ margin: 48 }} />

  const readOnly = !isModel || !!modelQ.data?.read_only
  const isTopic = tableList !== 'all'
  const title = isModel
    ? `${modelQ.data?.info.name ?? ''} / ${schema} · ${diagramName === DEFAULT_DIAGRAM ? t('er.defaultDiagram') : diagramName}`
    : `${schema} · ${t('er.readOnly')}`
  return (
    <div className="er-view" data-er-tab={tab.id} onKeyDown={onKeyDown} tabIndex={-1}>
      <div className="row toolbar" style={{ gap: 6, flexWrap: 'wrap' }}>
        <b>{title}</b>
        {dirty && <span className="err">● {t('er.unsaved')}</span>}
        <span className="grow" />
        <AutoComplete
          size="small"
          style={{ width: 160 }}
          placeholder={t('er.search')}
          value={search}
          onChange={setSearch}
          options={searchTables(tables.map((x) => x.table), search).map((n) => ({ value: n }))}
          onSelect={(v: string) => locate(v)}
        />
        <Segmented
          size="small"
          value={display}
          onChange={(v) => {
            setDisplay(v as ErDisplay)
            markDirty()
          }}
          options={[
            { value: 'columns', label: t('er.display.columns') },
            { value: 'keys', label: t('er.display.keys') },
            { value: 'names', label: t('er.display.names') },
          ]}
        />
        <Dropdown
          menu={{
            items: [
              { key: 'LR:all', label: t('er.layout.horizontal') },
              { key: 'TB:all', label: t('er.layout.vertical') },
            ],
            onClick: ({ key }) => {
              const [d, mode] = key.split(':')
              autoLayout(d as Direction, mode === 'all')
            },
          }}
        >
          <Button size="small" type="text" icon={<IconLayoutDistributeHorizontal size={16} />}>
            {t('er.layout.title')}
          </Button>
        </Dropdown>
        <Button size="small" type="text" icon={<IconFocusCentered size={16} />} title={t('er.fit')} onClick={() => void flow.fitView({ padding: 0.1 })} />
        {data.warnings.length > 0 && (
          <Popover
            content={
              <ul className="er-warnings">
                {data.warnings.map((w) => (
                  <li key={w}>{w}</li>
                ))}
              </ul>
            }
          >
            <Badge count={data.warnings.length} size="small">
              <Button size="small" type="text" icon={<IconAlertTriangle size={16} />} />
            </Badge>
          </Popover>
        )}
        {isModel && isTopic && !readOnly && (
          <Button size="small" type="text" icon={<IconPlus size={16} />} onClick={() => setAdding(true)}>
            {t('er.addTables')}
          </Button>
        )}
        <Dropdown
          menu={{
            items: [
              { key: 'svg', label: t('er.export.svg') },
              { key: 'png', label: t('er.export.png') },
              { key: 'svg:white', label: t('er.export.svgWhite') },
              { key: 'png:white', label: t('er.export.pngWhite') },
            ],
            onClick: ({ key }) => {
              const [f, w] = key.split(':')
              void exportImage(f as ImageFormat, w === 'white')
            },
          }}
        >
          <Button size="small" type="text" loading={exporting} icon={<IconPhoto size={16} />}>
            {t('er.export.title')}
          </Button>
        </Dropdown>
        <Button size="small" type="text" icon={<IconRefresh size={16} />} title={t('er.reload')} onClick={reload} />
        {isModel ? (
          <Button size="small" type="primary" icon={<IconDeviceFloppy size={16} />} disabled={!dirty || readOnly} onClick={() => void save()}>
            {t('er.save')}
          </Button>
        ) : (
          <Button
            size="small"
            icon={<IconSchema size={16} />}
            onClick={() => useModelDialogs.getState().openReverse({ profileId: target.profile, schema })}
          >
            {t('er.saveAsModel')}
          </Button>
        )}
      </div>
      <div className="er-canvas grow">
        <ReactFlow
          nodes={nodes}
          edges={edges}
          nodeTypes={NODE_TYPES}
          onNodesChange={onNodesChange}
          onNodeDragStop={(_, __, dragged) => {
            setPositions((p) => {
              const next = { ...p }
              for (const n of dragged) next[n.id] = { x: Math.round(n.position.x), y: Math.round(n.position.y) }
              return next
            })
            markDirty()
          }}
          onNodeDoubleClick={(_, n) => !n.data.error && openTable(n.id)}
          onConnect={onConnect}
          nodesConnectable={!readOnly}
          edgesFocusable
          onEdgesDelete={(es) => es.forEach(deleteKey)}
          deleteKeyCode={readOnly ? null : 'Delete'}
          onBeforeDelete={async ({ nodes: ns, edges: es }) => ns.length === 0 && es.every((e) => e.id.startsWith('lk:'))}
          onNodeContextMenu={(e, n) => {
            e.preventDefault()
            const items = isModel
              ? [
                  { key: 'design', label: t('er.menu.design') },
                  ...(isTopic && !readOnly ? [{ key: 'remove', label: t('er.menu.remove') }] : []),
                ]
              : [
                  { key: 'data', label: t('er.menu.data') },
                  { key: 'design-db', label: t('er.menu.design') },
                ]
            setMenu({
              x: e.clientX,
              y: e.clientY,
              items,
              on: (key) => {
                if (key === 'design' || key === 'data') openTable(n.id)
                else if (key === 'design-db' && target.kind === 'database') useTabs.getState().openDesign(target.profile, { schema, table: n.id })
                else if (key === 'remove' && isTopic) {
                  setTableList((tableList as string[]).filter((x) => x !== n.id))
                  setDirty(true)
                }
              },
            })
          }}
          onEdgeContextMenu={(e, edge) => {
            e.preventDefault()
            if (!edge.id.startsWith('lk:') || readOnly) return
            setMenu({ x: e.clientX, y: e.clientY, items: [{ key: 'delete', label: t('er.key.delete'), danger: true }], on: () => deleteKey(edge) })
          }}
          onPaneContextMenu={(e) => {
            e.preventDefault()
            if (!isModel || readOnly) return
            setMenu({ x: e.clientX, y: e.clientY, items: [{ key: 'new', label: t('er.menu.newTable') }], on: newTable })
          }}
          onPaneClick={() => setMenu(null)}
          onEdgeMouseEnter={(e, edge) => setHover({ x: e.clientX, y: e.clientY, text: (edge.data as { title: string }).title })}
          onEdgeMouseLeave={() => setHover(null)}
          onlyRenderVisibleElements={!exporting}
          colorMode={theme}
          fitView
          minZoom={0.05}
          maxZoom={2}
          proOptions={{ hideAttribution: true }}
        >
          <Background gap={16} />
          <Controls showInteractive={false} />
          <MiniMap pannable zoomable />
        </ReactFlow>
        {hover && (
          <div className="er-tip" style={{ left: hover.x + 12, top: hover.y + 12 }}>
            {hover.text}
          </div>
        )}
        {menu && (
          <Dropdown
            open
            onOpenChange={(o) => !o && setMenu(null)}
            menu={{
              items: menu.items,
              onClick: ({ key }) => {
                menu.on(key)
                setMenu(null)
              },
            }}
          >
            <span style={{ position: 'fixed', left: menu.x, top: menu.y }} />
          </Dropdown>
        )}
      </div>
      {pending && (
        <KeyDialog
          pending={pending}
          onCancel={() => setPending(null)}
          onOk={async (name, comment) => {
            try {
              await api.addLogicalKey(modelId!, schema, pending.table, {
                name: name || null,
                columns: [pending.column],
                ref_table: pending.refTable,
                ref_columns: [pending.refColumn],
                comment: comment || null,
              })
              setPending(null)
              void dataQ.refetch()
            } catch (e) {
              message.error((e as Error).message)
            }
          }}
        />
      )}
      {adding && data && (
        <AddTablesDialog
          all={data.tables.map((x) => x.table)}
          current={tableList === 'all' ? [] : tableList}
          onCancel={() => setAdding(false)}
          onOk={(names) => {
            setTableList([...(tableList === 'all' ? [] : tableList), ...names])
            setDirty(true)
            setAdding(false)
          }}
        />
      )}
    </div>
  )
}

function KeyDialog({ pending, onOk, onCancel }: { pending: PendingKey; onOk(name: string, comment: string): void; onCancel(): void }) {
  const { t } = useTranslation()
  const [name, setName] = useState(`lk_${pending.table}_${pending.refTable}`)
  const [comment, setComment] = useState('')
  return (
    <Modal open title={t('er.key.title')} onCancel={onCancel} onOk={() => onOk(name.trim(), comment.trim())}>
      <p>
        {pending.table}.{pending.column} → {pending.refTable}.{pending.refColumn}
      </p>
      <Form layout="vertical">
        <Form.Item label={t('er.key.name')}>
          <Input value={name} onChange={(e) => setName(e.target.value)} />
        </Form.Item>
        <Form.Item label={t('er.key.comment')}>
          <Input value={comment} onChange={(e) => setComment(e.target.value)} />
        </Form.Item>
      </Form>
    </Modal>
  )
}

function AddTablesDialog({ all, current, onOk, onCancel }: {
  all: DesignTableModel[]
  current: string[]
  onOk(names: string[]): void
  onCancel(): void
}) {
  const { t } = useTranslation()
  const [picked, setPicked] = useState<string[]>([])
  const options = all.filter((x) => !current.includes(x.name)).map((x) => ({ value: x.name }))
  return (
    <Modal open title={t('er.addTables')} onCancel={onCancel} onOk={() => onOk(picked)} okButtonProps={{ disabled: !picked.length }}>
      <Select mode="multiple" style={{ width: '100%' }} value={picked} onChange={setPicked} options={options} showSearch />
    </Modal>
  )
}

export default function ErView({ tab }: { tab: QueryTab }) {
  return (
    <ReactFlowProvider>
      <Canvas tab={tab} />
    </ReactFlowProvider>
  )
}
```

- [ ] **Step 12: 新建 `web/src/features/er/TableNode.tsx`**

```tsx
// ER 图中的表节点（M4b 设计文档 4.2 节）：表头为表名，每列一行（主键、外键图标，列名，类型）。
// 每列左边是被引用的连接点（`列:in`），右边是引用别的表的连接点（`列:out`）；表头另有一对（只显示表名时用）。

import { IconKey, IconLink } from '@tabler/icons-react'
import { Handle, type Node, type NodeProps, Position } from '@xyflow/react'
import { memo } from 'react'

import { type ErNodeData, TABLE_HANDLE } from './graph'

export type TableNodeType = Node<ErNodeData, 'table'>

function TableNode({ data, selected }: NodeProps<TableNodeType>) {
  return (
    <div className={`er-table${selected ? ' selected' : ''}${data.error ? ' error' : ''}`} title={data.error ?? data.comment ?? undefined}>
      <div className="er-table-h">
        <Handle type="target" position={Position.Left} id={`${TABLE_HANDLE}:in`} className="er-handle" />
        <span className="er-table-name">{data.name}</span>
        <Handle type="source" position={Position.Right} id={`${TABLE_HANDLE}:out`} className="er-handle" />
      </div>
      {data.columns.map((c) => (
        <div key={c.name} className="er-col">
          <Handle type="target" position={Position.Left} id={`${c.name}:in`} className="er-handle" />
          <span className="er-col-icon">
            {c.pk ? <IconKey size={12} className="er-pk" /> : c.fk ? <IconLink size={12} className="er-fk" /> : null}
          </span>
          <span className={c.nullable ? 'er-col-name muted' : 'er-col-name'}>{c.name}</span>
          <span className="er-col-type">{c.type}</span>
          <Handle type="source" position={Position.Right} id={`${c.name}:out`} className="er-handle" />
        </div>
      ))}
      {data.external.map((x) => (
        <div key={x} className="er-external muted">
          {x}
        </div>
      ))}
    </div>
  )
}

export default memo(TableNode)
```

- [ ] **Step 13: 新建 `web/src/features/er/events.ts`**

```ts
// ER 图与设计器之间的通知：在 ER 图中点"新建表"打开设计器，设计器保存新表后通知这张图把表加进来（主题图）。

import { create } from 'zustand'

export interface CreatedTable {
  /** 模型目录。 */
  path: string
  schema: string
  name: string
  /** 等着加表的 ER 图标签。 */
  erTab: string
}

interface ErEvents {
  /** 设计器标签 id → 等着它保存新表的 ER 图标签。 */
  waiting: Record<string, string>
  created: CreatedTable | null
  wait(designTab: string, erTab: string): void
  /** 设计器保存了新表：有 ER 图在等时发出通知。 */
  tableCreated(designTab: string, path: string, schema: string, name: string): void
}

export const useErEvents = create<ErEvents>()((set, get) => ({
  waiting: {},
  created: null,
  wait: (designTab, erTab) => set({ waiting: { ...get().waiting, [designTab]: erTab } }),
  tableCreated: (designTab, path, schema, name) => {
    const erTab = get().waiting[designTab]
    if (!erTab) return
    const waiting = { ...get().waiting }
    delete waiting[designTab]
    set({ waiting, created: { path, schema, name, erTab } })
  },
}))
```

- [ ] **Step 14: 新建 `web/src/features/er/export.ts`**

```ts
// 导出 ER 图（M4b 设计文档 4.5 节）：整张图（不是当前视口）画成 SVG 或 PNG（2 倍像素）。
// 用 html-to-image 把 React Flow 的视口层画下来；桌面程序写到"另存为"选的文件，浏览器模式下载。

import { toPng, toSvg } from 'html-to-image'

export type ImageFormat = 'svg' | 'png'

export interface Bounds {
  x: number
  y: number
  width: number
  height: number
}

const PADDING = 24

/** 把视口层按整张图的范围画成 data URL。 */
export async function renderImage(viewport: HTMLElement, bounds: Bounds, format: ImageFormat, background: string): Promise<string> {
  const width = Math.ceil(bounds.width + PADDING * 2)
  const height = Math.ceil(bounds.height + PADDING * 2)
  const options = {
    backgroundColor: background,
    width,
    height,
    pixelRatio: format === 'png' ? 2 : 1,
    // 不内嵌网页字体（用系统字体），SVG 小得多
    skipFonts: true,
    style: {
      width: `${width}px`,
      height: `${height}px`,
      transform: `translate(${PADDING - bounds.x}px, ${PADDING - bounds.y}px) scale(1)`,
    },
  }
  return format === 'png' ? toPng(viewport, options) : toSvg(viewport, options)
}

/** data URL 中的内容转成 base64（SVG 的 data URL 是百分号编码的文本）。 */
export function dataUrlBase64(url: string): string {
  const comma = url.indexOf(',')
  const head = url.slice(0, comma)
  const body = url.slice(comma + 1)
  if (head.endsWith(';base64')) return body
  const bytes = new TextEncoder().encode(decodeURIComponent(body))
  let bin = ''
  for (const b of bytes) bin += String.fromCharCode(b)
  return btoa(bin)
}

/** 文件名中不能用的字符换成下划线。 */
export function imageName(name: string, format: ImageFormat): string {
  return `${name.replace(/[\\/:*?"<>|]/g, '_') || 'er'}.${format}`
}
```

- [ ] **Step 15: 新建 `web/src/features/er/graph.ts`**

```ts
// ER 图的纯逻辑（M4b 设计文档 4.2、5.3 节）：表和外键换算成节点、连线；按显示方式算节点尺寸；dagre 自动布局。
// 不引入 React Flow，单独测试。

import { graphlib, layout as dagreLayout } from '@dagrejs/dagre'

import type { DesignTableModel, ErDisplay, ErPosition, ErTable } from '../../shared/api'

export const NODE_WIDTH = 240
export const HEADER_HEIGHT = 30
export const ROW_HEIGHT = 22
export const EXTERNAL_HEIGHT = 20
/** 显示方式为"只显示表名"时连线接在表头上。 */
export const TABLE_HANDLE = '__table'

export interface ErColumn {
  name: string
  type: string
  pk: boolean
  fk: boolean
  nullable: boolean
}

export interface ErNodeData extends Record<string, unknown> {
  name: string
  comment: string | null
  /** 文件有错（只显示名字）。 */
  error: string | null
  columns: ErColumn[]
  /** 引用图外的表（其他 schema、主题图中没有的表）："→ crm.customers"。 */
  external: string[]
}

export interface ErNode {
  id: string
  data: ErNodeData
  width: number
  height: number
}

export interface ErEdge {
  id: string
  source: string
  target: string
  sourceHandle: string
  targetHandle: string
  logical: boolean
  /** 复合外键的列数（大于 1 时显示）。 */
  count: number
  /** 悬停显示：约束名和列的对应。 */
  title: string
  /** 逻辑外键在源表 `logical_keys` 中的序号（删除用）。 */
  logicalIndex: number | null
}

export interface Graph {
  nodes: ErNode[]
  edges: ErEdge[]
}

interface KeyLike {
  name?: string | null
  columns: string[]
  ref_schema?: string | null
  ref_table: string
  ref_columns: string[]
}

const out = (col: string) => `${col}:out`
const into = (col: string) => `${col}:in`

/** "只显示键列"时显示的列：主键、外键（含逻辑外键）两端的列。 */
function keyColumns(t: ErTable, all: ErTable[], schema: string): Set<string> {
  const s = new Set<string>(t.table.primary_key?.columns ?? [])
  const keys = (x: ErTable): KeyLike[] => [...x.table.foreign_keys, ...x.logical_keys]
  for (const k of keys(t)) k.columns.forEach((c) => s.add(c))
  for (const other of all) {
    for (const k of keys(other)) {
      if ((k.ref_schema ?? schema) === schema && k.ref_table === t.table.name) k.ref_columns.forEach((c) => s.add(c))
    }
  }
  return s
}

export function nodeHeight(columns: number, external: number): number {
  return HEADER_HEIGHT + columns * ROW_HEIGHT + external * EXTERNAL_HEIGHT + 6
}

/** 表和外键换算成节点和连线。`schema` 是图所在的 schema（外键不写 schema 时指它）。 */
export function buildGraph(tables: ErTable[], display: ErDisplay, schema: string): Graph {
  const names = new Set(tables.map((t) => t.table.name))
  const nodes: ErNode[] = []
  const edges: ErEdge[] = []
  for (const t of tables) {
    const tab = t.table
    const fkCols = new Set([...tab.foreign_keys, ...t.logical_keys].flatMap((k) => k.columns))
    const pk = new Set(tab.primary_key?.columns ?? [])
    const shown = display === 'names' ? new Set<string>() : display === 'keys' ? keyColumns(t, tables, schema) : null
    const columns = t.error
      ? []
      : tab.columns
          .filter((c) => !shown || shown.has(c.name))
          .map((c) => ({ name: c.name, type: c.data_type.native, pk: pk.has(c.name), fk: fkCols.has(c.name), nullable: c.nullable }))
    const external: string[] = []
    const visible = new Set(columns.map((c) => c.name))
    const handle = (col: string | undefined, dir: (c: string) => string, cols: Set<string>) =>
      col !== undefined && cols.has(col) ? dir(col) : dir(TABLE_HANDLE)
    const add = (k: KeyLike, logical: boolean, index: number) => {
      const refSchema = k.ref_schema ?? schema
      if (refSchema !== schema || !names.has(k.ref_table)) {
        const label = `→ ${refSchema !== schema ? `${refSchema}.` : ''}${k.ref_table}`
        if (!external.includes(label)) external.push(label)
        return
      }
      const target = tables.find((x) => x.table.name === k.ref_table)!
      const targetVisible = new Set(
        display === 'names'
          ? []
          : display === 'keys'
            ? [...keyColumns(target, tables, schema)]
            : target.table.columns.map((c) => c.name),
      )
      const pairs = k.columns.map((c, i) => `${c} → ${k.ref_columns[i] ?? '?'}`).join(', ')
      edges.push({
        id: `${logical ? 'lk' : 'fk'}:${tab.name}:${index}`,
        source: tab.name,
        target: k.ref_table,
        sourceHandle: handle(k.columns[0], out, visible),
        targetHandle: handle(k.ref_columns[0], into, targetVisible),
        logical,
        count: k.columns.length,
        title: k.name ? `${k.name}: ${pairs}` : pairs,
        logicalIndex: logical ? index : null,
      })
    }
    tab.foreign_keys.forEach((k, i) => add(k, false, i))
    t.logical_keys.forEach((k, i) => add(k, true, i))
    nodes.push({
      id: tab.name,
      data: { name: tab.name, comment: tab.comment, error: t.error ? t.error.message : null, columns, external },
      width: NODE_WIDTH,
      height: nodeHeight(columns.length, external.length),
    })
  }
  return { nodes, edges }
}

export type Direction = 'LR' | 'TB'

/**
 * 自动布局。`all` 为 true 时全部重排；否则保留已记的位置，没有位置的表按 dagre 的顺序排在已有部分的下方
 * （全都没有位置时直接用 dagre 的结果）。返回每张表左上角的位置。
 */
export function layout(
  graph: Graph,
  positions: Record<string, ErPosition>,
  { direction = 'LR', all = false }: { direction?: Direction; all?: boolean } = {},
): Record<string, ErPosition> {
  const g = new graphlib.Graph()
  g.setGraph({ rankdir: direction, nodesep: 40, ranksep: 80, marginx: 20, marginy: 20 })
  g.setDefaultEdgeLabel(() => ({}))
  for (const n of graph.nodes) g.setNode(n.id, { width: n.width, height: n.height })
  for (const e of graph.edges) if (e.source !== e.target) g.setEdge(e.source, e.target)
  dagreLayout(g)
  const auto: Record<string, ErPosition> = {}
  for (const n of graph.nodes) {
    const p = g.node(n.id)
    auto[n.id] = { x: Math.round(p.x - n.width / 2), y: Math.round(p.y - n.height / 2) }
  }
  const kept = all ? {} : Object.fromEntries(Object.entries(positions).filter(([k]) => auto[k]))
  const missing = graph.nodes.filter((n) => !kept[n.id])
  if (!Object.keys(kept).length) return auto
  // 已有部分的下方，按 dagre 结果的左右顺序排成一行
  const bottom = Math.max(...graph.nodes.filter((n) => kept[n.id]).map((n) => kept[n.id].y + n.height))
  const left = Math.min(...Object.values(kept).map((p) => p.x))
  const result = { ...kept }
  let x = left
  for (const n of [...missing].sort((a, b) => auto[a.id].x - auto[b.id].x || auto[a.id].y - auto[b.id].y)) {
    result[n.id] = { x, y: bottom + 80 }
    x += n.width + 40
  }
  return result
}

/** 图中的表名（搜索定位用）：包含关键字的，不区分大小写。 */
export function searchTables(tables: DesignTableModel[], q: string): string[] {
  const k = q.trim().toLowerCase()
  if (!k) return []
  return tables.map((t) => t.name).filter((n) => n.toLowerCase().includes(k))
}
```

- [ ] **Step 16: 修改 `web/src/features/model/ModelsPanel.tsx`**

```diff
--- a/web/src/features/model/ModelsPanel.tsx
+++ b/web/src/features/model/ModelsPanel.tsx
@@ -2,7 +2,7 @@
 // 工具栏打开、新建、从库反向生成、重新加载；右键新建表、用设计器打开、改名、删除、与库比对。
 // 模型目录是服务所在机器上的路径：服务不允许时（监听其他地址的浏览器模式）只显示说明。
 
-import { IconDatabaseImport, IconFolderOpen, IconPlus, IconRefresh, IconSchema, IconTable } from '@tabler/icons-react'
+import { IconAffiliate, IconDatabaseImport, IconFolderOpen, IconPlus, IconRefresh, IconSchema, IconTable } from '@tabler/icons-react'
 import { useQuery, useQueryClient } from '@tanstack/react-query'
 import { App, Dropdown, Input, type MenuProps, Tree } from 'antd'
 import type { DataNode } from 'antd/es/tree'
@@ -10,7 +10,7 @@ import { useState } from 'react'
 import { useTranslation } from 'react-i18next'
 
 import { useApi } from '../../app/context'
-import { ApiError, type ModelSummary, type ModelTableEntry } from '../../shared/api'
+import { ApiError, DEFAULT_DIAGRAM, type ModelSummary, type ModelTableEntry } from '../../shared/api'
 import { desktop } from '../../shared/shell'
 import { useTabs } from '../editor/tabs'
 import { useModelDialogs } from './dialogs'
@@ -19,12 +19,19 @@ type NodeRef =
   | { type: 'model'; model: ModelSummary }
   | { type: 'schema'; model: ModelSummary; schema: string }
   | { type: 'table'; model: ModelSummary; entry: ModelTableEntry }
+  | { type: 'diagrams'; model: ModelSummary; schema: string }
+  | { type: 'diagram'; model: ModelSummary; schema: string; name: string }
 
 function ModelTree({ model, onMenu }: { model: ModelSummary; onMenu(n: NodeRef, key: string): void }) {
   const api = useApi()
   const { t } = useTranslation()
   const { data: tables = [] } = useQuery({ queryKey: ['model-tables', model.id], queryFn: () => api.modelTables(model.id) })
   const schemas = [...new Set([...model.info.schemas, ...tables.map((x) => x.schema)])]
+  const { data: diagrams = {} } = useQuery({
+    queryKey: ['model-diagrams', model.id, schemas],
+    queryFn: async () =>
+      Object.fromEntries(await Promise.all(schemas.map(async (s) => [s, (await api.diagrams(model.id, s)).map((d) => d.name)] as const))),
+  })
   const refs = new Map<string, NodeRef>()
   const node = (key: string, ref: NodeRef, d: Omit<DataNode, 'key'>): DataNode => {
     refs.set(key, ref)
@@ -37,7 +44,18 @@ function ModelTree({ model, onMenu }: { model: ModelSummary; onMenu(n: NodeRef,
       children: schemas.map((s) =>
         node(`${model.id}/${s}`, { type: 'schema', model, schema: s }, {
           title: s,
-          children: tables
+          children: [
+            node(`${model.id}/${s}/:er`, { type: 'diagrams', model, schema: s }, {
+              title: t('er.diagrams'),
+              icon: <IconAffiliate size={14} />,
+              children: (diagrams[s] ?? [DEFAULT_DIAGRAM]).map((d) =>
+                node(`${model.id}/${s}/:er/${d}`, { type: 'diagram', model, schema: s, name: d }, {
+                  title: d === DEFAULT_DIAGRAM ? t('er.defaultDiagram') : d,
+                  isLeaf: true,
+                }),
+              ),
+            }),
+            ...tables
             .filter((x) => x.schema === s)
             .map((e) =>
               node(`${model.id}/${s}/${e.name}`, { type: 'table', model, entry: e }, {
@@ -50,6 +68,7 @@ function ModelTree({ model, onMenu }: { model: ModelSummary; onMenu(n: NodeRef,
                 isLeaf: true,
               }),
             ),
+          ],
         }),
       ),
     }),
@@ -58,6 +77,24 @@ function ModelTree({ model, onMenu }: { model: ModelSummary; onMenu(n: NodeRef,
   const items = (n: NodeRef | null): MenuProps['items'] => {
     if (!n) return []
     const ro = n.model.read_only
+    if (n.type === 'diagram') {
+      const topic = n.name !== DEFAULT_DIAGRAM
+      return [
+        { key: 'er-open', label: t('er.menu.open') },
+        ...(topic
+          ? [
+              { key: 'er-rename', label: t('model.menu.rename'), disabled: ro },
+              { key: 'er-delete', label: t('model.menu.delete'), danger: true, disabled: ro },
+            ]
+          : []),
+      ]
+    }
+    if (n.type === 'diagrams') {
+      return [
+        { key: 'er-open', label: t('er.menu.openDefault') },
+        { key: 'er-new', label: t('er.menu.newDiagram'), disabled: ro },
+      ]
+    }
     if (n.type === 'table') {
       return [
         { key: 'design', label: t('model.menu.design'), disabled: !!n.entry.error },
@@ -71,7 +108,13 @@ function ModelTree({ model, onMenu }: { model: ModelSummary; onMenu(n: NodeRef,
       { key: 'new-table', label: t('model.menu.newTable'), disabled: ro },
       { key: 'compare', label: t('model.menu.compare') },
     ]
-    if (n.type === 'schema') return common
+    if (n.type === 'schema') {
+      return [
+        ...common,
+        { key: 'er-open', label: t('er.menu.openDefault') },
+        { key: 'er-new', label: t('er.menu.newDiagram'), disabled: ro },
+      ]
+    }
     return [
       ...common,
       { type: 'divider' },
@@ -92,6 +135,7 @@ function ModelTree({ model, onMenu }: { model: ModelSummary; onMenu(n: NodeRef,
           onDoubleClick={(_, n) => {
             const r = refs.get(String(n.key))
             if (r?.type === 'table' && !r.entry.error) onMenu(r, 'design')
+            if (r?.type === 'diagram') onMenu(r, 'er-open')
           }}
         />
       </div>
@@ -120,8 +164,61 @@ export default function ModelsPanel() {
 
   const onMenu = async (n: NodeRef, key: string) => {
     const m = n.model
-    const schema = n.type === 'schema' ? n.schema : n.type === 'table' ? n.entry.schema : (m.info.schemas[0] ?? '')
+    const schema = n.type === 'table' ? n.entry.schema : n.type === 'model' ? (m.info.schemas[0] ?? '') : n.schema
+    const openEr = (name: string) =>
+      useTabs.getState().openEr(
+        { kind: 'model', path: m.path, schema, diagram: name },
+        `${schema} · ${name === DEFAULT_DIAGRAM ? t('er.defaultDiagram') : name}`,
+      )
     try {
+      if (key === 'er-open') {
+        openEr(n.type === 'diagram' ? n.name : DEFAULT_DIAGRAM)
+        return
+      }
+      if (key === 'er-new') {
+        let name = ''
+        modal.confirm({
+          title: t('er.newDiagramTitle'),
+          content: <Input autoFocus onChange={(e) => (name = e.target.value)} />,
+          onOk: async () => {
+            if (!name.trim()) return
+            await api.createDiagram(m.id, schema, name.trim(), [])
+            await qc.invalidateQueries({ queryKey: ['model-diagrams', m.id] })
+            openEr(name.trim())
+          },
+        })
+        return
+      }
+      if (key === 'er-rename' && n.type === 'diagram') {
+        let to = n.name
+        modal.confirm({
+          title: t('er.renameTitle', { name: n.name }),
+          content: <Input defaultValue={to} onChange={(e) => (to = e.target.value)} />,
+          onOk: async () => {
+            if (!to.trim() || to === n.name) return
+            await api.renameDiagram(m.id, schema, n.name, to.trim())
+            await qc.invalidateQueries({ queryKey: ['model-diagrams', m.id] })
+            // 打开着的这张图的标签跟着改名
+            for (const x of useTabs.getState().tabs) {
+              if (x.er?.kind === 'model' && x.er.path === m.path && x.er.schema === schema && x.er.diagram === n.name) {
+                useTabs.getState().setEr(x.id, { ...x.er, diagram: to.trim() }, `${schema} · ${to.trim()}`)
+              }
+            }
+          },
+        })
+        return
+      }
+      if (key === 'er-delete' && n.type === 'diagram') {
+        modal.confirm({
+          title: t('er.deleteTitle', { name: n.name }),
+          okType: 'danger',
+          onOk: async () => {
+            await api.deleteDiagram(m.id, schema, n.name)
+            await qc.invalidateQueries({ queryKey: ['model-diagrams', m.id] })
+          },
+        })
+        return
+      }
       if (key === 'design' && n.type === 'table') useTabs.getState().openDesign('', { schema, table: n.entry.name, model: m.path })
       else if (key === 'new-table') useTabs.getState().openDesign('', { schema, table: null, model: m.path })
       else if (key === 'compare') useTabs.getState().openCompare({ source: { kind: 'model', path: m.path, schema }, target: null })
```

- [ ] **Step 17: 修改 `web/src/features/tree/ObjectTree.tsx`**

```diff
--- a/web/src/features/tree/ObjectTree.tsx
+++ b/web/src/features/tree/ObjectTree.tsx
@@ -301,6 +301,7 @@ export default function ObjectTree() {
       { key: 'import-new', label: t('tree.menu.importNew') },
       { key: 'sql-file', label: t('tree.menu.sqlFile') },
       { type: 'divider' },
+      { key: 'er', label: t('tree.menu.er') },
       { key: 'compare-model', label: t('tree.menu.compareModel') },
       { key: 'save-model', label: t('tree.menu.saveAsModel') },
       { type: 'divider' },
@@ -349,7 +350,9 @@ export default function ObjectTree() {
       } else if (key === 'view-data') openData(k)
       else if (key === 'design' && k.type === 'o') useTabs.getState().openDesign(k.profileId, { schema: k.schema, table: k.name })
       else if (key === 'new-table' && k.type === 's') useTabs.getState().openDesign(k.profileId, { schema: k.schema, table: null })
-      else if (key === 'compare-model' && k.type === 's') {
+      else if (key === 'er' && k.type === 's') {
+        useTabs.getState().openEr({ kind: 'database', profile: k.profileId, schema: k.schema }, `${k.schema} · ER`)
+      } else if (key === 'compare-model' && k.type === 's') {
         useTabs.getState().openCompare({ source: null, target: { kind: 'database', profile: k.profileId, schema: k.schema } })
       } else if (key === 'save-model' && k.type === 's') useModelDialogs.getState().openReverse({ profileId: k.profileId, schema: k.schema })
       else if (key === 'import' && k.type === 'o') {
```

- [ ] **Step 18: 新建 `web/src/locales/en/er.json`**

```json
{
  "diagrams": "ER Diagrams",
  "defaultDiagram": "All Tables",
  "readOnly": "Read-only",
  "unsaved": "Unsaved",
  "saved": "ER diagram saved",
  "save": "Save",
  "reload": "Reload",
  "reloadConfirm": "Discard unsaved layout changes and reload?",
  "saveAsModel": "Save as Model…",
  "search": "Find table",
  "fit": "Fit to window",
  "addTables": "Add Tables…",
  "exported": "Exported",
  "columns_one": "{{count}} column",
  "columns_other": "{{count}} columns",
  "display": {
    "columns": "All columns",
    "keys": "Key columns",
    "names": "Names"
  },
  "layout": {
    "title": "Auto Layout",
    "horizontal": "Left to right",
    "vertical": "Top to bottom"
  },
  "export": {
    "title": "Export",
    "svg": "Export SVG",
    "png": "Export PNG",
    "svgWhite": "Export SVG (white background)",
    "pngWhite": "Export PNG (white background)"
  },
  "menu": {
    "open": "Open",
    "openDefault": "Open ER Diagram",
    "newDiagram": "New Topic Diagram…",
    "design": "Open in Designer",
    "remove": "Remove from Diagram",
    "data": "View Data",
    "newTable": "New Table"
  },
  "newDiagramTitle": "New Topic Diagram",
  "renameTitle": "Rename diagram {{name}} to",
  "deleteTitle": "Delete diagram {{name}}?",
  "key": {
    "title": "New Logical Key",
    "name": "Name",
    "comment": "Comment",
    "delete": "Delete Logical Key",
    "deleteTitle": "Delete this logical key?",
    "needColumns": "Drag from a column to a column (not available when only table names are shown)"
  },
  "warning": {
    "missingTable": "Table {{name}} no longer exists",
    "brokenKey": "Logical key {{name}} of {{table}} references a missing table or column",
    "badFile": "The file of table {{name}} is invalid: {{message}}"
  }
}
```

- [ ] **Step 19: 修改 `web/src/locales/en/tree.json`**

```diff
--- a/web/src/locales/en/tree.json
+++ b/web/src/locales/en/tree.json
@@ -24,6 +24,7 @@
   "filterPlaceholder": "Filter by name or comment",
   "menu": {
     "compareModel": "Compare with Model…",
+    "er": "View ER Diagram",
     "saveAsModel": "Save as Model…",
     "newQuery": "New Query",
     "editConnection": "Edit Connection…",
```

- [ ] **Step 20: 修改 `web/src/locales/index.ts`**

```diff
--- a/web/src/locales/index.ts
+++ b/web/src/locales/index.ts
@@ -8,6 +8,7 @@ import enDesign from './en/design.json'
 import enEditor from './en/editor.json'
 import enImport from './en/import.json'
 import enModel from './en/model.json'
+import enEr from './en/er.json'
 import enResults from './en/results.json'
 import enTree from './en/tree.json'
 import zhApp from './zh-CN/app.json'
@@ -17,6 +18,7 @@ import zhDesign from './zh-CN/design.json'
 import zhEditor from './zh-CN/editor.json'
 import zhImport from './zh-CN/import.json'
 import zhModel from './zh-CN/model.json'
+import zhEr from './zh-CN/er.json'
 import zhResults from './zh-CN/results.json'
 import zhTree from './zh-CN/tree.json'
 
@@ -30,6 +32,7 @@ export const zhCN = {
   import: zhImport,
   design: zhDesign,
   model: zhModel,
+  er: zhEr,
 }
 
 export const en = {
@@ -42,4 +45,5 @@ export const en = {
   import: enImport,
   design: enDesign,
   model: enModel,
+  er: enEr,
 }
```

- [ ] **Step 21: 新建 `web/src/locales/zh-CN/er.json`**

```json
{
  "diagrams": "ER 图",
  "defaultDiagram": "全部表",
  "readOnly": "只读",
  "unsaved": "未保存",
  "saved": "已保存 ER 图",
  "save": "保存",
  "reload": "重新加载",
  "reloadConfirm": "放弃未保存的布局改动并重新加载？",
  "saveAsModel": "另存为模型…",
  "search": "搜索表",
  "fit": "适应窗口",
  "addTables": "添加表…",
  "exported": "已导出",
  "columns": "{{count}} 列",
  "display": {
    "columns": "全部列",
    "keys": "键列",
    "names": "表名"
  },
  "layout": {
    "title": "自动布局",
    "horizontal": "横向重排",
    "vertical": "纵向重排"
  },
  "export": {
    "title": "导出",
    "svg": "导出 SVG",
    "png": "导出 PNG",
    "svgWhite": "导出 SVG（白底）",
    "pngWhite": "导出 PNG（白底）"
  },
  "menu": {
    "open": "打开",
    "openDefault": "打开 ER 图",
    "newDiagram": "新建主题图…",
    "design": "用设计器打开",
    "remove": "从图中移除",
    "data": "查看数据",
    "newTable": "新建表"
  },
  "newDiagramTitle": "新建主题图",
  "renameTitle": "图 {{name}} 改名为",
  "deleteTitle": "删除图 {{name}}？",
  "key": {
    "title": "新建逻辑外键",
    "name": "名称",
    "comment": "注释",
    "delete": "删除逻辑外键",
    "deleteTitle": "删除这个逻辑外键？",
    "needColumns": "请从列拖到列（只显示表名时不能建逻辑外键）"
  },
  "warning": {
    "missingTable": "表 {{name}} 已不存在",
    "brokenKey": "{{table}} 的逻辑外键 {{name}} 引用的表或列不存在",
    "badFile": "表 {{name}} 的文件有错：{{message}}"
  }
}
```

- [ ] **Step 22: 修改 `web/src/locales/zh-CN/tree.json`**

```diff
--- a/web/src/locales/zh-CN/tree.json
+++ b/web/src/locales/zh-CN/tree.json
@@ -24,6 +24,7 @@
   "filterPlaceholder": "按名称或注释过滤",
   "menu": {
     "compareModel": "与模型比对…",
+    "er": "查看 ER 图",
     "saveAsModel": "另存为模型…",
     "newQuery": "新建查询",
     "editConnection": "编辑连接…",
```

- [ ] **Step 23: 修改 `web/src/shared/api.ts`**

```diff
--- a/web/src/shared/api.ts
+++ b/web/src/shared/api.ts
@@ -733,6 +733,52 @@ export interface SyncPreview {
   fingerprint: string
 }
 
+/** 逻辑外键（M4b）：只在模型中记录，不参与比对和同步。 */
+export interface LogicalKey {
+  name?: string | null
+  columns: string[]
+  ref_schema?: string | null
+  ref_table: string
+  ref_columns: string[]
+  comment?: string | null
+}
+
+export type ErDisplay = 'columns' | 'keys' | 'names'
+
+export interface ErPosition {
+  x: number
+  y: number
+}
+
+/** 图文件：`tables` 为 'all'（默认图）或表名列表。 */
+export interface ErDiagram {
+  name: string
+  tables: 'all' | string[]
+  display: ErDisplay
+  positions?: Record<string, ErPosition>
+  [extra: string]: unknown
+}
+
+export interface ErTable {
+  table: DesignTableModel
+  logical_keys: LogicalKey[]
+  error?: ModelFileError | null
+}
+
+export type ErWarning =
+  | { kind: 'broken_logical_key'; table: string; index: number; name: string | null }
+  | { kind: 'missing_table'; name: string }
+
+export interface DiagramView {
+  diagram: ErDiagram
+  tables: ErTable[]
+  fingerprint: string | null
+  warnings: ErWarning[]
+}
+
+/** 默认图的名字。 */
+export const DEFAULT_DIAGRAM = '_all'
+
 export type FetchFn = (input: string, init?: RequestInit) => Promise<Response>
 
 export function createApi(token: string, fetchFn: FetchFn = (i, o) => fetch(i, o)) {
@@ -754,6 +800,8 @@ export function createApi(token: string, fetchFn: FetchFn = (i, o) => fetch(i, o
 
   const p = (id: string) => `/profiles/${encodeURIComponent(id)}`
   const m = (id: string) => `/models/${encodeURIComponent(id)}`
+  const md = (id: string, schema: string, name: string) =>
+    `${m(id)}/diagrams/${encodeURIComponent(schema)}/${encodeURIComponent(name)}`
   const mt = (id: string, schema: string, name: string) =>
     `${m(id)}/tables/${encodeURIComponent(schema)}/${encodeURIComponent(name)}`
   const s = (id: string, schema: string) => `${p(id)}/schemas/${encodeURIComponent(schema)}`
@@ -908,6 +956,25 @@ export function createApi(token: string, fetchFn: FetchFn = (i, o) => fetch(i, o
     deleteModelTable: (id: string, schema: string, name: string) => request<null>('DELETE', mt(id, schema, name)),
     renameModelTable: (id: string, schema: string, name: string, to: string) =>
       request<null>('POST', `${mt(id, schema, name)}/rename`, { to }),
+    diagrams: (id: string, schema: string) =>
+      request<{ schema: string; name: string; tables: 'all' | string[] }[]>('GET', `${m(id)}/diagrams${q({ schema })}`),
+    createDiagram: async (id: string, schema: string, name: string, tables: string[]) =>
+      (await request<{ fingerprint: string }>('POST', `${m(id)}/diagrams`, { schema, name, tables })).fingerprint,
+    diagram: (id: string, schema: string, name: string) => request<DiagramView>('GET', md(id, schema, name)),
+    /** 保存图，返回新的指纹。 */
+    saveDiagram: async (id: string, schema: string, name: string, diagram: ErDiagram, fingerprint: string | null) =>
+      (await request<{ fingerprint: string }>('PUT', md(id, schema, name), { diagram, fingerprint })).fingerprint,
+    deleteDiagram: (id: string, schema: string, name: string) => request<null>('DELETE', md(id, schema, name)),
+    renameDiagram: (id: string, schema: string, name: string, to: string) =>
+      request<null>('POST', `${md(id, schema, name)}/rename`, { to }),
+    addLogicalKey: (id: string, schema: string, table: string, key: LogicalKey) =>
+      request<{ fingerprint: string }>('POST', `${mt(id, schema, table)}/logical-keys`, { key }),
+    deleteLogicalKey: (id: string, schema: string, table: string, index: number) =>
+      request<{ fingerprint: string }>('DELETE', `${mt(id, schema, table)}/logical-keys/${index}`),
+    /** 桌面版：把导出的图片（base64）写到"另存为"选的 .svg、.png 文件。 */
+    saveImage: (path: string, data: string) => request<null>('POST', '/files/image', { path, data }),
+    /** 整个 schema 的表明细（库的 ER 图）。 */
+    schemaTables: (id: string, schema: string) => request<DesignTableModel[]>('GET', `${s(id, schema)}/tables`),
     compare: (req: CompareRequest) => request<CompareResult>('POST', '/compare', req),
     syncPreview: (req: SyncRequest) => request<SyncPreview>('POST', '/compare/preview', req),
     /** 同步：目标是库时返回任务 id；目标是模型时已经写完，返回 null。 */
```

- [ ] **Step 24: 检查**

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

Expected: 全部通过（前端新增 7 个测试）。

- [ ] **Step 25: 浏览器实测**

浏览器实测（设计文档第 6 节）：用 `dblens-server --listen 127.0.0.1:<端口>` 启动浏览器模式。
1. 模型面板 schema 下"ER 图 → 全部表"：自动布局、列级连线、复合外键标"N 列"、自引用、其他 schema 的外键列在节点底部；悬停连线显示约束名和列的对应。
2. 从一列拖到另一张表的列：弹框后建成逻辑外键（虚线）；选中虚线按 Delete 删除。
3. 拖动表后标题出现"未保存"，Ctrl+S 保存，核对 `diagrams/<schema>/_all.yaml`；重新打开位置不变。
4. 新建主题图，"添加表…"、节点右键"从图中移除"；右键画布"新建表"，设计器保存后表出现在这张主题图中。
5. 双击表打开设计器，改列名保存后回到图上，连线和列名随之更新。
6. 导出 SVG、PNG（含白底），打开核对是整张图。
7. 对象树 schema 右键"查看 ER 图"：只读、可拖动，"另存为模型…"打开反向生成对话框并带上连接和 schema。
8. 约 200 张表的模型（如由接口生成）打开默认图，拖动、缩放流畅。

- [ ] **Step 26: 提交**

```bash
git add scripts/check-deps.mjs web/package.json web/pnpm-lock.yaml web/src/app/StatusBar.tsx web/src/app/shell.css web/src/features/design/DesignView.tsx web/src/features/editor/Workspace.tsx web/src/features/editor/actions.tsx web/src/features/editor/tabs.test.ts web/src/features/editor/tabs.ts web/src/features/er/ErView.tsx web/src/features/er/TableNode.tsx web/src/features/er/events.ts web/src/features/er/export.ts web/src/features/er/graph.test.ts web/src/features/er/graph.ts web/src/features/model/ModelsPanel.tsx web/src/features/tree/ObjectTree.tsx web/src/locales/en/er.json web/src/locales/en/tree.json web/src/locales/index.ts web/src/locales/zh-CN/er.json web/src/locales/zh-CN/tree.json web/src/shared/api.ts
git commit -m "feat(web): ER 图标签（React Flow + dagre），模型面板中的图，导出 SVG / PNG" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 7: 文档

- `docs/notes/m4b-er-diagram.md`：结构、文件格式、与设计文档不一致的地方、验证。
- `docs/notes/m0-4b-api.md`：ER 图与逻辑外键的接口。
- 总设计文档：M4b 完成。

**Files:**
- Modify: `docs/notes/m0-4b-api.md`
- Create: `docs/notes/m4b-er-diagram.md`
- Modify: `docs/superpowers/specs/2026-10-22-dblens-design.md`

- [ ] **Step 1: 修改 `docs/notes/m0-4b-api.md`**

```diff
--- a/docs/notes/m0-4b-api.md
+++ b/docs/notes/m0-4b-api.md
@@ -244,6 +244,27 @@
 - `script`（目标是库）：`{steps: [{kind: "sql", sql, items} | {kind: "rebuild", table, rebuild, items}], refused: [[表, 原因]], warnings, transactional}`；
   `writes`（目标是模型）：`[{name, from?, delete}]`。
 
+### ER 图与逻辑外键（M4b 加入）
+
+同样只在页面和服务在同一台机器上时可用，否则 403。
+
+| 方法 | 路径 | 说明 |
+|---|---|---|
+| GET | `/models/{id}/diagrams?schema=` | 图列表 `[{schema, name, tables}]`，默认图 `_all` 在前（没有文件时也列出）；`tables` 为 `"all"` 或表名数组 |
+| POST | `/models/{id}/diagrams` | 新建主题图 `{schema, name, tables}`，201 和 `{fingerprint}`；名字不能为空或 `_all` |
+| GET | `/models/{id}/diagrams/{schema}/{name}` | `{diagram, tables: [{table, logical_keys, error?}], fingerprint, warnings}`；默认图没有文件时 `fingerprint` 为 null |
+| PUT | `/models/{id}/diagrams/{schema}/{name}` | 保存 `{diagram, fingerprint}`，返回 `{fingerprint}`；文件已被修改时 400 |
+| DELETE | `/models/{id}/diagrams/{schema}/{name}` | 删除主题图（默认图不能删） |
+| POST | `/models/{id}/diagrams/{schema}/{name}/rename` | 主题图改名 `{to}` |
+| POST | `/models/{id}/tables/{schema}/{name}/logical-keys` | 加逻辑外键 `{key, fingerprint?}`，返回表文件的 `{fingerprint}` |
+| DELETE | `/models/{id}/tables/{schema}/{name}/logical-keys/{序号}?fingerprint=` | 删除逻辑外键（序号从 0 开始） |
+| POST | `/files/image` | `{path, data}`（`data` 为 base64），把导出的 ER 图写到 `.svg` 或 `.png` 文件 |
+
+- `diagram`：`{name, tables, display, positions}`，`display` 为 `columns` / `keys` / `names`，`positions` 为表名 → `{x, y}`（整数）。
+- `key`：`{name?, columns, ref_schema?, ref_table, ref_columns, comment?}`；两边的列都要存在、列数一致，名字不能与本表的外键、逻辑外键重复。
+- `warnings`：`[{kind: "missing_table", name} | {kind: "broken_logical_key", table, index, name}]`。
+- 库的 ER 图用 `GET /profiles/{id}/schemas/{schema}/tables`（不给 `table` 时返回整个 schema 的表明细）。
+
 ## WebSocket：`GET /api/sessions/{id}/ws?token=…&lang=…`
 
 `lang` 可选，决定事件中文字的语言（见"语言"一节）。
```

- [ ] **Step 2: 新建 `docs/notes/m4b-er-diagram.md`**

````markdown
# M4b ER 图

依据：设计文档 `docs/superpowers/specs/2026-10-28-m4b-er-diagram-design.md`。

## 结构

```text
crates/dblens-service/src/model/
  file.rs      LogicalKey；表文件的 logical_keys；ModelTable::new
  diagram.rs   图文件（Diagram、DiagramTables、DiagramDisplay、Position）；图的列表、读（表、逻辑外键、提示）、保存、新建、改名、删除；
               表改名、删除时更新各图（on_table_changed）
  store.rs     逻辑外键的增删；表、列改名时更新逻辑外键的引用；表改名、删除时调用 diagram::on_table_changed
crates/dblens-api/src/diagrams.rs   /models/{id}/diagrams…、…/logical-keys…、/files/image
web/src/features/er/
  graph.ts       节点和连线、节点尺寸、dagre 自动布局、搜索
  TableNode.tsx  表节点（每列左右各一个连接点）
  ErView.tsx     ER 图标签：工具栏、画布、建删逻辑外键、主题图的表、保存、导出
  export.ts      html-to-image 画整张图、data URL 转 base64
  events.ts      ER 图中"新建表"打开的设计器保存后，通知这张图把表加进来
```

入口：模型面板中 schema 下的"ER 图"节点（双击图打开；右键打开、改名、删除）、schema 右键"打开 ER 图""新建主题图…"；
对象树 schema 右键"查看 ER 图"（库图，只读）。

## 文件格式

- 图文件 `diagrams/<schema>/<图名>.yaml`：`name`、`tables`（`all` 或表名列表）、`display`（`columns` / `keys` / `names`）、
  `positions`（表名 → `{x, y}`，整数）；不认识的字段保留。默认图名为 `_all`。
- 表文件的 `logical_keys`：`name`（可省）、`columns`、`ref_schema`（可省）、`ref_table`、`ref_columns`、`comment`（可省），写在 `foreign_keys` 之后。

## 与设计文档不一致的地方

- `positions` 用块写法（每个坐标一行），`y` 写成 `"y"`：YAML 1.1 中 `y` 会被当作布尔值，serde-saphyr 自动加引号。
- 在图上拖线只建单列的逻辑外键；复合的逻辑外键在表文件中手写。
- "自动布局"菜单只有横向、纵向的全部重排；没有位置的表在打开、加表时自动排在已有部分的下方（不单设"只排新表"的菜单项）。
- 导出的"白底"只换画布背景，表节点仍按当前主题的颜色；导出不内嵌网页字体（`skipFonts`），SVG 用系统字体显示。
- `html-to-image` 固定在 1.11.11：依赖复查脚本（`scripts/check-deps.mjs`）加了"有意不升级"的名单 `HOLD`，复查时列出原因、不算可升级。
- 连线的悬停提示（约束名和列的对应）用自绘的浮层，不用 React Flow 的标签。
- 库图的标签绑定连接（标签带环境色）；模型图的标签不绑定连接。

## 验证

- 后端：fmt、clippy 通过；全部测试在 `DBLENS_LANG=en` 下通过（688 个）。
  - service：逻辑外键的校验、设计器保存后保留、表和列改名时引用跟着改、删除；图文件的写法（快照）、默认图没有文件、指纹冲突、手写字段保留；
    主题图的新建、改名、删除，表改名、删除时图跟着改；缺表和引用失效的逻辑外键进提示；文件有错的表只有名字。
  - api：没有 `local_files` 时禁止；图和逻辑外键的完整流程；导出图片只写 `.svg`、`.png`。
  - 实库：模型测试中同步之后再反向生成，核对默认图中外键的列对应（见下）。
- 前端：tsc、Vitest（节点和连线：复合外键、自引用、跨 schema、主题图外的表、逻辑外键；显示方式；布局；搜索；导出的 base64 和文件名；ER 图标签的保存与恢复）、打包通过。
- 浏览器实测（模型由接口建立）：模型面板的"ER 图"节点；默认图的自动布局、列级连线、复合外键标"2 列"、自引用、跨 schema 列在节点上；
  从列拖到列建逻辑外键（虚线）；拖动表后"未保存"标记、Ctrl+S 保存、图文件内容核对；导出 PNG（整张图）。
````

- [ ] **Step 3: 修改 `docs/superpowers/specs/2026-10-22-dblens-design.md`**

```diff
--- a/docs/superpowers/specs/2026-10-22-dblens-design.md
+++ b/docs/superpowers/specs/2026-10-22-dblens-design.md
@@ -332,6 +332,7 @@ trait Policy: Send + Sync {
 - 国产库第一批已完成：TiDB、OceanBase 社区版（MySQL 模式）、openGauss（A、PG 兼容模式，含 SHA256 认证）、人大金仓（Oracle、PG 模式）（设计 `docs/superpowers/specs/2026-10-27-domestic-batch1-design.md`，详细计划 `docs/superpowers/plans/2026-10-27-domestic-batch1.md`，说明 `docs/notes/domestic-batch1.md`）。第二批（PolarDB、TDSQL、GaussDB，只有云服务）、第三批（GBase 8s、OceanBase 的 Oracle 模式）尚未开始；GoldenDB 没有可用的测试环境。
 - 2026-10-27 M4 拆成三份（用户确认）：M4a 模型与比对（模型文件、反向生成、比对与同步）；M4b ER 图；M4c 数据字典和设计文档导出。
 - M4a 已完成（设计 `docs/superpowers/specs/2026-10-27-m4a-model-compare-design.md`，详细计划 `docs/superpowers/plans/2026-10-27-m4a-model-compare.md`，说明 `docs/notes/m4a-model-compare.md`）。M4b、M4c 尚未开始。
+- M4b 已完成（设计 `docs/superpowers/specs/2026-10-28-m4b-er-diagram-design.md`，详细计划 `docs/superpowers/plans/2026-10-28-m4b-er-diagram.md`，说明 `docs/notes/m4b-er-diagram.md`）。M4c 尚未开始。
 
 ## 12. 风险与待验证项
 
```

- [ ] **Step 4: 检查**

检查文档中的路径和接口与代码一致；按执行时的实际结果更新"验证"一节（测试数、实库与浏览器实测的内容）。

Expected: 无。

- [ ] **Step 5: 提交**

```bash
git add docs/notes/m0-4b-api.md docs/notes/m4b-er-diagram.md docs/superpowers/specs/2026-10-22-dblens-design.md
git commit -m "docs: M4b ER 图的说明、接口和进度" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```
