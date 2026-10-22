use chrono::{NaiveDate, NaiveTime};
use dblens_core::Value;
use dblens_driver::conformance::Fixture;
use dblens_driver::{ConnectParams, Connection, Driver, ExecEvent, collect_all};
use dblens_driver_dameng::DmDriver;

fn fixture(params: ConnectParams) -> Fixture {
    Fixture {
        driver: Box::new(DmDriver),
        params,
        select_one: "SELECT 1",
        gen_rows: |n| format!("SELECT LEVEL FROM DUAL CONNECT BY LEVEL <= {n} ORDER BY 1"),
        // 三表笛卡尔积的 COUNT(*) 会被优化成计数相乘，约 0.12 秒就返回；
        // 加上逐行计算的条件后不取消约需 130 秒（与技术验证 spike_2 的 SLOW_SQL 相同）
        long_running: "SELECT COUNT(*) FROM SYSOBJECTS a, SYSOBJECTS b, SYSOBJECTS c \
                       WHERE a.ID + b.ID + c.ID > 0 AND LENGTH(a.NAME || b.NAME || c.NAME) > 0",
        // 与技术验证 spike_3 使用同一写法
        large_text: "SELECT RPAD(TO_CLOB('a'), 100000, 'a') FROM DUAL",
        create_table: |t| format!("CREATE TABLE {t} (id INT PRIMARY KEY, name VARCHAR(100))"),
        drop_table_if_exists: |t| format!("DROP TABLE IF EXISTS {t}"),
        insert_with_params: |t| format!("INSERT INTO {t} (id, name) VALUES (?, ?)"),
        select_name_by_id: |t| format!("SELECT name FROM {t} WHERE id = ?"),
        count_rows: |t| format!("SELECT COUNT(*) FROM {t}"),
        missing_table_query: "SELECT * FROM dblens_no_such_table",
        type_cases: vec![
            ("SELECT NULL", Value::Null),
            ("SELECT CAST(42 AS INT)", Value::Int(42)),
            ("SELECT CAST(42 AS BIGINT)", Value::Int(42)),
            ("SELECT CAST(1.5 AS DOUBLE)", Value::Float(1.5)),
            ("SELECT CAST(123.45 AS DEC(10,4))", Value::Decimal("123.4500".into())),
            ("SELECT '数洞'", Value::String("数洞".into())),
            ("SELECT DATE '2024-02-29'", Value::Date(NaiveDate::from_ymd_opt(2024, 2, 29).unwrap())),
            (
                "SELECT TIMESTAMP '2024-02-29 13:45:30.123456'",
                Value::DateTime(
                    NaiveDate::from_ymd_opt(2024, 2, 29)
                        .unwrap()
                        .and_hms_micro_opt(13, 45, 30, 123456)
                        .unwrap(),
                ),
            ),
            ("SELECT TIME '13:45:30'", Value::Time(NaiveTime::from_hms_opt(13, 45, 30).unwrap())),
            ("SELECT 0x010203", Value::Bytes(vec![1, 2, 3])),
        ],
    }
}

dblens_driver::conformance_tests!("DBLENS_TEST_DM_URL", fixture);

async fn connect_or_skip() -> Option<Box<dyn Connection>> {
    let Ok(url) = std::env::var("DBLENS_TEST_DM_URL") else {
        eprintln!("跳过：未设置环境变量 DBLENS_TEST_DM_URL");
        return None;
    };
    let (_, params) = ConnectParams::from_url(&url).unwrap();
    Some(DmDriver.connect(&params).await.unwrap())
}

async fn run(conn: &mut Box<dyn Connection>, sql: &str, params: &[Value]) -> Vec<ExecEvent> {
    let mut stream = conn.execute(sql, params).await.unwrap_or_else(|e| panic!("{sql} 执行失败：{e}"));
    collect_all(stream.as_mut(), 100).await.unwrap_or_else(|e| panic!("{sql} 取数失败：{e}"))
}

fn first_value(events: &[ExecEvent]) -> Value {
    events
        .iter()
        .find_map(|e| if let ExecEvent::Rows(rows) = e { Some(rows[0][0].clone()) } else { None })
        .unwrap_or_else(|| panic!("没有返回行：{events:?}"))
}

/// 中文 CLOB：DM 按 UTF-8 字节报告长度（技术验证笔记第 4 条），与 LobPreview.total_len 的字节语义一致；
/// 分块会切在字符中间，预览必须落在字符边界上。
#[tokio::test]
async fn cjk_clob_preview_is_cut_on_char_boundary() {
    let Some(mut conn) = connect_or_skip().await else { return };
    // RPAD 按显示宽度补齐，汉字算 2：得到 5 万个“数”，UTF-8 下 15 万字节
    match first_value(&run(&mut conn, "SELECT RPAD(TO_CLOB('数'), 100000, '数') FROM DUAL", &[]).await) {
        Value::Lob(lob) => {
            assert!(!lob.binary && lob.truncated);
            assert_eq!(lob.total_len, 150_000);
            assert!(lob.preview.len() <= 65_536 && lob.preview.len() > 65_530, "{}", lob.preview.len());
            assert!(lob.preview.chars().all(|c| c == '数'));
        }
        other => panic!("应为 LOB 预览，实际是 {other:?}"),
    }
    // 不超过上限（2 万个“数” = 6 万字节）时原样返回字符串
    match first_value(&run(&mut conn, "SELECT RPAD(TO_CLOB('数'), 40000, '数') FROM DUAL", &[]).await) {
        Value::String(s) => assert_eq!(s.chars().count(), 20_000),
        other => panic!("应为完整字符串，实际是 {other:?}"),
    }
}

