use chrono::{NaiveDate, NaiveDateTime, NaiveTime};
use dblens_core::{Error, Result, Value};
use dblens_driver::ColumnMeta;
use mysql_async::consts::{ColumnFlags, ColumnType};
use mysql_async::{Column, Value as MyValue};

/// MySQL 中 binary 字符集的编号。
const BINARY_CHARSET: u16 = 63;

fn is_binary(col: &Column) -> bool {
    col.character_set() == BINARY_CHARSET
}

/// 结果集列的原生类型名（协议层能看到的近似值）。
pub(crate) fn native_type_name(col: &Column) -> String {
    use ColumnType::*;
    let base = match col.column_type() {
        MYSQL_TYPE_TINY => "TINYINT",
        MYSQL_TYPE_SHORT => "SMALLINT",
        MYSQL_TYPE_INT24 => "MEDIUMINT",
        MYSQL_TYPE_LONG => "INT",
        MYSQL_TYPE_LONGLONG => "BIGINT",
        MYSQL_TYPE_FLOAT => "FLOAT",
        MYSQL_TYPE_DOUBLE => "DOUBLE",
        MYSQL_TYPE_DECIMAL | MYSQL_TYPE_NEWDECIMAL => "DECIMAL",
        MYSQL_TYPE_DATE | MYSQL_TYPE_NEWDATE => "DATE",
        MYSQL_TYPE_TIME | MYSQL_TYPE_TIME2 => "TIME",
        MYSQL_TYPE_DATETIME | MYSQL_TYPE_DATETIME2 => "DATETIME",
        MYSQL_TYPE_TIMESTAMP | MYSQL_TYPE_TIMESTAMP2 => "TIMESTAMP",
        MYSQL_TYPE_YEAR => "YEAR",
        MYSQL_TYPE_BIT => "BIT",
        MYSQL_TYPE_JSON => "JSON",
        MYSQL_TYPE_ENUM => "ENUM",
        MYSQL_TYPE_SET => "SET",
        MYSQL_TYPE_GEOMETRY => "GEOMETRY",
        MYSQL_TYPE_NULL => "NULL",
        MYSQL_TYPE_TINY_BLOB | MYSQL_TYPE_MEDIUM_BLOB | MYSQL_TYPE_LONG_BLOB | MYSQL_TYPE_BLOB => {
            if is_binary(col) { "BLOB" } else { "TEXT" }
        }
        MYSQL_TYPE_VARCHAR | MYSQL_TYPE_VAR_STRING => {
            if is_binary(col) {
                "VARBINARY"
            } else {
                "VARCHAR"
            }
        }
        MYSQL_TYPE_STRING => {
            if is_binary(col) {
                "BINARY"
            } else {
                "CHAR"
            }
        }
        _ => "UNKNOWN",
    };
    if col.flags().contains(ColumnFlags::UNSIGNED_FLAG) {
        format!("{base} UNSIGNED")
    } else {
        base.to_string()
    }
}

pub(crate) fn column_meta(col: &Column) -> ColumnMeta {
    let table = col.org_table_str();
    let source = col.org_name_str();
    let is_decimal =
        matches!(col.column_type(), ColumnType::MYSQL_TYPE_DECIMAL | ColumnType::MYSQL_TYPE_NEWDECIMAL);
    ColumnMeta {
        name: col.name_str().into_owned(),
        type_name: native_type_name(col),
        nullable: Some(!col.flags().contains(ColumnFlags::NOT_NULL_FLAG)),
        precision: None,
        scale: is_decimal.then(|| i32::from(col.decimals())),
        source_table: (!table.is_empty()).then(|| table.into_owned()),
        source_column: (!source.is_empty()).then(|| source.into_owned()),
    }
}

/// 把驱动返回的值转换为 `Value`。文本协议下所有值都是 Bytes，需要按列类型解析；
/// 二进制协议（带参数执行时）直接给出数值和时间。
pub(crate) fn to_value(v: MyValue, col: &Column) -> Value {
    match v {
        MyValue::NULL => Value::Null,
        MyValue::Int(i) => Value::Int(i),
        MyValue::UInt(u) => Value::UInt(u),
        MyValue::Float(f) => Value::Float(f64::from(f)),
        MyValue::Double(d) => Value::Float(d),
        MyValue::Date(y, mo, d, h, mi, s, us) => date_value(col, y, mo, d, h, mi, s, us),
        MyValue::Time(neg, days, h, mi, s, us) => {
            if !neg && days == 0 {
                match NaiveTime::from_hms_micro_opt(h.into(), mi.into(), s.into(), us) {
                    Some(t) => Value::Time(t),
                    None => other(col, format!("{h:02}:{mi:02}:{s:02}.{us:06}")),
                }
            } else {
                let sign = if neg { "-" } else { "" };
                let hours = u32::from(h) + days * 24;
                other(col, format!("{sign}{hours:02}:{mi:02}:{s:02}"))
            }
        }
        MyValue::Bytes(b) => bytes_value(b, col),
    }
}

