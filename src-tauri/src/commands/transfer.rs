//! 전송 큐 명령

use tauri::ipc::Channel;
use tauri::State;

use super::files::check_path;
use super::CmdResult;
use crate::core::AppState;
use crate::error::Error;
use crate::transfer::{ConflictAction, JobInfo, OverwritePolicy, TransferEvent, TransferSink};

/// 한 번에 큐에 넣을 수 있는 선택 항목 수
const MAX_SOURCES: usize = 1000;

struct ChannelSink(Channel<TransferEvent>);

impl TransferSink for ChannelSink {
    fn send(&self, event: TransferEvent) {
        let _ = self.0.send(event);
    }
}

fn check_sources(sources: &[String]) -> Result<(), Error> {
    if sources.is_empty() || sources.len() > MAX_SOURCES {
        return Err(Error::Invalid("invalid selection".into()));
    }
    sources.iter().try_for_each(|s| check_path(s))
}

/// 전송 이벤트를 받을 채널을 등록하고 현재 목록을 돌려줍니다.
#[tauri::command]
pub fn transfer_subscribe(
    state: State<'_, AppState>,
    events: Channel<TransferEvent>,
) -> Vec<JobInfo> {
    state
        .transfers
        .set_sink(std::sync::Arc::new(ChannelSink(events)));
    state.transfers.snapshot()
}

#[tauri::command]
pub async fn transfer_upload(
    state: State<'_, AppState>,
    session_id: String,
    sources: Vec<String>,
    target_dir: String,
    policy: OverwritePolicy,
) -> CmdResult<Vec<JobInfo>> {
    check_sources(&sources)?;
    check_path(&target_dir)?;
    let fs = state.sessions.remote_fs(&session_id).await?;
    Ok(state
        .transfers
        .enqueue_upload(fs, &session_id, sources, &target_dir, policy)
        .await?)
}

#[tauri::command]
pub async fn transfer_download(
    state: State<'_, AppState>,
    session_id: String,
    sources: Vec<String>,
    dest_dir: String,
    policy: OverwritePolicy,
) -> CmdResult<Vec<JobInfo>> {
    check_sources(&sources)?;
    check_path(&dest_dir)?;
    let fs = state.sessions.remote_fs(&session_id).await?;
    Ok(state
        .transfers
        .enqueue_download(fs, &session_id, sources, &dest_dir, policy)
        .await?)
}

#[tauri::command]
pub fn transfer_resolve(
    state: State<'_, AppState>,
    job_id: String,
    action: ConflictAction,
) -> CmdResult<()> {
    Ok(state.transfers.resolve(&job_id, action)?)
}

#[tauri::command]
pub fn transfer_cancel(state: State<'_, AppState>, job_id: String) {
    state.transfers.cancel(&job_id);
}

#[tauri::command]
pub async fn transfer_retry(state: State<'_, AppState>, job_id: String) -> CmdResult<()> {
    Ok(state.transfers.retry(&job_id)?)
}

#[tauri::command]
pub fn transfer_clear_finished(state: State<'_, AppState>) {
    state.transfers.clear_finished();
}
