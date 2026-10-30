# M9a 数据库账号与权限（只读）实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在 DBLens 里只读地查看各数据库的用户、角色、直接授权、继承后的实际权限，并能从对象反查谁能访问。

**Architecture:** `dblens-dialect` 新增 `privilege` 模块：按方言的 SQL 模板输出统一列名，纯函数解析成模型并计算实际权限。`dblens-service` 经 `MetadataService::run` 跳过缓存取数，主查询报 SQL 错误时退到 `@limited` 段并标记范围不完整。`dblens-api` 提供三个 GET 接口，前端新增一种标签页。

**Tech Stack:** Rust（minijinja 模板、serde、axum）、React + antd + TanStack Query、vitest。

**Spec:** `docs/superpowers/specs/2026-10-30-m9-privileges-design.md`。本计划只实现其中 M9a。

## Global Constraints

- 只读：M9a 不生成、不执行任何写语句；实连测试的夹具只在 VM 测试库上建，结束时清理。
- 用户的真实达梦实例只做只读测试，不跑本计划的夹具。
- 权限名保留各库原文，不映射成统一枚举。
- 读操作不进执行历史，不写元数据缓存的长期结果（每次先 `forget`）。
- 读不到的段落不能显示成“没有授权”，必须进 `Coverage::Partial`。
- 界面措辞用“按字典计算”，不写“确定有权限”。
- 界面沿用现有布局和黑白基调，不新增活动栏图标，不用彩色标签和大按钮。
- 新词条中英文键必须一致（`locales.test.ts` 检查）。
- 实连测试串行：`--test-threads=1`，不加 `--include-ignored`。
- 远程命令一律有硬超时（`timeout -k`）。
- 不跑 GitHub CI，本地验证；未经用户同意不推送。
- 提交身份按仓库配置（ThinkBuddy + noreply 邮箱）。

## 执行环境

- 新建分支 `feat/m9a-privileges` 和工作树 `E:\VibeCoding\16数据库管理工具\dblens-m9a`，`CARGO_TARGET_DIR` 用独立目录 `E:\VibeCoding\16数据库管理工具\target-m9a`，不与其他克隆共用。
- `CARGO_INCREMENTAL=0`。在仓库里运行 cargo 时带 `--config "source.mirror.registry='sparse+https://rsproxy.cn/index/'"`（clippy 放在 `clippy` 之后）。
- 前端用 `D:\Environments\nvm4w\nodejs` 下的 pnpm 11。
- M8a 还在分支上，GBase 8s 不在本计划内。两边都改 `Workspace.tsx` 以外的文件很少，合并顺序不限。

## 规划时的实测结果

模板在 VM 上按 `{{ bind }}` 替换为字面量后逐段跑过（只读）：

| 目标 | 版本 | 结果 |
|---|---|---|
| MySQL | 9.7.4 | 全部段落通过 |
| MariaDB | 12.3.3 | 通过；`mysql.user` 是视图，没有 `account_locked`，改用 `global_priv` 的 JSON |
| TiDB | v8.5.8 | 通过；没有 `procs_priv` |
| OceanBase CE | 4.4.2.1 | 通过；没有 `mysql.tables_priv`，用 `information_schema` |
| PostgreSQL | 18.6 | 通过；PG16 起 `inherit_option` 按成员关系存放 |
| 金仓 | V009R002C016，Oracle 和 PG 模式 | 通过，直接用 PG 模板，`pg_auth_members` 没有 `inherit_option` |
| openGauss | 7.0.0-RC1，PG 和 A 模式 | 通过 |
| Oracle | 23.26.3 | 全部段落通过 |
| SQL Server | 2022 | 通过，2025 留到 Task 5 |
| 达梦 | | **未实连**，系统账号密码没有保存，Task 5 第一步修正模板 |

实测只覆盖 `admin` 或 sysdba 这类高权限账号；`@limited` 段的语法已执行，但低权限下的行为在 Task 5 才验证。

---

### Task 0：依赖审查

**Files:**
- Modify: `Cargo.toml`、`Cargo.lock`、`web/package.json`、`web/pnpm-lock.yaml`（有可升级项时）

- [ ] **Step 1: 建工作树并运行依赖检查**

```bash
git worktree add -b feat/m9a-privileges ../dblens-m9a main
NODE_USE_ENV_PROXY=1 node scripts/check-deps.mjs
```

Expected: 列出可升级依赖。显示 0 项且没设 `NODE_USE_ENV_PROXY=1` 属于误报。

- [ ] **Step 2: 按项目规则升级**

只取稳定版，发布满 7 天。升级后运行：

```bash
cargo test --workspace --offline
cd web && pnpm test && pnpm exec tsc --noEmit
```

Expected: 全部通过。

- [ ] **Step 3: 提交**

```bash
git add -A
git commit -m "chore: M9a 开始前升级依赖"
```

没有可升级项时跳过本步。

---

### Task 1：模型、模板加载和行解析

**Files:**
- Create: `crates/dblens-dialect/sql/privilege/{mysql,mariadb,tidb,oceanbase,postgres,opengauss,oracle,dameng,sqlserver}.sql`（从 `docs/superpowers/plans/2026-10-30-m9a-privileges-materials/sql/` 原样复制）
- Create: `crates/dblens-dialect/src/privilege/mod.rs`
- Create: `crates/dblens-dialect/src/privilege/rows.rs`
- Modify: `crates/dblens-dialect/src/lib.rs`
- Modify: `crates/dblens-dialect/src/meta/mod.rs`

**Interfaces:**
- Produces: `dblens_dialect::privilege::{Principal, PrincipalKind, PrincipalLevel, Membership, PrivilegeGrant, GrantScope, GrantEffect, GrantTarget, PrivSection, PrivTier, principals, memberships, grants, caps, Caps}`；`Dialect::privilege_query(section, tier, &MetaArgs) -> Result<Option<MetaQuery>>`；`Dialect::public_grantees() -> &'static [&'static str]`；`MetaKey::{PrivCaps, PrivPrincipals, PrivMemberships, PrivGrantsOf, PrivGrantsOn}`。

- [ ] **Step 1: 写失败的测试**

`crates/dblens-dialect/src/privilege/mod.rs` 末尾：

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::meta::template::ALL_VARIANTS;
    use crate::{Family, Variant};

    fn args() -> MetaArgs {
        MetaArgs { schema: Some("s".into()), table: Some("t".into()), name: Some("u".into()), ..Default::default() }
    }

    #[test]
    fn every_variant_has_the_required_sections() {
        for &v in &ALL_VARIANTS {
            let d = Dialect::new(v);
            for s in [PrivSection::Principals, PrivSection::Memberships, PrivSection::GrantsOf, PrivSection::GrantsOn] {
                let q = d.privilege_query(s, PrivTier::Full, &args()).unwrap();
                assert!(q.is_some_and(|q| !q.sql.is_empty() && !q.sql.contains("-- [")), "{v:?} {s:?}");
            }
        }
    }

    #[test]
    fn overrides_replace_or_remove_sections() {
        let caps = |v| Dialect::new(v).privilege_query(PrivSection::Caps, PrivTier::Full, &args()).unwrap();
        assert!(caps(Variant::MySql).is_some());
        assert!(caps(Variant::SqlServer).is_some());
        assert!(caps(Variant::MariaDb).is_none(), "MariaDB 没有 partial_revokes");
        assert!(caps(Variant::Postgres).is_none());
        let of = |v| {
            Dialect::new(v).privilege_query(PrivSection::GrantsOf, PrivTier::Full, &args()).unwrap().unwrap().sql
        };
        assert!(of(Variant::MySql).contains("procs_priv"));
        assert!(!of(Variant::TiDb).contains("procs_priv"));
        assert!(of(Variant::OpenGauss).contains("(x.acl)"));
        assert_eq!(of(Variant::Kingbase), of(Variant::Postgres));
    }

    #[test]
    fn limited_tier_is_a_separate_section() {
        let d = Dialect::new(Variant::Oracle);
        let q = d.privilege_query(PrivSection::Principals, PrivTier::Limited, &args()).unwrap().unwrap();
        assert!(q.sql.contains("all_users"));
        assert!(Dialect::new(Variant::Postgres)
            .privilege_query(PrivSection::Principals, PrivTier::Limited, &args())
            .unwrap()
            .is_none());
    }

    #[test]
    fn parameters_are_bound_and_version_gates_apply() {
        let d = Dialect::new(Variant::Postgres).with_version(Some(150000));
        let q = d.privilege_query(PrivSection::Memberships, PrivTier::Full, &args()).unwrap().unwrap();
        assert!(q.sql.contains("rolinherit") && !q.sql.contains("inherit_option"));
        let d = Dialect::new(Variant::Postgres).with_version(Some(160000));
        let q = d.privilege_query(PrivSection::Memberships, PrivTier::Full, &args()).unwrap().unwrap();
        assert!(q.sql.contains("inherit_option"));
        let q = d.privilege_query(PrivSection::GrantsOf, PrivTier::Full, &args()).unwrap().unwrap();
        assert!(q.sql.contains("$1"));
        assert_eq!(q.params, vec![dblens_core::Value::String("u".into())]);
    }

    #[test]
    fn public_grantees_follow_the_family() {
        assert_eq!(Dialect::new(Variant::Postgres).public_grantees(), ["PUBLIC"]);
        assert_eq!(Dialect::new(Variant::Oracle).public_grantees(), ["PUBLIC"]);
        assert_eq!(Dialect::new(Variant::MySql).public_grantees(), [] as [&str; 0]);
        assert_eq!(Dialect::new(Variant::MariaDb).public_grantees(), ["'PUBLIC'"]);
        assert_eq!(Dialect::new(Variant::SqlServer).public_grantees(), ["db:public", "server:public"]);
        assert_eq!(Dialect::new(Variant::SqlServer).family(), Family::SqlServer);
    }
}
```

`crates/dblens-dialect/src/privilege/rows.rs` 末尾：

```rust
#[cfg(test)]
mod tests {
    use dblens_core::Value;

    use super::super::*;
    use crate::meta::MetaRows;

    fn s(v: &str) -> Value {
        Value::String(v.into())
    }

    #[test]
    fn principals_keep_nulls_and_collect_attrs() {
        let cols = ["id", "name", "host", "principal_kind", "principal_level", "can_login", "locked", "expires_at", "builtin", "attr_plugin"];
        let rows = MetaRows::new(
            cols.iter().map(|c| c.to_string()).collect(),
            vec![vec![s("'u'@'%'"), s("u"), s("%"), s("user"), s("server"), Value::Int(1), s("N"), Value::Null, Value::Int(0), s("x")]],
        );
        let p = principals(&rows).unwrap().remove(0);
        assert_eq!((p.kind, p.level), (PrincipalKind::User, PrincipalLevel::Server));
        assert_eq!((p.can_login, p.locked, p.builtin), (Some(true), Some(false), false));
        assert_eq!(p.expires_at, None);
        assert_eq!(p.attrs, vec![("plugin".to_string(), "x".to_string())]);
    }

    #[test]
    fn grants_accept_each_spelling_of_grantable() {
        let cols = ["grantee", "grant_scope", "db_name", "schema_name", "object_kind", "object_name", "column_name", "privilege", "effect", "grantable", "grantor"];
        let row = |g: Value| vec![s("u"), s("object"), Value::Null, s("hr"), s("table"), s("emp"), Value::Null, s("SELECT"), s("grant"), g, Value::Null];
        let rows = MetaRows::new(
            cols.iter().map(|c| c.to_string()).collect(),
            vec![row(s("YES")), row(s("NO")), row(Value::Bool(true)), row(Value::Int(0))],
        );
        let g = grants(&rows).unwrap();
        assert_eq!(g.iter().map(|x| x.grantable).collect::<Vec<_>>(), [true, false, true, false]);
        assert_eq!(g[0].scope, GrantScope::Object);
        assert_eq!(g[0].target.schema.as_deref(), Some("hr"));
    }

    #[test]
    fn unknown_kind_is_reported() {
        let rows = MetaRows::new(
            ["id", "name", "host", "principal_kind", "principal_level", "can_login", "locked", "expires_at", "builtin"].iter().map(|c| c.to_string()).collect(),
            vec![vec![s("a"), s("a"), Value::Null, s("alien"), s("server"), Value::Null, Value::Null, Value::Null, Value::Int(0)]],
        );
        assert!(principals(&rows).is_err());
    }

    #[test]
    fn caps_default_to_full_view() {
        assert_eq!(caps(None), Caps { full_view: true, partial_revokes: false });
        let rows = MetaRows::new(vec!["full_view".into()], vec![vec![Value::Int(0)]]);
        assert_eq!(caps(Some(&rows)), Caps { full_view: false, partial_revokes: false });
        let rows = MetaRows::new(vec!["full_view".into(), "partial_revokes".into()], vec![vec![Value::Int(1), Value::Int(1)]]);
        assert_eq!(caps(Some(&rows)), Caps { full_view: true, partial_revokes: true });
    }
}
```

- [ ] **Step 2: 确认测试失败**

Run: `cargo test -p dblens-dialect privilege --offline`
Expected: 编译失败，`privilege` 模块不存在。

- [ ] **Step 3: 增加 MetaKey**

在 `crates/dblens-dialect/src/meta/mod.rs` 的 `MetaKey` 枚举末尾（`ViewDefinitionBasic,` 之后）加：

```rust
    // 以下是账号与权限（M9a），模板在 sql/privilege/，由 `Dialect::privilege_query` 生成。不在 `ALL` 中。
    PrivCaps,
    PrivPrincipals,
    PrivMemberships,
    PrivGrantsOf,
    PrivGrantsOn,
```

在 `as_str` 的 `match` 里加：

```rust
            MetaKey::PrivCaps => "priv_caps",
            MetaKey::PrivPrincipals => "priv_principals",
            MetaKey::PrivMemberships => "priv_memberships",
            MetaKey::PrivGrantsOf => "priv_grants_of",
            MetaKey::PrivGrantsOn => "priv_grants_on",
