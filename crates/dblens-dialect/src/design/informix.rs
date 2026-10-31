//! GBase 8s 的设计器 DDL。约束名写在定义之后（`UNIQUE (a) CONSTRAINT 名字`）；`MODIFY` 要给出完整的列定义，
//! 省略默认值就会清掉；改名用 `RENAME COLUMN`、`RENAME INDEX`。
//! 增、改、删列合成一条 `ALTER TABLE`，放在改列名之前：有视图依赖时，同一会话里先改名再 `MODIFY` 这一列，或者连续两条
//! `MODIFY`，都报 -201（服务器的行为，实测）。
//! `MODIFY` 还会删掉这一列上的单列主键、唯一、外键和检查约束，以及别的表引用它的外键；联合约束不受影响。
//! 这里重建本表的约束，别的表的外键由 `design_restore_keys` 在最后重建。

use dblens_core::schema::{Column, ForeignKey, Table};
use dblens_core::{Msg, Result, msg};

use super::diff::text;
use super::sql::{DesignStatement, stmt};
use super::{ColumnConstraints, TableChange as C};
use crate::meta::ReferencingKey;
use crate::{Dialect, Family};

impl Dialect {
    fn ifx_column(&self, c: &Column) -> String {
        let native = c.data_type.native.to_ascii_uppercase();
        // 自增就是 SERIAL 类型
        let kind = match (c.auto_increment, native.as_str()) {
            (true, "BIGINT" | "INT8" | "BIGSERIAL" | "SERIAL8") => "BIGSERIAL",
            (true, _) => "SERIAL",
            (false, _) => c.data_type.native.as_str(),
        };
        let mut s = format!("{} {kind}{}", self.ident(&c.name), Self::default_clause(c));
        if !c.nullable {
            s += " NOT NULL";
        }
        s
    }

    fn ifx_named(&self, clause: String, name: &Option<String>) -> String {
        match text(name) {
            Some(n) => format!("{clause} CONSTRAINT {}", self.ident(n)),
            None => clause,
        }
    }

    pub(crate) fn ifx_fk(&self, schema: &str, f: &ForeignKey) -> String {
        self.ifx_named(self.fk_clause(schema, f), &f.name)
    }

    fn ifx_check(&self, c: &dblens_core::schema::CheckConstraint) -> String {
        let e = c.expression.trim();
        let e = if e.starts_with('(') && e.ends_with(')') { e.to_string() } else { format!("({e})") };
        self.ifx_named(format!("CHECK {e}"), &c.name)
    }

    fn ifx_index(&self, schema: &str, table: &str, x: &dblens_core::schema::Index) -> String {
        let unique = if x.unique { "UNIQUE " } else { "" };
        format!(
            "CREATE {unique}INDEX {} ON {} ({})",
            self.ident(&x.name),
            self.qualified(Some(schema), table),
            self.index_items(x)
        )
    }

    /// `MODIFY` 后要重建的约束，顺序：主键、唯一、检查、外键。
    fn ifx_restore(&self, schema: &str, keep: &ColumnConstraints) -> Vec<String> {
        let keys = keep.primary_key.iter().map(|k| ("PRIMARY KEY", &k.columns, &k.name));
        let uniques = keep.uniques.iter().map(|u| ("UNIQUE", &u.columns, &u.name));
        let mut out: Vec<String> = keys
            .chain(uniques)
            .map(|(word, cols, name)| self.ifx_named(format!("{word} ({})", self.idents(cols)), name))
            .collect();
        out.extend(keep.checks.iter().map(|c| self.ifx_check(c)));
        out.extend(keep.foreign_keys.iter().map(|f| self.ifx_fk(schema, f)));
        out
    }

