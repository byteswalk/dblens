//! 模型目录中的文件（M4a 设计文档第 3 节）：`model.yaml` 与 `tables/<schema>/<表>.yaml`。
//!
//! 写法为 git 而定：字段顺序固定，空列表、没有的值不写，逻辑类型和列序号不写（读取时由原生类型推出、
//! 按先后顺序编号），多行文本用块写法。不认识的字段读进 `extra`，保存时原样写回。

use std::collections::BTreeMap;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use dblens_core::schema::{
    CheckConstraint, Column, ForeignKey, Index, IndexColumn, PrimaryKey, Table, UniqueConstraint,
};
use dblens_core::{ColumnType, Error, Result, msg};
use dblens_dialect::{Compat, Dialect, Variant};
use serde::{Deserialize, Serialize};

/// 当前的文件格式版本。
pub const FORMAT: u32 = 1;

type Extra = BTreeMap<String, serde_json::Value>;

fn is_false(b: &bool) -> bool {
    !*b
}

fn is_true(b: &bool) -> bool {
    *b
}

fn yes() -> bool {
    true
}

/// 模型绑定的数据库类型。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelDatabase {
    pub variant: Variant,
    #[serde(default, skip_serializing_if = "is_native")]
    pub compat: Compat,
    /// 编码后的版本（见 `parse_version`）；不写时按最新版本。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<u32>,
}

fn is_native(c: &Compat) -> bool {
    *c == Compat::Native
}

impl ModelDatabase {
    pub fn dialect(&self) -> Dialect {
        Dialect::new(self.variant).with_version(self.version).with_compat(self.compat)
    }
}

/// 编辑时记下的改名：新名 → 原名。列的改名按（新的）表名分组。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SchemaRenames {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub tables: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub columns: BTreeMap<String, BTreeMap<String, String>>,
}

impl SchemaRenames {
    pub fn is_empty(&self) -> bool {
        self.tables.is_empty() && self.columns.is_empty()
    }
}

/// `model.yaml`。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelInfo {
    pub format: u32,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub database: ModelDatabase,
    #[serde(default)]
    pub schemas: Vec<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub renames: BTreeMap<String, SchemaRenames>,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct ColumnFile {
    name: String,
    #[serde(rename = "type")]
    native: String,
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    nullable: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    default: Option<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    auto_increment: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    generated: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    comment: Option<String>,
    #[serde(flatten)]
    extra: Extra,
}

