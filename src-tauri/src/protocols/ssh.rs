//! russh 기반 SSH 클라이언트.
//!
//! - 호스트 키는 [`HostKeyVerifier`]가 판정합니다. 이 모듈은 판정 결과를 그대로 따르며
//!   무조건 승인하는 경로가 없습니다.
//! - 압축은 russh의 `flate2` 기능을 빼서 협상 목록에서 제외했고, 여기서도 `none`만 허용합니다.
//! - 셸 채널은 읽기/쓰기를 분리해 큰 입력을 보내는 동안에도 출력이 계속 소비되게 합니다.
//! - 점프 호스트: 인증을 마친 점프 연결에 `direct-tcpip` 채널을 열고 그 위에서 대상 서버와
//!   SSH를 새로 맺습니다(OpenSSH `ProxyJump`와 같은 방식). 대상 서버의 호스트 키도 똑같이 검증합니다.

use std::borrow::Cow;
use std::fmt;
use std::path::Path;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use async_trait::async_trait;
use russh::client::{self, Handle, Msg};
use russh::keys::agent::client::AgentClient;
use russh::keys::agent::AgentIdentity;
use russh::keys::{PrivateKey, PrivateKeyWithHashAlg, PublicKey, PublicKeyOrCertificate};
use russh::{Channel, ChannelMsg, Disconnect, Preferred};
use secrecy::{ExposeSecret, SecretString};
use serde::Serialize;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::mpsc;
use zeroize::Zeroizing;

use crate::core::known_hosts::fingerprint;
use crate::protocols::RemoteSession;

/// TCP 연결 수립 제한 시간
const TCP_CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
/// 키 교환 + 호스트 키 승인 대기까지 포함한 핸드셰이크 제한 시간
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(180);
const AUTH_TIMEOUT: Duration = Duration::from_secs(60);
/// 개인 키 파일 크기 상한 (일반적인 RSA 16384비트 키도 수 KiB)
const MAX_KEY_FILE_BYTES: u64 = 64 * 1024;
/// 에이전트에서 시도할 키 수 상한 (서버의 MaxAuthTries 초과 방지)
const MAX_AGENT_KEYS: usize = 8;
#[cfg(windows)]
const WINDOWS_AGENT_PIPE: &str = r"\\.\pipe\openssh-ssh-agent";

