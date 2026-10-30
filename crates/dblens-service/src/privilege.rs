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
            match self
                .metadata
                .run(profile, section.key(), args, |d| d.privilege_query(section, tier, args))
                .await
            {
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
        let principals =
            match self.privilege_rows(profile, PrivSection::Principals, &args, &mut missing).await? {
                Some(rows) => privilege::principals(&rows)?,
                None => vec![],
            };
        let memberships =
            match self.privilege_rows(profile, PrivSection::Memberships, &args, &mut missing).await? {
                Some(rows) => privilege::memberships(&rows)?,
                None => vec![],
            };
        Ok(Base { dialect, principals, memberships, partial_revokes: caps.partial_revokes, missing })
    }

    /// 打开标签页：主体、成员关系。
    pub async fn privilege_overview(
        &self,
        profile_id: &str,
        catalog: Option<&str>,
    ) -> Result<PrivilegeOverview> {
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
            if let Some(rows) =
                self.privilege_rows(&profile, PrivSection::GrantsOf, &args, &mut group_missing).await?
            {
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
        let grants =
            match self.privilege_rows(&profile, PrivSection::GrantsOn, &args, &mut base.missing).await? {
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
