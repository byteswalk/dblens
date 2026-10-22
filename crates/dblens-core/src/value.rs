use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use chrono::{DateTime, FixedOffset, NaiveDate, NaiveDateTime, NaiveTime};
use serde::{Deserialize, Serialize};

/// 驱动返回的单元格值。序列化为 `{"t": 类型, "v": 值}`，供前端按类型渲染。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", content = "v", rename_all = "snake_case")]
pub enum Value {
    Null,
    Bool(bool),
    Int(i64),
    UInt(u64),
    Float(f64),
    /// 精确小数，保存数据库返回的文本，不经过浮点。
    Decimal(String),
    String(String),
    Bytes(#[serde(with = "b64")] Vec<u8>),
    Date(NaiveDate),
    Time(NaiveTime),
    DateTime(NaiveDateTime),
    DateTimeTz(DateTime<FixedOffset>),
    /// 时间间隔的文本表示。
    Interval(String),
    Json(serde_json::Value),
    Uuid(uuid::Uuid),
    /// 大字段预览：只保留前若干字节。
    Lob(LobPreview),
    /// 暂不认识的类型，保留类型名和文本表示。
    Other {
        type_name: String,
        text: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LobPreview {
    pub binary: bool,
    /// 文本 LOB：前 N 字节对应的文本；二进制 LOB：前 N 字节的 base64。
    pub preview: String,
    pub total_len: u64,
    pub truncated: bool,
}

pub type Row = Vec<Value>;

/// 大字段默认预览长度（字节）。
pub const DEFAULT_LOB_PREVIEW_BYTES: usize = 64 * 1024;

impl Value {
    /// 超过 `limit` 字节的字符串或二进制转换为 LOB 预览，避免大字段占满内存。
    pub fn cap_large(self, limit: usize) -> Value {
        match self {
            Value::String(s) if s.len() > limit => {
                let mut end = limit;
                while !s.is_char_boundary(end) {
                    end -= 1;
                }
                Value::Lob(LobPreview {
                    binary: false,
                    preview: s[..end].to_string(),
                    total_len: s.len() as u64,
                    truncated: true,
                })
            }
            Value::Bytes(b) if b.len() > limit => Value::Lob(LobPreview {
                binary: true,
                preview: STANDARD.encode(&b[..limit]),
                total_len: b.len() as u64,
                truncated: true,
            }),
            other => other,
        }
    }
}

mod b64 {
    use base64::Engine;
    use base64::engine::general_purpose::STANDARD;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(bytes: &[u8], s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&STANDARD.encode(bytes))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
        let s = String::deserialize(d)?;
        STANDARD.decode(s).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{NaiveDate, TimeZone};

    #[test]
    fn json_shape_is_tagged() {
        assert_eq!(serde_json::to_value(Value::Int(42)).unwrap(), serde_json::json!({"t":"int","v":42}));
        assert_eq!(serde_json::to_value(Value::Null).unwrap(), serde_json::json!({"t":"null"}));
        assert_eq!(
            serde_json::to_value(Value::Bytes(vec![1, 2, 3])).unwrap(),
            serde_json::json!({"t":"bytes","v":"AQID"})
        );
    }

    #[test]
    fn all_variants_round_trip() {
        let values = vec![
            Value::Null,
            Value::Bool(true),
            Value::Int(-7),
            Value::UInt(u64::MAX),
            Value::Float(1.5),
            Value::Decimal("12345678901234567890.123456789".into()),
            Value::String("数洞".into()),
            Value::Bytes(vec![0, 255]),
            Value::Date(NaiveDate::from_ymd_opt(2024, 2, 29).unwrap()),
            Value::Time(chrono::NaiveTime::from_hms_micro_opt(13, 45, 30, 123456).unwrap()),
            Value::DateTime(NaiveDate::from_ymd_opt(2024, 2, 29).unwrap().and_hms_opt(1, 2, 3).unwrap()),
            Value::DateTimeTz(
                chrono::FixedOffset::east_opt(8 * 3600)
                    .unwrap()
                    .with_ymd_and_hms(2024, 2, 29, 13, 45, 30)
                    .unwrap(),
            ),
            Value::Interval("0 mons 1 days 02:03:04".into()),
            Value::Json(serde_json::json!({"a": [1, 2]})),
            Value::Uuid(uuid::Uuid::nil()),
            Value::Lob(LobPreview { binary: false, preview: "abc".into(), total_len: 100, truncated: true }),
            Value::Other { type_name: "geometry".into(), text: "\\x00".into() },
        ];
        let json = serde_json::to_string(&values).unwrap();
        let back: Vec<Value> = serde_json::from_str(&json).unwrap();
        assert_eq!(back, values);
    }

    #[test]
    fn cap_large_keeps_small_values() {
        assert_eq!(Value::String("abc".into()).cap_large(10), Value::String("abc".into()));
        assert_eq!(Value::Int(1).cap_large(0), Value::Int(1));
    }

    #[test]
    fn cap_large_truncates_on_char_boundary() {
        // "数" 占 3 字节，limit=4 时只能保留一个完整字符
        let v = Value::String("数洞数洞".into()).cap_large(4);
        assert_eq!(
            v,
            Value::Lob(LobPreview { binary: false, preview: "数".into(), total_len: 12, truncated: true })
        );
    }

    #[test]
    fn cap_large_encodes_binary_preview() {
        let v = Value::Bytes(vec![1, 2, 3, 4, 5]).cap_large(3);
        assert_eq!(
            v,
            Value::Lob(LobPreview { binary: true, preview: "AQID".into(), total_len: 5, truncated: true })
        );
    }
}
