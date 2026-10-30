//! 元数据查询 SQL 的快照测试：每个变体、每种查询各渲染一次（带上全部可选参数），
//! 改动模板时在快照差异里审查生成的 SQL。新增或改动快照后用 `INSTA_UPDATE=always cargo test` 重新生成并审查。

use dblens_dialect::meta::{MetaArgs, MetaKey};
use dblens_dialect::{Dialect, Family, Variant};

/// 有自己的模板覆盖文件的变体（PolarDB、GaussDB、HighGo 沿用家族模板）。
const VARIANTS: [Variant; 11] = [
    Variant::MySql,
    Variant::MariaDb,
    Variant::TiDb,
    Variant::OceanBase,
    Variant::Postgres,
    Variant::Kingbase,
    Variant::OpenGauss,
    Variant::Oracle,
    Variant::Dameng,
    Variant::SqlServer,
    Variant::Gbase8s,
];

#[test]
fn rendered_metadata_queries() {
    let full = MetaArgs {
        catalog: None,
        schema: Some("HR".into()),
        table: Some("EMP".into()),
        name: Some("EMP".into()),
        kind: Some("FUNCTION".into()),
        filter: Some("员工".into()),
    };
    for v in VARIANTS {
        let d = Dialect::new(v);
        let mut out = String::new();
        for key in MetaKey::ALL {
            let Some(q) = d.meta_query(key, &full).unwrap() else { continue };
            out.push_str(&format!("-- [{}] {:?}\n{}\n\n", key.as_str(), q.params, q.sql));
        }
        insta::assert_snapshot!(format!("{v:?}").to_lowercase(), out);
    }
}

/// 每个家族的模板都必须覆盖全部通用查询；只有个别家族才有的查询列在这里。
#[test]
fn every_family_covers_the_common_keys() {
    for v in VARIANTS {
        let d = Dialect::new(v);
        for key in MetaKey::ALL {
            let optional = match key {
                // 只有 SQL Server、GBase 8s 有数据库这一层
                MetaKey::Catalogs => !matches!(v.family(), Family::SqlServer | Family::Informix),
                MetaKey::MaterializedViews
                | MetaKey::Synonyms
                | MetaKey::Sequences
                | MetaKey::VariantProbe => true,
                MetaKey::DependentViews | MetaKey::Grants => {
                    matches!(v.family(), Family::MySql | Family::Informix)
                }
                // 表设计器在 M8b
                MetaKey::ReferencingKeys => v.family() == Family::Informix,
                MetaKey::TriggerDdl => v != Variant::Oracle,
                // OceanBase 没有 information_schema.KEYWORDS；SQL Server、GBase 8s 没有可查的保留字表（用 ident.rs 的清单）
                MetaKey::ReservedWords => {
                    matches!(v, Variant::MariaDb | Variant::OceanBase | Variant::SqlServer | Variant::Gbase8s)
                }
                _ => false,
            };
            assert!(optional || d.has_meta(key), "{v:?} 缺少 {key:?} 模板");
        }
    }
}

/// 旧版本分支（实库只有新版本，测不到）至少要能渲染：模板语法、绑定参数都正确。
#[test]
fn legacy_version_branches_render() {
    let full = MetaArgs {
        catalog: None,
        schema: Some("HR".into()),
        table: Some("EMP".into()),
        name: Some("EMP".into()),
        kind: Some("FUNCTION".into()),
        filter: Some("x".into()),
    };
    for (v, old) in [
        (Variant::MySql, 50744),
        (Variant::Postgres, 100000),
        (Variant::Kingbase, 120100),
        (Variant::OpenGauss, 90204),
        (Variant::Oracle, 110204),
        (Variant::Dameng, 80000),
    ] {
        let d = Dialect::new(v).with_version(Some(old));
        for key in MetaKey::ALL {
            if let Some(q) = d.meta_query(key, &full).unwrap() {
                assert!(!q.sql.contains("{{") && !q.sql.contains("{%"), "{v:?} {key:?}");
            }
        }
    }
}
