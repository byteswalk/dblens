# M0-3a 方言核心 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 新建 `dblens-dialect` crate，实现方言层中不涉及元数据读取的纯逻辑部分：
- 兼容库变体识别；
- 标识符引用与占位符；
- SQL 语句拆分；
- 字面量与分页包装；
- 原生类型与逻辑类型互转；
- 由表模型生成 SELECT/INSERT/UPDATE/DELETE。

**Architecture:**
- `dblens-dialect` 不做任何 IO，也不依赖 tokio。
- 方言差异全部按"家族"（`Family`：MySQL / PostgreSQL / Oracle / 达梦）区分，写成纯函数；兼容库变体（`Variant`）只在个别地方覆盖。
- 对外暴露一个轻量的 `Dialect` 结构体（只含一个 `Variant`），把各模块的纯函数包装成方法。

设计文档写的是 `Dialect` "接口"，这里有意改用"枚举 + 结构体"：四类方言的差异用 `match` 表达最直接，不需要 trait 对象。M0-3b 的元数据读取只需在 `Dialect` 上继续添加方法。

**Tech Stack:** Rust 1.99（edition 2024）、chrono、serde；只用到工作区已有的依赖。

**Spec:** `docs/superpowers/specs/2026-10-22-dblens-design.md`，第 6.2 节（方言接口的 2、3、4 项）、第 6.3 节（兼容库）、第 7.3 节（生成 SQL 菜单），以及第 9 节。总览见 `docs/superpowers/plans/2026-10-22-m0-overview.md`（M0-3 拆成 3a 和 3b）。

## Global Constraints

- 沿用 M0-1 和 M0-2 的约束：
  - 仓库根目录 `E:\VibeCoding\16数据库管理工具\dblens\`；
  - Rust 1.99.0，edition 2024；依赖版本在工作区统一声明；
  - 错误一律转换为 `dblens_core::Error`，不允许在库代码里 `unwrap`；
  - 注释和用户可见文案用中文；
  - 提交时附加 `Co-Authored-By` 行，写明实际完成工作的模型。
- `dblens-dialect` **不做任何 IO**：不连数据库、不读文件、不依赖 tokio 和任何驱动 crate。只依赖 `dblens-core`，以及 `dblens-driver` 里的两个纯类型 `DriverId` 和 `ServerInfo`。
- 计划中的代码已在临时工作区按最终状态完整编译并测试过：35 个测试全部通过，clippy 没有警告。但排版不完全符合 rustfmt，所以**每次提交前先运行 `cargo fmt --all`**。
- 全部用单元测试验证，不需要数据库。完成标准：`cargo fmt --all --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace` 全部通过。
- **生成的 SQL 要拿去执行**。凡是会拼进 SQL 的标识符和字面量，必须经过本 crate 的引用和转义函数；本 crate 不得把任何调用方传入的文本原样拼进 SQL。
- **语句拆分的偏移量用字节**。每条结果都必须满足 `&sql[start..end] == text`。编辑器需要的 UTF-16 偏移由 M1 自行换算。

---

## File Structure

```
crates/dblens-dialect/
├── Cargo.toml
└── src/
    ├── lib.rs        模块导出、Dialect 结构体（Task 5 补全方法）
    ├── variant.rs    Family、Variant、变体识别
    ├── ident.rs      标识符引用（必须加引号 / 必要时才加）、占位符、保留字表
    ├── split.rs      SQL 语句拆分
    ├── literal.rs    值 → SQL 字面量、分页包装
    ├── types.rs      原生类型解析，原生类型与逻辑类型互转
    └── dml.rs        由表模型生成 SELECT/INSERT/UPDATE/DELETE
docs/superpowers/plans/2026-10-22-m0-overview.md   （Task 5 更新 M0-3a 状态）
```

---

### Task 1: crate 骨架、变体识别、标识符与占位符

**Files:**
- Create: `crates/dblens-dialect/Cargo.toml`, `src/lib.rs`, `src/variant.rs`, `src/ident.rs`

**Interfaces:**
- Consumes: `dblens_driver::{DriverId, ServerInfo}`
- Produces:
  - `Family { MySql, Postgres, Oracle, Dameng }`（Copy、可序列化）
  - `Variant { MySql, MariaDb, TiDb, OceanBase, Postgres, Kingbase, OpenGauss, GaussDb, HighGo, Oracle, Dameng }`，以及 `Variant::family(self) -> Family`、`Variant::display_name(self) -> &'static str`、`Variant::detect(DriverId, &ServerInfo) -> Variant`、`Variant::base(DriverId) -> Variant`
  - `ident::quote_ident(Family, &str) -> String`（总是加引号并转义）
  - `ident::ident(Family, &str) -> String`（只在必要时加引号）
  - `ident::placeholder(Family, usize) -> String`（参数序号从 1 开始）

- [ ] **Step 1: 创建 crate 并写失败的测试**

`crates/dblens-dialect/Cargo.toml`：
```toml
[package]
name = "dblens-dialect"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
publish.workspace = true

[dependencies]
chrono.workspace = true
dblens-core.workspace = true
dblens-driver.workspace = true
serde.workspace = true
serde_json.workspace = true
uuid.workspace = true
```

在根目录 `Cargo.toml` 的 `[workspace.dependencies]` 中，`dblens-driver` 一行之后追加：
```toml
dblens-dialect = { path = "crates/dblens-dialect" }
```

`crates/dblens-dialect/src/lib.rs`（本任务的版本，后续任务逐步添加模块）：
```rust
//! 方言层：不做任何 IO，只负责各数据库的语法差异——标识符与字面量、语句拆分、类型映射、SQL 生成。

pub mod ident;
mod variant;

pub use variant::{Family, Variant};
```

`crates/dblens-dialect/src/variant.rs` 末尾：
```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn info(product: &str, version: &str) -> ServerInfo {
        ServerInfo { product: product.into(), version: version.into() }
    }

    #[test]
    fn detects_mysql_family_variants() {
        assert_eq!(Variant::detect(DriverId::Mysql, &info("MySQL", "8.4.3")), Variant::MySql);
        assert_eq!(Variant::detect(DriverId::Mysql, &info("MySQL", "11.4.4-MariaDB-ubu2404")), Variant::MariaDb);
        assert_eq!(Variant::detect(DriverId::Mysql, &info("MySQL", "8.0.11-TiDB-v7.5.1")), Variant::TiDb);
        assert_eq!(Variant::detect(DriverId::Mysql, &info("MySQL", "5.7.25-OceanBase_CE-v4.2.1.2")), Variant::OceanBase);
    }

    #[test]
    fn detects_postgres_family_variants() {
        let pg = "PostgreSQL 17.2 (Debian 17.2-1.pgdg120+1) on x86_64-pc-linux-gnu";
        assert_eq!(Variant::detect(DriverId::Postgres, &info("PostgreSQL", pg)), Variant::Postgres);
        let kb = "KingbaseES V008R006C008B0014 on x86_64-pc-linux-gnu, compiled by gcc";
        assert_eq!(Variant::detect(DriverId::Postgres, &info("PostgreSQL", kb)), Variant::Kingbase);
        let og = "(openGauss 5.0.1 build 33b035fd) compiled at 2023-12-15 on x86_64-unknown-linux-gnu";
        assert_eq!(Variant::detect(DriverId::Postgres, &info("PostgreSQL", og)), Variant::OpenGauss);
        let gs = "gaussdb (GaussDB Kernel 505.1.0 build 2c0ba3b1) compiled at 2024-03-01";
        assert_eq!(Variant::detect(DriverId::Postgres, &info("PostgreSQL", gs)), Variant::GaussDb);
        let hg = "HighGo Security Enterprise Edition Release 4.5.8 on x86_64";
        assert_eq!(Variant::detect(DriverId::Postgres, &info("PostgreSQL", hg)), Variant::HighGo);
    }

    #[test]
    fn oracle_and_dameng_and_families() {
        let ora = "Oracle AI Database 26ai Free Release 23.26.0.0.0 - Develop, Learn, and Run for Free";
        assert_eq!(Variant::detect(DriverId::Oracle, &info("Oracle", ora)), Variant::Oracle);
        assert_eq!(Variant::detect(DriverId::Dameng, &info("DM", "DM Database Server 64 V8")), Variant::Dameng);
        assert_eq!(Variant::TiDb.family(), Family::MySql);
        assert_eq!(Variant::Kingbase.family(), Family::Postgres);
        assert_eq!(Variant::Dameng.family(), Family::Dameng);
        assert_eq!(Variant::base(DriverId::Postgres), Variant::Postgres);
        assert_eq!(Variant::Kingbase.display_name(), "人大金仓 KingbaseES");
    }
}
```

`crates/dblens-dialect/src/ident.rs` 末尾：
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quote_ident_escapes() {
        assert_eq!(quote_ident(Family::MySql, "a`b"), "`a``b`");
        assert_eq!(quote_ident(Family::Postgres, "a\"b"), "\"a\"\"b\"");
        assert_eq!(quote_ident(Family::Oracle, "NAME"), "\"NAME\"");
    }

    #[test]
    fn ident_quotes_only_when_needed() {
        assert_eq!(ident(Family::MySql, "user_name"), "user_name");
        assert_eq!(ident(Family::MySql, "UserName"), "UserName");
        assert_eq!(ident(Family::MySql, "order"), "`order`");
        assert_eq!(ident(Family::MySql, "数洞"), "`数洞`");
        assert_eq!(ident(Family::MySql, "a b"), "`a b`");
        assert_eq!(ident(Family::MySql, "1abc"), "`1abc`");
        assert_eq!(ident(Family::Postgres, "name"), "name");
        assert_eq!(ident(Family::Postgres, "Name"), "\"Name\"");
        assert_eq!(ident(Family::Postgres, "user"), "\"user\"");
        assert_eq!(ident(Family::Oracle, "NAME"), "NAME");
        assert_eq!(ident(Family::Oracle, "name"), "\"name\"");
        assert_eq!(ident(Family::Oracle, "LEVEL"), "\"LEVEL\"");
        assert_eq!(ident(Family::Dameng, "T_USER$1"), "T_USER$1");
        assert_eq!(ident(Family::Dameng, ""), "\"\"");
    }

    #[test]
    fn placeholders() {
        assert_eq!(placeholder(Family::MySql, 3), "?");
        assert_eq!(placeholder(Family::Postgres, 3), "$3");
        assert_eq!(placeholder(Family::Oracle, 3), ":3");
        assert_eq!(placeholder(Family::Dameng, 3), "?");
    }
}
```

- [ ] **Step 2: 运行测试，确认失败**

Run: `cargo test -p dblens-dialect`
Expected: 编译失败，提示找不到 `Variant`、`quote_ident` 等。

- [ ] **Step 3: 实现 `variant.rs` 和 `ident.rs`**

`variant.rs`（测试模块之前）：
```rust
//! 方言家族与具体产品变体的识别。

use dblens_driver::{DriverId, ServerInfo};
use serde::{Deserialize, Serialize};

/// 方言家族：语法规则基本相同的一组数据库。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Family {
    MySql,
    Postgres,
    Oracle,
    Dameng,
}

/// 具体的数据库产品。兼容库复用所属家族的规则，只在个别地方覆盖。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Variant {
    MySql,
    MariaDb,
    TiDb,
    /// OceanBase 的 MySQL 模式。
    OceanBase,
    Postgres,
    Kingbase,
    OpenGauss,
    GaussDb,
    HighGo,
    Oracle,
    Dameng,
}

