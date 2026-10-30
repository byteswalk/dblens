//! 内置规范。默认值随程序发布；用户改动单独保存，只记差异。
//! 条款文字直接以中英文写在 `builtin.yaml` 里（不走词条），这样用户复制、编辑后
//! 文字就是普通字符串，不依赖词条表。

use std::collections::BTreeMap;
use std::sync::OnceLock;

use dblens_core::Lang;
use serde::{Deserialize, Serialize};

use crate::model::Ruleset;

/// 一段中英文文字。
#[derive(Debug, Clone, Deserialize)]
struct Text {
    #[serde(rename = "zh-CN")]
    zh: String,
    en: String,
}

impl Text {
    fn render(&self, lang: Lang) -> String {
        match lang {
            Lang::ZhCn => self.zh.clone(),
            Lang::En => self.en.clone(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
struct RawRuleset {
    id: String,
    name: Text,
    clauses: Vec<RawClause>,
}

#[derive(Debug, Clone, Deserialize)]
struct RawClause {
    id: String,
    section: crate::model::Section,
    text: Text,
    #[serde(default = "yes")]
    enabled: bool,
    severity: crate::model::Severity,
    #[serde(default)]
    manual_note: Option<Text>,
    #[serde(default)]
    rules: Vec<crate::model::RuleRef>,
}

fn yes() -> bool {
    true
}

fn raw() -> &'static Vec<RawRuleset> {
    static DATA: OnceLock<Vec<RawRuleset>> = OnceLock::new();
    DATA.get_or_init(|| {
        serde_saphyr::from_str(include_str!("builtin.yaml")).expect("内置规范 builtin.yaml 必须能解析")
    })
}

fn build(r: &RawRuleset, lang: Lang) -> Ruleset {
    Ruleset {
        id: r.id.clone(),
        name: r.name.render(lang),
        builtin: true,
        clauses: r
            .clauses
            .iter()
            .map(|c| crate::model::Clause {
                id: c.id.clone(),
                section: c.section,
                text: c.text.render(lang),
                enabled: c.enabled,
                severity: c.severity,
                manual_note: c.manual_note.as_ref().map(|t| t.render(lang)),
                rules: c.rules.clone(),
            })
            .collect(),
        exceptions: vec![],
    }
}

/// 内置规范的编号，按发布顺序。
pub fn builtin_ids() -> Vec<String> {
    raw().iter().map(|r| r.id.clone()).collect()
}

/// 取一份内置规范，文字按语言渲染。
pub fn builtin(id: &str, lang: Lang) -> Option<Ruleset> {
    raw().iter().find(|r| r.id == id).map(|r| build(r, lang))
}

/// 用户对一条条款的改动。只记和默认值不同的字段。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ClausePatch {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub severity: Option<crate::model::Severity>,
    /// 规则参数的覆盖：规则在条款里的序号 → 参数表。
    /// 用序号而非规则编号，因为一条条款里可以有多个同编号的规则（如 name.prefix）。
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub params: BTreeMap<usize, serde_json::Map<String, serde_json::Value>>,
}

/// 一份内置规范的全部用户改动，按条款编号索引。恢复默认就是清空它。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Overrides {
    #[serde(default)]
    pub clauses: BTreeMap<String, ClausePatch>,
    /// 内置规范没有默认例外，所以用户的例外全部算差异。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exceptions: Vec<crate::model::Exception>,
}

impl Overrides {
    pub fn is_empty(&self) -> bool {
        self.clauses.is_empty() && self.exceptions.is_empty()
    }
}

/// 比较编辑后的规范和内置默认值，得出差异。未知条款（内置里没有的）忽略。
pub fn diff(base: &Ruleset, edited: &Ruleset) -> Overrides {
    let mut overrides = Overrides { exceptions: edited.exceptions.clone(), ..Default::default() };
    for ec in &edited.clauses {
        let Some(bc) = base.clauses.iter().find(|c| c.id == ec.id) else { continue };
        let mut patch = ClausePatch::default();
        if ec.enabled != bc.enabled {
            patch.enabled = Some(ec.enabled);
        }
        if ec.severity != bc.severity {
            patch.severity = Some(ec.severity);
        }
        for (i, (er, br)) in ec.rules.iter().zip(&bc.rules).enumerate() {
            if er.params != br.params {
                patch.params.insert(i, er.params.clone());
            }
        }
        if patch != ClausePatch::default() {
            overrides.clauses.insert(ec.id.clone(), patch);
        }
    }
    overrides
}

/// 把差异套回内置默认值。这样新版新增的条款也会带上。
pub fn apply(mut base: Ruleset, overrides: &Overrides) -> Ruleset {
    base.exceptions = overrides.exceptions.clone();
    for clause in &mut base.clauses {
        let Some(patch) = overrides.clauses.get(&clause.id) else { continue };
        if let Some(enabled) = patch.enabled {
            clause.enabled = enabled;
        }
        if let Some(severity) = patch.severity {
            clause.severity = severity;
        }
        for (i, rule) in clause.rules.iter_mut().enumerate() {
            if let Some(params) = patch.params.get(&i) {
                rule.params = params.clone();
            }
        }
    }
    base
}

#[cfg(test)]
mod tests {
    use dblens_core::Lang;
    use insta::assert_snapshot;
    use serde_json::{Map, json};

