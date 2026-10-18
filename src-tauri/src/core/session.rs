//! 세션 수명주기 관리(`SessionManager`).
//!
//! 세션마다 백그라운드 태스크 하나가 연결 → 호스트 키 확인 → 인증 → 셸 구동을 맡고,
//! UI 이벤트와 터미널 출력은 [`SessionSink`]로 내보냅니다. 탭이 닫히면 태스크를 중단해
//! 연결과 대기 데이터를 정리합니다.
//!
//! 인증된 연결은 슬롯에 보관해 파일 탐색기가 같은 연결 위에 SFTP 채널을 열 수 있게 합니다.
//! SFTP 프로필은 셸 없이 연결만 유지합니다.
//!
//! FTP/FTPS 세션은 셸이 없고 파일 시스템(`FtpFs`)만 보관합니다. FTPS와 RDP는 서버 인증서를 OS가
//! 신뢰하지 않으면 고정 지문과 비교해 바뀐 인증서는 차단하고, 처음 보는 인증서는 사용자 승인을 받습니다.
//!
//! RDP 세션은 화면 갱신을 바이너리 출력으로, 클립보드 공유 알림을 이벤트로 내보냅니다.

use std::collections::HashMap;
use std::future::Future;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError, Weak};
use std::time::Duration;

use async_trait::async_trait;
use russh::keys::PublicKey;
use secrecy::SecretString;
use serde::Serialize;
use tokio::sync::{mpsc, oneshot, OnceCell};
use tokio::task::AbortHandle;

use crate::core::cert_pins::{CertPinStore, PinStatus};
use crate::core::known_hosts::{fingerprint, HostKeyStatus, KnownHostsStore};
use crate::core::profile::{AuthMethod, Profile, Protocol};
use crate::core::secrets::SecretStore;
use crate::error::{Error, Result};
use crate::protocols::ftp::{self, FtpConnectionInfo, FtpError, FtpFs, FtpSecurity, FtpTarget};
use crate::protocols::rdp::clipboard::LocalClipboard;
use crate::protocols::rdp::{
    ClipboardDirection, DisplaySize, RdpCommand, RdpConnection, RdpConnectionInfo, RdpError,
    RdpOutput, RdpTarget,
};
use crate::protocols::sftp::SftpFs;
use crate::protocols::ssh::{
    load_private_key, ConnectionInfo, HostKeyVerdict, HostKeyVerifier, ShellCommand, SshAuth,
    SshConnection, SshError, SshTarget,
};
use crate::protocols::tls::CertTrust;
use crate::protocols::{RemoteFs, RemoteSession};

/// 처음 보는 호스트 키에 대한 사용자 응답 대기 시간
const HOST_KEY_PROMPT_TIMEOUT: Duration = Duration::from_secs(120);
/// 세션당 대기 가능한 입력 명령 수
const COMMAND_QUEUE: usize = 256;
/// 터미널 크기 상한
pub const MAX_TERMINAL_DIM: u32 = 1000;
/// 셸 없는(SFTP 전용) 세션에서 연결 끊김을 확인하는 간격
const LIVENESS_INTERVAL: Duration = Duration::from_secs(5);
/// FTP 탐색 연결이 서버 유휴 시간 제한에 걸리지 않도록 NOOP을 보내는 간격
const FTP_KEEPALIVE: Duration = Duration::from_secs(30);

pub type SessionId = String;

/// 프론트엔드로 보내는 세션 이벤트
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum SessionEvent {
    /// 처음 보는 호스트: 지문을 보여주고 승인을 기다림
    HostKeyPrompt {
        host: String,
        port: u16,
        algorithm: String,
        fingerprint: String,
    },
    /// 저장된 키와 다름: 연결을 차단함
    HostKeyChanged {
        host: String,
        port: u16,
        algorithm: String,
        expected_fingerprint: String,
        actual_fingerprint: String,
    },
    /// FTPS 인증서를 OS가 신뢰하지 않음: 지문을 보여주고 승인을 기다림
    CertPrompt {
        host: String,
        port: u16,
        fingerprint: String,
        /// `unknownIssuer`, `badName`, `expired`, `invalid`
        problem: String,
    },
    /// 고정된 인증서와 다름: 연결을 차단함
    CertChanged {
        host: String,
        port: u16,
        expected_fingerprint: String,
        actual_fingerprint: String,
    },
    Connected {
        #[serde(flatten)]
        info: ConnectionInfo,
    },
    /// 점프 호스트 인증 실패: UI가 해당 점프 호스트의 비밀번호를 다시 받음
    JumpAuthFailed {
        profile_id: String,
        name: String,
    },
    /// RDP 텍스트 클립보드를 주고받음 (`toRemote` / `toLocal`)
    Clipboard {
        direction: String,
        chars: usize,
    },
    Closed {
        exit_status: Option<u32>,
    },
    Error {
        code: String,
        message: String,
    },
}

/// 세션 출력 대상 (Tauri Channel 또는 테스트용 수집기)
pub trait SessionSink: Send + Sync + 'static {
    fn output(&self, data: &[u8]);
    fn event(&self, event: SessionEvent);

    /// 큰 출력(RDP 화면)을 복사 없이 넘길 때 씁니다.
    fn output_owned(&self, data: Vec<u8>) {
        self.output(&data);
    }
}

/// 점프 호스트 한 단계
pub struct JumpHop {
    pub profile_id: String,
    pub name: String,
    pub target: SshTarget,
    pub auth: SshAuth,
    pub on_authenticated: Option<Box<dyn FnOnce() + Send>>,
}

/// SSH 세션 시작 요청
pub struct SshSessionRequest {
    /// 거쳐 갈 점프 호스트 (접속 순서). 비어 있으면 직접 연결
    pub jumps: Vec<JumpHop>,
    pub target: SshTarget,
    pub auth: SshAuth,
    /// 셸을 열 때의 터미널 크기. `None`이면 셸 없이 연결만 유지합니다(SFTP 프로필).
    pub pty: Option<(u32, u32)>,
    /// 인증에 성공하면 한 번 호출됩니다. (새로 입력받은 비밀 값을 keyring에 저장)
    pub on_authenticated: Option<Box<dyn FnOnce() + Send>>,
}

