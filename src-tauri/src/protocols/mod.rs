//! 네트워크 프로토콜 구현. 각 모듈은 공통 trait(`RemoteSession`, `RemoteFs`)을 구현합니다.

pub mod ftp;
pub mod rdp;
pub mod sftp;
pub mod ssh;

use async_trait::async_trait;
use serde::Serialize;
use tokio::io::{AsyncRead, AsyncWrite};

use crate::transfer::paths::NameError;

/// 프로토콜과 무관한 원격 연결의 수명주기
#[async_trait]
pub trait RemoteSession: Send + Sync {
    /// 연결을 정상 종료합니다. 이미 끊긴 연결이면 아무 일도 하지 않습니다.
    async fn disconnect(&self);
}

/// 한 폴더에서 돌려주는 항목 수 상한. 넘으면 앞부분만 보여주고 `truncated`를 표시합니다.
pub const MAX_LIST_ENTRIES: usize = 20_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum EntryKind {
    File,
    Dir,
    Symlink,
    Other,
}

/// 파일 탐색기 한 줄. 로컬과 원격이 같은 형태를 씁니다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FsEntry {
    pub name: String,
    pub kind: EntryKind,
    pub size: u64,
    /// 수정 시각 (Unix 초)
    pub modified: Option<i64>,
    /// POSIX 권한 비트 (형식 비트 제외)
    pub permissions: Option<u32>,
}

/// 폴더 목록 조회 결과
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Listing {
    /// 정규화된 폴더 경로
    pub path: String,
    pub parent: Option<String>,
    pub entries: Vec<FsEntry>,
    /// 항목이 [`MAX_LIST_ENTRIES`]를 넘어 일부만 담겼는지
    pub truncated: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum FsError {
    #[error("no such file or folder")]
    NotFound,
    #[error("permission denied")]
    PermissionDenied,
    #[error("already exists")]
    AlreadyExists,
    #[error("not a folder")]
    NotADirectory,
    #[error("folder has more than {MAX_LIST_ENTRIES} entries")]
    TooManyEntries,
    #[error("blocked by path check: {0}")]
    Name(#[from] NameError),
    #[error("symbolic links are not followed")]
    Symlink,
    #[error("server did not respond in time")]
    Timeout,
    #[error("connection lost")]
    ConnectionLost,
    #[error("{0}")]
    Remote(String),
    #[error("{0}")]
    Io(#[from] std::io::Error),
}

impl FsError {
    pub fn code(&self) -> &'static str {
        match self {
            FsError::NotFound => "fsNotFound",
            FsError::PermissionDenied => "permissionDenied",
            FsError::AlreadyExists => "alreadyExists",
            FsError::NotADirectory => "notADirectory",
            FsError::TooManyEntries => "tooManyEntries",
            FsError::Name(_) => "pathBlocked",
            FsError::Symlink => "symlink",
            FsError::Timeout => "timeout",
            FsError::ConnectionLost => "connectionLost",
            FsError::Remote(_) => "remote",
            FsError::Io(_) => "io",
        }
    }

    /// 다시 시도하면 성공할 수 있는 일시적 오류인지 (자동 재시도 대상)
    pub fn is_transient(&self) -> bool {
        use std::io::ErrorKind::*;
        match self {
            FsError::Timeout | FsError::ConnectionLost => true,
            FsError::Io(e) => matches!(
                e.kind(),
                TimedOut
                    | ConnectionReset
                    | ConnectionAborted
                    | BrokenPipe
                    | UnexpectedEof
                    | Interrupted
            ),
            _ => false,
        }
    }
}

/// 원격 파일 시스템. 전송 큐와 파일 탐색기는 프로토콜 대신 이 trait만 사용합니다.
///
/// 경로는 서버 기준 `/` 구분 문자열입니다. 심볼릭 링크는 따라가지 않습니다(`stat`은 lstat).
#[async_trait]
pub trait RemoteFs: Send + Sync {
    /// 로그인 직후의 작업 폴더 (보통 홈 폴더)
    async fn home(&self) -> Result<String, FsError>;
    async fn list(&self, path: &str) -> Result<Listing, FsError>;
    /// 항목 정보. 없으면 `None`
    async fn stat(&self, path: &str) -> Result<Option<FsEntry>, FsError>;
    async fn mkdir(&self, path: &str) -> Result<(), FsError>;
    async fn remove_file(&self, path: &str) -> Result<(), FsError>;
    /// 빈 폴더만 삭제합니다.
    async fn remove_dir(&self, path: &str) -> Result<(), FsError>;
    async fn rename(&self, from: &str, to: &str) -> Result<(), FsError>;
    async fn open_read(&self, path: &str) -> Result<Box<dyn AsyncRead + Send + Unpin>, FsError>;
    /// 파일을 만들거나 내용을 비우고 엽니다. 마지막에 `shutdown()`으로 쓰기 완료를 확인해야 합니다.
    async fn open_write(&self, path: &str) -> Result<Box<dyn AsyncWrite + Send + Unpin>, FsError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transient_errors_are_retryable() {
        assert!(FsError::ConnectionLost.is_transient());
        assert!(FsError::Io(std::io::ErrorKind::BrokenPipe.into()).is_transient());
        assert!(!FsError::PermissionDenied.is_transient());
        assert!(!FsError::Name(NameError::DotName).is_transient());
    }
}
