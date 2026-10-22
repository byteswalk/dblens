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

async fn connect_or_skip() -> Option<Box<dyn dblens_driver::Connection>> {
    let Ok(url) = std::env::var("DBLENS_TEST_ORACLE_URL") else {
        eprintln!("跳过：未设置环境变量 DBLENS_TEST_ORACLE_URL");
        return None;
    };
    let (_, params) = ConnectParams::from_url(&url).unwrap();
    Some(OracleDriver.connect(&params).await.unwrap())
}

async fn first_value(conn: &mut Box<dyn dblens_driver::Connection>, sql: &str) -> Value {
    let mut stream = conn.execute(sql, &[]).await.unwrap();
    let events = collect_all(stream.as_mut(), 100).await.unwrap();
    for e in events {
        if let ExecEvent::Rows(rows) = e {
            return rows[0][0].clone();
        }
    }
    panic!("没有返回行：{sql}");
}

/// 注意：Oracle 的 RPAD 按显示宽度补齐，全角字符占 2 列，所以 `RPAD(TO_CLOB('数'), n, '数')` 实测只有 n/2 个字符
/// （LENGTH 与 DBMS_LOB.GETLENGTH 都是 n/2，驱动读到的 size() 与之一致）。下面的期望值按此换算。
/// 中文 CLOB（每字符 3 字节）：超过预览上限时必须标记为截断，不能报错，也不能当作完整字符串返回。
#[tokio::test]
async fn cjk_clob_preview_is_truncated_by_chars() {
    let Some(mut conn) = connect_or_skip().await else { return };
    match first_value(&mut conn, "SELECT RPAD(TO_CLOB('数'), 100000, '数') FROM DUAL").await {
        Value::Lob(lob) => {
            assert!(lob.truncated);
            // 50000 字符 = 150000 字节，远超 64 KiB 预览上限
            assert_eq!(lob.total_len, 50000);
            assert!(!lob.preview.is_empty() && lob.preview.len() <= 65536);
            assert!(lob.preview.chars().all(|c| c == '数'));
        }
        other => panic!("应为 LOB 预览，实际是 {other:?}"),
    }
    // 未超过上限（60000 字节）的中文 CLOB 应原样返回字符串
    match first_value(&mut conn, "SELECT RPAD(TO_CLOB('数'), 40000, '数') FROM DUAL").await {
        Value::String(s) => assert_eq!(s.chars().count(), 20000),
        other => panic!("应为完整字符串，实际是 {other:?}"),
    }
}

/// 语句出错时，之前写入 DBMS_OUTPUT 的内容不能泄漏到下一个 PL/SQL 块的消息里。
#[tokio::test]
async fn dbms_output_discarded_after_error() {
    let Some(mut conn) = connect_or_skip().await else { return };
    let err = match conn
        .execute("BEGIN DBMS_OUTPUT.PUT_LINE('残留'); RAISE_APPLICATION_ERROR(-20001, 'x'); END;", &[])
        .await
    {
        Ok(_) => panic!("应当失败"),
        Err(e) => e,
    };
    assert!(matches!(&err, dblens_core::Error::Sql { code, .. } if code == "ORA-20001"), "{err:?}");
    let mut stream = conn.execute("BEGIN DBMS_OUTPUT.PUT_LINE('新'); END;", &[]).await.unwrap();
    let events = collect_all(stream.as_mut(), 100).await.unwrap();
    assert_eq!(events.len(), 2, "{events:?}");
    assert_eq!(events[0], ExecEvent::Message("新".into()));
    assert!(matches!(events[1], ExecEvent::Affected(_)));
}

/// 触发器在 DML 中写出的 DBMS_OUTPUT 应随该 DML 返回，而不是留给之后的语句。
#[tokio::test]
async fn dbms_output_from_trigger_is_collected_after_dml() {
    let Some(mut conn) = connect_or_skip().await else { return };
    let table = format!("DBLENS_TRG_{}", std::process::id());
    for sql in [
        format!("CREATE TABLE {table} (id NUMBER)"),
        format!(
            "CREATE OR REPLACE TRIGGER {table}_T AFTER INSERT ON {table} BEGIN DBMS_OUTPUT.PUT_LINE('触发器'); END;"
        ),
    ] {
        let mut s = conn.execute(&sql, &[]).await.unwrap();
        collect_all(s.as_mut(), 100).await.unwrap();
    }
    let insert = format!("INSERT INTO {table} VALUES (1)");
    let mut s = conn.execute(&insert, &[]).await.unwrap();
    let events = collect_all(s.as_mut(), 100).await.unwrap();
    drop(s);
    let drop_sql = format!("DROP TABLE {table}");
    let mut s = conn.execute(&drop_sql, &[]).await.unwrap();
    collect_all(s.as_mut(), 100).await.unwrap();
    assert_eq!(events, vec![ExecEvent::Message("触发器".into()), ExecEvent::Affected(1)]);
}
