//! 账号与权限的修改（M9 设计文档第 9 节）：按方言生成语句。`masked` 是可以显示、落盘的文本，密码写成 `***`。

use dblens_core::{Error, Result, msg};
use serde::{Deserialize, Serialize};

use super::{GrantScope, GrantTarget, PrincipalKind, PrincipalLevel};
use crate::{Dialect, Family, Variant};

const MASK: &str = "***";

/// 密码：只进要执行的语句，`Debug` 不输出内容，也不能序列化。
#[derive(Clone, Deserialize)]
#[serde(transparent)]
pub struct Secret(String);

impl Secret {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(MASK)
    }
}

/// 界面选中的主体。
#[derive(Debug, Clone, Deserialize)]
pub struct PrincipalRef {
    pub name: String,
    #[serde(default)]
    pub host: Option<String>,
    pub kind: PrincipalKind,
    pub level: PrincipalLevel,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum PrivChange {
    CreateUser {
        name: String,
        #[serde(default)]
        host: Option<String>,
        password: Secret,
    },
    CreateRole {
        name: String,
        level: PrincipalLevel,
        #[serde(default)]
        password: Option<Secret>,
    },
    Drop {
        principal: PrincipalRef,
        #[serde(default)]
        cascade: bool,
    },
    SetPassword {
        principal: PrincipalRef,
        password: Secret,
    },
    SetLocked {
        principal: PrincipalRef,
        locked: bool,
    },
    Grant {
        grantee: PrincipalRef,
        scope: GrantScope,
        target: GrantTarget,
        privilege: String,
        #[serde(default)]
        with_grant: bool,
    },
    Revoke {
        grantee: PrincipalRef,
        scope: GrantScope,
        target: GrantTarget,
        privilege: String,
        /// 这条授权带转授权时，SQL Server 要 CASCADE。
        #[serde(default)]
        grantable: bool,
    },
    AddRole {
        member: PrincipalRef,
        role: PrincipalRef,
        #[serde(default)]
        admin: bool,
    },
    RemoveRole {
        member: PrincipalRef,
        role: PrincipalRef,
    },
}

/// 一条语句：`sql` 只用于执行，`masked` 用于预览、历史和报错。
#[derive(Clone, PartialEq, Eq)]
pub struct PrivStatement {
    pub sql: String,
    pub masked: String,
}

impl PrivStatement {
    fn plain(sql: String) -> Self {
        Self { masked: sql.clone(), sql }
    }

    fn with_password(template: &str, literal: &str) -> Self {
        Self { sql: template.replace('\u{1}', literal), masked: template.replace('\u{1}', MASK) }
    }
}

impl std::fmt::Debug for PrivStatement {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.masked)
    }
}

/// 方言支持的操作，不支持的不显示入口。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct PrivCaps {
    pub lock: bool,
    /// 建角色必须给密码（openGauss）。
    pub role_password: bool,
    /// 用户带 host（MySQL 系）。
    pub host: bool,
    pub drop_cascade: bool,
    pub global_grant: bool,
    pub database_grant: bool,
    pub schema_grant: bool,
}

/// 管理员角色或全局最高权限，生产连接上要输入主体名才能授予、回收。
pub fn is_admin_privilege(name: &str) -> bool {
    let n = name.trim().to_ascii_uppercase();
    matches!(
        n.as_str(),
        "DBA"
            | "SYSDBA"
            | "SYSADMIN"
            | "SECURITYADMIN"
            | "DB_OWNER"
            | "DB_SECURITYADMIN"
            | "SUPER"
            | "SUPERUSER"
            | "ALL"
            | "ALL PRIVILEGES"
            | "CONTROL SERVER"
            | "CONTROL"
            | "GRANT ANY PRIVILEGE"
            | "GRANT ANY ROLE"
    )
}

