use bytes::BytesMut;
use chrono::{DateTime, NaiveDate, NaiveDateTime, NaiveTime, Utc};
use dblens_core::{Error, Msg, Result, Value, msg};
use tokio_postgres::Row;
use tokio_postgres::types::{FromSql, IsNull, Kind, ToSql, Type, to_sql_checked};

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

/// 把十进制文本编码成 PG numeric 的二进制格式（`decode_numeric` 的逆过程）。接受符号、小数点、指数和 NaN；
/// 文本不是数字时返回 None。PG 14 以前没有 numeric 的无穷大，这里也不编码。
pub(crate) fn encode_numeric(text: &str) -> Option<Vec<u8>> {
    let t = text.trim();
    if t.eq_ignore_ascii_case("nan") {
        return Some(
            [0i16.to_be_bytes(), 0i16.to_be_bytes(), 0xC000u16.to_be_bytes(), 0u16.to_be_bytes()].concat(),
        );
    }
    let (neg, body) = match t.as_bytes().first()? {
        b'-' => (true, &t[1..]),
        b'+' => (false, &t[1..]),
        _ => (false, t),
    };
    let (mantissa, exp) = match body.find(['e', 'E']) {
        Some(i) => (&body[..i], body[i + 1..].parse::<i32>().ok()?),
        None => (body, 0),
    };
    let (ip, fp) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    if ip.is_empty() && fp.is_empty() || !ip.bytes().chain(fp.bytes()).all(|b| b.is_ascii_digit()) {
        return None;
    }
    // 把指数并进去：小数点右移 exp 位
    let mut digits: String = format!("{ip}{fp}");
    let mut point = i64::try_from(ip.len()).ok()? + i64::from(exp);
    if point < 0 {
        digits.insert_str(0, &"0".repeat(usize::try_from(-point).ok()?));
        point = 0;
    }
    let point = usize::try_from(point).ok()?;
    if digits.len() < point {
        digits.push_str(&"0".repeat(point - digits.len()));
    }
    let (int_part, frac_part) = digits.split_at(point);
    let dscale = u16::try_from(frac_part.len()).ok()?;
    let int_part = int_part.trim_start_matches('0');
    // 按 4 位一组：整数部分左补零，小数部分右补零
    let int_pad = (4 - int_part.len() % 4) % 4;
    let frac_pad = (4 - frac_part.len() % 4) % 4;
    let all = format!("{}{int_part}{frac_part}{}", "0".repeat(int_pad), "0".repeat(frac_pad));
    let mut groups: Vec<i16> =
        all.as_bytes().chunks(4).map(|c| std::str::from_utf8(c).unwrap().parse().unwrap()).collect();
    let mut weight = i32::try_from((int_pad + int_part.len()) / 4).ok()? - 1;
    while groups.first() == Some(&0) {
        groups.remove(0);
        weight -= 1;
    }
    while groups.last() == Some(&0) {
        groups.pop();
    }
    if groups.is_empty() {
        weight = 0;
    }
    let sign: u16 = if neg && !groups.is_empty() { 0x4000 } else { 0 };
    let mut out = Vec::with_capacity(8 + groups.len() * 2);
    out.extend_from_slice(&i16::try_from(groups.len()).ok()?.to_be_bytes());
    out.extend_from_slice(&i16::try_from(weight).ok()?.to_be_bytes());
    out.extend_from_slice(&sign.to_be_bytes());
    out.extend_from_slice(&dscale.to_be_bytes());
    for g in groups {
        out.extend_from_slice(&g.to_be_bytes());
    }
    Some(out)
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

/// 人大金仓 SQL Server 模式的 `time`、`datetime2` 按 100 纳秒计（PG 按微秒），`datetime2` 从 2000-01-01 起算。
/// 名字和 MySQL 模式的同名类型（按微秒）相同，只能按 OID 区分，由连接按发送函数名查出。这里判断值得去查的类型。
pub(crate) fn is_tick_candidate(ty: &Type) -> bool {
    Type::from_oid(ty.oid()).is_none()
        && matches!(ty.name(), "time" | "datetime2" | "tinyint" | "bit" | "money")
}

/// 人大金仓 SQL Server 模式里按自己的二进制格式收发的类型（按发送函数名区分）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SpecialKind {
    /// time、datetime2：100 纳秒计
    Ticks,
    /// tinyint：0–255，一个无符号字节（MySQL 模式的同名类型是有符号的，不在此列）
    Tinyint,
    /// bit：一个字节 0/1
    Bit,
    /// money：8 字节整数，4 位小数
    Money,
}

