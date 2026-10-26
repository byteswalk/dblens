//! MySQL 系驱动（MySQL、MariaDB，以及兼容 MySQL 协议的 TiDB、OceanBase、PolarDB 等）。

mod conn;
mod error;
mod exec;
mod value;

use async_trait::async_trait;
use dblens_core::{Error, Result, msg};
use dblens_driver::TlsMode;
use dblens_driver::{Capabilities, ConnectParams, Connection, Driver, DriverId};

use mysql_async::{Conn, Opts, OptsBuilder, SslOpts};

pub use conn::MySqlConnection;

use crate::error::map_err;

pub struct MySqlDriver;

/// mysql_async 使用 rustls 的进程级默认加密实现。同一个程序里同时编进了 aws-lc-rs 和 ring 时（桌面版的更新插件用 ring），
/// rustls 无法自动选择，建 TLS 连接时会直接 panic。所以用 TLS 之前先装上 aws-lc-rs；已经有默认实现时保持不变。
fn ensure_crypto_provider() {
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
}

/// TLS 选项：`require` 只加密不校验证书；`verify` 校验证书链和主机名，`tls_ca` 为额外信任的 CA 证书。
/// 取消查询时另开的连接沿用同一份选项。
fn ssl_opts(mode: &TlsMode) -> Option<SslOpts> {
    if !matches!(mode, TlsMode::Disable) {
        ensure_crypto_provider();
    }
    match mode {
        TlsMode::Disable => None,
        TlsMode::Require => Some(
            SslOpts::default()
                .with_danger_accept_invalid_certs(true)
                .with_danger_skip_domain_validation(true),
        ),
        TlsMode::Verify { ca_file } => Some(match ca_file {
            Some(ca) => SslOpts::default().with_root_certs(vec![ca.clone().into()]),
            None => SslOpts::default(),
        }),
    }
}

#[async_trait]
impl Driver for MySqlDriver {
    fn id(&self) -> DriverId {
        DriverId::Mysql
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            catalogs: false,
            schemas: true,
            transactional_ddl: false,
            cancel: true,
            explain: true,
            multiple_result_sets: true,
        }
    }

    async fn connect(&self, p: &ConnectParams) -> Result<Box<dyn Connection>> {
        let opts: Opts = OptsBuilder::default()
            .ip_or_hostname(p.host.clone())
            .tcp_port(p.port)
            .user(Some(p.user.clone()))
            .pass(Some(p.password.clone()))
            .db_name(p.database.clone())
            .ssl_opts(ssl_opts(&p.tls_mode()?))
            .into();
        let conn = tokio::time::timeout(p.connect_timeout, Conn::new(opts.clone()))
            .await
            .map_err(|_| {
                Error::connect(msg!("driver.connect_timeout", seconds = p.connect_timeout.as_secs()))
            })?
            .map_err(map_err)?;
        Ok(Box::new(MySqlConnection { conn, opts }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tls_installs_a_default_crypto_provider() {
        assert!(ssl_opts(&TlsMode::Disable).is_none());
        assert!(ssl_opts(&TlsMode::Require).is_some());
        assert!(rustls::crypto::CryptoProvider::get_default().is_some(), "建 TLS 连接前已有进程级默认实现");
    }
}
