//! SQL Server 的设计器 DDL（M6b 设计文档第 3 节）。DDL 可以回滚，全部语句在一个事务中执行。
//! 默认值是有名字的约束：设计器建的命名为 `DF_表_列`；删除时在执行时从 `sys.default_constraints` 查名字（库自动起的名字带随机后缀，
//! 各库不同）。改名用 `sp_rename`，注释是扩展属性 `MS_Description`。

use dblens_core::Result;
use dblens_core::schema::{Column, Index, Table};

use super::TableChange as C;
use super::diff::{same_type, text};
use super::sql::{DesignStatement, stmt};
use crate::Dialect;

/// 约束名的最大长度。
const MAX_NAME: usize = 128;

/// 设计器建的默认值约束的名字：`DF_表_列`，超长时截断。
pub(crate) fn df_name(table: &str, column: &str) -> String {
    format!("DF_{table}_{column}").chars().take(MAX_NAME).collect()
}

/// 计算列的表达式去掉一层外括号（库中读出的是 `([a]+[b])`，写回时再加一层）。
fn strip_outer(e: &str) -> &str {
    let e = e.trim();
    match e.strip_prefix('(').and_then(|s| s.strip_suffix(')')) {
        Some(inner) if balanced(inner) => inner,
        _ => e,
    }
}

fn balanced(s: &str) -> bool {
    let mut depth = 0i32;
    for ch in s.chars() {
        match ch {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth < 0 {
                    return false;
                }
            }
            _ => {}
        }
    }
    depth == 0
}

impl Dialect {
    /// 列定义：计算列写 `名 AS (表达式)`；否则类型、自增、带名字的默认值、可空性（总是写明，不依赖会话的 ANSI_NULL_DFLT 设置）。
    fn ms_column(&self, table: &str, c: &Column) -> String {
        let name = self.ident(&c.name);
        if let Some(g) = text(&c.generated) {
            return format!("{name} AS ({})", strip_outer(g));
        }
        let mut s = format!("{name} {}", c.data_type.native);
        if c.auto_increment {
            s += " IDENTITY(1,1)";
        } else if let Some(d) = text(&c.default) {
            s += &format!(" CONSTRAINT {} DEFAULT {d}", self.ident(&df_name(table, &c.name)));
        }
        s += if c.nullable { " NULL" } else { " NOT NULL" };
        s
    }

    /// 删除列上的默认值约束（没有时什么也不做）：执行时按列查出约束名，拼成语句后用 `sp_executesql` 执行
    /// （`EXEC (…)` 的括号里只能拼字面量和变量，不能调用 `QUOTENAME`）。
    fn ms_drop_default(&self, schema: &str, table: &str, column: &str) -> Result<String> {
        let q = self.qualified(Some(schema), table);
        Ok(format!(
            "DECLARE @df sysname = (SELECT dc.name FROM sys.default_constraints dc \
             JOIN sys.columns c ON c.object_id = dc.parent_object_id AND c.column_id = dc.parent_column_id \
             WHERE dc.parent_object_id = OBJECT_ID({}) AND c.name = {});\n\
             IF @df IS NOT NULL\nBEGIN\n    \
             DECLARE @ddl nvarchar(max) = {} + QUOTENAME(@df);\n    \
             EXEC sp_executesql @ddl;\nEND;",
            self.text_literal(&q)?,
            self.text_literal(column)?,
            self.text_literal(&format!("ALTER TABLE {q} DROP CONSTRAINT "))?,
        ))
    }

    /// 表或列的注释（扩展属性 `MS_Description`）：有注释时已存在则更新、不存在则添加；没有注释时存在则删除。
    fn ms_comment(
        &self,
        schema: &str,
        table: &str,
        column: Option<&str>,
        comment: Option<&str>,
    ) -> Result<String> {
        let object = self.text_literal(&self.qualified(Some(schema), table))?;
        let minor = match column {
            Some(c) => format!("COLUMNPROPERTY(OBJECT_ID({object}), {}, 'ColumnId')", self.text_literal(c)?),
            None => "0".into(),
        };
        let mut target =
            format!("N'SCHEMA', {}, N'TABLE', {}", self.text_literal(schema)?, self.text_literal(table)?);
        if let Some(c) = column {
            target += &format!(", N'COLUMN', {}", self.text_literal(c)?);
        }
        let exists = format!(
            "IF EXISTS (SELECT 1 FROM sys.extended_properties WHERE class = 1 AND major_id = OBJECT_ID({object}) \
             AND minor_id = {minor} AND name = N'MS_Description')"
        );
        Ok(match comment {
            Some(text) => {
                let value = self.text_literal(text)?;
                format!(
                    "{exists}\n    EXEC sys.sp_updateextendedproperty N'MS_Description', {value}, {target};\n\
                     ELSE\n    EXEC sys.sp_addextendedproperty N'MS_Description', {value}, {target};"
                )
            }
            None => format!("{exists}\n    EXEC sys.sp_dropextendedproperty N'MS_Description', {target};"),
        })
    }

