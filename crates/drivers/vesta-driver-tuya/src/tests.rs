//! The driver against a mocked Tuya device: a TCP server on loopback that
//! speaks protocol 3.3, 3.4 and 3.5 (key negotiation included), answers
//! status queries and heartbeats, records every command, applies it and
//! reports the change, and can push updates, drop sessions, refuse
//! connections or stay silent. Nothing here talks to real hardware.

use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{Map, Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream, UdpSocket};
use tokio::sync::{broadcast, mpsc};
use tokio::task::JoinHandle;
use tokio::time::timeout;
use vesta_core::{
    AvailabilityChange, BatteryState, BusEvent, Capabilities, CapabilityState, DeviceId,
    EndpointType, OnOffCommand, OnOffState, Percent, PowerMeterState, RobotVacuumCommand,
    RobotVacuumState, RobotVacuumStatus, StateChange,
};

use super::*;
use crate::frame::Check;
use crate::protocol::{Codec, Side};

const DEVICE_ID: &str = "bf00000000000000000000";
const KEY: &crypto::Key = b"0123456789abcdef";
const DEVICE_NONCE: [u8; 16] = *b"0123456789ABCDEF";

fn plug_mapping() -> Value {
    json!({
        "1": { "code": "switch_1", "type": "Boolean", "values": {} },
        "9": { "code": "countdown_1", "type": "Integer", "values": { "unit": "s", "min": 0, "max": 86400, "scale": 0, "step": 1 } },
        "20": { "code": "add_ele", "type": "Integer", "values": { "unit": "kW·h", "min": 0, "max": 50000, "scale": 3, "step": 100 } },
        "21": { "code": "cur_current", "type": "Integer", "values": { "unit": "mA", "min": 0, "max": 30000, "scale": 0, "step": 1 } },
        "22": { "code": "cur_power", "type": "Integer", "values": { "unit": "W", "min": 0, "max": 50000, "scale": 1, "step": 1 } },
        "23": { "code": "cur_voltage", "type": "Integer", "values": { "unit": "V", "min": 0, "max": 5000, "scale": 1, "step": 1 } },
        "38": { "code": "relay_status", "type": "Enum", "values": { "range": ["0", "1", "2"] } }
    })
}

fn plug_dps() -> Value {
    json!({ "1": true, "9": 0, "20": 2, "21": 112, "22": 125, "23": 2272, "38": "memory" })
}

fn vacuum_mapping() -> Value {
    json!({
        "1": { "code": "power_go", "type": "Boolean", "values": {} },
        "2": { "code": "pause", "type": "Boolean", "values": {} },
        "3": { "code": "switch_charge", "type": "Boolean", "values": {} },
        "4": { "code": "mode", "type": "Enum", "values": { "range": ["smart", "zone", "pose"] } },
        "5": { "code": "status", "type": "Enum", "values": { "range": ["standby", "cleaning", "paused", "goto_charge", "charging", "charge_done"] } },
        "16": { "code": "clean_time", "type": "Integer", "values": { "unit": "min", "scale": 0 } },
        "17": { "code": "clean_area", "type": "Integer", "values": { "unit": "㎡", "scale": 0 } },
        "23": { "code": "electricity_left", "type": "Integer", "values": { "unit": "%", "scale": 0 } }
    })
}

fn vacuum_dps() -> Value {
    json!({ "1": false, "2": false, "3": false, "4": "smart", "5": "charge_done", "16": 30, "17": 22, "23": 100, "18": 104 })
}

// ---------------------------------------------------------------------------
// The mocked device
// ---------------------------------------------------------------------------

struct MockState {
    version: Version,
    device_id: String,
    dps: Mutex<Map<String, Value>>,
    /// Every command that sets data points: (cmd, full JSON payload).
    controls: Mutex<Vec<(u32, Value)>>,
    queries: AtomicUsize,
    heartbeats: AtomicUsize,
    handshakes: AtomicUsize,
    connections: AtomicUsize,
    /// Close new connections at once.
    refusing: AtomicBool,
    /// Never answer commands.
    ignoring_commands: AtomicBool,
    /// Answer a 3.3 status query with "data unvalid" (needs the listing
    /// query).
    wants_listing: AtomicBool,
    /// Claim another device ID in answers (3.3).
    impostor: AtomicBool,
    push: broadcast::Sender<Map<String, Value>>,
    kick: broadcast::Sender<()>,
}

