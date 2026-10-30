//! 设计器用的类型目录和方言能力（M3 设计文档 3.6、4.1 节）：前端据此给出类型下拉、启用长度和小数位、
//! 禁用库不支持的选项。

use serde::Serialize;

use crate::{Dialect, Family, Variant};

/// 类型的参数形式。`Length` 也用于时间类型的小数秒精度。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TypeParams {
    None,
    Length,
    PrecisionScale,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TypeEntry {
    pub name: &'static str,
    pub params: TypeParams,
    /// 长度可选按 BYTE 还是 CHAR 计（Oracle、达梦的字符类型）。
    pub char_unit: bool,
}

/// 设计器可用的功能。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DesignCaps {
    /// 能调整已有列的顺序、在指定位置加列（MySQL 家族）。不能时这些改动要重建表。
    pub reorder: bool,
    /// 支持检查约束（MySQL 8.0.16 以前只解析不执行）。
    pub checks: bool,
    pub descending_index: bool,
    /// 外键可选的动作；空表示不支持这一项（如 Oracle 没有 ON UPDATE）。
    pub on_delete: Vec<&'static str>,
    pub on_update: Vec<&'static str>,
    /// 可选的索引方法，第一个是默认值。
    pub index_methods: Vec<&'static str>,
    pub types: Vec<TypeEntry>,
}

const fn t(name: &'static str, params: TypeParams) -> TypeEntry {
    TypeEntry { name, params, char_unit: false }
}

const fn c(name: &'static str) -> TypeEntry {
    TypeEntry { name, params: TypeParams::Length, char_unit: true }
}

use TypeParams::{Length as L, None as N, PrecisionScale as P};

const MYSQL_TYPES: &[TypeEntry] = &[
    t("TINYINT", N),
    t("SMALLINT", N),
    t("MEDIUMINT", N),
    t("INT", N),
    t("BIGINT", N),
    t("DECIMAL", P),
    t("FLOAT", N),
    t("DOUBLE", N),
    t("BIT", L),
    t("CHAR", L),
    t("VARCHAR", L),
    t("TINYTEXT", N),
    t("TEXT", N),
    t("MEDIUMTEXT", N),
    t("LONGTEXT", N),
    t("BINARY", L),
    t("VARBINARY", L),
    t("TINYBLOB", N),
    t("BLOB", N),
    t("MEDIUMBLOB", N),
    t("LONGBLOB", N),
    t("DATE", N),
    t("TIME", L),
    t("DATETIME", L),
    t("TIMESTAMP", L),
    t("YEAR", N),
    t("JSON", N),
];

const POSTGRES_TYPES: &[TypeEntry] = &[
    t("smallint", N),
    t("integer", N),
    t("bigint", N),
    t("numeric", P),
    t("real", N),
    t("double precision", N),
    t("boolean", N),
    t("char", L),
    t("varchar", L),
    t("text", N),
    t("bytea", N),
    t("date", N),
    t("time", L),
    t("timetz", L),
    t("timestamp", L),
    t("timestamptz", L),
    t("interval", N),
    t("json", N),
    t("jsonb", N),
    t("uuid", N),
    t("inet", N),
    t("xml", N),
];

/// 人大金仓、openGauss 的 Oracle 兼容模式另外可选的 Oracle 风格类型（DATE 带时间）。
const PG_ORACLE_MODE_TYPES: &[TypeEntry] =
    &[t("NUMBER", P), t("VARCHAR2", L), t("NVARCHAR2", L), t("CLOB", N), t("BLOB", N), t("DATE", N)];

const ORACLE_TYPES: &[TypeEntry] = &[
    t("NUMBER", P),
    t("INTEGER", N),
    t("FLOAT", L),
    t("BINARY_FLOAT", N),
    t("BINARY_DOUBLE", N),
    c("CHAR"),
    c("VARCHAR2"),
    t("NCHAR", L),
    t("NVARCHAR2", L),
    t("CLOB", N),
    t("NCLOB", N),
    t("BLOB", N),
    t("RAW", L),
    t("DATE", N),
    t("TIMESTAMP", L),
    t("TIMESTAMP WITH TIME ZONE", L),
    t("TIMESTAMP WITH LOCAL TIME ZONE", L),
    t("INTERVAL YEAR TO MONTH", N),
    t("INTERVAL DAY TO SECOND", N),
];

