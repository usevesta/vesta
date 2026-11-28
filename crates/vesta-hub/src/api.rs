//! HTTP routes and the local-only guard.

use std::net::IpAddr;
use std::sync::Arc;

use axum::extract::{DefaultBodyLimit, Request, State};
use axum::http::StatusCode;
use axum::http::header::{HOST, ORIGIN};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde::Serialize;
use tokio::sync::watch;
use vesta_core::Snapshot;

use crate::dashboard::{self, Dashboards};
use crate::runtime::Runtime;
use crate::ws;

#[derive(Clone)]
pub(crate) struct AppState {
    pub(crate) runtime: Arc<Runtime>,
    pub(crate) dashboards: Arc<Dashboards>,
    /// Flips to `true` when the Hub shuts down, so WebSocket sessions close
    /// instead of holding graceful shutdown open.
    pub(crate) shutdown: watch::Receiver<bool>,
}

pub(crate) fn router(state: AppState) -> Router {
    Router::new()
        .route("/api/v1/health", get(health))
        .route("/api/v1/devices", get(devices))
        .route(
            "/api/v1/dashboard",
            get(dashboard::get)
                .put(dashboard::put)
                .layer(DefaultBodyLimit::max(dashboard::MAX_BODY_BYTES)),
        )
        .route("/api/v1/ws", get(ws::upgrade))
        .layer(middleware::from_fn(local_only))
        .with_state(state)
}

/// Liveness only. Clients should rely on the HTTP status, not the body.
#[derive(Serialize)]
struct Health {
    status: &'static str,
}

async fn health() -> Json<Health> {
    Json(Health { status: "ok" })
}

async fn devices(State(state): State<AppState>) -> Json<Snapshot> {
    Json(state.runtime.snapshot())
}

/// Defense in depth for a server on `127.0.0.1`: a web page in any browser
/// on the same machine can reach it. Rejects requests whose `Host` is not
/// local (DNS rebinding) and requests sent by pages from other origins
/// (WebSockets are not covered by CORS). Requests without `Origin`
/// (non-browser clients) are allowed.
pub(crate) async fn local_only(request: Request, next: Next) -> Response {
    let headers = request.headers();
    let host_ok = headers
        .get(HOST)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|host| is_local_host(host_of(host)));
    let origin_ok = headers
        .get(ORIGIN)
        .is_none_or(|value| value.to_str().ok().is_some_and(is_local_origin));
    if host_ok && origin_ok {
        next.run(request).await
    } else {
        tracing::debug!(host_ok, origin_ok, "rejected non-local request");
        StatusCode::FORBIDDEN.into_response()
    }
}

/// The host part of an `authority` (`host[:port]`, `[v6][:port]`).
pub(crate) fn host_of(authority: &str) -> &str {
    if let Some(rest) = authority.strip_prefix('[') {
        return rest.split_once(']').map_or("", |(host, _)| host);
    }
    match authority.rsplit_once(':') {
        Some((host, port)) if port.bytes().all(|b| b.is_ascii_digit()) => host,
        _ => authority,
    }
}

pub(crate) fn is_local_host(host: &str) -> bool {
    host.eq_ignore_ascii_case("localhost")
        || host.parse::<IpAddr>().is_ok_and(|ip| ip.is_loopback())
}

/// Pages served from this machine (dev server, the Tauri WebView).
fn is_local_origin(origin: &str) -> bool {
    let Some((scheme, authority)) = origin.split_once("://") else {
        return false;
    };
    let host = host_of(authority);
    match scheme {
        "http" | "https" => is_local_host(host) || host.eq_ignore_ascii_case("tauri.localhost"),
        "tauri" => host.eq_ignore_ascii_case("localhost"),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_hosts() {
        for host in [
            "127.0.0.1",
            "127.0.0.1:8080",
            "localhost",
            "LocalHost:5173",
            "[::1]",
            "[::1]:8080",
            "127.1.2.3:80",
        ] {
            assert!(is_local_host(host_of(host)), "{host} should be local");
        }
        for host in [
            "",
            "example.com",
            "example.com:8080",
            "192.0.2.10:8080",
            "localhost.example.com",
            "evil.localhost",
            "evil@localhost",
            "[2001:db8::1]:8080",
            "0.0.0.0:8080",
        ] {
            assert!(!is_local_host(host_of(host)), "{host} should be rejected");
        }
    }

    #[test]
    fn local_origins() {
        for origin in [
            "http://localhost:5173",
            "http://127.0.0.1:8080",
            "http://[::1]:5173",
            "https://localhost",
            "tauri://localhost",
            "http://tauri.localhost",
            "https://tauri.localhost",
        ] {
            assert!(is_local_origin(origin), "{origin} should be allowed");
        }
        for origin in [
            "null",
            "http://example.com",
            "https://192.0.2.10:5173",
            "http://evil.localhost",
            "file://",
            "ftp://localhost",
            "tauri://example.com",
        ] {
            assert!(!is_local_origin(origin), "{origin} should be rejected");
        }
    }
}
