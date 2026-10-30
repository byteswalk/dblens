//! 实际权限的计算（M9 设计文档第 6 节）。

use serde::Serialize;

use super::{GrantEffect, GrantScope, Membership, Principal, PrivilegeGrant};

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
