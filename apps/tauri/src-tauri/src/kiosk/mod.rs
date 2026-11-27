//! Wall-panel screen controls behind one platform-neutral trait (AGENTS.md
//! rule 4): keep the screen on, dim it, lock the device into the app.
//!
//! Each platform has its own [`KioskHost`]; the web UI reaches it through
//! the commands below (`web/src/platform/`), which are the only IPC the
//! panel page is granted. Payload and result types are exported to
//! TypeScript (`web/src/api/generated/shell/`, `cargo test -p vesta-app`).

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Runtime, State};

#[cfg(desktop)]
mod desktop;
#[cfg(mobile)]
mod mobile;

/// What this device's panel can do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export_to = "shell/"))]
pub struct KioskCapabilities {
    /// The screen brightness can be set, so the screensaver dims the
    /// backlight instead of only covering the screen.
    pub brightness: bool,
    /// The screen (and the device) can be kept awake while the panel runs.
    pub keep_awake: bool,
    /// Kiosk mode can be entered (full screen on desktop).
    pub kiosk: bool,
    /// Kiosk mode really locks the device into the app (Android LockTask
    /// as Device Owner, iOS Guided Access), not just full screen.
    pub locks_device: bool,
}

/// Why a kiosk command did nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, thiserror::Error)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(test, derive(ts_rs::TS), ts(export_to = "shell/"))]
pub enum KioskError {
    #[error("not supported on this device")]
    Unsupported,
    #[error("the system refused the request")]
    Failed,
}

/// `kiosk_set_keep_awake`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export_to = "shell/"))]
pub struct KeepAwakeRequest {
    pub on: bool,
}

/// `kiosk_set_brightness`: 0 (darkest the system allows) to 100.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export_to = "shell/"))]
pub struct BrightnessRequest {
    pub percent: u8,
}

/// The platform side of the wall panel.
pub trait KioskHost: Send + Sync + 'static {
    fn capabilities(&self) -> KioskCapabilities;
    fn set_keep_awake(&self, on: bool) -> Result<(), KioskError>;
    /// `percent` is already clamped to `0..=100`.
    fn set_brightness(&self, percent: u8) -> Result<(), KioskError>;
    /// Hands brightness back to the system (and the user's setting).
    fn restore_brightness(&self) -> Result<(), KioskError>;
    fn enter_kiosk(&self) -> Result<(), KioskError>;
    fn exit_kiosk(&self) -> Result<(), KioskError>;
}

/// A platform without any of these controls.
#[cfg(any(mobile, test))]
#[derive(Debug, Clone, Copy, Default)]
pub struct Unsupported;

