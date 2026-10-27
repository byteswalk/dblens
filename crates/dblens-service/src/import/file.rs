//! 读取导入文件（M2b 设计文档 4.2–4.4 节）：CSV 流式读取（自动判断编码和分隔符），Excel 用 calamine。
//! 单元格统一用 `Value`：CSV 的单元格是文本（空的为 NULL），Excel 的单元格按文件中的类型。

use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use calamine::{Data, Range, Reader, open_workbook_auto};
use chrono::NaiveTime;
use dblens_core::{Error, Result, Value, msg};
use encoding_rs::{Encoding, GB18030, UTF_8, UTF_16BE, UTF_16LE};
use encoding_rs_io::DecodeReaderBytesBuilder;
use serde::{Deserialize, Serialize};

/// 判断编码时读取的字节数。
const SNIFF_BYTES: usize = 64 * 1024;
/// 判断分隔符时看的行数。
const SNIFF_LINES: usize = 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileFormat {
    Csv,
    Excel,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextEncoding {
    #[default]
    Auto,
    Utf8,
    Gbk,
}

/// 文件的读取选项。
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct FileOptions {
    pub format: FileFormat,
    #[serde(default)]
    pub encoding: TextEncoding,
    /// CSV 的分隔符；不给时自动判断（`.tsv` 文件为 Tab）。
    #[serde(default)]
    pub delimiter: Option<char>,
    /// 首行是列名。
    #[serde(default = "yes")]
    pub header: bool,
    /// Excel 的工作表；不给时取第一个。
    #[serde(default)]
    pub sheet: Option<String>,
}

fn yes() -> bool {
    true
}

impl FileOptions {
    pub fn csv() -> Self {
        Self {
            format: FileFormat::Csv,
            encoding: TextEncoding::Auto,
            delimiter: None,
            header: true,
            sheet: None,
        }
    }
}

/// 读取中的警告计数。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct ReadWarnings {
    /// 列数与列名行不一致的行（多出的列忽略，缺少的列为 NULL）。
    pub ragged_rows: u64,
    /// 转码失败、替换成 `�` 的单元格。
    pub bad_encoding: u64,
    /// Excel 的错误值（`#N/A` 等），按 NULL 读取。
    pub error_cells: u64,
}

/// 文件中的一行。`line` 为行号：CSV 按记录计（首行为第 1 行），Excel 为工作表的行号。
#[derive(Debug, Clone, PartialEq)]
pub struct FileRow {
    pub line: u64,
    pub values: Vec<Value>,
}

/// 统计读过的字节数（进度用）。
struct Counting<R> {
    inner: R,
    read: Arc<AtomicU64>,
}

impl<R: Read> Read for Counting<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.inner.read(buf)?;
        self.read.fetch_add(n as u64, Ordering::Relaxed);
        Ok(n)
    }
}

type CsvReader = csv::Reader<Box<dyn Read + Send>>;

enum Inner {
    Csv { reader: CsvReader, record: csv::StringRecord, line: u64 },
    Excel { range: Range<Data>, next: usize },
}

/// 打开的导入文件：已读过列名行（或按首行的列数生成了列名）。
pub struct FileReader {
    columns: Vec<String>,
    inner: Inner,
    /// 首行不是列名时，先交出读到的首行。
    pending: Option<FileRow>,
    bytes: Arc<AtomicU64>,
    total: u64,
    warnings: ReadWarnings,
    /// 实际使用的编码和分隔符（CSV）。
    pub encoding: Option<&'static str>,
    pub delimiter: Option<char>,
    /// Excel 的全部工作表名。
    pub sheets: Vec<String>,
}

fn read_error(e: impl std::fmt::Display) -> Error {
    Error::invalid(msg!("import.read_failed", error = e))
}

/// 判断编码：有 BOM 按 BOM；否则开头能按 UTF-8 解码（末尾被截断的多字节字符不算错）就用 UTF-8，不能就用 GB18030。
pub fn detect_encoding(head: &[u8]) -> &'static Encoding {
    if let Some((enc, _)) = Encoding::for_bom(head) {
        return enc;
    }
    match std::str::from_utf8(head) {
        Ok(_) => UTF_8,
        Err(e) if e.error_len().is_none() => UTF_8,
        Err(_) => GB18030,
    }
}