impl Variant {
    pub fn family(self) -> Family {
        match self {
            Variant::MySql | Variant::MariaDb | Variant::TiDb | Variant::OceanBase => Family::MySql,
            Variant::Postgres | Variant::Kingbase | Variant::OpenGauss | Variant::GaussDb | Variant::HighGo => {
                Family::Postgres
            }
            Variant::Oracle => Family::Oracle,
            Variant::Dameng => Family::Dameng,
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Variant::MySql => "MySQL",
            Variant::MariaDb => "MariaDB",
            Variant::TiDb => "TiDB",
            Variant::OceanBase => "OceanBase（MySQL 模式）",
            Variant::Postgres => "PostgreSQL",
            Variant::Kingbase => "人大金仓 KingbaseES",
            Variant::OpenGauss => "openGauss",
            Variant::GaussDb => "GaussDB",
            Variant::HighGo => "瀚高 HighGo",
            Variant::Oracle => "Oracle",
            Variant::Dameng => "达梦 DM",
        }
    }

    /// 没有服务器信息时使用的基础变体。
    pub fn base(driver: DriverId) -> Variant {
        match driver {
            DriverId::Mysql => Variant::MySql,
            DriverId::Postgres => Variant::Postgres,
            DriverId::Oracle => Variant::Oracle,
            DriverId::Dameng => Variant::Dameng,
        }
    }

    /// 根据驱动和服务器返回的版本字符串识别具体产品，识别不出时退回基础变体。
    /// 注意：PolarDB-MySQL、GoldenDB 等的 `VERSION()` 与 MySQL 无法区分，会识别为 MySQL。
    pub fn detect(driver: DriverId, info: &ServerInfo) -> Variant {
        let v = info.version.to_ascii_lowercase();
        match driver {
            DriverId::Mysql if v.contains("mariadb") => Variant::MariaDb,
            DriverId::Mysql if v.contains("tidb") => Variant::TiDb,
            DriverId::Mysql if v.contains("oceanbase") => Variant::OceanBase,
            DriverId::Postgres if v.contains("kingbase") => Variant::Kingbase,
            DriverId::Postgres if v.contains("opengauss") => Variant::OpenGauss,
            DriverId::Postgres if v.contains("gaussdb") => Variant::GaussDb,
            DriverId::Postgres if v.contains("highgo") => Variant::HighGo,
            _ => Variant::base(driver),
        }
    }
}
```

`ident.rs`（测试模块之前）：
```rust
//! 标识符引用与参数占位符。

use crate::Family;

/// 总是加引号，并转义标识符中的引号字符。
pub fn quote_ident(family: Family, name: &str) -> String {
    match family {
        Family::MySql => format!("`{}`", name.replace('`', "``")),
        Family::Postgres | Family::Oracle | Family::Dameng => format!("\"{}\"", name.replace('"', "\"\"")),
    }
}

/// 只在必要时加引号：含特殊字符、以数字开头、是保留字，
/// 或者大小写与数据库的默认折叠方向不一致（PG 折叠为小写，Oracle/达梦折叠为大写）。
pub fn ident(family: Family, name: &str) -> String {
    if needs_quote(family, name) { quote_ident(family, name) } else { name.to_string() }
}

/// 参数占位符，`index` 从 1 开始。
pub fn placeholder(family: Family, index: usize) -> String {
    match family {
        Family::Postgres => format!("${index}"),
        Family::Oracle => format!(":{index}"),
        Family::MySql | Family::Dameng => "?".to_string(),
    }
}

fn needs_quote(family: Family, name: &str) -> bool {
    let Some(first) = name.chars().next() else { return true };
    if !(first.is_ascii_alphabetic() || first == '_') {
        return true;
    }
    let dollar_ok = family != Family::Postgres;
    if !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || (dollar_ok && c == '$')) {
        return true;
    }
    let case_ok = match family {
        Family::MySql => true,
        Family::Postgres => !name.chars().any(|c| c.is_ascii_uppercase()),
        Family::Oracle | Family::Dameng => !name.chars().any(|c| c.is_ascii_lowercase()),
    };
    !case_ok || RESERVED.contains(&name.to_ascii_uppercase().as_str())
}

/// 四类数据库常见保留字的并集。多加引号不会出错，所以宁多勿少。
const RESERVED: &[&str] = &[
    "ADD", "ALL", "ALTER", "AND", "ANY", "AS", "ASC", "BETWEEN", "BY", "CASE", "CHECK", "COLUMN", "CONSTRAINT",
    "CREATE", "CROSS", "CURRENT", "DATE", "DEFAULT", "DELETE", "DESC", "DISTINCT", "DROP", "ELSE", "END",
    "EXISTS", "FALSE", "FOR", "FOREIGN", "FROM", "FULL", "GRANT", "GROUP", "HAVING", "IN", "INDEX", "INNER",
    "INSERT", "INTERVAL", "INTO", "IS", "JOIN", "KEY", "LEFT", "LEVEL", "LIKE", "LIMIT", "NOT", "NULL", "NUMBER",
    "OF", "OFFSET", "ON", "OR", "ORDER", "OUTER", "PRIMARY", "REFERENCES", "RIGHT", "ROW", "ROWID", "ROWNUM",
    "ROWS", "SELECT", "SESSION", "SET", "SIZE", "TABLE", "THEN", "TO", "TRUE", "UNION", "UNIQUE", "UPDATE",
    "USER", "USING", "VALUES", "VIEW", "WHEN", "WHERE", "WITH",
];
```

- [ ] **Step 4: 运行测试，确认通过**

Run: `cargo test -p dblens-dialect && cargo clippy -p dblens-dialect --all-targets -- -D warnings`
Expected: 6 个测试全部 PASS，没有警告。

- [ ] **Step 5: 提交**

```bash
git add Cargo.toml Cargo.lock crates/dblens-dialect
git commit -m "feat(dialect): 方言 crate 骨架、兼容库变体识别、标识符与占位符" -m "Co-Authored-By: <实际模型> <noreply@anthropic.com>"
```

---

### Task 2: SQL 语句拆分

**Files:**
- Create: `crates/dblens-dialect/src/split.rs`
- Modify: `crates/dblens-dialect/src/lib.rs`

**Interfaces:**
- Consumes: `Family`
- Produces:
  - `SqlStatement { text: String, start: usize, end: usize }`（`&sql[start..end] == text`，字节偏移，不含结束符，已去掉首尾空白）
  - `split_statements(sql: &str, family: Family) -> Vec<SqlStatement>`

**拆分规则：**
- **字符串**：
  - 单引号 `'…'`，`''` 表示转义的引号。MySQL 中反斜杠也能转义；PG 只在 `E'…'` 里才能用反斜杠转义。
  - 双引号 `"…"`，`""` 表示转义。MySQL 中反斜杠也能转义。
  - 仅 MySQL：反引号 `` `…` ``。
  - 仅 PG：`$标签$…$标签$` 和 `$$…$$`。注意 `$1` 这类参数不算。
  - 仅 Oracle：`q'[…]'`，定界符可以是 `[] {} () <>`，也可以是任意相同字符。
- **注释**：
  - `--` 到行尾。MySQL 要求 `--` 后面跟空白或者正好在行尾。
  - 仅 MySQL：`#` 到行尾。
  - `/* … */`。PG 允许嵌套。`/*!…*/` 和 `/*+…*/` 算作语句内容（MySQL 可执行注释、Oracle 提示）。
- **结束符**：
  - 默认是 `;`。
  - MySQL 行首的 `DELIMITER xx` 会改变结束符，这一行本身不算语句内容。
  - Oracle/达梦中，单独一行的 `/` 结束当前语句。
- **PL/SQL 块**（仅 Oracle/达梦）：语句首个词是 `DECLARE` 或 `BEGIN`，或者是 `CREATE [OR REPLACE] [EDITIONABLE|NONEDITIONABLE] PROCEDURE|FUNCTION|PACKAGE|TRIGGER|TYPE|LIBRARY|CLASS` 时，进入块模式。块模式下 `;` 不拆分，只认单独一行的 `/` 或文本结尾。块的文本保留末尾的 `END;`。
- **其他**：只有空白和注释的片段不输出；语句前面的注释会算进下一条语句的文本里。

- [ ] **Step 1: 写失败的测试**

