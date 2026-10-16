use crate::core::app_info::AppInfo;

#[tauri::command]
pub fn app_info() -> AppInfo {
    AppInfo::current()
}
