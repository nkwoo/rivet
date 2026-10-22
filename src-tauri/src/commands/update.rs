//! 자동 업데이트 명령. 업데이트 주소와 공개 키는 tauri.conf.json에 고정되어 있습니다.

use tauri::ipc::Channel;
use tauri::{AppHandle, State};
use tauri_plugin_updater::UpdaterExt;

use super::CmdResult;
use crate::core::update::{self, PendingUpdate, UpdateEvent, UpdateInfo};
use crate::core::AppState;
use crate::error::Error;

/// 새 버전이 있으면 정보를 돌려주고, 설치할 수 있게 기억해 둡니다.
#[tauri::command]
pub async fn update_check(
    app: AppHandle,
    pending: State<'_, PendingUpdate>,
) -> CmdResult<Option<UpdateInfo>> {
    let updater = app
        .updater_builder()
        .timeout(update::CHECK_TIMEOUT)
        .build()
        .map_err(update::plugin_error)?;
    let found = updater.check().await.map_err(update::plugin_error)?;
    let info = found.as_ref().map(UpdateInfo::from_update);
    pending.set(found);
    Ok(info)
}

/// 마지막으로 확인한 업데이트를 내려받아 설치하고 앱을 다시 시작합니다.
///
/// 설치 직전에 열린 세션을 모두 닫습니다. Windows는 설치 프로그램을 띄운 뒤 앱이 바로 종료되고,
/// 설치가 끝나면 설치 프로그램이 앱을 다시 실행합니다.
#[tauri::command]
pub async fn update_install(
    app: AppHandle,
    state: State<'_, AppState>,
    pending: State<'_, PendingUpdate>,
    events: Channel<UpdateEvent>,
) -> CmdResult<()> {
    let mut found = pending
        .take()
        .ok_or_else(|| Error::NotFound("update".into()))?;
    found.timeout = Some(update::DOWNLOAD_TIMEOUT);
    let bytes = update::download(&found, |e| {
        let _ = events.send(e);
    })
    .await?;
    state.sessions.close_all();
    let _ = events.send(UpdateEvent::Installing);
    found.install(bytes).map_err(update::plugin_error)?;
    tracing::info!(version = %found.version, "update installed, restarting");
    app.restart()
}
