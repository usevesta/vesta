//! Every JSON example in `docs/architecture/capabilities.md` must parse into
//! the Rust types and serialize back to exactly the same JSON. The doc is
//! the spec; this test keeps code and doc from drifting apart.

// Integration-test crate: helpers outside #[test] fns may panic too.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;
use vesta_core::{
    CapabilityState, ClientMessage, Command, CommandError, Device, DeviceConfig, EndpointType,
    EvChargeMode, EvChargerCommand, EvChargerStatus, EvChargerWaitReason, EventKind, LevelCommand,
    MeterRole, OnOffCommand, RejectReason, RobotVacuumCommand, RobotVacuumStatus, ServerMessage,
    Weekday,
};

const CAPABILITIES_DOC: &str = include_str!("../../../docs/architecture/capabilities.md");

fn json_blocks(doc: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut current = String::new();
    let mut in_block = false;
    for line in doc.lines() {
        let trimmed = line.trim();
        if !in_block && trimmed == "```json" {
            in_block = true;
            current.clear();
        } else if in_block && trimmed == "```" {
            in_block = false;
            blocks.push(current.clone());
        } else if in_block {
            current.push_str(line);
            current.push('\n');
        }
    }
    assert!(!in_block, "unterminated ```json block in capabilities.md");
    blocks
}

fn round_trip<T: Serialize + DeserializeOwned>(original: &Value) -> T {
    let typed: T = serde_json::from_value(original.clone())
        .unwrap_or_else(|e| panic!("doc example does not parse: {e}\n{original:#}"));
    let back = serde_json::to_value(&typed).unwrap();
    assert_eq!(&back, original, "doc example does not round-trip");
    typed
}

#[derive(Debug, Default)]
struct Seen {
    device: usize,
    command: usize,
    command_result: usize,
    event: usize,
    state: usize,
    availability: usize,
    dashboard_changed: usize,
    device_config: usize,
}

#[test]
fn every_capabilities_md_json_example_round_trips() {
    let blocks = json_blocks(CAPABILITIES_DOC);
    let mut seen = Seen::default();

    for block in &blocks {
        let value: Value = serde_json::from_str(block)
            .unwrap_or_else(|e| panic!("invalid JSON in capabilities.md: {e}\n{block}"));

        match value.get("type").and_then(Value::as_str) {
            Some("command") => {
                let ClientMessage::Command(request) = round_trip::<ClientMessage>(&value);
                match request.target.to_string().as_str() {
                    "living_room_fan/light" => {
                        assert!(!request.confirmed);
                        assert!(matches!(
                            request.command,
                            Command::Level(LevelCommand::Set {
                                transition_ms: Some(300),
                                ..
                            })
                        ));
                    }
                    "car_charger/charger" => match &request.command {
                        Command::EvCharger(EvChargerCommand::Unlock) => {
                            // Sensitive: sent only after the person confirmed.
                            assert!(request.confirmed);
                            assert!(request.command.is_sensitive(EndpointType::EvCharger));
                        }
                        Command::EvCharger(EvChargerCommand::SetChargingSchedule {
                            enabled,
                            windows,
                        }) => {
                            assert!(*enabled && !request.confirmed);
                            assert_eq!(windows.len(), 1);
                            assert_eq!(windows[0].days, [Weekday::Saturday, Weekday::Sunday]);
                            assert!(windows[0].end < windows[0].start, "past midnight");
                            assert!(request.command.is_slow());
                        }
                        other => panic!("unexpected charger command example {other:?}"),
                    },
                    other => panic!("unexpected command example for {other}"),
                }
                seen.command += 1;
            }
            Some(_) => match round_trip::<ServerMessage>(&value) {
                ServerMessage::Event(event) => {
                    assert_eq!(event.event, EventKind::DoorbellPressed);
                    seen.event += 1;
                }
                ServerMessage::CommandResult(result) => {
                    assert_eq!(
                        result.result(),
                        Err(CommandError::Rejected {
                            reason: RejectReason::ConfirmationRequired
                        })
                    );
                    seen.command_result += 1;
                }
                ServerMessage::State(state) => {
                    match (state.target.to_string().as_str(), &state.state) {
                        ("living_room_fan/light", CapabilityState::Level(_)) => {}
                        ("car_charger/charger", CapabilityState::EvCharger(charger)) => {
                            assert_eq!(charger.status, EvChargerStatus::Waiting);
                            assert_eq!(charger.waiting_for, Some(EvChargerWaitReason::Schedule));
                        }
                        (other, state) => panic!("unexpected state example {other}: {state:?}"),
                    }
                    seen.state += 1;
                }
                ServerMessage::Availability(availability) => {
                    assert!(!availability.online);
                    seen.availability += 1;
                }
                ServerMessage::DashboardChanged(changed) => {
                    assert!(!changed.revision.is_empty());
                    seen.dashboard_changed += 1;
                }
                ServerMessage::Snapshot(_) => panic!("unexpected snapshot example"),
            },
            None if value.get("driver").is_some() => {
                let device = round_trip::<DeviceConfig>(&value);
                assert_eq!(device.endpoints.len(), 1);
                seen.device_config += 1;
            }
            None if value.get("online").is_some() => {
                let device = round_trip::<Device>(&value);
                match device.id.as_str() {
                    "living_room_fan" => assert_device_example(&device),
                    "car_charger" => assert_charger_example(&device),
                    "hall_plug" => assert_watched_plug_example(&device),
                    "robot_vacuum" => assert_robot_example(&device),
                    other => panic!("unexpected device example {other}"),
                }
                seen.device += 1;
            }
            None => panic!("unclassified JSON example in capabilities.md:\n{value:#}"),
        }
    }

    assert_eq!(seen.device, 4, "{seen:?}");
    assert_eq!(seen.command, 3, "{seen:?}");
    assert_eq!(seen.command_result, 1, "{seen:?}");
    assert_eq!(seen.event, 1, "{seen:?}");
    assert_eq!(seen.state, 2, "{seen:?}");
    assert_eq!(seen.availability, 1, "{seen:?}");
    assert_eq!(seen.dashboard_changed, 1, "{seen:?}");
    assert_eq!(seen.device_config, 1, "{seen:?}");
    assert_eq!(
        blocks.len(),
        14,
        "new JSON example in capabilities.md: classify it here"
    );
}

