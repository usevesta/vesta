//! The Hub, in-process (`--features hub`): the wall tablet, or a desktop
//! acting as the panel.
//!
//! - Loads `config.json` + `dashboard.json` from the app data directory;
//!   without them it runs the built-in simulated home (onboarding comes
//!   later).
//! - Release: listens on `127.0.0.1` (the port of the last run when it is
//!   free, so the page origin and its per-screen settings survive a
//!   restart; any free port otherwise) and serves the bundled UI and the
//!   API on that one origin.
//! - Development (`pnpm tauri dev`): listens where the Vite dev server
//!   proxies `/api` (`VESTA_HUB_ORIGIN`, default `http://127.0.0.1:8181`)
//!   and allows the dev server's exact origin.
//! - Always with sessions: the panel window first opens a one-time login
//!   URL, so no secret ever reaches JavaScript.

use std::collections::HashSet;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use anyhow::{Context, bail};
use tauri::async_runtime::{self, JoinHandle};
use tauri::{AppHandle, AssetResolver, Manager, Runtime, Url};
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use vesta_core::{Dashboard, HubConfig};
use vesta_drivers::Registry;
use vesta_hub::{ApiAuth, Hub, HubOptions, JsonFileStore, UiAsset, UiAssets, bind_loopback};

use crate::panel::Panel;

/// Where the dev Hub listens unless `VESTA_HUB_ORIGIN` says otherwise.
/// Must match the default in `web/vite.config.ts`.
const DEV_HUB_ORIGIN: &str = "http://127.0.0.1:8181";
/// Remembers the release port between runs (see the module docs).
const PORT_FILE: &str = "panel-port";
const STOP_TIMEOUT: Duration = Duration::from_secs(5);

/// The running Hub, kept as app state until exit.
pub struct InProcessHub(Mutex<Option<Running>>);

struct Running {
    stop: oneshot::Sender<()>,
    task: JoinHandle<anyhow::Result<()>>,
}

/// Starts the Hub and returns it with the page the panel opens first.
pub fn start<R: Runtime>(app: &AppHandle<R>) -> anyhow::Result<(InProcessHub, Panel)> {
    let data_dir = app.path().app_data_dir().context("no app data directory")?;
    let (config, dashboard, dashboard_file) = load_home(&data_dir)?;
    let dev_url = tauri::is_dev()
        .then(|| app.config().build.dev_url.clone())
        .flatten();

    let mut options = HubOptions {
        auth: ApiAuth::Sessions,
        ..HubOptions::default()
    };
    match &dev_url {
        Some(dev_url) => options.allowed_origins = vec![dev_url.origin().ascii_serialization()],
        None => options.ui = Some(Arc::new(EmbeddedUi::new(app.asset_resolver()))),
    }

    let (hub, listener) = async_runtime::block_on(async {
        let mut hub = Hub::start(&config, dashboard, &Registry::builtin(), options).await?;
        // Edits to a real home are saved next to its config; the simulated
        // home keeps them in memory, like `vesta-hubd --dev`.
        if let Some(path) = dashboard_file {
            hub = hub.with_dashboard_store(JsonFileStore::new(path));
        }
        let listener = match &dev_url {
            Some(_) => bind_loopback(dev_hub_addr()?).await.context(
                "is another app or `vesta-hubd --port 8181` running? \
                 Stop it or set VESTA_HUB_ORIGIN for both the app and the dev server",
            )?,
            None => bind_remembered(&data_dir).await?,
        };
        anyhow::Ok((hub, listener))
    })?;
    let local = listener.local_addr()?;

    let code = hub
        .login_codes()
        .context("the in-process hub must use sessions")?
        .issue()?;
    let panel = match dev_url {
        Some(dev_url) => Panel::DevServer(dev_url.join(&code.login_path())?),
        None => Panel::Local(Url::parse(&format!("http://{local}{}", code.login_path()))?),
    };

    let (stop, stopped) = oneshot::channel::<()>();
    let task = async_runtime::spawn(hub.serve(listener, async move {
        // A dropped sender also stops the Hub.
        let _ = stopped.await;
    }));
    Ok((
        InProcessHub(Mutex::new(Some(Running { stop, task }))),
        panel,
    ))
}

