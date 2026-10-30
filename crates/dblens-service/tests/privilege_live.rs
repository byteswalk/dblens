//! 账号与权限的实库测试：环境变量见 `live.rs`，没设置时直接通过。要串行运行（`--test-threads=1`）。
//!
//! 夹具：表 dblens_priv_t；角色 dblens_priv_r2 有该表的 SELECT，dblens_priv_r1 是 r2 的成员，
//! 用户 dblens_priv_u 是 r1 的成员；用户 dblens_priv_low 什么权限都没有。结束时删除，开始时也先删一次。

use std::path::Path;
use std::sync::Arc;

use dblens_dialect::privilege::{EffectiveState, GrantEffect, PrivilegeGrant};
use dblens_driver::{ConnectParams, DriverId};
use dblens_service::{
    Coverage, DefaultPolicy, DriverRegistry, Environment, ProfileInput, SecretBackend, Secrets, Service,
    SessionEvent, Store,
};

const LIB_OPTIONS: [&str; 2] = ["oracle_client_lib_dir", "dpi_lib_dir"];
const LOW_PASSWORD: &str = "Dblens_Priv_9x!";

struct Fixture {
    create: &'static [&'static str],
    drop: &'static [&'static str],
    /// 主体 id：用户、低权限用户。
    user: &'static str,
    /// 低权限用户登录时的账号名。
    low_login: &'static str,
    schema: &'static str,
    table: &'static str,
}

const MYSQL: Fixture = Fixture {
    create: &[
        "CREATE TABLE dblens_priv_t (id INT PRIMARY KEY)",
        "CREATE ROLE 'dblens_priv_r1', 'dblens_priv_r2'",
        "GRANT SELECT ON dblens_test.dblens_priv_t TO 'dblens_priv_r2'",
        "GRANT 'dblens_priv_r2' TO 'dblens_priv_r1'",
        "CREATE USER 'dblens_priv_u'@'%' IDENTIFIED BY 'Dblens_Priv_9x!'",
        "CREATE USER 'dblens_priv_low'@'%' IDENTIFIED BY 'Dblens_Priv_9x!'",
        "GRANT 'dblens_priv_r1' TO 'dblens_priv_u'@'%'",
        "SET DEFAULT ROLE 'dblens_priv_r1' TO 'dblens_priv_u'@'%'",
        // 连到库本身要求在库里有权限
        "GRANT SELECT ON dblens_test.dblens_priv_t TO 'dblens_priv_low'@'%'",
    ],
    drop: &[
        "DROP USER IF EXISTS 'dblens_priv_u'@'%'",
        "DROP USER IF EXISTS 'dblens_priv_low'@'%'",
        "DROP ROLE IF EXISTS 'dblens_priv_r1'",
        "DROP ROLE IF EXISTS 'dblens_priv_r2'",
        "DROP TABLE IF EXISTS dblens_priv_t",
    ],
    user: "'dblens_priv_u'@'%'",
    low_login: "dblens_priv_low",
    schema: "dblens_test",
    table: "dblens_priv_t",
};

const MARIADB: Fixture = Fixture {
    create: &[
        "CREATE TABLE dblens_priv_t (id INT PRIMARY KEY)",
        "CREATE ROLE dblens_priv_r1",
        "CREATE ROLE dblens_priv_r2",
        "GRANT SELECT ON dblens_test.dblens_priv_t TO dblens_priv_r2",
        "GRANT dblens_priv_r2 TO dblens_priv_r1",
        "CREATE USER 'dblens_priv_u'@'%' IDENTIFIED BY 'Dblens_Priv_9x!'",
        "CREATE USER 'dblens_priv_low'@'%' IDENTIFIED BY 'Dblens_Priv_9x!'",
        "GRANT dblens_priv_r1 TO 'dblens_priv_u'@'%'",
        "SET DEFAULT ROLE dblens_priv_r1 FOR 'dblens_priv_u'@'%'",
        "GRANT SELECT ON dblens_test.dblens_priv_t TO 'dblens_priv_low'@'%'",
    ],
    drop: &[
        "DROP USER IF EXISTS 'dblens_priv_u'@'%'",
        "DROP USER IF EXISTS 'dblens_priv_low'@'%'",
        "DROP ROLE IF EXISTS dblens_priv_r1",
        "DROP ROLE IF EXISTS dblens_priv_r2",
        "DROP TABLE IF EXISTS dblens_priv_t",
    ],
    user: "'dblens_priv_u'@'%'",
    low_login: "dblens_priv_low",
    schema: "dblens_test",
    table: "dblens_priv_t",
};

