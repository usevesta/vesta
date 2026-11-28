//! The simulator has no I/O of its own: tests feed it commands and assert
//! what it publishes through the sink. Time is paused, so motion and
//! latency run instantly and deterministically.

use std::sync::Arc;

use serde_json::json;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio::time::{Duration, Instant, timeout};
use vesta_core::{
    AvailabilityChange, BusEvent, DeviceId, EndpointEvent, LockCommand, StateChange, Target,
};

use super::*;

fn driver(config: serde_json::Value) -> Arc<SimDriver> {
    Arc::new(SimDriver::from_config(&config, &DriverContext::in_memory()).unwrap())
}

struct Running {
    rx: mpsc::Receiver<BusEvent>,
    task: JoinHandle<Result<(), DriverError>>,
}

impl Running {
    async fn next(&mut self) -> BusEvent {
        timeout(Duration::from_secs(600), self.rx.recv())
            .await
            .expect("driver went quiet")
            .expect("driver stopped")
    }

    async fn next_state(&mut self) -> StateChange {
        match self.next().await {
            BusEvent::State(change) => change,
            other => panic!("expected a state update, got {other:?}"),
        }
    }

    async fn next_event(&mut self) -> EndpointEvent {
        match self.next().await {
            BusEvent::Event(event) => event,
            other => panic!("expected an event, got {other:?}"),
        }
    }

    /// Skips the availability report and the initial state of every
    /// capability.
    async fn skip_initial(&mut self, states: usize) {
        assert_eq!(
            self.next().await,
            BusEvent::Availability(AvailabilityChange {
                device_id: DeviceId::new("sim_device").unwrap(),
                online: true,
            })
        );
        for _ in 0..states {
            self.next_state().await;
        }
    }

    fn assert_quiet(&mut self) {
        assert!(self.rx.try_recv().is_err(), "unexpected update");
    }
}

fn run(driver: &Arc<SimDriver>) -> Running {
    let (tx, rx) = mpsc::channel(256);
    let sink = EventSink::new(DeviceId::new("sim_device").unwrap(), tx);
    let driver = Arc::clone(driver);
    let task = tokio::spawn(async move { driver.run(sink).await });
    Running { rx, task }
}

fn ep(raw: &str) -> EndpointId {
    EndpointId::new(raw).unwrap()
}

fn target(endpoint: &str) -> Target {
    format!("sim_device/{endpoint}").parse().unwrap()
}

fn cover_state(change: &StateChange) -> CoverState {
    match change.state {
        CapabilityState::Cover(state) => state,
        ref other => panic!("expected cover state, got {other:?}"),
    }
}

#[test]
fn config_defaults() {
    let config = SimConfig::from_value(&json!({ "model": "light" })).unwrap();
    assert_eq!(config, SimConfig::new(SimModel::Light));
    assert_eq!(config.latency_ms, 150);
    assert_eq!(config.failure_rate, 0.0);
}

#[test]
fn config_is_validated() {
    for bad in [
        json!({}),
        json!({ "model": "toaster" }),
        json!({ "model": "light", "failure_rate": 1.5 }),
        json!({ "model": "light", "failure_rate": -0.1 }),
        json!({ "model": "light", "latency_ms": 600_000 }),
        json!({ "model": "light", "ring_every_sec": 10 }),
        json!({ "model": "doorbell", "ring_every_sec": 0 }),
        json!({ "model": "light", "colour": "red" }),
        json!({ "model": "light", "solar": true }),
        json!({ "model": "doorbell", "battery": false }),
        json!({ "model": "light", "hours": "on_request" }),
        json!({ "model": "ev_charger", "hours": "sometimes" }),
        json!({ "model": "light", "read_only": true }),
        json!({ "model": "robot_vacuum", "load_w": 10.0 }),
        json!({ "model": "smart_plug", "load_w": -1.0 }),
        json!("light"),
    ] {
        assert!(
            matches!(
                SimDriver::from_config(&bad, &DriverContext::in_memory()),
                Err(DriverError::InvalidConfig(_))
            ),
            "{bad} should be rejected"
        );
    }
    assert!(
        SimDriver::from_config(
            &json!({ "model": "doorbell", "ring_every_sec": 30 }),
            &DriverContext::in_memory()
        )
        .is_ok()
    );
}

