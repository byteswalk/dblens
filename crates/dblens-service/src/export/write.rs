//! 导出文件的写入（M1c3 设计文档 3.4 节）：CSV、xlsx、INSERT、JSON。只管把列和行写进文件，不涉及数据库。
//! 每种格式实现 `RowWriter`：先 `begin` 写表头，再分批 `rows`，最后 `finish`。

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use dblens_core::{Error, Result, Row, Value, msg};
use dblens_dialect::Dialect;
use dblens_driver::ColumnMeta;
use rust_xlsxwriter::{Format, Workbook, XlsxError};
use serde::{Deserialize, Serialize};

/// 导出格式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExportFormat {
    Csv,
    Xlsx,
    Insert,
    Json,
}

impl ExportFormat {
    pub fn extension(self) -> &'static str {
        match self {
            Self::Csv => "csv",
            Self::Xlsx => "xlsx",
            Self::Insert => "sql",
            Self::Json => "json",
        }
    }
}

/// 导出选项。只有对应格式用到的字段起作用。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ExportOptions {
    /// CSV 的分隔符：`,`、`\t` 或 `;`。
    pub delimiter: char,
    /// CSV 是否写表头。
    pub header: bool,
    /// CSV 是否在开头写 UTF-8 BOM（Excel 直接打开中文不乱码）。
    pub bom: bool,
    /// CSV 中 NULL 写成的文字。
    pub null_text: String,
    /// INSERT 的目标表（原样写入，需要时由用户加引号）。
    pub table: String,
    /// INSERT 每条语句的行数；大于 1 时多行合并成一条。
    pub rows_per_insert: usize,
}

impl Default for ExportOptions {
    fn default() -> Self {
        Self {
            delimiter: ',',
            header: true,
            bom: true,
            null_text: String::new(),
            table: "table_name".into(),
            rows_per_insert: 1,
        }
    }
}

/// 导出中出现的、不中断导出的问题。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct ExportWarnings {
    /// 超过读取上限、只写出了前一部分的大字段个数。
    pub lob_truncated: u64,
    /// xlsx 中超过 32767 个字符被截断的单元格个数。
    pub cell_truncated: u64,
}

pub trait RowWriter: Send {
    fn begin(&mut self, columns: &[ColumnMeta]) -> Result<()>;
    fn rows(&mut self, rows: &[Row]) -> Result<()>;
    /// 写完并关闭文件，返回过程中的警告。
    fn finish(self: Box<Self>) -> Result<ExportWarnings>;
}

/// 为 `path` 新建对应格式的写入器。INSERT 的值按 `dialect` 的写法生成。
pub fn writer(
    format: ExportFormat,
    options: &ExportOptions,
    path: &Path,
    dialect: Dialect,
) -> Result<Box<dyn RowWriter>> {
    Ok(match format {
        ExportFormat::Csv => Box::new(CsvWriter::new(path, options)?),
        ExportFormat::Xlsx => Box::new(XlsxWriter::new(path)?),
        ExportFormat::Insert => Box::new(InsertWriter::new(path, options, dialect)?),
        ExportFormat::Json => Box::new(JsonWriter::new(path)?),
    })
}

fn io_error(e: impl std::fmt::Display) -> Error {
    Error::internal(msg!("export.write_failed", error = e))
}

fn create(path: &Path) -> Result<BufWriter<File>> {
    Ok(BufWriter::with_capacity(256 * 1024, File::create(path).map_err(io_error)?))
}

/// 单元格的文本（与前端结果表格的显示一致）；NULL 为 None。二进制写成 base64，大字段写出已读取的部分。
pub fn cell_text(v: &Value, warnings: &mut ExportWarnings) -> Option<String> {
    Some(match v {
        Value::Null => return None,
        Value::Bool(b) => b.to_string(),
        Value::Int(i) => i.to_string(),
        Value::UInt(u) => u.to_string(),
        Value::Float(f) => f.to_string(),
        Value::Decimal(s) | Value::String(s) | Value::Interval(s) => s.clone(),
        Value::Bytes(b) => STANDARD.encode(b),
        Value::Date(d) => d.format("%Y-%m-%d").to_string(),
        Value::Time(t) => t.format("%H:%M:%S%.f").to_string(),
        Value::DateTime(dt) => dt.format("%Y-%m-%d %H:%M:%S%.f").to_string(),
        Value::DateTimeTz(dt) => dt.format("%Y-%m-%d %H:%M:%S%.f%:z").to_string(),
        Value::Json(j) => j.to_string(),
        Value::Uuid(u) => u.to_string(),
        Value::Lob(lob) => {
            if lob.truncated {
                warnings.lob_truncated += 1;
            }
            lob.preview.clone()
        }
        Value::Other { text, .. } => text.clone(),
    })
}

