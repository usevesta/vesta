//! Android and iOS/iPadOS: stubs that report every control as unsupported
//! until the native Tauri plugins land (AGENTS.md: native code lives in
//! plugins, not in hand-edited generated projects).
//!
//! - **Android** (Kotlin plugin): `FLAG_KEEP_SCREEN_ON` plus a WakeLock in
//!   the Foreground Service that hosts the Hub; window brightness through
//!   `WindowManager.LayoutParams.screenBrightness` (restore with
//!   `BRIGHTNESS_OVERRIDE_NONE`); kiosk mode with `startLockTask()`, which
//!   only locks for real when the app is Device Owner (`locks_device`).
//! - **iOS/iPadOS** (Swift plugin): `isIdleTimerDisabled` to stay awake,
//!   `UIScreen.brightness` to dim (remember and restore the user's level),
//!   and Guided Access detection (`UIAccessibility.isGuidedAccessEnabled`):
//!   an app cannot lock the device itself, so `kiosk` stays false and
//!   `locks_device` reports whether Guided Access is on.

use tauri::{AppHandle, Runtime};

use super::{KioskHost, Unsupported};

pub(super) fn host<R: Runtime>(_app: &AppHandle<R>) -> Box<dyn KioskHost> {
    #[cfg(target_os = "android")]
    tracing::info!("kiosk controls are not available on Android yet");
    #[cfg(target_os = "ios")]
    tracing::info!("kiosk controls are not available on iOS yet");
    Box::new(Unsupported)
}
