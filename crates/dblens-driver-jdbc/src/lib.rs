//! JDBC 桥接驱动：Rust 端经 stdin/stdout 与 Java 进程通信，由官方 JDBC 驱动连接数据库。GBase 8s 只走这条路；达梦、Oracle 在连接选项 `client=jdbc` 时走这条路，不需要原生客户端。

mod conn;
mod drivers;
mod error;
mod frame;
mod position;
mod process;

pub use drivers::{DamengJdbcDriver, GbaseDriver, OracleJdbcDriver};
