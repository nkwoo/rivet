//! 프로세스 안에서 띄운 IronRDP 서버 대상 RDP 통합 테스트.
//!
//! Docker가 필요 없어 기본 `cargo test`에서 실행됩니다. 실제 Windows 서버와의 호환성은
//! 수동 확인으로 보완합니다(docs/ROADMAP.md 4단계).

use std::net::SocketAddr;
use std::num::{NonZeroU16, NonZeroUsize};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ironrdp_server::{
    BitmapUpdate, Credentials, DesktopSize, DisplayUpdate, KeyboardEvent, MouseEvent, PixelFormat,
    RdpServer, RdpServerDisplay, RdpServerDisplayUpdates, RdpServerInputHandler, ServerResult,
};
use secrecy::SecretString;
use tokio::net::TcpListener;
use tokio::sync::mpsc;

use crate::core::cert_pins::CertPinStore;
use crate::core::db::Database;
use crate::core::known_hosts::KnownHostsStore;
use crate::core::profile::{AuthMethod, Profile, Protocol};
use crate::core::secrets::{MemoryStore, SecretStore};
use crate::core::session::{
    prepare_rdp_request, JumpSecrets, SessionEvent, SessionManager, SessionSink,
};
use crate::protocols::rdp::clipboard::LocalClipboard;
use crate::protocols::rdp::frame::KIND_FRAME;
use crate::protocols::rdp::input::InputEvent;
use crate::protocols::rdp::{DisplaySize, RdpCommand};

const USER: &str = "rivet";
const PASSWORD: &str = "rivet-rdp-it-password";
const WAIT: Duration = Duration::from_secs(20);
/// 서버 화면 색 (B, G, R, A 순서로 보냄)
const COLOR_BGRA: [u8; 4] = [30, 20, 10, 255];
/// 받아야 하는 색 (RGBA). RemoteFX는 손실 압축이라 약간의 오차를 허용합니다.
const COLOR_RGBA: [u8; 4] = [10, 20, 30, 255];

fn assert_color(pixel: [u8; 4]) {
    let close = pixel
        .iter()
        .zip(COLOR_RGBA)
        .all(|(a, b)| a.abs_diff(b) <= 4);
    assert!(close, "pixel {pixel:?} is not close to {COLOR_RGBA:?}");
}

/// RDP 통합 테스트는 하나씩 실행합니다. 다른 스레드에서 동시에 IronRDP 로그 지점이 처음 불리면
/// tracing의 관심도 캐시가 엇갈려 로그 검사 테스트가 로그를 놓칠 수 있습니다.
static SERIAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

#[derive(Clone, Copy)]
enum Security {
    Nla,
    Tls,
}

/// 서버가 받은 입력과 해상도 변경 요청
#[derive(Debug, PartialEq, Eq)]
enum Seen {
    Key {
        code: u8,
        extended: bool,
        pressed: bool,
    },
    Unicode(u16),
    Move {
        x: u16,
        y: u16,
    },
    Layout {
        width: u32,
        height: u32,
    },
}

#[derive(Clone)]
struct Handler {
    seen: mpsc::UnboundedSender<Seen>,
    size: Arc<Mutex<DesktopSize>>,
    updates: Arc<Mutex<Option<mpsc::UnboundedSender<DisplayUpdate>>>>,
}

fn bitmap(width: u16, height: u16) -> DisplayUpdate {
    bitmap_color(width, height, COLOR_BGRA)
}

fn bitmap_color(width: u16, height: u16, bgra: [u8; 4]) -> DisplayUpdate {
    let data: Vec<u8> = bgra
        .iter()
        .copied()
        .cycle()
        .take(usize::from(width) * usize::from(height) * 4)
        .collect();
    DisplayUpdate::Bitmap(BitmapUpdate {
        x: 0,
        y: 0,
        width: NonZeroU16::new(width).unwrap(),
        height: NonZeroU16::new(height).unwrap(),
        format: PixelFormat::BgrA32,
        data: data.into(),
        stride: NonZeroUsize::new(usize::from(width) * 4).unwrap(),
    })
}

