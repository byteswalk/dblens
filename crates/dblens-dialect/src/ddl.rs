//! "查看 DDL"（M1c2 设计文档第 3 节）：
//! - 表、视图、序列、同义词：MySQL 系用 `SHOW CREATE …`，Oracle、达梦用 `DBMS_METADATA.GET_DDL`，由数据库给出
//!   （`ddl_query`）；PG 系没有取整张表 DDL 的函数，由结构模型拼出（`table_ddl` 等）。
//! - 函数、过程、包：各方言都用已有的程序源码查询（`MetaKey::RoutineSource`）。它按行读取，
//!   不受 LOB 只读前 64 KB 预览的限制（`DBMS_METADATA` 返回的是 CLOB）。

use dblens_core::schema::{Sequence, Table, View};
use dblens_core::{Result, Value, msg};

use crate::meta::MetaRows;
use serde::{Deserialize, Serialize};

use crate::meta::{MetaKey, MetaQuery};
use crate::{Dialect, Family};

/// 可以查看 DDL 的对象种类。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DdlKind {
    Table,
    View,
    MaterializedView,
    Sequence,
    Function,
    Procedure,
    Package,
    PackageBody,
    Synonym,
}

impl DdlKind {
    pub fn as_str(self) -> &'static str {
        match self {
            DdlKind::Table => "table",
            DdlKind::View => "view",
            DdlKind::MaterializedView => "materialized_view",
            DdlKind::Sequence => "sequence",
            DdlKind::Function => "function",
            DdlKind::Procedure => "procedure",
            DdlKind::Package => "package",
            DdlKind::PackageBody => "package_body",
            DdlKind::Synonym => "synonym",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        [
            DdlKind::Table,
            DdlKind::View,
            DdlKind::MaterializedView,
            DdlKind::Sequence,
            DdlKind::Function,
            DdlKind::Procedure,
            DdlKind::Package,
            DdlKind::PackageBody,
            DdlKind::Synonym,
        ]
        .into_iter()
        .find(|k| k.as_str() == s)
    }
}

impl Dialect {
    /// 由数据库直接给出 DDL 的查询（结果见 `ddl_text`）。返回 `Ok(None)` 时由调用方拼装：
    /// PG 系的表、视图、物化视图、序列用结构模型，各方言的函数、过程、包用程序源码。
    /// 该方言没有这种对象时返回 `invalid`。
    pub fn ddl_query(&self, kind: DdlKind, schema: &str, name: &str) -> Result<Option<MetaQuery>> {
        let unsupported = || dblens_core::Error::invalid(msg!("ddl.unsupported", kind = kind.as_str()));
        let query = |sql: String, params: Vec<Value>| Ok(Some(MetaQuery { key: MetaKey::Ddl, sql, params }));
        match self.family() {
            Family::MySql => {
                let what = match kind {
                    DdlKind::Table => "TABLE",
                    DdlKind::View => "VIEW",
                    DdlKind::Sequence => "SEQUENCE",
                    DdlKind::Function | DdlKind::Procedure => return Ok(None),
                    _ => return Err(unsupported()),
                };
                // SHOW CREATE 不能用绑定变量，名字按方言加引号
                query(format!("SHOW CREATE {what} {}", self.qualified(Some(schema), name)), vec![])
            }
            Family::Oracle | Family::Dameng => {
                let object_type = match kind {
                    DdlKind::Table => "TABLE",
                    DdlKind::View => "VIEW",
                    DdlKind::MaterializedView => "MATERIALIZED_VIEW",
                    DdlKind::Sequence => "SEQUENCE",
                    DdlKind::Synonym => "SYNONYM",
                    DdlKind::Function | DdlKind::Procedure | DdlKind::Package | DdlKind::PackageBody => {
                        return Ok(None);
                    }
                };
                let sql = format!(
                    "SELECT DBMS_METADATA.GET_DDL({}, {}, {}) AS ddl FROM DUAL",
                    self.placeholder(1),
                    self.placeholder(2),
                    self.placeholder(3)
                );
                query(
                    sql,
                    vec![
                        Value::String(object_type.into()),
                        Value::String(name.into()),
                        Value::String(schema.into()),
                    ],
                )
            }
            Family::Postgres => match kind {
                DdlKind::Table
                | DdlKind::View
                | DdlKind::MaterializedView
                | DdlKind::Sequence
                | DdlKind::Function
                | DdlKind::Procedure => Ok(None),
                _ => Err(unsupported()),
            },
            Family::Informix => match kind {
                DdlKind::Table
                | DdlKind::View
                | DdlKind::Sequence
                | DdlKind::Function
                | DdlKind::Procedure => Ok(None),
                _ => Err(unsupported()),
            },
            // SQL Server：视图的模块定义就是完整的 CREATE VIEW；同义词由系统视图拼出；表、序列用结构模型，函数、过程用程序源码。
            // 名字用两段名交给 OBJECT_ID（在当前库中查找，调用方先切到对象所在的库）
            Family::SqlServer => {
                let two_part =
                    Value::String(format!("{}.{}", self.quote_ident(schema), self.quote_ident(name)));
                match kind {
                    DdlKind::Table | DdlKind::Sequence | DdlKind::Function | DdlKind::Procedure => Ok(None),
                    DdlKind::View => query(
                        "SELECT m.definition AS ddl FROM sys.sql_modules m WHERE m.object_id = OBJECT_ID(@P1)".into(),
                        vec![two_part],
                    ),
                    DdlKind::Synonym => query(
                        "SELECT N'CREATE SYNONYM ' + QUOTENAME(SCHEMA_NAME(s.schema_id)) + N'.' + QUOTENAME(s.name) \
                         + N' FOR ' + s.base_object_name + N';' AS ddl \
                         FROM sys.synonyms s WHERE s.object_id = OBJECT_ID(@P1)"
                            .into(),
                        vec![two_part],
                    ),
                    _ => Err(unsupported()),
                }
            }
        }
    }

