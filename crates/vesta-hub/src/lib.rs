//! The VESTA Hub: device runtime plus the local HTTP/WebSocket API.
//!
//! The frontend always talks to a **local** endpoint. On the Hub device that
//! is this server, bound to loopback only; other devices will reach it
//! through `vesta-link`'s encrypted tunnel, never over plain HTTP on the LAN.
//!
//! | Route | Response |
//! |---|---|
//! | `GET /api/v1/health` | `{ "status": "ok" }` (liveness, no session needed) |
//! | `GET /api/v1/session?code=…` | Redeems a one-time login code: session cookie + `303 → /` |
//! | `GET /api/v1/devices` | `Snapshot`: `{ "devices": [...] }` |
//! | `GET /api/v1/dashboard` | `Dashboard`, with an `ETag` |
//! | `PUT /api/v1/dashboard` | Saves a `Dashboard` (`If-Match` required; see `dashboard.rs`) |
//! | `GET /api/v1/ws` | WebSocket: `ServerMessage` / `ClientMessage` |
//!
//! The API is brand-free and never exposes driver keys or driver config:
//! devices are described only by their endpoints and capabilities.
//!
//! Every other `/api/v1/*` route needs a session (see [`ApiAuth`] and
//! `docs/architecture/local-session.md`). With [`HubOptions::ui`] set, the
//! same origin also serves the wall-panel UI.

use std::future::Future;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, bail};
use axum::Router;
use tokio::net::TcpListener;
use tokio::sync::watch;
use tokio::task::JoinSet;
use vesta_core::{Dashboard, EventBus, HubConfig};
pub use vesta_drivers::Registry;

mod api;
mod assets;
mod auth;
mod dashboard;
mod http;
mod runtime;
mod state;
mod ws;

pub use assets::{UiAsset, UiAssets, UiFiles, mime_for};
pub use auth::{
    ApiAuth, Identity, IdentityKind, LOGIN_CODE_TTL, LOGIN_PATH, LoginCode, LoginCodes, Role,
    SESSION_COOKIE,
};
use dashboard::Dashboards;
pub use dashboard::{DashboardStore, JsonFileStore, MemoryStore};
pub use state::{FileState, STATE_SCHEMA_VERSION, StateStorage};

use runtime::Runtime;

/// Tunables. `Default` matches the documented behaviour.
#[derive(Debug, Clone)]
pub struct HubOptions {
    /// How long a driver has to accept a command before the client gets
    /// `timeout`.
    pub command_timeout: Duration,
    /// The same for slow commands (`Command::is_slow`, e.g. saving a
    /// charger's hours, which may go through the internet).
    pub slow_command_timeout: Duration,
    /// Updates buffered per WebSocket session before it must resync with a
    /// fresh snapshot.
    pub bus_capacity: usize,
    /// Who may use the API. Sessions unless explicitly disabled.
    pub auth: ApiAuth,
    /// Page origins besides the Hub's own (`http://127.0.0.1:<port>`,
    /// `http://localhost:<port>`) whose requests may use a session, given
    /// exactly, e.g. `http://localhost:1420` for a dev server that proxies
    /// the API. Must be on this machine.
    pub allowed_origins: Vec<String>,
    /// The wall-panel UI, served on the API's origin.
    pub ui: Option<Arc<dyn UiAssets>>,
    /// Where drivers keep their per-device state between restarts (e.g. a
    /// charger's last known hours). In memory unless told otherwise.
    pub driver_state: StateStorage,
}

impl HubOptions {
    pub const DEFAULT_COMMAND_TIMEOUT: Duration = Duration::from_secs(5);
}

impl Default for HubOptions {
    fn default() -> Self {
        Self {
            command_timeout: Self::DEFAULT_COMMAND_TIMEOUT,
            slow_command_timeout: vesta_core::SLOW_COMMAND_TIMEOUT,
            bus_capacity: EventBus::DEFAULT_CAPACITY,
            auth: ApiAuth::default(),
            allowed_origins: Vec::new(),
            ui: None,
            driver_state: StateStorage::Memory,
        }
    }
}

/// A running Hub: devices are live as soon as [`Hub::start`] returns; the
/// API is served by [`Hub::serve`].
pub struct Hub {
    runtime: Arc<Runtime>,
    dashboards: Arc<Dashboards>,
    auth: auth::Auth,
    ui: Option<assets::UiService>,
    shutdown: watch::Sender<bool>,
    tasks: JoinSet<()>,
}