    /// 新建对象上的注释：一定不存在，直接添加。
    fn ms_add_comment(
        &self,
        schema: &str,
        table: &str,
        column: Option<&str>,
        comment: &str,
    ) -> Result<String> {
        let mut s = format!(
            "EXEC sys.sp_addextendedproperty N'MS_Description', {}, N'SCHEMA', {}, N'TABLE', {}",
            self.text_literal(comment)?,
            self.text_literal(schema)?,
            self.text_literal(table)?
        );
        if let Some(c) = column {
            s += &format!(", N'COLUMN', {}", self.text_literal(c)?);
        }
        Ok(s)
    }

    /// `sp_rename`：`what` 为 `COLUMN` 或 `INDEX`，`object` 是 `[架构].[表].[名]`。
    fn ms_rename(&self, object: &str, to: &str, what: &str) -> Result<String> {
        Ok(format!("EXEC sp_rename {}, {}, N'{what}'", self.text_literal(object)?, self.text_literal(to)?))
    }

    fn ms_index(&self, schema: &str, table: &str, x: &Index) -> String {
        let method = x
            .method
            .as_deref()
            .map(str::to_ascii_uppercase)
            .filter(|m| m == "CLUSTERED" || m == "NONCLUSTERED")
            .map(|m| format!("{m} "))
            .unwrap_or_default();
        format!(
            "CREATE {}{method}INDEX {} ON {} ({})",
            if x.unique { "UNIQUE " } else { "" },
            self.ident(&x.name),
            self.qualified(Some(schema), table),
            self.index_items(x)
        )
    }

    fn ms_create(&self, schema: &str, t: &Table) -> Result<Vec<String>> {
        let q = self.qualified(Some(schema), &t.name);
        let mut items: Vec<String> = t.columns.iter().map(|c| self.ms_column(&t.name, c)).collect();
        if let Some(pk) = &t.primary_key {
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
            items.push(format!("{}{}", self.constraint_name(&f.name), self.fk_clause(schema, f)));
        }
        for c in &t.checks {
            items.push(self.check_clause(c));
        }
        let mut out = vec![format!("CREATE TABLE {q} (\n    {}\n)", items.join(",\n    "))];
        for x in &t.indexes {
            out.push(self.ms_index(schema, &t.name, x));
        }
        if let Some(c) = text(&t.comment) {
            out.push(self.ms_add_comment(schema, &t.name, None, c)?);
        }
        for c in &t.columns {
            if let Some(cm) = text(&c.comment) {
                out.push(self.ms_add_comment(schema, &t.name, Some(&c.name), cm)?);
            }
        }
        Ok(out)
    }

    /// 修改已有列。自增和计算列的修改由 `design_unsupported` 改为重建。
    fn ms_alter_column(&self, schema: &str, table: &str, from: &Column, to: &Column) -> Result<Vec<String>> {
        let q = self.qualified(Some(schema), table);
        let col = self.ident(&to.name);
        let mut out = Vec::new();
        if !same_type(&from.data_type.native, &to.data_type.native) || from.nullable != to.nullable {
            // 可空性必须写明，否则按会话设置可能变成可空
            out.push(format!(
                "ALTER TABLE {q} ALTER COLUMN {col} {}{}",
                to.data_type.native,
                if to.nullable { " NULL" } else { " NOT NULL" }
            ));
        }
        if text(&from.default) != text(&to.default) && to.generated.is_none() && !to.auto_increment {
            if text(&from.default).is_some() {
                out.push(self.ms_drop_default(schema, table, &to.name)?);
            }
            if let Some(d) = text(&to.default) {
                out.push(format!(
                    "ALTER TABLE {q} ADD CONSTRAINT {} DEFAULT {d} FOR {col}",
                    self.ident(&df_name(table, &to.name))
                ));
            }
        }
        if text(&from.comment) != text(&to.comment) {
            out.push(self.ms_comment(schema, table, Some(&to.name), text(&to.comment))?);
        }
        Ok(out)
    }

