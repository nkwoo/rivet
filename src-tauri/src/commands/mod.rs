//! `#[tauri::command]` 진입점. 인자 변환만 하고 실제 로직은 `core`/`protocols`에 위임합니다.
//!
//! 새 명령을 추가하면 `build.rs`의 `AppManifest::commands`와
//! `capabilities/default.json`에도 함께 등록해야 웹뷰에서 호출할 수 있습니다.

pub mod app;