`crates/dblens-dialect/src/split.rs`（测试模块；实现在 Step 3 写在它前面）：
```rust
#[cfg(test)]
mod tests {
    use super::*;

    /// 拆分并校验偏移量与文本一致，返回各语句文本。
    fn texts(sql: &str, family: Family) -> Vec<String> {
        let out = split_statements(sql, family);
        for s in &out {
            assert_eq!(&sql[s.start..s.end], s.text, "偏移量与文本不一致：{s:?}");
        }
        out.into_iter().map(|s| s.text).collect()
    }

    #[test]
    fn basic_and_trailing() {
        assert_eq!(texts("SELECT 1; SELECT 2", Family::MySql), ["SELECT 1", "SELECT 2"]);
        assert_eq!(texts("  SELECT 1 ;\n\n", Family::Postgres), ["SELECT 1"]);
        assert!(texts("", Family::MySql).is_empty());
        assert!(texts(";;; \n ;", Family::Oracle).is_empty());
        assert!(texts("-- 只有注释\n/* 还有 */", Family::Postgres).is_empty());
    }

    #[test]
    fn offsets_are_byte_positions() {
        let sql = "SELECT '数洞;'; SELECT 2";
        let out = split_statements(sql, Family::MySql);
        assert_eq!(out.len(), 2);
        assert_eq!((out[0].start, out[0].end), (0, 16));
        assert_eq!(out[1].text, "SELECT 2");
        assert_eq!(out[1].start, 18);
    }

    #[test]
    fn mysql_quotes_and_escapes() {
        assert_eq!(
            texts("SELECT 'a;b'; SELECT \"c;d\"; SELECT `e;f`", Family::MySql),
            ["SELECT 'a;b'", "SELECT \"c;d\"", "SELECT `e;f`"]
        );
        assert_eq!(
            texts("SELECT 'it''s; ok', 'a\\';b'; SELECT 2", Family::MySql),
            ["SELECT 'it''s; ok', 'a\\';b'", "SELECT 2"]
        );
    }

    #[test]
    fn mysql_comments() {
        assert_eq!(
            texts("-- x;\nSELECT 1; # y;\nSELECT 2 /* ; */;", Family::MySql),
            ["-- x;\nSELECT 1", "# y;\nSELECT 2 /* ; */"]
        );
        // MySQL 中 `--` 后不跟空白就不是注释
        assert_eq!(texts("SELECT 5--2; SELECT 1", Family::MySql), ["SELECT 5--2", "SELECT 1"]);
    }

    #[test]
    fn mysql_delimiter() {
        let sql = "DELIMITER $$\nCREATE PROCEDURE p() BEGIN SELECT 1; SELECT 2; END$$\nDELIMITER ;\nSELECT 3;";
        assert_eq!(texts(sql, Family::MySql), ["CREATE PROCEDURE p() BEGIN SELECT 1; SELECT 2; END", "SELECT 3"]);
        // DELIMITER 前未结束的语句也会被结束
        assert_eq!(texts("SELECT 0\ndelimiter //\nSELECT 1//", Family::MySql), ["SELECT 0", "SELECT 1"]);
    }

    #[test]
    fn postgres_dollar_quotes_and_strings() {
        let f = "CREATE FUNCTION f() RETURNS int AS $$ BEGIN RETURN 1; END; $$ LANGUAGE plpgsql; SELECT 1";
        assert_eq!(texts(f, Family::Postgres).len(), 2);
        assert_eq!(texts("DO $body$ BEGIN PERFORM 1; END $body$; SELECT 2", Family::Postgres).len(), 2);
        assert_eq!(texts("SELECT $1; SELECT 2", Family::Postgres), ["SELECT $1", "SELECT 2"]);
        assert_eq!(texts("SELECT E'a\\';b'; SELECT 2", Family::Postgres), ["SELECT E'a\\';b'", "SELECT 2"]);
        // 普通字符串里反斜杠不转义
        assert_eq!(texts("SELECT 'a\\'; SELECT 2", Family::Postgres), ["SELECT 'a\\'", "SELECT 2"]);
        assert_eq!(texts("SELECT \"a;b\" FROM t; SELECT 2", Family::Postgres).len(), 2);
    }

    #[test]
    fn postgres_nested_comments_and_dash_comments() {
        assert_eq!(
            texts("/* a /* b */ ; */ SELECT 1; SELECT 2", Family::Postgres),
            ["/* a /* b */ ; */ SELECT 1", "SELECT 2"]
        );
        // PG 中 `--` 总是注释，会吃掉行内的分号
        assert_eq!(texts("SELECT 5--2;\nSELECT 1", Family::Postgres), ["SELECT 5--2;\nSELECT 1"]);
    }

    #[test]
    fn oracle_plsql_blocks_and_slash() {
        let sql = "BEGIN\n  NULL;\n  NULL;\nEND;\n/\nSELECT 1 FROM DUAL;";
        assert_eq!(texts(sql, Family::Oracle), ["BEGIN\n  NULL;\n  NULL;\nEND;", "SELECT 1 FROM DUAL"]);
        let proc = "CREATE OR REPLACE PROCEDURE p IS\nBEGIN\n  NULL;\nEND;\n/\n";
        assert_eq!(texts(proc, Family::Oracle), ["CREATE OR REPLACE PROCEDURE p IS\nBEGIN\n  NULL;\nEND;"]);
        let ed = "create or replace editionable function f return number is begin return 1; end;\n/";
        assert_eq!(texts(ed, Family::Oracle).len(), 1);
        assert_eq!(
            texts("CREATE TABLE t (a INT);\nINSERT INTO t VALUES (1);", Family::Oracle),
            ["CREATE TABLE t (a INT)", "INSERT INTO t VALUES (1)"]
        );
        assert_eq!(
            texts("SELECT 1 FROM DUAL\n/\nSELECT 2 FROM DUAL\n  /  \n", Family::Oracle),
            ["SELECT 1 FROM DUAL", "SELECT 2 FROM DUAL"]
        );
        assert_eq!(texts("SELECT 4 / 2 FROM DUAL;", Family::Oracle), ["SELECT 4 / 2 FROM DUAL"]);
    }

    #[test]
    fn oracle_q_quote() {
        assert_eq!(
            texts("SELECT q'[a;b]' FROM DUAL; SELECT Q'#c;d#' FROM DUAL", Family::Oracle),
            ["SELECT q'[a;b]' FROM DUAL", "SELECT Q'#c;d#' FROM DUAL"]
        );
    }

    #[test]
    fn dameng_uses_plsql_rules() {
        let sql = "DECLARE\n  x INT;\nBEGIN\n  x := 1;\nEND;\n/\nSELECT 1;";
        assert_eq!(texts(sql, Family::Dameng), ["DECLARE\n  x INT;\nBEGIN\n  x := 1;\nEND;", "SELECT 1"]);
    }

    #[test]
    fn unterminated_string_runs_to_end() {
        assert_eq!(texts("SELECT 'abc; SELECT 2", Family::MySql), ["SELECT 'abc; SELECT 2"]);
    }
}
```

`lib.rs` 改为：
```rust
//! 方言层：不做任何 IO，只负责各数据库的语法差异——标识符与字面量、语句拆分、类型映射、SQL 生成。

pub mod ident;
mod split;
mod variant;

pub use split::{SqlStatement, split_statements};
pub use variant::{Family, Variant};
```

- [ ] **Step 2: 运行测试，确认失败**

Run: `cargo test -p dblens-dialect split`
Expected: 编译失败，提示找不到 `split_statements`。

- [ ] **Step 3: 实现 `split.rs`**

在测试模块之前写入：
```rust
//! SQL 语句拆分：处理字符串、注释、PG 的 $$ 引号、MySQL 的 DELIMITER 命令，
//! 以及 Oracle/达梦的 PL/SQL 块和 `/` 结束符。保留每条语句在原文中的字节范围，便于定位出错位置。

use crate::Family;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SqlStatement {
    /// 语句文本：不含结束符，已去掉首尾空白。
    pub text: String,
    /// 在原文中的字节范围，满足 `&sql[start..end] == text`。
    pub start: usize,
    pub end: usize,
}

/// 按家族的语法规则拆分多条语句。
pub fn split_statements(sql: &str, family: Family) -> Vec<SqlStatement> {
    let mut s = Splitter {
        sql,
        b: sql.as_bytes(),
        family,
        i: 0,
        stmt_start: 0,
        has_code: false,
        block: false,
        delimiter: b";".to_vec(),
        out: Vec::new(),
    };
    s.run();
    s.out
}

struct Splitter<'a> {
    sql: &'a str,
    b: &'a [u8],
    family: Family,
    i: usize,
    stmt_start: usize,
    /// 当前语句是否已出现注释以外的内容。
    has_code: bool,
    /// 是否处于 PL/SQL 块中（块内 `;` 不拆分）。
    block: bool,
    delimiter: Vec<u8>,
    out: Vec<SqlStatement>,
}

fn is_ident_byte(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_' || c >= 0x80
}

impl Splitter<'_> {
    fn plsql(&self) -> bool {
        matches!(self.family, Family::Oracle | Family::Dameng)
    }

    fn run(&mut self) {
        while self.i < self.b.len() {
            if (self.i == 0 || self.b[self.i - 1] == b'\n') && self.line_command() {
                continue;
            }
            if self.skip_comment() || self.skip_string() {
                continue;
            }
            if !self.block && self.b[self.i..].starts_with(&self.delimiter) {
                let end = self.i;
                self.i += self.delimiter.len();
                self.finish(end);
                continue;
            }
            if !self.b[self.i].is_ascii_whitespace() {
                self.mark_code();
            }
            self.i += 1;
        }
        self.finish(self.b.len());
    }

    /// 遇到第一段非注释内容时记下，并判断是否是 PL/SQL 块的开头。
    fn mark_code(&mut self) {
        if !self.has_code {
            self.has_code = true;
            self.block = self.plsql() && starts_plsql(&self.b[self.i..]);
        }
    }

    /// 结束当前语句（范围到 `end` 为止），并从 `self.i` 开始新语句。
    fn finish(&mut self, end: usize) {
        if self.has_code {
            let mut s = self.stmt_start;
            let mut e = end;
            while s < e && self.b[s].is_ascii_whitespace() {
                s += 1;
            }
            while e > s && self.b[e - 1].is_ascii_whitespace() {
                e -= 1;
            }
            if s < e {
                self.out.push(SqlStatement { text: self.sql[s..e].to_string(), start: s, end: e });
            }
        }
        self.stmt_start = self.i;
        self.has_code = false;
        self.block = false;
    }

    /// 行首命令：MySQL 的 `DELIMITER xx`，Oracle/达梦单独一行的 `/`。处理了就返回 true。
    fn line_command(&mut self) -> bool {
        let mut j = self.i;
        while j < self.b.len() && matches!(self.b[j], b' ' | b'\t') {
            j += 1;
        }
        let line_end = self.b[j..].iter().position(|&c| c == b'\n').map_or(self.b.len(), |p| j + p);
        let line = self.b[j..line_end].trim_ascii_end();
        let next_line = (line_end + 1).min(self.b.len());
        match self.family {
            Family::MySql
                if line.len() > 9 && line[..9].eq_ignore_ascii_case(b"delimiter") && line[9].is_ascii_whitespace() =>
            {
                let new = line[9..].trim_ascii();
                if new.is_empty() {
                    return false;
                }
                self.finish(self.i);
                self.delimiter = new.to_vec();
                self.i = next_line;
                self.stmt_start = next_line;
                true
            }
            Family::Oracle | Family::Dameng if line == b"/" => {
                self.finish(self.i);
                self.i = next_line;
                self.stmt_start = next_line;
                true
            }
            _ => false,
        }
    }

    fn skip_comment(&mut self) -> bool {
        let b = self.b;
        let i = self.i;
        let next = b.get(i + 1).copied();
        let line_end = |from: usize| b[from..].iter().position(|&c| c == b'\n').map_or(b.len(), |p| from + p);
        if b[i] == b'-' && next == Some(b'-') {
            // MySQL 要求 `--` 后跟空白或行尾才是注释
            if self.family == Family::MySql && !matches!(b.get(i + 2), None | Some(b' ' | b'\t' | b'\r' | b'\n')) {
                return false;
            }
            self.i = line_end(i);
            return true;
        }
        if b[i] == b'#' && self.family == Family::MySql {
            self.i = line_end(i);
            return true;
        }
        if b[i] == b'/' && next == Some(b'*') {
            // MySQL 的 /*! … */ 与 Oracle 的 /*+ … */ 提示属于语句内容
            if matches!(b.get(i + 2), Some(b'!' | b'+')) {
                self.mark_code();
            }
            let nested = self.family == Family::Postgres;
            let mut depth = 1;
            let mut j = i + 2;
            while j < b.len() && depth > 0 {
                if b[j] == b'*' && b.get(j + 1) == Some(&b'/') {
                    depth -= 1;
                    j += 2;
                } else if nested && b[j] == b'/' && b.get(j + 1) == Some(&b'*') {
                    depth += 1;
                    j += 2;
                } else {
                    j += 1;
                }
            }
            self.i = j.min(b.len());
            return true;
        }
        false
    }

    fn skip_string(&mut self) -> bool {
        let b = self.b;
        let i = self.i;
        let c = b[i];
        let prev_is_ident = i > 0 && is_ident_byte(b[i - 1]);
        let end = if c == b'\'' {
            let pg_escape = self.family == Family::Postgres
                && i > 0
                && matches!(b[i - 1], b'E' | b'e')
                && (i < 2 || !is_ident_byte(b[i - 2]));
            Some(scan_quoted(b, i + 1, b'\'', self.family == Family::MySql || pg_escape))
        } else if c == b'"' {
            Some(scan_quoted(b, i + 1, b'"', self.family == Family::MySql))
        } else if c == b'`' && self.family == Family::MySql {
            Some(scan_quoted(b, i + 1, b'`', false))
        } else if matches!(c, b'q' | b'Q') && self.family == Family::Oracle && b.get(i + 1) == Some(&b'\'') && !prev_is_ident
        {
            Some(scan_q_quote(b, i + 2))
        } else if c == b'$' && self.family == Family::Postgres && !prev_is_ident {
            dollar_tag_len(b, i).map(|len| scan_dollar(b, i, len))
        } else {
            None
        };
        match end {
            Some(e) => {
                self.mark_code();
                self.i = e;
                true
            }
            None => false,
        }
    }
}

