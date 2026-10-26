//! 服务层入口：把本地库、密码、驱动、会话、元数据组装在一起，供 API 层调用。

use std::path::Path;
use std::sync::Arc;

use dblens_core::{Lang, Result, set_default_lang};

use crate::history::HistoryLog;
use crate::metadata::{MetadataCache, MetadataService};
use crate::policy::{DefaultPolicy, Policy};
use crate::profile::{Profile, ProfileInput};
use crate::registry::{DriverRegistry, ServerSummary};
use crate::secret::Secrets;
use crate::session::{SessionId, SessionInfo, SessionManager};
use crate::store::Store;

/// 界面语言设置：`auto`（跟随系统）、`zh-CN`、`en`。
pub const LANG_SETTING: &str = "ui.lang";

pub struct Service {
    pub store: Arc<Store>,
    pub secrets: Arc<Secrets>,
    pub registry: Arc<DriverRegistry>,
    pub sessions: SessionManager,
    pub metadata: MetadataService,
}

impl Service {
    /// 打开数据目录下的本地库，检测系统钥匙串，使用默认执行策略。
    pub fn open(data_dir: &Path) -> Result<Self> {
        let store = Arc::new(Store::open(data_dir)?);
        let secrets = Arc::new(Secrets::detect(store.clone()));
        let registry = Arc::new(DriverRegistry::new(store.clone()));
        let service = Self::with_parts(store, secrets, registry, Arc::new(DefaultPolicy));
        service.apply_lang_setting()?;
        Ok(service)
    }

    /// 按设置 `ui.lang` 设定进程的默认语言。
    pub fn apply_lang_setting(&self) -> Result<()> {
        let value = self.store.setting(LANG_SETTING)?;
        set_default_lang(value.as_deref().and_then(Lang::from_setting));
        Ok(())
    }

    /// 指定各个组成部分，供测试和团队版替换策略使用。历史由后台线程写入。
    pub fn with_parts(
        store: Arc<Store>,
        secrets: Arc<Secrets>,
        registry: Arc<DriverRegistry>,
        policy: Arc<dyn Policy>,
    ) -> Self {
        let history = HistoryLog::start(store.clone());
        Self::with_history(store, secrets, registry, policy, history)
    }

    /// 同 `with_parts`，另外指定历史的写法（测试中用 `HistoryLog::inline` 写完立即可查）。
    pub fn with_history(
        store: Arc<Store>,
        secrets: Arc<Secrets>,
        registry: Arc<DriverRegistry>,
        policy: Arc<dyn Policy>,
        history: HistoryLog,
    ) -> Self {
        let cache = Arc::new(MetadataCache::new());
        Self {
            sessions: SessionManager::new(policy, cache.clone(), history),
            metadata: MetadataService::new(registry.clone(), secrets.clone(), cache),
            store,
            secrets,
            registry,
        }
    }

    pub fn profiles(&self) -> Result<Vec<Profile>> {
        self.store.list_profiles()
    }

    pub fn profile(&self, id: &str) -> Result<Profile> {
        self.store.profile(id)
    }

    /// 新建连接配置；给出密码时一并保存。
    pub fn create_profile(&self, input: ProfileInput, password: Option<&str>) -> Result<Profile> {
        let p = self.store.create_profile(input)?;
        if let Some(pw) = password
            && let Err(e) = self.secrets.set(&p.id, pw)
        {
            // 密码存不进去时不留下半成品配置
            self.store.delete_profile(&p.id)?;
            return Err(e);
        }
        Ok(p)
    }

    /// 修改连接配置。已建立的元数据连接和缓存作废；已打开的编辑器会话不受影响。
    pub async fn update_profile(&self, id: &str, input: ProfileInput) -> Result<Profile> {
        let p = self.store.update_profile(id, input)?;
        self.metadata.forget(id).await?;
        Ok(p)
    }

    /// 设置（`Some`）或清除（`None`）保存的密码。
    pub async fn set_password(&self, id: &str, password: Option<&str>) -> Result<()> {
        self.store.profile(id)?;
        match password {
            Some(pw) => self.secrets.set(id, pw)?,
            None => self.secrets.delete(id)?,
        }
        self.metadata.forget(id).await
    }

    pub async fn delete_profile(&self, id: &str) -> Result<()> {
        self.store.profile(id)?;
        self.secrets.delete(id)?;
        self.metadata.forget(id).await?;
        self.store.delete_profile(id)
    }

    /// 测试一个（可能尚未保存的）连接配置。
    pub async fn test_connection(&self, input: ProfileInput, password: &str) -> Result<ServerSummary> {
        let p = Profile { id: String::new(), input, variant: None };
        self.registry.test_connection(&p, password.to_string()).await
    }

