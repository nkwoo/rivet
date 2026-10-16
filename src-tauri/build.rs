fn main() {
    // 앱 명령은 capabilities/에서 명시적으로 허용한 것만 웹뷰에서 호출 가능
    tauri_build::try_build(
        tauri_build::Attributes::new()
            .app_manifest(tauri_build::AppManifest::new().commands(&["app_info"])),
    )
    .unwrap_or_else(|e| panic!("tauri build script failed: {e:#}"));
}
