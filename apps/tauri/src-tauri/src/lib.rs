//! The VESTA app (`com.usevesta.app`).
//!
//! - With `--features hub` (wall tablet, desktop panel) it runs the Hub
//!   in-process and shows the wall panel served by it (see [`hub`]).
//! - Without it, it is a client. Pairing and the `vesta-link` proxy come
//!   later; for now it shows the bundled UI.
//!
//! Platform specifics stay behind `cfg` gates and the [`kiosk::KioskHost`]
//! trait (AGENTS.md rule 4).

// The mobile entry point that Tauri generates exports `#[unsafe(no_mangle)]`
// symbols for the Android/iOS launchers.
#![cfg_attr(mobile, allow(unsafe_code))]

#[cfg(all(feature = "hub", target_os = "ios"))]
compile_error!(
    "the `hub` feature is not available on iOS/iPadOS: iPhones and iPads are always clients \
     (AGENTS.md rule 4)"
);

#[cfg(feature = "hub")]
mod hub;
mod kiosk;
mod panel;

use tauri::{App, Manager, RunEvent};
use tracing_subscriber::EnvFilter;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_writer(std::io::stderr)
        .init();
    if let Err(error) = try_run() {
        tracing::error!("{error:#}");
        std::process::exit(1);
    }
}

fn try_run() -> anyhow::Result<()> {
    let app = tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            kiosk::kiosk_capabilities,
            kiosk::kiosk_set_keep_awake,
            kiosk::kiosk_set_brightness,
            kiosk::kiosk_restore_brightness,
            kiosk::kiosk_enter,
            kiosk::kiosk_exit,
        ])
        .setup(|app| setup(app).map_err(Into::into))
        .build(tauri::generate_context!())?;
    app.run(|_app, event| {
        if let RunEvent::Exit = event {
            #[cfg(feature = "hub")]
            if let Some(hub) = _app.try_state::<hub::InProcessHub>() {
                hub.stop();
            }
        }
    });
    Ok(())
}

fn setup(app: &mut App) -> anyhow::Result<()> {
    let handle = app.handle().clone();
    app.manage(kiosk::Kiosk::for_platform(&handle));

    #[cfg(feature = "hub")]
    let panel = {
        let (hub, panel) = hub::start(&handle)?;
        app.manage(hub);
        panel
    };
    #[cfg(not(feature = "hub"))]
    let panel = panel::Panel::Bundled;

    panel::open(&handle, panel)
}