#[test]
fn describes_every_model() {
    let describe = |model: &str| driver(json!({ "model": model })).describe().endpoints;

    let light = describe("light");
    assert_eq!(light.len(), 1);
    assert_eq!(light[0].id, "main");
    assert_eq!(light[0].endpoint_type, EndpointType::Light);
    assert_eq!(
        light[0].capabilities.kinds(),
        vec![
            CapabilityKind::OnOff,
            CapabilityKind::Level,
            CapabilityKind::ColorTemperature
        ]
    );

    let fan = describe("ceiling_fan");
    assert_eq!(
        fan.iter().map(|e| e.id.as_str()).collect::<Vec<_>>(),
        ["fan", "light"]
    );
    assert_eq!(fan[0].endpoint_type, EndpointType::Fan);
    assert_eq!(
        fan[0].capabilities.fan.as_ref().unwrap().features,
        FanFeatures {
            speed_steps: 6,
            reversible: true
        }
    );
    assert_eq!(
        fan[1].capabilities.kinds(),
        vec![CapabilityKind::OnOff, CapabilityKind::Level]
    );

    let blind = describe("blind");
    assert_eq!(blind[0].endpoint_type, EndpointType::WindowCovering);
    assert_eq!(
        blind[0].capabilities.cover.as_ref().unwrap().features,
        CoverFeatures {
            kind: CoverKind::Blind,
            position: true,
            stop: true
        }
    );

    let garage = describe("garage_door");
    assert_eq!(garage[0].endpoint_type, EndpointType::GarageDoor);
    let garage_cover = garage[0].capabilities.cover.as_ref().unwrap();
    assert!(!garage_cover.features.position);
    assert_eq!(
        garage_cover.state,
        Some(CoverState {
            position: None,
            motion: CoverMotion::Stopped,
            open: Some(false)
        })
    );

    let strike = describe("door_strike");
    assert_eq!(strike[0].endpoint_type, EndpointType::Door);
    assert_eq!(
        strike[0].capabilities.kinds(),
        vec![CapabilityKind::Trigger]
    );

    let doorbell = describe("doorbell");
    assert_eq!(doorbell[0].endpoint_type, EndpointType::Doorbell);
    assert_eq!(
        doorbell[0].capabilities.kinds(),
        vec![CapabilityKind::Doorbell, CapabilityKind::Occupancy]
    );
}

#[tokio::test(start_paused = true)]
async fn light_commands_update_state() {
    let light = driver(json!({ "model": "light" }));
    let mut sim = run(&light);
    sim.skip_initial(3).await;
    let main = EndpointId::main();

    light
        .execute(&main, Command::OnOff(OnOffCommand::On))
        .await
        .unwrap();
    assert_eq!(
        sim.next_state().await,
        StateChange {
            target: target("main"),
            state: OnOffState { on: true }.into()
        }
    );

    light
        .execute(&main, Command::OnOff(OnOffCommand::Toggle))
        .await
        .unwrap();
    assert_eq!(
        sim.next_state().await.state,
        OnOffState { on: false }.into()
    );

    // level.set on a light that is off also turns it on.
    light
        .execute(
            &main,
            Command::Level(LevelCommand::Set {
                level: percent(40),
                transition_ms: None,
            }),
        )
        .await
        .unwrap();
    assert_eq!(sim.next_state().await.state, OnOffState { on: true }.into());
    assert_eq!(
        sim.next_state().await.state,
        LevelState { level: percent(40) }.into()
    );

    // Out-of-range colour temperature is clamped to the features.
    light
        .execute(
            &main,
            Command::ColorTemperature(ColorTemperatureCommand::Set {
                kelvin: 9000,
                transition_ms: Some(300),
            }),
        )
        .await
        .unwrap();
    assert_eq!(
        sim.next_state().await.state,
        ColorTemperatureState { kelvin: 6500 }.into()
    );
    sim.assert_quiet();
}

