//! ER 图（M4b 设计文档第 3、5 节）：`diagrams/<schema>/<图名>.yaml`。每个 schema 都有默认图 `_all`（全部表），
//! 没有文件时按全部表、自动布局打开；主题图只放选中的表。表改名、删除时各图跟着改。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use dblens_core::schema::Table;
use dblens_core::{Error, Result, msg};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::file::{LogicalKey, escape_name, fingerprint, io_err, unescape_name, write_atomic};
use super::store::{FileError, ModelStore, OpenModel};

/// 默认图的名字（文件 `_all.yaml`）。
pub const DEFAULT_DIAGRAM: &str = "_all";

type Extra = BTreeMap<String, serde_json::Value>;

/// 图中的表：全部（写成 `all`）或列出的表。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiagramTables {
    All,
    List(Vec<String>),
}

impl Serialize for DiagramTables {
    fn serialize<S: Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        match self {
            DiagramTables::All => s.serialize_str("all"),
            DiagramTables::List(l) => l.serialize(s),
        }
    }
}

impl<'de> Deserialize<'de> for DiagramTables {
    fn deserialize<D: Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Raw {
            Word(String),
            List(Vec<String>),
        }
        match Raw::deserialize(d)? {
            Raw::Word(w) if w == "all" => Ok(DiagramTables::All),
            Raw::Word(w) => {
                Err(serde::de::Error::custom(format!("expected `all` or a list of tables, got `{w}`")))
            }
            Raw::List(l) => Ok(DiagramTables::List(l)),
        }
    }
}

impl DiagramTables {
    fn contains(&self, name: &str) -> bool {
        match self {
            DiagramTables::All => true,
            DiagramTables::List(l) => l.iter().any(|t| t == name),
        }
    }
}

/// 表节点的显示方式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagramDisplay {
    #[default]
    Columns,
    Keys,
    Names,
    /// 只画涉及关系的列，其余列合计成一行。
    Relations,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Position {
    pub x: i64,
    pub y: i64,
}

/// 图文件。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Diagram {
    pub name: String,
    pub tables: DiagramTables,
    #[serde(default)]
    pub display: DiagramDisplay,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub positions: BTreeMap<String, Position>,
    #[serde(flatten)]
    pub extra: Extra,
}

impl Diagram {
    fn default_for() -> Self {
        Diagram {
            name: DEFAULT_DIAGRAM.into(),
            tables: DiagramTables::All,
            display: DiagramDisplay::Columns,
            positions: BTreeMap::new(),
            extra: Extra::new(),
        }
    }
}

/// 图列表中的一项。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DiagramSummary {
    pub schema: String,
    pub name: String,
    pub tables: DiagramTables,
}

/// 图中的一张表。文件有错时 `table` 只有名字。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DiagramTable {
    pub table: Table,
    pub logical_keys: Vec<LogicalKey>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<FileError>,
}

/// 图的提示：引用不存在的逻辑外键、图中已不存在的表。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DiagramWarning {
    BrokenLogicalKey { table: String, index: usize, name: Option<String> },
    MissingTable { name: String },
}

/// 读出的图：图（默认图没有文件时为默认值）、表、指纹（没有文件时为 None）、提示。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DiagramView {
    pub diagram: Diagram,
    pub tables: Vec<DiagramTable>,
    pub fingerprint: Option<String>,
    pub warnings: Vec<DiagramWarning>,
}

fn diagram_dir(root: &Path, schema: &str) -> PathBuf {
    root.join("diagrams").join(escape_name(schema))
}

fn diagram_path(root: &Path, schema: &str, name: &str) -> PathBuf {
    diagram_dir(root, schema).join(format!("{}.yaml", escape_name(name)))
}

fn yaml_options() -> serde_saphyr::SerializerOptions {
    let mut o = serde_saphyr::SerializerOptions::default();
    o.prefer_block_scalars = true;
    o
}

pub fn diagram_yaml(d: &Diagram) -> Result<String> {
    serde_saphyr::to_string_with_options(d, yaml_options())
        .map_err(|e| Error::internal(msg!("model.yaml_write_failed", error = e)))
}

