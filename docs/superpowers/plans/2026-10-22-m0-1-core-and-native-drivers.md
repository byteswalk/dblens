# M0-1 核心类型与 MySQL/PG 驱动 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 建立 DBLens 的 Cargo 工作区，实现公共类型（`dblens-core`）、驱动接口与一致性测试套件（`dblens-driver`），以及通过一致性测试的 MySQL 和 PostgreSQL 驱动。

**Architecture:**
- `dblens-core` 只定义数据类型，不做任何 IO：错误、值、列类型、结构模型。
- `dblens-driver` 定义 `Driver`、`Connection`、`ExecStream` 这几个 trait，以及 `CancelHandle`。另外在 `conformance` 特性下提供一套各驱动共用的一致性测试。
- MySQL 驱动基于 `mysql_async`，PG 驱动基于 `tokio-postgres`。两者都实现拉取式的流式执行：调用方每次 `next(max_rows)` 才继续读取游标。

**Tech Stack:** Rust 1.99（edition 2024）、tokio、async-trait、serde、chrono、mysql_async 0.37、tokio-postgres 0.7、GitHub Actions。

**Spec:** `docs/superpowers/specs/2026-10-22-dblens-design.md`（第 4.2、5、6.1、8、9 节）。总览见 `docs/superpowers/plans/2026-10-22-m0-overview.md`。

## Global Constraints

- **仓库根目录**：`E:\VibeCoding\16数据库管理工具\dblens\`。下文的路径都相对于它。
- **工具链**：Rust 1.99.0 stable（由 `rust-toolchain.toml` 固定），edition 2024，`rust-version = "1.99"`。
- **命名**：crate 名统一用 `dblens-*`；所有 crate 设 `publish = false`。
- **依赖版本**：统一在根目录 `Cargo.toml` 的 `[workspace.dependencies]` 中声明，成员 crate 一律用 `xxx.workspace = true` 引用。
  - 如果某个依赖的实际 API 与本计划中的代码不一致，以 docs.rs 上对应版本的文档为准修改实现，但**不改变本计划定义的对外类型和函数签名**。
- **错误处理**：所有错误都转换为 `dblens_core::Error` 往上传，不允许 `unwrap` 吞错。测试代码除外。
- **敏感信息**：密码不得出现在 `Debug` 输出和日志中。
- **SQL 参数**：调用方传入的值一律走参数绑定，不拼接进 SQL。测试夹具中的表名是常量，拼接表名不受此限制。
- **注释和用户可见文案**：用中文。
- **集成测试的数据库**：
  - 连接 URL 从环境变量读取：`DBLENS_TEST_MYSQL_URL`、`DBLENS_TEST_MARIADB_URL`、`DBLENS_TEST_PG_URL`。
  - 未设置时测试会打印"跳过"并直接通过。
  - **Task 7 和 Task 9 必须在设置了这些环境变量的情况下实际跑通才算完成**，"跳过"不算完成。
- **提交信息**：每次提交都附加一行 `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`。
- **前置条件**：本机没有 Docker。执行 Task 7 之前，需要准备好 MySQL 8.4、MariaDB 11.4、PostgreSQL 17 测试库，方式二选一：
  - 安装 Docker Desktop（需要 WSL2），然后运行 `docker compose -f docker/docker-compose.test.yml up -d`；
  - 或者在本机或局域网已有的实例上，建一个空库 `dblens_test`，并设置上面三个环境变量。

---

## File Structure

```
dblens/
├── Cargo.toml                         工作区与依赖版本
├── rust-toolchain.toml                固定工具链 1.99.0 + rustfmt/clippy
├── rustfmt.toml
├── .gitignore
├── README.md                          项目说明、开发与测试方法
├── docker/docker-compose.test.yml     MySQL/MariaDB/PG 测试库
├── .github/workflows/ci.yml           fmt/clippy/单元测试（三平台）+ 集成测试（Linux）
├── docs/superpowers/{specs,plans}/    已有的设计文档与计划
└── crates/
    ├── dblens-core/src/
    │   ├── lib.rs                     模块导出
    │   ├── error.rs                   Error、Result
    │   ├── value.rs                   Value、LobPreview、Row、cap_large
    │   ├── types.rs                   LogicalType、ColumnType
    │   └── schema.rs                  结构模型
    ├── dblens-driver/
    │   ├── src/lib.rs                 模块导出
    │   ├── src/params.rs              DriverId、ConnectParams、Capabilities、ServerInfo
    │   ├── src/exec.rs                ColumnMeta、ExecEvent、ExecStream、collect_all
    │   ├── src/cancel.rs              CancelHandle
    │   ├── src/traits.rs              Driver、Connection
    │   ├── src/conformance.rs         一致性测试套件（feature = "conformance"）
    │   └── tests/fake_driver.rs       用内存假驱动验证 trait 可用性
    ├── dblens-driver-mysql/
    │   ├── src/lib.rs                 MySqlDriver
    │   ├── src/conn.rs                MySqlConnection
    │   ├── src/exec.rs                MyExec（流式结果）
    │   ├── src/value.rs               类型名、值映射、参数转换
    │   ├── src/error.rs               map_err
    │   └── tests/conformance.rs       MySQL 和 MariaDB 一致性测试
    └── dblens-driver-postgres/
        ├── src/lib.rs                 PgDriver
        ├── src/conn.rs                PgConnection（含 notice 转发、手动事务）
        ├── src/exec.rs                PgExec（流式结果）
        ├── src/types.rs               PgNumeric、PgInterval、RawValue、PgParam、pg_value
        ├── src/error.rs               map_err
        └── tests/conformance.rs       PostgreSQL 一致性测试
```

---

### Task 1: 工作区脚手架与 git 仓库

**Files:**
- Create: `Cargo.toml`, `rust-toolchain.toml`, `rustfmt.toml`, `.gitignore`, `README.md`
- Create: `crates/dblens-core/Cargo.toml`, `crates/dblens-core/src/lib.rs`

**Interfaces:**
- Consumes: 无
- Produces: 工作区依赖表（后续所有 crate 都引用它）；空的 `dblens-core` crate

- [ ] **Step 1: 创建 `Cargo.toml`（工作区）**

```toml
[workspace]
resolver = "3"
members = ["crates/*"]

[workspace.package]
version = "0.1.0"
edition = "2024"
rust-version = "1.99"
publish = false

[workspace.dependencies]
dblens-core = { path = "crates/dblens-core" }
dblens-driver = { path = "crates/dblens-driver" }

