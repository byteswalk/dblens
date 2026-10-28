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
    !case_ok || is_reserved(family, &name.to_ascii_uppercase())
}

fn is_reserved(family: Family, upper: &str) -> bool {
    // PUBLIC 只在 Oracle/达梦中是保留字；PG 的 public 是常见的默认模式名，MySQL 中也不是保留字
    if upper == "PUBLIC" && matches!(family, Family::Postgres | Family::MySql) {
        return false;
    }
    RESERVED.binary_search(&upper).is_ok()
        || (family == Family::Dameng && DAMENG_RESERVED.binary_search(&upper).is_ok())
}

/// PostgreSQL、MySQL（8.0/8.4）、Oracle、达梦以及国产库第一批（TiDB 等）的保留字与特殊值函数名（如 CURRENT_USER、SYSDATE）的并集，按字节序排列。
/// 多加引号不会出错，所以宁多勿少；个别词只在部分方言中保留（如 PUBLIC），由 `is_reserved` 按家族豁免。新增方言时要补充。
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
    "ASENSITIVE",
    "ASYMMETRIC",
    "AUDIT",
    "AUTHID",
    "AUTHORIZATION",
    "BEFORE",
    "BETWEEN",
    "BIGINT",
    "BINARY",
    "BLOB",
    "BOTH",
    "BREAK",
    "BUCKETS",
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
    "COMPACT",
    "COMPRESS",
    "CONCURRENTLY",
    "CONDITION",
    "CONNECT",
    "CONNECT_BY_ROOT",
    "CONSTRAINT",
    "CONTINUE",
    "CONVERT",
    "CREATE",
    "CROSS",
    "CSN",
    "CUBE",
    "CUME_DIST",
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
    "DAY_MICROSECOND",
    "DAY_MINUTE",
    "DAY_SECOND",
    "DEC",
    "DECIMAL",
    "DECLARE",
    "DEFAULT",
    "DEFERRABLE",
    "DELAYED",
    "DELETE",
    "DELTAMERGE",
    "DENSE_RANK",
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
    "EMPTY",
    "ENCKEY",
    "ENCLOSED",
    "END",
    "ESCAPED",
    "EXCEPT",
    "EXCLUDED",
    "EXCLUSIVE",
    "EXISTS",
    "EXIT",
    "EXPLAIN",
    "EXTERNAL",
    "FALSE",
    "FETCH",
    "FILE",
    "FIRST_VALUE",
    "FLOAT",
    "FLOAT4",
    "FLOAT8",
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
    "GROUPING",
    "GROUPPARENT",
    "GROUPS",
    "HAVING",
    "HDFSDIRECTORY",
    "HIGH_PRIORITY",
    "HOUR_MICROSECOND",
    "HOUR_MINUTE",
    "HOUR_SECOND",
    "IDENTIFIED",
    "IDENTITY",
    "IF",
    "IGNORE",
    "ILIKE",
    "IMCSTORED",
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
    "INT1",
    "INT2",
    "INT3",
    "INT4",
    "INT8",
    "INTEGER",
    "INTERSECT",
    "INTERVAL",
    "INTO",
    "IO_AFTER_GTIDS",
    "IO_BEFORE_GTIDS",
    "IS",
    "ISNULL",
    "ITERATE",
    "JOIN",
    "JSON_TABLE",
    "KEY",
    "KEYS",
    "KILL",
    "LAG",
    "LAST_VALUE",
    "LATERAL",
    "LEAD",
    "LEADING",
    "LEAVE",
    "LEFT",
    "LESS",
    "LEVEL",
    "LIBRARY",
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
    "MANUAL",
    "MASTER_BIND",
    "MASTER_SSL_VERIFY_SERVER_CERT",
    "MATCH",
    "MAXEXTENTS",
    "MAXVALUE",
    "MEDIUMBLOB",
    "MEDIUMINT",
    "MEDIUMTEXT",
    "MIDDLEINT",
    "MINUS",
    "MINUTE_MICROSECOND",
    "MINUTE_SECOND",
    "MLSLABEL",
    "MOD",
    "MODE",
    "MODIFIES",
    "MODIFY",
    "NATURAL",
    "NESTED_TABLE_ID",
    "NOAUDIT",
    "NOCOMPRESS",
    "NOCYCLE",
    "NOT",
    "NOTNULL",
    "NOWAIT",
    "NO_WRITE_TO_BINLOG",
    "NTH_VALUE",
    "NTILE",
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
    "OPTIMIZER_COSTS",
    "OPTION",
    "OPTIONALLY",
    "OR",
    "ORDER",
    "OUT",
    "OUTER",
    "OUTFILE",
    "OVER",
    "OVERLAPS",
    "PARALLEL",
    "PARTITION",
    "PCTFREE",
    "PERCENT_RANK",
    "PERFORMANCE",
    "PLACING",
    "PRECISION",
    "PRIMARY",
    "PRIOR",
    "PRIVILEGES",
    "PROCEDURE",
    "PUBLIC",
    "PURGE",
    "QUALIFY",
    "RANGE",
    "RANK",
    "RAW",
    "READ",
    "READS",
    "READ_WRITE",
    "REAL",
    "RECURSIVE",
    "RECYCLEBIN",
    "REFERENCES",
    "REGEXP",
    "REJECT",
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
    "ROW_NUMBER",
    "SCHEMA",
    "SCHEMAS",
    "SECOND_MICROSECOND",
    "SELECT",
    "SELF",
    "SENSITIVE",
    "SEPARATOR",
    "SESSION",
    "SESSIONTIMEZONE",
    "SESSION_USER",
    "SET",
    "SHARE",
    "SHOW",
    "SHRINK",
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
    "SQL_BIG_RESULT",
    "SQL_CALC_FOUND_ROWS",
    "SQL_SMALL_RESULT",
    "SSL",
    "START",
    "STARTING",
    "STORED",
    "STRAIGHT_JOIN",
    "SUCCESSFUL",
    "SYMMETRIC",
    "SYNONYM",
    "SYSDATE",
    "SYSTEM",
    "SYSTEM_USER",
    "SYSTIMESTAMP",
    "TABLE",
    "TABLESAMPLE",
    "TERMINATED",
    "THEN",
    "TIDB_CURRENT_TSO",
    "TIMECAPSULE",
    "TINYBLOB",
    "TINYINT",
    "TINYTEXT",
    "TO",
    "TOP",
    "TRAILING",
    "TRIGGER",
    "TRUE",
    "UID",
    "UNDO",
    "UNIMCSTORED",
    "UNION",
    "UNIQUE",
    "UNLOCK",
    "UNSIGNED",
    "UNTIL",
    "UPDATE",
    "USAGE",
    "USE",
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
    "VARCHARACTER",
    "VARIADIC",
    "VARYING",
    "VERBOSE",
    "VERIFY",
    "VIEW",
    "VIRTUAL",
    "WHEN",
    "WHENEVER",
    "WHERE",
    "WHILE",
    "WINDOW",
    "WITH",
    "WRITE",
    "XOR",
    "YEAR_MONTH",
    "ZEROFILL",
];