    fn comment_literal(&self, text: &str) -> Result<String> {
        self.literal(&Value::String(text.into()))
    }

    /// PG 系、SQL Server 的建表语句：列、约束、分区，之后是索引、触发器和注释（`table_extras`），每条以 `;` 结束。
    pub fn table_ddl(&self, schema: &str, t: &Table) -> Result<String> {
        let mut out = self.table_create(schema, t);
        out += &self.table_extras(schema, t, true)?;
        Ok(out.trim_end().to_string() + "\n")
    }

    pub(crate) fn table_create(&self, schema: &str, t: &Table) -> String {
        let q = self.qualified(Some(schema), &t.name);
        let list = |cols: &[String]| cols.iter().map(|c| self.ident(c)).collect::<Vec<_>>().join(", ");
        let mut items: Vec<String> = Vec::new();
        for c in &t.columns {
            let mut s = format!("{} {}", self.ident(&c.name), c.data_type.native);
            if self.family() == Family::SqlServer {
                // SQL Server 的计算列没有类型和可空性：`名 AS (表达式)`
                if let Some(g) = &c.generated {
                    items.push(format!("{} AS ({})", self.ident(&c.name), strip_parens(g)));
                    continue;
                }
                if c.auto_increment {
                    s += " IDENTITY(1,1)";
                }
                if let Some(d) = &c.default {
                    s += &format!(" DEFAULT {d}");
                }
            } else if let Some(g) = &c.generated {
                s += &format!(" GENERATED ALWAYS AS ({g}) STORED");
            } else if let Some(d) = &c.default {
                s += &format!(" DEFAULT {d}");
            } else if c.auto_increment {
                s += " GENERATED BY DEFAULT AS IDENTITY";
            }
            if !c.nullable {
                s += " NOT NULL";
            }
            items.push(s);
        }
        let constraint = |name: &Option<String>| {
            name.as_ref().map(|n| format!("CONSTRAINT {} ", self.ident(n))).unwrap_or_default()
        };
        if let Some(pk) = &t.primary_key {
            items.push(format!("{}PRIMARY KEY ({})", constraint(&pk.name), list(&pk.columns)));
        }
        for u in &t.uniques {
            items.push(format!("{}UNIQUE ({})", constraint(&u.name), list(&u.columns)));
        }
        for f in &t.foreign_keys {
            let mut s = format!(
                "{}FOREIGN KEY ({}) REFERENCES {} ({})",
                constraint(&f.name),
                list(&f.columns),
                self.qualified(f.ref_schema.as_deref().or(Some(schema)), &f.ref_table),
                list(&f.ref_columns)
            );
            for (what, action) in [("DELETE", &f.on_delete), ("UPDATE", &f.on_update)] {
                if let Some(a) = action.as_deref().filter(|a| *a != "NO ACTION") {
                    s += &format!(" ON {what} {a}");
                }
            }
            items.push(s);
        }
        for c in &t.checks {
            let e = c.expression.trim();
            let e = if e.starts_with('(') && e.ends_with(')') { e.to_string() } else { format!("({e})") };
            items.push(format!("{}CHECK {e}", constraint(&c.name)));
        }
        let mut out = format!("CREATE TABLE {q} (\n    {}\n)", items.join(",\n    "));
        if let Some(p) = &t.partition {
            out += &format!(" PARTITION BY {p}");
        }
        out + ";\n"
    }

