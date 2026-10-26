//! 连接配置：保存在本地库中，不含密码（密码见 `secret`）。

use std::collections::BTreeMap;
use std::time::Duration;

use dblens_core::{Error, Result, msg};
use dblens_driver::{ConnectParams, DriverId};
use rusqlite::{OptionalExtension, Row, params};
use serde::{Deserialize, Serialize};

use crate::store::Store;

/// 连接所属的环境。生产环境在界面上有醒目标识，执行危险语句前要确认（`DefaultPolicy`）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Environment {
    #[default]
    None,
    Dev,
    Test,
    Prod,
}

impl Environment {
    fn as_str(self) -> &'static str {
        match self {
            Environment::None => "none",
            Environment::Dev => "dev",
            Environment::Test => "test",
            Environment::Prod => "prod",
        }
    }

    fn parse(s: &str) -> Environment {
        match s {
            "dev" => Environment::Dev,
            "test" => Environment::Test,
            "prod" => Environment::Prod,
            _ => Environment::None,
        }
    }
}

/// 创建或修改连接配置时提交的内容。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProfileInput {
    pub name: String,
    pub driver: DriverId,
    pub host: String,
    pub port: u16,
    pub user: String,
    #[serde(default)]
    pub database: Option<String>,
    /// 驱动特有的选项（如 Oracle 的 `sid`），与 `ConnectParams::options` 相同。
    #[serde(default)]
    pub options: BTreeMap<String, String>,
    #[serde(default)]
    pub environment: Environment,
    /// 界面上的颜色标记，如 `#d4380d`。
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default = "default_timeout")]
    pub connect_timeout_secs: u64,
}

fn default_timeout() -> u64 {
    ConnectParams::DEFAULT_CONNECT_TIMEOUT.as_secs()
}

/// 已保存的连接配置。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Profile {
    pub id: String,
    #[serde(flatten)]
    pub input: ProfileInput,
}

impl Profile {
    /// 用给定密码组成连接参数。原生库目录由驱动注册表另外补上。
    pub fn params(&self, password: String) -> ConnectParams {
        let p = &self.input;
        ConnectParams {
            host: p.host.clone(),
            port: p.port,
            user: p.user.clone(),
            password,
            database: p.database.clone().filter(|d| !d.is_empty()),
            options: p.options.clone(),
            connect_timeout: Duration::from_secs(p.connect_timeout_secs),
        }
    }
}

fn driver_name(d: DriverId) -> &'static str {
    match d {
        DriverId::Mysql => "mysql",
        DriverId::Postgres => "postgres",
        DriverId::Oracle => "oracle",
        DriverId::Dameng => "dameng",
    }
}

fn parse_driver(s: &str) -> rusqlite::Result<DriverId> {
    match s {
        "mysql" => Ok(DriverId::Mysql),
        "postgres" => Ok(DriverId::Postgres),
        "oracle" => Ok(DriverId::Oracle),
        "dameng" => Ok(DriverId::Dameng),
        other => {
            Err(rusqlite::Error::InvalidColumnType(2, format!("driver={other}"), rusqlite::types::Type::Text))
        }
    }
}

fn validate(p: &ProfileInput) -> Result<()> {
    if p.name.trim().is_empty() {
        return Err(Error::invalid(msg!("profile.name_required")));
    }
    if p.host.trim().is_empty() {
        return Err(Error::invalid(msg!("profile.host_required")));
    }
    if p.port == 0 {
        return Err(Error::invalid(msg!("profile.port_zero")));
    }
    if p.connect_timeout_secs == 0 || p.connect_timeout_secs > 3600 {
        return Err(Error::invalid(msg!("profile.timeout_range")));
    }
    Ok(())
}

const COLUMNS: &str =
    "id, name, driver, host, port, user, database, options, environment, color, connect_timeout_secs";

fn from_row(r: &Row) -> rusqlite::Result<Profile> {
    let options: String = r.get(7)?;
    let environment: String = r.get(8)?;
    Ok(Profile {
        id: r.get(0)?,
        input: ProfileInput {
            name: r.get(1)?,
            driver: parse_driver(&r.get::<_, String>(2)?)?,
            host: r.get(3)?,
            port: r.get(4)?,
            user: r.get(5)?,
            database: r.get(6)?,
            options: serde_json::from_str(&options).unwrap_or_default(),
            environment: Environment::parse(&environment),
            color: r.get(9)?,
            connect_timeout_secs: u64::try_from(r.get::<_, i64>(10)?).unwrap_or(0),
        },
    })
}

fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}

impl Store {
    pub fn list_profiles(&self) -> Result<Vec<Profile>> {
        self.with(|c| {
            let mut st = c.prepare(&format!("SELECT {COLUMNS} FROM profiles ORDER BY name, id"))?;
            st.query_map([], from_row)?.collect()
        })
    }

    pub fn profile(&self, id: &str) -> Result<Profile> {
        self.with(|c| {
            c.query_row(&format!("SELECT {COLUMNS} FROM profiles WHERE id = ?1"), params![id], from_row)
                .optional()
        })?
        .ok_or_else(|| Error::not_found(msg!("noun.profile", id = id)))
    }

