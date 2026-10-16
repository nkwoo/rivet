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
