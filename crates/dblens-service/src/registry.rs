//! 驱动注册表：编译进来的驱动、它们是否可用，以及 Oracle/达梦原生客户端库目录的确认。
//!
//! 原生库目录必须由用户确认后才使用（写入设置，只接受绝对路径）。自动扫描到的目录只作为建议展示，
//! 不直接加载：常见安装位置可能被其他本地用户预先放置恶意 DLL（CWE-427）。

use std::path::{Path, PathBuf};
use std::sync::Arc;

use dblens_core::{Error, Msg, Result, msg};
use dblens_dialect::meta::{MetaArgs, MetaKey, MetaRows};
use dblens_dialect::{Dialect, Variant};
use dblens_driver::native::{self, NativeLib};
use dblens_driver::{ConnectParams, Connection, Driver, DriverId, collect_all};
use serde::{Deserialize, Serialize};

use crate::profile::Profile;
use crate::store::Store;

/// 某个驱动的可用状态，供界面展示和安装指引。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DriverStatus {
    pub id: DriverId,
    pub name: Msg,
    /// 是否编译进了本程序。
    pub compiled: bool,
    /// 能否直接连接：编译进来了，并且（需要原生库时）用户已确认库目录且目录中确实有库文件。
    pub available: bool,
    /// 是否需要原生客户端库（Oracle、达梦）。
    pub needs_native_lib: bool,
    /// 用户确认过的库目录。
    pub lib_dir: Option<PathBuf>,
    /// 尚未确认时，自动查找到的建议目录。
    pub suggested_lib_dir: Option<PathBuf>,
    /// 不可用时的原因和安装指引。
    pub hint: Option<Msg>,
}

/// 连接上的数据库：版本信息和识别出的方言。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerSummary {
    pub product: String,
    pub version: String,
    pub variant: Variant,
    pub variant_name: Msg,
    /// 编码后的版本号（见 `dblens_dialect::parse_version`）。
    pub version_code: Option<u32>,
}

fn native_lib(id: DriverId) -> Option<(NativeLib, &'static str)> {
    match id {
        DriverId::Oracle => Some((NativeLib::OracleClient, "oracle_client_lib_dir")),
        DriverId::Dameng => Some((NativeLib::DamengDpi, "dpi_lib_dir")),
        DriverId::Mysql | DriverId::Postgres => None,
    }
}

fn setting_key(option: &str) -> String {
    format!("native.{option}")
}

fn display_name(id: DriverId) -> Msg {
    match id {
        DriverId::Mysql => Msg::raw("MySQL / MariaDB"),
        DriverId::Postgres => Msg::raw("PostgreSQL"),
        DriverId::Oracle => Msg::raw("Oracle"),
        DriverId::Dameng => msg!("variant.dameng"),
    }
}

pub struct DriverRegistry {
    store: Arc<Store>,
    drivers: Vec<Box<dyn Driver>>,
}

impl DriverRegistry {
    /// 本程序编译进来的全部驱动（由 Cargo feature 控制）。
    pub fn new(store: Arc<Store>) -> Self {
        let drivers: Vec<Box<dyn Driver>> = vec![
            #[cfg(feature = "mysql")]
            Box::new(dblens_driver_mysql::MySqlDriver),
            #[cfg(feature = "postgres")]
            Box::new(dblens_driver_postgres::PgDriver),
            #[cfg(feature = "oracle")]
            Box::new(dblens_driver_oracle::OracleDriver),
            #[cfg(feature = "dameng")]
            Box::new(dblens_driver_dameng::DmDriver),
        ];
        Self::with_drivers(store, drivers)
    }

    /// 指定驱动集合，供测试注入假驱动。
    pub fn with_drivers(store: Arc<Store>, drivers: Vec<Box<dyn Driver>>) -> Self {
        Self { store, drivers }
    }

    pub fn driver(&self, id: DriverId) -> Result<&dyn Driver> {
        self.drivers.iter().find(|d| d.id() == id).map(|d| d.as_ref()).ok_or_else(|| {
            Error::DriverUnavailable { hint: msg!("registry.not_compiled", driver = display_name(id)) }
        })
    }

