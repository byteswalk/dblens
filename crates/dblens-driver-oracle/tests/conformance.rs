use chrono::{FixedOffset, NaiveDate, TimeZone};
use dblens_core::Value;
use dblens_driver::conformance::Fixture;
use dblens_driver::{ConnectParams, Driver, ExecEvent, collect_all};
use dblens_driver_oracle::OracleDriver;

fn fixture(params: ConnectParams) -> Fixture {
    Fixture {
        driver: Box::new(OracleDriver),
        params,
        select_one: "SELECT 1 FROM DUAL",
        gen_rows: |n| format!("SELECT LEVEL FROM DUAL CONNECT BY LEVEL <= {n} ORDER BY 1"),
        long_running: "SELECT COUNT(*) FROM all_objects a, all_objects b, all_objects c",
        large_text: "SELECT RPAD(TO_CLOB('a'), 100000, 'a') FROM DUAL",
        create_table: |t| format!("CREATE TABLE {t} (id NUMBER(10) PRIMARY KEY, name VARCHAR2(100))"),
        // 用 PL/SQL 块兼容 23ai 之前的版本，同时覆盖 DBMS_OUTPUT 的收取路径
        drop_table_if_exists: |t| {
            format!(
                "BEGIN EXECUTE IMMEDIATE 'DROP TABLE {t}'; EXCEPTION WHEN OTHERS THEN IF SQLCODE != -942 THEN RAISE; END IF; END;"
            )
        },
        insert_with_params: |t| format!("INSERT INTO {t} (id, name) VALUES (:1, :2)"),
        select_name_by_id: |t| format!("SELECT name FROM {t} WHERE id = :1"),
        count_rows: |t| format!("SELECT COUNT(*) FROM {t}"),
        missing_table_query: "SELECT * FROM dblens_no_such_table",
        type_cases: vec![
            ("SELECT NULL FROM DUAL", Value::Null),
            ("SELECT 42 FROM DUAL", Value::Decimal("42".into())),
            ("SELECT 123.45 FROM DUAL", Value::Decimal("123.45".into())),
            ("SELECT 0.5 FROM DUAL", Value::Decimal("0.5".into())),
            ("SELECT CAST(1.5 AS BINARY_DOUBLE) FROM DUAL", Value::Float(1.5)),
            ("SELECT '数洞' FROM DUAL", Value::String("数洞".into())),
            (
                "SELECT DATE '2024-02-29' FROM DUAL",
                Value::DateTime(NaiveDate::from_ymd_opt(2024, 2, 29).unwrap().and_hms_opt(0, 0, 0).unwrap()),
            ),
            (
                "SELECT TIMESTAMP '2024-02-29 13:45:30.123456' FROM DUAL",
                Value::DateTime(
                    NaiveDate::from_ymd_opt(2024, 2, 29)
                        .unwrap()
                        .and_hms_micro_opt(13, 45, 30, 123456)
                        .unwrap(),
                ),
            ),
            (
                "SELECT TIMESTAMP '2024-02-29 13:45:30 +08:00' FROM DUAL",
                Value::DateTimeTz(
                    FixedOffset::east_opt(8 * 3600)
                        .unwrap()
                        .with_ymd_and_hms(2024, 2, 29, 13, 45, 30)
                        .unwrap(),
                ),
            ),
            ("SELECT HEXTORAW('010203') FROM DUAL", Value::Bytes(vec![1, 2, 3])),
            ("SELECT TO_CLOB('abc') FROM DUAL", Value::String("abc".into())),
        ],
    }
}

dblens_driver::conformance_tests!("DBLENS_TEST_ORACLE_URL", fixture);

/// PL/SQL 中 DBMS_OUTPUT 的输出应作为 Message 事件返回。
#[tokio::test]
async fn dbms_output_becomes_message() {
    let Ok(url) = std::env::var("DBLENS_TEST_ORACLE_URL") else {
        eprintln!("跳过：未设置环境变量 DBLENS_TEST_ORACLE_URL");
        return;
    };
    let (_, params) = ConnectParams::from_url(&url).unwrap();
    let mut conn = OracleDriver.connect(&params).await.unwrap();
    let mut stream = conn
        .execute("BEGIN DBMS_OUTPUT.PUT_LINE('你好'); DBMS_OUTPUT.PUT_LINE('数洞'); END;", &[])
        .await
        .unwrap();
    let events = collect_all(stream.as_mut(), 100).await.unwrap();
    assert_eq!(
        events,
        vec![ExecEvent::Message("你好".into()), ExecEvent::Message("数洞".into()), ExecEvent::Affected(1)]
    );
}