// ---- CSV ----

struct CsvWriter {
    out: csv::Writer<BufWriter<File>>,
    header: bool,
    null_text: String,
    warnings: ExportWarnings,
}

impl CsvWriter {
    fn new(path: &Path, options: &ExportOptions) -> Result<Self> {
        let delimiter = u8::try_from(options.delimiter)
            .ok()
            .filter(|d| matches!(d, b',' | b'\t' | b';'))
            .ok_or_else(|| Error::invalid(msg!("export.bad_delimiter")))?;
        let mut file = create(path)?;
        if options.bom {
            file.write_all("\u{feff}".as_bytes()).map_err(io_error)?;
        }
        Ok(Self {
            out: csv::WriterBuilder::new().delimiter(delimiter).from_writer(file),
            header: options.header,
            null_text: options.null_text.clone(),
            warnings: ExportWarnings::default(),
        })
    }
}

impl RowWriter for CsvWriter {
    fn begin(&mut self, columns: &[ColumnMeta]) -> Result<()> {
        if self.header {
            self.out.write_record(columns.iter().map(|c| c.name.as_str())).map_err(io_error)?;
        }
        Ok(())
    }

    fn rows(&mut self, rows: &[Row]) -> Result<()> {
        for row in rows {
            let fields: Vec<String> = row
                .iter()
                .map(|v| cell_text(v, &mut self.warnings).unwrap_or_else(|| self.null_text.clone()))
                .collect();
            self.out.write_record(&fields).map_err(io_error)?;
        }
        Ok(())
    }

    fn finish(mut self: Box<Self>) -> Result<ExportWarnings> {
        self.out.flush().map_err(io_error)?;
        Ok(self.warnings)
    }
}

// ---- xlsx ----

/// 单个工作表的行数上限（1048576 行，其中一行是表头）。
const XLSX_MAX_ROWS: u32 = 1_048_576;
/// 单元格最多 32767 个字符。
const XLSX_MAX_CHARS: usize = 32_767;

struct XlsxWriter {
    path: std::path::PathBuf,
    book: Workbook,
    columns: Vec<String>,
    /// 当前工作表的下标和下一行的行号。
    sheet: usize,
    row: u32,
    header: Format,
    date: Format,
    time: Format,
    datetime: Format,
    warnings: ExportWarnings,
}

fn xlsx_error(e: XlsxError) -> Error {
    io_error(e)
}

impl XlsxWriter {
    fn new(path: &Path) -> Result<Self> {
        // 先建一个空文件：路径不可写时立即报错，而不是写完全部数据后才发现
        File::create(path).map_err(io_error)?;
        Ok(Self {
            path: path.to_path_buf(),
            book: Workbook::new(),
            columns: Vec::new(),
            sheet: 0,
            row: 0,
            header: Format::new().set_bold(),
            date: Format::new().set_num_format("yyyy-mm-dd"),
            time: Format::new().set_num_format("hh:mm:ss"),
            datetime: Format::new().set_num_format("yyyy-mm-dd hh:mm:ss"),
            warnings: ExportWarnings::default(),
        })
    }

    /// 新开一个工作表（低内存模式：按行顺序写入，写过的行落到临时文件），写上表头。
    fn new_sheet(&mut self) -> Result<()> {
        self.book.add_worksheet_with_constant_memory().map_err(xlsx_error)?;
        self.sheet = self.book.worksheets_mut().len() - 1;
        let ws = self.book.worksheet_from_index(self.sheet).map_err(xlsx_error)?;
        for (i, name) in self.columns.iter().enumerate() {
            ws.write_string_with_format(0, i as u16, name.as_str(), &self.header).map_err(xlsx_error)?;
        }
        self.row = 1;
        Ok(())
    }
}

impl RowWriter for XlsxWriter {
    fn begin(&mut self, columns: &[ColumnMeta]) -> Result<()> {
        self.columns = columns.iter().map(|c| c.name.clone()).collect();
        self.new_sheet()
    }

