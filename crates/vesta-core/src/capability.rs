//! Capability catalog (`docs/architecture/capabilities.md`, "Capability Catalog").
//!
//! Each capability has static `features` (what the hardware can do, reported
//! by the driver) and dynamic `state`. `state` is omitted while the driver
//! does not know it yet (e.g. right after start-up); a driver never invents a
//! value.

use std::fmt;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::command::{
    Command, CoverCommand, EvChargerCommand, FanCommand, LockCommand, RobotVacuumCommand,
};
use crate::units::{Hue, Percent, TimeOfDay};

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
    RobotVacuum,
}

impl CapabilityKind {
    pub const ALL: [Self; 18] = [
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
        Self::RobotVacuum,
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
            Self::RobotVacuum => "robot_vacuum",
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
    /// Omitted when VESTA may switch it (the usual case).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub features: Option<OnOffFeatures>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<OnOffState>,
}

impl OnOffCapability {
    /// Switchable, with a known state.
    pub const fn with_state(on: bool) -> Self {
        Self {
            features: None,
            state: Some(OnOffState { on }),
        }
    }

    /// VESTA may only watch it: `on`, `off` and `toggle` are refused.
    pub fn read_only(&self) -> bool {
        self.features.is_some_and(|features| features.read_only)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
pub struct OnOffFeatures {
    /// The device is set up for monitoring only (e.g. `read_only` in its
    /// driver config): its state is shown, but it is never switched from
    /// VESTA, so the UI shows no switch.
    pub read_only: bool,
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

/// Where a meter measures. Decides the sign of `power_w` and where the
/// meter sits in an energy-flow view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum MeterRole {
    /// The connection to the grid. Positive = importing, negative =
    /// exporting.
    Grid,
    /// Solar production. Positive = producing.
    Solar,
    /// One appliance or circuit. Positive = consuming.
    Load,
    /// An EV charger. Positive = consuming.
    Ev,
    /// The whole home's consumption, including sub-metered loads such as an
    /// EV charger. Positive = consuming.
    House,
    /// A home battery. Positive = discharging (supplying the home),
    /// negative = charging.
    Battery,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[ts(optional_fields)]
pub struct PowerMeterState {
    pub power_w: f64,
    /// Lifetime energy in the role's natural direction. Omitted when the
    /// device only measures power.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub energy_wh: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub energy_exported_wh: Option<f64>,
    /// The most power this connection may draw, e.g. the contracted power
    /// on a `grid` meter. Informative only: VESTA never enforces it. The
    /// devices that do (an EV charger's own power control, the utility's
    /// meter) keep doing it (AGENTS.md rule 7).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit_w: Option<f64>,
    /// Measured voltage, in V, when the meter reports it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub voltage_v: Option<f64>,
    /// Measured current, in A, when the meter reports it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_a: Option<f64>,
}

impl PowerMeterState {
    /// Only power is known.
    pub const fn power(power_w: f64) -> Self {
        Self {
            power_w,
            energy_wh: None,
            energy_exported_wh: None,
            limit_w: None,
            voltage_v: None,
            current_a: None,
        }
    }
}

/// `ev_charger`: an EV charger (Matter: EnergyEVSE). The charger's own
/// limits (e.g. its contracted-power control) always prevail; VESTA never
/// replaces them (AGENTS.md rule 7).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(optional_fields)]
pub struct EvChargerCapability {
    pub features: EvChargerFeatures,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<EvChargerState>,
}

impl EvChargerCapability {
    /// The range `set_current` accepts right now: the limits configured on
    /// the charger when it reports them, kept within the hardware range.
    pub fn current_range(&self) -> (f64, f64) {
        let features = &self.features;
        let hw_min = features.min_current_a.min(features.max_current_a);
        let hw_max = features.max_current_a.max(features.min_current_a);
        let within = |value: Option<f64>, fallback: f64| {
            value
                .filter(|v| v.is_finite())
                .map_or(fallback, |v| v.max(hw_min).min(hw_max))
        };
        let state = self.state.as_ref();
        let min = within(state.and_then(|s| s.min_current_a), hw_min);
        let max = within(state.and_then(|s| s.max_current_a), hw_max);
        (min.min(max), max.max(min))
    }

    /// Clamps a requested current to [`Self::current_range`]. Never panics,
    /// even on inconsistent limits; NaN becomes the minimum.
    pub fn clamp_current(&self, amps: f64) -> f64 {
        let (min, max) = self.current_range();
        if amps.is_nan() {
            min
        } else {
            amps.max(min).min(max)
        }
    }
}

/// What the charger can do. Every command needs its feature: a charger set
/// up for monitoring only reports them all `false` and no modes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
pub struct EvChargerFeatures {
    /// Charging current range of the hardware, in A. The limits configured
    /// on the charger may be narrower (`state.min_current_a` /
    /// `state.max_current_a`).
    pub min_current_a: f64,
    pub max_current_a: f64,
    /// Modes `set_mode` can switch to. Empty: the mode cannot be changed
    /// from VESTA (it may still be reported in `state.mode`).
    pub modes: Vec<EvChargeMode>,
    /// `pause` and `resume` work.
    pub pause: bool,
    /// `set_current` works.
    pub adjustable_current: bool,
    /// `lock` and `unlock` work. A locked charger does not charge until it
    /// is unlocked; unlocking is sensitive.
    pub lock: bool,
    /// `set_schedule` works: the charger's own schedule can be switched on
    /// and off.
    pub schedule: bool,
    /// The charger keeps a schedule of charging hours (time windows), and
    /// what VESTA can do with it. Omitted when it has none, or when its
    /// hours can only be seen and changed on the charger itself.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub charging_schedule: Option<ChargingScheduleFeatures>,
}

/// What a charger's schedule of charging hours allows. The schedule lives in
/// the charger, which follows it by itself (AGENTS.md rule 7: it keeps
/// working when VESTA, the panel or the internet are down).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ChargingScheduleFeatures {
    /// Most time windows the charger holds (at least 1).
    pub max_windows: u8,
    /// Each window can be limited to some days of the week. Otherwise every
    /// window is every day.
    pub weekdays: bool,
    /// A window may run past midnight (it ends before it starts, e.g.
    /// 22:00–06:00).
    pub overnight: bool,
    /// `set_charging_schedule` works: the hours can be changed from VESTA.
    pub editable: bool,
    /// The charger reports its hours. When `false`, `state.charging_schedule`
    /// holds the hours last saved from VESTA (they may have been changed
    /// elsewhere since) and is omitted until then.
    pub reports_windows: bool,
    /// Saving the hours needs the home's internet connection. Switching the
    /// schedule on and off (`set_schedule`) and following it do not.
    pub saving_needs_internet: bool,
}

impl ChargingScheduleFeatures {
    /// Whether `windows` fit this charger: between 1 and `max_windows`
    /// windows, each with at least one day and no day twice, every day
    /// when the charger has no weekdays, never empty (start = end), and
    /// past midnight only when the charger allows it.
    pub fn check(&self, windows: &[ChargingWindow]) -> Result<(), ScheduleError> {
        if windows.is_empty() {
            return Err(ScheduleError::NoWindows);
        }
        if windows.len() > usize::from(self.max_windows) {
            return Err(ScheduleError::TooManyWindows(self.max_windows));
        }
        for window in windows {
            if window.days.is_empty() {
                return Err(ScheduleError::NoDays);
            }
            let mut seen = [false; 7];
            for day in &window.days {
                let index = day.index();
                if seen[index] {
                    return Err(ScheduleError::RepeatedDay);
                }
                seen[index] = true;
            }
            if !self.weekdays && window.days.len() != Weekday::ALL.len() {
                return Err(ScheduleError::DaysNotSupported);
            }
            if window.start == window.end {
                return Err(ScheduleError::EmptyWindow);
            }
            if !self.overnight && window.end < window.start {
                return Err(ScheduleError::OvernightNotSupported);
            }
        }
        Ok(())
    }
}

/// Why charging hours do not fit a charger (see
/// [`ChargingScheduleFeatures::check`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ScheduleError {
    #[error("a schedule needs at least one time window")]
    NoWindows,
    #[error("this charger holds at most {0} time windows")]
    TooManyWindows(u8),
    #[error("a time window needs at least one day")]
    NoDays,
    #[error("a day appears twice in a time window")]
    RepeatedDay,
    #[error("this charger's time windows are every day")]
    DaysNotSupported,
    #[error("a time window cannot start and end at the same time")]
    EmptyWindow,
    #[error("this charger's time windows cannot run past midnight")]
    OvernightNotSupported,
}

/// A day of the week, Monday first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum Weekday {
    Monday,
    Tuesday,
    Wednesday,
    Thursday,
    Friday,
    Saturday,
    Sunday,
}

impl Weekday {
    pub const ALL: [Self; 7] = [
        Self::Monday,
        Self::Tuesday,
        Self::Wednesday,
        Self::Thursday,
        Self::Friday,
        Self::Saturday,
        Self::Sunday,
    ];