/// 索引列：升序的只写名字，降序的写成 `{ name, descending: true }`。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
enum IndexColumnFile {
    Name(String),
    Full { name: String, descending: bool },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct IndexFile {
    name: String,
    columns: Vec<IndexColumnFile>,
    #[serde(default, skip_serializing_if = "is_false")]
    unique: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    method: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct KeyFile {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    columns: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct ForeignKeyFile {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    columns: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    ref_schema: Option<String>,
    ref_table: String,
    ref_columns: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    on_delete: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    on_update: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct CheckFile {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    expression: String,
}

/// 表文件。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct TableFile {
    name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    comment: Option<String>,
    columns: Vec<ColumnFile>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    primary_key: Option<KeyFile>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    uniques: Vec<KeyFile>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    foreign_keys: Vec<ForeignKeyFile>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    logical_keys: Vec<LogicalKey>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    checks: Vec<CheckFile>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    indexes: Vec<IndexFile>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    partition: Option<String>,
    #[serde(flatten)]
    extra: Extra,
}

/// 逻辑外键（M4b）：库中没有建的关系，只在模型中记录，不参与比对和同步。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogicalKey {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub columns: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ref_schema: Option<String>,
    pub ref_table: String,
    pub ref_columns: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
}

/// 读出的一张表：结构、逻辑外键、文件中不认识的字段（保存时写回）。
#[derive(Debug, Clone, PartialEq)]
pub struct ModelTable {
    pub table: Table,
    pub logical_keys: Vec<LogicalKey>,
    pub extra: Extra,
    /// 每列不认识的字段，按列名。
    pub column_extra: BTreeMap<String, Extra>,
}

impl ModelTable {
    /// 新表：没有逻辑外键和扩展字段。
    pub fn new(table: Table) -> Self {
        Self { table, logical_keys: vec![], extra: Extra::new(), column_extra: BTreeMap::new() }
    }
}

fn to_file(t: &ModelTable) -> TableFile {
    let tab = &t.table;
    let key =
        |name: &Option<String>, columns: &[String]| KeyFile { name: name.clone(), columns: columns.to_vec() };
    TableFile {
        name: tab.name.clone(),
        comment: tab.comment.clone(),
        columns: tab
            .columns
            .iter()
            .map(|c| ColumnFile {
                name: c.name.clone(),
                native: c.data_type.native.clone(),
                nullable: c.nullable,
                default: c.default.clone(),
                auto_increment: c.auto_increment,
                generated: c.generated.clone(),
                comment: c.comment.clone(),
                extra: t.column_extra.get(&c.name).cloned().unwrap_or_default(),
            })
            .collect(),
        primary_key: tab.primary_key.as_ref().map(|k| key(&k.name, &k.columns)),
        uniques: tab.uniques.iter().map(|u| key(&u.name, &u.columns)).collect(),
        foreign_keys: tab
            .foreign_keys
            .iter()
            .map(|f| ForeignKeyFile {
                name: f.name.clone(),
                columns: f.columns.clone(),
                ref_schema: f.ref_schema.clone(),
                ref_table: f.ref_table.clone(),
                ref_columns: f.ref_columns.clone(),
                on_delete: f.on_delete.clone(),
                on_update: f.on_update.clone(),
            })
            .collect(),
        checks: tab
            .checks
            .iter()
            .map(|c| CheckFile { name: c.name.clone(), expression: c.expression.clone() })
            .collect(),
        indexes: tab
            .indexes
            .iter()
            .map(|x| IndexFile {
                name: x.name.clone(),
                columns: x
                    .columns
                    .iter()
                    .map(|c| {
                        if c.descending {
                            IndexColumnFile::Full { name: c.name.clone(), descending: true }
                        } else {
                            IndexColumnFile::Name(c.name.clone())
                        }
                    })
                    .collect(),
                unique: x.unique,
                method: x.method.clone(),
            })
            .collect(),
        partition: tab.partition.clone(),
        logical_keys: t.logical_keys.clone(),
        extra: t.extra.clone(),
    }
}

fn from_file(f: TableFile, d: &Dialect) -> ModelTable {
    let mut column_extra = BTreeMap::new();
    let columns = f
        .columns
        .into_iter()
        .enumerate()
        .map(|(i, c)| {
            if !c.extra.is_empty() {
                column_extra.insert(c.name.clone(), c.extra);
            }
            Column {
                ordinal: (i + 1) as u32,
                data_type: ColumnType { logical: d.logical_type(&c.native), native: c.native },
                name: c.name,
                nullable: c.nullable,
                default: c.default,
                comment: c.comment,
                auto_increment: c.auto_increment,
                generated: c.generated,
            }
        })
        .collect();
    let table = Table {
        name: f.name,
        comment: f.comment,
        columns,
        primary_key: f.primary_key.map(|k| PrimaryKey { name: k.name, columns: k.columns }),
        uniques: f
            .uniques
            .into_iter()
            .map(|k| UniqueConstraint { name: k.name, columns: k.columns })
            .collect(),
        foreign_keys: f
            .foreign_keys
            .into_iter()
            .map(|k| ForeignKey {
                name: k.name,
                columns: k.columns,
                ref_schema: k.ref_schema,
                ref_table: k.ref_table,
                ref_columns: k.ref_columns,
                on_delete: k.on_delete,
                on_update: k.on_update,
            })
            .collect(),
        checks: f
            .checks
            .into_iter()
            .map(|c| CheckConstraint { name: c.name, expression: c.expression })
            .collect(),
        indexes: f
            .indexes
            .into_iter()
            .map(|x| Index {
                name: x.name,
                columns: x
                    .columns
                    .into_iter()
                    .map(|c| match c {
                        IndexColumnFile::Name(name) => IndexColumn { name, descending: false },
                        IndexColumnFile::Full { name, descending } => IndexColumn { name, descending },
                    })
                    .collect(),
                unique: x.unique,
                method: x.method,
            })
            .collect(),
        triggers: vec![],
        partition: f.partition,
    };
    ModelTable { table, logical_keys: f.logical_keys, extra: f.extra, column_extra }
}

fn yaml_options() -> serde_saphyr::SerializerOptions {
    let mut o = serde_saphyr::SerializerOptions::default();
    o.prefer_block_scalars = true;
    o
}

fn yaml<T: Serialize>(v: &T) -> Result<String> {
    serde_saphyr::to_string_with_options(v, yaml_options())
        .map_err(|e| Error::internal(msg!("model.yaml_write_failed", error = e)))
}

/// 表结构写成 YAML 文本。
pub fn table_yaml(t: &ModelTable) -> Result<String> {
    yaml(&to_file(t))
}

/// 解析表文件；出错时返回带行号的说明。
pub fn parse_table(text: &str, d: &Dialect) -> std::result::Result<ModelTable, String> {
    serde_saphyr::from_str::<TableFile>(text).map(|f| from_file(f, d)).map_err(|e| e.to_string())
}

pub fn info_yaml(info: &ModelInfo) -> Result<String> {
    yaml(info)
}

pub fn parse_info(text: &str) -> std::result::Result<ModelInfo, String> {
    serde_saphyr::from_str::<ModelInfo>(text).map_err(|e| e.to_string())
}

/// 文件内容的指纹（保存前核对文件是否被别人改过）。
pub fn fingerprint(bytes: &[u8]) -> String {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut h);
    format!("{:016x}", h.finish())
}

/// 先写临时文件再改名，写到一半中断也不会留下坏文件。
pub fn write_atomic(path: &Path, text: &str) -> Result<()> {
    let dir = path.parent().ok_or_else(|| Error::invalid(msg!("model.bad_path", path = path.display())))?;
    std::fs::create_dir_all(dir).map_err(|e| io_err(dir, e))?;
    let tmp = dir.join(format!(".{}.tmp", path.file_name().map(|n| n.to_string_lossy()).unwrap_or_default()));
    std::fs::write(&tmp, text).map_err(|e| io_err(&tmp, e))?;
    std::fs::rename(&tmp, path).map_err(|e| io_err(path, e))
}

pub fn io_err(path: &Path, e: std::io::Error) -> Error {
    Error::internal(msg!("model.io_failed", path = path.display(), error = e))
}

const RESERVED: [&str; 22] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8", "COM9",
    "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// 名字转成文件名：文件系统不允许的字符、`%`、结尾的点和空格用 `%XX` 转义；Windows 的保留名在第一个字符转义。
pub fn escape_name(name: &str) -> String {
    let bad =
        |c: char| c.is_control() || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | '%');
    let enc = |c: char| c.to_string().bytes().map(|b| format!("%{b:02X}")).collect::<String>();
    let mut out: String = name.chars().map(|c| if bad(c) { enc(c) } else { c.to_string() }).collect();
    // 结尾的点和空格
    let trimmed = out.trim_end_matches(['.', ' ']).len();
    if trimmed < out.len() {
        let tail: String = out[trimmed..].chars().map(enc).collect();
        out.truncate(trimmed);
        out.push_str(&tail);
    }
    let stem = out.split('.').next().unwrap_or_default().to_ascii_uppercase();
    if RESERVED.contains(&stem.as_str()) || out.is_empty() {
        let first = out.chars().next().map(enc).unwrap_or_default();
        out = first + &out.chars().skip(1).collect::<String>();
    }
    out
}

