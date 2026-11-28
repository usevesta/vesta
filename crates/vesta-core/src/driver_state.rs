//! Small, durable state a driver keeps per device between Hub restarts.
//!
//! Some devices cannot report everything VESTA shows: a charger whose hours
//! are only read on request, for example. Its driver keeps the last known
//! value here so the screen is not blank after a restart, and says so (the
//! value is marked as restored).
//!
//! - **One namespace per device.** The Hub hands each driver a
//!   [`DriverState`] bound to its own device (through
//!   [`DriverContext`](crate::DriverContext)), like an
//!   [`EventSink`](crate::EventSink): a driver can never read or overwrite
//!   another device's state. Within it, values are JSON under short
//!   [`StateKey`]s such as `charging_schedule`.
//! - **Never secrets.** No API keys, local keys, passwords or tokens: those
//!   live only in the Hub's `config.json`. Every store refuses values with
//!   fields named like a secret ([`check_no_secrets`]); what a driver stores
//!   is device data only.
//! - **Never in the way.** State that cannot be read (a corrupt file, a
//!   newer format) is logged and dropped by the store when it opens; a
//!   driver must work without it.
//!
//! The Hub keeps state in one JSON file per device (`vesta-hub`), or in
//! memory ([`MemoryState`]) for `vesta-hubd --dev` and tests.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::{Mutex, PoisonError};

use async_trait::async_trait;
use serde_json::Value;

use crate::driver::DriverError;

/// Longest [`StateKey`], in bytes.
pub const MAX_STATE_KEY_LEN: usize = 64;

/// The name of one value in a device's state: lowercase `snake_case`
/// segments (`[a-z0-9]+` joined by `_`), optionally namespaced with dots
/// (`charging_schedule`, `meter.calibration`), at most
/// [`MAX_STATE_KEY_LEN`] bytes.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StateKey(String);

/// Why a string is not a [`StateKey`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("`{0}` is not a state key (lowercase snake_case, dot-separated, at most 64 bytes)")]
pub struct InvalidStateKey(pub String);

