//! suppaftp 기반 FTP/FTPS `RemoteFs`.
//!
//! - 데이터 연결은 패시브 모드만 사용하고, 서버가 PASV로 알려준 주소 대신 제어 연결의 주소로
//!   접속합니다(NAT 우회 옵션). 다른 호스트로 데이터 연결을 유도하는 응답을 따르지 않기 위해서입니다.
//! - FTP 명령 인자로 들어가는 문자열에 CR/LF/NUL이 있으면 보내기 전에 거부합니다.
//!   (suppaftp도 막지만 앱에서 한 번 더 막습니다.)
//! - 목록은 데이터 연결을 직접 읽어 줄 길이, 전체 크기, 항목 수에 상한을 둡니다.
//! - FTP 제어 연결은 한 번에 명령 하나만 처리하므로 탐색용 연결 하나와 전송용 연결 풀을 따로 둡니다.
//! - 로그인 직후 바이너리 전송(TYPE I)으로 바꿉니다. 기본값인 ASCII로 받으면 파일이 깨집니다.
//! - FTPS 인증서는 [`super::tls`]가 검증합니다(OS 신뢰 저장소 또는 고정 지문).

use std::future::Future;
use std::io;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex as StdMutex, PoisonError};
use std::task::{ready, Context, Poll};
use std::time::{Duration, UNIX_EPOCH};

use async_trait::async_trait;
use secrecy::{ExposeSecret, SecretString};
use suppaftp::list::PosixPexQuery;
use suppaftp::list::{File as ListFile, ListParser};
use suppaftp::tokio::{
    AsyncRustlsConnector, AsyncRustlsStream, ImplAsyncFtpStream, TransferStream,
};
use suppaftp::types::FileType;
use suppaftp::{FtpError as LibError, Mode, Status};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, BufReader, ReadBuf};
use tokio::sync::Mutex;

use super::tls::{client_config, CertTrust, Observed};
use super::{EntryKind, FsEntry, FsError, Listing, RemoteFs, MAX_LIST_ENTRIES};
use crate::transfer::paths::{remote_parent, NameError};

