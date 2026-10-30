//! 原生类型文本与逻辑类型的互相转换。原生类型文本来自元数据（如 `varchar(100)`、`NUMBER(10,2)`、
//! `timestamp(6) with time zone`），逻辑类型用于跨库映射和设计器。
//!
//! 约定：
//! - 原生 → 逻辑时，没有显式精度的时间类型补上源库的默认精度（PG/Oracle/达梦 6，MySQL 0，SQL Server 7），
//!   这样跨库时目标库不会因为默认精度不同而悄悄改变精度。
//! - 逻辑 → 原生时，按目标库的上限钳制精度、长度和小数位，保证生成的 DDL 在目标库上可执行。

use dblens_core::LogicalType;

use crate::Family;

/// 拆开后的原生类型：名称（大写、单词间单空格）、括号内的参数、名称之后的修饰（如 `WITH TIME ZONE`）。
/// 名称之后的列子句（`CHARACTER SET`、`COLLATE`、`DEFAULT`、`NOT NULL` 等）整体忽略。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedType {
    pub name: String,
    pub args: Vec<String>,
    pub suffix: String,
    pub unsigned: bool,
    /// 数组类型：末尾是 `[]`（或 `[N]`），或 PG 内部名以 `_` 开头（如 `_int4`）。
    pub array: bool,
}

/// 第一个列子句关键字的位置，没有则返回长度。`CHARACTER` 只有后面跟 `SET` 才算子句，`CHARACTER VARYING` 是类型名。
fn clause_pos(w: &[&str]) -> usize {
    let is_clause = |(i, x): (usize, &&str)| match *x {
        "CHARACTER" => w.get(i + 1) == Some(&"SET"),
        "CHARSET" | "COLLATE" | "ON" | "GENERATED" | "DEFAULT" | "NOT" | "NULL" | "AUTO_INCREMENT"
        | "COMMENT" => true,
        _ => false,
    };
    w.iter().enumerate().position(is_clause).unwrap_or(w.len())
}