/// 扫描引号字符串，返回结束引号之后的位置；没有结束引号时返回文本末尾。
fn scan_quoted(b: &[u8], mut j: usize, quote: u8, backslash: bool) -> usize {
    while j < b.len() {
        let c = b[j];
        if backslash && c == b'\\' {
            j += 2;
            continue;
        }
        if c == quote {
            if b.get(j + 1) == Some(&quote) {
                j += 2;
                continue;
            }
            return j + 1;
        }
        j += 1;
    }
    b.len()
}

/// Oracle 的 q'X…X' 字符串，`start` 指向定界符 X。
fn scan_q_quote(b: &[u8], start: usize) -> usize {
    let Some(&open) = b.get(start) else { return b.len() };
    let close = match open {
        b'[' => b']',
        b'{' => b'}',
        b'(' => b')',
        b'<' => b'>',
        c => c,
    };
    let mut j = start + 1;
    while j + 1 < b.len() {
        if b[j] == close && b[j + 1] == b'\'' {
            return j + 2;
        }
        j += 1;
    }
    b.len()
}

/// PG 的 `$标签$` 或 `$$`：返回标签长度（含两端的 `$`），不是标签时返回 None。
fn dollar_tag_len(b: &[u8], i: usize) -> Option<usize> {
    let mut j = i + 1;
    if b.get(j) == Some(&b'$') {
        return Some(2);
    }
    if !b.get(j).is_some_and(|c| c.is_ascii_alphabetic() || *c == b'_') {
        return None;
    }
    while j < b.len() && (b[j].is_ascii_alphanumeric() || b[j] == b'_') {
        j += 1;
    }
    (b.get(j) == Some(&b'$')).then_some(j - i + 1)
}

fn scan_dollar(b: &[u8], i: usize, tag_len: usize) -> usize {
    let tag = &b[i..i + tag_len];
    let body = i + tag_len;
    b[body..].windows(tag_len).position(|w| w == tag).map_or(b.len(), |p| body + p + tag_len)
}

/// 语句开头是否是 PL/SQL 块（Oracle/达梦）。
fn starts_plsql(s: &[u8]) -> bool {
    let words = leading_words(s, 6);
    let w = |k: usize| words.get(k).map(String::as_str).unwrap_or("");
    match w(0) {
        "DECLARE" | "BEGIN" => true,
        "CREATE" => {
            let mut k = 1;
            if w(1) == "OR" && w(2) == "REPLACE" {
                k = 3;
            }
            if matches!(w(k), "EDITIONABLE" | "NONEDITIONABLE" | "EDITIONING") {
                k += 1;
            }
            matches!(w(k), "PROCEDURE" | "FUNCTION" | "PACKAGE" | "TRIGGER" | "TYPE" | "LIBRARY" | "CLASS")
        }
        _ => false,
    }
}

fn leading_words(s: &[u8], n: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut j = 0;
    while out.len() < n {
        while j < s.len() && s[j].is_ascii_whitespace() {
            j += 1;
        }
        let start = j;
        while j < s.len() && (s[j].is_ascii_alphanumeric() || s[j] == b'_') {
            j += 1;
        }
        if start == j {
            break;
        }
        out.push(String::from_utf8_lossy(&s[start..j]).to_ascii_uppercase());
    }
    out
}
```

- [ ] **Step 4: 运行测试，确认通过**

Run: `cargo test -p dblens-dialect split && cargo clippy -p dblens-dialect --all-targets -- -D warnings`
Expected: 11 个拆分测试全部 PASS，没有警告。

测试失败时，先判断是实现有 bug，还是测试的期望写错了。只有在确认测试期望违背了本任务列出的拆分规则时，才可以修改期望，并要在报告里逐条说明。

- [ ] **Step 5: 提交**

```bash
git add crates/dblens-dialect
git commit -m "feat(dialect): SQL 语句拆分（字符串、注释、DELIMITER、\$\$、PL/SQL 块与 /）" -m "Co-Authored-By: <实际模型> <noreply@anthropic.com>"
```

---

### Task 3: 字面量与分页包装

**Files:**
- Create: `crates/dblens-dialect/src/literal.rs`
- Modify: `crates/dblens-dialect/src/lib.rs`

**Interfaces:**
- Consumes: `Family`、`dblens_core::{Error, Result, Value}`
- Produces:
  - `literal::literal(Family, &Value) -> Result<String>`
  - `literal::paginate(Family, sql: &str, offset: u64, limit: u64) -> String`

- [ ] **Step 1: 写失败的测试**

`crates/dblens-dialect/src/literal.rs`（测试模块）：
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{FixedOffset, NaiveDate, NaiveTime, TimeZone};

    fn lit(f: Family, v: Value) -> String {
        literal(f, &v).unwrap()
    }

    #[test]
    fn scalars() {
        assert_eq!(lit(Family::MySql, Value::Null), "NULL");
        assert_eq!(lit(Family::Postgres, Value::Bool(true)), "TRUE");
        assert_eq!(lit(Family::Oracle, Value::Bool(true)), "1");
        assert_eq!(lit(Family::Dameng, Value::Bool(false)), "0");
        assert_eq!(lit(Family::MySql, Value::Int(-7)), "-7");
        assert_eq!(lit(Family::MySql, Value::UInt(u64::MAX)), "18446744073709551615");
        assert_eq!(lit(Family::MySql, Value::Float(1.5)), "1.5");
        assert_eq!(lit(Family::Oracle, Value::Decimal("-123.4500".into())), "-123.4500");
        assert!(literal(Family::MySql, &Value::Float(f64::NAN)).is_err());
        // 不是数字的 Decimal 文本不能原样拼进 SQL
        assert!(literal(Family::MySql, &Value::Decimal("1; DROP TABLE t".into())).is_err());
        assert_eq!(lit(Family::Postgres, Value::Decimal("NaN".into())), "'NaN'::numeric");
        assert!(literal(Family::Oracle, &Value::Decimal("NaN".into())).is_err());
    }

    #[test]
    fn strings_are_escaped() {
        assert_eq!(lit(Family::Postgres, Value::String("it's".into())), "'it''s'");
        assert_eq!(lit(Family::MySql, Value::String("a\\b'c".into())), "'a\\\\b''c'");
        assert_eq!(lit(Family::Oracle, Value::String("a\\b".into())), "'a\\b'");
        assert_eq!(lit(Family::Dameng, Value::String("数洞".into())), "'数洞'");
        assert_eq!(lit(Family::MySql, Value::Json(serde_json::json!({"a": "x'y"}))), "'{\"a\":\"x''y\"}'");
    }

    #[test]
    fn bytes() {
        let v = Value::Bytes(vec![0x01, 0xab]);
        assert_eq!(lit(Family::MySql, v.clone()), "X'01AB'");
        assert_eq!(lit(Family::Postgres, v.clone()), "'\\x01AB'::bytea");
        assert_eq!(lit(Family::Oracle, v.clone()), "HEXTORAW('01AB')");
        assert_eq!(lit(Family::Dameng, v), "0x01AB");
        assert_eq!(lit(Family::Dameng, Value::Bytes(vec![])), "''");
    }

    #[test]
    fn temporal() {
        let d = NaiveDate::from_ymd_opt(2024, 2, 29).unwrap();
        assert_eq!(lit(Family::MySql, Value::Date(d)), "DATE '2024-02-29'");
        let t = NaiveTime::from_hms_micro_opt(13, 45, 30, 123456).unwrap();
        assert_eq!(lit(Family::Postgres, Value::Time(t)), "TIME '13:45:30.123456'");
        assert_eq!(lit(Family::Oracle, Value::Time(t)), "'13:45:30.123456'");
        let dt = d.and_hms_opt(1, 2, 3).unwrap();
        assert_eq!(lit(Family::Dameng, Value::DateTime(dt)), "TIMESTAMP '2024-02-29 01:02:03'");
        let tz = FixedOffset::east_opt(8 * 3600).unwrap().with_ymd_and_hms(2024, 2, 29, 13, 45, 30).unwrap();
        assert_eq!(lit(Family::Postgres, Value::DateTimeTz(tz)), "TIMESTAMPTZ '2024-02-29 13:45:30+08:00'");
        assert_eq!(lit(Family::Oracle, Value::DateTimeTz(tz)), "TIMESTAMP '2024-02-29 13:45:30 +08:00'");
        assert_eq!(lit(Family::MySql, Value::DateTimeTz(tz)), "TIMESTAMP '2024-02-29 13:45:30'");
    }

    #[test]
    fn unsupported_values() {
        let lob = Value::Lob(dblens_core::LobPreview { binary: false, preview: "a".into(), total_len: 9, truncated: true });
        assert!(literal(Family::MySql, &lob).is_err());
        assert!(literal(Family::MySql, &Value::Other { type_name: "x".into(), text: "y".into() }).is_err());
    }

    #[test]
    fn pagination() {
        assert_eq!(
            paginate(Family::MySql, "SELECT * FROM t;", 20, 10),
            "SELECT * FROM (\nSELECT * FROM t\n) dblens_page LIMIT 10 OFFSET 20"
        );
        assert_eq!(
            paginate(Family::Oracle, "SELECT * FROM t -- 注释", 0, 50),
            "SELECT * FROM (\nSELECT * FROM t -- 注释\n) OFFSET 0 ROWS FETCH NEXT 50 ROWS ONLY"
        );
        assert!(paginate(Family::Dameng, "SELECT 1", 0, 1).ends_with("dblens_page LIMIT 1 OFFSET 0"));
    }
}
```

`lib.rs` 在 `pub mod ident;` 之后增加一行 `pub mod literal;`。

- [ ] **Step 2: 运行测试，确认失败**

Run: `cargo test -p dblens-dialect literal`
Expected: 编译失败，提示找不到 `literal`、`paginate`。

- [ ] **Step 3: 实现 `literal.rs`**

