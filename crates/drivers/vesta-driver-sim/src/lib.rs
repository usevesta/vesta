//! Simulated devices: a whole home without hardware.
//!
//! Used by `vesta-hubd --dev` and by tests. One driver instance simulates one
//! device, picked by `model` in the device's driver config:
//!
//! ```json
//! { "model": "blind", "latency_ms": 150, "failure_rate": 0.0 }
//! ```
//!
//! | `model` | Endpoints (type) | Capabilities |
//! |---|---|---|
//! | `light` | `main` (`light`) | `on_off`, `level`, `color_temperature` |
//! | `ceiling_fan` | `fan` (`fan`), `light` (`light`) | `on_off` + `fan` (6 steps, reversible); `on_off` + `level` |
//! | `blind` | `main` (`window_covering`) | `cover` with position and stop, moves ~10 %/s |
//! | `garage_door` | `main` (`garage_door`) | `cover` without position, ~5 s travel, reports open/closed |
//! | `door_strike` | `main` (`door`) | `trigger` |
//! | `doorbell` | `main` (`doorbell`) | `doorbell`, `occupancy` |
//!
//! Options:
//! - `latency_ms` (default 150): delay before a command is applied, so the
//!   UI's pending state is visible. Values above the Hub's command timeout
//!   (5 s) exercise the `timeout` path.
//! - `failure_rate` (`0.0..=1.0`, default 0): probability that a command
//!   fails, to exercise error paths.
//! - `seed`: makes failures deterministic (tests).
//! - `ring_every_sec` (`doorbell` only): rings periodically, emitting
//!   `occupancy.detected` and `doorbell.pressed`.

use std::cmp::Ordering;
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use serde::Deserialize;
use tokio::sync::mpsc;
use tokio::time::{Instant, sleep, sleep_until};
use vesta_core::{
    Capabilities, CapabilityKind, CapabilityState, ColorCommand, ColorTemperatureCapability,
    ColorTemperatureCommand, ColorTemperatureFeatures, ColorTemperatureState, Command,
    CoverCapability, CoverCommand, CoverFeatures, CoverKind, CoverMotion, CoverState,
    DeviceDescriptor, DeviceDriver, DoorbellCapability, DriverError, Endpoint, EndpointId,
    EndpointType, EventKind, EventSink, FanCapability, FanCommand, FanDirection, FanFeatures,
    FanState, LevelCapability, LevelCommand, LevelState, OccupancyCapability, OccupancyState,
    OnOffCapability, OnOffCommand, OnOffState, Percent, TriggerCapability, TriggerCommand,
    async_trait,
};

/// Default simulated command latency.
pub const DEFAULT_LATENCY_MS: u64 = 150;
/// Upper bound for `latency_ms`.
pub const MAX_LATENCY_MS: u64 = 60_000;

/// How often moving covers report their progress.
const MOTION_TICK: Duration = Duration::from_millis(500);
/// Blind: 5 % per tick = 10 %/s.
const BLIND_STEP: u8 = 5;
/// Garage door: 10 % per tick = fully open or closed in 5 s.
const GARAGE_STEP: u8 = 10;
/// How long the doorbell reports someone at the door after a ring.
const OCCUPANCY_HOLD: Duration = Duration::from_secs(10);
/// Buffered state notifications between `execute` and `run`.
const CHANGE_BUFFER: usize = 64;

/// Which device to simulate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SimModel {
    Light,
    CeilingFan,
    Blind,
    GarageDoor,
    DoorStrike,
    Doorbell,
}

/// The device's driver config (`devices[].config` in `config.json`).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SimConfig {
    pub model: SimModel,
    #[serde(default = "default_latency_ms")]
    pub latency_ms: u64,
    #[serde(default)]
    pub failure_rate: f64,
    #[serde(default)]
    pub ring_every_sec: Option<u64>,
    #[serde(default)]
    pub seed: Option<u64>,
}

const fn default_latency_ms() -> u64 {
    DEFAULT_LATENCY_MS
}