    /// 建表语句之后的部分：不属于约束的索引、触发器（`triggers` 为 true 时）、表和列的注释，每条一行、以 `;` 结束。
    /// Oracle、达梦的 `DBMS_METADATA` 取表时不含索引和注释，也用这里补上。
    pub fn table_extras(&self, schema: &str, t: &Table, triggers: bool) -> Result<String> {
        let q = self.qualified(Some(schema), &t.name);
        let mut out = String::new();
        for i in &t.indexes {
            let cols: Vec<String> = i
                .columns
                .iter()
                .map(|c| {
                    // 普通列加引号；表达式（如 lower(name)）原样
                    let name = if t.columns.iter().any(|x| x.name == c.name) {
                        self.ident(&c.name)
                    } else {
                        c.name.clone()
                    };
                    if c.descending { format!("{name} DESC") } else { name }
                })
                .collect();
            let using = i
                .method
                .as_deref()
                // Oracle、达梦的 NORMAL / BITMAP 等不是 USING 子句
                .filter(|m| self.family() == Family::Postgres && !m.eq_ignore_ascii_case("btree"))
                .map(|m| format!(" USING {}", m.to_ascii_lowercase()))
                .unwrap_or_default();
            out += &format!(
                "\nCREATE {}INDEX {} ON {q}{using} ({});",
                if i.unique { "UNIQUE " } else { "" },
                self.ident(&i.name),
                cols.join(", ")
            );
        }
        let sqlserver = self.family() == Family::SqlServer;
        for tr in t.triggers.iter().filter(|_| triggers) {
            if let Some(d) = &tr.definition {
                if sqlserver {
                    // CREATE TRIGGER 必须是批中的第一条语句
                    out += &format!("\nGO\n{}\nGO", d.trim());
                } else {
                    out += &format!("\n{};", d.trim_end_matches(';').trim());
                }
            }
        }
        if sqlserver {
            // SQL Server 的注释是扩展属性 MS_Description
            let property = |text: &str, column: Option<&str>| -> Result<String> {
                let mut s = format!(
                    "\nEXEC sys.sp_addextendedproperty N'MS_Description', {}, N'SCHEMA', {}, N'TABLE', {}",
                    self.comment_literal(text)?,
                    self.comment_literal(schema)?,
                    self.comment_literal(&t.name)?
                );
                if let Some(col) = column {
                    s += &format!(", N'COLUMN', {}", self.comment_literal(col)?);
                }
                Ok(s + ";")
            };
            if let Some(c) = &t.comment {
                out += &property(c, None)?;
            }
            for c in &t.columns {
                if let Some(cm) = &c.comment {
                    out += &property(cm, Some(&c.name))?;
                }
            }
            return Ok(out);
        }
        if let Some(c) = &t.comment {
            out += &format!("\nCOMMENT ON TABLE {q} IS {};", self.comment_literal(c)?);
        }
        for c in &t.columns {
            if let Some(cm) = &c.comment {
                out += &format!(
                    "\nCOMMENT ON COLUMN {q}.{} IS {};",
                    self.ident(&c.name),
                    self.comment_literal(cm)?
                );
            }
        }
        Ok(out)
    }