pub fn parse_type(native: &str) -> ParsedType {
    let mut s = native.trim();
    let mut array = false;
    while s.ends_with(']') {
        let Some(i) = s.rfind('[') else { break };
        s = s[..i].trim_end();
        array = true;
    }
    let upper = s.to_ascii_uppercase();
    let (head, mut args, rest) = match upper.find('(') {
        Some(open) => {
            // 引号内的右括号不算结束（如 enum('a)b')）
            let mut quoted = false;
            let close = upper[open + 1..]
                .char_indices()
                .find(|&(_, c)| {
                    if c == '\'' {
                        quoted = !quoted;
                    }
                    c == ')' && !quoted
                })
                .map_or(upper.len(), |(i, _)| open + 1 + i);
            let args: Vec<String> = upper[open + 1..close]
                .split(',')
                .map(|a| a.trim().to_string())
                .filter(|a| !a.is_empty())
                .collect();
            (&upper[..open], args, upper.get(close + 1..).unwrap_or(""))
        }
        None => (upper.as_str(), Vec::new(), ""),
    };
    let head_words: Vec<&str> = head.split_whitespace().collect();
    let cut = clause_pos(&head_words);
    let mut words = head_words;
    if cut < words.len() {
        // 子句出现在括号之前，后面的括号属于子句（如 `int default (0)`）
        words.truncate(cut);
        args.clear();
    } else {
        let rest_words: Vec<&str> = rest.split_whitespace().collect();
        words.extend(&rest_words[..clause_pos(&rest_words)]);
    }
    let mut unsigned = false;
    words.retain(|w| match *w {
        "UNSIGNED" => {
            unsigned = true;
            false
        }
        "SIGNED" | "ZEROFILL" => false,
        _ => true,
    });
    let split = words.iter().position(|w| matches!(*w, "WITH" | "WITHOUT" | "TO")).unwrap_or(words.len());
    let name = words[..split].join(" ");
    let array = array || name.starts_with('_');
    ParsedType { name, args, suffix: words[split..].join(" "), unsigned, array }
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
    if t.array {
        return L::Other;
    }
    let n = t.name.as_str();
    // INTERVAL 按整词匹配；年月间隔（INTERVAL YEAR…）无法和日秒间隔互转，归为 Other
    if n == "INTERVAL" || n.starts_with("INTERVAL ") {
        let year_month = n.starts_with("INTERVAL YEAR") || n.starts_with("INTERVAL MONTH");
        return if year_month { L::Other } else { L::Interval };
    }
    let len = arg_u32(&t.args, 0);
    let precision = arg_u8(&t.args, 0);
    // 没有显式精度时补源库默认精度
    let prec = |default: u8| precision.or(Some(default));
    let tz = t.suffix.starts_with("WITH ") && t.suffix.ends_with("TIME ZONE");
    let int = |bits: u8| if t.unsigned { L::UInt { bits } } else { L::Int { bits } };
    let decimal = || L::Decimal { precision: arg_u32(&t.args, 0), scale: arg_i32(&t.args, 1) };
    match family {
        Family::MySql => match n {
            "TINYINT" if len == Some(1) && !t.unsigned => L::Bool,
            "BOOL" | "BOOLEAN" => L::Bool,
            // BIT(1) 当布尔；更宽的 BIT(n) 按位串存成 ceil(n/8) 字节
            "BIT" => match len {
                None | Some(1) => L::Bool,
                Some(n) => L::Binary { len: Some(n.div_ceil(8)) },
            },
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
            "TIME" => L::Time { precision: prec(0) },
            "DATETIME" => L::Timestamp { tz: false, precision: prec(0) },
            // MySQL 的 TIMESTAMP 按会话时区换算，语义上带时区
            "TIMESTAMP" => L::Timestamp { tz: true, precision: prec(0) },
            "JSON" => L::Json,
            _ => L::Other,
        },
        Family::Postgres => match n {
            "BOOLEAN" | "BOOL" => L::Bool,
            // 只有 bit(1) 当布尔，其余位串（bit(n)、bit varying）没有对应逻辑类型
            "BIT" if matches!(len, None | Some(1)) => L::Bool,
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
            // 带时区的 time（timetz）没有对应逻辑类型
            "TIME" if !tz => L::Time { precision: prec(6) },
            "TIMESTAMP" => L::Timestamp { tz, precision: prec(6) },
            "TIMESTAMPTZ" => L::Timestamp { tz: true, precision: prec(6) },
            "JSON" | "JSONB" => L::Json,
            "UUID" => L::Uuid,
            _ => L::Other,
        },
        Family::Oracle => match n {
            // Oracle 的 DEC/DECIMAL/NUMERIC 是 NUMBER 的别名，参数含义相同
            "NUMBER" | "DEC" | "DECIMAL" | "NUMERIC" => decimal(),
            // Oracle 的 INTEGER/INT/SMALLINT 都是 NUMBER(38)
            "INTEGER" | "INT" | "SMALLINT" => L::Decimal { precision: Some(38), scale: Some(0) },
            // Oracle 的 FLOAT(n) 是二进制精度的 NUMBER 子类型，按无限定的 NUMBER 处理
            "FLOAT" => L::Decimal { precision: None, scale: None },
            // REAL 与 DOUBLE PRECISION 是 NUMBER 的子类型 FLOAT(63)/FLOAT(126)，不是 IEEE 浮点，与 FLOAT 同样处理
            "REAL" | "DOUBLE PRECISION" => L::Decimal { precision: None, scale: None },
            "BINARY_FLOAT" => L::Float { bits: 32 },
            "BINARY_DOUBLE" => L::Float { bits: 64 },
            "VARCHAR2" | "NVARCHAR2" | "VARCHAR" => L::VarChar { len },
            "CHAR" | "NCHAR" => L::Char { len },
            "CLOB" | "NCLOB" | "LONG" => L::Text,
            "BLOB" | "LONG RAW" => L::Blob,
            "RAW" => L::VarBinary { len },
            // Oracle 的 DATE 带时分秒
            "DATE" => L::Timestamp { tz: false, precision: Some(0) },
            "TIMESTAMP" => L::Timestamp { tz, precision: prec(6) },
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
            "CHAR" | "CHARACTER" | "NCHAR" => L::Char { len },
            "VARCHAR" | "VARCHAR2" | "NVARCHAR" | "NVARCHAR2" | "CHARACTER VARYING" => L::VarChar { len },
            "TEXT" | "LONGVARCHAR" | "CLOB" => L::Text,
            "JSON" | "JSONB" => L::Json,
            "BINARY" => L::Binary { len },
            "VARBINARY" | "RAW" => L::VarBinary { len },
            "BLOB" | "IMAGE" | "LONGVARBINARY" => L::Blob,
            "DATE" => L::Date,
            // TIME WITH TIME ZONE 没有对应逻辑类型
            "TIME" if !tz => L::Time { precision: prec(6) },
            "TIMESTAMP" | "DATETIME" => L::Timestamp { tz, precision: prec(6) },
            _ => L::Other,
        },
        // SQL Server 的 (MAX) 长度解析为 None
        Family::SqlServer => match n {
            "BIT" => L::Bool,
            // TINYINT 是 0–255
            "TINYINT" => L::UInt { bits: 8 },
            "SMALLINT" => L::Int { bits: 16 },
            "INT" => L::Int { bits: 32 },
            "BIGINT" => L::Int { bits: 64 },
            "REAL" => L::Float { bits: 32 },
            "FLOAT" => L::Float { bits: if len.is_some_and(|p| p <= 24) { 32 } else { 64 } },
            "DECIMAL" | "NUMERIC" => decimal(),
            "MONEY" => L::Decimal { precision: Some(19), scale: Some(4) },
            "SMALLMONEY" => L::Decimal { precision: Some(10), scale: Some(4) },
            "CHAR" | "NCHAR" => L::Char { len },
            "VARCHAR" | "NVARCHAR" => L::VarChar { len },
            "TEXT" | "NTEXT" | "XML" => L::Text,
            "BINARY" => L::Binary { len },
            "VARBINARY" => L::VarBinary { len },
            "IMAGE" => L::Blob,
            "DATE" => L::Date,
            "TIME" => L::Time { precision: prec(7) },
            "DATETIME2" => L::Timestamp { tz: false, precision: prec(7) },
            "DATETIME" => L::Timestamp { tz: false, precision: Some(3) },
            "SMALLDATETIME" => L::Timestamp { tz: false, precision: Some(0) },
            "DATETIMEOFFSET" => L::Timestamp { tz: true, precision: prec(7) },
            "UNIQUEIDENTIFIER" => L::Uuid,
            // ROWVERSION / TIMESTAMP（行版本）、空间类型、HIERARCHYID、SQL_VARIANT 等没有对应逻辑类型
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

/// 时间精度按目标库上限钳制：Oracle 最高 9，SQL Server 最高 7，其余最高 6。
fn with_precision(family: Family, name: &str, p: Option<u8>) -> String {
    let max = match family {
        Family::Oracle => 9,
        Family::SqlServer => 7,
        _ => 6,
    };
    with_len(name, p.map(|v| u32::from(v.min(max))))
}

/// 定点数按目标库的上限和规则归一后生成类型文本。
fn decimal_native(family: Family, precision: Option<u32>, scale: Option<i32>) -> String {
    let (name, bare, max) = match family {
        Family::MySql => ("DECIMAL", "DECIMAL(65,30)", 65),
        Family::Postgres => ("NUMERIC", "NUMERIC", 1000),
        Family::Oracle => ("NUMBER", "NUMBER", 38),
        Family::Dameng => ("DEC", "DEC", 38),
        // SQL Server 的 DECIMAL 不写参数是 (18,0)，无限定的源类型给 (38,10)
        Family::SqlServer => ("DECIMAL", "DECIMAL(38,10)", 38),
    };
    // 精度 0 在各库都无效，按无限定处理
    let precision = precision.filter(|&p| p > 0);
    // Oracle 小数位范围 -84..=127
    let scale = if family == Family::Oracle { scale.map(|v| v.clamp(-84, 127)) } else { scale };
    let (mut p, mut s) = match (precision, scale) {
        (None, None) => return bare.into(),
        // 只有小数位没有精度（Oracle 的 NUMBER(*,s)）：补最大精度 38
        (None, s) => (38, s),
        (Some(p), s) => (p, s),
    };
    match s {
        // 只有 Oracle 支持负小数位；其他库把整数位补到精度里，小数位归 0
        Some(v) if v < 0 && family != Family::Oracle => {
            p = p.saturating_add(v.unsigned_abs());
            s = Some(0);
        }
        // 小数位大于精度时，把精度抬到小数位
        Some(v) if v > 0 && v.unsigned_abs() > p => p = v.unsigned_abs(),
        _ => {}
    }
    if p > max {
        return match family {
            // MySQL 精度上限 65 且小数位上限 30
            Family::MySql => match s {
                Some(v) => format!("DECIMAL(65,{})", v.min(30)),
                None => "DECIMAL(65)".into(),
            },
            // 超出上限时放弃限定，用各库的无限定类型
            Family::Postgres => "NUMERIC".into(),
            Family::Oracle => "NUMBER".into(),
            Family::Dameng => "DEC".into(),
            Family::SqlServer => "DECIMAL(38,10)".into(),
        };
    }
    if family == Family::MySql {
        // MySQL 小数位上限 30，超出会报错，这里钳制
        s = s.map(|v| v.min(30));
    }
    match s {
        Some(v) => format!("{name}({p},{v})"),
        None => format!("{name}({p})"),
    }
}

/// MySQL 的 VARCHAR：按 utf8mb4 每字符 4 字节，行内上限 16383 个字符，超出改 MEDIUMTEXT。
fn mysql_varchar(n: u32) -> String {
    if n > 16383 { "MEDIUMTEXT".into() } else { format!("VARCHAR({n})") }
}

/// MySQL 的 VARBINARY 上限 65535 字节，超出改 LONGBLOB。
fn mysql_varbinary(n: u32) -> String {
    if n > 65535 { "LONGBLOB".into() } else { format!("VARBINARY({n})") }
}

/// Oracle 的 VARCHAR2：保留 `n CHAR` 写法（n ≤ 4000）。注意多字节字符的值超过 4000 字节时仍会失败，这一点已接受。
fn oracle_varchar(n: u32) -> String {
    if n <= 4000 { format!("VARCHAR2({n} CHAR)") } else { "CLOB".into() }
}

/// 达梦的 VARCHAR：默认按字节计长，所以显式写 `CHAR` 语义；上限 8188，超出改 TEXT。
/// 注意：`VARCHAR(n CHAR)` 的写法尚未在真实 DM8 上验证。
fn dameng_varchar(n: u32) -> String {
    if n <= 8188 { format!("VARCHAR({n} CHAR)") } else { "TEXT".into() }
}

/// 长度为 0 的字符/二进制类型在各库都无效，按 1 处理。
fn fix_zero_len(logical: &LogicalType) -> LogicalType {
    use LogicalType as L;
    let one = |len: &Option<u32>| len.map(|n| n.max(1));
    match logical {
        L::Char { len } => L::Char { len: one(len) },
        L::VarChar { len } => L::VarChar { len: one(len) },
        L::Binary { len } => L::Binary { len: one(len) },
        L::VarBinary { len } => L::VarBinary { len: one(len) },
        other => other.clone(),
    }
}

/// 逻辑类型 → 该方言的原生类型文本（用于建表和跨库迁移）。
pub fn native_type(family: Family, logical: &LogicalType) -> String {
    use LogicalType as L;
    let logical = &fix_zero_len(logical);
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
            L::Decimal { precision, scale } => decimal_native(family, *precision, *scale),
            // CHAR 上限 255，超出按 VARCHAR 处理
            L::Char { len } => match len.unwrap_or(1) {
                n if n > 255 => mysql_varchar(n),
                n => format!("CHAR({n})"),
            },
            L::VarChar { len: Some(n) } => mysql_varchar(*n),
            L::VarChar { len: None } | L::Text | L::Other => "LONGTEXT".into(),
            // BINARY 上限 255，超出按 VARBINARY 处理
            L::Binary { len } => match len.unwrap_or(1) {
                n if n > 255 => mysql_varbinary(n),
                n => format!("BINARY({n})"),
            },
            L::VarBinary { len: Some(n) } => mysql_varbinary(*n),
            L::VarBinary { len: None } | L::Blob => "LONGBLOB".into(),
            L::Date => "DATE".into(),
            L::Time { precision } => with_precision(family, "TIME", *precision),
            L::Timestamp { tz: false, precision } => with_precision(family, "DATETIME", *precision),
            // 带时区的时间戳也用 DATETIME：MySQL 的 TIMESTAMP 只覆盖 1970–2038 年。
            // 值按其自身偏移的本地时间写入（与 literal.rs 和 MySQL 驱动一致）；是否先统一换算成 UTC
            // 是 M5（数据迁移）之前待定的决策。MySQL 原生 timestamp 映射回逻辑类型时仍是 tz: true。
            L::Timestamp { tz: true, precision } => with_precision(family, "DATETIME", *precision),
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
            L::Decimal { precision, scale } => decimal_native(family, *precision, *scale),
            L::Char { len } => with_len("CHAR", Some(len.unwrap_or(1))),
            L::VarChar { len: Some(n) } => format!("VARCHAR({n})"),
            L::VarChar { len: None } | L::Text | L::Other => "TEXT".into(),
            L::Binary { .. } | L::VarBinary { .. } | L::Blob => "BYTEA".into(),
            L::Date => "DATE".into(),
            L::Time { precision } => with_precision(family, "TIME", *precision),
            L::Timestamp { tz: false, precision } => with_precision(family, "TIMESTAMP", *precision),
            L::Timestamp { tz: true, precision } => {
                format!("{} WITH TIME ZONE", with_precision(family, "TIMESTAMP", *precision))
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
            L::Decimal { precision, scale } => decimal_native(family, *precision, *scale),
            // CHAR 上限 2000，超出按 VARCHAR2 处理
            L::Char { len } => match len.unwrap_or(1) {
                n if n > 2000 => oracle_varchar(n),
                n => format!("CHAR({n} CHAR)"),
            },
            L::VarChar { len: Some(n) } => oracle_varchar(*n),
            L::VarChar { len: None } | L::Text | L::Json | L::Other => "CLOB".into(),
            L::Binary { len: Some(n) } | L::VarBinary { len: Some(n) } if *n <= 2000 => format!("RAW({n})"),
            L::Binary { .. } | L::VarBinary { .. } | L::Blob => "BLOB".into(),
            L::Date => "DATE".into(),
            // Oracle 没有 TIME 类型，按 'HH24:MI:SS.FF6' 文本存储
            L::Time { .. } => "VARCHAR2(18)".into(),
            L::Timestamp { tz: false, precision } => with_precision(family, "TIMESTAMP", *precision),
            L::Timestamp { tz: true, precision } => {
                format!("{} WITH TIME ZONE", with_precision(family, "TIMESTAMP", *precision))
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
            L::Decimal { precision, scale } => decimal_native(family, *precision, *scale),
            // 与 VARCHAR 一样显式按字符计长；超过 8188 时按 VARCHAR 规则处理
            L::Char { len } => match len.unwrap_or(1) {
                n if n > 8188 => dameng_varchar(n),
                n => format!("CHAR({n} CHAR)"),
            },
            L::VarChar { len: Some(n) } => dameng_varchar(*n),
            L::VarChar { len: None } | L::Text => "TEXT".into(),
            // BINARY/VARBINARY 上限 8188 字节，超出改 BLOB
            L::Binary { len } => match len.unwrap_or(1) {
                n if n > 8188 => "BLOB".into(),
                n => format!("BINARY({n})"),
            },
            L::VarBinary { len: Some(n) } if *n > 8188 => "BLOB".into(),
            L::VarBinary { len: Some(n) } => format!("VARBINARY({n})"),
            L::VarBinary { len: None } | L::Blob => "BLOB".into(),
            L::Date => "DATE".into(),
            L::Time { precision } => with_precision(family, "TIME", *precision),
            L::Timestamp { tz: false, precision } => with_precision(family, "TIMESTAMP", *precision),
            L::Timestamp { tz: true, precision } => {
                format!("{} WITH TIME ZONE", with_precision(family, "TIMESTAMP", *precision))
            }
            L::Interval => "INTERVAL DAY TO SECOND".into(),
            L::Json | L::Other => "CLOB".into(),
            L::Uuid => "VARCHAR(36)".into(),
        },
        // 字符一律用 Unicode 的 N 类型：NCHAR/NVARCHAR 上限 4000 字符，超出用 NVARCHAR(MAX)；二进制上限 8000 字节
        Family::SqlServer => match logical {
            L::Bool => "BIT".into(),
            // TINYINT 无符号，有符号 8 位用 SMALLINT
            L::Int { bits } => match bits {
                0..=16 => "SMALLINT",
                17..=32 => "INT",
                _ => "BIGINT",
            }
            .into(),
            L::UInt { bits } => match bits {
                0..=8 => "TINYINT",
                9..=16 => "INT",
                17..=32 => "BIGINT",
                _ => "DECIMAL(20,0)",
            }
            .into(),
            L::Float { bits } => if *bits <= 32 { "REAL" } else { "FLOAT" }.into(),
            L::Decimal { precision, scale } => decimal_native(family, *precision, *scale),
            L::Char { len } => match len.unwrap_or(1) {
                n if n > 4000 => "NVARCHAR(MAX)".into(),
                n => format!("NCHAR({n})"),
            },
            L::VarChar { len: Some(n) } if *n <= 4000 => format!("NVARCHAR({n})"),
            L::VarChar { .. } | L::Text | L::Json | L::Other => "NVARCHAR(MAX)".into(),
            L::Binary { len } => match len.unwrap_or(1) {
                n if n > 8000 => "VARBINARY(MAX)".into(),
                n => format!("BINARY({n})"),
            },
            L::VarBinary { len: Some(n) } if *n <= 8000 => format!("VARBINARY({n})"),
            L::VarBinary { .. } | L::Blob => "VARBINARY(MAX)".into(),
            L::Date => "DATE".into(),
            L::Time { precision } => with_precision(family, "TIME", *precision),
            L::Timestamp { tz: false, precision } => with_precision(family, "DATETIME2", *precision),
            L::Timestamp { tz: true, precision } => with_precision(family, "DATETIMEOFFSET", *precision),
            L::Interval => "NVARCHAR(100)".into(),
            L::Uuid => "UNIQUEIDENTIFIER".into(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use LogicalType as L;

    const FAMILIES: [Family; 5] =
        [Family::MySql, Family::Postgres, Family::Oracle, Family::Dameng, Family::SqlServer];

    fn dec(p: Option<u32>, s: Option<i32>) -> L {
        L::Decimal { precision: p, scale: s }
    }

    fn ts(tz: bool, p: Option<u8>) -> L {
        L::Timestamp { tz, precision: p }
    }

    #[test]
    fn parses_type_text() {
        let t = parse_type("decimal(10, 2) unsigned zerofill");
        assert_eq!(
            (t.name.as_str(), t.args.clone(), t.suffix.as_str(), t.unsigned, t.array),
            ("DECIMAL", vec!["10".to_string(), "2".to_string()], "", true, false)
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

    #[test]
    fn parse_stops_at_clause_keywords() {
        let t = parse_type("varchar(255) character set utf8mb4 collate utf8mb4_bin");
        assert_eq!(
            (t.name.as_str(), t.args.clone(), t.suffix.as_str()),
            ("VARCHAR", vec!["255".to_string()], "")
        );
        let t = parse_type("datetime(6) on update current_timestamp");
        assert_eq!(
            (t.name.as_str(), t.args.clone(), t.suffix.as_str()),
            ("DATETIME", vec!["6".to_string()], "")
        );
        // CHARACTER 后面不是 SET 时是类型名的一部分
        assert_eq!(parse_type("character varying(10) not null").name, "CHARACTER VARYING");
        assert_eq!(parse_type("char(1) charset latin1").name, "CHAR");
        for tail in
            ["generated always as (a+b)", "default 5", "not null", "null", "auto_increment", "comment 'x'"]
        {
            let t = parse_type(&format!("int(11) unsigned {tail}"));
            assert_eq!(
                (t.name.as_str(), t.args.clone(), t.unsigned),
                ("INT", vec!["11".to_string()], true),
                "{tail}"
            );
        }
        // 子句在括号之前时，括号属于子句
        let t = parse_type("int default (0)");
        assert_eq!((t.name.as_str(), t.args.len()), ("INT", 0));
        // 时区修饰与子句同时出现
        let t = parse_type("timestamp(3) with time zone default now()");
        assert_eq!((t.name.as_str(), t.suffix.as_str()), ("TIMESTAMP", "WITH TIME ZONE"));
        assert_eq!(
            logical_type(Family::MySql, "varchar(255) character set utf8mb4"),
            L::VarChar { len: Some(255) }
        );
        assert_eq!(
            logical_type(Family::MySql, "datetime(6) on update current_timestamp"),
            ts(false, Some(6))
        );
    }

    #[test]
    fn parse_array_types() {
        for s in ["int[]", "int4[3]", "character varying(10)[]", "text[][]", "_int4", "_text"] {
            assert!(parse_type(s).array, "{s}");
            assert_eq!(logical_type(Family::Postgres, s), L::Other, "{s}");
        }
        assert_eq!(parse_type("character varying(10)[]").name, "CHARACTER VARYING");
        assert!(!parse_type("int4").array);
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
                ("decimal(10,2)", dec(Some(10), Some(2))),
                ("double", L::Float { bits: 64 }),
                ("float", L::Float { bits: 32 }),
                ("varchar(100)", L::VarChar { len: Some(100) }),
                ("char(2)", L::Char { len: Some(2) }),
                ("longtext", L::Text),
                ("varbinary(16)", L::VarBinary { len: Some(16) }),
                ("mediumblob", L::Blob),
                ("datetime(6)", ts(false, Some(6))),
                ("datetime", ts(false, Some(0))),
                ("timestamp", ts(true, Some(0))),
                ("timestamp(3)", ts(true, Some(3))),
                ("time(3)", L::Time { precision: Some(3) }),
                ("time", L::Time { precision: Some(0) }),
                ("year", L::Int { bits: 16 }),
                ("json", L::Json),
                ("enum('a','b')", L::Other),
                ("bit", L::Bool),
                ("bit(1)", L::Bool),
                ("bit(2)", L::Binary { len: Some(1) }),
                ("bit(8)", L::Binary { len: Some(1) }),
                ("bit(9)", L::Binary { len: Some(2) }),
                ("bit(64)", L::Binary { len: Some(8) }),
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
                (dec(Some(10), Some(2)), "DECIMAL(10,2)"),
                (dec(None, None), "DECIMAL(65,30)"),
                (L::VarChar { len: Some(100) }, "VARCHAR(100)"),
                (L::VarChar { len: None }, "LONGTEXT"),
                (ts(false, Some(6)), "DATETIME(6)"),
                (ts(true, None), "DATETIME"),
                (ts(true, Some(3)), "DATETIME(3)"),
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
                ("numeric(12,4)", dec(Some(12), Some(4))),
                ("numeric", dec(None, None)),
                ("character varying(50)", L::VarChar { len: Some(50) }),
                ("character(3)", L::Char { len: Some(3) }),
                ("text", L::Text),
                ("bytea", L::Blob),
                ("boolean", L::Bool),
                ("double precision", L::Float { bits: 64 }),
                ("real", L::Float { bits: 32 }),
                ("timestamp(3) without time zone", ts(false, Some(3))),
                ("timestamp", ts(false, Some(6))),
                ("timestamp with time zone", ts(true, Some(6))),
                ("timestamptz", ts(true, Some(6))),
                ("timestamptz(2)", ts(true, Some(2))),
                ("time", L::Time { precision: Some(6) }),
                ("time(3) without time zone", L::Time { precision: Some(3) }),
                ("time with time zone", L::Other),
                ("timetz", L::Other),
                ("bit(1)", L::Bool),
                ("bit", L::Bool),
                ("bit(8)", L::Other),
                ("bit varying(8)", L::Other),
                ("varbit", L::Other),
                ("interval day to second(3)", L::Interval),
                ("interval", L::Interval),
                ("interval year to month", L::Other),
                ("intervalx", L::Other),
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
                (ts(true, Some(6)), "TIMESTAMP(6) WITH TIME ZONE"),
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
                ("NUMBER(10,0)", dec(Some(10), Some(0))),
                ("NUMBER", dec(None, None)),
                ("NUMBER(*,0)", dec(None, Some(0))),
                ("NUMBER(5,-2)", dec(Some(5), Some(-2))),
                ("INTEGER", dec(Some(38), Some(0))),
                ("FLOAT", dec(None, None)),
                ("FLOAT(126)", dec(None, None)),
                ("DEC(10,2)", dec(Some(10), Some(2))),
                ("DECIMAL(8)", dec(Some(8), None)),
                ("NUMERIC(6,3)", dec(Some(6), Some(3))),
                // REAL / DOUBLE PRECISION 是 NUMBER 子类型，与 FLOAT 一样
                ("REAL", dec(None, None)),
                ("DOUBLE PRECISION", dec(None, None)),
                ("BINARY_DOUBLE", L::Float { bits: 64 }),
                ("VARCHAR2(100 CHAR)", L::VarChar { len: Some(100) }),
                ("NVARCHAR2(20)", L::VarChar { len: Some(20) }),
                ("CHAR(1)", L::Char { len: Some(1) }),
                ("CLOB", L::Text),
                ("LONG RAW", L::Blob),
                ("RAW(16)", L::VarBinary { len: Some(16) }),
                ("DATE", ts(false, Some(0))),
                ("TIMESTAMP", ts(false, Some(6))),
                ("TIMESTAMP(6)", ts(false, Some(6))),
                ("TIMESTAMP(9)", ts(false, Some(9))),
                ("TIMESTAMP(6) WITH LOCAL TIME ZONE", ts(true, Some(6))),
                ("TIMESTAMP WITH TIME ZONE", ts(true, Some(6))),
                ("INTERVAL YEAR(2) TO MONTH", L::Other),
                ("INTERVAL DAY(2) TO SECOND(6)", L::Interval),
                ("INTERVALX", L::Other),
                ("INTERVAL MONTH(2)", L::Other),
                ("BFILE", L::Other),
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
                (ts(false, None), "TIMESTAMP"),
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
                ("DEC(10,4)", dec(Some(10), Some(4))),
                ("NUMBER(8)", dec(Some(8), None)),
                ("DOUBLE", L::Float { bits: 64 }),
                ("VARCHAR(50)", L::VarChar { len: Some(50) }),
                ("VARCHAR(50 CHAR)", L::VarChar { len: Some(50) }),
                ("VARCHAR2(50)", L::VarChar { len: Some(50) }),
                ("NVARCHAR2(20)", L::VarChar { len: Some(20) }),
                ("NCHAR(4)", L::Char { len: Some(4) }),
                ("JSONB", L::Json),
                ("TEXT", L::Text),
                ("IMAGE", L::Blob),
                ("DATETIME(6)", ts(false, Some(6))),
                ("DATETIME", ts(false, Some(6))),
                ("TIMESTAMP(6) WITH TIME ZONE", ts(true, Some(6))),
                ("TIMESTAMP WITH TIME ZONE", ts(true, Some(6))),
                ("TIME(0)", L::Time { precision: Some(0) }),
                ("TIME", L::Time { precision: Some(6) }),
                ("TIME WITH TIME ZONE", L::Other),
                ("TIME(3) WITH TIME ZONE", L::Other),
                ("INTERVAL YEAR(2) TO MONTH", L::Other),
                ("INTERVAL DAY TO SECOND", L::Interval),
                ("ROWID", L::Other),
            ],
        );
        check_to_native(
            Family::Dameng,
            &[
                (L::Bool, "BIT"),
                (L::Int { bits: 64 }, "BIGINT"),
                (L::UInt { bits: 64 }, "DEC(20,0)"),
                (dec(Some(10), Some(2)), "DEC(10,2)"),
                (L::VarChar { len: None }, "TEXT"),
                (ts(true, Some(3)), "TIMESTAMP(3) WITH TIME ZONE"),
                (L::Json, "CLOB"),
            ],
        );
    }

    #[test]
    fn decimal_clamping() {
        let cases: &[(Family, L, &str)] = &[
            // 负小数位：Oracle 保留，其余库把整数位并入精度
            (Family::Oracle, dec(Some(5), Some(-2)), "NUMBER(5,-2)"),
            (Family::MySql, dec(Some(5), Some(-2)), "DECIMAL(7,0)"),
            (Family::Postgres, dec(Some(5), Some(-2)), "NUMERIC(7,0)"),
            (Family::Dameng, dec(Some(5), Some(-2)), "DEC(7,0)"),
            // 小数位大于精度
            (Family::Postgres, dec(Some(3), Some(5)), "NUMERIC(5,5)"),
            (Family::MySql, dec(Some(3), Some(5)), "DECIMAL(5,5)"),
            // 只有小数位
            (Family::MySql, dec(None, Some(0)), "DECIMAL(38,0)"),
            (Family::Postgres, dec(None, Some(2)), "NUMERIC(38,2)"),
            (Family::Oracle, dec(None, Some(2)), "NUMBER(38,2)"),
            (Family::Dameng, dec(None, Some(2)), "DEC(38,2)"),
            // 精度超上限
            (Family::Oracle, dec(Some(39), Some(2)), "NUMBER"),
            (Family::Oracle, dec(Some(65), Some(30)), "NUMBER"),
            (Family::Dameng, dec(Some(65), Some(30)), "DEC"),
            (Family::MySql, dec(Some(66), Some(40)), "DECIMAL(65,30)"),
            (Family::MySql, dec(Some(100), Some(10)), "DECIMAL(65,10)"),
            (Family::Postgres, dec(Some(1000), Some(10)), "NUMERIC(1000,10)"),
            (Family::Postgres, dec(Some(1001), Some(10)), "NUMERIC"),
            // MySQL 小数位上限 30
            (Family::MySql, dec(Some(65), Some(40)), "DECIMAL(65,30)"),
            (Family::MySql, dec(Some(50), Some(35)), "DECIMAL(50,30)"),
            // 无参数与只有精度
            (Family::MySql, dec(None, None), "DECIMAL(65,30)"),
            (Family::Oracle, dec(Some(10), None), "NUMBER(10)"),
        ];
        for (f, l, expected) in cases {
            assert_eq!(native_type(*f, l), *expected, "{f:?} {l:?}");
        }
    }

    #[test]
    fn time_precision_clamping() {
        for f in [Family::MySql, Family::Postgres, Family::Dameng] {
            assert_eq!(native_type(f, &L::Time { precision: Some(9) }), "TIME(6)", "{f:?}");
        }
        assert_eq!(native_type(Family::MySql, &ts(false, Some(9))), "DATETIME(6)");
        assert_eq!(native_type(Family::MySql, &ts(true, Some(9))), "DATETIME(6)");
        assert_eq!(native_type(Family::Postgres, &ts(false, Some(9))), "TIMESTAMP(6)");
        assert_eq!(native_type(Family::Postgres, &ts(true, Some(9))), "TIMESTAMP(6) WITH TIME ZONE");
        assert_eq!(native_type(Family::Dameng, &ts(false, Some(9))), "TIMESTAMP(6)");
        assert_eq!(native_type(Family::Dameng, &ts(true, Some(9))), "TIMESTAMP(6) WITH TIME ZONE");
        assert_eq!(native_type(Family::Oracle, &ts(false, Some(9))), "TIMESTAMP(9)");
        assert_eq!(native_type(Family::Oracle, &ts(true, Some(12))), "TIMESTAMP(9) WITH TIME ZONE");
        assert_eq!(native_type(Family::Oracle, &ts(false, Some(3))), "TIMESTAMP(3)");
    }

    #[test]
    fn length_clamping() {
        let cases: &[(Family, L, &str)] = &[
            // MySQL
            (Family::MySql, L::Char { len: Some(255) }, "CHAR(255)"),
            (Family::MySql, L::Char { len: Some(256) }, "VARCHAR(256)"),
            (Family::MySql, L::Char { len: Some(500) }, "VARCHAR(500)"),
            (Family::MySql, L::Char { len: Some(20000) }, "MEDIUMTEXT"),
            (Family::MySql, L::Char { len: None }, "CHAR(1)"),
            (Family::MySql, L::VarChar { len: Some(16383) }, "VARCHAR(16383)"),
            (Family::MySql, L::VarChar { len: Some(16384) }, "MEDIUMTEXT"),
            (Family::MySql, L::VarChar { len: None }, "LONGTEXT"),
            (Family::MySql, L::Binary { len: Some(255) }, "BINARY(255)"),
            (Family::MySql, L::Binary { len: Some(256) }, "VARBINARY(256)"),
            (Family::MySql, L::Binary { len: Some(70000) }, "LONGBLOB"),
            (Family::MySql, L::VarBinary { len: Some(65535) }, "VARBINARY(65535)"),
            (Family::MySql, L::VarBinary { len: Some(65536) }, "LONGBLOB"),
            // Oracle
            (Family::Oracle, L::Char { len: Some(2000) }, "CHAR(2000 CHAR)"),
            (Family::Oracle, L::Char { len: Some(2001) }, "VARCHAR2(2001 CHAR)"),
            (Family::Oracle, L::Char { len: Some(4001) }, "CLOB"),
            (Family::Oracle, L::VarChar { len: Some(4000) }, "VARCHAR2(4000 CHAR)"),
            (Family::Oracle, L::VarChar { len: Some(4001) }, "CLOB"),
            // 达梦
            (Family::Dameng, L::VarChar { len: Some(100) }, "VARCHAR(100 CHAR)"),
            (Family::Dameng, L::VarChar { len: Some(8188) }, "VARCHAR(8188 CHAR)"),
            (Family::Dameng, L::VarChar { len: Some(8189) }, "TEXT"),
            (Family::Dameng, L::VarBinary { len: Some(8188) }, "VARBINARY(8188)"),
            (Family::Dameng, L::VarBinary { len: Some(8189) }, "BLOB"),
            (Family::Dameng, L::Binary { len: Some(8188) }, "BINARY(8188)"),
            (Family::Dameng, L::Binary { len: Some(8189) }, "BLOB"),
            (Family::Dameng, L::Char { len: Some(10) }, "CHAR(10 CHAR)"),
            (Family::Dameng, L::Char { len: None }, "CHAR(1 CHAR)"),
            (Family::Dameng, L::Char { len: Some(8188) }, "CHAR(8188 CHAR)"),
            (Family::Dameng, L::Char { len: Some(8189) }, "TEXT"),
        ];
        for (f, l, expected) in cases {
            assert_eq!(native_type(*f, l), *expected, "{f:?} {l:?}");
        }
    }

    #[test]
    fn degenerate_sizes() {
        let cases: &[(Family, L, &str)] = &[
            // 精度 0 视为无限定
            (Family::MySql, dec(Some(0), None), "DECIMAL(65,30)"),
            (Family::Postgres, dec(Some(0), None), "NUMERIC"),
            (Family::Oracle, dec(Some(0), None), "NUMBER"),
            (Family::Dameng, dec(Some(0), None), "DEC"),
            (Family::Oracle, dec(Some(0), Some(2)), "NUMBER(38,2)"),
            // 长度 0 视为 1
            (Family::MySql, L::Char { len: Some(0) }, "CHAR(1)"),
            (Family::MySql, L::VarChar { len: Some(0) }, "VARCHAR(1)"),
            (Family::MySql, L::Binary { len: Some(0) }, "BINARY(1)"),
            (Family::MySql, L::VarBinary { len: Some(0) }, "VARBINARY(1)"),
            (Family::Postgres, L::Char { len: Some(0) }, "CHAR(1)"),
            (Family::Postgres, L::VarChar { len: Some(0) }, "VARCHAR(1)"),
            (Family::Oracle, L::Char { len: Some(0) }, "CHAR(1 CHAR)"),
            (Family::Oracle, L::VarChar { len: Some(0) }, "VARCHAR2(1 CHAR)"),
            (Family::Oracle, L::VarBinary { len: Some(0) }, "RAW(1)"),
            (Family::Oracle, L::Binary { len: Some(0) }, "RAW(1)"),
            (Family::Dameng, L::Char { len: Some(0) }, "CHAR(1 CHAR)"),
            (Family::Dameng, L::VarChar { len: Some(0) }, "VARCHAR(1 CHAR)"),
            (Family::Dameng, L::Binary { len: Some(0) }, "BINARY(1)"),
            (Family::Dameng, L::VarBinary { len: Some(0) }, "VARBINARY(1)"),
            // Oracle 小数位范围 -84..=127
            (Family::Oracle, dec(Some(10), Some(-100)), "NUMBER(10,-84)"),
            (Family::Oracle, dec(Some(10), Some(-84)), "NUMBER(10,-84)"),
            (Family::Oracle, dec(Some(38), Some(200)), "NUMBER"),
            (Family::Oracle, dec(None, Some(i32::MIN)), "NUMBER(38,-84)"),
        ];
        for (f, l, expected) in cases {
            assert_eq!(native_type(*f, l), *expected, "{f:?} {l:?}");
        }
    }

    #[test]
    fn round_trip_per_family() {
        // (原生类型, 经逻辑类型回到同库后的原生类型)；两者不同的行是有意的归一化
        let cases: &[(Family, &[(&str, &str)])] = &[
            (
                Family::MySql,
                &[
                    ("tinyint(1)", "TINYINT(1)"),
                    ("tinyint", "TINYINT"),
                    ("smallint", "SMALLINT"),
                    ("mediumint", "MEDIUMINT"),
                    ("int", "INT"),
                    ("int unsigned", "INT UNSIGNED"),
                    ("bigint", "BIGINT"),
                    ("bigint unsigned", "BIGINT UNSIGNED"),
                    ("decimal(10,2)", "DECIMAL(10,2)"),
                    ("decimal(65,30)", "DECIMAL(65,30)"),
                    ("float", "FLOAT"),
                    ("double", "DOUBLE"),
                    ("char(2)", "CHAR(2)"),
                    ("varchar(100)", "VARCHAR(100)"),
                    ("varchar(255) character set utf8mb4", "VARCHAR(255)"),
                    ("binary(4)", "BINARY(4)"),
                    ("varbinary(16)", "VARBINARY(16)"),
                    ("longtext", "LONGTEXT"),
                    ("text", "LONGTEXT"),
                    ("longblob", "LONGBLOB"),
                    ("mediumblob", "LONGBLOB"),
                    ("date", "DATE"),
                    ("time", "TIME(0)"),
                    ("time(3)", "TIME(3)"),
                    ("datetime", "DATETIME(0)"),
                    ("datetime(6)", "DATETIME(6)"),
                    // TIMESTAMP 回到 MySQL 时落成 DATETIME（见 native_type 的注释）
                    ("timestamp", "DATETIME(0)"),
                    ("json", "JSON"),
                    ("year", "SMALLINT"),
                    ("bit(1)", "TINYINT(1)"),
                    ("bit(16)", "BINARY(2)"),
                ],
            ),
            (
                Family::Postgres,
                &[
                    ("smallint", "SMALLINT"),
                    ("integer", "INTEGER"),
                    ("bigint", "BIGINT"),
                    ("real", "REAL"),
                    ("double precision", "DOUBLE PRECISION"),
                    ("numeric(12,4)", "NUMERIC(12,4)"),
                    ("numeric", "NUMERIC"),
                    ("character varying(50)", "VARCHAR(50)"),
                    ("character(3)", "CHAR(3)"),
                    ("text", "TEXT"),
                    ("bytea", "BYTEA"),
                    ("boolean", "BOOLEAN"),
                    ("date", "DATE"),
                    ("time without time zone", "TIME(6)"),
                    ("time(3)", "TIME(3)"),
                    ("timestamp", "TIMESTAMP(6)"),
                    ("timestamp(3) without time zone", "TIMESTAMP(3)"),
                    ("timestamp with time zone", "TIMESTAMP(6) WITH TIME ZONE"),
                    ("timestamptz(0)", "TIMESTAMP(0) WITH TIME ZONE"),
                    ("interval", "INTERVAL"),
                    ("json", "JSONB"),
                    ("jsonb", "JSONB"),
                    ("uuid", "UUID"),
                    ("bit(1)", "BOOLEAN"),
                    // 数组和其他未知类型回到 TEXT
                    ("integer[]", "TEXT"),
                    ("point", "TEXT"),
                ],
            ),
            (
                Family::Oracle,
                &[
                    ("NUMBER(10,0)", "NUMBER(10,0)"),
                    ("NUMBER", "NUMBER"),
                    ("NUMBER(*,0)", "NUMBER(38,0)"),
                    ("NUMBER(5,-2)", "NUMBER(5,-2)"),
                    ("INTEGER", "NUMBER(38,0)"),
                    ("FLOAT", "NUMBER"),
                    ("REAL", "NUMBER"),
                    ("DOUBLE PRECISION", "NUMBER"),
                    ("BINARY_FLOAT", "BINARY_FLOAT"),
                    ("BINARY_DOUBLE", "BINARY_DOUBLE"),
                    ("VARCHAR2(100 CHAR)", "VARCHAR2(100 CHAR)"),
                    ("NVARCHAR2(20)", "VARCHAR2(20 CHAR)"),
                    ("CHAR(1)", "CHAR(1 CHAR)"),
                    ("CLOB", "CLOB"),
                    ("BLOB", "BLOB"),
                    ("RAW(16)", "RAW(16)"),
                    // DATE 带时分秒，回到 Oracle 时落成 TIMESTAMP(0)
                    ("DATE", "TIMESTAMP(0)"),
                    ("TIMESTAMP", "TIMESTAMP(6)"),
                    ("TIMESTAMP(9)", "TIMESTAMP(9)"),
                    ("TIMESTAMP(6) WITH TIME ZONE", "TIMESTAMP(6) WITH TIME ZONE"),
                    ("TIMESTAMP(6) WITH LOCAL TIME ZONE", "TIMESTAMP(6) WITH TIME ZONE"),
                    ("INTERVAL DAY(2) TO SECOND(6)", "INTERVAL DAY(9) TO SECOND(6)"),
                    ("BOOLEAN", "NUMBER(1)"),
                    ("XMLTYPE", "CLOB"),
                ],
            ),
            (
                Family::Dameng,
                &[
                    ("TINYINT", "TINYINT"),
                    ("SMALLINT", "SMALLINT"),
                    ("INT", "INT"),
                    ("BIGINT", "BIGINT"),
                    ("BIT", "BIT"),
                    ("DEC(10,4)", "DEC(10,4)"),
                    ("NUMBER(8)", "DEC(8)"),
                    ("REAL", "REAL"),
                    ("DOUBLE", "DOUBLE"),
                    ("CHAR(3)", "CHAR(3 CHAR)"),
                    ("VARCHAR(50)", "VARCHAR(50 CHAR)"),
                    ("VARCHAR(50 CHAR)", "VARCHAR(50 CHAR)"),
                    ("TEXT", "TEXT"),
                    ("CLOB", "TEXT"),
                    ("BLOB", "BLOB"),
                    ("BINARY(4)", "BINARY(4)"),
                    ("VARBINARY(16)", "VARBINARY(16)"),
                    ("DATE", "DATE"),
                    ("TIME(0)", "TIME(0)"),
                    ("TIME", "TIME(6)"),
                    ("TIMESTAMP(6)", "TIMESTAMP(6)"),
                    ("DATETIME", "TIMESTAMP(6)"),
                    ("TIMESTAMP(6) WITH TIME ZONE", "TIMESTAMP(6) WITH TIME ZONE"),
                ],
            ),
        ];
        for (f, list) in cases {
            for (native, expected) in *list {
                let logical = logical_type(*f, native);
                assert_eq!(native_type(*f, &logical), *expected, "{f:?} {native} -> {logical:?}");
            }
        }
    }

    #[test]
    fn cross_family_mapping() {
        let cases: &[(Family, &str, Family, &str)] = &[
            (Family::Postgres, "timestamp", Family::MySql, "DATETIME(6)"),
            (Family::Postgres, "timestamptz", Family::MySql, "DATETIME(6)"),
            (Family::MySql, "decimal(65,30)", Family::Oracle, "NUMBER"),
            (Family::Oracle, "NUMBER(*,0)", Family::MySql, "DECIMAL(38,0)"),
            (Family::Oracle, "NUMBER(5,-2)", Family::MySql, "DECIMAL(7,0)"),
            (Family::Oracle, "NUMBER(3,5)", Family::Postgres, "NUMERIC(5,5)"),
            (Family::Oracle, "TIMESTAMP(9)", Family::MySql, "DATETIME(6)"),
            (Family::MySql, "varchar(100)", Family::Dameng, "VARCHAR(100 CHAR)"),
            (Family::MySql, "varchar(20000)", Family::Dameng, "TEXT"),
            (Family::MySql, "char(10)", Family::Postgres, "CHAR(10)"),
            (Family::MySql, "datetime", Family::Postgres, "TIMESTAMP(0)"),
            (Family::MySql, "timestamp", Family::Postgres, "TIMESTAMP(0) WITH TIME ZONE"),
            (Family::MySql, "tinyint(1)", Family::Oracle, "NUMBER(1)"),
            (Family::MySql, "bit(16)", Family::Postgres, "BYTEA"),
            (Family::Postgres, "timetz", Family::MySql, "LONGTEXT"),
            (Family::Postgres, "interval year to month", Family::Oracle, "CLOB"),
            (Family::Oracle, "DATE", Family::MySql, "DATETIME(0)"),
            (Family::Oracle, "FLOAT", Family::Postgres, "NUMERIC"),
            (Family::Oracle, "VARCHAR2(4000 CHAR)", Family::MySql, "VARCHAR(4000)"),
            (Family::Dameng, "TIMESTAMP(6) WITH TIME ZONE", Family::MySql, "DATETIME(6)"),
        ];
        for (from, native, to, expected) in cases {
            let logical = logical_type(*from, native);
            assert_eq!(native_type(*to, &logical), *expected, "{from:?} {native} -> {to:?}");
        }
        assert_eq!(native_type(Family::MySql, &L::Char { len: Some(500) }), "VARCHAR(500)");
    }

    #[test]
    fn sqlserver_round_trip() {
        let f = Family::SqlServer;
        for (native, back) in [
            ("bit", "BIT"),
            ("tinyint", "TINYINT"),
            ("smallint", "SMALLINT"),
            ("int", "INT"),
            ("bigint", "BIGINT"),
            ("real", "REAL"),
            ("float", "FLOAT"),
            ("decimal(10,2)", "DECIMAL(10,2)"),
            ("money", "DECIMAL(19,4)"),
            ("nchar(10)", "NCHAR(10)"),
            // 非 Unicode 的字符类型统一成 N 类型
            ("varchar(100)", "NVARCHAR(100)"),
            ("nvarchar(4000)", "NVARCHAR(4000)"),
            ("nvarchar(max)", "NVARCHAR(MAX)"),
            ("varchar(max)", "NVARCHAR(MAX)"),
            ("ntext", "NVARCHAR(MAX)"),
            ("binary(16)", "BINARY(16)"),
            ("varbinary(max)", "VARBINARY(MAX)"),
            ("image", "VARBINARY(MAX)"),
            ("date", "DATE"),
            ("time(3)", "TIME(3)"),
            ("datetime2(7)", "DATETIME2(7)"),
            ("datetime", "DATETIME2(3)"),
            ("smalldatetime", "DATETIME2(0)"),
            ("datetimeoffset(7)", "DATETIMEOFFSET(7)"),
            ("uniqueidentifier", "UNIQUEIDENTIFIER"),
        ] {
            assert_eq!(native_type(f, &logical_type(f, native)), back, "{native}");
        }
        assert_eq!(logical_type(f, "geography"), L::Other);
        assert_eq!(logical_type(f, "rowversion"), L::Other);
        // 有符号 8 位放不进 TINYINT
        assert_eq!(native_type(f, &L::Int { bits: 8 }), "SMALLINT");
        assert_eq!(native_type(f, &L::VarChar { len: Some(5000) }), "NVARCHAR(MAX)");
        assert_eq!(native_type(f, &dec(None, None)), "DECIMAL(38,10)");
        assert_eq!(native_type(f, &ts(true, Some(9))), "DATETIMEOFFSET(7)");
        // 从 PG 来的 timestamp 到 SQL Server 是 DATETIME2(6)
        assert_eq!(native_type(f, &logical_type(Family::Postgres, "timestamp")), "DATETIME2(6)");
    }

    #[test]
    fn malformed_input_never_panics() {
        let inputs = [
            "",
            "(",
            ")",
            "(((",
            "varchar(",
            "decimal(10,2",
            "数据类型(10)",
            "number(-1)",
            "timestamp(999)",
            "int[]",
            "_int4",
            "WITH TIME ZONE",
            "unsigned",
            "interval",
            "bit(",
            "]",
            "[",
            "[]",
            "int[",
            "enum('a",
            "varchar(4294967296)",
            "decimal(99999999999,99999999999)",
            "number(5,-2147483648)",
            "时间戳(6) with time zone",
            "character set",
            "on",
        ];
        for s in inputs {
            let _ = parse_type(s);
            for f in FAMILIES {
                let l = logical_type(f, s);
                // 逻辑类型再生成原生文本也不能 panic
                for g in FAMILIES {
                    let _ = native_type(g, &l);
                }
            }
        }
        // 极端数值
        for f in FAMILIES {
            for l in [
                dec(Some(u32::MAX), Some(i32::MIN)),
                dec(Some(u32::MAX), Some(i32::MAX)),
                dec(None, Some(i32::MIN)),
                L::Char { len: Some(u32::MAX) },
                L::VarChar { len: Some(u32::MAX) },
                L::Binary { len: Some(u32::MAX) },
                L::VarBinary { len: Some(u32::MAX) },
                L::Time { precision: Some(u8::MAX) },
                ts(true, Some(u8::MAX)),
            ] {
                let _ = native_type(f, &l);
            }
        }
        assert_eq!(logical_type(Family::Oracle, "timestamp(999)"), ts(false, Some(6)));
        assert_eq!(logical_type(Family::Postgres, "bit("), L::Bool);
    }
}