/// 인증 성공 후에 한 번 호출하는 콜백 (새로 입력받은 비밀 값 저장)
type OnAuthenticated = Option<Box<dyn FnOnce() + Send>>;

/// 비밀 값 저장 콜백 (인증 성공 후에만 호출)
fn save_after_auth(
    profile: &Profile,
    secret: Option<SecretString>,
    from_user: bool,
    save: bool,
    secrets: &Arc<dyn SecretStore>,
) -> OnAuthenticated {
    match secret {
        Some(secret) if from_user && save => {
            let (id, secrets) = (profile.id.clone(), secrets.clone());
            Some(Box::new(move || {
                if let Err(e) = secrets.set(&id, &secret) {
                    tracing::warn!(error = %e, "could not save secret to keychain");
                }
            }))
        }
        _ => None,
    }
}

/// 프로필의 인증 수단을 만듭니다. 비밀 값이 필요한데 없으면 `PasswordRequired`/`PassphraseRequired`.
fn ssh_auth(
    profile: &Profile,
    provided_secret: Option<SecretString>,
    save_secret: bool,
    secrets: &Arc<dyn SecretStore>,
) -> Result<(SshAuth, OnAuthenticated)> {
    let from_user = provided_secret.is_some();
    let secret = match provided_secret {
        Some(s) => Some(s),
        None if profile.auth_method == AuthMethod::Agent => None,
        None => secrets.get(&profile.id)?,
    };

    let auth = match profile.auth_method {
        AuthMethod::Password => SshAuth::Password(secret.clone().ok_or(Error::PasswordRequired)?),
        AuthMethod::Key => {
            let path = profile
                .key_path
                .as_deref()
                .ok_or_else(|| Error::Invalid("key file is not set".into()))?;
            SshAuth::Key(Arc::new(load_private_key(
                &expand_home(path),
                secret.as_ref(),
            )?))
        }
        AuthMethod::Agent => SshAuth::Agent,
    };
    Ok((
        auth,
        save_after_auth(profile, secret, from_user, save_secret, secrets),
    ))
}

fn ssh_target(profile: &Profile) -> SshTarget {
    SshTarget {
        host: profile.host.clone(),
        port: profile.port,
        username: profile.username.clone(),
    }
}

/// 점프 호스트에 쓸 비밀 값 (점프 프로필 ID → 사용자가 방금 입력한 값)
#[derive(Default)]
pub struct JumpSecrets {
    pub provided: HashMap<String, SecretString>,
    pub save: bool,
}

/// 프로필과 저장된/입력받은 비밀 값으로 SSH 세션 요청을 만듭니다.
///
/// 비밀 값이 필요한데 없으면 `PasswordRequired`/`PassphraseRequired`를 돌려주어
/// UI가 입력을 받아 다시 호출하게 합니다. 점프 호스트의 비밀 값이 없으면 `JumpSecretRequired`.
/// `save_secret`이 참이면 인증 성공 후에만 저장합니다.
#[allow(clippy::too_many_arguments)]
pub fn prepare_ssh_request(
    profile: &Profile,
    jumps: &[Profile],
    provided_secret: Option<SecretString>,
    save_secret: bool,
    mut jump_secrets: JumpSecrets,
    secrets: Arc<dyn SecretStore>,
    cols: u32,
    rows: u32,
) -> Result<SshSessionRequest> {
    let pty = match profile.protocol {
        Protocol::Ssh => Some(check_terminal_size(cols, rows)?),
        Protocol::Sftp => None,
        _ => return Err(Error::Invalid("profile is not an SSH/SFTP profile".into())),
    };

    let mut hops = Vec::with_capacity(jumps.len());
    for jump in jumps {
        let provided = jump_secrets.provided.remove(&jump.id);
        let (auth, on_authenticated) = ssh_auth(jump, provided, jump_secrets.save, &secrets)
            .map_err(|e| match e.code() {
                "passwordRequired" | "passphraseRequired" => Error::JumpSecretRequired {
                    profile_id: jump.id.clone(),
                    name: jump.name.clone(),
                    passphrase: e.code() == "passphraseRequired",
                },
                _ => e,
            })?;
        hops.push(JumpHop {
            profile_id: jump.id.clone(),
            name: jump.name.clone(),
            target: ssh_target(jump),
            auth,
            on_authenticated,
        });
    }

    let (auth, on_authenticated) = ssh_auth(profile, provided_secret, save_secret, &secrets)?;
    Ok(SshSessionRequest {
        jumps: hops,
        target: ssh_target(profile),
        auth,
        pty,
        on_authenticated,
    })
}

/// FTP 세션 시작 요청
pub struct FtpSessionRequest {
    pub target: FtpTarget,
    pub password: SecretString,
    pub on_authenticated: Option<Box<dyn FnOnce() + Send>>,
}

/// 비밀번호를 묻지 않는 익명 FTP 사용자명
fn is_anonymous(user: &str) -> bool {
    user.eq_ignore_ascii_case("anonymous") || user.eq_ignore_ascii_case("ftp")
}

/// 프로필로 FTP/FTPS 세션 요청을 만듭니다. FTPS는 포트 990이면 암시적 TLS, 아니면 AUTH TLS입니다.
pub fn prepare_ftp_request(
    profile: &Profile,
    provided_secret: Option<SecretString>,
    save_secret: bool,
    secrets: Arc<dyn SecretStore>,
) -> Result<FtpSessionRequest> {
    let security = match profile.protocol {
        Protocol::Ftp => FtpSecurity::Plain,
        Protocol::Ftps if profile.port == 990 => FtpSecurity::Implicit,
        Protocol::Ftps => FtpSecurity::Explicit,
        _ => return Err(Error::Invalid("profile is not an FTP/FTPS profile".into())),
    };
    let username = if profile.username.is_empty() {
        "anonymous".to_owned()
    } else {
        profile.username.clone()
    };
    ftp::check_arg(&username)
        .map_err(|_| Error::Invalid("username contains line breaks".into()))?;

    let from_user = provided_secret.is_some();
    let secret = match provided_secret {
        Some(s) => Some(s),
        None => secrets.get(&profile.id)?,
    };
    let password = match secret.clone() {
        Some(p) => p,
        None if is_anonymous(&username) => SecretString::from("rivet@"),
        None => return Err(Error::PasswordRequired),
    };
    if ftp::check_arg(secrecy::ExposeSecret::expose_secret(&password)).is_err() {
        return Err(Error::Invalid("password contains line breaks".into()));
    }

    let on_authenticated: Option<Box<dyn FnOnce() + Send>> = match secret {
        Some(secret) if from_user && save_secret => {
            let id = profile.id.clone();
            Some(Box::new(move || {
                if let Err(e) = secrets.set(&id, &secret) {
                    tracing::warn!(error = %e, "could not save secret to keychain");
                }
            }))
        }
        _ => None,
    };

    Ok(FtpSessionRequest {
        target: FtpTarget {
            host: profile.host.clone(),
            port: profile.port,
            username,
            security,
        },
        password,
        on_authenticated,
    })
}

