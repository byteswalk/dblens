//! Oracle 列信息、值映射与参数绑定。

use std::io::Read;

use chrono::{DateTime, FixedOffset, NaiveDateTime};
use dblens_core::{DEFAULT_LOB_PREVIEW_BYTES, Error, Result, Value};
use dblens_driver::ColumnMeta;
use oracle::sql_type::{Blob, Clob, Lob, Nclob, OracleType, ToSql};
use oracle::{ColumnInfo, SqlValue};

use crate::error::map_err;

/// ODPI-C 返回的数字文本可能省略整数部分的 0（如 ".5"），补齐以便阅读和比较。
pub(crate) fn normalize_number(s: &str) -> String {
    if let Some(rest) = s.strip_prefix("-.") {
        format!("-0.{rest}")
    } else if let Some(rest) = s.strip_prefix('.') {
        format!("0.{rest}")
    } else {
        s.to_string()
    }
}

pub(crate) fn column_meta(info: &ColumnInfo) -> ColumnMeta {
    let (precision, scale) = match info.oracle_type() {
        OracleType::Number(p, s) => ((*p > 0).then_some(u32::from(*p)), Some(i32::from(*s))),
        _ => (None, None),
    };
    ColumnMeta {
        name: info.name().to_string(),
        type_name: info.oracle_type().to_string(),
        nullable: Some(info.nullable()),
        precision,
        scale,
        source_table: None,
        source_column: None,
    }
}

/// `complete` 为 false 时，`bytes` 超过了预览长度，cap_large 一定会把它转成预览。
fn lob_preview(binary: bool, bytes: Vec<u8>, total: u64, complete: bool) -> Value {
    let value = if binary {
        Value::Bytes(bytes)
    } else {
        Value::String(String::from_utf8_lossy(&bytes).into_owned())
    };
    if complete {
        return value;
    }
    match value.cap_large(DEFAULT_LOB_PREVIEW_BYTES) {
        Value::Lob(mut lob) => {
            lob.total_len = total;
            Value::Lob(lob)
        }
        other => other,
    }
}

/// 每次读取的缓冲区大小。必须明显大于 oracle crate 读取字符 LOB 的最小缓冲区（400 字节），
/// 否则它会因为放不下一个完整字符而报错；也不能用 `Take` 把最后一次读取的缓冲区压得很小。
const LOB_READ_CHUNK: usize = 16 * 1024;

/// 读取 LOB 的前 N 字节作为预览。CLOB 的 size() 单位是字符（UTF-16 单元），BLOB 是字节。
fn read_lob(mut lob: impl Read + Lob, binary: bool) -> Result<Value> {
    let total = lob.size().map_err(map_err)?;
    let mut bytes = Vec::new();
    let mut chunk = vec![0u8; LOB_READ_CHUNK];
    // 读到超过预览长度或读完为止；每次都是整块读取，字符 LOB 由 crate 保证只返回完整字符
    while bytes.len() <= DEFAULT_LOB_PREVIEW_BYTES {
        let n = lob.read(&mut chunk).map_err(|e| Error::internal(format!("读取 LOB 失败：{e}")))?;
        if n == 0 {
            break;
        }
        bytes.extend_from_slice(&chunk[..n]);
    }
    // 完整性：二进制按字节数比较；字符 LOB 按 UTF-16 单元数比较（与 size() 同单位），
    // 不能用字节数判断，因为中文等多字节字符的字节数与字符数不同
    let read_units = if binary {
        bytes.len() as u64
    } else {
        String::from_utf8_lossy(&bytes).encode_utf16().count() as u64
    };
    let complete = read_units == total && bytes.len() <= DEFAULT_LOB_PREVIEW_BYTES;
    Ok(lob_preview(binary, bytes, total, complete))
}

