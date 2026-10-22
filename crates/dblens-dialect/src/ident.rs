//! 标识符引用与参数占位符。

use crate::Family;

/// 总是加引号，并转义标识符中的引号字符。
pub fn quote_ident(family: Family, name: &str) -> String {
    match family {
        Family::MySql => format!("`{}`", name.replace('`', "``")),
        Family::Postgres | Family::Oracle | Family::Dameng => format!("\"{}\"", name.replace('"', "\"\"")),
    }
}

/// 只在必要时加引号：含特殊字符、以数字开头、是保留字，
/// 或者大小写与数据库的默认折叠方向不一致（PG 折叠为小写，Oracle/达梦折叠为大写）。
pub fn ident(family: Family, name: &str) -> String {
    if needs_quote(family, name) { quote_ident(family, name) } else { name.to_string() }
}

/// 参数占位符，`index` 从 1 开始。
pub fn placeholder(family: Family, index: usize) -> String {
    match family {
        Family::Postgres => format!("${index}"),
        Family::Oracle => format!(":{index}"),
        Family::MySql | Family::Dameng => "?".to_string(),
    }
}

fn needs_quote(family: Family, name: &str) -> bool {
    let Some(first) = name.chars().next() else { return true };
    if !(first.is_ascii_alphabetic() || first == '_') {
        return true;
    }
    let dollar_ok = family != Family::Postgres;
    if !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || (dollar_ok && c == '$')) {
        return true;
    }
    let case_ok = match family {
        Family::MySql => true,
        Family::Postgres => !name.chars().any(|c| c.is_ascii_uppercase()),
        Family::Oracle | Family::Dameng => !name.chars().any(|c| c.is_ascii_lowercase()),
    };
    !case_ok || RESERVED.contains(&name.to_ascii_uppercase().as_str())
}

/// 四类数据库常见保留字的并集。多加引号不会出错，所以宁多勿少。
const RESERVED: &[&str] = &[
    "ADD",
    "ALL",
    "ALTER",
    "AND",
    "ANY",
    "AS",
    "ASC",
    "BETWEEN",
    "BY",
    "CASE",
    "CHECK",
    "COLUMN",
    "CONSTRAINT",
    "CREATE",
    "CROSS",
    "CURRENT",
    "DATE",
    "DEFAULT",
    "DELETE",
    "DESC",
    "DISTINCT",
    "DROP",
    "ELSE",
    "END",
    "EXISTS",
    "FALSE",
    "FOR",
    "FOREIGN",
    "FROM",
    "FULL",
    "GRANT",
    "GROUP",
    "HAVING",
    "IN",
    "INDEX",
    "INNER",
    "INSERT",
    "INTERVAL",
    "INTO",
    "IS",
    "JOIN",
    "KEY",
    "LEFT",
    "LEVEL",
    "LIKE",
    "LIMIT",
    "NOT",
    "NULL",
    "NUMBER",
    "OF",
    "OFFSET",
    "ON",
    "OR",
    "ORDER",
    "OUTER",
    "PRIMARY",
    "REFERENCES",
    "RIGHT",
    "ROW",
    "ROWID",
    "ROWNUM",
    "ROWS",
    "SELECT",
    "SESSION",
    "SET",
    "SIZE",
    "TABLE",
    "THEN",
    "TO",
    "TRUE",
    "UNION",
    "UNIQUE",
    "UPDATE",
    "USER",
    "USING",
    "VALUES",
    "VIEW",
    "WHEN",
    "WHERE",
    "WITH",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quote_ident_escapes() {
        assert_eq!(quote_ident(Family::MySql, "a`b"), "`a``b`");
        assert_eq!(quote_ident(Family::Postgres, "a\"b"), "\"a\"\"b\"");
        assert_eq!(quote_ident(Family::Oracle, "NAME"), "\"NAME\"");
    }

    #[test]
    fn ident_quotes_only_when_needed() {
        assert_eq!(ident(Family::MySql, "user_name"), "user_name");
        assert_eq!(ident(Family::MySql, "UserName"), "UserName");
        assert_eq!(ident(Family::MySql, "order"), "`order`");
        assert_eq!(ident(Family::MySql, "数洞"), "`数洞`");
        assert_eq!(ident(Family::MySql, "a b"), "`a b`");
        assert_eq!(ident(Family::MySql, "1abc"), "`1abc`");
        assert_eq!(ident(Family::Postgres, "name"), "name");
        assert_eq!(ident(Family::Postgres, "Name"), "\"Name\"");
        assert_eq!(ident(Family::Postgres, "user"), "\"user\"");
        assert_eq!(ident(Family::Oracle, "NAME"), "NAME");
        assert_eq!(ident(Family::Oracle, "name"), "\"name\"");
        assert_eq!(ident(Family::Oracle, "LEVEL"), "\"LEVEL\"");
        assert_eq!(ident(Family::Dameng, "T_USER$1"), "T_USER$1");
        assert_eq!(ident(Family::Dameng, ""), "\"\"");
    }

    #[test]
    fn placeholders() {
        assert_eq!(placeholder(Family::MySql, 3), "?");
        assert_eq!(placeholder(Family::Postgres, 3), "$3");
        assert_eq!(placeholder(Family::Oracle, 3), ":3");
        assert_eq!(placeholder(Family::Dameng, 3), "?");
    }
}
