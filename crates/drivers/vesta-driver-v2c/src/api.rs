//! The charger's local HTTP API: what `GET /RealTimeData` says, what it
//! means in VESTA's terms, and the only writes VESTA ever sends.
//!
//! Every fact here is documented, with its source, in this crate's
//! `README.md`.

use vesta_core::{
    DriverError, EvChargeMode, EvChargerState, EvChargerStatus, EvChargerWaitReason,
    PowerMeterState,
};

/// `GET /RealTimeData`: everything the charger reports, in one JSON object.
pub(crate) const REALTIME_PATH: &str = "/RealTimeData";
/// Largest answer we accept (the real one is about 800 bytes).
pub(crate) const MAX_RESPONSE_BYTES: usize = 64 * 1024;
/// The hardware's charging current range (V2C's API docs: 6–32 A).
pub(crate) const HW_MIN_A: f64 = 6.0;
pub(crate) const HW_MAX_A: f64 = 32.0;
/// Used to estimate the minimum charging power when the charger does not
/// report its installation voltage.
const NOMINAL_VOLTS: f64 = 230.0;

/// One `RealTimeData` answer. Every field is optional: firmware versions
/// differ, and a field we cannot read is unknown, never invented.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Reading {
    pub id: Option<String>,
    pub firmware: Option<String>,
    /// `ChargeState`: 0 no car (A), 1 car connected (B), 2 charging (C),
    /// 4–6 faults (F, E, D).
    pub charge_state: Option<i64>,
    pub paused: Option<bool>,
    pub locked: Option<bool>,
    /// `Timer`: the charger's schedules are on.
    pub timer: Option<bool>,
    /// `Dynamic`: dynamic power control is on.
    pub dynamic: Option<bool>,
    /// `PauseDynamic`: dynamic modulation is paused (1 = not modulating).
    pub pause_dynamic: Option<bool>,
    pub dynamic_power_mode: Option<i64>,
    /// `ChargeMode`: phases (0 single-phase, 1 three-phase, 2 mixed).
    pub phases_mode: Option<i64>,
    pub charge_power_w: Option<f64>,
    pub charge_energy_kwh: Option<f64>,
    pub charge_time_s: Option<f64>,
    pub intensity_a: Option<f64>,
    pub min_intensity_a: Option<f64>,
    pub max_intensity_a: Option<f64>,
    pub phase_current_a: [Option<f64>; 3],
    pub voltage_v: Option<f64>,
    pub grid_w: Option<f64>,
    pub house_w: Option<f64>,
    pub solar_w: Option<f64>,
    pub battery_w: Option<f64>,
    pub contracted_w: Option<f64>,
    /// `SlaveError`: 0 = the power clamp/meter answers.
    pub meter_error: Option<i64>,
}

/// A JSON value as a number: ints, floats and numeric strings.
fn number(value: &serde_json::Value) -> Option<f64> {
    match value {
        serde_json::Value::Number(n) => n.as_f64(),
        serde_json::Value::String(s) => s.trim().parse::<f64>().ok(),
        _ => None,
    }
    .filter(|n| n.is_finite())
}

fn integer(value: &serde_json::Value) -> Option<i64> {
    number(value).filter(|n| n.fract() == 0.0).map(|n| n as i64)
}

fn flag(value: &serde_json::Value) -> Option<bool> {
    match value {
        serde_json::Value::Bool(b) => Some(*b),
        other => match integer(other)? {
            0 => Some(false),
            1 => Some(true),
            _ => None,
        },
    }
}