impl StateKey {
    pub fn new(key: impl Into<String>) -> Result<Self, InvalidStateKey> {
        let key = key.into();
        let valid = !key.is_empty()
            && key.len() <= MAX_STATE_KEY_LEN
            && key.split('.').all(|part| {
                part.split('_').all(|segment| {
                    !segment.is_empty()
                        && segment
                            .bytes()
                            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
                })
            });
        if valid {
            Ok(Self(key))
        } else {
            Err(InvalidStateKey(key))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for StateKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A value meant for a device's state looked like a secret.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("driver state never holds secrets: refused the field `{field}`")]
pub struct SecretInState {
    /// The field's name (never its value).
    pub field: String,
}

/// Whether a field name looks like it holds a secret: an API key, a local
/// key, a password, a token, a credential...
fn secret_name(name: &str) -> bool {
    let plain: String = name
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_lowercase())
        .collect();
    plain.ends_with("key")
        || plain.ends_with("keys")
        || [
            "secret",
            "password",
            "passwd",
            "passphrase",
            "token",
            "credential",
            "apikey",
        ]
        .iter()
        .any(|word| plain.contains(word))
}

/// Refuses a value with any field (at any depth) named like a secret. A
/// guard for the rule "state never holds secrets": a driver that tries to
/// store its config by mistake fails loudly instead of writing keys to
/// disk. It cannot see secrets hidden in plain values, so drivers still
/// store device data only.
pub fn check_no_secrets(value: &Value) -> Result<(), SecretInState> {
    match value {
        Value::Object(map) => {
            for (name, inner) in map {
                if secret_name(name) {
                    return Err(SecretInState {
                        field: name.clone(),
                    });
                }
                check_no_secrets(inner)?;
            }
            Ok(())
        }
        Value::Array(items) => items.iter().try_for_each(check_no_secrets),
        _ => Ok(()),
    }
}

/// One device's durable state, handed to its driver by the Hub.
#[async_trait]
pub trait DriverState: Send + Sync {
    /// The value stored under `key`, if any. Never fails: state the store
    /// could not read was logged and dropped when it opened.
    fn get(&self, key: &StateKey) -> Option<Value>;

    /// Stores `value` under `key`, durably, before returning (`null`
    /// removes it). Refuses values with fields named like a secret
    /// ([`check_no_secrets`]). On error nothing changed; a driver should
    /// log it and carry on (the state is a convenience, never essential).
    async fn set(&self, key: &StateKey, value: Value) -> Result<(), DriverError>;
}

/// State kept in memory only: gone when the process stops
/// (`vesta-hubd --dev`, tests, tools).
#[derive(Debug, Default)]
pub struct MemoryState {
    values: Mutex<BTreeMap<StateKey, Value>>,
}

impl MemoryState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Starts with `values` (e.g. what a previous run stored, in tests).
    pub fn with_values(values: impl IntoIterator<Item = (StateKey, Value)>) -> Self {
        Self {
            values: Mutex::new(values.into_iter().collect()),
        }
    }

    /// Everything stored, by key.
    pub fn values(&self) -> BTreeMap<StateKey, Value> {
        self.values
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

#[async_trait]
impl DriverState for MemoryState {
    fn get(&self, key: &StateKey) -> Option<Value> {
        self.values
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(key)
            .cloned()
    }

    async fn set(&self, key: &StateKey, value: Value) -> Result<(), DriverError> {
        check_no_secrets(&value).map_err(DriverError::other)?;
        let mut values = self.values.lock().unwrap_or_else(PoisonError::into_inner);
        if value.is_null() {
            values.remove(key);
        } else {
            values.insert(key.clone(), value);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn keys_are_short_snake_case_names() {
        for good in ["charging_schedule", "a", "meter.calibration_2", "x1.y_z.w"] {
            assert!(StateKey::new(good).is_ok(), "{good}");
        }
        let long = "a".repeat(MAX_STATE_KEY_LEN + 1);
        for bad in [
            "",
            "Charging",
            "charging-schedule",
            "a..b",
            ".a",
            "a.",
            "a__b",
            "../etc",
            "a/b",
            long.as_str(),
        ] {
            assert!(StateKey::new(bad).is_err(), "{bad}");
        }
        assert_eq!(
            StateKey::new("charging_schedule").unwrap().to_string(),
            "charging_schedule"
        );
    }

    #[test]
    fn values_named_like_secrets_are_refused() {
        for secret in [
            json!({ "cloud_api_key": "x" }),
            json!({ "apikey": "x" }),
            json!({ "local_key": "x" }),
            json!({ "key": "x" }),
            json!({ "nested": [{ "Password": "x" }] }),
            json!({ "access_token": "x" }),
            json!({ "deep": { "deeper": { "client_secret": "x" } } }),
            json!({ "credentials": {} }),
        ] {
            let error = check_no_secrets(&secret).unwrap_err();
            assert!(!error.to_string().contains("\"x\""), "{error}");
        }
        for fine in [
            json!({ "windows": [{ "start": "22:00", "end": "06:00", "days": ["monday"] }],
                    "source": "charger", "at": "2026-11-28T10:00:00Z" }),
            json!(["key", "token"]),
            json!("password"),
            json!(null),
        ] {
            assert_eq!(check_no_secrets(&fine), Ok(()), "{fine}");
        }
    }

    #[tokio::test]
    async fn memory_state_keeps_values_until_removed() {
        let state = MemoryState::new();
        let key = StateKey::new("charging_schedule").unwrap();
        assert_eq!(state.get(&key), None);
        state.set(&key, json!({ "windows": [] })).await.unwrap();
        assert_eq!(state.get(&key), Some(json!({ "windows": [] })));
        assert!(
            state
                .set(&key, json!({ "local_key": "0123456789abcdef" }))
                .await
                .is_err()
        );
        assert_eq!(state.get(&key), Some(json!({ "windows": [] })), "unchanged");
        state.set(&key, Value::Null).await.unwrap();
        assert_eq!(state.get(&key), None);
        assert!(state.values().is_empty());

        let restored = MemoryState::with_values([(key.clone(), json!(1))]);
        assert_eq!(restored.get(&key), Some(json!(1)));
    }
}