    fn rows(&mut self, rows: &[Row]) -> Result<()> {
        for row in rows {
            if self.row >= XLSX_MAX_ROWS {
                self.new_sheet()?;
            }
            let r = self.row;
            let ws = self.book.worksheet_from_index(self.sheet).map_err(xlsx_error)?;
            for (i, v) in row.iter().enumerate() {
                let c = i as u16;
                match v {
                    Value::Null => {}
                    Value::Bool(b) => {
                        ws.write_boolean(r, c, *b).map_err(xlsx_error)?;
                    }
                    Value::Int(n) => {
                        ws.write_number(r, c, *n as f64).map_err(xlsx_error)?;
                    }
                    Value::UInt(n) => {
                        ws.write_number(r, c, *n as f64).map_err(xlsx_error)?;
                    }
                    Value::Float(f) if f.is_finite() => {
                        ws.write_number(r, c, *f).map_err(xlsx_error)?;
                    }
                    Value::Date(d) => {
                        ws.write_datetime_with_format(r, c, d, &self.date).map_err(xlsx_error)?;
                    }
                    Value::Time(t) => {
                        ws.write_datetime_with_format(r, c, t, &self.time).map_err(xlsx_error)?;
                    }
                    Value::DateTime(dt) => {
                        ws.write_datetime_with_format(r, c, dt, &self.datetime).map_err(xlsx_error)?;
                    }
                    Value::Bytes(b) => {
                        ws.write_string(r, c, msg!("export.binary_cell", bytes = b.len()).current())
                            .map_err(xlsx_error)?;
                    }
                    Value::Lob(lob) if lob.binary => {
                        ws.write_string(r, c, msg!("export.binary_cell", bytes = lob.total_len).current())
                            .map_err(xlsx_error)?;
                    }
                    // 精确小数写成文本，避免经过浮点丢精度
                    other => {
                        let mut text = cell_text(other, &mut self.warnings).unwrap_or_default();
                        if text.chars().count() > XLSX_MAX_CHARS {
                            text = text.chars().take(XLSX_MAX_CHARS).collect();
                            self.warnings.cell_truncated += 1;
                        }
                        ws.write_string(r, c, text).map_err(xlsx_error)?;
                    }
                }
            }
            self.row += 1;
        }
        Ok(())
    }

    fn finish(mut self: Box<Self>) -> Result<ExportWarnings> {
        if self.book.worksheets_mut().is_empty() {
            self.new_sheet()?;
        }
        self.book.save(&self.path).map_err(xlsx_error)?;
        Ok(self.warnings)
    }
}

// ---- INSERT ----

struct InsertWriter {
    out: BufWriter<File>,
    dialect: Dialect,
    table: String,
    per_statement: usize,
    columns: String,
    /// 当前语句已写的行数。
    pending: usize,
    warnings: ExportWarnings,
}

impl InsertWriter {
    fn new(path: &Path, options: &ExportOptions, dialect: Dialect) -> Result<Self> {
        let table = options.table.trim();
        if table.is_empty() {
            return Err(Error::invalid(msg!("export.no_table")));
        }
        Ok(Self {
            out: create(path)?,
            dialect,
            table: table.to_string(),
            per_statement: options.rows_per_insert.max(1),
            columns: String::new(),
            pending: 0,
            warnings: ExportWarnings::default(),
        })
    }

    fn value(&mut self, v: &Value) -> Result<String> {
        match v {
            // 已读到的部分照样写出，记一次截断
            Value::Lob(lob) => {
                if lob.truncated {
                    self.warnings.lob_truncated += 1;
                }
                if lob.binary {
                    let bytes = STANDARD.decode(&lob.preview).unwrap_or_default();
                    self.dialect.literal(&Value::Bytes(bytes))
                } else {
                    self.dialect.literal(&Value::String(lob.preview.clone()))
                }
            }
            Value::Other { text, .. } => self.dialect.literal(&Value::String(text.clone())),
            other => self.dialect.literal(other),
        }
    }

    /// 结束当前的多行语句。
    fn close_statement(&mut self) -> Result<()> {
        if self.pending > 0 {
            self.out.write_all(b";\n").map_err(io_error)?;
            self.pending = 0;
        }
        Ok(())
    }
}

impl RowWriter for InsertWriter {
    fn begin(&mut self, columns: &[ColumnMeta]) -> Result<()> {
        self.columns = columns.iter().map(|c| self.dialect.ident(&c.name)).collect::<Vec<_>>().join(", ");
        Ok(())
    }

