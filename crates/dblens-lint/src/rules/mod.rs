//! 规则注册表。检查逻辑都在这里，规范只记开关、级别和参数。

mod column;
mod consistency;
mod index;
mod naming;
mod object;
mod table;

use dblens_core::Msg;
use dblens_core::schema::{Column, Index, Table};
use dblens_dialect::Family;
use serde::Serialize;
use serde_json::{Map, Value};

use crate::engine::LintInput;
use crate::model::RuleRef;

pub(crate) struct Ctx<'a> {
    pub input: &'a LintInput,
}

impl Ctx<'_> {
    pub fn family(&self) -> Family {
        self.input.family
    }

    pub fn tables(&self) -> &[Table] {
        &self.input.schema.tables
    }
}

/// 规则发现的一处问题。
pub(crate) struct Finding {
    pub object: String,
    pub table: Option<String>,
    pub message: Msg,
}

impl Finding {
    pub fn table(t: &Table, message: Msg) -> Self {
        Finding { object: t.name.clone(), table: Some(t.name.clone()), message }
    }

    pub fn column(t: &Table, c: &Column, message: Msg) -> Self {
        Finding { object: format!("{}.{}", t.name, c.name), table: Some(t.name.clone()), message }
    }

    pub fn index(t: &Table, i: &Index, message: Msg) -> Self {
        Finding { object: format!("{}.{}", t.name, i.name), table: Some(t.name.clone()), message }
    }

    pub fn object(name: &str, message: Msg) -> Self {
        Finding { object: name.to_string(), table: None, message }
    }
}

type Fix = fn() -> Msg;

pub(crate) enum Check {
    Findings(Vec<Finding>),
    Unknown(Msg),
    NotApplicable,
}

pub(crate) enum Outcome {
    Findings(Vec<Finding>, Option<Fix>),
    Unknown(Msg),
    NotApplicable,
}

pub(crate) enum ParamDefault {
    Num(u64),
    Text(&'static str),
    List(&'static [&'static str]),
    Choice(&'static [&'static str]),
}

pub(crate) struct ParamDef {
    pub key: &'static str,
    pub default: ParamDefault,
}

pub(crate) struct RuleDef {
    pub id: &'static str,
    pub params: &'static [ParamDef],
    pub fix: Option<Fix>,
    pub check: fn(&Ctx, &Params) -> Check,
}

/// 规则参数：优先取规范里的值，没有就用规则的默认值。
pub(crate) struct Params<'a> {
    values: &'a Map<String, Value>,
    defs: &'static [ParamDef],
}

impl Params<'_> {
    fn def(&self, key: &str) -> &'static ParamDefault {
        match self.defs.iter().find(|d| d.key == key) {
            Some(d) => &d.default,
            None => unreachable!("rule reads an undeclared parameter: {key}"),
        }
    }

    pub fn num(&self, key: &str) -> u64 {
        let fallback = match self.def(key) {
            ParamDefault::Num(n) => *n,
            _ => 0,
        };
        self.values.get(key).and_then(Value::as_u64).unwrap_or(fallback)
    }

    pub fn text(&self, key: &str) -> String {
        let fallback = match self.def(key) {
            ParamDefault::Text(t) => t,
            ParamDefault::Choice(c) => c.first().copied().unwrap_or_default(),
            _ => "",
        };
        self.values.get(key).and_then(Value::as_str).unwrap_or(fallback).to_string()
    }

    pub fn list(&self, key: &str) -> Vec<String> {
        if let Some(Value::Array(items)) = self.values.get(key) {
            return items.iter().filter_map(Value::as_str).map(str::to_string).collect();
        }
        match self.def(key) {
            ParamDefault::List(items) => items.iter().map(|s| s.to_string()).collect(),
            _ => vec![],
        }
    }
}

fn all() -> impl Iterator<Item = &'static RuleDef> {
    [naming::RULES, table::RULES, column::RULES, index::RULES, object::RULES, consistency::RULES]
        .into_iter()
        .flatten()
}

pub(crate) fn exists(id: &str) -> bool {
    all().any(|r| r.id == id)
}

