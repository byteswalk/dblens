# M5b 数据迁移 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在 M5a 的建表和建约束之间迁数据：按主键流式分批读写、断点续传、按表并行、出错停止或跳过、行数和抽样校验，迁完把序列和自增推到当前值。

**Architecture:** 方言层生成增量读取、计数、抽样、自增和序列调整的语句；服务层开始时建迁移记录（方案快照、脚本、每张表的搬运参数和进度，存在数据目录），作为任务依次建表、迁数据、建约束、调整、校验，每批提交后更新记录，中断后从记录接着做；前端在迁移标签中加模式和数据选项、按表的进度、未完成的迁移。

**Tech Stack:** Rust（tokio JoinSet / Semaphore、serde、insta）；React 19 + TypeScript、antd 6、TanStack Query、Vitest。

**Spec:** `docs/superpowers/specs/2026-10-28-m5b-data-migration-design.md`

## Global Constraints

- 搬运方式：服务端流式分批（读源 → 按目标列的逻辑类型转换 → 参数化批量写入），不调用各库的批量装载工具。
- 断点续传：迁移记录存在程序数据目录（方案快照 + 进度），每批提交后更新；迁移标签中列出未完成的迁移，可继续或放弃；另存的方案仍是普通 JSON。
- 单行出错：可选停止（默认）/ 跳过并记录（与 M2b 导入一致）；并行表数 1–4，默认 2；每批 100–100000 行，默认 1000。
- 每张表可填源库写法的过滤条件，可勾"只迁结构"；校验行数必做，可选按主键抽样逐值比对。
- 不做增量同步：界面提示迁移期间源库最好不要写入。
- 依赖只用正式发布版，跟随最新；新版发布满 7 天再升级（有意固定版本的记入 `scripts/check-deps.mjs` 的 `HOLD`）。
- 前端给用户看的文字都用词条（中英文），英文区分单复数；后端错误和提示用 `msg!` 词条（`src` 中的中文字面量只能出现在 `#[cfg(test)]` 之后，单独的测试文件中也不能有）；日志不翻译。
- 代码注释用中文；文件 UTF-8、LF 换行；前端代码风格：无分号、单引号、两格缩进。
- cargo 命令加 `--config "source.mirror.registry='sparse+https://rsproxy.cn/index/'"`（clippy 写在 `clippy` 之后），并设 `CARGO_INCREMENTAL=0`；后端测试在 `DBLENS_LANG=en` 下运行。
- 实库测试一律串行（`--test-threads=1`），不加 `--include-ignored`（会跑达梦的 spike 测试并卡住）。
- 前端工具用 PowerShell 执行（`npm`/`npx` 在本机不可用，用 `pnpm exec`）。
- 测试数据库的密码只在命令行中以内联环境变量传入，不写进仓库、报告或提交信息。

## 与设计文档的差异（写计划时实测后调整）

- 自增：MySQL 系和达梦写入显式值后自动推进，不执行语句；PG 系用 `setval(pg_get_serial_sequence…)`，Oracle 用 `START WITH LIMIT VALUE`。
- 序列：Oracle 一律删掉后重建（与达梦相同）。
- 达梦的 `IDENTITY_INSERT` 只对有自增列的表打开；达梦序列的 `CACHE 0` 迁到 PG 时去掉。
- 校验按目标的规则把空串、空二进制视为 NULL（Oracle 及兼容 Oracle 的模式）。
- PG 上的精确小数参数（续传位置、抽样时的主键）写成 `$n::text::numeric`。
- 迁移标签恢复旧方案时补上模式和数据选项。

## 执行说明

任务 2–5 的代码都已在临时副本中逐任务实现并通过检查，执行时按顺序重放。新文件给出全文，修改的文件给出 diff（基于上一个任务完成后的状态，可直接 `git apply`；任务 3 中 `migrate.rs` 移到 `migrate/mod.rs`，diff 按改名后的文件给出）。`Cargo.lock`、`web/pnpm-lock.yaml` 没有变化。

写计划时已运行：全部后端测试（775 个）和前端测试（273 个）；迁移的实库测试 52 个（11 个库）；浏览器中 TiDB → openGauss 的数据迁移、失败、继续和放弃。

## 文件结构

```text
crates/dblens-dialect/src/migrate/{data.rs（新增）, mod.rs, script.rs, types.rs}
crates/dblens-service/src/migrate/{mod.rs（由 migrate.rs 移来）, record.rs, run.rs, copy.rs, values.rs, report.rs（新增）}
crates/dblens-service/src/{lib.rs, service.rs, task.rs, import/job.rs}, tests/migrate_live.rs
crates/dblens-api/src/{migrations.rs, lib.rs}, tests/models.rs
crates/dblens-core/locales/app.json
web/src/features/migrate/{progress.tsx（新增）, MigrateView.tsx, plan.ts, plan.test.ts}
web/src/{shared/api.ts, app/shell.css, features/editor/{tabs.ts, tabs.test.ts}, locales/{zh-CN,en}/migrate.json}
docs/notes/{m5b-data-migration.md（新增）, m0-4b-api.md}; docs/superpowers/specs/2026-10-22-dblens-design.md
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

Expected：输出中没有"查询失败"（写计划时 86 个直接依赖都不用升级：有更新的发布都不满 7 天，`html-to-image` 在"有意不升级"名单中）。以执行当天的结果为准。

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

### Task 2: 方言层：数据迁移的语句

- `dblens-dialect/src/migrate/data.rs`（新）：`TableRead { schema, table, columns, key, filter }` 及 `select(d, last)`（按主键排序；给出上次位置时加 `(a > ?) OR (a = ? AND b > ?)`）、`count`、`sample(d, desc, limit)`、`row(d, key)`；
  PG 上精确小数参数写成 `$n::text::numeric`；`keyset_params`；`identity_insert_sql`（只有达梦）；`identity_reset_sql`（PG 系 `setval(pg_get_serial_sequence…)`、Oracle `START WITH LIMIT VALUE`，MySQL 系和达梦为 None）；
  `sequence_next_sql`（PG 系 `last_value`/`is_called`，Oracle、达梦 `ALL_SEQUENCES.LAST_NUMBER`）；`sequence_restart_sql`（PG 系 `setval`，Oracle、达梦删掉后重建）。
- `migrate/script.rs`：`ExistingAction` 加 `Append`、`Replace`（不动结构）和 `copies_data()`；`MigrationStatement`、`StatementKind` 加 `Deserialize`（存进迁移记录）；`sequence_sql` 改为 `pub(super)`。
- `migrate/mod.rs`：`TableOverride` 加 `filter`、`structure_only`；`pub mod data`。
- `migrate/types.rs`：迁到 PG 的序列去掉 `CACHE 0`（达梦的不缓存）。

**Files:**
- Create: `crates/dblens-dialect/src/migrate/data.rs`
- Modify: `crates/dblens-dialect/src/migrate/mod.rs`
- Modify: `crates/dblens-dialect/src/migrate/script.rs`
- Modify: `crates/dblens-dialect/src/migrate/tests.rs`
- Modify: `crates/dblens-dialect/src/migrate/types.rs`

**Interfaces:**
- Consumes：M5a 的 `migrate` 模块；`Dialect::{ident, qualified, placeholder, literal, paginate}`。
- Produces（`dblens_dialect::migrate`）：`data::{TableRead, keyset_params, identity_insert_sql, identity_reset_sql, sequence_next_sql, sequence_restart_sql}`；`ExistingAction::{Append, Replace}`、`ExistingAction::copies_data`；`TableOverride::{filter, structure_only}`。

- [ ] **Step 1: 修改 `crates/dblens-dialect/src/migrate/tests.rs`**

```diff
--- a/crates/dblens-dialect/src/migrate/tests.rs
+++ b/crates/dblens-dialect/src/migrate/tests.rs
@@ -383,6 +383,10 @@ fn existing_tables_follow_the_chosen_action() {
         "DropTable CreateTable Comment Comment Index Unique Check"
     );
     assert_eq!(kinds(Some(ExistingAction::MissingOnly)), "Check");
+    // 追加数据、清空后导入不动结构
+    assert_eq!(kinds(Some(ExistingAction::Append)), "");
+    assert_eq!(kinds(Some(ExistingAction::Replace)), "");
+    assert!(ExistingAction::Replace.copies_data() && !ExistingAction::MissingOnly.copies_data());
     // 外键要求两张表都建好
     let script = migration_script(&d, "APP", &draft, &BTreeMap::new(), &target_tables).unwrap();
     let fk = script.iter().find(|s| s.kind == StatementKind::ForeignKey).unwrap();
```

- [ ] **Step 2: 新建 `crates/dblens-dialect/src/migrate/data.rs`**

```rust
//! 数据迁移的语句（M5b 设计文档第 4、5 节）：按主键增量读取、计数、抽样、自增和序列的调整、达梦的显式写入自增列。
//! 只生成 SQL，不做 IO。

use dblens_core::schema::Sequence;
use dblens_core::{Result, Value};

use super::script::sequence_sql;
use crate::{Dialect, Family};

/// 读一张表：列、主键（为空时不排序、不能续传）、过滤条件（源库写法，原样放进 WHERE）。
#[derive(Debug, Clone, Copy)]
pub struct TableRead<'a> {
    pub schema: &'a str,
    pub table: &'a str,
    pub columns: &'a [String],
    pub key: &'a [String],
    pub filter: Option<&'a str>,
}

impl TableRead<'_> {
    fn idents(d: &Dialect, cols: &[String]) -> String {
        cols.iter().map(|c| d.ident(c)).collect::<Vec<_>>().join(", ")
    }

    fn filter(&self) -> Option<&str> {
        self.filter.map(str::trim).filter(|f| !f.is_empty())
    }

    fn base(&self, d: &Dialect) -> String {
        format!(
            "SELECT {} FROM {}",
            Self::idents(d, self.columns),
            d.qualified(Some(self.schema), self.table)
        )
    }

    /// 增量读取的查询：给出 `last`（上次位置的主键值）时只读主键大于它的行（参数见 `keyset_params`），按主键排序。
    pub fn select(&self, d: &Dialect, last: Option<&[Value]>) -> String {
        let mut sql = self.base(d);
        let mut conds = Vec::new();
        if let Some(f) = self.filter() {
            conds.push(format!("({f})"));
        }
        if let Some(last) = last.filter(|_| !self.key.is_empty()) {
            // 过滤条件中没有参数，主键条件的占位符从 1 开始
            conds.push(format!("({})", keyset(d, self.key, last)));
        }
        if !conds.is_empty() {
            sql += &format!(" WHERE {}", conds.join(" AND "));
        }
        if !self.key.is_empty() {
            sql += &format!(" ORDER BY {}", Self::idents(d, self.key));
        }
        sql
    }

    /// 行数：`SELECT COUNT(*) AS n FROM 表 [WHERE 过滤条件]`。
    pub fn count(&self, d: &Dialect) -> String {
        let mut sql = format!("SELECT COUNT(*) AS n FROM {}", d.qualified(Some(self.schema), self.table));
        if let Some(f) = self.filter() {
            sql += &format!(" WHERE ({f})");
        }
        sql
    }

    /// 抽样：按主键升序（`desc` 为 false）或降序取前 `limit` 行。
    pub fn sample(&self, d: &Dialect, desc: bool, limit: u64) -> String {
        let mut base = self.base(d);
        if let Some(f) = self.filter() {
            base += &format!(" WHERE ({f})");
        }
        let order = self
            .key
            .iter()
            .map(|c| format!("{}{}", d.ident(c), if desc { " DESC" } else { "" }))
            .collect::<Vec<_>>()
            .join(", ");
        d.paginate(&format!("{base} ORDER BY {order}"), 0, limit)
    }

    /// 按主键取一行（抽样比对时读目标），不带过滤条件。`key` 为参数（决定 PG 的类型转换）。
    pub fn row(&self, d: &Dialect, key: &[Value]) -> String {
        let cond = self
            .key
            .iter()
            .zip(key)
            .enumerate()
            .map(|(i, (k, v))| format!("{} = {}", d.ident(k), placeholder(d, i + 1, v)))
            .collect::<Vec<_>>()
            .join(" AND ");
        format!("{} WHERE {cond}", self.base(d))
    }
}

/// 占位符：PG 驱动不能直接绑定 NUMERIC 参数，精确小数按文本传、由数据库转换（与导入相同）。
fn placeholder(d: &Dialect, n: usize, v: &Value) -> String {
    let p = d.placeholder(n);
    if d.family() == Family::Postgres && matches!(v, Value::Decimal(_)) {
        format!("{p}::text::numeric")
    } else {
        p
    }
}

/// `(a > ?) OR (a = ? AND b > ?) OR …`：复合主键的"大于上次位置"，各库都支持的写法。
fn keyset(d: &Dialect, key: &[String], last: &[Value]) -> String {
    let mut n = 1;
    let mut ors = Vec::new();
    for i in 0..key.len() {
        let mut ands = Vec::new();
        for (j, k) in key.iter().enumerate().take(i + 1) {
            let op = if j == i { ">" } else { "=" };
            let v = last.get(j).unwrap_or(&Value::Null);
            ands.push(format!("{} {op} {}", d.ident(k), placeholder(d, n, v)));
            n += 1;
        }
        ors.push(format!("({})", ands.join(" AND ")));
    }
    ors.join(" OR ")
}

/// `keyset` 的参数：上次位置的主键值按占位符的顺序展开。
pub fn keyset_params(last: &[Value]) -> Vec<Value> {
    (0..last.len()).flat_map(|i| last[..=i].iter().cloned()).collect()
}

/// 达梦写入自增列前后的开关；其他库为 None（标识列都是 BY DEFAULT，可以直接写）。
pub fn identity_insert_sql(d: &Dialect, schema: &str, table: &str, on: bool) -> Option<String> {
    (d.family() == Family::Dameng).then(|| {
        format!("SET IDENTITY_INSERT {} {}", d.qualified(Some(schema), table), if on { "ON" } else { "OFF" })
    })
}

/// 迁完数据后把自增列推到当前最大值之后。MySQL 系写入显式值时会自动推进计数器，达梦在 `IDENTITY_INSERT` 下也会，
/// 都返回 None。
pub fn identity_reset_sql(d: &Dialect, schema: &str, table: &str, column: &str) -> Result<Option<String>> {
    let q = d.qualified(Some(schema), table);
    let c = d.ident(column);
    Ok(match d.family() {
        Family::Postgres => Some(format!(
            "SELECT setval(pg_get_serial_sequence({}, {}), COALESCE(MAX({c}), 0) + 1, false) FROM {q}",
            d.literal(&Value::String(q.clone()))?,
            d.literal(&Value::String(column.to_string()))?
        )),
        Family::Oracle => Some(format!(
            "ALTER TABLE {q} MODIFY ({c} GENERATED BY DEFAULT AS IDENTITY (START WITH LIMIT VALUE))"
        )),
        Family::MySql | Family::Dameng => None,
    })
}

/// 读序列的下一个值的查询（一行一列 `n`）：PG 系由 `last_value`、`is_called` 算出；Oracle、达梦取 `LAST_NUMBER`
/// （带缓存时可能比实际的下一个值大，作为起点是安全的）。MySQL 系没有序列，返回 None。
pub fn sequence_next_sql(d: &Dialect, schema: &str, name: &str) -> Result<Option<String>> {
    Ok(match d.family() {
        Family::Postgres => Some(format!(
            "SELECT CASE WHEN is_called THEN last_value + 1 ELSE last_value END AS n FROM {}",
            d.qualified(Some(schema), name)
        )),
        Family::Oracle | Family::Dameng => Some(format!(
            "SELECT LAST_NUMBER AS n FROM ALL_SEQUENCES WHERE SEQUENCE_OWNER = {} AND SEQUENCE_NAME = {}",
            d.literal(&Value::String(schema.to_string()))?,
            d.literal(&Value::String(name.to_string()))?
        )),
        Family::MySql => None,
    })
}

/// 把目标序列推到 `next`：PG 系 `setval`；Oracle、达梦删掉后按原定义以 `next` 起始重建。
pub fn sequence_restart_sql(d: &Dialect, schema: &str, seq: &Sequence, next: &str) -> Result<Vec<String>> {
    let q = d.qualified(Some(schema), &seq.name);
    Ok(match d.family() {
        Family::Postgres => vec![format!("SELECT setval({}, {next}, false)", d.literal(&Value::String(q))?)],
        Family::Oracle | Family::Dameng => {
            let s = Sequence { start: Some(next.to_string()), ..seq.clone() };
            vec![format!("DROP SEQUENCE {q}"), sequence_sql(d, schema, &s)]
        }
        Family::MySql => vec![],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Variant;

    fn d(v: Variant) -> Dialect {
        Dialect::new(v)
    }

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    fn read<'a>(
        table: &'a str,
        cols: &'a [String],
        key: &'a [String],
        filter: Option<&'a str>,
    ) -> TableRead<'a> {
        TableRead { schema: if table == "T" { "APP" } else { "app" }, table, columns: cols, key, filter }
    }

    #[test]
    fn incremental_selects() {
        let (cols, id) = (s(&["id", "name"]), s(&["id"]));
        let pg = d(Variant::Postgres);
        assert_eq!(read("t", &cols, &id, None).select(&pg, None), "SELECT id, name FROM app.t ORDER BY id");
        assert_eq!(
            read("t", &cols, &id, Some("name <> 'x'")).select(&pg, Some(&[Value::Int(5)])),
            "SELECT id, name FROM app.t WHERE (name <> 'x') AND ((id > $1)) ORDER BY id"
        );
        // PG 的精确小数参数按文本传
        assert_eq!(
            read("t", &cols, &id, None).select(&pg, Some(&[Value::Decimal("5".into())])),
            "SELECT id, name FROM app.t WHERE ((id > $1::text::numeric)) ORDER BY id"
        );
        let ora = d(Variant::Oracle);
        let ab = s(&["A", "B"]);
        assert_eq!(
            read("T", &ab, &ab, None).select(&ora, Some(&[Value::Int(1), Value::Int(2)])),
            "SELECT A, B FROM APP.T WHERE ((A > :1) OR (A = :2 AND B > :3)) ORDER BY A, B"
        );
        assert_eq!(
            read("t", &cols, &[], None).select(&d(Variant::MySql), Some(&[])),
            "SELECT id, name FROM app.t"
        );
        assert_eq!(
            keyset_params(&[Value::Int(1), Value::Int(2)]),
            vec![Value::Int(1), Value::Int(1), Value::Int(2)]
        );
    }

    #[test]
    fn counts_samples_and_rows() {
        let (my, id) = (d(Variant::MySql), s(&["id"]));
        assert_eq!(
            read("t", &id, &id, Some("x > 1")).count(&my),
            "SELECT COUNT(*) AS n FROM app.t WHERE (x > 1)"
        );
        assert_eq!(
            read("t", &id, &id, Some("x > 1")).sample(&my, true, 50),
            "SELECT id FROM app.t WHERE (x > 1) ORDER BY id DESC
LIMIT 50 OFFSET 0"
        );
        let ora = d(Variant::Oracle);
        let (oid, cols) = (s(&["ID"]), s(&["ID", "X"]));
        assert!(read("T", &oid, &oid, None).sample(&ora, false, 50).contains("FETCH NEXT 50 ROWS ONLY"));
        assert_eq!(
            read("T", &cols, &oid, Some("X > 1")).row(&ora, &[Value::Int(1)]),
            "SELECT ID, X FROM APP.T WHERE ID = :1"
        );
    }

    #[test]
    fn identity_and_sequences() {
        let pg = d(Variant::Postgres);
        assert_eq!(
            identity_reset_sql(&pg, "app", "t", "id").unwrap().unwrap(),
            "SELECT setval(pg_get_serial_sequence('app.t', 'id'), COALESCE(MAX(id), 0) + 1, false) FROM app.t"
        );
        assert_eq!(
            identity_reset_sql(&d(Variant::Oracle), "APP", "T", "ID").unwrap().unwrap(),
            "ALTER TABLE APP.T MODIFY (ID GENERATED BY DEFAULT AS IDENTITY (START WITH LIMIT VALUE))"
        );
        assert_eq!(identity_reset_sql(&d(Variant::MySql), "db", "t", "id").unwrap(), None);
        assert_eq!(
            identity_insert_sql(&d(Variant::Dameng), "APP", "T", true).as_deref(),
            Some("SET IDENTITY_INSERT APP.T ON")
        );
        assert_eq!(identity_insert_sql(&pg, "app", "t", true), None);
        assert!(sequence_next_sql(&pg, "app", "s").unwrap().unwrap().contains("is_called"));
        let seq = Sequence {
            name: "S".into(),
            start: Some("1".into()),
            increment: Some("1".into()),
            min_value: None,
            max_value: None,
            cycle: false,
            cache: Some(20),
        };
        let ora = d(Variant::Oracle);
        let sqls = sequence_restart_sql(&ora, "APP", &seq, "42").unwrap();
        assert_eq!(sqls[0], "DROP SEQUENCE APP.S");
        assert!(sqls[1].contains("START WITH 42"), "{}", sqls[1]);
        assert_eq!(
            sequence_restart_sql(&pg, "app", &Sequence { name: "s".into(), ..seq }, "42").unwrap(),
            ["SELECT setval('app.s', 42, false)"]
        );
    }
}
```

- [ ] **Step 3: 修改 `crates/dblens-dialect/src/migrate/mod.rs`**

```diff
--- a/crates/dblens-dialect/src/migrate/mod.rs
+++ b/crates/dblens-dialect/src/migrate/mod.rs
@@ -1,6 +1,7 @@
 //! 结构迁移（M5a 设计文档第 3、4 节）：把源 schema 的表和序列按目标方言转换成目标结构，
 //! 给出逐项提示，再生成两步的 DDL（先建序列和表，再建索引和约束）。不做任何 IO。
 
+pub mod data;
 mod expr;
 mod names;
 mod script;
@@ -67,7 +68,8 @@ pub struct ColumnOverride {
     pub accept: bool,
 }
 
