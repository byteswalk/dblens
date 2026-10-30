//! 经 JDBC 桥接时的一致性测试，夹具和原生驱动相同。
//! 环境变量 DBLENS_TEST_DM_JDBC_URL，连接选项 jdbc_dir 指向驱动所在目录；另需 DBLENS_JAVA、DBLENS_BRIDGE_JAR。

use chrono::{NaiveDate, NaiveTime};
use dblens_core::Value;
use dblens_driver::ConnectParams;
use dblens_driver::conformance::Fixture;
use dblens_driver_jdbc::DamengJdbcDriver;

fn fixture(params: ConnectParams) -> Fixture {
    Fixture {
        driver: Box::new(DamengJdbcDriver),
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

dblens_driver::conformance_tests!("DBLENS_TEST_DM_JDBC_URL", fixture);
