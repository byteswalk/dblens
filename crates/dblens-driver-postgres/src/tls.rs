//! TLS 连接器（rustls）。`require` 只加密不校验证书；`verify` 用内置的公共根证书和 `tls_ca` 指定的证书校验。

use std::sync::Arc;

use dblens_core::{Error, Result, msg};
use dblens_driver::TlsMode;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::WebPkiSupportedAlgorithms;
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{ClientConfig, DigitallySignedStruct, RootCertStore, SignatureScheme};
use tokio_postgres_rustls::MakeRustlsConnect;

/// 接受任何证书的校验器，只用于 `tls=require`：仍然校验握手签名，只是不校验证书链和主机名。
#[derive(Debug)]
struct AcceptAnyCert(WebPkiSupportedAlgorithms);

impl ServerCertVerifier for AcceptAnyCert {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp: &[u8],
        _now: UnixTime,
    ) -> std::result::Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(message, cert, dss, &self.0)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(message, cert, dss, &self.0)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.0.supported_schemes()
    }
}

fn tls_err(e: impl std::fmt::Display) -> Error {
    Error::connect(msg!("tls.config_error", error = e))
}

/// 按 TLS 方式生成连接器；`disable` 时返回 None。
pub(crate) fn connector(mode: &TlsMode) -> Result<Option<MakeRustlsConnect>> {
    let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
    let builder = ClientConfig::builder_with_provider(provider.clone())
        .with_safe_default_protocol_versions()
        .map_err(tls_err)?;
    let config = match mode {
        TlsMode::Disable => return Ok(None),
        TlsMode::Require => builder
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(AcceptAnyCert(
                provider.signature_verification_algorithms,
            )))
            .with_no_client_auth(),
        TlsMode::Verify { ca_file } => {
            let mut roots = RootCertStore::empty();
            roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
            if let Some(path) = ca_file {
                let certs = CertificateDer::pem_file_iter(path).map_err(|e| {
                    Error::invalid(msg!("tls.ca_unreadable", path = path.display(), error = e))
                })?;
                for cert in certs {
                    let cert = cert.map_err(|e| {
                        Error::invalid(msg!("tls.ca_malformed", path = path.display(), error = e))
                    })?;
                    roots.add(cert).map_err(tls_err)?;
                }
            }
            builder.with_root_certificates(roots).with_no_client_auth()
        }
    };
    Ok(Some(MakeRustlsConnect::new(config)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_connectors() {
        assert!(connector(&TlsMode::Disable).unwrap().is_none());
        assert!(connector(&TlsMode::Require).unwrap().is_some());
        assert!(connector(&TlsMode::Verify { ca_file: None }).unwrap().is_some());
        let missing = TlsMode::Verify { ca_file: Some("no/such/ca.pem".into()) };
        assert!(matches!(connector(&missing), Err(Error::Invalid { .. })));
    }
}
