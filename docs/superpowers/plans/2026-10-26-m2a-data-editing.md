# M2a 表数据编辑 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在数据标签中编辑表的数据（改单元格、新增、复制、删除行、从 Excel 粘贴），改动暂存，预览后在一个事务里提交，能发现并发修改。

**Architecture:** 前端把暂存的改动转成改动集交给后端；`dblens-dialect` 按方言生成带占位符的 DML（同时给出字面量形式的预览）；会话在空闲时开事务逐条执行，检查影响行数，失败整体回滚并返回失败的改动序号。前端用 Glide Data Grid 的编辑和粘贴事件，编辑状态是纯函数，单独测试。

**Tech Stack:** Rust（axum、tokio）；React 19 + TypeScript、antd 6、Glide Data Grid、zustand、@tanstack/react-query 5、Vitest。

**Spec:** `docs/superpowers/specs/2026-10-26-m2a-data-editing-design.md`

## Global Constraints

- 只有带行标识（主键，或所有列都 NOT NULL 的唯一键）的表能编辑；视图只读；二进制、大字段（超过预览长度的值）、生成列只读。
- UPDATE 的 WHERE：行标识加改动列的原值；不能可靠比较相等的列不参与；删除只按行标识。
- 执行在数据标签自己的会话里，一个事务，每条语句必须恰好影响 1 行，失败整体回滚；每条语句经过执行策略，预览即确认，生产连接不额外确认。
- 依赖只用正式发布版，跟随最新；新版发布满 7 天再升级；本里程碑不引入新依赖。
- 前端给用户看的文字都用词条（中英文），英文区分单复数；后端错误文字用 `msg!` 词条；日志不翻译。
- 代码注释用中文；文件 UTF-8、LF 换行；前端代码风格：无分号、单引号、两格缩进。
- cargo 命令加 `--config "source.mirror.registry='sparse+https://rsproxy.cn/index/'"`（clippy 写在 `clippy` 之后），并设 `CARGO_INCREMENTAL=0`；后端测试在 `DBLENS_LANG=en` 下运行。
- 前端工具用 PowerShell 执行（`npm`/`npx` 在本机不可用，用 `pnpm exec`）。
- 测试数据库的密码只在命令行中以内联环境变量传入，不写进仓库、报告或提交信息。

## 与设计文档的差异（写计划时实测后调整）

- PG 系的精确小数参数写成 `$n::text::numeric`：PG 驱动不能直接绑定 NUMERIC 参数。
- 执行接口失败时也返回 200，结果中 `failed` 带失败的改动序号和错误对象，便于前端标出失败的行。
- 会话正在读取结果时由前端先停下（`Runner.stop()`），后端只在会话空闲时执行改动集。

## 执行说明

任务 2–7 的代码都已在临时副本中逐任务实现并通过检查（含 5 个实库；浏览器实测覆盖了编辑、新增、预览、提交），执行时按顺序重放。新文件给出全文，修改的文件给出 diff（基于上一个任务完成后的状态，可直接 `git apply`）。`Cargo.lock`、`web/pnpm-lock.yaml` 本里程碑不变（任务 1 有升级时除外）。

## 文件结构

```text
crates/dblens-dialect/src/changes.rs; src/lib.rs              改动集与 DML 生成（新增）
crates/dblens-core/locales/app.json                          后端词条
crates/dblens-service/src/{session.rs, changes.rs, fake.rs, lib.rs}; tests/live.rs   事务执行、Service 入口、实库测试
crates/dblens-api/src/{routes.rs, lib.rs}; tests/api.rs       接口
docs/notes/m0-4b-api.md                                       接口说明
web/src/shared/api.ts                                         接口客户端与类型
web/src/features/data/{parse.ts, edit.ts, editable.ts, editStore.ts}   纯逻辑与编辑状态（新增）
web/src/features/data/{EditableGrid.tsx, ChangePreview.tsx, DataView.tsx}   编辑界面
web/src/features/{results,editor}/                            renderResult、表格主题导出、Runner.stop、关闭前确认
web/src/locales/{zh-CN,en}/editor.json                        词条
docs/notes/m2a-data-editing.md                                说明文档（新增）
```

---

### Task 1: 依赖复查

按依赖原则（M1b2 设计文档 3.1 节）复查前端和后端的全部直接依赖。本里程碑不引入新依赖。

**Files:** 视复查结果而定（`web/package.json`、`Cargo.toml` 及锁文件）。

- [ ] **Step 1: 运行复查脚本**

本机经代理上网，Node 的 `fetch` 默认不读 `HTTPS_PROXY`，必须先设 `NODE_USE_ENV_PROXY=1`，否则全部查询失败，最后仍显示"0 个可升级"。

```powershell
$env:PATH = "D:\Environments\nvm4w\nodejs;$env:PATH"; $env:NPM_REGISTRY = 'https://registry.npmmirror.com'; $env:NODE_USE_ENV_PROXY = '1'; node scripts/check-deps.mjs
```

Expected：输出中没有"查询失败"。2026-10-26 写计划时：共 77 个直接依赖，0 个可升级（9 个有新版但发布未满 7 天）。以执行当天的结果为准。

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

### Task 2: 方言：改动集与 DML 生成

`dblens-dialect/src/changes.rs`：
- `RowChange`（`update` / `insert` / `delete`，值为 `Value`，按列名）；
- `row_key(table)`：主键，没有时取第一个所有列都 NOT NULL 的唯一键；
- `Dialect::change_statements(schema, table, changes)`：每个改动一条语句，同时给出带占位符的 SQL 和参数（执行用）与值写成字面量的 SQL（预览用）。

规则：
- UPDATE 的 WHERE 是行标识列 = 原值，加上改动列 = 原值；NULL 用 `IS NULL`；Float、Text、Blob、Binary、VarBinary、Json、Other 和原值是大字段预览的列不参与比较。
- INSERT 省略为 NULL 的自增列和有默认值的列；一列都不写时借 `insert_sql` 生成"只用默认值"的写法。
- 顺序为删除、修改、新增。
- PG 驱动不能直接绑定 NUMERIC 参数：PG 系的精确小数占位符写成 `$n::text::numeric`（实库测试发现）。

后端词条加 `changes.no_key`、`changes.bad_key`、`changes.unknown_column`、`changes.generated_column`、`changes.empty_update`。

**Files:**
- Modify: `crates/dblens-core/locales/app.json`
- Create: `crates/dblens-dialect/src/changes.rs`
- Modify: `crates/dblens-dialect/src/lib.rs`

**Interfaces:**
- Produces（`dblens_dialect`）：
  - `RowChange::{Update { key, original, values }, Insert { values }, Delete { key }}`，字段为 `BTreeMap<String, Value>`，serde 按 `kind`（snake_case）区分；
  - `ChangeStatement { index: usize, sql: String, params: Vec<Value>（不序列化）, preview: String }`；
  - `row_key(&Table) -> Option<Vec<&Column>>`；
  - `Dialect::change_statements(&self, schema: &str, table: &Table, changes: &[RowChange]) -> Result<Vec<ChangeStatement>>`。

- [ ] **Step 1: 修改 `crates/dblens-core/locales/app.json`**

```diff
--- a/crates/dblens-core/locales/app.json
+++ b/crates/dblens-core/locales/app.json
@@ -8,6 +8,11 @@
   "api.setting_too_large": {"zh-CN": "设置项 %{key} 的值超过 64 KiB", "en": "The value of setting %{key} exceeds 64 KiB"},
   "api.still_running": {"zh-CN": "上一段 SQL 还没有执行完", "en": "The previous SQL has not finished yet"},
   "app.name": {"zh-CN": "DBLens 数洞", "en": "DBLens"},
+  "changes.bad_key": {"zh-CN": "改动的行标识与表 %{table} 的主键（或唯一键）不一致", "en": "The row identity of a change does not match the primary (or unique) key of table %{table}"},
+  "changes.empty_update": {"zh-CN": "修改中没有改动的列", "en": "An update has no changed columns"},
+  "changes.generated_column": {"zh-CN": "列 %{column} 是生成列，不能写入", "en": "Column %{column} is generated and cannot be written"},
+  "changes.no_key": {"zh-CN": "表 %{table} 没有主键或非空的唯一键，不能编辑", "en": "Table %{table} has no primary key or non-null unique key and cannot be edited"},
+  "changes.unknown_column": {"zh-CN": "列 %{column} 不存在", "en": "Column %{column} does not exist"},
   "ddl.truncated": {"zh-CN": "DDL 超过 64 KB，以上只是开头部分", "en": "The DDL exceeds 64 KB; only the beginning is shown above"},
   "ddl.unsupported": {"zh-CN": "该数据库不支持查看 %{kind} 的 DDL", "en": "This database does not support viewing the DDL of %{kind}"},
   "desktop.bind_failed": {"zh-CN": "无法监听本机端口：%{error}", "en": "Cannot listen on a local port: %{error}"},
```

- [ ] **Step 2: 新建 `crates/dblens-dialect/src/changes.rs`**

```rust
//! 表数据编辑的改动集（M2a 设计文档第 3、4 节）：由前端暂存的修改、新增、删除生成 DML。
//! 每个改动生成一条语句，同时给出带占位符的 SQL 和参数（执行用）与值写成字面量的 SQL（预览用）。

use std::collections::BTreeMap;

use dblens_core::schema::{Column, Table};
use dblens_core::{Error, LogicalType, Result, Value, msg};
use serde::{Deserialize, Serialize};

use crate::{Dialect, Family};

/// 一行的改动。列名 → 值；`key` 是行标识列的原值。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RowChange {
    Update {
        key: BTreeMap<String, Value>,
        /// 改动列的原值（冲突检查用）。
        original: BTreeMap<String, Value>,
        values: BTreeMap<String, Value>,
    },
    Insert {
        values: BTreeMap<String, Value>,
    },
    Delete {
        key: BTreeMap<String, Value>,
    },
}

/// 一条生成的语句。`index` 是它在改动集中的序号。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ChangeStatement {
    pub index: usize,
    pub sql: String,
    #[serde(skip)]
    pub params: Vec<Value>,
    /// 值写成字面量的 SQL，预览和执行历史用。
    pub preview: String,
}

/// 标识一行用的列：主键；没有主键时取第一个所有列都 NOT NULL 的唯一键。都没有时为 None（整张表只读）。
pub fn row_key(table: &Table) -> Option<Vec<&Column>> {
    let find = |names: &[String]| -> Option<Vec<&Column>> {
        let cols: Option<Vec<&Column>> =
            names.iter().map(|n| table.columns.iter().find(|c| &c.name == n)).collect();
        cols.filter(|c| !c.is_empty())
    };
    if let Some(pk) = &table.primary_key
        && let Some(cols) = find(&pk.columns)
    {
        return Some(cols);
    }
    table.uniques.iter().filter_map(|u| find(&u.columns)).find(|cols| cols.iter().all(|c| !c.nullable))
}

/// 能否用 `=` 可靠地比较原值：浮点有精度问题，长文本（Oracle、达梦的 CLOB）、大字段、二进制、JSON 不能或不宜比较。
fn comparable(col: &Column, original: &Value) -> bool {
    let by_type = !matches!(
        col.data_type.logical,
        LogicalType::Float { .. }
            | LogicalType::Text
            | LogicalType::Blob
            | LogicalType::Binary { .. }
            | LogicalType::VarBinary { .. }
            | LogicalType::Json
            | LogicalType::Other
    );
    by_type && !matches!(original, Value::Lob(_))
}

/// 语句的两种形式一起拼：执行用占位符，预览用字面量。
struct Builder<'a> {
    dialect: &'a Dialect,
    sql: String,
    preview: String,
    params: Vec<Value>,
}

impl<'a> Builder<'a> {
    fn new(dialect: &'a Dialect) -> Self {
        Self { dialect, sql: String::new(), preview: String::new(), params: Vec::new() }
    }

    fn text(&mut self, s: &str) {
        self.sql.push_str(s);
        self.preview.push_str(s);
    }

    fn value(&mut self, v: &Value) -> Result<()> {
        self.params.push(v.clone());
        self.sql.push_str(&self.dialect.placeholder(self.params.len()));
        // PG 驱动不能直接绑定 NUMERIC 参数：精确小数按文本传，由数据库转换（整数列也接受 numeric 的赋值和比较）
        if self.dialect.family() == Family::Postgres && matches!(v, Value::Decimal(_)) {
            self.sql.push_str("::text::numeric");
        }
        self.preview.push_str(&self.dialect.literal(v)?);
        Ok(())
    }

    /// `列 = 值`；值为 NULL 时 `列 IS NULL`（不占参数）。
    fn equals(&mut self, col: &str, v: &Value) -> Result<()> {
        self.text(&self.dialect.ident(col));
        if matches!(v, Value::Null) {
            self.text(" IS NULL");
            Ok(())
        } else {
            self.text(" = ");
            self.value(v)
        }
    }

    fn finish(self, index: usize) -> ChangeStatement {
        ChangeStatement { index, sql: self.sql, params: self.params, preview: self.preview }
    }
}

fn column<'t>(table: &'t Table, name: &str) -> Result<&'t Column> {
    table
        .columns
        .iter()
        .find(|c| c.name == name)
        .ok_or_else(|| Error::invalid(msg!("changes.unknown_column", column = name)))
}

impl Dialect {
    /// 由改动集生成语句：先删除，再修改，最后新增（"改掉主键再新增一行用回旧值"不冲突）；同类改动保持原顺序。
    /// 表没有行标识、改动引用了不存在的列、写了生成列、`key` 的列与行标识不一致时返回 `invalid`。
    pub fn change_statements(
        &self,
        schema: &str,
        table: &Table,
        changes: &[RowChange],
    ) -> Result<Vec<ChangeStatement>> {
        let key = row_key(table).ok_or_else(|| Error::invalid(msg!("changes.no_key", table = table.name)))?;
        let target = self.qualified(Some(schema), &table.name);
        let check_key = |k: &BTreeMap<String, Value>| -> Result<()> {
            let same = k.len() == key.len() && key.iter().all(|c| k.contains_key(&c.name));
            if same { Ok(()) } else { Err(Error::invalid(msg!("changes.bad_key", table = table.name))) }
        };
        let writable = |name: &str| -> Result<&Column> {
            let c = column(table, name)?;
            if c.generated.is_some() {
                return Err(Error::invalid(msg!("changes.generated_column", column = name)));
            }
            Ok(c)
        };
        let order = |c: &RowChange| match c {
            RowChange::Delete { .. } => 0,
            RowChange::Update { .. } => 1,
            RowChange::Insert { .. } => 2,
        };
        let mut indexed: Vec<(usize, &RowChange)> = changes.iter().enumerate().collect();
        indexed.sort_by_key(|(i, c)| (order(c), *i));

        let mut out = Vec::with_capacity(changes.len());
        for (index, change) in indexed {
            let mut b = Builder::new(self);
            match change {
                RowChange::Delete { key: k } => {
                    check_key(k)?;
                    b.text(&format!("DELETE FROM {target} WHERE "));
                    for (i, c) in key.iter().enumerate() {
                        if i > 0 {
                            b.text(" AND ");
                        }
                        b.equals(&c.name, &k[&c.name])?;
                    }
                }
                RowChange::Update { key: k, original, values } => {
                    check_key(k)?;
                    if values.is_empty() {
                        return Err(Error::invalid(msg!("changes.empty_update")));
                    }
                    b.text(&format!("UPDATE {target} SET "));
                    for (i, (name, v)) in values.iter().enumerate() {
                        writable(name)?;
                        if i > 0 {
                            b.text(", ");
                        }
                        b.text(&format!("{} = ", self.ident(name)));
                        b.value(v)?;
                    }
                    b.text(" WHERE ");
                    for (i, c) in key.iter().enumerate() {
                        if i > 0 {
                            b.text(" AND ");
                        }
                        b.equals(&c.name, &k[&c.name])?;
                    }
                    // 改动列的原值：别人在此期间改过同一字段时影响行数为 0
                    for (name, old) in original {
                        let c = column(table, name)?;
                        if values.contains_key(name)
                            && !key.iter().any(|k| k.name == c.name)
                            && comparable(c, old)
                        {
                            b.text(" AND ");
                            b.equals(name, old)?;
                        }
                    }
                }
                RowChange::Insert { values } => {
                    // 自增列和有默认值的列留空（NULL）时不写，交给数据库
                    let cols: Vec<(&String, &Value)> = values
                        .iter()
                        .filter(|(name, v)| {
                            let c = table.columns.iter().find(|c| &&c.name == name);
                            !(matches!(v, Value::Null)
                                && c.is_some_and(|c| c.auto_increment || c.default.is_some()))
                        })
                        .collect();
                    for (name, _) in &cols {
                        writable(name)?;
                    }
                    if cols.is_empty() {
                        // 全部用默认值：把所有列当作自增列，借 insert_sql 生成各方言"只用默认值"的写法（不含占位符）
                        let defaults = Table {
                            columns: table
                                .columns
                                .iter()
                                .map(|c| Column { auto_increment: true, ..c.clone() })
                                .collect(),
                            ..table.clone()
                        };
                        b.text(&self.insert_sql(Some(schema), &defaults).sql);
                    } else {
                        let names: Vec<String> = cols.iter().map(|(n, _)| self.ident(n)).collect();
                        b.text(&format!("INSERT INTO {target} ({}) VALUES (", names.join(", ")));
                        for (i, (_, v)) in cols.iter().enumerate() {
                            if i > 0 {
                                b.text(", ");
                            }
                            b.value(v)?;
                        }
                        b.text(")");
                    }
                }
            }
            out.push(b.finish(index));
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use dblens_core::ColumnType;
    use dblens_core::schema::{PrimaryKey, UniqueConstraint};

    use super::*;
    use crate::Variant;

    fn col(name: &str, logical: LogicalType) -> Column {
        Column {
            name: name.into(),
            ordinal: 0,
            data_type: ColumnType { native: "x".into(), logical },
            nullable: true,
            default: None,
            comment: None,
            auto_increment: false,
            generated: None,
        }
    }

    fn orders() -> Table {
        let mut id = col("id", LogicalType::Int { bits: 32 });
        id.auto_increment = true;
        id.nullable = false;
        let mut status = col("status", LogicalType::VarChar { len: Some(10) });
        status.default = Some("'new'".into());
        let mut total = col("total", LogicalType::Decimal { precision: Some(10), scale: Some(2) });
        total.generated = Some("price * qty".into());
        Table {
            name: "orders".into(),
            columns: vec![
                id,
                col("name", LogicalType::VarChar { len: Some(20) }),
                col("price", LogicalType::Decimal { precision: Some(10), scale: Some(2) }),
                col("rate", LogicalType::Float { bits: 64 }),
                col("note", LogicalType::Text),
                status,
                total,
            ],
            primary_key: Some(PrimaryKey { name: None, columns: vec!["id".into()] }),
            ..Default::default()
        }
    }

    fn map(pairs: &[(&str, Value)]) -> BTreeMap<String, Value> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.clone())).collect()
    }

    fn s(v: &str) -> Value {
        Value::String(v.into())
    }

    #[test]
    fn update_where_has_key_and_comparable_originals() {
        let change = RowChange::Update {
            key: map(&[("id", Value::Int(1))]),
            original: map(&[
                ("name", s("旧")),
                ("price", Value::Null),
                ("rate", Value::Float(1.5)),
                ("note", s("长文本")),
            ]),
            values: map(&[
                ("name", s("新")),
                ("price", Value::Decimal("9.90".into())),
                ("rate", Value::Float(2.0)),
                ("note", s("x")),
            ]),
        };
        let st = Dialect::new(Variant::Postgres).change_statements("public", &orders(), &[change]).unwrap();
        assert_eq!(
            st[0].sql,
            "UPDATE public.orders SET name = $1, note = $2, price = $3::text::numeric, rate = $4 \
             WHERE id = $5 AND name = $6 AND price IS NULL"
        );
        assert_eq!(st[0].params.len(), 6);
        assert_eq!(
            st[0].preview,
            "UPDATE public.orders SET name = '新', note = 'x', price = 9.90, rate = 2 \
             WHERE id = 1 AND name = '旧' AND price IS NULL"
        );
    }

    #[test]
    fn order_is_delete_update_insert_and_placeholders_follow_dialect() {
        let changes = [
            RowChange::Insert {
                values: map(&[("id", Value::Null), ("name", s("a")), ("status", Value::Null)]),
            },
            RowChange::Update {
                key: map(&[("id", Value::Int(2))]),
                original: map(&[]),
                values: map(&[("name", s("b"))]),
            },
            RowChange::Delete { key: map(&[("id", Value::Int(3))]) },
        ];
        let st = Dialect::new(Variant::Oracle).change_statements("HR", &orders(), &changes).unwrap();
        assert_eq!(st.iter().map(|s| s.index).collect::<Vec<_>>(), [2, 1, 0]);
        // Oracle 的小写名要加引号
        assert_eq!(st[0].sql, r#"DELETE FROM HR."orders" WHERE "id" = :1"#);
        assert_eq!(st[1].sql, r#"UPDATE HR."orders" SET "name" = :1 WHERE "id" = :2"#);
        // 自增列和有默认值的列为 NULL 时不写
        assert_eq!(st[2].sql, r#"INSERT INTO HR."orders" ("name") VALUES (:1)"#);
        let my = Dialect::new(Variant::MySql).change_statements("shop", &orders(), &changes).unwrap();
        assert_eq!(my[1].sql, "UPDATE shop.orders SET name = ? WHERE id = ?");
    }

    #[test]
    fn rejects_bad_changes() {
        let d = Dialect::new(Variant::Postgres);
        let t = orders();
        let err = |c: RowChange| d.change_statements("p", &t, &[c]).unwrap_err();
        assert!(matches!(err(RowChange::Delete { key: map(&[("name", s("x"))]) }), Error::Invalid { .. }));
        assert!(matches!(
            err(RowChange::Insert { values: map(&[("total", Value::Int(1))]) }),
            Error::Invalid { .. }
        ));
        assert!(matches!(
            err(RowChange::Insert { values: map(&[("nope", Value::Int(1))]) }),
            Error::Invalid { .. }
        ));
        assert!(matches!(
            err(RowChange::Update {
                key: map(&[("id", Value::Int(1))]),
                original: map(&[]),
                values: map(&[])
            }),
            Error::Invalid { .. }
        ));
        let no_key = Table { primary_key: None, ..orders() };
        assert!(d.change_statements("p", &no_key, &[RowChange::Insert { values: map(&[]) }]).is_err());
    }

    #[test]
    fn row_key_falls_back_to_not_null_unique() {
        let mut code = col("code", LogicalType::VarChar { len: Some(5) });
        let t = |code: Column| Table {
            name: "t".into(),
            columns: vec![col("a", LogicalType::Int { bits: 32 }), code],
            uniques: vec![UniqueConstraint { name: None, columns: vec!["code".into()] }],
            ..Default::default()
        };
        assert!(row_key(&t(code.clone())).is_none(), "可为空的唯一键不能标识行");
        code.nullable = false;
        assert_eq!(row_key(&t(code)).unwrap().iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["code"]);
        assert_eq!(row_key(&orders()).unwrap()[0].name, "id");
    }

    #[test]
    fn insert_with_only_defaults_uses_insert_template() {
        let st = Dialect::new(Variant::Postgres)
            .change_statements("p", &orders(), &[RowChange::Insert { values: map(&[("id", Value::Null)]) }])
            .unwrap();
        assert!(st[0].sql.starts_with("INSERT INTO p.orders"), "{}", st[0].sql);
    }
}
```

