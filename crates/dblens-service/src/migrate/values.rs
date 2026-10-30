//! 迁移中的值（M5b 设计文档第 4、5 节）：把源库读出的值转换成目标列的类型（转换不了的交给 M2b 导入的文本转换），
//! 以及校验时的规整（两边都转换成目标列的类型后写成可比较的文本）。

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use chrono::{Local, NaiveDateTime, TimeZone, Timelike};
use dblens_core::schema::Column;
use dblens_core::{Error, LogicalType, Result, Value, msg};

use crate::import::{EmptyAs, convert};

/// 值的文本形式（转换成文本列、交给文本转换时用）。
fn text_of(v: &Value) -> String {
    match v {
        Value::Null => String::new(),
        Value::Bool(b) => (if *b { "1" } else { "0" }).into(),
        Value::Int(n) => n.to_string(),
        Value::UInt(n) => n.to_string(),
        Value::Float(f) => f.to_string(),
        Value::Decimal(s) | Value::String(s) | Value::Interval(s) => s.clone(),
        Value::Bytes(b) => format!("0x{}", b.iter().map(|x| format!("{x:02x}")).collect::<String>()),
        Value::Date(d) => d.to_string(),
        Value::Time(t) => t.to_string(),
        Value::DateTime(t) => t.to_string(),
        Value::DateTimeTz(t) => t.to_rfc3339(),
        Value::Json(j) => j.to_string(),
        Value::Uuid(u) => u.to_string(),
        Value::Lob(l) => l.preview.clone(),
        Value::Other { text, .. } => text.clone(),
    }
}

/// 布尔值的数值写法。
fn truthy(v: &Value) -> Option<bool> {
    match v {
        Value::Bool(b) => Some(*b),
        Value::Int(n) => Some(*n != 0),
        Value::UInt(n) => Some(*n != 0),
        Value::Decimal(s) => s.parse::<f64>().ok().map(|f| f != 0.0),
        _ => None,
    }
}

/// 把源值转换成目标列的类型。读不全的大字段（超过上限）算出错。
pub fn to_target(v: &Value, col: &Column) -> Result<Value> {
    use LogicalType as L;
    if let Value::Lob(l) = v {
        if l.truncated {
            return Err(Error::invalid(msg!("migrate.lob_too_large", column = col.name)));
        }
        // 没有截断的大字段：文本原样，二进制是 base64
        let full = if l.binary {
            Value::Bytes(STANDARD.decode(&l.preview).map_err(|e| {
                Error::invalid(msg!("migrate.bad_value", column = col.name, error = e.to_string()))
            })?)
        } else {
            Value::String(l.preview.clone())
        };
        return to_target(&full, col);
    }
    if matches!(v, Value::Null) {
        return Ok(Value::Null);
    }
    let logical = &col.data_type.logical;
    let out = match (logical, v) {
        (L::Bool, _) => match truthy(v) {
            Some(b) => Value::Bool(b),
            None => convert(v, col, EmptyAs::Null)?,
        },
        (L::Int { .. } | L::UInt { .. } | L::Decimal { .. } | L::Float { .. }, Value::Bool(b)) => {
            convert(&Value::Int(i64::from(*b)), col, EmptyAs::Null)?
        }
        (L::Char { .. } | L::VarChar { .. } | L::Text | L::Interval | L::Other, Value::String(_)) => {
            v.clone()
        }
        (L::Char { .. } | L::VarChar { .. } | L::Text | L::Interval | L::Other, _) => {
            Value::String(match v {
                // 二进制转文本时按 UTF-8 解码（大多是文本存在二进制列里）
                Value::Bytes(b) => String::from_utf8_lossy(b).into_owned(),
                _ => text_of(v),
            })
        }
        (L::Binary { .. } | L::VarBinary { .. } | L::Blob, Value::Bytes(_)) => v.clone(),
        (L::Binary { .. } | L::VarBinary { .. } | L::Blob, Value::String(s)) => {
            Value::Bytes(s.as_bytes().to_vec())
        }
        (L::Binary { .. } | L::VarBinary { .. } | L::Blob, Value::Uuid(u)) => {
            Value::Bytes(u.as_bytes().to_vec())
        }
        (L::Uuid, Value::Bytes(b)) if b.len() == 16 => {
            Value::Uuid(uuid::Uuid::from_slice(b).map_err(|e| {
                Error::invalid(msg!("migrate.bad_value", column = col.name, error = e.to_string()))
            })?)
        }
        (L::Json, Value::Json(_)) => v.clone(),
        (L::Json, _) => match serde_json::from_str(&text_of(v)) {
            Ok(j) => Value::Json(j),
            Err(_) => convert(v, col, EmptyAs::Null)?,
        },
        // 带时区转不带时区：按其自身偏移的本地时间
        (L::Timestamp { tz: false, .. }, Value::DateTimeTz(t)) => Value::DateTime(t.naive_local()),
        (L::Date, Value::DateTimeTz(t)) => Value::Date(t.naive_local().date()),
        (L::Timestamp { tz: true, .. }, Value::DateTime(t)) => {
            match Local.from_local_datetime(t).earliest() {
                Some(x) => Value::DateTimeTz(x.fixed_offset()),
                None => convert(v, col, EmptyAs::Null)?,
            }
        }
        (L::Time { .. }, Value::DateTimeTz(t)) => Value::Time(t.naive_local().time()),
        (L::Int { .. } | L::UInt { .. }, Value::Decimal(s)) => {
            // 整数位后面是 .0 的小数（如 Oracle 的 NUMBER）
            let s = s.trim().strip_suffix(".0").unwrap_or(s.trim());
            convert(&Value::String(s.to_string()), col, EmptyAs::Null)?
        }
        (L::Decimal { .. }, Value::Float(f)) => Value::Decimal(f.to_string()),
        _ => convert(v, col, EmptyAs::Null)?,
    };
    // 小数秒超出目标精度时先截断：有的库写入时四舍五入（达梦），有的截断，校验按截断比较
    Ok(match (out, logical) {
        (Value::DateTime(t), L::Timestamp { precision, .. }) => Value::DateTime(truncate(&t, *precision)),
        (Value::DateTimeTz(t), L::Timestamp { precision, .. }) => {
            let nanos = truncate(&t.naive_utc(), *precision).nanosecond();
            Value::DateTimeTz(t.with_nanosecond(nanos).unwrap_or(t))
        }
        (out, _) => out,
    })
}

