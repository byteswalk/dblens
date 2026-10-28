//! 重建表的脚本（M3 设计文档第 5 节）：库不支持的改动（或用户手动选择）时，建新表、复制数据、核对行数、
//! 删除旧表并把新表改回原名，再重建约束、索引、触发器、指向本表的外键、自增接续、授权，重新编译失效的视图。
//!
//! 执行约定：先把当前 schema 切换到表所在的 schema（MySQL、Oracle 的触发器 DDL 里的表名不带 schema）；
//! `drop_at` 之前失败或取消时执行 `cleanup` 删除新表，旧表不受影响。

use dblens_core::schema::{ForeignKey, Table};
use dblens_core::{Msg, Result, Value, msg};
use serde::Serialize;

use super::diff::same_type;
use super::{Diff, Renames, TableChange as C};
use crate::meta::{DependentView, Grant, ReferencingKey, TableStorage};
use crate::{Dialect, Family};

/// 生成重建脚本需要的依赖信息，由服务层读取。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RebuildContext {
    pub referencing: Vec<ReferencingKey>,
    pub views: Vec<DependentView>,
    pub grants: Vec<Grant>,
    pub storage: TableStorage,
    /// 本表触发器的完整 DDL。
    pub triggers: Vec<String>,
}

/// 脚本的一步。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RebuildStep {
    Sql {
        sql: String,
    },
    /// 核对两张表的行数，不一致即停止（由程序执行）。
    CountCheck {
        source: String,
        target: String,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Rebuild {
    pub steps: Vec<RebuildStep>,
    /// 删除旧表的那一步：从这里起不能取消，失败时也不再删除新表。
    pub drop_at: usize,
    /// `drop_at` 之前失败或取消时执行：删除新表。
    pub cleanup: String,
    pub warnings: Vec<Msg>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RebuildOutcome {
    Script(Rebuild),
    /// 不能重建及原因。
    Refused {
        reason: Msg,
    },
}

/// 一个字符串的 32 位 FNV-1a 哈希（只用来让截断后的临时表名不重复）。
fn short_hash(s: &str) -> String {
    let mut h: u32 = 0x811c_9dc5;
    for b in s.bytes() {
        h ^= u32::from(b);
        h = h.wrapping_mul(0x0100_0193);
    }
    format!("{h:08x}")
}

impl Dialect {
    /// 标识符的最大字节数。
    fn max_ident(&self) -> usize {
        match self.family() {
            Family::MySql => 64,
            Family::Postgres => 63,
            Family::Oracle if self.version().unwrap_or(u32::MAX) < 120200 => 30,
            Family::Oracle | Family::Dameng => 128,
        }
    }

    /// 临时表名 `<表名>_dblens_new`；超长时截断表名并加短哈希。
    pub(crate) fn temp_table_name(&self, table: &str) -> String {
        const SUFFIX: &str = "_dblens_new";
        let max = self.max_ident();
        let full = format!("{table}{SUFFIX}");
        if full.len() <= max {
            return full;
        }
        let keep = max - SUFFIX.len() - 9;
        let mut end = keep.min(table.len());
        while !table.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}_{}{SUFFIX}", &table[..end], short_hash(table))
    }

    /// 建表语句末尾的存储属性：MySQL 的引擎、字符集、排序规则，其他库的表空间。
    fn storage_clause(&self, s: &TableStorage) -> String {
        match self.family() {
            Family::MySql => {
                let mut out = String::new();
                if let Some(e) = &s.engine {
                    out += &format!(" ENGINE = {e}");
                }
                if let Some(c) = &s.charset {
                    out += &format!(" DEFAULT CHARSET = {c}");
                }
                if let Some(c) = &s.collation {
                    out += &format!(" COLLATE = {c}");
                }
                out
            }
            _ => s.tablespace.as_ref().map(|t| format!(" TABLESPACE {}", self.ident(t))).unwrap_or_default(),
        }
    }

    /// 生成重建脚本。`draft` 应已校验过；表名不变（已有表不能改名）。
    pub fn rebuild(
        &self,
        schema: &str,
        original: &Table,
        draft: &Table,
        renames: &Renames,
        ctx: &RebuildContext,
    ) -> Result<RebuildOutcome> {
        let refuse = |reason| Ok(RebuildOutcome::Refused { reason });
        if original.partition.is_some() {
            return refuse(msg!("design.rebuild.partitioned"));
        }
        let view_list =
            || ctx.views.iter().map(|v| format!("{}.{}", v.schema, v.name)).collect::<Vec<_>>().join(", ");
        if self.family() == Family::Postgres && !ctx.views.is_empty() {
            return refuse(msg!("design.rebuild.dependent_views", views = view_list()));
        }
        // 草稿列 → 原列名（保留下来的列）
        let old_of = |name: &str| -> Option<String> {
            let old = renames.columns.get(name).map_or(name, String::as_str);
            original.columns.iter().any(|c| c.name == old).then(|| old.to_string())
        };
        let new_of = |old: &str| -> Option<String> {
            draft.columns.iter().find(|c| old_of(&c.name).as_deref() == Some(old)).map(|c| c.name.clone())
        };
        // 其他表的外键引用的列都要还在
        let mut referencing = Vec::new();
        for rk in &ctx.referencing {
            let mut key = rk.key.clone();
            for c in &mut key.ref_columns {
                match new_of(c) {
                    Some(n) => *c = n,
                    None => {
                        return refuse(msg!(
                            "design.rebuild.referenced_column",
                            table = rk.table,
                            column = c
                        ));
                    }
                }
            }
            referencing.push((rk, key));
        }

        let name = &draft.name;
        let q = self.qualified(Some(schema), name);
        let tmp = self.temp_table_name(name);
        let q_tmp = self.qualified(Some(schema), &tmp);
        let mut steps = Vec::new();
        let mut sql = |s: String| steps.push(RebuildStep::Sql { sql: s });

        // 1. 新表：只有列（MySQL 带主键：自增列必须是键；主键名在 MySQL 里不会冲突）
        let mysql = self.family() == Family::MySql;
        let bare = Table {
            name: tmp.clone(),
            primary_key: if mysql { draft.primary_key.clone() } else { None },
            uniques: vec![],
            foreign_keys: vec![],
            checks: vec![],
            indexes: vec![],
            triggers: vec![],
            partition: None,
            ..draft.clone()
        };
        let create = Diff { changes: vec![C::CreateTable { table: bare }], appended: true };
        for (i, s) in self.design_sql(schema, &tmp, &create)?.into_iter().enumerate() {
            sql(if i == 0 { s.sql + &self.storage_clause(&ctx.storage) } else { s.sql });
        }

        // 2. 复制数据：保留的列从旧列取值，类型变了的套 CAST（MySQL 靠写入时的隐式转换）；生成列不写
        let mut targets = Vec::new();
        let mut sources = Vec::new();
        for c in draft.columns.iter().filter(|c| c.generated.is_none()) {
            let Some(old) = old_of(&c.name) else { continue };
            let o = original.columns.iter().find(|x| x.name == old).expect("old_of 只返回存在的列");
            let src = self.ident(&old);
            targets.push(self.ident(&c.name));
            sources.push(if !mysql && !same_type(&o.data_type.native, &c.data_type.native) {
                format!("CAST({src} AS {})", c.data_type.native)
            } else {
                src
            });
        }
        let dameng_identity =
            self.family() == Family::Dameng && draft.columns.iter().any(|c| c.auto_increment);
        if dameng_identity {
            sql(format!("SET IDENTITY_INSERT {q_tmp} ON"));
        }
        sql(format!("INSERT INTO {q_tmp} ({}) SELECT {} FROM {q}", targets.join(", "), sources.join(", ")));
        if dameng_identity {
            sql(format!("SET IDENTITY_INSERT {q_tmp} OFF"));
        }

        // 3. 核对行数
        steps.push(RebuildStep::CountCheck { source: q.clone(), target: q_tmp.clone() });
        let mut sql = |s: String| steps.push(RebuildStep::Sql { sql: s });

        // 4. 删除其他表指向本表的外键
        for (rk, _) in &referencing {
            let other = self.qualified(Some(&rk.schema), &rk.table);
            let fk = self.ident(self.required_name(&rk.key.name)?);
            sql(if mysql {
                format!("ALTER TABLE {other} DROP FOREIGN KEY {fk}")
            } else {
                format!("ALTER TABLE {other} DROP CONSTRAINT {fk}")
            });
        }

        // 5. 删除旧表，新表改回原名
        let drop_at = steps.len();
        let mut sql = |s: String| steps.push(RebuildStep::Sql { sql: s });
        sql(format!("DROP TABLE {q}"));
        sql(if mysql {
            format!("ALTER TABLE {q_tmp} RENAME TO {q}")
        } else {
            format!("ALTER TABLE {q_tmp} RENAME TO {}", self.ident(name))
        });

        // 6. 约束、索引、外键、触发器
        let mut changes = Vec::new();
        if let (Some(key), false) = (&draft.primary_key, mysql) {
            changes.push(C::AddPrimaryKey { key: key.clone() });
        }
        changes.extend(draft.uniques.iter().map(|u| C::AddUnique { unique: u.clone() }));
        changes.extend(draft.checks.iter().map(|c| C::AddCheck { check: c.clone() }));
        changes.extend(draft.indexes.iter().map(|x| C::AddIndex { index: x.clone() }));
        changes.extend(draft.foreign_keys.iter().map(|f| C::AddForeignKey { key: f.clone() }));
        for s in self.design_sql(schema, name, &Diff { changes, appended: true })? {
            sql(s.sql);
        }
        for t in &ctx.triggers {
            sql(t.trim().trim_end_matches(';').to_string()
                + if self.family() == Family::Postgres || mysql { "" } else { ";" });
        }

        // 7. 其他表指向本表的外键（引用列随改名）
        for (rk, key) in &referencing {
            let key =
                ForeignKey { ref_schema: Some(schema.to_string()), ref_table: name.clone(), ..key.clone() };
            sql(format!(
                "ALTER TABLE {} ADD {}{}",
                self.qualified(Some(&rk.schema), &rk.table),
                self.constraint_name(&key.name),
                self.fk_clause(&rk.schema, &key)
            ));
        }

        // 8. 自增接续、授权、重新编译视图
        for c in draft.columns.iter().filter(|c| c.auto_increment && old_of(&c.name).is_some()) {
            match self.family() {
                Family::Postgres => sql(format!(
                    "SELECT pg_catalog.setval(pg_catalog.pg_get_serial_sequence({}, {}), COALESCE(MAX({col}), 0) + 1, false) FROM {q}",
                    self.literal(&Value::String(q.clone()))?,
                    self.literal(&Value::String(c.name.clone()))?,
                    col = self.ident(&c.name)
                )),
                Family::Oracle => sql(format!(
                    "ALTER TABLE {q} MODIFY ({} GENERATED BY DEFAULT AS IDENTITY (START WITH LIMIT VALUE))",
                    self.ident(&c.name)
                )),
                Family::MySql | Family::Dameng => {}
            }
        }
        if !mysql {
            for g in &ctx.grants {
                let grantee = if g.grantee.eq_ignore_ascii_case("PUBLIC") {
                    "PUBLIC".into()
                } else {
                    self.ident(&g.grantee)
                };
                sql(format!(
                    "GRANT {} ON {q} TO {grantee}{}",
                    g.privilege,
                    if g.grantable { " WITH GRANT OPTION" } else { "" }
                ));
            }
        }
        let mut warnings = vec![msg!("design.rebuild.no_writes")];
        if matches!(self.family(), Family::Oracle | Family::Dameng) && !ctx.views.is_empty() {
            for v in &ctx.views {
                let kind = if v.materialized { "MATERIALIZED VIEW" } else { "VIEW" };
                sql(format!("ALTER {kind} {} COMPILE", self.qualified(Some(&v.schema), &v.name)));
            }
            warnings.push(msg!("design.rebuild.views_recompiled", views = view_list()));
        }
        if !mysql {
            warnings.push(msg!("design.rebuild.storage"));
        }
        Ok(RebuildOutcome::Script(Rebuild {
            steps,
            drop_at,
            cleanup: format!("DROP TABLE {q_tmp}"),
            warnings,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::super::sql::fixtures::*;
    use super::*;
    use crate::Variant;
    use crate::meta::{DependentView, Grant, ReferencingKey, TableStorage};

    fn render(r: &RebuildOutcome) -> String {
        let RebuildOutcome::Script(r) = r else { panic!("{r:?}") };
        let mut out: Vec<String> = r
            .steps
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let mark = if i == r.drop_at { "▶ " } else { "" };
                match s {
                    RebuildStep::Sql { sql } => format!("{mark}{sql}"),
                    RebuildStep::CountCheck { source, target } => {
                        format!("{mark}-- COUNT {source} = {target}")
                    }
                }
            })
            .collect();
        out.push(format!("-- cleanup: {}", r.cleanup));
        out.join("\n")
    }

    /// 把 amount 挪到最前、code 改名为 order_code 并改类型。
    fn moved(orig: &Table, varchar: &str) -> (Table, Renames) {
        let mut d = orig.clone();
        let amount = d.columns.pop().unwrap();
        d.columns.insert(0, amount);
        d.columns[2].name = "order_code".into();
        d.columns[2].data_type.native = varchar.into();
        d.uniques[0].columns = vec!["order_code".into()];
        d.foreign_keys[0].columns = vec!["order_code".into()];
        (d, Renames { columns: [("order_code".into(), "code".into())].into(), ..Default::default() })
    }

    fn ctx(schema: &str, trigger: &str) -> RebuildContext {
        RebuildContext {
            referencing: vec![ReferencingKey {
                schema: schema.into(),
                table: "items".into(),
                key: ForeignKey {
                    name: Some("fk_items_order".into()),
                    columns: vec!["order_code".into()],
                    ref_schema: Some(schema.into()),
                    ref_table: "orders".into(),
                    ref_columns: vec!["code".into()],
                    on_delete: Some("CASCADE".into()),
                    on_update: None,
                },
            }],
            views: vec![],
            grants: vec![Grant { grantee: "report".into(), privilege: "SELECT".into(), grantable: false }],
            storage: TableStorage {
                engine: Some("InnoDB".into()),
                charset: Some("utf8mb4".into()),
                collation: Some("utf8mb4_0900_ai_ci".into()),
                tablespace: None,
                row_count: Some(10),
                clustered: None,
            },
            triggers: vec![trigger.into()],
        }
    }

    #[test]
    fn scripts_by_dialect() {
        let cases = [
            (
                Variant::MySql,
                "shop",
                ["BIGINT", "VARCHAR(20)", "DECIMAL(10,2)"],
                "VARCHAR(50)",
                "CREATE TRIGGER `trg` BEFORE INSERT ON `orders` FOR EACH ROW SET NEW.amount = 0",
            ),
            (
                Variant::Postgres,
                "public",
                ["bigint", "varchar(20)", "numeric(10,2)"],
                "varchar(50)",
                "CREATE TRIGGER trg BEFORE INSERT ON public.orders FOR EACH ROW EXECUTE FUNCTION f()",
            ),
            (
                Variant::Oracle,
                "HR",
                ["NUMBER(19)", "VARCHAR2(20)", "NUMBER(10,2)"],
                "VARCHAR2(50 CHAR)",
                "CREATE OR REPLACE TRIGGER trg\nBEFORE INSERT ON orders\nFOR EACH ROW\nBEGIN NULL; END;",
            ),
            (
                Variant::Dameng,
                "HR",
                ["BIGINT", "VARCHAR(20)", "DECIMAL(10,2)"],
                "VARCHAR(50)",
                "CREATE OR REPLACE TRIGGER trg BEFORE INSERT ON orders FOR EACH ROW BEGIN NULL; END;",
            ),
        ];
        for (v, schema, types, varchar, trigger) in cases {
            let d = Dialect::new(v);
            let orig = table(types);
            let (draft, renames) = moved(&orig, varchar);
            let mut c = ctx(schema, trigger);
            if v != Variant::MySql {
                c.storage = TableStorage { tablespace: Some("USERS".into()), ..Default::default() };
            }
            if matches!(v, Variant::Oracle | Variant::Dameng) {
                c.views = vec![DependentView {
                    schema: schema.into(),
                    name: "v_orders".into(),
                    materialized: false,
                }];
            }
            let out = d.rebuild(schema, &orig, &draft, &renames, &c).unwrap();
            insta::assert_snapshot!(format!("rebuild_{v:?}").to_lowercase(), render(&out));
        }
    }

    #[test]
    fn refusals() {
        let orig = table(["bigint", "varchar(20)", "numeric(10,2)"]);
        let (draft, renames) = moved(&orig, "varchar(50)");
        let pg = Dialect::new(Variant::Postgres);
        let c = RebuildContext {
            views: vec![DependentView { schema: "public".into(), name: "v".into(), materialized: true }],
            ..Default::default()
        };
        assert_eq!(
            pg.rebuild("public", &orig, &draft, &renames, &c).unwrap(),
            RebuildOutcome::Refused { reason: msg!("design.rebuild.dependent_views", views = "public.v") }
        );
        // Oracle、达梦的依赖视图在最后重新编译，不拒绝
        assert!(matches!(
            Dialect::new(Variant::Oracle).rebuild("HR", &orig, &draft, &renames, &c).unwrap(),
            RebuildOutcome::Script(_)
        ));
        let mut part = orig.clone();
        part.partition = Some("RANGE (id)".into());
        assert_eq!(
            pg.rebuild("public", &part, &draft, &renames, &RebuildContext::default()).unwrap(),
            RebuildOutcome::Refused { reason: msg!("design.rebuild.partitioned") }
        );
        // 被其他表引用的列被删掉
        let mut gone = draft.clone();
        gone.columns.retain(|c| c.name != "order_code");
        gone.uniques.clear();
        gone.foreign_keys.clear();
        let c = ctx("public", "");
        assert_eq!(
            pg.rebuild("public", &orig, &gone, &renames, &c).unwrap(),
            RebuildOutcome::Refused {
                reason: msg!("design.rebuild.referenced_column", table = "items", column = "code")
            }
        );
    }

    #[test]
    fn temp_names_fit_the_limit() {
        let pg = Dialect::new(Variant::Postgres);
        assert_eq!(pg.temp_table_name("orders"), "orders_dblens_new");
        let long = "a".repeat(60);
        let t = pg.temp_table_name(&long);
        assert_eq!(t.len(), 63);
        assert!(t.ends_with("_dblens_new"));
        assert_ne!(t, pg.temp_table_name(&"a".repeat(61)), "截断后仍不同");
        let ora11 = Dialect::new(Variant::Oracle).with_version(Some(110200));
        assert_eq!(ora11.temp_table_name("CUSTOMER_ORDERS_2024").len(), 30);
        let cjk = Dialect::new(Variant::MySql).temp_table_name(&"订单".repeat(20));
        assert!(cjk.len() <= 64 && cjk.ends_with("_dblens_new"));
    }
}
