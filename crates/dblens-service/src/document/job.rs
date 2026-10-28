//! 文档导出的后台任务（M4c 设计文档 6.1 节）：读结构、组织内容、按格式写出，写到选定的路径或任务目录。

use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use dblens_core::{Error, Result, current_lang, msg};
use dblens_dialect::meta::{MetaArgs, MetaKey};
use serde::{Deserialize, Serialize};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

use super::content::{
    BuildOptions, DocDiagram, DocInfo, DocKind, SourceSchema, SourceTable, build, png_size,
};
use super::template::{TextFormat, check, render};
use super::{docx, xlsx};
use crate::model::DEFAULT_DIAGRAM;
use crate::service::Service;
use crate::task::{FileSource, TaskKind, TaskPhase};

/// 文档格式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DocFormat {
    Docx,
    Html,
    Markdown,
    /// 只用于数据字典。
    Xlsx,
}

impl DocFormat {
    fn extension(self) -> &'static str {
        match self {
            DocFormat::Docx => "docx",
            DocFormat::Html => "html",
            DocFormat::Markdown => "md",
            DocFormat::Xlsx => "xlsx",
        }
    }

    fn text(self) -> Option<TextFormat> {
        match self {
            DocFormat::Html => Some(TextFormat::Html),
            DocFormat::Markdown => Some(TextFormat::Markdown),
            _ => None,
        }
    }
}

/// 文档的来源。
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DocSource {
    /// 打开的模型中的若干 schema。
    Model { model: String, schemas: Vec<String> },
    /// 连接的一个 schema。
    Database { profile: String, schema: String },
}

/// 一张 ER 图的图片（前端画好后上传）。
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct DiagramImage {
    pub upload: String,
    pub schema: String,
    pub name: String,
    pub title: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub names_only: bool,
}

fn yes() -> bool {
    true
}

/// 开始导出的请求。
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct DocumentRequest {
    pub source: DocSource,
    /// 只放这些表（只有一个 schema 时）。
    #[serde(default)]
    pub tables: Option<Vec<String>>,
    pub kind: DocKind,
    pub format: DocFormat,
    #[serde(default)]
    pub info: DocInfo,
    /// html、markdown 的模板文件（本机路径）。
    #[serde(default)]
    pub template: Option<PathBuf>,
    /// docx 的样式来源（本机路径）。
    #[serde(default)]
    pub reference: Option<PathBuf>,
    #[serde(default = "yes")]
    pub view_definitions: bool,
    #[serde(default)]
    pub diagrams: Vec<DiagramImage>,
    /// 桌面版：写到这个路径；没有时写到任务目录，完成后下载。
    #[serde(default)]
    pub output: Option<PathBuf>,
}

/// 开始前检查出的问题。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DocumentProblem {
    /// `template`、`reference`、`output`、`format`。
    pub field: &'static str,
    pub error: Error,
}

fn read_file(path: &Path) -> Result<Vec<u8>> {
    std::fs::read(path)
        .map_err(|e| Error::invalid(msg!("document.read_failed", path = path.display(), error = e)))
}

/// 文件名中不能用的字符换成下划线。
fn file_name(name: &str) -> String {
    let s: String =
        name.chars().map(|c| if r#"\/:*?"<>|"#.contains(c) || c.is_control() { '_' } else { c }).collect();
    let s = s.trim().trim_end_matches('.').to_string();
    if s.is_empty() { "document".into() } else { s }
}

/// 写文件：先写临时文件再改名。
fn write_atomic(path: &Path, data: &[u8]) -> Result<()> {
    let tmp = path.with_extension("dblens-tmp");
    let fail = |e: std::io::Error| Error::internal(msg!("document.write_failed", error = e));
    std::fs::write(&tmp, data).map_err(fail)?;
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        fail(e)
    })
}

