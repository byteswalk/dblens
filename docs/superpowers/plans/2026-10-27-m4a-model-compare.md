# M4a 模型与比对 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 模型目录（`model.yaml` + 每表一个 YAML 文件，可放进 git）、从库反向生成模型、在模型中用设计器编辑表、模型与库或库与库的三层比对、同步到库（脚本预览后执行）或写回模型。

**Architecture:** 比对、写回和同步脚本在 `dblens-dialect::compare`（不做 IO，复用 M3 的 `diff_table`、`design_sql` 和重建）；模型目录的读写和打开的模型在 `dblens-service::model`；比对与同步的编排在 `dblens-service::compare`，执行复用 M3 的执行器（改为按段执行）；接口 `/models`、`/compare` 只在页面和服务在同一台机器上时可用；前端新增模型面板、比对标签，设计器增加模型模式。

**Tech Stack:** Rust（axum、tokio、serde、serde-saphyr、insta）；React 19 + TypeScript、antd 6、Monaco、zustand、TanStack Query、Vitest。

**Spec:** `docs/superpowers/specs/2026-10-27-m4a-model-compare-design.md`

## Global Constraints

- 模型绑定一种数据库类型（类型用原生写法）；模型目录 `model.yaml` + `tables/<schema>/<表名>.yaml`；模型只有表；可以有多个 schema，比对一次一对 schema，名字可以不同。
- 比对任意两边（模型或库），结果分表、列、约束和索引三层，可勾选；同步到库（脚本 + 执行）或写回模型。
- 模型文件写法为 git 而定：字段顺序固定，不认识的字段原样保留，写文件先写临时文件再改名；格式比程序新时只读。
- 模型目录是服务所在机器上的路径：只有桌面程序和只监听本机地址的浏览器模式可以使用模型。
- 依赖只用正式发布版，跟随最新；新版发布满 7 天再升级。
- 前端给用户看的文字都用词条（中英文），英文区分单复数；后端错误文字用 `msg!` 词条（`src` 中的中文字面量只能出现在 `#[cfg(test)]` 之后）；日志不翻译。
- 代码注释用中文；文件 UTF-8、LF 换行；前端代码风格：无分号、单引号、两格缩进。
- cargo 命令加 `--config "source.mirror.registry='sparse+https://rsproxy.cn/index/'"`（clippy 写在 `clippy` 之后），并设 `CARGO_INCREMENTAL=0`；后端测试在 `DBLENS_LANG=en` 下运行。
- 前端工具用 PowerShell 执行（`npm`/`npx` 在本机不可用，用 `pnpm exec`）。
- 测试数据库的密码只在命令行中以内联环境变量传入，不写进仓库、报告或提交信息。

## 与设计文档的差异（写计划时实测后调整）

- `dblens-server` 只监听本机地址时 `local_files` 为真：导出也可以直接写到页面给出的路径。
- MySQL 家族不查询 `lower_case_table_names`，由比对选项"名字不区分大小写"决定。
- 新建模型时不选版本（按最新版本）；反向生成的模型带库的版本。
- 外键的默认动作（`NO ACTION`、MySQL 家族的 `RESTRICT`）与不写视为相同。
- 比对结果另带两边的表，界面中对照显示、计算依赖提示。
- 结果树中一张表的变更平铺列出，没有再按类别分组。
- 同步脚本另存为：桌面程序经 `POST /compare/script` 写到 `.sql` 文件，浏览器模式下载。
- 设计器的模型模式中"SQL 预览"换成"变更"子标签，"与库比对"在工具栏。

## 执行说明

任务 2–9 的代码都已在临时副本中逐任务实现并通过检查（含 11 个实库目标的模型测试和浏览器实测），执行时按顺序重放。新文件给出全文，修改的文件给出 diff（基于上一个任务完成后的状态，可直接 `git apply`）。`Cargo.lock` 的变化由 cargo 生成；`web/pnpm-lock.yaml` 不变。

## 文件结构

```text
crates/dblens-dialect/src/compare/{mod.rs, normalize.rs, apply.rs, sync.rs, tests.rs, snapshots/}（新增）; src/{lib.rs, design/mod.rs}
crates/dblens-service/src/model/{mod.rs, file.rs, store.rs, ops.rs, snapshots/}（新增）; src/compare.rs（新增）; src/{lib.rs, service.rs, task.rs, design.rs}; tests/live.rs
crates/dblens-api/src/models.rs（新增）, src/lib.rs, tests/models.rs（新增）; crates/dblens-server/src/main.rs
crates/dblens-desktop/src/{main.rs, shell.rs}
crates/dblens-core/locales/app.json; Cargo.toml; crates/dblens-service/Cargo.toml
web/src/features/model/（新增）; features/compare/（新增）; features/design/{source.ts（新增）, DesignView.tsx, ForeignKeysPane.tsx, OptionsPane.tsx}
web/src/features/editor/{tabs.ts, tabs.test.ts, Workspace.tsx, actions.tsx}; features/tree/ObjectTree.tsx; app/{Shell.tsx, StatusBar.tsx, TitleBar.tsx, menus.ts, menus.test.ts, ui.ts, shell.css}
web/src/shared/{api.ts, shell.ts}; locales/{zh-CN,en}/{model.json（新增）, app.json, tree.json}; locales/index.ts
docs/notes/{m4a-model-compare.md（新增）, m0-4b-api.md}; docs/superpowers/specs/2026-10-22-dblens-design.md
```

---

### Task 1: 依赖复查

按依赖原则（M1b2 设计文档 3.1 节）复查前端和后端的全部直接依赖。本里程碑新增一个直接依赖 `serde-saphyr`（YAML 读写，任务 3 加入）：2.0.0 于 2026-10-13 发布，写计划时已满 7 天；纯 Rust、维护中，`serde_yaml` 已停止维护。只开 `serialize`、`deserialize` 两个 feature。

**Files:** 视复查结果而定（`web/package.json`、`Cargo.toml` 及锁文件）。

- [ ] **Step 1: 运行复查脚本**

本机经代理上网，Node 的 `fetch` 默认不读 `HTTPS_PROXY`，必须先设 `NODE_USE_ENV_PROXY=1`，否则全部查询失败，最后仍显示"0 个可升级"。

```powershell
$env:PATH = "D:\Environments\nvm4w\nodejs;$env:PATH"; $env:NODE_USE_ENV_PROXY = '1'; $env:HTTPS_PROXY = 'http://127.0.0.1:6789'; node scripts/check-deps.mjs
```

Expected：输出中没有"查询失败"。以执行当天的结果为准；`serde-saphyr` 如有更新的 2.x 正式版且发布满 7 天，任务 3 中用新版本（检查 `SerializerOptions` 的 `prefer_block_scalars` 字段、`to_string_with_options`、`from_str` 仍可用）。

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

### Task 2: 方言：结构比对、写回与同步脚本

新模块 `crates/dblens-dialect/src/compare/`（不做 IO）：
- `mod.rs`：`CompareRenames`（源 → 目标；列按源的表名分组）、`CompareOptions`（忽略注释、忽略列顺序、忽略系统生成的名字（默认是）、不区分大小写）、`CompareSide`、`TableStatus`、`SchemaItem`（编号）、`SchemaChange`、`TableDiff`、`RenameSuggestion`、`SchemaDiff`；`compare_schemas`：按名字和给出的改名配对，其余按列名的 Jaccard 系数 ≥ 0.7 且列数差不超过 2 推测表的改名（只作为建议），配对的表整理后用 M3 的 `diff_table` 比对；删一列加一列、类型相同、位置相差不超过 1 的推测为列的改名。
- `normalize.rs`：外键引用本 schema 时去掉 schema；目标中改了名的表按源的名字引用；按选项去掉注释、按目标的顺序排列共有的列、对齐系统生成的约束名和索引名（Oracle / 达梦的 `SYS_C…`，MySQL 的 `PRIMARY`、`<表>_ibfk_N`，PG 家族的 `<表>_pkey` 等）；PG 默认值去掉类型转换和外层括号；同义的类型名（`int4` 与 `integer` 等）视为相同。外键的默认动作（`NO ACTION`、MySQL 家族的 `RESTRICT`）与不写视为相同在任务 7 加入。
- `apply.rs`：`apply_changes`：把变更应用到表结构（列改名时同步约束、索引、自引用外键；重新编号）。
- `sync.rs`：`sync_rebuild_tables`（库不支持的改动要重建的表）、`sync_script`：删外键 → 删表 → 表改名 → 逐表修改（`design_sql`，一张表的改动一起生成；不支持的走 M3 重建）→ 建表（不带外键）→ 加外键。
- `design/mod.rs` 把 `diff` 模块改为 `pub(crate)`，测试中导出 `fixtures`。

后端词条加 `compare.apply_missing`。

快照文件（`snapshots/`）随本任务给出；改了生成逻辑时用 `INSTA_UPDATE=always` 重新生成并逐个审查差异。

**Files:**
- Modify: `crates/dblens-core/locales/app.json`
- Create: `crates/dblens-dialect/src/compare/apply.rs`
- Create: `crates/dblens-dialect/src/compare/mod.rs`
- Create: `crates/dblens-dialect/src/compare/normalize.rs`
- Create: `crates/dblens-dialect/src/compare/snapshots/dblens_dialect__compare__tests__sync_mysql.snap`
- Create: `crates/dblens-dialect/src/compare/snapshots/dblens_dialect__compare__tests__sync_pg.snap`
- Create: `crates/dblens-dialect/src/compare/sync.rs`
- Create: `crates/dblens-dialect/src/compare/tests.rs`
- Modify: `crates/dblens-dialect/src/design/mod.rs`
- Modify: `crates/dblens-dialect/src/lib.rs`

**Interfaces:**
- Consumes：M3 的 `diff_table`、`design_sql`、`design_unsupported`、`rebuild`、`RebuildContext`。
- Produces（`dblens_dialect::compare`）：
  - `compare_schemas(d: &Dialect, source: &CompareSide, target: &CompareSide, renames: &CompareRenames, opts: &CompareOptions) -> Result<SchemaDiff>`；
  - `apply_changes(table: &Table, changes: &[TableChange]) -> Result<Table>`；
  - `sync_rebuild_tables(d, diff, selected: &BTreeSet<u32>) -> Vec<String>`、`sync_script(d, schema, diff, selected, targets: &[Table], contexts: &BTreeMap<String, RebuildContext>) -> Result<SyncScript>`；
  - `SyncScript { steps: Vec<SyncStep>, refused: Vec<(String, Msg)>, warnings: Vec<Msg>, transactional: bool }`，`SyncStep::{Sql { sql, items }, Rebuild { table, rebuild, items }}`。

- [ ] **Step 1: 新建 `crates/dblens-dialect/src/compare/tests.rs`**

```rust
use std::collections::{BTreeMap, BTreeSet};

use dblens_core::schema::{ForeignKey, Index, IndexColumn, PrimaryKey, Table};

use super::*;
use crate::Variant;
use crate::design::fixtures::{col, table};

fn side<'a>(schema: &'a str, tables: &'a [Table]) -> CompareSide<'a> {
    CompareSide { schema, tables }
}

fn pg() -> Dialect {
    Dialect::new(Variant::Postgres)
}

fn kinds(t: &TableDiff) -> Vec<String> {
    t.items
        .iter()
        .map(|i| match &i.change {
            SchemaChange::DropTable { name } => format!("drop_table {name}"),
            SchemaChange::RenameTable { from, to } => format!("rename_table {from} {to}"),
            SchemaChange::Table { change } => {
                serde_json::to_value(change).unwrap()["kind"].as_str().unwrap().to_string()
            }
        })
        .collect()
}

fn customers(name: &str) -> Table {
    Table {
        name: name.into(),
        columns: vec![col("id", "integer"), col("name", "varchar(50)"), col("phone", "varchar(20)")],
        primary_key: Some(PrimaryKey { name: None, columns: vec!["id".into()] }),
        ..Default::default()
    }
}

#[test]
fn pairs_tables_and_suggests_renames() {
    let pg = pg();
    let mut src_orders = table(["integer", "varchar(20)", "numeric(10,2)"]);
    src_orders.columns[2].name = "total".into();
    src_orders.checks[0].expression = "total >= 0".into();
    src_orders.indexes[0].columns[0].name = "total".into();
    let source = vec![src_orders, customers("clients"), customers("only_source")];
    let target = vec![table(["integer", "varchar(20)", "numeric(10,2)"]), customers("customers")];
    let opts = CompareOptions { ignore_generated_names: false, ..Default::default() };
    let d = compare_schemas(&pg, &side("s", &source), &side("t", &target), &CompareRenames::default(), &opts)
        .unwrap();
    // 没采用改名：clients 新增、customers 删除，只给出建议
    let status: Vec<_> = d.tables.iter().map(|t| (t.source.clone(), t.target.clone(), t.status)).collect();
    assert_eq!(
        status,
        [
            (Some("orders".into()), Some("orders".into()), TableStatus::Changed),
            (Some("clients".into()), None, TableStatus::Added),
            (Some("only_source".into()), None, TableStatus::Added),
            (None, Some("customers".into()), TableStatus::Removed),
        ]
    );
    assert_eq!(
        d.suggestions,
        [
            RenameSuggestion { table: None, source: "clients".into(), target: "customers".into() },
            RenameSuggestion {
                table: Some("orders".into()),
                source: "total".into(),
                target: "amount".into()
            },
        ]
    );
    // 采用改名后重新比对
    let renames = CompareRenames {
        tables: [("clients".into(), "customers".into())].into(),
        columns: [("orders".into(), [("total".into(), "amount".into())].into())].into(),
    };
    let d = compare_schemas(&pg, &side("s", &source), &side("t", &target), &renames, &opts).unwrap();
    let clients = d.tables.iter().find(|t| t.source.as_deref() == Some("clients")).unwrap();
    assert_eq!(
        (clients.status, kinds(clients)),
        (TableStatus::Renamed, vec!["rename_table customers clients".to_string()])
    );
    let orders = &d.tables[0];
    assert_eq!(kinds(orders), ["drop_check", "rename_column", "add_check"]);
    assert!(d.suggestions.is_empty(), "{:?}", d.suggestions);
    // 编号唯一、连续
    let ids: Vec<u32> = d.tables.iter().flat_map(|t| t.items.iter().map(|i| i.id)).collect();
    assert_eq!(ids, (1..=ids.len() as u32).collect::<Vec<_>>());
    assert!(matches!(d.item(1), Some((t, _)) if t.source.as_deref() == Some("orders")));
}

#[test]
fn options_and_equivalent_spellings() {
    let pg = pg();
    let base = table(["integer", "varchar(20)", "numeric(10,2)"]);
    let mut target = base.clone();
    // 库读回的写法：类型同义、默认值带类型转换、系统生成的名字、外键带本 schema
    target.columns[0].data_type.native = "int4".into();
    target.columns[1].default = Some("'x'::character varying".into());
    target.primary_key.as_mut().unwrap().name = Some("orders_pkey".into());
    target.uniques[0].name = Some("orders_code_key".into());
    target.foreign_keys[0].ref_schema = Some("live".into());
    target.checks[0].expression = "(amount >= (0)::numeric)".into();
    let mut source = base.clone();
    source.columns[1].default = Some("'x'".into());
    source.primary_key.as_mut().unwrap().name = None;
    source.uniques[0].name = None;
    source.checks[0].expression = "(amount >= (0)::numeric)".into();
    source.comment = Some("another comment".into());
    source.columns.swap(1, 2);
    let (s, t) = (vec![source], vec![target]);
    let all = CompareOptions { ignore_comments: true, ignore_column_order: true, ..Default::default() };
    let d = compare_schemas(&pg, &side("model", &s), &side("live", &t), &CompareRenames::default(), &all)
        .unwrap();
    assert_eq!(d.tables[0].status, TableStatus::Same, "{:?}", kinds(&d.tables[0]));
    let none = CompareOptions { ignore_generated_names: false, ..Default::default() };
    let d = compare_schemas(&pg, &side("model", &s), &side("live", &t), &CompareRenames::default(), &none)
        .unwrap();
    // 主键没有名字时与任何名字都算相同（M3 的规则），只有唯一约束的名字不同
    let k = kinds(&d.tables[0]);
    for want in ["set_table_comment", "move_column", "drop_unique", "add_unique"] {
        assert!(k.iter().any(|x| x == want), "{want}: {k:?}");
    }
}

#[test]
fn generated_names_by_family() {
    use normalize::is_generated;
    let d = |v| Dialect::new(v);
    assert!(is_generated(&d(Variant::Oracle), "T", "SYS_C0012345"));
    assert!(!is_generated(&d(Variant::Oracle), "T", "PK_T"));
    assert!(is_generated(&d(Variant::MySql), "orders", "PRIMARY"));
    assert!(is_generated(&d(Variant::MySql), "orders", "orders_ibfk_2"));
    assert!(is_generated(&d(Variant::Postgres), "orders", "orders_code_key"));
    assert!(!is_generated(&d(Variant::Postgres), "orders", "uk_code"));
    assert!(is_generated(&d(Variant::Dameng), "T", "CONS134218790"));
}

/// 把比对出的变更应用到目标上，再比对应当没有差异（`apply_changes` 与 `diff_table` 互逆）。
#[test]
fn apply_is_inverse_of_diff() {
    let pg = pg();
    let target = table(["integer", "varchar(20)", "numeric(10,2)"]);
    for source in [
        crate::design::fixtures::everything(&target, "varchar(40)"),
        crate::design::fixtures::alter_only(&target, "numeric(12,2)"),
        crate::design::fixtures::move_first(&target),
    ]
    .into_iter()
    .map(|diff| apply_changes(&target, &diff.changes).unwrap())
    {
        let applied = apply_changes(
            &target,
            &crate::design::diff_table(Some(&target), &source, &Default::default()).unwrap().changes,
        );
        let again = crate::design::diff_table(Some(&applied.unwrap()), &source, &Default::default()).unwrap();
        assert!(again.changes.is_empty(), "{:?}", again.changes);
        let (s, t) = (vec![source.clone()], vec![target.clone()]);
        let d = compare_schemas(
            &pg,
            &side("a", &s),
            &side("a", &t),
            &CompareRenames::default(),
            &CompareOptions::default(),
        )
        .unwrap();
        let changes: Vec<_> = d.tables[0]
            .items
            .iter()
            .filter_map(|i| {
                if let SchemaChange::Table { change } = &i.change {
                    Some(change.as_ref().clone())
                } else {
                    None
                }
            })
            .collect();
        let written = apply_changes(&target, &changes).unwrap();
        let d2 = compare_schemas(
            &pg,
            &side("a", &s),
            &side("a", &[written]),
            &CompareRenames::default(),
            &CompareOptions::default(),
        )
        .unwrap();
        assert_eq!(d2.changed(), 0, "{:?}", d2.tables);
    }
    // 引用的列不存在
    let bad = apply_changes(&target, &[crate::design::TableChange::DropColumn { name: "nope".into() }]);
    assert!(bad.is_err());
}

fn fk(name: &str, col: &str, table: &str) -> ForeignKey {
    ForeignKey {
        name: Some(name.into()),
        columns: vec![col.into()],
        ref_schema: None,
        ref_table: table.into(),
        ref_columns: vec!["id".into()],
        on_delete: None,
        on_update: None,
    }
}

/// 一组覆盖各段的改动：orders 换外键、改列；items 新建并带外键；old 删除；cust → customers 改名；
/// moved 调整列顺序（PG 要重建）。
fn sync_case() -> (Vec<Table>, Vec<Table>) {
    let mut orders_t = customers("orders");
    orders_t.foreign_keys = vec![fk("fk_old", "name", "cust")];
    let mut orders_s = customers("orders");
    orders_s.columns[1].data_type.native = "varchar(80)".into();
    orders_s.foreign_keys = vec![fk("fk_new", "phone", "customers")];
    let mut items = customers("items");
    items.foreign_keys = vec![fk("fk_items_orders", "id", "orders")];
    let moved_t = customers("moved");
    let mut moved_s = customers("moved");
    moved_s.columns.swap(1, 2);
    moved_s.indexes = vec![Index {
        name: "ix_moved_phone".into(),
        columns: vec![IndexColumn { name: "phone".into(), descending: false }],
        unique: false,
        method: None,
    }];
    let source = vec![orders_s, items, customers("customers"), moved_s];
    let target = vec![orders_t, customers("old"), customers("cust"), moved_t];
    (source, target)
}

#[test]
fn sync_script_orders_phases_and_rebuilds() {
    let pg = pg();
    let (source, target) = sync_case();
    let renames =
        CompareRenames { tables: [("customers".into(), "cust".into())].into(), ..Default::default() };
    let d = compare_schemas(
        &pg,
        &side("app", &source),
        &side("app", &target),
        &renames,
        &CompareOptions::default(),
    )
    .unwrap();
    let all: BTreeSet<u32> = d.tables.iter().flat_map(|t| t.items.iter().map(|i| i.id)).collect();
    assert_eq!(sync_rebuild_tables(&pg, &d, &all), ["moved"]);
    let script = sync_script(&pg, "app", &d, &all, &target, &BTreeMap::new()).unwrap();
    assert!(script.transactional && script.refused.is_empty());
    let text: Vec<String> = script
        .steps
        .iter()
        .map(|s| match s {
            SyncStep::Sql { sql, items } => format!("{items:?} {sql}"),
            SyncStep::Rebuild { table, items, rebuild } => {
                format!("{items:?} rebuild {table} ({} steps)", rebuild.steps.len())
            }
        })
        .collect();
    insta::assert_snapshot!("sync_pg", text.join("\n"));
    // 只勾选一部分：没勾选的不出现
    let only: BTreeSet<u32> = d.tables[0].items.iter().map(|i| i.id).collect();
    let part = sync_script(&pg, "app", &d, &only, &target, &BTreeMap::new()).unwrap();
    assert!(
        part.steps.iter().all(|s| matches!(s, SyncStep::Sql { sql, .. } if sql.contains("orders"))),
        "{:?}",
        part.steps
    );
}

#[test]
fn sync_script_mysql_keeps_one_alter_per_table_rules() {
    let my = Dialect::new(Variant::MySql).with_version(Some(80040));
    let target = table(["BIGINT", "VARCHAR(20)", "DECIMAL(10,2)"]);
    let source =
        apply_changes(&target, &crate::design::fixtures::everything(&target, "VARCHAR(50)").changes).unwrap();
    let renames = CompareRenames {
        columns: [("orders".into(), [("order_code".into(), "code".into())].into())].into(),
        ..Default::default()
    };
    let (s, t) = (vec![source], vec![target]);
    let d = compare_schemas(&my, &side("shop", &s), &side("shop", &t), &renames, &CompareOptions::default())
        .unwrap();
    let all: BTreeSet<u32> = d.tables.iter().flat_map(|t| t.items.iter().map(|i| i.id)).collect();
    let script = sync_script(&my, "shop", &d, &all, &t, &BTreeMap::new()).unwrap();
    let text: Vec<String> = script
        .steps
        .iter()
        .map(
            |s| if let SyncStep::Sql { sql, items } = s { format!("{items:?} {sql}") } else { String::new() },
        )
        .collect();
    insta::assert_snapshot!("sync_mysql", text.join("\n"));
}
```

- [ ] **Step 2: 修改 `crates/dblens-core/locales/app.json`**

```diff
--- a/crates/dblens-core/locales/app.json
+++ b/crates/dblens-core/locales/app.json
@@ -17,6 +17,7 @@
   "changes.not_unique": {"zh-CN": "第 %{n} 个改动影响了 %{count} 行：行标识不唯一", "en": "Change %{n} affected %{count} rows: the row identity is not unique"},
   "changes.rolled_back": {"zh-CN": "已回滚（同一次提交中的其他改动失败）", "en": "Rolled back (another change in the same commit failed)"},
   "changes.unknown_column": {"zh-CN": "列 %{column} 不存在", "en": "Column %{column} does not exist"},
+  "compare.apply_missing": {"zh-CN": "找不到 %{name}：模型中的表与比对时不同，请重新比对", "en": "%{name} was not found: the model table differs from the compared one; compare again"},
   "ddl.truncated": {"zh-CN": "DDL 超过 64 KB，以上只是开头部分", "en": "The DDL exceeds 64 KB; only the beginning is shown above"},
   "ddl.unsupported": {"zh-CN": "该数据库不支持查看 %{kind} 的 DDL", "en": "This database does not support viewing the DDL of %{kind}"},
   "design.bad_rename": {"zh-CN": "列 %{name} 的原名不对", "en": "Column %{name} has an invalid original name"},
```

- [ ] **Step 3: 新建 `crates/dblens-dialect/src/compare/apply.rs`**

```rust
//! 把变更应用到表结构上（M4a 设计文档 6.2 节）：同步的目标是模型时，按勾选的变更改写模型中的表。
//! 与 `diff_table` 互逆：`apply_changes(原表, diff_table(原表, 草稿))` 与草稿再比对没有差异。

use dblens_core::schema::{Column, Table};
use dblens_core::{Error, Result, msg};

use crate::design::TableChange as C;

/// 依次应用变更。变更引用的列、约束在表中找不到时返回 `invalid`。
pub fn apply_changes(table: &Table, changes: &[C]) -> Result<Table> {
    let mut t = table.clone();
    for ch in changes {
        apply_one(&mut t, ch)?;
    }
    for (i, c) in t.columns.iter_mut().enumerate() {
        c.ordinal = (i + 1) as u32;
    }
    Ok(t)
}

fn missing(what: &str) -> Error {
    Error::invalid(msg!("compare.apply_missing", name = what))
}

fn position(t: &Table, name: &str) -> Result<usize> {
    t.columns.iter().position(|c| c.name == name).ok_or_else(|| missing(name))
}

/// 放在 `after` 之后（None 为最前）。
fn insert_after(t: &mut Table, column: Column, after: &Option<String>) -> Result<()> {
    let at = match after {
        None => 0,
        Some(a) => position(t, a)? + 1,
    };
    t.columns.insert(at, column);
    Ok(())
}

fn rename_refs(t: &mut Table, from: &str, to: &str) {
    let fix =
        |cols: &mut Vec<String>| cols.iter_mut().filter(|c| *c == from).for_each(|c| *c = to.to_string());
    if let Some(pk) = &mut t.primary_key {
        fix(&mut pk.columns);
    }
    t.uniques.iter_mut().for_each(|u| fix(&mut u.columns));
    let own = t.name.clone();
    for f in &mut t.foreign_keys {
        fix(&mut f.columns);
        if f.ref_table == own && f.ref_schema.is_none() {
            fix(&mut f.ref_columns);
        }
    }
    for x in &mut t.indexes {
        x.columns.iter_mut().filter(|c| c.name == from).for_each(|c| c.name = to.to_string());
    }
}

fn apply_one(t: &mut Table, ch: &C) -> Result<()> {
    match ch {
        C::CreateTable { table } => *t = table.clone(),
        C::SetTableComment { comment } => t.comment = comment.clone(),
        C::AddColumn { column, after } => insert_after(t, column.clone(), after)?,
        C::DropColumn { name } => {
            let i = position(t, name)?;
            t.columns.remove(i);
        }
        C::RenameColumn { from, to, .. } => {
            let i = position(t, from)?;
            t.columns[i].name = to.clone();
            rename_refs(t, from, to);
        }
        C::AlterColumn { to, .. } => {
            let i = position(t, &to.name)?;
            t.columns[i] = Column { ordinal: t.columns[i].ordinal, ..to.clone() };
        }
        C::MoveColumn { column, after } => {
            let i = position(t, &column.name)?;
            let c = t.columns.remove(i);
            insert_after(t, c, after)?;
        }
        C::DropPrimaryKey { .. } => t.primary_key = None,
        C::AddPrimaryKey { key } => t.primary_key = Some(key.clone()),
        C::DropUnique { unique } => {
            let i = t
                .uniques
                .iter()
                .position(|u| u.name == unique.name && (unique.name.is_some() || u.columns == unique.columns))
                .ok_or_else(|| missing(unique.name.as_deref().unwrap_or("UNIQUE")))?;
            t.uniques.remove(i);
        }
        C::AddUnique { unique } => t.uniques.push(unique.clone()),
        C::DropCheck { check } => {
            let i = t
                .checks
                .iter()
                .position(|c| {
                    c.name == check.name && (check.name.is_some() || c.expression == check.expression)
                })
                .ok_or_else(|| missing(check.name.as_deref().unwrap_or("CHECK")))?;
            t.checks.remove(i);
        }
        C::AddCheck { check } => t.checks.push(check.clone()),
        C::DropForeignKey { key } => {
            let i = t
                .foreign_keys
                .iter()
                .position(|f| f.name == key.name && (key.name.is_some() || f.columns == key.columns))
                .ok_or_else(|| missing(key.name.as_deref().unwrap_or("FOREIGN KEY")))?;
            t.foreign_keys.remove(i);
        }
        C::AddForeignKey { key } => t.foreign_keys.push(key.clone()),
        C::DropIndex { index } => {
            let i =
                t.indexes.iter().position(|x| x.name == index.name).ok_or_else(|| missing(&index.name))?;
            t.indexes.remove(i);
        }
        C::AddIndex { index } => t.indexes.push(index.clone()),
        C::RenameIndex { from, index } => {
            let i = t.indexes.iter().position(|x| &x.name == from).ok_or_else(|| missing(from))?;
            t.indexes[i] = index.clone();
        }
    }
    Ok(())
}
```

- [ ] **Step 4: 新建 `crates/dblens-dialect/src/compare/mod.rs`**

```rust
//! 结构比对（M4a 设计文档第 5 节）：两组表（模型或数据库的一个 schema）配对后逐表比对，
//! 结果描述"怎样修改目标才与源一致"。不做任何 IO。
//!
//! 表的配对：名字相同、调用方给出的改名（模型记下的、用户确认的推测），其余按列名的重合程度推测改名，
//! 推测的改名只作为建议列出，不采用。配上对的表用 M3 的 `diff_table` 比对。

mod apply;
mod normalize;
mod sync;

use std::collections::BTreeMap;

use dblens_core::Result;
use dblens_core::schema::Table;
use serde::{Deserialize, Serialize};

use crate::Dialect;
use crate::design::diff::same_type;
use crate::design::{Renames, TableChange, diff_table};

pub use apply::apply_changes;
pub use sync::{SyncScript, SyncStep, sync_rebuild_tables, sync_script};

/// 已采用的改名：源的名字 → 目标的名字。表的改名按表名，列的改名按（源的）表名分组。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompareRenames {
    #[serde(default)]
    pub tables: BTreeMap<String, String>,
    #[serde(default)]
    pub columns: BTreeMap<String, BTreeMap<String, String>>,
}

/// 比对选项。`case_insensitive`：表名、列名不区分大小写（MySQL 家族的 `lower_case_table_names` 不为 0 时）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompareOptions {
    #[serde(default)]
    pub ignore_comments: bool,
    #[serde(default)]
    pub ignore_column_order: bool,
    #[serde(default = "yes")]
    pub ignore_generated_names: bool,
    #[serde(default)]
    pub case_insensitive: bool,
}

fn yes() -> bool {
    true
}

impl Default for CompareOptions {
    fn default() -> Self {
        Self {
            ignore_comments: false,
            ignore_column_order: false,
            ignore_generated_names: true,
            case_insensitive: false,
        }
    }
}

/// 比对的两边：表和所在 schema 的名字（外键引用本 schema 时按两边各自的名字换算）。
pub struct CompareSide<'a> {
    pub schema: &'a str,
    pub tables: &'a [Table],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TableStatus {
    /// 只有源有：要在目标中新建。
    Added,
    /// 只有目标有：要从目标中删除。
    Removed,
    Renamed,
    Changed,
    Same,
}

/// 一项可以勾选的变更。`id` 在整个比对结果中唯一，勾选和同步都按它。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SchemaItem {
    pub id: u32,
    pub change: SchemaChange,
}

/// 表这一层的变更（建表、删表、改名）和表内的变更（M3 的变更列表）。
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SchemaChange {
    DropTable {
        name: String,
    },
    RenameTable {
        from: String,
        to: String,
    },
    /// 表内的变更；新建表是其中的 `CreateTable`。
    Table {
        change: Box<TableChange>,
    },
}

/// 一张表的比对结果。`source` / `target` 是两边的表名（新增时没有目标，删除时没有源）。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TableDiff {
    pub status: TableStatus,
    pub source: Option<String>,
    pub target: Option<String>,
    pub items: Vec<SchemaItem>,
    /// 新列都在最后、保留的列相对顺序不变（不能指定列位置的库据此判断要不要重建）。
    pub appended: bool,
}

/// 推测的改名：`table` 为 None 时是表的改名，否则是该表（源的名字）中列的改名。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RenameSuggestion {
    pub table: Option<String>,
    pub source: String,
    pub target: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SchemaDiff {
    pub tables: Vec<TableDiff>,
    pub suggestions: Vec<RenameSuggestion>,
}

impl SchemaDiff {
    /// 按编号找变更，并给出它所在的表。
    pub fn item(&self, id: u32) -> Option<(&TableDiff, &SchemaItem)> {
        self.tables.iter().find_map(|t| t.items.iter().find(|i| i.id == id).map(|i| (t, i)))
    }

    /// 有差异的表的数量。
    pub fn changed(&self) -> usize {
        self.tables.iter().filter(|t| t.status != TableStatus::Same).count()
    }
}

/// 推测表改名的门槛：列名的 Jaccard 系数。
const TABLE_SIMILARITY: f64 = 0.7;
/// 推测表改名时两表列数之差的上限。
const TABLE_COLUMN_SLACK: usize = 2;

fn same_name(a: &str, b: &str, opts: &CompareOptions) -> bool {
    if opts.case_insensitive { a.eq_ignore_ascii_case(b) } else { a == b }
}

/// 比对两组表。`renames` 中的名字对不上时返回 `invalid`。
pub fn compare_schemas(
    d: &Dialect,
    source: &CompareSide,
    target: &CompareSide,
    renames: &CompareRenames,
    opts: &CompareOptions,
) -> Result<SchemaDiff> {
    // 表的配对：源的下标 → 目标的下标
    let mut pairs: Vec<(Option<usize>, Option<usize>)> = Vec::new();
    let mut used = vec![false; target.tables.len()];
    let mut renamed = vec![false; source.tables.len()];
    for (si, s) in source.tables.iter().enumerate() {
        let want = renames.tables.get(&s.name).map(|t| (t.as_str(), true));
        let (name, by_rename) = want.unwrap_or((s.name.as_str(), false));
        let found =
            target.tables.iter().enumerate().position(|(ti, t)| !used[ti] && same_name(&t.name, name, opts));
        match found {
            Some(ti) => {
                used[ti] = true;
                renamed[si] = by_rename && !same_name(&s.name, &target.tables[ti].name, opts);
                pairs.push((Some(si), Some(ti)));
            }
            None => pairs.push((Some(si), None)),
        }
    }
    pairs.extend(used.iter().enumerate().filter(|(_, u)| !**u).map(|(ti, _)| (None, Some(ti))));

    let suggestions = suggest_tables(source.tables, target.tables, &pairs, opts);
    let table_map: BTreeMap<String, String> = pairs
        .iter()
        .filter_map(|p| match p {
            (Some(si), Some(ti)) => Some((target.tables[*ti].name.clone(), source.tables[*si].name.clone())),
            _ => None,
        })
        .collect();

    let mut id = 0u32;
    let mut next = |change: SchemaChange| {
        id += 1;
        SchemaItem { id, change }
    };
    let mut tables = Vec::new();
    let mut column_suggestions = Vec::new();
    for (si, ti) in pairs {
        let s = si.map(|i| &source.tables[i]);
        let t = ti.map(|i| &target.tables[i]);
        let diff = match (s, t) {
            (Some(s), None) => {
                let s = normalize::source_only(s, source.schema);
                let table = TableChange::CreateTable { table: s.clone() };
                TableDiff {
                    status: TableStatus::Added,
                    source: Some(s.name.clone()),
                    target: None,
                    items: vec![next(SchemaChange::Table { change: Box::new(table) })],
                    appended: true,
                }
            }
            (None, Some(t)) => TableDiff {
                status: TableStatus::Removed,
                source: None,
                target: Some(t.name.clone()),
                items: vec![next(SchemaChange::DropTable { name: t.name.clone() })],
                appended: true,
            },
            (Some(s), Some(t)) => {
                let cols = renames.columns.get(&s.name).cloned().unwrap_or_default();
                let (s2, t2) =
                    normalize::align(d, s, t, source.schema, target.schema, &table_map, &cols, opts);
                let r = Renames { columns: cols.clone(), indexes: BTreeMap::new() };
                let diff = diff_table(Some(&t2), &s2, &r)?;
                column_suggestions.extend(suggest_columns(&s2, &t2, &diff.changes));
                let is_renamed = renamed[si.expect("配对")];
                let mut items = Vec::new();
                if is_renamed {
                    items.push(next(SchemaChange::RenameTable { from: t.name.clone(), to: s.name.clone() }));
                }
                items.extend(
                    diff.changes
                        .into_iter()
                        .map(|change| next(SchemaChange::Table { change: Box::new(change) })),
                );
                let status = match (is_renamed, items.is_empty()) {
                    (true, _) => TableStatus::Renamed,
                    (false, true) => TableStatus::Same,
                    (false, false) => TableStatus::Changed,
                };
                TableDiff {
                    status,
                    source: Some(s.name.clone()),
                    target: Some(t.name.clone()),
                    items,
                    appended: diff.appended,
                }
            }
            (None, None) => unreachable!("至少一边有表"),
        };
        tables.push(diff);
    }
    let mut suggestions = suggestions;
    suggestions.extend(column_suggestions);
    Ok(SchemaDiff { tables, suggestions })
}

/// 推测表的改名：没配上对的源表和目标表中，列名重合度够高、列数相近的。每张表至多一个建议，取重合度最高的。
fn suggest_tables(
    source: &[Table],
    target: &[Table],
    pairs: &[(Option<usize>, Option<usize>)],
    opts: &CompareOptions,
) -> Vec<RenameSuggestion> {
    let names = |t: &Table| -> Vec<String> {
        t.columns
            .iter()
            .map(|c| if opts.case_insensitive { c.name.to_lowercase() } else { c.name.clone() })
            .collect()
    };
    let only_source: Vec<usize> =
        pairs.iter().filter_map(|p| if let (Some(s), None) = p { Some(*s) } else { None }).collect();
    let only_target: Vec<usize> =
        pairs.iter().filter_map(|p| if let (None, Some(t)) = p { Some(*t) } else { None }).collect();
    let mut out = Vec::new();
    let mut taken = vec![false; target.len()];
    for si in only_source {
        let a = names(&source[si]);
        let best = only_target
            .iter()
            .filter(|ti| !taken[**ti])
            .filter(|ti| a.len().abs_diff(target[**ti].columns.len()) <= TABLE_COLUMN_SLACK)
            .map(|ti| (*ti, jaccard(&a, &names(&target[*ti]))))
            .filter(|(_, j)| *j >= TABLE_SIMILARITY)
            .max_by(|x, y| x.1.total_cmp(&y.1));
        if let Some((ti, _)) = best {
            taken[ti] = true;
            out.push(RenameSuggestion {
                table: None,
                source: source[si].name.clone(),
                target: target[ti].name.clone(),
            });
        }
    }
    out
}

fn jaccard(a: &[String], b: &[String]) -> f64 {
    let both = a.iter().filter(|x| b.contains(x)).count();
    let all = a.len() + b.len() - both;
    if all == 0 { 0.0 } else { both as f64 / all as f64 }
}

/// 推测列的改名：同一张表中删掉的一列（目标）和新增的一列（源），类型相同、位置相差不超过 1。
fn suggest_columns(source: &Table, target: &Table, changes: &[TableChange]) -> Vec<RenameSuggestion> {
    let mut out = Vec::new();
    let mut taken: Vec<&str> = Vec::new();
    for c in changes {
        let TableChange::DropColumn { name } = c else { continue };
        let Some(ti) = target.columns.iter().position(|x| &x.name == name) else { continue };
        let tc = &target.columns[ti];
        let found = changes.iter().find_map(|a| {
            let TableChange::AddColumn { column, .. } = a else { return None };
            let si = source.columns.iter().position(|x| x.name == column.name)?;
            (!taken.contains(&column.name.as_str())
                && si.abs_diff(ti) <= 1
                && same_type(&column.data_type.native, &tc.data_type.native))
            .then_some(column.name.as_str())
        });
        if let Some(to) = found {
            taken.push(to);
            out.push(RenameSuggestion {
                table: Some(source.name.clone()),
                source: to.to_string(),
                target: name.clone(),
            });
        }
    }
    out
}

#[cfg(test)]
mod tests;
```

- [ ] **Step 5: 新建 `crates/dblens-dialect/src/compare/normalize.rs`**

```rust
//! 比对前把两边的表整理成可以直接比较的形式（M4a 设计文档 5.3 节）：
//! 外键对本 schema 的引用写成"不带 schema"；目标中改了名的表按源的名字引用；
//! 按选项去掉注释、按目标的顺序排列共有的列、对齐系统生成的约束名和索引名；
//! 写法不同但含义相同的默认值和类型按目标的写法。

use std::collections::BTreeMap;

use dblens_core::schema::{Column, ForeignKey, Table};

use super::CompareOptions;
use crate::design::diff::same_type;
use crate::{Dialect, Family};

/// 外键引用本 schema 时去掉 schema，两边各自的 schema 名不同时也能比较。
fn own_schema_refs(t: &mut Table, schema: &str) {
    for f in &mut t.foreign_keys {
        if f.ref_schema.as_deref().is_some_and(|s| s == schema || s.is_empty()) {
            f.ref_schema = None;
        }
    }
}

/// 只有源有的表（要新建）：整理外键的 schema。
pub(super) fn source_only(t: &Table, schema: &str) -> Table {
    let mut t = t.clone();
    own_schema_refs(&mut t, schema);
    t
}

/// 整理一对表，返回（源，目标）；目标的名字换成源的名字（表改名在表这一层单独处理）。
/// `table_map`：目标的表名 → 源的表名（配上对的表）；`columns`：源的列名 → 目标的列名。
#[allow(clippy::too_many_arguments)]
pub(super) fn align(
    d: &Dialect,
    source: &Table,
    target: &Table,
    source_schema: &str,
    target_schema: &str,
    table_map: &BTreeMap<String, String>,
    columns: &BTreeMap<String, String>,
    opts: &CompareOptions,
) -> (Table, Table) {
    let mut s = source.clone();
    let mut t = target.clone();
    own_schema_refs(&mut s, source_schema);
    own_schema_refs(&mut t, target_schema);
    t.name = s.name.clone();
    for f in &mut t.foreign_keys {
        if f.ref_schema.is_none()
            && let Some(n) = table_map.get(&f.ref_table)
        {
            f.ref_table = n.clone();
        }
    }
    if opts.case_insensitive {
        // 名字不区分大小写时，目标中的列名、引用的表名按源的写法
        for c in &mut t.columns {
            if let Some(x) = s.columns.iter().find(|x| x.name.eq_ignore_ascii_case(&c.name)) {
                c.name = x.name.clone();
            }
        }
    }
    if opts.ignore_comments {
        for x in [&mut s, &mut t] {
            x.comment = None;
            x.columns.iter_mut().for_each(|c| c.comment = None);
        }
    }
    // 源的列名 → 目标的列名（改名）；目标中的对应列
    let target_of = |c: &Column| columns.get(&c.name).cloned().unwrap_or_else(|| c.name.clone());
    for c in &mut s.columns {
        let name = target_of(c);
        if let Some(tc) = t.columns.iter().find(|x| x.name == name) {
            if c.default.is_some() && tc.default.is_some() && same_default(d, &c.default, &tc.default) {
                c.default = tc.default.clone();
            }
            if same_type(&c.data_type.native, &tc.data_type.native)
                || same_type(
                    &canonical_type(d, &c.data_type.native),
                    &canonical_type(d, &tc.data_type.native),
                )
            {
                c.data_type = tc.data_type.clone();
            }
        }
    }
    if opts.ignore_column_order {
        // 共有的列按目标中的顺序，只有源有的列放在最后（按源中的顺序）
        let pos = |c: &Column| t.columns.iter().position(|x| x.name == target_of(c)).unwrap_or(usize::MAX);
        s.columns.sort_by_key(|c| pos(c));
    }
    if opts.ignore_generated_names {
        align_names(d, &mut s, &t);
    }
    (s, t)
}

/// 去掉库加的类型转换（`'abc'::character varying`）和多余的括号后比较默认值。
fn same_default(d: &Dialect, a: &Option<String>, b: &Option<String>) -> bool {
    let norm = |v: &Option<String>| -> String {
        let mut s = v.as_deref().unwrap_or_default().trim().to_string();
        if d.family() == Family::Postgres {
            while let Some(i) = s.rfind("::") {
                let tail = &s[i + 2..];
                if tail.chars().all(|c| c.is_ascii_alphanumeric() || " _()[],\"".contains(c)) {
                    s.truncate(i);
                    s = s.trim().to_string();
                } else {
                    break;
                }
            }
        }
        while s.starts_with('(') && s.ends_with(')') && balanced(&s[1..s.len() - 1]) {
            s = s[1..s.len() - 1].trim().to_string();
        }
        s.to_ascii_lowercase()
    };
    norm(a) == norm(b)
}

fn balanced(s: &str) -> bool {
    let mut depth = 0i32;
    for c in s.chars() {
        match c {
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

/// 同义的类型名换成同一种写法（`int` 与 `integer`、`int4` 等），用于判断两边的类型是否相同。
fn canonical_type(d: &Dialect, native: &str) -> String {
    let lower = native.trim().to_ascii_lowercase();
    let (base, rest) = match lower.find('(') {
        Some(i) => (lower[..i].trim().to_string(), lower[i..].to_string()),
        None => (lower.clone(), String::new()),
    };
    let base = match (d.family(), base.as_str()) {
        (Family::Postgres, "int" | "int4") => "integer",
        (Family::Postgres, "int8") => "bigint",
        (Family::Postgres, "int2") => "smallint",
        (Family::Postgres, "bool") => "boolean",
        (Family::Postgres, "float8") => "double precision",
        (Family::Postgres, "float4") => "real",
        (Family::Postgres, "varchar") => "character varying",
        (Family::Postgres, "char" | "bpchar") => "character",
        (Family::Postgres, "decimal") => "numeric",
        (Family::Postgres, "timestamp without time zone") => "timestamp",
        (Family::Postgres, "timestamptz") => "timestamp with time zone",
        (Family::MySql, "integer") => "int",
        (Family::MySql, "bool" | "boolean") => "tinyint(1)",
        (Family::MySql, "dec" | "numeric") => "decimal",
        (Family::Oracle | Family::Dameng, "integer" | "int") => "number(38)",
        (_, b) => b,
    }
    .to_string();
    // MySQL 8 的整数类型不报显示宽度：int(11) 与 int 相同
    let rest = if d.family() == Family::MySql
        && matches!(base.as_str(), "tinyint" | "smallint" | "mediumint" | "int" | "bigint")
        && !rest.starts_with("(1)")
    {
        rest.trim_start_matches(|c: char| c == '(' || c.is_ascii_digit() || c == ')').to_string()
    } else {
        rest
    };
    if base.contains('(') { base } else { format!("{base}{rest}") }
}

/// 系统生成的约束名、索引名：内容相同、至少一边是生成的名字时，源用目标的名字（同步时不必改名）。
fn align_names(d: &Dialect, s: &mut Table, t: &Table) {
    let generated = |n: &str| is_generated(d, &t.name, n);
    // 主键
    if let (Some(sp), Some(tp)) = (&mut s.primary_key, &t.primary_key)
        && sp.columns == tp.columns
        && sp.name != tp.name
        && (sp.name.as_deref().is_none_or(generated) || tp.name.as_deref().is_none_or(generated))
    {
        sp.name = tp.name.clone();
    }
    for u in &mut s.uniques {
        if let Some(x) = t.uniques.iter().find(|x| x.columns == u.columns)
            && u.name != x.name
            && (u.name.as_deref().is_none_or(generated) || x.name.as_deref().is_none_or(generated))
        {
            u.name = x.name.clone();
        }
    }
    for c in &mut s.checks {
        if let Some(x) = t.checks.iter().find(|x| same_check(&x.expression, &c.expression))
            && c.name != x.name
            && (c.name.as_deref().is_none_or(generated) || x.name.as_deref().is_none_or(generated))
        {
            c.name = x.name.clone();
            c.expression = x.expression.clone();
        }
    }
    for f in &mut s.foreign_keys {
        if let Some(x) = t.foreign_keys.iter().find(|x| same_fk_target(x, f))
            && f.name != x.name
            && (f.name.as_deref().is_none_or(generated) || x.name.as_deref().is_none_or(generated))
        {
            f.name = x.name.clone();
        }
    }
    for i in &mut s.indexes {
        if let Some(x) = t.indexes.iter().find(|x| x.columns == i.columns && x.unique == i.unique)
            && i.name != x.name
            && (generated(&i.name) || generated(&x.name))
        {
            i.name = x.name.clone();
        }
    }
}

fn same_fk_target(a: &ForeignKey, b: &ForeignKey) -> bool {
    a.columns == b.columns
        && a.ref_table == b.ref_table
        && a.ref_columns == b.ref_columns
        && a.ref_schema == b.ref_schema
}

/// 检查约束的表达式：库读回时常加括号、改大小写，去掉空白和最外层括号后不区分大小写比较。
fn same_check(a: &str, b: &str) -> bool {
    let norm = |s: &str| {
        let mut s: String = s.chars().filter(|c| !c.is_whitespace()).collect::<String>().to_ascii_lowercase();
        while s.starts_with('(') && s.ends_with(')') && balanced(&s[1..s.len() - 1]) {
            s = s[1..s.len() - 1].to_string();
        }
        s.replace(['`', '"'], "")
    };
    norm(a) == norm(b)
}

/// 名字是否像库自动生成的：Oracle、达梦的 `SYS_C…`、`CONS…`、`INDEX…`；MySQL 的 `PRIMARY`、`<表>_ibfk_N`、
/// `<表>_chk_N`；PG 家族的 `<表>_pkey`、`<表>_…_key`、`<表>_…_fkey`、`<表>_…_check`。
pub(super) fn is_generated(d: &Dialect, table: &str, name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    let digits = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_digit());
    match d.family() {
        Family::Oracle | Family::Dameng => {
            upper.strip_prefix("SYS_C").is_some_and(digits)
                || upper.starts_with("SYS_")
                || upper.strip_prefix("CONS").is_some_and(digits)
                || upper.strip_prefix("INDEX").is_some_and(digits)
        }
        Family::MySql => {
            let t = table.to_ascii_lowercase();
            let n = name.to_ascii_lowercase();
            upper == "PRIMARY"
                || n.strip_prefix(&format!("{t}_ibfk_")).is_some_and(digits)
                || n.strip_prefix(&format!("{t}_chk_")).is_some_and(digits)
        }
        Family::Postgres => {
            let prefix = format!("{}_", table.to_ascii_lowercase());
            let n = name.to_ascii_lowercase();
            n.starts_with(&prefix)
                && ["_pkey", "_key", "_fkey", "_check", "_idx", "_excl"].iter().any(|s| n.ends_with(s))
        }
    }
}
```

- [ ] **Step 6: 新建 `crates/dblens-dialect/src/compare/snapshots/dblens_dialect__compare__tests__sync_mysql.snap`**

```text
---
source: crates/dblens-dialect/src/compare/tests.rs
expression: "text.join(\"\\n\")"
---
[1] ALTER TABLE shop.orders DROP FOREIGN KEY fk_parent
[3] ALTER TABLE shop.orders DROP CHECK ck_amount
[4] ALTER TABLE shop.orders DROP INDEX ix_amount
[5] ALTER TABLE shop.orders DROP COLUMN amount
[6] ALTER TABLE shop.orders RENAME COLUMN code TO order_code
[7] ALTER TABLE shop.orders MODIFY COLUMN order_code VARCHAR(50) NOT NULL
[8] ALTER TABLE shop.orders ADD COLUMN note VARCHAR(50) NULL AFTER id
[2, 9] ALTER TABLE shop.orders DROP PRIMARY KEY, ADD PRIMARY KEY (id, order_code)
[10] ALTER TABLE shop.orders ADD CONSTRAINT uk_note UNIQUE (note)
[11] ALTER TABLE shop.orders ADD INDEX ix_note (note)
[13] ALTER TABLE shop.orders COMMENT = '订单表'
[12] ALTER TABLE shop.orders ADD CONSTRAINT fk_parent FOREIGN KEY (order_code) REFERENCES shop.parent (code)
```

- [ ] **Step 7: 新建 `crates/dblens-dialect/src/compare/snapshots/dblens_dialect__compare__tests__sync_pg.snap`**

```text
---
source: crates/dblens-dialect/src/compare/tests.rs
expression: "text.join(\"\\n\")"
---
[1] ALTER TABLE app.orders DROP CONSTRAINT fk_old
[8] DROP TABLE app.old
[5] ALTER TABLE app.cust RENAME TO customers
[2] ALTER TABLE app.orders ALTER COLUMN name TYPE varchar(80) USING name::varchar(80)
[6, 7] rebuild moved (7 steps)
[4] CREATE TABLE app.items (
    id integer,
    name varchar(50),
    phone varchar(20),
    PRIMARY KEY (id)
)
[3] ALTER TABLE app.orders ADD CONSTRAINT fk_new FOREIGN KEY (phone) REFERENCES app.customers (id)
[4] ALTER TABLE app.items ADD CONSTRAINT fk_items_orders FOREIGN KEY (id) REFERENCES app.orders (id)
```

- [ ] **Step 8: 新建 `crates/dblens-dialect/src/compare/sync.rs`**

```rust
//! 同步脚本（M4a 设计文档 6.1 节）：按勾选的变更修改目标 schema。
//! 顺序：删外键 → 删表 → 表改名 → 逐表修改（不支持的走 M3 重建）→ 建表（不带外键）→ 加外键。
//!
//! 要重建的表先用 `sync_rebuild_tables` 找出来，调用方读取它们的重建依赖信息后传给 `sync_script`。

use std::collections::{BTreeMap, BTreeSet};

use dblens_core::schema::Table;
use dblens_core::{Msg, Result};
use serde::Serialize;

use super::{SchemaChange, SchemaDiff, TableDiff, apply_changes};
use crate::Dialect;
use crate::design::{Diff, Rebuild, RebuildContext, RebuildOutcome, Renames, TableChange as C};

/// 脚本的一段：一组语句，或一张表的重建。`items` 是这一段对应的变更编号。
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SyncStep {
    Sql { sql: String, items: Vec<u32> },
    Rebuild { table: String, rebuild: Rebuild, items: Vec<u32> },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SyncScript {
    pub steps: Vec<SyncStep>,
    /// 不能重建的表及原因（这些表的变更没有生成语句）。
    pub refused: Vec<(String, Msg)>,
    pub warnings: Vec<Msg>,
    pub transactional: bool,
}

/// 勾选的变更：（编号，变更）。
type Picks<'a> = Vec<(u32, &'a C)>;

/// 一张表中被勾选的变更（不含表这一层的建、删、改名）。
fn picked<'a>(t: &'a TableDiff, selected: &BTreeSet<u32>) -> Picks<'a> {
    t.items
        .iter()
        .filter(|i| selected.contains(&i.id))
        .filter_map(|i| match &i.change {
            SchemaChange::Table { change } => Some((i.id, change.as_ref())),
            _ => None,
        })
        .collect()
}

fn is_fk(c: &C) -> bool {
    matches!(c, C::DropForeignKey { .. } | C::AddForeignKey { .. })
}

/// 勾选的变更要不要重建：库不支持（M3 的 `design_unsupported`）时重建。
fn needs_rebuild(d: &Dialect, t: &TableDiff, changes: &[C]) -> bool {
    if changes.is_empty() || matches!(changes.first(), Some(C::CreateTable { .. })) {
        return false;
    }
    let reorder = changes.iter().any(|c| matches!(c, C::MoveColumn { .. } | C::AddColumn { .. }));
    let diff = Diff { changes: changes.to_vec(), appended: !reorder || t.appended };
    !d.design_unsupported(&diff).is_empty()
}

/// 勾选的变更中要重建的表（目标中的名字，改名后的名字）。
pub fn sync_rebuild_tables(d: &Dialect, diff: &SchemaDiff, selected: &BTreeSet<u32>) -> Vec<String> {
    diff.tables
        .iter()
        .filter(|t| t.target.is_some() && t.source.is_some())
        .filter(|t| {
            let changes: Vec<C> = picked(t, selected).into_iter().map(|(_, c)| c.clone()).collect();
            needs_rebuild(d, t, &changes)
        })
        .filter_map(|t| t.source.clone())
        .collect()
}

/// 生成同步脚本。`targets`：目标 schema 中的表（按目标的名字）；`contexts`：要重建的表的依赖信息
/// （按改名后的名字，见 `sync_rebuild_tables`）。
pub fn sync_script(
    d: &Dialect,
    schema: &str,
    diff: &SchemaDiff,
    selected: &BTreeSet<u32>,
    targets: &[Table],
    contexts: &BTreeMap<String, RebuildContext>,
) -> Result<SyncScript> {
    let mut drop_fks = Vec::new();
    let mut drop_tables = Vec::new();
    let mut renames = Vec::new();
    let mut alters = Vec::new();
    let mut creates = Vec::new();
    let mut add_fks = Vec::new();
    let mut refused = Vec::new();
    let mut warnings = Vec::new();
    let q = |name: &str| d.qualified(Some(schema), name);
    let sql_of = |table: &str, changes: Vec<C>| -> Result<Vec<String>> {
        let diff = Diff { changes, appended: true };
        Ok(d.design_sql(schema, table, &diff)?.into_iter().map(|s| s.sql).collect())
    };

    for t in &diff.tables {
        for item in t.items.iter().filter(|i| selected.contains(&i.id)) {
            match &item.change {
                SchemaChange::DropTable { name } => drop_tables
                    .push(SyncStep::Sql { sql: format!("DROP TABLE {}", q(name)), items: vec![item.id] }),
                SchemaChange::RenameTable { from, to } => renames.push(SyncStep::Sql {
                    sql: format!("ALTER TABLE {} RENAME TO {}", q(from), d.ident(to)),
                    items: vec![item.id],
                }),
                SchemaChange::Table { change } if matches!(change.as_ref(), C::CreateTable { .. }) => {
                    let C::CreateTable { table } = change.as_ref() else { unreachable!() };
                    // 外键在所有表建好之后再加
                    let bare = Table { foreign_keys: vec![], ..table.clone() };
                    for sql in sql_of(&table.name, vec![C::CreateTable { table: bare }])? {
                        creates.push(SyncStep::Sql { sql, items: vec![item.id] });
                    }
                    for key in &table.foreign_keys {
                        for sql in sql_of(&table.name, vec![C::AddForeignKey { key: key.clone() }])? {
                            add_fks.push(SyncStep::Sql { sql, items: vec![item.id] });
                        }
                    }
                }
                SchemaChange::Table { .. } => {}
            }
        }
        let (Some(name), Some(target)) = (&t.source, &t.target) else { continue };
        let picks = picked(t, selected);
        if picks.is_empty() {
            continue;
        }
        let changes: Vec<C> = picks.iter().map(|(_, c)| (*c).clone()).collect();
        let ids: Vec<u32> = picks.iter().map(|(id, _)| *id).collect();
        if needs_rebuild(d, t, &changes) {
            let original = targets.iter().find(|x| &x.name == target).cloned().unwrap_or_default();
            let original = Table { name: name.clone(), ..original };
            let draft = apply_changes(&original, &changes)?;
            let column_renames: BTreeMap<String, String> = changes
                .iter()
                .filter_map(|c| match c {
                    C::RenameColumn { from, to, .. } => Some((to.clone(), from.clone())),
                    _ => None,
                })
                .collect();
            let r = Renames { columns: column_renames, indexes: BTreeMap::new() };
            let ctx = contexts.get(name).cloned().unwrap_or_default();
            match d.rebuild(schema, &original, &draft, &r, &ctx)? {
                RebuildOutcome::Script(rebuild) => {
                    warnings.extend(rebuild.warnings.iter().cloned());
                    alters.push(SyncStep::Rebuild { table: name.clone(), rebuild, items: ids });
                }
                RebuildOutcome::Refused { reason } => refused.push((name.clone(), reason)),
            }
            continue;
        }
        // 一张表的改动一起生成（MySQL 换主键、既修改又挪动的列要合成一条）；外键分出去放在前后两段
        let groups: [(&mut Vec<SyncStep>, Picks); 3] = [
            (
                &mut drop_fks,
                picks.iter().filter(|(_, c)| matches!(c, C::DropForeignKey { .. })).copied().collect(),
            ),
            (&mut alters, picks.iter().filter(|(_, c)| !is_fk(c)).copied().collect()),
            (
                &mut add_fks,
                picks.iter().filter(|(_, c)| matches!(c, C::AddForeignKey { .. })).copied().collect(),
            ),
        ];
        for (out, group) in groups {
            if group.is_empty() {
                continue;
            }
            let reorder = group.iter().any(|(_, c)| matches!(c, C::MoveColumn { .. } | C::AddColumn { .. }));
            let diff = Diff {
                changes: group.iter().map(|(_, c)| (*c).clone()).collect(),
                appended: !reorder || t.appended,
            };
            for st in d.design_sql(schema, name, &diff)? {
                let items = st.changes.iter().map(|&i| group[i].0).collect();
                out.push(SyncStep::Sql { sql: st.sql, items });
            }
        }
    }
    let steps = [drop_fks, drop_tables, renames, alters, creates, add_fks].concat();
    Ok(SyncScript { steps, refused, warnings, transactional: d.design_transactional() })
}
```

- [ ] **Step 9: 修改 `crates/dblens-dialect/src/design/mod.rs`**

```diff
--- a/crates/dblens-dialect/src/design/mod.rs
+++ b/crates/dblens-dialect/src/design/mod.rs
@@ -2,7 +2,7 @@
 //! 再按方言生成 DDL；库不支持的改动改用重建表的脚本。不做任何 IO。
 
 mod catalog;
-mod diff;
+pub(crate) mod diff;
 mod mysql;
 mod oracle;
 mod postgres;
@@ -21,6 +21,8 @@ pub use catalog::{DesignCaps, TypeEntry, TypeParams};
 pub use diff::{diff_table, validate};
 pub use rebuild::{Rebuild, RebuildContext, RebuildOutcome, RebuildStep};
 pub use sql::DesignStatement;
+#[cfg(test)]
+pub(crate) use sql::fixtures;
 
 /// 草稿中改了名的项：新名 → 原名。前端用每一项的 `origin` 跟踪原来的名字，发送时整理成这个映射。
 /// 约束改名一律先删后建，不需要映射。
```

- [ ] **Step 10: 修改 `crates/dblens-dialect/src/lib.rs`**

```diff
--- a/crates/dblens-dialect/src/lib.rs
+++ b/crates/dblens-dialect/src/lib.rs
@@ -2,6 +2,7 @@
 
 mod browse;
 mod changes;
+pub mod compare;
 mod ddl;
 pub mod design;
 mod dml;
```

- [ ] **Step 11: 检查**

```bash
export CARGO_INCREMENTAL=0
cargo fmt --all
cargo clippy --config "source.mirror.registry='sparse+https://rsproxy.cn/index/'" --workspace --all-targets -- -D warnings
DBLENS_LANG=en cargo test --config "source.mirror.registry='sparse+https://rsproxy.cn/index/'" --workspace
```

Expected: 全部测试通过（dialect 新增 6 个，快照 2 个）。

- [ ] **Step 12: 提交**

```bash
git add crates/dblens-core/locales/app.json crates/dblens-dialect/src/compare/apply.rs crates/dblens-dialect/src/compare/mod.rs crates/dblens-dialect/src/compare/normalize.rs crates/dblens-dialect/src/compare/snapshots/dblens_dialect__compare__tests__sync_mysql.snap crates/dblens-dialect/src/compare/snapshots/dblens_dialect__compare__tests__sync_pg.snap crates/dblens-dialect/src/compare/sync.rs crates/dblens-dialect/src/compare/tests.rs crates/dblens-dialect/src/design/mod.rs crates/dblens-dialect/src/lib.rs
git commit -m "feat(dialect): 结构比对、写回与同步脚本" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 3: 服务层：模型目录的读写

- 依赖：工作区加 `serde-saphyr = { version = "2.0.0", default-features = false, features = ["serialize", "deserialize"] }`，`dblens-service` 使用它，开发依赖加 `insta`。
- `model/file.rs`：`ModelInfo`（`model.yaml`）、`ModelDatabase`（`dialect()` 由它构造方言）、`SchemaRenames`；表文件的写法（字段顺序固定，空的不写，逻辑类型和列序号不写，`nullable` 只写 false，升序的索引列只写名字，多行文本用块写法）；表和每列不认识的字段保留；`escape_name` / `unescape_name`；`fingerprint`（文件内容的哈希）；`write_atomic`。
- `model/store.rs`：`ModelStore`：打开、新建、由表列表新建（反向生成用）、关闭、列表；表文件缓存（按修改时间和长度重读，解析失败的标为"文件有错"）；读表、读一个 schema 的全部表（带指纹）；保存（核对指纹，列改名时更新其他表外键的引用、记入改名记录，删掉的列去掉记录和扩展字段）、表改名（改文件名、更新引用、记录；再改沿用最初的原名，改回原名删去记录）、删除、写回（先核对全部涉及的表的指纹再写）、删去已应用的改名；格式比程序新时只读。
- 后端词条加 `noun.model`、`model.*`（读写失败、目录不为空、不是模型目录、文件有错、只读、冲突、表已存在）。

快照文件（`snapshots/`）随本任务给出；改了生成逻辑时用 `INSTA_UPDATE=always` 重新生成并逐个审查差异。

**Files:**
- Modify: `Cargo.toml`
- Modify: `crates/dblens-core/locales/app.json`
- Modify: `crates/dblens-service/Cargo.toml`
- Modify: `crates/dblens-service/src/lib.rs`
- Create: `crates/dblens-service/src/model/file.rs`
- Create: `crates/dblens-service/src/model/mod.rs`
- Create: `crates/dblens-service/src/model/snapshots/dblens_service__model__file__tests__info_yaml.snap`
- Create: `crates/dblens-service/src/model/snapshots/dblens_service__model__file__tests__table_yaml.snap`
- Create: `crates/dblens-service/src/model/store.rs`

**Interfaces:**
- Consumes：任务 2 无；方言的 `logical_type`。
- Produces（`dblens_service`）：`ModelStore`（`open`、`create`、`create_from`、`close`、`list`、`summary_of`、`dialect`、`set_info`、`tables`、`read_table`、`schema_tables`、`renames`、`save_table`、`rename_table`、`delete_table`、`write_back`、`clear_renames`、`root`）、`ModelSummary { id, path, info, read_only }`、`ModelInfo`、`ModelDatabase { variant, compat, version }`、`SchemaRenames { tables, columns }`、`TableEntry`、`FileError`、`ModelTableView { table, fingerprint }`、`SchemaTables { tables: Vec<(Table, String)>, errors }`、`ModelWrite::{Put { table: Box<Table>, path_of }, Delete { name }}`。

- [ ] **Step 1: 修改 `Cargo.toml`**

```diff
--- a/Cargo.toml
+++ b/Cargo.toml
@@ -52,6 +52,7 @@ rustls = { version = "0.23.46", default-features = false, features = ["aws_lc_rs
 semver = "1.0.28"
 serde = { version = "1.0.229", features = ["derive"] }
 serde_json = "1.0.154"
+serde-saphyr = { version = "2.0.0", default-features = false, features = ["serialize", "deserialize"] }
 sqlparser = "0.63.0"
 sys-locale = "0.3.2"
 tauri = "2.12.5"
```

- [ ] **Step 2: 修改 `crates/dblens-core/locales/app.json`**

```diff
--- a/crates/dblens-core/locales/app.json
+++ b/crates/dblens-core/locales/app.json
@@ -140,6 +140,15 @@
   "metadata.not_source": {"zh-CN": "%{key} 不是源码查询", "en": "%{key} is not a source query"},
   "metadata.pool_closed": {"zh-CN": "连接池已关闭", "en": "The connection pool is closed"},
   "metadata.table_detail_count": {"zh-CN": "表明细查询数量不对", "en": "Wrong number of table detail queries"},
+  "model.bad_file": {"zh-CN": "文件 %{path} 有错：%{error}", "en": "File %{path} is invalid: %{error}"},
+  "model.bad_path": {"zh-CN": "路径无效：%{path}", "en": "Invalid path: %{path}"},
+  "model.conflict": {"zh-CN": "%{name} 的文件已被修改，请重新加载", "en": "The file for %{name} has changed; reload it"},
+  "model.dir_not_empty": {"zh-CN": "目录 %{path} 不为空", "en": "Directory %{path} is not empty"},
+  "model.io_failed": {"zh-CN": "读写 %{path} 失败：%{error}", "en": "Failed to access %{path}: %{error}"},
+  "model.not_a_model": {"zh-CN": "%{path} 不是模型目录（没有 model.yaml）", "en": "%{path} is not a model directory (model.yaml not found)"},
+  "model.read_only": {"zh-CN": "模型文件格式 %{format} 比本程序新，只能查看；请升级数洞", "en": "Model format %{format} is newer than this version supports; it is read-only. Please upgrade DBLens"},
+  "model.table_exists": {"zh-CN": "表 %{name} 已存在", "en": "Table %{name} already exists"},
+  "model.yaml_write_failed": {"zh-CN": "生成 YAML 失败：%{error}", "en": "Failed to write YAML: %{error}"},
   "native.dameng_dpi": {"zh-CN": "达梦 DPI 库", "en": "DM DPI library"},
   "native.dir_not_absolute": {"zh-CN": "%{lib}目录必须是绝对路径：%{dir}", "en": "The %{lib} directory must be an absolute path: %{dir}"},
   "native.install.dameng_dpi": {"zh-CN": "请安装达梦客户端（含 DPI 驱动），然后在连接设置中指定 dmdpi 所在目录，或设置 DM_HOME", "en": "Install the DM client (with the DPI driver), then set the directory containing dmdpi in the connection settings, or set DM_HOME"},
@@ -148,6 +157,7 @@
   "native.oracle_client": {"zh-CN": "Oracle 客户端库", "en": "Oracle client library"},
   "noun.favorite": {"zh-CN": "收藏 %{id}", "en": "favorite %{id}"},
   "noun.history": {"zh-CN": "历史记录 %{id}", "en": "history entry %{id}"},
+  "noun.model": {"zh-CN": "模型 %{id}", "en": "model %{id}"},
   "noun.object": {"zh-CN": "%{kind} %{name}", "en": "%{kind} %{name}"},
   "noun.profile": {"zh-CN": "连接配置 %{id}", "en": "connection profile %{id}"},
   "noun.session": {"zh-CN": "会话 %{id}", "en": "session %{id}"},
```

- [ ] **Step 3: 修改 `crates/dblens-service/Cargo.toml`**

```diff
--- a/crates/dblens-service/Cargo.toml
+++ b/crates/dblens-service/Cargo.toml
@@ -37,6 +37,7 @@ rusqlite.workspace = true
 rust_xlsxwriter.workspace = true
 serde.workspace = true
 serde_json.workspace = true
+serde-saphyr.workspace = true
 sqlparser.workspace = true
 tokio.workspace = true
 tracing.workspace = true
@@ -44,4 +45,5 @@ uuid.workspace = true
 
 [dev-dependencies]
 csv.workspace = true
+insta.workspace = true
 tempfile.workspace = true
```

- [ ] **Step 4: 修改 `crates/dblens-service/src/lib.rs`**

```diff
--- a/crates/dblens-service/src/lib.rs
+++ b/crates/dblens-service/src/lib.rs
@@ -10,6 +10,7 @@ mod favorite;
 mod history;
 mod import;
 mod metadata;
+mod model;
 mod objects;
 mod policy;
 mod profile;
@@ -38,6 +39,10 @@ pub use import::{
     infer_type, new_table, preview_file,
 };
 pub use metadata::{METADATA_POOL_SIZE, MetadataCache, MetadataService};
+pub use model::{
+    FileError, ModelDatabase, ModelInfo, ModelStore, ModelSummary, ModelTableView, ModelWrite, SchemaRenames,
+    SchemaTables, TableEntry,
+};
 pub use objects::{DataOrder, GenerateKind, ObjectKind};
 pub use policy::{
     Decision, DefaultPolicy, ExecContext, ObjectRef, Outcome, Policy, StatementInfo, StatementKind, classify,
```

- [ ] **Step 5: 新建 `crates/dblens-service/src/model/file.rs`**

```rust
//! 模型目录中的文件（M4a 设计文档第 3 节）：`model.yaml` 与 `tables/<schema>/<表>.yaml`。
//!
//! 写法为 git 而定：字段顺序固定，空列表、没有的值不写，逻辑类型和列序号不写（读取时由原生类型推出、
//! 按先后顺序编号），多行文本用块写法。不认识的字段读进 `extra`，保存时原样写回。

use std::collections::BTreeMap;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use dblens_core::schema::{
    CheckConstraint, Column, ForeignKey, Index, IndexColumn, PrimaryKey, Table, UniqueConstraint,
};
use dblens_core::{ColumnType, Error, Result, msg};
use dblens_dialect::{Compat, Dialect, Variant};
use serde::{Deserialize, Serialize};

/// 当前的文件格式版本。
pub const FORMAT: u32 = 1;

type Extra = BTreeMap<String, serde_json::Value>;

fn is_false(b: &bool) -> bool {
    !*b
}

fn is_true(b: &bool) -> bool {
    *b
}

fn yes() -> bool {
    true
}

/// 模型绑定的数据库类型。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelDatabase {
    pub variant: Variant,
    #[serde(default, skip_serializing_if = "is_native")]
    pub compat: Compat,
    /// 编码后的版本（见 `parse_version`）；不写时按最新版本。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<u32>,
}

fn is_native(c: &Compat) -> bool {
    *c == Compat::Native
}

impl ModelDatabase {
    pub fn dialect(&self) -> Dialect {
        Dialect::new(self.variant).with_version(self.version).with_compat(self.compat)
    }
}

/// 编辑时记下的改名：新名 → 原名。列的改名按（新的）表名分组。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SchemaRenames {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub tables: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub columns: BTreeMap<String, BTreeMap<String, String>>,
}

impl SchemaRenames {
    pub fn is_empty(&self) -> bool {
        self.tables.is_empty() && self.columns.is_empty()
    }
}

/// `model.yaml`。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelInfo {
    pub format: u32,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub database: ModelDatabase,
    #[serde(default)]
    pub schemas: Vec<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub renames: BTreeMap<String, SchemaRenames>,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct ColumnFile {
    name: String,
    #[serde(rename = "type")]
    native: String,
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    nullable: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    default: Option<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    auto_increment: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    generated: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    comment: Option<String>,
    #[serde(flatten)]
    extra: Extra,
}

/// 索引列：升序的只写名字，降序的写成 `{ name, descending: true }`。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
enum IndexColumnFile {
    Name(String),
    Full { name: String, descending: bool },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct IndexFile {
    name: String,
    columns: Vec<IndexColumnFile>,
    #[serde(default, skip_serializing_if = "is_false")]
    unique: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    method: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct KeyFile {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    columns: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct ForeignKeyFile {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    columns: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    ref_schema: Option<String>,
    ref_table: String,
    ref_columns: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    on_delete: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    on_update: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct CheckFile {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    expression: String,
}

/// 表文件。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct TableFile {
    name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    comment: Option<String>,
    columns: Vec<ColumnFile>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    primary_key: Option<KeyFile>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    uniques: Vec<KeyFile>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    foreign_keys: Vec<ForeignKeyFile>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    checks: Vec<CheckFile>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    indexes: Vec<IndexFile>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    partition: Option<String>,
    #[serde(flatten)]
    extra: Extra,
}

/// 读出的一张表：结构、文件中不认识的字段（保存时写回）。
#[derive(Debug, Clone, PartialEq)]
pub struct ModelTable {
    pub table: Table,
    pub extra: Extra,
    /// 每列不认识的字段，按列名。
    pub column_extra: BTreeMap<String, Extra>,
}

fn to_file(t: &ModelTable) -> TableFile {
    let tab = &t.table;
    let key =
        |name: &Option<String>, columns: &[String]| KeyFile { name: name.clone(), columns: columns.to_vec() };
    TableFile {
        name: tab.name.clone(),
        comment: tab.comment.clone(),
        columns: tab
            .columns
            .iter()
            .map(|c| ColumnFile {
                name: c.name.clone(),
                native: c.data_type.native.clone(),
                nullable: c.nullable,
                default: c.default.clone(),
                auto_increment: c.auto_increment,
                generated: c.generated.clone(),
                comment: c.comment.clone(),
                extra: t.column_extra.get(&c.name).cloned().unwrap_or_default(),
            })
            .collect(),
        primary_key: tab.primary_key.as_ref().map(|k| key(&k.name, &k.columns)),
        uniques: tab.uniques.iter().map(|u| key(&u.name, &u.columns)).collect(),
        foreign_keys: tab
            .foreign_keys
            .iter()
            .map(|f| ForeignKeyFile {
                name: f.name.clone(),
                columns: f.columns.clone(),
                ref_schema: f.ref_schema.clone(),
                ref_table: f.ref_table.clone(),
                ref_columns: f.ref_columns.clone(),
                on_delete: f.on_delete.clone(),
                on_update: f.on_update.clone(),
            })
            .collect(),
        checks: tab
            .checks
            .iter()
            .map(|c| CheckFile { name: c.name.clone(), expression: c.expression.clone() })
            .collect(),
        indexes: tab
            .indexes
            .iter()
            .map(|x| IndexFile {
                name: x.name.clone(),
                columns: x
                    .columns
                    .iter()
                    .map(|c| {
                        if c.descending {
                            IndexColumnFile::Full { name: c.name.clone(), descending: true }
                        } else {
                            IndexColumnFile::Name(c.name.clone())
                        }
                    })
                    .collect(),
                unique: x.unique,
                method: x.method.clone(),
            })
            .collect(),
        partition: tab.partition.clone(),
        extra: t.extra.clone(),
    }
}

fn from_file(f: TableFile, d: &Dialect) -> ModelTable {
    let mut column_extra = BTreeMap::new();
    let columns = f
        .columns
        .into_iter()
        .enumerate()
        .map(|(i, c)| {
            if !c.extra.is_empty() {
                column_extra.insert(c.name.clone(), c.extra);
            }
            Column {
                ordinal: (i + 1) as u32,
                data_type: ColumnType { logical: d.logical_type(&c.native), native: c.native },
                name: c.name,
                nullable: c.nullable,
                default: c.default,
                comment: c.comment,
                auto_increment: c.auto_increment,
                generated: c.generated,
            }
        })
        .collect();
    let table = Table {
        name: f.name,
        comment: f.comment,
        columns,
        primary_key: f.primary_key.map(|k| PrimaryKey { name: k.name, columns: k.columns }),
        uniques: f
            .uniques
            .into_iter()
            .map(|k| UniqueConstraint { name: k.name, columns: k.columns })
            .collect(),
        foreign_keys: f
            .foreign_keys
            .into_iter()
            .map(|k| ForeignKey {
                name: k.name,
                columns: k.columns,
                ref_schema: k.ref_schema,
                ref_table: k.ref_table,
                ref_columns: k.ref_columns,
                on_delete: k.on_delete,
                on_update: k.on_update,
            })
            .collect(),
        checks: f
            .checks
            .into_iter()
            .map(|c| CheckConstraint { name: c.name, expression: c.expression })
            .collect(),
        indexes: f
            .indexes
            .into_iter()
            .map(|x| Index {
                name: x.name,
                columns: x
                    .columns
                    .into_iter()
                    .map(|c| match c {
                        IndexColumnFile::Name(name) => IndexColumn { name, descending: false },
                        IndexColumnFile::Full { name, descending } => IndexColumn { name, descending },
                    })
                    .collect(),
                unique: x.unique,
                method: x.method,
            })
            .collect(),
        triggers: vec![],
        partition: f.partition,
    };
    ModelTable { table, extra: f.extra, column_extra }
}

fn yaml_options() -> serde_saphyr::SerializerOptions {
    let mut o = serde_saphyr::SerializerOptions::default();
    o.prefer_block_scalars = true;
    o
}

fn yaml<T: Serialize>(v: &T) -> Result<String> {
    serde_saphyr::to_string_with_options(v, yaml_options())
        .map_err(|e| Error::internal(msg!("model.yaml_write_failed", error = e)))
}

/// 表结构写成 YAML 文本。
pub fn table_yaml(t: &ModelTable) -> Result<String> {
    yaml(&to_file(t))
}

/// 解析表文件；出错时返回带行号的说明。
pub fn parse_table(text: &str, d: &Dialect) -> std::result::Result<ModelTable, String> {
    serde_saphyr::from_str::<TableFile>(text).map(|f| from_file(f, d)).map_err(|e| e.to_string())
}

pub fn info_yaml(info: &ModelInfo) -> Result<String> {
    yaml(info)
}

pub fn parse_info(text: &str) -> std::result::Result<ModelInfo, String> {
    serde_saphyr::from_str::<ModelInfo>(text).map_err(|e| e.to_string())
}

/// 文件内容的指纹（保存前核对文件是否被别人改过）。
pub fn fingerprint(bytes: &[u8]) -> String {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut h);
    format!("{:016x}", h.finish())
}

/// 先写临时文件再改名，写到一半中断也不会留下坏文件。
pub fn write_atomic(path: &Path, text: &str) -> Result<()> {
    let dir = path.parent().ok_or_else(|| Error::invalid(msg!("model.bad_path", path = path.display())))?;
    std::fs::create_dir_all(dir).map_err(|e| io_err(dir, e))?;
    let tmp = dir.join(format!(".{}.tmp", path.file_name().map(|n| n.to_string_lossy()).unwrap_or_default()));
    std::fs::write(&tmp, text).map_err(|e| io_err(&tmp, e))?;
    std::fs::rename(&tmp, path).map_err(|e| io_err(path, e))
}

pub fn io_err(path: &Path, e: std::io::Error) -> Error {
    Error::internal(msg!("model.io_failed", path = path.display(), error = e))
}

const RESERVED: [&str; 22] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8", "COM9",
    "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// 名字转成文件名：文件系统不允许的字符、`%`、结尾的点和空格用 `%XX` 转义；Windows 的保留名在第一个字符转义。
pub fn escape_name(name: &str) -> String {
    let bad =
        |c: char| c.is_control() || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | '%');
    let enc = |c: char| c.to_string().bytes().map(|b| format!("%{b:02X}")).collect::<String>();
    let mut out: String = name.chars().map(|c| if bad(c) { enc(c) } else { c.to_string() }).collect();
    // 结尾的点和空格
    let trimmed = out.trim_end_matches(['.', ' ']).len();
    if trimmed < out.len() {
        let tail: String = out[trimmed..].chars().map(enc).collect();
        out.truncate(trimmed);
        out.push_str(&tail);
    }
    let stem = out.split('.').next().unwrap_or_default().to_ascii_uppercase();
    if RESERVED.contains(&stem.as_str()) || out.is_empty() {
        let first = out.chars().next().map(enc).unwrap_or_default();
        out = first + &out.chars().skip(1).collect::<String>();
    }
    out
}

/// `escape_name` 的逆运算。
pub fn unescape_name(file: &str) -> String {
    let b = file.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && i + 2 < b.len()
            && let Ok(v) = u8::from_str_radix(std::str::from_utf8(&b[i + 1..i + 3]).unwrap_or("-"), 16)
        {
            out.push(v);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// 表文件的路径：`tables/<schema>/<表>.yaml`。
pub fn table_path(root: &Path, schema: &str, table: &str) -> PathBuf {
    root.join("tables").join(escape_name(schema)).join(format!("{}.yaml", escape_name(table)))
}

pub fn info_path(root: &Path) -> PathBuf {
    root.join("model.yaml")
}

#[cfg(test)]
mod tests {
    use dblens_core::LogicalType;

    use super::*;

    fn sample() -> ModelTable {
        let col = |name: &str, native: &str| Column {
            name: name.into(),
            ordinal: 0,
            data_type: ColumnType { native: native.into(), logical: LogicalType::Other },
            nullable: true,
            default: None,
            comment: None,
            auto_increment: false,
            generated: None,
        };
        let mut id = col("id", "bigint");
        id.nullable = false;
        id.auto_increment = true;
        let mut amount = col("amount", "numeric(10,2)");
        amount.default = Some("0".into());
        amount.comment = Some("金额\n（含税）".into());
        let table = Table {
            name: "orders".into(),
            comment: Some("订单".into()),
            columns: vec![id, col("code", "varchar(20)"), amount],
            primary_key: Some(PrimaryKey { name: None, columns: vec!["id".into()] }),
            uniques: vec![UniqueConstraint { name: Some("uk_code".into()), columns: vec!["code".into()] }],
            checks: vec![CheckConstraint {
                name: Some("ck_amount".into()),
                expression: "amount >= 0".into(),
            }],
            foreign_keys: vec![ForeignKey {
                name: Some("fk_cust".into()),
                columns: vec!["code".into()],
                ref_schema: Some("crm".into()),
                ref_table: "customers".into(),
                ref_columns: vec!["code".into()],
                on_delete: Some("CASCADE".into()),
                on_update: None,
            }],
            indexes: vec![Index {
                name: "ix_amount".into(),
                columns: vec![
                    IndexColumn { name: "amount".into(), descending: true },
                    IndexColumn { name: "code".into(), descending: false },
                ],
                unique: false,
                method: None,
            }],
            ..Default::default()
        };
        ModelTable { table, extra: Extra::new(), column_extra: BTreeMap::new() }
    }

    #[test]
    fn table_yaml_is_stable_and_round_trips() {
        let d = Dialect::new(Variant::Postgres);
        let t = sample();
        let text = table_yaml(&t).unwrap();
        insta::assert_snapshot!("table_yaml", text);
        let back = parse_table(&text, &d).unwrap();
        // 读回时序号按顺序编号、逻辑类型由原生类型推出
        assert_eq!(back.table.columns.iter().map(|c| c.ordinal).collect::<Vec<_>>(), [1, 2, 3]);
        assert_eq!(back.table.columns[0].data_type.logical, LogicalType::Int { bits: 64 });
        let mut expect = t.table.clone();
        for (i, c) in expect.columns.iter_mut().enumerate() {
            c.ordinal = (i + 1) as u32;
            c.data_type.logical = d.logical_type(&c.data_type.native);
        }
        assert_eq!(back.table, expect);
        assert_eq!(table_yaml(&back).unwrap(), text);
    }

    #[test]
    fn unknown_fields_survive_and_errors_have_lines() {
        let d = Dialect::new(Variant::Postgres);
        let text = format!("{}layout:\n  x: 10\n", table_yaml(&sample()).unwrap())
            .replace("  type: bigint\n", "  type: bigint\n  note_from_m4b: keep\n");
        let t = parse_table(&text, &d).unwrap();
        assert_eq!(t.extra["layout"]["x"], 10);
        assert_eq!(t.column_extra["id"]["note_from_m4b"], "keep");
        let again = table_yaml(&t).unwrap();
        assert!(again.contains("layout:") && again.contains("note_from_m4b: keep"), "{again}");
        let err = parse_table("name: x\ncolumns:\n  - name: a\n    type: [oops\n", &d).unwrap_err();
        assert!(err.contains("line 4"), "{err}");
    }

    #[test]
    fn info_yaml_round_trips() {
        let info = ModelInfo {
            format: FORMAT,
            name: "订单系统".into(),
            description: None,
            database: ModelDatabase {
                variant: Variant::Kingbase,
                compat: Compat::Oracle,
                version: Some(120100),
            },
            schemas: vec!["shop".into(), "crm".into()],
            renames: [(
                "shop".into(),
                SchemaRenames {
                    tables: [("orders_new".into(), "orders".into())].into(),
                    columns: [("orders_new".into(), [("amount".into(), "total".into())].into())].into(),
                },
            )]
            .into(),
            extra: Extra::new(),
        };
        let text = info_yaml(&info).unwrap();
        insta::assert_snapshot!("info_yaml", text);
        assert_eq!(parse_info(&text).unwrap(), info);
        let pg = ModelInfo {
            database: ModelDatabase { variant: Variant::Postgres, compat: Compat::Native, version: None },
            renames: BTreeMap::new(),
            ..info
        };
        assert!(!info_yaml(&pg).unwrap().contains("compat"));
    }

    #[test]
    fn file_names_are_escaped() {
        for (name, file) in [
            ("orders", "orders"),
            ("a/b:c", "a%2Fb%3Ac"),
            ("50%", "50%25"),
            ("CON", "%43ON"),
            ("con.x", "%63on.x"),
            ("trail. ", "trail%2E%20"),
            ("数洞 表", "数洞 表"),
        ] {
            assert_eq!(escape_name(name), file, "{name}");
            assert_eq!(unescape_name(file), name, "{file}");
        }
    }
}
```

- [ ] **Step 6: 新建 `crates/dblens-service/src/model/mod.rs`**

```rust
//! 模型（M4a）：模型目录的读写、打开的模型。

mod file;
mod store;

pub use file::{ModelDatabase, ModelInfo, SchemaRenames};
pub use store::{FileError, ModelStore, ModelSummary, ModelTableView, ModelWrite, SchemaTables, TableEntry};
```

- [ ] **Step 7: 新建 `crates/dblens-service/src/model/snapshots/dblens_service__model__file__tests__info_yaml.snap`**

```text
---
source: crates/dblens-service/src/model/file.rs
expression: text
---
format: 1
name: 订单系统
database:
  variant: kingbase
  compat: oracle
  version: 120100
schemas:
- shop
- crm
renames:
  shop:
    tables:
      orders_new: orders
    columns:
      orders_new:
        amount: total
```

- [ ] **Step 8: 新建 `crates/dblens-service/src/model/snapshots/dblens_service__model__file__tests__table_yaml.snap`**

```text
---
source: crates/dblens-service/src/model/file.rs
expression: text
---
name: orders
comment: 订单
columns:
- name: id
  type: bigint
  nullable: false
  auto_increment: true
- name: code
  type: varchar(20)
- name: amount
  type: numeric(10,2)
  default: "0"
  comment: |-
    金额
    （含税）
primary_key:
  columns:
  - id
uniques:
- name: uk_code
  columns:
  - code
foreign_keys:
- name: fk_cust
  columns:
  - code
  ref_schema: crm
  ref_table: customers
  ref_columns:
  - code
  on_delete: CASCADE
checks:
- name: ck_amount
  expression: amount >= 0
indexes:
- name: ix_amount
  columns:
  - name: amount
    descending: true
  - code
```

- [ ] **Step 9: 新建 `crates/dblens-service/src/model/store.rs`**

```rust
//! 打开的模型（M4a 设计文档 4.1 节）：编号 → 目录、模型信息；表文件按需读取，按修改时间和长度判断是否重读。
//! 只做文件读写，不连库。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;

use dblens_core::schema::Table;
use dblens_core::{Error, Result, msg};
use dblens_dialect::Dialect;
use serde::Serialize;

use super::file::{
    FORMAT, ModelDatabase, ModelInfo, ModelTable, SchemaRenames, fingerprint, info_path, info_yaml, io_err,
    parse_info, parse_table, table_path, table_yaml, unescape_name, write_atomic,
};

/// 打开的模型的概要。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ModelSummary {
    pub id: String,
    pub path: String,
    pub info: ModelInfo,
    /// 文件格式比程序认识的新：只读。
    pub read_only: bool,
}

/// 表文件解析失败的原因。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FileError {
    /// 相对模型目录的路径。
    pub path: String,
    /// 带行号、列号的说明。
    pub message: String,
}

/// 表列表中的一项。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TableEntry {
    pub schema: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<FileError>,
}

/// 读出的表和文件指纹（保存时核对）。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ModelTableView {
    pub table: Table,
    pub fingerprint: String,
}

struct Cached {
    modified: SystemTime,
    len: u64,
    fingerprint: String,
    parsed: std::result::Result<ModelTable, String>,
}

struct OpenModel {
    root: PathBuf,
    info: ModelInfo,
    read_only: bool,
    files: BTreeMap<PathBuf, Cached>,
}

/// 一个 schema 中读出的表：（表，文件指纹）；有错的表单独列出。
#[derive(Debug, Clone, Default)]
pub struct SchemaTables {
    pub tables: Vec<(Table, String)>,
    pub errors: Vec<FileError>,
}

/// 写回模型的一项操作（比对后同步到模型，见 6.2 节）。
#[derive(Debug, Clone, PartialEq)]
pub enum ModelWrite {
    /// 写入一张表；`path_of` 是原来的表名（新建时为 None）。改名时旧文件删除。
    Put {
        table: Box<Table>,
        path_of: Option<String>,
    },
    Delete {
        name: String,
    },
}

#[derive(Default)]
pub struct ModelStore {
    models: Mutex<BTreeMap<String, OpenModel>>,
}

fn rel(root: &Path, p: &Path) -> String {
    p.strip_prefix(root).unwrap_or(p).to_string_lossy().replace('\\', "/")
}

fn not_open(id: &str) -> Error {
    Error::not_found(msg!("noun.model", id = id))
}

fn table_missing(schema: &str, name: &str) -> Error {
    Error::not_found(msg!("noun.object", kind = "table", name = format!("{schema}.{name}")))
}

/// 目录必须不存在或为空。
fn check_empty(dir: &Path) -> Result<()> {
    match std::fs::read_dir(dir) {
        Ok(mut it) => match it.next() {
            Some(_) => Err(Error::invalid(msg!("model.dir_not_empty", path = dir.display()))),
            None => Ok(()),
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(io_err(dir, e)),
    }
}

fn load_info(root: &Path) -> Result<ModelInfo> {
    let path = info_path(root);
    let text = std::fs::read_to_string(&path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            Error::invalid(msg!("model.not_a_model", path = root.display()))
        } else {
            io_err(&path, e)
        }
    })?;
    parse_info(&text).map_err(|e| Error::invalid(msg!("model.bad_file", path = "model.yaml", error = e)))
}

/// 改名记录（新名 → 原名）中把 `from` 改成 `to`：`from` 本身是上次改名的结果时沿用最初的原名，改回原名时删去。
fn record_rename(map: &mut BTreeMap<String, String>, from: &str, to: &str) {
    let original = map.remove(from).unwrap_or_else(|| from.to_string());
    if original != to {
        map.insert(to.to_string(), original);
    }
}

impl OpenModel {
    fn dialect(&self) -> Dialect {
        self.info.database.dialect()
    }

    fn writable(&self) -> Result<()> {
        if self.read_only {
            return Err(Error::invalid(msg!("model.read_only", format = self.info.format)));
        }
        Ok(())
    }

    /// 扫描 `tables/` 下的文件，修改时间或长度变了的重读，删掉的文件移出缓存。
    fn refresh(&mut self) -> Result<()> {
        let d = self.dialect();
        let dir = self.root.join("tables");
        let mut seen = Vec::new();
        let schemas = match std::fs::read_dir(&dir) {
            Ok(it) => it.collect::<std::io::Result<Vec<_>>>().map_err(|e| io_err(&dir, e))?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => vec![],
            Err(e) => return Err(io_err(&dir, e)),
        };
        for s in schemas.into_iter().filter(|s| s.path().is_dir()) {
            let sdir = s.path();
            for f in std::fs::read_dir(&sdir).map_err(|e| io_err(&sdir, e))? {
                let path = f.map_err(|e| io_err(&sdir, e))?.path();
                if path.extension().is_none_or(|x| x != "yaml") {
                    continue;
                }
                let meta = std::fs::metadata(&path).map_err(|e| io_err(&path, e))?;
                let modified = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
                seen.push(path.clone());
                if self.files.get(&path).is_some_and(|c| c.modified == modified && c.len == meta.len()) {
                    continue;
                }
                let bytes = std::fs::read(&path).map_err(|e| io_err(&path, e))?;
                let parsed = std::str::from_utf8(&bytes)
                    .map_err(|e| e.to_string())
                    .and_then(|text| parse_table(text, &d));
                let cached = Cached { modified, len: meta.len(), fingerprint: fingerprint(&bytes), parsed };
                self.files.insert(path, cached);
            }
        }
        self.files.retain(|p, _| seen.contains(p));
        Ok(())
    }

    fn schema_of(&self, path: &Path) -> String {
        let dir = path.parent().and_then(|p| p.file_name()).map(|n| n.to_string_lossy()).unwrap_or_default();
        unescape_name(&dir)
    }

    /// 表所在的文件：表名以文件内容为准，解析失败的按文件名。
    fn find(&self, schema: &str, name: &str) -> Option<&Path> {
        self.files
            .iter()
            .find(|(p, c)| {
                self.schema_of(p) == schema
                    && match &c.parsed {
                        Ok(t) => t.table.name == name,
                        Err(_) => p.file_stem().is_some_and(|s| unescape_name(&s.to_string_lossy()) == name),
                    }
            })
            .map(|(p, _)| p.as_path())
    }

    fn entries(&self) -> Vec<TableEntry> {
        let mut out: Vec<TableEntry> = self
            .files
            .iter()
            .map(|(p, c)| {
                let schema = self.schema_of(p);
                match &c.parsed {
                    Ok(t) => TableEntry {
                        schema,
                        name: t.table.name.clone(),
                        comment: t.table.comment.clone(),
                        error: None,
                    },
                    Err(e) => TableEntry {
                        schema,
                        name: p.file_stem().map(|s| unescape_name(&s.to_string_lossy())).unwrap_or_default(),
                        comment: None,
                        error: Some(FileError { path: rel(&self.root, p), message: e.clone() }),
                    },
                }
            })
            .collect();
        out.sort_by(|a, b| (&a.schema, &a.name).cmp(&(&b.schema, &b.name)));
        out
    }

    fn cached(&self, schema: &str, name: &str) -> Result<(&Path, &Cached)> {
        let path = self.find(schema, name).ok_or_else(|| table_missing(schema, name))?;
        Ok((path, &self.files[path]))
    }

    fn read(&self, schema: &str, name: &str) -> Result<(PathBuf, ModelTable, String)> {
        let (path, c) = self.cached(schema, name)?;
        match &c.parsed {
            Ok(t) => Ok((path.to_path_buf(), t.clone(), c.fingerprint.clone())),
            Err(e) => Err(Error::invalid(msg!("model.bad_file", path = rel(&self.root, path), error = e))),
        }
    }

    fn check_fingerprint(&self, schema: &str, name: &str, expected: Option<&str>) -> Result<()> {
        if let Some(fp) = expected
            && self.cached(schema, name)?.1.fingerprint != fp
        {
            return Err(Error::invalid(msg!("model.conflict", name = format!("{schema}.{name}"))));
        }
        Ok(())
    }

    fn write_info(&mut self) -> Result<()> {
        write_atomic(&info_path(&self.root), &info_yaml(&self.info)?)
    }

    /// 写一张表的文件（保留文件中不认识的字段），返回新的指纹。
    fn write_table(&mut self, schema: &str, path: &Path, t: &ModelTable) -> Result<String> {
        let text = table_yaml(t)?;
        write_atomic(path, &text)?;
        if !self.info.schemas.iter().any(|s| s == schema) {
            self.info.schemas.push(schema.to_string());
            self.write_info()?;
        }
        Ok(fingerprint(text.as_bytes()))
    }

    /// 其他表中引用本 schema 的外键交给 `fix` 修改（返回是否改了），改过的表写回文件。
    fn update_refs(
        &mut self,
        schema: &str,
        skip: Option<&Path>,
        mut fix: impl FnMut(&mut dblens_core::schema::ForeignKey) -> bool,
    ) -> Result<()> {
        let mut changed = Vec::new();
        for (p, c) in &self.files {
            if Some(p.as_path()) == skip {
                continue;
            }
            let Ok(t) = &c.parsed else { continue };
            let own = self.schema_of(p);
            let mut t = t.clone();
            let mut hit = false;
            for f in &mut t.table.foreign_keys {
                let ref_schema = f.ref_schema.clone().unwrap_or_else(|| own.clone());
                if ref_schema == schema && fix(f) {
                    hit = true;
                }
            }
            if hit {
                changed.push((own, p.clone(), t));
            }
        }
        for (own, p, t) in changed {
            self.write_table(&own, &p, &t)?;
        }
        Ok(())
    }

    fn renames(&mut self, schema: &str) -> &mut SchemaRenames {
        self.info.renames.entry(schema.to_string()).or_default()
    }

    fn prune_renames(&mut self) {
        self.info.renames.retain(|_, r| {
            r.columns.retain(|_, c| !c.is_empty());
            !r.is_empty()
        });
    }
}

impl ModelStore {
    fn with<T>(&self, id: &str, f: impl FnOnce(&mut OpenModel) -> Result<T>) -> Result<T> {
        let mut models = self.models.lock().unwrap_or_else(|e| e.into_inner());
        let m = models.get_mut(id).ok_or_else(|| not_open(id))?;
        f(m)
    }

    fn summary(id: &str, m: &OpenModel) -> ModelSummary {
        ModelSummary {
            id: id.to_string(),
            path: m.root.to_string_lossy().into_owned(),
            info: m.info.clone(),
            read_only: m.read_only,
        }
    }

    fn insert(&self, root: PathBuf, info: ModelInfo) -> Result<ModelSummary> {
        let mut models = self.models.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((id, m)) = models.iter().find(|(_, m)| m.root == root) {
            return Ok(Self::summary(id, m));
        }
        let read_only = info.format > FORMAT;
        let mut m = OpenModel { root, info, read_only, files: BTreeMap::new() };
        m.refresh()?;
        let id = uuid::Uuid::new_v4().to_string();
        let s = Self::summary(&id, &m);
        models.insert(id, m);
        Ok(s)
    }

    /// 打开模型目录；已经打开的返回原来的编号。
    pub fn open(&self, path: &Path) -> Result<ModelSummary> {
        let root = std::path::absolute(path).map_err(|e| io_err(path, e))?;
        let info = load_info(&root)?;
        self.insert(root, info)
    }

    /// 新建空模型：目录必须不存在或为空。
    pub fn create(
        &self,
        path: &Path,
        name: &str,
        database: ModelDatabase,
        schemas: Vec<String>,
    ) -> Result<ModelSummary> {
        let root = std::path::absolute(path).map_err(|e| io_err(path, e))?;
        check_empty(&root)?;
        let info = ModelInfo {
            format: FORMAT,
            name: name.to_string(),
            description: None,
            database,
            schemas,
            renames: BTreeMap::new(),
            extra: BTreeMap::new(),
        };
        write_atomic(&info_path(&root), &info_yaml(&info)?)?;
        self.insert(root, info)
    }

    /// 反向生成：写入新目录，表文件写完后打开。
    pub fn create_from(
        &self,
        path: &Path,
        name: &str,
        database: ModelDatabase,
        tables: &[(String, Vec<Table>)],
    ) -> Result<ModelSummary> {
        let root = std::path::absolute(path).map_err(|e| io_err(path, e))?;
        check_empty(&root)?;
        for (schema, list) in tables {
            for t in list {
                let mt =
                    ModelTable { table: t.clone(), extra: BTreeMap::new(), column_extra: BTreeMap::new() };
                write_atomic(&table_path(&root, schema, &t.name), &table_yaml(&mt)?)?;
            }
        }
        let info = ModelInfo {
            format: FORMAT,
            name: name.to_string(),
            description: None,
            database,
            schemas: tables.iter().map(|(s, _)| s.clone()).collect(),
            renames: BTreeMap::new(),
            extra: BTreeMap::new(),
        };
        write_atomic(&info_path(&root), &info_yaml(&info)?)?;
        self.insert(root, info)
    }

    pub fn close(&self, id: &str) -> Result<()> {
        let mut models = self.models.lock().unwrap_or_else(|e| e.into_inner());
        models.remove(id).map(|_| ()).ok_or_else(|| not_open(id))
    }

    pub fn list(&self) -> Vec<ModelSummary> {
        let models = self.models.lock().unwrap_or_else(|e| e.into_inner());
        models.iter().map(|(id, m)| Self::summary(id, m)).collect()
    }

    /// 重读 `model.yaml` 和表文件。
    pub fn summary_of(&self, id: &str) -> Result<ModelSummary> {
        self.with(id, |m| {
            m.info = load_info(&m.root)?;
            m.read_only = m.info.format > FORMAT;
            m.refresh()?;
            Ok(Self::summary(id, m))
        })
    }

    pub fn dialect(&self, id: &str) -> Result<Dialect> {
        self.with(id, |m| Ok(m.dialect()))
    }

    /// 改名称、说明、schema 列表。
    pub fn set_info(
        &self,
        id: &str,
        name: &str,
        description: Option<String>,
        schemas: Vec<String>,
    ) -> Result<ModelSummary> {
        self.with(id, |m| {
            m.writable()?;
            m.info = load_info(&m.root)?;
            m.info.name = name.to_string();
            m.info.description = description;
            m.info.schemas = schemas;
            m.write_info()?;
            Ok(Self::summary(id, m))
        })
    }

    pub fn tables(&self, id: &str) -> Result<Vec<TableEntry>> {
        self.with(id, |m| {
            m.refresh()?;
            Ok(m.entries())
        })
    }

    pub fn read_table(&self, id: &str, schema: &str, name: &str) -> Result<ModelTableView> {
        self.with(id, |m| {
            m.refresh()?;
            let (_, t, fingerprint) = m.read(schema, name)?;
            Ok(ModelTableView { table: t.table, fingerprint })
        })
    }

    /// 一个 schema 的全部表（比对用）；文件有错的单独列出。
    pub fn schema_tables(&self, id: &str, schema: &str) -> Result<SchemaTables> {
        self.with(id, |m| {
            m.refresh()?;
            let mut out = SchemaTables::default();
            for (p, c) in &m.files {
                if m.schema_of(p) != schema {
                    continue;
                }
                match &c.parsed {
                    Ok(t) => out.tables.push((t.table.clone(), c.fingerprint.clone())),
                    Err(e) => out.errors.push(FileError { path: rel(&m.root, p), message: e.clone() }),
                }
            }
            out.tables.sort_by(|a, b| a.0.name.cmp(&b.0.name));
            Ok(out)
        })
    }

    /// 改名记录。
    pub fn renames(&self, id: &str, schema: &str) -> Result<SchemaRenames> {
        self.with(id, |m| Ok(m.info.renames.get(schema).cloned().unwrap_or_default()))
    }

    /// 保存设计器中的表。`name` 为原表名（新建时 None）；`column_renames`：新列名 → 原列名；
    /// `expected`：打开时的指纹，文件已变则拒绝。表名不能在这里改（用 `rename_table`）。返回新的指纹。
    pub fn save_table(
        &self,
        id: &str,
        schema: &str,
        name: Option<&str>,
        table: Table,
        column_renames: &BTreeMap<String, String>,
        expected: Option<&str>,
    ) -> Result<String> {
        self.with(id, |m| {
            m.writable()?;
            m.refresh()?;
            m.info = load_info(&m.root)?;
            let (path, mut mt) = match name {
                Some(n) => {
                    m.check_fingerprint(schema, n, expected)?;
                    let (path, mt, _) = m.read(schema, n)?;
                    (path, mt)
                }
                None => {
                    if m.find(schema, &table.name).is_some() {
                        return Err(Error::invalid(msg!(
                            "model.table_exists",
                            name = format!("{schema}.{}", table.name)
                        )));
                    }
                    let mt = ModelTable {
                        table: table.clone(),
                        extra: BTreeMap::new(),
                        column_extra: BTreeMap::new(),
                    };
                    (table_path(&m.root, schema, &table.name), mt)
                }
            };
            let tname = name.map(str::to_string).unwrap_or_else(|| table.name.clone());
            let table = Table { name: tname.clone(), ..table };
            // 列改名：其他表的外键跟着改，记入改名记录；列的扩展字段跟着新名
            for (to, from) in column_renames {
                if let Some(x) = mt.column_extra.remove(from) {
                    mt.column_extra.insert(to.clone(), x);
                }
                let (tn, from_c, to_c) = (tname.clone(), from.clone(), to.clone());
                m.update_refs(schema, Some(&path), |f| {
                    let hit = f.ref_table == tn && f.ref_columns.contains(&from_c);
                    if hit {
                        f.ref_columns.iter_mut().filter(|c| **c == from_c).for_each(|c| *c = to_c.clone());
                    }
                    hit
                })?;
                let cols = m.renames(schema).columns.entry(tname.clone()).or_default();
                record_rename(cols, from, to);
            }
            // 删掉的列不再保留改名记录和扩展字段
            let names: Vec<&str> = table.columns.iter().map(|c| c.name.as_str()).collect();
            if let Some(cols) = m.renames(schema).columns.get_mut(&tname) {
                cols.retain(|k, _| names.contains(&k.as_str()));
            }
            mt.column_extra.retain(|k, _| names.contains(&k.as_str()));
            m.prune_renames();
            mt.table = table;
            let fp = m.write_table(schema, &path, &mt)?;
            m.write_info()?;
            m.refresh()?;
            Ok(fp)
        })
    }

    /// 表改名：改文件名、更新其他表外键的引用、记入改名记录。
    pub fn rename_table(&self, id: &str, schema: &str, from: &str, to: &str) -> Result<()> {
        self.with(id, |m| {
            m.writable()?;
            m.refresh()?;
            m.info = load_info(&m.root)?;
            if m.find(schema, to).is_some() {
                return Err(Error::invalid(msg!("model.table_exists", name = format!("{schema}.{to}"))));
            }
            let (old, mut mt, _) = m.read(schema, from)?;
            mt.table.name = to.to_string();
            // 自引用的外键
            for f in &mut mt.table.foreign_keys {
                if f.ref_table == from && f.ref_schema.as_deref().is_none_or(|s| s == schema) {
                    f.ref_table = to.to_string();
                }
            }
            let new = table_path(&m.root, schema, to);
            m.write_table(schema, &new, &mt)?;
            if new != old {
                std::fs::remove_file(&old).map_err(|e| io_err(&old, e))?;
            }
            m.update_refs(schema, Some(&new), |f| {
                let hit = f.ref_table == from;
                if hit {
                    f.ref_table = to.to_string();
                }
                hit
            })?;
            let r = m.renames(schema);
            record_rename(&mut r.tables, from, to);
            if let Some(cols) = r.columns.remove(from) {
                r.columns.insert(to.to_string(), cols);
            }
            m.prune_renames();
            m.write_info()?;
            m.refresh()
        })
    }

    /// 删除表文件，去掉它的改名记录。
    pub fn delete_table(&self, id: &str, schema: &str, name: &str, expected: Option<&str>) -> Result<()> {
        self.with(id, |m| {
            m.writable()?;
            m.refresh()?;
            m.info = load_info(&m.root)?;
            m.check_fingerprint(schema, name, expected)?;
            let path = m.find(schema, name).ok_or_else(|| table_missing(schema, name))?.to_path_buf();
            std::fs::remove_file(&path).map_err(|e| io_err(&path, e))?;
            let r = m.renames(schema);
            r.tables.remove(name);
            r.columns.remove(name);
            m.prune_renames();
            m.write_info()?;
            m.refresh()
        })
    }

    /// 比对后写回：先核对涉及的表的指纹（比对时记下的），再依次写入、删除。
    /// `cleared`：已应用的改名（表名，及（表名，列名）），从改名记录中删去。
    pub fn write_back(
        &self,
        id: &str,
        schema: &str,
        fingerprints: &BTreeMap<String, String>,
        writes: Vec<ModelWrite>,
        cleared: &[(String, Option<String>)],
    ) -> Result<()> {
        self.with(id, |m| {
            m.writable()?;
            m.refresh()?;
            m.info = load_info(&m.root)?;
            for w in &writes {
                let name = match w {
                    ModelWrite::Put { path_of: Some(n), .. } | ModelWrite::Delete { name: n } => n,
                    ModelWrite::Put { path_of: None, table } => {
                        if m.find(schema, &table.name).is_some() {
                            return Err(Error::invalid(msg!(
                                "model.conflict",
                                name = format!("{schema}.{}", table.name)
                            )));
                        }
                        continue;
                    }
                };
                m.check_fingerprint(schema, name, fingerprints.get(name).map(String::as_str))?;
            }
            for w in writes {
                match w {
                    ModelWrite::Put { table, path_of } => {
                        let (old, mut mt) = match &path_of {
                            Some(n) => {
                                let (p, mt, _) = m.read(schema, n)?;
                                (Some(p), mt)
                            }
                            None => (
                                None,
                                ModelTable {
                                    table: Table::default(),
                                    extra: BTreeMap::new(),
                                    column_extra: BTreeMap::new(),
                                },
                            ),
                        };
                        let new = match &old {
                            Some(p) if path_of.as_deref() == Some(table.name.as_str()) => p.clone(),
                            _ => table_path(&m.root, schema, &table.name),
                        };
                        mt.table = *table;
                        m.write_table(schema, &new, &mt)?;
                        if let Some(p) = old.filter(|p| *p != new) {
                            std::fs::remove_file(&p).map_err(|e| io_err(&p, e))?;
                        }
                    }
                    ModelWrite::Delete { name } => {
                        let path =
                            m.find(schema, &name).ok_or_else(|| table_missing(schema, &name))?.to_path_buf();
                        std::fs::remove_file(&path).map_err(|e| io_err(&path, e))?;
                    }
                }
                m.refresh()?;
            }
            clear_renames(m, schema, cleared);
            m.write_info()?;
            m.refresh()
        })
    }

    /// 同步到库后，删去已应用的改名记录。
    pub fn clear_renames(&self, id: &str, schema: &str, cleared: &[(String, Option<String>)]) -> Result<()> {
        if cleared.is_empty() {
            return Ok(());
        }
        self.with(id, |m| {
            m.writable()?;
            m.info = load_info(&m.root)?;
            clear_renames(m, schema, cleared);
            m.write_info()
        })
    }

    /// 模型目录（"在文件管理器中显示"）。
    pub fn root(&self, id: &str) -> Result<PathBuf> {
        self.with(id, |m| Ok(m.root.clone()))
    }
}

fn clear_renames(m: &mut OpenModel, schema: &str, cleared: &[(String, Option<String>)]) {
    let r = m.renames(schema);
    for (table, column) in cleared {
        match column {
            None => {
                r.tables.remove(table);
            }
            Some(c) => {
                if let Some(cols) = r.columns.get_mut(table) {
                    cols.remove(c);
                }
            }
        }
    }
    m.prune_renames();
}

#[cfg(test)]
mod tests {
    use dblens_core::schema::{Column, ForeignKey, PrimaryKey};
    use dblens_core::{ColumnType, LogicalType};
    use dblens_dialect::{Compat, Variant};

    use super::*;

    fn col(name: &str) -> Column {
        Column {
            name: name.into(),
            ordinal: 0,
            data_type: ColumnType { native: "integer".into(), logical: LogicalType::Other },
            nullable: false,
            default: None,
            comment: None,
            auto_increment: false,
            generated: None,
        }
    }

    fn table(name: &str, cols: &[&str]) -> Table {
        Table {
            name: name.into(),
            columns: cols.iter().map(|c| col(c)).collect(),
            primary_key: Some(PrimaryKey { name: None, columns: vec![cols[0].into()] }),
            ..Default::default()
        }
    }

    fn fk(cols: &str, ref_table: &str, ref_cols: &str) -> ForeignKey {
        ForeignKey {
            name: Some(format!("fk_{cols}")),
            columns: vec![cols.into()],
            ref_schema: None,
            ref_table: ref_table.into(),
            ref_columns: vec![ref_cols.into()],
            on_delete: None,
            on_update: None,
        }
    }

    fn pg() -> ModelDatabase {
        ModelDatabase { variant: Variant::Postgres, compat: Compat::Native, version: None }
    }

    fn fingerprints(store: &ModelStore, id: &str) -> BTreeMap<String, String> {
        let s = store.schema_tables(id, "public").unwrap();
        s.tables.iter().map(|(t, fp)| (t.name.clone(), fp.clone())).collect()
    }

    /// 两张表：orders 的 cust_id 引用 customers.id。
    fn sample(dir: &Path) -> (ModelStore, String) {
        let store = ModelStore::default();
        let mut orders = table("orders", &["id", "cust_id"]);
        orders.foreign_keys.push(fk("cust_id", "customers", "id"));
        let m = store
            .create_from(dir, "shop", pg(), &[("public".into(), vec![table("customers", &["id"]), orders])])
            .unwrap();
        (store, m.id)
    }

    #[test]
    fn create_open_and_list() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("m");
        let (store, id) = sample(&root);
        assert!(root.join("tables/public/orders.yaml").exists());
        let names: Vec<_> = store.tables(&id).unwrap().into_iter().map(|e| e.name).collect();
        assert_eq!(names, ["customers", "orders"]);
        // 再次打开同一目录得到同一个编号；另一个 store 也能读
        assert_eq!(store.open(&root).unwrap().id, id);
        let s = ModelStore::default().open(&root).unwrap();
        assert_eq!(s.info.schemas, ["public"]);
        assert!(!s.read_only);
        // 目录不为空时不能新建
        let err = store.create(&root, "x", pg(), vec![]).unwrap_err();
        assert!(err.to_string().contains("not empty"), "{err}");
        store.close(&id).unwrap();
        assert!(store.list().is_empty());
    }

    #[test]
    fn bad_files_and_newer_format() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("m");
        let (store, id) = sample(&root);
        std::fs::write(root.join("tables/public/broken.yaml"), "name: broken\ncolumns: [\n").unwrap();
        let entries = store.tables(&id).unwrap();
        let bad = entries.iter().find(|e| e.name == "broken").unwrap();
        let err = bad.error.as_ref().unwrap();
        assert_eq!(err.path, "tables/public/broken.yaml");
        assert!(err.message.contains("line"), "{}", err.message);
        let s = store.schema_tables(&id, "public").unwrap();
        assert_eq!((s.tables.len(), s.errors.len()), (2, 1));
        assert!(store.read_table(&id, "public", "broken").is_err());
        // 格式比程序新：只读
        let text =
            std::fs::read_to_string(root.join("model.yaml")).unwrap().replace("format: 1", "format: 9");
        std::fs::write(root.join("model.yaml"), text).unwrap();
        assert!(store.summary_of(&id).unwrap().read_only);
        let err = store.delete_table(&id, "public", "orders", None).unwrap_err();
        assert!(err.to_string().contains("read-only"), "{err}");
    }

    #[test]
    fn fingerprint_conflict() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("m");
        let (store, id) = sample(&root);
        let v = store.read_table(&id, "public", "customers").unwrap();
        // 文件在外面被改了（如在 git 中切换了分支）
        let path = root.join("tables/public/customers.yaml");
        std::fs::write(&path, std::fs::read_to_string(&path).unwrap() + "comment: changed\n").unwrap();
        let t = table("customers", &["id", "name"]);
        let none = BTreeMap::new();
        let err = store
            .save_table(&id, "public", Some("customers"), t.clone(), &none, Some(&v.fingerprint))
            .unwrap_err();
        assert!(err.to_string().contains("changed"), "{err}");
        let v = store.read_table(&id, "public", "customers").unwrap();
        let fp = store.save_table(&id, "public", Some("customers"), t, &none, Some(&v.fingerprint)).unwrap();
        assert_eq!(store.read_table(&id, "public", "customers").unwrap().fingerprint, fp);
    }

    #[test]
    fn renames_update_references_and_are_recorded() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("m");
        let (store, id) = sample(&root);
        // 表改名：文件改名，orders 的外键跟着改
        store.rename_table(&id, "public", "customers", "clients").unwrap();
        assert!(!root.join("tables/public/customers.yaml").exists());
        let orders = store.read_table(&id, "public", "orders").unwrap().table;
        assert_eq!(orders.foreign_keys[0].ref_table, "clients");
        // 列改名：orders 的外键引用的列跟着改
        let mut t = store.read_table(&id, "public", "clients").unwrap().table;
        t.columns[0].name = "client_id".into();
        t.primary_key = Some(PrimaryKey { name: None, columns: vec!["client_id".into()] });
        let renames: BTreeMap<String, String> = [("client_id".into(), "id".into())].into();
        store.save_table(&id, "public", Some("clients"), t, &renames, None).unwrap();
        let orders = store.read_table(&id, "public", "orders").unwrap().table;
        assert_eq!(orders.foreign_keys[0].ref_columns, ["client_id"]);
        let r = store.renames(&id, "public").unwrap();
        assert_eq!(r.tables, BTreeMap::from([("clients".into(), "customers".into())]));
        assert_eq!(r.columns["clients"], BTreeMap::from([("client_id".into(), "id".into())]));
        // 再改一次沿用最初的原名；改回原名时删去记录
        store.rename_table(&id, "public", "clients", "buyers").unwrap();
        let r = store.renames(&id, "public").unwrap();
        assert_eq!(r.tables, BTreeMap::from([("buyers".into(), "customers".into())]));
        assert!(r.columns.contains_key("buyers"));
        store.rename_table(&id, "public", "buyers", "customers").unwrap();
        assert!(store.renames(&id, "public").unwrap().tables.is_empty());
        // 删除表去掉它的记录
        store.delete_table(&id, "public", "customers", None).unwrap();
        assert!(store.renames(&id, "public").unwrap().is_empty());
        assert!(!std::fs::read_to_string(root.join("model.yaml")).unwrap().contains("renames"));
    }

    #[test]
    fn save_new_table_keeps_unknown_fields() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("m");
        let (store, id) = sample(&root);
        let none = BTreeMap::new();
        store.save_table(&id, "sales", None, table("a/b", &["x"]), &none, None).unwrap();
        assert!(root.join("tables/sales/a%2Fb.yaml").exists());
        assert_eq!(store.summary_of(&id).unwrap().info.schemas, ["public", "sales"]);
        let err = store.save_table(&id, "sales", None, table("a/b", &["x"]), &none, None).unwrap_err();
        assert!(err.to_string().contains("already exists"), "{err}");
        // 手写的字段保存后还在
        let path = root.join("tables/public/orders.yaml");
        std::fs::write(&path, std::fs::read_to_string(&path).unwrap() + "layout:\n  x: 1\n").unwrap();
        let t = table("orders", &["id", "cust_id", "total"]);
        store.save_table(&id, "public", Some("orders"), t, &none, None).unwrap();
        assert!(std::fs::read_to_string(&path).unwrap().contains("layout:"));
    }

    #[test]
    fn write_back_checks_fingerprints() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("m");
        let (store, id) = sample(&root);
        store.rename_table(&id, "public", "customers", "clients").unwrap();
        let fps = fingerprints(&store, &id);
        let writes = vec![
            ModelWrite::Put { table: Box::new(table("customers", &["id"])), path_of: Some("clients".into()) },
            ModelWrite::Delete { name: "orders".into() },
            ModelWrite::Put { table: Box::new(table("items", &["id"])), path_of: None },
        ];
        // 比对之后 orders 被改过：拒绝，什么都不写
        let path = root.join("tables/public/orders.yaml");
        std::fs::write(&path, std::fs::read_to_string(&path).unwrap() + "comment: x\n").unwrap();
        assert!(store.write_back(&id, "public", &fps, writes.clone(), &[]).is_err());
        assert!(root.join("tables/public/clients.yaml").exists());
        let fps = fingerprints(&store, &id);
        store.write_back(&id, "public", &fps, writes, &[("clients".into(), None)]).unwrap();
        let names: Vec<_> = store.tables(&id).unwrap().into_iter().map(|e| e.name).collect();
        assert_eq!(names, ["customers", "items"]);
        assert!(!root.join("tables/public/clients.yaml").exists());
        assert!(store.renames(&id, "public").unwrap().is_empty());
    }
}
```

- [ ] **Step 10: 检查**

```bash
export CARGO_INCREMENTAL=0
cargo fmt --all
cargo clippy --config "source.mirror.registry='sparse+https://rsproxy.cn/index/'" --workspace --all-targets -- -D warnings
DBLENS_LANG=en cargo test --config "source.mirror.registry='sparse+https://rsproxy.cn/index/'" --workspace
```

Expected: 全部测试通过（service 新增 10 个，快照 2 个）。

- [ ] **Step 11: 提交**

```bash
git add Cargo.lock Cargo.toml crates/dblens-core/locales/app.json crates/dblens-service/Cargo.toml crates/dblens-service/src/lib.rs crates/dblens-service/src/model/file.rs crates/dblens-service/src/model/mod.rs crates/dblens-service/src/model/snapshots/dblens_service__model__file__tests__info_yaml.snap crates/dblens-service/src/model/snapshots/dblens_service__model__file__tests__table_yaml.snap crates/dblens-service/src/model/store.rs
git commit -m "feat(service): 模型目录的读写" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 4: 服务层：模型的新建、反向生成与设计器保存

- `Service.models: ModelStore`。
- `model/ops.rs`：最近打开的模型记在设置 `ui.models.recent`（至多 10 个）；`open_model`、`create_model`；`start_reverse`：作为任务（`TaskKind::Reverse`）执行，逐个 schema 跳过元数据缓存读表（不要触发器），全部读完再写目录（失败不留下目录），模型的数据库类型取连接识别出的变体、兼容模式和版本，结束后任务状态的 `model` 是打开的模型；`preview_model_design`（校验、提示、变更列表、指纹冲突）、`save_model_design`（冲突或有校验错误时拒绝）。
- `TaskKind::Reverse`；`TaskStatus.model`。
- 后端词条加 `model.no_schemas`。

**Files:**
- Modify: `crates/dblens-core/locales/app.json`
- Modify: `crates/dblens-service/src/lib.rs`
- Modify: `crates/dblens-service/src/model/mod.rs`
- Create: `crates/dblens-service/src/model/ops.rs`
- Modify: `crates/dblens-service/src/service.rs`
- Modify: `crates/dblens-service/src/task.rs`

**Interfaces:**
- Consumes：任务 3 的 `ModelStore`；M3 的 `validate`、`diff_table`、`prepare_draft`。
- Produces：`Service::{recent_models, open_model, create_model, start_reverse, preview_model_design, save_model_design}`；`NewModel { path, name, database, schemas }`、`ReverseRequest { profile_id, schemas, path, name }`、`ModelDesignRequest { schema, name, draft, renames, fingerprint }`、`ModelDesignPreview { errors, warnings, changes, conflict }`、`RECENT_MODELS_SETTING`。

- [ ] **Step 1: 修改 `crates/dblens-core/locales/app.json`**

```diff
--- a/crates/dblens-core/locales/app.json
+++ b/crates/dblens-core/locales/app.json
@@ -145,6 +145,7 @@
   "model.conflict": {"zh-CN": "%{name} 的文件已被修改，请重新加载", "en": "The file for %{name} has changed; reload it"},
   "model.dir_not_empty": {"zh-CN": "目录 %{path} 不为空", "en": "Directory %{path} is not empty"},
   "model.io_failed": {"zh-CN": "读写 %{path} 失败：%{error}", "en": "Failed to access %{path}: %{error}"},
+  "model.no_schemas": {"zh-CN": "没有选择 schema", "en": "No schema selected"},
   "model.not_a_model": {"zh-CN": "%{path} 不是模型目录（没有 model.yaml）", "en": "%{path} is not a model directory (model.yaml not found)"},
   "model.read_only": {"zh-CN": "模型文件格式 %{format} 比本程序新，只能查看；请升级数洞", "en": "Model format %{format} is newer than this version supports; it is read-only. Please upgrade DBLens"},
   "model.table_exists": {"zh-CN": "表 %{name} 已存在", "en": "Table %{name} already exists"},
```

- [ ] **Step 2: 修改 `crates/dblens-service/src/lib.rs`**

```diff
--- a/crates/dblens-service/src/lib.rs
+++ b/crates/dblens-service/src/lib.rs
@@ -40,8 +40,9 @@ pub use import::{
 };
 pub use metadata::{METADATA_POOL_SIZE, MetadataCache, MetadataService};
 pub use model::{
-    FileError, ModelDatabase, ModelInfo, ModelStore, ModelSummary, ModelTableView, ModelWrite, SchemaRenames,
-    SchemaTables, TableEntry,
+    FileError, ModelDatabase, ModelDesignPreview, ModelDesignRequest, ModelInfo, ModelStore, ModelSummary,
+    ModelTableView, ModelWrite, NewModel, RECENT_MODELS_SETTING, ReverseRequest, SchemaRenames, SchemaTables,
+    TableEntry,
 };
 pub use objects::{DataOrder, GenerateKind, ObjectKind};
 pub use policy::{
```

- [ ] **Step 3: 修改 `crates/dblens-service/src/model/mod.rs`**

```diff
--- a/crates/dblens-service/src/model/mod.rs
+++ b/crates/dblens-service/src/model/mod.rs
@@ -1,7 +1,9 @@
 //! 模型（M4a）：模型目录的读写、打开的模型。
 
 mod file;
+mod ops;
 mod store;
 
 pub use file::{ModelDatabase, ModelInfo, SchemaRenames};
+pub use ops::{ModelDesignPreview, ModelDesignRequest, NewModel, RECENT_MODELS_SETTING, ReverseRequest};
 pub use store::{FileError, ModelStore, ModelSummary, ModelTableView, ModelWrite, SchemaTables, TableEntry};
```

- [ ] **Step 4: 新建 `crates/dblens-service/src/model/ops.rs`**

```rust
//! 模型的服务层操作（M4a 设计文档第 4 节）：最近打开的模型、新建、反向生成、设计器保存。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use dblens_core::schema::Table;
use dblens_core::{Error, Msg, Result, msg};
use dblens_dialect::design::{DesignIssue, Renames, TableChange, diff_table, validate};
use dblens_dialect::meta::{MetaArgs, MetaKey};
use serde::{Deserialize, Serialize};

use super::file::ModelDatabase;
use super::store::ModelSummary;
use crate::service::Service;
use crate::task::{TaskKind, TaskPhase};

/// 最近打开的模型目录（JSON 数组，最近的在前）。
pub const RECENT_MODELS_SETTING: &str = "ui.models.recent";
const RECENT_MAX: usize = 10;

/// 新建空模型。
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct NewModel {
    pub path: String,
    pub name: String,
    pub database: ModelDatabase,
    #[serde(default)]
    pub schemas: Vec<String>,
}

/// 从库反向生成模型。
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ReverseRequest {
    pub profile_id: String,
    pub schemas: Vec<String>,
    pub path: String,
    pub name: String,
}

/// 设计器保存模型中的表。
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ModelDesignRequest {
    pub schema: String,
    /// 已有表的名字；新建表时不给。
    #[serde(default)]
    pub name: Option<String>,
    pub draft: Table,
    #[serde(default)]
    pub renames: Renames,
    /// 打开时文件的指纹；保存前核对。
    #[serde(default)]
    pub fingerprint: Option<String>,
}

/// 保存前的检查：校验错误、提示、变更列表。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ModelDesignPreview {
    pub errors: Vec<DesignIssue>,
    pub warnings: Vec<Msg>,
    pub changes: Vec<TableChange>,
    /// 文件已被修改（与请求中的指纹不同），要重新加载。
    pub conflict: bool,
}

impl Service {
    pub fn recent_models(&self) -> Result<Vec<String>> {
        let value = self.store.setting(RECENT_MODELS_SETTING)?;
        Ok(value.and_then(|v| serde_json::from_str(&v).ok()).unwrap_or_default())
    }

    fn remember_model(&self, path: &str) -> Result<()> {
        let mut list = self.recent_models()?;
        list.retain(|p| p != path);
        list.insert(0, path.to_string());
        list.truncate(RECENT_MAX);
        let text = serde_json::to_string(&list).unwrap_or_default();
        self.store.set_setting(RECENT_MODELS_SETTING, Some(&text))
    }

    pub fn open_model(&self, path: &Path) -> Result<ModelSummary> {
        let s = self.models.open(path)?;
        self.remember_model(&s.path)?;
        Ok(s)
    }

    pub fn create_model(&self, req: NewModel) -> Result<ModelSummary> {
        let s = self.models.create(Path::new(&req.path), &req.name, req.database, req.schemas)?;
        self.remember_model(&s.path)?;
        Ok(s)
    }

    /// 反向生成：作为任务执行，返回任务 id；结束后模型已打开，任务状态的 `model` 是模型编号。
    pub async fn start_reverse(self: &Arc<Self>, req: ReverseRequest) -> Result<String> {
        let profile = self.profile(&req.profile_id)?;
        if req.schemas.is_empty() {
            return Err(Error::invalid(msg!("model.no_schemas")));
        }
        let root = std::path::absolute(&req.path).unwrap_or_else(|_| PathBuf::from(&req.path));
        if root.join("model.yaml").exists() || std::fs::read_dir(&root).is_ok_and(|mut d| d.next().is_some())
        {
            return Err(Error::invalid(msg!("model.dir_not_empty", path = root.display())));
        }
        let dialect = self.metadata.dialect(&profile).await?;
        let task = self.tasks.create(TaskKind::Reverse)?;
        let mut id = String::new();
        task.update(|s| {
            id = s.id.clone();
            s.phase = TaskPhase::Scanning;
            s.progress.total = req.schemas.len() as u64;
        });
        let service = self.clone();
        tokio::spawn(async move {
            let run = async {
                let mut tables = Vec::new();
                for schema in &req.schemas {
                    if task.cancelled() {
                        return Err(Error::Cancelled);
                    }
                    // 跳过元数据缓存，以库中最新的结构为准
                    let args = MetaArgs { schema: Some(schema.clone()), ..Default::default() };
                    for key in MetaKey::TABLE_DETAIL {
                        service.metadata.cache().forget(&profile.id, key, &args);
                    }
                    let list: Vec<Table> = service
                        .metadata
                        .tables(&profile, schema, None)
                        .await?
                        .into_iter()
                        .map(|t| Table { triggers: vec![], ..t })
                        .collect();
                    tables.push((schema.clone(), list));
                    task.update(|s| s.progress.read += 1);
                }
                task.update(|s| s.phase = TaskPhase::Writing);
                let database = ModelDatabase {
                    variant: dialect.variant(),
                    compat: dialect.compat(),
                    version: dialect.version(),
                };
                let m = service.models.create_from(&root, &req.name, database, &tables)?;
                service.remember_model(&m.path)?;
                task.update(|s| s.model = Some(m.id.clone()));
                Ok(())
            };
            let result = run.await;
            task.finish(result);
        });
        Ok(id)
    }

    async fn prepare_model_design(
        &self,
        id: &str,
        req: &ModelDesignRequest,
    ) -> Result<(ModelDesignPreview, Table)> {
        let dialect = self.models.dialect(id)?;
        let mut draft = req.draft.clone();
        if let Some(name) = &req.name {
            // 表名在模型面板中改
            draft.name = name.clone();
        }
        let warnings = dialect.prepare_draft(&mut draft);
        let mut preview =
            ModelDesignPreview { errors: validate(&draft), warnings, changes: vec![], conflict: false };
        let original = match &req.name {
            Some(n) => Some(self.models.read_table(id, &req.schema, n)?),
            None => None,
        };
        preview.conflict =
            matches!((&original, &req.fingerprint), (Some(o), Some(fp)) if &o.fingerprint != fp);
        if preview.errors.is_empty() && !preview.conflict {
            let original = original.map(|o| o.table);
            preview.changes = diff_table(original.as_ref(), &draft, &req.renames)?.changes;
        }
        Ok((preview, draft))
    }

    /// 保存前的检查（不写文件）。
    pub async fn preview_model_design(
        &self,
        id: &str,
        req: &ModelDesignRequest,
    ) -> Result<ModelDesignPreview> {
        Ok(self.prepare_model_design(id, req).await?.0)
    }

    /// 保存这张表的文件，返回新的指纹。有校验错误、文件已被修改时报错。
    pub async fn save_model_design(&self, id: &str, req: ModelDesignRequest) -> Result<String> {
        let (preview, draft) = self.prepare_model_design(id, &req).await?;
        if preview.conflict {
            let name = format!("{}.{}", req.schema, draft.name);
            return Err(Error::invalid(msg!("model.conflict", name = name)));
        }
        if !preview.errors.is_empty() {
            return Err(Error::invalid(msg!("design.has_errors")));
        }
        let columns: BTreeMap<String, String> = req.renames.columns.clone();
        self.models.save_table(
            id,
            &req.schema,
            req.name.as_deref(),
            draft,
            &columns,
            req.fingerprint.as_deref(),
        )
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use dblens_core::schema::{Column, PrimaryKey};
    use dblens_core::{ColumnType, LogicalType};
    use dblens_dialect::{Compat, Variant};

    use super::*;
    use crate::fake::{FakeDriver, Log};
    use crate::history::HistoryLog;
    use crate::policy::DefaultPolicy;
    use crate::profile::tests::input;
    use crate::registry::DriverRegistry;
    use crate::secret::{SecretBackend, Secrets};
    use crate::store::Store;
    use crate::task::{TaskState, TaskStatus};

    fn service() -> (Arc<Service>, String, tempfile::TempDir) {
        let store = Arc::new(Store::open_in_memory().unwrap());
        let secrets = Arc::new(Secrets::with_backend(store.clone(), SecretBackend::Vault));
        secrets.init_vault("master").unwrap();
        let log: Log = Arc::default();
        let registry =
            Arc::new(DriverRegistry::with_drivers(store.clone(), vec![Box::new(FakeDriver { log })]));
        let s = Service::with_history(
            store.clone(),
            secrets,
            registry,
            Arc::new(DefaultPolicy),
            HistoryLog::inline(store),
        );
        let dir = tempfile::tempdir().unwrap();
        s.tasks.set_dirs(dir.path().join("tasks"), dir.path().join("imports"));
        let p = s.create_profile(input("dev"), Some("pw")).unwrap();
        (Arc::new(s), p.id, dir)
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

    fn draft(name: &str, cols: &[&str]) -> Table {
        Table {
            name: name.into(),
            columns: cols
                .iter()
                .map(|c| Column {
                    name: (*c).into(),
                    ordinal: 0,
                    data_type: ColumnType { native: "integer".into(), logical: LogicalType::Other },
                    nullable: true,
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

    #[tokio::test]
    async fn create_remembers_recent_and_design_saves_files() {
        let (s, _, dir) = service();
        let path = dir.path().join("model");
        let db = ModelDatabase { variant: Variant::Postgres, compat: Compat::Native, version: None };
        let req =
            NewModel { path: path.to_string_lossy().into(), name: "m".into(), database: db, schemas: vec![] };
        let m = s.create_model(req).unwrap();
        assert_eq!(s.recent_models().unwrap(), std::slice::from_ref(&m.path));
        // 新建表：主键列改为 NOT NULL 的提示，与 M3 相同
        let mut req = ModelDesignRequest {
            schema: "public".into(),
            name: None,
            draft: draft("t", &["id", "a"]),
            renames: Renames::default(),
            fingerprint: None,
        };
        let p = s.preview_model_design(&m.id, &req).await.unwrap();
        assert_eq!(p.warnings, vec![msg!("design.pk_not_null", column = "id")]);
        assert!(matches!(p.changes[..], [TableChange::CreateTable { .. }]));
        let fp = s.save_model_design(&m.id, req.clone()).await.unwrap();
        // 改列名
        req.name = Some("t".into());
        req.draft = draft("t", &["id", "b"]);
        req.renames.columns.insert("b".into(), "a".into());
        req.fingerprint = Some(fp);
        let p = s.preview_model_design(&m.id, &req).await.unwrap();
        assert!(matches!(p.changes[..], [TableChange::RenameColumn { .. }]), "{:?}", p.changes);
        s.save_model_design(&m.id, req.clone()).await.unwrap();
        assert_eq!(s.models.renames(&m.id, "public").unwrap().columns["t"]["b"], "a");
        // 用旧指纹预览标出冲突、保存被拒绝；校验错误不保存
        assert!(s.preview_model_design(&m.id, &req).await.unwrap().conflict);
        let err = s.save_model_design(&m.id, req.clone()).await.unwrap_err();
        assert!(err.to_string().contains("changed"), "{err}");
        req.fingerprint = Some(s.models.read_table(&m.id, "public", "t").unwrap().fingerprint);
        req.renames = Renames::default();
        req.draft.columns[0].data_type.native = String::new();
        let err = s.save_model_design(&m.id, req).await.unwrap_err();
        assert!(err.to_string().contains("error"), "{err}");
    }

    /// 假驱动没有元数据结果：任务失败，不留下模型目录（成功的流程在实库测试中）。
    #[tokio::test]
    async fn reverse_task_failure_leaves_nothing() {
        let (s, dev, dir) = service();
        let path = dir.path().join("rev");
        let req = ReverseRequest {
            profile_id: dev.clone(),
            schemas: vec!["public".into()],
            path: path.to_string_lossy().into(),
            name: "rev".into(),
        };
        let id = s.start_reverse(req.clone()).await.unwrap();
        let st = wait(&s, &id).await;
        assert_eq!((st.state, st.model), (TaskState::Failed, None));
        assert!(!path.exists());
        // 没有选 schema、目录不为空：不启动任务
        let none = ReverseRequest { schemas: vec![], ..req.clone() };
        assert!(s.start_reverse(none).await.is_err());
        std::fs::create_dir_all(&path).unwrap();
        std::fs::write(path.join("x.txt"), "x").unwrap();
        let err = s.start_reverse(req).await.unwrap_err();
        assert!(err.to_string().contains("not empty"), "{err}");
    }
}
```

- [ ] **Step 5: 修改 `crates/dblens-service/src/service.rs`**

```diff
--- a/crates/dblens-service/src/service.rs
+++ b/crates/dblens-service/src/service.rs
@@ -9,6 +9,7 @@ use dblens_dialect::{Dialect, OpsQuery, Variant};
 use crate::export::ExportManager;
 use crate::history::HistoryLog;
 use crate::metadata::{MetadataCache, MetadataService};
+use crate::model::ModelStore;
 use crate::policy::{DefaultPolicy, Policy};
 use crate::profile::{Profile, ProfileInput};
 use crate::registry::{DriverRegistry, ServerSummary};
@@ -30,6 +31,8 @@ pub struct Service {
     pub exports: ExportManager,
     /// 导入和执行 SQL 文件的任务、上传的文件（M2b）。
     pub tasks: TaskManager,
+    /// 打开的模型（M4a）。
+    pub models: ModelStore,
     pub(crate) history: HistoryLog,
 }
 
@@ -78,6 +81,7 @@ impl Service {
             metadata: MetadataService::new(registry.clone(), secrets.clone(), cache),
             exports: ExportManager::default(),
             tasks: TaskManager::default(),
+            models: ModelStore::default(),
             history,
             store,
             secrets,
```

- [ ] **Step 6: 修改 `crates/dblens-service/src/task.rs`**

```diff
--- a/crates/dblens-service/src/task.rs
+++ b/crates/dblens-service/src/task.rs
@@ -26,6 +26,8 @@ pub enum TaskKind {
     SqlFile,
     /// 表设计器执行改动（M3）。
     Design,
+    /// 从库反向生成模型（M4a）。
+    Reverse,
 }
 
 #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
@@ -113,6 +115,8 @@ pub struct TaskStatus {
     pub report: Option<String>,
     /// 表设计器：失败后没有执行的语句（以 `;` 分隔），可在查询标签中打开手动执行。
     pub remaining: Option<String>,
+    /// 反向生成：生成并打开的模型的编号。
+    pub model: Option<String>,
 }
 
 struct Entry {
@@ -399,6 +403,7 @@ impl TaskManager {
                 failures: Vec::new(),
                 confirm: None,
                 remaining: None,
+                model: None,
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

Expected: 全部测试通过（service 新增 2 个）。

- [ ] **Step 8: 提交**

```bash
git add crates/dblens-core/locales/app.json crates/dblens-service/src/lib.rs crates/dblens-service/src/model/mod.rs crates/dblens-service/src/model/ops.rs crates/dblens-service/src/service.rs crates/dblens-service/src/task.rs
git commit -m "feat(service): 模型的新建、反向生成与设计器保存" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 5: 服务层：结构比对与同步

- `design.rs`：执行器改为按段执行：`Part::{Sql, Rebuild}`，`Run::execute(parts, password)`；多段时失败后列出的"没有执行的语句"含后面各段（重建在删除旧表之前失败时这张表的改动已撤销，只列后面的段）；执行策略的检查抽成 `check_policy`（参数为 `&[String]`，避免异步函数中借用迭代器导致接口层的 `Send` 检查失败）；`fresh_table`、`rebuild_context` 改为 `pub(crate)`。
- `compare.rs`：比对的一边是模型或连接的一个 schema（连接一边跳过元数据缓存读表）；两边家族不同时拒绝；按目标的方言比对；改名合并模型记下的（模型是目标时反过来）和请求中的，只保留两边都有的表和列；结果带两边的表和指纹（比对结果加目标模型文件的指纹）。同步预览：目标是库时生成脚本（要重建的表先读依赖信息）并检查执行策略；目标是模型时列出要写的文件。同步：重新比对，指纹不同时拒绝；目标是模型时直接写回（先核对文件指纹，删去已应用的改名）；目标是库时作为任务（`TaskKind::Sync`）执行，结束后刷新目标 schema 的缓存，源是模型时删去已应用的改名。
- 后端词条加 `compare.family_mismatch`、`compare.changed`、`compare.rebuild_refused`。

**Files:**
- Modify: `crates/dblens-core/locales/app.json`
- Create: `crates/dblens-service/src/compare.rs`
- Modify: `crates/dblens-service/src/design.rs`
- Modify: `crates/dblens-service/src/lib.rs`
- Modify: `crates/dblens-service/src/task.rs`

**Interfaces:**
- Consumes：任务 2 的比对和同步脚本、任务 3 的 `ModelStore`。
- Produces：`Service::{compare, preview_sync, start_sync}`；`CompareSideRef::{Model { model, schema }, Database { profile, schema }}`、`CompareRequest { source, target, options, renames }`、`CompareResult { diff, renames, source_errors, target_errors, source_tables, target_tables, fingerprint }`、`SyncRequest { compare, selected, fingerprint }`、`SyncPreview { script, text, writes, denied, confirm, fingerprint }`、`ModelWriteInfo { name, from, delete }`、`SyncStarted { task }`；crate 内 `design::{Part, Run}`。

- [ ] **Step 1: 修改 `crates/dblens-core/locales/app.json`**

```diff
--- a/crates/dblens-core/locales/app.json
+++ b/crates/dblens-core/locales/app.json
@@ -18,6 +18,9 @@
   "changes.rolled_back": {"zh-CN": "已回滚（同一次提交中的其他改动失败）", "en": "Rolled back (another change in the same commit failed)"},
   "changes.unknown_column": {"zh-CN": "列 %{column} 不存在", "en": "Column %{column} does not exist"},
   "compare.apply_missing": {"zh-CN": "找不到 %{name}：模型中的表与比对时不同，请重新比对", "en": "%{name} was not found: the model table differs from the compared one; compare again"},
+  "compare.changed": {"zh-CN": "比对之后结构已有变化，请重新比对", "en": "The schemas changed after the comparison; compare again"},
+  "compare.family_mismatch": {"zh-CN": "两边的数据库类型不同（%{source} 与 %{target}），不能比对", "en": "Cannot compare different database types (%{source} and %{target})"},
+  "compare.rebuild_refused": {"zh-CN": "表 %{table} 不能重建：%{reason}", "en": "Table %{table} cannot be rebuilt: %{reason}"},
   "ddl.truncated": {"zh-CN": "DDL 超过 64 KB，以上只是开头部分", "en": "The DDL exceeds 64 KB; only the beginning is shown above"},
   "ddl.unsupported": {"zh-CN": "该数据库不支持查看 %{kind} 的 DDL", "en": "This database does not support viewing the DDL of %{kind}"},
   "design.bad_rename": {"zh-CN": "列 %{name} 的原名不对", "en": "Column %{name} has an invalid original name"},
```

- [ ] **Step 2: 新建 `crates/dblens-service/src/compare.rs`**

```rust
//! 结构比对与同步（M4a 设计文档第 5、6 节）：读取两边（模型或库的一个 schema）、比对、
//! 生成同步脚本并作为任务执行，或把变更写回模型。比对本身在 `dblens_dialect::compare`。

use std::collections::{BTreeMap, BTreeSet};
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use dblens_core::schema::Table;
use dblens_core::{Error, Msg, Result, msg};
use dblens_dialect::Dialect;
use dblens_dialect::compare::{
    CompareOptions, CompareRenames, CompareSide, SchemaChange, SchemaDiff, SyncScript, SyncStep,
    apply_changes, compare_schemas, sync_rebuild_tables, sync_script,
};
use dblens_dialect::design::{RebuildContext, TableChange};
use dblens_dialect::meta::{MetaArgs, MetaKey};
use serde::{Deserialize, Serialize};

use crate::design::{Part, Run};
use crate::model::{FileError, ModelWrite, SchemaRenames};
use crate::profile::Profile;
use crate::service::Service;
use crate::task::{TaskKind, TaskPhase};

/// 比对的一边：模型的一个 schema，或连接的一个 schema。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CompareSideRef {
    Model { model: String, schema: String },
    Database { profile: String, schema: String },
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct CompareRequest {
    pub source: CompareSideRef,
    pub target: CompareSideRef,
    #[serde(default)]
    pub options: CompareOptions,
    /// 用户确认采用的推测改名（模型中记下的改名自动采用，不必在这里给出）。
    #[serde(default)]
    pub renames: CompareRenames,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CompareResult {
    pub diff: SchemaDiff,
    /// 实际采用的改名（模型记下的加上请求中的）。
    pub renames: CompareRenames,
    /// 两边文件有错、没有参与比对的表。
    pub source_errors: Vec<FileError>,
    pub target_errors: Vec<FileError>,
    /// 两边的表（界面中对照显示）。
    pub source_tables: Vec<Table>,
    pub target_tables: Vec<Table>,
    /// 比对结果的指纹：同步前重新比对，结果不同则拒绝。
    pub fingerprint: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct SyncRequest {
    pub compare: CompareRequest,
    pub selected: BTreeSet<u32>,
    /// 预览时比对结果的指纹。
    #[serde(default)]
    pub fingerprint: Option<String>,
}

/// 写回模型时要改的一个表文件。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ModelWriteInfo {
    /// 写入后的表名（删除时为被删的表）。
    pub name: String,
    /// 改名前的名字。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
    pub delete: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SyncPreview {
    /// 目标是库：同步脚本；目标是模型：None。
    pub script: Option<SyncScript>,
    /// 脚本全文（复制、在查询标签中打开、另存为）。
    pub text: String,
    /// 目标是模型：要写的文件。
    pub writes: Vec<ModelWriteInfo>,
    pub denied: Option<Msg>,
    pub confirm: Vec<Msg>,
    pub fingerprint: String,
}

/// 同步的结果：目标是库时为任务 id；目标是模型时已经写完。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SyncStarted {
    pub task: Option<String>,
}

/// 读出的一边。
struct Side {
    dialect: Dialect,
    schema: String,
    tables: Vec<Table>,
    fingerprints: BTreeMap<String, String>,
    errors: Vec<FileError>,
    /// 模型中记下的改名。
    renames: Option<SchemaRenames>,
    profile: Option<Profile>,
}

/// 比对的全部结果（预览和执行共用）。
struct Compared {
    target: Side,
    result: CompareResult,
}

/// 已应用的改名：（表名，列名）；列名为 None 时是表的改名。
type Cleared = Vec<(String, Option<String>)>;

fn hash_of<T: Serialize>(v: &T) -> String {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    serde_json::to_string(v).unwrap_or_default().hash(&mut h);
    format!("{:016x}", h.finish())
}

/// 合并改名：模型是源时记下的改名（新名 → 原名）就是源 → 目标；模型是目标时反过来。
/// 只保留两边都有的表和列（记录过时的不报错）。
fn effective_renames(source: &Side, target: &Side, requested: &CompareRenames) -> CompareRenames {
    let mut out = CompareRenames::default();
    if let Some(r) = &source.renames {
        out.tables.extend(r.tables.clone());
        out.columns.extend(r.columns.clone());
    }
    if let Some(r) = &target.renames {
        let inverse = |m: &BTreeMap<String, String>| -> BTreeMap<String, String> {
            m.iter().map(|(k, v)| (v.clone(), k.clone())).collect()
        };
        out.tables.extend(inverse(&r.tables));
        for (t, cols) in &r.columns {
            let source_table = r.tables.get(t).cloned().unwrap_or_else(|| t.clone());
            out.columns.entry(source_table).or_default().extend(inverse(cols));
        }
    }
    out.tables.extend(requested.tables.clone());
    for (t, cols) in &requested.columns {
        out.columns.entry(t.clone()).or_default().extend(cols.clone());
    }
    let find = |tables: &[Table], name: &str| tables.iter().find(|t| t.name == name).cloned();
    out.tables.retain(|s, t| find(&source.tables, s).is_some() && find(&target.tables, t).is_some());
    let renames_tables = out.tables.clone();
    out.columns.retain(|s, cols| {
        let target_name = renames_tables.get(s).cloned().unwrap_or_else(|| s.clone());
        let (Some(st), Some(tt)) = (find(&source.tables, s), find(&target.tables, &target_name)) else {
            return false;
        };
        cols.retain(|a, b| {
            st.columns.iter().any(|c| &c.name == a)
                && tt.columns.iter().any(|c| &c.name == b)
                && !st.columns.iter().any(|c| &c.name == b)
        });
        !cols.is_empty()
    });
    out
}

/// 同步脚本全文：每条语句以 `;` 结尾，各段之间空一行。
fn script_text(script: &SyncScript) -> String {
    let parts: Vec<String> = script
        .steps
        .iter()
        .map(|s| part_of(s).texts().into_iter().map(|t| format!("{t};")).collect::<Vec<_>>().join("\n"))
        .collect();
    parts.join("\n\n")
}

fn part_of(step: &SyncStep) -> Part {
    match step {
        SyncStep::Sql { sql, .. } => Part::Sql(sql.clone()),
        SyncStep::Rebuild { rebuild, .. } => Part::Rebuild(rebuild.clone()),
    }
}

/// 写回模型：勾选的变更换算成表文件的写入、删除；同时得出已应用的改名（从模型的改名记录中删去）。
fn model_writes(
    diff: &SchemaDiff,
    selected: &BTreeSet<u32>,
    target: &[Table],
) -> Result<(Vec<ModelWrite>, Cleared)> {
    let mut writes = Vec::new();
    let mut cleared = Vec::new();
    for t in &diff.tables {
        let picked: Vec<&SchemaChange> =
            t.items.iter().filter(|i| selected.contains(&i.id)).map(|i| &i.change).collect();
        if picked.is_empty() {
            continue;
        }
        let mut changes = Vec::new();
        let mut rename = None;
        for c in picked {
            match c {
                SchemaChange::DropTable { name } => {
                    writes.push(ModelWrite::Delete { name: name.clone() });
                }
                SchemaChange::RenameTable { from, to } => {
                    rename = Some(to.clone());
                    cleared.push((from.clone(), None));
                }
                SchemaChange::Table { change } => match change.as_ref() {
                    TableChange::CreateTable { table } => {
                        writes.push(ModelWrite::Put { table: Box::new(table.clone()), path_of: None });
                    }
                    other => {
                        if let TableChange::RenameColumn { from, .. } = other {
                            cleared.push((t.target.clone().unwrap_or_default(), Some(from.clone())));
                        }
                        changes.push(other.clone());
                    }
                },
            }
        }
        let Some(target_name) = &t.target else { continue };
        if changes.is_empty() && rename.is_none() {
            continue;
        }
        let original = target.iter().find(|x| &x.name == target_name).cloned().unwrap_or_default();
        let mut table = apply_changes(&original, &changes)?;
        if let Some(to) = rename {
            table.name = to;
        }
        writes.push(ModelWrite::Put { table: Box::new(table), path_of: Some(target_name.clone()) });
    }
    Ok((writes, cleared))
}

/// 同步到库后，源模型中已应用的改名：表的改名按源的名字（新名），列的改名按源的表名和列名。
fn applied_renames(diff: &SchemaDiff, selected: &BTreeSet<u32>) -> Cleared {
    let mut out = Vec::new();
    for t in &diff.tables {
        for i in t.items.iter().filter(|i| selected.contains(&i.id)) {
            match &i.change {
                SchemaChange::RenameTable { to, .. } => out.push((to.clone(), None)),
                SchemaChange::Table { change } => {
                    if let TableChange::RenameColumn { to, .. } = change.as_ref() {
                        out.push((t.source.clone().unwrap_or_default(), Some(to.clone())));
                    }
                }
                SchemaChange::DropTable { .. } => {}
            }
        }
    }
    out
}

impl Service {
    async fn load_side(&self, side: &CompareSideRef) -> Result<Side> {
        match side {
            CompareSideRef::Model { model, schema } => {
                let s = self.models.schema_tables(model, schema)?;
                Ok(Side {
                    dialect: self.models.dialect(model)?,
                    schema: schema.clone(),
                    fingerprints: s.tables.iter().map(|(t, fp)| (t.name.clone(), fp.clone())).collect(),
                    tables: s.tables.into_iter().map(|(t, _)| t).collect(),
                    errors: s.errors,
                    renames: Some(self.models.renames(model, schema)?),
                    profile: None,
                })
            }
            CompareSideRef::Database { profile, schema } => {
                let profile = self.profile(profile)?;
                let dialect = self.metadata.dialect(&profile).await?;
                // 跳过元数据缓存，以库中最新的结构为准
                let args = MetaArgs { schema: Some(schema.clone()), ..Default::default() };
                for key in MetaKey::TABLE_DETAIL {
                    self.metadata.cache().forget(&profile.id, key, &args);
                }
                let tables = self.metadata.tables(&profile, schema, None).await?;
                Ok(Side {
                    dialect,
                    schema: schema.clone(),
                    tables: tables.into_iter().map(|t| Table { triggers: vec![], ..t }).collect(),
                    fingerprints: BTreeMap::new(),
                    errors: vec![],
                    renames: None,
                    profile: Some(profile),
                })
            }
        }
    }

    async fn run_compare(&self, req: &CompareRequest) -> Result<Compared> {
        let source = self.load_side(&req.source).await?;
        let target = self.load_side(&req.target).await?;
        if source.dialect.family() != target.dialect.family() {
            return Err(Error::invalid(msg!(
                "compare.family_mismatch",
                source = format!("{:?}", source.dialect.variant()),
                target = format!("{:?}", target.dialect.variant())
            )));
        }
        let renames = effective_renames(&source, &target, &req.renames);
        // 生成的语句在目标上执行，按目标的方言比对
        let diff = compare_schemas(
            &target.dialect,
            &CompareSide { schema: &source.schema, tables: &source.tables },
            &CompareSide { schema: &target.schema, tables: &target.tables },
            &renames,
            &req.options,
        )?;
        let result = CompareResult {
            fingerprint: hash_of(&(&diff, &target.fingerprints)),
            diff,
            renames,
            source_errors: source.errors.clone(),
            target_errors: target.errors.clone(),
            source_tables: source.tables.clone(),
            target_tables: target.tables.clone(),
        };
        Ok(Compared { target, result })
    }

    pub async fn compare(&self, req: &CompareRequest) -> Result<CompareResult> {
        Ok(self.run_compare(req).await?.result)
    }

    /// 目标是库时的同步脚本：要重建的表先读取依赖信息。
    async fn sync_script_of(&self, c: &Compared, selected: &BTreeSet<u32>) -> Result<SyncScript> {
        let (d, profile) = (&c.target.dialect, c.target.profile.as_ref().expect("database target"));
        let mut contexts = BTreeMap::<String, RebuildContext>::new();
        for name in sync_rebuild_tables(d, &c.result.diff, selected) {
            let target_name = c
                .result
                .diff
                .tables
                .iter()
                .find(|t| t.source.as_deref() == Some(&name))
                .and_then(|t| t.target.clone())
                .unwrap_or_else(|| name.clone());
            let t = self.fresh_table(profile, &c.target.schema, &target_name).await?;
            contexts.insert(name, self.rebuild_context(profile, d, &c.target.schema, &t).await?);
        }
        sync_script(d, &c.target.schema, &c.result.diff, selected, &c.target.tables, &contexts)
    }

    async fn prepare_sync(&self, req: &SyncRequest) -> Result<(Compared, SyncPreview)> {
        let c = self.run_compare(&req.compare).await?;
        let mut preview = SyncPreview {
            script: None,
            text: String::new(),
            writes: vec![],
            denied: None,
            confirm: vec![],
            fingerprint: c.result.fingerprint.clone(),
        };
        match &c.target.profile {
            Some(profile) => {
                let script = self.sync_script_of(&c, &req.selected).await?;
                preview.text = script_text(&script);
                let texts: Vec<String> = script.steps.iter().flat_map(|s| part_of(s).texts()).collect();
                (preview.denied, preview.confirm) =
                    self.check_policy(profile, c.target.dialect, &texts).await;
                preview.script = Some(script);
            }
            None => {
                let (writes, _) = model_writes(&c.result.diff, &req.selected, &c.target.tables)?;
                preview.writes = writes
                    .iter()
                    .map(|w| match w {
                        ModelWrite::Put { table, path_of } => ModelWriteInfo {
                            name: table.name.clone(),
                            from: path_of.clone().filter(|p| p != &table.name),
                            delete: false,
                        },
                        ModelWrite::Delete { name } => {
                            ModelWriteInfo { name: name.clone(), from: None, delete: true }
                        }
                    })
                    .collect();
            }
        }
        Ok((c, preview))
    }

    pub async fn preview_sync(&self, req: &SyncRequest) -> Result<SyncPreview> {
        Ok(self.prepare_sync(req).await?.1)
    }

    /// 同步：重新比对，结果与预览时不同则拒绝。目标是库时作为任务执行（结束后刷新缓存、删去源模型中已应用的改名）；
    /// 目标是模型时直接写回。
    pub async fn start_sync(self: &Arc<Self>, req: SyncRequest) -> Result<SyncStarted> {
        if req.selected.is_empty() {
            return Err(Error::invalid(msg!("design.no_changes")));
        }
        let (c, preview) = self.prepare_sync(&req).await?;
        if req.fingerprint.as_deref().is_some_and(|fp| fp != preview.fingerprint) {
            return Err(Error::invalid(msg!("compare.changed")));
        }
        let CompareSideRef::Database { .. } = &req.compare.target else {
            let CompareSideRef::Model { model, schema } = &req.compare.target else { unreachable!() };
            let (writes, cleared) = model_writes(&c.result.diff, &req.selected, &c.target.tables)?;
            self.models.write_back(model, schema, &c.target.fingerprints, writes, &cleared)?;
            return Ok(SyncStarted { task: None });
        };
        let script = preview.script.expect("database target");
        if let Some((table, reason)) = script.refused.first() {
            return Err(Error::invalid(msg!("compare.rebuild_refused", table = table, reason = reason)));
        }
        if let Some(reason) = preview.denied {
            return Err(Error::Denied { reason });
        }
        let profile = c.target.profile.clone().expect("database target");
        let password = self.secrets.get(&profile.id)?.unwrap_or_default();
        let parts: Vec<Part> = script.steps.iter().map(part_of).collect();
        let task = self.tasks.create(TaskKind::Sync)?;
        let mut id = String::new();
        task.update(|s| {
            id = s.id.clone();
            s.phase = TaskPhase::Executing;
            s.progress.statements_total = Part::len(&parts);
        });
        let cleared = match &req.compare.source {
            CompareSideRef::Model { model, schema } => {
                Some((model.clone(), schema.clone(), applied_renames(&c.result.diff, &req.selected)))
            }
            CompareSideRef::Database { .. } => None,
        };
        let service = self.clone();
        let (dialect, schema) = (c.target.dialect, c.target.schema.clone());
        tokio::spawn(async move {
            let run = Run { service: &service, task: &task, profile: &profile, dialect, schema: &schema };
            let mut result = run.execute(parts, password).await;
            service.metadata.cache().invalidate(&profile.id, Some(&schema));
            if let (Ok(()), Some((model, mschema, list))) = (&result, cleared) {
                result = service.models.clear_renames(&model, &mschema, &list);
            }
            task.finish(result);
        });
        Ok(SyncStarted { task: Some(id) })
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use dblens_core::schema::{Column, PrimaryKey};
    use dblens_core::{ColumnType, LogicalType};
    use dblens_dialect::compare::TableStatus;
    use dblens_dialect::design::{Rebuild, RebuildStep};
    use dblens_dialect::{Compat, Variant};

    use super::*;
    use crate::fake::{FakeDriver, Log};
    use crate::history::HistoryLog;
    use crate::model::ModelDatabase;
    use crate::policy::DefaultPolicy;
    use crate::profile::tests::input;
    use crate::registry::DriverRegistry;
    use crate::secret::{SecretBackend, Secrets};
    use crate::store::Store;
    use crate::task::{TaskState, TaskStatus};

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
        let p = s.create_profile(input("dev"), Some("pw")).unwrap();
        (Arc::new(s), p.id, log, dir)
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

    fn table(name: &str, cols: &[&str]) -> Table {
        Table {
            name: name.into(),
            columns: cols
                .iter()
                .enumerate()
                .map(|(i, c)| Column {
                    name: (*c).into(),
                    ordinal: i as u32 + 1,
                    data_type: ColumnType {
                        native: "integer".into(),
                        logical: LogicalType::Int { bits: 32 },
                    },
                    nullable: i > 0,
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

    fn model(s: &Service, dir: &std::path::Path, name: &str, tables: Vec<Table>) -> String {
        let db = ModelDatabase { variant: Variant::Postgres, compat: Compat::Native, version: None };
        s.models.create_from(&dir.join(name), name, db, &[("public".into(), tables)]).unwrap().id
    }

    fn side(model: &str) -> CompareSideRef {
        CompareSideRef::Model { model: model.into(), schema: "public".into() }
    }

    fn all(diff: &SchemaDiff) -> BTreeSet<u32> {
        diff.tables.iter().flat_map(|t| t.items.iter().map(|i| i.id)).collect()
    }

    /// 源模型记下了改名：按改名配对；写回目标模型后两边没有差异。
    #[tokio::test]
    async fn model_to_model_uses_recorded_renames_and_writes_back() {
        let (s, _, _, dir) = service();
        let base = vec![table("customers", &["id", "name"]), table("old_items", &["id"])];
        let a = model(&s, dir.path(), "a", base.clone());
        let b = model(&s, dir.path(), "b", base);
        s.models.rename_table(&a, "public", "customers", "clients").unwrap();
        s.models
            .save_table(&a, "public", None, table("orders", &["id", "client_id"]), &BTreeMap::new(), None)
            .unwrap();
        s.models.delete_table(&a, "public", "old_items", None).unwrap();
        let req = CompareRequest {
            source: side(&a),
            target: side(&b),
            options: CompareOptions::default(),
            renames: CompareRenames::default(),
        };
        let r = s.compare(&req).await.unwrap();
        assert_eq!(r.renames.tables, BTreeMap::from([("clients".into(), "customers".into())]));
        let status: Vec<_> = r.diff.tables.iter().map(|t| (t.source.clone(), t.status)).collect();
        assert!(status.contains(&(Some("clients".into()), TableStatus::Renamed)), "{status:?}");
        assert!(status.contains(&(Some("orders".into()), TableStatus::Added)), "{status:?}");
        assert!(status.contains(&(None, TableStatus::Removed)), "{status:?}");
        let sync =
            SyncRequest { compare: req.clone(), selected: all(&r.diff), fingerprint: Some(r.fingerprint) };
        let p = s.preview_sync(&sync).await.unwrap();
        assert!(p.script.is_none());
        assert!(p.writes.contains(&ModelWriteInfo {
            name: "clients".into(),
            from: Some("customers".into()),
            delete: false
        }));
        assert!(p.writes.contains(&ModelWriteInfo { name: "old_items".into(), from: None, delete: true }));
        assert_eq!(s.start_sync(sync).await.unwrap(), SyncStarted { task: None });
        let again = s.compare(&req).await.unwrap();
        assert_eq!(again.diff.changed(), 0, "{:?}", again.diff);
    }

    /// 预览之后目标被改了：拒绝同步。
    #[tokio::test]
    async fn sync_refuses_when_target_changed() {
        let (s, _, _, dir) = service();
        let a = model(&s, dir.path(), "a", vec![table("t", &["id", "x"])]);
        let b = model(&s, dir.path(), "b", vec![table("t", &["id"])]);
        let req = CompareRequest {
            source: side(&a),
            target: side(&b),
            options: CompareOptions::default(),
            renames: CompareRenames::default(),
        };
        let r = s.compare(&req).await.unwrap();
        let sync = SyncRequest { compare: req, selected: all(&r.diff), fingerprint: Some(r.fingerprint) };
        s.models
            .save_table(&b, "public", Some("t"), table("t", &["id", "y"]), &BTreeMap::new(), None)
            .unwrap();
        let err = s.start_sync(sync).await.unwrap_err();
        assert!(err.to_string().contains("changed"), "{err}");
    }

    /// 执行器依次执行多段：MySQL（不在事务中）在第二段的重建删除旧表之前失败，没有执行的只列出后面的段。
    #[tokio::test]
    async fn executor_runs_parts_and_lists_remaining() {
        let (s, dev, log, _dir) = service();
        let profile = s.profile(&dev).unwrap();
        let task = s.tasks.create(TaskKind::Sync).unwrap();
        let mut id = String::new();
        task.update(|st| id = st.id.clone());
        let sql = |s: &str| RebuildStep::Sql { sql: s.into() };
        let rebuild = Rebuild {
            steps: vec![sql("CREATE TABLE t_new"), sql("FAIL"), sql("DROP TABLE t")],
            drop_at: 2,
            cleanup: "DROP TABLE t_new".into(),
            warnings: vec![],
        };
        let parts = vec![Part::Sql("ALTER A".into()), Part::Rebuild(rebuild), Part::Sql("ALTER B".into())];
        let run = Run {
            service: &s,
            task: &task,
            profile: &profile,
            dialect: Dialect::new(Variant::MySql),
            schema: "shop",
        };
        let r = run.execute(parts, "pw".into()).await;
        task.finish(r);
        let st = s.tasks.status(&id).unwrap();
        assert_eq!(st.state, TaskState::Failed);
        assert_eq!(st.failures[0].index, 2);
        assert_eq!(st.remaining.as_deref(), Some("ALTER B;"));
        let l = log.lock().unwrap().clone();
        assert!(l.contains(&"execute DROP TABLE t_new".to_string()), "{l:?}");
        assert!(!l.contains(&"execute ALTER B".to_string()), "{l:?}");
        let _ = wait(&s, &id).await;
    }
}
```

- [ ] **Step 3: 修改 `crates/dblens-service/src/design.rs`**

```diff
--- a/crates/dblens-service/src/design.rs
+++ b/crates/dblens-service/src/design.rs
@@ -92,6 +92,44 @@ enum Plan {
     Rebuild(Rebuild),
 }
 
+impl Plan {
+    fn parts(self) -> Vec<Part> {
+        match self {
+            Plan::Alter(statements) => statements.into_iter().map(|s| Part::Sql(s.sql)).collect(),
+            Plan::Rebuild(r) => vec![Part::Rebuild(r)],
+        }
+    }
+}
+
+/// 执行器依次执行的一段：一条语句，或一张表的重建（M4a 的同步脚本由多段组成）。
+pub(crate) enum Part {
+    Sql(String),
+    Rebuild(Rebuild),
+}
+
+impl Part {
+    /// 这一段的语句文本（核对行数写成两条 `SELECT COUNT(*)`）。
+    pub(crate) fn texts(&self) -> Vec<String> {
+        match self {
+            Part::Sql(sql) => vec![sql.clone()],
+            Part::Rebuild(r) => r
+                .steps
+                .iter()
+                .map(|s| match s {
+                    RebuildStep::Sql { sql } => sql.clone(),
+                    RebuildStep::CountCheck { source, target } => {
+                        format!("SELECT COUNT(*) FROM {source}; SELECT COUNT(*) FROM {target}")
+                    }
+                })
+                .collect(),
+        }
+    }
+
+    pub(crate) fn len(parts: &[Part]) -> u64 {
+        parts.iter().map(|p| p.texts().len() as u64).sum()
+    }
+}
+
 struct Prepared {
     profile: Profile,
     dialect: Dialect,
@@ -130,7 +168,7 @@ impl Service {
         Ok(self.metadata.rows(profile, key, &args).await?.unwrap_or_default())
     }
 
-    async fn fresh_table(&self, profile: &Profile, schema: &str, name: &str) -> Result<Table> {
+    pub(crate) async fn fresh_table(&self, profile: &Profile, schema: &str, name: &str) -> Result<Table> {
         let args = MetaArgs { schema: Some(schema.into()), table: Some(name.into()), ..Default::default() };
         for key in MetaKey::TABLE_DETAIL {
             self.metadata.cache().forget(&profile.id, key, &args);
@@ -150,7 +188,7 @@ impl Service {
     }
 
     /// 重建脚本要用的依赖信息。
-    async fn rebuild_context(
+    pub(crate) async fn rebuild_context(
         &self,
         profile: &Profile,
         dialect: &Dialect,
@@ -253,30 +291,45 @@ impl Service {
             }
         };
         preview.warnings = warnings;
-        // 执行策略：被拒绝的语句不能执行；需要确认的列出原因（生产环境总要确认）
-        let ctx = ExecContext {
-            profile_id: prepared.profile.id.clone(),
-            environment: prepared.profile.input.environment,
-            dialect,
-        };
+        let sqls: Vec<String> = preview
+            .statements
+            .iter()
+            .filter_map(|s| match &s.step {
+                RebuildStep::Sql { sql } => Some(sql.clone()),
+                RebuildStep::CountCheck { .. } => None,
+            })
+            .collect();
+        (preview.denied, preview.confirm) = self.check_policy(&prepared.profile, dialect, &sqls).await;
+        prepared.preview = preview;
+        prepared.plan = plan;
+        Ok(prepared)
+    }
+
+    /// 执行策略：被拒绝的语句不能执行（返回第一个原因）；需要确认的列出原因（生产环境总要确认）。
+    pub(crate) async fn check_policy(
+        &self,
+        profile: &Profile,
+        dialect: Dialect,
+        sqls: &[String],
+    ) -> (Option<Msg>, Vec<Msg>) {
+        let ctx =
+            ExecContext { profile_id: profile.id.clone(), environment: profile.input.environment, dialect };
         let policy = self.sessions.policy();
-        for s in &preview.statements {
-            let RebuildStep::Sql { sql } = &s.step else { continue };
+        let (mut denied, mut confirm) = (None, Vec::new());
+        for sql in sqls {
             match policy.before_execute(&ctx, &classify(&dialect, sql)).await {
                 Decision::Allow => {}
                 Decision::Confirm { reason } => {
-                    if !preview.confirm.contains(&reason) {
-                        preview.confirm.push(reason);
+                    if !confirm.contains(&reason) {
+                        confirm.push(reason);
                     }
                 }
                 Decision::Deny { reason } => {
-                    preview.denied.get_or_insert(reason);
+                    denied.get_or_insert(reason);
                 }
             }
         }
-        prepared.preview = preview;
-        prepared.plan = plan;
-        Ok(prepared)
+        (denied, confirm)
     }
 
     /// 预览：校验错误、变更列表、生成的语句或重建脚本、提示、执行策略的结论。不执行。
@@ -306,10 +359,8 @@ impl Service {
         let password = self.secrets.get(&prepared.profile.id)?.unwrap_or_default();
         let task = self.tasks.create(TaskKind::Design)?;
         let mut id = String::new();
-        let total = match &plan {
-            Plan::Alter(s) => s.len(),
-            Plan::Rebuild(r) => r.steps.len(),
-        } as u64;
+        let parts = plan.parts();
+        let total = Part::len(&parts);
         task.update(|s| {
             id = s.id.clone();
             s.phase = TaskPhase::Executing;
@@ -321,7 +372,7 @@ impl Service {
         schemas.push(schema.clone());
         tokio::spawn(async move {
             let run = Run { service: &service, task: &task, profile: &profile, dialect, schema: &schema };
-            let result = run.execute(plan, password).await;
+            let result = run.execute(parts, password).await;
             for s in &schemas {
                 service.metadata.cache().invalidate(&profile.id, Some(s));
             }
@@ -331,13 +382,13 @@ impl Service {
     }
 }
 
-/// 一次执行。
-struct Run<'a> {
-    service: &'a Service,
-    task: &'a TaskHandle,
-    profile: &'a Profile,
-    dialect: Dialect,
-    schema: &'a str,
+/// 一次执行：表设计器的改动、比对后的同步脚本。
+pub(crate) struct Run<'a> {
+    pub service: &'a Service,
+    pub task: &'a TaskHandle,
+    pub profile: &'a Profile,
+    pub dialect: Dialect,
+    pub schema: &'a str,
 }
 
 impl Run<'_> {
@@ -384,7 +435,8 @@ impl Run<'_> {
         self.task.update(|s| s.remaining = (!rest.is_empty()).then(|| rest + ";"));
     }
 
-    async fn execute(&self, plan: Plan, password: String) -> Result<()> {
+    /// 在新连接上依次执行各段。PG 家族在一个事务中执行；其他库失败时列出没有执行的语句。
+    pub(crate) async fn execute(&self, parts: Vec<Part>, password: String) -> Result<()> {
         let (mut conn, _) = self.service.registry.connect(self.profile, password).await?;
         self.task.set_cancel(Some(conn.cancel_handle()))?;
         let transactional = self.dialect.design_transactional();
@@ -393,23 +445,26 @@ impl Run<'_> {
             if transactional {
                 conn.set_autocommit(false).await?;
             }
-            match plan {
-                Plan::Alter(statements) => {
-                    let texts: Vec<String> = statements.iter().map(|s| s.sql.clone()).collect();
-                    for (i, sql) in texts.iter().enumerate() {
+            let texts: Vec<Vec<String>> = parts.iter().map(Part::texts).collect();
+            let mut offset = 0;
+            for (pi, part) in parts.iter().enumerate() {
+                // PG 回滚全部，没有执行的语句不必再列出
+                let later: Vec<String> = if transactional { vec![] } else { texts[pi + 1..].concat() };
+                match part {
+                    Part::Sql(sql) => {
                         if self.task.cancelled() {
                             return Err(Error::Cancelled);
                         }
                         if let Err(e) = self.sql(&mut conn, sql).await {
                             let e = if self.task.cancelled() { Error::Cancelled } else { e };
-                            // PG 回滚全部，没有执行的语句不必再列出
-                            self.failed(i, sql, &e, if transactional { &[] } else { &texts[i + 1..] });
+                            self.failed(offset, sql, &e, &later);
                             return Err(e);
                         }
                         self.task.update(|s| s.progress.statements_done += 1);
                     }
+                    Part::Rebuild(r) => self.rebuild(&mut conn, r, transactional, offset, &later).await?,
                 }
-                Plan::Rebuild(r) => self.rebuild(&mut conn, &r, transactional).await?,
+                offset += texts[pi].len();
             }
             if transactional {
                 conn.commit().await?;
@@ -424,17 +479,16 @@ impl Run<'_> {
         outcome
     }
 
-    async fn rebuild(&self, conn: &mut Box<dyn Connection>, r: &Rebuild, transactional: bool) -> Result<()> {
-        let texts: Vec<String> = r
-            .steps
-            .iter()
-            .map(|s| match s {
-                RebuildStep::Sql { sql } => sql.clone(),
-                RebuildStep::CountCheck { source, target } => {
-                    format!("SELECT COUNT(*) FROM {source}; SELECT COUNT(*) FROM {target}")
-                }
-            })
-            .collect();
+    /// 一张表的重建。`offset`：第一步在整个脚本中的序号；`later`：后面各段的语句（失败时列为没有执行）。
+    async fn rebuild(
+        &self,
+        conn: &mut Box<dyn Connection>,
+        r: &Rebuild,
+        transactional: bool,
+        offset: usize,
+        later: &[String],
+    ) -> Result<()> {
+        let texts = Part::Rebuild(r.clone()).texts();
         for (i, step) in r.steps.iter().enumerate() {
             if i == r.drop_at {
                 // 从删除旧表起不能取消
@@ -461,8 +515,13 @@ impl Run<'_> {
             };
             if let Err(e) = result {
                 let e = if before_drop && self.task.cancelled() { Error::Cancelled } else { e };
-                let rest = if transactional || before_drop { &[][..] } else { &texts[i + 1..] };
-                self.failed(i, &texts[i], &e, rest);
+                // 删除旧表之前失败时这张表的改动已撤销，只列出后面各段
+                let rest = match (transactional, before_drop) {
+                    (true, _) => vec![],
+                    (false, true) => later.to_vec(),
+                    (false, false) => [&texts[i + 1..], later].concat(),
+                };
+                self.failed(offset + i, &texts[i], &e, &rest);
                 if before_drop {
                     self.cleanup(conn, r, transactional).await;
                 }
@@ -631,7 +690,7 @@ mod tests {
                     dialect: Dialect::new(Variant::MySql),
                     schema: "shop",
                 };
-                let r = run.execute(Plan::Rebuild(plan), "pw".into()).await;
+                let r = run.execute(vec![Part::Rebuild(plan)], "pw".into()).await;
                 task.finish(r);
                 s.tasks.status(&id).unwrap()
             }
```

- [ ] **Step 4: 修改 `crates/dblens-service/src/lib.rs`**

```diff
--- a/crates/dblens-service/src/lib.rs
+++ b/crates/dblens-service/src/lib.rs
@@ -1,6 +1,7 @@
 //! 服务层：连接配置、密码、驱动注册表、会话、元数据缓存、执行策略。供 `dblens-api` 使用，自身不处理 HTTP。
 
 mod changes;
+mod compare;
 mod design;
 mod export;
 /// 测试用的假驱动，供本 crate 和 `dblens-api` 的测试使用（feature `testing`）。
@@ -23,6 +24,9 @@ mod store;
 mod task;
 
 pub use changes::ChangeSet;
+pub use compare::{
+    CompareRequest, CompareResult, CompareSideRef, ModelWriteInfo, SyncPreview, SyncRequest, SyncStarted,
+};
 pub use design::{DesignPreview, DesignRequest, DesignTable, PreviewStatement, RebuildInfo};
 pub use export::{
     EXPORT_LOB_LIMIT, ExportFormat, ExportManager, ExportOptions, ExportRequest, ExportSource, ExportState,
```

- [ ] **Step 5: 修改 `crates/dblens-service/src/task.rs`**

```diff
--- a/crates/dblens-service/src/task.rs
+++ b/crates/dblens-service/src/task.rs
@@ -28,6 +28,8 @@ pub enum TaskKind {
     Design,
     /// 从库反向生成模型（M4a）。
     Reverse,
+    /// 比对后同步到库（M4a）。
+    Sync,
 }
 
 #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
```

- [ ] **Step 6: 检查**

```bash
export CARGO_INCREMENTAL=0
cargo fmt --all
cargo clippy --config "source.mirror.registry='sparse+https://rsproxy.cn/index/'" --workspace --all-targets -- -D warnings
DBLENS_LANG=en cargo test --config "source.mirror.registry='sparse+https://rsproxy.cn/index/'" --workspace
```

Expected: 全部测试通过（service 新增 3 个；M3 设计器的测试不变）。

- [ ] **Step 7: 提交**

```bash
git add crates/dblens-core/locales/app.json crates/dblens-service/src/compare.rs crates/dblens-service/src/design.rs crates/dblens-service/src/lib.rs crates/dblens-service/src/task.rs
git commit -m "feat(service): 结构比对与同步（执行器按段执行、写回模型）" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 6: 接口：模型与比对；只监听本机地址的浏览器模式可用本机路径

- `dblens-api/src/models.rs`：`/models`（列表与最近、新建）、`/models/open`、`/models/reverse`、`/models/{id}`（读、改、关闭）、`/models/{id}/caps`、`/models/{id}/tables`（列表、保存）、`/models/{id}/tables/preview`、`/models/{id}/tables/{schema}/{name}`（读、删除）、`.../rename`；`/compare`、`/compare/preview`、`/compare/sync`、`/compare/script`（把同步脚本写到 `.sql` 文件）。`/models` 下和 `/compare/script` 要求 `local_files`，否则 403。
- `ApiConfig.local_files` 的说明改为"页面和服务在同一台机器上"；`dblens-server` 只监听本机地址时为真。
- 后端词条加 `model.local_only`、`model.script_ext`。

**Files:**
- Modify: `crates/dblens-api/src/lib.rs`
- Create: `crates/dblens-api/src/models.rs`
- Create: `crates/dblens-api/tests/models.rs`
- Modify: `crates/dblens-core/locales/app.json`
- Modify: `crates/dblens-server/src/main.rs`

**Interfaces:**
- Consumes：任务 3–5 的服务层接口。

- [ ] **Step 1: 新建 `crates/dblens-api/tests/models.rs`**

```rust
//! 模型与比对接口（M4a）。

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use dblens_api::{ApiConfig, router};
use dblens_service::{DefaultPolicy, DriverRegistry, HistoryLog, SecretBackend, Secrets, Service, Store};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

fn app(local_files: bool) -> Router {
    let store = Arc::new(Store::open_in_memory().unwrap());
    let secrets = Arc::new(Secrets::with_backend(store.clone(), SecretBackend::Vault));
    let registry = Arc::new(DriverRegistry::with_drivers(store.clone(), vec![]));
    let history = HistoryLog::inline(store.clone());
    let service = Arc::new(Service::with_history(store, secrets, registry, Arc::new(DefaultPolicy), history));
    router(service, ApiConfig { token: "t".into(), allowed_origins: vec![], local_files })
}

async fn call(app: &Router, method: &str, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
    let mut req = Request::builder().method(method).uri(uri).header(header::AUTHORIZATION, "Bearer t");
    if body.is_some() {
        req = req.header(header::CONTENT_TYPE, "application/json");
    }
    let req = req.body(body.map_or_else(Body::empty, |b| Body::from(b.to_string()))).unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    (status, if bytes.is_empty() { Value::Null } else { serde_json::from_slice(&bytes).unwrap() })
}

fn column(name: &str) -> Value {
    json!({ "name": name, "ordinal": 0, "data_type": { "native": "integer", "logical": { "kind": "other" } },
            "nullable": false, "default": null, "comment": null, "auto_increment": false, "generated": null })
}

fn draft(name: &str, cols: &[&str]) -> Value {
    json!({ "name": name, "comment": null, "columns": cols.iter().map(|c| column(c)).collect::<Vec<_>>(),
            "primary_key": { "name": null, "columns": [cols[0]] }, "uniques": [], "foreign_keys": [],
            "checks": [], "indexes": [], "triggers": [], "partition": null })
}

async fn new_model(app: &Router, dir: &std::path::Path, name: &str) -> String {
    let body = json!({ "path": dir.join(name), "name": name, "database": { "variant": "postgres" } });
    let (st, m) = call(app, "POST", "/api/models", Some(body)).await;
    assert_eq!(st, StatusCode::CREATED, "{m}");
    m["id"].as_str().unwrap().to_string()
}

#[tokio::test]
async fn models_need_local_files() {
    let app = app(false);
    let (st, v) = call(&app, "GET", "/api/models", None).await;
    assert_eq!(st, StatusCode::FORBIDDEN, "{v}");
    let (st, _) = call(&app, "POST", "/api/models/open", Some(json!({ "path": "/tmp/x" }))).await;
    assert_eq!(st, StatusCode::FORBIDDEN);
    let (st, _) =
        call(&app, "POST", "/api/compare/script", Some(json!({ "path": "/tmp/x.sql", "text": "" }))).await;
    assert_eq!(st, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn save_script_only_writes_sql_files() {
    let app = app(true);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sync.SQL");
    let (st, _) =
        call(&app, "POST", "/api/compare/script", Some(json!({ "path": path, "text": "SELECT 1;" }))).await;
    assert_eq!(st, StatusCode::NO_CONTENT);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "SELECT 1;");
    let other = dir.path().join("x.txt");
    let (st, _) =
        call(&app, "POST", "/api/compare/script", Some(json!({ "path": other, "text": "x" }))).await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
    assert!(!other.exists());
}

#[tokio::test]
async fn edit_compare_and_write_back() {
    let app = app(true);
    let dir = tempfile::tempdir().unwrap();
    let a = new_model(&app, dir.path(), "a").await;
    let b = new_model(&app, dir.path(), "b").await;
    let (_, list) = call(&app, "GET", "/api/models", None).await;
    assert_eq!(list["open"].as_array().unwrap().len(), 2);
    assert_eq!(list["recent"].as_array().unwrap().len(), 2);

    // 在 a 中新建表、改名；b 中有一张旧表
    let save = |name: Option<&str>, d: Value, fp: Option<&str>| json!({ "schema": "public", "name": name, "draft": d, "fingerprint": fp });
    let (st, v) = call(
        &app,
        "POST",
        &format!("/api/models/{a}/tables"),
        Some(save(None, draft("t", &["id", "x"]), None)),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let (st, _) =
        call(&app, "POST", &format!("/api/models/{b}/tables"), Some(save(None, draft("old", &["id"]), None)))
            .await;
    assert_eq!(st, StatusCode::OK);
    let (st, _) = call(
        &app,
        "POST",
        &format!("/api/models/{a}/tables/public/t/rename"),
        Some(json!({ "to": "items" })),
    )
    .await;
    assert_eq!(st, StatusCode::NO_CONTENT);
    let (_, tables) = call(&app, "GET", &format!("/api/models/{a}/tables"), None).await;
    assert_eq!(tables[0]["name"], "items");
    let (st, t) = call(&app, "GET", &format!("/api/models/{a}/tables/public/items"), None).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(t["table"]["columns"][1]["name"], "x");
    // 旧指纹保存：预览标出冲突
    let req = save(Some("items"), draft("items", &["id", "x", "y"]), Some("stale"));
    let (_, p) = call(&app, "POST", &format!("/api/models/{a}/tables/preview"), Some(req)).await;
    assert_eq!(p["conflict"], true);

    // a → b：新增 items、删除 old
    let side = |m: &str| json!({ "kind": "model", "model": m, "schema": "public" });
    let cmp = json!({ "source": side(&a), "target": side(&b) });
    let (st, r) = call(&app, "POST", "/api/compare", Some(cmp.clone())).await;
    assert_eq!(st, StatusCode::OK, "{r}");
    let ids: Vec<u64> = r["diff"]["tables"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|t| t["items"].as_array().unwrap().iter().map(|i| i["id"].as_u64().unwrap()))
        .collect();
    assert_eq!(ids.len(), 2);
    let sync = json!({ "compare": cmp, "selected": ids, "fingerprint": r["fingerprint"] });
    let (_, p) = call(&app, "POST", "/api/compare/preview", Some(sync.clone())).await;
    assert_eq!(p["writes"].as_array().unwrap().len(), 2, "{p}");
    let (st, done) = call(&app, "POST", "/api/compare/sync", Some(sync)).await;
    assert_eq!((st, &done["task"]), (StatusCode::OK, &Value::Null), "{done}");
    let (_, tables) = call(&app, "GET", &format!("/api/models/{b}/tables"), None).await;
    let names: Vec<&str> = tables.as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["items"]);

    let (st, _) = call(&app, "DELETE", &format!("/api/models/{b}"), None).await;
    assert_eq!(st, StatusCode::NO_CONTENT);
    let (st, _) = call(&app, "GET", &format!("/api/models/{b}"), None).await;
    assert_eq!(st, StatusCode::NOT_FOUND);
}
```

- [ ] **Step 2: 修改 `crates/dblens-api/src/lib.rs`**

```diff
--- a/crates/dblens-api/src/lib.rs
+++ b/crates/dblens-api/src/lib.rs
@@ -6,6 +6,7 @@ mod design;
 mod error;
 mod exports;
 mod lang;
+mod models;
 mod routes;
 mod tasks;
 mod web;
@@ -31,8 +32,8 @@ pub struct ApiConfig {
     pub token: String,
     /// 除同源外还允许的 Origin，如桌面模式的 `tauri://localhost`、前端开发服务器 `http://localhost:5173`。
     pub allowed_origins: Vec<String>,
-    /// 页面和服务在同一台机器上（桌面程序）：导出可以直接写到页面给出的路径。
-    /// 浏览器模式（`dblens-server`）页面可能在另一台机器上，为 false，导出一律写临时文件后下载。
+    /// 页面和服务在同一台机器上（桌面程序、只监听本机地址的浏览器模式）：导出可以直接写到页面给出的路径，
+    /// 可以打开模型目录。监听其他地址的浏览器模式页面可能在另一台机器上，为 false，导出一律写临时文件后下载。
     pub local_files: bool,
 }
 
@@ -90,6 +91,19 @@ pub fn router(service: Arc<Service>, config: ApiConfig) -> Router {
         .route("/profiles/{id}/design/preview", post(design::preview))
         .route("/profiles/{id}/design/apply", post(design::apply))
         .route("/sessions/{id}/changes", post(routes::apply_changes))
+        .route("/models", get(models::list).post(models::create))
+        .route("/models/open", post(models::open))
+        .route("/models/reverse", post(models::reverse))
+        .route("/models/{id}", get(models::get).put(models::update).delete(models::close))
+        .route("/models/{id}/caps", get(models::caps))
+        .route("/models/{id}/tables", get(models::tables).post(models::save_table))
+        .route("/models/{id}/tables/preview", post(models::preview_table))
+        .route("/models/{id}/tables/{schema}/{name}", get(models::table).delete(models::delete_table))
+        .route("/models/{id}/tables/{schema}/{name}/rename", post(models::rename_table))
+        .route("/compare", post(models::compare))
+        .route("/compare/preview", post(models::sync_preview))
+        .route("/compare/sync", post(models::sync))
+        .route("/compare/script", post(models::save_script))
         .route("/exports", post(exports::create))
         .route("/exports/{id}", get(exports::status).delete(exports::cancel_or_remove))
         .route("/exports/{id}/file", get(exports::file))
```

- [ ] **Step 3: 新建 `crates/dblens-api/src/models.rs`**

```rust
//! 模型与比对的接口（M4a 设计文档 4.4 节）。模型目录是服务所在机器上的路径：
//! 只有页面和服务在同一台机器上（`ApiConfig::local_files`）时才能使用模型，否则返回禁止。

use std::path::PathBuf;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use dblens_core::{Error, msg};
use dblens_dialect::design::DesignCaps;
use dblens_service::{
    CompareRequest, CompareResult, ModelDesignPreview, ModelDesignRequest, ModelSummary, ModelTableView,
    NewModel, ReverseRequest, SyncPreview, SyncRequest, SyncStarted, TableEntry,
};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::AppState;
use crate::error::ApiResult;

fn local_only(s: &AppState) -> ApiResult<()> {
    if !s.config.local_files {
        return Err(Error::Denied { reason: msg!("model.local_only") }.into());
    }
    Ok(())
}

/// 打开的模型和最近打开的模型目录。
pub async fn list(State(s): State<AppState>) -> ApiResult<Json<Value>> {
    local_only(&s)?;
    Ok(Json(json!({ "open": s.service.models.list(), "recent": s.service.recent_models()? })))
}

#[derive(Deserialize)]
pub struct OpenBody {
    path: PathBuf,
}

pub async fn open(State(s): State<AppState>, Json(b): Json<OpenBody>) -> ApiResult<Json<ModelSummary>> {
    local_only(&s)?;
    Ok(Json(s.service.open_model(&b.path)?))
}

pub async fn create(
    State(s): State<AppState>,
    Json(req): Json<NewModel>,
) -> ApiResult<(StatusCode, Json<ModelSummary>)> {
    local_only(&s)?;
    Ok((StatusCode::CREATED, Json(s.service.create_model(req)?)))
}

/// 反向生成：返回任务 id，结束后任务状态的 `model` 是打开的模型。
pub async fn reverse(
    State(s): State<AppState>,
    Json(req): Json<ReverseRequest>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    local_only(&s)?;
    let id = s.service.start_reverse(req).await?;
    Ok((StatusCode::CREATED, Json(json!({ "id": id }))))
}

/// 重新加载并返回模型信息。
pub async fn get(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult<Json<ModelSummary>> {
    local_only(&s)?;
    Ok(Json(s.service.models.summary_of(&id)?))
}

#[derive(Deserialize)]
pub struct InfoBody {
    name: String,
    #[serde(default)]
    description: Option<String>,
    schemas: Vec<String>,
}

pub async fn update(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Json(b): Json<InfoBody>,
) -> ApiResult<Json<ModelSummary>> {
    local_only(&s)?;
    Ok(Json(s.service.models.set_info(&id, &b.name, b.description, b.schemas)?))
}

pub async fn close(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult<StatusCode> {
    local_only(&s)?;
    s.service.models.close(&id)?;
    Ok(StatusCode::NO_CONTENT)
}

/// 设计器的能力和类型目录：按模型的数据库类型。
pub async fn caps(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult<Json<DesignCaps>> {
    local_only(&s)?;
    Ok(Json(s.service.models.dialect(&id)?.design_caps()))
}

pub async fn tables(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult<Json<Vec<TableEntry>>> {
    local_only(&s)?;
    Ok(Json(s.service.models.tables(&id)?))
}

pub async fn table(
    State(s): State<AppState>,
    Path((id, schema, name)): Path<(String, String, String)>,
) -> ApiResult<Json<ModelTableView>> {
    local_only(&s)?;
    Ok(Json(s.service.models.read_table(&id, &schema, &name)?))
}

pub async fn preview_table(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<ModelDesignRequest>,
) -> ApiResult<Json<ModelDesignPreview>> {
    local_only(&s)?;
    Ok(Json(s.service.preview_model_design(&id, &req).await?))
}

/// 保存设计器中的表（新建或修改），返回新的指纹。
pub async fn save_table(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<ModelDesignRequest>,
) -> ApiResult<Json<Value>> {
    local_only(&s)?;
    let fingerprint = s.service.save_model_design(&id, req).await?;
    Ok(Json(json!({ "fingerprint": fingerprint })))
}

#[derive(Deserialize)]
pub struct DeleteQuery {
    #[serde(default)]
    fingerprint: Option<String>,
}

pub async fn delete_table(
    State(s): State<AppState>,
    Path((id, schema, name)): Path<(String, String, String)>,
    Query(q): Query<DeleteQuery>,
) -> ApiResult<StatusCode> {
    local_only(&s)?;
    s.service.models.delete_table(&id, &schema, &name, q.fingerprint.as_deref())?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
pub struct RenameBody {
    to: String,
}

pub async fn rename_table(
    State(s): State<AppState>,
    Path((id, schema, name)): Path<(String, String, String)>,
    Json(b): Json<RenameBody>,
) -> ApiResult<StatusCode> {
    local_only(&s)?;
    s.service.models.rename_table(&id, &schema, &name, &b.to)?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn compare(
    State(s): State<AppState>,
    Json(req): Json<CompareRequest>,
) -> ApiResult<Json<CompareResult>> {
    Ok(Json(s.service.compare(&req).await?))
}

/// 同步的预览：目标是库时为脚本，目标是模型时为要写的文件。
pub async fn sync_preview(
    State(s): State<AppState>,
    Json(req): Json<SyncRequest>,
) -> ApiResult<Json<SyncPreview>> {
    Ok(Json(s.service.preview_sync(&req).await?))
}

#[derive(Deserialize)]
pub struct ScriptBody {
    path: PathBuf,
    text: String,
}

/// 桌面程序：把同步脚本写到"另存为"选的路径（只接受 `.sql` 文件）。
pub async fn save_script(State(s): State<AppState>, Json(b): Json<ScriptBody>) -> ApiResult<StatusCode> {
    local_only(&s)?;
    if !b.path.extension().is_some_and(|e| e.eq_ignore_ascii_case("sql")) {
        return Err(Error::invalid(msg!("model.script_ext")).into());
    }
    std::fs::write(&b.path, b.text)
        .map_err(|e| Error::internal(msg!("model.io_failed", path = b.path.display(), error = e)))?;
    Ok(StatusCode::NO_CONTENT)
}

/// 同步：目标是库时返回任务 id（201），目标是模型时直接写回（200，`task` 为 null）。
pub async fn sync(
    State(s): State<AppState>,
    Json(req): Json<SyncRequest>,
) -> ApiResult<(StatusCode, Json<SyncStarted>)> {
    let started = s.service.start_sync(req).await?;
    let status = if started.task.is_some() { StatusCode::CREATED } else { StatusCode::OK };
    Ok((status, Json(started)))
}
```

- [ ] **Step 4: 修改 `crates/dblens-core/locales/app.json`**

```diff
--- a/crates/dblens-core/locales/app.json
+++ b/crates/dblens-core/locales/app.json
@@ -148,9 +148,11 @@
   "model.conflict": {"zh-CN": "%{name} 的文件已被修改，请重新加载", "en": "The file for %{name} has changed; reload it"},
   "model.dir_not_empty": {"zh-CN": "目录 %{path} 不为空", "en": "Directory %{path} is not empty"},
   "model.io_failed": {"zh-CN": "读写 %{path} 失败：%{error}", "en": "Failed to access %{path}: %{error}"},
+  "model.local_only": {"zh-CN": "只有桌面程序和只监听本机地址的浏览器模式可以使用模型", "en": "Models are available only in the desktop app and in browser mode listening on a local address"},
   "model.no_schemas": {"zh-CN": "没有选择 schema", "en": "No schema selected"},
   "model.not_a_model": {"zh-CN": "%{path} 不是模型目录（没有 model.yaml）", "en": "%{path} is not a model directory (model.yaml not found)"},
   "model.read_only": {"zh-CN": "模型文件格式 %{format} 比本程序新，只能查看；请升级数洞", "en": "Model format %{format} is newer than this version supports; it is read-only. Please upgrade DBLens"},
+  "model.script_ext": {"zh-CN": "脚本只能保存为 .sql 文件", "en": "Scripts can only be saved as .sql files"},
   "model.table_exists": {"zh-CN": "表 %{name} 已存在", "en": "Table %{name} already exists"},
   "model.yaml_write_failed": {"zh-CN": "生成 YAML 失败：%{error}", "en": "Failed to write YAML: %{error}"},
   "native.dameng_dpi": {"zh-CN": "达梦 DPI 库", "en": "DM DPI library"},
```

- [ ] **Step 5: 修改 `crates/dblens-server/src/main.rs`**

```diff
--- a/crates/dblens-server/src/main.rs
+++ b/crates/dblens-server/src/main.rs
@@ -132,8 +132,9 @@ async fn run(args: Args) -> Result<(), String> {
             }
         });
     }
-    let mut app =
-        router(service, ApiConfig { token, allowed_origins: args.allowed_origins, local_files: false });
+    // 只监听本机地址时页面和服务在同一台机器上，可以使用本机路径（打开模型、导出到指定路径）
+    let local_files = addr.ip().is_loopback();
+    let mut app = router(service, ApiConfig { token, allowed_origins: args.allowed_origins, local_files });
     if let Some(dir) = web_dir {
         app = with_web(app, dir);
     }
```

- [ ] **Step 6: 检查**

```bash
export CARGO_INCREMENTAL=0
cargo fmt --all
cargo clippy --config "source.mirror.registry='sparse+https://rsproxy.cn/index/'" --workspace --all-targets -- -D warnings
DBLENS_LANG=en cargo test --config "source.mirror.registry='sparse+https://rsproxy.cn/index/'" --workspace
```

Expected: 全部测试通过（api 新增 `tests/models.rs` 3 个）。

- [ ] **Step 7: 提交**

```bash
git add crates/dblens-api/src/lib.rs crates/dblens-api/src/models.rs crates/dblens-api/tests/models.rs crates/dblens-core/locales/app.json crates/dblens-server/src/main.rs
git commit -m "feat(api): 模型与比对接口；只监听本机地址的浏览器模式可用本机路径" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 7: 实库测试；比对时外键的默认动作视为相同

- `dblens-service/tests/live.rs` 的 `model_flow`（原有 5 个库和国产库第一批的 6 个目标）：建 `m4_cust`、`m4_ord` → 反向生成 → 在模型中列改名（`note` → `remark`）、加列、换索引、加外键、表改名（`m4_cust` → `m4_customer`，外键引用跟着改）→ 模型对库比对（按记下的改名配对）→ 同步到库 → 再比对无差异、改名记录已删去 → 库中加列 → 库对模型比对并写回 → 两边一致 → 同一个库中第二个 schema（PG 家族 `CREATE SCHEMA m4_b`，MySQL 家族 `CREATE DATABASE dblens_test_m4b`；Oracle、达梦跳过）的库对库比对与同步。同一 schema 中其他测试的表会并发变化：只看 `m4_` 开头的表，库对库同步不传指纹。
- 实测发现 MariaDB、OceanBase 读回的外键动作是 `RESTRICT`：`normalize.rs` 加 `default_actions`，比对单元测试加一处断言。

**Files:**
- Modify: `crates/dblens-dialect/src/compare/normalize.rs`
- Modify: `crates/dblens-dialect/src/compare/tests.rs`
- Modify: `crates/dblens-service/tests/live.rs`

**Interfaces:**
- Consumes：任务 2–5。

- [ ] **Step 1: 修改 `crates/dblens-dialect/src/compare/tests.rs`**

```diff
--- a/crates/dblens-dialect/src/compare/tests.rs
+++ b/crates/dblens-dialect/src/compare/tests.rs
@@ -101,6 +101,7 @@ fn options_and_equivalent_spellings() {
     target.primary_key.as_mut().unwrap().name = Some("orders_pkey".into());
     target.uniques[0].name = Some("orders_code_key".into());
     target.foreign_keys[0].ref_schema = Some("live".into());
+    target.foreign_keys[0].on_update = Some("NO ACTION".into());
     target.checks[0].expression = "(amount >= (0)::numeric)".into();
     let mut source = base.clone();
     source.columns[1].default = Some("'x'".into());
```

- [ ] **Step 2: 修改 `crates/dblens-service/tests/live.rs`**

```diff
--- a/crates/dblens-service/tests/live.rs
+++ b/crates/dblens-service/tests/live.rs
@@ -6,22 +6,27 @@
 //! 走一遍完整流程：保存连接配置（密码放本地密码库）→ 测试连接 → 打开会话执行 → 浏览元数据 →
 //! 会话中执行 DDL 后元数据缓存失效 → 取消长查询；查询工作台用到的执行计划、出错位置、切换 schema。
 
+use std::collections::BTreeSet;
 use std::path::Path;
 use std::sync::Arc;
 use std::time::Duration;
 
-use dblens_core::schema::{CheckConstraint, Column, Index, IndexColumn, PrimaryKey, Table, UniqueConstraint};
+use dblens_core::schema::{
+    CheckConstraint, Column, ForeignKey, Index, IndexColumn, PrimaryKey, Table, UniqueConstraint,
+};
 use dblens_core::{ColumnType, Error, LogicalType, Value};
+use dblens_dialect::compare::{CompareOptions, CompareRenames, SchemaChange, TableStatus};
 use dblens_dialect::design::{Renames, TableChange, diff_table};
 use dblens_dialect::meta::{MetaArgs, MetaKey};
 use dblens_dialect::{Compat, DdlKind, Direction, RowChange, Variant, WriteMode};
 use dblens_driver::{ConnectParams, DriverId};
 use dblens_service::{
-    ChangeSet, ColumnMapping, DEFAULT_BATCH, DataOrder, DefaultPolicy, DesignRequest, DriverRegistry,
-    Environment, ExecRequest, ExportFormat, ExportOptions, ExportRequest, ExportSource, ExportState,
-    ExportStatus, FileOptions, FileSource, GenerateKind, ImportRequest, NewColumn, NewTable, ObjectKind,
-    OnError, ProfileInput, SecretBackend, Secrets, Service, SessionEvent, SessionId, SqlFileRequest, Store,
-    TaskState, TaskStatus, Transaction,
+    ChangeSet, ColumnMapping, CompareRequest, CompareResult, CompareSideRef, DEFAULT_BATCH, DataOrder,
+    DefaultPolicy, DesignRequest, DriverRegistry, Environment, ExecRequest, ExportFormat, ExportOptions,
+    ExportRequest, ExportSource, ExportState, ExportStatus, FileOptions, FileSource, GenerateKind,
+    ImportRequest, ModelDesignRequest, NewColumn, NewTable, ObjectKind, OnError, ProfileInput,
+    ReverseRequest, SecretBackend, Secrets, Service, SessionEvent, SessionId, SqlFileRequest, Store,
+    SyncRequest, TaskState, TaskStatus, Transaction,
 };
 
 struct Live {
@@ -1437,6 +1442,241 @@ async fn dameng_design() {
 }
 
 /// MySQL 驱动的取消测试用的耗时查询（SLEEP() 被 KILL QUERY 打断时返回 1 而不报错）。
+/// 只看 `m4_` 开头的表（同一 schema 中其他测试的表会并发变化）。
+fn m4_items(r: &CompareResult) -> BTreeSet<u32> {
+    let mine = |n: &Option<String>| n.as_deref().is_some_and(|n| n.to_lowercase().starts_with("m4_"));
+    r.diff
+        .tables
+        .iter()
+        .filter(|t| mine(&t.source) || mine(&t.target))
+        .flat_map(|t| t.items.iter().map(|i| i.id))
+        .collect()
+}
+
+/// `m4_` 表的差异明细（断言失败时输出）。
+fn m4_detail(r: &CompareResult) -> String {
+    let mine = |n: &Option<String>| n.as_deref().is_some_and(|n| n.to_lowercase().starts_with("m4_"));
+    let tables: Vec<_> = r.diff.tables.iter().filter(|t| mine(&t.source) || mine(&t.target)).collect();
+    format!("{tables:#?}")
+}
+
+fn m4_status(r: &CompareResult) -> Vec<(String, TableStatus)> {
+    r.diff
+        .tables
+        .iter()
+        .filter_map(|t| {
+            let name = t.source.clone().or_else(|| t.target.clone())?;
+            name.to_lowercase().starts_with("m4_").then_some((name.to_lowercase(), t.status))
+        })
+        .collect()
+}
+
+/// 模型（M4a）：反向生成 → 在模型中改表（表改名、列改名、加列、换索引、加外键）→ 与库比对 → 同步到库 →
+/// 再比对没有差异；库中加列后写回模型；同一个库中两个 schema 的库对库比对（Oracle、达梦没有第二个 schema，跳过）。
+async fn model_flow(env: &str) {
+    let Some(live) = setup(env).await else { return };
+    let live = &live;
+    let s = &live.service;
+    let id = s.open_session(&live.profile_id).await.unwrap().id;
+    let upper = matches!(live.driver, DriverId::Oracle | DriverId::Dameng);
+    let n = |name: &str| if upper { name.to_uppercase() } else { name.to_string() };
+    let varchar = if live.driver == DriverId::Oracle { "VARCHAR2" } else { "VARCHAR" };
+    let drop_sql = &|schema: Option<&str>, table: &str| {
+        let q = schema.map_or(table.to_string(), |s| format!("{s}.{table}"));
+        match live.driver {
+            DriverId::Oracle => {
+                format!(
+                    "BEGIN EXECUTE IMMEDIATE 'DROP TABLE {q} CASCADE CONSTRAINTS'; EXCEPTION WHEN OTHERS THEN NULL; END;"
+                )
+            }
+            DriverId::Dameng => format!("DROP TABLE IF EXISTS {q} CASCADE"),
+            _ => format!("DROP TABLE IF EXISTS {q}"),
+        }
+    };
+    let cleanup = |schema: Option<&'static str>| async move {
+        for t in ["m4_ord", "m4_cust", "m4_customer"] {
+            live.submit(id, ExecRequest::new(drop_sql(schema, t))).await;
+        }
+    };
+    cleanup(None).await;
+    live.run(id, &format!("CREATE TABLE m4_cust (id INT PRIMARY KEY, name {varchar}(20))")).await;
+    live.run(id, &format!("CREATE TABLE m4_ord (id INT PRIMARY KEY, cust_id INT, note {varchar}(20))")).await;
+    live.run(id, "CREATE INDEX ix_m4_note ON m4_ord (note)").await;
+
+    // 1. 反向生成
+    let dir = tempfile::tempdir().unwrap();
+    let req = ReverseRequest {
+        profile_id: live.profile_id.clone(),
+        schemas: vec![live.schema.clone()],
+        path: dir.path().join("model").to_string_lossy().into(),
+        name: "m4".into(),
+    };
+    let st = wait_task(s, &s.start_reverse(req).await.unwrap()).await;
+    assert_eq!(st.state, TaskState::Done, "{env}：{st:?}");
+    let m = st.model.unwrap();
+    let info = s.models.summary_of(&m).unwrap().info;
+    assert_eq!((info.database.variant, info.database.compat), (live.variant, live.compat), "{env}");
+    let schema = live.schema.clone();
+    let ord = s.models.read_table(&m, &schema, &n("m4_ord")).unwrap();
+    assert_eq!(ord.table.columns.len(), 3, "{env}：{:?}", ord.table);
+
+    // 2. 在模型中改表：note 改名为 remark，加列 extra，换索引，加外键；m4_cust 改名为 m4_customer
+    let mut d = ord.table.clone();
+    let text_type = d.columns[2].data_type.native.clone();
+    d.columns[2].name = n("remark");
+    let mut extra = d.columns[2].clone();
+    extra.name = n("extra");
+    d.columns.push(extra);
+    let ix = |name: &str, col: &str| Index {
+        name: n(name),
+        columns: vec![IndexColumn { name: n(col), descending: false }],
+        unique: false,
+        method: None,
+    };
+    // 外键列另建索引：MySQL 家族不再自动建一个
+    d.indexes = vec![ix("ix_m4_remark", "remark"), ix("ix_m4_cust", "cust_id")];
+    d.foreign_keys = vec![ForeignKey {
+        name: Some(n("fk_m4_cust")),
+        columns: vec![n("cust_id")],
+        ref_schema: None,
+        ref_table: n("m4_cust"),
+        ref_columns: vec![n("id")],
+        on_delete: None,
+        on_update: None,
+    }];
+    let req = ModelDesignRequest {
+        schema: schema.clone(),
+        name: Some(n("m4_ord")),
+        draft: d,
+        renames: Renames { columns: [(n("remark"), n("note"))].into(), ..Default::default() },
+        fingerprint: Some(ord.fingerprint),
+    };
+    let p = s.preview_model_design(&m, &req).await.unwrap();
+    assert!(p.errors.is_empty() && !p.conflict, "{env}：{p:?}");
+    s.save_model_design(&m, req).await.unwrap();
+    s.models.rename_table(&m, &schema, &n("m4_cust"), &n("m4_customer")).unwrap();
+    let fk = &s.models.read_table(&m, &schema, &n("m4_ord")).unwrap().table.foreign_keys[0];
+    assert_eq!(fk.ref_table, n("m4_customer"), "{env}");
+    assert_eq!(
+        text_type,
+        s.models.read_table(&m, &schema, &n("m4_ord")).unwrap().table.columns[3].data_type.native
+    );
+
+    // 3. 模型 → 库：按记下的改名配对
+    let db = CompareSideRef::Database { profile: live.profile_id.clone(), schema: schema.clone() };
+    let md = CompareSideRef::Model { model: m.clone(), schema: schema.clone() };
+    let compare = |source: &CompareSideRef, target: &CompareSideRef| CompareRequest {
+        source: source.clone(),
+        target: target.clone(),
+        options: CompareOptions::default(),
+        renames: CompareRenames::default(),
+    };
+    let r = s.compare(&compare(&md, &db)).await.unwrap();
+    let status = m4_status(&r);
+    assert!(status.contains(&("m4_customer".into(), TableStatus::Renamed)), "{env}：{status:?}");
+    assert!(status.contains(&("m4_ord".into(), TableStatus::Changed)), "{env}：{status:?}");
+    let ord_items = &r.diff.tables.iter().find(|t| t.source.as_deref() == Some(&n("m4_ord"))).unwrap().items;
+    assert!(
+        ord_items.iter().any(|i| matches!(&i.change, SchemaChange::Table { change } if matches!(change.as_ref(), TableChange::RenameColumn { .. }))),
+        "{env}：{ord_items:?}"
+    );
+    // 其他测试会并发改动同一个 schema，不核对比对结果的指纹
+    let sync = SyncRequest { compare: compare(&md, &db), selected: m4_items(&r), fingerprint: None };
+    let p = s.preview_sync(&sync).await.unwrap();
+    let script = p.script.as_ref().unwrap();
+    assert!(script.refused.is_empty() && p.denied.is_none(), "{env}：{p:?}");
+    eprintln!("{env} 同步脚本：\n{}", p.text);
+    let started = s.start_sync(sync).await.unwrap();
+    let st = wait_task(s, &started.task.unwrap()).await;
+    assert_eq!(st.state, TaskState::Done, "{env}：{st:?}\n{}", p.text);
+    assert!(s.models.renames(&m, &schema).unwrap().is_empty(), "{env}");
+    let r = s.compare(&compare(&md, &db)).await.unwrap();
+    assert!(m4_items(&r).is_empty(), "{env}：{}", m4_detail(&r));
+
+    // 4. 库中加列 → 写回模型 → 两边一致
+    let z = match live.driver {
+        DriverId::Oracle => "z VARCHAR2(5)",
+        _ => "z VARCHAR(5)",
+    };
+    live.run(id, &format!("ALTER TABLE m4_ord ADD {z}")).await;
+    let r = s.compare(&compare(&db, &md)).await.unwrap();
+    let status = m4_status(&r);
+    assert!(status.contains(&("m4_customer".into(), TableStatus::Same)), "{env}：{status:?}");
+    assert!(status.contains(&("m4_ord".into(), TableStatus::Changed)), "{env}：{status:?}");
+    let sync = SyncRequest {
+        compare: compare(&db, &md),
+        selected: m4_items(&r),
+        fingerprint: Some(r.fingerprint.clone()),
+    };
+    let p = s.preview_sync(&sync).await.unwrap();
+    assert_eq!(p.writes.len(), 1, "{env}：{p:?}");
+    assert_eq!(s.start_sync(sync).await.unwrap().task, None);
+    let cols = s.models.read_table(&m, &schema, &n("m4_ord")).unwrap().table.columns;
+    assert_eq!(cols.last().unwrap().name, n("z"), "{env}");
+    let r = s.compare(&compare(&md, &db)).await.unwrap();
+    assert!(m4_items(&r).is_empty(), "{env}：{}", m4_detail(&r));
+
+    // 5. 库对库：同一个库中的第二个 schema
+    let other: Option<&'static str> = match live.driver {
+        DriverId::Postgres => Some("m4_b"),
+        DriverId::Mysql => Some("dblens_test_m4b"),
+        DriverId::Oracle | DriverId::Dameng => None,
+    };
+    let created = match other {
+        Some(o) => {
+            let sql = match live.driver {
+                DriverId::Mysql => format!("CREATE DATABASE IF NOT EXISTS {o}"),
+                _ => format!("CREATE SCHEMA IF NOT EXISTS {o}"),
+            };
+            let ev = live.submit(id, ExecRequest::new(sql)).await;
+            !ev.iter().any(|e| matches!(e, SessionEvent::Failed { .. }))
+        }
+        None => false,
+    };
+    if created {
+        let o = other.unwrap();
+        cleanup(Some(o)).await;
+        let b = CompareSideRef::Database { profile: live.profile_id.clone(), schema: o.into() };
+        let r = s.compare(&compare(&db, &b)).await.unwrap();
+        assert!(m4_status(&r).iter().all(|(_, st)| *st == TableStatus::Added), "{env}：{:?}", m4_status(&r));
+        let sync = SyncRequest { compare: compare(&db, &b), selected: m4_items(&r), fingerprint: None };
+        let p = s.preview_sync(&sync).await.unwrap();
+        let st = wait_task(s, &s.start_sync(sync).await.unwrap().task.unwrap()).await;
+        assert_eq!(st.state, TaskState::Done, "{env}：{st:?}\n{}", p.text);
+        let r = s.compare(&compare(&db, &b)).await.unwrap();
+        assert!(m4_items(&r).is_empty(), "{env}：{}", m4_detail(&r));
+        cleanup(Some(o)).await;
+    } else {
+        eprintln!("{env}：没有第二个 schema，跳过库对库比对");
+    }
+    cleanup(None).await;
+}
+
+#[tokio::test]
+async fn mysql_model() {
+    model_flow("DBLENS_TEST_MYSQL_URL").await;
+}
+
+#[tokio::test]
+async fn mariadb_model() {
+    model_flow("DBLENS_TEST_MARIADB_URL").await;
+}
+
+#[tokio::test]
+async fn postgres_model() {
+    model_flow("DBLENS_TEST_PG_URL").await;
+}
+
+#[tokio::test]
+async fn oracle_model() {
+    model_flow("DBLENS_TEST_ORACLE_URL").await;
+}
+
+#[tokio::test]
+async fn dameng_model() {
+    model_flow("DBLENS_TEST_DM_URL").await;
+}
+
 const MYSQL_SLOW: &str = "SELECT COUNT(*) FROM information_schema.columns a, information_schema.columns b, information_schema.columns c";
 
 /// 国产库第一批：与 MySQL、PostgreSQL 走同样的流程。`$ops` 是允许失败的运维查询。
@@ -1485,6 +1725,10 @@ macro_rules! domestic_flows {
             async fn design() {
                 design_flow($env).await;
             }
+            #[tokio::test]
+            async fn model() {
+                model_flow($env).await;
+            }
         }
     )*};
 }
```

- [ ] **Step 3: 修改 `crates/dblens-dialect/src/compare/normalize.rs`**

```diff
--- a/crates/dblens-dialect/src/compare/normalize.rs
+++ b/crates/dblens-dialect/src/compare/normalize.rs
@@ -44,6 +44,8 @@ pub(super) fn align(
     let mut t = target.clone();
     own_schema_refs(&mut s, source_schema);
     own_schema_refs(&mut t, target_schema);
+    default_actions(d, &mut s);
+    default_actions(d, &mut t);
     t.name = s.name.clone();
     for f in &mut t.foreign_keys {
         if f.ref_schema.is_none()
@@ -95,6 +97,24 @@ pub(super) fn align(
     (s, t)
 }
 
+/// 外键的默认动作写成"没有"：库读回 `NO ACTION`（MySQL 家族的 `RESTRICT` 与之相同），模型中通常不写。
+fn default_actions(d: &Dialect, t: &mut Table) {
+    let default = |a: &Option<String>| {
+        a.as_deref().is_some_and(|a| {
+            a.eq_ignore_ascii_case("NO ACTION")
+                || (d.family() == Family::MySql && a.eq_ignore_ascii_case("RESTRICT"))
+        })
+    };
+    for f in &mut t.foreign_keys {
+        if default(&f.on_delete) {
+            f.on_delete = None;
+        }
+        if default(&f.on_update) {
+            f.on_update = None;
+        }
+    }
+}
+
 /// 去掉库加的类型转换（`'abc'::character varying`）和多余的括号后比较默认值。
 fn same_default(d: &Dialect, a: &Option<String>, b: &Option<String>) -> bool {
     let norm = |v: &Option<String>| -> String {
```

- [ ] **Step 4: 检查**

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

- [ ] **Step 5: 提交**

```bash
git add crates/dblens-dialect/src/compare/normalize.rs crates/dblens-dialect/src/compare/tests.rs crates/dblens-service/tests/live.rs
git commit -m "test(service): 模型的实库测试；比对时外键的默认动作视为相同" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 8: 前端：模型面板、比对标签与设计器的模型模式

- 桌面程序：`folder_dialog` 命令（系统"选择文件夹"对话框）；`shared/shell.ts` 的 `folderDialog`。
- `shared/api.ts`：模型、比对、同步的类型和方法；`TaskStatus.kind` 加 `reverse`、`sync`，`TaskStatus.model`。
- `features/editor/tabs.ts`：`DesignTarget.model`（模型目录；这时标签的 `profileId` 为空串）；比对标签 `compare: { source, target }`（一边为 `{kind: 'model', path, schema}` 或 `{kind: 'database', profile, schema}`），`openCompare`、`setCompare`；保存与恢复时校验。
- `features/design/source.ts`：设计器选 schema、外键引用的数据来源（连接或模型）；`ForeignKeysPane`、`OptionsPane` 改用它。
- `DesignView.tsx` 的模型模式：按目录打开模型，能力和类型目录按模型的数据库类型，读表、检查（"变更"子标签代替"SQL 预览"）、保存（不弹确认框），工具栏"与库比对"，标题"模型名 / schema.表"，只读时不能保存。
- `features/model/`：`ModelsPanel`（打开的模型 → schema → 表，有错的标红；最近打开；工具栏；右键菜单）、`ModelDialogs`（打开、新建（产品，人大金仓、openGauss 另选兼容模式）、反向生成（任务进度））、`dialogs.ts`。
- `features/compare/`：`logic.ts`（默认不勾选删除表和删除列、依赖提示、变更摘要、两边定义的文本和不同的行、采用推测的改名）、`CompareView.tsx`、`SyncDialog.tsx`（目标是库时脚本预览和任务进度，目标是模型时列出要写的文件）。
- 外壳：活动栏"模型"；标题栏"文件 → 打开模型 / 新建模型 / 结构比对"；对象树 schema 右键"与模型比对… / 另存为模型…"；不绑定连接的标签在标签栏、标题、状态栏中不显示"连接已删除"；新建查询、新建表、导入时跳过这种标签的空连接。
- 词条：新文件 `locales/{zh-CN,en}/model.json`；`app.side.models`、`app.menu.file.*`、`tree.menu.compareModel` / `saveAsModel`。
- 样式：`shell.css` 末尾的模型与比对部分。

**Files:**
- Modify: `crates/dblens-desktop/src/main.rs`
- Modify: `crates/dblens-desktop/src/shell.rs`
- Modify: `web/src/app/Shell.tsx`
- Modify: `web/src/app/StatusBar.tsx`
- Modify: `web/src/app/TitleBar.tsx`
- Modify: `web/src/app/menus.test.ts`
- Modify: `web/src/app/menus.ts`
- Modify: `web/src/app/shell.css`
- Modify: `web/src/app/ui.ts`
- Create: `web/src/features/compare/CompareView.tsx`
- Create: `web/src/features/compare/SyncDialog.tsx`
- Create: `web/src/features/compare/logic.test.ts`
- Create: `web/src/features/compare/logic.ts`
- Modify: `web/src/features/design/DesignView.tsx`
- Modify: `web/src/features/design/ForeignKeysPane.tsx`
- Modify: `web/src/features/design/OptionsPane.tsx`
- Create: `web/src/features/design/source.ts`
- Modify: `web/src/features/editor/Workspace.tsx`
- Modify: `web/src/features/editor/actions.tsx`
- Modify: `web/src/features/editor/tabs.test.ts`
- Modify: `web/src/features/editor/tabs.ts`
- Create: `web/src/features/model/ModelDialogs.tsx`
- Create: `web/src/features/model/ModelsPanel.tsx`
- Create: `web/src/features/model/dialogs.ts`
- Modify: `web/src/features/tree/ObjectTree.tsx`
- Modify: `web/src/locales/en/app.json`
- Create: `web/src/locales/en/model.json`
- Modify: `web/src/locales/en/tree.json`
- Modify: `web/src/locales/index.ts`
- Modify: `web/src/locales/zh-CN/app.json`
- Create: `web/src/locales/zh-CN/model.json`
- Modify: `web/src/locales/zh-CN/tree.json`
- Modify: `web/src/shared/api.ts`
- Modify: `web/src/shared/shell.ts`

**Interfaces:**
- Consumes：任务 6 的接口。
- Produces：`useTabs` 的 `openCompare(target)`、`setCompare(id, target)`；`features/model/dialogs.ts` 的 `useModelDialogs`（`openOpen`、`openCreate`、`openReverse({profileId, schema})`）。

- [ ] **Step 1: 修改 `web/src/app/menus.test.ts`**

```diff
--- a/web/src/app/menus.test.ts
+++ b/web/src/app/menus.test.ts
@@ -19,6 +19,10 @@ describe('标题栏菜单', () => {
       'import',
       'sql-file',
       '-',
+      'open-model',
+      'new-model',
+      'compare',
+      '-',
       'settings',
       '-',
       'quit',
@@ -30,7 +34,7 @@ describe('标题栏菜单', () => {
   it('浏览器模式没有窗口菜单和退出，帮助只有关于', () => {
     const m = buildMenus({ ...base, desktop: false })
     expect(m.map((g) => g.label)).toEqual(['文件', '查看', 'SQL', '帮助'])
-    expect(keys(m[0].items)).toEqual(['new-connection', 'new-query', 'close-tab', '-', 'new-table', 'import', 'sql-file', '-', 'settings'])
+    expect(keys(m[0].items)).toEqual(['new-connection', 'new-query', 'close-tab', '-', 'new-table', 'import', 'sql-file', '-', 'open-model', 'new-model', 'compare', '-', 'settings'])
     expect(keys(m[3].items)).toEqual(['help:about'])
   })
 
```

- [ ] **Step 2: 新建 `web/src/features/compare/logic.test.ts`**

```ts
import { describe, expect, it } from 'vitest'

import type { CompareResult, DesignForeignKey, DesignTableModel, TableDiff } from '../../shared/api'
import { acceptSuggestion, compareLines, defaultSelection, dependencyIssues, EMPTY_RENAMES, tableText } from './logic'

const fk = (name: string, ref: string): DesignForeignKey => ({
  name,
  columns: ['ref_id'],
  ref_schema: null,
  ref_table: ref,
  ref_columns: ['id'],
  on_delete: null,
  on_update: null,
})

const table = (name: string, fks: DesignForeignKey[] = []): DesignTableModel => ({
  name,
  comment: null,
  columns: [
    { name: 'id', ordinal: 1, data_type: { native: 'integer', logical: null }, nullable: false, default: null, comment: null, auto_increment: false, generated: null },
  ],
  primary_key: { name: null, columns: ['id'] },
  uniques: [],
  foreign_keys: fks,
  checks: [],
  indexes: [],
  triggers: [],
  partition: null,
})

/** items 新建（1），orders 加指向 items 的外键（2）、删列（3）；old 删除（4），被 keep 的外键引用。 */
function result(): CompareResult {
  const tables: TableDiff[] = [
    { status: 'added', source: 'items', target: null, appended: true, items: [{ id: 1, change: { kind: 'table', change: { kind: 'create_table', table: table('items') } } }] },
    {
      status: 'changed',
      source: 'orders',
      target: 'orders',
      appended: true,
      items: [
        { id: 2, change: { kind: 'table', change: { kind: 'add_foreign_key', key: fk('fk_items', 'items') } } },
        { id: 3, change: { kind: 'table', change: { kind: 'drop_column', name: 'x' } } },
      ],
    },
    { status: 'removed', source: null, target: 'old', appended: true, items: [{ id: 4, change: { kind: 'drop_table', name: 'old' } }] },
    { status: 'same', source: 'keep', target: 'keep', appended: true, items: [] },
  ]
  return {
    diff: { tables, suggestions: [] },
    renames: EMPTY_RENAMES,
    source_errors: [],
    target_errors: [],
    source_tables: [],
    target_tables: [table('orders'), table('old'), table('keep', [fk('fk_old', 'old')])],
    fingerprint: 'f',
  }
}

describe('比对结果的勾选', () => {
  it('默认不勾选删除表、删除列', () => {
    expect(defaultSelection(result().diff.tables)).toEqual([1, 2])
  })

  it('依赖提示：外键引用的新表没有勾选建表、删除的表还被引用', () => {
    const r = result()
    expect(dependencyIssues(r, new Set([1, 2]))).toEqual([])
    expect(dependencyIssues(r, new Set([2]))).toEqual(['orders 的外键引用 items，但没有勾选新建 items'])
    expect(dependencyIssues(r, new Set([1, 2, 4]))).toEqual(['删除 old 时 keep 的外键仍引用它'])
  })
})

describe('两边定义对照', () => {
  it('表定义的文本与不同的行', () => {
    const a = table('t', [fk('fk_x', 'x')])
    const b = { ...table('t'), columns: [{ ...a.columns[0], data_type: { native: 'bigint', logical: null } }] }
    expect(tableText(a)).toEqual(['id integer NOT NULL', 'PRIMARY KEY (id)', 'FOREIGN KEY fk_x (ref_id) → x(id)'])
    const [l, r] = compareLines(tableText(a), tableText(b))
    expect(l.map((x) => x.diff)).toEqual([true, false, true])
    expect(r.map((x) => x.diff)).toEqual([true, false])
  })

  it('采用推测的改名', () => {
    const r = acceptSuggestion(EMPTY_RENAMES, { table: null, source: 'clients', target: 'customers' })
    const r2 = acceptSuggestion(r, { table: 'orders', source: 'total', target: 'amount' })
    expect(r2).toEqual({ tables: { clients: 'customers' }, columns: { orders: { total: 'amount' } } })
  })
})
```

- [ ] **Step 3: 修改 `web/src/features/editor/tabs.test.ts`**

```diff
--- a/web/src/features/editor/tabs.test.ts
+++ b/web/src/features/editor/tabs.test.ts
@@ -127,6 +127,23 @@ describe('打开后执行', () => {
     expect(parseTabs(JSON.stringify({ tabs: [{ id: 'x', title: 't', profileId: 'p', sql: '', design: { schema: 1 } }] })).tabs).toEqual([])
   })
 
+  it('模型中的表与库中同名的表各开一个；比对标签保存后恢复', () => {
+    useTabs.getState().restore(undefined)
+    const { openDesign, openCompare, setCompare } = useTabs.getState()
+    const db = openDesign('p', { schema: 'public', table: 'orders' })
+    const m = openDesign('', { schema: 'public', table: 'orders', model: 'D:/m' })
+    expect(m).not.toBe(db)
+    expect(openDesign('', { schema: 'public', table: 'orders', model: 'D:/m' })).toBe(m)
+    const c = openCompare({ source: { kind: 'model', path: 'D:/m', schema: 'public' }, target: null })
+    setCompare(c, { source: null, target: { kind: 'database', profile: 'p', schema: 'shop' } })
+    const raw = JSON.stringify({ tabs: useTabs.getState().tabs, active: c, seq: 1 })
+    const back = parseTabs(raw)
+    expect(back.tabs).toHaveLength(3)
+    expect(back.tabs[2]).toMatchObject({ title: '比对', profileId: '', compare: { source: null, target: { kind: 'database', schema: 'shop' } } })
+    const bad = { id: 'x', title: 't', profileId: '', sql: '', compare: { source: { kind: 'model', schema: 's' }, target: null } }
+    expect(parseTabs(JSON.stringify({ tabs: [bad] })).tabs).toEqual([])
+  })
+
   it('标记只取走一次，不随标签保存', () => {
     const id = useTabs.getState().open('p', 'SELECT 1')
     markRun(id)
```

- [ ] **Step 4: 修改 `crates/dblens-desktop/src/main.rs`**

```diff
--- a/crates/dblens-desktop/src/main.rs
+++ b/crates/dblens-desktop/src/main.rs
@@ -56,6 +56,7 @@ fn run(data_dir: PathBuf) -> Result<(), String> {
             shell::set_auto_check_update,
             shell::save_dialog,
             shell::open_dialog,
+            shell::folder_dialog,
             shell::reveal_in_folder
         ])
         .setup(move |app| {
```

- [ ] **Step 5: 修改 `crates/dblens-desktop/src/shell.rs`**

```diff
--- a/crates/dblens-desktop/src/shell.rs
+++ b/crates/dblens-desktop/src/shell.rs
@@ -137,6 +137,16 @@ pub async fn open_dialog(app: AppHandle, filters: Vec<FileFilter>) -> Option<Str
     dialog.blocking_pick_file().and_then(|p| p.into_path().ok()).map(|p| p.display().to_string())
 }
 
+/// 系统"选择文件夹"对话框（打开、新建模型时选目录）。用户取消时返回 None。
+#[tauri::command]
+pub async fn folder_dialog(app: AppHandle) -> Option<String> {
+    app.dialog()
+        .file()
+        .blocking_pick_folder()
+        .and_then(|p| p.into_path().ok())
+        .map(|p| p.display().to_string())
+}
+
 /// 在系统的文件管理器中显示文件（导出完成后"打开所在文件夹"）。
 #[tauri::command]
 pub fn reveal_in_folder(path: String) -> Result<(), String> {
```

- [ ] **Step 6: 修改 `web/src/app/Shell.tsx`**

```diff
--- a/web/src/app/Shell.tsx
+++ b/web/src/app/Shell.tsx
@@ -3,7 +3,7 @@
 // 没有查询标签时工作区显示欢迎页；工作区（Monaco、结果表格）懒加载。
 
 import { Button, Spin } from 'antd'
-import { IconDatabase, IconFileCode, IconHistory, IconPlus, IconSettings, IconStar, IconTool } from '@tabler/icons-react'
+import { IconDatabase, IconFileCode, IconHistory, IconPlus, IconSchema, IconSettings, IconStar, IconTool } from '@tabler/icons-react'
 import { Suspense, lazy, useEffect, useRef } from 'react'
 import { useTranslation } from 'react-i18next'
 import { Group, Panel, type PanelImperativeHandle, Separator, useDefaultLayout } from 'react-resizable-panels'
@@ -17,6 +17,8 @@ import SqlFileDialog from '../features/import/SqlFileDialog'
 import FavoriteDialog from '../features/favorites/FavoriteDialog'
 import FavoritesPanel from '../features/favorites/FavoritesPanel'
 import HistoryPanel from '../features/history/HistoryPanel'
+import ModelDialogs from '../features/model/ModelDialogs'
+import ModelsPanel from '../features/model/ModelsPanel'
 import OpsPanel from '../features/ops/OpsPanel'
 import ObjectTree from '../features/tree/ObjectTree'
 import { layoutStorage } from '../shared/settings'
@@ -34,6 +36,7 @@ const Workspace = lazy(async () => {
 // 侧栏名称在渲染时按 app.side.<key> 取
 const SIDES: { key: SideKey; icon: typeof IconDatabase }[] = [
   { key: 'db', icon: IconDatabase },
+  { key: 'models', icon: IconSchema },
   { key: 'history', icon: IconHistory },
   { key: 'favorites', icon: IconStar },
   { key: 'ops', icon: IconTool },
@@ -41,6 +44,7 @@ const SIDES: { key: SideKey; icon: typeof IconDatabase }[] = [
 
 function SideContent({ side }: { side: SideKey }) {
   if (side === 'db') return <ObjectTree />
+  if (side === 'models') return <ModelsPanel />
   if (side === 'history') return <HistoryPanel />
   if (side === 'favorites') return <FavoritesPanel />
   return <OpsPanel />
@@ -161,6 +165,7 @@ export default function Shell() {
       <ExportDialog />
       <ImportDialog />
       <SqlFileDialog />
+      <ModelDialogs />
     </div>
   )
 }
```

- [ ] **Step 7: 修改 `web/src/app/StatusBar.tsx`**

```diff
--- a/web/src/app/StatusBar.tsx
+++ b/web/src/app/StatusBar.tsx
@@ -65,6 +65,15 @@ export default function StatusBar() {
   const selected = useUi((s) => s.selectedProfile)
   const active = useTabs((s) => s.tabs.find((t) => t.id === s.active))
   const { data: profiles } = useQuery({ queryKey: ['profiles'], queryFn: () => api.profiles() })
+  // 模型的设计器标签、比对标签不绑定连接：显示模型目录
+  if (active && !active.profileId) {
+    return (
+      <div className="row statusbar">
+        <span className="muted">{active.design?.model ?? t('model.compare.title')}</span>
+        <span className="grow" />
+      </div>
+    )
+  }
   if (active) {
     return (
       <div className="row statusbar">
```

- [ ] **Step 8: 修改 `web/src/app/TitleBar.tsx`**

```diff
--- a/web/src/app/TitleBar.tsx
+++ b/web/src/app/TitleBar.tsx
@@ -12,6 +12,7 @@ import { useQueryActions } from '../features/editor/actions'
 import { type EditorCommand, sendCommand } from '../features/editor/commands'
 import { useTabs } from '../features/editor/tabs'
 import { useImportDialogs } from '../features/import/dialog'
+import { useModelDialogs } from '../features/model/dialogs'
 import { useSetting, useSettings } from '../shared/settings'
 import { type WindowAction, desktop, titleBarInset } from '../shared/shell'
 import { THEME_KEY, type ThemePref, parseThemePref } from '../shared/theme'
@@ -56,7 +57,8 @@ export default function TitleBar() {
   const title = tab
     ? t('app.titleBar.windowTitle', {
         tab: tab.title,
-        connection: profile?.name ?? t('app.status.connectionDeleted'),
+        // 模型的设计器标签、比对标签不绑定连接
+        connection: tab.profileId ? (profile?.name ?? t('app.status.connectionDeleted')) : t('app.side.models'),
         app: appName,
       })
     : appName
@@ -71,18 +73,21 @@ export default function TitleBar() {
     else if (key === 'close-tab') closeActive()
     else if (key === 'new-table') {
       // 连接同"导入"；schema 用当前数据标签或设计器标签的，没有时在设计器的"选项"中选
-      const id = ui.selectedProfile ?? tab?.profileId ?? profiles?.[0]?.id
+      const id = ui.selectedProfile ?? (tab?.profileId || undefined) ?? profiles?.[0]?.id
       const schema = (tab?.profileId === id && (tab?.data?.schema ?? tab?.design?.schema)) || ''
       if (!id) ui.openConnection(null)
       else useTabs.getState().openDesign(id, { schema, table: null })
     }
     else if (key === 'import' || key === 'sql-file') {
       // 用对象树中选中的连接，没有时用当前标签的、第一个连接；一个都没有时新建连接
-      const id = ui.selectedProfile ?? tab?.profileId ?? profiles?.[0]?.id
+      const id = ui.selectedProfile ?? (tab?.profileId || undefined) ?? profiles?.[0]?.id
       if (!id) ui.openConnection(null)
       else if (key === 'import') useImportDialogs.getState().openImport({ profileId: id, schema: null, table: null, create: false })
       else useImportDialogs.getState().openSqlFile({ profileId: id, schema: null })
     }
+    else if (key === 'open-model') useModelDialogs.getState().openOpen()
+    else if (key === 'new-model') useModelDialogs.getState().openCreate()
+    else if (key === 'compare') useTabs.getState().openCompare({ source: null, target: null })
     else if (kind === 'sql') sendCommand(arg as EditorCommand)
     else if (key === 'settings') ui.setSettingsOpen(true)
     else if (key === 'quit') act('quit')
```

- [ ] **Step 9: 修改 `web/src/app/menus.ts`**

```diff
--- a/web/src/app/menus.ts
+++ b/web/src/app/menus.ts
@@ -32,6 +32,10 @@ export function buildMenus(s: MenuState): MenuGroup[] {
     { key: 'import', label: t('app.menu.file.import') },
     { key: 'sql-file', label: t('app.menu.file.sqlFile') },
     'divider',
+    { key: 'open-model', label: t('app.menu.file.openModel') },
+    { key: 'new-model', label: t('app.menu.file.newModel') },
+    { key: 'compare', label: t('app.menu.file.compare') },
+    'divider',
   ]
   file.push({ key: 'settings', label: t('app.menu.file.settings') })
   if (s.desktop) file.push('divider', { key: 'quit', label: t('app.menu.file.quit') })
@@ -40,6 +44,7 @@ export function buildMenus(s: MenuState): MenuGroup[] {
   const theme = (key: ThemePref, label: string) => ({ key: `theme:${key}`, label, checked: s.theme === key })
   const view: MenuEntry[] = [
     side('db'),
+    side('models'),
     side('history'),
     side('favorites'),
     side('ops'),
```

- [ ] **Step 10: 修改 `web/src/app/shell.css`**

```diff
--- a/web/src/app/shell.css
+++ b/web/src/app/shell.css
@@ -718,3 +718,63 @@ body {
   max-height: 160px;
   overflow: auto;
 }
+
+/* 模型与比对（M4a） */
+.models-panel .a {
+  cursor: pointer;
+  padding: 2px;
+  border-radius: 3px;
+}
+.models-panel .a:hover {
+  background: var(--sel);
+}
+.models-recent {
+  padding: 2px 12px;
+  cursor: pointer;
+  overflow: hidden;
+  white-space: nowrap;
+  text-overflow: ellipsis;
+}
+.models-recent:hover {
+  background: var(--sel);
+}
+.compare-view {
+  display: flex;
+  flex-direction: column;
+  gap: 6px;
+  min-height: 0;
+  height: 100%;
+}
+.compare-side-label {
+  padding: 0 6px;
+  line-height: 24px;
+}
+.compare-body {
+  display: flex;
+  gap: 8px;
+  min-height: 0;
+  padding: 0 10px;
+}
+.compare-tree {
+  flex: 1;
+  overflow: auto;
+  min-width: 280px;
+}
+.compare-detail {
+  flex: 1;
+  display: flex;
+  gap: 8px;
+  overflow: auto;
+  font-family: var(--mono);
+}
+.compare-col {
+  flex: 1;
+  min-width: 0;
+}
+.compare-line {
+  white-space: pre-wrap;
+  padding: 0 4px;
+}
+.compare-line.diff {
+  background: var(--warn-bg);
+}
```

- [ ] **Step 11: 修改 `web/src/app/ui.ts`**

```diff
--- a/web/src/app/ui.ts
+++ b/web/src/app/ui.ts
@@ -4,13 +4,13 @@ import { create } from 'zustand'
 
 import { useSettings } from '../shared/settings'
 
-export type SideKey = 'db' | 'history' | 'favorites' | 'ops'
+export type SideKey = 'db' | 'models' | 'history' | 'favorites' | 'ops'
 
 export const SIDE_KEY = 'ui.side'
 
 export function parseSide(v: string | undefined): SideKey | null {
   if (v === 'none') return null
-  return v === 'history' || v === 'favorites' || v === 'ops' ? v : 'db'
+  return v === 'models' || v === 'history' || v === 'favorites' || v === 'ops' ? v : 'db'
 }
 
 interface UiState {
```

- [ ] **Step 12: 新建 `web/src/features/compare/CompareView.tsx`**

```tsx
// 比对标签（M4a 设计文档第 7 节）：上方两边的选择（模型或连接，再选 schema）、交换、选项；中间三层结果树
// （勾选框、只看差异、按状态过滤、疑似改名"按改名处理"）；右侧选中表的两边定义对照，不同的行标色；
// 底部依赖提示、"同步到目标"，目标是库时另有复制脚本、在查询标签中打开、另存为。由工作区懒加载。

import { IconArrowsExchange, IconCopy, IconDeviceFloppy, IconFileCode, IconPlayerPlay, IconRefresh } from '@tabler/icons-react'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { Alert, App, Button, Checkbox, Empty, Select, Space, Spin, Tag, Tree } from 'antd'
import type { DataNode } from 'antd/es/tree'
import { useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { useApi } from '../../app/context'
import { useUi } from '../../app/ui'
import type {
  CompareOptions,
  CompareRenames,
  CompareRequest,
  CompareResult,
  CompareSideRef,
  Profile,
  SyncRequest,
  TableStatus,
} from '../../shared/api'
import { desktop } from '../../shared/shell'
import { withVault } from '../connections/vault'
import { useQueryActions } from '../editor/actions'
import { type CompareSide, type QueryTab, useTabs } from '../editor/tabs'
import {
  EMPTY_RENAMES,
  STATUSES,
  acceptSuggestion,
  compareLines,
  defaultSelection,
  dependencyIssues,
  describeItem,
  isDangerous,
  tableLabel,
  tableText,
} from './logic'
import SyncDialog from './SyncDialog'

const DEFAULT_OPTIONS: CompareOptions = {
  ignore_comments: false,
  ignore_column_order: false,
  ignore_generated_names: true,
  case_insensitive: false,
}

const STATUS_COLOR: Record<TableStatus, string> = {
  added: 'green',
  removed: 'red',
  renamed: 'blue',
  changed: 'orange',
  same: 'default',
}

/** 一边的选择：模型（打开的模型）或连接，再选 schema。 */
function SidePicker({ label, value, profiles, onChange }: {
  label: string
  value: CompareSide | null
  profiles: Profile[]
  onChange(v: CompareSide | null): void
}) {
  const api = useApi()
  const { t } = useTranslation()
  // 已选的一边以它的种类为准（交换两边后也对）；还没选时用下拉框选的种类
  const [picked, setKind] = useState<'model' | 'database'>(value?.kind ?? 'database')
  const kind = value?.kind ?? picked
  const models = useQuery({ queryKey: ['models'], queryFn: () => api.models(), retry: false })
  const model = value?.kind === 'model' ? models.data?.open.find((m) => m.path === value.path) : undefined
  const { data: modelTables = [] } = useQuery({
    queryKey: ['model-tables', model?.id],
    queryFn: () => api.modelTables(model!.id),
    enabled: !!model,
  })
  const profile = value?.kind === 'database' ? value.profile : null
  const { data: dbSchemas = [] } = useQuery({
    queryKey: ['schemas', profile],
    queryFn: () => withVault(api, () => api.schemas(profile!)),
    enabled: !!profile,
  })
  const schemas =
    value?.kind === 'model'
      ? [...new Set([...(model?.info.schemas ?? []), ...modelTables.map((x) => x.schema)])]
      : dbSchemas.map((s) => s.name)
  return (
    <Space.Compact>
      <span className="compare-side-label">{label}</span>
      <Select
        size="small"
        style={{ width: 90 }}
        value={kind}
        onChange={(k) => {
          setKind(k)
          onChange(null)
        }}
        options={[
          { value: 'database', label: t('model.compare.database') },
          { value: 'model', label: t('model.compare.model') },
        ]}
      />
      {kind === 'model' ? (
        <Select
          size="small"
          style={{ width: 180 }}
          placeholder={t('model.compare.pickModel')}
          value={value?.kind === 'model' ? value.path : undefined}
          onChange={(path) => onChange({ kind: 'model', path, schema: '' })}
          options={(models.data?.open ?? []).map((m) => ({ value: m.path, label: m.info.name, title: m.path }))}
        />
      ) : (
        <Select
          size="small"
          style={{ width: 180 }}
          placeholder={t('model.compare.pickProfile')}
          value={value?.kind === 'database' ? value.profile : undefined}
          onChange={(p) => onChange({ kind: 'database', profile: p, schema: '' })}
          options={profiles.map((p) => ({ value: p.id, label: p.name }))}
        />
      )}
      <Select
        size="small"
        style={{ width: 160 }}
        showSearch
        placeholder="schema"
        disabled={!value}
        value={value?.schema || undefined}
        onChange={(schema) => value && onChange({ ...value, schema })}
        options={schemas.map((s) => ({ value: s }))}
      />
    </Space.Compact>
  )
}

export default function CompareView({ tab, profiles }: { tab: QueryTab; profiles: Profile[] }) {
  const api = useApi()
  const { t } = useTranslation()
  const { message } = App.useApp()
  const qc = useQueryClient()
  const { openSql } = useQueryActions()
  const target = tab.compare!
  const [options, setOptions] = useState(DEFAULT_OPTIONS)
  const [renames, setRenames] = useState<CompareRenames>(EMPTY_RENAMES)
  const [result, setResult] = useState<{ req: CompareRequest; r: CompareResult } | null>(null)
  const [selected, setSelected] = useState<Set<number>>(new Set())
  const [onlyDiff, setOnlyDiff] = useState(true)
  const [statuses, setStatuses] = useState<TableStatus[]>([])
  const [focus, setFocus] = useState<number | null>(null)
  const [busy, setBusy] = useState(false)
  const [syncing, setSyncing] = useState(false)

  const setSides = (source: CompareSide | null, tgt: CompareSide | null) => {
    useTabs.getState().setCompare(tab.id, { source, target: tgt })
    setResult(null)
    setRenames(EMPTY_RENAMES)
  }
  const ready = (s: CompareSide | null): s is CompareSide => !!s && !!s.schema
  const toRef = async (s: CompareSide): Promise<CompareSideRef> =>
    s.kind === 'model'
      ? { kind: 'model', model: (await api.openModel(s.path)).id, schema: s.schema }
      : { kind: 'database', profile: s.profile, schema: s.schema }

  const run = async (r: CompareRenames = renames, keepSelection = false) => {
    if (!ready(target.source) || !ready(target.target)) return
    setBusy(true)
    try {
      const req: CompareRequest = {
        source: await toRef(target.source),
        target: await toRef(target.target),
        options,
        renames: r,
      }
      const res = await withVault(api, () => api.compare(req))
      setResult({ req, r: res })
      if (!keepSelection) setSelected(new Set(defaultSelection(res.diff.tables)))
      setFocus(null)
    } catch (e) {
      message.error((e as Error).message)
    } finally {
      setBusy(false)
    }
  }

  const r = result?.r
  const issues = useMemo(() => (r ? dependencyIssues(r, selected) : []), [r, selected])
  const tree: DataNode[] = useMemo(() => {
    if (!r) return []
    return r.diff.tables
      .map((d, index) => ({ d, index }))
      .filter(({ d }) => (!onlyDiff || d.status !== 'same') && (!statuses.length || statuses.includes(d.status)))
      .map(({ d, index }) => ({
        key: `t:${index}`,
        checkable: d.items.length > 0,
        title: (
          <span>
            <Tag color={STATUS_COLOR[d.status]}>{t(`model.compare.status.${d.status}`)}</Tag>
            {tableLabel(d)}
          </span>
        ),
        children: d.items.map((i) => ({
          key: `i:${i.id}`,
          isLeaf: true,
          title: (
            <span className={isDangerous(i.change) && selected.has(i.id) ? 'err' : undefined}>{describeItem(i.change)}</span>
          ),
        })),
      }))
  }, [r, onlyDiff, statuses, selected, t])

  const targetProfile = target.target?.kind === 'database' ? profiles.find((p) => p.id === (target.target as { profile: string }).profile) : undefined
  const syncRequest: SyncRequest | null = result && { compare: result.req, selected: [...selected], fingerprint: result.r.fingerprint }

  const scriptText = async () => (await api.syncPreview(syncRequest!)).text
  const scriptAction = async (kind: 'copy' | 'open' | 'save') => {
    try {
      const text = await scriptText()
      if (kind === 'copy') {
        await navigator.clipboard.writeText(text)
        message.success(t('model.compare.copied'))
      } else if (kind === 'open') openSql(text, targetProfile?.id)
      else if (desktop) {
        const path = await desktop.saveDialog('sync.sql', [{ name: 'SQL', extensions: ['sql'] }])
        if (path) {
          await api.saveScript(path, text)
          message.success(t('model.compare.saved'))
        }
      } else {
        const a = document.createElement('a')
        a.href = URL.createObjectURL(new Blob([text], { type: 'text/plain' }))
        a.download = 'sync.sql'
        a.click()
        URL.revokeObjectURL(a.href)
      }
    } catch (e) {
      message.error((e as Error).message)
    }
  }

  const afterSync = () => {
    setSyncing(false)
    if (targetProfile) useUi.getState().refreshTree(targetProfile.id, target.target!.schema)
    void qc.invalidateQueries({ queryKey: ['model-tables'] })
    void run()
  }

  const focused = focus === null ? undefined : r?.diff.tables[focus]
  const [left, right] = compareLines(
    tableText(r?.source_tables.find((x) => x.name === focused?.source)),
    tableText(r?.target_tables.find((x) => x.name === focused?.target)),
  )
  const option = (key: keyof CompareOptions) => (
    <Checkbox checked={options[key]} onChange={(e) => setOptions({ ...options, [key]: e.target.checked })}>
      {t(`model.compare.option.${key}`)}
    </Checkbox>
  )
  return (
    <div className="compare-view">
      <div className="row toolbar" style={{ flexWrap: 'wrap', gap: 8 }}>
        <SidePicker key={`s-${tab.id}`} label={t('model.compare.source')} value={target.source} profiles={profiles} onChange={(s) => setSides(s, target.target)} />
        <Button size="small" type="text" title={t('model.compare.swap')} icon={<IconArrowsExchange size={16} />} onClick={() => setSides(target.target, target.source)} />
        <SidePicker key={`t-${tab.id}`} label={t('model.compare.target')} value={target.target} profiles={profiles} onChange={(s) => setSides(target.source, s)} />
        <Button size="small" type="primary" icon={<IconRefresh size={16} />} loading={busy} disabled={!ready(target.source) || !ready(target.target)} onClick={() => void run()}>
          {t('model.compare.run')}
        </Button>
      </div>
      <div className="row toolbar" style={{ gap: 12 }}>
        {option('ignore_comments')}
        {option('ignore_column_order')}
        {option('ignore_generated_names')}
        {option('case_insensitive')}
        <span className="grow" />
        <Checkbox checked={onlyDiff} onChange={(e) => setOnlyDiff(e.target.checked)}>{t('model.compare.onlyDiff')}</Checkbox>
        <Select
          size="small"
          mode="multiple"
          allowClear
          style={{ minWidth: 160 }}
          placeholder={t('model.compare.statusFilter')}
          value={statuses}
          onChange={setStatuses}
          options={STATUSES.map((s) => ({ value: s, label: t(`model.compare.status.${s}`) }))}
        />
      </div>
      {busy && !r ? (
        <Spin style={{ margin: 48 }} />
      ) : !r ? (
        <Empty style={{ margin: 48 }} description={t('model.compare.hint')} />
      ) : (
        <>
          {[...r.source_errors, ...r.target_errors].map((e) => (
            <Alert key={e.path} type="error" showIcon message={t('model.compare.fileError', { path: e.path, message: e.message })} />
          ))}
          {r.diff.suggestions.length > 0 && (
            <Alert
              type="info"
              showIcon
              message={t('model.compare.suggestions')}
              description={r.diff.suggestions.map((s) => (
                <div key={`${s.table}/${s.source}`} className="row" style={{ gap: 8 }}>
                  <span>
                    {s.table ? `${s.table}.` : ''}
                    {s.target} → {s.source}
                  </span>
                  <Button
                    size="small"
                    type="link"
                    onClick={() => {
                      const next = acceptSuggestion(renames, s)
                      setRenames(next)
                      void run(next)
                    }}
                  >
                    {t('model.compare.acceptRename')}
                  </Button>
                </div>
              ))}
            />
          )}
          <div className="compare-body grow">
            <div className="compare-tree">
              <div className="muted">{t('model.compare.summary', { changed: r.diff.tables.filter((d) => d.status !== 'same').length, total: r.diff.tables.length })}</div>
              <Tree
                checkable
                blockNode
                defaultExpandAll
                treeData={tree}
                checkedKeys={[...selected].map((id) => `i:${id}`)}
                onCheck={(keys) => {
                  const list = Array.isArray(keys) ? keys : keys.checked
                  setSelected(new Set(list.map(String).filter((k) => k.startsWith('i:')).map((k) => Number(k.slice(2)))))
                }}
                onSelect={(keys) => {
                  const k = String(keys[0] ?? '')
                  const id = k.startsWith('i:') ? Number(k.slice(2)) : null
                  const index = k.startsWith('t:') ? Number(k.slice(2)) : r.diff.tables.findIndex((d) => d.items.some((i) => i.id === id))
                  setFocus(index >= 0 ? index : null)
                }}
              />
            </div>
            <div className="compare-detail">
              {focused ? (
                <>
                  <div className="compare-col">
                    <b>{t('model.compare.source')}: {focused.source ?? '—'}</b>
                    {left.map((l, i) => (
                      <div key={i} className={l.diff ? 'compare-line diff' : 'compare-line'}>{l.text}</div>
                    ))}
                  </div>
                  <div className="compare-col">
                    <b>{t('model.compare.target')}: {focused.target ?? '—'}</b>
                    {right.map((l, i) => (
                      <div key={i} className={l.diff ? 'compare-line diff' : 'compare-line'}>{l.text}</div>
                    ))}
                  </div>
                </>
              ) : (
                <span className="muted">{t('model.compare.pickTable')}</span>
              )}
            </div>
          </div>
          {issues.map((x) => (
            <Alert key={x} type="warning" showIcon message={x} />
          ))}
          <div className="row design-run">
            <span>{t('model.compare.selected', { count: selected.size })}</span>
            <span className="grow" />
            {targetProfile && (
              <>
                <Button size="small" icon={<IconCopy size={16} />} disabled={!selected.size} onClick={() => void scriptAction('copy')}>
                  {t('model.compare.copy')}
                </Button>
                <Button size="small" icon={<IconFileCode size={16} />} disabled={!selected.size} onClick={() => void scriptAction('open')}>
                  {t('model.compare.openQuery')}
                </Button>
                <Button size="small" icon={<IconDeviceFloppy size={16} />} disabled={!selected.size} onClick={() => void scriptAction('save')}>
                  {t('model.compare.saveAs')}
                </Button>
              </>
            )}
            <Button size="small" type="primary" icon={<IconPlayerPlay size={16} />} disabled={!selected.size} onClick={() => setSyncing(true)}>
              {t('model.compare.sync')}
            </Button>
          </div>
        </>
      )}
      {syncing && syncRequest && (
        <SyncDialog
          request={syncRequest}
          driver={targetProfile?.driver}
          profileId={targetProfile?.id ?? null}
          onDone={afterSync}
          onClose={() => {
            setSyncing(false)
            void run(renames, true)
          }}
        />
      )}
    </div>
  )
}
```

- [ ] **Step 13: 新建 `web/src/features/compare/SyncDialog.tsx`**

```tsx
// 同步到目标（M4a 设计文档第 6 节）：目标是库时预览脚本（不能重建的表、执行策略的结论、提示），确认后作为任务执行并显示进度；
// 目标是模型时列出要写的表文件，确认后直接写回。引入 Monaco（脚本预览），由比对标签懒加载。

import { useQuery } from '@tanstack/react-query'
import { Alert, App, Button, Modal, Progress, Spin } from 'antd'
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { useApi } from '../../app/context'
import { type DriverId, type SyncRequest, type TaskStatus, errorText } from '../../shared/api'
import SqlBox from '../design/SqlBox'
import { useQueryActions } from '../editor/actions'

export default function SyncDialog({ request, driver, profileId, onDone, onClose }: {
  request: SyncRequest
  /** 目标是库时的驱动（脚本高亮）。 */
  driver: DriverId | undefined
  profileId: string | null
  onDone(): void
  onClose(): void
}) {
  const api = useApi()
  const { t } = useTranslation()
  const { message } = App.useApp()
  const { openSql } = useQueryActions()
  const [task, setTask] = useState<TaskStatus | null>(null)
  const [busy, setBusy] = useState(false)
  const { data: p, error, isFetching } = useQuery({
    queryKey: ['sync-preview', JSON.stringify(request)],
    queryFn: () => api.syncPreview(request),
    retry: false,
  })

  useEffect(() => {
    if (!task) return
    if (task.state === 'done') {
      message.success(t('model.sync.done'))
      onDone()
      return
    }
    if (task.state !== 'running') return
    const h = setTimeout(async () => {
      try {
        setTask(await api.task(task.id))
      } catch (e) {
        message.error((e as Error).message)
        setTask(null)
      }
    }, 400)
    return () => clearTimeout(h)
  }, [task, api, message, t, onDone])

  const run = async () => {
    setBusy(true)
    try {
      const id = await api.sync({ ...request, fingerprint: p?.fingerprint ?? null })
      if (id) setTask({ id, state: 'running', progress: { statements_done: 0, statements_total: 0 } } as TaskStatus)
      else {
        message.success(t('model.sync.written'))
        onDone()
      }
    } catch (e) {
      message.error((e as Error).message)
    } finally {
      setBusy(false)
    }
  }

  const script = p?.script
  const blocked = !p || !!p.denied || !!script?.refused.length || (script ? !script.steps.length : !p.writes.length)
  const running = task?.state === 'running'
  return (
    <Modal
      open
      width={820}
      title={t('model.sync.title')}
      onCancel={onClose}
      okText={script ? t('model.sync.execute') : t('model.sync.write')}
      okButtonProps={{ disabled: blocked || running || !!task, loading: busy || running, danger: !!p?.confirm.length }}
      onOk={() => void run()}
    >
      {error ? (
        <Alert type="error" message={(error as Error).message} />
      ) : !p || isFetching ? (
        <Spin />
      ) : (
        <div className="design-save">
          {p.denied && <Alert type="error" message={t('model.sync.denied', { reason: p.denied })} />}
          {script?.refused.map(([table, reason]) => (
            <Alert key={table} type="error" message={t('model.sync.refused', { table, reason })} />
          ))}
          {p.confirm.map((c) => (
            <Alert key={c} type="warning" showIcon message={c} />
          ))}
          {script?.warnings.map((w) => (
            <Alert key={w} type="warning" showIcon message={w} />
          ))}
          {script ? (
            <>
              <Alert type="info" showIcon message={t(script.transactional ? 'design.sql.transactional' : 'design.sql.autocommit')} />
              <SqlBox sql={p.text} driver={driver} height={360} />
            </>
          ) : (
            <ul className="design-changes">
              {p.writes.map((w) => (
                <li key={w.name} className={w.delete ? 'err' : undefined}>
                  {w.delete
                    ? t('model.sync.deleteFile', { name: w.name })
                    : w.from
                      ? t('model.sync.renameFile', { from: w.from, name: w.name })
                      : t('model.sync.writeFile', { name: w.name })}
                </li>
              ))}
            </ul>
          )}
          {running && (
            <Progress
              size="small"
              percent={Math.round((100 * task.progress.statements_done) / Math.max(1, task.progress.statements_total))}
            />
          )}
          {task && !running && task.state !== 'done' && (
            <Alert
              type={task.state === 'cancelled' ? 'warning' : 'error'}
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
                task.remaining && profileId ? (
                  <Button size="small" onClick={() => openSql(task.remaining ?? '', profileId)}>
                    {t('design.run.remaining')}
                  </Button>
                ) : undefined
              }
            />
          )}
          {running && (
            <Button size="small" onClick={() => void api.cancelTask(task.id).catch((e) => message.error((e as Error).message))}>
              {t('design.run.cancel')}
            </Button>
          )}
        </div>
      )}
    </Modal>
  )
}
```

- [ ] **Step 14: 新建 `web/src/features/compare/logic.ts`**

```ts
// 比对标签的纯逻辑（M4a 设计文档第 7 节）：默认勾选、依赖提示、变更的摘要、两边定义的文本对照、采用推测的改名。

import type {
  CompareRenames,
  CompareResult,
  DesignTableModel,
  RenameSuggestion,
  SchemaChange,
  TableDiff,
  TableStatus,
} from '../../shared/api'
import { t } from '../../shared/i18n'
import { describeChange } from '../design/summary'

/** 删除表、删除列：会丢数据，默认不勾选，勾选后标红。 */
export function isDangerous(c: SchemaChange): boolean {
  return c.kind === 'drop_table' || (c.kind === 'table' && c.change.kind === 'drop_column')
}

/** 默认勾选：除删除表、删除列以外的全部变更。 */
export function defaultSelection(tables: TableDiff[]): number[] {
  return tables.flatMap((d) => d.items.filter((i) => !isDangerous(i.change)).map((i) => i.id))
}

export function describeItem(c: SchemaChange): string {
  if (c.kind === 'drop_table') return t('model.compare.change.dropTable', { name: c.name })
  if (c.kind === 'rename_table') return t('model.compare.change.renameTable', { from: c.from, to: c.to })
  return describeChange(c.change).join('；')
}

/** 表的显示名：两边名字不同时写成"目标 → 源"。 */
export function tableLabel(d: TableDiff): string {
  if (d.source && d.target && d.source !== d.target) return `${d.target} → ${d.source}`
  return d.source ?? d.target ?? ''
}

export const STATUSES: TableStatus[] = ['added', 'removed', 'renamed', 'changed', 'same']

/** 勾选的组合中缺了依赖的变更：加外键引用的新表没有勾选建表；删除的表还被其他表的外键引用着。 */
export function dependencyIssues(r: CompareResult, selected: ReadonlySet<number>): string[] {
  const out = new Set<string>()
  const tables = r.diff.tables
  const created = (name: string) => {
    const d = tables.find((x) => x.status === 'added' && x.source === name)
    return !d || d.items.every((i) => selected.has(i.id))
  }
  for (const d of tables) {
    for (const i of d.items) {
      if (!selected.has(i.id) || i.change.kind !== 'table') continue
      const c = i.change.change
      const refs =
        c.kind === 'add_foreign_key'
          ? [c.key]
          : c.kind === 'create_table'
            ? c.table.foreign_keys
            : []
      for (const k of refs) {
        if (!k.ref_schema && k.ref_table !== d.source && !created(k.ref_table)) {
          out.add(t('model.compare.dep.create', { table: d.source ?? '', ref: k.ref_table }))
        }
      }
    }
  }
  // 要删除的表（目标中的名字）
  const dropped = new Set(
    tables.flatMap((d) => d.items.filter((i) => selected.has(i.id) && i.change.kind === 'drop_table').map(() => d.target ?? '')),
  )
  for (const x of dropped) {
    for (const tt of r.target_tables) {
      if (dropped.has(tt.name)) continue
      const d = tables.find((y) => y.target === tt.name)
      for (const k of tt.foreign_keys) {
        if (k.ref_schema || k.ref_table !== x) continue
        const droppedFk = d?.items.some(
          (i) =>
            selected.has(i.id) &&
            i.change.kind === 'table' &&
            i.change.change.kind === 'drop_foreign_key' &&
            i.change.change.key.name === k.name,
        )
        if (!droppedFk) out.add(t('model.compare.dep.drop', { table: x, by: tt.name }))
      }
    }
  }
  return [...out]
}

const cols = (c: string[]) => `(${c.join(', ')})`

/** 表定义的文本（右侧两边对照）：每列、每个约束、每个索引一行。 */
export function tableText(tb: DesignTableModel | undefined): string[] {
  if (!tb) return []
  const lines: string[] = []
  if (tb.comment) lines.push(`-- ${tb.comment}`)
  for (const c of tb.columns) {
    let s = `${c.name} ${c.data_type.native}`
    if (!c.nullable) s += ' NOT NULL'
    if (c.default) s += ` DEFAULT ${c.default}`
    if (c.auto_increment) s += ' AUTO'
    if (c.generated) s += ` AS (${c.generated})`
    if (c.comment) s += ` -- ${c.comment}`
    lines.push(s)
  }
  const pk = tb.primary_key
  if (pk) lines.push(`PRIMARY KEY ${pk.name ?? ''} ${cols(pk.columns)}`.replace('  ', ' '))
  for (const u of tb.uniques) lines.push(`UNIQUE ${u.name ?? ''} ${cols(u.columns)}`.replace('  ', ' '))
  for (const k of tb.foreign_keys) {
    const ref = `${k.ref_schema ? `${k.ref_schema}.` : ''}${k.ref_table}${cols(k.ref_columns)}`
    let s = `FOREIGN KEY ${k.name ?? ''} ${cols(k.columns)} → ${ref}`.replace('  ', ' ')
    if (k.on_delete) s += ` ON DELETE ${k.on_delete}`
    if (k.on_update) s += ` ON UPDATE ${k.on_update}`
    lines.push(s)
  }
  for (const c of tb.checks) lines.push(`CHECK ${c.name ?? ''} (${c.expression})`.replace('  ', ' '))
  for (const x of tb.indexes) {
    const c = x.columns.map((i) => (i.descending ? `${i.name} DESC` : i.name))
    lines.push(`${x.unique ? 'UNIQUE INDEX' : 'INDEX'} ${x.name} ${cols(c)}${x.method ? ` USING ${x.method}` : ''}`)
  }
  return lines
}

/** 两边的行：另一边没有的行标为不同。 */
export function compareLines(a: string[], b: string[]): { text: string; diff: boolean }[][] {
  const sa = new Set(a)
  const sb = new Set(b)
  return [a.map((text) => ({ text, diff: !sb.has(text) })), b.map((text) => ({ text, diff: !sa.has(text) }))]
}

/** 采用一条推测的改名。 */
export function acceptSuggestion(r: CompareRenames, s: RenameSuggestion): CompareRenames {
  if (s.table === null) return { ...r, tables: { ...r.tables, [s.source]: s.target } }
  return { ...r, columns: { ...r.columns, [s.table]: { ...(r.columns[s.table] ?? {}), [s.source]: s.target } } }
}

export const EMPTY_RENAMES: CompareRenames = { tables: {}, columns: {} }
```

- [ ] **Step 15: 修改 `web/src/features/design/DesignView.tsx`**

```diff
--- a/web/src/features/design/DesignView.tsx
+++ b/web/src/features/design/DesignView.tsx
@@ -1,8 +1,10 @@
 // 表设计器标签（M3 设计文档第 7 节）：子标签分列、索引、外键、约束、选项、SQL 预览；改动后约 300 毫秒自动预览，
 // 校验错误在子标签上显示数目、出错的单元格标红。Ctrl+S 或"保存"打开确认框，执行时进度显示在底部，可以取消。
+// 设计模型中的表（M4a 设计文档 4.3 节）：类型目录和能力按模型的数据库类型，保存只写这张表的文件、不弹确认框；
+// "SQL 预览"换成变更列表和"与库比对"的入口。
 // 引入 Monaco（SQL 预览），由工作区懒加载。
 
-import { IconDeviceFloppy, IconRefresh, IconTrashX } from '@tabler/icons-react'
+import { IconArrowsDiff, IconDeviceFloppy, IconRefresh, IconTrashX } from '@tabler/icons-react'
 import { keepPreviousData, useQuery, useQueryClient } from '@tanstack/react-query'
 import { Alert, App, Badge, Button, Progress, Spin, Tabs } from 'antd'
 import { type KeyboardEvent, useCallback, useEffect, useMemo, useState } from 'react'
@@ -10,7 +12,7 @@ import { useTranslation } from 'react-i18next'
 
 import { useApi } from '../../app/context'
 import { useUi } from '../../app/ui'
-import { type DesignRequest, type Profile, type TaskStatus, errorText } from '../../shared/api'
+import { type DesignRequest, type ModelDesignRequest, type Profile, type TaskStatus, errorText } from '../../shared/api'
 import { withVault } from '../connections/vault'
 import { useQueryActions } from '../editor/actions'
 import { type QueryTab, useTabs } from '../editor/tabs'
@@ -22,8 +24,9 @@ import IndexesPane from './IndexesPane'
 import { type Draft, type Pane, emptyDraft, fromTable, isDirty, issuesOf, renamesOf, toModel } from './logic'
 import OptionsPane from './OptionsPane'
 import SaveDialog from './SaveDialog'
+import { modelSource, profileSource } from './source'
 import SqlBox from './SqlBox'
-import { scriptOf } from './summary'
+import { describeChange, scriptOf } from './summary'
 
 const NO_ISSUES = issuesOf([], emptyDraft())
 
@@ -36,6 +39,19 @@ export default function DesignView({ tab, profiles }: { tab: QueryTab; profiles:
   const profile = profiles.find((p) => p.id === tab.profileId)
   const target = tab.design!
   const isNew = target.table === null
+  const modelPath = target.model
+  // 模型按目录打开（已经打开的返回原来的编号）
+  const modelQ = useQuery({
+    queryKey: ['model-open', modelPath],
+    queryFn: () => api.openModel(modelPath!),
+    enabled: !!modelPath,
+    staleTime: Infinity,
+  })
+  const modelId = modelQ.data?.id
+  const source = useMemo(
+    () => (modelPath ? modelSource(api, modelId ?? '') : profileSource(api, tab.profileId)),
+    [api, modelPath, modelId, tab.profileId],
+  )
   const state = useDesigns((s) => s.tabs[tab.id])
   const [schema, setSchema] = useState(target.schema)
   const [pane, setPane] = useState<Pane | 'sql'>('columns')
@@ -45,9 +61,9 @@ export default function DesignView({ tab, profiles }: { tab: QueryTab; profiles:
   const [loadError, setLoadError] = useState<string | null>(null)
 
   const caps = useQuery({
-    queryKey: ['design-caps', tab.profileId],
-    queryFn: () => withVault(api, () => api.designCaps(tab.profileId)),
-    enabled: !!profile,
+    queryKey: ['design-caps', source.key],
+    queryFn: () => (modelPath ? api.modelCaps(modelId!) : withVault(api, () => api.designCaps(tab.profileId))),
+    enabled: modelPath ? !!modelId : !!profile,
     staleTime: Infinity,
   })
 
@@ -57,8 +73,11 @@ export default function DesignView({ tab, profiles }: { tab: QueryTab; profiles:
       useDesigns.getState().set(tab.id, { original: null, fingerprint: null, storage: null, draft: emptyDraft(), rebuild: false })
       return
     }
+    if (modelPath && !modelId) return
     try {
-      const r = await withVault(api, () => api.designTable(tab.profileId, target.schema, target.table!))
+      const r = modelPath
+        ? { ...(await api.modelTable(modelId!, target.schema, target.table)), storage: null }
+        : await withVault(api, () => api.designTable(tab.profileId, target.schema, target.table!))
       useDesigns.getState().set(tab.id, {
         original: fromTable(r.table),
         fingerprint: r.fingerprint,
@@ -69,7 +88,7 @@ export default function DesignView({ tab, profiles }: { tab: QueryTab; profiles:
     } catch (e) {
       setLoadError((e as Error).message)
     }
-  }, [api, tab.id, tab.profileId, target.schema, target.table])
+  }, [api, tab.id, tab.profileId, target.schema, target.table, modelPath, modelId])
 
   // 第一次打开、新建的表保存后转为编辑这张表时读取
   useEffect(() => {
@@ -101,13 +120,28 @@ export default function DesignView({ tab, profiles }: { tab: QueryTab; profiles:
   const preview = useQuery({
     queryKey: ['design-preview', tab.id, JSON.stringify(debounced)],
     queryFn: () => api.previewDesign(tab.profileId, debounced!),
-    enabled: !!profile && !!debounced && dirty && !!debounced.schema,
+    enabled: !modelPath && !!profile && !!debounced && dirty && !!debounced.schema,
+    placeholderData: keepPreviousData,
+    retry: false,
+  })
+  const modelRequest = (r: DesignRequest): ModelDesignRequest => ({
+    schema: r.schema,
+    name: r.name,
+    draft: r.draft,
+    renames: r.renames,
+    fingerprint: r.fingerprint,
+  })
+  const modelPreview = useQuery({
+    queryKey: ['model-design-preview', tab.id, JSON.stringify(debounced)],
+    queryFn: () => api.previewModelTable(modelId!, modelRequest(debounced!)),
+    enabled: !!modelId && !!debounced && dirty && !!debounced.schema,
     placeholderData: keepPreviousData,
     retry: false,
   })
+  const errors = modelPath ? modelPreview.data?.errors : preview.data?.errors
   const issues = useMemo(
-    () => (state && preview.data && dirty ? issuesOf(preview.data.errors, state.draft) : NO_ISSUES),
-    [state, preview.data, dirty],
+    () => (state && errors && dirty ? issuesOf(errors, state.draft) : NO_ISSUES),
+    [state, errors, dirty],
   )
 
   // 执行中：轮询任务状态
@@ -138,13 +172,31 @@ export default function DesignView({ tab, profiles }: { tab: QueryTab; profiles:
     } else void load()
   }, [task?.state])
 
+  /** 模型：直接写这张表的文件；新建的表保存后转为编辑这张表。 */
+  const saveModel = async (req: DesignRequest) => {
+    try {
+      await api.saveModelTable(modelId!, modelRequest(req))
+      message.success(t('model.design.saved'))
+      void qc.invalidateQueries({ queryKey: ['model-tables', modelId] })
+      void qc.invalidateQueries({ queryKey: ['design-ref-tables', source.key] })
+      void qc.invalidateQueries({ queryKey: ['ref-columns', source.key] })
+      if (isNew) {
+        useDesigns.getState().clear(tab.id)
+        useTabs.getState().setDesign(tab.id, { schema: req.schema, table: req.draft.name.trim(), model: modelPath })
+      } else void load()
+    } catch (e) {
+      message.error((e as Error).message)
+    }
+  }
+
   const save = () => {
     if (!dirty || !request || task?.state === 'running') return
     if (!request.schema) {
       setPane('options')
       return
     }
-    setSaving(true)
+    if (modelPath) void saveModel(request)
+    else setSaving(true)
   }
   const onKeyDown = (e: KeyboardEvent) => {
     if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 's') {
@@ -155,8 +207,9 @@ export default function DesignView({ tab, profiles }: { tab: QueryTab; profiles:
   const confirmThen = (title: string, fn: () => void) =>
     dirty ? modal.confirm({ title, okType: 'danger', onOk: fn }) : fn()
 
+  if (modelQ.error) return <Alert type="error" style={{ margin: 12 }} message={(modelQ.error as Error).message} />
   if (loadError) return <Alert type="error" style={{ margin: 12 }} message={loadError} action={<Button onClick={() => void load()}>{t('design.toolbar.reload')}</Button>} />
-  if (!state || !profile) return <Spin style={{ margin: 48 }} />
+  if (!state || (modelPath ? !modelQ.data : !profile)) return <Spin style={{ margin: 48 }} />
 
   const count = (p: Pane) => issues.counts[p] ?? 0
   const label = (p: Pane, text: string) => (
@@ -166,11 +219,18 @@ export default function DesignView({ tab, profiles }: { tab: QueryTab; profiles:
   )
   const running = task?.state === 'running'
   const p = preview.data
+  const mp = modelPreview.data
+  const compareWithDb = () =>
+    useTabs.getState().openCompare({ source: { kind: 'model', path: modelPath!, schema: request?.schema ?? target.schema }, target: null })
   return (
     <div className="design-view" onKeyDown={onKeyDown} tabIndex={-1}>
       <div className="row toolbar">
-        <b>{isNew ? t('design.newTable') : `${target.schema}.${target.table}`}</b>
-        <span className="muted">{profile.name}</span>
+        <b>
+          {modelQ.data && `${modelQ.data.info.name} / `}
+          {isNew ? t('design.newTable') : `${target.schema}.${target.table}`}
+        </b>
+        {profile && <span className="muted">{profile.name}</span>}
+        {modelQ.data?.read_only && <span className="err">{t('model.readOnly')}</span>}
         <span className="grow" />
         <Button size="small" type="text" icon={<IconTrashX size={16} />} disabled={!dirty || running} onClick={() => confirmThen(t('design.toolbar.discardConfirm'), () => void load())}>
           {t('design.toolbar.discard')}
@@ -180,7 +240,12 @@ export default function DesignView({ tab, profiles }: { tab: QueryTab; profiles:
             {t('design.toolbar.reload')}
           </Button>
         )}
-        <Button size="small" type="primary" icon={<IconDeviceFloppy size={16} />} disabled={!dirty || running} onClick={save}>
+        {modelPath && !isNew && (
+          <Button size="small" type="text" icon={<IconArrowsDiff size={16} />} onClick={compareWithDb}>
+            {t('model.design.compare')}
+          </Button>
+        )}
+        <Button size="small" type="primary" icon={<IconDeviceFloppy size={16} />} disabled={!dirty || running || !!modelQ.data?.read_only} onClick={save}>
           {t('design.toolbar.save')}
         </Button>
       </div>
@@ -206,7 +271,7 @@ export default function DesignView({ tab, profiles }: { tab: QueryTab; profiles:
             key: 'foreignKeys',
             label: label('foreignKeys', t('design.tabs.foreignKeys')),
             children: (
-              <ForeignKeysPane profileId={tab.profileId} schema={request?.schema ?? ''} draft={state.draft} original={state.original} caps={caps.data} issues={issues} onChange={edit} />
+              <ForeignKeysPane source={source} schema={request?.schema ?? ''} draft={state.draft} original={state.original} caps={caps.data} issues={issues} onChange={edit} />
             ),
           },
           {
@@ -219,7 +284,7 @@ export default function DesignView({ tab, profiles }: { tab: QueryTab; profiles:
             label: label('options', t('design.tabs.options')),
             children: (
               <OptionsPane
-                profileId={tab.profileId}
+                source={source}
                 schema={request?.schema ?? ''}
                 isNew={isNew}
                 draft={state.draft}
@@ -230,7 +295,32 @@ export default function DesignView({ tab, profiles }: { tab: QueryTab; profiles:
               />
             ),
           },
-          {
+          modelPath ? {
+            key: 'sql',
+            label: t('model.design.changes'),
+            children: (
+              <div className="design-pane design-sql">
+                {!dirty ? (
+                  <span className="muted">{t('design.sql.nothing')}</span>
+                ) : !mp ? (
+                  <Spin />
+                ) : (
+                  <>
+                    {mp.conflict && <Alert type="error" message={t('model.design.conflict')} action={<Button size="small" onClick={() => void load()}>{t('design.toolbar.reload')}</Button>} />}
+                    {mp.errors.length > 0 && <Alert type="error" message={t('design.sql.errors', { count: mp.errors.length })} />}
+                    {mp.warnings.map((w) => (
+                      <Alert key={w} type="warning" showIcon message={w} />
+                    ))}
+                    <ul className="design-changes">
+                      {mp.changes.flatMap((c) => describeChange(c)).map((line, i) => (
+                        <li key={i}>{line}</li>
+                      ))}
+                    </ul>
+                  </>
+                )}
+              </div>
+            ),
+          } : {
             key: 'sql',
             label: t('design.tabs.sql'),
             children: (
@@ -252,7 +342,7 @@ export default function DesignView({ tab, profiles }: { tab: QueryTab; profiles:
                     {p.statements.length > 0 && (
                       <Alert type="info" showIcon message={t(p.transactional ? 'design.sql.transactional' : 'design.sql.autocommit')} />
                     )}
-                    <SqlBox sql={scriptOf(p.statements, profile.driver === 'oracle' || profile.driver === 'dameng')} driver={profile.driver} height="calc(100vh - 360px)" />
+                    <SqlBox sql={scriptOf(p.statements, profile!.driver === 'oracle' || profile!.driver === 'dameng')} driver={profile!.driver} height="calc(100vh - 360px)" />
                   </>
                 )}
               </div>
@@ -303,7 +393,7 @@ export default function DesignView({ tab, profiles }: { tab: QueryTab; profiles:
           )}
         </div>
       )}
-      {saving && request && (
+      {saving && request && profile && (
         <SaveDialog
           profile={profile}
           request={request}
```

- [ ] **Step 16: 修改 `web/src/features/design/ForeignKeysPane.tsx`**

```diff
--- a/web/src/features/design/ForeignKeysPane.tsx
+++ b/web/src/features/design/ForeignKeysPane.tsx
@@ -4,24 +4,23 @@ import { useQuery } from '@tanstack/react-query'
 import { Input, Select } from 'antd'
 import { useTranslation } from 'react-i18next'
 
-import { useApi } from '../../app/context'
 import type { DesignCaps } from '../../shared/api'
 import { Cell } from './ColumnsPane'
 import ListPane from './ListPane'
 import { type Draft, type DraftForeignKey, type Issues, newKey, updateItem } from './logic'
+import type { RefSource } from './source'
 
 /** 引用的表的列（选了表之后才查）。 */
-function RefColumns({ profileId, schema, fk, onChange }: {
-  profileId: string
+function RefColumns({ source, schema, fk, onChange }: {
+  source: RefSource
   schema: string
   fk: DraftForeignKey
   onChange(cols: string[]): void
 }) {
-  const api = useApi()
   const refSchema = fk.ref_schema || schema
   const { data } = useQuery({
-    queryKey: ['table-info', profileId, refSchema, fk.ref_table],
-    queryFn: () => api.table(profileId, refSchema, fk.ref_table),
+    queryKey: ['ref-columns', source.key, refSchema, fk.ref_table],
+    queryFn: () => source.columns(refSchema, fk.ref_table),
     enabled: !!fk.ref_table,
   })
   return (
@@ -32,14 +31,14 @@ function RefColumns({ profileId, schema, fk, onChange }: {
       style={{ width: '100%' }}
       value={fk.ref_columns}
       disabled={fk.deleted}
-      options={(data?.columns ?? []).map((c) => ({ value: c.name }))}
+      options={(data ?? []).map((c) => ({ value: c }))}
       onChange={onChange}
     />
   )
 }
 
-export default function ForeignKeysPane({ profileId, schema, draft, original, caps, issues, onChange }: {
-  profileId: string
+export default function ForeignKeysPane({ source, schema, draft, original, caps, issues, onChange }: {
+  source: RefSource
   schema: string
   draft: Draft
   original: Draft | null
@@ -48,13 +47,12 @@ export default function ForeignKeysPane({ profileId, schema, draft, original, ca
   onChange(fn: (d: Draft) => Draft): void
 }) {
   const { t } = useTranslation()
-  const api = useApi()
-  const { data: schemas = [] } = useQuery({ queryKey: ['schemas', profileId], queryFn: () => api.schemas(profileId) })
+  const { data: schemas = [] } = useQuery({ queryKey: ['ref-schemas', source.key], queryFn: () => source.schemas() })
   const refSchemas = [...new Set(draft.foreignKeys.map((f) => f.ref_schema || schema))]
   const tables = useQuery({
-    queryKey: ['design-ref-tables', profileId, refSchemas],
+    queryKey: ['design-ref-tables', source.key, refSchemas],
     queryFn: async () =>
-      Object.fromEntries(await Promise.all(refSchemas.map(async (s) => [s, (await api.objects(profileId, s, 'tables')).map((o) => o.name)] as const))),
+      Object.fromEntries(await Promise.all(refSchemas.map(async (s) => [s, await source.tables(s)] as const))),
     enabled: !!schema,
   })
   const names = draft.columns.filter((c) => !c.deleted && c.name).map((c) => ({ value: c.name }))
@@ -110,7 +108,7 @@ export default function ForeignKeysPane({ profileId, schema, draft, original, ca
               value={f.ref_schema || schema}
               disabled={f.deleted}
               showSearch
-              options={schemas.map((s) => ({ value: s.name }))}
+              options={schemas.map((s) => ({ value: s }))}
               onChange={(v: string) => set(f.key, { ref_schema: v === schema ? null : v, ref_table: '', ref_columns: [] })}
             />
           ),
@@ -138,7 +136,7 @@ export default function ForeignKeysPane({ profileId, schema, draft, original, ca
           width: 170,
           render: (_, f) => (
             <Cell error={issues.at(f.key, 'ref_columns')}>
-              <RefColumns profileId={profileId} schema={schema} fk={f} onChange={(ref_columns) => set(f.key, { ref_columns })} />
+              <RefColumns source={source} schema={schema} fk={f} onChange={(ref_columns) => set(f.key, { ref_columns })} />
             </Cell>
           ),
         },
```

- [ ] **Step 17: 修改 `web/src/features/design/OptionsPane.tsx`**

```diff
--- a/web/src/features/design/OptionsPane.tsx
+++ b/web/src/features/design/OptionsPane.tsx
@@ -5,13 +5,13 @@ import { useQuery } from '@tanstack/react-query'
 import { Button, Input, Select } from 'antd'
 import { useTranslation } from 'react-i18next'
 
-import { useApi } from '../../app/context'
 import type { DesignTableResult } from '../../shared/api'
 import { Cell } from './ColumnsPane'
 import { type Draft, type Issues, moveItem } from './logic'
+import type { RefSource } from './source'
 
-export default function OptionsPane({ profileId, schema, isNew, draft, storage, issues, onSchema, onChange }: {
-  profileId: string
+export default function OptionsPane({ source, schema, isNew, draft, storage, issues, onSchema, onChange }: {
+  source: RefSource
   schema: string
   isNew: boolean
   draft: Draft
@@ -21,8 +21,7 @@ export default function OptionsPane({ profileId, schema, isNew, draft, storage,
   onChange(fn: (d: Draft) => Draft): void
 }) {
   const { t } = useTranslation()
-  const api = useApi()
-  const { data: schemas = [] } = useQuery({ queryKey: ['schemas', profileId], queryFn: () => api.schemas(profileId), enabled: isNew })
+  const { data: schemas = [] } = useQuery({ queryKey: ['ref-schemas', source.key], queryFn: () => source.schemas(), enabled: isNew })
   const pk = draft.primaryKey
   const props = [
     storage?.engine && t('design.option.engine', { value: storage.engine }),
@@ -37,7 +36,7 @@ export default function OptionsPane({ profileId, schema, isNew, draft, storage,
         {t('design.option.schema')}
         {isNew ? (
           <Cell error={schema ? undefined : t('design.option.schemaRequired')}>
-            <Select size="small" showSearch value={schema || undefined} options={schemas.map((s) => ({ value: s.name }))} onChange={onSchema} />
+            <Select size="small" showSearch value={schema || undefined} options={schemas.map((s) => ({ value: s }))} onChange={onSchema} />
           </Cell>
         ) : (
           <Input size="small" value={schema} disabled />
```

- [ ] **Step 18: 新建 `web/src/features/design/source.ts`**

```ts
// 设计器选外键引用、选 schema 时的数据来源：连接的元数据，或模型中的表（M4a）。

import type { Api } from '../../shared/api'

export interface RefSource {
  /** 查询缓存的键：连接为连接 id，模型为 `model:<编号>`。 */
  key: string
  schemas(): Promise<string[]>
  tables(schema: string): Promise<string[]>
  columns(schema: string, table: string): Promise<string[]>
}

export function profileSource(api: Api, profileId: string): RefSource {
  return {
    key: profileId,
    schemas: async () => (await api.schemas(profileId)).map((s) => s.name),
    tables: async (schema) => (await api.objects(profileId, schema, 'tables')).map((o) => o.name),
    columns: async (schema, table) => (await api.table(profileId, schema, table))?.columns.map((c) => c.name) ?? [],
  }
}

/** 模型：schema 为模型信息中的和已有表文件的；文件有错的表不列出。 */
export function modelSource(api: Api, modelId: string): RefSource {
  return {
    key: `model:${modelId}`,
    schemas: async () => {
      const [m, tables] = await Promise.all([api.model(modelId), api.modelTables(modelId)])
      return [...new Set([...m.info.schemas, ...tables.map((t) => t.schema)])]
    },
    tables: async (schema) =>
      (await api.modelTables(modelId)).filter((t) => t.schema === schema && !t.error).map((t) => t.name),
    columns: async (schema, table) => {
      try {
        return (await api.modelTable(modelId, schema, table)).table.columns.map((c) => c.name)
      } catch {
        return []
      }
    },
  }
}
```

- [ ] **Step 19: 修改 `web/src/features/editor/Workspace.tsx`**

```diff
--- a/web/src/features/editor/Workspace.tsx
+++ b/web/src/features/editor/Workspace.tsx
@@ -3,7 +3,7 @@
 // 标签多时先缩窄（悬停看全名），再放不下就用滚轮横向滚动，不显示滚动条；右端 ▾ 列出全部标签；右键菜单批量关闭。
 
 import { Dropdown, type MenuProps } from 'antd'
-import { IconChevronDown, IconFileCode, IconPlus, IconTable, IconTableOptions, IconX } from '@tabler/icons-react'
+import { IconArrowsDiff, IconChevronDown, IconFileCode, IconPlus, IconSchema, IconTable, IconTableOptions, IconX } from '@tabler/icons-react'
 import { useQuery } from '@tanstack/react-query'
 import { Spin } from 'antd'
 import { Suspense, type WheelEvent, lazy, useEffect, useRef } from 'react'
@@ -21,6 +21,8 @@ import { type QueryTab, useTabs } from './tabs'
 
 // 设计器带 Monaco 的 SQL 预览，打开第一个设计器标签时才加载
 const DesignView = lazy(() => import('../design/DesignView'))
+// 比对标签的同步预览带 Monaco，同样懒加载
+const CompareView = lazy(() => import('../compare/CompareView'))
 
 function TabTitle({ tab, active, envClass, profileName, profile }: {
   tab: QueryTab
@@ -62,7 +64,11 @@ function TabTitle({ tab, active, envClass, profileName, profile }: {
         }}
         title={`${tab.title} · ${profileName}`}
       >
-        {tab.design ? (
+        {tab.compare ? (
+          <IconArrowsDiff size={16} />
+        ) : tab.design?.model ? (
+          <IconSchema size={16} />
+        ) : tab.design ? (
           <IconTableOptions size={16} />
         ) : tab.data ? (
           <IconTable size={16} />
@@ -117,7 +123,8 @@ export default function Workspace() {
     if (strip.current && e.deltaX === 0) strip.current.scrollLeft += e.deltaY
   }
 
-  const profileName = (id: string) => profiles.find((x) => x.id === id)?.name ?? t('editor.tab.profileDeleted')
+  // 模型的设计器标签、比对标签不绑定连接
+  const profileName = (id: string) => (id ? profiles.find((x) => x.id === id)?.name ?? t('editor.tab.profileDeleted') : '')
   const list: MenuProps = {
     items: tabs.map((tb) => ({ key: tb.id, label: `${tb.title} · ${profileName(tb.profileId)}` })),
     selectable: true,
@@ -158,7 +165,11 @@ export default function Workspace() {
         )}
       </div>
       {tab &&
-        (tab.design ? (
+        (tab.compare ? (
+          <Suspense fallback={<Spin style={{ margin: 48 }} />}>
+            <CompareView key={tab.id} tab={tab} profiles={profiles} />
+          </Suspense>
+        ) : tab.design ? (
           <Suspense fallback={<Spin style={{ margin: 48 }} />}>
             <DesignView key={tab.id} tab={tab} profiles={profiles} />
           </Suspense>
```

- [ ] **Step 20: 修改 `web/src/features/editor/actions.tsx`**

```diff
--- a/web/src/features/editor/actions.tsx
+++ b/web/src/features/editor/actions.tsx
@@ -24,7 +24,7 @@ export function useQueryActions() {
     const id =
       profileId ??
       useUi.getState().selectedProfile ??
-      tabs.find((x) => x.id === active)?.profileId ??
+      (tabs.find((x) => x.id === active)?.profileId || undefined) ??
       profiles?.[0]?.id
     if (id) useTabs.getState().open(id)
     else useUi.getState().openConnection(null)
@@ -49,7 +49,7 @@ export function useQueryActions() {
   const insertSql = (sql: string, profileId?: string) => {
     const { tabs, active } = useTabs.getState()
     const tab = tabs.find((x) => x.id === active)
-    if (tab && !tab.data && !tab.design) sendInsert(sql)
+    if (tab && !tab.data && !tab.design && !tab.compare) sendInsert(sql)
     else openSql(sql, profileId)
   }
 
```

- [ ] **Step 21: 修改 `web/src/features/editor/tabs.ts`**

```diff
--- a/web/src/features/editor/tabs.ts
+++ b/web/src/features/editor/tabs.ts
@@ -2,6 +2,8 @@
 // （ui.tabs），下次启动时恢复；会话不恢复，第一次执行时再打开。
 // 数据标签（M1c2 设计文档第 5 节）：带 data 的标签，只读地浏览一张表 / 视图 / 物化视图，同一对象只开一个。
 // 设计器标签（M3 设计文档 7.1 节）：带 design 的标签，设计一张已有的表（同一张表只开一个）或新建表；草稿不保存。
+// 设计模型中的表（M4a）时 design.model 是模型目录，标签不绑定连接（profileId 为空串）。
+// 比对标签（M4a 设计文档第 7 节）：带 compare 的标签，两边是模型或连接的一个 schema；也不绑定连接。
 
 import { create } from 'zustand'
 
@@ -21,10 +23,21 @@ export interface DataTarget {
   order?: { column: string; desc: boolean } | null
 }
 
-/** 设计器标签设计的表：`table` 为 null 时是新建表（`schema` 为空串时由用户在"选项"中选）。 */
+/** 设计器标签设计的表：`table` 为 null 时是新建表（`schema` 为空串时由用户在"选项"中选）。
+ * `model` 是模型目录：有值时设计模型中的表。 */
 export interface DesignTarget {
   schema: string
   table: string | null
+  model?: string
+}
+
+/** 比对的一边：模型（按目录）或连接的一个 schema。 */
+export type CompareSide = { kind: 'model'; path: string; schema: string } | { kind: 'database'; profile: string; schema: string }
+
+/** 比对标签的两边（还没选时为 null）。 */
+export interface CompareTarget {
+  source: CompareSide | null
+  target: CompareSide | null
 }
 
 export interface QueryTab {
@@ -37,6 +50,8 @@ export interface QueryTab {
   data?: DataTarget
   /** 有值时是设计器标签。 */
   design?: DesignTarget
+  /** 有值时是比对标签。 */
+  compare?: CompareTarget
 }
 
 const TARGET_KINDS: readonly unknown[] = ['table', 'view', 'materialized_view'] satisfies TargetKind[]
@@ -55,7 +70,23 @@ function isDataTarget(v: unknown): v is DataTarget {
 
 function isDesignTarget(v: unknown): v is DesignTarget {
   const d = v as DesignTarget | null
-  return typeof d?.schema === 'string' && (d.table === null || typeof d.table === 'string')
+  return (
+    typeof d?.schema === 'string' &&
+    (d.table === null || typeof d.table === 'string') &&
+    (d.model === undefined || typeof d.model === 'string')
+  )
+}
+
+function isCompareSide(v: unknown): v is CompareSide | null {
+  if (v === null) return true
+  const s = v as CompareSide | undefined
+  if (typeof s?.schema !== 'string') return false
+  return s.kind === 'model' ? typeof s.path === 'string' : s.kind === 'database' && typeof s.profile === 'string'
+}
+
+function isCompareTarget(v: unknown): v is CompareTarget {
+  const c = v as CompareTarget | null
+  return !!c && isCompareSide(c.source) && isCompareSide(c.target)
 }
 
 /** 是否同一个对象的数据标签。 */
@@ -88,7 +119,8 @@ export function parseTabs(raw: string | undefined): Persisted {
         typeof t.profileId === 'string' &&
         typeof t.sql === 'string' &&
         (t.data === undefined || isDataTarget(t.data)) &&
-        (t.design === undefined || isDesignTarget(t.design)),
+        (t.design === undefined || isDesignTarget(t.design)) &&
+        (t.compare === undefined || isCompareTarget(t.compare)),
     )
     const active = tabs.some((t) => t.id === v.active) ? (v.active as string) : (tabs[0]?.id ?? null)
     return { tabs, active, seq: typeof v.seq === 'number' && v.seq > 0 ? v.seq : tabs.length + 1 }
@@ -106,6 +138,10 @@ interface TabsState extends Persisted {
   openDesign(profileId: string, target: DesignTarget): string
   /** 新建的表保存后，标签转为设计这张表。 */
   setDesign(id: string, target: DesignTarget): void
+  /** 新开比对标签，返回它的 id。 */
+  openCompare(target: CompareTarget): string
+  /** 修改比对标签的两边。 */
+  setCompare(id: string, target: CompareTarget): void
   /** 修改数据标签的过滤条件和排序。 */
   setData(id: string, patch: Pick<DataTarget, 'filter' | 'order'>): void
   close(id: string): void
@@ -144,7 +180,13 @@ export const useTabs = create<TabsState>()((set, get) => ({
     const { tabs } = get()
     const same =
       target.table !== null &&
-      tabs.find((x) => x.profileId === profileId && x.design?.schema === target.schema && x.design.table === target.table)
+      tabs.find(
+        (x) =>
+          x.profileId === profileId &&
+          x.design?.schema === target.schema &&
+          x.design.table === target.table &&
+          x.design.model === target.model,
+      )
     if (same) {
       set({ active: same.id })
       return same.id
@@ -158,6 +200,13 @@ export const useTabs = create<TabsState>()((set, get) => ({
     set({
       tabs: get().tabs.map((x) => (x.id === id ? { ...x, design: { ...target }, title: target.table ?? x.title } : x)),
     }),
+  openCompare: (target) => {
+    const tab: QueryTab = { id: newId(), title: t('model.compare.title'), profileId: '', sql: '', compare: target }
+    set({ tabs: [...get().tabs, tab], active: tab.id })
+    return tab.id
+  },
+  setCompare: (id, target) =>
+    set({ tabs: get().tabs.map((x) => (x.id === id && x.compare ? { ...x, compare: target } : x)) }),
   setData: (id, patch) =>
     set({ tabs: get().tabs.map((x) => (x.id === id && x.data ? { ...x, data: { ...x.data, ...patch } } : x)) }),
   close: (id) => {
```

- [ ] **Step 22: 新建 `web/src/features/model/ModelDialogs.tsx`**

```tsx
// 打开模型、新建模型、从库反向生成（M4a 设计文档 4.2 节）。模型目录是服务所在机器上的路径：
// 桌面版可以用系统的"选择文件夹"对话框，浏览器模式直接填写。反向生成作为任务执行，显示进度、可以取消。

import { IconFolder } from '@tabler/icons-react'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { App, Button, Form, Input, Modal, Progress, Select, Space } from 'antd'
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { useApi } from '../../app/context'
import { useUi } from '../../app/ui'
import { type Compat, type TaskStatus, errorText } from '../../shared/api'
import { desktop } from '../../shared/shell'
import { withVault } from '../connections/vault'
import { PRODUCTS, type ProductId, productLabel } from '../connections/products'
import { baseName, useModelDialogs } from './dialogs'

function PathField({ value, onChange }: { value?: string; onChange?(v: string): void }) {
  const { t } = useTranslation()
  return (
    <Space.Compact style={{ width: '100%' }}>
      <Input value={value} placeholder={t('model.dialog.pathHint')} onChange={(e) => onChange?.(e.target.value)} />
      {desktop && (
        <Button
          icon={<IconFolder size={16} />}
          onClick={async () => {
            const p = await desktop?.folderDialog()
            if (p) onChange?.(p)
          }}
        />
      )}
    </Space.Compact>
  )
}

/** 打开后切到模型面板。 */
function useOpened() {
  const qc = useQueryClient()
  return () => {
    void qc.invalidateQueries({ queryKey: ['models'] })
    useUi.getState().setSide('models')
  }
}

function OpenDialog() {
  const api = useApi()
  const { t } = useTranslation()
  const { message } = App.useApp()
  const close = useModelDialogs((s) => s.close)
  const opened = useOpened()
  const [path, setPath] = useState('')
  const [busy, setBusy] = useState(false)
  const submit = async () => {
    setBusy(true)
    try {
      await api.openModel(path.trim())
      opened()
      close()
    } catch (e) {
      message.error((e as Error).message)
    } finally {
      setBusy(false)
    }
  }
  return (
    <Modal open title={t('model.dialog.openTitle')} onCancel={close} onOk={() => void submit()} okButtonProps={{ disabled: !path.trim(), loading: busy }}>
      <Form layout="vertical">
        <Form.Item label={t('model.dialog.path')}>
          <PathField value={path} onChange={setPath} />
        </Form.Item>
      </Form>
    </Modal>
  )
}

/** 新建模型时可选的产品：有兼容模式的（人大金仓、openGauss）另选模式。 */
const COMPAT_PRODUCTS: ProductId[] = ['kingbase', 'opengauss']

function CreateDialog() {
  const api = useApi()
  const { t } = useTranslation()
  const { message } = App.useApp()
  const close = useModelDialogs((s) => s.close)
  const opened = useOpened()
  const [path, setPath] = useState('')
  const [name, setName] = useState('')
  const [product, setProduct] = useState<ProductId>('mysql')
  const [compat, setCompat] = useState<Compat>('native')
  const [busy, setBusy] = useState(false)
  const submit = async () => {
    setBusy(true)
    try {
      const withCompat = COMPAT_PRODUCTS.includes(product)
      await api.createModel(path.trim(), name.trim() || baseName(path), { variant: product, compat: withCompat ? compat : 'native' })
      opened()
      close()
    } catch (e) {
      message.error((e as Error).message)
    } finally {
      setBusy(false)
    }
  }
  return (
    <Modal open title={t('model.dialog.createTitle')} onCancel={close} onOk={() => void submit()} okButtonProps={{ disabled: !path.trim(), loading: busy }}>
      <Form layout="vertical">
        <Form.Item label={t('model.dialog.path')} extra={t('model.dialog.emptyDir')}>
          <PathField value={path} onChange={setPath} />
        </Form.Item>
        <Form.Item label={t('model.dialog.name')}>
          <Input value={name} placeholder={baseName(path)} onChange={(e) => setName(e.target.value)} />
        </Form.Item>
        <Form.Item label={t('model.dialog.product')} extra={t('model.dialog.versionNote')}>
          <Select value={product} onChange={setProduct} options={PRODUCTS.map((p) => ({ value: p.id, label: productLabel(p.id) }))} />
        </Form.Item>
        {COMPAT_PRODUCTS.includes(product) && (
          <Form.Item label={t('model.dialog.compat')}>
            <Select
              value={compat}
              onChange={setCompat}
              options={[
                { value: 'native', label: t('model.dialog.compatNative') },
                { value: 'oracle', label: t('model.dialog.compatOracle') },
              ]}
            />
          </Form.Item>
        )}
      </Form>
    </Modal>
  )
}

function ReverseDialog() {
  const api = useApi()
  const { t } = useTranslation()
  const { message } = App.useApp()
  const init = useModelDialogs((s) => s.reverse)!
  const close = useModelDialogs((s) => s.close)
  const opened = useOpened()
  const { data: profiles = [] } = useQuery({ queryKey: ['profiles'], queryFn: () => api.profiles() })
  const [profileId, setProfileId] = useState(init.profileId ?? useUi.getState().selectedProfile ?? profiles[0]?.id ?? null)
  const [schemas, setSchemas] = useState<string[]>(init.schema ? [init.schema] : [])
  const [path, setPath] = useState('')
  const [name, setName] = useState('')
  const [task, setTask] = useState<TaskStatus | null>(null)
  // 连接列表加载完之前打开的对话框：加载后选第一个
  useEffect(() => {
    if (!profileId && profiles.length) setProfileId(profiles[0].id)
  }, [profileId, profiles])
  const { data: all = [] } = useQuery({
    queryKey: ['schemas', profileId],
    queryFn: () => withVault(api, () => api.schemas(profileId!)),
    enabled: !!profileId,
  })

  // 执行中：轮询任务状态；完成后打开模型
  useEffect(() => {
    if (!task) return
    if (task.state === 'done') {
      message.success(t('model.dialog.reverseDone'))
      opened()
      close()
      return
    }
    if (task.state !== 'running') return
    const h = setTimeout(async () => {
      try {
        setTask(await api.task(task.id))
      } catch (e) {
        message.error((e as Error).message)
        setTask(null)
      }
    }, 400)
    return () => clearTimeout(h)
  }, [task, api, message, t, opened, close])

  const start = async () => {
    try {
      const id = await withVault(api, () =>
        api.startReverse({ profile_id: profileId!, schemas, path: path.trim(), name: name.trim() || baseName(path) }),
      )
      setTask({ id, state: 'running', progress: { read: 0, total: schemas.length } } as TaskStatus)
    } catch (e) {
      message.error((e as Error).message)
    }
  }
  const running = task?.state === 'running'
  return (
    <Modal
      open
      title={t('model.dialog.reverseTitle')}
      onCancel={() => {
        if (running) void api.cancelTask(task!.id).catch(() => {})
        close()
      }}
      onOk={() => void start()}
      okText={t('model.dialog.reverseOk')}
      okButtonProps={{ disabled: !profileId || !schemas.length || !path.trim() || running, loading: running }}
    >
      <Form layout="vertical">
        <Form.Item label={t('model.dialog.profile')}>
          <Select
            value={profileId ?? undefined}
            onChange={(v) => {
              setProfileId(v)
              setSchemas([])
            }}
            options={profiles.map((p) => ({ value: p.id, label: p.name }))}
          />
        </Form.Item>
        <Form.Item label={t('model.dialog.schemas')}>
          <Select mode="multiple" value={schemas} onChange={setSchemas} options={all.map((s) => ({ value: s.name }))} />
        </Form.Item>
        <Form.Item label={t('model.dialog.path')} extra={t('model.dialog.emptyDir')}>
          <PathField value={path} onChange={setPath} />
        </Form.Item>
        <Form.Item label={t('model.dialog.name')}>
          <Input value={name} placeholder={baseName(path)} onChange={(e) => setName(e.target.value)} />
        </Form.Item>
      </Form>
      {task && task.state !== 'running' && task.state !== 'done' && (
        <div className="err">{task.error ? errorText(task.error) : t('model.dialog.reverseFailed')}</div>
      )}
      {running && <Progress size="small" percent={Math.round((100 * task.progress.read) / Math.max(1, task.progress.total))} />}
    </Modal>
  )
}

export default function ModelDialogs() {
  const { open, create, reverse } = useModelDialogs()
  return (
    <>
      {open && <OpenDialog />}
      {create && <CreateDialog />}
      {reverse && <ReverseDialog />}
    </>
  )
}
```

- [ ] **Step 23: 新建 `web/src/features/model/ModelsPanel.tsx`**

```tsx
// 活动栏的"模型"面板（M4a 设计文档第 7 节）：打开的模型 → schema → 表（文件有错的标红），最近打开的模型；
// 工具栏打开、新建、从库反向生成、重新加载；右键新建表、用设计器打开、改名、删除、与库比对。
// 模型目录是服务所在机器上的路径：服务不允许时（监听其他地址的浏览器模式）只显示说明。

import { IconDatabaseImport, IconFolderOpen, IconPlus, IconRefresh, IconSchema, IconTable } from '@tabler/icons-react'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { App, Dropdown, Input, type MenuProps, Tree } from 'antd'
import type { DataNode } from 'antd/es/tree'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

import { useApi } from '../../app/context'
import { ApiError, type ModelSummary, type ModelTableEntry } from '../../shared/api'
import { desktop } from '../../shared/shell'
import { useTabs } from '../editor/tabs'
import { useModelDialogs } from './dialogs'

type NodeRef =
  | { type: 'model'; model: ModelSummary }
  | { type: 'schema'; model: ModelSummary; schema: string }
  | { type: 'table'; model: ModelSummary; entry: ModelTableEntry }

function ModelTree({ model, onMenu }: { model: ModelSummary; onMenu(n: NodeRef, key: string): void }) {
  const api = useApi()
  const { t } = useTranslation()
  const { data: tables = [] } = useQuery({ queryKey: ['model-tables', model.id], queryFn: () => api.modelTables(model.id) })
  const schemas = [...new Set([...model.info.schemas, ...tables.map((x) => x.schema)])]
  const refs = new Map<string, NodeRef>()
  const node = (key: string, ref: NodeRef, d: Omit<DataNode, 'key'>): DataNode => {
    refs.set(key, ref)
    return { key, ...d }
  }
  const tree: DataNode[] = [
    node(model.id, { type: 'model', model }, {
      title: <span title={model.path}>{model.info.name}{model.read_only && <span className="err"> · {t('model.readOnly')}</span>}</span>,
      icon: <IconSchema size={16} />,
      children: schemas.map((s) =>
        node(`${model.id}/${s}`, { type: 'schema', model, schema: s }, {
          title: s,
          children: tables
            .filter((x) => x.schema === s)
            .map((e) =>
              node(`${model.id}/${s}/${e.name}`, { type: 'table', model, entry: e }, {
                title: e.error ? (
                  <span className="err" title={`${e.error.path}: ${e.error.message}`}>{e.name}</span>
                ) : (
                  <span title={e.comment ?? undefined}>{e.name}</span>
                ),
                icon: <IconTable size={14} />,
                isLeaf: true,
              }),
            ),
        }),
      ),
    }),
  ]
  const [menuNode, setMenuNode] = useState<NodeRef | null>(null)
  const items = (n: NodeRef | null): MenuProps['items'] => {
    if (!n) return []
    const ro = n.model.read_only
    if (n.type === 'table') {
      return [
        { key: 'design', label: t('model.menu.design'), disabled: !!n.entry.error },
        { key: 'compare', label: t('model.menu.compare') },
        { type: 'divider' },
        { key: 'rename', label: t('model.menu.rename'), disabled: ro || !!n.entry.error },
        { key: 'delete', label: t('model.menu.delete'), danger: true, disabled: ro },
      ]
    }
    const common: NonNullable<MenuProps['items']> = [
      { key: 'new-table', label: t('model.menu.newTable'), disabled: ro },
      { key: 'compare', label: t('model.menu.compare') },
    ]
    if (n.type === 'schema') return common
    return [
      ...common,
      { type: 'divider' },
      { key: 'reload', label: t('model.menu.reload') },
      ...(desktop ? [{ key: 'reveal', label: t('model.menu.reveal') }] : []),
      { key: 'close', label: t('model.menu.close') },
    ]
  }
  return (
    <Dropdown trigger={['contextMenu']} menu={{ items: items(menuNode), onClick: ({ key }) => menuNode && onMenu(menuNode, key) }}>
      <div>
        <Tree
          showIcon
          blockNode
          defaultExpandedKeys={[model.id]}
          treeData={tree}
          onRightClick={({ node: n }) => setMenuNode(refs.get(String(n.key)) ?? null)}
          onDoubleClick={(_, n) => {
            const r = refs.get(String(n.key))
            if (r?.type === 'table' && !r.entry.error) onMenu(r, 'design')
          }}
        />
      </div>
    </Dropdown>
  )
}

export default function ModelsPanel() {
  const api = useApi()
  const { t } = useTranslation()
  const { message, modal } = App.useApp()
  const qc = useQueryClient()
  const dialogs = useModelDialogs()
  const { data, error, refetch } = useQuery({ queryKey: ['models'], queryFn: () => api.models(), retry: false })

  if (error) {
    const forbidden = error instanceof ApiError && error.status === 403
    return <div className="muted" style={{ padding: 12 }}>{forbidden ? t('model.localOnly') : (error as Error).message}</div>
  }

  const reload = async (m?: ModelSummary) => {
    if (m) await api.model(m.id).catch((e) => message.error((e as Error).message))
    await qc.invalidateQueries({ queryKey: ['models'] })
    await qc.invalidateQueries({ queryKey: m ? ['model-tables', m.id] : ['model-tables'] })
  }

  const onMenu = async (n: NodeRef, key: string) => {
    const m = n.model
    const schema = n.type === 'schema' ? n.schema : n.type === 'table' ? n.entry.schema : (m.info.schemas[0] ?? '')
    try {
      if (key === 'design' && n.type === 'table') useTabs.getState().openDesign('', { schema, table: n.entry.name, model: m.path })
      else if (key === 'new-table') useTabs.getState().openDesign('', { schema, table: null, model: m.path })
      else if (key === 'compare') useTabs.getState().openCompare({ source: { kind: 'model', path: m.path, schema }, target: null })
      else if (key === 'reload') await reload(m)
      else if (key === 'reveal') await desktop?.revealInFolder(`${m.path}/model.yaml`)
      else if (key === 'close') {
        await api.closeModel(m.id)
        await qc.invalidateQueries({ queryKey: ['models'] })
        qc.removeQueries({ queryKey: ['model-open', m.path] })
      } else if (key === 'rename' && n.type === 'table') {
        let to = n.entry.name
        modal.confirm({
          title: t('model.rename.title', { name: n.entry.name }),
          content: <Input defaultValue={to} onChange={(e) => (to = e.target.value)} />,
          onOk: async () => {
            if (!to.trim() || to === n.entry.name) return
            await api.renameModelTable(m.id, schema, n.entry.name, to.trim())
            await reload()
          },
        })
      } else if (key === 'delete' && n.type === 'table') {
        modal.confirm({
          title: t('model.delete.title', { name: n.entry.name }),
          content: t('model.delete.content'),
          okType: 'danger',
          onOk: async () => {
            await api.deleteModelTable(m.id, schema, n.entry.name)
            await reload()
          },
        })
      }
    } catch (e) {
      message.error((e as Error).message)
    }
  }

  const open = data?.open ?? []
  const recent = (data?.recent ?? []).filter((p) => !open.some((m) => m.path === p))
  return (
    <div className="models-panel">
      <div className="row" style={{ gap: 4, padding: '4px 8px' }}>
        <span className="a" title={t('model.toolbar.open')} onClick={dialogs.openOpen}><IconFolderOpen size={16} /></span>
        <span className="a" title={t('model.toolbar.create')} onClick={dialogs.openCreate}><IconPlus size={16} /></span>
        <span className="a" title={t('model.toolbar.reverse')} onClick={() => dialogs.openReverse()}><IconDatabaseImport size={16} /></span>
        <span className="grow" />
        <span className="a" title={t('model.toolbar.reload')} onClick={() => void reload().then(() => refetch())}><IconRefresh size={16} /></span>
      </div>
      {open.length === 0 && <div className="muted" style={{ padding: '4px 12px' }}>{t('model.empty')}</div>}
      {open.map((m) => (
        <ModelTree key={m.id} model={m} onMenu={(n, k) => void onMenu(n, k)} />
      ))}
      {recent.length > 0 && (
        <>
          <div className="muted" style={{ padding: '8px 12px 4px' }}>{t('model.recent')}</div>
          {recent.map((p) => (
            <div
              key={p}
              className="models-recent"
              title={p}
              onClick={() =>
                void api
                  .openModel(p)
                  .then(() => reload())
                  .catch((e) => message.error((e as Error).message))
              }
            >
              {p}
            </div>
          ))}
        </>
      )}
    </div>
  )
}
```

- [ ] **Step 24: 新建 `web/src/features/model/dialogs.ts`**

```ts
// 模型的对话框（M4a 设计文档第 7 节）：打开、新建、从库反向生成。菜单、对象树、模型面板共用。

import { create } from 'zustand'

/** 反向生成的初始值：对象树中 schema 的"另存为模型…"带上连接和 schema。 */
export interface ReverseInit {
  profileId: string | null
  schema: string | null
}

interface ModelDialogs {
  open: boolean
  create: boolean
  reverse: ReverseInit | null
  openOpen(): void
  openCreate(): void
  openReverse(init?: ReverseInit): void
  close(): void
}

export const useModelDialogs = create<ModelDialogs>()((set) => ({
  open: false,
  create: false,
  reverse: null,
  openOpen: () => set({ open: true }),
  openCreate: () => set({ create: true }),
  openReverse: (init = { profileId: null, schema: null }) => set({ reverse: init }),
  close: () => set({ open: false, create: false, reverse: null }),
}))

/** 目录的最后一段（新建模型时作为默认名称）。 */
export function baseName(path: string): string {
  return path.replace(/[\\/]+$/, '').split(/[\\/]/).pop() ?? ''
}
```

- [ ] **Step 25: 修改 `web/src/features/tree/ObjectTree.tsx`**

```diff
--- a/web/src/features/tree/ObjectTree.tsx
+++ b/web/src/features/tree/ObjectTree.tsx
@@ -28,6 +28,7 @@ import { useQueryActions } from '../editor/actions'
 import { useTabs } from '../editor/tabs'
 import { useExportDialog } from '../export/dialog'
 import { useImportDialogs } from '../import/dialog'
+import { useModelDialogs } from '../model/dialogs'
 import type { Api, Environment, Profile, RoutineInfo } from '../../shared/api'
 import DbIcon from '../../shared/DbIcon'
 import { profileKind } from '../connections/form'
@@ -300,6 +301,9 @@ export default function ObjectTree() {
       { key: 'import-new', label: t('tree.menu.importNew') },
       { key: 'sql-file', label: t('tree.menu.sqlFile') },
       { type: 'divider' },
+      { key: 'compare-model', label: t('tree.menu.compareModel') },
+      { key: 'save-model', label: t('tree.menu.saveAsModel') },
+      { type: 'divider' },
     ]
   }
 
@@ -345,6 +349,9 @@ export default function ObjectTree() {
       } else if (key === 'view-data') openData(k)
       else if (key === 'design' && k.type === 'o') useTabs.getState().openDesign(k.profileId, { schema: k.schema, table: k.name })
       else if (key === 'new-table' && k.type === 's') useTabs.getState().openDesign(k.profileId, { schema: k.schema, table: null })
+      else if (key === 'compare-model' && k.type === 's') {
+        useTabs.getState().openCompare({ source: null, target: { kind: 'database', profile: k.profileId, schema: k.schema } })
+      } else if (key === 'save-model' && k.type === 's') useModelDialogs.getState().openReverse({ profileId: k.profileId, schema: k.schema })
       else if (key === 'import' && k.type === 'o') {
         useImportDialogs.getState().openImport({ profileId: k.profileId, schema: k.schema, table: k.name, create: false })
       } else if (key === 'import-new' && k.type === 's') {
```

- [ ] **Step 26: 修改 `web/src/locales/en/app.json`**

```diff
--- a/web/src/locales/en/app.json
+++ b/web/src/locales/en/app.json
@@ -27,6 +27,7 @@
   },
   "side": {
     "db": "Database",
+    "models": "Models",
     "history": "History",
     "favorites": "Favorites",
     "ops": "Ops Queries"
@@ -67,7 +68,10 @@
       "settings": "Settings…",
       "quit": "Exit",
       "import": "Import Data…",
-      "sqlFile": "Run SQL File…"
+      "sqlFile": "Run SQL File…",
+      "openModel": "Open Model…",
+      "newModel": "New Model…",
+      "compare": "Compare Schemas…"
     },
     "view": {
       "title": "View",
```

- [ ] **Step 27: 新建 `web/src/locales/en/model.json`**

```json
{
  "readOnly": "Read-only (newer file format)",
  "localOnly": "Model directories are paths on the server's machine: models are available only in the desktop app and in browser mode listening on a local address.",
  "empty": "No open models",
  "recent": "Recent",
  "toolbar": {
    "open": "Open Model",
    "create": "New Model",
    "reverse": "Reverse-engineer from Database",
    "reload": "Reload"
  },
  "menu": {
    "design": "Open in Designer",
    "compare": "Compare with Database…",
    "rename": "Rename…",
    "delete": "Delete",
    "newTable": "New Table",
    "reload": "Reload",
    "reveal": "Show in File Manager",
    "close": "Close Model"
  },
  "rename": {
    "title": "Rename table {{name}} to"
  },
  "delete": {
    "title": "Delete table {{name}}?",
    "content": "Deletes the table's file and its rename records."
  },
  "dialog": {
    "openTitle": "Open Model",
    "createTitle": "New Model",
    "reverseTitle": "Reverse-engineer a Model from a Database",
    "path": "Model directory",
    "pathHint": "A directory on the server's machine",
    "emptyDir": "The directory must not exist or must be empty",
    "name": "Name",
    "product": "Database type",
    "versionNote": "Uses the latest version; reverse-engineered models carry the database's version",
    "compat": "Compatibility mode",
    "compatNative": "Native",
    "compatOracle": "Oracle compatible",
    "profile": "Connection",
    "schemas": "Schemas",
    "reverseOk": "Generate",
    "reverseDone": "Model generated",
    "reverseFailed": "Generation failed"
  },
  "design": {
    "saved": "Saved to the model file",
    "compare": "Compare with Database",
    "changes": "Changes",
    "conflict": "The file has changed (for example after switching git branches); reload it"
  },
  "compare": {
    "title": "Compare",
    "source": "Source",
    "target": "Target",
    "swap": "Swap sides",
    "database": "Database",
    "model": "Model",
    "pickModel": "Select a model",
    "pickProfile": "Select a connection",
    "run": "Compare",
    "hint": "Choose both sides and click Compare. The result shows how to change the target to match the source.",
    "onlyDiff": "Differences only",
    "statusFilter": "Filter by status",
    "option": {
      "ignore_comments": "Ignore comments",
      "ignore_column_order": "Ignore column order",
      "ignore_generated_names": "Ignore system-generated names",
      "case_insensitive": "Case-insensitive names"
    },
    "status": {
      "added": "Added",
      "removed": "Removed",
      "renamed": "Renamed",
      "changed": "Changed",
      "same": "Same"
    },
    "change": {
      "dropTable": "Drop table {{name}}",
      "renameTable": "Rename table {{from}} to {{to}}"
    },
    "dep": {
      "create": "A foreign key of {{table}} references {{ref}}, but creating {{ref}} is not selected",
      "drop": "{{by}} still has a foreign key referencing {{table}}, which will be dropped"
    },
    "fileError": "File {{path}} is invalid and was not compared: {{message}}",
    "suggestions": "Possible renames (treated as drop plus add unless accepted; data would be lost)",
    "acceptRename": "Treat as rename",
    "summary": "{{changed}} of {{total}} tables differ",
    "pickTable": "Select a table to see both definitions",
    "selected": "{{count}} selected",
    "copy": "Copy Script",
    "openQuery": "Open in Query Tab",
    "saveAs": "Save As…",
    "sync": "Sync to Target…",
    "copied": "Copied",
    "saved": "Saved"
  },
  "sync": {
    "title": "Sync to Target",
    "execute": "Execute",
    "write": "Write to Model",
    "denied": "Denied by the execution policy: {{reason}}",
    "refused": "Table {{table}} cannot be rebuilt: {{reason}}",
    "deleteFile": "Delete the file of {{name}}",
    "renameFile": "Rename {{from}} to {{name}} and write it",
    "writeFile": "Write {{name}}",
    "done": "Sync finished",
    "written": "Written to the model"
  }
}
```

- [ ] **Step 28: 修改 `web/src/locales/en/tree.json`**

```diff
--- a/web/src/locales/en/tree.json
+++ b/web/src/locales/en/tree.json
@@ -23,6 +23,8 @@
   },
   "filterPlaceholder": "Filter by name or comment",
   "menu": {
+    "compareModel": "Compare with Model…",
+    "saveAsModel": "Save as Model…",
     "newQuery": "New Query",
     "editConnection": "Edit Connection…",
     "duplicateConnection": "Duplicate Connection",
```

- [ ] **Step 29: 修改 `web/src/locales/index.ts`**

```diff
--- a/web/src/locales/index.ts
+++ b/web/src/locales/index.ts
@@ -7,6 +7,7 @@ import enConnections from './en/connections.json'
 import enDesign from './en/design.json'
 import enEditor from './en/editor.json'
 import enImport from './en/import.json'
+import enModel from './en/model.json'
 import enResults from './en/results.json'
 import enTree from './en/tree.json'
 import zhApp from './zh-CN/app.json'
@@ -15,6 +16,7 @@ import zhConnections from './zh-CN/connections.json'
 import zhDesign from './zh-CN/design.json'
 import zhEditor from './zh-CN/editor.json'
 import zhImport from './zh-CN/import.json'
+import zhModel from './zh-CN/model.json'
 import zhResults from './zh-CN/results.json'
 import zhTree from './zh-CN/tree.json'
 
@@ -27,6 +29,7 @@ export const zhCN = {
   results: zhResults,
   import: zhImport,
   design: zhDesign,
+  model: zhModel,
 }
 
 export const en = {
@@ -38,4 +41,5 @@ export const en = {
   results: enResults,
   import: enImport,
   design: enDesign,
+  model: enModel,
 }
```

- [ ] **Step 30: 修改 `web/src/locales/zh-CN/app.json`**

```diff
--- a/web/src/locales/zh-CN/app.json
+++ b/web/src/locales/zh-CN/app.json
@@ -27,6 +27,7 @@
   },
   "side": {
     "db": "数据库",
+    "models": "模型",
     "history": "历史",
     "favorites": "收藏",
     "ops": "运维查询"
@@ -66,7 +67,10 @@
       "settings": "设置…",
       "quit": "退出",
       "import": "导入数据…",
-      "sqlFile": "执行 SQL 文件…"
+      "sqlFile": "执行 SQL 文件…",
+      "openModel": "打开模型…",
+      "newModel": "新建模型…",
+      "compare": "结构比对…"
     },
     "view": {
       "title": "查看",
```

- [ ] **Step 31: 新建 `web/src/locales/zh-CN/model.json`**

```json
{
  "readOnly": "只读（文件格式较新）",
  "localOnly": "模型目录是服务所在机器上的路径：只有桌面程序和只监听本机地址的浏览器模式可以使用模型。",
  "empty": "没有打开的模型",
  "recent": "最近打开",
  "toolbar": {
    "open": "打开模型",
    "create": "新建模型",
    "reverse": "从库反向生成",
    "reload": "重新加载"
  },
  "menu": {
    "design": "用设计器打开",
    "compare": "与库比对…",
    "rename": "改名…",
    "delete": "删除",
    "newTable": "新建表",
    "reload": "重新加载",
    "reveal": "在文件管理器中显示",
    "close": "关闭模型"
  },
  "rename": {
    "title": "表 {{name}} 改名为"
  },
  "delete": {
    "title": "删除表 {{name}}？",
    "content": "删除这张表的文件，并去掉它的改名记录。"
  },
  "dialog": {
    "openTitle": "打开模型",
    "createTitle": "新建模型",
    "reverseTitle": "从库反向生成模型",
    "path": "模型目录",
    "pathHint": "服务所在机器上的目录",
    "emptyDir": "目录必须不存在或为空",
    "name": "名称",
    "product": "数据库类型",
    "versionNote": "版本按最新版本；从库反向生成的模型带库的版本",
    "compat": "兼容模式",
    "compatNative": "原生",
    "compatOracle": "Oracle 兼容",
    "profile": "连接",
    "schemas": "Schema",
    "reverseOk": "生成",
    "reverseDone": "模型已生成",
    "reverseFailed": "生成失败"
  },
  "design": {
    "saved": "已保存到模型文件",
    "compare": "与库比对",
    "changes": "变更",
    "conflict": "文件已被修改（如在 git 中切换了分支），请重新加载"
  },
  "compare": {
    "title": "比对",
    "source": "源",
    "target": "目标",
    "swap": "交换两边",
    "database": "连接",
    "model": "模型",
    "pickModel": "选择模型",
    "pickProfile": "选择连接",
    "run": "比对",
    "hint": "选好两边后点“比对”。结果描述怎样修改目标才与源一致。",
    "onlyDiff": "只看差异",
    "statusFilter": "按状态过滤",
    "option": {
      "ignore_comments": "忽略注释",
      "ignore_column_order": "忽略列顺序",
      "ignore_generated_names": "忽略系统生成的名字",
      "case_insensitive": "名字不区分大小写"
    },
    "status": {
      "added": "新增",
      "removed": "删除",
      "renamed": "改名",
      "changed": "修改",
      "same": "相同"
    },
    "change": {
      "dropTable": "删除表 {{name}}",
      "renameTable": "表 {{from}} 改名为 {{to}}"
    },
    "dep": {
      "create": "{{table}} 的外键引用 {{ref}}，但没有勾选新建 {{ref}}",
      "drop": "删除 {{table}} 时 {{by}} 的外键仍引用它"
    },
    "fileError": "文件 {{path}} 有错，没有参与比对：{{message}}",
    "suggestions": "疑似改名（默认按删除加新增处理，不采用会丢数据）",
    "acceptRename": "按改名处理",
    "summary": "{{changed}} / {{total}} 张表有差异",
    "pickTable": "选择一张表查看两边的定义",
    "selected": "已勾选 {{count}} 项",
    "copy": "复制脚本",
    "openQuery": "在查询标签中打开",
    "saveAs": "另存为…",
    "sync": "同步到目标…",
    "copied": "已复制",
    "saved": "已保存"
  },
  "sync": {
    "title": "同步到目标",
    "execute": "执行",
    "write": "写回模型",
    "denied": "执行策略不允许：{{reason}}",
    "refused": "表 {{table}} 不能重建：{{reason}}",
    "deleteFile": "删除表文件 {{name}}",
    "renameFile": "{{from}} 改名为 {{name}} 并写入",
    "writeFile": "写入 {{name}}",
    "done": "同步完成",
    "written": "已写回模型"
  }
}
```

- [ ] **Step 32: 修改 `web/src/locales/zh-CN/tree.json`**

```diff
--- a/web/src/locales/zh-CN/tree.json
+++ b/web/src/locales/zh-CN/tree.json
@@ -23,6 +23,8 @@
   },
   "filterPlaceholder": "按名称或注释过滤",
   "menu": {
+    "compareModel": "与模型比对…",
+    "saveAsModel": "另存为模型…",
     "newQuery": "新建查询",
     "editConnection": "编辑连接…",
     "duplicateConnection": "复制连接",
```

- [ ] **Step 33: 修改 `web/src/shared/api.ts`**

```diff
--- a/web/src/shared/api.ts
+++ b/web/src/shared/api.ts
@@ -425,7 +425,7 @@ export interface SqlFileRequest {
 
 export interface TaskStatus {
   id: string
-  kind: 'import' | 'sql_file' | 'design'
+  kind: 'import' | 'sql_file' | 'design' | 'reverse' | 'sync'
   state: 'running' | 'confirm' | 'done' | 'failed' | 'cancelled'
   phase: 'preparing' | 'scanning' | 'creating' | 'clearing' | 'writing' | 'executing' | 'finished'
   progress: {
@@ -448,6 +448,8 @@ export interface TaskStatus {
   report: string | null
   /** 表设计器：失败后没有执行的语句。 */
   remaining: string | null
+  /** 反向生成：生成并打开的模型的编号。 */
+  model: string | null
 }
 
 /** 表设计器（M3）：与后端结构模型一致的完整表结构。 */
@@ -593,6 +595,144 @@ export interface DesignPreview {
   conflict: boolean
 }
 
+/** 模型（M4a）：绑定的数据库类型。`version` 是编码后的版本，不写时按最新版本。 */
+export interface ModelDatabase {
+  variant: string
+  compat?: Compat
+  version?: number | null
+}
+
+export interface ModelInfo {
+  format: number
+  name: string
+  description?: string | null
+  database: ModelDatabase
+  schemas: string[]
+  renames?: Record<string, { tables?: Record<string, string>; columns?: Record<string, Record<string, string>> }>
+}
+
+export interface ModelSummary {
+  id: string
+  path: string
+  info: ModelInfo
+  read_only: boolean
+}
+
+export interface ModelFileError {
+  path: string
+  message: string
+}
+
+export interface ModelTableEntry {
+  schema: string
+  name: string
+  comment?: string | null
+  error?: ModelFileError | null
+}
+
+export interface ModelDesignRequest {
+  schema: string
+  name: string | null
+  draft: DesignTableModel
+  renames: DesignRenames
+  fingerprint: string | null
+}
+
+export interface ModelDesignPreview {
+  errors: DesignIssue[]
+  warnings: string[]
+  changes: TableChange[]
+  conflict: boolean
+}
+
+export interface ReverseRequest {
+  profile_id: string
+  schemas: string[]
+  path: string
+  name: string
+}
+
+/** 比对的一边：模型的一个 schema，或连接的一个 schema。 */
+export type CompareSideRef =
+  | { kind: 'model'; model: string; schema: string }
+  | { kind: 'database'; profile: string; schema: string }
+
+export interface CompareOptions {
+  ignore_comments: boolean
+  ignore_column_order: boolean
+  ignore_generated_names: boolean
+  case_insensitive: boolean
+}
+
+/** 采用的改名：源的名字 → 目标的名字；列按源的表名分组。 */
+export interface CompareRenames {
+  tables: Record<string, string>
+  columns: Record<string, Record<string, string>>
+}
+
+export interface CompareRequest {
+  source: CompareSideRef
+  target: CompareSideRef
+  options: CompareOptions
+  renames: CompareRenames
+}
+
+export type SchemaChange =
+  | { kind: 'drop_table'; name: string }
+  | { kind: 'rename_table'; from: string; to: string }
+  | { kind: 'table'; change: TableChange }
+
+export type TableStatus = 'added' | 'removed' | 'renamed' | 'changed' | 'same'
+
+export interface TableDiff {
+  status: TableStatus
+  source: string | null
+  target: string | null
+  items: { id: number; change: SchemaChange }[]
+  appended: boolean
+}
+
+/** 推测的改名：`table` 为 null 时是表的改名，否则是该表（源的名字）中列的改名。 */
+export interface RenameSuggestion {
+  table: string | null
+  source: string
+  target: string
+}
+
+export interface CompareResult {
+  diff: { tables: TableDiff[]; suggestions: RenameSuggestion[] }
+  renames: CompareRenames
+  source_errors: ModelFileError[]
+  target_errors: ModelFileError[]
+  source_tables: DesignTableModel[]
+  target_tables: DesignTableModel[]
+  fingerprint: string
+}
+
+export interface SyncRequest {
+  compare: CompareRequest
+  selected: number[]
+  fingerprint: string | null
+}
+
+export type SyncStep =
+  | { kind: 'sql'; sql: string; items: number[] }
+  | {
+      kind: 'rebuild'
+      table: string
+      rebuild: { steps: DesignStep[]; drop_at: number; cleanup: string; warnings: string[] }
+      items: number[]
+    }
+
+export interface SyncPreview {
+  script: { steps: SyncStep[]; refused: [string, string][]; warnings: string[]; transactional: boolean } | null
+  text: string
+  writes: { name: string; from?: string | null; delete: boolean }[]
+  denied: string | null
+  confirm: string[]
+  fingerprint: string
+}
+
 export type FetchFn = (input: string, init?: RequestInit) => Promise<Response>
 
 export function createApi(token: string, fetchFn: FetchFn = (i, o) => fetch(i, o)) {
@@ -613,6 +753,9 @@ export function createApi(token: string, fetchFn: FetchFn = (i, o) => fetch(i, o
   }
 
   const p = (id: string) => `/profiles/${encodeURIComponent(id)}`
+  const m = (id: string) => `/models/${encodeURIComponent(id)}`
+  const mt = (id: string, schema: string, name: string) =>
+    `${m(id)}/tables/${encodeURIComponent(schema)}/${encodeURIComponent(name)}`
   const s = (id: string, schema: string) => `${p(id)}/schemas/${encodeURIComponent(schema)}`
   const q = (params: Record<string, string | undefined>) => {
     const entries = Object.entries(params).filter((e): e is [string, string] => e[1] !== undefined && e[1] !== '')
@@ -745,6 +888,32 @@ export function createApi(token: string, fetchFn: FetchFn = (i, o) => fetch(i, o
     previewDesign: (id: string, req: DesignRequest) => request<DesignPreview>('POST', `${p(id)}/design/preview`, req),
     applyDesign: async (id: string, req: DesignRequest) =>
       (await request<{ id: string }>('POST', `${p(id)}/design/apply`, req)).id,
+    models: () => request<{ open: ModelSummary[]; recent: string[] }>('GET', '/models'),
+    openModel: (path: string) => request<ModelSummary>('POST', '/models/open', { path }),
+    createModel: (path: string, name: string, database: ModelDatabase) =>
+      request<ModelSummary>('POST', '/models', { path, name, database }),
+    startReverse: async (req: ReverseRequest) => (await request<{ id: string }>('POST', '/models/reverse', req)).id,
+    /** 重新加载并返回模型信息。 */
+    model: (id: string) => request<ModelSummary>('GET', m(id)),
+    closeModel: (id: string) => request<null>('DELETE', m(id)),
+    modelCaps: (id: string) => request<DesignCaps>('GET', `${m(id)}/caps`),
+    modelTables: (id: string) => request<ModelTableEntry[]>('GET', `${m(id)}/tables`),
+    modelTable: (id: string, schema: string, name: string) =>
+      request<{ table: DesignTableModel; fingerprint: string }>('GET', mt(id, schema, name)),
+    previewModelTable: (id: string, req: ModelDesignRequest) =>
+      request<ModelDesignPreview>('POST', `${m(id)}/tables/preview`, req),
+    /** 保存设计器中的表，返回新的指纹。 */
+    saveModelTable: async (id: string, req: ModelDesignRequest) =>
+      (await request<{ fingerprint: string }>('POST', `${m(id)}/tables`, req)).fingerprint,
+    deleteModelTable: (id: string, schema: string, name: string) => request<null>('DELETE', mt(id, schema, name)),
+    renameModelTable: (id: string, schema: string, name: string, to: string) =>
+      request<null>('POST', `${mt(id, schema, name)}/rename`, { to }),
+    compare: (req: CompareRequest) => request<CompareResult>('POST', '/compare', req),
+    syncPreview: (req: SyncRequest) => request<SyncPreview>('POST', '/compare/preview', req),
+    /** 同步：目标是库时返回任务 id；目标是模型时已经写完，返回 null。 */
+    sync: async (req: SyncRequest) => (await request<{ task: string | null }>('POST', '/compare/sync', req)).task,
+    /** 桌面版：把同步脚本写到"另存为"选的 .sql 文件。 */
+    saveScript: (path: string, text: string) => request<null>('POST', '/compare/script', { path, text }),
     openSession: (profileId: string) => request<SessionInfo>('POST', '/sessions', { profile_id: profileId }),
     session: (id: number) => request<SessionInfo>('GET', `/sessions/${id}`),
     /** 关闭会话；有未提交的修改时按 commit 提交或回滚。 */
```

- [ ] **Step 34: 修改 `web/src/shared/shell.ts`**

```diff
--- a/web/src/shared/shell.ts
+++ b/web/src/shared/shell.ts
@@ -1,5 +1,5 @@
 // 桌面外壳的原生能力（M1 设计文档 2.2 节）：自绘标题栏的窗口操作，以及置顶、恢复默认位置、检查更新、关于、退出；
-// 导出（M1c3）用的"另存为"对话框和"在文件夹中显示"，导入（M2b）用的"打开"对话框。
+// 导出（M1c3）用的"另存为"对话框和"在文件夹中显示"，导入（M2b）用的"打开"对话框，模型（M4a）用的"选择文件夹"对话框。
 // 只有这里调用 Tauri；其他代码通过这里的函数使用。浏览器模式（dblens-server）没有 Tauri，desktop 为 null。
 
 import { invoke } from '@tauri-apps/api/core'
@@ -38,6 +38,8 @@ export interface DesktopShell {
   revealInFolder(path: string): Promise<void>
   /** 系统"打开"对话框；用户取消时为 null。 */
   openDialog(filters: { name: string; extensions: string[] }[]): Promise<string | null>
+  /** 系统"选择文件夹"对话框（模型目录）；用户取消时为 null。 */
+  folderDialog(): Promise<string | null>
 }
 
 /** 页面是否运行在桌面程序里：Tauri 会在页面脚本运行前注入 `__TAURI_INTERNALS__`。 */
@@ -54,6 +56,7 @@ function createDesktopShell(): DesktopShell {
     saveDialog: (defaultName, filters) => invoke<string | null>('save_dialog', { defaultName, filters }),
     revealInFolder: (path) => invoke('reveal_in_folder', { path }),
     openDialog: (filters) => invoke<string | null>('open_dialog', { filters }),
+    folderDialog: () => invoke<string | null>('folder_dialog'),
   }
 }
 
```

- [ ] **Step 35: 检查**

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

Expected: 全部通过（前端新增 5 个测试）。

- [ ] **Step 36: 浏览器实测**

浏览器实测（设计文档第 8 节）：用 `dblens-server --listen 127.0.0.1:<端口>` 启动浏览器模式，在 openGauss PG（或任一库）上：
1. 对象树 schema 右键"另存为模型…"：对话框带上连接和 schema，填目录生成；模型面板出现模型 → schema → 表。
2. 双击表打开设计器：标题"模型名 / schema.表"；列改名后"变更"子标签列出改名；保存提示"已保存到模型文件"。
3. 工具栏"与库比对"：比对标签的源已选好；选目标连接和 schema，比对：按改名配对，选中表时两边定义对照、不同的行标色。
4. "同步到目标…"：脚本预览（一个事务的提示），执行后再比对无差异。
5. 在库中加列、建表，交换两边比对，"写回模型"：核对表文件内容；再比对无差异。
6. 监听其他地址时模型面板显示只能在本机使用的说明。

- [ ] **Step 37: 提交**

```bash
git add crates/dblens-desktop/src/main.rs crates/dblens-desktop/src/shell.rs web/src/app/Shell.tsx web/src/app/StatusBar.tsx web/src/app/TitleBar.tsx web/src/app/menus.test.ts web/src/app/menus.ts web/src/app/shell.css web/src/app/ui.ts web/src/features/compare/CompareView.tsx web/src/features/compare/SyncDialog.tsx web/src/features/compare/logic.test.ts web/src/features/compare/logic.ts web/src/features/design/DesignView.tsx web/src/features/design/ForeignKeysPane.tsx web/src/features/design/OptionsPane.tsx web/src/features/design/source.ts web/src/features/editor/Workspace.tsx web/src/features/editor/actions.tsx web/src/features/editor/tabs.test.ts web/src/features/editor/tabs.ts web/src/features/model/ModelDialogs.tsx web/src/features/model/ModelsPanel.tsx web/src/features/model/dialogs.ts web/src/features/tree/ObjectTree.tsx web/src/locales/en/app.json web/src/locales/en/model.json web/src/locales/en/tree.json web/src/locales/index.ts web/src/locales/zh-CN/app.json web/src/locales/zh-CN/model.json web/src/locales/zh-CN/tree.json web/src/shared/api.ts web/src/shared/shell.ts
git commit -m "feat(web): 模型面板、比对标签与设计器的模型模式" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 9: 文档

- `docs/notes/m4a-model-compare.md`：结构、模型文件格式、与设计文档不一致的地方、验证。
- `docs/notes/m0-4b-api.md`：模型与比对的接口。
- 总设计文档：M4 拆成三份；M4a 完成。

**Files:**
- Modify: `docs/notes/m0-4b-api.md`
- Create: `docs/notes/m4a-model-compare.md`
- Modify: `docs/superpowers/specs/2026-10-22-dblens-design.md`

- [ ] **Step 1: 修改 `docs/notes/m0-4b-api.md`**

```diff
--- a/docs/notes/m0-4b-api.md
+++ b/docs/notes/m0-4b-api.md
@@ -212,6 +212,38 @@
 - `statements`：`[{kind: "sql", sql, changes} | {kind: "count_check", source, target, changes}]`；`count_check` 是重建时由程序执行的核对行数。
 - 任务失败时 `remaining` 为没有执行的语句（以 `;` 分隔），PG 家族整体回滚时为 null。
 
+### 模型与比对（M4a 加入）
+
+模型目录是服务所在机器上的路径：`/models` 下的接口和 `/compare/script` 只在页面和服务在同一台机器上（桌面程序、只监听本机地址的 `dblens-server`）时可用，否则 403。
+
+| 方法 | 路径 | 说明 |
+|---|---|---|
+| GET | `/models` | `{open: [模型], recent: [目录]}`；模型为 `{id, path, info, read_only}`，`info` 是 `model.yaml` 的内容 |
+| POST | `/models/open` | `{path}`，打开模型目录；已经打开的返回原来的编号 |
+| POST | `/models` | `{path, name, database: {variant, compat?, version?}}`，新建空模型（目录必须不存在或为空），201 |
+| POST | `/models/reverse` | `{profile_id, schemas, path, name}`，反向生成，返回 201 和 `{"id"}`（任务，`kind` 为 `reverse`；结束后 `model` 为打开的模型的编号） |
+| GET / PUT / DELETE | `/models/{id}` | 重新加载并返回模型 / 改 `{name, description, schemas}` / 关闭 |
+| GET | `/models/{id}/caps` | 设计器的能力和类型目录（按模型的数据库类型），格式同 `/profiles/{id}/design/caps` |
+| GET | `/models/{id}/tables` | `[{schema, name, comment?, error?: {path, message}}]`；`error` 是解析失败的文件 |
+| GET / DELETE | `/models/{id}/tables/{schema}/{name}` | `{table, fingerprint}` / 删除表文件（`?fingerprint=` 时核对） |
+| POST | `/models/{id}/tables/{schema}/{name}/rename` | `{to}`，表改名：改文件名、更新其他表外键的引用、记入改名记录 |
+| POST | `/models/{id}/tables/preview` | 设计器的检查：`{errors, warnings, changes, conflict}` |
+| POST | `/models/{id}/tables` | 保存设计器中的表，返回 `{fingerprint}`；有校验错误、文件已被修改时 400 |
+| POST | `/compare` | 比对：`{source, target, options, renames}` → `{diff: {tables, suggestions}, renames, source_errors, target_errors, source_tables, target_tables, fingerprint}` |
+| POST | `/compare/preview` | 同步的预览：`{compare, selected, fingerprint}` → `{script, text, writes, denied, confirm, fingerprint}` |
+| POST | `/compare/sync` | 同步：目标是库时 201 和 `{task}`（任务，`kind` 为 `sync`），目标是模型时 200 和 `{task: null}`（已写回）。比对结果与预览时不同时 400 |
+| POST | `/compare/script` | `{path, text}`，把同步脚本写到 `.sql` 文件（桌面程序的"另存为"） |
+
+- 设计器请求体：`{schema, name, draft, renames, fingerprint}`，含义同 `/profiles/{id}/design/preview`（没有 `rebuild`）。
+- 比对的一边：`{kind: "model", model, schema}` 或 `{kind: "database", profile, schema}`；结果描述"怎样修改目标才与源一致"。
+- `options`：`{ignore_comments, ignore_column_order, ignore_generated_names, case_insensitive}`，`ignore_generated_names` 默认为 true。
+- `renames`：采用的改名，`{tables: {源: 目标}, columns: {源表: {源列: 目标列}}}`；模型中记下的改名自动采用。
+- `diff.tables`：`[{status, source, target, items: [{id, change}], appended}]`，`status` 为 `added` / `removed` / `renamed` / `changed` / `same`；
+  `change` 为 `{kind: "drop_table", name}`、`{kind: "rename_table", from, to}` 或 `{kind: "table", change: 表设计器的变更}`。勾选和同步按 `id`。
+- `diff.suggestions`：推测的改名 `[{table, source, target}]`，`table` 为 null 时是表的改名。
+- `script`（目标是库）：`{steps: [{kind: "sql", sql, items} | {kind: "rebuild", table, rebuild, items}], refused: [[表, 原因]], warnings, transactional}`；
+  `writes`（目标是模型）：`[{name, from?, delete}]`。
+
 ## WebSocket：`GET /api/sessions/{id}/ws?token=…&lang=…`
 
 `lang` 可选，决定事件中文字的语言（见"语言"一节）。
```

- [ ] **Step 2: 新建 `docs/notes/m4a-model-compare.md`**

````markdown
# M4a 模型与比对

依据：设计文档 `docs/superpowers/specs/2026-10-27-m4a-model-compare-design.md`。

## 结构

```text
crates/dblens-dialect/src/compare/
  mod.rs        CompareRenames、CompareOptions、SchemaDiff；compare_schemas（表的配对、推测改名、逐表 diff_table）
  normalize.rs  比对前整理两边：本 schema 的外键引用、改名后的表名、注释、列顺序、系统生成的名字、同义的类型和默认值、外键的默认动作
  apply.rs      apply_changes：把变更应用到表结构（写回模型）
  sync.rs       sync_rebuild_tables、sync_script：同步脚本（删外键 → 删表 → 改名 → 逐表修改或重建 → 建表 → 加外键）
crates/dblens-service/src/model/
  file.rs       model.yaml、表文件的读写（serde-saphyr）、文件名转义、指纹、先写临时文件再改名
  store.rs      ModelStore：打开的模型、表文件缓存（按修改时间和长度重读）、表的保存/改名/删除、改名记录、写回
  ops.rs        最近打开的模型、新建、反向生成任务（TaskKind::Reverse）、设计器的预览和保存
crates/dblens-service/src/compare.rs  读取两边、比对、同步预览、同步执行（TaskKind::Sync）或写回模型
crates/dblens-service/src/design.rs   执行器改为按段执行（Part：一条语句或一张表的重建），M3 与同步共用
crates/dblens-api/src/models.rs       /models、/compare
web/src/features/model/    模型面板、打开/新建/反向生成对话框
web/src/features/compare/  比对标签、同步对话框、勾选与依赖提示等纯逻辑
web/src/features/design/source.ts  设计器选外键引用的数据来源（连接或模型）
```

入口：活动栏"模型"面板；标题栏"文件 → 打开模型 / 新建模型 / 结构比对"；对象树 schema 右键"与模型比对… / 另存为模型…"；
模型面板右键"与库比对…"；设计器（模型模式）工具栏"与库比对"。

## 模型文件

- `model.yaml`：`format`、`name`、`description`、`database {variant, compat, version}`、`schemas`、`renames {schema: {tables, columns}}`；
  不认识的字段保留。`compat` 为原生时不写，`version` 不写时按最新版本。
- `tables/<schema>/<表名>.yaml`：表名、注释、列（`type` 是原生类型；可空为默认，只写 `nullable: false`）、主键、唯一约束、外键、
  检查约束、索引（升序的列只写名字，降序写成 `{name, descending: true}`）、分区。逻辑类型和列序号不写，读取时按方言推出、按顺序编号；
  触发器不写。表和每列不认识的字段读进 `extra`，保存时原样写回。
- 文件名：`/ \ : * ? " < > | %`、控制字符、结尾的点和空格写成 `%XX`；Windows 保留名在第一个字符转义。表名以文件内容为准。
- 指纹：文件内容的哈希。设计器保存、删除、写回前核对。

## 与设计文档不一致的地方

- `dblens-server` 只监听本机地址时 `local_files` 为真：除了能使用模型，导出也可以直接写到页面给出的路径（与桌面程序相同）。
- MySQL 家族不查询 `lower_case_table_names`：名字是否区分大小写由比对选项"名字不区分大小写"决定（默认区分）。
- 新建模型时不选版本（按最新版本）；从库反向生成的模型带库的版本。
- 模型的改名记录在两种情况下删去已应用的项：同步到库（源是模型）的任务成功后；写回模型时（按写回的改名）。
- 比对结果树中一张表的变更按变更列表的顺序平铺（M3 的顺序已大致按列、主键、唯一、外键、检查、索引排列），没有再按类别分组。
- 比对结果另带两边的表（`source_tables`、`target_tables`），界面中对照显示、计算依赖提示。
- 外键的默认动作：`NO ACTION`（MySQL 家族的 `RESTRICT` 与之相同）与不写视为相同。
- 同步脚本另存为：桌面程序经 `POST /compare/script` 写到"另存为"选的 `.sql` 文件（只接受 `.sql`），浏览器模式下载。
- 设计器的模型模式中"SQL 预览"子标签换成"变更"子标签（变更摘要、校验错误、冲突提示），"与库比对"在工具栏。
- 库对库同步执行前也重新比对；预览后结构有变化（指纹不同）时拒绝。实库测试中同一 schema 有其他测试并发改动，不传指纹。

## 验证

- 后端：fmt、clippy 通过；全部测试在 `DBLENS_LANG=en` 下通过（682 个）。
  - dialect：表的配对与推测改名、各选项与等价写法、各家族系统生成的名字、`apply_changes` 与 `diff_table` 互逆、同步脚本的顺序和重建（PG、MySQL 快照）。
  - service：模型目录读写（文件名转义、未知字段保留、坏文件、格式较新时只读、指纹冲突、改名记录与外键引用的更新、写回前核对指纹）；
    设计器保存；反向生成失败时不留下目录；模型对模型比对并写回；预览后目标被改时拒绝同步；执行器多段执行时失败后列出没有执行的语句。
  - api：没有 `local_files` 时禁止；编辑、比对、写回模型的完整流程；脚本只写 `.sql` 文件。
  - 实库（MySQL、MariaDB、PG、Oracle、达梦、TiDB、OceanBase、openGauss PG/A、人大金仓 Oracle/PG 模式）：反向生成 → 在模型中表改名、列改名、
    加列、换索引、加外键 → 与库比对（按记下的改名配对）→ 同步到库 → 再比对无差异；库中加列后写回模型 → 两边一致；
    同一个库中第二个 schema 的库对库比对和同步（Oracle、达梦没有第二个 schema，跳过）。达梦单独运行通过（与其他测试同时运行时偶发连接失败，与以前的记录相同）。
- 前端：tsc、Vitest（勾选的默认值、依赖提示、定义对照、采用推测的改名、比对标签和模型设计器标签的保存与恢复）、打包通过。
- 浏览器实测（openGauss PG，浏览器模式监听 127.0.0.1）：反向生成对话框（从对象树 schema 右键带上连接和 schema）、模型面板的树、
  设计器的模型模式（列改名后的变更列表、保存到文件）、比对标签（按改名配对、两边定义对照标色、交换两边）、同步到库（脚本预览、执行、再比对无差异）、
  库中加列和建表后写回模型（文件内容核对）。
- 未验证：桌面程序中的"选择文件夹"对话框和"在文件管理器中显示"；监听其他地址的浏览器模式下模型面板的提示只有接口测试覆盖。
````

- [ ] **Step 3: 修改 `docs/superpowers/specs/2026-10-22-dblens-design.md`**

```diff
--- a/docs/superpowers/specs/2026-10-22-dblens-design.md
+++ b/docs/superpowers/specs/2026-10-22-dblens-design.md
@@ -330,6 +330,8 @@ trait Policy: Send + Sync {
 - M3 已完成（设计 `docs/superpowers/specs/2026-10-27-m3-table-designer-design.md`，详细计划 `docs/superpowers/plans/2026-10-27-m3-table-designer.md`，说明 `docs/notes/m3-table-designer.md`）。
 - 2026-10-27 用户决定：国产库适配（OceanBase、人大金仓、PolarDB、TiDB、TDSQL、GaussDB、GoldenDB、GBase）作为 M3 之后的单独里程碑，分三批：有 Docker 或免费版的；只有云服务的；需要新驱动或新架构的（GBase 8s、OceanBase 的 Oracle 模式）。
 - 国产库第一批已完成：TiDB、OceanBase 社区版（MySQL 模式）、openGauss（A、PG 兼容模式，含 SHA256 认证）、人大金仓（Oracle、PG 模式）（设计 `docs/superpowers/specs/2026-10-27-domestic-batch1-design.md`，详细计划 `docs/superpowers/plans/2026-10-27-domestic-batch1.md`，说明 `docs/notes/domestic-batch1.md`）。第二批（PolarDB、TDSQL、GaussDB，只有云服务）、第三批（GBase 8s、OceanBase 的 Oracle 模式）尚未开始；GoldenDB 没有可用的测试环境。
+- 2026-10-27 M4 拆成三份（用户确认）：M4a 模型与比对（模型文件、反向生成、比对与同步）；M4b ER 图；M4c 数据字典和设计文档导出。
+- M4a 已完成（设计 `docs/superpowers/specs/2026-10-27-m4a-model-compare-design.md`，详细计划 `docs/superpowers/plans/2026-10-27-m4a-model-compare.md`，说明 `docs/notes/m4a-model-compare.md`）。M4b、M4c 尚未开始。
 
 ## 12. 风险与待验证项
 
```

- [ ] **Step 4: 检查**

检查文档中的路径和接口与代码一致；按执行时的实际结果更新"验证"一节（测试数、浏览器实测的内容）。

Expected: 无。

- [ ] **Step 5: 提交**

```bash
git add docs/notes/m0-4b-api.md docs/notes/m4a-model-compare.md docs/superpowers/specs/2026-10-22-dblens-design.md
git commit -m "docs: M4a 模型与比对的说明、接口和进度" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```
