//! Driver for V2C Trydan EV chargers, over their local HTTP API.
//!
//! One driver instance is one charger. It reads `GET /RealTimeData` every
//! few seconds and reports a brand-free device:
//!
//! | Endpoint | Type | Capability |
//! |---|---|---|
//! | `charger` | `ev_charger` | `ev_charger`: status, power, session, current, mode, lock, schedule |
//! | `grid` | `meter` | `power_meter` (`grid`): import/export, contracted power as `limit_w` |
//! | `house` | `meter` | `power_meter` (`house`): the whole home's consumption |
//! | `solar` | `meter` | `power_meter` (`solar`), only with `meters.solar` |
//! | `battery` | `meter` | `power_meter` (`battery`), only with `meters.battery` |
//!
//! Commands map to the few writes in [`api::Write`]; the charger's safety
//! and installation settings (contracted power, dynamic power control,
//! current limits, phases) are never written (AGENTS.md rule 7). See
//! `README.md` for the API facts and their sources.
//!
//! Config (`devices[].config` in `config.json`):
//!
//! ```json
//! { "host": "192.0.2.10", "read_only": false, "meters": { "solar": true } }
//! ```

mod api;
mod config;
mod discovery;
mod http;
#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError, RwLock};
use std::time::Duration;

use tokio::sync::{mpsc, oneshot};
use tokio::time::{Instant, sleep, sleep_until, timeout};
use vesta_core::{
    Capabilities, CapabilityState, Command, DeviceDescriptor, DeviceDriver, DriverError, Endpoint,
    EndpointId, EndpointType, EvChargerCapability, EvChargerCommand, EvChargerFeatures, EventSink,
    MeterRole, PowerMeterCapability, PowerMeterFeatures, async_trait,
};

use crate::api::{HW_MAX_A, HW_MIN_A, Reading, SWITCHABLE_MODES, SolarMode, Write, intensity_for};
use crate::config::Address;
pub use crate::config::{
    DEFAULT_POLL_INTERVAL_MS, DEFAULT_REQUEST_TIMEOUT_MS, MAX_POLL_INTERVAL_MS,
    MAX_REQUEST_TIMEOUT_MS, MIN_POLL_INTERVAL_MS, MIN_REQUEST_TIMEOUT_MS, Meters, V2cConfig,
};
use crate::http::Http;

const CHARGER: &str = "charger";
const GRID: &str = "grid";
const HOUSE: &str = "house";
const SOLAR: &str = "solar";
const BATTERY: &str = "battery";

/// How the driver paces itself. Tests shorten everything.
#[derive(Debug, Clone, Copy)]
struct Timing {
    poll: Duration,
    request_timeout: Duration,
    /// Longest wait between attempts while the charger does not answer.
    max_backoff: Duration,
    /// Failed readings in a row before the charger shows as not responding
    /// (it sits on a Wi-Fi extender that drops now and then).
    offline_after: u32,
    /// Look for the charger elsewhere after it failed for this long...
    rediscover_after: Duration,
    /// ...and at most this often.
    rescan_every: Duration,
    probe_timeout: Duration,
    /// Space between two writes (some firmware mixes up writes sent at
    /// once).
    write_spacing: Duration,
    /// A command, including the fresh reading after it, ends within this
    /// (the Hub gives up after 5 s).
    command_budget: Duration,
}

impl Timing {
    fn new(config: &V2cConfig) -> Self {
        Self {
            poll: config.poll_interval(),
            request_timeout: config.request_timeout(),
            max_backoff: Duration::from_secs(30),
            offline_after: 3,
            rediscover_after: Duration::from_secs(60),
            rescan_every: Duration::from_secs(600),
            probe_timeout: Duration::from_millis(1_500),
            write_spacing: Duration::from_secs(1),
            command_budget: Duration::from_millis(4_500),
        }
    }
}

/// Where to look when the charger moved.
#[derive(Debug, Clone)]
enum Scan {
    /// The `/24` networks of its last address and of this machine.
    Network,
    /// Exactly these addresses (tests).
    #[cfg_attr(not(test), allow(dead_code))]
    Fixed(Vec<Address>),
}

/// A fresh reading, asked for by a command; answered when it was published.
type Refresh = oneshot::Sender<()>;

/// One V2C Trydan charger.
pub struct V2cDriver {
    config: V2cConfig,
    timing: Timing,
    scan: Scan,
    http: Http,
    /// Where the charger is now (changes when it is found elsewhere).
    address: RwLock<Address>,
    /// The latest reading, to clamp commands to the charger's own limits.
    last: Mutex<Option<Reading>>,
    /// The charger's ID: only this charger is ever read or controlled.
    known_id: Mutex<Option<String>>,
    /// The last answer came from our charger. Commands are only sent while
    /// it did, so a different charger that took over the address is never
    /// controlled.
    ours: AtomicBool,
    /// Serializes writes; holds when the last one was sent.
    writes: tokio::sync::Mutex<Option<Instant>>,
    refresh_tx: mpsc::Sender<Refresh>,
    refresh_rx: tokio::sync::Mutex<mpsc::Receiver<Refresh>>,
}

