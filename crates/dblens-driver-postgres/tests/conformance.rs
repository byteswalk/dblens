use chrono::{FixedOffset, NaiveDate, TimeZone};
use dblens_core::Value;
use dblens_driver::ConnectParams;
use dblens_driver::conformance::Fixture;
use dblens_driver_postgres::PgDriver;

fn base(params: ConnectParams, type_cases: Vec<(&'static str, Value)>) -> Fixture {
    Fixture {
        driver: Box::new(PgDriver),
        params,
        select_one: "SELECT 1",
        gen_rows: |n| format!("SELECT g FROM generate_series(1, {n}) AS g ORDER BY g"),
        long_running: "SELECT pg_sleep(10)",
        large_text: "SELECT repeat('a', 100000)",
        create_table: |t| format!("CREATE TABLE {t} (id INT PRIMARY KEY, name VARCHAR(100))"),
        drop_table_if_exists: |t| format!("DROP TABLE IF EXISTS {t}"),
        insert_with_params: |t| format!("INSERT INTO {t} (id, name) VALUES ($1, $2)"),
        select_name_by_id: |t| format!("SELECT name FROM {t} WHERE id = $1"),
        count_rows: |t| format!("SELECT COUNT(*) FROM {t}"),
        missing_table_query: "SELECT * FROM dblens_no_such_table",
        type_cases,
    }
}

/// PostgreSQL 的类型用例；人大金仓、openGauss 的 PG 模式也全部适用。字面量写成 CAST：人大金仓的
/// SQL Server 模式不认 `DATE '…'` 这类写法，MySQL 模式不认 `TIMESTAMPTZ '…'`；SQL Server 模式的 timestamp 是 rowversion。
fn pg_cases() -> Vec<(&'static str, Value)> {
    vec![
        ("SELECT NULL::text", Value::Null),
        ("SELECT true", Value::Bool(true)),
        ("SELECT 42::int4", Value::Int(42)),
        ("SELECT 42::int8", Value::Int(42)),
        ("SELECT 1.5::float8", Value::Float(1.5)),
        ("SELECT 123.4500::numeric(10,4)", Value::Decimal("123.4500".into())),
        (
            "SELECT 12345678901234567890.123456789::pg_catalog.numeric",
            Value::Decimal("12345678901234567890.123456789".into()),
        ),
        ("SELECT 'NaN'::numeric", Value::Decimal("NaN".into())),
        ("SELECT '数洞'::text", Value::String("数洞".into())),
        ("SELECT CAST('2024-02-29' AS date)", Value::Date(NaiveDate::from_ymd_opt(2024, 2, 29).unwrap())),
        (
            "SELECT CAST('2024-02-29 13:45:30.123456' AS pg_catalog.timestamp)",
            Value::DateTime(
                NaiveDate::from_ymd_opt(2024, 2, 29).unwrap().and_hms_micro_opt(13, 45, 30, 123456).unwrap(),
            ),
        ),
        (
            "SELECT CAST('2024-02-29 13:45:30+08' AS timestamptz)",
            Value::DateTimeTz(
                FixedOffset::east_opt(0).unwrap().with_ymd_and_hms(2024, 2, 29, 5, 45, 30).unwrap(),
            ),
        ),
        ("SELECT '\\x010203'::bytea", Value::Bytes(vec![1, 2, 3])),
        ("SELECT '{\"a\":1}'::jsonb", Value::Json(serde_json::json!({"a": 1}))),
        (
            "SELECT 'a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11'::uuid",
            Value::Uuid("a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11".parse().unwrap()),
        ),
        ("SELECT CAST('1 day 02:03:04' AS interval)", Value::Interval("0 mons 1 days 02:03:04".into())),
        ("SELECT 'x'::char(1)::\"char\"", Value::Other { type_name: "char".into(), text: "x".into() }),
    ]
}

/// Oracle 兼容模式（人大金仓、openGauss）：numeric 不接受 'NaN'，`DATE` 带时间；另测 Oracle 风格的类型。
fn oracle_mode_cases() -> Vec<(&'static str, Value)> {
    let day = NaiveDate::from_ymd_opt(2024, 2, 29).unwrap();
    let mut cases: Vec<_> = pg_cases()
        .into_iter()
        .filter(|(sql, _)| !sql.contains("'NaN'"))
        .map(|(sql, v)| match v {
            // GaussDB 的 A 模式按 nls_date_format（默认 DD-MON-RR）解析日期字面量，这里写明格式
            Value::Date(d) => (
                "SELECT TO_DATE('2024-02-29', 'YYYY-MM-DD')",
                Value::DateTime(d.and_hms_opt(0, 0, 0).unwrap()),
            ),
            v => (sql, v),
        })
        .collect();
    let dt = day.and_hms_opt(13, 45, 30).unwrap();
    cases.extend([
        ("SELECT CAST(1.5 AS NUMBER(10,2))", Value::Decimal("1.50".into())),
        ("SELECT CAST('数洞' AS VARCHAR2(10))", Value::String("数洞".into())),
        ("SELECT CAST('数洞' AS CLOB)", Value::String("数洞".into())),
        // Oracle 模式的 DATE 带时间
        ("SELECT CAST(TIMESTAMP '2024-02-29 13:45:30' AS DATE)", Value::DateTime(dt)),
    ]);
    cases
}

fn pg_fixture(params: ConnectParams) -> Fixture {
    base(params, pg_cases())
}

/// openGauss 5.0 用二进制格式发 numeric 的 NaN 时发成全零，客户端读到的是 0，所以不测 NaN。
fn opengauss_fixture(params: ConnectParams) -> Fixture {
    base(params, pg_cases().into_iter().filter(|(sql, _)| !sql.contains("'NaN'")).collect())
}

fn oracle_mode_fixture(params: ConnectParams) -> Fixture {
    base(params, oracle_mode_cases())
}

mod postgres {
    use super::*;
    dblens_driver::conformance_tests!("DBLENS_TEST_PG_URL", pg_fixture);
}

mod opengauss {
    use super::*;
    dblens_driver::conformance_tests!("DBLENS_TEST_OPENGAUSS_URL", opengauss_fixture);
}

/// 只存 SHA256 密码的账号：openGauss 自己的认证消息（vendor/PATCHES.md）。
mod opengauss_sha {
    use super::*;
    dblens_driver::conformance_tests!("DBLENS_TEST_OPENGAUSS_SHA_URL", opengauss_fixture);
}

mod opengauss_a {
    use super::*;
    dblens_driver::conformance_tests!("DBLENS_TEST_OPENGAUSS_A_URL", oracle_mode_fixture);
}

mod kingbase {
    use super::*;
    dblens_driver::conformance_tests!("DBLENS_TEST_KINGBASE_URL", oracle_mode_fixture);
}

mod kingbase_pg {
    use super::*;
    dblens_driver::conformance_tests!("DBLENS_TEST_KINGBASE_PG_URL", pg_fixture);
}

/// 人大金仓 MySQL 模式：日期时间是 sys 模式下的同名类型（OID 不同，格式与标准类型相同）。
fn kingbase_mysql_fixture(params: ConnectParams) -> Fixture {
    let day = NaiveDate::from_ymd_opt(2024, 2, 29).unwrap();
    let mut cases = pg_cases();
    cases.extend([
        (
            "SELECT CAST('2024-02-29 13:45:30' AS datetime)",
            Value::DateTime(day.and_hms_opt(13, 45, 30).unwrap()),
        ),
        (
            "SELECT CAST('2024-02-29 13:45:30' AS timestamp)",
            Value::DateTime(day.and_hms_opt(13, 45, 30).unwrap()),
        ),
        (
            "SELECT CAST('13:45:30' AS time)",
            Value::Time(chrono::NaiveTime::from_hms_opt(13, 45, 30).unwrap()),
        ),
    ]);
    base(params, cases)
}

/// 人大金仓 SQL Server 模式：numeric 不接受 'NaN'，`"char"` 是普通的 char；字符、GUID、日期时间是 sys 模式下的类型。
/// 它的 time、datetime2 按 100 纳秒计，驱动按 OID 识别。
fn kingbase_sqlserver_fixture(params: ConnectParams) -> Fixture {
    let day = NaiveDate::from_ymd_opt(2024, 2, 29).unwrap();
    let mut cases: Vec<_> = pg_cases()
        .into_iter()
        .filter(|(sql, _)| !sql.contains("'NaN'") && !sql.contains("\"char\""))
        .collect();
    cases.extend([
        ("SELECT CAST('数洞' AS varchar(10))", Value::String("数洞".into())),
        ("SELECT CAST('数洞' AS nvarchar(10))", Value::String("数洞".into())),
        ("SELECT CAST('ab' AS char(3))", Value::String("ab ".into())),
        (
            "SELECT CAST('a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11' AS uniqueidentifier)",
            Value::Uuid("a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11".parse().unwrap()),
        ),
        (
            "SELECT CAST('2024-02-29 13:45:30' AS datetime)",
            Value::DateTime(day.and_hms_opt(13, 45, 30).unwrap()),
        ),
        (
            "SELECT CAST('2024-02-29 13:45:30.1234567' AS datetime2)",
            Value::DateTime(day.and_hms_micro_opt(13, 45, 30, 123_456).unwrap()),
        ),
        (
            "SELECT CAST('13:45:30.25' AS time)",
            Value::Time(chrono::NaiveTime::from_hms_milli_opt(13, 45, 30, 250).unwrap()),
        ),
        // SQL Server 模式自己的二进制格式：bit 一个字节，tinyint 无符号，money 是 8 字节整数加 4 位小数
        ("SELECT CAST(1 AS bit)", Value::Bool(true)),
        ("SELECT CAST(200 AS tinyint)", Value::Int(200)),
        ("SELECT CAST(-12.3456 AS money)", Value::Decimal("-12.3456".into())),
    ]);
    base(params, cases)
}

mod kingbase_mysql {
    use super::*;
    dblens_driver::conformance_tests!("DBLENS_TEST_KINGBASE_MYSQL_URL", kingbase_mysql_fixture);
}

mod kingbase_sqlserver {
    use super::*;
    dblens_driver::conformance_tests!("DBLENS_TEST_KINGBASE_SQLSERVER_URL", kingbase_sqlserver_fixture);

    /// 参数按这几种类型的格式编码，写进去再读回来。
    #[tokio::test]
    async fn special_types_as_parameters() {
        let Some(f) = dblens_driver::conformance::load(
            "DBLENS_TEST_KINGBASE_SQLSERVER_URL",
            kingbase_sqlserver_fixture,
        ) else {
            return;
        };
        let mut c = f.driver.connect(&f.params).await.unwrap();
        let run = |sql: &'static str| sql;
        for sql in [
            run("DROP TABLE IF EXISTS dblens_conf_special"),
            run("CREATE TABLE dblens_conf_special (id INT PRIMARY KEY, b bit, t tinyint, m money)"),
        ] {
            let mut s = c.execute(sql, &[]).await.unwrap();
            dblens_driver::collect_all(s.as_mut(), 10).await.unwrap();
        }
        let params = [
            Value::Int(1),
            Value::Bool(true),
            Value::Int(255),
            Value::Decimal("922337203685477.5807".into()),
        ];
        let mut s =
            c.execute("INSERT INTO dblens_conf_special VALUES ($1, $2, $3, $4)", &params).await.unwrap();
        dblens_driver::collect_all(s.as_mut(), 10).await.unwrap();
        drop(s);
        let mut s = c
            .execute("SELECT b, t, m FROM dblens_conf_special WHERE id = $1", &[Value::Int(1)])
            .await
            .unwrap();
        let ev = dblens_driver::collect_all(s.as_mut(), 10).await.unwrap();
        drop(s);
        let rows: Vec<_> = ev
            .into_iter()
            .filter_map(|e| if let dblens_driver::ExecEvent::Rows(r) = e { Some(r) } else { None })
            .flatten()
            .collect();
        assert_eq!(
            rows,
            [vec![Value::Bool(true), Value::Int(255), Value::Decimal("922337203685477.5807".into())]]
        );
        let mut s = c.execute("DROP TABLE dblens_conf_special", &[]).await.unwrap();
        dblens_driver::collect_all(s.as_mut(), 10).await.unwrap();
    }
}
