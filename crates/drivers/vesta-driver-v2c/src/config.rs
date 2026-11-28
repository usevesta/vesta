//! The device's driver config (`devices[].config` in `config.json`).
//!
//! ```json
//! { "host": "192.0.2.10", "read_only": true, "meters": { "solar": true } }
//! ```

use std::net::Ipv4Addr;
use std::time::Duration;

use serde::Deserialize;
use vesta_core::DriverError;

/// Default time between two readings.
pub const DEFAULT_POLL_INTERVAL_MS: u64 = 3_000;
/// Never ask the charger more often than this: it answers slowly (up to
/// ~2 s) and sits on the home Wi-Fi.
pub const MIN_POLL_INTERVAL_MS: u64 = 2_000;
pub const MAX_POLL_INTERVAL_MS: u64 = 60_000;
/// Default time the charger has to answer one request.
pub const DEFAULT_REQUEST_TIMEOUT_MS: u64 = 3_000;
pub const MIN_REQUEST_TIMEOUT_MS: u64 = 500;
/// Stays below the Hub's 5 s command timeout.
pub const MAX_REQUEST_TIMEOUT_MS: u64 = 4_000;

/// Which of the charger's readings to show as meters. The charger cannot
/// tell whether its clamp, solar panels or a battery are installed, so the
/// installer says so here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Meters {
    /// Grid import/export and the contracted power (needs the charger's
    /// power clamp or meter). Default `true`.
    pub grid: bool,
    /// The whole home's consumption (same clamp). Default `true`.
    pub house: bool,
    /// Solar production. Also enables the solar charging modes. Default
    /// `false`.
    pub solar: bool,
    /// A home battery. Default `false`.
    pub battery: bool,
}

impl Default for Meters {
    fn default() -> Self {
        Self {
            grid: true,
            house: true,
            solar: false,
            battery: false,
        }
    }
}

#[derive(Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct V2cConfig {
    /// The charger's address on the home network: an IPv4 address or a
    /// host name, optionally with `:port` (default 80). No scheme, no path.
    pub host: String,
    #[serde(default = "default_poll_interval_ms")]
    pub poll_interval_ms: u64,
    #[serde(default = "default_request_timeout_ms")]
    pub request_timeout_ms: u64,
    /// Monitoring only: VESTA reads the charger but never sends it a
    /// command. Every control is hidden and commands are refused.
    #[serde(default)]
    pub read_only: bool,
    #[serde(default)]
    pub meters: Meters,
    /// When the charger stops answering for a while, look for it (by its ID)
    /// on the local network and follow it to its new address. Default
    /// `true`.
    #[serde(default = "default_true")]
    pub rediscover: bool,
    /// The charger's ID as shown in its app. Optional: VESTA learns it from
    /// the first reading; setting it lets VESTA find the charger again even
    /// when its address changed while VESTA was not running.
    #[serde(default)]
    pub charger_id: Option<String>,
    /// A key for V2C's cloud API (made on the v2c.cloud website, "API"),
    /// used **only** to store charging hours in the charger when someone
    /// saves them (the local API cannot). A secret: it lives only in the
    /// Hub's own `config.json`, is never logged, and never reaches a
    /// screen. Without it the hours are set in V2C's app; switching them on
    /// and off still works locally.
    #[serde(default)]
    pub cloud_api_key: Option<String>,
}

impl std::fmt::Debug for V2cConfig {
    /// Without the cloud key.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("V2cConfig")
            .field("host", &self.host)
            .field("poll_interval_ms", &self.poll_interval_ms)
            .field("request_timeout_ms", &self.request_timeout_ms)
            .field("read_only", &self.read_only)
            .field("meters", &self.meters)
            .field("rediscover", &self.rediscover)
            .field("charger_id", &self.charger_id)
            .field(
                "cloud_api_key",
                &self.cloud_api_key.as_ref().map(|_| "<hidden>"),
            )
            .finish()
    }
}

const fn default_poll_interval_ms() -> u64 {
    DEFAULT_POLL_INTERVAL_MS
}

const fn default_request_timeout_ms() -> u64 {
    DEFAULT_REQUEST_TIMEOUT_MS
}

