//! 值 → SQL 字面量（用于"生成 SQL"和脚本导出，执行用户 SQL 时一律走参数绑定），以及分页包装。

use dblens_core::{Error, Result, Value};

use crate::Family;

/// 把值格式化为该方言的 SQL 字面量。字符串一律转义；无法安全表示的值返回错误。
pub fn literal(family: Family, v: &Value) -> Result<String> {
    Ok(match v {
        Value::Null => "NULL".to_string(),
        Value::Bool(b) => match (family, b) {
            (Family::MySql | Family::Postgres, true) => "TRUE".to_string(),
            (Family::MySql | Family::Postgres, false) => "FALSE".to_string(),
            (_, true) => "1".to_string(),
            (_, false) => "0".to_string(),
        },
        Value::Int(i) => i.to_string(),
        Value::UInt(u) => u.to_string(),
        Value::Float(f) => float_literal(family, *f)?,
        Value::Decimal(s) => decimal_literal(family, s)?,
        Value::String(s) | Value::Interval(s) => string_literal(family, s)?,
        Value::Json(j) => string_literal(family, &j.to_string())?,
        Value::Uuid(u) => string_literal(family, &u.to_string())?,
        Value::Bytes(b) => bytes_literal(family, b)?,
        Value::Date(d) => format!("DATE '{}'", d.format("%Y-%m-%d")),
        Value::Time(t) => {
            let text = t.format("%H:%M:%S%.f").to_string();
            // Oracle 没有 TIME 类型，按字符串写出
            if family == Family::Oracle { string_literal(family, &text)? } else { format!("TIME '{text}'") }
        }
        Value::DateTime(dt) => format!("TIMESTAMP '{}'", dt.format("%Y-%m-%d %H:%M:%S%.f")),
        Value::DateTimeTz(dt) => match family {
            // MySQL 没有带时区的类型，按该值自身时区的本地时间写出
            Family::MySql => format!("TIMESTAMP '{}'", dt.naive_local().format("%Y-%m-%d %H:%M:%S%.f")),
            Family::Postgres => format!("TIMESTAMPTZ '{}'", dt.format("%Y-%m-%d %H:%M:%S%.f%:z")),
            Family::Oracle | Family::Dameng => {
                format!("TIMESTAMP '{}'", dt.format("%Y-%m-%d %H:%M:%S%.f %:z"))
            }
        },
        // 错误信息只报类型，不回显值本身（大字段或未知类型的文本可能很长或很敏感）
        Value::Lob(_) => return Err(Error::internal("LOB 预览值不能生成 SQL 字面量")),
        Value::Other { type_name, .. } => {
            return Err(Error::internal(format!("该类型（{type_name}）的值不能生成 SQL 字面量")));
        }
    })
}

fn float_literal(family: Family, f: f64) -> Result<String> {
    if f.is_nan() {
        return match family {
            Family::Postgres => Ok("'NaN'::float8".to_string()),
            Family::Oracle => Ok("BINARY_DOUBLE_NAN".to_string()),
            Family::MySql | Family::Dameng => Err(Error::internal("该方言不支持 NaN 的 SQL 字面量")),
        };
    }
    if f.is_infinite() {
        return match (family, f > 0.0) {
            (Family::Postgres, true) => Ok("'Infinity'::float8".to_string()),
            (Family::Postgres, false) => Ok("'-Infinity'::float8".to_string()),
            (Family::Oracle, true) => Ok("BINARY_DOUBLE_INFINITY".to_string()),
            (Family::Oracle, false) => Ok("-BINARY_DOUBLE_INFINITY".to_string()),
            (Family::MySql | Family::Dameng, _) => Err(Error::internal("该方言不支持无穷大的 SQL 字面量")),
        };
    }
    // 普通区间用十进制写法，避免指数形式在各库之间的解析差异；其余用科学计数法保留精度
    let plain = f == 0.0 || (1e-6..1e15).contains(&f.abs());
    Ok(if plain { f.to_string() } else { format!("{f:e}") })
}

