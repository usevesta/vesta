//! Devices remember what they must across Hub restarts (their state in a
//! folder, one file per device), through the real Hub and its API: a
//! simulated charger whose hours are only read on request.

// Integration-test crate: helpers outside #[test] fns may panic too.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio::net::TcpStream;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use tokio::time::timeout;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async};
use vesta_core::{
    CapabilityState, ChargingSchedule, CommandResult, Dashboard, HubConfig, ScheduleSource,
    ServerMessage, Snapshot,
};
use vesta_hub::{ApiAuth, Hub, HubOptions, Registry, StateStorage, bind_loopback};

const HOME: &str = r#"{
  "schema_version": 1,
  "home_name": "Test Home",
  "devices": [
    { "id": "car_charger", "name": "Car Charger", "driver": "sim",
      "config": { "model": "ev_charger", "latency_ms": 5, "hours": "on_request" } },
    { "id": "desk_lamp", "name": "Desk Lamp", "driver": "sim",
      "config": { "model": "light", "latency_ms": 5 } }
  ]
}"#;

const DASHBOARD: &str = r#"{
  "schema_version": 1,
  "theme": { "preset": "glass-dark", "auto_dark_mode": true, "touch_scale": "large" },
  "kiosk": { "screensaver_timeout_sec": 120, "wake_on": [] },
  "pages": [{ "id": "home", "title": "Home", "grid_columns": 4, "widgets": [] }]
}"#;

const CHARGER: &str = "car_charger/charger";

type Ws = WebSocketStream<MaybeTlsStream<TcpStream>>;

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "vesta-hub-driver-state-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        Self(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct Running {
    addr: SocketAddr,
    stop: oneshot::Sender<()>,
    task: JoinHandle<anyhow::Result<()>>,
}

impl Running {
    async fn stop(self) {
        let _ = self.stop.send(());
        self.task.await.unwrap().unwrap();
    }
}

async fn start(state_dir: &Path) -> Running {
    let options = HubOptions {
        auth: ApiAuth::Disabled,
        driver_state: StateStorage::Dir(state_dir.to_owned()),
        ..HubOptions::default()
    };
    let hub = Hub::start(
        &HubConfig::from_json(HOME).unwrap(),
        Dashboard::from_json(DASHBOARD).unwrap(),
        &Registry::builtin(),
        options,
    )
    .await
    .unwrap();
    let listener = bind_loopback("127.0.0.1:0".parse().unwrap()).await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (stop, stopped) = oneshot::channel::<()>();
    let task = tokio::spawn(hub.serve(listener, async move {
        let _ = stopped.await;
    }));
    Running { addr, stop, task }
}

async fn recv(ws: &mut Ws) -> ServerMessage {
    loop {
        let message = timeout(Duration::from_secs(5), ws.next())
            .await
            .expect("no message within 5 s")
            .expect("connection closed")
            .unwrap();
        if let Message::Text(text) = message {
            return serde_json::from_str(&text).unwrap();
        }
    }
}

/// Connects once every device is online; returns the first snapshot.
async fn connect(addr: SocketAddr) -> (Ws, Snapshot) {
    let (mut ws, _) = connect_async(format!("ws://{addr}/api/v1/ws"))
        .await
        .unwrap();
    let ServerMessage::Snapshot(snapshot) = recv(&mut ws).await else {
        panic!("expected a snapshot first");
    };
    let mut offline = snapshot.devices.iter().filter(|d| !d.online).count();
    while offline > 0 {
        if let ServerMessage::Availability(change) = recv(&mut ws).await
            && change.online
        {
            offline -= 1;
        }
    }
    (ws, snapshot)
}

async fn command(ws: &mut Ws, request_id: &str, command: Value) -> CommandResult {
    let message = json!({ "type": "command", "request_id": request_id, "target": CHARGER,
                          "command": command, "confirmed": false });
    ws.send(Message::text(message.to_string())).await.unwrap();
    loop {
        if let ServerMessage::CommandResult(result) = recv(ws).await
            && result.request_id == request_id
        {
            return result;
        }
    }
}

