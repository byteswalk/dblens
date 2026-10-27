//! 表数据编辑的改动集（M2a 设计文档第 3、4 节）：由前端暂存的修改、新增、删除生成 DML。
//! 每个改动生成一条语句，同时给出带占位符的 SQL 和参数（执行用）与值写成字面量的 SQL（预览用）。

use std::collections::BTreeMap;

use dblens_core::schema::{Column, Table};
use dblens_core::{Error, LogicalType, Result, Value, msg};
use serde::{Deserialize, Serialize};

use crate::{Dialect, Family};

/// 一行的改动。列名 → 值；`key` 是行标识列的原值。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RowChange {
    Update {
        key: BTreeMap<String, Value>,
        /// 改动列的原值（冲突检查用）。
        original: BTreeMap<String, Value>,
        values: BTreeMap<String, Value>,
    },
    Insert {
        values: BTreeMap<String, Value>,
    },
    Delete {
        key: BTreeMap<String, Value>,
    },
}

/// 一条生成的语句。`index` 是它在改动集中的序号。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ChangeStatement {
    pub index: usize,
    pub sql: String,
    #[serde(skip)]
    pub params: Vec<Value>,
    /// 值写成字面量的 SQL，预览和执行历史用。
    pub preview: String,
}

/// 标识一行用的列：主键；没有主键时取第一个所有列都 NOT NULL 的唯一键。都没有时为 None（整张表只读）。
pub fn row_key(table: &Table) -> Option<Vec<&Column>> {
    let find = |names: &[String]| -> Option<Vec<&Column>> {
        let cols: Option<Vec<&Column>> =
            names.iter().map(|n| table.columns.iter().find(|c| &c.name == n)).collect();
        cols.filter(|c| !c.is_empty())
    };
    if let Some(pk) = &table.primary_key
        && let Some(cols) = find(&pk.columns)
    {
        return Some(cols);
    }
    table.uniques.iter().filter_map(|u| find(&u.columns)).find(|cols| cols.iter().all(|c| !c.nullable))
}

/// 能否用 `=` 可靠地比较原值：浮点有精度问题，长文本（Oracle、达梦的 CLOB）、大字段、二进制、JSON 不能或不宜比较。
fn comparable(col: &Column, original: &Value) -> bool {
    let by_type = !matches!(
        col.data_type.logical,
        LogicalType::Float { .. }
            | LogicalType::Text
            | LogicalType::Blob
            | LogicalType::Binary { .. }
            | LogicalType::VarBinary { .. }
            | LogicalType::Json
            | LogicalType::Other
    );
    by_type && !matches!(original, Value::Lob(_))
}

/// 语句的两种形式一起拼：执行用占位符，预览用字面量。
struct Builder<'a> {
    dialect: &'a Dialect,
    sql: String,
    preview: String,
    params: Vec<Value>,
}

impl<'a> Builder<'a> {
    fn new(dialect: &'a Dialect) -> Self {
        Self { dialect, sql: String::new(), preview: String::new(), params: Vec::new() }
    }

    fn text(&mut self, s: &str) {
        self.sql.push_str(s);
        self.preview.push_str(s);
    }

    fn value(&mut self, v: &Value) -> Result<()> {
        self.params.push(v.clone());
        self.sql.push_str(&self.dialect.placeholder(self.params.len()));
        // PG 驱动不能直接绑定 NUMERIC 参数：精确小数按文本传，由数据库转换（整数列也接受 numeric 的赋值和比较）
        if self.dialect.family() == Family::Postgres && matches!(v, Value::Decimal(_)) {
            self.sql.push_str("::text::numeric");
        }
        self.preview.push_str(&self.dialect.literal(v)?);
        Ok(())
    }

    /// `列 = 值`；值为 NULL 时 `列 IS NULL`（不占参数）。
    fn equals(&mut self, col: &str, v: &Value) -> Result<()> {
        self.text(&self.dialect.ident(col));
        if matches!(v, Value::Null) {
            self.text(" IS NULL");
            Ok(())
        } else {
            self.text(" = ");
            self.value(v)
        }
    }

    fn finish(self, index: usize) -> ChangeStatement {
        ChangeStatement { index, sql: self.sql, params: self.params, preview: self.preview }
    }
}

fn column<'t>(table: &'t Table, name: &str) -> Result<&'t Column> {
    table
        .columns
        .iter()
        .find(|c| c.name == name)
        .ok_or_else(|| Error::invalid(msg!("changes.unknown_column", column = name)))
}

