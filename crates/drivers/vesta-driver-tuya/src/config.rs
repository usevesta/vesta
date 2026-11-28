//! The device's driver config (`devices[].config` in `config.json`).
//!
//! ```json
//! {
//!   "device_id": "bf00000000000000000000",
//!   "local_key": "0123456789abcdef",
//!   "protocol_version": "3.5",
//!   "host": "192.0.2.20",
//!   "category": "cz",
//!   "mapping": { "1": { "code": "switch_1", "type": "Boolean", "values": {} } },
//!   "read_only": true
//! }
//! ```
//!
//! **The local key controls the device.** It lives only in the Hub's
//! `config.json` (never in git, logs, screenshots or the API).

use std::collections::BTreeMap;
use std::time::Duration;

use serde::Deserialize;
use serde_json::Value;
use vesta_core::DriverError;

use crate::crypto::{KEY_LEN, Key};
use crate::link::DEFAULT_PORT;
use crate::protocol::Version;

/// Default time between two status queries.
pub const DEFAULT_POLL_INTERVAL_MS: u64 = 10_000;
/// Never ask a device more often than this.
pub const MIN_POLL_INTERVAL_MS: u64 = 5_000;
pub const MAX_POLL_INTERVAL_MS: u64 = 300_000;

/// One data point (DP) of the device, as the Tuya cloud describes it (the
/// `mapping` of a tinytuya `devices.json`).
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct DataPoint {
    /// Standard code, e.g. `switch_1`, `cur_power`.
    pub code: String,
    /// `Boolean`, `Integer`, `Enum`, `String`, `Bitmap`, `Json`, `Raw`.
    #[serde(rename = "type", default)]
    pub kind: String,
    /// Integer: `unit`, `min`, `max`, `scale`, `step`. Enum: `range`.
    /// Bitmap: `label`. May be a JSON string for some types.
    #[serde(default)]
    pub values: Value,
}

impl DataPoint {
    fn values(&self) -> Option<serde_json::Map<String, Value>> {
        match &self.values {
            Value::Object(map) => Some(map.clone()),
            // Some types carry their values as a JSON string.
            Value::String(text) => match serde_json::from_str(text).ok()? {
                Value::Object(map) => Some(map),
                _ => None,
            },
            _ => None,
        }
    }

    /// Integer DPs: the value is `raw / 10^scale`.
    pub fn scale(&self) -> u32 {
        self.values()
            .and_then(|values| values.get("scale").and_then(Value::as_u64))
            .and_then(|scale| u32::try_from(scale).ok())
            .filter(|scale| *scale <= 9)
            .unwrap_or(0)
    }

    pub fn unit(&self) -> Option<String> {
        self.values()
            .and_then(|values| {
                values
                    .get("unit")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            })
            .filter(|unit| !unit.is_empty())
    }

    /// Enum DPs: the values it can take.
    pub fn range(&self) -> Vec<String> {
        self.values()
            .and_then(|values| values.get("range").cloned())
            .and_then(|range| serde_json::from_value(range).ok())
            .unwrap_or_default()
    }

    pub fn is(&self, kind: &str) -> bool {
        self.kind.eq_ignore_ascii_case(kind)
    }
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TuyaConfig {
    /// The device's ID (shown in the Tuya / Smart Life app under device
    /// information).
    pub device_id: String,
    /// 16 characters; see the crate README for how to get it.
    pub local_key: String,
    /// `3.3`, `3.4` or `3.5`. The importer reads it from the device's own
    /// broadcast.
    pub protocol_version: String,
    /// The device's last known address: a hint only. The device is found
    /// by its ID wherever it moves on the local network.
    #[serde(default)]
    pub host: Option<String>,
    /// TCP port of the device's local API. Default 6668 (only tests change
    /// it).
    #[serde(default = "default_port")]
    pub port: u16,
    /// Tuya product category, e.g. `cz` (socket), `kg` (switch), `tdq`
    /// (on/off switch), `sd` (robot vacuum). Unknown categories expose
    /// nothing.
    pub category: String,
    /// The device's data points, by DP number.
    #[serde(default)]
    pub mapping: BTreeMap<String, DataPoint>,
    /// Monitoring only: VESTA reads the device but never sends it a
    /// command. Every control is hidden and commands are refused.
    #[serde(default)]
    pub read_only: bool,
    #[serde(default = "default_poll_interval_ms")]
    pub poll_interval_ms: u64,
}

const fn default_port() -> u16 {
    DEFAULT_PORT
}

const fn default_poll_interval_ms() -> u64 {
    DEFAULT_POLL_INTERVAL_MS
}

impl std::fmt::Debug for TuyaConfig {
    /// Without the ID, key or address: they never reach logs.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TuyaConfig")
            .field("protocol_version", &self.protocol_version)
            .field("category", &self.category)
            .field("data_points", &self.mapping.len())
            .field("read_only", &self.read_only)
            .field("poll_interval_ms", &self.poll_interval_ms)
            .finish_non_exhaustive()
    }
}