#[tokio::test(start_paused = true)]
async fn rejects_unknown_endpoints_and_unsupported_commands() {
    let light = driver(json!({ "model": "light" }));
    assert!(matches!(
        light
            .execute(&ep("fan"), Command::OnOff(OnOffCommand::On))
            .await,
        Err(DriverError::UnknownEndpoint(_))
    ));
    for cmd in [
        Command::Cover(CoverCommand::Open),
        Command::Lock(LockCommand::Unlock),
        Command::Trigger(TriggerCommand::Fire),
        Command::Fan(FanCommand::SetSpeed { speed: 1 }),
    ] {
        assert!(
            matches!(
                light.execute(&EndpointId::main(), cmd.clone()).await,
                Err(DriverError::Unsupported)
            ),
            "{cmd:?}"
        );
    }
}

#[tokio::test(start_paused = true)]
async fn commands_take_the_configured_latency() {
    let light = driver(json!({ "model": "light" }));
    let started = Instant::now();
    light
        .execute(&EndpointId::main(), Command::OnOff(OnOffCommand::On))
        .await
        .unwrap();
    assert!(started.elapsed() >= Duration::from_millis(150));

    let fast = driver(json!({ "model": "light", "latency_ms": 0 }));
    let started = Instant::now();
    fast.execute(&EndpointId::main(), Command::OnOff(OnOffCommand::On))
        .await
        .unwrap();
    assert_eq!(started.elapsed(), Duration::ZERO);
}

#[tokio::test(start_paused = true)]
async fn failure_rate_drives_errors() {
    let on = || Command::OnOff(OnOffCommand::On);
    let main = EndpointId::main();

    let always = driver(json!({ "model": "light", "failure_rate": 1.0, "latency_ms": 0 }));
    for _ in 0..20 {
        assert!(matches!(
            always.execute(&main, on()).await,
            Err(DriverError::Protocol(_))
        ));
    }
    // A failed command changes nothing.
    let light = always.describe().endpoints.remove(0);
    assert_eq!(
        light.capabilities.on_off.unwrap().state,
        Some(OnOffState { on: false })
    );

    let never = driver(json!({ "model": "light", "latency_ms": 0 }));
    let mut sim = run(&never);
    sim.skip_initial(3).await;
    for _ in 0..20 {
        never.execute(&main, on()).await.unwrap();
    }

    let half = driver(json!({ "model": "light", "failure_rate": 0.5, "seed": 7, "latency_ms": 0 }));
    let _sim = run(&half);
    let mut failures = 0;
    for _ in 0..100 {
        if half.execute(&main, on()).await.is_err() {
            failures += 1;
        }
    }
    assert!((20..=80).contains(&failures), "{failures} failures");
}

#[tokio::test(start_paused = true)]
async fn ceiling_fan_has_independent_endpoints() {
    let fan = driver(json!({ "model": "ceiling_fan", "latency_ms": 0 }));
    let mut sim = run(&fan);
    // fan: on_off + fan, light: on_off + level.
    sim.skip_initial(4).await;

    fan.execute(&ep("fan"), Command::Fan(FanCommand::SetSpeed { speed: 6 }))
        .await
        .unwrap();
    assert_eq!(
        sim.next_state().await,
        StateChange {
            target: target("fan"),
            state: OnOffState { on: true }.into()
        }
    );
    assert_eq!(
        sim.next_state().await.state,
        FanState {
            speed: 6,
            direction: FanDirection::Forward
        }
        .into()
    );

    fan.execute(
        &ep("fan"),
        Command::Fan(FanCommand::SetDirection {
            direction: FanDirection::Reverse,
        }),
    )
    .await
    .unwrap();
    assert_eq!(
        sim.next_state().await.state,
        FanState {
            speed: 6,
            direction: FanDirection::Reverse
        }
        .into()
    );

    fan.execute(&ep("light"), Command::OnOff(OnOffCommand::On))
        .await
        .unwrap();
    assert_eq!(
        sim.next_state().await,
        StateChange {
            target: target("light"),
            state: OnOffState { on: true }.into()
        }
    );

    // Out of range for a 6-step fan.
    assert!(matches!(
        fan.execute(&ep("fan"), Command::Fan(FanCommand::SetSpeed { speed: 7 }))
            .await,
        Err(DriverError::Unsupported)
    ));
    sim.assert_quiet();
}

