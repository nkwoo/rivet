//! Rivet 백엔드: SSH / SFTP / FTP(S) / RDP 통합 원격 접속 클라이언트.
//!
//! 계층: `commands`(얇은 Tauri 진입점) → `core`(프로필, 비밀 값, 세션) → `protocols`(네트워크).

mod commands;
mod core;
mod error;
mod protocols;
mod transfer;

pub use error::Error;

/// Tauri 앱을 초기화하고 이벤트 루프를 실행합니다.
pub fn run() -> Result<(), Error> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![commands::app::app_info])
        .run(tauri::generate_context!())?;
    Ok(())
}
