//! 자동 업데이트 (tauri-plugin-updater).
//!
//! 확인 → 사용자 승인 → 내려받기·서명 확인 → 설치 → 다시 시작 순서로 진행합니다.
//! 업데이트 주소, 공개 키, 서명된 버전 확인(`requireSignedVersion`)은 tauri.conf.json에 고정되어 있고,
//! 웹뷰는 플러그인 JS API 권한 없이 `update_check`/`update_install` 명령만 부릅니다.
//! 설치하는 것은 항상 마지막 확인에서 받은 업데이트이며, 웹뷰가 주소나 파일을 지정할 수 없습니다.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri_plugin_updater::Update;
use tokio::sync::Notify;

use crate::error::{Error, Result};

/// 업데이트 정보(latest.json) 요청 시간 상한
pub const CHECK_TIMEOUT: Duration = Duration::from_secs(30);
/// 설치 파일 내려받기 시간 상한
pub const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(15 * 60);
/// 설치 파일 크기 상한. 현재 설치 파일은 20MiB 안팎입니다.
pub const MAX_DOWNLOAD_BYTES: u64 = 256 * 1024 * 1024;
/// 웹뷰에 보여줄 변경 내용 길이 상한 (문자 수)
pub const MAX_NOTES_CHARS: usize = 4000;
const PROGRESS_INTERVAL: Duration = Duration::from_millis(100);

/// 웹뷰에 보여줄 새 버전 정보
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    pub version: String,
    pub current_version: String,
    /// 게시 시각 (유닉스 초)
    pub published_at: Option<i64>,
    pub notes: String,
}

impl UpdateInfo {
    pub fn new(
        version: &str,
        current_version: &str,
        published_at: Option<i64>,
        body: Option<&str>,
    ) -> Self {
        Self {
            version: version.to_owned(),
            current_version: current_version.to_owned(),
            published_at,
            notes: clean_notes(body.unwrap_or_default()),
        }
    }

    pub fn from_update(update: &Update) -> Self {
        Self::new(
            &update.version,
            &update.current_version,
            update.date.map(|d| d.unix_timestamp()),
            update.body.as_deref(),
        )
    }
}

/// 원격에서 받은 변경 내용에서 제어 문자를 빼고 길이를 자릅니다.
fn clean_notes(body: &str) -> String {
    let mut out: String = body
        .trim()
        .chars()
        .filter(|c| !c.is_control() || matches!(c, '\n' | '\t'))
        .take(MAX_NOTES_CHARS + 1)
        .collect();
    if out.chars().count() > MAX_NOTES_CHARS {
        out = out.chars().take(MAX_NOTES_CHARS).collect();
        out.push('…');
    }
    out
}

/// 설치 진행 상황
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum UpdateEvent {
    #[serde(rename_all = "camelCase")]
    Progress { downloaded: u64, total: Option<u64> },
    /// 내려받기와 서명 확인이 끝나 설치를 시작함. 이후 앱이 다시 시작됩니다.
    Installing,
}

/// 내려받은 양을 세어 크기 상한을 넘는지 확인하고, 진행 이벤트를 일정 간격으로만 내보냅니다.
#[derive(Debug, Default)]
struct DownloadGuard {
    downloaded: u64,
    last_emit: Option<Instant>,
    exceeded: bool,
}

impl DownloadGuard {
    /// 받은 조각 하나를 반영합니다. 상한을 넘으면 `Err`, 진행 이벤트를 보낼 때가 되면 `Some`.
    fn chunk(
        &mut self,
        len: u64,
        total: Option<u64>,
        now: Instant,
    ) -> Result<Option<UpdateEvent>, ()> {
        self.downloaded = self.downloaded.saturating_add(len);
        if self.downloaded > MAX_DOWNLOAD_BYTES || total.is_some_and(|t| t > MAX_DOWNLOAD_BYTES) {
            self.exceeded = true;
            return Err(());
        }
        if self
            .last_emit
            .is_some_and(|t| now.duration_since(t) < PROGRESS_INTERVAL)
        {
            return Ok(None);
        }
        self.last_emit = Some(now);
        Ok(Some(UpdateEvent::Progress {
            downloaded: self.downloaded,
            total,
        }))
    }
}

pub fn plugin_error(e: tauri_plugin_updater::Error) -> Error {
    tracing::warn!(error = %e, "update failed");
    Error::Update(e.to_string())
}

