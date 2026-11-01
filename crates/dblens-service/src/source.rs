//! 凭据和连接配置从哪里来（审计版设计 3.2 节）。单机版取本机的钥匙串或密码库、本地库；审计版由服务端托管。

use async_trait::async_trait;
use dblens_core::Result;

use crate::profile::Profile;
use crate::secret::Secrets;
use crate::store::Store;

#[async_trait]
pub trait CredentialSource: Send + Sync {
    async fn password(&self, profile_id: &str) -> Result<Option<String>>;
}

#[async_trait]
impl CredentialSource for Secrets {
    async fn password(&self, profile_id: &str) -> Result<Option<String>> {
        self.get(profile_id)
    }
}

/// 只读：连接的增删改只有单机版的路由用。
pub trait ProfileSource: Send + Sync {
    fn list(&self) -> Result<Vec<Profile>>;
    fn get(&self, id: &str) -> Result<Profile>;
}

impl ProfileSource for Store {
    fn list(&self) -> Result<Vec<Profile>> {
        self.list_profiles()
    }

    fn get(&self, id: &str) -> Result<Profile> {
        self.profile(id)
    }
}