    /// 用户确认过的原生库目录。
    pub fn lib_dir(&self, id: DriverId) -> Result<Option<PathBuf>> {
        let Some((_, option)) = native_lib(id) else { return Ok(None) };
        Ok(self.store.setting(&setting_key(option))?.map(PathBuf::from))
    }

    /// 确认（或用 None 清除）原生库目录。只接受绝对路径，目录中必须有对应的库文件。
    pub fn set_lib_dir(&self, id: DriverId, dir: Option<&Path>) -> Result<()> {
        let (lib, option) = native_lib(id)
            .ok_or_else(|| Error::invalid(msg!("registry.no_native_lib", driver = display_name(id))))?;
        if let Some(dir) = dir {
            if !dir.is_absolute() {
                return Err(Error::invalid(msg!("registry.dir_not_absolute", dir = dir.display())));
            }
            if !dir.join(lib.file_name()).is_file() {
                return Err(Error::invalid(msg!(
                    "registry.lib_missing",
                    dir = dir.display(),
                    file = lib.file_name()
                )));
            }
        }
        let value = dir.map(|d| d.to_string_lossy().into_owned());
        self.store.set_setting(&setting_key(option), value.as_deref())
    }

    pub fn statuses(&self) -> Result<Vec<DriverStatus>> {
        [DriverId::Mysql, DriverId::Postgres, DriverId::Oracle, DriverId::Dameng]
            .into_iter()
            .map(|id| self.status(id))
            .collect()
    }

    pub fn status(&self, id: DriverId) -> Result<DriverStatus> {
        let compiled = self.drivers.iter().any(|d| d.id() == id);
        let mut s = DriverStatus {
            id,
            name: display_name(id),
            compiled,
            available: compiled,
            needs_native_lib: native_lib(id).is_some(),
            lib_dir: None,
            suggested_lib_dir: None,
            hint: (!compiled).then(|| msg!("registry.not_compiled", driver = display_name(id))),
        };
        if let (true, Some((lib, _))) = (compiled, native_lib(id)) {
            match self.lib_dir(id)? {
                Some(dir) if dir.join(lib.file_name()).is_file() => s.lib_dir = Some(dir),
                Some(dir) => {
                    s.available = false;
                    s.hint = Some(msg!("registry.lib_gone", dir = dir.display(), file = lib.file_name()));
                    s.lib_dir = Some(dir);
                }
                None => {
                    s.available = false;
                    match native::locate(lib, None) {
                        Ok(found) => {
                            s.hint = Some(msg!("registry.lib_found", dir = found.display()));
                            s.suggested_lib_dir = Some(found);
                        }
                        Err(e) => s.hint = Some(e.to_msg()),
                    }
                }
            }
        }
        Ok(s)
    }

    /// 组成连接参数：用户确认过的原生库目录会覆盖配置里的同名选项；需要原生库但尚未确认时报错。
    pub fn connect_params(&self, profile: &Profile, password: String) -> Result<ConnectParams> {
        let id = profile.input.driver;
        let mut params = profile.params(password);
        if let Some((_, option)) = native_lib(id) {
            let status = self.status(id)?;
            match (status.available, status.lib_dir) {
                (true, Some(dir)) => {
                    params.options.insert(option.to_string(), dir.to_string_lossy().into_owned());
                }
                _ => {
                    return Err(Error::DriverUnavailable {
                        hint: status.hint.unwrap_or_else(|| msg!("registry.confirm_dir")),
                    });
                }
            }
        }
        Ok(params)
    }

    /// 连接并识别方言：`server_info` 识别兼容库和版本，再用 `variant_probe` 细化（如 PolarDB）。
    pub async fn connect(
        &self,
        profile: &Profile,
        password: String,
    ) -> Result<(Box<dyn Connection>, Dialect)> {
        let params = self.connect_params(profile, password)?;
        let driver = self.driver(profile.input.driver)?;
        let mut conn = driver.connect(&params).await?;
        let info = conn.server_info().await?;
        let mut dialect = Dialect::detect(driver.id(), &info);
        if let Some(q) = dialect.meta_query(MetaKey::VariantProbe, &MetaArgs::default())? {
            let mut stream = conn.execute(&q.sql, &q.params).await?;
            let rows = MetaRows::from_events(collect_all(stream.as_mut(), 100).await?)?;
            drop(stream);
            dialect = dialect.refine(&rows);
        }
        // 记下识别结果供界面显示图标；写不进去不影响连接
        if let Err(e) = self.store.set_profile_variant(&profile.id, dialect.variant()) {
            tracing::warn!("记录连接 {} 的数据库变体失败：{e}", profile.id);
        }
        Ok((conn, dialect))
    }

