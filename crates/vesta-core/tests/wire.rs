//! Exact JSON wire shapes of commands, errors, state and messages.

// Integration-test crate: helpers outside #[test] fns may panic too.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use serde_json::{Value, json};
use vesta_core::*;

fn assert_wire<T>(value: &T, expected: Value)
where
    T: serde::Serialize + serde::de::DeserializeOwned + PartialEq + std::fmt::Debug,
{
    assert_eq!(serde_json::to_value(value).unwrap(), expected);
    let back: T = serde_json::from_value(expected).unwrap();
    assert_eq!(&back, value);
}

fn pct(value: u8) -> Percent {
    Percent::new(value).unwrap()
}

#[test]
fn every_command_has_the_documented_shape() {
    let cases = [
        (
            Command::OnOff(OnOffCommand::On),
            json!({ "capability": "on_off", "action": "on" }),
        ),
        (
            Command::OnOff(OnOffCommand::Off),
            json!({ "capability": "on_off", "action": "off" }),
        ),
        (
            Command::OnOff(OnOffCommand::Toggle),
            json!({ "capability": "on_off", "action": "toggle" }),
        ),
        (
            Command::Level(LevelCommand::Set {
                level: pct(50),
                transition_ms: Some(300),
            }),
            json!({ "capability": "level", "action": "set", "level": 50, "transition_ms": 300 }),
        ),
        (
            Command::Level(LevelCommand::Set {
                level: pct(0),
                transition_ms: None,
            }),
            json!({ "capability": "level", "action": "set", "level": 0 }),
        ),
        (
            Command::ColorTemperature(ColorTemperatureCommand::Set {
                kelvin: 2700,
                transition_ms: None,
            }),
            json!({ "capability": "color_temperature", "action": "set", "kelvin": 2700 }),
        ),
        (
            Command::Color(ColorCommand::Set {
                hue: Hue::new(240).unwrap(),
                saturation: pct(80),
                transition_ms: Some(0),
            }),
            json!({ "capability": "color", "action": "set", "hue": 240, "saturation": 80, "transition_ms": 0 }),
        ),
        (
            Command::Cover(CoverCommand::Open),
            json!({ "capability": "cover", "action": "open" }),
        ),
        (
            Command::Cover(CoverCommand::Close),
            json!({ "capability": "cover", "action": "close" }),
        ),
        (
            Command::Cover(CoverCommand::Stop),
            json!({ "capability": "cover", "action": "stop" }),
        ),
        (
            Command::Cover(CoverCommand::SetPosition { position: pct(30) }),
            json!({ "capability": "cover", "action": "set_position", "position": 30 }),
        ),
        (
            Command::Fan(FanCommand::SetSpeed { speed: 4 }),
            json!({ "capability": "fan", "action": "set_speed", "speed": 4 }),
        ),
        (
            Command::Fan(FanCommand::SetDirection {
                direction: FanDirection::Reverse,
            }),
            json!({ "capability": "fan", "action": "set_direction", "direction": "reverse" }),
        ),
        (
            Command::Lock(LockCommand::Lock),
            json!({ "capability": "lock", "action": "lock" }),
        ),
        (
            Command::Lock(LockCommand::Unlock),
            json!({ "capability": "lock", "action": "unlock" }),
        ),
        (
            Command::Lock(LockCommand::Unlatch),
            json!({ "capability": "lock", "action": "unlatch" }),
        ),
        (
            Command::Trigger(TriggerCommand::Fire),
            json!({ "capability": "trigger", "action": "fire" }),
        ),
        (
            Command::EvCharger(EvChargerCommand::Pause),
            json!({ "capability": "ev_charger", "action": "pause" }),
        ),
        (
            Command::EvCharger(EvChargerCommand::Resume),
            json!({ "capability": "ev_charger", "action": "resume" }),
        ),
        (
            Command::EvCharger(EvChargerCommand::SetCurrent { amps: 10.0 }),
            json!({ "capability": "ev_charger", "action": "set_current", "amps": 10.0 }),
        ),
        (
            Command::EvCharger(EvChargerCommand::SetMode {
                mode: EvChargeMode::Solar,
            }),
            json!({ "capability": "ev_charger", "action": "set_mode", "mode": "solar" }),
        ),
        (
            Command::EvCharger(EvChargerCommand::SetMode {
                mode: EvChargeMode::SolarGrid,
            }),
            json!({ "capability": "ev_charger", "action": "set_mode", "mode": "solar_grid" }),
        ),
        (
            Command::EvCharger(EvChargerCommand::Lock),
            json!({ "capability": "ev_charger", "action": "lock" }),
        ),
        (
            Command::EvCharger(EvChargerCommand::Unlock),
            json!({ "capability": "ev_charger", "action": "unlock" }),
        ),
        (
            Command::EvCharger(EvChargerCommand::SetSchedule { enabled: true }),
            json!({ "capability": "ev_charger", "action": "set_schedule", "enabled": true }),
        ),
        (
            Command::RobotVacuum(RobotVacuumCommand::Start),
            json!({ "capability": "robot_vacuum", "action": "start" }),
        ),
        (
            Command::RobotVacuum(RobotVacuumCommand::Pause),
            json!({ "capability": "robot_vacuum", "action": "pause" }),
        ),
        (
            Command::RobotVacuum(RobotVacuumCommand::ReturnToDock),
            json!({ "capability": "robot_vacuum", "action": "return_to_dock" }),
        ),
    ];
    for (command, expected) in cases {
        assert_eq!(
            serde_json::to_value(command.capability()).unwrap(),
            expected["capability"]
        );
        assert_wire(&command, expected);
    }
}

