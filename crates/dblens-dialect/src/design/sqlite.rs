//! SQLite 的设计器 DDL（M10b）。`ALTER TABLE` 只能改表名、加列、删列、改列名；其余改动（类型、可空、默认值、
//! 约束、列顺序）由 `design_unsupported` 改为重建表（`rebuild.rs` 的 SQLite 分支）。
//! 约束只能写在建表语句里；外键的 REFERENCES 不能带 schema；没有注释。整个改动在一个事务中执行。

use dblens_core::schema::{Column, ForeignKey, Index, Table};
use dblens_core::{Msg, Result, msg};

use super::TableChange as C;
use super::diff::text;
use super::sql::{DesignStatement, stmt};
use crate::Dialect;

impl Dialect {
    /// 自增列只能是 rowid 的别名：单列主键、类型 INTEGER，写成 `INTEGER PRIMARY KEY AUTOINCREMENT`。
    fn sqlite_rowid_alias(t: &Table) -> Option<&Column> {
        let pk = t.primary_key.as_ref()?;
        let [only] = pk.columns.as_slice() else { return None };
        t.columns.iter().find(|c| &c.name == only && c.auto_increment)
    }

    /// 列定义。`rowid` 是这一列作 rowid 别名（自增主键）。
    fn sqlite_column(&self, c: &Column, rowid: bool) -> String {
        if rowid {
            return format!("{} INTEGER PRIMARY KEY AUTOINCREMENT", self.ident(&c.name));
        }
        let mut s = self.ident(&c.name);
        let ty = c.data_type.native.trim();
        if !ty.is_empty() {
            s += &format!(" {ty}");
        }
        if let Some(g) = text(&c.generated) {
            s += &format!(" GENERATED ALWAYS AS ({g}) STORED");
        } else {
            s += &Self::default_clause(c);
        }
        if !c.nullable {
            s += " NOT NULL";
        }
        s
    }

    /// 外键子句：REFERENCES 只写表名（SQLite 的外键只能引用同一个库里的表）。
    pub(crate) fn sqlite_fk_clause(&self, f: &ForeignKey) -> String {
        let mut s = format!(
            "{}FOREIGN KEY ({}) REFERENCES {} ({})",
            self.constraint_name(&f.name),
            self.idents(&f.columns),
            self.ident(&f.ref_table),
            self.idents(&f.ref_columns)
        );
        for (what, action) in [("DELETE", &f.on_delete), ("UPDATE", &f.on_update)] {
            if let Some(a) = text(action).map(str::to_ascii_uppercase).filter(|a| a != "NO ACTION") {
                s += &format!(" ON {what} {a}");
            }
        }
        s
    }

    /// 建表语句（不含索引）：列、主键、唯一、外键、检查。
    pub(crate) fn sqlite_create_table(&self, schema: &str, t: &Table) -> String {
        let rowid = Self::sqlite_rowid_alias(t).map(|c| c.name.clone());
        let mut items: Vec<String> = t
            .columns
            .iter()
            .map(|c| self.sqlite_column(c, rowid.as_deref() == Some(c.name.as_str())))
            .collect();
        if let (Some(pk), None) = (&t.primary_key, &rowid) {
            items.push(format!(
                "{}PRIMARY KEY ({})",
                self.constraint_name(&pk.name),
                self.idents(&pk.columns)
            ));
        }
        for u in &t.uniques {
            items.push(format!("{}UNIQUE ({})", self.constraint_name(&u.name), self.idents(&u.columns)));
        }
        for f in &t.foreign_keys {
            items.push(self.sqlite_fk_clause(f));
        }
        for c in &t.checks {
            items.push(self.check_clause(c));
        }
        format!("CREATE TABLE {} (\n    {}\n)", self.qualified(Some(schema), &t.name), items.join(",\n    "))
    }

    /// `CREATE INDEX "schema"."名字" ON 表 (列…)`：索引名带 schema，表名不带。
    pub(crate) fn sqlite_index(&self, schema: &str, table: &str, x: &Index) -> String {
        format!(
            "CREATE {}INDEX {} ON {} ({})",
            if x.unique { "UNIQUE " } else { "" },
            self.qualified(Some(schema), &x.name),
            self.ident(table),
            self.index_items(x)
        )
    }

