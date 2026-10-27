# M3 表设计器 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 表设计器：新建表和修改已有表（列、主键、唯一约束、索引、外键、检查约束、注释，分区只读），后端比对出变更列表并按方言生成 DDL，库不支持的改动生成重建表的脚本，预览确认后作为任务执行。

**Architecture:** 前端编辑一份草稿表（每一行记下原名），发送完整的草稿和"新名 → 原名"的映射；`dblens-dialect` 校验草稿、比对出变更列表（与方言无关，M4 复用）、按方言生成 DDL 或重建脚本；`dblens-service` 读原表（不走缓存）、预览（含执行策略的结论、指纹冲突）、在单独的连接上作为任务执行；前端是独立的设计器标签，子标签分区，保存前弹出确认框。

**Tech Stack:** Rust（axum、tokio、minijinja、insta）；React 19 + TypeScript、antd 6、zustand、@tanstack/react-query 5、Monaco、Vitest。

**Spec:** `docs/superpowers/specs/2026-10-27-m3-table-designer-design.md`

## Global Constraints

- 执行时后端用草稿重新生成语句，不接受前端传来的 SQL；执行前核对原表的指纹，被别人改过时拒绝。
- 每条语句过执行策略：拒绝的不能执行；生产环境在确认框中醒目提示。
- PG 家族整体一个事务；其他库的 DDL 自动提交，失败时列出没有执行的语句。重建从删除旧表起不能取消，之前失败或取消时删除新表。
- 依赖只用正式发布版，跟随最新；新版发布满 7 天再升级。本里程碑不新增依赖。
- 前端给用户看的文字都用词条（中英文），英文区分单复数；后端错误文字用 `msg!` 词条；日志不翻译。
- 代码注释用中文；文件 UTF-8、LF 换行；前端代码风格：无分号、单引号、两格缩进。
- cargo 命令加 `--config "source.mirror.registry='sparse+https://rsproxy.cn/index/'"`（clippy 写在 `clippy` 之后），并设 `CARGO_INCREMENTAL=0`；后端测试在 `DBLENS_LANG=en` 下运行。
- 前端工具用 PowerShell 执行（`npm`/`npx` 在本机不可用，用 `pnpm exec`）。
- 测试数据库的密码只在命令行中以内联环境变量传入，不写进仓库、报告或提交信息。

## 与设计文档的差异（写计划时实测后调整）

- `Renames` 只有列和索引：约束只改名也按先删后建处理。
- 库不支持的改动由 `Dialect::design_unsupported(&Diff)` 判断；`Diff` 另有 `appended`。
- MySQL 家族每项改动一条 `ALTER TABLE`（同一条中改名与 MODIFY、AFTER 的引用规则各版本不一）；换主键时删除和新增放在同一条；既修改又挪动的列只 MODIFY 一次。
- 生成的语句带对应的变更序号列表；变更的可读摘要由前端生成。
- 没有"只读连接"：预览中标出被策略拒绝的语句和需要确认的原因。
- 不能指定列位置的库，"新增列"加在最后。
- 索引方法不写与默认方法（BTREE、NORMAL）视为相同。
- Oracle 的触发器用 `trigger_ddl` 拼出完整 DDL；执行前切换到表所在的 schema。达梦的授权视图用 `OWNER` 列。
- 改动标记按行显示，没有逐个单元格标黄。
- 标题栏"文件 → 新建表"的 schema 用当前标签的，否则在"选项"中选。

## 执行说明

任务 2–11 的代码都已在临时副本中逐任务实现并通过检查（含 5 个实库；浏览器实测覆盖了中英文界面、PG 和 MySQL 上新建表、改名加挪动列触发重建、未保存时关闭的提示、数据标签的入口、降序索引和检查约束、Oracle 生产环境的确认框、在新查询标签中打开），执行时按顺序重放。新文件给出全文，修改的文件给出 diff（基于上一个任务完成后的状态，可直接 `git apply`）。`Cargo.lock` 和 `web/pnpm-lock.yaml` 不变。

## 文件结构

```text
crates/dblens-dialect/src/design/{mod.rs, diff.rs, catalog.rs, sql.rs, mysql.rs, postgres.rs, oracle.rs, rebuild.rs}; snapshots/   变更模型、比对、DDL、重建（新增）
crates/dblens-dialect/src/{lib.rs, meta/mod.rs, meta/assemble.rs}; sql/*.sql; tests/{meta_snapshots.rs, snapshots/}   新的元数据查询
crates/dblens-core/locales/app.json                                  后端词条
crates/dblens-service/src/{design.rs, task.rs, metadata.rs, fake.rs, lib.rs}; tests/live.rs   读表、预览、执行任务（新增 design.rs）
crates/dblens-api/src/{design.rs, lib.rs}; tests/api.rs               接口（新增 design.rs）
web/src/shared/api.ts                                                接口客户端
web/src/features/design/                                             设计器（新增）
web/src/features/{editor/{tabs.ts, Workspace.tsx, actions.tsx}, tree/ObjectTree.tsx, data/DataView.tsx}; app/{menus.ts, TitleBar.tsx, shell.css}   标签与入口
web/src/locales/{zh-CN,en}/{design.json, app.json, tree.json, editor.json}, locales/index.ts   词条
docs/notes/{m3-table-designer.md, m0-4b-api.md}; docs/superpowers/specs/2026-10-22-dblens-design.md
```

---

### Task 1: 依赖复查

按依赖原则（M1b2 设计文档 3.1 节）复查前端和后端的全部直接依赖。本里程碑不新增依赖。

**Files:** 视复查结果而定（`web/package.json`、`Cargo.toml` 及锁文件）。

- [ ] **Step 1: 运行复查脚本**

本机经代理上网，Node 的 `fetch` 默认不读 `HTTPS_PROXY`，必须先设 `NODE_USE_ENV_PROXY=1`，否则全部查询失败，最后仍显示"0 个可升级"。

```powershell
$env:PATH = "D:\Environments\nvm4w\nodejs;$env:PATH"; $env:NODE_USE_ENV_PROXY = '1'; node scripts/check-deps.mjs
```

Expected：输出中没有"查询失败"。2026-10-27 写计划时：共 80 个直接依赖，0 个可升级（9 个有新版但发布未满 7 天）。以执行当天的结果为准。

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

### Task 2: 方言：变更模型、校验与比对

新增 `dblens-dialect/src/design/`（`lib.rs` 中 `pub mod design`），不做 IO：
- `Renames { columns, indexes }`：草稿中改了名的列和索引，"新名 → 原名"；约束只改名也按先删后建，不需要映射。
- `TableChange`：`CreateTable`、`SetTableComment`、`AddColumn { column, after }`、`DropColumn`、`RenameColumn { from, to }`、`AlterColumn { from, to }`（`from` 的名字已换成新名）、`MoveColumn { column, after }`、主键 / 唯一 / 检查 / 外键 / 索引的 `Drop*` / `Add*`、`RenameIndex`。任务 4 给 `RenameColumn` 加 `column`、`RenameIndex` 改为带 `index`（旧版 MySQL 要用完整定义）。
- `validate(&Table) -> Vec<DesignIssue>`：空名、重名（列、索引、约束各自）、空类型、引用不存在的列或重复的列、外键两边列数、空的检查表达式；错误带 `section`、行号、字段。索引列含括号或空格的视为表达式，不检查。
- `diff_table(original, draft, renames) -> Result<Diff>`：`original` 为 None 时只有 `CreateTable`；列按改名映射配对，类型比较不区分大小写和多余空白，默认值、注释按去空白的原文比较；约束按名字配对（没有名字的按内容），内容不同先删后建；索引按（经改名换算的）原名配对，只改了名字时 `RenameIndex`；改了名的列，引用它的约束和索引按新名重写不算改动。顺序：删外键 → 删其他约束和索引 → 列（删除、改名、修改、新增、挪动）→ 建主键、唯一、检查、索引 → 索引改名 → 建外键 → 表注释。挪动按"保留的列保持原顺序、新列插到草稿位置"排出当前顺序，再逐个挪到草稿中的位置；`appended` 表示新列都在最后、保留的列相对顺序不变。
- `Dialect::prepare_draft(&mut Table) -> Vec<Msg>`：按原生类型重新解析逻辑类型，主键列一律 NOT NULL（返回提示），重排序号。

后端词条加 `design.*`（12 个）。

**Files:**
- Modify: `crates/dblens-core/locales/app.json`
- Create: `crates/dblens-dialect/src/design/diff.rs`
- Create: `crates/dblens-dialect/src/design/mod.rs`
- Modify: `crates/dblens-dialect/src/lib.rs`

**Interfaces:**
- Produces（`dblens_dialect::design`）：
  - `Renames { columns: BTreeMap<String, String>, indexes: BTreeMap<String, String> }`（Deserialize，字段可省略）；
  - `TableChange`（Serialize，`#[serde(tag = "kind", rename_all = "snake_case")]`）；`Diff { changes: Vec<TableChange>, appended: bool }`；
  - `IssueSection::{Table, Columns, PrimaryKey, Indexes, ForeignKeys, Uniques, Checks}`；`DesignIssue { section, row: Option<usize>, field: Option<&'static str>, message: Msg }`；
  - `validate(&Table) -> Vec<DesignIssue>`、`diff_table(Option<&Table>, &Table, &Renames) -> Result<Diff>`、`Dialect::prepare_draft(&self, &mut Table) -> Vec<Msg>`；
  - crate 内：`diff::{is_expression, same_column, text, same_type}`（任务 4 起用到 `text`、`same_type`）。

- [ ] **Step 1: 修改 `crates/dblens-core/locales/app.json`**

```diff
--- a/crates/dblens-core/locales/app.json
+++ b/crates/dblens-core/locales/app.json
@@ -19,6 +19,18 @@
   "changes.unknown_column": {"zh-CN": "列 %{column} 不存在", "en": "Column %{column} does not exist"},
   "ddl.truncated": {"zh-CN": "DDL 超过 64 KB，以上只是开头部分", "en": "The DDL exceeds 64 KB; only the beginning is shown above"},
   "ddl.unsupported": {"zh-CN": "该数据库不支持查看 %{kind} 的 DDL", "en": "This database does not support viewing the DDL of %{kind}"},
+  "design.bad_rename": {"zh-CN": "列 %{name} 的原名不对", "en": "Column %{name} has an invalid original name"},
+  "design.columns_empty": {"zh-CN": "至少要选一列", "en": "Select at least one column"},
+  "design.duplicate": {"zh-CN": "名称 %{name} 重复", "en": "Duplicate name %{name}"},
+  "design.duplicate_column": {"zh-CN": "列 %{column} 重复", "en": "Column %{column} is listed twice"},
+  "design.expression_empty": {"zh-CN": "表达式不能为空", "en": "Expression is required"},
+  "design.name_empty": {"zh-CN": "名称不能为空", "en": "Name is required"},
+  "design.no_columns": {"zh-CN": "表至少要有一列", "en": "A table needs at least one column"},
+  "design.pk_not_null": {"zh-CN": "主键列 %{column} 将设为 NOT NULL", "en": "Primary key column %{column} will be set to NOT NULL"},
+  "design.ref_columns_count": {"zh-CN": "引用列的个数要与外键列相同", "en": "The referenced columns must match the foreign key columns in number"},
+  "design.ref_table_empty": {"zh-CN": "请选择引用的表", "en": "Select the referenced table"},
+  "design.type_empty": {"zh-CN": "类型不能为空", "en": "Type is required"},
+  "design.unknown_column": {"zh-CN": "列 %{column} 不存在", "en": "Column %{column} does not exist"},
   "desktop.bind_failed": {"zh-CN": "无法监听本机端口：%{error}", "en": "Cannot listen on a local port: %{error}"},
   "desktop.log_dir_failed": {"zh-CN": "无法创建日志目录 %{dir}：%{error}", "en": "Cannot create the log directory %{dir}: %{error}"},
   "desktop.no_main_window": {"zh-CN": "主窗口不存在", "en": "The main window does not exist"},
```

- [ ] **Step 2: 新建 `crates/dblens-dialect/src/design/diff.rs`**

```rust
//! 草稿的校验与比对（M3 设计文档第 3 节）。与方言无关：哪些变更库不支持由方言另行判断。

use std::collections::{BTreeSet, HashMap};

use dblens_core::schema::{
    CheckConstraint, Column, ForeignKey, Index, IndexColumn, PrimaryKey, Table, UniqueConstraint,
};
use dblens_core::{Error, Result, msg};

use super::{DesignIssue, Diff, IssueSection as S, Renames, TableChange as C};

/// 索引列是不是表达式（如 `lower(name)`）：表达式不检查列是否存在，也不随列改名。
pub(crate) fn is_expression(name: &str) -> bool {
    name.contains('(') || name.contains(' ')
}

/// 校验草稿；返回全部错误（空表示可以生成 DDL）。
pub fn validate(draft: &Table) -> Vec<DesignIssue> {
    let mut out = Vec::new();
    let mut push = |section, row, field, message| out.push(DesignIssue::new(section, row, field, message));
    if draft.name.trim().is_empty() {
        push(S::Table, None, Some("name"), msg!("design.name_empty"));
    }
    if draft.columns.is_empty() {
        push(S::Columns, None, None, msg!("design.no_columns"));
    }
    let mut seen = BTreeSet::new();
    for (i, c) in draft.columns.iter().enumerate() {
        if c.name.trim().is_empty() {
            push(S::Columns, Some(i), Some("name"), msg!("design.name_empty"));
        } else if !seen.insert(c.name.as_str()) {
            push(S::Columns, Some(i), Some("name"), msg!("design.duplicate", name = c.name));
        }
        if c.data_type.native.trim().is_empty() {
            push(S::Columns, Some(i), Some("type"), msg!("design.type_empty"));
        }
    }
    let has = |n: &str| draft.columns.iter().any(|c| c.name == n);
    let mut columns = |section, row, cols: &[String], expressions: bool| {
        if cols.is_empty() {
            push(section, row, Some("columns"), msg!("design.columns_empty"));
        }
        let mut seen = BTreeSet::new();
        for c in cols {
            if expressions && is_expression(c) {
                continue;
            }
            if !has(c) {
                push(section, row, Some("columns"), msg!("design.unknown_column", column = c));
            } else if !seen.insert(c.as_str()) {
                push(section, row, Some("columns"), msg!("design.duplicate_column", column = c));
            }
        }
    };
    if let Some(pk) = &draft.primary_key {
        columns(S::PrimaryKey, None, &pk.columns, false);
    }
    for (i, u) in draft.uniques.iter().enumerate() {
        columns(S::Uniques, Some(i), &u.columns, false);
    }
    for (i, f) in draft.foreign_keys.iter().enumerate() {
        columns(S::ForeignKeys, Some(i), &f.columns, false);
    }
    for (i, x) in draft.indexes.iter().enumerate() {
        let names: Vec<String> = x.columns.iter().map(|c| c.name.clone()).collect();
        columns(S::Indexes, Some(i), &names, true);
    }
    for (i, f) in draft.foreign_keys.iter().enumerate() {
        if f.ref_table.trim().is_empty() {
            push(S::ForeignKeys, Some(i), Some("ref_table"), msg!("design.ref_table_empty"));
        } else if f.ref_columns.is_empty() || f.ref_columns.len() != f.columns.len() {
            push(S::ForeignKeys, Some(i), Some("ref_columns"), msg!("design.ref_columns_count"));
        }
    }
    for (i, c) in draft.checks.iter().enumerate() {
        if c.expression.trim().is_empty() {
            push(S::Checks, Some(i), Some("expression"), msg!("design.expression_empty"));
        }
    }
    let mut indexes = BTreeSet::new();
    for (i, x) in draft.indexes.iter().enumerate() {
        if x.name.trim().is_empty() {
            push(S::Indexes, Some(i), Some("name"), msg!("design.name_empty"));
        } else if !indexes.insert(x.name.as_str()) {
            push(S::Indexes, Some(i), Some("name"), msg!("design.duplicate", name = x.name));
        }
    }
    // 约束名在表内不能重复（未命名的由数据库取名）
    let mut names = BTreeSet::new();
    let named = [(S::PrimaryKey, None, draft.primary_key.as_ref().and_then(|k| k.name.as_deref()))]
        .into_iter()
        .chain(draft.uniques.iter().enumerate().map(|(i, u)| (S::Uniques, Some(i), u.name.as_deref())))
        .chain(
            draft.foreign_keys.iter().enumerate().map(|(i, f)| (S::ForeignKeys, Some(i), f.name.as_deref())),
        )
        .chain(draft.checks.iter().enumerate().map(|(i, c)| (S::Checks, Some(i), c.name.as_deref())));
    for (section, row, name) in named {
        if let Some(n) = name.filter(|n| !n.trim().is_empty())
            && !names.insert(n)
        {
            push(section, row, Some("name"), msg!("design.duplicate", name = n));
        }
    }
    out
}

/// 去掉首尾空白，空串当作没有。
fn text(v: &Option<String>) -> Option<&str> {
    v.as_deref().map(str::trim).filter(|s| !s.is_empty())
}

/// 类型文本比较：不区分大小写，忽略多余空白和括号、逗号两侧的空白。
fn same_type(a: &str, b: &str) -> bool {
    let norm = |s: &str| {
        let s = s.split_whitespace().collect::<Vec<_>>().join(" ").to_ascii_uppercase();
        s.replace(" (", "(").replace("( ", "(").replace(" )", ")").replace(", ", ",").replace(" ,", ",")
    };
    norm(a) == norm(b)
}

/// 列定义是否相同（不比较名字和序号）。
pub(crate) fn same_column(a: &Column, b: &Column) -> bool {
    same_type(&a.data_type.native, &b.data_type.native)
        && a.nullable == b.nullable
        && text(&a.default) == text(&b.default)
        && text(&a.comment) == text(&b.comment)
        && a.auto_increment == b.auto_increment
        && text(&a.generated) == text(&b.generated)
}

/// 外键动作：NO ACTION 与不写相同。
fn action(a: &Option<String>) -> Option<String> {
    text(a).map(|s| s.to_ascii_uppercase()).filter(|s| s != "NO ACTION")
}

/// 两项按名字配对；草稿中没有名字的项按内容找原表中同样没有名字的项。
/// 返回（要删除的原有项，要新增的草稿项）：内容不同的一对两边都出现。
fn pair<'a, T>(
    orig: &'a [T],
    draft: &'a [T],
    name: impl Fn(&T) -> Option<&str>,
    same: impl Fn(&T, &T) -> bool,
) -> (Vec<&'a T>, Vec<&'a T>) {
    let mut used = vec![false; orig.len()];
    let mut added = Vec::new();
    let mut dropped = Vec::new();
    for d in draft {
        let found = orig.iter().enumerate().position(|(i, o)| {
            !used[i]
                && match name(d) {
                    Some(n) => name(o) == Some(n),
                    None => name(o).is_none() && same(o, d),
                }
        });
        match found {
            Some(i) => {
                used[i] = true;
                if !same(&orig[i], d) {
                    dropped.push(&orig[i]);
                    added.push(d);
                }
            }
            None => added.push(d),
        }
    }
    dropped.extend(orig.iter().enumerate().filter(|(i, _)| !used[*i]).map(|(_, o)| o));
    (dropped, added)
}

/// 比对原表与草稿。`original` 为 None 表示新建表。草稿应已校验过（`validate`）。
/// `renames` 中的名字对不上时返回 `invalid`。
pub fn diff_table(original: Option<&Table>, draft: &Table, renames: &Renames) -> Result<Diff> {
    let Some(orig) = original else {
        return Ok(Diff { changes: vec![C::CreateTable { table: draft.clone() }], appended: true });
    };
    // 原名 → 新名（保留下来的列）
    let mut new_of: HashMap<&str, &str> = HashMap::new();
    for c in &draft.columns {
        let old = renames.columns.get(&c.name).map_or(c.name.as_str(), String::as_str);
        if renames.columns.contains_key(&c.name) && !orig.columns.iter().any(|o| o.name == old) {
            return Err(Error::invalid(msg!("design.bad_rename", name = c.name)));
        }
        if orig.columns.iter().any(|o| o.name == old) && new_of.insert(old, &c.name).is_some() {
            return Err(Error::invalid(msg!("design.bad_rename", name = c.name)));
        }
    }
    let map = |n: &String| new_of.get(n.as_str()).map_or(n.clone(), |s| s.to_string());
    let map_all = |cols: &[String]| cols.iter().map(map).collect::<Vec<_>>();

    let mut drops = Vec::new();
    let mut cols = Vec::new();
    let mut adds = Vec::new();

    // 列：删除、改名、修改、新增
    for o in &orig.columns {
        if !new_of.contains_key(o.name.as_str()) {
            cols.push(C::DropColumn { name: o.name.clone() });
        }
    }
    for o in &orig.columns {
        if let Some(&n) = new_of.get(o.name.as_str())
            && n != o.name
        {
            cols.push(C::RenameColumn { from: o.name.clone(), to: n.to_string() });
        }
    }
    for o in &orig.columns {
        let Some(&n) = new_of.get(o.name.as_str()) else { continue };
        let d = draft.columns.iter().find(|c| c.name == n).expect("配对的列在草稿中");
        if !same_column(o, d) {
            cols.push(C::AlterColumn { from: Column { name: n.to_string(), ..o.clone() }, to: d.clone() });
        }
    }
    let is_new = |c: &Column| !new_of.values().any(|&n| n == c.name);
    for (i, c) in draft.columns.iter().enumerate() {
        if is_new(c) {
            let after = i.checked_sub(1).map(|p| draft.columns[p].name.clone());
            cols.push(C::AddColumn { column: c.clone(), after });
        }
    }
    // 列顺序：先按"保留的列保持原顺序、新列插到草稿中的位置"排出当前顺序，再逐个挪到草稿中的位置
    let mut current: Vec<String> =
        orig.columns.iter().filter_map(|o| new_of.get(o.name.as_str()).map(|n| n.to_string())).collect();
    let kept = current.clone();
    for (i, c) in draft.columns.iter().enumerate() {
        if is_new(c) {
            let at = match i.checked_sub(1) {
                None => 0,
                Some(p) => {
                    current.iter().position(|x| *x == draft.columns[p].name).map_or(current.len(), |k| k + 1)
                }
            };
            current.insert(at, c.name.clone());
        }
    }
    for (i, c) in draft.columns.iter().enumerate() {
        if current[i] != c.name {
            let from = current.iter().position(|x| *x == c.name).expect("草稿的列都在当前顺序中");
            let name = current.remove(from);
            current.insert(i, name);
            let after = i.checked_sub(1).map(|p| draft.columns[p].name.clone());
            cols.push(C::MoveColumn { column: c.clone(), after });
        }
    }
    // 新列都追加在最后、保留的列相对顺序不变时，不能指定列位置的库也能照草稿的顺序得到结果
    let appended = {
        let mut expect = kept;
        expect.extend(draft.columns.iter().filter(|c| is_new(c)).map(|c| c.name.clone()));
        expect.iter().eq(draft.columns.iter().map(|c| &c.name))
    };

    // 主键
    let opk =
        orig.primary_key.as_ref().map(|k| PrimaryKey { name: k.name.clone(), columns: map_all(&k.columns) });
    let same_pk = |a: &PrimaryKey, b: &PrimaryKey| {
        a.columns == b.columns && (a.name.is_none() || b.name.is_none() || a.name == b.name)
    };
    match (&opk, &draft.primary_key) {
        (Some(a), Some(b)) if same_pk(a, b) => {}
        (Some(_), Some(b)) => {
            drops.push(C::DropPrimaryKey { key: orig.primary_key.clone().expect("有主键") });
            adds.push(C::AddPrimaryKey { key: b.clone() });
        }
        (Some(_), None) => {
            drops.push(C::DropPrimaryKey { key: orig.primary_key.clone().expect("有主键") })
        }
        (None, Some(b)) => adds.push(C::AddPrimaryKey { key: b.clone() }),
        (None, None) => {}
    }

    // 唯一约束、检查约束：先把原表中引用的列换成新名再比较
    let ouniq: Vec<UniqueConstraint> = orig
        .uniques
        .iter()
        .map(|u| UniqueConstraint { name: u.name.clone(), columns: map_all(&u.columns) })
        .collect();
    let (d, a) = pair(&ouniq, &draft.uniques, |u| u.name.as_deref(), |x, y| x.columns == y.columns);
    let original_of = |u: &UniqueConstraint| {
        orig.uniques[ouniq.iter().position(|x| std::ptr::eq(x, u)).expect("同一个切片")].clone()
    };
    drops.extend(d.into_iter().map(|u| C::DropUnique { unique: original_of(u) }));
    adds.extend(a.into_iter().map(|u| C::AddUnique { unique: u.clone() }));

    let (d, a) = pair(
        &orig.checks,
        &draft.checks,
        |c| c.name.as_deref(),
        |x: &CheckConstraint, y| x.expression.trim() == y.expression.trim(),
    );
    drops.extend(d.into_iter().map(|c| C::DropCheck { check: c.clone() }));
    adds.extend(a.into_iter().map(|c| C::AddCheck { check: c.clone() }));

    // 外键：指向本表自己的外键，引用列也随改名
    let self_ref = |f: &ForeignKey| f.ref_table == orig.name;
    let ofk: Vec<ForeignKey> = orig
        .foreign_keys
        .iter()
        .map(|f| ForeignKey {
            columns: map_all(&f.columns),
            ref_columns: if self_ref(f) { map_all(&f.ref_columns) } else { f.ref_columns.clone() },
            ..f.clone()
        })
        .collect();
    let same_fk = |x: &ForeignKey, y: &ForeignKey| {
        x.columns == y.columns
            && x.ref_table == y.ref_table
            && text(&x.ref_schema) == text(&y.ref_schema)
            && x.ref_columns == y.ref_columns
            && action(&x.on_delete) == action(&y.on_delete)
            && action(&x.on_update) == action(&y.on_update)
    };
    let (d, a) = pair(&ofk, &draft.foreign_keys, |f| f.name.as_deref(), same_fk);
    let fk_drops: Vec<C> = d
        .into_iter()
        .map(|f| C::DropForeignKey {
            key: orig.foreign_keys[ofk.iter().position(|x| std::ptr::eq(x, f)).expect("同一个切片")].clone(),
        })
        .collect();
    let fk_adds: Vec<C> = a.into_iter().map(|f| C::AddForeignKey { key: f.clone() }).collect();

    // 索引：按（经改名换算的）原名配对；只改了名字时改名，内容不同时先删后建
    let map_index = |x: &Index| Index {
        columns: x
            .columns
            .iter()
            .map(|c| IndexColumn {
                name: if is_expression(&c.name) { c.name.clone() } else { map(&c.name) },
                descending: c.descending,
            })
            .collect(),
        ..x.clone()
    };
    let same_index = |x: &Index, y: &Index| {
        x.columns == y.columns
            && x.unique == y.unique
            && x.method.as_deref().map(str::to_ascii_uppercase)
                == y.method.as_deref().map(str::to_ascii_uppercase)
    };
    let mut used = vec![false; orig.indexes.len()];
    let mut renames_ix = Vec::new();
    for x in &draft.indexes {
        let old = renames.indexes.get(&x.name).map_or(x.name.as_str(), String::as_str);
        match orig.indexes.iter().position(|o| o.name == old) {
            Some(i) if !used[i] => {
                used[i] = true;
                if !same_index(&map_index(&orig.indexes[i]), x) {
                    drops.push(C::DropIndex { index: orig.indexes[i].clone() });
                    adds.push(C::AddIndex { index: x.clone() });
                } else if old != x.name {
                    renames_ix.push(C::RenameIndex { from: old.to_string(), to: x.name.clone() });
                }
            }
            _ => adds.push(C::AddIndex { index: x.clone() }),
        }
    }
    for (i, o) in orig.indexes.iter().enumerate() {
        if !used[i] {
            drops.push(C::DropIndex { index: o.clone() });
        }
    }

    let mut changes = fk_drops;
    changes.extend(drops);
    changes.extend(cols);
    changes.extend(adds);
    changes.extend(renames_ix);
    changes.extend(fk_adds);
    if text(&orig.comment) != text(&draft.comment) {
        changes.push(C::SetTableComment { comment: text(&draft.comment).map(str::to_string) });
    }
    Ok(Diff { changes, appended })
}

#[cfg(test)]
mod tests {
    use dblens_core::{ColumnType, LogicalType};

    use super::*;

    fn col(name: &str, native: &str) -> Column {
        Column {
            name: name.into(),
            ordinal: 0,
            data_type: ColumnType { native: native.into(), logical: LogicalType::Other },
            nullable: true,
            default: None,
            comment: None,
            auto_increment: false,
            generated: None,
        }
    }

    fn table() -> Table {
        Table {
            name: "orders".into(),
            comment: Some("订单".into()),
            columns: vec![col("id", "BIGINT"), col("code", "VARCHAR(20)"), col("amount", "DECIMAL(10,2)")],
            primary_key: Some(PrimaryKey { name: Some("pk_orders".into()), columns: vec!["id".into()] }),
            uniques: vec![UniqueConstraint { name: Some("uk_code".into()), columns: vec!["code".into()] }],
            checks: vec![CheckConstraint {
                name: Some("ck_amount".into()),
                expression: "amount >= 0".into(),
            }],
            foreign_keys: vec![ForeignKey {
                name: Some("fk_parent".into()),
                columns: vec!["code".into()],
                ref_schema: None,
                ref_table: "parent".into(),
                ref_columns: vec!["code".into()],
                on_delete: Some("NO ACTION".into()),
                on_update: None,
            }],
            indexes: vec![Index {
                name: "ix_amount".into(),
                columns: vec![IndexColumn { name: "amount".into(), descending: false }],
                unique: false,
                method: Some("BTREE".into()),
            }],
            ..Default::default()
        }
    }

    fn diff(orig: &Table, draft: &Table, renames: &Renames) -> Vec<C> {
        diff_table(Some(orig), draft, renames).unwrap().changes
    }

    fn kinds(changes: &[C]) -> Vec<String> {
        changes
            .iter()
            .map(|c| serde_json::to_value(c).unwrap()["kind"].as_str().unwrap().to_string())
            .collect()
    }

    #[test]
    fn unchanged_table_has_no_changes() {
        let t = table();
        let d = diff_table(Some(&t), &t, &Renames::default()).unwrap();
        assert!(d.changes.is_empty());
        assert!(d.appended);
        // 类型的大小写、空白不同不算改动；NO ACTION 与不写相同
        let mut draft = t.clone();
        draft.columns[2].data_type.native = "decimal( 10, 2 )".into();
        draft.foreign_keys[0].on_delete = None;
        draft.comment = Some(" 订单 ".into());
        assert!(diff(&t, &draft, &Renames::default()).is_empty());
    }

    #[test]
    fn new_table_is_one_create() {
        let t = table();
        let d = diff_table(None, &t, &Renames::default()).unwrap();
        assert_eq!(d.changes, vec![C::CreateTable { table: t }]);
    }

    #[test]
    fn column_changes_in_order() {
        let t = table();
        let mut draft = t.clone();
        draft.columns.retain(|c| c.name != "amount");
        draft.indexes.clear();
        draft.checks.clear();
        draft.columns[1].data_type.native = "VARCHAR(50)".into();
        draft.columns[1].nullable = false;
        draft.columns.push(col("note", "TEXT"));
        let changes = diff(&t, &draft, &Renames::default());
        assert_eq!(
            kinds(&changes),
            ["drop_check", "drop_index", "drop_column", "alter_column", "add_column"]
        );
        let C::AlterColumn { from, to } = &changes[3] else { panic!() };
        assert_eq!(
            (from.data_type.native.as_str(), to.data_type.native.as_str()),
            ("VARCHAR(20)", "VARCHAR(50)")
        );
        assert_eq!(changes[4], C::AddColumn { column: col("note", "TEXT"), after: Some("code".into()) });
    }

    #[test]
    fn rename_follows_into_constraints_and_indexes() {
        let t = table();
        let mut draft = t.clone();
        draft.columns[1].name = "order_code".into();
        draft.columns[2].name = "total".into();
        draft.uniques[0].columns = vec!["order_code".into()];
        draft.foreign_keys[0].columns = vec!["order_code".into()];
        draft.indexes[0].columns[0].name = "total".into();
        draft.checks[0].expression = "total >= 0".into();
        let renames = Renames {
            columns: [("order_code".into(), "code".into()), ("total".into(), "amount".into())].into(),
            ..Default::default()
        };
        let changes = diff(&t, &draft, &renames);
        // 检查约束的表达式是文本，改了就先删后建；其余随改名不算改动
        assert_eq!(kinds(&changes), ["drop_check", "rename_column", "rename_column", "add_check"]);
        assert_eq!(changes[1], C::RenameColumn { from: "code".into(), to: "order_code".into() });
    }

    #[test]
    fn rename_and_alter_together() {
        let t = table();
        let mut draft = t.clone();
        draft.columns[2].name = "total".into();
        draft.columns[2].comment = Some("合计".into());
        draft.indexes[0].columns[0].name = "total".into();
        draft.checks[0].expression = "amount >= 0".into();
        draft.checks.clear();
        let renames = Renames { columns: [("total".into(), "amount".into())].into(), ..Default::default() };
        let changes = diff(&t, &draft, &renames);
        assert_eq!(kinds(&changes), ["drop_check", "rename_column", "alter_column"]);
        let C::AlterColumn { from, to } = &changes[2] else { panic!() };
        assert_eq!(from.name, "total", "修改发生在改名之后");
        assert_eq!(to.comment.as_deref(), Some("合计"));
    }

    #[test]
    fn bad_renames_are_rejected() {
        let t = table();
        let mut draft = t.clone();
        draft.columns[1].name = "x".into();
        let r = Renames { columns: [("x".into(), "nope".into())].into(), ..Default::default() };
        assert!(diff_table(Some(&t), &draft, &r).is_err());
        // 两个草稿列配到同一个原列
        let r = Renames { columns: [("x".into(), "id".into())].into(), ..Default::default() };
        assert!(diff_table(Some(&t), &draft, &r).is_err());
    }

    #[test]
    fn constraints_and_indexes() {
        let t = table();
        let mut draft = t.clone();
        draft.primary_key =
            Some(PrimaryKey { name: Some("pk_orders".into()), columns: vec!["id".into(), "code".into()] });
        draft.uniques[0].columns = vec!["code".into(), "amount".into()];
        draft.foreign_keys[0].on_delete = Some("CASCADE".into());
        draft.indexes[0].name = "ix_total".into();
        draft.indexes.push(Index {
            name: "ix_lower".into(),
            columns: vec![IndexColumn { name: "lower(code)".into(), descending: false }],
            unique: false,
            method: None,
        });
        draft.uniques.push(UniqueConstraint { name: None, columns: vec!["amount".into()] });
        let renames =
            Renames { indexes: [("ix_total".into(), "ix_amount".into())].into(), ..Default::default() };
        let changes = diff(&t, &draft, &renames);
        assert_eq!(
            kinds(&changes),
            [
                "drop_foreign_key",
                "drop_primary_key",
                "drop_unique",
                "add_primary_key",
                "add_unique",
                "add_unique",
                "add_index",
                "rename_index",
                "add_foreign_key"
            ]
        );
        assert_eq!(changes[7], C::RenameIndex { from: "ix_amount".into(), to: "ix_total".into() });
        // 改了索引内容：先删后建
        let mut draft = t.clone();
        draft.indexes[0].columns[0].descending = true;
        assert_eq!(kinds(&diff(&t, &draft, &Renames::default())), ["drop_index", "add_index"]);
        // 删除主键、改表注释
        let mut draft = t.clone();
        draft.primary_key = None;
        draft.comment = None;
        let changes = diff(&t, &draft, &Renames::default());
        assert_eq!(kinds(&changes), ["drop_primary_key", "set_table_comment"]);
        assert_eq!(changes[1], C::SetTableComment { comment: None });
    }

    #[test]
    fn column_order() {
        let t = table();
        // 新列追加在最后：不需要挪
        let mut draft = t.clone();
        draft.columns.push(col("note", "TEXT"));
        let d = diff_table(Some(&t), &draft, &Renames::default()).unwrap();
        assert!(d.appended);
        assert_eq!(kinds(&d.changes), ["add_column"]);
        // 新列插在中间：指定位置的 ADD，不能指定位置的库要重建
        let mut draft = t.clone();
        draft.columns.insert(1, col("note", "TEXT"));
        let d = diff_table(Some(&t), &draft, &Renames::default()).unwrap();
        assert!(!d.appended);
        assert_eq!(d.changes, vec![C::AddColumn { column: col("note", "TEXT"), after: Some("id".into()) }]);
        // 把最后一列挪到最前：一次挪动
        let mut draft = t.clone();
        let last = draft.columns.pop().unwrap();
        draft.columns.insert(0, last);
        let d = diff_table(Some(&t), &draft, &Renames::default()).unwrap();
        assert!(!d.appended);
        assert_eq!(d.changes, vec![C::MoveColumn { column: col("amount", "DECIMAL(10,2)"), after: None }]);
        // 交换前两列
        let mut draft = t.clone();
        draft.columns.swap(0, 1);
        let d = diff_table(Some(&t), &draft, &Renames::default()).unwrap();
        assert_eq!(d.changes, vec![C::MoveColumn { column: col("code", "VARCHAR(20)"), after: None }]);
    }

    #[test]
    fn validation_reports_positions() {
        let mut t = table();
        t.name = " ".into();
        t.columns.push(col("id", ""));
        t.uniques.push(UniqueConstraint { name: Some("uk_code".into()), columns: vec!["nope".into()] });
        t.foreign_keys[0].ref_columns.push("x".into());
        t.indexes.push(Index { name: "ix_amount".into(), columns: vec![], unique: false, method: None });
        t.indexes.push(Index {
            name: "ix_expr".into(),
            columns: vec![IndexColumn { name: "lower(code)".into(), descending: false }],
            unique: false,
            method: None,
        });
        t.checks.push(CheckConstraint { name: None, expression: "".into() });
        let issues = validate(&t);
        let at: Vec<(S, Option<usize>, Option<&str>)> =
            issues.iter().map(|i| (i.section, i.row, i.field)).collect();
        assert_eq!(
            at,
            [
                (S::Table, None, Some("name")),
                (S::Columns, Some(3), Some("name")),
                (S::Columns, Some(3), Some("type")),
                (S::Uniques, Some(1), Some("columns")),
                (S::Indexes, Some(1), Some("columns")),
                (S::ForeignKeys, Some(0), Some("ref_columns")),
                (S::Checks, Some(1), Some("expression")),
                (S::Indexes, Some(1), Some("name")),
                (S::Uniques, Some(1), Some("name")),
            ]
        );
        assert!(validate(&table()).is_empty());
    }
}
```

- [ ] **Step 3: 新建 `crates/dblens-dialect/src/design/mod.rs`**

```rust
//! 表设计器（M3 设计文档第 3–5 节）：前端编辑一份草稿表，这里校验草稿、与原表比对出变更列表，
//! 再按方言生成 DDL；库不支持的改动改用重建表的脚本。不做任何 IO。

mod diff;

use std::collections::BTreeMap;

use dblens_core::schema::{CheckConstraint, Column, ForeignKey, Index, PrimaryKey, Table, UniqueConstraint};
use dblens_core::{Msg, msg};
use serde::{Deserialize, Serialize};

use crate::Dialect;

pub use diff::{diff_table, validate};

/// 草稿中改了名的项：新名 → 原名。前端用每一项的 `origin` 跟踪原来的名字，发送时整理成这个映射。
/// 约束改名一律先删后建，不需要映射。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Renames {
    #[serde(default)]
    pub columns: BTreeMap<String, String>,
    #[serde(default)]
    pub indexes: BTreeMap<String, String>,
}

/// 一项结构变更。列的修改发生在改名之后：`AlterColumn.from` 的名字已换成新名。
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TableChange {
    /// 新建表：整张表（含约束、索引、注释）一次生成。
    CreateTable {
        table: Table,
    },
    SetTableComment {
        comment: Option<String>,
    },
    /// `after` 为草稿中前一列的名字，None 表示放在最前。
    AddColumn {
        column: Column,
        after: Option<String>,
    },
    DropColumn {
        name: String,
    },
    RenameColumn {
        from: String,
        to: String,
    },
    AlterColumn {
        from: Column,
        to: Column,
    },
    /// 调整已有列的位置：`column` 是草稿中的完整定义。
    MoveColumn {
        column: Column,
        after: Option<String>,
    },
    DropPrimaryKey {
        key: PrimaryKey,
    },
    AddPrimaryKey {
        key: PrimaryKey,
    },
    DropUnique {
        unique: UniqueConstraint,
    },
    AddUnique {
        unique: UniqueConstraint,
    },
    DropCheck {
        check: CheckConstraint,
    },
    AddCheck {
        check: CheckConstraint,
    },
    DropForeignKey {
        key: ForeignKey,
    },
    AddForeignKey {
        key: ForeignKey,
    },
    DropIndex {
        index: Index,
    },
    AddIndex {
        index: Index,
    },
    RenameIndex {
        from: String,
        to: String,
    },
}

/// 比对结果。`appended`：新列都在最后、保留的列相对顺序不变——不能指定列位置的库据此判断要不要重建。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Diff {
    pub changes: Vec<TableChange>,
    pub appended: bool,
}

/// 校验错误出现在设计器的哪个部分。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum IssueSection {
    Table,
    Columns,
    PrimaryKey,
    Indexes,
    ForeignKeys,
    Uniques,
    Checks,
}

/// 一处校验错误：`row` 是该部分中的行号（从 0 开始），`field` 是字段名（与前端的列名一致）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DesignIssue {
    pub section: IssueSection,
    pub row: Option<usize>,
    pub field: Option<&'static str>,
    pub message: Msg,
}

impl DesignIssue {
    pub(crate) fn new(
        section: IssueSection,
        row: Option<usize>,
        field: Option<&'static str>,
        message: Msg,
    ) -> Self {
        Self { section, row, field, message }
    }
}

impl Dialect {
    /// 整理前端发来的草稿：按原生类型文本重新解析逻辑类型；主键列一律 NOT NULL。
    /// 返回提示（如"主键列 id 将设为 NOT NULL"）。
    pub fn prepare_draft(&self, draft: &mut Table) -> Vec<Msg> {
        let mut notes = Vec::new();
        let pk: Vec<String> = draft.primary_key.as_ref().map(|k| k.columns.clone()).unwrap_or_default();
        for c in &mut draft.columns {
            c.data_type.native = c.data_type.native.trim().to_string();
            c.data_type.logical = self.logical_type(&c.data_type.native);
            if pk.contains(&c.name) && c.nullable {
                c.nullable = false;
                notes.push(msg!("design.pk_not_null", column = c.name));
            }
        }
        for (i, c) in draft.columns.iter_mut().enumerate() {
            c.ordinal = i as u32 + 1;
        }
        notes
    }
}

#[cfg(test)]
mod tests {
    use dblens_core::schema::PrimaryKey;
    use dblens_core::{ColumnType, LogicalType};

    use super::*;
    use crate::Variant;

    #[test]
    fn prepare_draft_parses_types_and_forces_pk_not_null() {
        let col = |name: &str, native: &str| Column {
            name: name.into(),
            ordinal: 9,
            data_type: ColumnType { native: native.into(), logical: LogicalType::Other },
            nullable: true,
            default: None,
            comment: None,
            auto_increment: false,
            generated: None,
        };
        let mut t = Table {
            name: "t".into(),
            columns: vec![col("id", " bigint "), col("name", "varchar(20)")],
            primary_key: Some(PrimaryKey { name: None, columns: vec!["id".into()] }),
            ..Default::default()
        };
        let notes = Dialect::new(Variant::Postgres).prepare_draft(&mut t);
        assert_eq!(notes, vec![msg!("design.pk_not_null", column = "id")]);
        assert_eq!(t.columns[0].data_type.native, "bigint");
        assert_eq!(t.columns[0].data_type.logical, LogicalType::Int { bits: 64 });
        assert_eq!(t.columns[1].data_type.logical, LogicalType::VarChar { len: Some(20) });
        assert!(!t.columns[0].nullable && t.columns[1].nullable);
        assert_eq!((t.columns[0].ordinal, t.columns[1].ordinal), (1, 2));
    }
}
```

- [ ] **Step 4: 修改 `crates/dblens-dialect/src/lib.rs`**

```diff
--- a/crates/dblens-dialect/src/lib.rs
+++ b/crates/dblens-dialect/src/lib.rs
@@ -3,6 +3,7 @@
 mod browse;
 mod changes;
 mod ddl;
+pub mod design;
 mod dml;
 pub mod ident;
 mod import;
```

- [ ] **Step 5: 检查**

```bash
export CARGO_INCREMENTAL=0
cargo fmt --all
cargo clippy --config "source.mirror.registry='sparse+https://rsproxy.cn/index/'" --workspace --all-targets -- -D warnings
DBLENS_LANG=en cargo test --config "source.mirror.registry='sparse+https://rsproxy.cn/index/'" --workspace
```

Expected: 全部测试通过（dialect 新增 10 个）。

- [ ] **Step 6: 提交**

```bash
git add crates/dblens-core/locales/app.json crates/dblens-dialect/src/design/diff.rs crates/dblens-dialect/src/design/mod.rs crates/dblens-dialect/src/lib.rs
git commit -m "feat(dialect): 表设计器的变更模型、校验与比对" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 3: 方言：类型目录与方言能力

`design/catalog.rs`：`Dialect::design_caps()`，前端据此给出类型下拉、启用长度和小数位、禁用库不支持的选项。
- `reorder`：MySQL 家族能调整列顺序、在指定位置加列；其他家族不能（要重建）。
- `checks`：MySQL 8.0.16、MariaDB 10.2 起执行检查约束；`descending_index`：MySQL 8.0、MariaDB 起。
- 外键动作：MySQL 家族 RESTRICT / CASCADE / SET NULL / NO ACTION；PG 家族再加 SET DEFAULT；Oracle、达梦删除时只有 NO ACTION / CASCADE / SET NULL，没有 ON UPDATE。
- 索引方法：MySQL BTREE / HASH / FULLTEXT / SPATIAL；PG BTREE / HASH / GIN / GIST / BRIN / SPGIST；Oracle、达梦 NORMAL / BITMAP。
- 类型目录：每个家族的常用原生类型，参数形式为无 / 长度（也用于时间类型的小数秒）/ 精度加小数位；Oracle 的 CHAR、VARCHAR2 和达梦的 CHAR、VARCHAR、VARCHAR2 可选 BYTE / CHAR 长度单位。

**Files:**
- Create: `crates/dblens-dialect/src/design/catalog.rs`
- Modify: `crates/dblens-dialect/src/design/mod.rs`

**Interfaces:**
- Consumes：任务 2 的 `design` 模块。
- Produces（`dblens_dialect::design`）：`TypeParams::{None, Length, PrecisionScale}`（snake_case）、`TypeEntry { name, params, char_unit }`、`DesignCaps { reorder, checks, descending_index, on_delete, on_update, index_methods, types }`（Serialize）、`Dialect::design_caps(&self) -> DesignCaps`。

- [ ] **Step 1: 新建 `crates/dblens-dialect/src/design/catalog.rs`**

```rust
//! 设计器用的类型目录和方言能力（M3 设计文档 3.6、4.1 节）：前端据此给出类型下拉、启用长度和小数位、
//! 禁用库不支持的选项。

use serde::Serialize;

use crate::{Dialect, Family, Variant};

/// 类型的参数形式。`Length` 也用于时间类型的小数秒精度。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TypeParams {
    None,
    Length,
    PrecisionScale,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TypeEntry {
    pub name: &'static str,
    pub params: TypeParams,
    /// 长度可选按 BYTE 还是 CHAR 计（Oracle、达梦的字符类型）。
    pub char_unit: bool,
}

/// 设计器可用的功能。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DesignCaps {
    /// 能调整已有列的顺序、在指定位置加列（MySQL 家族）。不能时这些改动要重建表。
    pub reorder: bool,
    /// 支持检查约束（MySQL 8.0.16 以前只解析不执行）。
    pub checks: bool,
    pub descending_index: bool,
    /// 外键可选的动作；空表示不支持这一项（如 Oracle 没有 ON UPDATE）。
    pub on_delete: Vec<&'static str>,
    pub on_update: Vec<&'static str>,
    /// 可选的索引方法，第一个是默认值。
    pub index_methods: Vec<&'static str>,
    pub types: Vec<TypeEntry>,
}

const fn t(name: &'static str, params: TypeParams) -> TypeEntry {
    TypeEntry { name, params, char_unit: false }
}

const fn c(name: &'static str) -> TypeEntry {
    TypeEntry { name, params: TypeParams::Length, char_unit: true }
}

use TypeParams::{Length as L, None as N, PrecisionScale as P};

const MYSQL_TYPES: &[TypeEntry] = &[
    t("TINYINT", N),
    t("SMALLINT", N),
    t("MEDIUMINT", N),
    t("INT", N),
    t("BIGINT", N),
    t("DECIMAL", P),
    t("FLOAT", N),
    t("DOUBLE", N),
    t("BIT", L),
    t("CHAR", L),
    t("VARCHAR", L),
    t("TINYTEXT", N),
    t("TEXT", N),
    t("MEDIUMTEXT", N),
    t("LONGTEXT", N),
    t("BINARY", L),
    t("VARBINARY", L),
    t("TINYBLOB", N),
    t("BLOB", N),
    t("MEDIUMBLOB", N),
    t("LONGBLOB", N),
    t("DATE", N),
    t("TIME", L),
    t("DATETIME", L),
    t("TIMESTAMP", L),
    t("YEAR", N),
    t("JSON", N),
];

const POSTGRES_TYPES: &[TypeEntry] = &[
    t("smallint", N),
    t("integer", N),
    t("bigint", N),
    t("numeric", P),
    t("real", N),
    t("double precision", N),
    t("boolean", N),
    t("char", L),
    t("varchar", L),
    t("text", N),
    t("bytea", N),
    t("date", N),
    t("time", L),
    t("timetz", L),
    t("timestamp", L),
    t("timestamptz", L),
    t("interval", N),
    t("json", N),
    t("jsonb", N),
    t("uuid", N),
    t("inet", N),
    t("xml", N),
];

const ORACLE_TYPES: &[TypeEntry] = &[
    t("NUMBER", P),
    t("INTEGER", N),
    t("FLOAT", L),
    t("BINARY_FLOAT", N),
    t("BINARY_DOUBLE", N),
    c("CHAR"),
    c("VARCHAR2"),
    t("NCHAR", L),
    t("NVARCHAR2", L),
    t("CLOB", N),
    t("NCLOB", N),
    t("BLOB", N),
    t("RAW", L),
    t("DATE", N),
    t("TIMESTAMP", L),
    t("TIMESTAMP WITH TIME ZONE", L),
    t("TIMESTAMP WITH LOCAL TIME ZONE", L),
    t("INTERVAL YEAR TO MONTH", N),
    t("INTERVAL DAY TO SECOND", N),
];

const DAMENG_TYPES: &[TypeEntry] = &[
    t("TINYINT", N),
    t("SMALLINT", N),
    t("INT", N),
    t("BIGINT", N),
    t("NUMBER", P),
    t("DECIMAL", P),
    t("FLOAT", N),
    t("DOUBLE", N),
    t("BIT", N),
    c("CHAR"),
    c("VARCHAR"),
    c("VARCHAR2"),
    t("TEXT", N),
    t("CLOB", N),
    t("BLOB", N),
    t("BINARY", L),
    t("VARBINARY", L),
    t("DATE", N),
    t("TIME", L),
    t("TIMESTAMP", L),
    t("DATETIME", L),
    t("TIMESTAMP WITH TIME ZONE", L),
];

impl Dialect {
    pub fn design_caps(&self) -> DesignCaps {
        let v = self.version().unwrap_or(u32::MAX);
        match self.family() {
            Family::MySql => DesignCaps {
                reorder: true,
                // MySQL 8.0.16、MariaDB 10.2 起执行检查约束
                checks: match self.variant() {
                    Variant::MariaDb => v >= 100200,
                    _ => v >= 80016,
                },
                // 8.0 起支持降序索引
                descending_index: self.variant() == Variant::MariaDb || v >= 80000,
                on_delete: vec!["RESTRICT", "CASCADE", "SET NULL", "NO ACTION"],
                on_update: vec!["RESTRICT", "CASCADE", "SET NULL", "NO ACTION"],
                index_methods: vec!["BTREE", "HASH", "FULLTEXT", "SPATIAL"],
                types: MYSQL_TYPES.to_vec(),
            },
            Family::Postgres => DesignCaps {
                reorder: false,
                checks: true,
                descending_index: true,
                on_delete: vec!["NO ACTION", "RESTRICT", "CASCADE", "SET NULL", "SET DEFAULT"],
                on_update: vec!["NO ACTION", "RESTRICT", "CASCADE", "SET NULL", "SET DEFAULT"],
                index_methods: vec!["BTREE", "HASH", "GIN", "GIST", "BRIN", "SPGIST"],
                types: POSTGRES_TYPES.to_vec(),
            },
            Family::Oracle | Family::Dameng => DesignCaps {
                reorder: false,
                checks: true,
                descending_index: true,
                on_delete: vec!["NO ACTION", "CASCADE", "SET NULL"],
                on_update: vec![],
                index_methods: vec!["NORMAL", "BITMAP"],
                types: if self.family() == Family::Oracle { ORACLE_TYPES } else { DAMENG_TYPES }.to_vec(),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn caps_follow_family_and_version() {
        let mysql57 = Dialect::new(Variant::MySql).with_version(Some(50744)).design_caps();
        assert!(mysql57.reorder && !mysql57.checks && !mysql57.descending_index);
        let mysql8 = Dialect::new(Variant::MySql).with_version(Some(80040)).design_caps();
        assert!(mysql8.checks && mysql8.descending_index);
        let maria = Dialect::new(Variant::MariaDb).with_version(Some(100411)).design_caps();
        assert!(maria.checks && maria.descending_index);
        let pg = Dialect::new(Variant::Kingbase).design_caps();
        assert!(!pg.reorder && pg.on_update.contains(&"SET DEFAULT"));
        assert!(pg.types.iter().any(|t| t.name == "jsonb"));
        let ora = Dialect::new(Variant::Oracle).design_caps();
        assert!(ora.on_update.is_empty());
        let varchar2 = ora.types.iter().find(|t| t.name == "VARCHAR2").unwrap();
        assert_eq!((varchar2.params, varchar2.char_unit), (TypeParams::Length, true));
        let dm = Dialect::new(Variant::Dameng).design_caps();
        assert!(dm.types.iter().any(|t| t.name == "VARCHAR" && t.char_unit));
    }

    #[test]
    fn every_catalog_type_parses_to_a_known_logical_type() {
        use dblens_core::LogicalType;
        for v in [Variant::MySql, Variant::Postgres, Variant::Oracle, Variant::Dameng] {
            let d = Dialect::new(v);
            for t in d.design_caps().types {
                let native = match t.params {
                    TypeParams::None => t.name.to_string(),
                    TypeParams::Length => format!("{}(10)", t.name),
                    TypeParams::PrecisionScale => format!("{}(10,2)", t.name),
                };
                // 文本、二进制的若干大类型和 PG 的 inet、xml 等映射为 Other 是允许的；只检查不会 panic 且数值类型正确
                let logical = d.logical_type(&native);
                if matches!(t.name, "DECIMAL" | "NUMBER" | "numeric") {
                    assert!(matches!(logical, LogicalType::Decimal { .. }), "{v:?} {native} → {logical:?}");
                }
            }
        }
    }
}
```

- [ ] **Step 2: 修改 `crates/dblens-dialect/src/design/mod.rs`**

```diff
--- a/crates/dblens-dialect/src/design/mod.rs
+++ b/crates/dblens-dialect/src/design/mod.rs
@@ -1,6 +1,7 @@
 //! 表设计器（M3 设计文档第 3–5 节）：前端编辑一份草稿表，这里校验草稿、与原表比对出变更列表，
 //! 再按方言生成 DDL；库不支持的改动改用重建表的脚本。不做任何 IO。
 
+mod catalog;
 mod diff;
 
 use std::collections::BTreeMap;
@@ -11,6 +12,7 @@ use serde::{Deserialize, Serialize};
 
 use crate::Dialect;
 
+pub use catalog::{DesignCaps, TypeEntry, TypeParams};
 pub use diff::{diff_table, validate};
 
 /// 草稿中改了名的项：新名 → 原名。前端用每一项的 `origin` 跟踪原来的名字，发送时整理成这个映射。
```

- [ ] **Step 3: 检查**

```bash
export CARGO_INCREMENTAL=0
cargo fmt --all
cargo clippy --config "source.mirror.registry='sparse+https://rsproxy.cn/index/'" --workspace --all-targets -- -D warnings
DBLENS_LANG=en cargo test --config "source.mirror.registry='sparse+https://rsproxy.cn/index/'" --workspace
```

Expected: 全部测试通过（dialect 新增 2 个）。

- [ ] **Step 4: 提交**

```bash
git add crates/dblens-dialect/src/design/catalog.rs crates/dblens-dialect/src/design/mod.rs
git commit -m "feat(dialect): 设计器的类型目录与方言能力" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 4: 方言：按方言生成 DDL

`design/sql.rs`（共用）、`mysql.rs`、`postgres.rs`、`oracle.rs`（含达梦）：
- `Dialect::design_unsupported(&Diff) -> Vec<Msg>`：不能指定列位置的库在 `!appended` 时；PG 改生成表达式、serial 列改自增；Oracle、达梦改生成表达式或已有列的自增。
- `Dialect::design_sql(schema, table, &Diff) -> Result<Vec<DesignStatement>>`，每条带对应的变更序号；`design_transactional()`：只有 PG 家族。
- MySQL 家族：每项改动一条 `ALTER TABLE`（同一条 ALTER 中改名与 MODIFY、AFTER 引用改名前还是改名后的列，各版本行为不一）；修改和挪动都用 `MODIFY COLUMN 完整定义 [FIRST | AFTER]`，既修改又挪动的列只在挪动时 MODIFY 一次；换主键时删除和新增放在同一条（自增列必须是键，单独删主键报 1075）；改名 MySQL 8.0、MariaDB 10.5 起 `RENAME COLUMN`，更早用 `CHANGE`；索引改名 MySQL 5.7、MariaDB 10.5.2 起 `RENAME INDEX`，更早先删后建；删检查约束 MariaDB 用 `DROP CONSTRAINT`；FULLTEXT / SPATIAL 索引、函数索引的表达式套一层括号。
- PG 家族：改类型 `TYPE t USING c::t`；默认值、可空、标识列（变成标识列前先去掉默认值）、注释各一条；索引用 `CREATE INDEX`（非 btree 写 USING），`ALTER INDEX … RENAME TO`；约束用 `DROP CONSTRAINT`。
- Oracle：`ADD (…)`、只写变化部分的 `MODIFY (…)`（去掉默认值写 `DEFAULT NULL`）、`COMMENT ON … IS ''` 删除注释；达梦：`ADD COLUMN`、类型用 `MODIFY`，默认值和可空用 `ALTER COLUMN … SET/DROP DEFAULT`、`SET NULL/NOT NULL`，自增 `IDENTITY(1, 1)`；外键没有 ON UPDATE；BITMAP 索引。
- 新建表：各家族自己的建表语句（MySQL 的注释、自增、索引写在建表语句里；其他家族之后跟索引和 COMMENT ON）。
- 回归：生成的每条语句经 `Dialect::split` 仍是原样的一条（注释和默认值中带分号、注释符和 `/`）。

后端词条加 `design.unsupported.*`（4 个）、`design.unnamed_constraint`。

快照文件（`src/design/snapshots/`、`tests/snapshots/`）随本任务给出；改了生成规则时用 `INSTA_UPDATE=always` 重新生成并逐个审查差异。

**Files:**
- Modify: `crates/dblens-core/locales/app.json`
- Modify: `crates/dblens-dialect/src/design/diff.rs`
- Modify: `crates/dblens-dialect/src/design/mod.rs`
- Create: `crates/dblens-dialect/src/design/mysql.rs`
- Create: `crates/dblens-dialect/src/design/oracle.rs`
- Create: `crates/dblens-dialect/src/design/postgres.rs`
- Create: `crates/dblens-dialect/src/design/snapshots/dblens_dialect__design__mysql__tests__mysql56_everything.snap`
- Create: `crates/dblens-dialect/src/design/snapshots/dblens_dialect__design__mysql__tests__mysql56_move.snap`
- Create: `crates/dblens-dialect/src/design/snapshots/dblens_dialect__design__mysql__tests__mysql_alter.snap`
- Create: `crates/dblens-dialect/src/design/snapshots/dblens_dialect__design__mysql__tests__mysql_create.snap`
- Create: `crates/dblens-dialect/src/design/snapshots/dblens_dialect__design__mysql__tests__mysql_everything.snap`
- Create: `crates/dblens-dialect/src/design/snapshots/dblens_dialect__design__mysql__tests__mysql_move.snap`
- Create: `crates/dblens-dialect/src/design/snapshots/dblens_dialect__design__oracle__tests__dameng_alter.snap`
- Create: `crates/dblens-dialect/src/design/snapshots/dblens_dialect__design__oracle__tests__dameng_create.snap`
- Create: `crates/dblens-dialect/src/design/snapshots/dblens_dialect__design__oracle__tests__dameng_everything.snap`
- Create: `crates/dblens-dialect/src/design/snapshots/dblens_dialect__design__oracle__tests__oracle_alter.snap`
- Create: `crates/dblens-dialect/src/design/snapshots/dblens_dialect__design__oracle__tests__oracle_create.snap`
- Create: `crates/dblens-dialect/src/design/snapshots/dblens_dialect__design__oracle__tests__oracle_everything.snap`
- Create: `crates/dblens-dialect/src/design/snapshots/dblens_dialect__design__postgres__tests__pg_alter.snap`
- Create: `crates/dblens-dialect/src/design/snapshots/dblens_dialect__design__postgres__tests__pg_create.snap`
- Create: `crates/dblens-dialect/src/design/snapshots/dblens_dialect__design__postgres__tests__pg_everything.snap`
- Create: `crates/dblens-dialect/src/design/sql.rs`

**Interfaces:**
- Consumes：任务 2 的 `Diff`、`TableChange`；任务 3 的 `design_caps`。
- Produces（`dblens_dialect::design`）：`DesignStatement { changes: Vec<usize>, sql: String }`（Serialize）；`Dialect::design_unsupported(&self, &Diff) -> Vec<Msg>`、`design_sql(&self, schema: &str, table: &str, &Diff) -> Result<Vec<DesignStatement>>`、`design_transactional(&self) -> bool`；crate 内：`idents`、`constraint_name`、`required_name`、`fk_clause`、`check_clause`、`index_items`、`default_clause`。

- [ ] **Step 1: 修改 `crates/dblens-core/locales/app.json`**

```diff
--- a/crates/dblens-core/locales/app.json
+++ b/crates/dblens-core/locales/app.json
@@ -31,6 +31,11 @@
   "design.ref_table_empty": {"zh-CN": "请选择引用的表", "en": "Select the referenced table"},
   "design.type_empty": {"zh-CN": "类型不能为空", "en": "Type is required"},
   "design.unknown_column": {"zh-CN": "列 %{column} 不存在", "en": "Column %{column} does not exist"},
+  "design.unnamed_constraint": {"zh-CN": "约束没有名字，无法删除", "en": "The constraint has no name and cannot be dropped"},
+  "design.unsupported.generated": {"zh-CN": "该库不能修改已有列 %{column} 的生成表达式", "en": "This database cannot change the generation expression of existing column %{column}"},
+  "design.unsupported.identity": {"zh-CN": "该库不能改变已有列 %{column} 的自增属性", "en": "This database cannot change the auto-increment of existing column %{column}"},
+  "design.unsupported.reorder": {"zh-CN": "该库不能调整已有列的顺序或在指定位置加列", "en": "This database cannot reorder existing columns or add a column at a given position"},
+  "design.unsupported.serial": {"zh-CN": "列 %{column} 是 serial 列，不能直接改变自增属性", "en": "Column %{column} is a serial column; its auto-increment cannot be changed directly"},
   "desktop.bind_failed": {"zh-CN": "无法监听本机端口：%{error}", "en": "Cannot listen on a local port: %{error}"},
   "desktop.log_dir_failed": {"zh-CN": "无法创建日志目录 %{dir}：%{error}", "en": "Cannot create the log directory %{dir}: %{error}"},
   "desktop.no_main_window": {"zh-CN": "主窗口不存在", "en": "The main window does not exist"},
```

- [ ] **Step 2: 修改 `crates/dblens-dialect/src/design/diff.rs`**

```diff
--- a/crates/dblens-dialect/src/design/diff.rs
+++ b/crates/dblens-dialect/src/design/diff.rs
@@ -105,12 +105,12 @@ pub fn validate(draft: &Table) -> Vec<DesignIssue> {
 }
 
 /// 去掉首尾空白，空串当作没有。
-fn text(v: &Option<String>) -> Option<&str> {
+pub(crate) fn text(v: &Option<String>) -> Option<&str> {
     v.as_deref().map(str::trim).filter(|s| !s.is_empty())
 }
 
 /// 类型文本比较：不区分大小写，忽略多余空白和括号、逗号两侧的空白。
-fn same_type(a: &str, b: &str) -> bool {
+pub(crate) fn same_type(a: &str, b: &str) -> bool {
     let norm = |s: &str| {
         let s = s.split_whitespace().collect::<Vec<_>>().join(" ").to_ascii_uppercase();
         s.replace(" (", "(").replace("( ", "(").replace(" )", ")").replace(", ", ",").replace(" ,", ",")
@@ -201,7 +201,8 @@ pub fn diff_table(original: Option<&Table>, draft: &Table, renames: &Renames) ->
         if let Some(&n) = new_of.get(o.name.as_str())
             && n != o.name
         {
-            cols.push(C::RenameColumn { from: o.name.clone(), to: n.to_string() });
+            let column = draft.columns.iter().find(|c| c.name == n).expect("配对的列在草稿中").clone();
+            cols.push(C::RenameColumn { from: o.name.clone(), to: n.to_string(), column });
         }
     }
     for o in &orig.columns {
@@ -347,7 +348,7 @@ pub fn diff_table(original: Option<&Table>, draft: &Table, renames: &Renames) ->
                     drops.push(C::DropIndex { index: orig.indexes[i].clone() });
                     adds.push(C::AddIndex { index: x.clone() });
                 } else if old != x.name {
-                    renames_ix.push(C::RenameIndex { from: old.to_string(), to: x.name.clone() });
+                    renames_ix.push(C::RenameIndex { from: old.to_string(), index: x.clone() });
                 }
             }
             _ => adds.push(C::AddIndex { index: x.clone() }),
@@ -492,7 +493,14 @@ mod tests {
         let changes = diff(&t, &draft, &renames);
         // 检查约束的表达式是文本，改了就先删后建；其余随改名不算改动
         assert_eq!(kinds(&changes), ["drop_check", "rename_column", "rename_column", "add_check"]);
-        assert_eq!(changes[1], C::RenameColumn { from: "code".into(), to: "order_code".into() });
+        assert_eq!(
+            changes[1],
+            C::RenameColumn {
+                from: "code".into(),
+                to: "order_code".into(),
+                column: draft.columns[1].clone()
+            }
+        );
     }
 
     #[test]
@@ -557,7 +565,7 @@ mod tests {
                 "add_foreign_key"
             ]
         );
-        assert_eq!(changes[7], C::RenameIndex { from: "ix_amount".into(), to: "ix_total".into() });
+        assert_eq!(changes[7], C::RenameIndex { from: "ix_amount".into(), index: draft.indexes[0].clone() });
         // 改了索引内容：先删后建
         let mut draft = t.clone();
         draft.indexes[0].columns[0].descending = true;
```

- [ ] **Step 3: 修改 `crates/dblens-dialect/src/design/mod.rs`**

```diff
--- a/crates/dblens-dialect/src/design/mod.rs
+++ b/crates/dblens-dialect/src/design/mod.rs
@@ -3,6 +3,10 @@
 
 mod catalog;
 mod diff;
+mod mysql;
+mod oracle;
+mod postgres;
+mod sql;
 
 use std::collections::BTreeMap;
 
@@ -14,6 +18,7 @@ use crate::Dialect;
 
 pub use catalog::{DesignCaps, TypeEntry, TypeParams};
 pub use diff::{diff_table, validate};
+pub use sql::DesignStatement;
 
 /// 草稿中改了名的项：新名 → 原名。前端用每一项的 `origin` 跟踪原来的名字，发送时整理成这个映射。
 /// 约束改名一律先删后建，不需要映射。
@@ -44,9 +49,11 @@ pub enum TableChange {
     DropColumn {
         name: String,
     },
+    /// `column` 是草稿中的完整定义（旧版 MySQL 只能用 `CHANGE` 带上定义改名）。
     RenameColumn {
         from: String,
         to: String,
+        column: Column,
     },
     AlterColumn {
         from: Column,
@@ -87,9 +94,10 @@ pub enum TableChange {
     AddIndex {
         index: Index,
     },
+    /// `index` 是草稿中的索引（不支持改名的旧版本先删后建）。
     RenameIndex {
         from: String,
-        to: String,
+        index: Index,
     },
 }
 
```

- [ ] **Step 4: 新建 `crates/dblens-dialect/src/design/mysql.rs`**

```rust
//! MySQL 家族（MySQL、MariaDB、TiDB、OceanBase、PolarDB）的设计器 DDL（M3 设计文档 4.2 节）。
//! 每项变更一条 `ALTER TABLE`：同一条 ALTER 中改名与 MODIFY、AFTER 引用的是改名前还是改名后的列，
//! 各版本行为不一，分开执行最可预期（MySQL 8 的多数列操作是 INSTANT / INPLACE，不重建表）。

use std::collections::HashMap;

use dblens_core::Result;
use dblens_core::schema::{Column, Index, Table};

use super::TableChange as C;
use super::diff::text;
use super::sql::{DesignStatement, stmt};
use crate::{Dialect, Variant};

impl Dialect {
    /// 支持 `RENAME COLUMN`：MySQL 8.0、MariaDB 10.5 起。
    fn mysql_rename_column(&self) -> bool {
        let v = self.version().unwrap_or(u32::MAX);
        if self.variant() == Variant::MariaDb { v >= 100500 } else { v >= 80000 }
    }

    /// 支持 `RENAME INDEX`：MySQL 5.7、MariaDB 10.5.2 起。
    fn mysql_rename_index(&self) -> bool {
        let v = self.version().unwrap_or(u32::MAX);
        if self.variant() == Variant::MariaDb { v >= 100502 } else { v >= 50700 }
    }

    /// 列定义：类型、生成表达式、可空、默认值、自增、注释。
    fn mysql_column(&self, c: &Column) -> Result<String> {
        let mut s = format!("{} {}", self.ident(&c.name), c.data_type.native);
        if let Some(g) = text(&c.generated) {
            s += &format!(" GENERATED ALWAYS AS ({g})");
        }
        s += if c.nullable { " NULL" } else { " NOT NULL" };
        s += &Self::default_clause(c);
        if c.auto_increment {
            s += " AUTO_INCREMENT";
        }
        if let Some(cm) = text(&c.comment) {
            s += &format!(" COMMENT {}", self.text_literal(cm)?);
        }
        Ok(s)
    }

    fn mysql_position(&self, after: &Option<String>) -> String {
        match after {
            None => " FIRST".into(),
            Some(a) => format!(" AFTER {}", self.ident(a)),
        }
    }

    /// `INDEX 名字 (列…) [USING 方法]`，FULLTEXT / SPATIAL 写在前面；`add` 时前面加 `ADD `。
    fn mysql_index(&self, x: &Index) -> String {
        let method = x.method.as_deref().map(str::to_ascii_uppercase);
        let kind = match method.as_deref() {
            Some("FULLTEXT") => "FULLTEXT ",
            Some("SPATIAL") => "SPATIAL ",
            _ if x.unique => "UNIQUE ",
            _ => "",
        };
        let using = match method.as_deref() {
            Some(m @ ("BTREE" | "HASH")) => format!(" USING {m}"),
            _ => String::new(),
        };
        format!("{kind}INDEX {} ({}){using}", self.ident(&x.name), self.index_items(x))
    }

    fn mysql_create(&self, schema: &str, t: &Table) -> Result<String> {
        let mut items = Vec::new();
        for c in &t.columns {
            items.push(self.mysql_column(c)?);
        }
        if let Some(pk) = &t.primary_key {
            items.push(format!("PRIMARY KEY ({})", self.idents(&pk.columns)));
        }
        for u in &t.uniques {
            items.push(format!("{}UNIQUE ({})", self.constraint_name(&u.name), self.idents(&u.columns)));
        }
        for x in &t.indexes {
            items.push(self.mysql_index(x));
        }
        for f in &t.foreign_keys {
            items.push(format!("{}{}", self.constraint_name(&f.name), self.fk_clause(schema, f)));
        }
        for c in &t.checks {
            items.push(self.check_clause(c));
        }
        let mut sql = format!(
            "CREATE TABLE {} (\n    {}\n)",
            self.qualified(Some(schema), &t.name),
            items.join(",\n    ")
        );
        if let Some(c) = text(&t.comment) {
            sql += &format!(" COMMENT = {}", self.text_literal(c)?);
        }
        Ok(sql)
    }

    pub(crate) fn mysql_design(
        &self,
        schema: &str,
        table: &str,
        changes: &[C],
    ) -> Result<Vec<DesignStatement>> {
        let q = self.qualified(Some(schema), table);
        let alter = |clause: String| format!("ALTER TABLE {q} {clause}");
        // 既修改又挪动的列：只在挪动时用草稿的完整定义 MODIFY 一次
        let moved: HashMap<&str, usize> = changes
            .iter()
            .enumerate()
            .filter_map(|(i, c)| match c {
                C::MoveColumn { column, .. } => Some((column.name.as_str(), i)),
                _ => None,
            })
            .collect();
        // 换主键：删除和新增放在同一条 ALTER 里（自增列必须是键，单独删主键会报错 1075）
        let add_pk = changes.iter().position(|c| matches!(c, C::AddPrimaryKey { .. }));
        let drop_pk = changes.iter().position(|c| matches!(c, C::DropPrimaryKey { .. }));
        let mut out: Vec<DesignStatement> = Vec::new();
        let mut merged: HashMap<usize, usize> = HashMap::new();
        for (i, ch) in changes.iter().enumerate() {
            let sql = match ch {
                C::CreateTable { table } => self.mysql_create(schema, table)?,
                C::SetTableComment { comment } => {
                    alter(format!("COMMENT = {}", self.text_literal(comment.as_deref().unwrap_or(""))?))
                }
                C::AddColumn { column, after } => {
                    alter(format!("ADD COLUMN {}{}", self.mysql_column(column)?, self.mysql_position(after)))
                }
                C::DropColumn { name } => alter(format!("DROP COLUMN {}", self.ident(name))),
                C::RenameColumn { from, to, column } => {
                    if self.mysql_rename_column() {
                        alter(format!("RENAME COLUMN {} TO {}", self.ident(from), self.ident(to)))
                    } else {
                        alter(format!("CHANGE COLUMN {} {}", self.ident(from), self.mysql_column(column)?))
                    }
                }
                C::AlterColumn { to, .. } => {
                    if let Some(&m) = moved.get(to.name.as_str()) {
                        merged.insert(m, i);
                        continue;
                    }
                    alter(format!("MODIFY COLUMN {}", self.mysql_column(to)?))
                }
                C::MoveColumn { column, after } => alter(format!(
                    "MODIFY COLUMN {}{}",
                    self.mysql_column(column)?,
                    self.mysql_position(after)
                )),
                C::DropPrimaryKey { .. } if add_pk.is_some() => {
                    merged.insert(add_pk.expect("有新主键"), i);
                    continue;
                }
                C::DropPrimaryKey { .. } => alter("DROP PRIMARY KEY".into()),
                C::AddPrimaryKey { key } => alter(format!(
                    "{}ADD PRIMARY KEY ({})",
                    if drop_pk.is_some() { "DROP PRIMARY KEY, " } else { "" },
                    self.idents(&key.columns)
                )),
                C::DropUnique { unique } => {
                    alter(format!("DROP INDEX {}", self.ident(self.required_name(&unique.name)?)))
                }
                C::AddUnique { unique } => alter(format!(
                    "ADD {}UNIQUE ({})",
                    self.constraint_name(&unique.name),
                    self.idents(&unique.columns)
                )),
                C::DropCheck { check } => {
                    let what = if self.variant() == Variant::MariaDb { "CONSTRAINT" } else { "CHECK" };
                    alter(format!("DROP {what} {}", self.ident(self.required_name(&check.name)?)))
                }
                C::AddCheck { check } => alter(format!("ADD {}", self.check_clause(check))),
                C::DropForeignKey { key } => {
                    alter(format!("DROP FOREIGN KEY {}", self.ident(self.required_name(&key.name)?)))
                }
                C::AddForeignKey { key } => {
                    alter(format!("ADD {}{}", self.constraint_name(&key.name), self.fk_clause(schema, key)))
                }
                C::DropIndex { index } => alter(format!("DROP INDEX {}", self.ident(&index.name))),
                C::AddIndex { index } => alter(format!("ADD {}", self.mysql_index(index))),
                C::RenameIndex { from, index } => {
                    if self.mysql_rename_index() {
                        alter(format!("RENAME INDEX {} TO {}", self.ident(from), self.ident(&index.name)))
                    } else {
                        alter(format!("DROP INDEX {}, ADD {}", self.ident(from), self.mysql_index(index)))
                    }
                }
            };
            let mut s = stmt(i, sql);
            if let Some(&a) = merged.get(&i) {
                s.changes.insert(0, a);
            }
            out.push(s);
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::super::sql::fixtures::*;
    use crate::design::{Renames, diff_table};
    use crate::{Dialect, Variant};

    const TYPES: [&str; 3] = ["BIGINT", "VARCHAR(20)", "DECIMAL(10,2)"];

    fn mysql8() -> Dialect {
        Dialect::new(Variant::MySql).with_version(Some(80040))
    }

    #[test]
    fn create_table() {
        let t = table(TYPES);
        let d = mysql8();
        let diff = diff_table(None, &t, &Renames::default()).unwrap();
        insta::assert_snapshot!("mysql_create", render(&d.design_sql("shop", "", &diff).unwrap()));
    }

    #[test]
    fn every_change() {
        let orig = table(TYPES);
        let d = mysql8();
        insta::assert_snapshot!(
            "mysql_everything",
            render(&d.design_sql("shop", "orders", &everything(&orig, "VARCHAR(50)")).unwrap())
        );
        insta::assert_snapshot!(
            "mysql_alter",
            render(&d.design_sql("shop", "orders", &alter_only(&orig, "DECIMAL(12,2)")).unwrap())
        );
        insta::assert_snapshot!(
            "mysql_move",
            render(&d.design_sql("shop", "orders", &move_first(&orig)).unwrap())
        );
    }

    #[test]
    fn old_versions_and_mariadb() {
        let orig = table(TYPES);
        let old = Dialect::new(Variant::MySql).with_version(Some(50650));
        insta::assert_snapshot!(
            "mysql56_everything",
            render(&old.design_sql("shop", "orders", &everything(&orig, "VARCHAR(50)")).unwrap())
        );
        insta::assert_snapshot!(
            "mysql56_move",
            render(&old.design_sql("shop", "orders", &move_first(&orig)).unwrap())
        );
        let maria = Dialect::new(Variant::MariaDb).with_version(Some(100411));
        let sql = render(&maria.design_sql("shop", "orders", &everything(&orig, "VARCHAR(50)")).unwrap());
        assert!(sql.contains("DROP CONSTRAINT ck_amount"), "{sql}");
        assert!(sql.contains("CHANGE COLUMN code order_code"), "{sql}");
    }

    #[test]
    fn move_and_alter_together_is_one_modify() {
        let orig = table(TYPES);
        let mut d = orig.clone();
        let mut amount = d.columns.pop().unwrap();
        amount.comment = Some("合计".into());
        d.columns.insert(0, amount);
        let diff = diff_table(Some(&orig), &d, &Renames::default()).unwrap();
        let out = mysql8().design_sql("shop", "orders", &diff).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].changes, vec![0, 1]);
        assert_eq!(
            out[0].sql,
            "ALTER TABLE shop.orders MODIFY COLUMN amount DECIMAL(10,2) NULL DEFAULT 0 COMMENT '合计' FIRST"
        );
    }
}
```

- [ ] **Step 5: 新建 `crates/dblens-dialect/src/design/oracle.rs`**

```rust
//! Oracle 与达梦的设计器 DDL（M3 设计文档 4.4、4.5 节）。DDL 自动提交，每项改动一条或几条语句。
//! 两者大体相同；差别：新列 Oracle 写 `ADD (…)`、达梦写 `ADD COLUMN`；自增 Oracle 用标识列、达梦用 `IDENTITY(1, 1)`；
//! 修改列 Oracle 用一条只写变化部分的 `MODIFY (…)`，达梦的类型用 `MODIFY`、默认值和可空用 `ALTER COLUMN`。

use dblens_core::Result;
use dblens_core::schema::{Column, Index, Table};

use super::TableChange as C;
use super::diff::{same_type, text};
use super::sql::{DesignStatement, stmt};
use crate::{Dialect, Family};

impl Dialect {
    fn is_dameng(&self) -> bool {
        self.family() == Family::Dameng
    }

    /// 列定义：类型、生成表达式 / 自增 / 默认值、NOT NULL。注释另用 COMMENT ON。
    fn ora_column(&self, c: &Column) -> String {
        let mut s = format!("{} {}", self.ident(&c.name), c.data_type.native);
        if let Some(g) = text(&c.generated) {
            s += &format!(" GENERATED ALWAYS AS ({g}) VIRTUAL");
        } else if c.auto_increment {
            s += if self.is_dameng() { " IDENTITY(1, 1)" } else { " GENERATED BY DEFAULT AS IDENTITY" };
        } else {
            s += &Self::default_clause(c);
        }
        if !c.nullable {
            s += " NOT NULL";
        }
        s
    }

    /// 注释：没有注释时写空串（Oracle 的 `IS ''` 即删除注释）。
    fn ora_comment(&self, on: &str, comment: Option<&str>) -> Result<String> {
        Ok(format!("COMMENT ON {on} IS {}", self.text_literal(comment.unwrap_or(""))?))
    }

    fn ora_index(&self, schema: &str, table: &str, x: &Index) -> String {
        let kind = match x.method.as_deref() {
            Some(m) if m.eq_ignore_ascii_case("BITMAP") => "BITMAP ",
            _ if x.unique => "UNIQUE ",
            _ => "",
        };
        format!(
            "CREATE {kind}INDEX {} ON {} ({})",
            self.qualified(Some(schema), &x.name),
            self.qualified(Some(schema), table),
            self.index_items(x)
        )
    }

    fn ora_create(&self, schema: &str, t: &Table) -> Result<Vec<String>> {
        let q = self.qualified(Some(schema), &t.name);
        let mut items: Vec<String> = t.columns.iter().map(|c| self.ora_column(c)).collect();
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
            out.push(self.ora_index(schema, &t.name, x));
        }
        if let Some(c) = text(&t.comment) {
            out.push(self.ora_comment(&format!("TABLE {q}"), Some(c))?);
        }
        for c in &t.columns {
            if let Some(cm) = text(&c.comment) {
                out.push(self.ora_comment(&format!("COLUMN {q}.{}", self.ident(&c.name)), Some(cm))?);
            }
        }
        Ok(out)
    }

    /// 修改已有列。自增和生成表达式的修改由 `design_unsupported` 挡住。
    fn ora_alter_column(&self, q: &str, from: &Column, to: &Column) -> Result<Vec<String>> {
        let col = self.ident(&to.name);
        let type_changed = !same_type(&from.data_type.native, &to.data_type.native);
        let default_changed = text(&from.default) != text(&to.default) && to.generated.is_none();
        let null_changed = from.nullable != to.nullable;
        let mut out = Vec::new();
        if self.is_dameng() {
            let alter = |what: String| format!("ALTER TABLE {q} ALTER COLUMN {col} {what}");
            if type_changed {
                out.push(format!("ALTER TABLE {q} MODIFY {col} {}", to.data_type.native));
            }
            if default_changed {
                out.push(match text(&to.default) {
                    Some(d) => alter(format!("SET DEFAULT {d}")),
                    None => alter("DROP DEFAULT".into()),
                });
            }
            if null_changed {
                out.push(alter(if to.nullable { "SET NULL" } else { "SET NOT NULL" }.into()));
            }
        } else if type_changed || default_changed || null_changed {
            // 只写变化的部分：重复写相同的 NOT NULL 会报 ORA-01442
            let mut s = col.clone();
            if type_changed {
                s += &format!(" {}", to.data_type.native);
            }
            if default_changed {
                s += &format!(" DEFAULT {}", text(&to.default).unwrap_or("NULL"));
            }
            if null_changed {
                s += if to.nullable { " NULL" } else { " NOT NULL" };
            }
            out.push(format!("ALTER TABLE {q} MODIFY ({s})"));
        }
        if text(&from.comment) != text(&to.comment) {
            out.push(self.ora_comment(&format!("COLUMN {q}.{col}"), text(&to.comment))?);
        }
        Ok(out)
    }

    pub(crate) fn oracle_design(
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
        let mut out = Vec::new();
        for (i, ch) in changes.iter().enumerate() {
            let sqls: Vec<String> = match ch {
                C::CreateTable { table } => self.ora_create(schema, table)?,
                C::SetTableComment { comment } => {
                    vec![self.ora_comment(&format!("TABLE {q}"), text(comment))?]
                }
                C::AddColumn { column, .. } => {
                    let def = self.ora_column(column);
                    let mut v = vec![if self.is_dameng() {
                        alter(format!("ADD COLUMN {def}"))
                    } else {
                        alter(format!("ADD ({def})"))
                    }];
                    if let Some(cm) = text(&column.comment) {
                        v.push(
                            self.ora_comment(&format!("COLUMN {q}.{}", self.ident(&column.name)), Some(cm))?,
                        );
                    }
                    v
                }
                C::DropColumn { name } => vec![alter(format!("DROP COLUMN {}", self.ident(name)))],
                C::RenameColumn { from, to, .. } => {
                    vec![alter(format!("RENAME COLUMN {} TO {}", self.ident(from), self.ident(to)))]
                }
                C::AlterColumn { from, to } => self.ora_alter_column(&q, from, to)?,
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
                C::DropIndex { index } => {
                    vec![format!("DROP INDEX {}", self.qualified(Some(schema), &index.name))]
                }
                C::AddIndex { index } => vec![self.ora_index(schema, table, index)],
                C::RenameIndex { from, index } => vec![format!(
                    "ALTER INDEX {} RENAME TO {}",
                    self.qualified(Some(schema), from),
                    self.ident(&index.name)
                )],
            };
            out.extend(sqls.into_iter().map(|s| stmt(i, s)));
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::super::sql::fixtures::*;
    use crate::design::{Renames, diff_table};
    use crate::{Dialect, Variant};

    fn appended(mut diff: crate::design::Diff) -> crate::design::Diff {
        // 新列插在中间会改为重建；这里把 note 放到最后，覆盖其余每种变更
        for c in &mut diff.changes {
            if let crate::design::TableChange::AddColumn { after, .. } = c {
                *after = Some("ORDER_CODE".into());
            }
        }
        diff
    }

    #[test]
    fn oracle() {
        let d = Dialect::new(Variant::Oracle);
        let t = table(["NUMBER(19)", "VARCHAR2(20)", "NUMBER(10,2)"]);
        let diff = diff_table(None, &t, &Renames::default()).unwrap();
        insta::assert_snapshot!("oracle_create", render(&d.design_sql("HR", "", &diff).unwrap()));
        let diff = appended(everything(&t, "VARCHAR2(50 CHAR)"));
        insta::assert_snapshot!("oracle_everything", render(&d.design_sql("HR", "orders", &diff).unwrap()));
        insta::assert_snapshot!(
            "oracle_alter",
            render(&d.design_sql("HR", "orders", &alter_only(&t, "NUMBER(12,2)")).unwrap())
        );
    }

    #[test]
    fn dameng() {
        let d = Dialect::new(Variant::Dameng);
        let t = table(["BIGINT", "VARCHAR(20)", "DECIMAL(10,2)"]);
        let diff = diff_table(None, &t, &Renames::default()).unwrap();
        insta::assert_snapshot!("dameng_create", render(&d.design_sql("HR", "", &diff).unwrap()));
        let diff = appended(everything(&t, "VARCHAR(50)"));
        insta::assert_snapshot!("dameng_everything", render(&d.design_sql("HR", "orders", &diff).unwrap()));
        insta::assert_snapshot!(
            "dameng_alter",
            render(&d.design_sql("HR", "orders", &alter_only(&t, "DECIMAL(12,2)")).unwrap())
        );
    }
}
```

- [ ] **Step 6: 新建 `crates/dblens-dialect/src/design/postgres.rs`**

```rust
//! PG 家族（PostgreSQL、金仓、openGauss、GaussDB、瀚高）的设计器 DDL（M3 设计文档 4.3 节）。
//! 每项改动一条或几条语句，整体放在一个事务中执行。

use dblens_core::Result;
use dblens_core::schema::{Column, Index, Table};

use super::TableChange as C;
use super::diff::{same_type, text};
use super::sql::{DesignStatement, stmt};
use crate::Dialect;

impl Dialect {
    /// 新列的定义：类型、生成表达式 / 默认值 / 标识列、NOT NULL。注释另用 COMMENT ON。
    fn pg_column(&self, c: &Column) -> String {
        let mut s = format!("{} {}", self.ident(&c.name), c.data_type.native);
        if let Some(g) = text(&c.generated) {
            s += &format!(" GENERATED ALWAYS AS ({g}) STORED");
        } else if c.auto_increment {
            s += " GENERATED BY DEFAULT AS IDENTITY";
        } else {
            s += &Self::default_clause(c);
        }
        if !c.nullable {
            s += " NOT NULL";
        }
        s
    }

    fn pg_column_comment(&self, q: &str, column: &str, comment: Option<&str>) -> Result<String> {
        let value = match comment {
            Some(c) => self.text_literal(c)?,
            None => "NULL".into(),
        };
        Ok(format!("COMMENT ON COLUMN {q}.{} IS {value}", self.ident(column)))
    }

    fn pg_index(&self, schema: &str, table: &str, x: &Index) -> String {
        let using = x
            .method
            .as_deref()
            .filter(|m| !m.eq_ignore_ascii_case("btree"))
            .map(|m| format!(" USING {}", m.to_ascii_lowercase()))
            .unwrap_or_default();
        format!(
            "CREATE {}INDEX {} ON {}{using} ({})",
            if x.unique { "UNIQUE " } else { "" },
            self.ident(&x.name),
            self.qualified(Some(schema), table),
            self.index_items(x)
        )
    }

    fn pg_create(&self, schema: &str, t: &Table) -> Result<Vec<String>> {
        let q = self.qualified(Some(schema), &t.name);
        let mut items: Vec<String> = t.columns.iter().map(|c| self.pg_column(c)).collect();
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
            out.push(self.pg_index(schema, &t.name, x));
        }
        if let Some(c) = text(&t.comment) {
            out.push(format!("COMMENT ON TABLE {q} IS {}", self.text_literal(c)?));
        }
        for c in &t.columns {
            if let Some(cm) = text(&c.comment) {
                out.push(self.pg_column_comment(&q, &c.name, Some(cm))?);
            }
        }
        Ok(out)
    }

    /// 修改已有列：类型、默认值、可空、标识列、注释，各一条语句。生成表达式的修改由 `design_unsupported` 挡住。
    fn pg_alter_column(&self, q: &str, from: &Column, to: &Column) -> Result<Vec<String>> {
        let col = self.ident(&to.name);
        let alter = |what: String| format!("ALTER TABLE {q} ALTER COLUMN {col} {what}");
        let mut out = Vec::new();
        if !same_type(&from.data_type.native, &to.data_type.native) {
            let t = &to.data_type.native;
            out.push(alter(format!("TYPE {t} USING {col}::{t}")));
        }
        // 标识列、生成列没有默认值；变成标识列时先去掉默认值
        let had = text(&from.default).filter(|_| !from.auto_increment && from.generated.is_none());
        let want = text(&to.default).filter(|_| !to.auto_increment && to.generated.is_none());
        if had != want {
            match want {
                Some(d) => out.push(alter(format!("SET DEFAULT {d}"))),
                None => out.push(alter("DROP DEFAULT".into())),
            }
        }
        if from.auto_increment != to.auto_increment {
            out.push(alter(
                if to.auto_increment { "ADD GENERATED BY DEFAULT AS IDENTITY" } else { "DROP IDENTITY" }
                    .into(),
            ));
        }
        if from.nullable != to.nullable {
            out.push(alter(if to.nullable { "DROP NOT NULL" } else { "SET NOT NULL" }.into()));
        }
        if text(&from.comment) != text(&to.comment) {
            out.push(self.pg_column_comment(q, &to.name, text(&to.comment))?);
        }
        Ok(out)
    }

    pub(crate) fn pg_design(&self, schema: &str, table: &str, changes: &[C]) -> Result<Vec<DesignStatement>> {
        let q = self.qualified(Some(schema), table);
        let alter = |clause: String| format!("ALTER TABLE {q} {clause}");
        let drop_constraint = |name: &Option<String>| -> Result<String> {
            Ok(alter(format!("DROP CONSTRAINT {}", self.ident(self.required_name(name)?))))
        };
        let mut out = Vec::new();
        for (i, ch) in changes.iter().enumerate() {
            let sqls: Vec<String> = match ch {
                C::CreateTable { table } => self.pg_create(schema, table)?,
                C::SetTableComment { comment } => vec![format!(
                    "COMMENT ON TABLE {q} IS {}",
                    match text(comment) {
                        Some(c) => self.text_literal(c)?,
                        None => "NULL".into(),
                    }
                )],
                C::AddColumn { column, .. } => {
                    let mut v = vec![alter(format!("ADD COLUMN {}", self.pg_column(column)))];
                    if let Some(cm) = text(&column.comment) {
                        v.push(self.pg_column_comment(&q, &column.name, Some(cm))?);
                    }
                    v
                }
                C::DropColumn { name } => vec![alter(format!("DROP COLUMN {}", self.ident(name)))],
                C::RenameColumn { from, to, .. } => {
                    vec![alter(format!("RENAME COLUMN {} TO {}", self.ident(from), self.ident(to)))]
                }
                C::AlterColumn { from, to } => self.pg_alter_column(&q, from, to)?,
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
                C::DropIndex { index } => {
                    vec![format!("DROP INDEX {}", self.qualified(Some(schema), &index.name))]
                }
                C::AddIndex { index } => vec![self.pg_index(schema, table, index)],
                C::RenameIndex { from, index } => vec![format!(
                    "ALTER INDEX {} RENAME TO {}",
                    self.qualified(Some(schema), from),
                    self.ident(&index.name)
                )],
            };
            out.extend(sqls.into_iter().map(|s| stmt(i, s)));
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::super::sql::fixtures::*;
    use crate::design::{Renames, diff_table};
    use crate::{Dialect, Variant};

    const TYPES: [&str; 3] = ["bigint", "character varying(20)", "numeric(10,2)"];

    #[test]
    fn create_and_alter() {
        let d = Dialect::new(Variant::Postgres);
        let t = table(TYPES);
        let diff = diff_table(None, &t, &Renames::default()).unwrap();
        insta::assert_snapshot!("pg_create", render(&d.design_sql("public", "", &diff).unwrap()));
        // 新列插在中间会改为重建；这里把 note 放到最后，覆盖其余每种变更
        let mut diff = everything(&t, "varchar(50)");
        for c in &mut diff.changes {
            if let crate::design::TableChange::AddColumn { after, .. } = c {
                *after = Some("order_code".into());
            }
        }
        insta::assert_snapshot!("pg_everything", render(&d.design_sql("public", "orders", &diff).unwrap()));
        insta::assert_snapshot!(
            "pg_alter",
            render(&d.design_sql("public", "orders", &alter_only(&t, "numeric(12,2)")).unwrap())
        );
    }

    #[test]
    fn identity_and_defaults() {
        let d = Dialect::new(Variant::Postgres);
        let orig = table(TYPES);
        let mut t = orig.clone();
        t.columns[0].auto_increment = false;
        t.columns[2].auto_increment = true;
        let diff = diff_table(Some(&orig), &t, &Renames::default()).unwrap();
        let sql: Vec<String> =
            d.design_sql("public", "orders", &diff).unwrap().into_iter().map(|s| s.sql).collect();
        assert_eq!(
            sql,
            [
                "ALTER TABLE public.orders ALTER COLUMN id DROP IDENTITY",
                "ALTER TABLE public.orders ALTER COLUMN amount DROP DEFAULT",
                "ALTER TABLE public.orders ALTER COLUMN amount ADD GENERATED BY DEFAULT AS IDENTITY",
            ]
        );
    }
}
```

- [ ] **Step 7: 新建 `crates/dblens-dialect/src/design/snapshots/dblens_dialect__design__mysql__tests__mysql56_everything.snap`**

```text
---
source: crates/dblens-dialect/src/design/mysql.rs
expression: "render(&old.design_sql(\"shop\", \"orders\",\n&everything(&orig, \"VARCHAR(50)\")).unwrap())"
---
[0] ALTER TABLE shop.orders DROP FOREIGN KEY fk_parent
[2] ALTER TABLE shop.orders DROP CHECK ck_amount
[3] ALTER TABLE shop.orders DROP INDEX ix_amount
[4] ALTER TABLE shop.orders DROP COLUMN amount
[5] ALTER TABLE shop.orders CHANGE COLUMN code order_code VARCHAR(50) NOT NULL
[6] ALTER TABLE shop.orders MODIFY COLUMN order_code VARCHAR(50) NOT NULL
[7] ALTER TABLE shop.orders ADD COLUMN note VARCHAR(50) NULL AFTER id
[1, 8] ALTER TABLE shop.orders DROP PRIMARY KEY, ADD PRIMARY KEY (id, order_code)
[9] ALTER TABLE shop.orders ADD CONSTRAINT uk_note UNIQUE (note)
[10] ALTER TABLE shop.orders ADD INDEX ix_note (note)
[11] ALTER TABLE shop.orders ADD CONSTRAINT fk_parent FOREIGN KEY (order_code) REFERENCES shop.parent (code)
[12] ALTER TABLE shop.orders COMMENT = '订单表'
```

- [ ] **Step 8: 新建 `crates/dblens-dialect/src/design/snapshots/dblens_dialect__design__mysql__tests__mysql56_move.snap`**

```text
---
source: crates/dblens-dialect/src/design/mysql.rs
expression: "render(&old.design_sql(\"shop\", \"orders\", &move_first(&orig)).unwrap())"
---
[0] ALTER TABLE shop.orders MODIFY COLUMN amount DECIMAL(10,2) NULL DEFAULT 0 COMMENT '金额' FIRST
[1] ALTER TABLE shop.orders DROP INDEX ix_amount, ADD INDEX ix_amount2 (amount DESC)
```

- [ ] **Step 9: 新建 `crates/dblens-dialect/src/design/snapshots/dblens_dialect__design__mysql__tests__mysql_alter.snap`**

```text
---
source: crates/dblens-dialect/src/design/mysql.rs
expression: "render(&d.design_sql(\"shop\", \"orders\",\n&alter_only(&orig, \"DECIMAL(12,2)\")).unwrap())"
---
[0] ALTER TABLE shop.orders MODIFY COLUMN code VARCHAR(20) NULL DEFAULT 'x' COMMENT '编号'
[1] ALTER TABLE shop.orders MODIFY COLUMN amount DECIMAL(12,2) NOT NULL
```

- [ ] **Step 10: 新建 `crates/dblens-dialect/src/design/snapshots/dblens_dialect__design__mysql__tests__mysql_create.snap`**

```text
---
source: crates/dblens-dialect/src/design/mysql.rs
expression: "render(&d.design_sql(\"shop\", \"\", &diff).unwrap())"
---
[0] CREATE TABLE shop.orders (
    id BIGINT NOT NULL AUTO_INCREMENT,
    code VARCHAR(20) NULL,
    amount DECIMAL(10,2) NULL DEFAULT 0 COMMENT '金额',
    PRIMARY KEY (id),
    CONSTRAINT uk_code UNIQUE (code),
    INDEX ix_amount (amount DESC),
    CONSTRAINT fk_parent FOREIGN KEY (code) REFERENCES shop.parent (code) ON DELETE CASCADE,
    CONSTRAINT ck_amount CHECK (amount >= 0)
) COMMENT = '订单'
```

- [ ] **Step 11: 新建 `crates/dblens-dialect/src/design/snapshots/dblens_dialect__design__mysql__tests__mysql_everything.snap`**

```text
---
source: crates/dblens-dialect/src/design/mysql.rs
expression: "render(&d.design_sql(\"shop\", \"orders\",\n&everything(&orig, \"VARCHAR(50)\")).unwrap())"
---
[0] ALTER TABLE shop.orders DROP FOREIGN KEY fk_parent
[2] ALTER TABLE shop.orders DROP CHECK ck_amount
[3] ALTER TABLE shop.orders DROP INDEX ix_amount
[4] ALTER TABLE shop.orders DROP COLUMN amount
[5] ALTER TABLE shop.orders RENAME COLUMN code TO order_code
[6] ALTER TABLE shop.orders MODIFY COLUMN order_code VARCHAR(50) NOT NULL
[7] ALTER TABLE shop.orders ADD COLUMN note VARCHAR(50) NULL AFTER id
[1, 8] ALTER TABLE shop.orders DROP PRIMARY KEY, ADD PRIMARY KEY (id, order_code)
[9] ALTER TABLE shop.orders ADD CONSTRAINT uk_note UNIQUE (note)
[10] ALTER TABLE shop.orders ADD INDEX ix_note (note)
[11] ALTER TABLE shop.orders ADD CONSTRAINT fk_parent FOREIGN KEY (order_code) REFERENCES shop.parent (code)
[12] ALTER TABLE shop.orders COMMENT = '订单表'
```

- [ ] **Step 12: 新建 `crates/dblens-dialect/src/design/snapshots/dblens_dialect__design__mysql__tests__mysql_move.snap`**

```text
---
source: crates/dblens-dialect/src/design/mysql.rs
expression: "render(&d.design_sql(\"shop\", \"orders\", &move_first(&orig)).unwrap())"
---
[0] ALTER TABLE shop.orders MODIFY COLUMN amount DECIMAL(10,2) NULL DEFAULT 0 COMMENT '金额' FIRST
[1] ALTER TABLE shop.orders RENAME INDEX ix_amount TO ix_amount2
```

- [ ] **Step 13: 新建 `crates/dblens-dialect/src/design/snapshots/dblens_dialect__design__oracle__tests__dameng_alter.snap`**

```text
---
source: crates/dblens-dialect/src/design/oracle.rs
expression: "render(&d.design_sql(\"HR\", \"orders\",\n&alter_only(&t, \"DECIMAL(12,2)\")).unwrap())"
---
[0] ALTER TABLE HR."orders" ALTER COLUMN "code" SET DEFAULT 'x'
[0] COMMENT ON COLUMN HR."orders"."code" IS '编号'
[1] ALTER TABLE HR."orders" MODIFY "amount" DECIMAL(12,2)
[1] ALTER TABLE HR."orders" ALTER COLUMN "amount" DROP DEFAULT
[1] ALTER TABLE HR."orders" ALTER COLUMN "amount" SET NOT NULL
[1] COMMENT ON COLUMN HR."orders"."amount" IS ''
```

- [ ] **Step 14: 新建 `crates/dblens-dialect/src/design/snapshots/dblens_dialect__design__oracle__tests__dameng_create.snap`**

```text
---
source: crates/dblens-dialect/src/design/oracle.rs
expression: "render(&d.design_sql(\"HR\", \"\", &diff).unwrap())"
---
[0] CREATE TABLE HR."orders" (
    "id" BIGINT IDENTITY(1, 1) NOT NULL,
    "code" VARCHAR(20),
    "amount" DECIMAL(10,2) DEFAULT 0,
    CONSTRAINT "pk_orders" PRIMARY KEY ("id"),
    CONSTRAINT "uk_code" UNIQUE ("code"),
    CONSTRAINT "fk_parent" FOREIGN KEY ("code") REFERENCES HR."parent" ("code") ON DELETE CASCADE,
    CONSTRAINT "ck_amount" CHECK (amount >= 0)
)
[0] CREATE INDEX HR."ix_amount" ON HR."orders" ("amount" DESC)
[0] COMMENT ON TABLE HR."orders" IS '订单'
[0] COMMENT ON COLUMN HR."orders"."amount" IS '金额'
```

- [ ] **Step 15: 新建 `crates/dblens-dialect/src/design/snapshots/dblens_dialect__design__oracle__tests__dameng_everything.snap`**

```text
---
source: crates/dblens-dialect/src/design/oracle.rs
expression: "render(&d.design_sql(\"HR\", \"orders\", &diff).unwrap())"
---
[0] ALTER TABLE HR."orders" DROP CONSTRAINT "fk_parent"
[1] ALTER TABLE HR."orders" DROP CONSTRAINT "pk_orders"
[2] ALTER TABLE HR."orders" DROP CONSTRAINT "ck_amount"
[3] DROP INDEX HR."ix_amount"
[4] ALTER TABLE HR."orders" DROP COLUMN "amount"
[5] ALTER TABLE HR."orders" RENAME COLUMN "code" TO "order_code"
[6] ALTER TABLE HR."orders" MODIFY "order_code" VARCHAR(50)
[6] ALTER TABLE HR."orders" ALTER COLUMN "order_code" SET NOT NULL
[7] ALTER TABLE HR."orders" ADD COLUMN "note" VARCHAR(50)
[8] ALTER TABLE HR."orders" ADD CONSTRAINT "pk_orders" PRIMARY KEY ("id", "order_code")
[9] ALTER TABLE HR."orders" ADD CONSTRAINT "uk_note" UNIQUE ("note")
[10] CREATE INDEX HR."ix_note" ON HR."orders" ("note")
[11] ALTER TABLE HR."orders" ADD CONSTRAINT "fk_parent" FOREIGN KEY ("order_code") REFERENCES HR."parent" ("code")
[12] COMMENT ON TABLE HR."orders" IS '订单表'
```

- [ ] **Step 16: 新建 `crates/dblens-dialect/src/design/snapshots/dblens_dialect__design__oracle__tests__oracle_alter.snap`**

```text
---
source: crates/dblens-dialect/src/design/oracle.rs
expression: "render(&d.design_sql(\"HR\", \"orders\",\n&alter_only(&t, \"NUMBER(12,2)\")).unwrap())"
---
[0] ALTER TABLE HR."orders" MODIFY ("code" DEFAULT 'x')
[0] COMMENT ON COLUMN HR."orders"."code" IS '编号'
[1] ALTER TABLE HR."orders" MODIFY ("amount" NUMBER(12,2) DEFAULT NULL NOT NULL)
[1] COMMENT ON COLUMN HR."orders"."amount" IS ''
```

- [ ] **Step 17: 新建 `crates/dblens-dialect/src/design/snapshots/dblens_dialect__design__oracle__tests__oracle_create.snap`**

```text
---
source: crates/dblens-dialect/src/design/oracle.rs
expression: "render(&d.design_sql(\"HR\", \"\", &diff).unwrap())"
---
[0] CREATE TABLE HR."orders" (
    "id" NUMBER(19) GENERATED BY DEFAULT AS IDENTITY NOT NULL,
    "code" VARCHAR2(20),
    "amount" NUMBER(10,2) DEFAULT 0,
    CONSTRAINT "pk_orders" PRIMARY KEY ("id"),
    CONSTRAINT "uk_code" UNIQUE ("code"),
    CONSTRAINT "fk_parent" FOREIGN KEY ("code") REFERENCES HR."parent" ("code") ON DELETE CASCADE,
    CONSTRAINT "ck_amount" CHECK (amount >= 0)
)
[0] CREATE INDEX HR."ix_amount" ON HR."orders" ("amount" DESC)
[0] COMMENT ON TABLE HR."orders" IS '订单'
[0] COMMENT ON COLUMN HR."orders"."amount" IS '金额'
```

- [ ] **Step 18: 新建 `crates/dblens-dialect/src/design/snapshots/dblens_dialect__design__oracle__tests__oracle_everything.snap`**

```text
---
source: crates/dblens-dialect/src/design/oracle.rs
expression: "render(&d.design_sql(\"HR\", \"orders\", &diff).unwrap())"
---
[0] ALTER TABLE HR."orders" DROP CONSTRAINT "fk_parent"
[1] ALTER TABLE HR."orders" DROP CONSTRAINT "pk_orders"
[2] ALTER TABLE HR."orders" DROP CONSTRAINT "ck_amount"
[3] DROP INDEX HR."ix_amount"
[4] ALTER TABLE HR."orders" DROP COLUMN "amount"
[5] ALTER TABLE HR."orders" RENAME COLUMN "code" TO "order_code"
[6] ALTER TABLE HR."orders" MODIFY ("order_code" VARCHAR2(50 CHAR) NOT NULL)
[7] ALTER TABLE HR."orders" ADD ("note" VARCHAR2(50 CHAR))
[8] ALTER TABLE HR."orders" ADD CONSTRAINT "pk_orders" PRIMARY KEY ("id", "order_code")
[9] ALTER TABLE HR."orders" ADD CONSTRAINT "uk_note" UNIQUE ("note")
[10] CREATE INDEX HR."ix_note" ON HR."orders" ("note")
[11] ALTER TABLE HR."orders" ADD CONSTRAINT "fk_parent" FOREIGN KEY ("order_code") REFERENCES HR."parent" ("code")
[12] COMMENT ON TABLE HR."orders" IS '订单表'
```

- [ ] **Step 19: 新建 `crates/dblens-dialect/src/design/snapshots/dblens_dialect__design__postgres__tests__pg_alter.snap`**

```text
---
source: crates/dblens-dialect/src/design/postgres.rs
expression: "render(&d.design_sql(\"public\", \"orders\",\n&alter_only(&t, \"numeric(12,2)\")).unwrap())"
---
[0] ALTER TABLE public.orders ALTER COLUMN code SET DEFAULT 'x'
[0] COMMENT ON COLUMN public.orders.code IS '编号'
[1] ALTER TABLE public.orders ALTER COLUMN amount TYPE numeric(12,2) USING amount::numeric(12,2)
[1] ALTER TABLE public.orders ALTER COLUMN amount DROP DEFAULT
[1] ALTER TABLE public.orders ALTER COLUMN amount SET NOT NULL
[1] COMMENT ON COLUMN public.orders.amount IS NULL
```

- [ ] **Step 20: 新建 `crates/dblens-dialect/src/design/snapshots/dblens_dialect__design__postgres__tests__pg_create.snap`**

```text
---
source: crates/dblens-dialect/src/design/postgres.rs
expression: "render(&d.design_sql(\"public\", \"\", &diff).unwrap())"
---
[0] CREATE TABLE public.orders (
    id bigint GENERATED BY DEFAULT AS IDENTITY NOT NULL,
    code character varying(20),
    amount numeric(10,2) DEFAULT 0,
    CONSTRAINT pk_orders PRIMARY KEY (id),
    CONSTRAINT uk_code UNIQUE (code),
    CONSTRAINT fk_parent FOREIGN KEY (code) REFERENCES public.parent (code) ON DELETE CASCADE,
    CONSTRAINT ck_amount CHECK (amount >= 0)
)
[0] CREATE INDEX ix_amount ON public.orders (amount DESC)
[0] COMMENT ON TABLE public.orders IS '订单'
[0] COMMENT ON COLUMN public.orders.amount IS '金额'
```

- [ ] **Step 21: 新建 `crates/dblens-dialect/src/design/snapshots/dblens_dialect__design__postgres__tests__pg_everything.snap`**

```text
---
source: crates/dblens-dialect/src/design/postgres.rs
expression: "render(&d.design_sql(\"public\", \"orders\", &diff).unwrap())"
---
[0] ALTER TABLE public.orders DROP CONSTRAINT fk_parent
[1] ALTER TABLE public.orders DROP CONSTRAINT pk_orders
[2] ALTER TABLE public.orders DROP CONSTRAINT ck_amount
[3] DROP INDEX public.ix_amount
[4] ALTER TABLE public.orders DROP COLUMN amount
[5] ALTER TABLE public.orders RENAME COLUMN code TO order_code
[6] ALTER TABLE public.orders ALTER COLUMN order_code TYPE varchar(50) USING order_code::varchar(50)
[6] ALTER TABLE public.orders ALTER COLUMN order_code SET NOT NULL
[7] ALTER TABLE public.orders ADD COLUMN note varchar(50)
[8] ALTER TABLE public.orders ADD CONSTRAINT pk_orders PRIMARY KEY (id, order_code)
[9] ALTER TABLE public.orders ADD CONSTRAINT uk_note UNIQUE (note)
[10] CREATE INDEX ix_note ON public.orders (note)
[11] ALTER TABLE public.orders ADD CONSTRAINT fk_parent FOREIGN KEY (order_code) REFERENCES public.parent (code)
[12] COMMENT ON TABLE public.orders IS '订单表'
```

- [ ] **Step 22: 新建 `crates/dblens-dialect/src/design/sql.rs`**

```rust
//! 设计器的 DDL 生成（M3 设计文档第 4 节）：判断库不支持的改动，按方言把变更列表生成为语句。
//! 每项变更生成一条或几条语句，语句带着它对应的变更序号，预览时可以对照。

use dblens_core::schema::{CheckConstraint, Column, ForeignKey, Index};
use dblens_core::{Error, Msg, Result, Value, msg};
use serde::Serialize;

use super::diff::{is_expression, text};
use super::{Diff, TableChange as C};
use crate::{Dialect, Family};

/// 一条生成的语句。`changes` 是它对应的变更序号（一项变更也可能生成几条语句）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DesignStatement {
    pub changes: Vec<usize>,
    pub sql: String,
}

pub(crate) fn stmt(change: usize, sql: String) -> DesignStatement {
    DesignStatement { changes: vec![change], sql }
}

impl Dialect {
    /// 整个改动能否放在一个事务中执行：只有 PG 家族的 DDL 可以回滚。
    pub fn design_transactional(&self) -> bool {
        self.family() == Family::Postgres
    }

    /// 库确定不支持的改动及原因；不为空时改用重建表。
    pub fn design_unsupported(&self, diff: &Diff) -> Vec<Msg> {
        let mut out = Vec::new();
        if !self.design_caps().reorder && !diff.appended {
            out.push(msg!("design.unsupported.reorder"));
        }
        for ch in &diff.changes {
            let C::AlterColumn { from, to } = ch else { continue };
            let generated = text(&from.generated) != text(&to.generated);
            let identity = from.auto_increment != to.auto_increment;
            match self.family() {
                Family::MySql => {}
                Family::Postgres => {
                    if generated {
                        out.push(msg!("design.unsupported.generated", column = to.name));
                    }
                    let serial = text(&from.default).is_some_and(|d| d.starts_with("nextval("));
                    if identity && serial {
                        out.push(msg!("design.unsupported.serial", column = to.name));
                    }
                }
                Family::Oracle | Family::Dameng => {
                    if generated {
                        out.push(msg!("design.unsupported.generated", column = to.name));
                    }
                    if identity {
                        out.push(msg!("design.unsupported.identity", column = to.name));
                    }
                }
            }
        }
        out
    }

    /// 按方言生成 DDL。`table` 是表的现名（新建表时不用）。
    pub fn design_sql(&self, schema: &str, table: &str, diff: &Diff) -> Result<Vec<DesignStatement>> {
        match self.family() {
            Family::MySql => self.mysql_design(schema, table, &diff.changes),
            Family::Postgres => self.pg_design(schema, table, &diff.changes),
            Family::Oracle | Family::Dameng => self.oracle_design(schema, table, &diff.changes),
        }
    }

    pub(crate) fn idents(&self, cols: &[String]) -> String {
        cols.iter().map(|c| self.ident(c)).collect::<Vec<_>>().join(", ")
    }

    /// `CONSTRAINT 名字 `，没有名字时为空。
    pub(crate) fn constraint_name(&self, name: &Option<String>) -> String {
        text(name).map(|n| format!("CONSTRAINT {} ", self.ident(n))).unwrap_or_default()
    }

    /// 约束必须有名字才能删除（从库中读出的约束都有）。
    pub(crate) fn required_name<'a>(&self, name: &'a Option<String>) -> Result<&'a str> {
        text(name).ok_or_else(|| Error::invalid(msg!("design.unnamed_constraint")))
    }

    pub(crate) fn text_literal(&self, s: &str) -> Result<String> {
        self.literal(&Value::String(s.into()))
    }

    /// `FOREIGN KEY (…) REFERENCES 表 (…) [ON DELETE …] [ON UPDATE …]`；未写 schema 时引用同一 schema 中的表。
    pub(crate) fn fk_clause(&self, schema: &str, f: &ForeignKey) -> String {
        let mut s = format!(
            "FOREIGN KEY ({}) REFERENCES {} ({})",
            self.idents(&f.columns),
            self.qualified(text(&f.ref_schema).or(Some(schema)), &f.ref_table),
            self.idents(&f.ref_columns)
        );
        // Oracle、达梦没有 ON UPDATE
        let on_update =
            if matches!(self.family(), Family::Oracle | Family::Dameng) { &None } else { &f.on_update };
        for (what, action) in [("DELETE", &f.on_delete), ("UPDATE", on_update)] {
            if let Some(a) = text(action).map(str::to_ascii_uppercase).filter(|a| a != "NO ACTION") {
                s += &format!(" ON {what} {a}");
            }
        }
        s
    }

    pub(crate) fn check_clause(&self, c: &CheckConstraint) -> String {
        let e = c.expression.trim();
        let e = if e.starts_with('(') && e.ends_with(')') { e.to_string() } else { format!("({e})") };
        format!("{}CHECK {e}", self.constraint_name(&c.name))
    }

    /// 索引的列清单：普通列加引号，表达式原样（MySQL 的函数索引要再套一层括号）。
    pub(crate) fn index_items(&self, x: &Index) -> String {
        x.columns
            .iter()
            .map(|c| {
                let name = if !is_expression(&c.name) {
                    self.ident(&c.name)
                } else if self.family() == Family::MySql && !c.name.starts_with('(') {
                    format!("({})", c.name)
                } else {
                    c.name.clone()
                };
                if c.descending { format!("{name} DESC") } else { name }
            })
            .collect::<Vec<_>>()
            .join(", ")
    }

    /// 列的默认值写法（MySQL 之外：生成列和自增列不写默认值）。
    pub(crate) fn default_clause(c: &Column) -> String {
        match text(&c.default) {
            Some(d) if c.generated.is_none() && !c.auto_increment => format!(" DEFAULT {d}"),
            _ => String::new(),
        }
    }
}

#[cfg(test)]
pub(crate) mod fixtures {
    //! 各方言生成测试共用的表。
    use dblens_core::schema::{
        CheckConstraint, Column, ForeignKey, Index, IndexColumn, PrimaryKey, Table, UniqueConstraint,
    };
    use dblens_core::{ColumnType, LogicalType};

    use crate::design::{Diff, Renames, diff_table};

    pub fn col(name: &str, native: &str) -> Column {
        Column {
            name: name.into(),
            ordinal: 0,
            data_type: ColumnType { native: native.into(), logical: LogicalType::Other },
            nullable: true,
            default: None,
            comment: None,
            auto_increment: false,
            generated: None,
        }
    }

    /// 一张带全部种类约束和索引的表；`types` 依次是 id、code、amount 的类型。
    pub fn table(types: [&str; 3]) -> Table {
        let mut id = col("id", types[0]);
        id.nullable = false;
        id.auto_increment = true;
        let mut amount = col("amount", types[2]);
        amount.default = Some("0".into());
        amount.comment = Some("金额".into());
        Table {
            name: "orders".into(),
            comment: Some("订单".into()),
            columns: vec![id, col("code", types[1]), amount],
            primary_key: Some(PrimaryKey { name: Some("pk_orders".into()), columns: vec!["id".into()] }),
            uniques: vec![UniqueConstraint { name: Some("uk_code".into()), columns: vec!["code".into()] }],
            checks: vec![CheckConstraint {
                name: Some("ck_amount".into()),
                expression: "amount >= 0".into(),
            }],
            foreign_keys: vec![ForeignKey {
                name: Some("fk_parent".into()),
                columns: vec!["code".into()],
                ref_schema: None,
                ref_table: "parent".into(),
                ref_columns: vec!["code".into()],
                on_delete: Some("CASCADE".into()),
                on_update: None,
            }],
            indexes: vec![Index {
                name: "ix_amount".into(),
                columns: vec![IndexColumn { name: "amount".into(), descending: true }],
                unique: false,
                method: None,
            }],
            ..Default::default()
        }
    }

    /// 覆盖每种变更的一组改动：删检查约束、删索引、删列 amount、改名 code → order_code 并改类型、
    /// 加列 note（放在 id 之后）、换主键、加唯一约束、加索引、索引改名、换外键、改表注释。
    pub fn everything(orig: &Table, varchar: &str) -> Diff {
        let mut d = orig.clone();
        d.checks.clear();
        d.columns.retain(|c| c.name != "amount");
        d.indexes.clear();
        d.columns[1].name = "order_code".into();
        d.columns[1].data_type.native = varchar.into();
        d.columns[1].nullable = false;
        d.columns.insert(1, col("note", varchar));
        d.primary_key = Some(PrimaryKey {
            name: Some("pk_orders".into()),
            columns: vec!["id".into(), "order_code".into()],
        });
        d.uniques =
            vec![UniqueConstraint { name: Some("uk_code".into()), columns: vec!["order_code".into()] }];
        d.uniques.push(UniqueConstraint { name: Some("uk_note".into()), columns: vec!["note".into()] });
        d.indexes.push(Index {
            name: "ix_note".into(),
            columns: vec![IndexColumn { name: "note".into(), descending: false }],
            unique: false,
            method: None,
        });
        d.foreign_keys[0].columns = vec!["order_code".into()];
        d.foreign_keys[0].on_delete = None;
        d.comment = Some("订单表".into());
        let renames =
            Renames { columns: [("order_code".into(), "code".into())].into(), ..Default::default() };
        diff_table(Some(orig), &d, &renames).unwrap()
    }

    /// 只改列的属性：可空、默认值、注释、类型。
    pub fn alter_only(orig: &Table, numeric: &str) -> Diff {
        let mut d = orig.clone();
        let amount = &mut d.columns[2];
        amount.data_type.native = numeric.into();
        amount.nullable = false;
        amount.default = None;
        amount.comment = None;
        d.columns[1].default = Some("'x'".into());
        d.columns[1].comment = Some("编号".into());
        diff_table(Some(orig), &d, &Renames::default()).unwrap()
    }

    /// 只调整列顺序：amount 挪到最前，并把它的索引改名。
    pub fn move_first(orig: &Table) -> Diff {
        let mut d = orig.clone();
        let amount = d.columns.pop().unwrap();
        d.columns.insert(0, amount);
        d.indexes[0].name = "ix_amount2".into();
        let renames =
            Renames { indexes: [("ix_amount2".into(), "ix_amount".into())].into(), ..Default::default() };
        diff_table(Some(orig), &d, &renames).unwrap()
    }

    /// 生成结果拼成一段文字（每条一行，带对应的变更序号），便于快照比较。
    pub fn render(statements: &[crate::design::DesignStatement]) -> String {
        statements.iter().map(|s| format!("{:?} {}", s.changes, s.sql)).collect::<Vec<_>>().join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::fixtures::*;
    use super::*;
    use crate::Variant;

    #[test]
    fn unsupported_changes_by_family() {
        let pg = Dialect::new(Variant::Postgres);
        let orig = table(["bigint", "varchar(20)", "numeric(10,2)"]);
        assert!(pg.design_unsupported(&alter_only(&orig, "numeric(12,2)")).is_empty());
        assert_eq!(pg.design_unsupported(&move_first(&orig)), vec![msg!("design.unsupported.reorder")]);
        // 新列插在中间
        assert_eq!(
            pg.design_unsupported(&everything(&orig, "varchar(50)")),
            vec![msg!("design.unsupported.reorder")]
        );
        let mysql = Dialect::new(Variant::MySql);
        assert!(mysql.design_unsupported(&move_first(&orig)).is_empty());

        // 改变自增：PG 的 serial 列、Oracle 和达梦的已有列不支持
        let mut serial = orig.clone();
        serial.columns[0].default = Some("nextval('orders_id_seq'::regclass)".into());
        let mut d = serial.clone();
        d.columns[0].auto_increment = false;
        let diff = crate::design::diff_table(Some(&serial), &d, &Default::default()).unwrap();
        assert_eq!(pg.design_unsupported(&diff), vec![msg!("design.unsupported.serial", column = "id")]);
        let mut d = orig.clone();
        d.columns[0].auto_increment = false;
        let diff = crate::design::diff_table(Some(&orig), &d, &Default::default()).unwrap();
        assert!(pg.design_unsupported(&diff).is_empty());
        assert_eq!(
            Dialect::new(Variant::Oracle).design_unsupported(&diff),
            vec![msg!("design.unsupported.identity", column = "id")]
        );
        // 生成列的表达式
        let mut d = orig.clone();
        d.columns[1].generated = Some("upper(code)".into());
        let diff = crate::design::diff_table(Some(&orig), &d, &Default::default()).unwrap();
        assert_eq!(pg.design_unsupported(&diff), vec![msg!("design.unsupported.generated", column = "code")]);
        assert!(mysql.design_unsupported(&diff).is_empty());
        assert!(pg.design_transactional() && !mysql.design_transactional());
    }

    #[test]
    fn every_generated_statement_splits_into_one() {
        let types = ["BIGINT", "VARCHAR(20)", "DECIMAL(10,2)"];
        for v in [Variant::MySql, Variant::MariaDb, Variant::Postgres, Variant::Oracle, Variant::Dameng] {
            let d = Dialect::new(v);
            let mut t = table(types);
            // 注释、默认值里带分号、注释符和 `/`
            t.comment = Some(
                "a;b -- c
/
"
                .into(),
            );
            t.columns[1].default = Some("';--'".into());
            let create = crate::design::diff_table(None, &t, &Default::default()).unwrap();
            for diff in
                [create, everything(&t, "VARCHAR(50)"), alter_only(&t, "DECIMAL(12,2)"), move_first(&t)]
            {
                for s in d.design_sql("s", "orders", &diff).unwrap() {
                    let parts = d.split(&s.sql);
                    assert_eq!(parts.len(), 1, "{v:?}: {}", s.sql);
                    assert_eq!(parts[0].text, s.sql, "{v:?}");
                }
            }
        }
    }
}
```

- [ ] **Step 23: 检查**

```bash
export CARGO_INCREMENTAL=0
cargo fmt --all
cargo clippy --config "source.mirror.registry='sparse+https://rsproxy.cn/index/'" --workspace --all-targets -- -D warnings
DBLENS_LANG=en cargo test --config "source.mirror.registry='sparse+https://rsproxy.cn/index/'" --workspace
```

Expected: 全部测试通过（dialect 新增 10 个，快照 15 个）。

- [ ] **Step 24: 提交**

```bash
git add crates/dblens-core/locales/app.json crates/dblens-dialect/src/design/diff.rs crates/dblens-dialect/src/design/mod.rs crates/dblens-dialect/src/design/mysql.rs crates/dblens-dialect/src/design/oracle.rs crates/dblens-dialect/src/design/postgres.rs crates/dblens-dialect/src/design/snapshots/dblens_dialect__design__mysql__tests__mysql56_everything.snap crates/dblens-dialect/src/design/snapshots/dblens_dialect__design__mysql__tests__mysql56_move.snap crates/dblens-dialect/src/design/snapshots/dblens_dialect__design__mysql__tests__mysql_alter.snap crates/dblens-dialect/src/design/snapshots/dblens_dialect__design__mysql__tests__mysql_create.snap crates/dblens-dialect/src/design/snapshots/dblens_dialect__design__mysql__tests__mysql_everything.snap crates/dblens-dialect/src/design/snapshots/dblens_dialect__design__mysql__tests__mysql_move.snap crates/dblens-dialect/src/design/snapshots/dblens_dialect__design__oracle__tests__dameng_alter.snap crates/dblens-dialect/src/design/snapshots/dblens_dialect__design__oracle__tests__dameng_create.snap crates/dblens-dialect/src/design/snapshots/dblens_dialect__design__oracle__tests__dameng_everything.snap crates/dblens-dialect/src/design/snapshots/dblens_dialect__design__oracle__tests__oracle_alter.snap crates/dblens-dialect/src/design/snapshots/dblens_dialect__design__oracle__tests__oracle_create.snap crates/dblens-dialect/src/design/snapshots/dblens_dialect__design__oracle__tests__oracle_everything.snap crates/dblens-dialect/src/design/snapshots/dblens_dialect__design__postgres__tests__pg_alter.snap crates/dblens-dialect/src/design/snapshots/dblens_dialect__design__postgres__tests__pg_create.snap crates/dblens-dialect/src/design/snapshots/dblens_dialect__design__postgres__tests__pg_everything.snap crates/dblens-dialect/src/design/sql.rs
git commit -m "feat(dialect): 设计器按方言生成 DDL（MySQL、PG、Oracle、达梦）" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 5: 方言：重建表的脚本与所需的元数据查询

- 元数据查询模板（`sql/*.sql`）新增：`referencing_keys`（其他表指向这张表的外键）、`dependent_views`（PG、Oracle、达梦）、`grants`（PG、Oracle、达梦；MySQL 的授权按表名记录）、`table_storage`（引擎、字符集、排序规则、表空间、行数估计）、`trigger_ddl`（只有 Oracle：由 `ALL_TRIGGERS.DESCRIPTION` 拼出完整的 CREATE TRIGGER）。`MetaKey` 增加对应的 5 个 key，`meta` 增加 `ReferencingKey`、`DependentView`、`Grant`、`TableStorage` 和组装函数。
- `design/rebuild.rs`：`Dialect::rebuild(schema, original, draft, renames, &RebuildContext) -> Result<RebuildOutcome>`。
  - 拒绝：分区表；PG 家族有视图依赖这张表；其他表的外键引用的列被删掉。
  - 步骤：建 `<表名>_dblens_new`（超长时截断加短哈希；只有列，MySQL 带主键；保留引擎、字符集或表空间；注释建在临时表上）→ `INSERT … SELECT`（类型变了的套 CAST，MySQL 靠隐式转换；达梦用 `SET IDENTITY_INSERT`）→ 核对行数（`CountCheck`，由程序执行）→ 删其他表指向本表的外键 → 删旧表（`drop_at`）、改回原名 → 主键、唯一、检查、索引、外键、触发器 → 其他表的外键（引用列随改名）→ 自增接续（PG `setval`，Oracle `START WITH LIMIT VALUE`）、授权、Oracle 和达梦重新编译视图。
  - `cleanup`：`drop_at` 之前失败或取消时删除新表。

后端词条加 `design.rebuild.*`（6 个）。

快照文件（`src/design/snapshots/`、`tests/snapshots/`）随本任务给出；改了生成规则时用 `INSTA_UPDATE=always` 重新生成并逐个审查差异。

**Files:**
- Modify: `crates/dblens-core/locales/app.json`
- Modify: `crates/dblens-dialect/sql/dameng.sql`
- Modify: `crates/dblens-dialect/sql/mysql.sql`
- Modify: `crates/dblens-dialect/sql/oracle.sql`
- Modify: `crates/dblens-dialect/sql/postgres.sql`
- Modify: `crates/dblens-dialect/src/design/mod.rs`
- Create: `crates/dblens-dialect/src/design/rebuild.rs`
- Create: `crates/dblens-dialect/src/design/snapshots/dblens_dialect__design__rebuild__tests__rebuild_dameng.snap`
- Create: `crates/dblens-dialect/src/design/snapshots/dblens_dialect__design__rebuild__tests__rebuild_mysql.snap`
- Create: `crates/dblens-dialect/src/design/snapshots/dblens_dialect__design__rebuild__tests__rebuild_oracle.snap`
- Create: `crates/dblens-dialect/src/design/snapshots/dblens_dialect__design__rebuild__tests__rebuild_postgres.snap`
- Modify: `crates/dblens-dialect/src/meta/assemble.rs`
- Modify: `crates/dblens-dialect/src/meta/mod.rs`
- Modify: `crates/dblens-dialect/tests/meta_snapshots.rs`
- Modify: `crates/dblens-dialect/tests/snapshots/meta_snapshots__dameng.snap`
- Modify: `crates/dblens-dialect/tests/snapshots/meta_snapshots__mariadb.snap`
- Modify: `crates/dblens-dialect/tests/snapshots/meta_snapshots__mysql.snap`
- Modify: `crates/dblens-dialect/tests/snapshots/meta_snapshots__oracle.snap`
- Modify: `crates/dblens-dialect/tests/snapshots/meta_snapshots__postgres.snap`

**Interfaces:**
- Consumes：任务 4 的 `design_sql` 等。
- Produces：
  - `dblens_dialect::meta::{MetaKey::{ReferencingKeys, DependentViews, Grants, TableStorage, TriggerDdl}, ReferencingKey { schema, table, key }, DependentView { schema, name, materialized }, Grant { grantee, privilege, grantable }, TableStorage { engine, charset, collation, tablespace, row_count }, referencing_keys(&MetaRows, ref_schema, ref_table), dependent_views, grants, table_storage, trigger_ddl}`；
  - `dblens_dialect::design::{RebuildContext { referencing, views, grants, storage, triggers }, RebuildStep::{Sql { sql }, CountCheck { source, target }}, Rebuild { steps, drop_at, cleanup, warnings }, RebuildOutcome::{Script(Rebuild), Refused { reason }}}`、`Dialect::rebuild(...)`。

- [ ] **Step 1: 修改 `crates/dblens-dialect/tests/meta_snapshots.rs`**

```diff
--- a/crates/dblens-dialect/tests/meta_snapshots.rs
+++ b/crates/dblens-dialect/tests/meta_snapshots.rs
@@ -38,6 +38,8 @@ fn every_family_covers_the_common_keys() {
                 | MetaKey::Synonyms
                 | MetaKey::Sequences
                 | MetaKey::VariantProbe => true,
+                MetaKey::DependentViews | MetaKey::Grants => matches!(v, Variant::MySql | Variant::MariaDb),
+                MetaKey::TriggerDdl => v != Variant::Oracle,
                 MetaKey::ReservedWords => v == Variant::MariaDb,
                 _ => false,
             };
```

- [ ] **Step 2: 修改 `crates/dblens-dialect/tests/snapshots/meta_snapshots__dameng.snap`**

```diff
--- a/crates/dblens-dialect/tests/snapshots/meta_snapshots__dameng.snap
+++ b/crates/dblens-dialect/tests/snapshots/meta_snapshots__dameng.snap
@@ -265,3 +265,54 @@ SELECT 'VERSIONS_ENDTRXID' FROM DUAL
 UNION
 SELECT 'VERSIONS_OPERATION' FROM DUAL
 ORDER BY 1
+
+-- [referencing_keys] [String("HR"), String("EMP")]
+-- Table designer rebuild: foreign keys of OTHER tables that reference this table's PK / UNIQUE constraints.
+SELECT c.OWNER AS "table_schema",
+       c.TABLE_NAME AS "table_name",
+       c.CONSTRAINT_NAME AS "constraint_name",
+       cc.COLUMN_NAME AS "column_name",
+       CAST(cc.POSITION AS INT) AS "position",
+       rc.COLUMN_NAME AS "ref_column",
+       COALESCE(c.DELETE_RULE, 'NO ACTION') AS "on_delete",
+       'NO ACTION' AS "on_update"
+FROM ALL_CONSTRAINTS c
+JOIN ALL_CONSTRAINTS p ON p.OWNER = c.R_OWNER AND p.CONSTRAINT_NAME = c.R_CONSTRAINT_NAME
+JOIN ALL_CONS_COLUMNS cc ON cc.OWNER = c.OWNER AND cc.CONSTRAINT_NAME = c.CONSTRAINT_NAME AND cc.TABLE_NAME = c.TABLE_NAME
+JOIN ALL_CONS_COLUMNS rc ON rc.OWNER = p.OWNER AND rc.CONSTRAINT_NAME = p.CONSTRAINT_NAME
+                        AND rc.TABLE_NAME = p.TABLE_NAME AND rc.POSITION = cc.POSITION
+WHERE c.CONSTRAINT_TYPE = 'R'
+  AND p.OWNER = ?
+  AND p.TABLE_NAME = ?
+  AND NOT (c.OWNER = p.OWNER AND c.TABLE_NAME = p.TABLE_NAME)
+ORDER BY c.OWNER, c.TABLE_NAME, c.CONSTRAINT_NAME, cc.POSITION
+
+-- [dependent_views] [String("HR"), String("EMP")]
+SELECT d.OWNER AS "schema_name",
+       d.NAME AS "name",
+       d.TYPE AS "kind"
+FROM ALL_DEPENDENCIES d
+WHERE d.REFERENCED_OWNER = ?
+  AND d.REFERENCED_NAME = ?
+  AND d.REFERENCED_TYPE = 'TABLE'
+  AND d.TYPE IN ('VIEW', 'MATERIALIZED VIEW')
+ORDER BY d.OWNER, d.NAME
+
+-- [grants] [String("HR"), String("EMP")]
+SELECT p.GRANTEE AS "grantee",
+       p.PRIVILEGE AS "privilege",
+       p.GRANTABLE AS "grantable"
+FROM ALL_TAB_PRIVS p
+WHERE p.TABLE_SCHEMA = ?
+  AND p.TABLE_NAME = ?
+ORDER BY p.GRANTEE, p.PRIVILEGE
+
+-- [table_storage] [String("HR"), String("EMP")]
+SELECT CAST(NULL AS VARCHAR(1)) AS "engine",
+       CAST(NULL AS VARCHAR(1)) AS "charset",
+       CAST(NULL AS VARCHAR(1)) AS "collation",
+       t.TABLESPACE_NAME AS "tablespace",
+       t.NUM_ROWS AS "row_count"
+FROM ALL_TABLES t
+WHERE t.OWNER = ?
+  AND t.TABLE_NAME = ?
```

- [ ] **Step 3: 修改 `crates/dblens-dialect/tests/snapshots/meta_snapshots__mariadb.snap`**

```diff
--- a/crates/dblens-dialect/tests/snapshots/meta_snapshots__mariadb.snap
+++ b/crates/dblens-dialect/tests/snapshots/meta_snapshots__mariadb.snap
@@ -176,3 +176,35 @@ WHERE r.ROUTINE_SCHEMA = ?
 
 -- [variant_probe] []
 SHOW VARIABLES LIKE 'polardb_version'
+
+-- [referencing_keys] [String("HR"), String("EMP")]
+-- Table designer rebuild: foreign keys of OTHER tables that reference this table (dropped and recreated around the swap).
+SELECT k.TABLE_SCHEMA AS table_schema,
+       k.TABLE_NAME AS table_name,
+       k.CONSTRAINT_NAME AS constraint_name,
+       k.COLUMN_NAME AS column_name,
+       k.ORDINAL_POSITION AS position,
+       k.REFERENCED_COLUMN_NAME AS ref_column,
+       rc.DELETE_RULE AS on_delete,
+       rc.UPDATE_RULE AS on_update
+FROM information_schema.KEY_COLUMN_USAGE k
+JOIN information_schema.REFERENTIAL_CONSTRAINTS rc
+  ON rc.CONSTRAINT_SCHEMA = k.CONSTRAINT_SCHEMA
+ AND rc.TABLE_NAME = k.TABLE_NAME
+ AND rc.CONSTRAINT_NAME = k.CONSTRAINT_NAME
+WHERE k.REFERENCED_TABLE_SCHEMA = ?
+  AND k.REFERENCED_TABLE_NAME = ?
+  AND NOT (k.TABLE_SCHEMA = k.REFERENCED_TABLE_SCHEMA AND k.TABLE_NAME = k.REFERENCED_TABLE_NAME)
+ORDER BY k.TABLE_SCHEMA, k.TABLE_NAME, k.CONSTRAINT_NAME, k.ORDINAL_POSITION
+
+-- [table_storage] [String("HR"), String("EMP")]
+-- Table designer: engine / charset / collation kept by a rebuild, and the row estimate shown in the designer.
+SELECT t.ENGINE AS engine,
+       (SELECT MAX(c.CHARACTER_SET_NAME) FROM information_schema.COLLATION_CHARACTER_SET_APPLICABILITY c
+         WHERE c.COLLATION_NAME = t.TABLE_COLLATION) AS charset,
+       t.TABLE_COLLATION AS collation,
+       NULL AS tablespace,
+       t.TABLE_ROWS AS row_count
+FROM information_schema.TABLES t
+WHERE t.TABLE_SCHEMA = ?
+  AND t.TABLE_NAME = ?
```

- [ ] **Step 4: 修改 `crates/dblens-dialect/tests/snapshots/meta_snapshots__mysql.snap`**

```diff
--- a/crates/dblens-dialect/tests/snapshots/meta_snapshots__mysql.snap
+++ b/crates/dblens-dialect/tests/snapshots/meta_snapshots__mysql.snap
@@ -179,3 +179,35 @@ ORDER BY k.WORD
 
 -- [variant_probe] []
 SHOW VARIABLES LIKE 'polardb_version'
+
+-- [referencing_keys] [String("HR"), String("EMP")]
+-- Table designer rebuild: foreign keys of OTHER tables that reference this table (dropped and recreated around the swap).
+SELECT k.TABLE_SCHEMA AS table_schema,
+       k.TABLE_NAME AS table_name,
+       k.CONSTRAINT_NAME AS constraint_name,
+       k.COLUMN_NAME AS column_name,
+       k.ORDINAL_POSITION AS position,
+       k.REFERENCED_COLUMN_NAME AS ref_column,
+       rc.DELETE_RULE AS on_delete,
+       rc.UPDATE_RULE AS on_update
+FROM information_schema.KEY_COLUMN_USAGE k
+JOIN information_schema.REFERENTIAL_CONSTRAINTS rc
+  ON rc.CONSTRAINT_SCHEMA = k.CONSTRAINT_SCHEMA
+ AND rc.TABLE_NAME = k.TABLE_NAME
+ AND rc.CONSTRAINT_NAME = k.CONSTRAINT_NAME
+WHERE k.REFERENCED_TABLE_SCHEMA = ?
+  AND k.REFERENCED_TABLE_NAME = ?
+  AND NOT (k.TABLE_SCHEMA = k.REFERENCED_TABLE_SCHEMA AND k.TABLE_NAME = k.REFERENCED_TABLE_NAME)
+ORDER BY k.TABLE_SCHEMA, k.TABLE_NAME, k.CONSTRAINT_NAME, k.ORDINAL_POSITION
+
+-- [table_storage] [String("HR"), String("EMP")]
+-- Table designer: engine / charset / collation kept by a rebuild, and the row estimate shown in the designer.
+SELECT t.ENGINE AS engine,
+       (SELECT MAX(c.CHARACTER_SET_NAME) FROM information_schema.COLLATION_CHARACTER_SET_APPLICABILITY c
+         WHERE c.COLLATION_NAME = t.TABLE_COLLATION) AS charset,
+       t.TABLE_COLLATION AS collation,
+       NULL AS tablespace,
+       t.TABLE_ROWS AS row_count
+FROM information_schema.TABLES t
+WHERE t.TABLE_SCHEMA = ?
+  AND t.TABLE_NAME = ?
```

- [ ] **Step 5: 修改 `crates/dblens-dialect/tests/snapshots/meta_snapshots__oracle.snap`**

```diff
--- a/crates/dblens-dialect/tests/snapshots/meta_snapshots__oracle.snap
+++ b/crates/dblens-dialect/tests/snapshots/meta_snapshots__oracle.snap
@@ -223,3 +223,66 @@ FROM v$reserved_words r
 WHERE (r.reserved = 'Y' OR r.res_semi = 'Y' OR r.res_attr = 'Y')
   AND REGEXP_LIKE(r.keyword, '^[A-Z][A-Z0-9_$#]*$')
 ORDER BY 1
+
+-- [referencing_keys] [String("HR"), String("EMP")]
+-- Table designer rebuild: foreign keys of OTHER tables that reference this table's PK / UNIQUE constraints.
+SELECT c.owner AS table_schema,
+       c.table_name AS table_name,
+       c.constraint_name AS constraint_name,
+       cc.column_name AS column_name,
+       cc.position AS position,
+       rc.column_name AS ref_column,
+       c.delete_rule AS on_delete,
+       CAST(NULL AS VARCHAR2(9)) AS on_update
+FROM all_constraints c
+JOIN all_constraints p ON p.owner = c.r_owner AND p.constraint_name = c.r_constraint_name
+JOIN all_cons_columns cc ON cc.owner = c.owner AND cc.constraint_name = c.constraint_name AND cc.table_name = c.table_name
+JOIN all_cons_columns rc ON rc.owner = p.owner AND rc.constraint_name = p.constraint_name AND rc.position = cc.position
+WHERE c.constraint_type = 'R'
+  AND p.owner = :1
+  AND p.table_name = :2
+  AND NOT (c.owner = p.owner AND c.table_name = p.table_name)
+ORDER BY c.owner, c.table_name, c.constraint_name, cc.position
+
+-- [dependent_views] [String("HR"), String("EMP")]
+SELECT d.owner AS schema_name,
+       d.name AS name,
+       d.type AS kind
+FROM all_dependencies d
+WHERE d.referenced_owner = :1
+  AND d.referenced_name = :2
+  AND d.referenced_type = 'TABLE'
+  AND d.type IN ('VIEW', 'MATERIALIZED VIEW')
+ORDER BY d.owner, d.name
+
+-- [grants] [String("HR"), String("EMP")]
+SELECT p.grantee AS grantee,
+       p.privilege AS privilege,
+       p.grantable AS grantable
+FROM all_tab_privs p
+WHERE p.table_schema = :1
+  AND p.table_name = :2
+ORDER BY p.grantee, p.privilege
+
+-- [table_storage] [String("HR"), String("EMP")]
+SELECT CAST(NULL AS VARCHAR2(1)) AS engine,
+       CAST(NULL AS VARCHAR2(1)) AS charset,
+       CAST(NULL AS VARCHAR2(1)) AS collation,
+       t.tablespace_name AS tablespace,
+       t.num_rows AS row_count
+FROM all_tables t
+WHERE t.owner = :1
+  AND t.table_name = :2
+
+-- [trigger_ddl] [String("HR"), String("EMP")]
+-- Full CREATE TRIGGER rebuilt from ALL_TRIGGERS (triggers.definition holds the body only). DESCRIPTION starts with the
+-- trigger name and the ON clause as written, unqualified: run it with the table's schema as the current schema.
+SELECT t.trigger_name AS name,
+       'CREATE OR REPLACE TRIGGER ' || t.description
+         || CASE WHEN t.when_clause IS NOT NULL THEN 'WHEN (' || t.when_clause || ')' || CHR(10) END AS head,
+       t.trigger_body AS body
+FROM all_triggers t
+WHERE t.table_owner = :1
+  AND t.table_name = :2
+  AND t.base_object_type = 'TABLE'
+ORDER BY t.trigger_name
```

- [ ] **Step 6: 修改 `crates/dblens-dialect/tests/snapshots/meta_snapshots__postgres.snap`**

```diff
--- a/crates/dblens-dialect/tests/snapshots/meta_snapshots__postgres.snap
+++ b/crates/dblens-dialect/tests/snapshots/meta_snapshots__postgres.snap
@@ -226,3 +226,74 @@ ORDER BY line
 -- [reserved_words] []
 -- catcode R = reserved, T = reserved (can be function or type name). C (col_name) and U are not reserved.
 SELECT word FROM pg_catalog.pg_get_keywords() WHERE catcode IN ('R', 'T') ORDER BY word
+
+-- [referencing_keys] [String("HR"), String("EMP")]
+-- Table designer rebuild: foreign keys of OTHER tables that reference this table.
+SELECT n.nspname AS table_schema,
+       c.relname AS table_name,
+       con.conname AS constraint_name,
+       a.attname AS column_name,
+       k.pos::int AS position,
+       ra.attname AS ref_column,
+       CASE con.confdeltype WHEN 'c' THEN 'CASCADE' WHEN 'n' THEN 'SET NULL' WHEN 'd' THEN 'SET DEFAULT'
+                         WHEN 'r' THEN 'RESTRICT' ELSE 'NO ACTION' END AS on_delete,
+       CASE con.confupdtype WHEN 'c' THEN 'CASCADE' WHEN 'n' THEN 'SET NULL' WHEN 'd' THEN 'SET DEFAULT'
+                         WHEN 'r' THEN 'RESTRICT' ELSE 'NO ACTION' END AS on_update
+FROM pg_catalog.pg_constraint con
+JOIN pg_catalog.pg_class c ON c.oid = con.conrelid
+JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace
+JOIN pg_catalog.pg_class t ON t.oid = con.confrelid
+JOIN pg_catalog.pg_namespace tn ON tn.oid = t.relnamespace
+CROSS JOIN LATERAL pg_catalog.unnest(con.conkey) WITH ORDINALITY AS k(attnum, pos)
+JOIN pg_catalog.pg_attribute a ON a.attrelid = con.conrelid AND a.attnum = k.attnum
+JOIN pg_catalog.pg_attribute ra ON ra.attrelid = con.confrelid AND ra.attnum = con.confkey[k.pos]
+WHERE con.contype = 'f'
+  AND tn.nspname = $1
+  AND t.relname = $2
+  AND con.conrelid <> con.confrelid
+ORDER BY n.nspname, c.relname, con.conname, k.pos
+
+-- [dependent_views] [String("HR"), String("EMP")]
+-- Views / materialized views whose rewrite rule depends on this table (PG cannot drop the table under them).
+SELECT DISTINCT vn.nspname AS schema_name,
+       v.relname AS name,
+       CASE v.relkind WHEN 'm' THEN 'MATERIALIZED VIEW' ELSE 'VIEW' END AS kind
+FROM pg_catalog.pg_depend d
+JOIN pg_catalog.pg_rewrite r ON r.oid = d.objid
+JOIN pg_catalog.pg_class v ON v.oid = r.ev_class
+JOIN pg_catalog.pg_namespace vn ON vn.oid = v.relnamespace
+JOIN pg_catalog.pg_class t ON t.oid = d.refobjid
+JOIN pg_catalog.pg_namespace tn ON tn.oid = t.relnamespace
+WHERE d.classid = 'pg_catalog.pg_rewrite'::regclass
+  AND d.refclassid = 'pg_catalog.pg_class'::regclass
+  AND tn.nspname = $1
+  AND t.relname = $2
+  AND v.oid <> t.oid
+ORDER BY 1, 2
+
+-- [grants] [String("HR"), String("EMP")]
+-- Privileges granted on the table to others (the owner's implicit privileges are skipped). grantee 0 = PUBLIC.
+SELECT CASE WHEN a.grantee = 0 THEN 'PUBLIC' ELSE pg_catalog.pg_get_userbyid(a.grantee) END AS grantee,
+       a.privilege_type AS privilege,
+       a.is_grantable AS grantable
+FROM pg_catalog.pg_class c
+JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace
+CROSS JOIN LATERAL pg_catalog.aclexplode(c.relacl) a
+WHERE n.nspname = $1
+  AND c.relname = $2
+  AND a.grantee <> c.relowner
+ORDER BY 1, 2
+
+-- [table_storage] [String("HR"), String("EMP")]
+-- reltuples is -1 (PG 14+) or 0 until the table is analyzed.
+SELECT NULL::text AS engine,
+       NULL::text AS charset,
+       NULL::text AS collation,
+       ts.spcname AS tablespace,
+       c.reltuples::bigint AS row_count
+FROM pg_catalog.pg_class c
+JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace
+LEFT JOIN pg_catalog.pg_tablespace ts ON ts.oid = c.reltablespace
+WHERE n.nspname = $1
+  AND c.relname = $2
+  AND c.relkind IN ('r', 'p')
```

- [ ] **Step 7: 修改 `crates/dblens-core/locales/app.json`**

```diff
--- a/crates/dblens-core/locales/app.json
+++ b/crates/dblens-core/locales/app.json
@@ -27,6 +27,12 @@
   "design.name_empty": {"zh-CN": "名称不能为空", "en": "Name is required"},
   "design.no_columns": {"zh-CN": "表至少要有一列", "en": "A table needs at least one column"},
   "design.pk_not_null": {"zh-CN": "主键列 %{column} 将设为 NOT NULL", "en": "Primary key column %{column} will be set to NOT NULL"},
+  "design.rebuild.dependent_views": {"zh-CN": "有视图依赖这张表（%{views}），删除旧表会失败；请先处理这些视图", "en": "Views depend on this table (%{views}); dropping the old table would fail. Deal with these views first"},
+  "design.rebuild.no_writes": {"zh-CN": "重建期间不要写入这张表：复制数据之后写入的内容会丢失", "en": "Do not write to this table during the rebuild: data written after the copy will be lost"},
+  "design.rebuild.partitioned": {"zh-CN": "分区表不能重建", "en": "A partitioned table cannot be rebuilt"},
+  "design.rebuild.referenced_column": {"zh-CN": "表 %{table} 的外键引用了列 %{column}，不能删除它", "en": "A foreign key of table %{table} references column %{column}; it cannot be dropped"},
+  "design.rebuild.storage": {"zh-CN": "除表空间外的存储参数不会保留", "en": "Storage parameters other than the tablespace are not kept"},
+  "design.rebuild.views_recompiled": {"zh-CN": "依赖这张表的视图（%{views}）会暂时失效，最后重新编译", "en": "Views depending on this table (%{views}) become invalid for a while and are recompiled at the end"},
   "design.ref_columns_count": {"zh-CN": "引用列的个数要与外键列相同", "en": "The referenced columns must match the foreign key columns in number"},
   "design.ref_table_empty": {"zh-CN": "请选择引用的表", "en": "Select the referenced table"},
   "design.type_empty": {"zh-CN": "类型不能为空", "en": "Type is required"},
```

- [ ] **Step 8: 修改 `crates/dblens-dialect/sql/dameng.sql`**

```diff
--- a/crates/dblens-dialect/sql/dameng.sql
+++ b/crates/dblens-dialect/sql/dameng.sql
@@ -298,3 +298,54 @@ SELECT 'VERSIONS_ENDTRXID' FROM DUAL
 UNION
 SELECT 'VERSIONS_OPERATION' FROM DUAL
 ORDER BY 1
+
+-- [referencing_keys]
+-- Table designer rebuild: foreign keys of OTHER tables that reference this table's PK / UNIQUE constraints.
+SELECT c.OWNER AS "table_schema",
+       c.TABLE_NAME AS "table_name",
+       c.CONSTRAINT_NAME AS "constraint_name",
+       cc.COLUMN_NAME AS "column_name",
+       CAST(cc.POSITION AS INT) AS "position",
+       rc.COLUMN_NAME AS "ref_column",
+       COALESCE(c.DELETE_RULE, 'NO ACTION') AS "on_delete",
+       'NO ACTION' AS "on_update"
+FROM ALL_CONSTRAINTS c
+JOIN ALL_CONSTRAINTS p ON p.OWNER = c.R_OWNER AND p.CONSTRAINT_NAME = c.R_CONSTRAINT_NAME
+JOIN ALL_CONS_COLUMNS cc ON cc.OWNER = c.OWNER AND cc.CONSTRAINT_NAME = c.CONSTRAINT_NAME AND cc.TABLE_NAME = c.TABLE_NAME
+JOIN ALL_CONS_COLUMNS rc ON rc.OWNER = p.OWNER AND rc.CONSTRAINT_NAME = p.CONSTRAINT_NAME
+                        AND rc.TABLE_NAME = p.TABLE_NAME AND rc.POSITION = cc.POSITION
+WHERE c.CONSTRAINT_TYPE = 'R'
+  AND p.OWNER = {{ bind("schema") }}
+  AND p.TABLE_NAME = {{ bind("table") }}
+  AND NOT (c.OWNER = p.OWNER AND c.TABLE_NAME = p.TABLE_NAME)
+ORDER BY c.OWNER, c.TABLE_NAME, c.CONSTRAINT_NAME, cc.POSITION
+
+-- [dependent_views]
+SELECT d.OWNER AS "schema_name",
+       d.NAME AS "name",
+       d.TYPE AS "kind"
+FROM ALL_DEPENDENCIES d
+WHERE d.REFERENCED_OWNER = {{ bind("schema") }}
+  AND d.REFERENCED_NAME = {{ bind("table") }}
+  AND d.REFERENCED_TYPE = 'TABLE'
+  AND d.TYPE IN ('VIEW', 'MATERIALIZED VIEW')
+ORDER BY d.OWNER, d.NAME
+
+-- [grants]
+SELECT p.GRANTEE AS "grantee",
+       p.PRIVILEGE AS "privilege",
+       p.GRANTABLE AS "grantable"
+FROM ALL_TAB_PRIVS p
+WHERE p.TABLE_SCHEMA = {{ bind("schema") }}
+  AND p.TABLE_NAME = {{ bind("table") }}
+ORDER BY p.GRANTEE, p.PRIVILEGE
+
+-- [table_storage]
+SELECT CAST(NULL AS VARCHAR(1)) AS "engine",
+       CAST(NULL AS VARCHAR(1)) AS "charset",
+       CAST(NULL AS VARCHAR(1)) AS "collation",
+       t.TABLESPACE_NAME AS "tablespace",
+       t.NUM_ROWS AS "row_count"
+FROM ALL_TABLES t
+WHERE t.OWNER = {{ bind("schema") }}
+  AND t.TABLE_NAME = {{ bind("table") }}
```

- [ ] **Step 9: 修改 `crates/dblens-dialect/sql/mysql.sql`**

```diff
--- a/crates/dblens-dialect/sql/mysql.sql
+++ b/crates/dblens-dialect/sql/mysql.sql
@@ -214,3 +214,35 @@ ORDER BY k.WORD
 
 -- [variant_probe]
 SHOW VARIABLES LIKE 'polardb_version'
+
+-- [referencing_keys]
+-- Table designer rebuild: foreign keys of OTHER tables that reference this table (dropped and recreated around the swap).
+SELECT k.TABLE_SCHEMA AS table_schema,
+       k.TABLE_NAME AS table_name,
+       k.CONSTRAINT_NAME AS constraint_name,
+       k.COLUMN_NAME AS column_name,
+       k.ORDINAL_POSITION AS position,
+       k.REFERENCED_COLUMN_NAME AS ref_column,
+       rc.DELETE_RULE AS on_delete,
+       rc.UPDATE_RULE AS on_update
+FROM information_schema.KEY_COLUMN_USAGE k
+JOIN information_schema.REFERENTIAL_CONSTRAINTS rc
+  ON rc.CONSTRAINT_SCHEMA = k.CONSTRAINT_SCHEMA
+ AND rc.TABLE_NAME = k.TABLE_NAME
+ AND rc.CONSTRAINT_NAME = k.CONSTRAINT_NAME
+WHERE k.REFERENCED_TABLE_SCHEMA = {{ bind("schema") }}
+  AND k.REFERENCED_TABLE_NAME = {{ bind("table") }}
+  AND NOT (k.TABLE_SCHEMA = k.REFERENCED_TABLE_SCHEMA AND k.TABLE_NAME = k.REFERENCED_TABLE_NAME)
+ORDER BY k.TABLE_SCHEMA, k.TABLE_NAME, k.CONSTRAINT_NAME, k.ORDINAL_POSITION
+
+-- [table_storage]
+-- Table designer: engine / charset / collation kept by a rebuild, and the row estimate shown in the designer.
+SELECT t.ENGINE AS engine,
+       (SELECT MAX(c.CHARACTER_SET_NAME) FROM information_schema.COLLATION_CHARACTER_SET_APPLICABILITY c
+         WHERE c.COLLATION_NAME = t.TABLE_COLLATION) AS charset,
+       t.TABLE_COLLATION AS collation,
+       NULL AS tablespace,
+       t.TABLE_ROWS AS row_count
+FROM information_schema.TABLES t
+WHERE t.TABLE_SCHEMA = {{ bind("schema") }}
+  AND t.TABLE_NAME = {{ bind("table") }}
```

- [ ] **Step 10: 修改 `crates/dblens-dialect/sql/oracle.sql`**

```diff
--- a/crates/dblens-dialect/sql/oracle.sql
+++ b/crates/dblens-dialect/sql/oracle.sql
@@ -294,3 +294,66 @@ FROM v$reserved_words r
 WHERE (r.reserved = 'Y' OR r.res_semi = 'Y' OR r.res_attr = 'Y')
   AND REGEXP_LIKE(r.keyword, '^[A-Z][A-Z0-9_$#]*$')
 ORDER BY 1
+
+-- [referencing_keys]
+-- Table designer rebuild: foreign keys of OTHER tables that reference this table's PK / UNIQUE constraints.
+SELECT c.owner AS table_schema,
+       c.table_name AS table_name,
+       c.constraint_name AS constraint_name,
+       cc.column_name AS column_name,
+       cc.position AS position,
+       rc.column_name AS ref_column,
+       c.delete_rule AS on_delete,
+       CAST(NULL AS VARCHAR2(9)) AS on_update
+FROM all_constraints c
+JOIN all_constraints p ON p.owner = c.r_owner AND p.constraint_name = c.r_constraint_name
+JOIN all_cons_columns cc ON cc.owner = c.owner AND cc.constraint_name = c.constraint_name AND cc.table_name = c.table_name
+JOIN all_cons_columns rc ON rc.owner = p.owner AND rc.constraint_name = p.constraint_name AND rc.position = cc.position
+WHERE c.constraint_type = 'R'
+  AND p.owner = {{ bind("schema") }}
+  AND p.table_name = {{ bind("table") }}
+  AND NOT (c.owner = p.owner AND c.table_name = p.table_name)
+ORDER BY c.owner, c.table_name, c.constraint_name, cc.position
+
+-- [dependent_views]
+SELECT d.owner AS schema_name,
+       d.name AS name,
+       d.type AS kind
+FROM all_dependencies d
+WHERE d.referenced_owner = {{ bind("schema") }}
+  AND d.referenced_name = {{ bind("table") }}
+  AND d.referenced_type = 'TABLE'
+  AND d.type IN ('VIEW', 'MATERIALIZED VIEW')
+ORDER BY d.owner, d.name
+
+-- [grants]
+SELECT p.grantee AS grantee,
+       p.privilege AS privilege,
+       p.grantable AS grantable
+FROM all_tab_privs p
+WHERE p.table_schema = {{ bind("schema") }}
+  AND p.table_name = {{ bind("table") }}
+ORDER BY p.grantee, p.privilege
+
+-- [table_storage]
+SELECT CAST(NULL AS VARCHAR2(1)) AS engine,
+       CAST(NULL AS VARCHAR2(1)) AS charset,
+       CAST(NULL AS VARCHAR2(1)) AS collation,
+       t.tablespace_name AS tablespace,
+       t.num_rows AS row_count
+FROM all_tables t
+WHERE t.owner = {{ bind("schema") }}
+  AND t.table_name = {{ bind("table") }}
+
+-- [trigger_ddl]
+-- Full CREATE TRIGGER rebuilt from ALL_TRIGGERS (triggers.definition holds the body only). DESCRIPTION starts with the
+-- trigger name and the ON clause as written, unqualified: run it with the table's schema as the current schema.
+SELECT t.trigger_name AS name,
+       'CREATE OR REPLACE TRIGGER ' || t.description
+         || CASE WHEN t.when_clause IS NOT NULL THEN 'WHEN (' || t.when_clause || ')' || CHR(10) END AS head,
+       t.trigger_body AS body
+FROM all_triggers t
+WHERE t.table_owner = {{ bind("schema") }}
+  AND t.table_name = {{ bind("table") }}
+  AND t.base_object_type = 'TABLE'
+ORDER BY t.trigger_name
```

- [ ] **Step 11: 修改 `crates/dblens-dialect/sql/postgres.sql`**

```diff
--- a/crates/dblens-dialect/sql/postgres.sql
+++ b/crates/dblens-dialect/sql/postgres.sql
@@ -269,3 +269,74 @@ ORDER BY line
 -- [reserved_words]
 -- catcode R = reserved, T = reserved (can be function or type name). C (col_name) and U are not reserved.
 SELECT word FROM pg_catalog.pg_get_keywords() WHERE catcode IN ('R', 'T') ORDER BY word
+
+-- [referencing_keys]
+-- Table designer rebuild: foreign keys of OTHER tables that reference this table.
+SELECT n.nspname AS table_schema,
+       c.relname AS table_name,
+       con.conname AS constraint_name,
+       a.attname AS column_name,
+       k.pos::int AS position,
+       ra.attname AS ref_column,
+       CASE con.confdeltype WHEN 'c' THEN 'CASCADE' WHEN 'n' THEN 'SET NULL' WHEN 'd' THEN 'SET DEFAULT'
+                         WHEN 'r' THEN 'RESTRICT' ELSE 'NO ACTION' END AS on_delete,
+       CASE con.confupdtype WHEN 'c' THEN 'CASCADE' WHEN 'n' THEN 'SET NULL' WHEN 'd' THEN 'SET DEFAULT'
+                         WHEN 'r' THEN 'RESTRICT' ELSE 'NO ACTION' END AS on_update
+FROM pg_catalog.pg_constraint con
+JOIN pg_catalog.pg_class c ON c.oid = con.conrelid
+JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace
+JOIN pg_catalog.pg_class t ON t.oid = con.confrelid
+JOIN pg_catalog.pg_namespace tn ON tn.oid = t.relnamespace
+CROSS JOIN LATERAL pg_catalog.unnest(con.conkey) WITH ORDINALITY AS k(attnum, pos)
+JOIN pg_catalog.pg_attribute a ON a.attrelid = con.conrelid AND a.attnum = k.attnum
+JOIN pg_catalog.pg_attribute ra ON ra.attrelid = con.confrelid AND ra.attnum = con.confkey[k.pos]
+WHERE con.contype = 'f'
+  AND tn.nspname = {{ bind("schema") }}
+  AND t.relname = {{ bind("table") }}
+  AND con.conrelid <> con.confrelid
+ORDER BY n.nspname, c.relname, con.conname, k.pos
+
+-- [dependent_views]
+-- Views / materialized views whose rewrite rule depends on this table (PG cannot drop the table under them).
+SELECT DISTINCT vn.nspname AS schema_name,
+       v.relname AS name,
+       CASE v.relkind WHEN 'm' THEN 'MATERIALIZED VIEW' ELSE 'VIEW' END AS kind
+FROM pg_catalog.pg_depend d
+JOIN pg_catalog.pg_rewrite r ON r.oid = d.objid
+JOIN pg_catalog.pg_class v ON v.oid = r.ev_class
+JOIN pg_catalog.pg_namespace vn ON vn.oid = v.relnamespace
+JOIN pg_catalog.pg_class t ON t.oid = d.refobjid
+JOIN pg_catalog.pg_namespace tn ON tn.oid = t.relnamespace
+WHERE d.classid = 'pg_catalog.pg_rewrite'::regclass
+  AND d.refclassid = 'pg_catalog.pg_class'::regclass
+  AND tn.nspname = {{ bind("schema") }}
+  AND t.relname = {{ bind("table") }}
+  AND v.oid <> t.oid
+ORDER BY 1, 2
+
+-- [grants]
+-- Privileges granted on the table to others (the owner's implicit privileges are skipped). grantee 0 = PUBLIC.
+SELECT CASE WHEN a.grantee = 0 THEN 'PUBLIC' ELSE pg_catalog.pg_get_userbyid(a.grantee) END AS grantee,
+       a.privilege_type AS privilege,
+       a.is_grantable AS grantable
+FROM pg_catalog.pg_class c
+JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace
+CROSS JOIN LATERAL pg_catalog.aclexplode(c.relacl) a
+WHERE n.nspname = {{ bind("schema") }}
+  AND c.relname = {{ bind("table") }}
+  AND a.grantee <> c.relowner
+ORDER BY 1, 2
+
+-- [table_storage]
+-- reltuples is -1 (PG 14+) or 0 until the table is analyzed.
+SELECT NULL::text AS engine,
+       NULL::text AS charset,
+       NULL::text AS collation,
+       ts.spcname AS tablespace,
+       c.reltuples::bigint AS row_count
+FROM pg_catalog.pg_class c
+JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace
+LEFT JOIN pg_catalog.pg_tablespace ts ON ts.oid = c.reltablespace
+WHERE n.nspname = {{ bind("schema") }}
+  AND c.relname = {{ bind("table") }}
+  AND c.relkind IN ('r', 'p')
```

- [ ] **Step 12: 修改 `crates/dblens-dialect/src/design/mod.rs`**

```diff
--- a/crates/dblens-dialect/src/design/mod.rs
+++ b/crates/dblens-dialect/src/design/mod.rs
@@ -6,6 +6,7 @@ mod diff;
 mod mysql;
 mod oracle;
 mod postgres;
+mod rebuild;
 mod sql;
 
 use std::collections::BTreeMap;
@@ -18,6 +19,7 @@ use crate::Dialect;
 
 pub use catalog::{DesignCaps, TypeEntry, TypeParams};
 pub use diff::{diff_table, validate};
+pub use rebuild::{Rebuild, RebuildContext, RebuildOutcome, RebuildStep};
 pub use sql::DesignStatement;
 
 /// 草稿中改了名的项：新名 → 原名。前端用每一项的 `origin` 跟踪原来的名字，发送时整理成这个映射。
```

- [ ] **Step 13: 新建 `crates/dblens-dialect/src/design/rebuild.rs`**

```rust
//! 重建表的脚本（M3 设计文档第 5 节）：库不支持的改动（或用户手动选择）时，建新表、复制数据、核对行数、
//! 删除旧表并把新表改回原名，再重建约束、索引、触发器、指向本表的外键、自增接续、授权，重新编译失效的视图。
//!
//! 执行约定：先把当前 schema 切换到表所在的 schema（MySQL、Oracle 的触发器 DDL 里的表名不带 schema）；
//! `drop_at` 之前失败或取消时执行 `cleanup` 删除新表，旧表不受影响。

use dblens_core::schema::{ForeignKey, Table};
use dblens_core::{Msg, Result, Value, msg};
use serde::Serialize;

use super::diff::same_type;
use super::{Diff, Renames, TableChange as C};
use crate::meta::{DependentView, Grant, ReferencingKey, TableStorage};
use crate::{Dialect, Family};

/// 生成重建脚本需要的依赖信息，由服务层读取。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RebuildContext {
    pub referencing: Vec<ReferencingKey>,
    pub views: Vec<DependentView>,
    pub grants: Vec<Grant>,
    pub storage: TableStorage,
    /// 本表触发器的完整 DDL。
    pub triggers: Vec<String>,
}

/// 脚本的一步。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RebuildStep {
    Sql {
        sql: String,
    },
    /// 核对两张表的行数，不一致即停止（由程序执行）。
    CountCheck {
        source: String,
        target: String,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Rebuild {
    pub steps: Vec<RebuildStep>,
    /// 删除旧表的那一步：从这里起不能取消，失败时也不再删除新表。
    pub drop_at: usize,
    /// `drop_at` 之前失败或取消时执行：删除新表。
    pub cleanup: String,
    pub warnings: Vec<Msg>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RebuildOutcome {
    Script(Rebuild),
    /// 不能重建及原因。
    Refused {
        reason: Msg,
    },
}

/// 一个字符串的 32 位 FNV-1a 哈希（只用来让截断后的临时表名不重复）。
fn short_hash(s: &str) -> String {
    let mut h: u32 = 0x811c_9dc5;
    for b in s.bytes() {
        h ^= u32::from(b);
        h = h.wrapping_mul(0x0100_0193);
    }
    format!("{h:08x}")
}

impl Dialect {
    /// 标识符的最大字节数。
    fn max_ident(&self) -> usize {
        match self.family() {
            Family::MySql => 64,
            Family::Postgres => 63,
            Family::Oracle if self.version().unwrap_or(u32::MAX) < 120200 => 30,
            Family::Oracle | Family::Dameng => 128,
        }
    }

    /// 临时表名 `<表名>_dblens_new`；超长时截断表名并加短哈希。
    pub(crate) fn temp_table_name(&self, table: &str) -> String {
        const SUFFIX: &str = "_dblens_new";
        let max = self.max_ident();
        let full = format!("{table}{SUFFIX}");
        if full.len() <= max {
            return full;
        }
        let keep = max - SUFFIX.len() - 9;
        let mut end = keep.min(table.len());
        while !table.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}_{}{SUFFIX}", &table[..end], short_hash(table))
    }

    /// 建表语句末尾的存储属性：MySQL 的引擎、字符集、排序规则，其他库的表空间。
    fn storage_clause(&self, s: &TableStorage) -> String {
        match self.family() {
            Family::MySql => {
                let mut out = String::new();
                if let Some(e) = &s.engine {
                    out += &format!(" ENGINE = {e}");
                }
                if let Some(c) = &s.charset {
                    out += &format!(" DEFAULT CHARSET = {c}");
                }
                if let Some(c) = &s.collation {
                    out += &format!(" COLLATE = {c}");
                }
                out
            }
            _ => s.tablespace.as_ref().map(|t| format!(" TABLESPACE {}", self.ident(t))).unwrap_or_default(),
        }
    }

    /// 生成重建脚本。`draft` 应已校验过；表名不变（已有表不能改名）。
    pub fn rebuild(
        &self,
        schema: &str,
        original: &Table,
        draft: &Table,
        renames: &Renames,
        ctx: &RebuildContext,
    ) -> Result<RebuildOutcome> {
        let refuse = |reason| Ok(RebuildOutcome::Refused { reason });
        if original.partition.is_some() {
            return refuse(msg!("design.rebuild.partitioned"));
        }
        let view_list =
            || ctx.views.iter().map(|v| format!("{}.{}", v.schema, v.name)).collect::<Vec<_>>().join(", ");
        if self.family() == Family::Postgres && !ctx.views.is_empty() {
            return refuse(msg!("design.rebuild.dependent_views", views = view_list()));
        }
        // 草稿列 → 原列名（保留下来的列）
        let old_of = |name: &str| -> Option<String> {
            let old = renames.columns.get(name).map_or(name, String::as_str);
            original.columns.iter().any(|c| c.name == old).then(|| old.to_string())
        };
        let new_of = |old: &str| -> Option<String> {
            draft.columns.iter().find(|c| old_of(&c.name).as_deref() == Some(old)).map(|c| c.name.clone())
        };
        // 其他表的外键引用的列都要还在
        let mut referencing = Vec::new();
        for rk in &ctx.referencing {
            let mut key = rk.key.clone();
            for c in &mut key.ref_columns {
                match new_of(c) {
                    Some(n) => *c = n,
                    None => {
                        return refuse(msg!(
                            "design.rebuild.referenced_column",
                            table = rk.table,
                            column = c
                        ));
                    }
                }
            }
            referencing.push((rk, key));
        }

        let name = &draft.name;
        let q = self.qualified(Some(schema), name);
        let tmp = self.temp_table_name(name);
        let q_tmp = self.qualified(Some(schema), &tmp);
        let mut steps = Vec::new();
        let mut sql = |s: String| steps.push(RebuildStep::Sql { sql: s });

        // 1. 新表：只有列（MySQL 带主键：自增列必须是键；主键名在 MySQL 里不会冲突）
        let mysql = self.family() == Family::MySql;
        let bare = Table {
            name: tmp.clone(),
            primary_key: if mysql { draft.primary_key.clone() } else { None },
            uniques: vec![],
            foreign_keys: vec![],
            checks: vec![],
            indexes: vec![],
            triggers: vec![],
            partition: None,
            ..draft.clone()
        };
        let create = Diff { changes: vec![C::CreateTable { table: bare }], appended: true };
        for (i, s) in self.design_sql(schema, &tmp, &create)?.into_iter().enumerate() {
            sql(if i == 0 { s.sql + &self.storage_clause(&ctx.storage) } else { s.sql });
        }

        // 2. 复制数据：保留的列从旧列取值，类型变了的套 CAST（MySQL 靠写入时的隐式转换）；生成列不写
        let mut targets = Vec::new();
        let mut sources = Vec::new();
        for c in draft.columns.iter().filter(|c| c.generated.is_none()) {
            let Some(old) = old_of(&c.name) else { continue };
            let o = original.columns.iter().find(|x| x.name == old).expect("old_of 只返回存在的列");
            let src = self.ident(&old);
            targets.push(self.ident(&c.name));
            sources.push(if !mysql && !same_type(&o.data_type.native, &c.data_type.native) {
                format!("CAST({src} AS {})", c.data_type.native)
            } else {
                src
            });
        }
        let dameng_identity =
            self.family() == Family::Dameng && draft.columns.iter().any(|c| c.auto_increment);
        if dameng_identity {
            sql(format!("SET IDENTITY_INSERT {q_tmp} ON"));
        }
        sql(format!("INSERT INTO {q_tmp} ({}) SELECT {} FROM {q}", targets.join(", "), sources.join(", ")));
        if dameng_identity {
            sql(format!("SET IDENTITY_INSERT {q_tmp} OFF"));
        }

        // 3. 核对行数
        steps.push(RebuildStep::CountCheck { source: q.clone(), target: q_tmp.clone() });
        let mut sql = |s: String| steps.push(RebuildStep::Sql { sql: s });

        // 4. 删除其他表指向本表的外键
        for (rk, _) in &referencing {
            let other = self.qualified(Some(&rk.schema), &rk.table);
            let fk = self.ident(self.required_name(&rk.key.name)?);
            sql(if mysql {
                format!("ALTER TABLE {other} DROP FOREIGN KEY {fk}")
            } else {
                format!("ALTER TABLE {other} DROP CONSTRAINT {fk}")
            });
        }

        // 5. 删除旧表，新表改回原名
        let drop_at = steps.len();
        let mut sql = |s: String| steps.push(RebuildStep::Sql { sql: s });
        sql(format!("DROP TABLE {q}"));
        sql(if mysql {
            format!("ALTER TABLE {q_tmp} RENAME TO {q}")
        } else {
            format!("ALTER TABLE {q_tmp} RENAME TO {}", self.ident(name))
        });

        // 6. 约束、索引、外键、触发器
        let mut changes = Vec::new();
        if let (Some(key), false) = (&draft.primary_key, mysql) {
            changes.push(C::AddPrimaryKey { key: key.clone() });
        }
        changes.extend(draft.uniques.iter().map(|u| C::AddUnique { unique: u.clone() }));
        changes.extend(draft.checks.iter().map(|c| C::AddCheck { check: c.clone() }));
        changes.extend(draft.indexes.iter().map(|x| C::AddIndex { index: x.clone() }));
        changes.extend(draft.foreign_keys.iter().map(|f| C::AddForeignKey { key: f.clone() }));
        for s in self.design_sql(schema, name, &Diff { changes, appended: true })? {
            sql(s.sql);
        }
        for t in &ctx.triggers {
            sql(t.trim().trim_end_matches(';').to_string()
                + if self.family() == Family::Postgres || mysql { "" } else { ";" });
        }

        // 7. 其他表指向本表的外键（引用列随改名）
        for (rk, key) in &referencing {
            let key =
                ForeignKey { ref_schema: Some(schema.to_string()), ref_table: name.clone(), ..key.clone() };
            sql(format!(
                "ALTER TABLE {} ADD {}{}",
                self.qualified(Some(&rk.schema), &rk.table),
                self.constraint_name(&key.name),
                self.fk_clause(&rk.schema, &key)
            ));
        }

        // 8. 自增接续、授权、重新编译视图
        for c in draft.columns.iter().filter(|c| c.auto_increment && old_of(&c.name).is_some()) {
            match self.family() {
                Family::Postgres => sql(format!(
                    "SELECT pg_catalog.setval(pg_catalog.pg_get_serial_sequence({}, {}), COALESCE(MAX({col}), 0) + 1, false) FROM {q}",
                    self.literal(&Value::String(q.clone()))?,
                    self.literal(&Value::String(c.name.clone()))?,
                    col = self.ident(&c.name)
                )),
                Family::Oracle => sql(format!(
                    "ALTER TABLE {q} MODIFY ({} GENERATED BY DEFAULT AS IDENTITY (START WITH LIMIT VALUE))",
                    self.ident(&c.name)
                )),
                Family::MySql | Family::Dameng => {}
            }
        }
        if !mysql {
            for g in &ctx.grants {
                let grantee = if g.grantee.eq_ignore_ascii_case("PUBLIC") {
                    "PUBLIC".into()
                } else {
                    self.ident(&g.grantee)
                };
                sql(format!(
                    "GRANT {} ON {q} TO {grantee}{}",
                    g.privilege,
                    if g.grantable { " WITH GRANT OPTION" } else { "" }
                ));
            }
        }
        let mut warnings = vec![msg!("design.rebuild.no_writes")];
        if matches!(self.family(), Family::Oracle | Family::Dameng) && !ctx.views.is_empty() {
            for v in &ctx.views {
                let kind = if v.materialized { "MATERIALIZED VIEW" } else { "VIEW" };
                sql(format!("ALTER {kind} {} COMPILE", self.qualified(Some(&v.schema), &v.name)));
            }
            warnings.push(msg!("design.rebuild.views_recompiled", views = view_list()));
        }
        if !mysql {
            warnings.push(msg!("design.rebuild.storage"));
        }
        Ok(RebuildOutcome::Script(Rebuild {
            steps,
            drop_at,
            cleanup: format!("DROP TABLE {q_tmp}"),
            warnings,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::super::sql::fixtures::*;
    use super::*;
    use crate::Variant;
    use crate::meta::{DependentView, Grant, ReferencingKey, TableStorage};

    fn render(r: &RebuildOutcome) -> String {
        let RebuildOutcome::Script(r) = r else { panic!("{r:?}") };
        let mut out: Vec<String> = r
            .steps
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let mark = if i == r.drop_at { "▶ " } else { "" };
                match s {
                    RebuildStep::Sql { sql } => format!("{mark}{sql}"),
                    RebuildStep::CountCheck { source, target } => {
                        format!("{mark}-- COUNT {source} = {target}")
                    }
                }
            })
            .collect();
        out.push(format!("-- cleanup: {}", r.cleanup));
        out.join("\n")
    }

    /// 把 amount 挪到最前、code 改名为 order_code 并改类型。
    fn moved(orig: &Table, varchar: &str) -> (Table, Renames) {
        let mut d = orig.clone();
        let amount = d.columns.pop().unwrap();
        d.columns.insert(0, amount);
        d.columns[2].name = "order_code".into();
        d.columns[2].data_type.native = varchar.into();
        d.uniques[0].columns = vec!["order_code".into()];
        d.foreign_keys[0].columns = vec!["order_code".into()];
        (d, Renames { columns: [("order_code".into(), "code".into())].into(), ..Default::default() })
    }

    fn ctx(schema: &str, trigger: &str) -> RebuildContext {
        RebuildContext {
            referencing: vec![ReferencingKey {
                schema: schema.into(),
                table: "items".into(),
                key: ForeignKey {
                    name: Some("fk_items_order".into()),
                    columns: vec!["order_code".into()],
                    ref_schema: Some(schema.into()),
                    ref_table: "orders".into(),
                    ref_columns: vec!["code".into()],
                    on_delete: Some("CASCADE".into()),
                    on_update: None,
                },
            }],
            views: vec![],
            grants: vec![Grant { grantee: "report".into(), privilege: "SELECT".into(), grantable: false }],
            storage: TableStorage {
                engine: Some("InnoDB".into()),
                charset: Some("utf8mb4".into()),
                collation: Some("utf8mb4_0900_ai_ci".into()),
                tablespace: None,
                row_count: Some(10),
            },
            triggers: vec![trigger.into()],
        }
    }

    #[test]
    fn scripts_by_dialect() {
        let cases = [
            (
                Variant::MySql,
                "shop",
                ["BIGINT", "VARCHAR(20)", "DECIMAL(10,2)"],
                "VARCHAR(50)",
                "CREATE TRIGGER `trg` BEFORE INSERT ON `orders` FOR EACH ROW SET NEW.amount = 0",
            ),
            (
                Variant::Postgres,
                "public",
                ["bigint", "varchar(20)", "numeric(10,2)"],
                "varchar(50)",
                "CREATE TRIGGER trg BEFORE INSERT ON public.orders FOR EACH ROW EXECUTE FUNCTION f()",
            ),
            (
                Variant::Oracle,
                "HR",
                ["NUMBER(19)", "VARCHAR2(20)", "NUMBER(10,2)"],
                "VARCHAR2(50 CHAR)",
                "CREATE OR REPLACE TRIGGER trg\nBEFORE INSERT ON orders\nFOR EACH ROW\nBEGIN NULL; END;",
            ),
            (
                Variant::Dameng,
                "HR",
                ["BIGINT", "VARCHAR(20)", "DECIMAL(10,2)"],
                "VARCHAR(50)",
                "CREATE OR REPLACE TRIGGER trg BEFORE INSERT ON orders FOR EACH ROW BEGIN NULL; END;",
            ),
        ];
        for (v, schema, types, varchar, trigger) in cases {
            let d = Dialect::new(v);
            let orig = table(types);
            let (draft, renames) = moved(&orig, varchar);
            let mut c = ctx(schema, trigger);
            if v != Variant::MySql {
                c.storage = TableStorage { tablespace: Some("USERS".into()), ..Default::default() };
            }
            if matches!(v, Variant::Oracle | Variant::Dameng) {
                c.views = vec![DependentView {
                    schema: schema.into(),
                    name: "v_orders".into(),
                    materialized: false,
                }];
            }
            let out = d.rebuild(schema, &orig, &draft, &renames, &c).unwrap();
            insta::assert_snapshot!(format!("rebuild_{v:?}").to_lowercase(), render(&out));
        }
    }

    #[test]
    fn refusals() {
        let orig = table(["bigint", "varchar(20)", "numeric(10,2)"]);
        let (draft, renames) = moved(&orig, "varchar(50)");
        let pg = Dialect::new(Variant::Postgres);
        let c = RebuildContext {
            views: vec![DependentView { schema: "public".into(), name: "v".into(), materialized: true }],
            ..Default::default()
        };
        assert_eq!(
            pg.rebuild("public", &orig, &draft, &renames, &c).unwrap(),
            RebuildOutcome::Refused { reason: msg!("design.rebuild.dependent_views", views = "public.v") }
        );
        // Oracle、达梦的依赖视图在最后重新编译，不拒绝
        assert!(matches!(
            Dialect::new(Variant::Oracle).rebuild("HR", &orig, &draft, &renames, &c).unwrap(),
            RebuildOutcome::Script(_)
        ));
        let mut part = orig.clone();
        part.partition = Some("RANGE (id)".into());
        assert_eq!(
            pg.rebuild("public", &part, &draft, &renames, &RebuildContext::default()).unwrap(),
            RebuildOutcome::Refused { reason: msg!("design.rebuild.partitioned") }
        );
        // 被其他表引用的列被删掉
        let mut gone = draft.clone();
        gone.columns.retain(|c| c.name != "order_code");
        gone.uniques.clear();
        gone.foreign_keys.clear();
        let c = ctx("public", "");
        assert_eq!(
            pg.rebuild("public", &orig, &gone, &renames, &c).unwrap(),
            RebuildOutcome::Refused {
                reason: msg!("design.rebuild.referenced_column", table = "items", column = "code")
            }
        );
    }

    #[test]
    fn temp_names_fit_the_limit() {
        let pg = Dialect::new(Variant::Postgres);
        assert_eq!(pg.temp_table_name("orders"), "orders_dblens_new");
        let long = "a".repeat(60);
        let t = pg.temp_table_name(&long);
        assert_eq!(t.len(), 63);
        assert!(t.ends_with("_dblens_new"));
        assert_ne!(t, pg.temp_table_name(&"a".repeat(61)), "截断后仍不同");
        let ora11 = Dialect::new(Variant::Oracle).with_version(Some(110200));
        assert_eq!(ora11.temp_table_name("CUSTOMER_ORDERS_2024").len(), 30);
        let cjk = Dialect::new(Variant::MySql).temp_table_name(&"订单".repeat(20));
        assert!(cjk.len() <= 64 && cjk.ends_with("_dblens_new"));
    }
}
```

- [ ] **Step 14: 新建 `crates/dblens-dialect/src/design/snapshots/dblens_dialect__design__rebuild__tests__rebuild_dameng.snap`**

```text
---
source: crates/dblens-dialect/src/design/rebuild.rs
expression: render(&out)
---
CREATE TABLE HR."orders_dblens_new" (
    "amount" DECIMAL(10,2) DEFAULT 0,
    "id" BIGINT IDENTITY(1, 1) NOT NULL,
    "order_code" VARCHAR(50)
) TABLESPACE USERS
COMMENT ON TABLE HR."orders_dblens_new" IS '订单'
COMMENT ON COLUMN HR."orders_dblens_new"."amount" IS '金额'
SET IDENTITY_INSERT HR."orders_dblens_new" ON
INSERT INTO HR."orders_dblens_new" ("amount", "id", "order_code") SELECT "amount", "id", CAST("code" AS VARCHAR(50)) FROM HR."orders"
SET IDENTITY_INSERT HR."orders_dblens_new" OFF
-- COUNT HR."orders" = HR."orders_dblens_new"
ALTER TABLE HR."items" DROP CONSTRAINT "fk_items_order"
▶ DROP TABLE HR."orders"
ALTER TABLE HR."orders_dblens_new" RENAME TO "orders"
ALTER TABLE HR."orders" ADD CONSTRAINT "pk_orders" PRIMARY KEY ("id")
ALTER TABLE HR."orders" ADD CONSTRAINT "uk_code" UNIQUE ("order_code")
ALTER TABLE HR."orders" ADD CONSTRAINT "ck_amount" CHECK (amount >= 0)
CREATE INDEX HR."ix_amount" ON HR."orders" ("amount" DESC)
ALTER TABLE HR."orders" ADD CONSTRAINT "fk_parent" FOREIGN KEY ("order_code") REFERENCES HR."parent" ("code") ON DELETE CASCADE
CREATE OR REPLACE TRIGGER trg BEFORE INSERT ON orders FOR EACH ROW BEGIN NULL; END;
ALTER TABLE HR."items" ADD CONSTRAINT "fk_items_order" FOREIGN KEY ("order_code") REFERENCES HR."orders" ("order_code") ON DELETE CASCADE
GRANT SELECT ON HR."orders" TO "report"
ALTER VIEW HR."v_orders" COMPILE
-- cleanup: DROP TABLE HR."orders_dblens_new"
```

- [ ] **Step 15: 新建 `crates/dblens-dialect/src/design/snapshots/dblens_dialect__design__rebuild__tests__rebuild_mysql.snap`**

```text
---
source: crates/dblens-dialect/src/design/rebuild.rs
expression: render(&out)
---
CREATE TABLE shop.orders_dblens_new (
    amount DECIMAL(10,2) NULL DEFAULT 0 COMMENT '金额',
    id BIGINT NOT NULL AUTO_INCREMENT,
    order_code VARCHAR(50) NULL,
    PRIMARY KEY (id)
) COMMENT = '订单' ENGINE = InnoDB DEFAULT CHARSET = utf8mb4 COLLATE = utf8mb4_0900_ai_ci
INSERT INTO shop.orders_dblens_new (amount, id, order_code) SELECT amount, id, code FROM shop.orders
-- COUNT shop.orders = shop.orders_dblens_new
ALTER TABLE shop.items DROP FOREIGN KEY fk_items_order
▶ DROP TABLE shop.orders
ALTER TABLE shop.orders_dblens_new RENAME TO shop.orders
ALTER TABLE shop.orders ADD CONSTRAINT uk_code UNIQUE (order_code)
ALTER TABLE shop.orders ADD CONSTRAINT ck_amount CHECK (amount >= 0)
ALTER TABLE shop.orders ADD INDEX ix_amount (amount DESC)
ALTER TABLE shop.orders ADD CONSTRAINT fk_parent FOREIGN KEY (order_code) REFERENCES shop.parent (code) ON DELETE CASCADE
CREATE TRIGGER `trg` BEFORE INSERT ON `orders` FOR EACH ROW SET NEW.amount = 0
ALTER TABLE shop.items ADD CONSTRAINT fk_items_order FOREIGN KEY (order_code) REFERENCES shop.orders (order_code) ON DELETE CASCADE
-- cleanup: DROP TABLE shop.orders_dblens_new
```

- [ ] **Step 16: 新建 `crates/dblens-dialect/src/design/snapshots/dblens_dialect__design__rebuild__tests__rebuild_oracle.snap`**

```text
---
source: crates/dblens-dialect/src/design/rebuild.rs
expression: render(&out)
---
CREATE TABLE HR."orders_dblens_new" (
    "amount" NUMBER(10,2) DEFAULT 0,
    "id" NUMBER(19) GENERATED BY DEFAULT AS IDENTITY NOT NULL,
    "order_code" VARCHAR2(50 CHAR)
) TABLESPACE USERS
COMMENT ON TABLE HR."orders_dblens_new" IS '订单'
COMMENT ON COLUMN HR."orders_dblens_new"."amount" IS '金额'
INSERT INTO HR."orders_dblens_new" ("amount", "id", "order_code") SELECT "amount", "id", CAST("code" AS VARCHAR2(50 CHAR)) FROM HR."orders"
-- COUNT HR."orders" = HR."orders_dblens_new"
ALTER TABLE HR."items" DROP CONSTRAINT "fk_items_order"
▶ DROP TABLE HR."orders"
ALTER TABLE HR."orders_dblens_new" RENAME TO "orders"
ALTER TABLE HR."orders" ADD CONSTRAINT "pk_orders" PRIMARY KEY ("id")
ALTER TABLE HR."orders" ADD CONSTRAINT "uk_code" UNIQUE ("order_code")
ALTER TABLE HR."orders" ADD CONSTRAINT "ck_amount" CHECK (amount >= 0)
CREATE INDEX HR."ix_amount" ON HR."orders" ("amount" DESC)
ALTER TABLE HR."orders" ADD CONSTRAINT "fk_parent" FOREIGN KEY ("order_code") REFERENCES HR."parent" ("code") ON DELETE CASCADE
CREATE OR REPLACE TRIGGER trg
BEFORE INSERT ON orders
FOR EACH ROW
BEGIN NULL; END;
ALTER TABLE HR."items" ADD CONSTRAINT "fk_items_order" FOREIGN KEY ("order_code") REFERENCES HR."orders" ("order_code") ON DELETE CASCADE
ALTER TABLE HR."orders" MODIFY ("id" GENERATED BY DEFAULT AS IDENTITY (START WITH LIMIT VALUE))
GRANT SELECT ON HR."orders" TO "report"
ALTER VIEW HR."v_orders" COMPILE
-- cleanup: DROP TABLE HR."orders_dblens_new"
```

- [ ] **Step 17: 新建 `crates/dblens-dialect/src/design/snapshots/dblens_dialect__design__rebuild__tests__rebuild_postgres.snap`**

```text
---
source: crates/dblens-dialect/src/design/rebuild.rs
expression: render(&out)
---
CREATE TABLE public.orders_dblens_new (
    amount numeric(10,2) DEFAULT 0,
    id bigint GENERATED BY DEFAULT AS IDENTITY NOT NULL,
    order_code varchar(50)
) TABLESPACE "USERS"
COMMENT ON TABLE public.orders_dblens_new IS '订单'
COMMENT ON COLUMN public.orders_dblens_new.amount IS '金额'
INSERT INTO public.orders_dblens_new (amount, id, order_code) SELECT amount, id, CAST(code AS varchar(50)) FROM public.orders
-- COUNT public.orders = public.orders_dblens_new
ALTER TABLE public.items DROP CONSTRAINT fk_items_order
▶ DROP TABLE public.orders
ALTER TABLE public.orders_dblens_new RENAME TO orders
ALTER TABLE public.orders ADD CONSTRAINT pk_orders PRIMARY KEY (id)
ALTER TABLE public.orders ADD CONSTRAINT uk_code UNIQUE (order_code)
ALTER TABLE public.orders ADD CONSTRAINT ck_amount CHECK (amount >= 0)
CREATE INDEX ix_amount ON public.orders (amount DESC)
ALTER TABLE public.orders ADD CONSTRAINT fk_parent FOREIGN KEY (order_code) REFERENCES public.parent (code) ON DELETE CASCADE
CREATE TRIGGER trg BEFORE INSERT ON public.orders FOR EACH ROW EXECUTE FUNCTION f()
ALTER TABLE public.items ADD CONSTRAINT fk_items_order FOREIGN KEY (order_code) REFERENCES public.orders (order_code) ON DELETE CASCADE
SELECT pg_catalog.setval(pg_catalog.pg_get_serial_sequence('public.orders', 'id'), COALESCE(MAX(id), 0) + 1, false) FROM public.orders
GRANT SELECT ON public.orders TO report
-- cleanup: DROP TABLE public.orders_dblens_new
```

- [ ] **Step 18: 修改 `crates/dblens-dialect/src/meta/assemble.rs`**

```diff
--- a/crates/dblens-dialect/src/meta/assemble.rs
+++ b/crates/dblens-dialect/src/meta/assemble.rs
@@ -322,6 +322,128 @@ pub fn reserved_words(rows: &MetaRows) -> Result<Vec<String>> {
     Ok(rows.iter().map(|r| r.name("word").trim().to_ascii_uppercase()).filter(|w| !w.is_empty()).collect())
 }
 
+/// 其他表指向某张表的外键（`referencing_keys`）：外键所在的 schema、表和外键本身。
+#[derive(Debug, Clone, PartialEq, Serialize)]
+pub struct ReferencingKey {
+    pub schema: String,
+    pub table: String,
+    pub key: ForeignKey,
+}
+
+/// 依赖某张表的视图（`dependent_views`）。
+#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
+pub struct DependentView {
+    pub schema: String,
+    pub name: String,
+    pub materialized: bool,
+}
+
+/// 表上的一项授权（`grants`）。
+#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
+pub struct Grant {
+    pub grantee: String,
+    pub privilege: String,
+    pub grantable: bool,
+}
+
+/// 存储属性和行数估计（`table_storage`）。统计信息没有收集时行数为 None。
+#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
+pub struct TableStorage {
+    pub engine: Option<String>,
+    pub charset: Option<String>,
+    pub collation: Option<String>,
+    pub tablespace: Option<String>,
+    pub row_count: Option<i64>,
+}
+
+/// `referencing_keys` 的结果：`ref_schema` / `ref_table` 是被引用的这张表。
+pub fn referencing_keys(rows: &MetaRows, ref_schema: &str, ref_table: &str) -> Result<Vec<ReferencingKey>> {
+    rows.require(
+        "referencing_keys",
+        &[
+            "table_schema",
+            "table_name",
+            "constraint_name",
+            "column_name",
+            "position",
+            "ref_column",
+            "on_delete",
+        ],
+    )?;
+    let mut out: Vec<ReferencingKey> = Vec::new();
+    for r in rows.iter() {
+        let (schema, table, name) = (r.name("table_schema"), r.name("table_name"), r.name("constraint_name"));
+        match out.last_mut() {
+            Some(k)
+                if k.schema == schema && k.table == table && k.key.name.as_deref() == Some(name.as_str()) =>
+            {
+                k.key.columns.push(r.name("column_name"));
+                k.key.ref_columns.push(r.name("ref_column"));
+            }
+            _ => out.push(ReferencingKey {
+                schema,
+                table,
+                key: ForeignKey {
+                    name: Some(name),
+                    columns: vec![r.name("column_name")],
+                    ref_schema: Some(ref_schema.to_string()),
+                    ref_table: ref_table.to_string(),
+                    ref_columns: vec![r.name("ref_column")],
+                    on_delete: r.text("on_delete"),
+                    on_update: r.text("on_update"),
+                },
+            }),
+        }
+    }
+    Ok(out)
+}
+
+pub fn dependent_views(rows: &MetaRows) -> Result<Vec<DependentView>> {
+    rows.require("dependent_views", &["schema_name", "name", "kind"])?;
+    Ok(rows
+        .iter()
+        .map(|r| DependentView {
+            schema: r.name("schema_name"),
+            name: r.name("name"),
+            materialized: r.name("kind").to_ascii_uppercase().starts_with("MATERIALIZED"),
+        })
+        .collect())
+}
+
+pub fn grants(rows: &MetaRows) -> Result<Vec<Grant>> {
+    rows.require("grants", &["grantee", "privilege", "grantable"])?;
+    Ok(rows
+        .iter()
+        .map(|r| Grant {
+            grantee: r.name("grantee"),
+            privilege: r.name("privilege"),
+            grantable: r.flag("grantable"),
+        })
+        .collect())
+}
+
+/// 第一行；没有结果（表不存在）时为默认值。
+pub fn table_storage(rows: &MetaRows) -> Result<TableStorage> {
+    rows.require("table_storage", &["engine", "charset", "collation", "tablespace", "row_count"])?;
+    Ok(rows
+        .iter()
+        .next()
+        .map(|r| TableStorage {
+            engine: r.text("engine"),
+            charset: r.text("charset"),
+            collation: r.text("collation"),
+            tablespace: r.text("tablespace"),
+            row_count: r.int("row_count").filter(|n| *n >= 0),
+        })
+        .unwrap_or_default())
+}
+
+/// `trigger_ddl`（Oracle）：每个触发器拼成一条完整的 CREATE TRIGGER。
+pub fn trigger_ddl(rows: &MetaRows) -> Result<Vec<String>> {
+    rows.require("trigger_ddl", &["name", "head", "body"])?;
+    Ok(rows.iter().map(|r| format!("{}{}", r.name("head"), r.name("body")).trim_end().to_string()).collect())
+}
+
 #[cfg(test)]
 mod tests {
     use dblens_core::{LogicalType, Value};
```

- [ ] **Step 19: 修改 `crates/dblens-dialect/src/meta/mod.rs`**

```diff
--- a/crates/dblens-dialect/src/meta/mod.rs
+++ b/crates/dblens-dialect/src/meta/mod.rs
@@ -11,8 +11,9 @@ use serde::{Deserialize, Serialize};
 use dblens_core::Value;
 
 pub use assemble::{
-    ObjectInfo, SchemaInfo, TableRows, objects, reserved_words, routines, schemas, sequences, source_text,
-    synonyms, tables, views,
+    DependentView, Grant, ObjectInfo, ReferencingKey, SchemaInfo, TableRows, TableStorage, dependent_views,
+    grants, objects, referencing_keys, reserved_words, routines, schemas, sequences, source_text, synonyms,
+    table_storage, tables, trigger_ddl, views,
 };
 pub use rows::{MetaRow, MetaRows};
 
@@ -44,10 +45,20 @@ pub enum MetaKey {
     VariantProbe,
     /// 由数据库给出的对象 DDL（不在模板文件中，由 `Dialect::ddl_query` 生成）。参数：schema、name、kind。
     Ddl,
+    /// 以下供表设计器重建表用，参数：schema、table。其他表指向这张表的外键。
+    ReferencingKeys,
+    /// 依赖这张表的视图和物化视图（MySQL 系没有：视图按名字引用表）。
+    DependentViews,
+    /// 这张表上授予他人的权限（MySQL 系没有：授权按表名记录，重建不受影响）。
+    Grants,
+    /// 存储属性（引擎、字符集、表空间）和行数估计。
+    TableStorage,
+    /// 触发器的完整 DDL（只有 Oracle：`triggers` 的 definition 只有触发器体）。
+    TriggerDdl,
 }
 
 impl MetaKey {
-    pub const ALL: [MetaKey; 16] = [
+    pub const ALL: [MetaKey; 21] = [
         MetaKey::Schemas,
         MetaKey::Tables,
         MetaKey::Views,
@@ -64,6 +75,11 @@ impl MetaKey {
         MetaKey::RoutineSource,
         MetaKey::ReservedWords,
         MetaKey::VariantProbe,
+        MetaKey::ReferencingKeys,
+        MetaKey::DependentViews,
+        MetaKey::Grants,
+        MetaKey::TableStorage,
+        MetaKey::TriggerDdl,
     ];
 
     /// 取一张表或整个 schema 明细时要执行的查询，结果按同样的顺序填进 `TableRows`。
@@ -95,6 +111,11 @@ impl MetaKey {
             MetaKey::ReservedWords => "reserved_words",
             MetaKey::VariantProbe => "variant_probe",
             MetaKey::Ddl => "ddl",
+            MetaKey::ReferencingKeys => "referencing_keys",
+            MetaKey::DependentViews => "dependent_views",
+            MetaKey::Grants => "grants",
+            MetaKey::TableStorage => "table_storage",
+            MetaKey::TriggerDdl => "trigger_ddl",
         }
     }
 }
```

- [ ] **Step 20: 检查**

```bash
export CARGO_INCREMENTAL=0
cargo fmt --all
cargo clippy --config "source.mirror.registry='sparse+https://rsproxy.cn/index/'" --workspace --all-targets -- -D warnings
DBLENS_LANG=en cargo test --config "source.mirror.registry='sparse+https://rsproxy.cn/index/'" --workspace
```

Expected: 全部测试通过（dialect 新增 3 个，重建脚本快照 4 个，元数据快照更新 5 个）。

- [ ] **Step 21: 提交**

```bash
git add crates/dblens-core/locales/app.json crates/dblens-dialect/sql/dameng.sql crates/dblens-dialect/sql/mysql.sql crates/dblens-dialect/sql/oracle.sql crates/dblens-dialect/sql/postgres.sql crates/dblens-dialect/src/design/mod.rs crates/dblens-dialect/src/design/rebuild.rs crates/dblens-dialect/src/design/snapshots/dblens_dialect__design__rebuild__tests__rebuild_dameng.snap crates/dblens-dialect/src/design/snapshots/dblens_dialect__design__rebuild__tests__rebuild_mysql.snap crates/dblens-dialect/src/design/snapshots/dblens_dialect__design__rebuild__tests__rebuild_oracle.snap crates/dblens-dialect/src/design/snapshots/dblens_dialect__design__rebuild__tests__rebuild_postgres.snap crates/dblens-dialect/src/meta/assemble.rs crates/dblens-dialect/src/meta/mod.rs crates/dblens-dialect/tests/meta_snapshots.rs crates/dblens-dialect/tests/snapshots/meta_snapshots__dameng.snap crates/dblens-dialect/tests/snapshots/meta_snapshots__mariadb.snap crates/dblens-dialect/tests/snapshots/meta_snapshots__mysql.snap crates/dblens-dialect/tests/snapshots/meta_snapshots__oracle.snap crates/dblens-dialect/tests/snapshots/meta_snapshots__postgres.snap
git commit -m "feat(dialect): 重建表的脚本与所需的元数据查询" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 6: 服务层：读表、预览与执行任务

`dblens-service/src/design.rs`：
- `design_caps`；`design_table`（跳过缓存直接读表，附存储属性和行数估计、指纹）；`MetadataCache::forget` 丢掉一条缓存。
- `preview_design(profile_id, &DesignRequest) -> DesignPreview`：整理草稿（已有表不能改名）→ 校验 → 比对 → 判断是否重建（方言不支持或用户手动选择；需要时读取重建的依赖信息）→ 生成语句或重建脚本 → 每条语句过执行策略（拒绝的原因、需要确认的原因）→ 与请求中的指纹比较。
- `start_design`：有校验错误、原表已被修改、不能重建时 `invalid`，被策略拒绝时 `Denied`，没有改动时 `invalid`；作为任务（`TaskKind::Design`）在单独的连接上执行：先切换到表所在的 schema，PG 家族整体一个事务；每条语句记入执行历史；失败时记下失败的语句和没有执行的语句（`TaskStatus.remaining`，PG 回滚时为空）；重建从删除旧表起不能取消，之前失败或取消时删除新表；结束后让本 schema 和其他表外键所在 schema 的缓存失效。
- 假驱动增加 `SELECT COUNT(*) …`（一行一列的 3）。

后端词条加 `design.has_errors`、`design.conflict`、`design.no_changes`、`design.count_mismatch`。

**Files:**
- Modify: `crates/dblens-core/locales/app.json`
- Create: `crates/dblens-service/src/design.rs`
- Modify: `crates/dblens-service/src/fake.rs`
- Modify: `crates/dblens-service/src/lib.rs`
- Modify: `crates/dblens-service/src/metadata.rs`
- Modify: `crates/dblens-service/src/task.rs`

**Interfaces:**
- Consumes：任务 2–5 的方言接口；M2b 的 `TaskManager`、`import::exec`。
- Produces（`dblens_service`）：
  - `DesignRequest { schema, name: Option<String>, draft: Table, renames: Renames, rebuild: bool, fingerprint: Option<String> }`（Deserialize）；
  - `DesignTable { table, storage, fingerprint }`、`PreviewStatement { step (flatten), changes }`、`RebuildInfo { reasons, active, refused }`、`DesignPreview { errors, changes, statements, rebuild, warnings, transactional, denied, confirm, conflict }`（Serialize）；
  - `Service::design_caps(&str)`、`design_table(&str, schema, name)`、`preview_design(&str, &DesignRequest)`、`start_design(self: &Arc<Self>, &str, DesignRequest) -> Result<String>`；
  - `TaskKind::Design`、`TaskStatus.remaining: Option<String>`、`MetadataCache::forget(profile_id, key, &args)`。

- [ ] **Step 1: 修改 `crates/dblens-core/locales/app.json`**

```diff
--- a/crates/dblens-core/locales/app.json
+++ b/crates/dblens-core/locales/app.json
@@ -21,10 +21,14 @@
   "ddl.unsupported": {"zh-CN": "该数据库不支持查看 %{kind} 的 DDL", "en": "This database does not support viewing the DDL of %{kind}"},
   "design.bad_rename": {"zh-CN": "列 %{name} 的原名不对", "en": "Column %{name} has an invalid original name"},
   "design.columns_empty": {"zh-CN": "至少要选一列", "en": "Select at least one column"},
+  "design.conflict": {"zh-CN": "表结构已被修改，请刷新后重新编辑", "en": "The table structure has been changed; reload and edit again"},
+  "design.count_mismatch": {"zh-CN": "行数不一致：原表 %{source} 行，新表 %{target} 行", "en": "Row counts differ: %{source} in the old table, %{target} in the new one"},
   "design.duplicate": {"zh-CN": "名称 %{name} 重复", "en": "Duplicate name %{name}"},
   "design.duplicate_column": {"zh-CN": "列 %{column} 重复", "en": "Column %{column} is listed twice"},
   "design.expression_empty": {"zh-CN": "表达式不能为空", "en": "Expression is required"},
+  "design.has_errors": {"zh-CN": "设计中有错误，请先修正", "en": "The design has errors; fix them first"},
   "design.name_empty": {"zh-CN": "名称不能为空", "en": "Name is required"},
+  "design.no_changes": {"zh-CN": "没有改动", "en": "There are no changes"},
   "design.no_columns": {"zh-CN": "表至少要有一列", "en": "A table needs at least one column"},
   "design.pk_not_null": {"zh-CN": "主键列 %{column} 将设为 NOT NULL", "en": "Primary key column %{column} will be set to NOT NULL"},
   "design.rebuild.dependent_views": {"zh-CN": "有视图依赖这张表（%{views}），删除旧表会失败；请先处理这些视图", "en": "Views depend on this table (%{views}); dropping the old table would fail. Deal with these views first"},
```

- [ ] **Step 2: 新建 `crates/dblens-service/src/design.rs`**

```rust
//! 表设计器（M3 设计文档第 6 节）：读取原表、预览改动生成的 DDL（或重建脚本）、作为任务执行。
//! 执行时用草稿重新生成语句，不接受前端传来的 SQL；执行前核对原表没有被别人改过（`fingerprint`）。

use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use dblens_core::schema::Table;
use dblens_core::{Error, Msg, Result, msg};
use dblens_dialect::Dialect;
use dblens_dialect::design::{
    DesignCaps, DesignIssue, DesignStatement, Rebuild, RebuildContext, RebuildOutcome, RebuildStep, Renames,
    TableChange, diff_table, validate,
};
use dblens_dialect::meta::{self, MetaArgs, MetaKey, MetaRows, TableStorage};
use dblens_driver::{Connection, collect_all};
use serde::{Deserialize, Serialize};

use crate::history::NewHistory;
use crate::import::exec;
use crate::policy::{Decision, ExecContext, classify};
use crate::profile::{Profile, driver_name, variant_code};
use crate::service::Service;
use crate::task::{TaskFailure, TaskHandle, TaskKind, TaskPhase};

/// 预览和执行的请求。
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct DesignRequest {
    pub schema: String,
    /// 已有表的名字；新建表时不给。
    #[serde(default)]
    pub name: Option<String>,
    pub draft: Table,
    #[serde(default)]
    pub renames: Renames,
    /// 用户在预览中手动选择了重建。
    #[serde(default)]
    pub rebuild: bool,
    /// 打开设计器时原表的指纹；执行前核对。
    #[serde(default)]
    pub fingerprint: Option<String>,
}

/// 打开设计器时读到的原表。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DesignTable {
    pub table: Table,
    pub storage: TableStorage,
    pub fingerprint: String,
}

/// 预览中的一条语句：普通 SQL，或由程序执行的核对行数（`sql` 为空）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PreviewStatement {
    #[serde(flatten)]
    pub step: RebuildStep,
    /// 对应的变更序号（重建脚本中为空）。
    pub changes: Vec<usize>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RebuildInfo {
    /// 方言确定不支持的改动：不为空时必须重建。
    pub reasons: Vec<Msg>,
    /// 这次预览是否用重建方式。
    pub active: bool,
    /// 不能重建的原因。
    pub refused: Option<Msg>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DesignPreview {
    pub errors: Vec<DesignIssue>,
    pub changes: Vec<TableChange>,
    pub statements: Vec<PreviewStatement>,
    pub rebuild: RebuildInfo,
    /// 提示：主键列改为 NOT NULL、重建期间不要写入等。
    pub warnings: Vec<Msg>,
    /// 整体在一个事务中执行（PG 家族）。
    pub transactional: bool,
    /// 被执行策略拒绝的原因（有则不能执行）。
    pub denied: Option<Msg>,
    /// 需要用户确认的原因（生产环境或危险语句）。
    pub confirm: Vec<Msg>,
    /// 原表已被修改（与请求中的指纹不同）。
    pub conflict: bool,
}

/// 生成好的执行计划。
enum Plan {
    Alter(Vec<DesignStatement>),
    Rebuild(Rebuild),
}

struct Prepared {
    profile: Profile,
    dialect: Dialect,
    preview: DesignPreview,
    plan: Option<Plan>,
    /// 其他表指向本表的外键所在的 schema（执行后这些 schema 的缓存也失效）。
    related: Vec<String>,
}

fn fingerprint(t: &Table) -> String {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    serde_json::to_string(t).unwrap_or_default().hash(&mut h);
    format!("{:016x}", h.finish())
}

fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64)
}

impl Service {
    pub async fn design_caps(&self, profile_id: &str) -> Result<DesignCaps> {
        let profile = self.profile(profile_id)?;
        Ok(self.metadata.dialect(&profile).await?.design_caps())
    }

    /// 元数据查询，跳过缓存（设计器要以库中最新的结构为准）。
    async fn fresh_rows(
        &self,
        profile: &Profile,
        key: MetaKey,
        schema: &str,
        table: &str,
    ) -> Result<Arc<MetaRows>> {
        let args = MetaArgs { schema: Some(schema.into()), table: Some(table.into()), ..Default::default() };
        self.metadata.cache().forget(&profile.id, key, &args);
        Ok(self.metadata.rows(profile, key, &args).await?.unwrap_or_default())
    }

    async fn fresh_table(&self, profile: &Profile, schema: &str, name: &str) -> Result<Table> {
        let args = MetaArgs { schema: Some(schema.into()), table: Some(name.into()), ..Default::default() };
        for key in MetaKey::TABLE_DETAIL {
            self.metadata.cache().forget(&profile.id, key, &args);
        }
        self.metadata.tables(profile, schema, Some(name)).await?.into_iter().next().ok_or_else(|| {
            Error::not_found(msg!("noun.object", kind = "table", name = format!("{schema}.{name}")))
        })
    }

    /// 打开设计器：原表（直接从库读取）、存储属性和行数估计、指纹。
    pub async fn design_table(&self, profile_id: &str, schema: &str, name: &str) -> Result<DesignTable> {
        let profile = self.profile(profile_id)?;
        let table = self.fresh_table(&profile, schema, name).await?;
        let storage =
            meta::table_storage(&*self.fresh_rows(&profile, MetaKey::TableStorage, schema, name).await?)?;
        Ok(DesignTable { fingerprint: fingerprint(&table), table, storage })
    }

    /// 重建脚本要用的依赖信息。
    async fn rebuild_context(
        &self,
        profile: &Profile,
        dialect: &Dialect,
        schema: &str,
        t: &Table,
    ) -> Result<RebuildContext> {
        let name = &t.name;
        let rows = |key| self.fresh_rows(profile, key, schema, name);
        let mut ctx = RebuildContext {
            referencing: meta::referencing_keys(&*rows(MetaKey::ReferencingKeys).await?, schema, name)?,
            storage: meta::table_storage(&*rows(MetaKey::TableStorage).await?)?,
            ..Default::default()
        };
        if dialect.has_meta(MetaKey::DependentViews) {
            ctx.views = meta::dependent_views(&*rows(MetaKey::DependentViews).await?)?;
        }
        if dialect.has_meta(MetaKey::Grants) {
            ctx.grants = meta::grants(&*rows(MetaKey::Grants).await?)?;
        }
        ctx.triggers = if dialect.has_meta(MetaKey::TriggerDdl) {
            meta::trigger_ddl(&*rows(MetaKey::TriggerDdl).await?)?
        } else {
            t.triggers.iter().filter_map(|tr| tr.definition.clone()).collect()
        };
        Ok(ctx)
    }

    async fn prepare_design(&self, profile_id: &str, req: &DesignRequest) -> Result<Prepared> {
        let profile = self.profile(profile_id)?;
        let dialect = self.metadata.dialect(&profile).await?;
        let mut draft = req.draft.clone();
        if let Some(name) = &req.name {
            // 已有表不能改名
            draft.name = name.clone();
        }
        let mut warnings = dialect.prepare_draft(&mut draft);
        let original = match &req.name {
            Some(n) => Some(self.fresh_table(&profile, &req.schema, n).await?),
            None => None,
        };
        let conflict = match (&original, &req.fingerprint) {
            (Some(t), Some(fp)) => &fingerprint(t) != fp,
            _ => false,
        };
        let mut preview = DesignPreview {
            errors: validate(&draft),
            changes: vec![],
            statements: vec![],
            rebuild: RebuildInfo { reasons: vec![], active: false, refused: None },
            warnings: vec![],
            transactional: dialect.design_transactional(),
            denied: None,
            confirm: vec![],
            conflict,
        };
        let mut prepared =
            Prepared { profile, dialect, preview: preview.clone(), plan: None, related: vec![] };
        if !preview.errors.is_empty() {
            prepared.preview = preview;
            return Ok(prepared);
        }
        let diff = diff_table(original.as_ref(), &draft, &req.renames)?;
        preview.rebuild.reasons = dialect.design_unsupported(&diff);
        preview.changes = diff.changes.clone();
        let rebuild = original.is_some()
            && !diff.changes.is_empty()
            && (req.rebuild || !preview.rebuild.reasons.is_empty());
        let plan = match (&original, rebuild) {
            _ if diff.changes.is_empty() => None,
            (Some(orig), true) => {
                preview.rebuild.active = true;
                let ctx = self.rebuild_context(&prepared.profile, &dialect, &req.schema, orig).await?;
                prepared.related = ctx.referencing.iter().map(|r| r.schema.clone()).collect();
                match dialect.rebuild(&req.schema, orig, &draft, &req.renames, &ctx)? {
                    RebuildOutcome::Refused { reason } => {
                        preview.rebuild.refused = Some(reason);
                        None
                    }
                    RebuildOutcome::Script(r) => {
                        warnings.extend(r.warnings.iter().cloned());
                        preview.statements = r
                            .steps
                            .iter()
                            .map(|s| PreviewStatement { step: s.clone(), changes: vec![] })
                            .collect();
                        Some(Plan::Rebuild(r))
                    }
                }
            }
            _ => {
                let statements = dialect.design_sql(&req.schema, &draft.name, &diff)?;
                preview.statements = statements
                    .iter()
                    .map(|s| PreviewStatement {
                        step: RebuildStep::Sql { sql: s.sql.clone() },
                        changes: s.changes.clone(),
                    })
                    .collect();
                Some(Plan::Alter(statements))
            }
        };
        preview.warnings = warnings;
        // 执行策略：被拒绝的语句不能执行；需要确认的列出原因（生产环境总要确认）
        let ctx = ExecContext {
            profile_id: prepared.profile.id.clone(),
            environment: prepared.profile.input.environment,
            dialect,
        };
        let policy = self.sessions.policy();
        for s in &preview.statements {
            let RebuildStep::Sql { sql } = &s.step else { continue };
            match policy.before_execute(&ctx, &classify(&dialect, sql)).await {
                Decision::Allow => {}
                Decision::Confirm { reason } => {
                    if !preview.confirm.contains(&reason) {
                        preview.confirm.push(reason);
                    }
                }
                Decision::Deny { reason } => {
                    preview.denied.get_or_insert(reason);
                }
            }
        }
        prepared.preview = preview;
        prepared.plan = plan;
        Ok(prepared)
    }

    /// 预览：校验错误、变更列表、生成的语句或重建脚本、提示、执行策略的结论。不执行。
    pub async fn preview_design(&self, profile_id: &str, req: &DesignRequest) -> Result<DesignPreview> {
        Ok(self.prepare_design(profile_id, req).await?.preview)
    }

    /// 执行：作为任务在单独的连接上运行，返回任务 id。有校验错误、被拒绝、原表已被修改、没有改动时报错。
    pub async fn start_design(self: &Arc<Self>, profile_id: &str, req: DesignRequest) -> Result<String> {
        let prepared = self.prepare_design(profile_id, &req).await?;
        let p = &prepared.preview;
        if !p.errors.is_empty() {
            return Err(Error::invalid(msg!("design.has_errors")));
        }
        if p.conflict {
            return Err(Error::invalid(msg!("design.conflict")));
        }
        if let Some(reason) = &p.rebuild.refused {
            return Err(Error::invalid(reason.clone()));
        }
        if let Some(reason) = &p.denied {
            return Err(Error::Denied { reason: reason.clone() });
        }
        let Some(plan) = prepared.plan else {
            return Err(Error::invalid(msg!("design.no_changes")));
        };
        let password = self.secrets.get(&prepared.profile.id)?.unwrap_or_default();
        let task = self.tasks.create(TaskKind::Design)?;
        let mut id = String::new();
        let total = match &plan {
            Plan::Alter(s) => s.len(),
            Plan::Rebuild(r) => r.steps.len(),
        } as u64;
        task.update(|s| {
            id = s.id.clone();
            s.phase = TaskPhase::Executing;
            s.progress.statements_total = total;
        });
        let service = self.clone();
        let (profile, dialect, schema) = (prepared.profile, prepared.dialect, req.schema);
        let mut schemas = prepared.related;
        schemas.push(schema.clone());
        tokio::spawn(async move {
            let run = Run { service: &service, task: &task, profile: &profile, dialect, schema: &schema };
            let result = run.execute(plan, password).await;
            for s in &schemas {
                service.metadata.cache().invalidate(&profile.id, Some(s));
            }
            task.finish(result);
        });
        Ok(id)
    }
}

/// 一次执行。
struct Run<'a> {
    service: &'a Service,
    task: &'a TaskHandle,
    profile: &'a Profile,
    dialect: Dialect,
    schema: &'a str,
}

impl Run<'_> {
    fn record(&self, sql: &str, at: i64, started: Instant, rows: Option<u64>, error: Option<&Error>) {
        self.service.history.record(NewHistory {
            at,
            profile_id: self.profile.id.clone(),
            profile_name: self.profile.input.name.clone(),
            driver: driver_name(self.profile.input.driver).to_string(),
            variant: Some(variant_code(self.dialect.variant())),
            schema: Some(self.schema.to_string()),
            sql: sql.to_string(),
            elapsed_ms: Some(started.elapsed().as_millis() as u64),
            rows,
            error: error.map(ToString::to_string),
        });
    }

    /// 执行一条语句并记入历史。
    async fn sql(&self, conn: &mut Box<dyn Connection>, sql: &str) -> Result<u64> {
        let (at, started) = (now_ms(), Instant::now());
        let result = exec(conn, sql, &[]).await;
        self.record(sql, at, started, result.as_ref().ok().copied(), result.as_ref().err());
        result
    }

    async fn count(&self, conn: &mut Box<dyn Connection>, table: &str) -> Result<i64> {
        let sql = format!("SELECT COUNT(*) AS n FROM {table}");
        let mut stream = conn.execute(&sql, &[]).await?;
        let rows = MetaRows::from_events(collect_all(stream.as_mut(), 10).await?)?;
        Ok(rows.iter().next().and_then(|r| r.int("n")).unwrap_or(-1))
    }

    /// 第 `index` 步失败：记下失败的语句和没有执行的语句。
    fn failed(&self, index: usize, text: &str, error: &Error, rest: &[String]) {
        let failure = TaskFailure {
            index: index as u64,
            line: index as u64 + 1,
            error: error.clone(),
            text: text.into(),
        };
        self.task.fail(failure, text.into());
        let rest = rest.join(";\n");
        self.task.update(|s| s.remaining = (!rest.is_empty()).then(|| rest + ";"));
    }

    async fn execute(&self, plan: Plan, password: String) -> Result<()> {
        let (mut conn, _) = self.service.registry.connect(self.profile, password).await?;
        self.task.set_cancel(Some(conn.cancel_handle()))?;
        let transactional = self.dialect.design_transactional();
        let outcome = async {
            exec(&mut conn, &self.dialect.set_schema_sql(self.schema), &[]).await?;
            if transactional {
                conn.set_autocommit(false).await?;
            }
            match plan {
                Plan::Alter(statements) => {
                    let texts: Vec<String> = statements.iter().map(|s| s.sql.clone()).collect();
                    for (i, sql) in texts.iter().enumerate() {
                        if self.task.cancelled() {
                            return Err(Error::Cancelled);
                        }
                        if let Err(e) = self.sql(&mut conn, sql).await {
                            let e = if self.task.cancelled() { Error::Cancelled } else { e };
                            // PG 回滚全部，没有执行的语句不必再列出
                            self.failed(i, sql, &e, if transactional { &[] } else { &texts[i + 1..] });
                            return Err(e);
                        }
                        self.task.update(|s| s.progress.statements_done += 1);
                    }
                }
                Plan::Rebuild(r) => self.rebuild(&mut conn, &r, transactional).await?,
            }
            if transactional {
                conn.commit().await?;
            }
            Ok(())
        }
        .await;
        if outcome.is_err() && transactional {
            let _ = conn.rollback().await;
        }
        let _ = conn.close().await;
        outcome
    }

    async fn rebuild(&self, conn: &mut Box<dyn Connection>, r: &Rebuild, transactional: bool) -> Result<()> {
        let texts: Vec<String> = r
            .steps
            .iter()
            .map(|s| match s {
                RebuildStep::Sql { sql } => sql.clone(),
                RebuildStep::CountCheck { source, target } => {
                    format!("SELECT COUNT(*) FROM {source}; SELECT COUNT(*) FROM {target}")
                }
            })
            .collect();
        for (i, step) in r.steps.iter().enumerate() {
            if i == r.drop_at {
                // 从删除旧表起不能取消
                self.task.set_cancel(None)?;
            }
            let before_drop = i < r.drop_at;
            if before_drop && self.task.cancelled() {
                self.cleanup(conn, r, transactional).await;
                return Err(Error::Cancelled);
            }
            let result = match step {
                RebuildStep::Sql { sql } => self.sql(conn, sql).await.map(|_| ()),
                RebuildStep::CountCheck { source, target } => {
                    async {
                        let (a, b) = (self.count(conn, source).await?, self.count(conn, target).await?);
                        if a == b {
                            Ok(())
                        } else {
                            Err(Error::invalid(msg!("design.count_mismatch", source = a, target = b)))
                        }
                    }
                    .await
                }
            };
            if let Err(e) = result {
                let e = if before_drop && self.task.cancelled() { Error::Cancelled } else { e };
                let rest = if transactional || before_drop { &[][..] } else { &texts[i + 1..] };
                self.failed(i, &texts[i], &e, rest);
                if before_drop {
                    self.cleanup(conn, r, transactional).await;
                }
                return Err(e);
            }
            self.task.update(|s| s.progress.statements_done += 1);
        }
        Ok(())
    }

    /// 删除旧表之前失败或取消：删除新表（PG 由回滚撤销，不必删除）。
    async fn cleanup(&self, conn: &mut Box<dyn Connection>, r: &Rebuild, transactional: bool) {
        if !transactional {
            let _ = self.sql(conn, &r.cleanup).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use dblens_core::schema::{Column, PrimaryKey};
    use dblens_core::{ColumnType, LogicalType};
    use dblens_dialect::Variant;

    use crate::fake::{FakeDriver, Log};
    use crate::history::{HistoryLog, HistoryQuery};
    use crate::policy::DefaultPolicy;
    use crate::profile::tests::input;
    use crate::profile::{Environment, ProfileInput};
    use crate::registry::DriverRegistry;
    use crate::secret::{SecretBackend, Secrets};
    use crate::store::Store;
    use crate::task::{TaskState, TaskStatus};

    use super::*;

    fn service() -> (Arc<Service>, String, Log, tempfile::TempDir) {
        let store = Arc::new(Store::open_in_memory().unwrap());
        let secrets = Arc::new(Secrets::with_backend(store.clone(), SecretBackend::Vault));
        secrets.init_vault("master").unwrap();
        let log: Log = Arc::default();
        let registry = Arc::new(DriverRegistry::with_drivers(
            store.clone(),
            vec![Box::new(FakeDriver { log: log.clone() })],
        ));
        let s = Service::with_history(
            store.clone(),
            secrets,
            registry,
            Arc::new(DefaultPolicy),
            HistoryLog::inline(store),
        );
        let dir = tempfile::tempdir().unwrap();
        s.tasks.set_dirs(dir.path().join("tasks"), dir.path().join("imports"));
        let dev = s
            .create_profile(ProfileInput { environment: Environment::Dev, ..input("dev") }, Some("pw"))
            .unwrap();
        (Arc::new(s), dev.id, log, dir)
    }

    async fn wait(s: &Service, id: &str) -> TaskStatus {
        for _ in 0..1000 {
            let st = s.tasks.status(id).unwrap();
            if st.state != TaskState::Running {
                return st;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("任务没有结束");
    }

    fn executed(log: &Log) -> Vec<String> {
        log.lock().unwrap().iter().filter_map(|l| l.strip_prefix("execute ").map(str::to_string)).collect()
    }

    fn new_table(name: &str) -> DesignRequest {
        let col = |name: &str, native: &str| Column {
            name: name.into(),
            ordinal: 0,
            data_type: ColumnType { native: native.into(), logical: LogicalType::Other },
            nullable: true,
            default: None,
            comment: Some("说明".into()),
            auto_increment: false,
            generated: None,
        };
        DesignRequest {
            schema: "public".into(),
            name: None,
            draft: Table {
                name: name.into(),
                columns: vec![col("id", "bigint"), col("名称", "varchar(20)")],
                primary_key: Some(PrimaryKey { name: None, columns: vec!["id".into()] }),
                ..Default::default()
            },
            renames: Renames::default(),
            rebuild: false,
            fingerprint: None,
        }
    }

    fn sql_of(p: &DesignPreview) -> Vec<String> {
        p.statements
            .iter()
            .map(|s| match &s.step {
                RebuildStep::Sql { sql } => sql.clone(),
                RebuildStep::CountCheck { .. } => panic!("新建表不会核对行数"),
            })
            .collect()
    }

    #[tokio::test]
    async fn preview_new_table() {
        let (s, dev, _log, _dir) = service();
        let p = s.preview_design(&dev, &new_table("t")).await.unwrap();
        assert!(p.errors.is_empty() && p.transactional && !p.conflict && p.denied.is_none());
        assert!(matches!(p.changes[..], [TableChange::CreateTable { .. }]));
        let sql = sql_of(&p);
        assert!(sql[0].starts_with("CREATE TABLE public.t (\n    id bigint NOT NULL,"), "{}", sql[0]);
        assert_eq!(sql.len(), 3, "建表加两条列注释");
        assert_eq!(p.warnings, vec![msg!("design.pk_not_null", column = "id")]);
        // 校验错误：不生成语句，不能执行
        let mut bad = new_table("");
        bad.draft.columns[1].data_type.native = String::new();
        let p = s.preview_design(&dev, &bad).await.unwrap();
        assert_eq!(p.errors.len(), 2);
        assert!(p.statements.is_empty());
        assert!(matches!(s.start_design(&dev, bad).await, Err(Error::Invalid { .. })));
    }

    #[tokio::test]
    async fn apply_runs_in_one_transaction_and_records_history() {
        let (s, dev, log, _dir) = service();
        let id = s.start_design(&dev, new_table("t")).await.unwrap();
        let st = wait(&s, &id).await;
        assert_eq!(st.state, TaskState::Done, "{st:?}");
        assert_eq!((st.progress.statements_done, st.progress.statements_total), (3, 3));
        let l = log.lock().unwrap().clone();
        let at = |x: &str| l.iter().position(|e| e == x).unwrap_or_else(|| panic!("{x} 不在 {l:?}"));
        assert!(at("execute SET search_path TO \"public\"") < at("autocommit false"));
        assert!(at("autocommit false") < at("commit"));
        assert_eq!(s.store.history(&HistoryQuery::default()).unwrap().len(), 3);
    }

    /// 直接执行一份重建计划（MySQL：不在事务中）。删除旧表之前失败要删除新表；之后失败列出没有执行的语句。
    #[tokio::test]
    async fn rebuild_failure_before_and_after_dropping_the_old_table() {
        let (s, dev, log, _dir) = service();
        let profile = s.profile(&dev).unwrap();
        let sql = |s: &str| RebuildStep::Sql { sql: s.into() };
        let count = || RebuildStep::CountCheck { source: "t".into(), target: "t_new".into() };
        let run = |steps: Vec<RebuildStep>| {
            let (s, profile) = (s.clone(), profile.clone());
            async move {
                let task = s.tasks.create(TaskKind::Design).unwrap();
                let mut id = String::new();
                task.update(|st| id = st.id.clone());
                let plan =
                    Rebuild { steps, drop_at: 3, cleanup: "DROP TABLE t_new".into(), warnings: vec![] };
                let run = Run {
                    service: &s,
                    task: &task,
                    profile: &profile,
                    dialect: Dialect::new(Variant::MySql),
                    schema: "shop",
                };
                let r = run.execute(Plan::Rebuild(plan), "pw".into()).await;
                task.finish(r);
                s.tasks.status(&id).unwrap()
            }
        };

        let st = run(vec![sql("CREATE TABLE t_new"), sql("FAIL"), count(), sql("DROP TABLE t")]).await;
        assert_eq!(st.state, TaskState::Failed);
        assert_eq!(st.remaining, None, "删除旧表之前失败：旧表完好，不列出剩余语句");
        let ex = executed(&log);
        assert_eq!(ex.last().unwrap(), "DROP TABLE t_new", "{ex:?}");
        assert!(!ex.contains(&"DROP TABLE t".to_string()));

        log.lock().unwrap().clear();
        let st = run(vec![
            sql("CREATE TABLE t_new"),
            sql("INSERT INTO t_new SELECT * FROM t"),
            count(),
            sql("DROP TABLE t"),
            sql("FAIL"),
            sql("CREATE INDEX a ON t (x)"),
            sql("CREATE INDEX b ON t (y)"),
        ])
        .await;
        assert_eq!(st.state, TaskState::Failed);
        assert_eq!(st.failures[0].index, 4);
        assert_eq!(st.remaining.as_deref(), Some("CREATE INDEX a ON t (x);\nCREATE INDEX b ON t (y);"));
        let ex = executed(&log);
        assert!(ex.contains(&"SELECT COUNT(*) AS n FROM t_new".to_string()), "{ex:?}");
        assert!(!ex.contains(&"DROP TABLE t_new".to_string()), "删除旧表之后不再删除新表");
    }
}
```

- [ ] **Step 3: 修改 `crates/dblens-service/src/fake.rs`**

```diff
--- a/crates/dblens-service/src/fake.rs
+++ b/crates/dblens-service/src/fake.rs
@@ -3,6 +3,7 @@
 //! - `ROWS n`：返回一个结果集，n 行，每行一个整数；
 //! - `FAIL`：SQL 错误；`FAILAT n`：带出错位置 n 的 SQL 错误；`DISCONNECT`：连接错误；
 //! - `SELECT current_schema()`：当前 schema（初始为 public），`SET search_path TO "x"` 切换它；
+//! - `SELECT COUNT(*) …`：一行一列的 3（表设计器重建时核对行数）；
 //! - `SLEEP`：一直等到被取消，然后返回 `Cancelled`；
 //! - 参数中有文本 `FAIL` 时报 SQL 错误，有 `SLEEP` 时同 `SLEEP`（导入测试用）；
 //! - 其他：影响 1 行。
@@ -156,6 +157,13 @@ impl Connection for FakeConn {
                 stream.events.push_back(ExecEvent::Affected(0));
             }
             "SLEEP" => stream.sleep = Some((self.cancelled.clone(), self.wake.clone())),
+            _ if sql.starts_with("SELECT COUNT(*) ") => {
+                stream
+                    .events
+                    .push_back(ExecEvent::ResultStart { columns: vec![ColumnMeta::new("n", "int8")] });
+                stream.events.push_back(ExecEvent::Rows(vec![vec![Value::Int(3)]]));
+                stream.events.push_back(ExecEvent::ResultEnd);
+            }
             // 表数据编辑的冲突：没有影响任何行
             _ if sql.contains("zero_rows") => stream.events.push_back(ExecEvent::Affected(0)),
             _ => match sql.strip_prefix("ROWS ").and_then(|n| n.parse::<i64>().ok()) {
```

- [ ] **Step 4: 修改 `crates/dblens-service/src/lib.rs`**

```diff
--- a/crates/dblens-service/src/lib.rs
+++ b/crates/dblens-service/src/lib.rs
@@ -1,6 +1,7 @@
 //! 服务层：连接配置、密码、驱动注册表、会话、元数据缓存、执行策略。供 `dblens-api` 使用，自身不处理 HTTP。
 
 mod changes;
+mod design;
 mod export;
 /// 测试用的假驱动，供本 crate 和 `dblens-api` 的测试使用（feature `testing`）。
 #[cfg(any(test, feature = "testing"))]
@@ -21,6 +22,7 @@ mod store;
 mod task;
 
 pub use changes::ChangeSet;
+pub use design::{DesignPreview, DesignRequest, DesignTable, PreviewStatement, RebuildInfo};
 pub use export::{
     EXPORT_LOB_LIMIT, ExportFormat, ExportManager, ExportOptions, ExportRequest, ExportSource, ExportState,
     ExportStatus, ExportWarnings, MAX_RUNNING_EXPORTS,
```

- [ ] **Step 5: 修改 `crates/dblens-service/src/metadata.rs`**

```diff
--- a/crates/dblens-service/src/metadata.rs
+++ b/crates/dblens-service/src/metadata.rs
@@ -42,6 +42,13 @@ impl MetadataCache {
         }
     }
 
+    /// 丢掉一条缓存的查询结果（下次重新查询）。
+    pub fn forget(&self, profile_id: &str, key: MetaKey, args: &MetaArgs) {
+        if let Ok(mut e) = self.entries.lock() {
+            e.remove(&(profile_id.to_string(), key, args.clone()));
+        }
+    }
+
     pub fn len(&self) -> usize {
         self.entries.lock().map(|e| e.len()).unwrap_or(0)
     }
```

- [ ] **Step 6: 修改 `crates/dblens-service/src/task.rs`**

```diff
--- a/crates/dblens-service/src/task.rs
+++ b/crates/dblens-service/src/task.rs
@@ -24,6 +24,8 @@ pub const MAX_FAILURES: usize = 100;
 pub enum TaskKind {
     Import,
     SqlFile,
+    /// 表设计器执行改动（M3）。
+    Design,
 }
 
 #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
@@ -109,6 +111,8 @@ pub struct TaskStatus {
     pub confirm: Option<TaskConfirm>,
     /// 有报告文件（出错报告或执行日志）时为下载用的文件名。
     pub report: Option<String>,
+    /// 表设计器：失败后没有执行的语句（以 `;` 分隔），可在查询标签中打开手动执行。
+    pub remaining: Option<String>,
 }
 
 struct Entry {
@@ -394,6 +398,7 @@ impl TaskManager {
                 committed_line: None,
                 failures: Vec::new(),
                 confirm: None,
+                remaining: None,
                 report: None,
             }),
             cancelled: AtomicBool::new(false),
```

- [ ] **Step 7: 检查**

```bash
export CARGO_INCREMENTAL=0
cargo fmt --all
cargo clippy --config "source.mirror.registry='sparse+https://rsproxy.cn/index/'" --workspace --all-targets -- -D warnings
DBLENS_LANG=en cargo test --config "source.mirror.registry='sparse+https://rsproxy.cn/index/'" --workspace
```

Expected: 全部测试通过（service 新增 3 个）。

- [ ] **Step 8: 提交**

```bash
git add crates/dblens-core/locales/app.json crates/dblens-service/src/design.rs crates/dblens-service/src/fake.rs crates/dblens-service/src/lib.rs crates/dblens-service/src/metadata.rs crates/dblens-service/src/task.rs
git commit -m "feat(service): 表设计器的读表、预览与执行任务" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 7: 实库测试（5 个库）

`tests/live.rs` 的 `design_flow`：用设计器新建表；改名并加长、改可空 / 默认值 / 注释、换索引、加列、改表注释后读回与草稿比对（默认值的写法各库不同，只比较是否有默认值）；原表被别人改过时拒绝执行；强制重建（挪动列、VARCHAR 改为数值类型；PG 有依赖视图时拒绝，删掉视图后再重建）后核对行数、类型转换后的数据、约束、索引、注释、触发器、其他表指向本表的外键、自增接续、授权（PG、Oracle、达梦授予 PUBLIC）、视图仍可用（MySQL、Oracle、达梦）；中途失败时 PG 全部回滚、其他库保留已执行的。

实库中发现并在本任务中修正：索引方法不写与默认方法（BTREE、NORMAL）视为相同（`diff.rs`）；达梦的 `ALL_TAB_PRIVS` 用 `OWNER` 列（`dameng.sql` 和元数据快照）。

**Files:**
- Modify: `crates/dblens-dialect/sql/dameng.sql`
- Modify: `crates/dblens-dialect/src/design/diff.rs`
- Modify: `crates/dblens-dialect/tests/snapshots/meta_snapshots__dameng.snap`
- Modify: `crates/dblens-service/tests/live.rs`

**Interfaces:**
- Consumes：任务 6 的服务层接口。

- [ ] **Step 1: 修改 `crates/dblens-dialect/tests/snapshots/meta_snapshots__dameng.snap`**

```diff
--- a/crates/dblens-dialect/tests/snapshots/meta_snapshots__dameng.snap
+++ b/crates/dblens-dialect/tests/snapshots/meta_snapshots__dameng.snap
@@ -299,11 +299,12 @@ WHERE d.REFERENCED_OWNER = ?
 ORDER BY d.OWNER, d.NAME
 
 -- [grants] [String("HR"), String("EMP")]
+-- DM's ALL_TAB_PRIVS names the schema column OWNER (Oracle: TABLE_SCHEMA).
 SELECT p.GRANTEE AS "grantee",
        p.PRIVILEGE AS "privilege",
        p.GRANTABLE AS "grantable"
 FROM ALL_TAB_PRIVS p
-WHERE p.TABLE_SCHEMA = ?
+WHERE p.OWNER = ?
   AND p.TABLE_NAME = ?
 ORDER BY p.GRANTEE, p.PRIVILEGE
 
```

- [ ] **Step 2: 修改 `crates/dblens-service/tests/live.rs`**

```diff
--- a/crates/dblens-service/tests/live.rs
+++ b/crates/dblens-service/tests/live.rs
@@ -8,16 +8,18 @@ use std::path::Path;
 use std::sync::Arc;
 use std::time::Duration;
 
-use dblens_core::{Error, LogicalType, Value};
-use dblens_dialect::meta::MetaKey;
+use dblens_core::schema::{CheckConstraint, Column, Index, IndexColumn, PrimaryKey, Table, UniqueConstraint};
+use dblens_core::{ColumnType, Error, LogicalType, Value};
+use dblens_dialect::design::{Renames, TableChange, diff_table};
+use dblens_dialect::meta::{MetaArgs, MetaKey};
 use dblens_dialect::{DdlKind, Direction, RowChange, WriteMode};
 use dblens_driver::{ConnectParams, DriverId};
 use dblens_service::{
-    ChangeSet, ColumnMapping, DEFAULT_BATCH, DataOrder, DefaultPolicy, DriverRegistry, Environment,
-    ExecRequest, ExportFormat, ExportOptions, ExportRequest, ExportSource, ExportState, ExportStatus,
-    FileOptions, FileSource, GenerateKind, ImportRequest, NewColumn, NewTable, ObjectKind, OnError,
-    ProfileInput, SecretBackend, Secrets, Service, SessionEvent, SessionId, SqlFileRequest, Store, TaskState,
-    TaskStatus, Transaction,
+    ChangeSet, ColumnMapping, DEFAULT_BATCH, DataOrder, DefaultPolicy, DesignRequest, DriverRegistry,
+    Environment, ExecRequest, ExportFormat, ExportOptions, ExportRequest, ExportSource, ExportState,
+    ExportStatus, FileOptions, FileSource, GenerateKind, ImportRequest, NewColumn, NewTable, ObjectKind,
+    OnError, ProfileInput, SecretBackend, Secrets, Service, SessionEvent, SessionId, SqlFileRequest, Store,
+    TaskState, TaskStatus, Transaction,
 };
 
 struct Live {
@@ -1099,3 +1101,295 @@ async fn oracle_sql_file() {
 async fn dameng_sql_file() {
     sql_file_flow("DBLENS_TEST_DM_URL").await;
 }
+
+/// 表设计器（M3）：新建表；修改列、索引、注释后读回元数据与草稿比对；调整列顺序并改类型（强制重建：
+/// 数据、约束、索引、触发器、其他表指向本表的外键、自增接续、依赖视图）；原表被别人改过时拒绝执行；中途失败。
+async fn design_flow(env: &str) {
+    let Some(live) = setup(env).await else { return };
+    let live = &live;
+    let s = &live.service;
+    let id = s.open_session(&live.profile_id).await.unwrap().id;
+    let upper = matches!(live.driver, DriverId::Oracle | DriverId::Dameng);
+    let n = |name: &str| if upper { name.to_uppercase() } else { name.to_string() };
+    let oracle_drop =
+        |what: &str| format!("BEGIN EXECUTE IMMEDIATE 'DROP {what}'; EXCEPTION WHEN OTHERS THEN NULL; END;");
+    let cleanup = || async {
+        let sqls = match live.driver {
+            DriverId::Oracle => {
+                vec![oracle_drop("VIEW m3_v"), oracle_drop("TABLE m3_items"), oracle_drop("TABLE m3_t")]
+            }
+            _ => vec![
+                "DROP VIEW IF EXISTS m3_v".into(),
+                "DROP TABLE IF EXISTS m3_items".into(),
+                "DROP TABLE IF EXISTS m3_t".into(),
+            ],
+        };
+        for sql in sqls {
+            live.submit(id, ExecRequest::new(sql)).await;
+        }
+    };
+    cleanup().await;
+    let types: [&str; 4] = match live.driver {
+        DriverId::Mysql => ["BIGINT", "VARCHAR(20)", "DECIMAL(10,2)", "VARCHAR(40)"],
+        DriverId::Postgres => ["bigint", "varchar(20)", "numeric(10,2)", "varchar(40)"],
+        DriverId::Oracle => ["NUMBER(19)", "VARCHAR2(20)", "NUMBER(10,2)", "VARCHAR2(40)"],
+        DriverId::Dameng => ["BIGINT", "VARCHAR(20)", "DECIMAL(10,2)", "VARCHAR(40)"],
+    };
+    let col = |name: &str, native: &str| Column {
+        name: n(name),
+        ordinal: 0,
+        data_type: ColumnType { native: native.into(), logical: LogicalType::Other },
+        nullable: true,
+        default: None,
+        comment: None,
+        auto_increment: false,
+        generated: None,
+    };
+    let mut id_col = col("id", types[0]);
+    (id_col.nullable, id_col.auto_increment) = (false, true);
+    let mut amount = col("amount", types[2]);
+    (amount.default, amount.comment) = (Some("0".into()), Some("金额".into()));
+    let draft = Table {
+        name: n("m3_t"),
+        comment: Some("设计器测试".into()),
+        columns: vec![id_col, col("code", types[1]), amount, col("note", types[3])],
+        primary_key: Some(PrimaryKey { name: Some(n("pk_m3_t")), columns: vec![n("id")] }),
+        uniques: vec![UniqueConstraint { name: Some(n("uk_m3_code")), columns: vec![n("code")] }],
+        checks: vec![CheckConstraint {
+            name: Some(n("ck_m3_amount")),
+            expression: format!("{} >= 0", n("amount")),
+        }],
+        indexes: vec![Index {
+            name: n("ix_m3_note"),
+            columns: vec![IndexColumn { name: n("note"), descending: false }],
+            unique: false,
+            method: None,
+        }],
+        ..Default::default()
+    };
+    let request = |name: Option<&str>,
+                   draft: Table,
+                   renames: Renames,
+                   rebuild: bool,
+                   fingerprint: Option<String>| {
+        DesignRequest { schema: live.schema.clone(), name: name.map(n), draft, renames, rebuild, fingerprint }
+    };
+    let apply = |req: DesignRequest| async move {
+        let task = s.start_design(&live.profile_id, req).await.unwrap();
+        wait_task(s, &task).await
+    };
+
+    // 1. 新建表
+    let p = s
+        .preview_design(&live.profile_id, &request(None, draft.clone(), Renames::default(), false, None))
+        .await
+        .unwrap();
+    assert!(p.errors.is_empty() && p.denied.is_none(), "{env}：{p:?}");
+    let st = apply(request(None, draft.clone(), Renames::default(), false, None)).await;
+    assert_eq!(st.state, TaskState::Done, "{env}：{st:?}");
+    for (code, amount, note) in [("a", "1.5", "n1"), ("b", "2.5", "n2"), ("c", "3", "n3")] {
+        live.run(id, &format!("INSERT INTO m3_t (code, amount, note) VALUES ('{code}', {amount}, '{note}')"))
+            .await;
+    }
+    live.run(id, "CREATE TABLE m3_items (item_id INT PRIMARY KEY, order_code VARCHAR(20))").await;
+    live.run(
+        id,
+        "ALTER TABLE m3_items ADD CONSTRAINT fk_m3_items FOREIGN KEY (order_code) REFERENCES m3_t (code)",
+    )
+    .await;
+    live.run(id, "INSERT INTO m3_items VALUES (1, 'a')").await;
+    live.run(id, "CREATE VIEW m3_v AS SELECT id, code FROM m3_t").await;
+    // 2. 修改：note 改名为 remark 并加长，amount 不可空、改默认值和注释，换索引，加列 extra，改表注释
+    let t = s.design_table(&live.profile_id, &live.schema, &n("m3_t")).await.unwrap();
+    assert_eq!(t.table.columns.len(), 4, "{env}");
+    assert!(t.table.columns[0].auto_increment, "{env}：{:?}", t.table.columns[0]);
+    let mut d = t.table.clone();
+    let note_type = d.columns[3].data_type.native.clone();
+    d.columns[3].name = n("remark");
+    d.columns[3].data_type.native = note_type.replace("40", "60");
+    d.columns[2].nullable = false;
+    d.columns[2].default = Some("1".into());
+    d.columns[2].comment = Some("金额（元）".into());
+    let mut extra = col("extra", &note_type.replace("40", "10"));
+    extra.comment = Some("附加".into());
+    d.columns.push(extra);
+    d.indexes = vec![Index {
+        name: n("ix_m3_remark"),
+        columns: vec![IndexColumn { name: n("remark"), descending: true }],
+        unique: false,
+        method: None,
+    }];
+    d.comment = Some("设计器测试（改）".into());
+    let renames = Renames { columns: [(n("remark"), n("note"))].into(), ..Default::default() };
+    let req = request(Some("m3_t"), d.clone(), renames.clone(), false, Some(t.fingerprint.clone()));
+    let p = s.preview_design(&live.profile_id, &req).await.unwrap();
+    assert!(p.errors.is_empty() && !p.rebuild.active && p.rebuild.reasons.is_empty(), "{env}：{p:?}");
+    let st = apply(req).await;
+    assert_eq!(st.state, TaskState::Done, "{env}：{st:?}");
+    let after = s.design_table(&live.profile_id, &live.schema, &n("m3_t")).await.unwrap().table;
+    // 读回的结构与草稿一致（默认值的写法各库不同，只比较是否有默认值）
+    let left = diff_table(Some(&after), &d, &Renames::default()).unwrap().changes;
+    let left: Vec<_> = left
+        .into_iter()
+        .filter(|c| match c {
+            TableChange::AlterColumn { from, to } => {
+                Column { default: None, ..from.clone() } != Column { default: None, ..to.clone() }
+                    && !(from.default.is_some() && to.default.is_some())
+            }
+            _ => true,
+        })
+        .collect();
+    assert!(left.is_empty(), "{env}：读回后仍有差异 {left:?}");
+
+    let trigger: &[&str] = match live.driver {
+        DriverId::Mysql => {
+            &["CREATE TRIGGER m3_trg BEFORE INSERT ON m3_t FOR EACH ROW SET NEW.remark = 'trg'"]
+        }
+        DriverId::Postgres => &[
+            "CREATE OR REPLACE FUNCTION m3_trg_f() RETURNS trigger AS $$ BEGIN NEW.remark := 'trg'; RETURN NEW; END; $$ LANGUAGE plpgsql",
+            "CREATE TRIGGER m3_trg BEFORE INSERT ON m3_t FOR EACH ROW EXECUTE FUNCTION m3_trg_f()",
+        ],
+        DriverId::Oracle | DriverId::Dameng => &[
+            "CREATE OR REPLACE TRIGGER m3_trg BEFORE INSERT ON m3_t FOR EACH ROW BEGIN :NEW.remark := 'trg'; END;",
+        ],
+    };
+    for sql in trigger {
+        live.run(id, sql).await;
+    }
+
+    // 3. 原表被别人改过：拒绝执行
+    let t = s.design_table(&live.profile_id, &live.schema, &n("m3_t")).await.unwrap();
+    live.run(id, "ALTER TABLE m3_t ADD tmp_col INT").await;
+    let mut d = t.table.clone();
+    d.comment = Some("冲突".into());
+    let err = s
+        .start_design(
+            &live.profile_id,
+            request(Some("m3_t"), d, Renames::default(), false, Some(t.fingerprint)),
+        )
+        .await;
+    assert!(matches!(err, Err(Error::Invalid { .. })), "{env}：{err:?}");
+    live.run(id, "ALTER TABLE m3_t DROP COLUMN tmp_col").await;
+
+    // 4. 重建：amount 挪到最前、extra 改成数值类型；PG 有依赖视图时拒绝
+    live.run(id, "UPDATE m3_t SET extra = '10'").await;
+    if live.driver != DriverId::Mysql {
+        live.run(id, "GRANT SELECT ON m3_t TO PUBLIC").await;
+    }
+    let t = s.design_table(&live.profile_id, &live.schema, &n("m3_t")).await.unwrap();
+    let mut d = t.table.clone();
+    let amount = d.columns.remove(2);
+    d.columns.insert(0, amount);
+    let number = match live.driver {
+        DriverId::Mysql => "INT",
+        DriverId::Postgres => "integer",
+        DriverId::Oracle => "NUMBER(10)",
+        DriverId::Dameng => "INT",
+    };
+    d.columns[4].data_type.native = number.into();
+    let req = request(Some("m3_t"), d.clone(), Renames::default(), true, Some(t.fingerprint.clone()));
+    let p = s.preview_design(&live.profile_id, &req).await.unwrap();
+    assert!(p.rebuild.active, "{env}");
+    assert_eq!(
+        p.rebuild.reasons.is_empty(),
+        live.driver == DriverId::Mysql,
+        "{env}：{:?}",
+        p.rebuild.reasons
+    );
+    if live.driver == DriverId::Postgres {
+        assert!(p.rebuild.refused.is_some(), "{env}：PG 有依赖视图时拒绝重建");
+        live.run(id, "DROP VIEW m3_v").await;
+    }
+    let p = s.preview_design(&live.profile_id, &req).await.unwrap();
+    assert!(p.rebuild.refused.is_none(), "{env}：{p:?}");
+    let st = apply(req).await;
+    assert_eq!(st.state, TaskState::Done, "{env}：{st:?}");
+    let after = s.design_table(&live.profile_id, &live.schema, &n("m3_t")).await.unwrap().table;
+    let names: Vec<&str> = after.columns.iter().map(|c| c.name.as_str()).collect();
+    assert_eq!(
+        names,
+        [n("amount"), n("id"), n("code"), n("remark"), n("extra")]
+            .iter()
+            .map(String::as_str)
+            .collect::<Vec<_>>(),
+        "{env}"
+    );
+    assert!(after.columns[1].auto_increment, "{env}：自增保留");
+    assert_eq!(after.columns[0].comment.as_deref(), Some("金额（元）"), "{env}");
+    assert_eq!(after.comment.as_deref(), Some("设计器测试（改）"), "{env}");
+    assert_eq!(after.uniques.len(), 1, "{env}");
+    assert_eq!(
+        after.indexes.iter().map(|x| x.name.as_str()).collect::<Vec<_>>(),
+        [n("ix_m3_remark")],
+        "{env}"
+    );
+    assert_eq!(after.checks.len(), draft.checks.len(), "{env}");
+    assert_eq!(after.triggers.len(), 1, "{env}：触发器重建");
+    let one = |sql: String| async move { text(&first_value(&live.run(id, &sql).await)) };
+    assert_eq!(one("SELECT COUNT(*) FROM m3_t".into()).await.as_deref(), Some("3"), "{env}");
+    assert_eq!(
+        one("SELECT SUM(extra) FROM m3_t".into()).await.as_deref(),
+        Some("30"),
+        "{env}：类型转换后的数据"
+    );
+    // 自增接续、触发器生效
+    live.run(id, "INSERT INTO m3_t (code, amount) VALUES ('d', 4)").await;
+    assert_eq!(one("SELECT MAX(id) FROM m3_t".into()).await.as_deref(), Some("4"), "{env}");
+    assert_eq!(one("SELECT remark FROM m3_t WHERE code = 'd'".into()).await.as_deref(), Some("trg"), "{env}");
+    // 授权恢复（MySQL 的授权按表名记录，不受影响）
+    if live.driver != DriverId::Mysql {
+        let profile = s.profile(&live.profile_id).unwrap();
+        let args =
+            MetaArgs { schema: Some(live.schema.clone()), table: Some(n("m3_t")), ..Default::default() };
+        let rows = s.metadata.rows(&profile, MetaKey::Grants, &args).await.unwrap().unwrap();
+        let grants = dblens_dialect::meta::grants(&rows).unwrap();
+        assert!(grants.iter().any(|g| g.grantee == "PUBLIC" && g.privilege == "SELECT"), "{env}：{grants:?}");
+    }
+    // 其他表指向本表的外键还在
+    let items = s.design_table(&live.profile_id, &live.schema, &n("m3_items")).await.unwrap().table;
+    assert_eq!(items.foreign_keys.len(), 1, "{env}");
+    if live.driver != DriverId::Postgres {
+        assert_eq!(one("SELECT COUNT(*) FROM m3_v".into()).await.as_deref(), Some("4"), "{env}：视图仍可用");
+    }
+
+    // 5. 中途失败：加列后检查约束出错。PG 全部回滚；其他库已执行的保留
+    let t = s.design_table(&live.profile_id, &live.schema, &n("m3_t")).await.unwrap();
+    let mut d = t.table.clone();
+    d.columns.push(col("z", types[1]));
+    d.checks.push(CheckConstraint { name: Some(n("ck_m3_bad")), expression: "no_such_column > 0".into() });
+    let st = apply(request(Some("m3_t"), d, Renames::default(), false, Some(t.fingerprint))).await;
+    assert_eq!(st.state, TaskState::Failed, "{env}：{st:?}");
+    let cols = s.design_table(&live.profile_id, &live.schema, &n("m3_t")).await.unwrap().table.columns.len();
+    assert_eq!(cols, if live.driver == DriverId::Postgres { 5 } else { 6 }, "{env}");
+
+    cleanup().await;
+    if live.driver == DriverId::Postgres {
+        live.run(id, "DROP FUNCTION IF EXISTS m3_trg_f()").await;
+    }
+    s.sessions.close(id, false).await.unwrap();
+}
+
+#[tokio::test]
+async fn mysql_design() {
+    design_flow("DBLENS_TEST_MYSQL_URL").await;
+}
+
+#[tokio::test]
+async fn mariadb_design() {
+    design_flow("DBLENS_TEST_MARIADB_URL").await;
+}
+
+#[tokio::test]
+async fn postgres_design() {
+    design_flow("DBLENS_TEST_PG_URL").await;
+}
+
+#[tokio::test]
+async fn oracle_design() {
+    design_flow("DBLENS_TEST_ORACLE_URL").await;
+}
+
+#[tokio::test]
+async fn dameng_design() {
+    design_flow("DBLENS_TEST_DM_URL").await;
+}
```

- [ ] **Step 3: 修改 `crates/dblens-dialect/sql/dameng.sql`**

```diff
--- a/crates/dblens-dialect/sql/dameng.sql
+++ b/crates/dblens-dialect/sql/dameng.sql
@@ -332,11 +332,12 @@ WHERE d.REFERENCED_OWNER = {{ bind("schema") }}
 ORDER BY d.OWNER, d.NAME
 
 -- [grants]
+-- DM's ALL_TAB_PRIVS names the schema column OWNER (Oracle: TABLE_SCHEMA).
 SELECT p.GRANTEE AS "grantee",
        p.PRIVILEGE AS "privilege",
        p.GRANTABLE AS "grantable"
 FROM ALL_TAB_PRIVS p
-WHERE p.TABLE_SCHEMA = {{ bind("schema") }}
+WHERE p.OWNER = {{ bind("schema") }}
   AND p.TABLE_NAME = {{ bind("table") }}
 ORDER BY p.GRANTEE, p.PRIVILEGE
 
```

- [ ] **Step 4: 修改 `crates/dblens-dialect/src/design/diff.rs`**

```diff
--- a/crates/dblens-dialect/src/design/diff.rs
+++ b/crates/dblens-dialect/src/design/diff.rs
@@ -331,12 +331,12 @@ pub fn diff_table(original: Option<&Table>, draft: &Table, renames: &Renames) ->
             .collect(),
         ..x.clone()
     };
-    let same_index = |x: &Index, y: &Index| {
-        x.columns == y.columns
-            && x.unique == y.unique
-            && x.method.as_deref().map(str::to_ascii_uppercase)
-                == y.method.as_deref().map(str::to_ascii_uppercase)
+    // 索引方法：不写与各库的默认方法（BTREE、NORMAL）相同
+    let method = |x: &Index| {
+        x.method.as_deref().map(str::to_ascii_uppercase).filter(|m| !matches!(m.as_str(), "BTREE" | "NORMAL"))
     };
+    let same_index =
+        |x: &Index, y: &Index| x.columns == y.columns && x.unique == y.unique && method(x) == method(y);
     let mut used = vec![false; orig.indexes.len()];
     let mut renames_ix = Vec::new();
     for x in &draft.indexes {
```

- [ ] **Step 5: 检查**

```bash
export CARGO_INCREMENTAL=0
cargo fmt --all
cargo clippy --config "source.mirror.registry='sparse+https://rsproxy.cn/index/'" --workspace --all-targets -- -D warnings
DBLENS_LANG=en cargo test --config "source.mirror.registry='sparse+https://rsproxy.cn/index/'" --workspace
```

实库测试：在上面的 `cargo test` 前以内联环境变量给出 `DBLENS_TEST_MYSQL_URL`、`DBLENS_TEST_MARIADB_URL`、`DBLENS_TEST_PG_URL`、`DBLENS_TEST_ORACLE_URL`、`DBLENS_TEST_DM_URL`（密码不写进任何文件）。达梦的测试与其他测试同时连库时偶发连接失败，失败时用 `-- dameng --test-threads=1` 单独重跑。

Expected: 全部测试通过（实库 5 个）。

- [ ] **Step 6: 提交**

```bash
git add crates/dblens-dialect/sql/dameng.sql crates/dblens-dialect/src/design/diff.rs crates/dblens-dialect/tests/snapshots/meta_snapshots__dameng.snap crates/dblens-service/tests/live.rs
git commit -m "test(service): 表设计器的实库测试（5 个库）" -m "实库中发现并修正：索引方法不写与默认方法（BTREE、NORMAL）相同；达梦的 ALL_TAB_PRIVS 用 OWNER 列。" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 8: 接口

`dblens-api/src/design.rs`：`GET /profiles/{id}/design/caps`、`GET /profiles/{id}/design/table?schema=&name=`、`POST /profiles/{id}/design/preview`、`POST /profiles/{id}/design/apply`（201 和任务 id，状态用 `/tasks/{id}`）。错误和提示按请求的语言渲染。

**Files:**
- Create: `crates/dblens-api/src/design.rs`
- Modify: `crates/dblens-api/src/lib.rs`
- Modify: `crates/dblens-api/tests/api.rs`

**Interfaces:**
- Consumes：任务 6 的 `Service` 方法。
- Produces：上述接口。

- [ ] **Step 1: 修改 `crates/dblens-api/tests/api.rs`**

```diff
--- a/crates/dblens-api/tests/api.rs
+++ b/crates/dblens-api/tests/api.rs
@@ -685,3 +685,47 @@ async fn uploads_respect_the_limit_and_paths_are_desktop_only() {
     .await;
     assert_eq!(st, StatusCode::BAD_REQUEST, "浏览器模式不接受本机路径");
 }
+
+/// 表设计器（M3）：能力和类型目录、预览新建表（含校验错误）、执行后任务完成、执行有错误的草稿被拒绝。
+#[tokio::test]
+async fn design_routes() {
+    let (app, _) = app();
+    let (_, p) =
+        call(&app, "POST", "/api/profiles", Some(json!({"profile": profile_json("d"), "password": "pw"})))
+            .await;
+    let id = p["id"].as_str().unwrap().to_string();
+    let (st, caps) = call(&app, "GET", &format!("/api/profiles/{id}/design/caps"), None).await;
+    assert_eq!(st, StatusCode::OK);
+    assert_eq!(caps["reorder"], json!(false));
+    assert!(caps["types"].as_array().unwrap().iter().any(|t| t["name"] == "jsonb" && t["params"] == "none"));
+
+    let col = |name: &str, native: &str| json!({"name": name, "ordinal": 0, "data_type": {"native": native, "logical": {"kind": "other"}}, "nullable": true});
+    let draft = json!({"name": "t", "columns": [col("id", "bigint"), col("note", "text")], "primary_key": {"name": null, "columns": ["id"]}});
+    let body = json!({"schema": "public", "draft": draft});
+    let (st, p) = call(&app, "POST", &format!("/api/profiles/{id}/design/preview"), Some(body.clone())).await;
+    assert_eq!(st, StatusCode::OK, "{p}");
+    assert_eq!(p["errors"], json!([]));
+    assert_eq!(p["changes"][0]["kind"], "create_table");
+    assert_eq!(p["statements"][0]["kind"], "sql");
+    assert!(p["statements"][0]["sql"].as_str().unwrap().starts_with("CREATE TABLE public.t"));
+    assert_eq!(p["warnings"][0], "Primary key column id will be set to NOT NULL");
+    assert_eq!(p["transactional"], json!(true));
+
+    let (st, r) = call(&app, "POST", &format!("/api/profiles/{id}/design/apply"), Some(body)).await;
+    assert_eq!(st, StatusCode::CREATED, "{r}");
+    let task = wait_task(&app, r["id"].as_str().unwrap()).await;
+    assert_eq!((task["kind"].clone(), task["state"].clone()), (json!("design"), json!("done")), "{task}");
+
+    // 校验错误：预览列出位置，执行被拒绝
+    let bad = json!({"schema": "public", "draft": {"name": "", "columns": [col("id", "")]}});
+    let (_, p) = call(&app, "POST", &format!("/api/profiles/{id}/design/preview"), Some(bad.clone())).await;
+    assert_eq!(
+        p["errors"][0],
+        json!({"section": "table", "row": null, "field": "name", "message": "Name is required"})
+    );
+    assert_eq!(p["errors"][1]["field"], "type");
+    let (st, e) = call(&app, "POST", &format!("/api/profiles/{id}/design/apply"), Some(bad)).await;
+    assert_eq!((st, e["kind"].clone()), (StatusCode::BAD_REQUEST, json!("invalid")));
+    let (st, _) = call(&app, "GET", "/api/profiles/nope/design/caps", None).await;
+    assert_eq!(st, StatusCode::NOT_FOUND);
+}
```

- [ ] **Step 2: 新建 `crates/dblens-api/src/design.rs`**

```rust
//! 表设计器的接口（M3 设计文档 6.1 节）：方言能力和类型目录、读原表、预览、执行（返回任务 id，状态用 `/tasks/{id}`）。

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use dblens_dialect::design::DesignCaps;
use dblens_service::{DesignPreview, DesignRequest, DesignTable};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::AppState;
use crate::error::ApiResult;

pub async fn caps(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult<Json<DesignCaps>> {
    Ok(Json(s.service.design_caps(&id).await?))
}

#[derive(Deserialize)]
pub struct TableQuery {
    schema: String,
    name: String,
}

/// 原表（直接从库读取，不走缓存）、存储属性和行数估计、指纹。
pub async fn table(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<TableQuery>,
) -> ApiResult<Json<DesignTable>> {
    Ok(Json(s.service.design_table(&id, &q.schema, &q.name).await?))
}

pub async fn preview(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<DesignRequest>,
) -> ApiResult<Json<DesignPreview>> {
    Ok(Json(s.service.preview_design(&id, &req).await?))
}

/// 执行：后端用草稿重新生成语句，作为任务运行。
pub async fn apply(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<DesignRequest>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    let task = s.service.start_design(&id, req).await?;
    Ok((StatusCode::CREATED, Json(json!({ "id": task }))))
}
```

- [ ] **Step 3: 修改 `crates/dblens-api/src/lib.rs`**

```diff
--- a/crates/dblens-api/src/lib.rs
+++ b/crates/dblens-api/src/lib.rs
@@ -2,6 +2,7 @@
 //! 另提供前端页面的静态文件服务（`with_web`）。桌面模式和浏览器模式（`dblens-server`）共用。所有请求都要带访问令牌，浏览器请求还要通过 Origin 校验。
 
 mod auth;
+mod design;
 mod error;
 mod exports;
 mod lang;
@@ -84,6 +85,10 @@ pub fn router(service: Arc<Service>, config: ApiConfig) -> Router {
         .route("/profiles/{id}/schemas/{schema}/data-sql", post(routes::data_sql))
         .route("/profiles/{id}/ops", get(exports::ops))
         .route("/profiles/{id}/changes/preview", post(routes::preview_changes))
+        .route("/profiles/{id}/design/caps", get(design::caps))
+        .route("/profiles/{id}/design/table", get(design::table))
+        .route("/profiles/{id}/design/preview", post(design::preview))
+        .route("/profiles/{id}/design/apply", post(design::apply))
         .route("/sessions/{id}/changes", post(routes::apply_changes))
         .route("/exports", post(exports::create))
         .route("/exports/{id}", get(exports::status).delete(exports::cancel_or_remove))
```

- [ ] **Step 4: 检查**

```bash
export CARGO_INCREMENTAL=0
cargo fmt --all
cargo clippy --config "source.mirror.registry='sparse+https://rsproxy.cn/index/'" --workspace --all-targets -- -D warnings
DBLENS_LANG=en cargo test --config "source.mirror.registry='sparse+https://rsproxy.cn/index/'" --workspace
```

Expected: 全部测试通过（api 新增 1 个）。

- [ ] **Step 5: 提交**

```bash
git add crates/dblens-api/src/design.rs crates/dblens-api/src/lib.rs crates/dblens-api/tests/api.rs
git commit -m "feat(api): 表设计器的接口" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 9: 前端：接口客户端、草稿逻辑与状态

- `shared/api.ts`：`DesignColumn`、`DesignKey`、`DesignCheck`、`DesignForeignKey`、`DesignIndex`、`DesignTableModel`、`TypeEntry`、`DesignCaps`、`DesignTableResult`、`DesignRequest`、`TableChange`、`DesignIssue`、`DesignStep`、`DesignPreview`；方法 `designCaps`、`designTable`、`previewDesign`、`applyDesign`；`TaskStatus` 加 `kind: 'design'` 和 `remaining`。
- `features/design/logic.ts`：草稿（每行 `key`、`origin`、`deleted`）；`fromTable`、`emptyDraft`、`emptyColumn`、`toModel`（去掉标为删除的行，删除的列也从主键、唯一约束、索引中去掉，引用它的外键整个去掉）、`renamesOf`、`isDirty`；列的 `updateColumn`（改名时改引用它的地方）、`addColumn`、`removeColumn`、`restoreColumn`、`moveColumn`、`togglePrimaryKey`；`splitType` / `joinType`（`TIMESTAMP(6) WITH TIME ZONE` 这类参数插在 WITH 之前，带引号的参数整体当作名称）、`findType`、`typeParams`；改动标记 `changedFields`、`columnState`、`rowState`。
- `designStore.ts`：每个设计器标签的原表、指纹、存储属性、草稿、是否手动重建；`hasDesignChanges`。

**Files:**
- Create: `web/src/features/design/designStore.ts`
- Create: `web/src/features/design/logic.test.ts`
- Create: `web/src/features/design/logic.ts`
- Modify: `web/src/shared/api.ts`

**Interfaces:**
- Consumes：任务 8 的接口。
- Produces：`useApi()` 上的上述方法；`features/design/logic.ts` 的 `Draft`、`DraftColumn`、`DraftKey`、`DraftCheck`、`DraftForeignKey`、`DraftIndex` 和上述函数；`useDesigns`（`set`、`edit`、`setRebuild`、`clear`）、`hasDesignChanges(tabId)`。

- [ ] **Step 1: 新建 `web/src/features/design/logic.test.ts`**

```ts
import { describe, expect, it } from 'vitest'

import type { DesignColumn, DesignTableModel, TypeEntry } from '../../shared/api'
import {
  addColumn,
  changedFields,
  columnState,
  emptyColumn,
  emptyDraft,
  fromTable,
  isDirty,
  joinType,
  moveColumn,
  removeColumn,
  renamesOf,
  restoreColumn,
  splitType,
  toModel,
  togglePrimaryKey,
  typeParams,
  updateColumn,
} from './logic'

const col = (name: string, native: string, extra: Partial<DesignColumn> = {}): DesignColumn => ({
  name,
  ordinal: 0,
  data_type: { native, logical: { kind: 'other' } },
  nullable: true,
  default: null,
  comment: null,
  auto_increment: false,
  generated: null,
  ...extra,
})

const table = (): DesignTableModel => ({
  name: 'orders',
  comment: '订单',
  columns: [col('id', 'bigint', { nullable: false, ordinal: 1 }), col('code', 'varchar(20)', { ordinal: 2 }), col('amount', 'numeric(10,2)', { ordinal: 3 })],
  primary_key: { name: 'pk_orders', columns: ['id'] },
  uniques: [{ name: 'uk_code', columns: ['code'] }],
  foreign_keys: [
    { name: 'fk_parent', columns: ['code'], ref_schema: null, ref_table: 'parent', ref_columns: ['code'], on_delete: null, on_update: null },
  ],
  checks: [{ name: 'ck_amount', expression: 'amount >= 0' }],
  indexes: [{ name: 'ix_amount', columns: [{ name: 'amount', descending: false }], unique: false, method: 'BTREE' }],
  triggers: [],
  partition: null,
})

describe('类型的拆分与拼接', () => {
  it('长度、精度、单位和后缀', () => {
    expect(splitType('NUMBER(10,2)')).toEqual({ name: 'NUMBER', length: '10', scale: '2', unit: '' })
    expect(splitType('VARCHAR2(20 CHAR)')).toEqual({ name: 'VARCHAR2', length: '20', scale: '', unit: 'CHAR' })
    expect(splitType('timestamp(6) with time zone')).toEqual({
      name: 'timestamp with time zone',
      length: '6',
      scale: '',
      unit: '',
    })
    expect(splitType('character varying(20)').name).toBe('character varying')
    expect(splitType("enum('a','b')")).toEqual({ name: "enum('a','b')", length: '', scale: '', unit: '' })
    expect(splitType(' text ')).toEqual({ name: 'text', length: '', scale: '', unit: '' })
  })

  it('拼回原生类型', () => {
    expect(joinType({ name: 'NUMBER', length: '10', scale: '2', unit: '' })).toBe('NUMBER(10,2)')
    expect(joinType({ name: 'VARCHAR2', length: '50', scale: '', unit: 'CHAR' })).toBe('VARCHAR2(50 CHAR)')
    expect(joinType({ name: 'TIMESTAMP WITH TIME ZONE', length: '6', scale: '', unit: '' })).toBe('TIMESTAMP(6) WITH TIME ZONE')
    expect(joinType({ name: 'INT UNSIGNED', length: '10', scale: '', unit: '' })).toBe('INT(10) UNSIGNED')
    expect(joinType({ name: 'text', length: '', scale: '3', unit: '' })).toBe('text')
    for (const t of ['numeric(12,4)', 'VARCHAR2(20 BYTE)', 'timestamp(3) without time zone', 'jsonb']) {
      expect(joinType(splitType(t))).toBe(t)
    }
  })

  it('按类型目录决定可填的参数', () => {
    const types: TypeEntry[] = [
      { name: 'VARCHAR2', params: 'length', char_unit: true },
      { name: 'DATE', params: 'none', char_unit: false },
    ]
    expect(typeParams('varchar2', types)).toBe('length')
    expect(typeParams('DATE', types)).toBe('none')
    expect(typeParams('my_domain', types)).toBe('precision_scale')
    expect(typeParams("enum('a')", types)).toBe('none')
  })
})

describe('草稿', () => {
  it('打开已有表后没有改动，整理回去与原表一致', () => {
    const d = fromTable(table())
    expect(isDirty(d, fromTable(table()))).toBe(false)
    const m = toModel(d)
    expect(m.columns.map((c) => c.ordinal)).toEqual([1, 2, 3])
    expect(m).toEqual({ ...table(), columns: m.columns })
    expect(renamesOf(d)).toEqual({ columns: {}, indexes: {} })
  })

  it('列改名时引用它的地方跟着改，并记下原名', () => {
    const original = fromTable(table())
    const key = original.columns[1].key
    const d = updateColumn(original, key, { name: 'order_code' })
    expect(d.uniques[0].columns).toEqual(['order_code'])
    expect(d.foreignKeys[0].columns).toEqual(['order_code'])
    expect(d.foreignKeys[0].ref_columns, '引用别的表的列不变').toEqual(['code'])
    expect(renamesOf(d)).toEqual({ columns: { order_code: 'code' }, indexes: {} })
    expect(isDirty(d, original)).toBe(true)
    expect(changedFields(d.columns[1], original)).toEqual(new Set(['name']))
    // 还原：名字和引用都改回
    const back = restoreColumn(d, key, original)
    expect(back.uniques[0].columns).toEqual(['code'])
    expect(isDirty(back, original)).toBe(false)
  })

  it('删除：新增的直接去掉；原有的标记删除，整理时从约束和索引中去掉', () => {
    const original = fromTable(table())
    let d = addColumn(original, { ...emptyColumn('note'), data_type: { native: 'text', logical: {} } }, original.columns[0].key)
    expect(d.columns.map((c) => c.name)).toEqual(['id', 'note', 'code', 'amount'])
    expect(columnState(d.columns[1], original)).toBe('new')
    d = removeColumn(d, d.columns[1].key)
    expect(d.columns).toHaveLength(3)
    d = removeColumn(d, d.columns[1].key)
    expect(columnState(d.columns[1], original)).toBe('deleted')
    const m = toModel(d)
    expect(m.columns.map((c) => c.name)).toEqual(['id', 'amount'])
    expect(m.uniques).toEqual([])
    expect(m.foreign_keys, '引用被删列的外键整个去掉').toEqual([])
    expect(restoreColumn(d, d.columns[1].key, original).columns[1].deleted).toBeUndefined()
  })

  it('调整顺序、主键', () => {
    const original = fromTable(table())
    let d = moveColumn(original, original.columns[2].key, -1)
    expect(d.columns.map((c) => c.name)).toEqual(['id', 'amount', 'code'])
    expect(moveColumn(d, d.columns[0].key, -1)).toBe(d)
    d = togglePrimaryKey(d, 'code')
    expect(d.primaryKey).toEqual({ name: 'pk_orders', columns: ['id', 'code'] })
    expect(d.columns.find((c) => c.name === 'code')?.nullable).toBe(false)
    d = togglePrimaryKey(togglePrimaryKey(d, 'code'), 'id')
    expect(d.primaryKey).toBeNull()
  })

  it('新建表：有名字或有列就算改动', () => {
    expect(isDirty(emptyDraft(), null)).toBe(false)
    expect(isDirty({ ...emptyDraft(), name: 't' }, null)).toBe(true)
    expect(isDirty(addColumn(emptyDraft(), emptyColumn()), null)).toBe(true)
  })
})
```

- [ ] **Step 2: 新建 `web/src/features/design/designStore.ts`**

```ts
// 每个设计器标签的状态（不随标签保存）：原表、指纹、草稿、是否手动选择重建。关闭标签前用 hasDesignChanges 判断要不要确认放弃。

import { create } from 'zustand'

import type { DesignTableResult } from '../../shared/api'
import { type Draft, isDirty } from './logic'

export interface DesignState {
  /** 原表（新建表时为 null）。 */
  original: Draft | null
  fingerprint: string | null
  storage: DesignTableResult['storage'] | null
  draft: Draft
  /** 用户在预览中手动选择了重建。 */
  rebuild: boolean
}

interface DesignStore {
  tabs: Record<string, DesignState>
  set(tabId: string, state: DesignState): void
  /** 修改草稿。 */
  edit(tabId: string, fn: (d: Draft) => Draft): void
  setRebuild(tabId: string, on: boolean): void
  clear(tabId: string): void
}

export const useDesigns = create<DesignStore>()((set, get) => ({
  tabs: {},
  set: (tabId, state) => set({ tabs: { ...get().tabs, [tabId]: state } }),
  edit: (tabId, fn) => {
    const s = get().tabs[tabId]
    if (s) set({ tabs: { ...get().tabs, [tabId]: { ...s, draft: fn(s.draft) } } })
  },
  setRebuild: (tabId, on) => {
    const s = get().tabs[tabId]
    if (s) set({ tabs: { ...get().tabs, [tabId]: { ...s, rebuild: on } } })
  },
  clear: (tabId) => {
    const tabs = { ...get().tabs }
    delete tabs[tabId]
    set({ tabs })
  },
}))

export function hasDesignChanges(tabId: string): boolean {
  const s = useDesigns.getState().tabs[tabId]
  return !!s && isDirty(s.draft, s.original)
}
```

- [ ] **Step 3: 新建 `web/src/features/design/logic.ts`**

```ts
// 表设计器（M3 设计文档第 7 节）的纯逻辑：草稿与改名跟踪、类型的拆分与拼接、改动标记、整理成请求。
// 草稿中的每一行有不随改名变化的 key 和原表中的名字 origin（新增的为 null）；原有的行删除时只标记 deleted，可以还原。

import type {
  DesignCheck,
  DesignColumn,
  DesignForeignKey,
  DesignIndex,
  DesignKey,
  DesignRenames,
  DesignTableModel,
  TypeEntry,
  TypeParams,
} from '../../shared/api'

interface Row {
  key: string
  origin: string | null
  deleted?: boolean
}

export type DraftColumn = DesignColumn & Row
export type DraftKey = DesignKey & Row
export type DraftCheck = DesignCheck & Row
export type DraftForeignKey = DesignForeignKey & Row
export type DraftIndex = DesignIndex & Row

export interface Draft {
  name: string
  comment: string | null
  columns: DraftColumn[]
  primaryKey: DesignKey | null
  uniques: DraftKey[]
  checks: DraftCheck[]
  foreignKeys: DraftForeignKey[]
  indexes: DraftIndex[]
  partition: string | null
}

let seq = 0
export const newKey = () => `r${++seq}`

/** 打开已有表：原表的每一行记下原名。没有名字的约束用序号当原名（只用于对照改动标记）。 */
export function fromTable(t: DesignTableModel): Draft {
  const row = <T extends object>(x: T, origin: string | null): T & Row => ({ ...x, key: newKey(), origin })
  return {
    name: t.name,
    comment: t.comment,
    columns: t.columns.map((c) => row(c, c.name)),
    primaryKey: t.primary_key && { ...t.primary_key, columns: [...t.primary_key.columns] },
    uniques: t.uniques.map((u, i) => row(u, u.name ?? `#${i}`)),
    checks: t.checks.map((c, i) => row(c, c.name ?? `#${i}`)),
    foreignKeys: t.foreign_keys.map((f, i) => row(f, f.name ?? `#${i}`)),
    indexes: t.indexes.map((x) => row(x, x.name)),
    partition: t.partition,
  }
}

export function emptyDraft(): Draft {
  return {
    name: '',
    comment: null,
    columns: [],
    primaryKey: null,
    uniques: [],
    checks: [],
    foreignKeys: [],
    indexes: [],
    partition: null,
  }
}

export function emptyColumn(name = ''): DraftColumn {
  return {
    key: newKey(),
    origin: null,
    name,
    ordinal: 0,
    data_type: { native: '', logical: { kind: 'other' } },
    nullable: true,
    default: null,
    comment: null,
    auto_increment: false,
    generated: null,
  }
}

const live = <T extends Row>(rows: T[]) => rows.filter((r) => !r.deleted)

function strip<T extends Row>(r: T): Omit<T, 'key' | 'origin' | 'deleted'> {
  const { key: _k, origin: _o, deleted: _d, ...rest } = r
  return rest
}

/** 整理成请求的表结构：去掉标为删除的行；删除的列也从主键、唯一约束、索引中去掉（去掉后为空的项一并去掉），引用它的外键整个去掉。 */
export function toModel(d: Draft): DesignTableModel {
  const gone = new Set(d.columns.filter((c) => c.deleted && c.origin !== null).map((c) => c.name))
  const keep = (cols: string[]) => cols.filter((c) => !gone.has(c))
  const pk = d.primaryKey && keep(d.primaryKey.columns)
  return {
    name: d.name.trim(),
    comment: d.comment?.trim() ? d.comment : null,
    columns: live(d.columns).map((c, i) => ({ ...strip(c), ordinal: i + 1 })),
    primary_key: pk?.length ? { name: d.primaryKey!.name, columns: pk } : null,
    uniques: live(d.uniques)
      .map((u) => ({ ...strip(u), columns: keep(u.columns) }))
      .filter((u) => u.columns.length),
    checks: live(d.checks).map(strip),
    foreign_keys: live(d.foreignKeys)
      .filter((f) => !f.columns.some((c) => gone.has(c)))
      .map(strip),
    indexes: live(d.indexes)
      .map((x) => ({ ...strip(x), columns: x.columns.filter((c) => !gone.has(c.name)) }))
      .filter((x) => x.columns.length),
    triggers: [],
    partition: d.partition,
  }
}

/** 改了名的列和索引：新名 → 原名。 */
export function renamesOf(d: Draft): DesignRenames {
  const map = (rows: (Row & { name: string })[]) =>
    Object.fromEntries(
      rows.filter((r) => !r.deleted && r.origin !== null && r.origin !== r.name).map((r) => [r.name, r.origin as string]),
    )
  return { columns: map(d.columns), indexes: map(d.indexes) }
}

/** 有没有未保存的改动：新建表时有名字或有列就算有。 */
export function isDirty(d: Draft, original: Draft | null): boolean {
  if (!original) return d.name.trim() !== '' || d.columns.length > 0
  return JSON.stringify(toModel(d)) !== JSON.stringify(toModel(original))
}

// —— 列的编辑 ——

const renameIn = (cols: string[], from: string, to: string) => cols.map((c) => (c === from ? to : c))

/** 修改一列；改名时同时改主键、唯一约束、索引、外键中引用它的地方（指向本表的外键的引用列也改）。 */
export function updateColumn(d: Draft, key: string, patch: Partial<DesignColumn>): Draft {
  const col = d.columns.find((c) => c.key === key)
  if (!col) return d
  const next = { ...d, columns: d.columns.map((c) => (c.key === key ? { ...c, ...patch } : c)) }
  const from = col.name
  const to = patch.name
  if (to === undefined || to === from || from === '') return next
  return {
    ...next,
    primaryKey: next.primaryKey && { ...next.primaryKey, columns: renameIn(next.primaryKey.columns, from, to) },
    uniques: next.uniques.map((u) => ({ ...u, columns: renameIn(u.columns, from, to) })),
    indexes: next.indexes.map((x) => ({
      ...x,
      columns: x.columns.map((c) => (c.name === from ? { ...c, name: to } : c)),
    })),
    foreignKeys: next.foreignKeys.map((f) => ({
      ...f,
      columns: renameIn(f.columns, from, to),
      ref_columns: f.ref_table === d.name ? renameIn(f.ref_columns, from, to) : f.ref_columns,
    })),
  }
}

/** 在 `after` 那一行之后插入新列（不给时加在最后）。 */
export function addColumn(d: Draft, column: DraftColumn, after?: string): Draft {
  const columns = [...d.columns]
  const i = after ? columns.findIndex((c) => c.key === after) : -1
  columns.splice(i < 0 ? columns.length : i + 1, 0, column)
  return { ...d, columns }
}

/** 删除一列：新增的直接去掉，原有的标记删除（可还原）。 */
export function removeColumn(d: Draft, key: string): Draft {
  const col = d.columns.find((c) => c.key === key)
  if (!col) return d
  if (col.origin === null) return { ...d, columns: d.columns.filter((c) => c.key !== key) }
  return { ...d, columns: d.columns.map((c) => (c.key === key ? { ...c, deleted: true } : c)) }
}

/** 还原一列：恢复原表中的定义（改名同样还原，引用它的地方跟着改回）。 */
export function restoreColumn(d: Draft, key: string, original: Draft): Draft {
  const col = d.columns.find((c) => c.key === key)
  const orig = original.columns.find((c) => c.origin === col?.origin)
  if (!col || !orig) return d
  const renamed = updateColumn(d, key, { name: orig.name })
  return { ...renamed, columns: renamed.columns.map((c) => (c.key === key ? { ...orig, key } : c)) }
}

/** 上移（-1）或下移（1）一列。 */
export function moveColumn(d: Draft, key: string, delta: -1 | 1): Draft {
  const i = d.columns.findIndex((c) => c.key === key)
  const j = i + delta
  if (i < 0 || j < 0 || j >= d.columns.length) return d
  const columns = [...d.columns]
  ;[columns[i], columns[j]] = [columns[j], columns[i]]
  return { ...d, columns }
}

/** 勾选或取消主键列：新加的列排在主键的最后。 */
export function togglePrimaryKey(d: Draft, column: string): Draft {
  const cols = d.primaryKey?.columns ?? []
  const next = cols.includes(column) ? cols.filter((c) => c !== column) : [...cols, column]
  const primaryKey = next.length ? { name: d.primaryKey?.name ?? null, columns: next } : null
  // 主键列不能为空
  const columns = d.columns.map((c) => (c.name === column && next.includes(column) ? { ...c, nullable: false } : c))
  return { ...d, primaryKey, columns }
}

/** 在列表中挪动一项（主键列的顺序等）。 */
export function moveItem<T>(list: T[], i: number, delta: -1 | 1): T[] {
  const j = i + delta
  if (j < 0 || j >= list.length) return list
  const out = [...list]
  ;[out[i], out[j]] = [out[j], out[i]]
  return out
}

// —— 类型 ——

export interface TypeParts {
  name: string
  length: string
  scale: string
  unit: '' | 'BYTE' | 'CHAR'
}

/** 拆开原生类型：`NUMBER(10,2)` → NUMBER、10、2；`VARCHAR2(20 CHAR)` → 单位 CHAR；
 * `timestamp(6) with time zone` → 名称 `timestamp with time zone`、长度 6。参数中有引号的（如 enum）整体当作名称。 */
export function splitType(native: string): TypeParts {
  const empty = { name: native.trim(), length: '', scale: '', unit: '' as const }
  const m = /^([^()]*)(?:\(([^)]*)\))?(.*)$/.exec(native)
  if (!m || native.includes("'")) return empty
  const [base, args, suffix] = [m[1].trim(), m[2] ?? '', m[3].trim()]
  const [first = '', second = ''] = args.split(',').map((a) => a.trim())
  const unit = /\s+(BYTE|CHAR)$/i.exec(first)
  return {
    name: suffix ? `${base} ${suffix}` : base,
    length: unit ? first.slice(0, unit.index).trim() : first,
    scale: second,
    unit: unit ? (unit[1].toUpperCase() as 'BYTE' | 'CHAR') : '',
  }
}

/** 拼回原生类型。名称中的 WITH / WITHOUT / UNSIGNED / ZEROFILL 之前插入参数（`TIMESTAMP(6) WITH TIME ZONE`）。 */
export function joinType(p: TypeParts): string {
  const name = p.name.trim()
  const length = p.length.trim()
  const scale = p.scale.trim()
  if (!length) return name
  const args = `${length}${p.unit ? ` ${p.unit}` : ''}${scale ? `,${scale}` : ''}`
  const m = /\s+(with|without|unsigned|zerofill)\b/i.exec(name)
  return m ? `${name.slice(0, m.index)}(${args})${name.slice(m.index)}` : `${name}(${args})`
}

export function findType(name: string, types: TypeEntry[]): TypeEntry | undefined {
  const n = name.trim().toLowerCase()
  return types.find((t) => t.name.toLowerCase() === n)
}

/** 类型的参数形式：目录里没有的类型两项都可以填（带括号的整体名称如 enum 不能再填）。 */
export function typeParams(name: string, types: TypeEntry[]): TypeParams {
  return findType(name, types)?.params ?? (name.includes('(') ? 'none' : 'precision_scale')
}

// —— 改动标记 ——

export type RowState = 'new' | 'changed' | 'deleted' | null

const COLUMN_FIELDS = ['name', 'type', 'nullable', 'default', 'comment', 'auto_increment', 'generated'] as const
export type ColumnField = (typeof COLUMN_FIELDS)[number]

function columnValue(c: DesignColumn, f: ColumnField): unknown {
  if (f === 'type') return c.data_type.native.trim().toLowerCase()
  const v = c[f]
  return typeof v === 'string' ? v.trim() || null : v
}

/** 原有的列改了哪些字段。 */
export function changedFields(c: DraftColumn, original: Draft | null): Set<ColumnField> {
  const orig = original?.columns.find((o) => o.origin === c.origin)
  if (!orig || c.origin === null) return new Set()
  return new Set(COLUMN_FIELDS.filter((f) => columnValue(c, f) !== columnValue(orig, f)))
}

/** 行的状态：新增、修改过、标为删除。`same` 比较原有行和原表中的那一行。 */
export function rowState<T extends Row>(row: T, originals: T[] | undefined, same: (a: T, b: T) => boolean): RowState {
  if (row.deleted) return 'deleted'
  if (row.origin === null) return 'new'
  const orig = originals?.find((o) => o.origin === row.origin)
  return !orig || !same(row, orig) ? 'changed' : null
}

export function columnState(c: DraftColumn, original: Draft | null): RowState {
  if (c.deleted) return 'deleted'
  if (c.origin === null) return 'new'
  return changedFields(c, original).size ? 'changed' : null
}

/** 去掉行标识后比较内容。 */
export const sameContent = <T extends Row>(a: T, b: T) => JSON.stringify(strip(a)) === JSON.stringify(strip(b))
```

- [ ] **Step 4: 修改 `web/src/shared/api.ts`**

```diff
--- a/web/src/shared/api.ts
+++ b/web/src/shared/api.ts
@@ -414,7 +414,7 @@ export interface SqlFileRequest {
 
 export interface TaskStatus {
   id: string
-  kind: 'import' | 'sql_file'
+  kind: 'import' | 'sql_file' | 'design'
   state: 'running' | 'confirm' | 'done' | 'failed' | 'cancelled'
   phase: 'preparing' | 'scanning' | 'creating' | 'clearing' | 'writing' | 'executing' | 'finished'
   progress: {
@@ -435,6 +435,149 @@ export interface TaskStatus {
   failures: { index: number; line: number; error: ErrorBody; text: string }[]
   confirm: { items: { line: number; text: string; reason: string }[]; more: number } | null
   report: string | null
+  /** 表设计器：失败后没有执行的语句。 */
+  remaining: string | null
+}
+
+/** 表设计器（M3）：与后端结构模型一致的完整表结构。 */
+export interface DesignColumn {
+  name: string
+  ordinal: number
+  data_type: { native: string; logical: unknown }
+  nullable: boolean
+  default: string | null
+  comment: string | null
+  auto_increment: boolean
+  generated: string | null
+}
+
+export interface DesignKey {
+  name: string | null
+  columns: string[]
+}
+
+export interface DesignCheck {
+  name: string | null
+  expression: string
+}
+
+export interface DesignForeignKey {
+  name: string | null
+  columns: string[]
+  ref_schema: string | null
+  ref_table: string
+  ref_columns: string[]
+  on_delete: string | null
+  on_update: string | null
+}
+
+export interface DesignIndex {
+  name: string
+  columns: { name: string; descending: boolean }[]
+  unique: boolean
+  method: string | null
+}
+
+export interface DesignTableModel {
+  name: string
+  comment: string | null
+  columns: DesignColumn[]
+  primary_key: DesignKey | null
+  uniques: DesignKey[]
+  foreign_keys: DesignForeignKey[]
+  checks: DesignCheck[]
+  indexes: DesignIndex[]
+  triggers: unknown[]
+  partition: string | null
+}
+
+/** 类型的参数形式：无参数、长度（也用于时间类型的小数秒精度）、精度加小数位。 */
+export type TypeParams = 'none' | 'length' | 'precision_scale'
+
+export interface TypeEntry {
+  name: string
+  params: TypeParams
+  /** 长度可选按 BYTE 还是 CHAR 计（Oracle、达梦的字符类型）。 */
+  char_unit: boolean
+}
+
+/** 方言能力：前端据此禁用库不支持的选项。 */
+export interface DesignCaps {
+  reorder: boolean
+  checks: boolean
+  descending_index: boolean
+  on_delete: string[]
+  on_update: string[]
+  index_methods: string[]
+  types: TypeEntry[]
+}
+
+export interface DesignTableResult {
+  table: DesignTableModel
+  storage: {
+    engine: string | null
+    charset: string | null
+    collation: string | null
+    tablespace: string | null
+    row_count: number | null
+  }
+  fingerprint: string
+}
+
+export interface DesignRenames {
+  columns: Record<string, string>
+  indexes: Record<string, string>
+}
+
+export interface DesignRequest {
+  schema: string
+  /** 已有表的名字；新建表时为 null。 */
+  name: string | null
+  draft: DesignTableModel
+  renames: DesignRenames
+  rebuild: boolean
+  fingerprint: string | null
+}
+
+export type TableChange =
+  | { kind: 'create_table'; table: DesignTableModel }
+  | { kind: 'set_table_comment'; comment: string | null }
+  | { kind: 'add_column'; column: DesignColumn; after: string | null }
+  | { kind: 'drop_column'; name: string }
+  | { kind: 'rename_column'; from: string; to: string; column: DesignColumn }
+  | { kind: 'alter_column'; from: DesignColumn; to: DesignColumn }
+  | { kind: 'move_column'; column: DesignColumn; after: string | null }
+  | { kind: 'drop_primary_key' | 'add_primary_key'; key: DesignKey }
+  | { kind: 'drop_unique' | 'add_unique'; unique: DesignKey }
+  | { kind: 'drop_check' | 'add_check'; check: DesignCheck }
+  | { kind: 'drop_foreign_key' | 'add_foreign_key'; key: DesignForeignKey }
+  | { kind: 'drop_index' | 'add_index'; index: DesignIndex }
+  | { kind: 'rename_index'; from: string; index: DesignIndex }
+
+export type DesignSection = 'table' | 'columns' | 'primary_key' | 'indexes' | 'foreign_keys' | 'uniques' | 'checks'
+
+export interface DesignIssue {
+  section: DesignSection
+  row: number | null
+  field: string | null
+  message: string
+}
+
+/** 预览中的一条：SQL，或由程序执行的核对行数。`changes` 是对应的变更序号。 */
+export type DesignStep =
+  | { kind: 'sql'; sql: string; changes: number[] }
+  | { kind: 'count_check'; source: string; target: string; changes: number[] }
+
+export interface DesignPreview {
+  errors: DesignIssue[]
+  changes: TableChange[]
+  statements: DesignStep[]
+  rebuild: { reasons: string[]; active: boolean; refused: string | null }
+  warnings: string[]
+  transactional: boolean
+  denied: string | null
+  confirm: string[]
+  conflict: boolean
 }
 
 export type FetchFn = (input: string, init?: RequestInit) => Promise<Response>
@@ -583,6 +726,12 @@ export function createApi(token: string, fetchFn: FetchFn = (i, o) => fetch(i, o
     /** 前 100 条失败中第 n 条的完整语句。 */
     taskStatement: async (id: string, n: number) =>
       (await request<{ sql: string }>('GET', `/tasks/${encodeURIComponent(id)}/statements/${n}`)).sql,
+    designCaps: (id: string) => request<DesignCaps>('GET', `${p(id)}/design/caps`),
+    designTable: (id: string, schema: string, name: string) =>
+      request<DesignTableResult>('GET', `${p(id)}/design/table${q({ schema, name })}`),
+    previewDesign: (id: string, req: DesignRequest) => request<DesignPreview>('POST', `${p(id)}/design/preview`, req),
+    applyDesign: async (id: string, req: DesignRequest) =>
+      (await request<{ id: string }>('POST', `${p(id)}/design/apply`, req)).id,
     openSession: (profileId: string) => request<SessionInfo>('POST', '/sessions', { profile_id: profileId }),
     session: (id: number) => request<SessionInfo>('GET', `/sessions/${id}`),
     /** 关闭会话；有未提交的修改时按 commit 提交或回滚。 */
```

- [ ] **Step 5: 检查**

在 PowerShell 中执行（bash 里 `cd web` 会触发坏掉的 fnm 钩子，`&&` 后面的命令会被跳过）：

```powershell
$env:PATH = "D:\Environments\nvm4w\nodejs;$env:PATH"; Set-Location web; pnpm typecheck; pnpm test; pnpm build
```

Expected: typecheck、测试（新增 8 个）、打包通过。

- [ ] **Step 6: 提交**

```bash
git add web/src/features/design/designStore.ts web/src/features/design/logic.test.ts web/src/features/design/logic.ts web/src/shared/api.ts
git commit -m "feat(web): 表设计器的接口客户端、草稿逻辑与状态" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 10: 前端：设计器界面、保存确认与入口

- 标签：`tabs.ts` 加 `design: { schema, table | null }`，`openDesign`（同一张表只开一个，新建表每次一个，标题"新表"）、`setDesign`（新表保存后转为编辑这张表）；`Workspace` 懒加载 `DesignView`（带 Monaco），标签图标 `IconTableOptions`；关闭有未保存改动的设计器标签时确认放弃；`insertSql` 在设计器标签中新开查询标签。
- `features/design/`：
  - `DesignView.tsx`：工具栏（撤销全部、重新加载、保存）、子标签（列、索引、外键、约束、选项、SQL 预览；子标签上显示错误数）、改动后约 300 毫秒自动预览、Ctrl+S 保存、执行进度（可取消）和失败时的提示（失败的语句、在新查询标签中打开没有执行的语句）；执行结束刷新对象树和数据标签的列信息，成功时重新读取这张表。
  - `ColumnsPane.tsx`（类型 AutoComplete、长度和小数位按类型启用、Oracle/达梦的长度单位、非空、主键、自增、默认值、注释；选中行的生成表达式和原生类型；新增（不能指定列位置的库加在最后）、删除、上移、下移（Alt+↑/↓）、还原此行）、`ListPane.tsx`（索引、外键、约束共用的列表）、`IndexesPane.tsx`（列用标签，列名后加 DESC 表示降序）、`ForeignKeysPane.tsx`（引用的 schema、表、列从元数据选，不支持的动作禁用）、`ConstraintsPane.tsx`、`OptionsPane.tsx`（新建表时选 schema、表名、表注释、主键名和主键列顺序、分区、行数估计、存储属性）、`SqlBox.tsx`（只读 Monaco）。
  - `SaveDialog.tsx`：生产环境横幅、原表已被修改（重新加载）、错误、被拒绝、不能重建、重建开关（确定不支持时强制打开）、提示、事务说明、变更摘要、SQL；复制 SQL、在新查询标签中打开、执行（生产环境或重建时红色）。
  - `summary.ts`：变更摘要（修改列时每个方面一行）、脚本文本（PL/SQL 块后加 `/`，核对行数写成注释）。
- 入口：对象树表的右键"设计表"、schema 的右键"新建表"；数据标签工具栏"设计表"；标题栏"文件 → 新建表"（schema 用当前数据标签或设计器标签的，否则在"选项"中选）。
- 词条：新文件 `design.json`（中、英，英文区分单复数）；`tree.menu.newTable` / `design`、`editor.data.design`、`app.menu.file.newTable`。样式加在 `shell.css`。

**Files:**
- Modify: `web/src/app/TitleBar.tsx`
- Modify: `web/src/app/menus.test.ts`
- Modify: `web/src/app/menus.ts`
- Modify: `web/src/app/shell.css`
- Modify: `web/src/features/data/DataView.tsx`
- Create: `web/src/features/design/ColumnsPane.tsx`
- Create: `web/src/features/design/ConstraintsPane.tsx`
- Create: `web/src/features/design/DesignView.tsx`
- Create: `web/src/features/design/ForeignKeysPane.tsx`
- Create: `web/src/features/design/IndexesPane.tsx`
- Create: `web/src/features/design/ListPane.tsx`
- Create: `web/src/features/design/OptionsPane.tsx`
- Create: `web/src/features/design/SaveDialog.tsx`
- Create: `web/src/features/design/SqlBox.tsx`
- Modify: `web/src/features/design/logic.ts`
- Create: `web/src/features/design/summary.test.ts`
- Create: `web/src/features/design/summary.ts`
- Modify: `web/src/features/editor/Workspace.tsx`
- Modify: `web/src/features/editor/actions.tsx`
- Modify: `web/src/features/editor/tabs.test.ts`
- Modify: `web/src/features/editor/tabs.ts`
- Modify: `web/src/features/tree/ObjectTree.tsx`
- Modify: `web/src/locales/en/app.json`
- Create: `web/src/locales/en/design.json`
- Modify: `web/src/locales/en/editor.json`
- Modify: `web/src/locales/en/tree.json`
- Modify: `web/src/locales/index.ts`
- Modify: `web/src/locales/zh-CN/app.json`
- Create: `web/src/locales/zh-CN/design.json`
- Modify: `web/src/locales/zh-CN/editor.json`
- Modify: `web/src/locales/zh-CN/tree.json`

**Interfaces:**
- Consumes：任务 9 的逻辑和状态。
- Produces：`useTabs().openDesign(profileId, target)`、`setDesign(id, target)`、`DesignTarget`；`features/design/summary.ts` 的 `describeChange`、`scriptOf`。

- [ ] **Step 1: 修改 `web/src/app/menus.test.ts`**

```diff
--- a/web/src/app/menus.test.ts
+++ b/web/src/app/menus.test.ts
@@ -15,6 +15,7 @@ describe('标题栏菜单', () => {
       'new-query',
       'close-tab',
       '-',
+      'new-table',
       'import',
       'sql-file',
       '-',
@@ -29,7 +30,7 @@ describe('标题栏菜单', () => {
   it('浏览器模式没有窗口菜单和退出，帮助只有关于', () => {
     const m = buildMenus({ ...base, desktop: false })
     expect(m.map((g) => g.label)).toEqual(['文件', '查看', 'SQL', '帮助'])
-    expect(keys(m[0].items)).toEqual(['new-connection', 'new-query', 'close-tab', '-', 'import', 'sql-file', '-', 'settings'])
+    expect(keys(m[0].items)).toEqual(['new-connection', 'new-query', 'close-tab', '-', 'new-table', 'import', 'sql-file', '-', 'settings'])
     expect(keys(m[3].items)).toEqual(['help:about'])
   })
 
```

- [ ] **Step 2: 新建 `web/src/features/design/summary.test.ts`**

```ts
import { describe, expect, it } from 'vitest'

import type { DesignColumn } from '../../shared/api'
import i18n from '../../shared/i18n'
import { describeChange, scriptOf } from './summary'

const col = (name: string, native: string, extra: Partial<DesignColumn> = {}): DesignColumn => ({
  name,
  ordinal: 1,
  data_type: { native, logical: { kind: 'other' } },
  nullable: true,
  default: null,
  comment: null,
  auto_increment: false,
  generated: null,
  ...extra,
})

describe('变更摘要', () => {
  it('修改列：每个改了的方面一行', async () => {
    await i18n.changeLanguage('zh-CN')
    const lines = describeChange({
      kind: 'alter_column',
      from: col('price', 'NUMBER(10,2)'),
      to: col('price', 'NUMBER(12,2)', { nullable: false, default: '0', comment: '价格' }),
    })
    expect(lines).toEqual([
      '修改列 price：类型 NUMBER(10,2) → NUMBER(12,2)',
      '修改列 price：不允许为空',
      '修改列 price：默认值改为 0',
      '修改列 price 的注释',
    ])
    expect(describeChange({ kind: 'rename_column', from: 'a', to: 'b', column: col('b', 'int') })).toEqual(['列 a 改名为 b'])
    expect(describeChange({ kind: 'add_unique', unique: { name: null, columns: ['a', 'b'] } })).toEqual(['新增唯一约束 a, b'])
  })

  it('英文区分单复数', async () => {
    await i18n.changeLanguage('en')
    const create = (n: number) =>
      describeChange({
        kind: 'create_table',
        table: {
          name: 't',
          comment: null,
          columns: Array.from({ length: n }, (_, i) => col(`c${i}`, 'int')),
          primary_key: null,
          uniques: [],
          foreign_keys: [],
          checks: [],
          indexes: [],
          triggers: [],
          partition: null,
        },
      })
    expect(create(1)).toEqual(['Create table t (1 column)'])
    expect(create(3)).toEqual(['Create table t (3 columns)'])
    await i18n.changeLanguage('zh-CN')
  })

  it('脚本：语句以分号结束，PL/SQL 块后加斜杠，核对行数写成注释', () => {
    const steps = [
      { kind: 'sql' as const, sql: 'CREATE TABLE t (a INT)', changes: [0] },
      { kind: 'count_check' as const, source: 'old', target: 'new', changes: [] },
      { kind: 'sql' as const, sql: 'CREATE TRIGGER x BEGIN NULL; END;', changes: [] },
    ]
    const text = scriptOf(steps, true)
    expect(text.split('\n\n')).toEqual([
      'CREATE TABLE t (a INT);',
      expect.stringMatching(/^-- 核对行数：old 与 new/),
      'CREATE TRIGGER x BEGIN NULL; END;\n/',
    ])
    expect(scriptOf(steps.slice(2), false)).toBe('CREATE TRIGGER x BEGIN NULL; END;')
  })
})
```

- [ ] **Step 3: 修改 `web/src/features/editor/tabs.test.ts`**

```diff
--- a/web/src/features/editor/tabs.test.ts
+++ b/web/src/features/editor/tabs.test.ts
@@ -109,6 +109,24 @@ describe('数据标签', () => {
 })
 
 describe('打开后执行', () => {
+  it('设计器标签：同一张表只开一个，新建表每次一个；保存新表后转为编辑这张表', () => {
+    useTabs.getState().restore(undefined)
+    const { openDesign, setDesign } = useTabs.getState()
+    const a = openDesign('p', { schema: 'public', table: 'orders' })
+    expect(openDesign('p', { schema: 'public', table: 'orders' })).toBe(a)
+    const n1 = openDesign('p', { schema: 'public', table: null })
+    const n2 = openDesign('p', { schema: 'public', table: null })
+    expect(n1).not.toBe(n2)
+    const tabs = useTabs.getState().tabs
+    expect(tabs.map((x) => x.title)).toEqual(['orders', '新表', '新表'])
+    setDesign(n1, { schema: 'public', table: 'items' })
+    expect(useTabs.getState().tabs[1]).toMatchObject({ title: 'items', design: { schema: 'public', table: 'items' } })
+    // 保存再恢复
+    const raw = JSON.stringify({ tabs: useTabs.getState().tabs, active: a, seq: 1 })
+    expect(parseTabs(raw).tabs).toHaveLength(3)
+    expect(parseTabs(JSON.stringify({ tabs: [{ id: 'x', title: 't', profileId: 'p', sql: '', design: { schema: 1 } }] })).tabs).toEqual([])
+  })
+
   it('标记只取走一次，不随标签保存', () => {
     const id = useTabs.getState().open('p', 'SELECT 1')
     markRun(id)
```

- [ ] **Step 4: 修改 `web/src/app/TitleBar.tsx`**

```diff
--- a/web/src/app/TitleBar.tsx
+++ b/web/src/app/TitleBar.tsx
@@ -69,6 +69,13 @@ export default function TitleBar() {
     if (key === 'new-connection') ui.openConnection(null)
     else if (key === 'new-query') newQuery()
     else if (key === 'close-tab') closeActive()
+    else if (key === 'new-table') {
+      // 连接同"导入"；schema 用当前数据标签或设计器标签的，没有时在设计器的"选项"中选
+      const id = ui.selectedProfile ?? tab?.profileId ?? profiles?.[0]?.id
+      const schema = (tab?.profileId === id && (tab?.data?.schema ?? tab?.design?.schema)) || ''
+      if (!id) ui.openConnection(null)
+      else useTabs.getState().openDesign(id, { schema, table: null })
+    }
     else if (key === 'import' || key === 'sql-file') {
       // 用对象树中选中的连接，没有时用当前标签的、第一个连接；一个都没有时新建连接
       const id = ui.selectedProfile ?? tab?.profileId ?? profiles?.[0]?.id
```

- [ ] **Step 5: 修改 `web/src/app/menus.ts`**

```diff
--- a/web/src/app/menus.ts
+++ b/web/src/app/menus.ts
@@ -28,6 +28,7 @@ export function buildMenus(s: MenuState): MenuGroup[] {
     { key: 'new-query', label: t('app.menu.file.newQuery'), shortcut: 'Ctrl+T' },
     { key: 'close-tab', label: t('app.menu.file.closeTab'), shortcut: 'Ctrl+W' },
     'divider',
+    { key: 'new-table', label: t('app.menu.file.newTable') },
     { key: 'import', label: t('app.menu.file.import') },
     { key: 'sql-file', label: t('app.menu.file.sqlFile') },
     'divider',
```

- [ ] **Step 6: 修改 `web/src/app/shell.css`**

```diff
--- a/web/src/app/shell.css
+++ b/web/src/app/shell.css
@@ -607,3 +607,114 @@ body {
   border-radius: 4px;
   white-space: pre-wrap;
 }
+
+/* 表设计器（M3） */
+.design-view {
+  display: flex;
+  flex-direction: column;
+  min-height: 0;
+  height: 100%;
+  outline: none;
+}
+.design-tabs {
+  min-height: 0;
+  padding: 0 10px;
+}
+.design-tabs .ant-tabs-content,
+.design-tabs .ant-tabs-tabpane {
+  height: 100%;
+}
+.design-pane,
+.design-list {
+  display: flex;
+  flex-direction: column;
+  gap: 6px;
+  outline: none;
+}
+.design-pane h4 {
+  margin: 8px 0 0;
+}
+.design-table .ant-table-cell {
+  padding: 2px 6px !important;
+}
+.design-table tr.design-new > td {
+  background: var(--ok-bg);
+}
+.design-table tr.design-changed > td {
+  background: var(--warn-bg);
+}
+.design-table tr.design-deleted > td {
+  text-decoration: line-through;
+  color: var(--muted);
+}
+.design-table tr.design-selected > td {
+  box-shadow: inset 0 0 0 9999px var(--sel);
+}
+.design-cell-error {
+  border-radius: 3px;
+  box-shadow: inset 0 0 0 1px var(--danger);
+  background: var(--danger-bg);
+}
+.design-changed-text {
+  color: var(--warn);
+}
+.design-props {
+  display: flex;
+  gap: 16px;
+  align-items: center;
+  padding: 6px 2px;
+  border-top: 1px solid var(--border);
+}
+.design-props label,
+.design-options label {
+  display: flex;
+  gap: 8px;
+  align-items: center;
+}
+.design-props label .ant-input {
+  width: 320px;
+}
+.design-options {
+  max-width: 640px;
+  gap: 10px;
+}
+.design-options label > :first-child {
+  flex: none;
+}
+.design-options label {
+  display: grid;
+  grid-template-columns: 140px 1fr;
+}
+.design-pk .row {
+  gap: 4px;
+}
+.design-sql {
+  gap: 8px;
+}
+.design-run {
+  display: flex;
+  gap: 12px;
+  align-items: center;
+  padding: 6px 10px;
+  border-top: 1px solid var(--border);
+}
+.design-failed-sql {
+  display: block;
+  margin-top: 4px;
+  white-space: pre-wrap;
+  font-family: var(--mono);
+}
+.design-save {
+  display: flex;
+  flex-direction: column;
+  gap: 8px;
+}
+.design-save h4 {
+  margin: 4px 0 0;
+}
+.design-changes {
+  margin: 0;
+  padding-left: 20px;
+  max-height: 160px;
+  overflow: auto;
+}
```

- [ ] **Step 7: 修改 `web/src/features/data/DataView.tsx`**

```diff
--- a/web/src/features/data/DataView.tsx
+++ b/web/src/features/data/DataView.tsx
@@ -1,5 +1,5 @@
 // 数据标签（M1c2 设计文档第 5 节）：浏览一张表 / 视图 / 物化视图。
-// 工具栏：连接和 schema（只显示）、WHERE 过滤、刷新、停止、统计行数、导出；表格沿用查询标签的结果面板，点列头排序。
+// 工具栏：连接和 schema（只显示）、WHERE 过滤、刷新、停止、统计行数、导出、导入、设计表；表格沿用查询标签的结果面板，点列头排序。
 // SELECT 由后端按方言生成（data-sql），经这个标签自己的会话执行（沿用 runner：续取、取消、行数上限）。
 // 表的数据可以编辑（M2a 设计文档第 6 节）：改动暂存在 editStore，提交前预览，经同一个会话在事务里执行。
 
@@ -12,6 +12,7 @@ import {
   IconRowInsertBottom,
   IconRowRemove,
   IconSum,
+  IconTableOptions,
   IconTrashX,
   IconUpload,
 } from '@tabler/icons-react'
@@ -246,6 +247,16 @@ export default function DataView({ tab, profiles }: { tab: QueryTab; profiles: P
             {t('editor.data.import')}
           </Button>
         )}
+        {isTable && (
+          <Button
+            size="small"
+            icon={<IconTableOptions size={14} />}
+            disabled={!profile}
+            onClick={() => useTabs.getState().openDesign(tab.profileId, { schema: data.schema, table: data.object })}
+          >
+            {t('editor.data.design')}
+          </Button>
+        )}
         {total !== null && <span className="muted">{t('editor.data.total', { count: Number(total) })}</span>}
       </div>
       {/* 表显示编辑按钮；视图和没有行标识的表显示只读原因 */}
```

- [ ] **Step 8: 新建 `web/src/features/design/ColumnsPane.tsx`**

```tsx
// "列"子标签（M3 设计文档 7.3 节）：行内编辑名称、类型（按方言的下拉，可自由输入）、长度、小数位、非空、主键、自增、默认值、注释。
// 选中一行时下方显示生成列表达式和原生类型全文。新增、删除、上移、下移（Alt+↑/↓）、还原此行。

import { IconArrowBackUp, IconArrowDown, IconArrowUp, IconPlus, IconTrash } from '@tabler/icons-react'
import { AutoComplete, Button, Checkbox, Input, Select, Table, Tooltip } from 'antd'
import type { KeyboardEvent } from 'react'
import { useTranslation } from 'react-i18next'

import type { DesignCaps } from '../../shared/api'
import {
  type Draft,
  type DraftColumn,
  type Issues,
  addColumn,
  changedFields,
  columnState,
  emptyColumn,
  findType,
  joinType,
  moveColumn,
  removeColumn,
  restoreColumn,
  splitType,
  togglePrimaryKey,
  typeParams,
  updateColumn,
} from './logic'

interface Props {
  draft: Draft
  original: Draft | null
  caps: DesignCaps | undefined
  issues: Issues
  selected: string | null
  onSelect(key: string | null): void
  onChange(fn: (d: Draft) => Draft): void
}

/** 带错误提示的单元格：出错时标红，悬停显示原因。 */
export function Cell({ error, children }: { error?: string; children: React.ReactNode }) {
  return error ? (
    <Tooltip title={error} color="red">
      <div className="design-cell-error">{children}</div>
    </Tooltip>
  ) : (
    <>{children}</>
  )
}

export default function ColumnsPane({ draft, original, caps, issues, selected, onSelect, onChange }: Props) {
  const { t } = useTranslation()
  const types = caps?.types ?? []
  const pk = draft.primaryKey?.columns ?? []
  const sel = draft.columns.find((c) => c.key === selected)
  const set = (key: string, patch: Partial<DraftColumn>) => onChange((d) => updateColumn(d, key, patch))

  const add = () => {
    const c = emptyColumn()
    // 不能指定列位置的库：新列加在最后（插在中间要重建表）
    onChange((d) => addColumn(d, c, caps?.reorder ? (selected ?? undefined) : undefined))
    onSelect(c.key)
  }
  const move = (delta: -1 | 1) => sel && onChange((d) => moveColumn(d, sel.key, delta))
  const onKeyDown = (e: KeyboardEvent) => {
    if (e.altKey && (e.key === 'ArrowUp' || e.key === 'ArrowDown')) {
      e.preventDefault()
      move(e.key === 'ArrowUp' ? -1 : 1)
    }
  }

  const typeCell = (c: DraftColumn) => {
    const parts = splitType(c.data_type.native)
    const entry = findType(parts.name, types)
    const params = typeParams(parts.name, types)
    const native = (p: Partial<typeof parts>) => set(c.key, { data_type: { ...c.data_type, native: joinType({ ...parts, ...p }) } })
    return { parts, entry, params, native }
  }

  const columns = [
    {
      title: t('design.column.name'),
      width: 180,
      render: (_: unknown, c: DraftColumn) => (
        <Cell error={issues.at(c.key, 'name')}>
          <Input size="small" variant="borderless" value={c.name} disabled={c.deleted} onChange={(e) => set(c.key, { name: e.target.value })} />
        </Cell>
      ),
    },
    {
      title: t('design.column.type'),
      width: 190,
      render: (_: unknown, c: DraftColumn) => {
        const { parts, native } = typeCell(c)
        return (
          <Cell error={issues.at(c.key, 'type')}>
            <AutoComplete
              size="small"
              variant="borderless"
              style={{ width: '100%' }}
              value={parts.name}
              disabled={c.deleted}
              options={types.map((x) => ({ value: x.name }))}
              filterOption={(input, o) => String(o?.value ?? '').toLowerCase().includes(input.toLowerCase())}
              onChange={(v: string) => native({ name: v })}
            />
          </Cell>
        )
      },
    },
    {
      title: t('design.column.length'),
      width: 120,
      render: (_: unknown, c: DraftColumn) => {
        const { parts, entry, params, native } = typeCell(c)
        return (
          <div className="row" style={{ gap: 4 }}>
            <Input
              size="small"
              variant="borderless"
              value={parts.length}
              disabled={c.deleted || params === 'none'}
              onChange={(e) => native({ length: e.target.value.replace(/[^\d]/g, '') })}
            />
            {entry?.char_unit && (
              <Select
                size="small"
                variant="borderless"
                style={{ width: 70 }}
                value={parts.unit || ''}
                disabled={c.deleted}
                options={[
                  { value: '', label: '-' },
                  { value: 'BYTE', label: 'BYTE' },
                  { value: 'CHAR', label: 'CHAR' },
                ]}
                onChange={(unit: '' | 'BYTE' | 'CHAR') => native({ unit })}
              />
            )}
          </div>
        )
      },
    },
    {
      title: t('design.column.scale'),
      width: 72,
      render: (_: unknown, c: DraftColumn) => {
        const { parts, params, native } = typeCell(c)
        return (
          <Input
            size="small"
            variant="borderless"
            value={parts.scale}
            disabled={c.deleted || params !== 'precision_scale'}
            onChange={(e) => native({ scale: e.target.value.replace(/[^\d-]/g, '') })}
          />
        )
      },
    },
    {
      title: t('design.column.notNull'),
      width: 56,
      align: 'center' as const,
      render: (_: unknown, c: DraftColumn) => (
        <Checkbox checked={!c.nullable} disabled={c.deleted || pk.includes(c.name)} onChange={(e) => set(c.key, { nullable: !e.target.checked })} />
      ),
    },
    {
      title: t('design.column.primaryKey'),
      width: 56,
      align: 'center' as const,
      render: (_: unknown, c: DraftColumn) => (
        <Checkbox checked={pk.includes(c.name)} disabled={c.deleted || !c.name} onChange={() => onChange((d) => togglePrimaryKey(d, c.name))} />
      ),
    },
    {
      title: t('design.column.autoIncrement'),
      width: 56,
      align: 'center' as const,
      render: (_: unknown, c: DraftColumn) => (
        <Checkbox checked={c.auto_increment} disabled={c.deleted} onChange={(e) => set(c.key, { auto_increment: e.target.checked })} />
      ),
    },
    {
      title: (
        <Tooltip title={t('design.column.defaultHint')}>
          <span>{t('design.column.default')}</span>
        </Tooltip>
      ),
      width: 150,
      render: (_: unknown, c: DraftColumn) => (
        <Input
          size="small"
          variant="borderless"
          value={c.default ?? ''}
          disabled={c.deleted || c.auto_increment || !!c.generated}
          onChange={(e) => set(c.key, { default: e.target.value || null })}
        />
      ),
    },
    {
      title: t('design.column.comment'),
      render: (_: unknown, c: DraftColumn) => (
        <Input size="small" variant="borderless" value={c.comment ?? ''} disabled={c.deleted} onChange={(e) => set(c.key, { comment: e.target.value || null })} />
      ),
    },
  ]

  const state = sel ? columnState(sel, original) : null
  return (
    <div className="design-pane" tabIndex={-1} onKeyDown={onKeyDown}>
      <div className="row toolbar">
        <Button size="small" type="text" icon={<IconPlus size={16} />} onClick={add}>
          {t('design.column.add')}
        </Button>
        <Button size="small" type="text" icon={<IconTrash size={16} />} disabled={!sel || sel.deleted} onClick={() => sel && onChange((d) => removeColumn(d, sel.key))}>
          {t('design.remove')}
        </Button>
        <Tooltip title="Alt+↑">
          <Button size="small" type="text" icon={<IconArrowUp size={16} />} disabled={!sel} onClick={() => move(-1)}>
            {t('design.column.moveUp')}
          </Button>
        </Tooltip>
        <Tooltip title="Alt+↓">
          <Button size="small" type="text" icon={<IconArrowDown size={16} />} disabled={!sel} onClick={() => move(1)}>
            {t('design.column.moveDown')}
          </Button>
        </Tooltip>
        <Button
          size="small"
          type="text"
          icon={<IconArrowBackUp size={16} />}
          disabled={!sel || !original || (state !== 'changed' && state !== 'deleted')}
          onClick={() => sel && original && onChange((d) => restoreColumn(d, sel.key, original))}
        >
          {t('design.restore')}
        </Button>
        {caps && !caps.reorder && original && <span className="muted">{t('design.column.reorderNote')}</span>}
      </div>
      <Table<DraftColumn>
        size="small"
        rowKey="key"
        className="design-table"
        pagination={false}
        dataSource={draft.columns}
        columns={columns}
        locale={{ emptyText: t('design.column.empty') }}
        scroll={{ y: 'calc(100vh - 380px)' }}
        rowClassName={(c) => {
          const s = columnState(c, original)
          return `${s ? `design-${s}` : ''}${c.key === selected ? ' design-selected' : ''}`
        }}
        onRow={(c) => ({ onClick: () => onSelect(c.key) })}
      />
      <div className="design-props">
        {sel ? (
          <>
            <label>
              {t('design.column.generated')}
              <Input
                size="small"
                value={sel.generated ?? ''}
                disabled={sel.deleted}
                placeholder={t('design.column.generatedHint')}
                onChange={(e) => set(sel.key, { generated: e.target.value || null })}
              />
            </label>
            <label>
              {t('design.column.native')}
              <code className={changedFields(sel, original).has('type') ? 'design-changed-text' : ''}>{sel.data_type.native || '-'}</code>
            </label>
          </>
        ) : (
          <span className="muted">{t('design.column.noSelection')}</span>
        )}
      </div>
    </div>
  )
}
```

- [ ] **Step 9: 新建 `web/src/features/design/ConstraintsPane.tsx`**

```tsx
// "约束"子标签：唯一约束和检查约束两组，各自增、删、改。

import { Input, Select } from 'antd'
import { useTranslation } from 'react-i18next'

import type { DesignCaps } from '../../shared/api'
import { Cell } from './ColumnsPane'
import ListPane from './ListPane'
import { type Draft, type DraftCheck, type DraftKey, type Issues, newKey, updateItem } from './logic'

export default function ConstraintsPane({ draft, original, caps, issues, onChange }: {
  draft: Draft
  original: Draft | null
  caps: DesignCaps | undefined
  issues: Issues
  onChange(fn: (d: Draft) => Draft): void
}) {
  const { t } = useTranslation()
  const names = draft.columns.filter((c) => !c.deleted && c.name).map((c) => ({ value: c.name }))
  const setU = (key: string, patch: Partial<DraftKey>) => onChange((d) => updateItem(d, 'uniques', key, patch))
  const setC = (key: string, patch: Partial<DraftCheck>) => onChange((d) => updateItem(d, 'checks', key, patch))
  const nameCell = (key: string, name: string | null, deleted: boolean | undefined, set: (n: string | null) => void) => (
    <Cell error={issues.at(key, 'name')}>
      <Input size="small" variant="borderless" value={name ?? ''} disabled={deleted} onChange={(e) => set(e.target.value || null)} />
    </Cell>
  )
  return (
    <div className="design-pane">
      <h4>{t('design.constraint.uniques')}</h4>
      <ListPane
        list="uniques"
        draft={draft}
        original={original}
        onChange={onChange}
        addLabel={t('design.constraint.addUnique')}
        emptyText={t('design.constraint.noUniques')}
        create={() => ({ key: newKey(), origin: null, name: `uk_${draft.name || 't'}_${draft.uniques.length + 1}`, columns: [] })}
        columns={[
          { title: t('design.constraint.name'), width: 220, render: (_, u) => nameCell(u.key, u.name, u.deleted, (name) => setU(u.key, { name })) },
          {
            title: t('design.constraint.columns'),
            render: (_, u) => (
              <Cell error={issues.at(u.key, 'columns')}>
                <Select mode="multiple" size="small" variant="borderless" style={{ width: '100%' }} value={u.columns} options={names} disabled={u.deleted} onChange={(columns: string[]) => setU(u.key, { columns })} />
              </Cell>
            ),
          },
        ]}
      />
      <h4>{t('design.constraint.checks')}</h4>
      <ListPane
        list="checks"
        draft={draft}
        original={original}
        onChange={onChange}
        addLabel={t('design.constraint.addCheck')}
        emptyText={t('design.constraint.noChecks')}
        note={caps && !caps.checks ? <span className="muted">{t('design.constraint.checksIgnored')}</span> : undefined}
        create={() => ({ key: newKey(), origin: null, name: `ck_${draft.name || 't'}_${draft.checks.length + 1}`, expression: '' })}
        columns={[
          { title: t('design.constraint.name'), width: 220, render: (_, c) => nameCell(c.key, c.name, c.deleted, (name) => setC(c.key, { name })) },
          {
            title: t('design.constraint.expression'),
            render: (_, c) => (
              <Cell error={issues.at(c.key, 'expression')}>
                <Input size="small" variant="borderless" value={c.expression} disabled={c.deleted} onChange={(e) => setC(c.key, { expression: e.target.value })} />
              </Cell>
            ),
          },
        ]}
      />
    </div>
  )
}
```

- [ ] **Step 10: 新建 `web/src/features/design/DesignView.tsx`**

```tsx
// 表设计器标签（M3 设计文档第 7 节）：子标签分列、索引、外键、约束、选项、SQL 预览；改动后约 300 毫秒自动预览，
// 校验错误在子标签上显示数目、出错的单元格标红。Ctrl+S 或"保存"打开确认框，执行时进度显示在底部，可以取消。
// 引入 Monaco（SQL 预览），由工作区懒加载。

import { IconDeviceFloppy, IconRefresh, IconTrashX } from '@tabler/icons-react'
import { keepPreviousData, useQuery, useQueryClient } from '@tanstack/react-query'
import { Alert, App, Badge, Button, Progress, Spin, Tabs } from 'antd'
import { type KeyboardEvent, useCallback, useEffect, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { useApi } from '../../app/context'
import { useUi } from '../../app/ui'
import { type DesignRequest, type Profile, type TaskStatus, errorText } from '../../shared/api'
import { withVault } from '../connections/vault'
import { useQueryActions } from '../editor/actions'
import { type QueryTab, useTabs } from '../editor/tabs'
import ColumnsPane from './ColumnsPane'
import ConstraintsPane from './ConstraintsPane'
import { useDesigns } from './designStore'
import ForeignKeysPane from './ForeignKeysPane'
import IndexesPane from './IndexesPane'
import { type Draft, type Pane, emptyDraft, fromTable, isDirty, issuesOf, renamesOf, toModel } from './logic'
import OptionsPane from './OptionsPane'
import SaveDialog from './SaveDialog'
import SqlBox from './SqlBox'
import { scriptOf } from './summary'

const NO_ISSUES = issuesOf([], emptyDraft())

export default function DesignView({ tab, profiles }: { tab: QueryTab; profiles: Profile[] }) {
  const api = useApi()
  const { t } = useTranslation()
  const { message, modal } = App.useApp()
  const qc = useQueryClient()
  const { openSql } = useQueryActions()
  const profile = profiles.find((p) => p.id === tab.profileId)
  const target = tab.design!
  const isNew = target.table === null
  const state = useDesigns((s) => s.tabs[tab.id])
  const [schema, setSchema] = useState(target.schema)
  const [pane, setPane] = useState<Pane | 'sql'>('columns')
  const [selected, setSelected] = useState<string | null>(null)
  const [saving, setSaving] = useState(false)
  const [task, setTask] = useState<TaskStatus | null>(null)
  const [loadError, setLoadError] = useState<string | null>(null)

  const caps = useQuery({
    queryKey: ['design-caps', tab.profileId],
    queryFn: () => withVault(api, () => api.designCaps(tab.profileId)),
    enabled: !!profile,
    staleTime: Infinity,
  })

  const load = useCallback(async () => {
    setLoadError(null)
    if (target.table === null) {
      useDesigns.getState().set(tab.id, { original: null, fingerprint: null, storage: null, draft: emptyDraft(), rebuild: false })
      return
    }
    try {
      const r = await withVault(api, () => api.designTable(tab.profileId, target.schema, target.table!))
      useDesigns.getState().set(tab.id, {
        original: fromTable(r.table),
        fingerprint: r.fingerprint,
        storage: r.storage,
        draft: fromTable(r.table),
        rebuild: false,
      })
    } catch (e) {
      setLoadError((e as Error).message)
    }
  }, [api, tab.id, tab.profileId, target.schema, target.table])

  // 第一次打开、新建的表保存后转为编辑这张表时读取
  useEffect(() => {
    const s = useDesigns.getState().tabs[tab.id]
    if (!s || (target.table !== null && s.original === null)) void load()
  }, [tab.id, target.table, load])

  const edit = (fn: (d: Draft) => Draft) => useDesigns.getState().edit(tab.id, fn)
  const dirty = !!state && isDirty(state.draft, state.original)

  const request: DesignRequest | null = useMemo(
    () =>
      state && {
        schema: isNew ? schema : target.schema,
        name: target.table,
        draft: toModel(state.draft),
        renames: renamesOf(state.draft),
        rebuild: state.rebuild,
        fingerprint: state.fingerprint,
      },
    [state, schema, isNew, target.schema, target.table],
  )
  // 改动后约 300 毫秒自动预览
  const [debounced, setDebounced] = useState(request)
  useEffect(() => {
    const h = setTimeout(() => setDebounced(request), 300)
    return () => clearTimeout(h)
  }, [request])
  const preview = useQuery({
    queryKey: ['design-preview', tab.id, JSON.stringify(debounced)],
    queryFn: () => api.previewDesign(tab.profileId, debounced!),
    enabled: !!profile && !!debounced && dirty && !!debounced.schema,
    placeholderData: keepPreviousData,
    retry: false,
  })
  const issues = useMemo(
    () => (state && preview.data && dirty ? issuesOf(preview.data.errors, state.draft) : NO_ISSUES),
    [state, preview.data, dirty],
  )

  // 执行中：轮询任务状态
  useEffect(() => {
    if (!task || task.state !== 'running') return
    const h = setTimeout(async () => {
      try {
        setTask(await api.task(task.id))
      } catch (e) {
        message.error((e as Error).message)
        setTask(null)
      }
    }, 400)
    return () => clearTimeout(h)
  }, [task, api, message])
  // 执行结束：刷新对象树和数据标签的列信息（失败时也可能已执行了一部分）；成功时重新读取这张表（新建的表转为编辑这张表）
  useEffect(() => {
    if (!task || task.state === 'running' || !state) return
    const s = isNew ? schema : target.schema
    useUi.getState().refreshTree(tab.profileId, s)
    void qc.invalidateQueries({ queryKey: ['table-info', tab.profileId, s] })
    if (task.state !== 'done') return
    message.success(t('design.run.done'))
    setTask(null)
    if (isNew) {
      useDesigns.getState().clear(tab.id)
      useTabs.getState().setDesign(tab.id, { schema: s, table: state.draft.name.trim() })
    } else void load()
  }, [task?.state])

  const save = () => {
    if (!dirty || !request || task?.state === 'running') return
    if (!request.schema) {
      setPane('options')
      return
    }
    setSaving(true)
  }
  const onKeyDown = (e: KeyboardEvent) => {
    if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 's') {
      e.preventDefault()
      save()
    }
  }
  const confirmThen = (title: string, fn: () => void) =>
    dirty ? modal.confirm({ title, okType: 'danger', onOk: fn }) : fn()

  if (loadError) return <Alert type="error" style={{ margin: 12 }} message={loadError} action={<Button onClick={() => void load()}>{t('design.toolbar.reload')}</Button>} />
  if (!state || !profile) return <Spin style={{ margin: 48 }} />

  const count = (p: Pane) => issues.counts[p] ?? 0
  const label = (p: Pane, text: string) => (
    <Badge count={count(p)} size="small" offset={[8, -2]}>
      {text}
    </Badge>
  )
  const running = task?.state === 'running'
  const p = preview.data
  return (
    <div className="design-view" onKeyDown={onKeyDown} tabIndex={-1}>
      <div className="row toolbar">
        <b>{isNew ? t('design.newTable') : `${target.schema}.${target.table}`}</b>
        <span className="muted">{profile.name}</span>
        <span className="grow" />
        <Button size="small" type="text" icon={<IconTrashX size={16} />} disabled={!dirty || running} onClick={() => confirmThen(t('design.toolbar.discardConfirm'), () => void load())}>
          {t('design.toolbar.discard')}
        </Button>
        {!isNew && (
          <Button size="small" type="text" icon={<IconRefresh size={16} />} disabled={running} onClick={() => confirmThen(t('design.toolbar.reloadConfirm'), () => void load())}>
            {t('design.toolbar.reload')}
          </Button>
        )}
        <Button size="small" type="primary" icon={<IconDeviceFloppy size={16} />} disabled={!dirty || running} onClick={save}>
          {t('design.toolbar.save')}
        </Button>
      </div>
      <Tabs
        className="design-tabs grow"
        size="small"
        activeKey={pane}
        onChange={(k) => setPane(k as Pane | 'sql')}
        items={[
          {
            key: 'columns',
            label: label('columns', t('design.tabs.columns')),
            children: (
              <ColumnsPane draft={state.draft} original={state.original} caps={caps.data} issues={issues} selected={selected} onSelect={setSelected} onChange={edit} />
            ),
          },
          {
            key: 'indexes',
            label: label('indexes', t('design.tabs.indexes')),
            children: <IndexesPane draft={state.draft} original={state.original} caps={caps.data} issues={issues} onChange={edit} />,
          },
          {
            key: 'foreignKeys',
            label: label('foreignKeys', t('design.tabs.foreignKeys')),
            children: (
              <ForeignKeysPane profileId={tab.profileId} schema={request?.schema ?? ''} draft={state.draft} original={state.original} caps={caps.data} issues={issues} onChange={edit} />
            ),
          },
          {
            key: 'constraints',
            label: label('constraints', t('design.tabs.constraints')),
            children: <ConstraintsPane draft={state.draft} original={state.original} caps={caps.data} issues={issues} onChange={edit} />,
          },
          {
            key: 'options',
            label: label('options', t('design.tabs.options')),
            children: (
              <OptionsPane
                profileId={tab.profileId}
                schema={request?.schema ?? ''}
                isNew={isNew}
                draft={state.draft}
                storage={state.storage}
                issues={issues}
                onSchema={setSchema}
                onChange={edit}
              />
            ),
          },
          {
            key: 'sql',
            label: t('design.tabs.sql'),
            children: (
              <div className="design-pane design-sql">
                {!dirty ? (
                  <span className="muted">{t('design.sql.nothing')}</span>
                ) : !p ? (
                  <Spin />
                ) : (
                  <>
                    {p.errors.length > 0 && <Alert type="error" message={t('design.sql.errors', { count: p.errors.length })} />}
                    {p.rebuild.active && (
                      <Alert type="warning" message={t('design.sql.rebuild')} description={p.rebuild.reasons.join(' · ') || undefined} />
                    )}
                    {p.rebuild.refused && <Alert type="error" message={t('design.sql.refused', { reason: p.rebuild.refused })} />}
                    {p.warnings.map((w) => (
                      <Alert key={w} type="warning" showIcon message={w} />
                    ))}
                    {p.statements.length > 0 && (
                      <Alert type="info" showIcon message={t(p.transactional ? 'design.sql.transactional' : 'design.sql.autocommit')} />
                    )}
                    <SqlBox sql={scriptOf(p.statements, profile.driver === 'oracle' || profile.driver === 'dameng')} driver={profile.driver} height="calc(100vh - 360px)" />
                  </>
                )}
              </div>
            ),
          },
        ]}
      />
      {task && (
        <div className="design-run">
          {running ? (
            <>
              <span>{t('design.run.running', { done: task.progress.statements_done, total: task.progress.statements_total })}</span>
              <Progress
                size="small"
                style={{ width: 240, margin: 0 }}
                percent={Math.round((100 * task.progress.statements_done) / Math.max(1, task.progress.statements_total))}
              />
              <Button size="small" onClick={() => void api.cancelTask(task.id).catch((e) => message.error((e as Error).message))}>
                {t('design.run.cancel')}
              </Button>
            </>
          ) : (
            <Alert
              type={task.state === 'cancelled' ? 'warning' : 'error'}
              style={{ flex: 1 }}
              closable
              onClose={() => setTask(null)}
              message={task.state === 'cancelled' ? t('design.run.cancelled') : t('design.run.failed')}
              description={
                <>
                  {task.failures[0] && (
                    <div>
                      {t('design.run.failedAt', { n: task.failures[0].index + 1 })}
                      <code className="design-failed-sql">{task.failures[0].text}</code>
                    </div>
                  )}
                  {task.error && <div className="err">{errorText(task.error)}</div>}
                </>
              }
              action={
                task.remaining ? (
                  <Button size="small" onClick={() => openSql(task.remaining ?? '', tab.profileId)}>
                    {t('design.run.remaining')}
                  </Button>
                ) : undefined
              }
            />
          )}
        </div>
      )}
      {saving && request && (
        <SaveDialog
          profile={profile}
          request={request}
          onRebuild={(on) => useDesigns.getState().setRebuild(tab.id, on)}
          onReload={() => {
            setSaving(false)
            void load()
          }}
          onClose={() => setSaving(false)}
          onStarted={(id) => setTask({ id, state: 'running', progress: { statements_done: 0, statements_total: 0 } } as TaskStatus)}
        />
      )}
    </div>
  )
}
```

- [ ] **Step 11: 新建 `web/src/features/design/ForeignKeysPane.tsx`**

```tsx
// "外键"子标签：名称、列、引用的 schema 和表（从元数据选）、引用列、删除时和更新时的动作（库不支持的禁用）。

import { useQuery } from '@tanstack/react-query'
import { Input, Select } from 'antd'
import { useTranslation } from 'react-i18next'

import { useApi } from '../../app/context'
import type { DesignCaps } from '../../shared/api'
import { Cell } from './ColumnsPane'
import ListPane from './ListPane'
import { type Draft, type DraftForeignKey, type Issues, newKey, updateItem } from './logic'

/** 引用的表的列（选了表之后才查）。 */
function RefColumns({ profileId, schema, fk, onChange }: {
  profileId: string
  schema: string
  fk: DraftForeignKey
  onChange(cols: string[]): void
}) {
  const api = useApi()
  const refSchema = fk.ref_schema || schema
  const { data } = useQuery({
    queryKey: ['table-info', profileId, refSchema, fk.ref_table],
    queryFn: () => api.table(profileId, refSchema, fk.ref_table),
    enabled: !!fk.ref_table,
  })
  return (
    <Select
      mode="multiple"
      size="small"
      variant="borderless"
      style={{ width: '100%' }}
      value={fk.ref_columns}
      disabled={fk.deleted}
      options={(data?.columns ?? []).map((c) => ({ value: c.name }))}
      onChange={onChange}
    />
  )
}

export default function ForeignKeysPane({ profileId, schema, draft, original, caps, issues, onChange }: {
  profileId: string
  schema: string
  draft: Draft
  original: Draft | null
  caps: DesignCaps | undefined
  issues: Issues
  onChange(fn: (d: Draft) => Draft): void
}) {
  const { t } = useTranslation()
  const api = useApi()
  const { data: schemas = [] } = useQuery({ queryKey: ['schemas', profileId], queryFn: () => api.schemas(profileId) })
  const refSchemas = [...new Set(draft.foreignKeys.map((f) => f.ref_schema || schema))]
  const tables = useQuery({
    queryKey: ['design-ref-tables', profileId, refSchemas],
    queryFn: async () =>
      Object.fromEntries(await Promise.all(refSchemas.map(async (s) => [s, (await api.objects(profileId, s, 'tables')).map((o) => o.name)] as const))),
    enabled: !!schema,
  })
  const names = draft.columns.filter((c) => !c.deleted && c.name).map((c) => ({ value: c.name }))
  const set = (key: string, patch: Partial<DraftForeignKey>) => onChange((d) => updateItem(d, 'foreignKeys', key, patch))
  const action = (values: string[]) => values.map((v) => ({ value: v }))
  return (
    <ListPane
      list="foreignKeys"
      draft={draft}
      original={original}
      onChange={onChange}
      addLabel={t('design.fk.add')}
      emptyText={t('design.fk.empty')}
      height="calc(100vh - 300px)"
      create={() => ({
        key: newKey(),
        origin: null,
        name: `fk_${draft.name || 't'}_${draft.foreignKeys.length + 1}`,
        columns: [],
        ref_schema: null,
        ref_table: '',
        ref_columns: [],
        on_delete: null,
        on_update: null,
      })}
      columns={[
        {
          title: t('design.fk.name'),
          width: 160,
          render: (_, f) => (
            <Cell error={issues.at(f.key, 'name')}>
              <Input size="small" variant="borderless" value={f.name ?? ''} disabled={f.deleted} onChange={(e) => set(f.key, { name: e.target.value || null })} />
            </Cell>
          ),
        },
        {
          title: t('design.fk.columns'),
          width: 170,
          render: (_, f) => (
            <Cell error={issues.at(f.key, 'columns')}>
              <Select mode="multiple" size="small" variant="borderless" style={{ width: '100%' }} value={f.columns} options={names} disabled={f.deleted} onChange={(columns: string[]) => set(f.key, { columns })} />
            </Cell>
          ),
        },
        {
          title: t('design.fk.refSchema'),
          width: 130,
          render: (_, f) => (
            <Select
              size="small"
              variant="borderless"
              style={{ width: '100%' }}
              value={f.ref_schema || schema}
              disabled={f.deleted}
              showSearch
              options={schemas.map((s) => ({ value: s.name }))}
              onChange={(v: string) => set(f.key, { ref_schema: v === schema ? null : v, ref_table: '', ref_columns: [] })}
            />
          ),
        },
        {
          title: t('design.fk.refTable'),
          width: 160,
          render: (_, f) => (
            <Cell error={issues.at(f.key, 'ref_table')}>
              <Select
                size="small"
                variant="borderless"
                style={{ width: '100%' }}
                value={f.ref_table || undefined}
                disabled={f.deleted}
                showSearch
                options={(tables.data?.[f.ref_schema || schema] ?? []).map((n) => ({ value: n }))}
                onChange={(v: string) => set(f.key, { ref_table: v, ref_columns: [] })}
              />
            </Cell>
          ),
        },
        {
          title: t('design.fk.refColumns'),
          width: 170,
          render: (_, f) => (
            <Cell error={issues.at(f.key, 'ref_columns')}>
              <RefColumns profileId={profileId} schema={schema} fk={f} onChange={(ref_columns) => set(f.key, { ref_columns })} />
            </Cell>
          ),
        },
        {
          title: t('design.fk.onDelete'),
          width: 120,
          render: (_, f) => (
            <Select
              size="small"
              variant="borderless"
              style={{ width: '100%' }}
              allowClear
              value={f.on_delete ?? undefined}
              disabled={f.deleted || !caps?.on_delete.length}
              options={action(caps?.on_delete ?? [])}
              onChange={(v?: string) => set(f.key, { on_delete: v ?? null })}
            />
          ),
        },
        {
          title: t('design.fk.onUpdate'),
          width: 120,
          render: (_, f) => (
            <Select
              size="small"
              variant="borderless"
              style={{ width: '100%' }}
              allowClear
              placeholder={caps && !caps.on_update.length ? t('design.fk.unsupported') : undefined}
              value={f.on_update ?? undefined}
              disabled={f.deleted || !caps?.on_update.length}
              options={action(caps?.on_update ?? [])}
              onChange={(v?: string) => set(f.key, { on_update: v ?? null })}
            />
          ),
        },
      ]}
    />
  )
}
```

- [ ] **Step 12: 新建 `web/src/features/design/IndexesPane.tsx`**

```tsx
// "索引"子标签：名称、列（选择列或输入表达式，列名后加 DESC 表示降序）、唯一、方法。主键和唯一约束自带的索引不在这里。

import { Checkbox, Input, Select } from 'antd'
import { useTranslation } from 'react-i18next'

import type { DesignCaps } from '../../shared/api'
import { Cell } from './ColumnsPane'
import ListPane from './ListPane'
import { type Draft, type DraftIndex, type Issues, newKey, updateItem } from './logic'

const DESC = /\s+desc$/i

/** 索引列与标签文字互转：降序的列写成"列名 DESC"。 */
export const indexTags = (x: DraftIndex) => x.columns.map((c) => (c.descending ? `${c.name} DESC` : c.name))
export const fromTags = (tags: string[]) =>
  tags.map((s) => s.trim()).filter(Boolean).map((s) => ({ name: s.replace(DESC, '').trim(), descending: DESC.test(s) }))

export default function IndexesPane({ draft, original, caps, issues, onChange }: {
  draft: Draft
  original: Draft | null
  caps: DesignCaps | undefined
  issues: Issues
  onChange(fn: (d: Draft) => Draft): void
}) {
  const { t } = useTranslation()
  const names = draft.columns.filter((c) => !c.deleted && c.name).map((c) => ({ value: c.name }))
  const set = (key: string, patch: Partial<DraftIndex>) => onChange((d) => updateItem(d, 'indexes', key, patch))
  return (
    <ListPane
      list="indexes"
      draft={draft}
      original={original}
      onChange={onChange}
      addLabel={t('design.index.add')}
      emptyText={t('design.index.empty')}
      height="calc(100vh - 300px)"
      create={() => ({
        key: newKey(),
        origin: null,
        name: `ix_${draft.name || 't'}_${draft.indexes.length + 1}`,
        columns: [],
        unique: false,
        method: caps?.index_methods[0] ?? null,
      })}
      columns={[
        {
          title: t('design.index.name'),
          width: 220,
          render: (_, x) => (
            <Cell error={issues.at(x.key, 'name')}>
              <Input size="small" variant="borderless" value={x.name} disabled={x.deleted} onChange={(e) => set(x.key, { name: e.target.value })} />
            </Cell>
          ),
        },
        {
          title: t('design.index.columns'),
          render: (_, x) => (
            <Cell error={issues.at(x.key, 'columns')}>
              <Select
                mode="tags"
                size="small"
                variant="borderless"
                style={{ width: '100%' }}
                placeholder={t('design.index.columnsHint')}
                value={indexTags(x)}
                options={names}
                disabled={x.deleted}
                onChange={(tags: string[]) => set(x.key, { columns: fromTags(tags) })}
              />
            </Cell>
          ),
        },
        {
          title: t('design.index.unique'),
          width: 64,
          align: 'center',
          render: (_, x) => <Checkbox checked={x.unique} disabled={x.deleted} onChange={(e) => set(x.key, { unique: e.target.checked })} />,
        },
        {
          title: t('design.index.method'),
          width: 140,
          render: (_, x) => (
            <Select
              size="small"
              variant="borderless"
              style={{ width: '100%' }}
              value={x.method ?? undefined}
              allowClear
              disabled={x.deleted}
              options={(caps?.index_methods ?? []).map((m) => ({ value: m }))}
              onChange={(m?: string) => set(x.key, { method: m ?? null })}
            />
          ),
        },
      ]}
    />
  )
}
```

- [ ] **Step 13: 新建 `web/src/features/design/ListPane.tsx`**

```tsx
// 索引、外键、唯一约束、检查约束共用的列表：工具栏（新增、删除、还原）和行内编辑的表格，带改动标记。

import { IconArrowBackUp, IconPlus, IconTrash } from '@tabler/icons-react'
import { Button, Table } from 'antd'
import type { ColumnsType } from 'antd/es/table'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

import { type Draft, type ListName, removeItem, restoreItem, rowState, sameContent } from './logic'

type Item<K extends ListName> = Draft[K][number]

interface Props<K extends ListName> {
  list: K
  draft: Draft
  original: Draft | null
  addLabel: string
  emptyText: string
  /** 新的一行。 */
  create(): Item<K>
  columns: ColumnsType<Item<K>>
  onChange(fn: (d: Draft) => Draft): void
  /** 工具栏右侧的说明。 */
  note?: React.ReactNode
  height?: string
}

export default function ListPane<K extends ListName>(p: Props<K>) {
  const { t } = useTranslation()
  const [selected, setSelected] = useState<string | null>(null)
  const rows = p.draft[p.list] as Item<K>[]
  const originals = p.original?.[p.list] as Item<K>[] | undefined
  const sel = rows.find((r) => r.key === selected)
  const state = (r: Item<K>) => rowState(r, originals, sameContent)
  const selState = sel ? state(sel) : null
  return (
    <div className="design-list">
      <div className="row toolbar">
        <Button
          size="small"
          type="text"
          icon={<IconPlus size={16} />}
          onClick={() => {
            const item = p.create()
            p.onChange((d) => ({ ...d, [p.list]: [...d[p.list], item] }))
            setSelected(item.key)
          }}
        >
          {p.addLabel}
        </Button>
        <Button
          size="small"
          type="text"
          icon={<IconTrash size={16} />}
          disabled={!sel || sel.deleted}
          onClick={() => sel && p.onChange((d) => removeItem(d, p.list, sel.key))}
        >
          {t('design.remove')}
        </Button>
        <Button
          size="small"
          type="text"
          icon={<IconArrowBackUp size={16} />}
          disabled={!sel || !p.original || (selState !== 'changed' && selState !== 'deleted')}
          onClick={() => sel && p.original && p.onChange((d) => restoreItem(d, p.list, sel.key, p.original!))}
        >
          {t('design.restore')}
        </Button>
        {p.note}
      </div>
      <Table<Item<K>>
        size="small"
        rowKey="key"
        className="design-table"
        pagination={false}
        dataSource={rows}
        columns={p.columns}
        locale={{ emptyText: p.emptyText }}
        scroll={p.height ? { y: p.height } : undefined}
        rowClassName={(r) => {
          const s = state(r)
          return `${s ? `design-${s}` : ''}${r.key === selected ? ' design-selected' : ''}`
        }}
        onRow={(r) => ({ onClick: () => setSelected(r.key) })}
      />
    </div>
  )
}
```

- [ ] **Step 14: 新建 `web/src/features/design/OptionsPane.tsx`**

```tsx
// "选项"子标签：schema（新建表时可选）、表名（只有新建时可改）、表注释、主键名和主键列的顺序、分区（只读）、行数估计和存储属性。

import { IconArrowDown, IconArrowUp } from '@tabler/icons-react'
import { useQuery } from '@tanstack/react-query'
import { Button, Input, Select } from 'antd'
import { useTranslation } from 'react-i18next'

import { useApi } from '../../app/context'
import type { DesignTableResult } from '../../shared/api'
import { Cell } from './ColumnsPane'
import { type Draft, type Issues, moveItem } from './logic'

export default function OptionsPane({ profileId, schema, isNew, draft, storage, issues, onSchema, onChange }: {
  profileId: string
  schema: string
  isNew: boolean
  draft: Draft
  storage: DesignTableResult['storage'] | null
  issues: Issues
  onSchema(schema: string): void
  onChange(fn: (d: Draft) => Draft): void
}) {
  const { t } = useTranslation()
  const api = useApi()
  const { data: schemas = [] } = useQuery({ queryKey: ['schemas', profileId], queryFn: () => api.schemas(profileId), enabled: isNew })
  const pk = draft.primaryKey
  const props = [
    storage?.engine && t('design.option.engine', { value: storage.engine }),
    storage?.charset && t('design.option.charset', { value: storage.charset }),
    storage?.collation && t('design.option.collation', { value: storage.collation }),
    storage?.tablespace && t('design.option.tablespace', { value: storage.tablespace }),
  ].filter(Boolean)
  return (
    <div className="design-pane design-options">
      <label>
        {t('design.option.schema')}
        {isNew ? (
          <Cell error={schema ? undefined : t('design.option.schemaRequired')}>
            <Select size="small" showSearch value={schema || undefined} options={schemas.map((s) => ({ value: s.name }))} onChange={onSchema} />
          </Cell>
        ) : (
          <Input size="small" value={schema} disabled />
        )}
      </label>
      <label>
        {t('design.option.name')}
        <Cell error={issues.at('table', 'name')}>
          <Input
            size="small"
            value={draft.name}
            disabled={!isNew}
            title={isNew ? undefined : t('design.option.nameReadonly')}
            onChange={(e) => onChange((d) => ({ ...d, name: e.target.value }))}
          />
        </Cell>
      </label>
      <label>
        {t('design.option.comment')}
        <Input.TextArea rows={2} value={draft.comment ?? ''} onChange={(e) => onChange((d) => ({ ...d, comment: e.target.value || null }))} />
      </label>
      <label>
        {t('design.option.pkName')}
        <Input
          size="small"
          value={pk?.name ?? ''}
          disabled={!pk}
          placeholder={pk ? undefined : t('design.option.noPk')}
          onChange={(e) => onChange((d) => ({ ...d, primaryKey: d.primaryKey && { ...d.primaryKey, name: e.target.value || null } }))}
        />
      </label>
      <label>
        {t('design.option.pkColumns')}
        <Cell error={issues.at('primary_key', 'columns')}>
          <div className="design-pk">
            {pk ? (
              pk.columns.map((c, i) => (
                <div key={c} className="row">
                  <span className="grow">{c}</span>
                  <Button size="small" type="text" icon={<IconArrowUp size={14} />} disabled={i === 0} onClick={() => onChange((d) => ({ ...d, primaryKey: d.primaryKey && { ...d.primaryKey, columns: moveItem(d.primaryKey.columns, i, -1) } }))} />
                  <Button size="small" type="text" icon={<IconArrowDown size={14} />} disabled={i === pk.columns.length - 1} onClick={() => onChange((d) => ({ ...d, primaryKey: d.primaryKey && { ...d.primaryKey, columns: moveItem(d.primaryKey.columns, i, 1) } }))} />
                </div>
              ))
            ) : (
              <span className="muted">{t('design.option.noPk')}</span>
            )}
          </div>
        </Cell>
      </label>
      <label>
        {t('design.option.partition')}
        <span className={draft.partition ? '' : 'muted'}>{draft.partition ?? t('design.option.noPartition')}</span>
      </label>
      {!isNew && (
        <label>
          {t('design.option.rows')}
          <span>{storage?.row_count != null ? storage.row_count.toLocaleString() : t('design.option.rowsUnknown')}</span>
        </label>
      )}
      {props.length > 0 && (
        <label>
          {t('design.option.storage')}
          <span>{props.join(' · ')}</span>
        </label>
      )}
    </div>
  )
}
```

- [ ] **Step 15: 新建 `web/src/features/design/SaveDialog.tsx`**

```tsx
// 保存确认框（M3 设计文档 7.4 节）：变更摘要、完整的 SQL、"用重建方式执行"开关（确定不支持时强制打开）、提示、
// 生产环境的横幅；执行、复制 SQL、在新查询标签中打开、取消。执行时后端用草稿重新生成语句，返回任务 id。

import { useQuery } from '@tanstack/react-query'
import { Alert, App, Button, Modal, Spin, Switch } from 'antd'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

import { useApi } from '../../app/context'
import { type DesignRequest, type Profile } from '../../shared/api'
import { useQueryActions } from '../editor/actions'
import SqlBox from './SqlBox'
import { describeChange, scriptOf } from './summary'

interface Props {
  profile: Profile
  request: DesignRequest
  onRebuild(on: boolean): void
  onReload(): void
  onClose(): void
  /** 已提交执行：任务 id。 */
  onStarted(taskId: string): void
}

export default function SaveDialog({ profile, request, onRebuild, onReload, onClose, onStarted }: Props) {
  const api = useApi()
  const { t } = useTranslation()
  const { message } = App.useApp()
  const { openSql } = useQueryActions()
  const [starting, setStarting] = useState(false)
  const { data: p, error, isFetching } = useQuery({
    queryKey: ['design-save', profile.id, JSON.stringify(request)],
    queryFn: () => api.previewDesign(profile.id, request),
    gcTime: 0,
    retry: false,
  })
  const plsql = profile.driver === 'oracle' || profile.driver === 'dameng'
  const script = p ? scriptOf(p.statements, plsql) : ''
  const blocked =
    !p || p.errors.length > 0 || p.conflict || !!p.rebuild.refused || !!p.denied || p.statements.length === 0
  const name = `${request.schema}.${request.draft.name}`
  const execute = async () => {
    setStarting(true)
    try {
      onStarted(await api.applyDesign(profile.id, request))
      onClose()
    } catch (e) {
      message.error((e as Error).message)
    } finally {
      setStarting(false)
    }
  }
  const danger = profile.environment === 'prod' || p?.rebuild.active

  return (
    <Modal
      open
      width={900}
      title={t('design.save.title', { name })}
      onCancel={onClose}
      footer={[
        <Button key="cancel" onClick={onClose}>
          {t('design.save.cancel')}
        </Button>,
        <Button
          key="copy"
          disabled={!script}
          onClick={() => void navigator.clipboard.writeText(script).then(() => message.success(t('design.save.copied')))}
        >
          {t('design.save.copy')}
        </Button>,
        <Button
          key="open"
          disabled={!script}
          onClick={() => {
            openSql(script, profile.id)
            onClose()
          }}
        >
          {t('design.save.openInTab')}
        </Button>,
        <Button key="run" type="primary" danger={danger} disabled={blocked} loading={starting} onClick={() => void execute()}>
          {t('design.save.execute')}
        </Button>,
      ]}
    >
      {isFetching && !p ? (
        <Spin style={{ margin: 48 }} />
      ) : error ? (
        <Alert type="error" message={(error as Error).message} />
      ) : p ? (
        <div className="design-save">
          {profile.environment === 'prod' && <Alert type="error" banner message={t('design.save.prod')} />}
          {p.conflict && (
            <Alert
              type="error"
              message={t('design.save.conflict')}
              action={
                <Button size="small" onClick={onReload}>
                  {t('design.toolbar.reload')}
                </Button>
              }
            />
          )}
          {p.errors.length > 0 && <Alert type="error" message={t('design.sql.errors', { count: p.errors.length })} />}
          {p.denied && <Alert type="error" message={t('design.save.denied', { reason: p.denied })} />}
          {p.rebuild.refused && <Alert type="error" message={t('design.sql.refused', { reason: p.rebuild.refused })} />}
          {request.name !== null && (
            <div className="row">
              <Switch
                size="small"
                checked={p.rebuild.active}
                disabled={p.rebuild.reasons.length > 0 || isFetching}
                onChange={onRebuild}
              />
              <span>{t('design.save.rebuildSwitch')}</span>
              {p.rebuild.reasons.length > 0 && <span className="muted">{t('design.save.rebuildForced')}</span>}
            </div>
          )}
          {p.rebuild.active && (
            <Alert
              type="warning"
              message={t('design.sql.rebuild')}
              description={p.rebuild.reasons.length ? p.rebuild.reasons.join(' · ') : undefined}
            />
          )}
          {[...p.warnings, ...p.confirm].map((w) => (
            <Alert key={w} type="warning" showIcon message={w} />
          ))}
          <Alert type="info" showIcon message={t(p.transactional ? 'design.sql.transactional' : 'design.sql.autocommit')} />
          <h4>{t('design.save.changes')}</h4>
          <ul className="design-changes">
            {p.changes.flatMap(describeChange).map((line, i) => (
              <li key={i}>{line}</li>
            ))}
          </ul>
          <h4>{t('design.save.sql')}</h4>
          <SqlBox sql={script} driver={profile.driver} height={260} />
        </div>
      ) : null}
    </Modal>
  )
}
```

- [ ] **Step 16: 新建 `web/src/features/design/SqlBox.tsx`**

```tsx
// 只读的 SQL 查看框（与"查看 DDL"相同的 Monaco 设置），设计器的 SQL 预览和保存确认框共用。引入 Monaco，由使用方懒加载。

import Editor from '@monaco-editor/react'

import type { DriverId } from '../../shared/api'
import { useSetting } from '../../shared/settings'
import { THEME_KEY, parseThemePref, resolveTheme, useSystemDark } from '../../shared/theme'
import { editorLanguage } from '../editor/dialect'
import '../editor/monaco'

export default function SqlBox({ sql, driver, height }: { sql: string; driver: DriverId | undefined; height: number | string }) {
  const theme = resolveTheme(parseThemePref(useSetting(THEME_KEY)), useSystemDark())
  return (
    <div style={{ height, border: '1px solid var(--border)' }}>
      <Editor
        language={driver ? editorLanguage(driver) : 'sql'}
        value={sql}
        theme={theme === 'dark' ? 'dblens-dark' : 'dblens-light'}
        options={{
          readOnly: true,
          domReadOnly: true,
          fontFamily: "'JetBrains Mono', 'Cascadia Mono', Consolas, 'Noto Sans Mono CJK SC', monospace",
          fontSize: 14,
          lineHeight: 22,
          minimap: { enabled: false },
          scrollBeyondLastLine: false,
          automaticLayout: true,
          wordWrap: 'on',
        }}
      />
    </div>
  )
}
```

- [ ] **Step 17: 修改 `web/src/features/design/logic.ts`**

```diff
--- a/web/src/features/design/logic.ts
+++ b/web/src/features/design/logic.ts
@@ -3,6 +3,8 @@
 
 import type {
   DesignCheck,
+  DesignIssue,
+  DesignSection,
   DesignColumn,
   DesignForeignKey,
   DesignIndex,
@@ -205,6 +207,43 @@ export function togglePrimaryKey(d: Draft, column: string): Draft {
   return { ...d, primaryKey, columns }
 }
 
+// —— 索引、约束等列表的编辑 ——
+
+export type ListName = 'uniques' | 'checks' | 'foreignKeys' | 'indexes'
+
+export function updateItem<K extends ListName>(d: Draft, list: K, key: string, patch: Partial<Draft[K][number]>): Draft {
+  return { ...d, [list]: (d[list] as Row[]).map((r) => (r.key === key ? { ...r, ...patch } : r)) }
+}
+
+export function addItem<K extends ListName>(d: Draft, list: K, item: Draft[K][number]): Draft {
+  return { ...d, [list]: [...d[list], item] }
+}
+
+/** 删除一项：新增的直接去掉，原有的标记删除。 */
+export function removeItem(d: Draft, list: ListName, key: string): Draft {
+  const rows = d[list] as Row[]
+  const row = rows.find((r) => r.key === key)
+  if (!row) return d
+  return {
+    ...d,
+    [list]: row.origin === null ? rows.filter((r) => r.key !== key) : rows.map((r) => (r.key === key ? { ...r, deleted: true } : r)),
+  }
+}
+
+/** 还原一项为原表中的内容。 */
+export function restoreItem(d: Draft, list: ListName, key: string, original: Draft): Draft {
+  const rows = d[list] as Row[]
+  const row = rows.find((r) => r.key === key)
+  const orig = (original[list] as Row[]).find((o) => o.origin === row?.origin)
+  if (!row || !orig) return d
+  return { ...d, [list]: rows.map((r) => (r.key === key ? { ...orig, key } : r)) }
+}
+
+/** 草稿中没有标为删除的行的序号：后端校验错误的行号是按整理后的列表算的，用它换回草稿中的行。 */
+export function liveKeys(rows: Row[]): string[] {
+  return rows.filter((r) => !r.deleted).map((r) => r.key)
+}
+
 /** 在列表中挪动一项（主键列的顺序等）。 */
 export function moveItem<T>(list: T[], i: number, delta: -1 | 1): T[] {
   const j = i + delta
@@ -297,3 +336,48 @@ export function columnState(c: DraftColumn, original: Draft | null): RowState {
 
 /** 去掉行标识后比较内容。 */
 export const sameContent = <T extends Row>(a: T, b: T) => JSON.stringify(strip(a)) === JSON.stringify(strip(b))
+
+// —— 校验错误 ——
+
+/** 设计器的子标签。 */
+export type Pane = 'columns' | 'indexes' | 'foreignKeys' | 'constraints' | 'options'
+
+const PANE: Record<DesignSection, Pane> = {
+  table: 'options',
+  columns: 'columns',
+  primary_key: 'options',
+  indexes: 'indexes',
+  foreign_keys: 'foreignKeys',
+  uniques: 'constraints',
+  checks: 'constraints',
+}
+
+const LIST: Partial<Record<DesignSection, keyof Draft>> = {
+  columns: 'columns',
+  indexes: 'indexes',
+  foreign_keys: 'foreignKeys',
+  uniques: 'uniques',
+  checks: 'checks',
+}
+
+export interface Issues {
+  /** 各子标签的错误数。 */
+  counts: Partial<Record<Pane, number>>
+  /** 某一行某个字段的错误；`key` 为行的 key（表和主键的错误用 section 名）。 */
+  at(key: string, field: string): string | undefined
+}
+
+/** 把后端的校验错误（行号按整理后的列表）换成草稿中的行。 */
+export function issuesOf(errors: DesignIssue[], d: Draft): Issues {
+  const counts: Partial<Record<Pane, number>> = {}
+  const map = new Map<string, string>()
+  for (const e of errors) {
+    const pane = PANE[e.section]
+    counts[pane] = (counts[pane] ?? 0) + 1
+    const list = LIST[e.section]
+    const key = list && e.row !== null ? liveKeys(d[list] as Row[])[e.row] : e.section
+    const id = `${key}|${e.field ?? ''}`
+    if (!map.has(id)) map.set(id, e.message)
+  }
+  return { counts, at: (key, field) => map.get(`${key}|${field}`) ?? map.get(`${key}|`) }
+}
```

- [ ] **Step 18: 新建 `web/src/features/design/summary.ts`**

```ts
// 变更列表的可读摘要（保存确认框中列出），以及复制、在查询标签中打开用的脚本文本。

import type { DesignColumn, DesignStep, TableChange } from '../../shared/api'
import { t } from '../../shared/i18n'

const list = (cols: string[]) => cols.join(', ')
const text = (v: string | null) => v?.trim() || null

/** 一项变更的摘要：修改列时每个改了的方面一行。 */
export function describeChange(c: TableChange): string[] {
  switch (c.kind) {
    case 'create_table':
      return [t('design.change.createTable', { name: c.table.name, count: c.table.columns.length })]
    case 'set_table_comment':
      return [t('design.change.tableComment')]
    case 'add_column':
      return [t('design.change.addColumn', { name: c.column.name, type: c.column.data_type.native })]
    case 'drop_column':
      return [t('design.change.dropColumn', { name: c.name })]
    case 'rename_column':
      return [t('design.change.renameColumn', { from: c.from, to: c.to })]
    case 'alter_column':
      return alterLines(c.from, c.to)
    case 'move_column':
      return [t('design.change.moveColumn', { name: c.column.name })]
    case 'add_primary_key':
      return [t('design.change.addPrimaryKey', { columns: list(c.key.columns) })]
    case 'drop_primary_key':
      return [t('design.change.dropPrimaryKey')]
    case 'add_unique':
      return [t('design.change.addUnique', { name: c.unique.name ?? list(c.unique.columns) })]
    case 'drop_unique':
      return [t('design.change.dropUnique', { name: c.unique.name ?? list(c.unique.columns) })]
    case 'add_check':
      return [t('design.change.addCheck', { name: c.check.name ?? c.check.expression })]
    case 'drop_check':
      return [t('design.change.dropCheck', { name: c.check.name ?? c.check.expression })]
    case 'add_foreign_key':
      return [t('design.change.addForeignKey', { name: c.key.name ?? list(c.key.columns), table: c.key.ref_table })]
    case 'drop_foreign_key':
      return [t('design.change.dropForeignKey', { name: c.key.name ?? list(c.key.columns) })]
    case 'add_index':
      return [t('design.change.addIndex', { name: c.index.name })]
    case 'drop_index':
      return [t('design.change.dropIndex', { name: c.index.name })]
    case 'rename_index':
      return [t('design.change.renameIndex', { from: c.from, to: c.index.name })]
  }
}

function alterLines(from: DesignColumn, to: DesignColumn): string[] {
  const name = to.name
  const out: string[] = []
  if (from.data_type.native.trim().toLowerCase() !== to.data_type.native.trim().toLowerCase()) {
    out.push(t('design.change.columnType', { name, from: from.data_type.native, to: to.data_type.native }))
  }
  if (from.nullable !== to.nullable) {
    out.push(t(to.nullable ? 'design.change.columnNull' : 'design.change.columnNotNull', { name }))
  }
  if (text(from.default) !== text(to.default)) {
    out.push(t('design.change.columnDefault', { name, value: text(to.default) ?? t('design.change.none') }))
  }
  if (text(from.comment) !== text(to.comment)) out.push(t('design.change.columnComment', { name }))
  if (from.auto_increment !== to.auto_increment) {
    out.push(t(to.auto_increment ? 'design.change.columnAutoOn' : 'design.change.columnAutoOff', { name }))
  }
  if (text(from.generated) !== text(to.generated)) out.push(t('design.change.columnGenerated', { name }))
  return out
}

/** 预览中的语句拼成脚本：每条以 `;` 结束（PL/SQL 块之后加一行 `/`），由程序执行的核对行数写成注释。 */
export function scriptOf(steps: DesignStep[], plsql: boolean): string {
  return steps
    .map((s) => {
      if (s.kind === 'count_check') return `-- ${t('design.save.countCheck', { source: s.source, target: s.target })}`
      const sql = s.sql.trimEnd()
      if (sql.endsWith(';')) return plsql ? `${sql}\n/` : sql
      return `${sql};`
    })
    .join('\n\n')
}
```

- [ ] **Step 19: 修改 `web/src/features/editor/Workspace.tsx`**

```diff
--- a/web/src/features/editor/Workspace.tsx
+++ b/web/src/features/editor/Workspace.tsx
@@ -3,9 +3,10 @@
 // 标签多时先缩窄（悬停看全名），再放不下就用滚轮横向滚动，不显示滚动条；右端 ▾ 列出全部标签；右键菜单批量关闭。
 
 import { Dropdown, type MenuProps } from 'antd'
-import { IconChevronDown, IconFileCode, IconPlus, IconTable, IconX } from '@tabler/icons-react'
+import { IconChevronDown, IconFileCode, IconPlus, IconTable, IconTableOptions, IconX } from '@tabler/icons-react'
 import { useQuery } from '@tanstack/react-query'
-import { type WheelEvent, useEffect, useRef } from 'react'
+import { Spin } from 'antd'
+import { Suspense, type WheelEvent, lazy, useEffect, useRef } from 'react'
 import { useTranslation } from 'react-i18next'
 
 import { useApi } from '../../app/context'
@@ -18,6 +19,9 @@ import { useQueryActions } from './actions'
 import { useTabRuntime } from './runner'
 import { type QueryTab, useTabs } from './tabs'
 
+// 设计器带 Monaco 的 SQL 预览，打开第一个设计器标签时才加载
+const DesignView = lazy(() => import('../design/DesignView'))
+
 function TabTitle({ tab, active, envClass, profileName, profile }: {
   tab: QueryTab
   active: boolean
@@ -58,7 +62,9 @@ function TabTitle({ tab, active, envClass, profileName, profile }: {
         }}
         title={`${tab.title} · ${profileName}`}
       >
-        {tab.data ? (
+        {tab.design ? (
+          <IconTableOptions size={16} />
+        ) : tab.data ? (
           <IconTable size={16} />
         ) : profile ? (
           <DbIcon driver={profile.driver} variant={rt.session?.variant ?? profile.variant} />
@@ -152,7 +158,15 @@ export default function Workspace() {
         )}
       </div>
       {tab &&
-        (tab.data ? <DataView key={tab.id} tab={tab} profiles={profiles} /> : <QueryView tab={tab} profiles={profiles} />)}
+        (tab.design ? (
+          <Suspense fallback={<Spin style={{ margin: 48 }} />}>
+            <DesignView key={tab.id} tab={tab} profiles={profiles} />
+          </Suspense>
+        ) : tab.data ? (
+          <DataView key={tab.id} tab={tab} profiles={profiles} />
+        ) : (
+          <QueryView tab={tab} profiles={profiles} />
+        ))}
     </div>
   )
 }
```

- [ ] **Step 20: 修改 `web/src/features/editor/actions.tsx`**

```diff
--- a/web/src/features/editor/actions.tsx
+++ b/web/src/features/editor/actions.tsx
@@ -7,6 +7,7 @@ import { useTranslation } from 'react-i18next'
 import { useApi } from '../../app/context'
 import { useUi } from '../../app/ui'
 import { hasPendingEdits, useEdits } from '../data/editStore'
+import { hasDesignChanges, useDesigns } from '../design/designStore'
 import { sendInsert } from './commands'
 import { disposeRunner, useRuntime } from './runner'
 import { markRun, useTabs } from './tabs'
@@ -44,19 +45,33 @@ export function useQueryActions() {
     } else useUi.getState().openConnection(null)
   }
 
-  /** 插入到当前编辑器的光标处；当前不是查询标签（没有标签或是数据标签）时新开一个。 */
+  /** 插入到当前编辑器的光标处；当前不是查询标签（没有标签，或是数据标签、设计器标签）时新开一个。 */
   const insertSql = (sql: string, profileId?: string) => {
     const { tabs, active } = useTabs.getState()
     const tab = tabs.find((x) => x.id === active)
-    if (tab && !tab.data) sendInsert(sql)
+    if (tab && !tab.data && !tab.design) sendInsert(sql)
     else openSql(sql, profileId)
   }
 
-  /** 关闭标签页：数据标签有未提交的编辑时先确认放弃；有未提交的事务时先问提交还是回滚。对话框结束（关闭或取消）后完成。 */
+  /** 关闭标签页：设计器有未保存的改动、数据标签有未提交的编辑时先确认放弃；有未提交的事务时先问提交还是回滚。对话框结束（关闭或取消）后完成。 */
   const closeQuery = (tabId: string): Promise<void> =>
     new Promise((done) => {
       const tab = useTabs.getState().tabs.find((x) => x.id === tabId)
       if (!tab) return done()
+      if (tab.design && hasDesignChanges(tabId)) {
+        modal.confirm({
+          title: t('design.close.title', { title: tab.title }),
+          content: t('design.close.content'),
+          okText: t('design.close.ok'),
+          okType: 'danger',
+          onOk: () => {
+            useDesigns.getState().clear(tabId)
+            void closeQuery(tabId).then(done)
+          },
+          onCancel: () => done(),
+        })
+        return
+      }
       if (tab.data && hasPendingEdits(tabId)) {
         modal.confirm({
           title: t('editor.data.edit.discardTitle'),
@@ -74,6 +89,7 @@ export function useQueryActions() {
       const finish = async (commit: boolean) => {
         await disposeRunner(tabId, commit)
         useEdits.getState().clear(tabId)
+        useDesigns.getState().clear(tabId)
         useTabs.getState().close(tabId)
         done()
       }
@@ -109,7 +125,8 @@ export function useQueryActions() {
   /** 依次关闭多个标签；要确认的标签先切过去再问，取消的留下，接着处理下一个。 */
   const closeMany = async (ids: string[]) => {
     for (const id of ids) {
-      const needsAsk = hasPendingEdits(id) || useRuntime.getState().tabs[id]?.session?.in_transaction
+      const needsAsk =
+        hasPendingEdits(id) || hasDesignChanges(id) || useRuntime.getState().tabs[id]?.session?.in_transaction
       if (needsAsk) useTabs.getState().activate(id)
       await closeQuery(id)
     }
```

- [ ] **Step 21: 修改 `web/src/features/editor/tabs.ts`**

```diff
--- a/web/src/features/editor/tabs.ts
+++ b/web/src/features/editor/tabs.ts
@@ -1,6 +1,7 @@
 // 查询标签页（M1 设计文档 1 节）：每个标签绑定一个连接，有自己的 SQL。打开的标签和其中的 SQL 存进设置
 // （ui.tabs），下次启动时恢复；会话不恢复，第一次执行时再打开。
 // 数据标签（M1c2 设计文档第 5 节）：带 data 的标签，只读地浏览一张表 / 视图 / 物化视图，同一对象只开一个。
+// 设计器标签（M3 设计文档 7.1 节）：带 design 的标签，设计一张已有的表（同一张表只开一个）或新建表；草稿不保存。
 
 import { create } from 'zustand'
 
@@ -20,6 +21,12 @@ export interface DataTarget {
   order?: { column: string; desc: boolean } | null
 }
 
+/** 设计器标签设计的表：`table` 为 null 时是新建表（`schema` 为空串时由用户在"选项"中选）。 */
+export interface DesignTarget {
+  schema: string
+  table: string | null
+}
+
 export interface QueryTab {
   id: string
   /** 「查询1」这样的标题：新建时按界面语言生成，之后是数据，不随语言变化。数据标签为对象名。 */
@@ -28,6 +35,8 @@ export interface QueryTab {
   sql: string
   /** 有值时是数据标签。 */
   data?: DataTarget
+  /** 有值时是设计器标签。 */
+  design?: DesignTarget
 }
 
 const TARGET_KINDS: readonly unknown[] = ['table', 'view', 'materialized_view'] satisfies TargetKind[]
@@ -44,6 +53,11 @@ function isDataTarget(v: unknown): v is DataTarget {
   )
 }
 
+function isDesignTarget(v: unknown): v is DesignTarget {
+  const d = v as DesignTarget | null
+  return typeof d?.schema === 'string' && (d.table === null || typeof d.table === 'string')
+}
+
 /** 是否同一个对象的数据标签。 */
 export function sameData(tab: QueryTab, profileId: string, d: DataTarget): boolean {
   return (
@@ -73,7 +87,8 @@ export function parseTabs(raw: string | undefined): Persisted {
         typeof t.title === 'string' &&
         typeof t.profileId === 'string' &&
         typeof t.sql === 'string' &&
-        (t.data === undefined || isDataTarget(t.data)),
+        (t.data === undefined || isDataTarget(t.data)) &&
+        (t.design === undefined || isDesignTarget(t.design)),
     )
     const active = tabs.some((t) => t.id === v.active) ? (v.active as string) : (tabs[0]?.id ?? null)
     return { tabs, active, seq: typeof v.seq === 'number' && v.seq > 0 ? v.seq : tabs.length + 1 }
@@ -87,6 +102,10 @@ interface TabsState extends Persisted {
   open(profileId: string, sql?: string): string
   /** 打开对象的数据标签：已经打开过时切换过去。返回标签的 id。 */
   openData(profileId: string, target: DataTarget): string
+  /** 打开表设计器：已有的表已经打开过时切换过去；新建表每次开一个新标签。返回标签的 id。 */
+  openDesign(profileId: string, target: DesignTarget): string
+  /** 新建的表保存后，标签转为设计这张表。 */
+  setDesign(id: string, target: DesignTarget): void
   /** 修改数据标签的过滤条件和排序。 */
   setData(id: string, patch: Pick<DataTarget, 'filter' | 'order'>): void
   close(id: string): void
@@ -121,6 +140,24 @@ export const useTabs = create<TabsState>()((set, get) => ({
     set({ tabs: [...tabs, tab], active: tab.id })
     return tab.id
   },
+  openDesign: (profileId, target) => {
+    const { tabs } = get()
+    const same =
+      target.table !== null &&
+      tabs.find((x) => x.profileId === profileId && x.design?.schema === target.schema && x.design.table === target.table)
+    if (same) {
+      set({ active: same.id })
+      return same.id
+    }
+    const title = target.table ?? t('design.newTable')
+    const tab: QueryTab = { id: newId(), title, profileId, sql: '', design: { ...target } }
+    set({ tabs: [...tabs, tab], active: tab.id })
+    return tab.id
+  },
+  setDesign: (id, target) =>
+    set({
+      tabs: get().tabs.map((x) => (x.id === id ? { ...x, design: { ...target }, title: target.table ?? x.title } : x)),
+    }),
   setData: (id, patch) =>
     set({ tabs: get().tabs.map((x) => (x.id === id && x.data ? { ...x, data: { ...x.data, ...patch } } : x)) }),
   close: (id) => {
```

- [ ] **Step 22: 修改 `web/src/features/tree/ObjectTree.tsx`**

```diff
--- a/web/src/features/tree/ObjectTree.tsx
+++ b/web/src/features/tree/ObjectTree.tsx
@@ -293,22 +293,24 @@ export default function ObjectTree() {
           ]
         : []
 
-  /** schema 节点：导入到新表、执行 SQL 文件，后面接分隔线。 */
+  /** schema 节点：新建表、导入到新表、执行 SQL 文件，后面接分隔线。 */
   function schemaItems(k: NodeKey): NonNullable<MenuProps['items']> {
     if (k.type !== 's') return []
     return [
+      { key: 'new-table', label: t('tree.menu.newTable') },
       { key: 'import-new', label: t('tree.menu.importNew') },
       { key: 'sql-file', label: t('tree.menu.sqlFile') },
       { type: 'divider' },
     ]
   }
 
-  /** 对象节点的查看数据、导入导出、查看 DDL、生成 SQL，后面接分隔线；其他节点没有。 */
+  /** 对象节点的查看数据、设计表、导入导出、查看 DDL、生成 SQL，后面接分隔线；其他节点没有。 */
   function objectItems(k: NodeKey): NonNullable<MenuProps['items']> {
     if (k.type !== 'o') return []
     const items: NonNullable<MenuProps['items']> = []
     const target = targetKind(k.kind)
     if (target) items.push({ key: 'view-data', label: t('tree.menu.viewData') })
+    if (target === 'table') items.push({ key: 'design', label: t('tree.menu.design') })
     if (target === 'table') items.push({ key: 'import', label: t('tree.menu.import') })
     if (target) items.push({ key: 'export-data', label: t('tree.menu.exportData') })
     if (ddlKind(k.kind, k.routineKind)) items.push({ key: 'ddl', label: t('tree.menu.viewDdl') })
@@ -342,6 +344,8 @@ export default function ObjectTree() {
         })
         insertSql(sql, k.profileId)
       } else if (key === 'view-data') openData(k)
+      else if (key === 'design' && k.type === 'o') useTabs.getState().openDesign(k.profileId, { schema: k.schema, table: k.name })
+      else if (key === 'new-table' && k.type === 's') useTabs.getState().openDesign(k.profileId, { schema: k.schema, table: null })
       else if (key === 'import' && k.type === 'o') {
         useImportDialogs.getState().openImport({ profileId: k.profileId, schema: k.schema, table: k.name, create: false })
       } else if (key === 'import-new' && k.type === 's') {
```

- [ ] **Step 23: 修改 `web/src/locales/en/app.json`**

```diff
--- a/web/src/locales/en/app.json
+++ b/web/src/locales/en/app.json
@@ -63,6 +63,7 @@
       "newConnection": "New Connection…",
       "newQuery": "New Query",
       "closeTab": "Close Tab",
+      "newTable": "New table",
       "settings": "Settings…",
       "quit": "Exit",
       "import": "Import Data…",
```

- [ ] **Step 24: 新建 `web/src/locales/en/design.json`**

```json
{
  "newTable": "New table",
  "remove": "Delete",
  "restore": "Restore row",
  "toolbar": {
    "save": "Save",
    "discard": "Undo all",
    "discardConfirm": "Undo all unsaved changes?",
    "reload": "Reload",
    "reloadConfirm": "Reloading discards unsaved changes. Continue?"
  },
  "tabs": {
    "columns": "Columns",
    "indexes": "Indexes",
    "foreignKeys": "Foreign keys",
    "constraints": "Constraints",
    "options": "Options",
    "sql": "SQL preview"
  },
  "column": {
    "name": "Name",
    "type": "Type",
    "length": "Length",
    "scale": "Scale",
    "notNull": "Not null",
    "primaryKey": "Key",
    "autoIncrement": "Auto inc.",
    "default": "Default",
    "defaultHint": "Written as is: quote strings, as in 'abc'; functions as in CURRENT_TIMESTAMP",
    "comment": "Comment",
    "add": "Add column",
    "moveUp": "Move up",
    "moveDown": "Move down",
    "empty": "No columns yet",
    "generated": "Generation expression",
    "generatedHint": "Leave empty for a regular column",
    "native": "Native type",
    "noSelection": "Select a column to see more properties",
    "reorderNote": "This database cannot reorder existing columns: moving one or inserting a column in the middle rebuilds the table on save"
  },
  "index": {
    "name": "Name",
    "columns": "Columns",
    "columnsHint": "Pick columns or type expressions; add DESC after a name for descending",
    "unique": "Unique",
    "method": "Method",
    "add": "Add index",
    "empty": "No indexes (indexes backing the primary key and unique constraints are not listed here)"
  },
  "fk": {
    "name": "Name",
    "columns": "Columns",
    "refSchema": "Referenced schema",
    "refTable": "Referenced table",
    "refColumns": "Referenced columns",
    "onDelete": "On delete",
    "onUpdate": "On update",
    "unsupported": "Not supported",
    "add": "Add foreign key",
    "empty": "No foreign keys"
  },
  "constraint": {
    "uniques": "Unique constraints",
    "checks": "Check constraints",
    "name": "Name",
    "columns": "Columns",
    "expression": "Expression",
    "addUnique": "Add unique constraint",
    "addCheck": "Add check constraint",
    "noUniques": "No unique constraints",
    "noChecks": "No check constraints",
    "checksIgnored": "This database version parses check constraints but does not enforce them"
  },
  "option": {
    "schema": "Schema",
    "schemaRequired": "Select a schema",
    "name": "Table name",
    "nameReadonly": "An existing table cannot be renamed here",
    "comment": "Table comment",
    "pkName": "Primary key name",
    "pkColumns": "Primary key column order",
    "noPk": "No primary key (tick key columns under Columns)",
    "partition": "Partitioning",
    "noPartition": "Not partitioned",
    "rows": "Estimated rows",
    "rowsUnknown": "Unknown (no statistics)",
    "storage": "Storage",
    "engine": "Engine: {{value}}",
    "charset": "Charset: {{value}}",
    "collation": "Collation: {{value}}",
    "tablespace": "Tablespace: {{value}}"
  },
  "sql": {
    "nothing": "No changes",
    "errors_one": "{{count}} error; fix it to generate SQL",
    "errors_other": "{{count}} errors; fix them to generate SQL",
    "rebuild": "The table will be rebuilt",
    "refused": "Cannot rebuild: {{reason}}",
    "transactional": "Runs in one transaction: if any statement fails, everything is rolled back",
    "autocommit": "This database commits DDL automatically: statements already run are not rolled back if a later one fails"
  },
  "save": {
    "title": "Save changes: {{name}}",
    "prod": "This is a production connection. Review carefully before running",
    "conflict": "The table structure has been changed; reload and edit again",
    "denied": "Denied by the execution policy: {{reason}}",
    "rebuildSwitch": "Run as a table rebuild",
    "rebuildForced": "Some changes are not supported by the database; the table must be rebuilt",
    "changes": "Changes",
    "sql": "SQL",
    "countCheck": "Check row counts: {{source}} and {{target}} must match (done by DBLens; check it yourself when running by hand)",
    "cancel": "Cancel",
    "copy": "Copy SQL",
    "copied": "Copied",
    "openInTab": "Open in new query tab",
    "execute": "Run"
  },
  "run": {
    "running": "Running: {{done}} / {{total}}",
    "cancel": "Cancel",
    "done": "Saved",
    "failed": "Failed",
    "cancelled": "Cancelled",
    "failedAt": "Statement {{n}} failed:",
    "remaining": "Open the statements not run in a new query tab"
  },
  "close": {
    "title": "Discard changes to {{title}}?",
    "content": "The designer has unsaved changes that will be lost.",
    "ok": "Discard and close"
  },
  "change": {
    "createTable_one": "Create table {{name}} ({{count}} column)",
    "createTable_other": "Create table {{name}} ({{count}} columns)",
    "tableComment": "Change the table comment",
    "addColumn": "Add column {{name}}: {{type}}",
    "dropColumn": "Drop column {{name}}",
    "renameColumn": "Rename column {{from}} to {{to}}",
    "columnType": "Column {{name}}: type {{from}} → {{to}}",
    "columnNull": "Column {{name}}: allow null",
    "columnNotNull": "Column {{name}}: not null",
    "columnDefault": "Column {{name}}: default {{value}}",
    "none": "(none)",
    "columnComment": "Change the comment of column {{name}}",
    "columnAutoOn": "Column {{name}}: auto increment",
    "columnAutoOff": "Column {{name}}: no auto increment",
    "columnGenerated": "Change the generation expression of column {{name}}",
    "moveColumn": "Move column {{name}}",
    "addPrimaryKey": "Add primary key ({{columns}})",
    "dropPrimaryKey": "Drop the primary key",
    "addUnique": "Add unique constraint {{name}}",
    "dropUnique": "Drop unique constraint {{name}}",
    "addCheck": "Add check constraint {{name}}",
    "dropCheck": "Drop check constraint {{name}}",
    "addForeignKey": "Add foreign key {{name}} (references {{table}})",
    "dropForeignKey": "Drop foreign key {{name}}",
    "addIndex": "Add index {{name}}",
    "dropIndex": "Drop index {{name}}",
    "renameIndex": "Rename index {{from}} to {{to}}"
  }
}
```

- [ ] **Step 25: 修改 `web/src/locales/en/editor.json`**

```diff
--- a/web/src/locales/en/editor.json
+++ b/web/src/locales/en/editor.json
@@ -114,6 +114,7 @@
       "applied_other": "{{count}} statements committed",
       "noSession": "The data has not been read yet; refresh first"
     },
-    "import": "Import"
+    "import": "Import",
+    "design": "Design"
   }
 }
```

- [ ] **Step 26: 修改 `web/src/locales/en/tree.json`**

```diff
--- a/web/src/locales/en/tree.json
+++ b/web/src/locales/en/tree.json
@@ -30,6 +30,7 @@
     "copyName": "Copy Name",
     "refresh": "Refresh",
     "viewData": "View Data",
+    "design": "Design table",
     "viewDdl": "View DDL",
     "generate": "Generate SQL",
     "join": "Join Query",
@@ -39,7 +40,8 @@
     "import": "Import Data…",
     "importNew": "Import into New Table…",
     "sqlFile": "Run SQL File…",
-    "exportData": "Export Data…"
+    "exportData": "Export Data…",
+    "newTable": "New table"
   },
   "duplicate": {
     "name": "{{name}} copy",
```

- [ ] **Step 27: 修改 `web/src/locales/index.ts`**

```diff
--- a/web/src/locales/index.ts
+++ b/web/src/locales/index.ts
@@ -4,6 +4,7 @@
 import enApp from './en/app.json'
 import enCommon from './en/common.json'
 import enConnections from './en/connections.json'
+import enDesign from './en/design.json'
 import enEditor from './en/editor.json'
 import enImport from './en/import.json'
 import enResults from './en/results.json'
@@ -11,6 +12,7 @@ import enTree from './en/tree.json'
 import zhApp from './zh-CN/app.json'
 import zhCommon from './zh-CN/common.json'
 import zhConnections from './zh-CN/connections.json'
+import zhDesign from './zh-CN/design.json'
 import zhEditor from './zh-CN/editor.json'
 import zhImport from './zh-CN/import.json'
 import zhResults from './zh-CN/results.json'
@@ -24,6 +26,7 @@ export const zhCN = {
   editor: zhEditor,
   results: zhResults,
   import: zhImport,
+  design: zhDesign,
 }
 
 export const en = {
@@ -34,4 +37,5 @@ export const en = {
   editor: enEditor,
   results: enResults,
   import: enImport,
+  design: enDesign,
 }
```

- [ ] **Step 28: 修改 `web/src/locales/zh-CN/app.json`**

```diff
--- a/web/src/locales/zh-CN/app.json
+++ b/web/src/locales/zh-CN/app.json
@@ -62,6 +62,7 @@
       "newConnection": "新建连接…",
       "newQuery": "新建查询",
       "closeTab": "关闭标签",
+      "newTable": "新建表",
       "settings": "设置…",
       "quit": "退出",
       "import": "导入数据…",
```

- [ ] **Step 29: 新建 `web/src/locales/zh-CN/design.json`**

```json
{
  "newTable": "新表",
  "remove": "删除",
  "restore": "还原此行",
  "toolbar": {
    "save": "保存",
    "discard": "撤销全部",
    "discardConfirm": "撤销全部未保存的改动？",
    "reload": "重新加载",
    "reloadConfirm": "重新加载会丢弃未保存的改动，继续吗？"
  },
  "tabs": {
    "columns": "列",
    "indexes": "索引",
    "foreignKeys": "外键",
    "constraints": "约束",
    "options": "选项",
    "sql": "SQL 预览"
  },
  "column": {
    "name": "名称",
    "type": "类型",
    "length": "长度",
    "scale": "小数位",
    "notNull": "非空",
    "primaryKey": "主键",
    "autoIncrement": "自增",
    "default": "默认值",
    "defaultHint": "按原文写入：字符串要加引号，如 'abc'；函数如 CURRENT_TIMESTAMP",
    "comment": "注释",
    "add": "新增列",
    "moveUp": "上移",
    "moveDown": "下移",
    "empty": "还没有列",
    "generated": "生成列表达式",
    "generatedHint": "不是生成列时留空",
    "native": "原生类型",
    "noSelection": "选中一列查看更多属性",
    "reorderNote": "该库不能调整已有列的顺序：挪动已有列或在中间加列，保存时将重建表"
  },
  "index": {
    "name": "名称",
    "columns": "列",
    "columnsHint": "选择列或输入表达式；列名后加 DESC 表示降序",
    "unique": "唯一",
    "method": "方法",
    "add": "新增索引",
    "empty": "没有索引（主键和唯一约束自带的索引不在这里列出）"
  },
  "fk": {
    "name": "名称",
    "columns": "列",
    "refSchema": "引用的 schema",
    "refTable": "引用的表",
    "refColumns": "引用列",
    "onDelete": "删除时",
    "onUpdate": "更新时",
    "unsupported": "该库不支持",
    "add": "新增外键",
    "empty": "没有外键"
  },
  "constraint": {
    "uniques": "唯一约束",
    "checks": "检查约束",
    "name": "名称",
    "columns": "列",
    "expression": "表达式",
    "addUnique": "新增唯一约束",
    "addCheck": "新增检查约束",
    "noUniques": "没有唯一约束",
    "noChecks": "没有检查约束",
    "checksIgnored": "该库版本只解析、不执行检查约束"
  },
  "option": {
    "schema": "Schema",
    "schemaRequired": "请选择 schema",
    "name": "表名",
    "nameReadonly": "已有表不能在这里改名",
    "comment": "表注释",
    "pkName": "主键名",
    "pkColumns": "主键列的顺序",
    "noPk": "没有主键（在\"列\"中勾选主键列）",
    "partition": "分区",
    "noPartition": "不是分区表",
    "rows": "行数估计",
    "rowsUnknown": "未知（没有统计信息）",
    "storage": "存储",
    "engine": "引擎 {{value}}",
    "charset": "字符集 {{value}}",
    "collation": "排序规则 {{value}}",
    "tablespace": "表空间 {{value}}"
  },
  "sql": {
    "nothing": "没有改动",
    "errors": "有 {{count}} 处错误，修正后才能生成 SQL",
    "rebuild": "将重建表",
    "refused": "不能重建：{{reason}}",
    "transactional": "整体在一个事务中执行：任何一条失败都全部回滚",
    "autocommit": "该库的 DDL 自动提交：中途失败时，已执行的语句不会回滚"
  },
  "save": {
    "title": "保存改动：{{name}}",
    "prod": "这是生产环境的连接，请仔细核对后再执行",
    "conflict": "表结构已被修改，请重新加载后再编辑",
    "denied": "被执行策略拒绝：{{reason}}",
    "rebuildSwitch": "用重建方式执行",
    "rebuildForced": "库不支持其中的改动，必须重建表",
    "changes": "变更",
    "sql": "SQL",
    "countCheck": "核对行数：{{source}} 与 {{target}} 的行数一致（由 DBLens 执行；手动执行时请自行核对）",
    "cancel": "取消",
    "copy": "复制 SQL",
    "copied": "已复制",
    "openInTab": "在新查询标签中打开",
    "execute": "执行"
  },
  "run": {
    "running": "正在执行：{{done}} / {{total}}",
    "cancel": "取消",
    "done": "已保存",
    "failed": "执行失败",
    "cancelled": "已取消",
    "failedAt": "第 {{n}} 条语句失败：",
    "remaining": "在新查询标签中打开没有执行的语句"
  },
  "close": {
    "title": "放弃对 {{title}} 的改动？",
    "content": "设计器中有未保存的改动，关闭后将丢失。",
    "ok": "放弃并关闭"
  },
  "change": {
    "createTable": "新建表 {{name}}（{{count}} 列）",
    "tableComment": "修改表注释",
    "addColumn": "新增列 {{name}}：{{type}}",
    "dropColumn": "删除列 {{name}}",
    "renameColumn": "列 {{from}} 改名为 {{to}}",
    "columnType": "修改列 {{name}}：类型 {{from}} → {{to}}",
    "columnNull": "修改列 {{name}}：允许为空",
    "columnNotNull": "修改列 {{name}}：不允许为空",
    "columnDefault": "修改列 {{name}}：默认值改为 {{value}}",
    "none": "（无）",
    "columnComment": "修改列 {{name}} 的注释",
    "columnAutoOn": "修改列 {{name}}：设为自增",
    "columnAutoOff": "修改列 {{name}}：取消自增",
    "columnGenerated": "修改列 {{name}} 的生成表达式",
    "moveColumn": "调整列 {{name}} 的位置",
    "addPrimaryKey": "新增主键（{{columns}}）",
    "dropPrimaryKey": "删除主键",
    "addUnique": "新增唯一约束 {{name}}",
    "dropUnique": "删除唯一约束 {{name}}",
    "addCheck": "新增检查约束 {{name}}",
    "dropCheck": "删除检查约束 {{name}}",
    "addForeignKey": "新增外键 {{name}}（引用 {{table}}）",
    "dropForeignKey": "删除外键 {{name}}",
    "addIndex": "新增索引 {{name}}",
    "dropIndex": "删除索引 {{name}}",
    "renameIndex": "索引 {{from}} 改名为 {{to}}"
  }
}
```

- [ ] **Step 30: 修改 `web/src/locales/zh-CN/editor.json`**

```diff
--- a/web/src/locales/zh-CN/editor.json
+++ b/web/src/locales/zh-CN/editor.json
@@ -111,6 +111,7 @@
       "applied": "已提交 {{count}} 条语句",
       "noSession": "数据还没有读取，请先刷新"
     },
-    "import": "导入"
+    "import": "导入",
+    "design": "设计表"
   }
 }
```

- [ ] **Step 31: 修改 `web/src/locales/zh-CN/tree.json`**

```diff
--- a/web/src/locales/zh-CN/tree.json
+++ b/web/src/locales/zh-CN/tree.json
@@ -30,6 +30,7 @@
     "copyName": "复制名称",
     "refresh": "刷新",
     "viewData": "查看数据",
+    "design": "设计表",
     "viewDdl": "查看 DDL",
     "generate": "生成 SQL",
     "join": "关联查询",
@@ -39,7 +40,8 @@
     "import": "导入数据…",
     "importNew": "导入到新表…",
     "sqlFile": "执行 SQL 文件…",
-    "exportData": "导出数据…"
+    "exportData": "导出数据…",
+    "newTable": "新建表"
   },
   "duplicate": {
     "name": "{{name}} 副本",
```

- [ ] **Step 32: 检查**

在 PowerShell 中执行（bash 里 `cd web` 会触发坏掉的 fnm 钩子，`&&` 后面的命令会被跳过）：

```powershell
$env:PATH = "D:\Environments\nvm4w\nodejs;$env:PATH"; Set-Location web; pnpm typecheck; pnpm test; pnpm build
```

Expected: typecheck、测试（新增 4 个）、打包通过。

- [ ] **Step 33: 提交**

```bash
git add web/src/app/TitleBar.tsx web/src/app/menus.test.ts web/src/app/menus.ts web/src/app/shell.css web/src/features/data/DataView.tsx web/src/features/design/ColumnsPane.tsx web/src/features/design/ConstraintsPane.tsx web/src/features/design/DesignView.tsx web/src/features/design/ForeignKeysPane.tsx web/src/features/design/IndexesPane.tsx web/src/features/design/ListPane.tsx web/src/features/design/OptionsPane.tsx web/src/features/design/SaveDialog.tsx web/src/features/design/SqlBox.tsx web/src/features/design/logic.ts web/src/features/design/summary.test.ts web/src/features/design/summary.ts web/src/features/editor/Workspace.tsx web/src/features/editor/actions.tsx web/src/features/editor/tabs.test.ts web/src/features/editor/tabs.ts web/src/features/tree/ObjectTree.tsx web/src/locales/en/app.json web/src/locales/en/design.json web/src/locales/en/editor.json web/src/locales/en/tree.json web/src/locales/index.ts web/src/locales/zh-CN/app.json web/src/locales/zh-CN/design.json web/src/locales/zh-CN/editor.json web/src/locales/zh-CN/tree.json
git commit -m "feat(web): 表设计器界面、保存确认与入口" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 11: 文档

- `docs/notes/m3-table-designer.md`：结构、与设计文档不一致的地方、验证（含浏览器实测的范围和未验证的项）。
- `docs/notes/m0-4b-api.md`：表设计器的接口；任务状态加 `remaining`。
- 总设计文档：M3 完成；国产库适配作为之后的单独里程碑。

**Files:**
- Modify: `docs/notes/m0-4b-api.md`
- Create: `docs/notes/m3-table-designer.md`
- Modify: `docs/superpowers/specs/2026-10-22-dblens-design.md`

- [ ] **Step 1: 修改 `docs/notes/m0-4b-api.md`**

```diff
--- a/docs/notes/m0-4b-api.md
+++ b/docs/notes/m0-4b-api.md
@@ -179,7 +179,7 @@
 | POST | `/profiles/{id}/imports/ddl` | `{schema, table, create}` → `{sql}`：新建表的建表语句 |
 | POST | `/imports` | 提交导入任务，返回 201 和 `{"id"}`。请求体：`{profile_id, source, file, schema, table, create?, mapping: [{source, target}], mode, transaction?, batch_size?, on_error?, empty_as_string?}` |
 | POST | `/sql-files` | 提交执行 SQL 文件的任务，返回 201 和 `{"id"}`。请求体：`{profile_id, source, encoding?, schema?, continue_on_error?, transaction?}` |
-| GET | `/tasks/{id}` | 任务状态：`{id, kind, state, phase, progress, warnings, error, error_line, committed_line, failures, confirm, report}` |
+| GET | `/tasks/{id}` | 任务状态：`{id, kind, state, phase, progress, warnings, error, error_line, committed_line, failures, confirm, report, remaining}` |
 | DELETE | `/tasks/{id}` | 进行中（或等待确认）时取消；已结束时删除任务和报告。204 |
 | POST | `/tasks/{id}/confirm` | `{yes}`：回答确认。不在等待确认时 400 |
 | GET | `/tasks/{id}/report?token=…` | 下载报告（导入的出错报告 CSV、SQL 文件的执行日志）。任务过期前可以多次下载 |
@@ -196,6 +196,21 @@
 - 上传大小上限：设置项 `ui.import.upload_limit_mb`（MB），默认 2048。超过时 400，已收到的部分删除。
 - 同时进行的任务最多 4 个，超过时 409。
 
+### 表设计器（M3 加入）
+
+| 方法 | 路径 | 说明 |
+|---|---|---|
+| GET | `/profiles/{id}/design/caps` | 方言能力和类型目录：`{reorder, checks, descending_index, on_delete, on_update, index_methods, types: [{name, params, char_unit}]}`；`params` 为 `none` / `length` / `precision_scale` |
+| GET | `/profiles/{id}/design/table?schema=&name=` | 原表（直接从库读取，不走缓存）：`{table, storage: {engine, charset, collation, tablespace, row_count}, fingerprint}` |
+| POST | `/profiles/{id}/design/preview` | 预览，不执行：`{errors, changes, statements, rebuild: {reasons, active, refused}, warnings, transactional, denied, confirm, conflict}` |
+| POST | `/profiles/{id}/design/apply` | 执行，返回 201 和 `{"id"}`（任务，`kind` 为 `design`，状态用 `/tasks/{id}`）。有校验错误、原表已被修改、不能重建、没有改动时 400，被执行策略拒绝时 403 |
+
+- 请求体：`{schema, name, draft, renames: {columns, indexes}, rebuild, fingerprint}`。`name` 为已有表的名字，新建表时为 null；`draft` 是完整的表结构（与 `/schemas/{schema}/tables` 返回的相同）；`renames` 为"新名 → 原名"；`rebuild` 为用户手动选择重建；`fingerprint` 为打开时读到的指纹，执行前核对。
+- `errors`：`[{section, row, field, message}]`，`section` 为 `table` / `columns` / `primary_key` / `indexes` / `foreign_keys` / `uniques` / `checks`，`row` 是该部分中的序号（不含标为删除的行）。
+- `changes`：变更列表，每项 `{kind, …}`，`kind` 为 `create_table`、`set_table_comment`、`add_column`、`drop_column`、`rename_column`、`alter_column`、`move_column`、`add_*` / `drop_*`（`primary_key`、`unique`、`check`、`foreign_key`、`index`）、`rename_index`。
+- `statements`：`[{kind: "sql", sql, changes} | {kind: "count_check", source, target, changes}]`；`count_check` 是重建时由程序执行的核对行数。
+- 任务失败时 `remaining` 为没有执行的语句（以 `;` 分隔），PG 家族整体回滚时为 null。
+
 ## WebSocket：`GET /api/sessions/{id}/ws?token=…&lang=…`
 
 `lang` 可选，决定事件中文字的语言（见"语言"一节）。
```

- [ ] **Step 2: 新建 `docs/notes/m3-table-designer.md`**

````markdown
# M3 表设计器

依据：设计文档 `docs/superpowers/specs/2026-10-27-m3-table-designer-design.md`。

## 结构

```text
crates/dblens-dialect/src/design/
  mod.rs       Renames、TableChange、Diff、DesignIssue；Dialect::prepare_draft（重新解析逻辑类型、主键列 NOT NULL）
  diff.rs      validate（校验草稿）、diff_table（比对出变更列表）
  catalog.rs   design_caps：类型目录和方言能力
  sql.rs       design_unsupported、design_sql（按家族分派）、design_transactional；共用的外键、检查约束、索引列写法
  mysql.rs / postgres.rs / oracle.rs（含达梦）   各家族的 DDL
  rebuild.rs   重建表的脚本（RebuildContext → RebuildOutcome）
crates/dblens-dialect/sql/*.sql   新增 referencing_keys、dependent_views、grants、table_storage、trigger_ddl（只有 Oracle）
crates/dblens-service/src/design.rs   design_caps / design_table / preview_design / start_design；执行任务（Run）
crates/dblens-api/src/design.rs       /profiles/{id}/design/{caps,table,preview,apply}
web/src/features/design/
  logic.ts          草稿（每行 key、origin、deleted）、改名跟踪、类型的拆分与拼接、改动标记、校验错误换回草稿中的行
  designStore.ts    每个设计器标签的状态
  summary.ts        变更摘要、脚本文本
  DesignView.tsx    标签页：工具栏、子标签、自动预览、执行进度
  ColumnsPane / IndexesPane / ForeignKeysPane / ConstraintsPane / OptionsPane / ListPane / SaveDialog / SqlBox
```

入口：对象树表的右键"设计表"、schema 的右键"新建表"；数据标签工具栏"设计表"；标题栏"文件 → 新建表"。

## 与设计文档不一致的地方

- `Renames` 只有列和索引：约束只改名也按先删后建处理，不需要映射。
- 库不支持的改动不放在 `Diff` 里，由 `Dialect::design_unsupported(&Diff)` 判断；`Diff` 另有 `appended`（新列都在最后、保留的列相对顺序不变），不能指定列位置的库据此判断是否要重建。
- MySQL 家族每项改动一条 `ALTER TABLE`，不合成一条：同一条 ALTER 中改名与 MODIFY、AFTER 引用的是改名前还是改名后的列，各版本行为不一；MySQL 8 的多数列操作是 INSTANT / INPLACE，分开执行的代价不大。例外：换主键时删除和新增放在同一条（自增列必须是键，单独删主键报 1075）；既修改又挪动的列只在挪动时 MODIFY 一次。
- 一条生成的语句对应的变更序号是列表（`changes`）。
- 变更的可读摘要由前端按 `changes` 生成（随界面语言），接口不返回文字摘要。
- 没有"只读连接"这个概念：预览中只标出被执行策略拒绝的语句（`denied`）和需要确认的原因（`confirm`）；生产环境的横幅由前端按连接的环境显示。
- 不能指定列位置的库，"新增列"加在最后（插在中间就要重建表）；仍可以挪动，挪动后保存时重建。
- 索引方法不写与默认方法（BTREE、NORMAL）视为相同（实库中读回的索引都带方法）。
- Oracle 的 `triggers` 只有触发器体，重建时另用 `trigger_ddl` 由 `ALL_TRIGGERS.DESCRIPTION` 拼出完整的 CREATE TRIGGER；执行前把当前 schema 切到表所在的 schema（MySQL、Oracle 的触发器 DDL 中表名不带 schema）。
- 达梦的 `ALL_TAB_PRIVS` 用 `OWNER` 列（Oracle 是 `TABLE_SCHEMA`）。
- 重建：MySQL 的新表带主键（自增列必须是键），其他库在改回原名之后再建主键（主键索引名在 schema 内不能重复）；表和列的注释建在临时表上，改名后保留。
- 改动标记按行显示（新增绿、修改黄、删除加删除线），没有逐个单元格标黄；选中行的原生类型在属性面板中改过时标色。
- 标题栏"文件 → 新建表"的 schema：当前是数据标签或设计器标签时用它的 schema，否则在"选项"中选。
- 执行的每条语句各记一条执行历史。

## 验证

- 后端：fmt、clippy 通过；全部测试在 `DBLENS_LANG=en` 下通过。
  - dialect：比对、校验、各家族每种变更的快照、版本差异（MySQL 5.6 的 CHANGE、MariaDB 的 DROP CONSTRAINT）、生成的每条语句经拆分仍是一条、重建脚本快照（四个家族）、拒绝重建、临时表名截断。
  - service（假驱动）：新建表的预览和执行（一个事务、执行历史）；重建在删除旧表之前失败时删除新表、之后失败时列出没有执行的语句。
  - 实库（5 个库）：用设计器新建表；改名并加长、改可空/默认值/注释、换索引、加列后读回与草稿比对无差异；原表被别人改过时拒绝执行；强制重建（调整列顺序、VARCHAR 改为数值类型）后数据、约束、索引、注释、触发器、其他表指向本表的外键、自增接续、授权（PG、Oracle、达梦授予 PUBLIC）、依赖视图（MySQL 按名字、Oracle 和达梦重新编译；PG 有依赖视图时拒绝）都正确；中途失败时 PG 全部回滚、其他库保留已执行的。
  - 达梦的测试与其他测试同时连库时偶发连接失败，单独串行运行通过（与以前的记录相同）。
- 前端：tsc、Vitest、打包通过。
- 浏览器实测（中文、英文界面）：PG 上从 schema 右键新建表（类型下拉与长度、小数位的启用规则，主键、自增、注释），保存后标签转为编辑这张表；改名加挪动列触发重建（确认框中重建开关强制打开），执行后列顺序正确；未保存时关闭标签的提示；数据标签的"设计表"按钮切到已打开的设计器；MySQL 上新建带降序索引和检查约束的表并读回核对；Oracle 生产环境连接的确认框横幅和红色执行按钮；"在新查询标签中打开"。
- 未验证：重建执行中途的取消只有单元测试覆盖（与失败走同一条清理路径）；桌面程序中的设计器没有单独实测。
````

- [ ] **Step 3: 修改 `docs/superpowers/specs/2026-10-22-dblens-design.md`**

```diff
--- a/docs/superpowers/specs/2026-10-22-dblens-design.md
+++ b/docs/superpowers/specs/2026-10-22-dblens-design.md
@@ -327,6 +327,8 @@ trait Policy: Send + Sync {
 - 2026-10-26 M2 拆成两份（用户确认）：M2a 表数据编辑；M2b 导入（CSV / Excel 导入到表、SQL 文件导入）和对象树"导出整表"的入口。查询结果和数据标签的导出已在 M1c3 完成。
 - M2a 已完成（设计 `docs/superpowers/specs/2026-10-26-m2a-data-editing-design.md`，详细计划 `docs/superpowers/plans/2026-10-26-m2a-data-editing.md`，说明 `docs/notes/m2a-data-editing.md`）。
 - M2b 已完成（设计 `docs/superpowers/specs/2026-10-27-m2b-import-design.md`，详细计划 `docs/superpowers/plans/2026-10-27-m2b-import.md`，说明 `docs/notes/m2b-import.md`），M2 完成。
+- M3 已完成（设计 `docs/superpowers/specs/2026-10-27-m3-table-designer-design.md`，详细计划 `docs/superpowers/plans/2026-10-27-m3-table-designer.md`，说明 `docs/notes/m3-table-designer.md`）。
+- 2026-10-27 用户决定：国产库适配（OceanBase、人大金仓、PolarDB、TiDB、TDSQL、GaussDB、GoldenDB、GBase）作为 M3 之后的单独里程碑，分三批：有 Docker 或免费版的；只有云服务的；需要新驱动或新架构的（GBase 8s、OceanBase 的 Oracle 模式）。
 
 ## 12. 风险与待验证项
 
```

- [ ] **Step 4: 检查**

检查文档中的路径和接口与代码一致。

Expected: 无。

- [ ] **Step 5: 提交**

```bash
git add docs/notes/m0-4b-api.md docs/notes/m3-table-designer.md docs/superpowers/specs/2026-10-22-dblens-design.md
git commit -m "docs: M3 表设计器的说明文档、接口说明与进度" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```