pub fn parse_diagram(text: &str) -> std::result::Result<Diagram, String> {
    serde_saphyr::from_str::<Diagram>(text).map_err(|e| e.to_string())
}

/// 读图文件：（图，指纹）；没有文件时为 None。
fn load(root: &Path, schema: &str, name: &str) -> Result<Option<(Diagram, String)>> {
    let path = diagram_path(root, schema, name);
    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(io_err(&path, e)),
    };
    let text = String::from_utf8_lossy(&bytes);
    let d = parse_diagram(&text).map_err(|e| {
        Error::invalid(msg!("model.bad_file", path = format!("diagrams/{schema}/{name}.yaml"), error = e))
    })?;
    Ok(Some((d, fingerprint(&bytes))))
}

fn not_found(schema: &str, name: &str) -> Error {
    Error::not_found(msg!("noun.object", kind = "diagram", name = format!("{schema}.{name}")))
}

/// 一个 schema 的图文件名（不含默认图）。
fn topic_names(root: &Path, schema: &str) -> Result<Vec<String>> {
    let dir = diagram_dir(root, schema);
    let entries = match std::fs::read_dir(&dir) {
        Ok(it) => it,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Err(e) => return Err(io_err(&dir, e)),
    };
    let mut out = Vec::new();
    for e in entries {
        let p = e.map_err(|e| io_err(&dir, e))?.path();
        if p.extension().is_some_and(|x| x == "yaml")
            && let Some(stem) = p.file_stem()
        {
            let name = unescape_name(&stem.to_string_lossy());
            if name != DEFAULT_DIAGRAM {
                out.push(name);
            }
        }
    }
    out.sort();
    Ok(out)
}

/// 表改名（`to` 为 Some）或删除（None）时更新一个 schema 的各图。
pub(super) fn on_table_changed(m: &OpenModel, schema: &str, from: &str, to: Option<&str>) -> Result<()> {
    let mut names = topic_names(&m.root, schema)?;
    names.push(DEFAULT_DIAGRAM.into());
    for name in names {
        let Some((mut d, _)) = load(&m.root, schema, &name)? else { continue };
        let mut changed = false;
        if let DiagramTables::List(l) = &mut d.tables
            && let Some(i) = l.iter().position(|t| t == from)
        {
            match to {
                Some(t) => l[i] = t.to_string(),
                None => {
                    l.remove(i);
                }
            }
            changed = true;
        }
        if let Some(p) = d.positions.remove(from) {
            if let Some(t) = to {
                d.positions.insert(t.to_string(), p);
            }
            changed = true;
        }
        if changed {
            write_atomic(&diagram_path(&m.root, schema, &name), &diagram_yaml(&d)?)?;
        }
    }
    Ok(())
}

fn check_name(name: &str) -> Result<()> {
    if name.trim().is_empty() || name == DEFAULT_DIAGRAM {
        return Err(Error::invalid(msg!("model.diagram_bad_name", name = name)));
    }
    Ok(())
}

impl ModelStore {
    /// 一个 schema 的图：默认图在前（没有文件时也列出），其余按名字。
    pub fn diagrams(&self, id: &str, schema: &str) -> Result<Vec<DiagramSummary>> {
        self.with(id, |m| {
            let mut out = Vec::new();
            let default =
                load(&m.root, schema, DEFAULT_DIAGRAM)?.map_or(DiagramTables::All, |(d, _)| d.tables);
            out.push(DiagramSummary { schema: schema.into(), name: DEFAULT_DIAGRAM.into(), tables: default });
            for name in topic_names(&m.root, schema)? {
                let tables =
                    load(&m.root, schema, &name)?.map_or(DiagramTables::List(vec![]), |(d, _)| d.tables);
                out.push(DiagramSummary { schema: schema.into(), name, tables });
            }
            Ok(out)
        })
    }

