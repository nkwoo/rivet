//! macOS 앱 메뉴.
//!
//! Tauri 기본 메뉴의 "Close Window"(⌘W)는 마지막 창을 닫아 앱을 바로 종료합니다.
//! 대신 ⌘W를 "탭 닫기"로 바꾸고 웹뷰에 이벤트로 알려, 열린 대화상자 → 탭 → 창 순서로 닫게 합니다.
//! Windows에서는 Ctrl+W가 셸의 단어 지우기와 겹치므로 메뉴를 두지 않습니다.

use tauri::menu::{Menu, MenuBuilder, MenuItemBuilder, SubmenuBuilder};
use tauri::{AppHandle, Emitter, Runtime};

/// "탭 닫기" 메뉴 항목 ID
const CLOSE_TAB_ID: &str = "close-tab";
/// 웹뷰가 구독하는 이벤트 이름 (`src/App.tsx`)
pub const CLOSE_TAB_EVENT: &str = "menu:close-tab";

/// macOS 기본 메뉴와 같은 구성에서 ⌘W만 "탭 닫기"로 바꾼 메뉴를 만듭니다.
pub fn build<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<Menu<R>> {
    let close_tab = MenuItemBuilder::with_id(CLOSE_TAB_ID, "탭 닫기")
        .accelerator("CmdOrCtrl+W")
        .build(app)?;

    let app_menu = SubmenuBuilder::new(app, "Rivet")
        .about(None)
        .separator()
        .services()
        .separator()
        .hide()
        .hide_others()
        .show_all()
        .separator()
        .quit()
        .build()?;
    let file = SubmenuBuilder::new(app, "파일").item(&close_tab).build()?;
    let edit = SubmenuBuilder::new(app, "편집")
        .undo()
        .redo()
        .separator()
        .cut()
        .copy()
        .paste()
        .select_all()
        .build()?;
    let view = SubmenuBuilder::new(app, "보기").fullscreen().build()?;
    let window = SubmenuBuilder::new(app, "윈도우")
        .minimize()
        .maximize()
        .build()?;

    MenuBuilder::new(app)
        .items(&[&app_menu, &file, &edit, &view, &window])
        .build()
}

/// 메뉴 선택을 웹뷰 이벤트로 전달합니다.
pub fn handle_event<R: Runtime>(app: &AppHandle<R>, id: &str) {
    if id == CLOSE_TAB_ID {
        if let Err(e) = app.emit_to("main", CLOSE_TAB_EVENT, ()) {
            tracing::warn!("탭 닫기 이벤트 전달 실패: {e}");
        }
    }
}