#[derive(Debug, thiserror::Error)]
pub enum SshError {
    #[error("could not connect: {0}")]
    Connect(String),
    #[error("connection timed out")]
    Timeout,
    #[error("host key changed (saved {expected}, received {actual})")]
    HostKeyChanged { expected: String, actual: String },
    #[error("host key was not accepted")]
    HostKeyRejected,
    #[error("authentication failed")]
    AuthFailed,
    #[error("private key is encrypted; passphrase required")]
    PassphraseRequired,
    #[error("cannot load private key: {0}")]
    KeyLoad(String),
    #[error("ssh-agent: {0}")]
    Agent(String),
    #[error("ssh: {0}")]
    Protocol(#[from] russh::Error),
}

impl SshError {
    pub fn code(&self) -> &'static str {
        match self {
            SshError::Connect(_) => "connect",
            SshError::Timeout => "timeout",
            SshError::HostKeyChanged { .. } => "hostKeyChanged",
            SshError::HostKeyRejected => "hostKeyRejected",
            SshError::AuthFailed => "authFailed",
            SshError::PassphraseRequired => "passphraseRequired",
            SshError::KeyLoad(_) => "keyLoad",
            SshError::Agent(_) => "agent",
            SshError::Protocol(_) => "protocol",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SshTarget {
    pub host: String,
    pub port: u16,
    pub username: String,
}

/// 인증 수단. 비밀 값은 `Debug`에 노출되지 않습니다.
pub enum SshAuth {
    Password(SecretString),
    /// 이미 복호화된 개인 키 (`ssh-key`의 `PrivateKey`는 drop 시 zeroize됨)
    Key(Arc<PrivateKey>),
    Agent,
}

impl fmt::Debug for SshAuth {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SshAuth::Password(_) => f.write_str("Password([REDACTED])"),
            SshAuth::Key(k) => write!(f, "Key({})", k.algorithm()),
            SshAuth::Agent => f.write_str("Agent"),
        }
    }
}

/// 개인 키 파일을 읽어 복호화합니다.
pub fn load_private_key(
    path: &Path,
    passphrase: Option<&SecretString>,
) -> Result<PrivateKey, SshError> {
    let meta = std::fs::metadata(path).map_err(|e| SshError::KeyLoad(e.to_string()))?;
    if !meta.is_file() || meta.len() > MAX_KEY_FILE_BYTES {
        return Err(SshError::KeyLoad("not a private key file".into()));
    }
    let text = Zeroizing::new(
        std::fs::read_to_string(path).map_err(|e| SshError::KeyLoad(e.to_string()))?,
    );
    match russh::keys::decode_secret_key(&text, passphrase.map(|p| p.expose_secret())) {
        Ok(key) => Ok(key),
        Err(russh::keys::Error::KeyIsEncrypted) if passphrase.is_none() => {
            Err(SshError::PassphraseRequired)
        }
        Err(_) if passphrase.is_some() => Err(SshError::KeyLoad(
            "wrong passphrase or unsupported key format".into(),
        )),
        Err(e) => Err(SshError::KeyLoad(e.to_string())),
    }
}

/// 호스트 키 판정 결과
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostKeyVerdict {
    Accept,
    Changed { expected: String, actual: String },
    Rejected,
}

/// 서버 호스트 키를 신뢰할지 결정합니다. (known_hosts 조회, 사용자 승인 등)
#[async_trait]
pub trait HostKeyVerifier: Send + Sync {
    async fn verify(&self, host: &str, port: u16, key: &PublicKey) -> HostKeyVerdict;
}

/// 상태 표시줄에 보여줄 협상 결과
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionInfo {
    /// 보안 상태 종류: `ssh`, `tlsCa`(인증 기관으로 검증), `tlsPinned`(고정 지문), `plain`(암호화 없음)
    pub transport: String,
    pub kex: String,
    /// SSH는 암호화 알고리즘, RDP는 인증 방식 코드(`nla` 또는 `logon`), FTP는 비어 있음
    pub cipher: String,
    pub host_key_algorithm: String,
    pub fingerprint: String,
    /// 거쳐 온 점프 호스트 이름 (접속 순서). 직접 연결이면 비어 있음
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub via: Vec<String>,
}

#[derive(Default)]
struct HandshakeState {
    verdict: Option<HostKeyVerdict>,
    info: ConnectionInfo,
}

pub struct ClientHandler {
    host: String,
    port: u16,
    verifier: Arc<dyn HostKeyVerifier>,
    state: Arc<Mutex<HandshakeState>>,
}

impl ClientHandler {
    fn state(&self) -> std::sync::MutexGuard<'_, HandshakeState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl client::Handler for ClientHandler {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        server_public_key: &PublicKeyOrCertificate,
    ) -> Result<bool, Self::Error> {
        // 호스트 인증서 알고리즘은 광고하지 않으므로 인증서가 오면 거부합니다.
        let PublicKeyOrCertificate::PublicKey { key, .. } = server_public_key else {
            self.state().verdict = Some(HostKeyVerdict::Rejected);
            return Ok(false);
        };
        let verdict = self.verifier.verify(&self.host, self.port, key).await;
        let accepted = verdict == HostKeyVerdict::Accept;
        let mut state = self.state();
        if accepted {
            state.info.host_key_algorithm = key.algorithm().as_str().to_owned();
            state.info.fingerprint = fingerprint(key);
        }
        state.verdict = Some(verdict);
        Ok(accepted)
    }

