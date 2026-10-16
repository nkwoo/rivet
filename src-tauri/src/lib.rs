//! Rivet 백엔드: SSH / SFTP / FTP(S) / RDP 통합 원격 접속 클라이언트.
//!
//! 계층: `commands`(얇은 Tauri 진입점) → `core`(프로필, 비밀 값, 세션) → `protocols`(네트워크).

mod commands;
mod core;
mod error;
#[cfg(test)]
mod integration_tests;
#[cfg(target_os = "macos")]
mod menu;
mod protocols;
mod transfer;

use tauri::Manager;

pub use error::Error;

/// Tauri 앱을 초기화하고 이벤트 루프를 실행합니다.
pub fn run() -> Result<(), Error> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let app = tauri::Builder::default()
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            let state = core::AppState::open(&data_dir)?;
            app.manage(state);
            #[cfg(target_os = "macos")]
            {
                app.set_menu(menu::build(app.handle())?)?;
                app.on_menu_event(|app, event| menu::handle_event(app, event.id().as_ref()));
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::app::app_info,
            commands::profiles::profile_list,
            commands::profiles::profile_create,
            commands::profiles::profile_update,
            commands::profiles::profile_delete,
            commands::profiles::profile_forget_secret,
            commands::known_hosts::known_hosts_list,
            commands::known_hosts::known_hosts_remove,
            commands::ssh::ssh_connect,
            commands::ssh::ssh_write,
            commands::ssh::ssh_resize,
            commands::ssh::ssh_close,
            commands::ssh::ssh_host_key_respond,
        ])
        .build(tauri::generate_context!())?;

    app.run(|handle, event| {
        if let tauri::RunEvent::Exit = event {
            if let Some(state) = handle.try_state::<core::AppState>() {
                state.sessions.close_all();
            }
        }
    });
    Ok(())
}
