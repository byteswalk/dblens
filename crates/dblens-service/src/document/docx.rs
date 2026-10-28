//! docx（M4c 设计文档 5.2 节）：程序生成正文，放进"样式来源"文档（类似 pandoc 的 reference-doc）。
//! 样式来源中的样式、页眉页脚、纸张和页边距都保留；样式按 Word 内部名找，缺的补上内置的定义。

use std::collections::HashMap;
use std::fmt::Write as _;
use std::io::{Cursor, Read, Write};

use dblens_core::{Error, Lang, Result, msg};
use quick_xml::escape::escape;
use quick_xml::events::Event;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use super::content::{DocContent, DocKey, DocKind, DocTable};
use super::labels::label;

const BUILTIN_PARTS: [(&str, &str); 7] = [
    ("[Content_Types].xml", include_str!("docx/content_types.xml")),
    ("_rels/.rels", include_str!("docx/rels.xml")),
    ("docProps/core.xml", include_str!("docx/core.xml")),
    ("word/document.xml", include_str!("docx/document.xml")),
    ("word/_rels/document.xml.rels", include_str!("docx/document_rels.xml")),
    ("word/styles.xml", include_str!("docx/styles.xml")),
    ("word/settings.xml", include_str!("docx/settings.xml")),
];

const BUILTIN_STYLES: &str = include_str!("docx/styles.xml");

/// 要用到的样式（Word 内部名，小写）。
const STYLE_NAMES: [&str; 12] = [
    "title",
    "heading 1",
    "heading 2",
    "heading 3",
    "caption",
    "toc heading",
    "toc 1",
    "toc 2",
    "dblens table text",
    "dblens code",
    "hyperlink",
    "table grid",
];

const NS_W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const REL_IMAGE: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/image";
const REL_SETTINGS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/settings";

/// 1 twip（1/20 磅）= 635 EMU；图片按 2 倍像素画，1 像素按 1/192 英寸 = 4762.5 EMU。
const EMU_PER_TWIP: u64 = 635;
const EMU_PER_PX: f64 = 4762.5;

fn bad_reference(e: impl std::fmt::Display) -> Error {
    Error::invalid(msg!("document.bad_reference", error = e))
}

fn write_err(e: impl std::fmt::Display) -> Error {
    Error::internal(msg!("document.write_failed", error = e))
}

/// 内置的样式来源。
pub fn builtin_reference() -> Vec<u8> {
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    let opts = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    for (name, text) in BUILTIN_PARTS {
        // 写入失败只会是内存不足
        let _ = zip.start_file(name, opts).and_then(|_| Ok(zip.write_all(text.as_bytes())?));
    }
    zip.finish().map(Cursor::into_inner).unwrap_or_default()
}

/// 读出的样式来源。
struct Reference {
    parts: Vec<(String, Vec<u8>)>,
}

impl Reference {
    fn read(bytes: &[u8]) -> Result<Self> {
        let mut zip = ZipArchive::new(Cursor::new(bytes)).map_err(bad_reference)?;
        let mut parts = Vec::new();
        for i in 0..zip.len() {
            let mut f = zip.by_index(i).map_err(bad_reference)?;
            if f.is_dir() {
                continue;
            }
            let mut data = Vec::new();
            f.read_to_end(&mut data).map_err(bad_reference)?;
            parts.push((f.name().map_err(bad_reference)?.to_string(), data));
        }
        let r = Reference { parts };
        for need in
            ["word/document.xml", "word/styles.xml", "word/_rels/document.xml.rels", "[Content_Types].xml"]
        {
            if r.get(need).is_none() {
                return Err(bad_reference(need));
            }
        }
        Ok(r)
    }

    fn get(&self, name: &str) -> Option<&str> {
        self.parts.iter().find(|(n, _)| n == name).and_then(|(_, d)| std::str::from_utf8(d).ok())
    }

    fn set(&mut self, name: &str, data: Vec<u8>) {
        match self.parts.iter_mut().find(|(n, _)| n == name) {
            Some(p) => p.1 = data,
            None => self.parts.push((name.to_string(), data)),
        }
    }
}

/// 检查样式来源是不是有效的 .docx。
pub fn check_reference(bytes: &[u8]) -> Result<()> {
    Reference::read(bytes).map(|_| ())
}

/// 正文最后的节属性（纸张、页边距、页眉页脚的引用）。
fn section(document: &str) -> String {
    let end = document.rfind("</w:body>").unwrap_or(document.len());
    match document[..end].rfind("<w:sectPr") {
        Some(start) => document[start..end].trim().to_string(),
        None => String::new(),
    }
}

/// 节属性中某个元素的数字属性。
fn attr_num(sect: &str, element: &str, attr: &str) -> Option<u64> {
    let start = sect.find(&format!("<{element} "))?;
    let tag = &sect[start..start + sect[start..].find('>')?];
    let key = format!("{attr}=\"");
    let at = tag.find(&key)? + key.len();
    tag[at..].split('"').next()?.parse().ok()
}

