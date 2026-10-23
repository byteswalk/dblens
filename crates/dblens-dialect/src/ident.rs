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
    let first_ok = match family {
        Family::MySql | Family::Postgres => first.is_ascii_alphabetic() || first == '_',
        Family::Oracle | Family::Dameng => first.is_ascii_alphabetic(),
    };
    if !first_ok {
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

/// PostgreSQL、MySQL、Oracle、达梦保留字与特殊值函数名（如 CURRENT_USER、SYSDATE）的并集。多加引号不会出错，所以宁多勿少；新增方言时要补充。
const RESERVED: &[&str] = &[
    "ACCESS",
    "ACCESSIBLE",
    "ADD",
    "ALL",
    "ALTER",
    "ANALYSE",
    "ANALYZE",
    "AND",
    "ANY",
    "ARRAY",
    "AS",
    "ASC",
    "ASYMMETRIC",
    "AUDIT",
    "AUTHORIZATION",
    "BEFORE",
    "BETWEEN",
    "BIGINT",
    "BINARY",
    "BLOB",
    "BOTH",
    "BREAK",
    "BY",
    "CALL",
    "CASCADE",
    "CASE",
    "CAST",
    "CHANGE",
    "CHAR",
    "CHARACTER",
    "CHECK",
    "CLASS",
    "CLUSTER",
    "COLLATE",
    "COLLATION",
    "COLUMN",
    "COLUMN_VALUE",
    "COMMENT",
    "COMPRESS",
    "CONCURRENTLY",
    "CONDITION",
    "CONNECT",
    "CONSTRAINT",
    "CONTINUE",
    "CONVERT",
    "CREATE",
    "CROSS",
    "CURRENT",
    "CURRENT_CATALOG",
    "CURRENT_DATE",
    "CURRENT_ROLE",
    "CURRENT_SCHEMA",
    "CURRENT_TIME",
    "CURRENT_TIMESTAMP",
    "CURRENT_USER",
    "CURSOR",
    "DATABASE",
    "DATABASES",
    "DATE",
    "DAY_HOUR",
    "DAY_MINUTE",
    "DAY_SECOND",
    "DEC",
    "DECIMAL",
    "DECLARE",
    "DEFAULT",
    "DEFERRABLE",
    "DELAYED",
    "DELETE",
    "DESC",
    "DESCRIBE",
    "DETERMINISTIC",
    "DISTINCT",
    "DISTINCTROW",
    "DIV",
    "DO",
    "DOUBLE",
    "DROP",
    "DUAL",
    "EACH",
    "ELSE",
    "ELSEIF",
    "ENCLOSED",
    "END",
    "ESCAPED",
    "EXCEPT",
    "EXCLUSIVE",
    "EXISTS",
    "EXIT",
    "EXPLAIN",
    "FALSE",
    "FETCH",
    "FILE",
    "FLOAT",
    "FOR",
    "FORCE",
    "FOREIGN",
    "FREEZE",
    "FROM",
    "FULL",
    "FULLTEXT",
    "FUNCTION",
    "GENERATED",
    "GET",
    "GRANT",
    "GROUP",
    "GROUPS",
    "HAVING",
    "HIGH_PRIORITY",
    "HOUR_MINUTE",
    "HOUR_SECOND",
    "IDENTIFIED",
    "IDENTITY",
    "IF",
    "IGNORE",
    "ILIKE",
    "IMMEDIATE",
    "IN",
    "INCREMENT",
    "INDEX",
    "INFILE",
    "INITIAL",
    "INITIALLY",
    "INNER",
    "INOUT",
    "INSENSITIVE",
    "INSERT",
    "INT",
    "INTEGER",
    "INTERSECT",
    "INTERVAL",
    "INTO",
    "IS",
    "ISNULL",
    "ITERATE",
    "JOIN",
    "KEY",
    "KEYS",
    "KILL",
    "LATERAL",
    "LEADING",
    "LEAVE",
    "LEFT",
    "LEVEL",
    "LIKE",
    "LIMIT",
    "LINEAR",
    "LINES",
    "LOAD",
    "LOCALTIME",
    "LOCALTIMESTAMP",
    "LOCK",
    "LONG",
    "LONGBLOB",
    "LONGTEXT",
    "LOOP",
    "LOW_PRIORITY",
    "MATCH",
    "MAXEXTENTS",
    "MAXVALUE",
    "MEDIUMINT",
    "MINUS",
    "MLSLABEL",
    "MOD",
    "MODE",
    "MODIFIES",
    "MODIFY",
    "NATURAL",
    "NESTED_TABLE_ID",
    "NOAUDIT",
    "NOCOMPRESS",
    "NOT",
    "NOTNULL",
    "NOWAIT",
    "NULL",
    "NUMBER",
    "NUMERIC",
    "OF",
    "OFFLINE",
    "OFFSET",
    "ON",
    "ONLINE",
    "ONLY",
    "OPTIMIZE",
    "OPTION",
    "OPTIONALLY",
    "OR",
    "ORDER",
    "OUT",
    "OUTER",
    "OUTFILE",
    "OVER",
    "OVERLAPS",
    "PARTITION",
    "PCTFREE",
    "PLACING",
    "PRECISION",
    "PRIMARY",
    "PRIOR",
    "PRIVILEGES",
    "PROCEDURE",
    "PUBLIC",
    "PURGE",
    "RANGE",
    "RANK",
    "RAW",
    "READ",
    "READS",
    "REAL",
    "RECURSIVE",
    "REFERENCES",
    "REGEXP",
    "RELEASE",
    "RENAME",
    "REPEAT",
    "REPLACE",
    "REQUIRE",
    "RESIGNAL",
    "RESOURCE",
    "RESTRICT",
    "RETURN",
    "RETURNING",
    "REVOKE",
    "RIGHT",
    "RLIKE",
    "ROW",
    "ROWID",
    "ROWNUM",
    "ROWS",
    "SCHEMA",
    "SCHEMAS",
    "SELECT",
    "SENSITIVE",
    "SEPARATOR",
    "SESSION",
    "SESSION_USER",
    "SET",
    "SHARE",
    "SHOW",
    "SIGNAL",
    "SIMILAR",
    "SIZE",
    "SMALLINT",
    "SOME",
    "SPATIAL",
    "SPECIFIC",
    "SQL",
    "SQLEXCEPTION",
    "SQLSTATE",
    "SQLWARNING",
    "SSL",
    "START",
    "STARTING",
    "STORED",
    "STRAIGHT_JOIN",
    "SUCCESSFUL",
    "SYMMETRIC",
    "SYNONYM",
    "SYSDATE",
    "SYSTEM_USER",
    "TABLE",
    "TABLESAMPLE",
    "TERMINATED",
    "THEN",
    "TINYINT",
    "TO",
    "TOP",
    "TRAILING",
    "TRIGGER",
    "TRUE",
    "UID",
    "UNDO",
    "UNION",
    "UNIQUE",
    "UNLOCK",
    "UNSIGNED",
    "UPDATE",
    "USAGE",
    "USER",
    "USING",
    "UTC_DATE",
    "UTC_TIME",
    "UTC_TIMESTAMP",
    "VALIDATE",
    "VALUES",
    "VARBINARY",
    "VARCHAR",
    "VARCHAR2",
    "VARIADIC",
    "VARYING",
    "VERBOSE",
    "VIEW",
    "VIRTUAL",
    "WHEN",
    "WHERE",
    "WHILE",
    "WINDOW",
    "WITH",
    "WRITE",
    "XOR",
    "YEAR_MONTH",
    "ZEROFILL",
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

    #[test]
    fn adversarial_names_are_always_quoted() {
        assert_eq!(ident(Family::MySql, "a`b"), "`a``b`");
        assert_eq!(ident(Family::Postgres, "x; DROP TABLE t"), "\"x; DROP TABLE t\"");
        assert_eq!(ident(Family::Oracle, "A\"B"), "\"A\"\"B\"");
        assert_eq!(ident(Family::Postgres, "a$b"), "\"a$b\"");
        assert_eq!(ident(Family::Oracle, "_X"), "\"_X\"");
        assert_eq!(ident(Family::Dameng, "_X"), "\"_X\"");
        assert_eq!(ident(Family::MySql, "_x"), "_x");
        assert_eq!(ident(Family::Postgres, "_x"), "_x");
    }

    #[test]
    fn reserved_words_and_special_functions() {
        assert_eq!(ident(Family::Postgres, "current_user"), "\"current_user\"");
        assert_eq!(ident(Family::MySql, "schema"), "`schema`");
        assert_eq!(ident(Family::Oracle, "SYSDATE"), "\"SYSDATE\"");
        assert_eq!(ident(Family::Oracle, "COMMENT"), "\"COMMENT\"");
        assert_eq!(ident(Family::Oracle, "UID"), "\"UID\"");
        assert_eq!(ident(Family::Dameng, "TOP"), "\"TOP\"");
        assert!(RESERVED.windows(2).all(|w| w[0] < w[1]));
    }

    #[test]
    fn ident_output_is_safe() {
        let simple = |s: &str| {
            let mut cs = s.chars();
            matches!(cs.next(), Some(c) if c.is_ascii_alphabetic() || c == '_')
                && cs.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
        };
        for f in [Family::MySql, Family::Postgres, Family::Oracle, Family::Dameng] {
            for name in
                ["a", "A", "a b", "a\"b", "a`b", "x;--", "数洞", "", "_", "1a", "$x", "ORDER", "order"]
            {
                let out = ident(f, name);
                assert!(
                    (out == name && simple(name)) || out == quote_ident(f, name),
                    "{f:?} {name:?} -> {out:?}"
                );
            }
        }
    }
}
