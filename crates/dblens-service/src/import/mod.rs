//! 导入（M2b 设计文档第 4、5 节）：读取 CSV / Excel 文件、预览与类型推断、值的转换。

mod file;
mod job;
mod values;

use std::path::Path;

use dblens_core::{LogicalType, Result, Value};
use serde::Serialize;

use crate::export::{ExportWarnings, cell_text};

pub(crate) use file::open_text;
pub use file::{FileFormat, FileOptions, FileReader, FileRow, ReadWarnings, TextEncoding};
pub(crate) use job::exec;
pub use job::{
    ColumnMapping, DEFAULT_BATCH, ImportRequest, NewColumn, NewTable, OnError, Transaction, new_table,
};
pub use values::{EmptyAs, INFER_ROWS, convert, infer_type};

/// 预览返回的行数。
pub const PREVIEW_ROWS: usize = 100;

/// 值的文本（与导出、结果表格的显示一致）；NULL 为空字符串。
pub(crate) fn text(v: &Value) -> String {
    cell_text(v, &mut ExportWarnings::default()).unwrap_or_default()
}

/// 文件预览：列名、前 100 行、按前 1000 行推断的类型。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FilePreview {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<Value>>,
    pub types: Vec<LogicalType>,
    pub warnings: ReadWarnings,
    /// Excel 的全部工作表名。
    pub sheets: Vec<String>,
    /// CSV 实际使用的编码（`UTF-8`、`GBK`、`UTF-16`）和分隔符。
    pub encoding: Option<&'static str>,
    pub delimiter: Option<char>,
}

