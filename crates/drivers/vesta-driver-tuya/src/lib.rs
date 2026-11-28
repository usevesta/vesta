//! Driver for Tuya devices over the local network (protocol 3.3, 3.4 and
//! 3.5), without the Tuya cloud.
//!
//! One driver instance is one device. It keeps one session open on the
//! device's local port, asks for its data points every few seconds, reads
//! the ones the device pushes by itself, and reports a brand-free device
//! (see `model.rs`):
//!
//! | Category | Endpoint | Capabilities |
//! |---|---|---|
//! | `cz`, `tdq`, `kg` | `main` (`outlet` / `switch`), or one per channel | `on_off`, `power_meter` when it measures |
//! | `sd` | `main` (`robot_vacuum`) | `robot_vacuum`, `battery` |
//!
//! The device's address is found from its own UDP broadcasts (by its ID),
//! with the configured `host` as a hint, so DHCP changes do not matter.
//! With `read_only`, nothing but status queries and heartbeats is ever
//! sent. See `README.md` for the protocol notes and their sources.
//!
//! Config (`devices[].config` in `config.json`; written by
//! `vesta-hubd tuya-import`):
//!
//! ```json
//! { "device_id": "bf00000000000000000000", "local_key": "0123456789abcdef",
//!   "protocol_version": "3.5", "category": "cz",
//!   "mapping": { "1": { "code": "switch_1", "type": "Boolean" } } }
//! ```

mod config;
mod crypto;
pub mod discovery;
mod frame;
pub mod import;
mod link;
mod model;
mod protocol;
#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::{Map, Value};
use tokio::sync::{mpsc, oneshot};
use tokio::time::{Instant, sleep_until, timeout};
use vesta_core::{
    CapabilityKind, CapabilityState, Command, DeviceDescriptor, DeviceDriver, DriverError,
    EndpointId, EventSink, async_trait,
};

pub use crate::config::{
    DEFAULT_POLL_INTERVAL_MS, DataPoint, MAX_POLL_INTERVAL_MS, MIN_POLL_INTERVAL_MS, TuyaConfig,
};
pub use crate::discovery::Discovery;
pub use crate::link::DEFAULT_PORT;
use crate::link::{Link, LinkError};
use crate::model::{DpValues, Model};
pub use crate::protocol::Version;
use crate::protocol::{ProtocolError, Request, cmd};

/// How the driver paces itself. Tests shorten everything.
#[derive(Debug, Clone, Copy)]
struct Timing {
    poll: Duration,
    /// A heartbeat goes out when nothing else was sent for this long.
    heartbeat: Duration,
    /// Connecting, negotiating and each write.
    io: Duration,
    /// No message at all for this long: the session is gone.
    silence: Duration,
    min_backoff: Duration,
    max_backoff: Duration,
    /// Failed attempts in a row before the device shows as not responding.
    offline_after: u32,
    /// With no address known, wait this long for the device's broadcast.
    discovery_wait: Duration,
    /// A command, from `execute` to the device's answer (the Hub gives up
    /// after 5 s).
    command_budget: Duration,
    /// After a command worked, ask for the status again at these times
    /// (from when it was confirmed): the switch reports at once, but a
    /// plug's power, voltage and current catch up a moment later, and the
    /// regular poll would show them only after `poll`.
    follow_ups: [Duration; 3],
}

impl Timing {
    fn new(config: &TuyaConfig) -> Self {
        let poll = config.poll_interval();
        let heartbeat = Duration::from_secs(10);
        Self {
            poll,
            heartbeat,
            io: Duration::from_secs(5),
            silence: heartbeat.min(poll).saturating_mul(3) + Duration::from_secs(5),
            min_backoff: Duration::from_secs(1),
            max_backoff: Duration::from_secs(60),
            offline_after: 2,
            discovery_wait: Duration::from_secs(15),
            command_budget: Duration::from_millis(4_500),
            follow_ups: [
                Duration::from_secs(1),
                Duration::from_millis(2_500),
                Duration::from_secs(5),
            ],
        }
    }