type Ftp = ImplAsyncFtpStream<AsyncRustlsStream>;
type Transfer = TransferStream<AsyncRustlsStream>;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
/// 패시브 데이터 연결 수립 제한 시간. 방화벽이 패킷을 버리면 OS 기본값(수십 초)까지 기다리게 됩니다.
const DATA_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// TLS 핸드셰이크와 로그인까지 포함한 제한 시간
const LOGIN_TIMEOUT: Duration = Duration::from_secs(60);
/// 목록 한 줄과 전체 크기 상한
const MAX_LIST_LINE: usize = 8 * 1024;
const MAX_LIST_BYTES: u64 = 32 * 1024 * 1024;
/// 탐색 명령 하나(목록 전체 포함)의 응답 대기 시간
const COMMAND_TIMEOUT: Duration = Duration::from_secs(30);
/// 재사용하려고 열어 두는 전송 연결 수
const MAX_IDLE: usize = 3;
/// 오류 메시지로 보여줄 서버 응답 길이
const MAX_REPLY_TEXT: usize = 300;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FtpSecurity {
    /// 암호화 없음 (UI에서 경고)
    Plain,
    /// 평문으로 접속한 뒤 AUTH TLS
    Explicit,
    /// 처음부터 TLS (보통 포트 990)
    Implicit,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FtpTarget {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub security: FtpSecurity,
}

#[derive(Debug, thiserror::Error)]
pub enum FtpError {
    #[error("could not connect: {0}")]
    Connect(String),
    #[error("connection timed out")]
    Timeout,
    #[error("authentication failed")]
    AuthFailed,
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
    #[error("{0}")]
    Fs(#[from] FsError),
}

impl FtpError {
    pub fn code(&self) -> &'static str {
        match self {
            FtpError::Connect(_) => "connect",
            FtpError::Timeout => "timeout",
            FtpError::AuthFailed => "authFailed",
            FtpError::CertUntrusted { .. } => "certUntrusted",
            FtpError::CertChanged { .. } => "certChanged",
            FtpError::CertRejected => "certRejected",
            FtpError::Tls(_) => "tls",
            FtpError::Fs(e) => e.code(),
        }
    }
}

/// FTP 명령 인자 검사: 줄바꿈이 들어가면 두 번째 명령을 끼워 넣을 수 있습니다.
pub fn check_arg(s: &str) -> Result<&str, FsError> {
    if s.contains(['\r', '\n', '\0']) {
        return Err(FsError::Name(NameError::Control));
    }
    Ok(s)
}

/// 데이터 연결 수립 실패 표시. 제어 연결이 끊긴 것과 구분해 사용자에게 원인을 알려 줍니다.
#[derive(Debug, thiserror::Error)]
#[error("data connection to {addr} failed: {source}")]
struct DataConnectError {
    addr: std::net::SocketAddr,
    source: io::Error,
}

/// 패시브 데이터 연결: 제한 시간을 두고, 실패하면 [`DataConnectError`]로 표시합니다.
fn passive_stream(
    addr: std::net::SocketAddr,
) -> Pin<Box<dyn Future<Output = suppaftp::FtpResult<tokio::net::TcpStream>> + Send + Sync>> {
    Box::pin(async move {
        let connect =
            tokio::time::timeout(DATA_CONNECT_TIMEOUT, tokio::net::TcpStream::connect(addr));
        let result = match connect.await {
            Ok(r) => r,
            Err(_) => Err(io::ErrorKind::TimedOut.into()),
        };
        result.map_err(|source| {
            let kind = source.kind();
            LibError::ConnectionError(io::Error::new(kind, DataConnectError { addr, source }))
        })
    })
}

/// 서버 응답을 짧고 안전한 문자열로 바꿉니다.
fn reply_text(body: &[u8]) -> String {
    let text: String = String::from_utf8_lossy(body)
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .take(MAX_REPLY_TEXT)
        .collect();
    text.trim().to_owned()
}

impl From<LibError> for FsError {
    fn from(e: LibError) -> Self {
        match e {
            LibError::ConnectionError(io)
                if io.get_ref().is_some_and(|e| e.is::<DataConnectError>()) =>
            {
                FsError::DataConnection
            }
            LibError::ConnectionError(io) => match io.kind() {
                io::ErrorKind::TimedOut => FsError::Timeout,
                io::ErrorKind::InvalidInput => FsError::Name(NameError::Control),
                _ => FsError::ConnectionLost,
            },
            LibError::UnexpectedResponse(r) => match r.status {
                // 421 서비스 종료, 425 데이터 연결 실패, 426 전송 중단: 다시 시도할 수 있음
                Status::NotAvailable
                | Status::CannotOpenDataConnection
                | Status::TransferAborted => FsError::ConnectionLost,
                Status::NotLoggedIn => FsError::PermissionDenied,
                _ => FsError::Remote(reply_text(&r.body)),
            },
            LibError::SecureError(s) => FsError::Remote(format!("TLS: {s}")),
            other => FsError::Remote(other.to_string()),
        }
    }
}

/// `io::Error`로 전달할 때 전송 큐가 일시적 오류를 알아볼 수 있도록 종류를 맞춥니다.
fn to_io(e: FsError) -> io::Error {
    let kind = match &e {
        FsError::ConnectionLost => io::ErrorKind::ConnectionAborted,
        FsError::Timeout => io::ErrorKind::TimedOut,
        FsError::NotFound => io::ErrorKind::NotFound,
        FsError::PermissionDenied => io::ErrorKind::PermissionDenied,
        _ => io::ErrorKind::Other,
    };
    io::Error::new(kind, e.to_string())
}

fn entry_from_list(file: &ListFile) -> FsEntry {
    let kind = if file.is_directory() {
        EntryKind::Dir
    } else if file.is_symlink() {
        EntryKind::Symlink
    } else if file.is_file() {
        EntryKind::File
    } else {
        EntryKind::Other
    };
    let mut mode = 0u32;
    for (shift, who) in [
        (6, PosixPexQuery::Owner),
        (3, PosixPexQuery::Group),
        (0, PosixPexQuery::Others),
    ] {
        let bits = (u32::from(file.can_read(who)) << 2)
            | (u32::from(file.can_write(who)) << 1)
            | u32::from(file.can_execute(who));
        mode |= bits << shift;
    }
    FsEntry {
        name: file.name().to_owned(),
        kind,
        size: file.size() as u64,
        modified: file
            .modified()
            .duration_since(UNIX_EPOCH)
            .ok()
            .and_then(|d| i64::try_from(d.as_secs()).ok()),
        permissions: Some(mode),
    }
}

/// 목록 한 줄을 해석합니다. 해석할 수 없는 줄(합계 줄 등)은 `None`
pub fn parse_list_line(line: &str, mlsd: bool) -> Option<FsEntry> {
    let file = if mlsd {
        ListParser::parse_mlsd(line).ok()?
    } else {
        ListParser::parse_posix(line)
            .or_else(|_| ListParser::parse_dos(line))
            .ok()?
    };
    let entry = entry_from_list(&file);
    (entry.name != "." && entry.name != ".." && !entry.name.is_empty()).then_some(entry)
}

/// 데이터 연결에서 목록을 읽습니다. 상한을 넘으면 `truncated`
async fn read_listing<R: AsyncRead + Unpin>(
    stream: R,
    mlsd: bool,
) -> Result<(Vec<FsEntry>, bool), FsError> {
    let mut reader = BufReader::new(stream.take(MAX_LIST_BYTES));
    let mut entries = Vec::new();
    let mut line = Vec::new();
    loop {
        line.clear();
        let n = (&mut reader)
            .take(MAX_LIST_LINE as u64 + 1)
            .read_until(b'\n', &mut line)
            .await?;
        if n == 0 {
            // 전체 크기 상한에 걸려 끊겼는지 확인
            let hit_limit = reader.get_ref().limit() == 0;
            return Ok((entries, hit_limit));
        }
        if line.len() > MAX_LIST_LINE {
            return Err(FsError::Remote(
                "server sent an overlong listing line".into(),
            ));
        }
        if entries.len() >= MAX_LIST_ENTRIES {
            return Ok((entries, true));
        }
        let text = String::from_utf8_lossy(&line);
        let text = text.trim_end_matches(['\r', '\n']);
        if let Some(entry) = parse_list_line(text, mlsd) {
            entries.push(entry);
        }
    }
}

/// 새 연결을 여는 데 필요한 정보 (탐색/전송 연결이 함께 씀)
struct Connector {
    target: FtpTarget,
    password: SecretString,
    tls: Option<tokio_rustls::TlsConnector>,
}

impl Connector {
    async fn open(&self) -> Result<Ftp, FtpError> {
        tokio::time::timeout(LOGIN_TIMEOUT, self.open_inner())
            .await
            .map_err(|_| FtpError::Timeout)?
    }

    async fn open_inner(&self) -> Result<Ftp, FtpError> {
        let host = self.target.host.as_str();
        let addr = (host, self.target.port);
        let tls = || {
            self.tls
                .clone()
                .map(AsyncRustlsConnector::from)
                .ok_or_else(|| FtpError::Tls("TLS is not configured".into()))
        };
        let tls_err = |e: LibError| match e {
            LibError::SecureError(s) => FtpError::Tls(s),
            other => FtpError::Fs(other.into()),
        };
        let ftp = match self.target.security {
            FtpSecurity::Implicit => Ftp::connect_secure_implicit(addr, tls()?, host)
                .await
                .map_err(tls_err)?,
            security => {
                let stream =
                    tokio::time::timeout(CONNECT_TIMEOUT, tokio::net::TcpStream::connect(addr))
                        .await
                        .map_err(|_| FtpError::Timeout)?
                        .map_err(|e| FtpError::Connect(e.to_string()))?;
                let ftp = Ftp::connect_with_stream(stream)
                    .await
                    .map_err(|e| FtpError::Connect(e.to_string()))?;
                if security == FtpSecurity::Explicit {
                    ftp.into_secure(tls()?, host).await.map_err(tls_err)?
                } else {
                    ftp
                }
            }
        };
        let mut ftp = ftp.passive_stream_builder(passive_stream);
        ftp.set_mode(Mode::Passive);
        ftp.set_passive_nat_workaround(true);
        match ftp
            .login(
                check_arg(&self.target.username)?,
                check_arg(self.password.expose_secret())?,
            )
            .await
        {
            Ok(()) => {}
            Err(LibError::UnexpectedResponse(r))
                if matches!(r.status, Status::NotLoggedIn | Status::NeedPassword) =>
            {
                return Err(FtpError::AuthFailed)
            }
            Err(e) => return Err(FtpError::Fs(e.into())),
        }
        if self.target.security == FtpSecurity::Implicit {
            // 명시적 FTPS는 suppaftp가 AUTH TLS 뒤에 보내지만, 암시적 FTPS 연결 함수는 보내지 않습니다.
            // 빠지면 서버가 평문 데이터 연결을 거부(522)하거나, 더 나쁘게는 받아들입니다.
            for command in ["PBSZ 0", "PROT P"] {
                ftp.custom_command(command, &[Status::CommandOk])
                    .await
                    .map_err(|e| FtpError::Fs(e.into()))?;
            }
        }
        ftp.transfer_type(FileType::Binary)
            .await
            .map_err(|e| FtpError::Fs(e.into()))?;
        Ok(ftp)
    }
}

/// 전송 연결 풀. 끝난 연결을 [`MAX_IDLE`]개까지 재사용합니다.
struct Pool {
    connector: Connector,
    idle: StdMutex<Vec<Ftp>>,
}

impl Pool {
    async fn take(&self) -> Result<Ftp, FsError> {
        let reused = self
            .idle
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .pop();
        match reused {
            Some(ftp) => Ok(ftp),
            None => self.connector.open().await.map_err(|e| match e {
                FtpError::Fs(fs) => fs,
                FtpError::Timeout => FsError::Timeout,
                other => FsError::Remote(other.to_string()),
            }),
        }
    }

    fn give(&self, ftp: Ftp) {
        let mut idle = self.idle.lock().unwrap_or_else(PoisonError::into_inner);
        if idle.len() < MAX_IDLE {
            idle.push(ftp);
        }
    }
}

/// 연결 정보 (상태 표시줄용)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FtpConnectionInfo {
    pub security: FtpSecurity,
    pub trust: Option<CertTrust>,
    pub fingerprint: Option<String>,
}

