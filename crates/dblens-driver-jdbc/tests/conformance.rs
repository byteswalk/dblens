//! 一致性测试。`DBLENS_TEST_GBASE_URL`（如 `gbase://admin:密码@192.168.139.106:9088/dblens_test?server_name=gbase01&jdbc_dir=绝对路径`），
//! 另需 `DBLENS_JAVA`、`DBLENS_BRIDGE_JAR`。

use chrono::NaiveDate;
use dblens_core::Value;
use dblens_driver::conformance::Fixture;
use dblens_driver::{ConnectParams, Driver, collect_all};
use dblens_driver_jdbc::GbaseDriver;

/// LVARCHAR 最长 32739，10 万字符只能放在 TEXT 列里，所以先建好这张表。
/// 测试运行在 tokio 里，不能再阻塞等待，换一个线程跑自己的运行时。
fn seed_large_text(params: ConnectParams) {
    std::thread::spawn(move || {
        tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(async move {
            let mut c = GbaseDriver.connect(&params).await.unwrap();
            for (sql, args) in [
                ("DROP TABLE IF EXISTS dblens_conf_large", vec![]),
                ("CREATE TABLE dblens_conf_large (t TEXT)", vec![]),
                ("INSERT INTO dblens_conf_large (t) VALUES (?)", vec![Value::Bytes(vec![b'a'; 100_000])]),
            ] {
                let mut s = c.execute(sql, &args).await.unwrap();
                collect_all(&mut *s, 10).await.unwrap();
            }
            c.close().await.unwrap();
        });
    })
    .join()
    .unwrap();
}

fn fixture(params: ConnectParams) -> Fixture {
    seed_large_text(params.clone());
    let day = NaiveDate::from_ymd_opt(2024, 2, 29).unwrap();
    Fixture {
        driver: Box::new(GbaseDriver),
        params,
        select_one: "SELECT 1 FROM sysmaster:sysdual",
        gen_rows: |n| {
            format!(
                "SELECT FIRST {n} ROW_NUMBER() OVER (ORDER BY a.tabid, b.tabid) AS n FROM systables a, systables b ORDER BY n"
            )
        },
        long_running: "SELECT COUNT(*) FROM systables a, systables b, systables c, systables d, systables e",
        large_text: "SELECT t FROM dblens_conf_large",
        create_table: |t| format!("CREATE TABLE {t} (id INT PRIMARY KEY, name VARCHAR(100))"),
        drop_table_if_exists: |t| format!("DROP TABLE IF EXISTS {t}"),
        insert_with_params: |t| format!("INSERT INTO {t} (id, name) VALUES (?, ?)"),
        select_name_by_id: |t| format!("SELECT name FROM {t} WHERE id = ?"),
        count_rows: |t| format!("SELECT COUNT(*) FROM {t}"),
        missing_table_query: "SELECT * FROM dblens_no_such_table",
        type_cases: vec![
            ("SELECT NULL::INT FROM sysmaster:sysdual", Value::Null),
            ("SELECT 42::INT FROM sysmaster:sysdual", Value::Int(42)),
            ("SELECT 42::BIGINT FROM sysmaster:sysdual", Value::Int(42)),
            ("SELECT 1.5::FLOAT FROM sysmaster:sysdual", Value::Float(1.5)),
            ("SELECT 123.4500::DECIMAL(10,4) FROM sysmaster:sysdual", Value::Decimal("123.4500".into())),
            ("SELECT '数洞'::VARCHAR(20) FROM sysmaster:sysdual", Value::String("数洞".into())),
            ("SELECT 't'::BOOLEAN FROM sysmaster:sysdual", Value::Bool(true)),
            ("SELECT MDY(2, 29, 2024) FROM sysmaster:sysdual", Value::Date(day)),
        ],
    }
}

dblens_driver::conformance_tests!("DBLENS_TEST_GBASE_URL", fixture);

/// 杀掉 Java 进程：运行中的连接报错，下一次连接重新启动进程。
#[cfg(windows)]
#[tokio::test]
async fn bridge_restarts_after_it_dies() {
    let Ok(url) = std::env::var("DBLENS_TEST_GBASE_URL") else { return };
    let (_, params) = ConnectParams::from_url(&url).unwrap();
    let mut c = GbaseDriver.connect(&params).await.unwrap();
    c.ping().await.unwrap();
    let status = std::process::Command::new("powershell")
        .args([
            "-NoProfile",
            "-Command",
            "Get-CimInstance Win32_Process -Filter \"Name='java.exe'\" | Where-Object { $_.CommandLine -like '*dblens-bridge*' } | ForEach-Object { Stop-Process -Id $_.ProcessId -Force }",
        ])
        .status()
        .unwrap();
    assert!(status.success());
    assert!(c.ping().await.is_err());
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    let mut again = GbaseDriver.connect(&params).await.unwrap();
    again.ping().await.unwrap();
    again.close().await.unwrap();
}
