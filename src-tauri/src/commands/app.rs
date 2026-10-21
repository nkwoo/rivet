use tauri::AppHandle;

use super::CmdResult;
use crate::core::app_info::AppInfo;
use crate::menu::MenuLocale;

#[tauri::command]
pub fn app_info() -> AppInfo {
    AppInfo::current()
}

/// 웹뷰의 언어 설정에 맞춰 macOS 앱 메뉴를 다시 만듭니다. 다른 OS에서는 메뉴가 없어 아무것도 하지 않습니다.
#[tauri::command]
pub fn app_set_menu_locale(app: AppHandle, locale: MenuLocale) -> CmdResult<()> {
    #[cfg(target_os = "macos")]
    app.set_menu(crate::menu::build(&app, locale)?)?;
    #[cfg(not(target_os = "macos"))]
    let _ = (app, locale);
    Ok(())
}
