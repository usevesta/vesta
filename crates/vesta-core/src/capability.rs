//! Capability catalog (`docs/architecture/capabilities.md`, "Capability Catalog").
//!
//! Each capability has static `features` (what the hardware can do, reported
//! by the driver) and dynamic `state`. `state` is omitted while the driver
//! does not know it yet (e.g. right after start-up); a driver never invents a
//! value.

use std::fmt;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::command::{Command, CoverCommand, EvChargerCommand, FanCommand, LockCommand};
use crate::units::{Hue, Percent};

/// Name of a capability, as used for the keys of [`Capabilities`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityKind {
    OnOff,
    Level,
    ColorTemperature,
    Color,
    Cover,
    Fan,
    Lock,
    Trigger,
    Camera,
    Doorbell,
    Occupancy,
    Contact,
    Temperature,
    Humidity,
    PowerMeter,
    EvCharger,
    Battery,
}

impl CapabilityKind {
    pub const ALL: [Self; 17] = [
        Self::OnOff,
        Self::Level,
        Self::ColorTemperature,
        Self::Color,
        Self::Cover,
        Self::Fan,
        Self::Lock,
        Self::Trigger,
        Self::Camera,
        Self::Doorbell,
        Self::Occupancy,
        Self::Contact,
        Self::Temperature,
        Self::Humidity,
        Self::PowerMeter,
        Self::EvCharger,
        Self::Battery,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::OnOff => "on_off",
            Self::Level => "level",
            Self::ColorTemperature => "color_temperature",
            Self::Color => "color",
            Self::Cover => "cover",
            Self::Fan => "fan",
            Self::Lock => "lock",
            Self::Trigger => "trigger",
            Self::Camera => "camera",
            Self::Doorbell => "doorbell",
            Self::Occupancy => "occupancy",
            Self::Contact => "contact",
            Self::Temperature => "temperature",
            Self::Humidity => "humidity",
            Self::PowerMeter => "power_meter",
            Self::EvCharger => "ev_charger",
            Self::Battery => "battery",
        }
    }
}

impl fmt::Display for CapabilityKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

// ---------------------------------------------------------------------------
// v1
// ---------------------------------------------------------------------------

