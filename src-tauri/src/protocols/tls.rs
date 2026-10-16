//! TLS 인증서 검증 (FTPS, 이후 RDP).
//!
//! 1. OS 신뢰 저장소(rustls-platform-verifier)로 검증되면 그대로 신뢰합니다.
//! 2. 검증에 실패해도 사용자가 이전에 승인해 고정한 지문과 같으면 신뢰합니다.
//! 3. 그 밖에는 연결을 거부하고, 본 인증서의 지문과 실패 이유를 기록합니다.
//!    세션은 이 기록으로 사용자에게 승인을 묻거나(처음 보는 인증서) 차단합니다(고정과 다름).
//!
//! 검증을 끄는 경로는 없습니다. rustls 검증 콜백은 동기 함수라 사용자 응답을 기다릴 수 없으므로,
//! 승인 후에는 고정 지문을 넣어 다시 연결합니다.

use std::fmt::Write as _;
use std::sync::{Arc, Mutex, PoisonError};

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{CertificateError, ClientConfig, DigitallySignedStruct, SignatureScheme};

/// 인증서(DER)의 SHA-256 지문. `AB:CD:…` 형식(브라우저 인증서 창과 같은 표기)
pub fn cert_fingerprint(der: &[u8]) -> String {
    let digest = ring::digest::digest(&ring::digest::SHA256, der);
    let mut out = String::with_capacity(95);
    for (i, b) in digest.as_ref().iter().enumerate() {
        if i > 0 {
            out.push(':');
        }
        let _ = write!(out, "{b:02X}");
    }
    out
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CertTrust {
    /// OS 신뢰 저장소의 인증 기관으로 검증됨
    Ca,
    /// 사용자가 고정한 지문과 일치
    Pinned,
}

/// 마지막 핸드셰이크에서 본 서버 인증서
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Observed {
    pub fingerprint: Option<String>,
    pub trust: Option<CertTrust>,
    /// 신뢰하지 못한 이유 (`unknownIssuer`, `badName`, `expired`, `invalid`)
    pub problem: Option<String>,
}

fn describe(e: &rustls::Error) -> String {
    match e {
        rustls::Error::InvalidCertificate(c) => match c {
            CertificateError::UnknownIssuer => "unknownIssuer",
            CertificateError::NotValidForName | CertificateError::NotValidForNameContext { .. } => {
                "badName"
            }
            CertificateError::Expired | CertificateError::ExpiredContext { .. } => "expired",
            _ => "invalid",
        },
        _ => "invalid",
    }
    .into()
}

#[derive(Debug)]
pub struct PinningVerifier {
    inner: Arc<dyn ServerCertVerifier>,
    pinned: Option<String>,
    observed: Arc<Mutex<Observed>>,
}

impl PinningVerifier {
    pub fn new(inner: Arc<dyn ServerCertVerifier>, pinned: Option<String>) -> Self {
        Self {
            inner,
            pinned: pinned.map(|p| p.to_ascii_uppercase()),
            observed: Arc::new(Mutex::new(Observed::default())),
        }
    }

    pub fn observed(&self) -> Observed {
        self.observed
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn record(&self, observed: Observed) {
        *self.observed.lock().unwrap_or_else(PoisonError::into_inner) = observed;
    }
}

impl ServerCertVerifier for PinningVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        server_name: &ServerName<'_>,
        ocsp_response: &[u8],
        now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        let fingerprint = cert_fingerprint(end_entity);
        match self.inner.verify_server_cert(
            end_entity,
            intermediates,
            server_name,
            ocsp_response,
            now,
        ) {
            Ok(v) => {
                self.record(Observed {
                    fingerprint: Some(fingerprint),
                    trust: Some(CertTrust::Ca),
                    problem: None,
                });
                Ok(v)
            }
            Err(e) if self.pinned.as_deref() == Some(fingerprint.as_str()) => {
                self.record(Observed {
                    fingerprint: Some(fingerprint),
                    trust: Some(CertTrust::Pinned),
                    problem: Some(describe(&e)),
                });
                Ok(ServerCertVerified::assertion())
            }
            Err(e) => {
                self.record(Observed {
                    fingerprint: Some(fingerprint),
                    trust: None,
                    problem: Some(describe(&e)),
                });
                Err(e)
            }
        }
    }

    // 핸드셰이크 서명 검증은 고정된 인증서에도 항상 수행합니다.
    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        self.inner.verify_tls12_signature(message, cert, dss)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        self.inner.verify_tls13_signature(message, cert, dss)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.inner.supported_verify_schemes()
    }
}

