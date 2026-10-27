//! 导入的值（M2b 设计文档 4.5、4.6 节）：按文件内容推断新建表的列类型；按目标列的类型转换文件中的值。
//! 文本格式与表数据编辑（M2a 前端 `parse.ts`）一致。

use chrono::{DateTime, FixedOffset, Local, NaiveDate, NaiveDateTime, NaiveTime, TimeZone};
use dblens_core::schema::Column;
use dblens_core::{Error, LogicalType, Result, Value, msg};

use super::text;

/// 推断时最多看的行数。
pub const INFER_ROWS: usize = 1000;

/// 布尔值的中文写法"是""否"：是可以输入的文字，不是给用户看的，用转义写（源码检查不把它当作界面文字）。
const YES: &str = "\u{662f}";
const NO: &str = "\u{5426}";

fn parse_date(s: &str) -> Option<NaiveDate> {
    ["%Y-%m-%d", "%Y/%m/%d"].iter().find_map(|f| NaiveDate::parse_from_str(s, f).ok())
}

fn parse_time(s: &str) -> Option<NaiveTime> {
    ["%H:%M:%S%.f", "%H:%M"].iter().find_map(|f| NaiveTime::parse_from_str(s, f).ok())
}

/// 日期加空格或 `T` 加时间。
fn parse_datetime(s: &str) -> Option<NaiveDateTime> {
    let (d, t) = s.split_once([' ', 'T'])?;
    Some(parse_date(d)?.and_time(parse_time(t.trim())?))
}

/// 带时区的日期时间：末尾可以有 `Z`、`±HH:MM`、`±HHMM` 或 `±HH`；没有时按本地时区。
fn parse_datetime_tz(s: &str) -> Option<DateTime<FixedOffset>> {
    let (body, offset) = match s.strip_suffix('Z') {
        Some(body) => (body, Some(0)),
        None => split_offset(s),
    };
    let naive = parse_datetime(body.trim_end())
        .or_else(|| parse_date(body.trim_end()).map(|d| d.and_time(NaiveTime::MIN)))?;
    match offset {
        Some(secs) => FixedOffset::east_opt(secs)?.from_local_datetime(&naive).single(),
        None => Local.from_local_datetime(&naive).earliest().map(|t| t.fixed_offset()),
    }
}

/// 拆出末尾的时区偏移（秒）。时区必须跟在时间后面，日期中的 `-` 不算。
fn split_offset(s: &str) -> (&str, Option<i32>) {
    let Some(k) = s.rfind(['+', '-']) else { return (s, None) };
    let (body, off) = s.split_at(k);
    let digits: String = off[1..].chars().filter(|c| *c != ':').collect();
    if !body.contains(':') || !matches!(digits.len(), 2 | 4) || !digits.bytes().all(|c| c.is_ascii_digit()) {
        return (s, None);
    }
    let h: i32 = digits[..2].parse().unwrap_or(0);
    let m: i32 = digits.get(2..).map_or(0, |m| m.parse().unwrap_or(0));
    let sign = if off.starts_with('-') { -1 } else { 1 };
    (body, Some(sign * (h * 3600 + m * 60)))
}

/// 精确小数的文本（与表数据编辑相同：可带符号、小数点和指数）。
fn is_decimal(s: &str) -> bool {
    let s = s.strip_prefix(['+', '-']).unwrap_or(s);
    let (mantissa, exp) = match s.split_once(['e', 'E']) {
        Some((m, e)) => (m, Some(e)),
        None => (s, None),
    };
    let (int, frac) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    let digits = |t: &str| t.bytes().all(|c| c.is_ascii_digit());
    let mantissa_ok = (!int.is_empty() || !frac.is_empty()) && digits(int) && digits(frac);
    let exp_ok = exp.is_none_or(|e| {
        let e = e.strip_prefix(['+', '-']).unwrap_or(e);
        !e.is_empty() && digits(e)
    });
    mantissa_ok && exp_ok
}

/// 推断用：一个值能归入哪些类型。
struct Fits {
    bool: bool,
    int: bool,
    /// 精确小数（无指数）：整数部分位数、小数位数。
    decimal: Option<(u32, u32)>,
    date: bool,
    datetime: bool,
}