```

- [ ] **Step 4: 写 `privilege/mod.rs`**

复制素材 SQL 到 `crates/dblens-dialect/sql/privilege/`，然后写：

```rust
//! 数据库账号与权限（M9 设计文档）。模板在 `sql/privilege/<方言>.sql`，按 `-- [段]` 分段，段名带 `@limited` 的
//! 是低权限时的退路。模板直接输出统一的列名，这里只做解析和计算，不做 IO。
//!
//! 列名约定：
//! - principals：id、name、host、principal_kind（user/role/login/group）、principal_level（server/database）、
//!   can_login、locked、expires_at、builtin，以及任意个 `attr_*`（原样展示的方言属性）；
//! - memberships：member_id、role_id、admin_option、inherits、default_role；
//! - grants_of、grants_on：grantee、grant_scope（global/database/schema/object/column）、db_name、schema_name、
//!   object_kind、object_name、column_name、privilege、effect（grant/deny/implicit）、grantable、grantor；
//! - caps（可选）：full_view、partial_revokes。

mod effective;
mod rows;

use std::collections::HashMap;
use std::sync::OnceLock;

use dblens_core::Result;
use serde::{Deserialize, Serialize};

pub use effective::{
    EffectiveContext, EffectivePrivilege, EffectiveState, ObjectAccess, PrivNote, Reach, effective, reachable,
    who_can_access,
};
pub use rows::{Caps, caps, grants, memberships, principals};

use crate::meta::template::{ALL_VARIANTS, parse_sections};
use crate::meta::{MetaArgs, MetaKey, MetaQuery, render};
use crate::{Dialect, Family, Variant};

const MYSQL: &str = include_str!("../../sql/privilege/mysql.sql");
const MARIADB: &str = include_str!("../../sql/privilege/mariadb.sql");
const TIDB: &str = include_str!("../../sql/privilege/tidb.sql");
const OCEANBASE: &str = include_str!("../../sql/privilege/oceanbase.sql");
const POSTGRES: &str = include_str!("../../sql/privilege/postgres.sql");
const OPENGAUSS: &str = include_str!("../../sql/privilege/opengauss.sql");
const ORACLE: &str = include_str!("../../sql/privilege/oracle.sql");
const DAMENG: &str = include_str!("../../sql/privilege/dameng.sql");
const SQLSERVER: &str = include_str!("../../sql/privilege/sqlserver.sql");

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrincipalKind {
    User,
    Role,
    Login,
    Group,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrincipalLevel {
    Server,
    Database,
}

/// 用户、角色或登录名。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Principal {
    /// 唯一标识：MySQL 系是 `'user'@'host'`，SQL Server 带 `server:`、`db:` 前缀，其他库是名字。
    pub id: String,
    pub name: String,
    pub host: Option<String>,
    pub kind: PrincipalKind,
    pub level: PrincipalLevel,
    pub can_login: Option<bool>,
    pub locked: Option<bool>,
    pub expires_at: Option<String>,
    /// 系统内置的账号或角色，界面默认不列。
    pub builtin: bool,
    /// 方言特有的属性，原样展示。
    pub attrs: Vec<(String, String)>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Membership {
    pub member: String,
    pub role: String,
    pub admin_option: bool,
    /// PG 系：这条成员关系是否自动继承权限。
    pub inherits: Option<bool>,
    /// Oracle、MySQL：登录时是否默认启用。
    pub default_role: Option<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GrantScope {
    Global,
    Database,
    Schema,
    Object,
    Column,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GrantEffect {
    Grant,
    Deny,
    /// 不是授权记录：属主、超级用户、固定角色隐含的权限。
    Implicit,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct GrantTarget {
    pub database: Option<String>,
    pub schema: Option<String>,
    pub object_kind: Option<String>,
    pub object: Option<String>,
    pub column: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PrivilegeGrant {
    pub grantee: String,
    pub scope: GrantScope,
    pub target: GrantTarget,
    /// 各库的原文。
    pub privilege: String,
    pub effect: GrantEffect,
    pub grantable: bool,
    pub grantor: Option<String>,
}

/// 模板的段。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrivSection {
    Caps,
    Principals,
    Memberships,
    GrantsOf,
    GrantsOn,
}

/// 主查询，或低权限时的退路。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrivTier {
    Full,
    Limited,
}

impl PrivSection {
    pub fn key(self) -> MetaKey {
        match self {
            Self::Caps => MetaKey::PrivCaps,
            Self::Principals => MetaKey::PrivPrincipals,
            Self::Memberships => MetaKey::PrivMemberships,
            Self::GrantsOf => MetaKey::PrivGrantsOf,
            Self::GrantsOn => MetaKey::PrivGrantsOn,
        }
    }

    fn name(self, tier: PrivTier) -> &'static str {
        match (self, tier) {
            (Self::Caps, PrivTier::Full) => "caps",
            (Self::Caps, PrivTier::Limited) => "caps@limited",
            (Self::Principals, PrivTier::Full) => "principals",
            (Self::Principals, PrivTier::Limited) => "principals@limited",
            (Self::Memberships, PrivTier::Full) => "memberships",
            (Self::Memberships, PrivTier::Limited) => "memberships@limited",
            (Self::GrantsOf, PrivTier::Full) => "grants_of",
            (Self::GrantsOf, PrivTier::Limited) => "grants_of@limited",
            (Self::GrantsOn, PrivTier::Full) => "grants_on",
            (Self::GrantsOn, PrivTier::Limited) => "grants_on@limited",
        }
    }
}

fn base_file(family: Family) -> &'static str {
    match family {
        Family::MySql => MYSQL,
        Family::Postgres => POSTGRES,
        Family::Oracle => ORACLE,
        Family::Dameng => DAMENG,
        Family::SqlServer => SQLSERVER,
    }
}

fn override_file(variant: Variant) -> Option<&'static str> {
    match variant {
        Variant::MariaDb => Some(MARIADB),
        Variant::TiDb => Some(TIDB),
        Variant::OceanBase => Some(OCEANBASE),
        Variant::OpenGauss | Variant::GaussDb => Some(OPENGAUSS),
        _ => None,
    }
}

/// 变体的全部段：家族模板，再用覆盖文件替换；覆盖文件里正文为空的段表示没有这种查询。
fn sections(variant: Variant) -> &'static HashMap<String, String> {
    static CACHE: OnceLock<HashMap<Variant, HashMap<String, String>>> = OnceLock::new();
    let all = CACHE.get_or_init(|| {
        ALL_VARIANTS
            .iter()
            .map(|&v| {
                let mut m = parse_sections(base_file(v.family()));
                if let Some(o) = override_file(v) {
                    m.extend(parse_sections(o));
                    m.retain(|_, body| !body.is_empty());
                }
                (v, m)
            })
            .collect()
    });
    &all[&variant]
}

impl Dialect {
    /// 生成账号与权限的查询；该方言没有这一段时返回 None。参数：`grants_of` 用 name（主体 id），
    /// `grants_on` 用 schema、table。
    pub fn privilege_query(&self, section: PrivSection, tier: PrivTier, args: &MetaArgs) -> Result<Option<MetaQuery>> {
        match sections(self.variant()).get(section.name(tier)) {
            None => Ok(None),
            Some(t) => {
                let version = self.version().unwrap_or(u32::MAX);
                render(section.key(), t, self.family(), version, self.oracle_mode(), args).map(Some)
            }
        }
    }

    /// 所有账号都隐含拥有其权限的主体。
    pub fn public_grantees(&self) -> &'static [&'static str] {
        match (self.family(), self.variant()) {
            (Family::SqlServer, _) => &["db:public", "server:public"],
            (Family::MySql, Variant::MariaDb) => &["'PUBLIC'"],
            (Family::MySql, _) => &[],
            _ => &["PUBLIC"],
        }
    }
}
```

在 `crates/dblens-dialect/src/lib.rs` 加 `pub mod privilege;`（按字母序放在 `mod ops;` 之后）。

- [ ] **Step 5: 写 `privilege/rows.rs`**

```rust
//! 把模板的结果行解析成模型。

use dblens_core::{Error, Result, Value, msg};

use super::{
    GrantEffect, GrantScope, GrantTarget, Membership, Principal, PrincipalKind, PrincipalLevel, PrivilegeGrant,
};
use crate::meta::{MetaRow, MetaRows};

/// 宽松的是非值：各库分别返回 bool、0/1、Y/N、YES/NO、t/f。
fn yes(row: &MetaRow<'_>, column: &str) -> Option<bool> {
    if matches!(row.get(column), Value::Null) {
        return None;
    }
    let text = row.str(column)?;
    Some(matches!(text.trim().to_ascii_uppercase().as_str(), "Y" | "YES" | "T" | "TRUE" | "1"))
}

fn bad(column: &str, value: &str) -> Error {
    Error::internal(msg!("privilege.bad_value", column = column, value = value))
}

pub fn principals(rows: &MetaRows) -> Result<Vec<Principal>> {
    rows.require(
        "principals",
        &["id", "name", "host", "principal_kind", "principal_level", "can_login", "locked", "expires_at", "builtin"],
    )?;
    rows.iter()
        .map(|r| {
            let kind = match r.name("principal_kind").as_str() {
                "user" => PrincipalKind::User,
                "role" => PrincipalKind::Role,
                "login" => PrincipalKind::Login,
                "group" => PrincipalKind::Group,
                other => return Err(bad("principal_kind", other)),
            };
            let level = match r.name("principal_level").as_str() {
                "server" => PrincipalLevel::Server,
                "database" => PrincipalLevel::Database,
                other => return Err(bad("principal_level", other)),
            };
            let attrs = r
                .columns()
                .iter()
                .filter_map(|c| {
                    let key = c.strip_prefix("attr_")?;
                    Some((key.to_string(), r.text(c)?))
                })
                .collect();
            Ok(Principal {
                id: r.name("id"),
                name: r.name("name"),
                host: r.text("host"),
                kind,
                level,
                can_login: yes(&r, "can_login"),
                locked: yes(&r, "locked"),
                expires_at: r.text("expires_at"),
                builtin: yes(&r, "builtin").unwrap_or(false),
                attrs,
            })
        })
        .collect()
}

pub fn memberships(rows: &MetaRows) -> Result<Vec<Membership>> {
    rows.require("memberships", &["member_id", "role_id", "admin_option", "inherits", "default_role"])?;
    Ok(rows
        .iter()
        .map(|r| Membership {
            member: r.name("member_id"),
            role: r.name("role_id"),
            admin_option: yes(&r, "admin_option").unwrap_or(false),
            inherits: yes(&r, "inherits"),
            default_role: yes(&r, "default_role"),
        })
        .collect())
}

pub fn grants(rows: &MetaRows) -> Result<Vec<PrivilegeGrant>> {
    rows.require(
        "grants",
        &["grantee", "grant_scope", "db_name", "schema_name", "object_kind", "object_name", "column_name", "privilege", "effect", "grantable", "grantor"],
    )?;
    rows.iter()
        .map(|r| {
            let scope = match r.name("grant_scope").as_str() {
                "global" => GrantScope::Global,
                "database" => GrantScope::Database,
                "schema" => GrantScope::Schema,
                "object" => GrantScope::Object,
                "column" => GrantScope::Column,
                other => return Err(bad("grant_scope", other)),
            };
            let effect = match r.name("effect").as_str() {
                "grant" => GrantEffect::Grant,
                "deny" => GrantEffect::Deny,
                "implicit" => GrantEffect::Implicit,
                other => return Err(bad("effect", other)),
            };
            Ok(PrivilegeGrant {
                grantee: r.name("grantee"),
                scope,
                target: GrantTarget {
                    database: r.text("db_name"),
                    schema: r.text("schema_name"),
                    object_kind: r.text("object_kind"),
                    object: r.text("object_name"),
                    column: r.text("column_name"),
                },
                privilege: r.name("privilege"),
                effect,
                grantable: yes(&r, "grantable").unwrap_or(false),
                grantor: r.text("grantor"),
            })
        })
        .collect()
}

/// `caps` 段的结果。没有这一段时范围完整、没有 partial revokes。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Caps {
    /// 能看到其他账号。SQL Server 的低权限账号查目录视图不报错，只是看不到别人。
    pub full_view: bool,
    pub partial_revokes: bool,
}

pub fn caps(rows: Option<&MetaRows>) -> Caps {
    let row = rows.and_then(|r| r.iter().next());
    Caps {
        full_view: row.as_ref().and_then(|r| yes(r, "full_view")).unwrap_or(true),
        partial_revokes: row.as_ref().and_then(|r| yes(r, "partial_revokes")).unwrap_or(false),
    }
}
```

确认 `meta/mod.rs` 的 `pub use rows::{MetaRow, MetaRows};` 已导出 `MetaRow`（已导出）。

在 `crates/dblens-core/locales/app.json` 里按键的字母序加：

```json
  "privilege.bad_value": {
    "zh-CN": "账号与权限查询返回了不认识的值：%{column} = %{value}",
    "en": "Unexpected value in privilege query: %{column} = %{value}"
  },
```

- [ ] **Step 6: 先建空的 `effective.rs` 让模块编译**

Task 2 才实现。此步内容：

```rust
//! 实际权限的计算（M9 设计文档第 6 节）。

use serde::Serialize;

