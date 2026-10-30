//! 一致性测试，加上 SQL Server 特有的：一批多个结果集、影响行数与 PRINT 消息、批内错误的位置、读一半丢弃后连接仍可用、
//! 手动提交时批里自己提交、Windows 认证。
//! `DBLENS_TEST_SQLSERVER_URL`（如 `sqlserver://sa:密码@192.168.139.106:1433/master`）；
//! Windows 认证：`DBLENS_TEST_SQLSERVER_WINAUTH_URL`（`?auth=windows`，用户名写 `机器名\用户`）、
//! `DBLENS_TEST_SQLSERVER_INTEGRATED_URL`（`?auth=integrated`，在 Windows 上运行）。

use chrono::{FixedOffset, NaiveDate, TimeZone};
use dblens_core::{Error, Value};
use dblens_driver::conformance::{Fixture, load};
use dblens_driver::{ConnectParams, Connection, Driver, ExecEvent, collect_all};
use dblens_driver_sqlserver::SqlServerDriver;

fn fixture(params: ConnectParams) -> Fixture {
    let day = NaiveDate::from_ymd_opt(2024, 2, 29).unwrap();
    Fixture {
        driver: Box::new(SqlServerDriver),
        params,
        select_one: "SELECT 1",
        gen_rows: |n| {
            format!(
                "SELECT TOP ({n}) ROW_NUMBER() OVER (ORDER BY (SELECT NULL)) AS n FROM sys.all_columns a CROSS JOIN sys.all_columns b ORDER BY n"
            )
        },
        long_running: "WAITFOR DELAY '00:00:20'",
        large_text: "SELECT REPLICATE(CAST('a' AS varchar(max)), 100000)",
        create_table: |t| format!("CREATE TABLE {t} (id INT PRIMARY KEY, name NVARCHAR(100))"),
        drop_table_if_exists: |t| format!("DROP TABLE IF EXISTS {t}"),
        insert_with_params: |t| format!("INSERT INTO {t} (id, name) VALUES (@P1, @P2)"),
        select_name_by_id: |t| format!("SELECT name FROM {t} WHERE id = @P1"),
        count_rows: |t| format!("SELECT COUNT(*) FROM {t}"),
        missing_table_query: "SELECT * FROM dblens_no_such_table",
        type_cases: vec![
            ("SELECT CAST(NULL AS int)", Value::Null),
            ("SELECT CAST(1 AS bit)", Value::Bool(true)),
            ("SELECT CAST(255 AS tinyint)", Value::Int(255)),
            ("SELECT CAST(42 AS int)", Value::Int(42)),
            ("SELECT CAST(42 AS bigint)", Value::Int(42)),
            ("SELECT CAST(1.5 AS float)", Value::Float(1.5)),
            ("SELECT CAST(1.5 AS real)", Value::Float(1.5)),
            ("SELECT CAST(123.4500 AS decimal(10,4))", Value::Decimal("123.4500".into())),
            ("SELECT CAST(-0.05 AS numeric(5,2))", Value::Decimal("-0.05".into())),
            (
                "SELECT CAST(12345678901234567890123456789012345678 AS decimal(38,0))",
                Value::Decimal("12345678901234567890123456789012345678".into()),
            ),
            ("SELECT CAST(12.34 AS money)", Value::Decimal("12.3400".into())),
            ("SELECT N'数洞'", Value::String("数洞".into())),
            ("SELECT CAST('abc' AS varchar(10))", Value::String("abc".into())),
            ("SELECT CAST('2024-02-29' AS date)", Value::Date(day)),
            (
                "SELECT CAST('13:45:30.1234567' AS time)",
                Value::Time(chrono::NaiveTime::from_hms_nano_opt(13, 45, 30, 123_456_700).unwrap()),
            ),
            (
                "SELECT CAST('2024-02-29T13:45:30.123456' AS datetime2(6))",
                Value::DateTime(day.and_hms_micro_opt(13, 45, 30, 123_456).unwrap()),
            ),
            (
                "SELECT CAST('2024-02-29T13:45:30' AS datetime)",
                Value::DateTime(day.and_hms_opt(13, 45, 30).unwrap()),
            ),
            (
                "SELECT CAST('2024-02-29T13:45:30+08:00' AS datetimeoffset)",
                Value::DateTimeTz(
                    FixedOffset::east_opt(8 * 3600)
                        .unwrap()
                        .with_ymd_and_hms(2024, 2, 29, 13, 45, 30)
                        .unwrap(),
                ),
            ),
            ("SELECT 0x010203", Value::Bytes(vec![1, 2, 3])),
            (
                "SELECT CAST('a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11' AS uniqueidentifier)",
                Value::Uuid("a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11".parse().unwrap()),
            ),
            ("SELECT CAST('<a>1</a>' AS xml)", Value::String("<a>1</a>".into())),
        ],
    }
}

dblens_driver::conformance_tests!("DBLENS_TEST_SQLSERVER_URL", fixture);

async fn connect(env: &str) -> Option<Box<dyn Connection>> {
    let f = load(env, fixture)?;
    Some(SqlServerDriver.connect(&f.params).await.expect("连接测试库失败"))
}

async fn run(c: &mut dyn Connection, sql: &str) -> Result<Vec<ExecEvent>, Error> {
    let mut s = c.execute(sql, &[]).await?;
    collect_all(s.as_mut(), 1000).await
}

fn kinds(events: &[ExecEvent]) -> Vec<String> {
    events
        .iter()
        .map(|e| match e {
            ExecEvent::ResultStart { columns } => format!("start{}", columns.len()),
            ExecEvent::Rows(r) => format!("rows{}", r.len()),
            ExecEvent::ResultEnd => "end".into(),
            ExecEvent::Affected(n) => format!("affected{n}"),
            ExecEvent::Message(m) => format!("msg:{m}"),
        })
        .collect()
}