    /// 能直接 `ADD COLUMN` 的列：没有主键、唯一约束（由约束变更另行判断），不是生成列（只能加 VIRTUAL），
    /// 非空时要有默认值，默认值是常量（不能是 CURRENT_TIMESTAMP 这类或括号里的表达式）。
    fn sqlite_can_add(c: &Column) -> bool {
        if c.generated.is_some() || c.auto_increment {
            return false;
        }
        match text(&c.default) {
            None => c.nullable,
            Some(d) => {
                let u = d.trim().to_ascii_uppercase();
                !u.starts_with('(')
                    && !matches!(u.as_str(), "CURRENT_TIME" | "CURRENT_DATE" | "CURRENT_TIMESTAMP")
            }
        }
    }

    /// 只能重建表才能做的改动。
    pub(crate) fn sqlite_unsupported(&self, changes: &[C]) -> Vec<Msg> {
        let mut out = Vec::new();
        for ch in changes {
            match ch {
                C::CreateTable { .. }
                | C::DropColumn { .. }
                | C::RenameColumn { .. }
                | C::AddIndex { .. }
                | C::DropIndex { .. }
                | C::RenameIndex { .. }
                | C::SetTableComment { .. }
                | C::MoveColumn { .. } => {}
                C::AddColumn { column, .. } if Self::sqlite_can_add(column) => {}
                C::AddColumn { column, .. } => {
                    out.push(msg!("sqlite.design.add_column", column = column.name))
                }
                C::AlterColumn { to, .. } => out.push(msg!("sqlite.design.alter_column", column = to.name)),
                C::AddPrimaryKey { .. }
                | C::DropPrimaryKey { .. }
                | C::AddUnique { .. }
                | C::DropUnique { .. }
                | C::AddCheck { .. }
                | C::DropCheck { .. }
                | C::AddForeignKey { .. }
                | C::DropForeignKey { .. } => {
                    let m = msg!("sqlite.design.constraints");
                    if !out.contains(&m) {
                        out.push(m);
                    }
                }
            }
        }
        out
    }

    pub(crate) fn sqlite_design(
        &self,
        schema: &str,
        table: &str,
        changes: &[C],
    ) -> Result<Vec<DesignStatement>> {
        let q = self.qualified(Some(schema), table);
        let mut out = Vec::new();
        for (i, ch) in changes.iter().enumerate() {
            let sqls: Vec<String> = match ch {
                C::CreateTable { table: t } => {
                    let mut v = vec![self.sqlite_create_table(schema, t)];
                    v.extend(t.indexes.iter().map(|x| self.sqlite_index(schema, &t.name, x)));
                    v
                }
                C::AddColumn { column, .. } => {
                    vec![format!("ALTER TABLE {q} ADD COLUMN {}", self.sqlite_column(column, false))]
                }
                C::DropColumn { name } => vec![format!("ALTER TABLE {q} DROP COLUMN {}", self.ident(name))],
                C::RenameColumn { from, to, .. } => {
                    vec![format!("ALTER TABLE {q} RENAME COLUMN {} TO {}", self.ident(from), self.ident(to))]
                }
                C::AddIndex { index } => vec![self.sqlite_index(schema, table, index)],
                C::DropIndex { index } => {
                    vec![format!("DROP INDEX {}", self.qualified(Some(schema), &index.name))]
                }
                // 没有改名：删掉重建
                C::RenameIndex { from, index } => vec![
                    format!("DROP INDEX {}", self.qualified(Some(schema), from)),
                    self.sqlite_index(schema, table, index),
                ],
                // 没有注释；列顺序、其余改动由 design_unsupported 改为重建，不会走到这里
                C::SetTableComment { .. } | C::MoveColumn { .. } => vec![],
                _ => return Err(dblens_core::Error::internal(msg!("sqlite.design.needs_rebuild"))),
            };
            out.extend(sqls.into_iter().map(|s| stmt(i, s)));
        }
        Ok(out)
    }

