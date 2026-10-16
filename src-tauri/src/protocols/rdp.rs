//! RDP 원격 데스크톱 (IronRDP).
//!
//! 연결 순서: TCP → X.224 협상 → TLS(인증서 검증/고정) → NLA(CredSSP) → 세션 활성화.
//! TLS는 FTPS와 같은 검증기([`super::tls`])를 써서 OS 신뢰 저장소로 확인하고, 실패하면 사용자가
//! 고정한 지문과 비교합니다. 검증을 끄는 경로는 없습니다. 보안 없는 표준 RDP 보안(RC4)은 협상하지 않습니다.
//!
//! 세션이 시작되면 서버가 보낸 그래픽을 프레임 버퍼에 그리고, 바뀐 영역만 모아 일정 간격으로
//! 웹뷰에 보냅니다([`frame`]). 웹뷰가 그리기를 마치면 확인(ack)을 보내며, 확인되지 않은 메시지가
//! 쌓이면 변경 영역만 모아 두었다가 다음에 한 번에 보냅니다.

pub mod clipboard;
pub mod frame;
pub mod input;

use std::sync::Arc;
use std::time::Duration;

use connector::sspi::generator::NetworkRequest;
use ironrdp::cliprdr::pdu::{ClipboardFormat, ClipboardFormatId, FormatDataResponse};
use ironrdp::cliprdr::{CliprdrClient, CliprdrSvcMessages};
use ironrdp::connector::connection_activation::ConnectionActivationState;
use ironrdp::connector::{
    self, ClientConnector, ClientConnectorState, ConnectionResult, ConnectorError,
    ConnectorErrorKind, Credentials, DesktopSize, ServerName,
};
use ironrdp::displaycontrol::client::DisplayControlClient;
use ironrdp::displaycontrol::pdu::MonitorLayoutEntry;
use ironrdp::dvc::DrdynvcClient;
use ironrdp::graphics::image_processing::PixelFormat;
use ironrdp::pdu::gcc::{ConnectionType, KeyboardType};
use ironrdp::pdu::nego::SecurityProtocol;
use ironrdp::pdu::rdp::capability_sets::{client_codecs_capabilities, MajorPlatformType};
use ironrdp::pdu::rdp::client_info::{PerformanceFlags, TimezoneInfo};
use ironrdp::session::image::DecodedImage;
use ironrdp::session::{ActiveStage, ActiveStageBuilder, ActiveStageOutput, SessionError};
use ironrdp_tokio::{FramedWrite as _, NetworkClient};
use secrecy::{ExposeSecret as _, SecretString};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use x509_cert::der::Decode as _;

use self::clipboard::{ChannelBackend, ClipEvent, LocalClipboard};
use self::frame::{DirtyRegion, Rect};
use self::input::InputEvent;
use super::tls::{cert_fingerprint, client_config, CertTrust, Observed};

/// TCP 연결 대기 시간
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
/// TLS·NLA·활성화까지 전체 접속 대기 시간
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(30);
/// 화면 메시지를 모아 보내는 간격 (최대 약 60fps)
const FRAME_INTERVAL: Duration = Duration::from_millis(16);
/// 웹뷰가 아직 그리지 않은 화면 메시지 수 상한
const MAX_FRAMES_IN_FLIGHT: u32 = 2;
/// 이 시간 동안 확인이 없으면 유실로 보고 다시 보냅니다.
const ACK_TIMEOUT: Duration = Duration::from_secs(1);
/// 원격 화면 크기 범위 ([MS-RDPEDISP] 2.2.2.2.1)
const MIN_DIM: u16 = 200;
const MAX_DIM: u16 = 8192;

#[derive(Debug, Clone)]
pub struct RdpTarget {
    pub host: String,
    pub port: u16,
    /// `DOMAIN\user` 또는 `user@domain` 형식도 받습니다.
    pub username: String,
}

impl RdpTarget {
    /// 사용자명에서 도메인을 분리합니다. `user@domain`은 UPN 그대로 서버에 넘깁니다.
    fn credentials(&self) -> (String, Option<String>) {
        match self.username.split_once('\\') {
            Some((domain, user)) if !domain.is_empty() => {
                (user.to_owned(), Some(domain.to_owned()))
            }
            _ => (self.username.clone(), None),
        }
    }