/// 版心的宽、高（twip）。
fn text_area(sect: &str) -> (u64, u64) {
    let w = attr_num(sect, "w:pgSz", "w:w").unwrap_or(11906);
    let h = attr_num(sect, "w:pgSz", "w:h").unwrap_or(16838);
    let left = attr_num(sect, "w:pgMar", "w:left").unwrap_or(1247);
    let right = attr_num(sect, "w:pgMar", "w:right").unwrap_or(1247);
    let top = attr_num(sect, "w:pgMar", "w:top").unwrap_or(1440);
    let bottom = attr_num(sect, "w:pgMar", "w:bottom").unwrap_or(1440);
    (w.saturating_sub(left + right).max(1440), h.saturating_sub(top + bottom).max(1440))
}

/// 样式名（小写）→ 样式编号。
fn style_ids(styles: &str) -> HashMap<String, String> {
    let mut reader = quick_xml::Reader::from_str(styles);
    let mut out = HashMap::new();
    let mut current: Option<String> = None;
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => match e.name().as_ref() {
                "w:style" => {
                    current = e
                        .attributes()
                        .flatten()
                        .find(|a| a.key.as_ref() == "w:styleId")
                        .map(|a| a.value.to_string());
                }
                "w:name" => {
                    let name = e
                        .attributes()
                        .flatten()
                        .find(|a| a.key.as_ref() == "w:val")
                        .map(|a| a.value.to_lowercase());
                    if let (Some(id), Some(name)) = (current.take(), name) {
                        out.entry(name).or_insert(id);
                    }
                }
                _ => {}
            },
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    out
}

/// 内置样式中按名字取整段 `<w:style>`。
fn builtin_style(name: &str) -> Option<&'static str> {
    BUILTIN_STYLES.split("<w:style ").skip(1).find_map(|s| {
        let body = &s[..s.find("</w:style>")? + "</w:style>".len()];
        body.to_lowercase().contains(&format!("<w:name w:val=\"{name}\"/>")).then_some(body)
    })
}

/// 补上样式来源中缺的样式：编号加上 `DBLens` 前缀，以免与已有的编号冲突。返回改后的 styles.xml 和名字 → 编号。
fn complete_styles(styles: &str) -> (String, HashMap<String, String>) {
    let mut ids = style_ids(styles);
    let mut added = String::new();
    for name in STYLE_NAMES {
        if ids.contains_key(name) {
            continue;
        }
        let Some(body) = builtin_style(name) else { continue };
        let id_at = body.find("w:styleId=\"").map(|i| i + "w:styleId=\"".len()).unwrap_or(0);
        let old_id = body[id_at..].split('"').next().unwrap_or_default();
        let new_id =
            if old_id.starts_with("DBLens") { old_id.to_string() } else { format!("DBLens{old_id}") };
        let fixed = body.replace(&format!("w:styleId=\"{old_id}\""), &format!("w:styleId=\"{new_id}\""));
        // 基于的样式在样式来源中也可能缺：一律基于正文
        let fixed =
            fixed.replace("<w:basedOn w:val=\"Normal\"/>", "").replace("<w:next w:val=\"Normal\"/>", "");
        added.push_str("<w:style ");
        added.push_str(&fixed);
        ids.insert(name.to_string(), new_id);
    }
    let out = match styles.rfind("</w:styles>") {
        Some(i) if !added.is_empty() => format!("{}{added}{}", &styles[..i], &styles[i..]),
        _ => styles.to_string(),
    };
    (out, ids)
}

/// XML 中不能出现的控制字符去掉。
fn clean(text: &str) -> String {
    text.chars().filter(|c| !c.is_control() || matches!(c, '\t' | '\n' | '\r')).collect()
}

/// 正文的写出。
struct Body<'a> {
    lang: Lang,
    styles: &'a HashMap<String, String>,
    /// 版心宽、高（twip）。
    width: u64,
    height: u64,
    xml: String,
    bookmark: u32,
    /// 图片：(关系编号, 媒体文件名, PNG)。
    images: Vec<(String, String, Vec<u8>)>,
}