impl InProcessHub {
    /// Stops the Hub gracefully (open requests finish, devices stop).
    pub fn stop(&self) {
        let running = self.0.lock().unwrap_or_else(PoisonError::into_inner).take();
        let Some(Running { stop, task }) = running else {
            return;
        };
        let _ = stop.send(());
        // The timer must be created inside the runtime.
        let stopped =
            async_runtime::block_on(async move { tokio::time::timeout(STOP_TIMEOUT, task).await });
        match stopped {
            Ok(Ok(Ok(()))) => {}
            Ok(Ok(Err(error))) => tracing::error!("hub stopped with an error: {error:#}"),
            Ok(Err(error)) => tracing::error!(%error, "hub task failed"),
            Err(_) => tracing::warn!("hub did not stop within {STOP_TIMEOUT:?}"),
        }
    }
}

/// The home to run and, for a real home, the file dashboard edits are saved to.
fn load_home(data_dir: &Path) -> anyhow::Result<(HubConfig, Dashboard, Option<PathBuf>)> {
    let config_path = data_dir.join("config.json");
    let dashboard_path = data_dir.join("dashboard.json");
    let (config, dashboard, saved_to) = match (config_path.exists(), dashboard_path.exists()) {
        (false, false) => {
            tracing::info!(
                dir = %data_dir.display(),
                "no home set up yet: running the built-in simulated home"
            );
            (
                vesta_drivers::demo::CONFIG.to_owned(),
                vesta_drivers::demo::DASHBOARD.to_owned(),
                None,
            )
        }
        (true, true) => {
            tracing::info!(dir = %data_dir.display(), "loading the home");
            let read = |path: &Path| {
                std::fs::read_to_string(path)
                    .with_context(|| format!("could not read {}", path.display()))
            };
            (
                read(&config_path)?,
                read(&dashboard_path)?,
                Some(dashboard_path.clone()),
            )
        }
        _ => bail!(
            "{} needs both config.json and dashboard.json (or neither, for the simulated home)",
            data_dir.display()
        ),
    };
    let config = HubConfig::from_json(&config).context("invalid config.json")?;
    let dashboard = Dashboard::from_json(&dashboard).context("invalid dashboard.json")?;
    Ok((config, dashboard, saved_to))
}

/// `VESTA_HUB_ORIGIN` (shared with the Vite dev server) as a socket address.
fn dev_hub_addr() -> anyhow::Result<SocketAddr> {
    let origin = std::env::var("VESTA_HUB_ORIGIN").unwrap_or_else(|_| DEV_HUB_ORIGIN.to_owned());
    parse_hub_origin(&origin).with_context(|| format!("invalid VESTA_HUB_ORIGIN `{origin}`"))
}

fn parse_hub_origin(origin: &str) -> anyhow::Result<SocketAddr> {
    let url = Url::parse(origin)?;
    let host = url.host_str().context("no host")?;
    let ip = if host.eq_ignore_ascii_case("localhost") {
        IpAddr::V4(Ipv4Addr::LOCALHOST)
    } else {
        host.trim_start_matches('[')
            .trim_end_matches(']')
            .parse()
            .context("expected http://127.0.0.1:<port>")?
    };
    let port = url.port_or_known_default().context("no port")?;
    Ok(SocketAddr::new(ip, port))
}

/// The port of the last run if it is free, any free port otherwise.
async fn bind_remembered(data_dir: &Path) -> anyhow::Result<TcpListener> {
    let port_file = data_dir.join(PORT_FILE);
    let remembered = std::fs::read_to_string(&port_file)
        .ok()
        .and_then(|text| text.trim().parse::<u16>().ok())
        .filter(|port| *port != 0);
    let any = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0);
    let listener = match remembered {
        Some(port) => match bind_loopback(SocketAddr::new(any.ip(), port)).await {
            Ok(listener) => listener,
            Err(error) => {
                tracing::info!("port {port} is taken ({error:#}); using another one");
                bind_loopback(any).await?
            }
        },
        None => bind_loopback(any).await?,
    };
    let port = listener.local_addr()?.port();
    if remembered != Some(port) {
        let saved = std::fs::create_dir_all(data_dir)
            .and_then(|()| std::fs::write(&port_file, port.to_string()));
        if let Err(error) = saved {
            tracing::warn!(%error, "could not remember the panel port");
        }
    }
    Ok(listener)
}

