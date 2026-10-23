//! 由表模型生成 SELECT/INSERT/UPDATE/DELETE（"生成 SQL"菜单）。参数一律用占位符。

use dblens_core::schema::{Column, Table};
use serde::Serialize;

use crate::{Dialect, Family};

/// 带占位符的 DML 语句。`params` 按占位符顺序给出每个占位符绑定的列名。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DmlTemplate {
    pub sql: String,
    pub params: Vec<String>,
}

impl Dialect {
    /// 每列一行，列注释以 `-- 注释` 的形式附在行末（注释里的空白和换行折叠成单个空格）。
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
                let comment =
                    c.comment.as_deref().map(|s| s.split_whitespace().collect::<Vec<_>>().join(" "));
                if let Some(comment) = comment.filter(|s| !s.is_empty()) {
                    line.push_str(" -- ");
                    line.push_str(&comment);
                }
                line
            })
            .collect();
        format!("SELECT\n{}\nFROM {from}", lines.join("\n"))
    }

    /// 跳过自增列和生成列。没有可插入的列时，MySQL 用 `() VALUES ()`，PG 用 `DEFAULT VALUES`，
    /// Oracle/达梦对第一列写 `DEFAULT`；表一列都没有时只返回一行注释。此时 `params` 为空。
    pub fn insert_sql(&self, schema: Option<&str>, table: &Table) -> DmlTemplate {
        let target = self.qualified(schema, &table.name);
        let cols: Vec<&Column> =
            table.columns.iter().filter(|c| !c.auto_increment && c.generated.is_none()).collect();
        let sql = if cols.is_empty() {
            match (self.family(), table.columns.first()) {
                (Family::MySql, _) => format!("INSERT INTO {target} () VALUES ()"),
                (Family::Postgres, _) => format!("INSERT INTO {target} DEFAULT VALUES"),
                (Family::Oracle | Family::Dameng, Some(first)) => {
                    format!("INSERT INTO {target} ({}) VALUES (DEFAULT)", self.ident(&first.name))
                }
                (Family::Oracle | Family::Dameng, None) => format!("-- 没有可插入的列：{target}"),
            }
        } else {
            let names: Vec<String> = cols.iter().map(|c| self.ident(&c.name)).collect();
            let marks: Vec<String> = (1..=cols.len()).map(|k| self.placeholder(k)).collect();
            format!("INSERT INTO {target} ({}) VALUES ({})", names.join(", "), marks.join(", "))
        };
        DmlTemplate { sql, params: cols.iter().map(|c| c.name.clone()).collect() }
    }

    /// 按主键更新其余列（不含主键列、生成列和自增列）。
    /// 没有主键、主键列在表中找不到或没有可更新的列时返回 None。
    pub fn update_sql(&self, schema: Option<&str>, table: &Table) -> Option<DmlTemplate> {
        let pk = primary_key(table)?;
        let set_cols: Vec<&Column> = table
            .columns
            .iter()
            .filter(|c| c.generated.is_none() && !c.auto_increment && !pk.iter().any(|k| k.name == c.name))
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
        let wheres: Vec<String> =
            pk.iter().map(|c| format!("{} = {}", self.ident(&c.name), next())).collect();
        let sql = format!(
            "UPDATE {} SET {} WHERE {}",
            self.qualified(schema, &table.name),
            sets.join(", "),
            wheres.join(" AND ")
        );
        let params = set_cols.iter().chain(pk.iter()).map(|c| c.name.clone()).collect();
        Some(DmlTemplate { sql, params })
    }

    /// 按主键删除；没有主键或主键列在表中找不到时返回 None。
    pub fn delete_sql(&self, schema: Option<&str>, table: &Table) -> Option<DmlTemplate> {
        let pk = primary_key(table)?;
        let wheres: Vec<String> = pk
            .iter()
            .enumerate()
            .map(|(k, c)| format!("{} = {}", self.ident(&c.name), self.placeholder(k + 1)))
            .collect();
        Some(DmlTemplate {
            sql: format!(
                "DELETE FROM {} WHERE {}",
                self.qualified(schema, &table.name),
                wheres.join(" AND ")
            ),
            params: pk.iter().map(|c| c.name.clone()).collect(),
        })
    }
}

/// 主键列对应的表列（顺序同主键定义，重复的只保留第一个）。
/// 没有主键，或任一主键列在 `table.columns` 中找不到（名称必须完全一致）时返回 None。
fn primary_key(table: &Table) -> Option<Vec<&Column>> {
    let pk = table.primary_key.as_ref()?;
    let mut cols: Vec<&Column> = Vec::new();
    for name in &pk.columns {
        let col = table.columns.iter().find(|c| &c.name == name)?;
        if !cols.iter().any(|c| c.name == col.name) {
            cols.push(col);
        }
    }
    (!cols.is_empty()).then_some(cols)
}

