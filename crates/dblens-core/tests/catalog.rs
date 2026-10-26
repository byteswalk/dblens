//! 词条检查（M1i 设计文档第 5.3 节）：扫描工作区各 crate 源码里的 `msg!(…)`，
//! 确认每个键在中英文词条里都有，参数名与词条里的 `%{…}` 一致；词条里没人用的键也报错。

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// 键 -> 语言 -> 文字
fn catalog() -> BTreeMap<String, BTreeMap<String, String>> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("locales/app.json");
    let mut v: serde_json::Value = serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap();
    v.as_object_mut().unwrap().remove("_version");
    serde_json::from_value(v).unwrap()
}

fn placeholders(text: &str) -> BTreeSet<String> {
    text.split("%{").skip(1).filter_map(|s| s.split_once('}')).map(|(name, _)| name.to_string()).collect()
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// 从 `msg!(` 之后的文字中取出整个参数列表（到配对的右括号为止，跳过字符串）。
fn macro_body(text: &str) -> &str {
    let (mut depth, mut in_str, mut escaped) = (1, false, false);
    for (i, c) in text.char_indices() {
        if in_str {
            match c {
                _ if escaped => escaped = false,
                '\\' => escaped = true,
                '"' => in_str = false,
                _ => {}
            }
            continue;
        }
        match c {
            '"' => in_str = true,
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => {
                depth -= 1;
                if depth == 0 {
                    return &text[..i];
                }
            }
            _ => {}
        }
    }
    panic!("msg! 没有闭合：{}", &text[..text.len().min(80)]);
}

/// 按顶层逗号切分参数列表。
fn split_top(body: &str) -> Vec<&str> {
    let (mut parts, mut start, mut depth, mut in_str, mut escaped) = (Vec::new(), 0, 0, false, false);
    for (i, c) in body.char_indices() {
        if in_str {
            match c {
                _ if escaped => escaped = false,
                '\\' => escaped = true,
                '"' => in_str = false,
                _ => {}
            }
            continue;
        }
        match c {
            '"' => in_str = true,
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth -= 1,
            ',' if depth == 0 => {
                parts.push(&body[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&body[start..]);
    parts.into_iter().map(str::trim).filter(|p| !p.is_empty()).collect()
}

/// 源码中所有 `msg!` 用法：(位置, 键, 参数名)
fn usages() -> Vec<(String, String, BTreeSet<String>)> {
    let mut files = Vec::new();
    for entry in fs::read_dir(workspace().join("crates")).unwrap() {
        let dir = entry.unwrap().path();
        for sub in ["src", "tests"] {
            if dir.join(sub).is_dir() {
                rust_files(&dir.join(sub), &mut files);
            }
        }
    }
    let mut out = Vec::new();
    for file in files {
        let text = fs::read_to_string(&file).unwrap();
        for (pos, _) in text.match_indices("msg!(") {
            // 宏定义本身和文档注释里的示例不算
            let line_start = text[..pos].rfind('\n').map_or(0, |i| i + 1);
            if text[line_start..pos].trim_start().starts_with("//") || text[..pos].ends_with("macro_rules! ")
            {
                continue;
            }
            let parts = split_top(macro_body(&text[pos + 5..]));
            let Some(key) = parts.first().and_then(|k| k.strip_prefix('"')).and_then(|k| k.strip_suffix('"'))
            else {
                continue; // 宏定义里的 `$key:literal` 等
            };
            let args = parts[1..]
                .iter()
                .filter_map(|p| p.split_once('='))
                .map(|(n, _)| n.trim().to_string())
                .collect();
            let line = text[..pos].matches('\n').count() + 1;
            let at = format!("{}:{line}", file.strip_prefix(workspace()).unwrap_or(&file).display());
            out.push((at, key.to_string(), args));
        }
    }
    out
}

#[test]
fn every_entry_has_both_languages_with_same_placeholders() {
    for (key, langs) in catalog() {
        let (Some(zh), Some(en)) = (langs.get("zh-CN"), langs.get("en")) else {
            panic!("词条 {key} 缺少中文或英文");
        };
        assert_eq!(langs.len(), 2, "词条 {key} 有多余的语言");
        assert!(!zh.is_empty() && !en.is_empty(), "词条 {key} 为空");
        assert_eq!(placeholders(zh), placeholders(en), "词条 {key} 中英文的参数不一致");
    }
}

#[test]
fn every_used_key_exists_with_matching_args_and_no_entry_is_unused() {
    let catalog = catalog();
    let usages = usages();
    assert!(!usages.is_empty());
    let mut problems = Vec::new();
    for (at, key, args) in &usages {
        match catalog.get(key) {
            None => problems.push(format!("{at}：词条 {key} 不存在")),
            Some(langs) => {
                let want = placeholders(&langs["zh-CN"]);
                if &want != args {
                    problems.push(format!("{at}：词条 {key} 的参数应为 {want:?}，实际 {args:?}"));
                }
            }
        }
    }
    let used: BTreeSet<&str> = usages.iter().map(|(_, k, _)| k.as_str()).collect();
    for key in catalog.keys().filter(|k| !used.contains(k.as_str())) {
        problems.push(format!("词条 {key} 没有被使用"));
    }
    assert!(problems.is_empty(), "\n{}", problems.join("\n"));
}