/// RDP 세션 시작 요청
pub struct RdpSessionRequest {
    pub target: RdpTarget,
    pub password: SecretString,
    pub size: DisplaySize,
    pub clipboard: bool,
    pub on_authenticated: Option<Box<dyn FnOnce() + Send>>,
}

/// 프로필로 RDP 세션 요청을 만듭니다. 비밀번호가 없으면 `PasswordRequired`를 돌려줍니다.
pub fn prepare_rdp_request(
    profile: &Profile,
    provided_secret: Option<SecretString>,
    save_secret: bool,
    secrets: Arc<dyn SecretStore>,
    size: DisplaySize,
) -> Result<RdpSessionRequest> {
    if profile.protocol != Protocol::Rdp {
        return Err(Error::Invalid("profile is not an RDP profile".into()));
    }
    let from_user = provided_secret.is_some();
    let password = match provided_secret {
        Some(s) => s,
        None => secrets.get(&profile.id)?.ok_or(Error::PasswordRequired)?,
    };
    let on_authenticated: Option<Box<dyn FnOnce() + Send>> = if from_user && save_secret {
        let (id, secret) = (profile.id.clone(), password.clone());
        Some(Box::new(move || {
            if let Err(e) = secrets.set(&id, &secret) {
                tracing::warn!(error = %e, "could not save secret to keychain");
            }
        }))
    } else {
        None
    };
    Ok(RdpSessionRequest {
        target: RdpTarget {
            host: profile.host.clone(),
            port: profile.port,
            username: profile.username.clone(),
        },
        password,
        size: size.normalized(),
        clipboard: true,
        on_authenticated,
    })
}

fn rdp_connection_info(info: &RdpConnectionInfo) -> ConnectionInfo {
    let auth = if info.nla {
        "NLA (CredSSP)"
    } else {
        "원격 로그인 화면"
    };
    ConnectionInfo {
        transport: if info.trust == CertTrust::Pinned {
            "tlsPinned"
        } else {
            "tlsCa"
        }
        .into(),
        kex: format!("RDP {} · {auth}", info.tls_version),
        cipher: String::new(),
        host_key_algorithm: "X.509".into(),
        fingerprint: info.fingerprint.clone(),
        via: Vec::new(),
    }
}

fn ftp_connection_info(info: &FtpConnectionInfo) -> ConnectionInfo {
    let (transport, kex) = match (info.security, info.trust) {
        (FtpSecurity::Plain, _) => ("plain", "FTP"),
        (FtpSecurity::Explicit, trust) => (
            if trust == Some(CertTrust::Pinned) {
                "tlsPinned"
            } else {
                "tlsCa"
            },
            "FTPS AUTH TLS",
        ),
        (FtpSecurity::Implicit, trust) => (
            if trust == Some(CertTrust::Pinned) {
                "tlsPinned"
            } else {
                "tlsCa"
            },
            "FTPS implicit TLS",
        ),
    };
    ConnectionInfo {
        transport: transport.into(),
        kex: kex.into(),
        cipher: String::new(),
        host_key_algorithm: if info.fingerprint.is_some() {
            "X.509".into()
        } else {
            String::new()
        },
        fingerprint: info.fingerprint.clone().unwrap_or_default(),
        via: Vec::new(),
    }
}

pub fn check_terminal_size(cols: u32, rows: u32) -> Result<(u32, u32)> {
    let ok = |n: u32| (1..=MAX_TERMINAL_DIM).contains(&n);
    if ok(cols) && ok(rows) {
        Ok((cols, rows))
    } else {
        Err(Error::Invalid("terminal size out of range".into()))
    }
}

/// `~/`로 시작하는 경로를 홈 디렉터리 기준으로 펼칩니다.
fn expand_home(path: &str) -> PathBuf {
    match (path.strip_prefix("~/"), std::env::home_dir()) {
        (Some(rest), Some(home)) => home.join(rest),
        _ => PathBuf::from(path),
    }
}

/// 세션 종류별 명령 채널
enum Commands {
    Shell(mpsc::Sender<ShellCommand>),
    Rdp(mpsc::Sender<RdpCommand>),
}

struct Slot {
    commands: Commands,
    host_key_reply: Option<oneshot::Sender<bool>>,
    task: Option<AbortHandle>,
    /// 인증이 끝난 뒤 채워짐
    conn: Option<Arc<SshConnection>>,
    /// 처음 요청할 때 한 번만 여는 SFTP
    fs: Arc<OnceCell<Arc<dyn RemoteFs>>>,
}

impl Slot {
    fn new(commands: Commands) -> Self {
        Self {
            commands,
            host_key_reply: None,
            task: None,
            conn: None,
            fs: Arc::new(OnceCell::new()),
        }
    }

    /// 태스크를 중단하고 연결을 정상 종료합니다. (tokio 런타임 밖이면 drop으로 정리)
    fn shutdown(self) {
        if let Some(task) = self.task {
            task.abort();
        }
        if let (Some(conn), Ok(rt)) = (self.conn, tokio::runtime::Handle::try_current()) {
            rt.spawn(async move { conn.disconnect().await });
        }
    }
}

#[derive(Default)]
pub struct SessionManager {
    slots: Mutex<HashMap<SessionId, Slot>>,
}

