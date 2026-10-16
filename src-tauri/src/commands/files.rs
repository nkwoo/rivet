//! 파일 탐색기: 로컬 목록과 SFTP 원격 폴더 작업

use tauri::State;

use super::CmdResult;
use crate::core::AppState;
use crate::error::Error;
use crate::protocols::Listing;
use crate::transfer::{local, paths::remote_join};

/// 웹뷰에서 받는 경로 길이 상한
const MAX_PATH_LEN: usize = 4096;

pub(super) fn check_path(path: &str) -> Result<(), Error> {
    if path.is_empty() || path.len() > MAX_PATH_LEN || path.contains('\0') {
        return Err(Error::Invalid("invalid path".into()));
    }
    Ok(())
}

#[tauri::command]
pub fn local_home() -> CmdResult<String> {
    Ok(local::home().map_err(Error::from)?)
}

#[tauri::command]
pub async fn local_list(path: String) -> CmdResult<Listing> {
    check_path(&path)?;
    let listing = tauri::async_runtime::spawn_blocking(move || local::list(&path))
        .await
        .map_err(|e| Error::Invalid(e.to_string()))?
        .map_err(Error::from)?;
    Ok(listing)
}

/// 세션의 SSH 연결에 SFTP를 열고 시작 폴더를 돌려줍니다.
#[tauri::command]
pub async fn sftp_open(state: State<'_, AppState>, session_id: String) -> CmdResult<String> {
    let fs = state.sessions.remote_fs(&session_id).await?;
    Ok(fs.home().await.map_err(Error::from)?)
}

#[tauri::command]
pub async fn sftp_list(
    state: State<'_, AppState>,
    session_id: String,
    path: String,
) -> CmdResult<Listing> {
    check_path(&path)?;
    let fs = state.sessions.remote_fs(&session_id).await?;
    Ok(fs.list(&path).await.map_err(Error::from)?)
}

#[tauri::command]
pub async fn sftp_mkdir(
    state: State<'_, AppState>,
    session_id: String,
    dir: String,
    name: String,
) -> CmdResult<()> {
    check_path(&dir)?;
    let path = remote_join(&dir, &name).map_err(|e| Error::Fs(e.into()))?;
    let fs = state.sessions.remote_fs(&session_id).await?;
    Ok(fs.mkdir(&path).await.map_err(Error::from)?)
}

#[tauri::command]
pub async fn sftp_rename(
    state: State<'_, AppState>,
    session_id: String,
    dir: String,
    from: String,
    to: String,
) -> CmdResult<()> {
    check_path(&dir)?;
    let old = remote_join(&dir, &from).map_err(|e| Error::Fs(e.into()))?;
    let new = remote_join(&dir, &to).map_err(|e| Error::Fs(e.into()))?;
    let fs = state.sessions.remote_fs(&session_id).await?;
    Ok(fs.rename(&old, &new).await.map_err(Error::from)?)
}

/// 파일 또는 빈 폴더를 삭제합니다.
#[tauri::command]
pub async fn sftp_remove(
    state: State<'_, AppState>,
    session_id: String,
    dir: String,
    name: String,
    is_dir: bool,
) -> CmdResult<()> {
    check_path(&dir)?;
    let path = remote_join(&dir, &name).map_err(|e| Error::Fs(e.into()))?;
    let fs = state.sessions.remote_fs(&session_id).await?;
    let result = if is_dir {
        fs.remove_dir(&path).await
    } else {
        fs.remove_file(&path).await
    };
    Ok(result.map_err(Error::from)?)
}