在测试模块之前写入：
```rust
//! 值 → SQL 字面量（用于"生成 SQL"和脚本导出，执行用户 SQL 时一律走参数绑定），以及分页包装。

use dblens_core::{Error, Result, Value};

use crate::Family;

/// 把值格式化为该方言的 SQL 字面量。字符串一律转义；无法安全表示的值返回错误。
pub fn literal(family: Family, v: &Value) -> Result<String> {
    Ok(match v {
        Value::Null => "NULL".to_string(),
        Value::Bool(b) => match (family, b) {
            (Family::MySql | Family::Postgres, true) => "TRUE".to_string(),
            (Family::MySql | Family::Postgres, false) => "FALSE".to_string(),
            (_, true) => "1".to_string(),
            (_, false) => "0".to_string(),
        },
        Value::Int(i) => i.to_string(),
        Value::UInt(u) => u.to_string(),
        Value::Float(f) => {
            if !f.is_finite() {
                return Err(Error::internal(format!("无法生成非有限浮点数的 SQL 字面量：{f}")));
            }
            f.to_string()
        }
        Value::Decimal(s) => decimal_literal(family, s)?,
        Value::String(s) | Value::Interval(s) => string_literal(family, s),
        Value::Json(j) => string_literal(family, &j.to_string()),
        Value::Uuid(u) => string_literal(family, &u.to_string()),
        Value::Bytes(b) => bytes_literal(family, b),
        Value::Date(d) => format!("DATE '{}'", d.format("%Y-%m-%d")),
        Value::Time(t) => {
            let text = t.format("%H:%M:%S%.f").to_string();
            // Oracle 没有 TIME 类型，按字符串写出
            if family == Family::Oracle { string_literal(family, &text) } else { format!("TIME '{text}'") }
        }
        Value::DateTime(dt) => format!("TIMESTAMP '{}'", dt.format("%Y-%m-%d %H:%M:%S%.f")),
        Value::DateTimeTz(dt) => match family {
            // MySQL 没有带时区的类型，按该值自身时区的本地时间写出
            Family::MySql => format!("TIMESTAMP '{}'", dt.naive_local().format("%Y-%m-%d %H:%M:%S%.f")),
            Family::Postgres => format!("TIMESTAMPTZ '{}'", dt.format("%Y-%m-%d %H:%M:%S%.f%:z")),
            Family::Oracle | Family::Dameng => format!("TIMESTAMP '{}'", dt.format("%Y-%m-%d %H:%M:%S%.f %:z")),
        },
        Value::Lob(_) | Value::Other { .. } => {
            return Err(Error::internal(format!("该类型的值不能生成 SQL 字面量：{v:?}")));
        }
    })
}

fn decimal_literal(family: Family, s: &str) -> Result<String> {
    let numeric = !s.is_empty()
        && s.bytes().any(|c| c.is_ascii_digit())
        && s.bytes().all(|c| c.is_ascii_digit() || matches!(c, b'+' | b'-' | b'.' | b'e' | b'E'));
    if numeric {
        return Ok(s.to_string());
    }
    if family == Family::Postgres && matches!(s, "NaN" | "Infinity" | "-Infinity") {
        return Ok(format!("'{s}'::numeric"));
    }
    Err(Error::internal(format!("不是有效的数字文本，无法生成字面量：{s}")))
}

fn string_literal(family: Family, s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for c in s.chars() {
        match c {
            '\'' => out.push_str("''"),
            // MySQL 默认把反斜杠当转义符
            '\\' if family == Family::MySql => out.push_str("\\\\"),
            '\0' if family == Family::MySql => out.push_str("\\0"),
            c => out.push(c),
        }
    }
    out.push('\'');
    out
}

fn bytes_literal(family: Family, b: &[u8]) -> String {
    let hex: String = b.iter().map(|x| format!("{x:02X}")).collect();
    match family {
        Family::MySql => format!("X'{hex}'"),
        Family::Postgres => format!("'\\x{hex}'::bytea"),
        Family::Oracle => format!("HEXTORAW('{hex}')"),
        // 达梦的 0x 字面量不能为空，空值写成空字符串由数据库隐式转换
        Family::Dameng if b.is_empty() => "''".to_string(),
        Family::Dameng => format!("0x{hex}"),
    }
}

/// 把查询包装成分页查询。原查询放在单独的行上，避免它末尾的行注释吞掉右括号。
/// Oracle 使用 12c 起支持的 OFFSET … FETCH 语法。
pub fn paginate(family: Family, sql: &str, offset: u64, limit: u64) -> String {
    let inner = sql.trim().trim_end_matches(';').trim_end();
    match family {
        Family::Oracle => format!("SELECT * FROM (\n{inner}\n) OFFSET {offset} ROWS FETCH NEXT {limit} ROWS ONLY"),
        Family::MySql | Family::Postgres | Family::Dameng => {
            format!("SELECT * FROM (\n{inner}\n) dblens_page LIMIT {limit} OFFSET {offset}")
        }
    }
}
```

- [ ] **Step 4: 运行测试，确认通过**

Run: `cargo test -p dblens-dialect literal && cargo clippy -p dblens-dialect --all-targets -- -D warnings`
Expected: 6 个测试全部 PASS，没有警告。

- [ ] **Step 5: 提交**

```bash
git add crates/dblens-dialect
git commit -m "feat(dialect): SQL 字面量生成与分页包装" -m "Co-Authored-By: <实际模型> <noreply@anthropic.com>"
```

---

### Task 4: 原生类型与逻辑类型互转

**Files:**
- Create: `crates/dblens-dialect/src/types.rs`
- Modify: `crates/dblens-dialect/src/lib.rs`

**Interfaces:**
- Consumes: `Family`、`dblens_core::LogicalType`
- Produces:
  - `types::ParsedType { name: String, args: Vec<String>, suffix: String, unsigned: bool }`
  - `types::parse_type(&str) -> ParsedType`
  - `types::logical_type(Family, native: &str) -> LogicalType`
  - `types::native_type(Family, &LogicalType) -> String`

**映射约定**：

| 场景 | 映射方式 |
|---|---|
| 原生 → 逻辑，识别不了的类型 | 一律映射为 `LogicalType::Other` |
| 逻辑 → 原生，目标库没有对应类型 | 映射为最安全的可存储类型（通常是文本或大对象），并在代码注释里说明 |
| MySQL `TINYINT(1)` | 映射为 `Bool`，与 JDBC 的惯例一致 |
| Oracle `DATE` | 映射为 `Timestamp { tz: false, precision: Some(0) }`，因为它带时分秒 |
| MySQL `TIMESTAMP` | 映射为 `Timestamp { tz: true }`，因为按会话时区换算 |

- [ ] **Step 1: 写失败的测试**

`crates/dblens-dialect/src/types.rs`（测试模块）：
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use LogicalType as L;

    #[test]
    fn parses_type_text() {
        let t = parse_type("decimal(10, 2) unsigned zerofill");
        assert_eq!((t.name.as_str(), t.args.clone(), t.suffix.as_str(), t.unsigned), ("DECIMAL", vec!["10".to_string(), "2".to_string()], "", true));
        let t = parse_type("timestamp(6) with time zone");
        assert_eq!((t.name.as_str(), t.args.clone(), t.suffix.as_str()), ("TIMESTAMP", vec!["6".to_string()], "WITH TIME ZONE"));
        let t = parse_type("TIMESTAMP WITH LOCAL TIME ZONE");
        assert_eq!((t.name.as_str(), t.suffix.as_str()), ("TIMESTAMP", "WITH LOCAL TIME ZONE"));
        assert_eq!(parse_type("character varying(100)").name, "CHARACTER VARYING");
        assert_eq!(parse_type("VARCHAR2(100 CHAR)").args, vec!["100 CHAR".to_string()]);
        assert_eq!(parse_type("int unsigned").name, "INT");
        assert_eq!(parse_type("INTERVAL DAY(2) TO SECOND(6)").name, "INTERVAL DAY");
        assert_eq!(parse_type("double precision").name, "DOUBLE PRECISION");
    }

    fn check_to_logical(f: Family, cases: &[(&str, L)]) {
        for (native, expected) in cases {
            assert_eq!(&logical_type(f, native), expected, "{f:?} 原生类型 {native}");
        }
    }

    fn check_to_native(f: Family, cases: &[(L, &str)]) {
        for (logical, expected) in cases {
            assert_eq!(native_type(f, logical), *expected, "{f:?} 逻辑类型 {logical:?}");
        }
    }

    #[test]
    fn mysql_native_to_logical() {
        check_to_logical(Family::MySql, &[
            ("tinyint(1)", L::Bool),
            ("tinyint", L::Int { bits: 8 }),
            ("int unsigned", L::UInt { bits: 32 }),
            ("bigint", L::Int { bits: 64 }),
            ("mediumint", L::Int { bits: 24 }),
            ("decimal(10,2)", L::Decimal { precision: Some(10), scale: Some(2) }),
            ("double", L::Float { bits: 64 }),
            ("float", L::Float { bits: 32 }),
            ("varchar(100)", L::VarChar { len: Some(100) }),
            ("char(2)", L::Char { len: Some(2) }),
            ("longtext", L::Text),
            ("varbinary(16)", L::VarBinary { len: Some(16) }),
            ("mediumblob", L::Blob),
            ("datetime(6)", L::Timestamp { tz: false, precision: Some(6) }),
            ("timestamp", L::Timestamp { tz: true, precision: None }),
            ("time(3)", L::Time { precision: Some(3) }),
            ("year", L::Int { bits: 16 }),
            ("json", L::Json),
            ("enum('a','b')", L::Other),
        ]);
    }

    #[test]
    fn mysql_logical_to_native() {
        check_to_native(Family::MySql, &[
            (L::Bool, "TINYINT(1)"),
            (L::UInt { bits: 64 }, "BIGINT UNSIGNED"),
            (L::Decimal { precision: Some(10), scale: Some(2) }, "DECIMAL(10,2)"),
            (L::Decimal { precision: None, scale: None }, "DECIMAL(65,30)"),
            (L::VarChar { len: Some(100) }, "VARCHAR(100)"),
            (L::VarChar { len: None }, "LONGTEXT"),
            (L::Timestamp { tz: false, precision: Some(6) }, "DATETIME(6)"),
            (L::Timestamp { tz: true, precision: None }, "TIMESTAMP"),
            (L::Uuid, "CHAR(36)"),
            (L::Other, "LONGTEXT"),
        ]);
    }

    #[test]
    fn postgres_mappings() {
        check_to_logical(Family::Postgres, &[
            ("integer", L::Int { bits: 32 }),
            ("int8", L::Int { bits: 64 }),
            ("numeric(12,4)", L::Decimal { precision: Some(12), scale: Some(4) }),
            ("numeric", L::Decimal { precision: None, scale: None }),
            ("character varying(50)", L::VarChar { len: Some(50) }),
            ("character(3)", L::Char { len: Some(3) }),
            ("text", L::Text),
            ("bytea", L::Blob),
            ("boolean", L::Bool),
            ("double precision", L::Float { bits: 64 }),
            ("real", L::Float { bits: 32 }),
            ("timestamp(3) without time zone", L::Timestamp { tz: false, precision: Some(3) }),
            ("timestamp with time zone", L::Timestamp { tz: true, precision: None }),
            ("timestamptz", L::Timestamp { tz: true, precision: None }),
            ("interval day to second(3)", L::Interval),
            ("jsonb", L::Json),
            ("uuid", L::Uuid),
            ("point", L::Other),
        ]);
        check_to_native(Family::Postgres, &[
            (L::Int { bits: 8 }, "SMALLINT"),
            (L::UInt { bits: 64 }, "NUMERIC(20)"),
            (L::Float { bits: 64 }, "DOUBLE PRECISION"),
            (L::Blob, "BYTEA"),
            (L::Timestamp { tz: true, precision: Some(6) }, "TIMESTAMP(6) WITH TIME ZONE"),
            (L::Json, "JSONB"),
            (L::VarChar { len: None }, "TEXT"),
        ]);
    }

    #[test]
    fn oracle_mappings() {
        check_to_logical(Family::Oracle, &[
            ("NUMBER(10,0)", L::Decimal { precision: Some(10), scale: Some(0) }),
            ("NUMBER", L::Decimal { precision: None, scale: None }),
            ("NUMBER(*,0)", L::Decimal { precision: None, scale: Some(0) }),
            ("INTEGER", L::Decimal { precision: Some(38), scale: Some(0) }),
            ("BINARY_DOUBLE", L::Float { bits: 64 }),
            ("VARCHAR2(100 CHAR)", L::VarChar { len: Some(100) }),
            ("NVARCHAR2(20)", L::VarChar { len: Some(20) }),
            ("CHAR(1)", L::Char { len: Some(1) }),
            ("CLOB", L::Text),
            ("LONG RAW", L::Blob),
            ("RAW(16)", L::VarBinary { len: Some(16) }),
            ("DATE", L::Timestamp { tz: false, precision: Some(0) }),
            ("TIMESTAMP(6)", L::Timestamp { tz: false, precision: Some(6) }),
            ("TIMESTAMP(6) WITH LOCAL TIME ZONE", L::Timestamp { tz: true, precision: Some(6) }),
            ("INTERVAL YEAR(2) TO MONTH", L::Interval),
            ("BOOLEAN", L::Bool),
            ("XMLTYPE", L::Other),
        ]);
        check_to_native(Family::Oracle, &[
            (L::Bool, "NUMBER(1)"),
            (L::Int { bits: 32 }, "NUMBER(10)"),
            (L::Float { bits: 32 }, "BINARY_FLOAT"),
            (L::VarChar { len: Some(100) }, "VARCHAR2(100 CHAR)"),
            (L::VarChar { len: None }, "CLOB"),
            (L::VarBinary { len: Some(16) }, "RAW(16)"),
            (L::VarBinary { len: Some(4000) }, "BLOB"),
            (L::Date, "DATE"),
            (L::Time { precision: None }, "VARCHAR2(18)"),
            (L::Timestamp { tz: false, precision: None }, "TIMESTAMP"),
            (L::Json, "CLOB"),
            (L::Uuid, "VARCHAR2(36)"),
        ]);
    }

    #[test]
    fn dameng_mappings() {
        check_to_logical(Family::Dameng, &[
            ("INT", L::Int { bits: 32 }),
            ("BIT", L::Bool),
            ("TINYINT", L::Int { bits: 8 }),
            ("DEC(10,4)", L::Decimal { precision: Some(10), scale: Some(4) }),
            ("NUMBER(8)", L::Decimal { precision: Some(8), scale: None }),
            ("DOUBLE", L::Float { bits: 64 }),
            ("VARCHAR(50)", L::VarChar { len: Some(50) }),
            ("VARCHAR2(50)", L::VarChar { len: Some(50) }),
            ("TEXT", L::Text),
            ("IMAGE", L::Blob),
            ("DATETIME(6)", L::Timestamp { tz: false, precision: Some(6) }),
            ("TIMESTAMP(6) WITH TIME ZONE", L::Timestamp { tz: true, precision: Some(6) }),
            ("TIME(0)", L::Time { precision: Some(0) }),
            ("ROWID", L::Other),
        ]);
        check_to_native(Family::Dameng, &[
            (L::Bool, "BIT"),
            (L::Int { bits: 64 }, "BIGINT"),
            (L::UInt { bits: 64 }, "DEC(20,0)"),
            (L::Decimal { precision: Some(10), scale: Some(2) }, "DEC(10,2)"),
            (L::VarChar { len: None }, "TEXT"),
            (L::Timestamp { tz: true, precision: Some(3) }, "TIMESTAMP(3) WITH TIME ZONE"),
            (L::Json, "CLOB"),
        ]);
    }
}
```

`lib.rs` 在 `mod split;` 之后增加一行 `pub mod types;`。

- [ ] **Step 2: 运行测试，确认失败**

Run: `cargo test -p dblens-dialect types`
Expected: 编译失败，提示找不到 `parse_type` 等。

- [ ] **Step 3: 实现 `types.rs`**

在测试模块之前写入：
```rust
//! 原生类型文本与逻辑类型的互相转换。原生类型文本来自元数据（如 `varchar(100)`、`NUMBER(10,2)`、
//! `timestamp(6) with time zone`），逻辑类型用于跨库映射和设计器。

