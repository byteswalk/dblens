//! 关联查询与数据标签的 SQL（M1c2 设计文档第 4、5 节）。

use dblens_core::schema::Table;
use serde::{Deserialize, Serialize};

use crate::Dialect;

/// 外键的方向：本表引用对方（`Out`，对方是父表），或对方引用本表（`In`，对方是子表）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    Out,
    In,
}

/// 与某张表有外键关系的表。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Relation {
    pub direction: Direction,
    /// 外键约束名。
    pub name: Option<String>,
    pub schema: String,
    pub table: String,
    /// 本表一侧的列。
    pub local_columns: Vec<String>,
    /// 对方一侧的列。
    pub other_columns: Vec<String>,
}

/// 表 `table`（在 `schema` 中）的外键关系：先是它引用的表，再是同一 schema 中引用它的表（`all` 为该 schema 的全部表）。
/// 自引用的外键只算一次（作为 `Out`）。
pub fn relations(schema: &str, table: &Table, all: &[Table]) -> Vec<Relation> {
    let mut out: Vec<Relation> = table
        .foreign_keys
        .iter()
        .map(|f| Relation {
            direction: Direction::Out,
            name: f.name.clone(),
            schema: f.ref_schema.clone().unwrap_or_else(|| schema.to_string()),
            table: f.ref_table.clone(),
            local_columns: f.columns.clone(),
            other_columns: f.ref_columns.clone(),
        })
        .collect();
    for t in all.iter().filter(|t| t.name != table.name) {
        for f in &t.foreign_keys {
            if f.ref_table == table.name && f.ref_schema.as_deref().unwrap_or(schema) == schema {
                out.push(Relation {
                    direction: Direction::In,
                    name: f.name.clone(),
                    schema: schema.to_string(),
                    table: t.name.clone(),
                    local_columns: f.ref_columns.clone(),
                    other_columns: f.columns.clone(),
                });
            }
        }
    }
    out
}

impl Dialect {
    /// 关联查询：本表别名 `t1`，相关的表依次为 `t2`、`t3`…，每个外键一个 JOIN，多列外键的条件用 AND 连接。
    pub fn join_sql(&self, schema: &str, table: &str, rels: &[Relation]) -> String {
        let mut select = vec!["t1.*".to_string()];
        let mut joins = Vec::new();
        for (i, r) in rels.iter().enumerate() {
            let alias = format!("t{}", i + 2);
            select.push(format!("{alias}.*"));
            let on: Vec<String> = r
                .local_columns
                .iter()
                .zip(&r.other_columns)
                .map(|(l, o)| format!("t1.{} = {alias}.{}", self.ident(l), self.ident(o)))
                .collect();
            joins.push(format!(
                "JOIN {} {alias} ON {}",
                self.qualified(Some(&r.schema), &r.table),
                on.join(" AND ")
            ));
        }
        let mut sql =
            format!("SELECT {}\nFROM {} t1", select.join(", "), self.qualified(Some(schema), table));
        for j in joins {
            sql += "\n";
            sql += &j;
        }
        sql
    }

    /// 数据标签的查询：`SELECT * FROM 对象 [WHERE (过滤)] [ORDER BY 列 ASC|DESC]`；`count` 时为 `SELECT COUNT(*)`（不排序）。
    /// 过滤条件由用户书写，原样放进括号中。
    pub fn data_sql(
        &self,
        schema: &str,
        object: &str,
        filter: Option<&str>,
        order: Option<(&str, bool)>,
        count: bool,
    ) -> String {
        let mut sql = format!(
            "SELECT {} FROM {}",
            if count { "COUNT(*)" } else { "*" },
            self.qualified(Some(schema), object)
        );
        if let Some(f) = filter.map(str::trim).filter(|f| !f.is_empty()) {
            sql += &format!(" WHERE ({f})");
        }
        if let (false, Some((column, desc))) = (count, order) {
            sql += &format!(" ORDER BY {} {}", self.ident(column), if desc { "DESC" } else { "ASC" });
        }
        sql
    }
}

