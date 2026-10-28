//! html、markdown 的模板（M4c 设计文档 5.1 节）：minijinja，内置一套，可换成用户的模板文件。
//! 模板只能使用传入的内容：没有加载器，`include`、`import` 找不到其他模板。

use base64::Engine;
use dblens_core::{Error, Lang, Result, msg};
use minijinja::syntax::SyntaxConfig;
use minijinja::value::{Serde, Value};
use minijinja::{AutoEscape, Environment};
use serde::{Deserialize, Serialize};

use super::content::DocContent;
use super::labels::label;

/// 用模板写的格式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextFormat {
    Html,
    Markdown,
}

impl TextFormat {
    /// 内置模板。
    pub fn builtin(self) -> &'static str {
        match self {
            TextFormat::Html => include_str!("templates/document.html"),
            TextFormat::Markdown => include_str!("templates/document.md"),
        }
    }

    /// 模板在环境中的名字（扩展名决定是否做 HTML 转义）。
    fn name(self) -> &'static str {
        match self {
            TextFormat::Html => "document.html",
            TextFormat::Markdown => "document.md",
        }
    }
}

/// markdown 表格单元格：`|` 转义，换行写成 `<br>`，空值写成空。
fn cell(v: Value) -> String {
    if v.is_none() || v.is_undefined() {
        return String::new();
    }
    v.to_string().replace('|', "\\|").replace("\r\n", "<br>").replace('\n', "<br>")
}

/// 空值写成空，其余原样。
fn text(v: Value) -> Value {
    if v.is_none() || v.is_undefined() { Value::from("") } else { v }
}

fn template_error(e: &minijinja::Error) -> Error {
    let mut detail = e.detail().map(str::to_string).unwrap_or_else(|| e.kind().to_string());
    if let Some(source) = std::error::Error::source(e) {
        detail = format!("{detail}: {source}");
    }
    Error::invalid(msg!("document.template_error", line = e.line().unwrap_or(0), error = detail))
}

fn environment(lang: Lang) -> Environment<'static> {
    let mut env = Environment::new();
    if let Ok(syntax) =
        SyntaxConfig::builder().trim_blocks(true).lstrip_blocks(true).keep_trailing_newline(true).build()
    {
        env.set_syntax(syntax);
    }
    env.set_auto_escape_callback(
        |name| if name.ends_with(".html") { AutoEscape::Html } else { AutoEscape::None },
    );
    env.add_function("t", move |key: &str| label(lang, key));
    env.add_filter("cell", cell);
    env.add_filter("v", text);
    env
}

/// 检查模板的语法。
pub fn check(format: TextFormat, source: &str) -> Result<()> {
    let mut env = environment(Lang::En);
    env.add_template_owned(format.name(), source.to_string()).map_err(|e| template_error(&e))
}

/// 按模板写出文档。`template` 为 None 时用内置模板；`image_dir` 是 markdown 中图片的相对目录。
pub fn render(
    content: &DocContent,
    format: TextFormat,
    template: Option<&str>,
    lang: Lang,
    image_dir: &str,
) -> Result<String> {
    let mut env = environment(lang);
    env.add_template_owned(format.name(), template.unwrap_or(format.builtin()).to_string())
        .map_err(|e| template_error(&e))?;
    let mut content = content.clone();
    for d in &mut content.diagrams {
        d.src = match format {
            TextFormat::Html => {
                format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(&d.png))
            }
            TextFormat::Markdown => format!("{image_dir}/{}", d.file),
        };
    }
    let mut ctx = serde_json::to_value(&content)
        .map_err(|e| Error::internal(msg!("document.write_failed", error = e)))?;
    if let Some(m) = ctx.as_object_mut() {
        m.insert("lang".into(), lang.code().into());
    }
    env.get_template(format.name())
        .and_then(|t| t.render(Value::from(Serde(&ctx))))
        .map_err(|e| template_error(&e))
}

#[cfg(test)]
mod tests {
    use super::super::content::tests::{opts, sample};
    use super::super::content::{DocDiagram, DocKind, build};
    use super::*;

    fn diagram() -> DocDiagram {
        DocDiagram {
            schema: "shop".into(),
            name: "_all".into(),
            title: "shop".into(),
            description: Some("全部表".into()),
            names_only: true,
            file: "01-shop-_all.png".into(),
            src: String::new(),
            width: 2,
            height: 1,
            png: vec![1, 2, 3],
        }
    }

    #[test]
    fn markdown_snapshots() {
        let c = build(opts(DocKind::Dictionary), sample(), vec![]);
        insta::assert_snapshot!(
            "dictionary_md_zh",
            render(&c, TextFormat::Markdown, None, Lang::ZhCn, "x").unwrap()
        );
        let c = build(opts(DocKind::Design), sample(), vec![diagram()]);
        insta::assert_snapshot!(
            "design_md_en",
            render(&c, TextFormat::Markdown, None, Lang::En, "shop_images").unwrap()
        );
    }

    #[test]
    fn html_snapshot_escapes_and_embeds_images() {
        let mut o = opts(DocKind::Design);
        o.info.title = "<商城> & 订单".into();
        let c = build(o, sample(), vec![diagram()]);
        let html = render(&c, TextFormat::Html, None, Lang::ZhCn, "").unwrap();
        assert!(html.contains("<title>&lt;商城&gt; &amp; 订单</title>"));
        assert!(html.contains(r#"<img src="data:image/png;base64,AQID""#));
        insta::assert_snapshot!("design_html_zh", html);
    }

    #[test]
    fn custom_template_errors_and_no_includes() {
        let c = build(opts(DocKind::Dictionary), sample(), vec![]);
        let out = render(
            &c,
            TextFormat::Markdown,
            Some("{{ info.title }}|{{ t('doc.col.table') }}|{{ none|v }}|{{ 'a|b'|cell }}"),
            Lang::En,
            "",
        )
        .unwrap();
        assert_eq!(out, "商城|Table||a\\|b");
        // 语法错误：带行号
        let e = check(TextFormat::Html, "line 1\n{% for x in %}").unwrap_err();
        assert!(e.to_string().contains('2'), "{e}");
        // 不能读其他模板或文件
        let e = render(&c, TextFormat::Markdown, Some("{% include 'C:/Windows/win.ini' %}"), Lang::En, "")
            .unwrap_err();
        assert!(matches!(e, Error::Invalid { .. }), "{e:?}");
    }

    #[test]
    fn every_label_is_used_and_exists() {
        let sources = [TextFormat::Html.builtin(), TextFormat::Markdown.builtin()];
        let used: std::collections::BTreeSet<String> = sources
            .iter()
            .flat_map(|s| s.match_indices("doc.").map(move |(i, _)| &s[i..]))
            .map(|s| {
                s.chars()
                    .take_while(|c| c.is_ascii_alphanumeric() || *c == '.' || *c == '_')
                    .collect::<String>()
            })
            .map(|k| k.trim_end_matches('.').to_string())
            .collect();
        let keys = super::super::labels::keys();
        // 用 `'doc.kind.' ~ 变量` 拼出来的键按前缀算
        let prefixes: Vec<&String> =
            used.iter().filter(|k| k.ends_with("kind") || k.ends_with("flag")).collect();
        for k in &keys {
            let dynamic = prefixes.iter().any(|p| k.starts_with(&format!("{p}.")));
            assert!(used.contains(k) || dynamic, "词条 {k} 没有被使用");
        }
        for k in used.iter().filter(|k| !prefixes.contains(k)) {
            assert!(keys.contains(k), "词条 {k} 不存在");
        }
    }
}
