//! russh-sftp 기반 `RemoteFs` 구현.
//!
//! 터미널과 같은 SSH 연결 위에 SFTP 채널 두 개를 엽니다.
//! - 탐색용(`RawSftpSession`): 목록 조회를 직접 반복해 항목 수 상한을 적용합니다.
//!   (`SftpSession::read_dir`은 서버가 보내는 만큼 모두 메모리에 쌓습니다.)
//! - 전송용(`SftpSession`): 읽기/쓰기 요청을 여러 개 겹쳐 보내는 파일 핸들을 씁니다.
//!
//! 응답 패킷 하나의 크기는 `Config::max_packet_len`(256KiB)으로 제한됩니다.

use async_trait::async_trait;
use russh_sftp::client::error::Error as SftpError;
use russh_sftp::client::{Config, RawSftpSession, SftpSession};
use russh_sftp::protocol::{FileAttributes, FileType, OpenFlags, StatusCode};
use tokio::io::{AsyncRead, AsyncWrite};

use super::ssh::{SshConnection, SshError};
use super::{EntryKind, FsEntry, FsError, Listing, RemoteFs, MAX_LIST_ENTRIES};
use crate::transfer::paths::remote_parent;

/// 요청 하나의 응답 대기 시간 (초)
const REQUEST_TIMEOUT_SECS: u64 = 30;

impl From<SftpError> for FsError {
    fn from(e: SftpError) -> Self {
        match e {
            SftpError::Status(s) => match s.status_code {
                StatusCode::NoSuchFile => FsError::NotFound,
                StatusCode::PermissionDenied => FsError::PermissionDenied,
                StatusCode::NoConnection | StatusCode::ConnectionLost => FsError::ConnectionLost,
                _ if s.error_message.is_empty() => FsError::Remote(s.status_code.to_string()),
                _ => FsError::Remote(s.error_message),
            },
            SftpError::Timeout => FsError::Timeout,
            // 채널이 닫히면 요청 송신/응답 수신이 실패합니다.
            SftpError::IO(_) | SftpError::UnexpectedBehavior(_) => FsError::ConnectionLost,
            other => FsError::Remote(other.to_string()),
        }
    }
}

impl From<SshError> for FsError {
    fn from(e: SshError) -> Self {
        match e {
            SshError::Timeout => FsError::Timeout,
            other => FsError::Remote(other.to_string()),
        }
    }
}

fn config() -> Config {
    Config {
        request_timeout_secs: REQUEST_TIMEOUT_SECS,
        ..Config::default()
    }
}

pub fn entry_from_attrs(name: String, attrs: &FileAttributes) -> FsEntry {
    let kind = match attrs.file_type() {
        FileType::Dir => EntryKind::Dir,
        FileType::File => EntryKind::File,
        FileType::Symlink => EntryKind::Symlink,
        FileType::Other => EntryKind::Other,
    };
    FsEntry {
        name,
        kind,
        size: attrs.size.unwrap_or(0),
        modified: attrs.mtime.map(i64::from),
        permissions: attrs.permissions.map(|p| p & 0o7777),
    }
}

pub struct SftpFs {
    browse: RawSftpSession,
    files: SftpSession,
}

impl SftpFs {
    /// 인증된 SSH 연결에 SFTP 채널을 열고 프로토콜을 초기화합니다.
    pub async fn open(conn: &SshConnection) -> Result<Self, FsError> {
        let channel = conn.open_sftp_channel().await?;
        let browse = RawSftpSession::new_with_config(channel.into_stream(), config());
        browse.init().await?;

        let channel = conn.open_sftp_channel().await?;
        let files = SftpSession::new_with_config(channel.into_stream(), config()).await?;
        Ok(Self { browse, files })
    }

    async fn realpath(&self, path: &str) -> Result<String, FsError> {
        let name = self.browse.realpath(path).await?;
        name.files
            .into_iter()
            .next()
            .map(|f| f.filename)
            .ok_or_else(|| FsError::Remote("server returned no path".into()))
    }
}

#[async_trait]
impl RemoteFs for SftpFs {
    async fn home(&self) -> Result<String, FsError> {
        self.realpath(".").await
    }