/// The next charger state whose hours satisfy `wanted`.
async fn hours_until(ws: &mut Ws, wanted: impl Fn(&ChargingSchedule) -> bool) -> ChargingSchedule {
    loop {
        if let ServerMessage::State(change) = recv(ws).await
            && change.target.to_string() == CHARGER
            && let CapabilityState::EvCharger(state) = change.state
            && let Some(schedule) = state.charging_schedule
            && wanted(&schedule)
        {
            return schedule;
        }
    }
}

fn hours_in(snapshot: &Snapshot) -> Option<ChargingSchedule> {
    let device = snapshot
        .devices
        .iter()
        .find(|d| d.id.as_str() == "car_charger")?;
    let charger = device
        .endpoints
        .iter()
        .find(|e| e.id.as_str() == "charger")?;
    charger
        .capabilities
        .ev_charger
        .as_ref()?
        .state
        .as_ref()?
        .charging_schedule
        .clone()
}

fn source(schedule: &ChargingSchedule) -> Option<ScheduleSource> {
    schedule.origin.map(|origin| origin.source)
}

#[tokio::test]
async fn a_chargers_hours_survive_a_restart() {
    let dir = TempDir::new();
    let hub = start(&dir.0).await;
    let (mut ws, snapshot) = connect(hub.addr).await;
    assert_eq!(hours_in(&snapshot), None, "unknown until read or saved");

    let read = command(
        &mut ws,
        "r-1",
        json!({ "capability": "ev_charger", "action": "refresh_charging_schedule" }),
    )
    .await;
    assert_eq!(read, CommandResult::ok("r-1"));
    let read = hours_until(&mut ws, |s| source(s) == Some(ScheduleSource::Charger)).await;
    assert_eq!(read.windows[0].start.to_string(), "23:00");

    let saved = command(
        &mut ws,
        "s-1",
        json!({ "capability": "ev_charger", "action": "set_charging_schedule", "enabled": true,
                "windows": [{ "start": "22:30", "end": "06:00", "days": ["friday", "monday"] }] }),
    )
    .await;
    assert_eq!(saved, CommandResult::ok("s-1"));
    let saved = hours_until(&mut ws, |s| source(s) == Some(ScheduleSource::Saved)).await;
    drop(ws);
    hub.stop().await;

    // One file per device that kept something; no driver config in it.
    let files: Vec<String> = std::fs::read_dir(&dir.0)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(files, ["car_charger.json"]);
    let kept = std::fs::read_to_string(dir.0.join("car_charger.json")).unwrap();
    for config in ["latency_ms", "model", "on_request", "sim", "driver"] {
        assert!(!kept.contains(config), "{config} in {kept}");
    }

    // The Hub starts again: the hours are back, marked as the last known.
    let hub = start(&dir.0).await;
    let (ws, snapshot) = connect(hub.addr).await;
    let restored = hours_in(&snapshot).unwrap();
    assert_eq!(restored.windows, saved.windows);
    let origin = restored.origin.unwrap();
    assert!(origin.restored);
    assert_eq!(origin.source, ScheduleSource::Saved);
    assert_eq!(Some(origin.at), saved.origin.map(|o| o.at));
    drop(ws);
    hub.stop().await;
}

#[tokio::test]
async fn unreadable_state_never_stops_the_hub() {
    let dir = TempDir::new();
    std::fs::create_dir_all(&dir.0).unwrap();
    std::fs::write(dir.0.join("car_charger.json"), "{ half a file").unwrap();
    let hub = start(&dir.0).await;
    let (mut ws, snapshot) = connect(hub.addr).await;
    assert_eq!(hours_in(&snapshot), None);
    // The charger works, and the next read replaces the broken file.
    let read = command(
        &mut ws,
        "r-1",
        json!({ "capability": "ev_charger", "action": "refresh_charging_schedule" }),
    )
    .await;
    assert_eq!(read, CommandResult::ok("r-1"));
    let kept: Value =
        serde_json::from_str(&std::fs::read_to_string(dir.0.join("car_charger.json")).unwrap())
            .unwrap();
    assert_eq!(kept["schema_version"], 1);
    assert_eq!(kept["values"]["charging_schedule"]["source"], "charger");
    drop(ws);
    hub.stop().await;
}