#[tokio::test]
async fn batch_with_results_counts_and_messages() {
    let Some(mut c) = connect("DBLENS_TEST_SQLSERVER_URL").await else { return };
    let events = run(
        c.as_mut(),
        "CREATE TABLE #t (id int)\nINSERT INTO #t VALUES (1), (2)\nPRINT N'完成'\nSELECT id FROM #t ORDER BY id\nSELECT 'x' AS a, 2 AS b WHERE 1 = 0\nUPDATE #t SET id = id + 1",
    )
    .await
    .unwrap();
    assert_eq!(
        kinds(&events),
        ["affected2", "msg:完成", "start1", "rows2", "end", "start2", "end", "affected2"]
    );
    // 临时表留在会话中（按批发送，不经 sp_executesql）
    assert_eq!(kinds(&run(c.as_mut(), "SELECT COUNT(*) FROM #t").await.unwrap()), ["start1", "rows1", "end"]);
    let events = run(c.as_mut(), "USE tempdb").await.unwrap();
    assert!(events.iter().all(|e| matches!(e, ExecEvent::Message(_))), "{events:?}");
    let db = run(c.as_mut(), "SELECT DB_NAME()").await.unwrap();
    assert!(matches!(&db[1], ExecEvent::Rows(r) if r[0][0] == Value::String("tempdb".into())), "{db:?}");
}

#[tokio::test]
async fn error_reports_code_and_line_and_keeps_earlier_results() {
    let Some(mut c) = connect("DBLENS_TEST_SQLSERVER_URL").await else { return };
    let sql = "SELECT 1\n\n  SELECT * FROM dblens_missing_table";
    let mut s = c.execute(sql, &[]).await.unwrap();
    let mut events = Vec::new();
    let err = loop {
        match s.next(100).await {
            Ok(Some(e)) => events.push(e),
            Ok(None) => panic!("期望出错：{events:?}"),
            Err(e) => break e,
        }
    };
    drop(s);
    assert_eq!(kinds(&events), ["start1", "rows1", "end"]);
    match err {
        Error::Sql { code, position, .. } => {
            assert_eq!(code, "208");
            assert_eq!(position, Some(sql.find("SELECT *").unwrap() as u32 + 1));
        }
        other => panic!("{other:?}"),
    }
    c.ping().await.unwrap();
}

#[tokio::test]
async fn abandoned_stream_leaves_connection_usable() {
    let Some(mut c) = connect("DBLENS_TEST_SQLSERVER_URL").await else { return };
    let mut s = c
        .execute("SELECT TOP (200000) a.object_id FROM sys.all_columns a CROSS JOIN sys.all_columns b", &[])
        .await
        .unwrap();
    assert!(matches!(s.next(10).await.unwrap(), Some(ExecEvent::ResultStart { .. })));
    assert!(matches!(s.next(10).await.unwrap(), Some(ExecEvent::Rows(_))));
    drop(s);
    let started = std::time::Instant::now();
    assert_eq!(kinds(&run(c.as_mut(), "SELECT 2").await.unwrap()), ["start1", "rows1", "end"]);
    assert!(started.elapsed() < std::time::Duration::from_secs(5), "{:?}", started.elapsed());
}

#[tokio::test]
async fn manual_commit_survives_commit_inside_the_batch() {
    let Some(mut c) = connect("DBLENS_TEST_SQLSERVER_URL").await else { return };
    run(c.as_mut(), "DROP TABLE IF EXISTS dblens_conf_tx2; CREATE TABLE dblens_conf_tx2 (id int)")
        .await
        .unwrap();
    c.set_autocommit(false).await.unwrap();
    run(c.as_mut(), "INSERT INTO dblens_conf_tx2 VALUES (1)\nCOMMIT").await.unwrap();
    // 事务已在批里提交，再提交不报错
    c.commit().await.unwrap();
    run(c.as_mut(), "INSERT INTO dblens_conf_tx2 VALUES (2)").await.unwrap();
    c.rollback().await.unwrap();
    c.set_autocommit(true).await.unwrap();
    let events = run(c.as_mut(), "SELECT COUNT(*) FROM dblens_conf_tx2").await.unwrap();
    assert!(matches!(&events[1], ExecEvent::Rows(r) if r[0][0] == Value::Int(1)), "{events:?}");
    run(c.as_mut(), "DROP TABLE dblens_conf_tx2").await.unwrap();
}

#[tokio::test]
async fn wrong_password_is_auth_error() {
    let Some(f) = load("DBLENS_TEST_SQLSERVER_URL", fixture) else { return };
    let params = ConnectParams { password: "dblens-wrong-password".into(), ..f.params };
    assert!(matches!(SqlServerDriver.connect(&params).await.err(), Some(Error::Auth { .. })));
}

async fn windows_login(env: &str) {
    let Some(mut c) = connect(env).await else { return };
    let events = run(c.as_mut(), "SELECT SUSER_SNAME()").await.unwrap();
    assert!(
        matches!(&events[1], ExecEvent::Rows(r) if matches!(&r[0][0], Value::String(s) if s.contains('\\'))),
        "{events:?}"
    );
}

#[tokio::test]
async fn windows_account_login() {
    windows_login("DBLENS_TEST_SQLSERVER_WINAUTH_URL").await;
}

#[tokio::test]
async fn integrated_login() {
    windows_login("DBLENS_TEST_SQLSERVER_INTEGRATED_URL").await;
}