impl PrivChange {
    /// 确认框要输入的主体名；普通操作返回 None。
    pub fn high_risk_name(&self) -> Option<&str> {
        match self {
            Self::Drop { principal, .. } => Some(&principal.name),
            Self::Grant { grantee, privilege, .. } | Self::Revoke { grantee, privilege, .. }
                if is_admin_privilege(privilege) =>
            {
                Some(&grantee.name)
            }
            Self::AddRole { member, role, .. } | Self::RemoveRole { member, role }
                if is_admin_privilege(&role.name) =>
            {
                Some(&member.name)
            }
            _ => None,
        }
    }

    /// 操作的是哪个主体，用来提示“修改的是当前连接自己的账号”。
    pub fn subject(&self) -> Option<&str> {
        match self {
            Self::CreateUser { .. } | Self::CreateRole { .. } => None,
            Self::Drop { principal, .. }
            | Self::SetPassword { principal, .. }
            | Self::SetLocked { principal, .. } => Some(&principal.name),
            Self::Grant { grantee, .. } | Self::Revoke { grantee, .. } => Some(&grantee.name),
            Self::AddRole { member, .. } | Self::RemoveRole { member, .. } => Some(&member.name),
        }
    }

    /// 要删除的主体，用来列出它持有的对象。
    pub fn dropping(&self) -> Option<&PrincipalRef> {
        match self {
            Self::Drop { principal, .. } => Some(principal),
            _ => None,
        }
    }
}

fn unsupported() -> Error {
    Error::invalid(msg!("privilege.change.unsupported"))
}

fn required(value: &str) -> Result<&str> {
    if value.trim().is_empty() {
        return Err(Error::invalid(msg!("privilege.change.name_required")));
    }
    Ok(value)
}

/// 权限名不是标识符，不能引用：只放行字母、数字、空格、下划线和逗号，防止拼出别的语句。
fn privilege_text(p: &str) -> Result<&str> {
    let valid =
        !p.trim().is_empty() && p.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, ' ' | '_' | ','));
    if valid { Ok(p.trim()) } else { Err(Error::invalid(msg!("privilege.change.bad_privilege"))) }
}

fn sql_string(family: Family, value: &str) -> String {
    match family {
        Family::MySql => format!("'{}'", value.replace('\\', "\\\\").replace('\'', "''")),
        Family::SqlServer => format!("N'{}'", value.replace('\'', "''")),
        _ => format!("'{}'", value.replace('\'', "''")),
    }
}

impl Dialect {
    pub fn privilege_caps(&self) -> PrivCaps {
        let family = self.family();
        let gauss = matches!(self.variant(), Variant::OpenGauss | Variant::GaussDb);
        PrivCaps {
            lock: !matches!(family, Family::Postgres | Family::Informix) || gauss,
            role_password: gauss,
            host: family == Family::MySql,
            drop_cascade: matches!(family, Family::Postgres | Family::Oracle | Family::Dameng),
            global_grant: !matches!(family, Family::Postgres | Family::Informix),
            database_grant: matches!(family, Family::Postgres | Family::SqlServer),
            schema_grant: matches!(family, Family::MySql | Family::Postgres | Family::SqlServer),
        }
    }

