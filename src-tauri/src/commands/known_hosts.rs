use tauri::State;

use super::CmdResult;
use crate::core::known_hosts::KnownHost;
use crate::core::AppState;

#[tauri::command]
pub fn known_hosts_list(state: State<'_, AppState>) -> CmdResult<Vec<KnownHost>> {
    Ok(state.known_hosts.list()?)
}

/// 저장된 호스트 키를 삭제합니다. 키가 바뀐 서버에 다시 접속하려면 이 과정이 필요합니다.
#[tauri::command]
pub fn known_hosts_remove(state: State<'_, AppState>, host: String, port: u16) -> CmdResult<()> {
    Ok(state.known_hosts.remove(&host, port)?)
}