/// `on_off`: switches the endpoint on and off.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, TS)]
#[ts(optional_fields)]
pub struct OnOffCapability {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<OnOffState>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct OnOffState {
    pub on: bool,
}

/// `level`: brightness or similar, in percent. `level.set` on an endpoint
/// that is off also turns it on.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, TS)]
#[ts(optional_fields)]
pub struct LevelCapability {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<LevelState>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct LevelState {
    pub level: Percent,
}

/// `color_temperature`: white tone, in Kelvin (not mireds).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(optional_fields)]
pub struct ColorTemperatureCapability {
    pub features: ColorTemperatureFeatures,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<ColorTemperatureState>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ColorTemperatureFeatures {
    pub min_kelvin: u16,
    pub max_kelvin: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ColorTemperatureState {
    pub kelvin: u16,
}

/// `color`: hue (degrees) and saturation (percent).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, TS)]
#[ts(optional_fields)]
pub struct ColorCapability {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<ColorState>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ColorState {
    pub hue: Hue,
    pub saturation: Percent,
}

/// `cover`: blinds, shutters, curtains, awnings, garage doors and gates.
/// Position `100` is fully open, `0` fully closed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(optional_fields)]
pub struct CoverCapability {
    pub features: CoverFeatures,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<CoverState>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct CoverFeatures {
    pub kind: CoverKind,
    /// Can report and move to an arbitrary position.
    pub position: bool,
    /// Can stop while moving.
    pub stop: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum CoverKind {
    Blind,
    Shutter,
    Curtain,
    Awning,
    Garage,
    Gate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(optional_fields)]
pub struct CoverState {
    /// Only present when `features.position` is true.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub position: Option<Percent>,
    pub motion: CoverMotion,
    /// End-stop state, only present when a contact sensor exists.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub open: Option<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum CoverMotion {
    Opening,
    Closing,
    Stopped,
}

/// `fan`: discrete speeds `1..=speed_steps`, optionally reversible.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(optional_fields)]
pub struct FanCapability {
    pub features: FanFeatures,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<FanState>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct FanFeatures {
    pub speed_steps: u8,
    pub reversible: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct FanState {
    pub speed: u8,
    pub direction: FanDirection,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum FanDirection {
    Forward,
    Reverse,
}

// ---------------------------------------------------------------------------
// v2
// ---------------------------------------------------------------------------

/// `lock`: door locks. `unlock` and `unlatch` are sensitive.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(optional_fields)]
pub struct LockCapability {
    pub features: LockFeatures,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<LockState>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct LockFeatures {
    /// Can pull the latch (open the door), not just unlock.
    pub unlatch: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct LockState {
    pub lock_state: LockStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum LockStatus {
    Locked,
    Unlocked,
    Jammed,
    Unknown,
}

/// `trigger`: a momentary action, e.g. an electric door strike. `fire` is
/// sensitive. No features, no state.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
pub struct TriggerCapability {}

/// `camera`: streams start through the signaling API, not through commands.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(optional_fields)]
pub struct CameraCapability {
    pub features: CameraFeatures,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<CameraState>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct CameraFeatures {
    pub two_way_audio: bool,
    pub snapshot: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct CameraState {
    pub stream_available: bool,
}

/// `doorbell`: emits `doorbell.pressed`. No features, no state.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
pub struct DoorbellCapability {}

/// `occupancy`: presence/motion. Emits `occupancy.detected`.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, TS)]
#[ts(optional_fields)]
pub struct OccupancyCapability {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<OccupancyState>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct OccupancyState {
    pub occupied: bool,
}

/// `contact`: door/window contact sensor.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, TS)]
#[ts(optional_fields)]
pub struct ContactCapability {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<ContactState>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ContactState {
    pub open: bool,
}

/// `temperature`, in °C.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, TS)]
#[ts(optional_fields)]
pub struct TemperatureCapability {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<TemperatureState>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
pub struct TemperatureState {
    pub celsius: f64,
}

/// `humidity`: relative humidity in percent.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, TS)]
#[ts(optional_fields)]
pub struct HumidityCapability {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<HumidityState>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct HumidityState {
    pub percent: Percent,
}

/// `power_meter`: power in W, energy in Wh. The sign of `power_w` follows
/// the role's natural direction (`grid` positive = importing).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(optional_fields)]
pub struct PowerMeterCapability {
    pub features: PowerMeterFeatures,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<PowerMeterState>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct PowerMeterFeatures {
    /// Where the clamp sits. The one feature users may override.
    pub role: MeterRole,
    pub bidirectional: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum MeterRole {
    Grid,
    Solar,
    Load,
    Ev,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[ts(optional_fields)]
pub struct PowerMeterState {
    pub power_w: f64,
    pub energy_wh: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub energy_exported_wh: Option<f64>,
}

/// `ev_charger`. The charger's own limits (e.g. contracted power) always
/// prevail; VESTA never replaces them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(optional_fields)]
pub struct EvChargerCapability {
    pub features: EvChargerFeatures,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<EvChargerState>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
pub struct EvChargerFeatures {
    pub min_current_a: f64,
    pub max_current_a: f64,
    pub modes: Vec<EvChargeMode>,
}

impl EvChargerFeatures {
    /// Clamps a requested current to the charger's range. Never panics, even
    /// on inconsistent features or NaN.
    pub fn clamp_current(&self, amps: f64) -> f64 {
        amps.max(self.min_current_a).min(self.max_current_a)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum EvChargeMode {
    /// Charge at the configured current.
    Manual,
    /// Follow surplus solar production.
    Solar,
    /// Charge during scheduled hours (e.g. off-peak tariff).
    Scheduled,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
pub struct EvChargerState {
    pub status: EvChargerStatus,
    pub power_w: f64,
    pub session_energy_wh: f64,
    pub current_a: f64,
    pub mode: EvChargeMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum EvChargerStatus {
    Disconnected,
    Connected,
    Charging,
    Paused,
    Error,
}

/// `battery`: charge in percent and a low-battery flag.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, TS)]
#[ts(optional_fields)]
pub struct BatteryCapability {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<BatteryState>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct BatteryState {
    pub percent: Percent,
    pub low: bool,
}

// ---------------------------------------------------------------------------
// Capability map
// ---------------------------------------------------------------------------

/// The capabilities of one endpoint, keyed by capability name. Reported by
/// the driver; users can never add or remove capabilities.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, TS)]
#[serde(default)]
#[ts(optional_fields)]
pub struct Capabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub on_off: Option<OnOffCapability>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub level: Option<LevelCapability>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color_temperature: Option<ColorTemperatureCapability>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<ColorCapability>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cover: Option<CoverCapability>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fan: Option<FanCapability>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lock: Option<LockCapability>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trigger: Option<TriggerCapability>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub camera: Option<CameraCapability>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub doorbell: Option<DoorbellCapability>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub occupancy: Option<OccupancyCapability>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contact: Option<ContactCapability>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<TemperatureCapability>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub humidity: Option<HumidityCapability>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub power_meter: Option<PowerMeterCapability>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ev_charger: Option<EvChargerCapability>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub battery: Option<BatteryCapability>,
}

impl Capabilities {
    pub fn has(&self, kind: CapabilityKind) -> bool {
        match kind {
            CapabilityKind::OnOff => self.on_off.is_some(),
            CapabilityKind::Level => self.level.is_some(),
            CapabilityKind::ColorTemperature => self.color_temperature.is_some(),
            CapabilityKind::Color => self.color.is_some(),
            CapabilityKind::Cover => self.cover.is_some(),
            CapabilityKind::Fan => self.fan.is_some(),
            CapabilityKind::Lock => self.lock.is_some(),
            CapabilityKind::Trigger => self.trigger.is_some(),
            CapabilityKind::Camera => self.camera.is_some(),
            CapabilityKind::Doorbell => self.doorbell.is_some(),
            CapabilityKind::Occupancy => self.occupancy.is_some(),
            CapabilityKind::Contact => self.contact.is_some(),
            CapabilityKind::Temperature => self.temperature.is_some(),
            CapabilityKind::Humidity => self.humidity.is_some(),
            CapabilityKind::PowerMeter => self.power_meter.is_some(),
            CapabilityKind::EvCharger => self.ev_charger.is_some(),
            CapabilityKind::Battery => self.battery.is_some(),
        }
    }

    /// Capabilities present on this endpoint, in catalog order.
    pub fn kinds(&self) -> Vec<CapabilityKind> {
        CapabilityKind::ALL
            .into_iter()
            .filter(|kind| self.has(*kind))
            .collect()
    }

    /// Whether `command` can run on this endpoint given its reported
    /// features. The Hub answers `unsupported` without calling the driver
    /// when this is false.
    pub fn supports(&self, command: &Command) -> bool {
        match command {
            Command::OnOff(_) => self.on_off.is_some(),
            Command::Level(_) => self.level.is_some(),
            Command::ColorTemperature(_) => self.color_temperature.is_some(),
            Command::Color(_) => self.color.is_some(),
            Command::Cover(cmd) => self.cover.as_ref().is_some_and(|cap| match cmd {
                CoverCommand::Open | CoverCommand::Close => true,
                CoverCommand::Stop => cap.features.stop,
                CoverCommand::SetPosition { .. } => cap.features.position,
            }),
            Command::Fan(cmd) => self.fan.as_ref().is_some_and(|cap| match cmd {
                FanCommand::SetSpeed { speed } => (1..=cap.features.speed_steps).contains(speed),
                FanCommand::SetDirection { .. } => cap.features.reversible,
            }),
            Command::Lock(cmd) => self.lock.as_ref().is_some_and(|cap| match cmd {
                LockCommand::Lock | LockCommand::Unlock => true,
                LockCommand::Unlatch => cap.features.unlatch,
            }),
            Command::Trigger(_) => self.trigger.is_some(),
            Command::EvCharger(cmd) => self.ev_charger.as_ref().is_some_and(|cap| match cmd {
                EvChargerCommand::Pause
                | EvChargerCommand::Resume
                | EvChargerCommand::SetCurrent { .. } => true,
                EvChargerCommand::SetMode { mode } => cap.features.modes.contains(mode),
            }),
        }
    }

    /// Stores a state update reported by the driver. Returns `false` (and
    /// changes nothing) if the endpoint does not have that capability.
    pub fn apply_state(&mut self, state: CapabilityState) -> bool {
        macro_rules! set {
            ($field:ident, $value:expr) => {
                match self.$field.as_mut() {
                    Some(capability) => {
                        capability.state = Some($value);
                        true
                    }
                    None => false,
                }
            };
        }
        match state {
            CapabilityState::OnOff(s) => set!(on_off, s),
            CapabilityState::Level(s) => set!(level, s),
            CapabilityState::ColorTemperature(s) => set!(color_temperature, s),
            CapabilityState::Color(s) => set!(color, s),
            CapabilityState::Cover(s) => set!(cover, s),
            CapabilityState::Fan(s) => set!(fan, s),
            CapabilityState::Lock(s) => set!(lock, s),
            CapabilityState::Camera(s) => set!(camera, s),
            CapabilityState::Occupancy(s) => set!(occupancy, s),
            CapabilityState::Contact(s) => set!(contact, s),
            CapabilityState::Temperature(s) => set!(temperature, s),
            CapabilityState::Humidity(s) => set!(humidity, s),
            CapabilityState::PowerMeter(s) => set!(power_meter, s),
            CapabilityState::EvCharger(s) => set!(ev_charger, s),
            CapabilityState::Battery(s) => set!(battery, s),
        }
    }
}

/// The full state of one capability, as published by a driver and sent in
/// `state` messages: `{ "capability": "level", "state": { "level": 80 } }`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "capability", content = "state", rename_all = "snake_case")]
pub enum CapabilityState {
    OnOff(OnOffState),
    Level(LevelState),
    ColorTemperature(ColorTemperatureState),
    Color(ColorState),
    Cover(CoverState),
    Fan(FanState),
    Lock(LockState),
    Camera(CameraState),
    Occupancy(OccupancyState),
    Contact(ContactState),
    Temperature(TemperatureState),
    Humidity(HumidityState),
    PowerMeter(PowerMeterState),
    EvCharger(EvChargerState),
    Battery(BatteryState),
}

impl CapabilityState {
    pub const fn kind(&self) -> CapabilityKind {
        match self {
            Self::OnOff(_) => CapabilityKind::OnOff,
            Self::Level(_) => CapabilityKind::Level,
            Self::ColorTemperature(_) => CapabilityKind::ColorTemperature,
            Self::Color(_) => CapabilityKind::Color,
            Self::Cover(_) => CapabilityKind::Cover,
            Self::Fan(_) => CapabilityKind::Fan,
            Self::Lock(_) => CapabilityKind::Lock,
            Self::Camera(_) => CapabilityKind::Camera,
            Self::Occupancy(_) => CapabilityKind::Occupancy,
            Self::Contact(_) => CapabilityKind::Contact,
            Self::Temperature(_) => CapabilityKind::Temperature,
            Self::Humidity(_) => CapabilityKind::Humidity,
            Self::PowerMeter(_) => CapabilityKind::PowerMeter,
            Self::EvCharger(_) => CapabilityKind::EvCharger,
            Self::Battery(_) => CapabilityKind::Battery,
        }
    }
}

macro_rules! state_from {
    ($($state:ident => $variant:ident),* $(,)?) => {
        $(
            impl From<$state> for CapabilityState {
                fn from(value: $state) -> Self {
                    Self::$variant(value)
                }
            }
        )*
    };
}

state_from! {
    OnOffState => OnOff,
    LevelState => Level,
    ColorTemperatureState => ColorTemperature,
    ColorState => Color,
    CoverState => Cover,
    FanState => Fan,
    LockState => Lock,
    CameraState => Camera,
    OccupancyState => Occupancy,
    ContactState => Contact,
    TemperatureState => Temperature,
    HumidityState => Humidity,
    PowerMeterState => PowerMeter,
    EvChargerState => EvCharger,
    BatteryState => Battery,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::command::{LevelCommand, TriggerCommand};

    fn fan_light() -> Capabilities {
        Capabilities {
            on_off: Some(OnOffCapability::default()),
            fan: Some(FanCapability {
                features: FanFeatures {
                    speed_steps: 6,
                    reversible: false,
                },
                state: None,
            }),
            ..Capabilities::default()
        }
    }

    #[test]
    fn kind_names_match_serde() {
        for kind in CapabilityKind::ALL {
            assert_eq!(serde_json::to_value(kind).unwrap(), json!(kind.as_str()));
        }
    }

    #[test]
    fn capability_without_features_or_state_is_an_empty_object() {
        let caps = Capabilities {
            trigger: Some(TriggerCapability {}),
            doorbell: Some(DoorbellCapability {}),
            ..Capabilities::default()
        };
        assert_eq!(
            serde_json::to_value(&caps).unwrap(),
            json!({ "trigger": {}, "doorbell": {} })
        );
        assert_eq!(
            caps.kinds(),
            vec![CapabilityKind::Trigger, CapabilityKind::Doorbell]
        );
    }

    #[test]
    fn unknown_state_is_omitted() {
        let caps = fan_light();
        assert_eq!(
            serde_json::to_value(&caps).unwrap(),
            json!({
                "on_off": {},
                "fan": { "features": { "speed_steps": 6, "reversible": false } }
            })
        );
    }

    #[test]
    fn supports_respects_features() {
        let caps = fan_light();
        assert!(caps.supports(&Command::Fan(FanCommand::SetSpeed { speed: 6 })));
        assert!(!caps.supports(&Command::Fan(FanCommand::SetSpeed { speed: 0 })));
        assert!(!caps.supports(&Command::Fan(FanCommand::SetSpeed { speed: 7 })));
        assert!(!caps.supports(&Command::Fan(FanCommand::SetDirection {
            direction: FanDirection::Reverse
        })));
        assert!(!caps.supports(&Command::Level(LevelCommand::Set {
            level: Percent::MAX,
            transition_ms: None
        })));
        assert!(!caps.supports(&Command::Trigger(TriggerCommand::Fire)));

        let garage = Capabilities {
            cover: Some(CoverCapability {
                features: CoverFeatures {
                    kind: CoverKind::Garage,
                    position: false,
                    stop: false,
                },
                state: None,
            }),
            ..Capabilities::default()
        };
        assert!(garage.supports(&Command::Cover(CoverCommand::Open)));
        assert!(!garage.supports(&Command::Cover(CoverCommand::Stop)));
        assert!(!garage.supports(&Command::Cover(CoverCommand::SetPosition {
            position: Percent::MIN
        })));
    }

    #[test]
    fn apply_state_only_touches_present_capabilities() {
        let mut caps = fan_light();
        assert!(caps.apply_state(OnOffState { on: true }.into()));
        assert_eq!(
            caps.on_off.as_ref().and_then(|c| c.state),
            Some(OnOffState { on: true })
        );
        assert!(
            !caps.apply_state(
                LevelState {
                    level: Percent::MAX
                }
                .into()
            )
        );
        assert!(caps.level.is_none());
    }

    #[test]
    fn clamp_current_never_exceeds_max() {
        let features = EvChargerFeatures {
            min_current_a: 6.0,
            max_current_a: 16.0,
            modes: vec![EvChargeMode::Manual],
        };
        assert_eq!(features.clamp_current(32.0), 16.0);
        assert_eq!(features.clamp_current(2.0), 6.0);
        assert_eq!(features.clamp_current(10.5), 10.5);
        assert_eq!(features.clamp_current(f64::NAN), 6.0);
    }
}