    pub fn privilege_statements(&self, change: &PrivChange) -> Result<Vec<PrivStatement>> {
        if self.family() == Family::Informix {
            return Err(unsupported());
        }
        match change {
            PrivChange::CreateUser { name, host, password } => {
                self.create_user(required(name)?, host.as_deref(), password)
            }
            PrivChange::CreateRole { name, level, password } => {
                self.create_role(required(name)?, *level, password.as_ref())
            }
            PrivChange::Drop { principal, cascade } => self.drop_principal(principal, *cascade),
            PrivChange::SetPassword { principal, password } => self.set_password(principal, password),
            PrivChange::SetLocked { principal, locked } => self.set_locked(principal, *locked),
            PrivChange::Grant { grantee, scope, target, privilege, with_grant } => {
                let on = self.grant_clause(*scope, target, privilege_text(privilege)?, true)?;
                let to = self.grantee_name(grantee);
                let option = match (*with_grant, self.family(), *scope) {
                    (false, ..) => "",
                    (true, Family::Oracle | Family::Dameng, GrantScope::Global) => " WITH ADMIN OPTION",
                    (true, ..) => " WITH GRANT OPTION",
                };
                Ok(vec![PrivStatement::plain(format!("GRANT {on} TO {to}{option}"))])
            }
            PrivChange::Revoke { grantee, scope, target, privilege, grantable } => {
                let on = self.grant_clause(*scope, target, privilege_text(privilege)?, false)?;
                let from = self.grantee_name(grantee);
                let cascade = if *grantable && matches!(self.family(), Family::SqlServer | Family::Dameng) {
                    " CASCADE"
                } else {
                    ""
                };
                let mut out = vec![PrivStatement::plain(format!("REVOKE {on} FROM {from}{cascade}"))];
                // MySQL 的转授权单独存放，只回收权限时账号上还留着一条 USAGE
                if *grantable && self.family() == Family::MySql {
                    let on = self.grant_clause(*scope, target, "GRANT OPTION", false)?;
                    out.push(PrivStatement::plain(format!("REVOKE {on} FROM {from}")));
                }
                Ok(out)
            }
            PrivChange::AddRole { member, role, admin } => self.add_role(member, role, *admin),
            PrivChange::RemoveRole { member, role } => self.remove_role(member, role),
        }
    }

    /// 密码的字面量。Oracle、达梦的 `IDENTIFIED BY` 用双引号，内容没法转义。
    fn password_literal(&self, password: &Secret) -> Result<String> {
        let value = password.expose();
        if value.is_empty() {
            return Err(Error::invalid(msg!("privilege.change.password_required")));
        }
        match self.family() {
            Family::Oracle | Family::Dameng if value.contains('"') => {
                Err(Error::invalid(msg!("privilege.change.password_quote")))
            }
            Family::Oracle | Family::Dameng => Ok(format!("\"{value}\"")),
            family => Ok(sql_string(family, value)),
        }
    }

    /// MySQL 系的账号写成 `'user'@'host'`；MariaDB 的角色没有 host。
    fn account(&self, name: &str, host: Option<&str>) -> String {
        let user = sql_string(Family::MySql, name);
        match host.filter(|h| !h.is_empty()) {
            Some(h) => format!("{user}@{}", sql_string(Family::MySql, h)),
            None => user,
        }
    }

    fn grantee_name(&self, p: &PrincipalRef) -> String {
        match self.family() {
            Family::MySql => self.account(&p.name, p.host.as_deref()),
            Family::Postgres | Family::SqlServer => self.quote_ident(&p.name),
            _ => self.ident(&p.name),
        }
    }

    fn create_user(&self, name: &str, host: Option<&str>, password: &Secret) -> Result<Vec<PrivStatement>> {
        let lit = self.password_literal(password)?;
        let q = self.quote_ident(name);
        Ok(match self.family() {
            Family::MySql => {
                let account = self.account(name, Some(host.unwrap_or("%")));
                vec![PrivStatement::with_password(
                    &format!("CREATE USER {account} IDENTIFIED BY \u{1}"),
                    &lit,
                )]
            }
            Family::Postgres if self.privilege_caps().role_password => {
                vec![PrivStatement::with_password(&format!("CREATE USER {q} PASSWORD \u{1}"), &lit)]
            }
            Family::Postgres => {
                vec![PrivStatement::with_password(&format!("CREATE ROLE {q} LOGIN PASSWORD \u{1}"), &lit)]
            }
            Family::SqlServer => vec![
                PrivStatement::with_password(&format!("CREATE LOGIN {q} WITH PASSWORD = \u{1}"), &lit),
                PrivStatement::plain(format!("CREATE USER {q} FOR LOGIN {q}")),
            ],
            _ => {
                let id = self.ident(name);
                vec![PrivStatement::with_password(&format!("CREATE USER {id} IDENTIFIED BY \u{1}"), &lit)]
            }
        })
    }