pub struct FtpFs {
    pool: Arc<Pool>,
    browse: Mutex<Option<Ftp>>,
    mlsd: AtomicBool,
    home: String,
    info: FtpConnectionInfo,
}

impl FtpFs {
    /// 접속, (FTPS면) 인증서 검증, 로그인까지 마칩니다.
    ///
    /// `pinned`는 이 호스트에 고정된 인증서 지문입니다. 인증서를 신뢰할 수 없으면
    /// [`FtpError::CertUntrusted`]로 본 지문을 돌려줍니다.
    pub async fn connect(
        target: FtpTarget,
        password: SecretString,
        pinned: Option<String>,
    ) -> Result<Self, FtpError> {
        let (tls, verifier) = if target.security == FtpSecurity::Plain {
            (None, None)
        } else {
            let (config, verifier) =
                client_config(pinned).map_err(|e| FtpError::Tls(e.to_string()))?;
            (
                Some(tokio_rustls::TlsConnector::from(config)),
                Some(verifier),
            )
        };
        let connector = Connector {
            target: target.clone(),
            password,
            tls,
        };

        let mut ftp = match connector.open().await {
            Ok(ftp) => ftp,
            Err(FtpError::Tls(msg)) => {
                let observed = verifier.as_ref().map(|v| v.observed()).unwrap_or_default();
                return Err(match observed {
                    Observed {
                        fingerprint: Some(fingerprint),
                        trust: None,
                        problem,
                    } => FtpError::CertUntrusted {
                        fingerprint,
                        problem: problem.unwrap_or_else(|| "invalid".into()),
                    },
                    _ => FtpError::Tls(msg),
                });
            }
            Err(e) => return Err(e),
        };

        let home = ftp.pwd().await.map_err(|e| FtpError::Fs(e.into()))?;
        let mlsd = ftp
            .feat()
            .await
            .map(|f| f.keys().any(|k| k.eq_ignore_ascii_case("MLST")))
            .unwrap_or(false);
        let observed = verifier.as_ref().map(|v| v.observed()).unwrap_or_default();
        Ok(Self {
            pool: Arc::new(Pool {
                connector,
                idle: StdMutex::new(Vec::new()),
            }),
            browse: Mutex::new(Some(ftp)),
            mlsd: AtomicBool::new(mlsd),
            home,
            info: FtpConnectionInfo {
                security: target.security,
                trust: observed.trust,
                fingerprint: observed.fingerprint,
            },
        })
    }