use dblens_core::LogicalType;

use crate::Family;

/// 拆开后的原生类型：名称（大写、单词间单空格）、括号内的参数、名称之后的修饰（如 `WITH TIME ZONE`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedType {
    pub name: String,
    pub args: Vec<String>,
    pub suffix: String,
    pub unsigned: bool,
}

pub fn parse_type(native: &str) -> ParsedType {
    let upper = native.trim().to_ascii_uppercase();
    let (head, args, rest) = match upper.find('(') {
        Some(open) => {
            let close = upper[open..].find(')').map_or(upper.len(), |p| open + p);
            let args = upper[open + 1..close]
                .split(',')
                .map(|a| a.trim().to_string())
                .filter(|a| !a.is_empty())
                .collect();
            (upper[..open].to_string(), args, upper.get(close + 1..).unwrap_or("").to_string())
        }
        None => (upper.clone(), Vec::new(), String::new()),
    };
    let mut unsigned = false;
    let words: Vec<&str> = head
        .split_whitespace()
        .chain(rest.split_whitespace())
        .filter(|w| match *w {
            "UNSIGNED" => {
                unsigned = true;
                false
            }
            "SIGNED" | "ZEROFILL" => false,
            _ => true,
        })
        .collect();
    let split = words.iter().position(|w| matches!(*w, "WITH" | "WITHOUT" | "TO")).unwrap_or(words.len());
    ParsedType { name: words[..split].join(" "), args, suffix: words[split..].join(" "), unsigned }
}

/// 取参数开头的数字（`100 CHAR` → 100，`*` → None）。
fn arg_u32(args: &[String], k: usize) -> Option<u32> {
    let a = args.get(k)?;
    let digits: String = a.chars().take_while(|c| c.is_ascii_digit()).collect();
    digits.parse().ok()
}

fn arg_i32(args: &[String], k: usize) -> Option<i32> {
    args.get(k)?.trim().parse().ok()
}

fn arg_u8(args: &[String], k: usize) -> Option<u8> {
    arg_u32(args, k).and_then(|v| u8::try_from(v).ok())
}

/// 原生类型 → 逻辑类型。不认识的类型返回 `LogicalType::Other`。
pub fn logical_type(family: Family, native: &str) -> LogicalType {
    use LogicalType as L;
    let t = parse_type(native);
    let n = t.name.as_str();
    if n.starts_with("INTERVAL") {
        return L::Interval;
    }
    let len = arg_u32(&t.args, 0);
    let precision = arg_u8(&t.args, 0);
    let tz = t.suffix.starts_with("WITH ") && t.suffix.ends_with("TIME ZONE");
    let int = |bits: u8| if t.unsigned { L::UInt { bits } } else { L::Int { bits } };
    let decimal = || L::Decimal { precision: arg_u32(&t.args, 0), scale: arg_i32(&t.args, 1) };
    match family {
        Family::MySql => match n {
            "TINYINT" if len == Some(1) && !t.unsigned => L::Bool,
            "BOOL" | "BOOLEAN" => L::Bool,
            "TINYINT" => int(8),
            "SMALLINT" => int(16),
            "MEDIUMINT" => int(24),
            "INT" | "INTEGER" => int(32),
            "BIGINT" => int(64),
            "YEAR" => L::Int { bits: 16 },
            "FLOAT" => L::Float { bits: 32 },
            "DOUBLE" | "DOUBLE PRECISION" | "REAL" => L::Float { bits: 64 },
            "DECIMAL" | "NUMERIC" | "DEC" | "FIXED" => decimal(),
            "CHAR" => L::Char { len },
            "VARCHAR" => L::VarChar { len },
            "TINYTEXT" | "TEXT" | "MEDIUMTEXT" | "LONGTEXT" => L::Text,
            "BINARY" => L::Binary { len },
            "VARBINARY" => L::VarBinary { len },
            "TINYBLOB" | "BLOB" | "MEDIUMBLOB" | "LONGBLOB" => L::Blob,
            "DATE" => L::Date,
            "TIME" => L::Time { precision },
            "DATETIME" => L::Timestamp { tz: false, precision },
            // MySQL 的 TIMESTAMP 按会话时区换算，语义上带时区
            "TIMESTAMP" => L::Timestamp { tz: true, precision },
            "JSON" => L::Json,
            _ => L::Other,
        },
        Family::Postgres => match n {
            "BOOLEAN" | "BOOL" => L::Bool,
            "SMALLINT" | "INT2" | "SMALLSERIAL" | "SERIAL2" => L::Int { bits: 16 },
            "INTEGER" | "INT" | "INT4" | "SERIAL" | "SERIAL4" => L::Int { bits: 32 },
            "BIGINT" | "INT8" | "BIGSERIAL" | "SERIAL8" => L::Int { bits: 64 },
            "REAL" | "FLOAT4" => L::Float { bits: 32 },
            "DOUBLE PRECISION" | "FLOAT8" => L::Float { bits: 64 },
            "FLOAT" => L::Float { bits: if len.is_some_and(|p| p <= 24) { 32 } else { 64 } },
            "NUMERIC" | "DECIMAL" => decimal(),
            "CHARACTER" | "CHAR" | "BPCHAR" => L::Char { len },
            "CHARACTER VARYING" | "VARCHAR" => L::VarChar { len },
            "TEXT" | "NAME" | "CITEXT" => L::Text,
            "BYTEA" => L::Blob,
            "DATE" => L::Date,
            "TIME" | "TIMETZ" => L::Time { precision },
            "TIMESTAMP" => L::Timestamp { tz, precision },
            "TIMESTAMPTZ" => L::Timestamp { tz: true, precision },
            "JSON" | "JSONB" => L::Json,
            "UUID" => L::Uuid,
            _ => L::Other,
        },
        Family::Oracle => match n {
            "NUMBER" => decimal(),
            // Oracle 的 INTEGER/INT/SMALLINT 都是 NUMBER(38)
            "INTEGER" | "INT" | "SMALLINT" => L::Decimal { precision: Some(38), scale: Some(0) },
            "FLOAT" => L::Float { bits: 64 },
            "BINARY_FLOAT" => L::Float { bits: 32 },
            "BINARY_DOUBLE" => L::Float { bits: 64 },
            "VARCHAR2" | "NVARCHAR2" | "VARCHAR" => L::VarChar { len },
            "CHAR" | "NCHAR" => L::Char { len },
            "CLOB" | "NCLOB" | "LONG" => L::Text,
            "BLOB" | "LONG RAW" | "BFILE" => L::Blob,
            "RAW" => L::VarBinary { len },
            // Oracle 的 DATE 带时分秒
            "DATE" => L::Timestamp { tz: false, precision: Some(0) },
            "TIMESTAMP" => L::Timestamp { tz, precision },
            "JSON" => L::Json,
            "BOOLEAN" => L::Bool,
            _ => L::Other,
        },
        Family::Dameng => match n {
            "BIT" | "BOOL" | "BOOLEAN" => L::Bool,
            "TINYINT" | "BYTE" => L::Int { bits: 8 },
            "SMALLINT" => L::Int { bits: 16 },
            "INT" | "INTEGER" | "PLS_INTEGER" => L::Int { bits: 32 },
            "BIGINT" => L::Int { bits: 64 },
            "DEC" | "DECIMAL" | "NUMERIC" | "NUMBER" => decimal(),
            "REAL" => L::Float { bits: 32 },
            "FLOAT" | "DOUBLE" | "DOUBLE PRECISION" => L::Float { bits: 64 },
            "CHAR" | "CHARACTER" => L::Char { len },
            "VARCHAR" | "VARCHAR2" => L::VarChar { len },
            "TEXT" | "LONGVARCHAR" | "CLOB" => L::Text,
            "BINARY" => L::Binary { len },
            "VARBINARY" | "RAW" => L::VarBinary { len },
            "BLOB" | "IMAGE" | "LONGVARBINARY" => L::Blob,
            "DATE" => L::Date,
            "TIME" => L::Time { precision },
            "TIMESTAMP" | "DATETIME" => L::Timestamp { tz, precision },
            _ => L::Other,
        },
    }
}