impl Dialect {
    /// 由改动集生成语句：先删除，再修改，最后新增（"改掉主键再新增一行用回旧值"不冲突）；同类改动保持原顺序。
    /// 表没有行标识、改动引用了不存在的列、写了生成列、`key` 的列与行标识不一致时返回 `invalid`。
    pub fn change_statements(
        &self,
        schema: &str,
        table: &Table,
        changes: &[RowChange],
    ) -> Result<Vec<ChangeStatement>> {
        let key = row_key(table).ok_or_else(|| Error::invalid(msg!("changes.no_key", table = table.name)))?;
        let target = self.qualified(Some(schema), &table.name);
        let check_key = |k: &BTreeMap<String, Value>| -> Result<()> {
            let same = k.len() == key.len() && key.iter().all(|c| k.contains_key(&c.name));
            if same { Ok(()) } else { Err(Error::invalid(msg!("changes.bad_key", table = table.name))) }
        };
        let writable = |name: &str| -> Result<&Column> {
            let c = column(table, name)?;
            if c.generated.is_some() {
                return Err(Error::invalid(msg!("changes.generated_column", column = name)));
            }
            Ok(c)
        };
        let order = |c: &RowChange| match c {
            RowChange::Delete { .. } => 0,
            RowChange::Update { .. } => 1,
            RowChange::Insert { .. } => 2,
        };
        let mut indexed: Vec<(usize, &RowChange)> = changes.iter().enumerate().collect();
        indexed.sort_by_key(|(i, c)| (order(c), *i));

        let mut out = Vec::with_capacity(changes.len());
        for (index, change) in indexed {
            let mut b = Builder::new(self);
            match change {
                RowChange::Delete { key: k } => {
                    check_key(k)?;
                    b.text(&format!("DELETE FROM {target} WHERE "));
                    for (i, c) in key.iter().enumerate() {
                        if i > 0 {
                            b.text(" AND ");
                        }
                        b.equals(&c.name, &k[&c.name])?;
                    }
                }
                RowChange::Update { key: k, original, values } => {
                    check_key(k)?;
                    if values.is_empty() {
                        return Err(Error::invalid(msg!("changes.empty_update")));
                    }
                    b.text(&format!("UPDATE {target} SET "));
                    for (i, (name, v)) in values.iter().enumerate() {
                        writable(name)?;
                        if i > 0 {
                            b.text(", ");
                        }
                        b.text(&format!("{} = ", self.ident(name)));
                        b.value(v)?;
                    }
                    b.text(" WHERE ");
                    for (i, c) in key.iter().enumerate() {
                        if i > 0 {
                            b.text(" AND ");
                        }
                        b.equals(&c.name, &k[&c.name])?;
                    }
                    // 改动列的原值：别人在此期间改过同一字段时影响行数为 0
                    for (name, old) in original {
                        let c = column(table, name)?;
                        if values.contains_key(name)
                            && !key.iter().any(|k| k.name == c.name)
                            && comparable(c, old)
                        {
                            b.text(" AND ");
                            b.equals(name, old)?;
                        }
                    }
                }
                RowChange::Insert { values } => {
                    // 自增列和有默认值的列留空（NULL）时不写，交给数据库
                    let cols: Vec<(&String, &Value)> = values
                        .iter()
                        .filter(|(name, v)| {
                            let c = table.columns.iter().find(|c| &&c.name == name);
                            !(matches!(v, Value::Null)
                                && c.is_some_and(|c| c.auto_increment || c.default.is_some()))
                        })
                        .collect();
                    for (name, _) in &cols {
                        writable(name)?;
                    }
                    if cols.is_empty() {
                        // 全部用默认值：把所有列当作自增列，借 insert_sql 生成各方言"只用默认值"的写法（不含占位符）
                        let defaults = Table {
                            columns: table
                                .columns
                                .iter()
                                .map(|c| Column { auto_increment: true, ..c.clone() })
                                .collect(),
                            ..table.clone()
                        };
                        b.text(&self.insert_sql(Some(schema), &defaults).sql);
                    } else {
                        let names: Vec<String> = cols.iter().map(|(n, _)| self.ident(n)).collect();
                        b.text(&format!("INSERT INTO {target} ({}) VALUES (", names.join(", ")));
                        for (i, (_, v)) in cols.iter().enumerate() {
                            if i > 0 {
                                b.text(", ");
                            }
                            b.value(v)?;
                        }
                        b.text(")");
                    }
                }
            }
            out.push(b.finish(index));
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use dblens_core::ColumnType;
    use dblens_core::schema::{PrimaryKey, UniqueConstraint};

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

    fn orders() -> Table {
        let mut id = col("id", LogicalType::Int { bits: 32 });
        id.auto_increment = true;
        id.nullable = false;
        let mut status = col("status", LogicalType::VarChar { len: Some(10) });
        status.default = Some("'new'".into());
        let mut total = col("total", LogicalType::Decimal { precision: Some(10), scale: Some(2) });
        total.generated = Some("price * qty".into());
        Table {
            name: "orders".into(),
            columns: vec![
                id,
                col("name", LogicalType::VarChar { len: Some(20) }),
                col("price", LogicalType::Decimal { precision: Some(10), scale: Some(2) }),
                col("rate", LogicalType::Float { bits: 64 }),
                col("note", LogicalType::Text),
                status,
                total,
            ],
            primary_key: Some(PrimaryKey { name: None, columns: vec!["id".into()] }),
            ..Default::default()
        }
    }

    fn map(pairs: &[(&str, Value)]) -> BTreeMap<String, Value> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.clone())).collect()
    }

    fn s(v: &str) -> Value {
        Value::String(v.into())
    }

    #[test]
    fn update_where_has_key_and_comparable_originals() {
        let change = RowChange::Update {
            key: map(&[("id", Value::Int(1))]),
            original: map(&[
                ("name", s("旧")),
                ("price", Value::Null),
                ("rate", Value::Float(1.5)),
                ("note", s("长文本")),
            ]),
            values: map(&[
                ("name", s("新")),
                ("price", Value::Decimal("9.90".into())),
                ("rate", Value::Float(2.0)),
                ("note", s("x")),
            ]),
        };
        let st = Dialect::new(Variant::Postgres).change_statements("public", &orders(), &[change]).unwrap();
        assert_eq!(
            st[0].sql,
            "UPDATE public.orders SET name = $1, note = $2, price = $3::text::numeric, rate = $4 \
             WHERE id = $5 AND name = $6 AND price IS NULL"
        );
        assert_eq!(st[0].params.len(), 6);
        assert_eq!(
            st[0].preview,
            "UPDATE public.orders SET name = '新', note = 'x', price = 9.90, rate = 2 \
             WHERE id = 1 AND name = '旧' AND price IS NULL"
        );
    }

    #[test]
    fn order_is_delete_update_insert_and_placeholders_follow_dialect() {
        let changes = [
            RowChange::Insert {
                values: map(&[("id", Value::Null), ("name", s("a")), ("status", Value::Null)]),
            },
            RowChange::Update {
                key: map(&[("id", Value::Int(2))]),
                original: map(&[]),
                values: map(&[("name", s("b"))]),
            },
            RowChange::Delete { key: map(&[("id", Value::Int(3))]) },
        ];
        let st = Dialect::new(Variant::Oracle).change_statements("HR", &orders(), &changes).unwrap();
        assert_eq!(st.iter().map(|s| s.index).collect::<Vec<_>>(), [2, 1, 0]);
        // Oracle 的小写名要加引号
        assert_eq!(st[0].sql, r#"DELETE FROM HR."orders" WHERE "id" = :1"#);
        assert_eq!(st[1].sql, r#"UPDATE HR."orders" SET "name" = :1 WHERE "id" = :2"#);
        // 自增列和有默认值的列为 NULL 时不写
        assert_eq!(st[2].sql, r#"INSERT INTO HR."orders" ("name") VALUES (:1)"#);
        let my = Dialect::new(Variant::MySql).change_statements("shop", &orders(), &changes).unwrap();
        assert_eq!(my[1].sql, "UPDATE shop.orders SET name = ? WHERE id = ?");
    }

    #[test]
    fn rejects_bad_changes() {
        let d = Dialect::new(Variant::Postgres);
        let t = orders();
        let err = |c: RowChange| d.change_statements("p", &t, &[c]).unwrap_err();
        assert!(matches!(err(RowChange::Delete { key: map(&[("name", s("x"))]) }), Error::Invalid { .. }));
        assert!(matches!(
            err(RowChange::Insert { values: map(&[("total", Value::Int(1))]) }),
            Error::Invalid { .. }
        ));
        assert!(matches!(
            err(RowChange::Insert { values: map(&[("nope", Value::Int(1))]) }),
            Error::Invalid { .. }
        ));
        assert!(matches!(
            err(RowChange::Update {
                key: map(&[("id", Value::Int(1))]),
                original: map(&[]),
                values: map(&[])
            }),
            Error::Invalid { .. }
        ));
        let no_key = Table { primary_key: None, ..orders() };
        assert!(d.change_statements("p", &no_key, &[RowChange::Insert { values: map(&[]) }]).is_err());
    }

    #[test]
    fn row_key_falls_back_to_not_null_unique() {
        let mut code = col("code", LogicalType::VarChar { len: Some(5) });
        let t = |code: Column| Table {
            name: "t".into(),
            columns: vec![col("a", LogicalType::Int { bits: 32 }), code],
            uniques: vec![UniqueConstraint { name: None, columns: vec!["code".into()] }],
            ..Default::default()
        };
        assert!(row_key(&t(code.clone())).is_none(), "可为空的唯一键不能标识行");
        code.nullable = false;
        assert_eq!(row_key(&t(code)).unwrap().iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["code"]);
        assert_eq!(row_key(&orders()).unwrap()[0].name, "id");
    }

    #[test]
    fn insert_with_only_defaults_uses_insert_template() {
        let st = Dialect::new(Variant::Postgres)
            .change_statements("p", &orders(), &[RowChange::Insert { values: map(&[("id", Value::Null)]) }])
            .unwrap();
        assert!(st[0].sql.starts_with("INSERT INTO p.orders"), "{}", st[0].sql);
    }
}
