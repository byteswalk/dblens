# M5a 结构迁移 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把一个 schema 中的表和序列迁到另一个连接（可以是不同的数据库家族）的 schema：按逻辑类型映射、可人工调整、两步建表和约束、同名表逐表处理、生成迁移报告。

**Architecture:** 方言层（纯逻辑）把源结构按目标方言转换成目标结构并给出逐项提示（等价 / 有损 / 不支持），再复用表设计器的 DDL 生成写出两步脚本；服务层读两端结构、按迁移方案预览和检查，作为后台任务逐条执行并写报告；前端迁移标签页编辑迁移方案（随标签保存，可另存为 JSON）。

**Tech Stack:** Rust（axum、serde、insta）；React 19 + TypeScript、antd 6、zustand、TanStack Query、Monaco、Vitest。

**Spec:** `docs/superpowers/specs/2026-10-28-m5a-structure-migration-design.md`

## Global Constraints

- 任意库到任意库（MySQL 系、PG 系、Oracle、达梦），经逻辑类型中转；实库测试重点覆盖信创替换的常见路线。
- 对象：表 + 序列自动转换；视图、例程、触发器、同义词列进报告并附源库 DDL。
- 人工调整：映射表格（全局类型规则 + 逐表逐列覆盖），只读的目标 DDL 预览；结构和数据（M5b）共用同一份迁移方案。
- 目标中已有同名表：必须逐表（可批量）选择处理方式，默认停下；不支持的项处理之前不能开始。
- 不新建目标 schema 或用户；字符集、排序规则不迁；分区只提示。
- 依赖只用正式发布版，跟随最新；新版发布满 7 天再升级（有意固定版本的记入 `scripts/check-deps.mjs` 的 `HOLD`）。
- 前端给用户看的文字都用词条（中英文），英文区分单复数；后端错误和提示用 `msg!` 词条（`src` 中的中文字面量只能出现在 `#[cfg(test)]` 之后，单独的测试文件中也不能有）；日志不翻译。
- 代码注释用中文；文件 UTF-8、LF 换行；前端代码风格：无分号、单引号、两格缩进。
- cargo 命令加 `--config "source.mirror.registry='sparse+https://rsproxy.cn/index/'"`（clippy 写在 `clippy` 之后），并设 `CARGO_INCREMENTAL=0`；后端测试在 `DBLENS_LANG=en` 下运行。
- 实库测试一律串行（`--test-threads=1`），不加 `--include-ignored`（会跑达梦的 spike 测试并卡住）。
- 前端工具用 PowerShell 执行（`npm`/`npx` 在本机不可用，用 `pnpm exec`）。
- 测试数据库的密码只在命令行中以内联环境变量传入，不写进仓库、报告或提交信息。

## 与设计文档的差异（写计划时实测后调整）

- 接口只有 `/migrations/preview` 和 `/migrations` 两个："只生成脚本"用预览中的脚本全文另存。
- 方案多了 `sequences`（迁移的序列范围）；目标中已有同名序列时自动跳过。
- PG 系（含兼容 Oracle 的模式）的名字默认小写。
- 同家族的不同产品：类型原样，默认值先尝试转换、转换不了再原样。
- 系统生成的约束名不带过去；MySQL 为外键自动建的同名索引不迁；Oracle 的 `NUMBER(p≤19)` 迁到其他库用整数，自增列一律用整数。
- TiDB、OceanBase 的 `CURRENT_TIMESTAMP` 默认值原来被读成字符串，读结构的 SQL 一并修正。

## 执行说明

任务 2–5 的代码都已在临时副本中逐任务实现并通过检查，执行时按顺序重放。新文件给出全文，修改的文件给出 diff（基于上一个任务完成后的状态，可直接 `git apply`）。`Cargo.lock`、`web/pnpm-lock.yaml` 没有变化。

写计划时已运行：全部后端测试（761 个）和前端测试（271 个）；44 条迁移路线的实库测试（11 个库）；浏览器中 TiDB → openGauss 的迁移。

## 文件结构

```text
crates/dblens-dialect/src/migrate/{mod.rs, names.rs, types.rs, expr.rs, script.rs, tests.rs, snapshots/}（新增）
crates/dblens-dialect/src/lib.rs, sql/{mysql.sql, tidb.sql}, tests/snapshots/meta_snapshots__{mysql,oceanbase,tidb}.snap
crates/dblens-service/src/{migrate.rs（新增）, lib.rs, task.rs, design.rs, compare.rs}, tests/migrate_live.rs（新增）
crates/dblens-api/src/{migrations.rs（新增）, lib.rs}, tests/models.rs
crates/dblens-core/locales/app.json
web/src/features/migrate/{plan.ts, plan.test.ts, MigrateView.tsx}（新增）
web/src/{shared/api.ts, app/shell.css, features/editor/{tabs.ts, Workspace.tsx}, features/tree/ObjectTree.tsx, locales/{zh-CN,en}/{migrate.json（新增）, tree.json}, locales/index.ts}
docs/notes/{m5a-structure-migration.md（新增）, m0-4b-api.md}; docs/superpowers/specs/2026-10-22-dblens-design.md
```

---

### Task 1: 依赖复查

按依赖原则（M1b2 设计文档 3.1 节）复查前端和后端的全部直接依赖。本里程碑不新增依赖。

**Files:** 视复查结果而定（`web/package.json`、`Cargo.toml` 及锁文件）。

- [ ] **Step 1: 运行复查脚本**

本机经代理上网，Node 的 `fetch` 默认不读 `HTTPS_PROXY`，必须先设 `NODE_USE_ENV_PROXY=1`，否则全部查询失败，最后仍显示"0 个可升级"。

```powershell
$env:PATH = "D:\Environments\nvm4w\nodejs;$env:PATH"; $env:NODE_USE_ENV_PROXY = '1'; $env:HTTPS_PROXY = 'http://127.0.0.1:6789'; node scripts/check-deps.mjs
```

Expected：输出中没有"查询失败"（写计划时 86 个直接依赖都不用升级：16 个有更新，但发布都不满 7 天，`html-to-image` 在"有意不升级"名单中）。以执行当天的结果为准。

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

### Task 2: 方言层：结构迁移的转换与两步脚本

- `dblens-dialect/src/migrate/`（新）：
  - `mod.rs`：`MigrationOptions { name_case, comments, checks }`、`TypeRule`、`ColumnOverride`、`TableOverride`、`SequenceOverride`、`MigrationSettings`；提示 `MigrationNote { kind, object, column, item, level: NoteLevel::{Info, Lossy, Unsupported}, message }`；结果 `MigrationDraft { tables: Vec<TableMapping>, sequences: Vec<SequenceMapping>, notes }`；`migrate(&MigrationInput, &MigrationSettings)`：序列（PG serial 自带的、Oracle 的 `ISEQ$$_` 不迁；MySQL 系目标没有序列）、表名和列名（覆盖或按大小写选项）、列（类型、生成列、自增、默认值）、主键、唯一约束、索引（MySQL 为外键自动建的同名索引不迁；表达式索引；大字段列；索引方法）、外键（引用范围外且目标中没有的表不建；目标不支持的动作去掉）、检查约束（覆盖或转换）、分区提示；系统生成的约束名不带过去。
  - `names.rs`：`NameCase::{Keep, Lower, Upper}` 和 `for_target`（Oracle、达梦大写，PG 系小写，MySQL 系保持）、`max_ident_len`、重名和超长检查（表、序列、PG/Oracle/达梦的索引在 schema 中唯一）。
  - `types.rs`：列类型（覆盖 → 规则 → 同家族原样 → 经逻辑类型，`Other` 不支持、确认后按文本）；有损判断（等价的容差：TEXT≈无长度 VARCHAR、PG 的 BYTEA、整数≈NUMBER(p)、UUID≈CHAR(36)、Oracle 的 DATE 容纳日期）；Oracle 的 `NUMBER(p≤19)` 迁到其他库用整数；自增列用整数；能否建标识列、外键动作、序列的 bigint 范围。
  - `expr.rs`：记号化与规整（去掉 `::类型`、`_utf8mb4` 前缀，`= ANY (ARRAY[…])` → `IN (…)`）、`convert_expr`（简单表达式：列名换成目标名、字符串按目标写字面量，Oracle/达梦的 TRUE/FALSE 写 1/0）、`convert_default`（字面量、布尔、当前时间和日期、UUID 函数、序列 nextval）、`nextval_target`。
  - `script.rs`：`ExistingAction::{Skip, Recreate, MissingOnly}`、`MigrationStatement { phase, kind, object, requires, sql }`、`migration_script`（建表和约束复用表设计器的 `design_sql`；序列按目标写法；只建缺的按列比较）、`script_text`。
  - `tests.rs` 与 `snapshots/`：PG 源迁到 MySQL、Oracle、达梦、openGauss，Oracle 源迁到 PG、MySQL 的快照，及覆盖、重名、同名表的测试。
- `lib.rs` 加 `pub mod migrate`。
- 读结构的修正：`sql/mysql.sql`（OceanBase 共用）、`sql/tidb.sql` 的列默认值——时间列的 `CURRENT_TIMESTAMP…` 按表达式读出（这两个库的 `EXTRA` 中没有 `DEFAULT_GENERATED`）；`tests/snapshots/meta_snapshots__{mysql,oceanbase,tidb}.snap` 随之更新。
- 后端词条加 `migrate.*`（22 个提示）。

快照文件（`snapshots/`）随本任务给出；改了生成逻辑时用 `INSTA_UPDATE=always` 重新生成并逐个审查差异。

**Files:**
- Modify: `crates/dblens-core/locales/app.json`
- Modify: `crates/dblens-dialect/sql/mysql.sql`
- Modify: `crates/dblens-dialect/sql/tidb.sql`
- Modify: `crates/dblens-dialect/src/lib.rs`
- Create: `crates/dblens-dialect/src/migrate/expr.rs`
- Create: `crates/dblens-dialect/src/migrate/mod.rs`
- Create: `crates/dblens-dialect/src/migrate/names.rs`
- Create: `crates/dblens-dialect/src/migrate/script.rs`
- Create: `crates/dblens-dialect/src/migrate/snapshots/dblens_dialect__migrate__tests__oracle_to_mysql.snap`
- Create: `crates/dblens-dialect/src/migrate/snapshots/dblens_dialect__migrate__tests__oracle_to_pg.snap`
- Create: `crates/dblens-dialect/src/migrate/snapshots/dblens_dialect__migrate__tests__pg_to_dameng.snap`
- Create: `crates/dblens-dialect/src/migrate/snapshots/dblens_dialect__migrate__tests__pg_to_mysql.snap`
- Create: `crates/dblens-dialect/src/migrate/snapshots/dblens_dialect__migrate__tests__pg_to_opengauss.snap`
- Create: `crates/dblens-dialect/src/migrate/snapshots/dblens_dialect__migrate__tests__pg_to_oracle.snap`
- Create: `crates/dblens-dialect/src/migrate/tests.rs`
- Create: `crates/dblens-dialect/src/migrate/types.rs`
- Modify: `crates/dblens-dialect/tests/snapshots/meta_snapshots__mysql.snap`
- Modify: `crates/dblens-dialect/tests/snapshots/meta_snapshots__oceanbase.snap`
- Modify: `crates/dblens-dialect/tests/snapshots/meta_snapshots__tidb.snap`

**Interfaces:**
- Consumes：M3 的 `Dialect::design_sql`、`Diff`、`TableChange`、`design::diff::{is_expression, text}`；`types::parse_type`、`Dialect::{logical_type, native_type, ident, literal, sequence_ddl}`。
- Produces（`dblens_dialect::migrate`）：上面的类型；`migrate(input: &MigrationInput, settings: &MigrationSettings) -> MigrationDraft`、`MigrationDraft::blocked()`、`migration_script(tgt: &Dialect, schema, draft, existing: &BTreeMap<String, ExistingAction>, target_tables: &[Table]) -> Result<Vec<MigrationStatement>>`、`script_text(&[MigrationStatement]) -> String`、`convert_expr`、`NameCase`、`max_ident_len`。

- [ ] **Step 1: 新建 `crates/dblens-dialect/src/migrate/tests.rs`**

```rust
use std::collections::BTreeMap;

use dblens_core::schema::{
    CheckConstraint, Column, ForeignKey, Index, IndexColumn, PrimaryKey, Sequence, Table, UniqueConstraint,
};
use dblens_core::{ColumnType, Lang, LogicalType};

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

fn idx(name: &str, cols: &[&str]) -> Index {
    Index {
        name: name.into(),
        columns: cols.iter().map(|c| IndexColumn { name: c.to_string(), descending: false }).collect(),
        unique: false,
        method: None,
    }
}

/// PG 中的源结构：serial 主键、各种默认值、数组列、检查约束、表达式索引、外键（一个引用范围外的表）、生成列、序列。
fn pg_source() -> (Vec<Table>, Vec<Sequence>) {
    let mut id = col("id", "bigint");
    id.nullable = false;
    id.auto_increment = true;
    id.default = Some("nextval('customers_id_seq'::regclass)".into());
    let mut name = col("name", "character varying(50)");
    name.nullable = false;
    name.default = Some("'n/a'::character varying".into());
    name.comment = Some("Customer name".into());
    let mut active = col("active", "boolean");
    active.default = Some("true".into());
    let mut created = col("created", "timestamp(6) without time zone");
    created.default = Some("now()".into());
    let mut balance = col("balance", "numeric(12,2)");
    balance.default = Some("0".into());
    let customers = Table {
        name: "customers".into(),
        comment: Some("Customers".into()),
        columns: vec![
            id,
            name,
            col("email", "character varying(100)"),
            active,
            created,
            balance,
            col("tags", "text[]"),
            col("note", "text"),
        ],
        primary_key: Some(PrimaryKey { name: Some("customers_pkey".into()), columns: vec!["id".into()] }),
        uniques: vec![UniqueConstraint {
            name: Some("customers_email_key".into()),
            columns: vec!["email".into()],
        }],
        checks: vec![CheckConstraint {
            name: Some("ck_balance".into()),
            expression: "(balance >= (0)::numeric)".into(),
        }],
        indexes: vec![
            idx("ix_customers_name", &["name"]),
            Index { method: Some("gin".into()), ..idx("ix_customers_note", &["note"]) },
            idx("ix_customers_lower", &["lower((email)::text)"]),
        ],
        ..Default::default()
    };
    let mut oid = col("id", "integer");
    oid.nullable = false;
    oid.default = Some("nextval('order_seq'::regclass)".into());
    let mut status = col("status", "character varying(10)");
    status.default = Some("'new'::character varying".into());
    let mut placed = col("placed", "date");
    placed.default = Some("CURRENT_DATE".into());
    let mut code = col("code", "uuid");
    code.default = Some("gen_random_uuid()".into());
    let mut double = col("double_amount", "numeric(12,2)");
    double.generated = Some("(amount * (2)::numeric)".into());
    let orders = Table {
        name: "orders".into(),
        columns: vec![
            oid,
            col("customer_id", "bigint"),
            col("product_id", "integer"),
            col("amount", "numeric(10,2)"),
            double,
            status,
            placed,
            code,
            col("pos", "point"),
        ],
        primary_key: Some(PrimaryKey { name: Some("orders_pkey".into()), columns: vec!["id".into()] }),
        foreign_keys: vec![
            ForeignKey {
                name: Some("fk_orders_customer".into()),
                columns: vec!["customer_id".into()],
                ref_schema: None,
                ref_table: "customers".into(),
                ref_columns: vec!["id".into()],
                on_delete: Some("CASCADE".into()),
                on_update: Some("CASCADE".into()),
            },
            ForeignKey {
                name: Some("fk_orders_product".into()),
                columns: vec!["product_id".into()],
                ref_schema: None,
                ref_table: "products".into(),
                ref_columns: vec!["id".into()],
                on_delete: None,
                on_update: None,
            },
        ],
        checks: vec![CheckConstraint {
            name: Some("ck_status".into()),
            expression: "((status)::text = ANY ((ARRAY['new'::character varying, 'paid'::character varying])::text[]))"
                .into(),
        }],
        ..Default::default()
    };
    let seq = |name: &str, max: &str, cache| Sequence {
        name: name.into(),
        start: Some("1".into()),
        increment: Some("1".into()),
        min_value: Some("1".into()),
        max_value: Some(max.into()),
        cycle: false,
        cache: Some(cache),
    };
    (
        vec![customers, orders],
        vec![seq("customers_id_seq", "9223372036854775807", 1), seq("order_seq", "9223372036854775807", 20)],
    )
}

fn input<'a>(
    src: Variant,
    tgt: Variant,
    tables: &'a [Table],
    sequences: &'a [Sequence],
) -> MigrationInput<'a> {
    MigrationInput {
        source: Dialect::new(src),
        target: Dialect::new(tgt),
        tables,
        sequences,
        target_tables: &[],
        target_sequences: &[],
    }
}

fn render(draft: &MigrationDraft, tgt: Variant) -> String {
    let mut out = String::new();
    for n in &draft.notes {
        out += &format!(
            "{:?} {}{}{}: {}\n",
            n.level,
            n.object,
            n.column.as_ref().map(|c| format!(".{c}")).unwrap_or_default(),
            n.item.as_ref().map(|c| format!(" [{c}]")).unwrap_or_default(),
            n.message.render(Lang::En)
        );
    }
    let script = migration_script(&Dialect::new(tgt), "app", draft, &BTreeMap::new(), &[]).unwrap();
    out + "\n" + &script_text(&script)
}

/// 不认识的类型都确认按文本迁移。
fn accept_unknown(settings: &mut MigrationSettings) {
    for (t, c) in [("customers", "tags"), ("orders", "pos")] {
        settings
            .tables
            .entry(t.into())
            .or_default()
            .columns
            .insert(c.into(), ColumnOverride { accept: true, ..Default::default() });
    }
}

#[test]
fn pg_to_each_family() {
    let (tables, sequences) = pg_source();
    let mut settings = MigrationSettings::default();
    accept_unknown(&mut settings);
    for (tgt, name) in [
        (Variant::MySql, "pg_to_mysql"),
        (Variant::Oracle, "pg_to_oracle"),
        (Variant::Dameng, "pg_to_dameng"),
        (Variant::OpenGauss, "pg_to_opengauss"),
    ] {
        let draft = migrate(&input(Variant::Postgres, tgt, &tables, &sequences), &settings);
        assert!(!draft.blocked(), "{name}");
        insta::assert_snapshot!(name, render(&draft, tgt));
    }
}

#[test]
fn unknown_types_block_until_accepted() {
    let (tables, sequences) = pg_source();
    let draft = migrate(
        &input(Variant::Postgres, Variant::MySql, &tables, &sequences),
        &MigrationSettings::default(),
    );
    let blocked: Vec<_> = draft
        .notes
        .iter()
        .filter(|n| n.level == NoteLevel::Unsupported)
        .map(|n| format!("{}.{}", n.object, n.column.as_deref().unwrap_or("")))
        .collect();
    assert_eq!(blocked, ["customers.tags", "orders.pos"]);
    assert!(draft.blocked());
}

/// Oracle 源：DATE、NUMBER、CLOB、标识列、序列默认值、系统序列。
#[test]
fn oracle_to_pg_and_mysql() {
    let mut id = col("ID", "NUMBER(10)");
    id.nullable = false;
    id.auto_increment = true;
    let mut created = col("CREATED", "DATE");
    created.default = Some("SYSDATE".into());
    let mut no = col("NO", "NUMBER(19)");
    no.default = Some("\"APP\".\"DOC_SEQ\".\"NEXTVAL\"".into());
    let mut flag = col("FLAG", "NUMBER(1)");
    flag.default = Some("0".into());
    let docs = Table {
        name: "DOCS".into(),
        columns: vec![
            id,
            no,
            col("TITLE", "VARCHAR2(200 CHAR)"),
            col("BODY", "CLOB"),
            created,
            flag,
            col("PRICE", "NUMBER"),
        ],
        primary_key: Some(PrimaryKey { name: Some("PK_DOCS".into()), columns: vec!["ID".into()] }),
        indexes: vec![idx("IX_DOCS_BODY", &["BODY"]), idx("IX_DOCS_TITLE", &["TITLE"])],
        checks: vec![CheckConstraint {
            name: Some("CK_FLAG".into()),
            expression: "\"FLAG\" IN (0,1)".into(),
        }],
        ..Default::default()
    };
    let seqs = vec![
        Sequence {
            name: "DOC_SEQ".into(),
            start: Some("100".into()),
            increment: Some("1".into()),
            min_value: Some("1".into()),
            max_value: Some("9999999999999999999999999999".into()),
            cycle: false,
            cache: Some(20),
        },
        Sequence {
            name: "ISEQ$$_12345".into(),
            start: Some("1".into()),
            increment: Some("1".into()),
            min_value: None,
            max_value: None,
            cycle: false,
            cache: None,
        },
    ];
    let tables = [docs];
    let mut settings = MigrationSettings::default();
    settings.type_rules.push(TypeRule { source: "NUMBER(1)".into(), target: "BOOLEAN".into() });
    let draft = migrate(&input(Variant::Oracle, Variant::Postgres, &tables, &seqs), &settings);
    insta::assert_snapshot!("oracle_to_pg", render(&draft, Variant::Postgres));
    let draft =
        migrate(&input(Variant::Oracle, Variant::MySql, &tables, &seqs), &MigrationSettings::default());
    insta::assert_snapshot!("oracle_to_mysql", render(&draft, Variant::MySql));
}

#[test]
fn overrides_names_and_skips() {
    let (tables, sequences) = pg_source();
    let mut settings = MigrationSettings::default();
    settings.options.name_case = Some(NameCase::Keep);
    settings.options.checks = false;
    settings.options.comments = false;
    settings.sequences.insert("order_seq".into(), SequenceOverride { skip: true, ..Default::default() });
    let ov = settings.tables.entry("customers".into()).or_default();
    ov.name = Some("Client".into());
    ov.columns.insert("tags".into(), ColumnOverride { skip: true, ..Default::default() });
    ov.columns.insert(
        "note".into(),
        ColumnOverride { data_type: Some("VARCHAR2(4000)".into()), ..Default::default() },
    );
    ov.columns
        .insert("created".into(), ColumnOverride { default: Some(String::new()), ..Default::default() });
    settings.tables.insert("orders".into(), TableOverride { skip: true, ..Default::default() });
    let draft = migrate(&input(Variant::Postgres, Variant::Oracle, &tables, &sequences), &settings);
    assert!(!draft.blocked());
    assert_eq!(draft.tables.len(), 1);
    assert!(draft.sequences.is_empty());
    let t = &draft.tables[0].target;
    assert_eq!(t.name, "Client");
    assert!(t.comment.is_none() && t.checks.is_empty());
    assert_eq!(
        t.columns.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(),
        ["id", "name", "email", "active", "created", "balance", "note"]
    );
    assert_eq!(t.columns[6].data_type.native, "VARCHAR2(4000)");
    assert_eq!(t.columns[4].default, None);
    assert_eq!(draft.tables[0].columns[6], ColumnMapping { source: "tags".into(), target: None });
}

#[test]
fn name_conflicts_and_length() {
    let t = |name: &str| Table {
        name: name.into(),
        columns: vec![col("a", "int"), col("A", "int")],
        ..Default::default()
    };
    let tables = [t("Item"), t("ITEM"), t(&"x".repeat(70))];
    let draft =
        migrate(&input(Variant::MySql, Variant::Postgres, &tables, &[]), &MigrationSettings::default());
    let msgs: Vec<String> = draft
        .notes
        .iter()
        .filter(|n| n.level == NoteLevel::Unsupported)
        .map(|n| {
            format!(
                "{}{} {}",
                n.object,
                n.column.as_ref().map(|c| format!(".{c}")).unwrap_or_default(),
                n.message.render(Lang::En)
            )
        })
        .collect();
    assert_eq!(msgs.len(), 5, "{msgs:#?}");
    assert!(msgs[0].starts_with("Item.A Name a conflicts with a"), "{msgs:#?}");
    assert!(msgs.iter().any(|m| m.starts_with("ITEM Name item conflicts with Item")), "{msgs:#?}");
    assert!(msgs.iter().any(|m| m.contains("exceeds")), "{msgs:#?}");
}

#[test]
fn existing_tables_follow_the_chosen_action() {
    let (tables, sequences) = pg_source();
    let mut settings = MigrationSettings::default();
    accept_unknown(&mut settings);
    let existing = Table {
        name: "CUSTOMERS".into(),
        columns: vec![col("ID", "NUMBER(19)")],
        indexes: vec![idx("IX_OLD_NAME", &["NAME"])],
        uniques: vec![UniqueConstraint { name: Some("UK_OLD".into()), columns: vec!["EMAIL".into()] }],
        ..Default::default()
    };
    let target_tables = [existing];
    let inp = MigrationInput {
        target_tables: &target_tables,
        target_sequences: &["ORDER_SEQ".to_string()],
        ..input(Variant::Postgres, Variant::Oracle, &tables, &sequences)
    };
    let draft = migrate(&inp, &settings);
    assert!(draft.tables[0].exists && !draft.tables[1].exists);
    assert!(draft.sequences[0].exists);
    let d = Dialect::new(Variant::Oracle);
    let kinds = |action: Option<ExistingAction>| {
        let existing: BTreeMap<_, _> = action.into_iter().map(|a| ("customers".to_string(), a)).collect();
        migration_script(&d, "APP", &draft, &existing, &target_tables)
            .unwrap()
            .iter()
            .filter(|s| s.object == "customers")
            .map(|s| format!("{:?}", s.kind))
            .collect::<Vec<_>>()
            .join(" ")
    };
    assert_eq!(kinds(None), "");
    assert_eq!(kinds(Some(ExistingAction::Skip)), "");
    assert_eq!(
        kinds(Some(ExistingAction::Recreate)),
        "DropTable CreateTable Comment Comment Index Unique Check"
    );
    assert_eq!(kinds(Some(ExistingAction::MissingOnly)), "Check");
    // 外键要求两张表都建好
    let script = migration_script(&d, "APP", &draft, &BTreeMap::new(), &target_tables).unwrap();
    let fk = script.iter().find(|s| s.kind == StatementKind::ForeignKey).unwrap();
    assert_eq!(fk.requires, ["orders", "customers"]);
}
```

- [ ] **Step 2: 修改 `crates/dblens-dialect/tests/snapshots/meta_snapshots__mysql.snap`**

```diff
--- a/crates/dblens-dialect/tests/snapshots/meta_snapshots__mysql.snap
+++ b/crates/dblens-dialect/tests/snapshots/meta_snapshots__mysql.snap
@@ -53,6 +53,8 @@ SELECT c.TABLE_NAME AS table_name,
          WHEN c.COLUMN_DEFAULT IS NULL THEN NULL
          WHEN c.GENERATION_EXPRESSION <> '' THEN NULL
          WHEN c.EXTRA LIKE '%DEFAULT\_GENERATED%' THEN c.COLUMN_DEFAULT
+         -- OceanBase 的 EXTRA 中没有 DEFAULT_GENERATED：时间列的 CURRENT_TIMESTAMP 默认值按表达式处理
+         WHEN c.DATA_TYPE IN ('datetime', 'timestamp') AND UPPER(c.COLUMN_DEFAULT) LIKE 'CURRENT\_TIMESTAMP%' THEN c.COLUMN_DEFAULT
          WHEN c.DATA_TYPE IN ('tinyint', 'smallint', 'mediumint', 'int', 'bigint', 'decimal', 'float', 'double', 'year', 'bit') THEN c.COLUMN_DEFAULT
          ELSE CONCAT('''', REPLACE(REPLACE(c.COLUMN_DEFAULT, '\\', '\\\\'), '''', ''''''), '''')
        END AS default_value,
```

- [ ] **Step 3: 修改 `crates/dblens-dialect/tests/snapshots/meta_snapshots__oceanbase.snap`**

```diff
--- a/crates/dblens-dialect/tests/snapshots/meta_snapshots__oceanbase.snap
+++ b/crates/dblens-dialect/tests/snapshots/meta_snapshots__oceanbase.snap
@@ -53,6 +53,8 @@ SELECT c.TABLE_NAME AS table_name,
          WHEN c.COLUMN_DEFAULT IS NULL THEN NULL
          WHEN c.GENERATION_EXPRESSION <> '' THEN NULL
          WHEN c.EXTRA LIKE '%DEFAULT\_GENERATED%' THEN c.COLUMN_DEFAULT
+         -- OceanBase 的 EXTRA 中没有 DEFAULT_GENERATED：时间列的 CURRENT_TIMESTAMP 默认值按表达式处理
+         WHEN c.DATA_TYPE IN ('datetime', 'timestamp') AND UPPER(c.COLUMN_DEFAULT) LIKE 'CURRENT\_TIMESTAMP%' THEN c.COLUMN_DEFAULT
          WHEN c.DATA_TYPE IN ('tinyint', 'smallint', 'mediumint', 'int', 'bigint', 'decimal', 'float', 'double', 'year', 'bit') THEN c.COLUMN_DEFAULT
          ELSE CONCAT('''', REPLACE(REPLACE(c.COLUMN_DEFAULT, '\\', '\\\\'), '''', ''''''), '''')
        END AS default_value,
```

- [ ] **Step 4: 修改 `crates/dblens-dialect/tests/snapshots/meta_snapshots__tidb.snap`**

```diff
--- a/crates/dblens-dialect/tests/snapshots/meta_snapshots__tidb.snap
+++ b/crates/dblens-dialect/tests/snapshots/meta_snapshots__tidb.snap
@@ -68,6 +68,8 @@ SELECT c.TABLE_NAME AS table_name,
          WHEN c.COLUMN_DEFAULT IS NULL THEN NULL
          WHEN c.GENERATION_EXPRESSION <> '' THEN NULL
          WHEN c.EXTRA LIKE '%DEFAULT\_GENERATED%' THEN c.COLUMN_DEFAULT
+         -- TiDB 的 EXTRA 中没有 DEFAULT_GENERATED：时间列的 CURRENT_TIMESTAMP 默认值按表达式处理
+         WHEN c.DATA_TYPE IN ('datetime', 'timestamp') AND UPPER(c.COLUMN_DEFAULT) LIKE 'CURRENT\_TIMESTAMP%' THEN c.COLUMN_DEFAULT
          WHEN c.DATA_TYPE IN ('tinyint', 'smallint', 'mediumint', 'int', 'bigint', 'decimal', 'float', 'double', 'year', 'bit') THEN c.COLUMN_DEFAULT
          ELSE CONCAT('''', REPLACE(REPLACE(c.COLUMN_DEFAULT, '\\', '\\\\'), '''', ''''''), '''')
        END AS default_value,
```

- [ ] **Step 5: 修改 `crates/dblens-core/locales/app.json`**