impl Hub {
    /// Validates both documents, builds every configured device through
    /// `registry` and starts the device loops. Dashboard edits are kept in
    /// memory until [`Hub::with_dashboard_store`] says where to save them.
    pub async fn start(
        config: &HubConfig,
        dashboard: Dashboard,
        registry: &Registry,
        options: HubOptions,
    ) -> anyhow::Result<Self> {
        config.validate().context("invalid hub config")?;
        dashboard.validate().context("invalid dashboard")?;
        let auth = auth::Auth::new(options.auth, &options.allowed_origins)?;
        if options.auth == ApiAuth::Disabled {
            tracing::warn!(
                "API authentication is disabled (development only): \
                 any program on this machine can control the home"
            );
        }
        let ui = options.ui.clone().map(assets::UiService::new).transpose()?;
        let mut tasks = JoinSet::new();
        let runtime = Runtime::start(config, registry, &options, &mut tasks);
        dashboard::warn_unknown_targets(&dashboard, &runtime);
        let dashboards = Arc::new(Dashboards::new(dashboard)?);
        let (shutdown, _) = watch::channel(false);
        Ok(Self {
            runtime,
            dashboards,
            auth,
            ui,
            shutdown,
            tasks,
        })
    }

    /// Saves dashboard edits to `store` (e.g. a [`JsonFileStore`] for the
    /// file the dashboard was loaded from). Applies to routers created
    /// before and after this call.
    #[must_use]
    pub fn with_dashboard_store(self, store: impl DashboardStore) -> Self {
        self.dashboards.set_store(Arc::new(store));
        self
    }

    /// Issues one-time login codes for the panel; `None` when
    /// authentication is disabled.
    pub fn login_codes(&self) -> Option<LoginCodes> {
        self.auth.login_codes()
    }

    /// The routes, for embedding or testing without a socket. Without a
    /// bound address only [`HubOptions::allowed_origins`] may use a
    /// session; [`Hub::serve`] also allows the Hub's own origin.
    pub fn router(&self) -> Router {
        self.app(None)
    }

    fn app(&self, local: Option<SocketAddr>) -> Router {
        let api = api::router(api::AppState {
            runtime: Arc::clone(&self.runtime),
            dashboards: Arc::clone(&self.dashboards),
            shutdown: self.shutdown.subscribe(),
        });
        http::app(api, self.auth.policy(local), self.ui.clone())
    }

    /// Serves the API on `listener` until `shutdown` resolves, then closes
    /// WebSocket sessions, waits for open requests and stops the devices.
    /// Refuses a listener that is not bound to loopback.
    pub async fn serve<F>(mut self, listener: TcpListener, shutdown: F) -> anyhow::Result<()>
    where
        F: Future<Output = ()> + Send + 'static,
    {
        let local = listener
            .local_addr()
            .context("listener has no local address")?;
        ensure_loopback(local)?;
        let router = self.app(Some(local));
        let notify = self.shutdown.clone();
        tracing::info!("listening on http://{local}");
        axum::serve(listener, router)
            .with_graceful_shutdown(async move {
                shutdown.await;
                notify.send_replace(true);
            })
            .await
            .context("server error")?;
        self.tasks.shutdown().await;
        tracing::info!("hub stopped");
        Ok(())
    }
}

/// Binds the API listener. Only loopback addresses are accepted in this
/// version: phones and tablets will reach the Hub through the paired,
/// encrypted link (`vesta-link`), never over plain HTTP on the LAN.
pub async fn bind_loopback(addr: SocketAddr) -> anyhow::Result<TcpListener> {
    ensure_loopback(addr)?;
    TcpListener::bind(addr)
        .await
        .with_context(|| format!("could not listen on {addr}"))
}

fn ensure_loopback(addr: SocketAddr) -> anyhow::Result<()> {
    if addr.ip().is_loopback() {
        Ok(())
    } else {
        bail!(
            "refusing to listen on {addr}: the hub only accepts connections from this machine \
             (127.0.0.1 or ::1); other devices will connect through the paired, encrypted link"
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn refuses_non_loopback_addresses() {
        for addr in ["0.0.0.0:8080", "192.0.2.10:8080", "[::]:8080"] {
            let error = bind_loopback(addr.parse().unwrap()).await.err().unwrap();
            assert!(error.to_string().contains("refusing to listen"), "{error}");
        }
        let listener = bind_loopback("127.0.0.1:0".parse().unwrap()).await.unwrap();
        assert!(listener.local_addr().unwrap().ip().is_loopback());
    }
}
