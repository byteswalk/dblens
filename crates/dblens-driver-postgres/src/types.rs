use bytes::BytesMut;
use chrono::{DateTime, NaiveDate, NaiveDateTime, NaiveTime, Utc};
use dblens_core::{Error, Msg, Result, Value, msg};
use tokio_postgres::Row;
use tokio_postgres::types::{FromSql, IsNull, ToSql, Type, to_sql_checked};

type BoxError = Box<dyn std::error::Error + Sync + Send>;

/// 解码 PG numeric 的二进制格式，得到精确的十进制文本。
/// 格式：ndigits(i16) weight(i16) sign(u16) dscale(u16)，后跟 ndigits 个以 10000 为基的 i16。
pub(crate) fn decode_numeric(raw: &[u8]) -> std::result::Result<String, BoxError> {
    if raw.len() < 8 {
        return Err(Error::internal(msg!("pg.bad_numeric")).into());
    }
    let i16_at = |i: usize| i16::from_be_bytes([raw[i], raw[i + 1]]);
    let ndigits = usize::try_from(i16_at(0))?;
    let weight = i32::from(i16_at(2));
    let sign = u16::from_be_bytes([raw[4], raw[5]]);
    let dscale = usize::from(u16::from_be_bytes([raw[6], raw[7]]));
    match sign {
        0xC000 => return Ok("NaN".into()),
        0xD000 => return Ok("Infinity".into()),
        0xF000 => return Ok("-Infinity".into()),
        _ => {}
    }
    if raw.len() < 8 + ndigits * 2 {
        return Err(Error::internal(msg!("pg.bad_numeric")).into());
    }
    let digits: Vec<i16> = (0..ndigits).map(|i| i16_at(8 + i * 2)).collect();
    let digit = |idx: i32| if idx >= 0 { digits.get(idx as usize).copied().unwrap_or(0) } else { 0 };

    let mut int_part = String::new();
    if weight >= 0 {
        for i in 0..=weight {
            let d = digit(i);
            if i == 0 {
                int_part.push_str(&d.to_string());
            } else {
                int_part.push_str(&format!("{d:04}"));
            }
        }
    } else {
        int_part.push('0');
    }

    let mut frac = String::new();
    let mut k = 1;
    while frac.len() < dscale {
        frac.push_str(&format!("{:04}", digit(weight + k)));
        k += 1;
    }
    frac.truncate(dscale);

    let mut s = String::new();
    if sign == 0x4000 {
        s.push('-');
    }
    s.push_str(&int_part);
    if dscale > 0 {
        s.push('.');
        s.push_str(&frac);
    }
    Ok(s)
}

/// 把 PG interval（月、天、微秒）格式化为文本。
pub(crate) fn format_interval(months: i32, days: i32, micros: i64) -> String {
    let sign = if micros < 0 { "-" } else { "" };
    let us = micros.unsigned_abs();
    let (h, rem) = (us / 3_600_000_000, us % 3_600_000_000);
    let (m, rem) = (rem / 60_000_000, rem % 60_000_000);
    let (s, frac) = (rem / 1_000_000, rem % 1_000_000);
    let time = if frac == 0 {
        format!("{sign}{h:02}:{m:02}:{s:02}")
    } else {
        format!("{sign}{h:02}:{m:02}:{s:02}.{frac:06}")
    };
    format!("{months} mons {days} days {time}")
}

pub(crate) struct PgNumeric(pub String);

impl<'a> FromSql<'a> for PgNumeric {
    fn from_sql(_: &Type, raw: &'a [u8]) -> std::result::Result<Self, BoxError> {
        decode_numeric(raw).map(PgNumeric)
    }
    fn accepts(ty: &Type) -> bool {
        *ty == Type::NUMERIC
    }
}

pub(crate) struct PgInterval(pub String);

