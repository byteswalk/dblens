//! 文档的内容（M4c 设计文档第 4 节）：从模型或库的结构整理出来，四种格式都从它写出，模板拿到的也是它。

use std::collections::BTreeMap;

use dblens_core::schema::{Column, ForeignKey, Table, View};
use serde::{Deserialize, Serialize};

use crate::model::{FileError, LogicalKey};

/// 文档种类。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DocKind {
    /// 数据字典：表清单和表明细。
    Dictionary,
    /// 设计文档：再加概述、ER 图、表间关系、视图。
    Design,
}

/// 文档信息：标题、版本等由用户填写，生成时间和来源由程序给出。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocInfo {
    pub title: String,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub author: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    /// 生成时间（本地时间，`2026-10-28 10:00`）。
    #[serde(default)]
    pub generated: String,
    /// 来源：模型名和目录，或连接名和库名。
    #[serde(default)]
    pub source: String,
}

/// 文档的全部内容。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DocContent {
    pub kind: DocKind,
    pub info: DocInfo,
    pub stats: DocStats,
    pub schemas: Vec<DocSchema>,
    /// 设计文档的 ER 图。
    pub diagrams: Vec<DocDiagram>,
    /// 设计文档的表间关系。
    pub relations: Vec<DocRelation>,
    /// 模型中读不出的表。
    pub unreadable: Vec<DocUnreadable>,
    /// 设计文档中是否放进视图的定义。
    pub view_definitions: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct DocStats {
    pub schemas: usize,
    pub tables: usize,
    pub columns: usize,
    pub foreign_keys: usize,
    pub logical_keys: usize,
    pub views: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DocSchema {
    pub name: String,
    pub tables: Vec<DocTable>,
    pub views: Vec<DocView>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DocTable {
    pub schema: String,
    pub name: String,
    /// 文档内的锚点（`t1`、`t2`……），html 的 id、docx 的书签、xlsx 的定位都用它。
    pub anchor: String,
    pub comment: Option<String>,
    pub partition: Option<String>,
    pub columns: Vec<DocColumn>,
    pub indexes: Vec<DocIndex>,
    pub constraints: Vec<DocConstraint>,
    pub foreign_keys: Vec<DocKey>,
    pub logical_keys: Vec<DocKey>,
    pub triggers: Vec<DocTrigger>,
    pub referenced_by: Vec<DocReference>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DocColumn {
    pub ordinal: usize,
    pub name: String,
    pub data_type: String,
    pub nullable: bool,
    pub default: Option<String>,
    /// 标记：`pk`（主键）、`auto`（自增）、`generated`（生成列）；文字由 `t('doc.flag.<标记>')` 取。
    pub flags: Vec<&'static str>,
    pub comment: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DocIndex {
    pub name: String,
    /// 列，以 `, ` 分隔；降序的列后面加 ` DESC`。
    pub columns: String,
    pub unique: bool,
    pub method: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DocConstraint {
    /// `primary`、`unique`、`check`。
    pub kind: &'static str,
    pub name: Option<String>,
    /// 列（主键、唯一）或表达式（检查）。
    pub detail: String,
}

/// 外键或逻辑外键。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DocKey {
    pub name: Option<String>,
    pub columns: String,
    pub ref_schema: String,
    pub ref_table: String,
    /// 引用的表在文档中时的锚点。
    pub ref_anchor: Option<String>,
    pub ref_columns: String,
    pub on_delete: Option<String>,
    pub on_update: Option<String>,
    pub comment: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DocTrigger {
    pub name: String,
    pub timing: Option<String>,
    pub events: String,
}

/// 指向这张表的外键或逻辑外键。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DocReference {
    pub schema: String,
    pub table: String,
    pub anchor: Option<String>,
    pub columns: String,
    pub ref_columns: String,
    /// `foreign_key` 或 `logical_key`。
    pub kind: &'static str,
    pub name: Option<String>,
}

/// 表间关系（设计文档）。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DocRelation {
    pub from_schema: String,
    pub from_table: String,
    pub from_anchor: Option<String>,
    pub from_columns: String,
    pub to_schema: String,
    pub to_table: String,
    pub to_anchor: Option<String>,
    pub to_columns: String,
    pub kind: &'static str,
    pub name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DocView {
    pub name: String,
    pub anchor: String,
    pub comment: Option<String>,
    pub columns: Vec<DocColumn>,
    pub definition: Option<String>,
}

/// 一张 ER 图的图片（PNG）。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DocDiagram {
    pub schema: String,
    /// 图名（默认图为 `_all`）。
    pub name: String,
    /// 显示的标题：默认图为 schema 名，主题图为图名。
    pub title: String,
    pub description: Option<String>,
    /// 表较多、只画了表名。
    pub names_only: bool,
    /// 图片的文件名（`01-<schema>-<图名>.png`）。
    pub file: String,
    /// 图片的引用：html 为 data URL，markdown 为相对路径；docx、xlsx 不用。
    pub src: String,
    /// 像素尺寸。
    pub width: u32,
    pub height: u32,
    #[serde(skip)]
    pub png: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DocUnreadable {
    pub schema: String,
    pub name: String,
    pub path: String,
    pub message: String,
}

/// 一个 schema 的结构（组织内容的输入）。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SourceSchema {
    pub name: String,
    pub tables: Vec<SourceTable>,
    pub views: Vec<View>,
    /// 模型中读不出的表：表名和原因。
    pub errors: Vec<(String, FileError)>,
}

/// 一张表和它的逻辑外键（来源是库时没有逻辑外键）。
#[derive(Debug, Clone, PartialEq)]
pub struct SourceTable {
    pub table: Table,
    pub logical_keys: Vec<LogicalKey>,
}

/// 组织内容的选项。
#[derive(Debug, Clone, PartialEq)]
pub struct BuildOptions {
    pub kind: DocKind,
    pub info: DocInfo,
    /// 只放这些表（只选了一个 schema 时可用）；None 为全部。
    pub tables: Option<Vec<String>>,
    pub view_definitions: bool,
}

fn join(cols: &[String]) -> String {
    cols.join(", ")
}

fn column(c: &Column, ordinal: usize, pk: &[String]) -> DocColumn {
    let mut flags = Vec::new();
    if pk.contains(&c.name) {
        flags.push("pk");
    }
    if c.auto_increment {
        flags.push("auto");
    }
    if c.generated.is_some() {
        flags.push("generated");
    }
    DocColumn {
        ordinal,
        name: c.name.clone(),
        data_type: c.data_type.native.clone(),
        nullable: c.nullable,
        default: c.generated.clone().or_else(|| c.default.clone()),
        flags,
        comment: c.comment.clone(),
    }
}

/// 外键和逻辑外键共用的字段。
struct KeyRef<'a> {
    name: &'a Option<String>,
    columns: &'a [String],
    ref_schema: &'a Option<String>,
    ref_table: &'a str,
    ref_columns: &'a [String],
}

impl<'a> From<&'a ForeignKey> for KeyRef<'a> {
    fn from(f: &'a ForeignKey) -> Self {
        KeyRef {
            name: &f.name,
            columns: &f.columns,
            ref_schema: &f.ref_schema,
            ref_table: &f.ref_table,
            ref_columns: &f.ref_columns,
        }
    }
}

impl<'a> From<&'a LogicalKey> for KeyRef<'a> {
    fn from(k: &'a LogicalKey) -> Self {
        KeyRef {
            name: &k.name,
            columns: &k.columns,
            ref_schema: &k.ref_schema,
            ref_table: &k.ref_table,
            ref_columns: &k.ref_columns,
        }
    }
}