    /// 读图：图中的表（文件有错的只有名字）和它们的逻辑外键，以及提示。
    pub fn read_diagram(&self, id: &str, schema: &str, name: &str) -> Result<DiagramView> {
        self.with(id, |m| {
            m.refresh()?;
            let (diagram, fp) = match load(&m.root, schema, name)? {
                Some((d, fp)) => (d, Some(fp)),
                None if name == DEFAULT_DIAGRAM => (Diagram::default_for(), None),
                None => return Err(not_found(schema, name)),
            };
            let entries: Vec<_> = m.entries().into_iter().filter(|e| e.schema == schema).collect();
            let mut tables = Vec::new();
            let mut warnings = Vec::new();
            for e in &entries {
                if !diagram.tables.contains(&e.name) {
                    continue;
                }
                match &e.error {
                    Some(err) => tables.push(DiagramTable {
                        table: Table { name: e.name.clone(), ..Default::default() },
                        logical_keys: vec![],
                        error: Some(err.clone()),
                    }),
                    None => {
                        let (_, mt, _) = m.read(schema, &e.name)?;
                        tables.push(DiagramTable {
                            table: mt.table,
                            logical_keys: mt.logical_keys,
                            error: None,
                        });
                    }
                }
            }
            if let DiagramTables::List(l) = &diagram.tables {
                for t in l.iter().filter(|t| !entries.iter().any(|e| &e.name == *t)) {
                    warnings.push(DiagramWarning::MissingTable { name: t.clone() });
                }
            }
            // 逻辑外键：本表的列、引用的表和列都要在
            for t in &tables {
                for (index, k) in t.logical_keys.iter().enumerate() {
                    let has = |tab: &Table, cols: &[String]| {
                        cols.iter().all(|c| tab.columns.iter().any(|x| &x.name == c))
                    };
                    let ref_schema = k.ref_schema.clone().unwrap_or_else(|| schema.to_string());
                    let target = m.read(&ref_schema, &k.ref_table).ok().map(|(_, mt, _)| mt.table);
                    let ok = has(&t.table, &k.columns) && target.is_some_and(|x| has(&x, &k.ref_columns));
                    if !ok {
                        warnings.push(DiagramWarning::BrokenLogicalKey {
                            table: t.table.name.clone(),
                            index,
                            name: k.name.clone(),
                        });
                    }
                }
            }
            Ok(DiagramView { diagram, tables, fingerprint: fp, warnings })
        })
    }

    /// 保存图（默认图第一次保存时建文件）：文件的指纹与 `expected` 不同时拒绝。返回新的指纹。
    pub fn save_diagram(
        &self,
        id: &str,
        schema: &str,
        name: &str,
        mut diagram: Diagram,
        expected: Option<&str>,
    ) -> Result<String> {
        self.with(id, |m| {
            m.writable()?;
            let current = load(&m.root, schema, name)?;
            if current.is_none() && name != DEFAULT_DIAGRAM {
                return Err(not_found(schema, name));
            }
            if current.as_ref().map(|(_, fp)| fp.as_str()) != expected {
                return Err(Error::invalid(msg!("model.conflict", name = format!("{schema}/{name}"))));
            }
            diagram.name = name.to_string();
            if name == DEFAULT_DIAGRAM {
                diagram.tables = DiagramTables::All;
            }
            // 文件中不认识的字段以文件为准保留
            if let Some((old, _)) = current {
                for (k, v) in old.extra {
                    diagram.extra.entry(k).or_insert(v);
                }
            }
            let text = diagram_yaml(&diagram)?;
            write_atomic(&diagram_path(&m.root, schema, name), &text)?;
            Ok(fingerprint(text.as_bytes()))
        })
    }

    /// 新建主题图。
    pub fn create_diagram(&self, id: &str, schema: &str, name: &str, tables: Vec<String>) -> Result<String> {
        self.with(id, |m| {
            m.writable()?;
            check_name(name)?;
            let path = diagram_path(&m.root, schema, name);
            if path.exists() {
                return Err(Error::invalid(msg!("model.diagram_exists", name = name)));
            }
            let d =
                Diagram { name: name.into(), tables: DiagramTables::List(tables), ..Diagram::default_for() };
            let text = diagram_yaml(&d)?;
            write_atomic(&path, &text)?;
            Ok(fingerprint(text.as_bytes()))
        })
    }