impl Service {
    /// 开始前的检查：格式与种类、模板语法、样式来源、输出位置。`allow_path` 为 false 时（浏览器模式）不接受本机路径。
    pub fn check_document(&self, req: &DocumentRequest, allow_path: bool) -> Vec<DocumentProblem> {
        let mut out = Vec::new();
        let mut push = |field, error: Error| out.push(DocumentProblem { field, error });
        if req.format == DocFormat::Xlsx && req.kind == DocKind::Design {
            push("format", Error::invalid(msg!("document.xlsx_dictionary_only")));
        }
        for (field, path) in
            [("template", &req.template), ("reference", &req.reference), ("output", &req.output)]
        {
            if path.is_some() && !allow_path {
                push(field, Error::invalid(msg!("export.path_not_allowed")));
            }
        }
        if !allow_path {
            return out;
        }
        if let Some(p) = &req.template {
            match (read_file(p), req.format.text()) {
                (Err(e), _) => push("template", e),
                (Ok(bytes), Some(f)) => {
                    if let Err(e) = check(f, &String::from_utf8_lossy(&bytes)) {
                        push("template", e);
                    }
                }
                (Ok(_), None) => {}
            }
        }
        if let Some(p) = &req.reference
            && req.format == DocFormat::Docx
            && let Err(e) = read_file(p).and_then(|b| docx::check_reference(&b))
        {
            push("reference", e);
        }
        if let Some(p) = &req.output {
            let dir = p.parent().filter(|d| !d.as_os_str().is_empty());
            if !p.is_absolute() || dir.is_none_or(|d| !d.is_dir()) {
                push("output", Error::invalid(msg!("document.bad_output", path = p.display())));
            }
        }
        out
    }

    /// 读一个来源的结构（模型的 schema 带逻辑外键和读不出的表；库的 schema 带视图）。
    async fn document_sources(
        &self,
        source: &DocSource,
        design: bool,
        done: impl Fn(),
    ) -> Result<Vec<SourceSchema>> {
        let mut out = Vec::new();
        match source {
            DocSource::Model { model, schemas } => {
                for schema in schemas {
                    let view = self.models.read_diagram(model, schema, DEFAULT_DIAGRAM)?;
                    let mut s = SourceSchema { name: schema.clone(), ..Default::default() };
                    for t in view.tables {
                        match t.error {
                            Some(e) => s.errors.push((t.table.name, e)),
                            None => {
                                s.tables.push(SourceTable { table: t.table, logical_keys: t.logical_keys })
                            }
                        }
                    }
                    out.push(s);
                    done();
                }
            }
            DocSource::Database { profile, schema } => {
                let p = self.profile(profile)?;
                // 跳过元数据缓存，以库中最新的结构为准
                let args = MetaArgs { schema: Some(schema.clone()), ..Default::default() };
                for key in MetaKey::TABLE_DETAIL.into_iter().chain([MetaKey::Views]) {
                    self.metadata.cache().forget(&p.id, key, &args);
                }
                let tables = self.metadata.tables(&p, schema, None).await?;
                let views =
                    if design { self.metadata.views(&p, schema, MetaKey::Views).await? } else { vec![] };
                out.push(SourceSchema {
                    name: schema.clone(),
                    tables: tables
                        .into_iter()
                        .map(|table| SourceTable { table, logical_keys: vec![] })
                        .collect(),
                    views,
                    errors: vec![],
                });
                done();
            }
        }
        Ok(out)
    }

    /// 来源的说明和默认标题。
    fn document_origin(&self, source: &DocSource) -> Result<(String, String)> {
        match source {
            DocSource::Model { model, .. } => {
                let m = self.models.summary_of(model)?;
                Ok((format!("{} ({})", m.info.name, m.path), m.info.name))
            }
            DocSource::Database { profile, schema } => {
                let p = self.profile(profile)?;
                Ok((format!("{} / {schema}", p.input.name), schema.clone()))
            }
        }
    }

