//! 数据库账号与权限（M9 设计文档）。读操作走元数据连接，不进执行历史；每次都从库里读，不用缓存。
//! 修改走专用路径（见 `privilege_apply`）：逐条执行，失败即停，密码不进历史、日志和错误信息。
//! 主查询报 SQL 错误（多半是没有权限或版本缺视图）时退到 `@limited` 段；两段都读不到的，记入 `Coverage`，
//! 不能显示成“没有授权”。

use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use dblens_core::{Error, Msg, Result, msg};
use dblens_dialect::meta::{MetaArgs, MetaRows};
use dblens_dialect::privilege::{
    self, EffectiveContext, EffectivePrivilege, Membership, ObjectAccess, Principal, PrivCaps, PrivChange,
    PrivSection, PrivStatement, PrivTier, PrivilegeGrant,
};
use dblens_dialect::{Dialect, Family};
use serde::Serialize;

use crate::history::NewHistory;
use crate::import::exec;
use crate::policy::{Decision, ExecContext, Outcome, classify};
use crate::profile::{Environment, Profile, driver_name, variant_code};
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
    pub caps: PrivCaps,
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

/// 修改前的预览。语句里的密码都是 `***`。
#[derive(Debug, Clone, Serialize)]
pub struct PrivPreview {
    pub statements: Vec<String>,
    pub profile_name: String,
    pub production: bool,
    /// 生产连接上要输入的主体名。
    pub confirm_name: Option<String>,
    /// 改的是当前连接自己的账号。
    pub own_account: bool,
    /// 要删除的主体持有的对象（最多 50 个）。
    pub owned: Vec<String>,
    /// 执行策略要确认的原因。
    pub confirm: Vec<Msg>,
}

/// 执行结果：`done` 是已成功的语句，失败时 `error` 是脱敏后的错误，`failed` 是失败的语句。
#[derive(Debug, Clone, Serialize)]
pub struct PrivApplyResult {
    pub done: Vec<String>,
    pub failed: Option<String>,
    pub error: Option<String>,
}

fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64)
}

/// 错误信息里去掉执行语句的原文和密码：数据库可能回显语句。
fn scrub(message: &str, statement: &PrivStatement, password: Option<&str>) -> String {
    let mut out = message.replace(&statement.sql, &statement.masked);
    if let Some(p) = password.filter(|p| !p.is_empty()) {
        out = out.replace(p, "***");
    }
    privilege::redact_sql(&out).into_owned()
}

fn change_password(change: &PrivChange) -> Option<&str> {
    match change {
        PrivChange::CreateUser { password, .. } | PrivChange::SetPassword { password, .. } => {
            Some(password.expose())
        }
        PrivChange::CreateRole { password, .. } => password.as_ref().map(|p| p.expose()),
        _ => None,
    }
}

