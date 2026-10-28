use std::collections::{BTreeMap, BTreeSet};

use dblens_core::schema::{ForeignKey, Index, IndexColumn, PrimaryKey, Table};

use super::*;
use crate::Variant;
use crate::design::fixtures::{col, table};

fn side<'a>(schema: &'a str, tables: &'a [Table]) -> CompareSide<'a> {
    CompareSide { schema, tables }
}

fn pg() -> Dialect {
    Dialect::new(Variant::Postgres)
}

fn kinds(t: &TableDiff) -> Vec<String> {
    t.items
        .iter()
        .map(|i| match &i.change {
            SchemaChange::DropTable { name } => format!("drop_table {name}"),
            SchemaChange::RenameTable { from, to } => format!("rename_table {from} {to}"),
            SchemaChange::Table { change } => {
                serde_json::to_value(change).unwrap()["kind"].as_str().unwrap().to_string()
            }
        })
        .collect()
}

fn customers(name: &str) -> Table {
    Table {
        name: name.into(),
        columns: vec![col("id", "integer"), col("name", "varchar(50)"), col("phone", "varchar(20)")],
        primary_key: Some(PrimaryKey { name: None, columns: vec!["id".into()] }),
        ..Default::default()
    }
}

#[test]
fn pairs_tables_and_suggests_renames() {
    let pg = pg();
    let mut src_orders = table(["integer", "varchar(20)", "numeric(10,2)"]);
    src_orders.columns[2].name = "total".into();
    src_orders.checks[0].expression = "total >= 0".into();
    src_orders.indexes[0].columns[0].name = "total".into();
    let source = vec![src_orders, customers("clients"), customers("only_source")];
    let target = vec![table(["integer", "varchar(20)", "numeric(10,2)"]), customers("customers")];
    let opts = CompareOptions { ignore_generated_names: false, ..Default::default() };
    let d = compare_schemas(&pg, &side("s", &source), &side("t", &target), &CompareRenames::default(), &opts)
        .unwrap();
    // 没采用改名：clients 新增、customers 删除，只给出建议
    let status: Vec<_> = d.tables.iter().map(|t| (t.source.clone(), t.target.clone(), t.status)).collect();
    assert_eq!(
        status,
        [
            (Some("orders".into()), Some("orders".into()), TableStatus::Changed),
            (Some("clients".into()), None, TableStatus::Added),
            (Some("only_source".into()), None, TableStatus::Added),
            (None, Some("customers".into()), TableStatus::Removed),
        ]
    );
    assert_eq!(
        d.suggestions,
        [
            RenameSuggestion { table: None, source: "clients".into(), target: "customers".into() },
            RenameSuggestion {
                table: Some("orders".into()),
                source: "total".into(),
                target: "amount".into()
            },
        ]
    );
    // 采用改名后重新比对
    let renames = CompareRenames {
        tables: [("clients".into(), "customers".into())].into(),
        columns: [("orders".into(), [("total".into(), "amount".into())].into())].into(),
    };
    let d = compare_schemas(&pg, &side("s", &source), &side("t", &target), &renames, &opts).unwrap();
    let clients = d.tables.iter().find(|t| t.source.as_deref() == Some("clients")).unwrap();
    assert_eq!(
        (clients.status, kinds(clients)),
        (TableStatus::Renamed, vec!["rename_table customers clients".to_string()])
    );
    let orders = &d.tables[0];
    assert_eq!(kinds(orders), ["drop_check", "rename_column", "add_check"]);
    assert!(d.suggestions.is_empty(), "{:?}", d.suggestions);
    // 编号唯一、连续
    let ids: Vec<u32> = d.tables.iter().flat_map(|t| t.items.iter().map(|i| i.id)).collect();
    assert_eq!(ids, (1..=ids.len() as u32).collect::<Vec<_>>());
    assert!(matches!(d.item(1), Some((t, _)) if t.source.as_deref() == Some("orders")));
}