#[cfg(any(mobile, test))]
impl KioskHost for Unsupported {
    fn capabilities(&self) -> KioskCapabilities {
        KioskCapabilities::default()
    }
    fn set_keep_awake(&self, _on: bool) -> Result<(), KioskError> {
        Err(KioskError::Unsupported)
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

/// The host for this platform, as Tauri state for the commands.
pub struct Kiosk(Box<dyn KioskHost>);

impl Kiosk {
    pub fn for_platform<R: Runtime>(app: &AppHandle<R>) -> Self {
        #[cfg(desktop)]
        let host = desktop::host(app);
        #[cfg(mobile)]
        let host = mobile::host(app);
        Self(host)
    }
}

/// Every kiosk command; each gets an `allow-…` permission from `build.rs`.
pub const COMMANDS: [&str; 6] = [
    "kiosk_capabilities",
    "kiosk_set_keep_awake",
    "kiosk_set_brightness",
    "kiosk_restore_brightness",
    "kiosk_enter",
    "kiosk_exit",
];

/// The permissions the panel page gets: these commands and nothing else.
pub fn permissions() -> impl Iterator<Item = String> {
    COMMANDS
        .iter()
        .map(|command| format!("allow-{}", command.replace('_', "-")))
}

#[tauri::command]
pub fn kiosk_capabilities(kiosk: State<'_, Kiosk>) -> KioskCapabilities {
    kiosk.0.capabilities()
}

#[tauri::command]
pub fn kiosk_set_keep_awake(
    kiosk: State<'_, Kiosk>,
    request: KeepAwakeRequest,
) -> Result<(), KioskError> {
    kiosk.0.set_keep_awake(request.on)
}

#[tauri::command]
pub fn kiosk_set_brightness(
    kiosk: State<'_, Kiosk>,
    request: BrightnessRequest,
) -> Result<(), KioskError> {
    kiosk.0.set_brightness(request.percent.min(100))
}

#[tauri::command]
pub fn kiosk_restore_brightness(kiosk: State<'_, Kiosk>) -> Result<(), KioskError> {
    kiosk.0.restore_brightness()
}

#[tauri::command]
pub fn kiosk_enter(kiosk: State<'_, Kiosk>) -> Result<(), KioskError> {
    kiosk.0.enter_kiosk()
}

#[tauri::command]
pub fn kiosk_exit(kiosk: State<'_, Kiosk>) -> Result<(), KioskError> {
    kiosk.0.exit_kiosk()
}

#[cfg(test)]
mod tests {
    use std::error::Error;
    use std::fs;

    use ts_rs::{Config, TS};

    use super::*;

    const TS_RS_HEADER: &str = "// This file was generated by [ts-rs]";

    #[test]
    fn unsupported_reports_nothing() {
        let host = Unsupported;
        assert_eq!(host.capabilities(), KioskCapabilities::default());
        assert_eq!(host.set_keep_awake(true), Err(KioskError::Unsupported));
        assert_eq!(host.set_brightness(10), Err(KioskError::Unsupported));
        assert_eq!(host.restore_brightness(), Err(KioskError::Unsupported));
        assert_eq!(host.enter_kiosk(), Err(KioskError::Unsupported));
        assert_eq!(host.exit_kiosk(), Err(KioskError::Unsupported));
    }

    #[test]
    fn wire_format() {
        let capabilities = KioskCapabilities {
            brightness: false,
            keep_awake: true,
            kiosk: true,
            locks_device: false,
        };
        assert_eq!(
            serde_json::to_string(&capabilities).unwrap(),
            r#"{"brightness":false,"keep_awake":true,"kiosk":true,"locks_device":false}"#
        );
        assert_eq!(
            serde_json::to_string(&KioskError::Unsupported).unwrap(),
            r#""unsupported""#
        );
        assert_eq!(
            serde_json::to_string(&KioskError::Failed).unwrap(),
            r#""failed""#
        );
        let request: BrightnessRequest = serde_json::from_str(r#"{"percent":15}"#).unwrap();
        assert_eq!(request, BrightnessRequest { percent: 15 });
    }

    #[test]
    fn build_script_declares_every_command() {
        let build = include_str!("../../build.rs");
        for command in COMMANDS {
            assert!(build.contains(&format!("\"{command}\"")), "{command}");
        }
        assert_eq!(
            permissions().collect::<Vec<_>>(),
            [
                "allow-kiosk-capabilities",
                "allow-kiosk-set-keep-awake",
                "allow-kiosk-set-brightness",
                "allow-kiosk-restore-brightness",
                "allow-kiosk-enter",
                "allow-kiosk-exit",
            ]
        );
    }

    /// Writes the bindings to `TS_RS_EXPORT_DIR/shell/` (set in
    /// `.cargo/config.toml`) with an `index.ts` barrel. `vesta-core`'s
    /// bindings test leaves this folder alone.
    #[test]
    fn export_typescript_bindings() -> Result<(), Box<dyn Error>> {
        let cfg = Config::from_env();
        let dir = cfg.out_dir().join("shell");
        if dir.exists() {
            for entry in fs::read_dir(&dir)? {
                let path = entry?.path();
                if fs::read_to_string(&path)?.starts_with(TS_RS_HEADER) {
                    fs::remove_file(&path)?;
                }
            }
        }
        KioskCapabilities::export_all(&cfg)?;
        KioskError::export_all(&cfg)?;
        KeepAwakeRequest::export_all(&cfg)?;
        BrightnessRequest::export_all(&cfg)?;

        let mut modules = Vec::new();
        for entry in fs::read_dir(&dir)? {
            let path = entry?.path();
            let source = fs::read_to_string(&path)?;
            if source.starts_with(TS_RS_HEADER) {
                assert!(!source.contains(": any"), "`any` in {}", path.display());
                if let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) {
                    modules.push(stem.to_owned());
                }
            }
        }
        modules.sort();
        assert_eq!(
            modules,
            [
                "BrightnessRequest",
                "KeepAwakeRequest",
                "KioskCapabilities",
                "KioskError"
            ]
        );
        let mut index = String::from(
            "// Generated by `cargo test -p vesta-app` (apps/tauri/src-tauri/src/kiosk). Do not edit.\n\
             // Kiosk command payloads are defined once in Rust and exported with ts-rs.\n\n",
        );
        for module in &modules {
            index.push_str(&format!("export type * from \"./{module}\";\n"));
        }
        fs::write(dir.join("index.ts"), index)?;
        Ok(())
    }
}
