//! 值的转换：结果按每个值实际的存储类别取，不按声明类型猜（SQLite 是动态类型，同一列可以存不同类型）。

use dblens_core::{Error, Result, Value, msg};
use rusqlite::types::{Value as SqlValue, ValueRef};

/// 结果中的一个值。超过 `lob_limit` 字节的文本、二进制截断为预览。
pub(crate) fn sqlite_value(v: ValueRef<'_>, lob_limit: usize) -> Value {
    match v {
        ValueRef::Null => Value::Null,
        ValueRef::Integer(i) => Value::Int(i),
        ValueRef::Real(f) => Value::Float(f),
        ValueRef::Text(t) => Value::String(String::from_utf8_lossy(t).into_owned()).cap_large(lob_limit),
        ValueRef::Blob(b) => Value::Bytes(b.to_vec()).cap_large(lob_limit),
    }
}

const DATETIME: &str = "%Y-%m-%d %H:%M:%S%.f";

/// 参数值。SQLite 没有日期、小数类型：写成文本，按 SQLite 的日期函数能识别的格式。
pub(crate) fn to_sql(v: &Value) -> Result<SqlValue> {
    Ok(match v {
        Value::Null => SqlValue::Null,
        Value::Bool(b) => SqlValue::Integer(i64::from(*b)),
        Value::Int(i) => SqlValue::Integer(*i),
        Value::UInt(u) => match i64::try_from(*u) {
            Ok(i) => SqlValue::Integer(i),
            Err(_) => SqlValue::Text(u.to_string()),
        },
        Value::Float(f) => SqlValue::Real(*f),
        Value::Decimal(s) | Value::String(s) | Value::Interval(s) => SqlValue::Text(s.clone()),
        Value::Bytes(b) => SqlValue::Blob(b.clone()),
        Value::Date(d) => SqlValue::Text(d.format("%Y-%m-%d").to_string()),
        Value::Time(t) => SqlValue::Text(t.format("%H:%M:%S%.f").to_string()),
        Value::DateTime(dt) => SqlValue::Text(dt.format(DATETIME).to_string()),
        Value::DateTimeTz(dt) => SqlValue::Text(dt.format("%Y-%m-%d %H:%M:%S%.f%:z").to_string()),
        Value::Json(j) => SqlValue::Text(j.to_string()),
        Value::Uuid(u) => SqlValue::Text(u.to_string()),
        Value::Lob(_) | Value::Other { .. } => {
            return Err(Error::internal(msg!("driver.unbindable_value", value = format!("{v:?}"))));
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{FixedOffset, NaiveDate, TimeZone};

    #[test]
    fn values_follow_storage_class() {
        assert_eq!(sqlite_value(ValueRef::Integer(7), 10), Value::Int(7));
        assert_eq!(sqlite_value(ValueRef::Real(1.5), 10), Value::Float(1.5));
        assert_eq!(sqlite_value(ValueRef::Text("中文".as_bytes()), 100), Value::String("中文".into()));
        assert!(
            matches!(sqlite_value(ValueRef::Text(b"abcdefghijk"), 4), Value::Lob(l) if !l.binary && l.truncated)
        );
        assert!(matches!(sqlite_value(ValueRef::Blob(&[0; 8]), 4), Value::Lob(l) if l.binary));
        assert_eq!(sqlite_value(ValueRef::Blob(&[1, 2]), 4), Value::Bytes(vec![1, 2]));
    }

    #[test]
    fn params_become_sqlite_values() {
        let dt = NaiveDate::from_ymd_opt(2026, 11, 5).unwrap().and_hms_opt(10, 20, 30).unwrap();
        assert_eq!(to_sql(&Value::DateTime(dt)).unwrap(), SqlValue::Text("2026-11-05 10:20:30".into()));
        let ms = NaiveDate::from_ymd_opt(2026, 11, 5).unwrap().and_hms_milli_opt(10, 20, 30, 250).unwrap();
        assert_eq!(to_sql(&Value::DateTime(ms)).unwrap(), SqlValue::Text("2026-11-05 10:20:30.250".into()));
        let tz = FixedOffset::east_opt(8 * 3600).unwrap().from_local_datetime(&dt).unwrap();
        assert_eq!(
            to_sql(&Value::DateTimeTz(tz)).unwrap(),
            SqlValue::Text("2026-11-05 10:20:30+08:00".into())
        );
        assert_eq!(to_sql(&Value::Bool(true)).unwrap(), SqlValue::Integer(1));
        assert_eq!(to_sql(&Value::Decimal("12.50".into())).unwrap(), SqlValue::Text("12.50".into()));
        assert_eq!(to_sql(&Value::UInt(u64::MAX)).unwrap(), SqlValue::Text(u64::MAX.to_string()));
        assert!(to_sql(&Value::Other { type_name: "x".into(), text: "y".into() }).is_err());
    }
}