```diff
--- a/crates/dblens-core/locales/app.json
+++ b/crates/dblens-core/locales/app.json
@@ -150,6 +150,28 @@
   "metadata.not_source": {"zh-CN": "%{key} 不是源码查询", "en": "%{key} is not a source query"},
   "metadata.pool_closed": {"zh-CN": "连接池已关闭", "en": "The connection pool is closed"},
   "metadata.table_detail_count": {"zh-CN": "表明细查询数量不对", "en": "Wrong number of table detail queries"},
+  "migrate.check_dropped": {"zh-CN": "检查约束 %{expression} 不能转换，不迁移", "en": "Check constraint %{expression} cannot be converted; not migrated"},
+  "migrate.default_dropped": {"zh-CN": "默认值 %{default} 不能转换，不迁移", "en": "Default %{default} cannot be converted; not migrated"},
+  "migrate.fk_action": {"zh-CN": "目标库不支持 %{action}，去掉", "en": "The target does not support %{action}; dropped"},
+  "migrate.fk_outside": {"zh-CN": "引用的表 %{table} 不在迁移范围内，目标中也没有，不迁移", "en": "Referenced table %{table} is neither migrated nor in the target; not migrated"},
+  "migrate.generated_dropped": {"zh-CN": "生成列表达式 %{expression} 不能转换，改为普通列", "en": "Generated column expression %{expression} cannot be converted; migrated as a plain column"},
+  "migrate.identity_unsupported": {"zh-CN": "目标库版本不支持自增（标识列）", "en": "The target database version does not support identity columns"},
+  "migrate.index_expression": {"zh-CN": "索引表达式 %{expression} 不能转换，不迁移", "en": "Index expression %{expression} cannot be converted; not migrated"},
+  "migrate.index_method": {"zh-CN": "目标库不支持索引方法 %{method}，按普通索引建", "en": "The target does not support index method %{method}; created as a plain index"},
+  "migrate.item_column_skipped": {"zh-CN": "用到的列没有迁移，不迁移", "en": "Uses a column that is not migrated; not migrated"},
+  "migrate.key_column_skipped": {"zh-CN": "主键的列没有迁移", "en": "A primary key column is not migrated"},
+  "migrate.lob_index": {"zh-CN": "包含大字段列，目标库不能建索引，不迁移", "en": "Includes a large-object column the target cannot index; not migrated"},
+  "migrate.lob_key": {"zh-CN": "主键包含大字段列，目标库不能建", "en": "The primary key includes a large-object column, which the target cannot index"},
+  "migrate.name_conflict": {"zh-CN": "名字 %{name} 与 %{other} 重名", "en": "Name %{name} conflicts with %{other}"},
+  "migrate.name_too_long": {"zh-CN": "名字 %{name} 超过目标库的长度上限", "en": "Name %{name} exceeds the target's length limit"},
+  "migrate.no_sequences": {"zh-CN": "目标库没有序列，不迁移", "en": "The target database has no sequences; not migrated"},
+  "migrate.partition": {"zh-CN": "分区不迁移，按普通表建", "en": "Partitioning is not migrated; created as a plain table"},
+  "migrate.sequence_bound": {"zh-CN": "%{bound} 超出 bigint 范围", "en": "%{bound} is outside the bigint range"},
+  "migrate.sequence_exists": {"zh-CN": "目标中已有同名序列，跳过", "en": "A sequence with this name already exists in the target; skipped"},
+  "migrate.serial_sequence": {"zh-CN": "serial 列自带的序列，随自增列一起迁移", "en": "Sequence owned by a serial column; migrated with the identity column"},
+  "migrate.type_lossy": {"zh-CN": "%{from} 映射为 %{to}，取值范围或精度可能改变", "en": "%{from} maps to %{to}; range or precision may change"},
+  "migrate.type_unknown": {"zh-CN": "不认识的类型 %{source}，按文本（%{target}）迁移", "en": "Unrecognized type %{source}; migrated as text (%{target})"},
+  "migrate.unknown_target_type": {"zh-CN": "目标库不认识类型 %{native}", "en": "The target does not recognize type %{native}"},
   "model.bad_file": {"zh-CN": "文件 %{path} 有错：%{error}", "en": "File %{path} is invalid: %{error}"},
   "model.bad_path": {"zh-CN": "路径无效：%{path}", "en": "Invalid path: %{path}"},
   "model.conflict": {"zh-CN": "%{name} 的文件已被修改，请重新加载", "en": "The file for %{name} has changed; reload it"},
```

- [ ] **Step 6: 修改 `crates/dblens-dialect/sql/mysql.sql`**

```diff
--- a/crates/dblens-dialect/sql/mysql.sql
+++ b/crates/dblens-dialect/sql/mysql.sql
@@ -64,6 +64,8 @@ SELECT c.TABLE_NAME AS table_name,
          WHEN c.COLUMN_DEFAULT IS NULL THEN NULL
          WHEN c.GENERATION_EXPRESSION <> '' THEN NULL
          WHEN c.EXTRA LIKE '%DEFAULT\_GENERATED%' THEN c.COLUMN_DEFAULT
+         -- OceanBase 的 EXTRA 中没有 DEFAULT_GENERATED：时间列的 CURRENT_TIMESTAMP 默认值按表达式处理
+         WHEN c.DATA_TYPE IN ('datetime', 'timestamp') AND UPPER(c.COLUMN_DEFAULT) LIKE 'CURRENT\_TIMESTAMP%' THEN c.COLUMN_DEFAULT
          WHEN c.DATA_TYPE IN ('tinyint', 'smallint', 'mediumint', 'int', 'bigint', 'decimal', 'float', 'double', 'year', 'bit') THEN c.COLUMN_DEFAULT
          ELSE CONCAT('''', REPLACE(REPLACE(c.COLUMN_DEFAULT, '\\', '\\\\'), '''', ''''''), '''')
        END AS default_value,
```

- [ ] **Step 7: 修改 `crates/dblens-dialect/sql/tidb.sql`**

```diff
--- a/crates/dblens-dialect/sql/tidb.sql
+++ b/crates/dblens-dialect/sql/tidb.sql
@@ -63,6 +63,8 @@ SELECT c.TABLE_NAME AS table_name,
          WHEN c.COLUMN_DEFAULT IS NULL THEN NULL
          WHEN c.GENERATION_EXPRESSION <> '' THEN NULL
          WHEN c.EXTRA LIKE '%DEFAULT\_GENERATED%' THEN c.COLUMN_DEFAULT
+         -- TiDB 的 EXTRA 中没有 DEFAULT_GENERATED：时间列的 CURRENT_TIMESTAMP 默认值按表达式处理
+         WHEN c.DATA_TYPE IN ('datetime', 'timestamp') AND UPPER(c.COLUMN_DEFAULT) LIKE 'CURRENT\_TIMESTAMP%' THEN c.COLUMN_DEFAULT
          WHEN c.DATA_TYPE IN ('tinyint', 'smallint', 'mediumint', 'int', 'bigint', 'decimal', 'float', 'double', 'year', 'bit') THEN c.COLUMN_DEFAULT
          ELSE CONCAT('''', REPLACE(REPLACE(c.COLUMN_DEFAULT, '\\', '\\\\'), '''', ''''''), '''')
        END AS default_value,
```

- [ ] **Step 8: 修改 `crates/dblens-dialect/src/lib.rs`**

```diff
--- a/crates/dblens-dialect/src/lib.rs
+++ b/crates/dblens-dialect/src/lib.rs
@@ -10,6 +10,7 @@ pub mod ident;
 mod import;
 pub mod literal;
 pub mod meta;
+pub mod migrate;
 mod ops;
 mod session;
 mod split;
```

- [ ] **Step 9: 新建 `crates/dblens-dialect/src/migrate/expr.rs`**

```rust
//! 迁移中的表达式：跨家族时只转换"简单表达式"（列名、字面量、比较、逻辑运算、IN、BETWEEN、IS NULL、LIKE），
//! 以及常见的默认值写法（字面量、布尔、当前时间或日期、UUID 函数、序列的 nextval）。
//!
//! 先把源文本切成记号并规整：去掉 PG 的类型转换（`::text`）、MySQL 的字符集前缀（`_utf8mb4'x'`），
//! 把 PG 的 `= ANY (ARRAY[…])` / `<> ALL (ARRAY[…])` 还原成 `IN (…)` / `NOT IN (…)`。

use std::collections::BTreeMap;

use dblens_core::{ColumnType, LogicalType, Value};

use crate::{Dialect, Family};

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    /// 不加引号的词（列名、关键字、函数名），保留原样。
    Word(String),
    /// 加引号的标识符（引号已去掉）。
    Quoted(String),
    /// 字符串字面量（已还原转义）。
    Str(String),
    Num(String),
    /// 运算符和标点：`( ) , . = <> != < > <= >= + - * / % [ ] ::`
    Op(&'static str),
}

const OPS: &[&str] = &[
    "::", "<>", "!=", "<=", ">=", "||", "(", ")", ",", ".", "=", "<", ">", "+", "-", "*", "/", "%", "[", "]",
];

fn tokenize(family: Family, s: &str) -> Option<Vec<Tok>> {
    let b: Vec<char> = s.chars().collect();
    let mut i = 0;
    let mut out = Vec::new();
    while i < b.len() {
        let c = b[i];
        if c.is_whitespace() {
            i += 1;
        } else if c == '\'' {
            let mut v = String::new();
            i += 1;
            loop {
                match b.get(i) {
                    None => return None,
                    Some('\'') if b.get(i + 1) == Some(&'\'') => {
                        v.push('\'');
                        i += 2;
                    }
                    Some('\'') => {
                        i += 1;
                        break;
                    }
                    Some('\\') if family == Family::MySql => {
                        v.push(*b.get(i + 1)?);
                        i += 2;
                    }
                    Some(ch) => {
                        v.push(*ch);
                        i += 1;
                    }
                }
            }
            out.push(Tok::Str(v));
        } else if c == '"' || (c == '`' && family == Family::MySql) {
            let end = b[i + 1..].iter().position(|&x| x == c)? + i + 1;
            out.push(Tok::Quoted(b[i + 1..end].iter().collect()));
            i = end + 1;
        } else if c.is_ascii_digit() {
            let start = i;
            while i < b.len() && (b[i].is_ascii_digit() || b[i] == '.') {
                i += 1;
            }
            if i < b.len() && (b[i] == 'e' || b[i] == 'E') {
                i += 1;
                if i < b.len() && (b[i] == '+' || b[i] == '-') {
                    i += 1;
                }
                while i < b.len() && b[i].is_ascii_digit() {
                    i += 1;
                }
            }
            out.push(Tok::Num(b[start..i].iter().collect()));
        } else if c.is_alphabetic() || c == '_' {
            let start = i;
            while i < b.len() && (b[i].is_alphanumeric() || b[i] == '_' || b[i] == '$' || b[i] == '#') {
                i += 1;
            }
            out.push(Tok::Word(b[start..i].iter().collect()));
        } else {
            let rest: String = b[i..b.len().min(i + 2)].iter().collect();
            let op = OPS.iter().find(|o| rest.starts_with(**o))?;
            out.push(Tok::Op(op));
            i += op.chars().count();
        }
    }
    Some(normalize(out))
}

fn is_kw(t: &Tok, kw: &str) -> bool {
    matches!(t, Tok::Word(w) if w.eq_ignore_ascii_case(kw))
}

/// 规整：去掉类型转换和字符集前缀，还原 `ANY (ARRAY[…])`。
fn normalize(toks: Vec<Tok>) -> Vec<Tok> {
    const STOP: &[&str] = &["AND", "OR", "NOT", "IN", "IS", "BETWEEN", "LIKE"];
    let mut out: Vec<Tok> = Vec::new();
    let mut i = 0;
    while i < toks.len() {
        match &toks[i] {
            Tok::Op("::") => {
                i += 1;
                while matches!(toks.get(i), Some(Tok::Word(w)) if !STOP.iter().any(|s| w.eq_ignore_ascii_case(s)))
                {
                    i += 1;
                }
                if toks.get(i) == Some(&Tok::Op("(")) {
                    while i < toks.len() && toks[i] != Tok::Op(")") {
                        i += 1;
                    }
                    i += 1;
                }
                while toks.get(i) == Some(&Tok::Op("[")) && toks.get(i + 1) == Some(&Tok::Op("]")) {
                    i += 2;
                }
            }
            Tok::Word(w) if w.starts_with('_') && matches!(toks.get(i + 1), Some(Tok::Str(_))) => i += 1,
            _ => {
                out.push(toks[i].clone());
                i += 1;
            }
        }
    }
    any_array(out)
}

/// `= ANY (ARRAY[a, b])` → `IN (a, b)`；`<> ALL (ARRAY[a, b])` → `NOT IN (a, b)`（括号可以多套几层）。
fn any_array(toks: Vec<Tok>) -> Vec<Tok> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < toks.len() {
        let neg = match (&toks[i], toks.get(i + 1)) {
            (Tok::Op("="), Some(t)) if is_kw(t, "ANY") => Some(false),
            (Tok::Op("<>" | "!="), Some(t)) if is_kw(t, "ALL") => Some(true),
            _ => None,
        };
        if let Some((items, next)) = neg.and_then(|_| array_items(&toks, i + 2)) {
            if neg == Some(true) {
                out.push(Tok::Word("NOT".into()));
            }
            out.push(Tok::Word("IN".into()));
            out.push(Tok::Op("("));
            out.extend(items.iter().cloned());
            out.push(Tok::Op(")"));
            i = next;
            continue;
        }
        out.push(toks[i].clone());
        i += 1;
    }
    out
}

/// 从 `start` 起的 `(…(ARRAY[元素])…)`：返回元素的记号和之后的位置。
fn array_items(toks: &[Tok], start: usize) -> Option<(&[Tok], usize)> {
    let mut j = start;
    while toks.get(j) == Some(&Tok::Op("(")) {
        j += 1;
    }
    let depth = j - start;
    if depth == 0 || !toks.get(j).is_some_and(|t| is_kw(t, "ARRAY")) || toks.get(j + 1) != Some(&Tok::Op("["))
    {
        return None;
    }
    let end = j + 2 + toks[j + 2..].iter().position(|t| *t == Tok::Op("]"))?;
    let after = end + 1;
    (0..depth).all(|k| toks.get(after + k) == Some(&Tok::Op(")"))).then(|| (&toks[j + 2..end], after + depth))
}

/// 简单表达式中允许的关键字。
const KEYWORDS: &[&str] = &["AND", "OR", "NOT", "IN", "IS", "NULL", "BETWEEN", "LIKE", "TRUE", "FALSE"];

/// 转换简单表达式：列名换成目标名并按目标方言引用，字符串按目标方言写字面量。不是简单表达式时返回 None。
/// `columns`：源列名 → 目标列名。
pub fn convert_expr(
    src: &Dialect,
    tgt: &Dialect,
    text: &str,
    columns: &BTreeMap<String, String>,
) -> Option<String> {
    let toks = tokenize(src.family(), text)?;
    let find = |name: &str, quoted: bool| -> Option<&String> {
        columns.get(name).or_else(|| {
            if quoted {
                None
            } else {
                columns.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v)
            }
        })
    };
    let mut parts: Vec<String> = Vec::new();
    for t in &toks {
        let s = match t {
            Tok::Word(w) if KEYWORDS.iter().any(|k| w.eq_ignore_ascii_case(k)) => {
                let up = w.to_ascii_uppercase();
                match up.as_str() {
                    // Oracle、达梦没有布尔字面量
                    "TRUE" | "FALSE" if matches!(tgt.family(), Family::Oracle | Family::Dameng) => {
                        (if up == "TRUE" { "1" } else { "0" }).to_string()
                    }
                    _ => up,
                }
            }
            Tok::Word(w) => tgt.ident(find(w, false)?),
            Tok::Quoted(q) => tgt.ident(find(q, true)?),
            Tok::Str(v) => tgt.literal(&Value::String(v.clone())).ok()?,
            Tok::Num(n) => n.clone(),
            Tok::Op(
                o @ ("(" | ")" | "," | "=" | "<>" | "!=" | "<" | ">" | "<=" | ">=" | "+" | "-" | "*" | "/"),
            ) => (if *o == "!=" { "<>" } else { o }).to_string(),
            Tok::Op(_) => return None,
        };
        parts.push(s);
    }
    if parts.is_empty() {
        return None;
    }
    // 拼接：左括号之后、右括号和逗号之前不加空格
    let mut out = String::new();
    for (i, p) in parts.iter().enumerate() {
        let prev = i.checked_sub(1).map(|j| parts[j].as_str());
        if i > 0 && prev != Some("(") && p != ")" && p != "," {
            out.push(' ');
        }
        out.push_str(p);
    }
    Some(out)
}

/// 去掉成对包住全部内容的括号。
fn strip_parens(mut toks: &[Tok]) -> &[Tok] {
    while toks.len() >= 2 && toks[0] == Tok::Op("(") && toks[toks.len() - 1] == Tok::Op(")") {
        let mut depth = 0;
        let whole = toks.iter().enumerate().all(|(i, t)| {
            match t {
                Tok::Op("(") => depth += 1,
                Tok::Op(")") => depth -= 1,
                _ => {}
            }
            depth > 0 || i == toks.len() - 1
        });
        if !whole {
            break;
        }
        toks = &toks[1..toks.len() - 1];
    }
    toks
}

/// 函数调用 `名字()` 或 `名字(数字)`：返回名字（大写）。
fn call(toks: &[Tok]) -> Option<String> {
    match toks {
        [Tok::Word(w)] => Some(w.to_ascii_uppercase()),
        [Tok::Word(w), Tok::Op("("), Tok::Op(")")]
        | [Tok::Word(w), Tok::Op("("), Tok::Num(_), Tok::Op(")")] => Some(w.to_ascii_uppercase()),
        _ => None,
    }
}

/// 序列 nextval 的目标序列名：PG 的 `nextval('s')`（可带 schema）、Oracle/达梦的 `s.NEXTVAL`、MariaDB 的 `nextval(s)`。
pub(super) fn nextval_target(text: &str) -> Option<String> {
    let family = if text.contains('`') { Family::MySql } else { Family::Postgres };
    let toks = tokenize(family, text)?;
    let toks = strip_parens(&toks);
    let last = |name: &str| name.rsplit('.').next().unwrap_or(name).trim_matches('"').to_string();
    match toks {
        [Tok::Word(f), Tok::Op("("), Tok::Str(s), Tok::Op(")")] if f.eq_ignore_ascii_case("nextval") => {
            Some(last(s))
        }
        [Tok::Word(f), Tok::Op("("), Tok::Word(s) | Tok::Quoted(s), Tok::Op(")")]
            if f.eq_ignore_ascii_case("nextval") =>
        {
            Some(s.clone())
        }
        [.., Tok::Word(s) | Tok::Quoted(s), Tok::Op("."), Tok::Word(n) | Tok::Quoted(n)]
            if n.eq_ignore_ascii_case("NEXTVAL") =>
        {
            Some(s.clone())
        }
        _ => None,
    }
}

/// 转换列的默认值。`Ok(None)`：没有默认值（源为 NULL）；`Err(())`：认不出，不迁。
/// `column`：目标列的类型；`sequences`：源序列名（小写）→ 目标序列名。
pub(super) fn convert_default(
    src: &Dialect,
    tgt: &Dialect,
    text: &str,
    column: &ColumnType,
    sequences: &BTreeMap<String, String>,
) -> Result<Option<String>, ()> {
    use LogicalType as L;
    let fam = tgt.family();
    if let Some(seq) = nextval_target(text) {
        let name = sequences.get(&seq.to_lowercase()).ok_or(())?;
        return match fam {
            Family::Postgres => tgt
                .literal(&Value::String(tgt.ident(name)))
                .map(|s| Some(format!("nextval({s})")))
                .map_err(|_| ()),
            Family::Oracle | Family::Dameng => Ok(Some(format!("{}.NEXTVAL", tgt.ident(name)))),
            Family::MySql => Err(()),
        };
    }
    let toks = tokenize(src.family(), text).ok_or(())?;
    let toks = strip_parens(&toks);
    let logical = &column.logical;
    let oracle_like = matches!(fam, Family::Oracle | Family::Dameng);
    let boolean = |v: bool| -> String {
        if oracle_like || (fam == Family::Postgres && *logical != L::Bool) {
            (if v { "1" } else { "0" }).into()
        } else {
            (if v { "TRUE" } else { "FALSE" }).into()
        }
    };
    let today = || -> Result<Option<String>, ()> {
        Ok(Some(match (fam, logical) {
            (Family::MySql, L::Date) => "(CURRENT_DATE)".into(),
            (Family::Postgres, L::Date) => "CURRENT_DATE".into(),
            (Family::Oracle, L::Timestamp { .. } | L::Date) => "TRUNC(SYSDATE)".into(),
            (Family::Dameng, L::Date) => "CURDATE()".into(),
            _ => return Err(()),
        }))
    };
    let now = || -> Result<Option<String>, ()> {
        Ok(Some(match (fam, logical) {
            (Family::MySql, L::Timestamp { precision: Some(p), .. }) if *p > 0 => {
                format!("CURRENT_TIMESTAMP({p})")
            }
            (Family::MySql, L::Timestamp { .. }) => "CURRENT_TIMESTAMP".into(),
            (Family::Postgres, L::Timestamp { tz: false, .. }) => "LOCALTIMESTAMP".into(),
            (Family::Postgres, L::Timestamp { tz: true, .. }) => "CURRENT_TIMESTAMP".into(),
            (Family::Oracle | Family::Dameng, L::Timestamp { tz: false, precision: Some(0) })
                if column.native.trim().eq_ignore_ascii_case("DATE") =>
            {
                "SYSDATE".into()
            }
            (Family::Oracle, L::Timestamp { tz: false, .. }) => "LOCALTIMESTAMP".into(),
            (Family::Oracle, L::Timestamp { tz: true, .. }) => "SYSTIMESTAMP".into(),
            (Family::Dameng, L::Timestamp { .. }) => "CURRENT_TIMESTAMP".into(),
            (_, L::Date) => return today(),
            _ => return Err(()),
        }))
    };
    match toks {
        [] => Err(()),
        [Tok::Word(w)] if w.eq_ignore_ascii_case("NULL") => Ok(None),
        [Tok::Num(n)] if *logical == L::Bool && (n == "0" || n == "1") => Ok(Some(boolean(n == "1"))),
        [Tok::Num(n)] => Ok(Some(n.clone())),
        [Tok::Op("-"), Tok::Num(n)] => Ok(Some(format!("-{n}"))),
        [Tok::Word(w)] if w.eq_ignore_ascii_case("TRUE") || w.eq_ignore_ascii_case("FALSE") => {
            Ok(Some(boolean(w.eq_ignore_ascii_case("TRUE"))))
        }
        // MySQL 的位串 b'1'
        [Tok::Word(b), Tok::Str(s)] if b.eq_ignore_ascii_case("b") && (s == "0" || s == "1") => {
            Ok(Some(boolean(s == "1")))
        }
        [Tok::Str(s)] => {
            let value = Value::String(s.clone());
            match (fam, logical) {
                (Family::Oracle | Family::Dameng, L::Date | L::Timestamp { .. }) => {
                    // 只有日期的写成 DATE 字面量，带时间的写成 TIMESTAMP 字面量
                    let kw = if s.trim().len() <= 10 { "DATE" } else { "TIMESTAMP" };
                    tgt.literal(&value).map(|l| Some(format!("{kw} {l}"))).map_err(|_| ())
                }
                (_, L::Bool) if ["t", "true", "1"].contains(&s.to_lowercase().as_str()) => {
                    Ok(Some(boolean(true)))
                }
                (_, L::Bool) if ["f", "false", "0"].contains(&s.to_lowercase().as_str()) => {
                    Ok(Some(boolean(false)))
                }
                _ => tgt.literal(&value).map(Some).map_err(|_| ()),
            }
        }
        _ => match call(toks).as_deref() {
            Some(
                "CURRENT_TIMESTAMP"
                | "LOCALTIMESTAMP"
                | "NOW"
                | "SYSDATE"
                | "SYSTIMESTAMP"
                | "GETDATE"
                | "STATEMENT_TIMESTAMP"
                | "TRANSACTION_TIMESTAMP"
                | "CLOCK_TIMESTAMP"
                | "LOCALTIME"
                | "CURRENT_TIME"
                | "PG_SYSTIMESTAMP",
            ) => {
                if *logical == L::Date {
                    today()
                } else {
                    now()
                }
            }
            Some("CURRENT_DATE" | "CURDATE") => today(),
            Some("UUID" | "GEN_RANDOM_UUID" | "SYS_GUID" | "NEWID" | "UUID_GENERATE_V4") => {
                match tgt.variant() {
                    crate::Variant::MySql => Ok(Some("(UUID())".into())),
                    crate::Variant::Postgres => Ok(Some("gen_random_uuid()".into())),
                    crate::Variant::Oracle => Ok(Some("SYS_GUID()".into())),
                    _ => Err(()),
                }
            }
            _ => {
                // Oracle 的 TRUNC(SYSDATE)
                match toks {
                    [Tok::Word(f), Tok::Op("("), Tok::Word(a), Tok::Op(")")]
                        if f.eq_ignore_ascii_case("TRUNC") && a.eq_ignore_ascii_case("SYSDATE") =>
                    {
                        today()
                    }
                    _ => Err(()),
                }
            }
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Variant;

    fn d(v: Variant) -> Dialect {
        Dialect::new(v)
    }

    fn cols(names: &[(&str, &str)]) -> BTreeMap<String, String> {
        names.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect()
    }

    fn ty(dialect: Variant, native: &str) -> ColumnType {
        ColumnType { native: native.into(), logical: d(dialect).logical_type(native) }
    }

    #[test]
    fn simple_expressions_convert_names_and_literals() {
        let c = cols(&[("qty", "QTY"), ("status", "STATUS")]);
        let pg = d(Variant::Postgres);
        let ora = d(Variant::Oracle);
        assert_eq!(convert_expr(&pg, &ora, "(qty > 0)", &c).as_deref(), Some("(QTY > 0)"));
        assert_eq!(
            convert_expr(
                &pg,
                &ora,
                "((status)::text = ANY ((ARRAY['a'::character varying, 'b''c'::character varying])::text[]))",
                &c
            )
            .as_deref(),
            Some("((STATUS) IN ('a', 'b''c'))")
        );
        assert_eq!(
            convert_expr(&pg, &ora, "status <> ALL (ARRAY['x'])", &c).as_deref(),
            Some("STATUS NOT IN ('x')")
        );
        let my = d(Variant::MySql);
        let c2 = cols(&[("qty", "qty")]);
        assert_eq!(
            convert_expr(&my, &pg, "((`qty` between 1 and 10) and (`qty` <> _utf8mb4'x'))", &c2).as_deref(),
            Some("((qty BETWEEN 1 AND 10) AND (qty <> 'x'))")
        );
        assert_eq!(
            convert_expr(&ora, &pg, "\"QTY\" IS NOT NULL", &cols(&[("QTY", "qty")])).as_deref(),
            Some("qty IS NOT NULL")
        );
        assert_eq!(
            convert_expr(&pg, &ora, "flag = true", &cols(&[("flag", "FLAG")])).as_deref(),
            Some("FLAG = 1")
        );
    }

    #[test]
    fn functions_and_unknown_names_are_not_simple() {
        let c = cols(&[("name", "name")]);
        let (pg, my) = (d(Variant::Postgres), d(Variant::MySql));
        assert_eq!(convert_expr(&pg, &my, "length(name) > 0", &c), None);
        assert_eq!(convert_expr(&pg, &my, "other > 0", &c), None);
        assert_eq!(convert_expr(&pg, &my, "name || 'x' = 'y'", &c), None);
        assert_eq!(convert_expr(&pg, &my, "", &c), None);
    }

    #[test]
    fn defaults_convert_between_families() {
        let none = BTreeMap::new();
        let conv = |s: Variant, t: Variant, text: &str, native: &str| {
            convert_default(&d(s), &d(t), text, &ty(t, native), &none)
        };
        use Variant::*;
        assert_eq!(
            conv(Postgres, MySql, "'abc'::character varying", "VARCHAR(10)"),
            Ok(Some("'abc'".into()))
        );
        assert_eq!(conv(Postgres, Oracle, "0", "NUMBER(10)"), Ok(Some("0".into())));
        assert_eq!(conv(Oracle, Postgres, "-1.5 ", "NUMERIC(5,1)"), Ok(Some("-1.5".into())));
        assert_eq!(conv(Oracle, Postgres, "NULL", "INTEGER"), Ok(None));
        assert_eq!(conv(Postgres, Oracle, "true", "NUMBER(1)"), Ok(Some("1".into())));
        assert_eq!(conv(MySql, Postgres, "b'1'", "BOOLEAN"), Ok(Some("TRUE".into())));
        assert_eq!(conv(Postgres, MySql, "now()", "DATETIME(6)"), Ok(Some("CURRENT_TIMESTAMP(6)".into())));
        assert_eq!(conv(Oracle, Postgres, "SYSDATE", "TIMESTAMP(0)"), Ok(Some("LOCALTIMESTAMP".into())));
        assert_eq!(conv(MySql, Oracle, "CURRENT_TIMESTAMP", "DATE"), Ok(Some("SYSDATE".into())));
        assert_eq!(
            conv(Postgres, Oracle, "CURRENT_TIMESTAMP", "TIMESTAMP(6) WITH TIME ZONE"),
            Ok(Some("SYSTIMESTAMP".into()))
        );
        assert_eq!(conv(Postgres, Dameng, "CURRENT_DATE", "DATE"), Ok(Some("CURDATE()".into())));
        assert_eq!(conv(Oracle, MySql, "trunc(sysdate)", "DATE"), Ok(Some("(CURRENT_DATE)".into())));
        assert_eq!(
            conv(Postgres, Oracle, "'2020-01-02'::date", "DATE"),
            Ok(Some("DATE '2020-01-02'".into()))
        );
        assert_eq!(conv(Postgres, MySql, "gen_random_uuid()", "CHAR(36)"), Ok(Some("(UUID())".into())));
        assert_eq!(conv(Postgres, MySql, "upper('x')", "VARCHAR(10)"), Err(()));
        assert_eq!(conv(Postgres, MySql, "nextval('s'::regclass)", "BIGINT"), Err(()));
    }

    #[test]
    fn nextval_defaults_use_the_target_sequence() {
        let seqs = cols(&[("order_seq", "ORDER_SEQ")]);
        let pg = d(Variant::Postgres);
        let ora = d(Variant::Oracle);
        assert_eq!(nextval_target("nextval('public.order_seq'::regclass)").as_deref(), Some("order_seq"));
        assert_eq!(nextval_target("\"APP\".\"ORDER_SEQ\".\"NEXTVAL\"").as_deref(), Some("ORDER_SEQ"));
        assert_eq!(nextval_target("\"APP\".\"ORDER_SEQ\".NEXTVAL").as_deref(), Some("ORDER_SEQ"));
        assert_eq!(
            convert_default(
                &pg,
                &ora,
                "nextval('order_seq'::regclass)",
                &ty(Variant::Oracle, "NUMBER(19)"),
                &seqs
            ),
            Ok(Some("ORDER_SEQ.NEXTVAL".into()))
        );
        let back = cols(&[("order_seq", "order_seq")]);
        assert_eq!(
            convert_default(&ora, &pg, "ORDER_SEQ.NEXTVAL", &ty(Variant::Postgres, "BIGINT"), &back),
            Ok(Some("nextval('order_seq')".into()))
        );
    }
}
```

- [ ] **Step 10: 新建 `crates/dblens-dialect/src/migrate/mod.rs`**

```rust
//! 结构迁移（M5a 设计文档第 3、4 节）：把源 schema 的表和序列按目标方言转换成目标结构，
//! 给出逐项提示，再生成两步的 DDL（先建序列和表，再建索引和约束）。不做任何 IO。

mod expr;
mod names;
mod script;
#[cfg(test)]
mod tests;
mod types;

use std::collections::{BTreeMap, BTreeSet};

use dblens_core::schema::{CheckConstraint, Column, ForeignKey, Index, Sequence, Table, UniqueConstraint};
use dblens_core::{ColumnType, Msg, msg};
use serde::{Deserialize, Serialize};

use crate::design::diff::{is_expression, text};
use crate::{Dialect, Family};

pub use expr::convert_expr;
pub use names::{NameCase, max_ident_len};
pub use script::{ExistingAction, MigrationStatement, StatementKind, migration_script, script_text};

/// 迁移选项。`name_case` 为 None 时按目标库的习惯（见 `NameCase::for_target`）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MigrationOptions {
    #[serde(default)]
    pub name_case: Option<NameCase>,
    #[serde(default = "yes")]
    pub comments: bool,
    #[serde(default = "yes")]
    pub checks: bool,
}

fn yes() -> bool {
    true
}

impl Default for MigrationOptions {
    fn default() -> Self {
        Self { name_case: None, comments: true, checks: true }
    }
}

/// 全局类型规则：源类型名（可带参数，如 `NUMBER(1)`）→ 目标类型文本。不带参数时匹配该类型的所有写法。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TypeRule {
    pub source: String,
    pub target: String,
}

