//! SQLite 的驱动一致性测试：每个测试用自己的临时文件，不需要外部服务。

use dblens_core::{Error, Value};
use dblens_driver::conformance::{self, Fixture};
use dblens_driver::{ConnectParams, Driver, ExecEvent, collect_all};
use dblens_driver_sqlite::SqliteDriver;

/// 临时目录随测试进程结束清理不了也无妨：放在系统临时目录下。
fn fixture() -> (tempfile::TempDir, Fixture) {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("conf.db");
    let url =
        format!("sqlite:///{}?create=1", file.to_string_lossy().replace('\\', "/").trim_start_matches('/'));
    let (_, params) = ConnectParams::from_url(&url).unwrap();
    (
        dir,
        Fixture {
            driver: Box::new(SqliteDriver),
            params,
            select_one: "SELECT 1",
            gen_rows: |n| {
                format!(
                    "WITH RECURSIVE c(x) AS (SELECT 1 UNION ALL SELECT x + 1 FROM c WHERE x < {n}) SELECT x FROM c ORDER BY x"
                )
            },
            // 无终止条件的递归，只能被中断
            long_running: "WITH RECURSIVE c(x) AS (SELECT 1 UNION ALL SELECT x + 1 FROM c) SELECT max(x) FROM c",
            large_text: "SELECT hex(zeroblob(50000))",
            create_table: |t| format!("CREATE TABLE {t} (id INTEGER PRIMARY KEY, name VARCHAR(100))"),
            drop_table_if_exists: |t| format!("DROP TABLE IF EXISTS {t}"),
            insert_with_params: |t| format!("INSERT INTO {t} (id, name) VALUES (?, ?)"),
            select_name_by_id: |t| format!("SELECT name FROM {t} WHERE id = ?"),
            count_rows: |t| format!("SELECT count(*) FROM {t}"),
            missing_table_query: "SELECT * FROM dblens_no_such_table",
            type_cases: vec![
                ("SELECT 42", Value::Int(42)),
                ("SELECT 1.5", Value::Float(1.5)),
                ("SELECT '中文'", Value::String("中文".into())),
                ("SELECT x'0102'", Value::Bytes(vec![1, 2])),
                ("SELECT NULL", Value::Null),
                ("SELECT 9223372036854775807", Value::Int(i64::MAX)),
            ],
        },
    )
}

macro_rules! suite {
    ($($name:ident),* $(,)?) => {
        $(
            #[tokio::test]
            async fn $name() {
                let (_dir, f) = fixture();
                conformance::$name(&f).await;
            }
        )*
    };
}

suite!(
    ping_and_server_info,
    select_one,
    row_batching,
    dml_and_params,
    transaction_commit_and_rollback,
    cancel_running_query,
    sql_error_has_code,
    large_text_becomes_lob_preview,
    lob_limit_reads_full_value,
    type_round_trip,
);

async fn run(
    c: &mut dyn dblens_driver::Connection,
    sql: &str,
    params: &[Value],
) -> Result<Vec<ExecEvent>, Error> {
    let mut s = c.execute(sql, params).await?;
    collect_all(s.as_mut(), 100).await
}

#[tokio::test]
async fn ddl_reports_zero_and_dates_bind_as_text() {
    let (_dir, f) = fixture();
    let mut c = f.driver.connect(&f.params).await.unwrap();
    run(c.as_mut(), "CREATE TABLE t (id INTEGER PRIMARY KEY, d TEXT, v DECIMAL(10,2))", &[]).await.unwrap();
    let ev = run(
        c.as_mut(),
        "INSERT INTO t VALUES (1, ?, ?), (2, NULL, NULL)",
        &[Value::Date(chrono::NaiveDate::from_ymd_opt(2026, 11, 5).unwrap()), Value::Decimal("12.50".into())],
    )
    .await
    .unwrap();
    assert_eq!(ev, [ExecEvent::Affected(2)]);
    // 建索引不改动行，不能报上一条 INSERT 的 2
    assert_eq!(run(c.as_mut(), "CREATE INDEX t_d ON t (d)", &[]).await.unwrap(), [ExecEvent::Affected(0)]);
    let ev = run(c.as_mut(), "SELECT d, v, typeof(v) FROM t WHERE id = 1", &[]).await.unwrap();
    let ExecEvent::ResultStart { columns } = &ev[0] else { panic!("{ev:?}") };
    assert_eq!(
        columns.iter().map(|c| c.type_name.as_str()).collect::<Vec<_>>(),
        ["TEXT", "DECIMAL(10,2)", ""]
    );
    assert_eq!(
        ev[1],
        ExecEvent::Rows(vec![vec![
            Value::String("2026-11-05".into()),
            Value::Float(12.5),
            Value::String("real".into())
        ]])
    );
}

#[tokio::test]
async fn syntax_error_has_char_position_and_trigger_body_runs() {
    let (_dir, f) = fixture();
    let mut c = f.driver.connect(&f.params).await.unwrap();
    // FORM 被当成列别名，出错的是后面的 t：第 17 个字符（'数' 是 3 个字节，按字符计）
    match run(c.as_mut(), "SELECT '数' FORM t", &[]).await {
        Err(Error::Sql { position: Some(p), .. }) => assert_eq!(p, 17),
        other => panic!("{other:?}"),
    }
    run(c.as_mut(), "CREATE TABLE a (id INTEGER PRIMARY KEY, n INT)", &[]).await.unwrap();
    run(c.as_mut(), "CREATE TABLE log (msg TEXT)", &[]).await.unwrap();
    run(
        c.as_mut(),
        "CREATE TRIGGER a_ai AFTER INSERT ON a BEGIN INSERT INTO log VALUES ('a'); INSERT INTO log VALUES ('b'); END",
        &[],
    )
    .await
    .unwrap();
    run(c.as_mut(), "INSERT INTO a (n) VALUES (1)", &[]).await.unwrap();
    let ev = run(c.as_mut(), "SELECT count(*) FROM log", &[]).await.unwrap();
    assert_eq!(ev[1], ExecEvent::Rows(vec![vec![Value::Int(2)]]));
}

#[tokio::test]
async fn read_only_and_not_a_database() {
    let (dir, f) = fixture();
    let mut c = f.driver.connect(&f.params).await.unwrap();
    run(c.as_mut(), "CREATE TABLE t (x)", &[]).await.unwrap();
    c.close().await.unwrap();

    let mut ro = f.params.clone();
    ro.options.insert("read_only".into(), "1".into());
    let mut c = SqliteDriver.connect(&ro).await.unwrap();
    assert!(
        matches!(run(c.as_mut(), "INSERT INTO t VALUES (1)", &[]).await, Err(Error::Sql { code, .. }) if code == "SQLITE_READONLY")
    );

    let junk = dir.path().join("junk.db");
    std::fs::write(&junk, vec![7u8; 4096]).unwrap();
    let mut p = f.params.clone();
    p.database = Some(junk.to_string_lossy().into_owned());
    assert!(matches!(SqliteDriver.connect(&p).await, Err(Error::Connect { .. })));
}

#[tokio::test]
async fn dropping_a_stream_midway_keeps_connection_usable() {
    let (_dir, f) = fixture();
    let mut c = f.driver.connect(&f.params).await.unwrap();
    {
        let sql = (f.gen_rows)(5000);
        let mut s = c.execute(&sql, &[]).await.unwrap();
        s.next(10).await.unwrap();
        s.next(10).await.unwrap();
    }
    run(c.as_mut(), "CREATE TABLE t (x)", &[]).await.unwrap();
    c.ping().await.unwrap();
}