#[tokio::test(start_paused = true)]
async fn blind_moves_ten_percent_per_second_and_stops() {
    let blind = driver(json!({ "model": "blind", "latency_ms": 0 }));
    let mut sim = run(&blind);
    sim.skip_initial(1).await;
    let main = EndpointId::main();

    blind
        .execute(&main, Command::Cover(CoverCommand::Close))
        .await
        .unwrap();
    let started = Instant::now();
    assert_eq!(
        cover_state(&sim.next_state().await),
        CoverState {
            position: Some(percent(100)),
            motion: CoverMotion::Closing,
            open: None
        }
    );
    let mut positions = Vec::new();
    for _ in 0..4 {
        let state = cover_state(&sim.next_state().await);
        assert_eq!(state.motion, CoverMotion::Closing);
        positions.push(state.position.unwrap().get());
    }
    assert_eq!(positions, [95, 90, 85, 80]);
    assert_eq!(started.elapsed(), Duration::from_secs(2));

    blind
        .execute(&main, Command::Cover(CoverCommand::Stop))
        .await
        .unwrap();
    assert_eq!(
        cover_state(&sim.next_state().await),
        CoverState {
            position: Some(percent(80)),
            motion: CoverMotion::Stopped,
            open: None
        }
    );
    tokio::time::sleep(Duration::from_secs(5)).await;
    sim.assert_quiet();

    blind
        .execute(
            &main,
            Command::Cover(CoverCommand::SetPosition {
                position: percent(92),
            }),
        )
        .await
        .unwrap();
    let mut last = cover_state(&sim.next_state().await);
    assert_eq!(last.motion, CoverMotion::Opening);
    while last.motion != CoverMotion::Stopped {
        last = cover_state(&sim.next_state().await);
    }
    // Snaps to the target instead of overshooting.
    assert_eq!(last.position, Some(percent(92)));
}

#[tokio::test(start_paused = true)]
async fn garage_door_travels_in_five_seconds() {
    let garage = driver(json!({ "model": "garage_door", "latency_ms": 0 }));
    let mut sim = run(&garage);
    sim.skip_initial(1).await;
    let main = EndpointId::main();

    assert!(matches!(
        garage
            .execute(&main, Command::Cover(CoverCommand::Stop))
            .await,
        Err(DriverError::Unsupported)
    ));

    garage
        .execute(&main, Command::Cover(CoverCommand::Open))
        .await
        .unwrap();
    let started = Instant::now();
    assert_eq!(
        cover_state(&sim.next_state().await),
        CoverState {
            position: None,
            motion: CoverMotion::Opening,
            open: Some(false)
        }
    );
    // Leaves the closed end-stop on the first step.
    assert_eq!(
        cover_state(&sim.next_state().await),
        CoverState {
            position: None,
            motion: CoverMotion::Opening,
            open: Some(true)
        }
    );
    assert_eq!(
        cover_state(&sim.next_state().await),
        CoverState {
            position: None,
            motion: CoverMotion::Stopped,
            open: Some(true)
        }
    );
    assert_eq!(started.elapsed(), Duration::from_secs(5));

    garage
        .execute(&main, Command::Cover(CoverCommand::Close))
        .await
        .unwrap();
    assert_eq!(
        cover_state(&sim.next_state().await).motion,
        CoverMotion::Closing
    );
    assert_eq!(
        cover_state(&sim.next_state().await),
        CoverState {
            position: None,
            motion: CoverMotion::Stopped,
            open: Some(false)
        }
    );
    sim.assert_quiet();
}