/// 连接上查出的 SQL Server 模式特有类型的 OID。
#[derive(Debug, Clone, Default)]
pub(crate) struct SpecialTypes(Vec<(u32, SpecialKind)>);

impl SpecialTypes {
    pub(crate) const SEND_FUNCTIONS: &'static str = "'sqlserver_time_send', 'sqlserver_datetime2_send', \
         'sqlserver_tinyint_send', 'sqlserver_bit_send', 'sqlserver_money_send'";

    pub(crate) fn from_rows(rows: impl IntoIterator<Item = (u32, String)>) -> Self {
        Self(
            rows.into_iter()
                .filter_map(|(oid, send)| {
                    let kind = match send.as_str() {
                        "sqlserver_time_send" | "sqlserver_datetime2_send" => SpecialKind::Ticks,
                        "sqlserver_tinyint_send" => SpecialKind::Tinyint,
                        "sqlserver_bit_send" => SpecialKind::Bit,
                        "sqlserver_money_send" => SpecialKind::Money,
                        _ => return None,
                    };
                    Some((oid, kind))
                })
                .collect(),
        )
    }

    fn kind(&self, oid: u32) -> Option<SpecialKind> {
        self.0.iter().find(|(o, _)| *o == oid).map(|(_, k)| *k)
    }
}

/// money 的整数值（单位万分之一）写成小数文本。
fn money_text(v: i64) -> String {
    let sign = if v < 0 { "-" } else { "" };
    let a = v.unsigned_abs();
    format!("{sign}{}.{:04}", a / 10_000, a % 10_000)
}

/// 小数文本 → money 的整数值（单位万分之一），多于 4 位的小数截掉。
fn money_units(s: &str) -> std::result::Result<i64, BoxError> {
    let s = s.trim();
    let (neg, body) = match s.strip_prefix('-') {
        Some(b) => (true, b),
        None => (false, s.strip_prefix('+').unwrap_or(s)),
    };
    let (int, frac) = body.split_once('.').unwrap_or((body, ""));
    let frac: String = frac.chars().chain(std::iter::repeat('0')).take(4).collect();
    let int: i64 = if int.is_empty() { 0 } else { int.parse()? };
    let v = int.checked_mul(10_000).and_then(|x| x.checked_add(frac.parse::<i64>().ok()?));
    let v = v.ok_or("money out of range")?;
    Ok(if neg { -v } else { v })
}

fn decode_special(kind: SpecialKind, ty: &Type, raw: &[u8]) -> std::result::Result<Value, BoxError> {
    Ok(match kind {
        SpecialKind::Ticks => return decode_ticks(ty, raw),
        SpecialKind::Tinyint => Value::Int(i64::from(*raw.first().ok_or("empty tinyint")?)),
        SpecialKind::Bit => Value::Bool(*raw.first().ok_or("empty bit")? != 0),
        SpecialKind::Money => Value::Decimal(money_text(i64::from_be_bytes(raw.try_into()?))),
    })
}

