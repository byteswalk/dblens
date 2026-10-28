//! 打开的模型（M4a 设计文档 4.1 节）：编号 → 目录、模型信息；表文件按需读取，按修改时间和长度判断是否重读。
//! 只做文件读写，不连库。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;

use dblens_core::schema::Table;
use dblens_core::{Error, Result, msg};
use dblens_dialect::Dialect;
use serde::Serialize;

use super::diagram;
use super::file::{
    FORMAT, LogicalKey, ModelDatabase, ModelInfo, ModelTable, SchemaRenames, fingerprint, info_path,
    info_yaml, io_err, parse_info, parse_table, table_path, table_yaml, unescape_name, write_atomic,
};

/// 打开的模型的概要。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ModelSummary {
    pub id: String,
    pub path: String,
    pub info: ModelInfo,
    /// 文件格式比程序认识的新：只读。
    pub read_only: bool,
}

/// 表文件解析失败的原因。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FileError {
    /// 相对模型目录的路径。
    pub path: String,
    /// 带行号、列号的说明。
    pub message: String,
}

/// 表列表中的一项。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TableEntry {
    pub schema: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<FileError>,
}

/// 读出的表和文件指纹（保存时核对）。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ModelTableView {
    pub table: Table,
    pub fingerprint: String,
}

struct Cached {
    modified: SystemTime,
    len: u64,
    fingerprint: String,
    parsed: std::result::Result<ModelTable, String>,
}

pub(super) struct OpenModel {
    pub(super) root: PathBuf,
    info: ModelInfo,
    read_only: bool,
    files: BTreeMap<PathBuf, Cached>,
}

/// 一个 schema 中读出的表：（表，文件指纹）；有错的表单独列出。
#[derive(Debug, Clone, Default)]
pub struct SchemaTables {
    pub tables: Vec<(Table, String)>,
    pub errors: Vec<FileError>,
}

/// 写回模型的一项操作（比对后同步到模型，见 6.2 节）。
#[derive(Debug, Clone, PartialEq)]
pub enum ModelWrite {
    /// 写入一张表；`path_of` 是原来的表名（新建时为 None）。改名时旧文件删除。
    Put {
        table: Box<Table>,
        path_of: Option<String>,
    },
    Delete {
        name: String,
    },
}

#[derive(Default)]
pub struct ModelStore {
    models: Mutex<BTreeMap<String, OpenModel>>,
}

fn rel(root: &Path, p: &Path) -> String {
    p.strip_prefix(root).unwrap_or(p).to_string_lossy().replace('\\', "/")
}

fn not_open(id: &str) -> Error {
    Error::not_found(msg!("noun.model", id = id))
}

fn table_missing(schema: &str, name: &str) -> Error {
    Error::not_found(msg!("noun.object", kind = "table", name = format!("{schema}.{name}")))
}

/// 目录必须不存在或为空。
fn check_empty(dir: &Path) -> Result<()> {
    match std::fs::read_dir(dir) {
        Ok(mut it) => match it.next() {
            Some(_) => Err(Error::invalid(msg!("model.dir_not_empty", path = dir.display()))),
            None => Ok(()),
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(io_err(dir, e)),
    }
}

fn load_info(root: &Path) -> Result<ModelInfo> {
    let path = info_path(root);
    let text = std::fs::read_to_string(&path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            Error::invalid(msg!("model.not_a_model", path = root.display()))
        } else {
            io_err(&path, e)
        }
    })?;
    parse_info(&text).map_err(|e| Error::invalid(msg!("model.bad_file", path = "model.yaml", error = e)))
}

/// 改名记录（新名 → 原名）中把 `from` 改成 `to`：`from` 本身是上次改名的结果时沿用最初的原名，改回原名时删去。
fn record_rename(map: &mut BTreeMap<String, String>, from: &str, to: &str) {
    let original = map.remove(from).unwrap_or_else(|| from.to_string());
    if original != to {
        map.insert(to.to_string(), original);
    }
}

impl OpenModel {
    fn dialect(&self) -> Dialect {
        self.info.database.dialect()
    }

    pub(super) fn writable(&self) -> Result<()> {
        if self.read_only {
            return Err(Error::invalid(msg!("model.read_only", format = self.info.format)));
        }
        Ok(())
    }

    /// 扫描 `tables/` 下的文件，修改时间或长度变了的重读，删掉的文件移出缓存。
    pub(super) fn refresh(&mut self) -> Result<()> {
        let d = self.dialect();
        let dir = self.root.join("tables");
        let mut seen = Vec::new();
        let schemas = match std::fs::read_dir(&dir) {
            Ok(it) => it.collect::<std::io::Result<Vec<_>>>().map_err(|e| io_err(&dir, e))?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => vec![],
            Err(e) => return Err(io_err(&dir, e)),
        };
        for s in schemas.into_iter().filter(|s| s.path().is_dir()) {
            let sdir = s.path();
            for f in std::fs::read_dir(&sdir).map_err(|e| io_err(&sdir, e))? {
                let path = f.map_err(|e| io_err(&sdir, e))?.path();
                if path.extension().is_none_or(|x| x != "yaml") {
                    continue;
                }
                let meta = std::fs::metadata(&path).map_err(|e| io_err(&path, e))?;
                let modified = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
                seen.push(path.clone());
                if self.files.get(&path).is_some_and(|c| c.modified == modified && c.len == meta.len()) {
                    continue;
                }
                let bytes = std::fs::read(&path).map_err(|e| io_err(&path, e))?;
                let parsed = std::str::from_utf8(&bytes)
                    .map_err(|e| e.to_string())
                    .and_then(|text| parse_table(text, &d));
                let cached = Cached { modified, len: meta.len(), fingerprint: fingerprint(&bytes), parsed };
                self.files.insert(path, cached);
            }
        }
        self.files.retain(|p, _| seen.contains(p));
        Ok(())
    }