#[tokio::test(start_paused = true)]
async fn door_strike_fires_without_state() {
    let strike = driver(json!({ "model": "door_strike" }));
    let mut sim = run(&strike);
    sim.skip_initial(0).await;
    strike
        .execute(&EndpointId::main(), Command::Trigger(TriggerCommand::Fire))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_secs(1)).await;
    sim.assert_quiet();
}

#[tokio::test(start_paused = true)]
async fn doorbell_rings_periodically() {
    let doorbell = driver(json!({ "model": "doorbell", "ring_every_sec": 30 }));
    let mut sim = run(&doorbell);
    sim.skip_initial(1).await;
    let started = Instant::now();

    assert_eq!(
        sim.next_state().await.state,
        OccupancyState { occupied: true }.into()
    );
    assert_eq!(started.elapsed(), Duration::from_secs(30));
    let seen = sim.next_event().await;
    assert_eq!(seen.event, EventKind::OccupancyDetected);
    assert_eq!(seen.target, target("main"));
    assert_eq!(sim.next_event().await.event, EventKind::DoorbellPressed);

    assert_eq!(
        sim.next_state().await.state,
        OccupancyState { occupied: false }.into()
    );
    assert_eq!(started.elapsed(), Duration::from_secs(40));

    assert_eq!(
        sim.next_state().await.state,
        OccupancyState { occupied: true }.into()
    );
    assert_eq!(started.elapsed(), Duration::from_secs(60));
}

#[tokio::test(start_paused = true)]
async fn doorbell_without_interval_stays_quiet() {
    let doorbell = driver(json!({ "model": "doorbell" }));
    let mut sim = run(&doorbell);
    sim.skip_initial(1).await;
    tokio::time::sleep(Duration::from_secs(3600)).await;
    sim.assert_quiet();
}

#[test]
fn ev_charger_describes_the_charger_and_the_homes_meters() {
    let endpoints = |config| driver(config).describe().endpoints;
    let ids = |endpoints: &[Endpoint]| {
        endpoints
            .iter()
            .map(|e| e.id.as_str().to_owned())
            .collect::<Vec<_>>()
    };

    let home = endpoints(json!({ "model": "ev_charger" }));
    assert_eq!(ids(&home), ["charger", "grid", "house", "solar"]);
    let charger = &home[0];
    assert_eq!(charger.endpoint_type, EndpointType::EvCharger);
    let cap = charger.capabilities.ev_charger.as_ref().unwrap();
    assert!(cap.features.pause && cap.features.adjustable_current);
    assert!(cap.features.lock && cap.features.schedule);
    assert_eq!(
        cap.features.modes,
        [
            vesta_core::EvChargeMode::Manual,
            vesta_core::EvChargeMode::Solar,
            vesta_core::EvChargeMode::SolarGrid
        ]
    );
    // Starts with state, never invented later.
    let state = cap.state.as_ref().unwrap();
    assert_eq!(state.status, vesta_core::EvChargerStatus::Charging);
    assert_eq!(cap.current_range(), (6.0, 25.0));

    let roles: Vec<_> = home[1..]
        .iter()
        .map(|e| {
            assert_eq!(e.endpoint_type, EndpointType::Meter);
            e.capabilities.power_meter.as_ref().unwrap().features.role
        })
        .collect();
    assert_eq!(
        roles,
        [
            vesta_core::MeterRole::Grid,
            vesta_core::MeterRole::House,
            vesta_core::MeterRole::Solar
        ]
    );
    let grid = home[1].capabilities.power_meter.as_ref().unwrap();
    assert_eq!(grid.state.unwrap().limit_w, Some(5750.0));

    let no_sun = endpoints(json!({ "model": "ev_charger", "solar": false, "battery": true }));
    assert_eq!(ids(&no_sun), ["charger", "grid", "house", "battery"]);
}

