//! 方言家族与具体产品变体的识别。

use dblens_core::{Msg, msg};
use dblens_driver::{DriverId, ServerInfo};
use serde::{Deserialize, Serialize};

/// 方言家族：语法规则基本相同的一组数据库。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Family {
    #[serde(rename = "mysql")]
    MySql,
    #[serde(rename = "postgres")]
    Postgres,
    #[serde(rename = "oracle")]
    Oracle,
    #[serde(rename = "dameng")]
    Dameng,
    #[serde(rename = "sqlserver")]
    SqlServer,
}

impl Family {
    /// 脚本中一条语句的写法（M6b 设计文档 5.1 节）：SQL Server 后接单独一行 `GO`（`CREATE VIEW`、`CREATE TRIGGER` 等
    /// 必须是批中第一条），其他库以 `;` 结尾。
    pub fn terminate(self, sql: &str) -> String {
        let sql = sql.trim_end();
        match self {
            Family::SqlServer => format!(
                "{sql}
GO"
            ),
            _ if sql.ends_with(';') => sql.to_string(),
            _ => format!("{sql};"),
        }
    }
}

/// 兼容模式：人大金仓、openGauss 可以按 Oracle 的规则解释 SQL（空串即 NULL、`NUMBER`、`VARCHAR2`、
/// 带时间的 `DATE`、Oracle 风格的过程块）。其他变体恒为 `Native`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum Compat {
    /// 产品自己的语法（人大金仓、openGauss 的 PG 模式也算）。
    #[default]
    #[serde(rename = "native")]
    Native,
    /// 人大金仓的 `database_mode = oracle`、openGauss 的 `DBCOMPATIBILITY 'A'`。
    #[serde(rename = "oracle")]
    Oracle,
}

impl Compat {
    /// 由探测到的模式名得出兼容模式：人大金仓的 `oracle`、openGauss 的 `A` 是 Oracle 模式，其余按原生处理。
    pub fn from_mode(mode: &str) -> Compat {
        if mode.eq_ignore_ascii_case("oracle") || mode.eq_ignore_ascii_case("a") {
            Compat::Oracle
        } else {
            Compat::Native
        }
    }

    /// 已适配的模式名：openGauss 的 B、C 和人大金仓的 mysql、sqlserver 等返回 false。
    pub fn is_adapted_mode(mode: &str) -> bool {
        ["oracle", "a", "pg"].iter().any(|m| mode.eq_ignore_ascii_case(m))
    }
}

/// 具体的数据库产品。兼容库复用所属家族的规则，只在个别地方覆盖。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Variant {
    #[serde(rename = "mysql")]
    MySql,
    #[serde(rename = "mariadb")]
    MariaDb,
    #[serde(rename = "tidb")]
    TiDb,
    /// OceanBase 的 MySQL 模式。
    #[serde(rename = "oceanbase")]
    OceanBase,
    /// 阿里云 PolarDB 的 MySQL 版。版本字符串与 MySQL 相同，靠 `Dialect::refine` 识别。
    #[serde(rename = "polardb")]
    PolarDb,
    #[serde(rename = "postgres")]
    Postgres,
    #[serde(rename = "kingbase")]
    Kingbase,
    #[serde(rename = "opengauss")]
    OpenGauss,
    #[serde(rename = "gaussdb")]
    GaussDb,
    #[serde(rename = "highgo")]
    HighGo,
    #[serde(rename = "oracle")]
    Oracle,
    #[serde(rename = "dameng")]
    Dameng,
    #[serde(rename = "sqlserver")]
    SqlServer,
}

impl Variant {
    pub fn family(self) -> Family {
        match self {
            Variant::MySql | Variant::MariaDb | Variant::TiDb | Variant::OceanBase | Variant::PolarDb => {
                Family::MySql
            }
            Variant::Postgres
            | Variant::Kingbase
            | Variant::OpenGauss
            | Variant::GaussDb
            | Variant::HighGo => Family::Postgres,
            Variant::Oracle => Family::Oracle,
            Variant::Dameng => Family::Dameng,
            Variant::SqlServer => Family::SqlServer,
        }
    }

    /// 给用户看的名称；国产库的中文名随界面语言变化。
    pub fn display_name(self) -> Msg {
        match self {
            Variant::MySql => Msg::raw("MySQL"),
            Variant::MariaDb => Msg::raw("MariaDB"),
            Variant::TiDb => Msg::raw("TiDB"),
            Variant::OceanBase => msg!("variant.oceanbase"),
            Variant::PolarDb => msg!("variant.polardb"),
            Variant::Postgres => Msg::raw("PostgreSQL"),
            Variant::Kingbase => msg!("variant.kingbase"),
            Variant::OpenGauss => Msg::raw("openGauss"),
            Variant::GaussDb => Msg::raw("GaussDB"),
            Variant::HighGo => msg!("variant.highgo"),
            Variant::Oracle => Msg::raw("Oracle"),
            Variant::Dameng => msg!("variant.dameng"),
            Variant::SqlServer => Msg::raw("SQL Server"),
        }
    }

    /// 没有服务器信息时使用的基础变体。
    pub fn base(driver: DriverId) -> Variant {
        match driver {
            DriverId::Mysql => Variant::MySql,
            DriverId::Postgres => Variant::Postgres,
            DriverId::Oracle => Variant::Oracle,
            DriverId::Dameng => Variant::Dameng,
            DriverId::SqlServer => Variant::SqlServer,
        }
    }

