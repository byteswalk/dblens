//! 执行策略（设计文档 7.7 节）：所有执行路径都要先问 `Policy`。个人版的 `DefaultPolicy` 只在生产环境的连接上，
//! 对不带 WHERE 的 UPDATE/DELETE、DROP、TRUNCATE 要求确认（SQL Server 另加 KILL、SHUTDOWN、DBCC、RESTORE、
//! ALTER DATABASE）；团队版以后用同一接口实现授权、审计等。
//!
//! SQL Server 按 `GO` 分批执行，一批里可以有多条语句：分析时取批中最危险的一条（见 `classify_batch`）。

use async_trait::async_trait;
use dblens_core::{Msg, msg};
use dblens_dialect::{Dialect, Family};
use serde::{Deserialize, Serialize};
use sqlparser::ast::{ObjectName, Statement};
use sqlparser::dialect::{GenericDialect, MsSqlDialect, MySqlDialect, PostgreSqlDialect};
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
        reason: Msg,
    },
    Deny {
        reason: Msg,
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
                msg!("policy.prod_no_where", keyword = stmt.keyword)
            }
            "DROP" | "TRUNCATE" => msg!("policy.prod_statement", keyword = stmt.keyword),
            "KILL" | "SHUTDOWN" | "DBCC" | "RESTORE" | "ALTER DATABASE"
                if ctx.dialect.family() == Family::SqlServer =>
            {
                msg!("policy.prod_statement", keyword = stmt.keyword)
            }
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
/// 和解析失败时只看首个关键字，此时无法判断是否带 WHERE。SQL Server 的一批见 `classify_batch`。
pub fn classify(dialect: &Dialect, sql: &str) -> StatementInfo {
    if dialect.family() == Family::SqlServer {
        return classify_batch(sql);
    }
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
        Family::Oracle | Family::Dameng | Family::SqlServer | Family::Informix => None,
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

/// SQL Server 中执行前要确认的关键字（另有 `ALTER DATABASE`）。
const SQLSERVER_DANGEROUS: [&str; 4] = ["KILL", "SHUTDOWN", "DBCC", "RESTORE"];

/// SQL Server 的一批：用 sqlparser 的 MsSQL 方言拆成语句，取第一条危险的语句（不带 WHERE 的 UPDATE/DELETE、DROP、
/// TRUNCATE、`SQLSERVER_DANGEROUS`、ALTER DATABASE）作为结果；没有危险语句时，有 DDL 则按 DDL（元数据缓存要失效），
/// 否则按第一条。所有 DDL 涉及的对象都收集起来。
/// 解析不了（`IF`、`WHILE`、`BEGIN…END` 等过程式代码，或没有用分号分隔的多条语句）时，跳过字符串、方括号和注释扫描关键字：出现危险关键字时
/// 以它为关键字、`has_where` 为 None，宁可多问一次。
fn classify_batch(sql: &str) -> StatementInfo {
    let first = first_keyword(sql);
    let Ok(stmts) = Parser::parse_sql(&MsSqlDialect {}, sql) else {
        let words = batch_words(sql);
        let found = words.iter().enumerate().find_map(|(i, w)| match w.as_str() {
            "DROP" | "TRUNCATE" | "DELETE" | "UPDATE" => Some(w.clone()),
            "ALTER" if words.get(i + 1).is_some_and(|n| n == "DATABASE") => Some("ALTER DATABASE".into()),
            w if SQLSERVER_DANGEROUS.contains(&w) => Some(w.to_string()),
            _ => None,
        });
        let keyword = found.unwrap_or(first);
        let kind = kind_of_keyword(keyword.split(' ').next().unwrap_or_default());
        return StatementInfo { kind, keyword, has_where: None, objects: vec![] };
    };
    let infos: Vec<StatementInfo> = stmts
        .iter()
        .map(|s| {
            let text = s.to_string();
            let mut keyword = first_keyword(&text);
            if keyword == "ALTER" && text.to_uppercase().split_whitespace().nth(1) == Some("DATABASE") {
                keyword = "ALTER DATABASE".into();
            }
            let (has_where, objects) = from_ast(s);
            StatementInfo {
                kind: kind_of_keyword(keyword.split(' ').next().unwrap_or_default()),
                keyword,
                has_where,
                objects,
            }
        })
        .collect();
    let objects: Vec<ObjectRef> = infos.iter().flat_map(|i| i.objects.clone()).collect();
    let dangerous = |i: &&StatementInfo| match i.keyword.as_str() {
        "UPDATE" | "DELETE" => i.has_where != Some(true),
        "DROP" | "TRUNCATE" | "ALTER DATABASE" => true,
        k => SQLSERVER_DANGEROUS.contains(&k),
    };
    let pick = infos
        .iter()
        .find(dangerous)
        .or_else(|| infos.iter().find(|i| i.kind == StatementKind::Ddl))
        .or(infos.first());
    match pick {
        Some(i) => StatementInfo { objects, ..i.clone() },
        None => StatementInfo { kind: kind_of_keyword(&first), keyword: first, has_where: None, objects },
    }
}

/// T-SQL 文本中字符串、方括号和双引号标识符、注释以外的单词（大写）。
fn batch_words(sql: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut chars = sql.chars().peekable();
    let mut word = String::new();
    let flush = |word: &mut String, words: &mut Vec<String>| {
        if !word.is_empty() {
            words.push(std::mem::take(word).to_uppercase());
        }
    };
    while let Some(c) = chars.next() {
        match c {
            '\'' | '"' | '[' => {
                flush(&mut word, &mut words);
                let close = if c == '[' { ']' } else { c };
                while let Some(d) = chars.next() {
                    if d == close {
                        // 两个结束符表示一个字符
                        if chars.peek() == Some(&close) {
                            chars.next();
                        } else {
                            break;
                        }
                    }
                }
            }
            '-' if chars.peek() == Some(&'-') => {
                flush(&mut word, &mut words);
                for d in chars.by_ref() {
                    if d == '\n' {
                        break;
                    }
                }
            }
            '/' if chars.peek() == Some(&'*') => {
                flush(&mut word, &mut words);
                chars.next();
                let mut depth = 1;
                while depth > 0 {
                    match chars.next() {
                        Some('*') if chars.peek() == Some(&'/') => {
                            chars.next();
                            depth -= 1;
                        }
                        Some('/') if chars.peek() == Some(&'*') => {
                            chars.next();
                            depth += 1;
                        }
                        Some(_) => {}
                        None => break,
                    }
                }
            }
            c if c.is_alphanumeric() || c == '_' || c == '@' || c == '#' => word.push(c),
            _ => flush(&mut word, &mut words),
        }
    }
    flush(&mut word, &mut words);
    words
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

    #[test]
    fn sqlserver_batches_pick_the_most_dangerous_statement() {
        let ms = Dialect::new(Variant::SqlServer);
        let s = classify(&ms, "SELECT 1; DELETE FROM dbo.t WHERE id = 1; UPDATE t SET a = 1");
        assert_eq!((s.keyword.as_str(), s.has_where), ("UPDATE", Some(false)));
        let s = classify(&ms, "SELECT 1; DELETE FROM t WHERE id = 1");
        assert_eq!((s.kind, s.keyword.as_str()), (StatementKind::Dql, "SELECT"));
        // 不写分号的多条语句 sqlparser 解析不了，按关键字扫描，无法判断 WHERE（生产环境上会多问一次）
        let s = classify(&ms, "SELECT 1\nDELETE FROM t WHERE id = 1");
        assert_eq!((s.kind, s.keyword.as_str(), s.has_where), (StatementKind::Dml, "DELETE", None));
        let s = classify(&ms, "SELECT 1; CREATE TABLE dbo.t2 (id int); DROP TABLE [dbo].[t3]");
        assert_eq!(s.keyword, "DROP");
        assert_eq!(s.objects.iter().map(|o| o.name.as_str()).collect::<Vec<_>>(), ["t2", "t3"]);
        let s = classify(&ms, "SELECT 1; CREATE TABLE t2 (id int)");
        assert_eq!(s.kind, StatementKind::Ddl);
        assert_eq!(classify(&ms, "ALTER DATABASE x SET OFFLINE").keyword, "ALTER DATABASE");
        // 解析不了的过程式代码：按关键字扫描，字符串和注释里的不算
        let s = classify(&ms, "IF 1 = 1 BEGIN PRINT 'DROP TABLE t' /* DELETE */ -- TRUNCATE\nEND");
        assert_eq!(s.keyword, "IF");
        let s = classify(&ms, "IF OBJECT_ID('t') IS NOT NULL BEGIN DBCC CHECKDB WITH NO_INFOMSGS END");
        assert_eq!((s.keyword.as_str(), s.has_where), ("DBCC", None));
        let s = classify(&ms, "WHILE 1 = 0 BEGIN DELETE FROM [t]] x] WHERE 1 = 1 END");
        assert_eq!((s.keyword.as_str(), s.has_where), ("DELETE", None));
        assert_eq!(batch_words("SELECT [a]]b], N'c''d', \"e\" /* x /* y */ z */ f"), ["SELECT", "N", "F"]);
    }

    #[tokio::test]
    async fn sqlserver_admin_statements_need_confirmation_on_production() {
        let ms = Dialect::new(Variant::SqlServer);
        let c = ExecContext { profile_id: "p".into(), environment: Environment::Prod, dialect: ms };
        for sql in ["KILL 53", "SHUTDOWN", "DBCC SHRINKDATABASE (x)", "RESTORE DATABASE x FROM DISK = 'a'"] {
            let d = DefaultPolicy.before_execute(&c, &classify(&ms, sql)).await;
            assert!(matches!(d, Decision::Confirm { .. }), "{sql}");
        }
        // 其他库的 KILL 不在此列
        let my = Dialect::new(Variant::MySql);
        let c = ExecContext { profile_id: "p".into(), environment: Environment::Prod, dialect: my };
        assert_eq!(DefaultPolicy.before_execute(&c, &classify(&my, "KILL 53")).await, Decision::Allow);
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