fn text(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::String(s) if !s.trim().is_empty() => Some(s.trim().to_owned()),
        serde_json::Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

impl Reading {
    /// Parses a `RealTimeData` body. Tolerant: unknown fields are ignored,
    /// missing or unreadable ones stay unknown, and a repeated key (some
    /// firmware sends `FirmwareVersion` twice) keeps the last value.
    pub(crate) fn parse(body: &[u8]) -> Result<Self, DriverError> {
        let value: serde_json::Value = serde_json::from_slice(body)
            .map_err(|e| DriverError::Protocol(format!("RealTimeData is not JSON: {e}")))?;
        let serde_json::Value::Object(map) = value else {
            return Err(DriverError::Protocol(
                "RealTimeData is not a JSON object".to_owned(),
            ));
        };
        let get = |key: &str| map.get(key);
        let num = |key: &str| get(key).and_then(number);
        let int = |key: &str| get(key).and_then(integer);
        let flag_of = |key: &str| get(key).and_then(flag);
        let reading = Self {
            id: get("ID").and_then(text),
            firmware: get("FirmwareVersion").and_then(text),
            charge_state: int("ChargeState"),
            paused: flag_of("Paused"),
            locked: flag_of("Locked"),
            timer: flag_of("Timer"),
            dynamic: flag_of("Dynamic"),
            pause_dynamic: flag_of("PauseDynamic"),
            dynamic_power_mode: int("DynamicPowerMode"),
            phases_mode: int("ChargeMode"),
            charge_power_w: num("ChargePower"),
            charge_energy_kwh: num("ChargeEnergy"),
            charge_time_s: num("ChargeTime"),
            intensity_a: num("Intensity"),
            min_intensity_a: num("MinIntensity"),
            max_intensity_a: num("MaxIntensity"),
            phase_current_a: [
                num("IntensityMeasure_L1"),
                num("IntensityMeasure_L2"),
                num("IntensityMeasure_L3"),
            ],
            voltage_v: num("VoltageInstallation"),
            grid_w: num("GridPower"),
            house_w: num("HousePower"),
            solar_w: num("FVPower"),
            battery_w: num("BatteryPower"),
            contracted_w: num("ContractedPower"),
            meter_error: int("SlaveError"),
        };
        if reading.charge_state.is_none() && reading.id.is_none() {
            return Err(DriverError::Protocol(
                "RealTimeData has neither ChargeState nor ID".to_owned(),
            ));
        }
        Ok(reading)
    }

    /// The charger's own power control is choosing the current: `Intensity`
    /// writes are ignored while `Dynamic=1` and `PauseDynamic=0`.
    pub(crate) fn auto_current(&self) -> Option<bool> {
        Some(self.dynamic? && !self.pause_dynamic?)
    }

    pub(crate) fn mode(&self) -> Option<EvChargeMode> {
        mode_of(self.dynamic_power_mode?)
    }

    /// The range `Intensity` accepts now: `MinIntensity..=MaxIntensity`,
    /// within the hardware's 6–32 A.
    pub(crate) fn current_range(&self) -> (f64, f64) {
        let within = |v: Option<f64>, fallback: f64| {
            v.filter(|v| *v > 0.0)
                .map_or(fallback, |v| v.clamp(HW_MIN_A, HW_MAX_A))
        };
        let min = within(self.min_intensity_a, HW_MIN_A);
        let max = within(self.max_intensity_a, HW_MAX_A);
        (min.min(max), max.max(min))
    }

    /// The meter readings can be trusted (`SlaveError` is 0 or absent).
    pub(crate) fn meters_ok(&self) -> bool {
        self.meter_error.is_none_or(|code| code == 0)
    }

    /// Import from the grid: `GridPower` when the firmware reports it, else
    /// house minus solar (only without a battery, whose sign is not
    /// documented).
    pub(crate) fn grid_import_w(&self) -> Option<f64> {
        self.grid_w.or_else(|| {
            let battery = self.battery_w.unwrap_or(0.0);
            (battery == 0.0).then_some(self.house_w? - self.solar_w.unwrap_or(0.0))
        })
    }

    /// The contracted power, when the charger knows it (it reports -1 or 0
    /// when it does not use one).
    pub(crate) fn contracted_w(&self) -> Option<f64> {
        self.contracted_w.filter(|w| *w > 0.0)
    }

    /// Too little room under the contracted power for even the minimum
    /// current: the charger's own power control is holding the car back.
    fn no_room_for_the_minimum(&self) -> bool {
        if self.dynamic != Some(true) || self.pause_dynamic == Some(true) {
            return false;
        }
        let (Some(contracted), Some(grid)) = (self.contracted_w(), self.grid_import_w()) else {
            return false;
        };
        let volts = self.voltage_v.filter(|v| *v > 0.0).unwrap_or(NOMINAL_VOLTS);
        let minimum_w = self.current_range().0 * volts;
        contracted - grid < minimum_w
    }

    fn status(&self) -> Option<(EvChargerStatus, Option<EvChargerWaitReason>)> {
        let paused = self.paused == Some(true) || self.dynamic_power_mode == Some(STOP_MODE);
        let status = match self.charge_state? {
            0 => (EvChargerStatus::Disconnected, None),
            // Firmware 2.5.0 keeps ChargeState at 2 after `Paused=1`.
            1 | 2 if paused => (EvChargerStatus::Paused, None),
            // Locked: nobody can charge until it is unlocked.
            1 | 2 if self.locked == Some(true) => (EvChargerStatus::Connected, None),
            2 => (EvChargerStatus::Charging, None),
            1 if self.mode() == Some(EvChargeMode::Solar) => {
                (EvChargerStatus::Waiting, Some(EvChargerWaitReason::Solar))
            }
            1 if self.no_room_for_the_minimum() => {
                (EvChargerStatus::Waiting, Some(EvChargerWaitReason::Power))
            }
            1 if self.timer == Some(true) => (
                EvChargerStatus::Waiting,
                Some(EvChargerWaitReason::Schedule),
            ),
            1 => (EvChargerStatus::Connected, None),
            // 3 is "ventilation required" in the cloud API; 4–6 are faults
            // locally; anything else is unknown, so not safe to call fine.
            _ => (EvChargerStatus::Error, None),
        };
        Some(status)
    }

    /// The `ev_charger` state, or `None` when the charger did not report
    /// enough to describe it (nothing is invented).
    pub(crate) fn charger_state(&self) -> Option<EvChargerState> {
        let (status, waiting_for) = self.status()?;
        let phases = match self.phases_mode {
            Some(0) => 1,
            _ => 3,
        };
        let measured: Vec<f64> = self
            .phase_current_a
            .iter()
            .take(phases)
            .map_while(|amps| amps.map(|a| round_to(a.max(0.0), 100.0)))
            .collect();
        let (min, max) = self.current_range();
        Some(EvChargerState {
            status,
            waiting_for,
            power_w: self.charge_power_w?.max(0.0).round(),
            session_energy_wh: (self.charge_energy_kwh?.max(0.0) * 1_000.0).round(),
            session_duration_s: self.charge_time_s.map(|s| s.max(0.0).round() as u64),
            current_a: round_to(self.intensity_a?.max(0.0), 10.0),
            auto_current: self.auto_current(),
            phase_current_a: (!measured.is_empty()).then_some(measured),
            min_current_a: self.min_intensity_a.map(|_| min),
            max_current_a: self.max_intensity_a.map(|_| max),
            mode: self.mode(),
            locked: self.locked,
            schedule_enabled: self.timer,
        })
    }

    pub(crate) fn grid_state(&self) -> Option<PowerMeterState> {
        Some(meter(self.grid_import_w()?, self.contracted_w()))
    }

    pub(crate) fn house_state(&self) -> Option<PowerMeterState> {
        Some(meter(self.house_w?, None))
    }

    pub(crate) fn solar_state(&self) -> Option<PowerMeterState> {
        Some(meter(self.solar_w?.max(0.0), None))
    }

    pub(crate) fn battery_state(&self) -> Option<PowerMeterState> {
        Some(meter(self.battery_w?, None))
    }
}

fn round_to(value: f64, per_unit: f64) -> f64 {
    (value * per_unit).round() / per_unit
}

fn meter(power_w: f64, limit_w: Option<f64>) -> PowerMeterState {
    PowerMeterState {
        limit_w,
        ..PowerMeterState::power(power_w.round())
    }
}

/// `DynamicPowerMode` values (V2C's HTTP API sheet, reviewed 2026-07).
const PROFILES_MODE: i64 = 0;
const SOLAR_GRID_MODE: i64 = 2;
const SOLAR_ONLY_MODE: i64 = 3;
const GRID_SOLAR_MODE: i64 = 4;
const STOP_MODE: i64 = 5;

/// Our charging mode for a `DynamicPowerMode`.
pub(crate) const fn mode_of(dynamic_power_mode: i64) -> Option<EvChargeMode> {
    match dynamic_power_mode {
        // The time-slot power profiles set in the charger's app.
        PROFILES_MODE => Some(EvChargeMode::Scheduled),
        SOLAR_GRID_MODE => Some(EvChargeMode::SolarGrid),
        SOLAR_ONLY_MODE => Some(EvChargeMode::Solar),
        // "Grid + solar, as fast as the contracted power allows".
        GRID_SOLAR_MODE => Some(EvChargeMode::Manual),
        // 1 is deprecated (ignored by the charger), 5 is "stop", and any
        // other value is undocumented.
        _ => None,
    }
}

/// Modes VESTA may switch between: the three solar modes only. Leaving or
/// entering the power profiles (mode 0) or "stop" (5) is left to the
/// charger's own app: the profiles may hold time-of-day contracted-power
/// limits, and VESTA never touches those (AGENTS.md rule 7).
pub(crate) const SWITCHABLE_MODES: [EvChargeMode; 3] = [
    EvChargeMode::Manual,
    EvChargeMode::Solar,
    EvChargeMode::SolarGrid,
];

/// A solar mode VESTA may write.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SolarMode {
    SolarGrid,
    SolarOnly,
    GridSolar,
}