#[tokio::test(start_paused = true)]
async fn ev_charger_reports_changes_and_follows_commands() {
    use vesta_core::{EvChargerCommand, EvChargerState, EvChargerStatus};

    fn charger_state(change: &StateChange) -> EvChargerState {
        match &change.state {
            CapabilityState::EvCharger(state) => state.clone(),
            other => panic!("expected charger state, got {other:?}"),
        }
    }

    let charger = driver(json!({ "model": "ev_charger", "latency_ms": 0, "seed": 7 }));
    let mut sim = run(&charger);
    sim.skip_initial(4).await;

    // The next tick moves the readings: at least the meters change.
    let first = sim.next_state().await;
    assert!(first.target.to_string().starts_with("sim_device/"));
    while sim.rx.try_recv().is_ok() {}

    let main = ep("charger");
    charger
        .execute(&main, Command::EvCharger(EvChargerCommand::Pause))
        .await
        .unwrap();
    let change = sim.next_state().await;
    assert_eq!(change.target, target("charger"));
    let paused = charger_state(&change);
    assert_eq!(paused.status, EvChargerStatus::Paused);
    assert_eq!(paused.power_w, 0.0);
    // The grid and the house no longer power the car; the sun did not
    // change, so the solar meter stays quiet.
    assert_eq!(sim.next_state().await.target, target("grid"));
    assert_eq!(sim.next_state().await.target, target("house"));

    charger
        .execute(
            &main,
            Command::EvCharger(EvChargerCommand::SetCurrent { amps: 99.0 }),
        )
        .await
        .unwrap();
    let state = charger_state(&sim.next_state().await);
    assert_eq!(state.current_a, 25.0, "clamped to the charger's limit");

    // Meters are not on the charger endpoint, and unknown modes fail.
    assert!(matches!(
        charger
            .execute(&ep("grid"), Command::EvCharger(EvChargerCommand::Resume))
            .await,
        Err(DriverError::Unsupported)
    ));
    assert!(matches!(
        charger
            .execute(
                &main,
                Command::EvCharger(EvChargerCommand::SetMode {
                    mode: vesta_core::EvChargeMode::Scheduled
                })
            )
            .await,
        Err(DriverError::Unsupported)
    ));
}