    pub(crate) fn sqlserver_design(
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
        let member = |name: &str| format!("{q}.{}", self.ident(name));
        let mut out = Vec::new();
        for (i, ch) in changes.iter().enumerate() {
            let sqls: Vec<String> = match ch {
                C::CreateTable { table } => self.ms_create(schema, table)?,
                C::SetTableComment { comment } => {
                    vec![self.ms_comment(schema, table, None, text(comment))?]
                }
                C::AddColumn { column, .. } => {
                    let mut v = vec![alter(format!("ADD {}", self.ms_column(table, column)))];
                    if let Some(cm) = text(&column.comment) {
                        v.push(self.ms_add_comment(schema, table, Some(&column.name), cm)?);
                    }
                    v
                }
                C::DropColumn { name } => {
                    vec![
                        self.ms_drop_default(schema, table, name)?,
                        alter(format!("DROP COLUMN {}", self.ident(name))),
                    ]
                }
                C::RenameColumn { from, to, .. } => vec![self.ms_rename(&member(from), to, "COLUMN")?],
                C::AlterColumn { from, to, .. } => self.ms_alter_column(schema, table, from, to)?,
                // 由 design_unsupported 改为重建，不会走到这里
                C::MoveColumn { .. } => vec![],
                C::DropPrimaryKey { key } => vec![drop_constraint(&key.name)?],
                C::AddPrimaryKey { key } => vec![alter(format!(
                    "ADD {}PRIMARY KEY ({})",
                    self.constraint_name(&key.name),
                    self.idents(&key.columns)
                ))],
                C::DropUnique { unique } => vec![drop_constraint(&unique.name)?],
                C::AddUnique { unique } => vec![alter(format!(
                    "ADD {}UNIQUE ({})",
                    self.constraint_name(&unique.name),
                    self.idents(&unique.columns)
                ))],
                C::DropCheck { check } => vec![drop_constraint(&check.name)?],
                C::AddCheck { check } => vec![alter(format!("ADD {}", self.check_clause(check)))],
                C::DropForeignKey { key } => vec![drop_constraint(&key.name)?],
                C::AddForeignKey { key } => {
                    vec![alter(format!(
                        "ADD {}{}",
                        self.constraint_name(&key.name),
                        self.fk_clause(schema, key)
                    ))]
                }
                C::DropIndex { index } => vec![format!("DROP INDEX {} ON {q}", self.ident(&index.name))],
                C::AddIndex { index } => vec![self.ms_index(schema, table, index)],
                C::RenameIndex { from, index } => {
                    vec![self.ms_rename(&member(from), &index.name, "INDEX")?]
                }
            };
            out.extend(sqls.into_iter().map(|s| stmt(i, s)));
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

    fn appended(mut diff: crate::design::Diff) -> crate::design::Diff {
        // 新列插在中间会改为重建；这里把 note 放到最后，覆盖其余每种变更
        for c in &mut diff.changes {
            if let C::AddColumn { after, .. } = c {
                *after = Some("order_code".into());
            }
        }
        diff
    }

    #[test]
    fn sqlserver() {
        let d = Dialect::new(Variant::SqlServer);
        let t = table(["bigint", "nvarchar(20)", "decimal(10,2)"]);
        let diff = diff_table(None, &t, &Renames::default()).unwrap();
        insta::assert_snapshot!("sqlserver_create", render(&d.design_sql("dbo", "", &diff).unwrap()));
        let diff = appended(everything(&t, "nvarchar(50)"));
        insta::assert_snapshot!(
            "sqlserver_everything",
            render(&d.design_sql("dbo", "orders", &diff).unwrap())
        );
        insta::assert_snapshot!(
            "sqlserver_alter",
            render(&d.design_sql("dbo", "orders", &alter_only(&t, "decimal(12,2)")).unwrap())
        );
        assert!(d.design_transactional());
    }

    #[test]
    fn names_and_expressions() {
        assert_eq!(df_name("orders", "amount"), "DF_orders_amount");
        assert_eq!(df_name(&"t".repeat(200), "c").chars().count(), MAX_NAME);
        assert_eq!(strip_outer("([a]+[b])"), "[a]+[b]");
        assert_eq!(strip_outer("(a)+(b)"), "(a)+(b)");
        let d = Dialect::new(Variant::SqlServer);
        let mut c = col("total", "int");
        c.generated = Some("([a]*(2))".into());
        assert_eq!(d.ms_column("t", &c), "total AS ([a]*(2))");
        // 名字中的单引号在字符串字面量中转义
        let s = d.ms_drop_default("dbo", "o'k", "c").unwrap();
        assert!(s.contains("OBJECT_ID(N'dbo.[o''k]')"), "{s}");
        assert!(
            s.contains(
                "DECLARE @ddl nvarchar(max) = N'ALTER TABLE dbo.[o''k] DROP CONSTRAINT ' + QUOTENAME(@df);"
            ),
            "{s}"
        );
    }
}