fn fits(v: &Value) -> Fits {
    let none = Fits { bool: false, int: false, decimal: None, date: false, datetime: false };
    let plain_decimal = |s: &str| -> Option<(u32, u32)> {
        let s = s.strip_prefix(['+', '-']).unwrap_or(s);
        let (int, frac) = s.split_once('.').unwrap_or((s, ""));
        let ok = !int.is_empty()
            && int.bytes().all(|c| c.is_ascii_digit())
            && frac.bytes().all(|c| c.is_ascii_digit());
        let int = int.trim_start_matches('0');
        ok.then_some((int.len().max(1) as u32, frac.len() as u32))
    };
    match v {
        Value::Bool(_) => Fits { bool: true, ..none },
        Value::Int(n) => Fits { int: true, decimal: plain_decimal(&n.to_string()), ..none },
        Value::Float(f) => {
            let s = f.to_string();
            Fits { int: s.parse::<i64>().is_ok(), decimal: plain_decimal(&s), ..none }
        }
        Value::Date(_) => Fits { date: true, datetime: true, ..none },
        Value::DateTime(_) => Fits { datetime: true, ..none },
        Value::String(s) => {
            let s = s.trim();
            let date = parse_date(s).is_some();
            Fits {
                bool: s.eq_ignore_ascii_case("true") || s.eq_ignore_ascii_case("false"),
                int: s.parse::<i64>().is_ok(),
                decimal: plain_decimal(s),
                date,
                datetime: date || parse_datetime(s).is_some(),
            }
        }
        _ => none,
    }
}

/// 按一列的值推断类型（空值不参与）：布尔 → 64 位整数 → 精确小数 → 日期 → 日期时间 → 字符串。
/// 不推断浮点；全空的列为 VarChar(50)。
pub fn infer_type<'a>(values: impl IntoIterator<Item = &'a Value>) -> LogicalType {
    let (mut bool, mut int, mut decimal, mut date, mut datetime) =
        (true, true, Some((0u32, 0u32)), true, true);
    let mut max_len = 0usize;
    let mut any = false;
    for v in values {
        if matches!(v, Value::Null) {
            continue;
        }
        any = true;
        let f = fits(v);
        bool &= f.bool;
        int &= f.int;
        decimal = match (decimal, f.decimal) {
            (Some((i, s)), Some((fi, fs))) => Some((i.max(fi), s.max(fs))),
            _ => None,
        };
        date &= f.date;
        datetime &= f.datetime;
        max_len = max_len.max(text(v).chars().count());
    }
    if !any {
        return LogicalType::VarChar { len: Some(50) };
    }
    if bool {
        return LogicalType::Bool;
    }
    if int {
        return LogicalType::Int { bits: 64 };
    }
    if let Some((i, s)) = decimal
        && i + s <= 38
    {
        return LogicalType::Decimal { precision: Some((i + s).max(10)), scale: Some(s as i32) };
    }
    if date {
        return LogicalType::Date;
    }
    if datetime {
        return LogicalType::Timestamp { tz: false, precision: None };
    }
    match (max_len * 2).max(50) {
        n if n > 4000 => LogicalType::Text,
        n => LogicalType::VarChar { len: Some(n as u32) },
    }
}

/// 空单元格的写法。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmptyAs {
    Null,
    /// 文本列写空字符串（其他类型仍为 NULL）。
    EmptyString,
}

fn is_text(t: &LogicalType) -> bool {
    matches!(t, LogicalType::Char { .. } | LogicalType::VarChar { .. } | LogicalType::Text)
}

