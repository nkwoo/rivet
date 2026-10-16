use crate::protocols::ftp::FtpError;
use crate::protocols::ssh::SshError;
use crate::protocols::FsError;

/// 백엔드 공통 에러 타입.
///
/// 비밀 값(비밀번호, 패스프레이즈)은 에러 메시지에 절대 포함하지 않습니다.
/// keyring 에러처럼 내부에 비밀 값이 들어 있을 수 있는 에러는 원본을 보관하지 않고
/// 설명 문자열로만 변환합니다.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("tauri: {0}")]
    Tauri(#[from] tauri::Error),

    #[error("database: {0}")]
    Database(#[from] rusqlite::Error),

    #[error("io: {0}")]
    Io(#[from] std::io::Error),

    #[error("secret store: {0}")]
    SecretStore(String),

    #[error("invalid input: {0}")]
    Invalid(String),

    #[error("not found: {0}")]
    NotFound(String),

    /// 저장된 비밀번호가 없어 사용자 입력이 필요함
    #[error("password required")]
    PasswordRequired,

    #[error(transparent)]
    Ssh(#[from] SshError),

    #[error(transparent)]
    Fs(#[from] FsError),

    #[error(transparent)]
    Ftp(#[from] FtpError),
}

impl Error {
    /// 프론트엔드가 분기할 때 쓰는 안정적인 에러 코드
    pub fn code(&self) -> &'static str {
        match self {
            Error::Tauri(_) | Error::Io(_) | Error::Database(_) => "internal",
            Error::SecretStore(_) => "secretStore",
            Error::Invalid(_) => "invalid",
            Error::NotFound(_) => "notFound",
            Error::PasswordRequired => "passwordRequired",
            Error::Ssh(e) => e.code(),
            Error::Fs(e) => e.code(),
            Error::Ftp(e) => e.code(),
        }
    }
}

pub type Result<T, E = Error> = std::result::Result<T, E>;