    pub fn info(&self) -> &FtpConnectionInfo {
        &self.info
    }

    /// 탐색 연결이 서버의 유휴 시간 제한으로 끊기지 않도록 NOOP을 보냅니다.
    pub async fn keepalive(&self) {
        let mut guard = self.browse.lock().await;
        if let Some(ftp) = guard.as_mut() {
            let ok = tokio::time::timeout(COMMAND_TIMEOUT, ftp.noop())
                .await
                .is_ok_and(|r| r.is_ok());
            if !ok {
                *guard = None;
            }
        }
    }

    /// 탐색 연결에서 명령을 실행합니다. 연결이 끊겼으면 한 번 다시 연결해 재시도합니다.
    async fn browse<T, F>(&self, op: F) -> Result<T, FsError>
    where
        F: for<'a> Fn(&'a mut Ftp) -> Pin<Box<dyn Future<Output = Result<T, FsError>> + Send + 'a>>,
    {
        let mut guard = self.browse.lock().await;
        for attempt in 0..2 {
            if guard.is_none() {
                *guard = Some(self.pool.take().await?);
            }
            let Some(ftp) = guard.as_mut() else {
                return Err(FsError::ConnectionLost);
            };
            let result = tokio::time::timeout(COMMAND_TIMEOUT, op(ftp))
                .await
                .unwrap_or(Err(FsError::Timeout));
            match result {
                Err(FsError::ConnectionLost) if attempt == 0 => *guard = None,
                // 데이터 연결만 실패: 제어 연결은 살아 있으므로 그대로 한 번 더 시도
                Err(FsError::DataConnection) if attempt == 0 => {}
                Err(e @ (FsError::ConnectionLost | FsError::Timeout)) => {
                    *guard = None;
                    return Err(e);
                }
                other => return other,
            }
        }
        Err(FsError::ConnectionLost)
    }
}