/// 一列的覆盖。`default` 为空串表示不要默认值；`accept` 表示确认把不认识的类型按文本迁移。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ColumnOverride {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, rename = "type", skip_serializing_if = "Option::is_none")]
    pub data_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<String>,
    /// 生成列的表达式；空串表示改为普通列。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expression: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub skip: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub accept: bool,
}

/// 一张表的覆盖。`checks`：源检查约束名 → 目标表达式，空串表示不迁。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TableOverride {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub skip: bool,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub columns: BTreeMap<String, ColumnOverride>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub checks: BTreeMap<String, String>,
}

/// 一个序列的覆盖。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SequenceOverride {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub skip: bool,
}

/// 映射设置：迁移方案中与结构转换有关的部分。覆盖按源对象名。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MigrationSettings {
    #[serde(default)]
    pub options: MigrationOptions,
    #[serde(default)]
    pub type_rules: Vec<TypeRule>,
    #[serde(default)]
    pub tables: BTreeMap<String, TableOverride>,
    #[serde(default)]
    pub sequences: BTreeMap<String, SequenceOverride>,
}

/// 提示的级别：等价（只是说明）、有损（照常迁移）、不支持（处理之前不能开始）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NoteLevel {
    Info,
    Lossy,
    Unsupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ObjectKind {
    Table,
    Sequence,
}

/// 一条提示。`object` 是源对象名；`column` 是源列名；`item` 是索引或约束的源名字（没有名字时为列清单）。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MigrationNote {
    pub kind: ObjectKind,
    pub object: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub column: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub item: Option<String>,
    pub level: NoteLevel,
    pub message: Msg,
}

/// 一列的对应：源列名 → 目标列名（不迁时为 None）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ColumnMapping {
    pub source: String,
    pub target: Option<String>,
}

/// 一张表的转换结果。`target` 是完整的目标表（含索引和约束）；`exists`：目标 schema 中已有同名表。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TableMapping {
    pub source: String,
    pub target: Table,
    pub columns: Vec<ColumnMapping>,
    pub exists: bool,
}

/// 一个序列的转换结果。`exists`：目标 schema 中已有同名序列（执行时跳过）。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SequenceMapping {
    pub source: String,
    pub target: Sequence,
    pub exists: bool,
}

/// 转换的结果：迁移的表和序列（被跳过的不在其中）、全部提示。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MigrationDraft {
    pub tables: Vec<TableMapping>,
    pub sequences: Vec<SequenceMapping>,
    pub notes: Vec<MigrationNote>,
}

impl MigrationDraft {
    /// 有不支持的项：处理之前不能开始。
    pub fn blocked(&self) -> bool {
        self.notes.iter().any(|n| n.level == NoteLevel::Unsupported)
    }
}

/// 转换的输入：源的表和序列（已按迁移范围筛选）、目标 schema 中已有的表和序列名。
pub struct MigrationInput<'a> {
    pub source: Dialect,
    pub target: Dialect,
    pub tables: &'a [Table],
    pub sequences: &'a [Sequence],
    pub target_tables: &'a [Table],
    pub target_sequences: &'a [String],
}

/// 收集提示。
struct Notes {
    kind: ObjectKind,
    object: String,
    out: Vec<MigrationNote>,
}

impl Notes {
    fn push(&mut self, column: Option<&str>, item: Option<&str>, level: NoteLevel, message: Msg) {
        self.out.push(MigrationNote {
            kind: self.kind,
            object: self.object.clone(),
            column: column.map(Into::into),
            item: item.map(Into::into),
            level,
            message,
        });
    }
}

/// 把源结构转换成目标结构。
pub fn migrate(input: &MigrationInput, settings: &MigrationSettings) -> MigrationDraft {
    let (src, tgt) = (&input.source, &input.target);
    let case = settings.options.name_case.unwrap_or_else(|| NameCase::for_target(tgt));
    let mut notes = Vec::new();

    // 序列：源序列名 → 目标序列名（默认值中的 nextval 要换成目标名）
    let owned = owned_sequences(src, input.tables);
    let mut sequences = Vec::new();
    let mut seq_names = BTreeMap::new();
    for s in input.sequences {
        let ov = settings.sequences.get(&s.name).cloned().unwrap_or_default();
        let mut n = Notes { kind: ObjectKind::Sequence, object: s.name.clone(), out: vec![] };
        if ov.skip || is_system_sequence(src, &s.name) {
            continue;
        }
        if owned.contains(&s.name.to_lowercase()) {
            n.push(None, None, NoteLevel::Info, msg!("migrate.serial_sequence"));
            notes.extend(n.out);
            continue;
        }
        if tgt.family() == Family::MySql {
            n.push(None, None, NoteLevel::Lossy, msg!("migrate.no_sequences"));
            notes.extend(n.out);
            continue;
        }
        let name = ov.name.clone().unwrap_or_else(|| case.apply(&s.name));
        seq_names.insert(s.name.to_lowercase(), name.clone());
        let target = types::sequence(tgt, s, &name, &mut n);
        let exists = input.target_sequences.iter().any(|x| x.eq_ignore_ascii_case(&name));
        if exists {
            n.push(None, None, NoteLevel::Info, msg!("migrate.sequence_exists"));
        }
        sequences.push(SequenceMapping { source: s.name.clone(), target, exists });
        notes.extend(n.out);
    }

    // 表名：源表名 → 目标表名（外键要引用目标名）
    let mut table_names = BTreeMap::new();
    for t in input.tables {
        let ov = settings.tables.get(&t.name);
        if ov.is_some_and(|o| o.skip) {
            continue;
        }
        let name = ov.and_then(|o| o.name.clone()).unwrap_or_else(|| case.apply(&t.name));
        table_names.insert(t.name.clone(), name);
    }

    let ctx = Context { src, tgt, case, settings, sequences: &seq_names, tables: &table_names, input };
    let mut tables = Vec::new();
    for t in input.tables.iter().filter(|t| table_names.contains_key(&t.name)) {
        let mut n = Notes { kind: ObjectKind::Table, object: t.name.clone(), out: vec![] };
        let (target, columns) = ctx.table(t, &mut n);
        let exists = input.target_tables.iter().any(|x| x.name.eq_ignore_ascii_case(&target.name));
        tables.push(TableMapping { source: t.name.clone(), target, columns, exists });
        notes.extend(n.out);
    }
    names::check(tgt, &tables, &sequences, &mut notes);
    MigrationDraft { tables, sequences, notes }
}

/// PG 的 serial 列自带的序列（默认值为 nextval 的自增列）：随自增列一起处理，不单独迁移。
fn owned_sequences(src: &Dialect, tables: &[Table]) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    if src.family() != Family::Postgres {
        return out;
    }
    for c in tables.iter().flat_map(|t| &t.columns).filter(|c| c.auto_increment) {
        if let Some(name) = c.default.as_deref().and_then(expr::nextval_target) {
            out.insert(name.to_lowercase());
        }
    }
    out
}

/// Oracle 标识列的系统序列（`ISEQ$$_…`）不迁移。
fn is_system_sequence(src: &Dialect, name: &str) -> bool {
    src.family() == Family::Oracle && name.starts_with("ISEQ$$_")
}

struct Context<'a> {
    src: &'a Dialect,
    tgt: &'a Dialect,
    case: NameCase,
    settings: &'a MigrationSettings,
    sequences: &'a BTreeMap<String, String>,
    tables: &'a BTreeMap<String, String>,
    input: &'a MigrationInput<'a>,
}

impl Context<'_> {
    fn same_family(&self) -> bool {
        self.src.family() == self.tgt.family()
    }

    fn same_product(&self) -> bool {
        self.src.variant() == self.tgt.variant() && self.src.compat() == self.tgt.compat()
    }

    /// MySQL 系为外键自动建的索引（与外键同名、列相同）：外键在目标中照样建，这个索引不单独迁
    /// （在 PG、Oracle、达梦中索引和约束同在一个名字空间，会重名）。
    fn fk_index(&self, t: &Table, x: &Index) -> bool {
        self.src.family() == Family::MySql
            && t.foreign_keys.iter().any(|f| {
                f.name.as_deref() == Some(x.name.as_str())
                    && f.columns.len() == x.columns.len()
                    && f.columns.iter().zip(&x.columns).all(|(a, b)| *a == b.name)
            })
    }

    /// 约束名：系统生成的名字（MySQL 的主键 `PRIMARY`、Oracle 的 `SYS_C…`、达梦的 `CONS…`）不带过去，由目标库生成。
    fn constraint(&self, name: &Option<String>) -> Option<String> {
        let n = text(name)?;
        let digits = |p: &str| {
            n.strip_prefix(p).is_some_and(|r| !r.is_empty() && r.chars().all(|c| c.is_ascii_digit()))
        };
        let system = match self.src.family() {
            Family::MySql => n.eq_ignore_ascii_case("PRIMARY"),
            Family::Oracle => digits("SYS_C"),
            Family::Dameng => digits("CONS"),
            Family::Postgres => false,
        };
        (!system).then(|| self.case.apply(n))
    }

    fn table(&self, t: &Table, n: &mut Notes) -> (Table, Vec<ColumnMapping>) {
        let ov = self.settings.tables.get(&t.name).cloned().unwrap_or_default();
        let name = self.tables[&t.name].clone();
        let mut mapping = Vec::new();
        let mut cols = BTreeMap::new();
        let mut columns = Vec::new();
        for c in &t.columns {
            let cov = ov.columns.get(&c.name).cloned().unwrap_or_default();
            if cov.skip {
                mapping.push(ColumnMapping { source: c.name.clone(), target: None });
                continue;
            }
            let cname = cov.name.clone().unwrap_or_else(|| self.case.apply(&c.name));
            cols.insert(c.name.clone(), cname.clone());
            mapping.push(ColumnMapping { source: c.name.clone(), target: Some(cname.clone()) });
            columns.push((c, cov, cname));
        }
        let comments = self.settings.options.comments;
        let mut out_cols = Vec::new();
        for (i, (c, cov, cname)) in columns.into_iter().enumerate() {
            out_cols.push(self.column(c, &cov, cname, i as u32 + 1, &cols, comments, n));
        }
        let mut target = Table {
            name,
            comment: if comments { t.comment.clone() } else { None },
            columns: out_cols,
            ..Default::default()
        };
        let lob = |col: &str| {
            target
                .columns
                .iter()
                .find(|c| c.name == col)
                .is_some_and(|c| types::is_lob(self.tgt, &c.data_type.logical))
        };
        let map_cols =
            |list: &[String]| -> Option<Vec<String>> { list.iter().map(|c| cols.get(c).cloned()).collect() };

        if let Some(pk) = &t.primary_key {
            match map_cols(&pk.columns) {
                Some(columns) if columns.iter().any(|c| lob(c)) => {
                    n.push(None, pk.name.as_deref(), NoteLevel::Unsupported, msg!("migrate.lob_key"));
                }
                Some(columns) => {
                    target.primary_key =
                        Some(dblens_core::schema::PrimaryKey { name: self.constraint(&pk.name), columns })
                }
                None => n.push(
                    None,
                    pk.name.as_deref(),
                    NoteLevel::Unsupported,
                    msg!("migrate.key_column_skipped"),
                ),
            }
        }
        for u in &t.uniques {
            let item = item_name(&u.name, &u.columns);
            match map_cols(&u.columns) {
                Some(columns) if columns.iter().any(|c| lob(c)) => {
                    n.push(None, Some(&item), NoteLevel::Lossy, msg!("migrate.lob_index"))
                }
                Some(columns) => {
                    target.uniques.push(UniqueConstraint { name: self.constraint(&u.name), columns })
                }
                None => n.push(None, Some(&item), NoteLevel::Lossy, msg!("migrate.item_column_skipped")),
            }
        }
        for x in t.indexes.iter().filter(|x| !self.fk_index(t, x)) {
            if let Some(i) = self.index(x, &cols, &lob, n) {
                target.indexes.push(i);
            }
        }
        for f in &t.foreign_keys {
            if let Some(f) = self.foreign_key(f, &cols, n) {
                target.foreign_keys.push(f);
            }
        }
        if self.settings.options.checks {
            for c in &t.checks {
                if let Some(c) = self.check(c, &ov, &cols, n) {
                    target.checks.push(c);
                }
            }
        }
        if t.partition.is_some() {
            n.push(None, None, NoteLevel::Lossy, msg!("migrate.partition"));
        }
        (target, mapping)
    }

    #[allow(clippy::too_many_arguments)]
    fn column(
        &self,
        c: &Column,
        ov: &ColumnOverride,
        name: String,
        ordinal: u32,
        cols: &BTreeMap<String, String>,
        comments: bool,
        n: &mut Notes,
    ) -> Column {
        let at = Some(c.name.as_str());
        let (native, logical) =
            types::column_type(self.src, self.tgt, c, ov, &self.settings.type_rules, |l, m| {
                n.push(at, None, l, m)
            });
        let mut out = Column {
            name,
            ordinal,
            data_type: ColumnType { native, logical },
            nullable: c.nullable,
            default: None,
            comment: if comments { c.comment.clone() } else { None },
            auto_increment: false,
            generated: None,
        };
        // 生成列
        if let Some(g) = text(&c.generated) {
            let converted = match ov.expression.as_deref() {
                Some(e) => Some(e.trim().to_string()).filter(|e| !e.is_empty()),
                None => self.expression(g, cols),
            };
            match converted {
                Some(e) => out.generated = Some(e),
                None => n.push(at, None, NoteLevel::Lossy, msg!("migrate.generated_dropped", expression = g)),
            }
            return out;
        }
        // 自增
        if c.auto_increment {
            if types::identity_supported(self.tgt) {
                out.auto_increment = true;
                if ov.data_type.is_none() {
                    let ColumnType { native, logical } = out.data_type.clone();
                    let (native, logical) = types::identity_type(self.tgt, native, logical);
                    out.data_type = ColumnType { native, logical };
                }
            } else {
                n.push(at, None, NoteLevel::Unsupported, msg!("migrate.identity_unsupported"));
            }
            return out;
        }
        // 默认值
        match ov.default.as_deref() {
            Some(d) => out.default = Some(d.trim().to_string()).filter(|d| !d.is_empty()),
            None => {
                if let Some(d) = text(&c.default) {
                    // 同一产品原样；同家族的其他产品先尝试转换，转换不了再原样
                    out.default = if self.same_product() {
                        Some(d.to_string())
                    } else {
                        match expr::convert_default(self.src, self.tgt, d, &out.data_type, self.sequences) {
                            Ok(v) => v,
                            Err(()) if self.same_family() => Some(d.to_string()),
                            Err(()) => {
                                n.push(
                                    at,
                                    None,
                                    NoteLevel::Lossy,
                                    msg!("migrate.default_dropped", default = d),
                                );
                                None
                            }
                        }
                    };
                }
            }
        }
        out
    }

    /// 表达式（检查约束、生成列、索引表达式）：同家族且列名不变时原样，否则只转换简单表达式。
    fn expression(&self, e: &str, cols: &BTreeMap<String, String>) -> Option<String> {
        if self.same_family() && cols.iter().all(|(a, b)| a == b) {
            return Some(e.to_string());
        }
        convert_expr(self.src, self.tgt, e, cols)
    }

    fn index(
        &self,
        x: &Index,
        cols: &BTreeMap<String, String>,
        lob: &dyn Fn(&str) -> bool,
        n: &mut Notes,
    ) -> Option<Index> {
        let mut out =
            Index { name: self.case.apply(&x.name), columns: Vec::new(), unique: x.unique, method: None };
        for c in &x.columns {
            let name = if is_expression(&c.name) {
                match self.expression(&c.name, cols) {
                    Some(e) => e,
                    None => {
                        n.push(
                            None,
                            Some(&x.name),
                            NoteLevel::Lossy,
                            msg!("migrate.index_expression", expression = c.name),
                        );
                        return None;
                    }
                }
            } else {
                match cols.get(&c.name) {
                    Some(t) if lob(t) => {
                        n.push(None, Some(&x.name), NoteLevel::Lossy, msg!("migrate.lob_index"));
                        return None;
                    }
                    Some(t) => t.clone(),
                    None => {
                        n.push(None, Some(&x.name), NoteLevel::Lossy, msg!("migrate.item_column_skipped"));
                        return None;
                    }
                }
            };
            out.columns.push(dblens_core::schema::IndexColumn { name, descending: c.descending });
        }
        if let Some(m) = text(&x.method) {
            if self.same_family() {
                out.method = Some(m.to_string());
            } else if !["btree", "normal"].iter().any(|d| m.eq_ignore_ascii_case(d)) {
                n.push(None, Some(&x.name), NoteLevel::Lossy, msg!("migrate.index_method", method = m));
            }
        }
        Some(out)
    }

    fn foreign_key(
        &self,
        f: &ForeignKey,
        cols: &BTreeMap<String, String>,
        n: &mut Notes,
    ) -> Option<ForeignKey> {
        let item = item_name(&f.name, &f.columns);
        let Some(columns) = f.columns.iter().map(|c| cols.get(c).cloned()).collect::<Option<Vec<_>>>() else {
            n.push(None, Some(&item), NoteLevel::Lossy, msg!("migrate.item_column_skipped"));
            return None;
        };
        let same_schema = f.ref_schema.is_none() || self.input.tables.iter().any(|x| x.name == f.ref_table);
        // 引用的表：在迁移范围内（用目标名和目标列名），或目标中已有同名表
        let (ref_table, ref_columns) = match self.tables.get(&f.ref_table).filter(|_| same_schema) {
            Some(name) => {
                let src_ref = self.input.tables.iter().find(|x| x.name == f.ref_table);
                let ov = self.settings.tables.get(&f.ref_table);
                let map = |c: &String| -> Option<String> {
                    let cov = ov.and_then(|o| o.columns.get(c));
                    if cov.is_some_and(|o| o.skip)
                        || src_ref.is_some_and(|r| !r.columns.iter().any(|x| &x.name == c))
                    {
                        return None;
                    }
                    Some(cov.and_then(|o| o.name.clone()).unwrap_or_else(|| self.case.apply(c)))
                };
                match f.ref_columns.iter().map(map).collect::<Option<Vec<_>>>() {
                    Some(rc) => (name.clone(), rc),
                    None => {
                        n.push(None, Some(&item), NoteLevel::Lossy, msg!("migrate.item_column_skipped"));
                        return None;
                    }
                }
            }
            None => {
                let name = self.case.apply(&f.ref_table);
                if !self.input.target_tables.iter().any(|x| x.name.eq_ignore_ascii_case(&name)) {
                    n.push(
                        None,
                        Some(&item),
                        NoteLevel::Lossy,
                        msg!("migrate.fk_outside", table = f.ref_table),
                    );
                    return None;
                }
                (name, f.ref_columns.iter().map(|c| self.case.apply(c)).collect())
            }
        };
        let mut out = ForeignKey {
            name: self.constraint(&f.name),
            columns,
            ref_schema: None,
            ref_table,
            ref_columns,
            on_delete: None,
            on_update: None,
        };
        for (what, action, slot) in
            [("DELETE", &f.on_delete, &mut out.on_delete), ("UPDATE", &f.on_update, &mut out.on_update)]
        {
            let Some(a) = text(action).map(str::to_ascii_uppercase) else { continue };
            if a == "NO ACTION" || a == "RESTRICT" {
                continue;
            }
            if types::fk_action_supported(self.tgt, what, &a) {
                *slot = Some(a);
            } else {
                n.push(
                    None,
                    Some(&item),
                    NoteLevel::Lossy,
                    msg!("migrate.fk_action", action = format!("ON {what} {a}")),
                );
            }
        }
        Some(out)
    }

    fn check(
        &self,
        c: &CheckConstraint,
        ov: &TableOverride,
        cols: &BTreeMap<String, String>,
        n: &mut Notes,
    ) -> Option<CheckConstraint> {
        let item = c.name.clone().unwrap_or_else(|| c.expression.clone());
        let expression = match c.name.as_ref().and_then(|x| ov.checks.get(x)) {
            Some(e) if e.trim().is_empty() => return None,
            Some(e) => e.trim().to_string(),
            None => match self.expression(&c.expression, cols) {
                Some(e) => e,
                None => {
                    n.push(
                        None,
                        Some(&item),
                        NoteLevel::Lossy,
                        msg!("migrate.check_dropped", expression = c.expression),
                    );
                    return None;
                }
            },
        };
        Some(CheckConstraint { name: self.constraint(&c.name), expression })
    }
}

/// 索引、约束在提示中的名字：有名字用名字，否则用列清单。
fn item_name(name: &Option<String>, columns: &[String]) -> String {
    name.clone().unwrap_or_else(|| columns.join(", "))
}
```

- [ ] **Step 11: 新建 `crates/dblens-dialect/src/migrate/names.rs`**

```rust
//! 迁移中的名字：大小写转换、长度上限、重名检查。

use std::collections::BTreeMap;

use dblens_core::msg;
use serde::{Deserialize, Serialize};

use super::{MigrationNote, NoteLevel, ObjectKind, SequenceMapping, TableMapping};
use crate::{Dialect, Family};

/// 名字的大小写转换。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NameCase {
    Keep,
    Lower,
    Upper,
}

impl NameCase {
    /// 目标库的习惯：Oracle、达梦折叠为大写，PG 系（含兼容 Oracle 的模式）折叠为小写，MySQL 系保持。
    pub fn for_target(target: &Dialect) -> NameCase {
        match target.family() {
            Family::Oracle | Family::Dameng => NameCase::Upper,
            Family::Postgres => NameCase::Lower,
            Family::MySql => NameCase::Keep,
        }
    }

    pub fn apply(self, name: &str) -> String {
        match self {
            NameCase::Keep => name.to_string(),
            NameCase::Lower => name.to_lowercase(),
            NameCase::Upper => name.to_uppercase(),
        }
    }
}

/// 标识符长度上限（MySQL 按字符，其余按字节）：MySQL 64，PG 系 63，Oracle 12.2 起 128、之前 30，达梦 128。
pub fn max_ident_len(target: &Dialect) -> usize {
    match target.family() {
        Family::MySql => 64,
        Family::Postgres => 63,
        Family::Oracle => {
            if target.version().is_some_and(|v| v < 120200) {
                30
            } else {
                128
            }
        }
        Family::Dameng => 128,
    }
}

fn too_long(target: &Dialect, name: &str) -> bool {
    let len = if target.family() == Family::MySql { name.chars().count() } else { name.len() };
    len > max_ident_len(target)
}

/// 检查重名和超长，结果写成不支持的提示。表和序列在同一 schema 中不能重名；PG、Oracle、达梦的索引名也在 schema 中唯一。
pub(super) fn check(
    target: &Dialect,
    tables: &[TableMapping],
    sequences: &[SequenceMapping],
    notes: &mut Vec<MigrationNote>,
) {
    let note = |kind, object: &str, column: Option<&str>, item: Option<&str>, message| MigrationNote {
        kind,
        object: object.to_string(),
        column: column.map(Into::into),
        item: item.map(Into::into),
        level: NoteLevel::Unsupported,
        message,
    };
    // schema 中的名字（不区分大小写）→ 第一次出现的源对象
    let mut seen: BTreeMap<String, String> = BTreeMap::new();
    let mut claim = |name: &str, owner: &str| -> Option<String> {
        let key = name.to_lowercase();
        match seen.get(&key) {
            Some(first) => Some(first.clone()),
            None => {
                seen.insert(key, owner.to_string());
                None
            }
        }
    };
    let schema_wide_indexes = target.family() != Family::MySql;
    for t in tables {
        let name = &t.target.name;
        if too_long(target, name) {
            notes.push(note(
                ObjectKind::Table,
                &t.source,
                None,
                None,
                msg!("migrate.name_too_long", name = name),
            ));
        }
        if let Some(first) = claim(name, &t.source) {
            notes.push(note(
                ObjectKind::Table,
                &t.source,
                None,
                None,
                msg!("migrate.name_conflict", name = name, other = first),
            ));
        }
        let mut cols: BTreeMap<String, &str> = BTreeMap::new();
        for (m, c) in t.columns.iter().filter_map(|m| m.target.as_ref().map(|c| (m, c))) {
            if too_long(target, c) {
                notes.push(note(
                    ObjectKind::Table,
                    &t.source,
                    Some(&m.source),
                    None,
                    msg!("migrate.name_too_long", name = c),
                ));
            }
            if let Some(first) = cols.insert(c.to_lowercase(), &m.source) {
                notes.push(note(
                    ObjectKind::Table,
                    &t.source,
                    Some(&m.source),
                    None,
                    msg!("migrate.name_conflict", name = c, other = first),
                ));
            }
        }
        let mut items: Vec<&str> = t.target.indexes.iter().map(|i| i.name.as_str()).collect();
        items.extend(t.target.primary_key.iter().filter_map(|k| k.name.as_deref()));
        items.extend(t.target.uniques.iter().filter_map(|k| k.name.as_deref()));
        items.extend(t.target.foreign_keys.iter().filter_map(|k| k.name.as_deref()));
        items.extend(t.target.checks.iter().filter_map(|k| k.name.as_deref()));
        for i in items {
            if too_long(target, i) {
                notes.push(note(
                    ObjectKind::Table,
                    &t.source,
                    None,
                    Some(i),
                    msg!("migrate.name_too_long", name = i),
                ));
            }
        }
        if schema_wide_indexes {
            for i in &t.target.indexes {
                if let Some(first) = claim(&i.name, &t.source) {
                    notes.push(note(
                        ObjectKind::Table,
                        &t.source,
                        None,
                        Some(&i.name),
                        msg!("migrate.name_conflict", name = i.name, other = first),
                    ));
                }
            }
        }
    }
    for s in sequences {
        let name = &s.target.name;
        if too_long(target, name) {
            notes.push(note(
                ObjectKind::Sequence,
                &s.source,
                None,
                None,
                msg!("migrate.name_too_long", name = name),
            ));
        }
        if let Some(first) = claim(name, &s.source) {
            notes.push(note(
                ObjectKind::Sequence,
                &s.source,
                None,
                None,
                msg!("migrate.name_conflict", name = name, other = first),
            ));
        }
    }
}
```

- [ ] **Step 12: 新建 `crates/dblens-dialect/src/migrate/script.rs`**

```rust
//! 迁移脚本（M5a 设计文档第 6 节）：第一步建序列、建表（列、主键、注释），删除重建的表先删；
//! 第二步建索引、唯一约束、检查约束，最后建外键。建表和约束的语句复用表设计器的 DDL 生成。

use std::collections::BTreeMap;

use dblens_core::Result;
use dblens_core::schema::{Index, Sequence, Table};
use serde::{Deserialize, Serialize};

use super::MigrationDraft;
use crate::design::{Diff, TableChange};
use crate::{Dialect, Family};

/// 目标中已有同名表时的处理方式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExistingAction {
    Skip,
    Recreate,
    /// 不建表，只建目标表中缺少的索引和约束。
    MissingOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StatementKind {
    DropTable,
    Sequence,
    CreateTable,
    Comment,
    Index,
    Unique,
    Check,
    ForeignKey,
}

/// 一条迁移语句。`object` 是源对象名；`requires`：这些源表建表失败时跳过本语句。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MigrationStatement {
    pub phase: u8,
    pub kind: StatementKind,
    pub object: String,
    pub requires: Vec<String>,
    pub sql: String,
}

/// 生成迁移脚本。`existing`：目标中已有同名表的处理方式（按源表名，缺省为跳过）；`target_tables`：目标中已有的表。
pub fn migration_script(
    tgt: &Dialect,
    schema: &str,
    draft: &MigrationDraft,
    existing: &BTreeMap<String, ExistingAction>,
    target_tables: &[Table],
) -> Result<Vec<MigrationStatement>> {
    let mut first = Vec::new();
    let mut second = Vec::new();
    let mut fks = Vec::new();
    let stmt = |phase, kind, object: &str, requires: Vec<String>, sql: String| MigrationStatement {
        phase,
        kind,
        object: object.to_string(),
        requires,
        sql,
    };
    for s in draft.sequences.iter().filter(|s| !s.exists) {
        first.push(stmt(1, StatementKind::Sequence, &s.source, vec![], sequence_sql(tgt, schema, &s.target)));
    }
    for t in &draft.tables {
        let action = if t.exists {
            existing.get(&t.source).copied().unwrap_or(ExistingAction::Skip)
        } else {
            ExistingAction::Recreate
        };
        if action == ExistingAction::Skip {
            continue;
        }
        let present = target_tables.iter().find(|x| x.name.eq_ignore_ascii_case(&t.target.name));
        let own = vec![t.source.clone()];
        if action == ExistingAction::Recreate {
            if t.exists {
                first.push(stmt(
                    1,
                    StatementKind::DropTable,
                    &t.source,
                    vec![],
                    drop_sql(tgt, schema, &t.target.name),
                ));
            }
            let base = Table {
                uniques: vec![],
                foreign_keys: vec![],
                checks: vec![],
                indexes: vec![],
                ..t.target.clone()
            };
            for (i, s) in
                design(tgt, schema, "", TableChange::CreateTable { table: base })?.into_iter().enumerate()
            {
                let kind = if i == 0 { StatementKind::CreateTable } else { StatementKind::Comment };
                first.push(stmt(1, kind, &t.source, if i == 0 { vec![] } else { own.clone() }, s));
            }
        }
        // 只建缺的：已有的表中有相同列（不区分大小写）的索引、约束不再建
        let missing_only = action == ExistingAction::MissingOnly;
        let cols_of = |i: &Index| i.columns.iter().map(|c| c.name.clone()).collect::<Vec<_>>();
        let (indexes, keys): (Vec<Vec<String>>, Vec<Vec<String>>) = match present.filter(|_| missing_only) {
            Some(p) => (
                p.indexes.iter().map(cols_of).collect(),
                p.uniques
                    .iter()
                    .map(|u| u.columns.clone())
                    .chain(p.indexes.iter().filter(|i| i.unique).map(cols_of))
                    .collect(),
            ),
            None => (vec![], vec![]),
        };
        let has = |list: &[Vec<String>], cols: &[String]| list.iter().any(|x| same_cols(x, cols));
        let name = &t.target.name;
        for x in &t.target.indexes {
            if has(&indexes, &cols_of(x)) {
                continue;
            }
            for s in design(tgt, schema, name, TableChange::AddIndex { index: x.clone() })? {
                second.push(stmt(2, StatementKind::Index, &t.source, own.clone(), s));
            }
        }
        for u in &t.target.uniques {
            if has(&keys, &u.columns) {
                continue;
            }
            for s in design(tgt, schema, name, TableChange::AddUnique { unique: u.clone() })? {
                second.push(stmt(2, StatementKind::Unique, &t.source, own.clone(), s));
            }
        }
        for c in &t.target.checks {
            if missing_only {
                let named = c.name.as_ref().is_some_and(|n| {
                    present.is_some_and(|p| {
                        p.checks.iter().any(|x| x.name.as_ref().is_some_and(|m| m.eq_ignore_ascii_case(n)))
                    })
                });
                // 没有名字的检查约束无法判断是否已有，不建
                if named || c.name.is_none() {
                    continue;
                }
            }
            for s in design(tgt, schema, name, TableChange::AddCheck { check: c.clone() })? {
                second.push(stmt(2, StatementKind::Check, &t.source, own.clone(), s));
            }
        }
        for f in &t.target.foreign_keys {
            let exists = missing_only
                && present.is_some_and(|p| {
                    p.foreign_keys.iter().any(|x| {
                        same_cols(&x.columns, &f.columns) && x.ref_table.eq_ignore_ascii_case(&f.ref_table)
                    })
                });
            if exists {
                continue;
            }
            let mut requires = own.clone();
            if let Some(r) =
                draft.tables.iter().find(|x| x.target.name == f.ref_table && x.source != t.source)
            {
                requires.push(r.source.clone());
            }
            for s in design(tgt, schema, name, TableChange::AddForeignKey { key: f.clone() })? {
                fks.push(stmt(2, StatementKind::ForeignKey, &t.source, requires.clone(), s));
            }
        }
    }
    first.extend(second);
    first.extend(fks);
    Ok(first)
}

