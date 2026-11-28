//! Where drivers keep their per-device state between restarts (see
//! `vesta_core::driver_state`): e.g. a charger's last known charging hours.
//!
//! - [`StateStorage::Memory`]: gone when the Hub stops (`vesta-hubd --dev`,
//!   the app's simulated home, tests).
//! - [`StateStorage::Dir`]: one JSON file per device, `<dir>/<device_id>.json`
//!   (device ids are lowercase `snake_case`, so always a plain file name):
//!
//!   ```json
//!   { "schema_version": 1,
//!     "values": { "charging_schedule": { "windows": [], "source": "charger", "at": "…" } } }
//!   ```
//!
//!   Written like the dashboard: a temporary file in the same directory,
//!   flushed to disk, then renamed over the old one, so a power cut leaves
//!   the old or the new state, never half of it. The directory is created
//!   on the first write.
//!
//! State never holds secrets: values with fields named like one are refused
//! when written and dropped when read ([`vesta_core::check_no_secrets`]).
//! A file that cannot be read (corrupt, written by a newer VESTA,
//! unreadable) is logged and ignored, and replaced by the next write: it
//! never stops the Hub or a device.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use vesta_core::{
    DeviceId, DriverError, DriverState, MemoryState, StateKey, async_trait, check_no_secrets,
};

use crate::dashboard::write_atomically;

/// Version of the per-device state files.
pub const STATE_SCHEMA_VERSION: u32 = 1;

/// Where the Hub keeps its drivers' per-device state.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum StateStorage {
    /// In memory only: forgotten when the Hub stops.
    #[default]
    Memory,
    /// One JSON file per device in this directory.
    Dir(PathBuf),
}