#[test]
fn options_and_equivalent_spellings() {
    let pg = pg();
    let base = table(["integer", "varchar(20)", "numeric(10,2)"]);
    let mut target = base.clone();
    // 库读回的写法：类型同义、默认值带类型转换、系统生成的名字、外键带本 schema
    target.columns[0].data_type.native = "int4".into();
    target.columns[1].default = Some("'x'::character varying".into());
    target.primary_key.as_mut().unwrap().name = Some("orders_pkey".into());
    target.uniques[0].name = Some("orders_code_key".into());
    target.foreign_keys[0].ref_schema = Some("live".into());
    target.checks[0].expression = "(amount >= (0)::numeric)".into();
    let mut source = base.clone();
    source.columns[1].default = Some("'x'".into());
    source.primary_key.as_mut().unwrap().name = None;
    source.uniques[0].name = None;
    source.checks[0].expression = "(amount >= (0)::numeric)".into();
    source.comment = Some("another comment".into());
    source.columns.swap(1, 2);
    let (s, t) = (vec![source], vec![target]);
    let all = CompareOptions { ignore_comments: true, ignore_column_order: true, ..Default::default() };
    let d = compare_schemas(&pg, &side("model", &s), &side("live", &t), &CompareRenames::default(), &all)
        .unwrap();
    assert_eq!(d.tables[0].status, TableStatus::Same, "{:?}", kinds(&d.tables[0]));
    let none = CompareOptions { ignore_generated_names: false, ..Default::default() };
    let d = compare_schemas(&pg, &side("model", &s), &side("live", &t), &CompareRenames::default(), &none)
        .unwrap();
    // 主键没有名字时与任何名字都算相同（M3 的规则），只有唯一约束的名字不同
    let k = kinds(&d.tables[0]);
    for want in ["set_table_comment", "move_column", "drop_unique", "add_unique"] {
        assert!(k.iter().any(|x| x == want), "{want}: {k:?}");
    }
}

#[test]
fn generated_names_by_family() {
    use normalize::is_generated;
    let d = |v| Dialect::new(v);
    assert!(is_generated(&d(Variant::Oracle), "T", "SYS_C0012345"));
    assert!(!is_generated(&d(Variant::Oracle), "T", "PK_T"));
    assert!(is_generated(&d(Variant::MySql), "orders", "PRIMARY"));
    assert!(is_generated(&d(Variant::MySql), "orders", "orders_ibfk_2"));
    assert!(is_generated(&d(Variant::Postgres), "orders", "orders_code_key"));
    assert!(!is_generated(&d(Variant::Postgres), "orders", "uk_code"));
    assert!(is_generated(&d(Variant::Dameng), "T", "CONS134218790"));
}

/// 把比对出的变更应用到目标上，再比对应当没有差异（`apply_changes` 与 `diff_table` 互逆）。
#[test]
fn apply_is_inverse_of_diff() {
    let pg = pg();
    let target = table(["integer", "varchar(20)", "numeric(10,2)"]);
    for source in [
        crate::design::fixtures::everything(&target, "varchar(40)"),
        crate::design::fixtures::alter_only(&target, "numeric(12,2)"),
        crate::design::fixtures::move_first(&target),
    ]
    .into_iter()
    .map(|diff| apply_changes(&target, &diff.changes).unwrap())
    {
        let applied = apply_changes(
            &target,
            &crate::design::diff_table(Some(&target), &source, &Default::default()).unwrap().changes,
        );
        let again = crate::design::diff_table(Some(&applied.unwrap()), &source, &Default::default()).unwrap();
        assert!(again.changes.is_empty(), "{:?}", again.changes);
        let (s, t) = (vec![source.clone()], vec![target.clone()]);
        let d = compare_schemas(
            &pg,
            &side("a", &s),
            &side("a", &t),
            &CompareRenames::default(),
            &CompareOptions::default(),
        )
        .unwrap();
        let changes: Vec<_> = d.tables[0]
            .items
            .iter()
            .filter_map(|i| {
                if let SchemaChange::Table { change } = &i.change {
                    Some(change.as_ref().clone())
                } else {
                    None
                }
            })
            .collect();
        let written = apply_changes(&target, &changes).unwrap();
        let d2 = compare_schemas(
            &pg,
            &side("a", &s),
            &side("a", &[written]),
            &CompareRenames::default(),
            &CompareOptions::default(),
        )
        .unwrap();
        assert_eq!(d2.changed(), 0, "{:?}", d2.tables);
    }
    // 引用的列不存在
    let bad = apply_changes(&target, &[crate::design::TableChange::DropColumn { name: "nope".into() }]);
    assert!(bad.is_err());
}

