//! 达梦类型分类、类型名、文本到 Value 的转换，以及参数转换。
//! DPI 的 SQL 类型常量来自 ffi.rs，类型宽度不确定，统一按 i64 比较。

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use chrono::{DateTime, NaiveDate, NaiveDateTime, NaiveTime};
use dblens_core::{DEFAULT_LOB_PREVIEW_BYTES, Error, LobPreview, Result, Value, msg};

use crate::ffi::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DmType {
    Int,
    Dec,
    Float,
    Bool,
    Date,
    Time,
    TimeTz,
    Timestamp,
    TimestampTz,
    Interval,
    Char,
    Clob,
    Binary,
    Blob,
    Other,
}

impl DmType {
    pub(crate) fn is_binary(self) -> bool {
        matches!(self, DmType::Binary | DmType::Blob)
    }
}

macro_rules! any_of {
    ($t:expr; $($c:ident),+ $(,)?) => { [$($c as i64),+].contains(&($t as i64)) };
}

pub(crate) fn classify(t: i32) -> DmType {
    if any_of!(t; DSQL_TINYINT, DSQL_SMALLINT, DSQL_INT, DSQL_BIGINT) {
        DmType::Int
    } else if any_of!(t; DSQL_DEC) {
        DmType::Dec
    } else if any_of!(t; DSQL_FLOAT, DSQL_DOUBLE) {
        DmType::Float
    } else if any_of!(t; DSQL_BIT, DSQL_BOOLEAN) {
        DmType::Bool
    } else if any_of!(t; DSQL_DATE) {
        DmType::Date
    } else if any_of!(t; DSQL_TIME) {
        DmType::Time
    } else if any_of!(t; DSQL_TIME_TZ) {
        DmType::TimeTz
    } else if any_of!(t; DSQL_TIMESTAMP) {
        DmType::Timestamp
    } else if any_of!(t; DSQL_TIMESTAMP_TZ) {
        DmType::TimestampTz
    } else if any_of!(t; DSQL_INTERVAL_YEAR, DSQL_INTERVAL_MONTH, DSQL_INTERVAL_DAY, DSQL_INTERVAL_HOUR,
        DSQL_INTERVAL_MINUTE, DSQL_INTERVAL_SECOND, DSQL_INTERVAL_YEAR_TO_MONTH, DSQL_INTERVAL_DAY_TO_HOUR,
        DSQL_INTERVAL_DAY_TO_MINUTE, DSQL_INTERVAL_DAY_TO_SECOND, DSQL_INTERVAL_HOUR_TO_MINUTE,
        DSQL_INTERVAL_HOUR_TO_SECOND, DSQL_INTERVAL_MINUTE_TO_SECOND)
    {
        DmType::Interval
    } else if any_of!(t; DSQL_CHAR, DSQL_VARCHAR, DSQL_ROWID) {
        DmType::Char
    } else if any_of!(t; DSQL_CLOB) {
        DmType::Clob
    } else if any_of!(t; DSQL_BINARY, DSQL_VARBINARY) {
        DmType::Binary
    } else if any_of!(t; DSQL_BLOB) {
        DmType::Blob
    } else {
        DmType::Other
    }
}

macro_rules! name_of {
    ($t:expr; $($c:ident => $n:literal),+ $(,)?) => {{ $( if $t as i64 == $c as i64 { return $n.to_string(); } )+ }};
}

/// 原生类型名，用于结果集列信息。
pub(crate) fn type_name(t: i32) -> String {
    name_of!(t;
        DSQL_TINYINT => "TINYINT", DSQL_SMALLINT => "SMALLINT", DSQL_INT => "INT", DSQL_BIGINT => "BIGINT",
        DSQL_DEC => "DEC", DSQL_FLOAT => "FLOAT", DSQL_DOUBLE => "DOUBLE", DSQL_BIT => "BIT",
        DSQL_BOOLEAN => "BOOLEAN", DSQL_DATE => "DATE", DSQL_TIME => "TIME", DSQL_TIME_TZ => "TIME WITH TIME ZONE",
        DSQL_TIMESTAMP => "TIMESTAMP", DSQL_TIMESTAMP_TZ => "TIMESTAMP WITH TIME ZONE", DSQL_CHAR => "CHAR",
        DSQL_VARCHAR => "VARCHAR", DSQL_CLOB => "CLOB", DSQL_BINARY => "BINARY", DSQL_VARBINARY => "VARBINARY",
        DSQL_BLOB => "BLOB", DSQL_ROWID => "ROWID",
    );
    if classify(t) == DmType::Interval { "INTERVAL".to_string() } else { format!("TYPE({t})") }
}