fn decimal_literal(family: Family, s: &str) -> Result<String> {
    if is_decimal_text(s) {
        return Ok(s.to_string());
    }
    if family == Family::Postgres && matches!(s, "NaN" | "Infinity" | "-Infinity") {
        return Ok(format!("'{s}'::numeric"));
    }
    Err(Error::internal(format!("不是有效的数字文本，无法生成字面量：{s}")))
}

/// 手写校验 `[+-]?(\d+(\.\d*)?|\.\d+)([eE][+-]?\d+)?`，只接受 ASCII 数字。
fn is_decimal_text(s: &str) -> bool {
    let body = s.strip_prefix('+').or_else(|| s.strip_prefix('-')).unwrap_or(s);
    let (mantissa, exponent) = match body.find(['e', 'E']) {
        Some(i) => (&body[..i], Some(&body[i + 1..])),
        None => (body, None),
    };
    if let Some(e) = exponent {
        let digits = e.strip_prefix('+').or_else(|| e.strip_prefix('-')).unwrap_or(e);
        if digits.is_empty() || !digits.bytes().all(|c| c.is_ascii_digit()) {
            return false;
        }
    }
    let (int, frac) = match mantissa.split_once('.') {
        Some((i, f)) => (i, Some(f)),
        None => (mantissa, None),
    };
    let frac_ok = frac.is_none_or(|f| f.bytes().all(|c| c.is_ascii_digit()));
    let digit_count = int.len() + frac.map_or(0, str::len);
    digit_count > 0 && int.bytes().all(|c| c.is_ascii_digit()) && frac_ok
}

/// Oracle 字符串字面量在 SQL 中的上限（字节）。
const ORACLE_LITERAL_MAX: usize = 4000;

/// 字符串字面量。MySQL 含反斜杠或 NUL 时用十六进制 + utf8mb4 形式，不受 sql_mode 影响；
/// PostgreSQL 含反斜杠时用 E'' 形式；Oracle 超过 4000 字节时拆成 TO_CLOB 拼接；NUL 在非 MySQL 方言中报错。
fn string_literal(family: Family, s: &str) -> Result<String> {
    if s.contains('\0') && family != Family::MySql {
        return Err(Error::internal("字符串含 NUL 字符，无法生成该方言的字面量"));
    }
    Ok(match family {
        Family::MySql if s.contains('\\') || s.contains('\0') => {
            let hex: String = s.bytes().map(|b| format!("{b:02X}")).collect();
            format!("_utf8mb4 X'{hex}'")
        }
        Family::MySql | Family::Dameng => quoted(s),
        Family::Postgres if s.contains('\\') => {
            format!("E'{}'", s.replace('\\', "\\\\").replace('\'', "''"))
        }
        Family::Postgres => quoted(s),
        Family::Oracle => {
            let chunks = oracle_chunks(s, ORACLE_LITERAL_MAX);
            if let [only] = chunks.as_slice() {
                format!("'{only}'")
            } else {
                chunks.iter().map(|c| format!("TO_CLOB('{c}')")).collect::<Vec<_>>().join("||")
            }
        }
    })
}

