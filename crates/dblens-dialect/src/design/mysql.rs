//! MySQL 家族（MySQL、MariaDB、TiDB、OceanBase、PolarDB）的设计器 DDL（M3 设计文档 4.2 节）。
//! 每项变更一条 `ALTER TABLE`：同一条 ALTER 中改名与 MODIFY、AFTER 引用的是改名前还是改名后的列，
//! 各版本行为不一，分开执行最可预期（MySQL 8 的多数列操作是 INSTANT / INPLACE，不重建表）。

use std::collections::HashMap;

use dblens_core::Result;
use dblens_core::schema::{Column, Index, Table};

use super::TableChange as C;
use super::diff::text;
use super::sql::{DesignStatement, stmt};
use crate::{Dialect, Variant};

impl Dialect {
    /// 支持 `RENAME COLUMN`：MySQL 8.0、MariaDB 10.5 起。
    fn mysql_rename_column(&self) -> bool {
        let v = self.version().unwrap_or(u32::MAX);
        if self.variant() == Variant::MariaDb { v >= 100500 } else { v >= 80000 }
    }

    /// 支持 `RENAME INDEX`：MySQL 5.7、MariaDB 10.5.2 起。
    fn mysql_rename_index(&self) -> bool {
        let v = self.version().unwrap_or(u32::MAX);
        if self.variant() == Variant::MariaDb { v >= 100502 } else { v >= 50700 }
    }

    /// 列定义：类型、生成表达式、可空、默认值、自增、注释。
    fn mysql_column(&self, c: &Column) -> Result<String> {
        let mut s = format!("{} {}", self.ident(&c.name), c.data_type.native);
        if let Some(g) = text(&c.generated) {
            s += &format!(" GENERATED ALWAYS AS ({g})");
        }
        s += if c.nullable { " NULL" } else { " NOT NULL" };
        s += &Self::default_clause(c);
        if c.auto_increment {
            s += " AUTO_INCREMENT";
        }
        if let Some(cm) = text(&c.comment) {
            s += &format!(" COMMENT {}", self.text_literal(cm)?);
        }
        Ok(s)
    }

    fn mysql_position(&self, after: &Option<String>) -> String {
        match after {
            None => " FIRST".into(),
            Some(a) => format!(" AFTER {}", self.ident(a)),
        }
    }

    /// `INDEX 名字 (列…) [USING 方法]`，FULLTEXT / SPATIAL 写在前面；`add` 时前面加 `ADD `。
    fn mysql_index(&self, x: &Index) -> String {
        let method = x.method.as_deref().map(str::to_ascii_uppercase);
        let kind = match method.as_deref() {
            Some("FULLTEXT") => "FULLTEXT ",
            Some("SPATIAL") => "SPATIAL ",
            _ if x.unique => "UNIQUE ",
            _ => "",
        };
        let using = match method.as_deref() {
            Some(m @ ("BTREE" | "HASH")) => format!(" USING {m}"),
            _ => String::new(),
        };
        format!("{kind}INDEX {} ({}){using}", self.ident(&x.name), self.index_items(x))
    }

    fn mysql_create(&self, schema: &str, t: &Table) -> Result<String> {
        let mut items = Vec::new();
        for c in &t.columns {
            items.push(self.mysql_column(c)?);
        }
        if let Some(pk) = &t.primary_key {
            items.push(format!("PRIMARY KEY ({})", self.idents(&pk.columns)));
        }
        for u in &t.uniques {
            items.push(format!("{}UNIQUE ({})", self.constraint_name(&u.name), self.idents(&u.columns)));
        }
        for x in &t.indexes {
            items.push(self.mysql_index(x));
        }
        for f in &t.foreign_keys {
            items.push(format!("{}{}", self.constraint_name(&f.name), self.fk_clause(schema, f)));
        }
        for c in &t.checks {
            items.push(self.check_clause(c));
        }
        let mut sql = format!(
            "CREATE TABLE {} (\n    {}\n)",
            self.qualified(Some(schema), &t.name),
            items.join(",\n    ")
        );
        if let Some(c) = text(&t.comment) {
            sql += &format!(" COMMENT = {}", self.text_literal(c)?);
        }
        Ok(sql)
    }