impl SessionManager {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    fn slots(&self) -> std::sync::MutexGuard<'_, HashMap<SessionId, Slot>> {
        self.slots.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// 세션을 백그라운드에서 시작하고 즉시 ID를 돌려줍니다. tokio 런타임 안에서 호출해야 합니다.
    pub fn spawn_ssh(
        self: &Arc<Self>,
        request: SshSessionRequest,
        known_hosts: Arc<KnownHostsStore>,
        sink: Arc<dyn SessionSink>,
    ) -> SessionId {
        let id = uuid::Uuid::new_v4().to_string();
        let (tx, rx) = mpsc::channel(COMMAND_QUEUE);
        self.slots()
            .insert(id.clone(), Slot::new(Commands::Shell(tx)));

        let verifier = Arc::new(InteractiveVerifier {
            store: known_hosts,
            manager: Arc::downgrade(self),
            session_id: id.clone(),
            sink: sink.clone(),
        });
        let manager = Arc::downgrade(self);
        let session_id = id.clone();
        let task = tokio::spawn(async move {
            let store = {
                let (manager, session_id) = (manager.clone(), session_id.clone());
                move |conn: Arc<SshConnection>| match manager.upgrade() {
                    Some(m) => m.set_connection(&session_id, conn),
                    None => false,
                }
            };
            run_ssh_session(request, verifier, rx, sink, store).await;
            if let Some(m) = manager.upgrade() {
                m.slots().remove(&session_id);
            }
        });
        if let Some(slot) = self.slots().get_mut(&id) {
            slot.task = Some(task.abort_handle());
        }
        id
    }

    pub async fn send(&self, id: &str, command: ShellCommand) -> Result<()> {
        let sender = match self.slots().get(id).map(|s| &s.commands) {
            Some(Commands::Shell(tx)) => tx.clone(),
            Some(Commands::Rdp(_)) => return Err(Error::Invalid("not a shell session".into())),
            None => return Err(Error::NotFound(format!("session {id}"))),
        };
        sender
            .send(command)
            .await
            .map_err(|_| Error::NotFound(format!("session {id}")))
    }

    /// RDP 세션에 명령을 보냅니다.
    pub async fn send_rdp(&self, id: &str, command: RdpCommand) -> Result<()> {
        let sender = match self.slots().get(id).map(|s| &s.commands) {
            Some(Commands::Rdp(tx)) => tx.clone(),
            Some(Commands::Shell(_)) => return Err(Error::Invalid("not an RDP session".into())),
            None => return Err(Error::NotFound(format!("session {id}"))),
        };
        sender
            .send(command)
            .await
            .map_err(|_| Error::NotFound(format!("session {id}")))
    }

    /// RDP 세션을 백그라운드에서 시작하고 즉시 ID를 돌려줍니다.
    pub fn spawn_rdp(
        self: &Arc<Self>,
        request: RdpSessionRequest,
        pins: Arc<CertPinStore>,
        sink: Arc<dyn SessionSink>,
        clipboard: Arc<dyn LocalClipboard>,
    ) -> SessionId {
        let id = uuid::Uuid::new_v4().to_string();
        let (tx, rx) = mpsc::channel(COMMAND_QUEUE);
        self.slots()
            .insert(id.clone(), Slot::new(Commands::Rdp(tx)));
        let manager = Arc::downgrade(self);
        let session_id = id.clone();
        let task = tokio::spawn(async move {
            run_rdp_session(
                request,
                pins,
                manager.clone(),
                session_id.clone(),
                rx,
                sink,
                clipboard,
            )
            .await;
            if let Some(m) = manager.upgrade() {
                m.slots().remove(&session_id);
            }
        });
        if let Some(slot) = self.slots().get_mut(&id) {
            slot.task = Some(task.abort_handle());
        }
        id
    }

    /// 호스트 키 승인 대화상자의 응답을 전달합니다.
    pub fn respond_host_key(&self, id: &str, accept: bool) -> Result<()> {
        let reply = self
            .slots()
            .get_mut(id)
            .and_then(|s| s.host_key_reply.take())
            .ok_or_else(|| Error::NotFound(format!("pending host key prompt for {id}")))?;
        let _ = reply.send(accept);
        Ok(())
    }

    /// FTP/FTPS 세션을 백그라운드에서 시작하고 즉시 ID를 돌려줍니다.
    pub fn spawn_ftp(
        self: &Arc<Self>,
        request: FtpSessionRequest,
        pins: Arc<CertPinStore>,
        sink: Arc<dyn SessionSink>,
    ) -> SessionId {
        let id = uuid::Uuid::new_v4().to_string();
        let (tx, rx) = mpsc::channel(COMMAND_QUEUE);
        self.slots()
            .insert(id.clone(), Slot::new(Commands::Shell(tx)));
        let manager = Arc::downgrade(self);
        let session_id = id.clone();
        let task = tokio::spawn(async move {
            run_ftp_session(request, pins, manager.clone(), session_id.clone(), rx, sink).await;
            if let Some(m) = manager.upgrade() {
                m.slots().remove(&session_id);
            }
        });
        if let Some(slot) = self.slots().get_mut(&id) {
            slot.task = Some(task.abort_handle());
        }
        id
    }

    /// 세션의 원격 파일 시스템. SSH 세션이면 같은 연결 위에 SFTP를 처음 한 번 엽니다.
    pub async fn remote_fs(&self, id: &str) -> Result<Arc<dyn RemoteFs>> {
        let (conn, cell) = {
            let slots = self.slots();
            let slot = slots
                .get(id)
                .ok_or_else(|| Error::NotFound(format!("session {id}")))?;
            if let Some(fs) = slot.fs.get() {
                return Ok(fs.clone());
            }
            let conn = slot
                .conn
                .clone()
                .ok_or_else(|| Error::Invalid("session is not connected yet".into()))?;
            (conn, slot.fs.clone())
        };
        let fs = cell
            .get_or_try_init(|| async {
                let fs = SftpFs::open(&conn).await?;
                tracing::info!("sftp opened");
                Ok::<_, Error>(Arc::new(fs) as Arc<dyn RemoteFs>)
            })
            .await?;
        Ok(fs.clone())
    }