/// 判断分隔符：前 20 行中（引号内的不算）每行出现次数相同且最多的候选字符；都不满足时用逗号。
pub fn detect_delimiter(sample: &str) -> char {
    let lines: Vec<&str> = sample.lines().filter(|l| !l.trim().is_empty()).take(SNIFF_LINES).collect();
    // 最后一行可能被截断，有多行时不用它
    let lines =
        if lines.len() > 1 && !sample.ends_with('\n') { &lines[..lines.len() - 1] } else { &lines[..] };
    let mut best = (',', 0);
    for d in [',', ';', '\t', '|'] {
        let counts: Vec<usize> = lines
            .iter()
            .map(|l| {
                let mut quoted = false;
                l.chars()
                    .filter(|&c| {
                        if c == '"' {
                            quoted = !quoted;
                        }
                        c == d && !quoted
                    })
                    .count()
            })
            .collect();
        if let Some(&first) = counts.first()
            && first > 0
            && counts.iter().all(|&n| n == first)
            && first > best.1
        {
            best = (d, first);
        }
    }
    best.0
}

/// 列名：去掉首尾空白，空的改为 `column_n`，重名的加后缀 `_2`、`_3`。
pub fn clean_names(raw: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for (k, name) in raw.into_iter().enumerate() {
        let base = match name.trim() {
            "" => format!("column_{}", k + 1),
            t => t.to_string(),
        };
        let mut name = base.clone();
        let mut n = 2;
        while out.iter().any(|o| o.eq_ignore_ascii_case(&name)) {
            name = format!("{base}_{n}");
            n += 1;
        }
        out.push(name);
    }
    out
}

/// Excel 单元格 → 值。日期时间的时间为 0 点时当作日期。
pub(crate) fn excel_value(d: &Data, warnings: &mut ReadWarnings) -> Value {
    match d {
        Data::Empty => Value::Null,
        Data::Int(n) => Value::Int(*n),
        Data::Float(f) => Value::Float(*f),
        Data::String(s) if s.is_empty() => Value::Null,
        Data::String(s) => Value::String(s.clone()),
        Data::Bool(b) => Value::Bool(*b),
        Data::DateTime(dt) => match dt.as_datetime() {
            Some(t) if t.time() == NaiveTime::MIN => Value::Date(t.date()),
            Some(t) => Value::DateTime(t),
            None => Value::String(dt.to_string()),
        },
        Data::DateTimeIso(s) | Data::DurationIso(s) => Value::String(s.clone()),
        Data::Error(_) => {
            warnings.error_cells += 1;
            Value::Null
        }
    }
}

impl FileReader {
    /// 打开文件，读取列名。`name` 是原文件名（判断 `.tsv` 用）。
    pub fn open(path: &Path, name: &str, opts: &FileOptions) -> Result<Self> {
        match opts.format {
            FileFormat::Csv => Self::open_csv(path, name, opts),
            FileFormat::Excel => Self::open_excel(path, opts),
        }
    }

    fn open_csv(path: &Path, name: &str, opts: &FileOptions) -> Result<Self> {
        let mut file = File::open(path).map_err(read_error)?;
        let total = file.metadata().map_err(read_error)?.len();
        let mut head = vec![0u8; SNIFF_BYTES];
        let mut n = 0;
        while n < head.len() {
            match file.read(&mut head[n..]).map_err(read_error)? {
                0 => break,
                k => n += k,
            }
        }
        head.truncate(n);
        let encoding = match opts.encoding {
            TextEncoding::Auto => detect_encoding(&head),
            TextEncoding::Utf8 => UTF_8,
            TextEncoding::Gbk => GB18030,
        };
        let delimiter = match opts.delimiter {
            Some(d) => d,
            None if name.to_ascii_lowercase().ends_with(".tsv") => '\t',
            None => {
                let (sample, _, _) = encoding.decode(&head);
                detect_delimiter(&sample)
            }
        };
        if !delimiter.is_ascii() {
            return Err(Error::invalid(msg!("import.bad_delimiter", delimiter = delimiter)));
        }
        let bytes = Arc::new(AtomicU64::new(0));
        let raw = Counting { inner: std::io::Cursor::new(head).chain(file), read: bytes.clone() };
        // BOM 优先于选择的编码；输出为 UTF-8，无法转码的字节替换为 U+FFFD
        let decoded = DecodeReaderBytesBuilder::new().encoding(Some(encoding)).bom_override(true).build(raw);
        let reader: Box<dyn Read + Send> = Box::new(BufReader::new(decoded));
        let reader = csv::ReaderBuilder::new()
            .has_headers(false)
            .flexible(true)
            .delimiter(delimiter as u8)
            .from_reader(reader);
        let label = match encoding {
            e if e == UTF_16LE || e == UTF_16BE => "UTF-16",
            e if e == GB18030 => "GBK",
            _ => "UTF-8",
        };
        let mut me = Self {
            columns: Vec::new(),
            inner: Inner::Csv { reader, record: csv::StringRecord::new(), line: 0 },
            pending: None,
            bytes,
            total,
            warnings: ReadWarnings::default(),
            encoding: Some(label),
            delimiter: Some(delimiter),
            sheets: Vec::new(),
        };
        me.read_header(opts.header)?;
        Ok(me)
    }

