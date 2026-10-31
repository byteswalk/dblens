//! 账号与权限的实库测试：环境变量见 `live.rs`，没设置时直接通过。要串行运行（`--test-threads=1`）。
//!
//! 夹具：表 dblens_priv_t；角色 dblens_priv_r2 有该表的 SELECT，dblens_priv_r1 是 r2 的成员，
//! 用户 dblens_priv_u 是 r1 的成员；用户 dblens_priv_low 什么权限都没有。结束时删除，开始时也先删一次。

use std::path::Path;
use std::sync::Arc;

use dblens_dialect::privilege::{
    EffectiveState, GrantEffect, GrantScope, GrantTarget, Principal, PrincipalKind, PrincipalLevel,
    PrincipalRef, PrivChange, PrivilegeGrant, Secret,
};
use dblens_driver::{ConnectParams, DriverId};
use dblens_service::{
    Coverage, DefaultPolicy, DriverRegistry, Environment, ProfileInput, SecretBackend, Secrets, Service,
    SessionEvent, Store,
};

const LIB_OPTIONS: [&str; 2] = ["oracle_client_lib_dir", "jdbc_dir"];
const LOW_PASSWORD: &str = "Dblens_Priv_9x!";
const SENTINEL: &str = "Dblens_Sentinel_9x!";

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

/// GBase 8s 的账号是操作系统用户，这里只授权给名字；没有低权限登录和修改流程，见 `gbase`。
const GBASE_CREATE: &[&str] = &[
    "CREATE TABLE dblens_priv_t (id INT PRIMARY KEY)",
    "CREATE ROLE dblens_priv_r2",
    "CREATE ROLE dblens_priv_r1",
    "GRANT SELECT ON dblens_priv_t TO dblens_priv_r2",
    "GRANT UPDATE (id) ON dblens_priv_t TO dblens_priv_r2",
    "GRANT dblens_priv_r2 TO dblens_priv_r1",
    "GRANT CONNECT TO dblens_priv_u",
    "GRANT dblens_priv_r1 TO dblens_priv_u",
    "GRANT DEFAULT ROLE dblens_priv_r1 TO dblens_priv_u",
    "GRANT INSERT ON dblens_priv_t TO dblens_priv_u WITH GRANT OPTION",
];
const GBASE_DROP: &[&str] = &[
    "REVOKE CONNECT FROM dblens_priv_in",
    "REVOKE CONNECT FROM dblens_priv_os",
    "DROP ROLE dblens_priv_gr",
    "REVOKE DEFAULT ROLE FROM dblens_priv_u",
    "REVOKE dblens_priv_r1 FROM dblens_priv_u",
    "REVOKE CONNECT FROM dblens_priv_u",
    "DROP ROLE dblens_priv_r1",
    "DROP ROLE dblens_priv_r2",
    "DROP TABLE dblens_priv_t",
];

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
    options.retain(|k, _| driver == DriverId::Dameng || !LIB_OPTIONS.contains(&k.as_str()));
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

fn reference(p: &Principal) -> PrincipalRef {
    PrincipalRef { name: p.name.clone(), host: p.host.clone(), kind: p.kind, level: p.level }
}

async fn apply(service: &Service, profile: &str, change: PrivChange) {
    let r = service.privilege_apply(profile, None, &change, None).await.unwrap();
    assert_eq!(r.error, None, "{change:?}");
}

fn same_name(p: &Principal, name: &str) -> bool {
    p.name.eq_ignore_ascii_case(name)
}

/// 同名的主体里取能接受授权的那个：SQL Server 是数据库级，其他库只有一个。
fn pick<'a>(list: &'a [Principal], name: &str, role: bool) -> &'a Principal {
    let mut found: Vec<&Principal> =
        list.iter().filter(|p| same_name(p, name) && (p.kind == PrincipalKind::Role) == role).collect();
    found.sort_by_key(|p| p.level != PrincipalLevel::Database);
    found.first().copied().unwrap_or_else(|| panic!("找不到 {name}"))
}

