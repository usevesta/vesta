//! Device runtime: builds drivers, keeps the current state of every device
//! and routes commands.
//!
//! ```text
//! driver.run(sink) ──mpsc──▶ record_updates ──▶ snapshot cache (source of truth)
//!                                          └──▶ EventBus ──▶ WebSocket sessions
//! ```
//!
//! Updates are recorded in the cache *before* they are published, and
//! sessions subscribe *before* taking their snapshot, so a client never
//! misses an update (it may see one twice, which is harmless: `state`
//! always carries the full capability state).

use std::collections::HashMap;
use std::sync::{Arc, PoisonError, RwLock, RwLockReadGuard, RwLockWriteGuard};
use std::time::Duration;

use tokio::sync::mpsc;
use tokio::task::JoinSet;
use tokio::time::{Instant, sleep, timeout};
use vesta_core::{
    AvailabilityChange, BusEvent, CommandError, CommandRequest, DeviceConfig, DeviceDescriptor,
    DeviceDriver, DeviceId, DriverError, Endpoint, EventBus, EventSink, HubConfig, RejectReason,
    Snapshot, Subscription, Target, Timestamp,
};
use vesta_drivers::Registry;

use crate::HubOptions;

/// Updates buffered between drivers and the cache. Bounded so a runaway
/// driver applies backpressure instead of growing memory; never lossy.
const UPDATE_BUFFER: usize = 1024;
const RESTART_MIN: Duration = Duration::from_secs(1);
const RESTART_MAX: Duration = Duration::from_secs(60);
/// A device loop that ran at least this long resets the restart backoff.
const HEALTHY_RUN: Duration = Duration::from_secs(60);

pub(crate) struct Runtime {
    /// In config order. Never held across an `.await`.
    devices: RwLock<Vec<vesta_core::Device>>,
    /// Immutable after start.
    drivers: HashMap<DeviceId, Arc<dyn DeviceDriver>>,
    bus: EventBus,
    command_timeout: Duration,
    slow_command_timeout: Duration,
}

impl Runtime {
    /// Builds a driver for every configured device and spawns the device
    /// loops into `tasks`. A device whose driver cannot be built is skipped
    /// (and logged) so one bad entry never takes the others down.
    pub(crate) fn start(
        config: &HubConfig,
        registry: &Registry,
        options: &HubOptions,
        tasks: &mut JoinSet<()>,
    ) -> Arc<Self> {
        let mut devices = Vec::new();
        let mut drivers = HashMap::new();
        for device_config in &config.devices {
            let driver = match registry.build(device_config) {
                Ok(driver) => driver,
                Err(error) => {
                    tracing::error!(%error, "device skipped");
                    continue;
                }
            };
            devices.push(assemble(device_config, driver.describe()));
            drivers.insert(device_config.id.clone(), driver);
        }
        tracing::info!(devices = devices.len(), "device runtime starting");

        let runtime = Arc::new(Self {
            devices: RwLock::new(devices),
            drivers,
            bus: EventBus::new(options.bus_capacity),
            command_timeout: options.command_timeout,
            slow_command_timeout: options.slow_command_timeout,
        });

        let (tx, rx) = mpsc::channel(UPDATE_BUFFER);
        for (id, driver) in &runtime.drivers {
            tasks.spawn(supervise(id.clone(), Arc::clone(driver), tx.clone()));
        }
        drop(tx);
        tasks.spawn(record_updates(Arc::clone(&runtime), rx));
        runtime
    }