use super::{Membership, Principal, PrivilegeGrant};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PrivNote {
    NeedsSetRole,
    NotDefaultRole,
    PublicGrant,
    Implicit,
    AnyPrivilege,
    PartialRevokes,
    DenyOverride,
    HostWildcard,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EffectiveState {
    Allowed,
    Denied,
    Conditional,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reach {
    pub id: String,
    pub via: Vec<String>,
    pub notes: Vec<PrivNote>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EffectiveContext {
    pub deny_wins: bool,
    pub partial_revokes: bool,
    pub host_wildcard: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EffectivePrivilege {
    pub grant: PrivilegeGrant,
    pub via: Vec<String>,
    pub state: EffectiveState,
    pub notes: Vec<PrivNote>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ObjectAccess {
    pub principal: String,
    pub privilege: EffectivePrivilege,
}

pub fn reachable(_start: &str, _memberships: &[Membership], _public: &[&str]) -> Vec<Reach> {
    Vec::new()
}

pub fn effective(_reach: &[Reach], _grants: &[PrivilegeGrant], _ctx: EffectiveContext) -> Vec<EffectivePrivilege> {
    Vec::new()
}

pub fn who_can_access(
    _principals: &[Principal],
    _memberships: &[Membership],
    _grants: &[PrivilegeGrant],
    _public: &[&str],
    _ctx: EffectiveContext,
) -> Vec<ObjectAccess> {
    Vec::new()
}
```

- [ ] **Step 7: 确认测试通过**

Run: `cargo test -p dblens-dialect privilege --offline`
Expected: Step 1 的 9 个测试全部 PASS。

- [ ] **Step 8: 提交**

```bash
git add crates/dblens-dialect crates/dblens-core/locales/app.json
git commit -m "feat(dialect): M9a 账号权限模型、模板和解析"
```

---

### Task 2：实际权限计算

**Files:**
- Modify: `crates/dblens-dialect/src/privilege/effective.rs`

**Interfaces:**
- Consumes: Task 1 的模型和占位类型。
- Produces: 与 Task 1 Step 6 相同签名的 `reachable`、`effective`、`who_can_access`。

- [ ] **Step 1: 写失败的测试**

追加到 `effective.rs`：

```rust
#[cfg(test)]
mod tests {
    use super::super::{GrantEffect, GrantScope, GrantTarget, PrincipalKind, PrincipalLevel};
    use super::*;

    fn m(member: &str, role: &str) -> Membership {
        Membership { member: member.into(), role: role.into(), admin_option: false, inherits: None, default_role: None }
    }

    fn g(grantee: &str, privilege: &str) -> PrivilegeGrant {
        PrivilegeGrant {
            grantee: grantee.into(),
            scope: GrantScope::Object,
            target: GrantTarget { schema: Some("s".into()), object: Some("t".into()), ..Default::default() },
            privilege: privilege.into(),
            effect: GrantEffect::Grant,
            grantable: false,
            grantor: None,
        }
    }

    fn user(id: &str) -> Principal {
        Principal {
            id: id.into(),
            name: id.into(),
            host: None,
            kind: PrincipalKind::User,
            level: PrincipalLevel::Server,
            can_login: Some(true),
            locked: None,
            expires_at: None,
            builtin: false,
            attrs: vec![],
        }
    }

    #[test]
    fn follows_nested_roles_and_keeps_the_path() {
        let ms = [m("u", "r1"), m("r1", "r2")];
        let reach = reachable("u", &ms, &[]);
        let last = reach.iter().find(|r| r.id == "r2").unwrap();
        assert_eq!(last.via, ["u", "r1", "r2"]);
        let e = effective(&reach, &[g("r2", "SELECT")], EffectiveContext::default());
        assert_eq!(e.len(), 1);
        assert_eq!(e[0].via, ["u", "r1", "r2"]);
        assert_eq!(e[0].state, EffectiveState::Allowed);
    }

    #[test]
    fn cycles_stop_and_every_path_is_kept() {
        let ms = [m("u", "a"), m("a", "b"), m("b", "a"), m("u", "b")];
        let reach = reachable("u", &ms, &[]);
        let to_b: Vec<_> = reach.iter().filter(|r| r.id == "b").map(|r| r.via.clone()).collect();
        assert_eq!(to_b.len(), 2);
        assert!(to_b.contains(&vec!["u".to_string(), "b".to_string()]));
        assert!(to_b.contains(&vec!["u".to_string(), "a".to_string(), "b".to_string()]));
    }

    #[test]
    fn noinherit_and_non_default_roles_are_conditional() {
        let mut no = m("u", "r");
        no.inherits = Some(false);
        let e = effective(&reachable("u", &[no], &[]), &[g("r", "SELECT")], EffectiveContext::default());
        assert_eq!((e[0].state, e[0].notes.clone()), (EffectiveState::Conditional, vec![PrivNote::NeedsSetRole]));
        let mut off = m("u", "r");
        off.default_role = Some(false);
        let e = effective(&reachable("u", &[off], &[]), &[g("r", "SELECT")], EffectiveContext::default());
        assert_eq!((e[0].state, e[0].notes.clone()), (EffectiveState::Conditional, vec![PrivNote::NotDefaultRole]));
    }

    #[test]
    fn public_is_reached_and_marked() {
        let e = effective(&reachable("u", &[], &["PUBLIC"]), &[g("PUBLIC", "SELECT")], EffectiveContext::default());
        assert_eq!(e[0].via, ["u", "PUBLIC"]);
        assert_eq!(e[0].notes, vec![PrivNote::PublicGrant]);
        assert_eq!(e[0].state, EffectiveState::Allowed);
    }

    #[test]
    fn sql_server_deny_wins_except_for_sysadmin() {
        let ctx = EffectiveContext { deny_wins: true, ..Default::default() };
        let mut deny = g("r2", "SELECT");
        deny.effect = GrantEffect::Deny;
        deny.scope = GrantScope::Schema;
        deny.target = GrantTarget { schema: Some("s".into()), ..Default::default() };
        let grants = [g("r1", "SELECT"), deny.clone()];
        let e = effective(&reachable("u", &[m("u", "r1"), m("u", "r2")], &[]), &grants, ctx);
        let allow = e.iter().find(|x| x.grant.grantee == "r1").unwrap();
        assert_eq!(allow.state, EffectiveState::Denied);
        assert_eq!(allow.notes, vec![PrivNote::DenyOverride]);
        assert_eq!(e.iter().find(|x| x.grant.grantee == "r2").unwrap().state, EffectiveState::Denied);

        let e = effective(
            &reachable("u", &[m("u", "r1"), m("u", "r2"), m("u", "server:sysadmin")], &[]),
            &grants,
            ctx,
        );
        assert_eq!(e.len(), 1, "sysadmin 不受 DENY 限制，DENY 行不列");
        assert_eq!(e[0].state, EffectiveState::Allowed);
    }

    #[test]
    fn deny_on_another_object_does_not_apply() {
        let ctx = EffectiveContext { deny_wins: true, ..Default::default() };
        let mut deny = g("u", "SELECT");
        deny.effect = GrantEffect::Deny;
        deny.target.object = Some("other".into());
        let e = effective(&reachable("u", &[], &[]), &[g("u", "SELECT"), deny], ctx);
        assert_eq!(e.iter().find(|x| x.grant.effect == GrantEffect::Grant).unwrap().state, EffectiveState::Allowed);
    }

    #[test]
    fn notes_for_implicit_any_partial_and_host() {
        let mut owner = g("u", "OWNER");
        owner.effect = GrantEffect::Implicit;
        let mut any = g("u", "SELECT ANY TABLE");
        any.scope = GrantScope::Global;
        let ctx = EffectiveContext { partial_revokes: true, host_wildcard: true, ..Default::default() };
        let e = effective(&reachable("u", &[], &[]), &[owner, any], ctx);
        assert_eq!(e[0].state, EffectiveState::Allowed);
        assert_eq!(e[0].notes, vec![PrivNote::Implicit, PrivNote::HostWildcard]);
        assert_eq!(e[1].state, EffectiveState::Conditional);
        assert_eq!(e[1].notes, vec![PrivNote::AnyPrivilege, PrivNote::PartialRevokes, PrivNote::HostWildcard]);
    }

    #[test]
    fn who_can_access_expands_roles_and_lists_public_once() {
        let principals = [user("u"), user("v")];
        let ms = [m("u", "r")];
        let grants = [g("r", "SELECT"), g("PUBLIC", "SELECT"), g("gone", "INSERT")];
        let a = who_can_access(&principals, &ms, &grants, &["PUBLIC"], EffectiveContext::default());
        let u: Vec<_> = a.iter().filter(|x| x.principal == "u").collect();
        assert_eq!(u.len(), 1);
        assert_eq!(u[0].privilege.via, ["u", "r"]);
        assert!(!a.iter().any(|x| x.principal == "v"), "v 没有授权，PUBLIC 不展开到每个账号");
        let public: Vec<_> = a.iter().filter(|x| x.principal == "PUBLIC").collect();
        assert_eq!(public.len(), 1);
        assert_eq!(public[0].privilege.notes, vec![PrivNote::PublicGrant]);
        assert!(a.iter().any(|x| x.principal == "gone"), "主体列表不完整时，授权对象照样列出");
    }
}
```

- [ ] **Step 2: 确认测试失败**

Run: `cargo test -p dblens-dialect effective --offline`
Expected: 多个测试 FAIL（占位函数返回空）。

- [ ] **Step 3: 实现**

替换三个占位函数，并加辅助函数：

```rust
/// 一个主体最多保留的路径数：角色网很密时避免组合爆炸。
const MAX_PATHS: usize = 256;

/// 从 `start` 经成员关系能到的主体及路径，包含自身。遇到环时停止；同一主体的多条路径都保留。
/// `public` 是所有账号都隐含拥有的主体，排在最后。
pub fn reachable(start: &str, memberships: &[Membership], public: &[&str]) -> Vec<Reach> {
    let mut out = vec![Reach { id: start.to_string(), via: vec![start.to_string()], notes: vec![] }];
    walk(start, &mut vec![start.to_string()], &[], memberships, &mut out);
    for &p in public {
        if !out.iter().any(|r| r.id == p) {
            out.push(Reach {
                id: p.to_string(),
                via: vec![start.to_string(), p.to_string()],
                notes: vec![PrivNote::PublicGrant],
            });
        }
    }
    out
}

fn walk(id: &str, path: &mut Vec<String>, notes: &[PrivNote], memberships: &[Membership], out: &mut Vec<Reach>) {
    for m in memberships.iter().filter(|m| m.member == id) {
        if path.contains(&m.role) || out.len() >= MAX_PATHS {
            continue;
        }
        let mut edge = notes.to_vec();
        if m.inherits == Some(false) && !edge.contains(&PrivNote::NeedsSetRole) {
            edge.push(PrivNote::NeedsSetRole);
        }
        if m.default_role == Some(false) && !edge.contains(&PrivNote::NotDefaultRole) {
            edge.push(PrivNote::NotDefaultRole);
        }
        path.push(m.role.clone());
        out.push(Reach { id: m.role.clone(), via: path.clone(), notes: edge.clone() });
        walk(&m.role, path, &edge, memberships, out);
        path.pop();
    }
}

/// `deny` 的范围是否覆盖 `grant`。
fn covers(deny: &PrivilegeGrant, grant: &PrivilegeGrant) -> bool {
    use super::GrantScope::{Column, Database, Global, Object, Schema};
    if deny.privilege != grant.privilege {
        return false;
    }
    let (d, g) = (&deny.target, &grant.target);
    match deny.scope {
        Global | Database => true,
        Schema => d.schema == g.schema,
        Object => d.schema == g.schema && d.object == g.object,
        Column => d.schema == g.schema && d.object == g.object && d.column == g.column,
    }
}

/// 把经 `reach` 能拿到的授权展开成实际权限。`grants` 里不在 `reach` 中的主体被忽略。
pub fn effective(reach: &[Reach], grants: &[PrivilegeGrant], ctx: EffectiveContext) -> Vec<EffectivePrivilege> {
    let sysadmin = reach.iter().any(|r| r.id == "server:sysadmin");
    let in_reach = |g: &&PrivilegeGrant| reach.iter().any(|r| r.id == g.grantee);
    let denies: Vec<&PrivilegeGrant> =
        grants.iter().filter(in_reach).filter(|g| g.effect == GrantEffect::Deny).collect();
    let mut out = Vec::new();
    for r in reach {
        for g in grants.iter().filter(|g| g.grantee == r.id) {
            if g.effect == GrantEffect::Deny && sysadmin {
                continue;
            }
            let mut notes = r.notes.clone();
            let mut state = if r.notes.iter().any(|n| matches!(n, PrivNote::NeedsSetRole | PrivNote::NotDefaultRole)) {
                EffectiveState::Conditional
            } else {
                EffectiveState::Allowed
            };
            if g.effect == GrantEffect::Implicit {
                notes.push(PrivNote::Implicit);
            }
            if g.privilege.contains(" ANY ") {
                notes.push(PrivNote::AnyPrivilege);
            }
            if ctx.partial_revokes && g.scope == GrantScope::Global {
                notes.push(PrivNote::PartialRevokes);
                state = EffectiveState::Conditional;
            }
            if ctx.host_wildcard {
                notes.push(PrivNote::HostWildcard);
            }
            if g.effect == GrantEffect::Deny {
                state = EffectiveState::Denied;
            } else if ctx.deny_wins && !sysadmin && denies.iter().any(|d| covers(d, g)) {
                state = EffectiveState::Denied;
                notes.push(PrivNote::DenyOverride);
            }
            out.push(EffectivePrivilege { grant: g.clone(), via: r.via.clone(), state, notes });
        }
    }
    out
}

/// 谁能访问：每个主体经角色拿到的权限；PUBLIC 只列一行，不展开到每个账号；不在主体列表里的被授权者
/// （权限不足时列表不全）也照样列出。
pub fn who_can_access(
    principals: &[Principal],
    memberships: &[Membership],
    grants: &[PrivilegeGrant],
    public: &[&str],
    ctx: EffectiveContext,
) -> Vec<ObjectAccess> {
    let mut out = Vec::new();
    for p in principals.iter().filter(|p| !public.contains(&p.id.as_str())) {
        let ctx = EffectiveContext { host_wildcard: p.host.as_deref().is_some_and(|h| h.contains('%')), ..ctx };
        for privilege in effective(&reachable(&p.id, memberships, &[]), grants, ctx) {
            out.push(ObjectAccess { principal: p.id.clone(), privilege });
        }
    }
    let mut seen: Vec<&str> = Vec::new();
    for g in grants {
        let known = principals.iter().any(|p| p.id == g.grantee);
        let is_public = public.contains(&g.grantee.as_str());
        if (known && !is_public) || seen.contains(&g.grantee.as_str()) {
            continue;
        }
        seen.push(&g.grantee);
        let reach = Reach {
            id: g.grantee.clone(),
            via: vec![g.grantee.clone()],
            notes: if is_public { vec![PrivNote::PublicGrant] } else { vec![] },
        };
        for privilege in effective(&[reach], grants, ctx) {
            out.push(ObjectAccess { principal: g.grantee.clone(), privilege });
        }
    }
    out
}
```

文件头的 `use` 改为：

```rust
use serde::Serialize;

use super::{GrantEffect, GrantScope, Membership, Principal, PrivilegeGrant};
```

- [ ] **Step 4: 确认测试通过**

Run: `cargo test -p dblens-dialect privilege --offline`
Expected: 全部 PASS。

- [ ] **Step 5: 提交**

```bash
git add crates/dblens-dialect/src/privilege/effective.rs
git commit -m "feat(dialect): M9a 实际权限和对象反查计算"
```

---

### Task 3：服务层

**Files:**
- Create: `crates/dblens-service/src/privilege.rs`
- Modify: `crates/dblens-service/src/lib.rs`
- Modify: `crates/dblens-core/locales/app.json`

**Interfaces:**
- Consumes: Task 1、2 的 `dblens_dialect::privilege::*`；`MetadataService::run`、`MetadataCache::forget`、`Service::scoped_profile`。
- Produces: `Service::privilege_overview(id, catalog) -> Result<PrivilegeOverview>`、`Service::principal_privileges(id, catalog, principal) -> Result<PrincipalPrivileges>`、`Service::object_access(id, catalog, schema, object) -> Result<ObjectAccessReport>`；类型 `Coverage`。

- [ ] **Step 1: 写失败的测试**

`crates/dblens-service/src/privilege.rs` 末尾：

```rust
#[cfg(test)]
mod tests {
    use dblens_core::msg;

    use super::*;

    #[test]
    fn coverage_is_complete_only_when_nothing_is_missing() {
        assert_eq!(coverage(vec![]), Coverage::Complete);
        let c = coverage(vec![msg!("privilege.missing.principals")]);
        assert!(matches!(c, Coverage::Partial { missing } if missing.len() == 1));
    }

    #[test]
    fn host_wildcard_needs_a_percent_sign() {
        let list = |host: Option<&str>| Principal {
            id: "a".into(),
            name: "a".into(),
            host: host.map(Into::into),
            kind: PrincipalKind::User,
            level: PrincipalLevel::Server,
            can_login: None,
            locked: None,
            expires_at: None,
            builtin: false,
            attrs: vec![],
        };
        assert!(host_wildcard(&[list(Some("10.%"))], "a"));
        assert!(!host_wildcard(&[list(Some("localhost"))], "a"));
        assert!(!host_wildcard(&[list(None)], "a"));
        assert!(!host_wildcard(&[], "a"));
    }
}
```

`Service` 的取数路径没有离线替身（`FakeDriver` 不识别这些 SQL），由 Task 5 的实连测试覆盖。

- [ ] **Step 2: 确认测试失败**

Run: `cargo test -p dblens-service privilege --offline`
Expected: 编译失败，模块不存在。

- [ ] **Step 3: 实现**

```rust
//! 数据库账号与权限（M9 设计文档）：只读。读操作走元数据连接，不进执行历史；每次都从库里读，不用缓存。
//! 主查询报 SQL 错误（多半是没有权限或版本缺视图）时退到 `@limited` 段；两段都读不到的，记入 `Coverage`，
//! 不能显示成“没有授权”。

use std::sync::Arc;

use dblens_core::{Error, Msg, Result, msg};
use dblens_dialect::meta::{MetaArgs, MetaRows};
use dblens_dialect::privilege::{
    self, EffectiveContext, EffectivePrivilege, Membership, ObjectAccess, Principal, PrivSection, PrivTier,
    PrivilegeGrant,
};
use dblens_dialect::{Dialect, Family};
use serde::Serialize;

use crate::profile::Profile;
use crate::service::Service;

#[cfg(test)]
use dblens_dialect::privilege::{PrincipalKind, PrincipalLevel};

/// 读到的范围是否完整。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum Coverage {
    Complete,
    Partial { missing: Vec<Msg> },
}

fn coverage(missing: Vec<Msg>) -> Coverage {
    if missing.is_empty() { Coverage::Complete } else { Coverage::Partial { missing } }
}

/// 标签页打开时读取的内容。
#[derive(Debug, Clone, Serialize)]
pub struct PrivilegeOverview {
    pub principals: Vec<Principal>,
    pub memberships: Vec<Membership>,
    pub public: Vec<String>,
    pub coverage: Coverage,
}

/// 一个主体的直接授权和实际权限。
#[derive(Debug, Clone, Serialize)]
pub struct PrincipalPrivileges {
    pub direct: Vec<PrivilegeGrant>,
    pub effective: Vec<EffectivePrivilege>,
    pub coverage: Coverage,
}

/// 一个对象的访问者。
#[derive(Debug, Clone, Serialize)]
pub struct ObjectAccessReport {
    pub access: Vec<ObjectAccess>,
    pub coverage: Coverage,
}

fn missing_msg(section: PrivSection) -> Msg {
    match section {
        PrivSection::Caps => msg!("privilege.missing.caps"),
        PrivSection::Principals => msg!("privilege.missing.principals"),
        PrivSection::Memberships => msg!("privilege.missing.memberships"),
        PrivSection::GrantsOf => msg!("privilege.missing.grants"),
        PrivSection::GrantsOn => msg!("privilege.missing.object"),
    }
}

fn host_wildcard(principals: &[Principal], id: &str) -> bool {
    principals.iter().find(|p| p.id == id).and_then(|p| p.host.as_deref()).is_some_and(|h| h.contains('%'))
}

struct Base {
    dialect: Dialect,
    principals: Vec<Principal>,
    memberships: Vec<Membership>,
    partial_revokes: bool,
    missing: Vec<Msg>,
}

impl Base {
    fn context(&self, principal: Option<&str>) -> EffectiveContext {
        EffectiveContext {
            deny_wins: self.dialect.family() == Family::SqlServer,
            partial_revokes: self.partial_revokes,
            host_wildcard: principal.is_some_and(|id| host_wildcard(&self.principals, id)),
        }
    }
}

impl Service {
    /// 读一段：先主查询，SQL 错误时退到 `@limited`。没有这一段时返回 None，且不算缺失；两段都失败时算缺失。
    async fn privilege_rows(
        &self,
        profile: &Profile,
        section: PrivSection,
        args: &MetaArgs,
        missing: &mut Vec<Msg>,
    ) -> Result<Option<Arc<MetaRows>>> {
        let mut failed = false;
        for tier in [PrivTier::Full, PrivTier::Limited] {
            self.metadata.cache().forget(&profile.id, section.key(), args);
            match self.metadata.run(profile, section.key(), args, |d| d.privilege_query(section, tier, args)).await {
                Ok(Some(rows)) => {
                    if failed {
                        missing.push(missing_msg(section));
                    }
                    return Ok(Some(rows));
                }
                Ok(None) => break,
                Err(Error::Sql { .. }) => failed = true,
                Err(e) => return Err(e),
            }
        }
        if failed {
            missing.push(missing_msg(section));
        }
        Ok(None)
    }

    async fn privilege_base(&self, profile: &Profile) -> Result<Base> {
        let dialect = self.metadata.dialect(profile).await?;
        let args = MetaArgs::default();
        let mut missing = Vec::new();
        let caps_rows = self.privilege_rows(profile, PrivSection::Caps, &args, &mut missing).await?;
        let caps = privilege::caps(caps_rows.as_deref());
        if !caps.full_view {
            missing.push(msg!("privilege.missing.others"));
        }
        let principals = match self.privilege_rows(profile, PrivSection::Principals, &args, &mut missing).await? {
            Some(rows) => privilege::principals(&rows)?,
            None => vec![],
        };
        let memberships = match self.privilege_rows(profile, PrivSection::Memberships, &args, &mut missing).await? {
            Some(rows) => privilege::memberships(&rows)?,
            None => vec![],
        };
        Ok(Base { dialect, principals, memberships, partial_revokes: caps.partial_revokes, missing })
    }

    /// 打开标签页：主体、成员关系。
    pub async fn privilege_overview(&self, profile_id: &str, catalog: Option<&str>) -> Result<PrivilegeOverview> {
        let profile = self.scoped_profile(profile_id, catalog)?;
        let base = self.privilege_base(&profile).await?;
        Ok(PrivilegeOverview {
            public: base.dialect.public_grantees().iter().map(|s| s.to_string()).collect(),
            principals: base.principals,
            memberships: base.memberships,
            coverage: coverage(base.missing),
        })
    }

    /// 一个主体的直接授权和实际权限。
    pub async fn principal_privileges(
        &self,
        profile_id: &str,
        catalog: Option<&str>,
        principal: &str,
    ) -> Result<PrincipalPrivileges> {
        let profile = self.scoped_profile(profile_id, catalog)?;
        let mut base = self.privilege_base(&profile).await?;
        let ctx = base.context(Some(principal));
        let reach = privilege::reachable(principal, &base.memberships, base.dialect.public_grantees());
        let mut ids: Vec<&str> = Vec::new();
        for r in &reach {
            if !ids.contains(&r.id.as_str()) {
                ids.push(&r.id);
            }
        }
        let mut grants = Vec::new();
        let mut group_missing = Vec::new();
        for id in ids {
            let args = MetaArgs { name: Some(id.to_string()), ..Default::default() };
            if let Some(rows) = self.privilege_rows(&profile, PrivSection::GrantsOf, &args, &mut group_missing).await? {
                grants.extend(privilege::grants(&rows)?);
            }
        }
        group_missing.dedup();
        base.missing.extend(group_missing);
        Ok(PrincipalPrivileges {
            direct: grants.iter().filter(|g| g.grantee == principal).cloned().collect(),
            effective: privilege::effective(&reach, &grants, ctx),
            coverage: coverage(base.missing),
        })
    }

    /// 谁能访问这个对象。
    pub async fn object_access(
        &self,
        profile_id: &str,
        catalog: Option<&str>,
        schema: &str,
        object: &str,
    ) -> Result<ObjectAccessReport> {
        let profile = self.scoped_profile(profile_id, catalog)?;
        let mut base = self.privilege_base(&profile).await?;
        let args = MetaArgs { schema: Some(schema.into()), table: Some(object.into()), ..Default::default() };
        let grants = match self.privilege_rows(&profile, PrivSection::GrantsOn, &args, &mut base.missing).await? {
            Some(rows) => privilege::grants(&rows)?,
            None => vec![],
        };
        let access = privilege::who_can_access(
            &base.principals,
            &base.memberships,
            &grants,
            base.dialect.public_grantees(),
            base.context(None),
        );
        Ok(ObjectAccessReport { access, coverage: coverage(base.missing) })
    }
}
```

`crates/dblens-service/src/lib.rs` 加 `mod privilege;`（放在 `mod policy;` 之后）和 `pub use privilege::{Coverage, ObjectAccessReport, PrincipalPrivileges, PrivilegeOverview};`。

`crates/dblens-core/locales/app.json` 按 `{ "zh-CN": …, "en": … }` 的格式、键的字母序加：

| 键 | 中文 | 英文 |
|---|---|---|
| `privilege.missing.caps` | 无法判断当前账号的查看范围 | Could not determine this account's visibility |
| `privilege.missing.principals` | 用户和角色列表不完整 | The list of users and roles is incomplete |
| `privilege.missing.memberships` | 角色成员关系读取不到 | Role memberships could not be read |
| `privilege.missing.grants` | 部分授权读取不到 | Some grants could not be read |
| `privilege.missing.object` | 这个对象的授权读取不到 | Grants on this object could not be read |
| `privilege.missing.others` | 当前账号看不到其他账号的信息 | This account cannot see other accounts |

- [ ] **Step 4: 确认测试通过**

Run: `cargo test -p dblens-service privilege --offline`
Expected: 2 个测试 PASS。

Run: `cargo clippy -p dblens-service -p dblens-dialect --offline -- -D warnings`
Expected: 无警告。

- [ ] **Step 5: 提交**

```bash
git add crates/dblens-service crates/dblens-core/locales/app.json
git commit -m "feat(service): M9a 账号权限读取和范围标记"
```

---

### Task 4：接口

**Files:**
- Create: `crates/dblens-api/src/privileges.rs`
- Modify: `crates/dblens-api/src/lib.rs`
- Test: `crates/dblens-api/tests/api.rs`

**Interfaces:**
- Consumes: Task 3 的 `Service` 方法。
- Produces: `GET /api/profiles/{id}/privileges?catalog=`、`GET /api/profiles/{id}/privileges/principal?principal=&catalog=`、`GET /api/profiles/{id}/privileges/object?schema=&object=&catalog=`。

- [ ] **Step 1: 写失败的测试**

在 `crates/dblens-api/tests/api.rs` 的 `ops_follow_language` 后面加：

```rust
#[tokio::test]
async fn privileges_check_parameters() {
    let (app, _) = app();
    let (_, p) =
        call(&app, "POST", "/api/profiles", Some(json!({"profile": profile_json("p"), "password": "pw"})))
            .await;
    let id = p["id"].as_str().unwrap();
    let uri = format!("/api/profiles/{id}/privileges/principal");
    assert_eq!(call(&app, "GET", &uri, None).await.0, StatusCode::BAD_REQUEST, "缺 principal 参数");
    let uri = format!("/api/profiles/{id}/privileges/object?schema=s");
    assert_eq!(call(&app, "GET", &uri, None).await.0, StatusCode::BAD_REQUEST, "缺 object 参数");
    let uri = "/api/profiles/none/privileges";
    assert_eq!(call(&app, "GET", uri, None).await.0, StatusCode::NOT_FOUND);
}
```

- [ ] **Step 2: 确认测试失败**

Run: `cargo test -p dblens-api privileges --offline`
Expected: FAIL，路由不存在时返回 404 而不是 400。

- [ ] **Step 3: 实现**

```rust
//! 数据库账号与权限接口（M9 设计文档第 7 节）。只读。

use axum::Json;
use axum::extract::{Path, Query, State};
use dblens_service::{ObjectAccessReport, PrincipalPrivileges, PrivilegeOverview};
use serde::Deserialize;

use crate::AppState;
use crate::error::ApiResult;

#[derive(Deserialize)]
pub struct CatalogQuery {
    catalog: Option<String>,
}

#[derive(Deserialize)]
pub struct PrincipalQuery {
    principal: String,
    catalog: Option<String>,
}

#[derive(Deserialize)]
pub struct ObjectQuery {
    schema: String,
    object: String,
    catalog: Option<String>,
}

pub async fn overview(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<CatalogQuery>,
) -> ApiResult<Json<PrivilegeOverview>> {
    Ok(Json(s.service.privilege_overview(&id, q.catalog.as_deref()).await?))
}

pub async fn principal(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<PrincipalQuery>,
) -> ApiResult<Json<PrincipalPrivileges>> {
    Ok(Json(s.service.principal_privileges(&id, q.catalog.as_deref(), &q.principal).await?))
}

pub async fn object(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<ObjectQuery>,
) -> ApiResult<Json<ObjectAccessReport>> {
    Ok(Json(s.service.object_access(&id, q.catalog.as_deref(), &q.schema, &q.object).await?))
}
```

`crates/dblens-api/src/lib.rs`：在 `mod models;` 后加 `mod privileges;`；在 `/profiles/{id}/ops` 路由后加：

```rust
        .route("/profiles/{id}/privileges", get(privileges::overview))
        .route("/profiles/{id}/privileges/principal", get(privileges::principal))
        .route("/profiles/{id}/privileges/object", get(privileges::object))
```

- [ ] **Step 4: 确认测试通过**

Run: `cargo test -p dblens-api --offline`
Expected: 全部 PASS。

- [ ] **Step 5: 提交**

```bash
git add crates/dblens-api
git commit -m "feat(api): M9a 账号权限接口"
```

---

### Task 5：实连验证和达梦模板

**Files:**
- Create: `crates/dblens-service/tests/privilege_live.rs`
- Modify: `crates/dblens-dialect/sql/privilege/*.sql`（实测发现问题时）

**Interfaces:**
- Consumes: Task 3 的 `Service` 方法。

运行前向用户要 `DBLENS_TEST_*_URL` 里的密码，只放环境变量，不写入仓库。

- [ ] **Step 1: 写测试**

```rust
//! 账号与权限的实库测试：环境变量见 `live.rs`，没设置时直接通过。要串行运行（`--test-threads=1`）。
//!
//! 夹具：表 dblens_priv_t；角色 dblens_priv_r2 有该表的 SELECT，dblens_priv_r1 是 r2 的成员，
//! 用户 dblens_priv_u 是 r1 的成员；用户 dblens_priv_low 什么权限都没有。结束时删除，开始时也先删一次。

use std::path::Path;
use std::sync::Arc;

use dblens_dialect::privilege::{EffectiveState, GrantEffect};
use dblens_driver::{ConnectParams, DriverId};
use dblens_service::{
    Coverage, DefaultPolicy, DriverRegistry, Environment, ProfileInput, SecretBackend, Secrets, Service, SessionEvent,
    Store,
};

const LIB_OPTIONS: [&str; 2] = ["oracle_client_lib_dir", "dpi_lib_dir"];
const LOW_PASSWORD: &str = "Dblens_Priv_9x!";

struct Fixture {
    create: &'static [&'static str],
    drop: &'static [&'static str],
    /// 主体 id：用户、低权限用户。
    user: &'static str,
    /// 低权限用户登录时的账号名。
    low_login: &'static str,
    schema: &'static str,
    table: &'static str,
}

const MYSQL: Fixture = Fixture {
    create: &[
        "CREATE TABLE dblens_priv_t (id INT PRIMARY KEY)",
        "CREATE ROLE 'dblens_priv_r1', 'dblens_priv_r2'",
        "GRANT SELECT ON dblens_test.dblens_priv_t TO 'dblens_priv_r2'",
        "GRANT 'dblens_priv_r2' TO 'dblens_priv_r1'",
        "CREATE USER 'dblens_priv_u'@'%' IDENTIFIED BY 'Dblens_Priv_9x!'",
        "CREATE USER 'dblens_priv_low'@'%' IDENTIFIED BY 'Dblens_Priv_9x!'",
        "GRANT 'dblens_priv_r1' TO 'dblens_priv_u'@'%'",
    ],
    drop: &[
        "DROP USER IF EXISTS 'dblens_priv_u'@'%'",
        "DROP USER IF EXISTS 'dblens_priv_low'@'%'",
        "DROP ROLE IF EXISTS 'dblens_priv_r1'",
        "DROP ROLE IF EXISTS 'dblens_priv_r2'",
        "DROP TABLE IF EXISTS dblens_priv_t",
    ],
    user: "'dblens_priv_u'@'%'",
    low_login: "dblens_priv_low",
    schema: "dblens_test",
    table: "dblens_priv_t",
};

const MARIADB: Fixture = Fixture {
    create: &[
        "CREATE TABLE dblens_priv_t (id INT PRIMARY KEY)",
        "CREATE ROLE dblens_priv_r1",
        "CREATE ROLE dblens_priv_r2",
        "GRANT SELECT ON dblens_test.dblens_priv_t TO dblens_priv_r2",
        "GRANT dblens_priv_r2 TO dblens_priv_r1",
        "CREATE USER 'dblens_priv_u'@'%' IDENTIFIED BY 'Dblens_Priv_9x!'",
        "CREATE USER 'dblens_priv_low'@'%' IDENTIFIED BY 'Dblens_Priv_9x!'",
        "GRANT dblens_priv_r1 TO 'dblens_priv_u'@'%'",
    ],
    drop: &[
        "DROP USER IF EXISTS 'dblens_priv_u'@'%'",
        "DROP USER IF EXISTS 'dblens_priv_low'@'%'",
        "DROP ROLE IF EXISTS dblens_priv_r1",
        "DROP ROLE IF EXISTS dblens_priv_r2",
        "DROP TABLE IF EXISTS dblens_priv_t",
    ],
    user: "'dblens_priv_u'@'%'",
    low_login: "dblens_priv_low",
    schema: "dblens_test",
    table: "dblens_priv_t",
};

const POSTGRES: Fixture = Fixture {
    create: &[
        "CREATE TABLE dblens_priv_t (id INT PRIMARY KEY)",
        "CREATE ROLE dblens_priv_r2",
        "CREATE ROLE dblens_priv_r1",
        "GRANT SELECT ON dblens_priv_t TO dblens_priv_r2",
        "GRANT dblens_priv_r2 TO dblens_priv_r1",
        "CREATE ROLE dblens_priv_u LOGIN PASSWORD 'Dblens_Priv_9x!'",
        "CREATE ROLE dblens_priv_low LOGIN PASSWORD 'Dblens_Priv_9x!'",
        "GRANT dblens_priv_r1 TO dblens_priv_u",
    ],
    drop: &[
        "DROP TABLE IF EXISTS dblens_priv_t",
        "DROP ROLE IF EXISTS dblens_priv_u",
        "DROP ROLE IF EXISTS dblens_priv_low",
        "DROP ROLE IF EXISTS dblens_priv_r1",
        "DROP ROLE IF EXISTS dblens_priv_r2",
    ],
    user: "dblens_priv_u",
    low_login: "dblens_priv_low",
    schema: "public",
    table: "dblens_priv_t",
};

const ORACLE: Fixture = Fixture {
    create: &[
        "CREATE TABLE dblens_priv_t (id NUMBER PRIMARY KEY)",
        "CREATE ROLE dblens_priv_r2",
        "CREATE ROLE dblens_priv_r1",
        "GRANT SELECT ON dblens_priv_t TO dblens_priv_r2",
        "GRANT dblens_priv_r2 TO dblens_priv_r1",
        "CREATE USER dblens_priv_u IDENTIFIED BY \"Dblens_Priv_9x!\"",
        "CREATE USER dblens_priv_low IDENTIFIED BY \"Dblens_Priv_9x!\"",
        "GRANT CREATE SESSION TO dblens_priv_low",
        "GRANT dblens_priv_r1 TO dblens_priv_u",
    ],
    drop: &[
        "DROP USER dblens_priv_u",
        "DROP USER dblens_priv_low",
        "DROP ROLE dblens_priv_r1",
        "DROP ROLE dblens_priv_r2",
        "DROP TABLE dblens_priv_t PURGE",
    ],
    user: "DBLENS_PRIV_U",
    low_login: "DBLENS_PRIV_LOW",
    schema: "DBLENS_TEST",
    table: "DBLENS_PRIV_T",
};

const DAMENG: Fixture = Fixture {
    drop: &[
        "DROP USER DBLENS_PRIV_U CASCADE",
        "DROP USER DBLENS_PRIV_LOW CASCADE",
        "DROP ROLE DBLENS_PRIV_R1",
        "DROP ROLE DBLENS_PRIV_R2",
        "DROP TABLE DBLENS_PRIV_T",
    ],
    ..ORACLE
};

const SQLSERVER: Fixture = Fixture {
    create: &[
        "CREATE TABLE dbo.dblens_priv_t (id INT PRIMARY KEY)",
        "CREATE ROLE dblens_priv_r2",
        "CREATE ROLE dblens_priv_r1",
        "GRANT SELECT ON dbo.dblens_priv_t TO dblens_priv_r2",
        "ALTER ROLE dblens_priv_r2 ADD MEMBER dblens_priv_r1",
        "CREATE LOGIN dblens_priv_u WITH PASSWORD = 'Dblens_Priv_9x!'",
        "CREATE LOGIN dblens_priv_low WITH PASSWORD = 'Dblens_Priv_9x!'",
        "CREATE USER dblens_priv_u FOR LOGIN dblens_priv_u",
        "CREATE USER dblens_priv_low FOR LOGIN dblens_priv_low",
        "ALTER ROLE dblens_priv_r1 ADD MEMBER dblens_priv_u",
    ],
    drop: &[
        "DROP USER IF EXISTS dblens_priv_u",
        "DROP USER IF EXISTS dblens_priv_low",
        "IF SUSER_ID('dblens_priv_u') IS NOT NULL DROP LOGIN dblens_priv_u",
        "IF SUSER_ID('dblens_priv_low') IS NOT NULL DROP LOGIN dblens_priv_low",
        "DROP ROLE IF EXISTS dblens_priv_r1",
        "DROP ROLE IF EXISTS dblens_priv_r2",
        "DROP TABLE IF EXISTS dbo.dblens_priv_t",
    ],
    user: "server:dblens_priv_u",
    low_login: "dblens_priv_low",
    schema: "dbo",
    table: "dblens_priv_t",
};

fn has_implicit(driver: DriverId) -> bool {
    driver != DriverId::Mysql
}

async fn profile(service: &Service, driver: DriverId, params: &ConnectParams, user: &str, password: &str) -> String {
    let mut options = params.options.clone();
    options.retain(|k, _| !LIB_OPTIONS.contains(&k.as_str()));
    let p = service
        .create_profile(
            ProfileInput {
                name: format!("priv-{user}"),
                driver,
                host: params.host.clone(),
                port: params.port,
                user: user.to_string(),
                database: params.database.clone(),
                options,
                environment: Environment::Test,
                color: None,
                connect_timeout_secs: 10,
                product: None,
            },
            Some(password),
        )
        .unwrap();
    p.id
}

/// 执行夹具语句；`tolerate` 为 true 时忽略错误（清理时对象可能不存在）。
async fn run(service: &Service, profile: &str, statements: &[&str], tolerate: bool) {
    let info = service.open_session(profile).await.unwrap();
    for sql in statements {
        service.sessions.execute(info.id, sql, true).await.unwrap();
        while let Some(ev) = service.sessions.next(info.id, 100).await.unwrap() {
            if let SessionEvent::Failed { error, .. } = &ev {
                assert!(tolerate, "{sql}: {error:?}");
            }
            if matches!(ev, SessionEvent::Done) {
                break;
            }
        }
    }
    service.sessions.close(info.id, false).await.unwrap();
}

async fn flow(env: &str, fx: Fixture) {
    let Ok(url) = std::env::var(env) else { return };
    let (driver, params) = ConnectParams::from_url(&url).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(Store::open(dir.path()).unwrap());
    let secrets = Arc::new(Secrets::with_backend(store.clone(), SecretBackend::Vault));
    secrets.init_vault("测试主密码").unwrap();
    let registry = Arc::new(DriverRegistry::new(store.clone()));
    for option in LIB_OPTIONS {
        if let Some(lib) = params.options.get(option) {
            registry.set_lib_dir(driver, Some(Path::new(lib))).unwrap();
        }
    }
    let service = Service::with_parts(store, secrets, registry, Arc::new(DefaultPolicy));
    let admin = profile(&service, driver, &params, &params.user, &params.password).await;

    run(&service, &admin, fx.drop, true).await;
    run(&service, &admin, fx.create, false).await;

    let overview = service.privilege_overview(&admin, None).await.unwrap();
    assert_eq!(overview.coverage, Coverage::Complete, "{env}");
    assert!(overview.principals.iter().any(|p| p.id == fx.user), "{env}: {:?}", overview.principals.len());
    // TiDB 没有 mysql.sys 这类系统账号
    assert!(driver == DriverId::Mysql || overview.principals.iter().any(|p| p.builtin), "{env}: 内置账号要有标记");

    let detail = service.principal_privileges(&admin, None, fx.user).await.unwrap();
    let select = detail
        .effective
        .iter()
        .find(|e| e.grant.privilege == "SELECT" && e.grant.target.object.as_deref().is_some_and(|o| o.eq_ignore_ascii_case(fx.table)))
        .unwrap_or_else(|| panic!("{env}: {:?}", detail.effective));
    assert!(select.via.len() >= 3, "{env}: 经两层角色：{:?}", select.via);
    assert_eq!(select.state, EffectiveState::Allowed, "{env}");
    assert!(detail.direct.is_empty(), "{env}: 用户本身没有直接授权：{:?}", detail.direct);

    let report = service.object_access(&admin, None, fx.schema, fx.table).await.unwrap();
    assert_eq!(report.coverage, Coverage::Complete, "{env}");
    assert!(
        report.access.iter().any(|a| a.principal == fx.user && a.privilege.grant.privilege == "SELECT"),
        "{env}: {:?}",
        report.access.iter().map(|a| &a.principal).collect::<Vec<_>>()
    );
    assert!(
        !has_implicit(driver) || report.access.iter().any(|a| a.privilege.grant.effect == GrantEffect::Implicit),
        "{env}: 属主或超级用户要列出"
    );

    // 低权限连接不报整页错误
    let low = profile(&service, driver, &params, fx.low_login, LOW_PASSWORD).await;
    let overview = service.privilege_overview(&low, None).await.unwrap();
    println!("{env}: 低权限 {:?}，主体 {}", overview.coverage, overview.principals.len());
    service.principal_privileges(&low, None, fx.user).await.unwrap();
    service.object_access(&low, None, fx.schema, fx.table).await.unwrap();

    run(&service, &admin, fx.drop, true).await;
}

#[tokio::test]
async fn mysql() {
    flow("DBLENS_TEST_MYSQL_URL", MYSQL).await;
}

#[tokio::test]
async fn mariadb() {
    flow("DBLENS_TEST_MARIADB_URL", MARIADB).await;
}

#[tokio::test]
async fn tidb() {
    flow("DBLENS_TEST_TIDB_URL", MYSQL).await;
}

#[tokio::test]
async fn oceanbase() {
    flow("DBLENS_TEST_OCEANBASE_URL", MYSQL).await;
}

#[tokio::test]
async fn postgres() {
    flow("DBLENS_TEST_PG_URL", POSTGRES).await;
}

#[tokio::test]
async fn opengauss() {
    flow("DBLENS_TEST_OPENGAUSS_URL", POSTGRES).await;
}

#[tokio::test]
async fn opengauss_a() {
    flow("DBLENS_TEST_OPENGAUSS_A_URL", POSTGRES).await;
}

#[tokio::test]
async fn kingbase() {
    flow("DBLENS_TEST_KINGBASE_URL", POSTGRES).await;
}

#[tokio::test]
async fn kingbase_pg() {
    flow("DBLENS_TEST_KINGBASE_PG_URL", POSTGRES).await;
}

#[tokio::test]
async fn oracle() {
    flow("DBLENS_TEST_ORACLE_URL", ORACLE).await;
}

#[tokio::test]
async fn dameng() {
    flow("DBLENS_TEST_DM_URL", DAMENG).await;
}

#[tokio::test]
async fn sqlserver() {
    flow("DBLENS_TEST_SQLSERVER_URL", SQLSERVER).await;
}
```

MySQL 系里 `dblens_priv_r2` 这类没有成员的角色，靠模板的“锁定且无密码”判断为角色。

openGauss 基于 PG 9.2，`NULL AS host` 的列类型可能是 `unknown`，驱动读不出时把模板里的这类列改成 `CAST(NULL AS text)`（`memberships` 用的是 `postgres.sql`，改动放进 `opengauss.sql` 的覆盖段）。

- [ ] **Step 2: 先修达梦模板**

用户提供 DM 连接串后，只设 `DBLENS_TEST_DM_URL`，运行：

```bash
cargo test -p dblens-service --test privilege_live dameng --offline -- --test-threads=1 --nocapture
```

Expected: 通过；不通过时按报错修正 `crates/dblens-dialect/sql/privilege/dameng.sql`（字段名、`DBA_ROLES` 列名、`ALL_TAB_PRIVS` 列名、内置账号名单），同步素材目录里的同名文件。

- [ ] **Step 3: 跑全部实连**

```bash
cargo test -p dblens-service --test privilege_live --offline -- --test-threads=1 --nocapture
```

Expected: 12 个测试 PASS，打印每种库低权限连接的 `Coverage`。

记录并判断打印结果：

| 库 | 低权限下应有的结果 |
|---|---|
| MySQL、MariaDB、TiDB、OceanBase | `Partial`，`mysql.*` 被拒，走 `@limited` |
| Oracle、达梦 | `Partial`，走 `ALL_`/`USER_` |
| PG 系 | `Complete`，目录表对所有人可读 |
| SQL Server | `Partial`，`caps.full_view` 为 0 |

结果不符时修模板。MySQL 8 以上用户默认没有 `mysql.*` 的 SELECT；若 `SHOW` 或视图表现不同，以实测为准并写进实现说明。

- [ ] **Step 4: SQL Server 2025**

把 `DBLENS_TEST_SQLSERVER_URL` 指向 VM .103（需要用户开机，密码在该机的 `C:\ProgramData\dblens-test\creds.txt`），重跑 `sqlserver`。

Expected: PASS。该 VM 未开机时在实现说明里记下“2025 未实测”，不阻塞合并。

- [ ] **Step 5: 确认没有遗留夹具**

```bash
cargo test -p dblens-service --test privilege_live --offline -- --test-threads=1
```

Expected: 第二次运行同样通过（开始时的清理和结束时的清理都生效）。

- [ ] **Step 6: 提交**

```bash
git add crates/dblens-service/tests/privilege_live.rs crates/dblens-dialect/sql/privilege docs/superpowers/plans/2026-10-30-m9a-privileges-materials
git commit -m "test(service): M9a 账号权限实库测试，修正各库模板"
```

---

### Task 6：前端数据层和标签

**Files:**
- Modify: `web/src/shared/api.ts`
- Modify: `web/src/features/editor/tabs.ts`
- Test: `web/src/features/editor/tabs.test.ts`
- Create: `web/src/features/privileges/logic.ts`
- Test: `web/src/features/privileges/logic.test.ts`

**Interfaces:**
- Consumes: Task 4 的接口。
- Produces: `api.privilegeOverview(id, catalog?)`、`api.principalPrivileges(id, principal, catalog?)`、`api.objectAccess(id, schema, object, catalog?)`；`useTabs.openPrivileges(profileId, title, focus?)`、`useTabs.setPrivilegesFocus(id, focus)`；类型 `PrivilegesTab`；`filterPrincipals`、`targetLabel`、`pathLabel`、`groupAccess`。

- [ ] **Step 1: 写失败的测试**

`web/src/features/privileges/logic.test.ts`：

```ts
import { describe, expect, it } from 'vitest'

import type { EffectivePrivilege, ObjectAccess, Principal, PrivilegeGrant } from '../../shared/api'
import { filterPrincipals, groupAccess, pathLabel, targetLabel } from './logic'

const p = (id: string, over: Partial<Principal> = {}): Principal => ({
  id, name: id, host: null, kind: 'user', level: 'server', can_login: true, locked: null, expires_at: null, builtin: false, attrs: [], ...over,
})
const g = (over: Partial<PrivilegeGrant> = {}): PrivilegeGrant => ({
  grantee: 'u', scope: 'object',
  target: { database: null, schema: 'hr', object_kind: 'table', object: 'emp', column: null },
  privilege: 'SELECT', effect: 'grant', grantable: false, grantor: null, ...over,
})

describe('filterPrincipals', () => {
  const list = [p('alice'), p('r', { kind: 'role' }), p('sys', { builtin: true })]
  it('默认不列内置账号', () => {
    expect(filterPrincipals(list, { text: '', kind: 'all', builtin: false }).map((x) => x.id)).toEqual(['alice', 'r'])
  })
  it('按种类和文字过滤，不区分大小写', () => {
    expect(filterPrincipals(list, { text: '', kind: 'role', builtin: true }).map((x) => x.id)).toEqual(['r'])
    expect(filterPrincipals(list, { text: 'ALI', kind: 'all', builtin: true }).map((x) => x.id)).toEqual(['alice'])
  })
  it('登录名算用户一类', () => {
    expect(filterPrincipals([p('l', { kind: 'login' })], { text: '', kind: 'user', builtin: false })).toHaveLength(1)
  })
  it('host 也能搜', () => {
    expect(filterPrincipals([p('a', { host: '10.0.%' })], { text: '10.0', kind: 'all', builtin: false })).toHaveLength(1)
  })
})

describe('targetLabel', () => {
  it('按范围写出对象', () => {
    expect(targetLabel(g())).toBe('hr.emp')
    expect(targetLabel(g({ scope: 'column', target: { ...g().target, column: 'id' } }))).toBe('hr.emp.id')
    expect(targetLabel(g({ scope: 'schema', target: { ...g().target, object: null } }))).toBe('hr')
    expect(targetLabel(g({ scope: 'database', target: { ...g().target, database: 'db', schema: null, object: null } }))).toBe('db')
    expect(targetLabel(g({ scope: 'global', target: { database: null, schema: null, object_kind: null, object: null, column: null } }))).toBe('')
  })
})

describe('pathLabel', () => {
  it('用主体名字连成路径，认不出的 id 原样显示', () => {
    expect(pathLabel(['a', 'b'], [p('a', { name: '甲' })])).toBe('甲 → b')
  })
})

describe('groupAccess', () => {
  const e = (privilege: string, state: EffectivePrivilege['state'] = 'allowed'): EffectivePrivilege => ({ grant: g({ privilege }), via: ['u'], state, notes: [] })
  it('按主体合并，保持出现顺序', () => {
    const rows: ObjectAccess[] = [
      { principal: 'u', privilege: e('SELECT') },
      { principal: 'v', privilege: e('INSERT') },
      { principal: 'u', privilege: e('UPDATE') },
    ]
    expect(groupAccess(rows).map((x) => [x.principal, x.items.length])).toEqual([['u', 2], ['v', 1]])
  })
})
```

`web/src/features/editor/tabs.test.ts` 追加：

```ts
describe('账号权限标签', () => {
  it('同一连接只开一个，重复打开时更新定位', () => {
    const { openPrivileges } = useTabs.getState()
    const a = openPrivileges('p1', '用户与权限')
    const b = openPrivileges('p1', '用户与权限', { schema: 'hr', object: 'emp' })
    expect(b).toBe(a)
    expect(useTabs.getState().tabs.find((x) => x.id === a)?.privileges?.focus).toEqual({ schema: 'hr', object: 'emp' })
    expect(openPrivileges('p2', '用户与权限')).not.toBe(a)
  })

  it('保存的标签能恢复，格式不对的丢弃', () => {
    const tab = { id: 'a', title: 't', profileId: 'p', sql: '', privileges: {} }
    expect(parseTabs(JSON.stringify({ tabs: [tab] })).tabs).toHaveLength(1)
    const bad = { ...tab, privileges: { focus: { schema: 1 } } }
    expect(parseTabs(JSON.stringify({ tabs: [bad] })).tabs).toHaveLength(0)
  })
})
```

该文件已导入 `useTabs` 和 `parseTabs`，并在 `beforeEach` 里重置状态；缺哪个就按文件现有方式补。

- [ ] **Step 2: 确认测试失败**

Run: `cd web && pnpm exec vitest run src/features/privileges src/features/editor/tabs.test.ts`
Expected: FAIL，`logic` 不存在，`openPrivileges` 未定义。

- [ ] **Step 3: 加 API 类型和方法**

`web/src/shared/api.ts`，在 `OpsQuery` 类型附近：

```ts
export type PrincipalKind = 'user' | 'role' | 'login' | 'group'
export type GrantScope = 'global' | 'database' | 'schema' | 'object' | 'column'
export type EffectiveState = 'allowed' | 'denied' | 'conditional'
export type PrivNote = 'needs_set_role' | 'not_default_role' | 'public_grant' | 'implicit' | 'any_privilege' | 'partial_revokes' | 'deny_override' | 'host_wildcard'

export interface Principal {
  id: string
  name: string
  host: string | null
  kind: PrincipalKind
  level: 'server' | 'database'
  can_login: boolean | null
  locked: boolean | null
  expires_at: string | null
  builtin: boolean
  attrs: [string, string][]
}

export interface Membership {
  member: string
  role: string
  admin_option: boolean
  inherits: boolean | null
  default_role: boolean | null
}

export interface PrivilegeGrant {
  grantee: string
  scope: GrantScope
  target: { database: string | null; schema: string | null; object_kind: string | null; object: string | null; column: string | null }
  privilege: string
  effect: 'grant' | 'deny' | 'implicit'
  grantable: boolean
  grantor: string | null
}

export interface EffectivePrivilege {
  grant: PrivilegeGrant
  via: string[]
  state: EffectiveState
  notes: PrivNote[]
}

export interface ObjectAccess {
  principal: string
  privilege: EffectivePrivilege
}

/** 读到的范围。`missing` 是后端按请求语言给出的文字。 */
export type Coverage = { state: 'complete' } | { state: 'partial'; missing: string[] }

export interface PrivilegeOverview {
  principals: Principal[]
  memberships: Membership[]
  public: string[]
  coverage: Coverage
}

export interface PrincipalPrivileges {
  direct: PrivilegeGrant[]
  effective: EffectivePrivilege[]
  coverage: Coverage
}

export interface ObjectAccessReport {
  access: ObjectAccess[]
  coverage: Coverage
}
```

在 `ops:` 方法后加：

```ts
    privilegeOverview: (id: string, catalog?: string) =>
      request<PrivilegeOverview>('GET', `${p(id)}/privileges${q({ catalog })}`),
    principalPrivileges: (id: string, principal: string, catalog?: string) =>
      request<PrincipalPrivileges>('GET', `${p(id)}/privileges/principal${q({ principal, catalog })}`),
    objectAccess: (id: string, schema: string, object: string, catalog?: string) =>
      request<ObjectAccessReport>('GET', `${p(id)}/privileges/object${q({ schema, object, catalog })}`),
```

- [ ] **Step 4: 写 `logic.ts`**

```ts
// 账号与权限的纯逻辑（M9 设计文档第 8 节）。

import type { ObjectAccess, Principal, PrivilegeGrant } from '../../shared/api'

export type KindFilter = 'all' | 'user' | 'role'

export interface PrincipalFilter {
  text: string
  kind: KindFilter
  /** 是否列出内置账号和角色。 */
  builtin: boolean
}

export function filterPrincipals(list: Principal[], f: PrincipalFilter): Principal[] {
  const text = f.text.trim().toLowerCase()
  return list.filter((p) => {
    if (!f.builtin && p.builtin) return false
    if (f.kind === 'role' && p.kind !== 'role') return false
    if (f.kind === 'user' && p.kind === 'role') return false
    return !text || p.name.toLowerCase().includes(text) || (p.host ?? '').toLowerCase().includes(text)
  })
}

/** 授权作用的对象，全局范围时为空串。 */
export function targetLabel(g: PrivilegeGrant): string {
  const { database, schema, object, column } = g.target
  return [g.scope === 'database' ? database : null, schema, object, column].filter(Boolean).join('.')
}

export function pathLabel(via: string[], principals: Principal[]): string {
  return via.map((id) => principals.find((p) => p.id === id)?.name ?? id).join(' → ')
}

/** 反查结果按主体合并，保持出现顺序。 */
export function groupAccess(rows: ObjectAccess[]): { principal: string; items: ObjectAccess[] }[] {
  const out: { principal: string; items: ObjectAccess[] }[] = []
  for (const r of rows) {
    const g = out.find((x) => x.principal === r.principal)
    if (g) g.items.push(r)
    else out.push({ principal: r.principal, items: [r] })
  }
  return out
}
```

- [ ] **Step 5: 改 `tabs.ts`**

文件头注释加一行：

```ts
// 账号权限标签（M9 设计文档第 8 节）：带 privileges 的标签，同一连接只开一个；focus 是对象反查的目标。
```

`QueryTab` 里 `lint?: LintTab` 后加：

```ts
  /** 有值时是账号权限标签。 */
  privileges?: PrivilegesTab
```

`LintTab` 类型后加：

```ts
/** 账号权限标签。`focus` 有值时显示这个对象的访问者。schema 是合成值，SQL Server 含库名。 */
export interface PrivilegesTab {
  focus?: { schema: string; object: string }
}

function isPrivilegesTab(v: unknown): v is PrivilegesTab {
  const p = v as PrivilegesTab | null
  if (typeof p !== 'object' || p === null) return false
  return p.focus === undefined || (typeof p.focus?.schema === 'string' && typeof p.focus.object === 'string')
}
```

`parseTabs` 的过滤条件在 `isLintTab` 那行后加：

```ts
        (t.lint === undefined || isLintTab(t.lint)) &&
        (t.privileges === undefined || isPrivilegesTab(t.privileges)),
```

`TabsState` 加：

```ts
  /** 打开连接的账号权限标签：已打开时切换过去。`focus` 有值时定位到对象。返回标签的 id。 */
  openPrivileges(profileId: string, title: string, focus?: PrivilegesTab['focus']): string
  /** 修改定位：undefined 回到按主体浏览。 */
  setPrivilegesFocus(id: string, focus: PrivilegesTab['focus']): void
```

实现放在 `openLintRun` 后：

```ts
  openPrivileges: (profileId, title, focus) => {
    const { tabs } = get()
    const same = tabs.find((x) => x.privileges && x.profileId === profileId)
    if (same) {
      set({
        active: same.id,
        tabs: focus ? tabs.map((x) => (x.id === same.id ? { ...x, privileges: { focus } } : x)) : tabs,
      })
      return same.id
    }
    const tab: QueryTab = { id: newId(), title, profileId, sql: '', privileges: focus ? { focus } : {} }
    set({ tabs: [...tabs, tab], active: tab.id })
    return tab.id
  },
  setPrivilegesFocus: (id, focus) =>
    set({ tabs: get().tabs.map((x) => (x.id === id && x.privileges ? { ...x, privileges: focus ? { focus } : {} } : x)) }),
```

- [ ] **Step 6: 确认测试通过**

Run: `cd web && pnpm exec vitest run src/features/privileges src/features/editor/tabs.test.ts`
Expected: PASS。

- [ ] **Step 7: 提交**

```bash
git add web/src/shared/api.ts web/src/features/editor/tabs.ts web/src/features/editor/tabs.test.ts web/src/features/privileges
git commit -m "feat(web): M9a 账号权限接口客户端、标签和纯逻辑"
```

---

### Task 7：界面

**Files:**
- Create: `web/src/features/privileges/PrivilegesView.tsx`
- Create: `web/src/locales/zh-CN/privilege.json`、`web/src/locales/en/privilege.json`
- Modify: `web/src/locales/index.ts`
- Modify: `web/src/locales/zh-CN/tree.json`、`web/src/locales/en/tree.json`
- Modify: `web/src/features/editor/Workspace.tsx`
- Modify: `web/src/features/tree/ObjectTree.tsx`

**Interfaces:**
- Consumes: Task 6 的 `api.*`、`useTabs.openPrivileges`、`logic.ts`。

- [ ] **Step 1: 写词条**

`web/src/locales/zh-CN/privilege.json`：

```json
{
  "title": "用户与权限",
  "search": "搜索用户或角色",
  "kind": { "all": "全部", "user": "用户", "role": "角色" },
  "showBuiltin": "显示内置",
  "refresh": "刷新",
  "catalog": "数据库",
  "empty": "选择左侧的用户或角色",
  "none": "没有符合条件的用户或角色",
  "tab": { "attrs": "属性", "direct": "直接授权", "effective": "实际权限" },
  "attr": { "name": "名称", "kind": "种类", "level": "级别", "host": "主机", "login": "可登录", "locked": "已锁定", "expires": "过期时间", "roles": "所属角色", "builtin": "内置" },
  "yes": "是",
  "no": "否",
  "unknown": "未知",
  "kindName": { "user": "用户", "role": "角色", "login": "登录名", "group": "组" },
  "levelName": { "server": "服务器", "database": "数据库" },
  "scope": { "global": "全局", "database": "数据库", "schema": "模式", "object": "对象", "column": "列" },
  "col": { "scope": "范围", "target": "对象", "privilege": "权限", "grantable": "可转授", "grantor": "授予者", "path": "来源路径", "state": "状态", "principal": "主体" },
  "state": { "allowed": "允许", "denied": "拒绝", "conditional": "有条件" },
  "note": {
    "needs_set_role": "需要 SET ROLE 才生效",
    "not_default_role": "角色需在会话中启用",
    "public_grant": "所有用户都有",
    "implicit": "隐含权限，不是授权记录",
    "any_privilege": "来自系统权限",
    "partial_revokes": "存在部分回收，结果可能不准",
    "deny_override": "被 DENY 覆盖",
    "host_wildcard": "主机含通配符，不判断实际命中的账号"
  },
  "computed": "按字典计算的结果，不代表实际执行一定成功。",
  "partial": "读取范围不完整",
  "object": { "title": "谁能访问 {{target}}", "back": "返回列表", "none": "没有读到访问者" },
  "error": "读取失败"
}
```

`web/src/locales/en/privilege.json`：

```json
{
  "title": "Users & privileges",
  "search": "Search users or roles",
  "kind": { "all": "All", "user": "Users", "role": "Roles" },
  "showBuiltin": "Show built-in",
  "refresh": "Refresh",
  "catalog": "Database",
  "empty": "Select a user or role on the left",
  "none": "No matching users or roles",
  "tab": { "attrs": "Properties", "direct": "Direct grants", "effective": "Effective privileges" },
  "attr": { "name": "Name", "kind": "Kind", "level": "Level", "host": "Host", "login": "Can log in", "locked": "Locked", "expires": "Expires", "roles": "Member of", "builtin": "Built-in" },
  "yes": "Yes",
  "no": "No",
  "unknown": "Unknown",
  "kindName": { "user": "User", "role": "Role", "login": "Login", "group": "Group" },
  "levelName": { "server": "Server", "database": "Database" },
  "scope": { "global": "Global", "database": "Database", "schema": "Schema", "object": "Object", "column": "Column" },
  "col": { "scope": "Scope", "target": "Object", "privilege": "Privilege", "grantable": "Grantable", "grantor": "Grantor", "path": "Granted through", "state": "State", "principal": "Principal" },
  "state": { "allowed": "Allowed", "denied": "Denied", "conditional": "Conditional" },
  "note": {
    "needs_set_role": "Takes effect after SET ROLE",
    "not_default_role": "Role must be enabled in the session",
    "public_grant": "Held by all users",
    "implicit": "Implicit, not a grant record",
    "any_privilege": "From a system privilege",
    "partial_revokes": "Partial revokes exist; this may be inaccurate",
    "deny_override": "Overridden by DENY",
    "host_wildcard": "Host has a wildcard; the matching account is not resolved"
  },
  "computed": "Computed from the data dictionary; execution may still differ.",
  "partial": "Incomplete results",
  "object": { "title": "Who can access {{target}}", "back": "Back to list", "none": "No one found" },
  "error": "Failed to read"
}
```

`web/src/locales/index.ts`：按现有顺序加 `enPrivilege`、`zhPrivilege` 两个 import，在 `zhCN` 和 `en` 对象里各加 `privilege: zhPrivilege` / `privilege: enPrivilege`。

`tree.json` 的 `menu` 里加：中文 `"privileges": "用户与权限"`、`"whoCanAccess": "谁能访问"`；英文 `"privileges": "Users & privileges"`、`"whoCanAccess": "Who can access"`。

- [ ] **Step 2: 写 `PrivilegesView.tsx`**

```tsx
// 账号与权限标签（M9 设计文档第 8 节）：左侧主体列表，右侧属性、直接授权、实际权限；对象反查时右侧列出访问者。只读。

import { Alert, Button, Checkbox, Empty, Input, Segmented, Select, Spin, Table, Tabs, Tooltip } from 'antd'
import type { ColumnsType } from 'antd/es/table'
import { IconArrowLeft, IconRefresh } from '@tabler/icons-react'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { useApi } from '../../app/context'
import { schemaLabel, splitCatalog } from '../../shared/api'
import type { Coverage, EffectivePrivilege, Principal, PrivilegeGrant, PrivNote } from '../../shared/api'
import { type QueryTab, useTabs } from '../editor/tabs'
import { type KindFilter, filterPrincipals, groupAccess, pathLabel, targetLabel } from './logic'

function CoverageNote({ coverage }: { coverage: Coverage | undefined }) {
  const { t } = useTranslation()
  if (coverage?.state !== 'partial') return null
  return <Alert type="warning" showIcon banner message={`${t('privilege.partial')}：${coverage.missing.join('；')}`} />
}

function Notes({ notes }: { notes: PrivNote[] }) {
  const { t } = useTranslation()
  if (!notes.length) return null
  return <span className="muted"> {notes.map((n) => t(`privilege.note.${n}`)).join('；')}</span>
}

function useGrantColumns(): ColumnsType<PrivilegeGrant> {
  const { t } = useTranslation()
  return [
    { title: t('privilege.col.scope'), dataIndex: 'scope', width: 90, render: (s: PrivilegeGrant['scope']) => t(`privilege.scope.${s}`) },
    { title: t('privilege.col.target'), key: 'target', ellipsis: true, render: (_, g) => targetLabel(g) },
    { title: t('privilege.col.privilege'), dataIndex: 'privilege', width: 200 },
    { title: t('privilege.col.grantable'), dataIndex: 'grantable', width: 80, render: (v: boolean) => (v ? t('privilege.yes') : t('privilege.no')) },
    { title: t('privilege.col.grantor'), dataIndex: 'grantor', width: 140, ellipsis: true },
  ]
}

function useEffectiveColumns(principals: Principal[]): ColumnsType<EffectivePrivilege> {
  const { t } = useTranslation()
  return [
    { title: t('privilege.col.scope'), key: 'scope', width: 90, render: (_, e) => t(`privilege.scope.${e.grant.scope}`) },
    { title: t('privilege.col.target'), key: 'target', ellipsis: true, render: (_, e) => targetLabel(e.grant) },
    { title: t('privilege.col.privilege'), key: 'privilege', width: 180, render: (_, e) => e.grant.privilege },
    { title: t('privilege.col.state'), dataIndex: 'state', width: 80, render: (s: EffectivePrivilege['state']) => t(`privilege.state.${s}`) },
    { title: t('privilege.col.path'), key: 'path', render: (_, e) => <span>{pathLabel(e.via, principals)}<Notes notes={e.notes} /></span> },
  ]
}

function PrincipalDetail({ tab, catalog, principal, principals, roles }: {
  tab: QueryTab
  catalog: string | undefined
  principal: Principal
  principals: Principal[]
  roles: string[]
}) {
  const api = useApi()
  const { t } = useTranslation()
  const grantColumns = useGrantColumns()
  const effectiveColumns = useEffectiveColumns(principals)
  const { data, error, isLoading } = useQuery({
    queryKey: ['privileges', tab.profileId, catalog, 'principal', principal.id],
    queryFn: () => api.principalPrivileges(tab.profileId, principal.id, catalog),
  })
  const yesNo = (v: boolean | null) => (v === null ? t('privilege.unknown') : v ? t('privilege.yes') : t('privilege.no'))
  const rows: [string, string][] = [
    [t('privilege.attr.name'), principal.name],
    [t('privilege.attr.kind'), t(`privilege.kindName.${principal.kind}`)],
    [t('privilege.attr.level'), t(`privilege.levelName.${principal.level}`)],
    ...(principal.host ? ([[t('privilege.attr.host'), principal.host]] as [string, string][]) : []),
    [t('privilege.attr.login'), yesNo(principal.can_login)],
    [t('privilege.attr.locked'), yesNo(principal.locked)],
    [t('privilege.attr.expires'), principal.expires_at ?? '—'],
    [t('privilege.attr.roles'), roles.join(', ') || '—'],
    [t('privilege.attr.builtin'), principal.builtin ? t('privilege.yes') : t('privilege.no')],
    ...principal.attrs,
  ]
  return (
    <div style={{ display: 'flex', flexDirection: 'column', minHeight: 0, flex: 1 }}>
      <CoverageNote coverage={data?.coverage} />
      {error && <div className="err" style={{ padding: 12 }}>{t('privilege.error')}：{(error as Error).message}</div>}
      <Tabs
        size="small"
        style={{ padding: '0 12px', flex: 1, minHeight: 0, overflow: 'auto' }}
        items={[
          {
            key: 'attrs',
            label: t('privilege.tab.attrs'),
            children: (
              <table className="kv">
                <tbody>
                  {rows.map(([k, v]) => (
                    <tr key={k}><td className="muted" style={{ paddingRight: 24 }}>{k}</td><td>{v}</td></tr>
                  ))}
                </tbody>
              </table>
            ),
          },
          {
            key: 'direct',
            label: t('privilege.tab.direct'),
            children: <Table size="small" pagination={false} loading={isLoading} rowKey={(g) => `${g.scope}|${targetLabel(g)}|${g.privilege}`} columns={grantColumns} dataSource={data?.direct} />,
          },
          {
            key: 'effective',
            label: t('privilege.tab.effective'),
            children: (
              <>
                <div className="muted" style={{ marginBottom: 8 }}>{t('privilege.computed')}</div>
                <Table size="small" pagination={false} loading={isLoading} rowKey={(e) => `${e.via.join('>')}|${e.grant.scope}|${targetLabel(e.grant)}|${e.grant.privilege}`} columns={effectiveColumns} dataSource={data?.effective} />
              </>
            ),
          },
        ]}
      />
    </div>
  )
}

function ObjectAccessPanel({ tab, principals, onPick }: { tab: QueryTab; principals: Principal[]; onPick: (id: string) => void }) {
  const api = useApi()
  const { t } = useTranslation()
  const focus = tab.privileges!.focus!
  const { catalog, schema } = splitCatalog(focus.schema)
  const { data, error, isLoading } = useQuery({
    queryKey: ['privileges', tab.profileId, catalog, 'object', focus.schema, focus.object],
    queryFn: () => api.objectAccess(tab.profileId, schema, focus.object, catalog),
  })
  const groups = useMemo(() => groupAccess(data?.access ?? []), [data])
  return (
    <div style={{ display: 'flex', flexDirection: 'column', minHeight: 0, flex: 1 }}>
      <div className="row" style={{ padding: '6px 12px', gap: 8 }}>
        <Button size="small" type="text" icon={<IconArrowLeft size={14} />} onClick={() => useTabs.getState().setPrivilegesFocus(tab.id, undefined)}>
          {t('privilege.object.back')}
        </Button>
        <strong>{t('privilege.object.title', { target: `${schemaLabel(focus.schema)}.${focus.object}` })}</strong>
      </div>
      <CoverageNote coverage={data?.coverage} />
      <div className="muted" style={{ padding: '4px 12px' }}>{t('privilege.computed')}</div>
      {error && <div className="err" style={{ padding: 12 }}>{t('privilege.error')}：{(error as Error).message}</div>}
      <div style={{ flex: 1, overflow: 'auto', padding: '0 12px' }}>
        {isLoading ? <Spin style={{ margin: 48 }} /> : !groups.length ? (
          <Empty image={Empty.PRESENTED_IMAGE_SIMPLE} description={t('privilege.object.none')} />
        ) : (
          groups.map((g) => (
            <div key={g.principal} style={{ marginBottom: 12 }}>
              <a onClick={() => onPick(g.principal)}>{principals.find((p) => p.id === g.principal)?.name ?? g.principal}</a>
              {g.items.map((a, i) => (
                <div key={i} className="muted" style={{ paddingLeft: 12 }}>
                  {a.privilege.grant.privilege}（{t(`privilege.state.${a.privilege.state}`)}）· {pathLabel(a.privilege.via, principals)}
                  <Notes notes={a.privilege.notes} />
                </div>
              ))}
            </div>
          ))
        )}
      </div>
    </div>
  )
}

export default function PrivilegesView({ tab }: { tab: QueryTab }) {
  const api = useApi()
  const qc = useQueryClient()
  const { t } = useTranslation()
  const [text, setText] = useState('')
  const [kind, setKind] = useState<KindFilter>('all')
  const [builtin, setBuiltin] = useState(false)
  const [picked, setPicked] = useState<string | null>(null)
  const focus = tab.privileges?.focus
  const focusCatalog = focus ? splitCatalog(focus.schema).catalog : undefined
  const [chosenCatalog, setChosenCatalog] = useState<string | undefined>(undefined)
  const catalog = focusCatalog ?? chosenCatalog
  const { data: catalogs = [] } = useQuery({ queryKey: ['catalogs', tab.profileId], queryFn: () => api.catalogs(tab.profileId) })
  const { data, error, isLoading } = useQuery({
    queryKey: ['privileges', tab.profileId, catalog, 'overview'],
    queryFn: () => api.privilegeOverview(tab.profileId, catalog),
  })
  const principals = useMemo(() => data?.principals ?? [], [data])
  const list = useMemo(() => filterPrincipals(principals, { text, kind, builtin }), [principals, text, kind, builtin])
  const current = principals.find((p) => p.id === picked)
  const roles = (id: string) =>
    (data?.memberships ?? [])
      .filter((m) => m.member === id)
      .map((m) => principals.find((p) => p.id === m.role)?.name ?? m.role)

  return (
    <div style={{ display: 'flex', flex: 1, minHeight: 0 }}>
      <div style={{ width: 260, display: 'flex', flexDirection: 'column', borderRight: '1px solid var(--border)' }}>
        <div style={{ padding: 8, display: 'flex', flexDirection: 'column', gap: 6 }}>
          {catalogs.length > 0 && (
            <Select
              size="small"
              placeholder={t('privilege.catalog')}
              value={catalog}
              onChange={setChosenCatalog}
              options={catalogs.map((c) => ({ value: c.name, label: c.name }))}
            />
          )}
          <Input size="small" allowClear placeholder={t('privilege.search')} value={text} onChange={(e) => setText(e.target.value)} />
          <div className="row" style={{ gap: 6 }}>
            <Segmented
              size="small"
              value={kind}
              onChange={(v) => setKind(v as KindFilter)}
              options={[
                { value: 'all', label: t('privilege.kind.all') },
                { value: 'user', label: t('privilege.kind.user') },
                { value: 'role', label: t('privilege.kind.role') },
              ]}
            />
            <span className="grow" />
            <Tooltip title={t('privilege.refresh')}>
              <Button size="small" type="text" icon={<IconRefresh size={14} />} onClick={() => void qc.invalidateQueries({ queryKey: ['privileges', tab.profileId] })} />
            </Tooltip>
          </div>
          <Checkbox checked={builtin} onChange={(e) => setBuiltin(e.target.checked)}>{t('privilege.showBuiltin')}</Checkbox>
        </div>
        <div style={{ flex: 1, overflow: 'auto' }}>
          {isLoading && <Spin style={{ margin: 24 }} />}
          {!isLoading && !list.length && <Empty image={Empty.PRESENTED_IMAGE_SIMPLE} description={t('privilege.none')} />}
          {list.map((p) => (
            <div
              key={p.id}
              className="hist-item"
              style={{ padding: '4px 12px', cursor: 'pointer', fontWeight: p.id === picked ? 600 : undefined }}
              onClick={() => {
                setPicked(p.id)
                useTabs.getState().setPrivilegesFocus(tab.id, undefined)
              }}
            >
              {p.name}
              {p.host && <span className="muted">@{p.host}</span>}
              <span className="muted" style={{ marginLeft: 6, fontSize: 'var(--fs-xs)' }}>{t(`privilege.kindName.${p.kind}`)}</span>
            </div>
          ))}
        </div>
      </div>
      <div style={{ flex: 1, minWidth: 0, display: 'flex', flexDirection: 'column' }}>
        <CoverageNote coverage={data?.coverage} />
        {error && <div className="err" style={{ padding: 12 }}>{t('privilege.error')}：{(error as Error).message}</div>}
        {focus ? (
          <ObjectAccessPanel tab={tab} principals={principals} onPick={(id) => { setPicked(id); useTabs.getState().setPrivilegesFocus(tab.id, undefined) }} />
        ) : current ? (
          <PrincipalDetail key={current.id} tab={tab} catalog={catalog} principal={current} principals={principals} roles={roles(current.id)} />
        ) : (
          <Empty style={{ marginTop: 64 }} image={Empty.PRESENTED_IMAGE_SIMPLE} description={t('privilege.empty')} />
        )}
      </div>
    </div>
  )
}
```

- [ ] **Step 3: 接到 Workspace**

`web/src/features/editor/Workspace.tsx`：

```tsx
// 账号权限：懒加载，不影响启动
const PrivilegesView = lazy(() => import('../privileges/PrivilegesView'))
```

图标：从 `@tabler/icons-react` 引入 `IconShieldLock`，在 `TabTitle` 里最前面加分支 `{tab.privileges ? (<IconShieldLock size={16} />) : tab.lint ? (`。

内容区最前面加：

```tsx
        (tab.privileges ? (
          <Suspense fallback={<Spin style={{ margin: 48 }} />}>
            <PrivilegesView key={tab.id} tab={tab} />
          </Suspense>
        ) : tab.lint?.view === 'library' ? (
```

检查 `Workspace.tsx` 后面的默认分支没有把带 `privileges` 的标签当查询标签处理；`useQueryActions` 的关闭逻辑对无会话的标签已适用（与 ER、规范检查标签相同）。

- [ ] **Step 4: 接到右键菜单**

`web/src/features/tree/ObjectTree.tsx`：

连接节点的菜单里，在 `sql-file` 项后加：

```tsx
          { key: 'privileges', label: t('tree.menu.privileges') },
```

`objectItems` 的最后，`return` 之前加：

```tsx
    if (target) items.push({ key: 'who-can-access', label: t('tree.menu.whoCanAccess') })
```

`onMenu` 里，`else if (key === 'view-data') openData(k)` 之后加：

```tsx
      else if (key === 'privileges') useTabs.getState().openPrivileges(k.profileId, t('privilege.title'))
      else if (key === 'who-can-access' && k.type === 'o') {
        useTabs.getState().openPrivileges(k.profileId, t('privilege.title'), { schema: k.schema, object: k.name })
      }
```

- [ ] **Step 5: 运行前端检查**

```bash
cd web && pnpm exec tsc --noEmit
pnpm exec eslint src/features/privileges src/features/editor src/features/tree
pnpm test
```

Expected: 全部通过，含 `locales.test.ts`。

- [ ] **Step 6: 用真实鼠标验证**

启动桌面程序或 `dblens-server` 连 VM 测试库。用真实鼠标操作并截图，不用合成事件：

1. 右键 MySQL 连接 → “用户与权限”：出现标签页，列表默认不含内置账号；
2. 勾选“显示内置”、切换用户/角色、输入搜索，列表随之变化；
3. 点一个用户：属性、直接授权、实际权限三个视图都有内容，实际权限显示路径；
4. 右键一张表 → “谁能访问”：同一标签页显示访问者，再次右键另一张表时不新开标签；
5. 点访问者名字跳到该用户，“返回列表”回到主体视图；
6. SQL Server 连接：能切换数据库；
7. 用低权限连接打开：顶部有范围不完整的提示，页面可用；
8. 切换深色主题和英文各看一遍；
9. 重启后标签恢复。

Expected: 全部符合。发现问题就地修复。

- [ ] **Step 7: 提交**

```bash
git add web/src
git commit -m "feat(web): M9a 用户与权限标签和右键入口"
```

---

### Task 8：文档和全量回归

**Files:**
- Create: `docs/notes/m9a-privileges.md`
- Modify: `docs/superpowers/specs/2026-10-30-m9-privileges-design.md`
- Modify: `README.md`（如列有功能清单）

- [ ] **Step 1: 写实现说明**

`docs/notes/m9a-privileges.md` 按 `docs/notes/m7a-lint.md` 的格式写：结构、接口、各库低权限的实测结果、SQL Server 2025 是否实测、发现的坑。

- [ ] **Step 2: 全量回归**

```bash
cargo fmt --all --check
cargo clippy --workspace --offline -- -D warnings
cargo test --workspace --offline -- --test-threads=1
cd web && pnpm exec tsc --noEmit && pnpm test && pnpm build
```

Expected: 全部通过。实连变量都设置时约 5 分钟以上。

- [ ] **Step 3: 提交，由用户决定合并**

```bash
git add docs README.md
git commit -m "docs: M9a 实现说明"
```

不合并、不推送，等用户确认。

---

## 覆盖对照

| 规格 | 任务 |
|---|---|
| 第 2 节 M9a 范围 | Task 1–7 |
| 第 3 节 各库来源 | Task 1 模板，Task 5 实连 |
| 第 4 节 数据模型 | Task 1 |
| 第 5 节 读取流程、权限不足 | Task 3，Task 5 Step 3 |
| 第 6 节 实际权限 | Task 2 |
| 第 7 节 接口 | Task 3、4 |
| 第 8 节 界面 | Task 6、7 |
| 第 10 节 错误处理 | Task 3（退级、缺失），Task 7（错误提示） |
| 第 12 节 测试 | Task 1–2 离线，Task 5 实连，Task 7 Step 6 界面 |
| 第 13 节 M9a 验收 | Task 5、Task 7 Step 6 |

M9b、M9c 和第 11 节团队版不在本计划内。
