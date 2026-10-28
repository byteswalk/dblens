# M4c 文档导出 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 从模型或连接的 schema 导出数据字典和设计文档（docx / html / markdown，数据字典另有 xlsx），html、markdown 可换模板，docx 可指定样式来源；设计文档带 ER 图图片。

**Architecture:** 服务层把模型或库的结构整理成统一的文档内容（`DocContent`），四种格式都从它写出：html、markdown 用 minijinja 模板，docx 由程序生成正文放进样式来源文档，xlsx 用 rust_xlsxwriter；导出作为后台任务进行。前端在屏幕外用 M4b 的画法把 ER 图画成 PNG 上传，再开始任务。

**Tech Stack:** Rust（axum、serde、minijinja、zip、quick-xml、rust_xlsxwriter、insta）；React 19 + TypeScript、antd 6、@xyflow/react、html-to-image、zustand、TanStack Query、Vitest。

**Spec:** `docs/superpowers/specs/2026-10-28-m4c-document-export-design.md`

## Global Constraints

- 来源：模型和库的 schema 都能导出；库直接读结构，不必先另存为模型。
- 文档种类：数据字典（表清单和表明细）；设计文档（再加概述、ER 图、表间关系、视图）。
- 格式：docx、html、markdown；xlsx 只用于数据字典。
- 模板：html、markdown 用 minijinja 模板文件，内置一套、可另存后修改；docx 由程序生成内容，可指定 .docx 作样式来源。
- 本机路径（模板、样式来源、输出位置）只在页面和服务在同一台机器上（`local_files`）时接受；浏览器模式用内置模板，完成后下载。
- 依赖只用正式发布版，跟随最新；新版发布满 7 天再升级（有意固定版本的记入 `scripts/check-deps.mjs` 的 `HOLD`）。
- 前端给用户看的文字都用词条（中英文），英文区分单复数；后端错误文字用 `msg!` 词条（`src` 中的中文字面量只能出现在 `#[cfg(test)]` 之后）；文档中的固定文字放在 `document/labels.json`；日志不翻译。
- 代码注释用中文；文件 UTF-8、LF 换行；前端代码风格：无分号、单引号、两格缩进。
- cargo 命令加 `--config "source.mirror.registry='sparse+https://rsproxy.cn/index/'"`（clippy 写在 `clippy` 之后），并设 `CARGO_INCREMENTAL=0`；后端测试在 `DBLENS_LANG=en` 下运行。
- 前端工具用 PowerShell 执行（`npm`/`npx` 在本机不可用，用 `pnpm exec`）。
- 测试数据库的密码只在命令行中以内联环境变量传入，不写进仓库、报告或提交信息。

## 与设计文档的差异（写计划时实测后调整）

- 内置模板 html、markdown 各一份（两种文档共用，按 `kind` 分支），不是四份。
- 模板的固定文字放在 `document/labels.json`（键 `doc.*`），不放进后端词条（词条检查要求每个键被 `msg!` 用到）。
- 生成列的表达式写在"默认值"一列，标记为"生成列"。
- docx 的目录包括标题 1、2（每张表一项），标题 1 前分页；补上的样式编号加 `DBLens` 前缀。
- `document` 模块在 `dblens_service` 中公开（`dblens_service::document::…`）。

## 执行说明

任务 2–8 的代码都已在临时副本中逐任务实现并通过检查，执行时按顺序重放。新文件给出全文，修改的文件给出 diff（基于上一个任务完成后的状态，可直接 `git apply`）。`Cargo.lock` 的变化由 cargo 生成（任务 4 要用 `cargo update -p zip --precise 9.0.5` 锁定）；`web/pnpm-lock.yaml` 没有变化。

写计划时已运行：全部后端测试（704 个）和前端测试（266 个）；11 个库的模型实库测试（含文档导出）；Word 打开 docx 并逐页核对；Excel 打开 xlsx；浏览器中模型导出 HTML 设计文档、库导出 xlsx 数据字典。

## 文件结构

```text
crates/dblens-service/src/document/{mod.rs, content.rs, labels.rs, labels.json, template.rs, templates/{document.html, document.md}, docx.rs, docx/*.xml, xlsx.rs, job.rs, snapshots/}（新增）
crates/dblens-service/src/{lib.rs, task.rs, model/{mod.rs, ops.rs}}, Cargo.toml, tests/live.rs
crates/dblens-api/src/{documents.rs（新增）, lib.rs}, tests/models.rs
crates/dblens-core/locales/app.json; Cargo.toml, Cargo.lock
web/src/features/document/{options.ts, options.test.ts, dialog.ts, ExportDocDialog.tsx, render.tsx, track.tsx}（新增）
web/src/features/er/{flow.ts, limits.ts}（新增）, {ErView.tsx, export.ts}
web/src/{shared/api.ts, app/Shell.tsx, features/model/ModelsPanel.tsx, features/tree/ObjectTree.tsx, locales/{zh-CN,en}/{document.json（新增）, er.json, model.json, tree.json}, locales/index.ts}
docs/notes/{m4c-document-export.md（新增）, m0-4b-api.md}; docs/superpowers/specs/2026-10-22-dblens-design.md
```

---

### Task 1: 依赖复查

按依赖原则（M1b2 设计文档 3.1 节）复查前端和后端的全部直接依赖。本里程碑在任务 4 新增两个后端依赖：`zip` 9.0.5（2026-10-17 发布，MIT；写计划时 9.1.0–9.1.3 发布不满 7 天）、`quick-xml` 0.42.0（2026-08-22 发布，MIT）；service 的 `minijinja` 开 `serde` 特性（任务 3）。

**Files:** 视复查结果而定（`web/package.json`、`Cargo.toml` 及锁文件）。

- [ ] **Step 1: 运行复查脚本**

本机经代理上网，Node 的 `fetch` 默认不读 `HTTPS_PROXY`，必须先设 `NODE_USE_ENV_PROXY=1`，否则全部查询失败，最后仍显示"0 个可升级"。

```powershell
$env:PATH = "D:\Environments\nvm4w\nodejs;$env:PATH"; $env:NODE_USE_ENV_PROXY = '1'; $env:HTTPS_PROXY = 'http://127.0.0.1:6789'; node scripts/check-deps.mjs
```

Expected：输出中没有"查询失败"（写计划时 84 个直接依赖都不用升级，`html-to-image` 在"有意不升级"名单中）。以执行当天的结果为准；`zip` 如有发布满 7 天的更新的 9.x，任务 4 中用新版本。

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

### Task 2: 服务层：文档内容的组织

- `dblens-service/src/document/content.rs`（新）：`DocKind::{Dictionary, Design}`、`DocInfo`、`DocContent { kind, info, stats, schemas, diagrams, relations, unreadable, view_definitions }` 及各部分（`DocSchema`、`DocTable`、`DocColumn`、`DocIndex`、`DocConstraint`、`DocKey`、`DocTrigger`、`DocReference`、`DocRelation`、`DocView`、`DocDiagram`、`DocUnreadable`、`DocStats`）；输入 `SourceSchema { name, tables: Vec<SourceTable>, views, errors }`、`SourceTable { table, logical_keys }`；`build(BuildOptions, sources, diagrams)`：表按名字排序，锚点按文档顺序 `t1`、`t2`……（视图 `v1`……）；外键、逻辑外键链接到文档中的表；被引用；设计文档的表间关系和视图；统计；`png_size`。
- `document/mod.rs`（新）；`lib.rs` 加 `pub mod document`（不从 crate 根重新导出）。
- 测试中的样例（两个 schema：客户、订单、订单明细、跨 schema 的地区；复合外键、逻辑外键、视图、读不出的表）是 `pub(crate)` 的，后面的任务复用。

**Files:**
- Create: `crates/dblens-service/src/document/content.rs`
- Create: `crates/dblens-service/src/document/mod.rs`
- Modify: `crates/dblens-service/src/lib.rs`

**Interfaces:**
- Consumes：M4a 的 `FileError`，M4b 的 `LogicalKey`；`dblens_core::schema::{Table, View, Column, ForeignKey}`。
- Produces（`dblens_service::document`）：上面的类型；`build(opts: BuildOptions, sources: Vec<SourceSchema>, diagrams: Vec<DocDiagram>) -> DocContent`、`png_size(bytes) -> Option<(u32, u32)>`；测试用的 `content::tests::{col, fk, st, sample, opts}`。

- [ ] **Step 1: 新建 `crates/dblens-service/src/document/content.rs`**

```rust
//! 文档的内容（M4c 设计文档第 4 节）：从模型或库的结构整理出来，四种格式都从它写出，模板拿到的也是它。

use std::collections::BTreeMap;

use dblens_core::schema::{Column, ForeignKey, Table, View};
use serde::{Deserialize, Serialize};

use crate::model::{FileError, LogicalKey};

/// 文档种类。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DocKind {
    /// 数据字典：表清单和表明细。
    Dictionary,
    /// 设计文档：再加概述、ER 图、表间关系、视图。
    Design,
}

/// 文档信息：标题、版本等由用户填写，生成时间和来源由程序给出。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocInfo {
    pub title: String,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub author: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    /// 生成时间（本地时间，`2026-10-28 10:00`）。
    #[serde(default)]
    pub generated: String,
    /// 来源：模型名和目录，或连接名和库名。
    #[serde(default)]
    pub source: String,
}

/// 文档的全部内容。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DocContent {
    pub kind: DocKind,
    pub info: DocInfo,
    pub stats: DocStats,
    pub schemas: Vec<DocSchema>,
    /// 设计文档的 ER 图。
    pub diagrams: Vec<DocDiagram>,
    /// 设计文档的表间关系。
    pub relations: Vec<DocRelation>,
    /// 模型中读不出的表。
    pub unreadable: Vec<DocUnreadable>,
    /// 设计文档中是否放进视图的定义。
    pub view_definitions: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct DocStats {
    pub schemas: usize,
    pub tables: usize,
    pub columns: usize,
    pub foreign_keys: usize,
    pub logical_keys: usize,
    pub views: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DocSchema {
    pub name: String,
    pub tables: Vec<DocTable>,
    pub views: Vec<DocView>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DocTable {
    pub schema: String,
    pub name: String,
    /// 文档内的锚点（`t1`、`t2`……），html 的 id、docx 的书签、xlsx 的定位都用它。
    pub anchor: String,
    pub comment: Option<String>,
    pub partition: Option<String>,
    pub columns: Vec<DocColumn>,
    pub indexes: Vec<DocIndex>,
    pub constraints: Vec<DocConstraint>,
    pub foreign_keys: Vec<DocKey>,
    pub logical_keys: Vec<DocKey>,
    pub triggers: Vec<DocTrigger>,
    pub referenced_by: Vec<DocReference>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DocColumn {
    pub ordinal: usize,
    pub name: String,
    pub data_type: String,
    pub nullable: bool,
    pub default: Option<String>,
    /// 标记：`pk`（主键）、`auto`（自增）、`generated`（生成列）；文字由 `t('doc.flag.<标记>')` 取。
    pub flags: Vec<&'static str>,
    pub comment: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DocIndex {
    pub name: String,
    /// 列，以 `, ` 分隔；降序的列后面加 ` DESC`。
    pub columns: String,
    pub unique: bool,
    pub method: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DocConstraint {
    /// `primary`、`unique`、`check`。
    pub kind: &'static str,
    pub name: Option<String>,
    /// 列（主键、唯一）或表达式（检查）。
    pub detail: String,
}

/// 外键或逻辑外键。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DocKey {
    pub name: Option<String>,
    pub columns: String,
    pub ref_schema: String,
    pub ref_table: String,
    /// 引用的表在文档中时的锚点。
    pub ref_anchor: Option<String>,
    pub ref_columns: String,
    pub on_delete: Option<String>,
    pub on_update: Option<String>,
    pub comment: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DocTrigger {
    pub name: String,
    pub timing: Option<String>,
    pub events: String,
}

/// 指向这张表的外键或逻辑外键。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DocReference {
    pub schema: String,
    pub table: String,
    pub anchor: Option<String>,
    pub columns: String,
    pub ref_columns: String,
    /// `foreign_key` 或 `logical_key`。
    pub kind: &'static str,
    pub name: Option<String>,
}

/// 表间关系（设计文档）。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DocRelation {
    pub from_schema: String,
    pub from_table: String,
    pub from_anchor: Option<String>,
    pub from_columns: String,
    pub to_schema: String,
    pub to_table: String,
    pub to_anchor: Option<String>,
    pub to_columns: String,
    pub kind: &'static str,
    pub name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DocView {
    pub name: String,
    pub anchor: String,
    pub comment: Option<String>,
    pub columns: Vec<DocColumn>,
    pub definition: Option<String>,
}

/// 一张 ER 图的图片（PNG）。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DocDiagram {
    pub schema: String,
    /// 图名（默认图为 `_all`）。
    pub name: String,
    /// 显示的标题：默认图为 schema 名，主题图为图名。
    pub title: String,
    pub description: Option<String>,
    /// 表较多、只画了表名。
    pub names_only: bool,
    /// 图片的文件名（`01-<schema>-<图名>.png`）。
    pub file: String,
    /// 图片的引用：html 为 data URL，markdown 为相对路径；docx、xlsx 不用。
    pub src: String,
    /// 像素尺寸。
    pub width: u32,
    pub height: u32,
    #[serde(skip)]
    pub png: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DocUnreadable {
    pub schema: String,
    pub name: String,
    pub path: String,
    pub message: String,
}

/// 一个 schema 的结构（组织内容的输入）。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SourceSchema {
    pub name: String,
    pub tables: Vec<SourceTable>,
    pub views: Vec<View>,
    /// 模型中读不出的表：表名和原因。
    pub errors: Vec<(String, FileError)>,
}

/// 一张表和它的逻辑外键（来源是库时没有逻辑外键）。
#[derive(Debug, Clone, PartialEq)]
pub struct SourceTable {
    pub table: Table,
    pub logical_keys: Vec<LogicalKey>,
}

/// 组织内容的选项。
#[derive(Debug, Clone, PartialEq)]
pub struct BuildOptions {
    pub kind: DocKind,
    pub info: DocInfo,
    /// 只放这些表（只选了一个 schema 时可用）；None 为全部。
    pub tables: Option<Vec<String>>,
    pub view_definitions: bool,
}

fn join(cols: &[String]) -> String {
    cols.join(", ")
}

fn column(c: &Column, ordinal: usize, pk: &[String]) -> DocColumn {
    let mut flags = Vec::new();
    if pk.contains(&c.name) {
        flags.push("pk");
    }
    if c.auto_increment {
        flags.push("auto");
    }
    if c.generated.is_some() {
        flags.push("generated");
    }
    DocColumn {
        ordinal,
        name: c.name.clone(),
        data_type: c.data_type.native.clone(),
        nullable: c.nullable,
        default: c.generated.clone().or_else(|| c.default.clone()),
        flags,
        comment: c.comment.clone(),
    }
}

/// 外键和逻辑外键共用的字段。
struct KeyRef<'a> {
    name: &'a Option<String>,
    columns: &'a [String],
    ref_schema: &'a Option<String>,
    ref_table: &'a str,
    ref_columns: &'a [String],
}

impl<'a> From<&'a ForeignKey> for KeyRef<'a> {
    fn from(f: &'a ForeignKey) -> Self {
        KeyRef {
            name: &f.name,
            columns: &f.columns,
            ref_schema: &f.ref_schema,
            ref_table: &f.ref_table,
            ref_columns: &f.ref_columns,
        }
    }
}

impl<'a> From<&'a LogicalKey> for KeyRef<'a> {
    fn from(k: &'a LogicalKey) -> Self {
        KeyRef {
            name: &k.name,
            columns: &k.columns,
            ref_schema: &k.ref_schema,
            ref_table: &k.ref_table,
            ref_columns: &k.ref_columns,
        }
    }
}

/// 组织文档内容。`diagrams` 是设计文档的 ER 图（数据字典时忽略）。
pub fn build(opts: BuildOptions, sources: Vec<SourceSchema>, diagrams: Vec<DocDiagram>) -> DocContent {
    let design = opts.kind == DocKind::Design;
    // 先定下全部表的锚点：外键、被引用可以链接到文档中的表
    let mut anchors: BTreeMap<(String, String), String> = BTreeMap::new();
    let mut picked: Vec<(String, Vec<&SourceTable>)> = Vec::new();
    for s in &sources {
        let mut tables: Vec<&SourceTable> = s
            .tables
            .iter()
            .filter(|t| opts.tables.as_ref().is_none_or(|names| names.contains(&t.table.name)))
            .collect();
        tables.sort_by(|a, b| a.table.name.cmp(&b.table.name));
        for SourceTable { table: t, .. } in &tables {
            let anchor = format!("t{}", anchors.len() + 1);
            anchors.insert((s.name.clone(), t.name.clone()), anchor);
        }
        picked.push((s.name.clone(), tables));
    }
    let anchor_of =
        |schema: &str, table: &str| anchors.get(&(schema.to_string(), table.to_string())).cloned();

    let key = |schema: &str, k: KeyRef, on_delete: Option<String>, on_update: Option<String>, comment| {
        let ref_schema = k.ref_schema.clone().unwrap_or_else(|| schema.to_string());
        DocKey {
            name: k.name.clone(),
            columns: join(k.columns),
            ref_anchor: anchor_of(&ref_schema, k.ref_table),
            ref_schema,
            ref_table: k.ref_table.to_string(),
            ref_columns: join(k.ref_columns),
            on_delete,
            on_update,
            comment,
        }
    };

    let mut stats = DocStats::default();
    let mut relations = Vec::new();
    // 被引用：(schema, 表) → 指向它的引用
    let mut referenced: BTreeMap<(String, String), Vec<DocReference>> = BTreeMap::new();
    let mut schemas = Vec::new();
    let mut view_count = 0;
    for ((schema, tables), source) in picked.into_iter().zip(&sources) {
        let mut out = Vec::new();
        for SourceTable { table: t, logical_keys: logical } in tables {
            let pk = t.primary_key.as_ref().map(|p| p.columns.clone()).unwrap_or_default();
            let mut constraints = Vec::new();
            if let Some(p) = &t.primary_key {
                constraints.push(DocConstraint {
                    kind: "primary",
                    name: p.name.clone(),
                    detail: join(&p.columns),
                });
            }
            for u in &t.uniques {
                constraints.push(DocConstraint {
                    kind: "unique",
                    name: u.name.clone(),
                    detail: join(&u.columns),
                });
            }
            for c in &t.checks {
                constraints.push(DocConstraint {
                    kind: "check",
                    name: c.name.clone(),
                    detail: c.expression.clone(),
                });
            }
            let foreign_keys: Vec<DocKey> = t
                .foreign_keys
                .iter()
                .map(|f| key(&schema, f.into(), f.on_delete.clone(), f.on_update.clone(), None))
                .collect();
            let logical_keys: Vec<DocKey> =
                logical.iter().map(|k| key(&schema, k.into(), None, None, k.comment.clone())).collect();
            let anchor = anchor_of(&schema, &t.name);
            for (keys, kind) in [(&foreign_keys, "foreign_key"), (&logical_keys, "logical_key")] {
                for k in keys {
                    referenced.entry((k.ref_schema.clone(), k.ref_table.clone())).or_default().push(
                        DocReference {
                            schema: schema.clone(),
                            table: t.name.clone(),
                            anchor: anchor.clone(),
                            columns: k.columns.clone(),
                            ref_columns: k.ref_columns.clone(),
                            kind,
                            name: k.name.clone(),
                        },
                    );
                    if design {
                        relations.push(DocRelation {
                            from_schema: schema.clone(),
                            from_table: t.name.clone(),
                            from_anchor: anchor.clone(),
                            from_columns: k.columns.clone(),
                            to_schema: k.ref_schema.clone(),
                            to_table: k.ref_table.clone(),
                            to_anchor: k.ref_anchor.clone(),
                            to_columns: k.ref_columns.clone(),
                            kind,
                            name: k.name.clone(),
                        });
                    }
                }
            }
            stats.tables += 1;
            stats.columns += t.columns.len();
            stats.foreign_keys += foreign_keys.len();
            stats.logical_keys += logical_keys.len();
            out.push(DocTable {
                schema: schema.clone(),
                name: t.name.clone(),
                anchor: anchor.unwrap_or_default(),
                comment: t.comment.clone(),
                partition: t.partition.clone(),
                columns: t.columns.iter().enumerate().map(|(i, c)| column(c, i + 1, &pk)).collect(),
                indexes: t
                    .indexes
                    .iter()
                    .map(|i| DocIndex {
                        name: i.name.clone(),
                        columns: i
                            .columns
                            .iter()
                            .map(|c| if c.descending { format!("{} DESC", c.name) } else { c.name.clone() })
                            .collect::<Vec<_>>()
                            .join(", "),
                        unique: i.unique,
                        method: i.method.clone(),
                    })
                    .collect(),
                constraints,
                foreign_keys,
                logical_keys,
                triggers: t
                    .triggers
                    .iter()
                    .map(|g| DocTrigger {
                        name: g.name.clone(),
                        timing: g.timing.clone(),
                        events: g.events.join(", "),
                    })
                    .collect(),
                referenced_by: vec![],
            });
        }
        let views: Vec<DocView> = if design {
            let mut vs: Vec<&View> = source.views.iter().collect();
            vs.sort_by(|a, b| a.name.cmp(&b.name));
            vs.into_iter()
                .map(|v| {
                    view_count += 1;
                    DocView {
                        name: v.name.clone(),
                        anchor: format!("v{view_count}"),
                        comment: v.comment.clone(),
                        columns: v.columns.iter().enumerate().map(|(i, c)| column(c, i + 1, &[])).collect(),
                        definition: if opts.view_definitions { v.definition.clone() } else { None },
                    }
                })
                .collect()
        } else {
            vec![]
        };
        stats.views += views.len();
        schemas.push(DocSchema { name: schema, tables: out, views });
    }
    for s in &mut schemas {
        for t in &mut s.tables {
            t.referenced_by = referenced.remove(&(t.schema.clone(), t.name.clone())).unwrap_or_default();
        }
    }
    stats.schemas = schemas.len();
    let unreadable = sources
        .iter()
        .flat_map(|s| {
            s.errors.iter().map(|(name, e)| DocUnreadable {
                schema: s.name.clone(),
                name: name.clone(),
                path: e.path.clone(),
                message: e.message.clone(),
            })
        })
        .collect();
    DocContent {
        kind: opts.kind,
        info: opts.info,
        stats,
        schemas,
        diagrams: if design { diagrams } else { vec![] },
        relations,
        unreadable,
        view_definitions: opts.view_definitions,
    }
}

/// PNG 的像素尺寸（读文件头）；不是 PNG 时为 None。
pub fn png_size(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.len() < 24 || &bytes[..8] != b"\x89PNG\r\n\x1a\n" || &bytes[12..16] != b"IHDR" {
        return None;
    }
    let w = u32::from_be_bytes(bytes[16..20].try_into().ok()?);
    let h = u32::from_be_bytes(bytes[20..24].try_into().ok()?);
    Some((w, h))
}

#[cfg(test)]
pub(crate) mod tests {
    use dblens_core::schema::{CheckConstraint, Index, IndexColumn, PrimaryKey, Trigger, UniqueConstraint};
    use dblens_core::{ColumnType, LogicalType};

    use super::*;

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

    pub fn fk(
        name: &str,
        cols: &[&str],
        ref_schema: Option<&str>,
        ref_table: &str,
        ref_cols: &[&str],
    ) -> ForeignKey {
        ForeignKey {
            name: Some(name.into()),
            columns: cols.iter().map(|c| c.to_string()).collect(),
            ref_schema: ref_schema.map(Into::into),
            ref_table: ref_table.into(),
            ref_columns: ref_cols.iter().map(|c| c.to_string()).collect(),
            on_delete: Some("CASCADE".into()),
            on_update: None,
        }
    }

    pub fn st(table: Table, logical_keys: Vec<LogicalKey>) -> SourceTable {
        SourceTable { table, logical_keys }
    }

    /// 测试用的两个 schema：shop（客户、订单、订单明细、地区引用 crm）和 crm（地区）。
    pub fn sample() -> Vec<SourceSchema> {
        let customers = Table {
            name: "customers".into(),
            comment: Some("客户".into()),
            columns: vec![
                Column { auto_increment: true, nullable: false, ..col("id", "int") },
                Column { nullable: false, comment: Some("编号".into()), ..col("code", "varchar(20)") },
                col("region_id", "int"),
            ],
            primary_key: Some(PrimaryKey { name: Some("pk_customers".into()), columns: vec!["id".into()] }),
            uniques: vec![UniqueConstraint { name: Some("uq_code".into()), columns: vec!["code".into()] }],
            foreign_keys: vec![fk("fk_region", &["region_id"], Some("crm"), "regions", &["id"])],
            indexes: vec![Index {
                name: "ix_code".into(),
                columns: vec![IndexColumn { name: "code".into(), descending: true }],
                unique: false,
                method: Some("BTREE".into()),
            }],
            ..Default::default()
        };
        let orders = Table {
            name: "orders".into(),
            columns: vec![
                Column { nullable: false, ..col("id", "int") },
                col("cust_id", "int"),
                col("cust_code", "varchar(20)"),
                Column { default: Some("0".into()), ..col("amount", "decimal(10,2)") },
                Column { generated: Some("amount * 2".into()), ..col("double_amount", "decimal(10,2)") },
            ],
            primary_key: Some(PrimaryKey { name: None, columns: vec!["id".into()] }),
            foreign_keys: vec![fk("fk_cust", &["cust_id", "cust_code"], None, "customers", &["id", "code"])],
            checks: vec![CheckConstraint {
                name: Some("ck_amount".into()),
                expression: "amount >= 0".into(),
            }],
            triggers: vec![Trigger {
                name: "trg_orders".into(),
                timing: Some("BEFORE".into()),
                events: vec!["INSERT".into(), "UPDATE".into()],
                definition: Some("BEGIN END".into()),
            }],
            ..Default::default()
        };
        let items = Table {
            name: "items".into(),
            columns: vec![col("id", "int"), col("order_id", "int"), col("sku", "varchar(30)")],
            ..Default::default()
        };
        let lk = LogicalKey {
            name: Some("lk_order".into()),
            columns: vec!["order_id".into()],
            ref_schema: None,
            ref_table: "orders".into(),
            ref_columns: vec!["id".into()],
            comment: Some("没有建外键".into()),
        };
        let regions = Table {
            name: "regions".into(),
            columns: vec![col("id", "int"), col("name", "varchar(50)")],
            primary_key: Some(PrimaryKey { name: None, columns: vec!["id".into()] }),
            ..Default::default()
        };
        vec![
            SourceSchema {
                name: "shop".into(),
                tables: vec![st(orders, vec![]), st(customers, vec![]), st(items, vec![lk])],
                views: vec![View {
                    name: "v_orders".into(),
                    comment: Some("订单视图".into()),
                    definition: Some("SELECT id FROM orders".into()),
                    columns: vec![col("id", "int")],
                }],
                errors: vec![(
                    "broken".into(),
                    FileError { path: "tables/shop/broken.yaml".into(), message: "line 3: bad".into() },
                )],
            },
            SourceSchema { name: "crm".into(), tables: vec![st(regions, vec![])], ..Default::default() },
        ]
    }

    pub fn opts(kind: DocKind) -> BuildOptions {
        BuildOptions {
            kind,
            info: DocInfo {
                title: "商城".into(),
                version: Some("1.0".into()),
                author: Some("张三".into()),
                description: Some("测试模型".into()),
                generated: "2026-10-28 10:00".into(),
                source: "shop-model".into(),
            },
            tables: None,
            view_definitions: true,
        }
    }

    #[test]
    fn dictionary_tables_keys_and_references() {
        let c = build(opts(DocKind::Dictionary), sample(), vec![]);
        // 表按名字排序，锚点按文档中的顺序
        let names: Vec<_> =
            c.schemas[0].tables.iter().map(|t| (t.name.as_str(), t.anchor.as_str())).collect();
        assert_eq!(names, [("customers", "t1"), ("items", "t2"), ("orders", "t3")]);
        assert_eq!(c.schemas[1].tables[0].anchor, "t4");
        let cust = &c.schemas[0].tables[0];
        assert_eq!(cust.columns[0].flags, ["pk", "auto"]);
        assert_eq!(cust.indexes[0].columns, "code DESC");
        assert_eq!(
            cust.constraints.iter().map(|k| (k.kind, k.detail.as_str())).collect::<Vec<_>>(),
            [("primary", "id"), ("unique", "code")]
        );
        // 跨 schema 的外键链接到 crm.regions
        assert_eq!(cust.foreign_keys[0].ref_schema, "crm");
        assert_eq!(cust.foreign_keys[0].ref_anchor.as_deref(), Some("t4"));
        // 复合外键
        let orders = &c.schemas[0].tables[2];
        assert_eq!(orders.foreign_keys[0].columns, "cust_id, cust_code");
        assert_eq!(orders.foreign_keys[0].ref_columns, "id, code");
        assert_eq!(orders.columns[4].flags, ["generated"]);
        assert_eq!(orders.columns[4].default.as_deref(), Some("amount * 2"));
        assert_eq!(orders.triggers[0].events, "INSERT, UPDATE");
        // 被引用：订单被明细的逻辑外键引用，客户被订单的外键引用
        assert_eq!(orders.referenced_by.len(), 1);
        assert_eq!(orders.referenced_by[0].kind, "logical_key");
        assert_eq!(orders.referenced_by[0].anchor.as_deref(), Some("t2"));
        assert_eq!(cust.referenced_by[0].table, "orders");
        assert_eq!(c.schemas[1].tables[0].referenced_by[0].schema, "shop");
        // 数据字典没有视图、关系、图
        assert!(c.schemas[0].views.is_empty() && c.relations.is_empty());
        assert_eq!(c.unreadable[0].name, "broken");
        assert_eq!(
            c.stats,
            DocStats { schemas: 2, tables: 4, columns: 13, foreign_keys: 2, logical_keys: 1, views: 0 }
        );
    }

    #[test]
    fn design_relations_views_and_scope() {
        let c = build(opts(DocKind::Design), sample(), vec![]);
        assert_eq!(c.relations.len(), 3);
        assert_eq!(c.relations.iter().filter(|r| r.kind == "logical_key").count(), 1);
        assert_eq!(c.schemas[0].views[0].anchor, "v1");
        assert_eq!(c.schemas[0].views[0].definition.as_deref(), Some("SELECT id FROM orders"));
        assert_eq!(c.stats.views, 1);
        let mut o = opts(DocKind::Design);
        o.view_definitions = false;
        o.tables = Some(vec!["orders".into(), "items".into()]);
        let c = build(o, sample()[..1].to_vec(), vec![]);
        assert_eq!(c.schemas[0].views[0].definition, None);
        let names: Vec<_> = c.schemas[0].tables.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, ["items", "orders"]);
        // 引用文档之外的表时没有锚点
        assert_eq!(c.schemas[0].tables[1].foreign_keys[0].ref_anchor, None);
    }

    #[test]
    fn reads_png_size() {
        let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
        png.extend(640u32.to_be_bytes());
        png.extend(480u32.to_be_bytes());
        assert_eq!(png_size(&png), Some((640, 480)));
        assert_eq!(png_size(b"GIF89a"), None);
    }
}
```

- [ ] **Step 2: 新建 `crates/dblens-service/src/document/mod.rs`**

```rust
//! 文档导出（M4c 设计文档）：数据字典和设计文档，docx / html / markdown / xlsx。

mod content;

pub use content::{
    BuildOptions, DocColumn, DocConstraint, DocContent, DocDiagram, DocIndex, DocInfo, DocKey, DocKind,
    DocReference, DocRelation, DocSchema, DocStats, DocTable, DocTrigger, DocUnreadable, DocView,
    SourceSchema, SourceTable, build, png_size,
};
```

- [ ] **Step 3: 修改 `crates/dblens-service/src/lib.rs`**

```diff
--- a/crates/dblens-service/src/lib.rs
+++ b/crates/dblens-service/src/lib.rs
@@ -3,6 +3,8 @@
 mod changes;
 mod compare;
 mod design;
+/// 文档导出（M4c）。
+pub mod document;
 mod export;
 /// 测试用的假驱动，供本 crate 和 `dblens-api` 的测试使用（feature `testing`）。
 #[cfg(any(test, feature = "testing"))]
```

- [ ] **Step 4: 检查**

```bash
export CARGO_INCREMENTAL=0
cargo fmt --all
cargo clippy --config "source.mirror.registry='sparse+https://rsproxy.cn/index/'" --workspace --all-targets -- -D warnings
DBLENS_LANG=en cargo test --config "source.mirror.registry='sparse+https://rsproxy.cn/index/'" --workspace
```

Expected: 全部测试通过（service 新增 3 个）。

- [ ] **Step 5: 提交**

```bash
git add crates/dblens-service/src/document/content.rs crates/dblens-service/src/document/mod.rs crates/dblens-service/src/lib.rs
git commit -m "feat(document): 文档内容的组织" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 3: 服务层：html、markdown 模板

- `document/labels.json`、`labels.rs`（新）：文档中的固定文字（键 `doc.*`，中英文），`label(lang, key)`。
- `document/templates/document.html`、`document.md`（新）：内置模板，两种文档共用、按 `kind` 分支；html 自带打印样式、目录、表名互相链接，图片用 data URL（`|safe`）；markdown 用 GFM 表格、图片相对路径。
- `document/template.rs`（新）：`TextFormat::{Html, Markdown}`、`builtin()`；minijinja 环境（trim/lstrip blocks、按扩展名决定 HTML 转义、函数 `t`、过滤器 `v`（空值写成空）和 `cell`（markdown 单元格转义））；没有加载器，`include` 找不到其他文件；`check` 检查语法、`render` 写出（html 的图片转成 data URL，markdown 为 `<image_dir>/<文件>`）；出错时报行号。
- 测试检查模板中用到的 `doc.*` 键都存在、`labels.json` 中没有用不到的键（`'doc.kind.' ~ 变量` 这类拼出来的键按前缀算）。
- 后端词条加 `document.template_error`、`document.write_failed`；service 的 `minijinja` 开 `serde` 特性。

快照文件（`snapshots/`）随本任务给出；改了生成逻辑时用 `INSTA_UPDATE=always` 重新生成并逐个审查差异。

**Files:**
- Modify: `crates/dblens-core/locales/app.json`
- Modify: `crates/dblens-service/Cargo.toml`
- Create: `crates/dblens-service/src/document/labels.json`
- Create: `crates/dblens-service/src/document/labels.rs`
- Modify: `crates/dblens-service/src/document/mod.rs`
- Create: `crates/dblens-service/src/document/snapshots/dblens_service__document__template__tests__design_html_zh.snap`
- Create: `crates/dblens-service/src/document/snapshots/dblens_service__document__template__tests__design_md_en.snap`
- Create: `crates/dblens-service/src/document/snapshots/dblens_service__document__template__tests__dictionary_md_zh.snap`
- Create: `crates/dblens-service/src/document/template.rs`
- Create: `crates/dblens-service/src/document/templates/document.html`
- Create: `crates/dblens-service/src/document/templates/document.md`

**Interfaces:**
- Consumes：任务 2 的 `DocContent`、`build` 和测试样例。
- Produces：`TextFormat`、`check(format, source) -> Result<()>`、`render(content, format, template: Option<&str>, lang, image_dir) -> Result<String>`、`label(lang, key) -> String`。

- [ ] **Step 1: 修改 `crates/dblens-core/locales/app.json`**

```diff
--- a/crates/dblens-core/locales/app.json
+++ b/crates/dblens-core/locales/app.json
@@ -61,6 +61,8 @@
   "dm.no_diagnostics": {"zh-CN": "调用 DPI 失败，且无法读取错误信息", "en": "A DPI call failed and its error information could not be read"},
   "dm.nul_in_string": {"zh-CN": "字符串中不能包含 \\0 字符", "en": "Strings cannot contain the \\0 character"},
   "dml.no_insertable_columns": {"zh-CN": "没有可插入的列：%{table}", "en": "No insertable columns: %{table}"},
+  "document.template_error": {"zh-CN": "模板第 %{line} 行有错：%{error}", "en": "Template error at line %{line}: %{error}"},
+  "document.write_failed": {"zh-CN": "写文档失败：%{error}", "en": "Failed to write the document: %{error}"},
   "driver.connect_timeout": {"zh-CN": "连接超时（%{seconds} 秒）", "en": "Connection timed out (%{seconds} s)"},
   "driver.connection_closed": {"zh-CN": "连接已断开：%{error}", "en": "Connection closed: %{error}"},
   "driver.lob_read_failed": {"zh-CN": "读取 LOB 失败：%{error}", "en": "Failed to read LOB: %{error}"},
```

- [ ] **Step 2: 修改 `crates/dblens-service/Cargo.toml`**

```diff
--- a/crates/dblens-service/Cargo.toml
+++ b/crates/dblens-service/Cargo.toml
@@ -33,6 +33,7 @@ directories.workspace = true
 encoding_rs.workspace = true
 encoding_rs_io.workspace = true
 keyring.workspace = true
+minijinja = { workspace = true, features = ["serde"] }
 rusqlite.workspace = true
 rust_xlsxwriter.workspace = true
 serde.workspace = true
```

- [ ] **Step 3: 新建 `crates/dblens-service/src/document/labels.json`**

```json
{
  "doc.dictionary": {
    "zh-CN": "数据字典",
    "en": "Data Dictionary"
  },
  "doc.design": {
    "zh-CN": "设计文档",
    "en": "Design Document"
  },
  "doc.version": {
    "zh-CN": "版本",
    "en": "Version"
  },
  "doc.author": {
    "zh-CN": "作者",
    "en": "Author"
  },
  "doc.generated": {
    "zh-CN": "生成时间",
    "en": "Generated"
  },
  "doc.source": {
    "zh-CN": "来源",
    "en": "Source"
  },
  "doc.contents": {
    "zh-CN": "目录",
    "en": "Contents"
  },
  "doc.overview": {
    "zh-CN": "概述",
    "en": "Overview"
  },
  "doc.stats": {
    "zh-CN": "统计",
    "en": "Statistics"
  },
  "doc.stat.schemas": {
    "zh-CN": "Schema 数",
    "en": "Schemas"
  },
  "doc.stat.tables": {
    "zh-CN": "表数",
    "en": "Tables"
  },
  "doc.stat.columns": {
    "zh-CN": "列数",
    "en": "Columns"
  },
  "doc.stat.foreign_keys": {
    "zh-CN": "外键数",
    "en": "Foreign keys"
  },
  "doc.stat.logical_keys": {
    "zh-CN": "逻辑外键数",
    "en": "Logical keys"
  },
  "doc.stat.views": {
    "zh-CN": "视图数",
    "en": "Views"
  },
  "doc.diagrams": {
    "zh-CN": "ER 图",
    "en": "ER Diagrams"
  },
  "doc.names_only": {
    "zh-CN": "表较多，图中只显示表名；明细见各表。",
    "en": "This diagram has many tables, so only table names are shown; see each table for details."
  },
  "doc.relations": {
    "zh-CN": "表间关系",
    "en": "Relationships"
  },
  "doc.tables": {
    "zh-CN": "表清单",
    "en": "Tables"
  },
  "doc.views": {
    "zh-CN": "视图",
    "en": "Views"
  },
  "doc.details": {
    "zh-CN": "表明细",
    "en": "Table Details"
  },
  "doc.unreadable": {
    "zh-CN": "无法读取的表",
    "en": "Unreadable Tables"
  },
  "doc.partition": {
    "zh-CN": "分区",
    "en": "Partition"
  },
  "doc.col.no": {
    "zh-CN": "序号",
    "en": "No."
  },
  "doc.col.schema": {
    "zh-CN": "Schema",
    "en": "Schema"
  },
  "doc.col.table": {
    "zh-CN": "表名",
    "en": "Table"
  },
  "doc.col.name": {
    "zh-CN": "名称",
    "en": "Name"
  },
  "doc.col.comment": {
    "zh-CN": "注释",
    "en": "Comment"
  },
  "doc.col.columns": {
    "zh-CN": "列数",
    "en": "Columns"
  },
  "doc.col.column": {
    "zh-CN": "列名",
    "en": "Column"
  },
  "doc.col.type": {
    "zh-CN": "类型",
    "en": "Type"
  },
  "doc.col.nullable": {
    "zh-CN": "可空",
    "en": "Nullable"
  },
  "doc.col.default": {
    "zh-CN": "默认值",
    "en": "Default"
  },
  "doc.col.flags": {
    "zh-CN": "标记",
    "en": "Flags"
  },
  "doc.col.index_columns": {
    "zh-CN": "列",
    "en": "Columns"
  },
  "doc.col.unique": {
    "zh-CN": "唯一",
    "en": "Unique"
  },
  "doc.col.method": {
    "zh-CN": "方法",
    "en": "Method"
  },
  "doc.col.kind": {
    "zh-CN": "类别",
    "en": "Kind"
  },
  "doc.col.detail": {
    "zh-CN": "内容",
    "en": "Detail"
  },
  "doc.col.references": {
    "zh-CN": "引用",
    "en": "References"
  },
  "doc.col.on_delete": {
    "zh-CN": "删除时",
    "en": "On delete"
  },
  "doc.col.on_update": {
    "zh-CN": "更新时",
    "en": "On update"
  },
  "doc.col.timing": {
    "zh-CN": "时机",
    "en": "Timing"
  },
  "doc.col.events": {
    "zh-CN": "事件",
    "en": "Events"
  },
  "doc.col.from": {
    "zh-CN": "从",
    "en": "From"
  },
  "doc.col.to": {
    "zh-CN": "到",
    "en": "To"
  },
  "doc.col.file": {
    "zh-CN": "文件",
    "en": "File"
  },
  "doc.col.error": {
    "zh-CN": "原因",
    "en": "Reason"
  },
  "doc.sec.indexes": {
    "zh-CN": "索引",
    "en": "Indexes"
  },
  "doc.sec.constraints": {
    "zh-CN": "约束",
    "en": "Constraints"
  },
  "doc.sec.foreign_keys": {
    "zh-CN": "外键",
    "en": "Foreign Keys"
  },
  "doc.sec.logical_keys": {
    "zh-CN": "逻辑外键",
    "en": "Logical Keys"
  },
  "doc.sec.triggers": {
    "zh-CN": "触发器",
    "en": "Triggers"
  },
  "doc.sec.referenced_by": {
    "zh-CN": "被引用",
    "en": "Referenced By"
  },
  "doc.sec.definition": {
    "zh-CN": "定义",
    "en": "Definition"
  },
  "doc.flag.pk": {
    "zh-CN": "主键",
    "en": "PK"
  },
  "doc.flag.auto": {
    "zh-CN": "自增",
    "en": "Auto"
  },
  "doc.flag.generated": {
    "zh-CN": "生成列",
    "en": "Generated"
  },
  "doc.kind.primary": {
    "zh-CN": "主键",
    "en": "Primary key"
  },
  "doc.kind.unique": {
    "zh-CN": "唯一",
    "en": "Unique"
  },
  "doc.kind.check": {
    "zh-CN": "检查",
    "en": "Check"
  },
  "doc.kind.index": {
    "zh-CN": "索引",
    "en": "Index"
  },
  "doc.kind.foreign_key": {
    "zh-CN": "外键",
    "en": "Foreign key"
  },
  "doc.kind.logical_key": {
    "zh-CN": "逻辑外键",
    "en": "Logical key"
  },
  "doc.yes": {
    "zh-CN": "是",
    "en": "Yes"
  },
  "doc.no": {
    "zh-CN": "否",
    "en": "No"
  }
}
```

- [ ] **Step 4: 新建 `crates/dblens-service/src/document/labels.rs`**

```rust
//! 文档中的固定文字（`labels.json`）：模板用 `t('键')` 取，docx、xlsx 的写法用 `label`。
//! 与后端词条分开放：模板可由用户修改，这些键不经过 `msg!`。

use std::collections::HashMap;
use std::sync::OnceLock;

use dblens_core::Lang;

type Labels = HashMap<String, HashMap<String, String>>;

fn labels() -> &'static Labels {
    static LABELS: OnceLock<Labels> = OnceLock::new();
    LABELS.get_or_init(|| serde_json::from_str(include_str!("labels.json")).unwrap_or_default())
}

/// 按语言取固定文字；没有这个键时原样返回键名。
pub fn label(lang: Lang, key: &str) -> String {
    labels().get(key).and_then(|m| m.get(lang.code())).cloned().unwrap_or_else(|| key.to_string())
}

#[cfg(test)]
pub(crate) fn keys() -> Vec<String> {
    labels().keys().cloned().collect()
}
```

- [ ] **Step 5: 修改 `crates/dblens-service/src/document/mod.rs`**

```diff
--- a/crates/dblens-service/src/document/mod.rs
+++ b/crates/dblens-service/src/document/mod.rs
@@ -1,9 +1,13 @@
 //! 文档导出（M4c 设计文档）：数据字典和设计文档，docx / html / markdown / xlsx。
 
 mod content;
+mod labels;
+mod template;
 
 pub use content::{
     BuildOptions, DocColumn, DocConstraint, DocContent, DocDiagram, DocIndex, DocInfo, DocKey, DocKind,
     DocReference, DocRelation, DocSchema, DocStats, DocTable, DocTrigger, DocUnreadable, DocView,
     SourceSchema, SourceTable, build, png_size,
 };
+pub use labels::label;
+pub use template::{TextFormat, check, render};
```

- [ ] **Step 6: 新建 `crates/dblens-service/src/document/snapshots/dblens_service__document__template__tests__design_html_zh.snap`**

```text
---
source: crates/dblens-service/src/document/template.rs
expression: html
---
<!doctype html>
<html lang="zh-CN">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<meta name="generator" content="DBLens">
<title>&lt;商城&gt; &amp; 订单</title>
<style>
body { font: 14px/1.6 -apple-system, "Segoe UI", "PingFang SC", "Microsoft YaHei", sans-serif; color: #1f1f23; margin: 0 auto; max-width: 1100px; padding: 24px 32px; }
h1 { font-size: 26px; margin: 0 0 4px; }
h2 { font-size: 20px; border-bottom: 1px solid #ddd; padding-bottom: 4px; margin-top: 36px; }
h3 { font-size: 16px; margin-top: 28px; }
h4 { font-size: 14px; margin: 16px 0 6px; color: #444; }
.kind { color: #666; margin-bottom: 12px; }
.meta td:first-child { color: #666; padding-right: 16px; }
table { border-collapse: collapse; width: 100%; margin: 6px 0 12px; }
th, td { border: 1px solid #d9d9de; padding: 4px 8px; text-align: left; vertical-align: top; }
th { background: #f4f4f6; font-weight: 600; }
thead { display: table-header-group; }
tr { page-break-inside: avoid; }
table.meta, table.meta td { border: none; width: auto; padding: 1px 0; }
td.num, th.num { text-align: right; width: 4em; }
code, pre { font-family: Consolas, "JetBrains Mono", monospace; font-size: 13px; }
pre { background: #f6f6f8; padding: 8px 12px; overflow-x: auto; white-space: pre-wrap; }
.toc ul { margin: 4px 0; }
figure { margin: 12px 0; }
figure img { max-width: 100%; border: 1px solid #e4e4e8; }
.note { color: #8a6d00; }
a { color: #1f5fbf; text-decoration: none; }
@media print { body { padding: 0; } h2 { page-break-before: auto; } a { color: inherit; } }
</style>
</head>
<body>
<h1>&lt;商城&gt; &amp; 订单</h1>
<div class="kind">设计文档</div>
<table class="meta">
<tr><td>版本</td><td>1.0</td></tr><tr><td>作者</td><td>张三</td></tr><tr><td>生成时间</td><td>2026-10-28 10:00</td></tr>
<tr><td>来源</td><td>shop-model</td></tr>
</table>

<nav class="toc">
<h2>目录</h2>
<ul>
<li><a href="#overview">概述</a></li>
<li><a href="#diagrams">ER 图</a></li><li><a href="#relations">表间关系</a></li><li><a href="#tables">表清单</a></li>
<li><a href="#views">视图</a></li><li><a href="#details">表明细</a></li>
<li><a href="#unreadable">无法读取的表</a></li></ul>
</nav>

<h2 id="overview">概述</h2>
<p>测试模型</p><table style="width:auto">
<thead><tr><th colspan="2">统计</th></tr></thead>
<tbody>
<tr><td>Schema 数</td><td class="num">2</td></tr>
<tr><td>表数</td><td class="num">4</td></tr>
<tr><td>列数</td><td class="num">13</td></tr>
<tr><td>外键数</td><td class="num">2</td></tr>
<tr><td>逻辑外键数</td><td class="num">1</td></tr>
<tr><td>视图数</td><td class="num">1</td></tr></tbody>
</table>

<h2 id="diagrams">ER 图</h2>
<h3>shop</h3>
<p>全部表</p><figure><img src="data:image/png;base64,AQID" alt="shop" width="2"></figure>
<p class="note">表较多，图中只显示表名；明细见各表。</p>
<h2 id="relations">表间关系</h2>
<table>
<thead><tr><th>从</th><th>列</th><th>到</th><th>列</th><th>类别</th><th>名称</th></tr></thead>
<tbody>
<tr><td><a href="#t1">shop.customers</a></td><td>region_id</td><td><a href="#t4">crm.regions</a></td><td>id</td><td>外键</td><td>fk_region</td></tr>
<tr><td><a href="#t2">shop.items</a></td><td>order_id</td><td><a href="#t3">shop.orders</a></td><td>id</td><td>逻辑外键</td><td>lk_order</td></tr>
<tr><td><a href="#t3">shop.orders</a></td><td>cust_id, cust_code</td><td><a href="#t1">shop.customers</a></td><td>id, code</td><td>外键</td><td>fk_cust</td></tr>
</tbody>
</table>

<h2 id="tables">表清单</h2>
<h3>shop</h3>
<table>
<thead><tr><th class="num">序号</th><th>表名</th><th>注释</th><th class="num">列数</th></tr></thead>
<tbody>
<tr><td class="num">1</td><td><a href="#t1">customers</a></td><td>客户</td><td class="num">3</td></tr>
<tr><td class="num">2</td><td><a href="#t2">items</a></td><td></td><td class="num">3</td></tr>
<tr><td class="num">3</td><td><a href="#t3">orders</a></td><td></td><td class="num">5</td></tr>
</tbody>
</table>
<h3>crm</h3>
<table>
<thead><tr><th class="num">序号</th><th>表名</th><th>注释</th><th class="num">列数</th></tr></thead>
<tbody>
<tr><td class="num">1</td><td><a href="#t4">regions</a></td><td></td><td class="num">2</td></tr>
</tbody>
</table>

<h2 id="views">视图</h2>
<h3 id="v1">shop.v_orders</h3>
<p>订单视图</p><table>
<thead><tr><th class="num">序号</th><th>列名</th><th>类型</th><th>注释</th></tr></thead>
<tbody>
<tr><td class="num">1</td><td>id</td><td><code>int</code></td><td></td></tr>
</tbody>
</table>
<h4>定义</h4>
<pre><code>SELECT id FROM orders</code></pre>

<h2 id="details">表明细</h2>
<h3 id="t1">shop.customers</h3>
<p>客户</p><table>
<thead><tr><th class="num">序号</th><th>列名</th><th>类型</th><th>可空</th><th>默认值</th><th>标记</th><th>注释</th></tr></thead>
<tbody>
<tr><td class="num">1</td><td>id</td><td><code>int</code></td><td>否</td><td></td><td>主键, 自增</td><td></td></tr>
<tr><td class="num">2</td><td>code</td><td><code>varchar(20)</code></td><td>否</td><td></td><td></td><td>编号</td></tr>
<tr><td class="num">3</td><td>region_id</td><td><code>int</code></td><td>是</td><td></td><td></td><td></td></tr>
</tbody>
</table>
<h4>索引</h4>
<table>
<thead><tr><th>名称</th><th>列</th><th>唯一</th><th>方法</th></tr></thead>
<tbody>
<tr><td>ix_code</td><td>code DESC</td><td>否</td><td>BTREE</td></tr>
</tbody>
</table>
<h4>约束</h4>
<table>
<thead><tr><th>类别</th><th>名称</th><th>内容</th></tr></thead>
<tbody>
<tr><td>主键</td><td>pk_customers</td><td><code>id</code></td></tr>
<tr><td>唯一</td><td>uq_code</td><td><code>code</code></td></tr>
</tbody>
</table>
<h4>外键</h4>
<table>
<thead><tr><th>名称</th><th>列</th><th>引用</th><th>删除时</th><th>更新时</th><th>注释</th></tr></thead>
<tbody>
<tr><td>fk_region</td><td>region_id</td><td><a href="#t4">crm.regions</a> (id)</td><td>CASCADE</td><td></td><td></td></tr>
</tbody>
</table>

<h4>被引用</h4>
<table>
<thead><tr><th>表名</th><th>列</th><th>引用</th><th>类别</th><th>名称</th></tr></thead>
<tbody>
<tr><td><a href="#t3">shop.orders</a></td><td>cust_id, cust_code</td><td>id, code</td><td>外键</td><td>fk_cust</td></tr>
</tbody>
</table>
<h3 id="t2">shop.items</h3>
<table>
<thead><tr><th class="num">序号</th><th>列名</th><th>类型</th><th>可空</th><th>默认值</th><th>标记</th><th>注释</th></tr></thead>
<tbody>
<tr><td class="num">1</td><td>id</td><td><code>int</code></td><td>是</td><td></td><td></td><td></td></tr>
<tr><td class="num">2</td><td>order_id</td><td><code>int</code></td><td>是</td><td></td><td></td><td></td></tr>
<tr><td class="num">3</td><td>sku</td><td><code>varchar(30)</code></td><td>是</td><td></td><td></td><td></td></tr>
</tbody>
</table>
<h4>逻辑外键</h4>
<table>
<thead><tr><th>名称</th><th>列</th><th>引用</th><th>删除时</th><th>更新时</th><th>注释</th></tr></thead>
<tbody>
<tr><td>lk_order</td><td>order_id</td><td><a href="#t3">shop.orders</a> (id)</td><td></td><td></td><td>没有建外键</td></tr>
</tbody>
</table>

<h3 id="t3">shop.orders</h3>
<table>
<thead><tr><th class="num">序号</th><th>列名</th><th>类型</th><th>可空</th><th>默认值</th><th>标记</th><th>注释</th></tr></thead>
<tbody>
<tr><td class="num">1</td><td>id</td><td><code>int</code></td><td>否</td><td></td><td>主键</td><td></td></tr>
<tr><td class="num">2</td><td>cust_id</td><td><code>int</code></td><td>是</td><td></td><td></td><td></td></tr>
<tr><td class="num">3</td><td>cust_code</td><td><code>varchar(20)</code></td><td>是</td><td></td><td></td><td></td></tr>
<tr><td class="num">4</td><td>amount</td><td><code>decimal(10,2)</code></td><td>是</td><td>0</td><td></td><td></td></tr>
<tr><td class="num">5</td><td>double_amount</td><td><code>decimal(10,2)</code></td><td>是</td><td>amount * 2</td><td>生成列</td><td></td></tr>
</tbody>
</table>
<h4>约束</h4>
<table>
<thead><tr><th>类别</th><th>名称</th><th>内容</th></tr></thead>
<tbody>
<tr><td>主键</td><td></td><td><code>id</code></td></tr>
<tr><td>检查</td><td>ck_amount</td><td><code>amount &gt;= 0</code></td></tr>
</tbody>
</table>
<h4>外键</h4>
<table>
<thead><tr><th>名称</th><th>列</th><th>引用</th><th>删除时</th><th>更新时</th><th>注释</th></tr></thead>
<tbody>
<tr><td>fk_cust</td><td>cust_id, cust_code</td><td><a href="#t1">shop.customers</a> (id, code)</td><td>CASCADE</td><td></td><td></td></tr>
</tbody>
</table>

<h4>触发器</h4>
<table>
<thead><tr><th>名称</th><th>时机</th><th>事件</th></tr></thead>
<tbody>
<tr><td>trg_orders</td><td>BEFORE</td><td>INSERT, UPDATE</td></tr>
</tbody>
</table>
<h4>被引用</h4>
<table>
<thead><tr><th>表名</th><th>列</th><th>引用</th><th>类别</th><th>名称</th></tr></thead>
<tbody>
<tr><td><a href="#t2">shop.items</a></td><td>order_id</td><td>id</td><td>逻辑外键</td><td>lk_order</td></tr>
</tbody>
</table>
<h3 id="t4">crm.regions</h3>
<table>
<thead><tr><th class="num">序号</th><th>列名</th><th>类型</th><th>可空</th><th>默认值</th><th>标记</th><th>注释</th></tr></thead>
<tbody>
<tr><td class="num">1</td><td>id</td><td><code>int</code></td><td>是</td><td></td><td>主键</td><td></td></tr>
<tr><td class="num">2</td><td>name</td><td><code>varchar(50)</code></td><td>是</td><td></td><td></td><td></td></tr>
</tbody>
</table>
<h4>约束</h4>
<table>
<thead><tr><th>类别</th><th>名称</th><th>内容</th></tr></thead>
<tbody>
<tr><td>主键</td><td></td><td><code>id</code></td></tr>
</tbody>
</table>
<h4>被引用</h4>
<table>
<thead><tr><th>表名</th><th>列</th><th>引用</th><th>类别</th><th>名称</th></tr></thead>
<tbody>
<tr><td><a href="#t1">shop.customers</a></td><td>region_id</td><td>id</td><td>外键</td><td>fk_region</td></tr>
</tbody>
</table>

<h2 id="unreadable">无法读取的表</h2>
<table>
<thead><tr><th>Schema</th><th>表名</th><th>文件</th><th>原因</th></tr></thead>
<tbody>
<tr><td>shop</td><td>broken</td><td><code>tables&#x2f;shop&#x2f;broken.yaml</code></td><td>line 3: bad</td></tr>
</tbody>
</table>
</body>
</html>
```

- [ ] **Step 7: 新建 `crates/dblens-service/src/document/snapshots/dblens_service__document__template__tests__design_md_en.snap`**

````text
---
source: crates/dblens-service/src/document/template.rs
expression: "render(&c, TextFormat::Markdown, None, Lang::En, \"shop_images\").unwrap()"
---
# 商城

Design Document

- Version: 1.0
- Author: 张三
- Generated: 2026-10-28 10:00
- Source: shop-model

## Overview

测试模型

| Statistics | |
|---|---:|
| Schemas | 2 |
| Tables | 4 |
| Columns | 13 |
| Foreign keys | 2 |
| Logical keys | 1 |
| Views | 1 |

## ER Diagrams

### shop

全部表

![shop](shop_images/01-shop-_all.png)

> This diagram has many tables, so only table names are shown; see each table for details.

## Relationships

| From | Columns | To | Columns | Kind | Name |
|---|---|---|---|---|---|
| shop.customers | region_id | crm.regions | id | Foreign key | fk_region |
| shop.items | order_id | shop.orders | id | Logical key | lk_order |
| shop.orders | cust_id, cust_code | shop.customers | id, code | Foreign key | fk_cust |

## Tables

### shop

| No. | Table | Comment | Columns |
|---:|---|---|---:|
| 1 | [customers](#t1) | 客户 | 3 |
| 2 | [items](#t2) |  | 3 |
| 3 | [orders](#t3) |  | 5 |

### crm

| No. | Table | Comment | Columns |
|---:|---|---|---:|
| 1 | [regions](#t4) |  | 2 |

## Views

### <a id="v1"></a>shop.v_orders

订单视图

| No. | Column | Type | Comment |
|---:|---|---|---|
| 1 | id | int |  |

```sql
SELECT id FROM orders
```

## Table Details

### <a id="t1"></a>shop.customers

客户

| No. | Column | Type | Nullable | Default | Flags | Comment |
|---:|---|---|---|---|---|---|
| 1 | id | int | No |  | PK, Auto |  |
| 2 | code | varchar(20) | No |  |  | 编号 |
| 3 | region_id | int | Yes |  |  |  |

#### Indexes

| Name | Columns | Unique | Method |
|---|---|---|---|
| ix_code | code DESC | No | BTREE |

#### Constraints

| Kind | Name | Detail |
|---|---|---|
| Primary key | pk_customers | id |
| Unique | uq_code | code |

#### Foreign Keys

| Name | Columns | References | On delete | On update | Comment |
|---|---|---|---|---|---|
| fk_region | region_id | [crm.regions](#t4) (id) | CASCADE |  |  |

#### Referenced By

| Table | Columns | References | Kind | Name |
|---|---|---|---|---|
| [shop.orders](#t3) | cust_id, cust_code | id, code | Foreign key | fk_cust |

### <a id="t2"></a>shop.items

| No. | Column | Type | Nullable | Default | Flags | Comment |
|---:|---|---|---|---|---|---|
| 1 | id | int | Yes |  |  |  |
| 2 | order_id | int | Yes |  |  |  |
| 3 | sku | varchar(30) | Yes |  |  |  |

#### Logical Keys

| Name | Columns | References | On delete | On update | Comment |
|---|---|---|---|---|---|
| lk_order | order_id | [shop.orders](#t3) (id) |  |  | 没有建外键 |

### <a id="t3"></a>shop.orders

| No. | Column | Type | Nullable | Default | Flags | Comment |
|---:|---|---|---|---|---|---|
| 1 | id | int | No |  | PK |  |
| 2 | cust_id | int | Yes |  |  |  |
| 3 | cust_code | varchar(20) | Yes |  |  |  |
| 4 | amount | decimal(10,2) | Yes | 0 |  |  |
| 5 | double_amount | decimal(10,2) | Yes | amount * 2 | Generated |  |

#### Constraints

| Kind | Name | Detail |
|---|---|---|
| Primary key |  | id |
| Check | ck_amount | amount >= 0 |

#### Foreign Keys

| Name | Columns | References | On delete | On update | Comment |
|---|---|---|---|---|---|
| fk_cust | cust_id, cust_code | [shop.customers](#t1) (id, code) | CASCADE |  |  |

#### Triggers

| Name | Timing | Events |
|---|---|---|
| trg_orders | BEFORE | INSERT, UPDATE |

#### Referenced By

| Table | Columns | References | Kind | Name |
|---|---|---|---|---|
| [shop.items](#t2) | order_id | id | Logical key | lk_order |

### <a id="t4"></a>crm.regions

| No. | Column | Type | Nullable | Default | Flags | Comment |
|---:|---|---|---|---|---|---|
| 1 | id | int | Yes |  | PK |  |
| 2 | name | varchar(50) | Yes |  |  |  |

#### Constraints

| Kind | Name | Detail |
|---|---|---|
| Primary key |  | id |

#### Referenced By

| Table | Columns | References | Kind | Name |
|---|---|---|---|---|
| [shop.customers](#t1) | region_id | id | Foreign key | fk_region |

## Unreadable Tables

| Schema | Table | File | Reason |
|---|---|---|---|
| shop | broken | tables/shop/broken.yaml | line 3: bad |
````

- [ ] **Step 8: 新建 `crates/dblens-service/src/document/snapshots/dblens_service__document__template__tests__dictionary_md_zh.snap`**

```text
---
source: crates/dblens-service/src/document/template.rs
expression: "render(&c, TextFormat::Markdown, None, Lang::ZhCn, \"x\").unwrap()"
---
# 商城

数据字典

- 版本: 1.0
- 作者: 张三
- 生成时间: 2026-10-28 10:00
- 来源: shop-model

测试模型

## 表清单

### shop

| 序号 | 表名 | 注释 | 列数 |
|---:|---|---|---:|
| 1 | [customers](#t1) | 客户 | 3 |
| 2 | [items](#t2) |  | 3 |
| 3 | [orders](#t3) |  | 5 |

### crm

| 序号 | 表名 | 注释 | 列数 |
|---:|---|---|---:|
| 1 | [regions](#t4) |  | 2 |

## 表明细

### <a id="t1"></a>shop.customers

客户

| 序号 | 列名 | 类型 | 可空 | 默认值 | 标记 | 注释 |
|---:|---|---|---|---|---|---|
| 1 | id | int | 否 |  | 主键, 自增 |  |
| 2 | code | varchar(20) | 否 |  |  | 编号 |
| 3 | region_id | int | 是 |  |  |  |

#### 索引

| 名称 | 列 | 唯一 | 方法 |
|---|---|---|---|
| ix_code | code DESC | 否 | BTREE |

#### 约束

| 类别 | 名称 | 内容 |
|---|---|---|
| 主键 | pk_customers | id |
| 唯一 | uq_code | code |

#### 外键

| 名称 | 列 | 引用 | 删除时 | 更新时 | 注释 |
|---|---|---|---|---|---|
| fk_region | region_id | [crm.regions](#t4) (id) | CASCADE |  |  |

#### 被引用

| 表名 | 列 | 引用 | 类别 | 名称 |
|---|---|---|---|---|
| [shop.orders](#t3) | cust_id, cust_code | id, code | 外键 | fk_cust |

### <a id="t2"></a>shop.items

| 序号 | 列名 | 类型 | 可空 | 默认值 | 标记 | 注释 |
|---:|---|---|---|---|---|---|
| 1 | id | int | 是 |  |  |  |
| 2 | order_id | int | 是 |  |  |  |
| 3 | sku | varchar(30) | 是 |  |  |  |

#### 逻辑外键

| 名称 | 列 | 引用 | 删除时 | 更新时 | 注释 |
|---|---|---|---|---|---|
| lk_order | order_id | [shop.orders](#t3) (id) |  |  | 没有建外键 |

### <a id="t3"></a>shop.orders

| 序号 | 列名 | 类型 | 可空 | 默认值 | 标记 | 注释 |
|---:|---|---|---|---|---|---|
| 1 | id | int | 否 |  | 主键 |  |
| 2 | cust_id | int | 是 |  |  |  |
| 3 | cust_code | varchar(20) | 是 |  |  |  |
| 4 | amount | decimal(10,2) | 是 | 0 |  |  |
| 5 | double_amount | decimal(10,2) | 是 | amount * 2 | 生成列 |  |

#### 约束

| 类别 | 名称 | 内容 |
|---|---|---|
| 主键 |  | id |
| 检查 | ck_amount | amount >= 0 |

#### 外键

| 名称 | 列 | 引用 | 删除时 | 更新时 | 注释 |
|---|---|---|---|---|---|
| fk_cust | cust_id, cust_code | [shop.customers](#t1) (id, code) | CASCADE |  |  |

#### 触发器

| 名称 | 时机 | 事件 |
|---|---|---|
| trg_orders | BEFORE | INSERT, UPDATE |

#### 被引用

| 表名 | 列 | 引用 | 类别 | 名称 |
|---|---|---|---|---|
| [shop.items](#t2) | order_id | id | 逻辑外键 | lk_order |

### <a id="t4"></a>crm.regions

| 序号 | 列名 | 类型 | 可空 | 默认值 | 标记 | 注释 |
|---:|---|---|---|---|---|---|
| 1 | id | int | 是 |  | 主键 |  |
| 2 | name | varchar(50) | 是 |  |  |  |

#### 约束

| 类别 | 名称 | 内容 |
|---|---|---|
| 主键 |  | id |

#### 被引用

| 表名 | 列 | 引用 | 类别 | 名称 |
|---|---|---|---|---|
| [shop.customers](#t1) | region_id | id | 外键 | fk_region |

## 无法读取的表

| Schema | 表名 | 文件 | 原因 |
|---|---|---|---|
| shop | broken | tables/shop/broken.yaml | line 3: bad |
```

- [ ] **Step 9: 新建 `crates/dblens-service/src/document/template.rs`**

```rust
//! html、markdown 的模板（M4c 设计文档 5.1 节）：minijinja，内置一套，可换成用户的模板文件。
//! 模板只能使用传入的内容：没有加载器，`include`、`import` 找不到其他模板。

use base64::Engine;
use dblens_core::{Error, Lang, Result, msg};
use minijinja::syntax::SyntaxConfig;
use minijinja::value::{Serde, Value};
use minijinja::{AutoEscape, Environment};
use serde::{Deserialize, Serialize};

use super::content::DocContent;
use super::labels::label;

/// 用模板写的格式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextFormat {
    Html,
    Markdown,
}

impl TextFormat {
    /// 内置模板。
    pub fn builtin(self) -> &'static str {
        match self {
            TextFormat::Html => include_str!("templates/document.html"),
            TextFormat::Markdown => include_str!("templates/document.md"),
        }
    }

    /// 模板在环境中的名字（扩展名决定是否做 HTML 转义）。
    fn name(self) -> &'static str {
        match self {
            TextFormat::Html => "document.html",
            TextFormat::Markdown => "document.md",
        }
    }
}

/// markdown 表格单元格：`|` 转义，换行写成 `<br>`，空值写成空。
fn cell(v: Value) -> String {
    if v.is_none() || v.is_undefined() {
        return String::new();
    }
    v.to_string().replace('|', "\\|").replace("\r\n", "<br>").replace('\n', "<br>")
}

/// 空值写成空，其余原样。
fn text(v: Value) -> Value {
    if v.is_none() || v.is_undefined() { Value::from("") } else { v }
}

fn template_error(e: &minijinja::Error) -> Error {
    let mut detail = e.detail().map(str::to_string).unwrap_or_else(|| e.kind().to_string());
    if let Some(source) = std::error::Error::source(e) {
        detail = format!("{detail}: {source}");
    }
    Error::invalid(msg!("document.template_error", line = e.line().unwrap_or(0), error = detail))
}

fn environment(lang: Lang) -> Environment<'static> {
    let mut env = Environment::new();
    if let Ok(syntax) =
        SyntaxConfig::builder().trim_blocks(true).lstrip_blocks(true).keep_trailing_newline(true).build()
    {
        env.set_syntax(syntax);
    }
    env.set_auto_escape_callback(
        |name| if name.ends_with(".html") { AutoEscape::Html } else { AutoEscape::None },
    );
    env.add_function("t", move |key: &str| label(lang, key));
    env.add_filter("cell", cell);
    env.add_filter("v", text);
    env
}

/// 检查模板的语法。
pub fn check(format: TextFormat, source: &str) -> Result<()> {
    let mut env = environment(Lang::En);
    env.add_template_owned(format.name(), source.to_string()).map_err(|e| template_error(&e))
}

/// 按模板写出文档。`template` 为 None 时用内置模板；`image_dir` 是 markdown 中图片的相对目录。
pub fn render(
    content: &DocContent,
    format: TextFormat,
    template: Option<&str>,
    lang: Lang,
    image_dir: &str,
) -> Result<String> {
    let mut env = environment(lang);
    env.add_template_owned(format.name(), template.unwrap_or(format.builtin()).to_string())
        .map_err(|e| template_error(&e))?;
    let mut content = content.clone();
    for d in &mut content.diagrams {
        d.src = match format {
            TextFormat::Html => {
                format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(&d.png))
            }
            TextFormat::Markdown => format!("{image_dir}/{}", d.file),
        };
    }
    let mut ctx = serde_json::to_value(&content)
        .map_err(|e| Error::internal(msg!("document.write_failed", error = e)))?;
    if let Some(m) = ctx.as_object_mut() {
        m.insert("lang".into(), lang.code().into());
    }
    env.get_template(format.name())
        .and_then(|t| t.render(Value::from(Serde(&ctx))))
        .map_err(|e| template_error(&e))
}

#[cfg(test)]
mod tests {
    use super::super::content::tests::{opts, sample};
    use super::super::content::{DocDiagram, DocKind, build};
    use super::*;

    fn diagram() -> DocDiagram {
        DocDiagram {
            schema: "shop".into(),
            name: "_all".into(),
            title: "shop".into(),
            description: Some("全部表".into()),
            names_only: true,
            file: "01-shop-_all.png".into(),
            src: String::new(),
            width: 2,
            height: 1,
            png: vec![1, 2, 3],
        }
    }

    #[test]
    fn markdown_snapshots() {
        let c = build(opts(DocKind::Dictionary), sample(), vec![]);
        insta::assert_snapshot!(
            "dictionary_md_zh",
            render(&c, TextFormat::Markdown, None, Lang::ZhCn, "x").unwrap()
        );
        let c = build(opts(DocKind::Design), sample(), vec![diagram()]);
        insta::assert_snapshot!(
            "design_md_en",
            render(&c, TextFormat::Markdown, None, Lang::En, "shop_images").unwrap()
        );
    }

    #[test]
    fn html_snapshot_escapes_and_embeds_images() {
        let mut o = opts(DocKind::Design);
        o.info.title = "<商城> & 订单".into();
        let c = build(o, sample(), vec![diagram()]);
        let html = render(&c, TextFormat::Html, None, Lang::ZhCn, "").unwrap();
        assert!(html.contains("<title>&lt;商城&gt; &amp; 订单</title>"));
        assert!(html.contains(r#"<img src="data:image/png;base64,AQID""#));
        insta::assert_snapshot!("design_html_zh", html);
    }

    #[test]
    fn custom_template_errors_and_no_includes() {
        let c = build(opts(DocKind::Dictionary), sample(), vec![]);
        let out = render(
            &c,
            TextFormat::Markdown,
            Some("{{ info.title }}|{{ t('doc.col.table') }}|{{ none|v }}|{{ 'a|b'|cell }}"),
            Lang::En,
            "",
        )
        .unwrap();
        assert_eq!(out, "商城|Table||a\\|b");
        // 语法错误：带行号
        let e = check(TextFormat::Html, "line 1\n{% for x in %}").unwrap_err();
        assert!(e.to_string().contains('2'), "{e}");
        // 不能读其他模板或文件
        let e = render(&c, TextFormat::Markdown, Some("{% include 'C:/Windows/win.ini' %}"), Lang::En, "")
            .unwrap_err();
        assert!(matches!(e, Error::Invalid { .. }), "{e:?}");
    }

    #[test]
    fn every_label_is_used_and_exists() {
        let sources = [TextFormat::Html.builtin(), TextFormat::Markdown.builtin()];
        let used: std::collections::BTreeSet<String> = sources
            .iter()
            .flat_map(|s| s.match_indices("doc.").map(move |(i, _)| &s[i..]))
            .map(|s| {
                s.chars()
                    .take_while(|c| c.is_ascii_alphanumeric() || *c == '.' || *c == '_')
                    .collect::<String>()
            })
            .map(|k| k.trim_end_matches('.').to_string())
            .collect();
        let keys = super::super::labels::keys();
        // 用 `'doc.kind.' ~ 变量` 拼出来的键按前缀算
        let prefixes: Vec<&String> =
            used.iter().filter(|k| k.ends_with("kind") || k.ends_with("flag")).collect();
        for k in &keys {
            let dynamic = prefixes.iter().any(|p| k.starts_with(&format!("{p}.")));
            assert!(used.contains(k) || dynamic, "词条 {k} 没有被使用");
        }
        for k in used.iter().filter(|k| !prefixes.contains(k)) {
            assert!(keys.contains(k), "词条 {k} 不存在");
        }
    }
}
```

- [ ] **Step 10: 新建 `crates/dblens-service/src/document/templates/document.html`**

```
{#- DBLens 文档的 html 模板。上下文见 M4c 设计文档第 4 节（DocContent）；
    t('键') 取固定文字，|v 把空值写成空。输出自动做 HTML 转义（图片的 data URL 由程序生成，用 |safe）。 -#}
{% macro keys_table(keys) %}
<table>
<thead><tr><th>{{ t('doc.col.name') }}</th><th>{{ t('doc.col.index_columns') }}</th><th>{{ t('doc.col.references') }}</th><th>{{ t('doc.col.on_delete') }}</th><th>{{ t('doc.col.on_update') }}</th><th>{{ t('doc.col.comment') }}</th></tr></thead>
<tbody>
{% for k in keys %}
<tr><td>{{ k.name|v }}</td><td>{{ k.columns }}</td><td>{% if k.ref_anchor %}<a href="#{{ k.ref_anchor }}">{{ k.ref_schema }}.{{ k.ref_table }}</a>{% else %}{{ k.ref_schema }}.{{ k.ref_table }}{% endif %} ({{ k.ref_columns }})</td><td>{{ k.on_delete|v }}</td><td>{{ k.on_update|v }}</td><td>{{ k.comment|v }}</td></tr>
{% endfor %}
</tbody>
</table>
{% endmacro %}
<!doctype html>
<html lang="{{ lang }}">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<meta name="generator" content="DBLens">
<title>{{ info.title }}</title>
<style>
body { font: 14px/1.6 -apple-system, "Segoe UI", "PingFang SC", "Microsoft YaHei", sans-serif; color: #1f1f23; margin: 0 auto; max-width: 1100px; padding: 24px 32px; }
h1 { font-size: 26px; margin: 0 0 4px; }
h2 { font-size: 20px; border-bottom: 1px solid #ddd; padding-bottom: 4px; margin-top: 36px; }
h3 { font-size: 16px; margin-top: 28px; }
h4 { font-size: 14px; margin: 16px 0 6px; color: #444; }
.kind { color: #666; margin-bottom: 12px; }
.meta td:first-child { color: #666; padding-right: 16px; }
table { border-collapse: collapse; width: 100%; margin: 6px 0 12px; }
th, td { border: 1px solid #d9d9de; padding: 4px 8px; text-align: left; vertical-align: top; }
th { background: #f4f4f6; font-weight: 600; }
thead { display: table-header-group; }
tr { page-break-inside: avoid; }
table.meta, table.meta td { border: none; width: auto; padding: 1px 0; }
td.num, th.num { text-align: right; width: 4em; }
code, pre { font-family: Consolas, "JetBrains Mono", monospace; font-size: 13px; }
pre { background: #f6f6f8; padding: 8px 12px; overflow-x: auto; white-space: pre-wrap; }
.toc ul { margin: 4px 0; }
figure { margin: 12px 0; }
figure img { max-width: 100%; border: 1px solid #e4e4e8; }
.note { color: #8a6d00; }
a { color: #1f5fbf; text-decoration: none; }
@media print { body { padding: 0; } h2 { page-break-before: auto; } a { color: inherit; } }
</style>
</head>
<body>
<h1>{{ info.title }}</h1>
<div class="kind">{{ t('doc.dictionary') if kind == 'dictionary' else t('doc.design') }}</div>
<table class="meta">
{% if info.version %}<tr><td>{{ t('doc.version') }}</td><td>{{ info.version }}</td></tr>{% endif %}
{% if info.author %}<tr><td>{{ t('doc.author') }}</td><td>{{ info.author }}</td></tr>{% endif %}
<tr><td>{{ t('doc.generated') }}</td><td>{{ info.generated }}</td></tr>
<tr><td>{{ t('doc.source') }}</td><td>{{ info.source }}</td></tr>
</table>
{% if kind == 'dictionary' and info.description %}
<p>{{ info.description }}</p>
{% endif %}

<nav class="toc">
<h2>{{ t('doc.contents') }}</h2>
<ul>
{% if kind == 'design' %}
<li><a href="#overview">{{ t('doc.overview') }}</a></li>
{% if diagrams %}<li><a href="#diagrams">{{ t('doc.diagrams') }}</a></li>{% endif %}
{% if relations %}<li><a href="#relations">{{ t('doc.relations') }}</a></li>{% endif %}
{% endif %}
<li><a href="#tables">{{ t('doc.tables') }}</a></li>
{% if stats.views %}<li><a href="#views">{{ t('doc.views') }}</a></li>{% endif %}
<li><a href="#details">{{ t('doc.details') }}</a></li>
{% if unreadable %}<li><a href="#unreadable">{{ t('doc.unreadable') }}</a></li>{% endif %}
</ul>
</nav>
{% if kind == 'design' %}

<h2 id="overview">{{ t('doc.overview') }}</h2>
{% if info.description %}<p>{{ info.description }}</p>{% endif %}
<table style="width:auto">
<thead><tr><th colspan="2">{{ t('doc.stats') }}</th></tr></thead>
<tbody>
<tr><td>{{ t('doc.stat.schemas') }}</td><td class="num">{{ stats.schemas }}</td></tr>
<tr><td>{{ t('doc.stat.tables') }}</td><td class="num">{{ stats.tables }}</td></tr>
<tr><td>{{ t('doc.stat.columns') }}</td><td class="num">{{ stats.columns }}</td></tr>
<tr><td>{{ t('doc.stat.foreign_keys') }}</td><td class="num">{{ stats.foreign_keys }}</td></tr>
<tr><td>{{ t('doc.stat.logical_keys') }}</td><td class="num">{{ stats.logical_keys }}</td></tr>
{% if stats.views %}<tr><td>{{ t('doc.stat.views') }}</td><td class="num">{{ stats.views }}</td></tr>{% endif %}
</tbody>
</table>
{% if diagrams %}

<h2 id="diagrams">{{ t('doc.diagrams') }}</h2>
{% for d in diagrams %}
<h3>{{ d.title }}</h3>
{% if d.description %}<p>{{ d.description }}</p>{% endif %}
<figure><img src="{{ d.src|safe }}" alt="{{ d.title }}" width="{{ d.width }}"></figure>
{% if d.names_only %}<p class="note">{{ t('doc.names_only') }}</p>{% endif %}
{% endfor %}
{% endif %}
{% if relations %}

<h2 id="relations">{{ t('doc.relations') }}</h2>
<table>
<thead><tr><th>{{ t('doc.col.from') }}</th><th>{{ t('doc.col.index_columns') }}</th><th>{{ t('doc.col.to') }}</th><th>{{ t('doc.col.index_columns') }}</th><th>{{ t('doc.col.kind') }}</th><th>{{ t('doc.col.name') }}</th></tr></thead>
<tbody>
{% for r in relations %}
<tr><td>{% if r.from_anchor %}<a href="#{{ r.from_anchor }}">{{ r.from_schema }}.{{ r.from_table }}</a>{% else %}{{ r.from_schema }}.{{ r.from_table }}{% endif %}</td><td>{{ r.from_columns }}</td><td>{% if r.to_anchor %}<a href="#{{ r.to_anchor }}">{{ r.to_schema }}.{{ r.to_table }}</a>{% else %}{{ r.to_schema }}.{{ r.to_table }}{% endif %}</td><td>{{ r.to_columns }}</td><td>{{ t('doc.kind.' ~ r.kind) }}</td><td>{{ r.name|v }}</td></tr>
{% endfor %}
</tbody>
</table>
{% endif %}
{% endif %}

<h2 id="tables">{{ t('doc.tables') }}</h2>
{% for s in schemas %}
<h3>{{ s.name }}</h3>
<table>
<thead><tr><th class="num">{{ t('doc.col.no') }}</th><th>{{ t('doc.col.table') }}</th><th>{{ t('doc.col.comment') }}</th><th class="num">{{ t('doc.col.columns') }}</th></tr></thead>
<tbody>
{% for tb in s.tables %}
<tr><td class="num">{{ loop.index }}</td><td><a href="#{{ tb.anchor }}">{{ tb.name }}</a></td><td>{{ tb.comment|v }}</td><td class="num">{{ tb.columns|length }}</td></tr>
{% endfor %}
</tbody>
</table>
{% endfor %}
{% if stats.views %}

<h2 id="views">{{ t('doc.views') }}</h2>
{% for s in schemas %}
{% for vw in s.views %}
<h3 id="{{ vw.anchor }}">{{ s.name }}.{{ vw.name }}</h3>
{% if vw.comment %}<p>{{ vw.comment }}</p>{% endif %}
<table>
<thead><tr><th class="num">{{ t('doc.col.no') }}</th><th>{{ t('doc.col.column') }}</th><th>{{ t('doc.col.type') }}</th><th>{{ t('doc.col.comment') }}</th></tr></thead>
<tbody>
{% for c in vw.columns %}
<tr><td class="num">{{ c.ordinal }}</td><td>{{ c.name }}</td><td><code>{{ c.data_type }}</code></td><td>{{ c.comment|v }}</td></tr>
{% endfor %}
</tbody>
</table>
{% if vw.definition %}
<h4>{{ t('doc.sec.definition') }}</h4>
<pre><code>{{ vw.definition }}</code></pre>
{% endif %}
{% endfor %}
{% endfor %}
{% endif %}

<h2 id="details">{{ t('doc.details') }}</h2>
{% for s in schemas %}
{% for tb in s.tables %}
<h3 id="{{ tb.anchor }}">{{ s.name }}.{{ tb.name }}</h3>
{% if tb.comment %}<p>{{ tb.comment }}</p>{% endif %}
{% if tb.partition %}<p>{{ t('doc.partition') }}: <code>{{ tb.partition }}</code></p>{% endif %}
<table>
<thead><tr><th class="num">{{ t('doc.col.no') }}</th><th>{{ t('doc.col.column') }}</th><th>{{ t('doc.col.type') }}</th><th>{{ t('doc.col.nullable') }}</th><th>{{ t('doc.col.default') }}</th><th>{{ t('doc.col.flags') }}</th><th>{{ t('doc.col.comment') }}</th></tr></thead>
<tbody>
{% for c in tb.columns %}
<tr><td class="num">{{ c.ordinal }}</td><td>{{ c.name }}</td><td><code>{{ c.data_type }}</code></td><td>{{ t('doc.yes') if c.nullable else t('doc.no') }}</td><td>{{ c.default|v }}</td><td>{% for f in c.flags %}{{ t('doc.flag.' ~ f) }}{{ ', ' if not loop.last }}{% endfor %}</td><td>{{ c.comment|v }}</td></tr>
{% endfor %}
</tbody>
</table>
{% if tb.indexes %}
<h4>{{ t('doc.sec.indexes') }}</h4>
<table>
<thead><tr><th>{{ t('doc.col.name') }}</th><th>{{ t('doc.col.index_columns') }}</th><th>{{ t('doc.col.unique') }}</th><th>{{ t('doc.col.method') }}</th></tr></thead>
<tbody>
{% for i in tb.indexes %}
<tr><td>{{ i.name }}</td><td>{{ i.columns }}</td><td>{{ t('doc.yes') if i.unique else t('doc.no') }}</td><td>{{ i.method|v }}</td></tr>
{% endfor %}
</tbody>
</table>
{% endif %}
{% if tb.constraints %}
<h4>{{ t('doc.sec.constraints') }}</h4>
<table>
<thead><tr><th>{{ t('doc.col.kind') }}</th><th>{{ t('doc.col.name') }}</th><th>{{ t('doc.col.detail') }}</th></tr></thead>
<tbody>
{% for k in tb.constraints %}
<tr><td>{{ t('doc.kind.' ~ k.kind) }}</td><td>{{ k.name|v }}</td><td><code>{{ k.detail }}</code></td></tr>
{% endfor %}
</tbody>
</table>
{% endif %}
{% if tb.foreign_keys %}
<h4>{{ t('doc.sec.foreign_keys') }}</h4>
{{ keys_table(tb.foreign_keys) }}
{% endif %}
{% if tb.logical_keys %}
<h4>{{ t('doc.sec.logical_keys') }}</h4>
{{ keys_table(tb.logical_keys) }}
{% endif %}
{% if tb.triggers %}
<h4>{{ t('doc.sec.triggers') }}</h4>
<table>
<thead><tr><th>{{ t('doc.col.name') }}</th><th>{{ t('doc.col.timing') }}</th><th>{{ t('doc.col.events') }}</th></tr></thead>
<tbody>
{% for g in tb.triggers %}
<tr><td>{{ g.name }}</td><td>{{ g.timing|v }}</td><td>{{ g.events }}</td></tr>
{% endfor %}
</tbody>
</table>
{% endif %}
{% if tb.referenced_by %}
<h4>{{ t('doc.sec.referenced_by') }}</h4>
<table>
<thead><tr><th>{{ t('doc.col.table') }}</th><th>{{ t('doc.col.index_columns') }}</th><th>{{ t('doc.col.references') }}</th><th>{{ t('doc.col.kind') }}</th><th>{{ t('doc.col.name') }}</th></tr></thead>
<tbody>
{% for r in tb.referenced_by %}
<tr><td>{% if r.anchor %}<a href="#{{ r.anchor }}">{{ r.schema }}.{{ r.table }}</a>{% else %}{{ r.schema }}.{{ r.table }}{% endif %}</td><td>{{ r.columns }}</td><td>{{ r.ref_columns }}</td><td>{{ t('doc.kind.' ~ r.kind) }}</td><td>{{ r.name|v }}</td></tr>
{% endfor %}
</tbody>
</table>
{% endif %}
{% endfor %}
{% endfor %}
{% if unreadable %}

<h2 id="unreadable">{{ t('doc.unreadable') }}</h2>
<table>
<thead><tr><th>{{ t('doc.col.schema') }}</th><th>{{ t('doc.col.table') }}</th><th>{{ t('doc.col.file') }}</th><th>{{ t('doc.col.error') }}</th></tr></thead>
<tbody>
{% for u in unreadable %}
<tr><td>{{ u.schema }}</td><td>{{ u.name }}</td><td><code>{{ u.path }}</code></td><td>{{ u.message }}</td></tr>
{% endfor %}
</tbody>
</table>
{% endif %}
</body>
</html>
```

- [ ] **Step 11: 新建 `crates/dblens-service/src/document/templates/document.md`**

````markdown
{#- DBLens 文档的 markdown 模板。上下文见 M4c 设计文档第 4 节（DocContent）；
    t('键') 取固定文字，|v 把空值写成空，|cell 转义表格单元格。 -#}
# {{ info.title }}

{{ t('doc.dictionary') if kind == 'dictionary' else t('doc.design') }}

{% if info.version %}
- {{ t('doc.version') }}: {{ info.version }}
{% endif %}
{% if info.author %}
- {{ t('doc.author') }}: {{ info.author }}
{% endif %}
- {{ t('doc.generated') }}: {{ info.generated }}
- {{ t('doc.source') }}: {{ info.source }}
{% if kind == 'dictionary' and info.description %}

{{ info.description }}
{% endif %}
{% if kind == 'design' %}

## {{ t('doc.overview') }}
{% if info.description %}

{{ info.description }}
{% endif %}

| {{ t('doc.stats') }} | |
|---|---:|
| {{ t('doc.stat.schemas') }} | {{ stats.schemas }} |
| {{ t('doc.stat.tables') }} | {{ stats.tables }} |
| {{ t('doc.stat.columns') }} | {{ stats.columns }} |
| {{ t('doc.stat.foreign_keys') }} | {{ stats.foreign_keys }} |
| {{ t('doc.stat.logical_keys') }} | {{ stats.logical_keys }} |
{% if stats.views %}
| {{ t('doc.stat.views') }} | {{ stats.views }} |
{% endif %}
{% if diagrams %}

## {{ t('doc.diagrams') }}
{% for d in diagrams %}

### {{ d.title }}
{% if d.description %}

{{ d.description }}
{% endif %}

![{{ d.title }}]({{ d.src }})
{% if d.names_only %}

> {{ t('doc.names_only') }}
{% endif %}
{% endfor %}
{% endif %}
{% if relations %}

## {{ t('doc.relations') }}

| {{ t('doc.col.from') }} | {{ t('doc.col.index_columns') }} | {{ t('doc.col.to') }} | {{ t('doc.col.index_columns') }} | {{ t('doc.col.kind') }} | {{ t('doc.col.name') }} |
|---|---|---|---|---|---|
{% for r in relations %}
| {{ r.from_schema }}.{{ r.from_table|cell }} | {{ r.from_columns|cell }} | {{ r.to_schema }}.{{ r.to_table|cell }} | {{ r.to_columns|cell }} | {{ t('doc.kind.' ~ r.kind) }} | {{ r.name|cell }} |
{% endfor %}
{% endif %}
{% endif %}

## {{ t('doc.tables') }}
{% for s in schemas %}

### {{ s.name }}

| {{ t('doc.col.no') }} | {{ t('doc.col.table') }} | {{ t('doc.col.comment') }} | {{ t('doc.col.columns') }} |
|---:|---|---|---:|
{% for tb in s.tables %}
| {{ loop.index }} | [{{ tb.name|cell }}](#{{ tb.anchor }}) | {{ tb.comment|cell }} | {{ tb.columns|length }} |
{% endfor %}
{% endfor %}
{% for s in schemas if s.views %}
{% if loop.first %}

## {{ t('doc.views') }}
{% endif %}
{% for vw in s.views %}

### <a id="{{ vw.anchor }}"></a>{{ s.name }}.{{ vw.name }}
{% if vw.comment %}

{{ vw.comment }}
{% endif %}

| {{ t('doc.col.no') }} | {{ t('doc.col.column') }} | {{ t('doc.col.type') }} | {{ t('doc.col.comment') }} |
|---:|---|---|---|
{% for c in vw.columns %}
| {{ c.ordinal }} | {{ c.name|cell }} | {{ c.data_type|cell }} | {{ c.comment|cell }} |
{% endfor %}
{% if vw.definition %}

```sql
{{ vw.definition }}
```
{% endif %}
{% endfor %}
{% endfor %}

## {{ t('doc.details') }}
{% for s in schemas %}
{% for tb in s.tables %}

### <a id="{{ tb.anchor }}"></a>{{ s.name }}.{{ tb.name }}
{% if tb.comment %}

{{ tb.comment }}
{% endif %}
{% if tb.partition %}

{{ t('doc.partition') }}: {{ tb.partition }}
{% endif %}

| {{ t('doc.col.no') }} | {{ t('doc.col.column') }} | {{ t('doc.col.type') }} | {{ t('doc.col.nullable') }} | {{ t('doc.col.default') }} | {{ t('doc.col.flags') }} | {{ t('doc.col.comment') }} |
|---:|---|---|---|---|---|---|
{% for c in tb.columns %}
| {{ c.ordinal }} | {{ c.name|cell }} | {{ c.data_type|cell }} | {{ t('doc.yes') if c.nullable else t('doc.no') }} | {{ c.default|cell }} | {% for f in c.flags %}{{ t('doc.flag.' ~ f) }}{{ ', ' if not loop.last }}{% endfor %} | {{ c.comment|cell }} |
{% endfor %}
{% if tb.indexes %}

#### {{ t('doc.sec.indexes') }}

| {{ t('doc.col.name') }} | {{ t('doc.col.index_columns') }} | {{ t('doc.col.unique') }} | {{ t('doc.col.method') }} |
|---|---|---|---|
{% for i in tb.indexes %}
| {{ i.name|cell }} | {{ i.columns|cell }} | {{ t('doc.yes') if i.unique else t('doc.no') }} | {{ i.method|cell }} |
{% endfor %}
{% endif %}
{% if tb.constraints %}

#### {{ t('doc.sec.constraints') }}

| {{ t('doc.col.kind') }} | {{ t('doc.col.name') }} | {{ t('doc.col.detail') }} |
|---|---|---|
{% for k in tb.constraints %}
| {{ t('doc.kind.' ~ k.kind) }} | {{ k.name|cell }} | {{ k.detail|cell }} |
{% endfor %}
{% endif %}
{% for title, keys in [(t('doc.sec.foreign_keys'), tb.foreign_keys), (t('doc.sec.logical_keys'), tb.logical_keys)] if keys %}

#### {{ title }}

| {{ t('doc.col.name') }} | {{ t('doc.col.index_columns') }} | {{ t('doc.col.references') }} | {{ t('doc.col.on_delete') }} | {{ t('doc.col.on_update') }} | {{ t('doc.col.comment') }} |
|---|---|---|---|---|---|
{% for k in keys %}
| {{ k.name|cell }} | {{ k.columns|cell }} | {% if k.ref_anchor %}[{{ k.ref_schema }}.{{ k.ref_table|cell }}](#{{ k.ref_anchor }}){% else %}{{ k.ref_schema }}.{{ k.ref_table|cell }}{% endif %} ({{ k.ref_columns|cell }}) | {{ k.on_delete|cell }} | {{ k.on_update|cell }} | {{ k.comment|cell }} |
{% endfor %}
{% endfor %}
{% if tb.triggers %}

#### {{ t('doc.sec.triggers') }}

| {{ t('doc.col.name') }} | {{ t('doc.col.timing') }} | {{ t('doc.col.events') }} |
|---|---|---|
{% for g in tb.triggers %}
| {{ g.name|cell }} | {{ g.timing|cell }} | {{ g.events|cell }} |
{% endfor %}
{% endif %}
{% if tb.referenced_by %}

#### {{ t('doc.sec.referenced_by') }}

| {{ t('doc.col.table') }} | {{ t('doc.col.index_columns') }} | {{ t('doc.col.references') }} | {{ t('doc.col.kind') }} | {{ t('doc.col.name') }} |
|---|---|---|---|---|
{% for r in tb.referenced_by %}
| {% if r.anchor %}[{{ r.schema }}.{{ r.table|cell }}](#{{ r.anchor }}){% else %}{{ r.schema }}.{{ r.table|cell }}{% endif %} | {{ r.columns|cell }} | {{ r.ref_columns|cell }} | {{ t('doc.kind.' ~ r.kind) }} | {{ r.name|cell }} |
{% endfor %}
{% endif %}
{% endfor %}
{% endfor %}
{% if unreadable %}

## {{ t('doc.unreadable') }}

| {{ t('doc.col.schema') }} | {{ t('doc.col.table') }} | {{ t('doc.col.file') }} | {{ t('doc.col.error') }} |
|---|---|---|---|
{% for u in unreadable %}
| {{ u.schema|cell }} | {{ u.name|cell }} | {{ u.path|cell }} | {{ u.message|cell }} |
{% endfor %}
{% endif %}
````

- [ ] **Step 12: 检查**

```bash
export CARGO_INCREMENTAL=0
cargo fmt --all
cargo clippy --config "source.mirror.registry='sparse+https://rsproxy.cn/index/'" --workspace --all-targets -- -D warnings
DBLENS_LANG=en cargo test --config "source.mirror.registry='sparse+https://rsproxy.cn/index/'" --workspace
```

Expected: 全部测试通过（service 新增 4 个，快照 3 个）。

- [ ] **Step 13: 提交**

```bash
git add Cargo.lock crates/dblens-core/locales/app.json crates/dblens-service/Cargo.toml crates/dblens-service/src/document/labels.json crates/dblens-service/src/document/labels.rs crates/dblens-service/src/document/mod.rs crates/dblens-service/src/document/snapshots/dblens_service__document__template__tests__design_html_zh.snap crates/dblens-service/src/document/snapshots/dblens_service__document__template__tests__design_md_en.snap crates/dblens-service/src/document/snapshots/dblens_service__document__template__tests__dictionary_md_zh.snap crates/dblens-service/src/document/template.rs crates/dblens-service/src/document/templates/document.html crates/dblens-service/src/document/templates/document.md
git commit -m "feat(document): html、markdown 模板" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 4: 服务层：docx

- `document/docx/*.xml`（新）：内置样式来源的部件（A4 纵向、中文正文"等线"、标题、题注、目录、表格、代码、链接样式），`builtin_reference()` 运行时打包成 .docx。
- `document/docx.rs`（新）：读取样式来源（必须有 `word/document.xml`、`styles.xml`、`_rels/document.xml.rels`、`[Content_Types].xml`），按 Word 内部名（小写）找样式编号，缺的从内置样式补上（编号加 `DBLens` 前缀）；正文每个块元素一行；标题、书签和链接、表格（表头跨页重复、行不拆开、按比例的列宽）、图片（2 倍像素，按版心缩放）、代码块、目录字段；保留样式来源最后的节属性；`settings.xml` 加打开时更新字段；补图片的关系和内容类型；去掉控制字符。
- 依赖：工作区加 `quick-xml = "0.42.0"`、`zip = { version = "9.0.5", default-features = false, features = ["deflate-flate2"] }`，service 引用两者；`Cargo.lock` 中用 `cargo update -p zip --precise 9.0.5` 锁定（9.1.x 写计划时发布不满 7 天）。
- `labels.json` 加 `doc.toc_hint`；标签测试也扫描 `docx.rs`（测试之前的部分，带引号的键）。
- 后端词条加 `document.bad_reference`。

快照文件（`snapshots/`）随本任务给出；改了生成逻辑时用 `INSTA_UPDATE=always` 重新生成并逐个审查差异。

**Files:**
- Modify: `Cargo.toml`
- Modify: `crates/dblens-core/locales/app.json`
- Modify: `crates/dblens-service/Cargo.toml`
- Create: `crates/dblens-service/src/document/docx.rs`
- Create: `crates/dblens-service/src/document/docx/content_types.xml`
- Create: `crates/dblens-service/src/document/docx/core.xml`
- Create: `crates/dblens-service/src/document/docx/document.xml`
- Create: `crates/dblens-service/src/document/docx/document_rels.xml`
- Create: `crates/dblens-service/src/document/docx/rels.xml`
- Create: `crates/dblens-service/src/document/docx/settings.xml`
- Create: `crates/dblens-service/src/document/docx/styles.xml`
- Modify: `crates/dblens-service/src/document/labels.json`
- Modify: `crates/dblens-service/src/document/mod.rs`
- Create: `crates/dblens-service/src/document/snapshots/dblens_service__document__docx__tests__docx_design_zh.snap`
- Modify: `crates/dblens-service/src/document/template.rs`

**Interfaces:**
- Consumes：任务 2、3。
- Produces：`write_docx(content, reference: Option<&[u8]>, lang) -> Result<Vec<u8>>`、`builtin_reference() -> Vec<u8>`、`check_reference(bytes) -> Result<()>`。

- [ ] **Step 1: 修改 `Cargo.toml`**

```diff
--- a/Cargo.toml
+++ b/Cargo.toml
@@ -44,6 +44,7 @@ minijinja = "3.0.3"
 mysql_async = { version = "0.37.1", default-features = false, features = ["default-rustls"] }
 oracle = { version = "0.6.3", features = ["chrono"] }
 percent-encoding = "2.3.2"
+quick-xml = "0.42.0"
 reqwest = { version = "0.13.5", default-features = false, features = ["rustls"] }
 rusqlite = { version = "0.41.0", features = ["bundled"] }
 rust_xlsxwriter = { version = "0.101.0", default-features = false, features = ["constant_memory", "chrono"] }
@@ -73,6 +74,7 @@ tracing-subscriber = "0.3.23"
 url = "2.5.8"
 uuid = { version = "1.28.0", features = ["serde", "v4"] }
 webpki-roots = "1.0.11"
+zip = { version = "9.0.5", default-features = false, features = ["deflate-flate2"] }
 
 # openGauss 的 SHA256 认证：替换为 vendor/ 中打了补丁的版本（见 vendor/PATCHES.md）
 [patch.crates-io]
```

- [ ] **Step 2: 修改 `crates/dblens-core/locales/app.json`**

```diff
--- a/crates/dblens-core/locales/app.json
+++ b/crates/dblens-core/locales/app.json
@@ -61,6 +61,7 @@
   "dm.no_diagnostics": {"zh-CN": "调用 DPI 失败，且无法读取错误信息", "en": "A DPI call failed and its error information could not be read"},
   "dm.nul_in_string": {"zh-CN": "字符串中不能包含 \\0 字符", "en": "Strings cannot contain the \\0 character"},
   "dml.no_insertable_columns": {"zh-CN": "没有可插入的列：%{table}", "en": "No insertable columns: %{table}"},
+  "document.bad_reference": {"zh-CN": "样式来源不是有效的 .docx 文件：%{error}", "en": "The style reference is not a valid .docx file: %{error}"},
   "document.template_error": {"zh-CN": "模板第 %{line} 行有错：%{error}", "en": "Template error at line %{line}: %{error}"},
   "document.write_failed": {"zh-CN": "写文档失败：%{error}", "en": "Failed to write the document: %{error}"},
   "driver.connect_timeout": {"zh-CN": "连接超时（%{seconds} 秒）", "en": "Connection timed out (%{seconds} s)"},
```

- [ ] **Step 3: 修改 `crates/dblens-service/Cargo.toml`**

```diff
--- a/crates/dblens-service/Cargo.toml
+++ b/crates/dblens-service/Cargo.toml
@@ -34,6 +34,7 @@ encoding_rs.workspace = true
 encoding_rs_io.workspace = true
 keyring.workspace = true
 minijinja = { workspace = true, features = ["serde"] }
+quick-xml.workspace = true
 rusqlite.workspace = true
 rust_xlsxwriter.workspace = true
 serde.workspace = true
@@ -43,6 +44,7 @@ sqlparser.workspace = true
 tokio.workspace = true
 tracing.workspace = true
 uuid.workspace = true
+zip.workspace = true
 
 [dev-dependencies]
 csv.workspace = true
```

- [ ] **Step 4: 新建 `crates/dblens-service/src/document/docx.rs`**

```rust
//! docx（M4c 设计文档 5.2 节）：程序生成正文，放进"样式来源"文档（类似 pandoc 的 reference-doc）。
//! 样式来源中的样式、页眉页脚、纸张和页边距都保留；样式按 Word 内部名找，缺的补上内置的定义。

use std::collections::HashMap;
use std::fmt::Write as _;
use std::io::{Cursor, Read, Write};

use dblens_core::{Error, Lang, Result, msg};
use quick_xml::escape::escape;
use quick_xml::events::Event;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use super::content::{DocContent, DocKey, DocKind, DocTable};
use super::labels::label;

const BUILTIN_PARTS: [(&str, &str); 7] = [
    ("[Content_Types].xml", include_str!("docx/content_types.xml")),
    ("_rels/.rels", include_str!("docx/rels.xml")),
    ("docProps/core.xml", include_str!("docx/core.xml")),
    ("word/document.xml", include_str!("docx/document.xml")),
    ("word/_rels/document.xml.rels", include_str!("docx/document_rels.xml")),
    ("word/styles.xml", include_str!("docx/styles.xml")),
    ("word/settings.xml", include_str!("docx/settings.xml")),
];

const BUILTIN_STYLES: &str = include_str!("docx/styles.xml");

/// 要用到的样式（Word 内部名，小写）。
const STYLE_NAMES: [&str; 12] = [
    "title",
    "heading 1",
    "heading 2",
    "heading 3",
    "caption",
    "toc heading",
    "toc 1",
    "toc 2",
    "dblens table text",
    "dblens code",
    "hyperlink",
    "table grid",
];

const NS_W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const REL_IMAGE: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/image";
const REL_SETTINGS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/settings";

/// 1 twip（1/20 磅）= 635 EMU；图片按 2 倍像素画，1 像素按 1/192 英寸 = 4762.5 EMU。
const EMU_PER_TWIP: u64 = 635;
const EMU_PER_PX: f64 = 4762.5;

fn bad_reference(e: impl std::fmt::Display) -> Error {
    Error::invalid(msg!("document.bad_reference", error = e))
}

fn write_err(e: impl std::fmt::Display) -> Error {
    Error::internal(msg!("document.write_failed", error = e))
}

/// 内置的样式来源。
pub fn builtin_reference() -> Vec<u8> {
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    let opts = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    for (name, text) in BUILTIN_PARTS {
        // 写入失败只会是内存不足
        let _ = zip.start_file(name, opts).and_then(|_| Ok(zip.write_all(text.as_bytes())?));
    }
    zip.finish().map(Cursor::into_inner).unwrap_or_default()
}

/// 读出的样式来源。
struct Reference {
    parts: Vec<(String, Vec<u8>)>,
}

impl Reference {
    fn read(bytes: &[u8]) -> Result<Self> {
        let mut zip = ZipArchive::new(Cursor::new(bytes)).map_err(bad_reference)?;
        let mut parts = Vec::new();
        for i in 0..zip.len() {
            let mut f = zip.by_index(i).map_err(bad_reference)?;
            if f.is_dir() {
                continue;
            }
            let mut data = Vec::new();
            f.read_to_end(&mut data).map_err(bad_reference)?;
            parts.push((f.name().map_err(bad_reference)?.to_string(), data));
        }
        let r = Reference { parts };
        for need in
            ["word/document.xml", "word/styles.xml", "word/_rels/document.xml.rels", "[Content_Types].xml"]
        {
            if r.get(need).is_none() {
                return Err(bad_reference(need));
            }
        }
        Ok(r)
    }

    fn get(&self, name: &str) -> Option<&str> {
        self.parts.iter().find(|(n, _)| n == name).and_then(|(_, d)| std::str::from_utf8(d).ok())
    }

    fn set(&mut self, name: &str, data: Vec<u8>) {
        match self.parts.iter_mut().find(|(n, _)| n == name) {
            Some(p) => p.1 = data,
            None => self.parts.push((name.to_string(), data)),
        }
    }
}

/// 检查样式来源是不是有效的 .docx。
pub fn check_reference(bytes: &[u8]) -> Result<()> {
    Reference::read(bytes).map(|_| ())
}

/// 正文最后的节属性（纸张、页边距、页眉页脚的引用）。
fn section(document: &str) -> String {
    let end = document.rfind("</w:body>").unwrap_or(document.len());
    match document[..end].rfind("<w:sectPr") {
        Some(start) => document[start..end].trim().to_string(),
        None => String::new(),
    }
}

/// 节属性中某个元素的数字属性。
fn attr_num(sect: &str, element: &str, attr: &str) -> Option<u64> {
    let start = sect.find(&format!("<{element} "))?;
    let tag = &sect[start..start + sect[start..].find('>')?];
    let key = format!("{attr}=\"");
    let at = tag.find(&key)? + key.len();
    tag[at..].split('"').next()?.parse().ok()
}

/// 版心的宽、高（twip）。
fn text_area(sect: &str) -> (u64, u64) {
    let w = attr_num(sect, "w:pgSz", "w:w").unwrap_or(11906);
    let h = attr_num(sect, "w:pgSz", "w:h").unwrap_or(16838);
    let left = attr_num(sect, "w:pgMar", "w:left").unwrap_or(1247);
    let right = attr_num(sect, "w:pgMar", "w:right").unwrap_or(1247);
    let top = attr_num(sect, "w:pgMar", "w:top").unwrap_or(1440);
    let bottom = attr_num(sect, "w:pgMar", "w:bottom").unwrap_or(1440);
    (w.saturating_sub(left + right).max(1440), h.saturating_sub(top + bottom).max(1440))
}

/// 样式名（小写）→ 样式编号。
fn style_ids(styles: &str) -> HashMap<String, String> {
    let mut reader = quick_xml::Reader::from_str(styles);
    let mut out = HashMap::new();
    let mut current: Option<String> = None;
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => match e.name().as_ref() {
                "w:style" => {
                    current = e
                        .attributes()
                        .flatten()
                        .find(|a| a.key.as_ref() == "w:styleId")
                        .map(|a| a.value.to_string());
                }
                "w:name" => {
                    let name = e
                        .attributes()
                        .flatten()
                        .find(|a| a.key.as_ref() == "w:val")
                        .map(|a| a.value.to_lowercase());
                    if let (Some(id), Some(name)) = (current.take(), name) {
                        out.entry(name).or_insert(id);
                    }
                }
                _ => {}
            },
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    out
}

/// 内置样式中按名字取整段 `<w:style>`。
fn builtin_style(name: &str) -> Option<&'static str> {
    BUILTIN_STYLES.split("<w:style ").skip(1).find_map(|s| {
        let body = &s[..s.find("</w:style>")? + "</w:style>".len()];
        body.to_lowercase().contains(&format!("<w:name w:val=\"{name}\"/>")).then_some(body)
    })
}

/// 补上样式来源中缺的样式：编号加上 `DBLens` 前缀，以免与已有的编号冲突。返回改后的 styles.xml 和名字 → 编号。
fn complete_styles(styles: &str) -> (String, HashMap<String, String>) {
    let mut ids = style_ids(styles);
    let mut added = String::new();
    for name in STYLE_NAMES {
        if ids.contains_key(name) {
            continue;
        }
        let Some(body) = builtin_style(name) else { continue };
        let id_at = body.find("w:styleId=\"").map(|i| i + "w:styleId=\"".len()).unwrap_or(0);
        let old_id = body[id_at..].split('"').next().unwrap_or_default();
        let new_id =
            if old_id.starts_with("DBLens") { old_id.to_string() } else { format!("DBLens{old_id}") };
        let fixed = body.replace(&format!("w:styleId=\"{old_id}\""), &format!("w:styleId=\"{new_id}\""));
        // 基于的样式在样式来源中也可能缺：一律基于正文
        let fixed =
            fixed.replace("<w:basedOn w:val=\"Normal\"/>", "").replace("<w:next w:val=\"Normal\"/>", "");
        added.push_str("<w:style ");
        added.push_str(&fixed);
        ids.insert(name.to_string(), new_id);
    }
    let out = match styles.rfind("</w:styles>") {
        Some(i) if !added.is_empty() => format!("{}{added}{}", &styles[..i], &styles[i..]),
        _ => styles.to_string(),
    };
    (out, ids)
}

/// XML 中不能出现的控制字符去掉。
fn clean(text: &str) -> String {
    text.chars().filter(|c| !c.is_control() || matches!(c, '\t' | '\n' | '\r')).collect()
}

/// 正文的写出。
struct Body<'a> {
    lang: Lang,
    styles: &'a HashMap<String, String>,
    /// 版心宽、高（twip）。
    width: u64,
    height: u64,
    xml: String,
    bookmark: u32,
    /// 图片：(关系编号, 媒体文件名, PNG)。
    images: Vec<(String, String, Vec<u8>)>,
}

impl Body<'_> {
    fn t(&self, key: &str) -> String {
        label(self.lang, key)
    }

    fn style(&self, name: &str) -> &str {
        self.styles.get(name).map(String::as_str).unwrap_or("Normal")
    }

    /// 文字 run：换行写成 `<w:br/>`。
    fn runs(text: &str, rpr: &str) -> String {
        let mut out = String::new();
        for (i, line) in clean(text).replace("\r\n", "\n").split('\n').enumerate() {
            if i > 0 {
                let _ = write!(out, "<w:r>{rpr}<w:br/></w:r>");
            }
            let _ = write!(out, "<w:r>{rpr}<w:t xml:space=\"preserve\">{}</w:t></w:r>", escape(line));
        }
        out
    }

    fn para(&mut self, style: Option<&str>, text: &str) {
        let ppr = style
            .map(|s| format!("<w:pPr><w:pStyle w:val=\"{}\"/></w:pPr>", self.style(s)))
            .unwrap_or_default();
        let _ = writeln!(self.xml, "<w:p>{ppr}{}</w:p>", Self::runs(text, ""));
    }

    fn heading(&mut self, level: u8, text: &str, anchor: Option<&str>) {
        let style = self.style(&format!("heading {level}")).to_string();
        let mut inner = Self::runs(text, "");
        if let Some(a) = anchor {
            self.bookmark += 1;
            let id = self.bookmark;
            inner = format!(
                "<w:bookmarkStart w:id=\"{id}\" w:name=\"{}\"/>{inner}<w:bookmarkEnd w:id=\"{id}\"/>",
                escape(a)
            );
        }
        let _ = writeln!(self.xml, "<w:p><w:pPr><w:pStyle w:val=\"{style}\"/></w:pPr>{inner}</w:p>");
    }

    /// 链接到文档中的书签。
    fn link(&self, text: &str, anchor: Option<&str>) -> String {
        match anchor {
            Some(a) => format!(
                "<w:hyperlink w:anchor=\"{}\" w:history=\"1\">{}</w:hyperlink>",
                escape(a),
                Self::runs(
                    text,
                    &format!("<w:rPr><w:rStyle w:val=\"{}\"/></w:rPr>", self.style("hyperlink"))
                )
            ),
            None => Self::runs(text, ""),
        }
    }

    /// 表格：`widths` 是各列的相对宽度，单元格内容是已写好的 run（`Cell::Xml`）或文字。
    fn table(&mut self, headers: &[String], widths: &[u64], rows: Vec<Vec<Cell>>) {
        let total: u64 = widths.iter().sum::<u64>().max(1);
        let cols: Vec<u64> = widths.iter().map(|w| self.width * w / total).collect();
        let text_style = self.style("dblens table text").to_string();
        let grid = self.style("table grid").to_string();
        let _ = write!(
            self.xml,
            "<w:tbl><w:tblPr><w:tblStyle w:val=\"{grid}\"/><w:tblW w:w=\"{}\" w:type=\"dxa\"/><w:tblLayout w:type=\"fixed\"/></w:tblPr><w:tblGrid>",
            self.width
        );
        for c in &cols {
            let _ = write!(self.xml, "<w:gridCol w:w=\"{c}\"/>");
        }
        self.xml.push_str("</w:tblGrid>\n");
        let cell = |xml: &mut String, w: u64, content: &str, header: bool| {
            let shd = if header { "<w:shd w:val=\"clear\" w:color=\"auto\" w:fill=\"F2F2F4\"/>" } else { "" };
            let _ = write!(
                xml,
                "<w:tc><w:tcPr><w:tcW w:w=\"{w}\" w:type=\"dxa\"/>{shd}</w:tcPr><w:p><w:pPr><w:pStyle w:val=\"{text_style}\"/></w:pPr>{content}</w:p></w:tc>"
            );
        };
        self.xml.push_str("<w:tr><w:trPr><w:tblHeader/></w:trPr>");
        for (h, w) in headers.iter().zip(&cols) {
            cell(&mut self.xml, *w, &Self::runs(h, "<w:rPr><w:b/></w:rPr>"), true);
        }
        self.xml.push_str("</w:tr>\n");
        for row in rows {
            self.xml.push_str("<w:tr><w:trPr><w:cantSplit/></w:trPr>");
            for (c, w) in row.iter().zip(&cols) {
                let content = match c {
                    Cell::Text(t) => Self::runs(t, ""),
                    Cell::Xml(x) => x.clone(),
                };
                cell(&mut self.xml, *w, &content, false);
            }
            self.xml.push_str("</w:tr>\n");
        }
        self.xml.push_str("</w:tbl>\n");
        // 两张表之间要有段落，否则 Word 会把它们合成一张
        self.xml.push_str("<w:p/>\n");
    }

    fn image(&mut self, png: &[u8], width: u32, height: u32, title: &str) {
        let n = self.images.len() + 1;
        let rel = format!("rIdDBLensImage{n}");
        let file = format!("dblens_image{n}.png");
        let mut cx = (f64::from(width) * EMU_PER_PX) as u64;
        let mut cy = (f64::from(height) * EMU_PER_PX) as u64;
        let max_w = self.width * EMU_PER_TWIP;
        // 高度留出题注的位置
        let max_h = self.height * EMU_PER_TWIP * 85 / 100;
        if cx > max_w {
            cy = cy * max_w / cx.max(1);
            cx = max_w;
        }
        if cy > max_h {
            cx = cx * max_h / cy.max(1);
            cy = max_h;
        }
        let _ = writeln!(
            self.xml,
            "<w:p><w:pPr><w:jc w:val=\"center\"/></w:pPr><w:r><w:drawing><wp:inline distT=\"0\" distB=\"0\" distL=\"0\" distR=\"0\"><wp:extent cx=\"{cx}\" cy=\"{cy}\"/><wp:docPr id=\"{n}\" name=\"{name}\"/><wp:cNvGraphicFramePr><a:graphicFrameLocks noChangeAspect=\"1\"/></wp:cNvGraphicFramePr><a:graphic><a:graphicData uri=\"http://schemas.openxmlformats.org/drawingml/2006/picture\"><pic:pic><pic:nvPicPr><pic:cNvPr id=\"{n}\" name=\"{file}\"/><pic:cNvPicPr/></pic:nvPicPr><pic:blipFill><a:blip r:embed=\"{rel}\"/><a:stretch><a:fillRect/></a:stretch></pic:blipFill><pic:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"{cx}\" cy=\"{cy}\"/></a:xfrm><a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></pic:spPr></pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p>",
            name = escape(clean(title)),
        );
        self.images.push((rel, file, png.to_vec()));
    }

    fn code(&mut self, text: &str) {
        let style = self.style("dblens code").to_string();
        for line in clean(text).replace("\r\n", "\n").split('\n') {
            let _ = writeln!(
                self.xml,
                "<w:p><w:pPr><w:pStyle w:val=\"{style}\"/></w:pPr><w:r><w:t xml:space=\"preserve\">{}</w:t></w:r></w:p>",
                escape(line)
            );
        }
        self.xml.push_str("<w:p/>\n");
    }

    fn toc(&mut self) {
        let heading = self.style("toc heading").to_string();
        let _ = writeln!(
            self.xml,
            "<w:p><w:pPr><w:pStyle w:val=\"{heading}\"/></w:pPr>{}</w:p>",
            Self::runs(&self.t("doc.contents"), "")
        );
        let _ = writeln!(
            self.xml,
            "<w:p><w:r><w:fldChar w:fldCharType=\"begin\" w:dirty=\"true\"/></w:r><w:r><w:instrText xml:space=\"preserve\"> TOC \\o \"1-2\" \\h \\z \\u </w:instrText></w:r><w:r><w:fldChar w:fldCharType=\"separate\"/></w:r>{}<w:r><w:fldChar w:fldCharType=\"end\"/></w:r></w:p>",
            Self::runs(&self.t("doc.toc_hint"), "")
        );
    }
}

enum Cell {
    Text(String),
    Xml(String),
}

fn text(s: impl Into<String>) -> Cell {
    Cell::Text(s.into())
}

fn opt(s: &Option<String>) -> Cell {
    Cell::Text(s.clone().unwrap_or_default())
}

fn write_keys(b: &mut Body, title: &str, keys: &[DocKey]) {
    if keys.is_empty() {
        return;
    }
    b.heading(3, title, None);
    let headers = [
        "doc.col.name",
        "doc.col.index_columns",
        "doc.col.references",
        "doc.col.on_delete",
        "doc.col.on_update",
        "doc.col.comment",
    ]
    .map(|k| b.t(k));
    let rows = keys
        .iter()
        .map(|k| {
            let target = format!("{}.{}", k.ref_schema, k.ref_table);
            let link = format!(
                "{}{}",
                b.link(&target, k.ref_anchor.as_deref()),
                Body::runs(&format!(" ({})", k.ref_columns), "")
            );
            vec![
                opt(&k.name),
                text(&k.columns),
                Cell::Xml(link),
                opt(&k.on_delete),
                opt(&k.on_update),
                opt(&k.comment),
            ]
        })
        .collect();
    b.table(&headers, &[3, 3, 4, 2, 2, 3], rows);
}

fn write_table(b: &mut Body, t: &DocTable) {
    b.heading(2, &format!("{}.{}", t.schema, t.name), Some(&t.anchor));
    if let Some(c) = &t.comment {
        b.para(None, c);
    }
    if let Some(p) = &t.partition {
        b.para(None, &format!("{}: {p}", b.t("doc.partition")));
    }
    let headers = [
        "doc.col.no",
        "doc.col.column",
        "doc.col.type",
        "doc.col.nullable",
        "doc.col.default",
        "doc.col.flags",
        "doc.col.comment",
    ]
    .map(|k| b.t(k));
    let rows = t
        .columns
        .iter()
        .map(|c| {
            let flags: Vec<String> = c.flags.iter().map(|f| b.t(&format!("doc.flag.{f}"))).collect();
            vec![
                text(c.ordinal.to_string()),
                text(&c.name),
                text(&c.data_type),
                text(b.t(if c.nullable { "doc.yes" } else { "doc.no" })),
                opt(&c.default),
                text(flags.join(", ")),
                opt(&c.comment),
            ]
        })
        .collect();
    b.table(&headers, &[2, 5, 5, 2, 4, 3, 7], rows);
    if !t.indexes.is_empty() {
        b.heading(3, &b.t("doc.sec.indexes"), None);
        let headers =
            ["doc.col.name", "doc.col.index_columns", "doc.col.unique", "doc.col.method"].map(|k| b.t(k));
        let rows = t
            .indexes
            .iter()
            .map(|i| {
                vec![
                    text(&i.name),
                    text(&i.columns),
                    text(b.t(if i.unique { "doc.yes" } else { "doc.no" })),
                    opt(&i.method),
                ]
            })
            .collect();
        b.table(&headers, &[4, 5, 2, 2], rows);
    }
    if !t.constraints.is_empty() {
        b.heading(3, &b.t("doc.sec.constraints"), None);
        let headers = ["doc.col.kind", "doc.col.name", "doc.col.detail"].map(|k| b.t(k));
        let rows = t
            .constraints
            .iter()
            .map(|k| vec![text(b.t(&format!("doc.kind.{}", k.kind))), opt(&k.name), text(&k.detail)])
            .collect();
        b.table(&headers, &[2, 4, 7], rows);
    }
    let fk = b.t("doc.sec.foreign_keys");
    write_keys(b, &fk, &t.foreign_keys);
    let lk = b.t("doc.sec.logical_keys");
    write_keys(b, &lk, &t.logical_keys);
    if !t.triggers.is_empty() {
        b.heading(3, &b.t("doc.sec.triggers"), None);
        let headers = ["doc.col.name", "doc.col.timing", "doc.col.events"].map(|k| b.t(k));
        let rows = t.triggers.iter().map(|g| vec![text(&g.name), opt(&g.timing), text(&g.events)]).collect();
        b.table(&headers, &[4, 2, 4], rows);
    }
    if !t.referenced_by.is_empty() {
        b.heading(3, &b.t("doc.sec.referenced_by"), None);
        let headers =
            ["doc.col.table", "doc.col.index_columns", "doc.col.references", "doc.col.kind", "doc.col.name"]
                .map(|k| b.t(k));
        let rows = t
            .referenced_by
            .iter()
            .map(|r| {
                vec![
                    Cell::Xml(b.link(&format!("{}.{}", r.schema, r.table), r.anchor.as_deref())),
                    text(&r.columns),
                    text(&r.ref_columns),
                    text(b.t(&format!("doc.kind.{}", r.kind))),
                    opt(&r.name),
                ]
            })
            .collect();
        b.table(&headers, &[4, 3, 3, 2, 3], rows);
    }
}

fn write_body(b: &mut Body, c: &DocContent) {
    let title_style = b.style("title").to_string();
    let _ = writeln!(
        b.xml,
        "<w:p><w:pPr><w:pStyle w:val=\"{title_style}\"/></w:pPr>{}</w:p>",
        Body::runs(&c.info.title, "")
    );
    let kind = b.t(if c.kind == DocKind::Dictionary { "doc.dictionary" } else { "doc.design" });
    b.para(Some("caption"), &kind);
    for (key, value) in [
        ("doc.version", c.info.version.clone()),
        ("doc.author", c.info.author.clone()),
        ("doc.generated", Some(c.info.generated.clone())),
        ("doc.source", Some(c.info.source.clone())),
    ] {
        if let Some(v) = value.filter(|v| !v.is_empty()) {
            b.para(None, &format!("{}: {v}", b.t(key)));
        }
    }
    if c.kind == DocKind::Dictionary
        && let Some(d) = &c.info.description
    {
        b.para(None, d);
    }
    b.toc();
    if c.kind == DocKind::Design {
        b.heading(1, &b.t("doc.overview"), None);
        if let Some(d) = &c.info.description {
            b.para(None, d);
        }
        let s = &c.stats;
        let mut rows = vec![
            vec![text(b.t("doc.stat.schemas")), text(s.schemas.to_string())],
            vec![text(b.t("doc.stat.tables")), text(s.tables.to_string())],
            vec![text(b.t("doc.stat.columns")), text(s.columns.to_string())],
            vec![text(b.t("doc.stat.foreign_keys")), text(s.foreign_keys.to_string())],
            vec![text(b.t("doc.stat.logical_keys")), text(s.logical_keys.to_string())],
        ];
        if s.views > 0 {
            rows.push(vec![text(b.t("doc.stat.views")), text(s.views.to_string())]);
        }
        b.table(&[b.t("doc.stats"), String::new()], &[3, 1], rows);
        if !c.diagrams.is_empty() {
            b.heading(1, &b.t("doc.diagrams"), None);
            for d in &c.diagrams {
                b.heading(2, &d.title, None);
                if let Some(desc) = &d.description {
                    b.para(None, desc);
                }
                b.image(&d.png, d.width, d.height, &d.title);
                if d.names_only {
                    b.para(Some("caption"), &b.t("doc.names_only"));
                }
            }
        }
        if !c.relations.is_empty() {
            b.heading(1, &b.t("doc.relations"), None);
            let headers = [
                "doc.col.from",
                "doc.col.index_columns",
                "doc.col.to",
                "doc.col.index_columns",
                "doc.col.kind",
                "doc.col.name",
            ]
            .map(|k| b.t(k));
            let rows = c
                .relations
                .iter()
                .map(|r| {
                    vec![
                        Cell::Xml(
                            b.link(&format!("{}.{}", r.from_schema, r.from_table), r.from_anchor.as_deref()),
                        ),
                        text(&r.from_columns),
                        Cell::Xml(b.link(&format!("{}.{}", r.to_schema, r.to_table), r.to_anchor.as_deref())),
                        text(&r.to_columns),
                        text(b.t(&format!("doc.kind.{}", r.kind))),
                        opt(&r.name),
                    ]
                })
                .collect();
            b.table(&headers, &[4, 3, 4, 3, 2, 3], rows);
        }
    }
    b.heading(1, &b.t("doc.tables"), None);
    for s in &c.schemas {
        b.heading(2, &s.name, None);
        let headers = ["doc.col.no", "doc.col.table", "doc.col.comment", "doc.col.columns"].map(|k| b.t(k));
        let rows = s
            .tables
            .iter()
            .enumerate()
            .map(|(i, t)| {
                vec![
                    text((i + 1).to_string()),
                    Cell::Xml(b.link(&t.name, Some(&t.anchor))),
                    opt(&t.comment),
                    text(t.columns.len().to_string()),
                ]
            })
            .collect();
        b.table(&headers, &[2, 6, 8, 2], rows);
    }
    if c.stats.views > 0 {
        b.heading(1, &b.t("doc.views"), None);
        for s in &c.schemas {
            for v in &s.views {
                b.heading(2, &format!("{}.{}", s.name, v.name), Some(&v.anchor));
                if let Some(cm) = &v.comment {
                    b.para(None, cm);
                }
                let headers =
                    ["doc.col.no", "doc.col.column", "doc.col.type", "doc.col.comment"].map(|k| b.t(k));
                let rows = v
                    .columns
                    .iter()
                    .map(|col| {
                        vec![
                            text(col.ordinal.to_string()),
                            text(&col.name),
                            text(&col.data_type),
                            opt(&col.comment),
                        ]
                    })
                    .collect();
                b.table(&headers, &[2, 5, 5, 8], rows);
                if let Some(def) = &v.definition {
                    b.heading(3, &b.t("doc.sec.definition"), None);
                    b.code(def);
                }
            }
        }
    }
    b.heading(1, &b.t("doc.details"), None);
    for s in &c.schemas {
        for t in &s.tables {
            write_table(b, t);
        }
    }
    if !c.unreadable.is_empty() {
        b.heading(1, &b.t("doc.unreadable"), None);
        let headers = ["doc.col.schema", "doc.col.table", "doc.col.file", "doc.col.error"].map(|k| b.t(k));
        let rows = c
            .unreadable
            .iter()
            .map(|u| vec![text(&u.schema), text(&u.name), text(&u.path), text(&u.message)])
            .collect();
        b.table(&headers, &[2, 3, 5, 8], rows);
    }
}

/// 在 `<w:settings …>` 开头加上打开时更新字段（目录的页码由 Word 算）。
fn update_fields(settings: &str) -> String {
    if settings.contains("<w:updateFields") {
        return settings.to_string();
    }
    match settings.find("<w:settings").and_then(|i| settings[i..].find('>').map(|j| i + j + 1)) {
        Some(at) => format!("{}<w:updateFields w:val=\"true\"/>{}", &settings[..at], &settings[at..]),
        None => settings.to_string(),
    }
}

/// 在关系文件末尾加关系。
fn add_rels(rels: &str, items: &[(String, &str, String)]) -> String {
    let mut extra = String::new();
    for (id, kind, target) in items {
        let _ = write!(extra, "<Relationship Id=\"{id}\" Type=\"{kind}\" Target=\"{target}\"/>");
    }
    match rels.rfind("</Relationships>") {
        Some(i) => format!("{}{extra}{}", &rels[..i], &rels[i..]),
        None => rels.to_string(),
    }
}

/// 写出 docx。`reference` 为 None 时用内置的样式来源。
pub fn write(content: &DocContent, reference: Option<&[u8]>, lang: Lang) -> Result<Vec<u8>> {
    let builtin;
    let bytes = match reference {
        Some(b) => b,
        None => {
            builtin = builtin_reference();
            &builtin
        }
    };
    let mut r = Reference::read(bytes)?;
    let doc = r.get("word/document.xml").unwrap_or_default().to_string();
    let (styles, ids) = complete_styles(r.get("word/styles.xml").unwrap_or_default());
    let sect = section(&doc);
    let (width, height) = text_area(&sect);
    let mut body =
        Body { lang, styles: &ids, width, height, xml: String::new(), bookmark: 0, images: vec![] };
    write_body(&mut body, content);

    // 正文：沿用样式来源根元素上的命名空间声明，补上图片要用的
    let root_end = doc.find("<w:body").unwrap_or(0);
    let mut head = doc[..root_end].to_string();
    for (prefix, uri) in [
        ("w", NS_W),
        ("r", "http://schemas.openxmlformats.org/officeDocument/2006/relationships"),
        ("wp", "http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing"),
        ("a", "http://schemas.openxmlformats.org/drawingml/2006/main"),
        ("pic", "http://schemas.openxmlformats.org/drawingml/2006/picture"),
    ] {
        if !head.contains(&format!("xmlns:{prefix}="))
            && let Some(i) = head.find("<w:document")
        {
            head.insert_str(i + "<w:document".len(), &format!(" xmlns:{prefix}=\"{uri}\""));
        }
    }
    if head.is_empty() {
        head = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<w:document xmlns:w=\"{NS_W}\">"
        );
    }
    let document = format!("{}\n<w:body>\n{}{sect}\n</w:body>\n</w:document>\n", head.trim_end(), body.xml);
    r.set("word/document.xml", document.into_bytes());
    r.set("word/styles.xml", styles.into_bytes());

    let mut rels = r.get("word/_rels/document.xml.rels").unwrap_or_default().to_string();
    let mut types = r.get("[Content_Types].xml").unwrap_or_default().to_string();
    let mut new_rels: Vec<(String, &str, String)> =
        body.images.iter().map(|(id, file, _)| (id.clone(), REL_IMAGE, format!("media/{file}"))).collect();
    match r.get("word/settings.xml") {
        Some(s) => {
            let s = update_fields(s);
            r.set("word/settings.xml", s.into_bytes());
        }
        None => {
            r.set("word/settings.xml", update_fields(include_str!("docx/settings.xml")).into_bytes());
            new_rels.push(("rIdDBLensSettings".into(), REL_SETTINGS, "settings.xml".into()));
            if let Some(i) = types.rfind("</Types>") {
                types.insert_str(i, "<Override PartName=\"/word/settings.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.settings+xml\"/>");
            }
        }
    }
    rels = add_rels(&rels, &new_rels);
    if !body.images.is_empty()
        && !types.to_lowercase().contains("extension=\"png\"")
        && let Some(i) = types.find("<Default ")
    {
        types.insert_str(i, "<Default Extension=\"png\" ContentType=\"image/png\"/>");
    }
    r.set("word/_rels/document.xml.rels", rels.into_bytes());
    r.set("[Content_Types].xml", types.into_bytes());
    for (_, file, png) in body.images {
        r.set(&format!("word/media/{file}"), png);
    }

    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    let opts = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    for (name, data) in &r.parts {
        zip.start_file(name.as_str(), opts).map_err(write_err)?;
        zip.write_all(data).map_err(write_err)?;
    }
    zip.finish().map(Cursor::into_inner).map_err(write_err)
}

#[cfg(test)]
mod tests {
    use super::super::content::tests::{opts, sample};
    use super::super::content::{DocDiagram, build};
    use super::*;

    fn unzip(bytes: &[u8]) -> HashMap<String, String> {
        let mut z = ZipArchive::new(Cursor::new(bytes)).unwrap();
        let mut out = HashMap::new();
        for i in 0..z.len() {
            let mut f = z.by_index(i).unwrap();
            let mut data = Vec::new();
            f.read_to_end(&mut data).unwrap();
            out.insert(f.name().unwrap().to_string(), String::from_utf8_lossy(&data).to_string());
        }
        out
    }

    fn reference_with(styles: &str, document: &str) -> Vec<u8> {
        let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
        for (name, text) in BUILTIN_PARTS {
            let text = match name {
                "word/styles.xml" => styles,
                "word/document.xml" => document,
                _ => text,
            };
            zip.start_file(name, SimpleFileOptions::default()).unwrap();
            zip.write_all(text.as_bytes()).unwrap();
        }
        zip.finish().unwrap().into_inner()
    }

    fn diagram() -> DocDiagram {
        DocDiagram {
            schema: "shop".into(),
            name: "_all".into(),
            title: "shop".into(),
            description: None,
            names_only: false,
            file: "01-shop-_all.png".into(),
            src: String::new(),
            width: 4000,
            height: 1000,
            png: vec![0x89, b'P', b'N', b'G'],
        }
    }

    #[test]
    fn builtin_reference_document() {
        let c = build(opts(DocKind::Design), sample(), vec![diagram()]);
        let parts = unzip(&write(&c, None, Lang::ZhCn).unwrap());
        let doc = &parts["word/document.xml"];
        insta::assert_snapshot!("docx_design_zh", doc);
        // 图片：关系、内容类型、媒体文件
        assert!(parts["word/_rels/document.xml.rels"].contains("Id=\"rIdDBLensImage1\""));
        assert!(parts["[Content_Types].xml"].contains("Extension=\"png\""));
        assert!(parts["word/media/dblens_image1.png"].ends_with("PNG"));
        // 4000 像素按 2 倍画，宽 4000 × 4762.5 EMU 超过版心（11906 − 2 × 1247 twip），按版心宽缩放
        assert!(doc.contains("<wp:extent cx=\"5976620\" cy=\"1494155\"/>"), "{doc}");
        assert!(parts["word/settings.xml"].contains("<w:updateFields w:val=\"true\"/>"));
        // 每个 XML 部件都是格式正确的 XML
        for (name, text) in parts.iter().filter(|(n, _)| n.ends_with(".xml") || n.ends_with(".rels")) {
            let mut reader = quick_xml::Reader::from_str(text);
            reader.config_mut().check_end_names = true;
            loop {
                match reader.read_event() {
                    Ok(Event::Eof) => break,
                    Ok(_) => {}
                    Err(e) => panic!("{name}: {e}"),
                }
            }
        }
        // 链接到书签、书签名
        assert!(doc.contains("<w:hyperlink w:anchor=\"t4\""));
        assert!(doc.contains("w:name=\"t1\""));
    }

    #[test]
    fn reference_with_localized_style_ids_and_missing_styles() {
        // 中文版 Word：样式编号是数字，名字仍是英文的内部名；样式来源中没有表格样式和代码样式
        let styles = r#"<?xml version="1.0" encoding="UTF-8"?><w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:style w:type="paragraph" w:default="1" w:styleId="a"><w:name w:val="Normal"/></w:style><w:style w:type="paragraph" w:styleId="1"><w:name w:val="heading 1"/></w:style><w:style w:type="paragraph" w:styleId="2"><w:name w:val="heading 2"/></w:style><w:style w:type="paragraph" w:styleId="a3"><w:name w:val="Title"/></w:style></w:styles>"#;
        // 横向 A4，页边距 1 英寸
        let document = r#"<?xml version="1.0" encoding="UTF-8"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><w:body><w:p><w:r><w:t>旧内容</w:t></w:r></w:p><w:sectPr><w:headerReference w:type="default" r:id="rId9"/><w:pgSz w:w="16838" w:h="11906" w:orient="landscape"/><w:pgMar w:top="1440" w:right="1440" w:bottom="1440" w:left="1440"/></w:sectPr></w:body></w:document>"#;
        let c = build(opts(DocKind::Design), sample(), vec![diagram()]);
        let parts = unzip(&write(&c, Some(&reference_with(styles, document)), Lang::En).unwrap());
        let doc = &parts["word/document.xml"];
        assert!(doc.contains("<w:pStyle w:val=\"1\"/>") && doc.contains("<w:pStyle w:val=\"a3\"/>"));
        assert!(!doc.contains("旧内容"), "原来的正文被替换");
        assert!(doc.contains("<w:headerReference w:type=\"default\" r:id=\"rId9\"/>"), "保留节属性");
        assert!(doc.contains("<w:tblStyle w:val=\"DBLensTableGrid\"/>"));
        assert!(doc.contains("xmlns:wp="), "补上图片的命名空间");
        // 版心宽 16838 − 2 × 1440 = 13958 twip
        assert!(doc.contains("<w:tblW w:w=\"13958\""));
        let s = &parts["word/styles.xml"];
        assert!(s.contains("w:styleId=\"DBLensTableGrid\"") && s.contains("w:styleId=\"DBLensCode\""));
        assert!(!s.contains("w:styleId=\"DBLensHeading1\""), "已有的样式不重复补");
    }

    #[test]
    fn rejects_invalid_reference_and_strips_control_chars() {
        assert!(matches!(check_reference(b"not a zip"), Err(Error::Invalid { .. })));
        let mut only_doc = ZipWriter::new(Cursor::new(Vec::new()));
        only_doc.start_file("word/document.xml", SimpleFileOptions::default()).unwrap();
        only_doc.write_all(b"<w:document/>").unwrap();
        assert!(check_reference(&only_doc.finish().unwrap().into_inner()).is_err());
        assert!(check_reference(&builtin_reference()).is_ok());
        let mut o = opts(DocKind::Dictionary);
        o.info.title = "a\u{1}b<c>".into();
        let c = build(o, sample(), vec![]);
        let doc = &unzip(&write(&c, None, Lang::En).unwrap())["word/document.xml"];
        assert!(doc.contains(">ab&lt;c&gt;<"));
    }
}
```

- [ ] **Step 5: 新建 `crates/dblens-service/src/document/docx/content_types.xml`**

```
<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Default Extension="png" ContentType="image/png"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml"/><Override PartName="/word/settings.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.settings+xml"/><Override PartName="/docProps/core.xml" ContentType="application/vnd.openxmlformats-package.core-properties+xml"/></Types>
```

- [ ] **Step 6: 新建 `crates/dblens-service/src/document/docx/core.xml`**

```
<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties" xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:dcterms="http://purl.org/dc/terms/" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"><dc:creator>DBLens</dc:creator></cp:coreProperties>
```

- [ ] **Step 7: 新建 `crates/dblens-service/src/document/docx/document.xml`**

```
<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><w:body><w:p/><w:sectPr><w:pgSz w:w="11906" w:h="16838"/><w:pgMar w:top="1440" w:right="1247" w:bottom="1440" w:left="1247" w:header="851" w:footer="992" w:gutter="0"/><w:cols w:space="425"/></w:sectPr></w:body></w:document>
```

- [ ] **Step 8: 新建 `crates/dblens-service/src/document/docx/document_rels.xml`**

```
<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/settings" Target="settings.xml"/></Relationships>
```

- [ ] **Step 9: 新建 `crates/dblens-service/src/document/docx/rels.xml`**

```
<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties" Target="docProps/core.xml"/></Relationships>
```

- [ ] **Step 10: 新建 `crates/dblens-service/src/document/docx/settings.xml`**

```
<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:settings xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:defaultTabStop w:val="420"/><w:characterSpacingControl w:val="compressPunctuation"/><w:compat><w:compatSetting w:name="compatibilityMode" w:uri="http://schemas.microsoft.com/office/word" w:val="15"/></w:compat></w:settings>
```

- [ ] **Step 11: 新建 `crates/dblens-service/src/document/docx/styles.xml`**

```
<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
<w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:ascii="Calibri" w:eastAsia="等线" w:hAnsi="Calibri" w:cs="Times New Roman"/><w:sz w:val="21"/><w:szCs w:val="21"/><w:lang w:val="en-US" w:eastAsia="zh-CN"/></w:rPr></w:rPrDefault><w:pPrDefault><w:pPr><w:spacing w:after="80" w:line="276" w:lineRule="auto"/></w:pPr></w:pPrDefault></w:docDefaults>
<w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:name w:val="Normal"/><w:qFormat/></w:style>
<w:style w:type="paragraph" w:styleId="Title"><w:name w:val="Title"/><w:basedOn w:val="Normal"/><w:next w:val="Normal"/><w:qFormat/><w:pPr><w:spacing w:before="240" w:after="120"/><w:jc w:val="center"/></w:pPr><w:rPr><w:b/><w:sz w:val="40"/><w:szCs w:val="40"/></w:rPr></w:style>
<w:style w:type="paragraph" w:styleId="Heading1"><w:name w:val="heading 1"/><w:basedOn w:val="Normal"/><w:next w:val="Normal"/><w:qFormat/><w:pPr><w:keepNext/><w:pageBreakBefore/><w:spacing w:before="240" w:after="120"/><w:outlineLvl w:val="0"/></w:pPr><w:rPr><w:b/><w:sz w:val="32"/><w:szCs w:val="32"/></w:rPr></w:style>
<w:style w:type="paragraph" w:styleId="Heading2"><w:name w:val="heading 2"/><w:basedOn w:val="Normal"/><w:next w:val="Normal"/><w:qFormat/><w:pPr><w:keepNext/><w:spacing w:before="240" w:after="80"/><w:outlineLvl w:val="1"/></w:pPr><w:rPr><w:b/><w:sz w:val="26"/><w:szCs w:val="26"/></w:rPr></w:style>
<w:style w:type="paragraph" w:styleId="Heading3"><w:name w:val="heading 3"/><w:basedOn w:val="Normal"/><w:next w:val="Normal"/><w:qFormat/><w:pPr><w:keepNext/><w:spacing w:before="160" w:after="60"/><w:outlineLvl w:val="2"/></w:pPr><w:rPr><w:b/><w:color w:val="404040"/><w:sz w:val="22"/><w:szCs w:val="22"/></w:rPr></w:style>
<w:style w:type="paragraph" w:styleId="Caption"><w:name w:val="caption"/><w:basedOn w:val="Normal"/><w:next w:val="Normal"/><w:qFormat/><w:pPr><w:jc w:val="center"/></w:pPr><w:rPr><w:color w:val="595959"/><w:sz w:val="18"/><w:szCs w:val="18"/></w:rPr></w:style>
<w:style w:type="paragraph" w:styleId="TOCHeading"><w:name w:val="TOC Heading"/><w:basedOn w:val="Normal"/><w:next w:val="Normal"/><w:qFormat/><w:pPr><w:keepNext/><w:spacing w:before="240" w:after="120"/></w:pPr><w:rPr><w:b/><w:sz w:val="32"/><w:szCs w:val="32"/></w:rPr></w:style>
<w:style w:type="paragraph" w:styleId="TOC1"><w:name w:val="toc 1"/><w:basedOn w:val="Normal"/><w:next w:val="Normal"/><w:pPr><w:spacing w:after="60"/></w:pPr></w:style>
<w:style w:type="paragraph" w:styleId="TOC2"><w:name w:val="toc 2"/><w:basedOn w:val="Normal"/><w:next w:val="Normal"/><w:pPr><w:spacing w:after="40"/><w:ind w:left="420"/></w:pPr></w:style>
<w:style w:type="paragraph" w:customStyle="1" w:styleId="DBLensTableText"><w:name w:val="DBLens Table Text"/><w:basedOn w:val="Normal"/><w:pPr><w:spacing w:after="0" w:line="240" w:lineRule="auto"/></w:pPr><w:rPr><w:sz w:val="18"/><w:szCs w:val="18"/></w:rPr></w:style>
<w:style w:type="paragraph" w:customStyle="1" w:styleId="DBLensCode"><w:name w:val="DBLens Code"/><w:basedOn w:val="Normal"/><w:pPr><w:shd w:val="clear" w:color="auto" w:fill="F4F4F6"/><w:spacing w:after="0" w:line="240" w:lineRule="auto"/></w:pPr><w:rPr><w:rFonts w:ascii="Consolas" w:hAnsi="Consolas"/><w:sz w:val="18"/><w:szCs w:val="18"/></w:rPr></w:style>
<w:style w:type="character" w:styleId="Hyperlink"><w:name w:val="Hyperlink"/><w:rPr><w:color w:val="1F5FBF"/></w:rPr></w:style>
<w:style w:type="table" w:styleId="TableGrid"><w:name w:val="Table Grid"/><w:tblPr><w:tblBorders><w:top w:val="single" w:sz="4" w:space="0" w:color="BFBFC6"/><w:left w:val="single" w:sz="4" w:space="0" w:color="BFBFC6"/><w:bottom w:val="single" w:sz="4" w:space="0" w:color="BFBFC6"/><w:right w:val="single" w:sz="4" w:space="0" w:color="BFBFC6"/><w:insideH w:val="single" w:sz="4" w:space="0" w:color="BFBFC6"/><w:insideV w:val="single" w:sz="4" w:space="0" w:color="BFBFC6"/></w:tblBorders><w:tblCellMar><w:left w:w="80" w:type="dxa"/><w:right w:w="80" w:type="dxa"/></w:tblCellMar></w:tblPr></w:style>
</w:styles>
```

- [ ] **Step 12: 修改 `crates/dblens-service/src/document/labels.json`**

```diff
--- a/crates/dblens-service/src/document/labels.json
+++ b/crates/dblens-service/src/document/labels.json
@@ -262,5 +262,9 @@
   "doc.no": {
     "zh-CN": "否",
     "en": "No"
+  },
+  "doc.toc_hint": {
+    "zh-CN": "打开文档时按提示更新域，或右键这里选“更新域”，即可生成目录。",
+    "en": "Update fields when prompted on opening, or right-click here and choose Update Field, to build the table of contents."
   }
 }
```

- [ ] **Step 13: 修改 `crates/dblens-service/src/document/mod.rs`**

```diff
--- a/crates/dblens-service/src/document/mod.rs
+++ b/crates/dblens-service/src/document/mod.rs
@@ -1,6 +1,7 @@
 //! 文档导出（M4c 设计文档）：数据字典和设计文档，docx / html / markdown / xlsx。
 
 mod content;
+mod docx;
 mod labels;
 mod template;
 
@@ -9,5 +10,6 @@ pub use content::{
     DocReference, DocRelation, DocSchema, DocStats, DocTable, DocTrigger, DocUnreadable, DocView,
     SourceSchema, SourceTable, build, png_size,
 };
+pub use docx::{builtin_reference, check_reference, write as write_docx};
 pub use labels::label;
 pub use template::{TextFormat, check, render};
```

- [ ] **Step 14: 新建 `crates/dblens-service/src/document/snapshots/dblens_service__document__docx__tests__docx_design_zh.snap`**

```text
---
source: crates/dblens-service/src/document/docx.rs
expression: doc
---
<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:pic="http://schemas.openxmlformats.org/drawingml/2006/picture" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing" xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
<w:body>
<w:p><w:pPr><w:pStyle w:val="Title"/></w:pPr><w:r><w:t xml:space="preserve">商城</w:t></w:r></w:p>
<w:p><w:pPr><w:pStyle w:val="Caption"/></w:pPr><w:r><w:t xml:space="preserve">设计文档</w:t></w:r></w:p>
<w:p><w:r><w:t xml:space="preserve">版本: 1.0</w:t></w:r></w:p>
<w:p><w:r><w:t xml:space="preserve">作者: 张三</w:t></w:r></w:p>
<w:p><w:r><w:t xml:space="preserve">生成时间: 2026-10-28 10:00</w:t></w:r></w:p>
<w:p><w:r><w:t xml:space="preserve">来源: shop-model</w:t></w:r></w:p>
<w:p><w:pPr><w:pStyle w:val="TOCHeading"/></w:pPr><w:r><w:t xml:space="preserve">目录</w:t></w:r></w:p>
<w:p><w:r><w:fldChar w:fldCharType="begin" w:dirty="true"/></w:r><w:r><w:instrText xml:space="preserve"> TOC \o "1-2" \h \z \u </w:instrText></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r><w:r><w:t xml:space="preserve">打开文档时按提示更新域，或右键这里选“更新域”，即可生成目录。</w:t></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r></w:p>
<w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t xml:space="preserve">概述</w:t></w:r></w:p>
<w:p><w:r><w:t xml:space="preserve">测试模型</w:t></w:r></w:p>
<w:tbl><w:tblPr><w:tblStyle w:val="TableGrid"/><w:tblW w:w="9412" w:type="dxa"/><w:tblLayout w:type="fixed"/></w:tblPr><w:tblGrid><w:gridCol w:w="7059"/><w:gridCol w:w="2353"/></w:tblGrid>
<w:tr><w:trPr><w:tblHeader/></w:trPr><w:tc><w:tcPr><w:tcW w:w="7059" w:type="dxa"/><w:shd w:val="clear" w:color="auto" w:fill="F2F2F4"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:rPr><w:b/></w:rPr><w:t xml:space="preserve">统计</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="2353" w:type="dxa"/><w:shd w:val="clear" w:color="auto" w:fill="F2F2F4"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:rPr><w:b/></w:rPr><w:t xml:space="preserve"></w:t></w:r></w:p></w:tc></w:tr>
<w:tr><w:trPr><w:cantSplit/></w:trPr><w:tc><w:tcPr><w:tcW w:w="7059" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">Schema 数</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="2353" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">2</w:t></w:r></w:p></w:tc></w:tr>
<w:tr><w:trPr><w:cantSplit/></w:trPr><w:tc><w:tcPr><w:tcW w:w="7059" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">表数</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="2353" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">4</w:t></w:r></w:p></w:tc></w:tr>
<w:tr><w:trPr><w:cantSplit/></w:trPr><w:tc><w:tcPr><w:tcW w:w="7059" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">列数</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="2353" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">13</w:t></w:r></w:p></w:tc></w:tr>
<w:tr><w:trPr><w:cantSplit/></w:trPr><w:tc><w:tcPr><w:tcW w:w="7059" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">外键数</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="2353" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">2</w:t></w:r></w:p></w:tc></w:tr>
<w:tr><w:trPr><w:cantSplit/></w:trPr><w:tc><w:tcPr><w:tcW w:w="7059" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">逻辑外键数</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="2353" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">1</w:t></w:r></w:p></w:tc></w:tr>
<w:tr><w:trPr><w:cantSplit/></w:trPr><w:tc><w:tcPr><w:tcW w:w="7059" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">视图数</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="2353" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">1</w:t></w:r></w:p></w:tc></w:tr>
</w:tbl>
<w:p/>
<w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t xml:space="preserve">ER 图</w:t></w:r></w:p>
<w:p><w:pPr><w:pStyle w:val="Heading2"/></w:pPr><w:r><w:t xml:space="preserve">shop</w:t></w:r></w:p>
<w:p><w:pPr><w:jc w:val="center"/></w:pPr><w:r><w:drawing><wp:inline distT="0" distB="0" distL="0" distR="0"><wp:extent cx="5976620" cy="1494155"/><wp:docPr id="1" name="shop"/><wp:cNvGraphicFramePr><a:graphicFrameLocks noChangeAspect="1"/></wp:cNvGraphicFramePr><a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/picture"><pic:pic><pic:nvPicPr><pic:cNvPr id="1" name="dblens_image1.png"/><pic:cNvPicPr/></pic:nvPicPr><pic:blipFill><a:blip r:embed="rIdDBLensImage1"/><a:stretch><a:fillRect/></a:stretch></pic:blipFill><pic:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="5976620" cy="1494155"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></pic:spPr></pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p>
<w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t xml:space="preserve">表间关系</w:t></w:r></w:p>
<w:tbl><w:tblPr><w:tblStyle w:val="TableGrid"/><w:tblW w:w="9412" w:type="dxa"/><w:tblLayout w:type="fixed"/></w:tblPr><w:tblGrid><w:gridCol w:w="1981"/><w:gridCol w:w="1486"/><w:gridCol w:w="1981"/><w:gridCol w:w="1486"/><w:gridCol w:w="990"/><w:gridCol w:w="1486"/></w:tblGrid>
<w:tr><w:trPr><w:tblHeader/></w:trPr><w:tc><w:tcPr><w:tcW w:w="1981" w:type="dxa"/><w:shd w:val="clear" w:color="auto" w:fill="F2F2F4"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:rPr><w:b/></w:rPr><w:t xml:space="preserve">从</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1486" w:type="dxa"/><w:shd w:val="clear" w:color="auto" w:fill="F2F2F4"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:rPr><w:b/></w:rPr><w:t xml:space="preserve">列</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1981" w:type="dxa"/><w:shd w:val="clear" w:color="auto" w:fill="F2F2F4"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:rPr><w:b/></w:rPr><w:t xml:space="preserve">到</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1486" w:type="dxa"/><w:shd w:val="clear" w:color="auto" w:fill="F2F2F4"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:rPr><w:b/></w:rPr><w:t xml:space="preserve">列</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="990" w:type="dxa"/><w:shd w:val="clear" w:color="auto" w:fill="F2F2F4"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:rPr><w:b/></w:rPr><w:t xml:space="preserve">类别</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1486" w:type="dxa"/><w:shd w:val="clear" w:color="auto" w:fill="F2F2F4"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:rPr><w:b/></w:rPr><w:t xml:space="preserve">名称</w:t></w:r></w:p></w:tc></w:tr>
<w:tr><w:trPr><w:cantSplit/></w:trPr><w:tc><w:tcPr><w:tcW w:w="1981" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:hyperlink w:anchor="t1" w:history="1"><w:r><w:rPr><w:rStyle w:val="Hyperlink"/></w:rPr><w:t xml:space="preserve">shop.customers</w:t></w:r></w:hyperlink></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1486" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">region_id</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1981" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:hyperlink w:anchor="t4" w:history="1"><w:r><w:rPr><w:rStyle w:val="Hyperlink"/></w:rPr><w:t xml:space="preserve">crm.regions</w:t></w:r></w:hyperlink></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1486" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">id</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="990" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">外键</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1486" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">fk_region</w:t></w:r></w:p></w:tc></w:tr>
<w:tr><w:trPr><w:cantSplit/></w:trPr><w:tc><w:tcPr><w:tcW w:w="1981" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:hyperlink w:anchor="t2" w:history="1"><w:r><w:rPr><w:rStyle w:val="Hyperlink"/></w:rPr><w:t xml:space="preserve">shop.items</w:t></w:r></w:hyperlink></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1486" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">order_id</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1981" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:hyperlink w:anchor="t3" w:history="1"><w:r><w:rPr><w:rStyle w:val="Hyperlink"/></w:rPr><w:t xml:space="preserve">shop.orders</w:t></w:r></w:hyperlink></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1486" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">id</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="990" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">逻辑外键</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1486" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">lk_order</w:t></w:r></w:p></w:tc></w:tr>
<w:tr><w:trPr><w:cantSplit/></w:trPr><w:tc><w:tcPr><w:tcW w:w="1981" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:hyperlink w:anchor="t3" w:history="1"><w:r><w:rPr><w:rStyle w:val="Hyperlink"/></w:rPr><w:t xml:space="preserve">shop.orders</w:t></w:r></w:hyperlink></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1486" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">cust_id, cust_code</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1981" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:hyperlink w:anchor="t1" w:history="1"><w:r><w:rPr><w:rStyle w:val="Hyperlink"/></w:rPr><w:t xml:space="preserve">shop.customers</w:t></w:r></w:hyperlink></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1486" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">id, code</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="990" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">外键</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1486" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">fk_cust</w:t></w:r></w:p></w:tc></w:tr>
</w:tbl>
<w:p/>
<w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t xml:space="preserve">表清单</w:t></w:r></w:p>
<w:p><w:pPr><w:pStyle w:val="Heading2"/></w:pPr><w:r><w:t xml:space="preserve">shop</w:t></w:r></w:p>
<w:tbl><w:tblPr><w:tblStyle w:val="TableGrid"/><w:tblW w:w="9412" w:type="dxa"/><w:tblLayout w:type="fixed"/></w:tblPr><w:tblGrid><w:gridCol w:w="1045"/><w:gridCol w:w="3137"/><w:gridCol w:w="4183"/><w:gridCol w:w="1045"/></w:tblGrid>
<w:tr><w:trPr><w:tblHeader/></w:trPr><w:tc><w:tcPr><w:tcW w:w="1045" w:type="dxa"/><w:shd w:val="clear" w:color="auto" w:fill="F2F2F4"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:rPr><w:b/></w:rPr><w:t xml:space="preserve">序号</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="3137" w:type="dxa"/><w:shd w:val="clear" w:color="auto" w:fill="F2F2F4"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:rPr><w:b/></w:rPr><w:t xml:space="preserve">表名</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="4183" w:type="dxa"/><w:shd w:val="clear" w:color="auto" w:fill="F2F2F4"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:rPr><w:b/></w:rPr><w:t xml:space="preserve">注释</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1045" w:type="dxa"/><w:shd w:val="clear" w:color="auto" w:fill="F2F2F4"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:rPr><w:b/></w:rPr><w:t xml:space="preserve">列数</w:t></w:r></w:p></w:tc></w:tr>
<w:tr><w:trPr><w:cantSplit/></w:trPr><w:tc><w:tcPr><w:tcW w:w="1045" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">1</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="3137" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:hyperlink w:anchor="t1" w:history="1"><w:r><w:rPr><w:rStyle w:val="Hyperlink"/></w:rPr><w:t xml:space="preserve">customers</w:t></w:r></w:hyperlink></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="4183" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">客户</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1045" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">3</w:t></w:r></w:p></w:tc></w:tr>
<w:tr><w:trPr><w:cantSplit/></w:trPr><w:tc><w:tcPr><w:tcW w:w="1045" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">2</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="3137" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:hyperlink w:anchor="t2" w:history="1"><w:r><w:rPr><w:rStyle w:val="Hyperlink"/></w:rPr><w:t xml:space="preserve">items</w:t></w:r></w:hyperlink></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="4183" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve"></w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1045" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">3</w:t></w:r></w:p></w:tc></w:tr>
<w:tr><w:trPr><w:cantSplit/></w:trPr><w:tc><w:tcPr><w:tcW w:w="1045" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">3</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="3137" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:hyperlink w:anchor="t3" w:history="1"><w:r><w:rPr><w:rStyle w:val="Hyperlink"/></w:rPr><w:t xml:space="preserve">orders</w:t></w:r></w:hyperlink></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="4183" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve"></w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1045" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">5</w:t></w:r></w:p></w:tc></w:tr>
</w:tbl>
<w:p/>
<w:p><w:pPr><w:pStyle w:val="Heading2"/></w:pPr><w:r><w:t xml:space="preserve">crm</w:t></w:r></w:p>
<w:tbl><w:tblPr><w:tblStyle w:val="TableGrid"/><w:tblW w:w="9412" w:type="dxa"/><w:tblLayout w:type="fixed"/></w:tblPr><w:tblGrid><w:gridCol w:w="1045"/><w:gridCol w:w="3137"/><w:gridCol w:w="4183"/><w:gridCol w:w="1045"/></w:tblGrid>
<w:tr><w:trPr><w:tblHeader/></w:trPr><w:tc><w:tcPr><w:tcW w:w="1045" w:type="dxa"/><w:shd w:val="clear" w:color="auto" w:fill="F2F2F4"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:rPr><w:b/></w:rPr><w:t xml:space="preserve">序号</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="3137" w:type="dxa"/><w:shd w:val="clear" w:color="auto" w:fill="F2F2F4"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:rPr><w:b/></w:rPr><w:t xml:space="preserve">表名</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="4183" w:type="dxa"/><w:shd w:val="clear" w:color="auto" w:fill="F2F2F4"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:rPr><w:b/></w:rPr><w:t xml:space="preserve">注释</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1045" w:type="dxa"/><w:shd w:val="clear" w:color="auto" w:fill="F2F2F4"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:rPr><w:b/></w:rPr><w:t xml:space="preserve">列数</w:t></w:r></w:p></w:tc></w:tr>
<w:tr><w:trPr><w:cantSplit/></w:trPr><w:tc><w:tcPr><w:tcW w:w="1045" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">1</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="3137" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:hyperlink w:anchor="t4" w:history="1"><w:r><w:rPr><w:rStyle w:val="Hyperlink"/></w:rPr><w:t xml:space="preserve">regions</w:t></w:r></w:hyperlink></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="4183" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve"></w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1045" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">2</w:t></w:r></w:p></w:tc></w:tr>
</w:tbl>
<w:p/>
<w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t xml:space="preserve">视图</w:t></w:r></w:p>
<w:p><w:pPr><w:pStyle w:val="Heading2"/></w:pPr><w:bookmarkStart w:id="1" w:name="v1"/><w:r><w:t xml:space="preserve">shop.v_orders</w:t></w:r><w:bookmarkEnd w:id="1"/></w:p>
<w:p><w:r><w:t xml:space="preserve">订单视图</w:t></w:r></w:p>
<w:tbl><w:tblPr><w:tblStyle w:val="TableGrid"/><w:tblW w:w="9412" w:type="dxa"/><w:tblLayout w:type="fixed"/></w:tblPr><w:tblGrid><w:gridCol w:w="941"/><w:gridCol w:w="2353"/><w:gridCol w:w="2353"/><w:gridCol w:w="3764"/></w:tblGrid>
<w:tr><w:trPr><w:tblHeader/></w:trPr><w:tc><w:tcPr><w:tcW w:w="941" w:type="dxa"/><w:shd w:val="clear" w:color="auto" w:fill="F2F2F4"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:rPr><w:b/></w:rPr><w:t xml:space="preserve">序号</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="2353" w:type="dxa"/><w:shd w:val="clear" w:color="auto" w:fill="F2F2F4"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:rPr><w:b/></w:rPr><w:t xml:space="preserve">列名</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="2353" w:type="dxa"/><w:shd w:val="clear" w:color="auto" w:fill="F2F2F4"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:rPr><w:b/></w:rPr><w:t xml:space="preserve">类型</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="3764" w:type="dxa"/><w:shd w:val="clear" w:color="auto" w:fill="F2F2F4"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:rPr><w:b/></w:rPr><w:t xml:space="preserve">注释</w:t></w:r></w:p></w:tc></w:tr>
<w:tr><w:trPr><w:cantSplit/></w:trPr><w:tc><w:tcPr><w:tcW w:w="941" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">1</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="2353" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">id</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="2353" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">int</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="3764" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve"></w:t></w:r></w:p></w:tc></w:tr>
</w:tbl>
<w:p/>
<w:p><w:pPr><w:pStyle w:val="Heading3"/></w:pPr><w:r><w:t xml:space="preserve">定义</w:t></w:r></w:p>
<w:p><w:pPr><w:pStyle w:val="DBLensCode"/></w:pPr><w:r><w:t xml:space="preserve">SELECT id FROM orders</w:t></w:r></w:p>
<w:p/>
<w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t xml:space="preserve">表明细</w:t></w:r></w:p>
<w:p><w:pPr><w:pStyle w:val="Heading2"/></w:pPr><w:bookmarkStart w:id="2" w:name="t1"/><w:r><w:t xml:space="preserve">shop.customers</w:t></w:r><w:bookmarkEnd w:id="2"/></w:p>
<w:p><w:r><w:t xml:space="preserve">客户</w:t></w:r></w:p>
<w:tbl><w:tblPr><w:tblStyle w:val="TableGrid"/><w:tblW w:w="9412" w:type="dxa"/><w:tblLayout w:type="fixed"/></w:tblPr><w:tblGrid><w:gridCol w:w="672"/><w:gridCol w:w="1680"/><w:gridCol w:w="1680"/><w:gridCol w:w="672"/><w:gridCol w:w="1344"/><w:gridCol w:w="1008"/><w:gridCol w:w="2353"/></w:tblGrid>
<w:tr><w:trPr><w:tblHeader/></w:trPr><w:tc><w:tcPr><w:tcW w:w="672" w:type="dxa"/><w:shd w:val="clear" w:color="auto" w:fill="F2F2F4"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:rPr><w:b/></w:rPr><w:t xml:space="preserve">序号</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1680" w:type="dxa"/><w:shd w:val="clear" w:color="auto" w:fill="F2F2F4"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:rPr><w:b/></w:rPr><w:t xml:space="preserve">列名</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1680" w:type="dxa"/><w:shd w:val="clear" w:color="auto" w:fill="F2F2F4"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:rPr><w:b/></w:rPr><w:t xml:space="preserve">类型</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="672" w:type="dxa"/><w:shd w:val="clear" w:color="auto" w:fill="F2F2F4"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:rPr><w:b/></w:rPr><w:t xml:space="preserve">可空</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1344" w:type="dxa"/><w:shd w:val="clear" w:color="auto" w:fill="F2F2F4"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:rPr><w:b/></w:rPr><w:t xml:space="preserve">默认值</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1008" w:type="dxa"/><w:shd w:val="clear" w:color="auto" w:fill="F2F2F4"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:rPr><w:b/></w:rPr><w:t xml:space="preserve">标记</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="2353" w:type="dxa"/><w:shd w:val="clear" w:color="auto" w:fill="F2F2F4"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:rPr><w:b/></w:rPr><w:t xml:space="preserve">注释</w:t></w:r></w:p></w:tc></w:tr>
<w:tr><w:trPr><w:cantSplit/></w:trPr><w:tc><w:tcPr><w:tcW w:w="672" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">1</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1680" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">id</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1680" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">int</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="672" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">否</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1344" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve"></w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1008" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">主键, 自增</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="2353" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve"></w:t></w:r></w:p></w:tc></w:tr>
<w:tr><w:trPr><w:cantSplit/></w:trPr><w:tc><w:tcPr><w:tcW w:w="672" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">2</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1680" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">code</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1680" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">varchar(20)</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="672" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">否</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1344" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve"></w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1008" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve"></w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="2353" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">编号</w:t></w:r></w:p></w:tc></w:tr>
<w:tr><w:trPr><w:cantSplit/></w:trPr><w:tc><w:tcPr><w:tcW w:w="672" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">3</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1680" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">region_id</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1680" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">int</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="672" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">是</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1344" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve"></w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1008" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve"></w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="2353" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve"></w:t></w:r></w:p></w:tc></w:tr>
</w:tbl>
<w:p/>
<w:p><w:pPr><w:pStyle w:val="Heading3"/></w:pPr><w:r><w:t xml:space="preserve">索引</w:t></w:r></w:p>
<w:tbl><w:tblPr><w:tblStyle w:val="TableGrid"/><w:tblW w:w="9412" w:type="dxa"/><w:tblLayout w:type="fixed"/></w:tblPr><w:tblGrid><w:gridCol w:w="2896"/><w:gridCol w:w="3620"/><w:gridCol w:w="1448"/><w:gridCol w:w="1448"/></w:tblGrid>
<w:tr><w:trPr><w:tblHeader/></w:trPr><w:tc><w:tcPr><w:tcW w:w="2896" w:type="dxa"/><w:shd w:val="clear" w:color="auto" w:fill="F2F2F4"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:rPr><w:b/></w:rPr><w:t xml:space="preserve">名称</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="3620" w:type="dxa"/><w:shd w:val="clear" w:color="auto" w:fill="F2F2F4"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:rPr><w:b/></w:rPr><w:t xml:space="preserve">列</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1448" w:type="dxa"/><w:shd w:val="clear" w:color="auto" w:fill="F2F2F4"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:rPr><w:b/></w:rPr><w:t xml:space="preserve">唯一</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1448" w:type="dxa"/><w:shd w:val="clear" w:color="auto" w:fill="F2F2F4"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:rPr><w:b/></w:rPr><w:t xml:space="preserve">方法</w:t></w:r></w:p></w:tc></w:tr>
<w:tr><w:trPr><w:cantSplit/></w:trPr><w:tc><w:tcPr><w:tcW w:w="2896" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">ix_code</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="3620" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">code DESC</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1448" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">否</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1448" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">BTREE</w:t></w:r></w:p></w:tc></w:tr>
</w:tbl>
<w:p/>
<w:p><w:pPr><w:pStyle w:val="Heading3"/></w:pPr><w:r><w:t xml:space="preserve">约束</w:t></w:r></w:p>
<w:tbl><w:tblPr><w:tblStyle w:val="TableGrid"/><w:tblW w:w="9412" w:type="dxa"/><w:tblLayout w:type="fixed"/></w:tblPr><w:tblGrid><w:gridCol w:w="1448"/><w:gridCol w:w="2896"/><w:gridCol w:w="5068"/></w:tblGrid>
<w:tr><w:trPr><w:tblHeader/></w:trPr><w:tc><w:tcPr><w:tcW w:w="1448" w:type="dxa"/><w:shd w:val="clear" w:color="auto" w:fill="F2F2F4"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:rPr><w:b/></w:rPr><w:t xml:space="preserve">类别</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="2896" w:type="dxa"/><w:shd w:val="clear" w:color="auto" w:fill="F2F2F4"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:rPr><w:b/></w:rPr><w:t xml:space="preserve">名称</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="5068" w:type="dxa"/><w:shd w:val="clear" w:color="auto" w:fill="F2F2F4"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:rPr><w:b/></w:rPr><w:t xml:space="preserve">内容</w:t></w:r></w:p></w:tc></w:tr>
<w:tr><w:trPr><w:cantSplit/></w:trPr><w:tc><w:tcPr><w:tcW w:w="1448" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">主键</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="2896" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">pk_customers</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="5068" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">id</w:t></w:r></w:p></w:tc></w:tr>
<w:tr><w:trPr><w:cantSplit/></w:trPr><w:tc><w:tcPr><w:tcW w:w="1448" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">唯一</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="2896" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">uq_code</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="5068" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">code</w:t></w:r></w:p></w:tc></w:tr>
</w:tbl>
<w:p/>
<w:p><w:pPr><w:pStyle w:val="Heading3"/></w:pPr><w:r><w:t xml:space="preserve">外键</w:t></w:r></w:p>
<w:tbl><w:tblPr><w:tblStyle w:val="TableGrid"/><w:tblW w:w="9412" w:type="dxa"/><w:tblLayout w:type="fixed"/></w:tblPr><w:tblGrid><w:gridCol w:w="1660"/><w:gridCol w:w="1660"/><w:gridCol w:w="2214"/><w:gridCol w:w="1107"/><w:gridCol w:w="1107"/><w:gridCol w:w="1660"/></w:tblGrid>
<w:tr><w:trPr><w:tblHeader/></w:trPr><w:tc><w:tcPr><w:tcW w:w="1660" w:type="dxa"/><w:shd w:val="clear" w:color="auto" w:fill="F2F2F4"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:rPr><w:b/></w:rPr><w:t xml:space="preserve">名称</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1660" w:type="dxa"/><w:shd w:val="clear" w:color="auto" w:fill="F2F2F4"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:rPr><w:b/></w:rPr><w:t xml:space="preserve">列</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="2214" w:type="dxa"/><w:shd w:val="clear" w:color="auto" w:fill="F2F2F4"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:rPr><w:b/></w:rPr><w:t xml:space="preserve">引用</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1107" w:type="dxa"/><w:shd w:val="clear" w:color="auto" w:fill="F2F2F4"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:rPr><w:b/></w:rPr><w:t xml:space="preserve">删除时</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1107" w:type="dxa"/><w:shd w:val="clear" w:color="auto" w:fill="F2F2F4"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:rPr><w:b/></w:rPr><w:t xml:space="preserve">更新时</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1660" w:type="dxa"/><w:shd w:val="clear" w:color="auto" w:fill="F2F2F4"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:rPr><w:b/></w:rPr><w:t xml:space="preserve">注释</w:t></w:r></w:p></w:tc></w:tr>
<w:tr><w:trPr><w:cantSplit/></w:trPr><w:tc><w:tcPr><w:tcW w:w="1660" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">fk_region</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1660" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">region_id</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="2214" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:hyperlink w:anchor="t4" w:history="1"><w:r><w:rPr><w:rStyle w:val="Hyperlink"/></w:rPr><w:t xml:space="preserve">crm.regions</w:t></w:r></w:hyperlink><w:r><w:t xml:space="preserve"> (id)</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1107" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">CASCADE</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1107" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve"></w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1660" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve"></w:t></w:r></w:p></w:tc></w:tr>
</w:tbl>
<w:p/>
<w:p><w:pPr><w:pStyle w:val="Heading3"/></w:pPr><w:r><w:t xml:space="preserve">被引用</w:t></w:r></w:p>
<w:tbl><w:tblPr><w:tblStyle w:val="TableGrid"/><w:tblW w:w="9412" w:type="dxa"/><w:tblLayout w:type="fixed"/></w:tblPr><w:tblGrid><w:gridCol w:w="2509"/><w:gridCol w:w="1882"/><w:gridCol w:w="1882"/><w:gridCol w:w="1254"/><w:gridCol w:w="1882"/></w:tblGrid>
<w:tr><w:trPr><w:tblHeader/></w:trPr><w:tc><w:tcPr><w:tcW w:w="2509" w:type="dxa"/><w:shd w:val="clear" w:color="auto" w:fill="F2F2F4"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:rPr><w:b/></w:rPr><w:t xml:space="preserve">表名</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1882" w:type="dxa"/><w:shd w:val="clear" w:color="auto" w:fill="F2F2F4"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:rPr><w:b/></w:rPr><w:t xml:space="preserve">列</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1882" w:type="dxa"/><w:shd w:val="clear" w:color="auto" w:fill="F2F2F4"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:rPr><w:b/></w:rPr><w:t xml:space="preserve">引用</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1254" w:type="dxa"/><w:shd w:val="clear" w:color="auto" w:fill="F2F2F4"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:rPr><w:b/></w:rPr><w:t xml:space="preserve">类别</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1882" w:type="dxa"/><w:shd w:val="clear" w:color="auto" w:fill="F2F2F4"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:rPr><w:b/></w:rPr><w:t xml:space="preserve">名称</w:t></w:r></w:p></w:tc></w:tr>
<w:tr><w:trPr><w:cantSplit/></w:trPr><w:tc><w:tcPr><w:tcW w:w="2509" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:hyperlink w:anchor="t3" w:history="1"><w:r><w:rPr><w:rStyle w:val="Hyperlink"/></w:rPr><w:t xml:space="preserve">shop.orders</w:t></w:r></w:hyperlink></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1882" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">cust_id, cust_code</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1882" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">id, code</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1254" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">外键</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1882" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">fk_cust</w:t></w:r></w:p></w:tc></w:tr>
</w:tbl>
<w:p/>
<w:p><w:pPr><w:pStyle w:val="Heading2"/></w:pPr><w:bookmarkStart w:id="3" w:name="t2"/><w:r><w:t xml:space="preserve">shop.items</w:t></w:r><w:bookmarkEnd w:id="3"/></w:p>
<w:tbl><w:tblPr><w:tblStyle w:val="TableGrid"/><w:tblW w:w="9412" w:type="dxa"/><w:tblLayout w:type="fixed"/></w:tblPr><w:tblGrid><w:gridCol w:w="672"/><w:gridCol w:w="1680"/><w:gridCol w:w="1680"/><w:gridCol w:w="672"/><w:gridCol w:w="1344"/><w:gridCol w:w="1008"/><w:gridCol w:w="2353"/></w:tblGrid>
<w:tr><w:trPr><w:tblHeader/></w:trPr><w:tc><w:tcPr><w:tcW w:w="672" w:type="dxa"/><w:shd w:val="clear" w:color="auto" w:fill="F2F2F4"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:rPr><w:b/></w:rPr><w:t xml:space="preserve">序号</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1680" w:type="dxa"/><w:shd w:val="clear" w:color="auto" w:fill="F2F2F4"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:rPr><w:b/></w:rPr><w:t xml:space="preserve">列名</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1680" w:type="dxa"/><w:shd w:val="clear" w:color="auto" w:fill="F2F2F4"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:rPr><w:b/></w:rPr><w:t xml:space="preserve">类型</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="672" w:type="dxa"/><w:shd w:val="clear" w:color="auto" w:fill="F2F2F4"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:rPr><w:b/></w:rPr><w:t xml:space="preserve">可空</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1344" w:type="dxa"/><w:shd w:val="clear" w:color="auto" w:fill="F2F2F4"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:rPr><w:b/></w:rPr><w:t xml:space="preserve">默认值</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1008" w:type="dxa"/><w:shd w:val="clear" w:color="auto" w:fill="F2F2F4"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:rPr><w:b/></w:rPr><w:t xml:space="preserve">标记</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="2353" w:type="dxa"/><w:shd w:val="clear" w:color="auto" w:fill="F2F2F4"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:rPr><w:b/></w:rPr><w:t xml:space="preserve">注释</w:t></w:r></w:p></w:tc></w:tr>
<w:tr><w:trPr><w:cantSplit/></w:trPr><w:tc><w:tcPr><w:tcW w:w="672" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">1</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1680" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">id</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1680" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">int</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="672" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">是</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1344" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve"></w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1008" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve"></w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="2353" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve"></w:t></w:r></w:p></w:tc></w:tr>
<w:tr><w:trPr><w:cantSplit/></w:trPr><w:tc><w:tcPr><w:tcW w:w="672" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">2</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1680" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">order_id</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1680" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">int</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="672" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">是</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1344" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve"></w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1008" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve"></w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="2353" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve"></w:t></w:r></w:p></w:tc></w:tr>
<w:tr><w:trPr><w:cantSplit/></w:trPr><w:tc><w:tcPr><w:tcW w:w="672" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">3</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1680" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">sku</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1680" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">varchar(30)</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="672" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">是</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1344" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve"></w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1008" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve"></w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="2353" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve"></w:t></w:r></w:p></w:tc></w:tr>
</w:tbl>
<w:p/>
<w:p><w:pPr><w:pStyle w:val="Heading3"/></w:pPr><w:r><w:t xml:space="preserve">逻辑外键</w:t></w:r></w:p>
<w:tbl><w:tblPr><w:tblStyle w:val="TableGrid"/><w:tblW w:w="9412" w:type="dxa"/><w:tblLayout w:type="fixed"/></w:tblPr><w:tblGrid><w:gridCol w:w="1660"/><w:gridCol w:w="1660"/><w:gridCol w:w="2214"/><w:gridCol w:w="1107"/><w:gridCol w:w="1107"/><w:gridCol w:w="1660"/></w:tblGrid>
<w:tr><w:trPr><w:tblHeader/></w:trPr><w:tc><w:tcPr><w:tcW w:w="1660" w:type="dxa"/><w:shd w:val="clear" w:color="auto" w:fill="F2F2F4"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:rPr><w:b/></w:rPr><w:t xml:space="preserve">名称</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1660" w:type="dxa"/><w:shd w:val="clear" w:color="auto" w:fill="F2F2F4"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:rPr><w:b/></w:rPr><w:t xml:space="preserve">列</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="2214" w:type="dxa"/><w:shd w:val="clear" w:color="auto" w:fill="F2F2F4"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:rPr><w:b/></w:rPr><w:t xml:space="preserve">引用</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1107" w:type="dxa"/><w:shd w:val="clear" w:color="auto" w:fill="F2F2F4"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:rPr><w:b/></w:rPr><w:t xml:space="preserve">删除时</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1107" w:type="dxa"/><w:shd w:val="clear" w:color="auto" w:fill="F2F2F4"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:rPr><w:b/></w:rPr><w:t xml:space="preserve">更新时</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1660" w:type="dxa"/><w:shd w:val="clear" w:color="auto" w:fill="F2F2F4"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:rPr><w:b/></w:rPr><w:t xml:space="preserve">注释</w:t></w:r></w:p></w:tc></w:tr>
<w:tr><w:trPr><w:cantSplit/></w:trPr><w:tc><w:tcPr><w:tcW w:w="1660" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">lk_order</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1660" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">order_id</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="2214" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:hyperlink w:anchor="t3" w:history="1"><w:r><w:rPr><w:rStyle w:val="Hyperlink"/></w:rPr><w:t xml:space="preserve">shop.orders</w:t></w:r></w:hyperlink><w:r><w:t xml:space="preserve"> (id)</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1107" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve"></w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1107" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve"></w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1660" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">没有建外键</w:t></w:r></w:p></w:tc></w:tr>
</w:tbl>
<w:p/>
<w:p><w:pPr><w:pStyle w:val="Heading2"/></w:pPr><w:bookmarkStart w:id="4" w:name="t3"/><w:r><w:t xml:space="preserve">shop.orders</w:t></w:r><w:bookmarkEnd w:id="4"/></w:p>
<w:tbl><w:tblPr><w:tblStyle w:val="TableGrid"/><w:tblW w:w="9412" w:type="dxa"/><w:tblLayout w:type="fixed"/></w:tblPr><w:tblGrid><w:gridCol w:w="672"/><w:gridCol w:w="1680"/><w:gridCol w:w="1680"/><w:gridCol w:w="672"/><w:gridCol w:w="1344"/><w:gridCol w:w="1008"/><w:gridCol w:w="2353"/></w:tblGrid>
<w:tr><w:trPr><w:tblHeader/></w:trPr><w:tc><w:tcPr><w:tcW w:w="672" w:type="dxa"/><w:shd w:val="clear" w:color="auto" w:fill="F2F2F4"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:rPr><w:b/></w:rPr><w:t xml:space="preserve">序号</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1680" w:type="dxa"/><w:shd w:val="clear" w:color="auto" w:fill="F2F2F4"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:rPr><w:b/></w:rPr><w:t xml:space="preserve">列名</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1680" w:type="dxa"/><w:shd w:val="clear" w:color="auto" w:fill="F2F2F4"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:rPr><w:b/></w:rPr><w:t xml:space="preserve">类型</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="672" w:type="dxa"/><w:shd w:val="clear" w:color="auto" w:fill="F2F2F4"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:rPr><w:b/></w:rPr><w:t xml:space="preserve">可空</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1344" w:type="dxa"/><w:shd w:val="clear" w:color="auto" w:fill="F2F2F4"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:rPr><w:b/></w:rPr><w:t xml:space="preserve">默认值</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1008" w:type="dxa"/><w:shd w:val="clear" w:color="auto" w:fill="F2F2F4"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:rPr><w:b/></w:rPr><w:t xml:space="preserve">标记</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="2353" w:type="dxa"/><w:shd w:val="clear" w:color="auto" w:fill="F2F2F4"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:rPr><w:b/></w:rPr><w:t xml:space="preserve">注释</w:t></w:r></w:p></w:tc></w:tr>
<w:tr><w:trPr><w:cantSplit/></w:trPr><w:tc><w:tcPr><w:tcW w:w="672" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">1</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1680" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">id</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1680" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">int</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="672" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">否</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1344" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve"></w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1008" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">主键</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="2353" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve"></w:t></w:r></w:p></w:tc></w:tr>
<w:tr><w:trPr><w:cantSplit/></w:trPr><w:tc><w:tcPr><w:tcW w:w="672" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">2</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1680" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">cust_id</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1680" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">int</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="672" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">是</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1344" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve"></w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1008" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve"></w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="2353" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve"></w:t></w:r></w:p></w:tc></w:tr>
<w:tr><w:trPr><w:cantSplit/></w:trPr><w:tc><w:tcPr><w:tcW w:w="672" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">3</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1680" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">cust_code</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1680" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">varchar(20)</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="672" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">是</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1344" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve"></w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1008" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve"></w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="2353" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve"></w:t></w:r></w:p></w:tc></w:tr>
<w:tr><w:trPr><w:cantSplit/></w:trPr><w:tc><w:tcPr><w:tcW w:w="672" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">4</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1680" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">amount</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1680" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">decimal(10,2)</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="672" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">是</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1344" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">0</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1008" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve"></w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="2353" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve"></w:t></w:r></w:p></w:tc></w:tr>
<w:tr><w:trPr><w:cantSplit/></w:trPr><w:tc><w:tcPr><w:tcW w:w="672" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">5</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1680" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">double_amount</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1680" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">decimal(10,2)</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="672" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">是</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1344" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">amount * 2</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1008" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">生成列</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="2353" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve"></w:t></w:r></w:p></w:tc></w:tr>
</w:tbl>
<w:p/>
<w:p><w:pPr><w:pStyle w:val="Heading3"/></w:pPr><w:r><w:t xml:space="preserve">约束</w:t></w:r></w:p>
<w:tbl><w:tblPr><w:tblStyle w:val="TableGrid"/><w:tblW w:w="9412" w:type="dxa"/><w:tblLayout w:type="fixed"/></w:tblPr><w:tblGrid><w:gridCol w:w="1448"/><w:gridCol w:w="2896"/><w:gridCol w:w="5068"/></w:tblGrid>
<w:tr><w:trPr><w:tblHeader/></w:trPr><w:tc><w:tcPr><w:tcW w:w="1448" w:type="dxa"/><w:shd w:val="clear" w:color="auto" w:fill="F2F2F4"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:rPr><w:b/></w:rPr><w:t xml:space="preserve">类别</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="2896" w:type="dxa"/><w:shd w:val="clear" w:color="auto" w:fill="F2F2F4"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:rPr><w:b/></w:rPr><w:t xml:space="preserve">名称</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="5068" w:type="dxa"/><w:shd w:val="clear" w:color="auto" w:fill="F2F2F4"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:rPr><w:b/></w:rPr><w:t xml:space="preserve">内容</w:t></w:r></w:p></w:tc></w:tr>
<w:tr><w:trPr><w:cantSplit/></w:trPr><w:tc><w:tcPr><w:tcW w:w="1448" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">主键</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="2896" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve"></w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="5068" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">id</w:t></w:r></w:p></w:tc></w:tr>
<w:tr><w:trPr><w:cantSplit/></w:trPr><w:tc><w:tcPr><w:tcW w:w="1448" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">检查</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="2896" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">ck_amount</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="5068" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">amount &gt;= 0</w:t></w:r></w:p></w:tc></w:tr>
</w:tbl>
<w:p/>
<w:p><w:pPr><w:pStyle w:val="Heading3"/></w:pPr><w:r><w:t xml:space="preserve">外键</w:t></w:r></w:p>
<w:tbl><w:tblPr><w:tblStyle w:val="TableGrid"/><w:tblW w:w="9412" w:type="dxa"/><w:tblLayout w:type="fixed"/></w:tblPr><w:tblGrid><w:gridCol w:w="1660"/><w:gridCol w:w="1660"/><w:gridCol w:w="2214"/><w:gridCol w:w="1107"/><w:gridCol w:w="1107"/><w:gridCol w:w="1660"/></w:tblGrid>
<w:tr><w:trPr><w:tblHeader/></w:trPr><w:tc><w:tcPr><w:tcW w:w="1660" w:type="dxa"/><w:shd w:val="clear" w:color="auto" w:fill="F2F2F4"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:rPr><w:b/></w:rPr><w:t xml:space="preserve">名称</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1660" w:type="dxa"/><w:shd w:val="clear" w:color="auto" w:fill="F2F2F4"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:rPr><w:b/></w:rPr><w:t xml:space="preserve">列</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="2214" w:type="dxa"/><w:shd w:val="clear" w:color="auto" w:fill="F2F2F4"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:rPr><w:b/></w:rPr><w:t xml:space="preserve">引用</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1107" w:type="dxa"/><w:shd w:val="clear" w:color="auto" w:fill="F2F2F4"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:rPr><w:b/></w:rPr><w:t xml:space="preserve">删除时</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1107" w:type="dxa"/><w:shd w:val="clear" w:color="auto" w:fill="F2F2F4"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:rPr><w:b/></w:rPr><w:t xml:space="preserve">更新时</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1660" w:type="dxa"/><w:shd w:val="clear" w:color="auto" w:fill="F2F2F4"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:rPr><w:b/></w:rPr><w:t xml:space="preserve">注释</w:t></w:r></w:p></w:tc></w:tr>
<w:tr><w:trPr><w:cantSplit/></w:trPr><w:tc><w:tcPr><w:tcW w:w="1660" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">fk_cust</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1660" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">cust_id, cust_code</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="2214" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:hyperlink w:anchor="t1" w:history="1"><w:r><w:rPr><w:rStyle w:val="Hyperlink"/></w:rPr><w:t xml:space="preserve">shop.customers</w:t></w:r></w:hyperlink><w:r><w:t xml:space="preserve"> (id, code)</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1107" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">CASCADE</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1107" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve"></w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1660" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve"></w:t></w:r></w:p></w:tc></w:tr>
</w:tbl>
<w:p/>
<w:p><w:pPr><w:pStyle w:val="Heading3"/></w:pPr><w:r><w:t xml:space="preserve">触发器</w:t></w:r></w:p>
<w:tbl><w:tblPr><w:tblStyle w:val="TableGrid"/><w:tblW w:w="9412" w:type="dxa"/><w:tblLayout w:type="fixed"/></w:tblPr><w:tblGrid><w:gridCol w:w="3764"/><w:gridCol w:w="1882"/><w:gridCol w:w="3764"/></w:tblGrid>
<w:tr><w:trPr><w:tblHeader/></w:trPr><w:tc><w:tcPr><w:tcW w:w="3764" w:type="dxa"/><w:shd w:val="clear" w:color="auto" w:fill="F2F2F4"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:rPr><w:b/></w:rPr><w:t xml:space="preserve">名称</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1882" w:type="dxa"/><w:shd w:val="clear" w:color="auto" w:fill="F2F2F4"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:rPr><w:b/></w:rPr><w:t xml:space="preserve">时机</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="3764" w:type="dxa"/><w:shd w:val="clear" w:color="auto" w:fill="F2F2F4"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:rPr><w:b/></w:rPr><w:t xml:space="preserve">事件</w:t></w:r></w:p></w:tc></w:tr>
<w:tr><w:trPr><w:cantSplit/></w:trPr><w:tc><w:tcPr><w:tcW w:w="3764" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">trg_orders</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1882" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">BEFORE</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="3764" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">INSERT, UPDATE</w:t></w:r></w:p></w:tc></w:tr>
</w:tbl>
<w:p/>
<w:p><w:pPr><w:pStyle w:val="Heading3"/></w:pPr><w:r><w:t xml:space="preserve">被引用</w:t></w:r></w:p>
<w:tbl><w:tblPr><w:tblStyle w:val="TableGrid"/><w:tblW w:w="9412" w:type="dxa"/><w:tblLayout w:type="fixed"/></w:tblPr><w:tblGrid><w:gridCol w:w="2509"/><w:gridCol w:w="1882"/><w:gridCol w:w="1882"/><w:gridCol w:w="1254"/><w:gridCol w:w="1882"/></w:tblGrid>
<w:tr><w:trPr><w:tblHeader/></w:trPr><w:tc><w:tcPr><w:tcW w:w="2509" w:type="dxa"/><w:shd w:val="clear" w:color="auto" w:fill="F2F2F4"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:rPr><w:b/></w:rPr><w:t xml:space="preserve">表名</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1882" w:type="dxa"/><w:shd w:val="clear" w:color="auto" w:fill="F2F2F4"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:rPr><w:b/></w:rPr><w:t xml:space="preserve">列</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1882" w:type="dxa"/><w:shd w:val="clear" w:color="auto" w:fill="F2F2F4"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:rPr><w:b/></w:rPr><w:t xml:space="preserve">引用</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1254" w:type="dxa"/><w:shd w:val="clear" w:color="auto" w:fill="F2F2F4"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:rPr><w:b/></w:rPr><w:t xml:space="preserve">类别</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1882" w:type="dxa"/><w:shd w:val="clear" w:color="auto" w:fill="F2F2F4"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:rPr><w:b/></w:rPr><w:t xml:space="preserve">名称</w:t></w:r></w:p></w:tc></w:tr>
<w:tr><w:trPr><w:cantSplit/></w:trPr><w:tc><w:tcPr><w:tcW w:w="2509" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:hyperlink w:anchor="t2" w:history="1"><w:r><w:rPr><w:rStyle w:val="Hyperlink"/></w:rPr><w:t xml:space="preserve">shop.items</w:t></w:r></w:hyperlink></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1882" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">order_id</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1882" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">id</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1254" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">逻辑外键</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1882" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">lk_order</w:t></w:r></w:p></w:tc></w:tr>
</w:tbl>
<w:p/>
<w:p><w:pPr><w:pStyle w:val="Heading2"/></w:pPr><w:bookmarkStart w:id="5" w:name="t4"/><w:r><w:t xml:space="preserve">crm.regions</w:t></w:r><w:bookmarkEnd w:id="5"/></w:p>
<w:tbl><w:tblPr><w:tblStyle w:val="TableGrid"/><w:tblW w:w="9412" w:type="dxa"/><w:tblLayout w:type="fixed"/></w:tblPr><w:tblGrid><w:gridCol w:w="672"/><w:gridCol w:w="1680"/><w:gridCol w:w="1680"/><w:gridCol w:w="672"/><w:gridCol w:w="1344"/><w:gridCol w:w="1008"/><w:gridCol w:w="2353"/></w:tblGrid>
<w:tr><w:trPr><w:tblHeader/></w:trPr><w:tc><w:tcPr><w:tcW w:w="672" w:type="dxa"/><w:shd w:val="clear" w:color="auto" w:fill="F2F2F4"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:rPr><w:b/></w:rPr><w:t xml:space="preserve">序号</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1680" w:type="dxa"/><w:shd w:val="clear" w:color="auto" w:fill="F2F2F4"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:rPr><w:b/></w:rPr><w:t xml:space="preserve">列名</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1680" w:type="dxa"/><w:shd w:val="clear" w:color="auto" w:fill="F2F2F4"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:rPr><w:b/></w:rPr><w:t xml:space="preserve">类型</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="672" w:type="dxa"/><w:shd w:val="clear" w:color="auto" w:fill="F2F2F4"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:rPr><w:b/></w:rPr><w:t xml:space="preserve">可空</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1344" w:type="dxa"/><w:shd w:val="clear" w:color="auto" w:fill="F2F2F4"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:rPr><w:b/></w:rPr><w:t xml:space="preserve">默认值</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1008" w:type="dxa"/><w:shd w:val="clear" w:color="auto" w:fill="F2F2F4"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:rPr><w:b/></w:rPr><w:t xml:space="preserve">标记</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="2353" w:type="dxa"/><w:shd w:val="clear" w:color="auto" w:fill="F2F2F4"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:rPr><w:b/></w:rPr><w:t xml:space="preserve">注释</w:t></w:r></w:p></w:tc></w:tr>
<w:tr><w:trPr><w:cantSplit/></w:trPr><w:tc><w:tcPr><w:tcW w:w="672" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">1</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1680" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">id</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1680" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">int</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="672" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">是</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1344" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve"></w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1008" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">主键</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="2353" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve"></w:t></w:r></w:p></w:tc></w:tr>
<w:tr><w:trPr><w:cantSplit/></w:trPr><w:tc><w:tcPr><w:tcW w:w="672" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">2</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1680" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">name</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1680" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">varchar(50)</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="672" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">是</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1344" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve"></w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1008" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve"></w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="2353" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve"></w:t></w:r></w:p></w:tc></w:tr>
</w:tbl>
<w:p/>
<w:p><w:pPr><w:pStyle w:val="Heading3"/></w:pPr><w:r><w:t xml:space="preserve">约束</w:t></w:r></w:p>
<w:tbl><w:tblPr><w:tblStyle w:val="TableGrid"/><w:tblW w:w="9412" w:type="dxa"/><w:tblLayout w:type="fixed"/></w:tblPr><w:tblGrid><w:gridCol w:w="1448"/><w:gridCol w:w="2896"/><w:gridCol w:w="5068"/></w:tblGrid>
<w:tr><w:trPr><w:tblHeader/></w:trPr><w:tc><w:tcPr><w:tcW w:w="1448" w:type="dxa"/><w:shd w:val="clear" w:color="auto" w:fill="F2F2F4"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:rPr><w:b/></w:rPr><w:t xml:space="preserve">类别</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="2896" w:type="dxa"/><w:shd w:val="clear" w:color="auto" w:fill="F2F2F4"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:rPr><w:b/></w:rPr><w:t xml:space="preserve">名称</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="5068" w:type="dxa"/><w:shd w:val="clear" w:color="auto" w:fill="F2F2F4"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:rPr><w:b/></w:rPr><w:t xml:space="preserve">内容</w:t></w:r></w:p></w:tc></w:tr>
<w:tr><w:trPr><w:cantSplit/></w:trPr><w:tc><w:tcPr><w:tcW w:w="1448" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">主键</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="2896" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve"></w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="5068" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">id</w:t></w:r></w:p></w:tc></w:tr>
</w:tbl>
<w:p/>
<w:p><w:pPr><w:pStyle w:val="Heading3"/></w:pPr><w:r><w:t xml:space="preserve">被引用</w:t></w:r></w:p>
<w:tbl><w:tblPr><w:tblStyle w:val="TableGrid"/><w:tblW w:w="9412" w:type="dxa"/><w:tblLayout w:type="fixed"/></w:tblPr><w:tblGrid><w:gridCol w:w="2509"/><w:gridCol w:w="1882"/><w:gridCol w:w="1882"/><w:gridCol w:w="1254"/><w:gridCol w:w="1882"/></w:tblGrid>
<w:tr><w:trPr><w:tblHeader/></w:trPr><w:tc><w:tcPr><w:tcW w:w="2509" w:type="dxa"/><w:shd w:val="clear" w:color="auto" w:fill="F2F2F4"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:rPr><w:b/></w:rPr><w:t xml:space="preserve">表名</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1882" w:type="dxa"/><w:shd w:val="clear" w:color="auto" w:fill="F2F2F4"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:rPr><w:b/></w:rPr><w:t xml:space="preserve">列</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1882" w:type="dxa"/><w:shd w:val="clear" w:color="auto" w:fill="F2F2F4"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:rPr><w:b/></w:rPr><w:t xml:space="preserve">引用</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1254" w:type="dxa"/><w:shd w:val="clear" w:color="auto" w:fill="F2F2F4"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:rPr><w:b/></w:rPr><w:t xml:space="preserve">类别</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1882" w:type="dxa"/><w:shd w:val="clear" w:color="auto" w:fill="F2F2F4"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:rPr><w:b/></w:rPr><w:t xml:space="preserve">名称</w:t></w:r></w:p></w:tc></w:tr>
<w:tr><w:trPr><w:cantSplit/></w:trPr><w:tc><w:tcPr><w:tcW w:w="2509" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:hyperlink w:anchor="t1" w:history="1"><w:r><w:rPr><w:rStyle w:val="Hyperlink"/></w:rPr><w:t xml:space="preserve">shop.customers</w:t></w:r></w:hyperlink></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1882" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">region_id</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1882" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">id</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1254" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">外键</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1882" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">fk_region</w:t></w:r></w:p></w:tc></w:tr>
</w:tbl>
<w:p/>
<w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t xml:space="preserve">无法读取的表</w:t></w:r></w:p>
<w:tbl><w:tblPr><w:tblStyle w:val="TableGrid"/><w:tblW w:w="9412" w:type="dxa"/><w:tblLayout w:type="fixed"/></w:tblPr><w:tblGrid><w:gridCol w:w="1045"/><w:gridCol w:w="1568"/><w:gridCol w:w="2614"/><w:gridCol w:w="4183"/></w:tblGrid>
<w:tr><w:trPr><w:tblHeader/></w:trPr><w:tc><w:tcPr><w:tcW w:w="1045" w:type="dxa"/><w:shd w:val="clear" w:color="auto" w:fill="F2F2F4"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:rPr><w:b/></w:rPr><w:t xml:space="preserve">Schema</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1568" w:type="dxa"/><w:shd w:val="clear" w:color="auto" w:fill="F2F2F4"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:rPr><w:b/></w:rPr><w:t xml:space="preserve">表名</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="2614" w:type="dxa"/><w:shd w:val="clear" w:color="auto" w:fill="F2F2F4"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:rPr><w:b/></w:rPr><w:t xml:space="preserve">文件</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="4183" w:type="dxa"/><w:shd w:val="clear" w:color="auto" w:fill="F2F2F4"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:rPr><w:b/></w:rPr><w:t xml:space="preserve">原因</w:t></w:r></w:p></w:tc></w:tr>
<w:tr><w:trPr><w:cantSplit/></w:trPr><w:tc><w:tcPr><w:tcW w:w="1045" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">shop</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="1568" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">broken</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="2614" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">tables/shop/broken.yaml</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="4183" w:type="dxa"/></w:tcPr><w:p><w:pPr><w:pStyle w:val="DBLensTableText"/></w:pPr><w:r><w:t xml:space="preserve">line 3: bad</w:t></w:r></w:p></w:tc></w:tr>
</w:tbl>
<w:p/>
<w:sectPr><w:pgSz w:w="11906" w:h="16838"/><w:pgMar w:top="1440" w:right="1247" w:bottom="1440" w:left="1247" w:header="851" w:footer="992" w:gutter="0"/><w:cols w:space="425"/></w:sectPr>
</w:body>
</w:document>
```

- [ ] **Step 15: 修改 `crates/dblens-service/src/document/template.rs`**

```diff
--- a/crates/dblens-service/src/document/template.rs
+++ b/crates/dblens-service/src/document/template.rs
@@ -179,10 +179,15 @@ mod tests {
 
     #[test]
     fn every_label_is_used_and_exists() {
-        let sources = [TextFormat::Html.builtin(), TextFormat::Markdown.builtin()];
+        // 模板中的 'doc.…'，Rust 代码（测试之前的部分）中的 "doc.…"
+        let code = |s: &'static str| s.split("#[cfg(test)]").next().unwrap_or_default();
+        let sources =
+            [TextFormat::Html.builtin(), TextFormat::Markdown.builtin(), code(include_str!("docx.rs"))];
         let used: std::collections::BTreeSet<String> = sources
             .iter()
-            .flat_map(|s| s.match_indices("doc.").map(move |(i, _)| &s[i..]))
+            .flat_map(|s| {
+                s.match_indices("'doc.").chain(s.match_indices("\"doc.")).map(move |(i, _)| &s[i + 1..])
+            })
             .map(|s| {
                 s.chars()
                     .take_while(|c| c.is_ascii_alphanumeric() || *c == '.' || *c == '_')
```

- [ ] **Step 16: 检查**

```bash
export CARGO_INCREMENTAL=0
cargo fmt --all
cargo clippy --config "source.mirror.registry='sparse+https://rsproxy.cn/index/'" --workspace --all-targets -- -D warnings
DBLENS_LANG=en cargo test --config "source.mirror.registry='sparse+https://rsproxy.cn/index/'" --workspace
```

写计划时另用 Word 打开生成的 docx（更新目录、导出 PDF、逐页转成图片核对标题、目录页码、图片、表格、链接）；执行时可选做同样的核对。

Expected: 全部测试通过（service 新增 3 个，快照 1 个）。

- [ ] **Step 17: 提交**

```bash
git add Cargo.lock Cargo.toml crates/dblens-core/locales/app.json crates/dblens-service/Cargo.toml crates/dblens-service/src/document/docx.rs crates/dblens-service/src/document/docx/content_types.xml crates/dblens-service/src/document/docx/core.xml crates/dblens-service/src/document/docx/document.xml crates/dblens-service/src/document/docx/document_rels.xml crates/dblens-service/src/document/docx/rels.xml crates/dblens-service/src/document/docx/settings.xml crates/dblens-service/src/document/docx/styles.xml crates/dblens-service/src/document/labels.json crates/dblens-service/src/document/mod.rs crates/dblens-service/src/document/snapshots/dblens_service__document__docx__tests__docx_design_zh.snap crates/dblens-service/src/document/template.rs
git commit -m "feat(document): docx（样式来源、目录、图片）" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 5: 服务层：数据字典的 xlsx

- `document/xlsx.rs`（新）：三张工作表"表清单"（表名链接到"列"工作表中这张表的第一行）、"列"、"索引和约束"；冻结表头、筛选、按内容估算的列宽（中日韩字符算 2，上限 60）；超过 32767 个字符的单元格截断并计数。
- `labels.json` 加 `doc.sheet.*`；标签测试也扫描 `xlsx.rs`。

**Files:**
- Modify: `crates/dblens-service/src/document/labels.json`
- Modify: `crates/dblens-service/src/document/mod.rs`
- Modify: `crates/dblens-service/src/document/template.rs`
- Create: `crates/dblens-service/src/document/xlsx.rs`

**Interfaces:**
- Consumes：任务 2、3。
- Produces：`write_xlsx(content, lang) -> Result<(Vec<u8>, u64)>`（文件内容、截断的单元格数）。

- [ ] **Step 1: 修改 `crates/dblens-service/src/document/labels.json`**

```diff
--- a/crates/dblens-service/src/document/labels.json
+++ b/crates/dblens-service/src/document/labels.json
@@ -266,5 +266,17 @@
   "doc.toc_hint": {
     "zh-CN": "打开文档时按提示更新域，或右键这里选“更新域”，即可生成目录。",
     "en": "Update fields when prompted on opening, or right-click here and choose Update Field, to build the table of contents."
+  },
+  "doc.sheet.tables": {
+    "zh-CN": "表清单",
+    "en": "Tables"
+  },
+  "doc.sheet.columns": {
+    "zh-CN": "列",
+    "en": "Columns"
+  },
+  "doc.sheet.keys": {
+    "zh-CN": "索引和约束",
+    "en": "Indexes and Constraints"
   }
 }
```

- [ ] **Step 2: 修改 `crates/dblens-service/src/document/mod.rs`**

```diff
--- a/crates/dblens-service/src/document/mod.rs
+++ b/crates/dblens-service/src/document/mod.rs
@@ -4,6 +4,7 @@ mod content;
 mod docx;
 mod labels;
 mod template;
+mod xlsx;
 
 pub use content::{
     BuildOptions, DocColumn, DocConstraint, DocContent, DocDiagram, DocIndex, DocInfo, DocKey, DocKind,
@@ -13,3 +14,4 @@ pub use content::{
 pub use docx::{builtin_reference, check_reference, write as write_docx};
 pub use labels::label;
 pub use template::{TextFormat, check, render};
+pub use xlsx::write as write_xlsx;
```

- [ ] **Step 3: 修改 `crates/dblens-service/src/document/template.rs`**

```diff
--- a/crates/dblens-service/src/document/template.rs
+++ b/crates/dblens-service/src/document/template.rs
@@ -181,8 +181,12 @@ mod tests {
     fn every_label_is_used_and_exists() {
         // 模板中的 'doc.…'，Rust 代码（测试之前的部分）中的 "doc.…"
         let code = |s: &'static str| s.split("#[cfg(test)]").next().unwrap_or_default();
-        let sources =
-            [TextFormat::Html.builtin(), TextFormat::Markdown.builtin(), code(include_str!("docx.rs"))];
+        let sources = [
+            TextFormat::Html.builtin(),
+            TextFormat::Markdown.builtin(),
+            code(include_str!("docx.rs")),
+            code(include_str!("xlsx.rs")),
+        ];
         let used: std::collections::BTreeSet<String> = sources
             .iter()
             .flat_map(|s| {
```

- [ ] **Step 4: 新建 `crates/dblens-service/src/document/xlsx.rs`**

```rust
//! 数据字典的 xlsx（M4c 设计文档 4.3 节）：表清单、列、索引和约束三张工作表。

use dblens_core::{Error, Lang, Result, msg};
use rust_xlsxwriter::{Format, Url, Workbook, Worksheet, XlsxError};

use super::content::DocContent;
use super::labels::label;

/// 单元格最多 32767 个字符。
const CELL_MAX: usize = 32767;
/// 自动列宽的上限（字符宽度）。
const WIDTH_MAX: usize = 60;

fn xlsx_error(e: XlsxError) -> Error {
    Error::internal(msg!("document.write_failed", error = e))
}

/// 一张工作表的写出：记下各列的宽度，统计截断的单元格。
struct Sheet<'a> {
    ws: &'a mut Worksheet,
    widths: Vec<usize>,
    row: u32,
    truncated: &'a mut u64,
}

/// 显示宽度：中日韩等宽字符算 2。
fn display_width(s: &str) -> usize {
    s.chars().map(|c| if (c as u32) >= 0x1100 { 2 } else { 1 }).sum()
}

impl Sheet<'_> {
    fn header(&mut self, headers: &[String], bold: &Format) -> Result<()> {
        for (c, h) in headers.iter().enumerate() {
            self.ws.write_string_with_format(0, c as u16, h, bold).map_err(xlsx_error)?;
            self.widths.push(display_width(h));
        }
        self.ws.set_freeze_panes(1, 0).map_err(xlsx_error)?;
        self.row = 1;
        Ok(())
    }

    fn cell(&mut self, col: usize, text: &str) -> Result<()> {
        let text = if text.chars().count() > CELL_MAX {
            *self.truncated += 1;
            text.chars().take(CELL_MAX).collect::<String>()
        } else {
            text.to_string()
        };
        if !text.is_empty() {
            self.ws.write_string(self.row, col as u16, &text).map_err(xlsx_error)?;
        }
        self.widen(col, &text);
        Ok(())
    }

    fn number(&mut self, col: usize, n: usize) -> Result<()> {
        self.ws.write_number(self.row, col as u16, n as f64).map_err(xlsx_error)?;
        self.widen(col, &n.to_string());
        Ok(())
    }

    fn link(&mut self, col: usize, text: &str, target: &str) -> Result<()> {
        self.ws.write_url(self.row, col as u16, Url::new(target).set_text(text)).map_err(xlsx_error)?;
        self.widen(col, text);
        Ok(())
    }

    fn widen(&mut self, col: usize, text: &str) {
        let w = text.lines().map(display_width).max().unwrap_or(0);
        if let Some(cur) = self.widths.get_mut(col) {
            *cur = (*cur).max(w);
        }
    }

    fn next(&mut self) {
        self.row += 1;
    }

    fn finish(self) -> Result<()> {
        if self.row > 1 {
            self.ws
                .autofilter(0, 0, self.row - 1, self.widths.len().saturating_sub(1) as u16)
                .map_err(xlsx_error)?;
        }
        for (c, w) in self.widths.iter().enumerate() {
            self.ws.set_column_width(c as u16, ((*w).clamp(6, WIDTH_MAX) + 2) as f64).map_err(xlsx_error)?;
        }
        Ok(())
    }
}

/// 写出数据字典的 xlsx，返回文件内容和截断的单元格个数。
pub fn write(content: &DocContent, lang: Lang) -> Result<(Vec<u8>, u64)> {
    let t = |k: &str| label(lang, k);
    let hs = |keys: &[&str]| keys.iter().map(|k| t(k)).collect::<Vec<_>>();
    let mut book = Workbook::new();
    let bold = Format::new().set_bold();
    let mut truncated = 0;
    let tables_name = t("doc.sheet.tables");
    let columns_name = t("doc.sheet.columns");

    // 每张表在"列"工作表中的第一行（从 1 起，第 0 行是表头）
    let mut first_rows = Vec::new();
    let mut row = 1u32;
    for s in &content.schemas {
        for tb in &s.tables {
            first_rows.push(row);
            row += tb.columns.len().max(1) as u32;
        }
    }

    {
        let ws = book.add_worksheet().set_name(&tables_name).map_err(xlsx_error)?;
        let mut sh = Sheet { ws, widths: vec![], row: 0, truncated: &mut truncated };
        sh.header(&hs(&["doc.col.schema", "doc.col.table", "doc.col.comment", "doc.col.columns"]), &bold)?;
        let mut i = 0;
        for s in &content.schemas {
            for tb in &s.tables {
                sh.cell(0, &s.name)?;
                let target =
                    format!("internal:'{}'!A{}", columns_name.replace('\'', "''"), first_rows[i] + 1);
                sh.link(1, &tb.name, &target)?;
                sh.cell(2, tb.comment.as_deref().unwrap_or_default())?;
                sh.number(3, tb.columns.len())?;
                sh.next();
                i += 1;
            }
        }
        sh.finish()?;
    }

    {
        let ws = book.add_worksheet().set_name(&columns_name).map_err(xlsx_error)?;
        let mut sh = Sheet { ws, widths: vec![], row: 0, truncated: &mut truncated };
        sh.header(
            &hs(&[
                "doc.col.schema",
                "doc.col.table",
                "doc.col.no",
                "doc.col.column",
                "doc.col.type",
                "doc.col.nullable",
                "doc.col.default",
                "doc.col.flags",
                "doc.col.comment",
            ]),
            &bold,
        )?;
        for s in &content.schemas {
            for tb in &s.tables {
                if tb.columns.is_empty() {
                    sh.cell(0, &s.name)?;
                    sh.cell(1, &tb.name)?;
                    sh.next();
                }
                for c in &tb.columns {
                    sh.cell(0, &s.name)?;
                    sh.cell(1, &tb.name)?;
                    sh.number(2, c.ordinal)?;
                    sh.cell(3, &c.name)?;
                    sh.cell(4, &c.data_type)?;
                    sh.cell(5, &t(if c.nullable { "doc.yes" } else { "doc.no" }))?;
                    sh.cell(6, c.default.as_deref().unwrap_or_default())?;
                    let flags: Vec<String> = c.flags.iter().map(|f| t(&format!("doc.flag.{f}"))).collect();
                    sh.cell(7, &flags.join(", "))?;
                    sh.cell(8, c.comment.as_deref().unwrap_or_default())?;
                    sh.next();
                }
            }
        }
        sh.finish()?;
    }

    {
        let ws = book.add_worksheet().set_name(t("doc.sheet.keys")).map_err(xlsx_error)?;
        let mut sh = Sheet { ws, widths: vec![], row: 0, truncated: &mut truncated };
        sh.header(
            &hs(&[
                "doc.col.schema",
                "doc.col.table",
                "doc.col.kind",
                "doc.col.name",
                "doc.col.index_columns",
                "doc.col.references",
                "doc.col.detail",
            ]),
            &bold,
        )?;
        for s in &content.schemas {
            for tb in &s.tables {
                let mut rows: Vec<[String; 5]> = Vec::new();
                for i in &tb.indexes {
                    let mut detail = Vec::new();
                    if i.unique {
                        detail.push(t("doc.kind.unique"));
                    }
                    detail.extend(i.method.clone());
                    rows.push([
                        t("doc.kind.index"),
                        i.name.clone(),
                        i.columns.clone(),
                        String::new(),
                        detail.join(", "),
                    ]);
                }
                for k in &tb.constraints {
                    let (cols, detail) = if k.kind == "check" {
                        (String::new(), k.detail.clone())
                    } else {
                        (k.detail.clone(), String::new())
                    };
                    rows.push([
                        t(&format!("doc.kind.{}", k.kind)),
                        k.name.clone().unwrap_or_default(),
                        cols,
                        String::new(),
                        detail,
                    ]);
                }
                for (kind, keys) in
                    [("doc.kind.foreign_key", &tb.foreign_keys), ("doc.kind.logical_key", &tb.logical_keys)]
                {
                    for k in keys {
                        let mut detail = Vec::new();
                        if let Some(d) = &k.on_delete {
                            detail.push(format!("{}: {d}", t("doc.col.on_delete")));
                        }
                        if let Some(u) = &k.on_update {
                            detail.push(format!("{}: {u}", t("doc.col.on_update")));
                        }
                        detail.extend(k.comment.clone());
                        rows.push([
                            t(kind),
                            k.name.clone().unwrap_or_default(),
                            k.columns.clone(),
                            format!("{}.{} ({})", k.ref_schema, k.ref_table, k.ref_columns),
                            detail.join("; "),
                        ]);
                    }
                }
                for r in rows {
                    sh.cell(0, &s.name)?;
                    sh.cell(1, &tb.name)?;
                    for (i, v) in r.iter().enumerate() {
                        sh.cell(i + 2, v)?;
                    }
                    sh.next();
                }
            }
        }
        sh.finish()?;
    }

    let bytes = book.save_to_buffer().map_err(xlsx_error)?;
    Ok((bytes, truncated))
}

#[cfg(test)]
mod tests {
    use std::io::{Cursor, Read};

    use super::super::content::tests::{opts, sample};
    use super::super::content::{DocKind, build};
    use super::*;

    fn part(bytes: &[u8], name: &str) -> String {
        let mut z = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
        let mut s = String::new();
        z.by_name(name).unwrap().read_to_string(&mut s).unwrap();
        s
    }

    #[test]
    fn three_sheets_with_links_and_truncation() {
        let mut src = sample();
        src[0].tables[0].table.comment = Some("x".repeat(CELL_MAX + 5));
        let c = build(opts(DocKind::Dictionary), src, vec![]);
        let (bytes, truncated) = write(&c, Lang::ZhCn).unwrap();
        // orders 的注释超长（表清单一处）
        assert_eq!(truncated, 1);
        let book = part(&bytes, "xl/workbook.xml");
        for name in ["表清单", "列", "索引和约束"] {
            assert!(book.contains(&format!("name=\"{name}\"")), "{book}");
        }
        let strings = part(&bytes, "xl/sharedStrings.xml");
        for s in [
            "customers",
            "code DESC",
            "逻辑外键",
            "shop.orders (id)",
            "删除时: CASCADE",
            "生成列",
            "amount &gt;= 0",
        ] {
            assert!(strings.contains(s), "{s}");
        }
        // 表名链接到"列"工作表中的第一行：customers 在第 2 行，items 在第 5 行
        let sheet1 = part(&bytes, "xl/worksheets/sheet1.xml");
        assert!(
            sheet1.contains("location=\"'列'!A2\"") && sheet1.contains("location=\"'列'!A5\""),
            "{sheet1}"
        );
        assert!(sheet1.contains("<autoFilter ref=\"A1:D5\"/>"));
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

Expected: 全部测试通过（service 新增 1 个）。

- [ ] **Step 6: 提交**

```bash
git add crates/dblens-service/src/document/labels.json crates/dblens-service/src/document/mod.rs crates/dblens-service/src/document/template.rs crates/dblens-service/src/document/xlsx.rs
git commit -m "feat(document): 数据字典的 xlsx" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 6: 后台任务与接口；实库测试

- `document/job.rs`（新）：`DocFormat`、`DocSource::{Model, Database}`、`DiagramImage`、`DocumentRequest`、`DocumentProblem { field, error }`；`Service::check_document`（xlsx 只用于数据字典；浏览器模式不接受本机路径；模板语法；样式来源；输出目录）、`Service::start_document`（检查、读模板和样式来源、取走上传的图片、`TaskKind::Document` 任务：读结构 → 组织内容 → 写出；写到本机路径，或浏览器模式写到任务目录作为报告；markdown 的图片在旁边的 `<文件名>_images/`，浏览器模式打包 zip；xlsx 截断数记在 `warnings`）、`Service::export_document_template`。模型来源读默认图（带逻辑外键、读不出的表），库来源跳过元数据缓存、设计文档另读视图。
- `task.rs`：`TaskKind::Document`。`model/ops.rs` 的测试辅助 `service`、`wait` 改为 `pub(crate)`（`model::tests`）。
- `dblens-api/src/documents.rs`（新）：`POST /documents`（201 `{id}`）、`/documents/check`（`{problems}`）、`/documents/templates/export`（要求 `local_files`）。
- `tests/live.rs` 的 `model_flow`：反向生成检查之后，对测试 schema 导出 markdown 数据字典，核对表和指向 `m4_customer` 的外键链接。
- 后端词条加 `document.read_failed`、`document.bad_output`、`document.bad_image`、`document.xlsx_dictionary_only`。

**Files:**
- Create: `crates/dblens-api/src/documents.rs`
- Modify: `crates/dblens-api/src/lib.rs`
- Modify: `crates/dblens-api/tests/models.rs`
- Modify: `crates/dblens-core/locales/app.json`
- Create: `crates/dblens-service/src/document/job.rs`
- Modify: `crates/dblens-service/src/document/mod.rs`
- Modify: `crates/dblens-service/src/model/mod.rs`
- Modify: `crates/dblens-service/src/model/ops.rs`
- Modify: `crates/dblens-service/src/task.rs`
- Modify: `crates/dblens-service/tests/live.rs`

**Interfaces:**
- Consumes：任务 2–5；M4b 的 `read_diagram`、`DEFAULT_DIAGRAM`；M2b 的上传和任务。
- Produces（`dblens_service::document`）：`DocumentRequest`、`DocSource`、`DocFormat`、`DiagramImage`、`DocumentProblem`；`Service::{check_document(&req, allow_path) -> Vec<DocumentProblem>, start_document(req, allow_path) -> Result<String>, export_document_template(format, path) -> Result<()>}`；接口见上。

- [ ] **Step 1: 修改 `crates/dblens-api/tests/models.rs`**

```diff
--- a/crates/dblens-api/tests/models.rs
+++ b/crates/dblens-api/tests/models.rs
@@ -65,6 +65,20 @@ async fn models_need_local_files() {
     let (st, _) =
         call(&app, "POST", "/api/files/image", Some(json!({ "path": "/tmp/x.png", "data": "" }))).await;
     assert_eq!(st, StatusCode::FORBIDDEN);
+    let (st, _) = call(
+        &app,
+        "POST",
+        "/api/documents/templates/export",
+        Some(json!({ "format": "html", "path": "/tmp/t.html" })),
+    )
+    .await;
+    assert_eq!(st, StatusCode::FORBIDDEN);
+    // 浏览器模式：本机路径作为检查出的问题
+    let req = json!({ "source": { "kind": "database", "profile": "p", "schema": "s" }, "kind": "dictionary",
+                      "format": "html", "template": "/tmp/t.html" });
+    let (st, v) = call(&app, "POST", "/api/documents/check", Some(req)).await;
+    assert_eq!(st, StatusCode::OK);
+    assert_eq!(v["problems"][0]["field"], "template", "{v}");
 }
 
 /// ER 图与逻辑外键（M4b）。
@@ -242,3 +256,56 @@ async fn edit_compare_and_write_back() {
     let (st, _) = call(&app, "GET", &format!("/api/models/{b}"), None).await;
     assert_eq!(st, StatusCode::NOT_FOUND);
 }
+
+/// 文档导出（M4c）：检查、导出内置模板、模型导出数据字典后下载。
+#[tokio::test]
+async fn documents() {
+    let app = app(true);
+    let dir = tempfile::tempdir().unwrap();
+    let m = new_model(&app, dir.path(), "m").await;
+    let save = json!({ "schema": "public", "name": null, "draft": draft("orders", &["id", "x"]), "fingerprint": null });
+    let (st, v) = call(&app, "POST", &format!("/api/models/{m}/tables"), Some(save)).await;
+    assert_eq!(st, StatusCode::OK, "{v}");
+    let source = json!({ "kind": "model", "model": m, "schemas": ["public"] });
+    // xlsx 只能用于数据字典
+    let req = json!({ "source": source, "kind": "design", "format": "xlsx" });
+    let (_, v) = call(&app, "POST", "/api/documents/check", Some(req.clone())).await;
+    assert_eq!(v["problems"][0]["field"], "format", "{v}");
+    let (st, _) = call(&app, "POST", "/api/documents", Some(req)).await;
+    assert_eq!(st, StatusCode::BAD_REQUEST);
+    // 导出内置模板
+    let path = dir.path().join("t.md");
+    let (st, _) = call(
+        &app,
+        "POST",
+        "/api/documents/templates/export",
+        Some(json!({ "format": "markdown", "path": path })),
+    )
+    .await;
+    assert_eq!(st, StatusCode::NO_CONTENT);
+    assert!(std::fs::read_to_string(&path).unwrap().contains("{{ info.title }}"));
+    // 用这份模板导出数据字典，写到本机路径
+    let out = dir.path().join("dict.md");
+    let req = json!({ "source": source, "kind": "dictionary", "format": "markdown", "template": path,
+                      "info": { "title": "订单库" }, "output": out });
+    let (st, v) = call(&app, "POST", "/api/documents", Some(req)).await;
+    assert_eq!(st, StatusCode::CREATED, "{v}");
+    let id = v["id"].as_str().unwrap().to_string();
+    for _ in 0..500 {
+        let (_, t) = call(&app, "GET", &format!("/api/tasks/{id}"), None).await;
+        if t["state"] != "running" {
+            assert_eq!(t["state"], "done", "{t}");
+            assert_eq!(t["kind"], "document");
+            break;
+        }
+        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
+    }
+    let md = std::fs::read_to_string(&out).unwrap();
+    assert!(
+        md.starts_with(
+            "# 订单库
+"
+        ) && md.contains("public.orders"),
+        "{md}"
+    );
+}
```

- [ ] **Step 2: 修改 `crates/dblens-service/tests/live.rs`**

```diff
--- a/crates/dblens-service/tests/live.rs
+++ b/crates/dblens-service/tests/live.rs
@@ -20,6 +20,7 @@ use dblens_dialect::design::{Renames, TableChange, diff_table};
 use dblens_dialect::meta::{MetaArgs, MetaKey};
 use dblens_dialect::{Compat, DdlKind, Direction, RowChange, Variant, WriteMode};
 use dblens_driver::{ConnectParams, DriverId};
+use dblens_service::document::{DocFormat, DocInfo, DocKind, DocSource, DocumentRequest};
 use dblens_service::{
     ChangeSet, ColumnMapping, CompareRequest, CompareResult, CompareSideRef, DEFAULT_BATCH, DEFAULT_DIAGRAM,
     DataOrder, DefaultPolicy, DesignRequest, DriverRegistry, Environment, ExecRequest, ExportFormat,
@@ -1633,6 +1634,26 @@ async fn model_flow(env: &str) {
     assert_eq!((fk.columns.clone(), fk.ref_columns.clone()), (vec![n("cust_id")], vec![n("id")]), "{env}");
     assert!(v.tables.iter().any(|t| t.table.name == n("m4_customer")), "{env}");
 
+    // 文档导出（M4c）：库的这个 schema 导出 markdown 数据字典，表和外键（链接到被引用的表）都在
+    let out = dir.path().join("dict.md");
+    let req = DocumentRequest {
+        source: DocSource::Database { profile: live.profile_id.clone(), schema: schema.clone() },
+        tables: None,
+        kind: DocKind::Dictionary,
+        format: DocFormat::Markdown,
+        info: DocInfo::default(),
+        template: None,
+        reference: None,
+        view_definitions: true,
+        diagrams: vec![],
+        output: Some(out.clone()),
+    };
+    let st = wait_task(s, &s.start_document(req, true).await.unwrap()).await;
+    assert_eq!(st.state, TaskState::Done, "{env}：{st:?}");
+    let md = std::fs::read_to_string(&out).unwrap();
+    assert!(md.contains(&format!("</a>{schema}.{}", n("m4_ord"))), "{env}：{md}");
+    assert!(md.contains(&format!("[{schema}.{}](#t", n("m4_customer"))), "{env}：{md}");
+
     // 6. 库对库：同一个库中的第二个 schema
     let other: Option<&'static str> = match live.driver {
         DriverId::Postgres => Some("m4_b"),
```

- [ ] **Step 3: 新建 `crates/dblens-api/src/documents.rs`**

```rust
//! 文档导出的接口（M4c 设计文档 6.2 节）。本机路径（模板、样式来源、输出位置）只有 `local_files` 时接受；
//! 浏览器模式用内置模板，完成后从任务的报告下载。

use std::path::PathBuf;

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use dblens_service::document::{DocumentRequest, TextFormat};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::AppState;
use crate::error::ApiResult;
use crate::models::local_only;

/// 开始导出：返回任务 id。
pub async fn start(
    State(s): State<AppState>,
    Json(req): Json<DocumentRequest>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    let id = s.service.start_document(req, s.config.local_files).await?;
    Ok((StatusCode::CREATED, Json(json!({ "id": id }))))
}

/// 开始前检查：模板语法、样式来源、输出位置。
pub async fn check(State(s): State<AppState>, Json(req): Json<DocumentRequest>) -> ApiResult<Json<Value>> {
    Ok(Json(json!({ "problems": s.service.check_document(&req, s.config.local_files) })))
}

#[derive(Deserialize)]
pub struct TemplateBody {
    format: TextFormat,
    path: PathBuf,
}

/// 把内置模板写到指定路径。
pub async fn export_template(
    State(s): State<AppState>,
    Json(b): Json<TemplateBody>,
) -> ApiResult<StatusCode> {
    local_only(&s)?;
    s.service.export_document_template(b.format, &b.path)?;
    Ok(StatusCode::NO_CONTENT)
}
```

- [ ] **Step 4: 修改 `crates/dblens-api/src/lib.rs`**

```diff
--- a/crates/dblens-api/src/lib.rs
+++ b/crates/dblens-api/src/lib.rs
@@ -4,6 +4,7 @@
 mod auth;
 mod design;
 mod diagrams;
+mod documents;
 mod error;
 mod exports;
 mod lang;
@@ -114,6 +115,9 @@ pub fn router(service: Arc<Service>, config: ApiConfig) -> Router {
         .route("/models/{id}/tables/{schema}/{name}/logical-keys", post(diagrams::add_key))
         .route("/models/{id}/tables/{schema}/{name}/logical-keys/{index}", delete(diagrams::delete_key))
         .route("/files/image", post(diagrams::save_image))
+        .route("/documents", post(documents::start))
+        .route("/documents/check", post(documents::check))
+        .route("/documents/templates/export", post(documents::export_template))
         .route("/exports", post(exports::create))
         .route("/exports/{id}", get(exports::status).delete(exports::cancel_or_remove))
         .route("/exports/{id}/file", get(exports::file))
```

- [ ] **Step 5: 修改 `crates/dblens-core/locales/app.json`**

```diff
--- a/crates/dblens-core/locales/app.json
+++ b/crates/dblens-core/locales/app.json
@@ -61,9 +61,13 @@
   "dm.no_diagnostics": {"zh-CN": "调用 DPI 失败，且无法读取错误信息", "en": "A DPI call failed and its error information could not be read"},
   "dm.nul_in_string": {"zh-CN": "字符串中不能包含 \\0 字符", "en": "Strings cannot contain the \\0 character"},
   "dml.no_insertable_columns": {"zh-CN": "没有可插入的列：%{table}", "en": "No insertable columns: %{table}"},
+  "document.bad_image": {"zh-CN": "ER 图 %{name} 的图片不是 PNG", "en": "The image of ER diagram %{name} is not a PNG"},
+  "document.bad_output": {"zh-CN": "不能写到这个位置：%{path}", "en": "Cannot write to this location: %{path}"},
   "document.bad_reference": {"zh-CN": "样式来源不是有效的 .docx 文件：%{error}", "en": "The style reference is not a valid .docx file: %{error}"},
+  "document.read_failed": {"zh-CN": "读文件 %{path} 失败：%{error}", "en": "Failed to read %{path}: %{error}"},
   "document.template_error": {"zh-CN": "模板第 %{line} 行有错：%{error}", "en": "Template error at line %{line}: %{error}"},
   "document.write_failed": {"zh-CN": "写文档失败：%{error}", "en": "Failed to write the document: %{error}"},
+  "document.xlsx_dictionary_only": {"zh-CN": "xlsx 只能用于数据字典", "en": "xlsx is only available for data dictionaries"},
   "driver.connect_timeout": {"zh-CN": "连接超时（%{seconds} 秒）", "en": "Connection timed out (%{seconds} s)"},
   "driver.connection_closed": {"zh-CN": "连接已断开：%{error}", "en": "Connection closed: %{error}"},
   "driver.lob_read_failed": {"zh-CN": "读取 LOB 失败：%{error}", "en": "Failed to read LOB: %{error}"},
```

- [ ] **Step 6: 新建 `crates/dblens-service/src/document/job.rs`**

```rust
//! 文档导出的后台任务（M4c 设计文档 6.1 节）：读结构、组织内容、按格式写出，写到选定的路径或任务目录。

use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use dblens_core::{Error, Result, current_lang, msg};
use dblens_dialect::meta::{MetaArgs, MetaKey};
use serde::{Deserialize, Serialize};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

use super::content::{
    BuildOptions, DocDiagram, DocInfo, DocKind, SourceSchema, SourceTable, build, png_size,
};
use super::template::{TextFormat, check, render};
use super::{docx, xlsx};
use crate::model::DEFAULT_DIAGRAM;
use crate::service::Service;
use crate::task::{FileSource, TaskKind, TaskPhase};

/// 文档格式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DocFormat {
    Docx,
    Html,
    Markdown,
    /// 只用于数据字典。
    Xlsx,
}

impl DocFormat {
    fn extension(self) -> &'static str {
        match self {
            DocFormat::Docx => "docx",
            DocFormat::Html => "html",
            DocFormat::Markdown => "md",
            DocFormat::Xlsx => "xlsx",
        }
    }

    fn text(self) -> Option<TextFormat> {
        match self {
            DocFormat::Html => Some(TextFormat::Html),
            DocFormat::Markdown => Some(TextFormat::Markdown),
            _ => None,
        }
    }
}

/// 文档的来源。
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DocSource {
    /// 打开的模型中的若干 schema。
    Model { model: String, schemas: Vec<String> },
    /// 连接的一个 schema。
    Database { profile: String, schema: String },
}

/// 一张 ER 图的图片（前端画好后上传）。
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct DiagramImage {
    pub upload: String,
    pub schema: String,
    pub name: String,
    pub title: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub names_only: bool,
}

fn yes() -> bool {
    true
}

/// 开始导出的请求。
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct DocumentRequest {
    pub source: DocSource,
    /// 只放这些表（只有一个 schema 时）。
    #[serde(default)]
    pub tables: Option<Vec<String>>,
    pub kind: DocKind,
    pub format: DocFormat,
    #[serde(default)]
    pub info: DocInfo,
    /// html、markdown 的模板文件（本机路径）。
    #[serde(default)]
    pub template: Option<PathBuf>,
    /// docx 的样式来源（本机路径）。
    #[serde(default)]
    pub reference: Option<PathBuf>,
    #[serde(default = "yes")]
    pub view_definitions: bool,
    #[serde(default)]
    pub diagrams: Vec<DiagramImage>,
    /// 桌面版：写到这个路径；没有时写到任务目录，完成后下载。
    #[serde(default)]
    pub output: Option<PathBuf>,
}

/// 开始前检查出的问题。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DocumentProblem {
    /// `template`、`reference`、`output`、`format`。
    pub field: &'static str,
    pub error: Error,
}

fn read_file(path: &Path) -> Result<Vec<u8>> {
    std::fs::read(path)
        .map_err(|e| Error::invalid(msg!("document.read_failed", path = path.display(), error = e)))
}

/// 文件名中不能用的字符换成下划线。
fn file_name(name: &str) -> String {
    let s: String =
        name.chars().map(|c| if r#"\/:*?"<>|"#.contains(c) || c.is_control() { '_' } else { c }).collect();
    let s = s.trim().trim_end_matches('.').to_string();
    if s.is_empty() { "document".into() } else { s }
}

/// 写文件：先写临时文件再改名。
fn write_atomic(path: &Path, data: &[u8]) -> Result<()> {
    let tmp = path.with_extension("dblens-tmp");
    let fail = |e: std::io::Error| Error::internal(msg!("document.write_failed", error = e));
    std::fs::write(&tmp, data).map_err(fail)?;
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        fail(e)
    })
}

impl Service {
    /// 开始前的检查：格式与种类、模板语法、样式来源、输出位置。`allow_path` 为 false 时（浏览器模式）不接受本机路径。
    pub fn check_document(&self, req: &DocumentRequest, allow_path: bool) -> Vec<DocumentProblem> {
        let mut out = Vec::new();
        let mut push = |field, error: Error| out.push(DocumentProblem { field, error });
        if req.format == DocFormat::Xlsx && req.kind == DocKind::Design {
            push("format", Error::invalid(msg!("document.xlsx_dictionary_only")));
        }
        for (field, path) in
            [("template", &req.template), ("reference", &req.reference), ("output", &req.output)]
        {
            if path.is_some() && !allow_path {
                push(field, Error::invalid(msg!("export.path_not_allowed")));
            }
        }
        if !allow_path {
            return out;
        }
        if let Some(p) = &req.template {
            match (read_file(p), req.format.text()) {
                (Err(e), _) => push("template", e),
                (Ok(bytes), Some(f)) => {
                    if let Err(e) = check(f, &String::from_utf8_lossy(&bytes)) {
                        push("template", e);
                    }
                }
                (Ok(_), None) => {}
            }
        }
        if let Some(p) = &req.reference
            && req.format == DocFormat::Docx
            && let Err(e) = read_file(p).and_then(|b| docx::check_reference(&b))
        {
            push("reference", e);
        }
        if let Some(p) = &req.output {
            let dir = p.parent().filter(|d| !d.as_os_str().is_empty());
            if !p.is_absolute() || dir.is_none_or(|d| !d.is_dir()) {
                push("output", Error::invalid(msg!("document.bad_output", path = p.display())));
            }
        }
        out
    }

    /// 读一个来源的结构（模型的 schema 带逻辑外键和读不出的表；库的 schema 带视图）。
    async fn document_sources(
        &self,
        source: &DocSource,
        design: bool,
        done: impl Fn(),
    ) -> Result<Vec<SourceSchema>> {
        let mut out = Vec::new();
        match source {
            DocSource::Model { model, schemas } => {
                for schema in schemas {
                    let view = self.models.read_diagram(model, schema, DEFAULT_DIAGRAM)?;
                    let mut s = SourceSchema { name: schema.clone(), ..Default::default() };
                    for t in view.tables {
                        match t.error {
                            Some(e) => s.errors.push((t.table.name, e)),
                            None => {
                                s.tables.push(SourceTable { table: t.table, logical_keys: t.logical_keys })
                            }
                        }
                    }
                    out.push(s);
                    done();
                }
            }
            DocSource::Database { profile, schema } => {
                let p = self.profile(profile)?;
                // 跳过元数据缓存，以库中最新的结构为准
                let args = MetaArgs { schema: Some(schema.clone()), ..Default::default() };
                for key in MetaKey::TABLE_DETAIL.into_iter().chain([MetaKey::Views]) {
                    self.metadata.cache().forget(&p.id, key, &args);
                }
                let tables = self.metadata.tables(&p, schema, None).await?;
                let views =
                    if design { self.metadata.views(&p, schema, MetaKey::Views).await? } else { vec![] };
                out.push(SourceSchema {
                    name: schema.clone(),
                    tables: tables
                        .into_iter()
                        .map(|table| SourceTable { table, logical_keys: vec![] })
                        .collect(),
                    views,
                    errors: vec![],
                });
                done();
            }
        }
        Ok(out)
    }

    /// 来源的说明和默认标题。
    fn document_origin(&self, source: &DocSource) -> Result<(String, String)> {
        match source {
            DocSource::Model { model, .. } => {
                let m = self.models.summary_of(model)?;
                Ok((format!("{} ({})", m.info.name, m.path), m.info.name))
            }
            DocSource::Database { profile, schema } => {
                let p = self.profile(profile)?;
                Ok((format!("{} / {schema}", p.input.name), schema.clone()))
            }
        }
    }

    /// 开始导出：作为任务执行，返回任务 id。浏览器模式（`allow_path` 为 false）不接受本机路径，结果在任务完成后下载。
    pub async fn start_document(self: &Arc<Self>, req: DocumentRequest, allow_path: bool) -> Result<String> {
        if let Some(p) = self.check_document(&req, allow_path).into_iter().next() {
            return Err(p.error);
        }
        let lang = current_lang();
        let template = match &req.template {
            Some(p) if req.format.text().is_some() => {
                Some(String::from_utf8_lossy(&read_file(p)?).to_string())
            }
            _ => None,
        };
        let reference = match &req.reference {
            Some(p) if req.format == DocFormat::Docx => Some(read_file(p)?),
            _ => None,
        };
        let (origin, default_title) = self.document_origin(&req.source)?;
        // 图片：任务开始前取走上传的文件
        let mut diagrams = Vec::new();
        if req.kind == DocKind::Design {
            for (i, d) in req.diagrams.iter().enumerate() {
                let f = self.tasks.resolve(&FileSource::Upload { upload: d.upload.clone() }, false, true)?;
                let png = std::fs::read(&f.path);
                f.release();
                let png =
                    png.map_err(|e| Error::invalid(msg!("document.read_failed", path = &f.name, error = e)))?;
                let (width, height) = png_size(&png)
                    .ok_or_else(|| Error::invalid(msg!("document.bad_image", name = &d.title)))?;
                diagrams.push(DocDiagram {
                    schema: d.schema.clone(),
                    name: d.name.clone(),
                    title: d.title.clone(),
                    description: d.description.clone(),
                    names_only: d.names_only,
                    file: file_name(&format!("{:02}-{}-{}.png", i + 1, d.schema, d.name)),
                    src: String::new(),
                    width,
                    height,
                    png,
                });
            }
        }
        let schemas = match &req.source {
            DocSource::Model { schemas, .. } => schemas.len(),
            DocSource::Database { .. } => 1,
        };
        if schemas == 0 {
            return Err(Error::invalid(msg!("model.no_schemas")));
        }
        let task = self.tasks.create(TaskKind::Document)?;
        let mut id = String::new();
        task.update(|s| {
            id = s.id.clone();
            s.phase = TaskPhase::Scanning;
            s.progress.total = schemas as u64;
        });
        let service = self.clone();
        tokio::spawn(async move {
            let run = async {
                let design = req.kind == DocKind::Design;
                let sources = service
                    .document_sources(&req.source, design, || task.update(|s| s.progress.read += 1))
                    .await?;
                if task.cancelled() {
                    return Err(Error::Cancelled);
                }
                task.update(|s| s.phase = TaskPhase::Writing);
                let mut info = req.info.clone();
                if info.title.trim().is_empty() {
                    info.title = default_title;
                }
                info.generated = chrono::Local::now().format("%Y-%m-%d %H:%M").to_string();
                info.source = origin;
                let name = file_name(&info.title);
                let content = build(
                    BuildOptions {
                        kind: req.kind,
                        info,
                        tables: req.tables.clone(),
                        view_definitions: req.view_definitions,
                    },
                    sources,
                    diagrams,
                );
                let images = format!("{name}_images");
                let main = match req.format {
                    DocFormat::Docx => docx::write(&content, reference.as_deref(), lang)?,
                    DocFormat::Xlsx => {
                        let (bytes, truncated) = xlsx::write(&content, lang)?;
                        task.update(|s| s.warnings = truncated);
                        bytes
                    }
                    DocFormat::Html | DocFormat::Markdown => {
                        let f = req.format.text().unwrap_or(TextFormat::Html);
                        render(&content, f, template.as_deref(), lang, &images)?.into_bytes()
                    }
                };
                // markdown 的图片放在旁边的目录中
                let pictures: Vec<(String, &[u8])> = if req.format == DocFormat::Markdown {
                    content.diagrams.iter().map(|d| (d.file.clone(), d.png.as_slice())).collect()
                } else {
                    vec![]
                };
                if task.cancelled() {
                    return Err(Error::Cancelled);
                }
                let ext = req.format.extension();
                match &req.output {
                    Some(path) => {
                        write_atomic(path, &main)?;
                        if !pictures.is_empty() {
                            let stem =
                                path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or(name);
                            let dir = path.with_file_name(format!("{stem}_images"));
                            std::fs::create_dir_all(&dir)
                                .map_err(|e| Error::internal(msg!("document.write_failed", error = e)))?;
                            for (file, png) in &pictures {
                                write_atomic(&dir.join(file), png)?;
                            }
                        }
                    }
                    None if pictures.is_empty() => {
                        let path = task.report_path(ext, format!("{name}.{ext}"))?;
                        write_atomic(&path, &main)?;
                    }
                    None => {
                        // 浏览器模式下 markdown 带图片：打包成 zip
                        let path = task.report_path("zip", format!("{name}.zip"))?;
                        let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
                        let opts =
                            SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
                        let fail = |e: &dyn std::fmt::Display| {
                            Error::internal(msg!("document.write_failed", error = e))
                        };
                        zip.start_file(format!("{name}.{ext}"), opts).map_err(|e| fail(&e))?;
                        zip.write_all(&main).map_err(|e| fail(&e))?;
                        for (file, png) in &pictures {
                            zip.start_file(format!("{images}/{file}"), opts).map_err(|e| fail(&e))?;
                            zip.write_all(png).map_err(|e| fail(&e))?;
                        }
                        let bytes = zip.finish().map_err(|e| fail(&e))?.into_inner();
                        write_atomic(&path, &bytes)?;
                    }
                }
                Ok(())
            };
            let result = run.await;
            task.finish(result);
        });
        Ok(id)
    }

    /// 把一份内置模板写到指定路径（只有本机服务可用）。
    pub fn export_document_template(&self, format: TextFormat, path: &Path) -> Result<()> {
        write_atomic(path, format.builtin().as_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::tests::{service, wait};

    fn req(format: DocFormat, kind: DocKind) -> DocumentRequest {
        DocumentRequest {
            source: DocSource::Database { profile: "p".into(), schema: "s".into() },
            tables: None,
            kind,
            format,
            info: DocInfo::default(),
            template: None,
            reference: None,
            view_definitions: true,
            diagrams: vec![],
            output: None,
        }
    }

    #[test]
    fn checks_before_start() {
        let (s, _, _dir) = service();
        assert!(s.check_document(&req(DocFormat::Markdown, DocKind::Design), false).is_empty());
        let p = s.check_document(&req(DocFormat::Xlsx, DocKind::Design), true);
        assert_eq!(p[0].field, "format");
        // 浏览器模式不接受本机路径
        let mut r = req(DocFormat::Html, DocKind::Dictionary);
        r.template = Some("C:/t.html".into());
        r.output = Some("C:/out.html".into());
        let fields: Vec<_> = s.check_document(&r, false).iter().map(|p| p.field).collect();
        assert_eq!(fields, ["template", "output"]);
        // 模板语法错误、样式来源无效、输出目录不存在
        let dir = tempfile::tempdir().unwrap();
        let t = dir.path().join("t.html");
        std::fs::write(&t, "{% for %}").unwrap();
        let bad = dir.path().join("ref.docx");
        std::fs::write(&bad, "not a docx").unwrap();
        let mut r = req(DocFormat::Html, DocKind::Dictionary);
        r.template = Some(t);
        r.output = Some(dir.path().join("missing").join("x.html"));
        let fields: Vec<_> = s.check_document(&r, true).iter().map(|p| p.field).collect();
        assert_eq!(fields, ["template", "output"]);
        let mut r = req(DocFormat::Docx, DocKind::Dictionary);
        r.reference = Some(bad);
        r.output = Some(dir.path().join("x.docx"));
        let fields: Vec<_> = s.check_document(&r, true).iter().map(|p| p.field).collect();
        assert_eq!(fields, ["reference"]);
    }

    #[test]
    fn safe_file_names() {
        assert_eq!(file_name("a/b:c*?"), "a_b_c__");
        assert_eq!(file_name("  "), "document");
        assert_eq!(file_name("报告."), "报告");
    }

    fn png() -> Vec<u8> {
        let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
        png.extend(800u32.to_be_bytes());
        png.extend(300u32.to_be_bytes());
        png
    }

    /// 打开一个有两张表、一个逻辑外键的模型。
    fn model(s: &Service, dir: &Path) -> String {
        use crate::document::content::tests::col;
        use crate::model::{LogicalKey, ModelDatabase};
        use dblens_core::schema::Table;
        use dblens_dialect::{Compat, Variant};
        let db = ModelDatabase { variant: Variant::Postgres, compat: Compat::Native, version: None };
        let tables = vec![
            Table {
                name: "orders".into(),
                columns: vec![col("id", "integer"), col("cust_id", "integer")],
                ..Default::default()
            },
            Table { name: "customers".into(), columns: vec![col("id", "integer")], ..Default::default() },
        ];
        let m = s.models.create_from(&dir.join("model"), "shop", db, &[("public".into(), tables)]).unwrap();
        let key = LogicalKey {
            name: None,
            columns: vec!["cust_id".into()],
            ref_schema: None,
            ref_table: "customers".into(),
            ref_columns: vec!["id".into()],
            comment: None,
        };
        s.models.add_logical_key(&m.id, "public", "orders", key, None).unwrap();
        m.id
    }

    fn upload(s: &Service) -> String {
        let (id, path) = s.tasks.begin_upload().unwrap();
        std::fs::write(&path, png()).unwrap();
        s.tasks.finish_upload(&id, "er.png", 1).unwrap();
        id
    }

    #[tokio::test]
    async fn model_to_markdown_file_with_images() {
        let (s, _, dir) = service();
        let id = model(&s, dir.path());
        let out = dir.path().join("shop.md");
        let mut r = req(DocFormat::Markdown, DocKind::Design);
        r.source = DocSource::Model { model: id, schemas: vec!["public".into()] };
        r.output = Some(out.clone());
        r.diagrams = vec![DiagramImage {
            upload: upload(&s),
            schema: "public".into(),
            name: "_all".into(),
            title: "public".into(),
            description: None,
            names_only: false,
        }];
        let task = s.start_document(r, true).await.unwrap();
        let st = wait(&s, &task).await;
        assert_eq!(st.state, crate::task::TaskState::Done, "{st:?}");
        let md = std::fs::read_to_string(&out).unwrap();
        // 标题默认取模型名；逻辑外键；图片在旁边的目录中
        assert!(
            md.starts_with(
                "# shop
"
            ),
            "{md}"
        );
        assert!(md.contains("public.customers") && md.contains("![public](shop_images/01-public-_all.png)"));
        assert_eq!(std::fs::read(dir.path().join("shop_images/01-public-_all.png")).unwrap(), png());
    }

    #[tokio::test]
    async fn browser_mode_zips_markdown_and_rejects_paths() {
        let (s, _, dir) = service();
        let id = model(&s, dir.path());
        let mut r = req(DocFormat::Markdown, DocKind::Design);
        r.source = DocSource::Model { model: id.clone(), schemas: vec!["public".into()] };
        r.info.title = "商城/文档".into();
        r.diagrams = vec![DiagramImage {
            upload: upload(&s),
            schema: "public".into(),
            name: "_all".into(),
            title: "public".into(),
            description: None,
            names_only: true,
        }];
        let task = s.start_document(r.clone(), false).await.unwrap();
        assert_eq!(wait(&s, &task).await.state, crate::task::TaskState::Done);
        let (path, name) = s.tasks.report(&task).unwrap();
        assert_eq!(name, "商城_文档.zip");
        let mut z = zip::ZipArchive::new(std::fs::File::open(path).unwrap()).unwrap();
        let names: Vec<String> =
            (0..z.len()).map(|i| z.by_index(i).unwrap().name().unwrap().to_string()).collect();
        assert_eq!(names, ["商城_文档.md", "商城_文档_images/01-public-_all.png"]);
        // 没有图片的格式直接下载文件
        let mut r2 = req(DocFormat::Xlsx, DocKind::Dictionary);
        r2.source = DocSource::Model { model: id, schemas: vec!["public".into()] };
        let task = s.start_document(r2, false).await.unwrap();
        assert_eq!(wait(&s, &task).await.state, crate::task::TaskState::Done);
        assert_eq!(s.tasks.report(&task).unwrap().1, "shop.xlsx");
        // 浏览器模式不接受本机路径
        r.output = Some(dir.path().join("x.md"));
        assert!(s.start_document(r, false).await.is_err());
    }
}
```

- [ ] **Step 7: 修改 `crates/dblens-service/src/document/mod.rs`**

```diff
--- a/crates/dblens-service/src/document/mod.rs
+++ b/crates/dblens-service/src/document/mod.rs
@@ -2,6 +2,7 @@
 
 mod content;
 mod docx;
+mod job;
 mod labels;
 mod template;
 mod xlsx;
@@ -12,6 +13,7 @@ pub use content::{
     SourceSchema, SourceTable, build, png_size,
 };
 pub use docx::{builtin_reference, check_reference, write as write_docx};
+pub use job::{DiagramImage, DocFormat, DocSource, DocumentProblem, DocumentRequest};
 pub use labels::label;
 pub use template::{TextFormat, check, render};
 pub use xlsx::write as write_xlsx;
```

- [ ] **Step 8: 修改 `crates/dblens-service/src/model/mod.rs`**

```diff
--- a/crates/dblens-service/src/model/mod.rs
+++ b/crates/dblens-service/src/model/mod.rs
@@ -10,5 +10,7 @@ pub use diagram::{
     DiagramWarning, Position,
 };
 pub use file::{LogicalKey, ModelDatabase, ModelInfo, SchemaRenames};
+#[cfg(test)]
+pub(crate) use ops::tests;
 pub use ops::{ModelDesignPreview, ModelDesignRequest, NewModel, RECENT_MODELS_SETTING, ReverseRequest};
 pub use store::{FileError, ModelStore, ModelSummary, ModelTableView, ModelWrite, SchemaTables, TableEntry};
```

- [ ] **Step 9: 修改 `crates/dblens-service/src/model/ops.rs`**

```diff
--- a/crates/dblens-service/src/model/ops.rs
+++ b/crates/dblens-service/src/model/ops.rs
@@ -208,7 +208,7 @@ impl Service {
 }
 
 #[cfg(test)]
-mod tests {
+pub(crate) mod tests {
     use std::time::Duration;
 
     use dblens_core::schema::{Column, PrimaryKey};
@@ -225,7 +225,7 @@ mod tests {
     use crate::store::Store;
     use crate::task::{TaskState, TaskStatus};
 
-    fn service() -> (Arc<Service>, String, tempfile::TempDir) {
+    pub(crate) fn service() -> (Arc<Service>, String, tempfile::TempDir) {
         let store = Arc::new(Store::open_in_memory().unwrap());
         let secrets = Arc::new(Secrets::with_backend(store.clone(), SecretBackend::Vault));
         secrets.init_vault("master").unwrap();
@@ -245,7 +245,7 @@ mod tests {
         (Arc::new(s), p.id, dir)
     }
 
-    async fn wait(s: &Service, id: &str) -> TaskStatus {
+    pub(crate) async fn wait(s: &Service, id: &str) -> TaskStatus {
         for _ in 0..1000 {
             let st = s.tasks.status(id).unwrap();
             if st.state != TaskState::Running {
```

- [ ] **Step 10: 修改 `crates/dblens-service/src/task.rs`**

```diff
--- a/crates/dblens-service/src/task.rs
+++ b/crates/dblens-service/src/task.rs
@@ -30,6 +30,8 @@ pub enum TaskKind {
     Reverse,
     /// 比对后同步到库（M4a）。
     Sync,
+    /// 导出文档（M4c）。
+    Document,
 }
 
 #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
```

- [ ] **Step 11: 检查**

```bash
export CARGO_INCREMENTAL=0
cargo fmt --all
cargo clippy --config "source.mirror.registry='sparse+https://rsproxy.cn/index/'" --workspace --all-targets -- -D warnings
DBLENS_LANG=en cargo test --config "source.mirror.registry='sparse+https://rsproxy.cn/index/'" --workspace
```

实库测试：在上面的 `cargo test` 前以内联环境变量给出原有 5 个库的 `DBLENS_TEST_MYSQL_URL`、`DBLENS_TEST_MARIADB_URL`、`DBLENS_TEST_PG_URL`、`DBLENS_TEST_ORACLE_URL`、`DBLENS_TEST_DM_URL`，以及国产库第一批的 `DBLENS_TEST_TIDB_URL`、`DBLENS_TEST_OCEANBASE_URL`（用户名写成 `admin%40test`）、`DBLENS_TEST_OPENGAUSS_URL`、`DBLENS_TEST_OPENGAUSS_A_URL`（库 `dblens_test_a`）、`DBLENS_TEST_OPENGAUSS_SHA_URL`（账号 `admin_sha`，库 `dblens_test_sha`）、`DBLENS_TEST_KINGBASE_URL`（54321）、`DBLENS_TEST_KINGBASE_PG_URL`（54322）。新目标的密码在测试虚拟机 root 的 `/root/dblens-test/creds.env`，用 `ssh … 'sudo cat …'` 读进当前 shell 的变量，不写进任何文件。只跑模型的测试：`cargo test … -p dblens-service --test live model -- --test-threads=1`。
- 人大金仓的开发版授权最多 10 个连接：它的测试要串行运行（`--test-threads=1`）。
- 达梦的测试与其他测试同时运行时偶发"网络通讯失败"，失败时用 `dameng_model` 单独重跑。

Expected: 全部测试通过（service 新增 4 个，api 新增 1 个、禁止的检查多 2 处）；模型的实库测试 11 个。

- [ ] **Step 12: 提交**

```bash
git add crates/dblens-api/src/documents.rs crates/dblens-api/src/lib.rs crates/dblens-api/tests/models.rs crates/dblens-core/locales/app.json crates/dblens-service/src/document/job.rs crates/dblens-service/src/document/mod.rs crates/dblens-service/src/model/mod.rs crates/dblens-service/src/model/ops.rs crates/dblens-service/src/task.rs crates/dblens-service/tests/live.rs
git commit -m "feat(document): 文档导出的后台任务与接口" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 7: 前端：导出文档

- `shared/api.ts`：`DocKind`、`DocFormat`、`DocSource`、`DocInfo`、`DocDiagramImage`、`DocumentRequest`、`DocumentProblem`；方法 `startDocument`、`checkDocument`、`exportDocTemplate`；任务的 `kind` 加 `document`。
- `features/er/flow.ts`、`limits.ts`（新）：图转成 React Flow 的节点和连线、`NODE_TYPES`；只显示表名的阈值 `LARGE`（不引入 React Flow，对话框也用）。`ErView.tsx` 改用它们，导出菜单加"导出设计文档…"。`er/export.ts` 的 `renderImage` 加像素倍数参数。
- `features/document/`（新）：`options.ts`（种类与格式的联动、按来源记住的选项 `ui.doc.*`、组成请求、文件名、像素倍数）、`dialog.ts`、`ExportDocDialog.tsx`（开始前检查并把问题显示在对应的项；桌面版先选保存位置；设计文档先画图并上传，可取消）、`render.tsx`（屏幕外画图，懒加载）、`track.tsx`（任务进度通知；浏览器版完成后下载，桌面版可在文件夹中显示）。
- 入口：模型面板模型、schema 右键"导出文档…"；对象树 schema 右键"导出文档…"；外壳挂上对话框。
- 词条：新文件 `locales/{zh-CN,en}/document.json`；`tree.menu.exportDoc`、`model.menu.exportDoc`、`er.export.designDoc`。

**Files:**
- Modify: `web/src/app/Shell.tsx`
- Create: `web/src/features/document/ExportDocDialog.tsx`
- Create: `web/src/features/document/dialog.ts`
- Create: `web/src/features/document/options.test.ts`
- Create: `web/src/features/document/options.ts`
- Create: `web/src/features/document/render.tsx`
- Create: `web/src/features/document/track.tsx`
- Modify: `web/src/features/er/ErView.tsx`
- Modify: `web/src/features/er/export.ts`
- Create: `web/src/features/er/flow.ts`
- Create: `web/src/features/er/limits.ts`
- Modify: `web/src/features/model/ModelsPanel.tsx`
- Modify: `web/src/features/tree/ObjectTree.tsx`
- Create: `web/src/locales/en/document.json`
- Modify: `web/src/locales/en/er.json`
- Modify: `web/src/locales/en/model.json`
- Modify: `web/src/locales/en/tree.json`
- Modify: `web/src/locales/index.ts`
- Create: `web/src/locales/zh-CN/document.json`
- Modify: `web/src/locales/zh-CN/er.json`
- Modify: `web/src/locales/zh-CN/model.json`
- Modify: `web/src/locales/zh-CN/tree.json`
- Modify: `web/src/shared/api.ts`

**Interfaces:**
- Consumes：任务 6 的接口；M4b 的 `buildGraph`、`layout`、`TableNode`、`renderImage`；M2b 的上传、任务。
- Produces：`useDocExport().open(target: DocTarget)`。

- [ ] **Step 1: 新建 `web/src/features/document/options.test.ts`**

```ts
import { describe, expect, it } from 'vitest'

import {
  DEFAULT_OPTIONS,
  type DocTarget,
  buildRequest,
  fileName,
  fixFormat,
  formatsFor,
  loadOptions,
  optionsKey,
  pixelRatio,
} from './options'

const model: DocTarget = {
  kind: 'model',
  model: 'm1',
  path: 'D:/models/Shop',
  name: '商城',
  description: '订单库',
  schemas: ['public', 'crm'],
}
const db: DocTarget = { kind: 'database', profile: 'p1', schema: 'sales' }

describe('导出文档的选项', () => {
  it('xlsx 只用于数据字典，换成设计文档时改用 docx', () => {
    expect(formatsFor('dictionary')).toContain('xlsx')
    expect(formatsFor('design')).not.toContain('xlsx')
    expect(fixFormat('design', 'xlsx')).toBe('docx')
    expect(fixFormat('design', 'markdown')).toBe('markdown')
  })

  it('按来源记住选项：模型按目录，库按连接和 schema；默认标题取模型名或 schema', () => {
    expect(optionsKey(model)).toBe('ui.doc.m.d--models-shop')
    expect(optionsKey(db)).toBe('ui.doc.d.p1.sales')
    expect(loadOptions(undefined, model)).toMatchObject({ title: '商城', description: '订单库', kind: 'dictionary' })
    expect(loadOptions(undefined, db).title).toBe('sales')
    const saved = JSON.stringify({ kind: 'design', format: 'xlsx', author: '张三', viewDefinitions: 'yes' })
    // 格式随种类修正；类型不对的字段用默认值
    expect(loadOptions(saved, db)).toMatchObject({ kind: 'design', format: 'docx', author: '张三', viewDefinitions: true })
    expect(loadOptions('{bad', db).kind).toBe('dictionary')
  })

  it('组成请求：只选一个 schema 时才带表；模板只给对应的格式；图片只给设计文档', () => {
    const o = { ...DEFAULT_OPTIONS, title: ' 商城 ', version: '', template: 'D:/t.md', reference: 'D:/r.docx' }
    const img = { upload: 'u1', schema: 'public', name: '_all', title: 'public', names_only: false }
    const r = buildRequest(model, { ...o, format: 'markdown' }, ['public'], ['orders'], [img], null)
    expect(r.source).toEqual({ kind: 'model', model: 'm1', schemas: ['public'] })
    expect(r.tables).toEqual(['orders'])
    expect(r.info).toEqual({ title: '商城', version: null, author: null, description: null })
    expect(r.template).toBe('D:/t.md')
    expect(r.reference).toBeNull()
    expect(r.diagrams).toEqual([])
    const r2 = buildRequest(model, { ...o, kind: 'design', format: 'docx' }, ['public', 'crm'], ['orders'], [img], 'D:/out.docx')
    expect(r2.tables).toBeNull()
    expect(r2.reference).toBe('D:/r.docx')
    expect(r2.template).toBeNull()
    expect(r2.diagrams).toEqual([img])
    expect(r2.output).toBe('D:/out.docx')
    expect(buildRequest(db, o, [], [], [], null).source).toEqual({ kind: 'database', profile: 'p1', schema: 'sales' })
  })

  it('文件名和图片的像素倍数', () => {
    expect(fileName('a/b:c', 'markdown')).toBe('a_b_c.md')
    expect(fileName(' ', 'xlsx')).toBe('document.xlsx')
    expect(pixelRatio(4000, 1000)).toBe(2)
    expect(pixelRatio(4001, 1000)).toBe(1)
    expect(pixelRatio(1000, 8001)).toBe(0)
  })
})
```

- [ ] **Step 2: 修改 `web/src/app/Shell.tsx`**

```diff
--- a/web/src/app/Shell.tsx
+++ b/web/src/app/Shell.tsx
@@ -17,6 +17,7 @@ import SqlFileDialog from '../features/import/SqlFileDialog'
 import FavoriteDialog from '../features/favorites/FavoriteDialog'
 import FavoritesPanel from '../features/favorites/FavoritesPanel'
 import HistoryPanel from '../features/history/HistoryPanel'
+import ExportDocDialog from '../features/document/ExportDocDialog'
 import ModelDialogs from '../features/model/ModelDialogs'
 import ModelsPanel from '../features/model/ModelsPanel'
 import OpsPanel from '../features/ops/OpsPanel'
@@ -166,6 +167,7 @@ export default function Shell() {
       <ImportDialog />
       <SqlFileDialog />
       <ModelDialogs />
+      <ExportDocDialog />
     </div>
   )
 }
```

- [ ] **Step 3: 新建 `web/src/features/document/ExportDocDialog.tsx`**

```tsx
// 导出文档对话框（M4c 设计文档 3.2 节）：文档种类、格式、范围、文档信息、模板；开始前检查，
// 设计文档先在屏幕外画 ER 图并上传，再开始后台任务，由右下角的通知跟踪。

import { useQuery } from '@tanstack/react-query'
import { App, Button, Checkbox, Form, Input, Modal, Progress, Radio, Segmented, Select, Space } from 'antd'
import { useEffect, useMemo, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { useApi } from '../../app/context'
import { DEFAULT_DIAGRAM, type DocFormat, type DocumentProblem, errorText } from '../../shared/api'
import { useSettings } from '../../shared/settings'
import { desktop } from '../../shared/shell'
import { withVault } from '../connections/vault'
import { LARGE } from '../er/limits'
import { useDocExport } from './dialog'
import {
  type DocOptions,
  type DocTarget,
  buildRequest,
  extensionOf,
  fileName,
  fixFormat,
  formatsFor,
  loadOptions,
  optionsKey,
} from './options'
import type { DiagramJob } from './render'
import { useDocTracker } from './track'

const SEP = '\u0000'

function Dialog({ target }: { target: DocTarget }) {
  const api = useApi()
  const { t } = useTranslation()
  const { message } = App.useApp()
  const close = useDocExport((s) => s.close)
  const track = useDocTracker()
  const key = optionsKey(target)
  const [o, setO] = useState<DocOptions>(() => {
    const loaded = loadOptions(useSettings.getState().values[key], target)
    // 从 ER 图打开：设计文档
    return target.kind === 'model' && target.diagram ? { ...loaded, kind: 'design', format: fixFormat('design', loaded.format) } : loaded
  })
  const set = (p: Partial<DocOptions>) => setO((cur) => ({ ...cur, ...p }))
  const isModel = target.kind === 'model'
  const [schemas, setSchemas] = useState<string[]>(
    target.kind === 'model' ? (target.schema ? [target.schema] : target.schemas) : [target.schema],
  )
  const single = schemas.length === 1 ? schemas[0] : null
  const [tables, setTables] = useState<string[]>([])
  const [diagrams, setDiagrams] = useState<string[]>(
    target.kind === 'model' && target.diagram && target.diagram !== DEFAULT_DIAGRAM && target.schema
      ? [`${target.schema}${SEP}${target.diagram}`]
      : [],
  )
  const [problems, setProblems] = useState<DocumentProblem[]>([])
  const [busy, setBusy] = useState(false)
  const [drawing, setDrawing] = useState<{ done: number; total: number } | null>(null)
  const stop = useRef(false)

  useEffect(() => setTables([]), [single])

  const tablesQ = useQuery({
    queryKey: ['doc-tables', target.kind === 'model' ? target.model : target.profile, single],
    queryFn: async () =>
      target.kind === 'model'
        ? (await api.modelTables(target.model)).filter((e) => e.schema === single && !e.error).map((e) => e.name)
        : (await withVault(api, () => api.objects(target.profile, target.schema, 'tables'))).map((x) => x.name),
    enabled: !!single,
  })
  const diagramsQ = useQuery({
    queryKey: ['model-diagrams', target.kind === 'model' ? target.model : null, 'doc', schemas.join(SEP)],
    queryFn: async () => {
      if (target.kind !== 'model') return []
      const lists = await Promise.all(schemas.map((s) => api.diagrams(target.model, s)))
      return lists.flat().filter((d) => d.name !== DEFAULT_DIAGRAM)
    },
    enabled: isModel && o.kind === 'design',
  })
  const diagramOptions = useMemo(
    () => (diagramsQ.data ?? []).map((d) => ({ value: `${d.schema}${SEP}${d.name}`, label: `${d.schema} / ${d.name}` })),
    [diagramsQ.data],
  )

  const problem = (field: DocumentProblem['field']) => problems.find((p) => p.field === field)
  const status = (field: DocumentProblem['field']) => {
    const p = problem(field)
    return p ? { validateStatus: 'error' as const, help: errorText(p.error) } : {}
  }

  /** 设计文档要画的图：每个 schema 的默认图，加选中的主题图。 */
  const jobs = async (): Promise<DiagramJob[]> => {
    if (target.kind === 'database') {
      const list = await withVault(api, () => api.schemaTables(target.profile, target.schema))
      const picked = tables.length ? list.filter((x) => tables.includes(x.name)) : list
      const large = picked.length > LARGE
      return [
        {
          schema: target.schema,
          name: DEFAULT_DIAGRAM,
          title: target.schema,
          description: null,
          tables: picked.map((table) => ({ table, logical_keys: [] })),
          display: large ? 'names' : 'columns',
          positions: {},
          namesOnly: large,
        },
      ]
    }
    const out: DiagramJob[] = []
    for (const schema of schemas) {
      const all = await api.diagram(target.model, schema, DEFAULT_DIAGRAM)
      const names = [DEFAULT_DIAGRAM, ...diagrams.filter((d) => d.startsWith(`${schema}${SEP}`)).map((d) => d.split(SEP)[1])]
      for (const name of names) {
        const v = name === DEFAULT_DIAGRAM ? all : await api.diagram(target.model, schema, name)
        const list = v.diagram.tables
        const pickedTables = name === DEFAULT_DIAGRAM && single && tables.length ? tables : null
        const desc = v.diagram.description
        out.push({
          schema,
          name,
          title: name === DEFAULT_DIAGRAM ? schema : name,
          description: typeof desc === 'string' ? desc : null,
          tables: all.tables.filter(
            (x) =>
              !x.error &&
              (list === 'all' || list.includes(x.table.name)) &&
              (!pickedTables || pickedTables.includes(x.table.name)),
          ),
          display: v.diagram.display,
          positions: v.diagram.positions ?? {},
          namesOnly: false,
        })
      }
    }
    return out
  }

  const submit = async () => {
    if (isModel && !schemas.length) return
    setBusy(true)
    setProblems([])
    stop.current = false
    try {
      useSettings.getState().set(key, JSON.stringify(o))
      const draft = buildRequest(target, o, schemas, tables, [], null)
      const found = await api.checkDocument(draft)
      if (found.length) {
        setProblems(found)
        return
      }
      let output: string | null = null
      if (desktop) {
        const ext = extensionOf(o.format)
        output = await desktop.saveDialog(fileName(o.title || draft.info.title, o.format), [
          { name: o.format.toUpperCase(), extensions: [ext] },
        ])
        if (!output) return
      }
      let images: Awaited<ReturnType<typeof import('./render').renderDiagrams>> = []
      if (o.kind === 'design') {
        const list = await jobs()
        setDrawing({ done: 0, total: list.length })
        const { renderDiagrams } = await import('./render')
        images = await renderDiagrams(
          api,
          list,
          (count) => t('er.columns', { count }),
          (done, total) => setDrawing({ done, total }),
          () => stop.current,
        )
        if (!images) return
      }
      const id = await api.startDocument(buildRequest(target, o, schemas, tables, images, output))
      close()
      void track(id, o.title.trim() || draft.info.title || t('document.title'), output)
    } catch (e) {
      message.error((e as Error).message)
    } finally {
      setBusy(false)
      setDrawing(null)
    }
  }

  const pickFile = async (field: 'template' | 'reference') => {
    if (!desktop) return
    const ext = field === 'reference' ? ['docx'] : [extensionOf(o.format)]
    const path = await desktop.openDialog([{ name: ext[0].toUpperCase(), extensions: ext }])
    if (path) set({ [field]: path })
  }

  const saveBuiltin = async () => {
    if (!desktop || (o.format !== 'html' && o.format !== 'markdown')) return
    const ext = extensionOf(o.format)
    const path = await desktop.saveDialog(`dblens-template.${ext}`, [{ name: ext.toUpperCase(), extensions: [ext] }])
    if (!path) return
    try {
      await api.exportDocTemplate(o.format, path)
      set({ template: path })
      message.success(t('document.templateSaved'))
    } catch (e) {
      message.error((e as Error).message)
    }
  }

  const textFormat = o.format === 'html' || o.format === 'markdown'
  const formatLabel: Record<DocFormat, string> = { docx: 'Word (docx)', html: 'HTML', markdown: 'Markdown', xlsx: 'Excel (xlsx)' }

  return (
    <Modal
      open
      width={620}
      title={t('document.title')}
      onCancel={() => {
        stop.current = true
        if (!busy) close()
      }}
      footer={
        drawing ? (
          <div className="row" style={{ gap: 12 }}>
            <span className="grow" style={{ textAlign: 'left' }}>
              {t('document.drawing', { done: drawing.done, total: drawing.total })}
              <Progress percent={Math.round((drawing.done / Math.max(1, drawing.total)) * 100)} size="small" />
            </span>
            <Button onClick={() => (stop.current = true)}>{t('document.cancel')}</Button>
          </div>
        ) : (
          <Space>
            <Button onClick={close}>{t('document.cancel')}</Button>
            <Button type="primary" loading={busy} disabled={isModel && !schemas.length} onClick={() => void submit()}>
              {t('document.export')}
            </Button>
          </Space>
        )
      }
    >
      <Form layout="horizontal" labelCol={{ flex: '96px' }} colon={false} disabled={busy}>
        <Form.Item label={t('document.kind')}>
          <Segmented
            value={o.kind}
            options={[
              { value: 'dictionary', label: t('document.dictionary') },
              { value: 'design', label: t('document.design') },
            ]}
            onChange={(v) => set({ kind: v as DocOptions['kind'], format: fixFormat(v as DocOptions['kind'], o.format) })}
          />
        </Form.Item>
        <Form.Item label={t('document.format')} {...status('format')}>
          <Radio.Group
            optionType="button"
            value={o.format}
            options={formatsFor(o.kind).map((f) => ({ value: f, label: formatLabel[f] }))}
            onChange={(e) => set({ format: e.target.value as DocFormat })}
          />
        </Form.Item>
        {target.kind === 'model' ? (
          <Form.Item label="Schema" required>
            <Select mode="multiple" value={schemas} onChange={setSchemas} options={target.schemas.map((s) => ({ value: s, label: s }))} />
          </Form.Item>
        ) : (
          <Form.Item label="Schema">
            <span>{target.schema}</span>
          </Form.Item>
        )}
        {single && (
          <Form.Item label={t('document.tables')} extra={t('document.tablesHint')}>
            <Select
              mode="multiple"
              allowClear
              showSearch
              value={tables}
              onChange={setTables}
              loading={tablesQ.isLoading}
              placeholder={t('document.allTables')}
              options={(tablesQ.data ?? []).map((n) => ({ value: n, label: n }))}
              maxTagCount="responsive"
            />
          </Form.Item>
        )}
        {o.kind === 'design' && isModel && (
          <Form.Item label={t('document.diagrams')} extra={t('document.diagramsHint')}>
            <Select
              mode="multiple"
              allowClear
              value={diagrams}
              onChange={setDiagrams}
              loading={diagramsQ.isLoading}
              placeholder={t('document.defaultDiagramsOnly')}
              options={diagramOptions}
            />
          </Form.Item>
        )}
        <Form.Item label={t('document.docTitle')}>
          <Input value={o.title} onChange={(e) => set({ title: e.target.value })} />
        </Form.Item>
        <Form.Item label={t('document.version')}>
          <Space.Compact style={{ width: '100%' }}>
            <Input value={o.version} onChange={(e) => set({ version: e.target.value })} />
            <Input value={o.author} placeholder={t('document.author')} onChange={(e) => set({ author: e.target.value })} />
          </Space.Compact>
        </Form.Item>
        <Form.Item label={t('document.description')}>
          <Input.TextArea autoSize={{ minRows: 2, maxRows: 5 }} value={o.description} onChange={(e) => set({ description: e.target.value })} />
        </Form.Item>
        {textFormat && (
          <Form.Item label={t('document.template')} {...status('template')}>
            <Space.Compact style={{ width: '100%' }}>
              <Input value={o.template} placeholder={t('document.builtinTemplate')} allowClear onChange={(e) => set({ template: e.target.value })} />
              {desktop && <Button onClick={() => void pickFile('template')}>{t('document.choose')}</Button>}
              {desktop && <Button onClick={() => void saveBuiltin()}>{t('document.saveBuiltin')}</Button>}
            </Space.Compact>
          </Form.Item>
        )}
        {o.format === 'docx' && (
          <Form.Item label={t('document.reference')} {...status('reference')} extra={t('document.referenceHint')}>
            <Space.Compact style={{ width: '100%' }}>
              <Input value={o.reference} placeholder={t('document.builtinReference')} allowClear onChange={(e) => set({ reference: e.target.value })} />
              {desktop && <Button onClick={() => void pickFile('reference')}>{t('document.choose')}</Button>}
            </Space.Compact>
          </Form.Item>
        )}
        {o.kind === 'design' && target.kind === 'database' && (
          <Form.Item label=" ">
            <Checkbox checked={o.viewDefinitions} onChange={(e) => set({ viewDefinitions: e.target.checked })}>
              {t('document.viewDefinitions')}
            </Checkbox>
          </Form.Item>
        )}
        {problem('output') && (
          <div className="err">{errorText(problem('output')!.error)}</div>
        )}
      </Form>
    </Modal>
  )
}

/** 挂在外壳中：打开时显示。 */
export default function ExportDocDialog() {
  const target = useDocExport((s) => s.target)
  return target ? <Dialog target={target} /> : null
}
```

- [ ] **Step 4: 新建 `web/src/features/document/dialog.ts`**

```ts
// 导出文档对话框的打开状态：模型面板、对象树、ER 图共用。

import { create } from 'zustand'

import type { DocTarget } from './options'

interface DocExportDialog {
  target: DocTarget | null
  open(target: DocTarget): void
  close(): void
}

export const useDocExport = create<DocExportDialog>()((set) => ({
  target: null,
  open: (target) => set({ target }),
  close: () => set({ target: null }),
}))
```

- [ ] **Step 5: 新建 `web/src/features/document/options.ts`**

```ts
// 导出文档对话框的选项（M4c 设计文档 3.2 节）：种类与格式的联动、按来源记住的选项、组成请求、图片的像素倍数。

import type { DocDiagramImage, DocFormat, DocKind, DocSource, DocumentRequest } from '../../shared/api'
import { settingKey } from '../../shared/settings'

/** 从哪里打开的对话框。 */
export type DocTarget =
  | {
      kind: 'model'
      model: string
      path: string
      name: string
      description: string | null
      /** 模型的全部 schema。 */
      schemas: string[]
      /** 默认选中的 schema、ER 图（从 schema 节点或 ER 图打开时）。 */
      schema?: string
      diagram?: string
    }
  | { kind: 'database'; profile: string; schema: string }

/** 记住的选项。 */
export interface DocOptions {
  kind: DocKind
  format: DocFormat
  title: string
  version: string
  author: string
  description: string
  /** html、markdown 的模板文件；空为内置。 */
  template: string
  /** docx 的样式来源；空为内置。 */
  reference: string
  viewDefinitions: boolean
}

export const DEFAULT_OPTIONS: DocOptions = {
  kind: 'dictionary',
  format: 'docx',
  title: '',
  version: '',
  author: '',
  description: '',
  template: '',
  reference: '',
  viewDefinitions: true,
}

/** 某种文档可选的格式：xlsx 只用于数据字典。 */
export function formatsFor(kind: DocKind): DocFormat[] {
  return kind === 'dictionary' ? ['docx', 'html', 'markdown', 'xlsx'] : ['docx', 'html', 'markdown']
}

/** 换文档种类后格式不可用时换成 docx。 */
export function fixFormat(kind: DocKind, format: DocFormat): DocFormat {
  return formatsFor(kind).includes(format) ? format : 'docx'
}

export function extensionOf(format: DocFormat): string {
  return format === 'markdown' ? 'md' : format
}

/** 记住选项用的设置项：模型按目录，库按连接和 schema。 */
export function optionsKey(target: DocTarget): string {
  return settingKey(target.kind === 'model' ? `ui.doc.m.${target.path}` : `ui.doc.d.${target.profile}.${target.schema}`)
}

/** 读记住的选项；没有或格式不对时用默认值，模型的标题、说明默认取模型的名称、说明。 */
export function loadOptions(raw: string | undefined, target: DocTarget): DocOptions {
  const base: DocOptions = {
    ...DEFAULT_OPTIONS,
    title: target.kind === 'model' ? target.name : target.schema,
    description: target.kind === 'model' ? (target.description ?? '') : '',
  }
  if (!raw) return base
  try {
    const saved = JSON.parse(raw) as Partial<DocOptions>
    const o = { ...base }
    for (const k of Object.keys(base) as (keyof DocOptions)[]) {
      if (typeof saved[k] === typeof base[k]) (o as Record<string, unknown>)[k] = saved[k]
    }
    o.format = fixFormat(o.kind, o.format)
    return o
  } catch {
    return base
  }
}

const orNull = (s: string) => (s.trim() ? s.trim() : null)

/** 组成导出请求。`tables` 为空数组时为全部表。 */
export function buildRequest(
  target: DocTarget,
  o: DocOptions,
  schemas: string[],
  tables: string[],
  diagrams: DocDiagramImage[],
  output: string | null,
): DocumentRequest {
  const source: DocSource =
    target.kind === 'model'
      ? { kind: 'model', model: target.model, schemas }
      : { kind: 'database', profile: target.profile, schema: target.schema }
  const single = target.kind === 'database' || schemas.length === 1
  return {
    source,
    tables: single && tables.length ? tables : null,
    kind: o.kind,
    format: o.format,
    info: { title: o.title.trim(), version: orNull(o.version), author: orNull(o.author), description: orNull(o.description) },
    template: o.format === 'html' || o.format === 'markdown' ? orNull(o.template) : null,
    reference: o.format === 'docx' ? orNull(o.reference) : null,
    view_definitions: o.viewDefinitions,
    diagrams: o.kind === 'design' ? diagrams : [],
    output,
  }
}

/** 文件名中不能用的字符换成下划线。 */
export function fileName(title: string, format: DocFormat): string {
  const base = title.replace(/[\\/:*?"<>|]/g, '_').trim() || 'document'
  return `${base}.${extensionOf(format)}`
}

/** 图片任一边的像素上限。 */
export const MAX_PIXELS = 8000

/**
 * 画 ER 图图片的像素倍数：2 倍不超过上限时用 2；1 倍不超过时用 1；仍超过时返回 0（改为只显示表名重画）。
 */
export function pixelRatio(width: number, height: number): 0 | 1 | 2 {
  const side = Math.max(width, height)
  if (side * 2 <= MAX_PIXELS) return 2
  return side <= MAX_PIXELS ? 1 : 0
}
```

- [ ] **Step 6: 新建 `web/src/features/document/render.tsx`**

```tsx
// 设计文档的 ER 图图片（M4c 设计文档 3.3 节）：在屏幕外挂一个不可交互的 React Flow，复用 ER 图的建图、布局和表节点，
// 浅色主题、白底，逐张画成 PNG 并上传。引入 React Flow，由对话框懒加载。

import { ReactFlow, ReactFlowProvider } from '@xyflow/react'
import '@xyflow/react/dist/style.css'
import { createRoot } from 'react-dom/client'

import type { Api, DocDiagramImage, ErDisplay, ErPosition, ErTable } from '../../shared/api'
import { renderImage } from '../er/export'
import { NODE_TYPES, flowEdges, flowNodes } from '../er/flow'
import { buildGraph, layout } from '../er/graph'
import { pixelRatio } from './options'

/** 一张要画的图。 */
export interface DiagramJob {
  schema: string
  name: string
  title: string
  description: string | null
  tables: ErTable[]
  display: ErDisplay
  positions: Record<string, ErPosition>
  /** 表较多、已改为只显示表名（库图按 ER 图的规则）：文档中注明。 */
  namesOnly: boolean
}

const PADDING = 24
const frame = () => new Promise((r) => requestAnimationFrame(() => r(null)))

/** 画一张图：返回 PNG 的 data URL，以及是否改成了只显示表名。 */
async function draw(job: DiagramJob, columns: (n: number) => string): Promise<{ url: string; namesOnly: boolean }> {
  const host = document.createElement('div')
  host.className = 'er-canvas'
  host.dataset.theme = 'light'
  host.style.cssText = 'position:fixed;left:-30000px;top:0;width:1600px;height:1200px;pointer-events:none'
  document.body.appendChild(host)
  const root = createRoot(host)
  try {
    for (const display of job.display === 'names' ? (['names'] as const) : [job.display, 'names' as const]) {
      const graph = buildGraph(job.tables, display, job.schema)
      const positions = graph.nodes.every((n) => job.positions[n.id]) ? job.positions : layout(graph, job.positions)
      root.render(
        <ReactFlowProvider>
          <ReactFlow
            nodes={flowNodes(graph, positions)}
            edges={flowEdges(graph, columns)}
            nodeTypes={NODE_TYPES}
            colorMode="light"
            nodesDraggable={false}
            nodesConnectable={false}
            proOptions={{ hideAttribution: true }}
          />
        </ReactFlowProvider>,
      )
      // 等节点画出来、连线算好
      for (let i = 0; i < 3; i++) await frame()
      await new Promise((r) => setTimeout(r, 100))
      const viewport = host.querySelector<HTMLElement>('.react-flow__viewport')
      if (!viewport) throw new Error('viewport')
      const xs = graph.nodes.map((n) => positions[n.id] ?? { x: 0, y: 0 })
      const minX = Math.min(0, ...xs.map((p) => p.x))
      const minY = Math.min(0, ...xs.map((p) => p.y))
      const maxX = Math.max(1, ...graph.nodes.map((n, i) => xs[i].x + n.width))
      const maxY = Math.max(1, ...graph.nodes.map((n, i) => xs[i].y + n.height))
      const bounds = { x: minX, y: minY, width: maxX - minX, height: maxY - minY }
      const ratio = pixelRatio(bounds.width + PADDING * 2, bounds.height + PADDING * 2)
      if (ratio === 0 && display !== 'names') continue
      const url = await renderImage(viewport, bounds, 'png', '#ffffff', ratio || 1)
      return { url, namesOnly: display === 'names' && job.display !== 'names' }
    }
    throw new Error('unreachable')
  } finally {
    root.unmount()
    host.remove()
  }
}

/** 逐张画图并上传；`onProgress(已完成, 总数)`；`cancelled()` 为真时停下。 */
export async function renderDiagrams(
  api: Api,
  jobs: DiagramJob[],
  columns: (n: number) => string,
  onProgress: (done: number, total: number) => void,
  cancelled: () => boolean,
): Promise<DocDiagramImage[] | null> {
  const out: DocDiagramImage[] = []
  for (const [i, job] of jobs.entries()) {
    if (cancelled()) return null
    onProgress(i, jobs.length)
    const { url, namesOnly } = await draw(job, columns)
    const blob = await (await fetch(url)).blob()
    const file = new File([blob], `${job.schema}-${job.name}.png`, { type: 'image/png' })
    const up = await api.upload(file)
    out.push({
      upload: up.id,
      schema: job.schema,
      name: job.name,
      title: job.title,
      description: job.description,
      names_only: namesOnly || job.namesOnly,
    })
  }
  onProgress(jobs.length, jobs.length)
  return out
}
```

- [ ] **Step 7: 新建 `web/src/features/document/track.tsx`**

```tsx
// 文档导出任务的进度与结果：右下角的通知轮询任务状态，可取消；完成后桌面版可在文件夹中显示，浏览器版下载。

import { App, Button } from 'antd'
import { useTranslation } from 'react-i18next'

import { useApi } from '../../app/context'
import { errorText } from '../../shared/api'
import { desktop } from '../../shared/shell'

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms))

export function useDocTracker() {
  const api = useApi()
  const { t } = useTranslation()
  const { notification } = App.useApp()

  /** 跟踪任务直到结束。`output` 是桌面版写到的路径。 */
  return async (id: string, title: string, output: string | null) => {
    const key = `doc-${id}`
    await sleep(300)
    for (;;) {
      const st = await api.task(id).catch(() => null)
      if (!st) return
      if (st.state === 'running') {
        const { read, total } = st.progress
        notification.open({
          key,
          message: title,
          description: st.phase === 'scanning' ? t('document.task.reading', { read, total }) : t('document.task.writing'),
          duration: 0,
          actions: (
            <Button size="small" onClick={() => void api.cancelTask(id)}>
              {t('document.task.cancel')}
            </Button>
          ),
        })
        await sleep(800)
        continue
      }
      if (st.state === 'done') {
        const warn = st.warnings ? t('document.task.truncated', { count: st.warnings }) : null
        if (!output && st.report) {
          const a = document.createElement('a')
          a.href = api.taskReportUrl(st.id)
          a.download = st.report
          a.click()
        }
        notification.success({
          key,
          message: t('document.task.done', { title }),
          description: [output, warn].filter(Boolean).map((l) => <div key={l}>{l}</div>),
          actions:
            output && desktop ? (
              <Button size="small" onClick={() => void desktop!.revealInFolder(output)}>
                {t('document.task.reveal')}
              </Button>
            ) : undefined,
          duration: 0,
        })
      } else if (st.state === 'failed') {
        notification.error({
          key,
          message: t('document.task.failed', { title }),
          description: st.error ? errorText(st.error) : null,
          duration: 0,
        })
      } else {
        notification.info({ key, message: t('document.task.cancelled', { title }), duration: 6 })
      }
      return
    }
  }
}
```

- [ ] **Step 8: 修改 `web/src/features/er/ErView.tsx`**

```diff
--- a/web/src/features/er/ErView.tsx
+++ b/web/src/features/er/ErView.tsx
@@ -18,7 +18,6 @@ import {
   type Connection,
   Controls,
   type Edge,
-  MarkerType,
   MiniMap,
   ReactFlow,
   ReactFlowProvider,
@@ -38,15 +37,15 @@ import { desktop } from '../../shared/shell'
 import { THEME_KEY, parseThemePref, resolveTheme, useSystemDark } from '../../shared/theme'
 import { withVault } from '../connections/vault'
 import { type QueryTab, useTabs } from '../editor/tabs'
+import { useDocExport } from '../document/dialog'
 import { useModelDialogs } from '../model/dialogs'
 import { type ErTabState, useErTabs } from './events'
 import { type ImageFormat, dataUrlBase64, imageName, renderImage } from './export'
+import { NODE_TYPES, flowEdges, flowNodes } from './flow'
 import { type Direction, TABLE_HANDLE, buildGraph, layout, searchTables } from './graph'
-import TableNode, { type TableNodeType } from './TableNode'
+import { LARGE } from './limits'
+import type { TableNodeType } from './TableNode'
 
-const NODE_TYPES = { table: TableNode }
-/** 默认图超过这么多张表时先只显示表名。 */
-const LARGE = 300
 
 interface Loaded {
   /** 可画的表：模型为 schema 的全部表（按图中的表过滤后画出），库为整个 schema。 */
@@ -160,33 +159,11 @@ function Canvas({ tab }: { tab: QueryTab }) {
   const [nodes, setNodes, onNodesChange] = useNodesState<TableNodeType>([])
   useEffect(() => {
     setNodes((prev) =>
-      graph.nodes.map((n) => ({
-        id: n.id,
-        type: 'table' as const,
-        position: positions[n.id] ?? { x: 0, y: 0 },
-        data: n.data,
-        selected: prev.find((p) => p.id === n.id)?.selected ?? false,
-        style: { width: n.width },
-      })),
+      flowNodes(graph, positions).map((n) => ({ ...n, selected: prev.find((p) => p.id === n.id)?.selected ?? false })),
     )
   }, [graph, positions, setNodes])
 
-  const edges: Edge[] = useMemo(
-    () =>
-      graph.edges.map((e) => ({
-        id: e.id,
-        source: e.source,
-        target: e.target,
-        sourceHandle: e.sourceHandle,
-        targetHandle: e.targetHandle,
-        type: 'smoothstep',
-        label: e.count > 1 ? t('er.columns', { count: e.count }) : undefined,
-        className: e.logical ? 'er-edge logical' : 'er-edge',
-        markerEnd: { type: MarkerType.ArrowClosed },
-        data: { title: e.title, logicalIndex: e.logicalIndex },
-      })),
-    [graph, t],
-  )
+  const edges: Edge[] = useMemo(() => flowEdges(graph, (count) => t('er.columns', { count })), [graph, t])
 
   const markDirty = () => isModel && patch({ dirty: true })
 
@@ -292,6 +269,26 @@ function Canvas({ tab }: { tab: QueryTab }) {
     }
   }
 
+  /** 导出设计文档：范围默认为当前这张图。 */
+  const openDoc = () => {
+    if (target.kind === 'database') {
+      useDocExport.getState().open({ kind: 'database', profile: target.profile, schema })
+      return
+    }
+    const m = modelQ.data
+    if (!m) return
+    useDocExport.getState().open({
+      kind: 'model',
+      model: m.id,
+      path: m.path,
+      name: m.info.name,
+      description: m.info.description ?? null,
+      schemas: m.info.schemas.includes(schema) ? m.info.schemas : [...m.info.schemas, schema],
+      schema,
+      diagram: diagramName,
+    })
+  }
+
   const [search, setSearch] = useState('')
   const locate = (name: string) => {
     const n = flow.getNode(name)
@@ -391,8 +388,14 @@ function Canvas({ tab }: { tab: QueryTab }) {
               { key: 'png', label: t('er.export.png') },
               { key: 'svg:white', label: t('er.export.svgWhite') },
               { key: 'png:white', label: t('er.export.pngWhite') },
+              { type: 'divider' },
+              { key: 'doc', label: t('er.export.designDoc') },
             ],
             onClick: ({ key }) => {
+              if (key === 'doc') {
+                openDoc()
+                return
+              }
               const [f, w] = key.split(':')
               void exportImage(f as ImageFormat, w === 'white')
             },
```

- [ ] **Step 9: 修改 `web/src/features/er/export.ts`**

```diff
--- a/web/src/features/er/export.ts
+++ b/web/src/features/er/export.ts
@@ -14,15 +14,21 @@ export interface Bounds {
 
 const PADDING = 24
 
-/** 把视口层按整张图的范围画成 data URL。 */
-export async function renderImage(viewport: HTMLElement, bounds: Bounds, format: ImageFormat, background: string): Promise<string> {
+/** 把视口层按整张图的范围画成 data URL。PNG 默认 2 倍像素。 */
+export async function renderImage(
+  viewport: HTMLElement,
+  bounds: Bounds,
+  format: ImageFormat,
+  background: string,
+  ratio = 2,
+): Promise<string> {
   const width = Math.ceil(bounds.width + PADDING * 2)
   const height = Math.ceil(bounds.height + PADDING * 2)
   const options = {
     backgroundColor: background,
     width,
     height,
-    pixelRatio: format === 'png' ? 2 : 1,
+    pixelRatio: format === 'png' ? ratio : 1,
     // 不内嵌网页字体（用系统字体），SVG 小得多
     skipFonts: true,
     // 连线手柄和连线的点击区域是操作用的，不画进图片（表多时也快得多）
```

- [ ] **Step 10: 新建 `web/src/features/er/flow.ts`**

```ts
// 图（graph.ts）转成 React Flow 的节点和连线：ER 图标签和导出文档时在屏幕外画图共用。

import { type Edge, MarkerType } from '@xyflow/react'

import type { ErPosition } from '../../shared/api'
import type { Graph } from './graph'
import TableNode, { type TableNodeType } from './TableNode'

export const NODE_TYPES = { table: TableNode }

export function flowNodes(graph: Graph, positions: Record<string, ErPosition>): TableNodeType[] {
  return graph.nodes.map((n) => ({
    id: n.id,
    type: 'table' as const,
    position: positions[n.id] ?? { x: 0, y: 0 },
    data: n.data,
    style: { width: n.width },
  }))
}

/** `columns(n)` 给出复合外键连线上的"N 列"。 */
export function flowEdges(graph: Graph, columns: (count: number) => string): Edge[] {
  return graph.edges.map((e) => ({
    id: e.id,
    source: e.source,
    target: e.target,
    sourceHandle: e.sourceHandle,
    targetHandle: e.targetHandle,
    type: 'smoothstep',
    label: e.count > 1 ? columns(e.count) : undefined,
    className: e.logical ? 'er-edge logical' : 'er-edge',
    markerEnd: { type: MarkerType.ArrowClosed },
    data: { title: e.title, logicalIndex: e.logicalIndex },
  }))
}
```

- [ ] **Step 11: 新建 `web/src/features/er/limits.ts`**

```ts
// ER 图的阈值（不引入 React Flow，导出文档的对话框也用）。

/** 库图、默认图超过这么多张表时先只显示表名。 */
export const LARGE = 300
```

- [ ] **Step 12: 修改 `web/src/features/model/ModelsPanel.tsx`**

```diff
--- a/web/src/features/model/ModelsPanel.tsx
+++ b/web/src/features/model/ModelsPanel.tsx
@@ -13,6 +13,7 @@ import { useApi } from '../../app/context'
 import { ApiError, DEFAULT_DIAGRAM, type ModelSummary, type ModelTableEntry } from '../../shared/api'
 import { desktop } from '../../shared/shell'
 import { useTabs } from '../editor/tabs'
+import { useDocExport } from '../document/dialog'
 import { useModelDialogs } from './dialogs'
 
 type NodeRef =
@@ -113,10 +114,12 @@ function ModelTree({ model, onMenu }: { model: ModelSummary; onMenu(n: NodeRef,
         ...common,
         { key: 'er-open', label: t('er.menu.openDefault') },
         { key: 'er-new', label: t('er.menu.newDiagram'), disabled: ro },
+        { key: 'export-doc', label: t('model.menu.exportDoc') },
       ]
     }
     return [
       ...common,
+      { key: 'export-doc', label: t('model.menu.exportDoc') },
       { type: 'divider' },
       { key: 'reload', label: t('model.menu.reload') },
       ...(desktop ? [{ key: 'reveal', label: t('model.menu.reveal') }] : []),
@@ -224,6 +227,17 @@ export default function ModelsPanel() {
       else if (key === 'compare') useTabs.getState().openCompare({ source: { kind: 'model', path: m.path, schema }, target: null })
       else if (key === 'reload') await reload(m)
       else if (key === 'reveal') await desktop?.revealInFolder(`${m.path}/model.yaml`)
+      else if (key === 'export-doc') {
+        useDocExport.getState().open({
+          kind: 'model',
+          model: m.id,
+          path: m.path,
+          name: m.info.name,
+          description: m.info.description ?? null,
+          schemas: m.info.schemas,
+          schema: n.type === 'schema' ? n.schema : undefined,
+        })
+      }
       else if (key === 'close') {
         await api.closeModel(m.id)
         await qc.invalidateQueries({ queryKey: ['models'] })
```

- [ ] **Step 13: 修改 `web/src/features/tree/ObjectTree.tsx`**

```diff
--- a/web/src/features/tree/ObjectTree.tsx
+++ b/web/src/features/tree/ObjectTree.tsx
@@ -28,6 +28,7 @@ import { useQueryActions } from '../editor/actions'
 import { useTabs } from '../editor/tabs'
 import { useExportDialog } from '../export/dialog'
 import { useImportDialogs } from '../import/dialog'
+import { useDocExport } from '../document/dialog'
 import { useModelDialogs } from '../model/dialogs'
 import type { Api, Environment, Profile, RoutineInfo } from '../../shared/api'
 import DbIcon from '../../shared/DbIcon'
@@ -304,6 +305,7 @@ export default function ObjectTree() {
       { key: 'er', label: t('tree.menu.er') },
       { key: 'compare-model', label: t('tree.menu.compareModel') },
       { key: 'save-model', label: t('tree.menu.saveAsModel') },
+      { key: 'export-doc', label: t('tree.menu.exportDoc') },
       { type: 'divider' },
     ]
   }
@@ -355,6 +357,7 @@ export default function ObjectTree() {
       } else if (key === 'compare-model' && k.type === 's') {
         useTabs.getState().openCompare({ source: null, target: { kind: 'database', profile: k.profileId, schema: k.schema } })
       } else if (key === 'save-model' && k.type === 's') useModelDialogs.getState().openReverse({ profileId: k.profileId, schema: k.schema })
+      else if (key === 'export-doc' && k.type === 's') useDocExport.getState().open({ kind: 'database', profile: k.profileId, schema: k.schema })
       else if (key === 'import' && k.type === 'o') {
         useImportDialogs.getState().openImport({ profileId: k.profileId, schema: k.schema, table: k.name, create: false })
       } else if (key === 'import-new' && k.type === 's') {
```

- [ ] **Step 14: 新建 `web/src/locales/en/document.json`**

```json
{
  "title": "Export Document",
  "kind": "Document",
  "dictionary": "Data Dictionary",
  "design": "Design Document",
  "format": "Format",
  "tables": "Tables",
  "tablesHint": "Leave empty for all tables",
  "allTables": "All tables",
  "diagrams": "ER diagrams",
  "diagramsHint": "The default diagram (all tables) of each schema is always included; pick topic diagrams here",
  "defaultDiagramsOnly": "Default diagrams only",
  "docTitle": "Title",
  "version": "Version",
  "author": "Author",
  "description": "Description",
  "template": "Template",
  "builtinTemplate": "Built-in template",
  "choose": "Choose…",
  "saveBuiltin": "Save Built-in Template…",
  "templateSaved": "Built-in template saved; edit it and export again",
  "reference": "Style reference",
  "builtinReference": "Built-in styles",
  "referenceHint": "Styles, headers and footers, paper size and margins are taken from this .docx",
  "viewDefinitions": "Include view definitions",
  "export": "Export",
  "cancel": "Cancel",
  "drawing": "Drawing ER diagrams {{done}}/{{total}}",
  "task": {
    "reading": "Reading structure {{read}}/{{total}}",
    "writing": "Writing document",
    "cancel": "Cancel",
    "done": "{{title}} exported",
    "failed": "Failed to export {{title}}",
    "cancelled": "{{title}} cancelled",
    "reveal": "Show in Folder",
    "truncated_one": "{{count}} cell exceeded 32767 characters and was truncated",
    "truncated_other": "{{count}} cells exceeded 32767 characters and were truncated"
  }
}
```

- [ ] **Step 15: 修改 `web/src/locales/en/er.json`**

```diff
--- a/web/src/locales/en/er.json
+++ b/web/src/locales/en/er.json
@@ -29,7 +29,8 @@
     "svg": "Export SVG",
     "png": "Export PNG",
     "svgWhite": "Export SVG (white background)",
-    "pngWhite": "Export PNG (white background)"
+    "pngWhite": "Export PNG (white background)",
+    "designDoc": "Export Design Document…"
   },
   "menu": {
     "open": "Open",
```

- [ ] **Step 16: 修改 `web/src/locales/en/model.json`**

```diff
--- a/web/src/locales/en/model.json
+++ b/web/src/locales/en/model.json
@@ -17,7 +17,8 @@
     "newTable": "New Table",
     "reload": "Reload",
     "reveal": "Show in File Manager",
-    "close": "Close Model"
+    "close": "Close Model",
+    "exportDoc": "Export Document…"
   },
   "rename": {
     "title": "Rename table {{name}} to"
```

- [ ] **Step 17: 修改 `web/src/locales/en/tree.json`**

```diff
--- a/web/src/locales/en/tree.json
+++ b/web/src/locales/en/tree.json
@@ -44,7 +44,8 @@
     "importNew": "Import into New Table…",
     "sqlFile": "Run SQL File…",
     "exportData": "Export Data…",
-    "newTable": "New table"
+    "newTable": "New table",
+    "exportDoc": "Export Document…"
   },
   "duplicate": {
     "name": "{{name}} copy",
```

- [ ] **Step 18: 修改 `web/src/locales/index.ts`**

```diff
--- a/web/src/locales/index.ts
+++ b/web/src/locales/index.ts
@@ -5,6 +5,7 @@ import enApp from './en/app.json'
 import enCommon from './en/common.json'
 import enConnections from './en/connections.json'
 import enDesign from './en/design.json'
+import enDocument from './en/document.json'
 import enEditor from './en/editor.json'
 import enImport from './en/import.json'
 import enModel from './en/model.json'
@@ -15,6 +16,7 @@ import zhApp from './zh-CN/app.json'
 import zhCommon from './zh-CN/common.json'
 import zhConnections from './zh-CN/connections.json'
 import zhDesign from './zh-CN/design.json'
+import zhDocument from './zh-CN/document.json'
 import zhEditor from './zh-CN/editor.json'
 import zhImport from './zh-CN/import.json'
 import zhModel from './zh-CN/model.json'
@@ -33,6 +35,7 @@ export const zhCN = {
   design: zhDesign,
   model: zhModel,
   er: zhEr,
+  document: zhDocument,
 }
 
 export const en = {
@@ -46,4 +49,5 @@ export const en = {
   design: enDesign,
   model: enModel,
   er: enEr,
+  document: enDocument,
 }
```

- [ ] **Step 19: 新建 `web/src/locales/zh-CN/document.json`**

```json
{
  "title": "导出文档",
  "kind": "文档种类",
  "dictionary": "数据字典",
  "design": "设计文档",
  "format": "格式",
  "tables": "表",
  "tablesHint": "不选为全部表",
  "allTables": "全部表",
  "diagrams": "ER 图",
  "diagramsHint": "每个 schema 的默认图（全部表）总会放进文档；这里另选主题图",
  "defaultDiagramsOnly": "只有默认图",
  "docTitle": "标题",
  "version": "版本",
  "author": "作者",
  "description": "说明",
  "template": "模板",
  "builtinTemplate": "内置模板",
  "choose": "选择…",
  "saveBuiltin": "另存内置模板…",
  "templateSaved": "已另存内置模板，可修改后再导出",
  "reference": "样式来源",
  "builtinReference": "内置样式",
  "referenceHint": "取这个 .docx 的样式、页眉页脚、纸张和页边距",
  "viewDefinitions": "放进视图的定义",
  "export": "导出",
  "cancel": "取消",
  "drawing": "正在画 ER 图 {{done}}/{{total}}",
  "task": {
    "reading": "读取结构 {{read}}/{{total}}",
    "writing": "生成文档",
    "cancel": "取消",
    "done": "{{title}} 已导出",
    "failed": "{{title}} 导出失败",
    "cancelled": "{{title}} 已取消",
    "reveal": "在文件夹中显示",
    "truncated": "{{count}} 个单元格超过 32767 个字符，已截断"
  }
}
```

- [ ] **Step 20: 修改 `web/src/locales/zh-CN/er.json`**

```diff
--- a/web/src/locales/zh-CN/er.json
+++ b/web/src/locales/zh-CN/er.json
@@ -28,7 +28,8 @@
     "svg": "导出 SVG",
     "png": "导出 PNG",
     "svgWhite": "导出 SVG（白底）",
-    "pngWhite": "导出 PNG（白底）"
+    "pngWhite": "导出 PNG（白底）",
+    "designDoc": "导出设计文档…"
   },
   "menu": {
     "open": "打开",
```

- [ ] **Step 21: 修改 `web/src/locales/zh-CN/model.json`**

```diff
--- a/web/src/locales/zh-CN/model.json
+++ b/web/src/locales/zh-CN/model.json
@@ -17,7 +17,8 @@
     "newTable": "新建表",
     "reload": "重新加载",
     "reveal": "在文件管理器中显示",
-    "close": "关闭模型"
+    "close": "关闭模型",
+    "exportDoc": "导出文档…"
   },
   "rename": {
     "title": "表 {{name}} 改名为"
```

- [ ] **Step 22: 修改 `web/src/locales/zh-CN/tree.json`**

```diff
--- a/web/src/locales/zh-CN/tree.json
+++ b/web/src/locales/zh-CN/tree.json
@@ -44,7 +44,8 @@
     "importNew": "导入到新表…",
     "sqlFile": "执行 SQL 文件…",
     "exportData": "导出数据…",
-    "newTable": "新建表"
+    "newTable": "新建表",
+    "exportDoc": "导出文档…"
   },
   "duplicate": {
     "name": "{{name}} 副本",
```

- [ ] **Step 23: 修改 `web/src/shared/api.ts`**

```diff
--- a/web/src/shared/api.ts
+++ b/web/src/shared/api.ts
@@ -425,7 +425,7 @@ export interface SqlFileRequest {
 
 export interface TaskStatus {
   id: string
-  kind: 'import' | 'sql_file' | 'design' | 'reverse' | 'sync'
+  kind: 'import' | 'sql_file' | 'design' | 'reverse' | 'sync' | 'document'
   state: 'running' | 'confirm' | 'done' | 'failed' | 'cancelled'
   phase: 'preparing' | 'scanning' | 'creating' | 'clearing' | 'writing' | 'executing' | 'finished'
   progress: {
@@ -779,6 +779,48 @@ export interface DiagramView {
 /** 默认图的名字。 */
 export const DEFAULT_DIAGRAM = '_all'
 
+/** 文档导出（M4c）：数据字典、设计文档。 */
+export type DocKind = 'dictionary' | 'design'
+export type DocFormat = 'docx' | 'html' | 'markdown' | 'xlsx'
+export type DocSource =
+  | { kind: 'model'; model: string; schemas: string[] }
+  | { kind: 'database'; profile: string; schema: string }
+
+export interface DocInfo {
+  title: string
+  version?: string | null
+  author?: string | null
+  description?: string | null
+}
+
+/** 前端画好、已上传的一张 ER 图。 */
+export interface DocDiagramImage {
+  upload: string
+  schema: string
+  name: string
+  title: string
+  description?: string | null
+  names_only: boolean
+}
+
+export interface DocumentRequest {
+  source: DocSource
+  tables?: string[] | null
+  kind: DocKind
+  format: DocFormat
+  info: DocInfo
+  template?: string | null
+  reference?: string | null
+  view_definitions: boolean
+  diagrams: DocDiagramImage[]
+  output?: string | null
+}
+
+export interface DocumentProblem {
+  field: 'template' | 'reference' | 'output' | 'format'
+  error: ErrorBody
+}
+
 export type FetchFn = (input: string, init?: RequestInit) => Promise<Response>
 
 export function createApi(token: string, fetchFn: FetchFn = (i, o) => fetch(i, o)) {
@@ -973,6 +1015,13 @@ export function createApi(token: string, fetchFn: FetchFn = (i, o) => fetch(i, o
       request<{ fingerprint: string }>('DELETE', `${mt(id, schema, table)}/logical-keys/${index}`),
     /** 桌面版：把导出的图片（base64）写到"另存为"选的 .svg、.png 文件。 */
     saveImage: (path: string, data: string) => request<null>('POST', '/files/image', { path, data }),
+    startDocument: async (req: DocumentRequest) => (await request<{ id: string }>('POST', '/documents', req)).id,
+    /** 开始前检查：模板语法、样式来源、输出位置。 */
+    checkDocument: async (req: DocumentRequest) =>
+      (await request<{ problems: DocumentProblem[] }>('POST', '/documents/check', req)).problems,
+    /** 把内置模板写到指定路径（桌面版）。 */
+    exportDocTemplate: (format: 'html' | 'markdown', path: string) =>
+      request<null>('POST', '/documents/templates/export', { format, path }),
     /** 整个 schema 的表明细（库的 ER 图）。 */
     schemaTables: (id: string, schema: string) => request<DesignTableModel[]>('GET', `${s(id, schema)}/tables`),
     compare: (req: CompareRequest) => request<CompareResult>('POST', '/compare', req),
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

Expected: 全部通过（前端新增 4 个测试）。

- [ ] **Step 25: 浏览器实测**

浏览器实测（设计文档第 8 节）：用 `dblens-server --listen 127.0.0.1:<端口>` 启动浏览器模式（下载由页面中的链接触发）。
1. 模型面板模型右键"导出文档…"：切到设计文档时 xlsx 消失、出现"ER 图"；选 HTML 导出，打开下载的文件核对：概述统计、ER 图（浅色白底、列级连线、复合外键"2 列"、逻辑外键虚线）、表间关系、表明细中的链接。
2. 同一模型导出 docx 数据字典、markdown 设计文档（下载 zip，含 `<标题>.md` 和 `<标题>_images/`）。
3. 对象树 schema 右键"导出文档…"，导出 xlsx 数据字典，用 Excel 打开核对三张工作表和表名链接。
4. 模板一栏填一个语法错误的模板路径：导出时该项显示错误和行号，不开始任务。
5. ER 图工具栏"导出 → 导出设计文档…"：对话框中种类为设计文档，主题图已选中当前的图。
6. 再次打开对话框，上次的选项（种类、格式、作者等）还在。

- [ ] **Step 26: 提交**

```bash
git add web/src/app/Shell.tsx web/src/features/document/ExportDocDialog.tsx web/src/features/document/dialog.ts web/src/features/document/options.test.ts web/src/features/document/options.ts web/src/features/document/render.tsx web/src/features/document/track.tsx web/src/features/er/ErView.tsx web/src/features/er/export.ts web/src/features/er/flow.ts web/src/features/er/limits.ts web/src/features/model/ModelsPanel.tsx web/src/features/tree/ObjectTree.tsx web/src/locales/en/document.json web/src/locales/en/er.json web/src/locales/en/model.json web/src/locales/en/tree.json web/src/locales/index.ts web/src/locales/zh-CN/document.json web/src/locales/zh-CN/er.json web/src/locales/zh-CN/model.json web/src/locales/zh-CN/tree.json web/src/shared/api.ts
git commit -m "feat(web): 导出文档对话框、ER 图图片、任务进度" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 8: 文档

- `docs/notes/m4c-document-export.md`（新）：结构、与设计文档不一致的地方、验证。
- `docs/notes/m0-4b-api.md`：文档导出的接口。
- 总设计文档：M4c 完成，M4 全部完成。

**Files:**
- Modify: `docs/notes/m0-4b-api.md`
- Create: `docs/notes/m4c-document-export.md`
- Modify: `docs/superpowers/specs/2026-10-22-dblens-design.md`

- [ ] **Step 1: 修改 `docs/notes/m0-4b-api.md`**

```diff
--- a/docs/notes/m0-4b-api.md
+++ b/docs/notes/m0-4b-api.md
@@ -265,6 +265,22 @@
 - `warnings`：`[{kind: "missing_table", name} | {kind: "broken_logical_key", table, index, name}]`。
 - 库的 ER 图用 `GET /profiles/{id}/schemas/{schema}/tables`（不给 `table` 时返回整个 schema 的表明细）。
 
+### 文档导出（M4c 加入）
+
+| 方法 | 路径 | 说明 |
+|---|---|---|
+| POST | `/documents` | 开始导出，201 和 `{id}`（任务 id，进度和取消用 `/tasks/{id}`）；浏览器模式完成后从 `/tasks/{id}/report` 下载 |
+| POST | `/documents/check` | 开始前检查，返回 `{problems: [{field, error}]}`，`field` 为 `format` / `template` / `reference` / `output` |
+| POST | `/documents/templates/export` | `{format: "html" \| "markdown", path}`，把内置模板写到本机路径（只在页面和服务在同一台机器上时可用，否则 403） |
+
+- 请求：`{source, tables?, kind, format, info, template?, reference?, view_definitions, diagrams, output?}`。
+  - `source`：`{kind: "model", model, schemas}` 或 `{kind: "database", profile, schema}`；`tables` 只在一个 schema 时生效。
+  - `kind`：`dictionary` / `design`；`format`：`docx` / `html` / `markdown` / `xlsx`（xlsx 只用于数据字典）。
+  - `info`：`{title, version?, author?, description?}`；标题为空时取模型名或 schema 名。
+  - `template`（html、markdown 的模板文件）、`reference`（docx 的样式来源）、`output`（写到的路径）是本机路径，只在页面和服务在同一台机器上时接受。
+  - `diagrams`：设计文档的 ER 图 `[{upload, schema, name, title, description?, names_only}]`，`upload` 为 `/uploads` 返回的编号（PNG）。
+- 任务 `kind` 为 `document`；xlsx 有截断的单元格时 `warnings` 为个数。markdown 带图片时：写到本机路径则图片在旁边的 `<文件名>_images/`，浏览器模式打包成 zip。
+
 ## WebSocket：`GET /api/sessions/{id}/ws?token=…&lang=…`
 
 `lang` 可选，决定事件中文字的语言（见"语言"一节）。
```

- [ ] **Step 2: 新建 `docs/notes/m4c-document-export.md`**

````markdown
# M4c 文档导出

依据：设计文档 `docs/superpowers/specs/2026-10-28-m4c-document-export-design.md`。

## 结构

```text
crates/dblens-service/src/document/
  content.rs   文档内容 DocContent：从模型或库的结构（SourceSchema）组织表清单、明细、被引用、表间关系、统计、视图；PNG 尺寸
  labels.rs    文档中的固定文字（labels.json，中英文）：模板用 t('键')，docx、xlsx 用 label
  template.rs  html、markdown 模板（minijinja）：内置模板、过滤器 v / cell、检查语法、渲染
  templates/   内置模板 document.html、document.md（两种文档共用，按 kind 分支）
  docx.rs      docx：样式来源的读取和校验、按 Word 内部名找样式（缺的补上）、正文、目录字段、书签和链接、图片、打包
  docx/        内置样式来源的各个部件（运行时打包成 .docx）
  xlsx.rs      数据字典的 xlsx：表清单、列、索引和约束三张工作表
  job.rs       DocumentRequest、开始前检查、后台任务（TaskKind::Document）、写到本机路径或任务目录、markdown 图片目录或 zip、另存内置模板
crates/dblens-api/src/documents.rs   POST /documents、/documents/check、/documents/templates/export
web/src/features/document/
  options.ts          种类与格式的联动、按来源记住的选项、组成请求、文件名、图片的像素倍数
  ExportDocDialog.tsx 导出文档对话框
  render.tsx          屏幕外画 ER 图图片并上传（懒加载）
  track.tsx           任务进度通知；完成后下载（浏览器版）或在文件夹中显示（桌面版）
  dialog.ts           对话框的打开状态
web/src/features/er/flow.ts    图转成 React Flow 的节点和连线（ER 图标签和导出文档共用）
web/src/features/er/limits.ts  库图只显示表名的阈值
```

入口：模型面板中模型、schema 右键"导出文档…"；对象树 schema 右键"导出文档…"；ER 图工具栏"导出"菜单的"导出设计文档…"（范围为当前的图）。

## 与设计文档不一致的地方

- 内置模板 html、markdown 各一份（两种文档共用，按 `kind` 分支），不是四份；"另存内置模板"存出的一份对两种文档都适用。
- 模板的固定文字放在 `document/labels.json`（键 `doc.*`），不放进后端词条：词条检查要求每个键都被 `msg!` 用到，模板中的键由模板引用；
  测试检查内置模板和 docx、xlsx 代码中用到的键都存在、`labels.json` 中没有用不到的键。
- 生成列的表达式写在"默认值"一列，标记为"生成列"。
- docx 的图片按 2 倍像素画，按 1/192 英寸一个像素换算尺寸，再按版心缩放；样式来源中缺的样式补上时编号加 `DBLens` 前缀，基于的样式去掉（避免依赖缺的样式）。
- docx 的目录包括标题 1、2（每张表一项）；标题 1 前分页。
- 依赖：`zip` 9.0.5（`default-features = false`，只开 `deflate-flate2`；写计划时 9.1.x 发布不满 7 天，用 `cargo update --precise` 锁定）、`quick-xml` 0.42.0；
  service 的 `minijinja` 开 `serde` 特性。
- `document` 模块在 `dblens_service` 中公开（`dblens_service::document::…`），不从 crate 根重新导出。

## 验证

- 后端：fmt、clippy 通过；全部测试在 `DBLENS_LANG=en` 下通过（704 个）。
  - service：内容组织（复合外键、跨 schema 的链接、逻辑外键、被引用、读不出的表、按表筛选、视图定义开关）；
    markdown、html 的快照（中英文）、HTML 转义和内嵌图片、自定义模板的过滤器、语法错误的行号、模板不能 include 文件；
    docx 的正文快照、图片的关系和内容类型和尺寸、打开时更新字段、全部 XML 部件格式正确、中文版 Word 样式编号的样式来源、缺的样式补上、横向纸张的版心宽度、
    无效的样式来源、控制字符；xlsx 的工作表、链接、截断；开始前检查（xlsx 只用于数据字典、浏览器模式不接受本机路径、模板语法、样式来源、输出目录）；
    模型导出 markdown 到本机路径（图片在旁边的目录）、浏览器模式 markdown 打包 zip、xlsx 下载。
  - api：没有 `local_files` 时另存内置模板被禁止、本机路径作为检查出的问题；用另存的内置模板导出数据字典到本机路径。
  - 实库：11 个库的模型测试中，对测试 schema 导出 markdown 数据字典，核对表和外键的链接。
- 前端：tsc、Vitest（种类与格式的联动、记住的选项、组成请求、文件名、像素倍数）、打包通过。
- 写计划时实测：docx 由 Word 打开、更新目录、导出 PDF 后逐页核对（标题、目录页码、图片缩放、表格、链接）；xlsx 由 Excel 打开（三张工作表、链接）；
  浏览器中模型导出 HTML 设计文档（ER 图浅色白底、列级连线、"2 列"、逻辑外键虚线）、库导出 xlsx 数据字典。
- 未验证：桌面程序的保存、选择文件对话框，"在文件夹中显示"；docx 套用用户自己的样式来源后在 WPS 中的效果。
````

- [ ] **Step 3: 修改 `docs/superpowers/specs/2026-10-22-dblens-design.md`**

```diff
--- a/docs/superpowers/specs/2026-10-22-dblens-design.md
+++ b/docs/superpowers/specs/2026-10-22-dblens-design.md
@@ -332,7 +332,8 @@ trait Policy: Send + Sync {
 - 国产库第一批已完成：TiDB、OceanBase 社区版（MySQL 模式）、openGauss（A、PG 兼容模式，含 SHA256 认证）、人大金仓（Oracle、PG 模式）（设计 `docs/superpowers/specs/2026-10-27-domestic-batch1-design.md`，详细计划 `docs/superpowers/plans/2026-10-27-domestic-batch1.md`，说明 `docs/notes/domestic-batch1.md`）。第二批（PolarDB、TDSQL、GaussDB，只有云服务）、第三批（GBase 8s、OceanBase 的 Oracle 模式）尚未开始；GoldenDB 没有可用的测试环境。
 - 2026-10-27 M4 拆成三份（用户确认）：M4a 模型与比对（模型文件、反向生成、比对与同步）；M4b ER 图；M4c 数据字典和设计文档导出。
 - M4a 已完成（设计 `docs/superpowers/specs/2026-10-27-m4a-model-compare-design.md`，详细计划 `docs/superpowers/plans/2026-10-27-m4a-model-compare.md`，说明 `docs/notes/m4a-model-compare.md`）。M4b、M4c 尚未开始。
-- M4b 已完成（设计 `docs/superpowers/specs/2026-10-28-m4b-er-diagram-design.md`，详细计划 `docs/superpowers/plans/2026-10-28-m4b-er-diagram.md`，说明 `docs/notes/m4b-er-diagram.md`）。M4c 尚未开始。
+- M4b 已完成（设计 `docs/superpowers/specs/2026-10-28-m4b-er-diagram-design.md`，详细计划 `docs/superpowers/plans/2026-10-28-m4b-er-diagram.md`，说明 `docs/notes/m4b-er-diagram.md`）。
+- M4c 已完成（设计 `docs/superpowers/specs/2026-10-28-m4c-document-export-design.md`，详细计划 `docs/superpowers/plans/2026-10-28-m4c-document-export.md`，说明 `docs/notes/m4c-document-export.md`）。M4 全部完成。
 
 ## 12. 风险与待验证项
 
```

- [ ] **Step 4: 检查**

检查文档中的路径和接口与代码一致；按执行时的实际结果更新"验证"一节（测试数、实库与浏览器实测的内容）。

Expected: 无。

- [ ] **Step 5: 提交**

```bash
git add docs/notes/m0-4b-api.md docs/notes/m4c-document-export.md docs/superpowers/specs/2026-10-22-dblens-design.md
git commit -m "docs: M4c 文档导出的说明、接口和进度" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```
