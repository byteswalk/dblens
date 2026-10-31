//! 数据字典的 xlsx（M4c 设计文档 4.3 节）：表清单、列、索引和约束三张工作表。

use dblens_core::{Error, Lang, Result, msg};
use rust_xlsxwriter::{Format, Url, Workbook, Worksheet, XlsxError};

use super::content::DocContent;
use super::labels::label;

/// 单元格最多 32767 个字符。
const CELL_MAX: usize = 32767;
/// 自动列宽的上限（字符宽度）。
const WIDTH_MAX: usize = 60;

pub(crate) fn xlsx_error(e: XlsxError) -> Error {
    Error::internal(msg!("document.write_failed", error = e))
}

/// 一张工作表的写出：记下各列的宽度，统计截断的单元格。
pub(crate) struct Sheet<'a> {
    ws: &'a mut Worksheet,
    widths: Vec<usize>,
    row: u32,
    truncated: &'a mut u64,
}

/// 显示宽度：中日韩等宽字符算 2。
fn display_width(s: &str) -> usize {
    s.chars().map(|c| if (c as u32) >= 0x1100 { 2 } else { 1 }).sum()
}

impl<'a> Sheet<'a> {
    pub(crate) fn new(ws: &'a mut Worksheet, truncated: &'a mut u64) -> Self {
        Self { ws, widths: vec![], row: 0, truncated }
    }

    pub(crate) fn header(&mut self, headers: &[String], bold: &Format) -> Result<()> {
        for (c, h) in headers.iter().enumerate() {
            self.ws.write_string_with_format(0, c as u16, h, bold).map_err(xlsx_error)?;
            self.widths.push(display_width(h));
        }
        self.ws.set_freeze_panes(1, 0).map_err(xlsx_error)?;
        self.row = 1;
        Ok(())
    }

    pub(crate) fn cell(&mut self, col: usize, text: &str) -> Result<()> {
        let text = if text.chars().count() > CELL_MAX {
            *self.truncated += 1;
            text.chars().take(CELL_MAX).collect::<String>()
        } else {
            text.to_string()
        };
        if !text.is_empty() {
            self.ws.write_string(self.row, col as u16, &text).map_err(xlsx_error)?;
        }
        self.widen(col, &text);
        Ok(())
    }

    pub(crate) fn number(&mut self, col: usize, n: usize) -> Result<()> {
        self.ws.write_number(self.row, col as u16, n as f64).map_err(xlsx_error)?;
        self.widen(col, &n.to_string());
        Ok(())
    }

    fn link(&mut self, col: usize, text: &str, target: &str) -> Result<()> {
        self.ws.write_url(self.row, col as u16, Url::new(target).set_text(text)).map_err(xlsx_error)?;
        self.widen(col, text);
        Ok(())
    }

    fn widen(&mut self, col: usize, text: &str) {
        let w = text.lines().map(display_width).max().unwrap_or(0);
        if let Some(cur) = self.widths.get_mut(col) {
            *cur = (*cur).max(w);
        }
    }

    pub(crate) fn next(&mut self) {
        self.row += 1;
    }

    pub(crate) fn finish(self) -> Result<()> {
        if self.row > 1 {
            self.ws
                .autofilter(0, 0, self.row - 1, self.widths.len().saturating_sub(1) as u16)
                .map_err(xlsx_error)?;
        }
        for (c, w) in self.widths.iter().enumerate() {
            self.ws.set_column_width(c as u16, ((*w).clamp(6, WIDTH_MAX) + 2) as f64).map_err(xlsx_error)?;
        }
        Ok(())
    }
}