/// 组织文档内容。`diagrams` 是设计文档的 ER 图（数据字典时忽略）。
pub fn build(opts: BuildOptions, sources: Vec<SourceSchema>, diagrams: Vec<DocDiagram>) -> DocContent {
    let design = opts.kind == DocKind::Design;
    // 先定下全部表的锚点：外键、被引用可以链接到文档中的表
    let mut anchors: BTreeMap<(String, String), String> = BTreeMap::new();
    let mut picked: Vec<(String, Vec<&SourceTable>)> = Vec::new();
    for s in &sources {
        let mut tables: Vec<&SourceTable> = s
            .tables
            .iter()
            .filter(|t| opts.tables.as_ref().is_none_or(|names| names.contains(&t.table.name)))
            .collect();
        tables.sort_by(|a, b| a.table.name.cmp(&b.table.name));
        for SourceTable { table: t, .. } in &tables {
            let anchor = format!("t{}", anchors.len() + 1);
            anchors.insert((s.name.clone(), t.name.clone()), anchor);
        }
        picked.push((s.name.clone(), tables));
    }
    let anchor_of =
        |schema: &str, table: &str| anchors.get(&(schema.to_string(), table.to_string())).cloned();

    let key = |schema: &str, k: KeyRef, on_delete: Option<String>, on_update: Option<String>, comment| {
        let ref_schema = k.ref_schema.clone().unwrap_or_else(|| schema.to_string());
        DocKey {
            name: k.name.clone(),
            columns: join(k.columns),
            ref_anchor: anchor_of(&ref_schema, k.ref_table),
            ref_schema,
            ref_table: k.ref_table.to_string(),
            ref_columns: join(k.ref_columns),
            on_delete,
            on_update,
            comment,
        }
    };

    let mut stats = DocStats::default();
    let mut relations = Vec::new();
    // 被引用：(schema, 表) → 指向它的引用
    let mut referenced: BTreeMap<(String, String), Vec<DocReference>> = BTreeMap::new();
    let mut schemas = Vec::new();
    let mut view_count = 0;
    for ((schema, tables), source) in picked.into_iter().zip(&sources) {
        let mut out = Vec::new();
        for SourceTable { table: t, logical_keys: logical } in tables {
            let pk = t.primary_key.as_ref().map(|p| p.columns.clone()).unwrap_or_default();
            let mut constraints = Vec::new();
            if let Some(p) = &t.primary_key {
                constraints.push(DocConstraint {
                    kind: "primary",
                    name: p.name.clone(),
                    detail: join(&p.columns),
                });
            }
            for u in &t.uniques {
                constraints.push(DocConstraint {
                    kind: "unique",
                    name: u.name.clone(),
                    detail: join(&u.columns),
                });
            }
            for c in &t.checks {
                constraints.push(DocConstraint {
                    kind: "check",
                    name: c.name.clone(),
                    detail: c.expression.clone(),
                });
            }
            let foreign_keys: Vec<DocKey> = t
                .foreign_keys
                .iter()
                .map(|f| key(&schema, f.into(), f.on_delete.clone(), f.on_update.clone(), None))
                .collect();
            let logical_keys: Vec<DocKey> =
                logical.iter().map(|k| key(&schema, k.into(), None, None, k.comment.clone())).collect();
            let anchor = anchor_of(&schema, &t.name);
            for (keys, kind) in [(&foreign_keys, "foreign_key"), (&logical_keys, "logical_key")] {
                for k in keys {
                    referenced.entry((k.ref_schema.clone(), k.ref_table.clone())).or_default().push(
                        DocReference {
                            schema: schema.clone(),
                            table: t.name.clone(),
                            anchor: anchor.clone(),
                            columns: k.columns.clone(),
                            ref_columns: k.ref_columns.clone(),
                            kind,
                            name: k.name.clone(),
                        },
                    );
                    if design {
                        relations.push(DocRelation {
                            from_schema: schema.clone(),
                            from_table: t.name.clone(),
                            from_anchor: anchor.clone(),
                            from_columns: k.columns.clone(),
                            to_schema: k.ref_schema.clone(),
                            to_table: k.ref_table.clone(),
                            to_anchor: k.ref_anchor.clone(),
                            to_columns: k.ref_columns.clone(),
                            kind,
                            name: k.name.clone(),
                        });
                    }
                }
            }
            stats.tables += 1;
            stats.columns += t.columns.len();
            stats.foreign_keys += foreign_keys.len();
            stats.logical_keys += logical_keys.len();
            out.push(DocTable {
                schema: schema.clone(),
                name: t.name.clone(),
                anchor: anchor.unwrap_or_default(),
                comment: t.comment.clone(),
                partition: t.partition.clone(),
                columns: t.columns.iter().enumerate().map(|(i, c)| column(c, i + 1, &pk)).collect(),
                indexes: t
                    .indexes
                    .iter()
                    .map(|i| DocIndex {
                        name: i.name.clone(),
                        columns: i
                            .columns
                            .iter()
                            .map(|c| if c.descending { format!("{} DESC", c.name) } else { c.name.clone() })
                            .collect::<Vec<_>>()
                            .join(", "),
                        unique: i.unique,
                        method: i.method.clone(),
                    })
                    .collect(),
                constraints,
                foreign_keys,
                logical_keys,
                triggers: t
                    .triggers
                    .iter()
                    .map(|g| DocTrigger {
                        name: g.name.clone(),
                        timing: g.timing.clone(),
                        events: g.events.join(", "),
                    })
                    .collect(),
                referenced_by: vec![],
            });
        }
        let views: Vec<DocView> = if design {
            let mut vs: Vec<&View> = source.views.iter().collect();
            vs.sort_by(|a, b| a.name.cmp(&b.name));
            vs.into_iter()
                .map(|v| {
                    view_count += 1;
                    DocView {
                        name: v.name.clone(),
                        anchor: format!("v{view_count}"),
                        comment: v.comment.clone(),
                        columns: v.columns.iter().enumerate().map(|(i, c)| column(c, i + 1, &[])).collect(),
                        definition: if opts.view_definitions { v.definition.clone() } else { None },
                    }
                })
                .collect()
        } else {
            vec![]
        };
        stats.views += views.len();
        schemas.push(DocSchema { name: schema, tables: out, views });
    }
    for s in &mut schemas {
        for t in &mut s.tables {
            t.referenced_by = referenced.remove(&(t.schema.clone(), t.name.clone())).unwrap_or_default();
        }
    }
    stats.schemas = schemas.len();
    let unreadable = sources
        .iter()
        .flat_map(|s| {
            s.errors.iter().map(|(name, e)| DocUnreadable {
                schema: s.name.clone(),
                name: name.clone(),
                path: e.path.clone(),
                message: e.message.clone(),
            })
        })
        .collect();
    DocContent {
        kind: opts.kind,
        info: opts.info,
        stats,
        schemas,
        diagrams: if design { diagrams } else { vec![] },
        relations,
        unreadable,
        view_definitions: opts.view_definitions,
    }
}

