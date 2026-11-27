//! End-to-end tests: a real Hub on an ephemeral loopback port, driven over
//! HTTP and WebSocket like the frontend does.

// Integration-test crate: helpers outside #[test] fns may panic too.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::BTreeSet;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio::net::TcpStream;
use tokio::sync::{Notify, oneshot};
use tokio::time::timeout;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::{self, Message};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async};
use tower::ServiceExt;
use vesta_core::{
    Capabilities, CapabilityKind, Command, CommandError, CommandResult, Dashboard,
    DeviceDescriptor, DeviceDriver, DriverError, Endpoint, EndpointId, EndpointType, EventSink,
    HubConfig, OnOffCapability, OnOffState, RejectReason, ServerMessage, Snapshot, StateChange,
    Target, async_trait,
};
use vesta_hub::{ApiAuth, Hub, HubOptions, Registry, bind_loopback};

const HOME: &str = r#"{
  "schema_version": 1,
  "home_name": "Test Home",
  "devices": [
    { "id": "living_room_light", "name": "Living Room Light", "room": "living_room",
      "driver": "sim", "config": { "model": "light", "latency_ms": 10 } },
    { "id": "living_room_fan", "name": "Living Room Fan", "room": "living_room",
      "driver": "sim", "config": { "model": "ceiling_fan", "latency_ms": 10 },
      "endpoints": { "light": { "name": "Fan light" } } },
    { "id": "bedroom_blind", "name": "Bedroom Blind", "room": "bedroom",
      "driver": "sim", "config": { "model": "blind", "latency_ms": 10 } },
    { "id": "garage_door", "name": "Garage Door", "room": "garage",
      "driver": "sim", "config": { "model": "garage_door", "latency_ms": 10 } },
    { "id": "front_door_strike", "name": "Front Door", "room": "entrance",
      "driver": "sim", "config": { "model": "door_strike", "latency_ms": 10 } },
    { "id": "front_doorbell", "name": "Front Doorbell", "room": "entrance",
      "driver": "sim", "config": { "model": "doorbell" } }
  ]
}"#;

const DASHBOARD: &str = r#"{
  "schema_version": 1,
  "theme": { "preset": "glass-dark", "auto_dark_mode": true, "touch_scale": "large" },
  "kiosk": {
    "screensaver_timeout_sec": 120,
    "wake_on": [{ "target": "front_doorbell/main", "event": "doorbell.pressed" }]
  },
  "pages": [{
    "id": "home", "title": "Home", "grid_columns": 4,
    "widgets": [
      { "type": "light-control", "target": "living_room_light/main", "col_span": 2, "row_span": 1 },
      { "type": "door-button", "target": "front_door_strike/main", "col_span": 2, "row_span": 1 }
    ]
  }]
}"#;

type Ws = WebSocketStream<MaybeTlsStream<TcpStream>>;

fn target(raw: &str) -> Target {
    raw.parse().unwrap()
}

/// These tests cover the device API; sign-in is covered in `session.rs`.
async fn hub(config: &str, registry: Registry, options: HubOptions) -> Hub {
    let options = HubOptions {
        auth: ApiAuth::Disabled,
        ..options
    };
    Hub::start(
        &HubConfig::from_json(config).unwrap(),
        Dashboard::from_json(DASHBOARD).unwrap(),
        &registry,
        options,
    )
    .await
    .unwrap()
}

/// Serves a Hub on an ephemeral port until the test ends.
async fn serve(hub: Hub) -> SocketAddr {
    let listener = bind_loopback("127.0.0.1:0".parse().unwrap()).await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(hub.serve(listener, std::future::pending()));
    addr
}

async fn start(config: &str, registry: Registry, options: HubOptions) -> SocketAddr {
    serve(hub(config, registry, options).await).await
}

async fn start_home() -> SocketAddr {
    start(HOME, Registry::builtin(), HubOptions::default()).await
}

async fn connect(addr: SocketAddr) -> Ws {
    let (ws, _) = connect_async(format!("ws://{addr}/api/v1/ws"))
        .await
        .unwrap();
    ws
}

async fn recv_text(ws: &mut Ws) -> String {
    loop {
        let message = timeout(Duration::from_secs(5), ws.next())
            .await
            .expect("no message within 5 s")
            .expect("connection closed")
            .unwrap();
        match message {
            Message::Text(text) => return text.to_string(),
            Message::Ping(_) | Message::Pong(_) => {}
            other => panic!("unexpected frame {other:?}"),
        }
    }
}

