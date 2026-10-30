//! 运维查询（M1c3 设计文档第 5 节）：每个家族一个 `sql/ops/<家族>.sql`，兼容库可以有只含差异 key 的覆盖文件。
//! 文件按 `-- [key]` 行分段，段首的注释行写名称和分类：
//!
//! ```sql
//! -- [sessions]
//! -- name.zh-CN: 会话列表
//! -- name.en: Sessions
//! -- category: session
//! SELECT …
//! ```
//!
//! 查询没有参数；会话、锁的查询带 `kill_sql` 列（结束会话的语句，只生成不执行）。

use std::sync::OnceLock;

use serde::Serialize;

use crate::{Dialect, Family, Variant};

const MYSQL: &str = include_str!("../sql/ops/mysql.sql");
const MARIADB: &str = include_str!("../sql/ops/mariadb.sql");
const POSTGRES: &str = include_str!("../sql/ops/postgres.sql");
const ORACLE: &str = include_str!("../sql/ops/oracle.sql");
const DAMENG: &str = include_str!("../sql/ops/dameng.sql");
const TIDB: &str = include_str!("../sql/ops/tidb.sql");
const OCEANBASE: &str = include_str!("../sql/ops/oceanbase.sql");
const OPENGAUSS: &str = include_str!("../sql/ops/opengauss.sql");
const SQLSERVER: &str = include_str!("../sql/ops/sqlserver.sql");

/// 运维查询的分类。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OpsCategory {
    Session,
    Lock,
    Storage,
    Sql,
}

impl OpsCategory {
    fn parse(s: &str) -> Option<Self> {
        match s {
            "session" => Some(Self::Session),
            "lock" => Some(Self::Lock),
            "storage" => Some(Self::Storage),
            "sql" => Some(Self::Sql),
            _ => None,
        }
    }
}

/// 一条运维查询。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpsQuery {
    pub key: String,
    pub name_zh: String,
    pub name_en: String,
    pub category: OpsCategory,
    pub sql: String,
}

/// 把模板文件拆成运维查询，保持文件中的顺序。段首注释缺名称或分类时 panic：模板随程序编译，由单元测试保证完整。
pub(crate) fn parse(text: &str) -> Vec<OpsQuery> {
    let mut out = Vec::new();
    let mut current: Option<OpsQuery> = None;
    let finish = |q: OpsQuery, out: &mut Vec<OpsQuery>| {
        assert!(!q.name_zh.is_empty() && !q.name_en.is_empty(), "运维查询 {} 缺名称", q.key);
        out.push(OpsQuery { sql: q.sql.trim().to_string(), ..q });
    };
    for line in text.lines() {
        let t = line.trim();
        if let Some(key) = t.strip_prefix("-- [").and_then(|r| r.strip_suffix(']')) {
            if let Some(q) = current.take() {
                finish(q, &mut out);
            }
            current = Some(OpsQuery {
                key: key.trim().to_string(),
                name_zh: String::new(),
                name_en: String::new(),
                category: OpsCategory::Session,
                sql: String::new(),
            });
            continue;
        }
        let Some(q) = current.as_mut() else { continue };
        if q.sql.is_empty() {
            if let Some(v) = t.strip_prefix("-- name.zh-CN:") {
                q.name_zh = v.trim().to_string();
                continue;
            }
            if let Some(v) = t.strip_prefix("-- name.en:") {
                q.name_en = v.trim().to_string();
                continue;
            }
            if let Some(v) = t.strip_prefix("-- category:") {
                q.category = OpsCategory::parse(v.trim())
                    .unwrap_or_else(|| panic!("运维查询 {} 的分类不认识：{v}", q.key));
                continue;
            }
            if t.is_empty() {
                continue;
            }
        }
        q.sql.push_str(line);
        q.sql.push('\n');
    }
    if let Some(q) = current {
        finish(q, &mut out);
    }
    out
}

fn base_file(family: Family) -> &'static str {
    match family {
        Family::MySql => MYSQL,
        Family::Postgres => POSTGRES,
        Family::Oracle => ORACLE,
        Family::Dameng => DAMENG,
        Family::SqlServer => SQLSERVER,
    }
}

fn override_file(variant: Variant) -> Option<&'static str> {
    match variant {
        Variant::MariaDb => Some(MARIADB),
        Variant::TiDb => Some(TIDB),
        Variant::OceanBase => Some(OCEANBASE),
        Variant::OpenGauss => Some(OPENGAUSS),
        _ => None,
    }
}