impl SolarMode {
    pub(crate) const fn of(mode: EvChargeMode) -> Option<Self> {
        match mode {
            EvChargeMode::Manual => Some(Self::GridSolar),
            EvChargeMode::Solar => Some(Self::SolarOnly),
            EvChargeMode::SolarGrid => Some(Self::SolarGrid),
            EvChargeMode::Scheduled => None,
        }
    }

    const fn value(self) -> i64 {
        match self {
            Self::SolarGrid => SOLAR_GRID_MODE,
            Self::SolarOnly => SOLAR_ONLY_MODE,
            Self::GridSolar => GRID_SOLAR_MODE,
        }
    }

    /// Whether the charger is in one of these modes now (so switching does
    /// not leave its power profiles).
    pub(crate) fn is_active(dynamic_power_mode: Option<i64>) -> bool {
        matches!(
            dynamic_power_mode,
            Some(SOLAR_GRID_MODE | SOLAR_ONLY_MODE | GRID_SOLAR_MODE)
        )
    }
}

/// Every write VESTA can send: `GET /write/<Key>=<Value>`. This enum is the
/// whole list on purpose. There is no way to write `ContractedPower`,
/// `Dynamic`, `PauseDynamic`, `MinIntensity`, `MaxIntensity`,
/// `ChargeMode` (phases) or `VoltageInstallation`: those are the charger's
/// safety and installation settings (AGENTS.md rule 7).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Write {
    /// `Paused`: 1 pauses the session, 0 resumes it.
    Paused(bool),
    /// `Locked`: 1 blocks the charger, 0 unblocks it.
    Locked(bool),
    /// `Timer`: 1 turns the charger's schedules on, 0 off.
    Timer(bool),
    /// `Intensity`: the current offered to the car, in whole amps, already
    /// clamped to `MinIntensity..=MaxIntensity`.
    Intensity(u8),
    /// `DynamicPowerMode`, solar modes only.
    DynamicPowerMode(SolarMode),
}