/// 删掉上次留下的修改夹具：数据库级在前，SQL Server 的登录名要等用户删掉。
async fn drop_change_fixture(service: &Service, profile: &str, names: [&str; 2]) {
    let list = service.privilege_overview(profile, None).await.unwrap().principals;
    let mut found: Vec<&Principal> = list.iter().filter(|p| names.iter().any(|n| same_name(p, n))).collect();
    found.sort_by_key(|p| p.level != PrincipalLevel::Database);
    for p in found {
        let change = PrivChange::Drop { principal: reference(p), cascade: false };
        service.privilege_apply(profile, None, &change, None).await.unwrap();
    }
}

/// M9b：建用户和角色、授予回收、改密、锁定、删除都走专用路径，结果用只读接口核对。
async fn changes(env: &str, service: &Service, admin: &str, fx: &Fixture, driver: DriverId) {
    let upper = fx.user.chars().any(|c| c.is_ascii_uppercase());
    let name = |s: &str| if upper { s.to_uppercase() } else { s.to_string() };
    let (user, role) = (name("dblens_priv_c"), name("dblens_priv_cr"));
    drop_change_fixture(service, admin, [&user, &role]).await;

    let overview = service.privilege_overview(admin, None).await.unwrap();
    let caps = overview.caps;
    apply(
        service,
        admin,
        PrivChange::CreateUser { name: user.clone(), host: None, password: Secret::new(SENTINEL) },
    )
    .await;
    let level = if driver == DriverId::SqlServer { PrincipalLevel::Database } else { PrincipalLevel::Server };
    let password = caps.role_password.then(|| Secret::new(SENTINEL));
    apply(service, admin, PrivChange::CreateRole { name: role.clone(), level, password }).await;

    let list = service.privilege_overview(admin, None).await.unwrap().principals;
    let (member, login) = (reference(pick(&list, &user, false)), {
        let mut logins: Vec<&Principal> = list.iter().filter(|p| same_name(p, &user)).collect();
        logins.sort_by_key(|p| p.level != PrincipalLevel::Server);
        reference(logins[0])
    });
    let role_ref = reference(pick(&list, &role, true));
    let member_id = pick(&list, &user, false).id.clone();
    let target =
        GrantTarget { schema: Some(fx.schema.into()), object: Some(fx.table.into()), ..Default::default() };
    let select = "SELECT".to_string();
    apply(
        service,
        admin,
        PrivChange::Grant {
            grantee: role_ref.clone(),
            scope: GrantScope::Object,
            target: target.clone(),
            privilege: select.clone(),
            with_grant: false,
        },
    )
    .await;
    apply(
        service,
        admin,
        PrivChange::AddRole { member: member.clone(), role: role_ref.clone(), admin: false },
    )
    .await;
    apply(
        service,
        admin,
        PrivChange::Grant {
            grantee: member.clone(),
            scope: GrantScope::Object,
            target: target.clone(),
            privilege: "UPDATE".into(),
            with_grant: true,
        },
    )
    .await;
    let on_table =
        |g: &PrivilegeGrant| g.target.object.as_deref().is_some_and(|o| o.eq_ignore_ascii_case(fx.table));
    let detail = service.principal_privileges(admin, None, &member_id).await.unwrap();
    assert!(
        detail
            .effective
            .iter()
            .any(|e| e.grant.privilege == "SELECT" && on_table(&e.grant) && e.via.len() >= 2),
        "{env}: 经角色拿到 SELECT：{:?}",
        detail.effective
    );
    let direct = detail.direct.iter().find(|g| g.privilege == "UPDATE" && on_table(g));
    let direct = direct.unwrap_or_else(|| panic!("{env}: 直接授权：{:?}", detail.direct));
    assert!(direct.grantable, "{env}: 带转授权");

    apply(
        service,
        admin,
        PrivChange::SetPassword { principal: login.clone(), password: Secret::new(format!("{SENTINEL}2")) },
    )
    .await;
    if caps.lock {
        apply(service, admin, PrivChange::SetLocked { principal: login.clone(), locked: true }).await;
        let list = service.privilege_overview(admin, None).await.unwrap().principals;
        let locked = list.iter().find(|p| same_name(p, &user) && p.locked == Some(true));
        assert!(locked.is_some(), "{env}: 要显示已锁定");
        apply(service, admin, PrivChange::SetLocked { principal: login.clone(), locked: false }).await;
    }
    apply(
        service,
        admin,
        PrivChange::Revoke {
            grantee: member.clone(),
            scope: GrantScope::Object,
            target: target.clone(),
            privilege: "UPDATE".into(),
            grantable: true,
            grantor: None,
        },
    )
    .await;
    apply(service, admin, PrivChange::RemoveRole { member: member.clone(), role: role_ref.clone() }).await;
    let detail = service.principal_privileges(admin, None, &member_id).await.unwrap();
    assert!(!detail.effective.iter().any(on_table_effective(fx.table)), "{env}: {:?}", detail.effective);
    apply(
        service,
        admin,
        PrivChange::Revoke {
            grantee: role_ref.clone(),
            scope: GrantScope::Object,
            target,
            privilege: select,
            grantable: false,
            grantor: None,
        },
    )
    .await;

    drop_change_fixture(service, admin, [&user, &role]).await;
    let list = service.privilege_overview(admin, None).await.unwrap().principals;
    assert!(!list.iter().any(|p| same_name(p, &user) || same_name(p, &role)), "{env}: 没有删干净");
}