    /// 开始导出：作为任务执行，返回任务 id。浏览器模式（`allow_path` 为 false）不接受本机路径，结果在任务完成后下载。
    pub async fn start_document(self: &Arc<Self>, req: DocumentRequest, allow_path: bool) -> Result<String> {
        if let Some(p) = self.check_document(&req, allow_path).into_iter().next() {
            return Err(p.error);
        }
        let lang = current_lang();
        let template = match &req.template {
            Some(p) if req.format.text().is_some() => {
                Some(String::from_utf8_lossy(&read_file(p)?).to_string())
            }
            _ => None,
        };
        let reference = match &req.reference {
            Some(p) if req.format == DocFormat::Docx => Some(read_file(p)?),
            _ => None,
        };
        let (origin, default_title) = self.document_origin(&req.source)?;
        // 图片：任务开始前取走上传的文件
        let mut diagrams = Vec::new();
        if req.kind == DocKind::Design {
            for (i, d) in req.diagrams.iter().enumerate() {
                let f = self.tasks.resolve(&FileSource::Upload { upload: d.upload.clone() }, false, true)?;
                let png = std::fs::read(&f.path);
                f.release();
                let png =
                    png.map_err(|e| Error::invalid(msg!("document.read_failed", path = &f.name, error = e)))?;
                let (width, height) = png_size(&png)
                    .ok_or_else(|| Error::invalid(msg!("document.bad_image", name = &d.title)))?;
                diagrams.push(DocDiagram {
                    schema: d.schema.clone(),
                    name: d.name.clone(),
                    title: d.title.clone(),
                    description: d.description.clone(),
                    names_only: d.names_only,
                    file: file_name(&format!("{:02}-{}-{}.png", i + 1, d.schema, d.name)),
                    src: String::new(),
                    width,
                    height,
                    png,
                });
            }
        }
        let schemas = match &req.source {
            DocSource::Model { schemas, .. } => schemas.len(),
            DocSource::Database { .. } => 1,
        };
        if schemas == 0 {
            return Err(Error::invalid(msg!("model.no_schemas")));
        }
        let task = self.tasks.create(TaskKind::Document)?;
        let mut id = String::new();
        task.update(|s| {
            id = s.id.clone();
            s.phase = TaskPhase::Scanning;
            s.progress.total = schemas as u64;
        });
        let service = self.clone();
        tokio::spawn(async move {
            let run = async {
                let design = req.kind == DocKind::Design;
                let sources = service
                    .document_sources(&req.source, design, || task.update(|s| s.progress.read += 1))
                    .await?;
                if task.cancelled() {
                    return Err(Error::Cancelled);
                }
                task.update(|s| s.phase = TaskPhase::Writing);
                let mut info = req.info.clone();
                if info.title.trim().is_empty() {
                    info.title = default_title;
                }
                info.generated = chrono::Local::now().format("%Y-%m-%d %H:%M").to_string();
                info.source = origin;
                let name = file_name(&info.title);
                let content = build(
                    BuildOptions {
                        kind: req.kind,
                        info,
                        tables: req.tables.clone(),
                        view_definitions: req.view_definitions,
                    },
                    sources,
                    diagrams,
                );
                let images = format!("{name}_images");
                let main = match req.format {
                    DocFormat::Docx => docx::write(&content, reference.as_deref(), lang)?,
                    DocFormat::Xlsx => {
                        let (bytes, truncated) = xlsx::write(&content, lang)?;
                        task.update(|s| s.warnings = truncated);
                        bytes
                    }
                    DocFormat::Html | DocFormat::Markdown => {
                        let f = req.format.text().unwrap_or(TextFormat::Html);
                        render(&content, f, template.as_deref(), lang, &images)?.into_bytes()
                    }
                };
                // markdown 的图片放在旁边的目录中
                let pictures: Vec<(String, &[u8])> = if req.format == DocFormat::Markdown {
                    content.diagrams.iter().map(|d| (d.file.clone(), d.png.as_slice())).collect()
                } else {
                    vec![]
                };
                if task.cancelled() {
                    return Err(Error::Cancelled);
                }
                let ext = req.format.extension();
                match &req.output {
                    Some(path) => {
                        write_atomic(path, &main)?;
                        if !pictures.is_empty() {
                            let stem =
                                path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or(name);
                            let dir = path.with_file_name(format!("{stem}_images"));
                            std::fs::create_dir_all(&dir)
                                .map_err(|e| Error::internal(msg!("document.write_failed", error = e)))?;
                            for (file, png) in &pictures {
                                write_atomic(&dir.join(file), png)?;
                            }
                        }
                    }
                    None if pictures.is_empty() => {
                        let path = task.report_path(ext, format!("{name}.{ext}"))?;
                        write_atomic(&path, &main)?;
                    }
                    None => {
                        // 浏览器模式下 markdown 带图片：打包成 zip
                        let path = task.report_path("zip", format!("{name}.zip"))?;
                        let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
                        let opts =
                            SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
                        let fail = |e: &dyn std::fmt::Display| {
                            Error::internal(msg!("document.write_failed", error = e))
                        };
                        zip.start_file(format!("{name}.{ext}"), opts).map_err(|e| fail(&e))?;
                        zip.write_all(&main).map_err(|e| fail(&e))?;
                        for (file, png) in &pictures {
                            zip.start_file(format!("{images}/{file}"), opts).map_err(|e| fail(&e))?;
                            zip.write_all(png).map_err(|e| fail(&e))?;
                        }
                        let bytes = zip.finish().map_err(|e| fail(&e))?.into_inner();
                        write_atomic(&path, &bytes)?;
                    }
                }
                Ok(())
            };
            let result = run.await;
            task.finish(result);
        });
        Ok(id)
    }

