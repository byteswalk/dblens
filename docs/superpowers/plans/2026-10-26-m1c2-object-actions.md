# M1c2 对象操作 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 对象树中的对象可以查看 DDL、生成 SQL（含按外键的关联查询），表和视图可以在只读的数据标签中浏览。

**Architecture:** `dblens-dialect` 负责各方言取 DDL 的查询、PG 系 DDL 的拼装、外键关系、关联查询和数据 SQL（无 IO，单元测试）；`dblens-service` 经元数据连接池和缓存取数，`dblens-api` 提供四个接口。前端在对象树右键菜单中加查看 DDL（懒加载的 Monaco 只读预览）和生成 SQL（插入当前编辑器），新增数据标签：SQL 由后端生成，经标签自己的会话和 runner 流式读取。

**Tech Stack:** Rust（axum、tokio）；React 19 + TypeScript、antd 6（Dropdown、Modal）、Monaco、Glide Data Grid、zustand、@tanstack/react-query 5、Vitest。

**Spec:** `docs/superpowers/specs/2026-10-26-m1c2-object-actions-design.md`

## Global Constraints

- DDL 和生成 SQL 只读取元数据，不修改数据库；数据标签只读，不生成任何修改语句，生产环境也不需要确认。
- 查看 DDL：弹出只读预览（复制、在新标签中打开）；生成的 SQL 插入当前编辑器的光标处，当前不是查询标签时用该连接新开一个。
- 数据标签：WHERE 过滤原样放进括号；点列头依次升序、降序、不排序，同一时间只按一列；统计行数手动触发；同一对象只开一个；随标签保存、恢复。
- 依赖只用正式发布版，跟随最新；新版发布满 7 天再升级；本里程碑不引入新依赖。
- 前端给用户看的文字都用词条（中英文）；后端错误文字用 `msg!` 词条；日志不翻译。
- 代码注释用中文；文件 UTF-8、LF 换行；前端代码风格：无分号、单引号、两格缩进。
- cargo 命令加 `--config "source.mirror.registry='sparse+https://rsproxy.cn/index/'"`（clippy 写在 `clippy` 之后），并设 `CARGO_INCREMENTAL=0`；后端测试在 `DBLENS_LANG=en` 下运行。
- 前端工具用 PowerShell 执行（`npm`/`npx` 在本机不可用，用 `pnpm exec`）。
- 测试数据库的密码只在命令行中以内联环境变量传入，不写进仓库、报告或提交信息。

## 与设计文档的差异（写计划时实测后调整，已同步修改设计文档）

- 函数、过程、包、包体的 DDL 各方言都取程序源码（`DBMS_METADATA` 返回 CLOB，长的包会在 64 KB 处截断）。
- Oracle、达梦的表 DDL 由 `DBMS_METADATA` 给出时不含索引和注释，用结构模型补上。
- 生成 SQL 的参数是 `object` + `object_kind`（视图也能生成 SELECT）；外键关系另有 `…/relations` 接口，前端用它组装"关联查询"子菜单。
- 对象不存在时，由数据库给出 DDL 的方言返回数据库的错误（422），不是 404。
- WebSocket 的 cancel 在等 fetch_more 时也能结束（见任务 7）。

## 执行说明

任务 2–7 的代码都已在临时副本中逐任务实现并通过检查（含 5 个实库和浏览器实测），执行时按顺序重放。新文件给出全文，修改的文件给出 diff（基于上一个任务完成后的状态，可直接 `git apply`）。`Cargo.lock`、`web/pnpm-lock.yaml` 本里程碑不变（任务 1 有升级时除外）。

## 文件结构

```text
crates/dblens-dialect/src/{ddl.rs, browse.rs}         DDL、外键关系、关联查询、数据 SQL（新增）
crates/dblens-dialect/src/{lib.rs, meta/mod.rs, meta/rows.rs}   导出、MetaKey::Ddl、MetaRow::columns
crates/dblens-core/locales/app.json                   后端词条
crates/dblens-service/src/objects.rs                  对象操作（新增）
crates/dblens-service/src/{metadata.rs, lib.rs}; tests/live.rs   run、导出、实库测试
crates/dblens-api/src/{routes.rs, lib.rs, ws.rs}; tests/{live.rs, api.rs}   接口、WebSocket 取消
docs/notes/m0-4b-api.md                               接口说明
web/src/shared/api.ts                                 接口客户端
web/src/features/tree/{objectMenu.ts, DdlDialog.tsx}  右键菜单逻辑、DDL 预览（新增）
web/src/features/tree/{ObjectTree.tsx, nodes.ts}      菜单、双击、程序节点的键
web/src/features/data/                                数据标签、排序、统计行数（新增）
web/src/features/editor/{tabs.ts, runner.ts, actions.tsx, Workspace.tsx, QueryView.tsx}   标签、rerun、插入
web/src/features/results/{ResultGrid.tsx, ResultsPanel.tsx}   列头排序
web/src/locales/{zh-CN,en}/{tree.json, editor.json}   词条
docs/notes/m1c2-object-actions.md                     说明文档（新增）
```

---

### Task 1: 依赖复查

按依赖原则（M1b2 设计文档 3.1 节）复查前端和后端的全部直接依赖。本里程碑不引入新依赖。

**Files:** 视复查结果而定（`web/package.json`、`Cargo.toml` 及锁文件）。

- [ ] **Step 1: 运行复查脚本**

```powershell
$env:PATH = "D:\Environments\nvm4w\nodejs;$env:PATH"; $env:NPM_REGISTRY = 'https://registry.npmmirror.com'; node scripts/check-deps.mjs
```

Expected：2026-10-26 写计划时为"共 75 个直接依赖，1 个可升级"：i18next 26.5.2 -> 26.5.3（2026-10-19 发布）。未满 7 天暂不升级的有 @tanstack/react-query 5.105.0、antd 6.6.8、react-i18next 17.1.9、react-resizable-panels 4.15.0、simple-icons 16.36.0、minijinja 3.0.4、uuid 1.29.0。crates.io 偶尔"查询失败：fetch failed"，重跑即可。

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

Expected：全部通过。结果记进任务 7 的说明文档（加"依赖"一节）。

- [ ] **Step 3: 提交（有改动时）**

```bash
git add web/package.json web/pnpm-lock.yaml Cargo.toml Cargo.lock
git commit -m "chore: 依赖复查：升级到发布满 7 天的最新正式版" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 2: 方言层：DDL、外键关系、关联查询、数据 SQL

`dblens-dialect` 新增两个无 IO 的模块：
- `ddl.rs`：各方言取 DDL 的查询（`ddl_query`：MySQL 系 `SHOW CREATE`，Oracle / 达梦 `DBMS_METADATA.GET_DDL`；程序类返回 None，由服务层取程序源码；PG 系返回 None，由结构模型拼装）、PG 系的拼装（`table_ddl` = `table_create` + `table_extras`、`view_ddl`、`sequence_ddl`）、从结果行取文本（`ddl_text`，LOB 截断时注明）。
- `browse.rs`：外键关系（`relations`：本表引用的 out、同一 schema 中引用本表的 in）、关联查询（`join_sql`，别名 t1、t2…）、数据标签的 SQL（`data_sql`：过滤、单列排序、计数）。

`MetaKey::Ddl` 只用于缓存，不在模板文件中；`MetaRow::columns()` 给 `ddl_text` 找列。后端词条加 `ddl.truncated`、`ddl.unsupported`。

**Files:**
- Modify: `crates/dblens-core/locales/app.json`
- Create: `crates/dblens-dialect/src/browse.rs`
- Create: `crates/dblens-dialect/src/ddl.rs`
- Modify: `crates/dblens-dialect/src/lib.rs`
- Modify: `crates/dblens-dialect/src/meta/mod.rs`
- Modify: `crates/dblens-dialect/src/meta/rows.rs`

**Interfaces:**
- Produces（`dblens_dialect`）：
  - `DdlKind::{Table, View, MaterializedView, Sequence, Function, Procedure, Package, PackageBody, Synonym}`（serde snake_case）、`as_str()`、`parse(&str)`；
  - `Dialect::ddl_query(kind, schema, name) -> Result<Option<MetaQuery>>`；
  - `Dialect::table_ddl(schema, &Table) -> Result<String>`、`table_extras(schema, &Table, triggers: bool) -> Result<String>`（每条以换行开头、以 `;` 结束）、`view_ddl(schema, &View, definition, materialized) -> Result<String>`、`sequence_ddl(schema, &Sequence) -> String`；
  - `normalize_ddl(&str) -> String`、`ddl_text(&MetaRows) -> Option<String>`；
  - `Direction::{Out, In}`、`Relation { direction, name, schema, table, local_columns, other_columns }`（Serialize）、`relations(schema, &Table, all: &[Table]) -> Vec<Relation>`；
  - `Dialect::join_sql(schema, table, &[Relation]) -> String`、`Dialect::data_sql(schema, object, filter: Option<&str>, order: Option<(&str, bool)>, count: bool) -> String`；
  - `MetaKey::Ddl`（`"ddl"`）、`MetaRow::columns() -> &[String]`。

- [ ] **Step 1: 修改 `crates/dblens-core/locales/app.json`**

```diff
--- a/crates/dblens-core/locales/app.json
+++ b/crates/dblens-core/locales/app.json
@@ -8,6 +8,8 @@
   "api.setting_too_large": {"zh-CN": "设置项 %{key} 的值超过 64 KiB", "en": "The value of setting %{key} exceeds 64 KiB"},
   "api.still_running": {"zh-CN": "上一段 SQL 还没有执行完", "en": "The previous SQL has not finished yet"},
   "app.name": {"zh-CN": "DBLens 数洞", "en": "DBLens"},
+  "ddl.truncated": {"zh-CN": "DDL 超过 64 KB，以上只是开头部分", "en": "The DDL exceeds 64 KB; only the beginning is shown above"},
+  "ddl.unsupported": {"zh-CN": "该数据库不支持查看 %{kind} 的 DDL", "en": "This database does not support viewing the DDL of %{kind}"},
   "desktop.bind_failed": {"zh-CN": "无法监听本机端口：%{error}", "en": "Cannot listen on a local port: %{error}"},
   "desktop.log_dir_failed": {"zh-CN": "无法创建日志目录 %{dir}：%{error}", "en": "Cannot create the log directory %{dir}: %{error}"},
   "desktop.no_main_window": {"zh-CN": "主窗口不存在", "en": "The main window does not exist"},
```

- [ ] **Step 2: 新建 `crates/dblens-dialect/src/browse.rs`**

```rust
//! 关联查询与数据标签的 SQL（M1c2 设计文档第 4、5 节）。

use dblens_core::schema::Table;
use serde::{Deserialize, Serialize};

use crate::Dialect;

/// 外键的方向：本表引用对方（`Out`，对方是父表），或对方引用本表（`In`，对方是子表）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    Out,
    In,
}

/// 与某张表有外键关系的表。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Relation {
    pub direction: Direction,
    /// 外键约束名。
    pub name: Option<String>,
    pub schema: String,
    pub table: String,
    /// 本表一侧的列。
    pub local_columns: Vec<String>,
    /// 对方一侧的列。
    pub other_columns: Vec<String>,
}

/// 表 `table`（在 `schema` 中）的外键关系：先是它引用的表，再是同一 schema 中引用它的表（`all` 为该 schema 的全部表）。
/// 自引用的外键只算一次（作为 `Out`）。
pub fn relations(schema: &str, table: &Table, all: &[Table]) -> Vec<Relation> {
    let mut out: Vec<Relation> = table
        .foreign_keys
        .iter()
        .map(|f| Relation {
            direction: Direction::Out,
            name: f.name.clone(),
            schema: f.ref_schema.clone().unwrap_or_else(|| schema.to_string()),
            table: f.ref_table.clone(),
            local_columns: f.columns.clone(),
            other_columns: f.ref_columns.clone(),
        })
        .collect();
    for t in all.iter().filter(|t| t.name != table.name) {
        for f in &t.foreign_keys {
            if f.ref_table == table.name && f.ref_schema.as_deref().unwrap_or(schema) == schema {
                out.push(Relation {
                    direction: Direction::In,
                    name: f.name.clone(),
                    schema: schema.to_string(),
                    table: t.name.clone(),
                    local_columns: f.ref_columns.clone(),
                    other_columns: f.columns.clone(),
                });
            }
        }
    }
    out
}

impl Dialect {
    /// 关联查询：本表别名 `t1`，相关的表依次为 `t2`、`t3`…，每个外键一个 JOIN，多列外键的条件用 AND 连接。
    pub fn join_sql(&self, schema: &str, table: &str, rels: &[Relation]) -> String {
        let mut select = vec!["t1.*".to_string()];
        let mut joins = Vec::new();
        for (i, r) in rels.iter().enumerate() {
            let alias = format!("t{}", i + 2);
            select.push(format!("{alias}.*"));
            let on: Vec<String> = r
                .local_columns
                .iter()
                .zip(&r.other_columns)
                .map(|(l, o)| format!("t1.{} = {alias}.{}", self.ident(l), self.ident(o)))
                .collect();
            joins.push(format!(
                "JOIN {} {alias} ON {}",
                self.qualified(Some(&r.schema), &r.table),
                on.join(" AND ")
            ));
        }
        let mut sql =
            format!("SELECT {}\nFROM {} t1", select.join(", "), self.qualified(Some(schema), table));
        for j in joins {
            sql += "\n";
            sql += &j;
        }
        sql
    }

    /// 数据标签的查询：`SELECT * FROM 对象 [WHERE (过滤)] [ORDER BY 列 ASC|DESC]`；`count` 时为 `SELECT COUNT(*)`（不排序）。
    /// 过滤条件由用户书写，原样放进括号中。
    pub fn data_sql(
        &self,
        schema: &str,
        object: &str,
        filter: Option<&str>,
        order: Option<(&str, bool)>,
        count: bool,
    ) -> String {
        let mut sql = format!(
            "SELECT {} FROM {}",
            if count { "COUNT(*)" } else { "*" },
            self.qualified(Some(schema), object)
        );
        if let Some(f) = filter.map(str::trim).filter(|f| !f.is_empty()) {
            sql += &format!(" WHERE ({f})");
        }
        if let (false, Some((column, desc))) = (count, order) {
            sql += &format!(" ORDER BY {} {}", self.ident(column), if desc { "DESC" } else { "ASC" });
        }
        sql
    }
}

#[cfg(test)]
mod tests {
    use dblens_core::schema::ForeignKey;

    use super::*;
    use crate::Variant;

    fn fk(cols: &[&str], ref_schema: Option<&str>, ref_table: &str, ref_cols: &[&str]) -> ForeignKey {
        ForeignKey {
            name: Some(format!("fk_{ref_table}")),
            columns: cols.iter().map(|s| s.to_string()).collect(),
            ref_schema: ref_schema.map(str::to_string),
            ref_table: ref_table.into(),
            ref_columns: ref_cols.iter().map(|s| s.to_string()).collect(),
            on_delete: None,
            on_update: None,
        }
    }

    fn table(name: &str, fks: Vec<ForeignKey>) -> Table {
        Table { name: name.into(), foreign_keys: fks, ..Default::default() }
    }

    /// emp 引用 dept 和 hr2.region（两列）；task 引用 emp；emp 自引用（manager）。
    fn schema() -> Vec<Table> {
        vec![
            table(
                "emp",
                vec![
                    fk(&["dept_id"], None, "dept", &["id"]),
                    fk(&["region", "country"], Some("hr2"), "region", &["code", "country"]),
                    fk(&["manager_id"], Some("hr"), "emp", &["id"]),
                ],
            ),
            table("dept", vec![]),
            table("task", vec![fk(&["owner_id"], Some("hr"), "emp", &["id"])]),
            table("other", vec![fk(&["x"], Some("elsewhere"), "emp", &["id"])]),
        ]
    }

    #[test]
    fn relations_in_both_directions() {
        let all = schema();
        let rels = relations("hr", &all[0], &all);
        let got: Vec<_> = rels.iter().map(|r| (r.direction, r.schema.as_str(), r.table.as_str())).collect();
        assert_eq!(
            got,
            [
                (Direction::Out, "hr", "dept"),
                (Direction::Out, "hr2", "region"),
                (Direction::Out, "hr", "emp"),
                (Direction::In, "hr", "task"),
            ],
            "其他 schema 的同名表不算；自引用只算一次"
        );
        assert_eq!(
            (rels[3].local_columns.clone(), rels[3].other_columns.clone()),
            (vec!["id".to_string()], vec!["owner_id".to_string()])
        );
    }

    #[test]
    fn join_uses_aliases_and_multi_column_conditions() {
        let all = schema();
        let rels = relations("hr", &all[0], &all);
        let d = Dialect::new(Variant::Postgres);
        assert_eq!(
            d.join_sql("hr", "emp", &rels[..2]),
            "SELECT t1.*, t2.*, t3.*\nFROM hr.emp t1\nJOIN hr.dept t2 ON t1.dept_id = t2.id\nJOIN hr2.region t3 ON t1.region = t3.code AND t1.country = t3.country"
        );
        assert_eq!(
            d.join_sql("hr", "emp", &rels[3..]),
            "SELECT t1.*, t2.*\nFROM hr.emp t1\nJOIN hr.task t2 ON t1.id = t2.owner_id"
        );
    }

    #[test]
    fn data_sql_filter_order_and_count() {
        let d = Dialect::new(Variant::Oracle);
        assert_eq!(d.data_sql("HR", "EMP", None, None, false), "SELECT * FROM HR.EMP");
        assert_eq!(
            d.data_sql("HR", "Emp Tab", Some(" salary > 10 OR 1=1 "), Some(("Name", true)), false),
            "SELECT * FROM HR.\"Emp Tab\" WHERE (salary > 10 OR 1=1) ORDER BY \"Name\" DESC"
        );
        assert_eq!(
            d.data_sql("HR", "EMP", Some("a = 1"), Some(("ID", false)), true),
            "SELECT COUNT(*) FROM HR.EMP WHERE (a = 1)"
        );
        assert_eq!(
            Dialect::new(Variant::MySql).data_sql("shop", "t", Some("  "), Some(("id", false)), false),
            "SELECT * FROM shop.t ORDER BY id ASC"
        );
    }
}
```

- [ ] **Step 3: 新建 `crates/dblens-dialect/src/ddl.rs`**

```rust
//! "查看 DDL"（M1c2 设计文档第 3 节）：
//! - 表、视图、序列、同义词：MySQL 系用 `SHOW CREATE …`，Oracle、达梦用 `DBMS_METADATA.GET_DDL`，由数据库给出
//!   （`ddl_query`）；PG 系没有取整张表 DDL 的函数，由结构模型拼出（`table_ddl` 等）。
//! - 函数、过程、包：各方言都用已有的程序源码查询（`MetaKey::RoutineSource`）。它按行读取，
//!   不受 LOB 只读前 64 KB 预览的限制（`DBMS_METADATA` 返回的是 CLOB）。

use dblens_core::schema::{Sequence, Table, View};
use dblens_core::{Result, Value, msg};

use crate::meta::MetaRows;
use serde::{Deserialize, Serialize};

use crate::meta::{MetaKey, MetaQuery};
use crate::{Dialect, Family};

/// 可以查看 DDL 的对象种类。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DdlKind {
    Table,
    View,
    MaterializedView,
    Sequence,
    Function,
    Procedure,
    Package,
    PackageBody,
    Synonym,
}

impl DdlKind {
    pub fn as_str(self) -> &'static str {
        match self {
            DdlKind::Table => "table",
            DdlKind::View => "view",
            DdlKind::MaterializedView => "materialized_view",
            DdlKind::Sequence => "sequence",
            DdlKind::Function => "function",
            DdlKind::Procedure => "procedure",
            DdlKind::Package => "package",
            DdlKind::PackageBody => "package_body",
            DdlKind::Synonym => "synonym",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        [
            DdlKind::Table,
            DdlKind::View,
            DdlKind::MaterializedView,
            DdlKind::Sequence,
            DdlKind::Function,
            DdlKind::Procedure,
            DdlKind::Package,
            DdlKind::PackageBody,
            DdlKind::Synonym,
        ]
        .into_iter()
        .find(|k| k.as_str() == s)
    }
}

impl Dialect {
    /// 由数据库直接给出 DDL 的查询（结果见 `ddl_text`）。返回 `Ok(None)` 时由调用方拼装：
    /// PG 系的表、视图、物化视图、序列用结构模型，各方言的函数、过程、包用程序源码。
    /// 该方言没有这种对象时返回 `invalid`。
    pub fn ddl_query(&self, kind: DdlKind, schema: &str, name: &str) -> Result<Option<MetaQuery>> {
        let unsupported = || dblens_core::Error::invalid(msg!("ddl.unsupported", kind = kind.as_str()));
        let query = |sql: String, params: Vec<Value>| Ok(Some(MetaQuery { key: MetaKey::Ddl, sql, params }));
        match self.family() {
            Family::MySql => {
                let what = match kind {
                    DdlKind::Table => "TABLE",
                    DdlKind::View => "VIEW",
                    DdlKind::Sequence => "SEQUENCE",
                    DdlKind::Function | DdlKind::Procedure => return Ok(None),
                    _ => return Err(unsupported()),
                };
                // SHOW CREATE 不能用绑定变量，名字按方言加引号
                query(format!("SHOW CREATE {what} {}", self.qualified(Some(schema), name)), vec![])
            }
            Family::Oracle | Family::Dameng => {
                let object_type = match kind {
                    DdlKind::Table => "TABLE",
                    DdlKind::View => "VIEW",
                    DdlKind::MaterializedView => "MATERIALIZED_VIEW",
                    DdlKind::Sequence => "SEQUENCE",
                    DdlKind::Synonym => "SYNONYM",
                    DdlKind::Function | DdlKind::Procedure | DdlKind::Package | DdlKind::PackageBody => {
                        return Ok(None);
                    }
                };
                let sql = format!(
                    "SELECT DBMS_METADATA.GET_DDL({}, {}, {}) AS ddl FROM DUAL",
                    self.placeholder(1),
                    self.placeholder(2),
                    self.placeholder(3)
                );
                query(
                    sql,
                    vec![
                        Value::String(object_type.into()),
                        Value::String(name.into()),
                        Value::String(schema.into()),
                    ],
                )
            }
            Family::Postgres => match kind {
                DdlKind::Table
                | DdlKind::View
                | DdlKind::MaterializedView
                | DdlKind::Sequence
                | DdlKind::Function
                | DdlKind::Procedure => Ok(None),
                _ => Err(unsupported()),
            },
        }
    }