    fn schema_of(&self, path: &Path) -> String {
        let dir = path.parent().and_then(|p| p.file_name()).map(|n| n.to_string_lossy()).unwrap_or_default();
        unescape_name(&dir)
    }

    /// 表所在的文件：表名以文件内容为准，解析失败的按文件名。
    fn find(&self, schema: &str, name: &str) -> Option<&Path> {
        self.files
            .iter()
            .find(|(p, c)| {
                self.schema_of(p) == schema
                    && match &c.parsed {
                        Ok(t) => t.table.name == name,
                        Err(_) => p.file_stem().is_some_and(|s| unescape_name(&s.to_string_lossy()) == name),
                    }
            })
            .map(|(p, _)| p.as_path())
    }

    pub(super) fn entries(&self) -> Vec<TableEntry> {
        let mut out: Vec<TableEntry> = self
            .files
            .iter()
            .map(|(p, c)| {
                let schema = self.schema_of(p);
                match &c.parsed {
                    Ok(t) => TableEntry {
                        schema,
                        name: t.table.name.clone(),
                        comment: t.table.comment.clone(),
                        error: None,
                    },
                    Err(e) => TableEntry {
                        schema,
                        name: p.file_stem().map(|s| unescape_name(&s.to_string_lossy())).unwrap_or_default(),
                        comment: None,
                        error: Some(FileError { path: rel(&self.root, p), message: e.clone() }),
                    },
                }
            })
            .collect();
        out.sort_by(|a, b| (&a.schema, &a.name).cmp(&(&b.schema, &b.name)));
        out
    }

    fn cached(&self, schema: &str, name: &str) -> Result<(&Path, &Cached)> {
        let path = self.find(schema, name).ok_or_else(|| table_missing(schema, name))?;
        Ok((path, &self.files[path]))
    }

    pub(super) fn read(&self, schema: &str, name: &str) -> Result<(PathBuf, ModelTable, String)> {
        let (path, c) = self.cached(schema, name)?;
        match &c.parsed {
            Ok(t) => Ok((path.to_path_buf(), t.clone(), c.fingerprint.clone())),
            Err(e) => Err(Error::invalid(msg!("model.bad_file", path = rel(&self.root, path), error = e))),
        }
    }

    fn check_fingerprint(&self, schema: &str, name: &str, expected: Option<&str>) -> Result<()> {
        if let Some(fp) = expected
            && self.cached(schema, name)?.1.fingerprint != fp
        {
            return Err(Error::invalid(msg!("model.conflict", name = format!("{schema}.{name}"))));
        }
        Ok(())
    }

    fn write_info(&mut self) -> Result<()> {
        write_atomic(&info_path(&self.root), &info_yaml(&self.info)?)
    }

    /// 写一张表的文件（保留文件中不认识的字段），返回新的指纹。
    fn write_table(&mut self, schema: &str, path: &Path, t: &ModelTable) -> Result<String> {
        let text = table_yaml(t)?;
        write_atomic(path, &text)?;
        if !self.info.schemas.iter().any(|s| s == schema) {
            self.info.schemas.push(schema.to_string());
            self.write_info()?;
        }
        Ok(fingerprint(text.as_bytes()))
    }

    /// 其他表中引用本 schema 的外键和逻辑外键交给 `fix` 修改（参数为引用的表名、列，返回是否改了），
    /// 改过的表写回文件。
    fn update_refs(
        &mut self,
        schema: &str,
        skip: Option<&Path>,
        mut fix: impl FnMut(&mut String, &mut Vec<String>) -> bool,
    ) -> Result<()> {
        let mut changed = Vec::new();
        for (p, c) in &self.files {
            if Some(p.as_path()) == skip {
                continue;
            }
            let Ok(t) = &c.parsed else { continue };
            let own = self.schema_of(p);
            let mut t = t.clone();
            let mut hit = false;
            for f in &mut t.table.foreign_keys {
                let ref_schema = f.ref_schema.clone().unwrap_or_else(|| own.clone());
                if ref_schema == schema && fix(&mut f.ref_table, &mut f.ref_columns) {
                    hit = true;
                }
            }
            for k in &mut t.logical_keys {
                let ref_schema = k.ref_schema.clone().unwrap_or_else(|| own.clone());
                if ref_schema == schema && fix(&mut k.ref_table, &mut k.ref_columns) {
                    hit = true;
                }
            }
            if hit {
                changed.push((own, p.clone(), t));
            }
        }
        for (own, p, t) in changed {
            self.write_table(&own, &p, &t)?;
        }
        Ok(())
    }