    /// 0 for Monday … 6 for Sunday.
    pub const fn index(self) -> usize {
        self as usize
    }
}

/// One time window of a charging schedule: the charger may charge from
/// `start` to `end` on `days`. A window that ends before it starts runs
/// past midnight (22:00–06:00 ends the next morning); `days` are the days
/// it starts on. Times are the home's local time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ChargingWindow {
    pub start: TimeOfDay,
    pub end: TimeOfDay,
    pub days: Vec<Weekday>,
}

impl ChargingWindow {
    /// The same window with its days in week order (Monday first).
    #[must_use]
    pub fn normalized(mut self) -> Self {
        self.days.sort_unstable();
        self.days.dedup();
        self
    }
}

/// A charger's schedule of charging hours: while `enabled`, it charges
/// only within its `windows`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ChargingSchedule {
    /// Same as `state.schedule_enabled`.
    pub enabled: bool,
    pub windows: Vec<ChargingWindow>,
}

impl EvChargerFeatures {
    /// Clamps a requested current to the hardware range. Never panics, even
    /// on inconsistent features or NaN.
    pub fn clamp_current(&self, amps: f64) -> f64 {
        amps.max(self.min_current_a).min(self.max_current_a)
    }
}

/// How the charger decides how much to charge (Matter: EnergyEVSEMode).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum EvChargeMode {
    /// Charge at the set current.
    Manual,
    /// Charge only with surplus solar power; wait while there is none.
    Solar,
    /// Charge with surplus solar power, topped up from the grid so the car
    /// always charges at least at the minimum current.
    SolarGrid,
    /// Charge during scheduled hours (e.g. off-peak tariff), for chargers
    /// whose schedule is a mode. Chargers with a schedule that can be
    /// switched on and off in any mode report `features.schedule` instead.
    Scheduled,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(optional_fields)]