fn quoted(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

/// 按字符边界切分字符串，返回每块已转义的内容；每块转义后不超过 `max` 字节，`''` 不会被拆开。
fn oracle_chunks(s: &str, max: usize) -> Vec<String> {
    let mut chunks = vec![String::new()];
    let mut len = 0;
    for c in s.chars() {
        let n = if c == '\'' { 2 } else { c.len_utf8() };
        if len + n > max {
            chunks.push(String::new());
            len = 0;
        }
        let cur = chunks.last_mut().expect("至少有一块");
        if c == '\'' {
            cur.push_str("''");
        } else {
            cur.push(c);
        }
        len += n;
    }
    chunks
}

/// Oracle 的 HEXTORAW 字面量上限对应 4000 个十六进制字符，即 2000 字节。
const ORACLE_RAW_MAX: usize = 2000;

fn bytes_literal(family: Family, b: &[u8]) -> Result<String> {
    let hex: String = b.iter().map(|x| format!("{x:02X}")).collect();
    Ok(match family {
        Family::MySql => format!("X'{hex}'"),
        Family::Postgres => format!("decode('{hex}','hex')"),
        Family::Oracle => {
            if b.len() > ORACLE_RAW_MAX {
                return Err(Error::internal("超过 Oracle 字面量长度上限（2000 字节），请改用参数绑定"));
            }
            format!("HEXTORAW('{hex}')")
        }
        // 达梦的 0x 字面量不能为空，空值写成空字符串由数据库隐式转换
        Family::Dameng if b.is_empty() => "''".to_string(),
        Family::Dameng => format!("0x{hex}"),
    })
}

/// 给单条 SELECT/WITH 查询追加分页子句（不做子查询包装）。
/// 仅适用于自身不含 LIMIT/FETCH 的查询。
/// 子句另起一行，原查询末尾的行注释不会吞掉它。
/// 采用追加而非包装：允许重复列名，ORDER BY 仍然有效（MariaDB 会忽略派生表内的 ORDER BY）。
/// Oracle 需要 12c 及以上；11g 的 ROWNUM 分页尚未实现。
pub fn paginate(family: Family, sql: &str, offset: u64, limit: u64) -> String {
    let inner = sql.trim_start().trim_end_matches(|c: char| c == ';' || c.is_whitespace());
    match family {
        Family::Oracle => format!("{inner}\nOFFSET {offset} ROWS FETCH NEXT {limit} ROWS ONLY"),
        Family::MySql | Family::Postgres | Family::Dameng => {
            format!("{inner}\nLIMIT {limit} OFFSET {offset}")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{FixedOffset, NaiveDate, NaiveTime, TimeZone};

    fn lit(f: Family, v: Value) -> String {
        literal(f, &v).unwrap()
    }

    #[test]
    fn scalars() {
        assert_eq!(lit(Family::MySql, Value::Null), "NULL");
        assert_eq!(lit(Family::Postgres, Value::Bool(true)), "TRUE");
        assert_eq!(lit(Family::Oracle, Value::Bool(true)), "1");
        assert_eq!(lit(Family::Dameng, Value::Bool(false)), "0");
        assert_eq!(lit(Family::MySql, Value::Int(-7)), "-7");
        assert_eq!(lit(Family::MySql, Value::UInt(u64::MAX)), "18446744073709551615");
        assert_eq!(lit(Family::MySql, Value::Float(1.5)), "1.5");
        assert_eq!(lit(Family::Oracle, Value::Decimal("-123.4500".into())), "-123.4500");
        assert!(literal(Family::MySql, &Value::Float(f64::NAN)).is_err());
        // 不是数字的 Decimal 文本不能原样拼进 SQL
        assert!(literal(Family::MySql, &Value::Decimal("1; DROP TABLE t".into())).is_err());
        assert_eq!(lit(Family::Postgres, Value::Decimal("NaN".into())), "'NaN'::numeric");
        assert!(literal(Family::Oracle, &Value::Decimal("NaN".into())).is_err());
    }

    #[test]
    fn strings_are_escaped() {
        assert_eq!(lit(Family::Postgres, Value::String("it's".into())), "'it''s'");
        assert_eq!(lit(Family::MySql, Value::String("a\\b'c".into())), "_utf8mb4 X'615C622763'");
        assert_eq!(lit(Family::Oracle, Value::String("a\\b".into())), "'a\\b'");
        assert_eq!(lit(Family::Dameng, Value::String("数洞".into())), "'数洞'");
        assert_eq!(lit(Family::MySql, Value::Json(serde_json::json!({"a": "x'y"}))), "'{\"a\":\"x''y\"}'");
    }

    #[test]
    fn bytes() {
        let v = Value::Bytes(vec![0x01, 0xab]);
        assert_eq!(lit(Family::MySql, v.clone()), "X'01AB'");
        assert_eq!(lit(Family::Postgres, v.clone()), "decode('01AB','hex')");
        assert_eq!(lit(Family::Oracle, v.clone()), "HEXTORAW('01AB')");
        assert_eq!(lit(Family::Dameng, v), "0x01AB");
        assert_eq!(lit(Family::Dameng, Value::Bytes(vec![])), "''");
    }

    #[test]
    fn temporal() {
        let d = NaiveDate::from_ymd_opt(2024, 2, 29).unwrap();
        assert_eq!(lit(Family::MySql, Value::Date(d)), "DATE '2024-02-29'");
        let t = NaiveTime::from_hms_micro_opt(13, 45, 30, 123456).unwrap();
        assert_eq!(lit(Family::Postgres, Value::Time(t)), "TIME '13:45:30.123456'");
        assert_eq!(lit(Family::Oracle, Value::Time(t)), "'13:45:30.123456'");
        let dt = d.and_hms_opt(1, 2, 3).unwrap();
        assert_eq!(lit(Family::Dameng, Value::DateTime(dt)), "TIMESTAMP '2024-02-29 01:02:03'");
        let tz = FixedOffset::east_opt(8 * 3600).unwrap().with_ymd_and_hms(2024, 2, 29, 13, 45, 30).unwrap();
        assert_eq!(lit(Family::Postgres, Value::DateTimeTz(tz)), "TIMESTAMPTZ '2024-02-29 13:45:30+08:00'");
        assert_eq!(lit(Family::Oracle, Value::DateTimeTz(tz)), "TIMESTAMP '2024-02-29 13:45:30 +08:00'");
        assert_eq!(lit(Family::MySql, Value::DateTimeTz(tz)), "TIMESTAMP '2024-02-29 13:45:30'");
    }

    #[test]
    fn unsupported_values() {
        let lob = Value::Lob(dblens_core::LobPreview {
            binary: false,
            preview: "a".into(),
            total_len: 9,
            truncated: true,
        });
        assert!(literal(Family::MySql, &lob).is_err());
        assert!(literal(Family::MySql, &Value::Other { type_name: "x".into(), text: "y".into() }).is_err());
    }

    /// 解析 Oracle 的 TO_CLOB 拼接形式，返回每块的转义内容（引号仍是 '' 形式）
    fn clob_chunks(out: &str) -> Vec<&str> {
        out.split("||")
            .map(|p| {
                p.strip_prefix("TO_CLOB('").and_then(|x| x.strip_suffix("')")).expect("TO_CLOB 块格式错误")
            })
            .collect()
    }

    /// 还原 Oracle TO_CLOB 拼接形式对应的原始字符串
    fn unescape_clob(out: &str) -> String {
        clob_chunks(out).concat().replace("''", "'")
    }

    #[test]
    fn decimal_text_is_validated() {
        for ok in ["-1.5", "+3", ".5", "5.", "1e10", "1.5E-3", "-123.4500"] {
            assert_eq!(lit(Family::MySql, Value::Decimal(ok.into())), ok);
        }
        for bad in ["--1", "1--", "1-1", "5-", "1.2.3", "1e", "e5", ".", "", "1e+", "1e5.5", "1 "] {
            assert!(literal(Family::MySql, &Value::Decimal(bad.into())).is_err(), "应拒绝：{bad:?}");
        }
    }

    #[test]
    fn mysql_strings_with_backslash_use_hex_charset_form() {
        assert_eq!(lit(Family::MySql, Value::String("a\\b".into())), "_utf8mb4 X'615C62'");
        assert_eq!(lit(Family::MySql, Value::String("it's".into())), "'it''s'");
        assert_eq!(lit(Family::MySql, Value::String("a\0b".into())), "_utf8mb4 X'610062'");
    }

    #[test]
    fn postgres_backslash_uses_escape_string() {
        assert_eq!(lit(Family::Postgres, Value::String("a\\b'c".into())), "E'a\\\\b''c'");
        assert_eq!(lit(Family::Postgres, Value::String("it's".into())), "'it''s'");
    }

    #[test]
    fn nul_in_string_is_rejected_outside_mysql() {
        for f in [Family::Postgres, Family::Oracle, Family::Dameng] {
            assert!(literal(f, &Value::String("a\0b".into())).is_err(), "{f:?}");
        }
    }

    #[test]
    fn oracle_long_strings_are_chunked() {
        // 4000 个 ASCII 字符：转义后正好 4000 字节，保持普通字面量
        let exact = "a".repeat(4000);
        assert_eq!(lit(Family::Oracle, Value::String(exact.clone())), format!("'{exact}'"));

        // 4001 个字符：拆成两块 TO_CLOB
        let two = "a".repeat(4001);
        let out = lit(Family::Oracle, Value::String(two.clone()));
        assert_eq!(clob_chunks(&out).len(), 2);
        assert!(clob_chunks(&out).iter().all(|p| p.len() <= 4000));
        assert_eq!(unescape_clob(&out), two);

        // 1500 个"中"共 4500 字节，每块转义后不超过 4000 字节
        let cjk = "中".repeat(1500);
        let out = lit(Family::Oracle, Value::String(cjk.clone()));
        assert!(clob_chunks(&out).len() >= 2);
        assert!(clob_chunks(&out).iter().all(|p| p.len() <= 4000));
        assert_eq!(unescape_clob(&out), cjk);

        // 2001 个单引号：转义后每个占 2 字节，块边界不能拆开 ''
        let quotes = "'".repeat(2001);
        let out = lit(Family::Oracle, Value::String(quotes.clone()));
        assert!(clob_chunks(&out).len() >= 2);
        assert!(clob_chunks(&out).iter().all(|p| p.len() <= 4000 && p.len() % 2 == 0));
        assert_eq!(unescape_clob(&out), quotes);
    }

    #[test]
    fn bytes_per_family() {
        assert_eq!(lit(Family::Postgres, Value::Bytes(vec![])), "decode('','hex')");
        let err = literal(Family::Oracle, &Value::Bytes(vec![0; 2001])).unwrap_err().to_string();
        assert!(err.contains("2000 字节"), "{err}");
        assert!(literal(Family::Oracle, &Value::Bytes(vec![0; 2000])).is_ok());
    }

    #[test]
    fn floats() {
        assert_eq!(lit(Family::MySql, Value::Float(0.5)), "0.5");
        assert_eq!(lit(Family::MySql, Value::Float(1e300)), "1e300");
        assert_eq!(lit(Family::MySql, Value::Float(5e-324)), "5e-324");
        assert_eq!(lit(Family::MySql, Value::Float(1e15)), "1e15");
        assert_eq!(lit(Family::MySql, Value::Float(123456.789)), "123456.789");
        assert_eq!(lit(Family::Postgres, Value::Float(f64::NAN)), "'NaN'::float8");
        assert_eq!(lit(Family::Postgres, Value::Float(f64::INFINITY)), "'Infinity'::float8");
        assert_eq!(lit(Family::Postgres, Value::Float(f64::NEG_INFINITY)), "'-Infinity'::float8");
        assert_eq!(lit(Family::Oracle, Value::Float(f64::NAN)), "BINARY_DOUBLE_NAN");
        assert_eq!(lit(Family::Oracle, Value::Float(f64::INFINITY)), "BINARY_DOUBLE_INFINITY");
        assert_eq!(lit(Family::Oracle, Value::Float(f64::NEG_INFINITY)), "-BINARY_DOUBLE_INFINITY");
        assert!(literal(Family::MySql, &Value::Float(f64::NAN)).is_err());
    }

    #[test]
    fn unsupported_error_does_not_leak_value() {
        let v = Value::Other { type_name: "geometry".into(), text: "SECRET".into() };
        let err = literal(Family::MySql, &v).unwrap_err().to_string();
        assert!(err.contains("geometry"), "{err}");
        assert!(!err.contains("SECRET"), "{err}");
    }

    #[test]
    fn pagination() {
        assert_eq!(
            paginate(Family::MySql, "SELECT * FROM t;", 20, 10),
            "SELECT * FROM t\nLIMIT 10 OFFSET 20"
        );
        assert_eq!(
            paginate(Family::MySql, "SELECT * FROM t;;  ;\n", 0, 5),
            "SELECT * FROM t\nLIMIT 5 OFFSET 0"
        );
        assert_eq!(
            paginate(Family::Oracle, "SELECT * FROM t -- 注释", 0, 50),
            "SELECT * FROM t -- 注释\nOFFSET 0 ROWS FETCH NEXT 50 ROWS ONLY"
        );
        assert!(paginate(Family::Dameng, "SELECT 1", 0, 1).ends_with("\nLIMIT 1 OFFSET 0"));
    }
}
