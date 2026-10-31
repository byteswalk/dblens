//! 把规范化的元数据结果行组装成结构模型。所有家族共用，列名约定见 `sql/*.sql` 文件头。

use std::collections::HashMap;

use dblens_core::schema::{
    CheckConstraint, Column, ForeignKey, Index, IndexColumn, PrimaryKey, Routine, RoutineKind, Sequence,
    Synonym, Table, Trigger, UniqueConstraint, View,
};
use dblens_core::{ColumnType, Result};
use serde::{Deserialize, Serialize};

use super::{MetaKey, MetaRow, MetaRows};
use crate::{Dialect, Family};

/// 对象树里的 schema 节点。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SchemaInfo {
    pub name: String,
    /// 数据库自带的系统 schema（界面默认折叠或隐藏）。
    pub is_system: bool,
}

/// 对象树里的表、视图、物化视图节点：只有名字和注释，展开时再取明细。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObjectInfo {
    pub name: String,
    pub comment: Option<String>,
}

/// 组装表明细所需的六个结果集，按 `MetaKey::TABLE_DETAIL` 的顺序执行得到。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TableRows {
    pub tables: MetaRows,
    pub columns: MetaRows,
    pub keys: MetaRows,
    pub checks: MetaRows,
    pub indexes: MetaRows,
    pub triggers: MetaRows,
}

pub fn schemas(rows: &MetaRows) -> Result<Vec<SchemaInfo>> {
    rows.require("schemas", &["name", "is_system"])?;
    Ok(rows.iter().map(|r| SchemaInfo { name: r.name("name"), is_system: r.flag("is_system") }).collect())
}

/// 表、视图、物化视图的名字列表。
pub fn objects(key: MetaKey, rows: &MetaRows) -> Result<Vec<ObjectInfo>> {
    rows.require(key.as_str(), &["name", "comment"])?;
    Ok(rows.iter().map(|r| ObjectInfo { name: r.name("name"), comment: r.text("comment") }).collect())
}

/// 按表名分组，组内保持结果行的顺序。
fn group_by_table<'a>(rows: &'a MetaRows) -> HashMap<String, Vec<MetaRow<'a>>> {
    let mut map: HashMap<String, Vec<MetaRow<'a>>> = HashMap::new();
    for r in rows.iter() {
        map.entry(r.name("table_name")).or_default().push(r);
    }
    map
}

/// 按某一列的值把相邻、同名的行归成一组（结果已按该列排序）。
fn runs<'a>(rows: &[MetaRow<'a>], column: &str) -> Vec<Vec<MetaRow<'a>>> {
    let mut out: Vec<Vec<MetaRow<'a>>> = Vec::new();
    for r in rows {
        match out.last_mut() {
            Some(run) if run[0].str(column) == r.str(column) => run.push(*r),
            _ => out.push(vec![*r]),
        }
    }
    out
}

fn column(d: &Dialect, r: &MetaRow) -> Column {
    // Informix 的类型是编码后的数字，见 `coltype`
    let decoded = (d.family() == Family::Informix).then(|| {
        let (coltype, len) = (r.int("coltype").unwrap_or(0), r.int("collength").unwrap_or(0));
        super::coltype::decode(coltype, len, r.text("extended_type").as_deref())
    });
    let native = match &decoded {
        Some(c) => c.type_name.clone(),
        None => r.name("type_text").trim().to_string(),
    };
    Column {
        name: r.name("name"),
        ordinal: r.int("ordinal").and_then(|v| u32::try_from(v).ok()).unwrap_or(0),
        data_type: ColumnType { logical: d.logical_type(&native), native },
        nullable: decoded.as_ref().map_or_else(|| r.flag("nullable"), |c| c.nullable),
        default: match &decoded {
            Some(c) => super::coltype::default_expr(
                r.text("default_kind").as_deref().unwrap_or_default(),
                r.text("default_value").as_deref().unwrap_or_default(),
                &c.type_name,
            ),
            None => r.text("default_value"),
        },
        comment: r.text("comment"),
        auto_increment: r.flag("auto_increment"),
        generated: r.text("generated"),
    }
}