/// 脚本全文：每条语句以 `;` 结尾，两步之间空一行。
pub fn script_text(statements: &[MigrationStatement]) -> String {
    let part = |phase| {
        statements
            .iter()
            .filter(|s| s.phase == phase)
            .map(|s| format!("{};", s.sql))
            .collect::<Vec<_>>()
            .join("\n")
    };
    [part(1), part(2)].into_iter().filter(|p| !p.is_empty()).collect::<Vec<_>>().join("\n\n")
}

fn same_cols(a: &[String], b: &[String]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.eq_ignore_ascii_case(y))
}

/// 用表设计器的生成得到一项变更的语句（去掉结尾的分号）。
fn design(tgt: &Dialect, schema: &str, table: &str, change: TableChange) -> Result<Vec<String>> {
    let diff = Diff { changes: vec![change], appended: true };
    Ok(tgt
        .design_sql(schema, table, &diff)?
        .into_iter()
        .map(|s| s.sql.trim().trim_end_matches(';').trim_end().to_string())
        .collect())
}

fn drop_sql(tgt: &Dialect, schema: &str, name: &str) -> String {
    let q = tgt.qualified(Some(schema), name);
    match tgt.family() {
        Family::MySql => format!("DROP TABLE {q}"),
        Family::Postgres | Family::Dameng => format!("DROP TABLE {q} CASCADE"),
        Family::Oracle => format!("DROP TABLE {q} CASCADE CONSTRAINTS"),
    }
}

/// 序列：PG 系用 `NO CYCLE`；Oracle、达梦用 `NOCYCLE`，缓存小于 2 时写 `NOCACHE`。
fn sequence_sql(tgt: &Dialect, schema: &str, s: &Sequence) -> String {
    if tgt.family() == Family::Postgres {
        return tgt.sequence_ddl(schema, s).trim().trim_end_matches(';').to_string();
    }
    let mut out = format!("CREATE SEQUENCE {}", tgt.qualified(Some(schema), &s.name));
    for (label, v) in [
        ("START WITH", &s.start),
        ("INCREMENT BY", &s.increment),
        ("MINVALUE", &s.min_value),
        ("MAXVALUE", &s.max_value),
    ] {
        if let Some(v) = v.as_deref().map(str::trim).filter(|v| !v.is_empty()) {
            out += &format!("\n    {label} {v}");
        }
    }
    out += &match s.cache {
        Some(c) if c >= 2 => format!("\n    CACHE {c}"),
        _ => "\n    NOCACHE".into(),
    };
    out += if s.cycle { "\n    CYCLE" } else { "\n    NOCYCLE" };
    out
}
```

- [ ] **Step 13: 新建 `crates/dblens-dialect/src/migrate/snapshots/dblens_dialect__migrate__tests__oracle_to_mysql.snap`**

```text
---
source: crates/dblens-dialect/src/migrate/tests.rs
expression: "render(&draft, Variant::MySql)"
---
Lossy DOC_SEQ: The target database has no sequences; not migrated
Lossy DOCS.NO: Default "APP"."DOC_SEQ"."NEXTVAL" cannot be converted; not migrated
Lossy DOCS.PRICE: NUMBER maps to DECIMAL(65,30); range or precision may change
Lossy DOCS [IX_DOCS_BODY]: Includes a large-object column the target cannot index; not migrated

CREATE TABLE app.DOCS (
    ID BIGINT NOT NULL AUTO_INCREMENT,
    NO BIGINT NULL,
    TITLE VARCHAR(200) NULL,
    BODY LONGTEXT NULL,
    CREATED DATETIME(0) NULL DEFAULT CURRENT_TIMESTAMP,
    FLAG SMALLINT NULL DEFAULT 0,
    PRICE DECIMAL(65,30) NULL,
    PRIMARY KEY (ID)
);

ALTER TABLE app.DOCS ADD INDEX IX_DOCS_TITLE (TITLE);
ALTER TABLE app.DOCS ADD CONSTRAINT CK_FLAG CHECK (FLAG IN (0, 1));
```

- [ ] **Step 14: 新建 `crates/dblens-dialect/src/migrate/snapshots/dblens_dialect__migrate__tests__oracle_to_pg.snap`**

```text
---
source: crates/dblens-dialect/src/migrate/tests.rs
expression: "render(&draft, Variant::Postgres)"
---
Info DOC_SEQ: MAXVALUE is outside the bigint range

CREATE SEQUENCE app.doc_seq
    START WITH 100
    INCREMENT BY 1
    MINVALUE 1
    CACHE 20
    NO CYCLE;
CREATE TABLE app.docs (
    id BIGINT GENERATED BY DEFAULT AS IDENTITY NOT NULL,
    no BIGINT DEFAULT nextval('doc_seq'),
    title VARCHAR(200),
    body TEXT,
    created TIMESTAMP(0) DEFAULT LOCALTIMESTAMP,
    flag BOOLEAN DEFAULT FALSE,
    price NUMERIC,
    CONSTRAINT pk_docs PRIMARY KEY (id)
);

CREATE INDEX ix_docs_body ON app.docs (body);
CREATE INDEX ix_docs_title ON app.docs (title);
ALTER TABLE app.docs ADD CONSTRAINT ck_flag CHECK (flag IN (0, 1));
```

- [ ] **Step 15: 新建 `crates/dblens-dialect/src/migrate/snapshots/dblens_dialect__migrate__tests__pg_to_dameng.snap`**

```text
---
source: crates/dblens-dialect/src/migrate/tests.rs
expression: "render(&draft, tgt)"
---
Info customers_id_seq: Sequence owned by a serial column; migrated with the identity column
Lossy customers.tags: Unrecognized type text[]; migrated as text (TEXT)
Lossy customers [ix_customers_note]: Includes a large-object column the target cannot index; not migrated
Lossy customers [ix_customers_lower]: Index expression lower((email)::text) cannot be converted; not migrated
Lossy orders.code: Default gen_random_uuid() cannot be converted; not migrated
Lossy orders.pos: Unrecognized type point; migrated as text (TEXT)
Lossy orders [fk_orders_customer]: The target does not support ON UPDATE CASCADE; dropped
Lossy orders [fk_orders_product]: Referenced table products is neither migrated nor in the target; not migrated

CREATE SEQUENCE "app".ORDER_SEQ
    START WITH 1
    INCREMENT BY 1
    MINVALUE 1
    MAXVALUE 9223372036854775807
    CACHE 20
    NOCYCLE;
CREATE TABLE "app".CUSTOMERS (
    ID BIGINT IDENTITY(1, 1) NOT NULL,
    NAME VARCHAR(50 CHAR) DEFAULT 'n/a' NOT NULL,
    EMAIL VARCHAR(100 CHAR),
    ACTIVE BIT DEFAULT 1,
    CREATED TIMESTAMP(6) DEFAULT CURRENT_TIMESTAMP,
    BALANCE DEC(12,2) DEFAULT 0,
    TAGS TEXT,
    NOTE TEXT,
    CONSTRAINT CUSTOMERS_PKEY PRIMARY KEY (ID)
);
COMMENT ON TABLE "app".CUSTOMERS IS 'Customers';
COMMENT ON COLUMN "app".CUSTOMERS.NAME IS 'Customer name';
CREATE TABLE "app".ORDERS (
    ID INT DEFAULT ORDER_SEQ.NEXTVAL NOT NULL,
    CUSTOMER_ID BIGINT,
    PRODUCT_ID INT,
    AMOUNT DEC(10,2),
    DOUBLE_AMOUNT DEC(12,2) GENERATED ALWAYS AS ((AMOUNT * (2))) VIRTUAL,
    STATUS VARCHAR(10 CHAR) DEFAULT 'new',
    PLACED DATE DEFAULT CURDATE(),
    CODE VARCHAR(36),
    POS TEXT,
    CONSTRAINT ORDERS_PKEY PRIMARY KEY (ID)
);

CREATE INDEX "app".IX_CUSTOMERS_NAME ON "app".CUSTOMERS (NAME);
ALTER TABLE "app".CUSTOMERS ADD CONSTRAINT CUSTOMERS_EMAIL_KEY UNIQUE (EMAIL);
ALTER TABLE "app".CUSTOMERS ADD CONSTRAINT CK_BALANCE CHECK (BALANCE >= (0));
ALTER TABLE "app".ORDERS ADD CONSTRAINT CK_STATUS CHECK ((STATUS) IN ('new', 'paid'));
ALTER TABLE "app".ORDERS ADD CONSTRAINT FK_ORDERS_CUSTOMER FOREIGN KEY (CUSTOMER_ID) REFERENCES "app".CUSTOMERS (ID) ON DELETE CASCADE;
```

- [ ] **Step 16: 新建 `crates/dblens-dialect/src/migrate/snapshots/dblens_dialect__migrate__tests__pg_to_mysql.snap`**

```text
---
source: crates/dblens-dialect/src/migrate/tests.rs
expression: "render(&draft, tgt)"
---
Info customers_id_seq: Sequence owned by a serial column; migrated with the identity column
Lossy order_seq: The target database has no sequences; not migrated
Lossy customers.tags: Unrecognized type text[]; migrated as text (LONGTEXT)
Lossy customers [ix_customers_note]: Includes a large-object column the target cannot index; not migrated
Lossy customers [ix_customers_lower]: Index expression lower((email)::text) cannot be converted; not migrated
Lossy orders.id: Default nextval('order_seq'::regclass) cannot be converted; not migrated
Lossy orders.pos: Unrecognized type point; migrated as text (LONGTEXT)
Lossy orders [fk_orders_product]: Referenced table products is neither migrated nor in the target; not migrated

CREATE TABLE app.customers (
    id BIGINT NOT NULL AUTO_INCREMENT,
    name VARCHAR(50) NOT NULL DEFAULT 'n/a' COMMENT 'Customer name',
    email VARCHAR(100) NULL,
    active TINYINT(1) NULL DEFAULT TRUE,
    created DATETIME(6) NULL DEFAULT CURRENT_TIMESTAMP(6),
    balance DECIMAL(12,2) NULL DEFAULT 0,
    tags LONGTEXT NULL,
    note LONGTEXT NULL,
    PRIMARY KEY (id)
) COMMENT = 'Customers';
CREATE TABLE app.orders (
    id INT NOT NULL,
    customer_id BIGINT NULL,
    product_id INT NULL,
    amount DECIMAL(10,2) NULL,
    double_amount DECIMAL(12,2) GENERATED ALWAYS AS ((amount * (2))) NULL,
    status VARCHAR(10) NULL DEFAULT 'new',
    placed DATE NULL DEFAULT (CURRENT_DATE),
    code CHAR(36) NULL DEFAULT (UUID()),
    pos LONGTEXT NULL,
    PRIMARY KEY (id)
);

ALTER TABLE app.customers ADD INDEX ix_customers_name (name);
ALTER TABLE app.customers ADD CONSTRAINT customers_email_key UNIQUE (email);
ALTER TABLE app.customers ADD CONSTRAINT ck_balance CHECK (balance >= (0));
ALTER TABLE app.orders ADD CONSTRAINT ck_status CHECK ((status) IN ('new', 'paid'));
ALTER TABLE app.orders ADD CONSTRAINT fk_orders_customer FOREIGN KEY (customer_id) REFERENCES app.customers (id) ON DELETE CASCADE ON UPDATE CASCADE;
```

- [ ] **Step 17: 新建 `crates/dblens-dialect/src/migrate/snapshots/dblens_dialect__migrate__tests__pg_to_opengauss.snap`**

```text
---
source: crates/dblens-dialect/src/migrate/tests.rs
expression: "render(&draft, tgt)"
---
Info customers_id_seq: Sequence owned by a serial column; migrated with the identity column
Lossy orders [fk_orders_product]: Referenced table products is neither migrated nor in the target; not migrated

CREATE SEQUENCE app.order_seq
    START WITH 1
    INCREMENT BY 1
    MINVALUE 1
    MAXVALUE 9223372036854775807
    CACHE 20
    NO CYCLE;
CREATE TABLE app.customers (
    id bigserial NOT NULL,
    name character varying(50) DEFAULT 'n/a' NOT NULL,
    email character varying(100),
    active boolean DEFAULT TRUE,
    created timestamp(6) without time zone DEFAULT LOCALTIMESTAMP,
    balance numeric(12,2) DEFAULT 0,
    tags text[],
    note text,
    CONSTRAINT customers_pkey PRIMARY KEY (id)
);
COMMENT ON TABLE app.customers IS 'Customers';
COMMENT ON COLUMN app.customers.name IS 'Customer name';
CREATE TABLE app.orders (
    id integer DEFAULT nextval('order_seq') NOT NULL,
    customer_id bigint,
    product_id integer,
    amount numeric(10,2),
    double_amount numeric(12,2) GENERATED ALWAYS AS ((amount * (2)::numeric)) STORED,
    status character varying(10) DEFAULT 'new',
    placed date DEFAULT CURRENT_DATE,
    code uuid DEFAULT gen_random_uuid(),
    pos point,
    CONSTRAINT orders_pkey PRIMARY KEY (id)
);

CREATE INDEX ix_customers_name ON app.customers (name);
CREATE INDEX ix_customers_note ON app.customers USING gin (note);
CREATE INDEX ix_customers_lower ON app.customers (lower((email)::text));
ALTER TABLE app.customers ADD CONSTRAINT customers_email_key UNIQUE (email);
ALTER TABLE app.customers ADD CONSTRAINT ck_balance CHECK (balance >= (0)::numeric);
ALTER TABLE app.orders ADD CONSTRAINT ck_status CHECK ((status)::text = ANY ((ARRAY['new'::character varying, 'paid'::character varying])::text[]));
ALTER TABLE app.orders ADD CONSTRAINT fk_orders_customer FOREIGN KEY (customer_id) REFERENCES app.customers (id) ON DELETE CASCADE ON UPDATE CASCADE;
```

- [ ] **Step 18: 新建 `crates/dblens-dialect/src/migrate/snapshots/dblens_dialect__migrate__tests__pg_to_oracle.snap`**

```text
---
source: crates/dblens-dialect/src/migrate/tests.rs
expression: "render(&draft, tgt)"
---
Info customers_id_seq: Sequence owned by a serial column; migrated with the identity column
Lossy customers.active: boolean maps to NUMBER(1); range or precision may change
Lossy customers.tags: Unrecognized type text[]; migrated as text (CLOB)
Lossy customers [ix_customers_note]: Includes a large-object column the target cannot index; not migrated
Lossy customers [ix_customers_lower]: Index expression lower((email)::text) cannot be converted; not migrated
Lossy orders.pos: Unrecognized type point; migrated as text (CLOB)
Lossy orders [fk_orders_customer]: The target does not support ON UPDATE CASCADE; dropped
Lossy orders [fk_orders_product]: Referenced table products is neither migrated nor in the target; not migrated

CREATE SEQUENCE "app".ORDER_SEQ
    START WITH 1
    INCREMENT BY 1
    MINVALUE 1
    MAXVALUE 9223372036854775807
    CACHE 20
    NOCYCLE;
CREATE TABLE "app".CUSTOMERS (
    ID NUMBER(19) GENERATED BY DEFAULT AS IDENTITY NOT NULL,
    NAME VARCHAR2(50 CHAR) DEFAULT 'n/a' NOT NULL,
    EMAIL VARCHAR2(100 CHAR),
    ACTIVE NUMBER(1) DEFAULT 1,
    CREATED TIMESTAMP(6) DEFAULT LOCALTIMESTAMP,
    BALANCE NUMBER(12,2) DEFAULT 0,
    TAGS CLOB,
    NOTE CLOB,
    CONSTRAINT CUSTOMERS_PKEY PRIMARY KEY (ID)
);
COMMENT ON TABLE "app".CUSTOMERS IS 'Customers';
COMMENT ON COLUMN "app".CUSTOMERS.NAME IS 'Customer name';
CREATE TABLE "app".ORDERS (
    ID NUMBER(10) DEFAULT ORDER_SEQ.NEXTVAL NOT NULL,
    CUSTOMER_ID NUMBER(19),
    PRODUCT_ID NUMBER(10),
    AMOUNT NUMBER(10,2),
    DOUBLE_AMOUNT NUMBER(12,2) GENERATED ALWAYS AS ((AMOUNT * (2))) VIRTUAL,
    STATUS VARCHAR2(10 CHAR) DEFAULT 'new',
    PLACED DATE DEFAULT TRUNC(SYSDATE),
    CODE VARCHAR2(36) DEFAULT SYS_GUID(),
    POS CLOB,
    CONSTRAINT ORDERS_PKEY PRIMARY KEY (ID)
);

CREATE INDEX "app".IX_CUSTOMERS_NAME ON "app".CUSTOMERS (NAME);
ALTER TABLE "app".CUSTOMERS ADD CONSTRAINT CUSTOMERS_EMAIL_KEY UNIQUE (EMAIL);
ALTER TABLE "app".CUSTOMERS ADD CONSTRAINT CK_BALANCE CHECK (BALANCE >= (0));
ALTER TABLE "app".ORDERS ADD CONSTRAINT CK_STATUS CHECK ((STATUS) IN ('new', 'paid'));
ALTER TABLE "app".ORDERS ADD CONSTRAINT FK_ORDERS_CUSTOMER FOREIGN KEY (CUSTOMER_ID) REFERENCES "app".CUSTOMERS (ID) ON DELETE CASCADE;
```

- [ ] **Step 19: 新建 `crates/dblens-dialect/src/migrate/types.rs`**

```rust
//! 迁移中的类型：列类型的映射（覆盖 → 规则 → 同家族原样 → 经逻辑类型）、有损判断、序列、目标库的能力。

use dblens_core::schema::{Column, Sequence};
use dblens_core::{LogicalType, Msg, msg};

use super::{ColumnOverride, NoteLevel, Notes, TypeRule};
use crate::types::parse_type;
use crate::{Compat, Dialect, Family};

/// 一列的目标类型：（原生类型文本，逻辑类型）。提示经 `note` 报告。
pub(super) fn column_type(
    src: &Dialect,
    tgt: &Dialect,
    c: &Column,
    ov: &ColumnOverride,
    rules: &[TypeRule],
    mut note: impl FnMut(NoteLevel, Msg),
) -> (String, LogicalType) {
    let source = c.data_type.native.trim();
    let logical = src.logical_type(source);
    let explicit = ov.data_type.as_deref().map(str::trim).filter(|t| !t.is_empty()).map(str::to_string);
    let explicit = explicit.or_else(|| {
        rules.iter().find(|r| rule_matches(&r.source, source)).map(|r| r.target.trim().to_string())
    });
    if let Some(native) = explicit {
        let back = tgt.logical_type(&native);
        if back == LogicalType::Other && logical != LogicalType::Other {
            note(NoteLevel::Lossy, msg!("migrate.unknown_target_type", native = native));
        }
        return (native, back);
    }
    if keeps_native(src, tgt) {
        return (source.to_string(), logical);
    }
    if logical == LogicalType::Other {
        let native = tgt.native_type(&LogicalType::Text);
        let level = if ov.accept { NoteLevel::Lossy } else { NoteLevel::Unsupported };
        note(level, msg!("migrate.type_unknown", source = source, target = native));
        return (native, LogicalType::Text);
    }
    let logical = integer_number(src, tgt, logical);
    let native = tgt.native_type(&logical);
    let back = tgt.logical_type(&native);
    if !equivalent(tgt, &logical, &back) {
        note(NoteLevel::Lossy, msg!("migrate.type_lossy", from = source, to = native));
    }
    (native, back)
}

/// Oracle 的 NUMBER(p)（小数位 0、p ≤ 19）迁到其他库时用整数类型：p ≤ 4 用 16 位，p ≤ 9 用 32 位，否则 64 位
/// （NUMBER(19) 超出 bigint 的取值很少见，给有损提示）。这样标识列能建，外键两边的类型也一致。
fn integer_number(src: &Dialect, tgt: &Dialect, logical: LogicalType) -> LogicalType {
    match logical {
        LogicalType::Decimal { precision: Some(p), scale: None | Some(0) }
            if src.family() == Family::Oracle && tgt.family() != Family::Oracle && p <= 19 =>
        {
            LogicalType::Int {
                bits: if p <= 4 {
                    16
                } else if p <= 9 {
                    32
                } else {
                    64
                },
            }
        }
        other => other,
    }
}

/// 自增列在目标中必须是整数：小数位为 0 的定点数改成 64 位整数。
pub(super) fn identity_type(tgt: &Dialect, native: String, logical: LogicalType) -> (String, LogicalType) {
    match logical {
        LogicalType::Decimal { scale: None | Some(0), .. } if tgt.family() != Family::Oracle => {
            let l = LogicalType::Int { bits: 64 };
            (tgt.native_type(&l), l)
        }
        other => (native, other),
    }
}

/// 同家族、且目标认得源的写法（兼容模式相同，或目标是兼容 Oracle 的模式）时原样保留类型文本。
fn keeps_native(src: &Dialect, tgt: &Dialect) -> bool {
    src.family() == tgt.family() && (src.compat() == tgt.compat() || tgt.compat() == Compat::Oracle)
}

/// 规则的源类型是否匹配：类型名相同；规则带参数时参数也要相同。
fn rule_matches(rule: &str, native: &str) -> bool {
    let (r, n) = (parse_type(rule), parse_type(native));
    let args = |a: &[String]| a.iter().map(|x| x.trim().to_ascii_uppercase()).collect::<Vec<_>>();
    r.name == n.name && (r.args.is_empty() || args(&r.args) == args(&n.args)) && r.unsigned == n.unsigned
}

/// 整数位数：能容纳 `bits` 位有符号整数的十进制位数。
fn int_digits(bits: u8) -> u32 {
    match bits {
        0..=8 => 3,
        9..=16 => 5,
        17..=24 => 7,
        25..=32 => 10,
        _ => 19,
    }
}

/// 映射后是否等价：没有改变取值范围、精度或语义。
fn equivalent(tgt: &Dialect, a: &LogicalType, b: &LogicalType) -> bool {
    use LogicalType as L;
    match (a, b) {
        _ if a == b => true,
        (L::Text, L::VarChar { len: None }) | (L::VarChar { len: None }, L::Text) => true,
        (L::Blob, L::VarBinary { len: None }) | (L::VarBinary { len: None }, L::Blob) => true,
        // UUID 存成 36 个字符的文本
        (L::Uuid, L::Char { len: Some(36) } | L::VarChar { len: Some(36) }) => true,
        // PG 只有 bytea
        (L::Binary { .. } | L::VarBinary { .. }, L::Blob) => tgt.family() == Family::Postgres,
        // Oracle 没有整数类型，NUMBER(n) 能容纳即可
        (L::Int { bits }, L::Decimal { precision: Some(p), scale: None | Some(0) }) => {
            *p >= int_digits(*bits)
        }
        (L::Decimal { precision: Some(p), scale: None | Some(0) }, L::Int { bits }) => *p < int_digits(*bits),
        // Oracle 的 DATE 带时分秒，能容纳日期
        (L::Date, L::Timestamp { tz: false, precision: Some(0) }) => tgt.family() == Family::Oracle,
        _ => false,
    }
}

/// 目标中不能建索引、主键的大字段类型：MySQL、Oracle、达梦的 TEXT/CLOB/BLOB/JSON。
pub(super) fn is_lob(tgt: &Dialect, l: &LogicalType) -> bool {
    use LogicalType as L;
    tgt.family() != Family::Postgres && matches!(l, L::Text | L::Blob | L::Json | L::VarChar { len: None })
}

/// 目标库能否建自增列：Oracle 12c 起才有标识列，其余都可以。
pub(super) fn identity_supported(tgt: &Dialect) -> bool {
    !(tgt.family() == Family::Oracle && tgt.version().is_some_and(|v| v < 120000))
}

/// 外键动作在目标库中是否可用：Oracle、达梦只有 ON DELETE CASCADE / SET NULL；MySQL（InnoDB）不支持 SET DEFAULT。
pub(super) fn fk_action_supported(tgt: &Dialect, what: &str, action: &str) -> bool {
    match tgt.family() {
        Family::Oracle | Family::Dameng => what == "DELETE" && (action == "CASCADE" || action == "SET NULL"),
        Family::MySql => action != "SET DEFAULT",
        Family::Postgres => true,
    }
}

/// 序列：PG 系的取值是 bigint，超出的上下限去掉。
pub(super) fn sequence(tgt: &Dialect, s: &Sequence, name: &str, n: &mut Notes) -> Sequence {
    let mut out = Sequence { name: name.to_string(), ..s.clone() };
    if tgt.family() == Family::Postgres {
        let fits = |v: &Option<String>| v.as_deref().is_none_or(|v| v.trim().parse::<i64>().is_ok());
        for (label, v) in [("MINVALUE", &mut out.min_value), ("MAXVALUE", &mut out.max_value)] {
            if !fits(v) {
                n.push(None, None, NoteLevel::Info, msg!("migrate.sequence_bound", bound = label));
                *v = None;
            }
        }
        if !fits(&out.start) {
            n.push(None, None, NoteLevel::Unsupported, msg!("migrate.sequence_bound", bound = "START WITH"));
        }
    }
    out
}
```

- [ ] **Step 20: 检查**

```bash
export CARGO_INCREMENTAL=0
cargo fmt --all
cargo clippy --config "source.mirror.registry='sparse+https://rsproxy.cn/index/'" --workspace --all-targets -- -D warnings
DBLENS_LANG=en cargo test --config "source.mirror.registry='sparse+https://rsproxy.cn/index/'" --workspace
```

Expected: 全部测试通过（dialect 新增 10 个，快照 6 个，元数据快照 3 个更新）。

- [ ] **Step 21: 提交**

```bash
git add crates/dblens-core/locales/app.json crates/dblens-dialect/sql/mysql.sql crates/dblens-dialect/sql/tidb.sql crates/dblens-dialect/src/lib.rs crates/dblens-dialect/src/migrate/expr.rs crates/dblens-dialect/src/migrate/mod.rs crates/dblens-dialect/src/migrate/names.rs crates/dblens-dialect/src/migrate/script.rs crates/dblens-dialect/src/migrate/snapshots/dblens_dialect__migrate__tests__oracle_to_mysql.snap crates/dblens-dialect/src/migrate/snapshots/dblens_dialect__migrate__tests__oracle_to_pg.snap crates/dblens-dialect/src/migrate/snapshots/dblens_dialect__migrate__tests__pg_to_dameng.snap crates/dblens-dialect/src/migrate/snapshots/dblens_dialect__migrate__tests__pg_to_mysql.snap crates/dblens-dialect/src/migrate/snapshots/dblens_dialect__migrate__tests__pg_to_opengauss.snap crates/dblens-dialect/src/migrate/snapshots/dblens_dialect__migrate__tests__pg_to_oracle.snap crates/dblens-dialect/src/migrate/tests.rs crates/dblens-dialect/src/migrate/types.rs crates/dblens-dialect/tests/snapshots/meta_snapshots__mysql.snap crates/dblens-dialect/tests/snapshots/meta_snapshots__oceanbase.snap crates/dblens-dialect/tests/snapshots/meta_snapshots__tidb.snap
git commit -m "feat(migrate): 结构迁移的转换与两步脚本" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 3: 服务层：预览、任务、报告与接口；实库测试

- `dblens-service/src/migrate.rs`（新）：`MigrationEnd`、`MigrationPlan { source, tables, sequences, target, settings, existing }`、`OtherObject`、`MigrationPreview`；读两端结构（目标 schema 必须存在；开始前跳过元数据缓存）、不迁移的对象（视图、物化视图、例程、同义词、迁移的表上的触发器）；`problems`（源和目标相同、没有要迁移的对象、有不支持的项、同名表没有选择）；`Service::preview_migration`、`Service::start_migration`（`TaskKind::Migrate`：逐条执行，失败记下后继续，建表失败的表跳过依赖它的语句；结束后刷新目标的元数据缓存，写迁移报告 `migration-<schema>.md`：结果、汇总、失败的语句、提示、不迁移的对象及其源库 DDL、脚本）。
- `task.rs`：`TaskKind::Migrate`、`TaskHandle::status()`；`design.rs` 的 `Run::sql` 改为 `pub(crate)`；`compare.rs` 的测试辅助 `service` 改为 `pub(crate)`；`lib.rs` 导出 `MigrationEnd`、`MigrationPlan`、`MigrationPreview`、`OtherObject`。
- `dblens-api/src/migrations.rs`（新）：`POST /migrations/preview`、`POST /migrations`（201 `{id}`）。
- `tests/migrate_live.rs`（新）：一个服务中建源和目标两个连接；源库建测试结构（自增主键、默认值、注释、唯一约束、检查约束、索引、外键），迁移，读回目标结构与草稿按要点比对；44 条路线，表名带源库代号。
- 后端词条加 `migrate.same_target`、`nothing`、`unsupported_left`、`existing_unchosen` 和报告的 `migrate.report.*`。

**Files:**
- Modify: `crates/dblens-api/src/lib.rs`
- Create: `crates/dblens-api/src/migrations.rs`
- Modify: `crates/dblens-api/tests/models.rs`
- Modify: `crates/dblens-core/locales/app.json`
- Modify: `crates/dblens-service/src/compare.rs`
- Modify: `crates/dblens-service/src/design.rs`
- Modify: `crates/dblens-service/src/lib.rs`
- Create: `crates/dblens-service/src/migrate.rs`
- Modify: `crates/dblens-service/src/task.rs`
- Create: `crates/dblens-service/tests/migrate_live.rs`

**Interfaces:**
- Consumes：任务 2；M4a 的 `Run`、`check_policy`；元数据服务；任务。
- Produces（`dblens_service`）：`MigrationPlan`、`MigrationEnd`、`MigrationPreview`、`OtherObject`；`Service::{preview_migration(&plan) -> Result<MigrationPreview>, start_migration(plan) -> Result<String>}`；接口见上。

- [ ] **Step 1: 修改 `crates/dblens-api/tests/models.rs`**

```diff
--- a/crates/dblens-api/tests/models.rs
+++ b/crates/dblens-api/tests/models.rs
@@ -309,3 +309,16 @@ async fn documents() {
         "{md}"
     );
 }
+
+/// 结构迁移（M5a）：方案能解析；连接不存在时预览和开始都报 404。
+#[tokio::test]
+async fn migrations_need_existing_profiles() {
+    let app = app(true);
+    let plan = json!({ "source": { "profile": "nope", "schema": "s" }, "target": { "profile": "nope2", "schema": "t" },
+                       "settings": { "options": { "name_case": "upper" }, "type_rules": [{ "source": "CLOB", "target": "TEXT" }] },
+                       "existing": { "a": "missing_only" } });
+    for path in ["/api/migrations/preview", "/api/migrations"] {
+        let (st, v) = call(&app, "POST", path, Some(plan.clone())).await;
+        assert_eq!(st, StatusCode::NOT_FOUND, "{path} {v}");
+    }
+}
```

- [ ] **Step 2: 新建 `crates/dblens-service/tests/migrate_live.rs`**