    /// 세션을 즉시 종료합니다. 대기 중인 호스트 키 승인은 거부로 처리됩니다.
    pub fn close(&self, id: &str) {
        let slot = self.slots().remove(id);
        if let Some(slot) = slot {
            slot.shutdown();
        }
    }

    pub fn close_all(&self) {
        let slots: Vec<Slot> = self.slots().drain().map(|(_, s)| s).collect();
        for slot in slots {
            slot.shutdown();
        }
    }

    fn set_fs(&self, id: &str, fs: Arc<dyn RemoteFs>) -> bool {
        self.slots()
            .get(id)
            .is_some_and(|slot| slot.fs.set(fs).is_ok())
    }

    fn set_connection(&self, id: &str, conn: Arc<SshConnection>) -> bool {
        match self.slots().get_mut(id) {
            Some(slot) => {
                slot.conn = Some(conn);
                true
            }
            None => false,
        }
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.slots().len()
    }

    fn set_host_key_reply(&self, id: &str, reply: oneshot::Sender<bool>) -> bool {
        match self.slots().get_mut(id) {
            Some(slot) => {
                slot.host_key_reply = Some(reply);
                true
            }
            None => false,
        }
    }
}

async fn run_ssh_session(
    request: SshSessionRequest,
    verifier: Arc<InteractiveVerifier>,
    mut commands: mpsc::Receiver<ShellCommand>,
    sink: Arc<dyn SessionSink>,
    store_connection: impl FnOnce(Arc<SshConnection>) -> bool,
) {
    let SshSessionRequest {
        jumps,
        target,
        auth,
        pty,
        on_authenticated,
    } = request;

    let result = async {
        // 점프 호스트를 차례로 연결합니다. 각 단계의 호스트 키도 똑같이 검증합니다.
        let mut via: Option<Arc<SshConnection>> = None;
        let mut via_names = Vec::new();
        for hop in jumps {
            let JumpHop {
                profile_id,
                name,
                target: hop_target,
                auth: hop_auth,
                on_authenticated: hop_saved,
            } = hop;
            let conn = match via.take() {
                None => SshConnection::connect(&hop_target, &hop_auth, verifier.clone()).await,
                Some(prev) => SshConnection::connect_via(prev, &hop_target, &hop_auth, verifier.clone()).await,
            };
            drop(hop_auth);
            let conn = conn.map_err(|source| {
                if matches!(source, SshError::AuthFailed) {
                    sink.event(SessionEvent::JumpAuthFailed {
                        profile_id: profile_id.clone(),
                        name: name.clone(),
                    });
                }
                Error::Jump {
                    name: name.clone(),
                    source,
                }
            })?;
            tracing::info!(host = %hop_target.host, port = hop_target.port, "ssh jump host authenticated");
            if let Some(callback) = hop_saved {
                callback();
            }
            via_names.push(name);
            via = Some(Arc::new(conn));
        }

        let conn = match via {
            None => SshConnection::connect(&target, &auth, verifier).await,
            Some(jump) => SshConnection::connect_via(jump, &target, &auth, verifier).await,
        };
        // 인증이 끝나면 비밀 값을 메모리에서 바로 지웁니다.
        drop(auth);
        let mut conn = conn?;
        conn.set_via_names(via_names);
        let conn = Arc::new(conn);
        tracing::info!(host = %target.host, port = target.port, "ssh authenticated");
        if let Some(callback) = on_authenticated {
            callback();
        }
        if !store_connection(conn.clone()) {
            // 인증 도중 탭이 닫힘
            conn.disconnect().await;
            return Ok(None);
        }
        sink.event(SessionEvent::Connected {
            info: conn.info().clone(),
        });

        let status = match pty {
            Some((cols, rows)) => {
                let channel = conn.open_shell(cols, rows).await?;
                let output_sink = sink.clone();
                SshConnection::run_shell(channel, &mut commands, move |data| {
                    output_sink.output(data)
                })
                .await
                .map_err(Error::from)
            }
            None => hold_connection(&conn, &mut commands).await,
        };
        conn.disconnect().await;
        status
    }
    .await;

    match result {
        Ok(exit_status) => sink.event(SessionEvent::Closed { exit_status }),
        Err(e) => {
            tracing::warn!(host = %target.host, port = target.port, code = e.code(), "ssh session failed");
            sink.event(SessionEvent::Error {
                code: e.code().into(),
                message: e.to_string(),
            });
            sink.event(SessionEvent::Closed { exit_status: None });
        }
    }
}

async fn run_ftp_session(
    request: FtpSessionRequest,
    pins: Arc<CertPinStore>,
    manager: Weak<SessionManager>,
    id: SessionId,
    mut commands: mpsc::Receiver<ShellCommand>,
    sink: Arc<dyn SessionSink>,
) {
    let FtpSessionRequest {
        target,
        password,
        on_authenticated,
    } = request;

    let result = async {
        let fs = connect_ftp(&target, &password, &pins, &manager, &id, &sink).await?;
        tracing::info!(host = %target.host, port = target.port, "ftp authenticated");
        if let Some(callback) = on_authenticated {
            callback();
        }
        let info = ftp_connection_info(fs.info());
        let fs = Arc::new(fs);
        let stored = manager.upgrade().is_some_and(|m| m.set_fs(&id, fs.clone()));
        if !stored {
            return Ok(());
        }
        sink.event(SessionEvent::Connected { info });

        let mut tick = tokio::time::interval(FTP_KEEPALIVE);
        tick.tick().await;
        loop {
            tokio::select! {
                cmd = commands.recv() => if cmd.is_none() { return Ok::<_, Error>(()) },
                _ = tick.tick() => fs.keepalive().await,
            }
        }
    }
    .await;

    match result {
        Ok(()) => sink.event(SessionEvent::Closed { exit_status: None }),
        Err(e) => {
            tracing::warn!(host = %target.host, port = target.port, code = e.code(), "ftp session failed");
            sink.event(SessionEvent::Error {
                code: e.code().into(),
                message: e.to_string(),
            });
            sink.event(SessionEvent::Closed { exit_status: None });
        }
    }
}