fn kind_name(kind: DmType) -> &'static str {
    match kind {
        DmType::Int => "INT",
        DmType::Dec => "DEC",
        DmType::Float => "DOUBLE",
        DmType::Bool => "BIT",
        DmType::Date => "DATE",
        DmType::Time => "TIME",
        DmType::TimeTz => "TIME WITH TIME ZONE",
        DmType::Timestamp => "TIMESTAMP",
        DmType::TimestampTz => "TIMESTAMP WITH TIME ZONE",
        DmType::Interval => "INTERVAL",
        DmType::Char => "VARCHAR",
        DmType::Clob => "CLOB",
        DmType::Binary => "VARBINARY",
        DmType::Blob => "BLOB",
        DmType::Other => "UNKNOWN",
    }
}

fn normalize_number(s: &str) -> String {
    if let Some(rest) = s.strip_prefix("-.") {
        format!("-0.{rest}")
    } else if let Some(rest) = s.strip_prefix('.') {
        format!("0.{rest}")
    } else {
        s.to_string()
    }
}

/// 把 DPI 以文本形式（DSQL_C_NCHAR）返回的值按类型解析为 Value。解析失败时保留原文。
pub(crate) fn text_value(kind: DmType, text: String) -> Value {
    let other = |text: String| Value::Other { type_name: kind_name(kind).to_string(), text };
    match kind {
        DmType::Int => text.parse().map(Value::Int).unwrap_or_else(|_| other(text)),
        DmType::Dec => Value::Decimal(normalize_number(&text)),
        DmType::Float => text.parse().map(Value::Float).unwrap_or_else(|_| other(text)),
        DmType::Bool => Value::Bool(text == "1" || text.eq_ignore_ascii_case("true")),
        DmType::Date => {
            NaiveDate::parse_from_str(&text, "%Y-%m-%d").map(Value::Date).unwrap_or_else(|_| other(text))
        }
        DmType::Time => {
            NaiveTime::parse_from_str(&text, "%H:%M:%S%.f").map(Value::Time).unwrap_or_else(|_| other(text))
        }
        DmType::Timestamp => NaiveDateTime::parse_from_str(&text, "%Y-%m-%d %H:%M:%S%.f")
            .map(Value::DateTime)
            .unwrap_or_else(|_| other(text)),
        DmType::TimestampTz => DateTime::parse_from_str(&text, "%Y-%m-%d %H:%M:%S%.f %:z")
            .map(Value::DateTimeTz)
            .unwrap_or_else(|_| other(text)),
        DmType::Interval => Value::Interval(text),
        DmType::Char | DmType::Clob => Value::String(text),
        // Value 没有带时区的时间类型，保留原文（如 `13:45:30 -07:00`）
        DmType::TimeTz | DmType::Binary | DmType::Blob | DmType::Other => other(text),
    }
}

/// DM 以文本返回 DEC 时会去掉小数部分末尾的 0（DEC(10,4) 的 123.4500 返回 "123.45"），
/// 这里按列的小数位数补齐。`scale` 为 0（含未声明小数位数的 NUMBER）或文本不是普通小数写法时保持原文。
pub(crate) fn with_scale(value: Value, scale: i32) -> Value {
    let Value::Decimal(mut text) = value else { return value };
    let plain = text.chars().all(|c| c.is_ascii_digit() || c == '-' || c == '.');
    if let (Ok(scale), true) = (usize::try_from(scale), plain) {
        let frac = text.split_once('.').map_or(0, |(_, f)| f.len());
        if scale > frac {
            if frac == 0 && !text.contains('.') {
                text.push('.');
            }
            text.extend(std::iter::repeat_n('0', scale - frac));
        }
    }
    Value::Decimal(text)
}

/// 去掉末尾不完整的 UTF-8 字符（分块读取在 limit 处停下时，最后一块可能切在字符中间）。
fn trim_incomplete_utf8(mut bytes: Vec<u8>) -> Vec<u8> {
    if let Err(e) = std::str::from_utf8(&bytes)
        && e.error_len().is_none()
    {
        bytes.truncate(e.valid_up_to());
    }
    bytes
}

