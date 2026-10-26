//! 会话级 SQL：执行计划、读取和切换当前 schema。

use crate::{Family, ident};

/// 生成一条语句的执行计划要依次执行的语句。最后一条的结果集就是计划：
/// - MySQL 系、PG 系：`EXPLAIN <语句>`；
/// - 达梦：`EXPLAIN FOR <语句>`，返回计划表的各行（`EXPLAIN <语句>` 经 DPI 执行不返回结果集）；
/// - Oracle：`EXPLAIN PLAN FOR <语句>` 写入计划表，再用 `DBMS_XPLAN.DISPLAY` 读出文本。
///
/// 都不会真正执行语句（不用 `EXPLAIN ANALYZE`）。
pub fn explain(family: Family, sql: &str) -> Vec<String> {
    match family {
        Family::Oracle => vec![
            format!("EXPLAIN PLAN FOR {sql}"),
            "SELECT PLAN_TABLE_OUTPUT FROM TABLE(DBMS_XPLAN.DISPLAY())".to_string(),
        ],
        Family::Dameng => vec![format!("EXPLAIN FOR {sql}")],
        Family::MySql | Family::Postgres => vec![format!("EXPLAIN {sql}")],
    }
}

/// 查询当前 schema 的语句，结果是一行一列；MySQL 没有选库时为 NULL。
pub fn current_schema(family: Family) -> &'static str {
    match family {
        Family::MySql => "SELECT DATABASE()",
        Family::Postgres => "SELECT current_schema()",
        Family::Oracle | Family::Dameng => "SELECT SYS_CONTEXT('USERENV', 'CURRENT_SCHEMA') FROM DUAL",
    }
}

/// 切换当前 schema 的语句。名字总是加引号，按原样使用（不转大小写）。
pub fn set_schema(family: Family, schema: &str) -> String {
    let name = ident::quote_ident(family, schema);
    match family {
        Family::MySql => format!("USE {name}"),
        Family::Postgres => format!("SET search_path TO {name}"),
        Family::Oracle | Family::Dameng => format!("ALTER SESSION SET CURRENT_SCHEMA = {name}"),
    }
}

/// 执行后可能改变当前 schema 的语句（按首个关键字判断），执行成功后要重新读取。
pub fn may_change_schema(keyword: &str) -> bool {
    matches!(keyword, "USE" | "SET" | "ALTER")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explain_statements() {
        assert_eq!(explain(Family::MySql, "SELECT 1"), ["EXPLAIN SELECT 1"]);
        assert_eq!(explain(Family::Postgres, "SELECT 1"), ["EXPLAIN SELECT 1"]);
        assert_eq!(explain(Family::Dameng, "SELECT 1"), ["EXPLAIN FOR SELECT 1"]);
        let o = explain(Family::Oracle, "SELECT 1 FROM DUAL");
        assert_eq!(o[0], "EXPLAIN PLAN FOR SELECT 1 FROM DUAL");
        assert!(o[1].contains("DBMS_XPLAN.DISPLAY"));
    }

    #[test]
    fn schema_statements_quote_names() {
        assert_eq!(set_schema(Family::MySql, "shop"), "USE `shop`");
        assert_eq!(set_schema(Family::MySql, "a`b"), "USE `a``b`");
        assert_eq!(set_schema(Family::Postgres, "Sales"), "SET search_path TO \"Sales\"");
        assert_eq!(set_schema(Family::Oracle, "HR"), "ALTER SESSION SET CURRENT_SCHEMA = \"HR\"");
        assert_eq!(set_schema(Family::Dameng, "x\"y"), "ALTER SESSION SET CURRENT_SCHEMA = \"x\"\"y\"");
        assert!(current_schema(Family::Oracle).contains("CURRENT_SCHEMA"));
        assert!(may_change_schema("USE") && may_change_schema("ALTER") && !may_change_schema("SELECT"));
    }
}