#[async_trait]
impl RemoteFs for FtpFs {
    async fn home(&self) -> Result<String, FsError> {
        Ok(self.home.clone())
    }

    async fn list(&self, path: &str) -> Result<Listing, FsError> {
        let path = check_arg(path)?.to_owned();
        let mlsd = self.mlsd.load(Ordering::Relaxed);
        let (cwd, entries, truncated) = self
            .browse(|ftp| {
                let path = path.clone();
                Box::pin(async move {
                    // LIST 인자는 서버마다 옵션으로 해석하기도 하므로 이동한 뒤 인자 없이 요청합니다.
                    ftp.cwd(&path).await.map_err(|e| match FsError::from(e) {
                        FsError::Remote(_) => FsError::NotFound,
                        other => other,
                    })?;
                    let cwd = ftp.pwd().await?;
                    let command = if mlsd { "MLSD" } else { "LIST" };
                    let (_, stream) = ftp
                        .custom_data_command(command, &[Status::AboutToSend, Status::AlreadyOpen])
                        .await?;
                    let mut stream: Transfer = stream;
                    let read = read_listing(&mut stream, mlsd).await;
                    match &read {
                        Ok((_, false)) => stream.finish().await?,
                        // 상한에 걸리면 남은 데이터를 받지 않고 중단
                        _ => ftp.abort(stream).await?,
                    }
                    let (entries, truncated) = read?;
                    Ok((cwd, entries, truncated))
                })
            })
            .await?;
        Ok(Listing {
            parent: remote_parent(&cwd),
            path: cwd,
            entries,
            truncated,
        })
    }