impl SimConfig {
    /// A config with default options for `model`.
    pub const fn new(model: SimModel) -> Self {
        Self {
            model,
            latency_ms: DEFAULT_LATENCY_MS,
            failure_rate: 0.0,
            ring_every_sec: None,
            seed: None,
        }
    }

    /// Parses and validates the driver config JSON.
    pub fn from_value(value: &serde_json::Value) -> Result<Self, DriverError> {
        let config =
            Self::deserialize(value).map_err(|e| DriverError::InvalidConfig(e.to_string()))?;
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<(), DriverError> {
        let invalid = |reason: &str| Err(DriverError::InvalidConfig(reason.to_owned()));
        if !(0.0..=1.0).contains(&self.failure_rate) {
            return invalid("failure_rate must be between 0.0 and 1.0");
        }
        if self.latency_ms > MAX_LATENCY_MS {
            return invalid("latency_ms must be at most 60000");
        }
        match self.ring_every_sec {
            Some(_) if self.model != SimModel::Doorbell => {
                invalid("ring_every_sec is only valid for the doorbell model")
            }
            Some(0) => invalid("ring_every_sec must be at least 1"),
            _ => Ok(()),
        }
    }
}

/// One simulated device.
pub struct SimDriver {
    config: SimConfig,
    state: Mutex<SimState>,
    rng: Mutex<fastrand::Rng>,
    /// `execute` tells the `run` loop which capability changed; `run`
    /// publishes the *current* state, so concurrent commands can never
    /// publish out of order.
    changed_tx: mpsc::Sender<Changed>,
    changed_rx: tokio::sync::Mutex<mpsc::Receiver<Changed>>,
}

impl std::fmt::Debug for SimDriver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SimDriver")
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}

#[derive(Debug)]
struct Changed {
    endpoint: EndpointId,
    kind: CapabilityKind,
}

#[derive(Debug)]
struct SimState {
    endpoints: Vec<SimEndpoint>,
}

#[derive(Debug)]
struct SimEndpoint {
    /// Description plus current state (kept in the capabilities).
    endpoint: Endpoint,
    /// Covers only: where the cover is and where it is going.
    motion: Option<Motion>,
}

#[derive(Debug, Clone, Copy)]
struct Motion {
    /// Tracked even when the hardware cannot report it (garage door).
    position: u8,
    target: u8,
    step: u8,
    /// Has an end-stop contact, so it reports `open`.
    end_stop: bool,
}

impl Motion {
    fn state(self, reports_position: bool) -> CoverState {
        CoverState {
            position: reports_position.then(|| Percent::saturating(self.position)),
            motion: match self.target.cmp(&self.position) {
                Ordering::Greater => CoverMotion::Opening,
                Ordering::Less => CoverMotion::Closing,
                Ordering::Equal => CoverMotion::Stopped,
            },
            open: self.end_stop.then_some(self.position > 0),
        }
    }

    fn is_moving(self) -> bool {
        self.position != self.target
    }

    fn advance(&mut self) {
        self.position = match self.target.cmp(&self.position) {
            Ordering::Greater => self.position.saturating_add(self.step).min(self.target),
            Ordering::Less => self.position.saturating_sub(self.step).max(self.target),
            Ordering::Equal => self.position,
        };
    }
}

impl SimEndpoint {
    fn sync_cover_state(&mut self) {
        if let (Some(motion), Some(cover)) =
            (self.motion, self.endpoint.capabilities.cover.as_mut())
        {
            cover.state = Some(motion.state(cover.features.position));
        }
    }

    fn state_of(&self, kind: CapabilityKind) -> Option<CapabilityState> {
        let caps = &self.endpoint.capabilities;
        match kind {
            CapabilityKind::OnOff => caps.on_off.as_ref()?.state.map(Into::into),
            CapabilityKind::Level => caps.level.as_ref()?.state.map(Into::into),
            CapabilityKind::ColorTemperature => {
                caps.color_temperature.as_ref()?.state.map(Into::into)
            }
            CapabilityKind::Color => caps.color.as_ref()?.state.map(Into::into),
            CapabilityKind::Cover => caps.cover.as_ref()?.state.map(Into::into),
            CapabilityKind::Fan => caps.fan.as_ref()?.state.map(Into::into),
            CapabilityKind::Occupancy => caps.occupancy.as_ref()?.state.map(Into::into),
            _ => None,
        }
    }
}