    pub(crate) fn mysql_design(
        &self,
        schema: &str,
        table: &str,
        changes: &[C],
    ) -> Result<Vec<DesignStatement>> {
        let q = self.qualified(Some(schema), table);
        let alter = |clause: String| format!("ALTER TABLE {q} {clause}");
        // 既修改又挪动的列：只在挪动时用草稿的完整定义 MODIFY 一次
        let moved: HashMap<&str, usize> = changes
            .iter()
            .enumerate()
            .filter_map(|(i, c)| match c {
                C::MoveColumn { column, .. } => Some((column.name.as_str(), i)),
                _ => None,
            })
            .collect();
        // 换主键：删除和新增放在同一条 ALTER 里（自增列必须是键，单独删主键会报错 1075）
        let add_pk = changes.iter().position(|c| matches!(c, C::AddPrimaryKey { .. }));
        let drop_pk = changes.iter().position(|c| matches!(c, C::DropPrimaryKey { .. }));
        let mut out: Vec<DesignStatement> = Vec::new();
        let mut merged: HashMap<usize, usize> = HashMap::new();
        for (i, ch) in changes.iter().enumerate() {
            let sql = match ch {
                C::CreateTable { table } => self.mysql_create(schema, table)?,
                C::SetTableComment { comment } => {
                    alter(format!("COMMENT = {}", self.text_literal(comment.as_deref().unwrap_or(""))?))
                }
                C::AddColumn { column, after } => {
                    alter(format!("ADD COLUMN {}{}", self.mysql_column(column)?, self.mysql_position(after)))
                }
                C::DropColumn { name } => alter(format!("DROP COLUMN {}", self.ident(name))),
                C::RenameColumn { from, to, column } => {
                    if self.mysql_rename_column() {
                        alter(format!("RENAME COLUMN {} TO {}", self.ident(from), self.ident(to)))
                    } else {
                        alter(format!("CHANGE COLUMN {} {}", self.ident(from), self.mysql_column(column)?))
                    }
                }
                C::AlterColumn { to, .. } => {
                    if let Some(&m) = moved.get(to.name.as_str()) {
                        merged.insert(m, i);
                        continue;
                    }
                    alter(format!("MODIFY COLUMN {}", self.mysql_column(to)?))
                }
                C::MoveColumn { column, after } => alter(format!(
                    "MODIFY COLUMN {}{}",
                    self.mysql_column(column)?,
                    self.mysql_position(after)
                )),
                C::DropPrimaryKey { .. } if add_pk.is_some() => {
                    merged.insert(add_pk.expect("有新主键"), i);
                    continue;
                }
                C::DropPrimaryKey { .. } => alter("DROP PRIMARY KEY".into()),
                C::AddPrimaryKey { key } => alter(format!(
                    "{}ADD PRIMARY KEY ({})",
                    if drop_pk.is_some() { "DROP PRIMARY KEY, " } else { "" },
                    self.idents(&key.columns)
                )),
                C::DropUnique { unique } => {
                    alter(format!("DROP INDEX {}", self.ident(self.required_name(&unique.name)?)))
                }
                C::AddUnique { unique } => alter(format!(
                    "ADD {}UNIQUE ({})",
                    self.constraint_name(&unique.name),
                    self.idents(&unique.columns)
                )),
                C::DropCheck { check } => {
                    let what = if self.variant() == Variant::MariaDb { "CONSTRAINT" } else { "CHECK" };
                    alter(format!("DROP {what} {}", self.ident(self.required_name(&check.name)?)))
                }
                C::AddCheck { check } => alter(format!("ADD {}", self.check_clause(check))),
                C::DropForeignKey { key } => {
                    alter(format!("DROP FOREIGN KEY {}", self.ident(self.required_name(&key.name)?)))
                }
                C::AddForeignKey { key } => {
                    alter(format!("ADD {}{}", self.constraint_name(&key.name), self.fk_clause(schema, key)))
                }
                C::DropIndex { index } => alter(format!("DROP INDEX {}", self.ident(&index.name))),
                C::AddIndex { index } => alter(format!("ADD {}", self.mysql_index(index))),
                C::RenameIndex { from, index } => {
                    if self.mysql_rename_index() {
                        alter(format!("RENAME INDEX {} TO {}", self.ident(from), self.ident(&index.name)))
                    } else {
                        alter(format!("DROP INDEX {}, ADD {}", self.ident(from), self.mysql_index(index)))
                    }
                }
            };
            let mut s = stmt(i, sql);
            if let Some(&a) = merged.get(&i) {
                s.changes.insert(0, a);
            }
            out.push(s);
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::super::sql::fixtures::*;
    use crate::design::{Renames, diff_table};
    use crate::{Dialect, Variant};

    const TYPES: [&str; 3] = ["BIGINT", "VARCHAR(20)", "DECIMAL(10,2)"];

    fn mysql8() -> Dialect {
        Dialect::new(Variant::MySql).with_version(Some(80040))
    }

    #[test]
    fn create_table() {
        let t = table(TYPES);
        let d = mysql8();
        let diff = diff_table(None, &t, &Renames::default()).unwrap();
        insta::assert_snapshot!("mysql_create", render(&d.design_sql("shop", "", &diff).unwrap()));
    }

    #[test]
    fn every_change() {
        let orig = table(TYPES);
        let d = mysql8();
        insta::assert_snapshot!(
            "mysql_everything",
            render(&d.design_sql("shop", "orders", &everything(&orig, "VARCHAR(50)")).unwrap())
        );
        insta::assert_snapshot!(
            "mysql_alter",
            render(&d.design_sql("shop", "orders", &alter_only(&orig, "DECIMAL(12,2)")).unwrap())
        );
        insta::assert_snapshot!(
            "mysql_move",
            render(&d.design_sql("shop", "orders", &move_first(&orig)).unwrap())
        );
    }

    #[test]
    fn old_versions_and_mariadb() {
        let orig = table(TYPES);
        let old = Dialect::new(Variant::MySql).with_version(Some(50650));
        insta::assert_snapshot!(
            "mysql56_everything",
            render(&old.design_sql("shop", "orders", &everything(&orig, "VARCHAR(50)")).unwrap())
        );
        insta::assert_snapshot!(
            "mysql56_move",
            render(&old.design_sql("shop", "orders", &move_first(&orig)).unwrap())
        );
        let maria = Dialect::new(Variant::MariaDb).with_version(Some(100411));
        let sql = render(&maria.design_sql("shop", "orders", &everything(&orig, "VARCHAR(50)")).unwrap());
        assert!(sql.contains("DROP CONSTRAINT ck_amount"), "{sql}");
        assert!(sql.contains("CHANGE COLUMN code order_code"), "{sql}");
    }

    #[test]
    fn move_and_alter_together_is_one_modify() {
        let orig = table(TYPES);
        let mut d = orig.clone();
        let mut amount = d.columns.pop().unwrap();
        amount.comment = Some("合计".into());
        d.columns.insert(0, amount);
        let diff = diff_table(Some(&orig), &d, &Renames::default()).unwrap();
        let out = mysql8().design_sql("shop", "orders", &diff).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].changes, vec![0, 1]);
        assert_eq!(
            out[0].sql,
            "ALTER TABLE shop.orders MODIFY COLUMN amount DECIMAL(10,2) NULL DEFAULT 0 COMMENT '合计' FIRST"
        );
    }
}
