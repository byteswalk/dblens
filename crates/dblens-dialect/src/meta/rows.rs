//! 元数据查询的结果行，以及按列名取值的宽松转换（各驱动对同一列可能返回不同的 `Value` 变体）。

use dblens_core::{Error, Result, Row, Value, msg};
use dblens_driver::ExecEvent;

/// 一个结果集：列名（已转小写）和行。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MetaRows {
    columns: Vec<String>,
    rows: Vec<Row>,
}

impl MetaRows {
    pub fn new(columns: Vec<String>, rows: Vec<Row>) -> Self {
        Self { columns: columns.into_iter().map(|c| c.to_lowercase()).collect(), rows }
    }

    /// 从执行事件中取第一个结果集。没有结果集（如只返回影响行数）时报错。
    pub fn from_events(events: Vec<ExecEvent>) -> Result<Self> {
        let mut columns = None;
        let mut rows = Vec::new();
        for ev in events {
            match ev {
                ExecEvent::ResultStart { columns: c } if columns.is_none() => {
                    columns = Some(c.into_iter().map(|m| m.name).collect());
                }
                ExecEvent::Rows(r) if columns.is_some() => rows.extend(r),
                ExecEvent::ResultEnd if columns.is_some() => break,
                _ => {}
            }
        }
        let columns = columns.ok_or_else(|| Error::internal(msg!("meta.no_result_set")))?;
        Ok(Self::new(columns, rows))
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// 检查结果里有这些列（模板写错列别名时尽早报错）。
    pub fn require(&self, key: &str, columns: &[&str]) -> Result<()> {
        match columns.iter().find(|c| !self.columns.iter().any(|have| have == *c)) {
            Some(missing) => Err(Error::internal(msg!("meta.missing_column", key = key, column = missing))),
            None => Ok(()),
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = MetaRow<'_>> {
        self.rows.iter().map(|row| MetaRow { columns: &self.columns, row })
    }
}

/// 一行。取不到的列按 NULL 处理（用 `MetaRows::require` 事先检查）。
#[derive(Debug, Clone, Copy)]
pub struct MetaRow<'a> {
    columns: &'a [String],
    row: &'a Row,
}

impl MetaRow<'_> {
    pub fn get(&self, name: &str) -> &Value {
        const NULL: Value = Value::Null;
        self.columns.iter().position(|c| c == name).and_then(|i| self.row.get(i)).unwrap_or(&NULL)
    }

    /// 文本值；数字等也转成文本。NULL 返回 None。LOB 只能拿到预览部分。
    pub fn str(&self, name: &str) -> Option<String> {
        match self.get(name) {
            Value::Null => None,
            Value::String(s) | Value::Decimal(s) | Value::Interval(s) => Some(s.clone()),
            Value::Int(i) => Some(i.to_string()),
            Value::UInt(u) => Some(u.to_string()),
            Value::Float(f) => Some(f.to_string()),
            Value::Bool(b) => Some(b.to_string()),
            Value::Bytes(b) => Some(String::from_utf8_lossy(b).into_owned()),
            Value::Lob(l) if !l.binary => Some(l.preview.clone()),
            Value::Other { text, .. } => Some(text.clone()),
            other => Some(format!("{other:?}")),
        }
    }

    /// 去掉首尾空白后的文本，空串当作 None（注释、默认值、表达式用）。
    pub fn text(&self, name: &str) -> Option<String> {
        self.str(name).map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
    }

    /// 名字等必有的文本，NULL 时为空串。
    pub fn name(&self, name: &str) -> String {
        self.str(name).unwrap_or_default()
    }

    pub fn int(&self, name: &str) -> Option<i64> {
        match self.get(name) {
            Value::Int(i) => Some(*i),
            Value::UInt(u) => i64::try_from(*u).ok(),
            Value::Bool(b) => Some(i64::from(*b)),
            Value::Float(f) if f.fract() == 0.0 => Some(*f as i64),
            Value::Decimal(s) | Value::String(s) => {
                let s = s.trim();
                s.parse().ok().or_else(|| s.strip_suffix(".0").and_then(|t| t.parse().ok()))
            }
            _ => None,
        }
    }