/// NULL 与二进制参数的绑定，以及影响 0 行的 UPDATE。
#[tokio::test]
async fn null_and_binary_params() {
    let Some(mut conn) = connect_or_skip().await else { return };
    let t = "DBLENS_DM_PARAMS";
    run(&mut conn, &format!("DROP TABLE IF EXISTS {t}"), &[]).await;
    run(&mut conn, &format!("CREATE TABLE {t} (id INT, name VARCHAR(100), data VARBINARY(100))"), &[]).await;
    let insert = format!("INSERT INTO {t} VALUES (?, ?, ?)");
    let events = run(&mut conn, &insert, &[Value::Int(1), Value::Null, Value::Bytes(vec![0, 1, 255])]).await;
    assert_eq!(events, vec![ExecEvent::Affected(1)]);
    let select = format!("SELECT name, data FROM {t} WHERE id = ?");
    let events = run(&mut conn, &select, &[Value::Int(1)]).await;
    let row = events
        .iter()
        .find_map(|e| if let ExecEvent::Rows(rows) = e { Some(rows[0].clone()) } else { None })
        .unwrap();
    assert_eq!(row, vec![Value::Null, Value::Bytes(vec![0, 1, 255])]);
    let update = format!("UPDATE {t} SET name = ? WHERE id = ?");
    let events = run(&mut conn, &update, &[Value::String("x".into()), Value::Int(2)]).await;
    assert_eq!(events, vec![ExecEvent::Affected(0)]);
    run(&mut conn, &format!("DROP TABLE {t}"), &[]).await;
}

/// 从手动提交切回自动提交时，未提交的事务应被提交（Connection::set_autocommit 的约定）。
#[tokio::test]
async fn switching_back_to_autocommit_commits_pending_work() {
    let Some(mut conn) = connect_or_skip().await else { return };
    let t = "DBLENS_DM_AUTOCOMMIT";
    run(&mut conn, &format!("DROP TABLE IF EXISTS {t}"), &[]).await;
    run(&mut conn, &format!("CREATE TABLE {t} (id INT)"), &[]).await;
    conn.set_autocommit(false).await.unwrap();
    run(&mut conn, &format!("INSERT INTO {t} VALUES (1)"), &[]).await;
    // 不调用 commit，直接切回自动提交
    conn.set_autocommit(true).await.unwrap();
    let Some(mut other) = connect_or_skip().await else { return };
    let n = first_value(&run(&mut other, &format!("SELECT COUNT(*) FROM {t}"), &[]).await);
    run(&mut conn, &format!("DROP TABLE {t}"), &[]).await;
    assert_eq!(n, Value::Int(1), "切回自动提交后，其他连接应能看到未显式提交的行");
}

/// 空字符串与空二进制作为参数：绑定不能出错，也不能读到无效内存。
#[tokio::test]
async fn empty_string_and_binary_params() {
    let Some(mut conn) = connect_or_skip().await else { return };
    let t = "DBLENS_DM_EMPTY_PARAMS";
    run(&mut conn, &format!("DROP TABLE IF EXISTS {t}"), &[]).await;
    run(&mut conn, &format!("CREATE TABLE {t} (id INT, name VARCHAR(100), data VARBINARY(100))"), &[]).await;
    let insert = format!("INSERT INTO {t} VALUES (?, ?, ?)");
    let events =
        run(&mut conn, &insert, &[Value::Int(1), Value::String(String::new()), Value::Bytes(Vec::new())])
            .await;
    assert_eq!(events, vec![ExecEvent::Affected(1)]);
    let select = format!(
        "SELECT name, data, CASE WHEN name IS NULL THEN 1 ELSE 0 END, CASE WHEN data IS NULL THEN 1 ELSE 0 END FROM {t} WHERE id = ?"
    );
    let events = run(&mut conn, &select, &[Value::Int(1)]).await;
    let row = events
        .iter()
        .find_map(|e| if let ExecEvent::Rows(rows) = e { Some(rows[0].clone()) } else { None })
        .unwrap();
    run(&mut conn, &format!("DROP TABLE {t}"), &[]).await;
    // DM（默认兼容模式）不像 Oracle 那样把 '' 当作 NULL：空字符串和空二进制都原样读回，IS NULL 为假
    assert_eq!(
        row,
        vec![Value::String(String::new()), Value::Bytes(Vec::new()), Value::Int(0), Value::Int(0)]
    );
}