/// 把文件中的值转换成目标列的类型。失败时的错误包含列名和原值。
pub fn convert(v: &Value, col: &Column, empty: EmptyAs) -> Result<Value> {
    let logical = &col.data_type.logical;
    if matches!(v, Value::Null) {
        return Ok(if empty == EmptyAs::EmptyString && is_text(logical) {
            Value::String(String::new())
        } else {
            Value::Null
        });
    }
    let raw = text(v);
    let s = raw.trim();
    let bad = || {
        let shown: String = raw.chars().take(100).collect();
        Error::invalid(msg!(
            "import.bad_value",
            column = col.name,
            value = shown,
            type_name = col.data_type.native
        ))
    };
    let out = match logical {
        LogicalType::Bool => match v {
            Value::Bool(b) => Value::Bool(*b),
            _ => match s.to_lowercase().as_str() {
                "true" | "1" | YES => Value::Bool(true),
                "false" | "0" | NO => Value::Bool(false),
                _ => return Err(bad()),
            },
        },
        LogicalType::Int { bits } | LogicalType::UInt { bits } => {
            let signed = matches!(logical, LogicalType::Int { .. });
            let n: i128 = match v {
                Value::Int(n) => *n as i128,
                Value::Float(f) if f.fract() == 0.0 && f.abs() < 1e38 => *f as i128,
                _ => s.strip_prefix('+').unwrap_or(s).parse().map_err(|_| bad())?,
            };
            let bits = (*bits).clamp(1, 64) as u32;
            let (min, max) = if signed {
                (-(1i128 << (bits - 1)), (1i128 << (bits - 1)) - 1)
            } else {
                (0, (1i128 << bits) - 1)
            };
            if n < min || n > max {
                return Err(bad());
            }
            // 超出 i64 的无符号整数按精确小数传
            i64::try_from(n).map_or_else(|_| Value::Decimal(n.to_string()), Value::Int)
        }
        LogicalType::Float { .. } => match v {
            Value::Float(f) => Value::Float(*f),
            Value::Int(n) => Value::Float(*n as f64),
            _ => Value::Float(s.parse().ok().filter(|f: &f64| f.is_finite()).ok_or_else(bad)?),
        },
        LogicalType::Decimal { .. } => {
            if !is_decimal(s) {
                return Err(bad());
            }
            Value::Decimal(s.to_string())
        }
        LogicalType::Date => match v {
            Value::Date(d) => Value::Date(*d),
            Value::DateTime(t) => Value::Date(t.date()),
            _ => Value::Date(parse_date(s).or_else(|| parse_datetime(s).map(|t| t.date())).ok_or_else(bad)?),
        },
        LogicalType::Time { .. } => match v {
            Value::DateTime(t) => Value::Time(t.time()),
            _ => Value::Time(parse_time(s).ok_or_else(bad)?),
        },
        LogicalType::Timestamp { tz: false, .. } => match v {
            Value::Date(d) => Value::DateTime(d.and_time(NaiveTime::MIN)),
            Value::DateTime(t) => Value::DateTime(*t),
            _ => Value::DateTime(
                parse_datetime(s)
                    .or_else(|| parse_date(s).map(|d| d.and_time(NaiveTime::MIN)))
                    .ok_or_else(bad)?,
            ),
        },
        LogicalType::Timestamp { tz: true, .. } => {
            let naive = match v {
                Value::Date(d) => Some(d.and_time(NaiveTime::MIN)),
                Value::DateTime(t) => Some(*t),
                _ => None,
            };
            let t = match naive {
                Some(n) => Local.from_local_datetime(&n).earliest().map(|t| t.fixed_offset()),
                None => parse_datetime_tz(s),
            };
            Value::DateTimeTz(t.ok_or_else(bad)?)
        }
        LogicalType::Binary { .. } | LogicalType::VarBinary { .. } | LogicalType::Blob => {
            let hex = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")).ok_or_else(bad)?;
            Value::Bytes(decode_hex(hex).ok_or_else(bad)?)
        }
        LogicalType::Json => Value::Json(serde_json::from_str(&raw).map_err(|_| bad())?),
        LogicalType::Uuid => Value::Uuid(uuid::Uuid::parse_str(s).map_err(|_| bad())?),
        // 文本、时间间隔和不认识的类型按文本传（PG 上由语句转换成列的类型）
        _ => Value::String(raw.clone()),
    };
    Ok(out)
}

fn decode_hex(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    (0..s.len()).step_by(2).map(|k| u8::from_str_radix(s.get(k..k + 2)?, 16).ok()).collect()
}