/// 설치 파일을 내려받고 서명을 확인합니다. 크기 상한을 넘으면 내려받기를 중단합니다.
pub async fn download(update: &Update, mut on_event: impl FnMut(UpdateEvent)) -> Result<Vec<u8>> {
    let too_large = Notify::new();
    let mut guard = DownloadGuard::default();
    let result = {
        let on_chunk =
            |len: usize, total: Option<u64>| match guard.chunk(len as u64, total, Instant::now()) {
                Ok(Some(event)) => on_event(event),
                Ok(None) => {}
                Err(()) => too_large.notify_one(),
            };
        tokio::select! {
            r = update.download(on_chunk, || {}) => r.map_err(plugin_error),
            _ = too_large.notified() => Err(Error::Update("download too large".into())),
        }
    };
    if guard.exceeded {
        return Err(Error::Update("download too large".into()));
    }
    let bytes = result?;
    on_event(UpdateEvent::Progress {
        downloaded: bytes.len() as u64,
        total: Some(bytes.len() as u64),
    });
    Ok(bytes)
}

/// 마지막 확인에서 받은 업데이트. 설치는 이 값으로만 진행합니다.
#[derive(Default)]
pub struct PendingUpdate(Mutex<Option<Update>>);

impl PendingUpdate {
    pub fn set(&self, update: Option<Update>) {
        *self.0.lock().unwrap_or_else(|e| e.into_inner()) = update;
    }

    pub fn take(&self) -> Option<Update> {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).take()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notes_drop_control_characters_and_keep_newlines() {
        let info = UpdateInfo::new(
            "0.2.0",
            "0.1.0",
            Some(1),
            Some("  * feat: a\n\t* fix: b\u{1b}[31m\u{7}\r  "),
        );
        assert_eq!(info.notes, "* feat: a\n\t* fix: b[31m");
        assert_eq!(UpdateInfo::new("0.2.0", "0.1.0", None, None).notes, "");
    }

    #[test]
    fn notes_are_truncated() {
        let long = "가".repeat(MAX_NOTES_CHARS + 10);
        let notes = UpdateInfo::new("0.2.0", "0.1.0", None, Some(&long)).notes;
        assert_eq!(notes.chars().count(), MAX_NOTES_CHARS + 1);
        assert!(notes.ends_with('…'));

        let exact = "a".repeat(MAX_NOTES_CHARS);
        assert_eq!(
            UpdateInfo::new("0.2.0", "0.1.0", None, Some(&exact)).notes,
            exact
        );
    }

    #[test]
    fn serializes_camel_case() {
        let json =
            serde_json::to_value(UpdateInfo::new("0.2.0", "0.1.0", Some(5), Some("n"))).unwrap();
        assert_eq!(json["currentVersion"], "0.1.0");
        assert_eq!(json["publishedAt"], 5);
        let event = serde_json::to_value(UpdateEvent::Progress {
            downloaded: 1,
            total: None,
        })
        .unwrap();
        assert_eq!(event["kind"], "progress");
        assert_eq!(event["downloaded"], 1);
        assert_eq!(
            serde_json::to_value(UpdateEvent::Installing).unwrap()["kind"],
            "installing"
        );
    }

    #[test]
    fn guard_throttles_progress() {
        let t0 = Instant::now();
        let mut g = DownloadGuard::default();
        assert!(matches!(g.chunk(10, Some(100), t0), Ok(Some(_))));
        assert_eq!(
            g.chunk(10, Some(100), t0 + Duration::from_millis(50)),
            Ok(None)
        );
        assert_eq!(
            g.chunk(10, Some(100), t0 + PROGRESS_INTERVAL),
            Ok(Some(UpdateEvent::Progress {
                downloaded: 30,
                total: Some(100)
            }))
        );
    }

    #[test]
    fn guard_rejects_oversized_downloads() {
        let now = Instant::now();
        let mut declared = DownloadGuard::default();
        assert_eq!(
            declared.chunk(1, Some(MAX_DOWNLOAD_BYTES + 1), now),
            Err(())
        );
        assert!(declared.exceeded);

        // 크기를 알리지 않는 응답도 받은 양으로 막음
        let mut streamed = DownloadGuard::default();
        assert!(streamed.chunk(MAX_DOWNLOAD_BYTES, None, now).is_ok());
        assert!(!streamed.exceeded);
        assert_eq!(streamed.chunk(1, None, now), Err(()));
        assert!(streamed.exceeded);
    }
}