fn endpoint_id(raw: &str) -> Result<EndpointId, DriverError> {
    EndpointId::new(raw).map_err(DriverError::other)
}

fn percent(value: u8) -> Percent {
    Percent::saturating(value)
}

fn endpoint(
    id: EndpointId,
    name: &str,
    endpoint_type: EndpointType,
    caps: Capabilities,
) -> Endpoint {
    Endpoint {
        id,
        name: name.to_owned(),
        endpoint_type,
        room: None,
        capabilities: caps,
    }
}

fn plain(endpoint: Endpoint) -> SimEndpoint {
    SimEndpoint {
        endpoint,
        motion: None,
    }
}

fn on_off(on: bool) -> Option<OnOffCapability> {
    Some(OnOffCapability {
        state: Some(OnOffState { on }),
    })
}

fn level(value: u8) -> Option<LevelCapability> {
    Some(LevelCapability {
        state: Some(LevelState {
            level: percent(value),
        }),
    })
}

fn cover(
    id: EndpointId,
    name: &str,
    endpoint_type: EndpointType,
    features: CoverFeatures,
    motion: Motion,
) -> SimEndpoint {
    let mut sim = SimEndpoint {
        endpoint: endpoint(
            id,
            name,
            endpoint_type,
            Capabilities {
                cover: Some(CoverCapability {
                    features,
                    state: None,
                }),
                ..Capabilities::default()
            },
        ),
        motion: Some(motion),
    };
    sim.sync_cover_state();
    sim
}

impl SimModel {
    /// Endpoints with their initial state.
    fn endpoints(self) -> Result<Vec<SimEndpoint>, DriverError> {
        let main = EndpointId::main();
        Ok(match self {
            Self::Light => vec![plain(endpoint(
                main,
                "Light",
                EndpointType::Light,
                Capabilities {
                    on_off: on_off(false),
                    level: level(80),
                    color_temperature: Some(ColorTemperatureCapability {
                        features: ColorTemperatureFeatures {
                            min_kelvin: 2700,
                            max_kelvin: 6500,
                        },
                        state: Some(ColorTemperatureState { kelvin: 3000 }),
                    }),
                    ..Capabilities::default()
                },
            ))],
            Self::CeilingFan => vec![
                plain(endpoint(
                    endpoint_id("fan")?,
                    "Fan",
                    EndpointType::Fan,
                    Capabilities {
                        on_off: on_off(false),
                        fan: Some(FanCapability {
                            features: FanFeatures {
                                speed_steps: 6,
                                reversible: true,
                            },
                            state: Some(FanState {
                                speed: 3,
                                direction: FanDirection::Forward,
                            }),
                        }),
                        ..Capabilities::default()
                    },
                )),
                plain(endpoint(
                    endpoint_id("light")?,
                    "Light",
                    EndpointType::Light,
                    Capabilities {
                        on_off: on_off(false),
                        level: level(100),
                        ..Capabilities::default()
                    },
                )),
            ],
            Self::Blind => vec![cover(
                main,
                "Blind",
                EndpointType::WindowCovering,
                CoverFeatures {
                    kind: CoverKind::Blind,
                    position: true,
                    stop: true,
                },
                Motion {
                    position: 100,
                    target: 100,
                    step: BLIND_STEP,
                    end_stop: false,
                },
            )],
            Self::GarageDoor => vec![cover(
                main,
                "Garage door",
                EndpointType::GarageDoor,
                CoverFeatures {
                    kind: CoverKind::Garage,
                    position: false,
                    stop: false,
                },
                Motion {
                    position: 0,
                    target: 0,
                    step: GARAGE_STEP,
                    end_stop: true,
                },
            )],
            Self::DoorStrike => vec![plain(endpoint(
                main,
                "Door",
                EndpointType::Door,
                Capabilities {
                    trigger: Some(TriggerCapability {}),
                    ..Capabilities::default()
                },
            ))],
            Self::Doorbell => vec![plain(endpoint(
                main,
                "Doorbell",
                EndpointType::Doorbell,
                Capabilities {
                    doorbell: Some(DoorbellCapability {}),
                    occupancy: Some(OccupancyCapability {
                        state: Some(OccupancyState { occupied: false }),
                    }),
                    ..Capabilities::default()
                },
            ))],
        })
    }
}

