//! 由表模型生成 SELECT/INSERT/UPDATE/DELETE（"生成 SQL"菜单）。参数一律用占位符。

use dblens_core::schema::{Column, Table};

use crate::Dialect;

impl Dialect {
    /// 每列一行，列注释以 `-- 注释` 的形式附在行末。
    pub fn select_sql(&self, schema: Option<&str>, table: &Table) -> String {
        let from = self.qualified(schema, &table.name);
        if table.columns.is_empty() {
            return format!("SELECT *\nFROM {from}");
        }
        let last = table.columns.len() - 1;
        let lines: Vec<String> = table
            .columns
            .iter()
            .enumerate()
            .map(|(k, c)| {
                let sep = if k == last { "" } else { "," };
                let mut line = format!("    {}{sep}", self.ident(&c.name));
                if let Some(comment) = c.comment.as_deref().filter(|s| !s.trim().is_empty()) {
                    line.push_str(" -- ");
                    line.push_str(&comment.replace(['\r', '\n'], " ").replace("  ", " "));
                }
                line
            })
            .collect();
        format!("SELECT\n{}\nFROM {from}", lines.join("\n"))
    }

    /// 跳过自增列和生成列。
    pub fn insert_sql(&self, schema: Option<&str>, table: &Table) -> String {
        let cols: Vec<&Column> =
            table.columns.iter().filter(|c| !c.auto_increment && c.generated.is_none()).collect();
        let names: Vec<String> = cols.iter().map(|c| self.ident(&c.name)).collect();
        let marks: Vec<String> = (1..=cols.len()).map(|k| self.placeholder(k)).collect();
        format!(
            "INSERT INTO {} ({}) VALUES ({})",
            self.qualified(schema, &table.name),
            names.join(", "),
            marks.join(", ")
        )
    }

    /// 按主键更新其余列；没有主键或没有可更新的列时返回 None。
    pub fn update_sql(&self, schema: Option<&str>, table: &Table) -> Option<String> {
        let pk = primary_key(table)?;
        let set_cols: Vec<&Column> = table
            .columns
            .iter()
            .filter(|c| c.generated.is_none() && !pk.contains(&c.name.as_str()))
            .collect();
        if set_cols.is_empty() {
            return None;
        }
        let mut k = 0;
        let mut next = || {
            k += 1;
            self.placeholder(k)
        };
        let sets: Vec<String> =
            set_cols.iter().map(|c| format!("{} = {}", self.ident(&c.name), next())).collect();
        let wheres: Vec<String> = pk.iter().map(|c| format!("{} = {}", self.ident(c), next())).collect();
        Some(format!(
            "UPDATE {} SET {} WHERE {}",
            self.qualified(schema, &table.name),
            sets.join(", "),
            wheres.join(" AND ")
        ))
    }

    /// 按主键删除；没有主键时返回 None。
    pub fn delete_sql(&self, schema: Option<&str>, table: &Table) -> Option<String> {
        let pk = primary_key(table)?;
        let wheres: Vec<String> = pk
            .iter()
            .enumerate()
            .map(|(k, c)| format!("{} = {}", self.ident(c), self.placeholder(k + 1)))
            .collect();
        Some(format!("DELETE FROM {} WHERE {}", self.qualified(schema, &table.name), wheres.join(" AND ")))
    }
}

fn primary_key(table: &Table) -> Option<Vec<&str>> {
    let pk = table.primary_key.as_ref()?;
    (!pk.columns.is_empty()).then(|| pk.columns.iter().map(String::as_str).collect())
}

#[cfg(test)]
mod tests {
    use dblens_core::schema::{Column, PrimaryKey, Table};
    use dblens_core::{ColumnType, LogicalType};
    use dblens_driver::{DriverId, ServerInfo};

    use crate::{Dialect, Family, Variant};

    fn col(name: &str, comment: Option<&str>) -> Column {
        Column {
            name: name.into(),
            ordinal: 0,
            data_type: ColumnType {
                native: "VARCHAR(10)".into(),
                logical: LogicalType::VarChar { len: Some(10) },
            },
            nullable: true,
            default: None,
            comment: comment.map(Into::into),
            auto_increment: false,
            generated: None,
        }
    }