/// TLS 접속 시도 결과
enum TlsAttempt<T> {
    Connected(T),
    /// 인증서를 신뢰할 수 없음 (OS 검증 실패, 고정 지문 없음/다름)
    Untrusted {
        fingerprint: String,
        problem: String,
    },
    Failed(Error),
}

/// 인증서 확인 실패를 프로토콜별 에러로 바꿀 때 쓰는 종류
enum CertFailure {
    Changed { expected: String, actual: String },
    Rejected,
    Tls(String),
}

/// TLS 접속. 신뢰할 수 없는 인증서는 고정 지문과 비교하거나 사용자 승인을 받습니다(TOFU).
///
/// `attempt`는 고정 지문을 받아 한 번 접속해 봅니다. 처음 보는 인증서를 사용자가 승인하면
/// 지문을 고정하고 한 번 더 접속합니다. 고정과 다른 인증서는 묻지 않고 차단합니다.
#[allow(clippy::too_many_arguments)]
async fn connect_pinned<T, Fut>(
    host: &str,
    port: u16,
    pins: &CertPinStore,
    manager: &Weak<SessionManager>,
    id: &str,
    sink: &Arc<dyn SessionSink>,
    mut attempt: impl FnMut(Option<String>) -> Fut,
    failure: impl Fn(CertFailure) -> Error,
) -> Result<T>
where
    Fut: Future<Output = TlsAttempt<T>>,
{
    for _ in 0..2 {
        let pinned = pins.pinned(host, port)?;
        let (fingerprint, problem) = match attempt(pinned).await {
            TlsAttempt::Connected(conn) => return Ok(conn),
            TlsAttempt::Untrusted {
                fingerprint,
                problem,
            } => (fingerprint, problem),
            TlsAttempt::Failed(e) => return Err(e),
        };
        match pins.check(host, port, &fingerprint)? {
            PinStatus::Changed { known } => {
                tracing::warn!(host, port, "certificate changed; connection blocked");
                sink.event(SessionEvent::CertChanged {
                    host: host.to_owned(),
                    port,
                    expected_fingerprint: known.fingerprint.clone(),
                    actual_fingerprint: fingerprint.clone(),
                });
                return Err(failure(CertFailure::Changed {
                    expected: known.fingerprint,
                    actual: fingerprint,
                }));
            }
            // 고정 지문과 같은데도 거부됨 (서명 오류 등)
            PinStatus::Pinned => {
                return Err(failure(CertFailure::Tls(format!(
                    "certificate rejected ({problem})"
                ))))
            }
            PinStatus::Unknown => {
                let (tx, rx) = oneshot::channel();
                let registered = manager
                    .upgrade()
                    .is_some_and(|m| m.set_host_key_reply(id, tx));
                if !registered {
                    return Err(failure(CertFailure::Rejected));
                }
                sink.event(SessionEvent::CertPrompt {
                    host: host.to_owned(),
                    port,
                    fingerprint: fingerprint.clone(),
                    problem,
                });
                match tokio::time::timeout(HOST_KEY_PROMPT_TIMEOUT, rx).await {
                    Ok(Ok(true)) => pins.pin(host, port, &fingerprint)?,
                    _ => return Err(failure(CertFailure::Rejected)),
                }
            }
        }
    }
    Err(failure(CertFailure::Tls(
        "certificate is still not trusted".into(),
    )))
}

/// FTP 접속 (FTPS 인증서 확인 포함)
async fn connect_ftp(
    target: &FtpTarget,
    password: &SecretString,
    pins: &CertPinStore,
    manager: &Weak<SessionManager>,
    id: &str,
    sink: &Arc<dyn SessionSink>,
) -> Result<FtpFs> {
    connect_pinned(
        &target.host,
        target.port,
        pins,
        manager,
        id,
        sink,
        |pinned| async move {
            match FtpFs::connect(target.clone(), password.clone(), pinned).await {
                Ok(fs) => TlsAttempt::Connected(fs),
                Err(FtpError::CertUntrusted {
                    fingerprint,
                    problem,
                }) => TlsAttempt::Untrusted {
                    fingerprint,
                    problem,
                },
                Err(e) => TlsAttempt::Failed(e.into()),
            }
        },
        |f| {
            match f {
                CertFailure::Changed { expected, actual } => {
                    FtpError::CertChanged { expected, actual }
                }
                CertFailure::Rejected => FtpError::CertRejected,
                CertFailure::Tls(m) => FtpError::Tls(m),
            }
            .into()
        },
    )
    .await
}

/// RDP 세션 출력을 [`SessionSink`]로 보냅니다.
struct RdpSink(Arc<dyn SessionSink>);

impl RdpOutput for RdpSink {
    fn binary(&self, data: Vec<u8>) {
        self.0.output_owned(data);
    }

    fn clipboard(&self, direction: ClipboardDirection, chars: usize) {
        self.0.event(SessionEvent::Clipboard {
            direction: match direction {
                ClipboardDirection::ToRemote => "toRemote",
                ClipboardDirection::ToLocal => "toLocal",
            }
            .into(),
            chars,
        });
    }
}

