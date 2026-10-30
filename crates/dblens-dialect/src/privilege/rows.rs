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