/// OS 신뢰 저장소 + 지문 고정 검증기로 rustls 클라이언트 설정을 만듭니다.
pub fn client_config(
    pinned: Option<String>,
) -> Result<(Arc<ClientConfig>, Arc<PinningVerifier>), rustls::Error> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let platform = rustls_platform_verifier::Verifier::new(provider.clone())?;
    let verifier = Arc::new(PinningVerifier::new(Arc::new(platform), pinned));
    let config = ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()?
        .dangerous()
        .with_custom_certificate_verifier(verifier.clone())
        .with_no_client_auth();
    Ok((Arc::new(config), verifier))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 결과가 정해진 가짜 검증기
    #[derive(Debug)]
    struct Fixed(bool);
    impl ServerCertVerifier for Fixed {
        fn verify_server_cert(
            &self,
            _: &CertificateDer<'_>,
            _: &[CertificateDer<'_>],
            _: &ServerName<'_>,
            _: &[u8],
            _: UnixTime,
        ) -> Result<ServerCertVerified, rustls::Error> {
            if self.0 {
                Ok(ServerCertVerified::assertion())
            } else {
                Err(rustls::Error::InvalidCertificate(
                    CertificateError::UnknownIssuer,
                ))
            }
        }
        fn verify_tls12_signature(
            &self,
            _: &[u8],
            _: &CertificateDer<'_>,
            _: &DigitallySignedStruct,
        ) -> Result<HandshakeSignatureValid, rustls::Error> {
            Ok(HandshakeSignatureValid::assertion())
        }
        fn verify_tls13_signature(
            &self,
            _: &[u8],
            _: &CertificateDer<'_>,
            _: &DigitallySignedStruct,
        ) -> Result<HandshakeSignatureValid, rustls::Error> {
            Ok(HandshakeSignatureValid::assertion())
        }
        fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
            vec![]
        }
    }

    fn verify(v: &PinningVerifier, der: &[u8]) -> bool {
        let name = ServerName::try_from("ftp.example").unwrap();
        v.verify_server_cert(
            &CertificateDer::from(der.to_vec()),
            &[],
            &name,
            &[],
            UnixTime::now(),
        )
        .is_ok()
    }

    #[test]
    fn fingerprint_format() {
        let fp = cert_fingerprint(b"abc");
        assert_eq!(fp.len(), 95);
        assert!(fp.starts_with("BA:78:16:BF"));
    }

    #[test]
    fn ca_trusted_certificate_is_accepted() {
        let v = PinningVerifier::new(Arc::new(Fixed(true)), None);
        assert!(verify(&v, b"cert"));
        assert_eq!(v.observed().trust, Some(CertTrust::Ca));
    }

    #[test]
    fn untrusted_certificate_is_rejected_and_recorded() {
        let v = PinningVerifier::new(Arc::new(Fixed(false)), None);
        assert!(!verify(&v, b"self-signed"));
        let o = v.observed();
        assert_eq!(o.trust, None);
        assert_eq!(o.problem.as_deref(), Some("unknownIssuer"));
        assert_eq!(o.fingerprint, Some(cert_fingerprint(b"self-signed")));
    }

    #[test]
    fn pinned_fingerprint_is_accepted_only_when_equal() {
        let pin = cert_fingerprint(b"self-signed").to_ascii_lowercase();
        let v = PinningVerifier::new(Arc::new(Fixed(false)), Some(pin));
        assert!(verify(&v, b"self-signed"));
        assert_eq!(v.observed().trust, Some(CertTrust::Pinned));
        assert!(!verify(&v, b"other-cert"));
        assert_eq!(v.observed().trust, None);
    }

    #[test]
    fn builds_client_config() {
        assert!(client_config(None).is_ok());
    }
}