/// 只在达梦中保留的词：取自 DM8 的 `V$RESERVED_WORDS WHERE RESERVED = 'Y'`，再加上不能直接当列名的伪列
/// （ROWID、TRXID、VERSIONS_*）。其中很多是其他数据库的常见列名（FIRST、NEXT、LIST、XML），所以单独列出，
/// 只对达梦生效。按字节序排列。
const DAMENG_RESERVED: &[&str] = &[
    "ABSOLUTE",
    "ABSTRACT",
    "ADMIN",
    "ARRAYLEN",
    "ASSIGN",
    "AUTO_INCREMENT",
    "BEGIN",
    "BIGDATEDIFF",
    "BSTRING",
    "BYTE",
    "CATCH",
    "CLUSTERBTR",
    "COMMIT",
    "COMMITWORK",
    "CONNECT_BY_ROOT",
    "CONST",
    "CONTAINS",
    "CONTEXT",
    "CORRESPONDING",
    "CRYPTO",
    "DATEADD",
    "DATEDIFF",
    "DATEPART",
    "DBTIMEZONE",
    "DECODE",
    "DIMENSION",
    "DISABLE",
    "DISKSPACE",
    "DISTRIBUTED",
    "DOMAIN",
    "ELSIF",
    "ENABLE",
    "EQU",
    "EXCHANGE",
    "EXEC",
    "EXECUTE",
    "EXTERN",
    "EXTRACT",
    "FINAL",
    "FINALLY",
    "FIRST",
    "FLASHBACK",
    "FULLY",
    "GOTO",
    "IFNULL",
    "INLINE",
    "JSON_ARRAYAGG",
    "JSON_TRANSFORM",
    "KEEP",
    "LARGE",
    "LESS",
    "LEXER",
    "LIST",
    "LNNVL",
    "LOGIN",
    "MEMBER",
    "MODEL",
    "MULTISET",
    "NEW",
    "NEXT",
    "NOCOPY",
    "NOCYCLE",
    "NOSORT",
    "OBJECT",
    "OVERLAY",
    "OVERRIDE",
    "PENDANT",
    "PERCENT",
    "PIPE",
    "PRINT",
    "PRIVATE",
    "PROTECTED",
    "RAISE",
    "RECORD",
    "REF",
    "REFERENCE",
    "REFERENCING",
    "REGEXP_SPLIT_TO_TABLE",
    "RELATIVE",
    "REPLICATE",
    "REVERSE",
    "ROLLBACK",
    "ROLLUP",
    "SAVEPOINT",
    "SBYTE",
    "SEALED",
    "SECTION",
    "SESSIONTIMEZONE",
    "SETS",
    "SHORT",
    "SIZEOF",
    "STATIC",
    "STRUCT",
    "SUBPARTITION",
    "SWITCH",
    "SYSTIMESTAMP",
    "THAN",
    "THROW",
    "TIMESTAMPADD",
    "TIMESTAMPDIFF",
    "TO_VECTOR",
    "TREAT",
    "TRIM",
    "TRUNCATE",
    "TRXID",
    "TRY",
    "TYPEDEF",
    "TYPEOF",
    "UINT",
    "ULONG",
    "UNTIL",
    "USHORT",
    "VARRAY",
    "VERIFY",
    "VERSIONS",
    "VERSIONS_ENDTIME",
    "VERSIONS_ENDTRXID",
    "VERSIONS_OPERATION",
    "VERSIONS_STARTTIME",
    "VERSIONS_STARTTRXID",
    "VOID",
    "WITHIN",
    "WITHOUT",
    "XML",
    "XMLAGG",
    "XMLATTRIBUTES",
    "XMLCAST",
    "XMLELEMENT",
    "XMLNAMESPACES",
    "XMLPARSE",
    "XMLQUERY",
    "XMLSERIALIZE",
    "XMLTABLE",
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
        assert!(DAMENG_RESERVED.windows(2).all(|w| w[0] < w[1]));
        // 达梦专有的保留字只对达梦加引号
        assert_eq!(ident(Family::Dameng, "FIRST"), "\"FIRST\"");
        assert_eq!(ident(Family::Oracle, "FIRST"), "FIRST");
        assert_eq!(ident(Family::MySql, "first"), "first");
    }

    #[test]
    fn mysql_8_reserved_words_and_oracle_extras() {
        for w in ["system", "lead", "lag", "use", "empty", "row_number", "int4", "sql_calc_found_rows"] {
            assert_eq!(ident(Family::MySql, w), format!("`{w}`"), "{w}");
        }
        assert_eq!(ident(Family::Oracle, "WHENEVER"), "\"WHENEVER\"");
        // 二分查找要求每个词都能被找到
        assert!(RESERVED.iter().all(|w| RESERVED.binary_search(w).is_ok()));
    }

    #[test]
    fn public_is_reserved_only_for_oracle_and_dameng() {
        assert_eq!(ident(Family::Postgres, "public"), "public");
        assert_eq!(ident(Family::MySql, "public"), "public");
        assert_eq!(ident(Family::Oracle, "PUBLIC"), "\"PUBLIC\"");
        assert_eq!(ident(Family::Dameng, "PUBLIC"), "\"PUBLIC\"");
        assert_eq!(ident(Family::Postgres, "PUBLIC"), "\"PUBLIC\"");
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