fn on_table_effective(
    table: &'static str,
) -> impl Fn(&dblens_dialect::privilege::EffectivePrivilege) -> bool {
    move |e| {
        e.grant.effect == GrantEffect::Grant
            && e.grant.target.object.as_deref().is_some_and(|o| o.eq_ignore_ascii_case(table))
    }
}

/// 数据目录里的所有文件都不能有哨兵密码。历史写入在后台线程里，先等一下。
fn assert_no_sentinel(env: &str, dir: &Path) {
    std::thread::sleep(std::time::Duration::from_millis(500));
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for entry in std::fs::read_dir(d).unwrap().flatten() {
            if entry.path().is_dir() {
                stack.push(entry.path());
            } else {
                let bytes = std::fs::read(entry.path()).unwrap();
                let found = bytes.windows(SENTINEL.len()).any(|w| w == SENTINEL.as_bytes());
                assert!(!found, "{env}: {} 里有密码", entry.path().display());
            }
        }
    }
}

async fn flow(env: &str, fx: Fixture) {
    let Ok(url) = std::env::var(env) else { return };
    let (driver, params) = ConnectParams::from_url(&url).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(Store::open(dir.path()).unwrap());
    let secrets = Arc::new(Secrets::with_backend(store.clone(), SecretBackend::Vault));
    secrets.init_vault("测试主密码").unwrap();
    let registry = Arc::new(DriverRegistry::new(store.clone()));
    // 达梦的驱动目录是连接选项，不是全局设置
    for option in LIB_OPTIONS.iter().filter(|_| driver != DriverId::Dameng) {
        if let Some(lib) = params.options.get(*option) {
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

    changes(env, &service, &admin, &fx, driver).await;
    assert_no_sentinel(env, dir.path());

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

    // 低权限连接开着时，SQL Server 删不掉它的登录名
    service.metadata.forget(&low).await.unwrap();
    run(&service, &admin, fx.drop, true).await;
    let left = service.privilege_overview(&admin, None).await.unwrap();
    assert!(
        !left.principals.iter().any(|p| p.name.eq_ignore_ascii_case("dblens_priv_low")),
        "{env}: 夹具没有清理干净"
    );
}

#[tokio::test]
async fn gbase() {
    let Ok(url) = std::env::var("DBLENS_TEST_GBASE_URL") else { return };
    let (driver, params) = ConnectParams::from_url(&url).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(Store::open(dir.path()).unwrap());
    let secrets = Arc::new(Secrets::with_backend(store.clone(), SecretBackend::Vault));
    secrets.init_vault("测试主密码").unwrap();
    let registry = Arc::new(DriverRegistry::new(store.clone()));
    if let Some(lib) = params.options.get("jdbc_dir") {
        registry.set_lib_dir(driver, Some(Path::new(lib))).unwrap();
    }
    let service = Service::with_parts(store, secrets, registry, Arc::new(DefaultPolicy));
    let admin = profile(&service, driver, &params, &params.user, &params.password).await;
    run(&service, &admin, GBASE_DROP, true).await;
    run(&service, &admin, GBASE_CREATE, false).await;

    let overview = service.privilege_overview(&admin, None).await.unwrap();
    assert_eq!(overview.coverage, Coverage::Complete);
    assert!(overview.caps.write && overview.caps.os_accounts);
    let by = |n: &str| overview.principals.iter().find(|p| p.id == n).unwrap_or_else(|| panic!("{n}"));
    assert_eq!(by("dblens_priv_r1").kind, PrincipalKind::Role);
    assert_eq!(by("dblens_priv_u").kind, PrincipalKind::User);
    assert!(by("gbasedbt").builtin);

    let detail = service.principal_privileges(&admin, None, "dblens_priv_u").await.unwrap();
    assert_eq!(detail.coverage, Coverage::Complete);
    let on_table = |g: &PrivilegeGrant| g.target.object.as_deref() == Some("dblens_priv_t");
    let select = detail.effective.iter().find(|e| e.grant.privilege == "SELECT" && on_table(&e.grant));
    let select = select.unwrap_or_else(|| panic!("{:?}", detail.effective));
    assert_eq!(select.via.len(), 3, "经两层角色：{:?}", select.via);
    assert_eq!(select.state, EffectiveState::Allowed, "默认角色：{:?}", select.notes);
    let insert = detail.direct.iter().find(|g| g.privilege == "INSERT" && on_table(g)).unwrap();
    assert!(insert.grantable, "大写表示可转授");
    assert!(detail.direct.iter().any(|g| g.privilege == "CONNECT"), "{:?}", detail.direct);
    assert!(
        detail.effective.iter().any(|e| e.grant.scope == GrantScope::Column && e.grant.privilege == "UPDATE")
    );

    let report = service.object_access(&admin, None, &params.user, "dblens_priv_t").await.unwrap();
    assert_eq!(report.coverage, Coverage::Complete);
    assert!(
        report
            .access
            .iter()
            .any(|a| a.principal == "dblens_priv_u" && a.privilege.grant.privilege == "SELECT")
    );
    assert!(report.access.iter().any(|a| a.privilege.grant.effect == GrantEffect::Implicit), "属主要列出");

    gbase_changes(&service, &admin, &params.user).await;
    run(&service, &admin, GBASE_DROP, true).await;
    let left = service.privilege_overview(&admin, None).await.unwrap();
    assert!(!left.principals.iter().any(|p| p.id.starts_with("dblens_priv")), "夹具没有清理干净");
}

/// 建内部用户到删除走一遍，新用户要能登录。
async fn gbase_changes(service: &Service, admin: &str, owner: &str) {
    const USER: &str = "dblens_priv_in";
    const ROLE: &str = "dblens_priv_gr";
    let apply = |change: PrivChange| async move {
        let r = service.privilege_apply(admin, None, &change, None).await.unwrap();
        assert!(r.error.is_none(), "{:?}: {:?}", r.failed, r.error);
    };
    let user = |kind| PrincipalRef { name: USER.into(), host: None, kind, level: PrincipalLevel::Database };
    let role = || PrincipalRef {
        name: ROLE.into(),
        host: None,
        kind: PrincipalKind::Role,
        level: PrincipalLevel::Database,
    };
    let login = || user(PrincipalKind::Login);
    let table = || GrantTarget {
        schema: Some(owner.into()),
        object: Some("dblens_priv_t".into()),
        column: None,
        ..Default::default()
    };
    apply(PrivChange::CreateUser {
        name: USER.into(),
        host: None,
        password: Secret::new("Dblens#Sentinel1"),
    })
    .await;
    apply(PrivChange::CreateRole { name: ROLE.into(), level: PrincipalLevel::Database, password: None })
        .await;
    apply(PrivChange::AddRole { member: login(), role: role(), admin: false }).await;
    apply(PrivChange::SetDefaultRole { member: login(), role: Some(role()) }).await;
    apply(PrivChange::Grant {
        grantee: login(),
        scope: GrantScope::Object,
        target: table(),
        privilege: "SELECT".into(),
        with_grant: true,
    })
    .await;
    apply(PrivChange::Grant {
        grantee: login(),
        scope: GrantScope::Column,
        target: GrantTarget { column: Some("id".into()), ..table() },
        privilege: "UPDATE".into(),
        with_grant: false,
    })
    .await;
    apply(PrivChange::Grant {
        grantee: login(),
        scope: GrantScope::Database,
        target: GrantTarget::default(),
        privilege: "RESOURCE".into(),
        with_grant: false,
    })
    .await;

    let overview = service.privilege_overview(admin, None).await.unwrap();
    let found = overview.principals.iter().find(|p| p.id == USER).expect("内部用户");
    assert_eq!(found.kind, PrincipalKind::Login);
    assert_eq!(found.locked, Some(false));
    let membership = overview.memberships.iter().find(|m| m.member == USER).unwrap();
    assert_eq!(membership.default_role, Some(true));
    let detail = service.principal_privileges(admin, None, USER).await.unwrap();
    assert!(detail.direct.iter().any(|g| g.privilege == "RESOURCE"), "{:?}", detail.direct);

    apply(PrivChange::SetLocked { principal: login(), locked: true }).await;
    let overview = service.privilege_overview(admin, None).await.unwrap();
    assert_eq!(overview.principals.iter().find(|p| p.id == USER).unwrap().locked, Some(true));
    apply(PrivChange::SetLocked { principal: login(), locked: false }).await;
    apply(PrivChange::SetPassword { principal: login(), password: Secret::new("Dblens#Sentinel2") }).await;

    // 新用户用改后的密码登录
    let input = ProfileInput { user: USER.into(), ..service.profile(admin).unwrap().input };
    service.test_connection(input, "Dblens#Sentinel2").await.unwrap();

    for g in detail.direct.iter().filter(|g| g.privilege == "SELECT" || g.privilege == "UPDATE") {
        apply(PrivChange::Revoke {
            grantee: login(),
            scope: g.scope,
            target: g.target.clone(),
            privilege: g.privilege.clone(),
            grantable: g.grantable,
            grantor: g.grantor.clone(),
        })
        .await;
    }
    apply(PrivChange::Revoke {
        grantee: login(),
        scope: GrantScope::Database,
        target: GrantTarget::default(),
        privilege: "RESOURCE".into(),
        grantable: false,
        grantor: None,
    })
    .await;
    apply(PrivChange::SetDefaultRole { member: login(), role: None }).await;
    apply(PrivChange::RemoveRole { member: login(), role: role() }).await;
    apply(PrivChange::Drop { principal: role(), cascade: false }).await;
    apply(PrivChange::Drop { principal: login(), cascade: false }).await;
    let overview = service.privilege_overview(admin, None).await.unwrap();
    assert!(!overview.principals.iter().any(|p| p.id == USER || p.id == ROLE), "清理干净");

    // 操作系统用户：只授权，移除后 sysusers 没有记录
    let os = "dblens_priv_os";
    apply(PrivChange::CreateUser { name: os.into(), host: None, password: Secret::new("") }).await;
    let overview = service.privilege_overview(admin, None).await.unwrap();
    let found = overview.principals.iter().find(|p| p.id == os).expect("操作系统用户");
    assert_eq!(found.kind, PrincipalKind::User);
    let principal = PrincipalRef { name: os.into(), ..user(PrincipalKind::User) };
    apply(PrivChange::Drop { principal, cascade: false }).await;
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