impl Body<'_> {
    fn t(&self, key: &str) -> String {
        label(self.lang, key)
    }

    fn style(&self, name: &str) -> &str {
        self.styles.get(name).map(String::as_str).unwrap_or("Normal")
    }

    /// 文字 run：换行写成 `<w:br/>`。
    fn runs(text: &str, rpr: &str) -> String {
        let mut out = String::new();
        for (i, line) in clean(text).replace("\r\n", "\n").split('\n').enumerate() {
            if i > 0 {
                let _ = write!(out, "<w:r>{rpr}<w:br/></w:r>");
            }
            let _ = write!(out, "<w:r>{rpr}<w:t xml:space=\"preserve\">{}</w:t></w:r>", escape(line));
        }
        out
    }

    fn para(&mut self, style: Option<&str>, text: &str) {
        let ppr = style
            .map(|s| format!("<w:pPr><w:pStyle w:val=\"{}\"/></w:pPr>", self.style(s)))
            .unwrap_or_default();
        let _ = writeln!(self.xml, "<w:p>{ppr}{}</w:p>", Self::runs(text, ""));
    }

    fn heading(&mut self, level: u8, text: &str, anchor: Option<&str>) {
        let style = self.style(&format!("heading {level}")).to_string();
        let mut inner = Self::runs(text, "");
        if let Some(a) = anchor {
            self.bookmark += 1;
            let id = self.bookmark;
            inner = format!(
                "<w:bookmarkStart w:id=\"{id}\" w:name=\"{}\"/>{inner}<w:bookmarkEnd w:id=\"{id}\"/>",
                escape(a)
            );
        }
        let _ = writeln!(self.xml, "<w:p><w:pPr><w:pStyle w:val=\"{style}\"/></w:pPr>{inner}</w:p>");
    }

    /// 链接到文档中的书签。
    fn link(&self, text: &str, anchor: Option<&str>) -> String {
        match anchor {
            Some(a) => format!(
                "<w:hyperlink w:anchor=\"{}\" w:history=\"1\">{}</w:hyperlink>",
                escape(a),
                Self::runs(
                    text,
                    &format!("<w:rPr><w:rStyle w:val=\"{}\"/></w:rPr>", self.style("hyperlink"))
                )
            ),
            None => Self::runs(text, ""),
        }
    }

    /// 表格：`widths` 是各列的相对宽度，单元格内容是已写好的 run（`Cell::Xml`）或文字。
    fn table(&mut self, headers: &[String], widths: &[u64], rows: Vec<Vec<Cell>>) {
        let total: u64 = widths.iter().sum::<u64>().max(1);
        let cols: Vec<u64> = widths.iter().map(|w| self.width * w / total).collect();
        let text_style = self.style("dblens table text").to_string();
        let grid = self.style("table grid").to_string();
        let _ = write!(
            self.xml,
            "<w:tbl><w:tblPr><w:tblStyle w:val=\"{grid}\"/><w:tblW w:w=\"{}\" w:type=\"dxa\"/><w:tblLayout w:type=\"fixed\"/></w:tblPr><w:tblGrid>",
            self.width
        );
        for c in &cols {
            let _ = write!(self.xml, "<w:gridCol w:w=\"{c}\"/>");
        }
        self.xml.push_str("</w:tblGrid>\n");
        let cell = |xml: &mut String, w: u64, content: &str, header: bool| {
            let shd = if header { "<w:shd w:val=\"clear\" w:color=\"auto\" w:fill=\"F2F2F4\"/>" } else { "" };
            let _ = write!(
                xml,
                "<w:tc><w:tcPr><w:tcW w:w=\"{w}\" w:type=\"dxa\"/>{shd}</w:tcPr><w:p><w:pPr><w:pStyle w:val=\"{text_style}\"/></w:pPr>{content}</w:p></w:tc>"
            );
        };
        self.xml.push_str("<w:tr><w:trPr><w:tblHeader/></w:trPr>");
        for (h, w) in headers.iter().zip(&cols) {
            cell(&mut self.xml, *w, &Self::runs(h, "<w:rPr><w:b/></w:rPr>"), true);
        }
        self.xml.push_str("</w:tr>\n");
        for row in rows {
            self.xml.push_str("<w:tr><w:trPr><w:cantSplit/></w:trPr>");
            for (c, w) in row.iter().zip(&cols) {
                let content = match c {
                    Cell::Text(t) => Self::runs(t, ""),
                    Cell::Xml(x) => x.clone(),
                };
                cell(&mut self.xml, *w, &content, false);
            }
            self.xml.push_str("</w:tr>\n");
        }
        self.xml.push_str("</w:tbl>\n");
        // 两张表之间要有段落，否则 Word 会把它们合成一张
        self.xml.push_str("<w:p/>\n");
    }

    fn image(&mut self, png: &[u8], width: u32, height: u32, title: &str) {
        let n = self.images.len() + 1;
        let rel = format!("rIdDBLensImage{n}");
        let file = format!("dblens_image{n}.png");
        let mut cx = (f64::from(width) * EMU_PER_PX) as u64;
        let mut cy = (f64::from(height) * EMU_PER_PX) as u64;
        let max_w = self.width * EMU_PER_TWIP;
        // 高度留出题注的位置
        let max_h = self.height * EMU_PER_TWIP * 85 / 100;
        if cx > max_w {
            cy = cy * max_w / cx.max(1);
            cx = max_w;
        }
        if cy > max_h {
            cx = cx * max_h / cy.max(1);
            cy = max_h;
        }
        let _ = writeln!(
            self.xml,
            "<w:p><w:pPr><w:jc w:val=\"center\"/></w:pPr><w:r><w:drawing><wp:inline distT=\"0\" distB=\"0\" distL=\"0\" distR=\"0\"><wp:extent cx=\"{cx}\" cy=\"{cy}\"/><wp:docPr id=\"{n}\" name=\"{name}\"/><wp:cNvGraphicFramePr><a:graphicFrameLocks noChangeAspect=\"1\"/></wp:cNvGraphicFramePr><a:graphic><a:graphicData uri=\"http://schemas.openxmlformats.org/drawingml/2006/picture\"><pic:pic><pic:nvPicPr><pic:cNvPr id=\"{n}\" name=\"{file}\"/><pic:cNvPicPr/></pic:nvPicPr><pic:blipFill><a:blip r:embed=\"{rel}\"/><a:stretch><a:fillRect/></a:stretch></pic:blipFill><pic:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"{cx}\" cy=\"{cy}\"/></a:xfrm><a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></pic:spPr></pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p>",
            name = escape(clean(title)),
        );
        self.images.push((rel, file, png.to_vec()));
    }

    fn code(&mut self, text: &str) {
        let style = self.style("dblens code").to_string();
        for line in clean(text).replace("\r\n", "\n").split('\n') {
            let _ = writeln!(
                self.xml,
                "<w:p><w:pPr><w:pStyle w:val=\"{style}\"/></w:pPr><w:r><w:t xml:space=\"preserve\">{}</w:t></w:r></w:p>",
                escape(line)
            );
        }
        self.xml.push_str("<w:p/>\n");
    }

    fn toc(&mut self) {
        let heading = self.style("toc heading").to_string();
        let _ = writeln!(
            self.xml,
            "<w:p><w:pPr><w:pStyle w:val=\"{heading}\"/></w:pPr>{}</w:p>",
            Self::runs(&self.t("doc.contents"), "")
        );
        let _ = writeln!(
            self.xml,
            "<w:p><w:r><w:fldChar w:fldCharType=\"begin\" w:dirty=\"true\"/></w:r><w:r><w:instrText xml:space=\"preserve\"> TOC \\o \"1-2\" \\h \\z \\u </w:instrText></w:r><w:r><w:fldChar w:fldCharType=\"separate\"/></w:r>{}<w:r><w:fldChar w:fldCharType=\"end\"/></w:r></w:p>",
            Self::runs(&self.t("doc.toc_hint"), "")
        );
    }
}