    pub fn rename_diagram(&self, id: &str, schema: &str, from: &str, to: &str) -> Result<()> {
        self.with(id, |m| {
            m.writable()?;
            check_name(from)?;
            check_name(to)?;
            let (mut d, _) = load(&m.root, schema, from)?.ok_or_else(|| not_found(schema, from))?;
            let target = diagram_path(&m.root, schema, to);
            if target.exists() {
                return Err(Error::invalid(msg!("model.diagram_exists", name = to)));
            }
            d.name = to.into();
            write_atomic(&target, &diagram_yaml(&d)?)?;
            let old = diagram_path(&m.root, schema, from);
            std::fs::remove_file(&old).map_err(|e| io_err(&old, e))
        })
    }

    pub fn delete_diagram(&self, id: &str, schema: &str, name: &str) -> Result<()> {
        self.with(id, |m| {
            m.writable()?;
            check_name(name)?;
            let path = diagram_path(&m.root, schema, name);
            if !path.exists() {
                return Err(not_found(schema, name));
            }
            std::fs::remove_file(&path).map_err(|e| io_err(&path, e))
        })
    }
}

#[cfg(test)]
mod tests {
    use dblens_core::schema::{Column, ForeignKey, PrimaryKey};
    use dblens_core::{ColumnType, LogicalType};
    use dblens_dialect::{Compat, Variant};

    use super::super::file::ModelDatabase;
    use super::*;

    fn table(name: &str, cols: &[&str]) -> Table {
        Table {
            name: name.into(),
            columns: cols
                .iter()
                .map(|c| Column {
                    name: (*c).into(),
                    ordinal: 0,
                    data_type: ColumnType { native: "integer".into(), logical: LogicalType::Other },
                    nullable: false,
                    default: None,
                    comment: None,
                    auto_increment: false,
                    generated: None,
                })
                .collect(),
            primary_key: Some(PrimaryKey { name: None, columns: vec![cols[0].into()] }),
            ..Default::default()
        }
    }

    fn sample(root: &Path) -> (ModelStore, String) {
        let store = ModelStore::default();
        let db = ModelDatabase { variant: Variant::Postgres, compat: Compat::Native, version: None };
        let mut orders = table("orders", &["id", "cust_id"]);
        orders.foreign_keys.push(ForeignKey {
            name: Some("fk_cust".into()),
            columns: vec!["cust_id".into()],
            ref_schema: None,
            ref_table: "customers".into(),
            ref_columns: vec!["id".into()],
            on_delete: None,
            on_update: None,
        });
        let tables = vec![table("customers", &["id", "code"]), orders, table("items", &["id"])];
        let id = store.create_from(root, "m", db, &[("public".into(), tables)]).unwrap().id;
        (store, id)
    }

    fn pos(x: i64, y: i64) -> Position {
        Position { x, y }
    }