impl RdpServerInputHandler for Handler {
    fn keyboard(&mut self, event: KeyboardEvent) {
        let seen = match event {
            KeyboardEvent::Pressed { code, extended } => Seen::Key {
                code,
                extended,
                pressed: true,
            },
            KeyboardEvent::Released { code, extended } => Seen::Key {
                code,
                extended,
                pressed: false,
            },
            KeyboardEvent::UnicodePressed(u) => Seen::Unicode(u),
            _ => return,
        };
        let _ = self.seen.send(seen);
    }

    fn mouse(&mut self, event: MouseEvent) {
        if let MouseEvent::Move { x, y } = event {
            let _ = self.seen.send(Seen::Move { x, y });
        }
    }
}

struct Updates(mpsc::UnboundedReceiver<DisplayUpdate>);

#[async_trait::async_trait]
impl RdpServerDisplayUpdates for Updates {
    async fn next_update(&mut self) -> ServerResult<Option<DisplayUpdate>> {
        Ok(self.0.recv().await)
    }
}

#[async_trait::async_trait]
impl RdpServerDisplay for Handler {
    async fn size(&mut self) -> DesktopSize {
        *self.size.lock().unwrap()
    }

    async fn updates(&mut self) -> ServerResult<Box<dyn RdpServerDisplayUpdates>> {
        let (tx, rx) = mpsc::unbounded_channel();
        let size = *self.size.lock().unwrap();
        let _ = tx.send(bitmap(size.width, size.height));
        *self.updates.lock().unwrap() = Some(tx);
        Ok(Box::new(Updates(rx)))
    }

    fn request_layout(
        &mut self,
        layout: ironrdp::displaycontrol::pdu::DisplayControlMonitorLayout,
    ) {
        let Some(monitor) = layout.monitors().first() else {
            return;
        };
        let (width, height) = monitor.dimensions();
        let _ = self.seen.send(Seen::Layout { width, height });
        let size = DesktopSize {
            width: u16::try_from(width).unwrap(),
            height: u16::try_from(height).unwrap(),
        };
        *self.size.lock().unwrap() = size;
        if let Some(tx) = self.updates.lock().unwrap().as_ref() {
            let _ = tx.send(DisplayUpdate::Resize(size));
            let _ = tx.send(bitmap(size.width, size.height));
        }
    }
}

/// 별도 스레드의 런타임에서 도는 서버. 테스트 스레드의 로그 구독자에 서버 로그가 섞이지 않습니다.
/// 테스트 인증서로 TLS 수락기와 CredSSP용 공개 키를 만듭니다.
fn tls_identity(name: &str) -> (tokio_rustls::TlsAcceptor, Vec<u8>) {
    use rustls::pki_types::pem::PemObject as _;
    use rustls::pki_types::{CertificateDer, PrivateKeyDer};

    let cert = CertificateDer::from_pem_file(fixture(&format!("{name}_cert.pem"))).unwrap();
    let key = PrivateKeyDer::from_pem_file(fixture(&format!("{name}_key.pem"))).unwrap();
    let pub_key = crate::protocols::rdp::public_key(&cert).unwrap();
    let config = rustls::ServerConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .unwrap()
    .with_no_client_auth()
    .with_single_cert(vec![cert], key)
    .unwrap();
    (tokio_rustls::TlsAcceptor::from(Arc::new(config)), pub_key)
}

struct TestServer {
    addr: SocketAddr,
    seen: mpsc::UnboundedReceiver<Seen>,
    /// 접속 중인 클라이언트로 화면 갱신을 보냄
    updates: Arc<Mutex<Option<mpsc::UnboundedSender<DisplayUpdate>>>>,
    _stop: tokio::sync::oneshot::Sender<()>,
}

impl TestServer {
    async fn start(security: Security, cert: &'static str) -> Self {
        Self::start_sized(security, cert, 800, 600).await
    }

    async fn start_sized(security: Security, cert: &'static str, width: u16, height: u16) -> Self {
        Self::start_on("127.0.0.1", security, cert, width, height).await
    }