enum Cell {
    Text(String),
    Xml(String),
}

fn text(s: impl Into<String>) -> Cell {
    Cell::Text(s.into())
}

fn opt(s: &Option<String>) -> Cell {
    Cell::Text(s.clone().unwrap_or_default())
}

fn write_keys(b: &mut Body, title: &str, keys: &[DocKey]) {
    if keys.is_empty() {
        return;
    }
    b.heading(3, title, None);
    let headers = [
        "doc.col.name",
        "doc.col.index_columns",
        "doc.col.references",
        "doc.col.on_delete",
        "doc.col.on_update",
        "doc.col.comment",
    ]
    .map(|k| b.t(k));
    let rows = keys
        .iter()
        .map(|k| {
            let target = format!("{}.{}", k.ref_schema, k.ref_table);
            let link = format!(
                "{}{}",
                b.link(&target, k.ref_anchor.as_deref()),
                Body::runs(&format!(" ({})", k.ref_columns), "")
            );
            vec![
                opt(&k.name),
                text(&k.columns),
                Cell::Xml(link),
                opt(&k.on_delete),
                opt(&k.on_update),
                opt(&k.comment),
            ]
        })
        .collect();
    b.table(&headers, &[3, 3, 4, 2, 2, 3], rows);
}

fn write_table(b: &mut Body, t: &DocTable) {
    b.heading(2, &format!("{}.{}", t.schema, t.name), Some(&t.anchor));
    if let Some(c) = &t.comment {
        b.para(None, c);
    }
    if let Some(p) = &t.partition {
        b.para(None, &format!("{}: {p}", b.t("doc.partition")));
    }
    let headers = [
        "doc.col.no",
        "doc.col.column",
        "doc.col.type",
        "doc.col.nullable",
        "doc.col.default",
        "doc.col.flags",
        "doc.col.comment",
    ]
    .map(|k| b.t(k));
    let rows = t
        .columns
        .iter()
        .map(|c| {
            let flags: Vec<String> = c.flags.iter().map(|f| b.t(&format!("doc.flag.{f}"))).collect();
            vec![
                text(c.ordinal.to_string()),
                text(&c.name),
                text(&c.data_type),
                text(b.t(if c.nullable { "doc.yes" } else { "doc.no" })),
                opt(&c.default),
                text(flags.join(", ")),
                opt(&c.comment),
            ]
        })
        .collect();
    b.table(&headers, &[2, 5, 5, 2, 4, 3, 7], rows);
    if !t.indexes.is_empty() {
        b.heading(3, &b.t("doc.sec.indexes"), None);
        let headers =
            ["doc.col.name", "doc.col.index_columns", "doc.col.unique", "doc.col.method"].map(|k| b.t(k));
        let rows = t
            .indexes
            .iter()
            .map(|i| {
                vec![
                    text(&i.name),
                    text(&i.columns),
                    text(b.t(if i.unique { "doc.yes" } else { "doc.no" })),
                    opt(&i.method),
                ]
            })
            .collect();
        b.table(&headers, &[4, 5, 2, 2], rows);
    }
    if !t.constraints.is_empty() {
        b.heading(3, &b.t("doc.sec.constraints"), None);
        let headers = ["doc.col.kind", "doc.col.name", "doc.col.detail"].map(|k| b.t(k));
        let rows = t
            .constraints
            .iter()
            .map(|k| vec![text(b.t(&format!("doc.kind.{}", k.kind))), opt(&k.name), text(&k.detail)])
            .collect();
        b.table(&headers, &[2, 4, 7], rows);
    }
    let fk = b.t("doc.sec.foreign_keys");
    write_keys(b, &fk, &t.foreign_keys);
    let lk = b.t("doc.sec.logical_keys");
    write_keys(b, &lk, &t.logical_keys);
    if !t.triggers.is_empty() {
        b.heading(3, &b.t("doc.sec.triggers"), None);
        let headers = ["doc.col.name", "doc.col.timing", "doc.col.events"].map(|k| b.t(k));
        let rows = t.triggers.iter().map(|g| vec![text(&g.name), opt(&g.timing), text(&g.events)]).collect();
        b.table(&headers, &[4, 2, 4], rows);
    }
    if !t.referenced_by.is_empty() {
        b.heading(3, &b.t("doc.sec.referenced_by"), None);
        let headers =
            ["doc.col.table", "doc.col.index_columns", "doc.col.references", "doc.col.kind", "doc.col.name"]
                .map(|k| b.t(k));
        let rows = t
            .referenced_by
            .iter()
            .map(|r| {
                vec![
                    Cell::Xml(b.link(&format!("{}.{}", r.schema, r.table), r.anchor.as_deref())),
                    text(&r.columns),
                    text(&r.ref_columns),
                    text(b.t(&format!("doc.kind.{}", r.kind))),
                    opt(&r.name),
                ]
            })
            .collect();
        b.table(&headers, &[4, 3, 3, 2, 3], rows);
    }
}

