//! 面向用户的文字（M1i 设计文档第 5.1 节）：以"词条键 + 命名参数"保存，序列化或显示时按当前语言渲染。
//! 词条在 `crates/dblens-core/locales/app.json`，全部 crate 共用；数据库返回的报错原文等不翻译的内容用 `Msg::Raw`。

use std::borrow::Cow;
use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::lang::{Lang, current_lang};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Msg {
    /// 词条键和命名参数。参数本身也是 `Msg`，与外层按同一种语言渲染。
    Text { key: Cow<'static, str>, args: Vec<(&'static str, Msg)> },
    /// 原样输出的文字。
    Raw(String),
}

/// 构造词条消息：`msg!("noun.session", id = 7)`。参数可以是实现了 `Display` 的值（原样输出），
/// 也可以是另一个 `Msg`（按同一种语言翻译）。
#[macro_export]
macro_rules! msg {
    ($key:literal $(, $name:ident = $value:expr)* $(,)?) => {
        $crate::Msg::text($key, vec![$((stringify!($name), $crate::ToMsg::to_msg(&$value))),*])
    };
}

/// `msg!` 的参数：`Msg` 保持可翻译，其他实现了 `Display` 的值转成原文。
pub trait ToMsg {
    fn to_msg(&self) -> Msg;
}

impl ToMsg for Msg {
    fn to_msg(&self) -> Msg {
        self.clone()
    }
}

impl ToMsg for &Msg {
    fn to_msg(&self) -> Msg {
        (*self).clone()
    }
}

impl<T: fmt::Display + ?Sized> ToMsg for T {
    fn to_msg(&self) -> Msg {
        Msg::Raw(self.to_string())
    }
}

impl Msg {
    pub fn text(key: &'static str, args: Vec<(&'static str, Msg)>) -> Msg {
        Msg::Text { key: Cow::Borrowed(key), args }
    }

    pub fn raw(text: impl Into<String>) -> Msg {
        Msg::Raw(text.into())
    }

    /// 按当前语言渲染（日志、命令行等场合）。`Msg` 不实现 `Display`，以免被当成普通文字提前渲染。
    pub fn current(&self) -> String {
        self.render(current_lang())
    }

    /// 按指定语言渲染。词条缺失时输出键名（测试保证不会发生）。
    pub fn render(&self, lang: Lang) -> String {
        match self {
            Msg::Raw(s) => s.clone(),
            Msg::Text { key, args } => {
                let template = crate::_rust_i18n_try_translate(lang.code(), key.as_ref())
                    .map(Cow::into_owned)
                    .unwrap_or_else(|| key.to_string());
                if args.is_empty() {
                    return template;
                }
                let names: Vec<&str> = args.iter().map(|(n, _)| *n).collect();
                let values: Vec<String> = args.iter().map(|(_, v)| v.render(lang)).collect();
                rust_i18n::replace_patterns(&template, &names, &values)
            }
        }
    }
}

impl From<String> for Msg {
    fn from(s: String) -> Msg {
        Msg::Raw(s)
    }
}

impl From<&str> for Msg {
    fn from(s: &str) -> Msg {
        Msg::Raw(s.to_string())
    }
}

/// 序列化为当前语言的文字，所以接口的 JSON 格式与改造前相同。
impl Serialize for Msg {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.render(current_lang()))
    }
}

/// 从文字反序列化时只能得到原文。
impl<'de> Deserialize<'de> for Msg {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Msg, D::Error> {
        String::deserialize(d).map(Msg::Raw)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lang::with_lang;

    #[test]
    fn renders_by_language_with_args() {
        let m = crate::msg!("noun.session", id = 7);
        assert_eq!(m.render(Lang::ZhCn), "会话 7");
        assert_eq!(m.render(Lang::En), "session 7");
        assert_eq!(with_lang(Lang::En, || m.current()), "session 7");
        // 嵌套的消息按外层的语言翻译
        let nested = crate::msg!("error.kind.not_found", what = m);
        assert_eq!(nested.render(Lang::ZhCn), "找不到会话 7");
        assert_eq!(nested.render(Lang::En), "Not found: session 7");
        assert_eq!(with_lang(Lang::ZhCn, || serde_json::to_string(&m).unwrap()), "\"会话 7\"");
    }

    #[test]
    fn raw_is_passed_through_and_unknown_key_shows_itself() {
        assert_eq!(Msg::raw("ORA-00942: 表或视图不存在").render(Lang::En), "ORA-00942: 表或视图不存在");
        assert_eq!(Msg::text("no.such.key", vec![]).render(Lang::En), "no.such.key");
        let back: Msg = serde_json::from_str("\"x\"").unwrap();
        assert_eq!(back, Msg::raw("x"));
    }
}