fn with_len(name: &str, len: Option<u32>) -> String {
    match len {
        Some(n) => format!("{name}({n})"),
        None => name.to_string(),
    }
}

fn with_precision(name: &str, p: Option<u8>) -> String {
    with_len(name, p.map(u32::from))
}

fn decimal_text(name: &str, precision: Option<u32>, scale: Option<i32>, default: &str) -> String {
    match (precision, scale) {
        (Some(p), Some(s)) => format!("{name}({p},{s})"),
        (Some(p), None) => format!("{name}({p})"),
        _ => default.to_string(),
    }
}

/// 逻辑类型 → 该方言的原生类型文本（用于建表和跨库迁移）。
pub fn native_type(family: Family, logical: &LogicalType) -> String {
    use LogicalType as L;
    match family {
        Family::MySql => match logical {
            L::Bool => "TINYINT(1)".into(),
            L::Int { bits } | L::UInt { bits } => {
                let base = match bits {
                    0..=8 => "TINYINT",
                    9..=16 => "SMALLINT",
                    17..=24 => "MEDIUMINT",
                    25..=32 => "INT",
                    _ => "BIGINT",
                };
                if matches!(logical, L::UInt { .. }) { format!("{base} UNSIGNED") } else { base.into() }
            }
            L::Float { bits } => if *bits <= 32 { "FLOAT" } else { "DOUBLE" }.into(),
            L::Decimal { precision, scale } => decimal_text("DECIMAL", *precision, *scale, "DECIMAL(65,30)"),
            L::Char { len } => with_len("CHAR", Some(len.unwrap_or(1))),
            L::VarChar { len: Some(n) } => format!("VARCHAR({n})"),
            L::VarChar { len: None } | L::Text | L::Other => "LONGTEXT".into(),
            L::Binary { len } => with_len("BINARY", Some(len.unwrap_or(1))),
            L::VarBinary { len: Some(n) } => format!("VARBINARY({n})"),
            L::VarBinary { len: None } | L::Blob => "LONGBLOB".into(),
            L::Date => "DATE".into(),
            L::Time { precision } => with_precision("TIME", *precision),
            L::Timestamp { tz: false, precision } => with_precision("DATETIME", *precision),
            L::Timestamp { tz: true, precision } => with_precision("TIMESTAMP", *precision),
            // MySQL 没有时间间隔类型，按文本存储
            L::Interval => "VARCHAR(100)".into(),
            L::Json => "JSON".into(),
            L::Uuid => "CHAR(36)".into(),
        },
        Family::Postgres => match logical {
            L::Bool => "BOOLEAN".into(),
            L::Int { bits } => match bits {
                0..=16 => "SMALLINT",
                17..=32 => "INTEGER",
                _ => "BIGINT",
            }
            .into(),
            // PG 没有无符号整数，取能容纳全部取值的更宽类型
            L::UInt { bits } => match bits {
                0..=8 => "SMALLINT",
                9..=16 => "INTEGER",
                17..=32 => "BIGINT",
                _ => "NUMERIC(20)",
            }
            .into(),
            L::Float { bits } => if *bits <= 32 { "REAL" } else { "DOUBLE PRECISION" }.into(),
            L::Decimal { precision, scale } => decimal_text("NUMERIC", *precision, *scale, "NUMERIC"),
            L::Char { len } => with_len("CHAR", Some(len.unwrap_or(1))),
            L::VarChar { len: Some(n) } => format!("VARCHAR({n})"),
            L::VarChar { len: None } | L::Text | L::Other => "TEXT".into(),
            L::Binary { .. } | L::VarBinary { .. } | L::Blob => "BYTEA".into(),
            L::Date => "DATE".into(),
            L::Time { precision } => with_precision("TIME", *precision),
            L::Timestamp { tz: false, precision } => with_precision("TIMESTAMP", *precision),
            L::Timestamp { tz: true, precision } => format!("{} WITH TIME ZONE", with_precision("TIMESTAMP", *precision)),
            L::Interval => "INTERVAL".into(),
            L::Json => "JSONB".into(),
            L::Uuid => "UUID".into(),
        },
        Family::Oracle => match logical {
            // 为兼容 23ai 之前的版本，布尔用 NUMBER(1)
            L::Bool => "NUMBER(1)".into(),
            L::Int { bits } => format!(
                "NUMBER({})",
                match bits {
                    0..=8 => 3,
                    9..=16 => 5,
                    17..=24 => 7,
                    25..=32 => 10,
                    _ => 19,
                }
            ),
            L::UInt { bits } => format!(
                "NUMBER({})",
                match bits {
                    0..=8 => 3,
                    9..=16 => 5,
                    17..=24 => 8,
                    25..=32 => 10,
                    _ => 20,
                }
            ),
            L::Float { bits } => if *bits <= 32 { "BINARY_FLOAT" } else { "BINARY_DOUBLE" }.into(),
            L::Decimal { precision, scale } => decimal_text("NUMBER", *precision, *scale, "NUMBER"),
            L::Char { len } => format!("CHAR({} CHAR)", len.unwrap_or(1)),
            L::VarChar { len: Some(n) } if *n <= 4000 => format!("VARCHAR2({n} CHAR)"),
            L::VarChar { .. } | L::Text | L::Json | L::Other => "CLOB".into(),
            L::Binary { len: Some(n) } | L::VarBinary { len: Some(n) } if *n <= 2000 => format!("RAW({n})"),
            L::Binary { .. } | L::VarBinary { .. } | L::Blob => "BLOB".into(),
            L::Date => "DATE".into(),
            // Oracle 没有 TIME 类型，按 'HH24:MI:SS.FF6' 文本存储
            L::Time { .. } => "VARCHAR2(18)".into(),
            L::Timestamp { tz: false, precision } => with_precision("TIMESTAMP", *precision),
            L::Timestamp { tz: true, precision } => format!("{} WITH TIME ZONE", with_precision("TIMESTAMP", *precision)),
            L::Interval => "INTERVAL DAY(9) TO SECOND(6)".into(),
            L::Uuid => "VARCHAR2(36)".into(),
        },
        Family::Dameng => match logical {
            L::Bool => "BIT".into(),
            L::Int { bits } => match bits {
                0..=8 => "TINYINT",
                9..=16 => "SMALLINT",
                17..=32 => "INT",
                _ => "BIGINT",
            }
            .into(),
            L::UInt { bits } => match bits {
                0..=8 => "SMALLINT",
                9..=16 => "INT",
                17..=32 => "BIGINT",
                _ => "DEC(20,0)",
            }
            .into(),
            L::Float { bits } => if *bits <= 32 { "REAL" } else { "DOUBLE" }.into(),
            L::Decimal { precision, scale } => decimal_text("DEC", *precision, *scale, "DEC"),
            L::Char { len } => with_len("CHAR", Some(len.unwrap_or(1))),
            L::VarChar { len: Some(n) } => format!("VARCHAR({n})"),
            L::VarChar { len: None } | L::Text => "TEXT".into(),
            L::Binary { len } => with_len("BINARY", Some(len.unwrap_or(1))),
            L::VarBinary { len: Some(n) } => format!("VARBINARY({n})"),
            L::VarBinary { len: None } | L::Blob => "BLOB".into(),
            L::Date => "DATE".into(),
            L::Time { precision } => with_precision("TIME", *precision),
            L::Timestamp { tz: false, precision } => with_precision("TIMESTAMP", *precision),
            L::Timestamp { tz: true, precision } => format!("{} WITH TIME ZONE", with_precision("TIMESTAMP", *precision)),
            L::Interval => "INTERVAL DAY TO SECOND".into(),
            L::Json | L::Other => "CLOB".into(),
            L::Uuid => "VARCHAR(36)".into(),
        },
    }
}
```

- [ ] **Step 4: 运行测试，确认通过**

Run: `cargo test -p dblens-dialect types && cargo clippy -p dblens-dialect --all-targets -- -D warnings`
Expected: 6 个测试全部 PASS，没有警告。规则同 Task 2：只有在确认测试期望违背了本任务的映射约定时，才可以修改期望，并要在报告里说明。

- [ ] **Step 5: 提交**

```bash
git add crates/dblens-dialect
git commit -m "feat(dialect): 原生类型与逻辑类型互转（MySQL/PG/Oracle/达梦）" -m "Co-Authored-By: <实际模型> <noreply@anthropic.com>"
```

---

### Task 5: `Dialect` 结构体与 DML 生成

**Files:**
- Create: `crates/dblens-dialect/src/dml.rs`
- Modify: `crates/dblens-dialect/src/lib.rs`、`docs/superpowers/plans/2026-10-22-m0-overview.md`

**Interfaces:**
- Consumes: Task 1–4 的全部函数；`dblens_core::schema::Table`
- Produces：
  - `Dialect`（Copy，包含一个 `Variant`），构造函数为 `new(Variant)`、`detect(DriverId, &ServerInfo)`、`for_driver(DriverId)`；
  - 方法：`variant()`、`family()`、`quote_ident(&str)`、`ident(&str)`、`qualified(schema: Option<&str>, name: &str)`、`placeholder(usize)`、`literal(&Value)`、`paginate(&str, u64, u64)`、`split(&str)`、`logical_type(&str)`、`native_type(&LogicalType)`；
  - SQL 生成：`select_sql(schema, &Table) -> String`、`insert_sql(schema, &Table) -> String`、`update_sql(schema, &Table) -> Option<String>`、`delete_sql(schema, &Table) -> Option<String>`。

**SQL 生成规则**（对应设计文档 7.3 节的"生成 SQL"菜单）：

| 语句 | 规则 |
|---|---|
| SELECT | 每列一行；列有注释时，在行末追加 `-- 注释`，注释中的换行替换为空格；没有列时为 `SELECT *` |
| INSERT | 跳过自增列和生成列 |
| UPDATE | SET 列为除主键、生成列之外的全部列；WHERE 按主键；没有主键或没有可更新的列时返回 None |
| DELETE | WHERE 按主键；没有主键时返回 None |

占位符按出现顺序从 1 开始编号，PG 写成 `$n`，Oracle 写成 `:n`。

- [ ] **Step 1: 写失败的测试**

`crates/dblens-dialect/src/dml.rs`（测试模块）：
```rust
#[cfg(test)]
mod tests {
    use dblens_core::schema::{Column, PrimaryKey, Table};
    use dblens_core::{ColumnType, LogicalType};
    use dblens_driver::{DriverId, ServerInfo};

    use crate::{Dialect, Family, Variant};

    fn col(name: &str, comment: Option<&str>) -> Column {
        Column {
            name: name.into(),
            ordinal: 0,
            data_type: ColumnType { native: "VARCHAR(10)".into(), logical: LogicalType::VarChar { len: Some(10) } },
            nullable: true,
            default: None,
            comment: comment.map(Into::into),
            auto_increment: false,
            generated: None,
        }
    }

    /// person 表：id（自增主键，注释"主键"）、name（注释含换行）、note、full_name（生成列）。
    fn person(upper: bool) -> Table {
        let n = |s: &str| if upper { s.to_ascii_uppercase() } else { s.to_string() };
        let mut id = col(&n("id"), Some("主键"));
        id.auto_increment = true;
        let mut full = col(&n("full_name"), None);
        full.generated = Some("name || note".into());
        Table {
            name: n("person"),
            columns: vec![id, col(&n("name"), Some("姓名\n全称")), col(&n("note"), None), full],
            primary_key: Some(PrimaryKey { name: None, columns: vec![n("id")] }),
            ..Default::default()
        }
    }