    fn backoff(&self, failures: u32) -> Duration {
        let doublings = failures.saturating_sub(1).min(6);
        self.min_backoff
            .saturating_mul(1 << doublings)
            .min(self.max_backoff)
    }
}

/// Where device addresses come from.
#[derive(Clone)]
enum Locator {
    /// The hub-wide broadcast listener.
    Hub,
    /// A given listener (tests, one-off reads).
    Given(Arc<Discovery>),
}

impl Locator {
    fn discovery(&self) -> Arc<Discovery> {
        match self {
            Self::Hub => Discovery::shared(),
            Self::Given(discovery) => Arc::clone(discovery),
        }
    }
}

/// A command on its way to the device.
struct Pending {
    endpoint: EndpointId,
    dps: Map<String, Value>,
    /// The caller gave up after this: never send it later.
    deadline: Instant,
    done: oneshot::Sender<Result<(), DriverError>>,
}

/// A command sent, waiting for the device's answer.
struct InFlight {
    endpoint: EndpointId,
    cmd: u32,
    dps: Map<String, Value>,
    deadline: Instant,
    done: oneshot::Sender<Result<(), DriverError>>,
}

impl InFlight {
    /// Answers the caller. A command that worked has its endpoint's state
    /// published again with the next report, even when nothing changed
    /// (e.g. "on" while on), so the screen sees it confirmed.
    fn finish(self, result: Result<(), DriverError>, state: &mut Loop) {
        if result.is_ok() {
            state
                .published
                .retain(|(endpoint, _), _| endpoint != &self.endpoint);
        }
        let _ = self.done.send(result);
    }
}

/// Why a session ended.
enum End {
    Lost(LinkError),
    /// Another device answered at the address.
    WrongDevice,
    SinkClosed,
}

impl From<LinkError> for End {
    fn from(error: LinkError) -> Self {
        Self::Lost(error)
    }
}

impl From<DriverError> for End {
    /// Publishing only fails when the Hub stopped listening.
    fn from(_: DriverError) -> Self {
        Self::SinkClosed
    }
}

/// What the device loop remembers across sessions.
#[derive(Default)]
struct Loop {
    online: bool,
    failures: u32,
    published: HashMap<(EndpointId, CapabilityKind), CapabilityState>,
    /// The device wants its data points listed in status queries.
    listing: bool,
    warned_version: bool,
    /// "Not answering yet" was logged (once until it answers).
    warned_unreachable: bool,
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs())
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

async fn sleep_until_opt(deadline: Option<Instant>) {
    match deadline {
        Some(deadline) => sleep_until(deadline).await,
        None => std::future::pending().await,
    }
}

/// What a device reports right now (see [`read_status`]).
#[derive(Debug, Clone, PartialEq)]
pub struct Status {
    /// The protocol the session used.
    pub version: Version,
    /// Data point number → value.
    pub values: Map<String, Value>,
}

/// Reads a device's data points once, without changing anything: opens a
/// session, asks for the status, closes. Waits up to `wait` for the
/// device's broadcast when no address is known.
pub async fn read_status(
    config: &TuyaConfig,
    discovery: Arc<Discovery>,
    wait: Duration,
) -> Result<Status, DriverError> {
    let mut timing = Timing::new(config);
    timing.discovery_wait = wait;
    let driver = TuyaDriver::build(config.clone(), timing, Locator::Given(discovery))?;
    let mut state = Loop::default();
    let (mut link, version) = driver.open(&driver.locator.discovery(), &mut state).await?;
    let result = driver.read_once(&mut link, version, &mut state).await;
    link.close().await;
    result.map(|values| Status { version, values })
}

/// One Tuya device.
pub struct TuyaDriver {
    config: TuyaConfig,
    key: crypto::Key,
    model: Model,
    timing: Timing,
    locator: Locator,
    /// The data points as last reported (for toggling and resuming).
    values: Mutex<DpValues>,
    commands_tx: mpsc::Sender<Pending>,
    commands_rx: tokio::sync::Mutex<mpsc::Receiver<Pending>>,
}

