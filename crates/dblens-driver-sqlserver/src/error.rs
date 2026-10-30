use dblens_core::{Error, Msg, char_position};
use tiberius::error::Error as TdsError;

/// 登录失败（密码错误、账号不存在或被禁用、无权访问默认库时也报这个）。
const LOGIN_FAILED: u32 = 18456;
/// 无法打开登录所请求的数据库。
const CANNOT_OPEN_DATABASE: u32 = 4060;

pub(crate) fn map_err(e: TdsError) -> Error {
    match e {
        TdsError::Server(ref t) => match t.code() {
            LOGIN_FAILED | CANNOT_OPEN_DATABASE => Error::Auth { message: Msg::raw(t.message()) },
            code => Error::Sql { code: code.to_string(), message: t.message().to_string(), position: None },
        },
        TdsError::Io { .. } | TdsError::Tls(_) | TdsError::Routing { .. } => Error::connect(e.to_string()),
        other => Error::internal(other.to_string()),
    }
}

/// 服务器报出的批内行号（从 1 开始），没有时为 None。
pub(crate) fn server_line(e: &TdsError) -> Option<u32> {
    match e {
        TdsError::Server(t) if t.line() > 0 => Some(t.line()),
        _ => None,
    }
}

/// 语法错误（102 `Incorrect syntax near 'x'.`、156 `… near the keyword 'FROM'.`）中出错处的原文。
/// 消息的语言随登录名的默认语言，所以按错误号判断，取引号里的部分。
fn near<'a>(code: &str, message: &'a str) -> Option<&'a str> {
    if !matches!(code, "102" | "156") {
        return None;
    }
    let (_, rest) = message.split_once('\'')?;
    rest.rsplit_once('\'').map(|(token, _)| token).filter(|t| !t.is_empty())
}

/// 把批内行号换算成在批文本中的位置（字符偏移，从 1 开始）：语法错误能在该行找到出错处的原文时指向它，
/// 否则指向该行第一个非空白字符。
pub(crate) fn with_line(e: Error, sql: &str, line: Option<u32>) -> Error {
    match (e, line) {
        (Error::Sql { code, message, position: None }, Some(line)) => {
            let start: usize = sql.split_inclusive('\n').take(line as usize - 1).map(str::len).sum();
            let start = start.min(sql.len());
            let text = sql[start..].split('\n').next().unwrap_or_default();
            let indent = text.len() - text.trim_start_matches([' ', '\t']).len();
            let at = near(&code, &message)
                .and_then(|token| text.to_ascii_lowercase().find(&token.to_ascii_lowercase()))
                .unwrap_or(indent);
            Error::Sql { code, message, position: Some(char_position(sql, start + at)) }
        }
        (e, _) => e,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sql_error(message: &str) -> Error {
        Error::Sql { code: "102".into(), message: message.into(), position: None }
    }

    #[test]
    fn line_becomes_position() {
        let sql = "SELECT '数洞'\n  SELECT * FROM nope";
        // 第 2 行的 SELECT 前面有 11 个字符、一个换行、两个空格，位置从 1 开始
        let e = with_line(sql_error("Invalid object name 'nope2'."), sql, Some(2));
        assert!(matches!(e, Error::Sql { position: Some(15), .. }), "{e:?}");
        assert!(matches!(with_line(sql_error("x"), sql, None), Error::Sql { position: None, .. }));
        assert!(matches!(with_line(sql_error("x"), sql, Some(9)), Error::Sql { position: Some(_), .. }));
    }

    #[test]
    fn syntax_error_points_at_the_token() {
        let sql = "SELECT '数洞', FROM t";
        let e = with_line(sql_error("Incorrect syntax near the keyword 'FROM'."), sql, Some(1));
        let Error::Sql { position: Some(p), .. } = e else { panic!("{e:?}") };
        assert_eq!(p as usize, sql[..sql.find("FROM").unwrap()].chars().count() + 1);
        assert_eq!(near("102", "Incorrect syntax near ')'."), Some(")"));
        assert_eq!(near("208", "Invalid object name 'x'."), None);
    }

    #[test]
    fn syntax_error_in_other_languages() {
        // 服务器的消息语言随登录名的默认语言（中文 Windows 上装的实例默认是简体中文）
        let sql = "SELECT '数洞', FROM t";
        let e = with_line(
            Error::Sql {
                code: "156".into(),
                message: "关键字 'FROM' 附近有语法错误。".into(),
                position: None,
            },
            sql,
            Some(1),
        );
        let Error::Sql { position: Some(p), .. } = e else { panic!("{e:?}") };
        assert_eq!(p as usize, sql[..sql.find("FROM").unwrap()].chars().count() + 1);
        assert_eq!(near("102", "')' 附近有语法错误。"), Some(")"));
    }
}
