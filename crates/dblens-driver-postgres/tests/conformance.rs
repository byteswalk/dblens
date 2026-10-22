use chrono::{FixedOffset, NaiveDate, TimeZone};
use dblens_core::Value;
use dblens_driver::ConnectParams;
use dblens_driver::conformance::Fixture;
use dblens_driver_postgres::PgDriver;

fn fixture(params: ConnectParams) -> Fixture {
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
        type_cases: vec![
            ("SELECT NULL::text", Value::Null),
            ("SELECT true", Value::Bool(true)),
            ("SELECT 42::int4", Value::Int(42)),
            ("SELECT 42::int8", Value::Int(42)),
            ("SELECT 1.5::float8", Value::Float(1.5)),
            ("SELECT 123.4500::numeric(10,4)", Value::Decimal("123.4500".into())),
            (
                "SELECT 12345678901234567890.123456789::numeric",
                Value::Decimal("12345678901234567890.123456789".into()),
            ),
            ("SELECT 'NaN'::numeric", Value::Decimal("NaN".into())),
            ("SELECT '数洞'::text", Value::String("数洞".into())),
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
            (
                "SELECT TIMESTAMPTZ '2024-02-29 13:45:30+08'",
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
            ("SELECT INTERVAL '1 day 02:03:04'", Value::Interval("0 mons 1 days 02:03:04".into())),
            ("SELECT 'x'::char(1)::\"char\"", Value::Other { type_name: "char".into(), text: "x".into() }),
        ],
    }
}

dblens_driver::conformance_tests!("DBLENS_TEST_PG_URL", fixture);
