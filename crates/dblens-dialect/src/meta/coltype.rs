//! `syscolumns` 的 coltype、collength 解码。

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decoded {
    pub type_name: String,
    pub nullable: bool,
    pub length: Option<i64>,
    pub precision: Option<i64>,
    pub scale: Option<i64>,
}

const UNITS: [&str; 16] = [
    "YEAR",
    "",
    "MONTH",
    "",
    "DAY",
    "",
    "HOUR",
    "",
    "MINUTE",
    "",
    "SECOND",
    "FRACTION(1)",
    "FRACTION(2)",
    "FRACTION(3)",
    "FRACTION(4)",
    "FRACTION(5)",
];

fn qualifier(len: i64) -> String {
    let (start, end) = (((len >> 4) & 0xf) as usize, (len & 0xf) as usize);
    let first = if UNITS[start].is_empty() { "FRACTION" } else { UNITS[start] };
    format!("{first} TO {}", UNITS[end])
}

/// `sysdefaults` 的默认值写回 SQL 表达式。`kind`：L 字面量、C CURRENT、T TODAY、U USER。
/// 字面量里数字和日期时间是“内部编码 + 空格 + 值”，字符串、布尔是值本身。
pub fn default_expr(kind: &str, raw: &str, native: &str) -> Option<String> {
    // 字符串默认值结尾带一个 NUL
    let raw = raw.trim_end_matches('\0');
    let upper = native.to_ascii_uppercase();
    let qualifier = upper.strip_prefix("DATETIME ").map(str::trim);
    match kind.trim() {
        "T" => Some("TODAY".into()),
        "U" => Some("USER".into()),
        "C" => Some(qualifier.map_or("CURRENT".into(), |q| format!("CURRENT {q}"))),
        "L" => {
            let value = raw.split_once(' ').map_or(raw, |(_, v)| v);
            let numeric = ["INT", "SERIAL", "DECIMAL", "NUMERIC", "MONEY", "FLOAT", "REAL"];
            if let Some(q) = qualifier {
                Some(format!("DATETIME({value}) {q}"))
            } else if numeric.iter().any(|n| upper.contains(n)) {
                Some(value.to_string())
            } else {
                Some(format!("'{}'", raw.replace('\'', "''")))
            }
        }
        _ => None,
    }
}

pub fn decode(coltype: i64, collength: i64, extended: Option<&str>) -> Decoded {
    let nullable = coltype & 256 == 0;
    let base = coltype & 0xff;
    let mut d = Decoded { type_name: String::new(), nullable, length: None, precision: None, scale: None };
    if let Some(name) = extended.filter(|n| !n.is_empty()) {
        d.type_name = name.to_ascii_uppercase();
        return d;
    }
    let sized = |n: &str, l: i64| format!("{n}({l})");
    d.type_name = match base {
        0 | 15 => {
            d.length = Some(collength);
            sized(if base == 0 { "CHAR" } else { "NCHAR" }, collength)
        }
        1 => "SMALLINT".into(),
        2 => "INTEGER".into(),
        3 => "FLOAT".into(),
        4 => "SMALLFLOAT".into(),
        5 | 8 => {
            let (p, s) = (collength >> 8, collength & 0xff);
            let name = if base == 5 { "DECIMAL" } else { "MONEY" };
            d.precision = Some(p);
            if s == 255 {
                sized(name, p)
            } else {
                d.scale = Some(s);
                format!("{name}({p},{s})")
            }
        }
        6 => "SERIAL".into(),
        7 => "DATE".into(),
        10 => format!("DATETIME {}", qualifier(collength)),
        11 => "BYTE".into(),
        12 => "TEXT".into(),
        13 | 16 => {
            let (max, min) = (collength & 0xff, collength >> 8);
            let name = if base == 13 { "VARCHAR" } else { "NVARCHAR" };
            d.length = Some(max);
            if min > 0 { format!("{name}({max},{min})") } else { sized(name, max) }
        }
        14 => format!("INTERVAL {}", qualifier(collength)),
        17 => "INT8".into(),
        18 => "SERIAL8".into(),
        41 => "BLOB".into(),
        43 => {
            d.length = Some(collength);
            sized("LVARCHAR", collength)
        }
        45 => "BOOLEAN".into(),
        52 => "BIGINT".into(),
        53 => "BIGSERIAL".into(),
        other => format!("TYPE{other}"),
    };
    d
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_types() {
        assert_eq!(decode(2, 4, None).type_name, "INTEGER");
        assert!(decode(2, 4, None).nullable);
        assert!(!decode(2 + 256, 4, None).nullable);
        assert_eq!(decode(52, 8, None).type_name, "BIGINT");
        assert_eq!(decode(53, 8, None).type_name, "BIGSERIAL");
        assert_eq!(decode(45, 1, None).type_name, "BOOLEAN");
    }

    #[test]
    fn varchar_length() {
        assert_eq!(decode(13, 40, None).type_name, "VARCHAR(40)");
        // VARCHAR 的 collength 低字节是最大长度，高字节是最小保留
        assert_eq!(decode(13, 10 * 256 + 40, None).type_name, "VARCHAR(40,10)");
        assert_eq!(decode(43, 2000, None).type_name, "LVARCHAR(2000)");
    }

    #[test]
    fn decimal_precision_and_scale() {
        let d = decode(5, 20 * 256 + 6, None);
        assert_eq!(d.type_name, "DECIMAL(20,6)");
        assert_eq!((d.precision, d.scale), (Some(20), Some(6)));
        // 比例 255 表示浮点小数
        assert_eq!(decode(5, 16 * 256 + 255, None).type_name, "DECIMAL(16)");
    }

    #[test]
    fn datetime_qualifier() {
        // YEAR=0 FRACTION(3)=13：collength = 长度*256 + 起*16 + 止
        assert_eq!(decode(10, 17 * 256 + 13, None).type_name, "DATETIME YEAR TO FRACTION(3)");
        assert_eq!(decode(10, 6 * 256 + 6 * 16 + 10, None).type_name, "DATETIME HOUR TO SECOND");
    }

    #[test]
    fn extended_types_use_their_catalog_name() {
        assert_eq!(decode(41, 2048, Some("blob")).type_name, "BLOB");
        assert_eq!(decode(45, 1, Some("boolean")).type_name, "BOOLEAN");
    }

    #[test]
    fn defaults_become_sql_expressions() {
        assert_eq!(default_expr("L", "wQEAAAAA 1", "DECIMAL(10,2)").as_deref(), Some("1"));
        assert_eq!(default_expr("L", "AAAABQ 5", "INT").as_deref(), Some("5"));
        assert_eq!(default_expr("L", "PmIAAA -2", "DECIMAL(5,0)").as_deref(), Some("-2"));
        assert_eq!(default_expr("L", "x y", "VARCHAR(20)").as_deref(), Some("'x y'"));
        assert_eq!(default_expr("L", "it's", "CHAR(9)").as_deref(), Some("'it''s'"));
        assert_eq!(default_expr("L", "t\0", "BOOLEAN").as_deref(), Some("'t'"));
        assert_eq!(
            default_expr("L", "xxQUAQIDBAU 2020-01-02 03:04:05", "DATETIME YEAR TO SECOND").as_deref(),
            Some("DATETIME(2020-01-02 03:04:05) YEAR TO SECOND")
        );
        assert_eq!(
            default_expr("C", "", "DATETIME YEAR TO SECOND").as_deref(),
            Some("CURRENT YEAR TO SECOND")
        );
        assert_eq!(default_expr("T", "", "DATE").as_deref(), Some("TODAY"));
        assert_eq!(default_expr("", "", "INT"), None);
    }
}