/// OceanBase 登录时账号名要带租户。
const OCEANBASE: Fixture = Fixture { low_login: "dblens_priv_low@test", ..MYSQL };

const POSTGRES: Fixture = Fixture {
    create: &[
        "CREATE TABLE dblens_priv_t (id INT PRIMARY KEY)",
        "CREATE ROLE dblens_priv_r2",
        "CREATE ROLE dblens_priv_r1",
        "GRANT SELECT ON dblens_priv_t TO dblens_priv_r2",
        "GRANT dblens_priv_r2 TO dblens_priv_r1",
        "CREATE ROLE dblens_priv_u LOGIN PASSWORD 'Dblens_Priv_9x!'",
        "CREATE ROLE dblens_priv_low LOGIN PASSWORD 'Dblens_Priv_9x!'",
        "GRANT dblens_priv_r1 TO dblens_priv_u",
    ],
    drop: &[
        "DROP TABLE IF EXISTS dblens_priv_t",
        "DROP ROLE IF EXISTS dblens_priv_u",
        "DROP ROLE IF EXISTS dblens_priv_low",
        "DROP ROLE IF EXISTS dblens_priv_r1",
        "DROP ROLE IF EXISTS dblens_priv_r2",
    ],
    user: "dblens_priv_u",
    low_login: "dblens_priv_low",
    schema: "public",
    table: "dblens_priv_t",
};

/// openGauss 的 CREATE ROLE 必须带密码，用户带同名 schema，要 CASCADE。
const OPENGAUSS: Fixture = Fixture {
    create: &[
        "CREATE TABLE dblens_priv_t (id INT PRIMARY KEY)",
        "CREATE ROLE dblens_priv_r2 PASSWORD 'Dblens_Priv_9x!'",
        "CREATE ROLE dblens_priv_r1 PASSWORD 'Dblens_Priv_9x!'",
        "GRANT SELECT ON dblens_priv_t TO dblens_priv_r2",
        "GRANT dblens_priv_r2 TO dblens_priv_r1",
        "CREATE USER dblens_priv_u PASSWORD 'Dblens_Priv_9x!'",
        "CREATE USER dblens_priv_low PASSWORD 'Dblens_Priv_9x!'",
        "GRANT dblens_priv_r1 TO dblens_priv_u",
    ],
    drop: &[
        "DROP TABLE IF EXISTS dblens_priv_t",
        "DROP USER IF EXISTS dblens_priv_u CASCADE",
        "DROP USER IF EXISTS dblens_priv_low CASCADE",
        "DROP ROLE IF EXISTS dblens_priv_r1",
        "DROP ROLE IF EXISTS dblens_priv_r2",
    ],
    ..POSTGRES
};

const ORACLE: Fixture = Fixture {
    create: &[
        "CREATE TABLE dblens_priv_t (id NUMBER PRIMARY KEY)",
        "CREATE ROLE dblens_priv_r2",
        "CREATE ROLE dblens_priv_r1",
        "GRANT SELECT ON dblens_priv_t TO dblens_priv_r2",
        "GRANT dblens_priv_r2 TO dblens_priv_r1",
        "CREATE USER dblens_priv_u IDENTIFIED BY \"Dblens_Priv_9x!\"",
        "CREATE USER dblens_priv_low IDENTIFIED BY \"Dblens_Priv_9x!\"",
        "GRANT CREATE SESSION TO dblens_priv_low",
        "GRANT dblens_priv_r1 TO dblens_priv_u",
    ],
    drop: &[
        "DROP USER dblens_priv_u",
        "DROP USER dblens_priv_low",
        "DROP ROLE dblens_priv_r1",
        "DROP ROLE dblens_priv_r2",
        "DROP TABLE dblens_priv_t PURGE",
    ],
    user: "DBLENS_PRIV_U",
    low_login: "DBLENS_PRIV_LOW",
    schema: "DBLENS_TEST",
    table: "DBLENS_PRIV_T",
};