    /// 为编辑器标签页打开一个会话（独占一个连接）。
    pub async fn open_session(&self, profile_id: &str) -> Result<SessionInfo> {
        let p = self.store.profile(profile_id)?;
        let password = self.secrets.get(profile_id)?.unwrap_or_default();
        let (conn, dialect) = self.registry.connect(&p, password).await?;
        let id = self.sessions.open(&p, conn, dialect)?;
        // 读不到当前 schema 不影响使用，工具栏显示为未选择
        let _ = self.sessions.refresh_schema(id).await;
        self.sessions.info(id)
    }

    /// 连接断开后，经用户确认重新连接。
    pub async fn reconnect_session(&self, id: SessionId) -> Result<SessionInfo> {
        let info = self.sessions.info(id)?;
        let p = self.store.profile(&info.profile_id)?;
        let password = self.secrets.get(&p.id)?.unwrap_or_default();
        let (conn, dialect) = self.registry.connect(&p, password).await?;
        self.sessions.replace_connection(id, conn, dialect).await?;
        self.sessions.info(id)
    }
}

#[cfg(test)]
mod tests {
    use dblens_core::Error;
    use dblens_dialect::Variant;

    use super::*;
    use crate::fake::{FakeDriver, Log};
    use crate::profile::tests::input;
    use crate::secret::SecretBackend;
    use crate::session::SessionEvent;

    fn service() -> (Service, Log) {
        let store = Arc::new(Store::open_in_memory().unwrap());
        let secrets = Arc::new(Secrets::with_backend(store.clone(), SecretBackend::Vault));
        secrets.init_vault("master").unwrap();
        let log: Log = Arc::default();
        let registry = Arc::new(DriverRegistry::with_drivers(
            store.clone(),
            vec![Box::new(FakeDriver { log: log.clone() })],
        ));
        (Service::with_parts(store, secrets, registry, Arc::new(DefaultPolicy)), log)
    }

    #[tokio::test]
    async fn profile_lifecycle_keeps_password_with_profile() {
        let (s, _) = service();
        let p = s.create_profile(input("pg"), Some("secret")).unwrap();
        assert_eq!(s.secrets.get(&p.id).unwrap().as_deref(), Some("secret"));
        assert_eq!(s.profiles().unwrap().len(), 1);
        s.set_password(&p.id, Some("new")).await.unwrap();
        assert_eq!(s.secrets.get(&p.id).unwrap().as_deref(), Some("new"));
        s.set_password(&p.id, None).await.unwrap();
        assert_eq!(s.secrets.get(&p.id).unwrap(), None);
        s.delete_profile(&p.id).await.unwrap();
        assert!(matches!(s.profile(&p.id), Err(Error::NotFound { .. })));
        assert!(matches!(s.set_password(&p.id, Some("x")).await, Err(Error::NotFound { .. })));
    }

    #[tokio::test]
    async fn locked_vault_keeps_no_half_created_profile() {
        let (s, _) = service();
        s.secrets.lock().unwrap();
        assert!(matches!(s.create_profile(input("pg"), Some("pw")), Err(Error::Locked)));
        assert!(s.profiles().unwrap().is_empty());
        // 不保存密码时可以创建
        s.create_profile(input("pg"), None).unwrap();
    }

    #[tokio::test]
    async fn test_connection_and_session_round_trip() {
        let (s, log) = service();
        let summary = s.test_connection(input("pg"), "pw").await.unwrap();
        assert_eq!((summary.variant, summary.version_code), (Variant::Postgres, Some(170000)));
        assert!(matches!(s.test_connection(input("pg"), "wrong").await, Err(Error::Auth { .. })));

        let p = s.create_profile(input("pg"), Some("pw")).unwrap();
        assert_eq!(p.variant, None);
        let info = s.open_session(&p.id).await.unwrap();
        assert_eq!(
            s.store.profile(&p.id).unwrap().variant,
            Some(Variant::Postgres),
            "连接时记下识别出的变体"
        );
        assert_eq!((info.profile_id.as_str(), info.autocommit), (p.id.as_str(), true));
        s.sessions.execute(info.id, "ROWS 1", false).await.unwrap();
        let mut last = None;
        while let Some(ev) = s.sessions.next(info.id, 10).await.unwrap() {
            last = Some(ev);
        }
        assert_eq!(last, Some(SessionEvent::Done));
        let reconnected = s.reconnect_session(info.id).await.unwrap();
        assert!(!reconnected.broken);
        s.sessions.close(info.id, false).await.unwrap();
        let log = log.lock().unwrap();
        assert_eq!(
            log.iter().filter(|l| *l == "connect").count(),
            3,
            "测试连接、会话、重连各一次（密码错误的那次没连上）：{log:?}"
        );
    }
}