async-trait = "0.1.92"
base64 = "0.23.1"
bytes = "1.12.1"
chrono = { version = "0.4.45", default-features = false, features = ["std", "clock", "serde"] }
futures = "0.3.34"
mysql_async = { version = "0.37.1", default-features = false, features = ["default-rustls"] }
percent-encoding = "2.3.2"
serde = { version = "1.0.229", features = ["derive"] }
serde_json = "1.0.154"
thiserror = "2.0.21"
tokio = { version = "1.54.1", features = ["rt-multi-thread", "macros", "sync", "time"] }
tokio-postgres = { version = "0.7.18", features = ["with-chrono-0_4", "with-serde_json-1", "with-uuid-1"] }
url = "2.5.8"
uuid = { version = "1.28.0", features = ["serde"] }
```

- [ ] **Step 2: 创建 `rust-toolchain.toml`、`rustfmt.toml`、`.gitignore`**

`rust-toolchain.toml`：
```toml
[toolchain]
channel = "1.99.0"
components = ["rustfmt", "clippy"]
```

`rustfmt.toml`：
```toml
max_width = 110
```

`.gitignore`：
```
/target
**/*.rs.bk
.env
.idea/
.vscode/
node_modules/
```

- [ ] **Step 3: 创建 `crates/dblens-core/Cargo.toml` 和 `src/lib.rs`**

`crates/dblens-core/Cargo.toml`：
```toml
[package]
name = "dblens-core"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
publish.workspace = true

[dependencies]
base64.workspace = true
chrono.workspace = true
serde.workspace = true
serde_json.workspace = true
thiserror.workspace = true
uuid.workspace = true
```

`crates/dblens-core/src/lib.rs`：
```rust
//! DBLens 公共类型：错误、值、列类型、结构模型。本 crate 不做任何 IO。
```

- [ ] **Step 4: 创建 `README.md`**

````markdown
# DBLens（数洞）

跨平台数据库管理工具，后端 Rust，前端 Web UI，形态是 Tauri 桌面应用加可选的浏览器模式。

- 设计文档：`docs/superpowers/specs/2026-10-22-dblens-design.md`
- 实施计划：`docs/superpowers/plans/`

## 开发

```bash
cargo build --workspace
cargo test --workspace
```

## 集成测试数据库

驱动的一致性测试需要真实数据库。连接 URL 从环境变量读取，未设置时对应测试会自动跳过：

| 环境变量 | 示例 |
|---|---|
| `DBLENS_TEST_MYSQL_URL` | `mysql://root:dblens@127.0.0.1:3306/dblens_test` |
| `DBLENS_TEST_MARIADB_URL` | `mysql://root:dblens@127.0.0.1:3307/dblens_test` |
| `DBLENS_TEST_PG_URL` | `postgres://postgres:dblens@127.0.0.1:5432/dblens_test` |

有 Docker 时可以一键启动这三个测试库：

```bash
docker compose -f docker/docker-compose.test.yml up -d
```

PowerShell 中设置环境变量：`$env:DBLENS_TEST_MYSQL_URL = "mysql://root:dblens@127.0.0.1:3306/dblens_test"`
````

- [ ] **Step 5: 构建验证**

Run: `cargo build --workspace`
Expected: 编译成功（首次会下载工具链和依赖），没有错误。

- [ ] **Step 6: 初始化 git 并提交**

```bash
cd "E:/VibeCoding/16数据库管理工具/dblens"
git init -b main
git add -A
git commit -m "chore: 初始化 DBLens 工作区与设计文档" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 2: 错误类型与值类型（`dblens-core`）

**Files:**
- Create: `crates/dblens-core/src/error.rs`, `crates/dblens-core/src/value.rs`
- Modify: `crates/dblens-core/src/lib.rs`

**Interfaces:**
- Consumes: Task 1 的工作区
- Produces:
  - `dblens_core::Error`（枚举，变体为 `Connect{message}`、`Auth{message}`、`Sql{code,message,position:Option<u32>}`、`Timeout`、`Cancelled`、`DriverUnavailable{hint}`、`Internal{message}`），以及 `Error::connect(impl Into<String>)` 和 `Error::internal(impl Into<String>)`
  - `dblens_core::Result<T>`
  - `dblens_core::Value`（变体见下）、`dblens_core::LobPreview { binary, preview, total_len, truncated }`、`dblens_core::Row = Vec<Value>`
  - `Value::cap_large(self, limit: usize) -> Value`
  - `dblens_core::DEFAULT_LOB_PREVIEW_BYTES: usize = 65536`

- [ ] **Step 1: 写失败的测试**

在 `crates/dblens-core/src/error.rs` 末尾写入：
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_is_chinese_and_contains_code() {
        let e = Error::Sql { code: "1146".into(), message: "表不存在".into(), position: None };
        assert_eq!(e.to_string(), "SQL 错误 [1146]：表不存在");
        assert_eq!(Error::Cancelled.to_string(), "执行已取消");
    }

    #[test]
    fn serializes_with_kind_tag() {
        let e = Error::Sql { code: "42P01".into(), message: "x".into(), position: Some(15) };
        let json = serde_json::to_value(&e).unwrap();
        assert_eq!(json, serde_json::json!({"kind":"sql","code":"42P01","message":"x","position":15}));
        assert_eq!(serde_json::to_value(Error::Timeout).unwrap(), serde_json::json!({"kind":"timeout"}));
    }
}
```

在 `crates/dblens-core/src/value.rs` 末尾写入：
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{NaiveDate, TimeZone};

    #[test]
    fn json_shape_is_tagged() {
        assert_eq!(serde_json::to_value(Value::Int(42)).unwrap(), serde_json::json!({"t":"int","v":42}));
        assert_eq!(serde_json::to_value(Value::Null).unwrap(), serde_json::json!({"t":"null"}));
        assert_eq!(serde_json::to_value(Value::Bytes(vec![1, 2, 3])).unwrap(), serde_json::json!({"t":"bytes","v":"AQID"}));
    }

    #[test]
    fn all_variants_round_trip() {
        let values = vec![
            Value::Null,
            Value::Bool(true),
            Value::Int(-7),
            Value::UInt(u64::MAX),
            Value::Float(1.5),
            Value::Decimal("12345678901234567890.123456789".into()),
            Value::String("数洞".into()),
            Value::Bytes(vec![0, 255]),
            Value::Date(NaiveDate::from_ymd_opt(2024, 2, 29).unwrap()),
            Value::Time(chrono::NaiveTime::from_hms_micro_opt(13, 45, 30, 123456).unwrap()),
            Value::DateTime(NaiveDate::from_ymd_opt(2024, 2, 29).unwrap().and_hms_opt(1, 2, 3).unwrap()),
            Value::DateTimeTz(chrono::FixedOffset::east_opt(8 * 3600).unwrap().with_ymd_and_hms(2024, 2, 29, 13, 45, 30).unwrap()),
            Value::Interval("0 mons 1 days 02:03:04".into()),
            Value::Json(serde_json::json!({"a": [1, 2]})),
            Value::Uuid(uuid::Uuid::nil()),
            Value::Lob(LobPreview { binary: false, preview: "abc".into(), total_len: 100, truncated: true }),
            Value::Other { type_name: "geometry".into(), text: "\\x00".into() },
        ];
        let json = serde_json::to_string(&values).unwrap();
        let back: Vec<Value> = serde_json::from_str(&json).unwrap();
        assert_eq!(back, values);
    }

    #[test]
    fn cap_large_keeps_small_values() {
        assert_eq!(Value::String("abc".into()).cap_large(10), Value::String("abc".into()));
        assert_eq!(Value::Int(1).cap_large(0), Value::Int(1));
    }

    #[test]
    fn cap_large_truncates_on_char_boundary() {
        // "数" 占 3 字节，limit=4 时只能保留一个完整字符
        let v = Value::String("数洞数洞".into()).cap_large(4);
        assert_eq!(v, Value::Lob(LobPreview { binary: false, preview: "数".into(), total_len: 12, truncated: true }));
    }

    #[test]
    fn cap_large_encodes_binary_preview() {
        let v = Value::Bytes(vec![1, 2, 3, 4, 5]).cap_large(3);
        assert_eq!(v, Value::Lob(LobPreview { binary: true, preview: "AQID".into(), total_len: 5, truncated: true }));
    }
}
```

把 `crates/dblens-core/src/lib.rs` 改为：
```rust
//! DBLens 公共类型：错误、值、列类型、结构模型。本 crate 不做任何 IO。

mod error;
mod value;

pub use error::{Error, Result};
pub use value::{DEFAULT_LOB_PREVIEW_BYTES, LobPreview, Row, Value};
```

- [ ] **Step 2: 运行测试，确认失败**

Run: `cargo test -p dblens-core`
Expected: 编译失败，提示找不到 `Error`、`Value` 等类型。

- [ ] **Step 3: 实现 `error.rs`**

在 `crates/dblens-core/src/error.rs` 顶部（测试模块之前）写入：
```rust
use serde::{Deserialize, Serialize};

/// DBLens 统一错误类型。各层都用它向上传递错误，不允许吞掉。
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Error {
    #[error("连接失败：{message}")]
    Connect { message: String },
    #[error("认证失败：{message}")]
    Auth { message: String },
    /// 数据库返回的 SQL 错误。`code` 是数据库原生错误码（如 `1146`、`42P01`、`ORA-00942`），
    /// `position` 是出错位置在 SQL 文本中的字符偏移（从 1 开始），数据库没给时为 None。
    #[error("SQL 错误 [{code}]：{message}")]
    Sql { code: String, message: String, position: Option<u32> },
    #[error("执行超时")]
    Timeout,
    #[error("执行已取消")]
    Cancelled,
    #[error("驱动不可用：{hint}")]
    DriverUnavailable { hint: String },
    #[error("内部错误：{message}")]
    Internal { message: String },
}

impl Error {
    pub fn connect(message: impl Into<String>) -> Self {
        Self::Connect { message: message.into() }
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::Internal { message: message.into() }
    }
}

pub type Result<T> = std::result::Result<T, Error>;
```

- [ ] **Step 4: 实现 `value.rs`**

在 `crates/dblens-core/src/value.rs` 顶部（测试模块之前）写入：
```rust
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use chrono::{DateTime, FixedOffset, NaiveDate, NaiveDateTime, NaiveTime};
use serde::{Deserialize, Serialize};

/// 驱动返回的单元格值。序列化为 `{"t": 类型, "v": 值}`，供前端按类型渲染。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", content = "v", rename_all = "snake_case")]
pub enum Value {
    Null,
    Bool(bool),
    Int(i64),
    UInt(u64),
    Float(f64),
    /// 精确小数，保存数据库返回的文本，不经过浮点。
    Decimal(String),
    String(String),
    Bytes(#[serde(with = "b64")] Vec<u8>),
    Date(NaiveDate),
    Time(NaiveTime),
    DateTime(NaiveDateTime),
    DateTimeTz(DateTime<FixedOffset>),
    /// 时间间隔的文本表示。
    Interval(String),
    Json(serde_json::Value),
    Uuid(uuid::Uuid),
    /// 大字段预览：只保留前若干字节。
    Lob(LobPreview),
    /// 暂不认识的类型，保留类型名和文本表示。
    Other { type_name: String, text: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LobPreview {
    pub binary: bool,
    /// 文本 LOB：前 N 字节对应的文本；二进制 LOB：前 N 字节的 base64。
    pub preview: String,
    pub total_len: u64,
    pub truncated: bool,
}

pub type Row = Vec<Value>;

/// 大字段默认预览长度（字节）。
pub const DEFAULT_LOB_PREVIEW_BYTES: usize = 64 * 1024;

impl Value {
    /// 超过 `limit` 字节的字符串或二进制转换为 LOB 预览，避免大字段占满内存。
    pub fn cap_large(self, limit: usize) -> Value {
        match self {
            Value::String(s) if s.len() > limit => {
                let mut end = limit;
                while !s.is_char_boundary(end) {
                    end -= 1;
                }
                Value::Lob(LobPreview {
                    binary: false,
                    preview: s[..end].to_string(),
                    total_len: s.len() as u64,
                    truncated: true,
                })
            }
            Value::Bytes(b) if b.len() > limit => Value::Lob(LobPreview {
                binary: true,
                preview: STANDARD.encode(&b[..limit]),
                total_len: b.len() as u64,
                truncated: true,
            }),
            other => other,
        }
    }
}

mod b64 {
    use base64::Engine;
    use base64::engine::general_purpose::STANDARD;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(bytes: &[u8], s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&STANDARD.encode(bytes))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
        let s = String::deserialize(d)?;
        STANDARD.decode(s).map_err(serde::de::Error::custom)
    }
}
```

- [ ] **Step 5: 运行测试，确认通过**

Run: `cargo test -p dblens-core`
Expected: 7 个测试全部 PASS。

- [ ] **Step 6: 提交**

```bash
git add crates/dblens-core
git commit -m "feat(core): 统一错误类型与值类型" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 3: 列类型与结构模型（`dblens-core`）

**Files:**
- Create: `crates/dblens-core/src/types.rs`, `crates/dblens-core/src/schema.rs`
- Modify: `crates/dblens-core/src/lib.rs`

**Interfaces:**
- Consumes: Task 2
- Produces（全部 `pub`，derive 了 `Debug, Clone, PartialEq, Serialize, Deserialize`）：
  - `LogicalType`：`Bool`、`Int{bits:u8}`、`UInt{bits:u8}`、`Float{bits:u8}`、`Decimal{precision:Option<u32>,scale:Option<i32>}`、`Char{len:Option<u32>}`、`VarChar{len:Option<u32>}`、`Text`、`Binary{len:Option<u32>}`、`VarBinary{len:Option<u32>}`、`Blob`、`Date`、`Time{precision:Option<u8>}`、`Timestamp{tz:bool,precision:Option<u8>}`、`Interval`、`Json`、`Uuid`、`Other`
  - `ColumnType { native: String, logical: LogicalType }`
  - `Catalog`、`Schema`、`Table`、`Column`、`PrimaryKey`、`UniqueConstraint`、`ForeignKey`、`CheckConstraint`、`Index`、`IndexColumn`、`Trigger`、`View`、`Sequence`、`Routine`、`RoutineKind`、`Synonym`（字段见 Step 3）

- [ ] **Step 1: 写失败的测试**

`crates/dblens-core/src/schema.rs` 末尾：
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ColumnType, LogicalType};

    fn sample_schema() -> Schema {
        let id = Column {
            name: "id".into(),
            ordinal: 1,
            data_type: ColumnType { native: "NUMBER(10)".into(), logical: LogicalType::Decimal { precision: Some(10), scale: Some(0) } },
            nullable: false,
            default: None,
            comment: Some("主键".into()),
            auto_increment: false,
            generated: None,
        };
        let name = Column {
            name: "name".into(),
            ordinal: 2,
            data_type: ColumnType { native: "VARCHAR2(100 CHAR)".into(), logical: LogicalType::VarChar { len: Some(100) } },
            nullable: true,
            default: Some("'无名'".into()),
            comment: Some("姓名".into()),
            auto_increment: false,
            generated: None,
        };
        let table = Table {
            name: "person".into(),
            comment: Some("人员".into()),
            columns: vec![id, name],
            primary_key: Some(PrimaryKey { name: Some("pk_person".into()), columns: vec!["id".into()] }),
            uniques: vec![UniqueConstraint { name: Some("uk_person_name".into()), columns: vec!["name".into()] }],
            foreign_keys: vec![ForeignKey {
                name: Some("fk_person_dept".into()),
                columns: vec!["dept_id".into()],
                ref_schema: None,
                ref_table: "dept".into(),
                ref_columns: vec!["id".into()],
                on_delete: Some("CASCADE".into()),
                on_update: None,
            }],
            checks: vec![CheckConstraint { name: Some("ck_id".into()), expression: "id > 0".into() }],
            indexes: vec![Index {
                name: "idx_person_name".into(),
                columns: vec![IndexColumn { name: "name".into(), descending: true }],
                unique: false,
                method: Some("NORMAL".into()),
            }],
            triggers: vec![Trigger { name: "trg".into(), timing: Some("BEFORE".into()), events: vec!["INSERT".into()], definition: None }],
            partition: None,
        };
        Schema {
            name: "HR".into(),
            tables: vec![table],
            views: vec![View { name: "v_person".into(), comment: None, definition: Some("select * from person".into()), columns: vec![] }],
            materialized_views: vec![],
            sequences: vec![Sequence {
                name: "seq_person".into(),
                start: Some("1".into()),
                increment: Some("1".into()),
                min_value: Some("1".into()),
                max_value: Some("9999999999999999999999999999".into()),
                cycle: false,
                cache: Some(20),
            }],
            routines: vec![Routine { name: "pkg_hr".into(), kind: RoutineKind::Package, source: None }],
            synonyms: vec![Synonym { name: "emp".into(), target_schema: Some("SCOTT".into()), target_name: "EMP".into(), db_link: None }],
        }
    }

    #[test]
    fn schema_round_trips_through_json() {
        let schema = sample_schema();
        let json = serde_json::to_string(&schema).unwrap();
        let back: Schema = serde_json::from_str(&json).unwrap();
        assert_eq!(back, schema);
    }

    #[test]
    fn logical_type_uses_snake_case_kind() {
        let json = serde_json::to_value(LogicalType::VarChar { len: Some(100) }).unwrap();
        assert_eq!(json, serde_json::json!({"kind":"var_char","len":100}));
        assert_eq!(serde_json::to_value(LogicalType::Json).unwrap(), serde_json::json!({"kind":"json"}));
    }

    #[test]
    fn missing_collections_default_to_empty() {
        let t: Table = serde_json::from_str(r#"{"name":"t"}"#).unwrap();
        assert!(t.columns.is_empty() && t.indexes.is_empty() && t.primary_key.is_none());
        let s: Schema = serde_json::from_str(r#"{"name":"s"}"#).unwrap();
        assert!(s.tables.is_empty() && s.synonyms.is_empty());
    }
}
```

把 `lib.rs` 改为：
```rust
//! DBLens 公共类型：错误、值、列类型、结构模型。本 crate 不做任何 IO。

mod error;
pub mod schema;
mod types;
mod value;

pub use error::{Error, Result};
pub use types::{ColumnType, LogicalType};
pub use value::{DEFAULT_LOB_PREVIEW_BYTES, LobPreview, Row, Value};
```

- [ ] **Step 2: 运行测试，确认失败**

Run: `cargo test -p dblens-core`
Expected: 编译失败，提示找不到 `types` 模块和 schema 中的各类型。

- [ ] **Step 3: 实现 `types.rs` 和 `schema.rs`**

`crates/dblens-core/src/types.rs`：
```rust
use serde::{Deserialize, Serialize};

/// 与具体数据库无关的逻辑类型，用于跨库类型映射和设计器。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LogicalType {
    Bool,
    Int { bits: u8 },
    UInt { bits: u8 },
    Float { bits: u8 },
    Decimal { precision: Option<u32>, scale: Option<i32> },
    Char { len: Option<u32> },
    VarChar { len: Option<u32> },
    Text,
    Binary { len: Option<u32> },
    VarBinary { len: Option<u32> },
    Blob,
    Date,
    Time { precision: Option<u8> },
    Timestamp { tz: bool, precision: Option<u8> },
    Interval,
    Json,
    Uuid,
    Other,
}

/// 列类型：同时保存原生类型文本（展示、同库 DDL 用）和逻辑类型（跨库映射用）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ColumnType {
    pub native: String,
    pub logical: LogicalType,
}
```

`crates/dblens-core/src/schema.rs`（测试模块之前）：
```rust
//! 统一结构模型。层级：Catalog → Schema → 各类对象。
//! Oracle/达梦没有 Catalog 层；MySQL 的 database 同时充当 Schema。

use serde::{Deserialize, Serialize};

use crate::ColumnType;

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Catalog {
    pub name: String,
    #[serde(default)]
    pub schemas: Vec<Schema>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Schema {
    pub name: String,
    #[serde(default)]
    pub tables: Vec<Table>,
    #[serde(default)]
    pub views: Vec<View>,
    #[serde(default)]
    pub materialized_views: Vec<View>,
    #[serde(default)]
    pub sequences: Vec<Sequence>,
    #[serde(default)]
    pub routines: Vec<Routine>,
    #[serde(default)]
    pub synonyms: Vec<Synonym>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Table {
    pub name: String,
    #[serde(default)]
    pub comment: Option<String>,
    #[serde(default)]
    pub columns: Vec<Column>,
    #[serde(default)]
    pub primary_key: Option<PrimaryKey>,
    #[serde(default)]
    pub uniques: Vec<UniqueConstraint>,
    #[serde(default)]
    pub foreign_keys: Vec<ForeignKey>,
    #[serde(default)]
    pub checks: Vec<CheckConstraint>,
    #[serde(default)]
    pub indexes: Vec<Index>,
    #[serde(default)]
    pub triggers: Vec<Trigger>,
    /// 分区信息的文本描述，首期只读。
    #[serde(default)]
    pub partition: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Column {
    pub name: String,
    /// 列序号，从 1 开始。
    pub ordinal: u32,
    pub data_type: ColumnType,
    pub nullable: bool,
    /// 默认值表达式的原文。
    #[serde(default)]
    pub default: Option<String>,
    #[serde(default)]
    pub comment: Option<String>,
    #[serde(default)]
    pub auto_increment: bool,
    /// 生成列的表达式。
    #[serde(default)]
    pub generated: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PrimaryKey {
    pub name: Option<String>,
    pub columns: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UniqueConstraint {
    pub name: Option<String>,
    pub columns: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ForeignKey {
    pub name: Option<String>,
    pub columns: Vec<String>,
    pub ref_schema: Option<String>,
    pub ref_table: String,
    pub ref_columns: Vec<String>,
    pub on_delete: Option<String>,
    pub on_update: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CheckConstraint {
    pub name: Option<String>,
    pub expression: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Index {
    pub name: String,
    pub columns: Vec<IndexColumn>,
    pub unique: bool,
    /// 索引方法或类型，如 BTREE、HASH、NORMAL、BITMAP。
    pub method: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IndexColumn {
    /// 列名；函数索引时为表达式原文。
    pub name: String,
    pub descending: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Trigger {
    pub name: String,
    /// BEFORE / AFTER / INSTEAD OF
    pub timing: Option<String>,
    /// INSERT / UPDATE / DELETE 等
    pub events: Vec<String>,
    pub definition: Option<String>,
}

/// 视图和物化视图共用。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct View {
    pub name: String,
    pub comment: Option<String>,
    pub definition: Option<String>,
    #[serde(default)]
    pub columns: Vec<Column>,
}

/// 序列。数值用字符串保存，因为 Oracle 序列上限可达 28 位。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Sequence {
    pub name: String,
    pub start: Option<String>,
    pub increment: Option<String>,
    pub min_value: Option<String>,
    pub max_value: Option<String>,
    pub cycle: bool,
    pub cache: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RoutineKind {
    Procedure,
    Function,
    Package,
    PackageBody,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Routine {
    pub name: String,
    pub kind: RoutineKind,
    pub source: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Synonym {
    pub name: String,
    pub target_schema: Option<String>,
    pub target_name: String,
    pub db_link: Option<String>,
}
```

- [ ] **Step 4: 运行测试，确认通过**

Run: `cargo test -p dblens-core`
Expected: 共 10 个测试，全部 PASS。

- [ ] **Step 5: 提交**

```bash
git add crates/dblens-core
git commit -m "feat(core): 逻辑类型与统一结构模型" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 4: 驱动接口（`dblens-driver`）

**Files:**
- Create: `crates/dblens-driver/Cargo.toml`
- Create: `crates/dblens-driver/src/{lib.rs,params.rs,exec.rs,cancel.rs,traits.rs}`
- Test: `crates/dblens-driver/tests/fake_driver.rs`

**Interfaces:**
- Consumes: `dblens_core::{Error, Result, Value, Row}`
- Produces:
  - `DriverId { Mysql, Postgres, Oracle, Dameng }`（序列化为 snake_case）
  - `ConnectParams { host: String, port: u16, user: String, password: String, database: Option<String>, options: BTreeMap<String,String>, connect_timeout: Duration }`
    - `Debug` 中密码显示为 `***`
    - `ConnectParams::DEFAULT_CONNECT_TIMEOUT`（10 秒）
    - `ConnectParams::from_url(&str) -> Result<(DriverId, ConnectParams)>`
  - `Capabilities { catalogs, schemas, transactional_ddl, cancel, explain, multiple_result_sets: bool }`
  - `ServerInfo { product: String, version: String }`
  - `ColumnMeta { name, type_name: String, nullable: Option<bool>, precision: Option<u32>, scale: Option<i32>, source_table: Option<String>, source_column: Option<String> }`，以及 `ColumnMeta::new(name, type_name)`
  - `ExecEvent { ResultStart{columns: Vec<ColumnMeta>}, Rows(Vec<Row>), ResultEnd, Affected(u64), Message(String) }`
  - `trait ExecStream: Send { async fn next(&mut self, max_rows: usize) -> Result<Option<ExecEvent>> }`
  - `async fn collect_all(stream: &mut (dyn ExecStream + '_), max_rows: usize) -> Result<Vec<ExecEvent>>`
  - `CancelHandle`（可 Clone），提供 `CancelHandle::new(f)` 和 `async fn cancel(&self) -> Result<()>`
  - `trait Driver`、`trait Connection`（签名见 Step 3）

- [ ] **Step 1: 创建 `Cargo.toml` 和失败的测试**

`crates/dblens-driver/Cargo.toml`：
```toml
[package]
name = "dblens-driver"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
publish.workspace = true

[features]
conformance = ["dep:tokio"]

[dependencies]
async-trait.workspace = true
dblens-core.workspace = true
futures.workspace = true
percent-encoding.workspace = true
serde.workspace = true
tokio = { workspace = true, optional = true }
url.workspace = true

[dev-dependencies]
tokio.workspace = true
```

`crates/dblens-driver/tests/fake_driver.rs`：
```rust
//! 用内存假驱动验证接口是否可用：trait 对象、流式拉取、批大小、取消句柄。

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use async_trait::async_trait;
use dblens_core::{Result, Value};
use dblens_driver::*;

struct FakeDriver;
struct FakeConn {
    cancelled: Arc<AtomicUsize>,
}
struct FakeStream {
    pending: Vec<ExecEvent>,
    rows_left: Vec<Vec<Value>>,
    started: bool,
}

#[async_trait]
impl Driver for FakeDriver {
    fn id(&self) -> DriverId {
        DriverId::Mysql
    }
    fn capabilities(&self) -> Capabilities {
        Capabilities { cancel: true, ..Default::default() }
    }
    async fn connect(&self, _params: &ConnectParams) -> Result<Box<dyn Connection>> {
        Ok(Box::new(FakeConn { cancelled: Arc::new(AtomicUsize::new(0)) }))
    }
}

#[async_trait]
impl ExecStream for FakeStream {
    async fn next(&mut self, max_rows: usize) -> Result<Option<ExecEvent>> {
        if !self.started {
            self.started = true;
            return Ok(Some(ExecEvent::ResultStart { columns: vec![ColumnMeta::new("n", "INT")] }));
        }
        if !self.rows_left.is_empty() {
            let take = max_rows.min(self.rows_left.len());
            return Ok(Some(ExecEvent::Rows(self.rows_left.drain(..take).collect())));
        }
        Ok(self.pending.pop())
    }
}

#[async_trait]
impl Connection for FakeConn {
    async fn execute<'a>(&'a mut self, _sql: &'a str, _params: &'a [Value]) -> Result<Box<dyn ExecStream + 'a>> {
        Ok(Box::new(FakeStream {
            pending: vec![ExecEvent::ResultEnd],
            rows_left: (1..=5).map(|i| vec![Value::Int(i)]).collect(),
            started: false,
        }))
    }
    fn cancel_handle(&self) -> CancelHandle {
        let counter = self.cancelled.clone();
        CancelHandle::new(move || {
            let counter = counter.clone();
            async move {
                counter.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }
        })
    }
    async fn set_autocommit(&mut self, _on: bool) -> Result<()> {
        Ok(())
    }
    async fn commit(&mut self) -> Result<()> {
        Ok(())
    }
    async fn rollback(&mut self) -> Result<()> {
        Ok(())
    }
    async fn ping(&mut self) -> Result<()> {
        Ok(())
    }
    async fn server_info(&mut self) -> Result<ServerInfo> {
        Ok(ServerInfo { product: "Fake".into(), version: "1.0".into() })
    }
    async fn close(self: Box<Self>) -> Result<()> {
        Ok(())
    }
}

#[tokio::test]
async fn trait_objects_stream_in_batches() {
    let driver: Box<dyn Driver> = Box::new(FakeDriver);
    let (_, params) = ConnectParams::from_url("mysql://u:p@localhost/db").unwrap();
    let mut conn = driver.connect(&params).await.unwrap();
    let mut stream = conn.execute("select", &[]).await.unwrap();
    let events = collect_all(stream.as_mut(), 2).await.unwrap();
    let sizes: Vec<usize> = events
        .iter()
        .filter_map(|e| if let ExecEvent::Rows(r) = e { Some(r.len()) } else { None })
        .collect();
    assert_eq!(sizes, vec![2, 2, 1]);
    assert!(matches!(events.first(), Some(ExecEvent::ResultStart { .. })));
    assert_eq!(events.last(), Some(&ExecEvent::ResultEnd));
}

#[tokio::test]
async fn cancel_handle_is_cloneable_and_invokes_closure() {
    let conn = FakeConn { cancelled: Arc::new(AtomicUsize::new(0)) };
    let h1 = conn.cancel_handle();
    let h2 = h1.clone();
    h1.cancel().await.unwrap();
    h2.cancel().await.unwrap();
    assert_eq!(conn.cancelled.load(Ordering::SeqCst), 2);
}
```

在 `crates/dblens-driver/src/params.rs` 末尾写入单元测试：
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_mysql_url_with_encoded_password() {
        let (id, p) = ConnectParams::from_url("mysql://root:p%40ss%3Aw@db.local:3307/app?charset=utf8mb4").unwrap();
        assert_eq!(id, DriverId::Mysql);
        assert_eq!((p.host.as_str(), p.port, p.user.as_str(), p.password.as_str()), ("db.local", 3307, "root", "p@ss:w"));
        assert_eq!(p.database.as_deref(), Some("app"));
        assert_eq!(p.options.get("charset").map(String::as_str), Some("utf8mb4"));
        assert_eq!(p.connect_timeout, ConnectParams::DEFAULT_CONNECT_TIMEOUT);
    }

    #[test]
    fn default_ports_and_empty_database() {
        let (id, p) = ConnectParams::from_url("postgres://u@h").unwrap();
        assert_eq!((id, p.port, p.database), (DriverId::Postgres, 5432, None));
        assert_eq!(ConnectParams::from_url("oracle://u:p@h/ORCLPDB1").unwrap().1.port, 1521);
        assert_eq!(ConnectParams::from_url("dm://SYSDBA:p@h").unwrap().0, DriverId::Dameng);
        assert_eq!(ConnectParams::from_url("dm://SYSDBA:p@h").unwrap().1.port, 5236);
    }

    #[test]
    fn connect_timeout_option() {
        let (_, p) = ConnectParams::from_url("mysql://u:p@h/db?connect_timeout=3").unwrap();
        assert_eq!(p.connect_timeout, Duration::from_secs(3));
    }

    #[test]
    fn rejects_unknown_scheme() {
        let err = ConnectParams::from_url("redis://h").unwrap_err();
        assert!(matches!(err, dblens_core::Error::Connect { .. }));
    }

    #[test]
    fn debug_hides_password() {
        let (_, p) = ConnectParams::from_url("mysql://u:secret@h/db").unwrap();
        let s = format!("{p:?}");
        assert!(!s.contains("secret") && s.contains("***"), "{s}");
    }
}
```

- [ ] **Step 2: 运行测试，确认失败**

Run: `cargo test -p dblens-driver`
Expected: 编译失败（`src/lib.rs` 不存在，或缺少各类型）。

- [ ] **Step 3: 实现各模块**

`crates/dblens-driver/src/lib.rs`：
```rust
//! 驱动接口：所有数据库驱动都实现这里的 `Driver` / `Connection` / `ExecStream`。

mod cancel;
mod exec;
mod params;
mod traits;

#[cfg(feature = "conformance")]
pub mod conformance;

pub use cancel::CancelHandle;
pub use exec::{ColumnMeta, ExecEvent, ExecStream, collect_all};
pub use params::{Capabilities, ConnectParams, DriverId, ServerInfo};
pub use traits::{Connection, Driver};
```

`crates/dblens-driver/src/params.rs`（测试模块之前）：
```rust
use std::collections::BTreeMap;
use std::fmt;
use std::time::Duration;

use dblens_core::{Error, Result};
use percent_encoding::percent_decode_str;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DriverId {
    Mysql,
    Postgres,
    Oracle,
    Dameng,
}

/// 建立连接所需的参数。
#[derive(Clone, PartialEq, Eq)]
pub struct ConnectParams {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub password: String,
    /// MySQL：默认库；PG：数据库名；Oracle：服务名；达梦：可为空。
    pub database: Option<String>,
    /// 驱动特有的选项。
    pub options: BTreeMap<String, String>,
    pub connect_timeout: Duration,
}

impl fmt::Debug for ConnectParams {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ConnectParams")
            .field("host", &self.host)
            .field("port", &self.port)
            .field("user", &self.user)
            .field("password", &"***")
            .field("database", &self.database)
            .field("options", &self.options)
            .field("connect_timeout", &self.connect_timeout)
            .finish()
    }
}

impl ConnectParams {
    pub const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

    /// 解析 `scheme://user:pass@host:port/database?k=v`。主要用于测试和命令行。
    /// 支持的 scheme：mysql、mariadb、postgres、postgresql、oracle、dm、dameng。
    /// 选项 `connect_timeout` 的单位为秒。
    pub fn from_url(s: &str) -> Result<(DriverId, ConnectParams)> {
        let u = url::Url::parse(s).map_err(|e| Error::connect(format!("无效的连接 URL：{e}")))?;
        let (id, default_port) = match u.scheme() {
            "mysql" | "mariadb" => (DriverId::Mysql, 3306),
            "postgres" | "postgresql" => (DriverId::Postgres, 5432),
            "oracle" => (DriverId::Oracle, 1521),
            "dm" | "dameng" => (DriverId::Dameng, 5236),
            other => return Err(Error::connect(format!("不支持的连接协议：{other}"))),
        };
        let decode = |v: &str| percent_decode_str(v).decode_utf8_lossy().into_owned();
        let host = u.host_str().ok_or_else(|| Error::connect("连接 URL 缺少主机名"))?.to_string();
        let database = Some(decode(u.path().trim_start_matches('/'))).filter(|d| !d.is_empty());
        let options: BTreeMap<String, String> = u.query_pairs().map(|(k, v)| (k.into_owned(), v.into_owned())).collect();
        let connect_timeout = match options.get("connect_timeout") {
            Some(v) => Duration::from_secs(
                v.parse().map_err(|_| Error::connect(format!("connect_timeout 不是有效的秒数：{v}")))?,
            ),
            None => Self::DEFAULT_CONNECT_TIMEOUT,
        };
        Ok((
            id,
            ConnectParams {
                host,
                port: u.port().unwrap_or(default_port),
                user: decode(u.username()),
                password: decode(u.password().unwrap_or("")),
                database,
                options,
                connect_timeout,
            },
        ))
    }
}

/// 驱动能力，供上层决定界面和行为。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Capabilities {
    /// 是否有 Catalog 层（PG 的 database）。
    pub catalogs: bool,
    pub schemas: bool,
    /// DDL 能否在事务中回滚。
    pub transactional_ddl: bool,
    pub cancel: bool,
    pub explain: bool,
    /// 一次执行能否返回多个结果集（如 MySQL 的 CALL）。
    pub multiple_result_sets: bool,
}

/// 服务器信息。`version` 保存数据库返回的原始版本字符串，供方言层识别兼容库。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerInfo {
    pub product: String,
    pub version: String,
}
```

`crates/dblens-driver/src/exec.rs`：
```rust
use async_trait::async_trait;
use dblens_core::{Result, Row};
use serde::{Deserialize, Serialize};

/// 结果集列的元信息。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ColumnMeta {
    pub name: String,
    /// 数据库原生类型名。
    pub type_name: String,
    pub nullable: Option<bool>,
    pub precision: Option<u32>,
    pub scale: Option<i32>,
    /// 来源表和列，能取到时填写，供数据编辑使用。
    pub source_table: Option<String>,
    pub source_column: Option<String>,
}

impl ColumnMeta {
    pub fn new(name: impl Into<String>, type_name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            type_name: type_name.into(),
            nullable: None,
            precision: None,
            scale: None,
            source_table: None,
            source_column: None,
        }
    }
}

/// 执行过程中依次产生的事件。
#[derive(Debug, Clone, PartialEq)]
pub enum ExecEvent {
    ResultStart { columns: Vec<ColumnMeta> },
    Rows(Vec<Row>),
    ResultEnd,
    Affected(u64),
    Message(String),
}

/// 拉取式的执行流：调用方调用 `next` 时才继续读取游标。
#[async_trait]
pub trait ExecStream: Send {
    /// 取下一个事件。`max_rows` 是单个 `Rows` 事件最多包含的行数。返回 `None` 表示执行全部结束。
    async fn next(&mut self, max_rows: usize) -> Result<Option<ExecEvent>>;
}

/// 读完整个执行流。只适合结果较小的场景（测试、元数据查询）。
pub async fn collect_all(stream: &mut (dyn ExecStream + '_), max_rows: usize) -> Result<Vec<ExecEvent>> {
    let mut events = Vec::new();
    while let Some(event) = stream.next(max_rows).await? {
        events.push(event);
    }
    Ok(events)
}
```

`crates/dblens-driver/src/cancel.rs`：
```rust
use std::fmt;
use std::future::Future;
use std::sync::Arc;

use dblens_core::Result;
use futures::future::BoxFuture;

type CancelFn = dyn Fn() -> BoxFuture<'static, Result<()>> + Send + Sync;

/// 取消正在执行的语句。可以克隆，可以在其他任务或线程中调用。
#[derive(Clone)]
pub struct CancelHandle {
    f: Arc<CancelFn>,
}

impl CancelHandle {
    pub fn new<F, Fut>(f: F) -> Self
    where
        F: Fn() -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<()>> + Send + 'static,
    {
        Self { f: Arc::new(move || Box::pin(f())) }
    }

    pub async fn cancel(&self) -> Result<()> {
        (self.f)().await
    }
}

impl fmt::Debug for CancelHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("CancelHandle")
    }
}
```

`crates/dblens-driver/src/traits.rs`：
```rust
use async_trait::async_trait;
use dblens_core::{Result, Value};

use crate::{CancelHandle, Capabilities, ConnectParams, DriverId, ExecStream, ServerInfo};

/// 一类数据库的驱动。
#[async_trait]
pub trait Driver: Send + Sync {
    fn id(&self) -> DriverId;
    fn capabilities(&self) -> Capabilities;
    async fn connect(&self, params: &ConnectParams) -> Result<Box<dyn Connection>>;
}

/// 一个数据库连接。每次只执行一条语句（拆分多条语句是方言层的职责）；
/// 只有存储过程等少数情况会返回多个结果集。
#[async_trait]
pub trait Connection: Send {
    async fn execute<'a>(&'a mut self, sql: &'a str, params: &'a [Value]) -> Result<Box<dyn ExecStream + 'a>>;
    /// 返回可以从其他任务调用的取消句柄。
    fn cancel_handle(&self) -> CancelHandle;
    /// 切换自动提交。从手动切回自动时，未提交的事务会被提交（与 JDBC 行为一致）。
    async fn set_autocommit(&mut self, on: bool) -> Result<()>;
    async fn commit(&mut self) -> Result<()>;
    async fn rollback(&mut self) -> Result<()>;
    async fn ping(&mut self) -> Result<()>;
    async fn server_info(&mut self) -> Result<ServerInfo>;
    async fn close(self: Box<Self>) -> Result<()>;
}
```

- [ ] **Step 4: 运行测试，确认通过**

Run: `cargo test -p dblens-driver`
Expected: 5 个单元测试 + 2 个集成测试全部 PASS。

- [ ] **Step 5: 提交**

```bash
git add crates/dblens-driver
git commit -m "feat(driver): 驱动接口、执行流与取消句柄" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 5: 一致性测试套件与测试数据库

**Files:**
- Create: `crates/dblens-driver/src/conformance.rs`
- Create: `docker/docker-compose.test.yml`

**Interfaces:**
- Consumes: Task 4 的全部接口
- Produces:
  - `dblens_driver::conformance::Fixture`（字段见下）
  - `conformance::load(env: &str, make: fn(ConnectParams) -> Fixture) -> Option<Fixture>`
  - 9 个检查函数：`ping_and_server_info`、`select_one`、`row_batching`、`dml_and_params`、`transaction_commit_and_rollback`、`cancel_running_query`、`sql_error_has_code`、`large_text_becomes_lob_preview`、`type_round_trip`，签名均为 `pub async fn xxx(f: &Fixture)`
  - 宏 `dblens_driver::conformance_tests!("ENV_VAR", fixture_fn)`，展开后在调用处生成 `mod conformance`，内含上述 9 个 `#[tokio::test]`

- [ ] **Step 1: 实现 `conformance.rs`**

这个任务产出的是测试工具本身，它的正确性要到 Task 7 和 Task 9 用真实数据库时才能验证。本任务只要求它在开启特性后能编译通过。

`crates/dblens-driver/src/conformance.rs`：
```rust
//! 驱动一致性测试套件。
//!
//! 用法：驱动 crate 在 dev-dependencies 中开启 `dblens-driver` 的 `conformance` 特性，
//! 写一个 `fn fixture(params: ConnectParams) -> Fixture`，再调用
//! `dblens_driver::conformance_tests!("DBLENS_TEST_XXX_URL", fixture);`。

use std::time::{Duration, Instant};

use dblens_core::{Error, Value};

use crate::{ConnectParams, Connection, Driver, ExecEvent, collect_all};

/// 每种数据库提供一份夹具：驱动、连接参数，以及该数据库方言下的测试 SQL。
pub struct Fixture {
    pub driver: Box<dyn Driver>,
    pub params: ConnectParams,
    /// 返回单行单列整数 1。
    pub select_one: &'static str,
    /// 返回 n 行单列整数 1..=n，并按升序排列。
    pub gen_rows: fn(usize) -> String,
    /// 一个至少运行 10 秒、被取消时会报错的查询。
    pub long_running: &'static str,
    /// 返回单行单列、长度为 100000 的字符串。
    pub large_text: &'static str,
    /// 建表：列 id（整数主键）、name（可变长字符串，100）。
    pub create_table: fn(&str) -> String,
    pub drop_table_if_exists: fn(&str) -> String,
    /// INSERT，两个占位参数依次为 id、name。
    pub insert_with_params: fn(&str) -> String,
    /// 按 id（一个占位参数）查询 name。
    pub select_name_by_id: fn(&str) -> String,
    pub count_rows: fn(&str) -> String,
    /// 查询一张不存在的表。
    pub missing_table_query: &'static str,
    /// （返回单行单列的 SQL，期望值）
    pub type_cases: Vec<(&'static str, Value)>,
}

/// 从环境变量读取连接 URL。未设置时返回 None，测试跳过。
pub fn load(env: &str, make: fn(ConnectParams) -> Fixture) -> Option<Fixture> {
    match std::env::var(env) {
        Ok(url) => {
            let (_, params) = ConnectParams::from_url(&url).expect("测试连接 URL 无效");
            Some(make(params))
        }
        Err(_) => {
            eprintln!("跳过：未设置环境变量 {env}");
            None
        }
    }
}

async fn connect(f: &Fixture) -> Box<dyn Connection> {
    f.driver.connect(&f.params).await.expect("连接测试库失败")
}

async fn run(conn: &mut dyn Connection, sql: &str, params: &[Value]) -> Result<Vec<ExecEvent>, Error> {
    let mut stream = conn.execute(sql, params).await?;
    collect_all(stream.as_mut(), 1000).await
}

fn single_value(events: &[ExecEvent]) -> Value {
    let rows: Vec<&Vec<Value>> = events
        .iter()
        .filter_map(|e| if let ExecEvent::Rows(r) = e { Some(r.iter()) } else { None })
        .flatten()
        .collect();
    assert_eq!(rows.len(), 1, "期望 1 行，实际事件：{events:?}");
    assert_eq!(rows[0].len(), 1, "期望 1 列，实际：{:?}", rows[0]);
    rows[0][0].clone()
}

fn as_i64(v: &Value) -> i64 {
    match v {
        Value::Int(i) => *i,
        Value::UInt(u) => i64::try_from(*u).expect("整数超出 i64 范围"),
        Value::Decimal(s) => s.parse().unwrap_or_else(|_| panic!("不是整数：{s}")),
        other => panic!("不是整数：{other:?}"),
    }
}

pub async fn ping_and_server_info(f: &Fixture) {
    let mut c = connect(f).await;
    c.ping().await.expect("ping 失败");
    let info = c.server_info().await.expect("server_info 失败");
    assert!(!info.version.is_empty(), "版本字符串为空");
    c.close().await.expect("关闭连接失败");
}

pub async fn select_one(f: &Fixture) {
    let mut c = connect(f).await;
    let events = run(c.as_mut(), f.select_one, &[]).await.expect("执行失败");
    assert!(
        matches!(events.first(), Some(ExecEvent::ResultStart { columns }) if columns.len() == 1),
        "{events:?}"
    );
    assert_eq!(events.last(), Some(&ExecEvent::ResultEnd), "{events:?}");
    assert_eq!(as_i64(&single_value(&events)), 1);
}

pub async fn row_batching(f: &Fixture) {
    let mut c = connect(f).await;
    let sql = (f.gen_rows)(1000);
    let mut stream = c.execute(&sql, &[]).await.expect("执行失败");
    let (mut sizes, mut values) = (Vec::new(), Vec::new());
    let (mut saw_start, mut saw_end) = (false, false);
    while let Some(event) = stream.next(300).await.expect("取数失败") {
        match event {
            ExecEvent::ResultStart { .. } => saw_start = true,
            ExecEvent::Rows(rows) => {
                sizes.push(rows.len());
                values.extend(rows.iter().map(|r| as_i64(&r[0])));
            }
            ExecEvent::ResultEnd => saw_end = true,
            ExecEvent::Message(_) => {}
            other => panic!("意外事件：{other:?}"),
        }
    }
    assert!(saw_start && saw_end, "缺少 ResultStart 或 ResultEnd");
    assert!(sizes.iter().all(|s| *s <= 300), "单批行数超过上限：{sizes:?}");
    assert_eq!(values, (1..=1000).collect::<Vec<i64>>());
}

pub async fn dml_and_params(f: &Fixture) {
    let t = "dblens_conf_dml";
    let mut c = connect(f).await;
    run(c.as_mut(), &(f.drop_table_if_exists)(t), &[]).await.expect("删表失败");
    run(c.as_mut(), &(f.create_table)(t), &[]).await.expect("建表失败");
    let events = run(c.as_mut(), &(f.insert_with_params)(t), &[Value::Int(1), Value::String("张三".into())])
        .await
        .expect("插入失败");
    assert!(events.contains(&ExecEvent::Affected(1)), "{events:?}");
    let events = run(c.as_mut(), &(f.select_name_by_id)(t), &[Value::Int(1)]).await.expect("查询失败");
    assert_eq!(single_value(&events), Value::String("张三".into()));
    run(c.as_mut(), &(f.drop_table_if_exists)(t), &[]).await.expect("删表失败");
}

pub async fn transaction_commit_and_rollback(f: &Fixture) {
    let t = "dblens_conf_tx";
    let mut c = connect(f).await;
    run(c.as_mut(), &(f.drop_table_if_exists)(t), &[]).await.expect("删表失败");
    run(c.as_mut(), &(f.create_table)(t), &[]).await.expect("建表失败");

    c.set_autocommit(false).await.expect("关闭自动提交失败");
    run(c.as_mut(), &(f.insert_with_params)(t), &[Value::Int(1), Value::String("a".into())]).await.expect("插入失败");
    c.rollback().await.expect("回滚失败");
    let n = as_i64(&single_value(&run(c.as_mut(), &(f.count_rows)(t), &[]).await.expect("计数失败")));
    assert_eq!(n, 0, "回滚后应为 0 行");

    run(c.as_mut(), &(f.insert_with_params)(t), &[Value::Int(2), Value::String("b".into())]).await.expect("插入失败");
    c.commit().await.expect("提交失败");
    c.set_autocommit(true).await.expect("恢复自动提交失败");

    // 用另一个连接确认数据确实已提交
    let mut other = connect(f).await;
    let n = as_i64(&single_value(&run(other.as_mut(), &(f.count_rows)(t), &[]).await.expect("计数失败")));
    assert_eq!(n, 1, "提交后其他连接应能看到 1 行");
    run(c.as_mut(), &(f.drop_table_if_exists)(t), &[]).await.expect("删表失败");
}

pub async fn cancel_running_query(f: &Fixture) {
    let mut c = connect(f).await;
    let handle = c.cancel_handle();
    let canceller = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(500)).await;
        handle.cancel().await
    });
    let started = Instant::now();
    let result = run(c.as_mut(), f.long_running, &[]).await;
    canceller.await.expect("取消任务异常退出").expect("发送取消失败");
    assert_eq!(result.unwrap_err(), Error::Cancelled);
    assert!(started.elapsed() < Duration::from_secs(5), "取消耗时过长：{:?}", started.elapsed());
    c.ping().await.expect("取消后连接应仍可用");
}

pub async fn sql_error_has_code(f: &Fixture) {
    let mut c = connect(f).await;
    match run(c.as_mut(), f.missing_table_query, &[]).await {
        Err(Error::Sql { code, message, .. }) => {
            assert!(!code.is_empty(), "错误码为空");
            assert!(!message.is_empty(), "错误信息为空");
        }
        other => panic!("期望 SQL 错误，实际：{other:?}"),
    }
    c.ping().await.expect("出错后连接应仍可用");
}

pub async fn large_text_becomes_lob_preview(f: &Fixture) {
    let mut c = connect(f).await;
    match single_value(&run(c.as_mut(), f.large_text, &[]).await.expect("执行失败")) {
        Value::Lob(lob) => {
            assert!(!lob.binary && lob.truncated);
            assert_eq!(lob.total_len, 100_000);
            assert_eq!(lob.preview.len(), dblens_core::DEFAULT_LOB_PREVIEW_BYTES);
        }
        other => panic!("期望 LOB 预览，实际：{other:?}"),
    }
}

pub async fn type_round_trip(f: &Fixture) {
    let mut c = connect(f).await;
    for (sql, expected) in &f.type_cases {
        let events = run(c.as_mut(), sql, &[]).await.unwrap_or_else(|e| panic!("{sql} 执行失败：{e}"));
        assert_eq!(&single_value(&events), expected, "SQL：{sql}");
    }
}

/// 生成全部一致性测试。必须在定义了 `fixture` 函数的测试文件根部调用。
#[macro_export]
macro_rules! conformance_tests {
    ($env:literal, $make:path) => {
        mod conformance {
            use super::*;

            #[tokio::test]
            async fn ping_and_server_info() {
                if let Some(f) = $crate::conformance::load($env, $make) {
                    $crate::conformance::ping_and_server_info(&f).await;
                }
            }
            #[tokio::test]
            async fn select_one() {
                if let Some(f) = $crate::conformance::load($env, $make) {
                    $crate::conformance::select_one(&f).await;
                }
            }
            #[tokio::test]
            async fn row_batching() {
                if let Some(f) = $crate::conformance::load($env, $make) {
                    $crate::conformance::row_batching(&f).await;
                }
            }
            #[tokio::test]
            async fn dml_and_params() {
                if let Some(f) = $crate::conformance::load($env, $make) {
                    $crate::conformance::dml_and_params(&f).await;
                }
            }
            #[tokio::test]
            async fn transaction_commit_and_rollback() {
                if let Some(f) = $crate::conformance::load($env, $make) {
                    $crate::conformance::transaction_commit_and_rollback(&f).await;
                }
            }
            #[tokio::test]
            async fn cancel_running_query() {
                if let Some(f) = $crate::conformance::load($env, $make) {
                    $crate::conformance::cancel_running_query(&f).await;
                }
            }
            #[tokio::test]
            async fn sql_error_has_code() {
                if let Some(f) = $crate::conformance::load($env, $make) {
                    $crate::conformance::sql_error_has_code(&f).await;
                }
            }
            #[tokio::test]
            async fn large_text_becomes_lob_preview() {
                if let Some(f) = $crate::conformance::load($env, $make) {
                    $crate::conformance::large_text_becomes_lob_preview(&f).await;
                }
            }
            #[tokio::test]
            async fn type_round_trip() {
                if let Some(f) = $crate::conformance::load($env, $make) {
                    $crate::conformance::type_round_trip(&f).await;
                }
            }
        }
    };
}
```

- [ ] **Step 2: 创建 `docker/docker-compose.test.yml`**

```yaml
# 驱动集成测试用的数据库。端口与 README 中的环境变量示例一致。
services:
  mysql:
    image: mysql:8.4
    environment:
      MYSQL_ROOT_PASSWORD: dblens
      MYSQL_DATABASE: dblens_test
    ports: ["3306:3306"]
  mariadb:
    image: mariadb:11.4
    environment:
      MARIADB_ROOT_PASSWORD: dblens
      MARIADB_DATABASE: dblens_test
    ports: ["3307:3306"]
  postgres:
    image: postgres:17
    environment:
      POSTGRES_PASSWORD: dblens
      POSTGRES_DB: dblens_test
    ports: ["5432:5432"]
```

- [ ] **Step 3: 编译验证**

Run: `cargo test -p dblens-driver --features conformance`
Expected: 编译通过，Task 4 的 7 个测试仍然 PASS。

Run: `cargo clippy -p dblens-driver --features conformance --all-targets -- -D warnings`
Expected: 没有警告。

- [ ] **Step 4: 提交**

```bash
git add crates/dblens-driver docker
git commit -m "test(driver): 驱动一致性测试套件与测试库编排" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 6: MySQL 值映射与错误映射（纯函数）

**Files:**
- Create: `crates/dblens-driver-mysql/Cargo.toml`
- Create: `crates/dblens-driver-mysql/src/lib.rs`, `src/value.rs`, `src/error.rs`

**Interfaces:**
- Consumes: `dblens_core::{Error, Value}`、`dblens_driver::ColumnMeta`
- Produces（crate 内部，`pub(crate)`）：
  - `value::native_type_name(&Column) -> String`
  - `value::column_meta(&Column) -> ColumnMeta`
  - `value::to_value(mysql_async::Value, &Column) -> Value`
  - `value::to_mysql_param(&Value) -> Result<mysql_async::Value>`
  - `error::map_err(mysql_async::Error) -> Error`

- [ ] **Step 1: 创建 `Cargo.toml` 和 `lib.rs`**

`crates/dblens-driver-mysql/Cargo.toml`：
```toml
[package]
name = "dblens-driver-mysql"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
publish.workspace = true

[dependencies]
async-trait.workspace = true
chrono.workspace = true
dblens-core.workspace = true
dblens-driver.workspace = true
mysql_async.workspace = true
serde_json.workspace = true
tokio.workspace = true

[dev-dependencies]
dblens-driver = { workspace = true, features = ["conformance"] }
serde_json.workspace = true
tokio.workspace = true
```

`crates/dblens-driver-mysql/src/lib.rs`（本任务先只声明这两个模块）：
```rust
//! MySQL 系驱动（MySQL、MariaDB，以及兼容 MySQL 协议的 TiDB、OceanBase、PolarDB 等）。

mod error;
mod value;
```

- [ ] **Step 2: 写失败的测试**

`crates/dblens-driver-mysql/src/value.rs` 末尾：
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{NaiveDate, NaiveTime};
    use mysql_async::consts::{ColumnFlags, ColumnType};

    fn col(t: ColumnType) -> Column {
        Column::new(t).with_character_set(255)
    }

    fn bin(t: ColumnType) -> Column {
        Column::new(t).with_character_set(63)
    }

    #[test]
    fn text_protocol_integers() {
        assert_eq!(to_value(MyValue::Bytes(b"-42".to_vec()), &col(ColumnType::MYSQL_TYPE_LONGLONG)), Value::Int(-42));
        let unsigned = col(ColumnType::MYSQL_TYPE_LONGLONG).with_flags(ColumnFlags::UNSIGNED_FLAG);
        assert_eq!(to_value(MyValue::Bytes(b"18446744073709551615".to_vec()), &unsigned), Value::UInt(u64::MAX));
        assert_eq!(native_type_name(&unsigned), "BIGINT UNSIGNED");
    }

    #[test]
    fn text_protocol_decimal_float_and_strings() {
        assert_eq!(
            to_value(MyValue::Bytes(b"123.4500".to_vec()), &col(ColumnType::MYSQL_TYPE_NEWDECIMAL)),
            Value::Decimal("123.4500".into())
        );
        assert_eq!(to_value(MyValue::Bytes(b"1.5".to_vec()), &col(ColumnType::MYSQL_TYPE_DOUBLE)), Value::Float(1.5));
        assert_eq!(
            to_value(MyValue::Bytes("数洞".as_bytes().to_vec()), &col(ColumnType::MYSQL_TYPE_VAR_STRING)),
            Value::String("数洞".into())
        );
        assert_eq!(to_value(MyValue::Bytes(vec![1, 2, 3]), &bin(ColumnType::MYSQL_TYPE_VAR_STRING)), Value::Bytes(vec![1, 2, 3]));
        assert_eq!(native_type_name(&bin(ColumnType::MYSQL_TYPE_BLOB)), "BLOB");
        assert_eq!(native_type_name(&col(ColumnType::MYSQL_TYPE_BLOB)), "TEXT");
    }

    #[test]
    fn text_protocol_temporal() {
        assert_eq!(
            to_value(MyValue::Bytes(b"2024-02-29".to_vec()), &col(ColumnType::MYSQL_TYPE_DATE)),
            Value::Date(NaiveDate::from_ymd_opt(2024, 2, 29).unwrap())
        );
        assert_eq!(
            to_value(MyValue::Bytes(b"2024-02-29 13:45:30.123456".to_vec()), &col(ColumnType::MYSQL_TYPE_DATETIME)),
            Value::DateTime(NaiveDate::from_ymd_opt(2024, 2, 29).unwrap().and_hms_micro_opt(13, 45, 30, 123456).unwrap())
        );
        assert_eq!(
            to_value(MyValue::Bytes(b"2024-02-29 13:45:30".to_vec()), &col(ColumnType::MYSQL_TYPE_TIMESTAMP)),
            Value::DateTime(NaiveDate::from_ymd_opt(2024, 2, 29).unwrap().and_hms_opt(13, 45, 30).unwrap())
        );
        assert_eq!(
            to_value(MyValue::Bytes(b"13:45:30".to_vec()), &col(ColumnType::MYSQL_TYPE_TIME)),
            Value::Time(NaiveTime::from_hms_opt(13, 45, 30).unwrap())
        );
        // 超出一天范围的 TIME 和零日期无法用 chrono 表示，保留原文
        assert_eq!(
            to_value(MyValue::Bytes(b"838:59:59".to_vec()), &col(ColumnType::MYSQL_TYPE_TIME)),
            Value::Other { type_name: "TIME".into(), text: "838:59:59".into() }
        );
        assert_eq!(
            to_value(MyValue::Bytes(b"0000-00-00".to_vec()), &col(ColumnType::MYSQL_TYPE_DATE)),
            Value::Other { type_name: "DATE".into(), text: "0000-00-00".into() }
        );
    }

    #[test]
    fn binary_protocol_values() {
        assert_eq!(to_value(MyValue::Int(7), &col(ColumnType::MYSQL_TYPE_LONG)), Value::Int(7));
        assert_eq!(to_value(MyValue::Double(2.5), &col(ColumnType::MYSQL_TYPE_DOUBLE)), Value::Float(2.5));
        assert_eq!(
            to_value(MyValue::Date(2024, 2, 29, 0, 0, 0, 0), &col(ColumnType::MYSQL_TYPE_DATE)),
            Value::Date(NaiveDate::from_ymd_opt(2024, 2, 29).unwrap())
        );
        assert_eq!(
            to_value(MyValue::Date(2024, 2, 29, 13, 45, 30, 5), &col(ColumnType::MYSQL_TYPE_DATETIME)),
            Value::DateTime(NaiveDate::from_ymd_opt(2024, 2, 29).unwrap().and_hms_micro_opt(13, 45, 30, 5).unwrap())
        );
        assert_eq!(
            to_value(MyValue::Time(false, 0, 13, 45, 30, 0), &col(ColumnType::MYSQL_TYPE_TIME)),
            Value::Time(NaiveTime::from_hms_opt(13, 45, 30).unwrap())
        );
        assert_eq!(to_value(MyValue::NULL, &col(ColumnType::MYSQL_TYPE_LONG)), Value::Null);
    }

    #[test]
    fn json_column() {
        assert_eq!(
            to_value(MyValue::Bytes(br#"{"a": 1}"#.to_vec()), &bin(ColumnType::MYSQL_TYPE_JSON)),
            Value::Json(serde_json::json!({"a": 1}))
        );
    }

    #[test]
    fn column_meta_reads_source_and_nullability() {
        let c = Column::new(ColumnType::MYSQL_TYPE_VAR_STRING)
            .with_name(b"n")
            .with_org_name(b"name")
            .with_org_table(b"person")
            .with_flags(ColumnFlags::NOT_NULL_FLAG)
            .with_character_set(255);
        let m = column_meta(&c);
        assert_eq!((m.name.as_str(), m.type_name.as_str(), m.nullable), ("n", "VARCHAR", Some(false)));
        assert_eq!((m.source_table.as_deref(), m.source_column.as_deref()), (Some("person"), Some("name")));
    }

    #[test]
    fn params_convert() {
        assert_eq!(to_mysql_param(&Value::Int(1)).unwrap(), MyValue::Int(1));
        assert_eq!(to_mysql_param(&Value::Bool(true)).unwrap(), MyValue::Int(1));
        assert_eq!(to_mysql_param(&Value::String("x".into())).unwrap(), MyValue::Bytes(b"x".to_vec()));
        assert_eq!(to_mysql_param(&Value::Null).unwrap(), MyValue::NULL);
        assert!(to_mysql_param(&Value::Other { type_name: "x".into(), text: "y".into() }).is_err());
    }
}
```

`crates/dblens-driver-mysql/src/error.rs` 末尾：
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use mysql_async::ServerError;

    fn server(code: u16, message: &str) -> mysql_async::Error {
        mysql_async::Error::Server(ServerError { code, message: message.into(), state: "HY000".into() })
    }

    #[test]
    fn maps_server_errors() {
        assert_eq!(map_err(server(1317, "Query execution was interrupted")), Error::Cancelled);
        assert!(matches!(map_err(server(1045, "Access denied")), Error::Auth { .. }));
        assert_eq!(
            map_err(server(1146, "Table 'x' doesn't exist")),
            Error::Sql { code: "1146".into(), message: "Table 'x' doesn't exist".into(), position: None }
        );
    }
}
```

- [ ] **Step 3: 运行测试，确认失败**

Run: `cargo test -p dblens-driver-mysql --lib`
Expected: 编译失败，提示找不到 `to_value`、`map_err` 等函数。

- [ ] **Step 4: 实现 `value.rs` 和 `error.rs`**

`crates/dblens-driver-mysql/src/value.rs`（测试模块之前）：
```rust
use chrono::{NaiveDate, NaiveDateTime, NaiveTime};
use dblens_core::{Error, Result, Value};
use dblens_driver::ColumnMeta;
use mysql_async::consts::{ColumnFlags, ColumnType};
use mysql_async::{Column, Value as MyValue};

/// MySQL 中 binary 字符集的编号。
const BINARY_CHARSET: u16 = 63;

fn is_binary(col: &Column) -> bool {
    col.character_set() == BINARY_CHARSET
}

/// 结果集列的原生类型名（协议层能看到的近似值）。
pub(crate) fn native_type_name(col: &Column) -> String {
    use ColumnType::*;
    let base = match col.column_type() {
        MYSQL_TYPE_TINY => "TINYINT",
        MYSQL_TYPE_SHORT => "SMALLINT",
        MYSQL_TYPE_INT24 => "MEDIUMINT",
        MYSQL_TYPE_LONG => "INT",
        MYSQL_TYPE_LONGLONG => "BIGINT",
        MYSQL_TYPE_FLOAT => "FLOAT",
        MYSQL_TYPE_DOUBLE => "DOUBLE",
        MYSQL_TYPE_DECIMAL | MYSQL_TYPE_NEWDECIMAL => "DECIMAL",
        MYSQL_TYPE_DATE | MYSQL_TYPE_NEWDATE => "DATE",
        MYSQL_TYPE_TIME | MYSQL_TYPE_TIME2 => "TIME",
        MYSQL_TYPE_DATETIME | MYSQL_TYPE_DATETIME2 => "DATETIME",
        MYSQL_TYPE_TIMESTAMP | MYSQL_TYPE_TIMESTAMP2 => "TIMESTAMP",
        MYSQL_TYPE_YEAR => "YEAR",
        MYSQL_TYPE_BIT => "BIT",
        MYSQL_TYPE_JSON => "JSON",
        MYSQL_TYPE_ENUM => "ENUM",
        MYSQL_TYPE_SET => "SET",
        MYSQL_TYPE_GEOMETRY => "GEOMETRY",
        MYSQL_TYPE_NULL => "NULL",
        MYSQL_TYPE_TINY_BLOB | MYSQL_TYPE_MEDIUM_BLOB | MYSQL_TYPE_LONG_BLOB | MYSQL_TYPE_BLOB => {
            if is_binary(col) { "BLOB" } else { "TEXT" }
        }
        MYSQL_TYPE_VARCHAR | MYSQL_TYPE_VAR_STRING => {
            if is_binary(col) { "VARBINARY" } else { "VARCHAR" }
        }
        MYSQL_TYPE_STRING => {
            if is_binary(col) { "BINARY" } else { "CHAR" }
        }
        _ => "UNKNOWN",
    };
    if col.flags().contains(ColumnFlags::UNSIGNED_FLAG) {
        format!("{base} UNSIGNED")
    } else {
        base.to_string()
    }
}

pub(crate) fn column_meta(col: &Column) -> ColumnMeta {
    let table = col.org_table_str();
    let source = col.org_name_str();
    let is_decimal = matches!(col.column_type(), ColumnType::MYSQL_TYPE_DECIMAL | ColumnType::MYSQL_TYPE_NEWDECIMAL);
    ColumnMeta {
        name: col.name_str().into_owned(),
        type_name: native_type_name(col),
        nullable: Some(!col.flags().contains(ColumnFlags::NOT_NULL_FLAG)),
        precision: None,
        scale: is_decimal.then(|| i32::from(col.decimals())),
        source_table: (!table.is_empty()).then(|| table.into_owned()),
        source_column: (!source.is_empty()).then(|| source.into_owned()),
    }
}

/// 把驱动返回的值转换为 `Value`。文本协议下所有值都是 Bytes，需要按列类型解析；
/// 二进制协议（带参数执行时）直接给出数值和时间。
pub(crate) fn to_value(v: MyValue, col: &Column) -> Value {
    match v {
        MyValue::NULL => Value::Null,
        MyValue::Int(i) => Value::Int(i),
        MyValue::UInt(u) => Value::UInt(u),
        MyValue::Float(f) => Value::Float(f64::from(f)),
        MyValue::Double(d) => Value::Float(d),
        MyValue::Date(y, mo, d, h, mi, s, us) => date_value(col, y, mo, d, h, mi, s, us),
        MyValue::Time(neg, days, h, mi, s, us) => {
            if !neg && days == 0 {
                match NaiveTime::from_hms_micro_opt(h.into(), mi.into(), s.into(), us) {
                    Some(t) => Value::Time(t),
                    None => other(col, format!("{h:02}:{mi:02}:{s:02}.{us:06}")),
                }
            } else {
                let sign = if neg { "-" } else { "" };
                let hours = u32::from(h) + days * 24;
                other(col, format!("{sign}{hours:02}:{mi:02}:{s:02}"))
            }
        }
        MyValue::Bytes(b) => bytes_value(b, col),
    }
}

fn other(col: &Column, text: String) -> Value {
    Value::Other { type_name: native_type_name(col), text }
}

#[allow(clippy::too_many_arguments)]
fn date_value(col: &Column, y: u16, mo: u8, d: u8, h: u8, mi: u8, s: u8, us: u32) -> Value {
    let date = NaiveDate::from_ymd_opt(y.into(), mo.into(), d.into());
    let is_date_only = matches!(col.column_type(), ColumnType::MYSQL_TYPE_DATE | ColumnType::MYSQL_TYPE_NEWDATE);
    match (date, is_date_only) {
        (Some(date), true) => Value::Date(date),
        (Some(date), false) => match date.and_hms_micro_opt(h.into(), mi.into(), s.into(), us) {
            Some(dt) => Value::DateTime(dt),
            None => other(col, format!("{y:04}-{mo:02}-{d:02} {h:02}:{mi:02}:{s:02}.{us:06}")),
        },
        // 零日期（0000-00-00）等 chrono 无法表示的值，保留原文
        (None, _) => other(col, format!("{y:04}-{mo:02}-{d:02}")),
    }
}

fn bytes_value(b: Vec<u8>, col: &Column) -> Value {
    use ColumnType::*;
    let text = || String::from_utf8_lossy(&b).into_owned();
    match col.column_type() {
        MYSQL_TYPE_TINY | MYSQL_TYPE_SHORT | MYSQL_TYPE_INT24 | MYSQL_TYPE_LONG | MYSQL_TYPE_LONGLONG
        | MYSQL_TYPE_YEAR => {
            let s = text();
            let parsed = if col.flags().contains(ColumnFlags::UNSIGNED_FLAG) {
                s.parse::<u64>().ok().map(Value::UInt)
            } else {
                s.parse::<i64>().ok().map(Value::Int)
            };
            parsed.unwrap_or_else(|| other(col, s))
        }
        MYSQL_TYPE_FLOAT | MYSQL_TYPE_DOUBLE => {
            let s = text();
            s.parse::<f64>().map(Value::Float).unwrap_or_else(|_| other(col, s))
        }
        MYSQL_TYPE_DECIMAL | MYSQL_TYPE_NEWDECIMAL => Value::Decimal(text()),
        MYSQL_TYPE_DATE | MYSQL_TYPE_NEWDATE => {
            let s = text();
            NaiveDate::parse_from_str(&s, "%Y-%m-%d").map(Value::Date).unwrap_or_else(|_| other(col, s))
        }
        MYSQL_TYPE_DATETIME | MYSQL_TYPE_DATETIME2 | MYSQL_TYPE_TIMESTAMP | MYSQL_TYPE_TIMESTAMP2 => {
            let s = text();
            NaiveDateTime::parse_from_str(&s, "%Y-%m-%d %H:%M:%S%.f")
                .map(Value::DateTime)
                .unwrap_or_else(|_| other(col, s))
        }
        MYSQL_TYPE_TIME | MYSQL_TYPE_TIME2 => {
            let s = text();
            NaiveTime::parse_from_str(&s, "%H:%M:%S%.f").map(Value::Time).unwrap_or_else(|_| other(col, s))
        }
        MYSQL_TYPE_JSON => serde_json::from_slice(&b).map(Value::Json).unwrap_or_else(|_| Value::String(text())),
        MYSQL_TYPE_BIT | MYSQL_TYPE_GEOMETRY => Value::Bytes(b),
        _ if is_binary(col) => Value::Bytes(b),
        _ => Value::String(text()),
    }
}

/// 把参数转换为 mysql_async 的值。
pub(crate) fn to_mysql_param(v: &Value) -> Result<MyValue> {
    use chrono::{Datelike, Timelike};
    let datetime = |dt: &NaiveDateTime| {
        MyValue::Date(
            dt.year() as u16,
            dt.month() as u8,
            dt.day() as u8,
            dt.hour() as u8,
            dt.minute() as u8,
            dt.second() as u8,
            dt.nanosecond() / 1000,
        )
    };
    Ok(match v {
        Value::Null => MyValue::NULL,
        Value::Bool(b) => MyValue::Int(i64::from(*b)),
        Value::Int(i) => MyValue::Int(*i),
        Value::UInt(u) => MyValue::UInt(*u),
        Value::Float(f) => MyValue::Double(*f),
        Value::Decimal(s) | Value::String(s) | Value::Interval(s) => MyValue::Bytes(s.as_bytes().to_vec()),
        Value::Bytes(b) => MyValue::Bytes(b.clone()),
        Value::Date(d) => MyValue::Date(d.year() as u16, d.month() as u8, d.day() as u8, 0, 0, 0, 0),
        Value::Time(t) => {
            MyValue::Time(false, 0, t.hour() as u8, t.minute() as u8, t.second() as u8, t.nanosecond() / 1000)
        }
        Value::DateTime(dt) => datetime(dt),
        // MySQL 没有带时区的时间类型，按该值自身时区的本地时间写入
        Value::DateTimeTz(dt) => datetime(&dt.naive_local()),
        Value::Json(j) => MyValue::Bytes(j.to_string().into_bytes()),
        Value::Uuid(u) => MyValue::Bytes(u.hyphenated().to_string().into_bytes()),
        Value::Lob(_) | Value::Other { .. } => {
            return Err(Error::internal(format!("该类型的值不能作为参数绑定：{v:?}")));
        }
    })
}
```

`crates/dblens-driver-mysql/src/error.rs`（测试模块之前）：
```rust
use dblens_core::Error;

/// MySQL 错误码：查询被 KILL QUERY 中断。
const ER_QUERY_INTERRUPTED: u16 = 1317;

pub(crate) fn map_err(e: mysql_async::Error) -> Error {
    match e {
        mysql_async::Error::Server(ref se) => match se.code {
            ER_QUERY_INTERRUPTED => Error::Cancelled,
            // 1044 无库权限，1045 密码错误，1698 用户被拒绝
            1044 | 1045 | 1698 => Error::Auth { message: se.message.clone() },
            code => Error::Sql { code: code.to_string(), message: se.message.clone(), position: None },
        },
        mysql_async::Error::Io(_) | mysql_async::Error::Driver(_) | mysql_async::Error::Url(_) => {
            Error::connect(e.to_string())
        }
        mysql_async::Error::Other(_) => Error::internal(e.to_string()),
    }
}
```

> 注：如果 `mysql_async::Error` 在 0.37 中被标记为 `#[non_exhaustive]`，就再加一个 `_ => Error::internal(e.to_string())` 分支。

- [ ] **Step 5: 运行测试，确认通过**

Run: `cargo test -p dblens-driver-mysql --lib`
Expected: 8 个测试全部 PASS（value 7 个、error 1 个）。

- [ ] **Step 6: 提交**

```bash
git add crates/dblens-driver-mysql
git commit -m "feat(mysql): 值映射、参数转换与错误映射" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 7: MySQL 连接与流式执行

**Files:**
- Create: `crates/dblens-driver-mysql/src/conn.rs`, `src/exec.rs`
- Modify: `crates/dblens-driver-mysql/src/lib.rs`
- Test: `crates/dblens-driver-mysql/tests/conformance.rs`

**Interfaces:**
- Consumes: Task 4 的 trait；Task 5 的 `Fixture` 和 `conformance_tests!`；Task 6 的映射函数
- Produces: `dblens_driver_mysql::MySqlDriver`（单元结构体，实现 `Driver`）、`dblens_driver_mysql::MySqlConnection`

- [ ] **Step 1: 写失败的一致性测试**

`crates/dblens-driver-mysql/tests/conformance.rs`：
```rust
use chrono::{NaiveDate, NaiveTime};
use dblens_core::Value;
use dblens_driver::ConnectParams;
use dblens_driver::conformance::Fixture;
use dblens_driver_mysql::MySqlDriver;

/// MySQL 与 MariaDB 都支持的类型用例。
fn common_type_cases() -> Vec<(&'static str, Value)> {
    vec![
        ("SELECT NULL", Value::Null),
        ("SELECT CAST(42 AS SIGNED)", Value::Int(42)),
        ("SELECT CAST(42 AS UNSIGNED)", Value::UInt(42)),
        ("SELECT CAST(1.5 AS DOUBLE)", Value::Float(1.5)),
        ("SELECT CAST('123.4500' AS DECIMAL(10,4))", Value::Decimal("123.4500".into())),
        ("SELECT '数洞'", Value::String("数洞".into())),
        ("SELECT CAST('2024-02-29' AS DATE)", Value::Date(NaiveDate::from_ymd_opt(2024, 2, 29).unwrap())),
        (
            "SELECT CAST('2024-02-29 13:45:30.123456' AS DATETIME(6))",
            Value::DateTime(NaiveDate::from_ymd_opt(2024, 2, 29).unwrap().and_hms_micro_opt(13, 45, 30, 123456).unwrap()),
        ),
        ("SELECT CAST('13:45:30' AS TIME)", Value::Time(NaiveTime::from_hms_opt(13, 45, 30).unwrap())),
        ("SELECT X'010203'", Value::Bytes(vec![1, 2, 3])),
    ]
}

fn base(params: ConnectParams, type_cases: Vec<(&'static str, Value)>) -> Fixture {
    Fixture {
        driver: Box::new(MySqlDriver),
        params,
        select_one: "SELECT 1",
        gen_rows: |n| {
            format!("WITH RECURSIVE s(n) AS (SELECT 1 UNION ALL SELECT n + 1 FROM s WHERE n < {n}) SELECT n FROM s ORDER BY n")
        },
        long_running: "SELECT COUNT(*) FROM information_schema.columns a, information_schema.columns b, information_schema.columns c",
        large_text: "SELECT REPEAT('a', 100000)",
        create_table: |t| {
            format!("CREATE TABLE {t} (id INT PRIMARY KEY, name VARCHAR(100)) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4")
        },
        drop_table_if_exists: |t| format!("DROP TABLE IF EXISTS {t}"),
        insert_with_params: |t| format!("INSERT INTO {t} (id, name) VALUES (?, ?)"),
        select_name_by_id: |t| format!("SELECT name FROM {t} WHERE id = ?"),
        count_rows: |t| format!("SELECT COUNT(*) FROM {t}"),
        missing_table_query: "SELECT * FROM dblens_no_such_table",
        type_cases,
    }
}

fn mysql_fixture(params: ConnectParams) -> Fixture {
    let mut cases = common_type_cases();
    cases.push(("SELECT CAST('{\"a\": 1}' AS JSON)", Value::Json(serde_json::json!({"a": 1}))));
    base(params, cases)
}

fn mariadb_fixture(params: ConnectParams) -> Fixture {
    base(params, common_type_cases())
}

mod mysql {
    use super::*;
    dblens_driver::conformance_tests!("DBLENS_TEST_MYSQL_URL", mysql_fixture);
}

mod mariadb {
    use super::*;
    dblens_driver::conformance_tests!("DBLENS_TEST_MARIADB_URL", mariadb_fixture);
}
```

- [ ] **Step 2: 运行测试，确认失败**

Run: `cargo test -p dblens-driver-mysql --test conformance`
Expected: 编译失败，提示找不到 `MySqlDriver`。

- [ ] **Step 3: 实现 `exec.rs`**

`crates/dblens-driver-mysql/src/exec.rs`：
```rust
use std::sync::Arc;

use async_trait::async_trait;
use dblens_core::{DEFAULT_LOB_PREVIEW_BYTES, Result, Row};
use dblens_driver::{ExecEvent, ExecStream};
use mysql_async::prelude::Protocol;
use mysql_async::{Column, QueryResult};

use crate::error::map_err;
use crate::value::{column_meta, to_value};

enum State {
    /// 位于两个结果集之间，下一步检查是否还有结果集。
    Between,
    InRows,
    /// 当前结果集的行已读完，下一步发出 ResultEnd。
    EndPending,
}

/// MySQL 执行流。`QueryResult` 在一个结果集读完后会自动跳到下一个结果集，
/// `columns()` 为 None 表示已没有更多结果集；列为空的结果集是 OK 包（影响行数）。
pub(crate) struct MyExec<'a, P: Protocol> {
    result: QueryResult<'a, 'static, P>,
    columns: Arc<[Column]>,
    state: State,
}

impl<'a, P: Protocol> MyExec<'a, P> {
    pub(crate) fn new(result: QueryResult<'a, 'static, P>) -> Self {
        Self { result, columns: Arc::from(Vec::new()), state: State::Between }
    }
}

fn convert_row(row: mysql_async::Row, columns: &[Column]) -> Row {
    row.unwrap()
        .into_iter()
        .zip(columns)
        .map(|(v, c)| to_value(v, c).cap_large(DEFAULT_LOB_PREVIEW_BYTES))
        .collect()
}

#[async_trait]
impl<'a, P> ExecStream for MyExec<'a, P>
where
    P: Protocol + Send + Sync + 'a,
{
    async fn next(&mut self, max_rows: usize) -> Result<Option<ExecEvent>> {
        match self.state {
            State::Between => {
                let Some(columns) = self.result.columns() else {
                    return Ok(None);
                };
                if columns.is_empty() {
                    let affected = self.result.affected_rows();
                    // 消费这个 OK 结果，推进到下一个结果集
                    self.result.next().await.map_err(map_err)?;
                    return Ok(Some(ExecEvent::Affected(affected)));
                }
                let metas = columns.iter().map(column_meta).collect();
                self.columns = columns;
                self.state = State::InRows;
                Ok(Some(ExecEvent::ResultStart { columns: metas }))
            }
            State::InRows => {
                let mut rows = Vec::new();
                while rows.len() < max_rows.max(1) {
                    match self.result.next().await.map_err(map_err)? {
                        Some(row) => rows.push(convert_row(row, &self.columns)),
                        None => {
                            self.state = State::EndPending;
                            break;
                        }
                    }
                }
                if rows.is_empty() {
                    self.state = State::Between;
                    Ok(Some(ExecEvent::ResultEnd))
                } else {
                    Ok(Some(ExecEvent::Rows(rows)))
                }
            }
            State::EndPending => {
                self.state = State::Between;
                Ok(Some(ExecEvent::ResultEnd))
            }
        }
    }
}
```

- [ ] **Step 4: 实现 `conn.rs` 和 `lib.rs`**

`crates/dblens-driver-mysql/src/conn.rs`：
```rust
use async_trait::async_trait;
use dblens_core::{Error, Result, Value};
use dblens_driver::{CancelHandle, Connection, ExecStream, ServerInfo};
use mysql_async::prelude::Queryable;
use mysql_async::{Conn, Opts};

use crate::error::map_err;
use crate::exec::MyExec;
use crate::value::to_mysql_param;

pub struct MySqlConnection {
    pub(crate) conn: Conn,
    pub(crate) opts: Opts,
}

#[async_trait]
impl Connection for MySqlConnection {
    async fn execute<'a>(&'a mut self, sql: &'a str, params: &'a [Value]) -> Result<Box<dyn ExecStream + 'a>> {
        if params.is_empty() {
            // 文本协议，支持一次返回多个结果集（如 CALL）
            let result = self.conn.query_iter(sql).await.map_err(map_err)?;
            Ok(Box::new(MyExec::new(result)))
        } else {
            let values = params.iter().map(to_mysql_param).collect::<Result<Vec<_>>>()?;
            let result = self.conn.exec_iter(sql, values).await.map_err(map_err)?;
            Ok(Box::new(MyExec::new(result)))
        }
    }

    /// 另开一个连接执行 `KILL QUERY <线程号>`。
    fn cancel_handle(&self) -> CancelHandle {
        let opts = self.opts.clone();
        let id = self.conn.id();
        CancelHandle::new(move || {
            let opts = opts.clone();
            async move {
                let mut killer = Conn::new(opts).await.map_err(map_err)?;
                killer.query_drop(format!("KILL QUERY {id}")).await.map_err(map_err)?;
                killer.disconnect().await.map_err(map_err)?;
                Ok(())
            }
        })
    }

    async fn set_autocommit(&mut self, on: bool) -> Result<()> {
        let sql = if on { "SET autocommit = 1" } else { "SET autocommit = 0" };
        self.conn.query_drop(sql).await.map_err(map_err)
    }

    async fn commit(&mut self) -> Result<()> {
        self.conn.query_drop("COMMIT").await.map_err(map_err)
    }

    async fn rollback(&mut self) -> Result<()> {
        self.conn.query_drop("ROLLBACK").await.map_err(map_err)
    }

    async fn ping(&mut self) -> Result<()> {
        self.conn.ping().await.map_err(map_err)
    }

    async fn server_info(&mut self) -> Result<ServerInfo> {
        let version: Option<String> = self.conn.query_first("SELECT VERSION()").await.map_err(map_err)?;
        Ok(ServerInfo {
            product: "MySQL".into(),
            version: version.ok_or_else(|| Error::internal("SELECT VERSION() 没有返回结果"))?,
        })
    }

    async fn close(self: Box<Self>) -> Result<()> {
        self.conn.disconnect().await.map_err(map_err)
    }
}
```

`crates/dblens-driver-mysql/src/lib.rs`：
```rust
//! MySQL 系驱动（MySQL、MariaDB，以及兼容 MySQL 协议的 TiDB、OceanBase、PolarDB 等）。

mod conn;
mod error;
mod exec;
mod value;

use async_trait::async_trait;
use dblens_core::{Error, Result};
use dblens_driver::{Capabilities, ConnectParams, Connection, Driver, DriverId};
use mysql_async::{Conn, Opts, OptsBuilder};

pub use conn::MySqlConnection;

use crate::error::map_err;

pub struct MySqlDriver;

#[async_trait]
impl Driver for MySqlDriver {
    fn id(&self) -> DriverId {
        DriverId::Mysql
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            catalogs: false,
            schemas: true,
            transactional_ddl: false,
            cancel: true,
            explain: true,
            multiple_result_sets: true,
        }
    }

    async fn connect(&self, p: &ConnectParams) -> Result<Box<dyn Connection>> {
        let opts: Opts = OptsBuilder::default()
            .ip_or_hostname(p.host.clone())
            .tcp_port(p.port)
            .user(Some(p.user.clone()))
            .pass(Some(p.password.clone()))
            .db_name(p.database.clone())
            .into();
        let conn = tokio::time::timeout(p.connect_timeout, Conn::new(opts.clone()))
            .await
            .map_err(|_| Error::connect(format!("连接超时（{} 秒）", p.connect_timeout.as_secs())))?
            .map_err(map_err)?;
        Ok(Box::new(MySqlConnection { conn, opts }))
    }
}
```

- [ ] **Step 5: 准备测试库并运行一致性测试**

按 Global Constraints 中的"前置条件"准备好 MySQL 8.4 和 MariaDB 11.4，并设置环境变量（PowerShell 示例）：
```powershell
$env:DBLENS_TEST_MYSQL_URL = "mysql://root:dblens@127.0.0.1:3306/dblens_test"
$env:DBLENS_TEST_MARIADB_URL = "mysql://root:dblens@127.0.0.1:3307/dblens_test"
```

Run: `cargo test -p dblens-driver-mysql --test conformance -- --nocapture`
Expected: `mysql::conformance::*` 和 `mariadb::conformance::*` 共 18 个测试全部 PASS，输出中**没有**"跳过"字样。

如果某个类型用例失败，先确认是映射函数的问题还是数据库返回的类型与预期不同（可以用 `--nocapture` 打印实际的事件），只修正映射代码，不修改期望值。

- [ ] **Step 6: 运行全部测试和 clippy**

Run: `cargo test -p dblens-driver-mysql && cargo clippy -p dblens-driver-mysql --all-targets -- -D warnings`
Expected: 全部 PASS，没有警告。

- [ ] **Step 7: 提交**

```bash
git add crates/dblens-driver-mysql
git commit -m "feat(mysql): MySQL/MariaDB 驱动连接、流式执行、取消与事务" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 8: PostgreSQL 类型映射（纯函数）

**Files:**
- Create: `crates/dblens-driver-postgres/Cargo.toml`
- Create: `crates/dblens-driver-postgres/src/lib.rs`, `src/types.rs`, `src/error.rs`

**Interfaces:**
- Consumes: `dblens_core::{Error, Value}`
- Produces（crate 内部）：
  - `types::decode_numeric(&[u8]) -> Result<String, BoxError>`
  - `types::format_interval(months: i32, days: i32, micros: i64) -> String`
  - `types::PgNumeric(String)`、`types::PgInterval(String)`、`types::RawValue<'a>(&'a [u8])`：均实现 `FromSql`
  - `types::PgParam<'a>(&'a Value)`：实现 `ToSql`
  - `types::pg_value(&tokio_postgres::Row, idx: usize) -> dblens_core::Result<Value>`
  - `error::map_err(tokio_postgres::Error) -> Error`

- [ ] **Step 1: 创建 `Cargo.toml` 和 `lib.rs`**

`crates/dblens-driver-postgres/Cargo.toml`：
```toml
[package]
name = "dblens-driver-postgres"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
publish.workspace = true

[dependencies]
async-trait.workspace = true
bytes.workspace = true
chrono.workspace = true
dblens-core.workspace = true
dblens-driver.workspace = true
futures.workspace = true
serde_json.workspace = true
tokio.workspace = true
tokio-postgres.workspace = true
uuid.workspace = true

[dev-dependencies]
dblens-driver = { workspace = true, features = ["conformance"] }
serde_json.workspace = true
tokio.workspace = true
```

`crates/dblens-driver-postgres/src/lib.rs`（本任务先只声明这两个模块）：
```rust
//! PostgreSQL 系驱动（PostgreSQL，以及兼容 PG 协议的人大金仓、openGauss、瀚高等）。

mod error;
mod types;
```

- [ ] **Step 2: 写失败的测试**

`crates/dblens-driver-postgres/src/types.rs` 末尾：
```rust
#[cfg(test)]
mod tests {
    use super::*;

    /// 按 PG numeric 二进制格式拼字节：ndigits, weight, sign, dscale, digits...
    fn numeric(weight: i16, sign: u16, dscale: u16, digits: &[i16]) -> Vec<u8> {
        let mut v = Vec::new();
        v.extend_from_slice(&(digits.len() as i16).to_be_bytes());
        v.extend_from_slice(&weight.to_be_bytes());
        v.extend_from_slice(&sign.to_be_bytes());
        v.extend_from_slice(&dscale.to_be_bytes());
        for d in digits {
            v.extend_from_slice(&d.to_be_bytes());
        }
        v
    }

    #[test]
    fn decodes_numeric() {
        assert_eq!(decode_numeric(&numeric(0, 0, 4, &[123, 4500])).unwrap(), "123.4500");
        assert_eq!(decode_numeric(&numeric(-1, 0, 2, &[500])).unwrap(), "0.05");
        assert_eq!(decode_numeric(&numeric(1, 0, 0, &[1])).unwrap(), "10000");
        assert_eq!(decode_numeric(&numeric(0, 0x4000, 1, &[3, 1400])).unwrap(), "-3.1");
        assert_eq!(decode_numeric(&numeric(0, 0, 0, &[])).unwrap(), "0");
        assert_eq!(
            decode_numeric(&numeric(4, 0, 9, &[1234, 5678, 9012, 3456, 7890, 1234, 5678, 9000])).unwrap(),
            "12345678901234567890.123456789"
        );
        assert_eq!(decode_numeric(&numeric(0, 0xC000, 0, &[])).unwrap(), "NaN");
        assert!(decode_numeric(&[0, 1]).is_err());
    }

    #[test]
    fn formats_interval() {
        assert_eq!(format_interval(0, 1, 7_384_000_000), "0 mons 1 days 02:03:04");
        assert_eq!(format_interval(14, 0, 1_500_000), "14 mons 0 days 00:00:01.500000");
        assert_eq!(format_interval(0, 0, -3_600_000_000), "0 mons 0 days -01:00:00");
    }
}
```

- [ ] **Step 3: 运行测试，确认失败**

Run: `cargo test -p dblens-driver-postgres --lib`
Expected: 编译失败，提示找不到 `decode_numeric`、`format_interval`。

- [ ] **Step 4: 实现 `types.rs` 和 `error.rs`**

`crates/dblens-driver-postgres/src/types.rs`（测试模块之前）：
```rust
use bytes::BytesMut;
use chrono::{DateTime, NaiveDate, NaiveDateTime, NaiveTime, Utc};
use dblens_core::{Error, Result, Value};
use tokio_postgres::Row;
use tokio_postgres::types::{FromSql, IsNull, ToSql, Type, to_sql_checked};

type BoxError = Box<dyn std::error::Error + Sync + Send>;

/// 解码 PG numeric 的二进制格式，得到精确的十进制文本。
/// 格式：ndigits(i16) weight(i16) sign(u16) dscale(u16)，后跟 ndigits 个以 10000 为基的 i16。
pub(crate) fn decode_numeric(raw: &[u8]) -> std::result::Result<String, BoxError> {
    if raw.len() < 8 {
        return Err("numeric 数据长度不足".into());
    }
    let i16_at = |i: usize| i16::from_be_bytes([raw[i], raw[i + 1]]);
    let ndigits = usize::try_from(i16_at(0))?;
    let weight = i32::from(i16_at(2));
    let sign = u16::from_be_bytes([raw[4], raw[5]]);
    let dscale = usize::from(u16::from_be_bytes([raw[6], raw[7]]));
    match sign {
        0xC000 => return Ok("NaN".into()),
        0xD000 => return Ok("Infinity".into()),
        0xF000 => return Ok("-Infinity".into()),
        _ => {}
    }
    if raw.len() < 8 + ndigits * 2 {
        return Err("numeric 数据长度与位数不符".into());
    }
    let digits: Vec<i16> = (0..ndigits).map(|i| i16_at(8 + i * 2)).collect();
    let digit = |idx: i32| if idx >= 0 { digits.get(idx as usize).copied().unwrap_or(0) } else { 0 };

    let mut int_part = String::new();
    if weight >= 0 {
        for i in 0..=weight {
            let d = digit(i);
            if i == 0 {
                int_part.push_str(&d.to_string());
            } else {
                int_part.push_str(&format!("{d:04}"));
            }
        }
    } else {
        int_part.push('0');
    }

    let mut frac = String::new();
    let mut k = 1;
    while frac.len() < dscale {
        frac.push_str(&format!("{:04}", digit(weight + k)));
        k += 1;
    }
    frac.truncate(dscale);

    let mut s = String::new();
    if sign == 0x4000 {
        s.push('-');
    }
    s.push_str(&int_part);
    if dscale > 0 {
        s.push('.');
        s.push_str(&frac);
    }
    Ok(s)
}

/// 把 PG interval（月、天、微秒）格式化为文本。
pub(crate) fn format_interval(months: i32, days: i32, micros: i64) -> String {
    let sign = if micros < 0 { "-" } else { "" };
    let us = micros.unsigned_abs();
    let (h, rem) = (us / 3_600_000_000, us % 3_600_000_000);
    let (m, rem) = (rem / 60_000_000, rem % 60_000_000);
    let (s, frac) = (rem / 1_000_000, rem % 1_000_000);
    let time = if frac == 0 {
        format!("{sign}{h:02}:{m:02}:{s:02}")
    } else {
        format!("{sign}{h:02}:{m:02}:{s:02}.{frac:06}")
    };
    format!("{months} mons {days} days {time}")
}

pub(crate) struct PgNumeric(pub String);

impl<'a> FromSql<'a> for PgNumeric {
    fn from_sql(_: &Type, raw: &'a [u8]) -> std::result::Result<Self, BoxError> {
        decode_numeric(raw).map(PgNumeric)
    }
    fn accepts(ty: &Type) -> bool {
        *ty == Type::NUMERIC
    }
}

pub(crate) struct PgInterval(pub String);

impl<'a> FromSql<'a> for PgInterval {
    fn from_sql(_: &Type, raw: &'a [u8]) -> std::result::Result<Self, BoxError> {
        if raw.len() != 16 {
            return Err("interval 数据长度应为 16 字节".into());
        }
        let micros = i64::from_be_bytes(raw[0..8].try_into()?);
        let days = i32::from_be_bytes(raw[8..12].try_into()?);
        let months = i32::from_be_bytes(raw[12..16].try_into()?);
        Ok(PgInterval(format_interval(months, days, micros)))
    }
    fn accepts(ty: &Type) -> bool {
        *ty == Type::INTERVAL
    }
}

/// 不认识的类型：原样取出二进制。
pub(crate) struct RawValue<'a>(pub &'a [u8]);

impl<'a> FromSql<'a> for RawValue<'a> {
    fn from_sql(_: &Type, raw: &'a [u8]) -> std::result::Result<Self, BoxError> {
        Ok(RawValue(raw))
    }
    fn accepts(_: &Type) -> bool {
        true
    }
}

/// 读取一行中第 `idx` 列的值。
pub(crate) fn pg_value(row: &Row, idx: usize) -> Result<Value> {
    let ty = row.columns()[idx].type_().clone();
    let fail = |e: tokio_postgres::Error| Error::internal(format!("读取第 {} 列（{}）失败：{e}", idx + 1, ty.name()));
    macro_rules! get {
        ($t:ty, $f:expr) => {
            row.try_get::<_, Option<$t>>(idx).map_err(fail)?.map($f)
        };
    }
    let value = match ty {
        Type::BOOL => get!(bool, Value::Bool),
        Type::INT2 => get!(i16, |v| Value::Int(v.into())),
        Type::INT4 => get!(i32, |v| Value::Int(v.into())),
        Type::INT8 => get!(i64, Value::Int),
        Type::OID => get!(u32, |v| Value::UInt(v.into())),
        Type::FLOAT4 => get!(f32, |v| Value::Float(v.into())),
        Type::FLOAT8 => get!(f64, Value::Float),
        Type::NUMERIC => get!(PgNumeric, |v| Value::Decimal(v.0)),
        Type::TEXT | Type::VARCHAR | Type::BPCHAR | Type::NAME | Type::UNKNOWN => get!(String, Value::String),
        Type::BYTEA => get!(Vec<u8>, Value::Bytes),
        Type::DATE => get!(NaiveDate, Value::Date),
        Type::TIME => get!(NaiveTime, Value::Time),
        Type::TIMESTAMP => get!(NaiveDateTime, Value::DateTime),
        Type::TIMESTAMPTZ => get!(DateTime<Utc>, |v| Value::DateTimeTz(v.fixed_offset())),
        Type::JSON | Type::JSONB => get!(serde_json::Value, Value::Json),
        Type::UUID => get!(uuid::Uuid, Value::Uuid),
        Type::INTERVAL => get!(PgInterval, |v| Value::Interval(v.0)),
        _ => {
            let raw = row.try_get::<_, Option<RawValue>>(idx).map_err(fail)?;
            raw.map(|r| Value::Other {
                type_name: ty.name().to_string(),
                // 枚举、xml、citext 等类型的二进制格式就是文本
                text: match std::str::from_utf8(r.0) {
                    Ok(s) => s.to_string(),
                    Err(_) => format!("\\x{}", r.0.iter().map(|b| format!("{b:02x}")).collect::<String>()),
                },
            })
        }
    };
    Ok(value.unwrap_or(Value::Null))
}

/// 把 `Value` 绑定为 PG 参数，按目标参数类型做必要的转换。
#[derive(Debug)]
pub(crate) struct PgParam<'a>(pub &'a Value);

impl ToSql for PgParam<'_> {
    fn to_sql(&self, ty: &Type, out: &mut BytesMut) -> std::result::Result<IsNull, BoxError> {
        match self.0 {
            Value::Null => Ok(IsNull::Yes),
            Value::Bool(b) => b.to_sql(ty, out),
            Value::Int(i) => match *ty {
                Type::INT2 => i16::try_from(*i)?.to_sql(ty, out),
                Type::INT4 => i32::try_from(*i)?.to_sql(ty, out),
                Type::FLOAT4 => (*i as f32).to_sql(ty, out),
                Type::FLOAT8 => (*i as f64).to_sql(ty, out),
                Type::NUMERIC => Err(NUMERIC_PARAM_HINT.into()),
                _ => i.to_sql(ty, out),
            },
            Value::UInt(u) => PgParam(&Value::Int(i64::try_from(*u)?)).to_sql(ty, out),
            Value::Float(f) => match *ty {
                Type::FLOAT4 => (*f as f32).to_sql(ty, out),
                Type::NUMERIC => Err(NUMERIC_PARAM_HINT.into()),
                _ => f.to_sql(ty, out),
            },
            Value::Decimal(_) if *ty == Type::NUMERIC => Err(NUMERIC_PARAM_HINT.into()),
            Value::String(s) | Value::Decimal(s) | Value::Interval(s) => s.as_str().to_sql(ty, out),
            Value::Bytes(b) => b.as_slice().to_sql(ty, out),
            Value::Date(d) => d.to_sql(ty, out),
            Value::Time(t) => t.to_sql(ty, out),
            Value::DateTime(dt) => dt.to_sql(ty, out),
            Value::DateTimeTz(dt) => dt.to_sql(ty, out),
            Value::Json(j) => j.to_sql(ty, out),
            Value::Uuid(u) => u.to_sql(ty, out),
            Value::Lob(_) | Value::Other { .. } => Err(format!("该类型的值不能作为参数绑定：{:?}", self.0).into()),
        }
    }

    fn accepts(_: &Type) -> bool {
        true
    }

    to_sql_checked!();
}

const NUMERIC_PARAM_HINT: &str = "暂不支持直接绑定 NUMERIC 参数，请在 SQL 中写成 $1::text::numeric";
```

`crates/dblens-driver-postgres/src/error.rs`：
```rust
use dblens_core::Error;
use tokio_postgres::error::ErrorPosition;

pub(crate) fn map_err(e: tokio_postgres::Error) -> Error {
    if let Some(db) = e.as_db_error() {
        let code = db.code().code();
        return match code {
            // query_canceled
            "57014" => Error::Cancelled,
            // invalid_password / invalid_authorization_specification
            "28P01" | "28000" => Error::Auth { message: db.message().to_string() },
            _ => Error::Sql {
                code: code.to_string(),
                message: db.message().to_string(),
                position: match db.position() {
                    Some(ErrorPosition::Original(p)) => Some(*p),
                    _ => None,
                },
            },
        };
    }
    if e.is_closed() {
        return Error::connect(format!("连接已断开：{e}"));
    }
    Error::connect(e.to_string())
}
```

- [ ] **Step 5: 运行测试，确认通过**

Run: `cargo test -p dblens-driver-postgres --lib`
Expected: 2 个测试全部 PASS。`pg_value` 和 `PgParam` 要依赖真实连接，在 Task 9 的一致性测试中覆盖。

- [ ] **Step 6: 提交**

```bash
git add crates/dblens-driver-postgres
git commit -m "feat(postgres): numeric/interval 解码、值映射与参数绑定" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 9: PostgreSQL 连接与流式执行

**Files:**
- Create: `crates/dblens-driver-postgres/src/conn.rs`, `src/exec.rs`
- Modify: `crates/dblens-driver-postgres/src/lib.rs`
- Test: `crates/dblens-driver-postgres/tests/conformance.rs`

**Interfaces:**
- Consumes: Task 4 的 trait；Task 5 的套件；Task 8 的 `pg_value`、`PgParam`、`map_err`
- Produces: `dblens_driver_postgres::PgDriver`（单元结构体，实现 `Driver`）、`dblens_driver_postgres::PgConnection`

- [ ] **Step 1: 写失败的一致性测试**

`crates/dblens-driver-postgres/tests/conformance.rs`：
```rust
use chrono::{FixedOffset, NaiveDate, TimeZone};
use dblens_core::Value;
use dblens_driver::ConnectParams;
use dblens_driver::conformance::Fixture;
use dblens_driver_postgres::PgDriver;

fn fixture(params: ConnectParams) -> Fixture {
    Fixture {
        driver: Box::new(PgDriver),
        params,
        select_one: "SELECT 1",
        gen_rows: |n| format!("SELECT g FROM generate_series(1, {n}) AS g ORDER BY g"),
        long_running: "SELECT pg_sleep(10)",
        large_text: "SELECT repeat('a', 100000)",
        create_table: |t| format!("CREATE TABLE {t} (id INT PRIMARY KEY, name VARCHAR(100))"),
        drop_table_if_exists: |t| format!("DROP TABLE IF EXISTS {t}"),
        insert_with_params: |t| format!("INSERT INTO {t} (id, name) VALUES ($1, $2)"),
        select_name_by_id: |t| format!("SELECT name FROM {t} WHERE id = $1"),
        count_rows: |t| format!("SELECT COUNT(*) FROM {t}"),
        missing_table_query: "SELECT * FROM dblens_no_such_table",
        type_cases: vec![
            ("SELECT NULL::text", Value::Null),
            ("SELECT true", Value::Bool(true)),
            ("SELECT 42::int4", Value::Int(42)),
            ("SELECT 42::int8", Value::Int(42)),
            ("SELECT 1.5::float8", Value::Float(1.5)),
            ("SELECT 123.4500::numeric(10,4)", Value::Decimal("123.4500".into())),
            (
                "SELECT 12345678901234567890.123456789::numeric",
                Value::Decimal("12345678901234567890.123456789".into()),
            ),
            ("SELECT 'NaN'::numeric", Value::Decimal("NaN".into())),
            ("SELECT '数洞'::text", Value::String("数洞".into())),
            ("SELECT DATE '2024-02-29'", Value::Date(NaiveDate::from_ymd_opt(2024, 2, 29).unwrap())),
            (
                "SELECT TIMESTAMP '2024-02-29 13:45:30.123456'",
                Value::DateTime(NaiveDate::from_ymd_opt(2024, 2, 29).unwrap().and_hms_micro_opt(13, 45, 30, 123456).unwrap()),
            ),
            (
                "SELECT TIMESTAMPTZ '2024-02-29 13:45:30+08'",
                Value::DateTimeTz(FixedOffset::east_opt(0).unwrap().with_ymd_and_hms(2024, 2, 29, 5, 45, 30).unwrap()),
            ),
            ("SELECT '\\x010203'::bytea", Value::Bytes(vec![1, 2, 3])),
            ("SELECT '{\"a\":1}'::jsonb", Value::Json(serde_json::json!({"a": 1}))),
            (
                "SELECT 'a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11'::uuid",
                Value::Uuid("a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11".parse().unwrap()),
            ),
            ("SELECT INTERVAL '1 day 02:03:04'", Value::Interval("0 mons 1 days 02:03:04".into())),
            ("SELECT 'x'::char(1)::\"char\"", Value::Other { type_name: "char".into(), text: "x".into() }),
        ],
    }
}

dblens_driver::conformance_tests!("DBLENS_TEST_PG_URL", fixture);
```

- [ ] **Step 2: 运行测试，确认失败**

Run: `cargo test -p dblens-driver-postgres --test conformance`
Expected: 编译失败，提示找不到 `PgDriver`。

- [ ] **Step 3: 实现 `exec.rs`**

`crates/dblens-driver-postgres/src/exec.rs`：
```rust
use std::pin::Pin;

use async_trait::async_trait;
use dblens_core::{DEFAULT_LOB_PREVIEW_BYTES, Result, Row};
use dblens_driver::{ColumnMeta, ExecEvent, ExecStream};
use futures::StreamExt;
use tokio::sync::mpsc::UnboundedReceiver;
use tokio_postgres::RowStream;

use crate::error::map_err;
use crate::types::pg_value;

enum State {
    Affected(u64),
    Start(Vec<ColumnMeta>),
    InRows,
    EndPending,
    Done,
}

/// PG 执行流。每次 `next` 时先把执行期间收到的 notice 作为 Message 发出。
pub(crate) struct PgExec<'a> {
    notices: &'a mut UnboundedReceiver<String>,
    rows: Option<Pin<Box<RowStream>>>,
    state: State,
}

impl<'a> PgExec<'a> {
    pub(crate) fn affected(notices: &'a mut UnboundedReceiver<String>, n: u64) -> Self {
        Self { notices, rows: None, state: State::Affected(n) }
    }

    pub(crate) fn rows(notices: &'a mut UnboundedReceiver<String>, columns: Vec<ColumnMeta>, rows: RowStream) -> Self {
        Self { notices, rows: Some(Box::pin(rows)), state: State::Start(columns) }
    }
}

fn convert_row(row: &tokio_postgres::Row) -> Result<Row> {
    (0..row.len()).map(|i| pg_value(row, i).map(|v| v.cap_large(DEFAULT_LOB_PREVIEW_BYTES))).collect()
}

#[async_trait]
impl ExecStream for PgExec<'_> {
    async fn next(&mut self, max_rows: usize) -> Result<Option<ExecEvent>> {
        if let Ok(message) = self.notices.try_recv() {
            return Ok(Some(ExecEvent::Message(message)));
        }
        match std::mem::replace(&mut self.state, State::Done) {
            State::Affected(n) => Ok(Some(ExecEvent::Affected(n))),
            State::Start(columns) => {
                self.state = State::InRows;
                Ok(Some(ExecEvent::ResultStart { columns }))
            }
            State::InRows => {
                let stream = self.rows.as_mut().expect("InRows 状态下必有结果流");
                let mut rows = Vec::new();
                let mut ended = false;
                while rows.len() < max_rows.max(1) {
                    match stream.next().await {
                        Some(row) => rows.push(convert_row(&row.map_err(map_err)?)?),
                        None => {
                            ended = true;
                            break;
                        }
                    }
                }
                if rows.is_empty() {
                    Ok(Some(ExecEvent::ResultEnd))
                } else {
                    self.state = if ended { State::EndPending } else { State::InRows };
                    Ok(Some(ExecEvent::Rows(rows)))
                }
            }
            State::EndPending => Ok(Some(ExecEvent::ResultEnd)),
            State::Done => Ok(None),
        }
    }
}
```

- [ ] **Step 4: 实现 `conn.rs` 和 `lib.rs`**

`crates/dblens-driver-postgres/src/conn.rs`：
```rust
use async_trait::async_trait;
use dblens_core::{Result, Value};
use dblens_driver::{CancelHandle, ColumnMeta, Connection, ExecStream, ServerInfo};
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::task::JoinHandle;
use tokio_postgres::{Client, NoTls};

use crate::error::map_err;
use crate::exec::PgExec;
use crate::types::PgParam;

pub struct PgConnection {
    pub(crate) client: Client,
    pub(crate) notices: UnboundedReceiver<String>,
    pub(crate) conn_task: JoinHandle<()>,
    pub(crate) autocommit: bool,
    pub(crate) in_tx: bool,
}

impl PgConnection {
    /// 手动提交模式下，在第一条语句前开启事务。
    async fn ensure_tx(&mut self) -> Result<()> {
        if !self.autocommit && !self.in_tx {
            self.client.batch_execute("BEGIN").await.map_err(map_err)?;
            self.in_tx = true;
        }
        Ok(())
    }

    async fn end_tx(&mut self, sql: &str) -> Result<()> {
        if self.in_tx {
            self.client.batch_execute(sql).await.map_err(map_err)?;
            self.in_tx = false;
        }
        Ok(())
    }
}

#[async_trait]
impl Connection for PgConnection {
    async fn execute<'a>(&'a mut self, sql: &'a str, params: &'a [Value]) -> Result<Box<dyn ExecStream + 'a>> {
        self.ensure_tx().await?;
        let stmt = self.client.prepare(sql).await.map_err(map_err)?;
        let pg_params: Vec<PgParam<'a>> = params.iter().map(PgParam).collect();
        // &PgParam 实现了 ToSql，因而也实现了 BorrowToSql；它是 Send，可以跨 await 持有
        let args = || pg_params.iter();
        if stmt.columns().is_empty() {
            let n = self.client.execute_raw(&stmt, args()).await.map_err(map_err)?;
            Ok(Box::new(PgExec::affected(&mut self.notices, n)))
        } else {
            let columns = stmt.columns().iter().map(|c| ColumnMeta::new(c.name(), c.type_().name())).collect();
            let rows = self.client.query_raw(&stmt, args()).await.map_err(map_err)?;
            Ok(Box::new(PgExec::rows(&mut self.notices, columns, rows)))
        }
    }

    fn cancel_handle(&self) -> CancelHandle {
        let token = self.client.cancel_token();
        CancelHandle::new(move || {
            let token = token.clone();
            async move { token.cancel_query(NoTls).await.map_err(map_err) }
        })
    }

    async fn set_autocommit(&mut self, on: bool) -> Result<()> {
        if on {
            self.end_tx("COMMIT").await?;
        }
        self.autocommit = on;
        Ok(())
    }

    async fn commit(&mut self) -> Result<()> {
        self.end_tx("COMMIT").await
    }

    async fn rollback(&mut self) -> Result<()> {
        self.end_tx("ROLLBACK").await
    }

    async fn ping(&mut self) -> Result<()> {
        self.client.batch_execute("SELECT 1").await.map_err(map_err)
    }

    async fn server_info(&mut self) -> Result<ServerInfo> {
        let row = self.client.query_one("SELECT version()", &[]).await.map_err(map_err)?;
        Ok(ServerInfo { product: "PostgreSQL".into(), version: row.get(0) })
    }

    async fn close(self: Box<Self>) -> Result<()> {
        let this = *self;
        drop(this.client);
        // 连接任务在 Client 释放后会自行结束
        let _ = this.conn_task.await;
        Ok(())
    }
}
```

`crates/dblens-driver-postgres/src/lib.rs`：
```rust
//! PostgreSQL 系驱动（PostgreSQL，以及兼容 PG 协议的人大金仓、openGauss、瀚高等）。

mod conn;
mod error;
mod exec;
mod types;

use async_trait::async_trait;
use dblens_core::Result;
use dblens_driver::{Capabilities, ConnectParams, Connection, Driver, DriverId};
use futures::StreamExt;
use tokio::sync::mpsc;
use tokio_postgres::{AsyncMessage, NoTls};

pub use conn::PgConnection;

use crate::error::map_err;

pub struct PgDriver;

#[async_trait]
impl Driver for PgDriver {
    fn id(&self) -> DriverId {
        DriverId::Postgres
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            catalogs: true,
            schemas: true,
            transactional_ddl: true,
            cancel: true,
            explain: true,
            multiple_result_sets: false,
        }
    }

    async fn connect(&self, p: &ConnectParams) -> Result<Box<dyn Connection>> {
        let mut cfg = tokio_postgres::Config::new();
        cfg.host(&p.host)
            .port(p.port)
            .user(&p.user)
            .password(&p.password)
            .dbname(p.database.as_deref().unwrap_or("postgres"))
            .connect_timeout(p.connect_timeout)
            .application_name("DBLens");
        let (client, mut connection) = cfg.connect(NoTls).await.map_err(map_err)?;

        // 驱动连接任务，同时把服务器发来的 notice 转发给执行流
        let (tx, notices) = mpsc::unbounded_channel();
        let conn_task = tokio::spawn(async move {
            let mut messages = futures::stream::poll_fn(move |cx| connection.poll_message(cx));
            while let Some(message) = messages.next().await {
                match message {
                    Ok(AsyncMessage::Notice(n)) => {
                        let _ = tx.send(format!("{}: {}", n.severity(), n.message()));
                    }
                    Ok(_) => {}
                    Err(_) => break,
                }
            }
        });

        Ok(Box::new(PgConnection { client, notices, conn_task, autocommit: true, in_tx: false }))
    }
}
```

- [ ] **Step 5: 准备测试库并运行一致性测试**

```powershell
$env:DBLENS_TEST_PG_URL = "postgres://postgres:dblens@127.0.0.1:5432/dblens_test"
```

Run: `cargo test -p dblens-driver-postgres --test conformance -- --nocapture`
Expected: 9 个测试全部 PASS，输出中**没有**"跳过"字样。

如果某个类型用例失败，用 `--nocapture` 看实际值，只修正映射代码，不修改期望值。唯一的例外是 `"char"` 用例：如果 PG 返回的二进制不是可读文本，可以把期望改为对应的 `\x..` 十六进制文本，并在测试旁边写注释说明原因。

- [ ] **Step 6: 运行全部测试和 clippy**

Run: `cargo test -p dblens-driver-postgres && cargo clippy -p dblens-driver-postgres --all-targets -- -D warnings`
Expected: 全部 PASS，没有警告。

- [ ] **Step 7: 提交**

```bash
git add crates/dblens-driver-postgres
git commit -m "feat(postgres): PostgreSQL 驱动连接、流式执行、notice、取消与手动事务" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 10: 持续集成

**Files:**
- Create: `.github/workflows/ci.yml`

**Interfaces:**
- Consumes: 全部 crate，以及 Task 5 中的环境变量约定
- Produces: 每次 push 或 PR 时自动执行检查

- [ ] **Step 1: 创建工作流**

`.github/workflows/ci.yml`：
```yaml
name: ci

on:
  push:
    branches: [main]
  pull_request:

env:
  CARGO_TERM_COLOR: always

jobs:
  check:
    name: 检查（${{ matrix.os }}）
    strategy:
      fail-fast: false
      matrix:
        os: [windows-latest, macos-latest, ubuntu-latest]
    runs-on: ${{ matrix.os }}
    steps:
      - uses: actions/checkout@v4
      - name: 安装工具链（读取 rust-toolchain.toml）
        run: rustup show
      - uses: Swatinem/rust-cache@v2
      - run: cargo fmt --all --check
      - run: cargo clippy --workspace --all-targets -- -D warnings
      - run: cargo test --workspace

  integration:
    name: 驱动集成测试
    runs-on: ubuntu-latest
    services:
      mysql:
        image: mysql:8.4
        env:
          MYSQL_ROOT_PASSWORD: dblens
          MYSQL_DATABASE: dblens_test
        ports: ["3306:3306"]
        options: >-
          --health-cmd "mysqladmin ping -h 127.0.0.1 -pdblens"
          --health-interval 5s --health-timeout 5s --health-retries 20
      mariadb:
        image: mariadb:11.4
        env:
          MARIADB_ROOT_PASSWORD: dblens
          MARIADB_DATABASE: dblens_test
        ports: ["3307:3306"]
        options: >-
          --health-cmd "healthcheck.sh --connect --innodb_initialized"
          --health-interval 5s --health-timeout 5s --health-retries 20
      postgres:
        image: postgres:17
        env:
          POSTGRES_PASSWORD: dblens
          POSTGRES_DB: dblens_test
        ports: ["5432:5432"]
        options: >-
          --health-cmd "pg_isready -U postgres"
          --health-interval 5s --health-timeout 5s --health-retries 20
    env:
      DBLENS_TEST_MYSQL_URL: mysql://root:dblens@127.0.0.1:3306/dblens_test
      DBLENS_TEST_MARIADB_URL: mysql://root:dblens@127.0.0.1:3307/dblens_test
      DBLENS_TEST_PG_URL: postgres://postgres:dblens@127.0.0.1:5432/dblens_test
    steps:
      - uses: actions/checkout@v4
      - run: rustup show
      - uses: Swatinem/rust-cache@v2
      - name: 运行驱动一致性测试（不允许跳过）
        run: |
          cargo test -p dblens-driver-mysql -p dblens-driver-postgres --test conformance -- --nocapture 2>&1 | tee test.log
          if grep -q "跳过" test.log; then echo "有测试被跳过，环境变量未生效"; exit 1; fi
```

- [ ] **Step 2: 本地预检**

Run: `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`
Expected: 全部通过（集成测试按本机环境变量执行或跳过）。如果 fmt 检查失败，先运行 `cargo fmt --all` 再检查。

- [ ] **Step 3: 提交**

```bash
git add .github
git commit -m "ci: 三平台检查与 MySQL/MariaDB/PG 驱动集成测试" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

> 本仓库还没有远程地址。推送到 GitHub 的时机由用户决定，不在本计划内执行。
