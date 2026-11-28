//! The native side of the VESTA app's panel window (AGENTS.md: native code
//! lives in Tauri plugins, not in hand-edited generated projects).
//!
//! **Android** (`android/`, an Android library merged into the app):
//! - A network security config that allows plain HTTP to `127.0.0.1` and
//!   `localhost` only, in every build type. The panel loads from the local
//!   endpoint over loopback (`docs/architecture/local-session.md`); every
//!   other host needs TLS. It replaces the generated project's
//!   `usesCleartextTraffic` switch, which Android ignores once a network
//!   security config is present.
//! - [`Screen`]: keeps the screen on while the panel is in the foreground
//!   (`FLAG_KEEP_SCREEN_ON` on the activity's window).
//!
//! The page never calls this plugin: it declares no commands, so no
//! capability can grant them. The app's kiosk commands call it from Rust.
//! Other platforms get an empty plugin.

use tauri::Runtime;
use tauri::plugin::{Builder, TauriPlugin};

/// The plugin name (`tauri-plugin-<name>`).
const NAME: &str = "vesta-panel";

/// Registers the plugin; on Android it manages a [`Screen`].
pub fn init<R: Runtime>() -> TauriPlugin<R> {
    Builder::new(NAME)
        .setup(|_app, _api| {
            #[cfg(target_os = "android")]
            {
                use tauri::Manager;
                let handle = _api.register_android_plugin(android::PACKAGE, android::CLASS)?;
                _app.manage(Screen(handle));
            }
            Ok(())
        })
        .build()
}

#[cfg(target_os = "android")]
pub use android::Screen;

#[cfg(target_os = "android")]
mod android {
    use serde::Serialize;
    use tauri::Runtime;
    use tauri::plugin::PluginHandle;
    use tauri::plugin::mobile::PluginInvokeError;

    /// `android/src/main/java/PanelPlugin.kt`.
    pub(super) const PACKAGE: &str = "com.usevesta.panel";
    pub(super) const CLASS: &str = "PanelPlugin";

    /// The panel's screen on Android, managed as app state by [`super::init`].
    pub struct Screen<R: Runtime>(pub(super) PluginHandle<R>);

    #[derive(Serialize)]
    struct KeepScreenOn {
        on: bool,
    }

    impl<R: Runtime> Screen<R> {
        /// Keeps the screen on while the panel is in the foreground, or
        /// lets it time out again. Android drops the flag's effect by itself
        /// when the app goes to the background.
        pub fn set_keep_on(&self, on: bool) -> Result<(), PluginInvokeError> {
            self.0
                .run_mobile_plugin("setKeepScreenOn", KeepScreenOn { on })
        }
    }
}