fn fk(name: &str, col: &str, table: &str) -> ForeignKey {
    ForeignKey {
        name: Some(name.into()),
        columns: vec![col.into()],
        ref_schema: None,
        ref_table: table.into(),
        ref_columns: vec!["id".into()],
        on_delete: None,
        on_update: None,
    }
}

/// 一组覆盖各段的改动：orders 换外键、改列；items 新建并带外键；old 删除；cust → customers 改名；
/// moved 调整列顺序（PG 要重建）。
fn sync_case() -> (Vec<Table>, Vec<Table>) {
    let mut orders_t = customers("orders");
    orders_t.foreign_keys = vec![fk("fk_old", "name", "cust")];
    let mut orders_s = customers("orders");
    orders_s.columns[1].data_type.native = "varchar(80)".into();
    orders_s.foreign_keys = vec![fk("fk_new", "phone", "customers")];
    let mut items = customers("items");
    items.foreign_keys = vec![fk("fk_items_orders", "id", "orders")];
    let moved_t = customers("moved");
    let mut moved_s = customers("moved");
    moved_s.columns.swap(1, 2);
    moved_s.indexes = vec![Index {
        name: "ix_moved_phone".into(),
        columns: vec![IndexColumn { name: "phone".into(), descending: false }],
        unique: false,
        method: None,
    }];
    let source = vec![orders_s, items, customers("customers"), moved_s];
    let target = vec![orders_t, customers("old"), customers("cust"), moved_t];
    (source, target)
}

#[test]
fn sync_script_orders_phases_and_rebuilds() {
    let pg = pg();
    let (source, target) = sync_case();
    let renames =
        CompareRenames { tables: [("customers".into(), "cust".into())].into(), ..Default::default() };
    let d = compare_schemas(
        &pg,
        &side("app", &source),
        &side("app", &target),
        &renames,
        &CompareOptions::default(),
    )
    .unwrap();
    let all: BTreeSet<u32> = d.tables.iter().flat_map(|t| t.items.iter().map(|i| i.id)).collect();
    assert_eq!(sync_rebuild_tables(&pg, &d, &all), ["moved"]);
    let script = sync_script(&pg, "app", &d, &all, &target, &BTreeMap::new()).unwrap();
    assert!(script.transactional && script.refused.is_empty());
    let text: Vec<String> = script
        .steps
        .iter()
        .map(|s| match s {
            SyncStep::Sql { sql, items } => format!("{items:?} {sql}"),
            SyncStep::Rebuild { table, items, rebuild } => {
                format!("{items:?} rebuild {table} ({} steps)", rebuild.steps.len())
            }
        })
        .collect();
    insta::assert_snapshot!("sync_pg", text.join("\n"));
    // 只勾选一部分：没勾选的不出现
    let only: BTreeSet<u32> = d.tables[0].items.iter().map(|i| i.id).collect();
    let part = sync_script(&pg, "app", &d, &only, &target, &BTreeMap::new()).unwrap();
    assert!(
        part.steps.iter().all(|s| matches!(s, SyncStep::Sql { sql, .. } if sql.contains("orders"))),
        "{:?}",
        part.steps
    );
}

#[test]
fn sync_script_mysql_keeps_one_alter_per_table_rules() {
    let my = Dialect::new(Variant::MySql).with_version(Some(80040));
    let target = table(["BIGINT", "VARCHAR(20)", "DECIMAL(10,2)"]);
    let source =
        apply_changes(&target, &crate::design::fixtures::everything(&target, "VARCHAR(50)").changes).unwrap();
    let renames = CompareRenames {
        columns: [("orders".into(), [("order_code".into(), "code".into())].into())].into(),
        ..Default::default()
    };
    let (s, t) = (vec![source], vec![target]);
    let d = compare_schemas(&my, &side("shop", &s), &side("shop", &t), &renames, &CompareOptions::default())
        .unwrap();
    let all: BTreeSet<u32> = d.tables.iter().flat_map(|t| t.items.iter().map(|i| i.id)).collect();
    let script = sync_script(&my, "shop", &d, &all, &t, &BTreeMap::new()).unwrap();
    let text: Vec<String> = script
        .steps
        .iter()
        .map(
            |s| if let SyncStep::Sql { sql, items } = s { format!("{items:?} {sql}") } else { String::new() },
        )
        .collect();
    insta::assert_snapshot!("sync_mysql", text.join("\n"));
}
