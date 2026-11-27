//! The panel window: where it loads from, where it may go, and which IPC
//! its page gets.
//!
//! - Navigation is locked to the origin the window starts on; links to
//!   anything else and new windows are refused.
//! - The page may call the kiosk commands and nothing else. When the page
//!   is served by the in-process Hub (`http://127.0.0.1:<port>`, a remote
//!   origin for Tauri), that grant is added at runtime for **exactly** that
//!   origin, port included, never for a pattern such as `127.0.0.1:*`.

use tauri::ipc::CapabilityBuilder;
#[cfg(desktop)]
use tauri::webview::NewWindowResponse;
use tauri::{AppHandle, Manager, Runtime, Url, WebviewUrl, WebviewWindowBuilder};

use crate::kiosk;

/// Label of the one panel window.
pub const MAIN_WINDOW: &str = "main";

/// Where the panel page comes from.
#[derive(Debug, Clone)]
pub enum Panel {
    /// The UI bundled with the app (the dev server in development). Tauri
    /// treats it as a local origin. Clients, until `vesta-link` lands.
    #[cfg_attr(feature = "hub", allow(dead_code))]
    Bundled,
    /// A page on the app's own dev server (development with the Hub
    /// in-process): a local origin too.
    #[cfg_attr(not(feature = "hub"), allow(dead_code))]
    DevServer(Url),
    /// A page served by a local endpoint the shell started (the
    /// in-process Hub): a remote origin, granted IPC by exact origin.
    #[cfg_attr(not(feature = "hub"), allow(dead_code))]
    Local(Url),
}

pub fn open<R: Runtime>(app: &AppHandle<R>, panel: Panel) -> anyhow::Result<()> {
    let (url, remote) = match panel {
        Panel::Bundled => (WebviewUrl::App("index.html".into()), false),
        Panel::DevServer(url) => (WebviewUrl::External(url), false),
        Panel::Local(url) => (WebviewUrl::External(url), true),
    };
    let origin = match &url {
        WebviewUrl::External(url) => url.origin(),
        // The bundled UI: `tauri://localhost` (`http://tauri.localhost` on
        // Windows and Android), or the dev server in development.
        _ => app_url(app)?.origin(),
    };

    let mut capability = CapabilityBuilder::new("panel-kiosk")
        .window(MAIN_WINDOW)
        .local(!remote);
    if remote {
        capability = capability.remote(origin.ascii_serialization());
    }
    for permission in kiosk::permissions() {
        capability = capability.permission(permission);
    }
    app.add_capability(capability)?;

    let allowed = origin.clone();
    let builder = WebviewWindowBuilder::new(app, MAIN_WINDOW, url).on_navigation(move |target| {
        let allowed_here = target.origin() == allowed;
        if !allowed_here {
            tracing::warn!(target = %target.origin().ascii_serialization(), "blocked navigation away from the panel");
        }
        allowed_here
    });
    #[cfg(desktop)]
    let builder = builder
        .title("VESTA")
        .inner_size(1280.0, 800.0)
        .min_inner_size(480.0, 600.0)
        .on_new_window(|_, _| NewWindowResponse::Deny);
    builder.build()?;
    tracing::info!(origin = %origin.ascii_serialization(), "panel window open");
    Ok(())
}

/// The bundled UI's origin.
fn app_url<R: Runtime>(app: &AppHandle<R>) -> anyhow::Result<Url> {
    if tauri::is_dev()
        && let Some(dev_url) = &app.config().build.dev_url
    {
        return Ok(dev_url.clone());
    }
    let url = if cfg!(any(windows, target_os = "android")) {
        "http://tauri.localhost"
    } else {
        "tauri://localhost"
    };
    Ok(Url::parse(url)?)
}
