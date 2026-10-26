//! 连接密码的保存（设计文档 4.3 节）：优先放进系统钥匙串（Windows 凭据管理器、macOS Keychain、
//! Linux Secret Service）；系统没有可用的钥匙串时，用主密码经 Argon2id 派生密钥，以 ChaCha20-Poly1305
//! 加密后存入本地库。主密码只保存在内存中，程序重启后要重新输入。

use std::sync::{Arc, Mutex};

use argon2::Argon2;
use chacha20poly1305::aead::{Aead, Generate, Key, KeyInit};
use chacha20poly1305::{ChaCha20Poly1305, Nonce};
use dblens_core::{Error, Msg, Result, msg};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::store::Store;

/// 钥匙串中的服务名；每个连接配置一条，账户名为 `profile:<id>`。
const KEYRING_SERVICE: &str = "DBLens";
/// 用来验证主密码的已知明文。
const CHECK_PLAINTEXT: &[u8] = b"dblens-vault-v1";

/// 本地密码库的盐和主密码校验数据：(salt, check_nonce, check_cipher)。
type VaultRow = (Vec<u8>, Vec<u8>, Vec<u8>);

/// 密码存放在哪里。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SecretBackend {
    Keyring,
    Vault,
}

/// 密码库的状态，供界面决定是否提示设置或输入主密码。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecretStatus {
    pub backend: SecretBackend,
    /// 本地密码库是否已设置主密码（钥匙串模式下总为 false）。
    pub vault_initialized: bool,
    /// 当前能否读写密码。
    pub unlocked: bool,
}

pub struct Secrets {
    store: Arc<Store>,
    backend: SecretBackend,
    key: Mutex<Option<Key<ChaCha20Poly1305>>>,
}

fn crypto_err(what: Msg) -> Error {
    Error::internal(msg!("secret.crypto_failed", what = what))
}

fn keyring_entry(profile_id: &str) -> Result<keyring::Entry> {
    keyring::Entry::new(KEYRING_SERVICE, &format!("profile:{profile_id}"))
        .map_err(|e| Error::internal(msg!("secret.keyring_unavailable", error = e)))
}

/// 写入、读回、删除一条探测记录，确认系统钥匙串真的可用（Linux 无图形会话时常常不可用）。
fn keyring_works() -> bool {
    let Ok(entry) = keyring::Entry::new(KEYRING_SERVICE, "probe") else { return false };
    let ok = entry.set_password("probe").is_ok() && entry.get_password().is_ok_and(|p| p == "probe");
    let _ = entry.delete_credential();
    ok
}

fn derive_key(master: &str, salt: &[u8]) -> Result<Key<ChaCha20Poly1305>> {
    let mut key = Key::<ChaCha20Poly1305>::default();
    Argon2::default()
        .hash_password_into(master.as_bytes(), salt, &mut key)
        .map_err(|_| crypto_err(msg!("secret.op.derive")))?;
    Ok(key)
}

fn seal(key: &Key<ChaCha20Poly1305>, plaintext: &[u8]) -> Result<(Vec<u8>, Vec<u8>)> {
    let nonce = Nonce::generate();
    let cipher = ChaCha20Poly1305::new(key)
        .encrypt(&nonce, plaintext)
        .map_err(|_| crypto_err(msg!("secret.op.encrypt")))?;
    Ok((nonce.to_vec(), cipher))
}

fn open(key: &Key<ChaCha20Poly1305>, nonce: &[u8], cipher: &[u8]) -> Option<Vec<u8>> {
    let nonce = Nonce::try_from(nonce).ok()?;
    ChaCha20Poly1305::new(key).decrypt(&nonce, cipher).ok()
}

impl Secrets {
    /// 系统钥匙串可用时用钥匙串，否则用本地密码库。
    pub fn detect(store: Arc<Store>) -> Self {
        let backend = if keyring_works() { SecretBackend::Keyring } else { SecretBackend::Vault };
        Self::with_backend(store, backend)
    }

    pub fn with_backend(store: Arc<Store>, backend: SecretBackend) -> Self {
        Self { store, backend, key: Mutex::new(None) }
    }