    fn renames(&mut self, schema: &str) -> &mut SchemaRenames {
        self.info.renames.entry(schema.to_string()).or_default()
    }

    fn prune_renames(&mut self) {
        self.info.renames.retain(|_, r| {
            r.columns.retain(|_, c| !c.is_empty());
            !r.is_empty()
        });
    }
}

impl ModelStore {
    pub(super) fn with<T>(&self, id: &str, f: impl FnOnce(&mut OpenModel) -> Result<T>) -> Result<T> {
        let mut models = self.models.lock().unwrap_or_else(|e| e.into_inner());
        let m = models.get_mut(id).ok_or_else(|| not_open(id))?;
        f(m)
    }

    fn summary(id: &str, m: &OpenModel) -> ModelSummary {
        ModelSummary {
            id: id.to_string(),
            path: m.root.to_string_lossy().into_owned(),
            info: m.info.clone(),
            read_only: m.read_only,
        }
    }

    fn insert(&self, root: PathBuf, info: ModelInfo) -> Result<ModelSummary> {
        let mut models = self.models.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((id, m)) = models.iter().find(|(_, m)| m.root == root) {
            return Ok(Self::summary(id, m));
        }
        let read_only = info.format > FORMAT;
        let mut m = OpenModel { root, info, read_only, files: BTreeMap::new() };
        m.refresh()?;
        let id = uuid::Uuid::new_v4().to_string();
        let s = Self::summary(&id, &m);
        models.insert(id, m);
        Ok(s)
    }

    /// 打开模型目录；已经打开的返回原来的编号。
    pub fn open(&self, path: &Path) -> Result<ModelSummary> {
        let root = std::path::absolute(path).map_err(|e| io_err(path, e))?;
        let info = load_info(&root)?;
        self.insert(root, info)
    }

    /// 新建空模型：目录必须不存在或为空。
    pub fn create(
        &self,
        path: &Path,
        name: &str,
        database: ModelDatabase,
        schemas: Vec<String>,
    ) -> Result<ModelSummary> {
        let root = std::path::absolute(path).map_err(|e| io_err(path, e))?;
        check_empty(&root)?;
        let info = ModelInfo {
            format: FORMAT,
            name: name.to_string(),
            description: None,
            database,
            schemas,
            renames: BTreeMap::new(),
            extra: BTreeMap::new(),
        };
        write_atomic(&info_path(&root), &info_yaml(&info)?)?;
        self.insert(root, info)
    }

    /// 反向生成：写入新目录，表文件写完后打开。
    pub fn create_from(
        &self,
        path: &Path,
        name: &str,
        database: ModelDatabase,
        tables: &[(String, Vec<Table>)],
    ) -> Result<ModelSummary> {
        let root = std::path::absolute(path).map_err(|e| io_err(path, e))?;
        check_empty(&root)?;
        for (schema, list) in tables {
            for t in list {
                let mt = ModelTable::new(t.clone());
                write_atomic(&table_path(&root, schema, &t.name), &table_yaml(&mt)?)?;
            }
        }
        let info = ModelInfo {
            format: FORMAT,
            name: name.to_string(),
            description: None,
            database,
            schemas: tables.iter().map(|(s, _)| s.clone()).collect(),
            renames: BTreeMap::new(),
            extra: BTreeMap::new(),
        };
        write_atomic(&info_path(&root), &info_yaml(&info)?)?;
        self.insert(root, info)
    }

    pub fn close(&self, id: &str) -> Result<()> {
        let mut models = self.models.lock().unwrap_or_else(|e| e.into_inner());
        models.remove(id).map(|_| ()).ok_or_else(|| not_open(id))
    }

    pub fn list(&self) -> Vec<ModelSummary> {
        let models = self.models.lock().unwrap_or_else(|e| e.into_inner());
        models.iter().map(|(id, m)| Self::summary(id, m)).collect()
    }

    /// 重读 `model.yaml` 和表文件。
    pub fn summary_of(&self, id: &str) -> Result<ModelSummary> {
        self.with(id, |m| {
            m.info = load_info(&m.root)?;
            m.read_only = m.info.format > FORMAT;
            m.refresh()?;
            Ok(Self::summary(id, m))
        })
    }

    pub fn dialect(&self, id: &str) -> Result<Dialect> {
        self.with(id, |m| Ok(m.dialect()))
    }

    /// 改名称、说明、schema 列表。
    pub fn set_info(
        &self,
        id: &str,
        name: &str,
        description: Option<String>,
        schemas: Vec<String>,
    ) -> Result<ModelSummary> {
        self.with(id, |m| {
            m.writable()?;
            m.info = load_info(&m.root)?;
            m.info.name = name.to_string();
            m.info.description = description;
            m.info.schemas = schemas;
            m.write_info()?;
            Ok(Self::summary(id, m))
        })
    }

    pub fn tables(&self, id: &str) -> Result<Vec<TableEntry>> {
        self.with(id, |m| {
            m.refresh()?;
            Ok(m.entries())
        })
    }