impl std::fmt::Debug for TuyaDriver {
    /// Without the ID, key or address.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TuyaDriver")
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}

impl TuyaDriver {
    /// Builds a driver from the device's driver config JSON.
    pub fn from_config(value: &Value) -> Result<Self, DriverError> {
        Self::new(TuyaConfig::from_value(value)?)
    }

    pub fn new(config: TuyaConfig) -> Result<Self, DriverError> {
        let timing = Timing::new(&config);
        Self::build(config, timing, Locator::Hub)
    }

    fn build(config: TuyaConfig, timing: Timing, locator: Locator) -> Result<Self, DriverError> {
        config.validate()?;
        let key = config
            .key()
            .ok_or_else(|| DriverError::InvalidConfig("local_key".to_owned()))?;
        let model = Model::new(config.category.trim(), &config.mapping)?;
        let (commands_tx, commands_rx) = mpsc::channel(4);
        Ok(Self {
            config,
            key,
            model,
            timing,
            locator,
            values: Mutex::new(HashMap::new()),
            commands_tx,
            commands_rx: tokio::sync::Mutex::new(commands_rx),
        })
    }

    pub fn config(&self) -> &TuyaConfig {
        &self.config
    }

    fn id(&self) -> &str {
        self.config.device_id()
    }

    fn configured_version(&self) -> Version {
        // Validated in `build`.
        self.config.version().unwrap_or(Version::V33)
    }

    /// Where to reach the device, and the protocol it announces.
    async fn locate(&self, discovery: &Discovery) -> Option<(SocketAddr, Option<Version>)> {
        let port = self.config.port;
        if let Some(seen) = discovery.fresh(self.id()) {
            return Some((SocketAddr::new(IpAddr::V4(seen.ip), port), seen.version));
        }
        if let Some(host) = &self.config.host {
            if let Ok(ip) = host.parse::<IpAddr>() {
                return Some((SocketAddr::new(ip, port), None));
            }
            if let Ok(Ok(mut found)) = timeout(
                self.timing.io,
                tokio::net::lookup_host((host.as_str(), port)),
            )
            .await
                && let Some(addr) = found.find(SocketAddr::is_ipv4)
            {
                return Some((addr, None));
            }
        }
        let seen = discovery
            .wait_for(self.id(), self.timing.discovery_wait)
            .await?;
        Some((SocketAddr::new(IpAddr::V4(seen.ip), port), seen.version))
    }

    /// Finds the device and opens a session.
    async fn open(
        &self,
        discovery: &Discovery,
        state: &mut Loop,
    ) -> Result<(Link, Version), DriverError> {
        let (addr, announced) = self.locate(discovery).await.ok_or(DriverError::Offline)?;
        let configured = self.configured_version();
        // Firmware updates change the protocol; the device's own broadcast
        // is the truth.
        let version = match announced {
            Some(announced) if announced != configured => {
                if !state.warned_version {
                    state.warned_version = true;
                    tracing::warn!(
                        %configured,
                        %announced,
                        "the device announces another protocol version than configured; using it"
                    );
                }
                announced
            }
            _ => configured,
        };
        tracing::debug!(%addr, %version, "connecting to the device");
        match Link::open(addr, version, self.key, self.timing.io).await {
            Ok(link) => Ok((link, version)),
            Err(LinkError::Unreadable | LinkError::Protocol(ProtocolError::HandshakeRejected)) => {
                tracing::warn!(%addr, "the device at this address does not accept its local key");
                Err(DriverError::Protocol("local key not accepted".to_owned()))
            }
            Err(LinkError::Timeout) => Err(DriverError::Timeout),
            Err(error) => Err(DriverError::Protocol(error.to_string())),
        }
    }