    use super::*;
    use crate::model::{Severity, validate};

    #[test]
    fn all_builtins_parse_and_validate() {
        assert!(!builtin_ids().is_empty());
        for id in builtin_ids() {
            let set = builtin(&id, Lang::ZhCn).unwrap();
            validate(&set).unwrap_or_else(|e| panic!("{id}: {e:?}"));
        }
    }

    #[test]
    fn clause_ids_are_unique_within_each_builtin() {
        for id in builtin_ids() {
            let set = builtin(&id, Lang::En).unwrap();
            let mut ids: Vec<_> = set.clauses.iter().map(|c| c.id.as_str()).collect();
            let n = ids.len();
            ids.sort_unstable();
            ids.dedup();
            assert_eq!(ids.len(), n, "规范 {id} 的条款编号有重复");
        }
    }

    #[test]
    fn text_is_rendered_in_the_requested_language() {
        let zh = builtin("general", Lang::ZhCn).unwrap();
        let en = builtin("general", Lang::En).unwrap();
        assert_ne!(zh.name, en.name);
        assert!(!zh.clauses.is_empty());
        for c in &zh.clauses {
            assert!(!c.text.trim().is_empty(), "条款 {} 没有中文文字", c.id);
        }
    }

    #[test]
    fn diff_records_only_changes_and_apply_restores() {
        let base = builtin("general", Lang::ZhCn).unwrap();
        let mut edited = base.clone();
        edited.clauses[0].enabled = !edited.clauses[0].enabled;
        edited.clauses[0].severity = Severity::Info;
        let overrides = diff(&base, &edited);
        assert_eq!(overrides.clauses.len(), 1);
        let rebuilt = apply(base.clone(), &overrides);
        assert_eq!(rebuilt.clauses[0].enabled, edited.clauses[0].enabled);
        assert_eq!(rebuilt.clauses[0].severity, Severity::Info);
        assert!(diff(&base, &base).clauses.is_empty());
    }

    #[test]
    fn exceptions_round_trip_through_overrides() {
        let base = builtin("general", Lang::ZhCn).unwrap();
        let mut edited = base.clone();
        edited.exceptions.push(crate::model::Exception {
            clause: "table.comment".into(),
            object: "t".into(),
            reason: "why".into(),
        });
        let overrides = diff(&base, &edited);
        assert!(!overrides.is_empty() && overrides.clauses.is_empty());
        assert_eq!(apply(base.clone(), &overrides).exceptions, edited.exceptions);
        assert!(diff(&base, &base).is_empty());
    }

    #[test]
    fn apply_keeps_new_clauses_from_a_newer_version() {
        let base = builtin("general", Lang::ZhCn).unwrap();
        // 模拟旧版本：少一条条款，用户差异里没有它
        let mut old = base.clone();
        let dropped = old.clauses.remove(0);
        let overrides = diff(&old, &old);
        let rebuilt = apply(base.clone(), &overrides);
        assert!(rebuilt.clauses.iter().any(|c| c.id == dropped.id), "新增条款应保留");
    }

    #[test]
    fn param_override_round_trips() {
        let base = builtin("strict-oltp", Lang::ZhCn).unwrap();
        let mut edited = base.clone();
        let clause =
            edited.clauses.iter_mut().find(|c| c.rules.iter().any(|r| r.id == "name.max_length")).unwrap();
        let rule = clause.rules.iter_mut().find(|r| r.id == "name.max_length").unwrap();
        let mut params = Map::new();
        params.insert("max".into(), json!(64));
        if let Some(target) = rule.params.get("target") {
            params.insert("target".into(), target.clone());
        }
        rule.params = params;
        let clause_id = clause.id.clone();
        let overrides = diff(&base, &edited);
        let rebuilt = apply(base, &overrides);
        let got = rebuilt
            .clauses
            .iter()
            .find(|c| c.id == clause_id)
            .unwrap()
            .rules
            .iter()
            .find(|r| r.id == "name.max_length")
            .unwrap();
        assert_eq!(got.params.get("max"), Some(&json!(64)));
    }

    /// 内置规范的快照，防止默认值被无意改动。
    #[test]
    fn builtin_snapshots() {
        for id in builtin_ids() {
            let set = builtin(&id, Lang::En).unwrap();
            assert_snapshot!(format!("builtin-{id}"), serde_json::to_string_pretty(&set).unwrap());
        }
    }
}