#[tokio::test(start_paused = true)]
async fn on_request_hours_are_read_saved_and_kept_across_restarts() {
    use vesta_core::{
        ChargingWindow, EvChargerCommand, MemoryState, ScheduleSource, StateKey, Weekday,
    };

    let features = |driver: &SimDriver| {
        driver.describe().endpoints[0]
            .capabilities
            .ev_charger
            .clone()
            .unwrap()
    };
    let config = json!({ "model": "ev_charger", "latency_ms": 10, "hours": "on_request" });
    let state = Arc::new(MemoryState::new());
    let context = DriverContext::new(Arc::clone(&state) as Arc<dyn vesta_core::DriverState>);
    let charger = Arc::new(SimDriver::from_config(&config, &context).unwrap());
    let cap = features(&charger);
    let hours = cap.features.charging_schedule.unwrap();
    assert!(!hours.reports_windows && hours.refreshable && hours.editable);
    assert_eq!(
        cap.state.unwrap().charging_schedule,
        None,
        "unknown until read or saved"
    );

    let main = ep("charger");
    let started = Instant::now();
    charger
        .execute(
            &main,
            Command::EvCharger(EvChargerCommand::RefreshChargingSchedule),
        )
        .await
        .unwrap();
    assert!(
        started.elapsed() >= Duration::from_millis(60),
        "the slower path"
    );
    let read = features(&charger).state.unwrap().charging_schedule.unwrap();
    let origin = read.origin.unwrap();
    assert_eq!(origin.source, ScheduleSource::Charger);
    assert!(!origin.restored);
    assert_eq!(read.windows[0].start.to_string(), "23:00");

    let weekend = ChargingWindow {
        start: "10:00".parse().unwrap(),
        end: "14:00".parse().unwrap(),
        days: vec![Weekday::Sunday, Weekday::Saturday],
    };
    charger
        .execute(
            &main,
            Command::EvCharger(EvChargerCommand::SetChargingSchedule {
                enabled: true,
                windows: vec![weekend.clone()],
            }),
        )
        .await
        .unwrap();
    let saved = features(&charger).state.unwrap().charging_schedule.unwrap();
    assert_eq!(saved.origin.unwrap().source, ScheduleSource::Saved);
    let kept = state.values();
    assert_eq!(
        kept.keys().map(StateKey::as_str).collect::<Vec<_>>(),
        ["charging_schedule"]
    );

    // A restart: the hours come back as the last known ones, and are
    // followed.
    let again = SimDriver::from_config(&config, &context).unwrap();
    let restored = features(&again).state.unwrap().charging_schedule.unwrap();
    assert_eq!(restored.windows, [weekend.normalized()]);
    let origin = restored.origin.unwrap();
    assert!(origin.restored);
    assert_eq!(origin.source, ScheduleSource::Saved);

    // Unreadable state is ignored: the charger starts as if new.
    let broken = DriverContext::new(Arc::new(MemoryState::with_values([(
        StateKey::new("charging_schedule").unwrap(),
        json!({ "windows": "nope" }),
    )])));
    let fresh = SimDriver::from_config(&config, &broken).unwrap();
    assert_eq!(features(&fresh).state.unwrap().charging_schedule, None);

    // A charger that reports its hours never needs reading.
    let reported = driver(json!({ "model": "ev_charger", "latency_ms": 0 }));
    assert!(
        !features(&reported)
            .features
            .charging_schedule
            .unwrap()
            .refreshable
    );
    assert!(matches!(
        reported
            .execute(
                &main,
                Command::EvCharger(EvChargerCommand::RefreshChargingSchedule)
            )
            .await,
        Err(DriverError::Unsupported)
    ));
    let live = features(&reported)
        .state
        .unwrap()
        .charging_schedule
        .unwrap();
    assert_eq!(live.origin, None);
}

#[tokio::test(start_paused = true)]
async fn smart_plug_switches_and_measures() {
    use vesta_core::PowerMeterState;

    fn meter(change: &StateChange) -> PowerMeterState {
        match &change.state {
            CapabilityState::PowerMeter(state) => *state,
            other => panic!("expected a meter reading, got {other:?}"),
        }
    }

    let plug =
        driver(json!({ "model": "smart_plug", "latency_ms": 0, "load_w": 100.0, "seed": 3 }));
    let main = ep("main");
    let described = plug.describe().endpoints;
    assert_eq!(described[0].endpoint_type, EndpointType::Outlet);
    assert!(
        !described[0]
            .capabilities
            .on_off
            .as_ref()
            .unwrap()
            .read_only()
    );
    assert_eq!(
        described[0]
            .capabilities
            .power_meter
            .as_ref()
            .unwrap()
            .features
            .role,
        vesta_core::MeterRole::Load
    );

    let mut sim = run(&plug);
    sim.skip_initial(2).await;
    plug.execute(&main, Command::OnOff(OnOffCommand::Off))
        .await
        .unwrap();
    assert_eq!(
        sim.next_state().await.state,
        OnOffState { on: false }.into()
    );
    let off = meter(&sim.next_state().await);
    assert_eq!(off.power_w, 0.0);
    assert_eq!(off.voltage_v, Some(230.0));

    plug.execute(&main, Command::OnOff(OnOffCommand::On))
        .await
        .unwrap();
    assert_eq!(sim.next_state().await.state, OnOffState { on: true }.into());
    let on = meter(&sim.next_state().await);
    assert!((97.0..=103.0).contains(&on.power_w), "{on:?}");
    assert!(
        on.current_a
            .is_some_and(|amps| (0.42..=0.45).contains(&amps))
    );
    // While on, the reading moves a little with every tick and energy adds up.
    let later = meter(&sim.next_state().await);
    assert!((97.0..=103.0).contains(&later.power_w), "{later:?}");
    assert!(later.energy_wh >= on.energy_wh);
}

