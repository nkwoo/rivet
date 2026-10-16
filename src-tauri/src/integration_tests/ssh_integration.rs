//! Docker OpenSSH 서버 대상 통합 테스트.
//!
//! 실행: `src-tauri/tests/docker/run-ssh-it.sh`
//! (서버를 띄우고 `RIVET_IT_SSH_PORT`, `RIVET_IT_SSH_KEY`를 설정한 뒤 `--ignored`로 실행)

use std::io::Write;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use russh::keys::PublicKey;
use secrecy::SecretString;
use tokio::sync::mpsc;

use crate::core::db::Database;
use crate::core::known_hosts::KnownHostsStore;
use crate::core::profile::{AuthMethod, Profile, Protocol};
use crate::core::secrets::{MemoryStore, SecretStore};
use crate::core::session::{prepare_ssh_request, SessionEvent, SessionManager, SessionSink};
use crate::protocols::ssh::ShellCommand;

const PASSWORD: &str = "rivet-it-password";
const WAIT: Duration = Duration::from_secs(20);

fn port() -> u16 {
    std::env::var("RIVET_IT_SSH_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(2222)
}

fn profile(auth: AuthMethod) -> Profile {
    Profile {
        id: uuid::Uuid::new_v4().to_string(),
        name: "it".into(),
        protocol: Protocol::Ssh,
        host: "127.0.0.1".into(),
        port: port(),
        username: "rivet".into(),
        auth_method: auth,
        key_path: std::env::var("RIVET_IT_SSH_KEY").ok(),
        group: None,
        tags: vec![],
        notes: String::new(),
        favorite: false,
        rdp: None,
        created_at: 0,
        updated_at: 0,
    }
}

enum Item {
    Output(Vec<u8>),
    Event(SessionEvent),
}

struct TestSink(mpsc::UnboundedSender<Item>);

impl SessionSink for TestSink {
    fn output(&self, data: &[u8]) {
        let _ = self.0.send(Item::Output(data.to_vec()));
    }
    fn event(&self, event: SessionEvent) {
        let _ = self.0.send(Item::Event(event));
    }
}

/// 테스트 하나가 쓰는 앱 구성요소 묶음
struct Harness {
    manager: Arc<SessionManager>,
    known_hosts: Arc<KnownHostsStore>,
    secrets: Arc<MemoryStore>,
}

impl Harness {
    fn new() -> Self {
        Self {
            manager: SessionManager::new(),
            known_hosts: Arc::new(KnownHostsStore::new(Arc::new(
                Database::open_in_memory().unwrap(),
            ))),
            secrets: Arc::new(MemoryStore::default()),
        }
    }

    fn connect(&self, profile: &Profile, secret: Option<&str>, save: bool) -> (String, Session) {
        let secrets: Arc<dyn SecretStore> = self.secrets.clone();
        let request = prepare_ssh_request(
            profile,
            secret.map(SecretString::from),
            save,
            secrets,
            80,
            24,
        )
        .unwrap();
        let (tx, rx) = mpsc::unbounded_channel();
        let id = self
            .manager
            .spawn_ssh(request, self.known_hosts.clone(), Arc::new(TestSink(tx)));
        (
            id.clone(),
            Session {
                id,
                rx,
                output: Vec::new(),
                manager: self.manager.clone(),
            },
        )
    }
}

struct Session {
    id: String,
    rx: mpsc::UnboundedReceiver<Item>,
    output: Vec<u8>,
    manager: Arc<SessionManager>,
}

impl Session {
    /// 조건을 만족하는 이벤트가 올 때까지 기다립니다. 출력은 누적합니다.
    async fn event(&mut self, pred: impl Fn(&SessionEvent) -> bool) -> SessionEvent {
        tokio::time::timeout(WAIT, async {
            loop {
                match self.rx.recv().await.expect("session ended") {
                    Item::Output(d) => self.output.extend_from_slice(&d),
                    Item::Event(e) if pred(&e) => return e,
                    Item::Event(SessionEvent::Error { code, message }) => {
                        panic!("unexpected error {code}: {message}")
                    }
                    Item::Event(_) => {}
                }
            }
        })
        .await
        .expect("timed out waiting for event")
    }

    /// 출력에 `needle`이 나타날 때까지 기다립니다.
    async fn output_contains(&mut self, needle: &str) {
        tokio::time::timeout(WAIT, async {
            while !String::from_utf8_lossy(&self.output).contains(needle) {
                match self.rx.recv().await.expect("session ended") {
                    Item::Output(d) => self.output.extend_from_slice(&d),
                    Item::Event(SessionEvent::Closed { .. }) => panic!("closed before output"),
                    Item::Event(_) => {}
                }
            }
        })
        .await
        .unwrap_or_else(|_| {
            panic!(
                "timed out waiting for {needle:?}; got {:?}",
                String::from_utf8_lossy(&self.output)
            )
        })
    }

    async fn type_line(&self, line: &str) {
        self.manager
            .send(
                &self.id,
                ShellCommand::Input(format!("{line}\n").into_bytes()),
            )
            .await
            .unwrap();
    }

    /// 첫 접속: 지문 프롬프트를 받고 승인
    async fn accept_host_key(&mut self) -> String {
        let SessionEvent::HostKeyPrompt { fingerprint, .. } = self
            .event(|e| matches!(e, SessionEvent::HostKeyPrompt { .. }))
            .await
        else {
            unreachable!()
        };
        self.manager.respond_host_key(&self.id, true).unwrap();
        fingerprint
    }

    async fn connected(&mut self) -> SessionEvent {
        self.event(|e| matches!(e, SessionEvent::Connected { .. }))
            .await
    }

    async fn closed(&mut self) -> Option<u32> {
        match self
            .event(|e| matches!(e, SessionEvent::Closed { .. }))
            .await
        {
            SessionEvent::Closed { exit_status } => exit_status,
            _ => unreachable!(),
        }
    }

    /// 에러 이벤트 코드와 이후 Closed까지 수집 (Connected가 오면 실패)
    async fn error_code(&mut self) -> String {
        tokio::time::timeout(WAIT, async {
            loop {
                match self.rx.recv().await.expect("session ended") {
                    Item::Event(SessionEvent::Error { code, .. }) => return code,
                    Item::Event(SessionEvent::Connected { .. }) => panic!("unexpectedly connected"),
                    Item::Event(SessionEvent::HostKeyPrompt { .. }) => {
                        self.manager.respond_host_key(&self.id, true).unwrap()
                    }
                    _ => {}
                }
            }
        })
        .await
        .expect("timed out waiting for error")
    }
}

#[tokio::test]
#[ignore = "needs Docker OpenSSH: tests/docker/run-ssh-it.sh"]
async fn ssh_integration_password_shell_resize_and_exit() {
    let h = Harness::new();
    let p = profile(AuthMethod::Password);
    let (_, mut s) = h.connect(&p, Some(PASSWORD), true);

    let fingerprint = s.accept_host_key().await;
    assert!(fingerprint.starts_with("SHA256:"));
    match s.connected().await {
        SessionEvent::Connected { info } => {
            assert_eq!(info.fingerprint, fingerprint);
            assert_eq!(info.host_key_algorithm, "ssh-ed25519");
            assert!(!info.cipher.is_empty());
        }
        _ => unreachable!(),
    }
    // 인증 성공 후 입력한 비밀번호가 keyring(여기선 메모리)에 저장됨
    assert!(h.secrets.get(&p.id).unwrap().is_some());

    s.type_line("echo rivet-$((40+2)) $TERM").await;
    s.output_contains("rivet-42 xterm-256color").await;

    h.manager
        .send(
            &s.id,
            ShellCommand::Resize {
                cols: 132,
                rows: 43,
            },
        )
        .await
        .unwrap();
    s.type_line("stty size").await;
    s.output_contains("43 132").await;

    s.type_line("exit 3").await;
    assert_eq!(s.closed().await, Some(3));
}

#[tokio::test]
#[ignore = "needs Docker OpenSSH: tests/docker/run-ssh-it.sh"]
async fn ssh_integration_trusted_host_reconnects_without_prompt_using_saved_secret() {
    let h = Harness::new();
    let p = profile(AuthMethod::Password);
    let (_, mut first) = h.connect(&p, Some(PASSWORD), true);
    first.accept_host_key().await;
    first.connected().await;
    first.type_line("exit").await;
    first.closed().await;

    // 두 번째 접속: 비밀번호 입력 없이 저장된 값 사용, 호스트 키 프롬프트 없음
    let (_, mut second) = h.connect(&p, None, false);
    let event = second
        .event(|e| {
            matches!(
                e,
                SessionEvent::Connected { .. } | SessionEvent::HostKeyPrompt { .. }
            )
        })
        .await;
    assert!(matches!(event, SessionEvent::Connected { .. }));
    second.type_line("exit").await;
    second.closed().await;
}

#[tokio::test]
#[ignore = "needs Docker OpenSSH: tests/docker/run-ssh-it.sh"]
async fn ssh_integration_changed_host_key_is_blocked() {
    let h = Harness::new();
    // 다른 키를 미리 저장해 서버 키가 '바뀐' 상황을 만듦
    let other = PublicKey::from_openssh(
        "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAILivYAx9W//3gPpBQ8sYmjiLyHYjP1Ow9bzH+Vs7rGm6",
    )
    .unwrap();
    h.known_hosts.trust("127.0.0.1", port(), &other).unwrap();

    let p = profile(AuthMethod::Password);
    let (_, mut s) = h.connect(&p, Some(PASSWORD), true);
    let mut saw_changed = false;
    let code = tokio::time::timeout(WAIT, async {
        loop {
            match s.rx.recv().await.expect("session ended") {
                Item::Event(SessionEvent::HostKeyChanged { .. }) => saw_changed = true,
                Item::Event(SessionEvent::Error { code, .. }) => return code,
                Item::Event(SessionEvent::Connected { .. }) => panic!("must not connect"),
                Item::Event(SessionEvent::HostKeyPrompt { .. }) => panic!("must not prompt"),
                _ => {}
            }
        }
    })
    .await
    .unwrap();
    assert!(saw_changed);
    assert_eq!(code, "hostKeyChanged");
    assert_eq!(s.closed().await, None);
    // 차단된 접속에서 입력한 비밀번호는 저장되지 않음
    assert!(h.secrets.get(&p.id).unwrap().is_none());
}

#[tokio::test]
#[ignore = "needs Docker OpenSSH: tests/docker/run-ssh-it.sh"]
async fn ssh_integration_rejected_host_key_aborts() {
    let h = Harness::new();
    let (id, mut s) = h.connect(&profile(AuthMethod::Password), Some(PASSWORD), false);
    s.event(|e| matches!(e, SessionEvent::HostKeyPrompt { .. }))
        .await;
    h.manager.respond_host_key(&id, false).unwrap();
    assert_eq!(s.error_code().await, "hostKeyRejected");
    assert!(h.known_hosts.list().unwrap().is_empty());
}

#[tokio::test]
#[ignore = "needs Docker OpenSSH: tests/docker/run-ssh-it.sh"]
async fn ssh_integration_wrong_password_fails_and_is_not_saved() {
    let h = Harness::new();
    let p = profile(AuthMethod::Password);
    let (_, mut s) = h.connect(&p, Some("wrong-password"), true);
    assert_eq!(s.error_code().await, "authFailed");
    assert!(h.secrets.get(&p.id).unwrap().is_none());
}

#[tokio::test]
#[ignore = "needs Docker OpenSSH: tests/docker/run-ssh-it.sh"]
async fn ssh_integration_public_key_auth() {
    assert!(
        std::env::var("RIVET_IT_SSH_KEY").is_ok(),
        "RIVET_IT_SSH_KEY not set"
    );
    let h = Harness::new();
    let (_, mut s) = h.connect(&profile(AuthMethod::Key), None, false);
    s.accept_host_key().await;
    s.connected().await;
    s.type_line("whoami").await;
    s.output_contains("rivet").await;
    s.type_line("exit").await;
    assert_eq!(s.closed().await, Some(0));
}

#[tokio::test]
#[ignore = "needs Docker OpenSSH: tests/docker/run-ssh-it.sh"]
async fn ssh_integration_close_tears_down_session() {
    let h = Harness::new();
    let (id, mut s) = h.connect(&profile(AuthMethod::Password), Some(PASSWORD), false);
    s.accept_host_key().await;
    s.connected().await;
    h.manager.close(&id);
    // 태스크가 중단되어 sink가 drop되면 채널이 닫힘
    tokio::time::timeout(WAIT, async { while s.rx.recv().await.is_some() {} })
        .await
        .unwrap();
    assert!(h
        .manager
        .send(&id, ShellCommand::Input(vec![b'x']))
        .await
        .is_err());
}

/// 로그 쓰기 대상 (테스트에서 로그 내용을 검사하기 위해 메모리에 모음)
#[derive(Clone, Default)]
struct LogBuffer(Arc<Mutex<Vec<u8>>>);

impl Write for LogBuffer {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "needs Docker OpenSSH: tests/docker/run-ssh-it.sh"]
async fn ssh_integration_password_never_appears_in_trace_logs() {
    // russh는 `log` 크레이트를 쓰므로 tracing으로 넘겨받아 함께 검사합니다.
    let _ = tracing_log::LogTracer::init();
    let buffer = LogBuffer::default();
    let writer = buffer.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::TRACE)
        .with_ansi(false)
        .with_writer(move || writer.clone())
        .finish();
    let _guard = tracing::subscriber::set_default(subscriber);

    let h = Harness::new();
    let p = profile(AuthMethod::Password);
    let (_, mut s) = h.connect(&p, Some(PASSWORD), true);
    s.accept_host_key().await;
    s.connected().await;
    s.type_line("exit").await;
    s.closed().await;

    let logs = String::from_utf8_lossy(&buffer.0.lock().unwrap()).into_owned();
    assert!(
        logs.contains("ssh authenticated"),
        "log capture did not work"
    );
    assert!(!logs.contains(PASSWORD), "password leaked into logs");
}

mod sftp;