    pub fn create_profile(&self, input: ProfileInput) -> Result<Profile> {
        validate(&input)?;
        let p = Profile { id: uuid::Uuid::new_v4().to_string(), input };
        let i = &p.input;
        let options = serde_json::to_string(&i.options).map_err(|e| Error::internal(e.to_string()))?;
        let ts = now();
        self.with(|c| {
            c.execute(
                &format!(
                    "INSERT INTO profiles ({COLUMNS}, created_at, updated_at)
                          VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?12)"
                ),
                params![
                    p.id,
                    i.name,
                    driver_name(i.driver),
                    i.host,
                    i.port,
                    i.user,
                    i.database,
                    options,
                    i.environment.as_str(),
                    i.color,
                    i.connect_timeout_secs as i64,
                    ts
                ],
            )
        })?;
        Ok(p)
    }

    pub fn update_profile(&self, id: &str, input: ProfileInput) -> Result<Profile> {
        validate(&input)?;
        let options = serde_json::to_string(&input.options).map_err(|e| Error::internal(e.to_string()))?;
        let i = &input;
        let n = self.with(|c| {
            c.execute(
                "UPDATE profiles SET name = ?2, driver = ?3, host = ?4, port = ?5, user = ?6, database = ?7,
                        options = ?8, environment = ?9, color = ?10, connect_timeout_secs = ?11, updated_at = ?12
                 WHERE id = ?1",
                params![
                    id,
                    i.name,
                    driver_name(i.driver),
                    i.host,
                    i.port,
                    i.user,
                    i.database,
                    options,
                    i.environment.as_str(),
                    i.color,
                    i.connect_timeout_secs as i64,
                    now()
                ],
            )
        })?;
        if n == 0 {
            return Err(Error::not_found(msg!("noun.profile", id = id)));
        }
        Ok(Profile { id: id.to_string(), input })
    }

    /// 删除连接配置；本地加密保存的密码随之删除（外键级联）。系统钥匙串中的密码由 `Secrets` 删除。
    pub fn delete_profile(&self, id: &str) -> Result<()> {
        let n = self.with(|c| c.execute("DELETE FROM profiles WHERE id = ?1", params![id]))?;
        if n == 0 {
            return Err(Error::not_found(msg!("noun.profile", id = id)));
        }
        Ok(())
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn input(name: &str) -> ProfileInput {
        ProfileInput {
            name: name.into(),
            driver: DriverId::Postgres,
            host: "db.local".into(),
            port: 5432,
            user: "app".into(),
            database: Some("shop".into()),
            options: BTreeMap::from([("sslmode".to_string(), "disable".to_string())]),
            environment: Environment::Prod,
            color: Some("#d4380d".into()),
            connect_timeout_secs: 5,
        }
    }

    #[test]
    fn crud() {
        let s = Store::open_in_memory().unwrap();
        let a = s.create_profile(input("b-生产库")).unwrap();
        let b = s.create_profile(input("a-测试库")).unwrap();
        assert_ne!(a.id, b.id);
        let list = s.list_profiles().unwrap();
        assert_eq!(list.iter().map(|p| p.input.name.as_str()).collect::<Vec<_>>(), ["a-测试库", "b-生产库"]);
        assert_eq!(s.profile(&a.id).unwrap(), a);

        let mut changed = input("b-生产库");
        changed.environment = Environment::Dev;
        changed.options.clear();
        let updated = s.update_profile(&a.id, changed.clone()).unwrap();
        assert_eq!(s.profile(&a.id).unwrap(), updated);
        assert_eq!(updated.input, changed);

        s.delete_profile(&a.id).unwrap();
        assert!(matches!(s.profile(&a.id), Err(Error::NotFound { .. })));
        assert!(matches!(s.delete_profile(&a.id), Err(Error::NotFound { .. })));
        assert!(matches!(s.update_profile(&a.id, input("x")), Err(Error::NotFound { .. })));
    }

    #[test]
    fn validation() {
        let s = Store::open_in_memory().unwrap();
        for bad in [
            ProfileInput { name: " ".into(), ..input("x") },
            ProfileInput { host: "".into(), ..input("x") },
            ProfileInput { port: 0, ..input("x") },
            ProfileInput { connect_timeout_secs: 0, ..input("x") },
        ] {
            assert!(matches!(s.create_profile(bad), Err(Error::Invalid { .. })));
        }
    }

    #[test]
    fn params_and_json_shape() {
        let p =
            Profile { id: "1".into(), input: ProfileInput { database: Some(String::new()), ..input("x") } };
        let cp = p.params("pw".into());
        assert_eq!((cp.host.as_str(), cp.port, cp.password.as_str()), ("db.local", 5432, "pw"));
        assert_eq!(cp.database, None, "空库名视为未指定");
        assert_eq!(cp.connect_timeout, Duration::from_secs(5));
        let json = serde_json::to_value(&p).unwrap();
        assert_eq!(json["id"], "1");
        assert_eq!(json["driver"], "postgres");
        assert_eq!(json["environment"], "prod");
        let back: Profile = serde_json::from_value(json).unwrap();
        assert_eq!(back, p);
        // 可选字段可以省略
        let minimal: ProfileInput =
            serde_json::from_str(r#"{"name":"n","driver":"mysql","host":"h","port":3306,"user":"u"}"#)
                .unwrap();
        assert_eq!(minimal.environment, Environment::None);
        assert_eq!(minimal.connect_timeout_secs, 10);
    }
}