async fn recv(ws: &mut Ws) -> ServerMessage {
    serde_json::from_str(&recv_text(ws).await).unwrap()
}

async fn snapshot(ws: &mut Ws) -> Snapshot {
    match recv(ws).await {
        ServerMessage::Snapshot(snapshot) => snapshot,
        other => panic!("expected a snapshot first, got {other:?}"),
    }
}

/// Connects and waits until every device in the snapshot is online, so
/// tests never race the drivers' start-up.
async fn connect_online(addr: SocketAddr) -> (Ws, Snapshot) {
    let mut ws = connect(addr).await;
    let snapshot = snapshot(&mut ws).await;
    let mut offline: BTreeSet<String> = snapshot
        .devices
        .iter()
        .filter(|device| !device.online)
        .map(|device| device.id.to_string())
        .collect();
    while !offline.is_empty() {
        if let ServerMessage::Availability(change) = recv(&mut ws).await
            && change.online
        {
            offline.remove(change.device_id.as_str());
        }
    }
    (ws, snapshot)
}

async fn send_json(ws: &mut Ws, value: Value) {
    ws.send(Message::text(value.to_string())).await.unwrap();
}

async fn send_command(
    ws: &mut Ws,
    request_id: &str,
    target: &str,
    command: Value,
    confirmed: bool,
) {
    send_json(
        ws,
        json!({
            "type": "command",
            "request_id": request_id,
            "target": target,
            "command": command,
            "confirmed": confirmed,
        }),
    )
    .await;
}

/// Skips unrelated messages until the result for `request_id` arrives.
async fn result_for(ws: &mut Ws, request_id: &str) -> CommandResult {
    loop {
        if let ServerMessage::CommandResult(result) = recv(ws).await
            && result.request_id == request_id
        {
            return result;
        }
    }
}

async fn command(
    ws: &mut Ws,
    request_id: &str,
    target: &str,
    command: Value,
    confirmed: bool,
) -> CommandResult {
    send_command(ws, request_id, target, command, confirmed).await;
    result_for(ws, request_id).await
}