    fn open_excel(path: &Path, opts: &FileOptions) -> Result<Self> {
        let mut book = open_workbook_auto(path).map_err(read_error)?;
        let sheets = book.sheet_names();
        let sheet = match &opts.sheet {
            Some(s) if sheets.contains(s) => s.clone(),
            Some(s) => return Err(Error::invalid(msg!("import.no_sheet", sheet = s))),
            None => {
                sheets.first().cloned().ok_or_else(|| Error::invalid(msg!("import.no_sheet", sheet = "")))?
            }
        };
        let range = book.worksheet_range(&sheet).map_err(read_error)?;
        let total = range.height() as u64;
        let mut me = Self {
            columns: Vec::new(),
            inner: Inner::Excel { range, next: 0 },
            pending: None,
            bytes: Arc::new(AtomicU64::new(0)),
            total,
            warnings: ReadWarnings::default(),
            encoding: None,
            delimiter: None,
            sheets,
        };
        me.read_header(opts.header)?;
        Ok(me)
    }

    /// 首行是列名时读作列名；否则按首行的列数生成 `column_n`，首行留作数据。
    fn read_header(&mut self, header: bool) -> Result<()> {
        let Some(first) = self.raw_row()? else { return Ok(()) };
        if header {
            let names = first.values.iter().map(|v| match v {
                Value::Null => String::new(),
                Value::String(s) => s.clone(),
                other => super::text(other),
            });
            self.columns = clean_names(names);
        } else {
            self.columns = clean_names(vec![String::new(); first.values.len()]);
            self.pending = Some(first);
        }
        Ok(())
    }

    /// 读一行原样的值（列数不整理）。
    fn raw_row(&mut self) -> Result<Option<FileRow>> {
        match &mut self.inner {
            Inner::Csv { reader, record, line } => {
                if !reader.read_record(record).map_err(read_error)? {
                    return Ok(None);
                }
                *line += 1;
                let mut values = Vec::with_capacity(record.len());
                for field in record.iter() {
                    if field.contains('\u{fffd}') {
                        self.warnings.bad_encoding += 1;
                    }
                    values.push(if field.is_empty() {
                        Value::Null
                    } else {
                        Value::String(field.to_string())
                    });
                }
                Ok(Some(FileRow { line: *line, values }))
            }
            Inner::Excel { range, next } => {
                let width = range.width();
                let Some(row) = range.rows().nth(*next) else { return Ok(None) };
                let line = range.start().map_or(0, |(r, _)| r as u64) + *next as u64 + 1;
                *next += 1;
                let mut values = Vec::with_capacity(width);
                for d in row {
                    values.push(excel_value(d, &mut self.warnings));
                }
                Ok(Some(FileRow { line, values }))
            }
        }
    }

    pub fn columns(&self) -> &[String] {
        &self.columns
    }

    /// 下一行数据，值的个数与列数相同（多出的忽略，缺少的为 NULL，计入警告）。
    pub fn next_row(&mut self) -> Result<Option<FileRow>> {
        let row = match self.pending.take() {
            Some(r) => Some(r),
            None => self.raw_row()?,
        };
        let Some(mut row) = row else { return Ok(None) };
        let width = self.columns.len();
        if row.values.len() != width {
            // Excel 的已用区域是矩形，行尾的空单元格不算列数不齐
            if matches!(self.inner, Inner::Csv { .. }) {
                self.warnings.ragged_rows += 1;
            }
            row.values.resize(width, Value::Null);
        }
        Ok(Some(row))
    }

    /// 进度：CSV 为已读字节和文件大小，Excel 为已读行数和总行数。
    pub fn progress(&self) -> (u64, u64) {
        match &self.inner {
            Inner::Csv { .. } => (self.bytes.load(Ordering::Relaxed).min(self.total), self.total),
            Inner::Excel { next, .. } => (*next as u64, self.total),
        }
    }

    pub fn warnings(&self) -> ReadWarnings {
        self.warnings
    }
}
