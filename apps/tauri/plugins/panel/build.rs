//! No commands, so no permissions: the panel page can never call this
//! plugin. The app calls it from Rust (its kiosk commands).

fn main() {
    tauri_plugin::Builder::new(&[])
        .android_path("android")
        .build();
}