    /// FTP에는 lstat이 없어 폴더 이동(CWD) → 크기(SIZE) 순서로 확인합니다.
    async fn stat(&self, path: &str) -> Result<Option<FsEntry>, FsError> {
        let path = check_arg(path)?.to_owned();
        let name = path.rsplit('/').next().unwrap_or(&path).to_owned();
        self.browse(|ftp| {
            let (path, name) = (path.clone(), name.clone());
            Box::pin(async move {
                if ftp.cwd(&path).await.is_ok() {
                    return Ok(Some(FsEntry {
                        name,
                        kind: EntryKind::Dir,
                        size: 0,
                        modified: None,
                        permissions: None,
                    }));
                }
                match ftp.size(&path).await {
                    Ok(size) => {
                        let modified = ftp.mdtm(&path).await.ok().map(|t| t.and_utc().timestamp());
                        Ok(Some(FsEntry {
                            name,
                            kind: EntryKind::File,
                            size: size as u64,
                            modified,
                            permissions: None,
                        }))
                    }
                    Err(LibError::UnexpectedResponse(_)) => Ok(None),
                    Err(e) => Err(e.into()),
                }
            })
        })
        .await
    }

    async fn mkdir(&self, path: &str) -> Result<(), FsError> {
        if self.stat(path).await?.is_some() {
            return Err(FsError::AlreadyExists);
        }
        let path = check_arg(path)?.to_owned();
        self.browse(|ftp| {
            let path = path.clone();
            Box::pin(async move { Ok(ftp.mkdir(&path).await?) })
        })
        .await
    }

    async fn remove_file(&self, path: &str) -> Result<(), FsError> {
        let path = check_arg(path)?.to_owned();
        self.browse(|ftp| {
            let path = path.clone();
            Box::pin(async move { Ok(ftp.rm(&path).await?) })
        })
        .await
    }

    async fn remove_dir(&self, path: &str) -> Result<(), FsError> {
        let path = check_arg(path)?.to_owned();
        self.browse(|ftp| {
            let path = path.clone();
            Box::pin(async move { Ok(ftp.rmdir(&path).await?) })
        })
        .await
    }

    async fn rename(&self, from: &str, to: &str) -> Result<(), FsError> {
        if self.stat(to).await?.is_some() {
            return Err(FsError::AlreadyExists);
        }
        let (from, to) = (check_arg(from)?.to_owned(), check_arg(to)?.to_owned());
        self.browse(|ftp| {
            let (from, to) = (from.clone(), to.clone());
            Box::pin(async move { Ok(ftp.rename(&from, &to).await?) })
        })
        .await
    }

    async fn open_read(&self, path: &str) -> Result<Box<dyn AsyncRead + Send + Unpin>, FsError> {
        let path = check_arg(path)?;
        let mut ftp = self.pool.take().await?;
        match ftp.retr_as_stream(path).await {
            Ok(stream) => Ok(Box::new(FtpReader {
                pool: self.pool.clone(),
                state: State::Streaming(Box::new((stream, ftp))),
            })),
            Err(e) => {
                let e = FsError::from(e);
                if !e.is_transient() {
                    self.pool.give(ftp);
                }
                Err(e)
            }
        }
    }