    async fn kex_done(
        &mut self,
        _shared_secret: Option<&[u8]>,
        names: &russh::Names,
        _session: &mut client::Session,
    ) -> Result<(), Self::Error> {
        let mut state = self.state();
        state.info.kex = names.kex.as_ref().to_owned();
        state.info.cipher = names.cipher.as_ref().to_owned();
        Ok(())
    }
}

/// 셸 채널로 보내는 명령
#[derive(Debug)]
pub enum ShellCommand {
    Input(Vec<u8>),
    Resize { cols: u32, rows: u32 },
}

fn client_config() -> client::Config {
    client::Config {
        keepalive_interval: Some(Duration::from_secs(30)),
        keepalive_max: 3,
        nodelay: true,
        preferred: Preferred {
            compression: Cow::Borrowed(&[russh::compression::NONE]),
            ..Preferred::default()
        },
        ..client::Config::default()
    }
}

/// 인증까지 끝난 SSH 연결. 하나의 연결에 여러 채널(셸, 이후 SFTP)을 붙입니다.
pub struct SshConnection {
    handle: Handle<ClientHandler>,
    info: ConnectionInfo,
    /// 이 연결이 지나가는 점프 호스트 연결. 대상 연결이 살아 있는 동안 유지합니다.
    via: Option<Arc<SshConnection>>,
}

impl SshConnection {
    /// TCP로 직접 연결합니다.
    pub async fn connect(
        target: &SshTarget,
        auth: &SshAuth,
        verifier: Arc<dyn HostKeyVerifier>,
    ) -> Result<Self, SshError> {
        let stream = tokio::time::timeout(
            TCP_CONNECT_TIMEOUT,
            tokio::net::TcpStream::connect((target.host.as_str(), target.port)),
        )
        .await
        .map_err(|_| SshError::Timeout)?
        .map_err(|e| SshError::Connect(e.to_string()))?;
        Self::handshake(stream, target, auth, verifier, None).await
    }

    /// 점프 호스트 연결을 거쳐 연결합니다. 대상 주소는 점프 호스트에서 본 주소입니다.
    pub async fn connect_via(
        jump: Arc<SshConnection>,
        target: &SshTarget,
        auth: &SshAuth,
        verifier: Arc<dyn HostKeyVerifier>,
    ) -> Result<Self, SshError> {
        let stream = tokio::time::timeout(
            TCP_CONNECT_TIMEOUT,
            jump.open_tunnel(&target.host, target.port),
        )
        .await
        .map_err(|_| SshError::Timeout)??;
        Self::handshake(stream, target, auth, verifier, Some(jump)).await
    }

    /// 이 연결을 거쳐 `host:port`로 가는 TCP 터널 (`direct-tcpip` 채널)
    pub async fn open_tunnel(
        &self,
        host: &str,
        port: u16,
    ) -> Result<russh::ChannelStream<Msg>, SshError> {
        let channel = self
            .handle
            .channel_open_direct_tcpip(host, u32::from(port), "127.0.0.1", 0)
            .await
            .map_err(|e| match e {
                russh::Error::ChannelOpenFailure(reason) => SshError::Connect(format!(
                    "jump host could not reach {host}:{port} ({reason:?})"
                )),
                other => SshError::Protocol(other),
            })?;
        Ok(channel.into_stream())
    }

    async fn handshake<S>(
        stream: S,
        target: &SshTarget,
        auth: &SshAuth,
        verifier: Arc<dyn HostKeyVerifier>,
        via: Option<Arc<SshConnection>>,
    ) -> Result<Self, SshError>
    where
        S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    {
        let state = Arc::new(Mutex::new(HandshakeState::default()));
        let handler = ClientHandler {
            host: target.host.clone(),
            port: target.port,
            verifier,
            state: state.clone(),
        };

        let handshake = tokio::time::timeout(
            HANDSHAKE_TIMEOUT,
            client::connect_stream(Arc::new(client_config()), stream, handler),
        )
        .await
        .map_err(|_| SshError::Timeout)?;

        let mut handle = match handshake {
            Ok(handle) => handle,
            Err(e) => {
                let verdict = state
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .verdict
                    .take();
                return Err(match verdict {
                    Some(HostKeyVerdict::Changed { expected, actual }) => {
                        SshError::HostKeyChanged { expected, actual }
                    }
                    Some(HostKeyVerdict::Rejected) => SshError::HostKeyRejected,
                    _ => SshError::Protocol(e),
                });
            }
        };

        tokio::time::timeout(
            AUTH_TIMEOUT,
            authenticate(&mut handle, &target.username, auth),
        )
        .await
        .map_err(|_| SshError::Timeout)??;

        let mut info = state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .info
            .clone();
        info.transport = "ssh".into();
        Ok(Self { handle, info, via })
    }