const DAMENG: Fixture = Fixture {
    drop: &[
        "DROP USER DBLENS_PRIV_U CASCADE",
        "DROP USER DBLENS_PRIV_LOW CASCADE",
        "DROP ROLE DBLENS_PRIV_R1",
        "DROP ROLE DBLENS_PRIV_R2",
        "DROP TABLE DBLENS_PRIV_T",
    ],
    ..ORACLE
};

const SQLSERVER: Fixture = Fixture {
    create: &[
        "CREATE TABLE dbo.dblens_priv_t (id INT PRIMARY KEY)",
        "CREATE ROLE dblens_priv_r2",
        "CREATE ROLE dblens_priv_r1",
        "GRANT SELECT ON dbo.dblens_priv_t TO dblens_priv_r2",
        "ALTER ROLE dblens_priv_r2 ADD MEMBER dblens_priv_r1",
        "CREATE LOGIN dblens_priv_u WITH PASSWORD = 'Dblens_Priv_9x!'",
        "CREATE LOGIN dblens_priv_low WITH PASSWORD = 'Dblens_Priv_9x!'",
        "CREATE USER dblens_priv_u FOR LOGIN dblens_priv_u",
        "CREATE USER dblens_priv_low FOR LOGIN dblens_priv_low",
        "ALTER ROLE dblens_priv_r1 ADD MEMBER dblens_priv_u",
    ],
    drop: &[
        "DROP USER IF EXISTS dblens_priv_u",
        "DROP USER IF EXISTS dblens_priv_low",
        "IF SUSER_ID('dblens_priv_u') IS NOT NULL DROP LOGIN dblens_priv_u",
        "IF SUSER_ID('dblens_priv_low') IS NOT NULL DROP LOGIN dblens_priv_low",
        "DROP ROLE IF EXISTS dblens_priv_r1",
        "DROP ROLE IF EXISTS dblens_priv_r2",
        "DROP TABLE IF EXISTS dbo.dblens_priv_t",
    ],
    user: "server:dblens_priv_u",
    low_login: "dblens_priv_low",
    schema: "dbo",
    table: "dblens_priv_t",
};

fn has_implicit(driver: DriverId) -> bool {
    driver != DriverId::Mysql
}

async fn profile(
    service: &Service,
    driver: DriverId,
    params: &ConnectParams,
    user: &str,
    password: &str,
) -> String {
    let mut options = params.options.clone();
    options.retain(|k, _| !LIB_OPTIONS.contains(&k.as_str()));
    let p = service
        .create_profile(
            ProfileInput {
                name: format!("priv-{user}"),
                driver,
                host: params.host.clone(),
                port: params.port,
                user: user.to_string(),
                database: params.database.clone(),
                options,
                environment: Environment::Test,
                color: None,
                connect_timeout_secs: 10,
                product: None,
            },
            Some(password),
        )
        .unwrap();
    p.id
}

/// 执行夹具语句；`tolerate` 为 true 时忽略错误（清理时对象可能不存在）。
async fn run(service: &Service, profile: &str, statements: &[&str], tolerate: bool) {
    let info = service.open_session(profile).await.unwrap();
    for sql in statements {
        service.sessions.execute(info.id, sql, true).await.unwrap();
        while let Some(ev) = service.sessions.next(info.id, 100).await.unwrap() {
            if let SessionEvent::Failed { error, .. } = &ev {
                assert!(tolerate, "{sql}: {error:?}");
            }
            if matches!(ev, SessionEvent::Done) {
                break;
            }
        }
    }
    service.sessions.close(info.id, false).await.unwrap();
}