    pub fn read_table(&self, id: &str, schema: &str, name: &str) -> Result<ModelTableView> {
        self.with(id, |m| {
            m.refresh()?;
            let (_, t, fingerprint) = m.read(schema, name)?;
            Ok(ModelTableView { table: t.table, fingerprint })
        })
    }

    /// 一个 schema 的全部表（比对用）；文件有错的单独列出。
    pub fn schema_tables(&self, id: &str, schema: &str) -> Result<SchemaTables> {
        self.with(id, |m| {
            m.refresh()?;
            let mut out = SchemaTables::default();
            for (p, c) in &m.files {
                if m.schema_of(p) != schema {
                    continue;
                }
                match &c.parsed {
                    Ok(t) => out.tables.push((t.table.clone(), c.fingerprint.clone())),
                    Err(e) => out.errors.push(FileError { path: rel(&m.root, p), message: e.clone() }),
                }
            }
            out.tables.sort_by(|a, b| a.0.name.cmp(&b.0.name));
            Ok(out)
        })
    }

    /// 改名记录。
    pub fn renames(&self, id: &str, schema: &str) -> Result<SchemaRenames> {
        self.with(id, |m| Ok(m.info.renames.get(schema).cloned().unwrap_or_default()))
    }

    /// 保存设计器中的表。`name` 为原表名（新建时 None）；`column_renames`：新列名 → 原列名；
    /// `expected`：打开时的指纹，文件已变则拒绝。表名不能在这里改（用 `rename_table`）。返回新的指纹。
    pub fn save_table(
        &self,
        id: &str,
        schema: &str,
        name: Option<&str>,
        table: Table,
        column_renames: &BTreeMap<String, String>,
        expected: Option<&str>,
    ) -> Result<String> {
        self.with(id, |m| {
            m.writable()?;
            m.refresh()?;
            m.info = load_info(&m.root)?;
            let (path, mut mt) = match name {
                Some(n) => {
                    m.check_fingerprint(schema, n, expected)?;
                    let (path, mt, _) = m.read(schema, n)?;
                    (path, mt)
                }
                None => {
                    if m.find(schema, &table.name).is_some() {
                        return Err(Error::invalid(msg!(
                            "model.table_exists",
                            name = format!("{schema}.{}", table.name)
                        )));
                    }
                    let mt = ModelTable::new(table.clone());
                    (table_path(&m.root, schema, &table.name), mt)
                }
            };
            let tname = name.map(str::to_string).unwrap_or_else(|| table.name.clone());
            let table = Table { name: tname.clone(), ..table };
            // 列改名：其他表的外键跟着改，记入改名记录；列的扩展字段跟着新名
            for (to, from) in column_renames {
                if let Some(x) = mt.column_extra.remove(from) {
                    mt.column_extra.insert(to.clone(), x);
                }
                let (tn, from_c, to_c) = (tname.clone(), from.clone(), to.clone());
                m.update_refs(schema, Some(&path), |ref_table, ref_columns| {
                    let hit = *ref_table == tn && ref_columns.contains(&from_c);
                    if hit {
                        ref_columns.iter_mut().filter(|c| **c == from_c).for_each(|c| *c = to_c.clone());
                    }
                    hit
                })?;
                // 本表的逻辑外键：本表的列、自引用的列
                for k in &mut mt.logical_keys {
                    k.columns.iter_mut().filter(|c| **c == from_c).for_each(|c| *c = to_c.clone());
                    if k.ref_table == tn && k.ref_schema.as_deref().is_none_or(|s| s == schema) {
                        k.ref_columns.iter_mut().filter(|c| **c == from_c).for_each(|c| *c = to_c.clone());
                    }
                }
                let cols = m.renames(schema).columns.entry(tname.clone()).or_default();
                record_rename(cols, from, to);
            }
            // 删掉的列不再保留改名记录和扩展字段
            let names: Vec<&str> = table.columns.iter().map(|c| c.name.as_str()).collect();
            if let Some(cols) = m.renames(schema).columns.get_mut(&tname) {
                cols.retain(|k, _| names.contains(&k.as_str()));
            }
            mt.column_extra.retain(|k, _| names.contains(&k.as_str()));
            m.prune_renames();
            mt.table = table;
            let fp = m.write_table(schema, &path, &mt)?;
            m.write_info()?;
            m.refresh()?;
            Ok(fp)
        })
    }