    /// PG 系的视图 / 物化视图：`CREATE [MATERIALIZED] VIEW … AS 定义;`，有注释时附上。
    pub fn view_ddl(&self, schema: &str, v: &View, definition: &str, materialized: bool) -> Result<String> {
        let q = self.qualified(Some(schema), &v.name);
        let kind = if materialized { "MATERIALIZED VIEW" } else { "VIEW" };
        let mut out = format!("CREATE {kind} {q} AS\n{};\n", definition.trim().trim_end_matches(';'));
        if let Some(c) = &v.comment {
            out += &format!("\nCOMMENT ON {kind} {q} IS {};\n", self.comment_literal(c)?);
        }
        Ok(out)
    }

    /// PG 系的序列。
    pub fn sequence_ddl(&self, schema: &str, s: &Sequence) -> String {
        let mut out = format!("CREATE SEQUENCE {}", self.qualified(Some(schema), &s.name));
        let mut opt = |label: &str, v: &Option<String>| {
            if let Some(v) = v {
                out += &format!("\n    {label} {v}");
            }
        };
        opt("START WITH", &s.start);
        opt("INCREMENT BY", &s.increment);
        opt("MINVALUE", &s.min_value);
        opt("MAXVALUE", &s.max_value);
        if let Some(c) = s.cache {
            out += &format!("\n    CACHE {c}");
        }
        out += if s.cycle { "\n    CYCLE" } else { "\n    NO CYCLE" };
        out + ";\n"
    }
}

/// 去掉表达式最外层成对的括号（SQL Server 的定义文本自带括号，如 `([a]+[b])`）。
fn strip_parens(e: &str) -> &str {
    let e = e.trim();
    match e.strip_prefix('(').and_then(|r| r.strip_suffix(')')) {
        // 只有首尾的括号互相配对时才去掉：`(a)+(b)` 不能变成 `a)+(b`
        Some(inner) if balanced(inner) => inner,
        _ => e,
    }
}

fn balanced(s: &str) -> bool {
    let mut depth = 0i32;
    for c in s.chars() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth < 0 {
                    return false;
                }
            }
            _ => {}
        }
    }
    depth == 0
}

/// 数据库返回的 DDL 文本：统一换行，去掉首尾空白。
pub fn normalize_ddl(text: &str) -> String {
    text.replace("\r\n", "\n").trim().to_string() + "\n"
}

/// `ddl_query` 的结果：第一行的 `ddl` 列（Oracle、达梦），或名字以 `Create ` 开头的列（MySQL 的 `SHOW CREATE`）。
/// LOB 只读到前 64 KB 时在末尾注明已截断。没有结果时返回 None。
pub fn ddl_text(rows: &MetaRows) -> Option<String> {
    let row = rows.iter().next()?;
    let column = row.columns().iter().find(|c| *c == "ddl" || c.starts_with("create "))?.clone();
    let truncated = matches!(row.get(&column), Value::Lob(l) if l.truncated);
    let mut text = normalize_ddl(&row.str(&column)?);
    if truncated {
        text += &format!("-- {}\n", msg!("ddl.truncated").current());
    }
    Some(text)
}

#[cfg(test)]
mod tests {
    use dblens_core::schema::{
        CheckConstraint, Column, ForeignKey, Index, IndexColumn, PrimaryKey, Trigger, UniqueConstraint,
    };
    use dblens_core::{ColumnType, LogicalType};

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

    fn pg() -> Dialect {
        Dialect::new(Variant::Postgres)
    }