    fn rows(&mut self, rows: &[Row]) -> Result<()> {
        for row in rows {
            let values = row.iter().map(|v| self.value(v)).collect::<Result<Vec<_>>>()?.join(", ");
            let line = if self.per_statement == 1 {
                format!("INSERT INTO {} ({}) VALUES ({values});\n", self.table, self.columns)
            } else if self.pending == 0 {
                format!("INSERT INTO {} ({}) VALUES\n  ({values})", self.table, self.columns)
            } else {
                format!(",\n  ({values})")
            };
            self.out.write_all(line.as_bytes()).map_err(io_error)?;
            if self.per_statement > 1 {
                self.pending += 1;
                if self.pending == self.per_statement {
                    self.close_statement()?;
                }
            }
        }
        Ok(())
    }

    fn finish(mut self: Box<Self>) -> Result<ExportWarnings> {
        self.close_statement()?;
        self.out.flush().map_err(io_error)?;
        Ok(self.warnings)
    }
}

// ---- JSON ----

struct JsonWriter {
    out: BufWriter<File>,
    /// 列名；重复的依次加 `_2`、`_3`。
    keys: Vec<String>,
    first: bool,
    warnings: ExportWarnings,
}

impl JsonWriter {
    fn new(path: &Path) -> Result<Self> {
        Ok(Self { out: create(path)?, keys: Vec::new(), first: true, warnings: ExportWarnings::default() })
    }
}

/// 重复的列名依次加后缀 `_2`、`_3`，保证 JSON 对象的键唯一。
pub fn unique_keys(columns: &[ColumnMeta]) -> Vec<String> {
    let mut keys: Vec<String> = Vec::with_capacity(columns.len());
    for c in columns {
        let mut key = c.name.clone();
        let mut n = 1;
        while keys.contains(&key) {
            n += 1;
            key = format!("{}_{n}", c.name);
        }
        keys.push(key);
    }
    keys
}

fn json_value(v: &Value, warnings: &mut ExportWarnings) -> serde_json::Value {
    use serde_json::Value as J;
    match v {
        Value::Null => J::Null,
        Value::Bool(b) => J::Bool(*b),
        Value::Int(i) => J::from(*i),
        Value::UInt(u) => J::from(*u),
        Value::Float(f) => serde_json::Number::from_f64(*f).map(J::Number).unwrap_or(J::Null),
        Value::Json(j) => j.clone(),
        other => cell_text(other, warnings).map(J::String).unwrap_or(J::Null),
    }
}

impl RowWriter for JsonWriter {
    fn begin(&mut self, columns: &[ColumnMeta]) -> Result<()> {
        self.keys = unique_keys(columns);
        self.out.write_all(b"[").map_err(io_error)?;
        Ok(())
    }

    fn rows(&mut self, rows: &[Row]) -> Result<()> {
        for row in rows {
            let obj: serde_json::Map<String, serde_json::Value> = self
                .keys
                .iter()
                .cloned()
                .zip(row.iter().map(|v| json_value(v, &mut self.warnings)))
                .collect();
            self.out.write_all(if self.first { b"\n  " } else { b",\n  " }).map_err(io_error)?;
            self.first = false;
            serde_json::to_writer(&mut self.out, &obj).map_err(io_error)?;
        }
        Ok(())
    }

    fn finish(mut self: Box<Self>) -> Result<ExportWarnings> {
        if self.keys.is_empty() && self.first {
            // 没有结果集时也写出合法的 JSON
            self.out.write_all(b"[").map_err(io_error)?;
        }
        self.out.write_all(if self.first { b"]\n" } else { b"\n]\n" }).map_err(io_error)?;
        self.out.flush().map_err(io_error)?;
        Ok(self.warnings)
    }
}

#[cfg(test)]
mod tests {
    use chrono::NaiveDate;
    use dblens_core::LobPreview;
    use dblens_dialect::Variant;

    use super::*;

    fn cols(names: &[&str]) -> Vec<ColumnMeta> {
        names.iter().map(|n| ColumnMeta::new(*n, "text")).collect()
    }

    fn sample() -> Vec<Row> {
        vec![
            vec![Value::Int(1), Value::String("a,\"b\"\n中文".into()), Value::Null],
            vec![
                Value::Int(2),
                Value::Bytes(vec![0, 255]),
                Value::Date(NaiveDate::from_ymd_opt(2026, 10, 26).unwrap()),
            ],
        ]
    }