    fn create_role(
        &self,
        name: &str,
        level: PrincipalLevel,
        password: Option<&Secret>,
    ) -> Result<Vec<PrivStatement>> {
        Ok(match self.family() {
            Family::MySql => vec![PrivStatement::plain(format!("CREATE ROLE {}", self.account(name, None)))],
            Family::Postgres if self.privilege_caps().role_password => {
                let password =
                    password.ok_or_else(|| Error::invalid(msg!("privilege.change.password_required")))?;
                let (q, lit) = (self.quote_ident(name), self.password_literal(password)?);
                vec![PrivStatement::with_password(&format!("CREATE ROLE {q} PASSWORD \u{1}"), &lit)]
            }
            Family::Postgres => vec![PrivStatement::plain(format!("CREATE ROLE {}", self.quote_ident(name)))],
            Family::SqlServer if level == PrincipalLevel::Server => {
                vec![PrivStatement::plain(format!("CREATE SERVER ROLE {}", self.quote_ident(name)))]
            }
            Family::SqlServer => {
                vec![PrivStatement::plain(format!("CREATE ROLE {}", self.quote_ident(name)))]
            }
            _ => vec![PrivStatement::plain(format!("CREATE ROLE {}", self.ident(name)))],
        })
    }

    fn drop_principal(&self, p: &PrincipalRef, cascade: bool) -> Result<Vec<PrivStatement>> {
        let name = required(&p.name)?;
        let role = p.kind == PrincipalKind::Role;
        let sql = match self.family() {
            Family::MySql => {
                let what = if role { "ROLE" } else { "USER" };
                vec![format!("DROP {what} {}", self.account(name, p.host.as_deref()))]
            }
            Family::Postgres if self.privilege_caps().role_password => {
                let what = if role { "ROLE" } else { "USER" };
                let tail = if cascade && !role { " CASCADE" } else { "" };
                vec![format!("DROP {what} {}{tail}", self.quote_ident(name))]
            }
            Family::Postgres => {
                let q = self.quote_ident(name);
                let mut out = Vec::new();
                if cascade {
                    out.push(format!("DROP OWNED BY {q}"));
                }
                out.push(format!("DROP ROLE {q}"));
                out
            }
            Family::SqlServer => {
                let q = self.quote_ident(name);
                vec![match (p.kind, p.level) {
                    (PrincipalKind::Role, PrincipalLevel::Server) => format!("DROP SERVER ROLE {q}"),
                    (PrincipalKind::Role, _) => format!("DROP ROLE {q}"),
                    (_, PrincipalLevel::Server) => format!("DROP LOGIN {q}"),
                    _ => format!("DROP USER {q}"),
                }]
            }
            _ if role => vec![format!("DROP ROLE {}", self.ident(name))],
            _ => {
                let tail = if cascade { " CASCADE" } else { "" };
                vec![format!("DROP USER {}{tail}", self.ident(name))]
            }
        };
        Ok(sql.into_iter().map(PrivStatement::plain).collect())
    }

    fn set_password(&self, p: &PrincipalRef, password: &Secret) -> Result<Vec<PrivStatement>> {
        let lit = self.password_literal(password)?;
        let template = match self.family() {
            Family::MySql => {
                format!("ALTER USER {} IDENTIFIED BY \u{1}", self.account(&p.name, p.host.as_deref()))
            }
            Family::Postgres => format!("ALTER USER {} PASSWORD \u{1}", self.quote_ident(&p.name)),
            Family::SqlServer if p.kind == PrincipalKind::Login => {
                format!("ALTER LOGIN {} WITH PASSWORD = \u{1}", self.quote_ident(&p.name))
            }
            Family::SqlServer => return Err(unsupported()),
            _ => format!("ALTER USER {} IDENTIFIED BY \u{1}", self.ident(&p.name)),
        };
        Ok(vec![PrivStatement::with_password(&template, &lit)])
    }