    /// 대괄호로 감싼 IPv6 주소를 벗깁니다.
    fn bare_host(&self) -> &str {
        self.host
            .strip_prefix('[')
            .and_then(|h| h.strip_suffix(']'))
            .unwrap_or(&self.host)
    }
}

/// 원격 화면 크기와 배율(%) 요청
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DisplaySize {
    pub width: u16,
    pub height: u16,
    pub scale: u32,
}

impl DisplaySize {
    /// 프로토콜 범위로 맞춥니다 (너비는 짝수).
    pub fn normalized(self) -> Self {
        let width = self.width.clamp(MIN_DIM, MAX_DIM) & !1;
        Self {
            width,
            height: self.height.clamp(MIN_DIM, MAX_DIM),
            scale: self.scale.clamp(100, 500),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum RdpError {
    #[error("could not connect: {0}")]
    Connect(String),
    #[error("connection timed out")]
    Timeout,
    /// 사용자명/비밀번호 오류 또는 계정 상태(잠김, 만료 등) 문제. 원인을 알면 함께 담습니다.
    #[error("authentication failed{}", .0.map(|r| format!(" ({r})")).unwrap_or_default())]
    AuthFailed(Option<&'static str>),
    /// OS 신뢰 저장소로 검증되지 않았고 고정된 지문과도 다름 (세션이 승인을 묻거나 차단)
    #[error("server certificate is not trusted ({problem})")]
    CertUntrusted {
        fingerprint: String,
        problem: String,
    },
    #[error("server certificate changed (pinned {expected}, received {actual})")]
    CertChanged { expected: String, actual: String },
    #[error("server certificate was not accepted")]
    CertRejected,
    #[error("TLS: {0}")]
    Tls(String),
    /// 서버가 TLS/NLA를 지원하지 않는 등 보안 협상 실패
    #[error("security negotiation failed: {0}")]
    Negotiation(String),
    #[error("RDP protocol error: {0}")]
    Protocol(String),
    #[error("connection lost")]
    ConnectionLost,
}

impl RdpError {
    pub fn code(&self) -> &'static str {
        match self {
            RdpError::Connect(_) => "connect",
            RdpError::Timeout => "timeout",
            RdpError::AuthFailed(_) => "authFailed",
            RdpError::CertUntrusted { .. } => "certUntrusted",
            RdpError::CertChanged { .. } => "certChanged",
            RdpError::CertRejected => "certRejected",
            RdpError::Tls(_) => "tls",
            RdpError::Negotiation(_) => "rdpNegotiation",
            RdpError::Protocol(_) => "rdpProtocol",
            RdpError::ConnectionLost => "connectionLost",
        }
    }
}

/// 서버가 CredSSP로 돌려준 NTSTATUS 중 인증 실패에 해당하는 것
fn auth_failure(status: connector::sspi::credssp::NStatusCode) -> Option<Option<&'static str>> {
    use connector::sspi::credssp::NStatusCode as S;
    Some(match status {
        S::LOGON_FAILURE | S::WRONG_PASSWORD | S::NO_SUCH_USER | S::INVALID_ACCOUNT_NAME => None,
        S::ACCOUNT_LOCKED_OUT => Some("account locked out"),
        S::ACCOUNT_DISABLED => Some("account disabled"),
        S::PASSWORD_EXPIRED | S::PASSWORD_MUST_CHANGE => Some("password expired"),
        S::ACCOUNT_RESTRICTION | S::INVALID_LOGON_HOURS | S::INVALID_WORKSTATION => {
            Some("account restriction")
        }
        S::LOGON_NOT_GRANTED | S::LOGON_TYPE_NOT_GRANTED => Some("remote logon not allowed"),
        S(code) if is_sspi_logon_denied(code) => None,
        _ => return None,
    })
}

/// sspi-rs 기반 서버(IronRDP 서버 등)는 SSPI 에러를 0xC007xxxx로 바꿔 보냅니다 (SEC_E_LOGON_DENIED 등).
fn is_sspi_logon_denied(code: u32) -> bool {
    use connector::sspi::ErrorKind as K;
    code & 0xFFFF_0000 == 0xC007_0000
        && [K::LogonDenied, K::UnknownCredentials, K::NoCredentials]
            .iter()
            .any(|k| (*k as u32) & 0xFFFF == code & 0xFFFF)
}

fn connector_error(e: ConnectorError) -> RdpError {
    match e.kind() {
        ConnectorErrorKind::AccessDenied => RdpError::AuthFailed(None),
        ConnectorErrorKind::Credssp(err) => {
            use connector::sspi::ErrorKind as K;
            if let Some(reason) = err.nstatus.and_then(auth_failure) {
                return RdpError::AuthFailed(reason);
            }
            match err.error_type {
                K::LogonDenied
                | K::NoCredentials
                | K::UnknownCredentials
                | K::IncompleteCredentials
                | K::WrongPrincipalName => RdpError::AuthFailed(None),
                _ => RdpError::Protocol(format!("CredSSP: {}", err.description)),
            }
        }
        ConnectorErrorKind::Negotiation(failure) => RdpError::Negotiation(failure.to_string()),
        _ if is_io_error(&e) => RdpError::ConnectionLost,
        _ => RdpError::Protocol(e.to_string()),
    }
}

fn is_io_error(e: &(dyn std::error::Error + 'static)) -> bool {
    let mut source = e.source();
    while let Some(s) = source {
        if s.is::<std::io::Error>() {
            return true;
        }
        source = s.source();
    }
    false
}

fn session_error(e: SessionError) -> RdpError {
    if is_io_error(&e) {
        RdpError::ConnectionLost
    } else {
        RdpError::Protocol(e.to_string())
    }
}

/// 접속 결과 (상태 표시줄용)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RdpConnectionInfo {
    pub fingerprint: String,
    pub trust: CertTrust,
    /// NLA(CredSSP)로 인증했는지. 거짓이면 TLS 뒤 원격 로그인 화면에서 인증
    pub nla: bool,
    pub tls_version: String,
}

/// 세션 중 웹뷰에서 오는 명령
#[derive(Debug)]
pub enum RdpCommand {
    Input(Vec<InputEvent>),
    Resize(DisplaySize),
    /// 화면 메시지 하나를 그렸음
    FrameAck,
    /// 로컬 클립보드가 바뀌었을 수 있음 (창/탭 포커스)
    ClipboardSync,
    SetClipboard(bool),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipboardDirection {
    ToRemote,
    ToLocal,
}

/// 세션 출력 대상
pub trait RdpOutput: Send + Sync {
    /// [`frame`] 형식의 바이너리 메시지
    fn binary(&self, data: Vec<u8>);
    fn clipboard(&self, direction: ClipboardDirection, chars: usize);
}

/// Kerberos를 쓰지 않으므로 KDC 요청은 항상 실패시킵니다. (NTLM으로 인증)
struct NoKdc;

impl NetworkClient for NoKdc {
    async fn send(&mut self, _: &NetworkRequest) -> connector::ConnectorResult<Vec<u8>> {
        Err(connector::general_err!("Kerberos is not supported"))
    }
}

type TlsStream = tokio_rustls::client::TlsStream<TcpStream>;
type Framed<S> = ironrdp_tokio::TokioFramed<S>;

/// 활성화까지 마친 연결
pub struct RdpConnection {
    framed: Framed<TlsStream>,
    result: ConnectionResult,
    info: RdpConnectionInfo,
    clip_events: Option<mpsc::UnboundedReceiver<ClipEvent>>,
}

fn client_name() -> String {
    // 서버에 보이는 클라이언트 이름 (15자 제한). 호스트 이름을 보내지 않습니다.
    "Rivet".to_owned()
}

fn connector_config(
    target: &RdpTarget,
    password: &SecretString,
    size: DisplaySize,
) -> connector::Config {
    let (username, domain) = target.credentials();
    connector::Config {
        desktop_size: DesktopSize {
            width: size.width,
            height: size.height,
        },
        monitor_layout: None,
        desktop_scale_factor: size.scale,
        // TLS만 쓰는 서버(NLA 미지원)도 접속하되, 인증서는 똑같이 검증합니다.
        enable_tls: true,
        enable_credssp: true,
        enable_standard_rdp_security: false,
        credentials: Credentials::UsernamePassword {
            username,
            password: password.expose_secret().to_owned(),
        },
        domain,
        client_build: 0,
        client_name: client_name(),
        keyboard_type: KeyboardType::IBM_ENHANCED,
        keyboard_subtype: 0,
        keyboard_functional_keys_count: 12,
        // 0이면 서버 기본 배열을 씁니다.
        keyboard_layout: 0,
        connection_type: ConnectionType::Lan,
        ime_file_name: String::new(),
        bitmap: client_codecs_capabilities(&[])
            .ok()
            .map(|codecs| connector::BitmapConfig {
                lossy_compression: true,
                color_depth: 32,
                codecs,
            }),
        dig_product_id: String::new(),
        client_dir: "C:\\Windows\\System32\\mstscax.dll".to_owned(),
        alternate_shell: String::new(),
        work_dir: String::new(),
        remote_application_mode: false,
        rail_support_level: ironrdp::pdu::rdp::capability_sets::RailSupportLevel::empty(),
        #[cfg(target_os = "windows")]
        platform: MajorPlatformType::WINDOWS,
        #[cfg(target_os = "macos")]
        platform: MajorPlatformType::MACINTOSH,
        #[cfg(not(any(target_os = "windows", target_os = "macos")))]
        platform: MajorPlatformType::UNIX,
        hardware_id: None,
        request_data: None,
        autologon: false,
        enable_audio_playback: false,
        enable_audio_capture: false,
        performance_flags: PerformanceFlags::default(),
        license_cache: None,
        timezone_info: TimezoneInfo::default(),
        compression_type: None,
        // 포인터 이미지는 웹뷰가 CSS 커서로 그려 지연 없이 움직이게 합니다.
        enable_server_pointer: true,
        pointer_software_rendering: false,
        multitransport_flags: None,
        support_dyn_vc_gfx_protocol: false,
    }
}

/// 인증서 공개 키 (CredSSP가 TLS 채널과 묶을 때 사용)
pub(crate) fn public_key(der: &[u8]) -> Result<Vec<u8>, RdpError> {
    let cert = x509_cert::Certificate::from_der(der)
        .map_err(|e| RdpError::Tls(format!("invalid server certificate: {e}")))?;
    cert.tbs_certificate
        .subject_public_key_info
        .subject_public_key
        .as_bytes()
        .map(<[u8]>::to_vec)
        .ok_or_else(|| RdpError::Tls("invalid server public key".into()))
}

fn tls_version(conn: &rustls::ClientConnection) -> String {
    match conn.protocol_version() {
        Some(rustls::ProtocolVersion::TLSv1_3) => "TLS 1.3".into(),
        Some(rustls::ProtocolVersion::TLSv1_2) => "TLS 1.2".into(),
        Some(v) => format!("{v:?}"),
        None => "TLS".into(),
    }
}

impl RdpConnection {
    /// 접속하고 인증과 세션 활성화까지 마칩니다.
    ///
    /// `pinned`는 사용자가 이전에 승인한 인증서 지문입니다. 인증서를 신뢰할 수 없으면
    /// [`RdpError::CertUntrusted`]를 돌려주어 세션이 사용자에게 묻게 합니다.
    pub async fn connect(
        target: &RdpTarget,
        password: &SecretString,
        pinned: Option<String>,
        size: DisplaySize,
        clipboard: bool,
    ) -> Result<Self, RdpError> {
        tokio::time::timeout(
            HANDSHAKE_TIMEOUT,
            Self::connect_inner(target, password, pinned, size.normalized(), clipboard),
        )
        .await
        .map_err(|_| RdpError::Timeout)?
    }

    async fn connect_inner(
        target: &RdpTarget,
        password: &SecretString,
        pinned: Option<String>,
        size: DisplaySize,
        clipboard: bool,
    ) -> Result<Self, RdpError> {
        let host = target.bare_host();
        let stream = tokio::time::timeout(CONNECT_TIMEOUT, TcpStream::connect((host, target.port)))
            .await
            .map_err(|_| RdpError::Timeout)?
            .map_err(|e| RdpError::Connect(e.to_string()))?;
        let _ = stream.set_nodelay(true);
        let client_addr = stream
            .local_addr()
            .map_err(|e| RdpError::Connect(e.to_string()))?;

        let drdynvc = DrdynvcClient::new()
            .with_dynamic_channel(DisplayControlClient::new(|_| Ok(Vec::new())));
        let mut connector =
            ClientConnector::new(connector_config(target, password, size), client_addr)
                .with_static_channel(drdynvc);
        let clip_events = if clipboard {
            let (tx, rx) = mpsc::unbounded_channel();
            connector.attach_static_channel(CliprdrClient::new(Box::new(ChannelBackend::new(tx))));
            Some(rx)
        } else {
            None
        };

        let mut framed = Framed::new(stream);
        let should_upgrade = ironrdp_tokio::connect_begin(&mut framed, &mut connector)
            .await
            .map_err(connector_error)?;
        let nla = match &connector.state {
            ClientConnectorState::EnhancedSecurityUpgrade { selected_protocol } => {
                selected_protocol.intersects(SecurityProtocol::HYBRID | SecurityProtocol::HYBRID_EX)
            }
            _ => false,
        };

        let (stream, leftover) = framed.into_inner();
        let (config, verifier) = client_config(pinned).map_err(|e| RdpError::Tls(e.to_string()))?;
        let server_name = rustls::pki_types::ServerName::try_from(host.to_owned())
            .map_err(|_| RdpError::Tls("invalid server name".into()))?;
        let tls = match tokio_rustls::TlsConnector::from(config)
            .connect(server_name, stream)
            .await
        {
            Ok(tls) => tls,
            Err(e) => {
                return Err(match verifier.observed() {
                    Observed {
                        fingerprint: Some(fingerprint),
                        trust: None,
                        problem,
                    } => RdpError::CertUntrusted {
                        fingerprint,
                        problem: problem.unwrap_or_else(|| "invalid".into()),
                    },
                    _ => RdpError::Tls(e.to_string()),
                })
            }
        };
        let observed = verifier.observed();
        let (fingerprint, trust) = match (observed.fingerprint, observed.trust) {
            (Some(f), Some(t)) => (f, t),
            _ => return Err(RdpError::Tls("certificate was not verified".into())),
        };
        let (server_public_key, tls_version) = {
            let conn = tls.get_ref().1;
            let cert = conn
                .peer_certificates()
                .and_then(|c| c.first())
                .ok_or_else(|| RdpError::Tls("server sent no certificate".into()))?;
            // 검증기가 본 인증서와 같은지 한 번 더 확인합니다.
            if cert_fingerprint(cert) != fingerprint {
                return Err(RdpError::Tls("certificate mismatch".into()));
            }
            (public_key(cert)?, tls_version(conn))
        };

        let upgraded = ironrdp_tokio::mark_as_upgraded(should_upgrade, &mut connector);
        let mut framed = Framed::new_with_leftover(tls, leftover);
        let result = ironrdp_tokio::connect_finalize(
            upgraded,
            connector,
            &mut framed,
            &mut NoKdc,
            ServerName::new(host),
            server_public_key,
            None,
        )
        .await
        .map_err(connector_error)?;

        Ok(Self {
            framed,
            result,
            info: RdpConnectionInfo {
                fingerprint,
                trust,
                nla,
                tls_version,
            },
            clip_events,
        })
    }

    pub fn info(&self) -> &RdpConnectionInfo {
        &self.info
    }

    /// 세션을 구동합니다. 명령 채널이 닫히면(탭 닫힘) `Ok`, 서버가 끊으면 에러로 끝납니다.
    pub async fn run(
        self,
        commands: &mut mpsc::Receiver<RdpCommand>,
        out: &dyn RdpOutput,
        local: Arc<dyn LocalClipboard>,
    ) -> Result<(), RdpError> {
        let Self {
            framed,
            result,
            clip_events,
            ..
        } = self;
        let (mut reader, mut writer) = ironrdp_tokio::split_tokio_framed(framed);
        let mut image = DecodedImage::new(
            PixelFormat::RgbA32,
            result.desktop_size.width,
            result.desktop_size.height,
        );
        let activation_factory = result.activation_factory;
        let mut stage = ActiveStageBuilder {
            static_channels: result.static_channels,
            user_channel_id: result.user_channel_id,
            io_channel_id: result.io_channel_id,
            message_channel_id: result.message_channel_id,
            share_id: result.share_id,
            compression_type: result.compression_type,
            enable_server_pointer: result.enable_server_pointer,
            pointer_software_rendering: result.pointer_software_rendering,
        }
        .build();

        let mut state = SessionState {
            input: ironrdp::input::Database::new(),
            dirty: DirtyRegion::default(),
            in_flight: 0,
            last_sent: tokio::time::Instant::now(),
            clipboard: ClipState {
                enabled: clip_events.is_some(),
                last_text: None,
                local,
            },
            requested: None,
        };
        let mut clip_events = clip_events;
        let mut tick = tokio::time::interval(FRAME_INTERVAL);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

        loop {
            let outputs = tokio::select! {
                pdu = reader.read_pdu() => {
                    let (action, payload) = pdu.map_err(|_| RdpError::ConnectionLost)?;
                    stage.process(&mut image, action, &payload).map_err(session_error)?
                }
                cmd = commands.recv() => match cmd {
                    None => return Ok(()),
                    Some(cmd) => state.command(cmd, &mut stage, &mut image).await?,
                },
                Some(event) = recv_clip(&mut clip_events) => {
                    let frame = state.clipboard.handle(event, &mut stage, out).await?;
                    vec![ActiveStageOutput::ResponseFrame(frame)]
                }
                _ = tick.tick() => {
                    state.flush(&image, out);
                    Vec::new()
                }
            };

            for output in outputs {
                match output {
                    ActiveStageOutput::ResponseFrame(frame) => {
                        if !frame.is_empty() {
                            writer
                                .write_all(&frame)
                                .await
                                .map_err(|_| RdpError::ConnectionLost)?;
                        }
                    }
                    ActiveStageOutput::GraphicsUpdate(region) => {
                        if let Some(rect) = Rect::clipped(&region, image.width(), image.height()) {
                            state.dirty.add(rect);
                        }
                    }
                    ActiveStageOutput::PointerDefault => {
                        out.binary(frame::encode_pointer_default())
                    }
                    ActiveStageOutput::PointerHidden => out.binary(frame::encode_pointer_hidden()),
                    ActiveStageOutput::PointerBitmap(p) => {
                        if let Some(msg) = frame::encode_pointer(
                            p.width,
                            p.height,
                            p.hotspot_x,
                            p.hotspot_y,
                            &p.bitmap_data,
                        ) {
                            out.binary(msg);
                        }
                    }
                    ActiveStageOutput::DeactivateAll => {
                        // 해상도 변경 등으로 서버가 세션을 다시 활성화함
                        let mut activation = activation_factory.create();
                        let mut buf = ironrdp::core::WriteBuf::new();
                        loop {
                            let written = ironrdp_tokio::single_sequence_step_read(
                                &mut reader,
                                &mut activation,
                                &mut buf,
                            )
                            .await
                            .map_err(connector_error)?;
                            if written.size().is_some() {
                                writer
                                    .write_all(buf.filled())
                                    .await
                                    .map_err(|_| RdpError::ConnectionLost)?;
                            }
                            if let ConnectionActivationState::Finalized {
                                desktop_size,
                                share_id,
                                enable_server_pointer,
                                pointer_software_rendering,
                                static_channel_chunk_size,
                                ..
                            } = activation.connection_activation_state()
                            {
                                tracing::debug!(
                                    width = desktop_size.width,
                                    height = desktop_size.height,
                                    "rdp session reactivated"
                                );
                                image = DecodedImage::new(
                                    PixelFormat::RgbA32,
                                    desktop_size.width,
                                    desktop_size.height,
                                );
                                if !stage.reactivate(
                                    activation.io_channel_id(),
                                    activation.user_channel_id(),
                                    share_id,
                                    enable_server_pointer,
                                    pointer_software_rendering,
                                    static_channel_chunk_size,
                                ) {
                                    return Err(RdpError::Protocol(
                                        "invalid static channel chunk size".into(),
                                    ));
                                }
                                state.dirty.clear();
                                state.dirty.add(Rect {
                                    x: 0,
                                    y: 0,
                                    w: desktop_size.width,
                                    h: desktop_size.height,
                                });
                                break;
                            }
                        }
                    }
                    ActiveStageOutput::Terminate(reason) => {
                        tracing::info!(reason = %reason.description(), "rdp session terminated by server");
                        return Err(RdpError::ConnectionLost);
                    }
                    _ => {}
                }
            }
        }
    }
}

async fn recv_clip(events: &mut Option<mpsc::UnboundedReceiver<ClipEvent>>) -> Option<ClipEvent> {
    match events {
        Some(rx) => rx.recv().await,
        None => std::future::pending().await,
    }
}

struct SessionState {
    input: ironrdp::input::Database,
    dirty: DirtyRegion,
    in_flight: u32,
    last_sent: tokio::time::Instant,
    clipboard: ClipState,
    /// 마지막으로 요청한 원격 화면 크기
    requested: Option<DisplaySize>,
}

impl SessionState {
    async fn command(
        &mut self,
        cmd: RdpCommand,
        stage: &mut ActiveStage,
        image: &mut DecodedImage,
    ) -> Result<Vec<ActiveStageOutput>, RdpError> {
        match cmd {
            RdpCommand::Input(events) => {
                let translated = input::translate(&events, image.width(), image.height())
                    .map_err(|e| RdpError::Protocol(e.to_string()))?;
                let mut fastpath = self.input.apply(translated.operations);
                if translated.release_all {
                    fastpath.extend(self.input.release_all());
                }
                stage
                    .process_fastpath_input(image, &fastpath)
                    .map_err(session_error)
            }
            RdpCommand::Resize(size) => {
                let size = size.normalized();
                if self.requested == Some(size)
                    || (size.width == image.width()
                        && size.height == image.height()
                        && self.requested.is_none())
                {
                    return Ok(Vec::new());
                }
                self.requested = Some(size);
                let (width, height) = MonitorLayoutEntry::adjust_display_size(
                    u32::from(size.width),
                    u32::from(size.height),
                );
                match stage.encode_resize(width, height, Some(size.scale), None) {
                    Some(frame) => Ok(vec![ActiveStageOutput::ResponseFrame(
                        frame.map_err(session_error)?,
                    )]),
                    None => {
                        // 서버가 디스플레이 제어 채널을 지원하지 않음: 웹뷰가 화면을 늘리거나 줄여 그림
                        tracing::debug!("display control unavailable; keeping desktop size");
                        Ok(Vec::new())
                    }
                }
            }
            RdpCommand::FrameAck => {
                self.in_flight = self.in_flight.saturating_sub(1);
                Ok(Vec::new())
            }
            RdpCommand::ClipboardSync => {
                let frame = self.clipboard.announce(false, stage).await?;
                Ok(vec![ActiveStageOutput::ResponseFrame(frame)])
            }
            RdpCommand::SetClipboard(enabled) => {
                self.clipboard.enabled = enabled;
                let frame = if enabled {
                    self.clipboard.announce(true, stage).await?
                } else {
                    Vec::new()
                };
                Ok(vec![ActiveStageOutput::ResponseFrame(frame)])
            }
        }
    }

    /// 쌓인 변경 영역을 웹뷰로 보냅니다. 확인되지 않은 메시지가 많으면 다음으로 미룹니다.
    fn flush(&mut self, image: &DecodedImage, out: &dyn RdpOutput) {
        if self.dirty.is_empty() {
            return;
        }
        if self.in_flight >= MAX_FRAMES_IN_FLIGHT {
            if self.last_sent.elapsed() < ACK_TIMEOUT {
                return;
            }
            self.in_flight = 0;
        }
        let rects = self.dirty.take();
        out.binary(frame::encode_frame(
            image.data(),
            image.width(),
            image.height(),
            &rects,
        ));
        self.in_flight += 1;
        self.last_sent = tokio::time::Instant::now();
    }
}

struct ClipState {
    enabled: bool,
    /// 마지막으로 주고받은 텍스트. 같은 내용을 다시 알리지 않습니다.
    last_text: Option<String>,
    local: Arc<dyn LocalClipboard>,
}

impl ClipState {
    async fn read_local(&self) -> Option<String> {
        let local = self.local.clone();
        tokio::task::spawn_blocking(move || local.read_text())
            .await
            .ok()
            .flatten()
            .filter(|t| !t.is_empty() && clipboard::fits(t))
    }

    fn encode(
        stage: &mut ActiveStage,
        messages: impl FnOnce(
            &mut CliprdrClient,
        ) -> Option<CliprdrSvcMessages<ironrdp::cliprdr::Client>>,
    ) -> Result<Vec<u8>, RdpError> {
        let Some(client) = stage.get_svc_processor_mut::<CliprdrClient>() else {
            return Ok(Vec::new());
        };
        match messages(client) {
            Some(msgs) => stage
                .process_svc_processor_messages(msgs)
                .map_err(session_error),
            None => Ok(Vec::new()),
        }
    }

    /// 로컬 텍스트를 원격에 알립니다. `force`면 바뀌지 않았어도(또는 비었어도) 알립니다.
    async fn announce(
        &mut self,
        force: bool,
        stage: &mut ActiveStage,
    ) -> Result<Vec<u8>, RdpError> {
        if !self.enabled {
            return Ok(Vec::new());
        }
        let text = self.read_local().await;
        if !force && (text.is_none() || text == self.last_text) {
            return Ok(Vec::new());
        }
        let formats: Vec<ClipboardFormat> = if text.is_some() {
            vec![ClipboardFormat::new(ClipboardFormatId::CF_UNICODETEXT)]
        } else {
            Vec::new()
        };
        if text.is_some() {
            self.last_text = text;
        }
        Self::encode(stage, |c| c.initiate_copy(&formats).ok())
    }

    async fn handle(
        &mut self,
        event: ClipEvent,
        stage: &mut ActiveStage,
        out: &dyn RdpOutput,
    ) -> Result<Vec<u8>, RdpError> {
        match event {
            ClipEvent::Announce => self.announce(true, stage).await,
            ClipEvent::RemoteCopy(formats) => {
                if !self.enabled || !formats.contains(&ClipboardFormatId::CF_UNICODETEXT) {
                    return Ok(Vec::new());
                }
                Self::encode(stage, |c| {
                    c.initiate_paste(ClipboardFormatId::CF_UNICODETEXT).ok()
                })
            }
            ClipEvent::DataRequest(format) => {
                let text = if self.enabled && format == ClipboardFormatId::CF_UNICODETEXT {
                    self.read_local().await
                } else {
                    None
                };
                let response = match &text {
                    Some(t) => {
                        FormatDataResponse::new_unicode_string(&clipboard::to_windows_newlines(t))
                    }
                    None => FormatDataResponse::new_error(),
                };
                let frame = Self::encode(stage, |c| c.submit_format_data(response).ok())?;
                if let Some(t) = text {
                    out.clipboard(ClipboardDirection::ToRemote, t.chars().count());
                }
                Ok(frame)
            }
            ClipEvent::DataResponse(data) => {
                let Some(text) = data.as_deref().and_then(clipboard::decode_unicode_text) else {
                    return Ok(Vec::new());
                };
                if !self.enabled || text.is_empty() || self.last_text.as_deref() == Some(&text) {
                    return Ok(Vec::new());
                }
                let local = self.local.clone();
                let written = {
                    let text = text.clone();
                    tokio::task::spawn_blocking(move || local.write_text(&text))
                        .await
                        .unwrap_or(false)
                };
                if written {
                    out.clipboard(ClipboardDirection::ToLocal, text.chars().count());
                    self.last_text = Some(text);
                }
                Ok(Vec::new())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(user: &str) -> RdpTarget {
        RdpTarget {
            host: "[::1]".into(),
            port: 3389,
            username: user.into(),
        }
    }

    #[test]
    fn splits_domain_from_username() {
        assert_eq!(
            target("CORP\\alice").credentials(),
            ("alice".into(), Some("CORP".into()))
        );
        assert_eq!(
            target("alice@corp.example").credentials(),
            ("alice@corp.example".into(), None)
        );
        assert_eq!(target("\\alice").credentials(), ("\\alice".into(), None));
        assert_eq!(target("x").bare_host(), "::1");
    }

    #[test]
    fn maps_logon_failures_to_auth_errors() {
        use connector::sspi::credssp::NStatusCode as S;
        assert_eq!(auth_failure(S::LOGON_FAILURE), Some(None));
        assert_eq!(
            auth_failure(S::ACCOUNT_LOCKED_OUT),
            Some(Some("account locked out"))
        );
        assert_eq!(auth_failure(S(0xC007_030C)), Some(None));
        assert_eq!(auth_failure(S(0xC007_0001)), None);
        assert_eq!(auth_failure(S::IO_TIMEOUT), None);
    }

    #[test]
    fn display_size_is_normalized() {
        let s = DisplaySize {
            width: 1921,
            height: 50,
            scale: 20,
        }
        .normalized();
        assert_eq!(
            s,
            DisplaySize {
                width: 1920,
                height: 200,
                scale: 100
            }
        );
        let s = DisplaySize {
            width: 9000,
            height: 9000,
            scale: 900,
        }
        .normalized();
        assert_eq!(
            s,
            DisplaySize {
                width: 8192,
                height: 8192,
                scale: 500
            }
        );
    }

    #[test]
    fn config_never_enables_standard_rdp_security() {
        let c = connector_config(
            &target("u"),
            &SecretString::from("pw"),
            DisplaySize {
                width: 1280,
                height: 720,
                scale: 100,
            },
        );
        assert!(!c.enable_standard_rdp_security);
        assert!(c.enable_credssp);
        assert_eq!(c.client_name, "Rivet");
    }
}