pub(crate) fn oracle_value(v: &SqlValue) -> Result<Value> {
    if v.is_null().map_err(map_err)? {
        return Ok(Value::Null);
    }
    let ty = v.oracle_type().map_err(map_err)?.clone();
    let value = match ty {
        OracleType::Varchar2(_)
        | OracleType::NVarchar2(_)
        | OracleType::Char(_)
        | OracleType::NChar(_)
        | OracleType::Long
        | OracleType::Rowid => Value::String(v.get::<String>().map_err(map_err)?),
        OracleType::Number(_, _) | OracleType::Float(_) => {
            Value::Decimal(normalize_number(&v.get::<String>().map_err(map_err)?))
        }
        OracleType::Int64 => Value::Int(v.get().map_err(map_err)?),
        OracleType::UInt64 => Value::UInt(v.get().map_err(map_err)?),
        OracleType::BinaryFloat | OracleType::BinaryDouble => Value::Float(v.get().map_err(map_err)?),
        OracleType::Date | OracleType::Timestamp(_) => {
            Value::DateTime(v.get::<NaiveDateTime>().map_err(map_err)?)
        }
        OracleType::TimestampTZ(_) | OracleType::TimestampLTZ(_) => {
            Value::DateTimeTz(v.get::<DateTime<FixedOffset>>().map_err(map_err)?)
        }
        OracleType::IntervalDS(_, _) | OracleType::IntervalYM(_) => Value::Interval(v.to_string()),
        OracleType::Raw(_) | OracleType::LongRaw => Value::Bytes(v.get::<Vec<u8>>().map_err(map_err)?),
        OracleType::CLOB => read_lob(v.get::<Clob>().map_err(map_err)?, false)?,
        // NCLOB 在 oracle crate 中是独立类型，v.get::<Clob>() 会报类型转换错误
        OracleType::NCLOB => read_lob(v.get::<Nclob>().map_err(map_err)?, false)?,
        OracleType::BLOB => read_lob(v.get::<Blob>().map_err(map_err)?, true)?,
        OracleType::Boolean => Value::Bool(v.get().map_err(map_err)?),
        other => Value::Other { type_name: other.to_string(), text: v.to_string() },
    };
    Ok(value)
}

/// 把参数转换为可绑定的值。
pub(crate) fn to_binds(params: &[Value]) -> Result<Vec<Box<dyn ToSql>>> {
    params
        .iter()
        .map(|v| -> Result<Box<dyn ToSql>> {
            Ok(match v {
                Value::Null => Box::new(None::<String>),
                Value::Bool(b) => Box::new(*b),
                Value::Int(i) => Box::new(*i),
                Value::UInt(u) => Box::new(*u),
                Value::Float(f) => Box::new(*f),
                Value::Decimal(s) | Value::String(s) | Value::Interval(s) => Box::new(s.clone()),
                Value::Bytes(b) => Box::new(b.clone()),
                Value::Date(d) => Box::new(*d),
                Value::Time(t) => Box::new(t.to_string()),
                Value::DateTime(dt) => Box::new(*dt),
                Value::DateTimeTz(dt) => Box::new(*dt),
                Value::Json(j) => Box::new(j.to_string()),
                Value::Uuid(u) => Box::new(u.to_string()),
                Value::Lob(_) | Value::Other { .. } => {
                    return Err(Error::internal(format!("该类型的值不能作为参数绑定：{v:?}")));
                }
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_leading_dot_numbers() {
        assert_eq!(normalize_number(".5"), "0.5");
        assert_eq!(normalize_number("-.25"), "-0.25");
        assert_eq!(normalize_number("42"), "42");
        assert_eq!(normalize_number("-3.1"), "-3.1");
    }

    #[test]
    fn rejects_unbindable_values() {
        assert!(to_binds(&[Value::Int(1), Value::String("x".into()), Value::Null]).is_ok());
        assert!(to_binds(&[Value::Other { type_name: "x".into(), text: "y".into() }]).is_err());
    }
}