    /// 测试连接：连上、读出服务器信息后立即断开。
    pub async fn test_connection(&self, profile: &Profile, password: String) -> Result<ServerSummary> {
        let (mut conn, dialect) = self.connect(profile, password).await?;
        let info = conn.server_info().await?;
        conn.close().await?;
        Ok(ServerSummary {
            product: info.product,
            version: info.version,
            variant: dialect.variant(),
            variant_name: dialect.variant().display_name(),
            version_code: dialect.version(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::tests::input;

    fn registry() -> (tempfile::TempDir, DriverRegistry) {
        (tempfile::tempdir().unwrap(), DriverRegistry::new(Arc::new(Store::open_in_memory().unwrap())))
    }

    #[test]
    fn pure_rust_drivers_are_available_without_native_libs() {
        let (_, r) = registry();
        let s = r.status(DriverId::Postgres).unwrap();
        assert!(s.compiled && s.available && !s.needs_native_lib && s.hint.is_none());
        assert_eq!(r.statuses().unwrap().len(), 4);
        assert!(matches!(r.set_lib_dir(DriverId::Mysql, None), Err(Error::Invalid { .. })));
    }

    #[test]
    fn native_lib_dir_must_be_confirmed_absolute_and_contain_the_library() {
        let (dir, r) = registry();
        // 未确认：不可用，连接参数报驱动不可用
        let s = r.status(DriverId::Dameng).unwrap();
        assert!(s.compiled && s.needs_native_lib && !s.available && s.lib_dir.is_none());
        let p = Profile {
            id: "1".into(),
            input: ProfileInput { driver: DriverId::Dameng, ..input("dm") },
            variant: None,
        };
        assert!(matches!(r.connect_params(&p, "pw".into()), Err(Error::DriverUnavailable { .. })));

        assert!(matches!(
            r.set_lib_dir(DriverId::Dameng, Some(Path::new("relative/dir"))),
            Err(Error::Invalid { .. })
        ));
        assert!(matches!(r.set_lib_dir(DriverId::Dameng, Some(dir.path())), Err(Error::Invalid { .. })));

        // 放一个同名文件冒充库文件，只检查路径逻辑
        std::fs::write(dir.path().join(NativeLib::DamengDpi.file_name()), b"").unwrap();
        r.set_lib_dir(DriverId::Dameng, Some(dir.path())).unwrap();
        let s = r.status(DriverId::Dameng).unwrap();
        assert!(s.available && s.lib_dir.as_deref() == Some(dir.path()), "{s:?}");
        let params = r.connect_params(&p, "pw".into()).unwrap();
        assert_eq!(params.options["dpi_lib_dir"], dir.path().to_string_lossy());

        // 库文件被删掉后：不可用，并提示重新选择
        std::fs::remove_file(dir.path().join(NativeLib::DamengDpi.file_name())).unwrap();
        let s = r.status(DriverId::Dameng).unwrap();
        assert!(!s.available && s.hint.unwrap().render(dblens_core::Lang::ZhCn).contains("重新选择"));

        r.set_lib_dir(DriverId::Dameng, None).unwrap();
        assert_eq!(r.lib_dir(DriverId::Dameng).unwrap(), None);
    }

    #[test]
    fn missing_driver_is_reported() {
        let r = DriverRegistry::with_drivers(Arc::new(Store::open_in_memory().unwrap()), vec![]);
        assert!(matches!(r.driver(DriverId::Mysql), Err(Error::DriverUnavailable { .. })));
        let s = r.status(DriverId::Oracle).unwrap();
        assert!(!s.compiled && !s.available);
    }

    use crate::profile::ProfileInput;
}
