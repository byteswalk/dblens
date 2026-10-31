//! 会话级 SQL：执行计划、读取和切换当前 schema。
//! SQL Server 不能在会话中切换默认架构，会话的"当前 schema"是当前数据库（`USE`），与 MySQL 的库相同。

use crate::{Family, ident};

/// 生成一条语句的执行计划要依次执行的语句。最后一条的结果集就是计划：
/// - MySQL 系、PG 系：`EXPLAIN <语句>`；
/// - 达梦：`EXPLAIN FOR <语句>`，返回计划表的各行（`EXPLAIN <语句>` 经 DPI 执行不返回结果集）；
/// - Oracle：`EXPLAIN PLAN FOR <语句>` 写入计划表，再用 `DBMS_XPLAN.DISPLAY` 读出文本；
/// - SQL Server：`SET SHOWPLAN_ALL ON`（必须单独成批）后执行语句得到估算计划的表格；之后必须执行
///   `explain_cleanup` 的语句恢复，否则会话之后的语句都只返回计划。
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
        Family::SqlServer => vec!["SET SHOWPLAN_ALL ON".to_string(), sql.to_string()],
        // SET EXPLAIN 写到服务器文件，读不回来；能力声明为不支持，界面不会发起
        Family::Informix => vec![],
    }
}

/// 生成执行计划之后无论成败都要执行的语句。
pub fn explain_cleanup(family: Family) -> Option<&'static str> {
    (family == Family::SqlServer).then_some("SET SHOWPLAN_ALL OFF")
}

/// 查询当前 schema 的语句，结果是一行一列；MySQL 没有选库时为 NULL。
pub fn current_schema(family: Family) -> &'static str {
    match family {
        Family::MySql => "SELECT DATABASE()",
        Family::Postgres => "SELECT current_schema()",
        Family::Oracle | Family::Dameng => "SELECT SYS_CONTEXT('USERENV', 'CURRENT_SCHEMA') FROM DUAL",
        Family::SqlServer => "SELECT DB_NAME()",
        Family::Informix => "SELECT TRIM(DBINFO('dbname')) FROM sysmaster:sysdual",
    }
}

/// 切换当前 schema 的语句。名字总是加引号，按原样使用（不转大小写）。
pub fn set_schema(family: Family, schema: &str) -> String {
    let name = ident::quote_ident(family, schema);
    match family {
        Family::MySql | Family::SqlServer => format!("USE {name}"),
        Family::Postgres => format!("SET search_path TO {name}"),
        Family::Oracle | Family::Dameng => format!("ALTER SESSION SET CURRENT_SCHEMA = {name}"),
        // 和 SQL Server 一样，会话的"当前 schema"是当前数据库
        Family::Informix => format!("DATABASE {name}"),
    }
}

/// 执行后可能改变当前 schema 的语句（按首个关键字判断），执行成功后要重新读取。
pub fn may_change_schema(keyword: &str) -> bool {
    matches!(keyword, "USE" | "SET" | "ALTER" | "DATABASE")
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
        assert_eq!(set_schema(Family::SqlServer, "a]b"), "USE [a]]b]");
        assert_eq!(current_schema(Family::SqlServer), "SELECT DB_NAME()");
        assert_eq!(explain(Family::SqlServer, "SELECT 1"), ["SET SHOWPLAN_ALL ON", "SELECT 1"]);
        assert_eq!(explain_cleanup(Family::SqlServer), Some("SET SHOWPLAN_ALL OFF"));
        assert_eq!(explain_cleanup(Family::Postgres), None);
        assert!(may_change_schema("USE") && may_change_schema("ALTER") && !may_change_schema("SELECT"));
    }
}
