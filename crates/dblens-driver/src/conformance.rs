//! 驱动一致性测试套件。
//!
//! 用法：驱动 crate 在 dev-dependencies 中开启 `dblens-driver` 的 `conformance` 特性，
//! 写一个 `fn fixture(params: ConnectParams) -> Fixture`，再调用
//! `dblens_driver::conformance_tests!("DBLENS_TEST_XXX_URL", fixture);`。

use std::time::{Duration, Instant};

use dblens_core::{Error, Value};

use crate::{ConnectParams, Connection, Driver, ExecEvent, collect_all};

/// 每种数据库提供一份夹具：驱动、连接参数，以及该数据库方言下的测试 SQL。
pub struct Fixture {
    pub driver: Box<dyn Driver>,
    pub params: ConnectParams,
    /// 返回单行单列整数 1。
    pub select_one: &'static str,
    /// 返回 n 行单列整数 1..=n，并按升序排列。
    pub gen_rows: fn(usize) -> String,
    /// 一个至少运行 10 秒、被取消时会报错的查询。
    pub long_running: &'static str,
    /// 返回单行单列、长度为 100000 的字符串。
    pub large_text: &'static str,
    /// 建表：列 id（整数主键）、name（可变长字符串，100）。
    pub create_table: fn(&str) -> String,
    pub drop_table_if_exists: fn(&str) -> String,
    /// INSERT，两个占位参数依次为 id、name。
    pub insert_with_params: fn(&str) -> String,
    /// 按 id（一个占位参数）查询 name。
    pub select_name_by_id: fn(&str) -> String,
    pub count_rows: fn(&str) -> String,
    /// 查询一张不存在的表。
    pub missing_table_query: &'static str,
    /// （返回单行单列的 SQL，期望值）
    pub type_cases: Vec<(&'static str, Value)>,
}

/// 从环境变量读取连接 URL。未设置时返回 None，测试跳过。
pub fn load(env: &str, make: fn(ConnectParams) -> Fixture) -> Option<Fixture> {
    match std::env::var(env) {
        Ok(url) => {
            let (_, params) = ConnectParams::from_url(&url).expect("测试连接 URL 无效");
            Some(make(params))
        }
        Err(_) => {
            eprintln!("跳过：未设置环境变量 {env}");
            None
        }
    }
}

async fn connect(f: &Fixture) -> Box<dyn Connection> {
    f.driver.connect(&f.params).await.expect("连接测试库失败")
}

async fn run(conn: &mut dyn Connection, sql: &str, params: &[Value]) -> Result<Vec<ExecEvent>, Error> {
    let mut stream = conn.execute(sql, params).await?;
    collect_all(stream.as_mut(), 1000).await
}

fn single_value(events: &[ExecEvent]) -> Value {
    let rows: Vec<&Vec<Value>> = events
        .iter()
        .filter_map(|e| if let ExecEvent::Rows(r) = e { Some(r.iter()) } else { None })
        .flatten()
        .collect();
    assert_eq!(rows.len(), 1, "期望 1 行，实际事件：{events:?}");
    assert_eq!(rows[0].len(), 1, "期望 1 列，实际：{:?}", rows[0]);
    rows[0][0].clone()
}

fn as_i64(v: &Value) -> i64 {
    match v {
        Value::Int(i) => *i,
        Value::UInt(u) => i64::try_from(*u).expect("整数超出 i64 范围"),
        Value::Decimal(s) => s.parse().unwrap_or_else(|_| panic!("不是整数：{s}")),
        other => panic!("不是整数：{other:?}"),
    }
}

pub async fn ping_and_server_info(f: &Fixture) {
    let mut c = connect(f).await;
    c.ping().await.expect("ping 失败");
    let info = c.server_info().await.expect("server_info 失败");
    assert!(!info.version.is_empty(), "版本字符串为空");
    c.close().await.expect("关闭连接失败");
}

pub async fn select_one(f: &Fixture) {
    let mut c = connect(f).await;
    let events = run(c.as_mut(), f.select_one, &[]).await.expect("执行失败");
    assert!(
        matches!(events.first(), Some(ExecEvent::ResultStart { columns }) if columns.len() == 1),
        "{events:?}"
    );
    assert_eq!(events.last(), Some(&ExecEvent::ResultEnd), "{events:?}");
    assert_eq!(as_i64(&single_value(&events)), 1);
}

pub async fn row_batching(f: &Fixture) {
    let mut c = connect(f).await;
    let sql = (f.gen_rows)(1000);
    let mut stream = c.execute(&sql, &[]).await.expect("执行失败");
    let (mut sizes, mut values) = (Vec::new(), Vec::new());
    let (mut saw_start, mut saw_end) = (false, false);
    while let Some(event) = stream.next(300).await.expect("取数失败") {
        match event {
            ExecEvent::ResultStart { .. } => saw_start = true,
            ExecEvent::Rows(rows) => {
                sizes.push(rows.len());
                values.extend(rows.iter().map(|r| as_i64(&r[0])));
            }
            ExecEvent::ResultEnd => saw_end = true,
            ExecEvent::Message(_) => {}
            other => panic!("意外事件：{other:?}"),
        }
    }
    assert!(saw_start && saw_end, "缺少 ResultStart 或 ResultEnd");
    assert!(sizes.iter().all(|s| *s <= 300), "单批行数超过上限：{sizes:?}");
    assert_eq!(values, (1..=1000).collect::<Vec<i64>>());
}

pub async fn dml_and_params(f: &Fixture) {
    let t = "dblens_conf_dml";
    let mut c = connect(f).await;
    run(c.as_mut(), &(f.drop_table_if_exists)(t), &[]).await.expect("删表失败");
    run(c.as_mut(), &(f.create_table)(t), &[]).await.expect("建表失败");
    let events = run(c.as_mut(), &(f.insert_with_params)(t), &[Value::Int(1), Value::String("张三".into())])
        .await
        .expect("插入失败");
    assert!(events.contains(&ExecEvent::Affected(1)), "{events:?}");
    let events = run(c.as_mut(), &(f.select_name_by_id)(t), &[Value::Int(1)]).await.expect("查询失败");
    assert_eq!(single_value(&events), Value::String("张三".into()));
    run(c.as_mut(), &(f.drop_table_if_exists)(t), &[]).await.expect("删表失败");
}

pub async fn transaction_commit_and_rollback(f: &Fixture) {
    let t = "dblens_conf_tx";
    let mut c = connect(f).await;
    run(c.as_mut(), &(f.drop_table_if_exists)(t), &[]).await.expect("删表失败");
    run(c.as_mut(), &(f.create_table)(t), &[]).await.expect("建表失败");

    c.set_autocommit(false).await.expect("关闭自动提交失败");
    run(c.as_mut(), &(f.insert_with_params)(t), &[Value::Int(1), Value::String("a".into())])
        .await
        .expect("插入失败");
    c.rollback().await.expect("回滚失败");
    let n = as_i64(&single_value(&run(c.as_mut(), &(f.count_rows)(t), &[]).await.expect("计数失败")));
    assert_eq!(n, 0, "回滚后应为 0 行");

    run(c.as_mut(), &(f.insert_with_params)(t), &[Value::Int(2), Value::String("b".into())])
        .await
        .expect("插入失败");
    c.commit().await.expect("提交失败");
    c.set_autocommit(true).await.expect("恢复自动提交失败");

    // 用另一个连接确认数据确实已提交
    let mut other = connect(f).await;
    let n = as_i64(&single_value(&run(other.as_mut(), &(f.count_rows)(t), &[]).await.expect("计数失败")));
    assert_eq!(n, 1, "提交后其他连接应能看到 1 行");
    run(c.as_mut(), &(f.drop_table_if_exists)(t), &[]).await.expect("删表失败");
}

pub async fn cancel_running_query(f: &Fixture) {
    let mut c = connect(f).await;
    let handle = c.cancel_handle();
    let canceller = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(500)).await;
        handle.cancel().await
    });
    let started = Instant::now();
    let result = run(c.as_mut(), f.long_running, &[]).await;
    canceller.await.expect("取消任务异常退出").expect("发送取消失败");
    assert_eq!(result.unwrap_err(), Error::Cancelled);
    // 达梦服务器端的取消约 3～4 秒才生效（见 docs/notes/dameng-dpi-spike.md），上限放宽到 10 秒
    assert!(started.elapsed() < Duration::from_secs(10), "取消耗时过长：{:?}", started.elapsed());
    c.ping().await.expect("取消后连接应仍可用");
}