fn other(col: &Column, text: String) -> Value {
    Value::Other { type_name: native_type_name(col), text }
}

#[allow(clippy::too_many_arguments)]
fn date_value(col: &Column, y: u16, mo: u8, d: u8, h: u8, mi: u8, s: u8, us: u32) -> Value {
    let date = NaiveDate::from_ymd_opt(y.into(), mo.into(), d.into());
    let is_date_only =
        matches!(col.column_type(), ColumnType::MYSQL_TYPE_DATE | ColumnType::MYSQL_TYPE_NEWDATE);
    match (date, is_date_only) {
        (Some(date), true) => Value::Date(date),
        (Some(date), false) => match date.and_hms_micro_opt(h.into(), mi.into(), s.into(), us) {
            Some(dt) => Value::DateTime(dt),
            None => other(col, format!("{y:04}-{mo:02}-{d:02} {h:02}:{mi:02}:{s:02}.{us:06}")),
        },
        // 零日期（0000-00-00）等 chrono 无法表示的值，保留原文
        (None, _) => other(col, format!("{y:04}-{mo:02}-{d:02}")),
    }
}

fn bytes_value(b: Vec<u8>, col: &Column) -> Value {
    use ColumnType::*;
    let text = || String::from_utf8_lossy(&b).into_owned();
    match col.column_type() {
        MYSQL_TYPE_TINY | MYSQL_TYPE_SHORT | MYSQL_TYPE_INT24 | MYSQL_TYPE_LONG | MYSQL_TYPE_LONGLONG
        | MYSQL_TYPE_YEAR => {
            let s = text();
            let parsed = if col.flags().contains(ColumnFlags::UNSIGNED_FLAG) {
                s.parse::<u64>().ok().map(Value::UInt)
            } else {
                s.parse::<i64>().ok().map(Value::Int)
            };
            parsed.unwrap_or_else(|| other(col, s))
        }
        MYSQL_TYPE_FLOAT | MYSQL_TYPE_DOUBLE => {
            let s = text();
            s.parse::<f64>().map(Value::Float).unwrap_or_else(|_| other(col, s))
        }
        MYSQL_TYPE_DECIMAL | MYSQL_TYPE_NEWDECIMAL => Value::Decimal(text()),
        MYSQL_TYPE_DATE | MYSQL_TYPE_NEWDATE => {
            let s = text();
            NaiveDate::parse_from_str(&s, "%Y-%m-%d").map(Value::Date).unwrap_or_else(|_| other(col, s))
        }
        MYSQL_TYPE_DATETIME | MYSQL_TYPE_DATETIME2 | MYSQL_TYPE_TIMESTAMP | MYSQL_TYPE_TIMESTAMP2 => {
            let s = text();
            NaiveDateTime::parse_from_str(&s, "%Y-%m-%d %H:%M:%S%.f")
                .map(Value::DateTime)
                .unwrap_or_else(|_| other(col, s))
        }
        MYSQL_TYPE_TIME | MYSQL_TYPE_TIME2 => {
            let s = text();
            NaiveTime::parse_from_str(&s, "%H:%M:%S%.f").map(Value::Time).unwrap_or_else(|_| other(col, s))
        }
        MYSQL_TYPE_JSON => {
            serde_json::from_slice(&b).map(Value::Json).unwrap_or_else(|_| Value::String(text()))
        }
        MYSQL_TYPE_BIT | MYSQL_TYPE_GEOMETRY => Value::Bytes(b),
        _ if is_binary(col) => Value::Bytes(b),
        _ => Value::String(text()),
    }
}

