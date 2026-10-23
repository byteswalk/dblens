//! 原生类型文本与逻辑类型的互相转换。原生类型文本来自元数据（如 `varchar(100)`、`NUMBER(10,2)`、
//! `timestamp(6) with time zone`），逻辑类型用于跨库映射和设计器。

use dblens_core::LogicalType;

use crate::Family;

/// 拆开后的原生类型：名称（大写、单词间单空格）、括号内的参数、名称之后的修饰（如 `WITH TIME ZONE`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedType {
    pub name: String,
    pub args: Vec<String>,
    pub suffix: String,
    pub unsigned: bool,
}

pub fn parse_type(native: &str) -> ParsedType {
    let upper = native.trim().to_ascii_uppercase();
    let (head, args, rest) = match upper.find('(') {
        Some(open) => {
            let close = upper[open..].find(')').map_or(upper.len(), |p| open + p);
            let args = upper[open + 1..close]
                .split(',')
                .map(|a| a.trim().to_string())
                .filter(|a| !a.is_empty())
                .collect();
            (upper[..open].to_string(), args, upper.get(close + 1..).unwrap_or("").to_string())
        }
        None => (upper.clone(), Vec::new(), String::new()),
    };
    let mut unsigned = false;
    let words: Vec<&str> = head
        .split_whitespace()
        .chain(rest.split_whitespace())
        .filter(|w| match *w {
            "UNSIGNED" => {
                unsigned = true;
                false
            }
            "SIGNED" | "ZEROFILL" => false,
            _ => true,
        })
        .collect();
    let split = words.iter().position(|w| matches!(*w, "WITH" | "WITHOUT" | "TO")).unwrap_or(words.len());
    ParsedType { name: words[..split].join(" "), args, suffix: words[split..].join(" "), unsigned }
}

/// 取参数开头的数字（`100 CHAR` → 100，`*` → None）。
fn arg_u32(args: &[String], k: usize) -> Option<u32> {
    let a = args.get(k)?;
    let digits: String = a.chars().take_while(|c| c.is_ascii_digit()).collect();
    digits.parse().ok()
}

fn arg_i32(args: &[String], k: usize) -> Option<i32> {
    args.get(k)?.trim().parse().ok()
}

fn arg_u8(args: &[String], k: usize) -> Option<u8> {
    arg_u32(args, k).and_then(|v| u8::try_from(v).ok())
}

