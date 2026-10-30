//! 结构迁移（M5a 设计文档第 3、4 节）：把源 schema 的表和序列按目标方言转换成目标结构，
//! 给出逐项提示，再生成两步的 DDL（先建序列和表，再建索引和约束）。不做任何 IO。

pub mod data;
mod expr;
mod names;
mod script;
#[cfg(test)]
mod tests;
mod types;

use std::collections::{BTreeMap, BTreeSet};

use dblens_core::schema::{CheckConstraint, Column, ForeignKey, Index, Sequence, Table, UniqueConstraint};
use dblens_core::{ColumnType, Msg, msg};
use serde::{Deserialize, Serialize};

use crate::design::diff::{is_expression, text};
use crate::{Dialect, Family};

pub use expr::convert_expr;
pub use names::{NameCase, max_ident_len};
pub use script::{ExistingAction, MigrationStatement, StatementKind, migration_script, script_text};

/// 迁移选项。`name_case` 为 None 时按目标库的习惯（见 `NameCase::for_target`）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MigrationOptions {
    #[serde(default)]
    pub name_case: Option<NameCase>,
    #[serde(default = "yes")]
    pub comments: bool,
    #[serde(default = "yes")]
    pub checks: bool,
}

fn yes() -> bool {
    true
}

impl Default for MigrationOptions {
    fn default() -> Self {
        Self { name_case: None, comments: true, checks: true }
    }
}

/// 全局类型规则：源类型名（可带参数，如 `NUMBER(1)`）→ 目标类型文本。不带参数时匹配该类型的所有写法。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TypeRule {
    pub source: String,
    pub target: String,
}

/// 一列的覆盖。`default` 为空串表示不要默认值；`accept` 表示确认把不认识的类型按文本迁移。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ColumnOverride {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, rename = "type", skip_serializing_if = "Option::is_none")]
    pub data_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<String>,
    /// 生成列的表达式；空串表示改为普通列。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expression: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub skip: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub accept: bool,
}

/// 一张表的覆盖。`checks`：源检查约束名 → 目标表达式，空串表示不迁。`filter`：迁数据时的过滤条件（源库写法）；
/// `structure_only`：只迁结构不迁数据（M5b）。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TableOverride {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub skip: bool,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub columns: BTreeMap<String, ColumnOverride>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub checks: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filter: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub structure_only: bool,
}

/// 一个序列的覆盖。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SequenceOverride {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub skip: bool,
}

/// 映射设置：迁移方案中与结构转换有关的部分。覆盖按源对象名。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MigrationSettings {
    #[serde(default)]
    pub options: MigrationOptions,
    #[serde(default)]
    pub type_rules: Vec<TypeRule>,
    #[serde(default)]
    pub tables: BTreeMap<String, TableOverride>,
    #[serde(default)]
    pub sequences: BTreeMap<String, SequenceOverride>,
}

/// 提示的级别：等价（只是说明）、有损（照常迁移）、不支持（处理之前不能开始）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NoteLevel {
    Info,
    Lossy,
    Unsupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ObjectKind {
    Table,
    Sequence,
}

/// 一条提示。`object` 是源对象名；`column` 是源列名；`item` 是索引或约束的源名字（没有名字时为列清单）。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MigrationNote {
    pub kind: ObjectKind,
    pub object: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub column: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub item: Option<String>,
    pub level: NoteLevel,
    pub message: Msg,
}

/// 一列的对应：源列名 → 目标列名（不迁时为 None）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ColumnMapping {
    pub source: String,
    pub target: Option<String>,
}

/// 一张表的转换结果。`target` 是完整的目标表（含索引和约束）；`exists`：目标 schema 中已有同名表。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TableMapping {
    pub source: String,
    pub target: Table,
    pub columns: Vec<ColumnMapping>,
    pub exists: bool,
}

