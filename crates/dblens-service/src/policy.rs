//! 执行策略（设计文档 7.7 节）：所有执行路径都要先问 `Policy`。个人版的 `DefaultPolicy` 只在生产环境的连接上，
//! 对不带 WHERE 的 UPDATE/DELETE、DROP、TRUNCATE 要求确认；团队版以后用同一接口实现授权、审计等。

use async_trait::async_trait;
use dblens_dialect::{Dialect, Family};
use serde::{Deserialize, Serialize};
use sqlparser::ast::{ObjectName, Statement};
use sqlparser::dialect::{GenericDialect, MySqlDialect, PostgreSqlDialect};
use sqlparser::parser::Parser;

use crate::profile::Environment;

/// 语句的大类。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StatementKind {
    /// SELECT、WITH、SHOW、EXPLAIN 等只读语句。
    Dql,
    /// INSERT、UPDATE、DELETE、MERGE。
    Dml,
    /// CREATE、ALTER、DROP、TRUNCATE、RENAME、COMMENT。
    Ddl,
    /// GRANT、REVOKE。
    Dcl,
    /// COMMIT、ROLLBACK、BEGIN、SAVEPOINT、SET TRANSACTION。
    Tcl,
    /// PL/SQL 匿名块、CALL、EXEC。
    Block,
    Other,
}

/// 语句涉及的对象（目前只对 DDL 提取，供元数据缓存失效使用）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObjectRef {
    pub schema: Option<String>,
    pub name: String,
}

/// 一条语句的分析结果。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatementInfo {
    pub kind: StatementKind,
    /// 首个关键字，大写，如 `DELETE`、`DROP`。
    pub keyword: String,
    /// UPDATE/DELETE 是否带 WHERE；其他语句、或解析失败无法判断时为 None。
    pub has_where: Option<bool>,
    pub objects: Vec<ObjectRef>,
}

/// 执行上下文：执行发生在哪个连接上。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecContext {
    pub profile_id: String,
    pub environment: Environment,
    pub dialect: Dialect,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "decision", rename_all = "snake_case")]
pub enum Decision {
    Allow,
    /// 需要用户确认后才能执行。
    Confirm {
        reason: String,
    },
    Deny {
        reason: String,
    },
}

/// 执行结果的摘要，供审计类策略使用。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Succeeded { affected: Option<u64> },
    Failed { message: String },
    Cancelled,
}

#[async_trait]
pub trait Policy: Send + Sync {
    async fn before_execute(&self, ctx: &ExecContext, stmt: &StatementInfo) -> Decision;
    async fn after_execute(&self, ctx: &ExecContext, stmt: &StatementInfo, outcome: &Outcome);
}

/// 个人版的默认策略。
pub struct DefaultPolicy;

#[async_trait]
impl Policy for DefaultPolicy {
    async fn before_execute(&self, ctx: &ExecContext, stmt: &StatementInfo) -> Decision {
        if ctx.environment != Environment::Prod {
            return Decision::Allow;
        }
        let reason = match stmt.keyword.as_str() {
            // 无法判断是否带 WHERE 时也要确认，宁可多问
            "UPDATE" | "DELETE" if stmt.has_where != Some(true) => {
                format!("生产环境：{} 语句没有 WHERE 条件", stmt.keyword)
            }
            "DROP" => "生产环境：DROP 语句".to_string(),
            "TRUNCATE" => "生产环境：TRUNCATE 语句".to_string(),
            _ => return Decision::Allow,
        };
        Decision::Confirm { reason }
    }

    async fn after_execute(&self, _: &ExecContext, _: &StatementInfo, _: &Outcome) {}
}

/// 跳过开头的空白和注释后的第一个单词（大写）。
fn first_keyword(sql: &str) -> String {
    let mut rest = sql;
    loop {
        rest = rest.trim_start();
        if let Some(r) = rest.strip_prefix("--").or_else(|| rest.strip_prefix('#')) {
            rest = r.split_once('\n').map_or("", |(_, after)| after);
        } else if let Some(r) = rest.strip_prefix("/*") {
            rest = r.split_once("*/").map_or("", |(_, after)| after);
        } else {
            break;
        }
    }
    rest.chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect::<String>().to_uppercase()
}

