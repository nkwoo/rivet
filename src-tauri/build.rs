/// 웹뷰에서 호출 가능한 앱 명령. capabilities/default.json에서 개별로 허용해야 실제로 호출됩니다.
const COMMANDS: &[&str] = &[
    "app_info",
    "app_set_menu_locale",
    "profile_list",
    "profile_create",
    "profile_update",
    "profile_delete",
    "profile_forget_secret",
    "ssh_config_scan",
    "known_hosts_list",
    "known_hosts_remove",
    "cert_pins_list",
    "cert_pins_remove",
    "ftp_connect",
    "rdp_connect",
    "rdp_input",
    "rdp_resize",
    "rdp_frame_ack",
    "rdp_clipboard",
    "ssh_connect",
    "ssh_write",
    "ssh_resize",
    "ssh_close",
    "ssh_host_key_respond",
    "local_home",
    "local_list",
    "sftp_open",
    "sftp_list",
    "sftp_mkdir",
    "sftp_rename",
    "sftp_remove",
    "transfer_subscribe",
    "transfer_upload",
    "transfer_download",
    "transfer_resolve",
    "transfer_cancel",
    "transfer_retry",
    "transfer_clear_finished",
    "update_check",
    "update_install",
];

fn main() {
    tauri_build::try_build(
        tauri_build::Attributes::new()
            .app_manifest(tauri_build::AppManifest::new().commands(COMMANDS)),
    )
    .unwrap_or_else(|e| panic!("tauri build script failed: {e:#}"));
}