/// `escape_name` 的逆运算。
pub fn unescape_name(file: &str) -> String {
    let b = file.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && i + 2 < b.len()
            && let Ok(v) = u8::from_str_radix(std::str::from_utf8(&b[i + 1..i + 3]).unwrap_or("-"), 16)
        {
            out.push(v);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// 表文件的路径：`tables/<schema>/<表>.yaml`。
pub fn table_path(root: &Path, schema: &str, table: &str) -> PathBuf {
    root.join("tables").join(escape_name(schema)).join(format!("{}.yaml", escape_name(table)))
}

pub fn info_path(root: &Path) -> PathBuf {
    root.join("model.yaml")
}

#[cfg(test)]
mod tests {
    use dblens_core::LogicalType;

    use super::*;

    fn sample() -> ModelTable {
        let col = |name: &str, native: &str| Column {
            name: name.into(),
            ordinal: 0,
            data_type: ColumnType { native: native.into(), logical: LogicalType::Other },
            nullable: true,
            default: None,
            comment: None,
            auto_increment: false,
            generated: None,
        };
        let mut id = col("id", "bigint");
        id.nullable = false;
        id.auto_increment = true;
        let mut amount = col("amount", "numeric(10,2)");
        amount.default = Some("0".into());
        amount.comment = Some("金额\n（含税）".into());
        let table = Table {
            name: "orders".into(),
            comment: Some("订单".into()),
            columns: vec![id, col("code", "varchar(20)"), amount],
            primary_key: Some(PrimaryKey { name: None, columns: vec!["id".into()] }),
            uniques: vec![UniqueConstraint { name: Some("uk_code".into()), columns: vec!["code".into()] }],
            checks: vec![CheckConstraint {
                name: Some("ck_amount".into()),
                expression: "amount >= 0".into(),
            }],
            foreign_keys: vec![ForeignKey {
                name: Some("fk_cust".into()),
                columns: vec!["code".into()],
                ref_schema: Some("crm".into()),
                ref_table: "customers".into(),
                ref_columns: vec!["code".into()],
                on_delete: Some("CASCADE".into()),
                on_update: None,
            }],
            indexes: vec![Index {
                name: "ix_amount".into(),
                columns: vec![
                    IndexColumn { name: "amount".into(), descending: true },
                    IndexColumn { name: "code".into(), descending: false },
                ],
                unique: false,
                method: None,
            }],
            ..Default::default()
        };
        let mut t = ModelTable::new(table);
        t.logical_keys = vec![LogicalKey {
            name: Some("lk_cust".into()),
            columns: vec!["code".into()],
            ref_schema: None,
            ref_table: "customers".into(),
            ref_columns: vec!["code".into()],
            comment: Some("历史原因没建外键".into()),
        }];
        t
    }

    #[test]
    fn table_yaml_is_stable_and_round_trips() {
        let d = Dialect::new(Variant::Postgres);
        let t = sample();
        let text = table_yaml(&t).unwrap();
        insta::assert_snapshot!("table_yaml", text);
        let back = parse_table(&text, &d).unwrap();
        // 读回时序号按顺序编号、逻辑类型由原生类型推出
        assert_eq!(back.table.columns.iter().map(|c| c.ordinal).collect::<Vec<_>>(), [1, 2, 3]);
        assert_eq!(back.table.columns[0].data_type.logical, LogicalType::Int { bits: 64 });
        let mut expect = t.table.clone();
        for (i, c) in expect.columns.iter_mut().enumerate() {
            c.ordinal = (i + 1) as u32;
            c.data_type.logical = d.logical_type(&c.data_type.native);
        }
        assert_eq!(back.table, expect);
        assert_eq!(table_yaml(&back).unwrap(), text);
    }

    #[test]
    fn unknown_fields_survive_and_errors_have_lines() {
        let d = Dialect::new(Variant::Postgres);
        let text = format!("{}layout:\n  x: 10\n", table_yaml(&sample()).unwrap())
            .replace("  type: bigint\n", "  type: bigint\n  note_from_m4b: keep\n");
        let t = parse_table(&text, &d).unwrap();
        assert_eq!(t.extra["layout"]["x"], 10);
        assert_eq!(t.column_extra["id"]["note_from_m4b"], "keep");
        let again = table_yaml(&t).unwrap();
        assert!(again.contains("layout:") && again.contains("note_from_m4b: keep"), "{again}");
        let err = parse_table("name: x\ncolumns:\n  - name: a\n    type: [oops\n", &d).unwrap_err();
        assert!(err.contains("line 4"), "{err}");
    }

    #[test]
    fn info_yaml_round_trips() {
        let info = ModelInfo {
            format: FORMAT,
            name: "订单系统".into(),
            description: None,
            database: ModelDatabase {
                variant: Variant::Kingbase,
                compat: Compat::Oracle,
                version: Some(120100),
            },
            schemas: vec!["shop".into(), "crm".into()],
            renames: [(
                "shop".into(),
                SchemaRenames {
                    tables: [("orders_new".into(), "orders".into())].into(),
                    columns: [("orders_new".into(), [("amount".into(), "total".into())].into())].into(),
                },
            )]
            .into(),
            extra: Extra::new(),
        };
        let text = info_yaml(&info).unwrap();
        insta::assert_snapshot!("info_yaml", text);
        assert_eq!(parse_info(&text).unwrap(), info);
        let pg = ModelInfo {
            database: ModelDatabase { variant: Variant::Postgres, compat: Compat::Native, version: None },
            renames: BTreeMap::new(),
            ..info
        };
        assert!(!info_yaml(&pg).unwrap().contains("compat"));
    }

    #[test]
    fn file_names_are_escaped() {
        for (name, file) in [
            ("orders", "orders"),
            ("a/b:c", "a%2Fb%3Ac"),
            ("50%", "50%25"),
            ("CON", "%43ON"),
            ("con.x", "%63on.x"),
            ("trail. ", "trail%2E%20"),
            ("数洞 表", "数洞 表"),
        ] {
            assert_eq!(escape_name(name), file, "{name}");
            assert_eq!(unescape_name(file), name, "{file}");
        }
    }
}