- [ ] **Step 3: 修改 `crates/dblens-dialect/src/lib.rs`**

```diff
--- a/crates/dblens-dialect/src/lib.rs
+++ b/crates/dblens-dialect/src/lib.rs
@@ -1,6 +1,7 @@
 //! 方言层：不做任何 IO，只负责各数据库的语法差异——标识符与字面量、语句拆分、类型映射、SQL 生成。
 
 mod browse;
+mod changes;
 mod ddl;
 mod dml;
 pub mod ident;
@@ -17,6 +18,7 @@ use dblens_core::{LogicalType, Result, Value};
 use dblens_driver::{DriverId, ServerInfo};
 
 pub use browse::{Direction, Relation, relations};
+pub use changes::{ChangeStatement, RowChange, row_key};
 pub use ddl::{DdlKind, ddl_text, normalize_ddl};
 pub use dml::DmlTemplate;
 pub use ops::{OpsCategory, OpsQuery};
```

- [ ] **Step 4: 检查**

```bash
export CARGO_INCREMENTAL=0
cargo fmt --all
cargo clippy --config "source.mirror.registry='sparse+https://rsproxy.cn/index/'" --workspace --all-targets -- -D warnings
DBLENS_LANG=en cargo test --config "source.mirror.registry='sparse+https://rsproxy.cn/index/'" --workspace
```

Expected: 全部测试通过（dialect 新增 5 个：WHERE 的构成、语句顺序和各方言占位符、非法改动、唯一键回退、只用默认值的新增）。

- [ ] **Step 5: 提交**

```bash
git add crates/dblens-core/locales/app.json crates/dblens-dialect/src/changes.rs crates/dblens-dialect/src/lib.rs
git commit -m "feat(dialect): 表数据编辑的改动集：行标识、按改动生成 DML（执行用占位符、预览用字面量）" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 3: 服务层：事务执行与冲突检查

- 会话增加 `Command::Apply`：空闲时执行（执行中报 `Busy`）。
  - 会话已在手动事务中时报 `invalid`。
  - 临时关闭自动提交，逐条执行：先经过执行策略（拒绝算失败，要求确认的直接执行），影响行数必须为 1（为 0 报冲突，大于 1 报行标识不唯一）。
  - 全部成功才提交，否则回滚；最后恢复自动提交。
  - 每条执行过的语句写一条历史（预览形式），回滚时都记为失败。
- `changes.rs`：`ChangeSet`；`Service::preview_changes(profile_id, set)`、`Service::apply_changes(session_id, set)`（取表的结构模型，生成语句，交给会话）。
- 假驱动：SQL 含 `zero_rows` 时影响 0 行。
- 实库测试 `*_editing`：一次提交新增、修改、删除；另一个会话先改同一字段后冲突回滚；没有主键的表被拒绝。

**Files:**
- Modify: `crates/dblens-core/locales/app.json`
- Create: `crates/dblens-service/src/changes.rs`
- Modify: `crates/dblens-service/src/fake.rs`
- Modify: `crates/dblens-service/src/lib.rs`
- Modify: `crates/dblens-service/src/session.rs`
- Modify: `crates/dblens-service/tests/live.rs`

**Interfaces:**
- Consumes：任务 2 的 `ChangeStatement`、`RowChange`、`Dialect::change_statements`。
- Produces（`dblens_service`）：
  - `SessionManager::apply(&self, id, statements: Vec<ChangeStatement>) -> Result<ApplyOutcome>`；
  - `ApplyOutcome { applied: usize, failed: Option<ApplyFailure> }`、`ApplyFailure { index: usize, error: Error }`（Serialize）；
  - `ChangeSet { schema, table, changes: Vec<RowChange> }`（Deserialize）；
  - `Service::preview_changes(&self, profile_id: &str, set: &ChangeSet) -> Result<Vec<ChangeStatement>>`；
  - `Service::apply_changes(&self, session: SessionId, set: &ChangeSet) -> Result<ApplyOutcome>`；
  - 后端词条 `changes.in_transaction`、`changes.conflict`、`changes.not_unique`、`changes.rolled_back`。

- [ ] **Step 1: 修改 `crates/dblens-service/tests/live.rs`**

```diff
--- a/crates/dblens-service/tests/live.rs
+++ b/crates/dblens-service/tests/live.rs
@@ -10,12 +10,12 @@ use std::time::Duration;
 
 use dblens_core::{Error, Value};
 use dblens_dialect::meta::MetaKey;
