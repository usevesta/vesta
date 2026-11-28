//! iOS/iPadOS: a stub that reports every control as unsupported until the
//! Swift side of the panel plugin (`apps/tauri/plugins/panel`) lands:
//! `isIdleTimerDisabled` to stay awake, `UIScreen.brightness` to dim
//! (remember and restore the user's level), and Guided Access detection
//! (`UIAccessibility.isGuidedAccessEnabled`): an app cannot lock the device
//! itself, so `kiosk` stays false and `locks_device` reports whether Guided
//! Access is on.

use tauri::{AppHandle, Runtime};

use super::{KioskHost, Unsupported};

pub(super) fn host<R: Runtime>(_app: &AppHandle<R>) -> Box<dyn KioskHost> {
    tracing::info!("kiosk controls are not available on iOS yet");
    Box::new(Unsupported)
}