````rust
//! 结构迁移（M5a）的实库测试：源库和目标库的环境变量都设置时运行，否则直接通过（变量见 `live.rs`）。
//!
//! 在源库建一套测试结构（自增主键、默认值、注释、唯一约束、检查约束、索引、外键），迁到目标库，
//! 读回目标结构，与迁移草稿中的目标结构比对应无差异；最后清理两边。表名带源库的代号，不同路线互不干扰。
//! 人大金仓开发版最多 10 个连接，与其他实库测试一样要串行运行（`--test-threads=1`）。

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use dblens_core::LogicalType;
use dblens_core::schema::Table;
use dblens_dialect::{Dialect, Variant};
use dblens_driver::{ConnectParams, DriverId};
use dblens_service::{
    DefaultPolicy, DriverRegistry, Environment, ExecRequest, MigrationEnd, MigrationPlan, ProfileInput,
    SecretBackend, Secrets, Service, SessionEvent, Store, TaskState, TaskStatus,
};

const LIB_OPTIONS: [&str; 2] = ["oracle_client_lib_dir", "dpi_lib_dir"];

struct End {
    profile: String,
    driver: DriverId,
    variant: Variant,
    schema: String,
}

/// 一个服务中建两个连接配置；任一环境变量没有设置时返回 None。
async fn setup(envs: [&str; 2]) -> Option<(Arc<Service>, Vec<End>, tempfile::TempDir)> {
    let urls: Vec<String> = envs.iter().map(|e| std::env::var(e).ok()).collect::<Option<_>>()?;
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(Store::open(dir.path()).unwrap());
    let secrets = Arc::new(Secrets::with_backend(store.clone(), SecretBackend::Vault));
    secrets.init_vault("测试主密码").unwrap();
    let registry = Arc::new(DriverRegistry::new(store.clone()));
    let service = Arc::new(Service::with_parts(store, secrets, registry.clone(), Arc::new(DefaultPolicy)));
    let mut ends = Vec::new();
    for (env, url) in envs.iter().zip(urls) {
        let (driver, params) = ConnectParams::from_url(&url).unwrap();
        for option in LIB_OPTIONS {
            if let Some(lib) = params.options.get(option) {
                registry.set_lib_dir(driver, Some(Path::new(lib))).unwrap();
            }
        }
        let mut options = params.options.clone();
        options.retain(|k, _| !LIB_OPTIONS.contains(&k.as_str()));
        let schema = match driver {
            DriverId::Mysql => params.database.clone().unwrap(),
            DriverId::Postgres => "public".to_string(),
            DriverId::Oracle | DriverId::Dameng => params.user.to_uppercase(),
        };
        let input = ProfileInput {
            name: env.to_string(),
            driver,
            host: params.host.clone(),
            port: params.port,
            user: params.user.clone(),
            database: params.database.clone(),
            options,
            environment: Environment::Test,
            color: None,
            connect_timeout_secs: 10,
            product: None,
        };
        let summary = service.test_connection(input.clone(), &params.password).await.unwrap();
        let profile = service.create_profile(input, Some(&params.password)).unwrap();
        ends.push(End { profile: profile.id, driver, variant: summary.variant, schema });
    }
    Some((service, ends, dir))
}

/// 在一端执行若干语句；`tolerate` 为 true 时忽略失败（清理用）。
async fn run(s: &Service, end: &End, sqls: &[String], tolerate: bool) {
    let id = s.open_session(&end.profile).await.unwrap().id;
    for sql in sqls {
        s.sessions.submit(id, ExecRequest::new(sql)).await.unwrap();
        while let Some(ev) = s.sessions.next(id, 500).await.unwrap() {
            if let SessionEvent::Failed { error, .. } = &ev {
                assert!(tolerate, "{sql} 执行失败：{error}");
            }
        }
    }
    s.sessions.close(id, true).await.unwrap();
}

/// 删除测试表（子表在前）。
fn drops(end: &End, names: &[String]) -> Vec<String> {
    names
        .iter()
        .map(|n| match end.driver {
            DriverId::Oracle => {
                format!("BEGIN EXECUTE IMMEDIATE 'DROP TABLE \"{n}\" CASCADE CONSTRAINTS'; EXCEPTION WHEN OTHERS THEN NULL; END;")
            }
            DriverId::Dameng => format!("DROP TABLE IF EXISTS \"{n}\" CASCADE"),
            DriverId::Postgres => format!("DROP TABLE IF EXISTS \"{n}\" CASCADE"),
            DriverId::Mysql => format!("DROP TABLE IF EXISTS `{n}`"),
        })
        .collect()
}

/// 源库的测试结构：`p` 为表名前缀（Oracle、达梦用大写）。
fn source_ddl(end: &End, p: &str) -> Vec<String> {
    let opengauss = end.variant == Variant::OpenGauss;
    match end.driver {
        DriverId::Mysql => vec![
            format!(
                "CREATE TABLE {p}_cust (id BIGINT AUTO_INCREMENT PRIMARY KEY, name VARCHAR(50) NOT NULL DEFAULT 'n/a' COMMENT 'name', \
                 active TINYINT(1) DEFAULT 1, created DATETIME DEFAULT CURRENT_TIMESTAMP, amount DECIMAL(12,2) DEFAULT 0, note TEXT, \
                 CONSTRAINT {p}_ck_amount CHECK (amount >= 0), CONSTRAINT {p}_uk_name UNIQUE (name)) COMMENT = 'customers'"
            ),
            format!("CREATE INDEX {p}_ix_created ON {p}_cust (created)"),
            format!(
                "CREATE TABLE {p}_ord (id INT PRIMARY KEY, cust_id BIGINT, qty INT, status VARCHAR(10) DEFAULT 'new', \
                 CONSTRAINT {p}_ck_qty CHECK (qty > 0), \
                 CONSTRAINT {p}_fk_cust FOREIGN KEY (cust_id) REFERENCES {p}_cust (id) ON DELETE CASCADE)"
            ),
        ],
        DriverId::Postgres => vec![
            format!(
                "CREATE TABLE {p}_cust (id {}, name VARCHAR(50) NOT NULL DEFAULT 'n/a', \
                 active BOOLEAN DEFAULT true, created TIMESTAMP DEFAULT CURRENT_TIMESTAMP, amount NUMERIC(12,2) DEFAULT 0, note TEXT, \
                 CONSTRAINT {p}_ck_amount CHECK (amount >= 0), CONSTRAINT {p}_uk_name UNIQUE (name))",
                if opengauss {
                    "BIGSERIAL PRIMARY KEY"
                } else {
                    "BIGINT GENERATED BY DEFAULT AS IDENTITY PRIMARY KEY"
                }
            ),
            format!("COMMENT ON TABLE {p}_cust IS 'customers'"),
            format!("COMMENT ON COLUMN {p}_cust.name IS 'name'"),
            format!("CREATE INDEX {p}_ix_created ON {p}_cust (created)"),
            format!(
                "CREATE TABLE {p}_ord (id INT PRIMARY KEY, cust_id BIGINT, qty INT, status VARCHAR(10) DEFAULT 'new', \
                 CONSTRAINT {p}_ck_qty CHECK (qty > 0), \
                 CONSTRAINT {p}_fk_cust FOREIGN KEY (cust_id) REFERENCES {p}_cust (id) ON DELETE CASCADE)"
            ),
        ],
        DriverId::Oracle | DriverId::Dameng => {
            // 达梦的外键要求两边类型相同
            let (id, created, cust_id) = if end.driver == DriverId::Oracle {
                ("NUMBER(19) GENERATED BY DEFAULT AS IDENTITY", "DATE DEFAULT SYSDATE", "NUMBER(19)")
            } else {
                ("BIGINT IDENTITY(1, 1)", "TIMESTAMP DEFAULT SYSDATE", "BIGINT")
            };
            vec![
                format!(
                    "CREATE TABLE {p}_CUST (ID {id} PRIMARY KEY, NAME VARCHAR2(50) DEFAULT 'n/a' NOT NULL, \
                     ACTIVE NUMBER(1) DEFAULT 1, CREATED {created}, AMOUNT NUMBER(12,2) DEFAULT 0, NOTE CLOB, \
                     CONSTRAINT {p}_CK_AMOUNT CHECK (AMOUNT >= 0), CONSTRAINT {p}_UK_NAME UNIQUE (NAME))"
                ),
                format!("COMMENT ON TABLE {p}_CUST IS 'customers'"),
                format!("COMMENT ON COLUMN {p}_CUST.NAME IS 'name'"),
                format!("CREATE INDEX {p}_IX_CREATED ON {p}_CUST (CREATED)"),
                format!(
                    "CREATE TABLE {p}_ORD (ID NUMBER(10) PRIMARY KEY, CUST_ID {cust_id}, QTY NUMBER(10), STATUS VARCHAR2(10) DEFAULT 'new', \
                     CONSTRAINT {p}_CK_QTY CHECK (QTY > 0), \
                     CONSTRAINT {p}_FK_CUST FOREIGN KEY (CUST_ID) REFERENCES {p}_CUST (ID) ON DELETE CASCADE)"
                ),
            ]
        }
    }
}

/// 源库中的表名（Oracle、达梦为大写）。
fn names(end: &End, p: &str) -> Vec<String> {
    let up = matches!(end.driver, DriverId::Oracle | DriverId::Dameng);
    ["ord", "cust"]
        .iter()
        .map(|t| if up { format!("{p}_{t}").to_uppercase() } else { format!("{p}_{t}") })
        .collect()
}

async fn wait(s: &Service, id: &str) -> TaskStatus {
    for _ in 0..1200 {
        let st = s.tasks.status(id).unwrap();
        if !matches!(st.state, TaskState::Running | TaskState::Confirm) {
            return st;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("任务没有结束");
}

/// 源 → 目标迁移一次并核对。`p`：表名前缀（小写）。
async fn migrate_flow(src_env: &str, tgt_env: &str, p: &str) {
    let Some((s, ends, _dir)) = setup([src_env, tgt_env]).await else { return };
    let (src, tgt) = (&ends[0], &ends[1]);
    let route = format!("{src_env} → {tgt_env}");
    let up = matches!(src.driver, DriverId::Oracle | DriverId::Dameng);
    let prefix = if up { p.to_uppercase() } else { p.to_string() };
    let src_names = names(src, &prefix);
    let mut tgt_names = names(tgt, p);
    // MySQL 区分表名大小写：清理时大小写两种都删
    if tgt.driver == DriverId::Mysql {
        tgt_names.extend(names(tgt, p).iter().map(|n| n.to_uppercase()).collect::<Vec<_>>());
    }
    run(&s, src, &drops(src, &src_names), true).await;
    run(&s, tgt, &drops(tgt, &tgt_names), true).await;
    run(&s, src, &source_ddl(src, &prefix), false).await;

    let plan = MigrationPlan {
        source: MigrationEnd { profile: src.profile.clone(), schema: src.schema.clone() },
        tables: Some(src_names.iter().rev().cloned().collect()),
        sequences: Some(vec![]),
        target: MigrationEnd { profile: tgt.profile.clone(), schema: tgt.schema.clone() },
        settings: Default::default(),
        existing: BTreeMap::new(),
    };
    let preview = s.preview_migration(&plan).await.unwrap();
    let notes: Vec<String> = preview
        .draft
        .notes
        .iter()
        .map(|n| format!("{:?} {}.{:?} {:?}: {}", n.level, n.object, n.column, n.item, n.message.current()))
        .collect();
    eprintln!("{route} 提示：\n{}\n脚本：\n{}", notes.join("\n"), preview.text);
    assert!(preview.problems.is_empty(), "{route}：{:?}", preview.problems);
    assert_eq!(preview.draft.tables.len(), 2, "{route}");
    let id = s.start_migration(plan).await.unwrap();
    let st = wait(&s, &id).await;
    let failures: Vec<String> = st.failures.iter().map(|f| format!("{}\n  {}", f.text, f.error)).collect();
    assert_eq!(st.state, TaskState::Done, "{route}：{st:?}");
    assert!(failures.is_empty(), "{route} 失败的语句：\n{}", failures.join("\n"));
    let (report, _) = s.tasks.report(&id).unwrap();
    assert!(std::fs::read_to_string(report).unwrap().contains("```sql"), "{route}");

    // 读回目标结构，与草稿中的目标结构比对
    let tp = s.profile(&tgt.profile).unwrap();
    let dialect: Dialect = s.metadata.dialect(&tp).await.unwrap();
    let mut actual = Vec::new();
    for t in &preview.draft.tables {
        let mut got: Vec<Table> = s.metadata.tables(&tp, &tgt.schema, Some(&t.target.name)).await.unwrap();
        assert_eq!(got.len(), 1, "{route}：{} 没有建成", t.target.name);
        actual.push(Table { triggers: vec![], ..got.remove(0) });
    }
    for (t, got) in preview.draft.tables.iter().zip(&actual) {
        let (want, have) = (shape(&dialect, &t.target, &t.target), shape(&dialect, got, &t.target));
        assert_eq!(want, have, "{route}：{} 读回后与草稿不同", t.target.name);
    }

    run(&s, tgt, &drops(tgt, &tgt_names), true).await;
    run(&s, src, &drops(src, &src_names), true).await;
}

/// 表结构的要点（核对用）：列（名字、逻辑类型、可空、有无默认值、自增、注释）、主键、唯一约束、
/// 草稿中有的索引（库为外键自动建的索引不算）、外键、检查约束的个数、表注释。默认值和表达式的写法会被库规整，不比文本。
fn shape(d: &Dialect, t: &Table, draft: &Table) -> Vec<String> {
    let lower = |v: &[String]| v.iter().map(|x| x.to_lowercase()).collect::<Vec<_>>().join(",");
    let mut out = vec![format!("comment {:?}", t.comment)];
    for c in &t.columns {
        // NUMBER(p) 与 NUMBER(p,0) 相同
        let logical = match d.logical_type(&c.data_type.native) {
            LogicalType::Decimal { precision: Some(p), scale: None } => {
                LogicalType::Decimal { precision: Some(p), scale: Some(0) }
            }
            l => l,
        };
        out.push(format!(
            "col {} {:?} null={} default={} auto={} comment={:?}",
            c.name.to_lowercase(),
            logical,
            c.nullable,
            c.default.is_some() && !c.auto_increment,
            c.auto_increment,
            c.comment
        ));
    }
    out.push(format!("pk {:?}", t.primary_key.as_ref().map(|k| lower(&k.columns))));
    let mut uniques: Vec<String> = t.uniques.iter().map(|u| lower(&u.columns)).collect();
    uniques.sort();
    out.push(format!("unique {uniques:?}"));
    let wanted: Vec<String> = draft
        .indexes
        .iter()
        .map(|i| lower(&i.columns.iter().map(|c| c.name.clone()).collect::<Vec<_>>()))
        .collect();
    let mut indexes: Vec<String> = t
        .indexes
        .iter()
        .map(|i| lower(&i.columns.iter().map(|c| c.name.clone()).collect::<Vec<_>>()))
        .filter(|i| wanted.contains(i))
        .collect();
    indexes.sort();
    out.push(format!("index {indexes:?}"));
    let mut fks: Vec<String> = t
        .foreign_keys
        .iter()
        .map(|f| {
            format!(
                "{} -> {}({}) {:?}",
                lower(&f.columns),
                f.ref_table.to_lowercase(),
                lower(&f.ref_columns),
                f.on_delete
            )
        })
        .collect();
    fks.sort();
    out.push(format!("fk {fks:?}"));
    out.push(format!("checks {}", t.checks.len()));
    out
}

/// 每个源库迁到 MySQL、PG、Oracle、达梦（不迁到自己），另加信创常见路线。
macro_rules! routes {
    ($($name:ident: $src:literal => $tgt:literal, $p:literal;)*) => {
        $(
            #[tokio::test]
            async fn $name() {
                migrate_flow($src, $tgt, $p).await;
            }
        )*
    };
}

routes! {
    mysql_to_pg: "DBLENS_TEST_MYSQL_URL" => "DBLENS_TEST_PG_URL", "m5my";
    mysql_to_oracle: "DBLENS_TEST_MYSQL_URL" => "DBLENS_TEST_ORACLE_URL", "m5my";
    mysql_to_dameng: "DBLENS_TEST_MYSQL_URL" => "DBLENS_TEST_DM_URL", "m5my";
    mysql_to_tidb: "DBLENS_TEST_MYSQL_URL" => "DBLENS_TEST_TIDB_URL", "m5my";
    mysql_to_oceanbase: "DBLENS_TEST_MYSQL_URL" => "DBLENS_TEST_OCEANBASE_URL", "m5my";
    mariadb_to_mysql: "DBLENS_TEST_MARIADB_URL" => "DBLENS_TEST_MYSQL_URL", "m5ma";
    mariadb_to_pg: "DBLENS_TEST_MARIADB_URL" => "DBLENS_TEST_PG_URL", "m5ma";
    mariadb_to_oracle: "DBLENS_TEST_MARIADB_URL" => "DBLENS_TEST_ORACLE_URL", "m5ma";
    mariadb_to_dameng: "DBLENS_TEST_MARIADB_URL" => "DBLENS_TEST_DM_URL", "m5ma";
    pg_to_mysql: "DBLENS_TEST_PG_URL" => "DBLENS_TEST_MYSQL_URL", "m5pg";
    pg_to_oracle: "DBLENS_TEST_PG_URL" => "DBLENS_TEST_ORACLE_URL", "m5pg";
    pg_to_dameng: "DBLENS_TEST_PG_URL" => "DBLENS_TEST_DM_URL", "m5pg";
    oracle_to_mysql: "DBLENS_TEST_ORACLE_URL" => "DBLENS_TEST_MYSQL_URL", "m5or";
    oracle_to_pg: "DBLENS_TEST_ORACLE_URL" => "DBLENS_TEST_PG_URL", "m5or";
    oracle_to_dameng: "DBLENS_TEST_ORACLE_URL" => "DBLENS_TEST_DM_URL", "m5or";
    oracle_to_kingbase: "DBLENS_TEST_ORACLE_URL" => "DBLENS_TEST_KINGBASE_URL", "m5or";
    oracle_to_opengauss: "DBLENS_TEST_ORACLE_URL" => "DBLENS_TEST_OPENGAUSS_A_URL", "m5or";
    dameng_to_mysql: "DBLENS_TEST_DM_URL" => "DBLENS_TEST_MYSQL_URL", "m5dm";
    dameng_to_pg: "DBLENS_TEST_DM_URL" => "DBLENS_TEST_PG_URL", "m5dm";
    dameng_to_oracle: "DBLENS_TEST_DM_URL" => "DBLENS_TEST_ORACLE_URL", "m5dm";
    tidb_to_mysql: "DBLENS_TEST_TIDB_URL" => "DBLENS_TEST_MYSQL_URL", "m5ti";
    tidb_to_pg: "DBLENS_TEST_TIDB_URL" => "DBLENS_TEST_PG_URL", "m5ti";
    tidb_to_oracle: "DBLENS_TEST_TIDB_URL" => "DBLENS_TEST_ORACLE_URL", "m5ti";
    tidb_to_dameng: "DBLENS_TEST_TIDB_URL" => "DBLENS_TEST_DM_URL", "m5ti";
    oceanbase_to_mysql: "DBLENS_TEST_OCEANBASE_URL" => "DBLENS_TEST_MYSQL_URL", "m5ob";
    oceanbase_to_pg: "DBLENS_TEST_OCEANBASE_URL" => "DBLENS_TEST_PG_URL", "m5ob";
    oceanbase_to_oracle: "DBLENS_TEST_OCEANBASE_URL" => "DBLENS_TEST_ORACLE_URL", "m5ob";
    oceanbase_to_dameng: "DBLENS_TEST_OCEANBASE_URL" => "DBLENS_TEST_DM_URL", "m5ob";
    opengauss_to_mysql: "DBLENS_TEST_OPENGAUSS_URL" => "DBLENS_TEST_MYSQL_URL", "m5og";
    opengauss_to_pg: "DBLENS_TEST_OPENGAUSS_URL" => "DBLENS_TEST_PG_URL", "m5og";
    opengauss_to_oracle: "DBLENS_TEST_OPENGAUSS_URL" => "DBLENS_TEST_ORACLE_URL", "m5og";
    opengauss_to_dameng: "DBLENS_TEST_OPENGAUSS_URL" => "DBLENS_TEST_DM_URL", "m5og";
    opengauss_a_to_mysql: "DBLENS_TEST_OPENGAUSS_A_URL" => "DBLENS_TEST_MYSQL_URL", "m5oa";
    opengauss_a_to_pg: "DBLENS_TEST_OPENGAUSS_A_URL" => "DBLENS_TEST_PG_URL", "m5oa";
    opengauss_a_to_oracle: "DBLENS_TEST_OPENGAUSS_A_URL" => "DBLENS_TEST_ORACLE_URL", "m5oa";
    opengauss_a_to_dameng: "DBLENS_TEST_OPENGAUSS_A_URL" => "DBLENS_TEST_DM_URL", "m5oa";
    kingbase_to_mysql: "DBLENS_TEST_KINGBASE_URL" => "DBLENS_TEST_MYSQL_URL", "m5kb";
    kingbase_to_pg: "DBLENS_TEST_KINGBASE_URL" => "DBLENS_TEST_PG_URL", "m5kb";
    kingbase_to_oracle: "DBLENS_TEST_KINGBASE_URL" => "DBLENS_TEST_ORACLE_URL", "m5kb";
    kingbase_to_dameng: "DBLENS_TEST_KINGBASE_URL" => "DBLENS_TEST_DM_URL", "m5kb";
    kingbase_pg_to_mysql: "DBLENS_TEST_KINGBASE_PG_URL" => "DBLENS_TEST_MYSQL_URL", "m5kp";
    kingbase_pg_to_pg: "DBLENS_TEST_KINGBASE_PG_URL" => "DBLENS_TEST_PG_URL", "m5kp";
    kingbase_pg_to_oracle: "DBLENS_TEST_KINGBASE_PG_URL" => "DBLENS_TEST_ORACLE_URL", "m5kp";
    kingbase_pg_to_dameng: "DBLENS_TEST_KINGBASE_PG_URL" => "DBLENS_TEST_DM_URL", "m5kp";
}
````

- [ ] **Step 3: 修改 `crates/dblens-api/src/lib.rs`**

```diff
--- a/crates/dblens-api/src/lib.rs
+++ b/crates/dblens-api/src/lib.rs
@@ -8,6 +8,7 @@ mod documents;
 mod error;
 mod exports;
 mod lang;
+mod migrations;
 mod models;
 mod routes;
 mod tasks;
@@ -118,6 +119,8 @@ pub fn router(service: Arc<Service>, config: ApiConfig) -> Router {
         .route("/documents", post(documents::start))
         .route("/documents/check", post(documents::check))
         .route("/documents/templates/export", post(documents::export_template))
+        .route("/migrations", post(migrations::start))
+        .route("/migrations/preview", post(migrations::preview))
         .route("/exports", post(exports::create))
         .route("/exports/{id}", get(exports::status).delete(exports::cancel_or_remove))
         .route("/exports/{id}/file", get(exports::file))
```

- [ ] **Step 4: 新建 `crates/dblens-api/src/migrations.rs`**

```rust
//! 结构迁移的接口（M5a 设计文档 3.2 节）：预览（转换结果、脚本、开始前检查），开始迁移。

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use dblens_service::{MigrationPlan, MigrationPreview};
use serde_json::{Value, json};

use crate::AppState;
use crate::error::ApiResult;

/// 预览：读结构、转换、检查、生成脚本。
pub async fn preview(
    State(s): State<AppState>,
    Json(plan): Json<MigrationPlan>,
) -> ApiResult<Json<MigrationPreview>> {
    Ok(Json(s.service.preview_migration(&plan).await?))
}

/// 开始迁移：返回任务 id。
pub async fn start(
    State(s): State<AppState>,
    Json(plan): Json<MigrationPlan>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    let id = s.service.start_migration(plan).await?;
    Ok((StatusCode::CREATED, Json(json!({ "id": id }))))
}
```

- [ ] **Step 5: 修改 `crates/dblens-core/locales/app.json`**

```diff
--- a/crates/dblens-core/locales/app.json
+++ b/crates/dblens-core/locales/app.json
@@ -152,6 +152,7 @@
   "metadata.table_detail_count": {"zh-CN": "表明细查询数量不对", "en": "Wrong number of table detail queries"},
   "migrate.check_dropped": {"zh-CN": "检查约束 %{expression} 不能转换，不迁移", "en": "Check constraint %{expression} cannot be converted; not migrated"},
   "migrate.default_dropped": {"zh-CN": "默认值 %{default} 不能转换，不迁移", "en": "Default %{default} cannot be converted; not migrated"},
+  "migrate.existing_unchosen": {"zh-CN": "目标中已有这些表，请选择处理方式：%{tables}", "en": "These tables already exist in the target; choose what to do: %{tables}"},
   "migrate.fk_action": {"zh-CN": "目标库不支持 %{action}，去掉", "en": "The target does not support %{action}; dropped"},
   "migrate.fk_outside": {"zh-CN": "引用的表 %{table} 不在迁移范围内，目标中也没有，不迁移", "en": "Referenced table %{table} is neither migrated nor in the target; not migrated"},
   "migrate.generated_dropped": {"zh-CN": "生成列表达式 %{expression} 不能转换，改为普通列", "en": "Generated column expression %{expression} cannot be converted; migrated as a plain column"},
@@ -165,13 +166,26 @@
   "migrate.name_conflict": {"zh-CN": "名字 %{name} 与 %{other} 重名", "en": "Name %{name} conflicts with %{other}"},
   "migrate.name_too_long": {"zh-CN": "名字 %{name} 超过目标库的长度上限", "en": "Name %{name} exceeds the target's length limit"},
   "migrate.no_sequences": {"zh-CN": "目标库没有序列，不迁移", "en": "The target database has no sequences; not migrated"},
+  "migrate.nothing": {"zh-CN": "没有要迁移的表或序列", "en": "There are no tables or sequences to migrate"},
   "migrate.partition": {"zh-CN": "分区不迁移，按普通表建", "en": "Partitioning is not migrated; created as a plain table"},
+  "migrate.report.cancelled": {"zh-CN": "迁移已取消，之后的语句没有执行。", "en": "Migration cancelled; later statements were not run."},
+  "migrate.report.failed": {"zh-CN": "迁移中断：%{error}", "en": "Migration stopped: %{error}"},
+  "migrate.report.failures": {"zh-CN": "失败的语句", "en": "Failed statements"},
+  "migrate.report.finished": {"zh-CN": "迁移已完成。", "en": "Migration finished."},
+  "migrate.report.no_ddl": {"zh-CN": "（取不到 DDL）", "en": "(DDL unavailable)"},
+  "migrate.report.notes": {"zh-CN": "映射提示", "en": "Mapping notes"},
+  "migrate.report.others": {"zh-CN": "没有迁移的对象（附源库 DDL，需人工改写）", "en": "Objects not migrated (source DDL attached; rewrite by hand)"},
+  "migrate.report.script": {"zh-CN": "迁移脚本", "en": "Migration script"},
+  "migrate.report.summary": {"zh-CN": "共 %{total} 条语句：执行 %{done} 条，其中失败 %{failed} 条、因依赖的表没有建成而跳过 %{skipped} 条；用时 %{seconds} 秒。", "en": "%{total} statements: %{done} run, %{failed} failed, %{skipped} skipped because a table they depend on was not created; %{seconds} s."},
+  "migrate.report.title": {"zh-CN": "结构迁移：%{source} → %{target}", "en": "Structure migration: %{source} → %{target}"},
+  "migrate.same_target": {"zh-CN": "源和目标是同一个 schema", "en": "The source and target are the same schema"},
   "migrate.sequence_bound": {"zh-CN": "%{bound} 超出 bigint 范围", "en": "%{bound} is outside the bigint range"},
   "migrate.sequence_exists": {"zh-CN": "目标中已有同名序列，跳过", "en": "A sequence with this name already exists in the target; skipped"},
   "migrate.serial_sequence": {"zh-CN": "serial 列自带的序列，随自增列一起迁移", "en": "Sequence owned by a serial column; migrated with the identity column"},
   "migrate.type_lossy": {"zh-CN": "%{from} 映射为 %{to}，取值范围或精度可能改变", "en": "%{from} maps to %{to}; range or precision may change"},
   "migrate.type_unknown": {"zh-CN": "不认识的类型 %{source}，按文本（%{target}）迁移", "en": "Unrecognized type %{source}; migrated as text (%{target})"},
   "migrate.unknown_target_type": {"zh-CN": "目标库不认识类型 %{native}", "en": "The target does not recognize type %{native}"},
+  "migrate.unsupported_left": {"zh-CN": "还有 %{count} 项不支持的映射需要处理", "en": "%{count} unsupported mappings still need attention"},
   "model.bad_file": {"zh-CN": "文件 %{path} 有错：%{error}", "en": "File %{path} is invalid: %{error}"},
   "model.bad_path": {"zh-CN": "路径无效：%{path}", "en": "Invalid path: %{path}"},
   "model.conflict": {"zh-CN": "%{name} 的文件已被修改，请重新加载", "en": "The file for %{name} has changed; reload it"},
```

- [ ] **Step 6: 修改 `crates/dblens-service/src/compare.rs`**

```diff
--- a/crates/dblens-service/src/compare.rs
+++ b/crates/dblens-service/src/compare.rs
@@ -439,7 +439,7 @@ impl Service {
 }
 
 #[cfg(test)]
-mod tests {
+pub(crate) mod tests {
     use std::time::Duration;
 
     use dblens_core::schema::{Column, PrimaryKey};
@@ -459,7 +459,7 @@ mod tests {
     use crate::store::Store;
     use crate::task::{TaskState, TaskStatus};
 
-    fn service() -> (Arc<Service>, String, Log, tempfile::TempDir) {
+    pub(crate) fn service() -> (Arc<Service>, String, Log, tempfile::TempDir) {
         let store = Arc::new(Store::open_in_memory().unwrap());
         let secrets = Arc::new(Secrets::with_backend(store.clone(), SecretBackend::Vault));
         secrets.init_vault("master").unwrap();
```

- [ ] **Step 7: 修改 `crates/dblens-service/src/design.rs`**

```diff
--- a/crates/dblens-service/src/design.rs
+++ b/crates/dblens-service/src/design.rs
@@ -408,7 +408,7 @@ impl Run<'_> {
     }
 
     /// 执行一条语句并记入历史。
-    async fn sql(&self, conn: &mut Box<dyn Connection>, sql: &str) -> Result<u64> {
+    pub(crate) async fn sql(&self, conn: &mut Box<dyn Connection>, sql: &str) -> Result<u64> {
         let (at, started) = (now_ms(), Instant::now());
         let result = exec(conn, sql, &[]).await;
         self.record(sql, at, started, result.as_ref().ok().copied(), result.as_ref().err());
```

- [ ] **Step 8: 修改 `crates/dblens-service/src/lib.rs`**

```diff
--- a/crates/dblens-service/src/lib.rs
+++ b/crates/dblens-service/src/lib.rs
@@ -13,6 +13,7 @@ mod favorite;
 mod history;
 mod import;
 mod metadata;
+mod migrate;
 mod model;
 mod objects;
 mod policy;
@@ -45,6 +46,7 @@ pub use import::{
     infer_type, new_table, preview_file,
 };
 pub use metadata::{METADATA_POOL_SIZE, MetadataCache, MetadataService};
+pub use migrate::{MigrationEnd, MigrationPlan, MigrationPreview, OtherObject};
 pub use model::{
     DEFAULT_DIAGRAM, Diagram, DiagramDisplay, DiagramSummary, DiagramTable, DiagramTables, DiagramView,
     DiagramWarning, FileError, LogicalKey, ModelDatabase, ModelDesignPreview, ModelDesignRequest, ModelInfo,
```

- [ ] **Step 9: 新建 `crates/dblens-service/src/migrate.rs`**

````rust
//! 结构迁移（M5a 设计文档第 3.2、6 节）：读源和目标的结构，按迁移方案转换、检查、生成脚本；
//! 作为任务逐条执行（某张表建表失败时跳过依赖它的语句），最后写迁移报告（markdown）。转换本身在 `dblens_dialect::migrate`。

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::sync::Arc;
use std::time::Instant;

use dblens_core::schema::{RoutineKind, Sequence, Table};
use dblens_core::{Error, Lang, Msg, Result, current_lang, msg};
use dblens_dialect::meta::{MetaArgs, MetaKey};
use dblens_dialect::migrate::{
    ExistingAction, MigrationDraft, MigrationInput, MigrationSettings, MigrationStatement, NoteLevel,
    StatementKind, migrate, migration_script, script_text,
};
use dblens_dialect::{DdlKind, Dialect, Variant};
use serde::{Deserialize, Serialize};

use crate::design::Run;
use crate::profile::Profile;
use crate::service::Service;
use crate::task::{TaskFailure, TaskHandle, TaskKind, TaskPhase};

/// 迁移的一端：连接和 schema。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MigrationEnd {
    pub profile: String,
    pub schema: String,
}

/// 迁移方案（M5a 设计文档 3.3 节）：可另存为 JSON 再打开。`tables`、`sequences` 为 None 时迁移源 schema 的全部表、序列；
/// `existing`：目标中已有同名表时的处理方式（按源表名）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MigrationPlan {
    pub source: MigrationEnd,
    #[serde(default)]
    pub tables: Option<Vec<String>>,
    /// 迁移的序列；None 为全部。
    #[serde(default)]
    pub sequences: Option<Vec<String>>,
    pub target: MigrationEnd,
    #[serde(default)]
    pub settings: MigrationSettings,
    #[serde(default)]
    pub existing: BTreeMap<String, ExistingAction>,
}