/// 去掉小数末尾的 0（`1.50` → `1.5`，`2.00` → `2`）。
fn trim_decimal(s: &str) -> String {
    let s = s.trim();
    if !s.contains('.') || s.contains(['e', 'E']) {
        return s.to_string();
    }
    let t = s.trim_end_matches('0').trim_end_matches('.');
    if t.is_empty() || t == "-" { "0".into() } else { t.to_string() }
}

fn truncate(t: &NaiveDateTime, precision: Option<u8>) -> NaiveDateTime {
    let p = u32::from(precision.unwrap_or(6).min(9));
    let unit = 10u32.pow(9 - p);
    t.with_nanosecond(t.nanosecond() / unit * unit).unwrap_or(*t)
}

/// 校验用的规整：转换成目标列的类型后写成文本。小数去掉末尾的 0，时间按目标精度截断，CHAR 去掉末尾空格，
/// 浮点按 f32 精度比较（单精度列）。`empty_is_null`：目标按 Oracle 的规则把空串、空二进制存成 NULL。
pub fn normalize(v: &Value, col: &Column, empty_is_null: bool) -> String {
    use LogicalType as L;
    let logical = &col.data_type.logical;
    let Ok(v) = to_target(v, col) else { return format!("!{}", text_of(v)) };
    let empty = match &v {
        Value::String(s) => s.is_empty(),
        Value::Bytes(b) => b.is_empty(),
        _ => false,
    };
    match (&v, logical) {
        (Value::Null, _) => "NULL".into(),
        _ if empty && empty_is_null => "NULL".into(),
        (Value::Bool(b), _) => (if *b { "1" } else { "0" }).into(),
        (Value::Decimal(s), _) => trim_decimal(s),
        (Value::Int(n), _) => n.to_string(),
        (Value::Float(f), L::Float { bits }) if *bits <= 32 => (*f as f32).to_string(),
        (Value::Float(f), _) => f.to_string(),
        (Value::String(s), L::Char { .. }) => s.trim_end().to_string(),
        (Value::DateTime(t), L::Timestamp { precision, .. }) => truncate(t, *precision).to_string(),
        (Value::DateTimeTz(t), L::Timestamp { precision, .. }) => {
            truncate(&t.naive_utc(), *precision).to_string()
        }
        (Value::Json(j), _) => j.to_string(),
        _ => text_of(&v),
    }
}