fn assert_device_example(device: &Device) {
    assert_eq!(device.id, "living_room_fan");
    assert_eq!(device.endpoints.len(), 2);

    let fan = &device.endpoints[0];
    assert_eq!(fan.endpoint_type, EndpointType::Fan);
    let fan_cap = fan.capabilities.fan.as_ref().unwrap();
    assert_eq!(fan_cap.features.speed_steps, 6);
    assert_eq!(fan_cap.state.unwrap().speed, 3);

    let light = &device.endpoints[1];
    assert_eq!(light.endpoint_type, EndpointType::Light);
    assert_eq!(
        light
            .capabilities
            .level
            .as_ref()
            .unwrap()
            .state
            .unwrap()
            .level
            .get(),
        80
    );
    assert_eq!(
        light
            .capabilities
            .color_temperature
            .as_ref()
            .unwrap()
            .features
            .max_kelvin,
        6500
    );
}

fn assert_charger_example(device: &Device) {
    let ids: Vec<&str> = device.endpoints.iter().map(|e| e.id.as_str()).collect();
    assert_eq!(ids, ["charger", "grid", "house"]);

    let charger = &device.endpoints[0];
    assert_eq!(charger.endpoint_type, EndpointType::EvCharger);
    let cap = charger.capabilities.ev_charger.as_ref().unwrap();
    assert!(cap.features.pause && cap.features.lock && cap.features.schedule);
    assert!(cap.features.modes.is_empty());
    let state = cap.state.as_ref().unwrap();
    assert_eq!(state.status, EvChargerStatus::Charging);
    assert_eq!(state.mode, Some(EvChargeMode::Scheduled));
    assert_eq!(state.auto_current, Some(true));
    assert_eq!(cap.current_range(), (6.0, 30.0));
    // Its hours: two windows, as last saved from VESTA (it does not report
    // them), and they fit what it holds.
    let hours = cap.features.charging_schedule.unwrap();
    assert!(hours.editable && !hours.reports_windows && hours.saving_needs_internet);
    let schedule = state.charging_schedule.as_ref().unwrap();
    assert_eq!(Some(schedule.enabled), state.schedule_enabled);
    assert_eq!(hours.check(&schedule.windows), Ok(()));
    assert!(charger.capabilities.supports(&Command::EvCharger(
        EvChargerCommand::SetChargingSchedule {
            enabled: true,
            windows: schedule.windows.clone(),
        }
    )));

    let grid = device.endpoints[1]
        .capabilities
        .power_meter
        .as_ref()
        .unwrap();
    assert_eq!(grid.features.role, MeterRole::Grid);
    assert_eq!(grid.state.unwrap().limit_w, Some(5700.0));
    let house = device.endpoints[2]
        .capabilities
        .power_meter
        .as_ref()
        .unwrap();
    assert_eq!(house.features.role, MeterRole::House);
    assert_eq!(house.state.unwrap().energy_wh, None);
}

fn assert_watched_plug_example(device: &Device) {
    let plug = &device.endpoints[0];
    assert_eq!(plug.endpoint_type, EndpointType::Outlet);
    let on_off = plug.capabilities.on_off.as_ref().unwrap();
    assert!(on_off.read_only());
    // Watched only: the Hub refuses to switch it.
    assert!(
        !plug
            .capabilities
            .supports(&Command::OnOff(OnOffCommand::On))
    );
    let meter = plug.capabilities.power_meter.as_ref().unwrap();
    assert_eq!(meter.features.role, MeterRole::Load);
    let state = meter.state.unwrap();
    assert_eq!(state.voltage_v, Some(227.2));
    assert_eq!(state.energy_wh, None);
}

fn assert_robot_example(device: &Device) {
    let robot = &device.endpoints[0];
    assert_eq!(robot.endpoint_type, EndpointType::RobotVacuum);
    let vacuum = robot.capabilities.robot_vacuum.as_ref().unwrap();
    assert_eq!(
        vacuum.state.as_ref().unwrap().status,
        RobotVacuumStatus::Docked
    );
    assert!(
        robot
            .capabilities
            .supports(&Command::RobotVacuum(RobotVacuumCommand::ReturnToDock))
    );
    assert!(
        !Command::RobotVacuum(RobotVacuumCommand::Start).is_sensitive(EndpointType::RobotVacuum)
    );
    let battery = robot.capabilities.battery.as_ref().unwrap().state.unwrap();
    assert_eq!(battery.percent.get(), 100);
    assert!(!battery.low);
}
