//! Importing devices from tinytuya's `devices.json` (written by
//! `python -m tinytuya wizard`) into device entries of VESTA's
//! `config.json`, so nobody edits JSON by hand.
//!
//! The protocol version and address of each device come from, in order:
//! its broadcast heard on the network right now, a tinytuya
//! `snapshot.json` (from `python -m tinytuya scan`), the `version` / `ip`
//! fields of `devices.json`. Devices whose version is unknown, and
//! categories VESTA does not support, are skipped and reported.
//!
//! The result holds every device's **local key**: the config file it goes
//! into is a secret (the hub writes it readable by its owner only).

use std::collections::{BTreeMap, HashMap, HashSet};
use std::net::Ipv4Addr;

use serde::Deserialize;
use serde_json::{Value, json};
use vesta_core::{CONFIG_SCHEMA_VERSION, HubConfig};

use crate::config::{DataPoint, TuyaConfig};
use crate::model::Model;
use crate::protocol::Version;

/// The `driver` key of Tuya devices in `config.json`.
pub const DRIVER: &str = "tuya";

#[derive(Debug, thiserror::Error)]
pub enum ImportError {
    #[error("this is not a tinytuya devices.json (a list of devices): {0}")]
    NotADeviceList(String),
    #[error("this is not a tinytuya snapshot.json: {0}")]
    NotASnapshot(String),
    #[error("the existing config is not valid: {0}")]
    BadConfig(String),
}

/// What is known about where a device is.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Hint {
    pub ip: Option<Ipv4Addr>,
    pub version: Option<Version>,
}