    fn read(&self) -> RwLockReadGuard<'_, Vec<vesta_core::Device>> {
        self.devices.read().unwrap_or_else(PoisonError::into_inner)
    }

    fn write(&self) -> RwLockWriteGuard<'_, Vec<vesta_core::Device>> {
        self.devices.write().unwrap_or_else(PoisonError::into_inner)
    }

    /// Every device with its current state.
    pub(crate) fn snapshot(&self) -> Snapshot {
        Snapshot {
            devices: self.read().clone(),
        }
    }

    pub(crate) fn subscribe(&self) -> Subscription {
        self.bus.subscribe()
    }

    pub(crate) fn has_target(&self, target: &Target) -> bool {
        self.read()
            .iter()
            .find(|device| &device.id == target.device())
            .is_some_and(|device| device.endpoint(target.endpoint()).is_some())
    }

    /// Runs one command. Checks, in order: the target exists and supports
    /// the command (`unsupported`), sensitive commands carry
    /// `confirmed: true` (`rejected`), the device is online (`offline`);
    /// then the driver gets `command_timeout` (`slow_command_timeout` for a
    /// slow command) to accept it (`timeout`).
    pub(crate) async fn execute(&self, request: &CommandRequest) -> Result<(), CommandError> {
        let target = &request.target;
        let (endpoint_type, online) = {
            let devices = self.read();
            let device = devices
                .iter()
                .find(|device| &device.id == target.device())
                .ok_or(CommandError::Unsupported)?;
            let endpoint = device
                .endpoint(target.endpoint())
                .ok_or(CommandError::Unsupported)?;
            if !endpoint.capabilities.supports(&request.command) {
                return Err(CommandError::Unsupported);
            }
            (endpoint.endpoint_type, device.online)
        };
        if request.command.is_sensitive(endpoint_type) && !request.confirmed {
            return Err(CommandError::Rejected {
                reason: RejectReason::ConfirmationRequired,
            });
        }
        if !online {
            return Err(CommandError::Offline);
        }
        let driver = self
            .drivers
            .get(target.device())
            .ok_or(CommandError::Unsupported)?;
        let capability = request.command.capability();
        let limit = if request.command.is_slow() {
            self.slow_command_timeout
        } else {
            self.command_timeout
        };
        match timeout(
            limit,
            driver.execute(target.endpoint(), request.command.clone()),
        )
        .await
        {
            Ok(Ok(())) => {
                tracing::debug!(endpoint = %target, %capability, "command accepted");
                Ok(())
            }
            Ok(Err(error)) => {
                // Details (hosts, protocol errors) stay in the Hub's log;
                // the client only gets the stable code.
                tracing::warn!(endpoint = %target, %capability, %error, "command failed");
                Err(CommandError::from(&error))
            }
            Err(_) => {
                tracing::warn!(endpoint = %target, %capability, "command timed out");
                Err(CommandError::Timeout)
            }
        }
    }

    /// Stores an update in the cache. Returns whether it should be
    /// published to clients.
    fn record(&self, update: &BusEvent) -> bool {
        let mut devices = self.write();
        let device_id = match update {
            BusEvent::State(change) => change.target.device(),
            BusEvent::Event(event) => event.target.device(),
            BusEvent::Availability(change) => &change.device_id,
        };
        let Some(device) = devices.iter_mut().find(|device| &device.id == device_id) else {
            tracing::warn!(device = %device_id, "dropped update for an unknown device");
            return false;
        };
        let now = Timestamp::now();
        match update {
            BusEvent::State(change) => {
                let applied =
                    device
                        .endpoint_mut(change.target.endpoint())
                        .is_some_and(|endpoint| {
                            endpoint.capabilities.apply_state(change.state.clone())
                        });
                if !applied {
                    tracing::warn!(
                        endpoint = %change.target,
                        capability = %change.state.kind(),
                        "dropped state for a capability the endpoint does not have"
                    );
                    return false;
                }
                device.last_seen = Some(now);
                true
            }
            BusEvent::Event(event) => {
                let capability = event.event.capability();
                let known = device
                    .endpoint(event.target.endpoint())
                    .is_some_and(|endpoint| endpoint.capabilities.has(capability));
                if !known {
                    tracing::warn!(
                        endpoint = %event.target,
                        event = %event.event,
                        "dropped event for a capability the endpoint does not have"
                    );
                    return false;
                }
                device.last_seen = Some(now);
                true
            }
            BusEvent::Availability(change) => {
                if change.online {
                    device.last_seen = Some(now);
                }
                let changed = device.online != change.online;
                device.online = change.online;
                if changed {
                    tracing::info!(device = %device.id, online = change.online, "availability changed");
                }
                changed
            }
        }
    }
}

/// Combines the driver's description with the device's config: id, name,
/// room and presentation overrides. Devices start offline until their
/// driver reports otherwise.
fn assemble(config: &DeviceConfig, descriptor: DeviceDescriptor) -> vesta_core::Device {
    let mut endpoints: Vec<Endpoint> = Vec::with_capacity(descriptor.endpoints.len());
    for endpoint in descriptor.endpoints {
        if endpoints.iter().any(|known| known.id == endpoint.id) {
            tracing::warn!(device = %config.id, endpoint = %endpoint.id, "duplicate endpoint ignored");
            continue;
        }
        endpoints.push(endpoint);
    }
    // A single-function device *is* its endpoint: show the name the user
    // gave the device ("Living room light"), not the driver's default.
    if let [only] = endpoints.as_mut_slice() {
        only.name.clone_from(&config.name);
    }
    for (id, presentation) in &config.endpoints {
        match endpoints.iter_mut().find(|endpoint| &endpoint.id == id) {
            Some(endpoint) => presentation.apply_to(endpoint),
            None => tracing::warn!(
                device = %config.id,
                endpoint = %id,
                "override for an endpoint the device does not have"
            ),
        }
    }
    vesta_core::Device {
        id: config.id.clone(),
        name: config.name.clone(),
        room: config.room.clone(),
        online: false,
        last_seen: None,
        endpoints,
    }
}

