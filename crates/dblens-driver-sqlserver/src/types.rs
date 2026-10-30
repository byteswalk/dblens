//! TDS 值与 DBLens 值的互相转换（M6a 设计文档 3.5 节）。

use chrono::{DateTime, FixedOffset, NaiveDate, NaiveDateTime, NaiveTime};
use dblens_core::{Error, Result, Value, msg};
use tiberius::numeric::Numeric;
use tiberius::{ColumnData, ColumnType, FromSql, ToSql};

/// 结果列的类型名（TDS 只给出线上的类型，不含长度，也分不出 varchar 与 varchar(max) 等）。
pub(crate) fn type_name(t: ColumnType) -> &'static str {
    use ColumnType as T;
    match t {
        T::Null => "null",
        T::Bit | T::Bitn => "bit",
        T::Int1 => "tinyint",
        T::Int2 => "smallint",
        T::Int4 | T::Intn => "int",
        T::Int8 => "bigint",
        T::Float4 => "real",
        T::Float8 | T::Floatn => "float",
        T::Money => "money",
        T::Money4 => "smallmoney",
        T::Datetime4 => "smalldatetime",
        T::Datetime | T::Datetimen => "datetime",
        T::Daten => "date",
        T::Timen => "time",
        T::Datetime2 => "datetime2",
        T::DatetimeOffsetn => "datetimeoffset",
        T::Guid => "uniqueidentifier",
        T::Decimaln | T::Numericn => "decimal",
        T::BigVarBin => "varbinary",
        T::BigBinary => "binary",
        T::BigVarChar => "varchar",
        T::BigChar => "char",
        T::NVarchar => "nvarchar",
        T::NChar => "nchar",
        T::Xml => "xml",
        T::Udt => "udt",
        T::Text => "text",
        T::Image => "image",
        T::NText => "ntext",
        T::SSVariant => "sql_variant",
    }
}

/// 精确小数的文本：整数部分和补足小数位的小数部分（tiberius 自己的格式在小数位为 0 时会多出 `.0`）。
pub(crate) fn numeric_text(n: Numeric) -> String {
    let scale = usize::from(n.scale());
    let digits = n.value().unsigned_abs().to_string();
    let sign = if n.value() < 0 { "-" } else { "" };
    if scale == 0 {
        return format!("{sign}{digits}");
    }
    let padded = format!("{digits:0>width$}", width = scale + 1);
    let (int, frac) = padded.split_at(padded.len() - scale);
    format!("{sign}{int}.{frac}")
}

fn chrono_value<'a, T: FromSql<'a>>(data: &'a ColumnData<'static>, wrap: fn(T) -> Value) -> Result<Value> {
    T::from_sql(data).map(|v| v.map_or(Value::Null, wrap)).map_err(|e| Error::internal(e.to_string()))
}

/// 一个单元格的值。`ty` 用来区分 money（线上是浮点）；大字段按 `lob_limit` 截断为预览。
pub(crate) fn cell_value(data: &ColumnData<'static>, ty: ColumnType, lob_limit: usize) -> Result<Value> {
    use ColumnData as D;
    Ok(match data {
        D::U8(v) => v.map_or(Value::Null, |x| Value::Int(i64::from(x))),
        D::I16(v) => v.map_or(Value::Null, |x| Value::Int(i64::from(x))),
        D::I32(v) => v.map_or(Value::Null, |x| Value::Int(i64::from(x))),
        D::I64(v) => v.map_or(Value::Null, Value::Int),
        D::F32(v) => v.map_or(Value::Null, |x| Value::Float(f64::from(x))),
        // money / smallmoney 在线上是按 4 位小数换算的浮点，按精确小数显示
        D::F64(v) if matches!(ty, ColumnType::Money | ColumnType::Money4) => {
            v.map_or(Value::Null, |x| Value::Decimal(format!("{x:.4}")))
        }
        D::F64(v) => v.map_or(Value::Null, Value::Float),
        D::Bit(v) => v.map_or(Value::Null, Value::Bool),
        D::String(v) => v.as_ref().map_or(Value::Null, |s| Value::String(s.to_string()).cap_large(lob_limit)),
        D::Guid(v) => v.map_or(Value::Null, Value::Uuid),
        D::Binary(v) => v.as_ref().map_or(Value::Null, |b| Value::Bytes(b.to_vec()).cap_large(lob_limit)),
        D::Numeric(v) => v.map_or(Value::Null, |n| Value::Decimal(numeric_text(n))),
        D::Xml(v) => v.as_ref().map_or(Value::Null, |x| Value::String(x.to_string()).cap_large(lob_limit)),
        D::Date(_) => chrono_value::<NaiveDate>(data, Value::Date)?,
        D::Time(_) => chrono_value::<NaiveTime>(data, Value::Time)?,
        D::DateTime(_) | D::SmallDateTime(_) | D::DateTime2(_) => {
            chrono_value::<NaiveDateTime>(data, Value::DateTime)?
        }
        D::DateTimeOffset(_) => chrono_value::<DateTime<FixedOffset>>(data, Value::DateTimeTz)?,
    })
}

/// 绑定参数。时间类型按 tiberius 的映射（date、time、datetime2、datetimeoffset）。
pub(crate) struct Param(pub(crate) Value);