    #[test]
    fn select_lists_columns_with_comments() {
        let d = Dialect::new(Variant::MySql);
        assert_eq!(
            d.select_sql(Some("hr"), &person(false)),
            "SELECT\n    id, -- 主键\n    name, -- 姓名 全称\n    note,\n    full_name\nFROM hr.person"
        );
        let empty = Table { name: "t".into(), ..Default::default() };
        assert_eq!(d.select_sql(None, &empty), "SELECT *\nFROM t");
    }

    #[test]
    fn identifiers_are_quoted_when_needed() {
        let pg = Dialect::new(Variant::Postgres);
        assert!(pg.select_sql(Some("HR"), &person(false)).ends_with("FROM \"HR\".person"));
        let ora = Dialect::new(Variant::Oracle);
        assert!(ora.select_sql(Some("HR"), &person(true)).ends_with("FROM HR.PERSON"));
        assert!(ora.select_sql(None, &person(false)).ends_with("FROM \"person\""));
    }

    #[test]
    fn insert_skips_auto_increment_and_generated() {
        assert_eq!(
            Dialect::new(Variant::MySql).insert_sql(Some("hr"), &person(false)),
            "INSERT INTO hr.person (name, note) VALUES (?, ?)"
        );
        assert_eq!(
            Dialect::new(Variant::Postgres).insert_sql(Some("public"), &person(false)),
            "INSERT INTO public.person (name, note) VALUES ($1, $2)"
        );
    }

    #[test]
    fn update_and_delete_use_primary_key() {
        assert_eq!(
            Dialect::new(Variant::Postgres).update_sql(None, &person(false)).unwrap(),
            "UPDATE person SET name = $1, note = $2 WHERE id = $3"
        );
        assert_eq!(
            Dialect::new(Variant::Oracle).update_sql(Some("HR"), &person(true)).unwrap(),
            "UPDATE HR.PERSON SET NAME = :1, NOTE = :2 WHERE ID = :3"
        );
        assert_eq!(
            Dialect::new(Variant::Dameng).delete_sql(Some("HR"), &person(true)).unwrap(),
            "DELETE FROM HR.PERSON WHERE ID = ?"
        );
        let mut no_pk = person(false);
        no_pk.primary_key = None;
        assert!(Dialect::new(Variant::MySql).update_sql(None, &no_pk).is_none());
        assert!(Dialect::new(Variant::MySql).delete_sql(None, &no_pk).is_none());
    }

    #[test]
    fn composite_primary_key() {
        let mut t = person(false);
        t.primary_key = Some(PrimaryKey { name: None, columns: vec!["id".into(), "name".into()] });
        assert_eq!(
            Dialect::new(Variant::MySql).delete_sql(None, &t).unwrap(),
            "DELETE FROM person WHERE id = ? AND name = ?"
        );
        assert_eq!(
            Dialect::new(Variant::MySql).update_sql(None, &t).unwrap(),
            "UPDATE person SET note = ? WHERE id = ? AND name = ?"
        );
    }

    #[test]
    fn dialect_delegates() {
        let d = Dialect::detect(
            DriverId::Postgres,
            &ServerInfo { product: "PostgreSQL".into(), version: "KingbaseES V008R006".into() },
        );
        assert_eq!((d.variant(), d.family()), (Variant::Kingbase, Family::Postgres));
        assert_eq!(Dialect::for_driver(DriverId::Dameng).variant(), Variant::Dameng);
        assert_eq!(d.placeholder(2), "$2");
        assert_eq!(d.split("SELECT 1; SELECT 2").len(), 2);
        assert_eq!(d.native_type(&d.logical_type("int4")), "INTEGER");
        assert_eq!(d.qualified(Some("s"), "T"), "s.\"T\"");
        assert!(d.paginate("SELECT 1", 0, 10).contains("LIMIT 10"));
        assert_eq!(d.literal(&dblens_core::Value::Int(1)).unwrap(), "1");
    }
}
```

`lib.rs` 改为最终版本：
```rust
//! 方言层：不做任何 IO，只负责各数据库的语法差异——标识符与字面量、语句拆分、类型映射、SQL 生成。

mod dml;
pub mod ident;
pub mod literal;
mod split;
pub mod types;
mod variant;

use dblens_core::{LogicalType, Result, Value};
use dblens_driver::{DriverId, ServerInfo};

pub use split::{SqlStatement, split_statements};
pub use variant::{Family, Variant};

/// 某个具体数据库的方言。只包含变体信息，可以随意复制；所有方法都是纯计算。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Dialect {
    variant: Variant,
}

impl Dialect {
    pub fn new(variant: Variant) -> Self {
        Self { variant }
    }

    /// 根据驱动和服务器版本识别兼容库。
    pub fn detect(driver: DriverId, info: &ServerInfo) -> Self {
        Self::new(Variant::detect(driver, info))
    }

    /// 还没有服务器信息时使用该驱动的基础方言。
    pub fn for_driver(driver: DriverId) -> Self {
        Self::new(Variant::base(driver))
    }

    pub fn variant(&self) -> Variant {
        self.variant
    }

    pub fn family(&self) -> Family {
        self.variant.family()
    }

    pub fn quote_ident(&self, name: &str) -> String {
        ident::quote_ident(self.family(), name)
    }

    pub fn ident(&self, name: &str) -> String {
        ident::ident(self.family(), name)
    }

    /// `schema.name`，两部分各自按需加引号。
    pub fn qualified(&self, schema: Option<&str>, name: &str) -> String {
        match schema {
            Some(s) => format!("{}.{}", self.ident(s), self.ident(name)),
            None => self.ident(name),
        }
    }

    pub fn placeholder(&self, index: usize) -> String {
        ident::placeholder(self.family(), index)
    }

    pub fn literal(&self, value: &Value) -> Result<String> {
        literal::literal(self.family(), value)
    }

    pub fn paginate(&self, sql: &str, offset: u64, limit: u64) -> String {
        literal::paginate(self.family(), sql, offset, limit)
    }

    pub fn split(&self, sql: &str) -> Vec<SqlStatement> {
        split_statements(sql, self.family())
    }

    pub fn logical_type(&self, native: &str) -> LogicalType {
        types::logical_type(self.family(), native)
    }

    pub fn native_type(&self, logical: &LogicalType) -> String {
        types::native_type(self.family(), logical)
    }
}
```

- [ ] **Step 2: 运行测试，确认失败**

Run: `cargo test -p dblens-dialect dml`
Expected: 编译失败，提示 `Dialect` 上没有 `select_sql` 等方法。

- [ ] **Step 3: 实现 `dml.rs`**

在测试模块之前写入：
```rust
//! 由表模型生成 SELECT/INSERT/UPDATE/DELETE（"生成 SQL"菜单）。参数一律用占位符。

use dblens_core::schema::{Column, Table};

use crate::Dialect;

impl Dialect {
    /// 每列一行，列注释以 `-- 注释` 的形式附在行末。
    pub fn select_sql(&self, schema: Option<&str>, table: &Table) -> String {
        let from = self.qualified(schema, &table.name);
        if table.columns.is_empty() {
            return format!("SELECT *\nFROM {from}");
        }
        let last = table.columns.len() - 1;
        let lines: Vec<String> = table
            .columns
            .iter()
            .enumerate()
            .map(|(k, c)| {
                let sep = if k == last { "" } else { "," };
                let mut line = format!("    {}{sep}", self.ident(&c.name));
                if let Some(comment) = c.comment.as_deref().filter(|s| !s.trim().is_empty()) {
                    line.push_str(" -- ");
                    line.push_str(&comment.replace(['\r', '\n'], " ").replace("  ", " "));
                }
                line
            })
            .collect();
        format!("SELECT\n{}\nFROM {from}", lines.join("\n"))
    }

    /// 跳过自增列和生成列。
    pub fn insert_sql(&self, schema: Option<&str>, table: &Table) -> String {
        let cols: Vec<&Column> =
            table.columns.iter().filter(|c| !c.auto_increment && c.generated.is_none()).collect();
        let names: Vec<String> = cols.iter().map(|c| self.ident(&c.name)).collect();
        let marks: Vec<String> = (1..=cols.len()).map(|k| self.placeholder(k)).collect();
        format!(
            "INSERT INTO {} ({}) VALUES ({})",
            self.qualified(schema, &table.name),
            names.join(", "),
            marks.join(", ")
        )
    }

    /// 按主键更新其余列；没有主键或没有可更新的列时返回 None。
    pub fn update_sql(&self, schema: Option<&str>, table: &Table) -> Option<String> {
        let pk = primary_key(table)?;
        let set_cols: Vec<&Column> =
            table.columns.iter().filter(|c| c.generated.is_none() && !pk.contains(&c.name.as_str())).collect();
        if set_cols.is_empty() {
            return None;
        }
        let mut k = 0;
        let mut next = || {
            k += 1;
            self.placeholder(k)
        };
        let sets: Vec<String> = set_cols.iter().map(|c| format!("{} = {}", self.ident(&c.name), next())).collect();
        let wheres: Vec<String> = pk.iter().map(|c| format!("{} = {}", self.ident(c), next())).collect();
        Some(format!(
            "UPDATE {} SET {} WHERE {}",
            self.qualified(schema, &table.name),
            sets.join(", "),
            wheres.join(" AND ")
        ))
    }

    /// 按主键删除；没有主键时返回 None。
    pub fn delete_sql(&self, schema: Option<&str>, table: &Table) -> Option<String> {
        let pk = primary_key(table)?;
        let wheres: Vec<String> =
            pk.iter().enumerate().map(|(k, c)| format!("{} = {}", self.ident(c), self.placeholder(k + 1))).collect();
        Some(format!("DELETE FROM {} WHERE {}", self.qualified(schema, &table.name), wheres.join(" AND ")))
    }
}

fn primary_key(table: &Table) -> Option<Vec<&str>> {
    let pk = table.primary_key.as_ref()?;
    (!pk.columns.is_empty()).then(|| pk.columns.iter().map(String::as_str).collect())
}
```

- [ ] **Step 4: 运行测试，确认通过**

Run: `cargo test -p dblens-dialect && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all --check`
Expected: dialect 的全部测试 PASS（Task 1 有 6 个、Task 2 有 11 个、Task 3 有 6 个、Task 4 有 6 个、Task 5 有 6 个，共 35 个），没有警告，格式检查通过。

- [ ] **Step 5: 更新总览**

把 `docs/superpowers/plans/2026-10-22-m0-overview.md` 子计划表中 **M0-3a** 一行的状态改为：
`完成：详细计划 2026-10-22-m0-3a-dialect-core.md`

- [ ] **Step 6: 全量验证并提交**

Run: `cargo test --workspace`
Expected: 全部通过（不设置数据库环境变量时，数据库测试会跳过）。

```bash
git add crates/dblens-dialect docs/superpowers/plans/2026-10-22-m0-overview.md
git commit -m "feat(dialect): Dialect 结构体与 SELECT/INSERT/UPDATE/DELETE 生成" -m "Co-Authored-By: <实际模型> <noreply@anthropic.com>"
```