fn encode_special(
    kind: SpecialKind,
    v: &Value,
    ty: &Type,
    out: &mut BytesMut,
) -> std::result::Result<IsNull, BoxError> {
    if matches!(v, Value::Null) {
        return Ok(IsNull::Yes);
    }
    let bad = || -> BoxError { format!("cannot bind {v:?} to {}", ty.name()).into() };
    match kind {
        SpecialKind::Ticks => return encode_ticks(v, ty, out),
        SpecialKind::Tinyint => {
            let n: u8 = match v {
                Value::Int(i) => u8::try_from(*i)?,
                Value::UInt(u) => u8::try_from(*u)?,
                Value::Bool(b) => u8::from(*b),
                Value::String(s) | Value::Decimal(s) => s.trim().parse()?,
                _ => return Err(bad()),
            };
            out.extend_from_slice(&[n]);
        }
        SpecialKind::Bit => {
            let b = match v {
                Value::Bool(b) => *b,
                Value::Int(i) => *i != 0,
                Value::UInt(u) => *u != 0,
                Value::String(s) | Value::Decimal(s) => !matches!(s.trim(), "0" | "false" | "FALSE" | ""),
                _ => return Err(bad()),
            };
            out.extend_from_slice(&[u8::from(b)]);
        }
        SpecialKind::Money => {
            let units = match v {
                Value::Int(i) => i.checked_mul(10_000).ok_or("money out of range")?,
                Value::Float(f) => money_units(&format!("{f:.4}"))?,
                Value::String(s) | Value::Decimal(s) => money_units(s)?,
                _ => return Err(bad()),
            };
            out.extend_from_slice(&units.to_be_bytes());
        }
    }
    Ok(IsNull::No)
}

fn epoch_2000() -> NaiveDateTime {
    NaiveDate::from_ymd_opt(2000, 1, 1).unwrap().and_time(NaiveTime::MIN)
}

fn decode_ticks(ty: &Type, raw: &[u8]) -> std::result::Result<Value, BoxError> {
    let micros = i64::from_be_bytes(raw.try_into()?) / 10;
    let delta = chrono::Duration::microseconds(micros);
    Ok(if ty.name() == "time" {
        Value::Time(NaiveTime::MIN.overflowing_add_signed(delta).0)
    } else {
        Value::DateTime(epoch_2000().checked_add_signed(delta).ok_or("datetime2 out of range")?)
    })
}