/// Keeps one device loop running, restarting it with exponential backoff
/// and reporting the device offline while it is down.
async fn supervise(id: DeviceId, driver: Arc<dyn DeviceDriver>, tx: mpsc::Sender<BusEvent>) {
    let mut backoff = RESTART_MIN;
    loop {
        let started = Instant::now();
        let result = driver.run(EventSink::new(id.clone(), tx.clone())).await;
        if tx.is_closed() {
            return;
        }
        match result {
            Err(DriverError::SinkClosed) => return,
            Ok(()) => tracing::warn!(device = %id, "device loop ended; restarting"),
            Err(error) => tracing::warn!(device = %id, %error, "device loop failed; restarting"),
        }
        let offline = BusEvent::Availability(AvailabilityChange {
            device_id: id.clone(),
            online: false,
        });
        if tx.send(offline).await.is_err() {
            return;
        }
        if started.elapsed() >= HEALTHY_RUN {
            backoff = RESTART_MIN;
        }
        sleep(backoff).await;
        backoff = (backoff * 2).min(RESTART_MAX);
    }
}

async fn record_updates(runtime: Arc<Runtime>, mut rx: mpsc::Receiver<BusEvent>) {
    while let Some(update) = rx.recv().await {
        if runtime.record(&update) {
            runtime.bus.publish(update);
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use vesta_core::{
        Capabilities, EndpointId, EndpointType, MeterRole, PowerMeterCapability, PowerMeterFeatures,
    };

    use super::*;

    fn endpoint(id: &str, endpoint_type: EndpointType) -> Endpoint {
        Endpoint {
            id: EndpointId::new(id).unwrap(),
            name: id.to_owned(),
            endpoint_type,
            room: None,
            capabilities: Capabilities::default(),
        }
    }

    fn config(value: serde_json::Value) -> DeviceConfig {
        serde_json::from_value(value).unwrap()
    }

    #[test]
    fn assemble_applies_presentation_overrides() {
        let mut meter = endpoint("meter", EndpointType::Meter);
        meter.capabilities.power_meter = Some(PowerMeterCapability {
            features: PowerMeterFeatures {
                role: MeterRole::Load,
                bidirectional: false,
            },
            state: None,
        });
        let descriptor = DeviceDescriptor {
            endpoints: vec![
                endpoint("relay", EndpointType::Switch),
                meter,
                endpoint("relay", EndpointType::Light),
            ],
        };
        let device = assemble(
            &config(json!({
                "id": "garden_box",
                "name": "Garden box",
                "room": "garden",
                "driver": "anything",
                "config": { "host": "192.0.2.1" },
                "endpoints": {
                    "relay": { "type": "light", "name": "Garden lights", "room": "patio" },
                    "meter": { "role": "solar" },
                    "missing": { "name": "Ignored" }
                }
            })),
            descriptor,
        );
        assert_eq!(device.id, "garden_box");
        assert_eq!(device.room.as_ref().map(|r| r.as_str()), Some("garden"));
        assert!(!device.online);
        assert_eq!(device.endpoints.len(), 2, "duplicate endpoint dropped");

        let relay = &device.endpoints[0];
        assert_eq!(relay.endpoint_type, EndpointType::Light);
        assert_eq!(relay.name, "Garden lights");
        assert_eq!(relay.room.as_ref().map(|r| r.as_str()), Some("patio"));

        let meter = &device.endpoints[1];
        assert_eq!(meter.name, "meter", "multi-endpoint names stay");
        assert_eq!(
            meter
                .capabilities
                .power_meter
                .as_ref()
                .unwrap()
                .features
                .role,
            MeterRole::Solar
        );
    }

    #[test]
    fn single_endpoint_devices_take_the_device_name() {
        let descriptor = DeviceDescriptor {
            endpoints: vec![endpoint("main", EndpointType::Light)],
        };
        let device = assemble(
            &config(json!({ "id": "desk_lamp", "name": "Desk lamp", "driver": "x" })),
            descriptor.clone(),
        );
        assert_eq!(device.endpoints[0].name, "Desk lamp");

        let renamed = assemble(
            &config(json!({
                "id": "desk_lamp", "name": "Desk lamp", "driver": "x",
                "endpoints": { "main": { "name": "Reading light" } }
            })),
            descriptor,
        );
        assert_eq!(renamed.endpoints[0].name, "Reading light");
    }
}
