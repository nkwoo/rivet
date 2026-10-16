use secrecy::SecretString;
use tauri::ipc::{Channel, InvokeResponseBody};
use tauri::State;

use super::CmdResult;
use crate::core::session::{
    check_terminal_size, prepare_ssh_request, SessionEvent, SessionId, SessionSink,
};
use crate::core::AppState;
use crate::error::Error;
use crate::protocols::ssh::ShellCommand;

/// 한 번에 보낼 수 있는 터미널 입력 크기 상한 (붙여넣기 포함)
const MAX_INPUT_BYTES: usize = 64 * 1024;

/// 세션 이벤트는 JSON, 터미널 출력은 바이너리로 웹뷰에 보냅니다.
struct ChannelSink {
    events: Channel<SessionEvent>,
    output: Channel<InvokeResponseBody>,
}

impl SessionSink for ChannelSink {
    fn output(&self, data: &[u8]) {
        let _ = self.output.send(InvokeResponseBody::Raw(data.to_vec()));
    }

    fn event(&self, event: SessionEvent) {
        let _ = self.events.send(event);
    }
}

/// 프로필로 SSH 세션을 시작하고 세션 ID를 돌려줍니다. 연결 진행 상황은 `events`로 전달됩니다.
///
/// `secret`은 사용자가 방금 입력한 비밀번호/패스프레이즈입니다. 없으면 keyring에서 찾습니다.
#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub async fn ssh_connect(
    state: State<'_, AppState>,
    profile_id: String,
    cols: u32,
    rows: u32,
    secret: Option<String>,
    save_secret: bool,
    events: Channel<SessionEvent>,
    output: Channel<InvokeResponseBody>,
) -> CmdResult<SessionId> {
    let profile = state.profiles.get(&profile_id)?;
    let secrets = state.secrets.clone();
    let secret = secret.map(SecretString::from);
    // keyring 조회와 키 파일 복호화는 블로킹 작업입니다.
    let request = tauri::async_runtime::spawn_blocking(move || {
        prepare_ssh_request(&profile, secret, save_secret, secrets, cols, rows)
    })
    .await
    .map_err(|e| Error::Invalid(e.to_string()))??;

    let sink = std::sync::Arc::new(ChannelSink { events, output });
    Ok(state
        .sessions
        .spawn_ssh(request, state.known_hosts.clone(), sink))
}

#[tauri::command]
pub async fn ssh_write(
    state: State<'_, AppState>,
    session_id: String,
    data: String,
) -> CmdResult<()> {
    if data.len() > MAX_INPUT_BYTES {
        return Err(Error::Invalid("input too large".into()).into());
    }
    Ok(state
        .sessions
        .send(&session_id, ShellCommand::Input(data.into_bytes()))
        .await?)
}

#[tauri::command]
pub async fn ssh_resize(
    state: State<'_, AppState>,
    session_id: String,
    cols: u32,
    rows: u32,
) -> CmdResult<()> {
    let (cols, rows) = check_terminal_size(cols, rows)?;
    Ok(state
        .sessions
        .send(&session_id, ShellCommand::Resize { cols, rows })
        .await?)
}

/// 세션을 닫고 그 세션의 남은 전송을 취소합니다. (연결 종료를 위해 비동기 런타임에서 실행)
#[tauri::command]
pub async fn ssh_close(state: State<'_, AppState>, session_id: String) -> CmdResult<()> {
    state.transfers.cancel_session(&session_id);
    state.sessions.close(&session_id);
    Ok(())
}

/// 처음 보는 호스트 키 승인 대화상자의 응답
#[tauri::command]
pub fn ssh_host_key_respond(
    state: State<'_, AppState>,
    session_id: String,
    accept: bool,
) -> CmdResult<()> {
    Ok(state.sessions.respond_host_key(&session_id, accept)?)
}