impl SimDriver {
    pub fn new(config: SimConfig) -> Result<Self, DriverError> {
        config.validate()?;
        let endpoints = config.model.endpoints()?;
        let rng = match config.seed {
            Some(seed) => fastrand::Rng::with_seed(seed),
            None => fastrand::Rng::new(),
        };
        let (changed_tx, changed_rx) = mpsc::channel(CHANGE_BUFFER);
        Ok(Self {
            config,
            state: Mutex::new(SimState { endpoints }),
            rng: Mutex::new(rng),
            changed_tx,
            changed_rx: tokio::sync::Mutex::new(changed_rx),
        })
    }

    /// Builds a driver from the device's driver config JSON.
    pub fn from_config(value: &serde_json::Value) -> Result<Self, DriverError> {
        Self::new(SimConfig::from_value(value)?)
    }

    pub fn config(&self) -> &SimConfig {
        &self.config
    }

    fn lock_state(&self) -> MutexGuard<'_, SimState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn should_fail(&self) -> bool {
        if self.config.failure_rate <= 0.0 {
            return false;
        }
        let roll = self
            .rng
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .f64();
        roll < self.config.failure_rate
    }

    /// Rejects commands for unknown endpoints or missing capabilities
    /// before simulating any latency, like a real device would locally.
    fn check(&self, endpoint: &EndpointId, cmd: &Command) -> Result<(), DriverError> {
        let state = self.lock_state();
        let sim = state
            .endpoints
            .iter()
            .find(|sim| &sim.endpoint.id == endpoint)
            .ok_or_else(|| DriverError::UnknownEndpoint(endpoint.clone()))?;
        if sim.endpoint.capabilities.supports(cmd) {
            Ok(())
        } else {
            Err(DriverError::Unsupported)
        }
    }

    /// Applies a command to the simulated state and returns the
    /// capabilities whose state changed.
    fn apply(
        &self,
        endpoint: &EndpointId,
        cmd: Command,
    ) -> Result<Vec<CapabilityKind>, DriverError> {
        let mut state = self.lock_state();
        let sim = state
            .endpoints
            .iter_mut()
            .find(|sim| &sim.endpoint.id == endpoint)
            .ok_or_else(|| DriverError::UnknownEndpoint(endpoint.clone()))?;
        let caps = &mut sim.endpoint.capabilities;
        if !caps.supports(&cmd) {
            return Err(DriverError::Unsupported);
        }
        let mut changed = Vec::new();
        match cmd {
            Command::OnOff(action) => {
                let cap = caps.on_off.as_mut().ok_or(DriverError::Unsupported)?;
                let on = match action {
                    OnOffCommand::On => true,
                    OnOffCommand::Off => false,
                    OnOffCommand::Toggle => !cap.state.is_some_and(|s| s.on),
                };
                cap.state = Some(OnOffState { on });
                changed.push(CapabilityKind::OnOff);
            }
            Command::Level(LevelCommand::Set { level, .. }) => {
                let cap = caps.level.as_mut().ok_or(DriverError::Unsupported)?;
                cap.state = Some(LevelState { level });
                // Like Matter's MoveToLevelWithOnOff: dragging the slider
                // lights the lamp.
                if level.get() > 0 && turn_on(caps) {
                    changed.push(CapabilityKind::OnOff);
                }
                changed.push(CapabilityKind::Level);
            }
            Command::ColorTemperature(ColorTemperatureCommand::Set { kelvin, .. }) => {
                let cap = caps
                    .color_temperature
                    .as_mut()
                    .ok_or(DriverError::Unsupported)?;
                let features = cap.features;
                let kelvin = kelvin.clamp(
                    features.min_kelvin.min(features.max_kelvin),
                    features.max_kelvin.max(features.min_kelvin),
                );
                cap.state = Some(ColorTemperatureState { kelvin });
                changed.push(CapabilityKind::ColorTemperature);
            }
            Command::Color(ColorCommand::Set {
                hue, saturation, ..
            }) => {
                let cap = caps.color.as_mut().ok_or(DriverError::Unsupported)?;
                cap.state = Some(vesta_core::ColorState { hue, saturation });
                changed.push(CapabilityKind::Color);
            }
            Command::Fan(action) => {
                let cap = caps.fan.as_mut().ok_or(DriverError::Unsupported)?;
                let mut fan = cap.state.unwrap_or(FanState {
                    speed: 1,
                    direction: FanDirection::Forward,
                });
                let starts = match action {
                    FanCommand::SetSpeed { speed } => {
                        fan.speed = speed;
                        true
                    }
                    FanCommand::SetDirection { direction } => {
                        fan.direction = direction;
                        false
                    }
                };
                cap.state = Some(fan);
                // Picking a speed starts the fan, like the level slider.
                if starts && turn_on(caps) {
                    changed.push(CapabilityKind::OnOff);
                }
                changed.push(CapabilityKind::Fan);
            }
            Command::Cover(action) => {
                let motion = sim.motion.as_mut().ok_or(DriverError::Unsupported)?;
                motion.target = match action {
                    CoverCommand::Open => 100,
                    CoverCommand::Close => 0,
                    CoverCommand::Stop => motion.position,
                    CoverCommand::SetPosition { position } => position.get(),
                };
                sim.sync_cover_state();
                changed.push(CapabilityKind::Cover);
            }
            Command::Trigger(TriggerCommand::Fire) => {
                tracing::info!(endpoint = %endpoint, "simulated trigger fired");
            }
            Command::Lock(_) | Command::EvCharger(_) => return Err(DriverError::Unsupported),
        }
        Ok(changed)
    }

    fn state_of(&self, endpoint: &EndpointId, kind: CapabilityKind) -> Option<CapabilityState> {
        let state = self.lock_state();
        state
            .endpoints
            .iter()
            .find(|sim| &sim.endpoint.id == endpoint)?
            .state_of(kind)
    }

    fn all_states(&self) -> Vec<(EndpointId, CapabilityState)> {
        let state = self.lock_state();
        state
            .endpoints
            .iter()
            .flat_map(|sim| {
                sim.endpoint
                    .capabilities
                    .kinds()
                    .into_iter()
                    .filter_map(|kind| sim.state_of(kind))
                    .map(|s| (sim.endpoint.id.clone(), s))
            })
            .collect()
    }

    fn is_moving(&self) -> bool {
        self.lock_state()
            .endpoints
            .iter()
            .any(|sim| sim.motion.is_some_and(Motion::is_moving))
    }

    /// Moves every moving cover one step. Returns the covers whose visible
    /// state changed (a garage door without position only reports start,
    /// end and the end-stop contact).
    fn step_covers(&self) -> Vec<(EndpointId, CapabilityState)> {
        let mut state = self.lock_state();
        let mut updates = Vec::new();
        for sim in &mut state.endpoints {
            let Some(motion) = sim.motion.as_mut() else {
                continue;
            };
            if !motion.is_moving() {
                continue;
            }
            motion.advance();
            let before = sim.state_of(CapabilityKind::Cover);
            sim.sync_cover_state();
            let after = sim.state_of(CapabilityKind::Cover);
            if let Some(after) = after.filter(|after| Some(after) != before.as_ref()) {
                updates.push((sim.endpoint.id.clone(), after));
            }
        }
        updates
    }

    /// Sets the doorbell's occupancy. Returns the new state if it exists.
    fn set_occupied(&self, occupied: bool) -> Option<CapabilityState> {
        let mut state = self.lock_state();
        let sim = state
            .endpoints
            .iter_mut()
            .find(|sim| sim.endpoint.capabilities.occupancy.is_some())?;
        let cap = sim.endpoint.capabilities.occupancy.as_mut()?;
        cap.state = Some(OccupancyState { occupied });
        cap.state.map(Into::into)
    }

    async fn ring(&self, sink: &EventSink) -> Result<(), DriverError> {
        tracing::info!(device = %sink.device(), "simulated doorbell ring");
        let main = EndpointId::main();
        if let Some(state) = self.set_occupied(true) {
            sink.state(&main, state).await?;
        }
        sink.event(&main, EventKind::OccupancyDetected).await?;
        sink.event(&main, EventKind::DoorbellPressed).await
    }
}