    fn set_locked(&self, p: &PrincipalRef, locked: bool) -> Result<Vec<PrivStatement>> {
        if !self.privilege_caps().lock {
            return Err(unsupported());
        }
        let word = if locked { "LOCK" } else { "UNLOCK" };
        let sql = match self.family() {
            Family::MySql => {
                format!("ALTER USER {} ACCOUNT {word}", self.account(&p.name, p.host.as_deref()))
            }
            Family::Postgres => format!("ALTER USER {} ACCOUNT {word}", self.quote_ident(&p.name)),
            Family::SqlServer if p.kind == PrincipalKind::Login => {
                let state = if locked { "DISABLE" } else { "ENABLE" };
                format!("ALTER LOGIN {} {state}", self.quote_ident(&p.name))
            }
            Family::SqlServer => return Err(unsupported()),
            _ => format!("ALTER USER {} ACCOUNT {word}", self.ident(&p.name)),
        };
        Ok(vec![PrivStatement::plain(sql)])
    }

    /// `权限 ON 目标`；`granting` 为 false 表示回收（Oracle 不能按列回收，回收整张表的这项权限）。
    fn grant_clause(
        &self,
        scope: GrantScope,
        t: &GrantTarget,
        privilege: &str,
        granting: bool,
    ) -> Result<String> {
        let need = |v: &Option<String>| v.clone().filter(|s| !s.is_empty()).ok_or_else(unsupported);
        let qi = |s: &str| self.quote_ident(s);
        let kind = t.object_kind.as_deref().unwrap_or("table");
        Ok(match (self.family(), scope) {
            (Family::MySql, GrantScope::Global) => format!("{privilege} ON *.*"),
            (Family::MySql, GrantScope::Schema) => format!("{privilege} ON {}.*", qi(&need(&t.schema)?)),
            (Family::MySql, GrantScope::Object | GrantScope::Column) => {
                let prefix = match kind {
                    "function" => "FUNCTION ",
                    "procedure" => "PROCEDURE ",
                    _ => "",
                };
                let on = format!("{prefix}{}.{}", qi(&need(&t.schema)?), qi(&need(&t.object)?));
                match scope {
                    GrantScope::Column => format!("{privilege} ({}) ON {on}", qi(&need(&t.column)?)),
                    _ => format!("{privilege} ON {on}"),
                }
            }
            (Family::Postgres, GrantScope::Database) => {
                format!("{privilege} ON DATABASE {}", qi(&need(&t.database)?))
            }
            (Family::Postgres, GrantScope::Schema) => {
                format!("{privilege} ON SCHEMA {}", qi(&need(&t.schema)?))
            }
            (Family::Postgres, GrantScope::Object | GrantScope::Column) => {
                let on = format!("{}.{}", qi(&need(&t.schema)?), qi(&need(&t.object)?));
                match (scope, kind) {
                    (GrantScope::Column, _) => {
                        format!("{privilege} ({}) ON TABLE {on}", qi(&need(&t.column)?))
                    }
                    (_, "sequence") => format!("{privilege} ON SEQUENCE {on}"),
                    (_, "function") => format!("{privilege} ON FUNCTION {on}"),
                    (_, "procedure") => format!("{privilege} ON PROCEDURE {on}"),
                    _ => format!("{privilege} ON TABLE {on}"),
                }
            }
            (Family::Oracle | Family::Dameng, GrantScope::Global) => privilege.to_string(),
            (Family::Oracle | Family::Dameng, GrantScope::Object | GrantScope::Column) => {
                let id = |s: &str| self.quote_ident(s);
                let on = format!("{}.{}", id(&need(&t.schema)?), id(&need(&t.object)?));
                let by_column = scope == GrantScope::Column && (granting || self.family() == Family::Dameng);
                if by_column {
                    format!("{privilege} ({}) ON {on}", id(&need(&t.column)?))
                } else {
                    format!("{privilege} ON {on}")
                }
            }
            (Family::SqlServer, GrantScope::Global | GrantScope::Database) => privilege.to_string(),
            (Family::SqlServer, GrantScope::Schema) => {
                format!("{privilege} ON SCHEMA::{}", qi(&need(&t.schema)?))
            }
            (Family::SqlServer, GrantScope::Object) => {
                format!("{privilege} ON OBJECT::{}.{}", qi(&need(&t.schema)?), qi(&need(&t.object)?))
            }
            (Family::SqlServer, GrantScope::Column) => format!(
                "{privilege} ON OBJECT::{}.{} ({})",
                qi(&need(&t.schema)?),
                qi(&need(&t.object)?),
                qi(&need(&t.column)?)
            ),
            _ => return Err(unsupported()),
        })
    }

