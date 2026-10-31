//! 导入（M2b 设计文档 5.1 节）：由若干行生成写入语句（带占位符和参数），以及新建表的建表语句。
//! 一批行按每条语句至多 500 行、30000 个参数拆成多条。

use std::ops::Range;

use dblens_core::schema::{Column, Table};
use dblens_core::{Error, LogicalType, Result, Value, msg};
use serde::{Deserialize, Serialize};

use crate::{Dialect, Family, Variant};

/// 每条语句至多包含的行数。
pub const MAX_ROWS_PER_STATEMENT: usize = 500;
/// 每条语句至多包含的参数个数。
pub const MAX_PARAMS_PER_STATEMENT: usize = 30000;

/// 写入方式。清空后导入（`Replace`）的写入语句与追加相同，清空另用 `delete_all_sql`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WriteMode {
    Append,
    Replace,
    /// 按主键更新：存在则更新，不存在则插入。
    Upsert,
}

/// 一条写入语句。`rows` 是它包含的行在输入中的下标范围。
#[derive(Debug, Clone, PartialEq)]
pub struct ImportStatement {
    pub sql: String,
    pub params: Vec<Value>,
    pub rows: Range<usize>,
}

impl Dialect {
    /// 清空表：`DELETE FROM 表`。不用 TRUNCATE：它在 MySQL、Oracle 中隐式提交，不能回滚。
    pub fn delete_all_sql(&self, schema: &str, table: &str) -> String {
        format!("DELETE FROM {}", self.qualified(Some(schema), table))
    }

    /// 每条写入语句至多包含的参数个数：SQL Server 一次请求最多 2100 个参数，留些余量取 2000。
    pub fn max_params(&self) -> usize {
        if self.family() == Family::SqlServer { 2000 } else { MAX_PARAMS_PER_STATEMENT }
    }

    /// 新建表的建表语句（列、可空、主键），不带结尾的分号。
    pub fn create_table_sql(&self, schema: &str, table: &Table) -> String {
        self.table_create(schema, table).trim_end().trim_end_matches(';').to_string()
    }

    /// 由若干行生成写入语句。`columns` 是映射到的表列，每行的值与之一一对应。
    /// 列不存在、是生成列，或按主键更新时表没有主键、主键列没有全部映射，返回 `invalid`。
    pub fn import_statements(
        &self,
        schema: &str,
        table: &Table,
        columns: &[String],
        rows: &[Vec<Value>],
        mode: WriteMode,
    ) -> Result<Vec<ImportStatement>> {
        if columns.is_empty() {
            return Err(Error::invalid(msg!("import.no_columns")));
        }
        let cols: Vec<&Column> = columns
            .iter()
            .map(|name| {
                let c = table
                    .columns
                    .iter()
                    .find(|c| &c.name == name)
                    .ok_or_else(|| Error::invalid(msg!("changes.unknown_column", column = name)))?;
                if c.generated.is_some() {
                    return Err(Error::invalid(msg!("changes.generated_column", column = name)));
                }
                Ok(c)
            })
            .collect::<Result<_>>()?;
        let key: Vec<usize> = if mode == WriteMode::Upsert {
            let pk = table.primary_key.as_ref().filter(|pk| !pk.columns.is_empty());
            let pos: Option<Vec<usize>> =
                pk.and_then(|pk| pk.columns.iter().map(|k| columns.iter().position(|c| c == k)).collect());
            pos.ok_or_else(|| Error::invalid(msg!("import.upsert_needs_key", table = table.name)))?
        } else {
            Vec::new()
        };
        let merge = mode == WriteMode::Upsert
            && matches!(self.family(), Family::Oracle | Family::Dameng | Family::Informix);
        // GBase 8s 的 INSERT 没有多行 VALUES；Oracle、达梦的 MERGE 每行一条：多行要用 UNION ALL 拼来源，各行的 NULL 参数类型不一致时会报错
        let per_statement = if merge || self.family() == Family::Informix {
            1
        } else {
            (self.max_params() / cols.len()).clamp(1, MAX_ROWS_PER_STATEMENT)
        };
        let target = self.qualified(Some(schema), &table.name);
        let names: Vec<String> = cols.iter().map(|c| self.ident(&c.name)).collect();
        let list = names.join(", ");
        let mut out = Vec::new();
        let mut start = 0;
        while start < rows.len() {
            let end = (start + per_statement).min(rows.len());
            let mut b = Binder { dialect: self, sql: String::new(), params: Vec::new() };
            if merge {
                b.merge(&target, &cols, &names, &key, &rows[start])?;
            } else if mode == WriteMode::Upsert && self.family() == Family::SqlServer {
                b.mssql_merge(&target, &cols, &names, &key, &rows[start..end])?;
            } else {
                b.insert(&target, &cols, &list, &rows[start..end])?;
                if mode == WriteMode::Upsert {
                    b.upsert_tail(&names, &key);
                }
            }
            out.push(ImportStatement { sql: b.sql, params: b.params, rows: start..end });
            start = end;
        }
        Ok(out)
    }
}