impl Hint {
    /// `self`, completed with what `other` knows.
    fn or(self, other: Self) -> Self {
        Self {
            ip: self.ip.or(other.ip),
            version: self.version.or(other.version),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Options {
    /// Monitoring only: VESTA never sends these devices a command.
    pub read_only: bool,
    /// Only devices with these names (case-insensitive). Empty: all.
    pub only: Vec<String>,
}

/// One device of the tinytuya list.
#[derive(Clone, Deserialize)]
struct ListedDevice {
    #[serde(default)]
    name: String,
    id: String,
    #[serde(default)]
    key: String,
    #[serde(default)]
    category: String,
    #[serde(default)]
    mapping: BTreeMap<String, DataPoint>,
    #[serde(default)]
    ip: Option<String>,
    #[serde(default)]
    version: Option<Value>,
}

/// Why a device was left out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkipReason {
    /// VESTA does not know what this kind of device does yet.
    Unsupported { category: String },
    /// Not heard on the network and no version recorded anywhere.
    VersionUnknown,
    /// The list entry has no usable local key (e.g. a sub-device).
    NoKey,
    /// Not in `only`.
    NotSelected,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skipped {
    pub name: String,
    pub reason: SkipReason,
}

/// A device ready for `config.json`.
#[derive(Clone, PartialEq)]
pub struct Entry {
    /// VESTA device id (from the name).
    pub id: String,
    pub name: String,
    pub version: Version,
    /// The device was heard on the network during the import.
    pub seen: bool,
    pub has_address: bool,
    /// The `devices[]` entry, local key included.
    pub json: Value,
}

impl std::fmt::Debug for Entry {
    /// Without the device's config (it holds the key).
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Entry")
            .field("id", &self.id)
            .field("version", &self.version)
            .field("seen", &self.seen)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, Default)]
pub struct Imported {
    pub entries: Vec<Entry>,
    pub skipped: Vec<Skipped>,
}

/// A VESTA id from a device name: lowercase ASCII `snake_case`, accents
/// dropped ("Batería 20 V" → `bateria_20_v`).
pub fn device_slug(name: &str) -> String {
    let mut slug = String::new();
    for c in name.chars().flat_map(char::to_lowercase) {
        let plain = match c {
            'á' | 'à' | 'ä' | 'â' | 'ã' | 'å' => 'a',
            'é' | 'è' | 'ë' | 'ê' => 'e',
            'í' | 'ì' | 'ï' | 'î' => 'i',
            'ó' | 'ò' | 'ö' | 'ô' | 'õ' => 'o',
            'ú' | 'ù' | 'ü' | 'û' => 'u',
            'ñ' => 'n',
            'ç' => 'c',
            other => other,
        };
        if plain.is_ascii_lowercase() || plain.is_ascii_digit() {
            slug.push(plain);
        } else if !slug.is_empty() && !slug.ends_with('_') {
            slug.push('_');
        }
    }
    let mut slug = slug.trim_end_matches('_').to_owned();
    slug.truncate(48);
    let slug = slug.trim_end_matches('_').to_owned();
    if slug.is_empty() {
        "tuya_device".to_owned()
    } else {
        slug
    }
}

fn version_of(value: &Value) -> Option<Version> {
    match value {
        Value::String(text) => Version::parse(text),
        Value::Number(number) => Version::parse(&number.to_string()),
        _ => None,
    }
}

/// Where tinytuya's `scan` found each device (`snapshot.json`), by device
/// ID.
pub fn snapshot_hints(snapshot_json: &str) -> Result<HashMap<String, Hint>, ImportError> {
    let value: Value = serde_json::from_str(snapshot_json)
        .map_err(|error| ImportError::NotASnapshot(error.to_string()))?;
    let devices = value
        .get("devices")
        .and_then(Value::as_array)
        .ok_or_else(|| ImportError::NotASnapshot("no `devices` list".to_owned()))?;
    let mut hints = HashMap::new();
    for device in devices {
        let Some(id) = device.get("id").and_then(Value::as_str) else {
            continue;
        };
        let hint = Hint {
            ip: device
                .get("ip")
                .and_then(Value::as_str)
                .and_then(|ip| ip.parse().ok()),
            version: device
                .get("ver")
                .or_else(|| device.get("version"))
                .and_then(version_of),
        };
        hints.insert(id.to_owned(), hint);
    }
    Ok(hints)
}

/// The devices a `config.json` already has.
#[derive(Debug, Clone, Default)]
pub struct Existing {
    /// Every device id in use.
    pub ids: HashSet<String>,
    /// VESTA id of each Tuya device already there, by Tuya device ID.
    pub tuya: HashMap<String, String>,
}

/// Turns tinytuya's device list into `config.json` device entries.
///
/// `heard` is what the network said during the import (live), `snapshot`
/// what an earlier scan found. New VESTA ids come from the names; a device
/// already in the config keeps its id.
pub fn import(
    devices_json: &str,
    heard: &HashMap<String, Hint>,
    snapshot: &HashMap<String, Hint>,
    existing: &Existing,
    options: &Options,
) -> Result<Imported, ImportError> {
    let devices: Vec<ListedDevice> = serde_json::from_str(devices_json).map_err(|error| {
        // serde quotes offending values; only its position is passed on.
        ImportError::NotADeviceList(format!("line {}, column {}", error.line(), error.column()))
    })?;
    let wanted: Vec<String> = options
        .only
        .iter()
        .map(|name| name.trim().to_lowercase())
        .collect();
    let mut used: HashSet<String> = existing.ids.clone();
    let mut imported = Imported::default();
    for device in devices {
        let name = device.name.trim().to_owned();
        let skip = |reason| Skipped {
            name: name.clone(),
            reason,
        };
        if !wanted.is_empty() && !wanted.contains(&name.to_lowercase()) {
            imported.skipped.push(skip(SkipReason::NotSelected));
            continue;
        }
        if device.key.len() != 16 || !device.key.bytes().all(|b| b.is_ascii_graphic()) {
            imported.skipped.push(skip(SkipReason::NoKey));
            continue;
        }
        let model = Model::new(device.category.trim(), &device.mapping).ok();
        if matches!(model, None | Some(Model::Unsupported)) {
            imported.skipped.push(skip(SkipReason::Unsupported {
                category: device.category.clone(),
            }));
            continue;
        }
        let listed = Hint {
            ip: device.ip.as_deref().and_then(|ip| ip.parse().ok()),
            version: device.version.as_ref().and_then(version_of),
        };
        let live = heard.get(&device.id).copied();
        let hint = live
            .unwrap_or_default()
            .or(snapshot.get(&device.id).copied().unwrap_or_default())
            .or(listed);
        let Some(version) = hint.version else {
            imported.skipped.push(skip(SkipReason::VersionUnknown));
            continue;
        };

        let id = match existing.tuya.get(&device.id) {
            Some(id) => id.clone(),
            None => {
                let base = device_slug(if name.is_empty() {
                    "tuya device"
                } else {
                    &name
                });
                let mut id = base.clone();
                let mut n = 2;
                while used.contains(&id) {
                    id = format!("{base}_{n}");
                    n += 1;
                }
                id
            }
        };
        used.insert(id.clone());

        let mut config = json!({
            "device_id": device.id,
            "local_key": device.key,
            "protocol_version": version.as_str(),
            "category": device.category.trim(),
            "mapping": device.mapping.iter().map(|(dp, point)| {
                (dp.clone(), json!({ "code": point.code, "type": point.kind, "values": point.values }))
            }).collect::<serde_json::Map<String, Value>>(),
            "read_only": options.read_only,
        });
        if let Some(ip) = hint.ip {
            config["host"] = json!(ip.to_string());
        }
        // Never write an entry the driver would refuse.
        if TuyaConfig::from_value(&config).is_err() {
            imported.skipped.push(skip(SkipReason::NoKey));
            continue;
        }
        let display = if name.is_empty() {
            "Tuya device".to_owned()
        } else {
            name.clone()
        };
        imported.entries.push(Entry {
            json: json!({ "id": id, "name": display, "driver": DRIVER, "config": config }),
            id,
            name: display,
            version,
            seen: live.is_some(),
            has_address: hint.ip.is_some(),
        });
    }
    Ok(imported)
}

/// The devices of a `config.json` (none when there is no file yet).
pub fn existing(config_json: Option<&str>) -> Result<Existing, ImportError> {
    let Some(text) = config_json else {
        return Ok(Existing::default());
    };
    let config =
        HubConfig::from_json(text).map_err(|error| ImportError::BadConfig(error.to_string()))?;
    let mut existing = Existing::default();
    for device in &config.devices {
        existing.ids.insert(device.id.to_string());
        if device.driver == DRIVER
            && let Some(tuya_id) = device.config.get("device_id").and_then(Value::as_str)
        {
            existing
                .tuya
                .insert(tuya_id.to_owned(), device.id.to_string());
        }
    }
    Ok(existing)
}

/// Adds `entries` to a `config.json` (a new one when `config_json` is
/// `None`). An entry replaces the Tuya device with the same device ID
/// (keeping its name, room and presentation); every other device and
/// setting is kept as it is.
pub fn merge(config_json: Option<&str>, entries: &[Entry]) -> Result<String, ImportError> {
    let mut config: Value = match config_json {
        Some(text) => {
            HubConfig::from_json(text)
                .map_err(|error| ImportError::BadConfig(error.to_string()))?;
            serde_json::from_str(text).map_err(|error| ImportError::BadConfig(error.to_string()))?
        }
        None => json!({
            "schema_version": CONFIG_SCHEMA_VERSION,
            "home_name": "My Home",
            "hub": { "bind_address": "127.0.0.1", "port": 8080 },
            "devices": []
        }),
    };
    let devices = config
        .get_mut("devices")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| ImportError::BadConfig("no `devices` list".to_owned()))?;
    for entry in entries {
        let tuya_id = entry.json["config"]["device_id"].as_str();
        let position = devices.iter().position(|existing| {
            existing["driver"].as_str() == Some(DRIVER)
                && existing["config"]["device_id"].as_str() == tuya_id
        });
        match position {
            Some(index) => {
                // Keep what was set up earlier: id, name, room, presentation.
                let mut replacement = entry.json.clone();
                for kept in ["id", "name", "room", "endpoints"] {
                    if let Some(value) = devices[index].get(kept) {
                        replacement[kept] = value.clone();
                    }
                }
                devices[index] = replacement;
            }
            None => devices.push(entry.json.clone()),
        }
    }
    let text = serde_json::to_string_pretty(&config)
        .map_err(|error| ImportError::BadConfig(error.to_string()))?;
    HubConfig::from_json(&text).map_err(|error| ImportError::BadConfig(error.to_string()))?;
    Ok(text + "\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tinytuya `devices.json` with made-up devices (placeholders only).
    fn devices() -> String {
        json!([
            {
                "name": "Lámpara del salón", "id": "bf00000000000000000001", "key": "0123456789abcdef",
                "mac": "00:00:00:00:00:01", "uuid": "x", "category": "cz", "product_name": "Smart Plug",
                "mapping": {
                    "1": { "code": "switch_1", "type": "Boolean", "values": {} },
                    "19": { "code": "cur_power", "type": "Integer", "values": { "unit": "W", "scale": 1 } }
                }
            },
            {
                "name": "Robot", "id": "bf00000000000000000002", "key": "fedcba9876543210",
                "category": "sd",
                "mapping": { "5": { "code": "status", "type": "Enum", "values": { "range": ["charging"] } } }
            },
            {
                "name": "Door sensor", "id": "bf00000000000000000003", "key": "0123456789abcdef",
                "category": "mcs", "mapping": {}
            },
            {
                "name": "Far away plug", "id": "bf00000000000000000004", "key": "0123456789abcdef",
                "category": "cz", "mapping": { "1": { "code": "switch_1", "type": "Boolean" } }
            },
            {
                "name": "Hub child", "id": "bf00000000000000000005", "key": "",
                "category": "cz", "mapping": { "1": { "code": "switch_1", "type": "Boolean" } }
            },
            {
                "name": "Lámpara del salón", "id": "bf00000000000000000006", "key": "0123456789abcdef",
                "category": "kg", "version": "3.3", "ip": "192.0.2.66",
                "mapping": { "1": { "code": "switch_1", "type": "Boolean" } }
            }
        ])
        .to_string()
    }

    fn heard() -> HashMap<String, Hint> {
        HashMap::from([(
            "bf00000000000000000001".to_owned(),
            Hint {
                ip: Some(Ipv4Addr::new(192, 0, 2, 61)),
                version: Some(Version::V35),
            },
        )])
    }

    fn snapshot() -> HashMap<String, Hint> {
        snapshot_hints(
            &json!({ "timestamp": 1, "devices": [
                { "id": "bf00000000000000000002", "ip": "192.0.2.62", "ver": "3.5", "key": "x" },
                { "id": "bf00000000000000000001", "ip": "192.0.2.99", "ver": "3.3" }
            ] })
            .to_string(),
        )
        .unwrap()
    }

    #[test]
    fn slugs() {
        assert_eq!(device_slug("Lámpara del salón"), "lampara_del_salon");
        assert_eq!(device_slug("  Batería 20 V / 4 Ah "), "bateria_20_v_4_ah");
        assert_eq!(device_slug("!!!"), "tuya_device");
        assert_eq!(device_slug("Ñu"), "nu");
        assert!(device_slug(&"long name ".repeat(20)).len() <= 48);
    }

    #[test]
    fn imports_supported_devices_it_can_reach() {
        let imported = import(
            &devices(),
            &heard(),
            &snapshot(),
            &Existing::default(),
            &Options {
                read_only: true,
                only: Vec::new(),
            },
        )
        .unwrap();
        let ids: Vec<&str> = imported.entries.iter().map(|e| e.id.as_str()).collect();
        assert_eq!(ids, ["lampara_del_salon", "robot", "lampara_del_salon_2"]);

        let plug = &imported.entries[0];
        // What is heard now wins over the snapshot.
        assert_eq!(plug.version, Version::V35);
        assert!(plug.seen);
        assert_eq!(
            plug.json,
            json!({
                "id": "lampara_del_salon",
                "name": "Lámpara del salón",
                "driver": "tuya",
                "config": {
                    "device_id": "bf00000000000000000001",
                    "local_key": "0123456789abcdef",
                    "protocol_version": "3.5",
                    "host": "192.0.2.61",
                    "category": "cz",
                    "mapping": {
                        "1": { "code": "switch_1", "type": "Boolean", "values": {} },
                        "19": { "code": "cur_power", "type": "Integer", "values": { "unit": "W", "scale": 1 } }
                    },
                    "read_only": true
                }
            })
        );
        let robot = &imported.entries[1];
        assert!(!robot.seen);
        assert_eq!(robot.json["config"]["host"], json!("192.0.2.62"));
        assert_eq!(robot.version, Version::V35);
        let listed = &imported.entries[2];
        assert_eq!(listed.version, Version::V33);
        assert_eq!(listed.json["config"]["host"], json!("192.0.2.66"));

        let reasons: Vec<(&str, &SkipReason)> = imported
            .skipped
            .iter()
            .map(|s| (s.name.as_str(), &s.reason))
            .collect();
        assert_eq!(
            reasons,
            [
                (
                    "Door sensor",
                    &SkipReason::Unsupported {
                        category: "mcs".to_owned()
                    }
                ),
                ("Far away plug", &SkipReason::VersionUnknown),
                ("Hub child", &SkipReason::NoKey),
            ]
        );
        assert!(!format!("{imported:?}").contains("0123456789abcdef"));
    }

    #[test]
    fn only_picks_by_name() {
        let imported = import(
            &devices(),
            &heard(),
            &snapshot(),
            &Existing::default(),
            &Options {
                read_only: false,
                only: vec!["robot".to_owned()],
            },
        )
        .unwrap();
        assert_eq!(imported.entries.len(), 1);
        assert_eq!(
            imported.entries[0].json["config"]["read_only"],
            json!(false)
        );
        assert!(
            imported
                .skipped
                .iter()
                .all(|s| s.reason == SkipReason::NotSelected)
        );
    }

    #[test]
    fn merging_keeps_everything_else() {
        let existing = json!({
            "schema_version": 1,
            "home_name": "Test Home",
            "hub": { "bind_address": "127.0.0.1", "port": 8099 },
            "devices": [
                { "id": "car_charger", "name": "Car Charger", "driver": "v2c",
                  "config": { "host": "192.0.2.10", "read_only": true } },
                { "id": "old_name", "name": "Living room lamp", "room": "living_room", "driver": "tuya",
                  "config": { "device_id": "bf00000000000000000001", "local_key": "aaaaaaaaaaaaaaaa",
                              "protocol_version": "3.3", "category": "cz" } }
            ]
        })
        .to_string();
        let known = super::existing(Some(&existing)).unwrap();
        assert_eq!(
            known.ids,
            HashSet::from(["car_charger".to_owned(), "old_name".to_owned()])
        );
        let imported = import(
            &devices(),
            &heard(),
            &snapshot(),
            &known,
            &Options {
                read_only: true,
                only: vec!["Lámpara del salón".to_owned(), "Robot".to_owned()],
            },
        )
        .unwrap();
        let merged: Value =
            serde_json::from_str(&merge(Some(&existing), &imported.entries).unwrap()).unwrap();
        let devices = merged["devices"].as_array().unwrap();
        assert_eq!(devices.len(), 4);
        assert_eq!(merged["hub"]["port"], json!(8099));
        assert_eq!(devices[0]["driver"], json!("v2c"));
        // Re-imported: new key and version, same id, name and room.
        assert_eq!(devices[1]["id"], json!("old_name"));
        assert_eq!(devices[1]["name"], json!("Living room lamp"));
        assert_eq!(devices[1]["room"], json!("living_room"));
        assert_eq!(devices[1]["config"]["local_key"], json!("0123456789abcdef"));
        assert_eq!(devices[1]["config"]["protocol_version"], json!("3.5"));
        assert_eq!(devices[2]["id"], json!("robot"));

        let fresh: Value = serde_json::from_str(&merge(None, &imported.entries).unwrap()).unwrap();
        assert_eq!(fresh["schema_version"], json!(1));
        assert_eq!(fresh["devices"].as_array().unwrap().len(), 3);
        assert!(HubConfig::from_json(&fresh.to_string()).is_ok());
    }

    #[test]
    fn bad_inputs_are_typed_errors_without_secrets() {
        let bad = r#"[{ "name": "x", "id": 5, "key": "0123456789abcdef" }]"#;
        let error = import(
            bad,
            &heard(),
            &snapshot(),
            &Existing::default(),
            &Options::default(),
        )
        .unwrap_err()
        .to_string();
        assert!(!error.contains("0123456789abcdef"), "{error}");
        assert!(matches!(
            snapshot_hints("[]"),
            Err(ImportError::NotASnapshot(_))
        ));
        assert!(matches!(
            merge(Some("{}"), &[]),
            Err(ImportError::BadConfig(_))
        ));
    }
}
