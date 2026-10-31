//! 数据库设计规范检查。本 crate 只做计算：输入结构模型和规范，输出每条条款的结论，不连库。

mod builtin;
mod engine;
mod model;
mod pinyin;
mod rules;

pub use builtin::{ClausePatch, Overrides, apply, builtin, builtin_ids, diff};
pub use engine::{LintInput, check, family_key};
pub use model::{
    Clause, ClauseResult, CleanVerdict, Exception, Issue, Review, RuleRef, Ruleset, Section, Severity,
    Summary, Verdict, WaivedIssue, summarize, validate,
};
pub use rules::{ParamInfo, RuleInfo, rule_infos};