pub struct EvChargerState {
    pub status: EvChargerStatus,
    /// Why a plugged-in car is not charging. Only with status `waiting`,
    /// and only when the charger makes it clear.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub waiting_for: Option<EvChargerWaitReason>,
    /// Power going to the car, in W.
    pub power_w: f64,
    /// Energy delivered in the current (or last) charging session, in Wh.
    pub session_energy_wh: f64,
    /// Length of the current (or last) charging session, in seconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_duration_s: Option<u64>,
    /// The current the charger offers the car right now (its setpoint), in
    /// A. The charger's own power control may lower it by itself.
    pub current_a: f64,
    /// The charger's own power control is choosing the current by itself
    /// (e.g. to stay within the contracted power): `set_current` has no
    /// effect until it stops, so it is refused.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_current: Option<bool>,
    /// Measured current per phase, in A, L1 first.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phase_current_a: Option<Vec<f64>>,
    /// The range `set_current` accepts right now, as configured on the
    /// charger (within `features`). Omitted when the charger does not say.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_current_a: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_current_a: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<EvChargeMode>,
    /// Locked: nobody can charge until it is unlocked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub locked: Option<bool>,
    /// The charger's own schedule is on: it charges only during the hours
    /// set on it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schedule_enabled: Option<bool>,
    /// The charger's charging hours, when known (see
    /// `features.charging_schedule.reports_windows`). Omitted while they
    /// are not.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub charging_schedule: Option<ChargingSchedule>,
}