#[test]
fn invalid_commands_are_rejected_at_the_boundary() {
    for raw in [
        json!({ "capability": "level", "action": "set", "level": 101 }),
        json!({ "capability": "level", "action": "set" }),
        json!({ "capability": "level", "action": "dim", "level": 5 }),
        json!({ "capability": "teleport", "action": "go" }),
        json!({ "capability": "color", "action": "set", "hue": 361, "saturation": 10 }),
        json!({ "action": "on" }),
    ] {
        assert!(
            serde_json::from_value::<Command>(raw.clone()).is_err(),
            "{raw} should be rejected"
        );
    }
}

#[test]
fn command_request_confirmed_defaults_to_false() {
    let message: ClientMessage = serde_json::from_value(json!({
        "type": "command",
        "request_id": "c-1",
        "target": "garage/main",
        "command": { "capability": "cover", "action": "open" }
    }))
    .unwrap();
    let ClientMessage::Command(request) = message;
    assert!(!request.confirmed);
    assert!(request.command.is_sensitive(EndpointType::GarageDoor));

    let confirmed = ClientMessage::Command(CommandRequest {
        confirmed: true,
        ..request
    });
    assert_wire(
        &confirmed,
        json!({
            "type": "command",
            "request_id": "c-1",
            "target": "garage/main",
            "command": { "capability": "cover", "action": "open" },
            "confirmed": true
        }),
    );

    assert!(
        serde_json::from_value::<ClientMessage>(json!({
            "type": "command",
            "request_id": "c-2",
            "target": "garage",
            "command": { "capability": "cover", "action": "open" }
        }))
        .is_err(),
        "targets must be <device_id>/<endpoint_id>"
    );
}

#[test]
fn command_errors_have_stable_codes() {
    let cases = [
        (CommandError::Unsupported, json!({ "code": "unsupported" })),
        (CommandError::Offline, json!({ "code": "offline" })),
        (CommandError::Timeout, json!({ "code": "timeout" })),
        (
            CommandError::Rejected {
                reason: RejectReason::NotAllowed,
            },
            json!({ "code": "rejected", "reason": "not_allowed" }),
        ),
        (
            CommandError::Rejected {
                reason: RejectReason::ConfirmationRequired,
            },
            json!({ "code": "rejected", "reason": "confirmation_required" }),
        ),
        (
            CommandError::Device { message: None },
            json!({ "code": "device" }),
        ),
        (
            CommandError::Device {
                message: Some("motor stalled".to_owned()),
            },
            json!({ "code": "device", "message": "motor stalled" }),
        ),
    ];
    for (error, expected) in cases {
        assert_wire(&error, expected);
    }
}

#[test]
fn command_results() {
    assert_wire(
        &ServerMessage::CommandResult(CommandResult::ok("c-42")),
        json!({ "type": "command_result", "request_id": "c-42", "ok": true }),
    );
    assert_wire(
        &ServerMessage::from(CommandResult::failed("c-43", CommandError::Offline)),
        json!({
            "type": "command_result",
            "request_id": "c-43",
            "ok": false,
            "error": { "code": "offline" }
        }),
    );
}