/// 不迁移的对象（视图、例程、触发器、同义词）。触发器的 `table` 是所属的表。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OtherObject {
    pub kind: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub table: Option<String>,
}

/// 预览：转换结果、脚本、开始前检查的问题、执行策略的结论。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MigrationPreview {
    pub source_variant: Variant,
    pub target_variant: Variant,
    /// 迁移范围内的源表（界面对照显示源类型）。
    pub source_tables: Vec<Table>,
    pub source_sequences: Vec<Sequence>,
    pub draft: MigrationDraft,
    pub others: Vec<OtherObject>,
    pub statements: Vec<MigrationStatement>,
    pub text: String,
    /// 不能开始的原因；为空才能开始。
    pub problems: Vec<Msg>,
    pub denied: Option<Msg>,
    pub confirm: Vec<Msg>,
}

/// 读出的两端。
struct Loaded {
    source: Profile,
    target: Profile,
    source_dialect: Dialect,
    target_dialect: Dialect,
    tables: Vec<Table>,
    sequences: Vec<Sequence>,
    target_tables: Vec<Table>,
    target_sequences: Vec<String>,
    others: Vec<OtherObject>,
}

/// 开始前检查（不读库的部分）：有不支持的项、同名表没有选择处理方式、没有要迁移的对象、源和目标相同。
fn problems(plan: &MigrationPlan, draft: &MigrationDraft) -> Vec<Msg> {
    let mut out = Vec::new();
    if plan.source == plan.target {
        out.push(msg!("migrate.same_target"));
    }
    if draft.tables.is_empty() && draft.sequences.is_empty() {
        out.push(msg!("migrate.nothing"));
    }
    let blocked = draft.notes.iter().filter(|n| n.level == NoteLevel::Unsupported).count();
    if blocked > 0 {
        out.push(msg!("migrate.unsupported_left", count = blocked));
    }
    let unchosen: Vec<&str> = draft
        .tables
        .iter()
        .filter(|t| t.exists && !plan.existing.contains_key(&t.source))
        .map(|t| t.source.as_str())
        .collect();
    if !unchosen.is_empty() {
        out.push(msg!("migrate.existing_unchosen", tables = unchosen.join(", ")));
    }
    out
}

impl Service {
    /// 读两端的结构。`fresh`：跳过元数据缓存（开始执行前以库中最新的结构为准）。
    async fn load_migration(&self, plan: &MigrationPlan, fresh: bool) -> Result<Loaded> {
        let source = self.profile(&plan.source.profile)?;
        let target = self.profile(&plan.target.profile)?;
        let source_dialect = self.metadata.dialect(&source).await?;
        let target_dialect = self.metadata.dialect(&target).await?;
        let schemas = self.metadata.schemas(&target, None).await?;
        if !schemas.iter().any(|s| s.name == plan.target.schema) {
            return Err(Error::not_found(msg!(
                "noun.object",
                kind = "schema",
                name = format!("{}.{}", target.input.name, plan.target.schema)
            )));
        }
        if fresh {
            for (p, schema) in [(&source, &plan.source.schema), (&target, &plan.target.schema)] {
                let args = MetaArgs { schema: Some(schema.clone()), ..Default::default() };
                for key in MetaKey::TABLE_DETAIL.into_iter().chain([MetaKey::Sequences]) {
                    self.metadata.cache().forget(&p.id, key, &args);
                }
            }
        }
        let wanted = |name: &str| plan.tables.as_ref().is_none_or(|t| t.iter().any(|x| x == name));
        let all = self.metadata.tables(&source, &plan.source.schema, None).await?;
        let tables: Vec<Table> = all.into_iter().filter(|t| wanted(&t.name)).collect();
        let sequences = if source_dialect.has_meta(MetaKey::Sequences) {
            let wanted = |name: &str| plan.sequences.as_ref().is_none_or(|t| t.iter().any(|x| x == name));
            let all = self.metadata.sequences(&source, &plan.source.schema).await?;
            all.into_iter().filter(|s| wanted(&s.name)).collect()
        } else {
            vec![]
        };
        let target_tables = self.metadata.tables(&target, &plan.target.schema, None).await?;
        let target_sequences = if target_dialect.has_meta(MetaKey::Sequences) {
            self.metadata.sequences(&target, &plan.target.schema).await?.into_iter().map(|s| s.name).collect()
        } else {
            vec![]
        };
        let others = self.other_objects(&source, &source_dialect, &plan.source.schema, &tables).await?;
        Ok(Loaded {
            source,
            target,
            source_dialect,
            target_dialect,
            tables,
            sequences,
            target_tables,
            target_sequences,
            others,
        })
    }

    /// 不迁移的对象：视图、物化视图、函数、过程、包、同义词，以及迁移的表上的触发器。
    async fn other_objects(
        &self,
        profile: &Profile,
        dialect: &Dialect,
        schema: &str,
        tables: &[Table],
    ) -> Result<Vec<OtherObject>> {
        let mut out = Vec::new();
        let obj = |kind: DdlKind, name: String| OtherObject { kind: kind.as_str().into(), name, table: None };
        for (key, kind) in
            [(MetaKey::Views, DdlKind::View), (MetaKey::MaterializedViews, DdlKind::MaterializedView)]
        {
            if dialect.has_meta(key) {
                out.extend(
                    self.metadata.views(profile, schema, key).await?.into_iter().map(|v| obj(kind, v.name)),
                );
            }
        }
        if dialect.has_meta(MetaKey::Routines) {
            for r in self.metadata.routines(profile, schema).await? {
                let kind = match r.kind {
                    RoutineKind::Procedure => DdlKind::Procedure,
                    RoutineKind::Function => DdlKind::Function,
                    RoutineKind::Package => DdlKind::Package,
                    RoutineKind::PackageBody => DdlKind::PackageBody,
                };
                out.push(obj(kind, r.name));
            }
        }
        if dialect.has_meta(MetaKey::Synonyms) {
            out.extend(
                self.metadata
                    .synonyms(profile, schema)
                    .await?
                    .into_iter()
                    .map(|s| obj(DdlKind::Synonym, s.name)),
            );
        }
        for t in tables {
            for tr in &t.triggers {
                out.push(OtherObject {
                    kind: "trigger".into(),
                    name: tr.name.clone(),
                    table: Some(t.name.clone()),
                });
            }
        }
        Ok(out)
    }

    async fn prepare_migration(
        &self,
        plan: &MigrationPlan,
        fresh: bool,
    ) -> Result<(Loaded, MigrationPreview)> {
        let l = self.load_migration(plan, fresh).await?;
        let input = MigrationInput {
            source: l.source_dialect,
            target: l.target_dialect,
            tables: &l.tables,
            sequences: &l.sequences,
            target_tables: &l.target_tables,
            target_sequences: &l.target_sequences,
        };
        let draft = migrate(&input, &plan.settings);
        let statements = migration_script(
            &l.target_dialect,
            &plan.target.schema,
            &draft,
            &plan.existing,
            &l.target_tables,
        )?;
        let sqls: Vec<String> = statements.iter().map(|s| s.sql.clone()).collect();
        let (denied, confirm) = self.check_policy(&l.target, l.target_dialect, &sqls).await;
        let preview = MigrationPreview {
            source_variant: l.source_dialect.variant(),
            target_variant: l.target_dialect.variant(),
            source_tables: l.tables.clone(),
            source_sequences: l.sequences.clone(),
            problems: problems(plan, &draft),
            text: script_text(&statements),
            draft,
            others: l.others.clone(),
            statements,
            denied,
            confirm,
        };
        Ok((l, preview))
    }

    /// 预览迁移：读结构（经元数据缓存）、转换、检查、生成脚本。不执行。
    pub async fn preview_migration(&self, plan: &MigrationPlan) -> Result<MigrationPreview> {
        Ok(self.prepare_migration(plan, false).await?.1)
    }

    /// 开始迁移：以库中最新的结构重新转换，检查不通过则拒绝；作为任务执行，返回任务 id。
    pub async fn start_migration(self: &Arc<Self>, plan: MigrationPlan) -> Result<String> {
        let (l, preview) = self.prepare_migration(&plan, true).await?;
        if let Some(p) = preview.problems.first() {
            return Err(Error::invalid(p.clone()));
        }
        if let Some(reason) = preview.denied.clone() {
            return Err(Error::Denied { reason });
        }
        let password = self.secrets.get(&l.target.id)?.unwrap_or_default();
        let task = self.tasks.create(TaskKind::Migrate)?;
        let mut id = String::new();
        task.update(|s| {
            id = s.id.clone();
            s.phase = TaskPhase::Executing;
            s.progress.statements_total = preview.statements.len() as u64;
        });
        let lang = current_lang();
        let service = self.clone();
        tokio::spawn(async move {
            let started = Instant::now();
            let run = Run {
                service: &service,
                task: &task,
                profile: &l.target,
                dialect: l.target_dialect,
                schema: &plan.target.schema,
            };
            let outcome = run_statements(&run, &preview.statements, password).await;
            service.metadata.cache().invalidate(&l.target.id, Some(&plan.target.schema));
            let report = service.migration_report(&plan, &l, &preview, &task, &outcome, started, lang).await;
            let result = match (outcome, report) {
                (Err(e), _) | (Ok(_), Err(e)) => Err(e),
                (Ok(_), Ok(())) => Ok(()),
            };
            task.finish(result);
        });
        Ok(id)
    }

    /// 写迁移报告：结果汇总、失败的语句、提示、不迁移的对象及其源库 DDL。
    #[allow(clippy::too_many_arguments)]
    async fn migration_report(
        &self,
        plan: &MigrationPlan,
        l: &Loaded,
        preview: &MigrationPreview,
        task: &TaskHandle,
        outcome: &Result<Outcome>,
        started: Instant,
        lang: Lang,
    ) -> Result<()> {
        let t = |m: Msg| m.render(lang);
        let mut md = String::new();
        let src = format!("{} / {}", l.source.input.name, plan.source.schema);
        let tgt = format!("{} / {}", l.target.input.name, plan.target.schema);
        let _ = writeln!(md, "# {}\n", t(msg!("migrate.report.title", source = &src, target = &tgt)));
        let status = task.status();
        let state = match outcome {
            Ok(_) => t(msg!("migrate.report.finished")),
            Err(Error::Cancelled) => t(msg!("migrate.report.cancelled")),
            Err(e) => t(msg!("migrate.report.failed", error = e.to_string())),
        };
        let _ = writeln!(md, "{state}\n");
        let p = &status.progress;
        let skipped = outcome.as_ref().map(|o| o.skipped).unwrap_or(0);
        let _ = writeln!(
            md,
            "{}\n",
            t(msg!(
                "migrate.report.summary",
                done = p.statements_done,
                failed = p.statements_failed,
                skipped = skipped,
                total = p.statements_total,
                seconds = started.elapsed().as_secs()
            ))
        );
        if !status.failures.is_empty() {
            let _ = writeln!(md, "## {}\n", t(msg!("migrate.report.failures")));
            for f in &status.failures {
                let _ = writeln!(md, "```sql\n{}\n```\n\n{}\n", f.text, f.error);
            }
        }
        let notes: Vec<_> = preview.draft.notes.iter().filter(|n| n.level != NoteLevel::Info).collect();
        if !notes.is_empty() {
            let _ = writeln!(md, "## {}\n", t(msg!("migrate.report.notes")));
            for n in notes {
                let at = [Some(n.object.as_str()), n.column.as_deref()]
                    .into_iter()
                    .flatten()
                    .collect::<Vec<_>>()
                    .join(".");
                let item = n.item.as_deref().map(|i| format!(" [{i}]")).unwrap_or_default();
                let _ = writeln!(md, "- {at}{item}: {}", n.message.render(lang));
            }
            md.push('\n');
        }
        if !preview.others.is_empty() {
            let _ = writeln!(md, "## {}\n", t(msg!("migrate.report.others")));
            for o in &preview.others {
                let _ = writeln!(md, "### {} {}\n", o.kind, o.name);
                let ddl = match (o.kind.as_str(), &o.table) {
                    ("trigger", Some(table)) => l
                        .tables
                        .iter()
                        .find(|x| &x.name == table)
                        .and_then(|x| x.triggers.iter().find(|tr| tr.name == o.name))
                        .and_then(|tr| tr.definition.clone()),
                    (kind, _) => match DdlKind::parse(kind) {
                        Some(k) => self.metadata.ddl(&l.source, &plan.source.schema, k, &o.name).await.ok(),
                        None => None,
                    },
                };
                match ddl {
                    Some(d) => {
                        let _ = writeln!(md, "```sql\n{}\n```\n", d.trim());
                    }
                    None => {
                        let _ = writeln!(md, "{}\n", t(msg!("migrate.report.no_ddl")));
                    }
                }
            }
        }
        let _ = writeln!(md, "## {}\n\n```sql\n{}\n```", t(msg!("migrate.report.script")), preview.text);
        let path = task.report_path("md", format!("migration-{}.md", plan.target.schema))?;
        std::fs::write(&path, md).map_err(|e| Error::internal(msg!("export.write_failed", error = e)))
    }
}

/// 执行的结果：建表失败的源表、因此跳过的语句数。
#[derive(Debug, Default)]
pub(crate) struct Outcome {
    pub failed_tables: BTreeSet<String>,
    pub skipped: u64,
}

/// 逐条执行：失败的语句记下后继续；建表（或删除重建时的删表）失败的表，依赖它的语句跳过。
pub(crate) async fn run_statements(
    run: &Run<'_>,
    statements: &[MigrationStatement],
    password: String,
) -> Result<Outcome> {
    let (mut conn, _) = run.service.registry.connect(run.profile, password).await?;
    run.task.set_cancel(Some(conn.cancel_handle()))?;
    let mut out = Outcome::default();
    let result = async {
        run.sql(&mut conn, &run.dialect.set_schema_sql(run.schema)).await?;
        for (i, s) in statements.iter().enumerate() {
            if run.task.cancelled() {
                return Err(Error::Cancelled);
            }
            if s.requires.iter().any(|r| out.failed_tables.contains(r)) {
                out.skipped += 1;
                run.task.update(|st| st.progress.statements_done += 1);
                continue;
            }
            match run.sql(&mut conn, &s.sql).await {
                Ok(_) => run.task.update(|st| st.progress.statements_done += 1),
                Err(e) => {
                    if run.task.cancelled() {
                        return Err(Error::Cancelled);
                    }
                    if matches!(s.kind, StatementKind::CreateTable | StatementKind::DropTable) {
                        out.failed_tables.insert(s.object.clone());
                    }
                    let failure =
                        TaskFailure { index: i as u64, line: i as u64 + 1, error: e, text: s.sql.clone() };
                    run.task.fail(failure, s.sql.clone());
                    run.task.update(|st| {
                        st.progress.statements_done += 1;
                        st.progress.statements_failed += 1;
                    });
                }
            }
        }
        Ok(())
    }
    .await;
    let _ = conn.close().await;
    result.map(|()| out)
}

#[cfg(test)]
mod tests {
    use dblens_dialect::migrate::{MigrationNote, ObjectKind, TableMapping};

    use super::*;
    use crate::compare::tests::service;
    use crate::model::tests::wait;
    use crate::task::TaskState;

    fn end(p: &str, s: &str) -> MigrationEnd {
        MigrationEnd { profile: p.into(), schema: s.into() }
    }

    fn plan() -> MigrationPlan {
        MigrationPlan {
            source: end("a", "s"),
            tables: None,
            sequences: None,
            target: end("b", "t"),
            settings: MigrationSettings::default(),
            existing: BTreeMap::new(),
        }
    }

    fn mapping(name: &str, exists: bool) -> TableMapping {
        TableMapping {
            source: name.into(),
            target: Table { name: name.into(), ..Default::default() },
            columns: vec![],
            exists,
        }
    }

    #[test]
    fn problems_block_the_start() {
        let mut draft = MigrationDraft { tables: vec![], sequences: vec![], notes: vec![] };
        let mut p = plan();
        p.target = p.source.clone();
        let keys = |v: Vec<Msg>| v.iter().map(|m| m.render(Lang::En)).collect::<Vec<_>>();
        assert_eq!(keys(problems(&p, &draft)).len(), 2);
        draft.tables = vec![mapping("a", true), mapping("b", true), mapping("c", false)];
        draft.notes.push(MigrationNote {
            kind: ObjectKind::Table,
            object: "c".into(),
            column: Some("x".into()),
            item: None,
            level: NoteLevel::Unsupported,
            message: Msg::raw("x"),
        });
        let mut p = plan();
        p.existing.insert("a".into(), ExistingAction::Skip);
        let got = keys(problems(&p, &draft));
        assert_eq!(got.len(), 2, "{got:?}");
        assert!(got[1].contains(": b"), "{got:?}");
        p.existing.insert("b".into(), ExistingAction::Recreate);
        draft.notes.clear();
        assert!(problems(&p, &draft).is_empty());
    }

    #[tokio::test]
    async fn failed_tables_skip_their_dependents() {
        let (s, profile, log, _dir) = service();
        let profile = s.profile(&profile).unwrap();
        let task = s.tasks.create(TaskKind::Migrate).unwrap();
        let id = task.status().id;
        let st = |kind, object: &str, requires: &[&str], sql: &str| MigrationStatement {
            phase: 1,
            kind,
            object: object.into(),
            requires: requires.iter().map(|r| r.to_string()).collect(),
            sql: sql.into(),
        };
        let statements = vec![
            st(StatementKind::CreateTable, "a", &[], "FAIL"),
            st(StatementKind::CreateTable, "b", &[], "CREATE TABLE b"),
            st(StatementKind::Comment, "a", &["a"], "COMMENT ON a"),
            st(StatementKind::Index, "b", &["b"], "FAIL"),
            st(StatementKind::ForeignKey, "b", &["b", "a"], "ALTER TABLE b ADD FK"),
            st(StatementKind::Unique, "b", &["b"], "ALTER TABLE b ADD UNIQUE"),
        ];
        let run = Run {
            service: &s,
            task: &task,
            profile: &profile,
            dialect: Dialect::new(Variant::Postgres),
            schema: "t",
        };
        let out = run_statements(&run, &statements, String::new()).await.unwrap();
        task.finish(Ok(()));
        assert_eq!(out.failed_tables, BTreeSet::from(["a".to_string()]));
        assert_eq!(out.skipped, 2);
        let status = wait(&s, &id).await;
        assert_eq!(status.state, TaskState::Done);
        assert_eq!((status.progress.statements_done, status.progress.statements_failed), (6, 2));
        assert_eq!(status.failures.iter().map(|f| f.index).collect::<Vec<_>>(), [0, 3]);
        let log = log.lock().unwrap().clone();
        assert!(
            log.iter().any(|l| l.contains("ADD UNIQUE")) && !log.iter().any(|l| l.contains("COMMENT ON a"))
        );
    }
}
````

- [ ] **Step 10: 修改 `crates/dblens-service/src/task.rs`**

```diff
--- a/crates/dblens-service/src/task.rs
+++ b/crates/dblens-service/src/task.rs
@@ -32,6 +32,8 @@ pub enum TaskKind {
     Sync,
     /// 导出文档（M4c）。
     Document,
+    /// 结构迁移（M5a）。
+    Migrate,
 }
 
 #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
@@ -158,6 +160,11 @@ impl TaskHandle {
         }
     }
 
+    /// 当前状态的副本。
+    pub fn status(&self) -> TaskStatus {
+        self.entry.status.lock().map(|s| s.clone()).unwrap_or_else(|e| e.into_inner().clone())
+    }
+
     pub fn cancelled(&self) -> bool {
         self.entry.cancelled.load(Ordering::SeqCst)
     }
```

- [ ] **Step 11: 检查**

```bash
export CARGO_INCREMENTAL=0
cargo fmt --all
cargo clippy --config "source.mirror.registry='sparse+https://rsproxy.cn/index/'" --workspace --all-targets -- -D warnings
DBLENS_LANG=en cargo test --config "source.mirror.registry='sparse+https://rsproxy.cn/index/'" --workspace
```

实库测试：在上面的 `cargo test` 前以内联环境变量给出原有 5 个库的 `DBLENS_TEST_MYSQL_URL`、`DBLENS_TEST_MARIADB_URL`、`DBLENS_TEST_PG_URL`、`DBLENS_TEST_ORACLE_URL`、`DBLENS_TEST_DM_URL`，以及国产库第一批的 `DBLENS_TEST_TIDB_URL`、`DBLENS_TEST_OCEANBASE_URL`（用户名写成 `admin%40test`）、`DBLENS_TEST_OPENGAUSS_URL`、`DBLENS_TEST_OPENGAUSS_A_URL`（库 `dblens_test_a`）、`DBLENS_TEST_OPENGAUSS_SHA_URL`（账号 `admin_sha`，库 `dblens_test_sha`）、`DBLENS_TEST_KINGBASE_URL`（54321）、`DBLENS_TEST_KINGBASE_PG_URL`（54322）。新目标的密码在测试虚拟机 root 的 `/root/dblens-test/creds.env`，用 `ssh … 'sudo cat …'` 读进当前 shell 的变量，不写进任何文件。只跑迁移的测试：`cargo test … -p dblens-service --test migrate_live -- --test-threads=1`（源和目标的环境变量都设置时才运行该路线）。
- 人大金仓的开发版授权最多 10 个连接：它的测试要串行运行（`--test-threads=1`）。
- 达梦偶发"网络通讯失败"，失败时单独重跑该路线（如 `dameng_to_mysql`）。

Expected: 全部测试通过（service 新增 2 个，api 新增 1 个）；迁移的实库测试 44 个。

- [ ] **Step 12: 提交**

```bash
git add crates/dblens-api/src/lib.rs crates/dblens-api/src/migrations.rs crates/dblens-api/tests/models.rs crates/dblens-core/locales/app.json crates/dblens-service/src/compare.rs crates/dblens-service/src/design.rs crates/dblens-service/src/lib.rs crates/dblens-service/src/migrate.rs crates/dblens-service/src/task.rs crates/dblens-service/tests/migrate_live.rs
git commit -m "feat(migrate): 结构迁移的预览、任务、报告与接口" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 4: 前端：迁移标签页

- `shared/api.ts`：`MigrationEnd`、`NameCase`、`ExistingAction`、`ColumnOverride`、`TableOverride`、`MigrationSettings`、`MigrationPlan`、`NoteLevel`、`MigrationNote`、`MigrationSequence`、`MigrationStatement`、`MigrationPreview`；方法 `previewMigration`、`startMigration`；任务的 `kind` 加 `migrate`。
- `features/migrate/plan.ts`（新，带测试）：新方案、`ready`、`setColumn`/`setTable`/`setCheck`/`setSequence`（去掉空的覆盖项）、`setExisting`、`worst`、`notesOf`、`unchosen`、`summary`、`statementsOf`、`parsePlan`。
- `features/migrate/MigrateView.tsx`（新，懒加载）：两端的选择、名字大小写、注释和检查约束开关、类型规则对话框、打开/另存方案（JSON）；对象列表（勾选迁移、提示级别、目标中已有、只看有问题的、不迁移的对象）；列映射表格（失去焦点时提交的输入框，"按文本迁移"）；检查约束的表达式；目标 DDL 预览；同名表的处理（逐表、全部设为）；不能开始的原因；汇总、另存脚本、开始迁移（需要确认时先确认）、进度、结果和迁移报告。
- `features/editor/tabs.ts`：迁移标签（`migrate` 存整个方案，随标签保存）、`openMigrate`、`setMigrate`；`Workspace.tsx` 显示它；对象树 schema 右键"迁移到…"。
- `app/shell.css`：迁移标签的样式。
- 词条：新文件 `locales/{zh-CN,en}/migrate.json`；`tree.menu.migrate`。

**Files:**
- Modify: `web/src/app/shell.css`
- Modify: `web/src/features/editor/Workspace.tsx`
- Modify: `web/src/features/editor/tabs.ts`
- Create: `web/src/features/migrate/MigrateView.tsx`
- Create: `web/src/features/migrate/plan.test.ts`
- Create: `web/src/features/migrate/plan.ts`
- Modify: `web/src/features/tree/ObjectTree.tsx`
- Create: `web/src/locales/en/migrate.json`
- Modify: `web/src/locales/en/tree.json`
- Modify: `web/src/locales/index.ts`
- Create: `web/src/locales/zh-CN/migrate.json`
- Modify: `web/src/locales/zh-CN/tree.json`
- Modify: `web/src/shared/api.ts`

**Interfaces:**
- Consumes：任务 3 的接口；`withVault`、`SqlBox`、`useUi().refreshTree`、`api.task`、`api.cancelTask`、`api.saveScript`、`api.saveTaskReport`、`api.taskReportUrl`。
- Produces：`useTabs().openMigrate(plan)`、`emptyPlan(source)`。

- [ ] **Step 1: 新建 `web/src/features/migrate/plan.test.ts`**

```ts
import { describe, expect, it } from 'vitest'

import type { MigrationPreview } from '../../shared/api'
import { emptyPlan, notesOf, parsePlan, ready, setCheck, setColumn, setExisting, setSequence, setTable, summary, unchosen, worst } from './plan'

const src = { profile: 'a', schema: 's' }

function preview(): MigrationPreview {
  const table = (name: string) => ({
    name,
    comment: null,
    columns: [],
    primary_key: null,
    uniques: [],
    foreign_keys: [],
    checks: [],
    indexes: [],
    triggers: [],
    partition: null,
  })
  return {
    source_variant: 'postgres',
    target_variant: 'oracle',
    source_tables: [],
    source_sequences: [],
    draft: {
      tables: [
        { source: 'a', target: table('A'), columns: [], exists: true },
        { source: 'b', target: table('B'), columns: [], exists: false },
      ],
      sequences: [],
      notes: [
        { kind: 'table', object: 'a', column: 'x', level: 'lossy', message: 'm1' },
        { kind: 'table', object: 'a', column: 'y', level: 'unsupported', message: 'm2' },
        { kind: 'table', object: 'a', level: 'info', message: 'm3' },
      ],
    },
    others: [],
    statements: [
      { phase: 1, kind: 'create_table', object: 'b', requires: [], sql: 'CREATE TABLE B' },
      { phase: 1, kind: 'comment', object: 'b', requires: ['b'], sql: 'COMMENT' },
      { phase: 2, kind: 'index', object: 'b', requires: ['b'], sql: 'CREATE INDEX' },
      { phase: 2, kind: 'foreign_key', object: 'b', requires: ['b', 'a'], sql: 'ALTER TABLE' },
    ],
    text: '',
    problems: [],
    denied: null,
    confirm: [],
  }
}

describe('迁移方案', () => {
  it('两端都选好 schema 才能读取', () => {
    const p = emptyPlan(src)
    expect(ready(p)).toBe(false)
    expect(ready({ ...p, target: { profile: 'b', schema: 't' } })).toBe(true)
  })

  it('逐列覆盖：合并修改，恢复默认时去掉空的项', () => {
    let p = setColumn(emptyPlan(src), 't', 'c', { type: 'TEXT' })
    p = setColumn(p, 't', 'c', { name: 'C2' })
    expect(p.settings.tables).toEqual({ t: { columns: { c: { type: 'TEXT', name: 'C2' } } } })
    p = setColumn(p, 't', 'c', { type: undefined, name: undefined })
    expect(p.settings.tables).toEqual({})
    // 空串是有效的覆盖（不要默认值）
    p = setColumn(p, 't', 'c', { default: '' })
    expect(p.settings.tables.t.columns?.c).toEqual({ default: '' })
  })

  it('表、检查约束、序列的覆盖', () => {
    let p = setTable(emptyPlan(src), 't', { skip: true })
    expect(p.settings.tables.t).toEqual({ skip: true })
    p = setTable(p, 't', { skip: false })
    expect(p.settings.tables).toEqual({})
    p = setCheck(p, 't', 'ck', '')
    expect(p.settings.tables.t.checks).toEqual({ ck: '' })
    p = setCheck(p, 't', 'ck', undefined)
    expect(p.settings.tables).toEqual({})
    p = setSequence(p, 's1', { name: 'S2' })
    expect(p.settings.sequences).toEqual({ s1: { name: 'S2' } })
  })

  it('提示的汇总与同名表', () => {
    const pv = preview()
    expect(notesOf(pv, 'table', 'a').length).toBe(3)
    expect(notesOf(pv, 'table', 'a', 'x').map((n) => n.message)).toEqual(['m1'])
    expect(notesOf(pv, 'table', 'a', null).map((n) => n.message)).toEqual(['m3'])
    expect(worst(notesOf(pv, 'table', 'a'))).toBe('unsupported')
    expect(worst([])).toBeNull()
    let p = emptyPlan(src)
    expect(unchosen(p, pv)).toEqual(['a'])
    p = setExisting(p, ['a'], 'missing_only')
    expect(unchosen(p, pv)).toEqual([])
    expect(summary(pv)).toEqual({ tables: 1, sequences: 0, constraints: 2 })
  })

  it('打开另存的方案：补默认值，格式不对返回 null', () => {
    const p = parsePlan(JSON.stringify({ source: src, target: { profile: 'b', schema: 't' }, settings: { type_rules: [{ source: 'CLOB', target: 'TEXT' }] } }))
    expect(p?.settings.options).toEqual({ name_case: null, comments: true, checks: true })
    expect(p?.settings.type_rules).toEqual([{ source: 'CLOB', target: 'TEXT' }])
    expect(p?.tables).toBeNull()
    expect(parsePlan('{"source": 1}')).toBeNull()
    expect(parsePlan('not json')).toBeNull()
  })
})
```

- [ ] **Step 2: 修改 `web/src/app/shell.css`**

```diff
--- a/web/src/app/shell.css
+++ b/web/src/app/shell.css
@@ -781,6 +781,41 @@ body {
   background: var(--warn-bg);
 }
 