pub(crate) fn run(ctx: &Ctx, rule: &RuleRef) -> Outcome {
    let Some(def) = all().find(|r| r.id == rule.id) else {
        return Outcome::Unknown(dblens_core::msg!("lint.rule_unknown", id = rule.id));
    };
    match (def.check)(ctx, &Params { values: &rule.params, defs: def.params }) {
        Check::Findings(findings) => Outcome::Findings(findings, def.fix),
        Check::Unknown(reason) => Outcome::Unknown(reason),
        Check::NotApplicable => Outcome::NotApplicable,
    }
}

/// 规则的编号和参数定义。说明文字和参数名在前端词条里。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RuleInfo {
    pub id: &'static str,
    pub params: Vec<ParamInfo>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ParamInfo {
    pub key: &'static str,
    /// number、text、list 或 choice。
    pub kind: &'static str,
    pub default: Value,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub choices: Vec<&'static str>,
}

pub fn rule_infos() -> Vec<RuleInfo> {
    all()
        .map(|r| RuleInfo {
            id: r.id,
            params: r
                .params
                .iter()
                .map(|p| {
                    let (kind, default, choices) = match &p.default {
                        ParamDefault::Num(n) => ("number", Value::from(*n), vec![]),
                        ParamDefault::Text(t) => ("text", Value::from(*t), vec![]),
                        ParamDefault::List(l) => ("list", Value::from(l.to_vec()), vec![]),
                        ParamDefault::Choice(c) => ("choice", Value::from(c.first().copied()), c.to_vec()),
                    };
                    ParamInfo { key: p.key, kind, default, choices }
                })
                .collect(),
        })
        .collect()
}

/// 名字比较不分大小写。
pub(crate) fn same_name(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}

pub(crate) fn upper(s: &str) -> String {
    s.to_ascii_uppercase()
}

#[cfg(test)]
pub(crate) mod testing {
    use dblens_core::schema::{Column, Index, IndexColumn, PrimaryKey, Schema, Table};
    use dblens_core::{ColumnType, Lang, LogicalType};
    use serde_json::{Map, Value};

    use super::*;

    pub fn col(name: &str, native: &str, logical: LogicalType) -> Column {
        Column {
            name: name.into(),
            ordinal: 1,
            data_type: ColumnType { native: native.into(), logical },
            nullable: false,
            default: None,
            comment: Some("备注".into()),
            auto_increment: false,
            generated: None,
        }
    }

    pub fn int(name: &str) -> Column {
        col(name, "int", LogicalType::Int { bits: 32 })
    }

    pub fn varchar(name: &str, len: u32) -> Column {
        col(name, &format!("varchar({len})"), LogicalType::VarChar { len: Some(len) })
    }

    pub fn table(name: &str, columns: Vec<Column>) -> Table {
        Table { name: name.into(), columns, ..Default::default() }
    }

    pub fn pk(columns: &[&str]) -> Option<PrimaryKey> {
        Some(PrimaryKey { name: None, columns: columns.iter().map(|c| c.to_string()).collect() })
    }

    pub fn index(name: &str, columns: &[&str]) -> Index {
        Index {
            name: name.into(),
            columns: columns.iter().map(|c| IndexColumn { name: c.to_string(), descending: false }).collect(),
            unique: false,
            method: None,
        }
    }

    pub fn schema(tables: Vec<Table>) -> Schema {
        Schema { name: "s".into(), tables, ..Default::default() }
    }

    pub fn input(schema: Schema, family: Family) -> LintInput {
        LintInput { schema, family, objects_known: true }
    }

    /// 运行一条规则，返回问题的对象名。
    pub fn objects(input: &LintInput, id: &str, params: Value) -> Vec<String> {
        match outcome(input, id, params) {
            Outcome::Findings(f, _) => f.into_iter().map(|f| f.object).collect(),
            Outcome::Unknown(m) => panic!("unknown: {}", m.render(Lang::En)),
            Outcome::NotApplicable => panic!("not applicable"),
        }
    }

    pub fn outcome(input: &LintInput, id: &str, params: Value) -> Outcome {
        let params: Map<String, Value> = params.as_object().cloned().unwrap_or_default();
        run(&Ctx { input }, &RuleRef { id: id.into(), params, databases: vec![] })
    }

    pub fn tables(tables: Vec<Table>) -> LintInput {
        input(schema(tables), Family::Postgres)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rule_ids_are_unique() {
        let mut ids: Vec<_> = all().map(|r| r.id).collect();
        let n = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), n);
    }
}
