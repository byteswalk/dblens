use std::collections::BTreeMap;

use dblens_core::schema::{
    CheckConstraint, Column, ForeignKey, Index, IndexColumn, PrimaryKey, Sequence, Table, UniqueConstraint,
};
use dblens_core::{ColumnType, Lang, LogicalType};

use super::*;
use crate::Variant;

fn col(name: &str, native: &str) -> Column {
    Column {
        name: name.into(),
        ordinal: 0,
        data_type: ColumnType { native: native.into(), logical: LogicalType::Other },
        nullable: true,
        default: None,
        comment: None,
        auto_increment: false,
        generated: None,
    }
}

fn idx(name: &str, cols: &[&str]) -> Index {
    Index {
        name: name.into(),
        columns: cols.iter().map(|c| IndexColumn { name: c.to_string(), descending: false }).collect(),
        unique: false,
        method: None,
    }
}

/// PG 中的源结构：serial 主键、各种默认值、数组列、检查约束、表达式索引、外键（一个引用范围外的表）、生成列、序列。
fn pg_source() -> (Vec<Table>, Vec<Sequence>) {
    let mut id = col("id", "bigint");
    id.nullable = false;
    id.auto_increment = true;
    id.default = Some("nextval('customers_id_seq'::regclass)".into());
    let mut name = col("name", "character varying(50)");
    name.nullable = false;
    name.default = Some("'n/a'::character varying".into());
    name.comment = Some("Customer name".into());
    let mut active = col("active", "boolean");
    active.default = Some("true".into());
    let mut created = col("created", "timestamp(6) without time zone");
    created.default = Some("now()".into());
    let mut balance = col("balance", "numeric(12,2)");
    balance.default = Some("0".into());
    let customers = Table {
        name: "customers".into(),
        comment: Some("Customers".into()),
        columns: vec![
            id,
            name,
            col("email", "character varying(100)"),
            active,
            created,
            balance,
            col("tags", "text[]"),
            col("note", "text"),
        ],
        primary_key: Some(PrimaryKey { name: Some("customers_pkey".into()), columns: vec!["id".into()] }),
        uniques: vec![UniqueConstraint {
            name: Some("customers_email_key".into()),
            columns: vec!["email".into()],
        }],
        checks: vec![CheckConstraint {
            name: Some("ck_balance".into()),
            expression: "(balance >= (0)::numeric)".into(),
        }],
        indexes: vec![
            idx("ix_customers_name", &["name"]),
            Index { method: Some("gin".into()), ..idx("ix_customers_note", &["note"]) },
            idx("ix_customers_lower", &["lower((email)::text)"]),
        ],
        ..Default::default()
    };
    let mut oid = col("id", "integer");
    oid.nullable = false;
    oid.default = Some("nextval('order_seq'::regclass)".into());
    let mut status = col("status", "character varying(10)");
    status.default = Some("'new'::character varying".into());
    let mut placed = col("placed", "date");
    placed.default = Some("CURRENT_DATE".into());
    let mut code = col("code", "uuid");
    code.default = Some("gen_random_uuid()".into());
    let mut double = col("double_amount", "numeric(12,2)");
    double.generated = Some("(amount * (2)::numeric)".into());
    let orders = Table {
        name: "orders".into(),
        columns: vec![
            oid,
            col("customer_id", "bigint"),
            col("product_id", "integer"),
            col("amount", "numeric(10,2)"),
            double,
            status,
            placed,
            code,
            col("pos", "point"),
        ],
        primary_key: Some(PrimaryKey { name: Some("orders_pkey".into()), columns: vec!["id".into()] }),
        foreign_keys: vec![
            ForeignKey {
                name: Some("fk_orders_customer".into()),
                columns: vec!["customer_id".into()],
                ref_schema: None,
                ref_table: "customers".into(),
                ref_columns: vec!["id".into()],
                on_delete: Some("CASCADE".into()),
                on_update: Some("CASCADE".into()),
            },
            ForeignKey {
                name: Some("fk_orders_product".into()),
                columns: vec!["product_id".into()],
                ref_schema: None,
                ref_table: "products".into(),
                ref_columns: vec!["id".into()],
                on_delete: None,
                on_update: None,
            },
        ],
        checks: vec![CheckConstraint {
            name: Some("ck_status".into()),
            expression: "((status)::text = ANY ((ARRAY['new'::character varying, 'paid'::character varying])::text[]))"
                .into(),
        }],
        ..Default::default()
    };
    let seq = |name: &str, max: &str, cache| Sequence {
        name: name.into(),
        start: Some("1".into()),
        increment: Some("1".into()),
        min_value: Some("1".into()),
        max_value: Some(max.into()),
        cycle: false,
        cache: Some(cache),
    };
    (
        vec![customers, orders],
        vec![seq("customers_id_seq", "9223372036854775807", 1), seq("order_seq", "9223372036854775807", 20)],
    )
}

fn input<'a>(
    src: Variant,
    tgt: Variant,
    tables: &'a [Table],
    sequences: &'a [Sequence],
) -> MigrationInput<'a> {
    MigrationInput {
        source: Dialect::new(src),
        target: Dialect::new(tgt),
        tables,
        sequences,
        target_tables: &[],
        target_sequences: &[],
    }
}

