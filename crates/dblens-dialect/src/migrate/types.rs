//! 迁移中的类型：列类型的映射（覆盖 → 规则 → 同家族原样 → 经逻辑类型）、有损判断、序列、目标库的能力。

use dblens_core::schema::{Column, Sequence};
use dblens_core::{LogicalType, Msg, msg};

use super::{ColumnOverride, NoteLevel, Notes, TypeRule};
use crate::types::parse_type;
use crate::{Compat, Dialect, Family};

/// 一列的目标类型：（原生类型文本，逻辑类型）。提示经 `note` 报告。
pub(super) fn column_type(
    src: &Dialect,
    tgt: &Dialect,
    c: &Column,
    ov: &ColumnOverride,
    rules: &[TypeRule],
    mut note: impl FnMut(NoteLevel, Msg),
) -> (String, LogicalType) {
    let source = c.data_type.native.trim();
    let logical = src.logical_type(source);
    let explicit = ov.data_type.as_deref().map(str::trim).filter(|t| !t.is_empty()).map(str::to_string);
    let explicit = explicit.or_else(|| {
        rules.iter().find(|r| rule_matches(&r.source, source)).map(|r| r.target.trim().to_string())
    });
    if let Some(native) = explicit {
        let back = tgt.logical_type(&native);
        if back == LogicalType::Other && logical != LogicalType::Other {
            note(NoteLevel::Lossy, msg!("migrate.unknown_target_type", native = native));
        }
        return (native, back);
    }
    if keeps_native(src, tgt) {
        return (source.to_string(), logical);
    }
    if logical == LogicalType::Other {
        let native = tgt.native_type(&LogicalType::Text);
        let level = if ov.accept { NoteLevel::Lossy } else { NoteLevel::Unsupported };
        note(level, msg!("migrate.type_unknown", source = source, target = native));
        return (native, LogicalType::Text);
    }
    let logical = integer_number(src, tgt, logical);
    let native = tgt.native_type(&logical);
    let back = tgt.logical_type(&native);
    if !equivalent(tgt, &logical, &back) {
        note(NoteLevel::Lossy, msg!("migrate.type_lossy", from = source, to = native));
    }
    (native, back)
}

/// Oracle 的 NUMBER(p)（小数位 0、p ≤ 19）迁到其他库时用整数类型：p ≤ 4 用 16 位，p ≤ 9 用 32 位，否则 64 位
/// （NUMBER(19) 超出 bigint 的取值很少见，给有损提示）。这样标识列能建，外键两边的类型也一致。
fn integer_number(src: &Dialect, tgt: &Dialect, logical: LogicalType) -> LogicalType {
    match logical {
        LogicalType::Decimal { precision: Some(p), scale: None | Some(0) }
            if src.family() == Family::Oracle && tgt.family() != Family::Oracle && p <= 19 =>
        {
            LogicalType::Int {
                bits: if p <= 4 {
                    16
                } else if p <= 9 {
                    32
                } else {
                    64
                },
            }
        }
        other => other,
    }
}

/// 自增列在目标中必须是整数：小数位为 0 的定点数改成 64 位整数。
pub(super) fn identity_type(tgt: &Dialect, native: String, logical: LogicalType) -> (String, LogicalType) {
    match logical {
        LogicalType::Decimal { scale: None | Some(0), .. } if tgt.family() != Family::Oracle => {
            let l = LogicalType::Int { bits: 64 };
            (tgt.native_type(&l), l)
        }
        other => (native, other),
    }
}

/// 同家族、且目标认得源的写法（兼容模式相同，或目标是兼容 Oracle 的模式）时原样保留类型文本。
fn keeps_native(src: &Dialect, tgt: &Dialect) -> bool {
    src.family() == tgt.family() && (src.compat() == tgt.compat() || tgt.compat() == Compat::Oracle)
}

/// 规则的源类型是否匹配：类型名相同；规则带参数时参数也要相同。
fn rule_matches(rule: &str, native: &str) -> bool {
    let (r, n) = (parse_type(rule), parse_type(native));
    let args = |a: &[String]| a.iter().map(|x| x.trim().to_ascii_uppercase()).collect::<Vec<_>>();
    r.name == n.name && (r.args.is_empty() || args(&r.args) == args(&n.args)) && r.unsigned == n.unsigned
}