/// 把读到的字节转换为 Value。`total` 是数据库报告的总字节数（文本按客户端编码 UTF-8 计），用于大字段预览。
/// 读到的字节少于 `total` 时一定返回截断的 LOB 预览，不会把不完整的内容当作完整字符串返回。
pub(crate) fn bytes_value(kind: DmType, bytes: Vec<u8>, total: Option<u64>, limit: usize) -> Value {
    let read = bytes.len() as u64;
    let total = total.unwrap_or(read).max(read);
    let short = total > read;
    let value = if kind.is_binary() {
        Value::Bytes(bytes)
    } else {
        // 先拼接全部字节再解码；读到一半的结尾字符丢弃，由截断标记说明
        let bytes = if short { trim_incomplete_utf8(bytes) } else { bytes };
        text_value(kind, String::from_utf8_lossy(&bytes).into_owned())
    };
    match value.cap_large(limit) {
        Value::Lob(mut lob) => {
            lob.total_len = lob.total_len.max(total);
            Value::Lob(lob)
        }
        Value::String(preview) if short => {
            Value::Lob(LobPreview { binary: false, preview, total_len: total, truncated: true })
        }
        Value::Bytes(b) if short => Value::Lob(LobPreview {
            binary: true,
            preview: STANDARD.encode(&b),
            total_len: total,
            truncated: true,
        }),
        other => other,
    }
}

/// 默认的大字段预览长度，供 conn.rs 使用。
pub(crate) const PREVIEW: usize = DEFAULT_LOB_PREVIEW_BYTES;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ParamData {
    Null,
    Text(Vec<u8>),
    Binary(Vec<u8>),
}

