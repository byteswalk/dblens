//! 文档中的固定文字（`labels.json`）：模板用 `t('键')` 取，docx、xlsx 的写法用 `label`。
//! 与后端词条分开放：模板可由用户修改，这些键不经过 `msg!`。

use std::collections::HashMap;
use std::sync::OnceLock;

use dblens_core::Lang;

type Labels = HashMap<String, HashMap<String, String>>;

fn labels() -> &'static Labels {
    static LABELS: OnceLock<Labels> = OnceLock::new();
    LABELS.get_or_init(|| serde_json::from_str(include_str!("labels.json")).unwrap_or_default())
}

/// 按语言取固定文字；没有这个键时原样返回键名。
pub fn label(lang: Lang, key: &str) -> String {
    labels().get(key).and_then(|m| m.get(lang.code())).cloned().unwrap_or_else(|| key.to_string())
}

#[cfg(test)]
pub(crate) fn keys() -> Vec<String> {
    labels().keys().cloned().collect()
}
