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

/// TLS 选项：`require` 只加密不校验证书；`verify` 校验证书链和主机名，`tls_ca` 为额外信任的 CA 证书。
/// 取消查询时另开的连接沿用同一份选项。TLS 由系统实现（Windows 的 SChannel、macOS 的 Security.framework、
/// Linux 上静态编入的 OpenSSL），支持 MySQL 5.7 常见的 DHE、RSA 密钥交换。
fn ssl_opts(mode: &TlsMode) -> Option<SslOpts> {
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
        Ok(Box::new(MySqlConnection { conn, opts, lob_limit: dblens_core::DEFAULT_LOB_PREVIEW_BYTES }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tls_options_by_mode() {
        assert!(ssl_opts(&TlsMode::Disable).is_none());
        let require = ssl_opts(&TlsMode::Require).unwrap();
        assert!(require.accept_invalid_certs() && require.skip_domain_validation());
        let verify = ssl_opts(&TlsMode::Verify { ca_file: None }).unwrap();
        assert!(!verify.accept_invalid_certs() && !verify.skip_domain_validation());
    }
}