    fn add_role(
        &self,
        member: &PrincipalRef,
        role: &PrincipalRef,
        admin: bool,
    ) -> Result<Vec<PrivStatement>> {
        let (m, r) = (self.grantee_name(member), self.grantee_name(role));
        let sql = match self.family() {
            Family::SqlServer if admin => return Err(unsupported()),
            Family::SqlServer if role.level == PrincipalLevel::Server => {
                format!("ALTER SERVER ROLE {r} ADD MEMBER {m}")
            }
            Family::SqlServer => format!("ALTER ROLE {r} ADD MEMBER {m}"),
            _ => {
                let option = if admin { " WITH ADMIN OPTION" } else { "" };
                format!("GRANT {r} TO {m}{option}")
            }
        };
        Ok(vec![PrivStatement::plain(sql)])
    }

    fn remove_role(&self, member: &PrincipalRef, role: &PrincipalRef) -> Result<Vec<PrivStatement>> {
        let (m, r) = (self.grantee_name(member), self.grantee_name(role));
        let sql = match self.family() {
            Family::SqlServer if role.level == PrincipalLevel::Server => {
                format!("ALTER SERVER ROLE {r} DROP MEMBER {m}")
            }
            Family::SqlServer => format!("ALTER ROLE {r} DROP MEMBER {m}"),
            _ => format!("REVOKE {r} FROM {m}"),
        };
        Ok(vec![PrivStatement::plain(sql)])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user(name: &str, host: Option<&str>) -> PrincipalRef {
        PrincipalRef {
            name: name.into(),
            host: host.map(Into::into),
            kind: PrincipalKind::User,
            level: PrincipalLevel::Server,
        }
    }

    fn role(name: &str) -> PrincipalRef {
        PrincipalRef { kind: PrincipalKind::Role, ..user(name, None) }
    }

    fn table() -> GrantTarget {
        GrantTarget { schema: Some("s".into()), object: Some("t".into()), ..Default::default() }
    }

    fn masked(v: Variant, c: &PrivChange) -> Vec<String> {
        Dialect::new(v).privilege_statements(c).unwrap().into_iter().map(|s| s.masked).collect()
    }

    fn create(name: &str) -> PrivChange {
        PrivChange::CreateUser { name: name.into(), host: None, password: Secret::new("pa'ss") }
    }

    #[test]
    fn create_user_by_dialect() {
        assert_eq!(masked(Variant::MySql, &create("u")), ["CREATE USER 'u'@'%' IDENTIFIED BY ***"]);
        assert_eq!(masked(Variant::Postgres, &create("u")), ["CREATE ROLE \"u\" LOGIN PASSWORD ***"]);
        assert_eq!(masked(Variant::OpenGauss, &create("u")), ["CREATE USER \"u\" PASSWORD ***"]);
        assert_eq!(masked(Variant::Oracle, &create("U")), ["CREATE USER U IDENTIFIED BY ***"]);
        assert_eq!(
            masked(Variant::SqlServer, &create("u")),
            ["CREATE LOGIN [u] WITH PASSWORD = ***", "CREATE USER [u] FOR LOGIN [u]"]
        );
    }

    #[test]
    fn password_goes_only_into_sql() {
        for v in [Variant::MySql, Variant::Postgres, Variant::SqlServer, Variant::Oracle] {
            let st = Dialect::new(v).privilege_statements(&create("U")).unwrap();
            assert!(st[0].sql.contains("pa"), "{v:?}");
            assert!(st.iter().all(|s| !s.masked.contains("pa'")), "{v:?}");
            assert!(!format!("{st:?}").contains("pa'"), "{v:?}");
        }
        assert!(!format!("{:?}", create("u")).contains("pa'"));
    }

    #[test]
    fn password_literal_is_escaped() {
        let sql = |v| Dialect::new(v).privilege_statements(&create("U")).unwrap().remove(0).sql;
        assert!(sql(Variant::MySql).ends_with("'pa''ss'"));
        assert!(sql(Variant::SqlServer).ends_with("N'pa''ss'"));
        assert!(sql(Variant::Oracle).ends_with("\"pa'ss\""));
        let quote = PrivChange::CreateUser { name: "U".into(), host: None, password: Secret::new("a\"b") };
        assert!(Dialect::new(Variant::Oracle).privilege_statements(&quote).is_err());
        let empty = PrivChange::CreateUser { name: "U".into(), host: None, password: Secret::new("") };
        assert!(Dialect::new(Variant::MySql).privilege_statements(&empty).is_err());
    }

    #[test]
    fn drop_does_not_cascade_unless_asked() {
        let drop = |cascade| PrivChange::Drop { principal: user("U", None), cascade };
        assert_eq!(masked(Variant::Oracle, &drop(false)), ["DROP USER U"]);
        assert_eq!(masked(Variant::Oracle, &drop(true)), ["DROP USER U CASCADE"]);
        assert_eq!(masked(Variant::Postgres, &drop(false)), ["DROP ROLE \"U\""]);
        assert_eq!(masked(Variant::Postgres, &drop(true)), ["DROP OWNED BY \"U\"", "DROP ROLE \"U\""]);
        assert_eq!(masked(Variant::OpenGauss, &drop(true)), ["DROP USER \"U\" CASCADE"]);
        let login = PrincipalRef { kind: PrincipalKind::Login, ..user("U", None) };
        assert_eq!(
            masked(Variant::SqlServer, &PrivChange::Drop { principal: login, cascade: false }),
            ["DROP LOGIN [U]"]
        );
    }

    #[test]
    fn lock_only_where_supported() {
        let lock = PrivChange::SetLocked { principal: user("u", Some("%")), locked: true };
        assert_eq!(masked(Variant::MySql, &lock), ["ALTER USER 'u'@'%' ACCOUNT LOCK"]);
        assert!(Dialect::new(Variant::Postgres).privilege_statements(&lock).is_err());
        assert!(!Dialect::new(Variant::Postgres).privilege_caps().lock);
        assert!(Dialect::new(Variant::OpenGauss).privilege_caps().lock);
        let login = PrincipalRef { kind: PrincipalKind::Login, ..user("u", None) };
        let c = PrivChange::SetLocked { principal: login, locked: false };
        assert_eq!(masked(Variant::SqlServer, &c), ["ALTER LOGIN [u] ENABLE"]);
    }

    #[test]
    fn grant_and_revoke_targets() {
        let g = |scope, target, privilege: &str, with_grant| PrivChange::Grant {
            grantee: user("u", Some("%")),
            scope,
            target,
            privilege: privilege.into(),
            with_grant,
        };
        let c = g(GrantScope::Object, table(), "SELECT", true);
        assert_eq!(masked(Variant::MySql, &c), ["GRANT SELECT ON `s`.`t` TO 'u'@'%' WITH GRANT OPTION"]);
        assert_eq!(
            masked(Variant::SqlServer, &c),
            ["GRANT SELECT ON OBJECT::[s].[t] TO [u] WITH GRANT OPTION"]
        );
        assert_eq!(
            masked(Variant::Postgres, &c),
            ["GRANT SELECT ON TABLE \"s\".\"t\" TO \"u\" WITH GRANT OPTION"]
        );
        let global = g(GrantScope::Global, GrantTarget::default(), "CREATE SESSION", true);
        assert_eq!(masked(Variant::Oracle, &global), ["GRANT CREATE SESSION TO \"u\" WITH ADMIN OPTION"]);
        assert_eq!(
            masked(Variant::MySql, &g(GrantScope::Global, GrantTarget::default(), "RELOAD", false)),
            ["GRANT RELOAD ON *.* TO 'u'@'%'"]
        );
        let col = GrantTarget { column: Some("c".into()), ..table() };
        let c = PrivChange::Revoke {
            grantee: user("U", None),
            scope: GrantScope::Column,
            target: col,
            privilege: "UPDATE".into(),
            grantable: true,
        };
        assert_eq!(masked(Variant::Oracle, &c), ["REVOKE UPDATE ON \"s\".\"t\" FROM U"]);
        assert_eq!(
            masked(Variant::SqlServer, &c),
            ["REVOKE UPDATE ON OBJECT::[s].[t] ([c]) FROM [U] CASCADE"]
        );
    }

    #[test]
    fn unknown_scope_and_bad_privilege_are_refused() {
        let c = PrivChange::Grant {
            grantee: user("u", None),
            scope: GrantScope::Schema,
            target: table(),
            privilege: "SELECT".into(),
            with_grant: false,
        };
        assert!(Dialect::new(Variant::Oracle).privilege_statements(&c).is_err());
        let c = PrivChange::Grant {
            grantee: user("u", None),
            scope: GrantScope::Global,
            target: GrantTarget::default(),
            privilege: "SELECT ON *.* TO x; DROP".into(),
            with_grant: false,
        };
        assert!(Dialect::new(Variant::MySql).privilege_statements(&c).is_err());
        assert!(Dialect::new(Variant::Gbase8s).privilege_statements(&create("u")).is_err());
    }

    #[test]
    fn roles_by_dialect() {
        let add = PrivChange::AddRole { member: user("u", Some("%")), role: role("r"), admin: true };
        assert_eq!(masked(Variant::MySql, &add), ["GRANT 'r' TO 'u'@'%' WITH ADMIN OPTION"]);
        let plain = PrivChange::AddRole { member: user("u", None), role: role("r"), admin: false };
        assert_eq!(masked(Variant::SqlServer, &plain), ["ALTER SERVER ROLE [r] ADD MEMBER [u]"]);
        let rm = PrivChange::RemoveRole {
            member: user("u", None),
            role: PrincipalRef { level: PrincipalLevel::Database, ..role("r") },
        };
        assert_eq!(masked(Variant::SqlServer, &rm), ["ALTER ROLE [r] DROP MEMBER [u]"]);
        assert_eq!(masked(Variant::Postgres, &rm), ["REVOKE \"r\" FROM \"u\""]);
    }

    #[test]
    fn high_risk_covers_drop_and_admin_roles() {
        let drop = PrivChange::Drop { principal: user("u", None), cascade: false };
        assert_eq!(drop.high_risk_name(), Some("u"));
        let add = |r: &str| PrivChange::AddRole { member: user("u", None), role: role(r), admin: false };
        assert_eq!(add("dba").high_risk_name(), Some("u"));
        assert_eq!(add("readers").high_risk_name(), None);
        assert_eq!(create("u").high_risk_name(), None);
    }
}