const COLUMN_COLS: [&str; 9] = [
    "table_name",
    "name",
    "ordinal",
    "type_text",
    "nullable",
    "default_value",
    "comment",
    "auto_increment",
    "generated",
];

/// 组装表明细。只组装 `tables` 结果里出现的表；`columns` 里视图的列会被忽略。
pub fn tables(d: &Dialect, rows: &TableRows) -> Result<Vec<Table>> {
    rows.tables.require("tables", &["name", "comment", "partition"])?;
    rows.columns.require("columns", &COLUMN_COLS)?;
    rows.keys.require(
        "keys",
        &[
            "table_name",
            "constraint_name",
            "constraint_type",
            "column_name",
            "position",
            "ref_schema",
            "ref_table",
            "ref_column",
            "on_delete",
            "on_update",
        ],
    )?;
    rows.checks.require("checks", &["table_name", "constraint_name", "expression"])?;
    rows.indexes.require(
        "indexes",
        &["table_name", "index_name", "is_unique", "method", "column_expr", "position", "descending"],
    )?;
    rows.triggers.require("triggers", &["table_name", "name", "timing", "events", "definition"])?;

    let columns = group_by_table(&rows.columns);
    let keys = group_by_table(&rows.keys);
    let checks = group_by_table(&rows.checks);
    let indexes = group_by_table(&rows.indexes);
    let triggers = group_by_table(&rows.triggers);
    let none: Vec<MetaRow> = Vec::new();

    Ok(rows
        .tables
        .iter()
        .map(|t| {
            let name = t.name("name");
            let mut table = Table {
                comment: t.text("comment"),
                partition: t.text("partition"),
                columns: columns.get(&name).unwrap_or(&none).iter().map(|r| column(d, r)).collect(),
                ..Default::default()
            };
            for run in runs(keys.get(&name).unwrap_or(&none), "constraint_name") {
                let first = run[0];
                let cname = first.text("constraint_name");
                let cols: Vec<String> = run.iter().map(|r| r.name("column_name")).collect();
                match first.name("constraint_type").trim() {
                    "P" => table.primary_key = Some(PrimaryKey { name: cname, columns: cols }),
                    "U" => table.uniques.push(UniqueConstraint { name: cname, columns: cols }),
                    "F" => table.foreign_keys.push(ForeignKey {
                        name: cname,
                        columns: cols,
                        ref_schema: first.text("ref_schema"),
                        ref_table: first.name("ref_table"),
                        ref_columns: run.iter().map(|r| r.name("ref_column")).collect(),
                        on_delete: first.text("on_delete"),
                        on_update: first.text("on_update"),
                    }),
                    _ => {}
                }
            }
            let check_rows = checks.get(&name).unwrap_or(&none);
            // Informix 的检查约束文本按块存放，一块一行
            let check_runs = if d.family() == Family::Informix {
                runs(check_rows, "constraint_name")
            } else {
                check_rows.iter().map(|r| vec![*r]).collect()
            };
            table.checks = check_runs
                .into_iter()
                .map(|run| CheckConstraint {
                    name: run[0].text("constraint_name"),
                    expression: run
                        .iter()
                        .filter_map(|r| r.str("expression"))
                        .collect::<String>()
                        .trim()
                        .to_string(),
                })
                .filter(|c| !is_not_null_check(&c.expression))
                .collect();
            table.indexes = runs(indexes.get(&name).unwrap_or(&none), "index_name")
                .into_iter()
                .map(|run| Index {
                    name: run[0].name("index_name"),
                    unique: run[0].flag("is_unique"),
                    method: run[0].text("method"),
                    columns: run
                        .iter()
                        .map(|r| IndexColumn {
                            // Oracle 12.2 以前函数索引的表达式另放在 LONG 列 column_expression 中
                            name: unquote_column(
                                r.text("column_expression").unwrap_or_else(|| r.name("column_expr")),
                            ),
                            descending: r.flag("descending"),
                        })
                        .collect(),
                })
                .collect();
            table.triggers = triggers
                .get(&name)
                .unwrap_or(&none)
                .iter()
                .map(|r| Trigger {
                    name: r.name("name"),
                    timing: r.text("timing"),
                    events: r
                        .name("events")
                        .split(',')
                        .map(|e| e.trim().to_string())
                        .filter(|e| !e.is_empty())
                        .collect(),
                    definition: r.text("definition"),
                })
                .collect();
            table.name = name;
            table
        })
        .collect())
}

