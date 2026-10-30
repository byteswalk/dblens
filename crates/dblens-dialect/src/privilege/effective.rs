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