impl std::fmt::Debug for V2cDriver {
    /// Without the address: it is driver config, which never reaches logs.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("V2cDriver")
            .field("read_only", &self.config.read_only)
            .field("meters", &self.config.meters)
            .finish_non_exhaustive()
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

fn endpoint_id(raw: &str) -> Result<EndpointId, DriverError> {
    EndpointId::new(raw).map_err(DriverError::other)
}

fn meter(
    id: &str,
    name: &str,
    role: MeterRole,
    bidirectional: bool,
) -> Result<Endpoint, DriverError> {
    Ok(Endpoint {
        id: endpoint_id(id)?,
        name: name.to_owned(),
        endpoint_type: EndpointType::Meter,
        room: None,
        capabilities: Capabilities {
            power_meter: Some(PowerMeterCapability {
                features: PowerMeterFeatures {
                    role,
                    bidirectional,
                },
                state: None,
            }),
            ..Capabilities::default()
        },
    })
}

impl V2cDriver {
    /// Builds a driver from the device's driver config JSON.
    pub fn from_config(value: &serde_json::Value) -> Result<Self, DriverError> {
        Self::new(V2cConfig::from_value(value)?)
    }

    pub fn new(config: V2cConfig) -> Result<Self, DriverError> {
        config.validate()?;
        let timing = Timing::new(&config);
        Self::build(config, timing, Scan::Network)
    }

    fn build(config: V2cConfig, timing: Timing, scan: Scan) -> Result<Self, DriverError> {
        let address = config.address()?;
        let http = Http::new(timing.request_timeout)?;
        let (refresh_tx, refresh_rx) = mpsc::channel(8);
        Ok(Self {
            known_id: Mutex::new(config.charger_id.clone()),
            ours: AtomicBool::new(false),
            config,
            timing,
            scan,
            http,
            address: RwLock::new(address),
            last: Mutex::new(None),
            writes: tokio::sync::Mutex::new(None),
            refresh_tx,
            refresh_rx: tokio::sync::Mutex::new(refresh_rx),
        })
    }

    pub fn config(&self) -> &V2cConfig {
        &self.config
    }