impl<'a> FromSql<'a> for PgInterval {
    fn from_sql(_: &Type, raw: &'a [u8]) -> std::result::Result<Self, BoxError> {
        if raw.len() != 16 {
            return Err(Error::internal(msg!("pg.bad_interval")).into());
        }
        let micros = i64::from_be_bytes(raw[0..8].try_into()?);
        let days = i32::from_be_bytes(raw[8..12].try_into()?);
        let months = i32::from_be_bytes(raw[12..16].try_into()?);
        Ok(PgInterval(format_interval(months, days, micros)))
    }
    fn accepts(ty: &Type) -> bool {
        *ty == Type::INTERVAL
    }
}

/// 不认识的类型：原样取出二进制。
pub(crate) struct RawValue<'a>(pub &'a [u8]);

impl<'a> FromSql<'a> for RawValue<'a> {
    fn from_sql(_: &Type, raw: &'a [u8]) -> std::result::Result<Self, BoxError> {
        Ok(RawValue(raw))
    }
    fn accepts(_: &Type) -> bool {
        true
    }
}

/// 读取一行中第 `idx` 列的值。
pub(crate) fn pg_value(row: &Row, idx: usize) -> Result<Value> {
    let ty = row.columns()[idx].type_().clone();
    let fail = |e: tokio_postgres::Error| {
        let cause = crate::error::ours(&e).map_or_else(|| Msg::raw(e.to_string()), |inner| inner.to_msg());
        Error::internal(msg!("pg.read_column_failed", column = idx + 1, ty = ty.name(), error = cause))
    };
    macro_rules! get {
        ($t:ty, $f:expr) => {
            row.try_get::<_, Option<$t>>(idx).map_err(fail)?.map($f)
        };
    }
    let value = match ty {
        Type::BOOL => get!(bool, Value::Bool),
        Type::INT2 => get!(i16, |v| Value::Int(v.into())),
        Type::INT4 => get!(i32, |v| Value::Int(v.into())),
        Type::INT8 => get!(i64, Value::Int),
        Type::OID => get!(u32, |v| Value::UInt(v.into())),
        Type::FLOAT4 => get!(f32, |v| Value::Float(v.into())),
        Type::FLOAT8 => get!(f64, Value::Float),
        Type::NUMERIC => get!(PgNumeric, |v| Value::Decimal(v.0)),
        Type::TEXT | Type::VARCHAR | Type::BPCHAR | Type::NAME | Type::UNKNOWN => get!(String, Value::String),
        Type::BYTEA => get!(Vec<u8>, Value::Bytes),
        Type::DATE => get!(NaiveDate, Value::Date),
        Type::TIME => get!(NaiveTime, Value::Time),
        Type::TIMESTAMP => get!(NaiveDateTime, Value::DateTime),
        Type::TIMESTAMPTZ => get!(DateTime<Utc>, |v| Value::DateTimeTz(v.fixed_offset())),
        Type::JSON | Type::JSONB => get!(serde_json::Value, Value::Json),
        Type::UUID => get!(uuid::Uuid, Value::Uuid),
        Type::INTERVAL => get!(PgInterval, |v| Value::Interval(v.0)),
        _ => {
            let raw = row.try_get::<_, Option<RawValue>>(idx).map_err(fail)?;
            raw.map(|r| Value::Other {
                type_name: ty.name().to_string(),
                // 枚举、xml、citext 等类型的二进制格式就是文本
                text: match std::str::from_utf8(r.0) {
                    Ok(s) => s.to_string(),
                    Err(_) => format!("\\x{}", r.0.iter().map(|b| format!("{b:02x}")).collect::<String>()),
                },
            })
        }
    };
    Ok(value.unwrap_or(Value::Null))
}

/// 把 `Value` 绑定为 PG 参数，按目标参数类型做必要的转换。
#[derive(Debug)]
pub(crate) struct PgParam<'a>(pub &'a Value);