    /// 布尔标志：接受 bool、非零整数，以及 Y/YES/T/TRUE/1（不区分大小写）。
    pub fn flag(&self, name: &str) -> bool {
        match self.get(name) {
            Value::Bool(b) => *b,
            Value::Int(i) => *i != 0,
            Value::UInt(u) => *u != 0,
            Value::Decimal(s) | Value::String(s) => {
                matches!(s.trim().to_ascii_uppercase().as_str(), "Y" | "YES" | "T" | "TRUE" | "1")
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use dblens_core::LobPreview;
    use dblens_driver::ColumnMeta;

    use super::*;

    fn rows() -> MetaRows {
        MetaRows::new(
            vec!["NAME".into(), "Flag".into(), "n".into()],
            vec![
                vec![Value::String("emp".into()), Value::String("YES".into()), Value::Decimal("10".into())],
                vec![Value::Null, Value::Int(0), Value::UInt(7)],
                vec![
                    Value::Lob(LobPreview {
                        binary: false,
                        preview: " 正文 ".into(),
                        total_len: 8,
                        truncated: false,
                    }),
                    Value::Bool(true),
                    Value::String("3.0".into()),
                ],
            ],
        )
    }

    #[test]
    fn column_names_are_case_insensitive_and_values_are_coerced() {
        let r = rows();
        let all: Vec<MetaRow> = r.iter().collect();
        assert_eq!(all[0].name("name"), "emp");
        assert!(all[0].flag("flag"));
        assert_eq!(all[0].int("n"), Some(10));
        assert_eq!(all[1].str("name"), None);
        assert_eq!(all[1].name("name"), "");
        assert!(!all[1].flag("flag"));
        assert_eq!(all[1].int("n"), Some(7));
        assert_eq!(all[2].text("name").as_deref(), Some("正文"));
        assert!(all[2].flag("flag"));
        assert_eq!(all[2].int("n"), Some(3));
        assert_eq!(all[2].get("no_such_column"), &Value::Null);
    }

    #[test]
    fn flag_accepts_common_spellings() {
        for (v, want) in [
            ("Y", true),
            ("yes", true),
            ("t", true),
            ("TRUE", true),
            ("1", true),
            ("N", false),
            ("NO", false),
            ("f", false),
            ("", false),
        ] {
            let r = MetaRows::new(vec!["f".into()], vec![vec![Value::String(v.into())]]);
            assert_eq!(r.iter().next().unwrap().flag("f"), want, "{v}");
        }
    }

    #[test]
    fn text_treats_blank_as_none() {
        let r = MetaRows::new(vec!["c".into()], vec![vec![Value::String("  \n".into())]]);
        assert_eq!(r.iter().next().unwrap().text("c"), None);
    }

    #[test]
    fn require_reports_missing_column() {
        let r = rows();
        assert!(r.require("tables", &["name", "flag"]).is_ok());
        let err = r.require("tables", &["name", "comment"]).unwrap_err().to_string();
        assert!(err.contains("tables") && err.contains("comment"), "{err}");
    }

    #[test]
    fn from_events_takes_first_result_set() {
        let events = vec![
            ExecEvent::Message("notice".into()),
            ExecEvent::ResultStart { columns: vec![ColumnMeta::new("A", "TEXT")] },
            ExecEvent::Rows(vec![vec![Value::Int(1)]]),
            ExecEvent::Rows(vec![vec![Value::Int(2)]]),
            ExecEvent::ResultEnd,
            ExecEvent::ResultStart { columns: vec![ColumnMeta::new("B", "TEXT")] },
            ExecEvent::Rows(vec![vec![Value::Int(3)]]),
            ExecEvent::ResultEnd,
        ];
        let r = MetaRows::from_events(events).unwrap();
        assert_eq!(r.len(), 2);
        assert!(r.require("x", &["a"]).is_ok());
        assert!(MetaRows::from_events(vec![ExecEvent::Affected(1)]).is_err());
    }
}