fn render(draft: &MigrationDraft, tgt: Variant) -> String {
    let mut out = String::new();
    for n in &draft.notes {
        out += &format!(
            "{:?} {}{}{}: {}\n",
            n.level,
            n.object,
            n.column.as_ref().map(|c| format!(".{c}")).unwrap_or_default(),
            n.item.as_ref().map(|c| format!(" [{c}]")).unwrap_or_default(),
            n.message.render(Lang::En)
        );
    }
    let script = migration_script(&Dialect::new(tgt), "app", draft, &BTreeMap::new(), &[]).unwrap();
    out + "\n" + &script_text(tgt.family(), &script)
}

/// 不认识的类型都确认按文本迁移。
fn accept_unknown(settings: &mut MigrationSettings) {
    for (t, c) in [("customers", "tags"), ("orders", "pos")] {
        settings
            .tables
            .entry(t.into())
            .or_default()
            .columns
            .insert(c.into(), ColumnOverride { accept: true, ..Default::default() });
    }
}

#[test]
fn pg_to_each_family() {
    let (tables, sequences) = pg_source();
    let mut settings = MigrationSettings::default();
    accept_unknown(&mut settings);
    for (tgt, name) in [
        (Variant::MySql, "pg_to_mysql"),
        (Variant::Oracle, "pg_to_oracle"),
        (Variant::Dameng, "pg_to_dameng"),
        (Variant::OpenGauss, "pg_to_opengauss"),
        (Variant::SqlServer, "pg_to_sqlserver"),
    ] {
        let draft = migrate(&input(Variant::Postgres, tgt, &tables, &sequences), &settings);
        assert!(!draft.blocked(), "{name}");
        insta::assert_snapshot!(name, render(&draft, tgt));
    }
}

#[test]
fn unknown_types_block_until_accepted() {
    let (tables, sequences) = pg_source();
    let draft = migrate(
        &input(Variant::Postgres, Variant::MySql, &tables, &sequences),
        &MigrationSettings::default(),
    );
    let blocked: Vec<_> = draft
        .notes
        .iter()
        .filter(|n| n.level == NoteLevel::Unsupported)
        .map(|n| format!("{}.{}", n.object, n.column.as_deref().unwrap_or("")))
        .collect();
    assert_eq!(blocked, ["customers.tags", "orders.pos"]);
    assert!(draft.blocked());
}

/// Oracle 源：DATE、NUMBER、CLOB、标识列、序列默认值、系统序列。
#[test]
fn oracle_to_pg_and_mysql() {
    let mut id = col("ID", "NUMBER(10)");
    id.nullable = false;
    id.auto_increment = true;
    let mut created = col("CREATED", "DATE");
    created.default = Some("SYSDATE".into());
    let mut no = col("NO", "NUMBER(19)");
    no.default = Some("\"APP\".\"DOC_SEQ\".\"NEXTVAL\"".into());
    let mut flag = col("FLAG", "NUMBER(1)");
    flag.default = Some("0".into());
    let docs = Table {
        name: "DOCS".into(),
        columns: vec![
            id,
            no,
            col("TITLE", "VARCHAR2(200 CHAR)"),
            col("BODY", "CLOB"),
            created,
            flag,
            col("PRICE", "NUMBER"),
        ],
        primary_key: Some(PrimaryKey { name: Some("PK_DOCS".into()), columns: vec!["ID".into()] }),
        indexes: vec![idx("IX_DOCS_BODY", &["BODY"]), idx("IX_DOCS_TITLE", &["TITLE"])],
        checks: vec![CheckConstraint {
            name: Some("CK_FLAG".into()),
            expression: "\"FLAG\" IN (0,1)".into(),
        }],
        ..Default::default()
    };
    let seqs = vec![
        Sequence {
            name: "DOC_SEQ".into(),
            start: Some("100".into()),
            increment: Some("1".into()),
            min_value: Some("1".into()),
            max_value: Some("9999999999999999999999999999".into()),
            cycle: false,
            cache: Some(20),
        },
        Sequence {
            name: "ISEQ$$_12345".into(),
            start: Some("1".into()),
            increment: Some("1".into()),
            min_value: None,
            max_value: None,
            cycle: false,
            cache: None,
        },
    ];
    let tables = [docs];
    let mut settings = MigrationSettings::default();
    settings.type_rules.push(TypeRule { source: "NUMBER(1)".into(), target: "BOOLEAN".into() });
    let draft = migrate(&input(Variant::Oracle, Variant::Postgres, &tables, &seqs), &settings);
    insta::assert_snapshot!("oracle_to_pg", render(&draft, Variant::Postgres));
    let draft =
        migrate(&input(Variant::Oracle, Variant::MySql, &tables, &seqs), &MigrationSettings::default());
    insta::assert_snapshot!("oracle_to_mysql", render(&draft, Variant::MySql));
}