const DAMENG_TYPES: &[TypeEntry] = &[
    t("TINYINT", N),
    t("SMALLINT", N),
    t("INT", N),
    t("BIGINT", N),
    t("NUMBER", P),
    t("DECIMAL", P),
    t("FLOAT", N),
    t("DOUBLE", N),
    t("BIT", N),
    c("CHAR"),
    c("VARCHAR"),
    c("VARCHAR2"),
    t("TEXT", N),
    t("CLOB", N),
    t("BLOB", N),
    t("BINARY", L),
    t("VARBINARY", L),
    t("DATE", N),
    t("TIME", L),
    t("TIMESTAMP", L),
    t("DATETIME", L),
    t("TIMESTAMP WITH TIME ZONE", L),
];

/// SQL Server：长度可写 MAX 的类型在设计器里按长度填写（M6b 再细化）。
const SQLSERVER_TYPES: &[TypeEntry] = &[
    t("BIT", N),
    t("TINYINT", N),
    t("SMALLINT", N),
    t("INT", N),
    t("BIGINT", N),
    t("DECIMAL", P),
    t("NUMERIC", P),
    t("MONEY", N),
    t("REAL", N),
    t("FLOAT", N),
    t("CHAR", L),
    t("VARCHAR", L),
    t("NCHAR", L),
    t("NVARCHAR", L),
    t("BINARY", L),
    t("VARBINARY", L),
    t("DATE", N),
    t("TIME", L),
    t("DATETIME", N),
    t("DATETIME2", L),
    t("DATETIMEOFFSET", L),
    t("UNIQUEIDENTIFIER", N),
    t("XML", N),
];