    pub fn info(&self) -> &ConnectionInfo {
        &self.info
    }

    /// 상태 표시줄에 보여줄 경유 점프 호스트 이름
    pub fn set_via_names(&mut self, names: Vec<String>) {
        self.info.via = names;
    }

    /// 서버가 연결을 끊었거나 keepalive가 실패했는지
    pub fn is_closed(&self) -> bool {
        self.handle.is_closed()
    }

    /// 같은 연결 위에 SFTP 서브시스템 채널을 엽니다.
    pub async fn open_sftp_channel(&self) -> Result<Channel<Msg>, SshError> {
        let channel = self.handle.channel_open_session().await?;
        channel.request_subsystem(true, "sftp").await?;
        Ok(channel)
    }

    /// PTY를 요청하고 로그인 셸을 엽니다.
    pub async fn open_shell(&self, cols: u32, rows: u32) -> Result<Channel<Msg>, SshError> {
        let channel = self.handle.channel_open_session().await?;
        channel
            .request_pty(true, "xterm-256color", cols, rows, 0, 0, &[])
            .await?;
        channel.request_shell(true).await?;
        Ok(channel)
    }

    /// 셸 채널을 구동합니다. 채널이 닫히거나 명령 송신 측이 모두 drop되면 끝나며,
    /// 원격 셸의 종료 코드를 돌려줍니다.
    pub async fn run_shell<F>(
        channel: Channel<Msg>,
        commands: &mut mpsc::Receiver<ShellCommand>,
        mut on_output: F,
    ) -> Result<Option<u32>, SshError>
    where
        F: FnMut(&[u8]) + Send + 'static,
    {
        let (mut reader, writer) = channel.split();
        let mut read_task = tokio::spawn(async move {
            let mut exit_status = None;
            while let Some(msg) = reader.wait().await {
                match msg {
                    ChannelMsg::Data { data } | ChannelMsg::ExtendedData { data, .. } => {
                        on_output(&data)
                    }
                    ChannelMsg::ExitStatus { exit_status: code } => exit_status = Some(code),
                    ChannelMsg::Close => break,
                    _ => {}
                }
            }
            exit_status
        });

        loop {
            tokio::select! {
                status = &mut read_task => {
                    return Ok(status.unwrap_or(None));
                }
                cmd = commands.recv() => match cmd {
                    Some(ShellCommand::Input(bytes)) => writer.data(&bytes[..]).await?,
                    Some(ShellCommand::Resize { cols, rows }) => {
                        writer.window_change(cols, rows, 0, 0).await?
                    }
                    None => {
                        let _ = writer.eof().await;
                        let _ = writer.close().await;
                        read_task.abort();
                        return Ok(None);
                    }
                },
            }
        }
    }
}

#[async_trait]
impl RemoteSession for SshConnection {
    /// 연결을 끊고, 거쳐 온 점프 호스트 연결도 차례로 끊습니다.
    async fn disconnect(&self) {
        let _ = self
            .handle
            .disconnect(Disconnect::ByApplication, "", "en")
            .await;
        let mut next = self.via.clone();
        while let Some(jump) = next {
            let _ = jump
                .handle
                .disconnect(Disconnect::ByApplication, "", "en")
                .await;
            next = jump.via.clone();
        }
    }
}