/// PNG 的像素尺寸（读文件头）；不是 PNG 时为 None。
pub fn png_size(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.len() < 24 || &bytes[..8] != b"\x89PNG\r\n\x1a\n" || &bytes[12..16] != b"IHDR" {
        return None;
    }
    let w = u32::from_be_bytes(bytes[16..20].try_into().ok()?);
    let h = u32::from_be_bytes(bytes[20..24].try_into().ok()?);
    Some((w, h))
}

#[cfg(test)]
pub(crate) mod tests {
    use dblens_core::schema::{CheckConstraint, Index, IndexColumn, PrimaryKey, Trigger, UniqueConstraint};
    use dblens_core::{ColumnType, LogicalType};

    use super::*;

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

    pub fn fk(
        name: &str,
        cols: &[&str],
        ref_schema: Option<&str>,
        ref_table: &str,
        ref_cols: &[&str],
    ) -> ForeignKey {
        ForeignKey {
            name: Some(name.into()),
            columns: cols.iter().map(|c| c.to_string()).collect(),
            ref_schema: ref_schema.map(Into::into),
            ref_table: ref_table.into(),
            ref_columns: ref_cols.iter().map(|c| c.to_string()).collect(),
            on_delete: Some("CASCADE".into()),
            on_update: None,
        }
    }