+/* 结构迁移（M5a） */
+.migrate-objects {
+  width: 280px;
+  flex-shrink: 0;
+  overflow: auto;
+}
+.migrate-object {
+  display: flex;
+  gap: 6px;
+  align-items: center;
+  padding: 2px 4px;
+  border-radius: 3px;
+  cursor: pointer;
+}
+.migrate-object:hover,
+.migrate-object.on {
+  background: var(--sel);
+}
+.migrate-mapping {
+  flex: 3;
+  min-width: 480px;
+  min-width: 0;
+  overflow: auto;
+  display: flex;
+  flex-direction: column;
+  gap: 6px;
+}
+.migrate-ddl {
+  flex: 2;
+  min-width: 0;
+  display: flex;
+  flex-direction: column;
+  gap: 4px;
+}
+
 /* ER 图（M4b） */
 .er-view {
   display: flex;
```

- [ ] **Step 3: 修改 `web/src/features/editor/Workspace.tsx`**

```diff
--- a/web/src/features/editor/Workspace.tsx
+++ b/web/src/features/editor/Workspace.tsx
@@ -3,7 +3,7 @@
 // 标签多时先缩窄（悬停看全名），再放不下就用滚轮横向滚动，不显示滚动条；右端 ▾ 列出全部标签；右键菜单批量关闭。
 
 import { Dropdown, type MenuProps } from 'antd'
-import { IconAffiliate, IconArrowsDiff, IconChevronDown, IconFileCode, IconPlus, IconSchema, IconTable, IconTableOptions, IconX } from '@tabler/icons-react'
+import { IconAffiliate, IconArrowsDiff, IconTransfer, IconChevronDown, IconFileCode, IconPlus, IconSchema, IconTable, IconTableOptions, IconX } from '@tabler/icons-react'
 import { useQuery } from '@tanstack/react-query'
 import { Spin } from 'antd'
 import { Suspense, type WheelEvent, lazy, useEffect, useRef } from 'react'
@@ -25,6 +25,8 @@ const DesignView = lazy(() => import('../design/DesignView'))
 const CompareView = lazy(() => import('../compare/CompareView'))
 // ER 图带 React Flow，打开第一张图时才加载
 const ErView = lazy(() => import('../er/ErView'))
+// 迁移标签的 DDL 预览带 Monaco
+const MigrateView = lazy(() => import('../migrate/MigrateView'))
 
 function TabTitle({ tab, active, envClass, profileName, profile }: {
   tab: QueryTab
@@ -68,6 +70,8 @@ function TabTitle({ tab, active, envClass, profileName, profile }: {
       >
         {tab.er ? (
           <IconAffiliate size={16} />
+        ) : tab.migrate ? (
+          <IconTransfer size={16} />
         ) : tab.compare ? (
           <IconArrowsDiff size={16} />
         ) : tab.design?.model ? (
@@ -169,7 +173,11 @@ export default function Workspace() {
         )}
       </div>
       {tab &&
-        (tab.er ? (
+        (tab.migrate ? (
+          <Suspense fallback={<Spin style={{ margin: 48 }} />}>
+            <MigrateView key={tab.id} tab={tab} profiles={profiles} />
+          </Suspense>
+        ) : tab.er ? (
           <Suspense fallback={<Spin style={{ margin: 48 }} />}>
             <ErView key={tab.id} tab={tab} />
           </Suspense>
```

- [ ] **Step 4: 修改 `web/src/features/editor/tabs.ts`**

```diff
--- a/web/src/features/editor/tabs.ts
+++ b/web/src/features/editor/tabs.ts
@@ -5,10 +5,11 @@
 // 设计模型中的表（M4a）时 design.model 是模型目录，标签不绑定连接（profileId 为空串）。
 // 比对标签（M4a 设计文档第 7 节）：带 compare 的标签，两边是模型或连接的一个 schema；也不绑定连接。
 // ER 图标签（M4b 设计文档 4.1 节）：带 er 的标签，模型的一张图（不绑定连接）或连接的一个 schema（只读）。
+// 迁移标签（M5a 设计文档第 5 节）：带 migrate 的标签，内容是整个迁移方案（一并保存，切换标签不丢）；不绑定连接。
 
 import { create } from 'zustand'
 
-import type { TargetKind } from '../../shared/api'
+import type { MigrationPlan, TargetKind } from '../../shared/api'
 import { t } from '../../shared/i18n'
 import { useSettings } from '../../shared/settings'
 
@@ -58,6 +59,8 @@ export interface QueryTab {
   compare?: CompareTarget
   /** 有值时是 ER 图标签。 */
   er?: ErTarget
+  /** 有值时是迁移标签。 */
+  migrate?: MigrationPlan
 }
 
 const TARGET_KINDS: readonly unknown[] = ['table', 'view', 'materialized_view'] satisfies TargetKind[]
@@ -96,6 +99,11 @@ function isErTarget(v: unknown): v is ErTarget {
   return e.kind === 'model' ? typeof e.path === 'string' && typeof e.diagram === 'string' : e.kind === 'database' && typeof e.profile === 'string'
 }
 
+function isMigratePlan(v: unknown): v is MigrationPlan {
+  const m = v as MigrationPlan | null
+  return typeof m?.source?.profile === 'string' && typeof m.target?.profile === 'string' && typeof m.settings === 'object'
+}
+
 function isCompareTarget(v: unknown): v is CompareTarget {
   const c = v as CompareTarget | null
   return !!c && isCompareSide(c.source) && isCompareSide(c.target)
@@ -133,7 +141,8 @@ export function parseTabs(raw: string | undefined): Persisted {
         (t.data === undefined || isDataTarget(t.data)) &&
         (t.design === undefined || isDesignTarget(t.design)) &&
         (t.compare === undefined || isCompareTarget(t.compare)) &&
-        (t.er === undefined || isErTarget(t.er)),
+        (t.er === undefined || isErTarget(t.er)) &&
+        (t.migrate === undefined || isMigratePlan(t.migrate)),
     )
     const active = tabs.some((t) => t.id === v.active) ? (v.active as string) : (tabs[0]?.id ?? null)
     return { tabs, active, seq: typeof v.seq === 'number' && v.seq > 0 ? v.seq : tabs.length + 1 }
@@ -159,6 +168,10 @@ interface TabsState extends Persisted {
   openEr(target: ErTarget, title: string): string
   /** 图改名后，标签跟着改。 */
   setEr(id: string, target: ErTarget, title: string): void
+  /** 新开迁移标签，返回它的 id。 */
+  openMigrate(plan: MigrationPlan): string
+  /** 修改迁移标签的方案。 */
+  setMigrate(id: string, plan: MigrationPlan): void
   /** 修改数据标签的过滤条件和排序。 */
   setData(id: string, patch: Pick<DataTarget, 'filter' | 'order'>): void
   close(id: string): void
@@ -243,6 +256,13 @@ export const useTabs = create<TabsState>()((set, get) => ({
     set({ tabs: get().tabs.map((x) => (x.id === id && x.er ? { ...x, er: { ...target }, title } : x)) }),
   setCompare: (id, target) =>
     set({ tabs: get().tabs.map((x) => (x.id === id && x.compare ? { ...x, compare: target } : x)) }),
+  openMigrate: (plan) => {
+    const tab: QueryTab = { id: newId(), title: t('migrate.title'), profileId: '', sql: '', migrate: plan }
+    set({ tabs: [...get().tabs, tab], active: tab.id })
+    return tab.id
+  },
+  setMigrate: (id, plan) =>
+    set({ tabs: get().tabs.map((x) => (x.id === id && x.migrate ? { ...x, migrate: plan } : x)) }),
   setData: (id, patch) =>
     set({ tabs: get().tabs.map((x) => (x.id === id && x.data ? { ...x, data: { ...x.data, ...patch } } : x)) }),
   close: (id) => {
```

- [ ] **Step 5: 新建 `web/src/features/migrate/MigrateView.tsx`**

```tsx
// 迁移标签（M5a 设计文档第 5 节）：上方选择源和目标、选项、打开/另存方案；读取结构后左侧对象列表（带提示、只看有问题的），
// 中间选中表的列映射表格（改目标名、类型、默认值，不迁某列，不认识的类型确认按文本迁），右侧这张表的目标 DDL；
// 底部同名表的处理方式、不能开始的原因、汇总、另存脚本、开始迁移，执行中显示进度，完成后可取迁移报告。由工作区懒加载。

import { IconDeviceFloppy, IconFolderOpen, IconPlayerPlay, IconRefresh, IconSettings } from '@tabler/icons-react'
import { keepPreviousData, useQuery } from '@tanstack/react-query'
import { Alert, App, Button, Checkbox, Empty, Input, Modal, Progress, Select, Space, Spin, Table, Tag } from 'antd'
import { useEffect, useMemo, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { useApi } from '../../app/context'
import { useUi } from '../../app/ui'
import {
  type ExistingAction,
  type MigrationEnd,
  type MigrationNote,
  type MigrationPlan,
  type NameCase,
  type NoteLevel,
  type Profile,
  type TaskStatus,
  errorText,
} from '../../shared/api'
import { desktop } from '../../shared/shell'
import { withVault } from '../connections/vault'
import SqlBox from '../design/SqlBox'
import { type QueryTab, useTabs } from '../editor/tabs'
import {
  notesOf,
  parsePlan,
  ready,
  setCheck,
  setColumn,
  setExisting,
  setSequence,
  setTable,
  statementsOf,
  summary,
  unchosen,
  worst,
} from './plan'

const LEVEL_COLOR: Record<NoteLevel, string> = { info: 'default', lossy: 'orange', unsupported: 'red' }
const ACTIONS: ExistingAction[] = ['skip', 'recreate', 'missing_only']

/** 一端的选择：连接，再选 schema。 */
function EndPicker({ label, value, profiles, onChange }: {
  label: string
  value: MigrationEnd
  profiles: Profile[]
  onChange(v: MigrationEnd): void
}) {
  const api = useApi()
  const { t } = useTranslation()
  const { data: schemas = [] } = useQuery({
    queryKey: ['schemas', value.profile],
    queryFn: () => withVault(api, () => api.schemas(value.profile)),
    enabled: !!value.profile,
  })
  return (
    <Space.Compact>
      <span className="compare-side-label">{label}</span>
      <Select
        size="small"
        style={{ width: 180 }}
        placeholder={t('migrate.pickProfile')}
        value={value.profile || undefined}
        onChange={(profile) => onChange({ profile, schema: '' })}
        options={profiles.map((p) => ({ value: p.id, label: p.name }))}
      />
      <Select
        size="small"
        style={{ width: 160 }}
        showSearch
        placeholder="schema"
        disabled={!value.profile}
        value={value.schema || undefined}
        onChange={(schema) => onChange({ ...value, schema })}
        options={schemas.map((s) => ({ value: s.name }))}
      />
    </Space.Compact>
  )
}

/** 全局类型规则的编辑框。 */
function RulesDialog({ rules, onSave, onClose }: {
  rules: MigrationPlan['settings']['type_rules']
  onSave(r: MigrationPlan['settings']['type_rules']): void
  onClose(): void
}) {
  const { t } = useTranslation()
  const [list, setList] = useState(rules.length ? rules : [{ source: '', target: '' }])
  const edit = (i: number, k: 'source' | 'target', v: string) => setList(list.map((r, j) => (j === i ? { ...r, [k]: v } : r)))
  return (
    <Modal
      open
      title={t('migrate.rulesTitle')}
      onCancel={onClose}
      onOk={() => onSave(list.filter((r) => r.source.trim() && r.target.trim()))}
    >
      <div className="muted" style={{ marginBottom: 8 }}>{t('migrate.rulesHint')}</div>
      {list.map((r, i) => (
        <Space.Compact key={i} style={{ display: 'flex', marginBottom: 4 }}>
          <Input size="small" placeholder={t('migrate.ruleSource')} value={r.source} onChange={(e) => edit(i, 'source', e.target.value)} />
          <Input size="small" placeholder={t('migrate.ruleTarget')} value={r.target} onChange={(e) => edit(i, 'target', e.target.value)} />
          <Button size="small" onClick={() => setList(list.filter((_, j) => j !== i))}>×</Button>
        </Space.Compact>
      ))}
      <Button size="small" type="dashed" onClick={() => setList([...list, { source: '', target: '' }])}>
        {t('migrate.addRule')}
      </Button>
    </Modal>
  )
}

/** 失去焦点或回车时才提交的输入框（避免每个字都重新预览）。 */
function LazyInput({ value, placeholder, onCommit }: { value: string | undefined; placeholder: string; onCommit(v: string | undefined): void }) {
  const [text, setText] = useState(value ?? '')
  useEffect(() => setText(value ?? ''), [value])
  const commit = () => {
    const v = text.trim() ? text : undefined
    if (v !== value) onCommit(v)
  }
  return <Input size="small" style={{ minWidth: 120 }} value={text} placeholder={placeholder} onChange={(e) => setText(e.target.value)} onBlur={commit} onPressEnter={commit} />
}

function NoteTags({ notes }: { notes: MigrationNote[] }) {
  const { t } = useTranslation()
  return (
    <>
      {notes.map((n, i) => (
        <div key={i}>
          <Tag color={LEVEL_COLOR[n.level]}>{t(`migrate.level.${n.level}`)}</Tag>
          {n.item ? `[${n.item}] ` : ''}
          {n.message}
        </div>
      ))}
    </>
  )
}

function download(name: string, text: string, type = 'text/plain') {
  const a = document.createElement('a')
  a.href = URL.createObjectURL(new Blob([text], { type }))
  a.download = name
  a.click()
  URL.revokeObjectURL(a.href)
}

export default function MigrateView({ tab, profiles }: { tab: QueryTab; profiles: Profile[] }) {
  const api = useApi()
  const { t } = useTranslation()
  const { message, modal } = App.useApp()
  const plan = tab.migrate!
  const setPlan = (p: MigrationPlan) => useTabs.getState().setMigrate(tab.id, p)
  const [loaded, setLoaded] = useState(false)
  const [focus, setFocus] = useState<{ kind: 'table' | 'sequence'; name: string } | null>(null)
  const [onlyIssues, setOnlyIssues] = useState(false)
  const [rules, setRules] = useState(false)
  const [task, setTask] = useState<TaskStatus | null>(null)
  const [busy, setBusy] = useState(false)
  const file = useRef<HTMLInputElement>(null)

  const { data: p, error, isFetching, refetch } = useQuery({
    queryKey: ['migration-preview', JSON.stringify(plan)],
    queryFn: () => withVault(api, () => api.previewMigration(plan)),
    enabled: loaded && ready(plan),
    placeholderData: keepPreviousData,
    retry: false,
  })

  const setEnds = (source: MigrationEnd, target: MigrationEnd) => {
    setPlan({ ...plan, source, target, tables: null, sequences: null, existing: {} })
    setLoaded(false)
    setFocus(null)
  }

  // 任务进度
  useEffect(() => {
    if (!task || task.state !== 'running') return
    const h = setTimeout(async () => {
      try {
        const st = await api.task(task.id)
        setTask(st)
        if (st.state !== 'running') {
          useUi.getState().refreshTree(plan.target.profile, plan.target.schema)
          void refetch()
        }
      } catch (e) {
        message.error((e as Error).message)
        setTask(null)
      }
    }, 500)
    return () => clearTimeout(h)
  }, [task, api, message, plan.target, refetch])

  const target = profiles.find((x) => x.id === plan.target.profile)
  const sum = p && summary(p)
  const pending = p ? unchosen(plan, p) : []
  const running = task?.state === 'running'

  const start = async () => {
    setBusy(true)
    try {
      const id = await withVault(api, () => api.startMigration(plan))
      setTask({ id, state: 'running', progress: { statements_done: 0, statements_total: 0 }, failures: [] } as unknown as TaskStatus)
    } catch (e) {
      message.error((e as Error).message)
    } finally {
      setBusy(false)
    }
  }
  const confirmStart = () => {
    if (!p?.confirm.length) return void start()
    modal.confirm({
      title: t('migrate.confirmTitle'),
      content: p.confirm.map((c) => <div key={c}>{c}</div>),
      okButtonProps: { danger: true },
      onOk: () => start(),
    })
  }
  const saveScript = async () => {
    if (!p) return
    try {
      if (desktop) {
        const path = await desktop.saveDialog('migration.sql', [{ name: 'SQL', extensions: ['sql'] }])
        if (!path) return
        await api.saveScript(path, p.text)
        message.success(t('migrate.saved'))
      } else download('migration.sql', p.text)
    } catch (e) {
      message.error((e as Error).message)
    }
  }
  const saveReport = async () => {
    if (!task?.report) return
    try {
      if (desktop) {
        const path = await desktop.saveDialog(task.report, [{ name: 'Markdown', extensions: ['md'] }])
        if (!path) return
        await api.saveTaskReport(task.id, path)
        message.success(t('migrate.saved'))
      } else {
        const a = document.createElement('a')
        a.href = api.taskReportUrl(task.id)
        a.download = task.report
        a.click()
      }
    } catch (e) {
      message.error((e as Error).message)
    }
  }
  const openPlan = async (f: File | undefined) => {
    if (!f) return
    const next = parsePlan(await f.text())
    if (!next) return void message.error(t('migrate.badPlan'))
    setPlan(next)
    setLoaded(false)
    setFocus(null)
  }

  // 左侧对象列表
  const objects = useMemo(() => {
    if (!p) return []
    const tables = p.source_tables.map((x) => {
      const m = p.draft.tables.find((d) => d.source === x.name)
      return {
        kind: 'table' as const,
        name: x.name,
        target: m?.target.name,
        exists: !!m?.exists,
        skip: !!plan.settings.tables[x.name]?.skip,
        level: worst(notesOf(p, 'table', x.name)),
      }
    })
    const seqs = p.source_sequences.map((x) => {
      const m = p.draft.sequences.find((d) => d.source === x.name)
      return {
        kind: 'sequence' as const,
        name: x.name,
        target: m?.target.name,
        exists: !!m?.exists,
        skip: !!plan.settings.sequences[x.name]?.skip,
        level: worst(notesOf(p, 'sequence', x.name)),
      }
    })
    return [...tables, ...seqs].filter((o) => !onlyIssues || (o.level && o.level !== 'info'))
  }, [p, plan.settings, onlyIssues])

  const source = focus?.kind === 'table' ? p?.source_tables.find((x) => x.name === focus.name) : undefined
  const mapped = focus ? p?.draft.tables.find((d) => d.source === focus.name) : undefined
  const tov = focus ? plan.settings.tables[focus.name] : undefined
  const rows = (source?.columns ?? []).map((c) => {
    const target = mapped?.columns.find((m) => m.source === c.name)?.target
    return { c, ov: tov?.columns?.[c.name] ?? {}, tc: mapped?.target.columns.find((x) => x.name === target), notes: p ? notesOf(p, 'table', focus!.name, c.name) : [] }
  })

  return (
    <div className="compare-view">
      <div className="row toolbar" style={{ flexWrap: 'wrap', gap: 8 }}>
        <EndPicker label={t('migrate.source')} value={plan.source} profiles={profiles} onChange={(s) => setEnds(s, plan.target)} />
        <span>→</span>
        <EndPicker label={t('migrate.target')} value={plan.target} profiles={profiles} onChange={(x) => setEnds(plan.source, x)} />
        <Button
          size="small"
          type="primary"
          icon={<IconRefresh size={16} />}
          loading={isFetching}
          disabled={!ready(plan)}
          onClick={() => (loaded ? void refetch() : setLoaded(true))}
        >
          {t('migrate.read')}
        </Button>
        <span className="grow" />
        <Button size="small" icon={<IconFolderOpen size={16} />} onClick={() => file.current?.click()}>{t('migrate.openPlan')}</Button>
        <Button size="small" icon={<IconDeviceFloppy size={16} />} onClick={() => download('migration-plan.json', JSON.stringify(plan, null, 2), 'application/json')}>
          {t('migrate.savePlan')}
        </Button>
        <input ref={file} type="file" accept=".json" hidden onChange={(e) => void openPlan(e.target.files?.[0]).finally(() => (e.target.value = ''))} />
      </div>
      <div className="row toolbar" style={{ gap: 12 }}>
        <span>{t('migrate.nameCase.label')}</span>
        <Select
          size="small"
          style={{ width: 120 }}
          value={plan.settings.options.name_case ?? 'auto'}
          onChange={(v: NameCase | 'auto') => setPlan({ ...plan, settings: { ...plan.settings, options: { ...plan.settings.options, name_case: v === 'auto' ? null : v } } })}
          options={(['auto', 'keep', 'lower', 'upper'] as const).map((v) => ({ value: v, label: t(`migrate.nameCase.${v}`) }))}
        />
        {(['comments', 'checks'] as const).map((k) => (
          <Checkbox
            key={k}
            checked={plan.settings.options[k]}
            onChange={(e) => setPlan({ ...plan, settings: { ...plan.settings, options: { ...plan.settings.options, [k]: e.target.checked } } })}
          >
            {t(`migrate.${k}`)}
          </Checkbox>
        ))}
        <Button size="small" icon={<IconSettings size={16} />} onClick={() => setRules(true)}>
          {t('migrate.typeRules')}
          {plan.settings.type_rules.length ? ` (${plan.settings.type_rules.length})` : ''}
        </Button>
      </div>
      {error ? (
        <Alert type="error" showIcon message={(error as Error).message} />
      ) : !p ? (
        isFetching ? <Spin style={{ margin: 48 }} /> : <Empty style={{ margin: 48 }} description={t('migrate.hint')} />
      ) : (
        <>
          <div className="compare-body grow">
            <div className="migrate-objects">
              <Checkbox checked={onlyIssues} onChange={(e) => setOnlyIssues(e.target.checked)}>{t('migrate.onlyIssues')}</Checkbox>
              {objects.map((o) => (
                <div
                  key={`${o.kind}:${o.name}`}
                  className={`migrate-object${focus?.kind === o.kind && focus.name === o.name ? ' on' : ''}`}
                  onClick={() => setFocus({ kind: o.kind, name: o.name })}
                >
                  <Checkbox
                    checked={!o.skip}
                    onClick={(e) => e.stopPropagation()}
                    onChange={(e) =>
                      setPlan(o.kind === 'table' ? setTable(plan, o.name, { skip: !e.target.checked }) : setSequence(plan, o.name, { skip: !e.target.checked }))
                    }
                  />
                  <span className="grow">
                    {o.kind === 'sequence' && <span className="muted">{t('migrate.sequences')} · </span>}
                    {o.name}
                    {o.target && o.target !== o.name && <span className="muted"> → {o.target}</span>}
                  </span>
                  {o.exists && <Tag>{t('migrate.exists')}</Tag>}
                  {o.level && o.level !== 'info' && <Tag color={LEVEL_COLOR[o.level]}>{t(`migrate.level.${o.level}`)}</Tag>}
                </div>
              ))}
              {p.others.length > 0 && (
                <details className="muted">
                  <summary>{t('migrate.others', { count: p.others.length })}</summary>
                  <div>{t('migrate.othersHint')}</div>
                  {p.others.map((o) => (
                    <div key={`${o.kind}:${o.table ?? ''}:${o.name}`}>
                      {o.kind} {o.table ? `${o.table}.` : ''}
                      {o.name}
                    </div>
                  ))}
                </details>
              )}
            </div>
            <div className="migrate-mapping">
              {!focus ? (
                <span className="muted">{t('migrate.pickTable')}</span>
              ) : focus.kind === 'sequence' ? (
                <>
                  <Space>
                    {t('migrate.col.name')}
                    <LazyInput
                      value={plan.settings.sequences[focus.name]?.name}
                      placeholder={p.draft.sequences.find((s) => s.source === focus.name)?.target.name ?? focus.name}
                      onCommit={(name) => setPlan(setSequence(plan, focus.name, { name }))}
                    />
                  </Space>
                  <NoteTags notes={notesOf(p, 'sequence', focus.name)} />
                </>
              ) : (
                <>
                  <Space>
                    {t('migrate.tableName')}
                    <LazyInput value={tov?.name} placeholder={mapped?.target.name ?? focus.name} onCommit={(name) => setPlan(setTable(plan, focus.name, { name }))} />
                  </Space>
                  <NoteTags notes={notesOf(p, 'table', focus.name, null)} />
                  <Table
                    size="small"
                    pagination={false}
                    scroll={{ x: 'max-content' }}
                    rowKey={(r) => r.c.name}
                    dataSource={rows}
                    columns={[
                      {
                        title: t('migrate.col.migrate'),
                        width: 56,
                        render: (_, r) => (
                          <Checkbox checked={!r.ov.skip} onChange={(e) => setPlan(setColumn(plan, focus.name, r.c.name, { skip: !e.target.checked || undefined }))} />
                        ),
                      },
                      { title: t('migrate.col.source'), render: (_, r) => r.c.name },
                      { title: t('migrate.col.sourceType'), render: (_, r) => <code>{r.c.data_type.native}</code> },
                      {
                        title: t('migrate.col.name'),
                        render: (_, r) => (
                          <LazyInput value={r.ov.name} placeholder={r.tc?.name ?? r.c.name} onCommit={(name) => setPlan(setColumn(plan, focus.name, r.c.name, { name }))} />
                        ),
                      },
                      {
                        title: t('migrate.col.type'),
                        render: (_, r) => (
                          <LazyInput value={r.ov.type} placeholder={r.tc?.data_type.native ?? ''} onCommit={(type) => setPlan(setColumn(plan, focus.name, r.c.name, { type }))} />
                        ),
                      },
                      {
                        title: t('migrate.col.default'),
                        render: (_, r) =>
                          r.c.generated ? (
                            <LazyInput
                              value={r.ov.expression}
                              placeholder={`= ${r.tc?.generated ?? ''}`}
                              onCommit={(expression) => setPlan(setColumn(plan, focus.name, r.c.name, { expression }))}
                            />
                          ) : (
                            <LazyInput
                              value={r.ov.default}
                              placeholder={r.tc?.default ?? t('migrate.noDefault')}
                              onCommit={(d) => setPlan(setColumn(plan, focus.name, r.c.name, { default: d }))}
                            />
                          ),
                      },
                      {
                        title: t('migrate.col.notes'),
                        render: (_, r) => (
                          <>
                            <NoteTags notes={r.notes} />
                            {r.notes.some((n) => n.level === 'unsupported') && !r.ov.accept && (
                              <Button size="small" type="link" onClick={() => setPlan(setColumn(plan, focus.name, r.c.name, { accept: true }))}>
                                {t('migrate.acceptText')}
                              </Button>
                            )}
                          </>
                        ),
                      },
                    ]}
                  />
                  {(source?.checks ?? []).filter((c) => c.name).map((c) => (
                    <Space key={c.name} style={{ marginTop: 4 }}>
                      <code>CHECK {c.name}</code>
                      <LazyInput
                        value={tov?.checks?.[c.name!]}
                        placeholder={mapped?.target.checks.find((x) => x.name?.toLowerCase() === c.name!.toLowerCase())?.expression ?? c.expression}
                        onCommit={(e) => setPlan(setCheck(plan, focus.name, c.name!, e))}
                      />
                    </Space>
                  ))}
                </>
              )}
            </div>
            <div className="migrate-ddl">
              <b>{t('migrate.ddl')}</b>
              <SqlBox sql={focus ? statementsOf(p, focus.name) : p.text} driver={target?.driver} height="100%" />
            </div>
          </div>
          {p.draft.tables.some((x) => x.exists) && (
            <div className="row toolbar" style={{ flexWrap: 'wrap', gap: 8 }}>
              <b>{t('migrate.existing.title')}</b>
              <span className="muted">{t('migrate.existing.all')}</span>
              <Select
                size="small"
                style={{ width: 200 }}
                placeholder={t('migrate.existing.pick')}
                value={null}
                onChange={(a: ExistingAction) => setPlan(setExisting(plan, p.draft.tables.filter((x) => x.exists).map((x) => x.source), a))}
                options={ACTIONS.map((a) => ({ value: a, label: t(`migrate.existing.${a}`) }))}
              />
              {p.draft.tables
                .filter((x) => x.exists)
                .map((x) => (
                  <Space.Compact key={x.source}>
                    <span className="compare-side-label">{x.target.name}</span>
                    <Select
                      size="small"
                      style={{ width: 200 }}
                      status={plan.existing[x.source] ? undefined : 'warning'}
                      placeholder={t('migrate.existing.pick')}
                      value={plan.existing[x.source]}
                      onChange={(a: ExistingAction) => setPlan(setExisting(plan, [x.source], a))}
                      options={ACTIONS.map((a) => ({ value: a, label: t(`migrate.existing.${a}`) }))}
                    />
                  </Space.Compact>
                ))}
            </div>
          )}
          {p.denied && <Alert type="error" showIcon message={p.denied} />}
          {p.problems.map((x) => (
            <Alert key={x} type="warning" showIcon message={x} />
          ))}
          {task && !running && (
            <Alert
              type={task.state === 'done' && !task.failures.length ? 'success' : task.state === 'done' || task.state === 'cancelled' ? 'warning' : 'error'}
              showIcon
              message={
                task.state === 'done'
                  ? task.failures.length
                    ? t('migrate.doneWithFailures', { count: task.progress.statements_failed })
                    : t('migrate.done')
                  : t(task.state === 'cancelled' ? 'migrate.cancelled' : 'migrate.failed')
              }
              description={
                <>
                  {task.failures.slice(0, 5).map((f) => (
                    <div key={f.index}>
                      <code className="design-failed-sql">{f.text}</code>
                      <div className="err">{errorText(f.error)}</div>
                    </div>
                  ))}
                  {task.error && <div className="err">{errorText(task.error)}</div>}
                </>
              }
              action={
                task.report ? (
                  <Button size="small" onClick={() => void saveReport()}>
                    {t('migrate.report')}
                  </Button>
                ) : undefined
              }
            />
          )}
          <div className="row design-run">
            {running ? (
              <>
                <span>{t('migrate.running', { done: task.progress.statements_done, total: task.progress.statements_total })}</span>
                <Progress size="small" style={{ width: 240 }} percent={Math.round((100 * task.progress.statements_done) / Math.max(1, task.progress.statements_total))} />
                <Button size="small" onClick={() => void api.cancelTask(task.id).catch((e) => message.error((e as Error).message))}>
                  {t('migrate.cancel')}
                </Button>
              </>
            ) : (
              sum && <span>{t('migrate.summary', sum)}</span>
            )}
            <span className="grow" />
            <Button size="small" icon={<IconDeviceFloppy size={16} />} disabled={!p.statements.length} onClick={() => void saveScript()}>
              {t('migrate.saveScript')}
            </Button>
            <Button
              size="small"
              type="primary"
              danger={!!p.confirm.length}
              icon={<IconPlayerPlay size={16} />}
              loading={busy}
              disabled={running || isFetching || !!p.problems.length || !!p.denied || pending.length > 0 || !p.statements.length}
              onClick={confirmStart}
            >
              {t('migrate.start')}
            </Button>
          </div>
        </>
      )}
      {rules && (
        <RulesDialog
          rules={plan.settings.type_rules}
          onClose={() => setRules(false)}
          onSave={(type_rules) => {
            setPlan({ ...plan, settings: { ...plan.settings, type_rules } })
            setRules(false)
          }}
        />
      )}
    </div>
  )
}
```

- [ ] **Step 6: 新建 `web/src/features/migrate/plan.ts`**

```ts
// 迁移方案的组成与修改（M5a 设计文档 3.3、5.2 节）：新方案、逐列覆盖（去掉空的项）、提示的汇总、
// 打开另存的方案时的校验、执行前的汇总。不依赖界面，单独测试。

import type {
  ColumnOverride,
  ExistingAction,
  MigrationEnd,
  MigrationNote,
  MigrationPlan,
  MigrationPreview,
  NoteLevel,
  TableOverride,
} from '../../shared/api'

export function emptyPlan(source: MigrationEnd): MigrationPlan {
  return {
    source,
    tables: null,
    sequences: null,
    target: { profile: '', schema: '' },
    settings: { options: { name_case: null, comments: true, checks: true }, type_rules: [], tables: {}, sequences: {} },
    existing: {},
  }
}

/** 两端都选好了 schema。 */
export function ready(plan: MigrationPlan): boolean {
  return !!(plan.source.profile && plan.source.schema && plan.target.profile && plan.target.schema)
}

/** 去掉值为 undefined 或 false 的项、空对象。 */
function compact<T extends object>(o: T): T | undefined {
  const entries = Object.entries(o).filter(([, v]) => v !== undefined && v !== false && !(typeof v === 'object' && v && !Object.keys(v).length))
  return entries.length ? (Object.fromEntries(entries) as T) : undefined
}

function withTable(plan: MigrationPlan, table: string, f: (t: TableOverride) => TableOverride): MigrationPlan {
  const tables = { ...plan.settings.tables }
  const next = compact(f({ ...tables[table] }))
  if (next) tables[table] = next
  else delete tables[table]
  return { ...plan, settings: { ...plan.settings, tables } }
}

/** 修改一列的覆盖：patch 中为 undefined 的项表示恢复默认。 */
export function setColumn(plan: MigrationPlan, table: string, column: string, patch: ColumnOverride): MigrationPlan {
  return withTable(plan, table, (t) => {
    const columns = { ...t.columns }
    const merged = compact({ ...columns[column], ...patch })
    if (merged) columns[column] = merged
    else delete columns[column]
    return { ...t, columns }
  })
}

/** 修改表的覆盖（目标名、不迁移）。 */
export function setTable(plan: MigrationPlan, table: string, patch: Pick<TableOverride, 'name' | 'skip'>): MigrationPlan {
  return withTable(plan, table, (t) => ({ ...t, ...patch }))
}

/** 修改检查约束的覆盖：expression 为 undefined 时恢复自动转换，空串表示不迁。 */
export function setCheck(plan: MigrationPlan, table: string, check: string, expression: string | undefined): MigrationPlan {
  return withTable(plan, table, (t) => {
    const checks = { ...t.checks }
    if (expression === undefined) delete checks[check]
    else checks[check] = expression
    return { ...t, checks }
  })
}

export function setSequence(plan: MigrationPlan, name: string, patch: { name?: string; skip?: boolean }): MigrationPlan {
  const sequences = { ...plan.settings.sequences }
  const next = compact({ ...sequences[name], ...patch })
  if (next) sequences[name] = next
  else delete sequences[name]
  return { ...plan, settings: { ...plan.settings, sequences } }
}

/** 给若干同名表设置处理方式。 */
export function setExisting(plan: MigrationPlan, tables: string[], action: ExistingAction): MigrationPlan {
  const existing = { ...plan.existing }
  for (const t of tables) existing[t] = action
  return { ...plan, existing }
}

const RANK: Record<NoteLevel, number> = { info: 0, lossy: 1, unsupported: 2 }

/** 一组提示中最严重的级别；没有提示时为 null。 */
export function worst(notes: MigrationNote[]): NoteLevel | null {
  return notes.reduce<NoteLevel | null>((w, n) => (w === null || RANK[n.level] > RANK[w] ? n.level : w), null)
}

/** 某个对象（某一列）的提示。`column` 为 null 时取对象本身（不属于某一列）的提示。 */
export function notesOf(p: MigrationPreview, kind: 'table' | 'sequence', object: string, column?: string | null): MigrationNote[] {
  return p.draft.notes.filter(
    (n) => n.kind === kind && n.object === object && (column === undefined || (column === null ? !n.column : n.column === column)),
  )
}

/** 目标中已有、还没选处理方式的表。 */
export function unchosen(plan: MigrationPlan, p: MigrationPreview): string[] {
  return p.draft.tables.filter((t) => t.exists && !plan.existing[t.source]).map((t) => t.source)
}

/** 执行前的汇总：建几张表、几个序列、几条索引和约束。 */
export function summary(p: MigrationPreview): { tables: number; sequences: number; constraints: number } {
  const count = (kinds: string[]) => p.statements.filter((s) => kinds.includes(s.kind)).length
  return {
    tables: count(['create_table']),
    sequences: count(['sequence']),
    constraints: count(['index', 'unique', 'check', 'foreign_key']),
  }
}

/** 一个对象的语句（DDL 预览）。 */
export function statementsOf(p: MigrationPreview, object: string): string {
  return p.statements
    .filter((s) => s.object === object)
    .map((s) => `${s.sql};`)
    .join('\n\n')
}

const isEnd = (v: unknown): v is MigrationEnd =>
  typeof (v as MigrationEnd)?.profile === 'string' && typeof (v as MigrationEnd).schema === 'string'
const isList = (v: unknown) => v === null || (Array.isArray(v) && v.every((x) => typeof x === 'string'))

/** 解析另存的方案；格式不对时返回 null。缺的设置项补默认值。 */
export function parsePlan(text: string): MigrationPlan | null {
  try {
    const v = JSON.parse(text) as Partial<MigrationPlan>
    if (!isEnd(v.source) || !isEnd(v.target) || !isList(v.tables ?? null) || !isList(v.sequences ?? null)) return null
    const base = emptyPlan(v.source)
    const s = (v.settings ?? {}) as Partial<MigrationPlan['settings']>
    return {
      ...base,
      tables: v.tables ?? null,
      sequences: v.sequences ?? null,
      target: v.target,
      settings: {
        options: { ...base.settings.options, ...s.options },
        type_rules: Array.isArray(s.type_rules) ? s.type_rules : [],
        tables: s.tables ?? {},
        sequences: s.sequences ?? {},
      },
      existing: v.existing ?? {},
    }
  } catch {
    return null
  }
}
```

- [ ] **Step 7: 修改 `web/src/features/tree/ObjectTree.tsx`**

```diff
--- a/web/src/features/tree/ObjectTree.tsx
+++ b/web/src/features/tree/ObjectTree.tsx
@@ -29,6 +29,7 @@ import { useTabs } from '../editor/tabs'
 import { useExportDialog } from '../export/dialog'
 import { useImportDialogs } from '../import/dialog'
 import { useDocExport } from '../document/dialog'
+import { emptyPlan } from '../migrate/plan'
 import { useModelDialogs } from '../model/dialogs'
 import type { Api, Environment, Profile, RoutineInfo } from '../../shared/api'
 import DbIcon from '../../shared/DbIcon'
@@ -306,6 +307,7 @@ export default function ObjectTree() {
       { key: 'compare-model', label: t('tree.menu.compareModel') },
       { key: 'save-model', label: t('tree.menu.saveAsModel') },
       { key: 'export-doc', label: t('tree.menu.exportDoc') },
+      { key: 'migrate', label: t('tree.menu.migrate') },
       { type: 'divider' },
     ]
   }
@@ -358,6 +360,7 @@ export default function ObjectTree() {
         useTabs.getState().openCompare({ source: null, target: { kind: 'database', profile: k.profileId, schema: k.schema } })
       } else if (key === 'save-model' && k.type === 's') useModelDialogs.getState().openReverse({ profileId: k.profileId, schema: k.schema })
       else if (key === 'export-doc' && k.type === 's') useDocExport.getState().open({ kind: 'database', profile: k.profileId, schema: k.schema })
+      else if (key === 'migrate' && k.type === 's') useTabs.getState().openMigrate(emptyPlan({ profile: k.profileId, schema: k.schema }))
       else if (key === 'import' && k.type === 'o') {
         useImportDialogs.getState().openImport({ profileId: k.profileId, schema: k.schema, table: k.name, create: false })
       } else if (key === 'import-new' && k.type === 's') {
```

- [ ] **Step 8: 新建 `web/src/locales/en/migrate.json`**

```json
{
  "title": "Migration",
  "source": "Source",
  "target": "Target",
  "pickProfile": "Connection",
  "read": "Read Structure",
  "nameCase": {
    "label": "Name case",
    "auto": "Target default",
    "keep": "Keep",
    "lower": "Lower",
    "upper": "Upper"
  },
  "comments": "Migrate comments",
  "checks": "Migrate check constraints",
  "openPlan": "Open Plan…",
  "savePlan": "Save Plan…",
  "badPlan": "Not a migration plan file",
  "typeRules": "Type Rules…",
  "rulesTitle": "Global type rules",
  "rulesHint": "Matched by source type name (optionally with parameters, e.g. NUMBER(1)); applied before the default mapping",
  "ruleSource": "Source type",
  "ruleTarget": "Target type",
  "addRule": "Add rule",
  "hint": "Pick the source and target schemas, then read the structure",
  "objects": "Objects",
  "onlyIssues": "Only with issues",
  "sequences": "Sequences",
  "others": "{{count}} objects not migrated",
  "othersHint": "Views, routines, triggers and synonyms are not converted; the report includes their source DDL",
  "exists": "Exists in target",
  "pickTable": "Pick a table on the left to see its column mapping",
  "col": {
    "migrate": "Migrate",
    "source": "Source column",
    "sourceType": "Source type",
    "name": "Target name",
    "type": "Target type",
    "default": "Default",
    "notes": "Notes"
  },
  "tableName": "Target table name",
  "skipTable": "Do not migrate this table",
  "acceptText": "Migrate as text",
  "noDefault": "(none)",
  "ddl": "Target DDL",
  "existing": {
    "title": "Tables that already exist in the target",
    "pick": "Choose an action",
    "all": "Set all to",
    "skip": "Skip",
    "recreate": "Drop and recreate",
    "missing_only": "Only add missing indexes and constraints"
  },
  "summary": "{{tables}} tables, {{sequences}} sequences, {{constraints}} indexes and constraints",
  "saveScript": "Save Script…",
  "start": "Start Migration",
  "confirmTitle": "Confirm migration",
  "saved": "Saved",
  "running": "Migrating {{done}}/{{total}}",
  "cancel": "Cancel",
  "done": "Migration finished",
  "doneWithFailures": "Migration finished; {{count}} statements failed",
  "failed": "Migration failed",
  "cancelled": "Migration cancelled",
  "report": "Migration Report",
  "level": {
    "info": "Info",
    "lossy": "Lossy",
    "unsupported": "Unsupported"
  }
}
```

- [ ] **Step 9: 修改 `web/src/locales/en/tree.json`**

```diff
--- a/web/src/locales/en/tree.json
+++ b/web/src/locales/en/tree.json
@@ -45,7 +45,8 @@
     "sqlFile": "Run SQL File…",
     "exportData": "Export Data…",
     "newTable": "New table",
-    "exportDoc": "Export Document…"
+    "exportDoc": "Export Document…",
+    "migrate": "Migrate To…"
   },
   "duplicate": {
     "name": "{{name}} copy",
```

- [ ] **Step 10: 修改 `web/src/locales/index.ts`**

```diff
--- a/web/src/locales/index.ts
+++ b/web/src/locales/index.ts
@@ -6,6 +6,7 @@ import enCommon from './en/common.json'
 import enConnections from './en/connections.json'
 import enDesign from './en/design.json'
 import enDocument from './en/document.json'
+import enMigrate from './en/migrate.json'
 import enEditor from './en/editor.json'
 import enImport from './en/import.json'
 import enModel from './en/model.json'
@@ -17,6 +18,7 @@ import zhCommon from './zh-CN/common.json'
 import zhConnections from './zh-CN/connections.json'
 import zhDesign from './zh-CN/design.json'
 import zhDocument from './zh-CN/document.json'
+import zhMigrate from './zh-CN/migrate.json'
 import zhEditor from './zh-CN/editor.json'
 import zhImport from './zh-CN/import.json'
 import zhModel from './zh-CN/model.json'
@@ -36,6 +38,7 @@ export const zhCN = {
   model: zhModel,
   er: zhEr,
   document: zhDocument,
+  migrate: zhMigrate,
 }
 
 export const en = {
@@ -50,4 +53,5 @@ export const en = {
   model: enModel,
   er: enEr,
   document: enDocument,
+  migrate: enMigrate,
 }
```

- [ ] **Step 11: 新建 `web/src/locales/zh-CN/migrate.json`**

```json
{
  "title": "迁移",
  "source": "源",
  "target": "目标",
  "pickProfile": "选择连接",
  "read": "读取结构",
  "nameCase": {
    "label": "名字大小写",
    "auto": "按目标库",
    "keep": "保持",
    "lower": "小写",
    "upper": "大写"
  },
  "comments": "迁移注释",
  "checks": "迁移检查约束",
  "openPlan": "打开方案…",
  "savePlan": "另存方案…",
  "badPlan": "不是迁移方案文件",
  "typeRules": "类型规则…",
  "rulesTitle": "全局类型规则",
  "rulesHint": "按源类型名匹配（可带参数，如 NUMBER(1)），优先于默认映射",
  "ruleSource": "源类型",
  "ruleTarget": "目标类型",
  "addRule": "添加规则",
  "hint": "选择源和目标的 schema 后读取结构",
  "objects": "对象",
  "onlyIssues": "只看有问题的",
  "sequences": "序列",
  "others": "不迁移的对象 {{count}} 个",
  "othersHint": "视图、例程、触发器、同义词不转换，迁移报告中附源库 DDL",
  "exists": "目标中已有",
  "pickTable": "选择左侧的表查看列映射",
  "col": {
    "migrate": "迁移",
    "source": "源列",
    "sourceType": "源类型",
    "name": "目标名",
    "type": "目标类型",
    "default": "默认值",
    "notes": "提示"
  },
  "tableName": "目标表名",
  "skipTable": "不迁移这张表",
  "acceptText": "按文本迁移",
  "noDefault": "（无）",
  "ddl": "目标 DDL",
  "existing": {
    "title": "目标中已有同名表",
    "pick": "请选择处理方式",
    "all": "全部设为",
    "skip": "跳过",
    "recreate": "删除重建",
    "missing_only": "只建缺的索引和约束"
  },
  "summary": "建 {{tables}} 张表、{{sequences}} 个序列、{{constraints}} 个索引和约束",
  "saveScript": "另存脚本…",
  "start": "开始迁移",
  "confirmTitle": "确认迁移",
  "saved": "已另存",
  "running": "正在迁移 {{done}}/{{total}}",
  "cancel": "取消",
  "done": "迁移完成",
  "doneWithFailures": "迁移完成，{{count}} 条语句失败",
  "failed": "迁移失败",
  "cancelled": "迁移已取消",
  "report": "迁移报告",
  "level": {
    "info": "说明",
    "lossy": "有损",
    "unsupported": "不支持"
  }
}
```

- [ ] **Step 12: 修改 `web/src/locales/zh-CN/tree.json`**

```diff
--- a/web/src/locales/zh-CN/tree.json
+++ b/web/src/locales/zh-CN/tree.json
@@ -45,7 +45,8 @@
     "sqlFile": "执行 SQL 文件…",
     "exportData": "导出数据…",
     "newTable": "新建表",
-    "exportDoc": "导出文档…"
+    "exportDoc": "导出文档…",
+    "migrate": "迁移到…"
   },
   "duplicate": {
     "name": "{{name}} 副本",
```

- [ ] **Step 13: 修改 `web/src/shared/api.ts`**

```diff
--- a/web/src/shared/api.ts
+++ b/web/src/shared/api.ts
@@ -423,9 +423,102 @@ export interface SqlFileRequest {
   transaction: boolean
 }
 
+/** 结构迁移（M5a）：迁移的一端。 */
+export interface MigrationEnd {
+  profile: string
+  schema: string
+}
+
+export type NameCase = 'keep' | 'lower' | 'upper'
+export type ExistingAction = 'skip' | 'recreate' | 'missing_only'
+
+/** 一列的覆盖：`default` 为空串表示不要默认值；`accept` 确认把不认识的类型按文本迁移。 */
+export interface ColumnOverride {
+  name?: string
+  type?: string
+  default?: string
+  expression?: string
+  skip?: boolean
+  accept?: boolean
+}
+
+export interface TableOverride {
+  name?: string
+  skip?: boolean
+  columns?: Record<string, ColumnOverride>
+  /** 源检查约束名 → 目标表达式，空串表示不迁。 */
+  checks?: Record<string, string>
+}
+
+export interface MigrationSettings {
+  options: { name_case: NameCase | null; comments: boolean; checks: boolean }
+  type_rules: { source: string; target: string }[]
+  tables: Record<string, TableOverride>
+  sequences: Record<string, { name?: string; skip?: boolean }>
+}
+
+/** 迁移方案：可另存为 JSON 再打开。`tables`、`sequences` 为 null 时迁移全部。 */
+export interface MigrationPlan {
+  source: MigrationEnd
+  tables: string[] | null
+  sequences: string[] | null
+  target: MigrationEnd
+  settings: MigrationSettings
+  existing: Record<string, ExistingAction>
+}
+
+export type NoteLevel = 'info' | 'lossy' | 'unsupported'
+
+export interface MigrationNote {
+  kind: 'table' | 'sequence'
+  object: string
+  column?: string
+  item?: string
+  level: NoteLevel
+  message: string
+}
+
+export interface MigrationSequence {
+  name: string
+  start: string | null
+  increment: string | null
+  min_value: string | null
+  max_value: string | null
+  cycle: boolean
+  cache: number | null
+}
+
+export interface MigrationStatement {
+  phase: 1 | 2
+  kind: 'drop_table' | 'sequence' | 'create_table' | 'comment' | 'index' | 'unique' | 'check' | 'foreign_key'
+  object: string
+  requires: string[]
+  sql: string
+}
+
+export interface MigrationPreview {
+  source_variant: string
+  target_variant: string
+  source_tables: DesignTableModel[]
+  source_sequences: MigrationSequence[]
+  draft: {
+    tables: { source: string; target: DesignTableModel; columns: { source: string; target: string | null }[]; exists: boolean }[]
+    sequences: { source: string; target: MigrationSequence; exists: boolean }[]
+    notes: MigrationNote[]
+  }
+  /** 不迁移的对象（视图、例程、触发器、同义词）。 */
+  others: { kind: string; name: string; table?: string }[]
+  statements: MigrationStatement[]
+  text: string
+  /** 不能开始的原因。 */
+  problems: string[]
+  denied: string | null
+  confirm: string[]
+}
+
 export interface TaskStatus {
   id: string
-  kind: 'import' | 'sql_file' | 'design' | 'reverse' | 'sync' | 'document'
+  kind: 'import' | 'sql_file' | 'design' | 'reverse' | 'sync' | 'document' | 'migrate'
   state: 'running' | 'confirm' | 'done' | 'failed' | 'cancelled'
   phase: 'preparing' | 'scanning' | 'creating' | 'clearing' | 'writing' | 'executing' | 'finished'
   progress: {
@@ -1025,6 +1118,10 @@ export function createApi(token: string, fetchFn: FetchFn = (i, o) => fetch(i, o
     /** 整个 schema 的表明细（库的 ER 图）。 */
     schemaTables: (id: string, schema: string) => request<DesignTableModel[]>('GET', `${s(id, schema)}/tables`),
     compare: (req: CompareRequest) => request<CompareResult>('POST', '/compare', req),
+    /** 结构迁移：读结构、转换、检查、生成脚本，不执行。 */
+    previewMigration: (plan: MigrationPlan) => request<MigrationPreview>('POST', '/migrations/preview', plan),
+    /** 开始迁移，返回任务 id。 */
+    startMigration: async (plan: MigrationPlan) => (await request<{ id: string }>('POST', '/migrations', plan)).id,
     syncPreview: (req: SyncRequest) => request<SyncPreview>('POST', '/compare/preview', req),
     /** 同步：目标是库时返回任务 id；目标是模型时已经写完，返回 null。 */
     sync: async (req: SyncRequest) => (await request<{ task: string | null }>('POST', '/compare/sync', req)).task,
```

- [ ] **Step 14: 检查**

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

- [ ] **Step 15: 浏览器实测**

浏览器实测（设计文档第 5 节）：用 `dblens-server --listen 127.0.0.1:<端口>` 启动浏览器模式。先在一个 MySQL 系的测试库建两张表（含 ENUM 列、自增主键、注释、唯一键、索引、外键）。
1. 对象树中该 schema 右键"迁移到…"：打开迁移标签，源已填好；选目标连接和 schema，"读取结构"。
2. 对象列表中有 ENUM 列的表标"不支持"，底部提示还有不支持的映射，"开始迁移"不可用；点该列的"按文本迁移"后变为"有损"，可以开始。
3. 改一列的目标名或类型：右侧 DDL 预览随之更新。
4. 开始迁移：显示进度，完成后提示"迁移完成"，可取迁移报告；重新读取后这些表标"目标中已有"，底部要求选择处理方式。
5. "全部设为 → 删除重建"：DDL 预览先 DROP 再 CREATE。
6. 刷新页面：迁移标签和方案还在。最后清理两边的测试表。

- [ ] **Step 16: 提交**

```bash
git add web/src/app/shell.css web/src/features/editor/Workspace.tsx web/src/features/editor/tabs.ts web/src/features/migrate/MigrateView.tsx web/src/features/migrate/plan.test.ts web/src/features/migrate/plan.ts web/src/features/tree/ObjectTree.tsx web/src/locales/en/migrate.json web/src/locales/en/tree.json web/src/locales/index.ts web/src/locales/zh-CN/migrate.json web/src/locales/zh-CN/tree.json web/src/shared/api.ts
git commit -m "feat(web): 迁移标签页" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 5: 文档

- `docs/notes/m5a-structure-migration.md`（新）：结构、与设计文档不一致的地方、验证。
- `docs/notes/m0-4b-api.md`：结构迁移的接口。
- 总设计文档：M5 的拆分，M5a 完成。

**Files:**
- Modify: `docs/notes/m0-4b-api.md`
- Create: `docs/notes/m5a-structure-migration.md`
- Modify: `docs/superpowers/specs/2026-10-22-dblens-design.md`

- [ ] **Step 1: 修改 `docs/notes/m0-4b-api.md`**

```diff
--- a/docs/notes/m0-4b-api.md
+++ b/docs/notes/m0-4b-api.md
@@ -281,6 +281,22 @@
   - `diagrams`：设计文档的 ER 图 `[{upload, schema, name, title, description?, names_only}]`，`upload` 为 `/uploads` 返回的编号（PNG）。
 - 任务 `kind` 为 `document`；xlsx 有截断的单元格时 `warnings` 为个数。markdown 带图片时：写到本机路径则图片在旁边的 `<文件名>_images/`，浏览器模式打包成 zip。
 
+### 结构迁移（M5a 加入）
+
+| 方法 | 路径 | 说明 |
+|---|---|---|
+| POST | `/migrations/preview` | 预览：读两端结构（经元数据缓存）、转换、检查、生成脚本，不执行 |
+| POST | `/migrations` | 开始迁移（以库中最新的结构重新转换，检查不通过则 400），201 和 `{id}`；完成后迁移报告（markdown）从 `/tasks/{id}/report` 取 |
+
+- 请求是迁移方案：`{source: {profile, schema}, tables?, sequences?, target: {profile, schema}, settings, existing}`。
+  - `tables`、`sequences`：迁移范围，null 为全部。
+  - `settings`：`{options: {name_case: "keep" | "lower" | "upper" | null, comments, checks}, type_rules: [{source, target}], tables: {源表名: 表的覆盖}, sequences: {源序列名: {name?, skip?}}}`；
+    表的覆盖 `{name?, skip?, columns: {源列名: {name?, type?, default?, expression?, skip?, accept?}}, checks: {源约束名: 表达式}}`。
+  - `existing`：目标中已有同名表时的处理（按源表名）：`skip` / `recreate` / `missing_only`。
+- 预览返回 `{source_variant, target_variant, source_tables, source_sequences, draft: {tables, sequences, notes}, others, statements, text, problems, denied, confirm}`；
+  `notes` 的 `level` 为 `info` / `lossy` / `unsupported`，有 `unsupported` 或同名表没有选择处理方式时 `problems` 不为空，不能开始。
+- 任务 `kind` 为 `migrate`：逐条执行，失败的语句记入 `failures` 后继续；建表失败的表，依赖它的语句跳过。
+
 ## WebSocket：`GET /api/sessions/{id}/ws?token=…&lang=…`
 
 `lang` 可选，决定事件中文字的语言（见"语言"一节）。
```

- [ ] **Step 2: 新建 `docs/notes/m5a-structure-migration.md`**

````markdown
# M5a 结构迁移

依据：设计文档 `docs/superpowers/specs/2026-10-28-m5a-structure-migration-design.md`。

## 结构

```text
crates/dblens-dialect/src/migrate/
  mod.rs     迁移设置（选项、类型规则、覆盖）、提示、MigrationDraft；migrate()：表、列、主键、唯一约束、索引、外键、检查约束、序列的转换
  names.rs   名字的大小写（按目标库的习惯）、长度上限、重名检查
  types.rs   列类型：覆盖 → 规则 → 同家族原样 → 经逻辑类型；有损判断；Oracle 的 NUMBER(p) 用整数；自增列用整数；外键动作；序列的取值范围
  expr.rs    简单表达式（检查约束、生成列、索引表达式）与默认值的转换：去掉 PG 的类型转换和 MySQL 的字符集前缀，还原 ANY (ARRAY[…])
  script.rs  两步脚本：第一步建序列、（删表）、建表和注释；第二步索引、唯一约束、检查约束，最后外键；同名表的处理；脚本全文
crates/dblens-service/src/migrate.rs   MigrationPlan、读两端结构、不迁移的对象、预览、开始前检查、任务（TaskKind::Migrate）、逐条执行、迁移报告
crates/dblens-api/src/migrations.rs    POST /migrations/preview、/migrations
crates/dblens-service/tests/migrate_live.rs   44 条迁移路线的实库测试
web/src/features/migrate/
  plan.ts          新方案、逐列/表/检查约束/序列的覆盖、同名表的处理、提示的汇总、执行前的汇总、打开另存的方案
  MigrateView.tsx  迁移标签页
```

入口：对象树 schema 右键"迁移到…"，打开迁移标签页（不绑定连接），源预填为该 schema。

## 与设计文档不一致的地方

- 接口只有两个：`/migrations/preview`（转换结果、脚本全文、开始前检查的问题、执行策略的结论）和 `/migrations`（开始）；
  "只生成脚本"用预览中的脚本全文另存，没有单独的检查和脚本接口。
- 方案多了 `sequences`（迁移的序列，None 为全部），与 `tables` 一样限定范围。
- 目标中已有同名序列时自动跳过（提示"已有同名序列"），不要求选择处理方式。
- 名字大小写的默认值：PG 系（含金仓、openGauss 兼容 Oracle 的模式）一律小写——这些库不加引号的名字都折叠为小写。
- 同家族的不同产品（如 openGauss → PG、TiDB → MySQL）：类型原样；默认值先尝试转换（openGauss 的 `pg_systimestamp()` 等），转换不了再原样。
- 系统生成的约束名（MySQL 的主键 `PRIMARY`、Oracle 的 `SYS_C…`、达梦的 `CONS…`）不带过去，由目标库生成；
  MySQL 系为外键自动建的同名索引不单独迁（在 PG、Oracle、达梦中索引和约束同名会冲突）。
- Oracle 的 `NUMBER(p)`（小数位 0、p ≤ 19）迁到其他库时用整数类型（p ≤ 4 用 16 位、≤ 9 用 32 位、否则 64 位），
  自增列在非 Oracle 目标中一律用整数：标识列才能建，外键两边类型才一致。
- 读结构的修正：TiDB、OceanBase 的 `information_schema` 中 `CURRENT_TIMESTAMP` 默认值没有 `DEFAULT_GENERATED` 标记，
  原来被当成字符串加了引号；`mysql.sql`、`tidb.sql` 改为时间列的 `CURRENT_TIMESTAMP…` 按表达式读出。
- 方案存在迁移标签中（随标签保存，切换标签不丢）；另存方案为下载 JSON 文件，打开方案用文件选择框读入。
- 检查约束的覆盖只对有名字的约束；生成列的表达式在"默认值"一列中编辑（以 `=` 提示）。

## 验证

- 后端：fmt、clippy 通过；全部测试在 `DBLENS_LANG=en` 下通过（761 个）。
  - 方言层：PG 源迁到 MySQL、Oracle、达梦、openGauss 的快照，Oracle 源迁到 PG、MySQL 的快照（含类型规则）；
    默认值、简单表达式、nextval 的转换；覆盖与跳过；重名与超长；同名表的三种处理；外键依赖两张表。
  - 服务层：开始前检查；建表失败后跳过依赖它的语句、其余照常执行。
  - 接口：方案能解析，连接不存在时 404。
- 实库：44 条路线（每个源库迁到 MySQL、PG、Oracle、达梦，另加 Oracle → 金仓、openGauss A，MySQL → TiDB、OceanBase），
  迁完读回目标结构与草稿比对（列名、逻辑类型、可空、有无默认值、自增、注释、主键、唯一约束、索引、外键、检查约束个数）；
  串行运行（`--test-threads=1`）约 3.5 分钟。
- 前端：tsc、Vitest（方案的修改、提示汇总、同名表、打开另存的方案）、打包通过。
- 浏览器：TiDB → openGauss 迁移（ENUM 列"按文本迁移"、执行、完成后重新读取显示"目标中已有"、全部设为删除重建后 DDL 预览带 DROP）；
  刷新页面后迁移标签和方案恢复。
- 未验证：桌面程序的"另存脚本""迁移报告"保存对话框。
````

- [ ] **Step 3: 修改 `docs/superpowers/specs/2026-10-22-dblens-design.md`**

```diff
--- a/docs/superpowers/specs/2026-10-22-dblens-design.md
+++ b/docs/superpowers/specs/2026-10-22-dblens-design.md
@@ -334,6 +334,8 @@ trait Policy: Send + Sync {
 - M4a 已完成（设计 `docs/superpowers/specs/2026-10-27-m4a-model-compare-design.md`，详细计划 `docs/superpowers/plans/2026-10-27-m4a-model-compare.md`，说明 `docs/notes/m4a-model-compare.md`）。M4b、M4c 尚未开始。
 - M4b 已完成（设计 `docs/superpowers/specs/2026-10-28-m4b-er-diagram-design.md`，详细计划 `docs/superpowers/plans/2026-10-28-m4b-er-diagram.md`，说明 `docs/notes/m4b-er-diagram.md`）。
 - M4c 已完成（设计 `docs/superpowers/specs/2026-10-28-m4c-document-export-design.md`，详细计划 `docs/superpowers/plans/2026-10-28-m4c-document-export.md`，说明 `docs/notes/m4c-document-export.md`）。M4 全部完成。
+- 2026-10-28 M5 拆成两份（用户确认）：M5a 结构迁移；M5b 数据迁移（分批迁数据、断点续传、行数和抽样校验，插在 M5a 的建表和建约束之间）。
+- M5a 已完成（设计 `docs/superpowers/specs/2026-10-28-m5a-structure-migration-design.md`，详细计划 `docs/superpowers/plans/2026-10-28-m5a-structure-migration.md`，说明 `docs/notes/m5a-structure-migration.md`）。M5b 尚未开始。
 
 ## 12. 风险与待验证项
 
```

- [ ] **Step 4: 检查**

检查文档中的路径和接口与代码一致；按执行时的实际结果更新"验证"一节（测试数、实库与浏览器实测的内容）。

Expected: 无。

- [ ] **Step 5: 提交**

```bash
git add docs/notes/m0-4b-api.md docs/notes/m5a-structure-migration.md docs/superpowers/specs/2026-10-22-dblens-design.md
git commit -m "docs: M5a 结构迁移的说明、接口和进度" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```