#[cfg(test)]
mod tests {
    use dblens_core::schema::ForeignKey;

    use super::*;
    use crate::Variant;

    fn fk(cols: &[&str], ref_schema: Option<&str>, ref_table: &str, ref_cols: &[&str]) -> ForeignKey {
        ForeignKey {
            name: Some(format!("fk_{ref_table}")),
            columns: cols.iter().map(|s| s.to_string()).collect(),
            ref_schema: ref_schema.map(str::to_string),
            ref_table: ref_table.into(),
            ref_columns: ref_cols.iter().map(|s| s.to_string()).collect(),
            on_delete: None,
            on_update: None,
        }
    }

    fn table(name: &str, fks: Vec<ForeignKey>) -> Table {
        Table { name: name.into(), foreign_keys: fks, ..Default::default() }
    }

    /// emp 引用 dept 和 hr2.region（两列）；task 引用 emp；emp 自引用（manager）。
    fn schema() -> Vec<Table> {
        vec![
            table(
                "emp",
                vec![
                    fk(&["dept_id"], None, "dept", &["id"]),
                    fk(&["region", "country"], Some("hr2"), "region", &["code", "country"]),
                    fk(&["manager_id"], Some("hr"), "emp", &["id"]),
                ],
            ),
            table("dept", vec![]),
            table("task", vec![fk(&["owner_id"], Some("hr"), "emp", &["id"])]),
            table("other", vec![fk(&["x"], Some("elsewhere"), "emp", &["id"])]),
        ]
    }

    #[test]
    fn relations_in_both_directions() {
        let all = schema();
        let rels = relations("hr", &all[0], &all);
        let got: Vec<_> = rels.iter().map(|r| (r.direction, r.schema.as_str(), r.table.as_str())).collect();
        assert_eq!(
            got,
            [
                (Direction::Out, "hr", "dept"),
                (Direction::Out, "hr2", "region"),
                (Direction::Out, "hr", "emp"),
                (Direction::In, "hr", "task"),
            ],
            "其他 schema 的同名表不算；自引用只算一次"
        );
        assert_eq!(
            (rels[3].local_columns.clone(), rels[3].other_columns.clone()),
            (vec!["id".to_string()], vec!["owner_id".to_string()])
        );
    }

    #[test]
    fn join_uses_aliases_and_multi_column_conditions() {
        let all = schema();
        let rels = relations("hr", &all[0], &all);
        let d = Dialect::new(Variant::Postgres);
        assert_eq!(
            d.join_sql("hr", "emp", &rels[..2]),
            "SELECT t1.*, t2.*, t3.*\nFROM hr.emp t1\nJOIN hr.dept t2 ON t1.dept_id = t2.id\nJOIN hr2.region t3 ON t1.region = t3.code AND t1.country = t3.country"
        );
        assert_eq!(
            d.join_sql("hr", "emp", &rels[3..]),
            "SELECT t1.*, t2.*\nFROM hr.emp t1\nJOIN hr.task t2 ON t1.id = t2.owner_id"
        );
    }

    #[test]
    fn data_sql_filter_order_and_count() {
        let d = Dialect::new(Variant::Oracle);
        assert_eq!(d.data_sql("HR", "EMP", None, None, false), "SELECT * FROM HR.EMP");
        assert_eq!(
            d.data_sql("HR", "Emp Tab", Some(" salary > 10 OR 1=1 "), Some(("Name", true)), false),
            "SELECT * FROM HR.\"Emp Tab\" WHERE (salary > 10 OR 1=1) ORDER BY \"Name\" DESC"
        );
        assert_eq!(
            d.data_sql("HR", "EMP", Some("a = 1"), Some(("ID", false)), true),
            "SELECT COUNT(*) FROM HR.EMP WHERE (a = 1)"
        );
        assert_eq!(
            Dialect::new(Variant::MySql).data_sql("shop", "t", Some("  "), Some(("id", false)), false),
            "SELECT * FROM shop.t ORDER BY id ASC"
        );
    }
}