fn kind_of_keyword(k: &str) -> StatementKind {
    match k {
        "SELECT" | "WITH" | "SHOW" | "EXPLAIN" | "DESC" | "DESCRIBE" | "VALUES" | "TABLE" => {
            StatementKind::Dql
        }
        "INSERT" | "UPDATE" | "DELETE" | "MERGE" | "REPLACE" | "UPSERT" => StatementKind::Dml,
        "CREATE" | "ALTER" | "DROP" | "TRUNCATE" | "RENAME" | "COMMENT" => StatementKind::Ddl,
        "GRANT" | "REVOKE" => StatementKind::Dcl,
        "COMMIT" | "ROLLBACK" | "BEGIN" | "START" | "SAVEPOINT" | "RELEASE" => StatementKind::Tcl,
        "DECLARE" | "CALL" | "EXEC" | "EXECUTE" => StatementKind::Block,
        _ => StatementKind::Other,
    }
}

fn object(name: &ObjectName) -> ObjectRef {
    let parts: Vec<String> =
        name.0.iter().map(|p| p.as_ident().map_or_else(|| p.to_string(), |i| i.value.clone())).collect();
    match parts.as_slice() {
        [.., schema, last] => ObjectRef { schema: Some(schema.clone()), name: last.clone() },
        [last] => ObjectRef { schema: None, name: last.clone() },
        [] => ObjectRef { schema: None, name: String::new() },
    }
}

fn from_ast(stmt: &Statement) -> (Option<bool>, Vec<ObjectRef>) {
    match stmt {
        Statement::Update(u) => (Some(u.selection.is_some()), vec![]),
        Statement::Delete(d) => (Some(d.selection.is_some()), vec![]),
        Statement::CreateTable(t) => (None, vec![object(&t.name)]),
        Statement::CreateView(v) => (None, vec![object(&v.name)]),
        Statement::CreateIndex(i) => (None, vec![object(&i.table_name)]),
        Statement::AlterTable(a) => (None, vec![object(&a.name)]),
        Statement::Drop { names, .. } => (None, names.iter().map(object).collect()),
        Statement::Truncate(t) => (None, t.table_names.iter().map(|n| object(&n.name)).collect()),
        _ => (None, vec![]),
    }
}

/// 分析一条语句。MySQL、PG 家族用 sqlparser 解析；Oracle/达梦（PL/SQL 语法 sqlparser 支持不全）
/// 和解析失败时只看首个关键字，此时无法判断是否带 WHERE。
pub fn classify(dialect: &Dialect, sql: &str) -> StatementInfo {
    let keyword = first_keyword(sql);
    // PL/SQL 匿名块
    let kind = if keyword == "BEGIN" && matches!(dialect.family(), Family::Oracle | Family::Dameng) {
        StatementKind::Block
    } else {
        kind_of_keyword(&keyword)
    };
    let parsed = match dialect.family() {
        Family::MySql => Parser::parse_sql(&MySqlDialect {}, sql).ok(),
        Family::Postgres => Parser::parse_sql(&PostgreSqlDialect {}, sql).ok(),
        Family::Oracle | Family::Dameng => None,
    };
    let (has_where, objects) = match parsed.as_deref() {
        Some([stmt]) => from_ast(stmt),
        _ => {
            // 兜底：通用方言再试一次，仍失败就只保留关键字
            match Parser::parse_sql(&GenericDialect {}, sql).ok().as_deref() {
                Some([stmt]) if matches!(dialect.family(), Family::Oracle | Family::Dameng) => {
                    let (_, objects) = from_ast(stmt);
                    (None, objects)
                }
                _ => (None, vec![]),
            }
        }
    };
    StatementInfo { kind, keyword, has_where, objects }
}

