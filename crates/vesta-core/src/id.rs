//! Stable identifiers and endpoint addressing.
//!
//! Device, endpoint and room IDs are lowercase `snake_case` (`[a-z0-9]+`
//! segments joined by single underscores, at most [`MAX_ID_LEN`] bytes). An
//! endpoint is addressed as a [`Target`]: `<device_id>/<endpoint_id>`.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use ts_rs::TS;

/// Maximum length of a single identifier, in bytes.
pub const MAX_ID_LEN: usize = 64;

/// Endpoint ID used by single-function devices.
pub const MAIN_ENDPOINT: &str = "main";

/// Why a string is not a valid identifier or target.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum IdError {
    #[error("identifier is empty")]
    Empty,
    #[error("identifier is longer than {MAX_ID_LEN} bytes")]
    TooLong,
    #[error("identifier `{0}` is not lowercase snake_case")]
    NotSnakeCase(String),
    #[error("target `{0}` is not `<device_id>/<endpoint_id>`")]
    InvalidTarget(String),
}

pub(crate) fn validate_id(id: &str) -> Result<(), IdError> {
    if id.is_empty() {
        return Err(IdError::Empty);
    }
    if id.len() > MAX_ID_LEN {
        return Err(IdError::TooLong);
    }
    let segments_ok = id.split('_').all(|segment| {
        !segment.is_empty()
            && segment
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
    });
    if segments_ok {
        Ok(())
    } else {
        Err(IdError::NotSnakeCase(id.to_owned()))
    }
}

macro_rules! id_type {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, TS)]
        pub struct $name(String);

        impl $name {
            /// Validates `id` as lowercase `snake_case`.
            pub fn new(id: impl Into<String>) -> Result<Self, IdError> {
                let id = id.into();
                validate_id(&id)?;
                Ok(Self(id))
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl FromStr for $name {
            type Err = IdError;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                Self::new(s)
            }
        }

        impl TryFrom<String> for $name {
            type Error = IdError;

            fn try_from(value: String) -> Result<Self, Self::Error> {
                Self::new(value)
            }
        }

        impl TryFrom<&str> for $name {
            type Error = IdError;

            fn try_from(value: &str) -> Result<Self, Self::Error> {
                Self::new(value)
            }
        }

        impl From<$name> for String {
            fn from(value: $name) -> Self {
                value.0
            }
        }

        impl AsRef<str> for $name {
            fn as_ref(&self) -> &str {
                &self.0
            }
        }

        impl PartialEq<str> for $name {
            fn eq(&self, other: &str) -> bool {
                self.0 == other
            }
        }

        impl PartialEq<&str> for $name {
            fn eq(&self, other: &&str) -> bool {
                self.0 == *other
            }
        }

        impl Serialize for $name {
            fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.serialize_str(&self.0)
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                let raw = String::deserialize(deserializer)?;
                Self::new(raw).map_err(de::Error::custom)
            }
        }
    };
}

id_type! {
    /// Stable ID of a physical device, e.g. `living_room_fan`.
    DeviceId
}

id_type! {
    /// Stable ID of one function of a device, e.g. `light`. Single-function
    /// devices use `main`.
    EndpointId
}

id_type! {
    /// Stable ID of a room, e.g. `living_room`.
    RoomId
}

impl EndpointId {
    /// The endpoint of a single-function device.
    pub fn main() -> Self {
        Self(MAIN_ENDPOINT.to_owned())
    }
}

/// Address of one endpoint: `<device_id>/<endpoint_id>`, e.g.
/// `living_room_fan/light`. Serialized as a plain string.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, TS)]
#[ts(type = "string")]
pub struct Target {
    device: DeviceId,
    endpoint: EndpointId,
}

impl Target {
    pub fn new(device: DeviceId, endpoint: EndpointId) -> Self {
        Self { device, endpoint }
    }

    pub fn device(&self) -> &DeviceId {
        &self.device
    }

    pub fn endpoint(&self) -> &EndpointId {
        &self.endpoint
    }

    pub fn into_parts(self) -> (DeviceId, EndpointId) {
        (self.device, self.endpoint)
    }
}

impl fmt::Display for Target {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.device, self.endpoint)
    }
}

impl FromStr for Target {
    type Err = IdError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (device, endpoint) = s
            .split_once('/')
            .ok_or_else(|| IdError::InvalidTarget(s.to_owned()))?;
        Ok(Self {
            device: DeviceId::new(device)?,
            endpoint: EndpointId::new(endpoint)?,
        })
    }
}

impl TryFrom<&str> for Target {
    type Error = IdError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        value.parse()
    }
}

impl Serialize for Target {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Target {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        raw.parse().map_err(de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_snake_case_ids() {
        for id in [
            "main",
            "light",
            "living_room_fan",
            "relay_2",
            "2nd_floor",
            "a",
        ] {
            assert!(DeviceId::new(id).is_ok(), "{id} should be valid");
        }
    }

    #[test]
    fn rejects_invalid_ids() {
        assert_eq!(DeviceId::new(""), Err(IdError::Empty));
        assert_eq!(
            DeviceId::new("a".repeat(MAX_ID_LEN + 1)),
            Err(IdError::TooLong)
        );
        assert!(DeviceId::new("a".repeat(MAX_ID_LEN)).is_ok());
        for id in [
            "Living_room",
            "living-room",
            "living room",
            "_light",
            "light_",
            "living__room",
            "luz_del_salón",
            "a/b",
            "light.main",
        ] {
            assert!(
                matches!(DeviceId::new(id), Err(IdError::NotSnakeCase(_))),
                "{id} should be rejected"
            );
        }
    }

    #[test]
    fn parses_and_displays_targets() {
        let target: Target = "living_room_fan/light".parse().unwrap();
        assert_eq!(target.device(), "living_room_fan");
        assert_eq!(target.endpoint(), "light");
        assert_eq!(target.to_string(), "living_room_fan/light");

        let main = Target::new(
            DeviceId::new("front_door_strike").unwrap(),
            EndpointId::main(),
        );
        assert_eq!(main.to_string(), "front_door_strike/main");
    }

    #[test]
    fn rejects_invalid_targets() {
        for raw in [
            "living_room_fan",
            "",
            "/light",
            "fan/",
            "a/b/c",
            "Fan/light",
            " a/b",
        ] {
            assert!(raw.parse::<Target>().is_err(), "{raw:?} should be rejected");
        }
        assert_eq!(
            "living_room_fan".parse::<Target>(),
            Err(IdError::InvalidTarget("living_room_fan".to_owned()))
        );
    }

    #[test]
    fn serializes_as_strings() {
        let target: Target = "front_doorbell/main".parse().unwrap();
        assert_eq!(
            serde_json::to_string(&target).unwrap(),
            r#""front_doorbell/main""#
        );
        let back: Target = serde_json::from_str(r#""front_doorbell/main""#).unwrap();
        assert_eq!(back, target);

        assert!(serde_json::from_str::<Target>(r#""front_doorbell""#).is_err());
        assert!(serde_json::from_str::<DeviceId>(r#""Front""#).is_err());
        assert!(serde_json::from_str::<EndpointId>("42").is_err());
    }
}