/// 参数统一以文本或二进制绑定，由数据库做隐式转换。
pub(crate) fn param_data(v: &Value) -> Result<ParamData> {
    let text = |s: String| Ok(ParamData::Text(s.into_bytes()));
    match v {
        Value::Null => Ok(ParamData::Null),
        Value::Bool(b) => text(if *b { "1".into() } else { "0".into() }),
        Value::Int(i) => text(i.to_string()),
        Value::UInt(u) => text(u.to_string()),
        Value::Float(f) => text(f.to_string()),
        Value::Decimal(s) | Value::String(s) | Value::Interval(s) => text(s.clone()),
        Value::Bytes(b) => Ok(ParamData::Binary(b.clone())),
        Value::Date(d) => text(d.format("%Y-%m-%d").to_string()),
        Value::Time(t) => text(t.format("%H:%M:%S%.f").to_string()),
        Value::DateTime(dt) => text(dt.format("%Y-%m-%d %H:%M:%S%.f").to_string()),
        Value::DateTimeTz(dt) => text(dt.format("%Y-%m-%d %H:%M:%S%.f %:z").to_string()),
        Value::Json(j) => text(j.to_string()),
        Value::Uuid(u) => text(u.to_string()),
        Value::Lob(_) | Value::Other { .. } => {
            Err(Error::internal(msg!("driver.unbindable_value", value = format!("{v:?}"))))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{NaiveDate, NaiveTime};

    #[test]
    fn parses_text_by_type() {
        assert_eq!(text_value(DmType::Int, "-42".into()), Value::Int(-42));
        assert_eq!(text_value(DmType::Dec, "123.4500".into()), Value::Decimal("123.4500".into()));
        assert_eq!(text_value(DmType::Dec, ".5".into()), Value::Decimal("0.5".into()));
        assert_eq!(text_value(DmType::Dec, "-.5".into()), Value::Decimal("-0.5".into()));
        assert_eq!(text_value(DmType::Float, "1.5".into()), Value::Float(1.5));
        assert_eq!(text_value(DmType::Bool, "1".into()), Value::Bool(true));
        assert_eq!(
            text_value(DmType::Date, "2024-02-29".into()),
            Value::Date(NaiveDate::from_ymd_opt(2024, 2, 29).unwrap())
        );
        assert_eq!(
            text_value(DmType::Time, "13:45:30".into()),
            Value::Time(NaiveTime::from_hms_opt(13, 45, 30).unwrap())
        );
        assert_eq!(
            text_value(DmType::Timestamp, "2024-02-29 13:45:30.123456".into()),
            Value::DateTime(
                NaiveDate::from_ymd_opt(2024, 2, 29).unwrap().and_hms_micro_opt(13, 45, 30, 123456).unwrap()
            )
        );
        assert_eq!(text_value(DmType::Char, "数洞".into()), Value::String("数洞".into()));
        assert_eq!(
            text_value(DmType::Interval, "INTERVAL '1' DAY".into()),
            Value::Interval("INTERVAL '1' DAY".into())
        );
        // 解析失败时保留原文
        assert_eq!(
            text_value(DmType::Int, "abc".into()),
            Value::Other { type_name: "INT".into(), text: "abc".into() }
        );
    }

    #[test]
    fn decimal_is_padded_to_column_scale() {
        // DM 以文本返回 DEC 时去掉小数末尾的 0：DEC(10,4) 的 123.4500 返回 "123.45"
        let dec = |s: &str| Value::Decimal(s.into());
        assert_eq!(with_scale(dec("123.45"), 4), dec("123.4500"));
        assert_eq!(with_scale(dec("100"), 2), dec("100.00"));
        assert_eq!(with_scale(dec("-0.5"), 2), dec("-0.50"));
        assert_eq!(with_scale(dec("0"), 3), dec("0.000"));
        assert_eq!(with_scale(dec("1.2345"), 4), dec("1.2345"));
        // 未声明小数位数（NUMBER）时 DPI 报告 0：保持原文
        assert_eq!(with_scale(dec("1.5"), 0), dec("1.5"));
        assert_eq!(with_scale(dec("1E+10"), 2), dec("1E+10"));
        assert_eq!(with_scale(Value::Int(1), 2), Value::Int(1));
    }

    #[test]
    fn time_with_time_zone_keeps_text() {
        assert_eq!(
            text_value(DmType::TimeTz, "13:45:30 -07:00".into()),
            Value::Other { type_name: "TIME WITH TIME ZONE".into(), text: "13:45:30 -07:00".into() }
        );
        assert_eq!(classify(DSQL_TIME_TZ as i32), DmType::TimeTz);
    }

    #[test]
    fn param_data_conversion() {
        assert_eq!(param_data(&Value::Null).unwrap(), ParamData::Null);
        assert_eq!(param_data(&Value::Int(7)).unwrap(), ParamData::Text(b"7".to_vec()));
        assert_eq!(param_data(&Value::Bool(true)).unwrap(), ParamData::Text(b"1".to_vec()));
        assert_eq!(
            param_data(&Value::String("数".into())).unwrap(),
            ParamData::Text("数".as_bytes().to_vec())
        );
        assert_eq!(param_data(&Value::Bytes(vec![1, 2])).unwrap(), ParamData::Binary(vec![1, 2]));
        assert_eq!(
            param_data(&Value::DateTime(
                NaiveDate::from_ymd_opt(2024, 2, 29).unwrap().and_hms_opt(1, 2, 3).unwrap()
            ))
            .unwrap(),
            ParamData::Text(b"2024-02-29 01:02:03".to_vec())
        );
        assert!(param_data(&Value::Other { type_name: "x".into(), text: "y".into() }).is_err());
    }

    #[test]
    fn lob_values_keep_total_length() {
        let text = "a".repeat(70_000);
        match bytes_value(DmType::Clob, text.into_bytes(), Some(100_000), 65_536) {
            Value::Lob(lob) => {
                assert!(!lob.binary && lob.truncated);
                assert_eq!((lob.preview.len(), lob.total_len), (65_536, 100_000));
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(bytes_value(DmType::Binary, vec![1, 2, 3], Some(3), 65_536), Value::Bytes(vec![1, 2, 3]));
    }

    #[test]
    fn short_read_is_never_a_silent_string() {
        // 数据库报告的总长超过读到的字节数：即使读到的部分没有超过预览上限，也必须标记为截断
        match bytes_value(DmType::Clob, b"abc".to_vec(), Some(10), 65_536) {
            Value::Lob(lob) => {
                assert!(!lob.binary && lob.truncated);
                assert_eq!((lob.preview.as_str(), lob.total_len), ("abc", 10));
            }
            other => panic!("{other:?}"),
        }
        match bytes_value(DmType::Blob, vec![1, 2, 3], Some(10), 65_536) {
            Value::Lob(lob) => {
                assert!(lob.binary && lob.truncated);
                assert_eq!((lob.preview.as_str(), lob.total_len), ("AQID", 10));
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn chunk_split_utf8_is_cut_on_char_boundary() {
        // 读到的字节在“数”（3 字节）中间结束：预览退回到字符边界，不出现替换字符
        let mut bytes = "数".repeat(30_000).into_bytes();
        bytes.truncate(70_001);
        match bytes_value(DmType::Clob, bytes, Some(150_000), 65_536) {
            Value::Lob(lob) => {
                assert!(lob.truncated);
                assert_eq!(lob.total_len, 150_000);
                assert!(lob.preview.len() <= 65_536 && lob.preview.len() > 65_530);
                assert!(lob.preview.chars().all(|c| c == '数'), "预览中有非“数”字符");
            }
            other => panic!("{other:?}"),
        }
        // 读到的部分没有超过上限、但结尾切在字符中间
        let mut bytes = "数数数".as_bytes().to_vec();
        bytes.truncate(7);
        match bytes_value(DmType::Clob, bytes, Some(9), 65_536) {
            Value::Lob(lob) => {
                assert_eq!((lob.preview.as_str(), lob.total_len, lob.truncated), ("数数", 9, true))
            }
            other => panic!("{other:?}"),
        }
        // 未截断时完整解码
        assert_eq!(
            bytes_value(DmType::Char, "数洞".as_bytes().to_vec(), Some(6), 65_536),
            Value::String("数洞".into())
        );
    }
}
