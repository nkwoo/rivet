// 릴리스 빌드에서 Windows 콘솔 창이 뜨지 않도록 함
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    if let Err(e) = rivet_lib::run() {
        eprintln!("Rivet: {e}");
        std::process::exit(1);
    }
}