    async fn list(&self, path: &str) -> Result<Listing, FsError> {
        let path = self.realpath(path).await?;
        let handle = match self.browse.opendir(path.as_str()).await {
            Ok(h) => h.handle,
            // 파일을 폴더처럼 열면 서버마다 Failure/NoSuchFile 등으로 응답합니다.
            Err(e) => {
                return Err(match self.stat(&path).await {
                    Ok(Some(entry)) if entry.kind != EntryKind::Dir => FsError::NotADirectory,
                    _ => e.into(),
                })
            }
        };

        let mut entries = Vec::new();
        let mut truncated = false;
        let result = loop {
            match self.browse.readdir(handle.as_str()).await {
                Ok(name) => {
                    for file in name.files {
                        if file.filename == "." || file.filename == ".." {
                            continue;
                        }
                        if entries.len() >= MAX_LIST_ENTRIES {
                            truncated = true;
                            break;
                        }
                        entries.push(entry_from_attrs(file.filename, &file.attrs));
                    }
                    if truncated {
                        break Ok(());
                    }
                }
                Err(SftpError::Status(s)) if s.status_code == StatusCode::Eof => break Ok(()),
                Err(e) => break Err(FsError::from(e)),
            }
        };
        let _ = self.browse.close(handle).await;
        result?;

        Ok(Listing {
            parent: remote_parent(&path),
            path,
            entries,
            truncated,
        })
    }

    async fn stat(&self, path: &str) -> Result<Option<FsEntry>, FsError> {
        match self.browse.lstat(path).await {
            Ok(attrs) => {
                let name = path.rsplit('/').next().unwrap_or(path).to_owned();
                Ok(Some(entry_from_attrs(name, &attrs.attrs)))
            }
            Err(SftpError::Status(s)) if s.status_code == StatusCode::NoSuchFile => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    async fn mkdir(&self, path: &str) -> Result<(), FsError> {
        if self.stat(path).await?.is_some() {
            return Err(FsError::AlreadyExists);
        }
        self.browse
            .mkdir(path, FileAttributes::empty())
            .await
            .map(|_| ())
            .map_err(Into::into)
    }

    async fn remove_file(&self, path: &str) -> Result<(), FsError> {
        self.browse
            .remove(path)
            .await
            .map(|_| ())
            .map_err(Into::into)
    }

    async fn remove_dir(&self, path: &str) -> Result<(), FsError> {
        self.browse
            .rmdir(path)
            .await
            .map(|_| ())
            .map_err(Into::into)
    }

    async fn rename(&self, from: &str, to: &str) -> Result<(), FsError> {
        if self.stat(to).await?.is_some() {
            return Err(FsError::AlreadyExists);
        }
        self.browse
            .rename(from, to)
            .await
            .map(|_| ())
            .map_err(Into::into)
    }

    async fn open_read(&self, path: &str) -> Result<Box<dyn AsyncRead + Send + Unpin>, FsError> {
        Ok(Box::new(self.files.open(path).await?))
    }

    async fn open_write(&self, path: &str) -> Result<Box<dyn AsyncWrite + Send + Unpin>, FsError> {
        let file = self
            .files
            .open_with_flags(
                path,
                OpenFlags::CREATE | OpenFlags::TRUNCATE | OpenFlags::WRITE,
            )
            .await?;
        Ok(Box::new(file))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_status_codes() {
        let status = |code, msg: &str| {
            SftpError::Status(russh_sftp::protocol::Status {
                id: 1,
                status_code: code,
                error_message: msg.into(),
                language_tag: "en".into(),
            })
        };
        assert!(matches!(
            FsError::from(status(StatusCode::NoSuchFile, "")),
            FsError::NotFound
        ));
        assert!(matches!(
            FsError::from(status(StatusCode::PermissionDenied, "x")),
            FsError::PermissionDenied
        ));
        assert!(matches!(
            FsError::from(status(StatusCode::Failure, "disk full")),
            FsError::Remote(m) if m == "disk full"
        ));
        assert!(FsError::from(SftpError::Timeout).is_transient());
    }

    #[test]
    fn entry_strips_file_type_bits() {
        let mut attrs = FileAttributes::empty();
        attrs.permissions = Some(0o100644);
        attrs.size = Some(42);
        attrs.mtime = Some(1_700_000_000);
        let e = entry_from_attrs("a".into(), &attrs);
        assert_eq!(e.kind, EntryKind::File);
        assert_eq!(e.permissions, Some(0o644));
        assert_eq!(e.size, 42);
        assert_eq!(e.modified, Some(1_700_000_000));
    }
}