    fn comment_literal(&self, text: &str) -> Result<String> {
        self.literal(&Value::String(text.into()))
    }

    /// PG 系的建表语句：列、约束、分区，之后是索引、触发器和注释，每条以 `;` 结束。
    pub fn table_ddl(&self, schema: &str, t: &Table) -> Result<String> {
        let q = self.qualified(Some(schema), &t.name);
        let list = |cols: &[String]| cols.iter().map(|c| self.ident(c)).collect::<Vec<_>>().join(", ");
        let mut items: Vec<String> = Vec::new();
        for c in &t.columns {
            let mut s = format!("{} {}", self.ident(&c.name), c.data_type.native);
            if let Some(g) = &c.generated {
                s += &format!(" GENERATED ALWAYS AS ({g}) STORED");
            } else if let Some(d) = &c.default {
                s += &format!(" DEFAULT {d}");
            } else if c.auto_increment {
                s += " GENERATED BY DEFAULT AS IDENTITY";
            }
            if !c.nullable {
                s += " NOT NULL";
            }
            items.push(s);
        }
        let constraint = |name: &Option<String>| {
            name.as_ref().map(|n| format!("CONSTRAINT {} ", self.ident(n))).unwrap_or_default()
        };
        if let Some(pk) = &t.primary_key {
            items.push(format!("{}PRIMARY KEY ({})", constraint(&pk.name), list(&pk.columns)));
        }
        for u in &t.uniques {
            items.push(format!("{}UNIQUE ({})", constraint(&u.name), list(&u.columns)));
        }
        for f in &t.foreign_keys {
            let mut s = format!(
                "{}FOREIGN KEY ({}) REFERENCES {} ({})",
                constraint(&f.name),
                list(&f.columns),
                self.qualified(f.ref_schema.as_deref().or(Some(schema)), &f.ref_table),
                list(&f.ref_columns)
            );
            for (what, action) in [("DELETE", &f.on_delete), ("UPDATE", &f.on_update)] {
                if let Some(a) = action.as_deref().filter(|a| *a != "NO ACTION") {
                    s += &format!(" ON {what} {a}");
                }
            }
            items.push(s);
        }
        for c in &t.checks {
            let e = c.expression.trim();
            let e = if e.starts_with('(') && e.ends_with(')') { e.to_string() } else { format!("({e})") };
            items.push(format!("{}CHECK {e}", constraint(&c.name)));
        }
        let mut out = format!("CREATE TABLE {q} (\n    {}\n)", items.join(",\n    "));
        if let Some(p) = &t.partition {
            out += &format!(" PARTITION BY {p}");
        }
        out += ";\n";
        for i in &t.indexes {
            let cols: Vec<String> = i
                .columns
                .iter()
                .map(|c| {
                    // 普通列加引号；表达式（如 lower(name)）原样
                    let name = if t.columns.iter().any(|x| x.name == c.name) {
                        self.ident(&c.name)
                    } else {
                        c.name.clone()
                    };
                    if c.descending { format!("{name} DESC") } else { name }
                })
                .collect();
            let using = i
                .method
                .as_deref()
                .filter(|m| !m.eq_ignore_ascii_case("btree"))
                .map(|m| format!(" USING {}", m.to_ascii_lowercase()))
                .unwrap_or_default();
            out += &format!(
                "\nCREATE {}INDEX {} ON {q}{using} ({});",
                if i.unique { "UNIQUE " } else { "" },
                self.ident(&i.name),
                cols.join(", ")
            );
        }
        for tr in &t.triggers {
            if let Some(d) = &tr.definition {
                out += &format!("\n{};", d.trim_end_matches(';').trim());
            }
        }
        if let Some(c) = &t.comment {
            out += &format!("\nCOMMENT ON TABLE {q} IS {};", self.comment_literal(c)?);
        }
        for c in &t.columns {
            if let Some(cm) = &c.comment {
                out += &format!(
                    "\nCOMMENT ON COLUMN {q}.{} IS {};",
                    self.ident(&c.name),
                    self.comment_literal(cm)?
                );
            }
        }
        Ok(out.trim_end().to_string() + "\n")
    }

    /// PG 系的视图 / 物化视图：`CREATE [MATERIALIZED] VIEW … AS 定义;`，有注释时附上。
    pub fn view_ddl(&self, schema: &str, v: &View, definition: &str, materialized: bool) -> Result<String> {
        let q = self.qualified(Some(schema), &v.name);
        let kind = if materialized { "MATERIALIZED VIEW" } else { "VIEW" };
        let mut out = format!("CREATE {kind} {q} AS\n{};\n", definition.trim().trim_end_matches(';'));
        if let Some(c) = &v.comment {
            out += &format!("\nCOMMENT ON {kind} {q} IS {};\n", self.comment_literal(c)?);
        }
        Ok(out)
    }

    /// PG 系的序列。
    pub fn sequence_ddl(&self, schema: &str, s: &Sequence) -> String {
        let mut out = format!("CREATE SEQUENCE {}", self.qualified(Some(schema), &s.name));
        let mut opt = |label: &str, v: &Option<String>| {
            if let Some(v) = v {
                out += &format!("\n    {label} {v}");
            }
        };
        opt("START WITH", &s.start);
        opt("INCREMENT BY", &s.increment);
        opt("MINVALUE", &s.min_value);
        opt("MAXVALUE", &s.max_value);
        if let Some(c) = s.cache {
            out += &format!("\n    CACHE {c}");
        }
        out += if s.cycle { "\n    CYCLE" } else { "\n    NO CYCLE" };
        out + ";\n"
    }
}

/// 数据库返回的 DDL 文本：统一换行，去掉首尾空白。
pub fn normalize_ddl(text: &str) -> String {
    text.replace("\r\n", "\n").trim().to_string() + "\n"
}

/// `ddl_query` 的结果：第一行的 `ddl` 列（Oracle、达梦），或名字以 `Create ` 开头的列（MySQL 的 `SHOW CREATE`）。
/// LOB 只读到前 64 KB 时在末尾注明已截断。没有结果时返回 None。
pub fn ddl_text(rows: &MetaRows) -> Option<String> {
    let row = rows.iter().next()?;
    let column = row.columns().iter().find(|c| *c == "ddl" || c.starts_with("create "))?.clone();
    let truncated = matches!(row.get(&column), Value::Lob(l) if l.truncated);
    let mut text = normalize_ddl(&row.str(&column)?);
    if truncated {
        text += &format!("-- {}\n", msg!("ddl.truncated").current());
    }
    Some(text)
}

#[cfg(test)]
mod tests {
    use dblens_core::schema::{
        CheckConstraint, Column, ForeignKey, Index, IndexColumn, PrimaryKey, Trigger, UniqueConstraint,
    };
    use dblens_core::{ColumnType, LogicalType};

    use super::*;
    use crate::Variant;

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

    fn pg() -> Dialect {
        Dialect::new(Variant::Postgres)
    }

    #[test]
    fn query_by_family() {
        let my =
            Dialect::new(Variant::MySql).ddl_query(DdlKind::Table, "shop", "order items").unwrap().unwrap();
        assert_eq!(my.sql, "SHOW CREATE TABLE shop.`order items`");
        assert!(my.params.is_empty());
        assert!(Dialect::new(Variant::MariaDb).ddl_query(DdlKind::Package, "s", "p").is_err());
        assert_eq!(
            Dialect::new(Variant::MySql).ddl_query(DdlKind::Procedure, "s", "p").unwrap(),
            None,
            "程序用源码"
        );

        let ora = Dialect::new(Variant::Oracle).ddl_query(DdlKind::Synonym, "HR", "SYN").unwrap().unwrap();
        assert_eq!(ora.sql, "SELECT DBMS_METADATA.GET_DDL(:1, :2, :3) AS ddl FROM DUAL");
        assert_eq!(
            ora.params,
            [Value::String("SYNONYM".into()), Value::String("SYN".into()), Value::String("HR".into())]
        );
        let dm =
            Dialect::new(Variant::Dameng).ddl_query(DdlKind::MaterializedView, "HR", "MV").unwrap().unwrap();
        assert_eq!(dm.params[0], Value::String("MATERIALIZED_VIEW".into()));
        assert_eq!(Dialect::new(Variant::Oracle).ddl_query(DdlKind::PackageBody, "HR", "P").unwrap(), None);

        assert_eq!(pg().ddl_query(DdlKind::Table, "public", "t").unwrap(), None);
        assert!(pg().ddl_query(DdlKind::Synonym, "public", "t").is_err());
    }

    #[test]
    fn pg_table_with_constraints_indexes_and_comments() {
        let mut id = col("id", "integer");
        id.nullable = false;
        id.auto_increment = true;
        let mut code = col("code", "character varying(20)");
        code.nullable = false;
        code.default = Some("'x'::character varying".into());
        code.comment = Some("编码 'A'".into());
        let mut total = col("total", "numeric(10,2)");
        total.generated = Some("(price * 2)".into());
        let t = Table {
            name: "Orders".into(),
            comment: Some("订单".into()),
            columns: vec![id, code, col("dept_id", "integer"), total],
            primary_key: Some(PrimaryKey { name: Some("orders_pkey".into()), columns: vec!["id".into()] }),
            uniques: vec![UniqueConstraint {
                name: Some("orders_code_key".into()),
                columns: vec!["code".into()],
            }],
            foreign_keys: vec![ForeignKey {
                name: Some("orders_dept_fk".into()),
                columns: vec!["dept_id".into()],
                ref_schema: Some("hr".into()),
                ref_table: "dept".into(),
                ref_columns: vec!["id".into()],
                on_delete: Some("CASCADE".into()),
                on_update: Some("NO ACTION".into()),
            }],
            checks: vec![CheckConstraint {
                name: Some("orders_total_check".into()),
                expression: "(total > (0)::numeric)".into(),
            }],
            indexes: vec![
                Index {
                    name: "orders_dept_idx".into(),
                    columns: vec![IndexColumn { name: "dept_id".into(), descending: true }],
                    unique: false,
                    method: Some("BTREE".into()),
                },
                Index {
                    name: "orders_lower_code".into(),
                    columns: vec![IndexColumn { name: "lower((code)::text)".into(), descending: false }],
                    unique: true,
                    method: Some("HASH".into()),
                },
            ],
            triggers: vec![Trigger {
                name: "trg".into(),
                timing: None,
                events: vec![],
                definition: Some(
                    "CREATE TRIGGER trg BEFORE INSERT ON public.\"Orders\" FOR EACH ROW EXECUTE FUNCTION f()"
                        .into(),
                ),
            }],
            partition: None,
        };
        assert_eq!(
            pg().table_ddl("public", &t).unwrap(),
            "CREATE TABLE public.\"Orders\" (
    id integer GENERATED BY DEFAULT AS IDENTITY NOT NULL,
    code character varying(20) DEFAULT 'x'::character varying NOT NULL,
    dept_id integer,
    total numeric(10,2) GENERATED ALWAYS AS ((price * 2)) STORED,
    CONSTRAINT orders_pkey PRIMARY KEY (id),
    CONSTRAINT orders_code_key UNIQUE (code),
    CONSTRAINT orders_dept_fk FOREIGN KEY (dept_id) REFERENCES hr.dept (id) ON DELETE CASCADE,
    CONSTRAINT orders_total_check CHECK (total > (0)::numeric)
);

CREATE INDEX orders_dept_idx ON public.\"Orders\" (dept_id DESC);
CREATE UNIQUE INDEX orders_lower_code ON public.\"Orders\" USING hash (lower((code)::text));
CREATE TRIGGER trg BEFORE INSERT ON public.\"Orders\" FOR EACH ROW EXECUTE FUNCTION f();
COMMENT ON TABLE public.\"Orders\" IS '订单';
COMMENT ON COLUMN public.\"Orders\".code IS '编码 ''A''';
"
        );
    }

    #[test]
    fn pg_partitioned_table_and_same_schema_reference() {
        let t = Table {
            name: "m".into(),
            columns: vec![col("d", "date")],
            foreign_keys: vec![ForeignKey {
                name: None,
                columns: vec!["d".into()],
                ref_schema: None,
                ref_table: "cal".into(),
                ref_columns: vec!["d".into()],
                on_delete: None,
                on_update: None,
            }],
            partition: Some("RANGE (d)".into()),
            ..Default::default()
        };
        assert_eq!(
            pg().table_ddl("s", &t).unwrap(),
            "CREATE TABLE s.m (\n    d date,\n    FOREIGN KEY (d) REFERENCES s.cal (d)\n) PARTITION BY RANGE (d);\n"
        );
    }

    #[test]
    fn pg_view_and_sequence() {
        let v = View { name: "v".into(), comment: Some("视图".into()), definition: None, columns: vec![] };
        assert_eq!(
            pg().view_ddl("s", &v, " SELECT 1;\n", true).unwrap(),
            "CREATE MATERIALIZED VIEW s.v AS\nSELECT 1;\n\nCOMMENT ON MATERIALIZED VIEW s.v IS '视图';\n"
        );
        let s = Sequence {
            name: "seq".into(),
            start: Some("100".into()),
            increment: Some("5".into()),
            min_value: Some("1".into()),
            max_value: None,
            cycle: false,
            cache: Some(10),
        };
        assert_eq!(
            pg().sequence_ddl("s", &s),
            "CREATE SEQUENCE s.seq\n    START WITH 100\n    INCREMENT BY 5\n    MINVALUE 1\n    CACHE 10\n    NO CYCLE;\n"
        );
    }

    #[test]
    fn text_from_result_rows() {
        let ora = MetaRows::new(
            vec!["DDL".into()],
            vec![vec![Value::String("\n  CREATE TABLE x\r\n(a INT);\r\n".into())]],
        );
        assert_eq!(ddl_text(&ora).unwrap(), "CREATE TABLE x\n(a INT);\n");
        let my = MetaRows::new(
            vec!["View".into(), "Create View".into(), "character_set_client".into()],
            vec![vec![
                Value::String("v".into()),
                Value::String("CREATE VIEW v AS select 1".into()),
                Value::String("utf8".into()),
            ]],
        );
        assert_eq!(ddl_text(&my).unwrap(), "CREATE VIEW v AS select 1\n");
        let lob = dblens_core::LobPreview {
            binary: false,
            preview: "CREATE".into(),
            total_len: 99,
            truncated: true,
        };
        let big = MetaRows::new(vec!["DDL".into()], vec![vec![Value::Lob(lob)]]);
        assert!(ddl_text(&big).unwrap().starts_with("CREATE\n-- "));
        assert_eq!(ddl_text(&MetaRows::new(vec!["DDL".into()], vec![])), None);
    }

    #[test]
    fn kinds_round_trip() {
        for k in [
            "table",
            "view",
            "materialized_view",
            "sequence",
            "function",
            "procedure",
            "package",
            "package_body",
            "synonym",
        ] {
            assert_eq!(DdlKind::parse(k).unwrap().as_str(), k);
        }
        assert_eq!(DdlKind::parse("index"), None);
    }
}
```

- [ ] **Step 4: 修改 `crates/dblens-dialect/src/lib.rs`**

```diff
--- a/crates/dblens-dialect/src/lib.rs
+++ b/crates/dblens-dialect/src/lib.rs
@@ -1,5 +1,7 @@
 //! 方言层：不做任何 IO，只负责各数据库的语法差异——标识符与字面量、语句拆分、类型映射、SQL 生成。
 
+mod browse;
+mod ddl;
 mod dml;
 pub mod ident;
 pub mod literal;
@@ -13,6 +15,8 @@ mod version;
 use dblens_core::{LogicalType, Result, Value};
 use dblens_driver::{DriverId, ServerInfo};
 
+pub use browse::{Direction, Relation, relations};
+pub use ddl::{DdlKind, ddl_text, normalize_ddl};
 pub use dml::DmlTemplate;
 pub use session::may_change_schema;
 pub use split::{SqlStatement, split_statements};
```

- [ ] **Step 5: 修改 `crates/dblens-dialect/src/meta/mod.rs`**

```diff
--- a/crates/dblens-dialect/src/meta/mod.rs
+++ b/crates/dblens-dialect/src/meta/mod.rs
@@ -42,6 +42,8 @@ pub enum MetaKey {
     ReservedWords,
     /// 识别兼容库用的附加查询（如 PolarDB），见 `Dialect::refine`。无参数。
     VariantProbe,
+    /// 由数据库给出的对象 DDL（不在模板文件中，由 `Dialect::ddl_query` 生成）。参数：schema、name、kind。
+    Ddl,
 }
 
 impl MetaKey {
@@ -92,6 +94,7 @@ impl MetaKey {
             MetaKey::RoutineSource => "routine_source",
             MetaKey::ReservedWords => "reserved_words",
             MetaKey::VariantProbe => "variant_probe",
+            MetaKey::Ddl => "ddl",
         }
     }
 }
```

- [ ] **Step 6: 修改 `crates/dblens-dialect/src/meta/rows.rs`**

```diff
--- a/crates/dblens-dialect/src/meta/rows.rs
+++ b/crates/dblens-dialect/src/meta/rows.rs
@@ -62,6 +62,11 @@ pub struct MetaRow<'a> {
 }
 
 impl MetaRow<'_> {