    /// `bind`: 듣는 주소. Docker 컨테이너(점프 호스트)에서 접속해야 하면 "0.0.0.0"
    async fn start_on(
        bind: &'static str,
        security: Security,
        cert: &'static str,
        width: u16,
        height: u16,
    ) -> Self {
        let updates = Arc::new(Mutex::new(None));
        let shared = updates.clone();
        let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
        let (stop, stop_rx) = tokio::sync::oneshot::channel::<()>();
        let (tx, seen) = mpsc::unbounded_channel();
        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
                .unwrap();
            rt.block_on(async move {
                let listener = TcpListener::bind((bind, 0)).await.unwrap();
                let _ = ready_tx.send(listener.local_addr().unwrap());
                let mut server =
                    Self::build(security, cert, tx, DesktopSize { width, height }, shared);
                let serve = async {
                    while let Ok((stream, _)) = listener.accept().await {
                        let _ = server.run_connection(stream).await;
                    }
                };
                tokio::select! {
                    _ = serve => {}
                    _ = stop_rx => {}
                }
            });
        });
        let addr = ready_rx.await.unwrap();
        Self {
            addr,
            seen,
            updates,
            _stop: stop,
        }
    }

    fn build(
        security: Security,
        cert: &str,
        tx: mpsc::UnboundedSender<Seen>,
        size: DesktopSize,
        updates: Arc<Mutex<Option<mpsc::UnboundedSender<DisplayUpdate>>>>,
    ) -> RdpServer {
        let addr: SocketAddr = ([127, 0, 0, 1], 0).into();
        let (acceptor, pub_key) = tls_identity(cert);
        let handler = Handler {
            seen: tx,
            size: Arc::new(Mutex::new(size)),
            updates,
        };
        let builder = RdpServer::builder().with_addr(addr);
        let builder = match security {
            Security::Nla => builder.with_hybrid(acceptor, pub_key),
            Security::Tls => builder.with_tls(acceptor),
        };
        let mut server = builder
            .with_input_handler(handler.clone())
            .with_display_handler(handler)
            .build();
        server.set_credentials(Some(Credentials {
            username: USER.into(),
            password: PASSWORD.into(),
            domain: None,
        }));
        server
    }

    async fn next_seen(&mut self, want: impl Fn(&Seen) -> bool) -> Seen {
        tokio::time::timeout(WAIT, async {
            loop {
                let s = self.seen.recv().await.expect("server stopped");
                if want(&s) {
                    return s;
                }
            }
        })
        .await
        .expect("server did not receive the expected input")
    }
}

fn profile(port: u16) -> Profile {
    Profile {
        id: uuid::Uuid::new_v4().to_string(),
        name: "rdp-it".into(),
        protocol: Protocol::Rdp,
        host: "127.0.0.1".into(),
        port,
        username: USER.into(),
        auth_method: AuthMethod::Password,
        key_path: None,
        group: None,
        tags: vec![],
        notes: String::new(),
        favorite: false,
        rdp: None,
        jump_profile_id: None,
        created_at: 0,
        updated_at: 0,
    }
}

struct Sink {
    events: mpsc::UnboundedSender<SessionEvent>,
    frames: mpsc::UnboundedSender<Vec<u8>>,
}

impl SessionSink for Sink {
    fn output(&self, data: &[u8]) {
        let _ = self.frames.send(data.to_vec());
    }

    fn output_owned(&self, data: Vec<u8>) {
        let _ = self.frames.send(data);
    }

    fn event(&self, event: SessionEvent) {
        let _ = self.events.send(event);
    }
}

#[derive(Default)]
struct MemoryClipboard(Mutex<Option<String>>);

impl LocalClipboard for MemoryClipboard {
    fn read_text(&self) -> Option<String> {
        self.0.lock().unwrap().clone()
    }

    fn write_text(&self, text: &str) -> bool {
        *self.0.lock().unwrap() = Some(text.to_owned());
        true
    }
}

struct Harness {
    manager: Arc<SessionManager>,
    pins: Arc<CertPinStore>,
    known_hosts: Arc<KnownHostsStore>,
    secrets: Arc<MemoryStore>,
}

impl Harness {
    fn new() -> Self {
        Self {
            manager: SessionManager::new(),
            pins: Arc::new(CertPinStore::new(Arc::new(
                Database::open_in_memory().unwrap(),
            ))),
            known_hosts: Arc::new(KnownHostsStore::new(Arc::new(
                Database::open_in_memory().unwrap(),
            ))),
            secrets: Arc::new(MemoryStore::default()),
        }
    }

    fn connect(&self, p: &Profile, password: Option<&str>, save: bool) -> Session {
        self.connect_sized(p, password, save, 800, 600)
    }