#[cfg(test)]
mod tests {
    use dblens_dialect::Variant;

    use super::*;

    fn pg() -> Dialect {
        Dialect::new(Variant::Postgres)
    }

    #[test]
    fn classifies_with_parser() {
        let s = classify(&pg(), "/* x */ -- y\n  delete from hr.emp where id = 1");
        assert_eq!((s.kind, s.keyword.as_str(), s.has_where), (StatementKind::Dml, "DELETE", Some(true)));
        let s = classify(&pg(), "UPDATE emp SET a = 1");
        assert_eq!(s.has_where, Some(false));
        let s = classify(&Dialect::new(Variant::MySql), "DROP TABLE `db`.`t1`, t2");
        assert_eq!(s.kind, StatementKind::Ddl);
        assert_eq!(
            s.objects,
            vec![
                ObjectRef { schema: Some("db".into()), name: "t1".into() },
                ObjectRef { schema: None, name: "t2".into() }
            ]
        );
        assert_eq!(classify(&pg(), "CREATE TABLE s.\"T\" (id int)").objects[0].name, "T");
        assert_eq!(classify(&pg(), "WITH a AS (SELECT 1) SELECT * FROM a").kind, StatementKind::Dql);
        assert_eq!(classify(&pg(), "grant select on t to u").kind, StatementKind::Dcl);
        assert_eq!(classify(&pg(), "COMMIT").kind, StatementKind::Tcl);
    }

    #[test]
    fn falls_back_to_keyword() {
        let ora = Dialect::new(Variant::Oracle);
        let s = classify(&ora, "BEGIN DBMS_OUTPUT.PUT_LINE('x'); END;");
        assert_eq!(s.kind, StatementKind::Block);
        let s = classify(&ora, "DELETE FROM emp WHERE id = 1");
        assert_eq!((s.keyword.as_str(), s.has_where), ("DELETE", None), "Oracle 不判断 WHERE");
        assert_eq!(
            classify(&ora, "DROP TABLE hr.emp").objects,
            vec![ObjectRef { schema: Some("hr".into()), name: "emp".into() }]
        );
        let s = classify(&pg(), "DELETE FROM emp WHERE");
        assert_eq!((s.kind, s.has_where), (StatementKind::Dml, None), "解析失败");
        assert_eq!(classify(&pg(), "").kind, StatementKind::Other);
        assert_eq!(classify(&pg(), "# c\nSELECT 1").keyword, "SELECT");
    }

    fn ctx(environment: Environment) -> ExecContext {
        ExecContext { profile_id: "p".into(), environment, dialect: pg() }
    }

    #[tokio::test]
    async fn default_policy_confirms_dangerous_statements_only_on_production() {
        let p = DefaultPolicy;
        let cases = [
            ("DELETE FROM t", true),
            ("UPDATE t SET a = 1", true),
            ("DELETE FROM t WHERE id = 1", false),
            ("DROP TABLE t", true),
            ("TRUNCATE t", true),
            ("SELECT 1", false),
            ("INSERT INTO t VALUES (1)", false),
        ];
        for (sql, confirm) in cases {
            let stmt = classify(&pg(), sql);
            let d = p.before_execute(&ctx(Environment::Prod), &stmt).await;
            assert_eq!(matches!(d, Decision::Confirm { .. }), confirm, "{sql}: {d:?}");
            assert_eq!(p.before_execute(&ctx(Environment::Dev), &stmt).await, Decision::Allow, "{sql}");
        }
        // 无法判断是否带 WHERE 时也确认
        let unknown = classify(&Dialect::new(Variant::Oracle), "DELETE FROM t WHERE id = 1");
        let d = p
            .before_execute(
                &ExecContext { dialect: Dialect::new(Variant::Oracle), ..ctx(Environment::Prod) },
                &unknown,
            )
            .await;
        assert!(matches!(d, Decision::Confirm { .. }));
    }
}