    /// One status query and its answer (for [`read_status`]).
    async fn read_once(
        &self,
        link: &mut Link,
        version: Version,
        state: &mut Loop,
    ) -> Result<Map<String, Value>, DriverError> {
        let deadline = Instant::now() + self.timing.io.saturating_mul(2);
        let protocol_error = |error: LinkError| DriverError::Protocol(error.to_string());
        link.send(&self.status_request(version, false), self.timing.io)
            .await
            .map_err(protocol_error)?;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let message = timeout(left, link.recv())
                .await
                .map_err(|_| DriverError::Timeout)?
                .map_err(protocol_error)?;
            if version == Version::V33
                && !state.listing
                && protocol::refuses_plain_query(&message.payload)
            {
                state.listing = true;
                link.send(&self.status_request(version, true), self.timing.io)
                    .await
                    .map_err(protocol_error)?;
                continue;
            }
            if let Some(values) = protocol::data_points(&message.payload) {
                return Ok(values);
            }
        }
    }

    fn values(&self) -> DpValues {
        lock(&self.values).clone()
    }

    fn status_request(&self, version: Version, listing: bool) -> Request {
        if listing {
            protocol::status_query_listing(self.id(), &self.listed_dps(), now_secs())
        } else {
            protocol::status_query(version, self.id(), now_secs())
        }
    }

    /// The data points a listing status query names.
    fn listed_dps(&self) -> Vec<String> {
        let mut dps: Vec<String> = self.config.mapping.keys().cloned().collect();
        if dps.is_empty() {
            dps.push("1".to_owned());
        }
        dps
    }

    /// Publishes what changed.
    async fn publish(&self, sink: &EventSink, state: &mut Loop) -> Result<(), DriverError> {
        let values = self.values();
        for (endpoint, capability) in self.model.states(&values) {
            let key = (endpoint.clone(), capability.kind());
            if state.published.get(&key) != Some(&capability) {
                sink.state(&endpoint, capability.clone()).await?;
                state.published.insert(key, capability);
            }
        }
        Ok(())
    }

    /// Waits `delay` while not connected, refusing commands at once (they
    /// are never kept for later).
    async fn wait_offline(&self, commands: &mut mpsc::Receiver<Pending>, delay: Duration) {
        let until = Instant::now() + delay;
        loop {
            tokio::select! {
                () = sleep_until(until) => return,
                pending = commands.recv() => match pending {
                    Some(pending) => {
                        let _ = pending.done.send(Err(DriverError::Offline));
                    }
                    None => {
                        sleep_until(until).await;
                        return;
                    }
                },
            }
        }
    }

    /// Takes a message from the device into account. Returns whether it
    /// confirmed the command in flight (by answering it, or by reporting
    /// its new values).
    async fn handle(
        &self,
        message: protocol::Message,
        version: Version,
        link: &mut Link,
        sink: &EventSink,
        state: &mut Loop,
        inflight: &mut Option<InFlight>,
    ) -> Result<bool, End> {
        if message.cmd == cmd::HEART_BEAT {
            return Ok(false);
        }
        let mut answered = false;
        if inflight
            .as_ref()
            .is_some_and(|flight| flight.cmd == message.cmd)
            && let Some(flight) = inflight.take()
        {
            if message.is_error() {
                tracing::warn!(code = message.retcode, "the device refused the command");
                flight.finish(
                    Err(DriverError::Protocol(
                        "the device refused the command".to_owned(),
                    )),
                    state,
                );
            } else {
                flight.finish(Ok(()), state);
                answered = true;
            }
        }
        if let Some(theirs) = protocol::device_id(&message.payload)
            && theirs != self.id()
        {
            return Err(End::WrongDevice);
        }
        if version == Version::V33
            && !state.listing
            && protocol::refuses_plain_query(&message.payload)
        {
            tracing::info!("the device wants its data points listed in status queries");
            state.listing = true;
            link.send(&self.status_request(version, true), self.timing.io)
                .await?;
            return Ok(answered);
        }
        let Some(dps) = protocol::data_points(&message.payload) else {
            return Ok(answered);
        };
        {
            let mut values = lock(&self.values);
            for (dp, value) in &dps {
                values.insert(dp.clone(), value.clone());
            }
        }
        if !state.online {
            state.online = true;
            state.warned_unreachable = false;
            tracing::info!(%version, "the device answers");
            sink.availability(true).await?;
        }
        state.failures = 0;
        // Some devices only report the change instead of answering the
        // command itself.
        if inflight.as_ref().is_some_and(|flight| {
            flight
                .dps
                .iter()
                .all(|(dp, value)| dps.get(dp) == Some(value))
        }) && let Some(flight) = inflight.take()
        {
            flight.finish(Ok(()), state);
            answered = true;
        }
        self.publish(sink, state).await?;
        Ok(answered)
    }

    /// One session: status, pushed updates, polls, heartbeats, commands.
    async fn session(
        &self,
        link: &mut Link,
        version: Version,
        sink: &EventSink,
        commands: &mut mpsc::Receiver<Pending>,
        state: &mut Loop,
    ) -> Result<(), End> {
        let timing = self.timing;
        link.send(&self.status_request(version, state.listing), timing.io)
            .await?;
        let mut last_rx = Instant::now();
        let mut last_tx = Instant::now();
        let mut next_poll = last_tx + timing.poll;
        // Extra status queries after a command, soonest first.
        let mut follow_ups: Vec<Instant> = Vec::new();
        let mut inflight: Option<InFlight> = None;
        loop {
            let heartbeat_at = last_tx + timing.heartbeat;
            let silent_at = last_rx + timing.silence;
            let inflight_until = inflight.as_ref().map(|flight| flight.deadline);
            let query_at = follow_ups
                .first()
                .map_or(next_poll, |at| next_poll.min(*at));
            tokio::select! {
                message = link.recv() => {
                    last_rx = Instant::now();
                    if self.handle(message?, version, link, sink, state, &mut inflight).await? {
                        // Show the result soon (power, voltage and current
                        // included), even if the device does not report it
                        // by itself.
                        let confirmed = Instant::now();
                        follow_ups = timing.follow_ups.iter().map(|after| confirmed + *after).collect();
                    }
                }
                () = sleep_until(query_at) => {
                    link.send(&self.status_request(version, state.listing), timing.io).await?;
                    last_tx = Instant::now();
                    next_poll = last_tx + timing.poll;
                    follow_ups.retain(|at| *at > last_tx);
                }
                () = sleep_until(heartbeat_at) => {
                    link.send(&protocol::heartbeat(self.id()), timing.io).await?;
                    last_tx = Instant::now();
                }
                () = sleep_until(silent_at) => {
                    if let Some(flight) = inflight.take() {
                        flight.finish(Err(DriverError::Offline), state);
                    }
                    return Err(End::Lost(LinkError::Timeout));
                }
                () = sleep_until_opt(inflight_until) => {
                    if let Some(flight) = inflight.take() {
                        tracing::info!("the device did not answer the command in time");
                        flight.finish(Err(DriverError::Timeout), state);
                    }
                }
                pending = commands.recv(), if inflight.is_none() => {
                    let Some(pending) = pending else {
                        // The driver holds the sender: never happens.
                        continue;
                    };
                    if Instant::now() >= pending.deadline {
                        let _ = pending.done.send(Err(DriverError::Timeout));
                        continue;
                    }
                    if !state.online {
                        let _ = pending.done.send(Err(DriverError::Offline));
                        continue;
                    }
                    let request = protocol::control(version, self.id(), &pending.dps, now_secs());
                    if let Err(error) = link.send(&request, timing.io).await {
                        let _ = pending.done.send(Err(DriverError::Offline));
                        return Err(error.into());
                    }
                    tracing::info!(
                        data_points = ?pending.dps.keys().collect::<Vec<_>>(),
                        "sent to the device"
                    );
                    last_tx = Instant::now();
                    inflight = Some(InFlight {
                        endpoint: pending.endpoint,
                        cmd: request.cmd,
                        dps: pending.dps,
                        deadline: pending.deadline,
                        done: pending.done,
                    });
                }
            }
        }
    }

    /// Counts a failed attempt; reports the device offline after a few.
    async fn failed(
        &self,
        sink: &EventSink,
        state: &mut Loop,
        why: &str,
    ) -> Result<(), DriverError> {
        state.failures = state.failures.saturating_add(1);
        if state.online && state.failures >= self.timing.offline_after {
            state.online = false;
            tracing::warn!(why, "the device stopped answering");
            sink.availability(false).await?;
        } else if !state.online && !state.warned_unreachable {
            state.warned_unreachable = true;
            tracing::warn!(
                why,
                "the device does not answer yet; trying again in the background"
            );
        } else {
            tracing::debug!(why, failures = state.failures, "reaching the device failed");
        }
        Ok(())
    }
}