    /// 真的要 `MODIFY` 的列（只改注释的不算）。
    fn ifx_modified<'a>(
        &self,
        changes: &'a [C],
    ) -> impl Iterator<Item = (&'a Column, &'a ColumnConstraints)> {
        let modified: Vec<_> = changes
            .iter()
            .filter_map(|c| match c {
                C::AlterColumn { from, to, keep } if self.ifx_column(from) != self.ifx_column(to) => {
                    Some((to, keep))
                }
                _ => None,
            })
            .collect();
        modified.into_iter()
    }

    /// 改了主键或唯一列时提示：别的表的外键会先被服务器删掉，再由 `design_restore_keys` 重建。
    pub(crate) fn ifx_warnings(&self, changes: &[C]) -> Vec<Msg> {
        self.ifx_modified(changes)
            .filter(|(_, keep)| keep.primary_key.is_some() || !keep.uniques.is_empty())
            .map(|(to, _)| msg!("design.gbase_key_modify", column = to.name))
            .collect()
    }

    /// `MODIFY` 主键或唯一列时被服务器删掉的、别的表的外键：放在全部语句之后重建。`referencing` 是原表上读到的引用。
    pub fn design_restore_keys(
        &self,
        schema: &str,
        table: &str,
        changes: &[C],
        referencing: &[ReferencingKey],
    ) -> Vec<DesignStatement> {
        if self.family() != Family::Informix {
            return Vec::new();
        }
        let mut out = Vec::new();
        for (to, keep) in self.ifx_modified(changes) {
            let keys =
                keep.primary_key.iter().map(|k| &k.columns).chain(keep.uniques.iter().map(|u| &u.columns));
            for column in keys.filter_map(|cols| cols.first()) {
                for r in
                    referencing.iter().filter(|r| matches!(r.key.ref_columns.as_slice(), [c] if c == column))
                {
                    let key = ForeignKey {
                        ref_schema: Some(schema.to_string()),
                        ref_table: table.to_string(),
                        ref_columns: vec![to.name.clone()],
                        ..r.key.clone()
                    };
                    let other = self.qualified(Some(&r.schema), &r.table);
                    let sql = format!("ALTER TABLE {other} ADD CONSTRAINT {}", self.ifx_fk(&r.schema, &key));
                    out.push(DesignStatement { changes: vec![], sql });
                }
            }
        }
        out
    }

    fn ifx_comment(&self, target: String, comment: &str) -> Result<String> {
        Ok(format!("COMMENT ON {target} IS {}", self.text_literal(comment)?))
    }

    /// 建表语句本身：列和约束，不含索引、注释。
    pub(crate) fn ifx_table(&self, schema: &str, t: &Table) -> String {
        let q = self.qualified(Some(schema), &t.name);
        let mut items: Vec<String> = t.columns.iter().map(|c| self.ifx_column(c)).collect();
        if let Some(pk) = &t.primary_key {
            items.push(self.ifx_named(format!("PRIMARY KEY ({})", self.idents(&pk.columns)), &pk.name));
        }
        for u in &t.uniques {
            items.push(self.ifx_named(format!("UNIQUE ({})", self.idents(&u.columns)), &u.name));
        }
        for f in &t.foreign_keys {
            items.push(self.ifx_fk(schema, f));
        }
        for c in &t.checks {
            items.push(self.ifx_check(c));
        }
        format!("CREATE TABLE {q} (\n    {}\n)", items.join(",\n    "))
    }

    fn ifx_create(&self, schema: &str, t: &Table) -> Result<Vec<String>> {
        let q = self.qualified(Some(schema), &t.name);
        let mut out = vec![self.ifx_table(schema, t)];
        for x in &t.indexes {
            out.push(self.ifx_index(schema, &t.name, x));
        }
        if let Some(c) = text(&t.comment) {
            out.push(self.ifx_comment(format!("TABLE {q}"), c)?);
        }
        for c in &t.columns {
            if let Some(cm) = text(&c.comment) {
                out.push(self.ifx_comment(format!("COLUMN {q}.{}", self.ident(&c.name)), cm)?);
            }
        }
        Ok(out)
    }

    pub(crate) fn informix_design(
        &self,
        schema: &str,
        table: &str,
        changes: &[C],
    ) -> Result<Vec<DesignStatement>> {
        let q = self.qualified(Some(schema), table);
        let alter = |clause: String| format!("ALTER TABLE {q} {clause}");
        let drop_constraint = |name: &Option<String>| -> Result<String> {
            Ok(alter(format!("DROP CONSTRAINT {}", self.ident(self.required_name(name)?))))
        };
        let add = |clause: String| format!("ALTER TABLE {q} ADD CONSTRAINT {clause}");
        let column = |name: &str| format!("COLUMN {q}.{}", self.ident(name));
        let comment_of =
            |name: &str, c: &Option<String>| self.ifx_comment(column(name), text(c).unwrap_or_default());
        let mut out = Vec::new();
        // 合并的列子句（每种只能写一次），和第一条子句所在的位置
        let (mut adds, mut drops, mut modifies) = (Vec::new(), Vec::new(), Vec::new());
        let mut renames: Vec<(&str, &str)> = Vec::new();
        let mut restore: Vec<String> = Vec::new();
        let mut merged_at = None;
        for (i, ch) in changes.iter().enumerate() {
            if matches!(
                ch,
                C::AddColumn { .. } | C::DropColumn { .. } | C::AlterColumn { .. } | C::RenameColumn { .. }
            ) {
                merged_at.get_or_insert((out.len(), i));
            }
            let sqls: Vec<String> = match ch {
                C::CreateTable { table } => self.ifx_create(schema, table)?,
                C::SetTableComment { comment } => {
                    vec![self.ifx_comment(format!("TABLE {q}"), text(comment).unwrap_or_default())?]
                }
                C::AddColumn { column: c, .. } => {
                    adds.push(self.ifx_column(c));
                    if text(&c.comment).is_some() { vec![comment_of(&c.name, &c.comment)?] } else { vec![] }
                }
                C::DropColumn { name } => {
                    drops.push(self.ident(name));
                    vec![]
                }
                C::RenameColumn { from, to, .. } => {
                    renames.push((from, to));
                    vec![]
                }
                C::AlterColumn { from, to, keep } => {
                    let mut v = Vec::new();
                    if self.ifx_column(from) != self.ifx_column(to) {
                        // 改名排在后面，这里还是旧名字
                        let old = renames.iter().find(|(_, new)| *new == to.name).map(|(o, _)| *o);
                        let mut def = to.clone();
                        def.name = old.unwrap_or(&to.name).to_string();
                        modifies.push(self.ifx_column(&def));
                        restore.extend(self.ifx_restore(schema, keep).into_iter().map(add));
                    }
                    if text(&from.comment) != text(&to.comment) {
                        v.push(comment_of(&to.name, &to.comment)?);
                    }
                    v
                }
                // 由 design_unsupported 改为重建，不会走到这里
                C::MoveColumn { .. } => vec![],
                C::DropPrimaryKey { key } => vec![drop_constraint(&key.name)?],
                C::AddPrimaryKey { key } => {
                    vec![add(
                        self.ifx_named(format!("PRIMARY KEY ({})", self.idents(&key.columns)), &key.name)
                    )]
                }
                C::DropUnique { unique } => vec![drop_constraint(&unique.name)?],
                C::AddUnique { unique } => {
                    vec![add(
                        self.ifx_named(format!("UNIQUE ({})", self.idents(&unique.columns)), &unique.name)
                    )]
                }
                C::DropCheck { check } => vec![drop_constraint(&check.name)?],
                C::AddCheck { check } => vec![add(self.ifx_check(check))],
                C::DropForeignKey { key } => vec![drop_constraint(&key.name)?],
                C::AddForeignKey { key } => vec![add(self.ifx_fk(schema, key))],
                C::DropIndex { index } => {
                    vec![format!("DROP INDEX {}", self.qualified(Some(schema), &index.name))]
                }
                C::AddIndex { index } => vec![self.ifx_index(schema, table, index)],
                C::RenameIndex { from, index } => {
                    vec![format!(
                        "RENAME INDEX {} TO {}",
                        self.qualified(Some(schema), from),
                        self.ident(&index.name)
                    )]
                }
            };
            out.extend(sqls.into_iter().map(|s| stmt(i, s)));
        }
        if let Some((at, change)) = merged_at {
            let parts = [("DROP", drops), ("ADD", adds), ("MODIFY", modifies)];
            let clauses: Vec<String> = parts
                .iter()
                .filter(|(_, items)| !items.is_empty())
                .map(|(word, items)| format!("{word} ({})", items.join(", ")))
                .collect();
            let rename = |(from, to): &(&str, &str)| {
                stmt(change, format!("RENAME COLUMN {q}.{} TO {}", self.ident(from), self.ident(to)))
            };
            let merged = (!clauses.is_empty()).then(|| stmt(change, alter(clauses.join(", "))));
            let head: Vec<DesignStatement> = merged
                .into_iter()
                .chain(restore.iter().map(|s| stmt(change, s.clone())))
                .chain(renames.iter().map(rename))
                .collect();
            out.splice(at..at, head);
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::super::sql::fixtures::*;
    use super::*;
    use crate::Variant;
    use crate::design::{Renames, diff_table};

    #[test]
    fn create_puts_constraint_names_after_the_definition() {
        let d = Dialect::new(Variant::Gbase8s);
        let t = table(["bigint", "varchar(20)", "decimal(10,2)"]);
        let diff = diff_table(None, &t, &Renames::default()).unwrap();
        let sql = render(&d.design_sql("admin", "", &diff).unwrap());
        assert!(sql.contains("CREATE TABLE admin.orders"), "{sql}");
        assert!(sql.contains(") CONSTRAINT "), "{sql}");
        assert!(!sql.contains("    CONSTRAINT"), "约束名在定义之后：{sql}");
        assert!(sql.contains("COMMENT ON"), "{sql}");
    }

    #[test]
    fn every_change_generates_a_statement() {
        let d = Dialect::new(Variant::Gbase8s);
        let t = table(["bigint", "varchar(20)", "decimal(10,2)"]);
        let sql = render(&d.design_sql("admin", "orders", &everything(&t, "varchar(50)")).unwrap());
        for piece in [
            "RENAME COLUMN",
            "MODIFY (",
            "ADD (",
            "DROP (",
            "ADD CONSTRAINT",
            "DROP CONSTRAINT",
            "DROP INDEX",
        ] {
            assert!(sql.contains(piece), "{piece}: {sql}");
        }
        assert!(!d.design_transactional());
    }

    #[test]
    fn modify_restores_single_column_constraints() {
        let d = Dialect::new(Variant::Gbase8s);
        let (a, mut a2) = (col("a", "int"), col("a", "bigint"));
        a2.nullable = false;
        let keep = ColumnConstraints {
            primary_key: Some(dblens_core::schema::PrimaryKey {
                name: Some("pk".into()),
                columns: vec!["a".into()],
            }),
            checks: vec![dblens_core::schema::CheckConstraint {
                name: Some("ck".into()),
                expression: "(a > 0)".into(),
            }],
            ..Default::default()
        };
        let changes = [
            C::RenameColumn { from: "a".into(), to: "z".into(), column: a2.clone() },
            C::AlterColumn {
                from: Column { name: "z".into(), ..a },
                to: Column { name: "z".into(), ..a2 },
                keep,
            },
        ];
        let got: Vec<String> =
            d.informix_design("o", "t", &changes).unwrap().into_iter().map(|s| s.sql).collect();
        assert_eq!(
            got,
            [
                "ALTER TABLE o.t MODIFY (a bigint NOT NULL)",
                "ALTER TABLE o.t ADD CONSTRAINT PRIMARY KEY (a) CONSTRAINT pk",
                "ALTER TABLE o.t ADD CONSTRAINT CHECK (a > 0) CONSTRAINT ck",
                "RENAME COLUMN o.t.a TO z",
            ]
        );
        assert_eq!(d.ifx_warnings(&changes).len(), 1);
        let child = ReferencingKey {
            schema: "o".into(),
            table: "c".into(),
            key: ForeignKey {
                name: Some("fk".into()),
                columns: vec!["pid".into()],
                ref_schema: Some("o".into()),
                ref_table: "t".into(),
                ref_columns: vec!["a".into()],
                on_delete: Some("CASCADE".into()),
                on_update: None,
            },
        };
        let other = ReferencingKey {
            key: ForeignKey { ref_columns: vec!["a".into(), "b".into()], ..child.key.clone() },
            ..child.clone()
        };
        let got = d.design_restore_keys("o", "t", &changes, &[child, other]);
        assert_eq!(got.len(), 1, "联合外键不受影响");
        assert_eq!(
            got[0].sql,
            "ALTER TABLE o.c ADD CONSTRAINT FOREIGN KEY (pid) REFERENCES o.t (z) ON DELETE CASCADE CONSTRAINT fk"
        );
    }

    #[test]
    fn column_changes_share_one_alter() {
        let d = Dialect::new(Variant::Gbase8s);
        let (a, b) = (col("a", "int"), col("b", "varchar(10)"));
        let mut b2 = b.clone();
        b2.data_type.native = "varchar(20)".into();
        let mut a2 = a.clone();
        a2.nullable = false;
        let changes = [
            C::AlterColumn { from: a, to: a2, keep: ColumnConstraints::default() },
            C::AlterColumn { from: b, to: b2, keep: ColumnConstraints::default() },
            C::DropColumn { name: "c".into() },
        ];
        let got = d.informix_design("admin", "t", &changes).unwrap();
        assert_eq!(got.len(), 1, "{got:?}");
        assert_eq!(got[0].sql, "ALTER TABLE admin.t DROP (c), MODIFY (a int NOT NULL, b varchar(20))");
    }
}