#[cfg(test)]
mod tests {
    use dblens_core::schema::{Column, PrimaryKey, Table};
    use dblens_core::{ColumnType, LogicalType};
    use dblens_driver::{DriverId, ServerInfo};

    use super::DmlTemplate;
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

    fn auto(name: &str) -> Column {
        Column { auto_increment: true, ..col(name, None) }
    }

    fn table(name: &str, columns: Vec<Column>, pk: &[&str]) -> Table {
        Table {
            name: name.into(),
            columns,
            primary_key: Some(PrimaryKey { name: None, columns: pk.iter().map(|s| s.to_string()).collect() }),
            ..Default::default()
        }
    }

    fn strs(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    const VARIANTS: [Variant; 4] = [Variant::MySql, Variant::Postgres, Variant::Oracle, Variant::Dameng];

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
    fn select_comment_whitespace_is_collapsed() {
        let t = table(
            "t",
            vec![
                col("a", Some("x \t y\r\n\n  z")),
                col("b", Some(" \n\t ")),
                col("c", Some("")),
                col("d", Some("  头尾  ")),
            ],
            &[],
        );
        assert_eq!(
            Dialect::new(Variant::MySql).select_sql(None, &t),
            "SELECT\n    a, -- x y z\n    b,\n    c,\n    d -- 头尾\nFROM t"
        );
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
    fn empty_schema_is_like_none() {
        for v in VARIANTS {
            let d = Dialect::new(v);
            assert_eq!(d.qualified(Some(""), "T"), d.qualified(None, "T"), "{v:?}");
            assert_eq!(d.insert_sql(Some(""), &person(true)), d.insert_sql(None, &person(true)), "{v:?}");
            assert_eq!(d.delete_sql(Some(""), &person(true)), d.delete_sql(None, &person(true)), "{v:?}");
        }
        assert_eq!(Dialect::new(Variant::MySql).qualified(Some(""), "order"), "`order`");
    }

    #[test]
    fn insert_skips_auto_increment_and_generated() {
        assert_eq!(
            Dialect::new(Variant::MySql).insert_sql(Some("hr"), &person(false)),
            DmlTemplate {
                sql: "INSERT INTO hr.person (name, note) VALUES (?, ?)".into(),
                params: strs(&["name", "note"])
            }
        );
        // public 在 PG 中不是保留字，不加引号
        assert_eq!(
            Dialect::new(Variant::Postgres).insert_sql(Some("public"), &person(false)).sql,
            "INSERT INTO public.person (name, note) VALUES ($1, $2)"
        );
    }

    #[test]
    fn insert_with_nothing_insertable() {
        let only_auto = table("t", vec![auto("id")], &[]);
        let mut gen_col = col("g", None);
        gen_col.generated = Some("1".into());
        let auto_and_gen = table("t", vec![auto("id"), gen_col], &[]);
        let none = table("t", vec![], &[]);
        let sql = |v: Variant, t: &Table| Dialect::new(v).insert_sql(None, t);
        for t in [&only_auto, &auto_and_gen] {
            assert_eq!(sql(Variant::MySql, t).sql, "INSERT INTO t () VALUES ()");
            assert_eq!(sql(Variant::Postgres, t).sql, "INSERT INTO t DEFAULT VALUES");
            assert_eq!(sql(Variant::Oracle, t).sql, "INSERT INTO \"t\" (\"id\") VALUES (DEFAULT)");
            assert_eq!(sql(Variant::Dameng, t).sql, "INSERT INTO \"t\" (\"id\") VALUES (DEFAULT)");
            for v in VARIANTS {
                assert!(sql(v, t).params.is_empty(), "{v:?}");
            }
        }
        // 一列都没有
        assert_eq!(sql(Variant::MySql, &none).sql, "INSERT INTO t () VALUES ()");
        assert_eq!(sql(Variant::Postgres, &none).sql, "INSERT INTO t DEFAULT VALUES");
        let none_upper = Table { name: "T".into(), ..Default::default() };
        for v in [Variant::Oracle, Variant::Dameng] {
            let out = Dialect::new(v).insert_sql(Some("HR"), &none_upper);
            assert_eq!(out.sql, "-- 没有可插入的列：HR.T");
            assert!(out.params.is_empty());
        }
    }

    #[test]
    fn update_and_delete_use_primary_key() {
        assert_eq!(
            Dialect::new(Variant::Postgres).update_sql(None, &person(false)).unwrap().sql,
            "UPDATE person SET name = $1, note = $2 WHERE id = $3"
        );
        assert_eq!(
            Dialect::new(Variant::Oracle).update_sql(Some("HR"), &person(true)).unwrap().sql,
            "UPDATE HR.PERSON SET NAME = :1, NOTE = :2 WHERE ID = :3"
        );
        assert_eq!(
            Dialect::new(Variant::Dameng).delete_sql(Some("HR"), &person(true)).unwrap(),
            DmlTemplate { sql: "DELETE FROM HR.PERSON WHERE ID = ?".into(), params: strs(&["ID"]) }
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
        let d = Dialect::new(Variant::MySql);
        assert_eq!(d.delete_sql(None, &t).unwrap().sql, "DELETE FROM person WHERE id = ? AND name = ?");
        assert_eq!(
            d.update_sql(None, &t).unwrap().sql,
            "UPDATE person SET note = ? WHERE id = ? AND name = ?"
        );
    }

    #[test]
    fn update_params_follow_placeholder_order_on_all_families() {
        // 复合主键按主键定义的顺序（b, a），不是表中的列序
        let build = |up: bool| {
            let n = |s: &str| if up { s.to_ascii_uppercase() } else { s.to_string() };
            let cols = vec![col(&n("a"), None), col(&n("x"), None), col(&n("b"), None), col(&n("y"), None)];
            table(&n("t"), cols, &[&n("b"), &n("a")])
        };
        let cases = [
            (Variant::MySql, false, "UPDATE t SET x = ?, y = ? WHERE b = ? AND a = ?", ["x", "y", "b", "a"]),
            (
                Variant::Postgres,
                false,
                "UPDATE t SET x = $1, y = $2 WHERE b = $3 AND a = $4",
                ["x", "y", "b", "a"],
            ),
            (
                Variant::Oracle,
                true,
                "UPDATE T SET X = :1, Y = :2 WHERE B = :3 AND A = :4",
                ["X", "Y", "B", "A"],
            ),
            (Variant::Dameng, true, "UPDATE T SET X = ?, Y = ? WHERE B = ? AND A = ?", ["X", "Y", "B", "A"]),
        ];
        for (v, up, sql, params) in cases {
            let d = Dialect::new(v);
            let t = build(up);
            let out = d.update_sql(None, &t).unwrap();
            assert_eq!((out.sql.as_str(), out.params), (sql, strs(&params)), "{v:?}");
            let del = d.delete_sql(None, &t).unwrap();
            assert_eq!(del.params, strs(&[params[2], params[3]]), "{v:?}");
        }
    }

    #[test]
    fn missing_key_column_yields_none() {
        let cols = || vec![col("id", None), col("name", None)];
        for pk in [&["nope"][..], &["id", "nope"], &["ID"]] {
            let t = table("t", cols(), pk);
            for v in VARIANTS {
                let d = Dialect::new(v);
                assert!(d.update_sql(None, &t).is_none(), "{v:?} {pk:?}");
                assert!(d.delete_sql(None, &t).is_none(), "{v:?} {pk:?}");
            }
        }
        // 主键列表为空也没有主键
        let t = table("t", cols(), &[]);
        assert!(Dialect::new(Variant::MySql).delete_sql(None, &t).is_none());
    }

    #[test]
    fn duplicate_key_columns_are_deduplicated() {
        let t = table("t", vec![col("id", None), col("v", None), col("k", None)], &["id", "k", "id"]);
        let d = Dialect::new(Variant::Postgres);
        let up = d.update_sql(None, &t).unwrap();
        assert_eq!(
            (up.sql.as_str(), up.params),
            ("UPDATE t SET v = $1 WHERE id = $2 AND k = $3", strs(&["v", "id", "k"]))
        );
        let del = d.delete_sql(None, &t).unwrap();
        assert_eq!(
            (del.sql.as_str(), del.params),
            ("DELETE FROM t WHERE id = $1 AND k = $2", strs(&["id", "k"]))
        );
    }

    #[test]
    fn update_set_excludes_auto_increment() {
        // 自增列不是主键，也不能出现在 SET 里
        let t = table("t", vec![col("code", None), auto("seq"), col("name", None)], &["code"]);
        let out = Dialect::new(Variant::MySql).update_sql(None, &t).unwrap();
        assert_eq!(
            (out.sql.as_str(), out.params),
            ("UPDATE t SET name = ? WHERE code = ?", strs(&["name", "code"]))
        );
        // 除主键外只剩自增列和生成列时没有可更新的列
        let only = table("t", vec![col("code", None), auto("seq")], &["code"]);
        assert!(Dialect::new(Variant::MySql).update_sql(None, &only).is_none());
        assert!(Dialect::new(Variant::MySql).delete_sql(None, &only).is_some());
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