    /// 把一份内置模板写到指定路径（只有本机服务可用）。
    pub fn export_document_template(&self, format: TextFormat, path: &Path) -> Result<()> {
        write_atomic(path, format.builtin().as_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::tests::{service, wait};

    fn req(format: DocFormat, kind: DocKind) -> DocumentRequest {
        DocumentRequest {
            source: DocSource::Database { profile: "p".into(), schema: "s".into() },
            tables: None,
            kind,
            format,
            info: DocInfo::default(),
            template: None,
            reference: None,
            view_definitions: true,
            diagrams: vec![],
            output: None,
        }
    }

    #[test]
    fn checks_before_start() {
        let (s, _, _dir) = service();
        assert!(s.check_document(&req(DocFormat::Markdown, DocKind::Design), false).is_empty());
        let p = s.check_document(&req(DocFormat::Xlsx, DocKind::Design), true);
        assert_eq!(p[0].field, "format");
        // 浏览器模式不接受本机路径
        let mut r = req(DocFormat::Html, DocKind::Dictionary);
        r.template = Some("C:/t.html".into());
        r.output = Some("C:/out.html".into());
        let fields: Vec<_> = s.check_document(&r, false).iter().map(|p| p.field).collect();
        assert_eq!(fields, ["template", "output"]);
        // 模板语法错误、样式来源无效、输出目录不存在
        let dir = tempfile::tempdir().unwrap();
        let t = dir.path().join("t.html");
        std::fs::write(&t, "{% for %}").unwrap();
        let bad = dir.path().join("ref.docx");
        std::fs::write(&bad, "not a docx").unwrap();
        let mut r = req(DocFormat::Html, DocKind::Dictionary);
        r.template = Some(t);
        r.output = Some(dir.path().join("missing").join("x.html"));
        let fields: Vec<_> = s.check_document(&r, true).iter().map(|p| p.field).collect();
        assert_eq!(fields, ["template", "output"]);
        let mut r = req(DocFormat::Docx, DocKind::Dictionary);
        r.reference = Some(bad);
        r.output = Some(dir.path().join("x.docx"));
        let fields: Vec<_> = s.check_document(&r, true).iter().map(|p| p.field).collect();
        assert_eq!(fields, ["reference"]);
    }

    #[test]
    fn safe_file_names() {
        assert_eq!(file_name("a/b:c*?"), "a_b_c__");
        assert_eq!(file_name("  "), "document");
        assert_eq!(file_name("报告."), "报告");
    }

    fn png() -> Vec<u8> {
        let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
        png.extend(800u32.to_be_bytes());
        png.extend(300u32.to_be_bytes());
        png
    }

    /// 打开一个有两张表、一个逻辑外键的模型。
    fn model(s: &Service, dir: &Path) -> String {
        use crate::document::content::tests::col;
        use crate::model::{LogicalKey, ModelDatabase};
        use dblens_core::schema::Table;
        use dblens_dialect::{Compat, Variant};
        let db = ModelDatabase { variant: Variant::Postgres, compat: Compat::Native, version: None };
        let tables = vec![
            Table {
                name: "orders".into(),
                columns: vec![col("id", "integer"), col("cust_id", "integer")],
                ..Default::default()
            },
            Table { name: "customers".into(), columns: vec![col("id", "integer")], ..Default::default() },
        ];
        let m = s.models.create_from(&dir.join("model"), "shop", db, &[("public".into(), tables)]).unwrap();
        let key = LogicalKey {
            name: None,
            columns: vec!["cust_id".into()],
            ref_schema: None,
            ref_table: "customers".into(),
            ref_columns: vec!["id".into()],
            comment: None,
        };
        s.models.add_logical_key(&m.id, "public", "orders", key, None).unwrap();
        m.id
    }

    fn upload(s: &Service) -> String {
        let (id, path) = s.tasks.begin_upload().unwrap();
        std::fs::write(&path, png()).unwrap();
        s.tasks.finish_upload(&id, "er.png", 1).unwrap();
        id
    }

    #[tokio::test]
    async fn model_to_markdown_file_with_images() {
        let (s, _, dir) = service();
        let id = model(&s, dir.path());
        let out = dir.path().join("shop.md");
        let mut r = req(DocFormat::Markdown, DocKind::Design);
        r.source = DocSource::Model { model: id, schemas: vec!["public".into()] };
        r.output = Some(out.clone());
        r.diagrams = vec![DiagramImage {
            upload: upload(&s),
            schema: "public".into(),
            name: "_all".into(),
            title: "public".into(),
            description: None,
            names_only: false,
        }];
        let task = s.start_document(r, true).await.unwrap();
        let st = wait(&s, &task).await;
        assert_eq!(st.state, crate::task::TaskState::Done, "{st:?}");
        let md = std::fs::read_to_string(&out).unwrap();
        // 标题默认取模型名；逻辑外键；图片在旁边的目录中
        assert!(
            md.starts_with(
                "# shop
"
            ),
            "{md}"
        );
        assert!(md.contains("public.customers") && md.contains("![public](shop_images/01-public-_all.png)"));
        assert_eq!(std::fs::read(dir.path().join("shop_images/01-public-_all.png")).unwrap(), png());
    }

    #[tokio::test]
    async fn browser_mode_zips_markdown_and_rejects_paths() {
        let (s, _, dir) = service();
        let id = model(&s, dir.path());
        let mut r = req(DocFormat::Markdown, DocKind::Design);
        r.source = DocSource::Model { model: id.clone(), schemas: vec!["public".into()] };
        r.info.title = "商城/文档".into();
        r.diagrams = vec![DiagramImage {
            upload: upload(&s),
            schema: "public".into(),
            name: "_all".into(),
            title: "public".into(),
            description: None,
            names_only: true,
        }];
        let task = s.start_document(r.clone(), false).await.unwrap();
        assert_eq!(wait(&s, &task).await.state, crate::task::TaskState::Done);
        let (path, name) = s.tasks.report(&task).unwrap();
        assert_eq!(name, "商城_文档.zip");
        let mut z = zip::ZipArchive::new(std::fs::File::open(path).unwrap()).unwrap();
        let names: Vec<String> =
            (0..z.len()).map(|i| z.by_index(i).unwrap().name().unwrap().to_string()).collect();
        assert_eq!(names, ["商城_文档.md", "商城_文档_images/01-public-_all.png"]);
        // 没有图片的格式直接下载文件
        let mut r2 = req(DocFormat::Xlsx, DocKind::Dictionary);
        r2.source = DocSource::Model { model: id, schemas: vec!["public".into()] };
        let task = s.start_document(r2, false).await.unwrap();
        assert_eq!(wait(&s, &task).await.state, crate::task::TaskState::Done);
        assert_eq!(s.tasks.report(&task).unwrap().1, "shop.xlsx");
        // 浏览器模式不接受本机路径
        r.output = Some(dir.path().join("x.md"));
        assert!(s.start_document(r, false).await.is_err());
    }
}