/// The frontend Tauri embedded at build time (`web/dist`), served by the
/// Hub. Only exact file names are looked up: the Hub does its own SPA
/// fallback, so a missing script is a 404 rather than `index.html`.
struct EmbeddedUi<R: Runtime> {
    resolver: AssetResolver<R>,
    files: HashSet<String>,
}

impl<R: Runtime> EmbeddedUi<R> {
    fn new(resolver: AssetResolver<R>) -> Self {
        let files = resolver
            .iter()
            .map(|(key, _)| key.trim_start_matches('/').to_owned())
            .collect();
        Self { resolver, files }
    }
}

impl<R: Runtime> UiAssets for EmbeddedUi<R> {
    fn get(&self, path: &str) -> Option<UiAsset> {
        if !self.files.contains(path) {
            return None;
        }
        let asset = self.resolver.get(path.to_owned())?;
        let mime = if path.ends_with(".html") || path.ends_with(".js") || path.ends_with(".css") {
            // Name the charset explicitly; Tauri's types omit it.
            vesta_hub::mime_for(path).to_owned()
        } else {
            asset.mime_type
        };
        Some(UiAsset {
            bytes: asset.bytes.into(),
            mime,
        })
    }

    /// Tauri's IPC tries a custom-protocol `fetch` first (`ipc://localhost`,
    /// `http://ipc.localhost` on Windows) and falls back to `postMessage`.
    fn extra_connect_src(&self) -> &[&'static str] {
        &["ipc:", "http://ipc.localhost"]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dev_hub_origin_matches_the_vite_default() {
        let vite = include_str!("../../../../web/vite.config.ts");
        assert!(vite.contains(DEV_HUB_ORIGIN), "web/vite.config.ts");
        assert_eq!(
            parse_hub_origin(DEV_HUB_ORIGIN).unwrap(),
            "127.0.0.1:8181".parse().unwrap()
        );
        assert_eq!(
            parse_hub_origin("http://localhost:9000").unwrap(),
            "127.0.0.1:9000".parse().unwrap()
        );
        assert!(parse_hub_origin("http://example.com:9000").is_err());
        assert!(parse_hub_origin("not a url").is_err());
    }

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("vesta-app-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn runs_the_simulated_home_until_one_is_set_up() {
        let dir = temp_dir("home");
        let (config, _, saved_to) = load_home(&dir).unwrap();
        assert!(config.devices.iter().all(|device| device.driver == "sim"));
        assert_eq!(saved_to, None, "the simulated home keeps edits in memory");

        std::fs::write(dir.join("config.json"), vesta_drivers::demo::CONFIG).unwrap();
        assert!(load_home(&dir).is_err(), "half a home is an error");
        std::fs::write(dir.join("dashboard.json"), "{}").unwrap();
        assert!(load_home(&dir).is_err(), "an invalid home is an error");
        std::fs::write(dir.join("dashboard.json"), vesta_drivers::demo::DASHBOARD).unwrap();
        let (_, _, saved_to) = load_home(&dir).unwrap();
        assert_eq!(
            saved_to,
            Some(dir.join("dashboard.json")),
            "a real home saves edits"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[tokio::test]
    async fn reuses_the_last_port_when_free() {
        let dir = temp_dir("port");
        let first = bind_remembered(&dir).await.unwrap();
        let port = first.local_addr().unwrap().port();
        assert!(first.local_addr().unwrap().ip().is_loopback());
        assert_eq!(
            std::fs::read_to_string(dir.join(PORT_FILE)).unwrap(),
            port.to_string()
        );

        // Still taken by `first`: another port, remembered instead.
        let second = bind_remembered(&dir).await.unwrap();
        let other = second.local_addr().unwrap().port();
        assert_ne!(other, port);
        drop(first);
        drop(second);

        let third = bind_remembered(&dir).await.unwrap();
        assert_eq!(third.local_addr().unwrap().port(), other);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