async fn flow(env: &str, fx: Fixture) {
    let Ok(url) = std::env::var(env) else { return };
    let (driver, params) = ConnectParams::from_url(&url).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(Store::open(dir.path()).unwrap());
    let secrets = Arc::new(Secrets::with_backend(store.clone(), SecretBackend::Vault));
    secrets.init_vault("测试主密码").unwrap();
    let registry = Arc::new(DriverRegistry::new(store.clone()));
    for option in LIB_OPTIONS {
        if let Some(lib) = params.options.get(option) {
            registry.set_lib_dir(driver, Some(Path::new(lib))).unwrap();
        }
    }
    let service = Service::with_parts(store, secrets, registry, Arc::new(DefaultPolicy));
    let admin = profile(&service, driver, &params, &params.user, &params.password).await;

    run(&service, &admin, fx.drop, true).await;
    run(&service, &admin, fx.create, false).await;

    let overview = service.privilege_overview(&admin, None).await.unwrap();
    assert_eq!(overview.coverage, Coverage::Complete, "{env}");
    assert!(overview.principals.iter().any(|p| p.id == fx.user), "{env}: {:?}", overview.principals.len());
    // TiDB 没有 mysql.sys 这类系统账号
    assert!(
        driver == DriverId::Mysql || overview.principals.iter().any(|p| p.builtin),
        "{env}: 内置账号要有标记"
    );

    let detail = service.principal_privileges(&admin, None, fx.user).await.unwrap();
    let select = detail
        .effective
        .iter()
        .find(|e| {
            e.grant.privilege == "SELECT"
                && e.grant.target.object.as_deref().is_some_and(|o| o.eq_ignore_ascii_case(fx.table))
        })
        .unwrap_or_else(|| panic!("{env}: {:?}", detail.effective));
    assert!(select.via.len() >= 3, "{env}: 经两层角色：{:?}", select.via);
    assert_eq!(select.state, EffectiveState::Allowed, "{env}");
    let on_table =
        |g: &&PrivilegeGrant| g.target.object.as_deref().is_some_and(|o| o.eq_ignore_ascii_case(fx.table));
    assert!(!detail.direct.iter().any(|g| on_table(&g)), "{env}: 用户对表没有直接授权：{:?}", detail.direct);

    let report = service.object_access(&admin, None, fx.schema, fx.table).await.unwrap();
    assert_eq!(report.coverage, Coverage::Complete, "{env}");
    assert!(
        report.access.iter().any(|a| a.principal == fx.user && a.privilege.grant.privilege == "SELECT"),
        "{env}: {:?}",
        report.access.iter().map(|a| &a.principal).collect::<Vec<_>>()
    );
    assert!(
        !has_implicit(driver)
            || report.access.iter().any(|a| a.privilege.grant.effect == GrantEffect::Implicit),
        "{env}: 属主或超级用户要列出"
    );

    // 低权限连接不报整页错误
    let low = profile(&service, driver, &params, fx.low_login, LOW_PASSWORD).await;
    let overview = service.privilege_overview(&low, None).await.unwrap();
    println!("{env}: 低权限 {:?}，主体 {}", overview.coverage, overview.principals.len());
    // 目录视图对低权限账号隐藏别人时不报错，这种情况不能报告范围完整
    assert!(
        overview.coverage != Coverage::Complete || overview.principals.iter().any(|p| p.id == fx.user),
        "{env}: 看不到其他账号却报告范围完整"
    );
    service.principal_privileges(&low, None, fx.user).await.unwrap();
    service.object_access(&low, None, fx.schema, fx.table).await.unwrap();

    run(&service, &admin, fx.drop, true).await;
}

#[tokio::test]
async fn mysql() {
    flow("DBLENS_TEST_MYSQL_URL", MYSQL).await;
}

#[tokio::test]
async fn mariadb() {
    flow("DBLENS_TEST_MARIADB_URL", MARIADB).await;
}

#[tokio::test]
async fn tidb() {
    flow("DBLENS_TEST_TIDB_URL", MYSQL).await;
}

#[tokio::test]
async fn oceanbase() {
    flow("DBLENS_TEST_OCEANBASE_URL", OCEANBASE).await;
}

#[tokio::test]
async fn postgres() {
    flow("DBLENS_TEST_PG_URL", POSTGRES).await;
}

#[tokio::test]
async fn opengauss() {
    flow("DBLENS_TEST_OPENGAUSS_URL", OPENGAUSS).await;
}

#[tokio::test]
async fn opengauss_a() {
    flow("DBLENS_TEST_OPENGAUSS_A_URL", OPENGAUSS).await;
}

#[tokio::test]
async fn kingbase() {
    flow("DBLENS_TEST_KINGBASE_URL", POSTGRES).await;
}

#[tokio::test]
async fn kingbase_pg() {
    flow("DBLENS_TEST_KINGBASE_PG_URL", POSTGRES).await;
}

#[tokio::test]
async fn oracle() {
    flow("DBLENS_TEST_ORACLE_URL", ORACLE).await;
}

#[tokio::test]
async fn dameng() {
    flow("DBLENS_TEST_DM_URL", DAMENG).await;
}

#[tokio::test]
async fn sqlserver() {
    flow("DBLENS_TEST_SQLSERVER_URL", SQLSERVER).await;
}