/// 原生类型 → 逻辑类型。不认识的类型返回 `LogicalType::Other`。
pub fn logical_type(family: Family, native: &str) -> LogicalType {
    use LogicalType as L;
    let t = parse_type(native);
    let n = t.name.as_str();
    if n.starts_with("INTERVAL") {
        return L::Interval;
    }
    let len = arg_u32(&t.args, 0);
    let precision = arg_u8(&t.args, 0);
    let tz = t.suffix.starts_with("WITH ") && t.suffix.ends_with("TIME ZONE");
    let int = |bits: u8| if t.unsigned { L::UInt { bits } } else { L::Int { bits } };
    let decimal = || L::Decimal { precision: arg_u32(&t.args, 0), scale: arg_i32(&t.args, 1) };
    match family {
        Family::MySql => match n {
            "TINYINT" if len == Some(1) && !t.unsigned => L::Bool,
            "BOOL" | "BOOLEAN" => L::Bool,
            "TINYINT" => int(8),
            "SMALLINT" => int(16),
            "MEDIUMINT" => int(24),
            "INT" | "INTEGER" => int(32),
            "BIGINT" => int(64),
            "YEAR" => L::Int { bits: 16 },
            "FLOAT" => L::Float { bits: 32 },
            "DOUBLE" | "DOUBLE PRECISION" | "REAL" => L::Float { bits: 64 },
            "DECIMAL" | "NUMERIC" | "DEC" | "FIXED" => decimal(),
            "CHAR" => L::Char { len },
            "VARCHAR" => L::VarChar { len },
            "TINYTEXT" | "TEXT" | "MEDIUMTEXT" | "LONGTEXT" => L::Text,
            "BINARY" => L::Binary { len },
            "VARBINARY" => L::VarBinary { len },
            "TINYBLOB" | "BLOB" | "MEDIUMBLOB" | "LONGBLOB" => L::Blob,
            "DATE" => L::Date,
            "TIME" => L::Time { precision },
            "DATETIME" => L::Timestamp { tz: false, precision },
            // MySQL 的 TIMESTAMP 按会话时区换算，语义上带时区
            "TIMESTAMP" => L::Timestamp { tz: true, precision },
            "JSON" => L::Json,
            _ => L::Other,
        },
        Family::Postgres => match n {
            "BOOLEAN" | "BOOL" => L::Bool,
            "SMALLINT" | "INT2" | "SMALLSERIAL" | "SERIAL2" => L::Int { bits: 16 },
            "INTEGER" | "INT" | "INT4" | "SERIAL" | "SERIAL4" => L::Int { bits: 32 },
            "BIGINT" | "INT8" | "BIGSERIAL" | "SERIAL8" => L::Int { bits: 64 },
            "REAL" | "FLOAT4" => L::Float { bits: 32 },
            "DOUBLE PRECISION" | "FLOAT8" => L::Float { bits: 64 },
            "FLOAT" => L::Float { bits: if len.is_some_and(|p| p <= 24) { 32 } else { 64 } },
            "NUMERIC" | "DECIMAL" => decimal(),
            "CHARACTER" | "CHAR" | "BPCHAR" => L::Char { len },
            "CHARACTER VARYING" | "VARCHAR" => L::VarChar { len },
            "TEXT" | "NAME" | "CITEXT" => L::Text,
            "BYTEA" => L::Blob,
            "DATE" => L::Date,
            "TIME" | "TIMETZ" => L::Time { precision },
            "TIMESTAMP" => L::Timestamp { tz, precision },
            "TIMESTAMPTZ" => L::Timestamp { tz: true, precision },
            "JSON" | "JSONB" => L::Json,
            "UUID" => L::Uuid,
            _ => L::Other,
        },
        Family::Oracle => match n {
            "NUMBER" => decimal(),
            // Oracle 的 INTEGER/INT/SMALLINT 都是 NUMBER(38)
            "INTEGER" | "INT" | "SMALLINT" => L::Decimal { precision: Some(38), scale: Some(0) },
            "FLOAT" => L::Float { bits: 64 },
            "BINARY_FLOAT" => L::Float { bits: 32 },
            "BINARY_DOUBLE" => L::Float { bits: 64 },
            "VARCHAR2" | "NVARCHAR2" | "VARCHAR" => L::VarChar { len },
            "CHAR" | "NCHAR" => L::Char { len },
            "CLOB" | "NCLOB" | "LONG" => L::Text,
            "BLOB" | "LONG RAW" | "BFILE" => L::Blob,
            "RAW" => L::VarBinary { len },
            // Oracle 的 DATE 带时分秒
            "DATE" => L::Timestamp { tz: false, precision: Some(0) },
            "TIMESTAMP" => L::Timestamp { tz, precision },
            "JSON" => L::Json,
            "BOOLEAN" => L::Bool,
            _ => L::Other,
        },
        Family::Dameng => match n {
            "BIT" | "BOOL" | "BOOLEAN" => L::Bool,
            "TINYINT" | "BYTE" => L::Int { bits: 8 },
            "SMALLINT" => L::Int { bits: 16 },
            "INT" | "INTEGER" | "PLS_INTEGER" => L::Int { bits: 32 },
            "BIGINT" => L::Int { bits: 64 },
            "DEC" | "DECIMAL" | "NUMERIC" | "NUMBER" => decimal(),
            "REAL" => L::Float { bits: 32 },
            "FLOAT" | "DOUBLE" | "DOUBLE PRECISION" => L::Float { bits: 64 },
            "CHAR" | "CHARACTER" => L::Char { len },
            "VARCHAR" | "VARCHAR2" => L::VarChar { len },
            "TEXT" | "LONGVARCHAR" | "CLOB" => L::Text,
            "BINARY" => L::Binary { len },
            "VARBINARY" | "RAW" => L::VarBinary { len },
            "BLOB" | "IMAGE" | "LONGVARBINARY" => L::Blob,
            "DATE" => L::Date,
            "TIME" => L::Time { precision },
            "TIMESTAMP" | "DATETIME" => L::Timestamp { tz, precision },
            _ => L::Other,
        },
    }
}