pub async fn sql_error_has_code(f: &Fixture) {
    let mut c = connect(f).await;
    match run(c.as_mut(), f.missing_table_query, &[]).await {
        Err(Error::Sql { code, message, .. }) => {
            assert!(!code.is_empty(), "错误码为空");
            assert!(!message.is_empty(), "错误信息为空");
        }
        other => panic!("期望 SQL 错误，实际：{other:?}"),
    }
    c.ping().await.expect("出错后连接应仍可用");
}

pub async fn large_text_becomes_lob_preview(f: &Fixture) {
    let mut c = connect(f).await;
    match single_value(&run(c.as_mut(), f.large_text, &[]).await.expect("执行失败")) {
        Value::Lob(lob) => {
            assert!(!lob.binary && lob.truncated);
            assert_eq!(lob.total_len, 100_000);
            assert_eq!(lob.preview.len(), dblens_core::DEFAULT_LOB_PREVIEW_BYTES);
        }
        other => panic!("期望 LOB 预览，实际：{other:?}"),
    }
}

pub async fn lob_limit_reads_full_value(f: &Fixture) {
    let mut c = connect(f).await;
    c.set_lob_limit(1024 * 1024);
    match single_value(&run(c.as_mut(), f.large_text, &[]).await.expect("执行失败")) {
        Value::String(s) => assert_eq!(s.len(), 100_000),
        other => panic!("期望完整的文本，实际：{other:?}"),
    }
    // 改回默认后又是预览
    c.set_lob_limit(dblens_core::DEFAULT_LOB_PREVIEW_BYTES);
    let v = single_value(&run(c.as_mut(), f.large_text, &[]).await.expect("执行失败"));
    assert!(matches!(v, Value::Lob(_)), "{v:?}");
}