/// 把参数转换为 mysql_async 的值。
pub(crate) fn to_mysql_param(v: &Value) -> Result<MyValue> {
    use chrono::{Datelike, Timelike};
    let datetime = |dt: &NaiveDateTime| {
        MyValue::Date(
            dt.year() as u16,
            dt.month() as u8,
            dt.day() as u8,
            dt.hour() as u8,
            dt.minute() as u8,
            dt.second() as u8,
            dt.nanosecond() / 1000,
        )
    };
    Ok(match v {
        Value::Null => MyValue::NULL,
        Value::Bool(b) => MyValue::Int(i64::from(*b)),
        Value::Int(i) => MyValue::Int(*i),
        Value::UInt(u) => MyValue::UInt(*u),
        Value::Float(f) => MyValue::Double(*f),
        Value::Decimal(s) | Value::String(s) | Value::Interval(s) => MyValue::Bytes(s.as_bytes().to_vec()),
        Value::Bytes(b) => MyValue::Bytes(b.clone()),
        Value::Date(d) => MyValue::Date(d.year() as u16, d.month() as u8, d.day() as u8, 0, 0, 0, 0),
        Value::Time(t) => {
            MyValue::Time(false, 0, t.hour() as u8, t.minute() as u8, t.second() as u8, t.nanosecond() / 1000)
        }
        Value::DateTime(dt) => datetime(dt),
        // MySQL 没有带时区的时间类型，按该值自身时区的本地时间写入
        Value::DateTimeTz(dt) => datetime(&dt.naive_local()),
        Value::Json(j) => MyValue::Bytes(j.to_string().into_bytes()),
        Value::Uuid(u) => MyValue::Bytes(u.hyphenated().to_string().into_bytes()),
        Value::Lob(_) | Value::Other { .. } => {
            return Err(Error::internal(format!("该类型的值不能作为参数绑定：{v:?}")));
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{NaiveDate, NaiveTime};
    use mysql_async::consts::{ColumnFlags, ColumnType};

    fn col(t: ColumnType) -> Column {
        Column::new(t).with_character_set(255)
    }

    fn bin(t: ColumnType) -> Column {
        Column::new(t).with_character_set(63)
    }

    #[test]
    fn text_protocol_integers() {
        assert_eq!(
            to_value(MyValue::Bytes(b"-42".to_vec()), &col(ColumnType::MYSQL_TYPE_LONGLONG)),
            Value::Int(-42)
        );
        let unsigned = col(ColumnType::MYSQL_TYPE_LONGLONG).with_flags(ColumnFlags::UNSIGNED_FLAG);
        assert_eq!(
            to_value(MyValue::Bytes(b"18446744073709551615".to_vec()), &unsigned),
            Value::UInt(u64::MAX)
        );
        assert_eq!(native_type_name(&unsigned), "BIGINT UNSIGNED");
    }

    #[test]
    fn text_protocol_decimal_float_and_strings() {
        assert_eq!(
            to_value(MyValue::Bytes(b"123.4500".to_vec()), &col(ColumnType::MYSQL_TYPE_NEWDECIMAL)),
            Value::Decimal("123.4500".into())
        );
        assert_eq!(
            to_value(MyValue::Bytes(b"1.5".to_vec()), &col(ColumnType::MYSQL_TYPE_DOUBLE)),
            Value::Float(1.5)
        );
        assert_eq!(
            to_value(MyValue::Bytes("数洞".as_bytes().to_vec()), &col(ColumnType::MYSQL_TYPE_VAR_STRING)),
            Value::String("数洞".into())
        );
        assert_eq!(
            to_value(MyValue::Bytes(vec![1, 2, 3]), &bin(ColumnType::MYSQL_TYPE_VAR_STRING)),
            Value::Bytes(vec![1, 2, 3])
        );
        assert_eq!(native_type_name(&bin(ColumnType::MYSQL_TYPE_BLOB)), "BLOB");
        assert_eq!(native_type_name(&col(ColumnType::MYSQL_TYPE_BLOB)), "TEXT");
    }

    #[test]
    fn text_protocol_temporal() {
        assert_eq!(
            to_value(MyValue::Bytes(b"2024-02-29".to_vec()), &col(ColumnType::MYSQL_TYPE_DATE)),
            Value::Date(NaiveDate::from_ymd_opt(2024, 2, 29).unwrap())
        );
        assert_eq!(
            to_value(
                MyValue::Bytes(b"2024-02-29 13:45:30.123456".to_vec()),
                &col(ColumnType::MYSQL_TYPE_DATETIME)
            ),
            Value::DateTime(
                NaiveDate::from_ymd_opt(2024, 2, 29).unwrap().and_hms_micro_opt(13, 45, 30, 123456).unwrap()
            )
        );
        assert_eq!(
            to_value(MyValue::Bytes(b"2024-02-29 13:45:30".to_vec()), &col(ColumnType::MYSQL_TYPE_TIMESTAMP)),
            Value::DateTime(NaiveDate::from_ymd_opt(2024, 2, 29).unwrap().and_hms_opt(13, 45, 30).unwrap())
        );
        assert_eq!(
            to_value(MyValue::Bytes(b"13:45:30".to_vec()), &col(ColumnType::MYSQL_TYPE_TIME)),
            Value::Time(NaiveTime::from_hms_opt(13, 45, 30).unwrap())
        );
        // 超出一天范围的 TIME 和零日期无法用 chrono 表示，保留原文
        assert_eq!(
            to_value(MyValue::Bytes(b"838:59:59".to_vec()), &col(ColumnType::MYSQL_TYPE_TIME)),
            Value::Other { type_name: "TIME".into(), text: "838:59:59".into() }
        );
        assert_eq!(
            to_value(MyValue::Bytes(b"0000-00-00".to_vec()), &col(ColumnType::MYSQL_TYPE_DATE)),
            Value::Other { type_name: "DATE".into(), text: "0000-00-00".into() }
        );
    }

    #[test]
    fn binary_protocol_values() {
        assert_eq!(to_value(MyValue::Int(7), &col(ColumnType::MYSQL_TYPE_LONG)), Value::Int(7));
        assert_eq!(to_value(MyValue::Double(2.5), &col(ColumnType::MYSQL_TYPE_DOUBLE)), Value::Float(2.5));
        assert_eq!(
            to_value(MyValue::Date(2024, 2, 29, 0, 0, 0, 0), &col(ColumnType::MYSQL_TYPE_DATE)),
            Value::Date(NaiveDate::from_ymd_opt(2024, 2, 29).unwrap())
        );
        assert_eq!(
            to_value(MyValue::Date(2024, 2, 29, 13, 45, 30, 5), &col(ColumnType::MYSQL_TYPE_DATETIME)),
            Value::DateTime(
                NaiveDate::from_ymd_opt(2024, 2, 29).unwrap().and_hms_micro_opt(13, 45, 30, 5).unwrap()
            )
        );
        assert_eq!(
            to_value(MyValue::Time(false, 0, 13, 45, 30, 0), &col(ColumnType::MYSQL_TYPE_TIME)),
            Value::Time(NaiveTime::from_hms_opt(13, 45, 30).unwrap())
        );
        assert_eq!(to_value(MyValue::NULL, &col(ColumnType::MYSQL_TYPE_LONG)), Value::Null);
    }

    #[test]
    fn json_column() {
        assert_eq!(
            to_value(MyValue::Bytes(br#"{"a": 1}"#.to_vec()), &bin(ColumnType::MYSQL_TYPE_JSON)),
            Value::Json(serde_json::json!({"a": 1}))
        );
    }

    #[test]
    fn column_meta_reads_source_and_nullability() {
        let c = Column::new(ColumnType::MYSQL_TYPE_VAR_STRING)
            .with_name(b"n")
            .with_org_name(b"name")
            .with_org_table(b"person")
            .with_flags(ColumnFlags::NOT_NULL_FLAG)
            .with_character_set(255);
        let m = column_meta(&c);
        assert_eq!((m.name.as_str(), m.type_name.as_str(), m.nullable), ("n", "VARCHAR", Some(false)));
        assert_eq!((m.source_table.as_deref(), m.source_column.as_deref()), (Some("person"), Some("name")));
    }

    #[test]
    fn params_convert() {
        assert_eq!(to_mysql_param(&Value::Int(1)).unwrap(), MyValue::Int(1));
        assert_eq!(to_mysql_param(&Value::Bool(true)).unwrap(), MyValue::Int(1));
        assert_eq!(to_mysql_param(&Value::String("x".into())).unwrap(), MyValue::Bytes(b"x".to_vec()));
        assert_eq!(to_mysql_param(&Value::Null).unwrap(), MyValue::NULL);
        assert!(to_mysql_param(&Value::Other { type_name: "x".into(), text: "y".into() }).is_err());
    }
}
