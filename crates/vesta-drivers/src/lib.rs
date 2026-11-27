//! Composition root for drivers.
//!
//! This is the **only** crate that maps a config `driver` key to a driver
//! crate. Each driver sits behind its own Cargo feature:
//!
//! | `driver` key | Feature | Crate |
//! |---|---|---|
//! | `sim` | `sim` (default) | `vesta-driver-sim` |
//!
//! The Hub asks a [`Registry`] to build one driver per configured device and
//! never learns which crate it came from.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

use vesta_core::{DeviceConfig, DeviceDriver, DeviceId, DriverError};

/// The built-in simulated home (`config.json` + `dashboard.json`), for
/// `vesta-hubd --dev` and for the app before a home is set up.
#[cfg(feature = "sim")]
pub use vesta_driver_sim::demo;

/// Builds a driver from a device's driver-specific `config` object.
pub type DriverFactory =
    dyn Fn(&serde_json::Value) -> Result<Arc<dyn DeviceDriver>, DriverError> + Send + Sync;

/// Why a device's driver could not be built. Hub-side only: these messages
/// name driver keys and must never reach a client.
#[derive(Debug, thiserror::Error)]
pub enum BuildError {
    #[error("device `{device}` uses unknown driver `{driver}`")]
    UnknownDriver { device: DeviceId, driver: String },
    #[error("device `{device}`: {source}")]
    Driver {
        device: DeviceId,
        #[source]
        source: DriverError,
    },
}

/// Driver keys and the factories behind them.
pub struct Registry {
    factories: BTreeMap<String, Box<DriverFactory>>,
}

impl Registry {
    /// A registry without any driver.
    pub fn empty() -> Self {
        Self {
            factories: BTreeMap::new(),
        }
    }

    /// Every driver compiled into this build.
    pub fn builtin() -> Self {
        #[allow(unused_mut)]
        let mut registry = Self::empty();
        #[cfg(feature = "sim")]
        registry.register("sim", |config| {
            Ok(Arc::new(vesta_driver_sim::SimDriver::from_config(config)?)
                as Arc<dyn DeviceDriver>)
        });
        registry
    }

    /// Adds (or replaces) the factory for `driver`.
    pub fn register<F>(&mut self, driver: impl Into<String>, factory: F) -> &mut Self
    where
        F: Fn(&serde_json::Value) -> Result<Arc<dyn DeviceDriver>, DriverError>
            + Send
            + Sync
            + 'static,
    {
        self.factories.insert(driver.into(), Box::new(factory));
        self
    }

    pub fn contains(&self, driver: &str) -> bool {
        self.factories.contains_key(driver)
    }

    /// Registered driver keys, sorted.
    pub fn drivers(&self) -> impl Iterator<Item = &str> {
        self.factories.keys().map(String::as_str)
    }

    /// Builds the driver for one configured device.
    pub fn build(&self, device: &DeviceConfig) -> Result<Arc<dyn DeviceDriver>, BuildError> {
        let factory =
            self.factories
                .get(&device.driver)
                .ok_or_else(|| BuildError::UnknownDriver {
                    device: device.id.clone(),
                    driver: device.driver.clone(),
                })?;
        factory(&device.config).map_err(|source| BuildError::Driver {
            device: device.id.clone(),
            source,
        })
    }
}

impl Default for Registry {
    fn default() -> Self {
        Self::builtin()
    }
}

impl fmt::Debug for Registry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Registry")
            .field("drivers", &self.factories.keys().collect::<Vec<_>>())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use vesta_core::{Command, DeviceDescriptor, EndpointId, EventSink, OnOffCommand, async_trait};

    use super::*;

    fn device(driver: &str, config: serde_json::Value) -> DeviceConfig {
        serde_json::from_value(json!({
            "id": "test_device",
            "name": "Test",
            "driver": driver,
            "config": config,
        }))
        .unwrap()
    }

    #[test]
    fn unknown_driver_is_a_typed_error() {
        let err = Registry::builtin()
            .build(&device("does_not_exist", json!({})))
            .err()
            .unwrap();
        match err {
            BuildError::UnknownDriver { device, driver } => {
                assert_eq!(device, "test_device");
                assert_eq!(driver, "does_not_exist");
            }
            other => panic!("unexpected {other:?}"),
        }
        assert!(
            Registry::empty()
                .build(&device("sim", json!({ "model": "light" })))
                .is_err()
        );
    }

    #[cfg(feature = "sim")]
    #[test]
    fn builds_the_simulator() {
        let registry = Registry::builtin();
        assert!(registry.contains("sim"));
        assert_eq!(registry.drivers().collect::<Vec<_>>(), ["sim"]);

        let driver = registry
            .build(&device("sim", json!({ "model": "ceiling_fan" })))
            .unwrap();
        assert_eq!(driver.describe().endpoints.len(), 2);
    }

    #[cfg(feature = "sim")]
    #[test]
    fn invalid_driver_config_is_a_typed_error() {
        let err = Registry::builtin()
            .build(&device("sim", json!({ "model": "toaster" })))
            .err()
            .unwrap();
        assert!(matches!(
            err,
            BuildError::Driver {
                source: DriverError::InvalidConfig(_),
                ..
            }
        ));
    }

    struct Nothing;

    #[async_trait]
    impl DeviceDriver for Nothing {
        fn describe(&self) -> DeviceDescriptor {
            DeviceDescriptor::default()
        }
        async fn execute(&self, _: &EndpointId, _: Command) -> Result<(), DriverError> {
            Err(DriverError::Unsupported)
        }
        async fn run(&self, _: EventSink) -> Result<(), DriverError> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn custom_factories_can_be_registered() {
        let mut registry = Registry::empty();
        registry.register(
            "nothing",
            |_| Ok(Arc::new(Nothing) as Arc<dyn DeviceDriver>),
        );
        let driver = registry.build(&device("nothing", json!({}))).unwrap();
        assert!(matches!(
            driver
                .execute(&EndpointId::main(), Command::OnOff(OnOffCommand::On))
                .await,
            Err(DriverError::Unsupported)
        ));
    }
}