impl ToSql for PgParam<'_> {
    fn to_sql(&self, ty: &Type, out: &mut BytesMut) -> std::result::Result<IsNull, BoxError> {
        match self.0 {
            Value::Null => Ok(IsNull::Yes),
            Value::Bool(b) => b.to_sql(ty, out),
            Value::Int(i) => match *ty {
                Type::INT2 => i16::try_from(*i)?.to_sql(ty, out),
                Type::INT4 => i32::try_from(*i)?.to_sql(ty, out),
                Type::FLOAT4 => (*i as f32).to_sql(ty, out),
                Type::FLOAT8 => (*i as f64).to_sql(ty, out),
                Type::NUMERIC => Err(numeric_param_hint()),
                _ => i.to_sql(ty, out),
            },
            Value::UInt(u) => PgParam(&Value::Int(i64::try_from(*u)?)).to_sql(ty, out),
            Value::Float(f) => match *ty {
                Type::FLOAT4 => (*f as f32).to_sql(ty, out),
                Type::NUMERIC => Err(numeric_param_hint()),
                _ => f.to_sql(ty, out),
            },
            Value::Decimal(_) if *ty == Type::NUMERIC => Err(numeric_param_hint()),
            Value::String(s) | Value::Decimal(s) | Value::Interval(s) => s.as_str().to_sql(ty, out),
            Value::Bytes(b) => b.as_slice().to_sql(ty, out),
            Value::Date(d) => d.to_sql(ty, out),
            Value::Time(t) => t.to_sql(ty, out),
            Value::DateTime(dt) => dt.to_sql(ty, out),
            Value::DateTimeTz(dt) => dt.to_sql(ty, out),
            Value::Json(j) => j.to_sql(ty, out),
            Value::Uuid(u) => u.to_sql(ty, out),
            Value::Lob(_) | Value::Other { .. } => {
                Err(Error::internal(msg!("driver.unbindable_value", value = format!("{:?}", self.0))).into())
            }
        }
    }

    fn accepts(_: &Type) -> bool {
        true
    }

    to_sql_checked!();
}

fn numeric_param_hint() -> BoxError {
    Error::invalid(msg!("pg.numeric_param")).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 按 PG numeric 二进制格式拼字节：ndigits, weight, sign, dscale, digits...
    fn numeric(weight: i16, sign: u16, dscale: u16, digits: &[i16]) -> Vec<u8> {
        let mut v = Vec::new();
        v.extend_from_slice(&(digits.len() as i16).to_be_bytes());
        v.extend_from_slice(&weight.to_be_bytes());
        v.extend_from_slice(&sign.to_be_bytes());
        v.extend_from_slice(&dscale.to_be_bytes());
        for d in digits {
            v.extend_from_slice(&d.to_be_bytes());
        }
        v
    }

    #[test]
    fn decodes_numeric() {
        assert_eq!(decode_numeric(&numeric(0, 0, 4, &[123, 4500])).unwrap(), "123.4500");
        assert_eq!(decode_numeric(&numeric(-1, 0, 2, &[500])).unwrap(), "0.05");
        assert_eq!(decode_numeric(&numeric(1, 0, 0, &[1])).unwrap(), "10000");
        assert_eq!(decode_numeric(&numeric(0, 0x4000, 1, &[3, 1400])).unwrap(), "-3.1");
        assert_eq!(decode_numeric(&numeric(0, 0, 0, &[])).unwrap(), "0");
        assert_eq!(
            decode_numeric(&numeric(4, 0, 9, &[1234, 5678, 9012, 3456, 7890, 1234, 5678, 9000])).unwrap(),
            "12345678901234567890.123456789"
        );
        assert_eq!(decode_numeric(&numeric(0, 0xC000, 0, &[])).unwrap(), "NaN");
        assert!(decode_numeric(&[0, 1]).is_err());
    }

    #[test]
    fn formats_interval() {
        assert_eq!(format_interval(0, 1, 7_384_000_000), "0 mons 1 days 02:03:04");
        assert_eq!(format_interval(14, 0, 1_500_000), "14 mons 0 days 00:00:01.500000");
        assert_eq!(format_interval(0, 0, -3_600_000_000), "0 mons 0 days -01:00:00");
    }
}