    /// person 表：id（自增主键，注释"主键"）、name（注释含换行）、note、full_name（生成列）。
    fn person(upper: bool) -> Table {
        let n = |s: &str| if upper { s.to_ascii_uppercase() } else { s.to_string() };
        let mut id = col(&n("id"), Some("主键"));
        id.auto_increment = true;
        let mut full = col(&n("full_name"), None);
        full.generated = Some("name || note".into());
        Table {
            name: n("person"),
            columns: vec![id, col(&n("name"), Some("姓名\n全称")), col(&n("note"), None), full],
            primary_key: Some(PrimaryKey { name: None, columns: vec![n("id")] }),
            ..Default::default()
        }
    }

    #[test]
    fn select_lists_columns_with_comments() {
        let d = Dialect::new(Variant::MySql);
        assert_eq!(
            d.select_sql(Some("hr"), &person(false)),
            "SELECT\n    id, -- 主键\n    name, -- 姓名 全称\n    note,\n    full_name\nFROM hr.person"
        );
        let empty = Table { name: "t".into(), ..Default::default() };
        assert_eq!(d.select_sql(None, &empty), "SELECT *\nFROM t");
    }

    #[test]
    fn identifiers_are_quoted_when_needed() {
        let pg = Dialect::new(Variant::Postgres);
        assert!(pg.select_sql(Some("HR"), &person(false)).ends_with("FROM \"HR\".person"));
        let ora = Dialect::new(Variant::Oracle);
        assert!(ora.select_sql(Some("HR"), &person(true)).ends_with("FROM HR.PERSON"));
        assert!(ora.select_sql(None, &person(false)).ends_with("FROM \"person\""));
    }

    #[test]
    fn insert_skips_auto_increment_and_generated() {
        assert_eq!(
            Dialect::new(Variant::MySql).insert_sql(Some("hr"), &person(false)),
            "INSERT INTO hr.person (name, note) VALUES (?, ?)"
        );
        assert_eq!(
            Dialect::new(Variant::Postgres).insert_sql(Some("public"), &person(false)),
            "INSERT INTO \"public\".person (name, note) VALUES ($1, $2)"
        );
    }

    #[test]
    fn update_and_delete_use_primary_key() {
        assert_eq!(
            Dialect::new(Variant::Postgres).update_sql(None, &person(false)).unwrap(),
            "UPDATE person SET name = $1, note = $2 WHERE id = $3"
        );
        assert_eq!(
            Dialect::new(Variant::Oracle).update_sql(Some("HR"), &person(true)).unwrap(),
            "UPDATE HR.PERSON SET NAME = :1, NOTE = :2 WHERE ID = :3"
        );
        assert_eq!(
            Dialect::new(Variant::Dameng).delete_sql(Some("HR"), &person(true)).unwrap(),
            "DELETE FROM HR.PERSON WHERE ID = ?"
        );
        let mut no_pk = person(false);
        no_pk.primary_key = None;
        assert!(Dialect::new(Variant::MySql).update_sql(None, &no_pk).is_none());
        assert!(Dialect::new(Variant::MySql).delete_sql(None, &no_pk).is_none());
    }

    #[test]
    fn composite_primary_key() {
        let mut t = person(false);
        t.primary_key = Some(PrimaryKey { name: None, columns: vec!["id".into(), "name".into()] });
        assert_eq!(
            Dialect::new(Variant::MySql).delete_sql(None, &t).unwrap(),
            "DELETE FROM person WHERE id = ? AND name = ?"
        );
        assert_eq!(
            Dialect::new(Variant::MySql).update_sql(None, &t).unwrap(),
            "UPDATE person SET note = ? WHERE id = ? AND name = ?"
        );
    }

    #[test]
    fn dialect_delegates() {
        let d = Dialect::detect(
            DriverId::Postgres,
            &ServerInfo { product: "PostgreSQL".into(), version: "KingbaseES V008R006".into() },
        );
        assert_eq!((d.variant(), d.family()), (Variant::Kingbase, Family::Postgres));
        assert_eq!(Dialect::for_driver(DriverId::Dameng).variant(), Variant::Dameng);
        assert_eq!(d.placeholder(2), "$2");
        assert_eq!(d.split("SELECT 1; SELECT 2").len(), 2);
        assert_eq!(d.native_type(&d.logical_type("int4")), "INTEGER");
        assert_eq!(d.qualified(Some("s"), "T"), "s.\"T\"");
        assert!(d.paginate("SELECT 1", 0, 10).contains("LIMIT 10"));
        assert_eq!(d.literal(&dblens_core::Value::Int(1)).unwrap(), "1");
    }
}
