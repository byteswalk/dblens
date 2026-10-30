//! 元数据查询模板：每个家族一个 `sql/<家族>.sql`，兼容库可以有只含差异 key 的覆盖文件。
//! 文件按 `-- [key]` 行分段，每段是一个 minijinja 模板。
//!
//! 模板上下文里只有布尔值和版本号，取不到参数值；参数只能用 `{{ bind("schema") }}` 生成占位符，
//! 保证一律走绑定变量。

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use dblens_core::{Error, Result, Value, msg};
use minijinja::syntax::SyntaxConfig;
use minijinja::{Environment, UndefinedBehavior, context};

use super::{MetaArgs, MetaKey, MetaQuery};
use crate::{Family, Variant, ident};

const MYSQL: &str = include_str!("../../sql/mysql.sql");
const MARIADB: &str = include_str!("../../sql/mariadb.sql");
const POSTGRES: &str = include_str!("../../sql/postgres.sql");
const ORACLE: &str = include_str!("../../sql/oracle.sql");
const DAMENG: &str = include_str!("../../sql/dameng.sql");
const KINGBASE: &str = include_str!("../../sql/kingbase.sql");
const OPENGAUSS: &str = include_str!("../../sql/opengauss.sql");
const TIDB: &str = include_str!("../../sql/tidb.sql");
const OCEANBASE: &str = include_str!("../../sql/oceanbase.sql");
const SQLSERVER: &str = include_str!("../../sql/sqlserver.sql");

pub(crate) const ALL_VARIANTS: [Variant; 13] = [
    Variant::MySql,
    Variant::MariaDb,
    Variant::TiDb,
    Variant::OceanBase,
    Variant::PolarDb,
    Variant::Postgres,
    Variant::Kingbase,
    Variant::OpenGauss,
    Variant::GaussDb,
    Variant::HighGo,
    Variant::Oracle,
    Variant::Dameng,
    Variant::SqlServer,
];

/// 把模板文件按 `-- [key]` 行拆成 key → 模板正文。
pub(crate) fn parse_sections(text: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    let mut current: Option<(String, String)> = None;
    for line in text.lines() {
        let t = line.trim();
        if let Some(key) = t.strip_prefix("-- [").and_then(|r| r.strip_suffix(']')) {
            if let Some((k, body)) = current.take() {
                out.insert(k, body.trim().to_string());
            }
            current = Some((key.trim().to_string(), String::new()));
        } else if let Some((_, body)) = current.as_mut() {
            body.push_str(line);
            body.push('\n');
        }
    }
    if let Some((k, body)) = current {
        out.insert(k, body.trim().to_string());
    }
    out
}

fn base_file(family: Family) -> &'static str {
    match family {
        Family::MySql => MYSQL,
        Family::Postgres => POSTGRES,
        Family::Oracle => ORACLE,
        Family::Dameng => DAMENG,
        Family::SqlServer => SQLSERVER,
    }
}

fn override_file(variant: Variant) -> Option<&'static str> {
    match variant {
        Variant::MariaDb => Some(MARIADB),
        Variant::TiDb => Some(TIDB),
        Variant::OceanBase => Some(OCEANBASE),
        Variant::Kingbase => Some(KINGBASE),
        Variant::OpenGauss => Some(OPENGAUSS),
        _ => None,
    }
}

/// 某个变体的全部模板：家族基础模板，再用变体覆盖文件中的同名 key 替换；
/// 覆盖文件里正文为空的 key 表示该变体没有这种查询，从结果中删除。
pub(crate) fn sections(variant: Variant) -> &'static HashMap<String, String> {
    static CACHE: OnceLock<HashMap<Variant, HashMap<String, String>>> = OnceLock::new();
    let all = CACHE.get_or_init(|| {
        ALL_VARIANTS
            .iter()
            .map(|&v| {
                let mut m = parse_sections(base_file(v.family()));
                if let Some(o) = override_file(v) {
                    m.extend(parse_sections(o));
                    m.retain(|_, body| !body.is_empty());
                }
                (v, m)
            })
            .collect()
    });
    &all[&variant]
}

/// LIKE 模式：包含匹配，`!` 作转义符（模板里写 `ESCAPE '!'`）。
pub(crate) fn like_pattern(filter: &str) -> String {
    let mut p = String::from("%");
    for c in filter.chars() {
        if matches!(c, '%' | '_' | '!') {
            p.push('!');
        }
        p.push(c);
    }
    p.push('%');
    p
}

