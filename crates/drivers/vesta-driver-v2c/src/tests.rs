//! The driver against a mocked charger: a small HTTP server on loopback that
//! answers `RealTimeData` like the real one, records every write and can be
//! made slow, failing or forgetful. Nothing here talks to real hardware.

use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::Router;
use axum::extract::{OriginalUri, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use serde_json::{Value, json};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio::time::timeout;
use vesta_core::{
    AvailabilityChange, BusEvent, CapabilityState, DeviceId, EvChargeMode, EvChargerState,
    EvChargerStatus, StateChange,
};

use super::*;

const CHARGING: &str = include_str!("../fixtures/realtime-charging.json");

fn sample() -> Value {
    serde_json::from_str(CHARGING).unwrap()
}

#[derive(Default)]
struct MockState {
    data: Mutex<Value>,
    writes: Mutex<Vec<String>>,
    reads: AtomicUsize,
    delay_ms: AtomicUsize,
    failing: AtomicBool,
    /// Apply `Key=Value` writes to `data`, like the charger does.
    applies_writes: AtomicBool,
    write_reply: Mutex<String>,
}

struct Mock {
    addr: SocketAddr,
    state: Arc<MockState>,
    task: JoinHandle<()>,
}

impl Drop for Mock {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl Mock {
    async fn start(data: Value) -> Self {
        let state = Arc::new(MockState {
            data: Mutex::new(data),
            write_reply: Mutex::new("OK".to_owned()),
            applies_writes: AtomicBool::new(true),
            ..MockState::default()
        });
        let app = Router::new()
            .route("/RealTimeData", get(realtime))
            .route("/write/{rest}", get(write))
            .with_state(Arc::clone(&state));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        Self { addr, state, task }
    }

    fn host(&self) -> String {
        self.addr.to_string()
    }

    fn set(&self, key: &str, value: Value) {
        self.state.data.lock().unwrap()[key] = value;
    }

    fn writes(&self) -> Vec<String> {
        self.state.writes.lock().unwrap().clone()
    }

    fn reads(&self) -> usize {
        self.state.reads.load(Ordering::SeqCst)
    }

    fn set_delay(&self, ms: usize) {
        self.state.delay_ms.store(ms, Ordering::SeqCst);
    }

    fn set_failing(&self, failing: bool) {
        self.state.failing.store(failing, Ordering::SeqCst);
    }
}

async fn pause(state: &MockState) {
    let ms = state.delay_ms.load(Ordering::SeqCst);
    if ms > 0 {
        tokio::time::sleep(Duration::from_millis(ms as u64)).await;
    }
}

async fn realtime(State(state): State<Arc<MockState>>) -> Response {
    state.reads.fetch_add(1, Ordering::SeqCst);
    pause(&state).await;
    if state.failing.load(Ordering::SeqCst) {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    let body = state.data.lock().unwrap().to_string();
    ([("content-type", "application/json")], body).into_response()
}

async fn write(State(state): State<Arc<MockState>>, OriginalUri(uri): OriginalUri) -> Response {
    let path = uri.path().to_owned();
    state.writes.lock().unwrap().push(path.clone());
    pause(&state).await;
    if state.failing.load(Ordering::SeqCst) {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    if state.applies_writes.load(Ordering::SeqCst)
        && let Some((key, value)) = path.trim_start_matches("/write/").split_once('=')
        && let Ok(value) = value.parse::<i64>()
    {
        state.data.lock().unwrap()[key] = json!(value);
        // Like the charger: pausing stops the power.
        if key == "Paused" && value == 1 {
            state.data.lock().unwrap()["ChargePower"] = json!(0.0);
        }
    }
    state.write_reply.lock().unwrap().clone().into_response()
}

fn fast_timing() -> Timing {
    Timing {
        poll: Duration::from_millis(60),
        request_timeout: Duration::from_millis(300),
        max_backoff: Duration::from_millis(150),
        offline_after: 3,
        rediscover_after: Duration::from_millis(100),
        rescan_every: Duration::from_secs(5),
        probe_timeout: Duration::from_millis(300),
        write_spacing: Duration::from_millis(5),
        command_budget: Duration::from_millis(1_500),
    }
}

fn driver_with(config: Value, scan: Scan) -> Arc<V2cDriver> {
    let config = V2cConfig::from_value(&config).unwrap();
    Arc::new(V2cDriver::build(config, fast_timing(), scan).unwrap())
}

fn driver(config: Value) -> Arc<V2cDriver> {
    driver_with(config, Scan::Fixed(Vec::new()))
}

struct Running {
    rx: mpsc::Receiver<BusEvent>,
    task: JoinHandle<Result<(), DriverError>>,
}

impl Drop for Running {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl Running {
    async fn next(&mut self) -> BusEvent {
        timeout(Duration::from_secs(10), self.rx.recv())
            .await
            .expect("driver went quiet")
            .expect("driver stopped")
    }

    /// The next state for `endpoint`, skipping everything else.
    async fn state_of(&mut self, endpoint: &str) -> CapabilityState {
        loop {
            if let BusEvent::State(StateChange { target, state }) = self.next().await
                && target.endpoint().as_str() == endpoint
            {
                return state;
            }
        }
    }

    async fn charger(&mut self) -> EvChargerState {
        match self.state_of("charger").await {
            CapabilityState::EvCharger(state) => state,
            other => panic!("expected the charger, got {other:?}"),
        }
    }

    async fn availability(&mut self) -> bool {
        loop {
            if let BusEvent::Availability(AvailabilityChange { online, .. }) = self.next().await {
                return online;
            }
        }
    }

    /// Everything published within `wait`.
    async fn drain(&mut self, wait: Duration) -> Vec<BusEvent> {
        let mut events = Vec::new();
        while let Ok(Some(event)) = timeout(wait, self.rx.recv()).await {
            events.push(event);
        }
        events
    }
}

fn run(driver: &Arc<V2cDriver>) -> Running {
    let (tx, rx) = mpsc::channel(256);
    let sink = EventSink::new(DeviceId::new("car_charger").unwrap(), tx);
    let driver = Arc::clone(driver);
    let task = tokio::spawn(async move { driver.run(sink).await });
    Running { rx, task }
}

fn ev(cmd: EvChargerCommand) -> Command {
    Command::EvCharger(cmd)
}

fn charger_ep() -> EndpointId {
    EndpointId::new("charger").unwrap()
}

/// A port nobody listens on.
async fn dead_host() -> String {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);
    addr.to_string()
}

// ---------------------------------------------------------------------------
// Description
// ---------------------------------------------------------------------------

#[test]
fn describes_the_charger_and_the_meters_it_was_told_about() {
    let ids = |config: Value| {
        driver(config)
            .describe()
            .endpoints
            .into_iter()
            .map(|e| (e.id.as_str().to_owned(), e.endpoint_type))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        ids(json!({ "host": "192.0.2.10" })),
        [
            ("charger".to_owned(), EndpointType::EvCharger),
            ("grid".to_owned(), EndpointType::Meter),
            ("house".to_owned(), EndpointType::Meter),
        ]
    );
    assert_eq!(
        ids(json!({ "host": "192.0.2.10", "meters": { "solar": true, "battery": true } })).len(),
        5
    );
    assert_eq!(
        ids(json!({ "host": "192.0.2.10", "meters": { "grid": false, "house": false } })),
        [("charger".to_owned(), EndpointType::EvCharger)]
    );

    let endpoints =
        driver(json!({ "host": "192.0.2.10", "meters": { "solar": true, "battery": true } }))
            .describe()
            .endpoints;
    let roles: Vec<_> = endpoints[1..]
        .iter()
        .map(|e| e.capabilities.power_meter.as_ref().unwrap().features)
        .map(|f| (f.role, f.bidirectional))
        .collect();
    assert_eq!(
        roles,
        [
            (MeterRole::Grid, true),
            (MeterRole::House, false),
            (MeterRole::Solar, false),
            (MeterRole::Battery, true),
        ]
    );
}

#[test]
fn the_config_example_builds_a_charger() {
    let example: Value =
        serde_json::from_str(include_str!("../../../../config.example.json")).unwrap();
    let devices = example["devices"].as_array().unwrap();
    let chargers: Vec<_> = devices.iter().filter(|d| d["driver"] == "v2c").collect();
    assert_eq!(chargers.len(), 1);
    let driver = V2cDriver::from_config(&chargers[0]["config"]).unwrap();
    assert_eq!(driver.config().host, "192.0.2.10");
    assert_eq!(driver.describe().endpoints.len(), 3);
}

#[test]
fn features_are_honest() {
    let features = |config: Value| {
        driver(config).describe().endpoints[0]
            .capabilities
            .ev_charger
            .clone()
            .unwrap()
            .features
    };
    let plain = features(json!({ "host": "192.0.2.10" }));
    assert_eq!((plain.min_current_a, plain.max_current_a), (6.0, 32.0));
    assert!(plain.pause && plain.adjustable_current && plain.lock && plain.schedule);
    assert!(plain.modes.is_empty(), "solar modes need solar panels");

    let solar = features(json!({ "host": "192.0.2.10", "meters": { "solar": true } }));
    assert_eq!(
        solar.modes,
        [
            EvChargeMode::Manual,
            EvChargeMode::Solar,
            EvChargeMode::SolarGrid
        ]
    );

    let monitoring =
        features(json!({ "host": "192.0.2.10", "read_only": true, "meters": { "solar": true } }));
    assert!(!monitoring.pause && !monitoring.adjustable_current);
    assert!(!monitoring.lock && !monitoring.schedule);
    assert!(monitoring.modes.is_empty());
}

// ---------------------------------------------------------------------------
// Reading
// ---------------------------------------------------------------------------

#[tokio::test]
async fn reports_the_charger_and_only_publishes_changes() {
    let mock = Mock::start(sample()).await;
    let charger = driver(json!({ "host": mock.host() }));
    let mut running = run(&charger);

    assert!(running.availability().await);
    let state = running.charger().await;
    assert_eq!(state.status, EvChargerStatus::Charging);
    assert_eq!(state.power_w, 5092.0);
    assert_eq!(state.current_a, 23.0);
    match running.state_of("grid").await {
        CapabilityState::PowerMeter(grid) => {
            assert_eq!(grid.power_w, 5739.0);
            assert_eq!(grid.limit_w, Some(5700.0));
        }
        other => panic!("{other:?}"),
    }
    assert!(matches!(
        running.state_of("house").await,
        CapabilityState::PowerMeter(_)
    ));

    // Same readings: nothing new, though the charger keeps being read.
    let reads = mock.reads();
    assert!(running.drain(Duration::from_millis(250)).await.is_empty());
    assert!(mock.reads() > reads);

    // One change: only that endpoint.
    mock.set("HousePower", json!(5800.0));
    let events = running.drain(Duration::from_millis(250)).await;
    assert_eq!(events.len(), 1, "{events:?}");
    match &events[0] {
        BusEvent::State(change) => assert_eq!(change.target.endpoint().as_str(), "house"),
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn meter_errors_hide_the_meters_but_not_the_charger() {
    let mut data = sample();
    data["SlaveError"] = json!(5);
    let mock = Mock::start(data).await;
    let charger = driver(json!({ "host": mock.host() }));
    let mut running = run(&charger);
    assert!(running.availability().await);
    let events = running.drain(Duration::from_millis(250)).await;
    let endpoints: Vec<_> = events
        .iter()
        .filter_map(|event| match event {
            BusEvent::State(change) => Some(change.target.endpoint().as_str().to_owned()),
            _ => None,
        })
        .collect();
    assert_eq!(endpoints, ["charger"]);
}

#[tokio::test]
async fn goes_offline_after_failures_and_recovers_by_itself() {
    let mock = Mock::start(sample()).await;
    let charger = driver(json!({ "host": mock.host() }));
    let mut running = run(&charger);
    assert!(running.availability().await);

    // One dropped answer is not enough (the Wi-Fi extender drops now and
    // then)...
    mock.set_failing(true);
    tokio::time::sleep(Duration::from_millis(80)).await;
    mock.set_failing(false);
    let quiet = running.drain(Duration::from_millis(200)).await;
    assert!(
        quiet
            .iter()
            .all(|event| !matches!(event, BusEvent::Availability(_))),
        "{quiet:?}"
    );

    // ...three in a row are.
    mock.set_failing(true);
    assert!(!running.availability().await);
    mock.set_failing(false);
    assert!(running.availability().await);
    assert!(mock.reads() > 4);
}

#[tokio::test]
async fn slow_answers_time_out() {
    let mock = Mock::start(sample()).await;
    mock.set_delay(800);
    let charger = driver(json!({ "host": mock.host() }));
    let mut running = run(&charger);
    // Never online while every answer is too slow.
    assert!(running.drain(Duration::from_millis(1_000)).await.is_empty());
    mock.set_delay(0);
    assert!(running.availability().await);
}

#[tokio::test]
async fn a_different_charger_at_the_same_address_is_not_ours() {
    let mock = Mock::start(sample()).await;
    let charger =
        driver(json!({ "host": mock.host(), "charger_id": "OTHER9", "rediscover": false }));
    let mut running = run(&charger);
    assert!(running.drain(Duration::from_millis(400)).await.is_empty());
    assert!(mock.reads() >= 2);
    // Never controlled either.
    assert!(matches!(
        charger
            .execute(&charger_ep(), ev(EvChargerCommand::Pause))
            .await,
        Err(DriverError::Offline)
    ));
    assert!(mock.writes().is_empty());
}

#[tokio::test]
async fn stops_controlling_when_another_charger_takes_the_address() {
    let (mock, charger, mut running) = connected(json!({}), sample()).await;
    mock.set("ID", json!("XYZ789"));
    // The next reading is refused...
    let reads = mock.reads();
    while mock.reads() < reads + 2 {
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    // ...and so are commands, at once.
    assert!(matches!(
        charger
            .execute(&charger_ep(), ev(EvChargerCommand::Pause))
            .await,
        Err(DriverError::Offline)
    ));
    assert!(mock.writes().is_empty());
    assert!(!running.availability().await);
}

#[tokio::test]
async fn finds_the_charger_at_its_new_address() {
    let ours = Mock::start(sample()).await;
    let mut other = sample();
    other["ID"] = json!("XYZ789");
    let stranger = Mock::start(other).await;
    let old = dead_host().await;
    let candidates = vec![
        Address::parse(&dead_host().await).unwrap(),
        Address::parse(&stranger.host()).unwrap(),
        Address::parse(&ours.host()).unwrap(),
    ];
    let charger = driver_with(
        json!({ "host": old, "charger_id": "ABC123" }),
        Scan::Fixed(candidates),
    );
    let mut running = run(&charger);
    assert!(running.availability().await);
    assert_eq!(charger.address().to_string(), ours.host());
    assert!(stranger.reads() >= 1);
    assert_eq!(running.charger().await.status, EvChargerStatus::Charging);
    // It never wrote anything while looking.
    assert!(ours.writes().is_empty() && stranger.writes().is_empty());
}

#[tokio::test]
async fn does_not_look_elsewhere_without_knowing_the_charger() {
    let stranger = Mock::start(sample()).await;
    let charger = driver_with(
        json!({ "host": dead_host().await }),
        Scan::Fixed(vec![Address::parse(&stranger.host()).unwrap()]),
    );
    let mut running = run(&charger);
    assert!(running.drain(Duration::from_millis(500)).await.is_empty());
    assert_eq!(stranger.reads(), 0);
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

/// A running driver connected to a mock, past its first reading.
async fn connected(config: Value, data: Value) -> (Mock, Arc<V2cDriver>, Running) {
    let mock = Mock::start(data).await;
    let mut config = config;
    config["host"] = json!(mock.host());
    let charger = driver(config);
    let mut running = run(&charger);
    assert!(running.availability().await);
    running.charger().await;
    (mock, charger, running)
}

#[tokio::test]
async fn every_command_sends_exactly_its_write() {
    let mut data = sample();
    data["PauseDynamic"] = json!(1); // the charger does not set the current itself
    data["DynamicPowerMode"] = json!(4);
    let (mock, charger, _running) = connected(json!({ "meters": { "solar": true } }), data).await;
    let cases = [
        (EvChargerCommand::Pause, "/write/Paused=1"),
        (EvChargerCommand::Resume, "/write/Paused=0"),
        (EvChargerCommand::Lock, "/write/Locked=1"),
        (EvChargerCommand::Unlock, "/write/Locked=0"),
        (
            EvChargerCommand::SetSchedule { enabled: false },
            "/write/Timer=0",
        ),
        (
            EvChargerCommand::SetSchedule { enabled: true },
            "/write/Timer=1",
        ),
        (
            EvChargerCommand::SetCurrent { amps: 16.0 },
            "/write/Intensity=16",
        ),
        (
            EvChargerCommand::SetCurrent { amps: 40.0 },
            "/write/Intensity=30",
        ),
        (
            EvChargerCommand::SetCurrent { amps: 2.0 },
            "/write/Intensity=6",
        ),
        (
            EvChargerCommand::SetCurrent { amps: 9.6 },
            "/write/Intensity=10",
        ),
        (
            EvChargerCommand::SetMode {
                mode: EvChargeMode::Solar,
            },
            "/write/DynamicPowerMode=3",
        ),
        (
            EvChargerCommand::SetMode {
                mode: EvChargeMode::SolarGrid,
            },
            "/write/DynamicPowerMode=2",
        ),
        (
            EvChargerCommand::SetMode {
                mode: EvChargeMode::Manual,
            },
            "/write/DynamicPowerMode=4",
        ),
    ];
    for (i, (cmd, expected)) in cases.into_iter().enumerate() {
        charger.execute(&charger_ep(), ev(cmd)).await.unwrap();
        let writes = mock.writes();
        assert_eq!(writes.len(), i + 1, "one write per command");
        assert_eq!(writes[i], expected, "{cmd:?}");
    }
}

#[tokio::test]
async fn the_new_state_is_on_screen_before_the_command_returns() {
    let (mock, charger, mut running) = connected(json!({}), sample()).await;
    let reads = mock.reads();
    charger
        .execute(&charger_ep(), ev(EvChargerCommand::Pause))
        .await
        .unwrap();
    assert!(mock.reads() > reads, "read again right after the write");
    let state = running.charger().await;
    assert_eq!(state.status, EvChargerStatus::Paused);
    assert_eq!(state.power_w, 0.0);
}

#[tokio::test]
async fn read_only_never_sends_anything() {
    let (mock, charger, _running) = connected(json!({ "read_only": true }), sample()).await;
    for cmd in [
        EvChargerCommand::Pause,
        EvChargerCommand::Resume,
        EvChargerCommand::SetCurrent { amps: 10.0 },
        EvChargerCommand::SetMode {
            mode: EvChargeMode::Solar,
        },
        EvChargerCommand::Lock,
        EvChargerCommand::Unlock,
        EvChargerCommand::SetSchedule { enabled: false },
    ] {
        assert!(
            matches!(
                charger.execute(&charger_ep(), ev(cmd)).await,
                Err(DriverError::ReadOnly)
            ),
            "{cmd:?}"
        );
    }
    assert!(mock.writes().is_empty());
}

#[tokio::test]
async fn refuses_what_the_charger_decides_itself() {
    // The real sample: dynamic control on (it sets the current) and the
    // power profiles from its app (mode 0).
    let (mock, charger, _running) =
        connected(json!({ "meters": { "solar": true } }), sample()).await;
    assert!(matches!(
        charger
            .execute(
                &charger_ep(),
                ev(EvChargerCommand::SetCurrent { amps: 16.0 })
            )
            .await,
        Err(DriverError::Unsupported)
    ));
    assert!(matches!(
        charger
            .execute(
                &charger_ep(),
                ev(EvChargerCommand::SetMode {
                    mode: EvChargeMode::Solar
                })
            )
            .await,
        Err(DriverError::Unsupported)
    ));
    assert!(mock.writes().is_empty());
}

#[tokio::test]
async fn refuses_modes_it_cannot_offer() {
    let mut data = sample();
    data["DynamicPowerMode"] = json!(4);
    // No solar panels: no solar modes.
    let (mock, charger, _running) = connected(json!({}), data.clone()).await;
    assert!(matches!(
        charger
            .execute(
                &charger_ep(),
                ev(EvChargerCommand::SetMode {
                    mode: EvChargeMode::Solar
                })
            )
            .await,
        Err(DriverError::Unsupported)
    ));
    assert!(mock.writes().is_empty());

    let (mock, charger, _running) = connected(json!({ "meters": { "solar": true } }), data).await;
    assert!(matches!(
        charger
            .execute(
                &charger_ep(),
                ev(EvChargerCommand::SetMode {
                    mode: EvChargeMode::Scheduled
                })
            )
            .await,
        Err(DriverError::Unsupported)
    ));
    assert!(mock.writes().is_empty());
}

#[tokio::test]
async fn commands_need_a_reading_and_the_right_endpoint() {
    let charger = driver(json!({ "host": dead_host().await }));
    assert!(matches!(
        charger
            .execute(
                &charger_ep(),
                ev(EvChargerCommand::SetCurrent { amps: 10.0 })
            )
            .await,
        Err(DriverError::Offline)
    ));
    assert!(matches!(
        charger
            .execute(
                &EndpointId::new("grid").unwrap(),
                ev(EvChargerCommand::Pause)
            )
            .await,
        Err(DriverError::Unsupported)
    ));
    assert!(matches!(
        charger
            .execute(
                &EndpointId::new("garage").unwrap(),
                ev(EvChargerCommand::Pause)
            )
            .await,
        Err(DriverError::UnknownEndpoint(_))
    ));
    assert!(matches!(
        charger
            .execute(&charger_ep(), Command::OnOff(vesta_core::OnOffCommand::On))
            .await,
        Err(DriverError::Unsupported)
    ));
    // Never read: nothing is sent.
    assert!(matches!(
        charger
            .execute(&charger_ep(), ev(EvChargerCommand::Pause))
            .await,
        Err(DriverError::Offline)
    ));
}

#[tokio::test]
async fn writes_the_charger_does_not_accept_are_errors() {
    let (mock, charger, _running) = connected(json!({}), sample()).await;
    *mock.state.write_reply.lock().unwrap() = "ERROR".to_owned();
    assert!(matches!(
        charger
            .execute(&charger_ep(), ev(EvChargerCommand::Lock))
            .await,
        Err(DriverError::Protocol(_))
    ));
    *mock.state.write_reply.lock().unwrap() = "OK".to_owned();
    mock.set_failing(true);
    assert!(matches!(
        charger
            .execute(&charger_ep(), ev(EvChargerCommand::Lock))
            .await,
        Err(DriverError::Protocol(_))
    ));
    mock.set_failing(false);
    mock.set_delay(800);
    assert!(matches!(
        charger
            .execute(&charger_ep(), ev(EvChargerCommand::Lock))
            .await,
        Err(DriverError::Timeout)
    ));
}

// ---------------------------------------------------------------------------
// AGENTS.md rule 7: the charger's safety settings are never written
// ---------------------------------------------------------------------------

fn reading(changes: &[(&str, Value)]) -> Reading {
    let mut data = sample();
    for (key, value) in changes {
        data[*key] = value.clone();
    }
    Reading::parse(data.to_string().as_bytes()).unwrap()
}

#[test]
fn no_command_can_touch_the_chargers_safety_settings() {
    let allowed = |path: &str| -> bool {
        let Some((key, value)) = path
            .strip_prefix("/write/")
            .and_then(|kv| kv.split_once('='))
        else {
            return false;
        };
        let Ok(value) = value.parse::<i64>() else {
            return false;
        };
        match key {
            "Paused" | "Locked" | "Timer" => (0..=1).contains(&value),
            "Intensity" => (6..=32).contains(&value),
            "DynamicPowerMode" => (2..=4).contains(&value),
            _ => false,
        }
    };

    let mut commands = vec![
        EvChargerCommand::Pause,
        EvChargerCommand::Resume,
        EvChargerCommand::Lock,
        EvChargerCommand::Unlock,
        EvChargerCommand::SetSchedule { enabled: true },
        EvChargerCommand::SetSchedule { enabled: false },
    ];
    for mode in [
        EvChargeMode::Manual,
        EvChargeMode::Solar,
        EvChargeMode::SolarGrid,
        EvChargeMode::Scheduled,
    ] {
        commands.push(EvChargerCommand::SetMode { mode });
    }
    for tenth in -500..=1_000 {
        commands.push(EvChargerCommand::SetCurrent {
            amps: f64::from(tenth) / 10.0,
        });
    }
    for amps in [
        f64::NAN,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::MAX,
        f64::MIN,
    ] {
        commands.push(EvChargerCommand::SetCurrent { amps });
    }

    let mut sent = 0;
    for solar in [false, true] {
        let charger = driver(json!({ "host": "192.0.2.10", "meters": { "solar": solar } }));
        for dynamic in [0, 1] {
            for pause_dynamic in [0, 1] {
                for mode in -1..=8 {
                    for (min, max) in [(6, 30), (0, 99), (20, 10), (-1, -1)] {
                        *lock(&charger.last) = Some(reading(&[
                            ("Dynamic", json!(dynamic)),
                            ("PauseDynamic", json!(pause_dynamic)),
                            ("DynamicPowerMode", json!(mode)),
                            ("MinIntensity", json!(min)),
                            ("MaxIntensity", json!(max)),
                        ]));
                        for cmd in &commands {
                            if let Ok(write) = charger.plan(*cmd) {
                                let path = write.path();
                                assert!(allowed(&path), "{cmd:?} → {path}");
                                sent += 1;
                            }
                        }
                    }
                }
            }
        }
    }
    assert!(sent > 10_000, "the guard really ran ({sent} writes)");
}

#[test]
fn the_current_is_only_set_when_the_charger_lets_it() {
    let charger = driver(json!({ "host": "192.0.2.10" }));
    let set = |amps| EvChargerCommand::SetCurrent { amps };
    // Dynamic control modulating: the charger decides.
    *lock(&charger.last) = Some(reading(&[]));
    assert!(matches!(
        charger.plan(set(16.0)),
        Err(DriverError::Unsupported)
    ));
    // Modulation paused (by the household, in the charger's app) or no
    // dynamic control: the setpoint is ours, within its limits.
    *lock(&charger.last) = Some(reading(&[("PauseDynamic", json!(1))]));
    assert_eq!(charger.plan(set(31.0)).unwrap(), Write::Intensity(30));
    *lock(&charger.last) = Some(reading(&[("Dynamic", json!(0))]));
    assert_eq!(charger.plan(set(12.0)).unwrap(), Write::Intensity(12));
}
