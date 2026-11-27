//! Sign-in and the UI: login codes, the session cookie on HTTP and on the
//! WebSocket, exact origins, `ApiAuth::Disabled`, and serving the UI.

// Integration-test crate: helpers outside #[test] fns may panic too.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::http::header::{
    CACHE_CONTROL, CONTENT_SECURITY_POLICY, CONTENT_TYPE, COOKIE, LOCATION, ORIGIN,
    REFERRER_POLICY, SET_COOKIE, X_CONTENT_TYPE_OPTIONS,
};
use axum::http::{HeaderMap, Method, Request, StatusCode};
use futures_util::StreamExt;
use tokio::time::timeout;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::{self, Message};
use tower::ServiceExt;
use vesta_core::{Dashboard, HubConfig};
use vesta_hub::{ApiAuth, Hub, HubOptions, Registry, UiFiles, bind_loopback};

const HOME: &str = r#"{
  "schema_version": 1,
  "home_name": "Test Home",
  "devices": [
    { "id": "living_room_light", "name": "Living Room Light", "room": "living_room",
      "driver": "sim", "config": { "model": "light", "latency_ms": 10 } }
  ]
}"#;

const DASHBOARD: &str = r#"{
  "schema_version": 1,
  "theme": { "preset": "default", "auto_dark_mode": true, "touch_scale": "large" },
  "kiosk": { "screensaver_timeout_sec": 120, "wake_on": [] },
  "pages": [{ "id": "home", "title": "Home", "grid_columns": 4, "widgets": [] }]
}"#;

const LOCAL: &str = "127.0.0.1:8080";
const INDEX_HTML: &str = "<!doctype html><title>VESTA</title>";

async fn hub(options: HubOptions) -> Hub {
    Hub::start(
        &HubConfig::from_json(HOME).unwrap(),
        Dashboard::from_json(DASHBOARD).unwrap(),
        &Registry::builtin(),
        options,
    )
    .await
    .unwrap()
}

async fn serve(hub: Hub) -> SocketAddr {
    let listener = bind_loopback("127.0.0.1:0".parse().unwrap()).await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(hub.serve(listener, std::future::pending()));
    addr
}

struct Reply {
    status: StatusCode,
    headers: HeaderMap,
    body: Vec<u8>,
}

impl Reply {
    fn header(&self, name: axum::http::HeaderName) -> &str {
        self.headers
            .get(&name)
            .map_or("", |value| value.to_str().unwrap())
    }
}