    fn address(&self) -> Address {
        self.address
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn last_reading(&self) -> Option<Reading> {
        lock(&self.last).clone()
    }

    fn known_id(&self) -> Option<String> {
        lock(&self.known_id).clone()
    }

    fn endpoints(&self) -> Result<Vec<Endpoint>, DriverError> {
        let control = !self.config.read_only;
        let meters = self.config.meters;
        let mut endpoints = vec![Endpoint {
            id: endpoint_id(CHARGER)?,
            name: "Car charger".to_owned(),
            endpoint_type: EndpointType::EvCharger,
            room: None,
            capabilities: Capabilities {
                ev_charger: Some(EvChargerCapability {
                    features: EvChargerFeatures {
                        min_current_a: HW_MIN_A,
                        max_current_a: HW_MAX_A,
                        // The solar modes only work (and only make sense)
                        // with solar panels.
                        modes: if control && meters.solar {
                            SWITCHABLE_MODES.to_vec()
                        } else {
                            Vec::new()
                        },
                        pause: control,
                        adjustable_current: control,
                        lock: control,
                        schedule: control,
                    },
                    state: None,
                }),
                ..Capabilities::default()
            },
        }];
        if meters.grid {
            endpoints.push(meter(GRID, "Grid", MeterRole::Grid, true)?);
        }
        if meters.house {
            endpoints.push(meter(HOUSE, "House", MeterRole::House, false)?);
        }
        if meters.solar {
            endpoints.push(meter(SOLAR, "Solar panels", MeterRole::Solar, false)?);
        }
        if meters.battery {
            endpoints.push(meter(BATTERY, "Home battery", MeterRole::Battery, true)?);
        }
        Ok(endpoints)
    }

    /// The one write a command needs, within the charger's own limits, or
    /// why it cannot be sent.
    fn plan(&self, cmd: EvChargerCommand) -> Result<Write, DriverError> {
        Ok(match cmd {
            EvChargerCommand::Pause => Write::Paused(true),
            EvChargerCommand::Resume => Write::Paused(false),
            EvChargerCommand::Lock => Write::Locked(true),
            EvChargerCommand::Unlock => Write::Locked(false),
            EvChargerCommand::SetSchedule { enabled } => Write::Timer(enabled),
            EvChargerCommand::SetCurrent { amps } => {
                let reading = self.last_reading().ok_or(DriverError::Offline)?;
                if reading.auto_current() == Some(true) {
                    tracing::info!(
                        "current not changed: the charger's own power control sets it \
                         (it ignores Intensity while Dynamic=1 and PauseDynamic=0)"
                    );
                    return Err(DriverError::Unsupported);
                }
                Write::Intensity(intensity_for(amps, reading.current_range()))
            }
            EvChargerCommand::SetMode { mode } => {
                if !self.config.meters.solar {
                    return Err(DriverError::Unsupported);
                }
                let solar = SolarMode::of(mode).ok_or(DriverError::Unsupported)?;
                let reading = self.last_reading().ok_or(DriverError::Offline)?;
                if !SolarMode::is_active(reading.dynamic_power_mode) {
                    tracing::info!(
                        "mode not changed: the charger follows the power profiles set in its \
                         own app, which VESTA never turns off"
                    );
                    return Err(DriverError::Unsupported);
                }
                Write::DynamicPowerMode(solar)
            }
        })
    }

    async fn send(&self, write: Write) -> Result<(), DriverError> {
        let mut last_write = self.writes.lock().await;
        if let Some(at) = *last_write {
            let wait = self.timing.write_spacing.saturating_sub(at.elapsed());
            if !wait.is_zero() {
                sleep(wait).await;
            }
        }
        let result = self.http.write(&self.address(), write).await;
        *last_write = Some(Instant::now());
        match &result {
            Ok(()) => tracing::info!(write = %write.path(), "sent to the charger"),
            Err(error) => tracing::warn!(write = %write.path(), %error, "write failed"),
        }
        result
    }

    /// Asks the device loop for a fresh reading and waits (within the
    /// command's time) until it was published, so the screen shows the
    /// result before the command is confirmed.
    async fn refresh(&self, started: Instant) {
        let (done, published) = oneshot::channel();
        if self.refresh_tx.try_send(done).is_err() {
            return;
        }
        let remaining = self.timing.command_budget.saturating_sub(started.elapsed());
        let _ = timeout(remaining, published).await;
    }

    /// One reading from the charger we know (a different charger at the
    /// same address is not ours).
    async fn read(&self) -> Result<Reading, DriverError> {
        let reading = self.http.read(&self.address()).await?;
        if let Some(id) = reading.id.as_deref() {
            let mut known = lock(&self.known_id);
            match known.as_deref() {
                None => *known = Some(id.to_owned()),
                Some(ours) if ours == id => {}
                Some(_) => {
                    self.ours.store(false, Ordering::SeqCst);
                    return Err(DriverError::Protocol(
                        "a different charger answers at this address".to_owned(),
                    ));
                }
            }
        }
        self.ours.store(true, Ordering::SeqCst);
        Ok(reading)
    }

    /// Publishes what changed since the last reading.
    async fn publish(
        &self,
        sink: &EventSink,
        reading: &Reading,
        published: &mut HashMap<EndpointId, CapabilityState>,
        meter_error: &mut bool,
    ) -> Result<(), DriverError> {
        let mut states: Vec<(&str, CapabilityState)> = Vec::new();
        if let Some(state) = reading.charger_state() {
            states.push((CHARGER, state.into()));
        }
        if reading.meters_ok() {
            *meter_error = false;
            let meters = self.config.meters;
            let readings = [
                (meters.grid, GRID, reading.grid_state()),
                (meters.house, HOUSE, reading.house_state()),
                (meters.solar, SOLAR, reading.solar_state()),
                (meters.battery, BATTERY, reading.battery_state()),
            ];
            for (shown, id, state) in readings {
                if let (true, Some(state)) = (shown, state) {
                    states.push((id, state.into()));
                }
            }
        } else if !*meter_error {
            // The clamp/meter does not answer the charger: its numbers are
            // not real. The charger's own power control deals with it.
            *meter_error = true;
            tracing::warn!(
                code = reading.meter_error.unwrap_or_default(),
                "the charger reports a power meter error; meter readings are not shown"
            );
        }
        for (id, state) in states {
            let endpoint = endpoint_id(id)?;
            if published.get(&endpoint) != Some(&state) {
                sink.state(&endpoint, state.clone()).await?;
                published.insert(endpoint, state);
            }
        }
        Ok(())
    }

    fn should_rediscover(&self, failing_since: Instant, last_scan: Option<Instant>) -> bool {
        self.config.rediscover
            && self.known_id().is_some()
            && failing_since.elapsed() >= self.timing.rediscover_after
            && last_scan.is_none_or(|at| at.elapsed() >= self.timing.rescan_every)
    }

    /// Looks for our charger (same ID) elsewhere on the local network and
    /// follows it there. Returns whether it was found.
    async fn rediscover(&self) -> bool {
        let Some(id) = self.known_id() else {
            return false;
        };
        let current = self.address();
        let candidates = match &self.scan {
            Scan::Network => discovery::candidates(&current, discovery::local_ipv4(current.ipv4())),
            Scan::Fixed(addresses) => addresses.clone(),
        };
        if candidates.is_empty() {
            return false;
        }
        tracing::info!(
            addresses = candidates.len(),
            "the charger does not answer; looking for it on the local network"
        );
        match discovery::find(candidates, &id, self.timing.probe_timeout).await {
            Some(found) => {
                tracing::info!(from = %current, to = %found, "found the charger at a new address");
                *self.address.write().unwrap_or_else(PoisonError::into_inner) = found;
                true
            }
            None => {
                tracing::info!("the charger was not found on the local network");
                false
            }
        }
    }

    fn retry_delay(&self, failures: u32, online: bool) -> Duration {
        if online {
            // Still counted as responding: try again at the normal pace.
            return self.timing.poll;
        }
        let doublings = failures.saturating_sub(1).min(5);
        self.timing
            .poll
            .saturating_mul(1 << doublings)
            .min(self.timing.max_backoff)
    }
}

#[async_trait]
impl DeviceDriver for V2cDriver {
    fn describe(&self) -> DeviceDescriptor {
        match self.endpoints() {
            Ok(endpoints) => DeviceDescriptor { endpoints },
            Err(error) => {
                tracing::error!(%error, "cannot describe the charger");
                DeviceDescriptor::default()
            }
        }
    }