/// 主键值的显示（报告中）。
pub fn key_text(values: &[Value]) -> String {
    values.iter().map(text_of).collect::<Vec<_>>().join(", ")
}

#[cfg(test)]
mod tests {
    use chrono::{FixedOffset, NaiveDate};
    use dblens_core::ColumnType;

    use super::*;

    fn col(native: &str, logical: LogicalType) -> Column {
        Column {
            name: "c".into(),
            ordinal: 1,
            data_type: ColumnType { native: native.into(), logical },
            nullable: true,
            default: None,
            comment: None,
            auto_increment: false,
            generated: None,
        }
    }

    #[test]
    fn values_convert_to_target_types() {
        use LogicalType as L;
        let num1 = col("NUMBER(1)", L::Decimal { precision: Some(1), scale: Some(0) });
        assert_eq!(to_target(&Value::Bool(true), &num1).unwrap(), Value::Decimal("1".into()));
        assert_eq!(
            to_target(&Value::Decimal("0".into()), &col("boolean", L::Bool)).unwrap(),
            Value::Bool(false)
        );
        assert_eq!(
            to_target(&Value::Decimal("42".into()), &col("bigint", L::Int { bits: 64 })).unwrap(),
            Value::Int(42)
        );
        let text = col("text", L::Text);
        let u = uuid::Uuid::nil();
        assert_eq!(to_target(&Value::Uuid(u), &text).unwrap(), Value::String(u.to_string()));
        assert_eq!(
            to_target(&Value::Json(serde_json::json!({"a": 1})), &text).unwrap(),
            Value::String("{\"a\":1}".into())
        );
        let ts = col("datetime", L::Timestamp { tz: false, precision: Some(0) });
        let tz = FixedOffset::east_opt(8 * 3600).unwrap().with_ymd_and_hms(2026, 1, 2, 3, 4, 5).unwrap();
        assert_eq!(
            to_target(&Value::DateTimeTz(tz), &ts).unwrap(),
            Value::DateTime(NaiveDate::from_ymd_opt(2026, 1, 2).unwrap().and_hms_opt(3, 4, 5).unwrap())
        );
        assert_eq!(
            to_target(&Value::String("ab".into()), &col("bytea", L::Blob)).unwrap(),
            Value::Bytes(b"ab".to_vec())
        );
        assert_eq!(to_target(&Value::Null, &ts).unwrap(), Value::Null);
        let bad = to_target(&Value::String("x".into()), &col("int", L::Int { bits: 32 }));
        assert!(bad.is_err());
        let lob = |truncated| {
            Value::Lob(dblens_core::LobPreview {
                binary: false,
                preview: "x".into(),
                total_len: 9,
                truncated,
            })
        };
        assert!(to_target(&lob(true), &text).is_err());
        assert_eq!(to_target(&lob(false), &text).unwrap(), Value::String("x".into()));
    }

    #[test]
    fn normalize_for_comparison() {
        use LogicalType as L;
        let n = |v: Value, c: &Column| normalize(&v, c, false);
        let dec = col("numeric(12,2)", L::Decimal { precision: Some(12), scale: Some(2) });
        assert_eq!(n(Value::Decimal("1.50".into()), &dec), n(Value::Decimal("1.5".into()), &dec));
        assert_eq!(n(Value::Decimal("2.00".into()), &dec), "2");
        let ch = col("char(5)", L::Char { len: Some(5) });
        assert_eq!(n(Value::String("ab   ".into()), &ch), "ab");
        let ts = col("datetime(0)", L::Timestamp { tz: false, precision: Some(0) });
        let t = NaiveDate::from_ymd_opt(2026, 1, 2).unwrap().and_hms_micro_opt(3, 4, 5, 678).unwrap();
        assert_eq!(n(Value::DateTime(t), &ts), "2026-01-02 03:04:05");
        let b = col("NUMBER(1)", L::Decimal { precision: Some(1), scale: Some(0) });
        assert_eq!(n(Value::Bool(true), &b), n(Value::Decimal("1".into()), &b));
        // Oracle 的空串即 NULL
        let raw = col("RAW(16)", L::VarBinary { len: Some(16) });
        assert_eq!(normalize(&Value::Bytes(vec![]), &raw, true), normalize(&Value::Null, &raw, true));
        assert_ne!(n(Value::String(String::new()), &ch), n(Value::Null, &ch));
        assert_eq!(key_text(&[Value::Int(1), Value::String("a".into())]), "1, a");
    }
}