+    /// 列名（已转小写）。
+    pub fn columns(&self) -> &[String] {
+        self.columns
+    }
+
     pub fn get(&self, name: &str) -> &Value {
         const NULL: Value = Value::Null;
         self.columns.iter().position(|c| c == name).and_then(|i| self.row.get(i)).unwrap_or(&NULL)
```

- [ ] **Step 7: 检查**

```bash
export CARGO_INCREMENTAL=0
cargo fmt --all
cargo clippy --config "source.mirror.registry='sparse+https://rsproxy.cn/index/'" --workspace --all-targets -- -D warnings
DBLENS_LANG=en cargo test --config "source.mirror.registry='sparse+https://rsproxy.cn/index/'" --workspace
```

Expected: fmt、clippy 无输出；全部测试通过（dialect 新增 DDL 6 个、外键关系与关联查询和数据 SQL 若干）。

- [ ] **Step 8: 提交**

```bash
git add crates/dblens-core/locales/app.json crates/dblens-dialect/src/browse.rs crates/dblens-dialect/src/ddl.rs crates/dblens-dialect/src/lib.rs crates/dblens-dialect/src/meta/mod.rs crates/dblens-dialect/src/meta/rows.rs
git commit -m "feat(dialect): 查看 DDL（各方言的取法与 PG 系的拼装）、外键关系与关联查询、数据标签的 SQL" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 3: 服务层：对象操作

`MetadataService` 新增 `ddl`、`generate`、`relations`、`data_sql`（`objects.rs`）。取数都经元数据连接池和缓存：`metadata.rs` 的 `rows` 改为调用新的 `run(profile, key, args, build)`，`ddl` 用它按 `MetaKey::Ddl` 缓存。

- Oracle、达梦的 `DBMS_METADATA` 给出的表 DDL 不含索引和注释（实测），用结构模型的 `table_extras`（不含触发器）补上；没有 `;` 时先补上。
- 函数、过程、包、包体：各方言都取程序源码（`MetaKey::RoutineSource`），不受 LOB 64 KB 预览的限制。
- 生成 SQL：视图和物化视图只能 SELECT；UPDATE / DELETE 需要主键；关联查询给出 `reference` 时只连接与它相关的外键，否则连接本表引用的全部表，没有时报 `generate.no_relations`。

`table_extras` 在本任务中从 `table_ddl` 拆出，`USING 方法` 只用于 PG 系。服务层的实库测试在 5 个库中建父子表、视图、序列、函数，测完删除。

**Files:**
- Modify: `crates/dblens-core/locales/app.json`
- Modify: `crates/dblens-dialect/src/ddl.rs`
- Modify: `crates/dblens-service/src/lib.rs`
- Modify: `crates/dblens-service/src/metadata.rs`
- Create: `crates/dblens-service/src/objects.rs`
- Modify: `crates/dblens-service/tests/live.rs`

**Interfaces:**
- Consumes：任务 2 的全部接口。
- Produces（`dblens_service`）：
  - `GenerateKind::{Select, Insert, Update, Delete, Join}`、`ObjectKind::{Table（默认）, View, MaterializedView}`、`DataOrder { column, desc }`（serde snake_case）；
  - `MetadataService::ddl(&Profile, schema, DdlKind, name) -> Result<String>`；
  - `MetadataService::generate(&Profile, schema, object, ObjectKind, GenerateKind, reference: Option<(&str, &str)>) -> Result<String>`；
  - `MetadataService::relations(&Profile, schema, table) -> Result<Vec<Relation>>`；
  - `MetadataService::data_sql(&Profile, schema, object, filter, Option<&DataOrder>, count) -> Result<String>`；
  - 后端词条 `generate.needs_primary_key`、`generate.no_relations`、`generate.select_only`、`noun.object`。

- [ ] **Step 1: 修改 `crates/dblens-service/tests/live.rs`**

```diff
--- a/crates/dblens-service/tests/live.rs
+++ b/crates/dblens-service/tests/live.rs
@@ -10,10 +10,11 @@ use std::time::Duration;
 
 use dblens_core::{Error, Value};
 use dblens_dialect::meta::MetaKey;
+use dblens_dialect::{DdlKind, Direction};
 use dblens_driver::{ConnectParams, DriverId};
 use dblens_service::{
-    DefaultPolicy, DriverRegistry, Environment, ExecRequest, ProfileInput, SecretBackend, Secrets, Service,
-    SessionEvent, SessionId, Store,
+    DataOrder, DefaultPolicy, DriverRegistry, Environment, ExecRequest, GenerateKind, ObjectKind,
+    ProfileInput, SecretBackend, Secrets, Service, SessionEvent, SessionId, Store,
 };
 
 struct Live {
@@ -300,3 +301,159 @@ async fn mysql_cancel() {
     let slow = "SELECT COUNT(*) FROM information_schema.columns a, information_schema.columns b, information_schema.columns c";
     cancel_flow("DBLENS_TEST_MYSQL_URL", slow).await;
 }
+
+/// 对象操作（M1c2）：在测试 schema 中建父子表、视图、序列、函数，查看 DDL、生成 SQL、取外键关系，最后删除。
+async fn objects_flow(env: &str) {
+    let Some(live) = setup(env).await else { return };
+    let s = &live.service;
+    let profile = s.profile(&live.profile_id).unwrap();
+    let info = s.open_session(&live.profile_id).await.unwrap();
+    let id = info.id;
+    let upper = matches!(live.driver, DriverId::Oracle | DriverId::Dameng);
+    let n = |name: &str| if upper { name.to_uppercase() } else { name.to_string() };
+    let drops: &[&str] = match live.driver {
+        DriverId::Oracle => &[
+            "BEGIN EXECUTE IMMEDIATE 'DROP VIEW c2_v'; EXCEPTION WHEN OTHERS THEN NULL; END;",
+            "BEGIN EXECUTE IMMEDIATE 'DROP TABLE c2_child'; EXCEPTION WHEN OTHERS THEN NULL; END;",
+            "BEGIN EXECUTE IMMEDIATE 'DROP TABLE c2_parent'; EXCEPTION WHEN OTHERS THEN NULL; END;",
+            "BEGIN EXECUTE IMMEDIATE 'DROP SEQUENCE c2_seq'; EXCEPTION WHEN OTHERS THEN NULL; END;",
+            "BEGIN EXECUTE IMMEDIATE 'DROP FUNCTION c2_f'; EXCEPTION WHEN OTHERS THEN NULL; END;",
+        ],
+        DriverId::Mysql => {
+            &["DROP VIEW IF EXISTS c2_v", "DROP TABLE IF EXISTS c2_child", "DROP TABLE IF EXISTS c2_parent"]
+        }
+        _ => &[
+            "DROP VIEW IF EXISTS c2_v",
+            "DROP TABLE IF EXISTS c2_child",
+            "DROP TABLE IF EXISTS c2_parent",
+            "DROP SEQUENCE IF EXISTS c2_seq",
+            "DROP FUNCTION IF EXISTS c2_f",
+        ],
+    };
+    for sql in drops {
+        live.submit(id, ExecRequest::new(*sql)).await;
+    }
+    let creates: Vec<&str> = match live.driver {
+        DriverId::Mysql => vec![
+            "CREATE TABLE c2_parent (id INT PRIMARY KEY, code VARCHAR(10) NOT NULL UNIQUE COMMENT '编码') COMMENT='父表'",
+            "CREATE TABLE c2_child (id INT PRIMARY KEY, parent_id INT, qty INT, CONSTRAINT c2_fk FOREIGN KEY (parent_id) REFERENCES c2_parent (id), INDEX c2_idx (qty))",
+            "CREATE VIEW c2_v AS SELECT id, code FROM c2_parent",
+        ],
+        DriverId::Postgres => vec![
+            "CREATE TABLE c2_parent (id INT PRIMARY KEY, code VARCHAR(10) NOT NULL UNIQUE)",
+            "COMMENT ON TABLE c2_parent IS '父表'",
+            "COMMENT ON COLUMN c2_parent.code IS '编码'",
+            "CREATE TABLE c2_child (id INT GENERATED BY DEFAULT AS IDENTITY PRIMARY KEY, parent_id INT, qty INT CHECK (qty > 0), CONSTRAINT c2_fk FOREIGN KEY (parent_id) REFERENCES c2_parent (id) ON DELETE CASCADE)",
+            "CREATE INDEX c2_idx ON c2_child (qty DESC)",
+            "CREATE VIEW c2_v AS SELECT id, code FROM c2_parent",
+            "CREATE SEQUENCE c2_seq START 10 INCREMENT 2",
+            "CREATE FUNCTION c2_f() RETURNS int LANGUAGE sql AS 'SELECT 1'",
+        ],
+        _ => vec![
+            "CREATE TABLE c2_parent (id INT PRIMARY KEY, code VARCHAR(10) NOT NULL UNIQUE)",
+            "COMMENT ON TABLE c2_parent IS '父表'",
+            "CREATE TABLE c2_child (id INT PRIMARY KEY, parent_id INT, qty INT CHECK (qty > 0), CONSTRAINT c2_fk FOREIGN KEY (parent_id) REFERENCES c2_parent (id))",
+            "CREATE INDEX c2_idx ON c2_child (qty)",
+            "CREATE VIEW c2_v AS SELECT id, code FROM c2_parent",
+            "CREATE SEQUENCE c2_seq START WITH 10 INCREMENT BY 2",
+            "CREATE OR REPLACE FUNCTION c2_f RETURN INT IS BEGIN RETURN 1; END;",
+        ],
+    };
+    for sql in creates {
+        live.run(id, sql).await;
+    }
+    let schema = live.schema.as_str();
+    let ddl = |kind: DdlKind, name: String| {
+        let profile = &profile;
+        async move { s.metadata.ddl(profile, schema, kind, &name).await }
+    };
+
+    let child = ddl(DdlKind::Table, n("c2_child")).await.unwrap();
+    let low = child.to_lowercase();
+    assert!(
+        low.contains("c2_fk") && low.contains("foreign key") && low.contains("c2_parent"),
+        "{env}：{child}"
+    );
+    assert!(low.contains("c2_idx") || live.driver == DriverId::Dameng, "{env}：索引 {child}");
+    let parent = ddl(DdlKind::Table, n("c2_parent")).await.unwrap();
+    assert!(parent.contains("父表"), "{env}：表注释 {parent}");
+    let view = ddl(DdlKind::View, n("c2_v")).await.unwrap();
+    assert!(view.to_lowercase().contains("c2_parent"), "{env}：{view}");
+    if live.driver != DriverId::Mysql {
+        let seq = ddl(DdlKind::Sequence, n("c2_seq")).await.unwrap();
+        assert!(seq.to_lowercase().contains("c2_seq"), "{env}：{seq}");
+        let f = ddl(DdlKind::Function, n("c2_f")).await.unwrap();
+        assert!(f.to_lowercase().contains("c2_f"), "{env}：{f}");
+    } else {
+        assert!(matches!(ddl(DdlKind::Synonym, n("c2_v")).await, Err(Error::Invalid { .. })));
+    }
+    assert!(ddl(DdlKind::Table, n("c2_no_such")).await.is_err(), "{env}：不存在的表");
+
+    // 生成 SQL
+    let child_name = n("c2_child");
+    let generate =
+        |kind: GenerateKind, object_kind: ObjectKind, object: String, reference: Option<String>| {
+            let profile = &profile;
+            let schema = schema.to_string();
+            async move {
+                let r = reference.as_deref().map(|t| (schema.as_str(), t));
+                s.metadata.generate(profile, &schema, &object, object_kind, kind, r).await
+            }
+        };
+    let select = generate(GenerateKind::Select, ObjectKind::Table, child_name.clone(), None).await.unwrap();
+    assert!(select.to_lowercase().contains("parent_id"), "{select}");
+    let update = generate(GenerateKind::Update, ObjectKind::Table, child_name.clone(), None).await.unwrap();
+    assert!(update.starts_with("UPDATE"), "{update}");
+    let join = generate(GenerateKind::Join, ObjectKind::Table, child_name.clone(), Some(n("c2_parent")))
+        .await
+        .unwrap();
+    assert!(join.contains("JOIN") && join.contains(" t2 ON "), "{join}");
+    let r = generate(GenerateKind::Insert, ObjectKind::View, n("c2_v"), None).await;
+    assert!(matches!(r, Err(Error::Invalid { .. })), "视图只能生成 SELECT");
+    // 生成的 SELECT 能执行
+    live.run(id, &select).await;
+
+    let rels = s.metadata.relations(&profile, schema, &n("c2_parent")).await.unwrap();
+    assert!(
+        rels.iter().any(|r| r.direction == Direction::In && r.table.eq_ignore_ascii_case("c2_child")),
+        "{env}：{rels:?}"
+    );
+
+    // 数据标签的 SQL 能执行
+    let order = DataOrder { column: n("qty"), desc: true };
+    let data =
+        s.metadata.data_sql(&profile, schema, &child_name, Some("1 = 1"), Some(&order), false).await.unwrap();
+    live.run(id, &data).await;
+    let count = s.metadata.data_sql(&profile, schema, &child_name, None, None, true).await.unwrap();
+    live.run(id, &count).await;
+
+    for sql in drops {
+        live.submit(id, ExecRequest::new(*sql)).await;
+    }
+    s.sessions.close(id, false).await.unwrap();
+}
+
+#[tokio::test]
+async fn mysql_objects() {
+    objects_flow("DBLENS_TEST_MYSQL_URL").await;
+}
+
+#[tokio::test]
+async fn mariadb_objects() {
+    objects_flow("DBLENS_TEST_MARIADB_URL").await;
+}
+
+#[tokio::test]
+async fn postgres_objects() {
+    objects_flow("DBLENS_TEST_PG_URL").await;
+}
+
+#[tokio::test]
+async fn oracle_objects() {
+    objects_flow("DBLENS_TEST_ORACLE_URL").await;
+}
+
+#[tokio::test]
+async fn dameng_objects() {
+    objects_flow("DBLENS_TEST_DM_URL").await;
+}
```

- [ ] **Step 2: 修改 `crates/dblens-core/locales/app.json`**

```diff
--- a/crates/dblens-core/locales/app.json
+++ b/crates/dblens-core/locales/app.json
@@ -45,6 +45,9 @@
   "favorite.name_required": {"zh-CN": "名称不能为空", "en": "The name is required"},
   "favorite.parent_not_folder": {"zh-CN": "只能放在文件夹中", "en": "Favorites can only be placed in a folder"},
   "favorite.sql_required": {"zh-CN": "收藏的 SQL 不能为空", "en": "The SQL of a favorite is required"},
+  "generate.needs_primary_key": {"zh-CN": "表 %{table} 没有主键，无法生成按主键定位的语句", "en": "Table %{table} has no primary key, so a statement that locates rows by primary key cannot be generated"},
+  "generate.no_relations": {"zh-CN": "表 %{table} 没有可以连接的外键", "en": "Table %{table} has no foreign keys to join"},
+  "generate.select_only": {"zh-CN": "视图只能生成 SELECT", "en": "Only SELECT can be generated for a view"},
   "internal.lock_poisoned": {"zh-CN": "内部锁已损坏（%{lock}）", "en": "Internal lock poisoned (%{lock})"},
   "internal.task_failed": {"zh-CN": "后台任务异常退出（%{task}）：%{error}", "en": "Background task failed (%{task}): %{error}"},
   "literal.bad_number": {"zh-CN": "不是有效的数字文本，无法生成字面量：%{value}", "en": "Not a valid number, cannot make a literal: %{value}"},
@@ -80,6 +83,7 @@
   "native.oracle_client": {"zh-CN": "Oracle 客户端库", "en": "Oracle client library"},
   "noun.favorite": {"zh-CN": "收藏 %{id}", "en": "favorite %{id}"},
   "noun.history": {"zh-CN": "历史记录 %{id}", "en": "history entry %{id}"},
+  "noun.object": {"zh-CN": "%{kind} %{name}", "en": "%{kind} %{name}"},
   "noun.profile": {"zh-CN": "连接配置 %{id}", "en": "connection profile %{id}"},
   "noun.session": {"zh-CN": "会话 %{id}", "en": "session %{id}"},
   "oracle.bad_chars": {"zh-CN": "Oracle %{what}含有不允许的字符：%{value}", "en": "The Oracle %{what} contains characters that are not allowed: %{value}"},
```

- [ ] **Step 3: 修改 `crates/dblens-dialect/src/ddl.rs`**

```diff
--- a/crates/dblens-dialect/src/ddl.rs
+++ b/crates/dblens-dialect/src/ddl.rs
@@ -121,8 +121,14 @@ impl Dialect {
         self.literal(&Value::String(text.into()))
     }
 
-    /// PG 系的建表语句：列、约束、分区，之后是索引、触发器和注释，每条以 `;` 结束。
+    /// PG 系的建表语句：列、约束、分区，之后是索引、触发器和注释（`table_extras`），每条以 `;` 结束。
     pub fn table_ddl(&self, schema: &str, t: &Table) -> Result<String> {
+        let mut out = self.table_create(schema, t);
+        out += &self.table_extras(schema, t, true)?;
+        Ok(out.trim_end().to_string() + "\n")
+    }
+
+    fn table_create(&self, schema: &str, t: &Table) -> String {
         let q = self.qualified(Some(schema), &t.name);
         let list = |cols: &[String]| cols.iter().map(|c| self.ident(c)).collect::<Vec<_>>().join(", ");
         let mut items: Vec<String> = Vec::new();
@@ -173,7 +179,14 @@ impl Dialect {
         if let Some(p) = &t.partition {
             out += &format!(" PARTITION BY {p}");
         }
-        out += ";\n";
+        out + ";\n"
+    }
+
+    /// 建表语句之后的部分：不属于约束的索引、触发器（`triggers` 为 true 时）、表和列的注释，每条一行、以 `;` 结束。
+    /// Oracle、达梦的 `DBMS_METADATA` 取表时不含索引和注释，也用这里补上。
+    pub fn table_extras(&self, schema: &str, t: &Table, triggers: bool) -> Result<String> {
+        let q = self.qualified(Some(schema), &t.name);
+        let mut out = String::new();
         for i in &t.indexes {
             let cols: Vec<String> = i
                 .columns
@@ -191,7 +204,8 @@ impl Dialect {
             let using = i
                 .method
                 .as_deref()
-                .filter(|m| !m.eq_ignore_ascii_case("btree"))
+                // Oracle、达梦的 NORMAL / BITMAP 等不是 USING 子句
+                .filter(|m| self.family() == Family::Postgres && !m.eq_ignore_ascii_case("btree"))
                 .map(|m| format!(" USING {}", m.to_ascii_lowercase()))
                 .unwrap_or_default();
             out += &format!(
@@ -201,7 +215,7 @@ impl Dialect {
                 cols.join(", ")
             );
         }
-        for tr in &t.triggers {
+        for tr in t.triggers.iter().filter(|_| triggers) {
             if let Some(d) = &tr.definition {
                 out += &format!("\n{};", d.trim_end_matches(';').trim());
             }
@@ -218,7 +232,7 @@ impl Dialect {
                 );
             }
         }
-        Ok(out.trim_end().to_string() + "\n")
+        Ok(out)
     }
 
     /// PG 系的视图 / 物化视图：`CREATE [MATERIALIZED] VIEW … AS 定义;`，有注释时附上。
@@ -405,6 +419,34 @@ COMMENT ON COLUMN public.\"Orders\".code IS '编码 ''A''';
         );
     }
 
+    #[test]
+    fn extras_without_triggers() {
+        let mut c = col("ID", "NUMBER");
+        c.comment = Some("编号".into());
+        let t = Table {
+            name: "T".into(),
+            comment: Some("表".into()),
+            columns: vec![c],
+            indexes: vec![Index {
+                name: "T_IDX".into(),
+                columns: vec![IndexColumn { name: "ID".into(), descending: false }],
+                unique: false,
+                method: Some("NORMAL".into()),
+            }],
+            triggers: vec![Trigger {
+                name: "TRG".into(),
+                timing: None,
+                events: vec![],
+                definition: Some("x".into()),
+            }],
+            ..Default::default()
+        };
+        assert_eq!(
+            Dialect::new(Variant::Oracle).table_extras("HR", &t, false).unwrap(),
+            "\nCREATE INDEX T_IDX ON HR.T (ID);\nCOMMENT ON TABLE HR.T IS '表';\nCOMMENT ON COLUMN HR.T.ID IS '编号';"
+        );
+    }
+
     #[test]
     fn pg_partitioned_table_and_same_schema_reference() {
         let t = Table {
```

- [ ] **Step 4: 修改 `crates/dblens-service/src/lib.rs`**

```diff
--- a/crates/dblens-service/src/lib.rs
+++ b/crates/dblens-service/src/lib.rs
@@ -6,6 +6,7 @@ pub mod fake;
 mod favorite;
 mod history;
 mod metadata;
+mod objects;
 mod policy;
 mod profile;
 mod registry;
@@ -20,6 +21,7 @@ pub use history::{
     HistoryQuery, NewHistory,
 };
 pub use metadata::{METADATA_POOL_SIZE, MetadataCache, MetadataService};
+pub use objects::{DataOrder, GenerateKind, ObjectKind};
 pub use policy::{
     Decision, DefaultPolicy, ExecContext, ObjectRef, Outcome, Policy, StatementInfo, StatementKind, classify,
 };
```

- [ ] **Step 5: 修改 `crates/dblens-service/src/metadata.rs`**

```diff
--- a/crates/dblens-service/src/metadata.rs
+++ b/crates/dblens-service/src/metadata.rs
@@ -7,7 +7,7 @@ use std::sync::{Arc, Mutex};
 use dblens_core::schema::{Routine, Sequence, Synonym, Table, View};
 use dblens_core::{Error, Result, msg};
 use dblens_dialect::Dialect;
-use dblens_dialect::meta::{self, MetaArgs, MetaKey, MetaRows, ObjectInfo, SchemaInfo, TableRows};
+use dblens_dialect::meta::{self, MetaArgs, MetaKey, MetaQuery, MetaRows, ObjectInfo, SchemaInfo, TableRows};
 use dblens_driver::{Connection, collect_all};
 use tokio::sync::Semaphore;
 
@@ -135,6 +135,17 @@ impl MetadataService {
         profile: &Profile,
         key: MetaKey,
         args: &MetaArgs,
+    ) -> Result<Option<Arc<MetaRows>>> {
+        self.run(profile, key, args, |d| d.meta_query(key, args)).await
+    }
+
+    /// 执行 `build` 生成的查询，按（连接配置、`key`、`args`）缓存。`build` 返回 None 时不执行，返回 None。
+    pub(crate) async fn run(
+        &self,
+        profile: &Profile,
+        key: MetaKey,
+        args: &MetaArgs,
+        build: impl FnOnce(&Dialect) -> Result<Option<MetaQuery>>,
     ) -> Result<Option<Arc<MetaRows>>> {
         let cache_key = (profile.id.clone(), key, args.clone());
         if let Some(rows) = self.cache.get(&cache_key) {
@@ -155,7 +166,7 @@ impl MetadataService {
                 self.registry.connect(profile, password).await?
             }
         };
-        let Some(q) = dialect.meta_query(key, args)? else {
+        let Some(q) = build(&dialect)? else {
             pool.idle
                 .lock()
                 .map_err(|_| Error::internal(msg!("internal.lock_poisoned", lock = "pool")))?
```

- [ ] **Step 6: 新建 `crates/dblens-service/src/objects.rs`**

```rust
//! 对象操作（M1c2 设计文档第 3–5 节）：查看 DDL、生成 SQL、外键关系、数据标签的 SQL。
//! 取数都经元数据连接池和缓存；SQL 的拼装在 `dblens-dialect`。

use dblens_core::schema::Table;
use dblens_core::{Error, Result, msg};
use dblens_dialect::meta::{MetaArgs, MetaKey};
use dblens_dialect::{DdlKind, Family, Relation, ddl_text, normalize_ddl, relations};
use serde::{Deserialize, Serialize};

use crate::metadata::MetadataService;
use crate::profile::Profile;

/// 生成 SQL 的种类。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GenerateKind {
    Select,
    Insert,
    Update,
    Delete,
    Join,
}

/// 生成 SQL 的对象种类：视图和物化视图只能生成 SELECT。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObjectKind {
    #[default]
    Table,
    View,
    MaterializedView,
}

/// 数据标签的排序：一列，升序或降序。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DataOrder {
    pub column: String,
    #[serde(default)]
    pub desc: bool,
}

fn not_found(kind: &str, schema: &str, name: &str) -> Error {
    Error::not_found(msg!("noun.object", kind = kind, name = format!("{schema}.{name}")))
}

/// 程序源码查询（`MetaKey::RoutineSource`）用的种类名。
fn routine_kind(kind: DdlKind) -> Option<&'static str> {
    match kind {
        DdlKind::Function => Some("FUNCTION"),
        DdlKind::Procedure => Some("PROCEDURE"),
        DdlKind::Package => Some("PACKAGE"),
        DdlKind::PackageBody => Some("PACKAGE BODY"),
        _ => None,
    }
}

impl MetadataService {
    /// 对象的 DDL（见 `Dialect::ddl_query`）。
    pub async fn ddl(&self, profile: &Profile, schema: &str, kind: DdlKind, name: &str) -> Result<String> {
        let dialect = self.dialect(profile).await?;
        if let Some(q) = dialect.ddl_query(kind, schema, name)? {
            let args = MetaArgs {
                schema: Some(schema.into()),
                name: Some(name.into()),
                kind: Some(kind.as_str().into()),
                ..Default::default()
            };
            let rows = self.run(profile, MetaKey::Ddl, &args, |_| Ok(Some(q))).await?.unwrap_or_default();
            let mut text = ddl_text(&rows).ok_or_else(|| not_found(kind.as_str(), schema, name))?;
            // DBMS_METADATA 取表时不含索引和注释，用结构模型补上
            if kind == DdlKind::Table && matches!(dialect.family(), Family::Oracle | Family::Dameng) {
                let t = self.table(profile, schema, name).await?;
                let extras = dialect.table_extras(schema, &t, false)?;
                if !extras.is_empty() {
                    text = text.trim_end().to_string();
                    if !text.ends_with(';') {
                        text.push(';');
                    }
                    text += &extras;
                    text.push('\n');
                }
            }
            return Ok(text);
        }
        if let Some(rk) = routine_kind(kind) {
            let text = self.source(profile, schema, MetaKey::RoutineSource, name, Some(rk)).await?;
            return text.map(|t| normalize_ddl(&t)).ok_or_else(|| not_found(kind.as_str(), schema, name));
        }
        // PG 系：用结构模型拼装
        match kind {
            DdlKind::Table => {
                let t = self.table(profile, schema, name).await?;
                dialect.table_ddl(schema, &t)
            }
            DdlKind::View | DdlKind::MaterializedView => {
                let key = if kind == DdlKind::View { MetaKey::Views } else { MetaKey::MaterializedViews };
                let v = self
                    .views(profile, schema, key)
                    .await?
                    .into_iter()
                    .find(|v| v.name == name)
                    .ok_or_else(|| not_found(kind.as_str(), schema, name))?;
                let definition = match &v.definition {
                    Some(d) => d.clone(),
                    None => self
                        .source(profile, schema, MetaKey::ViewDefinition, name, None)
                        .await?
                        .unwrap_or_default(),
                };
                dialect.view_ddl(schema, &v, &definition, kind == DdlKind::MaterializedView)
            }
            DdlKind::Sequence => {
                let s = self
                    .sequences(profile, schema)
                    .await?
                    .into_iter()
                    .find(|s| s.name == name)
                    .ok_or_else(|| not_found(kind.as_str(), schema, name))?;
                Ok(dialect.sequence_ddl(schema, &s))
            }
            _ => Err(Error::invalid(msg!("ddl.unsupported", kind = kind.as_str()))),
        }
    }

    /// 一张表的明细；不存在时报 `not_found`。
    async fn table(&self, profile: &Profile, schema: &str, name: &str) -> Result<Table> {
        self.tables(profile, schema, Some(name))
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| not_found("table", schema, name))
    }

    /// 表、视图或物化视图（当作只有列的表）。
    async fn object(&self, profile: &Profile, schema: &str, object: &str, kind: ObjectKind) -> Result<Table> {
        let key = match kind {
            ObjectKind::Table => return self.table(profile, schema, object).await,
            ObjectKind::View => MetaKey::Views,
            ObjectKind::MaterializedView => MetaKey::MaterializedViews,
        };
        let v = self
            .views(profile, schema, key)
            .await?
            .into_iter()
            .find(|v| v.name == object)
            .ok_or_else(|| not_found("view", schema, object))?;
        Ok(Table { name: v.name, comment: v.comment, columns: v.columns, ..Default::default() })
    }

    /// 表 `table` 的外键关系：它引用的表，以及同一 schema 中引用它的表。
    pub async fn relations(&self, profile: &Profile, schema: &str, table: &str) -> Result<Vec<Relation>> {
        let t = self.table(profile, schema, table).await?;
        let all = self.tables(profile, schema, None).await?;
        Ok(relations(schema, &t, &all))
    }

    /// 生成 SQL。`reference` 只用于关联查询：给出时只连接与这张表相关的外键，否则连接本表引用的全部表。
    pub async fn generate(
        &self,
        profile: &Profile,
        schema: &str,
        object: &str,
        object_kind: ObjectKind,
        kind: GenerateKind,
        reference: Option<(&str, &str)>,
    ) -> Result<String> {
        if object_kind != ObjectKind::Table && kind != GenerateKind::Select {
            return Err(Error::invalid(msg!("generate.select_only")));
        }
        let dialect = self.dialect(profile).await?;
        let t = self.object(profile, schema, object, object_kind).await?;
        let needs_pk = || Error::invalid(msg!("generate.needs_primary_key", table = object));
        match kind {
            GenerateKind::Select => Ok(dialect.select_sql(Some(schema), &t)),
            GenerateKind::Insert => Ok(dialect.insert_sql(Some(schema), &t).sql),
            GenerateKind::Update => dialect.update_sql(Some(schema), &t).map(|d| d.sql).ok_or_else(needs_pk),
            GenerateKind::Delete => dialect.delete_sql(Some(schema), &t).map(|d| d.sql).ok_or_else(needs_pk),
            GenerateKind::Join => {
                let all = self.relations(profile, schema, object).await?;
                let picked: Vec<Relation> = match reference {
                    Some((rs, rt)) => all.into_iter().filter(|r| r.schema == rs && r.table == rt).collect(),
                    None => {
                        all.into_iter().filter(|r| r.direction == dblens_dialect::Direction::Out).collect()
                    }
                };
                if picked.is_empty() {
                    return Err(Error::invalid(msg!("generate.no_relations", table = object)));
                }
                Ok(dialect.join_sql(schema, object, &picked))
            }
        }
    }

    /// 数据标签的 SQL（见 `Dialect::data_sql`）。
    pub async fn data_sql(
        &self,
        profile: &Profile,
        schema: &str,
        object: &str,
        filter: Option<&str>,
        order: Option<&DataOrder>,
        count: bool,
    ) -> Result<String> {
        let dialect = self.dialect(profile).await?;
        Ok(dialect.data_sql(schema, object, filter, order.map(|o| (o.column.as_str(), o.desc)), count))
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

实库测试：在上面的 `cargo test` 前以内联环境变量给出 `DBLENS_TEST_MYSQL_URL`、`DBLENS_TEST_MARIADB_URL`、`DBLENS_TEST_PG_URL`、`DBLENS_TEST_ORACLE_URL`、`DBLENS_TEST_DM_URL`（密码不写进任何文件）。

Expected: 全部测试通过；`dblens-service` 的 `mysql_objects`、`mariadb_objects`、`postgres_objects`、`oracle_objects`、`dameng_objects` 在 5 个实库上通过。

- [ ] **Step 8: 提交**

```bash
git add crates/dblens-core/locales/app.json crates/dblens-dialect/src/ddl.rs crates/dblens-service/src/lib.rs crates/dblens-service/src/metadata.rs crates/dblens-service/src/objects.rs crates/dblens-service/tests/live.rs
git commit -m "feat(service): 对象操作——查看 DDL、生成 SQL、外键关系、数据标签的 SQL" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 4: 接口

新增 `…/ddl`、`…/generate`、`…/relations`、`…/data-sql` 四个接口，接口说明补充。实库测试检查路由、参数和状态码（取数和拼装已在服务层测过）；接口测试的 `call` 在响应不是 JSON 时（axum 的参数解析错误是纯文本）返回原文。

**Files:**
- Modify: `crates/dblens-api/src/lib.rs`
- Modify: `crates/dblens-api/src/routes.rs`
- Modify: `crates/dblens-api/tests/live.rs`
- Modify: `docs/notes/m0-4b-api.md`

**Interfaces:**
- Consumes：任务 3 的服务层方法。
- Produces（HTTP，都在 `/api/profiles/{id}/schemas/{schema}` 下）：
  - `GET /ddl?kind=&name=` → `{"ddl": "…"}`；
  - `GET /generate?object=&object_kind=&kind=&ref_schema=&ref_table=` → `{"sql": "…"}`；
  - `GET /relations?table=` → `[Relation]`；
  - `POST /data-sql`，`{object, filter?, order?: {column, desc}, count?}` → `{"sql": "…"}`（空白的 filter 视为不过滤）。

- [ ] **Step 1: 修改 `crates/dblens-api/tests/live.rs`**

```diff
--- a/crates/dblens-api/tests/live.rs
+++ b/crates/dblens-api/tests/live.rs
@@ -32,7 +32,14 @@ async fn call(app: &Router, method: &str, uri: &str, body: Option<Value>) -> (St
     let resp = app.clone().oneshot(req).await.unwrap();
     let status = resp.status();
     let bytes = resp.into_body().collect().await.unwrap().to_bytes();
-    (status, if bytes.is_empty() { Value::Null } else { serde_json::from_slice(&bytes).unwrap() })
+    // 参数解析失败时 axum 返回纯文本
+    let body = if bytes.is_empty() {
+        Value::Null
+    } else {
+        serde_json::from_slice(&bytes)
+            .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&bytes).into()))
+    };
+    (status, body)
 }
 
 async fn flow(env: &str) {
@@ -92,6 +99,35 @@ async fn flow(env: &str) {
     assert_eq!(st, StatusCode::CREATED, "{s}");
     let sid = s["id"].as_u64().unwrap();
 
+    // 对象操作（M1c2）：取数和拼装在服务层的实库测试中覆盖，这里只看路由、参数和状态码
+    let schema = s["schema"].as_str().unwrap();
+    let base = format!("/api/profiles/{id}/schemas/{schema}");
+    let (st, d) = call(
+        &app,
+        "POST",
+        &format!("{base}/data-sql"),
+        Some(json!({
+            "object": "c2_x", "filter": " ", "order": {"column": "id", "desc": true},
+        })),
+    )
+    .await;
+    assert_eq!(st, StatusCode::OK, "{d}");
+    let sql = d["sql"].as_str().unwrap().to_lowercase();
+    assert!(sql.contains("c2_x") && sql.contains("desc") && !sql.contains("where"), "{sql}");
+    let (st, d) =
+        call(&app, "POST", &format!("{base}/data-sql"), Some(json!({"object": "c2_x", "count": true}))).await;
+    assert!(st == StatusCode::OK && d["sql"].as_str().unwrap().to_lowercase().contains("count(*)"), "{d}");
+    // 不存在的表：PG 系由结构模型拼装时 404；其余由数据库报错（422）
+    let (st, e) = call(&app, "GET", &format!("{base}/ddl?kind=table&name=c2_no_such"), None).await;
+    assert!(st.is_client_error(), "{st} {e}");
+    let (st, e) = call(&app, "GET", &format!("{base}/relations?table=c2_no_such"), None).await;
+    assert_eq!(st, StatusCode::NOT_FOUND, "{e}");
+    let (st, e) =
+        call(&app, "GET", &format!("{base}/generate?object=c2_v&object_kind=view&kind=insert"), None).await;
+    assert_eq!(st, StatusCode::BAD_REQUEST, "{e}");
+    let (st, _) = call(&app, "GET", &format!("{base}/ddl?kind=bogus&name=x"), None).await;
+    assert_eq!(st, StatusCode::BAD_REQUEST);
+
     // WebSocket：900 行，每批 400 行
     let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
     let addr = listener.local_addr().unwrap();
```

- [ ] **Step 2: 修改 `crates/dblens-api/src/lib.rs`**

```diff
--- a/crates/dblens-api/src/lib.rs
+++ b/crates/dblens-api/src/lib.rs
@@ -72,6 +72,10 @@ pub fn router(service: Arc<Service>, config: ApiConfig) -> Router {
         .route("/profiles/{id}/schemas/{schema}/routines", get(routes::routines))
         .route("/profiles/{id}/schemas/{schema}/synonyms", get(routes::synonyms))
         .route("/profiles/{id}/schemas/{schema}/source", get(routes::source))
+        .route("/profiles/{id}/schemas/{schema}/ddl", get(routes::ddl))
+        .route("/profiles/{id}/schemas/{schema}/generate", get(routes::generate))
+        .route("/profiles/{id}/schemas/{schema}/relations", get(routes::relations))
+        .route("/profiles/{id}/schemas/{schema}/data-sql", post(routes::data_sql))
         .route("/sessions", get(routes::sessions).post(routes::open_session))
         .route("/sessions/{id}", get(routes::session).delete(routes::close_session))
         .route("/sessions/{id}/autocommit", post(routes::set_autocommit))
```

- [ ] **Step 3: 修改 `crates/dblens-api/src/routes.rs`**

```diff
--- a/crates/dblens-api/src/routes.rs
+++ b/crates/dblens-api/src/routes.rs
@@ -9,12 +9,14 @@ use axum::http::StatusCode;
 use dblens_core::schema::{Routine, Sequence, Synonym, Table, View};
 use dblens_core::{Error, msg};
 use dblens_dialect::meta::{MetaKey, ObjectInfo, SchemaInfo};
+use dblens_dialect::{DdlKind, Relation};
 use dblens_driver::DriverId;
 use dblens_service::{
-    DriverStatus, Favorite, FavoritePatch, HistoryItem, HistoryQuery, NewFavorite, Profile, ProfileInput,
-    SecretStatus, ServerSummary, SessionId, SessionInfo,
+    DataOrder, DriverStatus, Favorite, FavoritePatch, GenerateKind, HistoryItem, HistoryQuery, NewFavorite,
+    ObjectKind, Profile, ProfileInput, SecretStatus, ServerSummary, SessionId, SessionInfo,
 };
 use serde::Deserialize;
+use serde_json::{Value, json};
 
 use crate::AppState;
 use crate::error::ApiResult;
@@ -344,6 +346,85 @@ pub struct RefreshBody {
     pub schema: Option<String>,
 }
 
+// ---- 对象操作（M1c2） ----
+
+#[derive(Deserialize)]
+pub struct DdlQuery {
+    /// table / view / materialized_view / sequence / function / procedure / package / package_body / synonym。
+    pub kind: DdlKind,
+    pub name: String,
+}
+
+pub async fn ddl(
+    State(s): State<AppState>,
+    Path((id, schema)): Path<(String, String)>,
+    Query(q): Query<DdlQuery>,
+) -> ApiResult<Json<Value>> {
+    let p = s.service.profile(&id)?;
+    let ddl = s.service.metadata.ddl(&p, &schema, q.kind, &q.name).await?;
+    Ok(Json(json!({ "ddl": ddl })))
+}
+
+#[derive(Deserialize)]
+pub struct GenerateQuery {
+    pub object: String,
+    /// table / view / materialized_view，默认 table。
+    #[serde(default)]
+    pub object_kind: ObjectKind,
+    /// select / insert / update / delete / join。
+    pub kind: GenerateKind,
+    /// 关联查询只连接这张表；两个都不给时连接本表引用的全部表。
+    pub ref_schema: Option<String>,
+    pub ref_table: Option<String>,
+}
+
+pub async fn generate(
+    State(s): State<AppState>,
+    Path((id, schema)): Path<(String, String)>,
+    Query(q): Query<GenerateQuery>,
+) -> ApiResult<Json<Value>> {
+    let p = s.service.profile(&id)?;
+    let reference = q.ref_table.as_deref().map(|t| (q.ref_schema.as_deref().unwrap_or(&schema), t));
+    let sql = s.service.metadata.generate(&p, &schema, &q.object, q.object_kind, q.kind, reference).await?;
+    Ok(Json(json!({ "sql": sql })))
+}
+
+#[derive(Deserialize)]
+pub struct RelationsQuery {
+    pub table: String,
+}
+
+pub async fn relations(
+    State(s): State<AppState>,
+    Path((id, schema)): Path<(String, String)>,
+    Query(q): Query<RelationsQuery>,
+) -> ApiResult<Json<Vec<Relation>>> {
+    let p = s.service.profile(&id)?;
+    Ok(Json(s.service.metadata.relations(&p, &schema, &q.table).await?))
+}
+
+#[derive(Deserialize)]
+pub struct DataSqlBody {
+    pub object: String,
+    /// WHERE 后的条件，原样拼入。
+    pub filter: Option<String>,
+    pub order: Option<DataOrder>,
+    /// true 时生成 `SELECT COUNT(*)`。
+    #[serde(default)]
+    pub count: bool,
+}
+
+pub async fn data_sql(
+    State(s): State<AppState>,
+    Path((id, schema)): Path<(String, String)>,
+    Json(b): Json<DataSqlBody>,
+) -> ApiResult<Json<Value>> {
+    let p = s.service.profile(&id)?;
+    let filter = b.filter.as_deref().map(str::trim).filter(|f| !f.is_empty());
+    let sql = s.service.metadata.data_sql(&p, &schema, &b.object, filter, b.order.as_ref(), b.count).await?;
+    Ok(Json(json!({ "sql": sql })))
+}
+
 /// 手动刷新：让元数据缓存失效。
 pub async fn refresh(
     State(s): State<AppState>,
```

- [ ] **Step 4: 修改 `docs/notes/m0-4b-api.md`**

```diff
--- a/docs/notes/m0-4b-api.md
+++ b/docs/notes/m0-4b-api.md
@@ -94,6 +94,10 @@
 | GET | `…/schemas/{schema}/routines` | 存储过程、函数、包 |
 | GET | `…/schemas/{schema}/synonyms` | 同义词 |
 | GET | `…/schemas/{schema}/source?kind=view_definition\|routine_source&name=&routine_kind=` | 视图定义或程序源码 |
+| GET | `…/schemas/{schema}/ddl?kind=&name=` | 对象的 DDL，`{"ddl": "…"}`（M1c2 加入）。`kind`：table / view / materialized_view / sequence / function / procedure / package / package_body / synonym；该数据库没有这种对象时 400；对象不存在时 404，由数据库给出 DDL 的（MySQL 系、Oracle、达梦）则是数据库的错误（422） |
+| GET | `…/schemas/{schema}/generate?object=&object_kind=&kind=&ref_schema=&ref_table=` | 生成 SQL，`{"sql": "…"}`（M1c2 加入）。`kind`：select / insert / update / delete / join；`object_kind`：table（默认）/ view / materialized_view，视图只能 select；update、delete 需要主键；join 给出 `ref_table` 时只连接与它相关的外键，否则连接本表引用的全部表 |
+| GET | `…/schemas/{schema}/relations?table=` | 表的外键关系（M1c2 加入）：`[{direction: out\|in, name, schema, table, local_columns, other_columns}]`，`in` 只含同一 schema 中引用它的表 |
+| POST | `…/schemas/{schema}/data-sql` | 数据标签的 SQL（M1c2 加入），`{"object", "filter"?, "order"?: {"column", "desc"}, "count"?}` → `{"sql": "…"}`；`filter` 为空白时不加 WHERE，`count` 为 true 时生成 `SELECT COUNT(*)` |
 | POST | `/profiles/{id}/refresh` | 刷新元数据缓存，`{"schema": "…" \| null}` |
 
 ### 会话
```

- [ ] **Step 5: 检查**

```bash
export CARGO_INCREMENTAL=0
cargo fmt --all
cargo clippy --config "source.mirror.registry='sparse+https://rsproxy.cn/index/'" --workspace --all-targets -- -D warnings
DBLENS_LANG=en cargo test --config "source.mirror.registry='sparse+https://rsproxy.cn/index/'" --workspace
```

实库测试：在上面的 `cargo test` 前以内联环境变量给出 `DBLENS_TEST_MYSQL_URL`、`DBLENS_TEST_MARIADB_URL`、`DBLENS_TEST_PG_URL`、`DBLENS_TEST_ORACLE_URL`、`DBLENS_TEST_DM_URL`（密码不写进任何文件）。

Expected: 全部测试通过（dblens-api 的 5 个实库测试包含新接口的检查）。

- [ ] **Step 6: 提交**

```bash
git add crates/dblens-api/src/lib.rs crates/dblens-api/src/routes.rs crates/dblens-api/tests/live.rs docs/notes/m0-4b-api.md
git commit -m "feat(api): 对象操作接口——ddl、generate、relations、data-sql" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 5: 前端：查看 DDL 与生成 SQL

对象树的对象节点右键菜单增加"查看 DDL"和"生成 SQL"：
- 查看 DDL：弹出只读的 Monaco 预览（按连接的语言高亮），可复制、在新标签中打开。预览懒加载，先加载 Monaco 语言包（与工作区相同）。
- 生成 SQL：SELECT、INSERT、UPDATE、DELETE（视图只有 SELECT）；表另有"关联查询"子菜单（右键时取外键关系：全部外键、每个外键一项，标明方向；没有外键时置灰）。生成后插入到当前编辑器（沿用 M1c1 的 `insertSql`）。

菜单的组装是纯逻辑（`objectMenu.ts`），单独测试。

**Files:**
- Create: `web/src/features/tree/DdlDialog.tsx`
- Modify: `web/src/features/tree/ObjectTree.tsx`
- Create: `web/src/features/tree/objectMenu.test.ts`
- Create: `web/src/features/tree/objectMenu.ts`
- Modify: `web/src/locales/en/tree.json`
- Modify: `web/src/locales/zh-CN/tree.json`
- Modify: `web/src/shared/api.test.ts`
- Modify: `web/src/shared/api.ts`

**Interfaces:**
- Consumes：任务 4 的接口；M1c1 的 `useQueryActions().insertSql / openSql`。
- Produces：
  - `shared/api.ts`：`DdlKind`、`TargetKind = 'table' | 'view' | 'materialized_view'`、`GenerateKind`、`Relation`、`GenerateParams { object, objectKind, kind, ref? }`、`DataSqlParams { object, filter?, order?, count? }`；`api.ddl / generate / relations / dataSql`；
  - `features/tree/objectMenu.ts`：`targetKind(CategoryKind)`、`ddlKind(CategoryKind, routineKind?)`、`generateMenu(TargetKind, Relation[] | undefined)`、`generateChoice(key, relations)`；
  - `features/tree/DdlDialog.tsx`：`DdlTarget { profileId, driver, schema, kind, name }`；
  - 词条 `tree.menu.viewData / viewDdl / generate / join / joinAll / joinNone / loading`、`tree.ddl.*`。

- [ ] **Step 1: 新建 `web/src/features/tree/objectMenu.test.ts`**

```ts
import { describe, expect, it } from 'vitest'

import type { Relation } from '../../shared/api'
import { ddlKind, generateChoice, generateMenu, targetKind } from './objectMenu'

const rel = (direction: 'out' | 'in', table: string, cols: string[]): Relation => ({
  direction,
  name: null,
  schema: 's',
  table,
  local_columns: cols,
  other_columns: ['id'],
})

describe('对象的右键菜单', () => {
  it('种类', () => {
    expect(targetKind('materialized_views')).toBe('materialized_view')
    expect(targetKind('sequences')).toBeNull()
    expect(ddlKind('tables')).toBe('table')
    expect(ddlKind('routines', 'package_body')).toBe('package_body')
    expect(ddlKind('routines')).toBeNull()
    expect(ddlKind('synonyms')).toBe('synonym')
  })

  it('视图只能生成 SELECT', () => {
    expect(generateMenu('view', []).map((i) => i.key)).toEqual(['gen:select'])
  })

  it('关联查询：加载中、没有外键时置灰', () => {
    const loading = generateMenu('table', undefined).at(-1)!
    expect(loading.disabled).toBe(false)
    expect(loading.children).toEqual([{ key: 'gen:loading', label: '加载中…', disabled: true }])
    const none = generateMenu('table', []).at(-1)!
    expect(none.disabled).toBe(true)
  })

  it('关联查询：按外键列出两个方向；只有被引用时"全部外键"置灰', () => {
    const rels = [rel('in', 'order_items', ['order_id']), rel('in', 'refunds', ['order_id'])]
    const join = generateMenu('table', rels).at(-1)!
    expect(join.children!.map((c) => [c.label, c.disabled ?? false])).toEqual([
      ['全部外键', true],
      ['← order_items (order_id)', false],
      ['← refunds (order_id)', false],
    ])
    const out = generateMenu('table', [rel('out', 'users', ['user_id'])]).at(-1)!
    expect(out.children![0].disabled).toBe(false)
    expect(out.children![1].label).toBe('→ users (user_id)')
  })

  it('由菜单键取回生成的种类', () => {
    const rels = [rel('out', 'users', ['user_id'])]
    expect(generateChoice('gen:update', rels)).toEqual({ kind: 'update' })
    expect(generateChoice('gen:join', rels)).toEqual({ kind: 'join' })
    expect(generateChoice('gen:join:0', rels)).toEqual({ kind: 'join', relation: rels[0] })
    expect(generateChoice('gen:join:5', rels)).toBeNull()
    expect(generateChoice('refresh', rels)).toBeNull()
  })
})
```

- [ ] **Step 2: 修改 `web/src/shared/api.test.ts`**

```diff
--- a/web/src/shared/api.test.ts
+++ b/web/src/shared/api.test.ts
@@ -31,6 +31,29 @@ describe('createApi', () => {
     })
   })
 