    /// 表改名：改文件名、更新其他表外键的引用、记入改名记录。
    pub fn rename_table(&self, id: &str, schema: &str, from: &str, to: &str) -> Result<()> {
        self.with(id, |m| {
            m.writable()?;
            m.refresh()?;
            m.info = load_info(&m.root)?;
            if m.find(schema, to).is_some() {
                return Err(Error::invalid(msg!("model.table_exists", name = format!("{schema}.{to}"))));
            }
            let (old, mut mt, _) = m.read(schema, from)?;
            mt.table.name = to.to_string();
            // 自引用的外键和逻辑外键
            for f in &mut mt.table.foreign_keys {
                if f.ref_table == from && f.ref_schema.as_deref().is_none_or(|s| s == schema) {
                    f.ref_table = to.to_string();
                }
            }
            for k in &mut mt.logical_keys {
                if k.ref_table == from && k.ref_schema.as_deref().is_none_or(|s| s == schema) {
                    k.ref_table = to.to_string();
                }
            }
            let new = table_path(&m.root, schema, to);
            m.write_table(schema, &new, &mt)?;
            if new != old {
                std::fs::remove_file(&old).map_err(|e| io_err(&old, e))?;
            }
            m.update_refs(schema, Some(&new), |ref_table, _| {
                let hit = ref_table == from;
                if hit {
                    *ref_table = to.to_string();
                }
                hit
            })?;
            diagram::on_table_changed(m, schema, from, Some(to))?;
            let r = m.renames(schema);
            record_rename(&mut r.tables, from, to);
            if let Some(cols) = r.columns.remove(from) {
                r.columns.insert(to.to_string(), cols);
            }
            m.prune_renames();
            m.write_info()?;
            m.refresh()
        })
    }

    /// 加一个逻辑外键：两边的列存在、列数一致、名字在本表中不重复。返回表文件的新指纹。
    pub fn add_logical_key(
        &self,
        id: &str,
        schema: &str,
        table: &str,
        key: LogicalKey,
        expected: Option<&str>,
    ) -> Result<String> {
        self.with(id, |m| {
            m.writable()?;
            m.refresh()?;
            m.check_fingerprint(schema, table, expected)?;
            let (path, mut mt, _) = m.read(schema, table)?;
            let missing = |name: &str| Error::invalid(msg!("model.lk_column_missing", name = name));
            if key.columns.is_empty() || key.columns.len() != key.ref_columns.len() {
                return Err(Error::invalid(msg!("model.lk_count_mismatch")));
            }
            for c in &key.columns {
                if !mt.table.columns.iter().any(|x| &x.name == c) {
                    return Err(missing(&format!("{table}.{c}")));
                }
            }
            let ref_schema = key.ref_schema.clone().unwrap_or_else(|| schema.to_string());
            let target = if ref_schema == schema && key.ref_table == table {
                mt.table.clone()
            } else {
                m.read(&ref_schema, &key.ref_table)?.1.table
            };
            for c in &key.ref_columns {
                if !target.columns.iter().any(|x| &x.name == c) {
                    return Err(missing(&format!("{}.{c}", key.ref_table)));
                }
            }
            if let Some(n) = &key.name {
                let taken = mt.logical_keys.iter().any(|k| k.name.as_ref() == Some(n))
                    || mt.table.foreign_keys.iter().any(|k| k.name.as_ref() == Some(n));
                if taken {
                    return Err(Error::invalid(msg!("model.lk_name_exists", name = n)));
                }
            }
            mt.logical_keys.push(key);
            let fp = m.write_table(schema, &path, &mt)?;
            m.refresh()?;
            Ok(fp)
        })
    }

    /// 删除第 `index` 个逻辑外键（从 0 开始）。返回表文件的新指纹。
    pub fn delete_logical_key(
        &self,
        id: &str,
        schema: &str,
        table: &str,
        index: usize,
        expected: Option<&str>,
    ) -> Result<String> {
        self.with(id, |m| {
            m.writable()?;
            m.refresh()?;
            m.check_fingerprint(schema, table, expected)?;
            let (path, mut mt, _) = m.read(schema, table)?;
            if index >= mt.logical_keys.len() {
                return Err(Error::not_found(msg!("model.lk_not_found", index = index)));
            }
            mt.logical_keys.remove(index);
            let fp = m.write_table(schema, &path, &mt)?;
            m.refresh()?;
            Ok(fp)
        })
    }

    /// 删除表文件，去掉它的改名记录。
    pub fn delete_table(&self, id: &str, schema: &str, name: &str, expected: Option<&str>) -> Result<()> {
        self.with(id, |m| {
            m.writable()?;
            m.refresh()?;
            m.info = load_info(&m.root)?;
            m.check_fingerprint(schema, name, expected)?;
            let path = m.find(schema, name).ok_or_else(|| table_missing(schema, name))?.to_path_buf();
            std::fs::remove_file(&path).map_err(|e| io_err(&path, e))?;
            diagram::on_table_changed(m, schema, name, None)?;
            let r = m.renames(schema);
            r.tables.remove(name);
            r.columns.remove(name);
            m.prune_renames();
            m.write_info()?;
            m.refresh()
        })
    }