#[async_trait]
impl DeviceDriver for TuyaDriver {
    fn describe(&self) -> DeviceDescriptor {
        match self.model.endpoints(self.config.read_only) {
            Ok(endpoints) => DeviceDescriptor { endpoints },
            Err(error) => {
                tracing::error!(%error, "cannot describe the device");
                DeviceDescriptor::default()
            }
        }
    }

    async fn execute(&self, endpoint: &EndpointId, cmd: Command) -> Result<(), DriverError> {
        if !self.model.knows(endpoint) {
            return Err(DriverError::UnknownEndpoint(endpoint.clone()));
        }
        if self.config.read_only {
            tracing::info!("command refused: the device is set up as read-only");
            return Err(DriverError::ReadOnly);
        }
        let dps = self.model.plan(endpoint, &cmd, &self.values())?;
        let deadline = Instant::now() + self.timing.command_budget;
        let (done, answer) = oneshot::channel();
        let pending = Pending {
            endpoint: endpoint.clone(),
            dps,
            deadline,
            done,
        };
        timeout(self.timing.command_budget, self.commands_tx.send(pending))
            .await
            .map_err(|_| DriverError::Timeout)?
            .map_err(|_| DriverError::Offline)?;
        match timeout(deadline.saturating_duration_since(Instant::now()), answer).await {
            Ok(Ok(result)) => result,
            // The device loop is not running.
            Ok(Err(_)) => Err(DriverError::Offline),
            Err(_) => Err(DriverError::Timeout),
        }
    }

