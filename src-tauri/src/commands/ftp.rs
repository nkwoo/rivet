use secrecy::SecretString;
use tauri::ipc::Channel;
use tauri::State;

use super::CmdResult;
use crate::core::session::{prepare_ftp_request, SessionEvent, SessionId, SessionSink};
use crate::core::AppState;
use crate::error::Error;

struct EventSink(Channel<SessionEvent>);

impl SessionSink for EventSink {
    fn output(&self, _: &[u8]) {}

    fn event(&self, event: SessionEvent) {
        let _ = self.0.send(event);
    }
}

/// 프로필로 FTP/FTPS 세션을 시작하고 세션 ID를 돌려줍니다. 진행 상황은 `events`로 전달됩니다.
/// 세션 종료와 인증서 승인 응답은 SSH와 같은 `ssh_close`, `ssh_host_key_respond`를 씁니다.
#[tauri::command]
pub async fn ftp_connect(
    state: State<'_, AppState>,
    profile_id: String,
    secret: Option<String>,
    save_secret: bool,
    events: Channel<SessionEvent>,
) -> CmdResult<SessionId> {
    let profile = state.profiles.get(&profile_id)?;
    let secrets = state.secrets.clone();
    let secret = secret.map(SecretString::from);
    // keyring 조회는 블로킹 작업입니다.
    let request = tauri::async_runtime::spawn_blocking(move || {
        prepare_ftp_request(&profile, secret, save_secret, secrets)
    })
    .await
    .map_err(|e| Error::Invalid(e.to_string()))??;
    Ok(state.sessions.spawn_ftp(
        request,
        state.cert_pins.clone(),
        std::sync::Arc::new(EventSink(events)),
    ))
}