    #[test]
    fn default_diagram_without_file_then_saved() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("m");
        let (store, id) = sample(&root);
        let list = store.diagrams(&id, "public").unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!((list[0].name.as_str(), &list[0].tables), (DEFAULT_DIAGRAM, &DiagramTables::All));
        let v = store.read_diagram(&id, "public", DEFAULT_DIAGRAM).unwrap();
        assert_eq!(v.fingerprint, None);
        let names: Vec<_> = v.tables.iter().map(|t| t.table.name.as_str()).collect();
        assert_eq!(names, ["customers", "items", "orders"]);
        let mut d = v.diagram;
        d.positions.insert("orders".into(), pos(120, 40));
        d.display = DiagramDisplay::Keys;
        let fp = store.save_diagram(&id, "public", DEFAULT_DIAGRAM, d.clone(), None).unwrap();
        let text = std::fs::read_to_string(root.join("diagrams/public/_all.yaml")).unwrap();
        insta::assert_snapshot!("diagram_yaml", text);
        // 用旧的指纹（没有文件时的 None）再保存被拒绝；文件中手写的字段保留
        assert!(store.save_diagram(&id, "public", DEFAULT_DIAGRAM, d.clone(), None).is_err());
        std::fs::write(root.join("diagrams/public/_all.yaml"), format!("{text}theme: dark\n")).unwrap();
        let v = store.read_diagram(&id, "public", DEFAULT_DIAGRAM).unwrap();
        assert_ne!(v.fingerprint.as_deref(), Some(fp.as_str()));
        store.save_diagram(&id, "public", DEFAULT_DIAGRAM, d, v.fingerprint.as_deref()).unwrap();
        assert!(
            std::fs::read_to_string(root.join("diagrams/public/_all.yaml")).unwrap().contains("theme: dark")
        );
    }

    #[test]
    fn topic_diagrams_follow_table_renames_and_deletes() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("m");
        let (store, id) = sample(&root);
        store.create_diagram(&id, "public", "订单/明细", vec!["orders".into(), "customers".into()]).unwrap();
        assert!(root.join("diagrams/public/订单%2F明细.yaml").exists());
        assert!(store.create_diagram(&id, "public", "订单/明细", vec![]).is_err());
        assert!(store.create_diagram(&id, "public", DEFAULT_DIAGRAM, vec![]).is_err());
        let v = store.read_diagram(&id, "public", "订单/明细").unwrap();
        assert_eq!(v.tables.len(), 2);
        let mut d = v.diagram;
        d.positions.insert("customers".into(), pos(1, 2));
        store.save_diagram(&id, "public", "订单/明细", d, v.fingerprint.as_deref()).unwrap();
        // 表改名、删除
        store.rename_table(&id, "public", "customers", "clients").unwrap();
        let v = store.read_diagram(&id, "public", "订单/明细").unwrap();
        assert_eq!(v.diagram.tables, DiagramTables::List(vec!["orders".into(), "clients".into()]));
        assert_eq!(v.diagram.positions["clients"], pos(1, 2));
        store.delete_table(&id, "public", "orders", None).unwrap();
        let v = store.read_diagram(&id, "public", "订单/明细").unwrap();
        assert_eq!(v.diagram.tables, DiagramTables::List(vec!["clients".into()]));
        // 改名、删除主题图；默认图不能改名、删除
        store.rename_diagram(&id, "public", "订单/明细", "客户").unwrap();
        let names: Vec<_> = store.diagrams(&id, "public").unwrap().into_iter().map(|d| d.name).collect();
        assert_eq!(names, [DEFAULT_DIAGRAM, "客户"]);
        assert!(store.rename_diagram(&id, "public", DEFAULT_DIAGRAM, "x").is_err());
        assert!(store.delete_diagram(&id, "public", DEFAULT_DIAGRAM).is_err());
        store.delete_diagram(&id, "public", "客户").unwrap();
        assert_eq!(store.diagrams(&id, "public").unwrap().len(), 1);
    }

    #[test]
    fn warnings_for_missing_tables_and_broken_logical_keys() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("m");
        let (store, id) = sample(&root);
        let key = LogicalKey {
            name: Some("lk_code".into()),
            columns: vec!["cust_id".into()],
            ref_schema: None,
            ref_table: "customers".into(),
            ref_columns: vec!["code".into()],
            comment: None,
        };
        store.add_logical_key(&id, "public", "orders", key, None).unwrap();
        store.create_diagram(&id, "public", "t", vec!["orders".into(), "gone".into()]).unwrap();
        let v = store.read_diagram(&id, "public", "t").unwrap();
        assert_eq!(v.tables[0].logical_keys.len(), 1);
        assert_eq!(v.warnings, [DiagramWarning::MissingTable { name: "gone".into() }]);
        // 被引用的列删掉后逻辑外键保留，列入提示
        let mut c = store.read_table(&id, "public", "customers").unwrap().table;
        c.columns.pop();
        store.save_table(&id, "public", Some("customers"), c, &BTreeMap::new(), None).unwrap();
        let v = store.read_diagram(&id, "public", DEFAULT_DIAGRAM).unwrap();
        let broken = DiagramWarning::BrokenLogicalKey {
            table: "orders".into(),
            index: 0,
            name: Some("lk_code".into()),
        };
        assert_eq!(v.warnings, [broken]);
        // 文件有错的表只有名字
        std::fs::write(root.join("tables/public/bad.yaml"), "name: [").unwrap();
        let v = store.read_diagram(&id, "public", DEFAULT_DIAGRAM).unwrap();
        let bad = v.tables.iter().find(|t| t.table.name == "bad").unwrap();
        assert!(bad.error.is_some() && bad.table.columns.is_empty());
    }
}
