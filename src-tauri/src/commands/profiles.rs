use tauri::State;

use super::CmdResult;
use crate::core::profile::{Profile, ProfileInput};
use crate::core::AppState;

#[tauri::command]
pub fn profile_list(state: State<'_, AppState>) -> CmdResult<Vec<Profile>> {
    Ok(state.profiles.list()?)
}

#[tauri::command]
pub fn profile_create(state: State<'_, AppState>, input: ProfileInput) -> CmdResult<Profile> {
    Ok(state.profiles.create(input)?)
}

#[tauri::command]
pub fn profile_update(
    state: State<'_, AppState>,
    id: String,
    input: ProfileInput,
) -> CmdResult<Profile> {
    Ok(state.profiles.update(&id, input)?)
}

#[tauri::command]
pub async fn profile_delete(state: State<'_, AppState>, id: String) -> CmdResult<()> {
    Ok(state.delete_profile(&id)?)
}

/// keyring에 저장된 비밀번호/패스프레이즈를 지웁니다.
#[tauri::command]
pub async fn profile_forget_secret(state: State<'_, AppState>, id: String) -> CmdResult<()> {
    state.profiles.get(&id)?;
    Ok(state.secrets.delete(&id)?)
}

/// `~/.ssh/config`의 호스트 목록 (가져오기 후보). 파일 읽기는 블로킹이라 별도 스레드에서 실행합니다.
#[tauri::command]
pub async fn ssh_config_scan() -> CmdResult<Vec<crate::core::ssh_config::SshConfigHost>> {
    tauri::async_runtime::spawn_blocking(crate::core::ssh_config::load_user_config)
        .await
        .map_err(|e| crate::error::Error::Invalid(e.to_string()).into())
}