    async fn open_write(&self, path: &str) -> Result<Box<dyn AsyncWrite + Send + Unpin>, FsError> {
        let path = check_arg(path)?;
        let mut ftp = self.pool.take().await?;
        match ftp.put_with_stream(path).await {
            Ok(stream) => Ok(Box::new(FtpWriter {
                pool: self.pool.clone(),
                state: State::Streaming(Box::new((stream, ftp))),
            })),
            Err(e) => {
                let e = FsError::from(e);
                if !e.is_transient() {
                    self.pool.give(ftp);
                }
                Err(e)
            }
        }
    }
}

type Finishing = Pin<Box<dyn Future<Output = Result<Ftp, FsError>> + Send>>;

/// 전송 스트림 상태. 끝나면 서버의 완료 응답(226)을 확인하고 연결을 풀에 돌려줍니다.
enum State {
    Streaming(Box<(Transfer, Ftp)>),
    Finishing(Finishing),
    Done,
}

impl State {
    fn finish(self) -> Self {
        match self {
            State::Streaming(pair) => State::Finishing(Box::pin(async move {
                let (stream, ftp) = *pair;
                stream.finish().await?;
                Ok(ftp)
            })),
            other => other,
        }
    }

    /// 완료 응답을 기다립니다. 성공하면 연결을 풀에 돌려줍니다.
    fn poll_finished(&mut self, cx: &mut Context<'_>, pool: &Pool) -> Poll<io::Result<()>> {
        let State::Finishing(fut) = self else {
            return Poll::Ready(Ok(()));
        };
        let result = ready!(fut.as_mut().poll(cx));
        *self = State::Done;
        Poll::Ready(match result {
            Ok(ftp) => {
                pool.give(ftp);
                Ok(())
            }
            Err(e) => Err(to_io(e)),
        })
    }
}

struct FtpReader {
    pool: Arc<Pool>,
    state: State,
}

impl AsyncRead for FtpReader {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let me = self.get_mut();
        if buf.remaining() == 0 {
            return Poll::Ready(Ok(()));
        }
        loop {
            match &mut me.state {
                State::Streaming(pair) => {
                    let before = buf.filled().len();
                    ready!(Pin::new(&mut pair.0).poll_read(cx, buf))?;
                    if buf.filled().len() > before {
                        return Poll::Ready(Ok(()));
                    }
                    // 데이터 연결이 닫힘: 서버가 전송 완료(226)를 보냈는지 확인해야
                    // 중간에 끊긴 파일을 완료로 착각하지 않습니다.
                    let state = std::mem::replace(&mut me.state, State::Done);
                    me.state = state.finish();
                }
                State::Finishing(_) => return me.state.poll_finished(cx, &me.pool),
                State::Done => return Poll::Ready(Ok(())),
            }
        }
    }
}

struct FtpWriter {
    pool: Arc<Pool>,
    state: State,
}