struct Mock {
    addr: SocketAddr,
    state: Arc<MockState>,
    task: JoinHandle<()>,
}

impl Drop for Mock {
    fn drop(&mut self) {
        self.task.abort();
        let _ = self.state.kick.send(());
    }
}

impl Mock {
    async fn start(version: Version, dps: Value) -> Self {
        let Value::Object(dps) = dps else {
            panic!("dps must be an object");
        };
        let state = Arc::new(MockState {
            version,
            device_id: DEVICE_ID.to_owned(),
            dps: Mutex::new(dps),
            controls: Mutex::new(Vec::new()),
            queries: AtomicUsize::new(0),
            heartbeats: AtomicUsize::new(0),
            handshakes: AtomicUsize::new(0),
            connections: AtomicUsize::new(0),
            refusing: AtomicBool::new(false),
            ignoring_commands: AtomicBool::new(false),
            wants_listing: AtomicBool::new(false),
            impostor: AtomicBool::new(false),
            push: broadcast::channel(16).0,
            kick: broadcast::channel(4).0,
        });
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let shared = Arc::clone(&state);
        let task = tokio::spawn(async move {
            loop {
                let Ok((stream, _)) = listener.accept().await else {
                    return;
                };
                shared.connections.fetch_add(1, Ordering::SeqCst);
                if shared.refusing.load(Ordering::SeqCst) {
                    drop(stream);
                    continue;
                }
                tokio::spawn(serve(Arc::clone(&shared), stream));
            }
        });
        Self { addr, state, task }
    }

    fn port(&self) -> u16 {
        self.addr.port()
    }

    /// Changes data points on the device and reports them, like a device
    /// whose button was pressed.
    fn push(&self, changes: Value) {
        let Value::Object(changes) = changes else {
            panic!("changes must be an object");
        };
        self.state.dps.lock().unwrap().extend(changes.clone());
        let _ = self.state.push.send(changes);
    }

    fn controls(&self) -> Vec<(u32, Value)> {
        self.state.controls.lock().unwrap().clone()
    }

    /// The data points set by commands, in order.
    fn sets(&self) -> Vec<Value> {
        self.controls()
            .into_iter()
            .filter_map(|(_, payload)| {
                let dps = payload
                    .get("dps")
                    .or_else(|| payload.get("data").and_then(|data| data.get("dps")))?;
                (!dps.as_object()?.values().all(Value::is_null)).then(|| dps.clone())
            })
            .collect()
    }

    fn count(counter: &AtomicUsize) -> usize {
        counter.load(Ordering::SeqCst)
    }

    /// Drops every open session.
    fn kick(&self) {
        let _ = self.state.kick.send(());
    }
}

async fn write(stream: &mut TcpStream, frame: &[u8]) -> bool {
    stream.write_all(frame).await.is_ok()
}

fn status_payload(state: &MockState, dps: &Map<String, Value>) -> Vec<u8> {
    let id = if state.impostor.load(Ordering::SeqCst) {
        "bf99999999999999999999"
    } else {
        state.device_id.as_str()
    };
    match state.version {
        Version::V33 => json!({ "devId": id, "dps": dps, "t": 1_700_000_000 }),
        Version::V34 | Version::V35 => {
            json!({ "protocol": 4, "t": 1_700_000_000, "data": { "dps": dps } })
        }
    }
    .to_string()
    .into_bytes()
}

