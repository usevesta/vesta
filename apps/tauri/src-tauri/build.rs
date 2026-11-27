//! Declares the app's own commands so Tauri generates an `allow-<command>`
//! permission for each and checks every call against the capabilities the
//! shell grants (see `src/panel.rs`). Keep in sync with `kiosk::COMMANDS`
//! (a test checks it).

fn main() {
    let manifest = tauri_build::AppManifest::new().commands(&[
        "kiosk_capabilities",
        "kiosk_set_keep_awake",
        "kiosk_set_brightness",
        "kiosk_restore_brightness",
        "kiosk_enter",
        "kiosk_exit",
    ]);
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(manifest))
        .unwrap_or_else(|error| panic!("tauri-build failed: {error:#}"));
}