/// Charger status (Matter: EnergyEVSE `State`, plus `SupplyState` for
/// `paused`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum EvChargerStatus {
    /// No car plugged in.
    Disconnected,
    /// A car is plugged in and not charging, for no reason the charger
    /// reports (e.g. the car is full or not asking for power).
    Connected,
    Charging,
    /// Paused by someone (from VESTA, the charger's own app or button).
    Paused,
    /// A car is plugged in and the charger holds back by itself; see
    /// `waiting_for`.
    Waiting,
    Error,
}

/// Why a charger with a car plugged in is waiting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum EvChargerWaitReason {
    /// Outside the charger's schedule.
    Schedule,
    /// Not enough surplus solar power.
    Solar,
    /// The home is using a lot of power: the charger's own power control
    /// holds the car back so the contracted power is not exceeded.
    Power,
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

/// `robot_vacuum`: a robot vacuum cleaner (Matter: RVC Operational State and
/// RVC Run Mode). Its charge is a `battery` capability on the same
/// endpoint. Every command needs its feature: a robot set up for monitoring
/// only reports them all `false`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(optional_fields)]
pub struct RobotVacuumCapability {
    pub features: RobotVacuumFeatures,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<RobotVacuumState>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct RobotVacuumFeatures {
    /// `start` works (starts cleaning, or carries on after a pause).
    pub start: bool,
    /// `pause` works.
    pub pause: bool,
    /// `return_to_dock` works.
    pub return_to_dock: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(optional_fields)]
pub struct RobotVacuumState {
    pub status: RobotVacuumStatus,
    /// Area cleaned in the current (or last) cleaning, in m².
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cleaned_area_m2: Option<f64>,
    /// Length of the current (or last) cleaning, in seconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cleaning_duration_s: Option<u64>,
}

/// What the robot is doing (Matter: RVC Operational State, plus RVC Run
/// Mode for `cleaning` and `idle`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum RobotVacuumStatus {
    /// On its dock, fully charged (Matter: Docked).
    Docked,
    /// On its dock, charging (Matter: Charging).
    Charging,
    /// Cleaning, or on its way to clean a spot (Matter: Running).
    Cleaning,
    /// Paused by someone; `start` carries on (Matter: Paused).
    Paused,
    /// On its way back to the dock (Matter: SeekingCharger).
    Returning,
    /// Off the dock and not cleaning, e.g. standing by or asleep (Matter:
    /// Stopped).
    Idle,
    /// Stuck or needs attention, e.g. a full bin or a tangled brush (Matter:
    /// Error).
    Error,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub robot_vacuum: Option<RobotVacuumCapability>,
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
            CapabilityKind::RobotVacuum => self.robot_vacuum.is_some(),
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
            Command::OnOff(_) => self.on_off.as_ref().is_some_and(|cap| !cap.read_only()),
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
            Command::EvCharger(cmd) => self.ev_charger.as_ref().is_some_and(|cap| {
                let features = &cap.features;
                match cmd {
                    EvChargerCommand::Pause | EvChargerCommand::Resume => features.pause,
                    EvChargerCommand::SetCurrent { .. } => features.adjustable_current,
                    EvChargerCommand::SetMode { mode } => features.modes.contains(mode),
                    EvChargerCommand::Lock | EvChargerCommand::Unlock => features.lock,
                    EvChargerCommand::SetSchedule { .. } => features.schedule,
                    // Editable, and hours that fit this charger.
                    EvChargerCommand::SetChargingSchedule { windows, .. } => {
                        features.charging_schedule.is_some_and(|schedule| {
                            schedule.editable && schedule.check(windows).is_ok()
                        })
                    }
                }
            }),
            Command::RobotVacuum(cmd) => self.robot_vacuum.as_ref().is_some_and(|cap| {
                let features = cap.features;
                match cmd {
                    RobotVacuumCommand::Start => features.start,
                    RobotVacuumCommand::Pause => features.pause,
                    RobotVacuumCommand::ReturnToDock => features.return_to_dock,
                }
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
            CapabilityState::RobotVacuum(s) => set!(robot_vacuum, s),
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
    RobotVacuum(RobotVacuumState),
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
            Self::RobotVacuum(_) => CapabilityKind::RobotVacuum,
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
    RobotVacuumState => RobotVacuum,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::command::{LevelCommand, OnOffCommand, TriggerCommand};

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

    fn charger_features() -> EvChargerFeatures {
        EvChargerFeatures {
            min_current_a: 6.0,
            max_current_a: 16.0,
            modes: vec![EvChargeMode::Manual],
            pause: true,
            adjustable_current: true,
            lock: false,
            schedule: false,
            charging_schedule: None,
        }
    }

    fn charger_state(min: Option<f64>, max: Option<f64>) -> EvChargerState {
        EvChargerState {
            status: EvChargerStatus::Charging,
            waiting_for: None,
            power_w: 3680.0,
            session_energy_wh: 1200.0,
            session_duration_s: None,
            current_a: 16.0,
            auto_current: None,
            phase_current_a: None,
            min_current_a: min,
            max_current_a: max,
            mode: None,
            locked: None,
            schedule_enabled: None,
            charging_schedule: None,
        }
    }

    #[test]
    fn clamp_current_never_exceeds_max() {
        let features = charger_features();
        assert_eq!(features.clamp_current(32.0), 16.0);
        assert_eq!(features.clamp_current(2.0), 6.0);
        assert_eq!(features.clamp_current(10.5), 10.5);
        assert_eq!(features.clamp_current(f64::NAN), 6.0);
    }

    #[test]
    fn current_range_uses_the_chargers_own_limits_within_the_hardware() {
        let mut cap = EvChargerCapability {
            features: charger_features(),
            state: None,
        };
        assert_eq!(cap.current_range(), (6.0, 16.0));

        cap.state = Some(charger_state(Some(8.0), Some(13.0)));
        assert_eq!(cap.current_range(), (8.0, 13.0));
        assert_eq!(cap.clamp_current(20.0), 13.0);
        assert_eq!(cap.clamp_current(1.0), 8.0);
        assert_eq!(cap.clamp_current(f64::NAN), 8.0);
        assert_eq!(cap.clamp_current(f64::INFINITY), 13.0);

        // Limits outside the hardware range, swapped or not numbers never
        // widen it.
        cap.state = Some(charger_state(Some(2.0), Some(40.0)));
        assert_eq!(cap.current_range(), (6.0, 16.0));
        cap.state = Some(charger_state(Some(14.0), Some(10.0)));
        assert_eq!(cap.current_range(), (10.0, 14.0));
        cap.state = Some(charger_state(Some(f64::NAN), Some(f64::NEG_INFINITY)));
        assert_eq!(cap.current_range(), (6.0, 16.0));
    }

    #[test]
    fn charger_commands_need_their_feature() {
        let mut features = charger_features();
        let caps = |features: &EvChargerFeatures| Capabilities {
            ev_charger: Some(EvChargerCapability {
                features: features.clone(),
                state: None,
            }),
            ..Capabilities::default()
        };
        let ev = |cmd| Command::EvCharger(cmd);
        let all = [
            ev(EvChargerCommand::Pause),
            ev(EvChargerCommand::Resume),
            ev(EvChargerCommand::SetCurrent { amps: 10.0 }),
            ev(EvChargerCommand::SetMode {
                mode: EvChargeMode::Manual,
            }),
            ev(EvChargerCommand::Lock),
            ev(EvChargerCommand::Unlock),
            ev(EvChargerCommand::SetSchedule { enabled: true }),
        ];
        let supported: Vec<bool> = all
            .iter()
            .map(|cmd| caps(&features).supports(cmd))
            .collect();
        assert_eq!(supported, [true, true, true, true, false, false, false]);
        assert!(!caps(&features).supports(&ev(EvChargerCommand::SetMode {
            mode: EvChargeMode::Solar
        })));

        features.lock = true;
        features.schedule = true;
        assert!(all.iter().all(|cmd| caps(&features).supports(cmd)));

        // Monitoring only: nothing can be sent.
        let read_only = EvChargerFeatures {
            modes: Vec::new(),
            pause: false,
            adjustable_current: false,
            lock: false,
            schedule: false,
            ..charger_features()
        };
        assert!(all.iter().all(|cmd| !caps(&read_only).supports(cmd)));
    }

    fn window(start: &str, end: &str, days: &[Weekday]) -> ChargingWindow {
        ChargingWindow {
            start: start.parse().unwrap(),
            end: end.parse().unwrap(),
            days: days.to_vec(),
        }
    }

    fn schedule_features() -> ChargingScheduleFeatures {
        ChargingScheduleFeatures {
            max_windows: 2,
            weekdays: true,
            overnight: true,
            editable: true,
            reports_windows: false,
            saving_needs_internet: true,
        }
    }

    #[test]
    fn charging_hours_must_fit_the_charger() {
        use Weekday::*;
        let features = schedule_features();
        let night = window("22:00", "06:00", &Weekday::ALL);
        let weekend = window("10:00", "14:00", &[Saturday, Sunday]);
        assert_eq!(features.check(std::slice::from_ref(&night)), Ok(()));
        assert_eq!(features.check(&[night.clone(), weekend.clone()]), Ok(()));
        assert_eq!(features.check(&[]), Err(ScheduleError::NoWindows));
        assert_eq!(
            features.check(&[night.clone(), weekend.clone(), night.clone()]),
            Err(ScheduleError::TooManyWindows(2))
        );
        assert_eq!(
            features.check(&[window("22:00", "06:00", &[])]),
            Err(ScheduleError::NoDays)
        );
        assert_eq!(
            features.check(&[window("22:00", "06:00", &[Monday, Monday])]),
            Err(ScheduleError::RepeatedDay)
        );
        assert_eq!(
            features.check(&[window("06:00", "06:00", &[Monday])]),
            Err(ScheduleError::EmptyWindow)
        );

        let same_day_only = ChargingScheduleFeatures {
            overnight: false,
            weekdays: false,
            ..features
        };
        assert_eq!(
            same_day_only.check(&[window("01:00", "07:00", &Weekday::ALL)]),
            Ok(())
        );
        assert_eq!(
            same_day_only.check(&[night]),
            Err(ScheduleError::OvernightNotSupported)
        );
        assert_eq!(
            same_day_only.check(&[weekend]),
            Err(ScheduleError::DaysNotSupported)
        );
    }

    #[test]
    fn charging_hours_are_editable_only_when_the_charger_says_so() {
        use Weekday::*;
        let caps = |schedule: Option<ChargingScheduleFeatures>| Capabilities {
            ev_charger: Some(EvChargerCapability {
                features: EvChargerFeatures {
                    charging_schedule: schedule,
                    ..charger_features()
                },
                state: None,
            }),
            ..Capabilities::default()
        };
        let save = |windows: Vec<ChargingWindow>| {
            Command::EvCharger(EvChargerCommand::SetChargingSchedule {
                enabled: true,
                windows,
            })
        };
        let good = save(vec![window("23:00", "07:00", &[Monday, Friday])]);
        assert!(caps(Some(schedule_features())).supports(&good));
        assert!(!caps(None).supports(&good));
        let locked = ChargingScheduleFeatures {
            editable: false,
            ..schedule_features()
        };
        assert!(!caps(Some(locked)).supports(&good));
        // Hours that do not fit are refused before reaching the driver.
        assert!(!caps(Some(schedule_features())).supports(&save(Vec::new())));
        assert!(good.is_slow());
        assert!(!good.is_sensitive(crate::EndpointType::EvCharger));
        assert!(!Command::EvCharger(EvChargerCommand::SetSchedule { enabled: true }).is_slow());
    }

    #[test]
    fn charging_hours_on_the_wire() {
        let command: Command = serde_json::from_value(serde_json::json!({
            "capability": "ev_charger",
            "action": "set_charging_schedule",
            "enabled": true,
            "windows": [{ "start": "22:00", "end": "06:00", "days": ["monday", "sunday"] }]
        }))
        .unwrap();
        let Command::EvCharger(cmd) = &command else {
            panic!("{command:?}");
        };
        let schedule = cmd.charging_schedule().unwrap();
        assert!(schedule.enabled);
        assert_eq!(schedule.windows[0].start.minutes(), 22 * 60);
        assert_eq!(schedule.windows[0].days, [Weekday::Monday, Weekday::Sunday]);
        for bad in [
            serde_json::json!({ "start": "25:00", "end": "06:00", "days": ["monday"] }),
            serde_json::json!({ "start": "22:00", "end": "06:00", "days": ["funday"] }),
            serde_json::json!({ "start": 1320, "end": "06:00", "days": ["monday"] }),
        ] {
            assert!(
                serde_json::from_value::<ChargingWindow>(bad.clone()).is_err(),
                "{bad}"
            );
        }
        let normalized = window(
            "10:00",
            "12:00",
            &[Weekday::Sunday, Weekday::Monday, Weekday::Sunday],
        )
        .normalized();
        assert_eq!(normalized.days, [Weekday::Monday, Weekday::Sunday]);
    }
    #[test]
    fn read_only_switches_refuse_every_on_off_command() {
        let switchable = Capabilities {
            on_off: Some(OnOffCapability::with_state(true)),
            ..Capabilities::default()
        };
        let watched = Capabilities {
            on_off: Some(OnOffCapability {
                features: Some(OnOffFeatures { read_only: true }),
                state: Some(OnOffState { on: true }),
            }),
            ..Capabilities::default()
        };
        for action in [OnOffCommand::On, OnOffCommand::Off, OnOffCommand::Toggle] {
            assert!(switchable.supports(&Command::OnOff(action)));
            assert!(!watched.supports(&Command::OnOff(action)));
        }
        // Switchable is the default: no features on the wire.
        assert_eq!(
            serde_json::to_value(&switchable).unwrap(),
            json!({ "on_off": { "state": { "on": true } } })
        );
        assert_eq!(
            serde_json::to_value(&watched).unwrap(),
            json!({ "on_off": { "features": { "read_only": true }, "state": { "on": true } } })
        );
    }

    #[test]
    fn robot_vacuum_commands_need_their_feature() {
        let caps = |features: RobotVacuumFeatures| Capabilities {
            robot_vacuum: Some(RobotVacuumCapability {
                features,
                state: None,
            }),
            ..Capabilities::default()
        };
        let all = [
            RobotVacuumCommand::Start,
            RobotVacuumCommand::Pause,
            RobotVacuumCommand::ReturnToDock,
        ];
        let everything = RobotVacuumFeatures {
            start: true,
            pause: true,
            return_to_dock: true,
        };
        assert!(
            all.iter()
                .all(|cmd| caps(everything).supports(&Command::RobotVacuum(*cmd)))
        );
        let no_pause = RobotVacuumFeatures {
            pause: false,
            ..everything
        };
        assert!(!caps(no_pause).supports(&Command::RobotVacuum(RobotVacuumCommand::Pause)));
        assert!(caps(no_pause).supports(&Command::RobotVacuum(RobotVacuumCommand::Start)));
        let watched = RobotVacuumFeatures {
            start: false,
            pause: false,
            return_to_dock: false,
        };
        assert!(
            all.iter()
                .all(|cmd| !caps(watched).supports(&Command::RobotVacuum(*cmd)))
        );
        assert!(
            !Capabilities::default().supports(&Command::RobotVacuum(RobotVacuumCommand::Start))
        );

        let mut with_state = caps(everything);
        assert!(
            with_state.apply_state(
                RobotVacuumState {
                    status: RobotVacuumStatus::Charging,
                    cleaned_area_m2: Some(12.0),
                    cleaning_duration_s: None,
                }
                .into()
            )
        );
        assert_eq!(
            serde_json::to_value(&with_state).unwrap(),
            json!({ "robot_vacuum": {
                "features": { "start": true, "pause": true, "return_to_dock": true },
                "state": { "status": "charging", "cleaned_area_m2": 12.0 }
            } })
        );
    }
}