fn write_body(b: &mut Body, c: &DocContent) {
    let title_style = b.style("title").to_string();
    let _ = writeln!(
        b.xml,
        "<w:p><w:pPr><w:pStyle w:val=\"{title_style}\"/></w:pPr>{}</w:p>",
        Body::runs(&c.info.title, "")
    );
    let kind = b.t(if c.kind == DocKind::Dictionary { "doc.dictionary" } else { "doc.design" });
    b.para(Some("caption"), &kind);
    for (key, value) in [
        ("doc.version", c.info.version.clone()),
        ("doc.author", c.info.author.clone()),
        ("doc.generated", Some(c.info.generated.clone())),
        ("doc.source", Some(c.info.source.clone())),
    ] {
        if let Some(v) = value.filter(|v| !v.is_empty()) {
            b.para(None, &format!("{}: {v}", b.t(key)));
        }
    }
    if c.kind == DocKind::Dictionary
        && let Some(d) = &c.info.description
    {
        b.para(None, d);
    }
    b.toc();
    if c.kind == DocKind::Design {
        b.heading(1, &b.t("doc.overview"), None);
        if let Some(d) = &c.info.description {
            b.para(None, d);
        }
        let s = &c.stats;
        let mut rows = vec![
            vec![text(b.t("doc.stat.schemas")), text(s.schemas.to_string())],
            vec![text(b.t("doc.stat.tables")), text(s.tables.to_string())],
            vec![text(b.t("doc.stat.columns")), text(s.columns.to_string())],
            vec![text(b.t("doc.stat.foreign_keys")), text(s.foreign_keys.to_string())],
            vec![text(b.t("doc.stat.logical_keys")), text(s.logical_keys.to_string())],
        ];
        if s.views > 0 {
            rows.push(vec![text(b.t("doc.stat.views")), text(s.views.to_string())]);
        }
        b.table(&[b.t("doc.stats"), String::new()], &[3, 1], rows);
        if !c.diagrams.is_empty() {
            b.heading(1, &b.t("doc.diagrams"), None);
            for d in &c.diagrams {
                b.heading(2, &d.title, None);
                if let Some(desc) = &d.description {
                    b.para(None, desc);
                }
                b.image(&d.png, d.width, d.height, &d.title);
                if d.names_only {
                    b.para(Some("caption"), &b.t("doc.names_only"));
                }
            }
        }
        if !c.relations.is_empty() {
            b.heading(1, &b.t("doc.relations"), None);
            let headers = [
                "doc.col.from",
                "doc.col.index_columns",
                "doc.col.to",
                "doc.col.index_columns",
                "doc.col.kind",
                "doc.col.name",
            ]
            .map(|k| b.t(k));
            let rows = c
                .relations
                .iter()
                .map(|r| {
                    vec![
                        Cell::Xml(
                            b.link(&format!("{}.{}", r.from_schema, r.from_table), r.from_anchor.as_deref()),
                        ),
                        text(&r.from_columns),
                        Cell::Xml(b.link(&format!("{}.{}", r.to_schema, r.to_table), r.to_anchor.as_deref())),
                        text(&r.to_columns),
                        text(b.t(&format!("doc.kind.{}", r.kind))),
                        opt(&r.name),
                    ]
                })
                .collect();
            b.table(&headers, &[4, 3, 4, 3, 2, 3], rows);
        }
    }
    b.heading(1, &b.t("doc.tables"), None);
    for s in &c.schemas {
        b.heading(2, &s.name, None);
        let headers = ["doc.col.no", "doc.col.table", "doc.col.comment", "doc.col.columns"].map(|k| b.t(k));
        let rows = s
            .tables
            .iter()
            .enumerate()
            .map(|(i, t)| {
                vec![
                    text((i + 1).to_string()),
                    Cell::Xml(b.link(&t.name, Some(&t.anchor))),
                    opt(&t.comment),
                    text(t.columns.len().to_string()),
                ]
            })
            .collect();
        b.table(&headers, &[2, 6, 8, 2], rows);
    }
    if c.stats.views > 0 {
        b.heading(1, &b.t("doc.views"), None);
        for s in &c.schemas {
            for v in &s.views {
                b.heading(2, &format!("{}.{}", s.name, v.name), Some(&v.anchor));
                if let Some(cm) = &v.comment {
                    b.para(None, cm);
                }
                let headers =
                    ["doc.col.no", "doc.col.column", "doc.col.type", "doc.col.comment"].map(|k| b.t(k));
                let rows = v
                    .columns
                    .iter()
                    .map(|col| {
                        vec![
                            text(col.ordinal.to_string()),
                            text(&col.name),
                            text(&col.data_type),
                            opt(&col.comment),
                        ]
                    })
                    .collect();
                b.table(&headers, &[2, 5, 5, 8], rows);
                if let Some(def) = &v.definition {
                    b.heading(3, &b.t("doc.sec.definition"), None);
                    b.code(def);
                }
            }
        }
    }
    b.heading(1, &b.t("doc.details"), None);
    for s in &c.schemas {
        for t in &s.tables {
            write_table(b, t);
        }
    }
    if !c.unreadable.is_empty() {
        b.heading(1, &b.t("doc.unreadable"), None);
        let headers = ["doc.col.schema", "doc.col.table", "doc.col.file", "doc.col.error"].map(|k| b.t(k));
        let rows = c
            .unreadable
            .iter()
            .map(|u| vec![text(&u.schema), text(&u.name), text(&u.path), text(&u.message)])
            .collect();
        b.table(&headers, &[2, 3, 5, 8], rows);
    }
}

