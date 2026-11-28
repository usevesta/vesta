//! Android, through the app's panel plugin (`apps/tauri/plugins/panel`,
//! Kotlin): the screen stays on while the panel is in the foreground
//! (`FLAG_KEEP_SCREEN_ON` on the activity's window; Android lets it time
//! out again as soon as the app is in the background).
//!
//! Still to come in that plugin:
//! - Brightness: `WindowManager.LayoutParams.screenBrightness` on the
//!   window, restored with `BRIGHTNESS_OVERRIDE_NONE`.
//! - Kiosk mode: `startLockTask()`, which only locks for real when the app
//!   is Device Owner (`locks_device`).
//! - A Foreground Service with a WakeLock that keeps the Hub running while
//!   the panel is not in the foreground.

use tauri::{AppHandle, Manager, Runtime};
use tauri_plugin_vesta_panel::Screen;

use super::{KioskCapabilities, KioskError, KioskHost};

pub(super) fn host<R: Runtime>(app: &AppHandle<R>) -> Box<dyn KioskHost> {
    Box::new(AndroidKiosk { app: app.clone() })
}

struct AndroidKiosk<R: Runtime> {
    app: AppHandle<R>,
}

impl<R: Runtime> KioskHost for AndroidKiosk<R> {
    fn capabilities(&self) -> KioskCapabilities {
        KioskCapabilities {
            keep_awake: true,
            ..KioskCapabilities::default()
        }
    }

    fn set_keep_awake(&self, on: bool) -> Result<(), KioskError> {
        let screen = self.app.try_state::<Screen<R>>().ok_or_else(|| {
            tracing::warn!("the panel plugin is not registered");
            KioskError::Failed
        })?;
        screen.set_keep_on(on).map_err(|error| {
            tracing::warn!(%error, on, "could not keep the screen on");
            KioskError::Failed
        })
    }

    fn set_brightness(&self, _percent: u8) -> Result<(), KioskError> {
        Err(KioskError::Unsupported)
    }

    fn restore_brightness(&self) -> Result<(), KioskError> {
        Err(KioskError::Unsupported)
    }

    fn enter_kiosk(&self) -> Result<(), KioskError> {
        Err(KioskError::Unsupported)
    }

    fn exit_kiosk(&self) -> Result<(), KioskError> {
        Err(KioskError::Unsupported)
    }
}
