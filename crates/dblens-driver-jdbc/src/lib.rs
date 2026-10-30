//! JDBC 桥接驱动：Rust 端经 stdin/stdout 与 Java 进程通信，由官方 JDBC 驱动连接数据库。目前只有 GBase 8s。

mod conn;
mod error;
mod frame;
mod gbase;
mod process;

pub use gbase::GbaseDriver;