    pub fn st(table: Table, logical_keys: Vec<LogicalKey>) -> SourceTable {
        SourceTable { table, logical_keys }
    }

    /// 测试用的两个 schema：shop（客户、订单、订单明细、地区引用 crm）和 crm（地区）。
    pub fn sample() -> Vec<SourceSchema> {
        let customers = Table {
            name: "customers".into(),
            comment: Some("客户".into()),
            columns: vec![
                Column { auto_increment: true, nullable: false, ..col("id", "int") },
                Column { nullable: false, comment: Some("编号".into()), ..col("code", "varchar(20)") },
                col("region_id", "int"),
            ],
            primary_key: Some(PrimaryKey { name: Some("pk_customers".into()), columns: vec!["id".into()] }),
            uniques: vec![UniqueConstraint { name: Some("uq_code".into()), columns: vec!["code".into()] }],
            foreign_keys: vec![fk("fk_region", &["region_id"], Some("crm"), "regions", &["id"])],
            indexes: vec![Index {
                name: "ix_code".into(),
                columns: vec![IndexColumn { name: "code".into(), descending: true }],
                unique: false,
                method: Some("BTREE".into()),
            }],
            ..Default::default()
        };
        let orders = Table {
            name: "orders".into(),
            columns: vec![
                Column { nullable: false, ..col("id", "int") },
                col("cust_id", "int"),
                col("cust_code", "varchar(20)"),
                Column { default: Some("0".into()), ..col("amount", "decimal(10,2)") },
                Column { generated: Some("amount * 2".into()), ..col("double_amount", "decimal(10,2)") },
            ],
            primary_key: Some(PrimaryKey { name: None, columns: vec!["id".into()] }),
            foreign_keys: vec![fk("fk_cust", &["cust_id", "cust_code"], None, "customers", &["id", "code"])],
            checks: vec![CheckConstraint {
                name: Some("ck_amount".into()),
                expression: "amount >= 0".into(),
            }],
            triggers: vec![Trigger {
                name: "trg_orders".into(),
                timing: Some("BEFORE".into()),
                events: vec!["INSERT".into(), "UPDATE".into()],
                definition: Some("BEGIN END".into()),
            }],
            ..Default::default()
        };
        let items = Table {
            name: "items".into(),
            columns: vec![col("id", "int"), col("order_id", "int"), col("sku", "varchar(30)")],
            ..Default::default()
        };
        let lk = LogicalKey {
            name: Some("lk_order".into()),
            columns: vec!["order_id".into()],
            ref_schema: None,
            ref_table: "orders".into(),
            ref_columns: vec!["id".into()],
            comment: Some("没有建外键".into()),
        };
        let regions = Table {
            name: "regions".into(),
            columns: vec![col("id", "int"), col("name", "varchar(50)")],
            primary_key: Some(PrimaryKey { name: None, columns: vec!["id".into()] }),
            ..Default::default()
        };
        vec![
            SourceSchema {
                name: "shop".into(),
                tables: vec![st(orders, vec![]), st(customers, vec![]), st(items, vec![lk])],
                views: vec![View {
                    name: "v_orders".into(),
                    comment: Some("订单视图".into()),
                    definition: Some("SELECT id FROM orders".into()),
                    columns: vec![col("id", "int")],
                }],
                errors: vec![(
                    "broken".into(),
                    FileError { path: "tables/shop/broken.yaml".into(), message: "line 3: bad".into() },
                )],
            },
            SourceSchema { name: "crm".into(), tables: vec![st(regions, vec![])], ..Default::default() },
        ]
    }