/// One session, from the device's side.
async fn serve(state: Arc<MockState>, mut stream: TcpStream) {
    let mut codec = Codec::new(state.version, *KEY, Side::Device);
    let mut client_nonce = [0u8; 16];
    let mut buf = Vec::new();
    let mut pushes = state.push.subscribe();
    let mut kicks = state.kick.subscribe();
    let mut seq = 100u32;
    loop {
        let mut chunk = [0u8; 4096];
        tokio::select! {
            read = stream.read(&mut chunk) => {
                let Ok(read) = read else { return };
                if read == 0 {
                    return;
                }
                buf.extend_from_slice(&chunk[..read]);
            }
            changes = pushes.recv() => {
                let Ok(changes) = changes else { continue };
                seq += 1;
                let frame = codec
                    .encode_frame(seq, cmd::STATUS, 0, &status_payload(&state, &changes))
                    .unwrap();
                if !write(&mut stream, &frame).await {
                    return;
                }
                continue;
            }
            _ = kicks.recv() => return,
        }
        loop {
            let (message, used) = match codec.decode(&buf) {
                Ok(Some(decoded)) => decoded,
                Ok(None) => break,
                // A client with the wrong key: a real device just closes.
                Err(_) => return,
            };
            buf.drain(..used);
            let reply: Option<Vec<u8>> = match message.cmd {
                cmd::SESS_KEY_NEG_START => {
                    client_nonce.copy_from_slice(&message.payload[..16]);
                    let proof = crypto::hmac_sha256(KEY, &[&client_nonce]);
                    let mut payload = DEVICE_NONCE.to_vec();
                    payload.extend_from_slice(&proof);
                    Some(
                        codec
                            .encode_frame(message.seq, cmd::SESS_KEY_NEG_RESP, 0, &payload)
                            .unwrap(),
                    )
                }
                cmd::SESS_KEY_NEG_FINISH => {
                    if message.payload != crypto::hmac_sha256(KEY, &[&DEVICE_NONCE]) {
                        return;
                    }
                    let key =
                        protocol::session_key(state.version, KEY, &client_nonce, &DEVICE_NONCE)
                            .unwrap();
                    codec.set_session_key(key);
                    state.handshakes.fetch_add(1, Ordering::SeqCst);
                    None
                }
                cmd::HEART_BEAT => {
                    state.heartbeats.fetch_add(1, Ordering::SeqCst);
                    Some(
                        codec
                            .encode_frame(message.seq, cmd::HEART_BEAT, 0, b"")
                            .unwrap(),
                    )
                }
                cmd::DP_QUERY | cmd::DP_QUERY_NEW => {
                    state.queries.fetch_add(1, Ordering::SeqCst);
                    if message.cmd == cmd::DP_QUERY && state.wants_listing.load(Ordering::SeqCst) {
                        let mut body = 1u32.to_be_bytes().to_vec();
                        body.extend_from_slice(b"json obj data unvalid");
                        Some(frame::encode_55aa(
                            message.seq,
                            cmd::DP_QUERY,
                            &body,
                            Check::Crc,
                        ))
                    } else {
                        let dps = state.dps.lock().unwrap().clone();
                        Some(
                            codec
                                .encode_frame(
                                    message.seq,
                                    message.cmd,
                                    0,
                                    &status_payload(&state, &dps),
                                )
                                .unwrap(),
                        )
                    }
                }
                cmd::CONTROL | cmd::CONTROL_NEW => {
                    let payload: Value = serde_json::from_slice(&message.payload).unwrap();
                    state
                        .controls
                        .lock()
                        .unwrap()
                        .push((message.cmd, payload.clone()));
                    let dps = payload
                        .get("dps")
                        .or_else(|| payload.get("data").and_then(|data| data.get("dps")))
                        .and_then(Value::as_object)
                        .cloned()
                        .unwrap_or_default();
                    if dps.values().all(Value::is_null) {
                        // A listing status query: report, set nothing.
                        state.queries.fetch_add(1, Ordering::SeqCst);
                        let all = state.dps.lock().unwrap().clone();
                        Some(
                            codec
                                .encode_frame(
                                    message.seq,
                                    message.cmd,
                                    0,
                                    &status_payload(&state, &all),
                                )
                                .unwrap(),
                        )
                    } else if state.ignoring_commands.load(Ordering::SeqCst) {
                        None
                    } else {
                        state.dps.lock().unwrap().extend(dps.clone());
                        let ack = codec
                            .encode_frame(message.seq, message.cmd, 0, b"")
                            .unwrap();
                        if !write(&mut stream, &ack).await {
                            return;
                        }
                        seq += 1;
                        Some(
                            codec
                                .encode_frame(seq, cmd::STATUS, 0, &status_payload(&state, &dps))
                                .unwrap(),
                        )
                    }
                }
                _ => None,
            };
            if let Some(reply) = reply
                && !write(&mut stream, &reply).await
            {
                return;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// The driver
// ---------------------------------------------------------------------------

fn fast_timing() -> Timing {
    Timing {
        poll: Duration::from_millis(250),
        heartbeat: Duration::from_millis(400),
        io: Duration::from_millis(500),
        silence: Duration::from_millis(1_500),
        min_backoff: Duration::from_millis(50),
        max_backoff: Duration::from_millis(200),
        offline_after: 2,
        discovery_wait: Duration::from_millis(300),
        command_budget: Duration::from_millis(1_500),
        confirm_query: Duration::from_millis(100),
    }
}

fn config(version: &str, category: &str, mapping: Value, port: u16) -> Value {
    json!({
        "device_id": DEVICE_ID,
        "local_key": "0123456789abcdef",
        "protocol_version": version,
        "host": "127.0.0.1",
        "port": port,
        "category": category,
        "mapping": mapping,
    })
}

fn driver_with(config: Value, timing: Timing, discovery: Arc<Discovery>) -> Arc<TuyaDriver> {
    let config = TuyaConfig::from_value(&config).unwrap();
    Arc::new(TuyaDriver::build(config, timing, Locator::Given(discovery)).unwrap())
}

fn driver(config: Value) -> Arc<TuyaDriver> {
    driver_with(config, fast_timing(), Discovery::idle())
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

    async fn state_of(&mut self, kind: CapabilityKind) -> CapabilityState {
        loop {
            if let BusEvent::State(StateChange { state, .. }) = self.next().await
                && state.kind() == kind
            {
                return state;
            }
        }
    }

    async fn availability(&mut self) -> bool {
        loop {
            if let BusEvent::Availability(AvailabilityChange { online, .. }) = self.next().await {
                return online;
            }
        }
    }

    /// Everything published within `window`.
    async fn drain(&mut self, window: Duration) -> Vec<BusEvent> {
        let mut events = Vec::new();
        let until = Instant::now() + window;
        while let Ok(Some(event)) = timeout(
            until.saturating_duration_since(Instant::now()),
            self.rx.recv(),
        )
        .await
        {
            events.push(event);
        }
        events
    }
}

/// Waits (up to 5 s) until `check` returns something.
async fn eventually<T>(check: impl Fn() -> Option<T>) -> T {
    let until = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(value) = check() {
            return value;
        }
        assert!(Instant::now() < until, "timed out");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

fn start(driver: &Arc<TuyaDriver>) -> Running {
    let (tx, rx) = mpsc::channel(64);
    let sink = EventSink::new(DeviceId::new("plug").unwrap(), tx);
    let runner = Arc::clone(driver);
    let task = tokio::spawn(async move { runner.run(sink).await });
    Running { rx, task }
}

const VERSIONS: [(Version, &str); 3] = [
    (Version::V33, "3.3"),
    (Version::V34, "3.4"),
    (Version::V35, "3.5"),
];

fn plug_meter(power_w: f64) -> CapabilityState {
    PowerMeterState {
        voltage_v: Some(227.2),
        current_a: Some(0.112),
        ..PowerMeterState::power(power_w)
    }
    .into()
}

#[tokio::test]
async fn reads_and_scales_the_status_in_every_version() {
    for (version, name) in VERSIONS {
        let mock = Mock::start(version, plug_dps()).await;
        let driver = driver(config(name, "tdq", plug_mapping(), mock.port()));
        let mut running = start(&driver);
        assert!(running.availability().await, "{name}");
        assert_eq!(
            running.state_of(CapabilityKind::OnOff).await,
            OnOffState { on: true }.into(),
            "{name}"
        );
        assert_eq!(
            running.state_of(CapabilityKind::PowerMeter).await,
            plug_meter(12.5),
            "{name}"
        );
        let negotiated = Mock::count(&mock.state.handshakes);
        assert_eq!(negotiated, usize::from(version.negotiates()), "{name}");
        assert!(
            mock.controls().is_empty(),
            "{name}: a status read sets nothing"
        );
    }
}

#[tokio::test]
async fn describes_a_plug_and_a_robot() {
    let plug = driver(config("3.5", "tdq", plug_mapping(), 1));
    let endpoints = plug.describe().endpoints;
    assert_eq!(endpoints.len(), 1);
    assert_eq!(endpoints[0].endpoint_type, EndpointType::Outlet);
    assert!(endpoints[0].capabilities.power_meter.is_some());

    let robot = driver(config("3.5", "sd", vacuum_mapping(), 1));
    let endpoints = robot.describe().endpoints;
    assert_eq!(endpoints[0].endpoint_type, EndpointType::RobotVacuum);
    let caps: &Capabilities = &endpoints[0].capabilities;
    assert!(caps.robot_vacuum.is_some() && caps.battery.is_some());

    let unknown = driver(config("3.5", "mcs", json!({}), 1));
    assert!(unknown.describe().endpoints.is_empty());
}

#[tokio::test]
async fn publishes_what_the_device_pushes_and_only_changes() {
    for (version, name) in VERSIONS {
        let mock = Mock::start(version, plug_dps()).await;
        let driver = driver(config(name, "tdq", plug_mapping(), mock.port()));
        let mut running = start(&driver);
        assert_eq!(
            running.state_of(CapabilityKind::PowerMeter).await,
            plug_meter(12.5)
        );
        mock.push(json!({ "22": 1500 }));
        assert_eq!(
            running.state_of(CapabilityKind::PowerMeter).await,
            plug_meter(150.0),
            "{name}"
        );
        mock.push(json!({ "1": false }));
        assert_eq!(
            running.state_of(CapabilityKind::OnOff).await,
            OnOffState { on: false }.into(),
            "{name}"
        );
        // Polls keep coming, but nothing changed: nothing is republished.
        let quiet = running.drain(Duration::from_millis(700)).await;
        assert!(quiet.is_empty(), "{name}: {quiet:?}");
        assert!(Mock::count(&mock.state.queries) >= 3, "{name}");
    }
}

fn without_time(mut payload: Value) -> Value {
    if let Some(object) = payload.as_object_mut() {
        let t = object.remove("t");
        assert!(t.is_some(), "every command carries a timestamp");
    }
    payload
}

#[tokio::test]
async fn switch_commands_send_exactly_one_data_point() {
    for (version, name) in VERSIONS {
        let mock = Mock::start(version, plug_dps()).await;
        let driver = driver(config(name, "tdq", plug_mapping(), mock.port()));
        let mut running = start(&driver);
        assert_eq!(
            running.state_of(CapabilityKind::OnOff).await,
            OnOffState { on: true }.into()
        );
        let main = EndpointId::main();
        driver
            .execute(&main, Command::OnOff(OnOffCommand::Off))
            .await
            .unwrap();
        assert_eq!(
            running.state_of(CapabilityKind::OnOff).await,
            OnOffState { on: false }.into()
        );
        driver
            .execute(&main, Command::OnOff(OnOffCommand::Toggle))
            .await
            .unwrap();
        driver
            .execute(&main, Command::OnOff(OnOffCommand::On))
            .await
            .unwrap();

        // The device's report of the toggle may confirm "on" before the
        // mock has logged it.
        let controls = eventually(|| {
            let controls = mock.controls();
            (controls.len() >= 3).then_some(controls)
        })
        .await;
        assert_eq!(controls.len(), 3, "{name}");
        let expected_cmd = match version {
            Version::V33 => cmd::CONTROL,
            Version::V34 | Version::V35 => cmd::CONTROL_NEW,
        };
        for (sent, on) in controls.iter().zip([false, true, true]) {
            assert_eq!(sent.0, expected_cmd, "{name}");
            let expected = match version {
                Version::V33 => json!({
                    "devId": DEVICE_ID, "uid": DEVICE_ID, "dps": { "1": on }
                }),
                Version::V34 | Version::V35 => json!({
                    "protocol": 5, "data": { "dps": { "1": on } }
                }),
            };
            assert_eq!(without_time(sent.1.clone()), expected, "{name}");
            let t = &sent.1["t"];
            match version {
                Version::V33 => assert!(t.is_string(), "{name}: 3.3 sends the time as text"),
                _ => assert!(t.is_u64(), "{name}: 3.4/3.5 send the time as a number"),
            }
        }
    }
}

#[tokio::test]
async fn a_repeated_command_is_still_confirmed_on_screen() {
    let mock = Mock::start(Version::V35, plug_dps()).await;
    let driver = driver(config("3.5", "tdq", plug_mapping(), mock.port()));
    let mut running = start(&driver);
    assert_eq!(
        running.state_of(CapabilityKind::OnOff).await,
        OnOffState { on: true }.into()
    );
    running.drain(Duration::from_millis(300)).await;
    // "On" while on: nothing changes, but the state is reported again.
    driver
        .execute(&EndpointId::main(), Command::OnOff(OnOffCommand::On))
        .await
        .unwrap();
    assert_eq!(
        running.state_of(CapabilityKind::OnOff).await,
        OnOffState { on: true }.into()
    );
    assert_eq!(mock.sets(), [json!({ "1": true })]);
}

#[tokio::test]
async fn robot_commands_send_exactly_these_data_points() {
    let mock = Mock::start(Version::V35, vacuum_dps()).await;
    let driver = driver(config("3.5", "sd", vacuum_mapping(), mock.port()));
    let mut running = start(&driver);
    assert_eq!(
        running.state_of(CapabilityKind::RobotVacuum).await,
        RobotVacuumState {
            status: RobotVacuumStatus::Docked,
            cleaned_area_m2: Some(22.0),
            cleaning_duration_s: Some(1800),
        }
        .into()
    );
    assert_eq!(
        running.state_of(CapabilityKind::Battery).await,
        BatteryState {
            percent: Percent::saturating(100),
            low: false
        }
        .into()
    );
    let main = EndpointId::main();
    let vacuum = |action| Command::RobotVacuum(action);
    driver
        .execute(&main, vacuum(RobotVacuumCommand::Start))
        .await
        .unwrap();
    driver
        .execute(&main, vacuum(RobotVacuumCommand::Pause))
        .await
        .unwrap();
    // The robot reports it paused; starting again carries on.
    mock.push(json!({ "5": "paused" }));
    loop {
        if let CapabilityState::RobotVacuum(state) =
            running.state_of(CapabilityKind::RobotVacuum).await
            && state.status == RobotVacuumStatus::Paused
        {
            break;
        }
    }
    driver
        .execute(&main, vacuum(RobotVacuumCommand::Start))
        .await
        .unwrap();
    driver
        .execute(&main, vacuum(RobotVacuumCommand::ReturnToDock))
        .await
        .unwrap();
    assert_eq!(
        mock.sets(),
        [
            json!({ "1": true }),
            json!({ "2": true }),
            json!({ "2": false }),
            json!({ "3": true }),
        ]
    );
    // Commands for things the robot does not have never reach it.
    assert!(matches!(
        driver
            .execute(&main, Command::OnOff(OnOffCommand::On))
            .await,
        Err(DriverError::Unsupported)
    ));
    assert!(matches!(
        driver
            .execute(
                &EndpointId::new("light").unwrap(),
                vacuum(RobotVacuumCommand::Start)
            )
            .await,
        Err(DriverError::UnknownEndpoint(_))
    ));
    assert_eq!(mock.sets().len(), 4);
}

#[tokio::test]
async fn read_only_devices_only_ever_get_queries_and_heartbeats() {
    for (version, name) in VERSIONS {
        let mock = Mock::start(version, vacuum_dps()).await;
        let mut config = config(name, "sd", vacuum_mapping(), mock.port());
        config["read_only"] = json!(true);
        let mut timing = fast_timing();
        timing.poll = Duration::from_secs(5);
        timing.heartbeat = Duration::from_millis(100);
        let driver = driver_with(config, timing, Discovery::idle());

        let features = driver.describe().endpoints[0]
            .capabilities
            .robot_vacuum
            .as_ref()
            .unwrap()
            .features;
        assert!(!features.start && !features.pause && !features.return_to_dock);

        let mut running = start(&driver);
        assert!(running.availability().await);
        for action in [
            RobotVacuumCommand::Start,
            RobotVacuumCommand::Pause,
            RobotVacuumCommand::ReturnToDock,
        ] {
            assert!(matches!(
                driver
                    .execute(&EndpointId::main(), Command::RobotVacuum(action))
                    .await,
                Err(DriverError::ReadOnly)
            ));
        }
        running.drain(Duration::from_millis(500)).await;
        assert!(mock.controls().is_empty(), "{name}: nothing was set");
        assert!(Mock::count(&mock.state.heartbeats) >= 2, "{name}");
        assert_eq!(Mock::count(&mock.state.queries), 1, "{name}");
    }

    let mock = Mock::start(Version::V35, plug_dps()).await;
    let mut config = config("3.5", "tdq", plug_mapping(), mock.port());
    config["read_only"] = json!(true);
    let driver = driver(config);
    let on_off = driver.describe().endpoints[0].capabilities.clone();
    assert!(on_off.on_off.as_ref().unwrap().read_only());
    let mut running = start(&driver);
    assert!(running.availability().await);
    assert!(matches!(
        driver
            .execute(&EndpointId::main(), Command::OnOff(OnOffCommand::Off))
            .await,
        Err(DriverError::ReadOnly)
    ));
    assert!(mock.controls().is_empty());
}

#[tokio::test]
async fn goes_offline_and_comes_back_by_itself() {
    let mock = Mock::start(Version::V35, plug_dps()).await;
    let driver = driver(config("3.5", "tdq", plug_mapping(), mock.port()));
    let mut running = start(&driver);
    assert!(running.availability().await);

    // The plug loses power: sessions drop and connections are refused.
    mock.state.refusing.store(true, Ordering::SeqCst);
    mock.kick();
    assert!(!running.availability().await);
    assert!(matches!(
        driver
            .execute(&EndpointId::main(), Command::OnOff(OnOffCommand::Off))
            .await,
        Err(DriverError::Offline)
    ));
    let attempts = Mock::count(&mock.state.connections);

    // Power is back.
    mock.state.refusing.store(false, Ordering::SeqCst);
    assert!(running.availability().await);
    assert!(Mock::count(&mock.state.connections) > attempts);
    assert!(Mock::count(&mock.state.handshakes) >= 2);
    // The command sent while it was away was never kept for later.
    running.drain(Duration::from_millis(300)).await;
    assert!(mock.controls().is_empty());
}

#[tokio::test]
async fn one_dropped_session_does_not_show_offline() {
    let mock = Mock::start(Version::V34, plug_dps()).await;
    let driver = driver(config("3.4", "tdq", plug_mapping(), mock.port()));
    let mut running = start(&driver);
    assert!(running.availability().await);
    mock.kick();
    let events = running.drain(Duration::from_millis(800)).await;
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, BusEvent::Availability(_))),
        "{events:?}"
    );
    assert!(Mock::count(&mock.state.handshakes) >= 2, "it reconnected");
}

#[tokio::test]
async fn a_silent_device_is_dropped_and_commands_time_out() {
    let mock = Mock::start(Version::V35, plug_dps()).await;
    let driver = driver(config("3.5", "tdq", plug_mapping(), mock.port()));
    let mut running = start(&driver);
    assert!(running.availability().await);
    mock.state.ignoring_commands.store(true, Ordering::SeqCst);
    let started = Instant::now();
    assert!(matches!(
        driver
            .execute(&EndpointId::main(), Command::OnOff(OnOffCommand::Off))
            .await,
        Err(DriverError::Timeout)
    ));
    assert!(started.elapsed() <= Duration::from_millis(1_800));
    // It was sent once, never again.
    running.drain(Duration::from_millis(400)).await;
    assert_eq!(mock.sets(), [json!({ "1": false })]);
}

#[tokio::test]
async fn never_offline_without_an_address() {
    // No host, nothing heard: the device simply stays unavailable.
    let mut config = config("3.5", "tdq", plug_mapping(), 1);
    config.as_object_mut().unwrap().remove("host");
    let driver = driver(config);
    let mut running = start(&driver);
    assert!(running.drain(Duration::from_millis(800)).await.is_empty());
    assert!(matches!(
        driver
            .execute(&EndpointId::main(), Command::OnOff(OnOffCommand::On))
            .await,
        Err(DriverError::Offline)
    ));
}

#[tokio::test]
async fn finds_the_device_by_its_broadcast() {
    let mock = Mock::start(Version::V35, plug_dps()).await;
    let discovery = Discovery::listen(&["127.0.0.1:0".parse().unwrap()]);
    let [listening] = discovery.local_addrs() else {
        panic!("not listening");
    };
    let listening = *listening;
    // No address configured, and an old protocol version: the broadcast
    // tells where the device is and what it speaks.
    let mut config = config("3.3", "tdq", plug_mapping(), mock.port());
    config.as_object_mut().unwrap().remove("host");
    let mut timing = fast_timing();
    timing.discovery_wait = Duration::from_secs(5);
    let driver = driver_with(config, timing, Arc::clone(&discovery));
    let mut running = start(&driver);

    let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let broadcast = discovery::tests::broadcast_v35(DEVICE_ID, "192.0.2.123");
    socket.send_to(&broadcast, listening).await.unwrap();
    assert!(running.availability().await);
    assert_eq!(
        running.state_of(CapabilityKind::PowerMeter).await,
        plug_meter(12.5)
    );
    assert_eq!(Mock::count(&mock.state.handshakes), 1);
}

#[tokio::test]
async fn a_wrong_key_never_shows_the_device() {
    let mock = Mock::start(Version::V35, plug_dps()).await;
    let mut config = config("3.5", "tdq", plug_mapping(), mock.port());
    config["local_key"] = json!("fedcba9876543210");
    let driver = driver(config);
    let mut running = start(&driver);
    assert!(running.drain(Duration::from_millis(800)).await.is_empty());
    assert!(Mock::count(&mock.state.connections) >= 2, "it keeps trying");
    assert_eq!(Mock::count(&mock.state.handshakes), 0);
}

#[tokio::test]
async fn another_device_at_the_address_is_never_controlled() {
    let mock = Mock::start(Version::V33, plug_dps()).await;
    mock.state.impostor.store(true, Ordering::SeqCst);
    let driver = driver(config("3.3", "tdq", plug_mapping(), mock.port()));
    let mut running = start(&driver);
    assert!(running.drain(Duration::from_millis(600)).await.is_empty());
    assert!(matches!(
        driver
            .execute(&EndpointId::main(), Command::OnOff(OnOffCommand::On))
            .await,
        Err(DriverError::Offline)
    ));
    assert!(mock.sets().is_empty());
}

#[tokio::test]
async fn v33_devices_that_want_a_listing_query_get_one() {
    let mock = Mock::start(Version::V33, plug_dps()).await;
    mock.state.wants_listing.store(true, Ordering::SeqCst);
    let driver = driver(config("3.3", "tdq", plug_mapping(), mock.port()));
    let mut running = start(&driver);
    assert!(running.availability().await);
    assert_eq!(
        running.state_of(CapabilityKind::PowerMeter).await,
        plug_meter(12.5)
    );
    // The listing query names every data point, each null: it sets nothing.
    let (code, payload) = mock.controls()[0].clone();
    assert_eq!(code, cmd::CONTROL_NEW);
    let listed = payload["dps"].as_object().unwrap();
    assert_eq!(listed.len(), 7);
    assert!(listed.values().all(Value::is_null));
    assert!(mock.sets().is_empty());
}

#[tokio::test]
async fn reads_the_status_once_for_the_importer() {
    for (version, name) in VERSIONS {
        let mock = Mock::start(version, vacuum_dps()).await;
        let config =
            TuyaConfig::from_value(&config(name, "sd", vacuum_mapping(), mock.port())).unwrap();
        let status = read_status(&config, Discovery::idle(), Duration::from_millis(100))
            .await
            .unwrap();
        assert_eq!(status.version, version);
        assert_eq!(status.values.get("5"), Some(&json!("charge_done")));
        assert!(mock.controls().is_empty());
    }
}

#[test]
fn the_config_example_builds_a_plug_and_a_robot() {
    let example: Value =
        serde_json::from_str(include_str!("../../../../config.example.json")).unwrap();
    let devices = example["devices"].as_array().unwrap();
    let tuya: Vec<_> = devices.iter().filter(|d| d["driver"] == "tuya").collect();
    assert_eq!(tuya.len(), 2);
    let plug = TuyaDriver::from_config(&tuya[0]["config"]).unwrap();
    let endpoints = plug.describe().endpoints;
    assert_eq!(endpoints[0].endpoint_type, EndpointType::Outlet);
    assert!(endpoints[0].capabilities.power_meter.is_some());
    let robot = TuyaDriver::from_config(&tuya[1]["config"]).unwrap();
    assert!(robot.config().read_only);
    assert_eq!(
        robot.describe().endpoints[0].endpoint_type,
        EndpointType::RobotVacuum
    );
}