    async fn execute(&self, endpoint: &EndpointId, cmd: Command) -> Result<(), DriverError> {
        if endpoint.as_str() != CHARGER {
            let known = [GRID, HOUSE, SOLAR, BATTERY].contains(&endpoint.as_str());
            return Err(if known {
                DriverError::Unsupported
            } else {
                DriverError::UnknownEndpoint(endpoint.clone())
            });
        }
        if self.config.read_only {
            tracing::info!("command refused: the charger is set up as read-only");
            return Err(DriverError::ReadOnly);
        }
        let Command::EvCharger(cmd) = cmd else {
            return Err(DriverError::Unsupported);
        };
        if !self.ours.load(Ordering::SeqCst) {
            // Not read yet, or a different charger answers at the address.
            return Err(DriverError::Offline);
        }
        let started = Instant::now();
        let write = self.plan(cmd)?;
        self.send(write).await?;
        self.refresh(started).await;
        Ok(())
    }

    async fn run(&self, sink: EventSink) -> Result<(), DriverError> {
        // One loop at a time; a restarted loop picks up where the last one
        // stopped.
        let mut refresh = self.refresh_rx.lock().await;
        let mut published = HashMap::new();
        let mut meter_error = false;
        let mut online = false;
        let mut failures: u32 = 0;
        let mut failing_since: Option<Instant> = None;
        let mut last_scan: Option<Instant> = None;
        let mut next_read = Instant::now();
        loop {
            let mut waiting = Vec::new();
            tokio::select! {
                () = sleep_until(next_read) => {}
                request = refresh.recv() => match request {
                    Some(done) => waiting.push(done),
                    // The driver holds the sender: never happens.
                    None => return Ok(()),
                },
            }
            // Commands that came in meanwhile share this reading.
            while let Ok(done) = refresh.try_recv() {
                waiting.push(done);
            }

            match self.read().await {
                Ok(reading) => {
                    if !online {
                        online = true;
                        tracing::info!(
                            firmware = reading.firmware.as_deref().unwrap_or("unknown"),
                            "the charger answers"
                        );
                        sink.availability(true).await?;
                    }
                    failures = 0;
                    failing_since = None;
                    self.publish(&sink, &reading, &mut published, &mut meter_error)
                        .await?;
                    *lock(&self.last) = Some(reading);
                    next_read = Instant::now() + self.timing.poll;
                }
                Err(error) => {
                    failures = failures.saturating_add(1);
                    let since = *failing_since.get_or_insert_with(Instant::now);
                    if online && failures >= self.timing.offline_after {
                        online = false;
                        tracing::warn!(%error, "the charger stopped answering");
                        sink.availability(false).await?;
                    } else {
                        tracing::debug!(%error, failures, "reading the charger failed");
                    }
                    next_read = Instant::now() + self.retry_delay(failures, online);
                    if !online && self.should_rediscover(since, last_scan) {
                        last_scan = Some(Instant::now());
                        if self.rediscover().await {
                            next_read = Instant::now();
                        }
                    }
                }
            }
            for done in waiting {
                let _ = done.send(());
            }
        }
    }
}