/// Turns the endpoint on if it has `on_off` and is off. Returns whether it
/// changed.
fn turn_on(caps: &mut Capabilities) -> bool {
    match caps.on_off.as_mut() {
        Some(cap) if !cap.state.is_some_and(|s| s.on) => {
            cap.state = Some(OnOffState { on: true });
            true
        }
        _ => false,
    }
}

async fn sleep_until_opt(deadline: Option<Instant>) {
    match deadline {
        Some(deadline) => sleep_until(deadline).await,
        None => std::future::pending().await,
    }
}

#[async_trait]
impl DeviceDriver for SimDriver {
    fn describe(&self) -> DeviceDescriptor {
        DeviceDescriptor {
            endpoints: self
                .lock_state()
                .endpoints
                .iter()
                .map(|sim| sim.endpoint.clone())
                .collect(),
        }
    }

    async fn execute(&self, endpoint: &EndpointId, cmd: Command) -> Result<(), DriverError> {
        self.check(endpoint, &cmd)?;
        if self.config.latency_ms > 0 {
            sleep(Duration::from_millis(self.config.latency_ms)).await;
        }
        if self.should_fail() {
            return Err(DriverError::Protocol("simulated failure".to_owned()));
        }
        for kind in self.apply(endpoint, cmd)? {
            self.changed_tx
                .send(Changed {
                    endpoint: endpoint.clone(),
                    kind,
                })
                .await
                .map_err(|_| DriverError::SinkClosed)?;
        }
        Ok(())
    }