impl Write {
    pub(crate) fn path(self) -> String {
        let bit = |on: bool| u8::from(on);
        match self {
            Self::Paused(on) => format!("/write/Paused={}", bit(on)),
            Self::Locked(on) => format!("/write/Locked={}", bit(on)),
            Self::Timer(on) => format!("/write/Timer={}", bit(on)),
            Self::Intensity(amps) => format!("/write/Intensity={amps}"),
            Self::DynamicPowerMode(mode) => format!("/write/DynamicPowerMode={}", mode.value()),
        }
    }
}

/// A requested current as the whole amps `Intensity` takes, within the
/// range the charger accepts now. NaN becomes the minimum.
pub(crate) fn intensity_for(amps: f64, (min, max): (f64, f64)) -> u8 {
    let amps = if amps.is_nan() { min } else { amps };
    let rounded = amps.round().max(min.ceil()).min(max.floor());
    // Within 6..=32 by construction.
    rounded.clamp(HW_MIN_A, HW_MAX_A) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The sanitized sample from a charging Trydan (firmware 2.5.2).
    pub(crate) const CHARGING: &str = include_str!("../fixtures/realtime-charging.json");

    fn with(changes: &[(&str, serde_json::Value)]) -> Reading {
        let mut value: serde_json::Value = serde_json::from_str(CHARGING).unwrap();
        for (key, v) in changes {
            if v.is_null() {
                value.as_object_mut().unwrap().remove(*key);
            } else {
                value[*key] = v.clone();
            }
        }
        Reading::parse(value.to_string().as_bytes()).unwrap()
    }

    #[test]
    fn parses_the_real_sample() {
        let reading = Reading::parse(CHARGING.as_bytes()).unwrap();
        assert_eq!(reading.id.as_deref(), Some("ABC123"));
        assert_eq!(reading.firmware.as_deref(), Some("2.5.2"));
        assert_eq!(reading.charge_state, Some(2));
        assert_eq!(reading.paused, Some(false));
        assert_eq!(reading.timer, Some(true));
        assert_eq!(reading.dynamic, Some(true));
        assert_eq!(reading.pause_dynamic, Some(false));
        assert_eq!(reading.intensity_a, Some(23.0));
        assert_eq!(reading.phase_current_a, [Some(22.94), Some(0.0), Some(0.0)]);
        assert_eq!(reading.contracted_w, Some(5700.0));
        assert_eq!(reading.meter_error, Some(0));

        let state = reading.charger_state().unwrap();
        assert_eq!(state.status, EvChargerStatus::Charging);
        assert_eq!(state.waiting_for, None);
        assert_eq!(state.power_w, 5092.0);
        assert_eq!(state.session_energy_wh, 4890.0);
        assert_eq!(state.session_duration_s, Some(3841));
        assert_eq!(state.current_a, 23.0);
        assert_eq!(state.auto_current, Some(true));
        assert_eq!(state.phase_current_a, Some(vec![22.94, 0.0, 0.0]));
        assert_eq!(state.min_current_a, Some(6.0));
        assert_eq!(state.max_current_a, Some(30.0));
        assert_eq!(state.mode, Some(EvChargeMode::Scheduled));
        assert_eq!(state.locked, Some(false));
        assert_eq!(state.schedule_enabled, Some(true));

        let grid = reading.grid_state().unwrap();
        assert_eq!(grid.power_w, 5739.0);
        assert_eq!(grid.limit_w, Some(5700.0));
        assert_eq!(grid.energy_wh, None, "never invented");
        assert_eq!(reading.house_state().unwrap().power_w, 5739.0);
        assert_eq!(reading.solar_state().unwrap().power_w, 0.0);
        assert_eq!(reading.battery_state().unwrap().power_w, 0.0);
        assert!(reading.meters_ok());
    }

    #[test]
    fn tolerates_ints_strings_unknown_and_repeated_fields() {
        let body = br#"{"ID":"ABC123","FirmwareVersion":"2.4.4","ChargeState":"1",
            "ChargePower":0,"ChargeEnergy":"0.00","ChargeTime":0,"Intensity":0,
            "Paused":0,"Locked":0,"Timer":0,"Dynamic":0,"PauseDynamic":0,
            "DynamicPowerMode":4,"MinIntensity":6,"MaxIntensity":16,"HousePower":"812.5",
            "FVPower":0,"BatteryPower":0,"ContractedPower":-1,"SlaveError":0,
            "SomethingNew":{"nested":true},"FirmwareVersion":"2.4.4"}"#;
        let reading = Reading::parse(body).unwrap();
        assert_eq!(reading.charge_state, Some(1));
        assert_eq!(reading.house_w, Some(812.5));
        assert_eq!(reading.grid_w, None);
        // Older firmware: no per-phase currents, no GridPower.
        let state = reading.charger_state().unwrap();
        assert_eq!(state.status, EvChargerStatus::Connected);
        assert_eq!(state.phase_current_a, None);
        assert_eq!(state.current_a, 0.0);
        assert_eq!(state.auto_current, Some(false));
        assert_eq!(state.mode, Some(EvChargeMode::Manual));
        assert_eq!(state.max_current_a, Some(16.0));
        // Grid derived from house - solar (no battery); no contracted power.
        let grid = reading.grid_state().unwrap();
        assert_eq!(grid.power_w, 813.0);
        assert_eq!(grid.limit_w, None);
    }

    #[test]
    fn rejects_what_is_not_a_reading() {
        for bad in [
            &b""[..],
            b"OK",
            b"[1,2]",
            b"{}",
            b"{\"Foo\":1}",
            b"\xff\xfe",
        ] {
            assert!(
                matches!(Reading::parse(bad), Err(DriverError::Protocol(_))),
                "{bad:?}"
            );
        }
    }

    #[test]
    fn missing_essentials_leave_the_charger_unknown() {
        for key in ["ChargeState", "ChargePower", "ChargeEnergy", "Intensity"] {
            let reading = with(&[(key, serde_json::Value::Null)]);
            assert_eq!(reading.charger_state(), None, "{key}");
        }
        let reading = with(&[("HousePower", serde_json::Value::Null)]);
        assert_eq!(reading.house_state(), None);
    }

    #[test]
    fn status_in_every_situation() {
        use serde_json::json;
        let status = |changes: &[(&str, serde_json::Value)]| {
            let state = with(changes).charger_state().unwrap();
            (state.status, state.waiting_for)
        };
        assert_eq!(
            status(&[("ChargeState", json!(0)), ("Paused", json!(1))]),
            (EvChargerStatus::Disconnected, None)
        );
        // Firmware 2.5.0 keeps ChargeState 2 while paused.
        assert_eq!(
            status(&[("Paused", json!(1)), ("ChargePower", json!(0))]),
            (EvChargerStatus::Paused, None)
        );
        assert_eq!(
            status(&[("ChargeState", json!(1)), ("DynamicPowerMode", json!(5))]),
            (EvChargerStatus::Paused, None)
        );
        assert_eq!(
            status(&[("ChargeState", json!(1)), ("Locked", json!(1))]),
            (EvChargerStatus::Connected, None)
        );
        assert_eq!(
            status(&[("ChargeState", json!(1)), ("DynamicPowerMode", json!(3))]),
            (EvChargerStatus::Waiting, Some(EvChargerWaitReason::Solar))
        );
        // 5700 W contracted, 5500 W from the grid: no room for 6 A.
        assert_eq!(
            status(&[("ChargeState", json!(1)), ("GridPower", json!(5500.0))]),
            (EvChargerStatus::Waiting, Some(EvChargerWaitReason::Power))
        );
        // Plenty of room, schedule on: waiting for its hours.
        assert_eq!(
            status(&[("ChargeState", json!(1)), ("GridPower", json!(600.0))]),
            (
                EvChargerStatus::Waiting,
                Some(EvChargerWaitReason::Schedule)
            )
        );
        assert_eq!(
            status(&[
                ("ChargeState", json!(1)),
                ("GridPower", json!(600.0)),
                ("Timer", json!(0))
            ]),
            (EvChargerStatus::Connected, None)
        );
        // Without dynamic control the charger is not holding anything back.
        assert_eq!(
            status(&[
                ("ChargeState", json!(1)),
                ("GridPower", json!(5500.0)),
                ("Dynamic", json!(0)),
                ("Timer", json!(0))
            ]),
            (EvChargerStatus::Connected, None)
        );
        for fault in [3, 4, 5, 6, 7, 99, -1] {
            assert_eq!(
                status(&[("ChargeState", json!(fault))]),
                (EvChargerStatus::Error, None),
                "{fault}"
            );
        }
    }

    #[test]
    fn phases_follow_the_installation() {
        use serde_json::json;
        let single = with(&[("ChargeMode", json!(0))]).charger_state().unwrap();
        assert_eq!(single.phase_current_a, Some(vec![22.94]));
        let three = with(&[
            ("IntensityMeasure_L2", json!(15.96)),
            ("IntensityMeasure_L3", json!(16.012)),
        ])
        .charger_state()
        .unwrap();
        assert_eq!(three.phase_current_a, Some(vec![22.94, 15.96, 16.01]));
    }

    #[test]
    fn meter_errors_and_unknown_modes() {
        use serde_json::json;
        assert!(!with(&[("SlaveError", json!(5))]).meters_ok());
        assert!(with(&[("SlaveError", serde_json::Value::Null)]).meters_ok());
        for (raw, mode) in [
            (0, Some(EvChargeMode::Scheduled)),
            (1, None),
            (2, Some(EvChargeMode::SolarGrid)),
            (3, Some(EvChargeMode::Solar)),
            (4, Some(EvChargeMode::Manual)),
            (5, None),
            (6, None),
            (7, None),
        ] {
            assert_eq!(mode_of(raw), mode, "{raw}");
        }
        let without_dynamic = with(&[("PauseDynamic", serde_json::Value::Null)]);
        assert_eq!(without_dynamic.auto_current(), None);
        assert_eq!(
            with(&[("PauseDynamic", json!(1))]).auto_current(),
            Some(false)
        );
    }

    #[test]
    fn writes_have_the_documented_shape() {
        assert_eq!(Write::Paused(true).path(), "/write/Paused=1");
        assert_eq!(Write::Paused(false).path(), "/write/Paused=0");
        assert_eq!(Write::Locked(true).path(), "/write/Locked=1");
        assert_eq!(Write::Locked(false).path(), "/write/Locked=0");
        assert_eq!(Write::Timer(true).path(), "/write/Timer=1");
        assert_eq!(Write::Intensity(16).path(), "/write/Intensity=16");
        assert_eq!(
            Write::DynamicPowerMode(SolarMode::SolarGrid).path(),
            "/write/DynamicPowerMode=2"
        );
        assert_eq!(
            Write::DynamicPowerMode(SolarMode::SolarOnly).path(),
            "/write/DynamicPowerMode=3"
        );
        assert_eq!(
            Write::DynamicPowerMode(SolarMode::GridSolar).path(),
            "/write/DynamicPowerMode=4"
        );
    }

    #[test]
    fn intensity_is_clamped_and_whole() {
        let range = (6.0, 30.0);
        assert_eq!(intensity_for(16.0, range), 16);
        assert_eq!(intensity_for(9.6, range), 10);
        assert_eq!(intensity_for(40.0, range), 30);
        assert_eq!(intensity_for(2.0, range), 6);
        assert_eq!(intensity_for(-5.0, range), 6);
        assert_eq!(intensity_for(f64::NAN, range), 6);
        assert_eq!(intensity_for(f64::INFINITY, range), 30);
        // Odd limits from the charger never widen the hardware range.
        assert_eq!(intensity_for(40.0, (6.0, 32.0)), 32);
        assert_eq!(intensity_for(12.4, (12.5, 20.0)), 13);
    }

    #[test]
    fn current_range_comes_from_the_charger() {
        use serde_json::json;
        assert_eq!(with(&[]).current_range(), (6.0, 30.0));
        assert_eq!(
            with(&[("MinIntensity", json!(0)), ("MaxIntensity", json!(80))]).current_range(),
            (6.0, 32.0)
        );
        assert_eq!(
            with(&[("MinIntensity", json!(20)), ("MaxIntensity", json!(10))]).current_range(),
            (10.0, 20.0)
        );
    }
}