pub async fn type_round_trip(f: &Fixture) {
    let mut c = connect(f).await;
    for (sql, expected) in &f.type_cases {
        let events = run(c.as_mut(), sql, &[]).await.unwrap_or_else(|e| panic!("{sql} 执行失败：{e}"));
        assert_eq!(&single_value(&events), expected, "SQL：{sql}");
    }
}

/// 生成全部一致性测试。必须在定义了 `fixture` 函数的测试文件根部调用。
#[macro_export]
macro_rules! conformance_tests {
    ($env:literal, $make:path) => {
        mod conformance {
            use super::*;

            #[tokio::test]
            async fn ping_and_server_info() {
                if let Some(f) = $crate::conformance::load($env, $make) {
                    $crate::conformance::ping_and_server_info(&f).await;
                }
            }
            #[tokio::test]
            async fn select_one() {
                if let Some(f) = $crate::conformance::load($env, $make) {
                    $crate::conformance::select_one(&f).await;
                }
            }
            #[tokio::test]
            async fn row_batching() {
                if let Some(f) = $crate::conformance::load($env, $make) {
                    $crate::conformance::row_batching(&f).await;
                }
            }
            #[tokio::test]
            async fn dml_and_params() {
                if let Some(f) = $crate::conformance::load($env, $make) {
                    $crate::conformance::dml_and_params(&f).await;
                }
            }
            #[tokio::test]
            async fn transaction_commit_and_rollback() {
                if let Some(f) = $crate::conformance::load($env, $make) {
                    $crate::conformance::transaction_commit_and_rollback(&f).await;
                }
            }
            #[tokio::test]
            async fn cancel_running_query() {
                if let Some(f) = $crate::conformance::load($env, $make) {
                    $crate::conformance::cancel_running_query(&f).await;
                }
            }
            #[tokio::test]
            async fn sql_error_has_code() {
                if let Some(f) = $crate::conformance::load($env, $make) {
                    $crate::conformance::sql_error_has_code(&f).await;
                }
            }
            #[tokio::test]
            async fn large_text_becomes_lob_preview() {
                if let Some(f) = $crate::conformance::load($env, $make) {
                    $crate::conformance::large_text_becomes_lob_preview(&f).await;
                }
            }
            #[tokio::test]
            async fn lob_limit_reads_full_value() {
                if let Some(f) = $crate::conformance::load($env, $make) {
                    $crate::conformance::lob_limit_reads_full_value(&f).await;
                }
            }
            #[tokio::test]
            async fn type_round_trip() {
                if let Some(f) = $crate::conformance::load($env, $make) {
                    $crate::conformance::type_round_trip(&f).await;
                }
            }
        }
    };
}