    /// 比对后写回：先核对涉及的表的指纹（比对时记下的），再依次写入、删除。
    /// `cleared`：已应用的改名（表名，及（表名，列名）），从改名记录中删去。
    pub fn write_back(
        &self,
        id: &str,
        schema: &str,
        fingerprints: &BTreeMap<String, String>,
        writes: Vec<ModelWrite>,
        cleared: &[(String, Option<String>)],
    ) -> Result<()> {
        self.with(id, |m| {
            m.writable()?;
            m.refresh()?;
            m.info = load_info(&m.root)?;
            for w in &writes {
                let name = match w {
                    ModelWrite::Put { path_of: Some(n), .. } | ModelWrite::Delete { name: n } => n,
                    ModelWrite::Put { path_of: None, table } => {
                        if m.find(schema, &table.name).is_some() {
                            return Err(Error::invalid(msg!(
                                "model.conflict",
                                name = format!("{schema}.{}", table.name)
                            )));
                        }
                        continue;
                    }
                };
                m.check_fingerprint(schema, name, fingerprints.get(name).map(String::as_str))?;
            }
            for w in writes {
                match w {
                    ModelWrite::Put { table, path_of } => {
                        let (old, mut mt) = match &path_of {
                            Some(n) => {
                                let (p, mt, _) = m.read(schema, n)?;
                                (Some(p), mt)
                            }
                            None => (None, ModelTable::new(Table::default())),
                        };
                        let new = match &old {
                            Some(p) if path_of.as_deref() == Some(table.name.as_str()) => p.clone(),
                            _ => table_path(&m.root, schema, &table.name),
                        };
                        mt.table = *table;
                        m.write_table(schema, &new, &mt)?;
                        if let Some(p) = old.filter(|p| *p != new) {
                            std::fs::remove_file(&p).map_err(|e| io_err(&p, e))?;
                        }
                    }
                    ModelWrite::Delete { name } => {
                        let path =
                            m.find(schema, &name).ok_or_else(|| table_missing(schema, &name))?.to_path_buf();
                        std::fs::remove_file(&path).map_err(|e| io_err(&path, e))?;
                    }
                }
                m.refresh()?;
            }
            clear_renames(m, schema, cleared);
            m.write_info()?;
            m.refresh()
        })
    }

    /// 同步到库后，删去已应用的改名记录。
    pub fn clear_renames(&self, id: &str, schema: &str, cleared: &[(String, Option<String>)]) -> Result<()> {
        if cleared.is_empty() {
            return Ok(());
        }
        self.with(id, |m| {
            m.writable()?;
            m.info = load_info(&m.root)?;
            clear_renames(m, schema, cleared);
            m.write_info()
        })
    }

    /// 模型目录（"在文件管理器中显示"）。
    pub fn root(&self, id: &str) -> Result<PathBuf> {
        self.with(id, |m| Ok(m.root.clone()))
    }
}

fn clear_renames(m: &mut OpenModel, schema: &str, cleared: &[(String, Option<String>)]) {
    let r = m.renames(schema);
    for (table, column) in cleared {
        match column {
            None => {
                r.tables.remove(table);
            }
            Some(c) => {
                if let Some(cols) = r.columns.get_mut(table) {
                    cols.remove(c);
                }
            }
        }
    }
    m.prune_renames();
}

#[cfg(test)]
mod tests {
    use dblens_core::schema::{Column, ForeignKey, PrimaryKey};
    use dblens_core::{ColumnType, LogicalType};
    use dblens_dialect::{Compat, Variant};

    use super::*;

    fn col(name: &str) -> Column {
        Column {
            name: name.into(),
            ordinal: 0,
            data_type: ColumnType { native: "integer".into(), logical: LogicalType::Other },
            nullable: false,
            default: None,
            comment: None,
            auto_increment: false,
            generated: None,
        }
    }

    fn table(name: &str, cols: &[&str]) -> Table {
        Table {
            name: name.into(),
            columns: cols.iter().map(|c| col(c)).collect(),
            primary_key: Some(PrimaryKey { name: None, columns: vec![cols[0].into()] }),
            ..Default::default()
        }
    }

    fn fk(cols: &str, ref_table: &str, ref_cols: &str) -> ForeignKey {
        ForeignKey {
            name: Some(format!("fk_{cols}")),
            columns: vec![cols.into()],
            ref_schema: None,
            ref_table: ref_table.into(),
            ref_columns: vec![ref_cols.into()],
            on_delete: None,
            on_update: None,
        }
    }

    fn pg() -> ModelDatabase {
        ModelDatabase { variant: Variant::Postgres, compat: Compat::Native, version: None }
    }

    fn fingerprints(store: &ModelStore, id: &str) -> BTreeMap<String, String> {
        let s = store.schema_tables(id, "public").unwrap();
        s.tables.iter().map(|(t, fp)| (t.name.clone(), fp.clone())).collect()
    }

    /// 两张表：orders 的 cust_id 引用 customers.id。
    fn sample(dir: &Path) -> (ModelStore, String) {
        let store = ModelStore::default();
        let mut orders = table("orders", &["id", "cust_id"]);
        orders.foreign_keys.push(fk("cust_id", "customers", "id"));
        let m = store
            .create_from(dir, "shop", pg(), &[("public".into(), vec![table("customers", &["id"]), orders])])
            .unwrap();
        (store, m.id)
    }