/// Oracle 的降序索引列以表达式 `"CREATED"` 的形式出现，只是一个带引号的列名时去掉引号。
fn unquote_column(expr: String) -> String {
    match expr.strip_prefix('"').and_then(|e| e.strip_suffix('"')) {
        Some(inner) if !inner.is_empty() && !inner.contains('"') => inner.to_string(),
        _ => expr,
    }
}

/// Oracle 12.2 以前，NOT NULL 也以 `"COL" IS NOT NULL` 形式的检查约束出现，这里统一过滤掉。
fn is_not_null_check(expression: &str) -> bool {
    expression
        .strip_suffix(" IS NOT NULL")
        .and_then(|c| c.strip_prefix('"'))
        .and_then(|c| c.strip_suffix('"'))
        .is_some_and(|c| !c.contains('"'))
}

/// 组装视图（或物化视图）及其列；定义 SQL 另用 `view_definition` 按需获取。
pub fn views(d: &Dialect, key: MetaKey, views: &MetaRows, columns: &MetaRows) -> Result<Vec<View>> {
    views.require(key.as_str(), &["name", "comment"])?;
    columns.require("columns", &COLUMN_COLS)?;
    let columns = group_by_table(columns);
    Ok(views
        .iter()
        .map(|v| {
            let name = v.name("name");
            View {
                comment: v.text("comment"),
                definition: None,
                columns: columns
                    .get(&name)
                    .map(|rs| rs.iter().map(|r| column(d, r)).collect())
                    .unwrap_or_default(),
                name,
            }
        })
        .collect())
}

pub fn sequences(rows: &MetaRows) -> Result<Vec<Sequence>> {
    rows.require(
        "sequences",
        &["name", "start_value", "increment", "min_value", "max_value", "cycle", "cache"],
    )?;
    Ok(rows
        .iter()
        .map(|r| Sequence {
            name: r.name("name"),
            start: r.text("start_value"),
            increment: r.text("increment"),
            min_value: r.text("min_value"),
            max_value: r.text("max_value"),
            cycle: r.flag("cycle"),
            cache: r.int("cache").and_then(|c| u64::try_from(c).ok()),
        })
        .collect())
}

/// 存储过程、函数、包。不认识的种类跳过。源码另用 `routine_source` 按需获取。
pub fn routines(rows: &MetaRows) -> Result<Vec<Routine>> {
    rows.require("routines", &["name", "kind", "comment"])?;
    Ok(rows
        .iter()
        .filter_map(|r| {
            let kind = match r.name("kind").trim().to_ascii_uppercase().as_str() {
                "PROCEDURE" => RoutineKind::Procedure,
                "FUNCTION" => RoutineKind::Function,
                "PACKAGE" => RoutineKind::Package,
                "PACKAGE BODY" => RoutineKind::PackageBody,
                _ => return None,
            };
            Some(Routine { name: r.name("name"), kind, source: None })
        })
        .collect())
}

pub fn synonyms(rows: &MetaRows) -> Result<Vec<Synonym>> {
    rows.require("synonyms", &["name", "target_schema", "target_name", "db_link"])?;
    Ok(rows
        .iter()
        .map(|r| Synonym {
            name: r.name("name"),
            target_schema: r.text("target_schema"),
            target_name: r.name("target_name"),
            db_link: r.text("db_link"),
        })
        .collect())
}

/// 视图定义或程序源码：`view_definition` 取第一行的 `definition`；
/// `routine_source` 按行顺序拼接 `text`（每行自带换行，原样拼接）。找不到对象时返回 None。
pub fn source_text(key: MetaKey, rows: &MetaRows) -> Result<Option<String>> {
    match key {
        MetaKey::RoutineSource => {
            rows.require("routine_source", &["line", "text"])?;
            if rows.is_empty() {
                return Ok(None);
            }
            Ok(Some(rows.iter().map(|r| r.str("text").unwrap_or_default()).collect()))
        }
        _ => {
            rows.require(key.as_str(), &["definition"])?;
            // 一般只有一行；Informix 的视图文本按块存放，一块一行
            let chunks: Vec<String> = rows.iter().filter_map(|r| r.str("definition")).collect();
            Ok((!chunks.is_empty()).then(|| chunks.concat().trim_end().to_string()))
        }
    }
}