impl TuyaConfig {
    /// Parses and validates the driver config JSON.
    pub fn from_value(value: &Value) -> Result<Self, DriverError> {
        // serde's messages may quote values (e.g. the key in the wrong
        // place): only those that name fields are passed on.
        let config = Self::deserialize(value).map_err(|error| {
            let message = error.to_string();
            DriverError::InvalidConfig(
                if message.starts_with("unknown field") || message.starts_with("missing field") {
                    message
                } else {
                    "a setting of this Tuya device has the wrong type".to_owned()
                },
            )
        })?;
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<(), DriverError> {
        let invalid = |reason: &str| Err(DriverError::InvalidConfig(reason.to_owned()));
        let id = self.device_id.trim();
        if id.is_empty() || id.len() > 64 || !id.chars().all(|c| c.is_ascii_alphanumeric()) {
            return invalid("device_id must be the device's ID (letters and digits)");
        }
        if self.key().is_none() {
            return invalid("local_key must be the device's 16-character local key");
        }
        if self.version().is_none() {
            return invalid("protocol_version must be 3.3, 3.4 or 3.5");
        }
        if let Some(host) = &self.host {
            let allowed = |c: char| c.is_ascii_alphanumeric() || c == '.' || c == '-';
            if host.is_empty() || host.len() > 253 || !host.chars().all(allowed) {
                return invalid("host must be an address such as 192.168.1.20, without a port");
            }
        }
        if self.port == 0 {
            return invalid("port must be a number from 1 to 65535");
        }
        if !(MIN_POLL_INTERVAL_MS..=MAX_POLL_INTERVAL_MS).contains(&self.poll_interval_ms) {
            return Err(DriverError::InvalidConfig(format!(
                "poll_interval_ms must be between {MIN_POLL_INTERVAL_MS} and {MAX_POLL_INTERVAL_MS}"
            )));
        }
        let category = self.category.trim();
        if category.is_empty()
            || category.len() > 16
            || !category.chars().all(|c| c.is_ascii_lowercase())
        {
            return invalid("category must be the device's Tuya category, e.g. cz");
        }
        if self.mapping.keys().any(|dp| dp.parse::<u16>().is_err()) {
            return invalid("mapping keys must be data point numbers");
        }
        Ok(())
    }

    pub fn device_id(&self) -> &str {
        self.device_id.trim()
    }

    /// The local key as AES key bytes, if it has the right length.
    pub fn key(&self) -> Option<Key> {
        let bytes = self.local_key.as_bytes();
        let printable = bytes.iter().all(|b| b.is_ascii_graphic());
        if bytes.len() != KEY_LEN || !printable {
            return None;
        }
        let mut key = [0; KEY_LEN];
        key.copy_from_slice(bytes);
        Some(key)
    }

    pub fn version(&self) -> Option<Version> {
        Version::parse(&self.protocol_version)
    }

    pub fn poll_interval(&self) -> Duration {
        Duration::from_millis(self.poll_interval_ms)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn base() -> Value {
        json!({
            "device_id": "bf00000000000000000000",
            "local_key": "0123456789abcdef",
            "protocol_version": "3.5",
            "category": "cz",
            "mapping": {
                "1": { "code": "switch_1", "type": "Boolean", "values": {} },
                "19": { "code": "cur_power", "type": "Integer",
                        "values": { "unit": "W", "min": 0, "max": 50000, "scale": 1, "step": 1 } },
                "42": { "code": "random_time", "type": "String", "values": "{\"maxlen\":255}" }
            }
        })
    }

    #[test]
    fn defaults() {
        let config = TuyaConfig::from_value(&base()).unwrap();
        assert_eq!(config.version(), Some(Version::V35));
        assert_eq!(config.key(), Some(*b"0123456789abcdef"));
        assert_eq!(config.port, 6668);
        assert_eq!(config.host, None);
        assert!(!config.read_only);
        assert_eq!(config.poll_interval(), Duration::from_secs(10));
        let power = &config.mapping["19"];
        assert_eq!(power.scale(), 1);
        assert_eq!(power.unit().as_deref(), Some("W"));
        assert!(power.is("integer"));
        assert_eq!(config.mapping["42"].scale(), 0);
    }

    #[test]
    fn debug_never_shows_secrets() {
        let mut value = base();
        value["host"] = json!("192.0.2.20");
        let config = TuyaConfig::from_value(&value).unwrap();
        let debug = format!("{config:?}");
        for secret in ["bf00000000000000000000", "0123456789abcdef", "192.0.2.20"] {
            assert!(!debug.contains(secret), "{debug}");
        }
    }

    #[test]
    fn errors_never_quote_the_key() {
        let mut value = base();
        value["local_key"] = json!(12345);
        value["unknown"] = json!("0123456789abcdef");
        let Err(DriverError::InvalidConfig(message)) = TuyaConfig::from_value(&value) else {
            panic!("should be rejected");
        };
        assert!(!message.contains("0123456789abcdef"), "{message}");
    }

    #[test]
    fn rejects_bad_configs() {
        let cases = [
            ("device_id", json!("")),
            ("device_id", json!("bf00/../x")),
            ("local_key", json!("short")),
            ("local_key", json!("0123456789abcdef0")),
            ("local_key", json!("0123456789abcde ")),
            ("protocol_version", json!("3.1")),
            ("protocol_version", json!("4")),
            ("host", json!("http://192.0.2.20")),
            ("host", json!("192.0.2.20:6668")),
            ("port", json!(0)),
            ("poll_interval_ms", json!(500)),
            ("poll_interval_ms", json!(3_600_000)),
            ("category", json!("")),
            ("category", json!("Socket!")),
            ("mapping", json!({ "one": { "code": "switch_1" } })),
            ("password", json!("x")),
        ];
        for (key, bad) in cases {
            let mut value = base();
            value[key] = bad.clone();
            assert!(
                matches!(
                    TuyaConfig::from_value(&value),
                    Err(DriverError::InvalidConfig(_))
                ),
                "{key} = {bad} should be rejected"
            );
        }
        assert!(TuyaConfig::from_value(&json!("x")).is_err());
    }
}