    fn vault_row(&self) -> Result<Option<VaultRow>> {
        self.store.with(|c| {
            c.query_row("SELECT salt, check_nonce, check_cipher FROM vault WHERE id = 1", [], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })
            .optional()
        })
    }

    fn key(&self) -> Result<Key<ChaCha20Poly1305>> {
        self.key
            .lock()
            .map_err(|_| Error::internal(msg!("internal.lock_poisoned", lock = "vault")))?
            .ok_or(Error::Locked)
    }

    pub fn status(&self) -> Result<SecretStatus> {
        let unlocked = self
            .key
            .lock()
            .map_err(|_| Error::internal(msg!("internal.lock_poisoned", lock = "vault")))?
            .is_some();
        Ok(match self.backend {
            SecretBackend::Keyring => {
                SecretStatus { backend: self.backend, vault_initialized: false, unlocked: true }
            }
            SecretBackend::Vault => SecretStatus {
                backend: self.backend,
                vault_initialized: self.vault_row()?.is_some(),
                unlocked,
            },
        })
    }

    /// 首次设置主密码。已经设置过时报错（修改主密码暂不支持）。
    pub fn init_vault(&self, master: &str) -> Result<()> {
        if master.is_empty() {
            return Err(Error::invalid(msg!("secret.master_empty")));
        }
        if self.vault_row()?.is_some() {
            return Err(Error::invalid(msg!("secret.master_exists")));
        }
        let salt = Key::<ChaCha20Poly1305>::generate();
        let key = derive_key(master, &salt)?;
        let (nonce, cipher) = seal(&key, CHECK_PLAINTEXT)?;
        self.store.with(|c| {
            c.execute(
                "INSERT INTO vault (id, salt, check_nonce, check_cipher) VALUES (1, ?1, ?2, ?3)",
                params![salt.to_vec(), nonce, cipher],
            )
        })?;
        *self.key.lock().map_err(|_| Error::internal(msg!("internal.lock_poisoned", lock = "vault")))? =
            Some(key);
        Ok(())
    }

    /// 输入主密码解锁本地密码库。主密码错误时返回 `Error::Auth`。
    pub fn unlock(&self, master: &str) -> Result<()> {
        let (salt, nonce, cipher) =
            self.vault_row()?.ok_or_else(|| Error::invalid(msg!("secret.master_missing")))?;
        let key = derive_key(master, &salt)?;
        if open(&key, &nonce, &cipher).as_deref() != Some(CHECK_PLAINTEXT) {
            return Err(Error::Auth { message: msg!("secret.master_wrong") });
        }
        *self.key.lock().map_err(|_| Error::internal(msg!("internal.lock_poisoned", lock = "vault")))? =
            Some(key);
        Ok(())
    }

    pub fn lock(&self) -> Result<()> {
        *self.key.lock().map_err(|_| Error::internal(msg!("internal.lock_poisoned", lock = "vault")))? = None;
        Ok(())
    }

    /// 取连接配置的密码；没有保存过时返回 None。
    pub fn get(&self, profile_id: &str) -> Result<Option<String>> {
        match self.backend {
            SecretBackend::Keyring => match keyring_entry(profile_id)?.get_password() {
                Ok(p) => Ok(Some(p)),
                Err(keyring::Error::NoEntry) => Ok(None),
                Err(e) => Err(Error::internal(msg!("secret.keyring_read_failed", error = e))),
            },
            SecretBackend::Vault => {
                let key = self.key()?;
                let row: Option<(Vec<u8>, Vec<u8>)> = self.store.with(|c| {
                    c.query_row(
                        "SELECT nonce, cipher FROM secrets WHERE profile_id = ?1",
                        params![profile_id],
                        |r| Ok((r.get(0)?, r.get(1)?)),
                    )
                    .optional()
                })?;
                match row {
                    None => Ok(None),
                    Some((nonce, cipher)) => {
                        let plain = open(&key, &nonce, &cipher)
                            .ok_or_else(|| crypto_err(msg!("secret.op.decrypt")))?;
                        String::from_utf8(plain).map(Some).map_err(|_| crypto_err(msg!("secret.op.decrypt")))
                    }
                }
            }
        }
    }

    pub fn set(&self, profile_id: &str, password: &str) -> Result<()> {
        match self.backend {
            SecretBackend::Keyring => keyring_entry(profile_id)?
                .set_password(password)
                .map_err(|e| Error::internal(msg!("secret.keyring_write_failed", error = e))),
            SecretBackend::Vault => {
                let (nonce, cipher) = seal(&self.key()?, password.as_bytes())?;
                self.store.with(|c| {
                    c.execute(
                        "INSERT INTO secrets (profile_id, nonce, cipher) VALUES (?1, ?2, ?3)
                         ON CONFLICT(profile_id) DO UPDATE SET nonce = excluded.nonce, cipher = excluded.cipher",
                        params![profile_id, nonce, cipher],
                    )
                })?;
                Ok(())
            }
        }
    }

    /// 删除密码；本来就没有时不报错。
    pub fn delete(&self, profile_id: &str) -> Result<()> {
        match self.backend {
            SecretBackend::Keyring => match keyring_entry(profile_id)?.delete_credential() {
                Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
                Err(e) => Err(Error::internal(msg!("secret.keyring_delete_failed", error = e))),
            },
            SecretBackend::Vault => {
                self.store
                    .with(|c| c.execute("DELETE FROM secrets WHERE profile_id = ?1", params![profile_id]))?;
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::tests::input;

    fn vault() -> (Arc<Store>, Secrets, String) {
        let store = Arc::new(Store::open_in_memory().unwrap());
        let id = store.create_profile(input("p")).unwrap().id;
        let s = Secrets::with_backend(store.clone(), SecretBackend::Vault);
        (store, s, id)
    }

    #[test]
    fn vault_requires_master_password() {
        let (_, s, id) = vault();
        assert_eq!(
            s.status().unwrap(),
            SecretStatus { backend: SecretBackend::Vault, vault_initialized: false, unlocked: false }
        );
        assert!(matches!(s.set(&id, "pw"), Err(Error::Locked)));
        assert!(matches!(s.unlock("x"), Err(Error::Invalid { .. })));
        assert!(matches!(s.init_vault(""), Err(Error::Invalid { .. })));
        s.init_vault("主密码").unwrap();
        assert!(s.status().unwrap().unlocked);
        assert!(matches!(s.init_vault("again"), Err(Error::Invalid { .. })));
    }

    #[test]
    fn vault_round_trip_and_wrong_master() {
        let (store, s, id) = vault();
        s.init_vault("主密码").unwrap();
        assert_eq!(s.get(&id).unwrap(), None);
        s.set(&id, "p@ss 中文").unwrap();
        s.set(&id, "p@ss 中文2").unwrap();
        assert_eq!(s.get(&id).unwrap().as_deref(), Some("p@ss 中文2"));

        // 密文里不含明文
        let raw: Vec<u8> =
            store.with(|c| c.query_row("SELECT cipher FROM secrets", [], |r| r.get(0))).unwrap();
        assert!(!raw.windows(4).any(|w| w == b"p@ss"));

        // 重新打开：要先解锁；主密码错误时报认证失败
        let again = Secrets::with_backend(store.clone(), SecretBackend::Vault);
        assert!(again.status().unwrap().vault_initialized);
        assert!(matches!(again.get(&id), Err(Error::Locked)));
        assert!(matches!(again.unlock("wrong"), Err(Error::Auth { .. })));
        again.unlock("主密码").unwrap();
        assert_eq!(again.get(&id).unwrap().as_deref(), Some("p@ss 中文2"));
        again.lock().unwrap();
        assert!(matches!(again.get(&id), Err(Error::Locked)));

        again.unlock("主密码").unwrap();
        again.delete(&id).unwrap();
        again.delete(&id).unwrap();
        assert_eq!(again.get(&id).unwrap(), None);
    }

    #[test]
    fn deleting_profile_removes_vault_secret() {
        let (store, s, id) = vault();
        s.init_vault("m").unwrap();
        s.set(&id, "pw").unwrap();
        store.delete_profile(&id).unwrap();
        let n: i64 = store.with(|c| c.query_row("SELECT COUNT(*) FROM secrets", [], |r| r.get(0))).unwrap();
        assert_eq!(n, 0);
    }

    /// 真实的系统钥匙串。没有可用钥匙串的环境（如无图形会话的 Linux CI）跳过。
    #[test]
    fn keyring_round_trip() {
        if !keyring_works() {
            eprintln!("跳过：系统钥匙串不可用");
            return;
        }
        let store = Arc::new(Store::open_in_memory().unwrap());
        let s = Secrets::with_backend(store, SecretBackend::Keyring);
        let id = format!("test-{}", uuid::Uuid::new_v4());
        assert_eq!(s.get(&id).unwrap(), None);
        s.set(&id, "钥匙串密码").unwrap();
        assert_eq!(s.get(&id).unwrap().as_deref(), Some("钥匙串密码"));
        s.delete(&id).unwrap();
        s.delete(&id).unwrap();
        assert_eq!(s.get(&id).unwrap(), None);
    }
}
