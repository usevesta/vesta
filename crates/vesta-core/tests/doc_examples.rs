//! Every JSON example in `docs/architecture/capabilities.md` must parse into
//! the Rust types and serialize back to exactly the same JSON. The doc is
//! the spec; this test keeps code and doc from drifting apart.

// Integration-test crate: helpers outside #[test] fns may panic too.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;
use vesta_core::{
    ClientMessage, Command, CommandError, Device, DeviceConfig, EndpointType, EventKind,
    LevelCommand, RejectReason, ServerMessage,
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
                assert_eq!(request.target.to_string(), "living_room_fan/light");
                assert!(!request.confirmed);
                assert!(matches!(
                    request.command,
                    Command::Level(LevelCommand::Set {
                        transition_ms: Some(300),
                        ..
                    })
                ));
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
                    assert_eq!(state.target.to_string(), "living_room_fan/light");
                    seen.state += 1;
                }
                ServerMessage::Availability(availability) => {
                    assert!(!availability.online);
                    seen.availability += 1;
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
                assert_device_example(&device);
                seen.device += 1;
            }
            None => panic!("unclassified JSON example in capabilities.md:\n{value:#}"),
        }
    }

    assert_eq!(seen.device, 1, "{seen:?}");
    assert_eq!(seen.command, 1, "{seen:?}");
    assert_eq!(seen.command_result, 1, "{seen:?}");
    assert_eq!(seen.event, 1, "{seen:?}");
    assert_eq!(seen.state, 1, "{seen:?}");
    assert_eq!(seen.availability, 1, "{seen:?}");
    assert_eq!(seen.device_config, 1, "{seen:?}");
    assert_eq!(
        blocks.len(),
        7,
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