/// 在 `<w:settings …>` 开头加上打开时更新字段（目录的页码由 Word 算）。
fn update_fields(settings: &str) -> String {
    if settings.contains("<w:updateFields") {
        return settings.to_string();
    }
    match settings.find("<w:settings").and_then(|i| settings[i..].find('>').map(|j| i + j + 1)) {
        Some(at) => format!("{}<w:updateFields w:val=\"true\"/>{}", &settings[..at], &settings[at..]),
        None => settings.to_string(),
    }
}

/// 在关系文件末尾加关系。
fn add_rels(rels: &str, items: &[(String, &str, String)]) -> String {
    let mut extra = String::new();
    for (id, kind, target) in items {
        let _ = write!(extra, "<Relationship Id=\"{id}\" Type=\"{kind}\" Target=\"{target}\"/>");
    }
    match rels.rfind("</Relationships>") {
        Some(i) => format!("{}{extra}{}", &rels[..i], &rels[i..]),
        None => rels.to_string(),
    }
}

/// 写出 docx。`reference` 为 None 时用内置的样式来源。
pub fn write(content: &DocContent, reference: Option<&[u8]>, lang: Lang) -> Result<Vec<u8>> {
    let builtin;
    let bytes = match reference {
        Some(b) => b,
        None => {
            builtin = builtin_reference();
            &builtin
        }
    };
    let mut r = Reference::read(bytes)?;
    let doc = r.get("word/document.xml").unwrap_or_default().to_string();
    let (styles, ids) = complete_styles(r.get("word/styles.xml").unwrap_or_default());
    let sect = section(&doc);
    let (width, height) = text_area(&sect);
    let mut body =
        Body { lang, styles: &ids, width, height, xml: String::new(), bookmark: 0, images: vec![] };
    write_body(&mut body, content);

    // 正文：沿用样式来源根元素上的命名空间声明，补上图片要用的
    let root_end = doc.find("<w:body").unwrap_or(0);
    let mut head = doc[..root_end].to_string();
    for (prefix, uri) in [
        ("w", NS_W),
        ("r", "http://schemas.openxmlformats.org/officeDocument/2006/relationships"),
        ("wp", "http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing"),
        ("a", "http://schemas.openxmlformats.org/drawingml/2006/main"),
        ("pic", "http://schemas.openxmlformats.org/drawingml/2006/picture"),
    ] {
        if !head.contains(&format!("xmlns:{prefix}="))
            && let Some(i) = head.find("<w:document")
        {
            head.insert_str(i + "<w:document".len(), &format!(" xmlns:{prefix}=\"{uri}\""));
        }
    }
    if head.is_empty() {
        head = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<w:document xmlns:w=\"{NS_W}\">"
        );
    }
    let document = format!("{}\n<w:body>\n{}{sect}\n</w:body>\n</w:document>\n", head.trim_end(), body.xml);
    r.set("word/document.xml", document.into_bytes());
    r.set("word/styles.xml", styles.into_bytes());

    let mut rels = r.get("word/_rels/document.xml.rels").unwrap_or_default().to_string();
    let mut types = r.get("[Content_Types].xml").unwrap_or_default().to_string();
    let mut new_rels: Vec<(String, &str, String)> =
        body.images.iter().map(|(id, file, _)| (id.clone(), REL_IMAGE, format!("media/{file}"))).collect();
    match r.get("word/settings.xml") {
        Some(s) => {
            let s = update_fields(s);
            r.set("word/settings.xml", s.into_bytes());
        }
        None => {
            r.set("word/settings.xml", update_fields(include_str!("docx/settings.xml")).into_bytes());
            new_rels.push(("rIdDBLensSettings".into(), REL_SETTINGS, "settings.xml".into()));
            if let Some(i) = types.rfind("</Types>") {
                types.insert_str(i, "<Override PartName=\"/word/settings.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.settings+xml\"/>");
            }
        }
    }
    rels = add_rels(&rels, &new_rels);
    if !body.images.is_empty()
        && !types.to_lowercase().contains("extension=\"png\"")
        && let Some(i) = types.find("<Default ")
    {
        types.insert_str(i, "<Default Extension=\"png\" ContentType=\"image/png\"/>");
    }
    r.set("word/_rels/document.xml.rels", rels.into_bytes());
    r.set("[Content_Types].xml", types.into_bytes());
    for (_, file, png) in body.images {
        r.set(&format!("word/media/{file}"), png);
    }

    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    let opts = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    for (name, data) in &r.parts {
        zip.start_file(name.as_str(), opts).map_err(write_err)?;
        zip.write_all(data).map_err(write_err)?;
    }
    zip.finish().map(Cursor::into_inner).map_err(write_err)
}