/// 服务器报告的保留字，统一转成大写。
pub fn reserved_words(rows: &MetaRows) -> Result<Vec<String>> {
    rows.require("reserved_words", &["word"])?;
    Ok(rows.iter().map(|r| r.name("word").trim().to_ascii_uppercase()).filter(|w| !w.is_empty()).collect())
}

/// 其他表指向某张表的外键（`referencing_keys`）：外键所在的 schema、表和外键本身。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ReferencingKey {
    pub schema: String,
    pub table: String,
    pub key: ForeignKey,
}

/// 依赖某张表的视图（`dependent_views`）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DependentView {
    pub schema: String,
    pub name: String,
    pub materialized: bool,
}

/// 表上的一项授权（`grants`）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Grant {
    pub grantee: String,
    pub privilege: String,
    pub grantable: bool,
}

/// 存储属性和行数估计（`table_storage`）。统计信息没有收集时行数为 None。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct TableStorage {
    pub engine: Option<String>,
    pub charset: Option<String>,
    pub collation: Option<String>,
    pub tablespace: Option<String>,
    pub row_count: Option<i64>,
    /// 主键是否聚簇（只有 TiDB 报告）：聚簇主键不能直接删除或更换，表设计器改为重建。
    pub clustered: Option<bool>,
}

/// `referencing_keys` 的结果：`ref_schema` / `ref_table` 是被引用的这张表。
pub fn referencing_keys(rows: &MetaRows, ref_schema: &str, ref_table: &str) -> Result<Vec<ReferencingKey>> {
    rows.require(
        "referencing_keys",
        &[
            "table_schema",
            "table_name",
            "constraint_name",
            "column_name",
            "position",
            "ref_column",
            "on_delete",
        ],
    )?;
    let mut out: Vec<ReferencingKey> = Vec::new();
    for r in rows.iter() {
        let (schema, table, name) = (r.name("table_schema"), r.name("table_name"), r.name("constraint_name"));
        match out.last_mut() {
            Some(k)
                if k.schema == schema && k.table == table && k.key.name.as_deref() == Some(name.as_str()) =>
            {
                k.key.columns.push(r.name("column_name"));
                k.key.ref_columns.push(r.name("ref_column"));
            }
            _ => out.push(ReferencingKey {
                schema,
                table,
                key: ForeignKey {
                    name: Some(name),
                    columns: vec![r.name("column_name")],
                    ref_schema: Some(ref_schema.to_string()),
                    ref_table: ref_table.to_string(),
                    ref_columns: vec![r.name("ref_column")],
                    on_delete: r.text("on_delete"),
                    on_update: r.text("on_update"),
                },
            }),
        }
    }
    Ok(out)
}

pub fn dependent_views(rows: &MetaRows) -> Result<Vec<DependentView>> {
    rows.require("dependent_views", &["schema_name", "name", "kind"])?;
    Ok(rows
        .iter()
        .map(|r| DependentView {
            schema: r.name("schema_name"),
            name: r.name("name"),
            materialized: r.name("kind").to_ascii_uppercase().starts_with("MATERIALIZED"),
        })
        .collect())
}

pub fn grants(rows: &MetaRows) -> Result<Vec<Grant>> {
    rows.require("grants", &["grantee", "privilege", "grantable"])?;
    Ok(rows
        .iter()
        .map(|r| Grant {
            grantee: r.name("grantee"),
            privilege: r.name("privilege"),
            grantable: r.flag("grantable"),
        })
        .collect())
}

/// 第一行；没有结果（表不存在）时为默认值。
pub fn table_storage(rows: &MetaRows) -> Result<TableStorage> {
    rows.require("table_storage", &["engine", "charset", "collation", "tablespace", "row_count"])?;
    Ok(rows
        .iter()
        .next()
        .map(|r| TableStorage {
            engine: r.text("engine"),
            charset: r.text("charset"),
            collation: r.text("collation"),
            tablespace: r.text("tablespace"),
            row_count: r.int("row_count").filter(|n| *n >= 0),
            clustered: r.columns().iter().any(|c| c == "clustered").then(|| r.flag("clustered")),
        })
        .unwrap_or_default())
}