/// 渲染一个模板。`version` 是编码后的版本号，未知时传 `u32::MAX`（按最新版本处理）；
/// `oracle_mode` 是人大金仓、openGauss 的 Oracle 兼容模式。
pub(crate) fn render(
    key: MetaKey,
    template: &str,
    family: Family,
    version: u32,
    oracle_mode: bool,
    args: &MetaArgs,
) -> Result<MetaQuery> {
    let names: Arc<Mutex<Vec<String>>> = Arc::default();
    let mut env = Environment::new();
    env.set_undefined_behavior(UndefinedBehavior::Strict);
    let syntax = SyntaxConfig::builder()
        .trim_blocks(true)
        .lstrip_blocks(true)
        .build()
        .map_err(|e| Error::internal(msg!("meta.syntax_config", error = e)))?;
    env.set_syntax(syntax);
    let recorded = names.clone();
    env.add_function("bind", move |name: String| -> String {
        let mut list = recorded.lock().unwrap();
        list.push(name);
        ident::placeholder(family, list.len())
    });
    let k = key.as_str();
    let tmpl = env
        .template_from_str(template)
        .map_err(|e| Error::internal(msg!("meta.template_parse", key = k, error = e)))?;
    let sql = tmpl
        .render(context! {
            table => args.table.is_some(),
            name => args.name.is_some(),
            kind => args.kind.is_some(),
            filter => args.filter.is_some(),
            version => version,
            oracle_mode => oracle_mode,
        })
        .map_err(|e| Error::internal(msg!("meta.template_render", key = k, error = e)))?;
    let params = names
        .lock()
        .unwrap()
        .iter()
        .map(|n| {
            let v = match n.as_str() {
                "schema" => args.schema.clone(),
                "table" => args.table.clone(),
                "name" => args.name.clone(),
                "kind" => args.kind.clone(),
                "filter" => args.filter.as_deref().map(like_pattern),
                other => return Err(Error::internal(msg!("meta.unknown_param", key = k, param = other))),
            };
            Ok(v.map_or(Value::Null, Value::String))
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(MetaQuery { key, sql: sql.trim().to_string(), params })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_sections_and_overrides_win() {
        let base = parse_sections("-- 注释行不属于任何段\n-- [a]\nSELECT 1\n\n-- [b]\nSELECT 2\n");
        assert_eq!(base.len(), 2);
        assert_eq!(base["a"], "SELECT 1");
        assert_eq!(base["b"], "SELECT 2");
        let mut merged = base.clone();
        merged.extend(parse_sections("-- [b]\nSELECT 3"));
        assert_eq!(merged["b"], "SELECT 3");
    }

    #[test]
    fn like_pattern_escapes_wildcards() {
        assert_eq!(like_pattern("emp"), "%emp%");
        assert_eq!(like_pattern("a_b%c!d"), "%a!_b!%c!!d%");
        assert_eq!(like_pattern("数洞"), "%数洞%");
    }

    fn args() -> MetaArgs {
        MetaArgs { schema: Some("hr".into()), table: Some("emp".into()), ..Default::default() }
    }

    const T: &str = "SELECT x FROM t WHERE s = {{ bind(\"schema\") }}\n\
                     {% if table %}\n  AND n = {{ bind(\"table\") }}\n{% endif %}\n\
                     {% if version >= 80016 %}\n  AND new = 1\n{% endif %}";

    #[test]
    fn renders_placeholders_per_family_and_binds_in_order() {
        let q = render(MetaKey::Tables, T, Family::Postgres, 90000, false, &args()).unwrap();
        assert_eq!(q.sql, "SELECT x FROM t WHERE s = $1\n  AND n = $2\n  AND new = 1");
        assert_eq!(q.params, vec![Value::String("hr".into()), Value::String("emp".into())]);
        let q = render(MetaKey::Tables, T, Family::Oracle, 80000, false, &args()).unwrap();
        assert_eq!(q.sql, "SELECT x FROM t WHERE s = :1\n  AND n = :2");
        let q = render(
            MetaKey::Tables,
            T,
            Family::MySql,
            80000,
            false,
            &MetaArgs { schema: Some("hr".into()), ..Default::default() },
        )
        .unwrap();
        assert_eq!(q.sql, "SELECT x FROM t WHERE s = ?");
        assert_eq!(q.params.len(), 1);
    }

    #[test]
    fn filter_is_bound_as_escaped_pattern_and_values_cannot_be_printed() {
        let a = MetaArgs { filter: Some("a_b".into()), ..Default::default() };
        let q = render(
            MetaKey::Tables,
            "WHERE x LIKE {{ bind(\"filter\") }} OR y LIKE {{ bind(\"filter\") }}",
            Family::Oracle,
            0,
            false,
            &a,
        )
        .unwrap();
        assert_eq!(q.sql, "WHERE x LIKE :1 OR y LIKE :2");
        assert_eq!(q.params, vec![Value::String("%a!_b%".into()), Value::String("%a!_b%".into())]);
        // 上下文里的 table 只是布尔值，模板不能借它把参数值拼进 SQL
        let q = render(MetaKey::Tables, "{{ table }}", Family::MySql, 0, false, &args()).unwrap();
        assert_eq!(q.sql, "True");
        // 未定义的变量直接报错，而不是悄悄渲染成空串
        assert!(render(MetaKey::Tables, "{{ schema_name }}", Family::MySql, 0, false, &args()).is_err());
        assert!(render(MetaKey::Tables, "{{ bind(\"owner\") }}", Family::MySql, 0, false, &args()).is_err());
    }

    #[test]
    fn missing_arguments_bind_null() {
        let q =
            render(MetaKey::Tables, "{{ bind(\"table\") }}", Family::MySql, 0, false, &MetaArgs::default())
                .unwrap();
        assert_eq!(q.params, vec![Value::Null]);
    }
}