    async fn run(&self, sink: EventSink) -> Result<(), DriverError> {
        // One loop at a time; a restarted loop picks up where the last one
        // stopped.
        let mut commands = self.commands_rx.lock().await;
        if self.model == Model::Unsupported {
            // Nothing to show: never talk to the device.
            while let Some(pending) = commands.recv().await {
                let _ = pending.done.send(Err(DriverError::Unsupported));
            }
            return Ok(());
        }
        let discovery = self.locator.discovery();
        let mut state = Loop::default();
        loop {
            let (mut link, version) = match self.open(&discovery, &mut state).await {
                Ok(opened) => opened,
                Err(error) => {
                    self.failed(&sink, &mut state, &error.to_string()).await?;
                    let delay = self.timing.backoff(state.failures);
                    self.wait_offline(&mut commands, delay).await;
                    continue;
                }
            };
            let ended = self
                .session(&mut link, version, &sink, &mut commands, &mut state)
                .await;
            link.close().await;
            let why = match ended {
                Ok(()) => "the session ended".to_owned(),
                Err(End::SinkClosed) => return Err(DriverError::SinkClosed),
                Err(End::WrongDevice) => {
                    tracing::warn!(
                        "a different device answers at this address; it is not controlled"
                    );
                    "a different device answered".to_owned()
                }
                Err(End::Lost(error)) => error.to_string(),
            };
            self.failed(&sink, &mut state, &why).await?;
            // The first retry after a lost session is quick.
            let delay = if state.failures <= 1 {
                self.timing.min_backoff
            } else {
                self.timing.backoff(state.failures)
            };
            self.wait_offline(&mut commands, delay).await;
        }
    }
}
