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
    Arc::new(SimDriver::from_config(&config).unwrap())
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
        json!("light"),
    ] {
        assert!(
            matches!(
                SimDriver::from_config(&bad),
                Err(DriverError::InvalidConfig(_))
            ),
            "{bad} should be rejected"
        );
    }
    assert!(SimDriver::from_config(&json!({ "model": "doorbell", "ring_every_sec": 30 })).is_ok());
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
        SimDriver::from_config(&device.config).unwrap();
    }
    let dashboard = vesta_core::Dashboard::from_json(demo::DASHBOARD).unwrap();
    assert!(!dashboard.pages.is_empty());
}