async fn run_rdp_session(
    request: RdpSessionRequest,
    pins: Arc<CertPinStore>,
    manager: Weak<SessionManager>,
    id: SessionId,
    mut commands: mpsc::Receiver<RdpCommand>,
    sink: Arc<dyn SessionSink>,
    clipboard: Arc<dyn LocalClipboard>,
) {
    let RdpSessionRequest {
        target,
        password,
        size,
        clipboard: share_clipboard,
        on_authenticated,
    } = request;

    let result = async {
        let conn = connect_pinned(
            &target.host,
            target.port,
            &pins,
            &manager,
            &id,
            &sink,
            |pinned| {
                let (target, password) = (&target, &password);
                async move {
                    match RdpConnection::connect(target, password, pinned, size, share_clipboard).await {
                        Ok(conn) => TlsAttempt::Connected(conn),
                        Err(RdpError::CertUntrusted {
                            fingerprint,
                            problem,
                        }) => TlsAttempt::Untrusted {
                            fingerprint,
                            problem,
                        },
                        Err(e) => TlsAttempt::Failed(e.into()),
                    }
                }
            },
            |f| {
                match f {
                    CertFailure::Changed { expected, actual } => {
                        RdpError::CertChanged { expected, actual }
                    }
                    CertFailure::Rejected => RdpError::CertRejected,
                    CertFailure::Tls(m) => RdpError::Tls(m),
                }
                .into()
            },
        )
        .await;
        // 인증이 끝나면 비밀번호를 메모리에서 바로 지웁니다.
        drop(password);
        let conn = conn?;
        tracing::info!(host = %target.host, port = target.port, nla = conn.info().nla, "rdp connected");
        if let Some(callback) = on_authenticated {
            callback();
        }
        if manager.upgrade().is_none() {
            return Ok(());
        }
        sink.event(SessionEvent::Connected {
            info: rdp_connection_info(conn.info()),
        });
        let out = RdpSink(sink.clone());
        conn.run(&mut commands, &out, clipboard)
            .await
            .map_err(Error::from)
    }
    .await;

    match result {
        Ok(()) => sink.event(SessionEvent::Closed { exit_status: None }),
        Err(e) => {
            tracing::warn!(host = %target.host, port = target.port, code = e.code(), "rdp session failed");
            sink.event(SessionEvent::Error {
                code: e.code().into(),
                message: e.to_string(),
            });
            sink.event(SessionEvent::Closed { exit_status: None });
        }
    }
}

/// 셸 없는 세션: 탭이 닫히거나 서버가 연결을 끊을 때까지 기다립니다.
async fn hold_connection(
    conn: &SshConnection,
    commands: &mut mpsc::Receiver<ShellCommand>,
) -> Result<Option<u32>> {
    let mut tick = tokio::time::interval(LIVENESS_INTERVAL);
    loop {
        tokio::select! {
            cmd = commands.recv() => if cmd.is_none() { return Ok(None) },
            _ = tick.tick() => if conn.is_closed() {
                return Err(Error::Fs(crate::protocols::FsError::ConnectionLost));
            },
        }
    }
}

/// known_hosts 확인 후 처음 보는 키는 UI에 물어보는 검증기
struct InteractiveVerifier {
    store: Arc<KnownHostsStore>,
    manager: Weak<SessionManager>,
    session_id: SessionId,
    sink: Arc<dyn SessionSink>,
}

