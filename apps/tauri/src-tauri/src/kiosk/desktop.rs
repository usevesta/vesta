//! macOS, Windows and Linux: kiosk mode is full screen; keep-awake uses
//! the OS power APIs through `keepawake` (IOKit assertions,
//! `SetThreadExecutionState`, D-Bus inhibitors). Desktop displays have no
//! portable brightness control, so the screensaver only covers the screen.

use std::sync::{Mutex, MutexGuard, PoisonError};

use tauri::{AppHandle, Manager, Runtime};

use super::{KioskCapabilities, KioskError, KioskHost};
use crate::panel::MAIN_WINDOW;

pub(super) fn host<R: Runtime>(app: &AppHandle<R>) -> Box<dyn KioskHost> {
    Box::new(DesktopKiosk {
        app: app.clone(),
        awake: Mutex::new(None),
    })
}

struct DesktopKiosk<R: Runtime> {
    app: AppHandle<R>,
    /// Held while the panel asks to stay awake; dropping it releases it.
    awake: Mutex<Option<keepawake::KeepAwake>>,
}

impl<R: Runtime> DesktopKiosk<R> {
    fn awake(&self) -> MutexGuard<'_, Option<keepawake::KeepAwake>> {
        self.awake.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn set_fullscreen(&self, on: bool) -> Result<(), KioskError> {
        let window = self
            .app
            .get_webview_window(MAIN_WINDOW)
            .ok_or(KioskError::Failed)?;
        window.set_fullscreen(on).map_err(|error| {
            tracing::warn!(%error, on, "could not change full screen");
            KioskError::Failed
        })
    }
}

impl<R: Runtime> KioskHost for DesktopKiosk<R> {
    fn capabilities(&self) -> KioskCapabilities {
        KioskCapabilities {
            brightness: false,
            keep_awake: true,
            kiosk: true,
            locks_device: false,
        }
    }

    fn set_keep_awake(&self, on: bool) -> Result<(), KioskError> {
        let mut awake = self.awake();
        if !on {
            *awake = None;
            return Ok(());
        }
        if awake.is_none() {
            // The display stays on and the machine does not idle-sleep,
            // since the home runs on it too.
            let assertion = keepawake::Builder::default()
                .display(true)
                .idle(true)
                .reason("Wall panel")
                .app_name("VESTA")
                .app_reverse_domain("com.usevesta.app")
                .create()
                .map_err(|error| {
                    tracing::warn!(%error, "could not keep the screen awake");
                    KioskError::Failed
                })?;
            *awake = Some(assertion);
        }
        Ok(())
    }

    fn set_brightness(&self, _percent: u8) -> Result<(), KioskError> {
        Err(KioskError::Unsupported)
    }

    fn restore_brightness(&self) -> Result<(), KioskError> {
        Err(KioskError::Unsupported)
    }

    fn enter_kiosk(&self) -> Result<(), KioskError> {
        self.set_fullscreen(true)
    }

    fn exit_kiosk(&self) -> Result<(), KioskError> {
        self.set_fullscreen(false)
    }
}
