//! 设计器的 DDL 生成（M3 设计文档第 4 节）：判断库不支持的改动，按方言把变更列表生成为语句。
//! 每项变更生成一条或几条语句，语句带着它对应的变更序号，预览时可以对照。

use dblens_core::schema::{CheckConstraint, Column, ForeignKey, Index};
use dblens_core::{Error, Msg, Result, Value, msg};
use serde::Serialize;

use super::diff::{is_expression, text};
use super::{Diff, TableChange as C};
use crate::{Dialect, Family, Variant};

/// 一条生成的语句。`changes` 是它对应的变更序号（一项变更也可能生成几条语句）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DesignStatement {
    pub changes: Vec<usize>,
    pub sql: String,
}

pub(crate) fn stmt(change: usize, sql: String) -> DesignStatement {
    DesignStatement { changes: vec![change], sql }
}

impl Dialect {
    /// 整个改动能否放在一个事务中执行：PG 家族和 SQL Server 的 DDL 可以回滚。
    pub fn design_transactional(&self) -> bool {
        matches!(self.family(), Family::Postgres | Family::SqlServer)
    }

    /// 执行前要提示的副作用。
    pub fn design_warnings(&self, diff: &Diff) -> Vec<Msg> {
        if self.family() == Family::Informix { self.ifx_warnings(&diff.changes) } else { Vec::new() }
    }

    /// 库确定不支持的改动及原因；不为空时改用重建表。
    pub fn design_unsupported(&self, diff: &Diff) -> Vec<Msg> {
        let mut out = Vec::new();
        if !self.design_caps().reorder && !diff.appended {
            out.push(msg!("design.unsupported.reorder"));
        }
        // TiDB 的主键默认是聚簇索引，不能直接删除或更换
        if self.variant() == Variant::TiDb
            && diff.changes.iter().any(|c| matches!(c, C::DropPrimaryKey { .. }))
        {
            out.push(msg!("design.unsupported.clustered_pk"));
        }
        for ch in &diff.changes {
            let C::AlterColumn { from, to, .. } = ch else { continue };
            let generated = text(&from.generated) != text(&to.generated);
            let identity = from.auto_increment != to.auto_increment;
            match self.family() {
                Family::MySql => {}
                Family::Postgres => {
                    if generated {
                        out.push(msg!("design.unsupported.generated", column = to.name));
                    }
                    let serial = text(&from.default).is_some_and(|d| d.starts_with("nextval("));
                    if identity && serial {
                        out.push(msg!("design.unsupported.serial", column = to.name));
                    }
                }
                Family::Oracle | Family::Dameng | Family::SqlServer | Family::Informix => {
                    if generated {
                        out.push(msg!("design.unsupported.generated", column = to.name));
                    }
                    if identity {
                        out.push(msg!("design.unsupported.identity", column = to.name));
                    }
                }
            }
        }
        out
    }

    /// 按方言生成 DDL。`table` 是表的现名（新建表时不用）。
    pub fn design_sql(&self, schema: &str, table: &str, diff: &Diff) -> Result<Vec<DesignStatement>> {
        match self.family() {
            Family::MySql => self.mysql_design(schema, table, &diff.changes),
            Family::Postgres => self.pg_design(schema, table, &diff.changes),
            Family::Oracle | Family::Dameng => self.oracle_design(schema, table, &diff.changes),
            Family::SqlServer => self.sqlserver_design(schema, table, &diff.changes),
            Family::Informix => self.informix_design(schema, table, &diff.changes),
        }
    }

    /// 表改名：MySQL `ALTER TABLE … RENAME TO 带 schema 的新名`；SQL Server `sp_rename`；其他库 `RENAME TO 新名`。
    pub(crate) fn rename_table_sql(&self, schema: &str, from: &str, to: &str) -> Result<String> {
        let q = self.qualified(Some(schema), from);
        Ok(match self.family() {
            Family::MySql => format!("ALTER TABLE {q} RENAME TO {}", self.qualified(Some(schema), to)),
            Family::SqlServer => {
                format!("EXEC sp_rename {}, {}", self.text_literal(&q)?, self.text_literal(to)?)
            }
            Family::Informix => format!("RENAME TABLE {q} TO {}", self.ident(to)),
            _ => format!("ALTER TABLE {q} RENAME TO {}", self.ident(to)),
        })
    }

    pub(crate) fn idents(&self, cols: &[String]) -> String {
        cols.iter().map(|c| self.ident(c)).collect::<Vec<_>>().join(", ")
    }

    /// `CONSTRAINT 名字 `，没有名字时为空。
    pub(crate) fn constraint_name(&self, name: &Option<String>) -> String {
        text(name).map(|n| format!("CONSTRAINT {} ", self.ident(n))).unwrap_or_default()
    }

