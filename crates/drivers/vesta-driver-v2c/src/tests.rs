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
    EvChargerStatus, StateChange, Weekday,
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
        cloud_timeout: Duration::from_millis(400),
        schedule_budget: Duration::from_millis(3_000),
    }
}

/// Nobody listens here: a driver built with it never reaches a cloud.
const NO_CLOUD: &str = "http://127.0.0.1:9";

fn driver_with(config: Value, scan: Scan) -> Arc<V2cDriver> {
    driver_with_cloud(config, scan, NO_CLOUD)
}

fn driver_with_cloud(config: Value, scan: Scan, cloud: &str) -> Arc<V2cDriver> {
    let config = V2cConfig::from_value(&config).unwrap();
    Arc::new(V2cDriver::build(config, fast_timing(), scan, cloud).unwrap())
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
        charger
            .execute(&charger_ep(), ev(cmd.clone()))
            .await
            .unwrap();
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
        EvChargerCommand::SetChargingSchedule {
            enabled: true,
            windows: vec![night()],
        },
    ] {
        assert!(
            matches!(
                charger.execute(&charger_ep(), ev(cmd.clone())).await,
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
        // Not a local write at all (see the cloud guard below).
        EvChargerCommand::SetChargingSchedule {
            enabled: true,
            windows: vec![night()],
        },
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
                            if let Ok(write) = charger.plan(cmd.clone()) {
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

// ---------------------------------------------------------------------------
// Charging hours, saved through the cloud (mocked: never V2C's real cloud)
// ---------------------------------------------------------------------------

const CLOUD_KEY: &str = "test-key-not-real";

fn window(start: &str, end: &str, days: &[Weekday]) -> ChargingWindow {
    ChargingWindow {
        start: start.parse().unwrap(),
        end: end.parse().unwrap(),
        days: days.to_vec(),
    }
}

fn night() -> ChargingWindow {
    use Weekday::*;
    window(
        "22:00",
        "06:00",
        &[Monday, Tuesday, Wednesday, Thursday, Friday],
    )
}

/// One request the mocked cloud received.
#[derive(Debug, Clone, PartialEq)]
struct CloudCall {
    method: String,
    path: String,
    query: Vec<(String, String)>,
    key: Option<String>,
    body: Option<Value>,
}

#[derive(Default)]
struct CloudState {
    calls: Mutex<Vec<CloudCall>>,
    /// What `/device/reported` answers (the timers, as the cloud names them).
    reported: Mutex<Value>,
    /// Timer writes change `reported`, like the real charger.
    applies: AtomicBool,
    /// Answer every request with this status instead (0: normal).
    status: AtomicUsize,
    /// Answer `/device/reported` with nothing useful.
    silent: AtomicBool,
}

struct CloudMock {
    addr: SocketAddr,
    state: Arc<CloudState>,
    task: JoinHandle<()>,
}

impl Drop for CloudMock {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl CloudMock {
    async fn start() -> Self {
        let state = Arc::new(CloudState {
            reported: Mutex::new(json!({
                "programed": 0, "daysofweek_00": "8", "daysofweek_01": "8",
                "hourstart_00": 0, "minutestart_00": 0, "hourend_00": 0, "minuteend_00": 0,
                "hourstart_01": 0, "minutestart_01": 0, "hourend_01": 0, "minuteend_01": 0
            })),
            applies: AtomicBool::new(true),
            ..CloudState::default()
        });
        let app = Router::new()
            .fallback(cloud_handler)
            .with_state(Arc::clone(&state));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        Self { addr, state, task }
    }

    fn base(&self) -> String {
        format!("http://{}/kong/v2c_service", self.addr)
    }

    fn calls(&self) -> Vec<CloudCall> {
        self.state.calls.lock().unwrap().clone()
    }

    fn set_status(&self, status: u16) {
        self.state
            .status
            .store(usize::from(status), Ordering::SeqCst);
    }
}

async fn cloud_handler(
    State(state): State<Arc<CloudState>>,
    method: axum::http::Method,
    OriginalUri(uri): OriginalUri,
    headers: axum::http::HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    let query: Vec<(String, String)> = uri
        .query()
        .unwrap_or_default()
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
        .collect();
    let path = uri
        .path()
        .trim_start_matches("/kong/v2c_service")
        .to_owned();
    let call = CloudCall {
        method: method.to_string(),
        path: path.clone(),
        query: query.clone(),
        key: headers
            .get("apikey")
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned),
        body: serde_json::from_slice(&body).ok(),
    };
    state.calls.lock().unwrap().push(call.clone());
    let forced = state.status.load(Ordering::SeqCst);
    if forced != 0 {
        return StatusCode::from_u16(u16::try_from(forced).unwrap())
            .unwrap()
            .into_response();
    }
    if call.key.as_deref() != Some(CLOUD_KEY) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    match (method.as_str(), path.as_str()) {
        ("POST", "/device/timer") => {
            let slot = query
                .iter()
                .find(|(k, _)| k == "timerId")
                .and_then(|(_, v)| v.parse::<u8>().ok());
            let (Some(slot @ 0..=1), Some(body)) = (slot, call.body) else {
                return StatusCode::BAD_REQUEST.into_response();
            };
            if state.applies.load(Ordering::SeqCst) {
                let time = |field: &str| {
                    let text = body[field].as_str().unwrap().to_owned();
                    let (h, m) = text.split_once(':').unwrap();
                    (h.parse::<u8>().unwrap(), m.parse::<u8>().unwrap())
                };
                let (hs, ms) = time("timeStart");
                let (he, me) = time("timeEnd");
                let mut reported = state.reported.lock().unwrap();
                reported[format!("hourstart_0{slot}")] = json!(hs);
                reported[format!("minutestart_0{slot}")] = json!(ms);
                reported[format!("hourend_0{slot}")] = json!(he);
                reported[format!("minuteend_0{slot}")] = json!(me);
                reported[format!("daysofweek_0{slot}")] = body["daysOfWeek"].clone();
                reported["programed"] = json!(1);
            }
            "Command executed successfully".into_response()
        }
        ("GET", "/device/reported") => {
            if state.silent.load(Ordering::SeqCst) {
                return "{}".into_response();
            }
            let body = state.reported.lock().unwrap().to_string();
            ([("content-type", "application/json")], body).into_response()
        }
        _ => StatusCode::NOT_FOUND.into_response(),
    }
}

/// A running driver with a cloud key, connected to a mocked charger and a
/// mocked cloud, past its first reading.
async fn with_cloud(config: Value) -> (Mock, CloudMock, Arc<V2cDriver>, Running) {
    let mock = Mock::start(sample()).await;
    let cloud = CloudMock::start().await;
    let mut config = config;
    config["host"] = json!(mock.host());
    if config.get("cloud_api_key").is_none() {
        config["cloud_api_key"] = json!(CLOUD_KEY);
    }
    let charger = driver_with_cloud(config, Scan::Fixed(Vec::new()), &cloud.base());
    let mut running = run(&charger);
    assert!(running.availability().await);
    running.charger().await;
    (mock, cloud, charger, running)
}

fn save(enabled: bool, windows: Vec<ChargingWindow>) -> Command {
    ev(EvChargerCommand::SetChargingSchedule { enabled, windows })
}

fn schedule_features(charger: &V2cDriver) -> Option<ChargingScheduleFeatures> {
    charger.describe().endpoints[0]
        .capabilities
        .ev_charger
        .as_ref()
        .unwrap()
        .features
        .charging_schedule
}

#[test]
fn charging_hours_are_editable_only_with_a_cloud_key() {
    let plain = driver(json!({ "host": "192.0.2.10" }));
    assert_eq!(schedule_features(&plain), None, "set in V2C's app");
    let keyed = driver(json!({ "host": "192.0.2.10", "cloud_api_key": CLOUD_KEY }));
    let features = schedule_features(&keyed).unwrap();
    assert_eq!(features.max_windows, 2);
    assert!(features.editable && features.weekdays && features.overnight);
    assert!(
        !features.reports_windows,
        "the local API does not report them"
    );
    assert!(features.saving_needs_internet);
    let watched =
        driver(json!({ "host": "192.0.2.10", "cloud_api_key": CLOUD_KEY, "read_only": true }));
    assert_eq!(schedule_features(&watched), None);
    // The key never shows.
    assert!(!format!("{keyed:?}").contains(CLOUD_KEY));
    assert!(!format!("{:?}", keyed.config()).contains(CLOUD_KEY));
}

#[tokio::test]
async fn saves_charging_hours_in_the_charger_through_the_cloud() {
    let (mock, cloud, charger, mut running) = with_cloud(json!({})).await;
    // Daily operation never touches the cloud.
    running.drain(Duration::from_millis(300)).await;
    assert!(cloud.calls().is_empty(), "{:?}", cloud.calls());

    charger
        .execute(&charger_ep(), save(true, vec![night()]))
        .await
        .unwrap();
    let calls = cloud.calls();
    let summary: Vec<(&str, &str)> = calls
        .iter()
        .map(|c| (c.method.as_str(), c.path.as_str()))
        .collect();
    assert_eq!(
        summary,
        [
            ("POST", "/device/timer"),
            ("POST", "/device/timer"),
            ("GET", "/device/reported")
        ]
    );
    for (call, timer) in calls.iter().zip(["0", "1", ""]) {
        assert_eq!(call.key.as_deref(), Some(CLOUD_KEY));
        assert!(
            call.query
                .contains(&("deviceId".to_owned(), "ABC123".to_owned()))
        );
        if call.path == "/device/timer" {
            assert!(
                call.query
                    .contains(&("timerId".to_owned(), timer.to_owned()))
            );
            // One window: the same hours in both timers.
            assert_eq!(
                call.body,
                Some(json!({ "timeStart": "22:00", "timeEnd": "06:00", "daysOfWeek": "12345" }))
            );
        }
    }
    // On or off is local.
    assert_eq!(mock.writes(), ["/write/Timer=1"]);
    let state = running.charger().await;
    assert_eq!(state.schedule_enabled, Some(true));
    assert_eq!(
        state.charging_schedule,
        Some(ChargingSchedule {
            enabled: true,
            windows: vec![night()]
        })
    );
}

#[tokio::test]
async fn two_windows_go_to_the_two_timers() {
    use Weekday::*;
    let (mock, cloud, charger, mut running) = with_cloud(json!({})).await;
    let weekend = window("10:00", "16:30", &[Sunday, Saturday]);
    charger
        .execute(&charger_ep(), save(false, vec![night(), weekend]))
        .await
        .unwrap();
    let bodies: Vec<Value> = cloud.calls().into_iter().filter_map(|c| c.body).collect();
    assert_eq!(
        bodies,
        [
            json!({ "timeStart": "22:00", "timeEnd": "06:00", "daysOfWeek": "12345" }),
            json!({ "timeStart": "10:00", "timeEnd": "16:30", "daysOfWeek": "67" }),
        ]
    );
    assert_eq!(mock.writes(), ["/write/Timer=0"]);
    let state = loop {
        let state = running.charger().await;
        if state.charging_schedule.is_some() {
            break state;
        }
    };
    let schedule = state.charging_schedule.unwrap();
    assert!(!schedule.enabled);
    assert_eq!(
        schedule.windows,
        [night(), window("10:00", "16:30", &[Saturday, Sunday])]
    );
}

#[tokio::test]
async fn cloud_errors_change_nothing_on_the_charger() {
    for status in [401, 403, 429, 500, 400] {
        let (mock, cloud, charger, _running) = with_cloud(json!({})).await;
        cloud.set_status(status);
        let result = charger
            .execute(&charger_ep(), save(true, vec![night()]))
            .await;
        let error = result.unwrap_err();
        assert!(
            matches!(error, DriverError::Protocol(_)),
            "{status}: {error:?}"
        );
        assert!(!error.to_string().contains(CLOUD_KEY), "{error}");
        assert!(!error.to_string().contains("ABC123"), "{error}");
        assert_eq!(cloud.calls().len(), 1, "{status}: stops at the first error");
        assert!(mock.writes().is_empty(), "{status}: Timer untouched");
        assert_eq!(*lock(&charger.saved_windows), None);
    }
}

#[tokio::test]
async fn a_wrong_key_is_an_error() {
    let (mock, _cloud, charger, _running) =
        with_cloud(json!({ "cloud_api_key": "someone-elses-key" })).await;
    let error = charger
        .execute(&charger_ep(), save(true, vec![night()]))
        .await
        .unwrap_err();
    assert!(error.to_string().contains("API key"), "{error}");
    assert!(mock.writes().is_empty());
}

#[tokio::test]
async fn no_internet_is_an_error_not_a_charger_offline() {
    let mock = Mock::start(sample()).await;
    let config = json!({ "host": mock.host(), "cloud_api_key": CLOUD_KEY });
    let dead = format!("http://{}", dead_host().await);
    let charger = driver_with_cloud(config, Scan::Fixed(Vec::new()), &dead);
    let mut running = run(&charger);
    running.charger().await;
    let error = charger
        .execute(&charger_ep(), save(true, vec![night()]))
        .await
        .unwrap_err();
    assert!(matches!(error, DriverError::Protocol(_)), "{error:?}");
    assert!(mock.writes().is_empty());
}

#[tokio::test]
async fn a_charger_that_did_not_take_the_hours_is_an_error() {
    let (mock, cloud, charger, _running) = with_cloud(json!({})).await;
    // The cloud says OK but the charger keeps its old (empty) timers.
    cloud.state.applies.store(false, Ordering::SeqCst);
    let error = charger
        .execute(&charger_ep(), save(true, vec![night()]))
        .await
        .unwrap_err();
    assert!(error.to_string().contains("did not take"), "{error}");
    assert!(mock.writes().is_empty());
    assert_eq!(*lock(&charger.saved_windows), None);

    // A read-back that says nothing: the write is trusted.
    cloud.state.silent.store(true, Ordering::SeqCst);
    charger
        .execute(&charger_ep(), save(true, vec![night()]))
        .await
        .unwrap();
    assert_eq!(mock.writes(), ["/write/Timer=1"]);
}

#[tokio::test]
async fn without_a_key_or_with_hours_that_do_not_fit_nothing_is_sent() {
    use Weekday::*;
    // No key: the hours are set in V2C's app.
    let (mock, charger, _running) = connected(json!({}), sample()).await;
    assert!(matches!(
        charger
            .execute(&charger_ep(), save(true, vec![night()]))
            .await,
        Err(DriverError::Unsupported)
    ));
    assert!(mock.writes().is_empty());

    let (mock, cloud, charger, _running) = with_cloud(json!({})).await;
    for windows in [
        Vec::new(),
        vec![night(), night(), night()],
        vec![window("22:00", "22:00", &[Monday])],
        vec![window("22:00", "06:00", &[])],
        vec![window("22:00", "06:00", &[Monday, Monday])],
    ] {
        assert!(
            matches!(
                charger
                    .execute(&charger_ep(), save(true, windows.clone()))
                    .await,
                Err(DriverError::Unsupported)
            ),
            "{windows:?}"
        );
    }
    assert!(cloud.calls().is_empty());
    assert!(mock.writes().is_empty());
}

#[tokio::test]
async fn a_cloud_that_never_answers_gives_up_within_the_budget() {
    let mock = Mock::start(sample()).await;
    // Accepts connections and never answers.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let silent = format!("http://{}", listener.local_addr().unwrap());
    let hold = tokio::spawn(async move {
        let mut held = Vec::new();
        while let Ok((stream, _)) = listener.accept().await {
            held.push(stream);
        }
    });
    let config = json!({ "host": mock.host(), "cloud_api_key": CLOUD_KEY });
    let charger = driver_with_cloud(config, Scan::Fixed(Vec::new()), &silent);
    let mut running = run(&charger);
    running.charger().await;
    let started = std::time::Instant::now();
    let error = charger
        .execute(&charger_ep(), save(true, vec![night()]))
        .await
        .unwrap_err();
    assert!(matches!(error, DriverError::Protocol(_)), "{error:?}");
    assert!(started.elapsed() < fast_timing().schedule_budget + Duration::from_millis(500));
    assert!(mock.writes().is_empty());
    hold.abort();
}

/// AGENTS.md rule 7 for the cloud: whatever is asked, the only requests
/// that ever reach it are timer writes and the read-back, and only for a
/// `set_charging_schedule`. Never the contracted power, the dynamic power
/// control, the current limits or anything else.
#[tokio::test]
async fn the_cloud_only_ever_gets_timers() {
    use Weekday::*;
    let mut data = sample();
    data["PauseDynamic"] = json!(1);
    data["DynamicPowerMode"] = json!(4);
    let mock = Mock::start(data).await;
    let cloud = CloudMock::start().await;
    let config = json!({
        "host": mock.host(), "cloud_api_key": CLOUD_KEY, "meters": { "solar": true }
    });
    let charger = driver_with_cloud(config, Scan::Fixed(Vec::new()), &cloud.base());
    let mut running = run(&charger);
    running.charger().await;

    let mut commands = vec![
        EvChargerCommand::Pause,
        EvChargerCommand::Resume,
        EvChargerCommand::Lock,
        EvChargerCommand::Unlock,
        EvChargerCommand::SetSchedule { enabled: true },
        EvChargerCommand::SetSchedule { enabled: false },
        EvChargerCommand::SetCurrent { amps: 12.0 },
    ];
    for mode in [
        EvChargeMode::Manual,
        EvChargeMode::Solar,
        EvChargeMode::Scheduled,
    ] {
        commands.push(EvChargerCommand::SetMode { mode });
    }
    let saves = [
        vec![night()],
        vec![window("00:00", "23:59", &Weekday::ALL)],
        vec![night(), window("13:00", "15:00", &[Saturday])],
        vec![window("06:00", "05:59", &[Sunday])],
    ];
    for windows in &saves {
        for enabled in [true, false] {
            commands.push(EvChargerCommand::SetChargingSchedule {
                enabled,
                windows: windows.clone(),
            });
        }
    }
    for cmd in commands {
        let _ = charger.execute(&charger_ep(), ev(cmd)).await;
    }
    let calls = cloud.calls();
    assert_eq!(calls.len(), saves.len() * 2 * 3, "{calls:?}");
    for call in &calls {
        match (call.method.as_str(), call.path.as_str()) {
            ("POST", "/device/timer") => {
                let body = call.body.as_ref().unwrap().as_object().unwrap();
                let mut keys: Vec<&str> = body.keys().map(String::as_str).collect();
                keys.sort_unstable();
                assert_eq!(keys, ["daysOfWeek", "timeEnd", "timeStart"]);
            }
            ("GET", "/device/reported") => assert!(call.body.is_none()),
            other => panic!("the cloud got {other:?}"),
        }
    }
    // Locally, still only the allowed writes.
    for path in mock.writes() {
        let key = path
            .trim_start_matches("/write/")
            .split('=')
            .next()
            .unwrap()
            .to_owned();
        assert!(
            ["Paused", "Locked", "Timer", "Intensity", "DynamicPowerMode"].contains(&key.as_str()),
            "{path}"
        );
    }
}
