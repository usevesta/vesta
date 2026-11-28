//! `config.json`: hub settings and devices (see `config.example.json`).
//!
//! Hub-only. This type is never sent to clients and has no TypeScript
//! binding: driver `config` holds hosts, keys and passwords. `Debug` output
//! redacts it so it cannot end up in logs.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::net::{IpAddr, Ipv4Addr};

use serde::{Deserialize, Serialize};

use crate::capability::MeterRole;
use crate::device::{Endpoint, EndpointType};
use crate::id::{DeviceId, EndpointId, IdError, RoomId};

/// `schema_version` understood by this build for `config.json`.
pub const CONFIG_SCHEMA_VERSION: u32 = 1;

const REDACTED: &str = "<redacted>";

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("invalid JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("unsupported schema_version {found} (this build understands {supported})")]
    UnsupportedSchemaVersion { found: u32, supported: u32 },
    #[error("{setting} must bind to a loopback address, got {address}")]
    NotLoopback {
        setting: &'static str,
        address: IpAddr,
    },
    #[error("device `{0}` is defined more than once")]
    DuplicateDevice(DeviceId),
    #[error("page `{0}` is defined more than once")]
    DuplicatePage(String),
    #[error("page id `{page}` is invalid: {source}")]
    InvalidPageId {
        page: String,
        #[source]
        source: IdError,
    },
    #[error("{found} pages, at most {max} are allowed")]
    TooManyPages { found: usize, max: usize },
    #[error("page `{page}`: {reason}")]
    InvalidLayout { page: String, reason: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HubConfig {
    pub schema_version: u32,
    pub home_name: String,
    #[serde(default)]
    pub hub: HubSettings,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notifications: Option<NotificationsConfig>,
    #[serde(default)]
    pub devices: Vec<DeviceConfig>,
}

impl HubConfig {
    /// Parses and validates a `config.json` document.
    pub fn from_json(json: &str) -> Result<Self, ConfigError> {
        let config: Self = serde_json::from_str(json)?;
        config.validate()?;
        Ok(config)
    }

    /// Checks invariants serde cannot express: schema version, loopback-only
    /// binds (the Hub never exposes plain HTTP on the LAN, go2rtc's API stays
    /// local) and unique device IDs.
    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.schema_version != CONFIG_SCHEMA_VERSION {
            return Err(ConfigError::UnsupportedSchemaVersion {
                found: self.schema_version,
                supported: CONFIG_SCHEMA_VERSION,
            });
        }
        if !self.hub.bind_address.is_loopback() {
            return Err(ConfigError::NotLoopback {
                setting: "hub.bind_address",
                address: self.hub.bind_address,
            });
        }
        if !self.hub.go2rtc.api_bind_address.is_loopback() {
            return Err(ConfigError::NotLoopback {
                setting: "hub.go2rtc.api_bind_address",
                address: self.hub.go2rtc.api_bind_address,
            });
        }
        let mut seen = BTreeSet::new();
        for device in &self.devices {
            if !seen.insert(&device.id) {
                return Err(ConfigError::DuplicateDevice(device.id.clone()));
            }
        }
        Ok(())
    }

    pub fn device(&self, id: &DeviceId) -> Option<&DeviceConfig> {
        self.devices.iter().find(|device| &device.id == id)
    }
}

/// Local listener settings. Every field has a default.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct HubSettings {
    pub bind_address: IpAddr,
    pub port: u16,
    pub go2rtc: Go2rtcSettings,
}

impl Default for HubSettings {
    fn default() -> Self {
        Self {
            bind_address: IpAddr::V4(Ipv4Addr::LOCALHOST),
            port: 8080,
            go2rtc: Go2rtcSettings::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Go2rtcSettings {
    pub api_bind_address: IpAddr,
    pub api_port: u16,
    pub webrtc_port: u16,
}

impl Default for Go2rtcSettings {
    fn default() -> Self {
        Self {
            api_bind_address: IpAddr::V4(Ipv4Addr::LOCALHOST),
            api_port: 1984,
            webrtc_port: 8555,
        }
    }
}

/// Push notification delivery, tagged by `provider`.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "provider", rename_all = "snake_case")]
pub enum NotificationsConfig {
    /// A private ntfy topic URL. Anyone who knows it can read the
    /// notifications, so it is treated as a secret.
    Ntfy { endpoint: String },
}

impl fmt::Debug for NotificationsConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Ntfy { .. } => f.debug_struct("Ntfy").field("endpoint", &REDACTED).finish(),
        }
    }
}

/// One device: connection settings for its driver plus optional
/// presentation overrides. Never declares capabilities.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct DeviceConfig {
    pub id: DeviceId,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub room: Option<RoomId>,
    /// Driver key resolved by `vesta-drivers`. Never exposed by the API.
    pub driver: String,
    /// Driver-specific settings, opaque to everything but the driver. Never
    /// exposed by the API and redacted from `Debug`.
    #[serde(default = "empty_object")]
    pub config: serde_json::Value,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub endpoints: BTreeMap<EndpointId, EndpointOverride>,
}

fn empty_object() -> serde_json::Value {
    serde_json::Value::Object(serde_json::Map::new())
}

impl fmt::Debug for DeviceConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DeviceConfig")
            .field("id", &self.id)
            .field("name", &self.name)
            .field("room", &self.room)
            .field("driver", &self.driver)
            .field("config", &REDACTED)
            .field("endpoints", &self.endpoints)
            .finish()
    }
}

/// Presentation overrides for one endpoint, set from the UI.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct EndpointOverride {
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    pub endpoint_type: Option<EndpointType>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub room: Option<RoomId>,
    /// `power_meter.role`: where the clamp sits, which only the installer
    /// knows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<MeterRole>,
}

impl EndpointOverride {
    /// Applies the overrides to an endpoint reported by a driver. A `role`
    /// override is ignored if the endpoint has no power meter.
    pub fn apply_to(&self, endpoint: &mut Endpoint) {
        if let Some(endpoint_type) = self.endpoint_type {
            endpoint.endpoint_type = endpoint_type;
        }
        if let Some(name) = &self.name {
            endpoint.name.clone_from(name);
        }
        if let Some(room) = &self.room {
            endpoint.room = Some(room.clone());
        }
        if let (Some(role), Some(meter)) = (self.role, endpoint.capabilities.power_meter.as_mut()) {
            meter.features.role = role;
        }
    }
}
