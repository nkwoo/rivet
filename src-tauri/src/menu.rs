//! macOS 앱 메뉴.
//!
//! Tauri 기본 메뉴의 "Close Window"(⌘W)는 마지막 창을 닫아 앱을 바로 종료합니다.
//! 대신 ⌘W를 "탭 닫기"로 바꾸고 웹뷰에 이벤트로 알려, 열린 대화상자 → 탭 → 창 순서로 닫게 합니다.
//! Windows에서는 Ctrl+W가 셸의 단어 지우기와 겹치므로 메뉴를 두지 않습니다.
//!
//! 메뉴 이름은 웹뷰의 언어 설정을 따릅니다. 시작할 때는 한국어로 만들고, 웹뷰가 언어를 정하면
//! `app_set_menu_locale` 명령으로 다시 만듭니다.

use serde::Deserialize;
use tauri::menu::{Menu, MenuBuilder, MenuItemBuilder, SubmenuBuilder};
use tauri::{AppHandle, Emitter, Runtime};

/// 메뉴 언어. 웹뷰의 `Locale`(`src/i18n`)과 같은 값
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MenuLocale {
    #[default]
    Ko,
    En,
}

struct Labels {
    check_update: &'static str,
    close_tab: &'static str,
    file: &'static str,
    edit: &'static str,
    view: &'static str,
    window: &'static str,
}

impl MenuLocale {
    fn labels(self) -> Labels {
        match self {
            MenuLocale::Ko => Labels {
                check_update: "업데이트 확인…",
                close_tab: "탭 닫기",
                file: "파일",
                edit: "편집",
                view: "보기",
                window: "윈도우",
            },
            MenuLocale::En => Labels {
                check_update: "Check for Updates…",
                close_tab: "Close Tab",
                file: "File",
                edit: "Edit",
                view: "View",
                window: "Window",
            },
        }
    }
}

/// "탭 닫기" 메뉴 항목 ID
const CLOSE_TAB_ID: &str = "close-tab";
/// 웹뷰가 구독하는 이벤트 이름 (`src/App.tsx`)
pub const CLOSE_TAB_EVENT: &str = "menu:close-tab";
const CHECK_UPDATE_ID: &str = "check-update";
/// 웹뷰가 구독하는 이벤트 이름 (`src/update/UpdateDialog.tsx`)
pub const CHECK_UPDATE_EVENT: &str = "menu:check-update";

/// macOS 기본 메뉴와 같은 구성에서 ⌘W만 "탭 닫기"로 바꾼 메뉴를 만듭니다.
pub fn build<R: Runtime>(app: &AppHandle<R>, locale: MenuLocale) -> tauri::Result<Menu<R>> {
    let labels = locale.labels();
    let close_tab = MenuItemBuilder::with_id(CLOSE_TAB_ID, labels.close_tab)
        .accelerator("CmdOrCtrl+W")
        .build(app)?;
    let check_update = MenuItemBuilder::with_id(CHECK_UPDATE_ID, labels.check_update).build(app)?;

    let app_menu = SubmenuBuilder::new(app, "Rivet")
        .about(None)
        .item(&check_update)
        .separator()
        .services()
        .separator()
        .hide()
        .hide_others()
        .show_all()
        .separator()
        .quit()
        .build()?;
    let file = SubmenuBuilder::new(app, labels.file)
        .item(&close_tab)
        .build()?;
    let edit = SubmenuBuilder::new(app, labels.edit)
        .undo()
        .redo()
        .separator()
        .cut()
        .copy()
        .paste()
        .select_all()
        .build()?;
    let view = SubmenuBuilder::new(app, labels.view).fullscreen().build()?;
    let window = SubmenuBuilder::new(app, labels.window)
        .minimize()
        .maximize()
        .build()?;

    MenuBuilder::new(app)
        .items(&[&app_menu, &file, &edit, &view, &window])
        .build()
}

/// 메뉴 선택을 웹뷰 이벤트로 전달합니다.
pub fn handle_event<R: Runtime>(app: &AppHandle<R>, id: &str) {
    let event = match id {
        CLOSE_TAB_ID => CLOSE_TAB_EVENT,
        CHECK_UPDATE_ID => CHECK_UPDATE_EVENT,
        _ => return,
    };
    if let Err(e) = app.emit_to("main", event, ()) {
        tracing::warn!("메뉴 이벤트 전달 실패 ({event}): {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locale_parses_webview_values_and_rejects_others() {
        let ko: MenuLocale = serde_json::from_str("\"ko\"").unwrap();
        let en: MenuLocale = serde_json::from_str("\"en\"").unwrap();
        assert_eq!(ko, MenuLocale::Ko);
        assert_eq!(en, MenuLocale::En);
        assert!(serde_json::from_str::<MenuLocale>("\"fr\"").is_err());
        assert!(serde_json::from_str::<MenuLocale>("\"EN\"").is_err());
    }

    #[test]
    fn english_labels_have_no_korean() {
        let l = MenuLocale::En.labels();
        for label in [l.close_tab, l.file, l.edit, l.view, l.window] {
            assert!(label.is_ascii(), "{label}");
        }
        // 말줄임표(…)만 ASCII가 아님
        assert!(l.check_update.trim_end_matches('…').is_ascii());
    }
}
