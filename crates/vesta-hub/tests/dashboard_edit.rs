//! Saving the dashboard from the editor: `PUT /api/v1/dashboard` with
//! optimistic concurrency, validation, atomic file storage and the
//! `dashboard_changed` broadcast.

// Integration-test crate: helpers outside #[test] fns may panic too.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::http::{HeaderMap, Method, Request, StatusCode};
use futures_util::StreamExt;
use serde_json::{Value, json};
use tokio::net::TcpStream;
use tokio::time::timeout;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async};
use tower::ServiceExt;
use vesta_core::{Dashboard, HubConfig, ServerMessage};
use vesta_hub::{DashboardStore, Hub, HubOptions, JsonFileStore, Registry, bind_loopback};

const HOME: &str = r#"{
  "schema_version": 1,
  "home_name": "Test Home",
  "devices": [
    { "id": "living_room_light", "name": "Living Room Light", "room": "living_room",
      "driver": "sim", "config": { "model": "light", "latency_ms": 10 } },
    { "id": "front_door_strike", "name": "Front Door", "room": "entrance",
      "driver": "sim", "config": { "model": "door_strike", "latency_ms": 10 } }
  ]
}"#;

const DASHBOARD: &str = r#"{
  "schema_version": 1,
  "theme": { "preset": "glass-dark", "auto_dark_mode": true, "touch_scale": "large" },
  "kiosk": { "screensaver_timeout_sec": 120, "wake_on": [] },
  "pages": [{
    "id": "home", "title": "Home", "grid_columns": 4,
    "widgets": [
      { "type": "light-control", "target": "living_room_light/main", "col_span": 2, "row_span": 1 },
      { "type": "door-button", "target": "front_door_strike/main", "col_span": 2, "row_span": 1 }
    ]
  }]
}"#;

type Ws = WebSocketStream<MaybeTlsStream<TcpStream>>;

fn initial() -> Dashboard {
    Dashboard::from_json(DASHBOARD).unwrap()
}

/// An edit like the UI makes: rename the page, add a page with a clock.
fn edited() -> Dashboard {
    let mut dashboard = initial();
    dashboard.pages[0].title = "Inicio".to_owned();
    dashboard.pages[0].widgets.swap(0, 1);
    let mut page: vesta_core::Page = serde_json::from_value(json!({
        "id": "page_2", "title": "Kitchen", "icon": "kitchen", "grid_columns": 4,
        "widgets": [{ "type": "clock-weather", "col_span": 2, "row_span": 1 }]
    }))
    .unwrap();
    page.widgets[0]
        .props
        .insert("label".to_owned(), json!("Kitchen clock"));
    dashboard.pages.push(page);
    dashboard
}

async fn hub() -> Hub {
    Hub::start(
        &HubConfig::from_json(HOME).unwrap(),
        initial(),
        &Registry::builtin(),
        HubOptions::default(),
    )
    .await
    .unwrap()
}

struct Response {
    status: StatusCode,
    headers: HeaderMap,
    body: Value,
}

impl Response {
    fn etag(&self) -> &str {
        self.headers.get("etag").expect("no ETag").to_str().unwrap()
    }
}

async fn send(router: &Router, request: Request<Body>) -> Response {
    let response = router.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
        .await
        .unwrap();
    Response {
        status,
        headers,
        body: serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    }
}

async fn get(router: &Router) -> Response {
    let request = Request::builder()
        .uri("/api/v1/dashboard")
        .header("host", "127.0.0.1:8080")
        .body(Body::empty())
        .unwrap();
    send(router, request).await
}

fn put_request(if_match: Option<&str>, content_type: Option<&str>, body: String) -> Request<Body> {
    let mut request = Request::builder()
        .method(Method::PUT)
        .uri("/api/v1/dashboard")
        .header("host", "127.0.0.1:8080")
        .header("origin", "http://localhost:5173");
    if let Some(if_match) = if_match {
        request = request.header("if-match", if_match);
    }
    if let Some(content_type) = content_type {
        request = request.header("content-type", content_type);
    }
    request.body(Body::from(body)).unwrap()
}

async fn put(router: &Router, if_match: &str, dashboard: &Dashboard) -> Response {
    let body = serde_json::to_string(dashboard).unwrap();
    send(
        router,
        put_request(Some(if_match), Some("application/json"), body),
    )
    .await
}

