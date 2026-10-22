use chrono::{NaiveDate, NaiveTime};
use dblens_core::Value;
use dblens_driver::ConnectParams;
use dblens_driver::conformance::Fixture;
use dblens_driver_mysql::MySqlDriver;

/// MySQL 与 MariaDB 都支持的类型用例。
fn common_type_cases() -> Vec<(&'static str, Value)> {
    vec![
        ("SELECT NULL", Value::Null),
        ("SELECT CAST(42 AS SIGNED)", Value::Int(42)),
        ("SELECT CAST(42 AS UNSIGNED)", Value::UInt(42)),
        ("SELECT CAST(1.5 AS DOUBLE)", Value::Float(1.5)),
        ("SELECT CAST('123.4500' AS DECIMAL(10,4))", Value::Decimal("123.4500".into())),
        ("SELECT '数洞'", Value::String("数洞".into())),
        ("SELECT CAST('2024-02-29' AS DATE)", Value::Date(NaiveDate::from_ymd_opt(2024, 2, 29).unwrap())),
        (
            "SELECT CAST('2024-02-29 13:45:30.123456' AS DATETIME(6))",
            Value::DateTime(
                NaiveDate::from_ymd_opt(2024, 2, 29).unwrap().and_hms_micro_opt(13, 45, 30, 123456).unwrap(),
            ),
        ),
        ("SELECT CAST('13:45:30' AS TIME)", Value::Time(NaiveTime::from_hms_opt(13, 45, 30).unwrap())),
        ("SELECT X'010203'", Value::Bytes(vec![1, 2, 3])),
    ]
}

fn base(params: ConnectParams, type_cases: Vec<(&'static str, Value)>) -> Fixture {
    Fixture {
        driver: Box::new(MySqlDriver),
        params,
        select_one: "SELECT 1",
        gen_rows: |n| {
            format!(
                "WITH RECURSIVE s(n) AS (SELECT 1 UNION ALL SELECT n + 1 FROM s WHERE n < {n}) SELECT n FROM s ORDER BY n"
            )
        },
        long_running: "SELECT COUNT(*) FROM information_schema.columns a, information_schema.columns b, information_schema.columns c",
        large_text: "SELECT REPEAT('a', 100000)",
        create_table: |t| {
            format!(
                "CREATE TABLE {t} (id INT PRIMARY KEY, name VARCHAR(100)) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4"
            )
        },
        drop_table_if_exists: |t| format!("DROP TABLE IF EXISTS {t}"),
        insert_with_params: |t| format!("INSERT INTO {t} (id, name) VALUES (?, ?)"),
        select_name_by_id: |t| format!("SELECT name FROM {t} WHERE id = ?"),
        count_rows: |t| format!("SELECT COUNT(*) FROM {t}"),
        missing_table_query: "SELECT * FROM dblens_no_such_table",
        type_cases,
    }
}

fn mysql_fixture(params: ConnectParams) -> Fixture {
    let mut cases = common_type_cases();
    cases.push(("SELECT CAST('{\"a\": 1}' AS JSON)", Value::Json(serde_json::json!({"a": 1}))));
    base(params, cases)
}

fn mariadb_fixture(params: ConnectParams) -> Fixture {
    base(params, common_type_cases())
}

mod mysql {
    use super::*;
    dblens_driver::conformance_tests!("DBLENS_TEST_MYSQL_URL", mysql_fixture);
}

mod mariadb {
    use super::*;
    dblens_driver::conformance_tests!("DBLENS_TEST_MARIADB_URL", mariadb_fixture);
}