    /// 约束必须有名字才能删除（从库中读出的约束都有）。
    pub(crate) fn required_name<'a>(&self, name: &'a Option<String>) -> Result<&'a str> {
        text(name).ok_or_else(|| Error::invalid(msg!("design.unnamed_constraint")))
    }

    pub(crate) fn text_literal(&self, s: &str) -> Result<String> {
        self.literal(&Value::String(s.into()))
    }

    /// `FOREIGN KEY (…) REFERENCES 表 (…) [ON DELETE …] [ON UPDATE …]`；未写 schema 时引用同一 schema 中的表。
    pub(crate) fn fk_clause(&self, schema: &str, f: &ForeignKey) -> String {
        let mut s = format!(
            "FOREIGN KEY ({}) REFERENCES {} ({})",
            self.idents(&f.columns),
            self.qualified(text(&f.ref_schema).or(Some(schema)), &f.ref_table),
            self.idents(&f.ref_columns)
        );
        // Oracle、达梦没有 ON UPDATE
        let on_update =
            if matches!(self.family(), Family::Oracle | Family::Dameng) { &None } else { &f.on_update };
        for (what, action) in [("DELETE", &f.on_delete), ("UPDATE", on_update)] {
            if let Some(a) = text(action).map(str::to_ascii_uppercase).filter(|a| a != "NO ACTION") {
                s += &format!(" ON {what} {a}");
            }
        }
        s
    }

    pub(crate) fn check_clause(&self, c: &CheckConstraint) -> String {
        let e = c.expression.trim();
        let e = if e.starts_with('(') && e.ends_with(')') { e.to_string() } else { format!("({e})") };
        format!("{}CHECK {e}", self.constraint_name(&c.name))
    }

    /// 索引的列清单：普通列加引号，表达式原样（MySQL 的函数索引要再套一层括号）。
    pub(crate) fn index_items(&self, x: &Index) -> String {
        x.columns
            .iter()
            .map(|c| {
                let name = if !is_expression(&c.name) {
                    self.ident(&c.name)
                } else if self.family() == Family::MySql && !c.name.starts_with('(') {
                    format!("({})", c.name)
                } else {
                    c.name.clone()
                };
                if c.descending { format!("{name} DESC") } else { name }
            })
            .collect::<Vec<_>>()
            .join(", ")
    }

    /// 列的默认值写法（MySQL 之外：生成列和自增列不写默认值）。
    pub(crate) fn default_clause(c: &Column) -> String {
        match text(&c.default) {
            Some(d) if c.generated.is_none() && !c.auto_increment => format!(" DEFAULT {d}"),
            _ => String::new(),
        }
    }
}

#[cfg(test)]
pub(crate) mod fixtures {
    //! 各方言生成测试共用的表。
    use dblens_core::schema::{
        CheckConstraint, Column, ForeignKey, Index, IndexColumn, PrimaryKey, Table, UniqueConstraint,
    };
    use dblens_core::{ColumnType, LogicalType};

    use crate::design::{Diff, Renames, diff_table};