fn with_len(name: &str, len: Option<u32>) -> String {
    match len {
        Some(n) => format!("{name}({n})"),
        None => name.to_string(),
    }
}

fn with_precision(name: &str, p: Option<u8>) -> String {
    with_len(name, p.map(u32::from))
}

fn decimal_text(name: &str, precision: Option<u32>, scale: Option<i32>, default: &str) -> String {
    match (precision, scale) {
        (Some(p), Some(s)) => format!("{name}({p},{s})"),
        (Some(p), None) => format!("{name}({p})"),
        _ => default.to_string(),
    }
}

/// 逻辑类型 → 该方言的原生类型文本（用于建表和跨库迁移）。
pub fn native_type(family: Family, logical: &LogicalType) -> String {
    use LogicalType as L;
    match family {
        Family::MySql => match logical {
            L::Bool => "TINYINT(1)".into(),
            L::Int { bits } | L::UInt { bits } => {
                let base = match bits {
                    0..=8 => "TINYINT",
                    9..=16 => "SMALLINT",
                    17..=24 => "MEDIUMINT",
                    25..=32 => "INT",
                    _ => "BIGINT",
                };
                if matches!(logical, L::UInt { .. }) { format!("{base} UNSIGNED") } else { base.into() }
            }
            L::Float { bits } => if *bits <= 32 { "FLOAT" } else { "DOUBLE" }.into(),
            L::Decimal { precision, scale } => decimal_text("DECIMAL", *precision, *scale, "DECIMAL(65,30)"),
            L::Char { len } => with_len("CHAR", Some(len.unwrap_or(1))),
            L::VarChar { len: Some(n) } => format!("VARCHAR({n})"),
            L::VarChar { len: None } | L::Text | L::Other => "LONGTEXT".into(),
            L::Binary { len } => with_len("BINARY", Some(len.unwrap_or(1))),
            L::VarBinary { len: Some(n) } => format!("VARBINARY({n})"),
            L::VarBinary { len: None } | L::Blob => "LONGBLOB".into(),
            L::Date => "DATE".into(),
            L::Time { precision } => with_precision("TIME", *precision),
            L::Timestamp { tz: false, precision } => with_precision("DATETIME", *precision),
            L::Timestamp { tz: true, precision } => with_precision("TIMESTAMP", *precision),
            // MySQL 没有时间间隔类型，按文本存储
            L::Interval => "VARCHAR(100)".into(),
            L::Json => "JSON".into(),
            L::Uuid => "CHAR(36)".into(),
        },
        Family::Postgres => match logical {
            L::Bool => "BOOLEAN".into(),
            L::Int { bits } => match bits {
                0..=16 => "SMALLINT",
                17..=32 => "INTEGER",
                _ => "BIGINT",
            }
            .into(),
            // PG 没有无符号整数，取能容纳全部取值的更宽类型
            L::UInt { bits } => match bits {
                0..=8 => "SMALLINT",
                9..=16 => "INTEGER",
                17..=32 => "BIGINT",
                _ => "NUMERIC(20)",
            }
            .into(),
            L::Float { bits } => if *bits <= 32 { "REAL" } else { "DOUBLE PRECISION" }.into(),
            L::Decimal { precision, scale } => decimal_text("NUMERIC", *precision, *scale, "NUMERIC"),
            L::Char { len } => with_len("CHAR", Some(len.unwrap_or(1))),
            L::VarChar { len: Some(n) } => format!("VARCHAR({n})"),
            L::VarChar { len: None } | L::Text | L::Other => "TEXT".into(),
            L::Binary { .. } | L::VarBinary { .. } | L::Blob => "BYTEA".into(),
            L::Date => "DATE".into(),
            L::Time { precision } => with_precision("TIME", *precision),
            L::Timestamp { tz: false, precision } => with_precision("TIMESTAMP", *precision),
            L::Timestamp { tz: true, precision } => {
                format!("{} WITH TIME ZONE", with_precision("TIMESTAMP", *precision))
            }
            L::Interval => "INTERVAL".into(),
            L::Json => "JSONB".into(),
            L::Uuid => "UUID".into(),
        },
        Family::Oracle => match logical {
            // 为兼容 23ai 之前的版本，布尔用 NUMBER(1)
            L::Bool => "NUMBER(1)".into(),
            L::Int { bits } => format!(
                "NUMBER({})",
                match bits {
                    0..=8 => 3,
                    9..=16 => 5,
                    17..=24 => 7,
                    25..=32 => 10,
                    _ => 19,
                }
            ),
            L::UInt { bits } => format!(
                "NUMBER({})",
                match bits {
                    0..=8 => 3,
                    9..=16 => 5,
                    17..=24 => 8,
                    25..=32 => 10,
                    _ => 20,
                }
            ),
            L::Float { bits } => if *bits <= 32 { "BINARY_FLOAT" } else { "BINARY_DOUBLE" }.into(),
            L::Decimal { precision, scale } => decimal_text("NUMBER", *precision, *scale, "NUMBER"),
            L::Char { len } => format!("CHAR({} CHAR)", len.unwrap_or(1)),
            L::VarChar { len: Some(n) } if *n <= 4000 => format!("VARCHAR2({n} CHAR)"),
            L::VarChar { .. } | L::Text | L::Json | L::Other => "CLOB".into(),
            L::Binary { len: Some(n) } | L::VarBinary { len: Some(n) } if *n <= 2000 => format!("RAW({n})"),
            L::Binary { .. } | L::VarBinary { .. } | L::Blob => "BLOB".into(),
            L::Date => "DATE".into(),
            // Oracle 没有 TIME 类型，按 'HH24:MI:SS.FF6' 文本存储
            L::Time { .. } => "VARCHAR2(18)".into(),
            L::Timestamp { tz: false, precision } => with_precision("TIMESTAMP", *precision),
            L::Timestamp { tz: true, precision } => {
                format!("{} WITH TIME ZONE", with_precision("TIMESTAMP", *precision))
            }
            L::Interval => "INTERVAL DAY(9) TO SECOND(6)".into(),
            L::Uuid => "VARCHAR2(36)".into(),
        },
        Family::Dameng => match logical {
            L::Bool => "BIT".into(),
            L::Int { bits } => match bits {
                0..=8 => "TINYINT",
                9..=16 => "SMALLINT",
                17..=32 => "INT",
                _ => "BIGINT",
            }
            .into(),
            L::UInt { bits } => match bits {
                0..=8 => "SMALLINT",
                9..=16 => "INT",
                17..=32 => "BIGINT",
                _ => "DEC(20,0)",
            }
            .into(),
            L::Float { bits } => if *bits <= 32 { "REAL" } else { "DOUBLE" }.into(),
            L::Decimal { precision, scale } => decimal_text("DEC", *precision, *scale, "DEC"),
            L::Char { len } => with_len("CHAR", Some(len.unwrap_or(1))),
            L::VarChar { len: Some(n) } => format!("VARCHAR({n})"),
            L::VarChar { len: None } | L::Text => "TEXT".into(),
            L::Binary { len } => with_len("BINARY", Some(len.unwrap_or(1))),
            L::VarBinary { len: Some(n) } => format!("VARBINARY({n})"),
            L::VarBinary { len: None } | L::Blob => "BLOB".into(),
            L::Date => "DATE".into(),
            L::Time { precision } => with_precision("TIME", *precision),
            L::Timestamp { tz: false, precision } => with_precision("TIMESTAMP", *precision),
            L::Timestamp { tz: true, precision } => {
                format!("{} WITH TIME ZONE", with_precision("TIMESTAMP", *precision))
            }
            L::Interval => "INTERVAL DAY TO SECOND".into(),
            L::Json | L::Other => "CLOB".into(),
            L::Uuid => "VARCHAR(36)".into(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use LogicalType as L;

    #[test]
    fn parses_type_text() {
        let t = parse_type("decimal(10, 2) unsigned zerofill");
        assert_eq!(
            (t.name.as_str(), t.args.clone(), t.suffix.as_str(), t.unsigned),
            ("DECIMAL", vec!["10".to_string(), "2".to_string()], "", true)
        );
        let t = parse_type("timestamp(6) with time zone");
        assert_eq!(
            (t.name.as_str(), t.args.clone(), t.suffix.as_str()),
            ("TIMESTAMP", vec!["6".to_string()], "WITH TIME ZONE")
        );
        let t = parse_type("TIMESTAMP WITH LOCAL TIME ZONE");
        assert_eq!((t.name.as_str(), t.suffix.as_str()), ("TIMESTAMP", "WITH LOCAL TIME ZONE"));
        assert_eq!(parse_type("character varying(100)").name, "CHARACTER VARYING");
        assert_eq!(parse_type("VARCHAR2(100 CHAR)").args, vec!["100 CHAR".to_string()]);
        assert_eq!(parse_type("int unsigned").name, "INT");
        assert_eq!(parse_type("INTERVAL DAY(2) TO SECOND(6)").name, "INTERVAL DAY");
        assert_eq!(parse_type("double precision").name, "DOUBLE PRECISION");
    }

    fn check_to_logical(f: Family, cases: &[(&str, L)]) {
        for (native, expected) in cases {
            assert_eq!(&logical_type(f, native), expected, "{f:?} 原生类型 {native}");
        }
    }

    fn check_to_native(f: Family, cases: &[(L, &str)]) {
        for (logical, expected) in cases {
            assert_eq!(native_type(f, logical), *expected, "{f:?} 逻辑类型 {logical:?}");
        }
    }

    #[test]
    fn mysql_native_to_logical() {
        check_to_logical(
            Family::MySql,
            &[
                ("tinyint(1)", L::Bool),
                ("tinyint", L::Int { bits: 8 }),
                ("int unsigned", L::UInt { bits: 32 }),
                ("bigint", L::Int { bits: 64 }),
                ("mediumint", L::Int { bits: 24 }),
                ("decimal(10,2)", L::Decimal { precision: Some(10), scale: Some(2) }),
                ("double", L::Float { bits: 64 }),
                ("float", L::Float { bits: 32 }),
                ("varchar(100)", L::VarChar { len: Some(100) }),
                ("char(2)", L::Char { len: Some(2) }),
                ("longtext", L::Text),
                ("varbinary(16)", L::VarBinary { len: Some(16) }),
                ("mediumblob", L::Blob),
                ("datetime(6)", L::Timestamp { tz: false, precision: Some(6) }),
                ("timestamp", L::Timestamp { tz: true, precision: None }),
                ("time(3)", L::Time { precision: Some(3) }),
                ("year", L::Int { bits: 16 }),
                ("json", L::Json),
                ("enum('a','b')", L::Other),
            ],
        );
    }

    #[test]
    fn mysql_logical_to_native() {
        check_to_native(
            Family::MySql,
            &[
                (L::Bool, "TINYINT(1)"),
                (L::UInt { bits: 64 }, "BIGINT UNSIGNED"),
                (L::Decimal { precision: Some(10), scale: Some(2) }, "DECIMAL(10,2)"),
                (L::Decimal { precision: None, scale: None }, "DECIMAL(65,30)"),
                (L::VarChar { len: Some(100) }, "VARCHAR(100)"),
                (L::VarChar { len: None }, "LONGTEXT"),
                (L::Timestamp { tz: false, precision: Some(6) }, "DATETIME(6)"),
                (L::Timestamp { tz: true, precision: None }, "TIMESTAMP"),
                (L::Uuid, "CHAR(36)"),
                (L::Other, "LONGTEXT"),
            ],
        );
    }

    #[test]
    fn postgres_mappings() {
        check_to_logical(
            Family::Postgres,
            &[
                ("integer", L::Int { bits: 32 }),
                ("int8", L::Int { bits: 64 }),
                ("numeric(12,4)", L::Decimal { precision: Some(12), scale: Some(4) }),
                ("numeric", L::Decimal { precision: None, scale: None }),
                ("character varying(50)", L::VarChar { len: Some(50) }),
                ("character(3)", L::Char { len: Some(3) }),
                ("text", L::Text),
                ("bytea", L::Blob),
                ("boolean", L::Bool),
                ("double precision", L::Float { bits: 64 }),
                ("real", L::Float { bits: 32 }),
                ("timestamp(3) without time zone", L::Timestamp { tz: false, precision: Some(3) }),
                ("timestamp with time zone", L::Timestamp { tz: true, precision: None }),
                ("timestamptz", L::Timestamp { tz: true, precision: None }),
                ("interval day to second(3)", L::Interval),
                ("jsonb", L::Json),
                ("uuid", L::Uuid),
                ("point", L::Other),
            ],
        );
        check_to_native(
            Family::Postgres,
            &[
                (L::Int { bits: 8 }, "SMALLINT"),
                (L::UInt { bits: 64 }, "NUMERIC(20)"),
                (L::Float { bits: 64 }, "DOUBLE PRECISION"),
                (L::Blob, "BYTEA"),
                (L::Timestamp { tz: true, precision: Some(6) }, "TIMESTAMP(6) WITH TIME ZONE"),
                (L::Json, "JSONB"),
                (L::VarChar { len: None }, "TEXT"),
            ],
        );
    }

    #[test]
    fn oracle_mappings() {
        check_to_logical(
            Family::Oracle,
            &[
                ("NUMBER(10,0)", L::Decimal { precision: Some(10), scale: Some(0) }),
                ("NUMBER", L::Decimal { precision: None, scale: None }),
                ("NUMBER(*,0)", L::Decimal { precision: None, scale: Some(0) }),
                ("INTEGER", L::Decimal { precision: Some(38), scale: Some(0) }),
                ("BINARY_DOUBLE", L::Float { bits: 64 }),
                ("VARCHAR2(100 CHAR)", L::VarChar { len: Some(100) }),
                ("NVARCHAR2(20)", L::VarChar { len: Some(20) }),
                ("CHAR(1)", L::Char { len: Some(1) }),
                ("CLOB", L::Text),
                ("LONG RAW", L::Blob),
                ("RAW(16)", L::VarBinary { len: Some(16) }),
                ("DATE", L::Timestamp { tz: false, precision: Some(0) }),
                ("TIMESTAMP(6)", L::Timestamp { tz: false, precision: Some(6) }),
                ("TIMESTAMP(6) WITH LOCAL TIME ZONE", L::Timestamp { tz: true, precision: Some(6) }),
                ("INTERVAL YEAR(2) TO MONTH", L::Interval),
                ("BOOLEAN", L::Bool),
                ("XMLTYPE", L::Other),
            ],
        );
        check_to_native(
            Family::Oracle,
            &[
                (L::Bool, "NUMBER(1)"),
                (L::Int { bits: 32 }, "NUMBER(10)"),
                (L::Float { bits: 32 }, "BINARY_FLOAT"),
                (L::VarChar { len: Some(100) }, "VARCHAR2(100 CHAR)"),
                (L::VarChar { len: None }, "CLOB"),
                (L::VarBinary { len: Some(16) }, "RAW(16)"),
                (L::VarBinary { len: Some(4000) }, "BLOB"),
                (L::Date, "DATE"),
                (L::Time { precision: None }, "VARCHAR2(18)"),
                (L::Timestamp { tz: false, precision: None }, "TIMESTAMP"),
                (L::Json, "CLOB"),
                (L::Uuid, "VARCHAR2(36)"),
            ],
        );
    }

    #[test]
    fn dameng_mappings() {
        check_to_logical(
            Family::Dameng,
            &[
                ("INT", L::Int { bits: 32 }),
                ("BIT", L::Bool),
                ("TINYINT", L::Int { bits: 8 }),
                ("DEC(10,4)", L::Decimal { precision: Some(10), scale: Some(4) }),
                ("NUMBER(8)", L::Decimal { precision: Some(8), scale: None }),
                ("DOUBLE", L::Float { bits: 64 }),
                ("VARCHAR(50)", L::VarChar { len: Some(50) }),
                ("VARCHAR2(50)", L::VarChar { len: Some(50) }),
                ("TEXT", L::Text),
                ("IMAGE", L::Blob),
                ("DATETIME(6)", L::Timestamp { tz: false, precision: Some(6) }),
                ("TIMESTAMP(6) WITH TIME ZONE", L::Timestamp { tz: true, precision: Some(6) }),
                ("TIME(0)", L::Time { precision: Some(0) }),
                ("ROWID", L::Other),
            ],
        );
        check_to_native(
            Family::Dameng,
            &[
                (L::Bool, "BIT"),
                (L::Int { bits: 64 }, "BIGINT"),
                (L::UInt { bits: 64 }, "DEC(20,0)"),
                (L::Decimal { precision: Some(10), scale: Some(2) }, "DEC(10,2)"),
                (L::VarChar { len: None }, "TEXT"),
                (L::Timestamp { tz: true, precision: Some(3) }, "TIMESTAMP(3) WITH TIME ZONE"),
                (L::Json, "CLOB"),
            ],
        );
    }
}