    #[test]
    fn query_by_family() {
        let my =
            Dialect::new(Variant::MySql).ddl_query(DdlKind::Table, "shop", "order items").unwrap().unwrap();
        assert_eq!(my.sql, "SHOW CREATE TABLE shop.`order items`");
        assert!(my.params.is_empty());
        assert!(Dialect::new(Variant::MariaDb).ddl_query(DdlKind::Package, "s", "p").is_err());
        assert_eq!(
            Dialect::new(Variant::MySql).ddl_query(DdlKind::Procedure, "s", "p").unwrap(),
            None,
            "程序用源码"
        );

        let ora = Dialect::new(Variant::Oracle).ddl_query(DdlKind::Synonym, "HR", "SYN").unwrap().unwrap();
        assert_eq!(ora.sql, "SELECT DBMS_METADATA.GET_DDL(:1, :2, :3) AS ddl FROM DUAL");
        assert_eq!(
            ora.params,
            [Value::String("SYNONYM".into()), Value::String("SYN".into()), Value::String("HR".into())]
        );
        let dm =
            Dialect::new(Variant::Dameng).ddl_query(DdlKind::MaterializedView, "HR", "MV").unwrap().unwrap();
        assert_eq!(dm.params[0], Value::String("MATERIALIZED_VIEW".into()));
        assert_eq!(Dialect::new(Variant::Oracle).ddl_query(DdlKind::PackageBody, "HR", "P").unwrap(), None);

        assert_eq!(pg().ddl_query(DdlKind::Table, "public", "t").unwrap(), None);
        assert!(pg().ddl_query(DdlKind::Synonym, "public", "t").is_err());
    }