async fn authenticate(
    handle: &mut Handle<ClientHandler>,
    user: &str,
    auth: &SshAuth,
) -> Result<(), SshError> {
    let success = match auth {
        SshAuth::Password(password) => handle
            .authenticate_password(user, password.expose_secret())
            .await?
            .success(),
        SshAuth::Key(key) => {
            let hash = if key.algorithm().is_rsa() {
                handle.best_supported_rsa_hash().await?.flatten()
            } else {
                None
            };
            handle
                .authenticate_publickey(user, PrivateKeyWithHashAlg::new(key.clone(), hash))
                .await?
                .success()
        }
        SshAuth::Agent => authenticate_agent(handle, user).await?,
    };
    if success {
        Ok(())
    } else {
        Err(SshError::AuthFailed)
    }
}

async fn authenticate_agent(
    handle: &mut Handle<ClientHandler>,
    user: &str,
) -> Result<bool, SshError> {
    let agent_err = |e: russh::keys::Error| SshError::Agent(e.to_string());
    #[cfg(unix)]
    let mut agent = AgentClient::connect_env().await.map_err(agent_err)?;
    #[cfg(windows)]
    let mut agent = AgentClient::connect_named_pipe(WINDOWS_AGENT_PIPE)
        .await
        .map_err(agent_err)?;
    try_agent_keys(handle, user, &mut agent).await
}

async fn try_agent_keys<S>(
    handle: &mut Handle<ClientHandler>,
    user: &str,
    agent: &mut AgentClient<S>,
) -> Result<bool, SshError>
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let identities = agent
        .request_identities()
        .await
        .map_err(|e| SshError::Agent(e.to_string()))?;
    if identities.is_empty() {
        return Err(SshError::Agent("agent has no keys".into()));
    }
    for identity in identities.into_iter().take(MAX_AGENT_KEYS) {
        let AgentIdentity::PublicKey { key, .. } = identity else {
            continue;
        };
        let hash = if key.algorithm().is_rsa() {
            handle.best_supported_rsa_hash().await?.flatten()
        } else {
            None
        };
        let result = handle
            .authenticate_publickey_with(user, key, hash, agent)
            .await
            .map_err(|e| SshError::Agent(e.to_string()))?;
        if result.success() {
            return Ok(true);
        }
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auth_debug_never_shows_password() {
        let auth = SshAuth::Password(SecretString::from("hunter2"));
        assert!(!format!("{auth:?}").contains("hunter2"));
    }

    #[test]
    fn compression_is_disabled() {
        let config = client_config();
        assert_eq!(
            config.preferred.compression.as_ref(),
            &[russh::compression::NONE]
        );
    }

    #[test]
    fn load_private_key_reports_missing_passphrase() {
        // ssh-keygen -t ed25519 -N 'correct horse' 로 만든 테스트 전용 키
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/encrypted_ed25519");
        assert!(matches!(
            load_private_key(&path, None),
            Err(SshError::PassphraseRequired)
        ));
        assert!(matches!(
            load_private_key(&path, Some(&SecretString::from("wrong"))),
            Err(SshError::KeyLoad(_))
        ));
        assert!(load_private_key(&path, Some(&SecretString::from("correct horse"))).is_ok());
    }

    #[test]
    fn load_private_key_rejects_large_or_missing_files() {
        let dir = tempfile::tempdir().unwrap();
        let big = dir.path().join("big");
        std::fs::write(&big, vec![b'a'; (MAX_KEY_FILE_BYTES + 1) as usize]).unwrap();
        assert!(matches!(
            load_private_key(&big, None),
            Err(SshError::KeyLoad(_))
        ));
        assert!(matches!(
            load_private_key(&dir.path().join("missing"), None),
            Err(SshError::KeyLoad(_))
        ));
        assert!(matches!(
            load_private_key(dir.path(), None),
            Err(SshError::KeyLoad(_))
        ));
    }
}