    async fn run(&self, sink: EventSink) -> Result<(), DriverError> {
        // One loop at a time; a restarted loop picks up where the last one
        // stopped.
        let mut changed = self.changed_rx.lock().await;
        sink.availability(true).await?;
        for (endpoint, state) in self.all_states() {
            sink.state(&endpoint, state).await?;
        }

        let ring_every = self.config.ring_every_sec.map(Duration::from_secs);
        let mut next_ring = ring_every.map(|every| Instant::now() + every);
        let mut clear_occupancy: Option<Instant> = None;
        let mut next_step: Option<Instant> = None;
        loop {
            if self.is_moving() {
                next_step.get_or_insert_with(|| Instant::now() + MOTION_TICK);
            } else {
                next_step = None;
            }
            tokio::select! {
                update = changed.recv() => {
                    let Some(Changed { endpoint, kind }) = update else {
                        return Ok(());
                    };
                    if let Some(state) = self.state_of(&endpoint, kind) {
                        sink.state(&endpoint, state).await?;
                    }
                }
                () = sleep_until_opt(next_step) => {
                    next_step = None;
                    for (endpoint, state) in self.step_covers() {
                        sink.state(&endpoint, state).await?;
                    }
                }
                () = sleep_until_opt(next_ring) => {
                    next_ring = ring_every.map(|every| Instant::now() + every);
                    self.ring(&sink).await?;
                    clear_occupancy = Some(Instant::now() + OCCUPANCY_HOLD);
                }
                () = sleep_until_opt(clear_occupancy) => {
                    clear_occupancy = None;
                    if let Some(state) = self.set_occupied(false) {
                        sink.state(&EndpointId::main(), state).await?;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;