/// 写出数据字典的 xlsx，返回文件内容和截断的单元格个数。
pub fn write(content: &DocContent, lang: Lang) -> Result<(Vec<u8>, u64)> {
    let t = |k: &str| label(lang, k);
    let hs = |keys: &[&str]| keys.iter().map(|k| t(k)).collect::<Vec<_>>();
    let mut book = Workbook::new();
    let bold = Format::new().set_bold();
    let mut truncated = 0;
    let tables_name = t("doc.sheet.tables");
    let columns_name = t("doc.sheet.columns");

    // 每张表在"列"工作表中的第一行（从 1 起，第 0 行是表头）
    let mut first_rows = Vec::new();
    let mut row = 1u32;
    for s in &content.schemas {
        for tb in &s.tables {
            first_rows.push(row);
            row += tb.columns.len().max(1) as u32;
        }
    }

    {
        let ws = book.add_worksheet().set_name(&tables_name).map_err(xlsx_error)?;
        let mut sh = Sheet::new(ws, &mut truncated);
        sh.header(&hs(&["doc.col.schema", "doc.col.table", "doc.col.comment", "doc.col.columns"]), &bold)?;
        let mut i = 0;
        for s in &content.schemas {
            for tb in &s.tables {
                sh.cell(0, &s.name)?;
                let target =
                    format!("internal:'{}'!A{}", columns_name.replace('\'', "''"), first_rows[i] + 1);
                sh.link(1, &tb.name, &target)?;
                sh.cell(2, tb.comment.as_deref().unwrap_or_default())?;
                sh.number(3, tb.columns.len())?;
                sh.next();
                i += 1;
            }
        }
        sh.finish()?;
    }

    {
        let ws = book.add_worksheet().set_name(&columns_name).map_err(xlsx_error)?;
        let mut sh = Sheet::new(ws, &mut truncated);
        sh.header(
            &hs(&[
                "doc.col.schema",
                "doc.col.table",
                "doc.col.no",
                "doc.col.column",
                "doc.col.type",
                "doc.col.nullable",
                "doc.col.default",
                "doc.col.flags",
                "doc.col.comment",
            ]),
            &bold,
        )?;
        for s in &content.schemas {
            for tb in &s.tables {
                if tb.columns.is_empty() {
                    sh.cell(0, &s.name)?;
                    sh.cell(1, &tb.name)?;
                    sh.next();
                }
                for c in &tb.columns {
                    sh.cell(0, &s.name)?;
                    sh.cell(1, &tb.name)?;
                    sh.number(2, c.ordinal)?;
                    sh.cell(3, &c.name)?;
                    sh.cell(4, &c.data_type)?;
                    sh.cell(5, &t(if c.nullable { "doc.yes" } else { "doc.no" }))?;
                    sh.cell(6, c.default.as_deref().unwrap_or_default())?;
                    let flags: Vec<String> = c.flags.iter().map(|f| t(&format!("doc.flag.{f}"))).collect();
                    sh.cell(7, &flags.join(", "))?;
                    sh.cell(8, c.comment.as_deref().unwrap_or_default())?;
                    sh.next();
                }
            }
        }
        sh.finish()?;
    }

    {
        let ws = book.add_worksheet().set_name(t("doc.sheet.keys")).map_err(xlsx_error)?;
        let mut sh = Sheet::new(ws, &mut truncated);
        sh.header(
            &hs(&[
                "doc.col.schema",
                "doc.col.table",
                "doc.col.kind",
                "doc.col.name",
                "doc.col.index_columns",
                "doc.col.references",
                "doc.col.detail",
            ]),
            &bold,
        )?;
        for s in &content.schemas {
            for tb in &s.tables {
                let mut rows: Vec<[String; 5]> = Vec::new();
                for i in &tb.indexes {
                    let mut detail = Vec::new();
                    if i.unique {
                        detail.push(t("doc.kind.unique"));
                    }
                    detail.extend(i.method.clone());
                    rows.push([
                        t("doc.kind.index"),
                        i.name.clone(),
                        i.columns.clone(),
                        String::new(),
                        detail.join(", "),
                    ]);
                }
                for k in &tb.constraints {
                    let (cols, detail) = if k.kind == "check" {
                        (String::new(), k.detail.clone())
                    } else {
                        (k.detail.clone(), String::new())
                    };
                    rows.push([
                        t(&format!("doc.kind.{}", k.kind)),
                        k.name.clone().unwrap_or_default(),
                        cols,
                        String::new(),
                        detail,
                    ]);
                }
                for (kind, keys) in
                    [("doc.kind.foreign_key", &tb.foreign_keys), ("doc.kind.logical_key", &tb.logical_keys)]
                {
                    for k in keys {
                        let mut detail = Vec::new();
                        if let Some(d) = &k.on_delete {
                            detail.push(format!("{}: {d}", t("doc.col.on_delete")));
                        }
                        if let Some(u) = &k.on_update {
                            detail.push(format!("{}: {u}", t("doc.col.on_update")));
                        }
                        detail.extend(k.comment.clone());
                        rows.push([
                            t(kind),
                            k.name.clone().unwrap_or_default(),
                            k.columns.clone(),
                            format!("{}.{} ({})", k.ref_schema, k.ref_table, k.ref_columns),
                            detail.join("; "),
                        ]);
                    }
                }
                for r in rows {
                    sh.cell(0, &s.name)?;
                    sh.cell(1, &tb.name)?;
                    for (i, v) in r.iter().enumerate() {
                        sh.cell(i + 2, v)?;
                    }
                    sh.next();
                }
            }
        }
        sh.finish()?;
    }

    let bytes = book.save_to_buffer().map_err(xlsx_error)?;
    Ok((bytes, truncated))
}

#[cfg(test)]
mod tests {
    use std::io::{Cursor, Read};

    use super::super::content::tests::{opts, sample};
    use super::super::content::{DocKind, build};
    use super::*;

    fn part(bytes: &[u8], name: &str) -> String {
        let mut z = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
        let mut s = String::new();
        z.by_name(name).unwrap().read_to_string(&mut s).unwrap();
        s
    }

    #[test]
    fn three_sheets_with_links_and_truncation() {
        let mut src = sample();
        src[0].tables[0].table.comment = Some("x".repeat(CELL_MAX + 5));
        let c = build(opts(DocKind::Dictionary), src, vec![]);
        let (bytes, truncated) = write(&c, Lang::ZhCn).unwrap();
        // orders 的注释超长（表清单一处）
        assert_eq!(truncated, 1);
        let book = part(&bytes, "xl/workbook.xml");
        for name in ["表清单", "列", "索引和约束"] {
            assert!(book.contains(&format!("name=\"{name}\"")), "{book}");
        }
        let strings = part(&bytes, "xl/sharedStrings.xml");
        for s in [
            "customers",
            "code DESC",
            "逻辑外键",
            "shop.orders (id)",
            "删除时: CASCADE",
            "生成列",
            "amount &gt;= 0",
        ] {
            assert!(strings.contains(s), "{s}");
        }
        // 表名链接到"列"工作表中的第一行：customers 在第 2 行，items 在第 5 行
        let sheet1 = part(&bytes, "xl/worksheets/sheet1.xml");
        assert!(
            sheet1.contains("location=\"'列'!A2\"") && sheet1.contains("location=\"'列'!A5\""),
            "{sheet1}"
        );
        assert!(sheet1.contains("<autoFilter ref=\"A1:D5\"/>"));
    }
}