impl Dialect {
    pub fn design_caps(&self) -> DesignCaps {
        let v = self.version().unwrap_or(u32::MAX);
        match self.family() {
            Family::MySql => DesignCaps {
                reorder: true,
                // MySQL 8.0.16、MariaDB 10.2 起执行检查约束；OceanBase 报告 5.7 但执行检查约束；
                // TiDB 默认只解析不执行（tidb_enable_check_constraint 关闭）
                checks: match self.variant() {
                    Variant::MariaDb => v >= 100200,
                    Variant::OceanBase => true,
                    Variant::TiDb => false,
                    _ => v >= 80016,
                },
                // 8.0 起支持降序索引；TiDB、OceanBase 接受 DESC 但按升序建
                descending_index: match self.variant() {
                    Variant::MariaDb => true,
                    Variant::TiDb | Variant::OceanBase => false,
                    _ => v >= 80000,
                },
                on_delete: vec!["RESTRICT", "CASCADE", "SET NULL", "NO ACTION"],
                on_update: vec!["RESTRICT", "CASCADE", "SET NULL", "NO ACTION"],
                index_methods: match self.variant() {
                    Variant::TiDb => vec!["BTREE"],
                    Variant::OceanBase => vec!["BTREE", "FULLTEXT"],
                    _ => vec!["BTREE", "HASH", "FULLTEXT", "SPATIAL"],
                },
                types: MYSQL_TYPES.to_vec(),
            },
            Family::Postgres => DesignCaps {
                reorder: false,
                checks: true,
                descending_index: true,
                on_delete: vec!["NO ACTION", "RESTRICT", "CASCADE", "SET NULL", "SET DEFAULT"],
                on_update: vec!["NO ACTION", "RESTRICT", "CASCADE", "SET NULL", "SET DEFAULT"],
                index_methods: match self.variant() {
                    Variant::OpenGauss => vec!["BTREE", "HASH", "GIN", "GIST"],
                    _ => vec!["BTREE", "HASH", "GIN", "GIST", "BRIN", "SPGIST"],
                },
                types: if self.oracle_mode() {
                    POSTGRES_TYPES.iter().chain(PG_ORACLE_MODE_TYPES).cloned().collect()
                } else {
                    POSTGRES_TYPES.to_vec()
                },
            },
            Family::Oracle | Family::Dameng => DesignCaps {
                reorder: false,
                checks: true,
                descending_index: true,
                on_delete: vec!["NO ACTION", "CASCADE", "SET NULL"],
                on_update: vec![],
                index_methods: vec!["NORMAL", "BITMAP"],
                types: if self.family() == Family::Oracle { ORACLE_TYPES } else { DAMENG_TYPES }.to_vec(),
            },
            Family::SqlServer => DesignCaps {
                reorder: false,
                checks: true,
                descending_index: true,
                on_delete: vec!["NO ACTION", "CASCADE", "SET NULL", "SET DEFAULT"],
                on_update: vec!["NO ACTION", "CASCADE", "SET NULL", "SET DEFAULT"],
                index_methods: vec!["NONCLUSTERED", "CLUSTERED"],
                types: SQLSERVER_TYPES.to_vec(),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::Compat;

    #[test]
    fn caps_follow_family_and_version() {
        let mysql57 = Dialect::new(Variant::MySql).with_version(Some(50744)).design_caps();
        assert!(mysql57.reorder && !mysql57.checks && !mysql57.descending_index);
        let mysql8 = Dialect::new(Variant::MySql).with_version(Some(80040)).design_caps();
        assert!(mysql8.checks && mysql8.descending_index);
        let maria = Dialect::new(Variant::MariaDb).with_version(Some(100411)).design_caps();
        assert!(maria.checks && maria.descending_index);
        let pg = Dialect::new(Variant::Kingbase).design_caps();
        assert!(!pg.reorder && pg.on_update.contains(&"SET DEFAULT"));
        assert!(pg.types.iter().any(|t| t.name == "jsonb"));
        let ora = Dialect::new(Variant::Oracle).design_caps();
        assert!(ora.on_update.is_empty());
        let varchar2 = ora.types.iter().find(|t| t.name == "VARCHAR2").unwrap();
        assert_eq!((varchar2.params, varchar2.char_unit), (TypeParams::Length, true));
        let tidb = Dialect::new(Variant::TiDb).with_version(Some(80011)).design_caps();
        assert!(tidb.reorder && !tidb.checks && !tidb.descending_index && tidb.index_methods == ["BTREE"]);
        let ob = Dialect::new(Variant::OceanBase).with_version(Some(50725)).design_caps();
        assert!(ob.checks && !ob.descending_index);
        let kb = Dialect::new(Variant::Kingbase).with_compat(Compat::Oracle).design_caps();
        assert!(kb.types.iter().any(|t| t.name == "VARCHAR2") && kb.types.iter().any(|t| t.name == "jsonb"));
        let kb_pg = Dialect::new(Variant::Kingbase).design_caps();
        assert!(!kb_pg.types.iter().any(|t| t.name == "VARCHAR2"));
        assert!(!Dialect::new(Variant::OpenGauss).design_caps().index_methods.contains(&"BRIN"));
        let dm = Dialect::new(Variant::Dameng).design_caps();
        assert!(dm.types.iter().any(|t| t.name == "VARCHAR" && t.char_unit));
    }

    #[test]
    fn every_catalog_type_parses_to_a_known_logical_type() {
        use dblens_core::LogicalType;
        for v in [Variant::MySql, Variant::Postgres, Variant::Oracle, Variant::Dameng] {
            let d = Dialect::new(v);
            for t in d.design_caps().types {
                let native = match t.params {
                    TypeParams::None => t.name.to_string(),
                    TypeParams::Length => format!("{}(10)", t.name),
                    TypeParams::PrecisionScale => format!("{}(10,2)", t.name),
                };
                // 文本、二进制的若干大类型和 PG 的 inet、xml 等映射为 Other 是允许的；只检查不会 panic 且数值类型正确
                let logical = d.logical_type(&native);
                if matches!(t.name, "DECIMAL" | "NUMBER" | "numeric") {
                    assert!(matches!(logical, LogicalType::Decimal { .. }), "{v:?} {native} → {logical:?}");
                }
            }
        }
    }
}
