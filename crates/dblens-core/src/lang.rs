//! 界面语言（M1i 设计文档第 5.2 节）：接口层按请求设定"当前语言"，`Msg` 渲染时读取；
//! 没有请求语言的场合（日志、命令行、桌面对话框）用进程级的默认语言。

use std::future::Future;
use std::sync::atomic::{AtomicU8, Ordering};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Lang {
    ZhCn,
    En,
}

impl Lang {
    pub const ALL: [Lang; 2] = [Lang::ZhCn, Lang::En];

    /// 词条文件中的语言代码。
    pub fn code(self) -> &'static str {
        match self {
            Lang::ZhCn => "zh-CN",
            Lang::En => "en",
        }
    }

    /// 语言标签（如 `zh-CN`、`zh-Hans-CN`、`en-US`）：以 `zh` 开头的取中文，其余取英文。
    pub fn from_tag(tag: &str) -> Lang {
        if tag.trim().to_ascii_lowercase().starts_with("zh") { Lang::ZhCn } else { Lang::En }
    }

    /// `Accept-Language` 头：取第一个语言（不看权重，浏览器总是把首选语言放在最前）。空头返回 None。
    pub fn from_accept_language(header: &str) -> Option<Lang> {
        let first = header.split(',').next()?.split(';').next()?.trim();
        (!first.is_empty() && first != "*").then(|| Lang::from_tag(first))
    }

    /// 设置项 `ui.lang` 的值：`zh-CN` / `en` 为指定语言，`auto` 或其他值为 None（跟随系统）。
    pub fn from_setting(value: &str) -> Option<Lang> {
        match value {
            "zh-CN" => Some(Lang::ZhCn),
            "en" => Some(Lang::En),
            _ => None,
        }
    }

    /// 操作系统的语言；读不到时取英文。
    pub fn system() -> Lang {
        sys_locale::get_locale().map(|l| Lang::from_tag(&l)).unwrap_or(Lang::En)
    }
}

/// 进程级默认语言：0 表示未设置（按环境变量 `DBLENS_LANG`，没有时按系统语言），1 中文，2 英文。
static DEFAULT: AtomicU8 = AtomicU8::new(0);

/// 设定进程级默认语言；None 表示跟随系统。
pub fn set_default_lang(lang: Option<Lang>) {
    let v = match lang {
        None => 0,
        Some(Lang::ZhCn) => 1,
        Some(Lang::En) => 2,
    };
    DEFAULT.store(v, Ordering::Relaxed);
}

pub fn default_lang() -> Lang {
    match DEFAULT.load(Ordering::Relaxed) {
        1 => Lang::ZhCn,
        2 => Lang::En,
        _ => std::env::var("DBLENS_LANG")
            .ok()
            .filter(|v| !v.is_empty())
            .map_or_else(Lang::system, |v| Lang::from_tag(&v)),
    }
}

tokio::task_local! {
    static CURRENT: Lang;
}

/// 当前语言：在 `scope_lang` / `with_lang` 之内取设定的语言，否则取默认语言。
pub fn current_lang() -> Lang {
    CURRENT.try_with(|l| *l).unwrap_or_else(|_| default_lang())
}

/// 在指定语言下运行一个异步任务（接口层用来包住整个请求处理，含响应的序列化）。
pub async fn scope_lang<F: Future>(lang: Lang, f: F) -> F::Output {
    CURRENT.scope(lang, f).await
}

/// 在指定语言下同步执行（如序列化一个 WebSocket 事件）。
pub fn with_lang<R>(lang: Lang, f: impl FnOnce() -> R) -> R {
    CURRENT.sync_scope(lang, f)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_tags_and_headers() {
        assert_eq!(Lang::from_tag("zh-CN"), Lang::ZhCn);
        assert_eq!(Lang::from_tag("zh-Hans-CN"), Lang::ZhCn);
        assert_eq!(Lang::from_tag("ZH"), Lang::ZhCn);
        assert_eq!(Lang::from_tag("en-US"), Lang::En);
        assert_eq!(Lang::from_tag("ja"), Lang::En);
        assert_eq!(Lang::from_accept_language("zh-CN,zh;q=0.9,en;q=0.8"), Some(Lang::ZhCn));
        assert_eq!(Lang::from_accept_language("en-US,en;q=0.9"), Some(Lang::En));
        assert_eq!(Lang::from_accept_language(""), None);
        assert_eq!(Lang::from_accept_language("*"), None);
        assert_eq!(Lang::from_setting("en"), Some(Lang::En));
        assert_eq!(Lang::from_setting("auto"), None);
    }

    #[test]
    fn scoped_language_wins_over_default() {
        // 默认语言是进程级的，测试并行时不去改它，只验证作用域
        assert_eq!(with_lang(Lang::En, current_lang), Lang::En);
        assert_eq!(with_lang(Lang::ZhCn, current_lang), Lang::ZhCn);
        let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
        assert_eq!(rt.block_on(scope_lang(Lang::En, async { current_lang() })), Lang::En);
        assert_eq!(
            rt.block_on(scope_lang(Lang::ZhCn, async { with_lang(Lang::En, current_lang) })),
            Lang::En
        );
    }
}