impl StateStorage {
    /// The state of one device, for its driver.
    pub fn open(&self, device: &DeviceId) -> Arc<dyn DriverState> {
        match self {
            Self::Memory => Arc::new(MemoryState::new()),
            Self::Dir(dir) => Arc::new(FileState::open(dir, device)),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct StateFile {
    schema_version: u32,
    #[serde(default)]
    values: BTreeMap<String, Value>,
}

/// One device's state in its own JSON file (see the module docs). Read
/// once when opened; every write goes to disk before it returns.
#[derive(Debug)]
pub struct FileState {
    path: PathBuf,
    /// What is on disk. Never held across an `.await`.
    values: Mutex<BTreeMap<String, Value>>,
    /// One write at a time, so they reach the disk in order.
    writing: tokio::sync::Mutex<()>,
}

impl FileState {
    /// The state of `device` in `dir`. Never fails: a file that cannot be
    /// read is logged and the device starts without state.
    pub fn open(dir: &Path, device: &DeviceId) -> Self {
        let path = dir.join(format!("{device}.json"));
        let values = load(&path, device);
        Self {
            path,
            values: Mutex::new(values),
            writing: tokio::sync::Mutex::new(()),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn current(&self) -> BTreeMap<String, Value> {
        self.values
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

fn load(path: &Path, device: &DeviceId) -> BTreeMap<String, Value> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return BTreeMap::new(),
        Err(error) => {
            tracing::warn!(%device, path = %path.display(), %error, "could not read the device's kept state; starting without it");
            return BTreeMap::new();
        }
    };
    let file: StateFile = match serde_json::from_slice(&bytes) {
        Ok(file) => file,
        Err(error) => {
            tracing::warn!(%device, path = %path.display(), %error, "the device's kept state is unreadable; starting without it");
            return BTreeMap::new();
        }
    };
    if file.schema_version != STATE_SCHEMA_VERSION {
        tracing::warn!(
            %device,
            path = %path.display(),
            version = file.schema_version,
            "the device's kept state has an unknown format; starting without it"
        );
        return BTreeMap::new();
    }
    file.values
        .into_iter()
        .filter(|(key, value)| {
            let usable = StateKey::new(key.as_str()).is_ok() && check_no_secrets(value).is_ok();
            if !usable {
                tracing::warn!(%device, %key, "dropped a kept value that is not allowed in state");
            }
            usable
        })
        .collect()
}

#[async_trait]
impl DriverState for FileState {
    fn get(&self, key: &StateKey) -> Option<Value> {
        self.values
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(key.as_str())
            .cloned()
    }

    async fn set(&self, key: &StateKey, value: Value) -> Result<(), DriverError> {
        check_no_secrets(&value).map_err(DriverError::other)?;
        let _writing = self.writing.lock().await;
        let mut next = self.current();
        if value.is_null() {
            next.remove(key.as_str());
        } else {
            next.insert(key.as_str().to_owned(), value);
        }
        let file = StateFile {
            schema_version: STATE_SCHEMA_VERSION,
            values: next,
        };
        let mut json = serde_json::to_vec_pretty(&file).map_err(DriverError::other)?;
        json.push(b'\n');
        let path = self.path.clone();
        tokio::task::spawn_blocking(move || {
            if let Some(dir) = path.parent().filter(|dir| !dir.as_os_str().is_empty()) {
                fs::create_dir_all(dir)?;
            }
            write_atomically(&path, &json)
        })
        .await
        .map_err(DriverError::other)??;
        // Only once it is on disk.
        *self.values.lock().unwrap_or_else(PoisonError::into_inner) = file.values;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use serde_json::json;

    use super::*;

    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let dir = std::env::temp_dir().join(format!(
                "vesta-hub-state-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            let _ = fs::remove_dir_all(&dir);
            Self(dir)
        }

        fn entries(&self) -> Vec<String> {
            let mut names: Vec<String> = fs::read_dir(&self.0)
                .unwrap()
                .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
                .collect();
            names.sort();
            names
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn device(id: &str) -> DeviceId {
        DeviceId::new(id).unwrap()
    }

    fn key() -> StateKey {
        StateKey::new("charging_schedule").unwrap()
    }

    fn hours() -> Value {
        json!({ "windows": [{ "start": "22:00", "end": "06:00", "days": ["monday"] }],
                "source": "charger", "at": "2026-11-28T09:30:00Z" })
    }

    #[tokio::test]
    async fn keeps_values_across_restarts_one_file_per_device() {
        let dir = TempDir::new();
        let storage = StateStorage::Dir(dir.0.clone());
        let charger = storage.open(&device("car_charger"));
        assert_eq!(charger.get(&key()), None);
        // The directory appears with the first write.
        charger.set(&key(), hours()).await.unwrap();
        storage
            .open(&device("garage_door"))
            .set(&key(), json!(1))
            .await
            .unwrap();
        assert_eq!(dir.entries(), ["car_charger.json", "garage_door.json"]);

        // A restart: every device gets back its own values.
        assert_eq!(
            storage.open(&device("car_charger")).get(&key()),
            Some(hours())
        );
        assert_eq!(
            storage.open(&device("garage_door")).get(&key()),
            Some(json!(1))
        );
        assert_eq!(storage.open(&device("front_door")).get(&key()), None);

        let written: Value =
            serde_json::from_slice(&fs::read(dir.0.join("car_charger.json")).unwrap()).unwrap();
        assert_eq!(
            written,
            json!({ "schema_version": 1, "values": { "charging_schedule": hours() } })
        );

        // `null` removes a value.
        charger.set(&key(), Value::Null).await.unwrap();
        assert_eq!(storage.open(&device("car_charger")).get(&key()), None);
    }

    #[tokio::test]
    async fn writes_atomically_in_order() {
        let dir = TempDir::new();
        let state = Arc::new(FileState::open(&dir.0, &device("car_charger")));
        let mut writes = Vec::new();
        for n in 0..20 {
            let state = Arc::clone(&state);
            writes.push(tokio::spawn(async move {
                state.set(&key(), json!(n)).await.unwrap();
            }));
        }
        for write in writes {
            write.await.unwrap();
        }
        let last = state.get(&key()).unwrap();
        let reopened = FileState::open(&dir.0, &device("car_charger"));
        assert_eq!(reopened.get(&key()), Some(last));
        assert_eq!(dir.entries(), ["car_charger.json"], "no temporary files");
        assert_eq!(state.path(), dir.0.join("car_charger.json"));
    }

    #[tokio::test]
    async fn unreadable_state_is_ignored_and_replaced() {
        let dir = TempDir::new();
        fs::create_dir_all(&dir.0).unwrap();
        let path = dir.0.join("car_charger.json");
        for broken in [
            "not json".to_owned(),
            json!({ "values": {} }).to_string(),
            json!({ "schema_version": 99, "values": { "charging_schedule": 1 } }).to_string(),
            json!({ "schema_version": 1, "values": "nope" }).to_string(),
        ] {
            fs::write(&path, &broken).unwrap();
            let state = FileState::open(&dir.0, &device("car_charger"));
            assert_eq!(state.get(&key()), None, "{broken}");
            // The next write replaces it.
            state.set(&key(), hours()).await.unwrap();
            assert_eq!(
                FileState::open(&dir.0, &device("car_charger")).get(&key()),
                Some(hours())
            );
        }

        // Values that are not allowed are dropped; the rest stays.
        fs::write(
            &path,
            json!({ "schema_version": 1, "values": {
                "charging_schedule": hours(),
                "Bad Key": 1,
                "cloud": { "cloud_api_key": "do-not-keep" }
            } })
            .to_string(),
        )
        .unwrap();
        let state = FileState::open(&dir.0, &device("car_charger"));
        assert_eq!(state.get(&key()), Some(hours()));
        assert_eq!(state.get(&StateKey::new("cloud").unwrap()), None);
    }

    #[tokio::test]
    async fn a_state_dir_that_cannot_be_used_never_stops_the_device() {
        let dir = TempDir::new();
        fs::create_dir_all(&dir.0).unwrap();
        // A directory where the device's file should be.
        fs::create_dir_all(dir.0.join("car_charger.json").join("x")).unwrap();
        let state = FileState::open(&dir.0, &device("car_charger"));
        assert_eq!(state.get(&key()), None);
        assert!(state.set(&key(), hours()).await.is_err());
        assert_eq!(state.get(&key()), None, "nothing changed");

        // A file where the directory should be.
        let blocked = dir.0.join("blocked");
        fs::write(&blocked, "x").unwrap();
        let state = StateStorage::Dir(blocked).open(&device("car_charger"));
        assert!(state.set(&key(), hours()).await.is_err());
    }

    #[tokio::test]
    async fn secrets_never_reach_the_disk() {
        let dir = TempDir::new();
        let state = FileState::open(&dir.0, &device("car_charger"));
        state.set(&key(), hours()).await.unwrap();
        for secret in [
            json!({ "cloud_api_key": "secret-key-123" }),
            json!({ "config": { "local_key": "0123456789abcdef" } }),
            json!([{ "password": "hunter2" }]),
        ] {
            assert!(state.set(&key(), secret).await.is_err());
        }
        let written = fs::read_to_string(dir.0.join("car_charger.json")).unwrap();
        for leaked in ["secret-key-123", "0123456789abcdef", "hunter2"] {
            assert!(!written.contains(leaked), "{written}");
        }
        assert_eq!(state.get(&key()), Some(hours()));
    }

    #[tokio::test]
    async fn memory_storage_forgets_at_restart() {
        let storage = StateStorage::default();
        assert_eq!(storage, StateStorage::Memory);
        let state = storage.open(&device("car_charger"));
        state.set(&key(), hours()).await.unwrap();
        assert_eq!(state.get(&key()), Some(hours()));
        assert_eq!(storage.open(&device("car_charger")).get(&key()), None);
    }
}