async fn send(router: &Router, method: Method, path: &str, headers: &[(&str, &str)]) -> Reply {
    let mut request = Request::builder()
        .method(method)
        .uri(path)
        .header("host", LOCAL);
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    let response = router
        .clone()
        .oneshot(request.body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let body = axum::body::to_bytes(response.into_body(), 1 << 20)
        .await
        .unwrap()
        .to_vec();
    Reply {
        status,
        headers,
        body,
    }
}

async fn get(router: &Router, path: &str, headers: &[(&str, &str)]) -> Reply {
    send(router, Method::GET, path, headers).await
}

/// Redeems a fresh code; returns the `name=value` part of the cookie.
async fn sign_in(hub: &Hub, router: &Router) -> String {
    let code = hub.login_codes().unwrap().issue().unwrap();
    let reply = get(router, &code.login_path(), &[]).await;
    assert_eq!(reply.status, StatusCode::SEE_OTHER);
    reply
        .header(SET_COOKIE)
        .split(';')
        .next()
        .unwrap()
        .to_owned()
}

#[tokio::test]
async fn health_stays_open_and_the_api_needs_a_session() {
    let hub = hub(HubOptions::default()).await;
    let router = hub.router();

    let health = get(&router, "/api/v1/health", &[]).await;
    assert_eq!(health.status, StatusCode::OK);

    for path in ["/api/v1/devices", "/api/v1/dashboard", "/api/v1/ws"] {
        let reply = get(&router, path, &[]).await;
        assert_eq!(reply.status, StatusCode::UNAUTHORIZED, "{path}");
        assert!(reply.body.is_empty(), "{path}: no details");
        let bogus = get(&router, path, &[("cookie", "vesta_session=AAAA")]).await;
        assert_eq!(bogus.status, StatusCode::UNAUTHORIZED, "{path}");
    }

    let cookie = sign_in(&hub, &router).await;
    for path in ["/api/v1/devices", "/api/v1/dashboard"] {
        let reply = get(&router, path, &[("cookie", &cookie)]).await;
        assert_eq!(reply.status, StatusCode::OK, "{path}");
        assert_eq!(reply.header(X_CONTENT_TYPE_OPTIONS), "nosniff");
    }
}

#[tokio::test]
async fn a_login_code_becomes_a_session_cookie_once() {
    let hub = hub(HubOptions::default()).await;
    let router = hub.router();
    let code = hub.login_codes().unwrap().issue().unwrap();

    let reply = get(&router, &code.login_path(), &[]).await;
    assert_eq!(reply.status, StatusCode::SEE_OTHER);
    assert_eq!(reply.header(LOCATION), "/");
    assert_eq!(reply.header(CACHE_CONTROL), "no-store");
    assert_eq!(reply.header(REFERRER_POLICY), "no-referrer");
    let cookie = reply.header(SET_COOKIE);
    let mut parts = cookie.split("; ");
    let session = parts.next().unwrap();
    assert!(session.starts_with("vesta_session="), "{cookie}");
    assert_eq!(session.len(), "vesta_session=".len() + 43, "256-bit id");
    assert_eq!(
        parts.collect::<Vec<_>>(),
        ["HttpOnly", "SameSite=Strict", "Path=/"],
        "no Secure on plain loopback, no expiry"
    );
    assert!(
        !cookie.contains(code.as_str()),
        "the session is not the code"
    );

    // Single use.
    let again = get(&router, &code.login_path(), &[]).await;
    assert_eq!(again.status, StatusCode::UNAUTHORIZED);
    assert!(again.header(SET_COOKIE).is_empty());

    for bad in [
        "/api/v1/session",
        "/api/v1/session?code=",
        "/api/v1/session?code=AAAAAAAAAAAAAAAAAAAAAA",
        "/api/v1/session?other=1",
    ] {
        assert_eq!(
            get(&router, bad, &[]).await.status,
            StatusCode::UNAUTHORIZED,
            "{bad}"
        );
    }

    let devices = get(&router, "/api/v1/devices", &[("cookie", session)]).await;
    assert_eq!(devices.status, StatusCode::OK);
}

async fn ws_with(
    addr: SocketAddr,
    cookie: Option<&str>,
    origin: Option<&str>,
) -> Result<Message, tungstenite::Error> {
    let mut request = format!("ws://{addr}/api/v1/ws")
        .into_client_request()
        .unwrap();
    if let Some(cookie) = cookie {
        request
            .headers_mut()
            .insert(COOKIE, cookie.parse().unwrap());
    }
    if let Some(origin) = origin {
        request
            .headers_mut()
            .insert(ORIGIN, origin.parse().unwrap());
    }
    let (mut ws, _) = connect_async(request).await?;
    let first = timeout(Duration::from_secs(5), ws.next())
        .await
        .expect("no message within 5 s")
        .expect("connection closed")?;
    Ok(first)
}

fn http_status(result: Result<Message, tungstenite::Error>) -> StatusCode {
    match result {
        Err(tungstenite::Error::Http(response)) => response.status(),
        other => panic!("expected an HTTP refusal, got {other:?}"),
    }
}

fn is_snapshot(result: Result<Message, tungstenite::Error>) -> bool {
    match result {
        Ok(Message::Text(text)) => text.as_str().contains(r#""type":"snapshot""#),
        _ => false,
    }
}

#[tokio::test]
async fn the_websocket_needs_the_cookie_and_an_exact_origin() {
    let hub = hub(HubOptions {
        allowed_origins: vec!["http://localhost:1420".to_owned()],
        ..HubOptions::default()
    })
    .await;
    let cookie = sign_in(&hub, &hub.router()).await;
    let addr = serve(hub).await;
    let port = addr.port();
    let own = format!("http://127.0.0.1:{port}");
    let own_by_name = format!("http://localhost:{port}");

    assert_eq!(
        http_status(ws_with(addr, None, Some(&own)).await),
        StatusCode::UNAUTHORIZED
    );
    assert!(is_snapshot(ws_with(addr, Some(&cookie), None).await));
    assert!(is_snapshot(ws_with(addr, Some(&cookie), Some(&own)).await));
    assert!(is_snapshot(
        ws_with(addr, Some(&cookie), Some(&own_by_name)).await
    ));
    assert!(
        is_snapshot(ws_with(addr, Some(&cookie), Some("http://localhost:1420")).await),
        "the configured dev server"
    );

    // Cookies ignore ports: another local server must not borrow the session.
    let other_port = format!("http://127.0.0.1:{}", port.wrapping_add(1).max(1));
    for origin in [
        other_port.as_str(),
        "http://localhost:5173",
        "http://example.com",
    ] {
        assert_eq!(
            http_status(ws_with(addr, Some(&cookie), Some(origin)).await),
            StatusCode::FORBIDDEN,
            "{origin}"
        );
    }
}

#[tokio::test]
async fn disabled_is_only_used_when_chosen() {
    assert_eq!(HubOptions::default().auth, ApiAuth::Sessions);
    assert!(hub(HubOptions::default()).await.login_codes().is_some());

    let hub = hub(HubOptions {
        auth: ApiAuth::Disabled,
        ..HubOptions::default()
    })
    .await;
    assert!(hub.login_codes().is_none());
    let router = hub.router();
    assert_eq!(
        get(&router, "/api/v1/devices", &[]).await.status,
        StatusCode::OK
    );
    assert_eq!(
        get(&router, "/api/v1/session?code=AAAAAAAAAAAAAAAAAAAAAA", &[])
            .await
            .status,
        StatusCode::NOT_FOUND
    );
    let addr = serve(hub).await;
    assert!(is_snapshot(ws_with(addr, None, None).await));
}

#[tokio::test]
async fn allowed_origins_must_be_local_and_exact() {
    for bad in [
        "http://example.com",
        "localhost:1420",
        "http://localhost:1420/x",
    ] {
        let error = Hub::start(
            &HubConfig::from_json(HOME).unwrap(),
            Dashboard::from_json(DASHBOARD).unwrap(),
            &Registry::builtin(),
            HubOptions {
                allowed_origins: vec![bad.to_owned()],
                ..HubOptions::default()
            },
        )
        .await
        .err()
        .unwrap();
        assert!(
            error.to_string().contains("allowed origin"),
            "{bad}: {error}"
        );
    }
}

fn ui() -> UiFiles {
    let mut files = UiFiles::new();
    files
        .insert("index.html", INDEX_HTML)
        .insert("assets/index-1a2b.js", "export {}")
        .insert("favicon.svg", "<svg/>");
    files
}

async fn hub_with_ui() -> Hub {
    hub(HubOptions {
        ui: Some(Arc::new(ui())),
        ..HubOptions::default()
    })
    .await
}

fn assert_ui_headers(reply: &Reply) {
    let csp = reply.header(CONTENT_SECURITY_POLICY);
    assert!(csp.contains("default-src 'self'"), "{csp}");
    assert!(csp.contains("script-src 'self';"), "{csp}");
    assert!(csp.contains("connect-src 'self';"), "{csp}");
    assert!(csp.contains("frame-ancestors 'none'"), "{csp}");
    assert_eq!(reply.header(X_CONTENT_TYPE_OPTIONS), "nosniff");
    assert_eq!(reply.header(REFERRER_POLICY), "no-referrer");
}

#[tokio::test]
async fn the_ui_is_served_with_spa_fallback_and_strict_headers() {
    let hub = hub_with_ui().await;
    let router = hub.router();

    for path in ["/", "/rooms/kitchen", "/rooms/kitchen/"] {
        let reply = get(&router, path, &[]).await;
        assert_eq!(reply.status, StatusCode::OK, "{path}");
        assert_eq!(reply.body, INDEX_HTML.as_bytes(), "{path}");
        assert_eq!(reply.header(CONTENT_TYPE), "text/html; charset=utf-8");
        assert_eq!(reply.header(CACHE_CONTROL), "no-cache");
        assert_ui_headers(&reply);
    }

    let script = get(&router, "/assets/index-1a2b.js", &[]).await;
    assert_eq!(script.status, StatusCode::OK);
    assert_eq!(script.body, b"export {}");
    assert_eq!(
        script.header(CONTENT_TYPE),
        "text/javascript; charset=utf-8"
    );
    assert_eq!(
        script.header(CACHE_CONTROL),
        "public, max-age=31536000, immutable"
    );
    assert_ui_headers(&script);

    // A missing file is not the app.
    let missing = get(&router, "/assets/missing.js", &[]).await;
    assert_eq!(missing.status, StatusCode::NOT_FOUND);
    // Unknown API paths are never answered with the UI.
    for path in ["/api", "/api/v1/nope", "/api/v2/devices"] {
        let reply = get(&router, path, &[]).await;
        assert_eq!(reply.status, StatusCode::NOT_FOUND, "{path}");
        assert_ne!(reply.body, INDEX_HTML.as_bytes(), "{path}");
    }

    let head = send(&router, Method::HEAD, "/", &[]).await;
    assert_eq!(head.status, StatusCode::OK);
    let post = send(&router, Method::POST, "/", &[]).await;
    assert_eq!(post.status, StatusCode::METHOD_NOT_ALLOWED);

    // The UI is public; the API next to it still needs a session.
    assert_eq!(
        get(&router, "/api/v1/devices", &[]).await.status,
        StatusCode::UNAUTHORIZED
    );
    // The local-only guard covers the UI too (DNS rebinding).
    let request = Request::builder()
        .uri("/")
        .header("host", "attacker.example")
        .body(Body::empty())
        .unwrap();
    let response = router.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn ui_paths_cannot_escape() {
    let dir = std::env::temp_dir().join(format!("vesta-hub-ui-{}-{}", std::process::id(), line!()));
    let dist = dir.join("dist");
    std::fs::create_dir_all(dist.join("assets")).unwrap();
    std::fs::write(dist.join("index.html"), INDEX_HTML).unwrap();
    std::fs::write(dist.join("assets/app.js"), "export {}").unwrap();
    std::fs::write(dist.join(".hidden"), "no").unwrap();
    std::fs::write(dir.join("secret.txt"), "top secret").unwrap();

    let files = UiFiles::load_dir(&dist).unwrap();
    assert_eq!(files.len(), 2, "hidden files are skipped");
    let hub = hub(HubOptions {
        ui: Some(Arc::new(files)),
        ..HubOptions::default()
    })
    .await;
    let router = hub.router();
    assert_eq!(
        get(&router, "/assets/app.js", &[]).await.status,
        StatusCode::OK
    );
    for path in [
        "/../secret.txt",
        "/assets/../../secret.txt",
        "/assets/%2e%2e/%2e%2e/secret.txt",
        "/%2e%2e%2fsecret.txt",
        "/assets/..%5c..%5csecret.txt",
        "/.hidden",
    ] {
        let reply = get(&router, path, &[]).await;
        assert_eq!(reply.status, StatusCode::NOT_FOUND, "{path}");
        assert!(
            !String::from_utf8_lossy(&reply.body).contains("secret"),
            "{path}"
        );
    }

    assert!(UiFiles::load_dir(&dir.join("missing")).is_err());
    assert!(
        UiFiles::load_dir(&dist.join("assets")).is_err(),
        "no index.html"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}