/// 某个变体的运维查询：家族的模板，变体覆盖文件中的同名 key 原位替换。
fn queries(variant: Variant) -> Vec<OpsQuery> {
    let mut list = parse(base_file(variant.family()));
    if let Some(o) = override_file(variant) {
        for q in parse(o) {
            match list.iter_mut().find(|x| x.key == q.key) {
                Some(slot) => *slot = q,
                None => list.push(q),
            }
        }
    }
    list
}

impl Dialect {
    /// 这个方言可用的运维查询（M1c3）。
    pub fn ops_queries(&self) -> &'static [OpsQuery] {
        static CACHE: OnceLock<Vec<(Variant, Vec<OpsQuery>)>> = OnceLock::new();
        let all = CACHE
            .get_or_init(|| crate::meta::template::ALL_VARIANTS.iter().map(|&v| (v, queries(v))).collect());
        &all.iter().find(|(v, _)| *v == self.variant()).expect("每个变体都有运维查询").1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEYS: [&str; 6] = ["sessions", "running", "locks", "storage", "largest_tables", "slow_sql"];

    #[test]
    fn every_variant_has_all_queries() {
        for &v in &crate::meta::template::ALL_VARIANTS {
            let qs = Dialect::new(v).ops_queries();
            assert_eq!(qs.iter().map(|q| q.key.as_str()).collect::<Vec<_>>(), KEYS, "{v:?}");
            for q in qs {
                assert!(!q.sql.is_empty() && !q.sql.contains("-- ["), "{v:?} {}", q.key);
                if matches!(q.category, OpsCategory::Session | OpsCategory::Lock) {
                    assert!(q.sql.contains("kill_sql"), "{v:?} {} 缺 kill_sql 列", q.key);
                }
            }
        }
    }

    #[test]
    fn parse_keeps_order_and_header() {
        let qs = parse(
            "-- 文件说明\n\n-- [b]\n-- name.zh-CN: 乙\n-- name.en: B\n-- category: lock\nSELECT 2\n-- 正文里的注释\nFROM t\n\n-- [a]\n-- name.zh-CN: 甲\n-- name.en: A\n-- category: sql\nSELECT 1\n",
        );
        assert_eq!(qs.len(), 2);
        assert_eq!(
            (qs[0].key.as_str(), qs[0].name_zh.as_str(), qs[0].category),
            ("b", "乙", OpsCategory::Lock)
        );
        assert_eq!(qs[0].sql, "SELECT 2\n-- 正文里的注释\nFROM t");
        assert_eq!((qs[1].key.as_str(), qs[1].name_en.as_str(), qs[1].sql.as_str()), ("a", "A", "SELECT 1"));
    }

    #[test]
    fn mariadb_overrides_in_place() {
        let my = Dialect::new(Variant::MySql).ops_queries();
        let maria = Dialect::new(Variant::MariaDb).ops_queries();
        let locks = |qs: &[OpsQuery]| qs.iter().find(|q| q.key == "locks").unwrap().sql.clone();
        assert_ne!(locks(my), locks(maria));
        assert_eq!(maria[2].key, "locks");
        // 没有覆盖文件的兼容库用所属家族的模板；有覆盖文件的只替换其中的查询，顺序不变
        assert_eq!(Dialect::new(Variant::PolarDb).ops_queries(), my);
        assert_eq!(
            Dialect::new(Variant::Kingbase).ops_queries(),
            Dialect::new(Variant::Postgres).ops_queries()
        );
        let keys = |qs: &[OpsQuery]| qs.iter().map(|q| q.key.clone()).collect::<Vec<_>>();
        for v in [Variant::TiDb, Variant::OceanBase] {
            assert_eq!(keys(Dialect::new(v).ops_queries()), keys(my), "{v:?}");
            assert_ne!(locks(Dialect::new(v).ops_queries()), locks(my), "{v:?}");
        }
        let og = Dialect::new(Variant::OpenGauss).ops_queries();
        assert_eq!(keys(og), keys(Dialect::new(Variant::Postgres).ops_queries()));
        assert!(og.iter().all(|q| !q.sql.contains("backend_type") && !q.sql.contains("pg_blocking_pids")));
    }
}