/// `trigger_ddl`（Oracle）：每个触发器拼成一条完整的 CREATE TRIGGER。
pub fn trigger_ddl(rows: &MetaRows) -> Result<Vec<String>> {
    rows.require("trigger_ddl", &["name", "head", "body"])?;
    Ok(rows.iter().map(|r| format!("{}{}", r.name("head"), r.name("body")).trim_end().to_string()).collect())
}

#[cfg(test)]
mod tests {
    use dblens_core::{LogicalType, Value};

    use super::*;
    use crate::Variant;

    fn s(v: &str) -> Value {
        Value::String(v.into())
    }

    fn rows(cols: &[&str], data: Vec<Vec<Value>>) -> MetaRows {
        MetaRows::new(cols.iter().map(|c| c.to_string()).collect(), data)
    }

    fn fixture() -> TableRows {
        let n = Value::Null;
        TableRows {
            tables: rows(
                &["name", "comment", "partition"],
                vec![vec![s("dept"), s("部门"), n.clone()], vec![s("emp"), s(""), s("RANGE (created)")]],
            ),
            columns: rows(
                &COLUMN_COLS,
                vec![
                    vec![
                        s("dept"),
                        s("id"),
                        Value::Int(1),
                        s("int"),
                        s("NO"),
                        n.clone(),
                        s("主键"),
                        Value::Int(1),
                        n.clone(),
                    ],
                    vec![
                        s("emp"),
                        s("id"),
                        Value::Decimal("1".into()),
                        s("bigint"),
                        s("N"),
                        n.clone(),
                        n.clone(),
                        s("N"),
                        n.clone(),
                    ],
                    vec![
                        s("emp"),
                        s("name"),
                        Value::Int(2),
                        s("varchar(100)"),
                        s("Y"),
                        s("'无名'\n"),
                        s("姓名"),
                        s("N"),
                        n.clone(),
                    ],
                    vec![
                        s("emp"),
                        s("name_upper"),
                        Value::Int(3),
                        s("varchar(100)"),
                        s("YES"),
                        n.clone(),
                        n.clone(),
                        s("N"),
                        s("upper(name)"),
                    ],
                    vec![
                        s("v_emp"),
                        s("id"),
                        Value::Int(1),
                        s("bigint"),
                        s("YES"),
                        n.clone(),
                        n.clone(),
                        s("N"),
                        n.clone(),
                    ],
                ],
            ),
            keys: rows(
                &[
                    "table_name",
                    "constraint_name",
                    "constraint_type",
                    "column_name",
                    "position",
                    "ref_schema",
                    "ref_table",
                    "ref_column",
                    "on_delete",
                    "on_update",
                ],
                vec![
                    vec![
                        s("dept"),
                        s("pk_dept"),
                        s("P"),
                        s("id"),
                        Value::Int(1),
                        n.clone(),
                        n.clone(),
                        n.clone(),
                        n.clone(),
                        n.clone(),
                    ],
                    vec![
                        s("emp"),
                        s("fk_emp_dept"),
                        s("F"),
                        s("dept_id"),
                        Value::Int(1),
                        s("hr"),
                        s("dept"),
                        s("id"),
                        s("CASCADE"),
                        s("NO ACTION"),
                    ],
                    vec![
                        s("emp"),
                        s("pk_emp"),
                        s("P"),
                        s("id"),
                        Value::Int(1),
                        n.clone(),
                        n.clone(),
                        n.clone(),
                        n.clone(),
                        n.clone(),
                    ],
                    vec![
                        s("emp"),
                        s("uk_emp"),
                        s("U"),
                        s("dept_id"),
                        Value::Int(1),
                        n.clone(),
                        n.clone(),
                        n.clone(),
                        n.clone(),
                        n.clone(),
                    ],
                    vec![
                        s("emp"),
                        s("uk_emp"),
                        s("U"),
                        s("name"),
                        Value::Int(2),
                        n.clone(),
                        n.clone(),
                        n.clone(),
                        n.clone(),
                        n.clone(),
                    ],
                ],
            ),
            checks: rows(
                &["table_name", "constraint_name", "expression"],
                vec![
                    vec![s("emp"), s("SYS_C001"), s("\"NAME\" IS NOT NULL")],
                    vec![s("emp"), s("ck_salary"), s("salary >= 0 ")],
                ],
            ),
            indexes: rows(
                &["table_name", "index_name", "is_unique", "method", "column_expr", "position", "descending"],
                vec![
                    vec![s("emp"), s("idx_a"), s("N"), s("BTREE"), s("created"), Value::Int(1), s("Y")],
                    vec![s("emp"), s("idx_a"), s("N"), s("BTREE"), s("id"), Value::Int(2), s("N")],
                    vec![
                        s("emp"),
                        s("idx_b"),
                        Value::Bool(true),
                        n.clone(),
                        s("upper(name)"),
                        Value::Int(1),
                        Value::Bool(false),
                    ],
                ],
            ),
            triggers: rows(
                &["table_name", "name", "timing", "events", "definition"],
                vec![vec![s("emp"), s("trg_emp"), s("BEFORE"), s("INSERT,UPDATE"), s("BEGIN END")]],
            ),
        }
    }