    /// 草稿里 SQLite 做不到的设置：自增列必须是单列主键（rowid 的别名）。
    pub(crate) fn sqlite_issues(&self, draft: &Table) -> Vec<super::DesignIssue> {
        let rowid = Self::sqlite_rowid_alias(draft).map(|c| c.name.as_str());
        draft
            .columns
            .iter()
            .enumerate()
            .filter(|(_, c)| c.auto_increment && Some(c.name.as_str()) != rowid)
            .map(|(i, c)| {
                super::DesignIssue::new(
                    super::IssueSection::Columns,
                    Some(i),
                    Some("auto_increment"),
                    msg!("sqlite.design.auto_increment", column = c.name),
                )
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use dblens_core::schema::{IndexColumn, PrimaryKey};
    use dblens_core::{ColumnType, LogicalType};

    use super::super::{Diff, Renames, diff_table};
    use super::*;
    use crate::Variant;

    fn col(name: &str, native: &str) -> Column {
        Column {
            name: name.into(),
            ordinal: 0,
            data_type: ColumnType { native: native.into(), logical: LogicalType::Other },
            nullable: true,
            default: None,
            comment: None,
            auto_increment: false,
            generated: None,
        }
    }

    fn table() -> Table {
        let mut id = col("id", "INTEGER");
        id.auto_increment = true;
        id.nullable = false;
        Table {
            name: "orders".into(),
            columns: vec![id, col("code", "TEXT"), col("cust_id", "INTEGER")],
            primary_key: Some(PrimaryKey { name: None, columns: vec!["id".into()] }),
            foreign_keys: vec![ForeignKey {
                name: Some("fk_cust".into()),
                columns: vec!["cust_id".into()],
                ref_schema: Some("main".into()),
                ref_table: "cust".into(),
                ref_columns: vec!["id".into()],
                on_delete: Some("CASCADE".into()),
                on_update: Some("NO ACTION".into()),
            }],
            indexes: vec![Index {
                name: "ix_code".into(),
                columns: vec![IndexColumn { name: "lower(code)".into(), descending: true }],
                unique: false,
                method: None,
            }],
            ..Default::default()
        }
    }

    fn sql(orig: Option<&Table>, draft: &Table) -> (Vec<String>, Vec<Msg>) {
        let d = Dialect::new(Variant::Sqlite);
        let diff: Diff = diff_table(orig, draft, &Renames::default()).unwrap();
        let unsupported = d.design_unsupported(&diff);
        let sqls = if unsupported.is_empty() {
            d.design_sql("main", &draft.name, &diff).unwrap().into_iter().map(|s| s.sql).collect()
        } else {
            vec![]
        };
        (sqls, unsupported)
    }

    #[test]
    fn create_table_inlines_constraints() {
        let (s, u) = sql(None, &table());
        assert!(u.is_empty());
        assert_eq!(
            s,
            [
                "CREATE TABLE main.orders (\n    id INTEGER PRIMARY KEY AUTOINCREMENT,\n    code TEXT,\n    cust_id INTEGER,\n    \
                 CONSTRAINT fk_cust FOREIGN KEY (cust_id) REFERENCES cust (id) ON DELETE CASCADE\n)",
                "CREATE INDEX main.ix_code ON orders (lower(code) DESC)",
            ]
        );
    }

    #[test]
    fn simple_changes_use_alter_others_rebuild() {
        let t = table();
        let mut d = t.clone();
        d.columns.push(col("note", "TEXT"));
        d.columns.retain(|c| c.name != "code");
        d.indexes.clear();
        let (s, u) = sql(Some(&t), &d);
        assert!(u.is_empty(), "{u:?}");
        assert_eq!(
            s,
            [
                "DROP INDEX main.ix_code",
                "ALTER TABLE main.orders DROP COLUMN code",
                "ALTER TABLE main.orders ADD COLUMN note TEXT",
            ]
        );
        // 非空且没有默认值的列、改类型、改约束都要重建
        let mut d = t.clone();
        let mut n = col("n", "INT");
        n.nullable = false;
        d.columns.push(n);
        assert_eq!(sql(Some(&t), &d).1.len(), 1);
        let mut d = t.clone();
        d.columns[1].data_type.native = "VARCHAR(10)".into();
        assert_eq!(sql(Some(&t), &d).1.len(), 1);
        let mut d = t.clone();
        d.foreign_keys.clear();
        assert_eq!(sql(Some(&t), &d).1.len(), 1);
        // 非空但有常量默认值的列可以直接加
        let mut d = t.clone();
        let mut n = col("n", "INT");
        n.nullable = false;
        n.default = Some("0".into());
        d.columns.push(n);
        assert_eq!(sql(Some(&t), &d).0, ["ALTER TABLE main.orders ADD COLUMN n INT DEFAULT 0 NOT NULL"]);
    }

    #[test]
    fn auto_increment_must_be_the_single_pk() {
        let d = Dialect::new(Variant::Sqlite);
        let mut t = table();
        assert!(d.sqlite_issues(&t).is_empty());
        t.primary_key.as_mut().unwrap().columns.push("code".into());
        assert_eq!(d.sqlite_issues(&t).len(), 1);
    }
}