    /// 根据驱动和服务器返回的版本字符串识别具体产品，识别不出时退回基础变体。
    /// 注意：PolarDB-MySQL、GoldenDB 等的 `VERSION()` 与 MySQL 无法区分，会识别为 MySQL；
    /// PolarDB 再用 `Dialect::refine` 识别。
    pub fn detect(driver: DriverId, info: &ServerInfo) -> Variant {
        let v = info.version.to_ascii_lowercase();
        match driver {
            DriverId::Mysql if v.contains("mariadb") => Variant::MariaDb,
            DriverId::Mysql if v.contains("tidb") => Variant::TiDb,
            DriverId::Mysql if v.contains("oceanbase") => Variant::OceanBase,
            DriverId::Postgres if v.contains("kingbase") => Variant::Kingbase,
            DriverId::Postgres if v.contains("opengauss") => Variant::OpenGauss,
            DriverId::Postgres if v.contains("gaussdb") => Variant::GaussDb,
            DriverId::Postgres if v.contains("highgo") => Variant::HighGo,
            _ => Variant::base(driver),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(product: &str, version: &str) -> ServerInfo {
        ServerInfo { product: product.into(), version: version.into() }
    }

    #[test]
    fn detects_mysql_family_variants() {
        assert_eq!(Variant::detect(DriverId::Mysql, &info("MySQL", "8.4.3")), Variant::MySql);
        assert_eq!(
            Variant::detect(DriverId::Mysql, &info("MySQL", "11.4.4-MariaDB-ubu2404")),
            Variant::MariaDb
        );
        assert_eq!(Variant::detect(DriverId::Mysql, &info("MySQL", "8.0.11-TiDB-v7.5.1")), Variant::TiDb);
        assert_eq!(
            Variant::detect(DriverId::Mysql, &info("MySQL", "5.7.25-OceanBase_CE-v4.2.1.2")),
            Variant::OceanBase
        );
    }

    #[test]
    fn detects_postgres_family_variants() {
        let pg = "PostgreSQL 17.2 (Debian 17.2-1.pgdg120+1) on x86_64-pc-linux-gnu";
        assert_eq!(Variant::detect(DriverId::Postgres, &info("PostgreSQL", pg)), Variant::Postgres);
        let kb = "KingbaseES V008R006C008B0014 on x86_64-pc-linux-gnu, compiled by gcc";
        assert_eq!(Variant::detect(DriverId::Postgres, &info("PostgreSQL", kb)), Variant::Kingbase);
        let og = "(openGauss 5.0.1 build 33b035fd) compiled at 2023-12-15 on x86_64-unknown-linux-gnu";
        assert_eq!(Variant::detect(DriverId::Postgres, &info("PostgreSQL", og)), Variant::OpenGauss);
        let gs = "gaussdb (GaussDB Kernel 505.1.0 build 2c0ba3b1) compiled at 2024-03-01";
        assert_eq!(Variant::detect(DriverId::Postgres, &info("PostgreSQL", gs)), Variant::GaussDb);
        let hg = "HighGo Security Enterprise Edition Release 4.5.8 on x86_64";
        assert_eq!(Variant::detect(DriverId::Postgres, &info("PostgreSQL", hg)), Variant::HighGo);
    }

    #[test]
    fn oracle_and_dameng_and_families() {
        let ora = "Oracle AI Database 26ai Free Release 23.26.0.0.0 - Develop, Learn, and Run for Free";
        assert_eq!(Variant::detect(DriverId::Oracle, &info("Oracle", ora)), Variant::Oracle);
        assert_eq!(
            Variant::detect(DriverId::Dameng, &info("DM", "DM Database Server 64 V8")),
            Variant::Dameng
        );
        assert_eq!(Variant::TiDb.family(), Family::MySql);
        assert_eq!(Variant::Kingbase.family(), Family::Postgres);
        assert_eq!(Variant::Dameng.family(), Family::Dameng);
        assert_eq!(Variant::base(DriverId::Postgres), Variant::Postgres);
        assert_eq!(Variant::Kingbase.display_name().render(dblens_core::Lang::ZhCn), "人大金仓 KingbaseES");
        assert_eq!(Variant::Kingbase.display_name().render(dblens_core::Lang::En), "KingbaseES");
    }

    #[test]
    fn detection_is_gated_by_driver_and_falls_back() {
        assert_eq!(Variant::detect(DriverId::Postgres, &info("MySQL", "11.4.4-MariaDB")), Variant::Postgres);
        assert_eq!(Variant::detect(DriverId::Mysql, &info("MySQL", "some unknown")), Variant::MySql);
        assert_eq!(serde_json::to_string(&Variant::OpenGauss).unwrap(), "\"opengauss\"");
        assert_eq!(serde_json::to_string(&Family::MySql).unwrap(), "\"mysql\"");
    }

    #[test]
    fn script_terminators() {
        assert_eq!(Family::MySql.terminate("SELECT 1"), "SELECT 1;");
        assert_eq!(
            Family::Oracle.terminate(
                "SELECT 1 FROM DUAL;
"
            ),
            "SELECT 1 FROM DUAL;"
        );
        assert_eq!(
            Family::SqlServer.terminate("CREATE VIEW v AS SELECT 1"),
            "CREATE VIEW v AS SELECT 1
GO"
        );
    }
}