const fn default_true() -> bool {
    true
}

/// Where the charger is: `host[:port]`, split.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Address {
    pub host: String,
    pub port: u16,
}

impl Address {
    pub(crate) const DEFAULT_PORT: u16 = 80;

    /// Parses `host` or `host:port`. Only plain host names and IPv4
    /// addresses: nothing that could turn the URL into something else.
    pub(crate) fn parse(raw: &str) -> Result<Self, DriverError> {
        let invalid = |reason: &str| Err(DriverError::InvalidConfig(format!("host: {reason}")));
        let raw = raw.trim();
        let (host, port) = match raw.rsplit_once(':') {
            Some((host, port)) => match port.parse::<u16>() {
                Ok(port) if port > 0 => (host, port),
                _ => return invalid("the port must be a number from 1 to 65535"),
            },
            None => (raw, Self::DEFAULT_PORT),
        };
        if host.is_empty() || host.len() > 253 {
            return invalid("must be an address such as 192.168.1.20");
        }
        let allowed = |c: char| c.is_ascii_alphanumeric() || c == '.' || c == '-';
        if !host.chars().all(allowed) || host.starts_with(['.', '-']) || host.ends_with('-') {
            return invalid("only an address or a host name, without http:// or a path");
        }
        Ok(Self {
            host: host.to_owned(),
            port,
        })
    }

    pub(crate) fn ipv4(&self) -> Option<Ipv4Addr> {
        self.host.parse().ok()
    }

    pub(crate) fn with_host(&self, host: Ipv4Addr) -> Self {
        Self {
            host: host.to_string(),
            port: self.port,
        }
    }

    /// `http://host[:port]` + `path`.
    pub(crate) fn url(&self, path: &str) -> String {
        if self.port == Self::DEFAULT_PORT {
            format!("http://{}{path}", self.host)
        } else {
            format!("http://{}:{}{path}", self.host, self.port)
        }
    }
}

impl std::fmt::Display for Address {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.port == Self::DEFAULT_PORT {
            f.write_str(&self.host)
        } else {
            write!(f, "{}:{}", self.host, self.port)
        }
    }
}