#[async_trait]
impl HostKeyVerifier for InteractiveVerifier {
    async fn verify(&self, host: &str, port: u16, key: &PublicKey) -> HostKeyVerdict {
        let actual = fingerprint(key);
        let algorithm = key.algorithm().as_str().to_owned();
        match self.store.check(host, port, key) {
            Ok(HostKeyStatus::Trusted) => HostKeyVerdict::Accept,
            Ok(HostKeyStatus::Changed { known }) => {
                tracing::warn!(host, port, "host key changed; connection blocked");
                self.sink.event(SessionEvent::HostKeyChanged {
                    host: host.into(),
                    port,
                    algorithm,
                    expected_fingerprint: known.fingerprint.clone(),
                    actual_fingerprint: actual.clone(),
                });
                HostKeyVerdict::Changed {
                    expected: known.fingerprint,
                    actual,
                }
            }
            Ok(HostKeyStatus::Unknown) => {
                let (tx, rx) = oneshot::channel();
                let registered = self
                    .manager
                    .upgrade()
                    .is_some_and(|m| m.set_host_key_reply(&self.session_id, tx));
                if !registered {
                    return HostKeyVerdict::Rejected;
                }
                self.sink.event(SessionEvent::HostKeyPrompt {
                    host: host.into(),
                    port,
                    algorithm,
                    fingerprint: actual,
                });
                match tokio::time::timeout(HOST_KEY_PROMPT_TIMEOUT, rx).await {
                    Ok(Ok(true)) => match self.store.trust(host, port, key) {
                        Ok(()) => HostKeyVerdict::Accept,
                        Err(e) => {
                            tracing::error!(error = %e, "could not save host key");
                            HostKeyVerdict::Rejected
                        }
                    },
                    _ => HostKeyVerdict::Rejected,
                }
            }
            Err(e) => {
                tracing::error!(error = %e, "known_hosts lookup failed");
                HostKeyVerdict::Rejected
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::db::Database;
    use crate::core::secrets::MemoryStore;
    use secrecy::ExposeSecret;

    /// 점프 호스트 없는 요청
    fn prep(
        p: &Profile,
        secret: Option<SecretString>,
        save: bool,
        secrets: Arc<dyn SecretStore>,
        cols: u32,
        rows: u32,
    ) -> Result<SshSessionRequest> {
        prepare_ssh_request(
            p,
            &[],
            secret,
            save,
            JumpSecrets::default(),
            secrets,
            cols,
            rows,
        )
    }

    fn profile(auth: AuthMethod) -> Profile {
        Profile {
            id: "p1".into(),
            name: "web".into(),
            protocol: Protocol::Ssh,
            host: "127.0.0.1".into(),
            port: 22,
            username: "deploy".into(),
            auth_method: auth,
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

    #[test]
    fn password_profile_without_secret_requires_password() {
        let secrets: Arc<dyn SecretStore> = Arc::new(MemoryStore::default());
        let err = prep(&profile(AuthMethod::Password), None, false, secrets, 80, 24)
            .err()
            .unwrap();
        assert!(matches!(err, Error::PasswordRequired));
    }

    #[test]
    fn stored_password_is_used_and_not_resaved() {
        let store = Arc::new(MemoryStore::default());
        store.set("p1", &SecretString::from("stored")).unwrap();
        let req = prep(&profile(AuthMethod::Password), None, true, store, 80, 24).unwrap();
        match req.auth {
            SshAuth::Password(p) => assert_eq!(p.expose_secret(), "stored"),
            other => panic!("unexpected {other:?}"),
        }
        assert!(req.on_authenticated.is_none());
    }

    #[test]
    fn provided_password_is_saved_only_after_authentication() {
        let store = Arc::new(MemoryStore::default());
        let req = prep(
            &profile(AuthMethod::Password),
            Some(SecretString::from("typed")),
            true,
            store.clone(),
            80,
            24,
        )
        .unwrap();
        assert!(store.get("p1").unwrap().is_none());
        (req.on_authenticated.unwrap())();
        assert_eq!(store.get("p1").unwrap().unwrap().expose_secret(), "typed");
    }

    #[test]
    fn provided_password_is_not_saved_when_not_requested() {
        let store = Arc::new(MemoryStore::default());
        let req = prep(
            &profile(AuthMethod::Password),
            Some(SecretString::from("typed")),
            false,
            store,
            80,
            24,
        )
        .unwrap();
        assert!(req.on_authenticated.is_none());
    }

    #[test]
    fn encrypted_key_without_passphrase_requires_passphrase() {
        let mut p = profile(AuthMethod::Key);
        p.key_path = Some(
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/encrypted_ed25519"
            )
            .into(),
        );
        let secrets: Arc<dyn SecretStore> = Arc::new(MemoryStore::default());
        let err = prep(&p, None, false, secrets, 80, 24).err().unwrap();
        assert_eq!(err.code(), "passphraseRequired");
    }

    #[test]
    fn rejects_non_ssh_profiles_and_bad_sizes() {
        let secrets: Arc<dyn SecretStore> = Arc::new(MemoryStore::default());
        let mut p = profile(AuthMethod::Agent);
        assert!(prep(&p, None, false, secrets.clone(), 0, 24).is_err());
        assert!(prep(&p, None, false, secrets.clone(), 80, 5000).is_err());
        p.protocol = Protocol::Rdp;
        assert!(prep(&p, None, false, secrets.clone(), 80, 24).is_err());
        // SFTP 프로필은 셸이 없어 터미널 크기를 보지 않음
        p.protocol = Protocol::Sftp;
        let req = prep(&p, None, false, secrets, 0, 0).unwrap();
        assert!(req.pty.is_none());
    }

    #[test]
    fn jump_hosts_get_their_own_secrets_and_report_missing_ones() {
        let store = Arc::new(MemoryStore::default());
        let mut jump = profile(AuthMethod::Password);
        jump.id = "jump".into();
        jump.name = "bastion".into();
        let target = profile(AuthMethod::Agent);
        let secrets: Arc<dyn SecretStore> = store.clone();

        // 점프 호스트 비밀번호가 없으면 어느 프로필인지 알려 줌
        let err = prepare_ssh_request(
            &target,
            &[jump.clone()],
            None,
            false,
            JumpSecrets::default(),
            secrets.clone(),
            80,
            24,
        )
        .err()
        .unwrap();
        assert!(
            matches!(&err, Error::JumpSecretRequired { profile_id, name, passphrase: false } if profile_id == "jump" && name == "bastion")
        );
        assert_eq!(err.code(), "jumpPasswordRequired");

        // 입력받은 값으로 요청을 만들고, 인증 후에만 저장
        let mut provided = HashMap::new();
        provided.insert("jump".to_owned(), SecretString::from("jump-pw"));
        let req = prepare_ssh_request(
            &target,
            &[jump],
            None,
            false,
            JumpSecrets {
                provided,
                save: true,
            },
            secrets,
            80,
            24,
        )
        .unwrap();
        assert_eq!(req.jumps.len(), 1);
        assert!(
            matches!(&req.jumps[0].auth, SshAuth::Password(p) if p.expose_secret() == "jump-pw")
        );
        assert!(store.get("jump").unwrap().is_none());
        let JumpHop {
            on_authenticated, ..
        } = req.jumps.into_iter().next().unwrap();
        (on_authenticated.unwrap())();
        assert_eq!(
            store.get("jump").unwrap().unwrap().expose_secret(),
            "jump-pw"
        );
    }

    #[test]
    fn expands_home_prefix_only() {
        let home = std::env::home_dir().unwrap();
        assert_eq!(expand_home("~/.ssh/id"), home.join(".ssh/id"));
        assert_eq!(expand_home("/abs/id"), PathBuf::from("/abs/id"));
        assert_eq!(expand_home("~user/id"), PathBuf::from("~user/id"));
    }

    #[test]
    fn event_serializes_with_camel_case_tag() {
        let json = serde_json::to_value(SessionEvent::HostKeyChanged {
            host: "h".into(),
            port: 22,
            algorithm: "ssh-ed25519".into(),
            expected_fingerprint: "SHA256:a".into(),
            actual_fingerprint: "SHA256:b".into(),
        })
        .unwrap();
        assert_eq!(json["type"], "hostKeyChanged");
        assert_eq!(json["expectedFingerprint"], "SHA256:a");
    }

    #[derive(Default)]
    struct NullSink(Mutex<Vec<SessionEvent>>);
    impl SessionSink for NullSink {
        fn output(&self, _: &[u8]) {}
        fn event(&self, e: SessionEvent) {
            self.0.lock().unwrap().push(e);
        }
    }

    #[tokio::test]
    async fn closed_session_rejects_pending_host_key_prompt() {
        let manager = SessionManager::new();
        let store = Arc::new(KnownHostsStore::new(Arc::new(
            Database::open_in_memory().unwrap(),
        )));
        let sink = Arc::new(NullSink::default());
        let verifier = InteractiveVerifier {
            store,
            manager: Arc::downgrade(&manager),
            session_id: "s1".into(),
            sink: sink.clone(),
        };
        let (tx, _rx) = mpsc::channel(1);
        manager
            .slots()
            .insert("s1".into(), Slot::new(Commands::Shell(tx)));
        let key = PublicKey::from_openssh(
            "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAILivYAx9W//3gPpBQ8sYmjiLyHYjP1Ow9bzH+Vs7rGm6",
        )
        .unwrap();

        let m = manager.clone();
        let closer = tokio::spawn(async move {
            // 프롬프트가 등록될 때까지 대기 후 탭을 닫음
            while m
                .slots()
                .get("s1")
                .is_some_and(|s| s.host_key_reply.is_none())
            {
                tokio::task::yield_now().await;
            }
            m.close("s1");
        });
        let verdict = verifier.verify("h", 22, &key).await;
        closer.await.unwrap();
        assert_eq!(verdict, HostKeyVerdict::Rejected);
        assert!(matches!(
            sink.0.lock().unwrap()[0],
            SessionEvent::HostKeyPrompt { .. }
        ));
        assert_eq!(manager.len(), 0);
    }
}
