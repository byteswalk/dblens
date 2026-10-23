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
        Value::Float(f) => {
            if !f.is_finite() {
                return Err(Error::internal(format!("无法生成非有限浮点数的 SQL 字面量：{f}")));
            }
            f.to_string()
        }
        Value::Decimal(s) => decimal_literal(family, s)?,
        Value::String(s) | Value::Interval(s) => string_literal(family, s),
        Value::Json(j) => string_literal(family, &j.to_string()),
        Value::Uuid(u) => string_literal(family, &u.to_string()),
        Value::Bytes(b) => bytes_literal(family, b),
        Value::Date(d) => format!("DATE '{}'", d.format("%Y-%m-%d")),
        Value::Time(t) => {
            let text = t.format("%H:%M:%S%.f").to_string();
            // Oracle 没有 TIME 类型，按字符串写出
            if family == Family::Oracle { string_literal(family, &text) } else { format!("TIME '{text}'") }
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
        Value::Lob(_) | Value::Other { .. } => {
            return Err(Error::internal(format!("该类型的值不能生成 SQL 字面量：{v:?}")));
        }
    })
}

fn decimal_literal(family: Family, s: &str) -> Result<String> {
    let numeric = !s.is_empty()
        && s.bytes().any(|c| c.is_ascii_digit())
        && s.bytes().all(|c| c.is_ascii_digit() || matches!(c, b'+' | b'-' | b'.' | b'e' | b'E'));
    if numeric {
        return Ok(s.to_string());
    }
    if family == Family::Postgres && matches!(s, "NaN" | "Infinity" | "-Infinity") {
        return Ok(format!("'{s}'::numeric"));
    }
    Err(Error::internal(format!("不是有效的数字文本，无法生成字面量：{s}")))
}

fn string_literal(family: Family, s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for c in s.chars() {
        match c {
            '\'' => out.push_str("''"),
            // MySQL 默认把反斜杠当转义符
            '\\' if family == Family::MySql => out.push_str("\\\\"),
            '\0' if family == Family::MySql => out.push_str("\\0"),
            c => out.push(c),
        }
    }
    out.push('\'');
    out
}

fn bytes_literal(family: Family, b: &[u8]) -> String {
    let hex: String = b.iter().map(|x| format!("{x:02X}")).collect();
    match family {
        Family::MySql => format!("X'{hex}'"),
        Family::Postgres => format!("'\\x{hex}'::bytea"),
        Family::Oracle => format!("HEXTORAW('{hex}')"),
        // 达梦的 0x 字面量不能为空，空值写成空字符串由数据库隐式转换
        Family::Dameng if b.is_empty() => "''".to_string(),
        Family::Dameng => format!("0x{hex}"),
    }
}

/// 把查询包装成分页查询。原查询放在单独的行上，避免它末尾的行注释吞掉右括号。
/// Oracle 使用 12c 起支持的 OFFSET … FETCH 语法。
pub fn paginate(family: Family, sql: &str, offset: u64, limit: u64) -> String {
    let inner = sql.trim().trim_end_matches(';').trim_end();
    match family {
        Family::Oracle => {
            format!("SELECT * FROM (\n{inner}\n) OFFSET {offset} ROWS FETCH NEXT {limit} ROWS ONLY")
        }
        Family::MySql | Family::Postgres | Family::Dameng => {
            format!("SELECT * FROM (\n{inner}\n) dblens_page LIMIT {limit} OFFSET {offset}")
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
        assert_eq!(lit(Family::MySql, Value::String("a\\b'c".into())), "'a\\\\b''c'");
        assert_eq!(lit(Family::Oracle, Value::String("a\\b".into())), "'a\\b'");
        assert_eq!(lit(Family::Dameng, Value::String("数洞".into())), "'数洞'");
        assert_eq!(lit(Family::MySql, Value::Json(serde_json::json!({"a": "x'y"}))), "'{\"a\":\"x''y\"}'");
    }

    #[test]
    fn bytes() {
        let v = Value::Bytes(vec![0x01, 0xab]);
        assert_eq!(lit(Family::MySql, v.clone()), "X'01AB'");
        assert_eq!(lit(Family::Postgres, v.clone()), "'\\x01AB'::bytea");
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

    #[test]
    fn pagination() {
        assert_eq!(
            paginate(Family::MySql, "SELECT * FROM t;", 20, 10),
            "SELECT * FROM (\nSELECT * FROM t\n) dblens_page LIMIT 10 OFFSET 20"
        );
        assert_eq!(
            paginate(Family::Oracle, "SELECT * FROM t -- 注释", 0, 50),
            "SELECT * FROM (\nSELECT * FROM t -- 注释\n) OFFSET 0 ROWS FETCH NEXT 50 ROWS ONLY"
        );
        assert!(paginate(Family::Dameng, "SELECT 1", 0, 1).ends_with("dblens_page LIMIT 1 OFFSET 0"));
    }
}