#[test]
fn overrides_names_and_skips() {
    let (tables, sequences) = pg_source();
    let mut settings = MigrationSettings::default();
    settings.options.name_case = Some(NameCase::Keep);
    settings.options.checks = false;
    settings.options.comments = false;
    settings.sequences.insert("order_seq".into(), SequenceOverride { skip: true, ..Default::default() });
    let ov = settings.tables.entry("customers".into()).or_default();
    ov.name = Some("Client".into());
    ov.columns.insert("tags".into(), ColumnOverride { skip: true, ..Default::default() });
    ov.columns.insert(
        "note".into(),
        ColumnOverride { data_type: Some("VARCHAR2(4000)".into()), ..Default::default() },
    );
    ov.columns
        .insert("created".into(), ColumnOverride { default: Some(String::new()), ..Default::default() });
    settings.tables.insert("orders".into(), TableOverride { skip: true, ..Default::default() });
    let draft = migrate(&input(Variant::Postgres, Variant::Oracle, &tables, &sequences), &settings);
    assert!(!draft.blocked());
    assert_eq!(draft.tables.len(), 1);
    assert!(draft.sequences.is_empty());
    let t = &draft.tables[0].target;
    assert_eq!(t.name, "Client");
    assert!(t.comment.is_none() && t.checks.is_empty());
    assert_eq!(
        t.columns.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(),
        ["id", "name", "email", "active", "created", "balance", "note"]
    );
    assert_eq!(t.columns[6].data_type.native, "VARCHAR2(4000)");
    assert_eq!(t.columns[4].default, None);
    assert_eq!(draft.tables[0].columns[6], ColumnMapping { source: "tags".into(), target: None });
}

#[test]
fn name_conflicts_and_length() {
    let t = |name: &str| Table {
        name: name.into(),
        columns: vec![col("a", "int"), col("A", "int")],
        ..Default::default()
    };
    let tables = [t("Item"), t("ITEM"), t(&"x".repeat(70))];
    let draft =
        migrate(&input(Variant::MySql, Variant::Postgres, &tables, &[]), &MigrationSettings::default());
    let msgs: Vec<String> = draft
        .notes
        .iter()
        .filter(|n| n.level == NoteLevel::Unsupported)
        .map(|n| {
            format!(
                "{}{} {}",
                n.object,
                n.column.as_ref().map(|c| format!(".{c}")).unwrap_or_default(),
                n.message.render(Lang::En)
            )
        })
        .collect();
    assert_eq!(msgs.len(), 5, "{msgs:#?}");
    assert!(msgs[0].starts_with("Item.A Name a conflicts with a"), "{msgs:#?}");
    assert!(msgs.iter().any(|m| m.starts_with("ITEM Name item conflicts with Item")), "{msgs:#?}");
    assert!(msgs.iter().any(|m| m.contains("exceeds")), "{msgs:#?}");
}

#[test]
fn existing_tables_follow_the_chosen_action() {
    let (tables, sequences) = pg_source();
    let mut settings = MigrationSettings::default();
    accept_unknown(&mut settings);
    let existing = Table {
        name: "CUSTOMERS".into(),
        columns: vec![col("ID", "NUMBER(19)")],
        indexes: vec![idx("IX_OLD_NAME", &["NAME"])],
        uniques: vec![UniqueConstraint { name: Some("UK_OLD".into()), columns: vec!["EMAIL".into()] }],
        ..Default::default()
    };
    let target_tables = [existing];
    let inp = MigrationInput {
        target_tables: &target_tables,
        target_sequences: &["ORDER_SEQ".to_string()],
        ..input(Variant::Postgres, Variant::Oracle, &tables, &sequences)
    };
    let draft = migrate(&inp, &settings);
    assert!(draft.tables[0].exists && !draft.tables[1].exists);
    assert!(draft.sequences[0].exists);
    let d = Dialect::new(Variant::Oracle);
    let kinds = |action: Option<ExistingAction>| {
        let existing: BTreeMap<_, _> = action.into_iter().map(|a| ("customers".to_string(), a)).collect();
        migration_script(&d, "APP", &draft, &existing, &target_tables)
            .unwrap()
            .iter()
            .filter(|s| s.object == "customers")
            .map(|s| format!("{:?}", s.kind))
            .collect::<Vec<_>>()
            .join(" ")
    };
    assert_eq!(kinds(None), "");
    assert_eq!(kinds(Some(ExistingAction::Skip)), "");
    assert_eq!(
        kinds(Some(ExistingAction::Recreate)),
        "DropTable CreateTable Comment Comment Index Unique Check"
    );
    assert_eq!(kinds(Some(ExistingAction::MissingOnly)), "Check");
    // 追加数据、清空后导入不动结构
    assert_eq!(kinds(Some(ExistingAction::Append)), "");
    assert_eq!(kinds(Some(ExistingAction::Replace)), "");
    assert!(ExistingAction::Replace.copies_data() && !ExistingAction::MissingOnly.copies_data());
    // 外键要求两张表都建好
    let script = migration_script(&d, "APP", &draft, &BTreeMap::new(), &target_tables).unwrap();
    let fk = script.iter().find(|s| s.kind == StatementKind::ForeignKey).unwrap();
    assert_eq!(fk.requires, ["orders", "customers"]);
}