impl AsyncWrite for FtpWriter {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        data: &[u8],
    ) -> Poll<io::Result<usize>> {
        match &mut self.get_mut().state {
            State::Streaming(pair) => Pin::new(&mut pair.0).poll_write(cx, data),
            _ => Poll::Ready(Err(io::Error::other("upload already finished"))),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match &mut self.get_mut().state {
            State::Streaming(pair) => Pin::new(&mut pair.0).poll_flush(cx),
            _ => Poll::Ready(Ok(())),
        }
    }

    /// 데이터 연결을 닫고 서버의 완료 응답(226)을 확인합니다.
    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let me = self.get_mut();
        if matches!(me.state, State::Streaming(..)) {
            let state = std::mem::replace(&mut me.state, State::Done);
            me.state = state.finish();
        }
        me.state.poll_finished(cx, &me.pool)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_line_breaks_in_arguments() {
        assert!(check_arg("/var/www/a.txt").is_ok());
        for bad in ["a\r\nDELE b", "a\nb", "a\rb", "a\0b"] {
            assert!(
                matches!(check_arg(bad), Err(FsError::Name(NameError::Control))),
                "{bad:?}"
            );
        }
    }

    #[test]
    fn parses_posix_dos_and_mlsd_lines() {
        let e = parse_list_line(
            "-rw-r--r--    1 1000     1000         1234 Oct 15 10:02 app.js",
            false,
        )
        .unwrap();
        assert_eq!(
            (e.name.as_str(), e.kind, e.size),
            ("app.js", EntryKind::File, 1234)
        );
        assert_eq!(e.permissions, Some(0o644));

        let d = parse_list_line(
            "drwxr-x---    2 1000     1000         4096 Jan 01  2025 assets",
            false,
        )
        .unwrap();
        assert_eq!((d.name.as_str(), d.kind), ("assets", EntryKind::Dir));
        assert_eq!(d.permissions, Some(0o750));

        let l = parse_list_line(
            "lrwxrwxrwx    1 0        0              11 Oct 15 10:02 current -> releases/42",
            false,
        )
        .unwrap();
        assert_eq!((l.name.as_str(), l.kind), ("current", EntryKind::Symlink));

        let w = parse_list_line("10-15-26  10:02AM       <DIR>          Logs", false).unwrap();
        assert_eq!((w.name.as_str(), w.kind), ("Logs", EntryKind::Dir));

        let m = parse_list_line(
            "type=file;size=42;modify=20261015100200;UNIX.mode=0640; report.pdf",
            true,
        )
        .unwrap();
        assert_eq!(
            (m.name.as_str(), m.kind, m.size),
            ("report.pdf", EntryKind::File, 42)
        );

        assert!(parse_list_line("total 12", false).is_none());
        assert!(parse_list_line("drwxr-xr-x 2 0 0 4096 Oct 15 10:02 .", false).is_none());
        assert!(parse_list_line("drwxr-xr-x 2 0 0 4096 Oct 15 10:02 ..", false).is_none());
    }

    #[tokio::test]
    async fn listing_is_bounded() {
        let line = "-rw-r--r--    1 1000     1000            1 Oct 15 10:02 f\r\n";
        let data = line.repeat(MAX_LIST_ENTRIES + 5);
        let (entries, truncated) = read_listing(data.as_bytes(), false).await.unwrap();
        assert_eq!(entries.len(), MAX_LIST_ENTRIES);
        assert!(truncated);

        let (entries, truncated) = read_listing(line.repeat(3).as_bytes(), false)
            .await
            .unwrap();
        assert_eq!(entries.len(), 3);
        assert!(!truncated);

        let long = format!("{}\n", "x".repeat(MAX_LIST_LINE + 10));
        assert!(read_listing(long.as_bytes(), false).await.is_err());
    }

    #[test]
    fn maps_ftp_replies() {
        let reply = |code: u32, body: &str| {
            LibError::UnexpectedResponse(suppaftp::types::Response::new(
                Status::from(code),
                body.as_bytes().to_vec(),
            ))
        };
        assert!(matches!(
            FsError::from(reply(421, "bye")),
            FsError::ConnectionLost
        ));
        let data = FsError::from(LibError::ConnectionError(io::Error::new(
            io::ErrorKind::ConnectionRefused,
            DataConnectError {
                addr: "127.0.0.1:21103".parse().unwrap(),
                source: io::ErrorKind::ConnectionRefused.into(),
            },
        )));
        assert!(matches!(data, FsError::DataConnection));
        assert!(data.is_transient());
        // 제어 연결이 끊긴 경우는 그대로 다시 시도 대상
        let lost = LibError::ConnectionError(io::ErrorKind::UnexpectedEof.into());
        assert!(matches!(FsError::from(lost), FsError::ConnectionLost));
        assert!(matches!(
            FsError::from(reply(530, "no")),
            FsError::PermissionDenied
        ));
        assert!(matches!(
            FsError::from(reply(550, "550 Permission denied.\r\n")),
            FsError::Remote(m) if m == "550 Permission denied."
        ));
    }
}