    #[test]
    fn create_open_and_list() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("m");
        let (store, id) = sample(&root);
        assert!(root.join("tables/public/orders.yaml").exists());
        let names: Vec<_> = store.tables(&id).unwrap().into_iter().map(|e| e.name).collect();
        assert_eq!(names, ["customers", "orders"]);
        // 再次打开同一目录得到同一个编号；另一个 store 也能读
        assert_eq!(store.open(&root).unwrap().id, id);
        let s = ModelStore::default().open(&root).unwrap();
        assert_eq!(s.info.schemas, ["public"]);
        assert!(!s.read_only);
        // 目录不为空时不能新建
        let err = store.create(&root, "x", pg(), vec![]).unwrap_err();
        assert!(err.to_string().contains("not empty"), "{err}");
        store.close(&id).unwrap();
        assert!(store.list().is_empty());
    }

    #[test]
    fn bad_files_and_newer_format() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("m");
        let (store, id) = sample(&root);
        std::fs::write(root.join("tables/public/broken.yaml"), "name: broken\ncolumns: [\n").unwrap();
        let entries = store.tables(&id).unwrap();
        let bad = entries.iter().find(|e| e.name == "broken").unwrap();
        let err = bad.error.as_ref().unwrap();
        assert_eq!(err.path, "tables/public/broken.yaml");
        assert!(err.message.contains("line"), "{}", err.message);
        let s = store.schema_tables(&id, "public").unwrap();
        assert_eq!((s.tables.len(), s.errors.len()), (2, 1));
        assert!(store.read_table(&id, "public", "broken").is_err());
        // 格式比程序新：只读
        let text =
            std::fs::read_to_string(root.join("model.yaml")).unwrap().replace("format: 1", "format: 9");
        std::fs::write(root.join("model.yaml"), text).unwrap();
        assert!(store.summary_of(&id).unwrap().read_only);
        let err = store.delete_table(&id, "public", "orders", None).unwrap_err();
        assert!(err.to_string().contains("read-only"), "{err}");
    }

    #[test]
    fn fingerprint_conflict() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("m");
        let (store, id) = sample(&root);
        let v = store.read_table(&id, "public", "customers").unwrap();
        // 文件在外面被改了（如在 git 中切换了分支）
        let path = root.join("tables/public/customers.yaml");
        std::fs::write(&path, std::fs::read_to_string(&path).unwrap() + "comment: changed\n").unwrap();
        let t = table("customers", &["id", "name"]);
        let none = BTreeMap::new();
        let err = store
            .save_table(&id, "public", Some("customers"), t.clone(), &none, Some(&v.fingerprint))
            .unwrap_err();
        assert!(err.to_string().contains("changed"), "{err}");
        let v = store.read_table(&id, "public", "customers").unwrap();
        let fp = store.save_table(&id, "public", Some("customers"), t, &none, Some(&v.fingerprint)).unwrap();
        assert_eq!(store.read_table(&id, "public", "customers").unwrap().fingerprint, fp);
    }

    #[test]
    fn renames_update_references_and_are_recorded() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("m");
        let (store, id) = sample(&root);
        // 表改名：文件改名，orders 的外键跟着改
        store.rename_table(&id, "public", "customers", "clients").unwrap();
        assert!(!root.join("tables/public/customers.yaml").exists());
        let orders = store.read_table(&id, "public", "orders").unwrap().table;
        assert_eq!(orders.foreign_keys[0].ref_table, "clients");
        // 列改名：orders 的外键引用的列跟着改
        let mut t = store.read_table(&id, "public", "clients").unwrap().table;
        t.columns[0].name = "client_id".into();
        t.primary_key = Some(PrimaryKey { name: None, columns: vec!["client_id".into()] });
        let renames: BTreeMap<String, String> = [("client_id".into(), "id".into())].into();
        store.save_table(&id, "public", Some("clients"), t, &renames, None).unwrap();
        let orders = store.read_table(&id, "public", "orders").unwrap().table;
        assert_eq!(orders.foreign_keys[0].ref_columns, ["client_id"]);
        let r = store.renames(&id, "public").unwrap();
        assert_eq!(r.tables, BTreeMap::from([("clients".into(), "customers".into())]));
        assert_eq!(r.columns["clients"], BTreeMap::from([("client_id".into(), "id".into())]));
        // 再改一次沿用最初的原名；改回原名时删去记录
        store.rename_table(&id, "public", "clients", "buyers").unwrap();
        let r = store.renames(&id, "public").unwrap();
        assert_eq!(r.tables, BTreeMap::from([("buyers".into(), "customers".into())]));
        assert!(r.columns.contains_key("buyers"));
        store.rename_table(&id, "public", "buyers", "customers").unwrap();
        assert!(store.renames(&id, "public").unwrap().tables.is_empty());
        // 删除表去掉它的记录
        store.delete_table(&id, "public", "customers", None).unwrap();
        assert!(store.renames(&id, "public").unwrap().is_empty());
        assert!(!std::fs::read_to_string(root.join("model.yaml")).unwrap().contains("renames"));
    }

    #[test]
    fn save_new_table_keeps_unknown_fields() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("m");
        let (store, id) = sample(&root);
        let none = BTreeMap::new();
        store.save_table(&id, "sales", None, table("a/b", &["x"]), &none, None).unwrap();
        assert!(root.join("tables/sales/a%2Fb.yaml").exists());
        assert_eq!(store.summary_of(&id).unwrap().info.schemas, ["public", "sales"]);
        let err = store.save_table(&id, "sales", None, table("a/b", &["x"]), &none, None).unwrap_err();
        assert!(err.to_string().contains("already exists"), "{err}");
        // 手写的字段保存后还在
        let path = root.join("tables/public/orders.yaml");
        std::fs::write(&path, std::fs::read_to_string(&path).unwrap() + "layout:\n  x: 1\n").unwrap();
        let t = table("orders", &["id", "cust_id", "total"]);
        store.save_table(&id, "public", Some("orders"), t, &none, None).unwrap();
        assert!(std::fs::read_to_string(&path).unwrap().contains("layout:"));
    }

    fn lk(columns: &str, ref_table: &str, ref_columns: &str) -> LogicalKey {
        LogicalKey {
            name: Some(format!("lk_{columns}")),
            columns: vec![columns.into()],
            ref_schema: None,
            ref_table: ref_table.into(),
            ref_columns: vec![ref_columns.into()],
            comment: None,
        }
    }

    /// 逻辑外键：校验、设计器保存后保留、表和列改名时引用跟着改、删除。
    #[test]
    fn logical_keys_follow_renames() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("m");
        let (store, id) = sample(&root);
        let bad = [
            lk("nope", "customers", "id"),
            lk("cust_id", "customers", "nope"),
            LogicalKey { ref_columns: vec![], ..lk("cust_id", "customers", "id") },
        ];
        for k in bad {
            assert!(store.add_logical_key(&id, "public", "orders", k, None).is_err());
        }
        let fp =
            store.add_logical_key(&id, "public", "orders", lk("cust_id", "customers", "id"), None).unwrap();
        let err = store
            .add_logical_key(&id, "public", "orders", lk("cust_id", "customers", "id"), None)
            .unwrap_err();
        assert!(err.to_string().contains("already used"), "{err}");
        // 设计器保存（草稿中没有逻辑外键）后还在
        let t = store.read_table(&id, "public", "orders").unwrap().table;
        store.save_table(&id, "public", Some("orders"), t, &BTreeMap::new(), Some(&fp)).unwrap();
        let text = std::fs::read_to_string(root.join("tables/public/orders.yaml")).unwrap();
        assert!(text.contains("logical_keys:") && text.contains("ref_table: customers"), "{text}");
        // 被引用的表、列改名
        store.rename_table(&id, "public", "customers", "clients").unwrap();
        let mut c = store.read_table(&id, "public", "clients").unwrap().table;
        c.columns[0].name = "client_id".into();
        c.primary_key = Some(PrimaryKey { name: None, columns: vec!["client_id".into()] });
        store
            .save_table(&id, "public", Some("clients"), c, &[("client_id".into(), "id".into())].into(), None)
            .unwrap();
        // 本表的列改名
        let mut o = store.read_table(&id, "public", "orders").unwrap().table;
        o.columns[1].name = "client_ref".into();
        o.foreign_keys[0].columns = vec!["client_ref".into()];
        store
            .save_table(
                &id,
                "public",
                Some("orders"),
                o,
                &[("client_ref".into(), "cust_id".into())].into(),
                None,
            )
            .unwrap();
        let text = std::fs::read_to_string(root.join("tables/public/orders.yaml")).unwrap();
        let mt = super::super::file::parse_table(&text, &Dialect::new(Variant::Postgres)).unwrap();
        assert_eq!(mt.logical_keys[0].columns, ["client_ref"]);
        assert_eq!(
            (mt.logical_keys[0].ref_table.as_str(), mt.logical_keys[0].ref_columns.as_slice()),
            ("clients", &["client_id".to_string()][..])
        );
        // 比对用的表结构中没有逻辑外键
        assert!(
            store.schema_tables(&id, "public").unwrap().tables.iter().all(|(t, _)| t.foreign_keys.len() <= 1)
        );
        store.delete_logical_key(&id, "public", "orders", 0, None).unwrap();
        assert!(store.delete_logical_key(&id, "public", "orders", 0, None).is_err());
        assert!(
            !std::fs::read_to_string(root.join("tables/public/orders.yaml"))
                .unwrap()
                .contains("logical_keys")
        );
    }

    #[test]
    fn write_back_checks_fingerprints() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("m");
        let (store, id) = sample(&root);
        store.rename_table(&id, "public", "customers", "clients").unwrap();
        let fps = fingerprints(&store, &id);
        let writes = vec![
            ModelWrite::Put { table: Box::new(table("customers", &["id"])), path_of: Some("clients".into()) },
            ModelWrite::Delete { name: "orders".into() },
            ModelWrite::Put { table: Box::new(table("items", &["id"])), path_of: None },
        ];
        // 比对之后 orders 被改过：拒绝，什么都不写
        let path = root.join("tables/public/orders.yaml");
        std::fs::write(&path, std::fs::read_to_string(&path).unwrap() + "comment: x\n").unwrap();
        assert!(store.write_back(&id, "public", &fps, writes.clone(), &[]).is_err());
        assert!(root.join("tables/public/clients.yaml").exists());
        let fps = fingerprints(&store, &id);
        store.write_back(&id, "public", &fps, writes, &[("clients".into(), None)]).unwrap();
        let names: Vec<_> = store.tables(&id).unwrap().into_iter().map(|e| e.name).collect();
        assert_eq!(names, ["customers", "items"]);
        assert!(!root.join("tables/public/clients.yaml").exists());
        assert!(store.renames(&id, "public").unwrap().is_empty());
    }
}