impl Param {
    /// 检查值能否作为参数（大字段预览不能写回）。
    pub(crate) fn new(v: &Value) -> Result<Self> {
        match v {
            Value::Lob(_) => Err(Error::invalid(msg!("literal.lob"))),
            v => Ok(Self(v.clone())),
        }
    }
}

/// 精确小数文本 → TDS 的 Numeric；超出 38 位或不是数字时返回 None（改用字符串，由服务器转换）。
fn parse_numeric(s: &str) -> Option<Numeric> {
    let s = s.trim();
    let (neg, body) = match s.strip_prefix('-') {
        Some(b) => (true, b),
        None => (false, s.strip_prefix('+').unwrap_or(s)),
    };
    let (int, frac) = body.split_once('.').unwrap_or((body, ""));
    if int.is_empty() && frac.is_empty() || !(int.bytes().chain(frac.bytes())).all(|c| c.is_ascii_digit()) {
        return None;
    }
    if int.len() + frac.len() > 38 {
        return None;
    }
    let value: i128 = format!("{int}{frac}").parse().ok()?;
    Some(Numeric::new_with_scale(if neg { -value } else { value }, u8::try_from(frac.len()).ok()?))
}

impl ToSql for Param {
    fn to_sql(&self) -> ColumnData<'_> {
        match &self.0 {
            // 类型未知的 NULL 用 nvarchar，服务器可以隐式转换成任何类型
            Value::Null | Value::Lob(_) => ColumnData::String(None),
            Value::Bool(b) => ColumnData::Bit(Some(*b)),
            Value::Int(i) => ColumnData::I64(Some(*i)),
            Value::UInt(u) => match i64::try_from(*u) {
                Ok(i) => ColumnData::I64(Some(i)),
                Err(_) => ColumnData::Numeric(Some(Numeric::new_with_scale(i128::from(*u), 0))),
            },
            Value::Float(f) => ColumnData::F64(Some(*f)),
            Value::Decimal(s) => match parse_numeric(s) {
                Some(n) => ColumnData::Numeric(Some(n)),
                None => ColumnData::String(Some(s.as_str().into())),
            },
            Value::String(s) | Value::Interval(s) => ColumnData::String(Some(s.as_str().into())),
            Value::Json(j) => ColumnData::String(Some(j.to_string().into())),
            Value::Uuid(u) => ColumnData::Guid(Some(*u)),
            Value::Bytes(b) => ColumnData::Binary(Some(b.as_slice().into())),
            Value::Date(d) => d.to_sql(),
            Value::Time(t) => t.to_sql(),
            Value::DateTime(dt) => dt.to_sql(),
            Value::DateTimeTz(dt) => dt.to_sql(),
            Value::Other { text, .. } => ColumnData::String(Some(text.as_str().into())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numeric_text_pads_and_signs() {
        let n = |v: i128, s: u8| numeric_text(Numeric::new_with_scale(v, s));
        assert_eq!(n(12345, 2), "123.45");
        assert_eq!(n(-5, 3), "-0.005");
        assert_eq!(n(42, 0), "42");
        assert_eq!(n(0, 2), "0.00");
        assert_eq!(n(-100, 0), "-100");
    }

    #[test]
    fn decimal_params_become_numeric() {
        let n = parse_numeric("-123.4500").unwrap();
        assert_eq!((n.value(), n.scale()), (-1234500, 4));
        assert_eq!(parse_numeric(".5").map(|n| (n.value(), n.scale())), Some((5, 1)));
        assert!(parse_numeric("1e5").is_none());
        assert!(parse_numeric("NaN").is_none());
        assert!(parse_numeric(".").is_none());
        assert!(parse_numeric(&"9".repeat(39)).is_none());
        assert!(matches!(Param(Value::Decimal("1e5".into())).to_sql(), ColumnData::String(Some(_))));
        assert!(matches!(Param(Value::UInt(u64::MAX)).to_sql(), ColumnData::Numeric(Some(_))));
        assert!(
            Param::new(&Value::Lob(dblens_core::LobPreview {
                binary: false,
                preview: String::new(),
                total_len: 1,
                truncated: true
            }))
            .is_err()
        );
    }

    #[test]
    fn cells_convert_and_money_is_decimal() {
        assert_eq!(
            cell_value(&ColumnData::F64(Some(12.5)), ColumnType::Money, 10).unwrap(),
            Value::Decimal("12.5000".into())
        );
        assert_eq!(
            cell_value(&ColumnData::F64(Some(1.5)), ColumnType::Floatn, 10).unwrap(),
            Value::Float(1.5)
        );
        assert_eq!(cell_value(&ColumnData::I32(None), ColumnType::Intn, 10).unwrap(), Value::Null);
        assert_eq!(cell_value(&ColumnData::U8(Some(255)), ColumnType::Int1, 10).unwrap(), Value::Int(255));
        let long = ColumnData::String(Some("x".repeat(20).into()));
        assert!(matches!(cell_value(&long, ColumnType::NVarchar, 10).unwrap(), Value::Lob(_)));
        let d = NaiveDate::from_ymd_opt(2024, 2, 29).unwrap();
        let ColumnData::Date(date) = d.to_sql() else { panic!("date") };
        assert_eq!(cell_value(&ColumnData::Date(date), ColumnType::Daten, 10).unwrap(), Value::Date(d));
    }
}
