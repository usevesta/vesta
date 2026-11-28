//! Devices and endpoints as seen by the Hub and the frontend.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::Timestamp;
use crate::capability::Capabilities;
use crate::id::{DeviceId, EndpointId, RoomId};

/// What an endpoint *is*: decides the icon, the default widget and whether
/// opening it is sensitive. Capabilities decide the controls. Drivers report
/// a default; users may override it ("this relay is a light").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum EndpointType {
    Light,
    Outlet,
    Switch,
    Fan,
    WindowCovering,
    GarageDoor,
    Gate,
    Door,
    Lock,
    Camera,
    Doorbell,
    Sensor,
    Meter,
    EvCharger,
    RobotVacuum,
}

impl EndpointType {
    /// Opening or unlocking this endpoint gives physical access to the home,
    /// so those commands are sensitive.
    pub const fn guards_access(self) -> bool {
        matches!(
            self,
            Self::GarageDoor | Self::Gate | Self::Door | Self::Lock
        )
    }
}

/// A physical unit known to one driver, with one or more endpoints.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(optional_fields)]
pub struct Device {
    pub id: DeviceId,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub room: Option<RoomId>,
    /// Offline devices keep their last state but render as unavailable.
    pub online: bool,
    /// RFC 3339 time of the last message from the device.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_seen: Option<Timestamp>,
    pub endpoints: Vec<Endpoint>,
}

impl Device {
    pub fn endpoint(&self, id: &EndpointId) -> Option<&Endpoint> {
        self.endpoints.iter().find(|endpoint| &endpoint.id == id)
    }

    pub fn endpoint_mut(&mut self, id: &EndpointId) -> Option<&mut Endpoint> {
        self.endpoints
            .iter_mut()
            .find(|endpoint| &endpoint.id == id)
    }
}

/// One controllable function of a device (e.g. the fan, or its light).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(optional_fields)]
pub struct Endpoint {
    pub id: EndpointId,
    pub name: String,
    #[serde(rename = "type")]
    pub endpoint_type: EndpointType,
    /// Overrides the device's room for this endpoint.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub room: Option<RoomId>,
    #[serde(default)]
    pub capabilities: Capabilities,
}

/// What a driver reports about its hardware: endpoints, default types and
/// names, capabilities and features (state may be omitted). The Hub combines
/// it with the device's config (id, name, room, overrides) into a [`Device`].
/// Internal to the Hub; never sent to clients as-is.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct DeviceDescriptor {
    pub endpoints: Vec<Endpoint>,
}
