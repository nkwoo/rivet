//! `#[tauri::command]` 진입점. 인자 변환만 하고 실제 로직은 `core`/`protocols`에 위임합니다.
//!
//! 새 명령을 추가하면 `build.rs`의 `AppManifest::commands`와
//! `capabilities/default.json`에도 함께 등록해야 웹뷰에서 호출할 수 있습니다.

pub mod app;
pub mod files;
pub mod ftp;
pub mod known_hosts;
pub mod profiles;
pub mod rdp;
pub mod ssh;
pub mod transfer;
pub mod update;

use serde::Serialize;

use crate::error::Error;

/// 웹뷰로 전달되는 에러. `code`로 분기하고 `message`는 표시용입니다.
#[derive(Debug, Serialize)]
pub struct CommandError {
    code: &'static str,
    message: String,
    /// 에러가 가리키는 다른 프로필 (점프 호스트 비밀번호 요청 등)
    #[serde(skip_serializing_if = "Option::is_none")]
    profile_id: Option<String>,
}

impl From<Error> for CommandError {
    fn from(e: Error) -> Self {
        let message = match &e {
            // 내부 에러의 상세 내용(경로, SQL 등)은 로그에만 남깁니다.
            Error::Tauri(_) | Error::Io(_) | Error::Database(_) => {
                tracing::error!(error = %e, "internal error");
                "internal error".to_owned()
            }
            _ => e.to_string(),
        };
        let profile_id = match &e {
            Error::JumpSecretRequired { profile_id, .. } => Some(profile_id.clone()),
            _ => None,
        };
        Self {
            code: e.code(),
            message,
            profile_id,
        }
    }
}

impl From<tauri::Error> for CommandError {
    fn from(e: tauri::Error) -> Self {
        Error::from(e).into()
    }
}

pub type CmdResult<T> = Result<T, CommandError>;