fn encode_ticks(v: &Value, ty: &Type, out: &mut BytesMut) -> std::result::Result<IsNull, BoxError> {
    let micros = match v {
        Value::Null => return Ok(IsNull::Yes),
        Value::Time(t) => (*t - NaiveTime::MIN).num_microseconds().ok_or("time out of range")?,
        Value::DateTime(dt) => (*dt - epoch_2000()).num_microseconds().ok_or("datetime2 out of range")?,
        Value::Date(d) => {
            (d.and_time(NaiveTime::MIN) - epoch_2000()).num_microseconds().ok_or("date out of range")?
        }
        other => return Err(format!("cannot bind {other:?} to {}", ty.name()).into()),
    };
    out.extend_from_slice(&(micros * 10).to_be_bytes());
    Ok(IsNull::No)
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

/// 兼容库自定义的类型对应的标准类型：域类型取基础类型（人大金仓 Oracle 模式的 `date` 是 timestamp 上的域，
/// `blob`、`clob` 也是域）；openGauss 的 `clob`、`nvarchar2` 等是独立的类型，二进制格式与对应的标准类型相同
/// （GaussDB A 模式的 `date` 是 `datea`，格式同 timestamp）。只有 OID 不是内置类型时才查这里，同名的标准类型不受影响。
/// `int1`、`tinyint`、`int16`（16 字节整数，openGauss 的序列参数用它）没有对应的标准类型，原样返回，
/// 由 `decode_as` 单独处理。
fn standard_type(ty: &Type) -> Option<Type> {
    if let Kind::Domain(base) = ty.kind() {
        return Some(standard_type(base).unwrap_or_else(|| base.clone()));
    }
    match ty.name() {
        "clob" | "nclob" | "nvarchar2" | "varchar2" | "long" => Some(Type::TEXT),
        // SQL Server 模式的行版本是 8 个字节
        "rowversion" => Some(Type::BYTEA),
        // 人大金仓 MySQL、SQL Server 模式 sys 模式下的字符、二进制、GUID 类型（SQL Server 模式的 datetime2 格式不同，不在此列）
        "varcharbyte" | "bpcharbyte" | "nchar" | "nvarchar" => Some(Type::TEXT),
        "binary" | "varbinary" => Some(Type::BYTEA),
        // SQL Server 模式的 uniqueidentifier 取值的类型名是 uuid（另一个 OID）
        "uniqueidentifier" | "uuid" => Some(Type::UUID),
        "blob" | "raw" => Some(Type::BYTEA),
        "number" => Some(Type::NUMERIC),
        "smalldatetime" | "datea" => Some(Type::TIMESTAMP),
        // 人大金仓 MySQL、SQL Server 模式在 sys 模式下另有同名的日期时间类型，二进制格式与标准类型相同
        "date" => Some(Type::DATE),
        "time" => Some(Type::TIME),
        "datetime" | "timestamp" => Some(Type::TIMESTAMP),
        "year" => Some(Type::INT4),
        "binary_double" => Some(Type::FLOAT8),
        "binary_float" => Some(Type::FLOAT4),
        "int1" | "tinyint" | "int16" => Some(ty.clone()),
        _ => None,
    }
}

/// 按 `standard_type` 给出的类型解码二进制值。
fn decode_as(ty: &Type, raw: &[u8]) -> std::result::Result<Value, BoxError> {
    macro_rules! from {
        ($t:ty, $f:expr) => {
            <$t as FromSql>::from_sql(ty, raw).map($f)?
        };
    }
    Ok(match *ty {
        Type::BOOL => from!(bool, Value::Bool),
        Type::INT2 => from!(i16, |v| Value::Int(v.into())),
        Type::INT4 => from!(i32, |v| Value::Int(v.into())),
        Type::INT8 => from!(i64, Value::Int),
        Type::FLOAT4 => from!(f32, |v| Value::Float(v.into())),
        Type::FLOAT8 => from!(f64, Value::Float),
        Type::NUMERIC => Value::Decimal(decode_numeric(raw)?),
        Type::TEXT | Type::VARCHAR | Type::BPCHAR => from!(String, Value::String),
        Type::BYTEA => Value::Bytes(raw.to_vec()),
        Type::DATE => from!(NaiveDate, Value::Date),
        Type::TIME => from!(NaiveTime, Value::Time),
        Type::TIMESTAMP => from!(NaiveDateTime, Value::DateTime),
        Type::TIMESTAMPTZ => from!(DateTime<Utc>, |v| Value::DateTimeTz(v.fixed_offset())),
        Type::UUID => from!(uuid::Uuid, Value::Uuid),
        // openGauss 的 int1 是 0–255，人大金仓的 tinyint 是 -128–127，都是 1 个字节
        _ if raw.len() == 1 && ty.name() == "int1" => Value::Int(raw[0].into()),
        _ if raw.len() == 1 && ty.name() == "tinyint" => Value::Int((raw[0] as i8).into()),
        _ if ty.name() == "int16" => match <[u8; 16]>::try_from(raw) {
            Ok(b) => match i64::try_from(i128::from_be_bytes(b)) {
                Ok(v) => Value::Int(v),
                Err(_) => Value::Decimal(i128::from_be_bytes(b).to_string()),
            },
            Err(_) => Value::Other { type_name: ty.name().to_string(), text: other_text(raw) },
        },
        _ => Value::Other { type_name: ty.name().to_string(), text: other_text(raw) },
    })
}

/// 不认识的类型：枚举、xml、citext 等类型的二进制格式就是文本，其余显示为十六进制。
fn other_text(raw: &[u8]) -> String {
    match std::str::from_utf8(raw) {
        Ok(s) => s.to_string(),
        Err(_) => format!("\\x{}", raw.iter().map(|b| format!("{b:02x}")).collect::<String>()),
    }
}

/// 读取一行中第 `idx` 列的值。
/// `special`：SQL Server 模式特有格式的类型（见 `SpecialTypes`）。
pub(crate) fn pg_value(row: &Row, idx: usize, special: &SpecialTypes) -> Result<Value> {
    let ty = row.columns()[idx].type_().clone();
    if let Some(kind) = special.kind(ty.oid()) {
        let raw =
            row.try_get::<_, Option<RawValue>>(idx).map_err(|e| Error::internal(Msg::raw(e.to_string())))?;
        return match raw {
            None => Ok(Value::Null),
            Some(r) => decode_special(kind, &ty, r.0).map_err(|e| {
                Error::internal(msg!(
                    "pg.read_column_failed",
                    column = idx + 1,
                    ty = ty.name(),
                    error = Msg::raw(e.to_string())
                ))
            }),
        };
    }
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
            match (raw, standard_type(&ty)) {
                (None, _) => None,
                (Some(r), Some(base)) => Some(decode_as(&base, r.0).map_err(|e| {
                    let cause =
                        e.downcast_ref::<Error>().map_or_else(|| Msg::raw(e.to_string()), Error::to_msg);
                    Error::internal(msg!(
                        "pg.read_column_failed",
                        column = idx + 1,
                        ty = ty.name(),
                        error = cause
                    ))
                })?),
                (Some(r), None) => {
                    Some(Value::Other { type_name: ty.name().to_string(), text: other_text(r.0) })
                }
            }
        }
    };
    Ok(value.unwrap_or(Value::Null))
}