#[tokio::test(start_paused = true)]
async fn a_watched_plug_and_robot_refuse_every_command() {
    let plug = driver(json!({ "model": "smart_plug", "read_only": true, "latency_ms": 0 }));
    let caps = plug.describe().endpoints[0].capabilities.clone();
    assert!(caps.on_off.as_ref().unwrap().read_only());
    assert!(matches!(
        plug.execute(&ep("main"), Command::OnOff(OnOffCommand::Toggle))
            .await,
        Err(DriverError::Unsupported)
    ));

    let robot = driver(json!({ "model": "robot_vacuum", "read_only": true, "latency_ms": 0 }));
    let features = robot.describe().endpoints[0]
        .capabilities
        .robot_vacuum
        .as_ref()
        .unwrap()
        .features;
    assert!(!features.start && !features.pause && !features.return_to_dock);
    assert!(matches!(
        robot
            .execute(
                &ep("main"),
                Command::RobotVacuum(vesta_core::RobotVacuumCommand::Start)
            )
            .await,
        Err(DriverError::Unsupported)
    ));
}

#[tokio::test(start_paused = true)]
async fn robot_vacuum_cleans_and_goes_home() {
    use vesta_core::{RobotVacuumCommand, RobotVacuumStatus};

    fn status(change: &StateChange) -> Option<RobotVacuumStatus> {
        match &change.state {
            CapabilityState::RobotVacuum(state) => Some(state.status),
            _ => None,
        }
    }

    let robot = driver(json!({ "model": "robot_vacuum", "latency_ms": 0 }));
    let described = robot.describe().endpoints;
    assert_eq!(described[0].endpoint_type, EndpointType::RobotVacuum);
    assert_eq!(
        described[0].capabilities.kinds(),
        vec![CapabilityKind::Battery, CapabilityKind::RobotVacuum]
    );
    let mut sim = run(&robot);
    sim.skip_initial(2).await;
    let main = ep("main");

    robot
        .execute(&main, Command::RobotVacuum(RobotVacuumCommand::Start))
        .await
        .unwrap();
    assert_eq!(
        status(&sim.next_state().await),
        Some(RobotVacuumStatus::Cleaning)
    );
    robot
        .execute(&main, Command::RobotVacuum(RobotVacuumCommand::Pause))
        .await
        .unwrap();
    loop {
        if status(&sim.next_state().await) == Some(RobotVacuumStatus::Paused) {
            break;
        }
    }
    robot
        .execute(
            &main,
            Command::RobotVacuum(RobotVacuumCommand::ReturnToDock),
        )
        .await
        .unwrap();
    loop {
        if status(&sim.next_state().await) == Some(RobotVacuumStatus::Returning) {
            break;
        }
    }
    // Home, charging, then charged.
    loop {
        if status(&sim.next_state().await) == Some(RobotVacuumStatus::Docked) {
            break;
        }
    }
    assert!(matches!(
        robot.execute(&main, Command::OnOff(OnOffCommand::On)).await,
        Err(DriverError::Unsupported)
    ));
}

#[tokio::test(start_paused = true)]
async fn run_stops_when_the_hub_stops_listening() {
    let light = driver(json!({ "model": "light" }));
    let sim = run(&light);
    drop(sim.rx);
    assert!(matches!(
        sim.task.await.unwrap(),
        Err(DriverError::SinkClosed)
    ));
}

#[test]
fn demo_home_is_valid_and_only_uses_the_simulator() {
    let config = vesta_core::HubConfig::from_json(demo::CONFIG).unwrap();
    assert!(config.hub.bind_address.is_loopback());
    assert!(!config.devices.is_empty());
    for device in &config.devices {
        assert_eq!(device.driver, "sim", "{}", device.id);
        SimDriver::from_config(&device.config, &DriverContext::in_memory()).unwrap();
    }
    let dashboard = vesta_core::Dashboard::from_json(demo::DASHBOARD).unwrap();
    assert!(!dashboard.pages.is_empty());
}