    #[test]
    fn pg_table_with_constraints_indexes_and_comments() {
        let mut id = col("id", "integer");
        id.nullable = false;
        id.auto_increment = true;
        let mut code = col("code", "character varying(20)");
        code.nullable = false;
        code.default = Some("'x'::character varying".into());
        code.comment = Some("编码 'A'".into());
        let mut total = col("total", "numeric(10,2)");
        total.generated = Some("(price * 2)".into());
        let t = Table {
            name: "Orders".into(),
            comment: Some("订单".into()),
            columns: vec![id, code, col("dept_id", "integer"), total],
            primary_key: Some(PrimaryKey { name: Some("orders_pkey".into()), columns: vec!["id".into()] }),
            uniques: vec![UniqueConstraint {
                name: Some("orders_code_key".into()),
                columns: vec!["code".into()],
            }],
            foreign_keys: vec![ForeignKey {
                name: Some("orders_dept_fk".into()),
                columns: vec!["dept_id".into()],
                ref_schema: Some("hr".into()),
                ref_table: "dept".into(),
                ref_columns: vec!["id".into()],
                on_delete: Some("CASCADE".into()),
                on_update: Some("NO ACTION".into()),
            }],
            checks: vec![CheckConstraint {
                name: Some("orders_total_check".into()),
                expression: "(total > (0)::numeric)".into(),
            }],
            indexes: vec![
                Index {
                    name: "orders_dept_idx".into(),
                    columns: vec![IndexColumn { name: "dept_id".into(), descending: true }],
                    unique: false,
                    method: Some("BTREE".into()),
                },
                Index {
                    name: "orders_lower_code".into(),
                    columns: vec![IndexColumn { name: "lower((code)::text)".into(), descending: false }],
                    unique: true,
                    method: Some("HASH".into()),
                },
            ],
            triggers: vec![Trigger {
                name: "trg".into(),
                timing: None,
                events: vec![],
                definition: Some(
                    "CREATE TRIGGER trg BEFORE INSERT ON public.\"Orders\" FOR EACH ROW EXECUTE FUNCTION f()"
                        .into(),
                ),
            }],
            partition: None,
        };
        assert_eq!(
            pg().table_ddl("public", &t).unwrap(),
            "CREATE TABLE public.\"Orders\" (
    id integer GENERATED BY DEFAULT AS IDENTITY NOT NULL,
    code character varying(20) DEFAULT 'x'::character varying NOT NULL,
    dept_id integer,
    total numeric(10,2) GENERATED ALWAYS AS ((price * 2)) STORED,
    CONSTRAINT orders_pkey PRIMARY KEY (id),
    CONSTRAINT orders_code_key UNIQUE (code),
    CONSTRAINT orders_dept_fk FOREIGN KEY (dept_id) REFERENCES hr.dept (id) ON DELETE CASCADE,
    CONSTRAINT orders_total_check CHECK (total > (0)::numeric)
);

CREATE INDEX orders_dept_idx ON public.\"Orders\" (dept_id DESC);
CREATE UNIQUE INDEX orders_lower_code ON public.\"Orders\" USING hash (lower((code)::text));
CREATE TRIGGER trg BEFORE INSERT ON public.\"Orders\" FOR EACH ROW EXECUTE FUNCTION f();
COMMENT ON TABLE public.\"Orders\" IS '订单';
COMMENT ON COLUMN public.\"Orders\".code IS '编码 ''A''';
"
        );
    }

    #[test]
    fn sqlserver_queries_and_table() {
        let d = Dialect::new(Variant::SqlServer);
        let v = d.ddl_query(DdlKind::View, "dbo", "v]1").unwrap().unwrap();
        assert!(v.sql.contains("sys.sql_modules"), "{}", v.sql);
        assert_eq!(v.params, [Value::String("[dbo].[v]]1]".into())]);
        assert!(d.ddl_query(DdlKind::Synonym, "dbo", "s").unwrap().unwrap().sql.contains("CREATE SYNONYM"));
        assert_eq!(d.ddl_query(DdlKind::Table, "dbo", "t").unwrap(), None);
        assert_eq!(d.ddl_query(DdlKind::Procedure, "dbo", "p").unwrap(), None);
        assert!(d.ddl_query(DdlKind::Package, "dbo", "p").is_err());

        let mut id = col("id", "int");
        id.nullable = false;
        id.auto_increment = true;
        let mut name = col("name", "nvarchar(50)");
        name.default = Some("(N'x')".into());
        name.comment = Some("名称".into());
        let mut total = col("total", "int");
        total.generated = Some("([id]*(2))".into());
        let t = Table {
            name: "Orders".into(),
            comment: Some("订单".into()),
            columns: vec![id, name, total],
            primary_key: Some(PrimaryKey { name: Some("PK_Orders".into()), columns: vec!["id".into()] }),
            indexes: vec![Index {
                name: "IX_name".into(),
                columns: vec![IndexColumn { name: "name".into(), descending: false }],
                unique: false,
                method: Some("NONCLUSTERED".into()),
            }],
            triggers: vec![Trigger {
                name: "trg".into(),
                timing: None,
                events: vec![],
                definition: Some("CREATE TRIGGER trg ON dbo.Orders AFTER INSERT AS SELECT 1".into()),
            }],
            ..Default::default()
        };
        assert_eq!(
            d.table_ddl("dbo", &t).unwrap(),
            "CREATE TABLE dbo.Orders (
    id int IDENTITY(1,1) NOT NULL,
    name nvarchar(50) DEFAULT (N'x'),
    total AS ([id]*(2)),
    CONSTRAINT PK_Orders PRIMARY KEY (id)
);

CREATE INDEX IX_name ON dbo.Orders (name);
GO
CREATE TRIGGER trg ON dbo.Orders AFTER INSERT AS SELECT 1
GO
EXEC sys.sp_addextendedproperty N'MS_Description', N'订单', N'SCHEMA', N'dbo', N'TABLE', N'Orders';
EXEC sys.sp_addextendedproperty N'MS_Description', N'名称', N'SCHEMA', N'dbo', N'TABLE', N'Orders', N'COLUMN', N'name';
"
        );
        assert_eq!(strip_parens("(a)+(b)"), "(a)+(b)");
        assert_eq!(strip_parens(" ((a)) "), "(a)");
    }

    #[test]
    fn extras_without_triggers() {
        let mut c = col("ID", "NUMBER");
        c.comment = Some("编号".into());
        let t = Table {
            name: "T".into(),
            comment: Some("表".into()),
            columns: vec![c],
            indexes: vec![Index {
                name: "T_IDX".into(),
                columns: vec![IndexColumn { name: "ID".into(), descending: false }],
                unique: false,
                method: Some("NORMAL".into()),
            }],
            triggers: vec![Trigger {
                name: "TRG".into(),
                timing: None,
                events: vec![],
                definition: Some("x".into()),
            }],
            ..Default::default()
        };
        assert_eq!(
            Dialect::new(Variant::Oracle).table_extras("HR", &t, false).unwrap(),
            "\nCREATE INDEX T_IDX ON HR.T (ID);\nCOMMENT ON TABLE HR.T IS '表';\nCOMMENT ON COLUMN HR.T.ID IS '编号';"
        );
    }

    #[test]
    fn pg_partitioned_table_and_same_schema_reference() {
        let t = Table {
            name: "m".into(),
            columns: vec![col("d", "date")],
            foreign_keys: vec![ForeignKey {
                name: None,
                columns: vec!["d".into()],
                ref_schema: None,
                ref_table: "cal".into(),
                ref_columns: vec!["d".into()],
                on_delete: None,
                on_update: None,
            }],
            partition: Some("RANGE (d)".into()),
            ..Default::default()
        };
        assert_eq!(
            pg().table_ddl("s", &t).unwrap(),
            "CREATE TABLE s.m (\n    d date,\n    FOREIGN KEY (d) REFERENCES s.cal (d)\n) PARTITION BY RANGE (d);\n"
        );
    }

    #[test]
    fn pg_view_and_sequence() {
        let v = View { name: "v".into(), comment: Some("视图".into()), definition: None, columns: vec![] };
        assert_eq!(
            pg().view_ddl("s", &v, " SELECT 1;\n", true).unwrap(),
            "CREATE MATERIALIZED VIEW s.v AS\nSELECT 1;\n\nCOMMENT ON MATERIALIZED VIEW s.v IS '视图';\n"
        );
        let s = Sequence {
            name: "seq".into(),
            start: Some("100".into()),
            increment: Some("5".into()),
            min_value: Some("1".into()),
            max_value: None,
            cycle: false,
            cache: Some(10),
        };
        assert_eq!(
            pg().sequence_ddl("s", &s),
            "CREATE SEQUENCE s.seq\n    START WITH 100\n    INCREMENT BY 5\n    MINVALUE 1\n    CACHE 10\n    NO CYCLE;\n"
        );
    }

    #[test]
    fn text_from_result_rows() {
        let ora = MetaRows::new(
            vec!["DDL".into()],
            vec![vec![Value::String("\n  CREATE TABLE x\r\n(a INT);\r\n".into())]],
        );
        assert_eq!(ddl_text(&ora).unwrap(), "CREATE TABLE x\n(a INT);\n");
        let my = MetaRows::new(
            vec!["View".into(), "Create View".into(), "character_set_client".into()],
            vec![vec![
                Value::String("v".into()),
                Value::String("CREATE VIEW v AS select 1".into()),
                Value::String("utf8".into()),
            ]],
        );
        assert_eq!(ddl_text(&my).unwrap(), "CREATE VIEW v AS select 1\n");
        let lob = dblens_core::LobPreview {
            binary: false,
            preview: "CREATE".into(),
            total_len: 99,
            truncated: true,
        };
        let big = MetaRows::new(vec!["DDL".into()], vec![vec![Value::Lob(lob)]]);
        assert!(ddl_text(&big).unwrap().starts_with("CREATE\n-- "));
        assert_eq!(ddl_text(&MetaRows::new(vec!["DDL".into()], vec![])), None);
    }

    #[test]
    fn kinds_round_trip() {
        for k in [
            "table",
            "view",
            "materialized_view",
            "sequence",
            "function",
            "procedure",
            "package",
            "package_body",
            "synonym",
        ] {
            assert_eq!(DdlKind::parse(k).unwrap().as_str(), k);
        }
        assert_eq!(DdlKind::parse("index"), None);
    }
}