#[test]
fn every_capability_state_has_the_documented_shape() {
    let cases: Vec<(CapabilityState, Value)> = vec![
        (OnOffState { on: true }.into(), json!({ "on": true })),
        (LevelState { level: pct(80) }.into(), json!({ "level": 80 })),
        (
            ColorTemperatureState { kelvin: 3000 }.into(),
            json!({ "kelvin": 3000 }),
        ),
        (
            ColorState {
                hue: Hue::new(120).unwrap(),
                saturation: pct(100),
            }
            .into(),
            json!({ "hue": 120, "saturation": 100 }),
        ),
        (
            CoverState {
                position: Some(pct(40)),
                motion: CoverMotion::Opening,
                open: None,
            }
            .into(),
            json!({ "position": 40, "motion": "opening" }),
        ),
        (
            CoverState {
                position: None,
                motion: CoverMotion::Stopped,
                open: Some(false),
            }
            .into(),
            json!({ "motion": "stopped", "open": false }),
        ),
        (
            FanState {
                speed: 2,
                direction: FanDirection::Forward,
            }
            .into(),
            json!({ "speed": 2, "direction": "forward" }),
        ),
        (
            LockState {
                lock_state: LockStatus::Jammed,
            }
            .into(),
            json!({ "lock_state": "jammed" }),
        ),
        (
            CameraState {
                stream_available: true,
            }
            .into(),
            json!({ "stream_available": true }),
        ),
        (
            OccupancyState { occupied: false }.into(),
            json!({ "occupied": false }),
        ),
        (ContactState { open: true }.into(), json!({ "open": true })),
        (
            TemperatureState { celsius: 21.5 }.into(),
            json!({ "celsius": 21.5 }),
        ),
        (
            HumidityState { percent: pct(45) }.into(),
            json!({ "percent": 45 }),
        ),
        (
            PowerMeterState {
                energy_wh: Some(1_000_000.0),
                energy_exported_wh: Some(250_000.0),
                ..PowerMeterState::power(-1250.0)
            }
            .into(),
            json!({ "power_w": -1250.0, "energy_wh": 1_000_000.0, "energy_exported_wh": 250_000.0 }),
        ),
        (
            PowerMeterState {
                limit_w: Some(5700.0),
                ..PowerMeterState::power(5517.8)
            }
            .into(),
            json!({ "power_w": 5517.8, "limit_w": 5700.0 }),
        ),
        (
            PowerMeterState {
                energy_wh: Some(1234.0),
                voltage_v: Some(227.2),
                current_a: Some(0.112),
                ..PowerMeterState::power(12.5)
            }
            .into(),
            json!({ "power_w": 12.5, "energy_wh": 1234.0, "voltage_v": 227.2, "current_a": 0.112 }),
        ),
        (
            EvChargerState {
                status: EvChargerStatus::Charging,
                waiting_for: None,
                power_w: 3680.0,
                session_energy_wh: 4200.0,
                session_duration_s: None,
                current_a: 16.0,
                auto_current: None,
                phase_current_a: None,
                min_current_a: None,
                max_current_a: None,
                mode: Some(EvChargeMode::Manual),
                locked: None,
                schedule_enabled: None,
            }
            .into(),
            json!({
                "status": "charging",
                "power_w": 3680.0,
                "session_energy_wh": 4200.0,
                "current_a": 16.0,
                "mode": "manual"
            }),
        ),
        (
            EvChargerState {
                status: EvChargerStatus::Waiting,
                waiting_for: Some(EvChargerWaitReason::Power),
                power_w: 0.0,
                session_energy_wh: 4890.0,
                session_duration_s: Some(3841),
                current_a: 6.0,
                auto_current: Some(true),
                phase_current_a: Some(vec![0.0, 0.0, 0.0]),
                min_current_a: Some(6.0),
                max_current_a: Some(30.0),
                mode: Some(EvChargeMode::SolarGrid),
                locked: Some(false),
                schedule_enabled: Some(true),
            }
            .into(),
            json!({
                "status": "waiting",
                "waiting_for": "power",
                "power_w": 0.0,
                "session_energy_wh": 4890.0,
                "session_duration_s": 3841,
                "current_a": 6.0,
                "auto_current": true,
                "phase_current_a": [0.0, 0.0, 0.0],
                "min_current_a": 6.0,
                "max_current_a": 30.0,
                "mode": "solar_grid",
                "locked": false,
                "schedule_enabled": true
            }),
        ),
        (
            BatteryState {
                percent: pct(15),
                low: true,
            }
            .into(),
            json!({ "percent": 15, "low": true }),
        ),
        (
            RobotVacuumState {
                status: RobotVacuumStatus::Returning,
                cleaned_area_m2: None,
                cleaning_duration_s: None,
            }
            .into(),
            json!({ "status": "returning" }),
        ),
        (
            RobotVacuumState {
                status: RobotVacuumStatus::Cleaning,
                cleaned_area_m2: Some(14.0),
                cleaning_duration_s: Some(1_500),
            }
            .into(),
            json!({ "status": "cleaning", "cleaned_area_m2": 14.0, "cleaning_duration_s": 1500 }),
        ),
    ];

    for (state, expected_state) in cases {
        let message = ServerMessage::State(StateChange {
            target: "hall/main".parse().unwrap(),
            state: state.clone(),
        });
        assert_wire(
            &message,
            json!({
                "type": "state",
                "target": "hall/main",
                "capability": state.kind().as_str(),
                "state": expected_state
            }),
        );
    }
}