async fn get(router: &Router, path: &str, host: &str, origin: Option<&str>) -> (StatusCode, Value) {
    let mut request = Request::builder().uri(path).header("host", host);
    if let Some(origin) = origin {
        request = request.header("origin", origin);
    }
    let response = router
        .clone()
        .oneshot(request.body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
        .await
        .unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

fn rejected(reason: RejectReason) -> Option<CommandError> {
    Some(CommandError::Rejected { reason })
}

// ---------------------------------------------------------------------------
// HTTP
// ---------------------------------------------------------------------------

#[tokio::test]
async fn http_endpoints_serve_health_devices_and_dashboard() {
    let hub = hub(HOME, Registry::builtin(), HubOptions::default()).await;
    let router = hub.router();
    let local = "127.0.0.1:8080";

    let (status, health) = get(&router, "/api/v1/health", local, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(health, json!({ "status": "ok" }));

    let (status, devices) = get(&router, "/api/v1/devices", local, None).await;
    assert_eq!(status, StatusCode::OK);
    let snapshot: Snapshot = serde_json::from_value(devices).unwrap();
    let ids: Vec<_> = snapshot.devices.iter().map(|d| d.id.as_str()).collect();
    assert_eq!(
        ids,
        [
            "living_room_light",
            "living_room_fan",
            "bedroom_blind",
            "garage_door",
            "front_door_strike",
            "front_doorbell"
        ],
        "config order"
    );
    let fan = &snapshot.devices[1];
    assert_eq!(
        fan.endpoints[1].name, "Fan light",
        "endpoint override applied"
    );
    assert_eq!(
        snapshot.devices[0].endpoints[0].name, "Living Room Light",
        "single endpoint takes the device name"
    );

    let (status, dashboard) = get(&router, "/api/v1/dashboard", local, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        serde_json::from_value::<Dashboard>(dashboard).unwrap(),
        Dashboard::from_json(DASHBOARD).unwrap()
    );

    let (status, _) = get(&router, "/api/v1/nope", local, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn only_local_hosts_and_origins_are_served() {
    let hub = hub(HOME, Registry::builtin(), HubOptions::default()).await;
    let router = hub.router();

    for host in ["localhost:8080", "[::1]:8080", "127.0.0.1"] {
        let (status, _) = get(&router, "/api/v1/health", host, None).await;
        assert_eq!(status, StatusCode::OK, "{host}");
    }
    // DNS rebinding: a foreign name that resolves to 127.0.0.1.
    for host in ["attacker.example:8080", "192.0.2.10:8080"] {
        let (status, _) = get(&router, "/api/v1/devices", host, None).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{host}");
    }
    let (status, _) = get(
        &router,
        "/api/v1/devices",
        "127.0.0.1:8080",
        Some("http://localhost:5173"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = get(
        &router,
        "/api/v1/devices",
        "127.0.0.1:8080",
        Some("https://attacker.example"),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn websocket_from_a_foreign_origin_is_refused() {
    let addr = start_home().await;

    let mut request = format!("ws://{addr}/api/v1/ws")
        .into_client_request()
        .unwrap();
    request
        .headers_mut()
        .insert("origin", "https://attacker.example".parse().unwrap());
    match connect_async(request).await {
        Err(tungstenite::Error::Http(response)) => {
            assert_eq!(response.status(), StatusCode::FORBIDDEN);
        }
        other => panic!("expected 403, got {other:?}"),
    }

    let mut request = format!("ws://{addr}/api/v1/ws")
        .into_client_request()
        .unwrap();
    request
        .headers_mut()
        .insert("origin", "http://localhost:5173".parse().unwrap());
    let (mut ws, _) = connect_async(request).await.unwrap();
    snapshot(&mut ws).await;
}

// ---------------------------------------------------------------------------
// WebSocket
// ---------------------------------------------------------------------------

#[tokio::test]
async fn command_produces_state_and_result() {
    let addr = start_home().await;
    let (mut ws, snapshot) = connect_online(addr).await;
    assert_eq!(snapshot.devices.len(), 6);

    send_command(
        &mut ws,
        "c-1",
        "living_room_light/main",
        json!({ "capability": "on_off", "action": "on" }),
        false,
    )
    .await;

    // Order between the two is not guaranteed.
    let (mut result, mut state) = (None, None);
    while result.is_none() || state.is_none() {
        match recv(&mut ws).await {
            ServerMessage::CommandResult(r) if r.request_id == "c-1" => result = Some(r),
            ServerMessage::State(change) if change.target == target("living_room_light/main") => {
                state = Some(change);
            }
            _ => {}
        }
    }
    assert_eq!(result, Some(CommandResult::ok("c-1")));
    assert_eq!(
        state,
        Some(StateChange {
            target: target("living_room_light/main"),
            state: OnOffState { on: true }.into(),
        })
    );

    // The HTTP snapshot reflects it too.
    let fresh = http_get(addr, "/api/v1/devices").await;
    assert_eq!(
        fresh["devices"][0]["endpoints"][0]["capabilities"]["on_off"]["state"]["on"],
        json!(true)
    );
}

/// Minimal HTTP/1.1 GET over a real socket (no client library needed).
async fn http_get(addr: SocketAddr, path: &str) -> Value {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut stream = TcpStream::connect(addr).await.unwrap();
    stream
        .write_all(
            format!("GET {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n").as_bytes(),
        )
        .await
        .unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).await.unwrap();
    let (head, body) = response.split_once("\r\n\r\n").unwrap();
    assert!(head.starts_with("HTTP/1.1 200"), "{head}");
    serde_json::from_str(body).unwrap()
}

#[tokio::test]
async fn sensitive_commands_require_confirmation() {
    let addr = start_home().await;
    let (mut ws, _) = connect_online(addr).await;
    let fire = || json!({ "capability": "trigger", "action": "fire" });
    let open = || json!({ "capability": "cover", "action": "open" });

    send_command(&mut ws, "s-1", "front_door_strike/main", fire(), false).await;
    let raw: Value = loop {
        let value: Value = serde_json::from_str(&recv_text(&mut ws).await).unwrap();
        if value["type"] == "command_result" && value["request_id"] == "s-1" {
            break value;
        }
    };
    assert_eq!(
        raw,
        json!({
            "type": "command_result",
            "request_id": "s-1",
            "ok": false,
            "error": { "code": "rejected", "reason": "confirmation_required" }
        })
    );

    let confirmed = command(&mut ws, "s-2", "front_door_strike/main", fire(), true).await;
    assert_eq!(confirmed, CommandResult::ok("s-2"));

    // Opening a garage door guards access to the home; a blind does not.
    let garage = command(&mut ws, "s-3", "garage_door/main", open(), false).await;
    assert_eq!(garage.error, rejected(RejectReason::ConfirmationRequired));
    let garage = command(&mut ws, "s-4", "garage_door/main", open(), true).await;
    assert!(garage.ok);
    let blind = command(&mut ws, "s-5", "bedroom_blind/main", open(), false).await;
    assert!(blind.ok);
    // Closing is never sensitive.
    let close = json!({ "capability": "cover", "action": "close" });
    let garage = command(&mut ws, "s-6", "garage_door/main", close, false).await;
    assert!(garage.ok);
}

#[tokio::test]
async fn unknown_targets_and_unsupported_commands_are_rejected() {
    let addr = start_home().await;
    let (mut ws, _) = connect_online(addr).await;
    let on = || json!({ "capability": "on_off", "action": "on" });

    for (id, target, cmd) in [
        ("u-1", "attic_light/main", on()),
        ("u-2", "living_room_light/fan", on()),
        (
            "u-3",
            "living_room_light/main",
            json!({ "capability": "cover", "action": "open" }),
        ),
        (
            "u-4",
            "living_room_fan/fan",
            json!({ "capability": "fan", "action": "set_speed", "speed": 7 }),
        ),
        (
            "u-5",
            "garage_door/main",
            json!({ "capability": "cover", "action": "stop" }),
        ),
        // Invalid on the wire (level out of range, bad target): still
        // answered because the request_id is usable.
        (
            "u-6",
            "living_room_light/main",
            json!({ "capability": "level", "action": "set", "level": 150 }),
        ),
        ("u-7", "Living Room/Light", on()),
    ] {
        let result = command(&mut ws, id, target, cmd, true).await;
        assert_eq!(
            result,
            CommandResult::failed(id, CommandError::Unsupported),
            "{target}"
        );
    }
}

#[tokio::test]
async fn malformed_messages_are_dropped_and_the_connection_stays_open() {
    let addr = start_home().await;
    let (mut ws, _) = connect_online(addr).await;

    ws.send(Message::text("this is not json")).await.unwrap();
    ws.send(Message::binary(vec![1, 2, 3])).await.unwrap();
    send_json(
        &mut ws,
        json!({ "type": "command", "request_id": "x".repeat(65) }),
    )
    .await;
    send_json(&mut ws, json!({ "type": "subscribe" })).await;

    let result = command(
        &mut ws,
        "after-garbage",
        "living_room_fan/light",
        json!({ "capability": "level", "action": "set", "level": 30 }),
        false,
    )
    .await;
    assert_eq!(result, CommandResult::ok("after-garbage"));
}

#[tokio::test]
async fn slow_devices_time_out() {
    let home = r#"{ "schema_version": 1, "home_name": "Test", "devices": [
        { "id": "slow_light", "name": "Slow", "driver": "sim",
          "config": { "model": "light", "latency_ms": 2000 } } ] }"#;
    let options = HubOptions {
        command_timeout: Duration::from_millis(200),
        ..HubOptions::default()
    };
    let addr = start(home, Registry::builtin(), options).await;
    let (mut ws, _) = connect_online(addr).await;
    let result = command(
        &mut ws,
        "t-1",
        "slow_light/main",
        json!({ "capability": "on_off", "action": "toggle" }),
        false,
    )
    .await;
    assert_eq!(result, CommandResult::failed("t-1", CommandError::Timeout));
}

#[tokio::test]
async fn failing_devices_report_a_device_error() {
    let home = r#"{ "schema_version": 1, "home_name": "Test", "devices": [
        { "id": "flaky_light", "name": "Flaky", "driver": "sim",
          "config": { "model": "light", "latency_ms": 0, "failure_rate": 1.0 } } ] }"#;
    let addr = start(home, Registry::builtin(), HubOptions::default()).await;
    let (mut ws, _) = connect_online(addr).await;
    let result = command(
        &mut ws,
        "f-1",
        "flaky_light/main",
        json!({ "capability": "on_off", "action": "on" }),
        false,
    )
    .await;
    // The driver's message stays on the Hub.
    assert_eq!(
        result,
        CommandResult::failed("f-1", CommandError::Device { message: None })
    );
}

// ---------------------------------------------------------------------------
// Test drivers
// ---------------------------------------------------------------------------

fn switch_endpoint() -> Endpoint {
    Endpoint {
        id: EndpointId::main(),
        name: "Switch".to_owned(),
        endpoint_type: EndpointType::Switch,
        room: None,
        capabilities: Capabilities {
            on_off: Some(OnOffCapability {
                state: Some(OnOffState { on: false }),
            }),
            ..Capabilities::default()
        },
    }
}

/// Never reports itself online.
struct Unreachable;

#[async_trait]
impl DeviceDriver for Unreachable {
    fn describe(&self) -> DeviceDescriptor {
        DeviceDescriptor {
            endpoints: vec![switch_endpoint()],
        }
    }
    async fn execute(&self, _: &EndpointId, _: Command) -> Result<(), DriverError> {
        panic!("the hub must not call an offline device");
    }
    async fn run(&self, _: EventSink) -> Result<(), DriverError> {
        std::future::pending().await
    }
}

/// Online, but every command fails with a message full of secrets.
struct Leaky;

const SECRETS: [&str; 4] = [
    "example_cloud_bridge",
    "192.0.2.77",
    "hunter2-not-for-clients",
    "AA:BB:CC:DD:EE:FF",
];

#[async_trait]
impl DeviceDriver for Leaky {
    fn describe(&self) -> DeviceDescriptor {
        DeviceDescriptor {
            endpoints: vec![switch_endpoint()],
        }
    }
    async fn execute(&self, _: &EndpointId, _: Command) -> Result<(), DriverError> {
        Err(DriverError::Protocol(format!(
            "example_cloud_bridge at 192.0.2.77 rejected password hunter2-not-for-clients ({})",
            SECRETS[3]
        )))
    }
    async fn run(&self, sink: EventSink) -> Result<(), DriverError> {
        sink.availability(true).await?;
        std::future::pending().await
    }
}

/// Emits a burst of state updates each time it is poked.
struct Burst(Arc<Notify>);

#[async_trait]
impl DeviceDriver for Burst {
    fn describe(&self) -> DeviceDescriptor {
        DeviceDescriptor {
            endpoints: vec![switch_endpoint()],
        }
    }
    async fn execute(&self, _: &EndpointId, _: Command) -> Result<(), DriverError> {
        Ok(())
    }
    async fn run(&self, sink: EventSink) -> Result<(), DriverError> {
        sink.availability(true).await?;
        loop {
            self.0.notified().await;
            for i in 0..100 {
                sink.state(&EndpointId::main(), OnOffState { on: i % 2 == 0 })
                    .await?;
            }
        }
    }
}

#[tokio::test]
async fn offline_devices_reject_commands() {
    let home = r#"{ "schema_version": 1, "home_name": "Test", "devices": [
        { "id": "porch_switch", "name": "Porch", "driver": "unreachable" } ] }"#;
    let mut registry = Registry::empty();
    registry.register("unreachable", |_| {
        Ok(Arc::new(Unreachable) as Arc<dyn DeviceDriver>)
    });
    let addr = start(home, registry, HubOptions::default()).await;
    let mut ws = connect(addr).await;
    let snapshot = snapshot(&mut ws).await;
    assert!(!snapshot.devices[0].online);

    let result = command(
        &mut ws,
        "o-1",
        "porch_switch/main",
        json!({ "capability": "on_off", "action": "on" }),
        false,
    )
    .await;
    assert_eq!(result, CommandResult::failed("o-1", CommandError::Offline));
}

fn collect_keys(value: &Value, keys: &mut BTreeSet<String>) {
    match value {
        Value::Object(map) => {
            for (key, value) in map {
                keys.insert(key.clone());
                collect_keys(value, keys);
            }
        }
        Value::Array(items) => items.iter().for_each(|item| collect_keys(item, keys)),
        _ => {}
    }
}

fn assert_no_driver_details(context: &str, json: &str) {
    for secret in SECRETS {
        assert!(!json.contains(secret), "{context} leaks {secret}: {json}");
    }
    let mut keys = BTreeSet::new();
    collect_keys(&serde_json::from_str(json).unwrap(), &mut keys);
    for forbidden in [
        "driver",
        "config",
        "host",
        "password",
        "latency_ms",
        "model",
    ] {
        assert!(
            !keys.contains(forbidden),
            "{context} exposes `{forbidden}`: {json}"
        );
    }
}

#[tokio::test]
async fn api_never_exposes_driver_names_or_config() {
    let home = r#"{ "schema_version": 1, "home_name": "Test", "devices": [
        { "id": "porch_switch", "name": "Porch", "driver": "example_cloud_bridge",
          "config": { "host": "192.0.2.77", "password": "hunter2-not-for-clients",
                      "mac": "AA:BB:CC:DD:EE:FF" } },
        { "id": "living_room_light", "name": "Living Room Light", "driver": "sim",
          "config": { "model": "light", "latency_ms": 0 } } ] }"#;
    let mut registry = Registry::builtin();
    registry.register("example_cloud_bridge", |_| {
        Ok(Arc::new(Leaky) as Arc<dyn DeviceDriver>)
    });
    let hub = hub(home, registry, HubOptions::default()).await;
    let router = hub.router();
    let addr = serve(hub).await;

    for path in ["/api/v1/devices", "/api/v1/dashboard", "/api/v1/health"] {
        let (status, body) = get(&router, path, "127.0.0.1:8080", None).await;
        assert_eq!(status, StatusCode::OK);
        assert_no_driver_details(path, &body.to_string());
    }

    let mut ws = connect(addr).await;
    let first = recv_text(&mut ws).await;
    assert!(first.contains(r#""type":"snapshot""#));
    assert_no_driver_details("ws snapshot", &first);

    send_command(
        &mut ws,
        "l-1",
        "porch_switch/main",
        json!({ "capability": "on_off", "action": "on" }),
        false,
    )
    .await;
    let mut seen = Vec::new();
    loop {
        let text = recv_text(&mut ws).await;
        assert_no_driver_details("ws message", &text);
        let done = text.contains(r#""request_id":"l-1""#);
        seen.push(text);
        if done {
            break;
        }
    }
    assert_eq!(
        serde_json::from_str::<Value>(seen.last().unwrap()).unwrap(),
        json!({ "type": "command_result", "request_id": "l-1", "ok": false,
                "error": { "code": "device" } })
    );
}

#[tokio::test]
async fn lagging_clients_get_a_fresh_snapshot() {
    let home = r#"{ "schema_version": 1, "home_name": "Test", "devices": [
        { "id": "noisy_switch", "name": "Noisy", "driver": "burst" } ] }"#;
    let poke = Arc::new(Notify::new());
    let mut registry = Registry::empty();
    let driver_poke = Arc::clone(&poke);
    registry.register("burst", move |_| {
        Ok(Arc::new(Burst(Arc::clone(&driver_poke))) as Arc<dyn DeviceDriver>)
    });
    let options = HubOptions {
        bus_capacity: 1,
        ..HubOptions::default()
    };
    let addr = start(home, registry, options).await;
    let (mut ws, _) = connect_online(addr).await;

    poke.notify_one();
    for _ in 0..200 {
        match recv(&mut ws).await {
            ServerMessage::Snapshot(snapshot) => {
                let switch = &snapshot.devices[0].endpoints[0];
                assert!(switch.capabilities.has(CapabilityKind::OnOff));
                return;
            }
            ServerMessage::State(_) => {}
            other => panic!("unexpected {other:?}"),
        }
    }
    panic!("no resync snapshot after lagging");
}

#[tokio::test]
async fn shutdown_closes_sessions_and_stops_the_server() {
    let hub = hub(HOME, Registry::builtin(), HubOptions::default()).await;
    let listener = bind_loopback("127.0.0.1:0".parse().unwrap()).await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (stop, stopped) = oneshot::channel::<()>();
    let server = tokio::spawn(hub.serve(listener, async move {
        let _ = stopped.await;
    }));

    let mut ws = connect(addr).await;
    snapshot(&mut ws).await;
    stop.send(()).unwrap();

    let closed = timeout(Duration::from_secs(5), async {
        loop {
            match ws.next().await {
                None | Some(Err(_)) | Some(Ok(Message::Close(_))) => break,
                Some(Ok(_)) => {}
            }
        }
    })
    .await;
    assert!(closed.is_ok(), "session was not closed on shutdown");
    let served = timeout(Duration::from_secs(5), server).await;
    assert!(matches!(served, Ok(Ok(Ok(())))), "{served:?}");
}