async fn put_raw(router: &Router, if_match: &str, body: Value) -> Response {
    send(
        router,
        put_request(Some(if_match), Some("application/json"), body.to_string()),
    )
    .await
}

fn revision_of(etag: &str) -> &str {
    etag.trim_matches('"')
}

// ---------------------------------------------------------------------------
// HTTP
// ---------------------------------------------------------------------------

#[tokio::test]
async fn saving_replaces_the_dashboard_and_changes_the_etag() {
    let hub = hub().await;
    let router = hub.router();

    let before = get(&router).await;
    assert_eq!(before.status, StatusCode::OK);
    let etag = before.etag().to_owned();
    assert!(etag.starts_with('"') && etag.ends_with('"'), "{etag}");
    assert_eq!(before.headers["cache-control"], "no-cache");
    assert_eq!(
        serde_json::from_value::<Dashboard>(before.body).unwrap(),
        initial()
    );

    let saved = put(&router, &etag, &edited()).await;
    assert_eq!(saved.status, StatusCode::OK, "{}", saved.body);
    assert_ne!(saved.etag(), etag);
    assert_eq!(
        serde_json::from_value::<Dashboard>(saved.body.clone()).unwrap(),
        edited()
    );

    let after = get(&router).await;
    assert_eq!(after.etag(), saved.etag());
    assert_eq!(after.body, saved.body);

    // The same content always has the same ETag, also on another Hub
    // (e.g. after a restart).
    let other = hub_with(edited()).await;
    assert_eq!(get(&other.router()).await.etag(), saved.etag());

    // Saving it again changes nothing.
    let again = put(&router, saved.etag(), &edited()).await;
    assert_eq!(again.status, StatusCode::OK);
    assert_eq!(again.etag(), saved.etag());
}

async fn hub_with(dashboard: Dashboard) -> Hub {
    Hub::start(
        &HubConfig::from_json(HOME).unwrap(),
        dashboard,
        &Registry::builtin(),
        HubOptions::default(),
    )
    .await
    .unwrap()
}

#[tokio::test]
async fn a_save_based_on_an_old_version_is_refused() {
    let hub = hub().await;
    let router = hub.router();
    let original = get(&router).await.etag().to_owned();

    // Someone else saves first.
    let first = put(&router, &original, &edited()).await;
    assert_eq!(first.status, StatusCode::OK);

    // This screen still has the old version: 412, with the current ETag.
    let mut mine = initial();
    mine.pages[0].title = "Mine".to_owned();
    let stale = put(&router, &original, &mine).await;
    assert_eq!(stale.status, StatusCode::PRECONDITION_FAILED);
    assert_eq!(stale.etag(), first.etag());
    assert_eq!(stale.body, json!({ "error": "stale" }));
    assert_eq!(get(&router).await.body["pages"][0]["title"], "Inicio");

    // Weak or unquoted tags never match (strong comparison).
    let current = first.etag().to_owned();
    for weak in [format!("W/{current}"), revision_of(&current).to_owned()] {
        let response = put(&router, &weak, &mine).await;
        assert_eq!(response.status, StatusCode::PRECONDITION_FAILED, "{weak}");
    }
    // A list containing the current ETag does.
    let listed = put(&router, &format!("\"other\", {current}"), &mine).await;
    assert_eq!(listed.status, StatusCode::OK);
}

#[tokio::test]
async fn a_save_without_if_match_is_refused() {
    let hub = hub().await;
    let router = hub.router();
    let body = serde_json::to_string(&edited()).unwrap();
    let response = send(&router, put_request(None, Some("application/json"), body)).await;
    assert_eq!(response.status, StatusCode::PRECONDITION_REQUIRED);
    assert_eq!(response.body["error"], "if_match_required");
    assert_eq!(get(&router).await.body["pages"][0]["title"], "Home");
}