#[cfg(test)]
mod tests {
    use super::super::content::tests::{opts, sample};
    use super::super::content::{DocDiagram, build};
    use super::*;

    fn unzip(bytes: &[u8]) -> HashMap<String, String> {
        let mut z = ZipArchive::new(Cursor::new(bytes)).unwrap();
        let mut out = HashMap::new();
        for i in 0..z.len() {
            let mut f = z.by_index(i).unwrap();
            let mut data = Vec::new();
            f.read_to_end(&mut data).unwrap();
            out.insert(f.name().unwrap().to_string(), String::from_utf8_lossy(&data).to_string());
        }
        out
    }

    fn reference_with(styles: &str, document: &str) -> Vec<u8> {
        let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
        for (name, text) in BUILTIN_PARTS {
            let text = match name {
                "word/styles.xml" => styles,
                "word/document.xml" => document,
                _ => text,
            };
            zip.start_file(name, SimpleFileOptions::default()).unwrap();
            zip.write_all(text.as_bytes()).unwrap();
        }
        zip.finish().unwrap().into_inner()
    }

    fn diagram() -> DocDiagram {
        DocDiagram {
            schema: "shop".into(),
            name: "_all".into(),
            title: "shop".into(),
            description: None,
            names_only: false,
            file: "01-shop-_all.png".into(),
            src: String::new(),
            width: 4000,
            height: 1000,
            png: vec![0x89, b'P', b'N', b'G'],
        }
    }