    fn run(
        format: ExportFormat,
        options: &ExportOptions,
        columns: &[ColumnMeta],
        rows: &[Row],
    ) -> (Vec<u8>, ExportWarnings) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(format!("out.{}", format.extension()));
        let mut w = writer(format, options, &path, Dialect::new(Variant::Postgres)).unwrap();
        w.begin(columns).unwrap();
        // 分两批写，检查跨批次的状态
        let (first, rest) = rows.split_at(rows.len().min(1));
        w.rows(first).unwrap();
        w.rows(rest).unwrap();
        let warnings = w.finish().unwrap();
        (std::fs::read(&path).unwrap(), warnings)
    }

    #[test]
    fn csv_escapes_and_bom() {
        let (bytes, _) =
            run(ExportFormat::Csv, &ExportOptions::default(), &cols(&["id", "name", "d"]), &sample());
        let text = String::from_utf8(bytes).unwrap();
        assert_eq!(text, "\u{feff}id,name,d\n1,\"a,\"\"b\"\"\n中文\",\n2,AP8=,2026-10-26\n");
    }

    #[test]
    fn csv_options() {
        let options = ExportOptions {
            delimiter: '\t',
            header: false,
            bom: false,
            null_text: "NULL".into(),
            ..Default::default()
        };
        let (bytes, _) = run(ExportFormat::Csv, &options, &cols(&["id", "name", "d"]), &sample()[..1]);
        assert_eq!(String::from_utf8(bytes).unwrap(), "1\t\"a,\"\"b\"\"\n中文\"\tNULL\n");
        let bad = ExportOptions { delimiter: '|', ..Default::default() };
        let dir = tempfile::tempdir().unwrap();
        assert!(
            writer(ExportFormat::Csv, &bad, &dir.path().join("x.csv"), Dialect::new(Variant::MySql)).is_err()
        );
    }

    #[test]
    fn insert_single_and_multi_row() {
        let options = ExportOptions { table: "public.t".into(), ..Default::default() };
        let (bytes, _) = run(ExportFormat::Insert, &options, &cols(&["id", "name", "d"]), &sample());
        assert_eq!(
            String::from_utf8(bytes).unwrap(),
            "INSERT INTO public.t (id, name, d) VALUES (1, 'a,\"b\"\n中文', NULL);\n\
             INSERT INTO public.t (id, name, d) VALUES (2, decode('00FF','hex'), DATE '2026-10-26');\n"
        );
        let options = ExportOptions { table: "t".into(), rows_per_insert: 2, ..Default::default() };
        let rows: Vec<Row> = (1..=3).map(|i| vec![Value::Int(i)]).collect();
        let (bytes, _) = run(ExportFormat::Insert, &options, &cols(&["id"]), &rows);
        assert_eq!(
            String::from_utf8(bytes).unwrap(),
            "INSERT INTO t (id) VALUES\n  (1),\n  (2);\nINSERT INTO t (id) VALUES\n  (3);\n"
        );
    }

    #[test]
    fn json_types_and_duplicate_keys() {
        let rows = vec![vec![
            Value::Int(1),
            Value::Decimal("1.50".into()),
            Value::Null,
            Value::Json(serde_json::json!({"k": 1})),
        ]];
        let (bytes, _) =
            run(ExportFormat::Json, &ExportOptions::default(), &cols(&["id", "id", "n", "j"]), &rows);
        let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v, serde_json::json!([{"id": 1, "id_2": "1.50", "n": null, "j": {"k": 1}}]));
        let (bytes, _) = run(ExportFormat::Json, &ExportOptions::default(), &cols(&["id"]), &[]);
        assert_eq!(serde_json::from_slice::<serde_json::Value>(&bytes).unwrap(), serde_json::json!([]));
    }

    #[test]
    fn truncated_lob_is_counted() {
        let lob =
            Value::Lob(LobPreview { binary: false, preview: "abc".into(), total_len: 9, truncated: true });
        let (bytes, warnings) = run(
            ExportFormat::Csv,
            &ExportOptions { bom: false, ..Default::default() },
            &cols(&["c"]),
            &[vec![lob.clone()], vec![lob]],
        );
        assert_eq!(String::from_utf8(bytes).unwrap(), "c\nabc\nabc\n");
        assert_eq!(warnings.lob_truncated, 2);
    }

    #[test]
    fn xlsx_writes_and_truncates_long_text() {
        let long = "长".repeat(XLSX_MAX_CHARS + 5);
        let rows = vec![vec![Value::Int(1), Value::String(long)], vec![Value::Float(1.5), Value::Null]];
        let (bytes, warnings) = run(ExportFormat::Xlsx, &ExportOptions::default(), &cols(&["n", "s"]), &rows);
        assert_eq!(&bytes[..2], b"PK", "xlsx 是 zip 文件");
        assert_eq!(warnings.cell_truncated, 1);
    }
}