/// 整数位数：能容纳 `bits` 位有符号整数的十进制位数。
fn int_digits(bits: u8) -> u32 {
    match bits {
        0..=8 => 3,
        9..=16 => 5,
        17..=24 => 7,
        25..=32 => 10,
        _ => 19,
    }
}

/// 映射后是否等价：没有改变取值范围、精度或语义。
fn equivalent(tgt: &Dialect, a: &LogicalType, b: &LogicalType) -> bool {
    use LogicalType as L;
    match (a, b) {
        _ if a == b => true,
        (L::Text, L::VarChar { len: None }) | (L::VarChar { len: None }, L::Text) => true,
        (L::Blob, L::VarBinary { len: None }) | (L::VarBinary { len: None }, L::Blob) => true,
        // UUID 存成 36 个字符的文本
        (L::Uuid, L::Char { len: Some(36) } | L::VarChar { len: Some(36) }) => true,
        // PG 只有 bytea
        (L::Binary { .. } | L::VarBinary { .. }, L::Blob) => tgt.family() == Family::Postgres,
        // Oracle 没有整数类型，NUMBER(n) 能容纳即可
        (L::Int { bits }, L::Decimal { precision: Some(p), scale: None | Some(0) }) => {
            *p >= int_digits(*bits)
        }
        (L::Decimal { precision: Some(p), scale: None | Some(0) }, L::Int { bits }) => *p < int_digits(*bits),
        // Oracle 的 DATE 带时分秒，能容纳日期
        (L::Date, L::Timestamp { tz: false, precision: Some(0) }) => tgt.family() == Family::Oracle,
        _ => false,
    }
}

/// 目标中不能建索引、主键的大字段类型：MySQL、Oracle、达梦的 TEXT/CLOB/BLOB/JSON。
pub(super) fn is_lob(tgt: &Dialect, l: &LogicalType) -> bool {
    use LogicalType as L;
    tgt.family() != Family::Postgres && matches!(l, L::Text | L::Blob | L::Json | L::VarChar { len: None })
}

/// 目标库能否建自增列：Oracle 12c 起才有标识列，其余都可以。
pub(super) fn identity_supported(tgt: &Dialect) -> bool {
    !(tgt.family() == Family::Oracle && tgt.version().is_some_and(|v| v < 120000))
}

/// 外键动作在目标库中是否可用：Oracle、达梦只有 ON DELETE CASCADE / SET NULL；MySQL（InnoDB）不支持 SET DEFAULT。
pub(super) fn fk_action_supported(tgt: &Dialect, what: &str, action: &str) -> bool {
    match tgt.family() {
        Family::Oracle | Family::Dameng => what == "DELETE" && (action == "CASCADE" || action == "SET NULL"),
        Family::MySql => action != "SET DEFAULT",
        Family::Postgres => true,
    }
}

/// 序列：PG 系的取值是 bigint，超出的上下限去掉。
pub(super) fn sequence(tgt: &Dialect, s: &Sequence, name: &str, n: &mut Notes) -> Sequence {
    let mut out = Sequence { name: name.to_string(), ..s.clone() };
    if tgt.family() == Family::Postgres {
        // 达梦的不缓存是 CACHE 0，PG 的缓存至少为 1
        out.cache = out.cache.filter(|&c| c >= 1);
        let fits = |v: &Option<String>| v.as_deref().is_none_or(|v| v.trim().parse::<i64>().is_ok());
        for (label, v) in [("MINVALUE", &mut out.min_value), ("MAXVALUE", &mut out.max_value)] {
            if !fits(v) {
                n.push(None, None, NoteLevel::Info, msg!("migrate.sequence_bound", bound = label));
                *v = None;
            }
        }
        if !fits(&out.start) {
            n.push(None, None, NoteLevel::Unsupported, msg!("migrate.sequence_bound", bound = "START WITH"));
        }
    }
    out
}