    pub fn opts(kind: DocKind) -> BuildOptions {
        BuildOptions {
            kind,
            info: DocInfo {
                title: "商城".into(),
                version: Some("1.0".into()),
                author: Some("张三".into()),
                description: Some("测试模型".into()),
                generated: "2026-10-28 10:00".into(),
                source: "shop-model".into(),
            },
            tables: None,
            view_definitions: true,
        }
    }

    #[test]
    fn dictionary_tables_keys_and_references() {
        let c = build(opts(DocKind::Dictionary), sample(), vec![]);
        // 表按名字排序，锚点按文档中的顺序
        let names: Vec<_> =
            c.schemas[0].tables.iter().map(|t| (t.name.as_str(), t.anchor.as_str())).collect();
        assert_eq!(names, [("customers", "t1"), ("items", "t2"), ("orders", "t3")]);
        assert_eq!(c.schemas[1].tables[0].anchor, "t4");
        let cust = &c.schemas[0].tables[0];
        assert_eq!(cust.columns[0].flags, ["pk", "auto"]);
        assert_eq!(cust.indexes[0].columns, "code DESC");
        assert_eq!(
            cust.constraints.iter().map(|k| (k.kind, k.detail.as_str())).collect::<Vec<_>>(),
            [("primary", "id"), ("unique", "code")]
        );
        // 跨 schema 的外键链接到 crm.regions
        assert_eq!(cust.foreign_keys[0].ref_schema, "crm");
        assert_eq!(cust.foreign_keys[0].ref_anchor.as_deref(), Some("t4"));
        // 复合外键
        let orders = &c.schemas[0].tables[2];
        assert_eq!(orders.foreign_keys[0].columns, "cust_id, cust_code");
        assert_eq!(orders.foreign_keys[0].ref_columns, "id, code");
        assert_eq!(orders.columns[4].flags, ["generated"]);
        assert_eq!(orders.columns[4].default.as_deref(), Some("amount * 2"));
        assert_eq!(orders.triggers[0].events, "INSERT, UPDATE");
        // 被引用：订单被明细的逻辑外键引用，客户被订单的外键引用
        assert_eq!(orders.referenced_by.len(), 1);
        assert_eq!(orders.referenced_by[0].kind, "logical_key");
        assert_eq!(orders.referenced_by[0].anchor.as_deref(), Some("t2"));
        assert_eq!(cust.referenced_by[0].table, "orders");
        assert_eq!(c.schemas[1].tables[0].referenced_by[0].schema, "shop");
        // 数据字典没有视图、关系、图
        assert!(c.schemas[0].views.is_empty() && c.relations.is_empty());
        assert_eq!(c.unreadable[0].name, "broken");
        assert_eq!(
            c.stats,
            DocStats { schemas: 2, tables: 4, columns: 13, foreign_keys: 2, logical_keys: 1, views: 0 }
        );
    }