/// 一个序列的转换结果。`exists`：目标 schema 中已有同名序列（执行时跳过）。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SequenceMapping {
    pub source: String,
    pub target: Sequence,
    pub exists: bool,
}

/// 转换的结果：迁移的表和序列（被跳过的不在其中）、全部提示。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MigrationDraft {
    pub tables: Vec<TableMapping>,
    pub sequences: Vec<SequenceMapping>,
    pub notes: Vec<MigrationNote>,
}

impl MigrationDraft {
    /// 有不支持的项：处理之前不能开始。
    pub fn blocked(&self) -> bool {
        self.notes.iter().any(|n| n.level == NoteLevel::Unsupported)
    }
}

/// 转换的输入：源的表和序列（已按迁移范围筛选）、目标 schema 中已有的表和序列名。
pub struct MigrationInput<'a> {
    pub source: Dialect,
    pub target: Dialect,
    pub tables: &'a [Table],
    pub sequences: &'a [Sequence],
    pub target_tables: &'a [Table],
    pub target_sequences: &'a [String],
}

/// 收集提示。
struct Notes {
    kind: ObjectKind,
    object: String,
    out: Vec<MigrationNote>,
}

impl Notes {
    fn push(&mut self, column: Option<&str>, item: Option<&str>, level: NoteLevel, message: Msg) {
        self.out.push(MigrationNote {
            kind: self.kind,
            object: self.object.clone(),
            column: column.map(Into::into),
            item: item.map(Into::into),
            level,
            message,
        });
    }
}

/// 把源结构转换成目标结构。
pub fn migrate(input: &MigrationInput, settings: &MigrationSettings) -> MigrationDraft {
    let (src, tgt) = (&input.source, &input.target);
    let case = settings.options.name_case.unwrap_or_else(|| NameCase::for_target(tgt));
    let mut notes = Vec::new();

    // 序列：源序列名 → 目标序列名（默认值中的 nextval 要换成目标名）
    let owned = owned_sequences(src, input.tables);
    let mut sequences = Vec::new();
    let mut seq_names = BTreeMap::new();
    for s in input.sequences {
        let ov = settings.sequences.get(&s.name).cloned().unwrap_or_default();
        let mut n = Notes { kind: ObjectKind::Sequence, object: s.name.clone(), out: vec![] };
        if ov.skip || is_system_sequence(src, &s.name) {
            continue;
        }
        if owned.contains(&s.name.to_lowercase()) {
            n.push(None, None, NoteLevel::Info, msg!("migrate.serial_sequence"));
            notes.extend(n.out);
            continue;
        }
        if tgt.family() == Family::MySql {
            n.push(None, None, NoteLevel::Lossy, msg!("migrate.no_sequences"));
            notes.extend(n.out);
            continue;
        }
        let name = ov.name.clone().unwrap_or_else(|| case.apply(&s.name));
        seq_names.insert(s.name.to_lowercase(), name.clone());
        let target = types::sequence(tgt, s, &name, &mut n);
        let exists = input.target_sequences.iter().any(|x| x.eq_ignore_ascii_case(&name));
        if exists {
            n.push(None, None, NoteLevel::Info, msg!("migrate.sequence_exists"));
        }
        sequences.push(SequenceMapping { source: s.name.clone(), target, exists });
        notes.extend(n.out);
    }

    // 表名：源表名 → 目标表名（外键要引用目标名）
    let mut table_names = BTreeMap::new();
    for t in input.tables {
        let ov = settings.tables.get(&t.name);
        if ov.is_some_and(|o| o.skip) {
            continue;
        }
        let name = ov.and_then(|o| o.name.clone()).unwrap_or_else(|| case.apply(&t.name));
        table_names.insert(t.name.clone(), name);
    }

    let ctx = Context { src, tgt, case, settings, sequences: &seq_names, tables: &table_names, input };
    let mut tables = Vec::new();
    for t in input.tables.iter().filter(|t| table_names.contains_key(&t.name)) {
        let mut n = Notes { kind: ObjectKind::Table, object: t.name.clone(), out: vec![] };
        let (target, columns) = ctx.table(t, &mut n);
        let exists = input.target_tables.iter().any(|x| x.name.eq_ignore_ascii_case(&target.name));
        tables.push(TableMapping { source: t.name.clone(), target, columns, exists });
        notes.extend(n.out);
    }
    names::check(tgt, &tables, &sequences, &mut notes);
    MigrationDraft { tables, sequences, notes }
}