    #[test]
    fn builtin_reference_document() {
        let c = build(opts(DocKind::Design), sample(), vec![diagram()]);
        let parts = unzip(&write(&c, None, Lang::ZhCn).unwrap());
        let doc = &parts["word/document.xml"];
        insta::assert_snapshot!("docx_design_zh", doc);
        // 图片：关系、内容类型、媒体文件
        assert!(parts["word/_rels/document.xml.rels"].contains("Id=\"rIdDBLensImage1\""));
        assert!(parts["[Content_Types].xml"].contains("Extension=\"png\""));
        assert!(parts["word/media/dblens_image1.png"].ends_with("PNG"));
        // 4000 像素按 2 倍画，宽 4000 × 4762.5 EMU 超过版心（11906 − 2 × 1247 twip），按版心宽缩放
        assert!(doc.contains("<wp:extent cx=\"5976620\" cy=\"1494155\"/>"), "{doc}");
        assert!(parts["word/settings.xml"].contains("<w:updateFields w:val=\"true\"/>"));
        // 每个 XML 部件都是格式正确的 XML
        for (name, text) in parts.iter().filter(|(n, _)| n.ends_with(".xml") || n.ends_with(".rels")) {
            let mut reader = quick_xml::Reader::from_str(text);
            reader.config_mut().check_end_names = true;
            loop {
                match reader.read_event() {
                    Ok(Event::Eof) => break,
                    Ok(_) => {}
                    Err(e) => panic!("{name}: {e}"),
                }
            }
        }
        // 链接到书签、书签名
        assert!(doc.contains("<w:hyperlink w:anchor=\"t4\""));
        assert!(doc.contains("w:name=\"t1\""));
    }

    #[test]
    fn reference_with_localized_style_ids_and_missing_styles() {
        // 中文版 Word：样式编号是数字，名字仍是英文的内部名；样式来源中没有表格样式和代码样式
        let styles = r#"<?xml version="1.0" encoding="UTF-8"?><w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:style w:type="paragraph" w:default="1" w:styleId="a"><w:name w:val="Normal"/></w:style><w:style w:type="paragraph" w:styleId="1"><w:name w:val="heading 1"/></w:style><w:style w:type="paragraph" w:styleId="2"><w:name w:val="heading 2"/></w:style><w:style w:type="paragraph" w:styleId="a3"><w:name w:val="Title"/></w:style></w:styles>"#;
        // 横向 A4，页边距 1 英寸
        let document = r#"<?xml version="1.0" encoding="UTF-8"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><w:body><w:p><w:r><w:t>旧内容</w:t></w:r></w:p><w:sectPr><w:headerReference w:type="default" r:id="rId9"/><w:pgSz w:w="16838" w:h="11906" w:orient="landscape"/><w:pgMar w:top="1440" w:right="1440" w:bottom="1440" w:left="1440"/></w:sectPr></w:body></w:document>"#;
        let c = build(opts(DocKind::Design), sample(), vec![diagram()]);
        let parts = unzip(&write(&c, Some(&reference_with(styles, document)), Lang::En).unwrap());
        let doc = &parts["word/document.xml"];
        assert!(doc.contains("<w:pStyle w:val=\"1\"/>") && doc.contains("<w:pStyle w:val=\"a3\"/>"));
        assert!(!doc.contains("旧内容"), "原来的正文被替换");
        assert!(doc.contains("<w:headerReference w:type=\"default\" r:id=\"rId9\"/>"), "保留节属性");
        assert!(doc.contains("<w:tblStyle w:val=\"DBLensTableGrid\"/>"));
        assert!(doc.contains("xmlns:wp="), "补上图片的命名空间");
        // 版心宽 16838 − 2 × 1440 = 13958 twip
        assert!(doc.contains("<w:tblW w:w=\"13958\""));
        let s = &parts["word/styles.xml"];
        assert!(s.contains("w:styleId=\"DBLensTableGrid\"") && s.contains("w:styleId=\"DBLensCode\""));
        assert!(!s.contains("w:styleId=\"DBLensHeading1\""), "已有的样式不重复补");
    }

    #[test]
    fn rejects_invalid_reference_and_strips_control_chars() {
        assert!(matches!(check_reference(b"not a zip"), Err(Error::Invalid { .. })));
        let mut only_doc = ZipWriter::new(Cursor::new(Vec::new()));
        only_doc.start_file("word/document.xml", SimpleFileOptions::default()).unwrap();
        only_doc.write_all(b"<w:document/>").unwrap();
        assert!(check_reference(&only_doc.finish().unwrap().into_inner()).is_err());
        assert!(check_reference(&builtin_reference()).is_ok());
        let mut o = opts(DocKind::Dictionary);
        o.info.title = "a\u{1}b<c>".into();
        let c = build(o, sample(), vec![]);
        let doc = &unzip(&write(&c, None, Lang::En).unwrap())["word/document.xml"];
        assert!(doc.contains(">ab&lt;c&gt;<"));
    }
}