+  it('对象操作的参数（M1c2）', async () => {
+    const { fn, calls } = fakeFetch(200, { sql: 'SELECT 1' })
+    const api = createApi('tok', fn)
+    await expect(
+      api.generate('p', 'my schema', {
+        object: 't',
+        objectKind: 'table',
+        kind: 'join',
+        ref: { schema: 'my schema', table: 'u' },
+      }),
+    ).resolves.toBe('SELECT 1')
+    expect(calls[0].url).toBe(
+      '/api/profiles/p/schemas/my%20schema/generate?object=t&object_kind=table&kind=join&ref_schema=my+schema&ref_table=u',
+    )
+    await api.dataSql('p', 's', { object: 't', filter: 'a > 1', order: { column: 'a', desc: true } })
+    expect(calls[1].url).toBe('/api/profiles/p/schemas/s/data-sql')
+    expect(JSON.parse(calls[1].init?.body as string)).toEqual({
+      object: 't',
+      filter: 'a > 1',
+      order: { column: 'a', desc: true },
+    })
+  })
+
   it('204 没有响应体', async () => {
     const { fn, calls } = fakeFetch(204, null)
     await expect(createApi('tok', fn).deleteProfile('a/b')).resolves.toBeNull()
```

- [ ] **Step 3: 新建 `web/src/features/tree/DdlDialog.tsx`**

```tsx
// 查看 DDL（M1c2 设计文档第 3 节）：只读的 Monaco 预览，按连接的语言高亮；可复制、在新标签中打开。
// 引入 Monaco，由对象树懒加载（见 ObjectTree 的 DdlDialog）。

import Editor from '@monaco-editor/react'
import { useQuery } from '@tanstack/react-query'
import { App, Button, Modal, Spin } from 'antd'
import { useTranslation } from 'react-i18next'

import { useApi } from '../../app/context'
import type { DdlKind, DriverId } from '../../shared/api'
import { useSetting } from '../../shared/settings'
import { THEME_KEY, parseThemePref, resolveTheme, useSystemDark } from '../../shared/theme'
import { useQueryActions } from '../editor/actions'
import { editorLanguage } from '../editor/dialect'
import '../editor/monaco'
import { withVault } from '../connections/vault'

export interface DdlTarget {
  profileId: string
  driver: DriverId
  schema: string
  kind: DdlKind
  name: string
}

export default function DdlDialog({ target, onClose }: { target: DdlTarget; onClose(): void }) {
  const api = useApi()
  const { t } = useTranslation()
  const { message } = App.useApp()
  const { openSql } = useQueryActions()
  const theme = resolveTheme(parseThemePref(useSetting(THEME_KEY)), useSystemDark())
  const { profileId, schema, kind, name } = target
  // 每次打开都重新取（缓存在后端），不在前端缓存
  const { data, error, isFetching } = useQuery({
    queryKey: ['ddl', profileId, schema, kind, name],
    queryFn: () => withVault(api, () => api.ddl(profileId, schema, kind, name)),
    gcTime: 0,
    retry: false,
  })

  return (
    <Modal
      open
      width={820}
      title={t('tree.ddl.title', { name: `${schema}.${name}` })}
      onCancel={onClose}
      footer={[
        <Button
          key="copy"
          disabled={!data}
          onClick={() =>
            void navigator.clipboard.writeText(data ?? '').then(() => message.success(t('tree.copied')))
          }
        >
          {t('tree.ddl.copy')}
        </Button>,
        <Button
          key="open"
          disabled={!data}
          onClick={() => {
            openSql(data ?? '', profileId)
            onClose()
          }}
        >
          {t('tree.ddl.openInTab')}
        </Button>,
        <Button key="close" type="primary" onClick={onClose}>
          {t('tree.ddl.close')}
        </Button>,
      ]}
    >
      <div style={{ height: 460, border: '1px solid var(--border)' }}>
        {isFetching ? (
          <Spin style={{ margin: 48 }} />
        ) : error ? (
          <div className="err" style={{ padding: 12, whiteSpace: 'pre-wrap' }}>
            {(error as Error).message}
          </div>
        ) : (
          <Editor
            language={editorLanguage(target.driver)}
            value={data ?? ''}
            theme={theme === 'dark' ? 'dblens-dark' : 'dblens-light'}
            options={{
              readOnly: true,
              domReadOnly: true,
              fontFamily: "'JetBrains Mono', 'Cascadia Mono', Consolas, 'Noto Sans Mono CJK SC', monospace",
              fontSize: 13,
              lineHeight: 20,
              minimap: { enabled: false },
              scrollBeyondLastLine: false,
              automaticLayout: true,
            }}
          />
        )}
      </div>
    </Modal>
  )
}
```

- [ ] **Step 4: 修改 `web/src/features/tree/ObjectTree.tsx`**

```diff
--- a/web/src/features/tree/ObjectTree.tsx
+++ b/web/src/features/tree/ObjectTree.tsx
@@ -1,6 +1,7 @@
 // 数据库对象树（设计文档 7.3 节；M1 设计稿第 1、3 部分）：所有连接在一棵树里，可以同时展开多个。
 // 懒加载：展开时向后端取下一层（后端有元数据缓存，"刷新"会先清后端缓存再重新取）。顶部过滤框按名称或注释过滤已加载的对象。
-// 右键菜单（M1a）：连接 —— 编辑、复制、删除、刷新；其他节点 —— 复制名称、刷新。"查看 DDL""生成 SQL"等在 M1c 加入。
+// 右键菜单（M1a）：连接 —— 编辑、复制、删除、刷新；其他节点 —— 复制名称、刷新。
+// 对象节点另有（M1c2）："查看 DDL"（弹出只读预览）、"生成 SQL"（插入到当前编辑器）。
 
 import { App, Dropdown, Input, type MenuProps, Tree } from 'antd'
 import type { DataNode, EventDataNode } from 'antd/es/tree'
@@ -17,7 +18,7 @@ import {
   IconTable,
 } from '@tabler/icons-react'
 import { useQuery, useQueryClient } from '@tanstack/react-query'
-import { type Key, type ReactNode, useCallback, useMemo, useState } from 'react'
+import { type Key, type ReactNode, Suspense, lazy, useCallback, useMemo, useState } from 'react'
 import { useTranslation } from 'react-i18next'
 
 import { useApi } from '../../app/context'
@@ -29,6 +30,7 @@ import { driverLabel } from '../connections/form'
 import { variantName } from '../editor/dialect'
 import { withVault } from '../connections/vault'
 import { metaKey } from '../sql/metadata'
+import type { DdlTarget } from './DdlDialog'
 import {
   type CategoryKind,
   type NodeKey,
@@ -41,6 +43,14 @@ import {
   qualifiedName,
   routineKindLabel,
 } from './nodes'
+import { ddlKind, generateChoice, generateMenu, targetKind } from './objectMenu'
+
+// 预览用 Monaco：和工作区一样先加载它的语言包，第一次查看 DDL 时才加载
+const DdlDialog = lazy(async () => {
+  const { loadMonacoLocale } = await import('../editor/monacoLocale')
+  await loadMonacoLocale()
+  return import('./DdlDialog')
+})
 
 /**
  * 已加载的一层子节点。`items` 中的 key 为编码后的节点键。
@@ -143,12 +153,30 @@ export default function ObjectTree() {
   const { t } = useTranslation()
   const { message, modal } = App.useApp()
   const { openConnection, selectProfile } = useUi()
-  const { newQuery } = useQueryActions()
+  const { newQuery, insertSql } = useQueryActions()
   const { data: profiles = [], error: profilesError } = useQuery({ queryKey: ['profiles'], queryFn: () => api.profiles() })
   const [loaded, setLoaded] = useState<Record<string, Loaded>>({})
   const [expanded, setExpanded] = useState<Key[]>([])
   const [filter, setFilter] = useState('')
   const [menuNode, setMenuNode] = useState<NodeKey | null>(null)
+  const [ddl, setDdl] = useState<DdlTarget | null>(null)
+
+  // 右键表时取它的外键关系，组装"关联查询"子菜单；取不到时当作没有外键
+  const relTable = menuNode?.type === 'o' && menuNode.kind === 'tables' ? menuNode : null
+  const { data: relationsData, error: relationsError } = useQuery({
+    queryKey: ['relations', relTable?.profileId, relTable?.schema, relTable?.name],
+    queryFn: () => api.relations(relTable!.profileId, relTable!.schema, relTable!.name),
+    enabled: !!relTable,
+    retry: false,
+    staleTime: 30_000,
+  })
+  const relations = relationsError ? [] : relationsData
+
+  /** 对象节点的程序类型（函数、过程、包、包体），从已加载的上一层取。 */
+  const routineKindOf = (k: Extract<NodeKey, { type: 'o' }>) =>
+    loaded[encodeKey({ type: 'k', profileId: k.profileId, schema: k.schema, kind: k.kind })]?.items.find(
+      (i) => i.key === encodeKey(k),
+    )?.routineKind
 
   const load = useCallback(
     async (key: string) => {
@@ -248,16 +276,44 @@ export default function ObjectTree() {
         ]
       : menuNode
         ? [
+            ...objectItems(menuNode),
             ...(qualifiedName(menuNode) ? [{ key: 'copy', label: t('tree.menu.copyName') }] : []),
             { key: 'refresh', label: t('tree.menu.refresh') },
           ]
         : []
 
+  /** 对象节点的查看 DDL、生成 SQL，后面接分隔线；其他节点没有。 */
+  function objectItems(k: NodeKey): NonNullable<MenuProps['items']> {
+    if (k.type !== 'o') return []
+    const items: NonNullable<MenuProps['items']> = []
+    if (ddlKind(k.kind, routineKindOf(k))) items.push({ key: 'ddl', label: t('tree.menu.viewDdl') })
+    const target = targetKind(k.kind)
+    if (target) {
+      // generateMenu 的项是 antd 菜单项的子集
+      const children = generateMenu(target, relations) as NonNullable<MenuProps['items']>
+      items.push({ key: 'gen-menu', label: t('tree.menu.generate'), children })
+    }
+    return items.length ? [...items, { type: 'divider' }] : []
+  }
+
   const onMenu = async (key: string) => {
     const k = menuNode
     if (!k) return
     try {
-      if (key === 'new-query') newQuery(k.profileId)
+      const gen = k.type === 'o' ? generateChoice(key, relations) : null
+      if (gen && k.type === 'o') {
+        const sql = await api.generate(k.profileId, k.schema, {
+          object: k.name,
+          objectKind: targetKind(k.kind) ?? 'table',
+          kind: gen.kind,
+          ref: gen.relation && { schema: gen.relation.schema, table: gen.relation.table },
+        })
+        insertSql(sql, k.profileId)
+      } else if (key === 'ddl' && k.type === 'o') {
+        const kind = ddlKind(k.kind, routineKindOf(k))
+        const driver = profiles.find((p) => p.id === k.profileId)?.driver
+        if (kind && driver) setDdl({ profileId: k.profileId, driver, schema: k.schema, kind, name: k.name })
+      } else if (key === 'new-query') newQuery(k.profileId)
       else if (key === 'edit' && k.type === 'c') openConnection(k.profileId)
       else if (key === 'duplicate' && k.type === 'c') {
         const p = await api.profile(k.profileId)
@@ -322,6 +378,11 @@ export default function ObjectTree() {
           />
         </div>
       </Dropdown>
+      {ddl && (
+        <Suspense fallback={null}>
+          <DdlDialog target={ddl} onClose={() => setDdl(null)} />
+        </Suspense>
+      )}
     </div>
   )
 }
```

- [ ] **Step 5: 新建 `web/src/features/tree/objectMenu.ts`**

```ts
// 对象节点的右键菜单（M1c2 设计文档第 3–5 节）：查看数据、查看 DDL、生成 SQL。这里只放纯逻辑，单独测试。

import type { DdlKind, GenerateKind, Relation, RoutineInfo, TargetKind } from '../../shared/api'
import { t } from '../../shared/i18n'
import type { CategoryKind } from './nodes'

/** 表、视图、物化视图在生成 SQL 和数据标签中的种类；其他分类返回 null。 */
export function targetKind(kind: CategoryKind): TargetKind | null {
  switch (kind) {
    case 'tables':
      return 'table'
    case 'views':
      return 'view'
    case 'materialized_views':
      return 'materialized_view'
    default:
      return null
  }
}

/** 查看 DDL 用的种类。程序按它的类型（函数、过程、包、包体）。 */
export function ddlKind(kind: CategoryKind, routineKind?: RoutineInfo['kind']): DdlKind | null {
  switch (kind) {
    case 'tables':
    case 'views':
    case 'materialized_views':
      return targetKind(kind)
    case 'sequences':
      return 'sequence'
    case 'synonyms':
      return 'synonym'
    case 'routines':
      return routineKind ?? null
  }
}

/** 菜单项：与 antd Menu 的 items 同形（只用到的字段）。 */
export interface MenuItem {
  key: string
  label: string
  disabled?: boolean
  children?: MenuItem[]
}

/** 生成 SQL 菜单项的键：`gen:select`、`gen:join`（全部外键）、`gen:join:序号`（relations 中的一项）。 */
export type GenerateChoice = { kind: GenerateKind; relation?: Relation }

/**
 * "生成 SQL"子菜单。视图只有 SELECT；表另有 INSERT、UPDATE、DELETE 和"关联查询"：
 * - relations 为 undefined 表示外键还在加载；
 * - "全部外键"只连接本表引用的表，没有时置灰；
 * - 每个外键各一项，"→ 父表"为本表引用的，"← 子表"为引用本表的。
 */
export function generateMenu(kind: TargetKind, relations: Relation[] | undefined): MenuItem[] {
  const items: MenuItem[] = [{ key: 'gen:select', label: 'SELECT' }]
  if (kind !== 'table') return items
  items.push({ key: 'gen:insert', label: 'INSERT' }, { key: 'gen:update', label: 'UPDATE' }, { key: 'gen:delete', label: 'DELETE' })
  let children: MenuItem[]
  if (!relations) children = [{ key: 'gen:loading', label: t('tree.menu.loading'), disabled: true }]
  else if (!relations.length) children = [{ key: 'gen:none', label: t('tree.menu.joinNone'), disabled: true }]
  else {
    children = [
      {
        key: 'gen:join',
        label: t('tree.menu.joinAll'),
        disabled: !relations.some((r) => r.direction === 'out'),
      },
      ...relations.map((r, i) => ({
        key: `gen:join:${i}`,
        label: `${r.direction === 'out' ? '→' : '←'} ${r.table} (${r.local_columns.join(', ')})`,
      })),
    ]
  }
  items.push({ key: 'gen:join-menu', label: t('tree.menu.join'), disabled: relations?.length === 0, children })
  return items
}

/** 由菜单项的键取回要生成的种类；不是生成 SQL 的项时返回 null。 */
export function generateChoice(key: string, relations: Relation[] | undefined): GenerateChoice | null {
  const m = /^gen:(select|insert|update|delete|join)(?::(\d+))?$/.exec(key)
  if (!m) return null
  const kind = m[1] as GenerateKind
  if (m[2] === undefined) return { kind }
  const relation = relations?.[Number(m[2])]
  return relation ? { kind, relation } : null
}
```

- [ ] **Step 6: 修改 `web/src/locales/en/tree.json`**

```diff
--- a/web/src/locales/en/tree.json
+++ b/web/src/locales/en/tree.json
@@ -28,7 +28,14 @@
     "duplicateConnection": "Duplicate Connection",
     "deleteConnection": "Delete Connection",
     "copyName": "Copy Name",
-    "refresh": "Refresh"
+    "refresh": "Refresh",
+    "viewData": "View Data",
+    "viewDdl": "View DDL",
+    "generate": "Generate SQL",
+    "join": "Join Query",
+    "joinAll": "All Foreign Keys",
+    "joinNone": "No foreign keys",
+    "loading": "Loading…"
   },
   "duplicate": {
     "name": "{{name}} copy",
@@ -42,5 +49,11 @@
   "copied": "Copied",
   "error": {
     "badKey": "Unrecognized node key: {{key}}"
+  },
+  "ddl": {
+    "title": "DDL of {{name}}",
+    "copy": "Copy",
+    "openInTab": "Open in New Tab",
+    "close": "Close"
   }
 }
```

- [ ] **Step 7: 修改 `web/src/locales/zh-CN/tree.json`**

```diff
--- a/web/src/locales/zh-CN/tree.json
+++ b/web/src/locales/zh-CN/tree.json
@@ -28,7 +28,14 @@
     "duplicateConnection": "复制连接",
     "deleteConnection": "删除连接",
     "copyName": "复制名称",
-    "refresh": "刷新"
+    "refresh": "刷新",
+    "viewData": "查看数据",
+    "viewDdl": "查看 DDL",
+    "generate": "生成 SQL",
+    "join": "关联查询",
+    "joinAll": "全部外键",
+    "joinNone": "没有外键",
+    "loading": "加载中…"
   },
   "duplicate": {
     "name": "{{name}} 副本",
@@ -42,5 +49,11 @@
   "copied": "已复制",
   "error": {
     "badKey": "无法识别的节点键：{{key}}"
+  },
+  "ddl": {
+    "title": "{{name}} 的 DDL",
+    "copy": "复制",
+    "openInTab": "在新标签中打开",
+    "close": "关闭"
   }
 }
```

- [ ] **Step 8: 修改 `web/src/shared/api.ts`**

```diff
--- a/web/src/shared/api.ts
+++ b/web/src/shared/api.ts
@@ -149,6 +149,48 @@ export interface TableInfo {
   primary_key: { name: string | null; columns: string[] } | null
 }
 
+/** 查看 DDL 的对象种类（M1c2）。 */
+export type DdlKind =
+  | 'table'
+  | 'view'
+  | 'materialized_view'
+  | 'sequence'
+  | 'function'
+  | 'procedure'
+  | 'package'
+  | 'package_body'
+  | 'synonym'
+
+/** 生成 SQL、数据标签的对象种类：视图和物化视图只能生成 SELECT。 */
+export type TargetKind = 'table' | 'view' | 'materialized_view'
+
+export type GenerateKind = 'select' | 'insert' | 'update' | 'delete' | 'join'
+
+/** 表的一个外键关系：out 为本表引用的表，in 为引用本表的表（同一 schema）。 */
+export interface Relation {
+  direction: 'out' | 'in'
+  name: string | null
+  schema: string
+  table: string
+  local_columns: string[]
+  other_columns: string[]
+}
+
+export interface GenerateParams {
+  object: string
+  objectKind: TargetKind
+  kind: GenerateKind
+  /** 关联查询只连接这张表；不给时连接本表引用的全部表。 */
+  ref?: { schema: string; table: string }
+}
+
+export interface DataSqlParams {
+  object: string
+  filter?: string
+  order?: { column: string; desc: boolean } | null
+  count?: boolean
+}
+
 export interface SequenceInfo {
   name: string
 }
@@ -283,6 +325,25 @@ export function createApi(token: string, fetchFn: FetchFn = (i, o) => fetch(i, o
     sequences: (id: string, schema: string) => request<SequenceInfo[]>('GET', `${s(id, schema)}/sequences`),
     routines: (id: string, schema: string) => request<RoutineInfo[]>('GET', `${s(id, schema)}/routines`),
     synonyms: (id: string, schema: string) => request<SynonymInfo[]>('GET', `${s(id, schema)}/synonyms`),
+    ddl: async (id: string, schema: string, kind: DdlKind, name: string) =>
+      (await request<{ ddl: string }>('GET', `${s(id, schema)}/ddl${q({ kind, name })}`)).ddl,
+    generate: async (id: string, schema: string, g: GenerateParams) =>
+      (
+        await request<{ sql: string }>(
+          'GET',
+          `${s(id, schema)}/generate${q({
+            object: g.object,
+            object_kind: g.objectKind,
+            kind: g.kind,
+            ref_schema: g.ref?.schema,
+            ref_table: g.ref?.table,
+          })}`,
+        )
+      ).sql,
+    relations: (id: string, schema: string, table: string) =>
+      request<Relation[]>('GET', `${s(id, schema)}/relations${q({ table })}`),
+    dataSql: async (id: string, schema: string, d: DataSqlParams) =>
+      (await request<{ sql: string }>('POST', `${s(id, schema)}/data-sql`, d)).sql,
     /** 清掉后端的元数据缓存；不给 schema 时清整个连接的。 */
     refresh: (id: string, schema: string | null) => request<null>('POST', `${p(id)}/refresh`, { schema }),
     history: (p: HistoryParams = {}) =>
```

- [ ] **Step 9: 检查**

在 PowerShell 中执行（bash 里 `cd web` 会触发坏掉的 fnm 钩子，`&&` 后面的命令会被跳过）：

```powershell
$env:PATH = "D:\Environments\nvm4w\nodejs;$env:PATH"; Set-Location web; pnpm typecheck; pnpm test; pnpm build
```

Expected: tsc 无输出；30 个测试文件、195 个测试通过；打包成功。

- [ ] **Step 10: 提交**

```bash
git add web/src/features/tree/DdlDialog.tsx web/src/features/tree/ObjectTree.tsx web/src/features/tree/objectMenu.test.ts web/src/features/tree/objectMenu.ts web/src/locales/en/tree.json web/src/locales/zh-CN/tree.json web/src/shared/api.test.ts web/src/shared/api.ts
git commit -m "feat(web): 对象树右键查看 DDL（只读预览）、生成 SQL（含按外键的关联查询）" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 6: 前端：数据标签

表、视图、物化视图可以打开只读的数据标签（右键"查看数据"或双击）：
- 标签：`QueryTab.data`（schema、对象、种类、过滤、排序），与查询标签一起保存、恢复；同一对象只开一个；标题为对象名，图标与查询标签不同。
- 工具栏：连接和 schema（只显示）、WHERE 过滤（回车应用，清空取消）、刷新、停止、统计行数（另开会话执行 COUNT(*)）。
- 表格沿用结果面板：首批 500 行、滚动续取、行数上限；点列头依次升序、降序、不排序，排序的列头带箭头。
- 当前是数据标签时，生成的 SQL 用新标签打开（`insertSql`）。

**Files:**
- Create: `web/src/features/data/DataView.tsx`
- Create: `web/src/features/data/count.ts`
- Create: `web/src/features/data/sort.test.ts`
- Create: `web/src/features/data/sort.ts`
- Modify: `web/src/features/editor/Workspace.tsx`
- Modify: `web/src/features/editor/actions.tsx`
- Modify: `web/src/features/editor/tabs.test.ts`
- Modify: `web/src/features/editor/tabs.ts`
- Modify: `web/src/features/results/ResultGrid.tsx`
- Modify: `web/src/features/results/ResultsPanel.tsx`
- Modify: `web/src/features/tree/ObjectTree.tsx`
- Modify: `web/src/locales/en/editor.json`
- Modify: `web/src/locales/zh-CN/editor.json`

**Interfaces:**
- Consumes：任务 5 的 `api.dataSql`、`TargetKind`。
- Produces：
  - `features/editor/tabs.ts`：`DataTarget { schema, object, objectKind, filter?, order? }`、`QueryTab.data?`、`sameData(tab, profileId, target)`、`useTabs().openData(profileId, target)`、`setData(id, { filter, order })`；
  - `features/data/sort.ts`：`Order`、`nextOrder(current, column)`、`sortedTitle(name, order)`；
  - `features/data/count.ts`：`countRows(api, profileId, sql)`；
  - `ResultGrid` / `ResultsPanel` 新增可选的 `sort`、`onSort(column)`；
  - 词条 `editor.data.*`。

- [ ] **Step 1: 新建 `web/src/features/data/sort.test.ts`**

```ts
import { describe, expect, it } from 'vitest'

import { nextOrder, sortedTitle } from './sort'

describe('数据标签的排序', () => {
  it('同一列依次切换升序、降序、不排序', () => {
    const asc = nextOrder(null, 'a')
    expect(asc).toEqual({ column: 'a', desc: false })
    const desc = nextOrder(asc, 'a')
    expect(desc).toEqual({ column: 'a', desc: true })
    expect(nextOrder(desc, 'a')).toBeNull()
  })

  it('换一列时从升序开始', () => {
    expect(nextOrder({ column: 'a', desc: true }, 'b')).toEqual({ column: 'b', desc: false })
    expect(nextOrder(undefined, 'b')).toEqual({ column: 'b', desc: false })
  })

  it('排序的列头加箭头', () => {
    expect(sortedTitle('a', { column: 'a', desc: false })).toBe('a ↑')
    expect(sortedTitle('a', { column: 'a', desc: true })).toBe('a ↓')
    expect(sortedTitle('b', { column: 'a', desc: true })).toBe('b')
    expect(sortedTitle('b', null)).toBe('b')
  })
})
```

- [ ] **Step 2: 修改 `web/src/features/editor/tabs.test.ts`**

```diff
--- a/web/src/features/editor/tabs.test.ts
+++ b/web/src/features/editor/tabs.test.ts
@@ -62,10 +62,52 @@ describe('标签页', () => {
       seq: 4,
     })
     expect(parseTabs('not json')).toEqual({ tabs: [], active: null, seq: 1 })
+    const data = { schema: 's', object: 't', objectKind: 'view', filter: 'a > 1', order: { column: 'a', desc: true } }
+    const withData = JSON.stringify({
+      tabs: [
+        { id: 'd', title: 't', profileId: 'p', sql: '', data },
+        { id: 'bad', title: 't', profileId: 'p', sql: '', data: { schema: 's', object: 't', objectKind: 'index' } },
+      ],
+    })
+    expect(parseTabs(withData).tabs).toEqual([{ id: 'd', title: 't', profileId: 'p', sql: '', data }])
     expect(parseTabs(undefined).tabs).toEqual([])
   })
 })
 
+describe('数据标签', () => {
+  beforeEach(() => useTabs.getState().restore(undefined))
+
+  it('标题为对象名，不占查询编号；同一对象只开一个', () => {
+    const { open, openData, activate } = useTabs.getState()
+    const q = open('p')
+    const a = openData('p', { schema: 's', object: 'orders', objectKind: 'table' })
+    activate(q)
+    expect(openData('p', { schema: 's', object: 'orders', objectKind: 'table' })).toBe(a)
+    expect(useTabs.getState().active).toBe(a)
+    const b = openData('p2', { schema: 's', object: 'orders', objectKind: 'table' })
+    expect(b).not.toBe(a)
+    open('p')
+    expect(useTabs.getState().tabs.map((x) => x.title)).toEqual(['查询1', 'orders', 'orders', '查询2'])
+  })
+
+  it('修改过滤和排序；查询标签不受影响', () => {
+    const { open, openData, setData } = useTabs.getState()
+    const q = open('p')
+    const d = openData('p', { schema: 's', object: 'v', objectKind: 'view' })
+    setData(d, { filter: 'x = 1', order: { column: 'x', desc: false } })
+    setData(q, { filter: 'y' })
+    const tabs = useTabs.getState().tabs
+    expect(tabs[1].data).toEqual({
+      schema: 's',
+      object: 'v',
+      objectKind: 'view',
+      filter: 'x = 1',
+      order: { column: 'x', desc: false },
+    })
+    expect(tabs[0].data).toBeUndefined()
+  })
+})
+
 describe('保存标签', () => {
   beforeEach(() => vi.useFakeTimers())
   afterEach(() => vi.useRealTimers())
```

- [ ] **Step 3: 新建 `web/src/features/data/DataView.tsx`**

```tsx
// 数据标签（M1c2 设计文档第 5 节）：只读地浏览一张表 / 视图 / 物化视图。
// 工具栏：连接和 schema（只显示）、WHERE 过滤、刷新、停止、统计行数；表格沿用查询标签的结果面板，点列头排序。
// SELECT 由后端按方言生成（data-sql），经这个标签自己的会话执行（沿用 runner：续取、取消、行数上限）。

import { IconPlayerStop, IconRefresh, IconSum } from '@tabler/icons-react'
import { Alert, App, Button, Input, Tooltip } from 'antd'
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { useApi } from '../../app/context'
import DbIcon from '../../shared/DbIcon'
import type { Profile } from '../../shared/api'
import ResultsPanel from '../results/ResultsPanel'
import { runnerFor, useTabRuntime } from '../editor/runner'
import { type DataTarget, type QueryTab, useTabs } from '../editor/tabs'
import { countRows } from './count'
import { nextOrder } from './sort'

export default function DataView({ tab, profiles }: { tab: QueryTab; profiles: Profile[] }) {
  const api = useApi()
  const { t } = useTranslation()
  const { message } = App.useApp()
  const runner = runnerFor(api, tab.id)
  const rt = useTabRuntime(tab.id)
  const profile = profiles.find((p) => p.id === tab.profileId)
  const data = tab.data!
  const [filter, setFilter] = useState(data.filter ?? '')
  const [total, setTotal] = useState<string | null>(null)
  const [counting, setCounting] = useState(false)

  const load = async (d: DataTarget) => {
    try {
      const sql = await api.dataSql(tab.profileId, d.schema, { object: d.object, filter: d.filter, order: d.order })
      await runner.run(sql, 0)
    } catch (e) {
      message.error((e as Error).message)
    }
  }

  /** 改过滤条件或排序：保存到标签，重新读取。 */
  const apply = (patch: Pick<DataTarget, 'filter' | 'order'>) => {
    const next = { ...data, ...patch }
    useTabs.getState().setData(tab.id, patch)
    if ('filter' in patch) setTotal(null)
    void load(next)
  }

  // 第一次显示时（以及连接从已删除恢复时）读取；切换回来时保留已读取的结果
  useEffect(() => {
    if (profile && !rt.submission && !rt.running) void load(data)
  }, [profile?.id])

  const count = async () => {
    setCounting(true)
    try {
      const sql = await api.dataSql(tab.profileId, data.schema, { object: data.object, filter: data.filter, count: true })
      setTotal(await countRows(api, tab.profileId, sql))
    } catch (e) {
      message.error((e as Error).message)
    } finally {
      setCounting(false)
    }
  }

  const loaded = rt.exec.statements[0]?.results[0]?.rows.length ?? 0
  return (
    <div className="query">
      <div className="row toolbar">
        {profile && <DbIcon driver={profile.driver} variant={rt.session?.variant ?? profile.variant} />}
        <span>{profile?.name}</span>
        <span className="muted">{data.schema}</span>
        <span className="sep" />
        <Input
          size="small"
          allowClear
          style={{ flex: 1, fontFamily: 'var(--mono)' }}
          placeholder={t('editor.data.filterPlaceholder')}
          value={filter}
          disabled={!profile}
          onChange={(e) => {
            setFilter(e.target.value)
            // 清空即取消过滤
            if (!e.target.value && data.filter) apply({ filter: undefined })
          }}
          onPressEnter={() => apply({ filter: filter.trim() || undefined })}
        />
        <Tooltip title={t('editor.data.refresh')} mouseEnterDelay={0.6}>
          <Button type="text" size="small" disabled={!profile || rt.running} onClick={() => void load(data)}>
            <IconRefresh size={16} />
          </Button>
        </Tooltip>
        <Tooltip title={t('editor.data.stop')} mouseEnterDelay={0.6}>
          <Button type="text" size="small" disabled={!rt.running} onClick={() => runner.cancel()}>
            <IconPlayerStop size={16} />
          </Button>
        </Tooltip>
        <Button size="small" icon={<IconSum size={14} />} loading={counting} disabled={!profile} onClick={() => void count()}>
          {t('editor.data.count')}
        </Button>
        {total !== null && <span className="muted">{t('editor.data.total', { count: Number(total) })}</span>}
      </div>
      {!profile && <Alert type="error" banner message={t('editor.query.profileDeleted')} />}
      <div className="grow" style={{ minHeight: 0 }}>
        <ResultsPanel
          rt={rt}
          onFetchMore={() => runner.fetchMore()}
          sort={data.order ?? null}
          onSort={(column) => apply({ order: nextOrder(data.order, column) })}
        />
      </div>
      <div className="row muted" style={{ padding: '2px 8px', gap: 8, fontSize: 12, borderTop: '1px solid var(--border)' }}>
        <span>{t('editor.data.loaded', { count: loaded })}</span>
        <span className="grow" />
        <span>{t('editor.data.sortHint')}</span>
      </div>
    </div>
  )
}
```

- [ ] **Step 4: 新建 `web/src/features/data/count.ts`**

```ts
// 数据标签的"统计行数"：另开一个会话执行 COUNT(*)，取第一行第一列后关闭。
// 不用标签自己的会话：它可能还有没读完的结果（服务端在等续取），会话正忙。

import { type Api, errorText } from '../../shared/api'
import { ExecSocket, cellText } from '../../shared/exec'
import { t } from '../../shared/i18n'
import { withVault } from '../connections/vault'

export async function countRows(api: Api, profileId: string, sql: string): Promise<string> {
  const session = await withVault(api, () => api.openSession(profileId))
  let socket: ExecSocket | null = null
  try {
    return await new Promise<string>((resolve, reject) => {
      let value: string | null = null
      socket = new ExecSocket(
        api.wsUrl(session.id),
        (ev) => {
          if (ev.type === 'rows' && value === null && ev.rows.length) value = cellText(ev.rows[0][0])
          else if (ev.type === 'failed' || ev.type === 'error') reject(new Error(errorText(ev.error)))
          else if (ev.type === 'done') resolve(value ?? '')
        },
        () => reject(new Error(t('editor.runner.serviceDisconnected'))),
      )
      socket.exec(sql).catch(reject)
    })
  } finally {
    ;(socket as ExecSocket | null)?.close()
    await api.closeSession(session.id).catch(() => {})
  }
}
```

- [ ] **Step 5: 新建 `web/src/features/data/sort.ts`**

```ts
// 数据标签的排序（M1c2 设计文档第 5 节）：点列头依次切换升序、降序、不排序；同一时间只按一列排序。

export type Order = { column: string; desc: boolean } | null

/** 点了 column 的列头之后的排序。 */
export function nextOrder(current: Order | undefined, column: string): Order {
  if (!current || current.column !== column) return { column, desc: false }
  return current.desc ? null : { column, desc: true }
}

/** 列头的标题：排序的列后面加箭头。 */
export function sortedTitle(name: string, order: Order | undefined): string {
  if (order?.column !== name) return name
  return `${name} ${order.desc ? '↓' : '↑'}`
}
```

- [ ] **Step 6: 修改 `web/src/features/editor/Workspace.tsx`**

```diff
--- a/web/src/features/editor/Workspace.tsx
+++ b/web/src/features/editor/Workspace.tsx
@@ -1,7 +1,7 @@
-// 工作区（设计稿第 2 部分）：标签栏和当前标签页。标签顶部有一条环境色线，生产连接的标签整块标红、
+// 工作区（设计稿第 2 部分）：标签栏和当前标签页（查询标签或数据标签）。标签顶部有一条环境色线，生产连接的标签整块标红、
 // 标题带"生产"。懒加载：Monaco 和结果表格只在打开第一个查询标签时才加载。
 
-import { IconFileCode, IconPlus, IconX } from '@tabler/icons-react'
+import { IconFileCode, IconPlus, IconTable, IconX } from '@tabler/icons-react'
 import { useQuery } from '@tanstack/react-query'
 import { useEffect, useRef } from 'react'
 import { useTranslation } from 'react-i18next'
@@ -9,6 +9,7 @@ import { useTranslation } from 'react-i18next'
 import { useApi } from '../../app/context'
 import DbIcon from '../../shared/DbIcon'
 import type { Profile } from '../../shared/api'
+import DataView from '../data/DataView'
 import QueryView from './QueryView'
 import { disposeModel } from './SqlEditor'
 import { useQueryActions } from './actions'
@@ -38,7 +39,13 @@ function TabTitle({ tab, active, envClass, profileName, profile }: {
       }}
       title={`${tab.title} · ${profileName}`}
     >
-      {profile ? <DbIcon driver={profile.driver} variant={rt.session?.variant ?? profile.variant} /> : <IconFileCode size={14} />}
+      {tab.data ? (
+        <IconTable size={14} />
+      ) : profile ? (
+        <DbIcon driver={profile.driver} variant={rt.session?.variant ?? profile.variant} />
+      ) : (
+        <IconFileCode size={14} />
+      )}
       <span className="tab-title">{tab.title}</span>
       {envClass === 'prod' ? <span>· {t('editor.tab.prod')}</span> : <span className="muted">{profileName}</span>}
       {rt.running && <span className="running-dot" />}
@@ -91,7 +98,8 @@ export default function Workspace() {
           <IconPlus size={14} />
         </div>
       </div>
-      {tab && <QueryView tab={tab} profiles={profiles} />}
+      {tab &&
+        (tab.data ? <DataView key={tab.id} tab={tab} profiles={profiles} /> : <QueryView tab={tab} profiles={profiles} />)}
     </div>
   )
 }
```

- [ ] **Step 7: 修改 `web/src/features/editor/actions.tsx`**

```diff
--- a/web/src/features/editor/actions.tsx
+++ b/web/src/features/editor/actions.tsx
@@ -41,9 +41,11 @@ export function useQueryActions() {
     else useUi.getState().openConnection(null)
   }
 
-  /** 插入到当前编辑器的光标处；没有打开的标签时新开一个。 */
+  /** 插入到当前编辑器的光标处；当前不是查询标签（没有标签或是数据标签）时新开一个。 */
   const insertSql = (sql: string, profileId?: string) => {
-    if (useTabs.getState().active) sendInsert(sql)
+    const { tabs, active } = useTabs.getState()
+    const tab = tabs.find((x) => x.id === active)
+    if (tab && !tab.data) sendInsert(sql)
     else openSql(sql, profileId)
   }
 
```

- [ ] **Step 8: 修改 `web/src/features/editor/tabs.ts`**

```diff
--- a/web/src/features/editor/tabs.ts
+++ b/web/src/features/editor/tabs.ts
@@ -1,19 +1,57 @@
 // 查询标签页（M1 设计文档 1 节）：每个标签绑定一个连接，有自己的 SQL。打开的标签和其中的 SQL 存进设置
 // （ui.tabs），下次启动时恢复；会话不恢复，第一次执行时再打开。
+// 数据标签（M1c2 设计文档第 5 节）：带 data 的标签，只读地浏览一张表 / 视图 / 物化视图，同一对象只开一个。
 
 import { create } from 'zustand'
 
+import type { TargetKind } from '../../shared/api'
 import { t } from '../../shared/i18n'
 import { useSettings } from '../../shared/settings'
 
 export const TABS_KEY = 'ui.tabs'
 
+/** 数据标签浏览的对象，以及当前的过滤条件和排序（一并保存）。 */
+export interface DataTarget {
+  schema: string
+  object: string
+  objectKind: TargetKind
+  /** WHERE 后的条件；空表示不过滤。 */
+  filter?: string
+  order?: { column: string; desc: boolean } | null
+}
+
 export interface QueryTab {
   id: string
-  /** 「查询1」这样的标题：新建时按界面语言生成，之后是数据，不随语言变化。 */
+  /** 「查询1」这样的标题：新建时按界面语言生成，之后是数据，不随语言变化。数据标签为对象名。 */
   title: string
   profileId: string
   sql: string
+  /** 有值时是数据标签。 */
+  data?: DataTarget
+}
+
+const TARGET_KINDS: readonly unknown[] = ['table', 'view', 'materialized_view'] satisfies TargetKind[]
+
+function isDataTarget(v: unknown): v is DataTarget {
+  const d = v as DataTarget | null
+  const o = d?.order
+  return (
+    typeof d?.schema === 'string' &&
+    typeof d.object === 'string' &&
+    TARGET_KINDS.includes(d.objectKind) &&
+    (d.filter === undefined || typeof d.filter === 'string') &&
+    (o === undefined || o === null || (typeof o.column === 'string' && typeof o.desc === 'boolean'))
+  )
+}
+
+/** 是否同一个对象的数据标签。 */
+export function sameData(tab: QueryTab, profileId: string, d: DataTarget): boolean {
+  return (
+    tab.profileId === profileId &&
+    tab.data?.schema === d.schema &&
+    tab.data.object === d.object &&
+    tab.data.objectKind === d.objectKind
+  )
 }
 
 interface Persisted {
@@ -34,7 +72,8 @@ export function parseTabs(raw: string | undefined): Persisted {
         typeof t?.id === 'string' &&
         typeof t.title === 'string' &&
         typeof t.profileId === 'string' &&
-        typeof t.sql === 'string',
+        typeof t.sql === 'string' &&
+        (t.data === undefined || isDataTarget(t.data)),
     )
     const active = tabs.some((t) => t.id === v.active) ? (v.active as string) : (tabs[0]?.id ?? null)
     return { tabs, active, seq: typeof v.seq === 'number' && v.seq > 0 ? v.seq : tabs.length + 1 }
@@ -46,6 +85,10 @@ export function parseTabs(raw: string | undefined): Persisted {
 interface TabsState extends Persisted {
   /** 新建标签并切换过去，返回它的 id。 */
   open(profileId: string, sql?: string): string
+  /** 打开对象的数据标签：已经打开过时切换过去。返回标签的 id。 */
+  openData(profileId: string, target: DataTarget): string
+  /** 修改数据标签的过滤条件和排序。 */
+  setData(id: string, patch: Pick<DataTarget, 'filter' | 'order'>): void
   close(id: string): void
   activate(id: string): void
   setSql(id: string, sql: string): void
@@ -66,6 +109,20 @@ export const useTabs = create<TabsState>()((set, get) => ({
     set({ tabs: [...tabs, tab], active: tab.id, seq: seq + 1 })
     return tab.id
   },
+  openData: (profileId, target) => {
+    const { tabs } = get()
+    const same = tabs.find((x) => sameData(x, profileId, target))
+    if (same) {
+      set({ active: same.id })
+      return same.id
+    }
+    const data = { schema: target.schema, object: target.object, objectKind: target.objectKind }
+    const tab: QueryTab = { id: newId(), title: target.object, profileId, sql: '', data }
+    set({ tabs: [...tabs, tab], active: tab.id })
+    return tab.id
+  },
+  setData: (id, patch) =>
+    set({ tabs: get().tabs.map((x) => (x.id === id && x.data ? { ...x, data: { ...x.data, ...patch } } : x)) }),
   close: (id) => {
     const { tabs, active } = get()
     const i = tabs.findIndex((t) => t.id === id)
```

- [ ] **Step 9: 修改 `web/src/features/results/ResultGrid.tsx`**

```diff
--- a/web/src/features/results/ResultGrid.tsx
+++ b/web/src/features/results/ResultGrid.tsx
@@ -1,5 +1,6 @@
 // 结果表格（M1 设计文档 2.1 节）：Glide Data Grid，canvas 渲染。行号、数值右对齐、NULL 灰显；
 // 滚动到接近已加载的末尾时续取下一批；双击或回车查看单元格；选中区域可用 Ctrl+C 复制（制表符分隔）。
+// 数据标签（M1c2）：点列头排序，排序的列头带箭头。
 
 import '@glideapps/glide-data-grid/dist/index.css'
 
@@ -15,6 +16,7 @@ import {
 import { useMemo, useState } from 'react'
 
 import { type ResultSet, cellText } from '../../shared/exec'
+import { type Order, sortedTitle } from '../data/sort'
 import { useSetting } from '../../shared/settings'
 import { THEME_KEY } from '../../shared/theme'
 import CellViewer, { type ViewedCell } from './CellViewer'
@@ -60,16 +62,19 @@ export interface ResultGridProps {
   /** 服务端在等 fetch_more（这个结果集还有下一批）。 */
   waiting: boolean
   onFetchMore(): void
+  /** 当前的排序；给出 onSort 时点列头排序。 */
+  sort?: Order
+  onSort?(column: string): void
 }
 
-export default function ResultGrid({ result, waiting, onFetchMore }: ResultGridProps) {
+export default function ResultGrid({ result, waiting, onFetchMore, sort, onSort }: ResultGridProps) {
   const theme = useGridTheme()
   const [viewed, setViewed] = useState<ViewedCell | null>(null)
   // 列宽在列信息和第一批行到达时估计，续取的行不影响
   const hasRows = result.rows.length > 0
   const columns: GridColumn[] = useMemo(
-    () => result.columns.map((c, i) => ({ id: String(i), title: c.name, width: columnWidth(result, i) })),
-    [result.columns, hasRows],
+    () => result.columns.map((c, i) => ({ id: String(i), title: sortedTitle(c.name, sort), width: columnWidth(result, i) })),
+    [result.columns, hasRows, sort],
   )
   const muted = useMemo(() => ({ textDark: theme.textLight }), [theme])
 
@@ -98,6 +103,7 @@ export default function ResultGrid({ result, waiting, onFetchMore }: ResultGridP
         getCellContent={getCellContent}
         getCellsForSelection
         onVisibleRegionChanged={onVisible}
+        onHeaderClicked={onSort ? (col) => onSort(result.columns[col].name) : undefined}
         onCellActivated={([col, row]) =>
           setViewed({ column: result.columns[col].name, row: row + 1, value: result.rows[row][col] })
         }
```

- [ ] **Step 10: 修改 `web/src/features/results/ResultsPanel.tsx`**

```diff
--- a/web/src/features/results/ResultsPanel.tsx
+++ b/web/src/features/results/ResultsPanel.tsx
@@ -8,12 +8,16 @@ import { useTranslation } from 'react-i18next'
 
 import { errorText } from '../../shared/api'
 import type { TabRuntime } from '../editor/runner'
+import type { Order } from '../data/sort'
 import ResultGrid from './ResultGrid'
 import { type ResultTab, messageLines, resultTabs } from './results'
 
 export interface ResultsPanelProps {
   rt: TabRuntime
   onFetchMore(): void
+  /** 数据标签的排序（见 ResultGrid）。 */
+  sort?: Order
+  onSort?(column: string): void
 }
 
 function TabIcon({ tab, explain }: { tab: ResultTab; explain: boolean }) {
@@ -22,7 +26,7 @@ function TabIcon({ tab, explain }: { tab: ResultTab; explain: boolean }) {
   return explain ? <IconBinaryTree size={14} /> : <IconTable size={14} />
 }
 
-export default function ResultsPanel({ rt, onFetchMore }: ResultsPanelProps) {
+export default function ResultsPanel({ rt, onFetchMore, sort, onSort }: ResultsPanelProps) {
   const { t } = useTranslation()
   const { exec, explain, capped, running } = rt
   const tabs = resultTabs(exec, explain, capped)
@@ -61,6 +65,8 @@ export default function ResultsPanel({ rt, onFetchMore }: ResultsPanelProps) {
             result={exec.statements[current.stmt].results[current.result]}
             waiting={exec.waitingForMore && current.stmt === lastStmt}
             onFetchMore={onFetchMore}
+            sort={sort}
+            onSort={onSort}
           />
         )}
         {current.kind === 'error' && (
```

- [ ] **Step 11: 修改 `web/src/features/tree/ObjectTree.tsx`**

```diff
--- a/web/src/features/tree/ObjectTree.tsx
+++ b/web/src/features/tree/ObjectTree.tsx
@@ -1,7 +1,8 @@
 // 数据库对象树（设计文档 7.3 节；M1 设计稿第 1、3 部分）：所有连接在一棵树里，可以同时展开多个。
 // 懒加载：展开时向后端取下一层（后端有元数据缓存，"刷新"会先清后端缓存再重新取）。顶部过滤框按名称或注释过滤已加载的对象。
 // 右键菜单（M1a）：连接 —— 编辑、复制、删除、刷新；其他节点 —— 复制名称、刷新。
-// 对象节点另有（M1c2）："查看 DDL"（弹出只读预览）、"生成 SQL"（插入到当前编辑器）。
+// 对象节点另有（M1c2）："查看数据"（数据标签，也可双击打开）、"查看 DDL"（弹出只读预览）、
+// "生成 SQL"（插入到当前编辑器）。
 
 import { App, Dropdown, Input, type MenuProps, Tree } from 'antd'
 import type { DataNode, EventDataNode } from 'antd/es/tree'
@@ -24,6 +25,7 @@ import { useTranslation } from 'react-i18next'
 import { useApi } from '../../app/context'
 import { useUi } from '../../app/ui'
 import { useQueryActions } from '../editor/actions'
+import { useTabs } from '../editor/tabs'
 import type { Api, Environment, Profile, RoutineInfo } from '../../shared/api'
 import DbIcon from '../../shared/DbIcon'
 import { driverLabel } from '../connections/form'
@@ -286,8 +288,9 @@ export default function ObjectTree() {
   function objectItems(k: NodeKey): NonNullable<MenuProps['items']> {
     if (k.type !== 'o') return []
     const items: NonNullable<MenuProps['items']> = []
-    if (ddlKind(k.kind, routineKindOf(k))) items.push({ key: 'ddl', label: t('tree.menu.viewDdl') })
     const target = targetKind(k.kind)
+    if (target) items.push({ key: 'view-data', label: t('tree.menu.viewData') })
+    if (ddlKind(k.kind, routineKindOf(k))) items.push({ key: 'ddl', label: t('tree.menu.viewDdl') })
     if (target) {
       // generateMenu 的项是 antd 菜单项的子集
       const children = generateMenu(target, relations) as NonNullable<MenuProps['items']>
@@ -296,6 +299,14 @@ export default function ObjectTree() {
     return items.length ? [...items, { type: 'divider' }] : []
   }
 
+  /** 打开表、视图、物化视图的数据标签；其他节点不处理。 */
+  const openData = (k: NodeKey) => {
+    const objectKind = k.type === 'o' ? targetKind(k.kind) : null
+    if (k.type === 'o' && objectKind) {
+      useTabs.getState().openData(k.profileId, { schema: k.schema, object: k.name, objectKind })
+    }
+  }
+
   const onMenu = async (key: string) => {
     const k = menuNode
     if (!k) return
@@ -309,7 +320,8 @@ export default function ObjectTree() {
           ref: gen.relation && { schema: gen.relation.schema, table: gen.relation.table },
         })
         insertSql(sql, k.profileId)
-      } else if (key === 'ddl' && k.type === 'o') {
+      } else if (key === 'view-data') openData(k)
+      else if (key === 'ddl' && k.type === 'o') {
         const kind = ddlKind(k.kind, routineKindOf(k))
         const driver = profiles.find((p) => p.id === k.profileId)?.driver
         if (kind && driver) setDdl({ profileId: k.profileId, driver, schema: k.schema, kind, name: k.name })
@@ -371,6 +383,10 @@ export default function ObjectTree() {
             loadedKeys={Object.keys(loaded)}
             loadData={(node: EventDataNode<DataNode>) => load(String(node.key))}
             onSelect={(_, { node }) => selectProfile(decodeKey(String(node.key)).profileId)}
+            onDoubleClick={(_, node) => {
+              const key = String(node.key)
+              if (!key.includes('\u0002')) openData(decodeKey(key))
+            }}
             onRightClick={({ node }) => {
               const key = String(node.key)
               setMenuNode(key.includes('\u0002') ? null : decodeKey(key))
```

- [ ] **Step 12: 修改 `web/src/locales/en/editor.json`**

```diff
--- a/web/src/locales/en/editor.json
+++ b/web/src/locales/en/editor.json
@@ -61,5 +61,14 @@
     "incomplete": "Statement is incomplete",
     "near": "Syntax error near '{{near}}'",
     "compatHint": " (checked with {{grammar}} grammar; {{name}} extensions may be flagged by mistake)"
+  },
+  "data": {
+    "filterPlaceholder": "WHERE condition, Enter to apply, e.g. status = 1",
+    "refresh": "Refresh",
+    "stop": "Stop",
+    "count": "Count Rows",
+    "total": "{{count}} rows in total",
+    "loaded": "{{count}} rows loaded",
+    "sortHint": "Click a column header to sort: ascending, descending, none"
   }
 }
```

- [ ] **Step 13: 修改 `web/src/locales/zh-CN/editor.json`**

```diff
--- a/web/src/locales/zh-CN/editor.json
+++ b/web/src/locales/zh-CN/editor.json
@@ -61,5 +61,14 @@
     "incomplete": "语句不完整",
     "near": "“{{near}}”附近有语法错误",
     "compatHint": "（按 {{grammar}} 语法检查，{{name}} 的扩展语法可能被误报）"
+  },
+  "data": {
+    "filterPlaceholder": "WHERE 条件，回车应用，如 status = 1",
+    "refresh": "刷新",
+    "stop": "停止",
+    "count": "统计行数",
+    "total": "共 {{count}} 行",
+    "loaded": "已读取 {{count}} 行",
+    "sortHint": "点列头排序：升序、降序、不排序"
   }
 }
```

- [ ] **Step 14: 检查**

在 PowerShell 中执行（bash 里 `cd web` 会触发坏掉的 fnm 钩子，`&&` 后面的命令会被跳过）：

```powershell
$env:PATH = "D:\Environments\nvm4w\nodejs;$env:PATH"; Set-Location web; pnpm typecheck; pnpm test; pnpm build
```

Expected: tsc 无输出；31 个测试文件、200 个测试通过；打包成功。

- [ ] **Step 15: 提交**

```bash
git add web/src/features/data/DataView.tsx web/src/features/data/count.ts web/src/features/data/sort.test.ts web/src/features/data/sort.ts web/src/features/editor/Workspace.tsx web/src/features/editor/actions.tsx web/src/features/editor/tabs.test.ts web/src/features/editor/tabs.ts web/src/features/results/ResultGrid.tsx web/src/features/results/ResultsPanel.tsx web/src/features/tree/ObjectTree.tsx web/src/locales/en/editor.json web/src/locales/zh-CN/editor.json
git commit -m "feat(web): 数据标签——只读浏览表 / 视图，WHERE 过滤、点列头排序、滚动续取、统计行数" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 7: 实测修正与说明文档

写计划前在浏览器中实测发现、已经修正的问题：
- 数据标签换排序时数据不变：首批读完后服务端在等 fetch_more，执行状态一直是 running，`run` 忽略新的提交。前端新增 `Runner.rerun`（先取消、等结束再执行）；但服务端在等 fetch_more 时语句并没有在执行，数据库层面的取消不起作用，pump 一直等下去。WebSocket 的 cancel 现在同时通知 pump：在服务端读完并丢掉剩下的行，其他事件照常发出，直到 done（查询标签达到行数上限时的取消也受益）。
- 插入的 SQL 和光标后面的内容连在一起：光标后已有内容时也另起一行。
- 包体的 DDL 取成了包：包和包体同名，对象树节点的键相同。程序节点的键增加 routineKind。

然后写 M1c2 说明文档，更新接口说明（WebSocket 的 cancel）和 M1 设计文档的进度。若任务 1 的复查有升级，在说明文档中加"依赖"一节。

**Files:**
- Modify: `crates/dblens-api/src/ws.rs`
- Modify: `crates/dblens-api/tests/api.rs`
- Modify: `docs/notes/m0-4b-api.md`
- Create: `docs/notes/m1c2-object-actions.md`
- Modify: `docs/superpowers/specs/2026-10-25-m1-workbench-design.md`
- Modify: `web/src/features/data/DataView.tsx`
- Modify: `web/src/features/editor/QueryView.tsx`
- Modify: `web/src/features/editor/runner.test.ts`
- Modify: `web/src/features/editor/runner.ts`
- Modify: `web/src/features/tree/ObjectTree.tsx`
- Modify: `web/src/features/tree/nodes.test.ts`
- Modify: `web/src/features/tree/nodes.ts`

**Interfaces:**
- Produces：
  - `Runner.rerun(sql)`；
  - 对象树 `NodeKey` 的 `o` 增加可选的 `routineKind`（编码为第 6 段）；
  - WebSocket：等 fetch_more 时收到 cancel，丢掉剩下的行，发出其余事件直到 `done`。

- [ ] **Step 1: 修改 `crates/dblens-api/tests/api.rs`**

```diff
--- a/crates/dblens-api/tests/api.rs
+++ b/crates/dblens-api/tests/api.rs
@@ -284,6 +284,29 @@ async fn cancel_over_websocket() {
     assert_eq!(recv(&mut ws).await["type"], "done");
 }
 
+#[tokio::test]
+async fn cancel_while_waiting_for_fetch_more_discards_the_rest() {
+    let (mut ws, _, _) = ws_session().await;
+    send(&mut ws, json!({"type": "exec", "sql": "ROWS 10", "max_rows": 2})).await;
+    for _ in 0..3 {
+        recv(&mut ws).await;
+    }
+    assert!(quiet(&mut ws).await, "在等 fetch_more");
+    send(&mut ws, json!({"type": "cancel"})).await;
+    let mut types = Vec::new();
+    loop {
+        let v = recv(&mut ws).await;
+        types.push(v["type"].as_str().unwrap().to_string());
+        if v["type"] == "done" {
+            break;
+        }
+    }
+    assert!(!types.iter().any(|t| t == "rows"), "剩下的行被丢掉：{types:?}");
+    // 会话照常可用
+    send(&mut ws, json!({"type": "exec", "sql": "ROWS 1"})).await;
+    assert_eq!(recv(&mut ws).await["type"], "statement_start");
+}
+
 #[tokio::test]
 async fn disconnect_mid_result_frees_the_session() {
     let (mut ws, service, sid) = ws_session().await;
```

- [ ] **Step 2: 修改 `web/src/features/editor/runner.test.ts`**

```diff
--- a/web/src/features/editor/runner.test.ts
+++ b/web/src/features/editor/runner.test.ts
@@ -143,6 +143,23 @@ describe('执行器', () => {
     expect(rt(tabId).exec.waitingForMore).toBe(true)
   })
 
+  it('重新执行：先取消还在等续取的结果，结束后再提交', async () => {
+    const r = new Runner(fakeApi(), tabId)
+    await r.run('select * from t', 0)
+    const ws = FakeWs.last
+    const batch = Array.from({ length: 500 }, (_, i) => [{ t: 'int', v: i }])
+    ws.push({ type: 'statement_start', index: 0, total: 1, start: 0, end: 15, info: { keyword: 'SELECT' } })
+    ws.push({ type: 'result_start', columns: [{ name: 'n', type_name: 'int', nullable: false }] })
+    ws.push({ type: 'rows', rows: batch })
+    const again = r.rerun('select * from t order by n')
+    await flush()
+    expect(ws.sent.map((m) => m.type)).toEqual(['exec', 'cancel'])
+    ws.push({ type: 'done' })
+    await again
+    expect(ws.sent.at(-1)).toMatchObject({ type: 'exec', sql: 'select * from t order by n' })
+    expect(rt(tabId).running).toBe(true)
+  })
+
   it('关闭时关闭会话并丢掉状态', async () => {
     const api = fakeApi()
     const r = new Runner(api, tabId)
```

- [ ] **Step 3: 修改 `web/src/features/tree/nodes.test.ts`**

```diff
--- a/web/src/features/tree/nodes.test.ts
+++ b/web/src/features/tree/nodes.test.ts
@@ -28,6 +28,7 @@ describe('对象树节点', () => {
       { type: 's', profileId: 'p1', schema: 'my.schema' },
       { type: 'k', profileId: 'p1', schema: 'HR', kind: 'materialized_views' },
       { type: 'o', profileId: 'p1', schema: 'public', kind: 'tables', name: '订单 明细' },
+      { type: 'o', profileId: 'p1', schema: 'HR', kind: 'routines', name: 'PKG', routineKind: 'package_body' },
       { type: 'l', profileId: 'p1', schema: 'public', table: 'orders', column: 'id' },
     ]
     for (const k of keys) expect(decodeKey(encodeKey(k))).toEqual(k)
```

- [ ] **Step 4: 修改 `crates/dblens-api/src/ws.rs`**

```diff
--- a/crates/dblens-api/src/ws.rs
+++ b/crates/dblens-api/src/ws.rs
@@ -7,7 +7,7 @@
 //!   - `cursor`：只执行这个 UTF-16 偏移所在的那条语句；
 //!   - `explain`：只生成执行计划（只能有一条语句，或用 `cursor` 选出一条）；
 //! - `{"type":"fetch_more","max_rows":500}`：取下一批行；
-//! - `{"type":"cancel"}`：取消正在执行的语句。
+//! - `{"type":"cancel"}`：取消正在执行的语句；正在等 `fetch_more` 时丢掉还没读的行，接着发后面的事件直到 `done`。
 //!
 //! 服务端发送 `SessionEvent`（`statement_start`、`result_start`、`rows`、`result_end`、`affected`、`message`、
 //! `statement_end`、`confirm_required`、`failed`、`done`），协议层面的错误发 `{"type":"error","error":{…}}`。
@@ -118,6 +118,31 @@ pub async fn upgrade(
     Ok(ws.on_upgrade(move |socket| scope_lang(lang, run(socket, service, id))))
 }
 
+/// 发给 pump 的额度为 0：客户端取消了还没读完的结果。
+const STOP: usize = 0;
+
+/// 客户端取消了还没读完的结果：在服务端读完并丢掉剩下的行（驱动要求读完才能执行下一条），
+/// 其他事件照常发出，直到 `done`。
+async fn discard(service: &Service, id: SessionId, sql: &str, out: &mpsc::Sender<String>) {
+    loop {
+        match service.sessions.next(id, 1000).await {
+            Ok(Some(SessionEvent::Rows { .. } | SessionEvent::ResultEnd)) => {}
+            Ok(Some(event)) => {
+                let done = event == SessionEvent::Done;
+                let _ = out.send(encode(event, sql)).await;
+                if done {
+                    return;
+                }
+            }
+            Ok(None) => return,
+            Err(e) => {
+                let _ = out.send(error_message(&e)).await;
+                return;
+            }
+        }
+    }
+}
+
 /// 从会话取事件发给客户端；每发出装满的一批行就等一次 `fetch_more`。
 async fn pump(
     service: Arc<Service>,
@@ -139,7 +164,8 @@ async fn pump(
                 }
                 if full {
                     match credit.recv().await {
-                        Some(n) => max_rows = n.max(1),
+                        Some(STOP) => return discard(&service, id, &sql, &out).await,
+                        Some(n) => max_rows = n,
                         None => break,
                     }
                 }
@@ -206,13 +232,17 @@ async fn run(socket: WebSocket, service: Arc<Service>, id: SessionId) {
             }
             ClientMsg::FetchMore { max_rows } => {
                 if let Some(c) = &credit {
-                    let _ = c.try_send(max_rows);
+                    let _ = c.try_send(max_rows.max(1));
                 }
             }
             ClientMsg::Cancel => {
                 if let Err(e) = service.sessions.cancel(id).await {
                     let _ = out.send(error_message(&e)).await;
                 }
+                // 正在等 fetch_more 时语句并没有在执行，数据库层面的取消不起作用：让 pump 丢掉剩下的行（STOP）
+                if let Some(c) = &credit {
+                    let _ = c.try_send(STOP);
+                }
             }
         }
     }
```

- [ ] **Step 5: 修改 `docs/notes/m0-4b-api.md`**

```diff
--- a/docs/notes/m0-4b-api.md
+++ b/docs/notes/m0-4b-api.md
@@ -151,7 +151,7 @@
     - `cursor`：只执行这个 UTF-16 偏移所在的语句；
     - `explain`：只生成执行计划。
 - `{"type":"fetch_more","max_rows":500}`：取下一批行。
-- `{"type":"cancel"}`：取消。
+- `{"type":"cancel"}`：取消。服务端正在等 `fetch_more` 时（语句并没有在执行），丢掉还没读的行，接着发后面的事件直到 `done`（M1c2 加入）。
 
 ### 服务端发送
 
```

- [ ] **Step 6: 新建 `docs/notes/m1c2-object-actions.md`**

````markdown
# M1c2 对象操作

依据：设计文档 `docs/superpowers/specs/2026-10-26-m1c2-object-actions-design.md`。M1c 的第二份（M1c1 历史与收藏已完成，M1c3 导出与运维查询在后）。

## 结构

```text
crates/dblens-dialect/src/
  ddl.rs          DdlKind；ddl_query（MySQL 系 SHOW CREATE、Oracle / 达梦 DBMS_METADATA.GET_DDL）；
                  PG 系由结构模型拼装（table_ddl = table_create + table_extras、view_ddl、sequence_ddl）；ddl_text
  browse.rs       外键关系 relations、关联查询 join_sql、数据标签的 data_sql
  meta/mod.rs     MetaKey::Ddl（不在模板文件中，只用于缓存）
crates/dblens-service/src/
  objects.rs      MetadataService::ddl / generate / relations / data_sql
  metadata.rs     run：按方言生成查询、经元数据连接池执行、按键缓存（rows 改为调用它）
crates/dblens-api/src/
  routes.rs       …/ddl、…/generate、…/relations、…/data-sql
  ws.rs           cancel 在等 fetch_more 时丢掉剩下的行（见下）
web/src/
  features/tree/  右键菜单的纯逻辑（objectMenu.ts）、DDL 预览（DdlDialog.tsx，懒加载 Monaco）、ObjectTree 的菜单和双击
  features/data/  数据标签（DataView.tsx）、排序（sort.ts）、统计行数（count.ts）
  features/editor/ 标签增加 data（tabs.ts）、Runner.rerun、工作区按标签种类显示
  features/results/ 列头点击排序（ResultGrid、ResultsPanel 的 sort / onSort）
```

## 查看 DDL

- 表、视图、物化视图、序列、同义词：
  - MySQL 系：`SHOW CREATE TABLE / VIEW / SEQUENCE`，取名字以 `Create ` 开头的列。
  - Oracle、达梦：`DBMS_METADATA.GET_DDL`。实测它给出的表 DDL 不含索引和注释，服务层用结构模型补上（`table_extras`，不含触发器）。
  - PG 系：由结构模型拼装。
  - 返回 LOB 时只读到前 64 KB，超出时在末尾注明已截断。
- 函数、过程、包、包体：各方言都用已有的程序源码查询（`MetaKey::RoutineSource`）。这里与设计文档不同：`DBMS_METADATA` 返回 CLOB，长的包会被截断；程序源码按行读取，没有这个问题。
- 达梦的 DDL 换行是 `\r\n`，统一换成 `\n`。
- 对象不存在：PG 系报 404；由数据库给出 DDL 的报数据库的错误（422）。
- 索引的 `USING 方法` 只用于 PG 系（Oracle、达梦的 NORMAL、BITMAP 不是 USING 子句）。

## 生成 SQL

- 接口参数用 `object` + `object_kind`（table / view / materialized_view），不是设计文档中的 `table`：视图也能生成 SELECT。
- 关联查询：
  - 外键关系由单独的接口 `…/relations?table=` 给出，前端右键表时取，用来组装"关联查询"子菜单；取不到时当作没有外键。
  - 子菜单项：`→ 父表 (本表的列)`、`← 子表 (子表的列)`。"全部外键"只连接本表引用的表，只有被引用时置灰。
- 插入：沿用 M1c1 的插入通道。当前不是查询标签（没有标签或是数据标签）时用该连接新开一个。
  - 实测后改：光标后面已有内容时也另起一行（之前只处理光标前），生成的 SQL 不和后面的 SQL 连在一起。

## 数据标签

- 标签：`QueryTab.data = { schema, object, objectKind, filter?, order? }`。过滤条件和排序随标签保存、恢复；同一连接的同一对象只开一个；标题为对象名，不占"查询N"的编号。
- 读取：前端调 `data-sql` 取 SQL，经标签自己的 runner 执行（首批 500 行、滚动续取、行数上限、错误显示与查询标签一致）。
- 换过滤、排序或刷新时用 `Runner.rerun`：首批读完后服务端在等 fetch_more，执行状态一直是 running，`run` 会忽略新的提交，所以先取消、等结束再执行。
- 取消还没读完的结果（实测时发现，属于 WebSocket 协议的修正）：
  - 服务端在等 fetch_more 时语句并没有在执行，数据库层面的取消不起作用，原先 pump 会一直等下去，客户端收不到 done。查询标签达到行数上限时的取消也有同样的问题。
  - 现在 cancel 同时通知 pump：在服务端读完并丢掉剩下的行（驱动要求读完才能执行下一条），其他事件照常发出，直到 done。
  - 剩下的行很多时要在服务端读完，与客户端断开时的处理相同。
  - REST 的 `/sessions/{id}/cancel` 不经过 pump，没有这个处理；前端只用 WebSocket 取消。
- 统计行数：另开一个会话执行 `COUNT(*)`，取完即关（标签自己的会话可能还有没读完的结果）。
- 数据标签执行的 SELECT 照常记入历史；生产环境不需要确认（只读）。

## 对象树

- 程序节点的键增加 routineKind：包和包体同名，原来的键相同，查看包体的 DDL 会取成包。这是实测时发现的。

## 验证

- 后端：fmt、clippy 通过；全部测试在 `DBLENS_LANG=en` 下对 5 个实库通过。
  - 服务层的实库测试在 5 个库中建父子表（主键、唯一、外键、检查、索引、注释）、视图、序列、函数，查看 DDL、生成 SQL、取外键关系、执行数据 SQL 后删除。
  - 接口的实库测试检查参数、状态码；WebSocket 新增"等 fetch_more 时取消"的测试。
- 前端：tsc、Vitest、打包通过。
- 浏览器实测（5 个库建测试对象，1200 行子表）：
  - PostgreSQL，中文界面：右键菜单；表的 DDL（外键 ON DELETE CASCADE、检查约束、降序索引）、在新标签中打开；生成 SQL 子菜单、按外键插入关联查询；双击打开数据标签，排序（升序、降序、不排序）、WHERE 过滤、错误的条件显示数据库报错、统计行数、滚动续取到 1000 行；重新打开页面后数据标签和排序恢复。
  - Oracle（生产环境），英文界面：数据标签（标签标红）、统计行数、排序；包体的 DDL。
  - 5 个库各种对象的 DDL 和关联查询经接口逐一核对：MySQL 系的程序、序列（没有这些对象）和包等不支持的种类返回 404 / 422 / 400；Oracle、达梦的表 DDL 补上了索引和注释。
````

- [ ] **Step 7: 修改 `docs/superpowers/specs/2026-10-25-m1-workbench-design.md`**

```diff
--- a/docs/superpowers/specs/2026-10-25-m1-workbench-design.md
+++ b/docs/superpowers/specs/2026-10-25-m1-workbench-design.md
@@ -158,6 +158,7 @@ M1b 在写计划时（2026-10-25，用户确认）再拆成两份：
 - M1b2 已完成（设计 `docs/superpowers/specs/2026-10-25-m1b2-completion-design.md`，详细计划 `docs/superpowers/plans/2026-10-26-m1b2-completion.md`，说明 `docs/notes/m1b2-completion.md`）。
 - 2026-10-26 M1c 再拆成三份（用户确认）：M1c1 历史与收藏；M1c2 对象操作（查看 DDL、生成 SQL、关联查询、"查看数据"标签）；M1c3 导出与运维查询。
 - M1c1 已完成（设计 `docs/superpowers/specs/2026-10-26-m1c1-history-favorites-design.md`，详细计划 `docs/superpowers/plans/2026-10-26-m1c1-history-favorites.md`，说明 `docs/notes/m1c1-history-favorites.md`）。
+- M1c2 已完成（设计 `docs/superpowers/specs/2026-10-26-m1c2-object-actions-design.md`，详细计划 `docs/superpowers/plans/2026-10-26-m1c2-object-actions.md`，说明 `docs/notes/m1c2-object-actions.md`）。
 
 右键菜单中的"查看 DDL"和"生成 SQL"依赖 M1c 的后端，M1a 先不显示这两项。
 
```

- [ ] **Step 8: 修改 `web/src/features/data/DataView.tsx`**

```diff
--- a/web/src/features/data/DataView.tsx
+++ b/web/src/features/data/DataView.tsx
@@ -31,7 +31,7 @@ export default function DataView({ tab, profiles }: { tab: QueryTab; profiles: P
   const load = async (d: DataTarget) => {
     try {
       const sql = await api.dataSql(tab.profileId, d.schema, { object: d.object, filter: d.filter, order: d.order })
-      await runner.run(sql, 0)
+      await runner.rerun(sql)
     } catch (e) {
       message.error((e as Error).message)
     }
@@ -85,7 +85,7 @@ export default function DataView({ tab, profiles }: { tab: QueryTab; profiles: P
           onPressEnter={() => apply({ filter: filter.trim() || undefined })}
         />
         <Tooltip title={t('editor.data.refresh')} mouseEnterDelay={0.6}>
-          <Button type="text" size="small" disabled={!profile || rt.running} onClick={() => void load(data)}>
+          <Button type="text" size="small" disabled={!profile} onClick={() => void load(data)}>
             <IconRefresh size={16} />
           </Button>
         </Tooltip>
```

- [ ] **Step 9: 修改 `web/src/features/editor/QueryView.tsx`**

```diff
--- a/web/src/features/editor/QueryView.tsx
+++ b/web/src/features/editor/QueryView.tsx
@@ -133,9 +133,10 @@ export default function QueryView({ tab, profiles }: { tab: QueryTab; profiles:
         const sel = ed?.getSelection()
         const model = ed?.getModel()
         if (!ed || !sel || !model) return
-        // 光标前已有内容时另起一行，不和原有的 SQL 连在一起
+        // 光标前、后已有内容时各另起一行，不和原有的 SQL 连在一起
         const before = model.getLineContent(sel.startLineNumber).slice(0, sel.startColumn - 1)
-        const text = before.trim() ? `\n${sql}` : sql
+        const after = model.getLineContent(sel.endLineNumber).slice(sel.endColumn - 1)
+        const text = `${before.trim() ? '\n' : ''}${sql}${after.trim() ? '\n' : ''}`
         ed.executeEdits('insert', [{ range: sel, text, forceMoveMarkers: true }])
         ed.focus()
       }),
```

- [ ] **Step 10: 修改 `web/src/features/editor/runner.ts`**

```diff
--- a/web/src/features/editor/runner.ts
+++ b/web/src/features/editor/runner.ts
@@ -180,6 +180,26 @@ export class Runner {
     }
   }
 
+  /**
+   * 先停下正在执行的（包括还在等续取的结果），再执行 sql。数据标签换过滤条件、排序或刷新时用：
+   * 首批结果读完后服务端在等续取，执行状态一直是 running，run 会忽略新的提交。
+   */
+  async rerun(sql: string) {
+    if (this.rt().running) {
+      const stopped = new Promise<void>((resolve) => {
+        const off = useRuntime.subscribe((s) => {
+          if (!s.tabs[this.tabId]?.running) {
+            off()
+            resolve()
+          }
+        })
+      })
+      this.cancel()
+      await stopped
+    }
+    await this.run(sql, 0)
+  }
+
   /** 用户确认后，从需要确认的那条语句接着执行。 */
   async confirm() {
     const { exec, submission } = this.rt()
```

- [ ] **Step 11: 修改 `web/src/features/tree/ObjectTree.tsx`**

```diff
--- a/web/src/features/tree/ObjectTree.tsx
+++ b/web/src/features/tree/ObjectTree.tsx
@@ -114,7 +114,7 @@ async function loadChildren(api: Api, profiles: Profile[], k: NodeKey): Promise<
     }
     case 'k': {
       const obj = (name: string, comment: string | null, extra?: string, routineKind?: RoutineInfo['kind']) => ({
-        key: encodeKey({ type: 'o', profileId: k.profileId, schema: k.schema, kind: k.kind, name }),
+        key: encodeKey({ type: 'o', profileId: k.profileId, schema: k.schema, kind: k.kind, name, routineKind }),
         label: name,
         comment,
         extra,
@@ -174,12 +174,6 @@ export default function ObjectTree() {
   })
   const relations = relationsError ? [] : relationsData
 
-  /** 对象节点的程序类型（函数、过程、包、包体），从已加载的上一层取。 */
-  const routineKindOf = (k: Extract<NodeKey, { type: 'o' }>) =>
-    loaded[encodeKey({ type: 'k', profileId: k.profileId, schema: k.schema, kind: k.kind })]?.items.find(
-      (i) => i.key === encodeKey(k),
-    )?.routineKind
-
   const load = useCallback(
     async (key: string) => {
       const k = decodeKey(key)
@@ -290,7 +284,7 @@ export default function ObjectTree() {
     const items: NonNullable<MenuProps['items']> = []
     const target = targetKind(k.kind)
     if (target) items.push({ key: 'view-data', label: t('tree.menu.viewData') })
-    if (ddlKind(k.kind, routineKindOf(k))) items.push({ key: 'ddl', label: t('tree.menu.viewDdl') })
+    if (ddlKind(k.kind, k.routineKind)) items.push({ key: 'ddl', label: t('tree.menu.viewDdl') })
     if (target) {
       // generateMenu 的项是 antd 菜单项的子集
       const children = generateMenu(target, relations) as NonNullable<MenuProps['items']>
@@ -322,7 +316,7 @@ export default function ObjectTree() {
         insertSql(sql, k.profileId)
       } else if (key === 'view-data') openData(k)
       else if (key === 'ddl' && k.type === 'o') {
-        const kind = ddlKind(k.kind, routineKindOf(k))
+        const kind = ddlKind(k.kind, k.routineKind)
         const driver = profiles.find((p) => p.id === k.profileId)?.driver
         if (kind && driver) setDdl({ profileId: k.profileId, driver, schema: k.schema, kind, name: k.name })
       } else if (key === 'new-query') newQuery(k.profileId)
```

- [ ] **Step 12: 修改 `web/src/features/tree/nodes.ts`**

```diff
--- a/web/src/features/tree/nodes.ts
+++ b/web/src/features/tree/nodes.ts
@@ -43,7 +43,7 @@ export function categoriesFor(driver: DriverId): Category[] {
  * - `s` schema：s、profileId、schema
  * - `g` 系统 schema 分组：g、profileId
  * - `k` 分类：k、profileId、schema、kind
- * - `o` 对象：o、profileId、schema、kind、name
+ * - `o` 对象：o、profileId、schema、kind、name，程序另有 routineKind（包和包体同名，靠它区分）
  * - `l` 列：l、profileId、schema、table、column
  */
 export type NodeKey =
@@ -51,7 +51,14 @@ export type NodeKey =
   | { type: 's'; profileId: string; schema: string }
   | { type: 'g'; profileId: string }
   | { type: 'k'; profileId: string; schema: string; kind: CategoryKind }
-  | { type: 'o'; profileId: string; schema: string; kind: CategoryKind; name: string }
+  | {
+      type: 'o'
+      profileId: string
+      schema: string
+      kind: CategoryKind
+      name: string
+      routineKind?: RoutineInfo['kind']
+    }
   | { type: 'l'; profileId: string; schema: string; table: string; column: string }
 
 const SEP = '\u0001'
@@ -66,14 +73,14 @@ export function encodeKey(k: NodeKey): string {
     case 'k':
       return [k.type, k.profileId, k.schema, k.kind].join(SEP)
     case 'o':
-      return [k.type, k.profileId, k.schema, k.kind, k.name].join(SEP)
+      return [k.type, k.profileId, k.schema, k.kind, k.name, ...(k.routineKind ? [k.routineKind] : [])].join(SEP)
     case 'l':
       return [k.type, k.profileId, k.schema, k.table, k.column].join(SEP)
   }
 }
 
 export function decodeKey(s: string): NodeKey {
-  const [type, profileId, a, b, c] = s.split(SEP)
+  const [type, profileId, a, b, c, d] = s.split(SEP)
   switch (type) {
     case 'c':
       return { type, profileId }
@@ -84,7 +91,9 @@ export function decodeKey(s: string): NodeKey {
     case 'k':
       return { type, profileId, schema: a, kind: b as CategoryKind }
     case 'o':
-      return { type, profileId, schema: a, kind: b as CategoryKind, name: c }
+      return d
+        ? { type, profileId, schema: a, kind: b as CategoryKind, name: c, routineKind: d as RoutineInfo['kind'] }
+        : { type, profileId, schema: a, kind: b as CategoryKind, name: c }
     case 'l':
       return { type, profileId, schema: a, table: b, column: c }
     default:
```

- [ ] **Step 13: 检查**

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

实测（浏览器模式，用新的数据目录启动 `dblens-server --web-dir web/dist`）：
1. 经接口建 5 个测试连接（密码只经环境变量传给建连接的脚本），设好 Oracle、达梦的客户端库目录；经 WebSocket 在 5 个库中建测试对象：父子表（主键、唯一、外键、检查、索引、注释）、视图、序列、函数（PG、Oracle、达梦）、物化视图（PG、Oracle、达梦）、包和同义词（Oracle、达梦），子表 1200 行（MySQL 系 990 行，递归 CTE 默认上限 1000 次）。
2. PostgreSQL，中文界面：右键子表 → 查看 DDL，有外键（ON DELETE CASCADE）、检查约束、降序索引；"在新标签中打开"。生成 SQL → 关联查询 → "→ c2_parent (parent_id)"，插入的 SQL 与光标后的内容分行。
3. 双击子表打开数据标签：首批 500 行；点 qty 列头两次（升序、降序），数据随之变化；WHERE `parent_id = 2` 回车、统计行数为 400；错误的条件显示数据库报错；清空过滤后拖到底部续取到 1000 行；刷新页面后标签和排序恢复。
4. 切到 English，Oracle（生产环境）：数据标签标红、统计行数 1200、排序；右键包体 → View DDL 显示 PACKAGE BODY。
5. 经接口核对 5 个库各种对象的 DDL 和关联查询：Oracle、达梦的表 DDL 末尾有索引和注释；MySQL 系不支持的种类返回 400 / 404 / 422。
6. 删除测试对象和测试连接（系统凭据中的密码随之删除），停掉服务。

Expected: 全部通过（后端 377 个测试，前端 31 个测试文件、201 个测试）。

- [ ] **Step 14: 提交**

```bash
git add crates/dblens-api/src/ws.rs crates/dblens-api/tests/api.rs docs/notes/m0-4b-api.md docs/notes/m1c2-object-actions.md docs/superpowers/specs/2026-10-25-m1-workbench-design.md web/src/features/data/DataView.tsx web/src/features/editor/QueryView.tsx web/src/features/editor/runner.test.ts web/src/features/editor/runner.ts web/src/features/tree/ObjectTree.tsx web/src/features/tree/nodes.test.ts web/src/features/tree/nodes.ts
git commit -m "fix: 浏览器实测的修正；M1c2 说明文档" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```
