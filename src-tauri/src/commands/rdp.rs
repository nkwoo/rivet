use secrecy::SecretString;
use tauri::ipc::{Channel, InvokeResponseBody};
use tauri::State;

use super::CmdResult;
use crate::core::session::{prepare_rdp_request, SessionEvent, SessionId, SessionSink};
use crate::core::AppState;
use crate::error::Error;
use crate::protocols::rdp::clipboard::SystemClipboard;
use crate::protocols::rdp::input::{InputEvent, MAX_EVENTS};
use crate::protocols::rdp::{DisplaySize, RdpCommand};

/// 세션 이벤트는 JSON, 화면 갱신은 바이너리로 웹뷰에 보냅니다.
struct ChannelSink {
    events: Channel<SessionEvent>,
    frames: Channel<InvokeResponseBody>,
}

impl SessionSink for ChannelSink {
    fn output(&self, data: &[u8]) {
        self.output_owned(data.to_vec());
    }

    fn output_owned(&self, data: Vec<u8>) {
        let _ = self.frames.send(InvokeResponseBody::Raw(data));
    }

    fn event(&self, event: SessionEvent) {
        let _ = self.events.send(event);
    }
}

/// 프로필로 RDP 세션을 시작하고 세션 ID를 돌려줍니다.
///
/// `width`/`height`/`scale`은 처음 요청할 원격 화면 크기와 배율(%)입니다.
/// 세션 종료와 인증서 승인 응답은 SSH와 같은 `ssh_close`, `ssh_host_key_respond`를 씁니다.
#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub async fn rdp_connect(
    state: State<'_, AppState>,
    profile_id: String,
    width: u16,
    height: u16,
    scale: u32,
    secret: Option<String>,
    save_secret: bool,
    events: Channel<SessionEvent>,
    frames: Channel<InvokeResponseBody>,
) -> CmdResult<SessionId> {
    let profile = state.profiles.get(&profile_id)?;
    let secrets = state.secrets.clone();
    let secret = secret.map(SecretString::from);
    let size = DisplaySize {
        width,
        height,
        scale,
    };
    // keyring 조회는 블로킹 작업입니다.
    let request = tauri::async_runtime::spawn_blocking(move || {
        prepare_rdp_request(&profile, secret, save_secret, secrets, size)
    })
    .await
    .map_err(|e| Error::Invalid(e.to_string()))??;
    Ok(state.sessions.spawn_rdp(
        request,
        state.cert_pins.clone(),
        std::sync::Arc::new(ChannelSink { events, frames }),
        std::sync::Arc::new(SystemClipboard),
    ))
}

/// 키보드/마우스 입력 묶음
#[tauri::command]
pub async fn rdp_input(
    state: State<'_, AppState>,
    session_id: String,
    events: Vec<InputEvent>,
) -> CmdResult<()> {
    if events.len() > MAX_EVENTS {
        return Err(Error::Invalid("too many input events".into()).into());
    }
    Ok(state
        .sessions
        .send_rdp(&session_id, RdpCommand::Input(events))
        .await?)
}

/// 원격 화면 크기 변경 요청. 서버가 지원하지 않으면 무시되고 웹뷰가 화면을 늘리거나 줄여 그립니다.
#[tauri::command]
pub async fn rdp_resize(
    state: State<'_, AppState>,
    session_id: String,
    width: u16,
    height: u16,
    scale: u32,
) -> CmdResult<()> {
    let size = DisplaySize {
        width,
        height,
        scale,
    };
    Ok(state
        .sessions
        .send_rdp(&session_id, RdpCommand::Resize(size))
        .await?)
}

/// 화면 메시지 하나를 그렸음
#[tauri::command]
pub async fn rdp_frame_ack(state: State<'_, AppState>, session_id: String) -> CmdResult<()> {
    Ok(state
        .sessions
        .send_rdp(&session_id, RdpCommand::FrameAck)
        .await?)
}

/// 클립보드 공유 켜기/끄기. `enabled`가 없으면 로컬 클립보드 변경만 알립니다(포커스 복귀 시).
#[tauri::command]
pub async fn rdp_clipboard(
    state: State<'_, AppState>,
    session_id: String,
    enabled: Option<bool>,
) -> CmdResult<()> {
    let command = match enabled {
        Some(enabled) => RdpCommand::SetClipboard(enabled),
        None => RdpCommand::ClipboardSync,
    };
    Ok(state.sessions.send_rdp(&session_id, command).await?)
}