#[test]
fn full_catalog_endpoint_round_trips() {
    let capabilities = Capabilities {
        on_off: Some(OnOffCapability {
            features: Some(OnOffFeatures { read_only: true }),
            state: Some(OnOffState { on: true }),
        }),
        level: Some(LevelCapability {
            state: Some(LevelState { level: pct(10) }),
        }),
        color_temperature: Some(ColorTemperatureCapability {
            features: ColorTemperatureFeatures {
                min_kelvin: 2700,
                max_kelvin: 6500,
            },
            state: None,
        }),
        color: Some(ColorCapability::default()),
        cover: Some(CoverCapability {
            features: CoverFeatures {
                kind: CoverKind::Blind,
                position: true,
                stop: true,
            },
            state: None,
        }),
        fan: Some(FanCapability {
            features: FanFeatures {
                speed_steps: 3,
                reversible: true,
            },
            state: None,
        }),
        lock: Some(LockCapability {
            features: LockFeatures { unlatch: true },
            state: None,
        }),
        trigger: Some(TriggerCapability {}),
        camera: Some(CameraCapability {
            features: CameraFeatures {
                two_way_audio: true,
                snapshot: false,
            },
            state: None,
        }),
        doorbell: Some(DoorbellCapability {}),
        occupancy: Some(OccupancyCapability::default()),
        contact: Some(ContactCapability::default()),
        temperature: Some(TemperatureCapability::default()),
        humidity: Some(HumidityCapability::default()),
        power_meter: Some(PowerMeterCapability {
            features: PowerMeterFeatures {
                role: MeterRole::Grid,
                bidirectional: true,
            },
            state: None,
        }),
        ev_charger: Some(EvChargerCapability {
            features: EvChargerFeatures {
                min_current_a: 6.0,
                max_current_a: 32.0,
                modes: vec![EvChargeMode::Manual, EvChargeMode::Scheduled],
                pause: true,
                adjustable_current: true,
                lock: true,
                schedule: false,
            },
            state: None,
        }),
        battery: Some(BatteryCapability::default()),
        robot_vacuum: Some(RobotVacuumCapability {
            features: RobotVacuumFeatures {
                start: true,
                pause: false,
                return_to_dock: true,
            },
            state: None,
        }),
    };
    assert_eq!(capabilities.kinds(), CapabilityKind::ALL.to_vec());

    let device = Device {
        id: DeviceId::new("test_device").unwrap(),
        name: "Test device".to_owned(),
        room: None,
        online: true,
        last_seen: Some("2026-01-01T12:00:00Z".parse().unwrap()),
        endpoints: vec![Endpoint {
            id: EndpointId::main(),
            name: "Everything".to_owned(),
            endpoint_type: EndpointType::Sensor,
            room: Some(RoomId::new("hall").unwrap()),
            capabilities,
        }],
    };
    let value = serde_json::to_value(&device).unwrap();
    let keys: Vec<&str> = value["endpoints"][0]["capabilities"]
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    let mut expected: Vec<&str> = CapabilityKind::ALL.iter().map(|k| k.as_str()).collect();
    expected.sort_unstable();
    let mut sorted_keys = keys.clone();
    sorted_keys.sort_unstable();
    assert_eq!(sorted_keys, expected);
    assert_eq!(value["last_seen"], json!("2026-01-01T12:00:00Z"));
    assert_eq!(value["endpoints"][0]["room"], json!("hall"));
    assert_eq!(value["endpoints"][0]["type"], json!("sensor"));

    let back: Device = serde_json::from_value(value).unwrap();
    assert_eq!(back, device);
}

#[test]
fn snapshot_and_availability_messages() {
    assert_wire(
        &ServerMessage::Snapshot(Snapshot { devices: vec![] }),
        json!({ "type": "snapshot", "devices": [] }),
    );
    assert_wire(
        &ServerMessage::Availability(AvailabilityChange {
            device_id: DeviceId::new("living_room_fan").unwrap(),
            online: true,
        }),
        json!({ "type": "availability", "device_id": "living_room_fan", "online": true }),
    );
}

#[test]
fn dashboard_changed_message() {
    assert_wire(
        &ServerMessage::from(DashboardChanged {
            revision: "d-0123456789abcdef".to_owned(),
        }),
        json!({ "type": "dashboard_changed", "revision": "d-0123456789abcdef" }),
    );
}

#[test]
fn unknown_capabilities_are_ignored_for_forward_compatibility() {
    let capabilities: Capabilities = serde_json::from_value(json!({
        "on_off": { "state": { "on": false } },
        "air_quality": { "state": { "pm25": 3 } }
    }))
    .unwrap();
    assert_eq!(capabilities.kinds(), vec![CapabilityKind::OnOff]);
}