    #[test]
    fn assembles_tables_with_all_parts() {
        let d = Dialect::new(Variant::MySql);
        let t = tables(&d, &fixture()).unwrap();
        assert_eq!(t.len(), 2);
        let dept = &t[0];
        assert_eq!(dept.name, "dept");
        assert_eq!(dept.comment.as_deref(), Some("部门"));
        assert_eq!(dept.columns.len(), 1);
        assert!(dept.columns[0].auto_increment && !dept.columns[0].nullable);
        assert_eq!(dept.columns[0].data_type.logical, LogicalType::Int { bits: 32 });
        assert_eq!(dept.primary_key.as_ref().unwrap().columns, vec!["id"]);

        let emp = &t[1];
        assert_eq!(emp.comment, None, "空注释视为没有注释");
        assert_eq!(emp.partition.as_deref(), Some("RANGE (created)"));
        assert_eq!(
            emp.columns.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(),
            ["id", "name", "name_upper"]
        );
        assert_eq!(emp.columns[1].default.as_deref(), Some("'无名'"));
        assert_eq!(emp.columns[2].generated.as_deref(), Some("upper(name)"));
        assert_eq!(emp.primary_key.as_ref().unwrap().name.as_deref(), Some("pk_emp"));
        assert_eq!(emp.uniques[0].columns, vec!["dept_id", "name"]);
        let fk = &emp.foreign_keys[0];
        assert_eq!((fk.ref_schema.as_deref(), fk.ref_table.as_str()), (Some("hr"), "dept"));
        assert_eq!(fk.ref_columns, vec!["id"]);
        assert_eq!((fk.on_delete.as_deref(), fk.on_update.as_deref()), (Some("CASCADE"), Some("NO ACTION")));
        assert_eq!(emp.checks.len(), 1, "Oracle 旧版本的 NOT NULL 检查约束被过滤");
        assert_eq!(emp.checks[0].expression, "salary >= 0");
        assert_eq!(emp.indexes.len(), 2);
        assert_eq!(emp.indexes[0].columns.len(), 2);
        assert!(emp.indexes[0].columns[0].descending && !emp.indexes[0].columns[1].descending);
        assert!(emp.indexes[1].unique && emp.indexes[1].method.is_none());
        assert_eq!(emp.triggers[0].events, vec!["INSERT", "UPDATE"]);
    }