fn missing_msg(section: PrivSection) -> Msg {
    match section {
        PrivSection::Caps => msg!("privilege.missing.caps"),
        PrivSection::Principals => msg!("privilege.missing.principals"),
        PrivSection::Memberships => msg!("privilege.missing.memberships"),
        PrivSection::GrantsOf => msg!("privilege.missing.grants"),
        PrivSection::GrantsOn | PrivSection::Owned => msg!("privilege.missing.object"),
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
            caps: base.dialect.privilege_caps(),
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

    /// 生成语句，不执行。
    pub async fn privilege_preview(
        &self,
        profile_id: &str,
        catalog: Option<&str>,
        change: &PrivChange,
    ) -> Result<PrivPreview> {
        let profile = self.scoped_profile(profile_id, catalog)?;
        let dialect = self.metadata.dialect(&profile).await?;
        let statements = dialect.privilege_statements(change)?;
        let masked: Vec<String> = statements.iter().map(|s| s.masked.clone()).collect();
        let sqls: Vec<String> = masked.clone();
        let (denied, confirm) = self.check_policy(&profile, dialect, &sqls).await;
        if let Some(reason) = denied {
            return Err(Error::Denied { reason });
        }
        let production = profile.input.environment == Environment::Prod;
        let mut owned = Vec::new();
        if let Some(p) = change.dropping() {
            let args = MetaArgs { name: Some(p.name.clone()), ..Default::default() };
            let mut ignored = Vec::new();
            // 只是提示：读不到不影响预览
            if let Ok(Some(rows)) =
                self.privilege_rows(&profile, PrivSection::Owned, &args, &mut ignored).await
            {
                owned = privilege::owned(&rows).unwrap_or_default();
            }
        }
        Ok(PrivPreview {
            statements: masked,
            profile_name: profile.input.name.clone(),
            production,
            confirm_name: change.high_risk_name().filter(|_| production).map(str::to_string),
            own_account: change.subject().is_some_and(|s| s.eq_ignore_ascii_case(&profile.input.user)),
            owned,
            confirm,
        })
    }

    /// 执行已确认的修改：逐条执行，失败即停，历史里只有脱敏后的语句。
    pub async fn privilege_apply(
        &self,
        profile_id: &str,
        catalog: Option<&str>,
        change: &PrivChange,
        confirm_name: Option<&str>,
    ) -> Result<PrivApplyResult> {
        let profile = self.scoped_profile(profile_id, catalog)?;
        let dialect = self.metadata.dialect(&profile).await?;
        let statements = dialect.privilege_statements(change)?;
        if profile.input.environment == Environment::Prod
            && let Some(name) = change.high_risk_name()
            && confirm_name != Some(name)
        {
            return Err(Error::invalid(msg!("privilege.change.confirm_name")));
        }
        let ctx =
            ExecContext { profile_id: profile.id.clone(), environment: profile.input.environment, dialect };
        let policy = self.sessions.policy();
        let infos: Vec<_> = statements.iter().map(|s| classify(&dialect, &s.masked)).collect();
        for info in &infos {
            if let Decision::Deny { reason } = policy.before_execute(&ctx, info).await {
                return Err(Error::Denied { reason });
            }
        }
        let password = self.secrets.get(&profile.id)?.unwrap_or_default();
        let (mut conn, _) = self.registry.connect(&profile, password).await?;
        let secret = change_password(change);
        let mut result = PrivApplyResult { done: Vec::new(), failed: None, error: None };
        for (statement, info) in statements.iter().zip(&infos) {
            let (at, started) = (now_ms(), Instant::now());
            let outcome = exec(&mut conn, &statement.sql, &[]).await;
            let error = outcome.as_ref().err().map(|e| scrub(&e.to_string(), statement, secret));
            self.history.record(NewHistory {
                at,
                profile_id: profile.id.clone(),
                profile_name: profile.input.name.clone(),
                driver: driver_name(profile.input.driver).to_string(),
                variant: Some(variant_code(dialect.variant())),
                schema: profile.input.database.clone(),
                sql: statement.masked.clone(),
                elapsed_ms: Some(started.elapsed().as_millis() as u64),
                rows: None,
                error: error.clone(),
            });
            let summary = match (&outcome, &error) {
                (Ok(n), _) => Outcome::Succeeded { affected: Some(*n) },
                (_, e) => Outcome::Failed { message: e.clone().unwrap_or_default() },
            };
            policy.after_execute(&ctx, info, &summary).await;
            if error.is_some() {
                result.failed = Some(statement.masked.clone());
                result.error = error;
                break;
            }
            result.done.push(statement.masked.clone());
        }
        conn.close().await.ok();
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use dblens_core::msg;
    use dblens_dialect::privilege::{PrincipalRef, Secret};

    use crate::fake::{FakeDriver, Log};
    use crate::history::{HistoryLog, HistoryQuery};
    use crate::policy::DefaultPolicy;
    use crate::profile::ProfileInput;
    use crate::profile::tests::input;
    use crate::registry::DriverRegistry;
    use crate::secret::{SecretBackend, Secrets};
    use crate::store::Store;

    use super::*;

    const SENTINEL: &str = "S3ntinel!pw";

    fn service(environment: Environment) -> (Service, String, Log, Arc<Store>) {
        let store = Arc::new(Store::open_in_memory().unwrap());
        let secrets = Arc::new(Secrets::with_backend(store.clone(), SecretBackend::Vault));
        secrets.init_vault("master").unwrap();
        let log: Log = Arc::default();
        let registry = Arc::new(DriverRegistry::with_drivers(
            store.clone(),
            vec![Box::new(FakeDriver { log: log.clone() })],
        ));
        let s = Service::with_history(
            store.clone(),
            secrets,
            registry,
            Arc::new(DefaultPolicy),
            HistoryLog::inline(store.clone()),
        );
        let p = s.create_profile(ProfileInput { environment, ..input("c") }, Some("pw")).unwrap();
        (s, p.id, log, store)
    }

    fn set_password() -> PrivChange {
        PrivChange::SetPassword {
            principal: PrincipalRef {
                name: "u".into(),
                host: Some("%".into()),
                kind: PrincipalKind::User,
                level: PrincipalLevel::Server,
            },
            password: Secret::new(SENTINEL),
        }
    }

    fn drop_user() -> PrivChange {
        let PrivChange::SetPassword { principal, .. } = set_password() else { unreachable!() };
        PrivChange::Drop { principal, cascade: false }
    }

    #[tokio::test]
    async fn the_password_reaches_the_database_but_not_the_preview_or_history() {
        let (s, id, log, store) = service(Environment::Dev);
        let preview = s.privilege_preview(&id, None, &set_password()).await.unwrap();
        assert!(!format!("{preview:?}").contains(SENTINEL));
        let r = s.privilege_apply(&id, None, &set_password(), None).await.unwrap();
        assert_eq!(r.error, None);
        assert_eq!(r.done.len(), 1);
        assert!(!format!("{r:?}").contains(SENTINEL));
        assert!(log.lock().unwrap().iter().any(|l| l.contains(SENTINEL)), "数据库要收到密码");
        let items = store.history(&HistoryQuery::default()).unwrap();
        assert_eq!(items.len(), 1);
        assert!(format!("{:?}", items[0]).contains("***"));
        assert!(!format!("{:?}", items[0]).contains(SENTINEL));
    }

    #[test]
    fn database_errors_are_scrubbed() {
        let st = dblens_dialect::Dialect::new(dblens_dialect::Variant::MySql)
            .privilege_statements(&set_password())
            .unwrap()
            .remove(0);
        let raw = format!("near '{}' and {SENTINEL}", st.sql);
        let got = scrub(&raw, &st, Some(SENTINEL));
        assert!(!got.contains(SENTINEL), "{got}");
    }

    #[tokio::test]
    async fn production_needs_the_name_for_high_risk_changes() {
        let (s, id, _log, _store) = service(Environment::Prod);
        let preview = s.privilege_preview(&id, None, &drop_user()).await.unwrap();
        assert!(preview.production);
        assert_eq!(preview.confirm_name.as_deref(), Some("u"));
        assert!(s.privilege_apply(&id, None, &drop_user(), None).await.is_err());
        assert!(s.privilege_apply(&id, None, &drop_user(), Some("x")).await.is_err());
        let r = s.privilege_apply(&id, None, &drop_user(), Some("u")).await.unwrap();
        assert_eq!(r.error, None);
    }

    #[tokio::test]
    async fn development_needs_no_name() {
        let (s, id, _log, _store) = service(Environment::Dev);
        let preview = s.privilege_preview(&id, None, &drop_user()).await.unwrap();
        assert_eq!(preview.confirm_name, None);
        assert_eq!(s.privilege_apply(&id, None, &drop_user(), None).await.unwrap().error, None);
    }

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
