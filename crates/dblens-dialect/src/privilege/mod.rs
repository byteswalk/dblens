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
    EffectiveContext, EffectivePrivilege, EffectiveState, ObjectAccess, PrivNote, Reach, effective,
    reachable, who_can_access,
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
    pub fn privilege_query(
        &self,
        section: PrivSection,
        tier: PrivTier,
        args: &MetaArgs,
    ) -> Result<Option<MetaQuery>> {
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
            (Family::MySql, Variant::MariaDb) => &["'PUBLIC'@''"],
            (Family::MySql, _) => &[],
            _ => &["PUBLIC"],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::meta::template::ALL_VARIANTS;
    use crate::{Family, Variant};

    fn args() -> MetaArgs {
        MetaArgs {
            schema: Some("s".into()),
            table: Some("t".into()),
            name: Some("u".into()),
            ..Default::default()
        }
    }

    #[test]
    fn every_variant_has_the_required_sections() {
        for &v in &ALL_VARIANTS {
            let d = Dialect::new(v);
            for s in [
                PrivSection::Principals,
                PrivSection::Memberships,
                PrivSection::GrantsOf,
                PrivSection::GrantsOn,
            ] {
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
            Dialect::new(v)
                .privilege_query(PrivSection::GrantsOf, PrivTier::Full, &args())
                .unwrap()
                .unwrap()
                .sql
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
        assert!(
            Dialect::new(Variant::Postgres)
                .privilege_query(PrivSection::Principals, PrivTier::Limited, &args())
                .unwrap()
                .is_none()
        );
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
        assert_eq!(Dialect::new(Variant::MariaDb).public_grantees(), ["'PUBLIC'@''"]);
        assert_eq!(Dialect::new(Variant::SqlServer).public_grantees(), ["db:public", "server:public"]);
        assert_eq!(Dialect::new(Variant::SqlServer).family(), Family::SqlServer);
    }
}