impl V2cConfig {
    /// Parses and validates the driver config JSON.
    pub fn from_value(value: &serde_json::Value) -> Result<Self, DriverError> {
        let config =
            Self::deserialize(value).map_err(|e| DriverError::InvalidConfig(e.to_string()))?;
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<(), DriverError> {
        let invalid = |reason: String| Err(DriverError::InvalidConfig(reason));
        Address::parse(&self.host)?;
        if !(MIN_POLL_INTERVAL_MS..=MAX_POLL_INTERVAL_MS).contains(&self.poll_interval_ms) {
            return invalid(format!(
                "poll_interval_ms must be between {MIN_POLL_INTERVAL_MS} and {MAX_POLL_INTERVAL_MS}"
            ));
        }
        if !(MIN_REQUEST_TIMEOUT_MS..=MAX_REQUEST_TIMEOUT_MS).contains(&self.request_timeout_ms) {
            return invalid(format!(
                "request_timeout_ms must be between {MIN_REQUEST_TIMEOUT_MS} and {MAX_REQUEST_TIMEOUT_MS}"
            ));
        }
        if self
            .charger_id
            .as_ref()
            .is_some_and(|id| id.trim().is_empty() || id.len() > 64)
        {
            return invalid("charger_id must be the ID shown in the charger's app".to_owned());
        }
        // Never quote the key in the message.
        if self.cloud_api_key.as_ref().is_some_and(|key| {
            let key = key.trim();
            key.is_empty() || key.len() > 512 || !key.chars().all(|c| c.is_ascii_graphic())
        }) {
            return invalid(
                "cloud_api_key must be the key made on the v2c.cloud website (no spaces)"
                    .to_owned(),
            );
        }
        Ok(())
    }

    /// The cloud key, when set.
    pub(crate) fn cloud_api_key(&self) -> Option<&str> {
        self.cloud_api_key.as_deref().map(str::trim)
    }

    pub(crate) fn address(&self) -> Result<Address, DriverError> {
        Address::parse(&self.host)
    }

    pub(crate) fn poll_interval(&self) -> Duration {
        Duration::from_millis(self.poll_interval_ms)
    }

    pub(crate) fn request_timeout(&self) -> Duration {
        Duration::from_millis(self.request_timeout_ms)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn defaults() {
        let config = V2cConfig::from_value(&json!({ "host": "192.0.2.10" })).unwrap();
        assert_eq!(config.poll_interval_ms, 3_000);
        assert_eq!(config.request_timeout_ms, 3_000);
        assert!(!config.read_only);
        assert!(config.rediscover);
        assert_eq!(config.charger_id, None);
        assert_eq!(config.cloud_api_key, None);
        assert_eq!(config.meters, Meters::default());
        assert!(config.meters.grid && config.meters.house);
        assert!(!config.meters.solar && !config.meters.battery);
        assert_eq!(
            config.address().unwrap().url("/RealTimeData"),
            "http://192.0.2.10/RealTimeData"
        );
    }

    #[test]
    fn full_config() {
        let config = V2cConfig::from_value(&json!({
            "host": "charger.local:8080",
            "poll_interval_ms": 5000,
            "request_timeout_ms": 2500,
            "read_only": true,
            "meters": { "solar": true, "battery": true, "grid": false },
            "rediscover": false,
            "charger_id": "ABC123"
        }))
        .unwrap();
        assert!(config.read_only);
        assert_eq!(
            config.meters,
            Meters {
                grid: false,
                house: true,
                solar: true,
                battery: true
            }
        );
        let address = config.address().unwrap();
        assert_eq!(address.port, 8080);
        assert_eq!(address.url("/x"), "http://charger.local:8080/x");
        assert_eq!(address.to_string(), "charger.local:8080");
        assert_eq!(address.ipv4(), None);
    }

    #[test]
    fn the_cloud_key_is_a_secret() {
        let config = V2cConfig::from_value(
            &json!({ "host": "192.0.2.10", "cloud_api_key": " k3y-SECRET-value " }),
        )
        .unwrap();
        assert_eq!(config.cloud_api_key(), Some("k3y-SECRET-value"));
        let debug = format!("{config:?}");
        assert!(!debug.contains("SECRET"), "{debug}");
        assert!(debug.contains("<hidden>"));
        // Errors never quote it either.
        let error =
            V2cConfig::from_value(&json!({ "host": "192.0.2.10", "cloud_api_key": "SECRET key" }))
                .unwrap_err()
                .to_string();
        assert!(!error.contains("SECRET"), "{error}");
    }

    #[test]
    fn rejects_bad_configs() {
        for bad in [
            json!({}),
            json!({ "host": "" }),
            json!({ "host": "http://192.0.2.10" }),
            json!({ "host": "192.0.2.10/RealTimeData" }),
            json!({ "host": "192.0.2.10:0" }),
            json!({ "host": "192.0.2.10:99999" }),
            json!({ "host": "user@192.0.2.10" }),
            json!({ "host": "192.0.2.10?x=1" }),
            json!({ "host": "-charger" }),
            json!({ "host": "a b" }),
            json!({ "host": "192.0.2.10", "poll_interval_ms": 500 }),
            json!({ "host": "192.0.2.10", "poll_interval_ms": 600_000 }),
            json!({ "host": "192.0.2.10", "request_timeout_ms": 10 }),
            json!({ "host": "192.0.2.10", "request_timeout_ms": 9_000 }),
            json!({ "host": "192.0.2.10", "charger_id": " " }),
            json!({ "host": "192.0.2.10", "meters": { "wind": true } }),
            json!({ "host": "192.0.2.10", "password": "x" }),
            json!({ "host": "192.0.2.10", "cloud_api_key": "" }),
            json!({ "host": "192.0.2.10", "cloud_api_key": "has a space" }),
            json!({ "host": "192.0.2.10", "cloud_api_key": "x".repeat(600) }),
            json!("192.0.2.10"),
        ] {
            assert!(
                matches!(
                    V2cConfig::from_value(&bad),
                    Err(DriverError::InvalidConfig(_))
                ),
                "{bad} should be rejected"
            );
        }
    }
}