/// 把 `Value` 绑定为 PG 参数，按目标参数类型做必要的转换。
#[derive(Debug)]
/// 第二项是按 100 纳秒编码的类型的 OID（见 `is_tick_candidate`）。
pub(crate) struct PgParam<'a>(pub &'a Value, pub &'a SpecialTypes);

impl ToSql for PgParam<'_> {
    fn to_sql(&self, ty: &Type, out: &mut BytesMut) -> std::result::Result<IsNull, BoxError> {
        if let Some(kind) = self.1.kind(ty.oid()) {
            return encode_special(kind, self.0, ty, out);
        }
        // 兼容库的域类型和自定义类型按对应的标准类型编码（人大金仓 Oracle 模式的 date 是 timestamp 上的域）
        if let Some(base) = standard_type(ty).filter(|b| b != ty) {
            return self.to_sql(&base, out);
        }
        match self.0 {
            Value::Null => Ok(IsNull::Yes),
            Value::Bool(b) => b.to_sql(ty, out),
            Value::Int(i) => match *ty {
                Type::INT2 => i16::try_from(*i)?.to_sql(ty, out),
                Type::INT4 => i32::try_from(*i)?.to_sql(ty, out),
                Type::FLOAT4 => (*i as f32).to_sql(ty, out),
                Type::FLOAT8 => (*i as f64).to_sql(ty, out),
                Type::NUMERIC => numeric_to_sql(&i.to_string(), out),
                _ => i.to_sql(ty, out),
            },
            Value::UInt(u) => PgParam(&Value::Int(i64::try_from(*u)?), self.1).to_sql(ty, out),
            Value::Float(f) => match *ty {
                Type::FLOAT4 => (*f as f32).to_sql(ty, out),
                Type::NUMERIC if f.is_finite() => numeric_to_sql(&f.to_string(), out),
                Type::NUMERIC if f.is_nan() => numeric_to_sql("NaN", out),
                Type::NUMERIC => Err(numeric_param_hint()),
                _ => f.to_sql(ty, out),
            },
            Value::String(d) | Value::Decimal(d) if *ty == Type::NUMERIC => numeric_to_sql(d, out),
            Value::String(s) | Value::Decimal(s) | Value::Interval(s) => s.as_str().to_sql(ty, out),
            Value::Bytes(b) => b.as_slice().to_sql(ty, out),
            Value::Date(d) if *ty == Type::TIMESTAMP => d.and_time(chrono::NaiveTime::MIN).to_sql(ty, out),
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

fn numeric_to_sql(text: &str, out: &mut BytesMut) -> std::result::Result<IsNull, BoxError> {
    let bytes = encode_numeric(text).ok_or_else(numeric_param_hint)?;
    out.extend_from_slice(&bytes);
    Ok(IsNull::No)
}

fn numeric_param_hint() -> BoxError {
    Error::invalid(msg!("pg.numeric_param")).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numeric_encode_round_trip() {
        for (text, back) in [
            ("0", "0"),
            ("1", "1"),
            ("-1", "-1"),
            ("10000", "10000"),
            ("123456789012345678901234567890", "123456789012345678901234567890"),
            ("1234.5", "1234.5"),
            ("-0.01", "-0.01"),
            ("0.0001", "0.0001"),
            ("0.00001", "0.00001"),
            ("12.30", "12.30"),
            ("+7", "7"),
            (".25", "0.25"),
            ("5.", "5"),
            ("1e3", "1000"),
            ("1.5e-3", "0.0015"),
            ("-2.5E2", "-250"),
            ("-0.000", "0.000"),
            ("NaN", "NaN"),
        ] {
            let raw = encode_numeric(text).unwrap_or_else(|| panic!("{text}"));
            assert_eq!(decode_numeric(&raw).unwrap(), back, "{text}");
        }
        for bad in ["", "-", ".", "abc", "1.2.3", "1e", "Infinity", "1 2"] {
            assert!(encode_numeric(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn sqlserver_money_round_trip() {
        assert_eq!(money_text(123_456), "12.3456");
        assert_eq!(money_text(-5_000), "-0.5000");
        assert_eq!(money_text(i64::MAX), "922337203685477.5807");
        assert_eq!(money_units("12.3456").unwrap(), 123_456);
        assert_eq!(money_units("-0.5").unwrap(), -5_000);
        assert_eq!(money_units(".25").unwrap(), 2_500);
        assert_eq!(money_units("1.234567").unwrap(), 12_345);
        assert!(money_units("1e3").is_err());
        let t = Type::new("tinyint".into(), 90001, tokio_postgres::types::Kind::Simple, "sys".into());
        assert_eq!(decode_special(SpecialKind::Tinyint, &t, &[200]).unwrap(), Value::Int(200));
        assert_eq!(decode_special(SpecialKind::Bit, &t, &[1]).unwrap(), Value::Bool(true));
        let mut out = BytesMut::new();
        encode_special(SpecialKind::Money, &Value::Decimal("12.3456".into()), &t, &mut out).unwrap();
        assert_eq!(decode_special(SpecialKind::Money, &t, &out).unwrap(), Value::Decimal("12.3456".into()));
        let types =
            SpecialTypes::from_rows([(7, "sqlserver_bit_send".to_string()), (8, "other_send".to_string())]);
        assert_eq!((types.kind(7), types.kind(8)), (Some(SpecialKind::Bit), None));
    }

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
    fn tick_types_round_trip() {
        let time = Type::new("time".into(), 7755, Kind::Simple, "sys".into());
        let dt2 = Type::new("datetime2".into(), 7756, Kind::Simple, "sys".into());
        assert!(is_tick_candidate(&time) && is_tick_candidate(&dt2) && !is_tick_candidate(&Type::TIME));
        // 13:45:30 是 49530 秒，按 100 纳秒计
        let raw = (49_530_i64 * 10_000_000).to_be_bytes();
        assert_eq!(
            decode_ticks(&time, &raw).unwrap(),
            Value::Time(NaiveTime::from_hms_opt(13, 45, 30).unwrap())
        );
        let dt =
            NaiveDate::from_ymd_opt(2024, 2, 29).unwrap().and_hms_micro_opt(13, 45, 30, 123_456).unwrap();
        let mut out = BytesMut::new();
        encode_ticks(&Value::DateTime(dt), &dt2, &mut out).unwrap();
        assert_eq!(decode_ticks(&dt2, &out).unwrap(), Value::DateTime(dt));
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