    #[test]
    fn index_expression_column_wins_when_present() {
        let d = Dialect::new(Variant::Oracle);
        let mut f = fixture();
        f.indexes = rows(
            &[
                "table_name",
                "index_name",
                "is_unique",
                "method",
                "column_expr",
                "position",
                "descending",
                "column_expression",
            ],
            vec![vec![
                s("emp"),
                s("ix"),
                s("N"),
                s("NORMAL"),
                s("SYS_NC00010$"),
                Value::Int(1),
                s("N"),
                s("UPPER(\"NAME\") "),
            ]],
        );
        let t = tables(&d, &f).unwrap();
        assert_eq!(t[1].indexes[0].columns[0].name, "UPPER(\"NAME\")");
        assert_eq!(unquote_column("\"CREATED\"".into()), "CREATED");
        assert_eq!(unquote_column("\"A\"||\"B\"".into()), "\"A\"||\"B\"");
        assert!(!is_not_null_check("\"A\" IS NOT NULL OR \"B\" IS NOT NULL"));
    }

    #[test]
    fn views_take_their_columns() {
        let d = Dialect::new(Variant::MySql);
        let f = fixture();
        let v = rows(&["name", "comment"], vec![vec![s("v_emp"), Value::Null]]);
        let views = views(&d, MetaKey::Views, &v, &f.columns).unwrap();
        assert_eq!(views[0].columns.len(), 1);
        assert_eq!(views[0].definition, None);
    }

    #[test]
    fn missing_column_in_results_is_an_error() {
        let d = Dialect::new(Variant::MySql);
        let mut f = fixture();
        f.checks = rows(&["table_name", "constraint_name"], vec![]);
        let err = tables(&d, &f).unwrap_err().to_string();
        assert!(err.contains("checks") && err.contains("expression"), "{err}");
    }

    #[test]
    fn lists_sequences_routines_synonyms_and_sources() {
        let seq = rows(
            &["name", "start_value", "increment", "min_value", "max_value", "cycle", "cache"],
            vec![vec![
                s("seq"),
                Value::Decimal("100".into()),
                s("5"),
                s("1"),
                s("9999999999999999999999999999"),
                s("Y"),
                Value::Decimal("10".into()),
            ]],
        );
        let seq = sequences(&seq).unwrap();
        assert_eq!(seq[0].max_value.as_deref(), Some("9999999999999999999999999999"));
        assert!(seq[0].cycle);
        assert_eq!(seq[0].cache, Some(10));

        let r = rows(
            &["name", "kind", "comment"],
            vec![
                vec![s("p"), s("PROCEDURE"), Value::Null],
                vec![s("pkg"), s("package body"), Value::Null],
                vec![s("x"), s("TYPE"), Value::Null],
            ],
        );
        let r = routines(&r).unwrap();
        assert_eq!(r.len(), 2);
        assert_eq!(r[1].kind, RoutineKind::PackageBody);

        let syn = rows(
            &["name", "target_schema", "target_name", "db_link"],
            vec![vec![s("syn"), s("HR"), s("EMP"), Value::Null]],
        );
        assert_eq!(synonyms(&syn).unwrap()[0].target_name, "EMP");

        let src = rows(
            &["line", "text"],
            vec![vec![Value::Int(1), s("PROCEDURE p IS\n")], vec![Value::Int(2), s("BEGIN NULL; END;\n")]],
        );
        assert_eq!(
            source_text(MetaKey::RoutineSource, &src).unwrap().as_deref(),
            Some("PROCEDURE p IS\nBEGIN NULL; END;\n")
        );
        assert_eq!(source_text(MetaKey::RoutineSource, &rows(&["line", "text"], vec![])).unwrap(), None);
        let def = rows(&["definition"], vec![vec![s("select 1")]]);
        assert_eq!(source_text(MetaKey::ViewDefinition, &def).unwrap().as_deref(), Some("select 1"));

        let words = rows(&["word"], vec![vec![s("select")], vec![s(" ")]]);
        assert_eq!(reserved_words(&words).unwrap(), vec!["SELECT"]);
    }

    #[test]
    fn schema_and_object_lists() {
        let sc = rows(&["name", "is_system"], vec![vec![s("hr"), s("N")], vec![s("SYS"), Value::Int(1)]]);
        let sc = schemas(&sc).unwrap();
        assert!(!sc[0].is_system && sc[1].is_system);
        let ob = rows(&["name", "comment"], vec![vec![s("t"), s(" 说明 ")]]);
        assert_eq!(objects(MetaKey::Tables, &ob).unwrap()[0].comment.as_deref(), Some("说明"));
    }
}