/// 拼语句，同时收集参数。
struct Binder<'a> {
    dialect: &'a Dialect,
    sql: String,
    params: Vec<Value>,
}

impl Binder<'_> {
    /// 一个参数。PG 驱动不能直接绑定 NUMERIC 参数，精确小数按文本传、由数据库转换（与表数据编辑相同）；
    /// 时间间隔和不认识的类型也按文本传，转换成列的原生类型。
    fn param(&mut self, col: &Column, v: &Value) {
        // SQL Server：类型未知的 NULL 参数按 nvarchar 传，写进 varbinary 等列会报"不能隐式转换"，直接写 NULL
        if self.dialect.family() == Family::SqlServer && matches!(v, Value::Null) {
            self.sql.push_str("NULL");
            return;
        }
        self.params.push(v.clone());
        self.sql.push_str(&self.dialect.placeholder(self.params.len()));
        if self.dialect.family() != Family::Postgres || matches!(v, Value::Null) {
            return;
        }
        if matches!(v, Value::Decimal(_)) {
            self.sql.push_str("::text::numeric");
        } else if matches!(col.data_type.logical, LogicalType::Interval | LogicalType::Other) {
            self.sql.push_str("::text::");
            self.sql.push_str(&col.data_type.native);
        }
    }

    fn row_values(&mut self, cols: &[&Column], row: &[Value]) -> Result<()> {
        if row.len() != cols.len() {
            return Err(Error::invalid(msg!("import.row_width", expected = cols.len(), actual = row.len())));
        }
        for (k, (c, v)) in cols.iter().zip(row).enumerate() {
            if k > 0 {
                self.sql.push_str(", ");
            }
            self.param(c, v);
        }
        Ok(())
    }

    /// 多行 INSERT。Oracle 用 `INSERT ALL`（多行 VALUES 要 23ai 才支持）。
    fn insert(&mut self, target: &str, cols: &[&Column], list: &str, rows: &[Vec<Value>]) -> Result<()> {
        if self.dialect.family() == Family::Oracle {
            self.sql.push_str("INSERT ALL");
            for row in rows {
                self.sql.push_str(&format!(" INTO {target} ({list}) VALUES ("));
                self.row_values(cols, row)?;
                self.sql.push(')');
            }
            self.sql.push_str(" SELECT 1 FROM DUAL");
            return Ok(());
        }
        self.sql.push_str(&format!("INSERT INTO {target} ({list}) VALUES "));
        for (n, row) in rows.iter().enumerate() {
            if n > 0 {
                self.sql.push_str(", ");
            }
            self.sql.push('(');
            self.row_values(cols, row)?;
            self.sql.push(')');
        }
        Ok(())
    }

    /// MySQL、PG 的按主键更新：接在多行 INSERT 后面。只映射了主键列时，已存在的行不变。
    fn upsert_tail(&mut self, names: &[String], key: &[usize]) {
        let others: Vec<&String> =
            names.iter().enumerate().filter(|(k, _)| !key.contains(k)).map(|(_, n)| n).collect();
        match self.dialect.family() {
            // 用 `VALUES(列)` 兼容 MariaDB；没有其他列时把主键列更新为自身，相当于不变
            Family::MySql => {
                let sets: Vec<String> = if others.is_empty() {
                    vec![format!("{0} = {0}", names[key[0]])]
                } else {
                    others.iter().map(|n| format!("{n} = VALUES({n})")).collect()
                };
                self.sql.push_str(&format!(" ON DUPLICATE KEY UPDATE {}", sets.join(", ")));
            }
            // openGauss 没有 ON CONFLICT，用它自己的 ON DUPLICATE KEY UPDATE（可引用 EXCLUDED）
            _ if self.dialect.variant() == Variant::OpenGauss => {
                self.sql.push_str(" ON DUPLICATE KEY UPDATE ");
                if others.is_empty() {
                    self.sql.push_str("NOTHING");
                } else {
                    let sets: Vec<String> = others.iter().map(|n| format!("{n} = EXCLUDED.{n}")).collect();
                    self.sql.push_str(&sets.join(", "));
                }
            }
            _ => {
                let keys: Vec<&str> = key.iter().map(|&k| names[k].as_str()).collect();
                self.sql.push_str(&format!(" ON CONFLICT ({}) DO ", keys.join(", ")));
                if others.is_empty() {
                    self.sql.push_str("NOTHING");
                } else {
                    let sets: Vec<String> = others.iter().map(|n| format!("{n} = EXCLUDED.{n}")).collect();
                    self.sql.push_str(&format!("UPDATE SET {}", sets.join(", ")));
                }
            }
        }
    }

    /// SQL Server 的按主键更新：一批一条多行 MERGE（来源是带参数的 VALUES）。自增列不能更新，不写进 UPDATE SET；
    /// 没有可更新的列时省略 WHEN MATCHED。MERGE 必须以分号结束。
    fn mssql_merge(
        &mut self,
        target: &str,
        cols: &[&Column],
        names: &[String],
        key: &[usize],
        rows: &[Vec<Value>],
    ) -> Result<()> {
        self.sql.push_str(&format!("MERGE INTO {target} AS t USING (VALUES "));
        for (n, row) in rows.iter().enumerate() {
            if n > 0 {
                self.sql.push_str(", ");
            }
            self.sql.push('(');
            self.row_values(cols, row)?;
            self.sql.push(')');
        }
        let on: Vec<String> = key.iter().map(|&k| format!("t.{0} = s.{0}", names[k])).collect();
        self.sql.push_str(&format!(") AS s ({}) ON {}", names.join(", "), on.join(" AND ")));
        let sets: Vec<String> = (0..names.len())
            .filter(|&k| !key.contains(&k) && !cols[k].auto_increment)
            .map(|k| format!("t.{0} = s.{0}", names[k]))
            .collect();
        if !sets.is_empty() {
            self.sql.push_str(&format!(" WHEN MATCHED THEN UPDATE SET {}", sets.join(", ")));
        }
        let values: Vec<String> = names.iter().map(|n| format!("s.{n}")).collect();
        self.sql.push_str(&format!(
            " WHEN NOT MATCHED THEN INSERT ({}) VALUES ({});",
            names.join(", "),
            values.join(", ")
        ));
        Ok(())
    }

    /// Oracle、达梦、GBase 8s 的按主键更新：一行一条 MERGE。只映射了主键列时省略 WHEN MATCHED。
    /// GBase 8s 的来源是 sysmaster:sysdual，SELECT 列表里的参数要 CAST 成列的类型。
    fn merge(
        &mut self,
        target: &str,
        cols: &[&Column],
        names: &[String],
        key: &[usize],
        row: &[Value],
    ) -> Result<()> {
        if row.len() != cols.len() {
            return Err(Error::invalid(msg!("import.row_width", expected = cols.len(), actual = row.len())));
        }
        self.sql.push_str(&format!("MERGE INTO {target} t USING (SELECT "));
        for (k, (c, v)) in cols.iter().zip(row).enumerate() {
            if k > 0 {
                self.sql.push_str(", ");
            }
            let informix = self.dialect.family() == Family::Informix;
            if informix {
                self.sql.push_str("CAST(");
            }
            self.param(c, v);
            if informix {
                self.sql.push_str(&format!(" AS {})", c.data_type.native));
            }
            self.sql.push_str(&format!(" AS {}", names[k]));
        }
        let on: Vec<String> = key.iter().map(|&k| format!("t.{0} = s.{0}", names[k])).collect();
        let dual = if self.dialect.family() == Family::Informix { "sysmaster:sysdual" } else { "DUAL" };
        self.sql.push_str(&format!(" FROM {dual}) s ON ({})", on.join(" AND ")));
        let sets: Vec<String> = (0..names.len())
            .filter(|k| !key.contains(k))
            .map(|k| format!("t.{0} = s.{0}", names[k]))
            .collect();
        if !sets.is_empty() {
            self.sql.push_str(&format!(" WHEN MATCHED THEN UPDATE SET {}", sets.join(", ")));
        }
        let values: Vec<String> = names.iter().map(|n| format!("s.{n}")).collect();
        self.sql.push_str(&format!(
            " WHEN NOT MATCHED THEN INSERT ({}) VALUES ({})",
            names.join(", "),
            values.join(", ")
        ));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use dblens_core::ColumnType;
    use dblens_core::schema::PrimaryKey;

    use super::*;
    use crate::Variant;

    fn col(name: &str, logical: LogicalType) -> Column {
        Column {
            name: name.into(),
            ordinal: 0,
            data_type: ColumnType { native: "x".into(), logical },
            nullable: true,
            default: None,
            comment: None,
            auto_increment: false,
            generated: None,
        }
    }

    /// orders(id 主键, name, price 精确小数, total 生成列)
    fn orders() -> Table {
        let mut total = col("total", LogicalType::Decimal { precision: Some(10), scale: Some(2) });
        total.generated = Some("price * 2".into());
        Table {
            name: "orders".into(),
            columns: vec![
                col("id", LogicalType::Int { bits: 32 }),
                col("name", LogicalType::VarChar { len: Some(20) }),
                col("price", LogicalType::Decimal { precision: Some(10), scale: Some(2) }),
                total,
            ],
            primary_key: Some(PrimaryKey { name: None, columns: vec!["id".into()] }),
            ..Default::default()
        }
    }

    fn names(cols: &[&str]) -> Vec<String> {
        cols.iter().map(|c| c.to_string()).collect()
    }

    fn rows() -> Vec<Vec<Value>> {
        vec![
            vec![Value::Int(1), Value::String("a".into()), Value::Decimal("1.50".into())],
            vec![Value::Int(2), Value::Null, Value::Null],
        ]
    }

    fn sql(v: Variant, schema: &str, mode: WriteMode) -> Vec<String> {
        Dialect::new(v)
            .import_statements(schema, &orders(), &names(&["id", "name", "price"]), &rows(), mode)
            .unwrap()
            .into_iter()
            .map(|s| s.sql)
            .collect()
    }

    #[test]
    fn append_uses_multi_row_insert() {
        assert_eq!(
            sql(Variant::Postgres, "public", WriteMode::Append),
            ["INSERT INTO public.orders (id, name, price) VALUES ($1, $2, $3::text::numeric), ($4, $5, $6)"]
        );
        assert_eq!(
            sql(Variant::MySql, "shop", WriteMode::Replace),
            ["INSERT INTO shop.orders (id, name, price) VALUES (?, ?, ?), (?, ?, ?)"]
        );
        assert_eq!(
            sql(Variant::Dameng, "SHOP", WriteMode::Append),
            [r#"INSERT INTO SHOP."orders" ("id", "name", "price") VALUES (?, ?, ?), (?, ?, ?)"#]
        );
        // Oracle：INSERT ALL
        assert_eq!(
            sql(Variant::Oracle, "HR", WriteMode::Append),
            [concat!(
                r#"INSERT ALL INTO HR."orders" ("id", "name", "price") VALUES (:1, :2, :3)"#,
                r#" INTO HR."orders" ("id", "name", "price") VALUES (:4, :5, :6) SELECT 1 FROM DUAL"#
            )]
        );
        let st = Dialect::new(Variant::Postgres)
            .import_statements(
                "public",
                &orders(),
                &names(&["id", "name", "price"]),
                &rows(),
                WriteMode::Append,
            )
            .unwrap();
        assert_eq!(st[0].params, rows().concat());
        assert_eq!(st[0].rows, 0..2);
    }

    #[test]
    fn upsert_per_family() {
        assert_eq!(
            sql(Variant::MySql, "shop", WriteMode::Upsert),
            ["INSERT INTO shop.orders (id, name, price) VALUES (?, ?, ?), (?, ?, ?) \
              ON DUPLICATE KEY UPDATE name = VALUES(name), price = VALUES(price)"]
        );
        assert_eq!(
            sql(Variant::Postgres, "public", WriteMode::Upsert),
            ["INSERT INTO public.orders (id, name, price) VALUES ($1, $2, $3::text::numeric), ($4, $5, $6) \
              ON CONFLICT (id) DO UPDATE SET name = EXCLUDED.name, price = EXCLUDED.price"]
        );
        assert_eq!(
            sql(Variant::OpenGauss, "public", WriteMode::Upsert),
            [
                "INSERT INTO public.orders (id, name, price) VALUES ($1, $2, $3::text::numeric), ($4, $5, $6) ON DUPLICATE KEY UPDATE name = EXCLUDED.name, price = EXCLUDED.price"
            ]
        );
        // Oracle、达梦：一行一条 MERGE
        let ora = sql(Variant::Oracle, "HR", WriteMode::Upsert);
        assert_eq!(ora.len(), 2);
        assert_eq!(
            ora[0],
            concat!(
                r#"MERGE INTO HR."orders" t USING (SELECT :1 AS "id", :2 AS "name", :3 AS "price" FROM DUAL) s"#,
                r#" ON (t."id" = s."id") WHEN MATCHED THEN UPDATE SET t."name" = s."name", t."price" = s."price""#,
                r#" WHEN NOT MATCHED THEN INSERT ("id", "name", "price") VALUES (s."id", s."name", s."price")"#
            )
        );
        let dm = sql(Variant::Dameng, "SHOP", WriteMode::Upsert);
        assert!(dm[1].starts_with(r#"MERGE INTO SHOP."orders" t USING (SELECT ? AS "id""#), "{}", dm[1]);
    }

    #[test]
    fn upsert_with_only_key_columns_keeps_existing_rows() {
        let only = |v: Variant| {
            Dialect::new(v)
                .import_statements("s", &orders(), &names(&["id"]), &[vec![Value::Int(1)]], WriteMode::Upsert)
                .unwrap()[0]
                .sql
                .clone()
        };
        assert_eq!(
            only(Variant::MySql),
            "INSERT INTO s.orders (id) VALUES (?) ON DUPLICATE KEY UPDATE id = id"
        );
        assert_eq!(
            only(Variant::Postgres),
            "INSERT INTO s.orders (id) VALUES ($1) ON CONFLICT (id) DO NOTHING"
        );
        assert_eq!(
            only(Variant::Oracle),
            concat!(
                r#"MERGE INTO "s"."orders" t USING (SELECT :1 AS "id" FROM DUAL) s ON (t."id" = s."id")"#,
                r#" WHEN NOT MATCHED THEN INSERT ("id") VALUES (s."id")"#
            )
        );
    }

    #[test]
    fn splits_by_rows_and_params() {
        let d = Dialect::new(Variant::MySql);
        let many: Vec<Vec<Value>> =
            (0..1200).map(|n| vec![Value::Int(n), Value::Null, Value::Null]).collect();
        let cols = names(&["id", "name", "price"]);
        let st = d.import_statements("s", &orders(), &cols, &many, WriteMode::Append).unwrap();
        assert_eq!(st.iter().map(|s| s.rows.clone()).collect::<Vec<_>>(), [0..500, 500..1000, 1000..1200]);
        assert_eq!(st[2].params.len(), 600);
        // 列多时按参数个数限制每条的行数：100 列 → 每条 300 行
        let wide = Table {
            columns: (0..100).map(|k| col(&format!("c{k}"), LogicalType::Int { bits: 32 })).collect(),
            ..orders()
        };
        let cols: Vec<String> = (0..100).map(|k| format!("c{k}")).collect();
        let rows: Vec<Vec<Value>> = (0..700).map(|_| vec![Value::Null; 100]).collect();
        let st = d.import_statements("s", &wide, &cols, &rows, WriteMode::Append).unwrap();
        assert_eq!(st.iter().map(|s| s.rows.len()).collect::<Vec<_>>(), [300, 300, 100]);
        assert!(st.iter().all(|s| s.params.len() <= MAX_PARAMS_PER_STATEMENT));
        // SQL Server 一次请求最多 2100 个参数：100 列 → 每条 20 行
        let ms = Dialect::new(Variant::SqlServer);
        let st = ms.import_statements("s", &wide, &cols, &rows, WriteMode::Append).unwrap();
        assert!(
            st.iter().all(|s| s.rows.len() == 20),
            "{:?}",
            st.iter().map(|s| s.rows.len()).collect::<Vec<_>>()
        );
        let st = ms
            .import_statements("s", &orders(), &names(&["id", "name", "price"]), &many, WriteMode::Upsert)
            .unwrap();
        assert!(st.iter().all(|s| s.params.len() <= 2000) && st.len() == 3);
    }

    #[test]
    fn sqlserver_upsert_is_one_multi_row_merge() {
        assert_eq!(
            sql(Variant::SqlServer, "dbo", WriteMode::Upsert),
            [
                "MERGE INTO dbo.orders AS t USING (VALUES (@P1, @P2, @P3), (@P4, NULL, NULL)) AS s (id, name, price) \
              ON t.id = s.id WHEN MATCHED THEN UPDATE SET t.name = s.name, t.price = s.price \
              WHEN NOT MATCHED THEN INSERT (id, name, price) VALUES (s.id, s.name, s.price);"
            ]
        );
        // 只映射了主键列：没有 WHEN MATCHED
        let only = Dialect::new(Variant::SqlServer)
            .import_statements("dbo", &orders(), &names(&["id"]), &[vec![Value::Int(1)]], WriteMode::Upsert)
            .unwrap();
        assert_eq!(
            only[0].sql,
            "MERGE INTO dbo.orders AS t USING (VALUES (@P1)) AS s (id) ON t.id = s.id \
             WHEN NOT MATCHED THEN INSERT (id) VALUES (s.id);"
        );
    }

    #[test]
    fn rejects_bad_input() {
        let d = Dialect::new(Variant::Postgres);
        let t = orders();
        let err = |cols: &[&str], rows: &[Vec<Value>], mode| {
            d.import_statements("s", &t, &names(cols), rows, mode).unwrap_err()
        };
        let one = [vec![Value::Int(1)]];
        assert!(matches!(err(&[], &[], WriteMode::Append), Error::Invalid { .. }), "没有列");
        assert!(matches!(err(&["nope"], &one, WriteMode::Append), Error::Invalid { .. }), "不存在的列");
        assert!(matches!(err(&["total"], &one, WriteMode::Append), Error::Invalid { .. }), "生成列");
        assert!(matches!(err(&["name"], &one, WriteMode::Upsert), Error::Invalid { .. }), "主键列没有映射");
        let no_pk = Table { primary_key: None, ..orders() };
        let r = d.import_statements("s", &no_pk, &names(&["id"]), &one, WriteMode::Upsert);
        assert!(matches!(r, Err(Error::Invalid { .. })), "没有主键");
        assert!(
            matches!(err(&["id", "name"], &one, WriteMode::Append), Error::Invalid { .. }),
            "值的个数不对"
        );
        // 没有行时没有语句
        assert!(d.import_statements("s", &t, &names(&["id"]), &[], WriteMode::Append).unwrap().is_empty());
    }

    #[test]
    fn interval_and_unknown_types_are_cast_from_text_on_postgres() {
        let mut t = orders();
        let typed = |name: &str, native: &str, logical: LogicalType| Column {
            data_type: ColumnType { native: native.into(), logical: logical.clone() },
            ..col(name, logical)
        };
        t.columns.push(typed("span", "interval", LogicalType::Interval));
        t.columns.push(typed("feel", "mood", LogicalType::Other));
        let row = vec![Value::String("1 day".into()), Value::String("happy".into())];
        let st = Dialect::new(Variant::Postgres)
            .import_statements("s", &t, &names(&["span", "feel"]), &[row], WriteMode::Append)
            .unwrap();
        assert_eq!(
            st[0].sql,
            "INSERT INTO s.orders (span, feel) VALUES ($1::text::interval, $2::text::mood)"
        );
    }

    #[test]
    fn create_table_and_delete_all() {
        let mut id = col("id", LogicalType::Int { bits: 64 });
        id.nullable = false;
        id.data_type.native = "BIGINT".into();
        let mut name = col("名称", LogicalType::VarChar { len: Some(100) });
        name.data_type.native = "VARCHAR(100)".into();
        let t = Table {
            name: "导入表".into(),
            columns: vec![id, name],
            primary_key: Some(PrimaryKey { name: None, columns: vec!["id".into()] }),
            ..Default::default()
        };
        assert_eq!(
            Dialect::new(Variant::Postgres).create_table_sql("public", &t),
            "CREATE TABLE public.\"导入表\" (\n    id BIGINT NOT NULL,\n    \"名称\" VARCHAR(100),\n    PRIMARY KEY (id)\n)"
        );
        assert_eq!(Dialect::new(Variant::MySql).delete_all_sql("shop", "orders"), "DELETE FROM shop.orders");
        assert_eq!(
            Dialect::new(Variant::Oracle).delete_all_sql("HR", "orders"),
            r#"DELETE FROM HR."orders""#
        );
    }
}