-/// 一张表的覆盖。`checks`：源检查约束名 → 目标表达式，空串表示不迁。
+/// 一张表的覆盖。`checks`：源检查约束名 → 目标表达式，空串表示不迁。`filter`：迁数据时的过滤条件（源库写法）；
+/// `structure_only`：只迁结构不迁数据（M5b）。
 #[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
 pub struct TableOverride {
     #[serde(default, skip_serializing_if = "Option::is_none")]
@@ -78,6 +80,10 @@ pub struct TableOverride {
     pub columns: BTreeMap<String, ColumnOverride>,
     #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
     pub checks: BTreeMap<String, String>,
+    #[serde(default, skip_serializing_if = "Option::is_none")]
+    pub filter: Option<String>,
+    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
+    pub structure_only: bool,
 }
 
 /// 一个序列的覆盖。
```

- [ ] **Step 4: 修改 `crates/dblens-dialect/src/migrate/script.rs`**

```diff
--- a/crates/dblens-dialect/src/migrate/script.rs
+++ b/crates/dblens-dialect/src/migrate/script.rs
@@ -11,7 +11,7 @@ use super::MigrationDraft;
 use crate::design::{Diff, TableChange};
 use crate::{Dialect, Family};
 
-/// 目标中已有同名表时的处理方式。
+/// 目标中已有同名表时的处理方式。追加数据、清空后导入（M5b）不动结构。
 #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
 #[serde(rename_all = "snake_case")]
 pub enum ExistingAction {
@@ -19,9 +19,18 @@ pub enum ExistingAction {
     Recreate,
     /// 不建表，只建目标表中缺少的索引和约束。
     MissingOnly,
+    Append,
+    Replace,
 }
 
-#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
+impl ExistingAction {
+    /// 是否迁数据：跳过、只建缺的索引和约束不迁。
+    pub fn copies_data(self) -> bool {
+        matches!(self, ExistingAction::Recreate | ExistingAction::Append | ExistingAction::Replace)
+    }
+}
+
+#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
 #[serde(rename_all = "snake_case")]
 pub enum StatementKind {
     DropTable,
@@ -35,7 +44,7 @@ pub enum StatementKind {
 }
 
 /// 一条迁移语句。`object` 是源对象名；`requires`：这些源表建表失败时跳过本语句。
-#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
+#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
 pub struct MigrationStatement {
     pub phase: u8,
     pub kind: StatementKind,
@@ -71,7 +80,7 @@ pub fn migration_script(
         } else {
             ExistingAction::Recreate
         };
-        if action == ExistingAction::Skip {
+        if matches!(action, ExistingAction::Skip | ExistingAction::Append | ExistingAction::Replace) {
             continue;
         }
         let present = target_tables.iter().find(|x| x.name.eq_ignore_ascii_case(&t.target.name));
@@ -211,7 +220,7 @@ fn drop_sql(tgt: &Dialect, schema: &str, name: &str) -> String {
 }
 
 /// 序列：PG 系用 `NO CYCLE`；Oracle、达梦用 `NOCYCLE`，缓存小于 2 时写 `NOCACHE`。
-fn sequence_sql(tgt: &Dialect, schema: &str, s: &Sequence) -> String {
+pub(super) fn sequence_sql(tgt: &Dialect, schema: &str, s: &Sequence) -> String {
     if tgt.family() == Family::Postgres {
         return tgt.sequence_ddl(schema, s).trim().trim_end_matches(';').to_string();
     }
```

- [ ] **Step 5: 修改 `crates/dblens-dialect/src/migrate/types.rs`**

```diff
--- a/crates/dblens-dialect/src/migrate/types.rs
+++ b/crates/dblens-dialect/src/migrate/types.rs
@@ -148,6 +148,8 @@ pub(super) fn fk_action_supported(tgt: &Dialect, what: &str, action: &str) -> bo
 pub(super) fn sequence(tgt: &Dialect, s: &Sequence, name: &str, n: &mut Notes) -> Sequence {
     let mut out = Sequence { name: name.to_string(), ..s.clone() };
     if tgt.family() == Family::Postgres {
+        // 达梦的不缓存是 CACHE 0，PG 的缓存至少为 1
+        out.cache = out.cache.filter(|&c| c >= 1);
         let fits = |v: &Option<String>| v.as_deref().is_none_or(|v| v.trim().parse::<i64>().is_ok());
         for (label, v) in [("MINVALUE", &mut out.min_value), ("MAXVALUE", &mut out.max_value)] {
             if !fits(v) {
```

- [ ] **Step 6: 检查**

```bash
export CARGO_INCREMENTAL=0
cargo fmt --all
cargo clippy --config "source.mirror.registry='sparse+https://rsproxy.cn/index/'" --workspace --all-targets -- -D warnings
DBLENS_LANG=en cargo test --config "source.mirror.registry='sparse+https://rsproxy.cn/index/'" --workspace
```

Expected: 全部测试通过（dialect 新增 3 个，原有的同名表测试加两种处理方式）。

- [ ] **Step 7: 提交**

```bash
git add crates/dblens-dialect/src/migrate/data.rs crates/dblens-dialect/src/migrate/mod.rs crates/dblens-dialect/src/migrate/script.rs crates/dblens-dialect/src/migrate/tests.rs crates/dblens-dialect/src/migrate/types.rs
git commit -m "feat(migrate): 数据迁移的读取、计数、自增和序列的语句" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 3: 服务层：迁移记录、数据搬运、续传与校验；接口；实库测试

- `dblens-service/src/migrate.rs` 移到 `migrate/mod.rs` 并拆分：
  - `mod.rs`：`MigrationMode::{All, Structure, Data}`、`DataOptions { batch_size, parallel, on_error, sample }`，`MigrationPlan` 加 `mode`、`data`；`OtherObject` 加 `definition`（触发器）；
    开始前检查加"只迁数据时表必须已有"和数据选项的范围；`start_migration` 建迁移记录（每张要迁数据的表的 `TableCopy`、行数估计、序列、提示文字）后执行；
    `resume_migration`（有失败的表时回到迁数据）、`unfinished_migrations`、`discard_migration`；`launch` 在开始时的界面语言下运行任务，结束后刷新缓存、写报告。
  - `record.rs`（新）：`MigrationPhase`、`TableCopy`、`TableState`、`TableProgress`、`Verify`、`Mismatch`、`SkippedRow`、`Adjustment`、`MigrationRecord`、`MigrationSummary`、`MigrationRecords`（保存先写临时文件再改名、未完成的列表、正在执行的标记）。
  - `run.rs`（新）：`Ctx`、`run`（各步按记录的步骤接着做）、语句执行（成功的记下，失败的继续时再执行）、按表并行搬运（`JoinSet` + `Semaphore`）、调整自增和序列、校验（行数、抽样逐值比对）；`MigrationProgress`、`TableView`。
  - `copy.rs`（新）：一张表的搬运（从头开始时清空或记下原有行数；达梦打开 `IDENTITY_INSERT`；流式读、分批写、每批一个事务；停止或跳过逐行重试；续传）。
  - `values.rs`（新）：`to_target`、`normalize`（`empty_is_null`：Oracle 规则）、`key_text`。
  - `report.rs`（新）：由迁移记录写报告。
- `task.rs`：`TaskStatus::migration`；去掉 M5a 加的 `TaskHandle::status`。`service.rs`：`Service::migrations`（`open` 时放到数据目录的 `migrations`）。`import/job.rs`：`OnError` 加 `Serialize`。`lib.rs` 导出新类型。
- `dblens-api/src/migrations.rs`：`GET /migrations`、`POST /migrations/{id}/resume`、`DELETE /migrations/{id}`；`tests/models.rs` 的测试服务用单独的迁移记录目录。
- `tests/migrate_live.rs`：源库加 1000 行的表、各种类型的表、推进过的序列；迁结构和数据后核对校验、自增、序列；续传和跳过的测试（`special!`）。
- 后端词条加 `migrate.*`（数据选项、记录、续传、值、报告），去掉不再用的 `migrate.report.summary`。

**Files:**
- Modify: `crates/dblens-api/src/lib.rs`
- Modify: `crates/dblens-api/src/migrations.rs`
- Modify: `crates/dblens-api/tests/models.rs`
- Modify: `crates/dblens-core/locales/app.json`
- Modify: `crates/dblens-service/src/import/job.rs`
- Modify: `crates/dblens-service/src/lib.rs`
- Create: `crates/dblens-service/src/migrate/copy.rs`
- Modify: `crates/dblens-service/src/migrate/mod.rs`（由 `crates/dblens-service/src/migrate.rs` 移来）
- Create: `crates/dblens-service/src/migrate/record.rs`
- Create: `crates/dblens-service/src/migrate/report.rs`
- Create: `crates/dblens-service/src/migrate/run.rs`
- Create: `crates/dblens-service/src/migrate/values.rs`
- Modify: `crates/dblens-service/src/service.rs`
- Modify: `crates/dblens-service/src/task.rs`
- Modify: `crates/dblens-service/tests/migrate_live.rs`

**Interfaces:**
- Consumes：任务 2；M5a 的预览、`Run::sql`、`check_policy`；M2b 的 `convert`、`exec`、`OnError`；导出的 `EXPORT_LOB_LIMIT`。
- Produces（`dblens_service`）：`MigrationMode`、`DataOptions`、`MigrationPhase`、`MigrationProgress`、`MigrationSummary`、`TableState`、`TableView`、`Verify`、`Mismatch`；
  `Service::{resume_migration(id) -> Result<String>, unfinished_migrations() -> Result<Vec<MigrationSummary>>, discard_migration(id) -> Result<()>}`、`Service::migrations`；接口见上。

- [ ] **Step 1: 修改 `crates/dblens-api/tests/models.rs`**

```diff
--- a/crates/dblens-api/tests/models.rs
+++ b/crates/dblens-api/tests/models.rs
@@ -17,6 +17,8 @@ fn app(local_files: bool) -> Router {
     let registry = Arc::new(DriverRegistry::with_drivers(store.clone(), vec![]));
     let history = HistoryLog::inline(store.clone());
     let service = Arc::new(Service::with_history(store, secrets, registry, Arc::new(DefaultPolicy), history));
+    // 迁移记录放在单独的临时目录（默认目录可能有其他测试留下的记录）
+    service.migrations.set_dir(tempfile::tempdir().unwrap().keep().join("migrations"));
     router(service, ApiConfig { token: "t".into(), allowed_origins: vec![], local_files })
 }
 
@@ -321,4 +323,11 @@ async fn migrations_need_existing_profiles() {
         let (st, v) = call(&app, "POST", path, Some(plan.clone())).await;
         assert_eq!(st, StatusCode::NOT_FOUND, "{path} {v}");
     }
+    // 没有完成的迁移：一开始为空；不存在的记录继续、放弃都报 404
+    let (st, v) = call(&app, "GET", "/api/migrations", None).await;
+    assert_eq!((st, v), (StatusCode::OK, json!([])));
+    let (st, _) = call(&app, "POST", "/api/migrations/abc123/resume", None).await;
+    assert_eq!(st, StatusCode::NOT_FOUND);
+    let (st, _) = call(&app, "DELETE", "/api/migrations/abc123", None).await;
+    assert_eq!(st, StatusCode::NOT_FOUND);
 }
```

- [ ] **Step 2: 修改 `crates/dblens-service/tests/migrate_live.rs`**

````diff
--- a/crates/dblens-service/tests/migrate_live.rs
+++ b/crates/dblens-service/tests/migrate_live.rs
@@ -1,7 +1,9 @@
-//! 结构迁移（M5a）的实库测试：源库和目标库的环境变量都设置时运行，否则直接通过（变量见 `live.rs`）。
+//! 迁移（M5a 结构、M5b 数据）的实库测试：源库和目标库的环境变量都设置时运行，否则直接通过（变量见 `live.rs`）。
 //!
-//! 在源库建一套测试结构（自增主键、默认值、注释、唯一约束、检查约束、索引、外键），迁到目标库，
-//! 读回目标结构，与迁移草稿中的目标结构比对应无差异；最后清理两边。表名带源库的代号，不同路线互不干扰。
+//! 在源库建一套测试结构（自增主键、默认值、注释、唯一约束、检查约束、索引、外键）并放数据（1000 行的表、各种类型的表、
+//! 推进过的序列），迁到目标库：读回目标结构与迁移草稿比对；行数和抽样逐值比对（迁移自己的校验）都一致；
+//! 目标上插入新行时自增不冲突，序列接着源序列往下走；最后清理两边。表名带源库的代号，不同路线互不干扰。
+//! 另有续传（迁到一半取消再继续）和跳过出错行的测试。
 //! 人大金仓开发版最多 10 个连接，与其他实库测试一样要串行运行（`--test-threads=1`）。
 
 use std::collections::BTreeMap;
@@ -9,17 +11,22 @@ use std::path::Path;
 use std::sync::Arc;
 use std::time::Duration;
 
-use dblens_core::LogicalType;
 use dblens_core::schema::Table;
+use dblens_core::{LogicalType, Value};
+use dblens_dialect::migrate::{ColumnOverride, ExistingAction};
 use dblens_dialect::{Dialect, Variant};
 use dblens_driver::{ConnectParams, DriverId};
 use dblens_service::{
-    DefaultPolicy, DriverRegistry, Environment, ExecRequest, MigrationEnd, MigrationPlan, ProfileInput,
-    SecretBackend, Secrets, Service, SessionEvent, Store, TaskState, TaskStatus,
+    DataOptions, DefaultPolicy, DriverRegistry, Environment, ExecRequest, MigrationEnd, MigrationPlan,
+    OnError, ProfileInput, SecretBackend, Secrets, Service, SessionEvent, Store, TableState, TaskState,
+    TaskStatus,
 };
 
 const LIB_OPTIONS: [&str; 2] = ["oracle_client_lib_dir", "dpi_lib_dir"];
 
+/// 大表的行数。MySQL 的递归 CTE 默认至多 1000 层。
+const ROWS: u64 = 1000;
+
 struct End {
     profile: String,
     driver: DriverId,
@@ -27,6 +34,16 @@ struct End {
     schema: String,
 }
 
+impl End {
+    fn upper(&self) -> bool {
+        matches!(self.driver, DriverId::Oracle | DriverId::Dameng)
+    }
+
+    fn has_sequences(&self) -> bool {
+        self.driver != DriverId::Mysql
+    }
+}
+
 /// 一个服务中建两个连接配置；任一环境变量没有设置时返回 None。
 async fn setup(envs: [&str; 2]) -> Option<(Arc<Service>, Vec<End>, tempfile::TempDir)> {
     let urls: Vec<String> = envs.iter().map(|e| std::env::var(e).ok()).collect::<Option<_>>()?;
@@ -36,6 +53,7 @@ async fn setup(envs: [&str; 2]) -> Option<(Arc<Service>, Vec<End>, tempfile::Tem
     secrets.init_vault("测试主密码").unwrap();
     let registry = Arc::new(DriverRegistry::new(store.clone()));
     let service = Arc::new(Service::with_parts(store, secrets, registry.clone(), Arc::new(DefaultPolicy)));
+    service.migrations.set_dir(dir.path().join("migrations"));
     let mut ends = Vec::new();
     for (env, url) in envs.iter().zip(urls) {
         let (driver, params) = ConnectParams::from_url(&url).unwrap();
@@ -71,38 +89,59 @@ async fn setup(envs: [&str; 2]) -> Option<(Arc<Service>, Vec<End>, tempfile::Tem
     Some((service, ends, dir))
 }
 
-/// 在一端执行若干语句；`tolerate` 为 true 时忽略失败（清理用）。
-async fn run(s: &Service, end: &End, sqls: &[String], tolerate: bool) {
+/// 在一端执行若干语句，返回最后一个结果集的第一个值；`tolerate` 为 true 时忽略失败（清理用）。
+async fn run(s: &Service, end: &End, sqls: &[String], tolerate: bool) -> Option<Value> {
     let id = s.open_session(&end.profile).await.unwrap().id;
+    let mut last = None;
     for sql in sqls {
         s.sessions.submit(id, ExecRequest::new(sql)).await.unwrap();
         while let Some(ev) = s.sessions.next(id, 500).await.unwrap() {
-            if let SessionEvent::Failed { error, .. } = &ev {
-                assert!(tolerate, "{sql} 执行失败：{error}");
+            match &ev {
+                SessionEvent::Failed { error, .. } => assert!(tolerate, "{sql} 执行失败：{error}"),
+                SessionEvent::Rows { rows } => last = rows.first().and_then(|r| r.first()).cloned(),
+                _ => {}
             }
         }
     }
     s.sessions.close(id, true).await.unwrap();
+    last
 }
 
-/// 删除测试表（子表在前）。
-fn drops(end: &End, names: &[String]) -> Vec<String> {
-    names
+fn number(v: Option<Value>) -> i64 {
+    match v {
+        Some(Value::Int(n)) => n,
+        Some(Value::UInt(n)) => n as i64,
+        Some(Value::Decimal(s) | Value::String(s)) => s.trim().split('.').next().unwrap().parse().unwrap(),
+        other => panic!("不是数字：{other:?}"),
+    }
+}
+
+/// 删除测试表（子表在前）和序列。
+fn drops(end: &End, names: &[String], seq: &str) -> Vec<String> {
+    let mut out: Vec<String> = names
         .iter()
         .map(|n| match end.driver {
             DriverId::Oracle => {
                 format!("BEGIN EXECUTE IMMEDIATE 'DROP TABLE \"{n}\" CASCADE CONSTRAINTS'; EXCEPTION WHEN OTHERS THEN NULL; END;")
             }
-            DriverId::Dameng => format!("DROP TABLE IF EXISTS \"{n}\" CASCADE"),
-            DriverId::Postgres => format!("DROP TABLE IF EXISTS \"{n}\" CASCADE"),
+            DriverId::Dameng | DriverId::Postgres => format!("DROP TABLE IF EXISTS \"{n}\" CASCADE"),
             DriverId::Mysql => format!("DROP TABLE IF EXISTS `{n}`"),
         })
-        .collect()
+        .collect();
+    out.push(match end.driver {
+        DriverId::Oracle => format!(
+            "BEGIN EXECUTE IMMEDIATE 'DROP SEQUENCE \"{seq}\"'; EXCEPTION WHEN OTHERS THEN NULL; END;"
+        ),
+        DriverId::Dameng | DriverId::Postgres => format!("DROP SEQUENCE IF EXISTS \"{seq}\""),
+        DriverId::Mysql => "SELECT 1".into(),
+    });
+    out
 }
 
-/// 源库的测试结构：`p` 为表名前缀（Oracle、达梦用大写）。
+/// 源库的测试结构和数据：`p` 为表名前缀（Oracle、达梦用大写）。
 fn source_ddl(end: &End, p: &str) -> Vec<String> {
     let opengauss = end.variant == Variant::OpenGauss;
+    let n = ROWS;
     match end.driver {
         DriverId::Mysql => vec![
             format!(
@@ -116,6 +155,21 @@ fn source_ddl(end: &End, p: &str) -> Vec<String> {
                  CONSTRAINT {p}_ck_qty CHECK (qty > 0), \
                  CONSTRAINT {p}_fk_cust FOREIGN KEY (cust_id) REFERENCES {p}_cust (id) ON DELETE CASCADE)"
             ),
+            format!(
+                "CREATE TABLE {p}_types (id INT PRIMARY KEY, txt VARCHAR(100), big TEXT, amount DECIMAL(12,2), ratio DOUBLE, \
+                 flag TINYINT(1), day DATE, ts DATETIME(3), bin VARBINARY(16))"
+            ),
+            format!(
+                "INSERT INTO {p}_cust (name, amount, note) WITH RECURSIVE s(n) AS (SELECT 1 UNION ALL SELECT n + 1 FROM s WHERE n < {n}) \
+                 SELECT CONCAT('name', n), n * 1.5, CONCAT('备注', n) FROM s"
+            ),
+            format!(
+                "INSERT INTO {p}_ord (id, cust_id, qty, status) VALUES (1, 1, 2, 'paid'), (2, 3, 1, 'new')"
+            ),
+            format!(
+                "INSERT INTO {p}_types VALUES (1, '中文 abc', REPEAT('y', 3000), 1234.5, 0.25, 1, '2026-01-02', '2026-01-02 03:04:05.678', X'0102FF'), \
+                 (2, 'x''q', 'z', -0.01, NULL, 0, NULL, NULL, X''), (3, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL)"
+            ),
         ],
         DriverId::Postgres => vec![
             format!(
@@ -136,13 +190,45 @@ fn source_ddl(end: &End, p: &str) -> Vec<String> {
                  CONSTRAINT {p}_ck_qty CHECK (qty > 0), \
                  CONSTRAINT {p}_fk_cust FOREIGN KEY (cust_id) REFERENCES {p}_cust (id) ON DELETE CASCADE)"
             ),
+            format!(
+                "CREATE TABLE {p}_types (id INT PRIMARY KEY, txt VARCHAR(100), big TEXT, amount NUMERIC(12,2), ratio DOUBLE PRECISION, \
+                 flag BOOLEAN, day DATE, ts TIMESTAMP(3), bin BYTEA)"
+            ),
+            format!("CREATE SEQUENCE {p}_seq START WITH 1"),
+            format!("SELECT nextval('{p}_seq') FROM generate_series(1, 5)"),
+            format!(
+                "INSERT INTO {p}_cust (name, amount, note) SELECT 'name' || g, g * 1.5, '备注' || g FROM generate_series(1, {n}) g"
+            ),
+            format!(
+                "INSERT INTO {p}_ord (id, cust_id, qty, status) VALUES (1, 1, 2, 'paid'), (2, 3, 1, 'new')"
+            ),
+            format!(
+                "INSERT INTO {p}_types VALUES (1, '中文 abc', repeat('y', 3000), 1234.5, 0.25, true, DATE '2026-01-02', \
+                 TIMESTAMP '2026-01-02 03:04:05.678', '\\x0102ff'::bytea), (2, 'x''q', 'z', -0.01, NULL, false, NULL, NULL, NULL), \
+                 (3, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL)"
+            ),
         ],
         DriverId::Oracle | DriverId::Dameng => {
+            let ora = end.driver == DriverId::Oracle;
             // 达梦的外键要求两边类型相同
-            let (id, created, cust_id) = if end.driver == DriverId::Oracle {
-                ("NUMBER(19) GENERATED BY DEFAULT AS IDENTITY", "DATE DEFAULT SYSDATE", "NUMBER(19)")
+            let (id, created, cust_id, flag, bin, hex) = if ora {
+                (
+                    "NUMBER(19) GENERATED BY DEFAULT AS IDENTITY",
+                    "DATE DEFAULT SYSDATE",
+                    "NUMBER(19)",
+                    "NUMBER(1)",
+                    "RAW(16)",
+                    "HEXTORAW('0102FF')",
+                )
             } else {
-                ("BIGINT IDENTITY(1, 1)", "TIMESTAMP DEFAULT SYSDATE", "BIGINT")
+                (
+                    "BIGINT IDENTITY(1, 1)",
+                    "TIMESTAMP DEFAULT SYSDATE",
+                    "BIGINT",
+                    "BIT",
+                    "VARBINARY(16)",
+                    "0x0102FF",
+                )
             };
             vec![
                 format!(
@@ -158,22 +244,46 @@ fn source_ddl(end: &End, p: &str) -> Vec<String> {
                      CONSTRAINT {p}_CK_QTY CHECK (QTY > 0), \
                      CONSTRAINT {p}_FK_CUST FOREIGN KEY (CUST_ID) REFERENCES {p}_CUST (ID) ON DELETE CASCADE)"
                 ),
+                format!(
+                    "CREATE TABLE {p}_TYPES (ID NUMBER(10) PRIMARY KEY, TXT VARCHAR2(100), BIG CLOB, AMOUNT NUMBER(12,2), \
+                     RATIO {}, FLAG {flag}, DAY DATE, TS TIMESTAMP(3), BIN {bin})",
+                    if ora { "BINARY_DOUBLE" } else { "DOUBLE" }
+                ),
+                format!("CREATE SEQUENCE {p}_SEQ START WITH 1"),
+                format!(
+                    "DECLARE n NUMBER; BEGIN FOR i IN 1..5 LOOP SELECT {p}_SEQ.NEXTVAL INTO n FROM DUAL; END LOOP; END;"
+                ),
+                format!(
+                    "INSERT INTO {p}_CUST (NAME, AMOUNT, NOTE) SELECT 'name' || LEVEL, LEVEL * 1.5, '备注' || LEVEL FROM DUAL CONNECT BY LEVEL <= {n}"
+                ),
+                format!("INSERT INTO {p}_ORD (ID, CUST_ID, QTY, STATUS) VALUES (1, 1, 2, 'paid')"),
+                format!("INSERT INTO {p}_ORD (ID, CUST_ID, QTY, STATUS) VALUES (2, 3, 1, 'new')"),
+                format!(
+                    "INSERT INTO {p}_TYPES VALUES (1, '中文 abc', RPAD('y', 3000, 'y'), 1234.5, 0.25, 1, DATE '2026-01-02', \
+                     TIMESTAMP '2026-01-02 03:04:05.678', {hex})"
+                ),
+                format!("INSERT INTO {p}_TYPES VALUES (2, 'x''q', 'z', -0.01, NULL, 0, NULL, NULL, NULL)"),
+                format!("INSERT INTO {p}_TYPES VALUES (3, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL)"),
+                "COMMIT".into(),
             ]
         }
     }
 }
 
-/// 源库中的表名（Oracle、达梦为大写）。
+/// 表名（子表在前；Oracle、达梦为大写）。
 fn names(end: &End, p: &str) -> Vec<String> {
-    let up = matches!(end.driver, DriverId::Oracle | DriverId::Dameng);
-    ["ord", "cust"]
+    ["ord", "types", "cust"]
         .iter()
-        .map(|t| if up { format!("{p}_{t}").to_uppercase() } else { format!("{p}_{t}") })
+        .map(|t| if end.upper() { format!("{p}_{t}").to_uppercase() } else { format!("{p}_{t}") })
         .collect()
 }
 
+fn seq_name(end: &End, p: &str) -> String {
+    if end.upper() { format!("{p}_seq").to_uppercase() } else { format!("{p}_seq") }
+}
+
 async fn wait(s: &Service, id: &str) -> TaskStatus {
-    for _ in 0..1200 {
+    for _ in 0..3000 {
         let st = s.tasks.status(id).unwrap();
         if !matches!(st.state, TaskState::Running | TaskState::Confirm) {
             return st;
@@ -183,31 +293,53 @@ async fn wait(s: &Service, id: &str) -> TaskStatus {
     panic!("任务没有结束");
 }
 
-/// 源 → 目标迁移一次并核对。`p`：表名前缀（小写）。
-async fn migrate_flow(src_env: &str, tgt_env: &str, p: &str) {
-    let Some((s, ends, _dir)) = setup([src_env, tgt_env]).await else { return };
-    let (src, tgt) = (&ends[0], &ends[1]);
-    let route = format!("{src_env} → {tgt_env}");
-    let up = matches!(src.driver, DriverId::Oracle | DriverId::Dameng);
-    let prefix = if up { p.to_uppercase() } else { p.to_string() };
+/// 一条路线的源和目标：建好源库的测试数据，清理目标。返回 `(源表名, 目标表名, 源序列名)`。
+async fn prepare(s: &Service, src: &End, tgt: &End, p: &str) -> (Vec<String>, Vec<String>, String) {
+    let prefix = if src.upper() { p.to_uppercase() } else { p.to_string() };
     let src_names = names(src, &prefix);
     let mut tgt_names = names(tgt, p);
     // MySQL 区分表名大小写：清理时大小写两种都删
     if tgt.driver == DriverId::Mysql {
         tgt_names.extend(names(tgt, p).iter().map(|n| n.to_uppercase()).collect::<Vec<_>>());
     }
-    run(&s, src, &drops(src, &src_names), true).await;
-    run(&s, tgt, &drops(tgt, &tgt_names), true).await;
-    run(&s, src, &source_ddl(src, &prefix), false).await;
+    let seq = seq_name(src, &prefix);
+    run(s, src, &drops(src, &src_names, &seq), true).await;
+    run(s, tgt, &drops(tgt, &tgt_names, &seq_name(tgt, p)), true).await;
+    let ddl: Vec<String> = source_ddl(src, &prefix)
+        .into_iter()
+        .filter(|sql| src.has_sequences() || !sql.contains("_seq"))
+        .collect();
+    run(s, src, &ddl, false).await;
+    (src_names, tgt_names, seq)
+}
+
+async fn cleanup(s: &Service, src: &End, tgt: &End, p: &str, names: (&[String], &[String], &str)) {
+    run(s, tgt, &drops(tgt, names.1, &seq_name(tgt, p)), true).await;
+    run(s, src, &drops(src, names.0, names.2), true).await;
+}
 
-    let plan = MigrationPlan {
+fn plan(src: &End, tgt: &End, tables: &[String], seq: Option<&str>, data: DataOptions) -> MigrationPlan {
+    MigrationPlan {
         source: MigrationEnd { profile: src.profile.clone(), schema: src.schema.clone() },
-        tables: Some(src_names.iter().rev().cloned().collect()),
-        sequences: Some(vec![]),
+        tables: Some(tables.iter().rev().cloned().collect()),
+        sequences: Some(seq.into_iter().map(String::from).collect()),
         target: MigrationEnd { profile: tgt.profile.clone(), schema: tgt.schema.clone() },
+        mode: Default::default(),
         settings: Default::default(),
+        data,
         existing: BTreeMap::new(),
-    };
+    }
+}
+
+/// 源 → 目标迁移一次并核对。`p`：表名前缀（小写）。
+async fn migrate_flow(src_env: &str, tgt_env: &str, p: &str) {
+    let Some((s, ends, _dir)) = setup([src_env, tgt_env]).await else { return };
+    let (src, tgt) = (&ends[0], &ends[1]);
+    let route = format!("{src_env} → {tgt_env}");
+    let (src_names, tgt_names, seq) = prepare(&s, src, tgt, p).await;
+    let with_seq = src.has_sequences() && tgt.has_sequences();
+    let data = DataOptions { batch_size: 300, parallel: 2, on_error: OnError::Stop, sample: 100 };
+    let plan = plan(src, tgt, &src_names, with_seq.then_some(seq.as_str()), data);
     let preview = s.preview_migration(&plan).await.unwrap();
     let notes: Vec<String> = preview
         .draft
@@ -217,31 +349,138 @@ async fn migrate_flow(src_env: &str, tgt_env: &str, p: &str) {
         .collect();
     eprintln!("{route} 提示：\n{}\n脚本：\n{}", notes.join("\n"), preview.text);
     assert!(preview.problems.is_empty(), "{route}：{:?}", preview.problems);
-    assert_eq!(preview.draft.tables.len(), 2, "{route}");
+    assert_eq!(preview.draft.tables.len(), 3, "{route}");
     let id = s.start_migration(plan).await.unwrap();
     let st = wait(&s, &id).await;
     let failures: Vec<String> = st.failures.iter().map(|f| format!("{}\n  {}", f.text, f.error)).collect();
     assert_eq!(st.state, TaskState::Done, "{route}：{st:?}");
     assert!(failures.is_empty(), "{route} 失败的语句：\n{}", failures.join("\n"));
     let (report, _) = s.tasks.report(&id).unwrap();
-    assert!(std::fs::read_to_string(report).unwrap().contains("```sql"), "{route}");
+    let report = std::fs::read_to_string(report).unwrap();
+    // 数据：每张表都迁完，行数和抽样都一致
+    let m = st.migration.expect("迁移进度");
+    for t in &m.tables {
+        let v =
+            t.verify.as_ref().unwrap_or_else(|| panic!("{route}：{} 没有校验：{t:?}\n{report}", t.source));
+        assert_eq!(t.state, TableState::Done, "{route}：{t:?}");
+        assert!(
+            v.target_rows == v.expected_rows && v.mismatches.is_empty(),
+            "{route}：{} 校验不一致：{v:?}",
+            t.source
+        );
+    }
+    let cust = m.tables.iter().find(|t| t.source.to_lowercase().ends_with("_cust")).unwrap();
+    assert_eq!((cust.rows, cust.verify.as_ref().unwrap().source_rows), (ROWS, ROWS), "{route}");
+    assert!(s.unfinished_migrations().unwrap().is_empty(), "{route}");
 
     // 读回目标结构，与草稿中的目标结构比对
     let tp = s.profile(&tgt.profile).unwrap();
     let dialect: Dialect = s.metadata.dialect(&tp).await.unwrap();
-    let mut actual = Vec::new();
     for t in &preview.draft.tables {
         let mut got: Vec<Table> = s.metadata.tables(&tp, &tgt.schema, Some(&t.target.name)).await.unwrap();
         assert_eq!(got.len(), 1, "{route}：{} 没有建成", t.target.name);
-        actual.push(Table { triggers: vec![], ..got.remove(0) });
+        let got = Table { triggers: vec![], ..got.remove(0) };
+        assert_eq!(
+            shape(&dialect, &t.target, &t.target),
+            shape(&dialect, &got, &t.target),
+            "{route}：{} 读回后与草稿不同",
+            t.target.name
+        );
     }
-    for (t, got) in preview.draft.tables.iter().zip(&actual) {
-        let (want, have) = (shape(&dialect, &t.target, &t.target), shape(&dialect, got, &t.target));
-        assert_eq!(want, have, "{route}：{} 读回后与草稿不同", t.target.name);
+
+    // 自增推进过：插入新行不与迁来的主键冲突；序列接着源序列往下走
+    let ct =
+        &preview.draft.tables.iter().find(|t| t.source.to_lowercase().ends_with("_cust")).unwrap().target;
+    let (q, name) = (dialect.qualified(Some(&tgt.schema), &ct.name), dialect.ident(&ct.columns[1].name));
+    run(&s, tgt, &[format!("INSERT INTO {q} ({name}) VALUES ('new')")], false).await;
+    let id_col = dialect.ident(&ct.columns[0].name);
+    let new_id =
+        number(run(&s, tgt, &[format!("SELECT {id_col} FROM {q} WHERE {name} = 'new'")], false).await);
+    assert!(new_id > ROWS as i64, "{route}：新行的主键 {new_id}");
+    if with_seq {
+        let tseq = dialect.qualified(Some(&tgt.schema), &preview.draft.sequences[0].target.name);
+        let sql = match tgt.driver {
+            DriverId::Postgres => format!("SELECT nextval('{tseq}')"),
+            _ => format!("SELECT {tseq}.NEXTVAL FROM DUAL"),
+        };
+        let next = number(run(&s, tgt, &[sql], false).await);
+        assert!(next > 5, "{route}：序列的下一个值 {next}");
     }
+    cleanup(&s, src, tgt, p, (&src_names, &tgt_names, &seq)).await;
+}
 
-    run(&s, tgt, &drops(tgt, &tgt_names), true).await;
-    run(&s, src, &drops(src, &src_names), true).await;
+/// 续传：小批量迁移，迁了几批后取消；未完成的迁移列出来，继续后全部迁完，没有重复也没有遗漏。
+async fn resume_flow(src_env: &str, tgt_env: &str, p: &str) {
+    let Some((s, ends, _dir)) = setup([src_env, tgt_env]).await else { return };
+    let (src, tgt) = (&ends[0], &ends[1]);
+    let route = format!("{src_env} → {tgt_env}（续传）");
+    let (src_names, tgt_names, seq) = prepare(&s, src, tgt, p).await;
+    let data = DataOptions { batch_size: 100, parallel: 1, on_error: OnError::Stop, sample: 20 };
+    let id = s.start_migration(plan(src, tgt, &src_names, None, data)).await.unwrap();
+    // 大表迁了几批后取消
+    for _ in 0..600 {
+        let st = s.tasks.status(&id).unwrap();
+        let copied =
+            st.migration.as_ref().map(|m| m.tables.iter().map(|t| t.rows).max().unwrap_or(0)).unwrap_or(0);
+        if copied >= 200 || st.state != TaskState::Running {
+            break;
+        }
+        tokio::time::sleep(Duration::from_millis(20)).await;
+    }
+    s.tasks.cancel_or_remove(&id).await.unwrap();
+    let st = wait(&s, &id).await;
+    assert_eq!(st.state, TaskState::Cancelled, "{route}：{st:?}");
+    let list = s.unfinished_migrations().unwrap();
+    assert_eq!(list.len(), 1, "{route}");
+    let id = s.resume_migration(&list[0].id).await.unwrap();
+    let st = wait(&s, &id).await;
+    assert_eq!(st.state, TaskState::Done, "{route}：{st:?}");
+    for t in &st.migration.unwrap().tables {
+        let v = t.verify.as_ref().unwrap();
+        assert!(
+            t.state == TableState::Done && v.target_rows == v.source_rows && v.mismatches.is_empty(),
+            "{route}：{t:?}"
+        );
+    }
+    assert!(s.unfinished_migrations().unwrap().is_empty(), "{route}");
+    cleanup(&s, src, tgt, p, (&src_names, &tgt_names, &seq)).await;
+}
+
+/// 跳过出错的行：订单状态列改成目标上很短的类型，超长的值写不进去；选"跳过并记录"时其余行照常迁，跳过的行记进报告；
+/// 再以"清空后导入"迁一次，结果相同。
+async fn skip_flow(src_env: &str, tgt_env: &str, p: &str) {
+    let Some((s, ends, _dir)) = setup([src_env, tgt_env]).await else { return };
+    let (src, tgt) = (&ends[0], &ends[1]);
+    let route = format!("{src_env} → {tgt_env}（跳过）");
+    let (src_names, tgt_names, seq) = prepare(&s, src, tgt, p).await;
+    let ord = src_names[0].clone();
+    let short = if tgt.upper() { "VARCHAR2(3)" } else { "VARCHAR(3)" };
+    let status = if src.upper() { "STATUS" } else { "status" };
+    let data = DataOptions { batch_size: 1000, parallel: 2, on_error: OnError::Skip, sample: 0 };
+    let mut pl = plan(src, tgt, std::slice::from_ref(&ord), None, data);
+    pl.settings
+        .tables
+        .entry(ord.clone())
+        .or_default()
+        .columns
+        .insert(status.into(), ColumnOverride { data_type: Some(short.into()), ..Default::default() });
+    let id = s.start_migration(pl.clone()).await.unwrap();
+    let st = wait(&s, &id).await;
+    assert_eq!(st.state, TaskState::Done, "{route}：{st:?}");
+    let t = &st.migration.unwrap().tables[0];
+    // 'paid' 超长被跳过，'new' 写入
+    assert_eq!((t.rows, t.skipped), (1, 1), "{route}：{t:?}");
+    assert!(t.verify.as_ref().unwrap().target_rows == 1, "{route}：{t:?}");
+    let (report, _) = s.tasks.report(&id).unwrap();
+    assert!(std::fs::read_to_string(report).unwrap().contains(&format!("{ord} (1)")), "{route}");
+    // 清空后导入：仍是 1 行
+    pl.existing.insert(ord.clone(), ExistingAction::Replace);
+    let id = s.start_migration(pl).await.unwrap();
+    let st = wait(&s, &id).await;
+    let t = &st.migration.unwrap().tables[0];
+    let v = t.verify.as_ref().unwrap();
+    assert_eq!((v.expected_rows, v.target_rows, t.skipped), (1, 1, 1), "{route}：{t:?}");
+    cleanup(&s, src, tgt, p, (&src_names, &tgt_names, &seq)).await;
 }
 
 /// 表结构的要点（核对用）：列（名字、逻辑类型、可空、有无默认值、自增、注释）、主键、唯一约束、
@@ -361,3 +600,26 @@ routes! {
     kingbase_pg_to_oracle: "DBLENS_TEST_KINGBASE_PG_URL" => "DBLENS_TEST_ORACLE_URL", "m5kp";
     kingbase_pg_to_dameng: "DBLENS_TEST_KINGBASE_PG_URL" => "DBLENS_TEST_DM_URL", "m5kp";
 }
+
+/// 续传和跳过：每种目标家族各一条。
+macro_rules! special {
+    ($($name:ident: $flow:ident, $src:literal => $tgt:literal, $p:literal;)*) => {
+        $(
+            #[tokio::test]
+            async fn $name() {
+                $flow($src, $tgt, $p).await;
+            }
+        )*
+    };
+}
+
+special! {
+    resume_pg_to_mysql: resume_flow, "DBLENS_TEST_PG_URL" => "DBLENS_TEST_MYSQL_URL", "m5rp";
+    resume_mysql_to_oracle: resume_flow, "DBLENS_TEST_MYSQL_URL" => "DBLENS_TEST_ORACLE_URL", "m5rm";
+    resume_oracle_to_dameng: resume_flow, "DBLENS_TEST_ORACLE_URL" => "DBLENS_TEST_DM_URL", "m5ro";
+    resume_dameng_to_pg: resume_flow, "DBLENS_TEST_DM_URL" => "DBLENS_TEST_PG_URL", "m5rd";
+    skip_pg_to_mysql: skip_flow, "DBLENS_TEST_PG_URL" => "DBLENS_TEST_MYSQL_URL", "m5sp";
+    skip_mysql_to_oracle: skip_flow, "DBLENS_TEST_MYSQL_URL" => "DBLENS_TEST_ORACLE_URL", "m5sm";
+    skip_oracle_to_pg: skip_flow, "DBLENS_TEST_ORACLE_URL" => "DBLENS_TEST_PG_URL", "m5so";
+    skip_pg_to_dameng: skip_flow, "DBLENS_TEST_PG_URL" => "DBLENS_TEST_DM_URL", "m5sd";
+}
````

- [ ] **Step 3: 修改 `crates/dblens-api/src/lib.rs`**

```diff
--- a/crates/dblens-api/src/lib.rs
+++ b/crates/dblens-api/src/lib.rs
@@ -119,7 +119,9 @@ pub fn router(service: Arc<Service>, config: ApiConfig) -> Router {
         .route("/documents", post(documents::start))
         .route("/documents/check", post(documents::check))
         .route("/documents/templates/export", post(documents::export_template))
-        .route("/migrations", post(migrations::start))
+        .route("/migrations", get(migrations::unfinished).post(migrations::start))
+        .route("/migrations/{id}/resume", post(migrations::resume))
+        .route("/migrations/{id}", delete(migrations::discard))
         .route("/migrations/preview", post(migrations::preview))
         .route("/exports", post(exports::create))
         .route("/exports/{id}", get(exports::status).delete(exports::cancel_or_remove))
```

- [ ] **Step 4: 修改 `crates/dblens-api/src/migrations.rs`**

```diff
--- a/crates/dblens-api/src/migrations.rs
+++ b/crates/dblens-api/src/migrations.rs
@@ -1,9 +1,10 @@
-//! 结构迁移的接口（M5a 设计文档 3.2 节）：预览（转换结果、脚本、开始前检查），开始迁移。
+//! 迁移的接口（M5a 设计文档 3.2 节、M5b 设计文档第 7 节）：预览（转换结果、脚本、开始前检查）、开始迁移；
+//! 没有完成的迁移：列出、继续、放弃。
 
 use axum::Json;
-use axum::extract::State;
+use axum::extract::{Path, State};
 use axum::http::StatusCode;
-use dblens_service::{MigrationPlan, MigrationPreview};
+use dblens_service::{MigrationPlan, MigrationPreview, MigrationSummary};
 use serde_json::{Value, json};
 
 use crate::AppState;
@@ -25,3 +26,23 @@ pub async fn start(
     let id = s.service.start_migration(plan).await?;
     Ok((StatusCode::CREATED, Json(json!({ "id": id }))))
 }
+
+/// 没有完成的迁移。
+pub async fn unfinished(State(s): State<AppState>) -> ApiResult<Json<Vec<MigrationSummary>>> {
+    Ok(Json(s.service.unfinished_migrations()?))
+}
+
+/// 继续一个没有完成的迁移：返回任务 id。
+pub async fn resume(
+    State(s): State<AppState>,
+    Path(id): Path<String>,
+) -> ApiResult<(StatusCode, Json<Value>)> {
+    let task = s.service.resume_migration(&id).await?;
+    Ok((StatusCode::CREATED, Json(json!({ "id": task }))))
+}
+
+/// 放弃：删除迁移记录（不动目标库）。
+pub async fn discard(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult<StatusCode> {
+    s.service.discard_migration(&id)?;
+    Ok(StatusCode::NO_CONTENT)
+}
```

- [ ] **Step 5: 修改 `crates/dblens-core/locales/app.json`**

```diff
--- a/crates/dblens-core/locales/app.json
+++ b/crates/dblens-core/locales/app.json
@@ -150,7 +150,11 @@
   "metadata.not_source": {"zh-CN": "%{key} 不是源码查询", "en": "%{key} is not a source query"},
   "metadata.pool_closed": {"zh-CN": "连接池已关闭", "en": "The connection pool is closed"},
   "metadata.table_detail_count": {"zh-CN": "表明细查询数量不对", "en": "Wrong number of table detail queries"},
+  "migrate.already_finished": {"zh-CN": "这个迁移已经完成", "en": "This migration has already finished"},
+  "migrate.bad_data_options": {"zh-CN": "数据选项超出范围：每批 100–100000 行、并行 1–4 张表、抽样至多 10000 行", "en": "Data options out of range: batch 100–100000 rows, 1–4 tables in parallel, sample at most 10000 rows"},
+  "migrate.bad_value": {"zh-CN": "列 %{column} 的值不能转换：%{error}", "en": "The value of column %{column} cannot be converted: %{error}"},
   "migrate.check_dropped": {"zh-CN": "检查约束 %{expression} 不能转换，不迁移", "en": "Check constraint %{expression} cannot be converted; not migrated"},
+  "migrate.data_needs_table": {"zh-CN": "只迁数据时目标中必须已有这些表：%{tables}", "en": "Migrating data only requires these tables to exist in the target: %{tables}"},
   "migrate.default_dropped": {"zh-CN": "默认值 %{default} 不能转换，不迁移", "en": "Default %{default} cannot be converted; not migrated"},
   "migrate.existing_unchosen": {"zh-CN": "目标中已有这些表，请选择处理方式：%{tables}", "en": "These tables already exist in the target; choose what to do: %{tables}"},
   "migrate.fk_action": {"zh-CN": "目标库不支持 %{action}，去掉", "en": "The target does not support %{action}; dropped"},
@@ -163,21 +167,36 @@
   "migrate.key_column_skipped": {"zh-CN": "主键的列没有迁移", "en": "A primary key column is not migrated"},
   "migrate.lob_index": {"zh-CN": "包含大字段列，目标库不能建索引，不迁移", "en": "Includes a large-object column the target cannot index; not migrated"},
   "migrate.lob_key": {"zh-CN": "主键包含大字段列，目标库不能建", "en": "The primary key includes a large-object column, which the target cannot index"},
+  "migrate.lob_too_large": {"zh-CN": "列 %{column} 的大字段超过 256 MB，没有读全", "en": "A large value in column %{column} exceeds 256 MB and was not read completely"},
   "migrate.name_conflict": {"zh-CN": "名字 %{name} 与 %{other} 重名", "en": "Name %{name} conflicts with %{other}"},
   "migrate.name_too_long": {"zh-CN": "名字 %{name} 超过目标库的长度上限", "en": "Name %{name} exceeds the target's length limit"},
+  "migrate.no_key_append": {"zh-CN": "表 %{table} 没有主键，追加数据时不能续传；请改为清空后导入或重新迁移", "en": "Table %{table} has no primary key, so appending cannot resume; use replace or start over"},
   "migrate.no_sequences": {"zh-CN": "目标库没有序列，不迁移", "en": "The target database has no sequences; not migrated"},
+  "migrate.not_found": {"zh-CN": "找不到迁移记录 %{id}", "en": "Migration record %{id} not found"},
   "migrate.nothing": {"zh-CN": "没有要迁移的表或序列", "en": "There are no tables or sequences to migrate"},
   "migrate.partition": {"zh-CN": "分区不迁移，按普通表建", "en": "Partitioning is not migrated; created as a plain table"},
+  "migrate.record_io": {"zh-CN": "读写迁移记录失败：%{error}", "en": "Failed to read or write the migration record: %{error}"},
+  "migrate.report.adjustments": {"zh-CN": "序列和自增的调整", "en": "Sequence and identity adjustments"},
   "migrate.report.cancelled": {"zh-CN": "迁移已取消，之后的语句没有执行。", "en": "Migration cancelled; later statements were not run."},
+  "migrate.report.data": {"zh-CN": "数据", "en": "Data"},
+  "migrate.report.data_header": {"zh-CN": "表 | 状态 | 源行数 | 已迁行数 | 跳过 | 用时 | 校验", "en": "Table | State | Source rows | Rows copied | Skipped | Time | Verification"},
   "migrate.report.failed": {"zh-CN": "迁移中断：%{error}", "en": "Migration stopped: %{error}"},
   "migrate.report.failures": {"zh-CN": "失败的语句", "en": "Failed statements"},
   "migrate.report.finished": {"zh-CN": "迁移已完成。", "en": "Migration finished."},
+  "migrate.report.mismatches": {"zh-CN": "%{table} 抽样比对不一致", "en": "Sample differences in %{table}"},
   "migrate.report.no_ddl": {"zh-CN": "（取不到 DDL）", "en": "(DDL unavailable)"},
   "migrate.report.notes": {"zh-CN": "映射提示", "en": "Mapping notes"},
   "migrate.report.others": {"zh-CN": "没有迁移的对象（附源库 DDL，需人工改写）", "en": "Objects not migrated (source DDL attached; rewrite by hand)"},
+  "migrate.report.row_missing": {"zh-CN": "目标中没有这一行", "en": "Row missing in the target"},
   "migrate.report.script": {"zh-CN": "迁移脚本", "en": "Migration script"},
-  "migrate.report.summary": {"zh-CN": "共 %{total} 条语句：执行 %{done} 条，其中失败 %{failed} 条、因依赖的表没有建成而跳过 %{skipped} 条；用时 %{seconds} 秒。", "en": "%{total} statements: %{done} run, %{failed} failed, %{skipped} skipped because a table they depend on was not created; %{seconds} s."},
+  "migrate.report.skipped_rows": {"zh-CN": "跳过的行", "en": "Skipped rows"},
+  "migrate.report.state": {"zh-CN": "%{state}", "en": "%{state}"},
+  "migrate.report.statements": {"zh-CN": "结构语句共 %{total} 条：成功 %{done} 条，失败 %{failed} 条。", "en": "%{total} structure statements: %{done} succeeded, %{failed} failed."},
   "migrate.report.title": {"zh-CN": "结构迁移：%{source} → %{target}", "en": "Structure migration: %{source} → %{target}"},
+  "migrate.report.unfinished": {"zh-CN": "迁移没有全部完成（有搬运失败的表），修改后可在迁移标签中继续。", "en": "The migration did not finish completely (some tables failed); fix them and continue from the migration tab."},
+  "migrate.report.verify_bad": {"zh-CN": "不一致：应有 %{expected} 行，目标 %{target} 行，%{mismatches} 处值不同", "en": "Mismatch: %{expected} rows expected, %{target} in target, %{mismatches} differing values"},
+  "migrate.report.verify_ok": {"zh-CN": "一致（抽样 %{sampled} 行）", "en": "Matched (%{sampled} rows sampled)"},
+  "migrate.running": {"zh-CN": "这个迁移正在执行", "en": "This migration is already running"},
   "migrate.same_target": {"zh-CN": "源和目标是同一个 schema", "en": "The source and target are the same schema"},
   "migrate.sequence_bound": {"zh-CN": "%{bound} 超出 bigint 范围", "en": "%{bound} is outside the bigint range"},
   "migrate.sequence_exists": {"zh-CN": "目标中已有同名序列，跳过", "en": "A sequence with this name already exists in the target; skipped"},
```

- [ ] **Step 6: 修改 `crates/dblens-service/src/import/job.rs`**

```diff
--- a/crates/dblens-service/src/import/job.rs
+++ b/crates/dblens-service/src/import/job.rs
@@ -8,7 +8,7 @@ use dblens_core::schema::{Column, PrimaryKey, Table};
 use dblens_core::{ColumnType, Error, LogicalType, Result, Value, msg};
 use dblens_dialect::{Dialect, ImportStatement, WriteMode};
 use dblens_driver::{Connection, ExecEvent};
-use serde::Deserialize;
+use serde::{Deserialize, Serialize};
 use tokio::sync::mpsc;
 
 use super::file::{FileOptions, FileReader, FileRow};
@@ -62,7 +62,7 @@ pub enum Transaction {
     Batch,
 }
 
-#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
+#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
 #[serde(rename_all = "snake_case")]
 pub enum OnError {
     #[default]
```

- [ ] **Step 7: 修改 `crates/dblens-service/src/lib.rs`**

```diff
--- a/crates/dblens-service/src/lib.rs
+++ b/crates/dblens-service/src/lib.rs
@@ -46,7 +46,10 @@ pub use import::{
     infer_type, new_table, preview_file,
 };
 pub use metadata::{METADATA_POOL_SIZE, MetadataCache, MetadataService};
-pub use migrate::{MigrationEnd, MigrationPlan, MigrationPreview, OtherObject};
+pub use migrate::{
+    DataOptions, MigrationEnd, MigrationMode, MigrationPhase, MigrationPlan, MigrationPreview,
+    MigrationProgress, MigrationSummary, Mismatch, OtherObject, TableState, TableView, Verify,
+};
 pub use model::{
     DEFAULT_DIAGRAM, Diagram, DiagramDisplay, DiagramSummary, DiagramTable, DiagramTables, DiagramView,
     DiagramWarning, FileError, LogicalKey, ModelDatabase, ModelDesignPreview, ModelDesignRequest, ModelInfo,
```

- [ ] **Step 8: 新建 `crates/dblens-service/src/migrate/copy.rs`**

```rust
//! 一张表的数据搬运（M5b 设计文档第 4 节）：源库一条按主键排序的流式查询（续传时从上次的主键之后读），
//! 按目标列的类型转换，参数化的多行 INSERT 按批写入、每批一个事务，提交后更新迁移记录。
//! 出错时按方案停止（这张表失败）或跳过（出错的批逐行重试，失败的行记进报告）。

use dblens_core::schema::Table;
use dblens_core::{Error, Result, Row, Value, msg};
use dblens_dialect::WriteMode;
use dblens_dialect::migrate::data::{TableRead, identity_insert_sql, keyset_params};
use dblens_driver::{Connection, ExecEvent};

use super::record::{MAX_SKIPPED_ROWS, SkippedRow, TableCopy, TableState};
use super::run::{Ctx, as_u64, scalar};
use super::values::{key_text, to_target};
use crate::export::EXPORT_LOB_LIMIT;
use crate::import::{OnError, exec};

/// 搬运一张表。返回错误时这张表停在最后提交的批之后（取消时为 `Cancelled`）。
pub(crate) async fn copy_table(ctx: &Ctx, c: &TableCopy) -> Result<()> {
    let mut src = ctx.connect_source().await?;
    src.set_lob_limit(EXPORT_LOB_LIMIT);
    let mut tgt = match ctx.connect_target().await {
        Ok(t) => t,
        Err(e) => {
            let _ = src.close().await;
            return Err(e);
        }
    };
    let result = copy(ctx, c, &mut src, &mut tgt).await;
    if result.is_err() {
        let _ = tgt.rollback().await;
    }
    let _ = src.close().await;
    let _ = tgt.close().await;
    result
}

async fn copy(
    ctx: &Ctx,
    c: &TableCopy,
    src: &mut Box<dyn Connection>,
    tgt: &mut Box<dyn Connection>,
) -> Result<()> {
    let (sd, td) = (ctx.source_dialect, ctx.target_dialect);
    let (ss, ts) = (ctx.source_schema()?, ctx.target_schema()?);
    let (state, last, batch_size, on_error) = ctx.with(|r| {
        let p = &r.tables[&c.source];
        (p.state, p.last_key.clone(), r.plan.data.batch_size.clamp(100, 100_000), r.plan.data.on_error)
    })?;
    let key_names: Vec<String> = c.key.iter().map(|&i| c.source_columns[i].clone()).collect();
    let resumable = !c.key.is_empty();
    // 第一次搬运，或没有主键的表重迁：从头开始
    let fresh = state == TableState::Pending || !resumable || last.is_none();
    if fresh {
        if state != TableState::Pending && c.append && !resumable {
            return Err(Error::invalid(msg!("migrate.no_key_append", table = c.source)));
        }
        let target_read = TableRead { schema: &ts, table: &c.target, columns: &[], key: &[], filter: None };
        if c.replace || (state != TableState::Pending && !c.append) {
            exec(tgt, &format!("DELETE FROM {}", td.qualified(Some(&ts), &c.target)), &[]).await?;
        }
        let base =
            if c.append { Some(as_u64(scalar(tgt, &target_read.count(&td), &[]).await?)) } else { None };
        ctx.with(|r| {
            if let Some(p) = r.tables.get_mut(&c.source) {
                if !(c.append && state != TableState::Pending) {
                    p.base_rows = base;
                }
                p.rows = 0;
                p.skipped = 0;
                p.last_key = None;
            }
            r.skipped_rows.retain(|s| s.table != c.source);
        })?;
    }
    ctx.with(|r| {
        if let Some(p) = r.tables.get_mut(&c.source) {
            p.state = TableState::Copying;
            p.error = None;
        }
    })?;
    ctx.save()?;
    // 达梦写入自增列要先打开开关（只对有自增列的表）
    let identity_insert = c.identity.as_ref().and_then(|_| identity_insert_sql(&td, &ts, &c.target, true));
    if let Some(sql) = &identity_insert {
        exec(tgt, sql, &[]).await?;
    }
    tgt.set_autocommit(false).await?;

    let read = TableRead {
        schema: &ss,
        table: &c.source,
        columns: &c.source_columns,
        key: &key_names,
        filter: c.filter.as_deref(),
    };
    let last = if fresh { None } else { last };
    let params = last.as_deref().map(keyset_params).unwrap_or_default();
    let table = Table { name: c.target.clone(), columns: c.target_columns.clone(), ..Default::default() };
    let names: Vec<String> = c.target_columns.iter().map(|x| x.name.clone()).collect();
    let writer = Writer { ctx, c, table: &table, names: &names, schema: &ts, on_error };
    let sql = read.select(&sd, last.as_deref());
    let mut stream = src.execute(&sql, &params).await?;
    let mut batch: Vec<Row> = Vec::with_capacity(batch_size);
    loop {
        if ctx.task.cancelled() {
            return Err(Error::Cancelled);
        }
        match stream.next(batch_size).await? {
            Some(ExecEvent::Rows(rows)) => {
                batch.extend(rows);
                if batch.len() >= batch_size {
                    writer.write(tgt, &batch).await?;
                    batch.clear();
                }
            }
            Some(_) => {}
            None => break,
        }
    }
    drop(stream);
    if !batch.is_empty() {
        writer.write(tgt, &batch).await?;
    }
    tgt.set_autocommit(true).await?;
    if identity_insert.is_some()
        && let Some(sql) = identity_insert_sql(&td, &ts, &c.target, false)
    {
        exec(tgt, &sql, &[]).await?;
    }
    Ok(())
}

struct Writer<'a> {
    ctx: &'a Ctx,
    c: &'a TableCopy,
    table: &'a Table,
    names: &'a [String],
    schema: &'a str,
    on_error: OnError,
}

impl Writer<'_> {
    fn convert(&self, row: &Row) -> Result<Vec<Value>> {
        row.iter().zip(&self.c.target_columns).map(|(v, col)| to_target(v, col)).collect()
    }

    fn key(&self, row: &Row) -> Vec<Value> {
        self.c.key.iter().map(|&i| row[i].clone()).collect()
    }

    /// 写一组行（一个事务）。
    async fn insert(&self, tgt: &mut Box<dyn Connection>, rows: &[Vec<Value>]) -> Result<()> {
        let td = self.ctx.target_dialect;
        let result = async {
            for s in td.import_statements(self.schema, self.table, self.names, rows, WriteMode::Append)? {
                exec(tgt, &s.sql, &s.params).await?;
            }
            tgt.commit().await
        }
        .await;
        if result.is_err() {
            let _ = tgt.rollback().await;
        }
        result
    }

    /// 写一批：成功后记下行数和最后的主键。跳过模式下出错的批逐行重试。
    async fn write(&self, tgt: &mut Box<dyn Connection>, batch: &[Row]) -> Result<()> {
        let converted: Result<Vec<Vec<Value>>> = batch.iter().map(|r| self.convert(r)).collect();
        let whole = match converted {
            Ok(rows) => self.insert(tgt, &rows).await,
            Err(e) => Err(e),
        };
        let (written, skipped) = match whole {
            Ok(()) => (batch.len() as u64, vec![]),
            Err(e) if self.on_error == OnError::Stop => return Err(e),
            Err(_) => {
                let mut written = 0;
                let mut skipped = Vec::new();
                for row in batch {
                    let r = match self.convert(row) {
                        Ok(v) => self.insert(tgt, &[v]).await,
                        Err(e) => Err(e),
                    };
                    match r {
                        Ok(()) => written += 1,
                        Err(e) => skipped.push(SkippedRow {
                            table: self.c.source.clone(),
                            key: key_text(&self.key(row)),
                            error: e.to_string(),
                        }),
                    }
                }
                (written, skipped)
            }
        };
        let last = batch.last().map(|r| self.key(r));
        self.ctx.with(|r| {
            if let Some(p) = r.tables.get_mut(&self.c.source) {
                p.rows += written;
                p.skipped += skipped.len() as u64;
                if !self.c.key.is_empty() {
                    p.last_key = last;
                }
            }
            let room = MAX_SKIPPED_ROWS.saturating_sub(r.skipped_rows.len());
            r.skipped_rows.extend(skipped.into_iter().take(room));
        })?;
        self.ctx.save()
    }
}
```

- [ ] **Step 9: 修改 `crates/dblens-service/src/migrate/mod.rs`**

````diff
rename from crates/dblens-service/src/migrate.rs
rename to crates/dblens-service/src/migrate/mod.rs
index 57df8cf..7904f1e 100644
--- a/crates/dblens-service/src/migrate.rs
+++ b/crates/dblens-service/src/migrate/mod.rs
@@ -1,25 +1,34 @@
-//! 结构迁移（M5a 设计文档第 3.2、6 节）：读源和目标的结构，按迁移方案转换、检查、生成脚本；
-//! 作为任务逐条执行（某张表建表失败时跳过依赖它的语句），最后写迁移报告（markdown）。转换本身在 `dblens_dialect::migrate`。
+//! 结构和数据迁移（M5a 设计文档第 3.2、6 节，M5b 设计文档）：读源和目标的结构，按迁移方案转换、检查、生成脚本；
+//! 开始时建迁移记录（`record`），作为任务依次建表、迁数据、建约束、调整序列和自增、校验（`run`、`copy`），
+//! 最后写迁移报告（`report`）。没有完成的迁移可以继续或放弃。结构的转换在 `dblens_dialect::migrate`。
+
+mod copy;
+mod record;
+mod report;
+mod run;
+mod values;
 
 use std::collections::{BTreeMap, BTreeSet};
-use std::fmt::Write as _;
-use std::sync::Arc;
-use std::time::Instant;
+use std::sync::{Arc, Mutex};
 
 use dblens_core::schema::{RoutineKind, Sequence, Table};
-use dblens_core::{Error, Lang, Msg, Result, current_lang, msg};
-use dblens_dialect::meta::{MetaArgs, MetaKey};
+use dblens_core::{Error, Msg, Result, current_lang, msg, scope_lang};
+use dblens_dialect::meta::{self, MetaArgs, MetaKey};
 use dblens_dialect::migrate::{
     ExistingAction, MigrationDraft, MigrationInput, MigrationSettings, MigrationStatement, NoteLevel,
-    StatementKind, migrate, migration_script, script_text,
+    migrate, migration_script, script_text,
 };
 use dblens_dialect::{DdlKind, Dialect, Variant};
 use serde::{Deserialize, Serialize};
 
-use crate::design::Run;
+use crate::import::OnError;
 use crate::profile::Profile;
 use crate::service::Service;
-use crate::task::{TaskFailure, TaskHandle, TaskKind, TaskPhase};
+use crate::task::{TaskKind, TaskPhase};
+pub use record::{MigrationPhase, MigrationRecords, MigrationSummary, Mismatch, TableState, Verify};
+use record::{MigrationRecord, TableCopy, TableProgress, now_ms};
+use run::Ctx;
+pub use run::{MigrationProgress, TableView};
 
 /// 迁移的一端：连接和 schema。
 #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
@@ -28,8 +37,45 @@ pub struct MigrationEnd {
     pub schema: String,
 }
 
+/// 迁移的模式（M5b 设计文档 3.1 节）：结构和数据、只迁结构、只迁数据（目标表必须已存在）。
+#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
+#[serde(rename_all = "snake_case")]
+pub enum MigrationMode {
+    #[default]
+    All,
+    Structure,
+    Data,
+}
+
+/// 迁数据的选项：每批行数（100–100000）、并行表数（1–4）、出错时停止或跳过、每张表抽样比对的行数（0 为不抽样，至多 10000）。
+#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
+pub struct DataOptions {
+    #[serde(default = "default_batch")]
+    pub batch_size: usize,
+    #[serde(default = "default_parallel")]
+    pub parallel: usize,
+    #[serde(default)]
+    pub on_error: OnError,
+    #[serde(default)]
+    pub sample: u64,
+}
+
+fn default_batch() -> usize {
+    1000
+}
+
+fn default_parallel() -> usize {
+    2
+}
+
+impl Default for DataOptions {
+    fn default() -> Self {
+        Self { batch_size: default_batch(), parallel: default_parallel(), on_error: OnError::Stop, sample: 0 }
+    }
+}
+
 /// 迁移方案（M5a 设计文档 3.3 节）：可另存为 JSON 再打开。`tables`、`sequences` 为 None 时迁移源 schema 的全部表、序列；
-/// `existing`：目标中已有同名表时的处理方式（按源表名）。
+/// `existing`：目标中已有同名表时的处理方式（按源表名）。`mode`、`data` 见 M5b 设计文档。
 #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
 pub struct MigrationPlan {
     pub source: MigrationEnd,
@@ -40,18 +86,24 @@ pub struct MigrationPlan {
     pub sequences: Option<Vec<String>>,
     pub target: MigrationEnd,
     #[serde(default)]
+    pub mode: MigrationMode,
+    #[serde(default)]
     pub settings: MigrationSettings,
     #[serde(default)]
+    pub data: DataOptions,
+    #[serde(default)]
     pub existing: BTreeMap<String, ExistingAction>,
 }
 
-/// 不迁移的对象（视图、例程、触发器、同义词）。触发器的 `table` 是所属的表。
-#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
+/// 不迁移的对象（视图、例程、触发器、同义词）。触发器的 `table` 是所属的表，`definition` 是它的定义。
+#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
 pub struct OtherObject {
     pub kind: String,
     pub name: String,
-    #[serde(skip_serializing_if = "Option::is_none")]
+    #[serde(default, skip_serializing_if = "Option::is_none")]
     pub table: Option<String>,
+    #[serde(default, skip_serializing_if = "Option::is_none")]
+    pub definition: Option<String>,
 }
 
 /// 预览：转换结果、脚本、开始前检查的问题、执行策略的结论。
@@ -107,6 +159,17 @@ fn problems(plan: &MigrationPlan, draft: &MigrationDraft) -> Vec<Msg> {
     if !unchosen.is_empty() {
         out.push(msg!("migrate.existing_unchosen", tables = unchosen.join(", ")));
     }
+    if plan.mode == MigrationMode::Data {
+        let missing: Vec<&str> =
+            draft.tables.iter().filter(|t| !t.exists).map(|t| t.source.as_str()).collect();
+        if !missing.is_empty() {
+            out.push(msg!("migrate.data_needs_table", tables = missing.join(", ")));
+        }
+    }
+    let d = &plan.data;
+    if !(100..=100_000).contains(&d.batch_size) || !(1..=4).contains(&d.parallel) || d.sample > 10_000 {
+        out.push(msg!("migrate.bad_data_options"));
+    }
     out
 }
 
@@ -172,7 +235,12 @@ impl Service {
         tables: &[Table],
     ) -> Result<Vec<OtherObject>> {
         let mut out = Vec::new();
-        let obj = |kind: DdlKind, name: String| OtherObject { kind: kind.as_str().into(), name, table: None };
+        let obj = |kind: DdlKind, name: String| OtherObject {
+            kind: kind.as_str().into(),
+            name,
+            table: None,
+            definition: None,
+        };
         for (key, kind) in
             [(MetaKey::Views, DdlKind::View), (MetaKey::MaterializedViews, DdlKind::MaterializedView)]
         {
@@ -208,6 +276,7 @@ impl Service {
                     kind: "trigger".into(),
                     name: tr.name.clone(),
                     table: Some(t.name.clone()),
+                    definition: tr.definition.clone(),
                 });
             }
         }
@@ -259,7 +328,7 @@ impl Service {
         Ok(self.prepare_migration(plan, false).await?.1)
     }
 
-    /// 开始迁移：以库中最新的结构重新转换，检查不通过则拒绝；作为任务执行，返回任务 id。
+    /// 开始迁移：以库中最新的结构重新转换，检查不通过则拒绝；建迁移记录，作为任务执行，返回任务 id。
     pub async fn start_migration(self: &Arc<Self>, plan: MigrationPlan) -> Result<String> {
         let (l, preview) = self.prepare_migration(&plan, true).await?;
         if let Some(p) = preview.problems.first() {
@@ -268,200 +337,234 @@ impl Service {
         if let Some(reason) = preview.denied.clone() {
             return Err(Error::Denied { reason });
         }
-        let password = self.secrets.get(&l.target.id)?.unwrap_or_default();
-        let task = self.tasks.create(TaskKind::Migrate)?;
-        let mut id = String::new();
-        task.update(|s| {
-            id = s.id.clone();
-            s.phase = TaskPhase::Executing;
-            s.progress.statements_total = preview.statements.len() as u64;
-        });
-        let lang = current_lang();
-        let service = self.clone();
-        tokio::spawn(async move {
-            let started = Instant::now();
-            let run = Run {
-                service: &service,
-                task: &task,
-                profile: &l.target,
-                dialect: l.target_dialect,
-                schema: &plan.target.schema,
-            };
-            let outcome = run_statements(&run, &preview.statements, password).await;
-            service.metadata.cache().invalidate(&l.target.id, Some(&plan.target.schema));
-            let report = service.migration_report(&plan, &l, &preview, &task, &outcome, started, lang).await;
-            let result = match (outcome, report) {
-                (Err(e), _) | (Ok(_), Err(e)) => Err(e),
-                (Ok(_), Ok(())) => Ok(()),
-            };
-            task.finish(result);
-        });
-        Ok(id)
+        let record = self.new_record(plan, &l, preview).await?;
+        self.migrations.save(&record)?;
+        self.launch(record).await
     }
 
-    /// 写迁移报告：结果汇总、失败的语句、提示、不迁移的对象及其源库 DDL。
-    #[allow(clippy::too_many_arguments)]
-    async fn migration_report(
+    /// 由预览建迁移记录：每张要迁数据的表的搬运参数和行数估计。
+    async fn new_record(
         &self,
-        plan: &MigrationPlan,
+        plan: MigrationPlan,
         l: &Loaded,
-        preview: &MigrationPreview,
-        task: &TaskHandle,
-        outcome: &Result<Outcome>,
-        started: Instant,
-        lang: Lang,
-    ) -> Result<()> {
-        let t = |m: Msg| m.render(lang);
-        let mut md = String::new();
-        let src = format!("{} / {}", l.source.input.name, plan.source.schema);
-        let tgt = format!("{} / {}", l.target.input.name, plan.target.schema);
-        let _ = writeln!(md, "# {}\n", t(msg!("migrate.report.title", source = &src, target = &tgt)));
-        let status = task.status();
-        let state = match outcome {
-            Ok(_) => t(msg!("migrate.report.finished")),
-            Err(Error::Cancelled) => t(msg!("migrate.report.cancelled")),
-            Err(e) => t(msg!("migrate.report.failed", error = e.to_string())),
-        };
-        let _ = writeln!(md, "{state}\n");
-        let p = &status.progress;
-        let skipped = outcome.as_ref().map(|o| o.skipped).unwrap_or(0);
-        let _ = writeln!(
-            md,
-            "{}\n",
-            t(msg!(
-                "migrate.report.summary",
-                done = p.statements_done,
-                failed = p.statements_failed,
-                skipped = skipped,
-                total = p.statements_total,
-                seconds = started.elapsed().as_secs()
-            ))
-        );
-        if !status.failures.is_empty() {
-            let _ = writeln!(md, "## {}\n", t(msg!("migrate.report.failures")));
-            for f in &status.failures {
-                let _ = writeln!(md, "```sql\n{}\n```\n\n{}\n", f.text, f.error);
+        preview: MigrationPreview,
+    ) -> Result<MigrationRecord> {
+        let lang = current_lang();
+        let mut copies = Vec::new();
+        let mut tables = BTreeMap::new();
+        for t in &preview.draft.tables {
+            let action =
+                if t.exists { plan.existing.get(&t.source).copied() } else { Some(ExistingAction::Recreate) };
+            if plan.mode == MigrationMode::Structure || !action.is_some_and(ExistingAction::copies_data) {
+                continue;
             }
+            let Some(source) = l.tables.iter().find(|x| x.name == t.source) else { continue };
+            let ov = plan.settings.tables.get(&t.source);
+            let mut source_columns = Vec::new();
+            let mut target_columns = Vec::new();
+            for m in &t.columns {
+                let Some(col) =
+                    m.target.as_ref().and_then(|n| t.target.columns.iter().find(|c| &c.name == n))
+                else {
+                    continue;
+                };
+                if col.generated.is_some() {
+                    continue;
+                }
+                source_columns.push(m.source.clone());
+                target_columns.push(col.clone());
+            }
+            let pk = source.primary_key.as_ref().map(|k| k.columns.clone()).unwrap_or_default();
+            let key: Option<Vec<usize>> =
+                pk.iter().map(|k| source_columns.iter().position(|c| c == k)).collect();
+            let structure_only = ov.is_some_and(|o| o.structure_only);
+            let estimate = if structure_only {
+                None
+            } else {
+                self.row_estimate(&l.source, &plan.source.schema, &t.source).await
+            };
+            tables.insert(
+                t.source.clone(),
+                TableProgress::new(
+                    if structure_only { TableState::Skipped } else { TableState::Pending },
+                    estimate,
+                ),
+            );
+            copies.push(TableCopy {
+                source: t.source.clone(),
+                target: t.target.name.clone(),
+                identity: target_columns.iter().find(|c| c.auto_increment).map(|c| c.name.clone()),
+                source_columns,
+                target_columns,
+                key: key.unwrap_or_default(),
+                filter: ov.and_then(|o| o.filter.clone()).filter(|f| !f.trim().is_empty()),
+                replace: action == Some(ExistingAction::Replace),
+                append: action == Some(ExistingAction::Append),
+            });
         }
-        let notes: Vec<_> = preview.draft.notes.iter().filter(|n| n.level != NoteLevel::Info).collect();
-        if !notes.is_empty() {
-            let _ = writeln!(md, "## {}\n", t(msg!("migrate.report.notes")));
-            for n in notes {
+        let sequences = if plan.mode == MigrationMode::Structure {
+            vec![]
+        } else {
+            preview
+                .draft
+                .sequences
+                .iter()
+                .filter(|s| !s.exists)
+                .map(|s| (s.source.clone(), s.target.clone()))
+                .collect()
+        };
+        let notes = preview
+            .draft
+            .notes
+            .iter()
+            .filter(|n| n.level != NoteLevel::Info)
+            .map(|n| {
                 let at = [Some(n.object.as_str()), n.column.as_deref()]
                     .into_iter()
                     .flatten()
                     .collect::<Vec<_>>()
                     .join(".");
                 let item = n.item.as_deref().map(|i| format!(" [{i}]")).unwrap_or_default();
-                let _ = writeln!(md, "- {at}{item}: {}", n.message.render(lang));
-            }
-            md.push('\n');
+                format!("{at}{item}: {}", n.message.render(lang))
+            })
+            .collect();
+        let now = now_ms();
+        Ok(MigrationRecord {
+            id: uuid::Uuid::new_v4().simple().to_string(),
+            created_at: now,
+            updated_at: now,
+            source_name: format!("{} / {}", l.source.input.name, plan.source.schema),
+            target_name: format!("{} / {}", l.target.input.name, plan.target.schema),
+            plan,
+            phase: MigrationPhase::Structure,
+            statements: preview.statements,
+            done: BTreeSet::new(),
+            statement_errors: BTreeMap::new(),
+            failed_tables: BTreeSet::new(),
+            copies,
+            tables,
+            sequences,
+            skipped_rows: vec![],
+            adjustments: vec![],
+            notes,
+            others: preview.others,
+            finished: false,
+        })
+    }
+
+    /// 元数据中的行数估计（读不出时为 None）。
+    async fn row_estimate(&self, profile: &Profile, schema: &str, table: &str) -> Option<u64> {
+        let args = MetaArgs { schema: Some(schema.into()), table: Some(table.into()), ..Default::default() };
+        let rows = self.metadata.rows(profile, MetaKey::TableStorage, &args).await.ok()??;
+        meta::table_storage(&rows).ok()?.row_count.and_then(|n| u64::try_from(n).ok())
+    }
+
+    /// 继续一个没有完成的迁移：有失败的表时回到迁数据这一步。
+    pub async fn resume_migration(self: &Arc<Self>, id: &str) -> Result<String> {
+        let mut record = self.migrations.load(id)?;
+        if record.finished {
+            return Err(Error::invalid(msg!("migrate.already_finished")));
         }
-        if !preview.others.is_empty() {
-            let _ = writeln!(md, "## {}\n", t(msg!("migrate.report.others")));
-            for o in &preview.others {
-                let _ = writeln!(md, "### {} {}\n", o.kind, o.name);
-                let ddl = match (o.kind.as_str(), &o.table) {
-                    ("trigger", Some(table)) => l
-                        .tables
-                        .iter()
-                        .find(|x| &x.name == table)
-                        .and_then(|x| x.triggers.iter().find(|tr| tr.name == o.name))
-                        .and_then(|tr| tr.definition.clone()),
-                    (kind, _) => match DdlKind::parse(kind) {
-                        Some(k) => self.metadata.ddl(&l.source, &plan.source.schema, k, &o.name).await.ok(),
-                        None => None,
-                    },
-                };
-                match ddl {
-                    Some(d) => {
-                        let _ = writeln!(md, "```sql\n{}\n```\n", d.trim());
-                    }
-                    None => {
-                        let _ = writeln!(md, "{}\n", t(msg!("migrate.report.no_ddl")));
-                    }
-                }
-            }
+        if record.tables.values().any(|t| t.state == TableState::Failed) {
+            record.phase = record.phase.min(MigrationPhase::Data);
         }
-        let _ = writeln!(md, "## {}\n\n```sql\n{}\n```", t(msg!("migrate.report.script")), preview.text);
-        let path = task.report_path("md", format!("migration-{}.md", plan.target.schema))?;
-        std::fs::write(&path, md).map_err(|e| Error::internal(msg!("export.write_failed", error = e)))
+        self.launch(record).await
     }
-}
 
-/// 执行的结果：建表失败的源表、因此跳过的语句数。
-#[derive(Debug, Default)]
-pub(crate) struct Outcome {
-    pub failed_tables: BTreeSet<String>,
-    pub skipped: u64,
-}
+    /// 没有完成的迁移。
+    pub fn unfinished_migrations(&self) -> Result<Vec<MigrationSummary>> {
+        self.migrations.unfinished()
+    }
 
-/// 逐条执行：失败的语句记下后继续；建表（或删除重建时的删表）失败的表，依赖它的语句跳过。
-pub(crate) async fn run_statements(
-    run: &Run<'_>,
-    statements: &[MigrationStatement],
-    password: String,
-) -> Result<Outcome> {
-    let (mut conn, _) = run.service.registry.connect(run.profile, password).await?;
-    run.task.set_cancel(Some(conn.cancel_handle()))?;
-    let mut out = Outcome::default();
-    let result = async {
-        run.sql(&mut conn, &run.dialect.set_schema_sql(run.schema)).await?;
-        for (i, s) in statements.iter().enumerate() {
-            if run.task.cancelled() {
-                return Err(Error::Cancelled);
-            }
-            if s.requires.iter().any(|r| out.failed_tables.contains(r)) {
-                out.skipped += 1;
-                run.task.update(|st| st.progress.statements_done += 1);
-                continue;
+    /// 放弃一个迁移：删除记录，不动目标库。
+    pub fn discard_migration(&self, id: &str) -> Result<()> {
+        self.migrations.delete(id)
+    }
+
+    /// 作为任务执行迁移记录，返回任务 id。结束后刷新目标的元数据缓存、保存记录、写迁移报告。
+    async fn launch(self: &Arc<Self>, record: MigrationRecord) -> Result<String> {
+        let source = self.profile(&record.plan.source.profile)?;
+        let target = self.profile(&record.plan.target.profile)?;
+        let source_dialect = self.metadata.dialect(&source).await?;
+        let target_dialect = self.metadata.dialect(&target).await?;
+        let source_password = self.secrets.get(&source.id)?.unwrap_or_default();
+        let target_password = self.secrets.get(&target.id)?.unwrap_or_default();
+        if !self.migrations.begin(&record.id) {
+            return Err(Error::invalid(msg!("migrate.running")));
+        }
+        let task = match self.tasks.create(TaskKind::Migrate) {
+            Ok(t) => t,
+            Err(e) => {
+                self.migrations.end(&record.id);
+                return Err(e);
             }
-            match run.sql(&mut conn, &s.sql).await {
-                Ok(_) => run.task.update(|st| st.progress.statements_done += 1),
-                Err(e) => {
-                    if run.task.cancelled() {
-                        return Err(Error::Cancelled);
-                    }
-                    if matches!(s.kind, StatementKind::CreateTable | StatementKind::DropTable) {
-                        out.failed_tables.insert(s.object.clone());
-                    }
-                    let failure =
-                        TaskFailure { index: i as u64, line: i as u64 + 1, error: e, text: s.sql.clone() };
-                    run.task.fail(failure, s.sql.clone());
-                    run.task.update(|st| {
-                        st.progress.statements_done += 1;
-                        st.progress.statements_failed += 1;
-                    });
+        };
+        let mut id = String::new();
+        let total =
+            record.statements.iter().enumerate().filter(|(i, _)| !record.done.contains(i)).count() as u64;
+        task.update(|s| {
+            id = s.id.clone();
+            s.phase = TaskPhase::Executing;
+            s.progress.statements_total = total;
+            s.migration = Some(run::progress(&record));
+        });
+        let lang = current_lang();
+        let record_id = record.id.clone();
+        let ctx = Ctx {
+            service: self.clone(),
+            task: task.clone(),
+            record: Arc::new(Mutex::new(record)),
+            source,
+            target,
+            source_dialect,
+            target_dialect,
+            source_password,
+            target_password,
+        };
+        // 错误文字、报告都用开始迁移时的界面语言
+        tokio::spawn(scope_lang(lang, async move {
+            let outcome = run::run(&ctx).await;
+            let _ = ctx.save();
+            let target_schema = ctx.target_schema().unwrap_or_default();
+            ctx.service.metadata.cache().invalidate(&ctx.target.id, Some(&target_schema));
+            let mut result = outcome;
+            if let Ok(r) = ctx.snapshot() {
+                let md = ctx.service.migration_report(&r, &ctx.source, &result, lang).await;
+                let written = task.report_path("md", format!("migration-{target_schema}.md")).and_then(|p| {
+                    std::fs::write(&p, md)
+                        .map_err(|e| Error::internal(msg!("export.write_failed", error = e)))
+                });
+                if result.is_ok() {
+                    result = written;
                 }
             }
-        }
-        Ok(())
+            ctx.service.migrations.end(&record_id);
+            task.finish(result);
+        }));
+        Ok(id)
     }
-    .await;
-    let _ = conn.close().await;
-    result.map(|()| out)
 }
 
 #[cfg(test)]
-mod tests {
+pub(crate) mod tests {
+    use dblens_core::Lang;
     use dblens_dialect::migrate::{MigrationNote, ObjectKind, TableMapping};
 
     use super::*;
-    use crate::compare::tests::service;
-    use crate::model::tests::wait;
-    use crate::task::TaskState;
 
     fn end(p: &str, s: &str) -> MigrationEnd {
         MigrationEnd { profile: p.into(), schema: s.into() }
     }
 
-    fn plan() -> MigrationPlan {
+    pub(crate) fn plan() -> MigrationPlan {
         MigrationPlan {
             source: end("a", "s"),
             tables: None,
             sequences: None,
             target: end("b", "t"),
+            mode: MigrationMode::All,
             settings: MigrationSettings::default(),
+            data: DataOptions::default(),
             existing: BTreeMap::new(),
         }
     }
@@ -499,47 +602,13 @@ mod tests {
         p.existing.insert("b".into(), ExistingAction::Recreate);
         draft.notes.clear();
         assert!(problems(&p, &draft).is_empty());
-    }
-
-    #[tokio::test]
-    async fn failed_tables_skip_their_dependents() {
-        let (s, profile, log, _dir) = service();
-        let profile = s.profile(&profile).unwrap();
-        let task = s.tasks.create(TaskKind::Migrate).unwrap();
-        let id = task.status().id;
-        let st = |kind, object: &str, requires: &[&str], sql: &str| MigrationStatement {
-            phase: 1,
-            kind,
-            object: object.into(),
-            requires: requires.iter().map(|r| r.to_string()).collect(),
-            sql: sql.into(),
-        };
-        let statements = vec![
-            st(StatementKind::CreateTable, "a", &[], "FAIL"),
-            st(StatementKind::CreateTable, "b", &[], "CREATE TABLE b"),
-            st(StatementKind::Comment, "a", &["a"], "COMMENT ON a"),
-            st(StatementKind::Index, "b", &["b"], "FAIL"),
-            st(StatementKind::ForeignKey, "b", &["b", "a"], "ALTER TABLE b ADD FK"),
-            st(StatementKind::Unique, "b", &["b"], "ALTER TABLE b ADD UNIQUE"),
-        ];
-        let run = Run {
-            service: &s,
-            task: &task,
-            profile: &profile,
-            dialect: Dialect::new(Variant::Postgres),
-            schema: "t",
-        };
-        let out = run_statements(&run, &statements, String::new()).await.unwrap();
-        task.finish(Ok(()));
-        assert_eq!(out.failed_tables, BTreeSet::from(["a".to_string()]));
-        assert_eq!(out.skipped, 2);
-        let status = wait(&s, &id).await;
-        assert_eq!(status.state, TaskState::Done);
-        assert_eq!((status.progress.statements_done, status.progress.statements_failed), (6, 2));
-        assert_eq!(status.failures.iter().map(|f| f.index).collect::<Vec<_>>(), [0, 3]);
-        let log = log.lock().unwrap().clone();
-        assert!(
-            log.iter().any(|l| l.contains("ADD UNIQUE")) && !log.iter().any(|l| l.contains("COMMENT ON a"))
-        );
+        // 只迁数据：目标中没有的表不能迁
+        p.mode = MigrationMode::Data;
+        let got = keys(problems(&p, &draft));
+        assert!(got.len() == 1 && got[0].contains(": c"), "{got:?}");
+        // 数据选项超出范围
+        p.mode = MigrationMode::All;
+        p.data.parallel = 9;
+        assert_eq!(problems(&p, &draft).len(), 1);
     }
 }
````

- [ ] **Step 10: 新建 `crates/dblens-service/src/migrate/record.rs`**

```rust
//! 迁移记录（M5b 设计文档 3.3 节）：方案快照、脚本、每张表的搬运参数和进度，存在数据目录的 `migrations/<id>.json`，
//! 每批提交后更新（先写临时文件再改名）。没有完成的记录在迁移标签中列出，可继续或放弃。

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use dblens_core::schema::{Column, Sequence};
use dblens_core::{Error, Result, Value, msg};
use dblens_dialect::migrate::MigrationStatement;
use serde::{Deserialize, Serialize};

use super::{MigrationPlan, OtherObject};

/// 迁移进行到哪一步。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MigrationPhase {
    Structure,
    Data,
    Constraints,
    Sequences,
    Verify,
    Done,
}

/// 一张表的搬运参数（开始时由迁移草稿算出，续传时不再读结构）。
/// `columns`：读的源列与写的目标列一一对应（目标的生成列不写）；`key`：源主键列在 `columns` 中的位置（为空时不能续传）；
/// `identity`：目标的自增列。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TableCopy {
    pub source: String,
    pub target: String,
    pub source_columns: Vec<String>,
    pub target_columns: Vec<Column>,
    pub key: Vec<usize>,
    pub filter: Option<String>,
    pub identity: Option<String>,
    /// 开始前清空目标表（清空后导入、没有主键的表重迁）。
    pub replace: bool,
    /// 追加到已有数据：校验时加上原有的行数。
    pub append: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TableState {
    Pending,
    Copying,
    Done,
    Failed,
    /// 只迁结构，或建表失败、不迁数据。
    Skipped,
}

/// 抽样比对中不一致的一处。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mismatch {
    pub key: String,
    pub column: String,
    pub source: String,
    pub target: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Verify {
    pub source_rows: u64,
    pub target_rows: u64,
    /// 应有的目标行数（追加时加上原有的行数，减去跳过的行）。
    pub expected_rows: u64,
    pub sampled: u64,
    pub mismatches: Vec<Mismatch>,
}

impl Verify {
    pub fn ok(&self) -> bool {
        self.target_rows == self.expected_rows && self.mismatches.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TableProgress {
    pub state: TableState,
    pub rows: u64,
    pub skipped: u64,
    /// 最后提交的一行的源主键值。
    pub last_key: Option<Vec<Value>>,
    /// 追加前目标表原有的行数。
    pub base_rows: Option<u64>,
    pub estimate: Option<u64>,
    pub error: Option<String>,
    pub elapsed_ms: u64,
    pub verify: Option<Verify>,
}

impl TableProgress {
    pub fn new(state: TableState, estimate: Option<u64>) -> Self {
        Self {
            state,
            rows: 0,
            skipped: 0,
            last_key: None,
            base_rows: None,
            estimate,
            error: None,
            elapsed_ms: 0,
            verify: None,
        }
    }
}

/// 跳过的一行。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkippedRow {
    pub table: String,
    pub key: String,
    pub error: String,
}

/// 序列和自增的调整结果。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Adjustment {
    pub object: String,
    pub sql: String,
    pub error: Option<String>,
}

/// 跳过的行至多记这么多条，超出只计数。
pub const MAX_SKIPPED_ROWS: usize = 1000;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MigrationRecord {
    pub id: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub plan: MigrationPlan,
    /// 两端的显示名（`连接 / schema`）。
    pub source_name: String,
    pub target_name: String,
    pub phase: MigrationPhase,
    pub statements: Vec<MigrationStatement>,
    /// 执行成功的语句（序号）；失败的语句继续时再执行。
    pub done: BTreeSet<usize>,
    /// 失败的语句：序号 → 错误。
    pub statement_errors: BTreeMap<usize, String>,
    /// 建表失败的源表：不迁数据，依赖它的语句跳过。
    pub failed_tables: BTreeSet<String>,
    pub copies: Vec<TableCopy>,
    pub tables: BTreeMap<String, TableProgress>,
    /// 迁移的序列：（源序列名，目标序列）。
    pub sequences: Vec<(String, Sequence)>,
    pub skipped_rows: Vec<SkippedRow>,
    pub adjustments: Vec<Adjustment>,
    /// 映射提示（开始时按界面语言写成文字）。
    pub notes: Vec<String>,
    pub others: Vec<OtherObject>,
    pub finished: bool,
}

pub fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64)
}

/// 未完成的迁移（列表用）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MigrationSummary {
    pub id: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub source: String,
    pub target: String,
    pub phase: MigrationPhase,
    pub tables_done: usize,
    pub tables_total: usize,
    pub running: bool,
}

/// 迁移记录的存放处。`running`：正在执行的记录（不能同时继续两次）。
pub struct MigrationRecords {
    dir: Mutex<PathBuf>,
    running: Mutex<HashSet<String>>,
}

impl Default for MigrationRecords {
    fn default() -> Self {
        Self { dir: Mutex::new(std::env::temp_dir().join("dblens-migrations")), running: Mutex::default() }
    }
}

fn io(e: impl std::fmt::Display) -> Error {
    Error::internal(msg!("migrate.record_io", error = e.to_string()))
}

fn lock_error() -> Error {
    Error::internal(msg!("internal.lock_poisoned", lock = "migrations"))
}

impl MigrationRecords {
    pub fn set_dir(&self, dir: PathBuf) {
        if let Ok(mut d) = self.dir.lock() {
            *d = dir;
        }
    }

    fn path(&self, id: &str) -> Result<PathBuf> {
        // 记录编号只有十六进制字符，不会跑出目录
        if id.is_empty() || !id.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(Error::not_found(msg!("migrate.not_found", id = id)));
        }
        Ok(self.dir.lock().map_err(|_| lock_error())?.join(format!("{id}.json")))
    }

    pub fn save(&self, r: &MigrationRecord) -> Result<()> {
        let path = self.path(&r.id)?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(io)?;
        }
        let tmp = path.with_extension("json.tmp");
        let text = serde_json::to_string(r).map_err(io)?;
        std::fs::write(&tmp, text).map_err(io)?;
        std::fs::rename(&tmp, &path).map_err(io)
    }

    pub fn load(&self, id: &str) -> Result<MigrationRecord> {
        let path = self.path(id)?;
        let text = std::fs::read_to_string(&path)
            .map_err(|_| Error::not_found(msg!("migrate.not_found", id = id)))?;
        serde_json::from_str(&text).map_err(io)
    }

    pub fn delete(&self, id: &str) -> Result<()> {
        if self.is_running(id) {
            return Err(Error::invalid(msg!("migrate.running")));
        }
        let path = self.path(id)?;
        std::fs::remove_file(&path).map_err(|_| Error::not_found(msg!("migrate.not_found", id = id)))
    }

    /// 没有完成的迁移，最近更新的在前。读不出的文件跳过。
    pub fn unfinished(&self) -> Result<Vec<MigrationSummary>> {
        let dir = self.dir.lock().map_err(|_| lock_error())?.clone();
        let Ok(entries) = std::fs::read_dir(&dir) else { return Ok(vec![]) };
        let mut out = Vec::new();
        for e in entries.flatten() {
            let p = e.path();
            if p.extension().is_none_or(|x| x != "json") {
                continue;
            }
            let Some(r) = std::fs::read_to_string(&p)
                .ok()
                .and_then(|t| serde_json::from_str::<MigrationRecord>(&t).ok())
            else {
                continue;
            };
            if r.finished {
                continue;
            }
            out.push(MigrationSummary {
                running: self.is_running(&r.id),
                id: r.id,
                created_at: r.created_at,
                updated_at: r.updated_at,
                source: r.source_name,
                target: r.target_name,
                phase: r.phase,
                tables_done: r
                    .tables
                    .values()
                    .filter(|t| matches!(t.state, TableState::Done | TableState::Skipped))
                    .count(),
                tables_total: r.tables.len(),
            });
        }
        out.sort_by_key(|s| std::cmp::Reverse(s.updated_at));
        Ok(out)
    }

    pub fn is_running(&self, id: &str) -> bool {
        self.running.lock().map(|r| r.contains(id)).unwrap_or(false)
    }

    /// 标记开始执行；已经在执行时返回 false。
    pub fn begin(&self, id: &str) -> bool {
        self.running.lock().map(|mut r| r.insert(id.to_string())).unwrap_or(false)
    }

    pub fn end(&self, id: &str) {
        if let Ok(mut r) = self.running.lock() {
            r.remove(id);
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::migrate::tests::plan;

    pub(crate) fn record(id: &str) -> MigrationRecord {
        MigrationRecord {
            id: id.into(),
            created_at: 1,
            updated_at: 1,
            plan: plan(),
            source_name: "a / s".into(),
            target_name: "b / t".into(),
            phase: MigrationPhase::Data,
            statements: vec![],
            done: BTreeSet::new(),
            statement_errors: BTreeMap::new(),
            failed_tables: BTreeSet::new(),
            copies: vec![],
            tables: BTreeMap::from([
                ("x".to_string(), TableProgress::new(TableState::Done, Some(3))),
                ("y".to_string(), TableProgress::new(TableState::Copying, None)),
            ]),
            sequences: vec![],
            skipped_rows: vec![],
            adjustments: vec![],
            notes: vec![],
            others: vec![],
            finished: false,
        }
    }

    #[test]
    fn records_round_trip_and_list_unfinished() {
        let dir = tempfile::tempdir().unwrap();
        let store = MigrationRecords::default();
        store.set_dir(dir.path().join("migrations"));
        let mut r = record("a1");
        r.tables.get_mut("y").unwrap().last_key = Some(vec![Value::Int(7), Value::String("k".into())]);
        store.save(&r).unwrap();
        assert_eq!(store.load("a1").unwrap(), r);
        let mut done = record("b2");
        done.finished = true;
        store.save(&done).unwrap();
        let list = store.unfinished().unwrap();
        assert_eq!(
            list.iter().map(|s| (s.id.as_str(), s.tables_done, s.tables_total)).collect::<Vec<_>>(),
            [("a1", 1, 2)]
        );
        assert!(store.begin("a1") && !store.begin("a1"));
        assert!(store.delete("a1").is_err());
        store.end("a1");
        store.delete("a1").unwrap();
        assert!(store.load("a1").is_err());
        assert!(store.load("../x").is_err());
    }
}
```

- [ ] **Step 11: 新建 `crates/dblens-service/src/migrate/report.rs`**

````rust
//! 迁移报告（M5a 设计文档第 5.2 节、M5b 设计文档第 5 节）：由迁移记录写成 markdown——结果、语句汇总、每张表的数据和校验、
//! 跳过的行、序列和自增的调整、失败的语句、映射提示、不迁移的对象及其源库 DDL、脚本。

use std::fmt::Write as _;

use dblens_core::{Error, Lang, Msg, Result, msg};
use dblens_dialect::DdlKind;
use dblens_dialect::migrate::script_text;

use super::record::{MigrationRecord, TableState};
use crate::profile::Profile;
use crate::service::Service;

/// 表格单元格中的竖线和换行。
fn cell(s: &str) -> String {
    s.replace('|', "\\|").replace(['\r', '\n'], " ")
}

impl Service {
    /// 写迁移报告。`outcome` 是这次执行的结果；`source` 用来取不迁移的对象的 DDL。
    pub(crate) async fn migration_report(
        &self,
        r: &MigrationRecord,
        source: &Profile,
        outcome: &Result<()>,
        lang: Lang,
    ) -> String {
        let t = |m: Msg| m.render(lang);
        let mut md = String::new();
        let _ = writeln!(
            md,
            "# {}\n",
            t(msg!("migrate.report.title", source = &r.source_name, target = &r.target_name))
        );
        let state = match outcome {
            Ok(()) if r.finished => t(msg!("migrate.report.finished")),
            Ok(()) => t(msg!("migrate.report.unfinished")),
            Err(Error::Cancelled) => t(msg!("migrate.report.cancelled")),
            Err(e) => t(msg!("migrate.report.failed", error = e.to_string())),
        };
        let _ = writeln!(md, "{state}\n");
        let _ = writeln!(
            md,
            "{}\n",
            t(msg!(
                "migrate.report.statements",
                done = r.done.len(),
                failed = r.statement_errors.len(),
                total = r.statements.len()
            ))
        );
        if !r.copies.is_empty() {
            let _ = writeln!(md, "## {}\n", t(msg!("migrate.report.data")));
            let _ =
                writeln!(md, "| {} |\n|---|---|---|---|---|---|---|", t(msg!("migrate.report.data_header")));
            for c in &r.copies {
                let Some(p) = r.tables.get(&c.source) else { continue };
                let verify = match (&p.verify, p.state) {
                    (Some(v), _) if v.ok() => t(msg!("migrate.report.verify_ok", sampled = v.sampled)),
                    (Some(v), _) => t(msg!(
                        "migrate.report.verify_bad",
                        expected = v.expected_rows,
                        target = v.target_rows,
                        mismatches = v.mismatches.len()
                    )),
                    (None, TableState::Failed | TableState::Skipped) => p.error.clone().unwrap_or_default(),
                    (None, _) => String::new(),
                };
                let source_rows = p.verify.as_ref().map(|v| v.source_rows.to_string()).unwrap_or_default();
                let _ = writeln!(
                    md,
                    "| {} → {} | {} | {} | {} | {} | {:.1} s | {} |",
                    cell(&c.source),
                    cell(&c.target),
                    t(msg!("migrate.report.state", state = format!("{:?}", p.state).to_lowercase())),
                    source_rows,
                    p.rows,
                    p.skipped,
                    p.elapsed_ms as f64 / 1000.0,
                    cell(&verify)
                );
            }
            md.push('\n');
            for c in &r.copies {
                let Some(v) = r.tables.get(&c.source).and_then(|p| p.verify.as_ref()) else { continue };
                if v.mismatches.is_empty() {
                    continue;
                }
                let _ = writeln!(md, "### {}\n", t(msg!("migrate.report.mismatches", table = &c.source)));
                for m in &v.mismatches {
                    if m.column.is_empty() {
                        let _ = writeln!(md, "- {}: {}", m.key, t(msg!("migrate.report.row_missing")));
                    } else {
                        let _ = writeln!(md, "- {} · {}: `{}` → `{}`", m.key, m.column, m.source, m.target);
                    }
                }
                md.push('\n');
            }
        }
        if !r.skipped_rows.is_empty() {
            let _ = writeln!(md, "## {}\n", t(msg!("migrate.report.skipped_rows")));
            for s in &r.skipped_rows {
                let _ = writeln!(md, "- {} ({}): {}", s.table, s.key, s.error);
            }
            md.push('\n');
        }
        if !r.adjustments.is_empty() {
            let _ = writeln!(md, "## {}\n", t(msg!("migrate.report.adjustments")));
            for a in &r.adjustments {
                match &a.error {
                    None => {
                        let _ = writeln!(md, "- {}: `{}`", a.object, a.sql);
                    }
                    Some(e) => {
                        let _ = writeln!(md, "- {}: `{}` — {}", a.object, a.sql, e);
                    }
                }
            }
            md.push('\n');
        }
        if !r.statement_errors.is_empty() {
            let _ = writeln!(md, "## {}\n", t(msg!("migrate.report.failures")));
            for (i, e) in &r.statement_errors {
                if let Some(s) = r.statements.get(*i) {
                    let _ = writeln!(md, "```sql\n{}\n```\n\n{e}\n", s.sql);
                }
            }
        }
        if !r.notes.is_empty() {
            let _ = writeln!(md, "## {}\n", t(msg!("migrate.report.notes")));
            for n in &r.notes {
                let _ = writeln!(md, "- {n}");
            }
            md.push('\n');
        }
        if !r.others.is_empty() {
            let _ = writeln!(md, "## {}\n", t(msg!("migrate.report.others")));
            for o in &r.others {
                let _ = writeln!(md, "### {} {}\n", o.kind, o.name);
                let ddl = match (o.kind.as_str(), &o.table) {
                    ("trigger", Some(_)) => o.definition.clone(),
                    (kind, _) => match DdlKind::parse(kind) {
                        Some(k) => self.metadata.ddl(source, &r.plan.source.schema, k, &o.name).await.ok(),
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
        let _ = writeln!(
            md,
            "## {}\n\n```sql\n{}\n```",
            t(msg!("migrate.report.script")),
            script_text(&r.statements)
        );
        md
    }
}
````

- [ ] **Step 12: 新建 `crates/dblens-service/src/migrate/run.rs`**

```rust
//! 迁移的执行（M5b 设计文档 3.2、3.3、第 5 节）：按迁移记录依次建表、迁数据（按表并行）、建索引和约束、
//! 调整序列和自增、校验；每一步之后保存记录，继续时从记录的步骤接着做。

use std::sync::{Arc, Mutex};
use std::time::Instant;

use dblens_core::{Error, Result, Value, msg};
use dblens_dialect::migrate::StatementKind;
use dblens_dialect::migrate::data::{TableRead, identity_reset_sql, sequence_next_sql, sequence_restart_sql};
use dblens_dialect::{Compat, Dialect, Family};
use dblens_driver::{Connection, ExecEvent};
use serde::Serialize;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;

use super::copy::copy_table;
use super::record::{
    Adjustment, MigrationPhase, MigrationRecord, Mismatch, TableCopy, TableState, Verify, now_ms,
};
use super::values::{key_text, normalize, to_target};
use crate::design::Run;
use crate::import::exec;
use crate::profile::Profile;
use crate::service::Service;
use crate::task::{TaskFailure, TaskHandle};

/// 任务状态中的迁移进度（`TaskStatus::migration`）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MigrationProgress {
    pub record: String,
    pub phase: MigrationPhase,
    pub tables: Vec<TableView>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TableView {
    pub source: String,
    pub target: String,
    pub state: TableState,
    pub rows: u64,
    pub skipped: u64,
    pub estimate: Option<u64>,
    pub elapsed_ms: u64,
    pub error: Option<String>,
    pub verify: Option<Verify>,
}

/// 执行中共用的上下文（每张表的搬运在各自的任务中进行，所以都是可以复制的）。
#[derive(Clone)]
pub(crate) struct Ctx {
    pub service: Arc<Service>,
    pub task: TaskHandle,
    pub record: Arc<Mutex<MigrationRecord>>,
    pub source: Profile,
    pub target: Profile,
    pub source_dialect: Dialect,
    pub target_dialect: Dialect,
    pub source_password: String,
    pub target_password: String,
}

fn lock_error() -> Error {
    Error::internal(msg!("internal.lock_poisoned", lock = "migration"))
}

impl Ctx {
    pub fn with<R>(&self, f: impl FnOnce(&mut MigrationRecord) -> R) -> Result<R> {
        Ok(f(&mut *self.record.lock().map_err(|_| lock_error())?))
    }

    pub fn snapshot(&self) -> Result<MigrationRecord> {
        self.with(|r| r.clone())
    }

    /// 保存记录，并更新任务状态中的进度。
    pub fn save(&self) -> Result<()> {
        let r = self.with(|r| {
            r.updated_at = now_ms();
            r.clone()
        })?;
        self.service.migrations.save(&r)?;
        self.task.update(|s| s.migration = Some(progress(&r)));
        Ok(())
    }

    pub fn source_schema(&self) -> Result<String> {
        self.with(|r| r.plan.source.schema.clone())
    }

    pub fn target_schema(&self) -> Result<String> {
        self.with(|r| r.plan.target.schema.clone())
    }

    pub async fn connect_source(&self) -> Result<Box<dyn Connection>> {
        let (mut c, _) = self.service.registry.connect(&self.source, self.source_password.clone()).await?;
        exec(&mut c, &self.source_dialect.set_schema_sql(&self.source_schema()?), &[]).await?;
        Ok(c)
    }

    pub async fn connect_target(&self) -> Result<Box<dyn Connection>> {
        let (mut c, _) = self.service.registry.connect(&self.target, self.target_password.clone()).await?;
        exec(&mut c, &self.target_dialect.set_schema_sql(&self.target_schema()?), &[]).await?;
        Ok(c)
    }

    fn run(&self) -> Run<'_> {
        Run {
            service: &self.service,
            task: &self.task,
            profile: &self.target,
            dialect: self.target_dialect,
            schema: "",
        }
    }
}

pub fn progress(r: &MigrationRecord) -> MigrationProgress {
    MigrationProgress {
        record: r.id.clone(),
        phase: r.phase,
        tables: r
            .copies
            .iter()
            .filter_map(|c| {
                let p = r.tables.get(&c.source)?;
                Some(TableView {
                    source: c.source.clone(),
                    target: c.target.clone(),
                    state: p.state,
                    rows: p.rows,
                    skipped: p.skipped,
                    estimate: p.estimate,
                    elapsed_ms: p.elapsed_ms,
                    error: p.error.clone(),
                    verify: p.verify.clone(),
                })
            })
            .collect(),
    }
}

/// 查询结果的第一行第一列。
pub(crate) async fn scalar(
    conn: &mut Box<dyn Connection>,
    sql: &str,
    params: &[Value],
) -> Result<Option<Value>> {
    let mut stream = conn.execute(sql, params).await?;
    let mut out = None;
    while let Some(ev) = stream.next(10).await? {
        if let (ExecEvent::Rows(rows), None) = (ev, &out) {
            out = rows.into_iter().next().and_then(|r| r.into_iter().next());
        }
    }
    Ok(out)
}

pub(crate) fn as_u64(v: Option<Value>) -> u64 {
    match v {
        Some(Value::Int(n)) => n.max(0) as u64,
        Some(Value::UInt(n)) => n,
        Some(Value::Decimal(s) | Value::String(s)) => {
            s.trim().split('.').next().and_then(|x| x.parse().ok()).unwrap_or(0)
        }
        Some(Value::Float(f)) => f.max(0.0) as u64,
        _ => 0,
    }
}

/// 依次执行各步。已经做完的步骤跳过；出错（含取消）时记录已保存，可以继续。
pub(crate) async fn run(ctx: &Ctx) -> Result<()> {
    let phase = ctx.with(|r| r.phase)?;
    if phase <= MigrationPhase::Structure {
        statements(ctx, 1).await?;
        set_phase(ctx, MigrationPhase::Data)?;
    }
    if phase <= MigrationPhase::Data {
        data(ctx).await?;
        set_phase(ctx, MigrationPhase::Constraints)?;
    }
    if phase <= MigrationPhase::Constraints {
        statements(ctx, 2).await?;
        set_phase(ctx, MigrationPhase::Sequences)?;
    }
    if phase <= MigrationPhase::Sequences {
        adjust(ctx).await?;
        set_phase(ctx, MigrationPhase::Verify)?;
    }
    if phase <= MigrationPhase::Verify {
        verify(ctx).await?;
        set_phase(ctx, MigrationPhase::Done)?;
    }
    // 有搬运失败的表时没有完成，修改后可以继续
    ctx.with(|r| r.finished = !r.tables.values().any(|t| t.state == TableState::Failed))?;
    ctx.save()
}

fn set_phase(ctx: &Ctx, phase: MigrationPhase) -> Result<()> {
    ctx.with(|r| r.phase = phase)?;
    ctx.save()
}

/// 执行某一步的语句：已成功的跳过；建表失败的表，依赖它的语句跳过；失败的语句记下后继续。
async fn statements(ctx: &Ctx, phase: u8) -> Result<()> {
    let todo: Vec<usize> = ctx.with(|r| {
        r.statements
            .iter()
            .enumerate()
            .filter(|(i, s)| s.phase == phase && !r.done.contains(i))
            .map(|(i, _)| i)
            .collect()
    })?;
    if todo.is_empty() {
        return Ok(());
    }
    let mut conn = ctx.connect_target().await?;
    ctx.task.set_cancel(Some(conn.cancel_handle()))?;
    let run = ctx.run();
    let result = async {
        for i in todo {
            if ctx.task.cancelled() {
                return Err(Error::Cancelled);
            }
            let (s, blocked) = ctx.with(|r| {
                let s = r.statements[i].clone();
                let blocked = s.requires.iter().any(|t| r.failed_tables.contains(t));
                (s, blocked)
            })?;
            if blocked {
                ctx.task.update(|st| st.progress.statements_done += 1);
                continue;
            }
            match run.sql(&mut conn, &s.sql).await {
                Ok(_) => {
                    ctx.with(|r| {
                        r.done.insert(i);
                        r.statement_errors.remove(&i);
                    })?;
                    ctx.task.update(|st| st.progress.statements_done += 1);
                }
                Err(e) => {
                    if ctx.task.cancelled() {
                        return Err(Error::Cancelled);
                    }
                    ctx.with(|r| {
                        if matches!(s.kind, StatementKind::CreateTable | StatementKind::DropTable) {
                            r.failed_tables.insert(s.object.clone());
                            if let Some(t) = r.tables.get_mut(&s.object) {
                                t.state = TableState::Skipped;
                                t.error = Some(e.to_string());
                            }
                        }
                        r.statement_errors.insert(i, e.to_string());
                    })?;
                    let failure =
                        TaskFailure { index: i as u64, line: i as u64 + 1, error: e, text: s.sql.clone() };
                    ctx.task.fail(failure, s.sql.clone());
                    ctx.task.update(|st| {
                        st.progress.statements_done += 1;
                        st.progress.statements_failed += 1;
                    });
                }
            }
            ctx.save()?;
        }
        Ok(())
    }
    .await;
    let _ = ctx.task.set_cancel(None);
    let _ = conn.close().await;
    result
}

/// 迁数据：没有完成的表按并行数同时搬运。某张表失败只标记这张表；取消时等正在搬运的表停下后返回。
async fn data(ctx: &Ctx) -> Result<()> {
    let (copies, parallel) = ctx.with(|r| {
        let copies: Vec<TableCopy> = r
            .copies
            .iter()
            .filter(|c| {
                r.tables.get(&c.source).is_some_and(|t| {
                    matches!(t.state, TableState::Pending | TableState::Copying | TableState::Failed)
                })
            })
            .cloned()
            .collect();
        (copies, r.plan.data.parallel.clamp(1, 4))
    })?;
    let sem = Arc::new(Semaphore::new(parallel));
    let mut set = JoinSet::new();
    for c in copies {
        let permit = sem.clone().acquire_owned().await.map_err(|_| lock_error())?;
        if ctx.task.cancelled() {
            break;
        }
        let ctx = ctx.clone();
        set.spawn(async move {
            let _permit = permit;
            let started = Instant::now();
            let result = copy_table(&ctx, &c).await;
            let _ = ctx.with(|r| {
                if let Some(t) = r.tables.get_mut(&c.source) {
                    t.elapsed_ms += started.elapsed().as_millis() as u64;
                    match &result {
                        Ok(()) => {
                            t.state = TableState::Done;
                            t.error = None;
                        }
                        // 取消时保持"迁移中"，继续时从断点接着做
                        Err(Error::Cancelled) => {}
                        Err(e) => {
                            t.state = TableState::Failed;
                            t.error = Some(e.to_string());
                        }
                    }
                }
            });
            let _ = ctx.save();
            result
        });
    }
    let mut cancelled = ctx.task.cancelled();
    while let Some(r) = set.join_next().await {
        if matches!(r, Ok(Err(Error::Cancelled))) {
            cancelled = true;
        }
    }
    if cancelled { Err(Error::Cancelled) } else { Ok(()) }
}

/// 调整自增列和序列：自增推到当前最大值之后，序列推到源序列的位置。出错只记下。
async fn adjust(ctx: &Ctx) -> Result<()> {
    let (copies, sequences, source_schema, target_schema) = ctx.with(|r| {
        let copies: Vec<TableCopy> = r
            .copies
            .iter()
            .filter(|c| {
                c.identity.is_some() && r.tables.get(&c.source).is_some_and(|t| t.state == TableState::Done)
            })
            .cloned()
            .collect();
        (copies, r.sequences.clone(), r.plan.source.schema.clone(), r.plan.target.schema.clone())
    })?;
    if copies.is_empty() && sequences.is_empty() {
        return Ok(());
    }
    let (sd, td) = (ctx.source_dialect, ctx.target_dialect);
    let mut tgt = ctx.connect_target().await?;
    let mut src = if sequences.is_empty() { None } else { Some(ctx.connect_source().await?) };
    let mut out = Vec::new();
    for c in &copies {
        let column = c.identity.as_deref().unwrap_or_default();
        if let Some(sql) = identity_reset_sql(&td, &target_schema, &c.target, column)? {
            let error = exec(&mut tgt, &sql, &[]).await.err().map(|e| e.to_string());
            out.push(Adjustment { object: format!("{}.{column}", c.target), sql, error });
        }
    }
    for (name, seq) in &sequences {
        let (Some(conn), Some(q)) = (src.as_mut(), sequence_next_sql(&sd, &source_schema, name)?) else {
            continue;
        };
        let next = match scalar(conn, &q, &[]).await {
            Ok(v) => as_u64(v),
            Err(e) => {
                out.push(Adjustment { object: seq.name.clone(), sql: q, error: Some(e.to_string()) });
                continue;
            }
        };
        for sql in sequence_restart_sql(&td, &target_schema, seq, &next.max(1).to_string())? {
            let error = exec(&mut tgt, &sql, &[]).await.err().map(|e| e.to_string());
            let failed = error.is_some();
            out.push(Adjustment { object: seq.name.clone(), sql, error });
            if failed {
                break;
            }
        }
    }
    let _ = tgt.close().await;
    if let Some(c) = src {
        let _ = c.close().await;
    }
    ctx.with(|r| r.adjustments = out)?;
    ctx.save()
}

/// 校验：每张迁完的表比对行数；设了抽样时按主键取最前和最后各一半逐值比对。
async fn verify(ctx: &Ctx) -> Result<()> {
    let (copies, sample) = ctx.with(|r| {
        let copies: Vec<TableCopy> = r
            .copies
            .iter()
            .filter(|c| r.tables.get(&c.source).is_some_and(|t| t.state == TableState::Done))
            .cloned()
            .collect();
        (copies, r.plan.data.sample)
    })?;
    if copies.is_empty() {
        return Ok(());
    }
    let mut src = ctx.connect_source().await?;
    let mut tgt = ctx.connect_target().await?;
    let result = async {
        for c in &copies {
            if ctx.task.cancelled() {
                return Err(Error::Cancelled);
            }
            let v = verify_table(ctx, c, &mut src, &mut tgt, sample).await?;
            ctx.with(|r| {
                if let Some(t) = r.tables.get_mut(&c.source) {
                    t.verify = Some(v);
                }
            })?;
            ctx.save()?;
        }
        Ok(())
    }
    .await;
    let _ = src.close().await;
    let _ = tgt.close().await;
    result
}

/// 抽样比对中不一致的处至多记这么多。
const MAX_MISMATCHES: usize = 20;

async fn verify_table(
    ctx: &Ctx,
    c: &TableCopy,
    src: &mut Box<dyn Connection>,
    tgt: &mut Box<dyn Connection>,
    sample: u64,
) -> Result<Verify> {
    let (sd, td) = (ctx.source_dialect, ctx.target_dialect);
    let (ss, ts) = (ctx.source_schema()?, ctx.target_schema()?);
    let key_names: Vec<String> = c.key.iter().map(|&i| c.source_columns[i].clone()).collect();
    let target_names: Vec<String> = c.target_columns.iter().map(|x| x.name.clone()).collect();
    let target_key: Vec<String> = c.key.iter().map(|&i| target_names[i].clone()).collect();
    let read = TableRead {
        schema: &ss,
        table: &c.source,
        columns: &c.source_columns,
        key: &key_names,
        filter: c.filter.as_deref(),
    };
    let target_read =
        TableRead { schema: &ts, table: &c.target, columns: &target_names, key: &target_key, filter: None };
    let source_rows = as_u64(scalar(src, &read.count(&sd), &[]).await?);
    let target_rows = as_u64(scalar(tgt, &target_read.count(&td), &[]).await?);
    let (base, skipped) = ctx
        .with(|r| r.tables.get(&c.source).map(|t| (t.base_rows.unwrap_or(0), t.skipped)).unwrap_or((0, 0)))?;
    let mut v = Verify {
        source_rows,
        target_rows,
        expected_rows: (base + source_rows).saturating_sub(skipped),
        sampled: 0,
        mismatches: vec![],
    };
    if sample == 0 || c.key.is_empty() {
        return Ok(v);
    }
    let mut rows = Vec::new();
    for (desc, n) in [(false, sample / 2), (true, sample - sample / 2)] {
        let sql = read.sample(&sd, desc, n);
        let mut stream = src.execute(&sql, &[]).await?;
        while let Some(ev) = stream.next(1000).await? {
            if let ExecEvent::Rows(r) = ev {
                rows.extend(r);
            }
        }
    }
    let mut seen = std::collections::HashSet::new();
    for row in rows {
        let key: Vec<Value> = c.key.iter().map(|&i| row[i].clone()).collect();
        if !seen.insert(key_text(&key)) {
            continue;
        }
        v.sampled += 1;
        let params: Vec<Value> = c
            .key
            .iter()
            .map(|&i| to_target(&row[i], &c.target_columns[i]))
            .collect::<Result<_>>()
            .unwrap_or_else(|_| key.clone());
        let sql = target_read.row(&td, &params);
        let mut stream = tgt.execute(&sql, &params).await?;
        let mut found = None;
        while let Some(ev) = stream.next(10).await? {
            if let (ExecEvent::Rows(r), None) = (ev, &found) {
                found = r.into_iter().next();
            }
        }
        drop(stream);
        let Some(target_row) = found else {
            if v.mismatches.len() < MAX_MISMATCHES {
                v.mismatches.push(Mismatch {
                    key: key_text(&key),
                    column: String::new(),
                    source: String::new(),
                    target: String::new(),
                });
            }
            continue;
        };
        let empty_is_null = td.family() == Family::Oracle || td.compat() == Compat::Oracle;
        for (i, col) in c.target_columns.iter().enumerate() {
            let (a, b) =
                (normalize(&row[i], col, empty_is_null), normalize(&target_row[i], col, empty_is_null));
            if a != b && v.mismatches.len() < MAX_MISMATCHES {
                v.mismatches.push(Mismatch {
                    key: key_text(&key),
                    column: col.name.clone(),
                    source: a,
                    target: b,
                });
            }
        }
    }
    Ok(v)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use dblens_dialect::Variant;
    use dblens_dialect::migrate::MigrationStatement;

    use super::*;
    use crate::compare::tests::service;
    use crate::migrate::record::tests::record;
    use crate::task::{TaskKind, TaskState};

    #[tokio::test]
    async fn statements_skip_dependents_and_resume_retries_failures() {
        let (s, profile, log, dir) = service();
        s.migrations.set_dir(dir.path().join("migrations"));
        let profile = s.profile(&profile).unwrap();
        let st = |phase, kind, object: &str, requires: &[&str], sql: &str| MigrationStatement {
            phase,
            kind,
            object: object.into(),
            requires: requires.iter().map(|r| r.to_string()).collect(),
            sql: sql.into(),
        };
        let mut r = record("c3");
        r.phase = MigrationPhase::Structure;
        r.tables = BTreeMap::new();
        r.statements = vec![
            st(1, StatementKind::CreateTable, "a", &[], "FAIL"),
            st(1, StatementKind::CreateTable, "b", &[], "CREATE TABLE b"),
            st(1, StatementKind::Comment, "a", &["a"], "COMMENT ON a"),
            st(2, StatementKind::Index, "b", &["b"], "FAIL"),
            st(2, StatementKind::ForeignKey, "b", &["b", "a"], "ALTER TABLE b ADD FK"),
            st(2, StatementKind::Unique, "b", &["b"], "ALTER TABLE b ADD UNIQUE"),
        ];
        let task = s.tasks.create(TaskKind::Migrate).unwrap();
        let ctx = Ctx {
            service: s.clone(),
            task: task.clone(),
            record: Arc::new(Mutex::new(r)),
            source: profile.clone(),
            target: profile,
            source_dialect: Dialect::new(Variant::Postgres),
            target_dialect: Dialect::new(Variant::Postgres),
            source_password: String::new(),
            target_password: String::new(),
        };
        run(&ctx).await.unwrap();
        task.finish(Ok(()));
        let r = ctx.snapshot().unwrap();
        assert_eq!(r.phase, MigrationPhase::Done);
        assert!(r.finished);
        assert_eq!(r.done, [1, 5].into_iter().collect());
        assert_eq!(r.failed_tables, ["a".to_string()].into_iter().collect());
        assert_eq!(r.statement_errors.keys().copied().collect::<Vec<_>>(), [0, 3]);
        // 记录已保存
        assert_eq!(s.migrations.load("c3").unwrap().done, r.done);
        let st = s.tasks.status(&task_id(&ctx)).unwrap();
        assert_eq!(st.state, TaskState::Done);
        assert_eq!(st.migration.as_ref().map(|m| m.phase), Some(MigrationPhase::Done));
        let log = log.lock().unwrap().clone();
        assert!(
            log.iter().any(|l| l.contains("ADD UNIQUE")) && !log.iter().any(|l| l.contains("COMMENT ON a"))
        );
    }

    fn task_id(ctx: &Ctx) -> String {
        let mut id = String::new();
        ctx.task.update(|s| id = s.id.clone());
        id
    }
}
```

- [ ] **Step 13: 新建 `crates/dblens-service/src/migrate/values.rs`**

```rust
//! 迁移中的值（M5b 设计文档第 4、5 节）：把源库读出的值转换成目标列的类型（转换不了的交给 M2b 导入的文本转换），
//! 以及校验时的规整（两边都转换成目标列的类型后写成可比较的文本）。

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use chrono::{Local, NaiveDateTime, TimeZone, Timelike};
use dblens_core::schema::Column;
use dblens_core::{Error, LogicalType, Result, Value, msg};

use crate::import::{EmptyAs, convert};

/// 值的文本形式（转换成文本列、交给文本转换时用）。
fn text_of(v: &Value) -> String {
    match v {
        Value::Null => String::new(),
        Value::Bool(b) => (if *b { "1" } else { "0" }).into(),
        Value::Int(n) => n.to_string(),
        Value::UInt(n) => n.to_string(),
        Value::Float(f) => f.to_string(),
        Value::Decimal(s) | Value::String(s) | Value::Interval(s) => s.clone(),
        Value::Bytes(b) => format!("0x{}", b.iter().map(|x| format!("{x:02x}")).collect::<String>()),
        Value::Date(d) => d.to_string(),
        Value::Time(t) => t.to_string(),
        Value::DateTime(t) => t.to_string(),
        Value::DateTimeTz(t) => t.to_rfc3339(),
        Value::Json(j) => j.to_string(),
        Value::Uuid(u) => u.to_string(),
        Value::Lob(l) => l.preview.clone(),
        Value::Other { text, .. } => text.clone(),
    }
}

/// 布尔值的数值写法。
fn truthy(v: &Value) -> Option<bool> {
    match v {
        Value::Bool(b) => Some(*b),
        Value::Int(n) => Some(*n != 0),
        Value::UInt(n) => Some(*n != 0),
        Value::Decimal(s) => s.parse::<f64>().ok().map(|f| f != 0.0),
        _ => None,
    }
}

/// 把源值转换成目标列的类型。读不全的大字段（超过上限）算出错。
pub fn to_target(v: &Value, col: &Column) -> Result<Value> {
    use LogicalType as L;
    if let Value::Lob(l) = v {
        if l.truncated {
            return Err(Error::invalid(msg!("migrate.lob_too_large", column = col.name)));
        }
        // 没有截断的大字段：文本原样，二进制是 base64
        let full = if l.binary {
            Value::Bytes(STANDARD.decode(&l.preview).map_err(|e| {
                Error::invalid(msg!("migrate.bad_value", column = col.name, error = e.to_string()))
            })?)
        } else {
            Value::String(l.preview.clone())
        };
        return to_target(&full, col);
    }
    if matches!(v, Value::Null) {
        return Ok(Value::Null);
    }
    let logical = &col.data_type.logical;
    let out = match (logical, v) {
        (L::Bool, _) => match truthy(v) {
            Some(b) => Value::Bool(b),
            None => convert(v, col, EmptyAs::Null)?,
        },
        (L::Int { .. } | L::UInt { .. } | L::Decimal { .. } | L::Float { .. }, Value::Bool(b)) => {
            convert(&Value::Int(i64::from(*b)), col, EmptyAs::Null)?
        }
        (L::Char { .. } | L::VarChar { .. } | L::Text | L::Interval | L::Other, Value::String(_)) => {
            v.clone()
        }
        (L::Char { .. } | L::VarChar { .. } | L::Text | L::Interval | L::Other, _) => {
            Value::String(match v {
                // 二进制转文本时按 UTF-8 解码（大多是文本存在二进制列里）
                Value::Bytes(b) => String::from_utf8_lossy(b).into_owned(),
                _ => text_of(v),
            })
        }
        (L::Binary { .. } | L::VarBinary { .. } | L::Blob, Value::Bytes(_)) => v.clone(),
        (L::Binary { .. } | L::VarBinary { .. } | L::Blob, Value::String(s)) => {
            Value::Bytes(s.as_bytes().to_vec())
        }
        (L::Binary { .. } | L::VarBinary { .. } | L::Blob, Value::Uuid(u)) => {
            Value::Bytes(u.as_bytes().to_vec())
        }
        (L::Uuid, Value::Bytes(b)) if b.len() == 16 => {
            Value::Uuid(uuid::Uuid::from_slice(b).map_err(|e| {
                Error::invalid(msg!("migrate.bad_value", column = col.name, error = e.to_string()))
            })?)
        }
        (L::Json, Value::Json(_)) => v.clone(),
        (L::Json, _) => match serde_json::from_str(&text_of(v)) {
            Ok(j) => Value::Json(j),
            Err(_) => convert(v, col, EmptyAs::Null)?,
        },
        // 带时区转不带时区：按其自身偏移的本地时间
        (L::Timestamp { tz: false, .. }, Value::DateTimeTz(t)) => Value::DateTime(t.naive_local()),
        (L::Date, Value::DateTimeTz(t)) => Value::Date(t.naive_local().date()),
        (L::Timestamp { tz: true, .. }, Value::DateTime(t)) => {
            match Local.from_local_datetime(t).earliest() {
                Some(x) => Value::DateTimeTz(x.fixed_offset()),
                None => convert(v, col, EmptyAs::Null)?,
            }
        }
        (L::Time { .. }, Value::DateTimeTz(t)) => Value::Time(t.naive_local().time()),
        (L::Int { .. } | L::UInt { .. }, Value::Decimal(s)) => {
            // 整数位后面是 .0 的小数（如 Oracle 的 NUMBER）
            let s = s.trim().strip_suffix(".0").unwrap_or(s.trim());
            convert(&Value::String(s.to_string()), col, EmptyAs::Null)?
        }
        (L::Decimal { .. }, Value::Float(f)) => Value::Decimal(f.to_string()),
        _ => convert(v, col, EmptyAs::Null)?,
    };
    Ok(out)
}

/// 去掉小数末尾的 0（`1.50` → `1.5`，`2.00` → `2`）。
fn trim_decimal(s: &str) -> String {
    let s = s.trim();
    if !s.contains('.') || s.contains(['e', 'E']) {
        return s.to_string();
    }
    let t = s.trim_end_matches('0').trim_end_matches('.');
    if t.is_empty() || t == "-" { "0".into() } else { t.to_string() }
}

fn truncate(t: &NaiveDateTime, precision: Option<u8>) -> NaiveDateTime {
    let p = u32::from(precision.unwrap_or(6).min(9));
    let unit = 10u32.pow(9 - p);
    t.with_nanosecond(t.nanosecond() / unit * unit).unwrap_or(*t)
}

/// 校验用的规整：转换成目标列的类型后写成文本。小数去掉末尾的 0，时间按目标精度截断，CHAR 去掉末尾空格，
/// 浮点按 f32 精度比较（单精度列）。`empty_is_null`：目标按 Oracle 的规则把空串、空二进制存成 NULL。
pub fn normalize(v: &Value, col: &Column, empty_is_null: bool) -> String {
    use LogicalType as L;
    let logical = &col.data_type.logical;
    let Ok(v) = to_target(v, col) else { return format!("!{}", text_of(v)) };
    let empty = match &v {
        Value::String(s) => s.is_empty(),
        Value::Bytes(b) => b.is_empty(),
        _ => false,
    };
    match (&v, logical) {
        (Value::Null, _) => "NULL".into(),
        _ if empty && empty_is_null => "NULL".into(),
        (Value::Bool(b), _) => (if *b { "1" } else { "0" }).into(),
        (Value::Decimal(s), _) => trim_decimal(s),
        (Value::Int(n), _) => n.to_string(),
        (Value::Float(f), L::Float { bits }) if *bits <= 32 => (*f as f32).to_string(),
        (Value::Float(f), _) => f.to_string(),
        (Value::String(s), L::Char { .. }) => s.trim_end().to_string(),
        (Value::DateTime(t), L::Timestamp { precision, .. }) => truncate(t, *precision).to_string(),
        (Value::DateTimeTz(t), L::Timestamp { precision, .. }) => {
            truncate(&t.naive_utc(), *precision).to_string()
        }
        (Value::Json(j), _) => j.to_string(),
        _ => text_of(&v),
    }
}

/// 主键值的显示（报告中）。
pub fn key_text(values: &[Value]) -> String {
    values.iter().map(text_of).collect::<Vec<_>>().join(", ")
}

#[cfg(test)]
mod tests {
    use chrono::{FixedOffset, NaiveDate};
    use dblens_core::ColumnType;

    use super::*;

    fn col(native: &str, logical: LogicalType) -> Column {
        Column {
            name: "c".into(),
            ordinal: 1,
            data_type: ColumnType { native: native.into(), logical },
            nullable: true,
            default: None,
            comment: None,
            auto_increment: false,
            generated: None,
        }
    }

    #[test]
    fn values_convert_to_target_types() {
        use LogicalType as L;
        let num1 = col("NUMBER(1)", L::Decimal { precision: Some(1), scale: Some(0) });
        assert_eq!(to_target(&Value::Bool(true), &num1).unwrap(), Value::Decimal("1".into()));
        assert_eq!(
            to_target(&Value::Decimal("0".into()), &col("boolean", L::Bool)).unwrap(),
            Value::Bool(false)
        );
        assert_eq!(
            to_target(&Value::Decimal("42".into()), &col("bigint", L::Int { bits: 64 })).unwrap(),
            Value::Int(42)
        );
        let text = col("text", L::Text);
        let u = uuid::Uuid::nil();
        assert_eq!(to_target(&Value::Uuid(u), &text).unwrap(), Value::String(u.to_string()));
        assert_eq!(
            to_target(&Value::Json(serde_json::json!({"a": 1})), &text).unwrap(),
            Value::String("{\"a\":1}".into())
        );
        let ts = col("datetime", L::Timestamp { tz: false, precision: Some(0) });
        let tz = FixedOffset::east_opt(8 * 3600).unwrap().with_ymd_and_hms(2026, 1, 2, 3, 4, 5).unwrap();
        assert_eq!(
            to_target(&Value::DateTimeTz(tz), &ts).unwrap(),
            Value::DateTime(NaiveDate::from_ymd_opt(2026, 1, 2).unwrap().and_hms_opt(3, 4, 5).unwrap())
        );
        assert_eq!(
            to_target(&Value::String("ab".into()), &col("bytea", L::Blob)).unwrap(),
            Value::Bytes(b"ab".to_vec())
        );
        assert_eq!(to_target(&Value::Null, &ts).unwrap(), Value::Null);
        let bad = to_target(&Value::String("x".into()), &col("int", L::Int { bits: 32 }));
        assert!(bad.is_err());
        let lob = |truncated| {
            Value::Lob(dblens_core::LobPreview {
                binary: false,
                preview: "x".into(),
                total_len: 9,
                truncated,
            })
        };
        assert!(to_target(&lob(true), &text).is_err());
        assert_eq!(to_target(&lob(false), &text).unwrap(), Value::String("x".into()));
    }

    #[test]
    fn normalize_for_comparison() {
        use LogicalType as L;
        let n = |v: Value, c: &Column| normalize(&v, c, false);
        let dec = col("numeric(12,2)", L::Decimal { precision: Some(12), scale: Some(2) });
        assert_eq!(n(Value::Decimal("1.50".into()), &dec), n(Value::Decimal("1.5".into()), &dec));
        assert_eq!(n(Value::Decimal("2.00".into()), &dec), "2");
        let ch = col("char(5)", L::Char { len: Some(5) });
        assert_eq!(n(Value::String("ab   ".into()), &ch), "ab");
        let ts = col("datetime(0)", L::Timestamp { tz: false, precision: Some(0) });
        let t = NaiveDate::from_ymd_opt(2026, 1, 2).unwrap().and_hms_micro_opt(3, 4, 5, 678).unwrap();
        assert_eq!(n(Value::DateTime(t), &ts), "2026-01-02 03:04:05");
        let b = col("NUMBER(1)", L::Decimal { precision: Some(1), scale: Some(0) });
        assert_eq!(n(Value::Bool(true), &b), n(Value::Decimal("1".into()), &b));
        // Oracle 的空串即 NULL
        let raw = col("RAW(16)", L::VarBinary { len: Some(16) });
        assert_eq!(normalize(&Value::Bytes(vec![]), &raw, true), normalize(&Value::Null, &raw, true));
        assert_ne!(n(Value::String(String::new()), &ch), n(Value::Null, &ch));
        assert_eq!(key_text(&[Value::Int(1), Value::String("a".into())]), "1, a");
    }
}
```

- [ ] **Step 14: 修改 `crates/dblens-service/src/service.rs`**

```diff
--- a/crates/dblens-service/src/service.rs
+++ b/crates/dblens-service/src/service.rs
@@ -9,6 +9,7 @@ use dblens_dialect::{Dialect, OpsQuery, Variant};
 use crate::export::ExportManager;
 use crate::history::HistoryLog;
 use crate::metadata::{MetadataCache, MetadataService};
+use crate::migrate::MigrationRecords;
 use crate::model::ModelStore;
 use crate::policy::{DefaultPolicy, Policy};
 use crate::profile::{Profile, ProfileInput};
@@ -33,6 +34,8 @@ pub struct Service {
     pub tasks: TaskManager,
     /// 打开的模型（M4a）。
     pub models: ModelStore,
+    /// 迁移记录（M5b）。
+    pub migrations: MigrationRecords,
     pub(crate) history: HistoryLog,
 }
 
@@ -46,6 +49,7 @@ impl Service {
         service.apply_lang_setting()?;
         service.exports.set_dir(data_dir.join("exports"));
         service.tasks.set_dirs(data_dir.join("tasks"), data_dir.join("imports"));
+        service.migrations.set_dir(data_dir.join("migrations"));
         Ok(service)
     }
 
@@ -82,6 +86,7 @@ impl Service {
             exports: ExportManager::default(),
             tasks: TaskManager::default(),
             models: ModelStore::default(),
+            migrations: MigrationRecords::default(),
             history,
             store,
             secrets,
```

- [ ] **Step 15: 修改 `crates/dblens-service/src/task.rs`**

```diff
--- a/crates/dblens-service/src/task.rs
+++ b/crates/dblens-service/src/task.rs
@@ -123,6 +123,8 @@ pub struct TaskStatus {
     pub remaining: Option<String>,
     /// 反向生成：生成并打开的模型的编号。
     pub model: Option<String>,
+    /// 迁移（M5b）：迁移记录的编号、当前步骤、每张表的进度。
+    pub migration: Option<crate::migrate::MigrationProgress>,
 }
 
 struct Entry {
@@ -160,11 +162,6 @@ impl TaskHandle {
         }
     }
 
-    /// 当前状态的副本。
-    pub fn status(&self) -> TaskStatus {
-        self.entry.status.lock().map(|s| s.clone()).unwrap_or_else(|e| e.into_inner().clone())
-    }
-
     pub fn cancelled(&self) -> bool {
         self.entry.cancelled.load(Ordering::SeqCst)
     }
@@ -415,6 +412,7 @@ impl TaskManager {
                 confirm: None,
                 remaining: None,
                 model: None,
+                migration: None,
                 report: None,
             }),
             cancelled: AtomicBool::new(false),
```

- [ ] **Step 16: 检查**

```bash
export CARGO_INCREMENTAL=0
cargo fmt --all
cargo clippy --config "source.mirror.registry='sparse+https://rsproxy.cn/index/'" --workspace --all-targets -- -D warnings
DBLENS_LANG=en cargo test --config "source.mirror.registry='sparse+https://rsproxy.cn/index/'" --workspace
```

实库测试：在上面的 `cargo test` 前以内联环境变量给出原有 5 个库的 `DBLENS_TEST_MYSQL_URL`、`DBLENS_TEST_MARIADB_URL`、`DBLENS_TEST_PG_URL`、`DBLENS_TEST_ORACLE_URL`、`DBLENS_TEST_DM_URL`，以及国产库第一批的 `DBLENS_TEST_TIDB_URL`、`DBLENS_TEST_OCEANBASE_URL`（用户名写成 `admin%40test`）、`DBLENS_TEST_OPENGAUSS_URL`、`DBLENS_TEST_OPENGAUSS_A_URL`（库 `dblens_test_a`）、`DBLENS_TEST_OPENGAUSS_SHA_URL`（账号 `admin_sha`，库 `dblens_test_sha`）、`DBLENS_TEST_KINGBASE_URL`（54321）、`DBLENS_TEST_KINGBASE_PG_URL`（54322）。新目标的密码在测试虚拟机 root 的 `/root/dblens-test/creds.env`，用 `ssh … 'sudo cat …'` 读进当前 shell 的变量，不写进任何文件。只跑迁移的测试：`cargo test … -p dblens-service --test migrate_live -- --test-threads=1`（源和目标的环境变量都设置时才运行该路线）。
- 人大金仓的开发版授权最多 10 个连接：它的测试要串行运行（`--test-threads=1`）。
- 达梦偶发"网络通讯失败"，失败时单独重跑该路线（如 `dameng_to_mysql`）。

Expected: 全部测试通过（service 新增 4 个，api 的迁移测试多 3 处检查）；迁移的实库测试 52 个。

- [ ] **Step 17: 提交**

```bash
git add crates/dblens-api/src/lib.rs crates/dblens-api/src/migrations.rs crates/dblens-api/tests/models.rs crates/dblens-core/locales/app.json crates/dblens-service/src/import/job.rs crates/dblens-service/src/lib.rs crates/dblens-service/src/migrate.rs crates/dblens-service/src/migrate/copy.rs crates/dblens-service/src/migrate/mod.rs crates/dblens-service/src/migrate/record.rs crates/dblens-service/src/migrate/report.rs crates/dblens-service/src/migrate/run.rs crates/dblens-service/src/migrate/values.rs crates/dblens-service/src/service.rs crates/dblens-service/src/task.rs crates/dblens-service/tests/migrate_live.rs
git commit -m "feat(migrate): 迁移记录、数据搬运、续传与校验" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 4: 前端：数据迁移的选项、进度与未完成的迁移

- `shared/api.ts`：`MigrationMode`、`DataOptions`、`ExistingAction` 加 `append`/`replace`、`TableOverride` 加 `filter`/`structure_only`、`MigrationPlan` 加 `mode`/`data`、`MigrationPhase`、`MigrationVerify`、`MigrationProgress`、`MigrationSummary`、`TaskStatus.migration`；
  方法 `unfinishedMigrations`、`resumeMigration`、`discardMigration`。
- `features/migrate/plan.ts`：`DEFAULT_DATA`、新方案带模式和数据选项、`existingActions(mode)`、`setTable` 可改过滤条件和只迁结构、打开旧方案时补默认值；测试随之补充。
- `features/migrate/progress.tsx`（新）：`ProgressTable`（当前步骤、按表的状态 / 已迁 / 估计 / 跳过 / 用时 / 校验）、`UnfinishedList`（继续、放弃）。
- `features/migrate/MigrateView.tsx`：模式和数据选项一行、表的过滤条件和"只迁结构"、同名表的处理方式随模式变化、进度表、标签顶部的未完成的迁移、"迁移期间源库最好不要写入"的提示；只迁数据时没有语句也能开始。
- `features/editor/tabs.ts`：恢复迁移标签时用 `parsePlan` 补上后来加的项（`tabs.test.ts` 加测试）。
- `app/shell.css`：进度表的样式。词条：`migrate.json` 加模式、数据选项、过滤条件、进度、未完成的迁移等。

**Files:**
- Modify: `web/src/app/shell.css`
- Modify: `web/src/features/editor/tabs.test.ts`
- Modify: `web/src/features/editor/tabs.ts`
- Modify: `web/src/features/migrate/MigrateView.tsx`
- Modify: `web/src/features/migrate/plan.test.ts`
- Modify: `web/src/features/migrate/plan.ts`
- Create: `web/src/features/migrate/progress.tsx`
- Modify: `web/src/locales/en/migrate.json`
- Modify: `web/src/locales/zh-CN/migrate.json`
- Modify: `web/src/shared/api.ts`

**Interfaces:**
- Consumes：任务 3 的接口。
- Produces：`existingActions(mode)`、`DEFAULT_DATA`、`ProgressTable`、`UnfinishedList`。

- [ ] **Step 1: 修改 `web/src/features/editor/tabs.test.ts`**

```diff
--- a/web/src/features/editor/tabs.test.ts
+++ b/web/src/features/editor/tabs.test.ts
@@ -191,3 +191,20 @@ describe('保存标签', () => {
     expect(parseTabs(value).tabs[0].sql).toBe('se')
   })
 })
+
+describe('迁移标签', () => {
+  it('恢复时给旧方案补上模式和数据选项', () => {
+    const old = {
+      source: { profile: 'a', schema: 's' },
+      target: { profile: 'b', schema: 't' },
+      tables: null,
+      sequences: null,
+      settings: { options: { name_case: null, comments: true, checks: true }, type_rules: [], tables: {}, sequences: {} },
+      existing: {},
+    }
+    const raw = JSON.stringify({ tabs: [{ id: 'm', title: '迁移', profileId: '', sql: '', migrate: old }], active: 'm', seq: 2 })
+    const plan = parseTabs(raw).tabs[0].migrate
+    expect(plan?.mode).toBe('all')
+    expect(plan?.data.batch_size).toBe(1000)
+  })
+})
```

- [ ] **Step 2: 修改 `web/src/features/migrate/plan.test.ts`**

```diff
--- a/web/src/features/migrate/plan.test.ts
+++ b/web/src/features/migrate/plan.test.ts
@@ -1,7 +1,21 @@
 import { describe, expect, it } from 'vitest'
 
 import type { MigrationPreview } from '../../shared/api'
-import { emptyPlan, notesOf, parsePlan, ready, setCheck, setColumn, setExisting, setSequence, setTable, summary, unchosen, worst } from './plan'
+import {
+  emptyPlan,
+  existingActions,
+  notesOf,
+  parsePlan,
+  ready,
+  setCheck,
+  setColumn,
+  setExisting,
+  setSequence,
+  setTable,
+  summary,
+  unchosen,
+  worst,
+} from './plan'
 
 const src = { profile: 'a', schema: 's' }
 
@@ -67,6 +81,16 @@ describe('迁移方案', () => {
     expect(p.settings.tables.t.columns?.c).toEqual({ default: '' })
   })
 
+  it('数据选项：过滤条件、只迁结构、同名表可选的处理方式', () => {
+    let p = setTable(emptyPlan(src), 't', { filter: 'x > 1', structure_only: true })
+    expect(p.settings.tables.t).toEqual({ filter: 'x > 1', structure_only: true })
+    p = setTable(p, 't', { filter: undefined, structure_only: false })
+    expect(p.settings.tables).toEqual({})
+    expect(existingActions('structure')).not.toContain('append')
+    expect(existingActions('data')).toEqual(['skip', 'append', 'replace'])
+    expect(existingActions('all').length).toBe(5)
+  })
+
   it('表、检查约束、序列的覆盖', () => {
     let p = setTable(emptyPlan(src), 't', { skip: true })
     expect(p.settings.tables.t).toEqual({ skip: true })
@@ -97,6 +121,10 @@ describe('迁移方案', () => {
   it('打开另存的方案：补默认值，格式不对返回 null', () => {
     const p = parsePlan(JSON.stringify({ source: src, target: { profile: 'b', schema: 't' }, settings: { type_rules: [{ source: 'CLOB', target: 'TEXT' }] } }))
     expect(p?.settings.options).toEqual({ name_case: null, comments: true, checks: true })
+    // 旧方案没有模式和数据选项时补默认值
+    expect(p?.mode).toBe('all')
+    expect(p?.data).toEqual({ batch_size: 1000, parallel: 2, on_error: 'stop', sample: 0 })
+    expect(parsePlan(JSON.stringify({ ...p, mode: 'data', data: { sample: 50 } }))?.data.sample).toBe(50)
     expect(p?.settings.type_rules).toEqual([{ source: 'CLOB', target: 'TEXT' }])
     expect(p?.tables).toBeNull()
     expect(parsePlan('{"source": 1}')).toBeNull()
```

- [ ] **Step 3: 修改 `web/src/app/shell.css`**

```diff
--- a/web/src/app/shell.css
+++ b/web/src/app/shell.css
@@ -808,6 +808,11 @@ body {
   flex-direction: column;
   gap: 6px;
 }
+.migrate-progress {
+  padding: 0 10px;
+  max-height: 240px;
+  overflow: auto;
+}
 .migrate-ddl {
   flex: 2;
   min-width: 0;
```

- [ ] **Step 4: 修改 `web/src/features/editor/tabs.ts`**

```diff
--- a/web/src/features/editor/tabs.ts
+++ b/web/src/features/editor/tabs.ts
@@ -12,6 +12,7 @@ import { create } from 'zustand'
 import type { MigrationPlan, TargetKind } from '../../shared/api'
 import { t } from '../../shared/i18n'
 import { useSettings } from '../../shared/settings'
+import { parsePlan } from '../migrate/plan'
 
 export const TABS_KEY = 'ui.tabs'
 
@@ -144,6 +145,10 @@ export function parseTabs(raw: string | undefined): Persisted {
         (t.er === undefined || isErTarget(t.er)) &&
         (t.migrate === undefined || isMigratePlan(t.migrate)),
     )
+    // 迁移标签的方案补上后来加的项（M5b 的模式、数据选项），格式不对的丢掉
+    for (const t of tabs) {
+      if (t.migrate) t.migrate = parsePlan(JSON.stringify(t.migrate)) ?? undefined
+    }
     const active = tabs.some((t) => t.id === v.active) ? (v.active as string) : (tabs[0]?.id ?? null)
     return { tabs, active, seq: typeof v.seq === 'number' && v.seq > 0 ? v.seq : tabs.length + 1 }
   } catch {
```

- [ ] **Step 5: 修改 `web/src/features/migrate/MigrateView.tsx`**

```diff
--- a/web/src/features/migrate/MigrateView.tsx
+++ b/web/src/features/migrate/MigrateView.tsx
@@ -1,10 +1,11 @@
 // 迁移标签（M5a 设计文档第 5 节）：上方选择源和目标、选项、打开/另存方案；读取结构后左侧对象列表（带提示、只看有问题的），
 // 中间选中表的列映射表格（改目标名、类型、默认值，不迁某列，不认识的类型确认按文本迁），右侧这张表的目标 DDL；
 // 底部同名表的处理方式、不能开始的原因、汇总、另存脚本、开始迁移，执行中显示进度，完成后可取迁移报告。由工作区懒加载。
+// 数据迁移（M5b）：模式和数据选项、每张表的过滤条件和"只迁结构"、按表的进度、标签顶部的未完成的迁移。
 
 import { IconDeviceFloppy, IconFolderOpen, IconPlayerPlay, IconRefresh, IconSettings } from '@tabler/icons-react'
-import { keepPreviousData, useQuery } from '@tanstack/react-query'
-import { Alert, App, Button, Checkbox, Empty, Input, Modal, Progress, Select, Space, Spin, Table, Tag } from 'antd'
+import { keepPreviousData, useQuery, useQueryClient } from '@tanstack/react-query'
+import { Alert, App, Button, Checkbox, Empty, Input, InputNumber, Modal, Progress, Select, Space, Spin, Table, Tag } from 'antd'
 import { useEffect, useMemo, useRef, useState } from 'react'
 import { useTranslation } from 'react-i18next'
 
@@ -13,6 +14,7 @@ import { useUi } from '../../app/ui'
 import {
   type ExistingAction,
   type MigrationEnd,
+  type MigrationMode,
   type MigrationNote,
   type MigrationPlan,
   type NameCase,
@@ -25,7 +27,9 @@ import { desktop } from '../../shared/shell'
 import { withVault } from '../connections/vault'
 import SqlBox from '../design/SqlBox'
 import { type QueryTab, useTabs } from '../editor/tabs'
+import { ProgressTable, UnfinishedList } from './progress'
 import {
+  existingActions,
   notesOf,
   parsePlan,
   ready,
@@ -41,7 +45,6 @@ import {
 } from './plan'
 
 const LEVEL_COLOR: Record<NoteLevel, string> = { info: 'default', lossy: 'orange', unsupported: 'red' }
-const ACTIONS: ExistingAction[] = ['skip', 'recreate', 'missing_only']
 
 /** 一端的选择：连接，再选 schema。 */
 function EndPicker({ label, value, profiles, onChange }: {
@@ -160,6 +163,8 @@ export default function MigrateView({ tab, profiles }: { tab: QueryTab; profiles
   const [task, setTask] = useState<TaskStatus | null>(null)
   const [busy, setBusy] = useState(false)
   const file = useRef<HTMLInputElement>(null)
+  const qc = useQueryClient()
+  const track = (id: string) => setTask({ id, state: 'running', progress: { statements_done: 0, statements_total: 0 }, failures: [], migration: null } as unknown as TaskStatus)
 
   const { data: p, error, isFetching, refetch } = useQuery({
     queryKey: ['migration-preview', JSON.stringify(plan)],
@@ -184,7 +189,8 @@ export default function MigrateView({ tab, profiles }: { tab: QueryTab; profiles
         setTask(st)
         if (st.state !== 'running') {
           useUi.getState().refreshTree(plan.target.profile, plan.target.schema)
-          void refetch()
+          void qc.invalidateQueries({ queryKey: ['migrations'] })
+          if (loaded) void refetch()
         }
       } catch (e) {
         message.error((e as Error).message)
@@ -192,7 +198,7 @@ export default function MigrateView({ tab, profiles }: { tab: QueryTab; profiles
       }
     }, 500)
     return () => clearTimeout(h)
-  }, [task, api, message, plan.target, refetch])
+  }, [task, api, message, plan.target, refetch, qc, loaded])
 
   const target = profiles.find((x) => x.id === plan.target.profile)
   const sum = p && summary(p)
@@ -202,8 +208,7 @@ export default function MigrateView({ tab, profiles }: { tab: QueryTab; profiles
   const start = async () => {
     setBusy(true)
     try {
-      const id = await withVault(api, () => api.startMigration(plan))
-      setTask({ id, state: 'running', progress: { statements_done: 0, statements_total: 0 }, failures: [] } as unknown as TaskStatus)
+      track(await withVault(api, () => api.startMigration(plan)))
     } catch (e) {
       message.error((e as Error).message)
     } finally {
@@ -290,6 +295,8 @@ export default function MigrateView({ tab, profiles }: { tab: QueryTab; profiles
   const source = focus?.kind === 'table' ? p?.source_tables.find((x) => x.name === focus.name) : undefined
   const mapped = focus ? p?.draft.tables.find((d) => d.source === focus.name) : undefined
   const tov = focus ? plan.settings.tables[focus.name] : undefined
+  const data = plan.mode !== 'structure'
+  const setData = (patch: Partial<MigrationPlan['data']>) => setPlan({ ...plan, data: { ...plan.data, ...patch } })
   const rows = (source?.columns ?? []).map((c) => {
     const target = mapped?.columns.find((m) => m.source === c.name)?.target
     return { c, ov: tov?.columns?.[c.name] ?? {}, tc: mapped?.target.columns.find((x) => x.name === target), notes: p ? notesOf(p, 'table', focus!.name, c.name) : [] }
@@ -297,6 +304,7 @@ export default function MigrateView({ tab, profiles }: { tab: QueryTab; profiles
 
   return (
     <div className="compare-view">
+      <UnfinishedList onResume={track} />
       <div className="row toolbar" style={{ flexWrap: 'wrap', gap: 8 }}>
         <EndPicker label={t('migrate.source')} value={plan.source} profiles={profiles} onChange={(s) => setEnds(s, plan.target)} />
         <span>→</span>
@@ -341,6 +349,35 @@ export default function MigrateView({ tab, profiles }: { tab: QueryTab; profiles
           {plan.settings.type_rules.length ? ` (${plan.settings.type_rules.length})` : ''}
         </Button>
       </div>
+      <div className="row toolbar" style={{ gap: 12, flexWrap: 'wrap' }}>
+        <span>{t('migrate.mode.label')}</span>
+        <Select
+          size="small"
+          style={{ width: 140 }}
+          value={plan.mode}
+          onChange={(mode: MigrationMode) => setPlan({ ...plan, mode, existing: {} })}
+          options={(['all', 'structure', 'data'] as const).map((v) => ({ value: v, label: t(`migrate.mode.${v}`) }))}
+        />
+        {data && (
+          <>
+            <span>{t('migrate.data.batch')}</span>
+            <InputNumber size="small" min={100} max={100000} step={500} value={plan.data.batch_size} onChange={(v) => v && setData({ batch_size: v })} />
+            <span>{t('migrate.data.parallel')}</span>
+            <InputNumber size="small" min={1} max={4} value={plan.data.parallel} onChange={(v) => v && setData({ parallel: v })} />
+            <span>{t('migrate.data.onError')}</span>
+            <Select
+              size="small"
+              style={{ width: 130 }}
+              value={plan.data.on_error}
+              onChange={(on_error: 'stop' | 'skip') => setData({ on_error })}
+              options={(['stop', 'skip'] as const).map((v) => ({ value: v, label: t(`migrate.data.${v}`) }))}
+            />
+            <span title={t('migrate.data.sampleHint')}>{t('migrate.data.sample')}</span>
+            <InputNumber size="small" min={0} max={10000} step={100} value={plan.data.sample} onChange={(v) => setData({ sample: v ?? 0 })} />
+          </>
+        )}
+      </div>
+      {task?.migration && <ProgressTable progress={task.migration} />}
       {error ? (
         <Alert type="error" showIcon message={(error as Error).message} />
       ) : !p ? (
@@ -402,9 +439,20 @@ export default function MigrateView({ tab, profiles }: { tab: QueryTab; profiles
                 </>
               ) : (
                 <>
-                  <Space>
+                  <Space wrap>
                     {t('migrate.tableName')}
                     <LazyInput value={tov?.name} placeholder={mapped?.target.name ?? focus.name} onCommit={(name) => setPlan(setTable(plan, focus.name, { name }))} />
+                    {data && (
+                      <>
+                        <LazyInput value={tov?.filter} placeholder={t('migrate.filter')} onCommit={(filter) => setPlan(setTable(plan, focus.name, { filter }))} />
+                        <Checkbox
+                          checked={!!tov?.structure_only}
+                          onChange={(e) => setPlan(setTable(plan, focus.name, { structure_only: e.target.checked }))}
+                        >
+                          {t('migrate.structureOnly')}
+                        </Checkbox>
+                      </>
+                    )}
                   </Space>
                   <NoteTags notes={notesOf(p, 'table', focus.name, null)} />
                   <Table
@@ -495,7 +543,7 @@ export default function MigrateView({ tab, profiles }: { tab: QueryTab; profiles
                 placeholder={t('migrate.existing.pick')}
                 value={null}
                 onChange={(a: ExistingAction) => setPlan(setExisting(plan, p.draft.tables.filter((x) => x.exists).map((x) => x.source), a))}
-                options={ACTIONS.map((a) => ({ value: a, label: t(`migrate.existing.${a}`) }))}
+                options={existingActions(plan.mode).map((a) => ({ value: a, label: t(`migrate.existing.${a}`) }))}
               />
               {p.draft.tables
                 .filter((x) => x.exists)
@@ -509,7 +557,7 @@ export default function MigrateView({ tab, profiles }: { tab: QueryTab; profiles
                       placeholder={t('migrate.existing.pick')}
                       value={plan.existing[x.source]}
                       onChange={(a: ExistingAction) => setPlan(setExisting(plan, [x.source], a))}
-                      options={ACTIONS.map((a) => ({ value: a, label: t(`migrate.existing.${a}`) }))}
+                      options={existingActions(plan.mode).map((a) => ({ value: a, label: t(`migrate.existing.${a}`) }))}
                     />
                   </Space.Compact>
                 ))}
@@ -525,9 +573,11 @@ export default function MigrateView({ tab, profiles }: { tab: QueryTab; profiles
               showIcon
               message={
                 task.state === 'done'
-                  ? task.failures.length
-                    ? t('migrate.doneWithFailures', { count: task.progress.statements_failed })
-                    : t('migrate.done')
+                  ? task.migration?.tables.some((x) => x.state === 'failed')
+                    ? t('migrate.unfinishedDone')
+                    : task.failures.length
+                      ? t('migrate.doneWithFailures', { count: task.progress.statements_failed })
+                      : t('migrate.done')
                   : t(task.state === 'cancelled' ? 'migrate.cancelled' : 'migrate.failed')
               }
               description={
@@ -563,6 +613,7 @@ export default function MigrateView({ tab, profiles }: { tab: QueryTab; profiles
               sum && <span>{t('migrate.summary', sum)}</span>
             )}
             <span className="grow" />
+            {data && <span className="muted">{t('migrate.sourceHint')}</span>}
             <Button size="small" icon={<IconDeviceFloppy size={16} />} disabled={!p.statements.length} onClick={() => void saveScript()}>
               {t('migrate.saveScript')}
             </Button>
@@ -572,7 +623,7 @@ export default function MigrateView({ tab, profiles }: { tab: QueryTab; profiles
               danger={!!p.confirm.length}
               icon={<IconPlayerPlay size={16} />}
               loading={busy}
-              disabled={running || isFetching || !!p.problems.length || !!p.denied || pending.length > 0 || !p.statements.length}
+              disabled={running || isFetching || !!p.problems.length || !!p.denied || pending.length > 0 || (!data && !p.statements.length)}
               onClick={confirmStart}
             >
               {t('migrate.start')}
```

- [ ] **Step 6: 修改 `web/src/features/migrate/plan.ts`**

```diff
--- a/web/src/features/migrate/plan.ts
+++ b/web/src/features/migrate/plan.ts
@@ -3,8 +3,10 @@
 
 import type {
   ColumnOverride,
+  DataOptions,
   ExistingAction,
   MigrationEnd,
+  MigrationMode,
   MigrationNote,
   MigrationPlan,
   MigrationPreview,
@@ -12,17 +14,28 @@ import type {
   TableOverride,
 } from '../../shared/api'
 
+export const DEFAULT_DATA: DataOptions = { batch_size: 1000, parallel: 2, on_error: 'stop', sample: 0 }
+
 export function emptyPlan(source: MigrationEnd): MigrationPlan {
   return {
     source,
     tables: null,
     sequences: null,
     target: { profile: '', schema: '' },
+    mode: 'all',
     settings: { options: { name_case: null, comments: true, checks: true }, type_rules: [], tables: {}, sequences: {} },
+    data: { ...DEFAULT_DATA },
     existing: {},
   }
 }
 
+/** 目标中已有同名表时可选的处理方式：只迁结构时不能追加或清空，只迁数据时不动结构。 */
+export function existingActions(mode: MigrationMode): ExistingAction[] {
+  if (mode === 'structure') return ['skip', 'recreate', 'missing_only']
+  if (mode === 'data') return ['skip', 'append', 'replace']
+  return ['skip', 'recreate', 'missing_only', 'append', 'replace']
+}
+
 /** 两端都选好了 schema。 */
 export function ready(plan: MigrationPlan): boolean {
   return !!(plan.source.profile && plan.source.schema && plan.target.profile && plan.target.schema)
@@ -53,8 +66,12 @@ export function setColumn(plan: MigrationPlan, table: string, column: string, pa
   })
 }
 
-/** 修改表的覆盖（目标名、不迁移）。 */
-export function setTable(plan: MigrationPlan, table: string, patch: Pick<TableOverride, 'name' | 'skip'>): MigrationPlan {
+/** 修改表的覆盖（目标名、不迁移、过滤条件、只迁结构）。 */
+export function setTable(
+  plan: MigrationPlan,
+  table: string,
+  patch: Pick<TableOverride, 'name' | 'skip' | 'filter' | 'structure_only'>,
+): MigrationPlan {
   return withTable(plan, table, (t) => ({ ...t, ...patch }))
 }
 
@@ -136,6 +153,8 @@ export function parsePlan(text: string): MigrationPlan | null {
       tables: v.tables ?? null,
       sequences: v.sequences ?? null,
       target: v.target,
+      mode: v.mode === 'structure' || v.mode === 'data' ? v.mode : 'all',
+      data: { ...DEFAULT_DATA, ...v.data },
       settings: {
         options: { ...base.settings.options, ...s.options },
         type_rules: Array.isArray(s.type_rules) ? s.type_rules : [],
```

- [ ] **Step 7: 新建 `web/src/features/migrate/progress.tsx`**

```tsx
// 迁移的进度与未完成的迁移（M5b 设计文档第 6 节）：执行中按表列出状态、已迁行数 / 估计、跳过、用时、校验结果；
// 标签顶部列出没有完成的迁移，可继续或放弃。

import { useQuery, useQueryClient } from '@tanstack/react-query'
import { Alert, App, Button, Space, Table, Tag } from 'antd'
import { useTranslation } from 'react-i18next'

import { useApi } from '../../app/context'
import type { MigrationProgress } from '../../shared/api'
import { withVault } from '../connections/vault'

const STATE_COLOR: Record<MigrationProgress['tables'][number]['state'], string> = {
  pending: 'default',
  copying: 'processing',
  done: 'success',
  failed: 'error',
  skipped: 'default',
}

export function ProgressTable({ progress }: { progress: MigrationProgress }) {
  const { t } = useTranslation()
  if (!progress.tables.length) return null
  return (
    <div className="migrate-progress">
      <b>{t(`migrate.phase.${progress.phase}`)}</b>
      <Table
        size="small"
        pagination={false}
        rowKey="source"
        dataSource={progress.tables}
        columns={[
          { title: t('migrate.progress.table'), render: (_, r) => `${r.source} → ${r.target}` },
          {
            title: t('migrate.progress.state'),
            render: (_, r) => (
              <Tag color={STATE_COLOR[r.state]} title={r.error ?? undefined}>
                {t(`migrate.state.${r.state}`)}
              </Tag>
            ),
          },
          { title: t('migrate.progress.rows'), render: (_, r) => `${r.rows.toLocaleString()}${r.estimate !== null ? ` / ${r.estimate.toLocaleString()}` : ''}` },
          { title: t('migrate.progress.skipped'), render: (_, r) => r.skipped || '' },
          { title: t('migrate.progress.time'), render: (_, r) => `${(r.elapsed_ms / 1000).toFixed(1)} s` },
          {
            title: t('migrate.progress.verify'),
            render: (_, r) =>
              r.error && r.state !== 'done' ? (
                <span className="err">{r.error}</span>
              ) : !r.verify ? null : r.verify.target_rows === r.verify.expected_rows && !r.verify.mismatches.length ? (
                <Tag color="success">{t('migrate.progress.verifyOk')}</Tag>
              ) : (
                <span className="err">
                  {t('migrate.progress.verifyBad', {
                    expected: r.verify.expected_rows,
                    target: r.verify.target_rows,
                    mismatches: r.verify.mismatches.length,
                  })}
                </span>
              ),
          },
        ]}
      />
    </div>
  )
}

/** 没有完成的迁移：继续时把任务 id 交给 `onResume`。 */
export function UnfinishedList({ onResume }: { onResume(task: string): void }) {
  const api = useApi()
  const { t } = useTranslation()
  const { message, modal } = App.useApp()
  const qc = useQueryClient()
  const { data = [] } = useQuery({ queryKey: ['migrations'], queryFn: () => api.unfinishedMigrations() })
  if (!data.length) return null
  const refresh = () => void qc.invalidateQueries({ queryKey: ['migrations'] })
  return (
    <Alert
      type="info"
      showIcon
      message={t('migrate.unfinished.title')}
      description={data.map((m) => (
        <div key={m.id} className="row" style={{ gap: 8 }}>
          <span className="grow">
            {t('migrate.unfinished.item', {
              source: m.source,
              target: m.target,
              phase: t(`migrate.phase.${m.phase}`),
              done: m.tables_done,
              total: m.tables_total,
            })}
          </span>
          {m.running ? (
            <Tag color="processing">{t('migrate.unfinished.running')}</Tag>
          ) : (
            <Space>
              <Button
                size="small"
                type="primary"
                onClick={async () => {
                  try {
                    onResume(await withVault(api, () => api.resumeMigration(m.id)))
                    refresh()
                  } catch (e) {
                    message.error((e as Error).message)
                  }
                }}
              >
                {t('migrate.unfinished.resume')}
              </Button>
              <Button
                size="small"
                danger
                onClick={() =>
                  modal.confirm({
                    title: t('migrate.unfinished.discard'),
                    content: t('migrate.unfinished.discardConfirm'),
                    okButtonProps: { danger: true },
                    onOk: async () => {
                      await api.discardMigration(m.id).catch((e) => message.error((e as Error).message))
                      refresh()
                    },
                  })
                }
              >
                {t('migrate.unfinished.discard')}
              </Button>
            </Space>
          )}
        </div>
      ))}
    />
  )
}
```

- [ ] **Step 8: 修改 `web/src/locales/en/migrate.json`**

```diff
--- a/web/src/locales/en/migrate.json
+++ b/web/src/locales/en/migrate.json
@@ -50,7 +50,9 @@
     "all": "Set all to",
     "skip": "Skip",
     "recreate": "Drop and recreate",
-    "missing_only": "Only add missing indexes and constraints"
+    "missing_only": "Only add missing indexes and constraints",
+    "append": "Append data",
+    "replace": "Clear and import"
   },
   "summary": "{{tables}} tables, {{sequences}} sequences, {{constraints}} indexes and constraints",
   "saveScript": "Save Script…",
@@ -68,5 +70,57 @@
     "info": "Info",
     "lossy": "Lossy",
     "unsupported": "Unsupported"
-  }
+  },
+  "mode": {
+    "label": "Mode",
+    "all": "Structure and data",
+    "structure": "Structure only",
+    "data": "Data only"
+  },
+  "data": {
+    "batch": "Batch rows",
+    "parallel": "Tables in parallel",
+    "onError": "On error",
+    "stop": "Stop",
+    "skip": "Skip and log",
+    "sample": "Rows to sample",
+    "sampleHint": "0 compares row counts only"
+  },
+  "filter": "Filter (source SQL)",
+  "structureOnly": "Structure only",
+  "sourceHint": "Avoid writing to the source while migrating",
+  "phase": {
+    "structure": "Creating tables",
+    "data": "Copying data",
+    "constraints": "Creating indexes and constraints",
+    "sequences": "Adjusting sequences",
+    "verify": "Verifying",
+    "done": "Done"
+  },
+  "state": {
+    "pending": "Pending",
+    "copying": "Copying",
+    "done": "Done",
+    "failed": "Failed",
+    "skipped": "No data"
+  },
+  "progress": {
+    "table": "Table",
+    "state": "State",
+    "rows": "Copied / estimate",
+    "skipped": "Skipped",
+    "time": "Time",
+    "verify": "Verification",
+    "verifyOk": "Matched",
+    "verifyBad": "{{expected}} rows expected, {{target}} in target, {{mismatches}} differing values"
+  },
+  "unfinished": {
+    "title": "Unfinished migrations",
+    "item": "{{source}} → {{target}} · {{phase}} · {{done}}/{{total}} tables",
+    "resume": "Continue",
+    "discard": "Discard",
+    "discardConfirm": "Discard this migration? Its record is deleted; data already copied to the target stays.",
+    "running": "Running"
+  },
+  "unfinishedDone": "The migration did not finish; continue it from \"Unfinished migrations\" above"
 }
```

- [ ] **Step 9: 修改 `web/src/locales/zh-CN/migrate.json`**

```diff
--- a/web/src/locales/zh-CN/migrate.json
+++ b/web/src/locales/zh-CN/migrate.json
@@ -50,7 +50,9 @@
     "all": "全部设为",
     "skip": "跳过",
     "recreate": "删除重建",
-    "missing_only": "只建缺的索引和约束"
+    "missing_only": "只建缺的索引和约束",
+    "append": "追加数据",
+    "replace": "清空后导入"
   },
   "summary": "建 {{tables}} 张表、{{sequences}} 个序列、{{constraints}} 个索引和约束",
   "saveScript": "另存脚本…",
@@ -68,5 +70,57 @@
     "info": "说明",
     "lossy": "有损",
     "unsupported": "不支持"
-  }
+  },
+  "mode": {
+    "label": "模式",
+    "all": "结构和数据",
+    "structure": "只迁结构",
+    "data": "只迁数据"
+  },
+  "data": {
+    "batch": "每批行数",
+    "parallel": "并行表数",
+    "onError": "出错时",
+    "stop": "停止",
+    "skip": "跳过并记录",
+    "sample": "抽样比对行数",
+    "sampleHint": "0 为只比对行数"
+  },
+  "filter": "过滤条件（源库写法）",
+  "structureOnly": "只迁结构",
+  "sourceHint": "迁移期间源库最好不要写入",
+  "phase": {
+    "structure": "建表",
+    "data": "迁数据",
+    "constraints": "建索引和约束",
+    "sequences": "调整序列和自增",
+    "verify": "校验",
+    "done": "完成"
+  },
+  "state": {
+    "pending": "等待",
+    "copying": "迁移中",
+    "done": "完成",
+    "failed": "失败",
+    "skipped": "不迁数据"
+  },
+  "progress": {
+    "table": "表",
+    "state": "状态",
+    "rows": "已迁 / 估计",
+    "skipped": "跳过",
+    "time": "用时",
+    "verify": "校验",
+    "verifyOk": "一致",
+    "verifyBad": "应有 {{expected}} 行，目标 {{target}} 行，{{mismatches}} 处值不同"
+  },
+  "unfinished": {
+    "title": "未完成的迁移",
+    "item": "{{source}} → {{target}} · {{phase}} · {{done}}/{{total}} 张表",
+    "resume": "继续",
+    "discard": "放弃",
+    "discardConfirm": "放弃这个迁移？迁移记录将删除，目标库中已迁的内容不会删除。",
+    "running": "执行中"
+  },
+  "unfinishedDone": "迁移没有全部完成，可在上方\"未完成的迁移\"中继续"
 }
```

- [ ] **Step 10: 修改 `web/src/shared/api.ts`**

```diff
--- a/web/src/shared/api.ts
+++ b/web/src/shared/api.ts
@@ -430,7 +430,18 @@ export interface MigrationEnd {
 }
 
 export type NameCase = 'keep' | 'lower' | 'upper'
-export type ExistingAction = 'skip' | 'recreate' | 'missing_only'
+export type ExistingAction = 'skip' | 'recreate' | 'missing_only' | 'append' | 'replace'
+/** 迁移的模式（M5b）：结构和数据、只迁结构、只迁数据。 */
+export type MigrationMode = 'all' | 'structure' | 'data'
+
+/** 迁数据的选项（M5b）。 */
+export interface DataOptions {
+  batch_size: number
+  parallel: number
+  on_error: 'stop' | 'skip'
+  /** 每张表抽样比对的行数，0 为不抽样。 */
+  sample: number
+}
 
 /** 一列的覆盖：`default` 为空串表示不要默认值；`accept` 确认把不认识的类型按文本迁移。 */
 export interface ColumnOverride {
@@ -448,6 +459,10 @@ export interface TableOverride {
   columns?: Record<string, ColumnOverride>
   /** 源检查约束名 → 目标表达式，空串表示不迁。 */
   checks?: Record<string, string>
+  /** 迁数据时的过滤条件（源库写法）。 */
+  filter?: string
+  /** 只迁结构不迁数据。 */
+  structure_only?: boolean
 }
 
 export interface MigrationSettings {
@@ -463,10 +478,52 @@ export interface MigrationPlan {
   tables: string[] | null
   sequences: string[] | null
   target: MigrationEnd
+  mode: MigrationMode
   settings: MigrationSettings
+  data: DataOptions
   existing: Record<string, ExistingAction>
 }
 
+export type MigrationPhase = 'structure' | 'data' | 'constraints' | 'sequences' | 'verify' | 'done'
+
+export interface MigrationVerify {
+  source_rows: number
+  target_rows: number
+  expected_rows: number
+  sampled: number
+  mismatches: { key: string; column: string; source: string; target: string }[]
+}
+
+/** 任务状态中的迁移进度：每张要迁数据的表。 */
+export interface MigrationProgress {
+  record: string
+  phase: MigrationPhase
+  tables: {
+    source: string
+    target: string
+    state: 'pending' | 'copying' | 'done' | 'failed' | 'skipped'
+    rows: number
+    skipped: number
+    estimate: number | null
+    elapsed_ms: number
+    error: string | null
+    verify: MigrationVerify | null
+  }[]
+}
+
+/** 没有完成的迁移。 */
+export interface MigrationSummary {
+  id: string
+  created_at: number
+  updated_at: number
+  source: string
+  target: string
+  phase: MigrationPhase
+  tables_done: number
+  tables_total: number
+  running: boolean
+}
+
 export type NoteLevel = 'info' | 'lossy' | 'unsupported'
 
 export interface MigrationNote {
@@ -543,6 +600,8 @@ export interface TaskStatus {
   remaining: string | null
   /** 反向生成：生成并打开的模型的编号。 */
   model: string | null
+  /** 迁移（M5b）：当前步骤和每张表的进度。 */
+  migration: MigrationProgress | null
 }
 
 /** 表设计器（M3）：与后端结构模型一致的完整表结构。 */
@@ -1122,6 +1181,13 @@ export function createApi(token: string, fetchFn: FetchFn = (i, o) => fetch(i, o
     previewMigration: (plan: MigrationPlan) => request<MigrationPreview>('POST', '/migrations/preview', plan),
     /** 开始迁移，返回任务 id。 */
     startMigration: async (plan: MigrationPlan) => (await request<{ id: string }>('POST', '/migrations', plan)).id,
+    /** 没有完成的迁移。 */
+    unfinishedMigrations: () => request<MigrationSummary[]>('GET', '/migrations'),
+    /** 继续一个没有完成的迁移，返回任务 id。 */
+    resumeMigration: async (id: string) =>
+      (await request<{ id: string }>('POST', `/migrations/${encodeURIComponent(id)}/resume`)).id,
+    /** 放弃：删除迁移记录（不动目标库）。 */
+    discardMigration: (id: string) => request<null>('DELETE', `/migrations/${encodeURIComponent(id)}`),
     syncPreview: (req: SyncRequest) => request<SyncPreview>('POST', '/compare/preview', req),
     /** 同步：目标是库时返回任务 id；目标是模型时已经写完，返回 null。 */
     sync: async (req: SyncRequest) => (await request<{ task: string | null }>('POST', '/compare/sync', req)).task,
```

- [ ] **Step 11: 检查**

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

Expected: 全部通过（前端新增 2 个测试）。

- [ ] **Step 12: 浏览器实测**

浏览器实测（设计文档第 6 节）：用 `dblens-server --listen 127.0.0.1:<端口>` 启动浏览器模式。先在一个 MySQL 系的测试库建一张 1000 行的表和一张引用它的子表。
1. 打开一个 M5a 时保存的迁移标签：页面正常，模式为"结构和数据"、数据选项为默认值。
2. 每批行数设为 100、抽样 50，读取结构后开始迁移：进度表按表显示状态、已迁 / 估计、用时，完成后校验"一致"。
3. 只迁数据、父表选"清空后导入"：被子表的外键挡住时这张表标"失败"并显示错误；标签顶部出现"未完成的迁移"。
4. "继续"：重新执行，进度表显示结果；"放弃"并确认：列表消失。最后清理两边的测试表。

- [ ] **Step 13: 提交**

```bash
git add web/src/app/shell.css web/src/features/editor/tabs.test.ts web/src/features/editor/tabs.ts web/src/features/migrate/MigrateView.tsx web/src/features/migrate/plan.test.ts web/src/features/migrate/plan.ts web/src/features/migrate/progress.tsx web/src/locales/en/migrate.json web/src/locales/zh-CN/migrate.json web/src/shared/api.ts
git commit -m "feat(web): 数据迁移的选项、进度与未完成的迁移" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 5: 文档

- `docs/notes/m5b-data-migration.md`（新）：结构、与设计文档不一致的地方、验证。
- `docs/notes/m0-4b-api.md`：数据迁移的接口。
- 总设计文档：M5b 完成，M5 全部完成。

**Files:**
- Modify: `docs/notes/m0-4b-api.md`
- Create: `docs/notes/m5b-data-migration.md`
- Modify: `docs/superpowers/specs/2026-10-22-dblens-design.md`

- [ ] **Step 1: 修改 `docs/notes/m0-4b-api.md`**

```diff
--- a/docs/notes/m0-4b-api.md
+++ b/docs/notes/m0-4b-api.md
@@ -297,6 +297,19 @@
   `notes` 的 `level` 为 `info` / `lossy` / `unsupported`，有 `unsupported` 或同名表没有选择处理方式时 `problems` 不为空，不能开始。
 - 任务 `kind` 为 `migrate`：逐条执行，失败的语句记入 `failures` 后继续；建表失败的表，依赖它的语句跳过。
 
+数据迁移（M5b 加入）：
+
+| 方法 | 路径 | 说明 |
+|---|---|---|
+| GET | `/migrations` | 没有完成的迁移：`[{id, created_at, updated_at, source, target, phase, tables_done, tables_total, running}]` |
+| POST | `/migrations/{id}/resume` | 继续，201 和 `{id}`（任务 id）；已完成、正在执行时 400，记录不存在时 404 |
+| DELETE | `/migrations/{id}` | 放弃：删除迁移记录（不动目标库），204 |
+
+- 方案加 `mode`（`all` / `structure` / `data`）、`data: {batch_size, parallel, on_error: "stop" | "skip", sample}`；表的覆盖加 `filter`（源库写法）、`structure_only`；
+  `existing` 加 `append`、`replace`。
+- 任务状态加 `migration: {record, phase, tables: [{source, target, state, rows, skipped, estimate, elapsed_ms, error, verify}]}`，
+  `phase` 为 `structure` / `data` / `constraints` / `sequences` / `verify` / `done`，`verify` 为 `{source_rows, target_rows, expected_rows, sampled, mismatches}`。
+
 ## WebSocket：`GET /api/sessions/{id}/ws?token=…&lang=…`
 
 `lang` 可选，决定事件中文字的语言（见"语言"一节）。
```

- [ ] **Step 2: 新建 `docs/notes/m5b-data-migration.md`**

````markdown
# M5b 数据迁移

依据：设计文档 `docs/superpowers/specs/2026-10-28-m5b-data-migration-design.md`。

## 结构

```text
crates/dblens-dialect/src/migrate/data.rs   TableRead（增量读取、计数、抽样、按主键取一行；PG 的精确小数参数写成 $n::text::numeric）、
                                            keyset_params、达梦的 IDENTITY_INSERT、自增的调整、序列的下一个值和推进
crates/dblens-dialect/src/migrate/script.rs ExistingAction 加 Append、Replace（不动结构）、copies_data
crates/dblens-dialect/src/migrate/mod.rs    TableOverride 加 filter、structure_only
crates/dblens-service/src/migrate/
  mod.rs     MigrationPlan 加 mode、data；开始前检查（只迁数据时表必须已有、数据选项的范围）；建迁移记录、开始、继续、未完成的列表、放弃；
             作为任务执行（错误和报告用开始时的界面语言）
  record.rs  迁移记录（方案快照、脚本、每张表的搬运参数和进度、跳过的行、调整、提示、不迁移的对象），存在 数据目录/migrations/<id>.json
  run.rs     依次执行各步：建表语句 → 迁数据（按表并行）→ 建约束语句 → 调整序列和自增 → 校验；任务状态中的进度（TaskStatus::migration）
  copy.rs    一张表的搬运：流式读、转换、分批写，停止或跳过（逐行重试），续传
  values.rs  值转换成目标列的类型；校验时的规整
  report.rs  迁移报告
crates/dblens-api/src/migrations.rs  GET /migrations、POST /migrations/{id}/resume、DELETE /migrations/{id}
web/src/features/migrate/progress.tsx  按表的进度、未完成的迁移
```

## 与设计文档不一致的地方

- 自增的调整：MySQL 系写入显式值时自动推进计数器，达梦在 `IDENTITY_INSERT` 下也会，都不执行语句；PG 系用
  `setval(pg_get_serial_sequence(…), MAX + 1, false)`（标识列和 serial 通用）；Oracle 用 `START WITH LIMIT VALUE`，不先查最大值。
- 序列的推进：Oracle 一律删掉后按原定义以新起点重建（不区分版本），与达梦相同。
- 达梦的 `IDENTITY_INSERT` 只对有自增列的表打开（没有自增列的表上打开会报错）。
- 达梦序列的不缓存是 `CACHE 0`，迁到 PG 时去掉（PG 至少为 1），属于 M5a 的转换，一并修正。
- 校验按目标的规则把空串、空二进制视为 NULL（Oracle 及兼容 Oracle 的模式会这样存），否则逐值比对误报。
- 续传位置的主键值、抽样时按主键取目标行的参数，在 PG 上是精确小数时写成 `$n::text::numeric`（驱动不能直接绑定 NUMERIC 参数，与导入相同）。
- 迁移记录先放在系统临时目录的 `dblens-migrations`，`Service::open` 时改到数据目录的 `migrations`（测试各自指定目录）。
- 映射提示在开始时按界面语言写成文字存进记录，报告用记录中的文字。
- 迁移标签保存的方案在恢复时补上后来加的项（模式、数据选项），旧方案打开不会出错。

## 验证

- 后端：fmt、clippy 通过；全部测试在 `DBLENS_LANG=en` 下通过（775 个）。
  - 方言层：增量读取（单列、复合主键、过滤条件、PG 的精确小数参数）、计数、抽样、按主键取行、自增和序列的语句；追加、清空不动结构。
  - 服务层：迁移记录的读写和未完成的列表、值转换、校验的规整（含 Oracle 的空串）、语句执行（建表失败跳过依赖、成功的记下、保存记录）、开始前检查。
  - 接口：未完成的列表、继续和放弃不存在的记录。
- 实库（串行，约 7.5 分钟）：52 个——44 条路线迁结构和数据（1000 行的表、各种类型的表、推进过的序列；行数和抽样逐值比对一致，
  目标上插入新行自增不冲突，序列接着源序列往下走），4 条续传（迁了几批后取消、继续后无重复无遗漏），4 条跳过（超长的值被跳过并记进报告，清空后导入结果相同）。
- 前端：tsc、Vitest（数据选项、同名表可选的处理方式、旧方案补默认值）、打包通过。
- 浏览器：TiDB → openGauss 迁 1000 行（按表的进度、校验"一致"）；清空父表被外键挡住时表标为失败、显示错误；未完成的迁移列出后继续、放弃。
- 未验证：桌面程序的"迁移报告"保存对话框；大数据量（百万行以上）的耗时。
````

- [ ] **Step 3: 修改 `docs/superpowers/specs/2026-10-22-dblens-design.md`**

```diff
--- a/docs/superpowers/specs/2026-10-22-dblens-design.md
+++ b/docs/superpowers/specs/2026-10-22-dblens-design.md
@@ -335,7 +335,8 @@ trait Policy: Send + Sync {
 - M4b 已完成（设计 `docs/superpowers/specs/2026-10-28-m4b-er-diagram-design.md`，详细计划 `docs/superpowers/plans/2026-10-28-m4b-er-diagram.md`，说明 `docs/notes/m4b-er-diagram.md`）。
 - M4c 已完成（设计 `docs/superpowers/specs/2026-10-28-m4c-document-export-design.md`，详细计划 `docs/superpowers/plans/2026-10-28-m4c-document-export.md`，说明 `docs/notes/m4c-document-export.md`）。M4 全部完成。
 - 2026-10-28 M5 拆成两份（用户确认）：M5a 结构迁移；M5b 数据迁移（分批迁数据、断点续传、行数和抽样校验，插在 M5a 的建表和建约束之间）。
-- M5a 已完成（设计 `docs/superpowers/specs/2026-10-28-m5a-structure-migration-design.md`，详细计划 `docs/superpowers/plans/2026-10-28-m5a-structure-migration.md`，说明 `docs/notes/m5a-structure-migration.md`）。M5b 尚未开始。
+- M5a 已完成（设计 `docs/superpowers/specs/2026-10-28-m5a-structure-migration-design.md`，详细计划 `docs/superpowers/plans/2026-10-28-m5a-structure-migration.md`，说明 `docs/notes/m5a-structure-migration.md`）。
+- M5b 已完成（设计 `docs/superpowers/specs/2026-10-28-m5b-data-migration-design.md`，详细计划 `docs/superpowers/plans/2026-10-29-m5b-data-migration.md`，说明 `docs/notes/m5b-data-migration.md`）。M5 全部完成。
 
 ## 12. 风险与待验证项
 
```

- [ ] **Step 4: 检查**

检查文档中的路径和接口与代码一致；按执行时的实际结果更新"验证"一节（测试数、实库与浏览器实测的内容）。

Expected: 无。

- [ ] **Step 5: 提交**

```bash
git add docs/notes/m0-4b-api.md docs/notes/m5b-data-migration.md docs/superpowers/specs/2026-10-22-dblens-design.md
git commit -m "docs: M5b 数据迁移的说明、接口和进度" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```