    pub fn col(name: &str, native: &str) -> Column {
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

    /// 一张带全部种类约束和索引的表；`types` 依次是 id、code、amount 的类型。
    pub fn table(types: [&str; 3]) -> Table {
        let mut id = col("id", types[0]);
        id.nullable = false;
        id.auto_increment = true;
        let mut amount = col("amount", types[2]);
        amount.default = Some("0".into());
        amount.comment = Some("金额".into());
        Table {
            name: "orders".into(),
            comment: Some("订单".into()),
            columns: vec![id, col("code", types[1]), amount],
            primary_key: Some(PrimaryKey { name: Some("pk_orders".into()), columns: vec!["id".into()] }),
            uniques: vec![UniqueConstraint { name: Some("uk_code".into()), columns: vec!["code".into()] }],
            checks: vec![CheckConstraint {
                name: Some("ck_amount".into()),
                expression: "amount >= 0".into(),
            }],
            foreign_keys: vec![ForeignKey {
                name: Some("fk_parent".into()),
                columns: vec!["code".into()],
                ref_schema: None,
                ref_table: "parent".into(),
                ref_columns: vec!["code".into()],
                on_delete: Some("CASCADE".into()),
                on_update: None,
            }],
            indexes: vec![Index {
                name: "ix_amount".into(),
                columns: vec![IndexColumn { name: "amount".into(), descending: true }],
                unique: false,
                method: None,
            }],
            ..Default::default()
        }
    }

    /// 覆盖每种变更的一组改动：删检查约束、删索引、删列 amount、改名 code → order_code 并改类型、
    /// 加列 note（放在 id 之后）、换主键、加唯一约束、加索引、索引改名、换外键、改表注释。
    pub fn everything(orig: &Table, varchar: &str) -> Diff {
        let mut d = orig.clone();
        d.checks.clear();
        d.columns.retain(|c| c.name != "amount");
        d.indexes.clear();
        d.columns[1].name = "order_code".into();
        d.columns[1].data_type.native = varchar.into();
        d.columns[1].nullable = false;
        d.columns.insert(1, col("note", varchar));
        d.primary_key = Some(PrimaryKey {
            name: Some("pk_orders".into()),
            columns: vec!["id".into(), "order_code".into()],
        });
        d.uniques =
            vec![UniqueConstraint { name: Some("uk_code".into()), columns: vec!["order_code".into()] }];
        d.uniques.push(UniqueConstraint { name: Some("uk_note".into()), columns: vec!["note".into()] });
        d.indexes.push(Index {
            name: "ix_note".into(),
            columns: vec![IndexColumn { name: "note".into(), descending: false }],
            unique: false,
            method: None,
        });
        d.foreign_keys[0].columns = vec!["order_code".into()];
        d.foreign_keys[0].on_delete = None;
        d.comment = Some("订单表".into());
        let renames =
            Renames { columns: [("order_code".into(), "code".into())].into(), ..Default::default() };
        diff_table(Some(orig), &d, &renames).unwrap()
    }

    /// 只改列的属性：可空、默认值、注释、类型。
    pub fn alter_only(orig: &Table, numeric: &str) -> Diff {
        let mut d = orig.clone();
        let amount = &mut d.columns[2];
        amount.data_type.native = numeric.into();
        amount.nullable = false;
        amount.default = None;
        amount.comment = None;
        d.columns[1].default = Some("'x'".into());
        d.columns[1].comment = Some("编号".into());
        diff_table(Some(orig), &d, &Renames::default()).unwrap()
    }

    /// 只调整列顺序：amount 挪到最前，并把它的索引改名。
    pub fn move_first(orig: &Table) -> Diff {
        let mut d = orig.clone();
        let amount = d.columns.pop().unwrap();
        d.columns.insert(0, amount);
        d.indexes[0].name = "ix_amount2".into();
        let renames =
            Renames { indexes: [("ix_amount2".into(), "ix_amount".into())].into(), ..Default::default() };
        diff_table(Some(orig), &d, &renames).unwrap()
    }

    /// 生成结果拼成一段文字（每条一行，带对应的变更序号），便于快照比较。
    pub fn render(statements: &[crate::design::DesignStatement]) -> String {
        statements.iter().map(|s| format!("{:?} {}", s.changes, s.sql)).collect::<Vec<_>>().join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::fixtures::*;
    use super::*;
    use crate::Variant;

    #[test]
    fn unsupported_changes_by_family() {
        let pg = Dialect::new(Variant::Postgres);
        let orig = table(["bigint", "varchar(20)", "numeric(10,2)"]);
        assert!(pg.design_unsupported(&alter_only(&orig, "numeric(12,2)")).is_empty());
        assert_eq!(pg.design_unsupported(&move_first(&orig)), vec![msg!("design.unsupported.reorder")]);
        // 新列插在中间
        assert_eq!(
            pg.design_unsupported(&everything(&orig, "varchar(50)")),
            vec![msg!("design.unsupported.reorder")]
        );
        let mysql = Dialect::new(Variant::MySql);
        assert!(mysql.design_unsupported(&move_first(&orig)).is_empty());

        // 改变自增：PG 的 serial 列、Oracle 和达梦的已有列不支持
        let mut serial = orig.clone();
        serial.columns[0].default = Some("nextval('orders_id_seq'::regclass)".into());
        let mut d = serial.clone();
        d.columns[0].auto_increment = false;
        let diff = crate::design::diff_table(Some(&serial), &d, &Default::default()).unwrap();
        assert_eq!(pg.design_unsupported(&diff), vec![msg!("design.unsupported.serial", column = "id")]);
        let mut d = orig.clone();
        d.columns[0].auto_increment = false;
        let diff = crate::design::diff_table(Some(&orig), &d, &Default::default()).unwrap();
        assert!(pg.design_unsupported(&diff).is_empty());
        assert_eq!(
            Dialect::new(Variant::Oracle).design_unsupported(&diff),
            vec![msg!("design.unsupported.identity", column = "id")]
        );
        // 生成列的表达式
        let mut d = orig.clone();
        d.columns[1].generated = Some("upper(code)".into());
        let diff = crate::design::diff_table(Some(&orig), &d, &Default::default()).unwrap();
        assert_eq!(pg.design_unsupported(&diff), vec![msg!("design.unsupported.generated", column = "code")]);
        assert!(mysql.design_unsupported(&diff).is_empty());
        assert!(pg.design_transactional() && !mysql.design_transactional());
    }

    #[test]
    fn every_generated_statement_splits_into_one() {
        let types = ["BIGINT", "VARCHAR(20)", "DECIMAL(10,2)"];
        for v in [
            Variant::MySql,
            Variant::MariaDb,
            Variant::Postgres,
            Variant::Oracle,
            Variant::Dameng,
            Variant::SqlServer,
        ] {
            let d = Dialect::new(v);
            let mut t = table(types);
            // 注释、默认值里带分号、注释符和 `/`
            t.comment = Some(
                "a;b -- c
/
"
                .into(),
            );
            t.columns[1].default = Some("';--'".into());
            let create = crate::design::diff_table(None, &t, &Default::default()).unwrap();
            for diff in
                [create, everything(&t, "VARCHAR(50)"), alter_only(&t, "DECIMAL(12,2)"), move_first(&t)]
            {
                for s in d.design_sql("s", "orders", &diff).unwrap() {
                    let parts = d.split(&s.sql);
                    assert_eq!(parts.len(), 1, "{v:?}: {}", s.sql);
                    assert_eq!(parts[0].text, s.sql, "{v:?}");
                }
            }
        }
    }
}