-use dblens_dialect::{DdlKind, Direction};
+use dblens_dialect::{DdlKind, Direction, RowChange};
 use dblens_driver::{ConnectParams, DriverId};
 use dblens_service::{
-    DataOrder, DefaultPolicy, DriverRegistry, Environment, ExecRequest, ExportFormat, ExportOptions,
-    ExportRequest, ExportSource, ExportState, ExportStatus, GenerateKind, ObjectKind, ProfileInput,
-    SecretBackend, Secrets, Service, SessionEvent, SessionId, Store,
+    ChangeSet, DataOrder, DefaultPolicy, DriverRegistry, Environment, ExecRequest, ExportFormat,
+    ExportOptions, ExportRequest, ExportSource, ExportState, ExportStatus, GenerateKind, ObjectKind,
+    ProfileInput, SecretBackend, Secrets, Service, SessionEvent, SessionId, Store,
 };
 
 struct Live {
@@ -613,3 +613,181 @@ async fn oracle_export() {
 async fn dameng_export() {
     export_flow("DBLENS_TEST_DM_URL").await;
 }
+
+/// 单元格的文本，比较用（各库返回的类型不同：Oracle 的整数是 Decimal）。
+fn text(v: &Value) -> Option<String> {
+    match v {
+        Value::Null => None,
+        Value::Int(i) => Some(i.to_string()),
+        Value::UInt(u) => Some(u.to_string()),
+        Value::Decimal(s) | Value::String(s) => Some(s.clone()),
+        other => Some(format!("{other:?}")),
+    }
+}
+
+/// 表数据编辑（M2a）：一次提交新增、修改、删除；另一个会话先改了同一字段时冲突回滚；没有主键的表被拒绝。
+async fn editing_flow(env: &str) {
+    let Some(live) = setup(env).await else { return };
+    // 下面的闭包要多次调用，只借用 live
+    let live = &live;
+    let s = &live.service;
+    let id = s.open_session(&live.profile_id).await.unwrap().id;
+    let upper = matches!(live.driver, DriverId::Oracle | DriverId::Dameng);
+    let n = |name: &str| if upper { name.to_uppercase() } else { name.to_string() };
+    let drop = match live.driver {
+        DriverId::Oracle => vec![
+            "BEGIN EXECUTE IMMEDIATE 'DROP TABLE m2_t'; EXCEPTION WHEN OTHERS THEN NULL; END;",
+            "BEGIN EXECUTE IMMEDIATE 'DROP TABLE m2_nokey'; EXCEPTION WHEN OTHERS THEN NULL; END;",
+        ],
+        _ => vec!["DROP TABLE IF EXISTS m2_t", "DROP TABLE IF EXISTS m2_nokey"],
+    };
+    for sql in &drop {
+        live.submit(id, ExecRequest::new(*sql)).await;
+    }
+    let create = match live.driver {
+        DriverId::Mysql => {
+            "CREATE TABLE m2_t (id INT AUTO_INCREMENT PRIMARY KEY, code VARCHAR(10) NOT NULL UNIQUE, \
+             name VARCHAR(20), qty INT DEFAULT 5, price DECIMAL(10, 2))"
+        }
+        DriverId::Postgres => {
+            "CREATE TABLE m2_t (id INT GENERATED BY DEFAULT AS IDENTITY PRIMARY KEY, code VARCHAR(10) NOT NULL UNIQUE, \
+             name VARCHAR(20), qty INT DEFAULT 5, price DECIMAL(10, 2))"
+        }
+        DriverId::Oracle => {
+            "CREATE TABLE m2_t (id NUMBER GENERATED BY DEFAULT AS IDENTITY PRIMARY KEY, code VARCHAR2(10) NOT NULL UNIQUE, \
+             name VARCHAR2(20), qty NUMBER DEFAULT 5, price NUMBER(10, 2))"
+        }
+        DriverId::Dameng => {
+            "CREATE TABLE m2_t (id INT IDENTITY(1, 1) PRIMARY KEY, code VARCHAR(10) NOT NULL UNIQUE, \
+             name VARCHAR(20), qty INT DEFAULT 5, price DECIMAL(10, 2))"
+        }
+    };
+    live.run(id, create).await;
+    live.run(id, "CREATE TABLE m2_nokey (a INT)").await;
+    for (code, name, qty) in [("A", "'甲'", 1), ("B", "NULL", 2), ("C", "'丙'", 3)] {
+        live.run(
+            id,
+            &format!("INSERT INTO m2_t (code, name, qty, price) VALUES ('{code}', {name}, {qty}, 1.50)"),
+        )
+        .await;
+    }
+    let one = |sql: String| async move { first_value(&live.run(id, &sql).await) };
+    let id_of = |code: &str| one(format!("SELECT id FROM m2_t WHERE code = '{code}'"));
+    let (a, b, c) = (id_of("A").await, id_of("B").await, id_of("C").await);
+    let map = |pairs: Vec<(&str, Value)>| pairs.into_iter().map(|(k, v)| (n(k), v)).collect();
+    let set = |changes: Vec<RowChange>| ChangeSet { schema: live.schema.clone(), table: n("m2_t"), changes };
+
+    // 一次提交：修改 A、删除 B、新增 D（id、qty 留空，用自增和默认值）
+    let changes = set(vec![
+        RowChange::Update {
+            key: map(vec![("id", a.clone())]),
+            original: map(vec![
+                ("name", Value::String("甲".into())),
+                ("qty", one("SELECT qty FROM m2_t WHERE code = 'A'".into()).await),
+            ]),
+            values: map(vec![("name", Value::String("甲改".into())), ("qty", Value::Int(10))]),
+        },
+        RowChange::Delete { key: map(vec![("id", b)]) },
+        RowChange::Insert {
+            values: map(vec![
+                ("id", Value::Null),
+                ("code", Value::String("D".into())),
+                ("name", Value::String("新".into())),
+                ("qty", Value::Null),
+                ("price", Value::Decimal("4.50".into())),
+            ]),
+        },
+    ]);
+    let preview = s.preview_changes(&live.profile_id, &changes).await.unwrap();
+    assert_eq!(preview.len(), 3, "{env}");
+    assert!(preview[0].preview.starts_with("DELETE"), "{env}：先删除 {preview:?}");
+    assert!(preview[2].preview.contains("'新'"), "{env}：{}", preview[2].preview);
+    let outcome = s.apply_changes(id, &changes).await.unwrap();
+    assert_eq!((outcome.applied, outcome.failed.as_ref().map(|f| f.index)), (3, None), "{env}：{outcome:?}");
+    assert_eq!(
+        text(&one("SELECT name FROM m2_t WHERE code = 'A'".into()).await).as_deref(),
+        Some("甲改"),
+        "{env}"
+    );
+    assert_eq!(
+        text(&one("SELECT COUNT(*) FROM m2_t WHERE code = 'B'".into()).await).as_deref(),
+        Some("0"),
+        "{env}"
+    );
+    assert_eq!(
+        text(&one("SELECT qty FROM m2_t WHERE code = 'D'".into()).await).as_deref(),
+        Some("5"),
+        "{env}：默认值"
+    );
+
+    // 冲突：另一个会话先改了 C 的 name，再按旧值提交，整体回滚
+    let other = s.open_session(&live.profile_id).await.unwrap().id;
+    live.run(other, "UPDATE m2_t SET name = '别人' WHERE code = 'C'").await;
+    s.sessions.close(other, true).await.unwrap();
+    let conflict = set(vec![
+        RowChange::Update {
+            key: map(vec![("id", a)]),
+            original: map(vec![("name", Value::String("甲改".into()))]),
+            values: map(vec![("name", Value::String("不该写入".into()))]),
+        },
+        RowChange::Update {
+            key: map(vec![("id", c)]),
+            original: map(vec![("name", Value::String("丙".into()))]),
+            values: map(vec![("name", Value::String("我".into()))]),
+        },
+    ]);
+    let outcome = s.apply_changes(id, &conflict).await.unwrap();
+    let failed = outcome.failed.expect("应当冲突");
+    assert_eq!(failed.index, 1, "{env}");
+    assert!(matches!(failed.error, Error::Invalid { .. }), "{env}：{:?}", failed.error);
+    assert_eq!(
+        text(&one("SELECT name FROM m2_t WHERE code = 'C'".into()).await).as_deref(),
+        Some("别人"),
+        "{env}"
+    );
+    assert_eq!(
+        text(&one("SELECT name FROM m2_t WHERE code = 'A'".into()).await).as_deref(),
+        Some("甲改"),
+        "{env}：回滚"
+    );
+    let info = s.sessions.info(id).unwrap();
+    assert!(info.autocommit && !info.in_transaction, "{env}：{info:?}");
+
+    // 没有主键或唯一键的表不能编辑
+    let nokey = ChangeSet {
+        schema: live.schema.clone(),
+        table: n("m2_nokey"),
+        changes: vec![RowChange::Insert { values: map(vec![("a", Value::Int(1))]) }],
+    };
+    assert!(matches!(s.preview_changes(&live.profile_id, &nokey).await, Err(Error::Invalid { .. })), "{env}");
+
+    for sql in &drop {
+        live.submit(id, ExecRequest::new(*sql)).await;
+    }
+    s.sessions.close(id, false).await.unwrap();
+}
+
+#[tokio::test]
+async fn mysql_editing() {
+    editing_flow("DBLENS_TEST_MYSQL_URL").await;
+}
+
+#[tokio::test]
+async fn mariadb_editing() {
+    editing_flow("DBLENS_TEST_MARIADB_URL").await;
+}
+
+#[tokio::test]
+async fn postgres_editing() {
+    editing_flow("DBLENS_TEST_PG_URL").await;
+}
+
+#[tokio::test]
+async fn oracle_editing() {
+    editing_flow("DBLENS_TEST_ORACLE_URL").await;
+}
+
+#[tokio::test]
+async fn dameng_editing() {
+    editing_flow("DBLENS_TEST_DM_URL").await;
+}
```

- [ ] **Step 2: 修改 `crates/dblens-core/locales/app.json`**

```diff
--- a/crates/dblens-core/locales/app.json
+++ b/crates/dblens-core/locales/app.json
@@ -9,9 +9,13 @@
   "api.still_running": {"zh-CN": "上一段 SQL 还没有执行完", "en": "The previous SQL has not finished yet"},
   "app.name": {"zh-CN": "DBLens 数洞", "en": "DBLens"},
   "changes.bad_key": {"zh-CN": "改动的行标识与表 %{table} 的主键（或唯一键）不一致", "en": "The row identity of a change does not match the primary (or unique) key of table %{table}"},
+  "changes.conflict": {"zh-CN": "第 %{n} 个改动没有影响任何行：这一行已被修改或删除，请刷新后再改", "en": "Change %{n} affected no rows: the row has been modified or deleted; refresh and try again"},
   "changes.empty_update": {"zh-CN": "修改中没有改动的列", "en": "An update has no changed columns"},
   "changes.generated_column": {"zh-CN": "列 %{column} 是生成列，不能写入", "en": "Column %{column} is generated and cannot be written"},
+  "changes.in_transaction": {"zh-CN": "会话中有未提交的事务，不能提交编辑", "en": "The session has an uncommitted transaction; edits cannot be committed"},
   "changes.no_key": {"zh-CN": "表 %{table} 没有主键或非空的唯一键，不能编辑", "en": "Table %{table} has no primary key or non-null unique key and cannot be edited"},
+  "changes.not_unique": {"zh-CN": "第 %{n} 个改动影响了 %{count} 行：行标识不唯一", "en": "Change %{n} affected %{count} rows: the row identity is not unique"},
+  "changes.rolled_back": {"zh-CN": "已回滚（同一次提交中的其他改动失败）", "en": "Rolled back (another change in the same commit failed)"},
   "changes.unknown_column": {"zh-CN": "列 %{column} 不存在", "en": "Column %{column} does not exist"},
   "ddl.truncated": {"zh-CN": "DDL 超过 64 KB，以上只是开头部分", "en": "The DDL exceeds 64 KB; only the beginning is shown above"},
   "ddl.unsupported": {"zh-CN": "该数据库不支持查看 %{kind} 的 DDL", "en": "This database does not support viewing the DDL of %{kind}"},
```

- [ ] **Step 3: 新建 `crates/dblens-service/src/changes.rs`**

```rust
//! 表数据编辑（M2a 设计文档第 5 节）：由改动集生成语句，预览或在会话中执行。

use dblens_core::{Error, Result, msg};
use dblens_dialect::{ChangeStatement, RowChange};
use serde::Deserialize;

use crate::profile::Profile;
use crate::service::Service;
use crate::session::{ApplyOutcome, SessionId};

/// 前端提交的改动集。
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ChangeSet {
    pub schema: String,
    pub table: String,
    pub changes: Vec<RowChange>,
}

impl Service {
    async fn change_statements(&self, profile: &Profile, set: &ChangeSet) -> Result<Vec<ChangeStatement>> {
        let dialect = self.metadata.dialect(profile).await?;
        let table = self
            .metadata
            .tables(profile, &set.schema, Some(&set.table))
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| {
                Error::not_found(msg!(
                    "noun.object",
                    kind = "table",
                    name = format!("{}.{}", set.schema, set.table)
                ))
            })?;
        dialect.change_statements(&set.schema, &table, &set.changes)
    }

    /// 预览：生成的语句（`preview` 为值写成字面量的 SQL），不执行。
    pub async fn preview_changes(&self, profile_id: &str, set: &ChangeSet) -> Result<Vec<ChangeStatement>> {
        let profile = self.profile(profile_id)?;
        self.change_statements(&profile, set).await
    }

    /// 在会话中执行改动集（一个事务，失败整体回滚）。
    pub async fn apply_changes(&self, session: SessionId, set: &ChangeSet) -> Result<ApplyOutcome> {
        let info = self.sessions.info(session)?;
        let profile = self.profile(&info.profile_id)?;
        let statements = self.change_statements(&profile, set).await?;
        self.sessions.apply(session, statements).await
    }
}
```

- [ ] **Step 4: 修改 `crates/dblens-service/src/fake.rs`**

```diff
--- a/crates/dblens-service/src/fake.rs
+++ b/crates/dblens-service/src/fake.rs
@@ -143,6 +143,8 @@ impl Connection for FakeConn {
                 stream.events.push_back(ExecEvent::Affected(0));
             }
             "SLEEP" => stream.sleep = Some((self.cancelled.clone(), self.wake.clone())),
+            // 表数据编辑的冲突：没有影响任何行
+            _ if sql.contains("zero_rows") => stream.events.push_back(ExecEvent::Affected(0)),
             _ => match sql.strip_prefix("ROWS ").and_then(|n| n.parse::<i64>().ok()) {
                 Some(n) => {
                     stream
```

- [ ] **Step 5: 修改 `crates/dblens-service/src/lib.rs`**

```diff
--- a/crates/dblens-service/src/lib.rs
+++ b/crates/dblens-service/src/lib.rs
@@ -1,5 +1,6 @@
 //! 服务层：连接配置、密码、驱动注册表、会话、元数据缓存、执行策略。供 `dblens-api` 使用，自身不处理 HTTP。
 
+mod changes;
 mod export;
 /// 测试用的假驱动，供本 crate 和 `dblens-api` 的测试使用（feature `testing`）。
 #[cfg(any(test, feature = "testing"))]
@@ -16,6 +17,7 @@ mod service;
 mod session;
 mod store;
 
+pub use changes::ChangeSet;
 pub use export::{
     EXPORT_LOB_LIMIT, ExportFormat, ExportManager, ExportOptions, ExportRequest, ExportSource, ExportState,
     ExportStatus, ExportWarnings, MAX_RUNNING_EXPORTS,
@@ -34,5 +36,8 @@ pub use profile::{Environment, Profile, ProfileInput};
 pub use registry::{DriverRegistry, DriverStatus, ServerSummary};
 pub use secret::{SecretBackend, SecretStatus, Secrets};
 pub use service::{LANG_SETTING, Service};
-pub use session::{ExecRequest, SessionEvent, SessionId, SessionInfo, SessionManager, statement_at};
+pub use session::{
+    ApplyFailure, ApplyOutcome, ExecRequest, SessionEvent, SessionId, SessionInfo, SessionManager,
+    statement_at,
+};
 pub use store::{Store, default_data_dir};
```

- [ ] **Step 6: 修改 `crates/dblens-service/src/session.rs`**

```diff
--- a/crates/dblens-service/src/session.rs
+++ b/crates/dblens-service/src/session.rs
@@ -10,7 +10,7 @@ use std::sync::{Arc, Mutex};
 use std::time::{Instant, SystemTime, UNIX_EPOCH};
 
 use dblens_core::{Error, Msg, Result, Row, Value, msg};
-use dblens_dialect::{Dialect, SqlStatement, Variant, may_change_schema};
+use dblens_dialect::{ChangeStatement, Dialect, SqlStatement, Variant, may_change_schema};
 use dblens_driver::{CancelHandle, ColumnMeta, Connection, ExecEvent};
 use serde::{Deserialize, Serialize};
 use tokio::sync::{mpsc, oneshot};
@@ -175,11 +175,31 @@ async fn run_quiet(conn: &mut Box<dyn Connection>, sql: &str) -> Result<()> {
     Ok(())
 }
 
+/// 改动集执行失败的那一条（M2a）。
+#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
+pub struct ApplyFailure {
+    /// 在改动集中的序号。
+    pub index: usize,
+    pub error: Error,
+}
+
+/// 改动集的执行结果：全部成功时 `failed` 为 None，`applied` 是执行的语句数；失败时已整体回滚。
+#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
+pub struct ApplyOutcome {
+    pub applied: usize,
+    pub failed: Option<ApplyFailure>,
+}
+
 enum Command {
     Execute {
         req: ExecRequest,
         reply: oneshot::Sender<Result<()>>,
     },
+    /// 在一个事务里执行表数据编辑生成的语句（M2a）。
+    Apply {
+        statements: Vec<ChangeStatement>,
+        reply: oneshot::Sender<Result<ApplyOutcome>>,
+    },
     /// 切换当前 schema；`None` 只重新读取。
     Schema {
         set: Option<String>,
@@ -325,6 +345,12 @@ impl SessionManager {
         self.call(id, |reply| Command::Execute { req, reply }).await
     }
 
+    /// 在一个事务里执行表数据编辑的语句（M2a 设计文档第 5 节）：每条必须恰好影响 1 行，否则整体回滚。
+    /// 会话正在执行（如还有没读完的结果）时报 `Busy`；会话已经在手动事务中时报 `invalid`。
+    pub async fn apply(&self, id: SessionId, statements: Vec<ChangeStatement>) -> Result<ApplyOutcome> {
+        self.call(id, |reply| Command::Apply { statements, reply }).await
+    }
+
     /// 切换当前 schema，成功后会话信息中的 `schema` 随之更新。
     pub async fn set_schema(&self, id: SessionId, schema: &str) -> Result<()> {
         let set = Some(schema.to_string());
@@ -460,6 +486,13 @@ impl Actor {
                         return;
                     }
                 }
+                Command::Apply { statements, reply: tx } => {
+                    self.update(|i| i.busy = true);
+                    let r = self.apply(statements).await;
+                    self.update(|i| i.busy = false);
+                    self.note_error(&r);
+                    reply(tx, r);
+                }
                 Command::Next { reply: tx, .. } => reply(tx, Ok(None)),
                 Command::Schema { set, reply: tx } => {
                     let r = self.schema(set.as_deref()).await;
@@ -545,6 +578,84 @@ impl Actor {
         Ok(())
     }
 
+    /// 执行改动集：临时关闭自动提交，逐条执行（先经过执行策略，影响行数必须为 1），
+    /// 全部成功才提交，否则回滚。预览就是用户的确认，策略要求确认的语句直接执行；策略拒绝的语句算失败。
+    /// 每条执行过的语句都写一条历史（预览形式的 SQL）；回滚时它们都记为失败。
+    async fn apply(&mut self, statements: Vec<ChangeStatement>) -> Result<ApplyOutcome> {
+        let state = self.snapshot();
+        if state.broken {
+            return Err(Error::connect(msg!("session.broken_reconnect")));
+        }
+        if state.in_transaction || !state.autocommit {
+            return Err(Error::invalid(msg!("changes.in_transaction")));
+        }
+        let Actor { conn, ctx, owner, policy, history, .. } = self;
+        let Some(conn) = conn.as_mut() else { return Err(Error::connect(msg!("session.disconnected"))) };
+        conn.set_autocommit(false).await?;
+        let mut done: Vec<NewHistory> = Vec::new();
+        let mut failed: Option<ApplyFailure> = None;
+        for st in &statements {
+            let at = now_ms();
+            let started = Instant::now();
+            let info = classify(&ctx.dialect, &st.sql);
+            let result: Result<u64> = async {
+                if let Decision::Deny { reason } = policy.before_execute(ctx, &info).await {
+                    return Err(Error::Denied { reason });
+                }
+                let mut stream = conn.execute(&st.sql, &st.params).await?;
+                let mut affected = 0;
+                while let Some(ev) = stream.next(100).await? {
+                    if let ExecEvent::Affected(n) = ev {
+                        affected += n;
+                    }
+                }
+                match affected {
+                    1 => Ok(1),
+                    0 => Err(Error::invalid(msg!("changes.conflict", n = st.index + 1))),
+                    n => Err(Error::invalid(msg!("changes.not_unique", n = st.index + 1, count = n))),
+                }
+            }
+            .await;
+            let outcome = match &result {
+                Ok(_) => Outcome::Succeeded { affected: Some(1) },
+                Err(Error::Cancelled) => Outcome::Cancelled,
+                Err(e) => Outcome::Failed { message: e.to_string() },
+            };
+            policy.after_execute(ctx, &info, &outcome).await;
+            done.push(NewHistory {
+                at,
+                profile_id: ctx.profile_id.clone(),
+                profile_name: owner.name.clone(),
+                driver: owner.driver.clone(),
+                variant: Some(variant_code(ctx.dialect.variant())),
+                schema: state.schema.clone(),
+                sql: st.preview.clone(),
+                elapsed_ms: Some(started.elapsed().as_millis() as u64),
+                rows: result.as_ref().ok().copied(),
+                error: result.as_ref().err().map(ToString::to_string),
+            });
+            if let Err(error) = result {
+                failed = Some(ApplyFailure { index: st.index, error });
+                break;
+            }
+        }
+        let end = if failed.is_none() { conn.commit().await } else { conn.rollback().await };
+        // 恢复自动提交；提交或回滚已经结束了事务
+        let restore = conn.set_autocommit(true).await;
+        if failed.is_some() {
+            let rolled_back = msg!("changes.rolled_back").current();
+            for h in done.iter_mut().filter(|h| h.error.is_none()) {
+                h.error = Some(rolled_back.clone());
+            }
+        }
+        for h in done {
+            history.record(h);
+        }
+        end?;
+        restore?;
+        Ok(ApplyOutcome { applied: if failed.is_none() { statements.len() } else { 0 }, failed })
+    }
+
     /// 关闭连接；有未提交的修改时按 `commit` 提交或回滚。
     async fn close(&mut self, commit: bool) -> Result<()> {
         let Some(mut conn) = self.conn.take() else { return Ok(()) };
@@ -583,6 +694,7 @@ async fn take_request(rx: &mut mpsc::Receiver<Command>, queue: &mut VecDeque<Ses
             },
             Command::Close { commit, reply: tx } => return Req::Close(commit, tx),
             Command::Execute { reply: tx, .. } => reply(tx, Err(busy())),
+            Command::Apply { reply: tx, .. } => reply(tx, Err(busy())),
             Command::SetAutocommit { reply: tx, .. }
             | Command::Schema { reply: tx, .. }
             | Command::Commit { reply: tx }
@@ -797,6 +909,7 @@ impl Actor {
                 Command::Next { reply: tx, .. } => reply(tx, Ok(queue.pop_front())),
                 Command::Close { commit, reply: tx } => return Some((commit, tx)),
                 Command::Execute { reply: tx, .. } => reply(tx, Err(busy())),
+                Command::Apply { reply: tx, .. } => reply(tx, Err(busy())),
                 Command::SetAutocommit { reply: tx, .. }
                 | Command::Schema { reply: tx, .. }
                 | Command::Commit { reply: tx }
@@ -835,6 +948,65 @@ mod tests {
         (id, log)
     }
 
+    #[tokio::test]
+    async fn apply_commits_or_rolls_back_as_a_whole() {
+        let store = Arc::new(Store::open_in_memory().unwrap());
+        let m = SessionManager::new(
+            Arc::new(DefaultPolicy),
+            Arc::new(MetadataCache::new()),
+            HistoryLog::inline(store.clone()),
+        );
+        let (id, log) = open(&m, Environment::Prod);
+        let st = |index: usize, sql: &str| ChangeStatement {
+            index,
+            sql: sql.into(),
+            params: vec![],
+            preview: format!("{sql} -- 预览"),
+        };
+        let ok = m
+            .apply(id, vec![st(1, "DELETE FROM t WHERE id = 2"), st(0, "UPDATE t SET a = 1 WHERE id = 1")])
+            .await
+            .unwrap();
+        assert_eq!((ok.applied, ok.failed), (2, None));
+        let tail: Vec<String> = log.lock().unwrap().iter().rev().take(5).rev().cloned().collect();
+        assert_eq!(
+            tail,
+            [
+                "autocommit false",
+                "execute DELETE FROM t WHERE id = 2",
+                "execute UPDATE t SET a = 1 WHERE id = 1",
+                "commit",
+                "autocommit true"
+            ]
+        );
+
+        // 第二条没有影响任何行：整体回滚，两条都记为失败
+        let bad = m
+            .apply(
+                id,
+                vec![
+                    st(0, "UPDATE t SET a = 1 WHERE id = 1"),
+                    st(3, "UPDATE t SET zero_rows = 1 WHERE id = 9"),
+                ],
+            )
+            .await
+            .unwrap();
+        let failed = bad.failed.unwrap();
+        assert_eq!((bad.applied, failed.index), (0, 3));
+        assert!(matches!(failed.error, Error::Invalid { .. }));
+        assert!(log.lock().unwrap().ends_with(&["rollback".to_string(), "autocommit true".to_string()]));
+        let info = m.info(id).unwrap();
+        assert!(info.autocommit && !info.in_transaction && !info.busy);
+        let items = store.history(&HistoryQuery::default()).unwrap();
+        assert_eq!(items[0].entry.sql, "UPDATE t SET zero_rows = 1 WHERE id = 9 -- 预览");
+        assert!(items[0].entry.error.is_some() && items[1].entry.error.is_some(), "回滚时都记为失败");
+        assert_eq!(items[2].entry.error, None);
+
+        // 手动事务中不能提交编辑
+        m.set_autocommit(id, false).await.unwrap();
+        assert!(matches!(m.apply(id, vec![]).await, Err(Error::Invalid { .. })));
+    }
+
     #[tokio::test]
     async fn records_history_for_each_executed_statement() {
         let store = Arc::new(Store::open_in_memory().unwrap());
```

- [ ] **Step 7: 检查**

```bash
export CARGO_INCREMENTAL=0
cargo fmt --all
cargo clippy --config "source.mirror.registry='sparse+https://rsproxy.cn/index/'" --workspace --all-targets -- -D warnings
DBLENS_LANG=en cargo test --config "source.mirror.registry='sparse+https://rsproxy.cn/index/'" --workspace
```

实库测试：在上面的 `cargo test` 前以内联环境变量给出 `DBLENS_TEST_MYSQL_URL`、`DBLENS_TEST_MARIADB_URL`、`DBLENS_TEST_PG_URL`、`DBLENS_TEST_ORACLE_URL`、`DBLENS_TEST_DM_URL`（密码不写进任何文件）。

Expected: 全部测试通过（会话新增 `apply_commits_or_rolls_back_as_a_whole`）；`*_editing` 在 5 个实库上通过。

- [ ] **Step 8: 提交**

```bash
git add crates/dblens-core/locales/app.json crates/dblens-service/src/changes.rs crates/dblens-service/src/fake.rs crates/dblens-service/src/lib.rs crates/dblens-service/src/session.rs crates/dblens-service/tests/live.rs
git commit -m "feat(service): 表数据编辑：在会话中按事务执行改动集，检查影响行数，冲突时整体回滚" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 4: 接口

- `POST /api/profiles/{id}/changes/preview`：返回 `[{index, sql}]`（`sql` 为字面量形式）。
- `POST /api/sessions/{id}/changes`：返回 `{applied, failed}`；失败也是 200，`failed` 带序号和错误对象（与设计文档的差异，见上）。
- 接口说明补充"表数据编辑"一节。

**Files:**
- Modify: `crates/dblens-api/src/lib.rs`
- Modify: `crates/dblens-api/src/routes.rs`
- Modify: `crates/dblens-api/tests/api.rs`
- Modify: `docs/notes/m0-4b-api.md`

**Interfaces:**
- Consumes：任务 3 的 `Service::preview_changes`、`Service::apply_changes`。
- Produces（HTTP）：见上。

- [ ] **Step 1: 修改 `crates/dblens-api/tests/api.rs`**

```diff
--- a/crates/dblens-api/tests/api.rs
+++ b/crates/dblens-api/tests/api.rs
@@ -519,3 +519,24 @@ async fn ops_follow_language() {
     assert!(zh[0]["sql"].as_str().unwrap().contains("pg_stat_activity"), "没连接过时按驱动选 PG 的模板");
     assert_eq!(get("en").await[0]["name"], "Sessions");
 }
+
+// ---- 表数据编辑（M2a） ----
+
+#[tokio::test]
+async fn changes_routes_report_errors() {
+    let (app, _) = app();
+    let set = json!({"schema": "public", "table": "t", "changes": [{"kind": "delete", "key": {"id": {"t": "int", "v": 1}}}]});
+    let (st, e) = call(&app, "POST", "/api/profiles/nope/changes/preview", Some(set.clone())).await;
+    assert_eq!((st, e["kind"].clone()), (StatusCode::NOT_FOUND, json!("not_found")));
+    let (st, _) = call(&app, "POST", "/api/sessions/999/changes", Some(set)).await;
+    assert_eq!(st, StatusCode::NOT_FOUND);
+    // 改动的种类不认识：请求体不合法（axum 的拒绝是纯文本，不经过 call 解析 JSON）
+    let bad = json!({"schema": "public", "table": "t", "changes": [{"kind": "upsert"}]});
+    let req = Request::post("/api/profiles/nope/changes/preview")
+        .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
+        .header(header::CONTENT_TYPE, "application/json")
+        .body(Body::from(bad.to_string()))
+        .unwrap();
+    let st = app.clone().oneshot(req).await.unwrap().status();
+    assert!(st.is_client_error(), "{st}");
+}
```

- [ ] **Step 2: 修改 `crates/dblens-api/src/lib.rs`**

```diff
--- a/crates/dblens-api/src/lib.rs
+++ b/crates/dblens-api/src/lib.rs
@@ -81,6 +81,8 @@ pub fn router(service: Arc<Service>, config: ApiConfig) -> Router {
         .route("/profiles/{id}/schemas/{schema}/relations", get(routes::relations))
         .route("/profiles/{id}/schemas/{schema}/data-sql", post(routes::data_sql))
         .route("/profiles/{id}/ops", get(exports::ops))
+        .route("/profiles/{id}/changes/preview", post(routes::preview_changes))
+        .route("/sessions/{id}/changes", post(routes::apply_changes))
         .route("/exports", post(exports::create))
         .route("/exports/{id}", get(exports::status).delete(exports::cancel_or_remove))
         .route("/exports/{id}/file", get(exports::file))
```

- [ ] **Step 3: 修改 `crates/dblens-api/src/routes.rs`**

```diff
--- a/crates/dblens-api/src/routes.rs
+++ b/crates/dblens-api/src/routes.rs
@@ -12,8 +12,9 @@ use dblens_dialect::meta::{MetaKey, ObjectInfo, SchemaInfo};
 use dblens_dialect::{DdlKind, Relation};
 use dblens_driver::DriverId;
 use dblens_service::{
-    DataOrder, DriverStatus, Favorite, FavoritePatch, GenerateKind, HistoryItem, HistoryQuery, NewFavorite,
-    ObjectKind, Profile, ProfileInput, SecretStatus, ServerSummary, SessionId, SessionInfo,
+    ApplyOutcome, ChangeSet, DataOrder, DriverStatus, Favorite, FavoritePatch, GenerateKind, HistoryItem,
+    HistoryQuery, NewFavorite, ObjectKind, Profile, ProfileInput, SecretStatus, ServerSummary, SessionId,
+    SessionInfo,
 };
 use serde::Deserialize;
 use serde_json::{Value, json};
@@ -517,6 +518,27 @@ pub async fn reconnect(State(s): State<AppState>, Path(id): Path<SessionId>) ->
     Ok(Json(s.service.reconnect_session(id).await?))
 }
 
+// ---- 表数据编辑（M2a） ----
+
+/// 预览改动集生成的语句：`[{index, sql}]`，`sql` 是值写成字面量的形式，不执行。
+pub async fn preview_changes(
+    State(s): State<AppState>,
+    Path(id): Path<String>,
+    Json(set): Json<ChangeSet>,
+) -> ApiResult<Json<Vec<Value>>> {
+    let statements = s.service.preview_changes(&id, &set).await?;
+    Ok(Json(statements.into_iter().map(|st| json!({ "index": st.index, "sql": st.preview })).collect()))
+}
+
+/// 在会话中执行改动集。失败时整体回滚，结果中 `failed` 带失败的改动序号和错误（仍是 200）。
+pub async fn apply_changes(
+    State(s): State<AppState>,
+    Path(id): Path<SessionId>,
+    Json(set): Json<ChangeSet>,
+) -> ApiResult<Json<ApplyOutcome>> {
+    Ok(Json(s.service.apply_changes(id, &set).await?))
+}
+
 pub async fn cancel(State(s): State<AppState>, Path(id): Path<SessionId>) -> ApiResult<StatusCode> {
     s.service.sessions.cancel(id).await?;
     Ok(StatusCode::NO_CONTENT)
```

- [ ] **Step 4: 修改 `docs/notes/m0-4b-api.md`**

```diff
--- a/docs/notes/m0-4b-api.md
+++ b/docs/notes/m0-4b-api.md
@@ -139,6 +139,22 @@
 
 每条：`{id, parent_id, kind, name, sql, note, driver, variant, position, created_at, updated_at}`。收藏必须有 SQL，文件夹没有；上级必须是文件夹；不能把文件夹移到它自己或它的下级中（都返回 `invalid`）。
 
+### 表数据编辑（M2a 加入）
+
+| 方法 | 路径 | 说明 |
+|---|---|---|
+| POST | `/profiles/{id}/changes/preview` | 预览改动集生成的语句，返回 `[{index, sql}]`（`sql` 中的值写成字面量），不执行 |
+| POST | `/sessions/{id}/changes` | 在会话中执行改动集，返回 `{applied, failed}`。失败时已整体回滚，`failed` 为 `{index, error}`（仍是 200）；会话正忙时 409 |
+
+- 改动集：`{schema, table, changes: [...]}`，每个改动按 `kind` 区分：
+  - `{"kind": "update", "key": {列: 值}, "original": {列: 原值}, "values": {列: 新值}}`；
+  - `{"kind": "insert", "values": {列: 值}}`；
+  - `{"kind": "delete", "key": {列: 值}}`。
+
+  值的格式同结果集（`{"t": 类型, "v": 值}`）。`key` 必须正好是表的行标识列（主键，没有时为所有列非空的唯一键）。
+- 表没有行标识、引用了不存在的列、写了生成列时 400（`invalid`）。
+- 执行时每条语句必须恰好影响 1 行：为 0 时报"已被修改或删除"，大于 1 时报行标识不唯一。
+
 ### 导出（M1c3 加入）
 
 | 方法 | 路径 | 说明 |
```

- [ ] **Step 5: 检查**

```bash
export CARGO_INCREMENTAL=0
cargo fmt --all
cargo clippy --config "source.mirror.registry='sparse+https://rsproxy.cn/index/'" --workspace --all-targets -- -D warnings
DBLENS_LANG=en cargo test --config "source.mirror.registry='sparse+https://rsproxy.cn/index/'" --workspace
```

Expected: 全部测试通过（接口新增 `changes_routes_report_errors`）。

- [ ] **Step 6: 提交**

```bash
git add crates/dblens-api/src/lib.rs crates/dblens-api/src/routes.rs crates/dblens-api/tests/api.rs docs/notes/m0-4b-api.md
git commit -m "feat(api): 表数据编辑接口：预览改动集、在会话中执行" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 5: 前端：解析与编辑状态

纯逻辑，单独测试：
- `parse.ts`：`parseCell(text, logical)` 按逻辑类型解析（布尔、整数及范围、浮点、精确小数、日期、时间、时间戳及时区、UUID、JSON、其他）；空文本对文本类为空字符串、其他为 NULL；超出 JS 安全整数的整数按精确小数传；`parseTsv` 解析 Excel 复制的制表符分隔文本（含引号）。
- `edit.ts`：`EditState`（修改、新增、删除）；`setCell`（改回原值时取消）、`addRows`、`deleteRows`、`undoRow`、`undoCell`、`changeCount`、`hasErrors`、`paste`（覆盖、新增行、多余列忽略、不能编辑的列跳过）、`buildChanges`（删除、修改、新增的顺序，`rowOf` 对应表格行号）。
- 接口客户端：`RowChange`、`ChangeSet`、`ApplyOutcome`；`previewChanges`、`applyChanges`。
- 词条 `editor.data.edit.bad`、`outOfRange`、`kind.*`。

**Files:**
- Create: `web/src/features/data/edit.test.ts`
- Create: `web/src/features/data/edit.ts`
- Create: `web/src/features/data/parse.test.ts`
- Create: `web/src/features/data/parse.ts`
- Modify: `web/src/locales/en/editor.json`
- Modify: `web/src/locales/zh-CN/editor.json`
- Modify: `web/src/shared/api.ts`

**Interfaces:**
- Produces：
  - `features/data/parse.ts`：`Logical { kind, bits?, tz? }`、`Parsed`、`editableKind(logical)`、`parseCell(text, logical)`、`parseTsv(text)`；
  - `features/data/edit.ts`：`Pending`、`EditState`、`emptyEdit`、`sameCell`、`setCell`、`addRows`、`deleteRows`、`undoRow`、`undoCell`、`changeCount`、`hasErrors`、`PasteColumn`、`paste`、`buildChanges`；
  - `shared/api.ts`：`RowChange`、`ChangeSet`、`ApplyOutcome`、`api.previewChanges(id, set)`、`api.applyChanges(sessionId, set)`。

- [ ] **Step 1: 新建 `web/src/features/data/edit.test.ts`**

```ts
import { describe, expect, it } from 'vitest'

import type { CellValue } from '../../shared/exec'
import {
  type PasteColumn,
  addRows,
  buildChanges,
  changeCount,
  deleteRows,
  emptyEdit,
  hasErrors,
  paste,
  setCell,
  undoCell,
  undoRow,
} from './edit'

const str = (v: string): CellValue => ({ t: 'string', v })
const int = (v: number): CellValue => ({ t: 'int', v })
// 两行已加载的数据：id、name
const rows: CellValue[][] = [
  [int(1), str('甲')],
  [int(2), str('乙')],
]
const columns = [{ name: 'id' }, { name: 'name' }]

describe('编辑状态', () => {
  it('修改：改回原值时取消；删除了的行不能改', () => {
    let s = setCell(emptyEdit, 2, 0, 1, { value: str('丙') }, rows[0][1])
    expect(s.updates).toEqual({ 0: { 1: { value: str('丙') } } })
    s = setCell(s, 2, 0, 1, { value: str('甲') }, rows[0][1])
    expect(s.updates).toEqual({})
    s = deleteRows(s, 2, [1])
    expect(setCell(s, 2, 1, 1, { value: str('x') }, rows[1][1])).toBe(s)
  })

  it('新增、复制、删除新增行、撤销', () => {
    let s = addRows(emptyEdit)
    s = setCell(s, 2, 2, 1, { value: str('新') })
    s = addRows(s, 1, s.inserts[0])
    expect(s.inserts).toEqual([{ 1: { value: str('新') } }, { 1: { value: str('新') } }])
    s = deleteRows(s, 2, [3, 0])
    expect(s.inserts.length).toBe(1)
    expect(s.deletes).toEqual([0])
    expect(changeCount(s)).toBe(2)
    s = undoRow(s, 2, 0)
    s = undoCell(s, 2, 2, 1)
    expect(s).toEqual({ updates: {}, inserts: [{}], deletes: [] })
  })

  it('解析失败的单元格阻止提交', () => {
    const s = setCell(emptyEdit, 2, 0, 0, { error: '不是整数', text: 'x' }, rows[0][0])
    expect(hasErrors(s)).toBe(true)
    expect(hasErrors(undoCell(s, 2, 0, 0))).toBe(false)
  })
})

describe('粘贴', () => {
  const cols: PasteColumn[] = [
    { editable: false, parse: (t) => ({ value: int(Number(t)) }) },
    { editable: true, parse: (t) => ({ value: str(t) }) },
  ]

  it('覆盖已加载的行，超出部分新增行，多出的列忽略，不能编辑的列跳过', () => {
    const { state, ignoredColumns } = paste(emptyEdit, 2, rows, { row: 1, col: 0 }, [
      ['9', '乙改', '多余', '多余'],
      ['9', '新行'],
    ], cols)
    expect(ignoredColumns).toBe(2)
    expect(state.updates).toEqual({ 1: { 1: { value: str('乙改') } } })
    expect(state.inserts).toEqual([{ 1: { value: str('新行') } }])
  })
})

describe('转成改动集', () => {
  it('先删除，再修改，最后新增；修改带行标识和改动列的原值', () => {
    let s = setCell(emptyEdit, 2, 0, 1, { value: str('丙') }, rows[0][1])
    s = deleteRows(s, 2, [1])
    s = addRows(s)
    s = setCell(s, 2, 2, 1, { value: str('新') })
    const { changes, rowOf } = buildChanges(s, 2, rows, columns, [0])
    expect(changes).toEqual([
      { kind: 'delete', key: { id: int(2) } },
      { kind: 'update', key: { id: int(1) }, original: { name: str('甲') }, values: { name: str('丙') } },
      { kind: 'insert', values: { name: str('新') } },
    ])
    expect(rowOf).toEqual([1, 0, 2])
  })
})
```

- [ ] **Step 2: 新建 `web/src/features/data/parse.test.ts`**

```ts
import { describe, expect, it } from 'vitest'

import { editableKind, parseCell, parseTsv } from './parse'

const v = (text: string, kind: string, extra = {}) => parseCell(text, { kind, ...extra })

describe('按类型解析单元格', () => {
  it('空文本：文本类为空字符串，其他为 NULL', () => {
    expect(v('', 'var_char')).toEqual({ value: { t: 'string', v: '' } })
    expect(v('', 'int', { bits: 32 })).toEqual({ value: { t: 'null' } })
    expect(v('', 'date')).toEqual({ value: { t: 'null' } })
  })

  it('布尔、整数（范围）、浮点、精确小数', () => {
    expect(v('TRUE', 'bool')).toEqual({ value: { t: 'bool', v: true } })
    expect(v('0', 'bool')).toEqual({ value: { t: 'bool', v: false } })
    expect(v('是', 'bool')).toHaveProperty('error')
    expect(v(' 42 ', 'int', { bits: 32 })).toEqual({ value: { t: 'int', v: 42 } })
    expect(v('32768', 'int', { bits: 16 })).toHaveProperty('error')
    expect(v('-1', 'u_int', { bits: 8 })).toHaveProperty('error')
    expect(v('9223372036854775807', 'int', { bits: 64 })).toEqual({ value: { t: 'decimal', v: '9223372036854775807' } })
    expect(v('1.5', 'int', { bits: 32 })).toHaveProperty('error')
    expect(v('1e3', 'float', { bits: 64 })).toEqual({ value: { t: 'float', v: 1000 } })
    expect(v('12.30', 'decimal')).toEqual({ value: { t: 'decimal', v: '12.30' } })
    expect(v('12,30', 'decimal')).toHaveProperty('error')
  })

  it('日期、时间、时间戳（带时区时补上时区）', () => {
    expect(v('2026/1/5', 'date')).toEqual({ value: { t: 'date', v: '2026-01-05' } })
    expect(v('2026-02-30', 'date')).toHaveProperty('error')
    expect(v('9:05', 'time')).toEqual({ value: { t: 'time', v: '09:05:00' } })
    expect(v('25:00', 'time')).toHaveProperty('error')
    expect(v('2026-10-26 13:45:30.5', 'timestamp', { tz: false })).toEqual({
      value: { t: 'date_time', v: '2026-10-26T13:45:30.5' },
    })
    expect(v('2026-10-26T13:45 +0800', 'timestamp', { tz: true })).toEqual({
      value: { t: 'date_time_tz', v: '2026-10-26T13:45:00+08:00' },
    })
    const local = v('2026-10-26 13:45', 'timestamp', { tz: true })
    expect('value' in local && String(local.value.v)).toMatch(/^2026-10-26T13:45:00[+-]\d{2}:\d{2}$/)
  })

  it('UUID、JSON、其他按原文', () => {
    expect(v('3F2504E0-4F89-11D3-9A0C-0305E82C3301', 'uuid')).toEqual({
      value: { t: 'uuid', v: '3f2504e0-4f89-11d3-9a0c-0305e82c3301' },
    })
    expect(v('{"a": 1}', 'json')).toEqual({ value: { t: 'json', v: { a: 1 } } })
    expect(v('{a', 'json')).toHaveProperty('error')
    expect(v('中文 ', 'text')).toEqual({ value: { t: 'string', v: '中文 ' } })
    expect(editableKind({ kind: 'blob' })).toBe(false)
    expect(editableKind({ kind: 'text' })).toBe(true)
  })
})

describe('解析制表符分隔的文本', () => {
  it('行、列，末尾空行忽略，Windows 换行', () => {
    expect(parseTsv('a\tb\r\n1\t2\r\n')).toEqual([
      ['a', 'b'],
      ['1', '2'],
    ])
    expect(parseTsv('only')).toEqual([['only']])
    expect(parseTsv('a\t\tc')).toEqual([['a', '', 'c']])
  })

  it('引号包住的单元格可以含换行、制表符和引号', () => {
    expect(parseTsv('"多\n行"\t"x""y"\t"a\tb"\n2\t3\t4')).toEqual([
      ['多\n行', 'x"y', 'a\tb'],
      ['2', '3', '4'],
    ])
  })
})
```

- [ ] **Step 3: 新建 `web/src/features/data/edit.ts`**

```ts
// 数据标签的编辑状态（M2a 设计文档 6.1、6.2 节）：修改、新增、删除都先暂存在这里，提交时转成改动集。纯函数，单独测试。
// 表格的行：先是已加载的行（0 … loaded-1），后面接新增的行（loaded + i）。

import type { RowChange } from '../../shared/api'
import type { CellValue } from '../../shared/exec'

/** 暂存的单元格：解析成功的值，或解析失败的原文和原因。 */
export type Pending = { value: CellValue } | { error: string; text: string }

export interface EditState {
  /** 已加载行的修改：行号 → 列号 → 新值。 */
  updates: Record<number, Record<number, Pending>>
  /** 新增的行，按列号存；没填的列不写（交给数据库的默认值）。 */
  inserts: Record<number, Pending>[]
  /** 删除的已加载行号，升序。 */
  deletes: number[]
}

export const emptyEdit: EditState = { updates: {}, inserts: [], deletes: [] }

export function sameCell(a: CellValue, b: CellValue): boolean {
  return a.t === b.t && JSON.stringify(a.v) === JSON.stringify(b.v)
}

/** 设置一个单元格。已加载的行改回原值时取消这个改动；删除了的行不能改。 */
export function setCell(s: EditState, loaded: number, row: number, col: number, cell: Pending, original?: CellValue): EditState {
  if (row >= loaded) {
    const i = row - loaded
    if (i >= s.inserts.length) return s
    const inserts = s.inserts.slice()
    inserts[i] = { ...inserts[i], [col]: cell }
    return { ...s, inserts }
  }
  if (s.deletes.includes(row)) return s
  const cols = { ...s.updates[row] }
  if (original && 'value' in cell && sameCell(cell.value, original)) delete cols[col]
  else cols[col] = cell
  const updates = { ...s.updates }
  if (Object.keys(cols).length) updates[row] = cols
  else delete updates[row]
  return { ...s, updates }
}

/** 末尾新增 `count` 行；给出 `template` 时每行以它为底稿（复制行）。 */
export function addRows(s: EditState, count = 1, template: Record<number, Pending> = {}): EditState {
  return { ...s, inserts: [...s.inserts, ...Array.from({ length: count }, () => ({ ...template }))] }
}

/** 删除行：已加载的行记为删除（丢掉它的修改），新增的行直接移除。 */
export function deleteRows(s: EditState, loaded: number, rows: number[]): EditState {
  const loadedRows = rows.filter((r) => r < loaded)
  const removed = new Set(rows.filter((r) => r >= loaded).map((r) => r - loaded))
  const updates = { ...s.updates }
  for (const r of loadedRows) delete updates[r]
  return {
    updates,
    inserts: s.inserts.filter((_, i) => !removed.has(i)),
    deletes: [...new Set([...s.deletes, ...loadedRows])].sort((a, b) => a - b),
  }
}

/** 撤销一行的全部改动：修改、删除取消，新增的行移除。 */
export function undoRow(s: EditState, loaded: number, row: number): EditState {
  if (row >= loaded) return { ...s, inserts: s.inserts.filter((_, i) => i !== row - loaded) }
  const updates = { ...s.updates }
  delete updates[row]
  return { ...s, updates, deletes: s.deletes.filter((r) => r !== row) }
}

/** 撤销一个单元格的修改（新增行的单元格清空）。 */
export function undoCell(s: EditState, loaded: number, row: number, col: number): EditState {
  if (row >= loaded) {
    const i = row - loaded
    if (i >= s.inserts.length) return s
    const inserts = s.inserts.slice()
    const { [col]: _, ...rest } = inserts[i]
    inserts[i] = rest
    return { ...s, inserts }
  }
  const cols = { ...s.updates[row] }
  delete cols[col]
  const updates = { ...s.updates }
  if (Object.keys(cols).length) updates[row] = cols
  else delete updates[row]
  return { ...s, updates }
}

/** 改动数：修改的行、新增的行、删除的行各算一个。 */
export function changeCount(s: EditState): number {
  return Object.keys(s.updates).length + s.inserts.length + s.deletes.length
}

/** 有解析失败的单元格时不能提交。 */
export function hasErrors(s: EditState): boolean {
  const bad = (cells: Record<number, Pending>) => Object.values(cells).some((c) => 'error' in c)
  return Object.values(s.updates).some(bad) || s.inserts.some(bad)
}

/** 粘贴时每列的信息：能否编辑、怎样解析。 */
export interface PasteColumn {
  editable: boolean
  parse(text: string): Pending
}

/**
 * 从 `anchor` 开始向右下覆盖：覆盖已加载的行算修改（删除了的行跳过），超出已加载和已新增的行时新增行。
 * 超出表格列数的列忽略，返回忽略的列数；不能编辑的列跳过。
 */
export function paste(
  s: EditState,
  loaded: number,
  rows: CellValue[][],
  anchor: { row: number; col: number },
  matrix: string[][],
  columns: PasteColumn[],
): { state: EditState; ignoredColumns: number } {
  let state = s
  let ignored = 0
  matrix.forEach((cells, r) => {
    const row = anchor.row + r
    // 需要时补新增行
    const need = row - loaded - state.inserts.length + 1
    if (row >= loaded && need > 0) state = addRows(state, need)
    cells.forEach((text, c) => {
      const col = anchor.col + c
      if (col >= columns.length) {
        ignored = Math.max(ignored, cells.length - c)
        return
      }
      if (!columns[col].editable) return
      state = setCell(state, loaded, row, col, columns[col].parse(text), row < loaded ? rows[row][col] : undefined)
    })
  })
  return { state, ignoredColumns: ignored }
}

/**
 * 转成提交给后端的改动集。`keyCols` 是行标识列的列号。
 * `rowOf[i]` 是第 i 个改动在表格中的行号，执行失败时据此标出那一行。调用前先用 `hasErrors` 检查。
 */
export function buildChanges(
  s: EditState,
  loaded: number,
  rows: CellValue[][],
  columns: { name: string }[],
  keyCols: number[],
): { changes: RowChange[]; rowOf: number[] } {
  const changes: RowChange[] = []
  const rowOf: number[] = []
  const key = (row: number) => Object.fromEntries(keyCols.map((c) => [columns[c].name, rows[row][c]]))
  const value = (p: Pending): CellValue => ('value' in p ? p.value : { t: 'null' })
  for (const row of s.deletes) {
    changes.push({ kind: 'delete', key: key(row) })
    rowOf.push(row)
  }
  for (const [r, cells] of Object.entries(s.updates)) {
    const row = Number(r)
    const cols = Object.keys(cells).map(Number)
    changes.push({
      kind: 'update',
      key: key(row),
      original: Object.fromEntries(cols.map((c) => [columns[c].name, rows[row][c]])),
      values: Object.fromEntries(cols.map((c) => [columns[c].name, value(cells[c])])),
    })
    rowOf.push(row)
  }
  s.inserts.forEach((cells, i) => {
    changes.push({
      kind: 'insert',
      values: Object.fromEntries(Object.entries(cells).map(([c, p]) => [columns[Number(c)].name, value(p)])),
    })
    rowOf.push(loaded + i)
  })
  return { changes, rowOf }
}
```

- [ ] **Step 4: 新建 `web/src/features/data/parse.ts`**

```ts
// 表数据编辑的文本解析（M2a 设计文档 6.3 节）：编辑框输入和粘贴的文本按列的逻辑类型转成单元格值。
// 值的格式与结果集相同（{t, v}），提交时原样交给后端绑定参数。

import type { CellValue } from '../../shared/exec'
import { t } from '../../shared/i18n'

/** 列的逻辑类型（dblens_core::LogicalType 的 JSON，按 kind 区分）。 */
export interface Logical {
  kind: string
  bits?: number
  tz?: boolean
}

export type Parsed = { value: CellValue } | { error: string }

const TEXT_KINDS = new Set(['char', 'var_char', 'text'])

/** 不能在表格里编辑的类型：二进制和大字段。 */
export function editableKind(logical: Logical): boolean {
  return !['binary', 'var_binary', 'blob'].includes(logical.kind)
}

const pad = (n: number | string, w = 2) => String(n).padStart(w, '0')

function date(text: string): string | null {
  const m = /^(\d{4})[-/](\d{1,2})[-/](\d{1,2})$/.exec(text)
  if (!m) return null
  const [y, mo, d] = [Number(m[1]), Number(m[2]), Number(m[3])]
  const dt = new Date(Date.UTC(y, mo - 1, d))
  if (dt.getUTCFullYear() !== y || dt.getUTCMonth() !== mo - 1 || dt.getUTCDate() !== d) return null
  return `${m[1]}-${pad(mo)}-${pad(d)}`
}

function time(text: string): string | null {
  const m = /^(\d{1,2}):(\d{2})(?::(\d{2})(\.\d{1,9})?)?$/.exec(text)
  if (!m) return null
  const [h, mi, s] = [Number(m[1]), Number(m[2]), Number(m[3] ?? 0)]
  if (h > 23 || mi > 59 || s > 59) return null
  return `${pad(h)}:${pad(mi)}:${pad(s)}${m[4] ?? ''}`
}

/** 本地时区的偏移，如 `+08:00`。 */
function localOffset(): string {
  const off = -new Date().getTimezoneOffset()
  const sign = off >= 0 ? '+' : '-'
  return `${sign}${pad(Math.floor(Math.abs(off) / 60))}:${pad(Math.abs(off) % 60)}`
}

/** 文本按逻辑类型解析。空文本：文本类为空字符串，其他类型为 NULL。 */
export function parseCell(text: string, logical: Logical): Parsed {
  const s = text.trim()
  const fail = (kind: string) => ({ error: t('editor.data.edit.bad', { kind }) })
  if (text === '') return { value: TEXT_KINDS.has(logical.kind) ? { t: 'string', v: '' } : { t: 'null' } }
  switch (logical.kind) {
    case 'bool': {
      const v = s.toLowerCase()
      if (v === 'true' || v === '1') return { value: { t: 'bool', v: true } }
      if (v === 'false' || v === '0') return { value: { t: 'bool', v: false } }
      return fail(t('editor.data.edit.kind.bool'))
    }
    case 'int':
    case 'u_int': {
      if (!/^[+-]?\d+$/.test(s)) return fail(t('editor.data.edit.kind.int'))
      const n = BigInt(s)
      const bits = BigInt(logical.bits ?? 64)
      const [min, max] =
        logical.kind === 'int' ? [-(1n << (bits - 1n)), (1n << (bits - 1n)) - 1n] : [0n, (1n << bits) - 1n]
      if (n < min || n > max) return { error: t('editor.data.edit.outOfRange') }
      // 超出 JS 安全整数范围的按精确小数传，避免丢精度
      return Number.isSafeInteger(Number(n))
        ? { value: { t: 'int', v: Number(n) } }
        : { value: { t: 'decimal', v: n.toString() } }
    }
    case 'float': {
      const n = Number(s)
      return s !== '' && Number.isFinite(n) ? { value: { t: 'float', v: n } } : fail(t('editor.data.edit.kind.number'))
    }
    case 'decimal':
      return /^[+-]?(\d+(\.\d*)?|\.\d+)([eE][+-]?\d+)?$/.test(s)
        ? { value: { t: 'decimal', v: s } }
        : fail(t('editor.data.edit.kind.number'))
    case 'date': {
      const d = date(s)
      return d ? { value: { t: 'date', v: d } } : fail(t('editor.data.edit.kind.date'))
    }
    case 'time': {
      const v = time(s)
      return v ? { value: { t: 'time', v } } : fail(t('editor.data.edit.kind.time'))
    }
    case 'timestamp': {
      const m = /^(\S+)[ T](\d{1,2}:\d{2}(?::\d{2}(?:\.\d{1,9})?)?)\s*(Z|[+-]\d{2}:?\d{2})?$/.exec(s)
      const d = m && date(m[1])
      const tm = m && time(m[2])
      if (!m || !d || !tm) return fail(t('editor.data.edit.kind.timestamp'))
      if (!logical.tz) return { value: { t: 'date_time', v: `${d}T${tm}` } }
      // 带时区的类型：没写时区时按本地时区
      let off = m[3] ?? localOffset()
      if (off === 'Z') off = '+00:00'
      else if (!off.includes(':')) off = `${off.slice(0, 3)}:${off.slice(3)}`
      return { value: { t: 'date_time_tz', v: `${d}T${tm}${off}` } }
    }
    case 'uuid':
      return /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i.test(s)
        ? { value: { t: 'uuid', v: s.toLowerCase() } }
        : fail('UUID')
    case 'json':
      try {
        return { value: { t: 'json', v: JSON.parse(text) } }
      } catch {
        return fail('JSON')
      }
    case 'interval':
      return { value: { t: 'interval', v: s } }
    default:
      return { value: { t: 'string', v: text } }
  }
}

/**
 * 解析从 Excel 等表格复制的制表符分隔文本：行之间是换行，单元格之间是制表符；
 * 含制表符、换行或引号的单元格用双引号包住，里面的引号写成两个。末尾的空行忽略。
 */
export function parseTsv(text: string): string[][] {
  const rows: string[][] = []
  let row: string[] = []
  let cell = ''
  let i = 0
  let quoted = false
  while (i < text.length) {
    const c = text[i]
    if (quoted) {
      if (c === '"' && text[i + 1] === '"') {
        cell += '"'
        i += 2
        continue
      }
      if (c === '"') quoted = false
      else cell += c
      i += 1
      continue
    }
    if (c === '"' && cell === '') quoted = true
    else if (c === '\t') {
      row.push(cell)
      cell = ''
    } else if (c === '\n' || c === '\r') {
      row.push(cell)
      rows.push(row)
      row = []
      cell = ''
      if (c === '\r' && text[i + 1] === '\n') i += 1
    } else cell += c
    i += 1
  }
  if (cell !== '' || row.length) {
    row.push(cell)
    rows.push(row)
  }
  return rows
}
```

- [ ] **Step 5: 修改 `web/src/locales/en/editor.json`**

```diff
--- a/web/src/locales/en/editor.json
+++ b/web/src/locales/en/editor.json
@@ -69,6 +69,18 @@
     "count": "Count Rows",
     "total": "{{count}} rows in total",
     "loaded": "{{count}} rows loaded",
-    "sortHint": "Click a column header to sort: ascending, descending, none"
+    "sortHint": "Click a column header to sort: ascending, descending, none",
+    "edit": {
+      "bad": "Not a valid {{kind}}",
+      "outOfRange": "Out of range for this column",
+      "kind": {
+        "bool": "boolean (true / false / 1 / 0)",
+        "int": "integer",
+        "number": "number",
+        "date": "date (e.g. 2026-10-26)",
+        "time": "time (e.g. 13:45:30)",
+        "timestamp": "timestamp (e.g. 2026-10-26 13:45:30)"
+      }
+    }
   }
 }
```

- [ ] **Step 6: 修改 `web/src/locales/zh-CN/editor.json`**

```diff
--- a/web/src/locales/zh-CN/editor.json
+++ b/web/src/locales/zh-CN/editor.json
@@ -69,6 +69,18 @@
     "count": "统计行数",
     "total": "共 {{count}} 行",
     "loaded": "已读取 {{count}} 行",
-    "sortHint": "点列头排序：升序、降序、不排序"
+    "sortHint": "点列头排序：升序、降序、不排序",
+    "edit": {
+      "bad": "不是有效的{{kind}}",
+      "outOfRange": "超出这一列的取值范围",
+      "kind": {
+        "bool": "布尔值（true / false / 1 / 0）",
+        "int": "整数",
+        "number": "数字",
+        "date": "日期（如 2026-10-26）",
+        "time": "时间（如 13:45:30）",
+        "timestamp": "时间戳（如 2026-10-26 13:45:30）"
+      }
+    }
   }
 }
```

- [ ] **Step 7: 修改 `web/src/shared/api.ts`**

```diff
--- a/web/src/shared/api.ts
+++ b/web/src/shared/api.ts
@@ -1,6 +1,7 @@
 // REST 接口客户端。接口说明见 docs/notes/m0-4b-api.md；页面与接口同源，路径都以 /api 开头。
 // 请求都带上当前界面语言（REST 用 Accept-Language，WebSocket 用 lang 参数），后端按它输出错误和提示。
 
+import type { CellValue } from './exec'
 import { currentLang, t } from './i18n'
 
 export type DriverId = 'mysql' | 'postgres' | 'oracle' | 'dameng'
@@ -229,6 +230,24 @@ export interface ExportStatus {
   path: string | null
 }
 
+/** 表数据编辑的一个改动（M2a）；值的格式同结果集。 */
+export type RowChange =
+  | { kind: 'update'; key: Record<string, CellValue>; original: Record<string, CellValue>; values: Record<string, CellValue> }
+  | { kind: 'insert'; values: Record<string, CellValue> }
+  | { kind: 'delete'; key: Record<string, CellValue> }
+
+export interface ChangeSet {
+  schema: string
+  table: string
+  changes: RowChange[]
+}
+
+/** 改动集的执行结果：失败时已整体回滚，`failed.index` 是失败的改动序号。 */
+export interface ApplyOutcome {
+  applied: number
+  failed: { index: number; error: ErrorBody } | null
+}
+
 /** 一条运维查询（M1c3）。 */
 export interface OpsQuery {
   key: string
@@ -411,6 +430,10 @@ export function createApi(token: string, fetchFn: FetchFn = (i, o) => fetch(i, o
       request<Favorite>('PUT', `/favorites/${encodeURIComponent(id)}`, patch),
     deleteFavorite: (id: string) => request<null>('DELETE', `/favorites/${encodeURIComponent(id)}`),
     ops: (id: string) => request<OpsQuery[]>('GET', `${p(id)}/ops`),
+    previewChanges: (id: string, set: ChangeSet) =>
+      request<{ index: number; sql: string }[]>('POST', `${p(id)}/changes/preview`, set),
+    applyChanges: (sessionId: number, set: ChangeSet) =>
+      request<ApplyOutcome>('POST', `/sessions/${sessionId}/changes`, set),
     startExport: async (req: ExportRequest) => (await request<{ id: string }>('POST', '/exports', req)).id,
     exportStatus: (id: string) => request<ExportStatus>('GET', `/exports/${encodeURIComponent(id)}`),
     /** 进行中的导出取消；已结束的删除记录。 */
```

- [ ] **Step 8: 检查**

在 PowerShell 中执行（bash 里 `cd web` 会触发坏掉的 fnm 钩子，`&&` 后面的命令会被跳过）：

```powershell
$env:PATH = "D:\Environments\nvm4w\nodejs;$env:PATH"; Set-Location web; pnpm typecheck; pnpm test; pnpm build
```

Expected: tsc 无输出；35 个测试文件、218 个测试通过；打包成功。

- [ ] **Step 9: 提交**

```bash
git add web/src/features/data/edit.test.ts web/src/features/data/edit.ts web/src/features/data/parse.test.ts web/src/features/data/parse.ts web/src/locales/en/editor.json web/src/locales/zh-CN/editor.json web/src/shared/api.ts
git commit -m "feat(web): 表数据编辑的纯逻辑：按类型解析文本、编辑状态、粘贴区域、生成改动集" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 6: 前端：数据标签的编辑界面

- `editable.ts`：按表的结构模型判断每列能否编辑、行标识列（结果集按名称对应表的列）。
- `editStore.ts`：每个数据标签的编辑状态和提交失败的行；`hasPendingEdits`。
- `EditableGrid.tsx`：已加载的行后接新增的行；改过、新增、删除、解析失败、提交失败用不同底色；双击或回车编辑（NULL 的编辑框为空，没改就关上不算改动）；`onPaste` 按 `paste` 处理；Delete 键不清空；右键菜单（设为 NULL、撤销这个改动、撤销这一行、复制、删除行）；行号旁可多选。
- `ChangePreview.tsx`：预览对话框，生产连接标红；取消 / 复制到新查询标签 / 执行。
- `DataView.tsx`：表的数据标签多一行编辑工具栏（新增行、复制行、删除行、撤销所选、全部放弃、提交（改动数））；视图和没有行标识的表显示只读原因；刷新、改过滤或排序前确认放弃；提交：预览 → `Runner.stop()` → 执行 → 成功后重新读取，失败时标出那一行。
- `ResultsPanel` 增加 `renderResult`；`ResultGrid` 导出 `useGridTheme`、`columnWidth`；`Runner.stop()`（从 `rerun` 拆出）；关闭有未提交编辑的标签前确认。
- 接口类型：`ColumnInfo` 增加 `auto_increment`、`generated`，`TableInfo` 增加 `uniques`。词条 `editor.data.edit.*`。

**Files:**
- Create: `web/src/features/data/ChangePreview.tsx`
- Modify: `web/src/features/data/DataView.tsx`
- Create: `web/src/features/data/EditableGrid.tsx`
- Create: `web/src/features/data/editStore.ts`
- Create: `web/src/features/data/editable.test.ts`
- Create: `web/src/features/data/editable.ts`
- Modify: `web/src/features/editor/actions.tsx`
- Modify: `web/src/features/editor/runner.ts`
- Modify: `web/src/features/results/ResultGrid.tsx`
- Modify: `web/src/features/results/ResultsPanel.tsx`
- Modify: `web/src/locales/en/editor.json`
- Modify: `web/src/locales/zh-CN/editor.json`
- Modify: `web/src/shared/api.ts`

**Interfaces:**
- Consumes：任务 5 的全部纯逻辑和接口客户端。
- Produces：
  - `features/data/editable.ts`：`EditColumn { editable, logical }`、`EditInfo { columns, key }`、`editInfo(resultColumns, table)`；
  - `features/data/editStore.ts`：`useEdits`、`editsOf(tabId)`、`hasPendingEdits(tabId)`；
  - `Runner.stop()`；`ResultsPanel` 的 `renderResult(result, waiting)`。

- [ ] **Step 1: 新建 `web/src/features/data/editable.test.ts`**

```ts
import { describe, expect, it } from 'vitest'

import type { ColumnInfo, TableInfo } from '../../shared/api'
import { editInfo } from './editable'

const col = (name: string, kind: string, extra: Partial<ColumnInfo> = {}): ColumnInfo => ({
  name,
  ordinal: 0,
  data_type: { native: kind, logical: { kind } },
  nullable: true,
  default: null,
  comment: null,
  ...extra,
})

describe('可编辑的列和行标识', () => {
  it('主键；生成列、二进制和结果集中多出的列不能编辑', () => {
    const table: TableInfo = {
      name: 't',
      comment: null,
      columns: [col('id', 'int'), col('name', 'var_char'), col('total', 'decimal', { generated: 'a * b' }), col('pic', 'blob')],
      primary_key: { name: null, columns: ['id'] },
    }
    const info = editInfo(['id', 'name', 'total', 'pic', 'extra'], table)
    expect(info.key).toEqual([0])
    expect(info.columns.map((c) => c.editable)).toEqual([true, true, false, false, false])
    expect(info.columns[1].logical).toEqual({ kind: 'var_char' })
  })

  it('没有主键时用所有列都非空的唯一键；都没有时为 null', () => {
    const table: TableInfo = {
      name: 't',
      comment: null,
      columns: [col('a', 'int'), col('code', 'var_char', { nullable: false }), col('b', 'int')],
      primary_key: null,
      uniques: [
        { name: null, columns: ['b'] },
        { name: null, columns: ['code'] },
      ],
    }
    expect(editInfo(['a', 'code', 'b'], table).key).toEqual([1])
    expect(editInfo(['a', 'b'], table).key).toBeNull()
    expect(editInfo(['a'], { ...table, uniques: [] }).key).toBeNull()
  })
})
```

- [ ] **Step 2: 新建 `web/src/features/data/ChangePreview.tsx`**

```tsx
// 提交前的预览（M2a 设计文档 6.4 节）：列出要执行的语句（值写成字面量），生产连接时顶部标红。
// 按钮：取消 / 复制到新查询标签 / 执行。

import { Alert, Button, Modal } from 'antd'
import { useTranslation } from 'react-i18next'

export interface ChangePreviewProps {
  statements: { index: number; sql: string }[] | null
  prod: boolean
  busy: boolean
  onCancel(): void
  onOpenInTab(): void
  onExecute(): void
}

export default function ChangePreview({ statements, prod, busy, onCancel, onOpenInTab, onExecute }: ChangePreviewProps) {
  const { t } = useTranslation()
  return (
    <Modal
      open={statements !== null}
      width={820}
      title={t('editor.data.edit.previewTitle', { count: statements?.length ?? 0 })}
      onCancel={onCancel}
      footer={[
        <Button key="cancel" onClick={onCancel}>
          {t('editor.data.edit.cancel')}
        </Button>,
        <Button key="tab" onClick={onOpenInTab}>
          {t('editor.data.edit.openInTab')}
        </Button>,
        <Button key="run" type="primary" danger={prod} loading={busy} onClick={onExecute}>
          {t('editor.data.edit.execute')}
        </Button>,
      ]}
    >
      {prod && <Alert type="error" showIcon message={t('editor.data.edit.prodWarning')} style={{ marginBottom: 12 }} />}
      <pre className="confirm-sql" style={{ maxHeight: 420 }}>
        {(statements ?? []).map((s) => `${s.sql};`).join('\n')}
      </pre>
    </Modal>
  )
}
```

- [ ] **Step 3: 修改 `web/src/features/data/DataView.tsx`**

```diff
--- a/web/src/features/data/DataView.tsx
+++ b/web/src/features/data/DataView.tsx
@@ -1,26 +1,45 @@
-// 数据标签（M1c2 设计文档第 5 节）：只读地浏览一张表 / 视图 / 物化视图。
-// 工具栏：连接和 schema（只显示）、WHERE 过滤、刷新、停止、统计行数；表格沿用查询标签的结果面板，点列头排序。
+// 数据标签（M1c2 设计文档第 5 节）：浏览一张表 / 视图 / 物化视图。
+// 工具栏：连接和 schema（只显示）、WHERE 过滤、刷新、停止、统计行数、导出；表格沿用查询标签的结果面板，点列头排序。
 // SELECT 由后端按方言生成（data-sql），经这个标签自己的会话执行（沿用 runner：续取、取消、行数上限）。
+// 表的数据可以编辑（M2a 设计文档第 6 节）：改动暂存在 editStore，提交前预览，经同一个会话在事务里执行。
 
-import { IconDownload, IconPlayerStop, IconRefresh, IconSum } from '@tabler/icons-react'
+import {
+  IconArrowBackUp,
+  IconCopy,
+  IconDownload,
+  IconPlayerStop,
+  IconRefresh,
+  IconRowInsertBottom,
+  IconRowRemove,
+  IconSum,
+  IconTrashX,
+} from '@tabler/icons-react'
+import { useQuery } from '@tanstack/react-query'
 import { Alert, App, Button, Input, Tooltip } from 'antd'
-import { useEffect, useState } from 'react'
+import { useEffect, useMemo, useState } from 'react'
 import { useTranslation } from 'react-i18next'
 
 import { useApi } from '../../app/context'
 import DbIcon from '../../shared/DbIcon'
-import type { Profile } from '../../shared/api'
+import { type ChangeSet, type Profile, errorText } from '../../shared/api'
 import ResultsPanel from '../results/ResultsPanel'
+import { useQueryActions } from '../editor/actions'
 import { runnerFor, useTabRuntime } from '../editor/runner'
 import { useExportDialog } from '../export/dialog'
 import { type DataTarget, type QueryTab, useTabs } from '../editor/tabs'
+import ChangePreview from './ChangePreview'
 import { countRows } from './count'
+import { type Pending, addRows, buildChanges, changeCount, deleteRows, emptyEdit, hasErrors, undoRow } from './edit'
+import EditableGrid from './EditableGrid'
+import { editInfo } from './editable'
+import { useEdits } from './editStore'
 import { nextOrder } from './sort'
 
 export default function DataView({ tab, profiles }: { tab: QueryTab; profiles: Profile[] }) {
   const api = useApi()
   const { t } = useTranslation()
-  const { message } = App.useApp()
+  const { message, modal } = App.useApp()
+  const { openSql } = useQueryActions()
   const runner = runnerFor(api, tab.id)
   const rt = useTabRuntime(tab.id)
   const profile = profiles.find((p) => p.id === tab.profileId)
@@ -29,6 +48,46 @@ export default function DataView({ tab, profiles }: { tab: QueryTab; profiles: P
   const [total, setTotal] = useState<string | null>(null)
   const [counting, setCounting] = useState(false)
 
+  // ---- 编辑（M2a） ----
+  const isTable = data.objectKind === 'table'
+  const { data: table } = useQuery({
+    queryKey: ['table-info', tab.profileId, data.schema, data.object],
+    queryFn: () => api.table(tab.profileId, data.schema, data.object),
+    enabled: isTable && !!profile,
+  })
+  const result = rt.exec.statements[0]?.results[0]
+  const info = useMemo(
+    () => (table && result ? editInfo(result.columns.map((c) => c.name), table) : null),
+    [table, result?.columns],
+  )
+  const readOnly = !isTable || !info?.key
+  const readOnlyReason = !isTable ? t('editor.data.edit.viewReadOnly') : info && !info.key ? t('editor.data.edit.noKey') : null
+  const edit = useEdits((s) => s.tabs[tab.id]) ?? emptyEdit
+  const failedRow = useEdits((s) => s.failed[tab.id] ?? null)
+  const setEdit = (next: typeof edit) => useEdits.getState().set(tab.id, next)
+  const pending = changeCount(edit)
+  const [selectedRows, setSelectedRows] = useState<number[]>([])
+  const [preview, setPreview] = useState<{ set: ChangeSet; rowOf: number[]; sql: { index: number; sql: string }[] } | null>(
+    null,
+  )
+  const [applying, setApplying] = useState(false)
+  const loaded = result?.rows.length ?? 0
+
+  /** 有未提交的修改时先确认放弃，再做 `then`。 */
+  const confirmDiscard = (then: () => void) => {
+    if (!pending) return then()
+    modal.confirm({
+      title: t('editor.data.edit.discardTitle'),
+      content: t('editor.data.edit.discardContent'),
+      okText: t('editor.data.edit.discardOk'),
+      okType: 'danger',
+      onOk: () => {
+        useEdits.getState().clear(tab.id)
+        then()
+      },
+    })
+  }
+
   const load = async (d: DataTarget) => {
     try {
       const sql = await api.dataSql(tab.profileId, d.schema, { object: d.object, filter: d.filter, order: d.order })
@@ -39,12 +98,13 @@ export default function DataView({ tab, profiles }: { tab: QueryTab; profiles: P
   }
 
   /** 改过滤条件或排序：保存到标签，重新读取。 */
-  const apply = (patch: Pick<DataTarget, 'filter' | 'order'>) => {
-    const next = { ...data, ...patch }
-    useTabs.getState().setData(tab.id, patch)
-    if ('filter' in patch) setTotal(null)
-    void load(next)
-  }
+  const apply = (patch: Pick<DataTarget, 'filter' | 'order'>) =>
+    confirmDiscard(() => {
+      const next = { ...data, ...patch }
+      useTabs.getState().setData(tab.id, patch)
+      if ('filter' in patch) setTotal(null)
+      void load(next)
+    })
 
   // 第一次显示时（以及连接从已删除恢复时）读取；切换回来时保留已读取的结果
   useEffect(() => {
@@ -63,7 +123,63 @@ export default function DataView({ tab, profiles }: { tab: QueryTab; profiles: P
     }
   }
 
-  const loaded = rt.exec.statements[0]?.results[0]?.rows.length ?? 0
+  /** 复制行：以第一行选中的行为底稿新增一行，行标识列不复制。 */
+  const copyRow = () => {
+    const row = selectedRows[0]
+    if (row === undefined || !info?.key || !result) return
+    const template: Record<number, Pending> = {}
+    info.columns.forEach((c, col) => {
+      if (!c.editable || info.key!.includes(col)) return
+      const cell = row < loaded ? { value: result.rows[row][col] } : edit.inserts[row - loaded]?.[col]
+      if (cell) template[col] = cell
+    })
+    setEdit(addRows(edit, 1, template))
+  }
+
+  const commit = async () => {
+    if (!info?.key || !result) return
+    if (hasErrors(edit)) {
+      message.error(t('editor.data.edit.hasErrors'))
+      return
+    }
+    const { changes, rowOf } = buildChanges(edit, loaded, result.rows, result.columns, info.key)
+    const set: ChangeSet = { schema: data.schema, table: data.object, changes }
+    try {
+      setPreview({ set, rowOf, sql: await api.previewChanges(tab.profileId, set) })
+    } catch (e) {
+      message.error((e as Error).message)
+    }
+  }
+
+  const execute = async () => {
+    if (!preview) return
+    setApplying(true)
+    try {
+      // 会话可能还在等续取的结果：先停下
+      await runner.stop()
+      const session = rt.session
+      if (!session) {
+        message.error(t('editor.data.edit.noSession'))
+        return
+      }
+      const out = await api.applyChanges(session.id, preview.set)
+      if (out.failed) {
+        useEdits.getState().setFailed(tab.id, preview.rowOf[out.failed.index] ?? null)
+        message.error(errorText(out.failed.error), 8)
+      } else {
+        useEdits.getState().clear(tab.id)
+        message.success(t('editor.data.edit.applied', { count: out.applied }))
+        void load(data)
+      }
+      setPreview(null)
+      void runner.refreshSession()
+    } catch (e) {
+      message.error((e as Error).message)
+    } finally {
+      setApplying(false)
+    }
+  }
+
   return (
     <div className="query">
       <div className="row toolbar">
@@ -86,7 +202,7 @@ export default function DataView({ tab, profiles }: { tab: QueryTab; profiles: P
           onPressEnter={() => apply({ filter: filter.trim() || undefined })}
         />
         <Tooltip title={t('editor.data.refresh')} mouseEnterDelay={0.6}>
-          <Button type="text" size="small" disabled={!profile} onClick={() => void load(data)}>
+          <Button type="text" size="small" disabled={!profile} onClick={() => confirmDiscard(() => void load(data))}>
             <IconRefresh size={16} />
           </Button>
         </Tooltip>
@@ -116,6 +232,47 @@ export default function DataView({ tab, profiles }: { tab: QueryTab; profiles: P
         </Button>
         {total !== null && <span className="muted">{t('editor.data.total', { count: Number(total) })}</span>}
       </div>
+      {isTable && (
+        <div className="row toolbar">
+          {readOnlyReason ? (
+            <span className="muted">{readOnlyReason}</span>
+          ) : (
+            <>
+              <Button size="small" type="text" icon={<IconRowInsertBottom size={16} />} disabled={readOnly} onClick={() => setEdit(addRows(edit))}>
+                {t('editor.data.edit.addRow')}
+              </Button>
+              <Button size="small" type="text" icon={<IconCopy size={16} />} disabled={readOnly || !selectedRows.length} onClick={copyRow}>
+                {t('editor.data.edit.copyRow')}
+              </Button>
+              <Button
+                size="small"
+                type="text"
+                icon={<IconRowRemove size={16} />}
+                disabled={readOnly || !selectedRows.length}
+                onClick={() => setEdit(deleteRows(edit, loaded, selectedRows))}
+              >
+                {t('editor.data.edit.deleteRows')}
+              </Button>
+              <Button
+                size="small"
+                type="text"
+                icon={<IconArrowBackUp size={16} />}
+                disabled={!pending || !selectedRows.length}
+                onClick={() => setEdit([...selectedRows].sort((a, b) => b - a).reduce((s, r) => undoRow(s, loaded, r), edit))}
+              >
+                {t('editor.data.edit.undo')}
+              </Button>
+              <Button size="small" type="text" icon={<IconTrashX size={16} />} disabled={!pending} onClick={() => confirmDiscard(() => {})}>
+                {t('editor.data.edit.discard')}
+              </Button>
+              <span className="grow" />
+              <Button size="small" type="primary" disabled={!pending || readOnly} onClick={() => void commit()}>
+                {t('editor.data.edit.commit', { count: pending })}
+              </Button>
+            </>
+          )}
+        </div>
+      )}
       {!profile && <Alert type="error" banner message={t('editor.query.profileDeleted')} />}
       <div className="grow" style={{ minHeight: 0 }}>
         <ResultsPanel
@@ -123,6 +280,25 @@ export default function DataView({ tab, profiles }: { tab: QueryTab; profiles: P
           onFetchMore={() => runner.fetchMore()}
           sort={data.order ?? null}
           onSort={(column) => apply({ order: nextOrder(data.order, column) })}
+          renderResult={
+            isTable && info
+              ? (r, waiting) => (
+                  <EditableGrid
+                    result={r}
+                    waiting={waiting}
+                    onFetchMore={() => runner.fetchMore()}
+                    sort={data.order ?? null}
+                    onSort={(column) => apply({ order: nextOrder(data.order, column) })}
+                    columns={info.columns}
+                    readOnly={readOnly}
+                    edit={edit}
+                    onEdit={setEdit}
+                    failedRow={failedRow}
+                    onSelectRows={setSelectedRows}
+                  />
+                )
+              : undefined
+          }
         />
       </div>
       <div className="row muted" style={{ padding: '2px 8px', gap: 8, fontSize: 'var(--fs-sm)', borderTop: '1px solid var(--border)' }}>
@@ -130,6 +306,18 @@ export default function DataView({ tab, profiles }: { tab: QueryTab; profiles: P
         <span className="grow" />
         <span>{t('editor.data.sortHint')}</span>
       </div>
+      <ChangePreview
+        statements={preview?.sql ?? null}
+        prod={profile?.environment === 'prod'}
+        busy={applying}
+        onCancel={() => setPreview(null)}
+        onOpenInTab={() => {
+          if (!preview) return
+          openSql(preview.sql.map((s) => `${s.sql};`).join('\n'), tab.profileId)
+          setPreview(null)
+        }}
+        onExecute={() => void execute()}
+      />
     </div>
   )
 }
```

- [ ] **Step 4: 新建 `web/src/features/data/EditableGrid.tsx`**

```tsx
// 数据标签的可编辑表格（M2a 设计文档第 6 节）：在已加载的行后面接新增的行；改过的单元格、新增行、删除行、
// 解析失败的单元格和提交失败的行用不同底色标出。双击或回车编辑，Ctrl+V 粘贴，右键菜单设为 NULL、撤销、删除行。

import '@glideapps/glide-data-grid/dist/index.css'

import {
  CompactSelection,
  DataEditor,
  type GridCell,
  GridCellKind,
  type GridColumn,
  type GridSelection,
  type Item,
  type Rectangle,
} from '@glideapps/glide-data-grid'
import { App, Dropdown } from 'antd'
import { useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { type CellValue, type ResultSet, cellText } from '../../shared/exec'
import { columnWidth, useGridTheme } from '../results/ResultGrid'
import { isNumeric } from '../results/cell'
import { shouldFetchMore } from '../results/results'
import { type EditState, type Pending, deleteRows, paste, setCell, undoCell, undoRow } from './edit'
import type { EditColumn } from './editable'
import { parseCell } from './parse'
import { type Order, sortedTitle } from './sort'

export interface EditableGridProps {
  result: ResultSet
  waiting: boolean
  onFetchMore(): void
  sort?: Order
  onSort?(column: string): void
  /** 结果集每一列能否编辑；整张表只读时 readOnly 为 true。 */
  columns: EditColumn[]
  readOnly: boolean
  edit: EditState
  onEdit(next: EditState): void
  /** 提交失败的行。 */
  failedRow: number | null
  /** 表格中选中的行（删除、复制行用）。 */
  onSelectRows(rows: number[]): void
}

const emptySelection: GridSelection = { columns: CompactSelection.empty(), rows: CompactSelection.empty() }

/** 大字段超过预览长度时只读：编辑会把截断的内容写回去。 */
function truncated(v: CellValue | undefined): boolean {
  return v?.t === 'lob' && (v.v as { truncated?: boolean } | undefined)?.truncated === true
}

export default function EditableGrid(props: EditableGridProps) {
  const { result, waiting, onFetchMore, sort, onSort, columns, readOnly, edit, onEdit, failedRow, onSelectRows } = props
  const { t } = useTranslation()
  const { message } = App.useApp()
  const theme = useGridTheme()
  const [selection, setSelection] = useState<GridSelection>(emptySelection)
  const [menu, setMenu] = useState<{ x: number; y: number; cell: Item } | null>(null)
  const loaded = result.rows.length
  const hasRows = loaded > 0
  const gridColumns: GridColumn[] = useMemo(
    () => result.columns.map((c, i) => ({ id: String(i), title: sortedTitle(c.name, sort), width: columnWidth(result, i) })),
    [result.columns, hasRows, sort],
  )
  const css = (name: string) => getComputedStyle(document.documentElement).getPropertyValue(name).trim()
  const colors = useMemo(
    () => ({
      changed: { bgCell: css('--warn-bg') },
      inserted: { bgCell: css('--ok-bg') },
      error: { bgCell: css('--danger-bg'), textDark: css('--danger') },
      deleted: { bgCell: css('--hover'), textDark: css('--muted') },
      muted: { textDark: theme.textLight },
    }),
    [theme],
  )

  const original = (row: number, col: number): CellValue | undefined => (row < loaded ? result.rows[row][col] : undefined)
  const pendingOf = (row: number, col: number): Pending | undefined =>
    row < loaded ? edit.updates[row]?.[col] : edit.inserts[row - loaded]?.[col]
  const canEdit = (row: number, col: number) =>
    !readOnly && columns[col]?.editable && !edit.deletes.includes(row) && !truncated(original(row, col))

  const getCellContent = ([col, row]: Item): GridCell => {
    const pending = pendingOf(row, col)
    const value = pending && 'value' in pending ? pending.value : (original(row, col) ?? { t: 'null' })
    const text = pending && 'error' in pending ? pending.text : cellText(value)
    const isNull = !(pending && 'error' in pending) && value.t === 'null'
    const override =
      pending && 'error' in pending
        ? colors.error
        : edit.deletes.includes(row)
          ? colors.deleted
          : row === failedRow
            ? colors.error
            : row >= loaded
              ? colors.inserted
              : pending
                ? colors.changed
                : isNull
                  ? colors.muted
                  : undefined
    return {
      kind: GridCellKind.Text,
      // 编辑框里的初始文本：NULL 为空
      data: isNull ? '' : text,
      displayData: text.length > 500 ? `${text.slice(0, 500)}…` : text,
      allowOverlay: canEdit(row, col),
      readonly: !canEdit(row, col),
      contentAlign: isNumeric(value) ? 'right' : 'left',
      themeOverride: override,
    }
  }

  const parse = (col: number, text: string): Pending => {
    const p = parseCell(text, columns[col].logical)
    return 'value' in p ? { value: p.value } : { error: p.error, text }
  }

  const set = (row: number, col: number, cell: Pending) => onEdit(setCell(edit, loaded, row, col, cell, original(row, col)))

  const rowsOf = (sel: GridSelection, fallback: number) => {
    const rows = sel.rows.toArray()
    return rows.length ? rows : [fallback]
  }

  const menuItems = menu
    ? [
        { key: 'null', label: t('editor.data.edit.setNull'), disabled: !canEdit(menu.cell[1], menu.cell[0]) },
        { key: 'undoCell', label: t('editor.data.edit.undoCell'), disabled: !pendingOf(menu.cell[1], menu.cell[0]) },
        { key: 'undoRow', label: t('editor.data.edit.undoRow') },
        { type: 'divider' as const },
        { key: 'copy', label: t('editor.data.edit.copyCell') },
        { key: 'delete', label: t('editor.data.edit.deleteRows'), danger: true, disabled: readOnly },
      ]
    : []

  const onMenu = (key: string) => {
    if (!menu) return
    const [col, row] = menu.cell
    setMenu(null)
    if (key === 'null') set(row, col, { value: { t: 'null' } })
    else if (key === 'undoCell') onEdit(undoCell(edit, loaded, row, col))
    else if (key === 'undoRow') onEdit(undoRow(edit, loaded, row))
    else if (key === 'delete') onEdit(deleteRows(edit, loaded, rowsOf(selection, row)))
    else if (key === 'copy') {
      const cell = getCellContent([col, row])
      void navigator.clipboard.writeText(cell.kind === GridCellKind.Text ? cell.data : '')
    }
  }

  return (
    <>
      <DataEditor
        columns={gridColumns}
        rows={loaded + edit.inserts.length}
        getCellContent={getCellContent}
        getCellsForSelection
        onVisibleRegionChanged={(range: Rectangle) => {
          if (shouldFetchMore(range.y + range.height, loaded, waiting)) onFetchMore()
        }}
        onHeaderClicked={onSort ? (col) => onSort(result.columns[col].name) : undefined}
        onCellEdited={([col, row], cell) => {
          if (cell.kind !== GridCellKind.Text || !canEdit(row, col)) return
          const before = getCellContent([col, row])
          // 打开编辑框后没改就关上：不算改动（NULL 的编辑框是空的，不能当成空字符串写回去）
          if (before.kind === GridCellKind.Text && before.data === cell.data) return
          set(row, col, parse(col, cell.data))
        }}
        onPaste={([col, row], values) => {
          if (readOnly) return false
          const r = paste(
            edit,
            loaded,
            result.rows,
            { row, col },
            values.map((v) => [...v]),
            columns.map((c, i) => ({ editable: c.editable, parse: (text: string) => parse(i, text) })),
          )
          onEdit(r.state)
          if (r.ignoredColumns) message.warning(t('editor.data.edit.ignoredColumns', { count: r.ignoredColumns }))
          return false
        }}
        // Delete 键不清空单元格（删除行用工具栏或右键菜单）
        onDelete={() => false}
        gridSelection={selection}
        onGridSelectionChange={(sel) => {
          setSelection(sel)
          onSelectRows(rowsOf(sel, sel.current?.cell[1] ?? -1).filter((r) => r >= 0))
        }}
        onCellContextMenu={(cell, e) => {
          e.preventDefault()
          setMenu({ x: e.bounds.x + e.localEventX, y: e.bounds.y + e.localEventY, cell })
        }}
        rowMarkers="both"
        smoothScrollX
        smoothScrollY
        rowHeight={28}
        headerHeight={32}
        theme={theme}
        width="100%"
        height="100%"
      />
      <Dropdown
        open={menu !== null}
        onOpenChange={(open) => !open && setMenu(null)}
        menu={{ items: menuItems, onClick: ({ key }) => onMenu(key) }}
        trigger={['contextMenu']}
      >
        <span style={{ position: 'fixed', left: menu?.x ?? 0, top: menu?.y ?? 0, width: 1, height: 1 }} />
      </Dropdown>
    </>
  )
}
```

- [ ] **Step 5: 新建 `web/src/features/data/editStore.ts`**

```ts
// 每个数据标签的编辑状态（不随标签保存）。关闭、刷新等操作前用 hasPendingEdits 判断要不要确认放弃。

import { create } from 'zustand'

import { type EditState, changeCount, emptyEdit } from './edit'

interface EditStore {
  tabs: Record<string, EditState>
  /** 提交失败的行（表格中的行号），标红显示；改动后清除。 */
  failed: Record<string, number | null>
  set(tabId: string, state: EditState): void
  setFailed(tabId: string, row: number | null): void
  clear(tabId: string): void
}

export const useEdits = create<EditStore>()((set, get) => ({
  tabs: {},
  failed: {},
  set: (tabId, state) => set({ tabs: { ...get().tabs, [tabId]: state }, failed: { ...get().failed, [tabId]: null } }),
  setFailed: (tabId, row) => set({ failed: { ...get().failed, [tabId]: row } }),
  clear: (tabId) => {
    const tabs = { ...get().tabs }
    const failed = { ...get().failed }
    delete tabs[tabId]
    delete failed[tabId]
    set({ tabs, failed })
  },
}))

export function editsOf(tabId: string): EditState {
  return useEdits.getState().tabs[tabId] ?? emptyEdit
}

export function hasPendingEdits(tabId: string): boolean {
  return changeCount(editsOf(tabId)) > 0
}
```

- [ ] **Step 6: 新建 `web/src/features/data/editable.ts`**

```ts
// 数据标签中哪些列能编辑、用哪些列标识一行（M2a 设计文档第 3 节）。按表的结构模型判断，纯函数，单独测试。

import type { TableInfo } from '../../shared/api'
import { type Logical, editableKind } from './parse'

export interface EditColumn {
  /** 能否编辑：表中有这一列、不是生成列、不是二进制或大字段类型。 */
  editable: boolean
  logical: Logical
}

export interface EditInfo {
  /** 结果集的每一列，顺序同结果集。 */
  columns: EditColumn[]
  /** 行标识列在结果集中的列号；为 null 时整张表只读。 */
  key: number[] | null
}

/**
 * 行标识：主键；没有主键时取第一个所有列都 NOT NULL 的唯一键。标识列必须都在结果集中。
 * 结果集的列按名称对应表的列（数据标签执行的是 `SELECT *`）。
 */
export function editInfo(resultColumns: string[], table: TableInfo): EditInfo {
  const byName = new Map(table.columns.map((c) => [c.name, c]))
  const columns = resultColumns.map((name) => {
    const c = byName.get(name)
    const logical = (c?.data_type.logical ?? { kind: 'other' }) as Logical
    return { editable: !!c && !c.generated && editableKind(logical), logical }
  })
  const find = (names: string[]) => {
    const idx = names.map((n) => resultColumns.indexOf(n))
    return names.length && idx.every((i) => i >= 0) ? idx : null
  }
  let key = table.primary_key ? find(table.primary_key.columns) : null
  if (!key) {
    for (const u of table.uniques ?? []) {
      if (u.columns.every((n) => byName.get(n)?.nullable === false)) {
        key = find(u.columns)
        if (key) break
      }
    }
  }
  return { columns, key }
}
```

- [ ] **Step 7: 修改 `web/src/features/editor/actions.tsx`**

```diff
--- a/web/src/features/editor/actions.tsx
+++ b/web/src/features/editor/actions.tsx
@@ -6,6 +6,7 @@ import { useTranslation } from 'react-i18next'
 
 import { useApi } from '../../app/context'
 import { useUi } from '../../app/ui'
+import { hasPendingEdits, useEdits } from '../data/editStore'
 import { sendInsert } from './commands'
 import { disposeRunner, useRuntime } from './runner'
 import { markRun, useTabs } from './tabs'
@@ -51,12 +52,26 @@ export function useQueryActions() {
     else openSql(sql, profileId)
   }
 
-  /** 关闭标签页：有未提交的事务时先问提交还是回滚。 */
+  /** 关闭标签页：数据标签有未提交的编辑时先确认放弃；有未提交的事务时先问提交还是回滚。 */
   const closeQuery = (tabId: string) => {
     const tab = useTabs.getState().tabs.find((x) => x.id === tabId)
     if (!tab) return
+    if (tab.data && hasPendingEdits(tabId)) {
+      modal.confirm({
+        title: t('editor.data.edit.discardTitle'),
+        content: t('editor.data.edit.discardContent'),
+        okText: t('editor.data.edit.discardOk'),
+        okType: 'danger',
+        onOk: () => {
+          useEdits.getState().clear(tabId)
+          closeQuery(tabId)
+        },
+      })
+      return
+    }
     const finish = async (commit: boolean) => {
       await disposeRunner(tabId, commit)
+      useEdits.getState().clear(tabId)
       useTabs.getState().close(tabId)
     }
     if (!useRuntime.getState().tabs[tabId]?.session?.in_transaction) {
```

- [ ] **Step 8: 修改 `web/src/features/editor/runner.ts`**

```diff
--- a/web/src/features/editor/runner.ts
+++ b/web/src/features/editor/runner.ts
@@ -185,6 +185,12 @@ export class Runner {
    * 首批结果读完后服务端在等续取，执行状态一直是 running，run 会忽略新的提交。
    */
   async rerun(sql: string) {
+    await this.stop()
+    await this.run(sql, 0)
+  }
+
+  /** 停下正在执行的（包括还在等续取的结果），等它结束。数据标签提交编辑前也要先停下（M2a）。 */
+  async stop() {
     if (this.rt().running) {
       const stopped = new Promise<void>((resolve) => {
         const off = useRuntime.subscribe((s) => {
@@ -197,7 +203,6 @@ export class Runner {
       this.cancel()
       await stopped
     }
-    await this.run(sql, 0)
   }
 
   /** 用户确认后，从需要确认的那条语句接着执行。 */
```

- [ ] **Step 9: 修改 `web/src/features/results/ResultGrid.tsx`**

```diff
--- a/web/src/features/results/ResultGrid.tsx
+++ b/web/src/features/results/ResultGrid.tsx
@@ -24,14 +24,14 @@ import { isNumeric } from './cell'
 import { shouldFetchMore } from './results'
 
 /** 按列名和前几行的内容估计列宽。 */
-function columnWidth(r: ResultSet, col: number): number {
+export function columnWidth(r: ResultSet, col: number): number {
   let chars = r.columns[col].name.length + 2
   for (const row of r.rows.slice(0, 50)) chars = Math.max(chars, cellText(row[col]).length)
   return Math.min(420, Math.max(72, chars * 8 + 24))
 }
 
 /** 表格配色取自外壳的 CSS 变量，主题切换时重新读取。 */
-function useGridTheme(): Partial<Theme> {
+export function useGridTheme(): Partial<Theme> {
   const pref = useSetting(THEME_KEY)
   const dark = document.documentElement.dataset.theme
   return useMemo(() => {
```

- [ ] **Step 10: 修改 `web/src/features/results/ResultsPanel.tsx`**

```diff
--- a/web/src/features/results/ResultsPanel.tsx
+++ b/web/src/features/results/ResultsPanel.tsx
@@ -3,10 +3,11 @@
 
 import { IconAlertCircle, IconBinaryTree, IconDownload, IconMessage, IconTable } from '@tabler/icons-react'
 import { Button, Spin } from 'antd'
-import { useEffect, useState } from 'react'
+import { type ReactNode, useEffect, useState } from 'react'
 import { useTranslation } from 'react-i18next'
 
 import { errorText } from '../../shared/api'
+import type { ResultSet } from '../../shared/exec'
 import type { TabRuntime } from '../editor/runner'
 import type { Order } from '../data/sort'
 import ResultGrid from './ResultGrid'
@@ -20,6 +21,8 @@ export interface ResultsPanelProps {
   onSort?(column: string): void
   /** 导出第 `stmt` 条语句的结果；不给时不显示导出按钮。 */
   onExport?(stmt: number): void
+  /** 自己画结果集（数据标签的可编辑表格，M2a）；不给时用只读的结果表格。 */
+  renderResult?(result: ResultSet, waiting: boolean): ReactNode
 }
 
 function TabIcon({ tab, explain }: { tab: ResultTab; explain: boolean }) {
@@ -28,7 +31,7 @@ function TabIcon({ tab, explain }: { tab: ResultTab; explain: boolean }) {
   return explain ? <IconBinaryTree size={16} /> : <IconTable size={16} />
 }
 
-export default function ResultsPanel({ rt, onFetchMore, sort, onSort, onExport }: ResultsPanelProps) {
+export default function ResultsPanel({ rt, onFetchMore, sort, onSort, onExport, renderResult }: ResultsPanelProps) {
   const { t } = useTranslation()
   const { exec, explain, capped, running } = rt
   const tabs = resultTabs(exec, explain, capped)
@@ -72,7 +75,12 @@ export default function ResultsPanel({ rt, onFetchMore, sort, onSort, onExport }
         )}
       </div>
       <div className="results-body">
-        {current.kind === 'result' && (
+        {current.kind === 'result' &&
+          renderResult?.(
+            exec.statements[current.stmt].results[current.result],
+            exec.waitingForMore && current.stmt === lastStmt,
+          )}
+        {current.kind === 'result' && !renderResult && (
           <ResultGrid
             key={current.key}
             result={exec.statements[current.stmt].results[current.result]}
```

- [ ] **Step 11: 修改 `web/src/locales/en/editor.json`**

```diff
--- a/web/src/locales/en/editor.json
+++ b/web/src/locales/en/editor.json
@@ -80,7 +80,34 @@
         "date": "date (e.g. 2026-10-26)",
         "time": "time (e.g. 13:45:30)",
         "timestamp": "timestamp (e.g. 2026-10-26 13:45:30)"
-      }
+      },
+      "addRow": "Add Row",
+      "copyRow": "Copy Row",
+      "deleteRows": "Delete Rows",
+      "undo": "Undo Selected",
+      "discard": "Discard All",
+      "commit": "Commit ({{count}})",
+      "setNull": "Set NULL",
+      "undoCell": "Undo This Change",
+      "undoRow": "Undo This Row",
+      "copyCell": "Copy",
+      "ignoredColumns_one": "The pasted data has {{count}} extra column; it was ignored",
+      "ignoredColumns_other": "The pasted data has {{count}} extra columns; they were ignored",
+      "viewReadOnly": "Views are read-only",
+      "noKey": "Read-only: no primary key or non-null unique key",
+      "hasErrors": "Some cells could not be parsed (shown in red); fix them before committing",
+      "discardTitle": "Discard uncommitted changes?",
+      "discardContent": "Changes here have not been committed and will be lost.",
+      "discardOk": "Discard Changes",
+      "previewTitle_one": "Commit changes: {{count}} statement",
+      "previewTitle_other": "Commit changes: {{count}} statements",
+      "prodWarning": "This is a production connection; check these statements",
+      "openInTab": "Copy to New Query Tab",
+      "execute": "Execute",
+      "cancel": "Cancel",
+      "applied_one": "{{count}} statement committed",
+      "applied_other": "{{count}} statements committed",
+      "noSession": "The data has not been read yet; refresh first"
     }
   }
 }
```

- [ ] **Step 12: 修改 `web/src/locales/zh-CN/editor.json`**

```diff
--- a/web/src/locales/zh-CN/editor.json
+++ b/web/src/locales/zh-CN/editor.json
@@ -80,7 +80,31 @@
         "date": "日期（如 2026-10-26）",
         "time": "时间（如 13:45:30）",
         "timestamp": "时间戳（如 2026-10-26 13:45:30）"
-      }
+      },
+      "addRow": "新增行",
+      "copyRow": "复制行",
+      "deleteRows": "删除行",
+      "undo": "撤销所选",
+      "discard": "全部放弃",
+      "commit": "提交（{{count}}）",
+      "setNull": "设为 NULL",
+      "undoCell": "撤销这个改动",
+      "undoRow": "撤销这一行",
+      "copyCell": "复制",
+      "ignoredColumns": "粘贴的内容多出 {{count}} 列，已忽略",
+      "viewReadOnly": "视图只读",
+      "noKey": "没有主键或非空的唯一键，只读",
+      "hasErrors": "有无法解析的单元格（标红），请改正后再提交",
+      "discardTitle": "放弃未提交的修改？",
+      "discardContent": "这里的修改还没有提交，继续会丢掉它们。",
+      "discardOk": "放弃修改",
+      "previewTitle": "提交修改：{{count}} 条语句",
+      "prodWarning": "这是生产环境的连接，请确认这些语句",
+      "openInTab": "复制到新查询标签",
+      "execute": "执行",
+      "cancel": "取消",
+      "applied": "已提交 {{count}} 条语句",
+      "noSession": "数据还没有读取，请先刷新"
     }
   }
 }
```

- [ ] **Step 13: 修改 `web/src/shared/api.ts`**

```diff
--- a/web/src/shared/api.ts
+++ b/web/src/shared/api.ts
@@ -140,6 +140,8 @@ export interface ColumnInfo {
   nullable: boolean
   default: string | null
   comment: string | null
+  auto_increment?: boolean
+  generated?: string | null
 }
 
 /** 表明细（只列出对象树用到的字段）。 */
@@ -148,6 +150,7 @@ export interface TableInfo {
   comment: string | null
   columns: ColumnInfo[]
   primary_key: { name: string | null; columns: string[] } | null
+  uniques?: { name: string | null; columns: string[] }[]
 }
 
 /** 查看 DDL 的对象种类（M1c2）。 */
```

- [ ] **Step 14: 检查**

在 PowerShell 中执行（bash 里 `cd web` 会触发坏掉的 fnm 钩子，`&&` 后面的命令会被跳过）：

```powershell
$env:PATH = "D:\Environments\nvm4w\nodejs;$env:PATH"; Set-Location web; pnpm typecheck; pnpm test; pnpm build
```

Expected: tsc 无输出；36 个测试文件、220 个测试通过；打包成功。

- [ ] **Step 15: 提交**

```bash
git add web/src/features/data/ChangePreview.tsx web/src/features/data/DataView.tsx web/src/features/data/EditableGrid.tsx web/src/features/data/editStore.ts web/src/features/data/editable.test.ts web/src/features/data/editable.ts web/src/features/editor/actions.tsx web/src/features/editor/runner.ts web/src/features/results/ResultGrid.tsx web/src/features/results/ResultsPanel.tsx web/src/locales/en/editor.json web/src/locales/zh-CN/editor.json web/src/shared/api.ts
git commit -m "feat(web): 数据标签编辑：可编辑表格（改单元格、新增、复制、删除行、粘贴、右键菜单）、提交前预览、冲突时标出失败的行" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 7: 说明文档与实测

写 M2a 说明文档，总设计文档记录 M2 的拆分和进度。然后用浏览器模式实测。若任务 1 的复查有升级，在说明文档中加"依赖"一节。

**Files:**
- Create: `docs/notes/m2a-data-editing.md`
- Modify: `docs/superpowers/specs/2026-10-22-dblens-design.md`

- [ ] **Step 1: 新建 `docs/notes/m2a-data-editing.md`**

````markdown
# M2a 表数据编辑

依据：设计文档 `docs/superpowers/specs/2026-10-26-m2a-data-editing-design.md`。M2 的第一份（M2b 导入在后）。

## 结构

```text
crates/dblens-dialect/src/changes.rs   改动集（RowChange）、行标识（row_key）、按改动生成语句（执行用占位符、预览用字面量）
crates/dblens-service/src/
  session.rs     Command::Apply：在会话中按事务执行，检查影响行数，失败整体回滚；ApplyOutcome / ApplyFailure
  changes.rs     ChangeSet；Service::preview_changes / apply_changes（取表的结构模型、生成语句）
crates/dblens-api/src/routes.rs        /profiles/{id}/changes/preview、/sessions/{id}/changes
web/src/features/data/
  parse.ts       按逻辑类型解析文本；解析制表符分隔的文本
  edit.ts        编辑状态（修改、新增、删除）、粘贴区域、生成改动集
  editable.ts    可编辑的列、行标识
  editStore.ts   每个数据标签的编辑状态
  EditableGrid.tsx   可编辑表格；ChangePreview.tsx 提交前的预览
  DataView.tsx   编辑工具栏、放弃确认、提交流程
web/src/features/editor/{runner.ts, actions.tsx}   Runner.stop；关闭有未提交编辑的标签前确认
```

## 生成与执行

- 行标识：主键；没有主键时取第一个所有列都 NOT NULL 的唯一键；都没有时整张表只读。
- UPDATE 的 WHERE：行标识列 = 原值，加上改动列 = 原值（NULL 用 `IS NULL`）。不能可靠比较相等的列不参与：逻辑类型为 Float、Text、Blob、Binary、VarBinary、Json、Other，以及原值是大字段预览的。
- INSERT：自增列和有默认值的列为 NULL 时不写；一列都不写时用各方言"只用默认值"的写法。
- 语句顺序：先删除，再修改，最后新增。
- PG 的精确小数：PG 驱动不能直接绑定 NUMERIC 参数，占位符写成 `$n::text::numeric`，值按文本传。这是写计划时实库测试发现的。
- 执行：
  - 会话已在手动事务中时拒绝；
  - 临时关闭自动提交，逐条执行，每条语句先经过执行策略：要求确认的语句直接执行（预览就是确认），拒绝的语句算失败；
  - 影响行数必须为 1：为 0 时报"已被修改或删除"，大于 1 时报行标识不唯一；
  - 全部成功才提交，否则回滚；最后恢复自动提交；
  - 每条执行过的语句写一条历史（预览形式的 SQL），回滚时都记为失败。
- 接口：执行失败也是 200，结果中 `failed` 带失败的改动序号和错误。设计文档写的是"失败时返回错误"，改成这样是为了前端按序号标出失败的行。
- 会话正在读取结果（等续取）时不能执行：前端先调 `Runner.stop()` 停下，再提交。

## 前端

- 表格：已加载的行后面接新增的行。改过的单元格、新增行、删除行、解析失败的单元格和提交失败的行用不同底色。
- 编辑：双击或回车打开编辑框（Glide Data Grid 的浮层，挂在 `#portal`）。NULL 的编辑框是空的；打开后没改就关上不算改动（否则 NULL 会变成空字符串）。
- 粘贴：Glide 解析剪贴板后交给 `onPaste`，按 `edit.ts` 的规则覆盖或新增，返回 false 阻止默认行为。
- Delete 键不清空单元格；删除行用工具栏或右键菜单。
- 复制行：以第一行选中的行为底稿，行标识列不复制。
- 有未提交的编辑时，刷新、改过滤或排序、关闭标签前确认放弃。

## 验证

- 后端：fmt、clippy 通过；全部测试在 `DBLENS_LANG=en` 下对 5 个实库通过（420 个）。
  - 实库测试（5 个库）：建带主键、唯一键、自增列、默认值的表，含中文和 NULL。一次提交新增、修改、删除，核对结果（新增行取到默认值）。另一个会话先改同一字段，再按旧值提交，整体回滚，表中数据不变，会话恢复自动提交。没有主键的表被拒绝。
  - 有一次全量运行中，达梦的导出实库测试失败一次，单独重跑和再跑全量都通过。与 M1c1、M1c3 记录的情况相同：多个测试同时连达梦时偶发失败。
- 前端：tsc、Vitest、打包通过。
- 浏览器实测（PostgreSQL）：
  - 双击或回车编辑单元格，提交按钮显示改动数；
  - 新增行并填写；
  - 预览：UPDATE 带主键和改动列的原值，INSERT 只写填了的列；
  - 执行成功后提示条数，按当前过滤和排序重新读取，编辑状态清空。
- 实测中没有覆盖（内置浏览器在窗口最小化时停止绘制，表格的画布不能交互）：粘贴、右键菜单、删除行、冲突时标红、放弃确认。冲突回滚由实库测试覆盖，粘贴区域和编辑状态有单元测试。这几项在执行计划时人工确认。
````

- [ ] **Step 2: 修改 `docs/superpowers/specs/2026-10-22-dblens-design.md`**

```diff
--- a/docs/superpowers/specs/2026-10-22-dblens-design.md
+++ b/docs/superpowers/specs/2026-10-22-dblens-design.md
@@ -323,6 +323,10 @@ trait Policy: Send + Sync {
 | M5 迁移 | 选择源和目标 → 按逻辑类型映射结构，生成目标 DDL，可人工调整 → 分批迁移数据（断点续传、进度、行数校验） → 最后建索引和约束 | M2 的批量读写、M3 的 DDL 生成、类型映射 |
 | 后续 | 团队管控（多用户、RBAC、SQL 审核、审计、执行前备份）、JDBC 桥、SQLite、SQL Server 等 | 浏览器模式、Policy、Driver 接口 |
 
+进度：
+- 2026-10-26 M2 拆成两份（用户确认）：M2a 表数据编辑；M2b 导入（CSV / Excel 导入到表、SQL 文件导入）和对象树"导出整表"的入口。查询结果和数据标签的导出已在 M1c3 完成。
+- M2a 已完成（设计 `docs/superpowers/specs/2026-10-26-m2a-data-editing-design.md`，详细计划 `docs/superpowers/plans/2026-10-26-m2a-data-editing.md`，说明 `docs/notes/m2a-data-editing.md`）。
+
 ## 12. 风险与待验证项
 
 | 风险 | 应对 |
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

浏览器实测（用新的数据目录启动 `dblens-server --web-dir web/dist`；窗口要保持显示，表格是画布，窗口最小化时不能交互）：
1. 经接口建 5 个测试连接（密码只经环境变量传给建连接的脚本），经 WebSocket 建测试数据（父子表，主键为 id）。
2. 打开 PG 的 `c2_parent` 数据标签：出现编辑工具栏；打开视图的数据标签：显示"视图只读"。
3. 双击（或选中后回车）把一行的 `code` 改掉：单元格变色，"提交（1）"。
4. 新增行，填写 `id` 和 `code`；从 Excel 复制两行三列粘贴到最后一行：第三列忽略并提示，超出的行新增。
5. 右键：设为 NULL、撤销这个改动；选中一行"删除行"：整行变灰。
6. 提交：预览中 UPDATE 带主键和改动列的原值，INSERT 只写填了的列；执行后提示条数，重新读取。
7. 冲突：改一行的 `code`，在另一个查询标签中先把同一行改掉，再提交：提示"已被修改或删除"，那一行标红，改动保留。
8. 有未提交的改动时刷新、改排序、关闭标签：先确认放弃。
9. 中英文界面各一遍；Oracle（生产环境）提交时预览标红。
10. 删除测试数据和测试连接（系统凭据中的密码随之删除），确认凭据管理器中没有残留，停掉服务。

Expected: 全部通过。

- [ ] **Step 4: 提交**

```bash
git add docs/notes/m2a-data-editing.md docs/superpowers/specs/2026-10-22-dblens-design.md
git commit -m "docs: M2a 说明文档；总设计文档记录 M2 的拆分与进度" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```