    #[test]
    fn design_relations_views_and_scope() {
        let c = build(opts(DocKind::Design), sample(), vec![]);
        assert_eq!(c.relations.len(), 3);
        assert_eq!(c.relations.iter().filter(|r| r.kind == "logical_key").count(), 1);
        assert_eq!(c.schemas[0].views[0].anchor, "v1");
        assert_eq!(c.schemas[0].views[0].definition.as_deref(), Some("SELECT id FROM orders"));
        assert_eq!(c.stats.views, 1);
        let mut o = opts(DocKind::Design);
        o.view_definitions = false;
        o.tables = Some(vec!["orders".into(), "items".into()]);
        let c = build(o, sample()[..1].to_vec(), vec![]);
        assert_eq!(c.schemas[0].views[0].definition, None);
        let names: Vec<_> = c.schemas[0].tables.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, ["items", "orders"]);
        // 引用文档之外的表时没有锚点
        assert_eq!(c.schemas[0].tables[1].foreign_keys[0].ref_anchor, None);
    }

    #[test]
    fn reads_png_size() {
        let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
        png.extend(640u32.to_be_bytes());
        png.extend(480u32.to_be_bytes());
        assert_eq!(png_size(&png), Some((640, 480)));
        assert_eq!(png_size(b"GIF89a"), None);
    }
}