    fn connect_sized(
        &self,
        p: &Profile,
        password: Option<&str>,
        save: bool,
        width: u16,
        height: u16,
    ) -> Session {
        self.connect_with(
            p,
            &[],
            JumpSecrets::default(),
            password,
            save,
            width,
            height,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn connect_with(
        &self,
        p: &Profile,
        jumps: &[Profile],
        jump_secrets: JumpSecrets,
        password: Option<&str>,
        save: bool,
        width: u16,
        height: u16,
    ) -> Session {
        let secrets: Arc<dyn SecretStore> = self.secrets.clone();
        let size = DisplaySize {
            width,
            height,
            scale: 100,
        };
        let request = prepare_rdp_request(
            p,
            jumps,
            password.map(SecretString::from),
            save,
            jump_secrets,
            secrets,
            size,
        )
        .unwrap();
        let (events, rx) = mpsc::unbounded_channel();
        let (frames, frames_rx) = mpsc::unbounded_channel();
        let id = self.manager.spawn_rdp(
            request,
            self.pins.clone(),
            self.known_hosts.clone(),
            Arc::new(Sink { events, frames }),
            Arc::new(MemoryClipboard::default()),
        );
        Session {
            id,
            rx,
            frames: frames_rx,
            manager: self.manager.clone(),
        }
    }
}

struct Session {
    id: String,
    rx: mpsc::UnboundedReceiver<SessionEvent>,
    frames: mpsc::UnboundedReceiver<Vec<u8>>,
    manager: Arc<SessionManager>,
}

/// 화면 메시지 헤더: (원격 너비, 높이, 첫 사각형의 첫 픽셀 RGBA)
fn parse_frame(msg: &[u8]) -> (u16, u16, [u8; 4]) {
    assert_eq!(msg[0], KIND_FRAME);
    let u16_at = |i: usize| u16::from_le_bytes([msg[i], msg[i + 1]]);
    let count = u16_at(6);
    assert!(count >= 1);
    (u16_at(2), u16_at(4), [msg[16], msg[17], msg[18], msg[19]])
}

impl Session {
    async fn next(&mut self) -> SessionEvent {
        tokio::time::timeout(WAIT, self.rx.recv())
            .await
            .expect("timed out waiting for event")
            .expect("session ended")
    }

    async fn connected_accepting_certs(&mut self) -> SessionEvent {
        loop {
            match self.next().await {
                SessionEvent::CertPrompt { .. } => {
                    self.manager.respond_host_key(&self.id, true).unwrap()
                }
                e @ SessionEvent::Connected { .. } => return e,
                SessionEvent::Error { code, message } => {
                    panic!("unexpected error {code}: {message}")
                }
                _ => {}
            }
        }
    }

    async fn error_code(&mut self) -> String {
        loop {
            match self.next().await {
                SessionEvent::Error { code, .. } => return code,
                SessionEvent::Connected { .. } => panic!("unexpectedly connected"),
                _ => {}
            }
        }
    }

    /// 지정한 크기의 화면 메시지가 올 때까지 기다리며 매번 확인(ack)을 보냅니다.
    /// 서버 비트맵이 오기 전에 보내는 빈(아직 그려지지 않은, 검은) 화면은 건너뜁니다.
    async fn frame_of_size(&mut self, width: u16, height: u16) -> [u8; 4] {
        tokio::time::timeout(WAIT, async {
            loop {
                let msg = self.frames.recv().await.expect("session ended");
                if msg[0] != KIND_FRAME {
                    continue;
                }
                self.send(RdpCommand::FrameAck).await;
                let (w, h, pixel) = parse_frame(&msg);
                if (w, h) == (width, height) && pixel[..3] != [0, 0, 0] {
                    return pixel;
                }
            }
        })
        .await
        .expect("no frame of the expected size")
    }

    async fn send(&self, cmd: RdpCommand) {
        self.manager.send_rdp(&self.id, cmd).await.unwrap();
    }
}

fn info(e: &SessionEvent) -> (&str, &str, &str) {
    match e {
        SessionEvent::Connected { info } => (
            info.transport.as_str(),
            info.kex.as_str(),
            info.fingerprint.as_str(),
        ),
        other => panic!("not connected: {other:?}"),
    }
}

#[tokio::test]
async fn rdp_integration_nla_session_streams_frames_and_input() {
    let _serial = SERIAL.lock().await;
    let mut server = TestServer::start(Security::Nla, "rdp").await;
    let h = Harness::new();
    let p = profile(server.addr.port());
    let mut s = h.connect(&p, Some(PASSWORD), true);

    // 자체 서명 인증서: 처음에는 지문을 보여주고 승인을 기다림
    let fingerprint = match s.next().await {
        SessionEvent::CertPrompt {
            fingerprint, port, ..
        } => {
            assert_eq!(port, p.port);
            fingerprint
        }
        other => panic!("expected certificate prompt, got {other:?}"),
    };
    h.manager.respond_host_key(&s.id, true).unwrap();
    let connected = s.connected_accepting_certs().await;
    let (transport, kex, fp) = info(&connected);
    assert_eq!(transport, "tlsPinned");
    assert!(kex.contains("NLA"), "{kex}");
    assert_eq!(fp, fingerprint);
    assert_eq!(
        h.pins.pinned("127.0.0.1", p.port).unwrap().as_deref(),
        Some(fingerprint.as_str())
    );
    assert!(
        h.secrets.get(&p.id).unwrap().is_some(),
        "password saved after auth"
    );

    // 서버 화면(BGRA)이 RGBA로 변환되어 도착
    assert_color(s.frame_of_size(800, 600).await);

    // 마우스, 스캔 코드(확장 키 포함), IME 조합 글자
    s.send(RdpCommand::Input(vec![
        InputEvent::Move { x: 120, y: 80 },
        InputEvent::Key {
            code: 0x1E,
            down: true,
        },
        InputEvent::Key {
            code: 0x1E,
            down: false,
        },
        InputEvent::Key {
            code: 0xE053,
            down: true,
        },
        InputEvent::Text { text: "한".into() },
        InputEvent::ReleaseAll,
    ]))
    .await;
    assert_eq!(
        server.next_seen(|s| matches!(s, Seen::Move { .. })).await,
        Seen::Move { x: 120, y: 80 }
    );
    assert_eq!(
        server.next_seen(|s| matches!(s, Seen::Key { .. })).await,
        Seen::Key {
            code: 0x1E,
            extended: false,
            pressed: true
        }
    );
    assert_eq!(
        server
            .next_seen(|s| matches!(s, Seen::Key { extended: true, .. }))
            .await,
        Seen::Key {
            code: 0x53,
            extended: true,
            pressed: true
        }
    );
    assert_eq!(
        server.next_seen(|s| matches!(s, Seen::Unicode(_))).await,
        Seen::Unicode(u16::from_str_radix("D55C", 16).unwrap())
    );
    // 포커스를 잃으면 눌린 확장 키를 뗌
    assert_eq!(
        server
            .next_seen(|s| matches!(
                s,
                Seen::Key {
                    pressed: false,
                    extended: true,
                    ..
                }
            ))
            .await,
        Seen::Key {
            code: 0x53,
            extended: true,
            pressed: false
        }
    );

    // 해상도 변경: 디스플레이 제어 채널로 요청 → 서버가 재활성화 → 새 크기 화면
    s.send(RdpCommand::Resize(DisplaySize {
        width: 1024,
        height: 768,
        scale: 100,
    }))
    .await;
    assert_eq!(
        server.next_seen(|s| matches!(s, Seen::Layout { .. })).await,
        Seen::Layout {
            width: 1024,
            height: 768
        }
    );
    assert_color(s.frame_of_size(1024, 768).await);

    // 탭을 닫으면 세션 정리
    h.manager.close(&s.id);
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(h.manager.len(), 0);

    // 두 번째 접속은 묻지 않고 저장된 비밀번호와 고정 지문으로 연결
    let mut again = h.connect(&p, None, false);
    match again.next().await {
        e @ SessionEvent::Connected { .. } => assert_eq!(info(&e).0, "tlsPinned"),
        other => panic!("expected direct connection, got {other:?}"),
    }
}

#[tokio::test]
async fn rdp_integration_wrong_password_fails_without_saving() {
    let _serial = SERIAL.lock().await;
    let server = TestServer::start(Security::Nla, "rdp").await;
    let h = Harness::new();
    let p = profile(server.addr.port());
    let mut s = h.connect(&p, Some("wrong-password"), true);
    assert!(matches!(s.next().await, SessionEvent::CertPrompt { .. }));
    h.manager.respond_host_key(&s.id, true).unwrap();
    let (code, message) = loop {
        match s.next().await {
            SessionEvent::Error { code, message } => break (code, message),
            SessionEvent::Connected { .. } => panic!("unexpectedly connected"),
            _ => {}
        }
    };
    assert_eq!(code, "authFailed", "{message}");
    assert!(h.secrets.get(&p.id).unwrap().is_none());
}

#[tokio::test]
async fn rdp_integration_rejected_certificate_aborts() {
    let _serial = SERIAL.lock().await;
    let server = TestServer::start(Security::Nla, "rdp").await;
    let h = Harness::new();
    let p = profile(server.addr.port());
    let mut s = h.connect(&p, Some(PASSWORD), true);
    assert!(matches!(s.next().await, SessionEvent::CertPrompt { .. }));
    h.manager.respond_host_key(&s.id, false).unwrap();
    assert_eq!(s.error_code().await, "certRejected");
    assert!(h.pins.pinned("127.0.0.1", p.port).unwrap().is_none());
    assert!(h.secrets.get(&p.id).unwrap().is_none());
}

#[tokio::test]
async fn rdp_integration_changed_certificate_is_blocked() {
    let _serial = SERIAL.lock().await;
    // 처음 인증서를 고정한 뒤 서버 인증서가 바뀐 상황
    let first = TestServer::start(Security::Nla, "rdp").await;
    let h = Harness::new();
    let mut s = h.connect(&profile(first.addr.port()), Some(PASSWORD), false);
    s.connected_accepting_certs().await;
    let pinned = h
        .pins
        .pinned("127.0.0.1", first.addr.port())
        .unwrap()
        .unwrap();
    h.manager.close(&s.id);

    let other = TestServer::start(Security::Nla, "rdp_other").await;
    h.pins.pin("127.0.0.1", other.addr.port(), &pinned).unwrap();
    let mut s = h.connect(&profile(other.addr.port()), Some(PASSWORD), false);
    match s.next().await {
        SessionEvent::CertChanged {
            expected_fingerprint,
            actual_fingerprint,
            ..
        } => {
            assert_eq!(expected_fingerprint, pinned);
            assert_ne!(actual_fingerprint, pinned);
        }
        other => panic!("expected certificate change, got {other:?}"),
    }
    assert_eq!(s.error_code().await, "certChanged");
    assert_eq!(
        h.pins.pinned("127.0.0.1", other.addr.port()).unwrap(),
        Some(pinned)
    );
}

#[tokio::test]
async fn rdp_integration_tls_only_server_is_marked_without_nla() {
    let _serial = SERIAL.lock().await;
    let server = TestServer::start(Security::Tls, "rdp").await;
    let h = Harness::new();
    let mut s = h.connect(&profile(server.addr.port()), Some(PASSWORD), false);
    let connected = s.connected_accepting_certs().await;
    let (_, kex, _) = info(&connected);
    assert!(!kex.contains("NLA"), "{kex}");
    assert_color(s.frame_of_size(800, 600).await);
}

#[tokio::test]
async fn rdp_integration_unreachable_port_reports_connect_error() {
    let _serial = SERIAL.lock().await;
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let h = Harness::new();
    let mut s = h.connect(&profile(port), Some(PASSWORD), false);
    assert_eq!(s.error_code().await, "connect");
}

#[tokio::test]
async fn rdp_integration_password_never_reaches_logs() {
    let _serial = SERIAL.lock().await;
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::Layer;

    #[derive(Clone, Default)]
    struct LogBuffer(Arc<Mutex<Vec<u8>>>);
    impl std::io::Write for LogBuffer {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    let _ = tracing_log::LogTracer::init();
    let buffer = LogBuffer::default();
    let writer = buffer.clone();
    let subscriber = tracing_subscriber::registry().with(
        tracing_subscriber::fmt::layer()
            .with_ansi(false)
            .with_writer(move || writer.clone())
            .with_filter(tracing_subscriber::filter::LevelFilter::TRACE)
            .with_filter(crate::secret_safe_filter()),
    );
    let _guard = tracing::subscriber::set_default(subscriber);

    // 서버는 다른 스레드에서 돌므로 여기에는 클라이언트(앱) 로그만 모입니다.
    let server = TestServer::start(Security::Nla, "rdp").await;
    let h = Harness::new();
    let mut s = h.connect(&profile(server.addr.port()), Some(PASSWORD), false);
    s.connected_accepting_certs().await;
    s.frame_of_size(800, 600).await;
    h.manager.close(&s.id);

    let logs = String::from_utf8_lossy(&buffer.0.lock().unwrap()).into_owned();
    // 다른 테스트와 병렬로 돌 때도 이 스레드의 클라이언트 로그(TLS, IronRDP 연결 단계)는 모입니다.
    assert!(logs.contains("ironrdp"), "logs were captured: {logs}");
    assert!(!logs.contains(PASSWORD), "password leaked into logs");
}

/// 1080p 처리량 측정 (성능 확인용, 기본 실행에서 제외).
///
/// 서버가 화면 전체를 계속 바꾸는 최악의 경우에 웹뷰 쪽으로 몇 fps가 전달되는지 봅니다.
/// RemoteFX 인코딩(서버)·디코딩·변경 영역 인코딩·확인(ack) 흐름 제어까지 포함하고,
/// 웹뷰 IPC와 캔버스 그리기는 포함하지 않습니다.
/// 실행: `cargo test --release rdp_integration_1080p_throughput -- --ignored --nocapture`
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "performance measurement"]
async fn rdp_integration_1080p_throughput() {
    let _serial = SERIAL.lock().await;
    let (w, h) = (1920u16, 1080u16);
    let server = TestServer::start_sized(Security::Nla, "rdp", w, h).await;
    let harness = Harness::new();
    let mut s = harness.connect_sized(&profile(server.addr.port()), Some(PASSWORD), false, w, h);
    s.connected_accepting_certs().await;
    s.frame_of_size(w, h).await;

    let updates = server.updates.lock().unwrap().clone().unwrap();
    let duration = Duration::from_secs(5);
    let start = std::time::Instant::now();
    let producer = std::thread::spawn(move || {
        let mut i = 0u8;
        while start.elapsed() < duration {
            // 60fps로 화면 전체를 바꿈
            i = i.wrapping_add(7);
            if updates
                .send(bitmap_color(1920, 1080, [i, 255 - i, 128, 255]))
                .is_err()
            {
                break;
            }
            std::thread::sleep(Duration::from_millis(16));
        }
    });

    let (mut frames, mut bytes) = (0u64, 0u64);
    while start.elapsed() < duration {
        match tokio::time::timeout(Duration::from_millis(200), s.frames.recv()).await {
            Ok(Some(msg)) if msg[0] == KIND_FRAME => {
                frames += 1;
                bytes += msg.len() as u64;
                s.send(RdpCommand::FrameAck).await;
            }
            Ok(Some(_)) | Err(_) => {}
            Ok(None) => break,
        }
    }
    producer.join().unwrap();
    let secs = start.elapsed().as_secs_f64();
    println!(
        "1080p full-screen updates: {:.1} fps to webview, {:.0} MiB/s",
        frames as f64 / secs,
        bytes as f64 / secs / 1024.0 / 1024.0
    );
    assert!(frames > 0);
}

/// 작은 화면 변경(타이핑 한 글자 정도)이 웹뷰 쪽에 도착하기까지의 지연 측정 (성능 확인용).
///
/// 실행: `cargo test [--release] rdp_integration_small_update_latency -- --ignored --nocapture`
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "performance measurement"]
async fn rdp_integration_small_update_latency() {
    let _serial = SERIAL.lock().await;
    let (w, h) = (1920u16, 1080u16);
    let server = TestServer::start_sized(Security::Nla, "rdp", w, h).await;
    let harness = Harness::new();
    let mut s = harness.connect_sized(&profile(server.addr.port()), Some(PASSWORD), false, w, h);
    s.connected_accepting_certs().await;
    s.frame_of_size(w, h).await;
    let updates = server.updates.lock().unwrap().clone().unwrap();

    let mut samples = Vec::new();
    for i in 0..40u16 {
        let color = [(i * 6) as u8, 200, 40, 255];
        let start = std::time::Instant::now();
        updates
            .send(DisplayUpdate::Bitmap(BitmapUpdate {
                x: 100 + i * 40,
                y: 500,
                width: NonZeroU16::new(32).unwrap(),
                height: NonZeroU16::new(32).unwrap(),
                format: PixelFormat::BgrA32,
                data: color
                    .iter()
                    .copied()
                    .cycle()
                    .take(32 * 32 * 4)
                    .collect::<Vec<u8>>()
                    .into(),
                stride: NonZeroUsize::new(32 * 4).unwrap(),
            }))
            .unwrap();
        loop {
            let msg = tokio::time::timeout(WAIT, s.frames.recv())
                .await
                .unwrap()
                .unwrap();
            if msg[0] == KIND_FRAME {
                s.send(RdpCommand::FrameAck).await;
                break;
            }
        }
        samples.push(start.elapsed().as_secs_f64() * 1000.0);
        tokio::time::sleep(Duration::from_millis(30)).await;
    }
    samples.sort_by(f64::total_cmp);
    println!(
        "small update latency: median {:.1} ms, p90 {:.1} ms (server encode + client decode)",
        samples[samples.len() / 2],
        samples[samples.len() * 9 / 10]
    );
}

/// SSH 터널을 거친 RDP: Docker OpenSSH 컨테이너를 점프 호스트로 쓰고, 컨테이너에서 호스트의
/// 테스트 RDP 서버(host.docker.internal)로 터널을 엽니다. `run-ssh-it.sh`로 실행합니다.
#[tokio::test]
#[ignore = "needs Docker OpenSSH: tests/docker/run-ssh-it.sh"]
async fn ssh_integration_rdp_over_jump_host_tunnel() {
    let _serial = SERIAL.lock().await;
    let ssh_port: u16 = std::env::var("RIVET_IT_SSH_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(2222);
    let server = TestServer::start_on("0.0.0.0", Security::Nla, "rdp", 800, 600).await;
    let h = Harness::new();

    let jump = Profile {
        id: "jump".into(),
        name: "bastion".into(),
        protocol: Protocol::Ssh,
        host: "127.0.0.1".into(),
        port: ssh_port,
        username: "rivet".into(),
        auth_method: AuthMethod::Password,
        ..profile(0)
    };
    let mut target = profile(server.addr.port());
    target.host = "host.docker.internal".into();
    target.jump_profile_id = Some(jump.id.clone());

    let mut provided = std::collections::HashMap::new();
    provided.insert(jump.id.clone(), SecretString::from("rivet-it-password"));
    let mut s = h.connect_with(
        &target,
        std::slice::from_ref(&jump),
        JumpSecrets {
            provided,
            save: false,
        },
        Some(PASSWORD),
        false,
        800,
        600,
    );

    // SSH 호스트 키(점프 호스트) → RDP 인증서 순서로 확인
    let mut saw_host_key = false;
    let connected = tokio::time::timeout(WAIT, async {
        loop {
            match s.next().await {
                SessionEvent::HostKeyPrompt { port, .. } => {
                    assert_eq!(port, ssh_port);
                    saw_host_key = true;
                    s.manager.respond_host_key(&s.id, true).unwrap();
                }
                SessionEvent::CertPrompt { host, .. } => {
                    assert!(saw_host_key, "SSH host key must be verified first");
                    assert_eq!(host, "host.docker.internal");
                    s.manager.respond_host_key(&s.id, true).unwrap();
                }
                e @ SessionEvent::Connected { .. } => return e,
                SessionEvent::Error { code, message } => {
                    panic!("unexpected error {code}: {message}")
                }
                _ => {}
            }
        }
    })
    .await
    .unwrap();
    match connected {
        SessionEvent::Connected { info } => {
            assert_eq!(info.via, vec!["bastion".to_string()]);
            assert!(info.kex.contains("NLA"), "{}", info.kex);
        }
        _ => unreachable!(),
    }
    // 터널 위에서 화면과 입력이 오감
    assert_color(s.frame_of_size(800, 600).await);
    let mut server = server;
    s.send(RdpCommand::Input(vec![InputEvent::Key {
        code: 0x1E,
        down: true,
    }]))
    .await;
    assert_eq!(
        server.next_seen(|x| matches!(x, Seen::Key { .. })).await,
        Seen::Key {
            code: 0x1E,
            extended: false,
            pressed: true
        }
    );
    h.manager.close(&s.id);
}