#[tokio::test]
async fn invalid_dashboards_are_refused_with_a_reason() {
    let hub = hub().await;
    let router = hub.router();
    let etag = get(&router).await.etag().to_owned();
    let base = serde_json::to_value(initial()).unwrap();
    let with = |edit: &dyn Fn(&mut Value)| {
        let mut value = base.clone();
        edit(&mut value);
        value
    };

    let not_json = send(
        &router,
        put_request(
            Some(&etag),
            Some("application/json"),
            "{ not json".to_owned(),
        ),
    )
    .await;
    assert_eq!(not_json.status, StatusCode::BAD_REQUEST);
    assert_eq!(not_json.body["error"], "invalid_json");

    for (case, body) in [
        (
            "missing pages",
            with(&|d| drop(d.as_object_mut().unwrap().remove("pages"))),
        ),
        (
            "bad target",
            with(&|d| d["pages"][0]["widgets"][0]["target"] = json!("Living Room")),
        ),
        ("newer schema", with(&|d| d["schema_version"] = json!(2))),
        (
            "too wide",
            with(&|d| d["pages"][0]["widgets"][0]["col_span"] = json!(5)),
        ),
        (
            "empty title",
            with(&|d| d["pages"][0]["title"] = json!(" ")),
        ),
        (
            "bad page id",
            with(&|d| d["pages"][0]["id"] = json!("Home Page")),
        ),
        (
            "duplicate page",
            with(&|d| {
                let page = d["pages"][0].clone();
                d["pages"].as_array_mut().unwrap().push(page);
            }),
        ),
    ] {
        let response = put_raw(&router, &etag, body).await;
        assert_eq!(response.status, StatusCode::UNPROCESSABLE_ENTITY, "{case}");
        assert_eq!(response.body["error"], "invalid_dashboard", "{case}");
        assert!(response.body["message"].is_string(), "{case}");
    }

    let wrong_type = send(
        &router,
        put_request(Some(&etag), Some("text/plain"), base.to_string()),
    )
    .await;
    assert_eq!(wrong_type.status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
    let no_type = send(&router, put_request(Some(&etag), None, base.to_string())).await;
    assert_eq!(no_type.status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
    let charset = send(
        &router,
        put_request(
            Some(&etag),
            Some("application/json; charset=utf-8"),
            base.to_string(),
        ),
    )
    .await;
    assert_eq!(charset.status, StatusCode::OK);

    let huge = send(
        &router,
        put_request(
            Some(&etag),
            Some("application/json"),
            " ".repeat(300 * 1024),
        ),
    )
    .await;
    assert_eq!(huge.status, StatusCode::PAYLOAD_TOO_LARGE);

    // Nothing changed.
    assert_eq!(get(&router).await.etag(), etag);
}

#[tokio::test]
async fn unknown_targets_are_allowed() {
    let hub = hub().await;
    let router = hub.router();
    let etag = get(&router).await.etag().to_owned();
    let mut dashboard = initial();
    dashboard.pages[0].widgets[0].target = Some("attic_light/main".parse().unwrap());
    dashboard.pages[0].widgets[1].widget_type = "thermostat-control".to_owned();
    let response = put(&router, &etag, &dashboard).await;
    assert_eq!(response.status, StatusCode::OK);
}

#[tokio::test]
async fn foreign_pages_cannot_save() {
    let hub = hub().await;
    let router = hub.router();
    let etag = get(&router).await.etag().to_owned();
    let mut request = put_request(
        Some(&etag),
        Some("application/json"),
        serde_json::to_string(&edited()).unwrap(),
    );
    request
        .headers_mut()
        .insert("origin", "https://attacker.example".parse().unwrap());
    let response = send(&router, request).await;
    assert_eq!(response.status, StatusCode::FORBIDDEN);
    assert_eq!(get(&router).await.etag(), etag);
}

// ---------------------------------------------------------------------------
// Storage
// ---------------------------------------------------------------------------

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "vesta-hub-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    fn entries(&self) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(&self.0)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[tokio::test]
async fn saves_are_written_to_the_dashboard_file() {
    let dir = TempDir::new();
    let path = dir.0.join("dashboard.json");
    fs::write(&path, DASHBOARD).unwrap();

    let hub = hub().await.with_dashboard_store(JsonFileStore::new(&path));
    let router = hub.router();
    let etag = get(&router).await.etag().to_owned();

    let saved = put(&router, &etag, &edited()).await;
    assert_eq!(saved.status, StatusCode::OK);
    let on_disk = fs::read_to_string(&path).unwrap();
    assert_eq!(Dashboard::from_json(&on_disk).unwrap(), edited());
    assert_eq!(dir.entries(), ["dashboard.json"], "no temporary files left");

    // A refused save leaves the file alone.
    let stale = put(&router, &etag, &initial()).await;
    assert_eq!(stale.status, StatusCode::PRECONDITION_FAILED);
    assert_eq!(fs::read_to_string(&path).unwrap(), on_disk);

    // A Hub started from the file serves what was saved, same ETag.
    let restarted = hub_with(Dashboard::from_json(&on_disk).unwrap()).await;
    assert_eq!(get(&restarted.router()).await.etag(), saved.etag());
}

#[tokio::test]
async fn a_failed_write_changes_nothing() {
    let dir = TempDir::new();
    // The directory does not exist: every write fails.
    let path = dir.0.join("gone").join("dashboard.json");
    let hub = hub().await.with_dashboard_store(JsonFileStore::new(&path));
    let router = hub.router();
    let addr = serve(hub).await;
    let mut ws = connect(addr).await;
    let etag = get(&router).await.etag().to_owned();

    let response = put(&router, &etag, &edited()).await;
    assert_eq!(response.status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(
        response.body,
        json!({ "error": "save_failed" }),
        "no paths leak"
    );
    assert_eq!(get(&router).await.etag(), etag);
    assert!(
        timeout(Duration::from_millis(200), next_dashboard_change(&mut ws))
            .await
            .is_err(),
        "nothing to announce"
    );
}

#[test]
fn json_file_store_is_usable_on_its_own() {
    // What the app will use for its data directory.
    let dir = TempDir::new();
    let store = JsonFileStore::new(dir.0.join("dashboard.json"));
    store.save(&edited()).unwrap();
    let written = fs::read_to_string(store.path()).unwrap();
    assert_eq!(Dashboard::from_json(&written).unwrap(), edited());
}

// ---------------------------------------------------------------------------
// WebSocket broadcast
// ---------------------------------------------------------------------------

async fn serve(hub: Hub) -> SocketAddr {
    let listener = bind_loopback("127.0.0.1:0".parse().unwrap()).await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(hub.serve(listener, std::future::pending()));
    addr
}

async fn connect(addr: SocketAddr) -> Ws {
    let (mut ws, _) = connect_async(format!("ws://{addr}/api/v1/ws"))
        .await
        .unwrap();
    // The snapshot always comes first.
    let first = next_message(&mut ws).await;
    assert!(matches!(first, ServerMessage::Snapshot(_)), "{first:?}");
    ws
}

async fn next_message(ws: &mut Ws) -> ServerMessage {
    loop {
        let frame = timeout(Duration::from_secs(5), ws.next())
            .await
            .expect("no message within 5 s")
            .expect("connection closed")
            .unwrap();
        if let Message::Text(text) = frame {
            return serde_json::from_str(&text).unwrap();
        }
    }
}

/// Skips device updates until the next `dashboard_changed`.
async fn next_dashboard_change(ws: &mut Ws) -> String {
    loop {
        if let ServerMessage::DashboardChanged(changed) = next_message(ws).await {
            return changed.revision;
        }
    }
}

#[tokio::test]
async fn every_session_hears_about_a_save() {
    let hub = hub().await;
    let router = hub.router();
    let addr = serve(hub).await;
    let mut panel = connect(addr).await;
    let mut phone = connect(addr).await;

    let etag = get(&router).await.etag().to_owned();
    let saved = put(&router, &etag, &edited()).await;
    assert_eq!(saved.status, StatusCode::OK);
    let revision = revision_of(saved.etag()).to_owned();
    assert_eq!(next_dashboard_change(&mut panel).await, revision);
    assert_eq!(next_dashboard_change(&mut phone).await, revision);

    // A no-op save announces nothing; the next real one does.
    assert_eq!(
        put(&router, saved.etag(), &edited()).await.status,
        StatusCode::OK
    );
    let back = put(&router, saved.etag(), &initial()).await;
    assert_eq!(back.status, StatusCode::OK);
    assert_eq!(
        next_dashboard_change(&mut panel).await,
        revision_of(back.etag())
    );

    // A session that connects later starts from the current revision.
    let mut late = connect(addr).await;
    assert!(
        timeout(Duration::from_millis(200), next_dashboard_change(&mut late))
            .await
            .is_err()
    );
}