/// 读取文件开头生成预览。读取文件是阻塞操作，异步代码中要放在 `spawn_blocking` 里调用。
pub fn preview_file(path: &Path, name: &str, opts: &FileOptions) -> Result<FilePreview> {
    let mut reader = FileReader::open(path, name, opts)?;
    let mut rows = Vec::new();
    while rows.len() < INFER_ROWS {
        match reader.next_row()? {
            Some(r) => rows.push(r.values),
            None => break,
        }
    }
    let types = (0..reader.columns().len()).map(|k| infer_type(rows.iter().map(|r| &r[k]))).collect();
    rows.truncate(PREVIEW_ROWS);
    Ok(FilePreview {
        columns: reader.columns().to_vec(),
        rows,
        types,
        warnings: reader.warnings(),
        sheets: reader.sheets.clone(),
        encoding: reader.encoding,
        delimiter: reader.delimiter,
    })
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use chrono::{NaiveDate, NaiveDateTime};
    use dblens_core::schema::Column;
    use dblens_core::{ColumnType, Error, LogicalType, Value};
    use encoding_rs::GB18030;

    use super::file::{detect_delimiter, detect_encoding};
    use super::*;

    fn write(dir: &tempfile::TempDir, name: &str, bytes: &[u8]) -> PathBuf {
        let p = dir.path().join(name);
        std::fs::write(&p, bytes).unwrap();
        p
    }

    fn s(v: &str) -> Value {
        Value::String(v.into())
    }

    fn read_all(
        path: &std::path::Path,
        name: &str,
        opts: &FileOptions,
    ) -> (Vec<String>, Vec<FileRow>, ReadWarnings) {
        let mut r = FileReader::open(path, name, opts).unwrap();
        let mut rows = Vec::new();
        while let Some(row) = r.next_row().unwrap() {
            rows.push(row);
        }
        (r.columns().to_vec(), rows, r.warnings())
    }

    #[test]
    fn csv_utf8_with_bom_quotes_and_ragged_rows() {
        let dir = tempfile::tempdir().unwrap();
        let text = "\u{feff}编号,名称, 备注 ,名称\n1,\"含,逗号\",\"多\n行\",x\n2,b\n3,c,,y,多余\n";
        let p = write(&dir, "a.csv", text.as_bytes());
        let (cols, rows, w) = read_all(&p, "a.csv", &FileOptions::csv());
        assert_eq!(cols, ["编号", "名称", "备注", "名称_2"]);
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].values, [s("1"), s("含,逗号"), s("多\n行"), s("x")]);
        // 行号按记录计，列名行为第 1 行
        assert_eq!(rows.iter().map(|r| r.line).collect::<Vec<_>>(), [2, 3, 4]);
        // 缺少的列为 NULL，多出的忽略；空单元格为 NULL
        assert_eq!(rows[1].values, [s("2"), s("b"), Value::Null, Value::Null]);
        assert_eq!(rows[2].values, [s("3"), s("c"), Value::Null, s("y")]);
        assert_eq!(w.ragged_rows, 2);
    }

    #[test]
    fn csv_detects_gbk_and_delimiters() {
        let dir = tempfile::tempdir().unwrap();
        let (gbk, _, _) = GB18030.encode("名称;数量\n苹果;3\n香蕉;5\n");
        let p = write(&dir, "gbk.csv", &gbk);
        let preview = preview_file(&p, "gbk.csv", &FileOptions::csv()).unwrap();
        assert_eq!((preview.encoding, preview.delimiter), (Some("GBK"), Some(';')));
        assert_eq!(preview.columns, ["名称", "数量"]);
        assert_eq!(preview.rows[0], [s("苹果"), s("3")]);
        // 指定编码时不再判断
        let utf8 = FileOptions { encoding: TextEncoding::Utf8, ..FileOptions::csv() };
        let wrong = preview_file(&p, "gbk.csv", &utf8).unwrap();
        assert_eq!(wrong.encoding, Some("UTF-8"));
        assert!(wrong.warnings.bad_encoding > 0, "按 UTF-8 读 GBK 文件会出现替换字符");
        // .tsv 默认 Tab；判断分隔符时引号内的不算
        let p = write(&dir, "t.tsv", "a,b\tc\n1,2\t3\n".as_bytes());
        assert_eq!(preview_file(&p, "t.tsv", &FileOptions::csv()).unwrap().columns, ["a,b", "c"]);
        assert_eq!(detect_delimiter("a|b|c\n1|2|3\n"), '|');
        assert_eq!(detect_delimiter("\"x,y\";b\n1;2\n"), ';');
        assert_eq!(detect_delimiter("单列\n值\n"), ',');
        assert_eq!(detect_encoding(&[0xe4, 0xb8]), encoding_rs::UTF_8, "末尾被截断的多字节字符不算错");
    }

    #[test]
    fn csv_without_header_keeps_first_row() {
        let dir = tempfile::tempdir().unwrap();
        let p = write(&dir, "n.csv", b"1,a\n2,b\n");
        let opts = FileOptions { header: false, ..FileOptions::csv() };
        let (cols, rows, _) = read_all(&p, "n.csv", &opts);
        assert_eq!(cols, ["column_1", "column_2"]);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].line, 1);
    }

    #[test]
    fn excel_cells_keep_their_types() {
        use rust_xlsxwriter::{ExcelDateTime, Format, Workbook};
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("b.xlsx");
        let mut book = Workbook::new();
        let sheet = book.add_worksheet().set_name("数据").unwrap();
        let date = Format::new().set_num_format("yyyy-mm-dd");
        let datetime = Format::new().set_num_format("yyyy-mm-dd hh:mm");
        for (c, h) in ["数量", "价格", "日期", "时间", "有效", "名称"].iter().enumerate() {
            sheet.write_string(0, c as u16, *h).unwrap();
        }
        sheet.write_number(1, 0, 3).unwrap();
        sheet.write_number(1, 1, 1.25).unwrap();
        let day = ExcelDateTime::from_ymd(2026, 10, 27).unwrap();
        sheet.write_datetime_with_format(1, 2, day, &date).unwrap();
        let dt = ExcelDateTime::from_ymd(2026, 10, 27).unwrap().and_hms(8, 30, 0).unwrap();
        sheet.write_datetime_with_format(1, 3, &dt, &datetime).unwrap();
        sheet.write_boolean(1, 4, true).unwrap();
        sheet.write_string(1, 5, "苹果").unwrap();
        sheet.write_number(2, 0, 4).unwrap();
        book.add_worksheet().set_name("另一张").unwrap();
        book.save(&path).unwrap();

        let opts = FileOptions { format: FileFormat::Excel, ..FileOptions::csv() };
        let preview = preview_file(&path, "b.xlsx", &opts).unwrap();
        assert_eq!(preview.sheets, ["数据", "另一张"]);
        assert_eq!(preview.columns, ["数量", "价格", "日期", "时间", "有效", "名称"]);
        assert_eq!(
            preview.rows[0],
            [
                Value::Float(3.0),
                Value::Float(1.25),
                Value::Date(NaiveDate::from_ymd_opt(2026, 10, 27).unwrap()),
                Value::DateTime(
                    NaiveDate::from_ymd_opt(2026, 10, 27).unwrap().and_hms_opt(8, 30, 0).unwrap()
                ),
                Value::Bool(true),
                s("苹果"),
            ]
        );
        // 已用区域是矩形：第 3 行的空单元格为 NULL，不算列数不齐
        assert_eq!(preview.rows[1][1], Value::Null);
        assert_eq!(preview.warnings.ragged_rows, 0);
        assert_eq!(
            preview.types,
            [
                LogicalType::Int { bits: 64 },
                LogicalType::Decimal { precision: Some(10), scale: Some(2) },
                LogicalType::Date,
                LogicalType::Timestamp { tz: false, precision: None },
                LogicalType::Bool,
                LogicalType::VarChar { len: Some(50) },
            ]
        );
        let missing = FileOptions { sheet: Some("没有".into()), ..opts };
        assert!(matches!(FileReader::open(&path, "b.xlsx", &missing), Err(Error::Invalid { .. })));
    }

    #[test]
    fn excel_error_cells_read_as_null() {
        let mut w = ReadWarnings::default();
        let v = super::file::excel_value(&calamine::Data::Error(calamine::CellErrorType::NA), &mut w);
        assert_eq!((v, w.error_cells), (Value::Null, 1));
    }

    #[test]
    fn infers_types_in_order() {
        let t = |vals: &[&str]| {
            infer_type(
                vals.iter().map(|v| if v.is_empty() { Value::Null } else { s(v) }).collect::<Vec<_>>().iter(),
            )
        };
        assert_eq!(t(&["true", "FALSE", ""]), LogicalType::Bool);
        assert_eq!(t(&["1", "-20", "+3"]), LogicalType::Int { bits: 64 });
        assert_eq!(t(&["1", "2.5", "-30.125"]), LogicalType::Decimal { precision: Some(10), scale: Some(3) });
        let long = format!("1{}.5", "0".repeat(30));
        assert_eq!(t(&[&long]), LogicalType::Decimal { precision: Some(32), scale: Some(1) });
        assert_eq!(
            t(&[&format!("1{}", "0".repeat(40))]),
            LogicalType::VarChar { len: Some(82) },
            "超过 38 位用字符串"
        );
        assert_eq!(t(&["1e5"]), LogicalType::VarChar { len: Some(50) }, "带指数的不推断为数字");
        assert_eq!(t(&["2026-10-27", "2026/1/2"]), LogicalType::Date);
        assert_eq!(
            t(&["2026-10-27", "2026-10-27 08:30"]),
            LogicalType::Timestamp { tz: false, precision: None }
        );
        assert_eq!(t(&["a", "数洞数洞"]), LogicalType::VarChar { len: Some(50) });
        assert_eq!(t(&[&"字".repeat(30)]), LogicalType::VarChar { len: Some(60) });
        assert_eq!(t(&[&"x".repeat(2001)]), LogicalType::Text);
        assert_eq!(t(&["", ""]), LogicalType::VarChar { len: Some(50) }, "全空的列");
    }

    fn column(name: &str, native: &str, logical: LogicalType) -> Column {
        Column {
            name: name.into(),
            ordinal: 1,
            data_type: ColumnType { native: native.into(), logical },
            nullable: true,
            default: None,
            comment: None,
            auto_increment: false,
            generated: None,
        }
    }

    #[test]
    fn converts_values_by_column_type() {
        let c = |l: LogicalType| column("c", "t", l);
        let ok = |v: Value, l: LogicalType| convert(&v, &c(l), EmptyAs::Null).unwrap();
        let int32 = LogicalType::Int { bits: 32 };
        assert_eq!(ok(s(" 42 "), int32.clone()), Value::Int(42));
        assert_eq!(ok(Value::Float(7.0), int32.clone()), Value::Int(7), "Excel 的数字");
        assert!(convert(&s("3000000000"), &c(int32.clone()), EmptyAs::Null).is_err(), "超出 32 位");
        assert!(convert(&s("1.5"), &c(int32), EmptyAs::Null).is_err());
        assert_eq!(
            ok(s("18446744073709551615"), LogicalType::UInt { bits: 64 }),
            Value::Decimal("18446744073709551615".into())
        );
        let dec = LogicalType::Decimal { precision: Some(10), scale: Some(2) };
        assert_eq!(ok(s("12.30"), dec.clone()), Value::Decimal("12.30".into()), "不经过浮点");
        assert_eq!(ok(Value::Float(0.1), dec.clone()), Value::Decimal("0.1".into()));
        assert!(convert(&s("12,30"), &c(dec), EmptyAs::Null).is_err());
        assert_eq!(ok(s("是"), LogicalType::Bool), Value::Bool(true));
        assert_eq!(ok(s("0"), LogicalType::Bool), Value::Bool(false));
        let d = NaiveDate::from_ymd_opt(2026, 1, 2).unwrap();
        assert_eq!(ok(s("2026/1/2"), LogicalType::Date), Value::Date(d));
        let ts = LogicalType::Timestamp { tz: false, precision: None };
        let at = |h, m, sec| NaiveDateTime::new(d, chrono::NaiveTime::from_hms_opt(h, m, sec).unwrap());
        assert_eq!(ok(s("2026-01-02T08:30"), ts.clone()), Value::DateTime(at(8, 30, 0)));
        assert_eq!(ok(Value::Date(d), ts), Value::DateTime(at(0, 0, 0)));
        let tz = LogicalType::Timestamp { tz: true, precision: None };
        match ok(s("2026-01-02 08:30:00+08:00"), tz.clone()) {
            Value::DateTimeTz(t) => assert_eq!(t.to_rfc3339(), "2026-01-02T08:30:00+08:00"),
            v => panic!("{v:?}"),
        }
        match ok(s("2026-01-02 08:30:00Z"), tz.clone()) {
            Value::DateTimeTz(t) => assert_eq!(t.offset().local_minus_utc(), 0),
            v => panic!("{v:?}"),
        }
        assert!(matches!(ok(s("2026-01-02 08:30"), tz), Value::DateTimeTz(_)), "没有时区按本地时区");
        assert_eq!(ok(s("0x0aFF"), LogicalType::Blob), Value::Bytes(vec![0x0a, 0xff]));
        assert_eq!(ok(s("{\"a\": 1}"), LogicalType::Json), Value::Json(serde_json::json!({"a": 1})));
        assert_eq!(ok(Value::Float(1.5), LogicalType::Text), s("1.5"), "数字写入文本列");
        assert_eq!(ok(s("1 day"), LogicalType::Interval), s("1 day"));
    }

    #[test]
    fn empty_cells_and_error_messages() {
        let text = column("备注", "VARCHAR(20)", LogicalType::VarChar { len: Some(20) });
        let num = column("数量", "INTEGER", LogicalType::Int { bits: 32 });
        assert_eq!(convert(&Value::Null, &text, EmptyAs::Null).unwrap(), Value::Null);
        assert_eq!(convert(&Value::Null, &text, EmptyAs::EmptyString).unwrap(), s(""));
        assert_eq!(
            convert(&Value::Null, &num, EmptyAs::EmptyString).unwrap(),
            Value::Null,
            "非文本列仍为 NULL"
        );
        let err = convert(&s("abc"), &num, EmptyAs::Null).unwrap_err().to_string();
        assert!(err.contains("数量") && err.contains("abc") && err.contains("INTEGER"), "{err}");
    }
}
