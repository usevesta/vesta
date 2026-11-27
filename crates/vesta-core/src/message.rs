//! WebSocket messages between the frontend and the Hub, tagged by `"type"`.
//!
//! On connect, reconnect, or when the Hub's subscription lags, the client
//! receives a fresh [`Snapshot`].

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::capability::CapabilityState;
use crate::command::{CommandRequest, CommandResult};
use crate::device::Device;
use crate::event::EndpointEvent;
use crate::id::{DeviceId, Target};

/// Hub → client.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMessage {
    Snapshot(Snapshot),
    State(StateChange),
    Event(EndpointEvent),
    Availability(AvailabilityChange),
    CommandResult(CommandResult),
}

/// Client → Hub.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMessage {
    Command(CommandRequest),
}

/// Every device with its full current state.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, TS)]
pub struct Snapshot {
    pub devices: Vec<Device>,
}

/// New full state of one capability of one endpoint:
/// `{ "target": "living_room_fan/light", "capability": "level", "state": { "level": 80 } }`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
pub struct StateChange {
    pub target: Target,
    #[serde(flatten)]
    pub state: CapabilityState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct AvailabilityChange {
    pub device_id: DeviceId,
    pub online: bool,
}

impl From<Snapshot> for ServerMessage {
    fn from(value: Snapshot) -> Self {
        Self::Snapshot(value)
    }
}

impl From<StateChange> for ServerMessage {
    fn from(value: StateChange) -> Self {
        Self::State(value)
    }
}

impl From<EndpointEvent> for ServerMessage {
    fn from(value: EndpointEvent) -> Self {
        Self::Event(value)
    }
}

impl From<AvailabilityChange> for ServerMessage {
    fn from(value: AvailabilityChange) -> Self {
        Self::Availability(value)
    }
}

impl From<CommandResult> for ServerMessage {
    fn from(value: CommandResult) -> Self {
        Self::CommandResult(value)
    }
}
