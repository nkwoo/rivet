//! Rivet 백엔드: SSH / SFTP / FTP(S) / RDP 통합 원격 접속 클라이언트.
//!
//! 계층: `commands`(얇은 Tauri 진입점) → `core`(프로필, 비밀 값, 세션) → `protocols`(네트워크).

mod commands;
mod core;
mod error;
#[cfg(test)]
mod integration_tests;
// 메뉴는 macOS에서만 붙이지만, 언어 설정 명령이 `MenuLocale`을 쓰므로 모든 OS에서 컴파일합니다.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
mod menu;
mod protocols;
mod transfer;

use tauri::Manager;

pub use error::Error;

/// `RUST_LOG` 설정과 무관하게 비밀 값이 들어갈 수 있는 로그를 버립니다.
///
/// suppaftp는 TRACE 레벨에서 보내는 FTP 명령을 그대로 기록하므로 `PASS <비밀번호>`가 남습니다.
pub(crate) fn secret_safe_filter(
) -> tracing_subscriber::filter::FilterFn<impl Fn(&tracing::Metadata<'_>) -> bool> {
    tracing_subscriber::filter::filter_fn(|meta| {
        !(meta.target().starts_with("suppaftp") && *meta.level() == tracing::Level::TRACE)
    })
}

/// Tauri 앱을 초기화하고 이벤트 루프를 실행합니다.
pub fn run() -> Result<(), Error> {
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;
    use tracing_subscriber::Layer;

    let env = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer()
                .with_filter(env)
                .with_filter(secret_safe_filter()),
        )
        .init();

    let app = tauri::Builder::default()
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            let state = core::AppState::open(&data_dir)?;
            app.manage(state);
            #[cfg(target_os = "macos")]
            {
                app.set_menu(menu::build(app.handle(), menu::MenuLocale::default())?)?;
                app.on_menu_event(|app, event| menu::handle_event(app, event.id().as_ref()));
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::app::app_info,
            commands::app::app_set_menu_locale,
            commands::profiles::profile_list,
            commands::profiles::profile_create,
            commands::profiles::profile_update,
            commands::profiles::profile_delete,
            commands::profiles::profile_forget_secret,
            commands::profiles::ssh_config_scan,
            commands::known_hosts::known_hosts_list,
            commands::known_hosts::known_hosts_remove,
            commands::known_hosts::cert_pins_list,
            commands::known_hosts::cert_pins_remove,
            commands::ftp::ftp_connect,
            commands::rdp::rdp_connect,
            commands::rdp::rdp_input,
            commands::rdp::rdp_resize,
            commands::rdp::rdp_frame_ack,
            commands::rdp::rdp_clipboard,
            commands::ssh::ssh_connect,
            commands::ssh::ssh_write,
            commands::ssh::ssh_resize,
            commands::ssh::ssh_close,
            commands::ssh::ssh_host_key_respond,
            commands::files::local_home,
            commands::files::local_list,
            commands::files::sftp_open,
            commands::files::sftp_list,
            commands::files::sftp_mkdir,
            commands::files::sftp_rename,
            commands::files::sftp_remove,
            commands::transfer::transfer_subscribe,
            commands::transfer::transfer_upload,
            commands::transfer::transfer_download,
            commands::transfer::transfer_resolve,
            commands::transfer::transfer_cancel,
            commands::transfer::transfer_retry,
            commands::transfer::transfer_clear_finished,
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