/// PG 的 serial 列自带的序列（默认值为 nextval 的自增列）：随自增列一起处理，不单独迁移。
fn owned_sequences(src: &Dialect, tables: &[Table]) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    if src.family() != Family::Postgres {
        return out;
    }
    for c in tables.iter().flat_map(|t| &t.columns).filter(|c| c.auto_increment) {
        if let Some(name) = c.default.as_deref().and_then(expr::nextval_target) {
            out.insert(name.to_lowercase());
        }
    }
    out
}

/// Oracle 标识列的系统序列（`ISEQ$$_…`）不迁移。
fn is_system_sequence(src: &Dialect, name: &str) -> bool {
    src.family() == Family::Oracle && name.starts_with("ISEQ$$_")
}

struct Context<'a> {
    src: &'a Dialect,
    tgt: &'a Dialect,
    case: NameCase,
    settings: &'a MigrationSettings,
    sequences: &'a BTreeMap<String, String>,
    tables: &'a BTreeMap<String, String>,
    input: &'a MigrationInput<'a>,
}

impl Context<'_> {
    fn same_family(&self) -> bool {
        self.src.family() == self.tgt.family()
    }

    fn same_product(&self) -> bool {
        self.src.variant() == self.tgt.variant() && self.src.compat() == self.tgt.compat()
    }

    /// MySQL 系为外键自动建的索引（与外键同名、列相同）：外键在目标中照样建，这个索引不单独迁
    /// （在 PG、Oracle、达梦中索引和约束同在一个名字空间，会重名）。
    fn fk_index(&self, t: &Table, x: &Index) -> bool {
        self.src.family() == Family::MySql
            && t.foreign_keys.iter().any(|f| {
                f.name.as_deref() == Some(x.name.as_str())
                    && f.columns.len() == x.columns.len()
                    && f.columns.iter().zip(&x.columns).all(|(a, b)| *a == b.name)
            })
    }

    /// 约束名：系统生成的名字（MySQL 的主键 `PRIMARY`、Oracle 的 `SYS_C…`、达梦的 `CONS…`、SQL Server 的 `PK__…` 等）
    /// 不带过去，由目标库生成。
    fn constraint(&self, name: &Option<String>) -> Option<String> {
        let n = text(name)?;
        let digits = |p: &str| {
            n.strip_prefix(p).is_some_and(|r| !r.is_empty() && r.chars().all(|c| c.is_ascii_digit()))
        };
        let system = match self.src.family() {
            Family::MySql => n.eq_ignore_ascii_case("PRIMARY"),
            Family::Oracle => digits("SYS_C"),
            Family::Dameng => digits("CONS"),
            Family::SqlServer => ["PK__", "UQ__", "FK__", "CK__", "DF__"].iter().any(|p| n.starts_with(p)),
            Family::Postgres => false,
        };
        (!system).then(|| self.case.apply(n))
    }

    fn table(&self, t: &Table, n: &mut Notes) -> (Table, Vec<ColumnMapping>) {
        let ov = self.settings.tables.get(&t.name).cloned().unwrap_or_default();
        let name = self.tables[&t.name].clone();
        let mut mapping = Vec::new();
        let mut cols = BTreeMap::new();
        let mut columns = Vec::new();
        for c in &t.columns {
            let cov = ov.columns.get(&c.name).cloned().unwrap_or_default();
            if cov.skip {
                mapping.push(ColumnMapping { source: c.name.clone(), target: None });
                continue;
            }
            let cname = cov.name.clone().unwrap_or_else(|| self.case.apply(&c.name));
            cols.insert(c.name.clone(), cname.clone());
            mapping.push(ColumnMapping { source: c.name.clone(), target: Some(cname.clone()) });
            columns.push((c, cov, cname));
        }
        let comments = self.settings.options.comments;
        let mut out_cols = Vec::new();
        for (i, (c, cov, cname)) in columns.into_iter().enumerate() {
            out_cols.push(self.column(c, &cov, cname, i as u32 + 1, &cols, comments, n));
        }
        let mut target = Table {
            name,
            comment: if comments { t.comment.clone() } else { None },
            columns: out_cols,
            ..Default::default()
        };
        let lob = |col: &str| {
            target
                .columns
                .iter()
                .find(|c| c.name == col)
                .is_some_and(|c| types::is_lob(self.tgt, &c.data_type.logical))
        };
        let map_cols =
            |list: &[String]| -> Option<Vec<String>> { list.iter().map(|c| cols.get(c).cloned()).collect() };

        if let Some(pk) = &t.primary_key {
            match map_cols(&pk.columns) {
                Some(columns) if columns.iter().any(|c| lob(c)) => {
                    n.push(None, pk.name.as_deref(), NoteLevel::Unsupported, msg!("migrate.lob_key"));
                }
                Some(columns) => {
                    target.primary_key =
                        Some(dblens_core::schema::PrimaryKey { name: self.constraint(&pk.name), columns })
                }
                None => n.push(
                    None,
                    pk.name.as_deref(),
                    NoteLevel::Unsupported,
                    msg!("migrate.key_column_skipped"),
                ),
            }
        }
        for u in &t.uniques {
            let item = item_name(&u.name, &u.columns);
            match map_cols(&u.columns) {
                Some(columns) if columns.iter().any(|c| lob(c)) => {
                    n.push(None, Some(&item), NoteLevel::Lossy, msg!("migrate.lob_index"))
                }
                Some(columns) => {
                    target.uniques.push(UniqueConstraint { name: self.constraint(&u.name), columns })
                }
                None => n.push(None, Some(&item), NoteLevel::Lossy, msg!("migrate.item_column_skipped")),
            }
        }
        for x in t.indexes.iter().filter(|x| !self.fk_index(t, x)) {
            if let Some(i) = self.index(x, &cols, &lob, n) {
                target.indexes.push(i);
            }
        }
        for f in &t.foreign_keys {
            if let Some(f) = self.foreign_key(f, &cols, n) {
                target.foreign_keys.push(f);
            }
        }
        if self.settings.options.checks {
            for c in &t.checks {
                if let Some(c) = self.check(c, &ov, &cols, n) {
                    target.checks.push(c);
                }
            }
        }
        if t.partition.is_some() {
            n.push(None, None, NoteLevel::Lossy, msg!("migrate.partition"));
        }
        (target, mapping)
    }

    #[allow(clippy::too_many_arguments)]
    fn column(
        &self,
        c: &Column,
        ov: &ColumnOverride,
        name: String,
        ordinal: u32,
        cols: &BTreeMap<String, String>,
        comments: bool,
        n: &mut Notes,
    ) -> Column {
        let at = Some(c.name.as_str());
        let (native, logical) =
            types::column_type(self.src, self.tgt, c, ov, &self.settings.type_rules, |l, m| {
                n.push(at, None, l, m)
            });
        let mut out = Column {
            name,
            ordinal,
            data_type: ColumnType { native, logical },
            nullable: c.nullable,
            default: None,
            comment: if comments { c.comment.clone() } else { None },
            auto_increment: false,
            generated: None,
        };
        // 生成列
        if let Some(g) = text(&c.generated) {
            let converted = match ov.expression.as_deref() {
                Some(e) => Some(e.trim().to_string()).filter(|e| !e.is_empty()),
                None => self.expression(g, cols),
            };
            match converted {
                Some(e) => out.generated = Some(e),
                None => n.push(at, None, NoteLevel::Lossy, msg!("migrate.generated_dropped", expression = g)),
            }
            return out;
        }
        // 自增
        if c.auto_increment {
            if types::identity_supported(self.tgt) {
                out.auto_increment = true;
                if ov.data_type.is_none() {
                    let ColumnType { native, logical } = out.data_type.clone();
                    let (native, logical) = types::identity_type(self.tgt, native, logical);
                    out.data_type = ColumnType { native, logical };
                }
            } else {
                n.push(at, None, NoteLevel::Unsupported, msg!("migrate.identity_unsupported"));
            }
            return out;
        }
        // 默认值
        match ov.default.as_deref() {
            Some(d) => out.default = Some(d.trim().to_string()).filter(|d| !d.is_empty()),
            None => {
                if let Some(d) = text(&c.default) {
                    // 同一产品原样；同家族的其他产品先尝试转换，转换不了再原样
                    out.default = if self.same_product() {
                        Some(d.to_string())
                    } else {
                        match expr::convert_default(self.src, self.tgt, d, &out.data_type, self.sequences) {
                            Ok(v) => v,
                            Err(()) if self.same_family() => Some(d.to_string()),
                            Err(()) => {
                                n.push(
                                    at,
                                    None,
                                    NoteLevel::Lossy,
                                    msg!("migrate.default_dropped", default = d),
                                );
                                None
                            }
                        }
                    };
                }
            }
        }
        out
    }

    /// 表达式（检查约束、生成列、索引表达式）：同家族且列名不变时原样，否则只转换简单表达式。
    fn expression(&self, e: &str, cols: &BTreeMap<String, String>) -> Option<String> {
        if self.same_family() && cols.iter().all(|(a, b)| a == b) {
            return Some(e.to_string());
        }
        convert_expr(self.src, self.tgt, e, cols)
    }

    fn index(
        &self,
        x: &Index,
        cols: &BTreeMap<String, String>,
        lob: &dyn Fn(&str) -> bool,
        n: &mut Notes,
    ) -> Option<Index> {
        let mut out =
            Index { name: self.case.apply(&x.name), columns: Vec::new(), unique: x.unique, method: None };
        for c in &x.columns {
            let name = if is_expression(&c.name) {
                match self.expression(&c.name, cols) {
                    Some(e) => e,
                    None => {
                        n.push(
                            None,
                            Some(&x.name),
                            NoteLevel::Lossy,
                            msg!("migrate.index_expression", expression = c.name),
                        );
                        return None;
                    }
                }
            } else {
                match cols.get(&c.name) {
                    Some(t) if lob(t) => {
                        n.push(None, Some(&x.name), NoteLevel::Lossy, msg!("migrate.lob_index"));
                        return None;
                    }
                    Some(t) => t.clone(),
                    None => {
                        n.push(None, Some(&x.name), NoteLevel::Lossy, msg!("migrate.item_column_skipped"));
                        return None;
                    }
                }
            };
            out.columns.push(dblens_core::schema::IndexColumn { name, descending: c.descending });
        }
        if let Some(m) = text(&x.method) {
            if self.same_family() {
                out.method = Some(m.to_string());
            } else if !["btree", "normal"].iter().any(|d| m.eq_ignore_ascii_case(d)) {
                n.push(None, Some(&x.name), NoteLevel::Lossy, msg!("migrate.index_method", method = m));
            }
        }
        Some(out)
    }

    fn foreign_key(
        &self,
        f: &ForeignKey,
        cols: &BTreeMap<String, String>,
        n: &mut Notes,
    ) -> Option<ForeignKey> {
        let item = item_name(&f.name, &f.columns);
        let Some(columns) = f.columns.iter().map(|c| cols.get(c).cloned()).collect::<Option<Vec<_>>>() else {
            n.push(None, Some(&item), NoteLevel::Lossy, msg!("migrate.item_column_skipped"));
            return None;
        };
        let same_schema = f.ref_schema.is_none() || self.input.tables.iter().any(|x| x.name == f.ref_table);
        // 引用的表：在迁移范围内（用目标名和目标列名），或目标中已有同名表
        let (ref_table, ref_columns) = match self.tables.get(&f.ref_table).filter(|_| same_schema) {
            Some(name) => {
                let src_ref = self.input.tables.iter().find(|x| x.name == f.ref_table);
                let ov = self.settings.tables.get(&f.ref_table);
                let map = |c: &String| -> Option<String> {
                    let cov = ov.and_then(|o| o.columns.get(c));
                    if cov.is_some_and(|o| o.skip)
                        || src_ref.is_some_and(|r| !r.columns.iter().any(|x| &x.name == c))
                    {
                        return None;
                    }
                    Some(cov.and_then(|o| o.name.clone()).unwrap_or_else(|| self.case.apply(c)))
                };
                match f.ref_columns.iter().map(map).collect::<Option<Vec<_>>>() {
                    Some(rc) => (name.clone(), rc),
                    None => {
                        n.push(None, Some(&item), NoteLevel::Lossy, msg!("migrate.item_column_skipped"));
                        return None;
                    }
                }
            }
            None => {
                let name = self.case.apply(&f.ref_table);
                if !self.input.target_tables.iter().any(|x| x.name.eq_ignore_ascii_case(&name)) {
                    n.push(
                        None,
                        Some(&item),
                        NoteLevel::Lossy,
                        msg!("migrate.fk_outside", table = f.ref_table),
                    );
                    return None;
                }
                (name, f.ref_columns.iter().map(|c| self.case.apply(c)).collect())
            }
        };
        let mut out = ForeignKey {
            name: self.constraint(&f.name),
            columns,
            ref_schema: None,
            ref_table,
            ref_columns,
            on_delete: None,
            on_update: None,
        };
        for (what, action, slot) in
            [("DELETE", &f.on_delete, &mut out.on_delete), ("UPDATE", &f.on_update, &mut out.on_update)]
        {
            let Some(a) = text(action).map(str::to_ascii_uppercase) else { continue };
            if a == "NO ACTION" || a == "RESTRICT" {
                continue;
            }
            if types::fk_action_supported(self.tgt, what, &a) {
                *slot = Some(a);
            } else {
                n.push(
                    None,
                    Some(&item),
                    NoteLevel::Lossy,
                    msg!("migrate.fk_action", action = format!("ON {what} {a}")),
                );
            }
        }
        Some(out)
    }

    fn check(
        &self,
        c: &CheckConstraint,
        ov: &TableOverride,
        cols: &BTreeMap<String, String>,
        n: &mut Notes,
    ) -> Option<CheckConstraint> {
        let item = c.name.clone().unwrap_or_else(|| c.expression.clone());
        let expression = match c.name.as_ref().and_then(|x| ov.checks.get(x)) {
            Some(e) if e.trim().is_empty() => return None,
            Some(e) => e.trim().to_string(),
            None => match self.expression(&c.expression, cols) {
                Some(e) => e,
                None => {
                    n.push(
                        None,
                        Some(&item),
                        NoteLevel::Lossy,
                        msg!("migrate.check_dropped", expression = c.expression),
                    );
                    return None;
                }
            },
        };
        Some(CheckConstraint { name: self.constraint(&c.name), expression })
    }
}

/// 索引、约束在提示中的名字：有名字用名字，否则用列清单。
fn item_name(name: &Option<String>, columns: &[String]) -> String {
    name.clone().unwrap_or_else(|| columns.join(", "))
}
