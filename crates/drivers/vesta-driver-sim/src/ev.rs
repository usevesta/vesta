//! Simulated EV charger (`model: "ev_charger"`): a wall charger with a car
//! plugged in, plus the home's grid and house meters, solar panels and
//! (optionally) a home battery.
//!
//! Time runs on a compressed day ([`DAY_SECS`] of real time per day) that
//! starts mid-morning, so the sun is up when the demo opens. Every tick:
//! - solar follows a bell curve between sunrise and sunset, with passing
//!   clouds;
//! - the rest of the house uses a base load plus appliances that switch on
//!   and off (a washing machine shortly after start, the oven at lunch and
//!   dinner time);
//! - the charger's own power control keeps the grid import under the
//!   contracted power, like a real charger with dynamic power control: it
//!   lowers the current, and holds the car back ("waiting for power") when
//!   not even the minimum current fits. VESTA never does this itself
//!   (AGENTS.md rule 7); the simulator plays the charger;
//! - `solar` charges only with surplus, `solar_grid` tops the surplus up to
//!   the minimum current, `manual` charges at the set current;
//! - the car's onboard charger ramps up a few amps per tick;
//! - with the schedule on, it charges only during the night (off-peak).

use std::f64::consts::PI;
use std::time::Duration;

use vesta_core::{
    Capabilities, DriverError, Endpoint, EndpointId, EndpointType, EvChargeMode,
    EvChargerCapability, EvChargerCommand, EvChargerFeatures, EvChargerState, EvChargerStatus,
    EvChargerWaitReason, MeterRole, PowerMeterCapability, PowerMeterFeatures, PowerMeterState,
};

/// How often the charger and the meters report.
pub(crate) const EV_TICK: Duration = Duration::from_secs(2);
/// One simulated day, in seconds of real time (12 minutes).
pub(crate) const DAY_SECS: f64 = 720.0;
/// The simulated day starts at 10:30.
const START_HOUR: f64 = 10.5;
const SUNRISE_HOUR: f64 = 7.5;
const SUNSET_HOUR: f64 = 20.5;
/// The schedule's charging hours: 23:00 to 07:00 (off-peak).
const SCHEDULE_FROM_HOUR: f64 = 23.0;
const SCHEDULE_TO_HOUR: f64 = 7.0;

const SOLAR_PEAK_W: f64 = 4_200.0;
pub(crate) const CONTRACTED_W: f64 = 5_750.0;
const VOLTS: f64 = 230.0;
/// What the hardware can do.
pub(crate) const HW_MIN_A: f64 = 6.0;
pub(crate) const HW_MAX_A: f64 = 32.0;
/// What the installer configured on the charger (the circuit's limit).
pub(crate) const LIMIT_MAX_A: f64 = 25.0;
/// The car's onboard charger ramps up this much per tick.
const RAMP_A: f64 = 4.0;
const BASE_LOAD_W: f64 = 380.0;
const CAR_CAPACITY_WH: f64 = 60_000.0;
const CAR_START_WH: f64 = 26_000.0;
const BATTERY_CAPACITY_WH: f64 = 10_000.0;
const BATTERY_MAX_W: f64 = 2_500.0;
const BATTERY_RESERVE_WH: f64 = 1_000.0;

pub(crate) const MODES: [EvChargeMode; 3] = [
    EvChargeMode::Manual,
    EvChargeMode::Solar,
    EvChargeMode::SolarGrid,
];

/// Endpoint ids of the simulated charger.
pub(crate) const CHARGER: &str = "charger";
pub(crate) const GRID: &str = "grid";
pub(crate) const HOUSE: &str = "house";
pub(crate) const SOLAR: &str = "solar";
pub(crate) const BATTERY: &str = "battery";

/// Hour of the simulated day (`0.0..24.0`) after `elapsed_s` seconds.
pub(crate) fn hour_of_day(elapsed_s: f64) -> f64 {
    (START_HOUR + elapsed_s / DAY_SECS * 24.0).rem_euclid(24.0)
}

/// Solar production at `hour`, before clouds.
fn clear_sky_w(hour: f64) -> f64 {
    if !(SUNRISE_HOUR..SUNSET_HOUR).contains(&hour) {
        return 0.0;
    }
    let x = (hour - SUNRISE_HOUR) / (SUNSET_HOUR - SUNRISE_HOUR);
    SOLAR_PEAK_W * (PI * x).sin().powf(1.4)
}

/// Passing clouds: a smooth, repeatable dip of up to 45 %.
fn cloud_factor(elapsed_s: f64) -> f64 {
    let wave = (elapsed_s / 37.0).sin() * (elapsed_s / 11.0).sin();
    1.0 - 0.45 * wave.max(0.0)
}

/// Appliances on top of the base load at `hour`.
fn appliances_w(hour: f64, elapsed_s: f64) -> f64 {
    let on = |from: f64, to: f64| (from..to).contains(&hour);
    let mut load = 0.0;
    if on(10.9, 11.9) {
        load += 1_900.0; // washing machine
    }
    if on(13.2, 13.9) || on(20.6, 21.3) {
        load += 2_100.0; // oven
    }
    if on(18.5, 22.5) {
        load += 450.0; // lights, TV
    }
    // Fridge compressor: on 40 s out of every 100 s.
    if elapsed_s.rem_euclid(100.0) < 40.0 {
        load += 110.0;
    }
    load
}

fn in_schedule(hour: f64) -> bool {
    hour >= SCHEDULE_FROM_HOUR || hour < SCHEDULE_TO_HOUR
}

fn round_w(watts: f64) -> f64 {
    watts.round()
}

fn round_a(amps: f64) -> f64 {
    (amps * 10.0).round() / 10.0
}

/// The simulated charger and everything around it.
#[derive(Debug)]
pub(crate) struct EvSim {
    solar: bool,
    battery: bool,
    elapsed_s: f64,
    // The car.
    plugged_in: bool,
    car_energy_wh: f64,
    // Settings, changed by commands.
    paused: bool,
    locked: bool,
    schedule_enabled: bool,
    mode: EvChargeMode,
    setpoint_a: f64,
    // What is happening.
    status: EvChargerStatus,
    waiting_for: Option<EvChargerWaitReason>,
    offered_a: f64,
    session_energy_wh: f64,
    session_s: f64,
    solar_w: f64,
    house_w: f64,
    battery_w: f64,
    battery_wh: f64,
    noise: f64,
}

impl EvSim {
    pub(crate) fn new(solar: bool, battery: bool) -> Self {
        let mut sim = Self {
            solar,
            battery,
            elapsed_s: 0.0,
            plugged_in: true,
            car_energy_wh: CAR_START_WH,
            paused: false,
            locked: false,
            schedule_enabled: false,
            mode: EvChargeMode::Manual,
            setpoint_a: 16.0,
            status: EvChargerStatus::Connected,
            waiting_for: None,
            offered_a: 0.0,
            session_energy_wh: 0.0,
            session_s: 0.0,
            solar_w: 0.0,
            house_w: 0.0,
            battery_w: 0.0,
            battery_wh: BATTERY_CAPACITY_WH * 0.6,
            noise: 0.0,
        };
        // The car has been charging for a while when the demo opens.
        sim.offered_a = sim.setpoint_a;
        sim.session_energy_wh = 4_850.0;
        sim.session_s = 1_920.0;
        sim.step(0.0);
        sim
    }

    pub(crate) fn has_solar(&self) -> bool {
        self.solar
    }

    pub(crate) fn has_battery(&self) -> bool {
        self.battery
    }

    /// Advances the simulation by `dt` seconds. `noise` (`-1.0..=1.0`)
    /// wobbles the readings a little, like real meters.
    pub(crate) fn tick(&mut self, dt: f64, noise: f64) {
        self.noise = noise.clamp(-1.0, 1.0);
        self.elapsed_s += dt;
        self.step(dt);
    }

    /// Recomputes everything for the current time; `dt` seconds have passed
    /// since the last step (0 right after a command).
    fn step(&mut self, dt: f64) {
        let hour = hour_of_day(self.elapsed_s);
        let base = BASE_LOAD_W + appliances_w(hour, self.elapsed_s) + 25.0 * self.noise;
        self.solar_w = if self.solar {
            (clear_sky_w(hour) * cloud_factor(self.elapsed_s) * (1.0 + 0.02 * self.noise)).max(0.0)
        } else {
            0.0
        };

        // The home battery covers the house (not the car) and stores
        // surplus solar.
        self.battery_w = 0.0;
        if self.battery {
            let net = self.solar_w - base;
            if net > 0.0 && self.battery_wh < BATTERY_CAPACITY_WH {
                self.battery_w = -net.min(BATTERY_MAX_W);
            } else if net < 0.0 && self.battery_wh > BATTERY_RESERVE_WH {
                self.battery_w = (-net).min(BATTERY_MAX_W);
            }
            self.battery_wh =
                (self.battery_wh - self.battery_w * dt / 3_600.0).clamp(0.0, BATTERY_CAPACITY_WH);
        }

        // Grid import without the car, and what the charger's own power
        // control allows on top of it.
        let grid_without_car = base - self.solar_w - self.battery_w;
        let headroom_w = CONTRACTED_W - grid_without_car;
        // A small margin, like a real charger's control loop.
        let allowed_a = ((headroom_w - 60.0) / VOLTS).floor().min(LIMIT_MAX_A);
        let surplus_a = (-grid_without_car).max(0.0) / VOLTS;

        let wanted_a = match self.mode {
            EvChargeMode::Solar => surplus_a.floor().min(self.setpoint_a),
            EvChargeMode::SolarGrid => surplus_a.floor().max(HW_MIN_A).min(self.setpoint_a),
            EvChargeMode::Manual | EvChargeMode::Scheduled => self.setpoint_a,
        };
        let car_full = self.car_energy_wh >= CAR_CAPACITY_WH;

        let (status, waiting_for) = if !self.plugged_in {
            (EvChargerStatus::Disconnected, None)
        } else if self.paused {
            (EvChargerStatus::Paused, None)
        } else if self.locked || car_full {
            (EvChargerStatus::Connected, None)
        } else if self.schedule_enabled && !in_schedule(hour) {
            (
                EvChargerStatus::Waiting,
                Some(EvChargerWaitReason::Schedule),
            )
        } else if self.mode == EvChargeMode::Solar && wanted_a < HW_MIN_A {
            (EvChargerStatus::Waiting, Some(EvChargerWaitReason::Solar))
        } else if allowed_a < HW_MIN_A {
            (EvChargerStatus::Waiting, Some(EvChargerWaitReason::Power))
        } else {
            (EvChargerStatus::Charging, None)
        };
        self.status = status;
        self.waiting_for = waiting_for;

        if status == EvChargerStatus::Charging {
            let target = wanted_a.min(allowed_a).max(HW_MIN_A);
            // Ramp up gently, drop at once (the charger must never overshoot).
            self.offered_a = if target > self.offered_a {
                (self.offered_a.max(HW_MIN_A) + RAMP_A).min(target)
            } else {
                target
            };
        } else {
            self.offered_a = 0.0;
        }

        let car_w = self.car_power_w();
        self.house_w = base + car_w;
        self.session_energy_wh += car_w * dt / 3_600.0;
        self.car_energy_wh = (self.car_energy_wh + car_w * dt / 3_600.0).min(CAR_CAPACITY_WH);
        if self.plugged_in {
            self.session_s += dt;
        }
    }

    fn car_power_w(&self) -> f64 {
        if self.status == EvChargerStatus::Charging {
            self.offered_a * (VOLTS + 1.5 * self.noise)
        } else {
            0.0
        }
    }

    /// Applies a command, the way the charger would.
    pub(crate) fn apply(&mut self, cmd: EvChargerCommand) -> Result<(), DriverError> {
        match cmd {
            EvChargerCommand::Pause => self.paused = true,
            EvChargerCommand::Resume => self.paused = false,
            EvChargerCommand::SetCurrent { amps } => {
                let amps = if amps.is_nan() { HW_MIN_A } else { amps };
                self.setpoint_a = amps.round().clamp(HW_MIN_A, LIMIT_MAX_A);
            }
            EvChargerCommand::SetMode { mode } => {
                if !MODES.contains(&mode) {
                    return Err(DriverError::Unsupported);
                }
                self.mode = mode;
            }
            EvChargerCommand::Lock => self.locked = true,
            EvChargerCommand::Unlock => self.locked = false,
            EvChargerCommand::SetSchedule { enabled } => self.schedule_enabled = enabled,
        }
        self.step(0.0);
        Ok(())
    }

    pub(crate) fn charger_state(&self) -> EvChargerState {
        EvChargerState {
            status: self.status,
            waiting_for: self.waiting_for,
            power_w: round_w(self.car_power_w()),
            session_energy_wh: self.session_energy_wh.round(),
            session_duration_s: Some(self.session_s.max(0.0).round() as u64),
            current_a: self.setpoint_a,
            auto_current: Some(false),
            phase_current_a: Some(vec![round_a(if self.status == EvChargerStatus::Charging {
                self.offered_a + 0.08 * self.noise
            } else {
                0.0
            })]),
            min_current_a: Some(HW_MIN_A),
            max_current_a: Some(LIMIT_MAX_A),
            mode: Some(self.mode),
            locked: Some(self.locked),
            schedule_enabled: Some(self.schedule_enabled),
        }
    }

    pub(crate) fn grid_state(&self) -> PowerMeterState {
        meter(
            self.house_w - self.solar_w - self.battery_w,
            Some(CONTRACTED_W),
        )
    }

    pub(crate) fn house_state(&self) -> PowerMeterState {
        meter(self.house_w, None)
    }

    pub(crate) fn solar_state(&self) -> PowerMeterState {
        meter(self.solar_w, None)
    }

    pub(crate) fn battery_state(&self) -> PowerMeterState {
        meter(self.battery_w, None)
    }

    #[cfg(test)]
    pub(crate) fn hour(&self) -> f64 {
        hour_of_day(self.elapsed_s)
    }
}

fn meter(power_w: f64, limit_w: Option<f64>) -> PowerMeterState {
    PowerMeterState {
        limit_w,
        ..PowerMeterState::power(round_w(power_w))
    }
}

fn meter_endpoint(id: EndpointId, name: &str, role: MeterRole, bidirectional: bool) -> Endpoint {
    Endpoint {
        id,
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
    }
}

fn endpoint_id(raw: &str) -> Result<EndpointId, DriverError> {
    EndpointId::new(raw).map_err(DriverError::other)
}

/// The charger's endpoints, without state (filled in by [`sync`]).
pub(crate) fn endpoints(sim: &EvSim) -> Result<Vec<Endpoint>, DriverError> {
    let mut endpoints = vec![
        Endpoint {
            id: endpoint_id(CHARGER)?,
            name: "Car charger".to_owned(),
            endpoint_type: EndpointType::EvCharger,
            room: None,
            capabilities: Capabilities {
                ev_charger: Some(EvChargerCapability {
                    features: EvChargerFeatures {
                        min_current_a: HW_MIN_A,
                        max_current_a: HW_MAX_A,
                        modes: MODES.to_vec(),
                        pause: true,
                        adjustable_current: true,
                        lock: true,
                        schedule: true,
                    },
                    state: None,
                }),
                ..Capabilities::default()
            },
        },
        meter_endpoint(endpoint_id(GRID)?, "Grid", MeterRole::Grid, true),
        meter_endpoint(endpoint_id(HOUSE)?, "House", MeterRole::House, false),
    ];
    if sim.has_solar() {
        endpoints.push(meter_endpoint(
            endpoint_id(SOLAR)?,
            "Solar panels",
            MeterRole::Solar,
            false,
        ));
    }
    if sim.has_battery() {
        endpoints.push(meter_endpoint(
            endpoint_id(BATTERY)?,
            "Home battery",
            MeterRole::Battery,
            true,
        ));
    }
    Ok(endpoints)
}

/// Copies the simulation's current readings into the endpoints'
/// capabilities.
pub(crate) fn sync(sim: &EvSim, endpoints: &mut [Endpoint]) {
    for endpoint in endpoints {
        let caps = &mut endpoint.capabilities;
        if let Some(charger) = caps.ev_charger.as_mut() {
            charger.state = Some(sim.charger_state());
        }
        if let Some(meter) = caps.power_meter.as_mut() {
            meter.state = Some(match meter.features.role {
                MeterRole::Grid => sim.grid_state(),
                MeterRole::Solar => sim.solar_state(),
                MeterRole::Battery => sim.battery_state(),
                MeterRole::House | MeterRole::Load | MeterRole::Ev => sim.house_state(),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run_for(sim: &mut EvSim, seconds: f64) {
        let ticks = (seconds / EV_TICK.as_secs_f64()).round() as usize;
        for _ in 0..ticks {
            sim.tick(EV_TICK.as_secs_f64(), 0.0);
        }
    }

    #[test]
    fn the_day_starts_mid_morning_and_wraps() {
        assert_eq!(hour_of_day(0.0), START_HOUR);
        assert!((hour_of_day(DAY_SECS) - START_HOUR).abs() < 1e-9);
        assert!((hour_of_day(DAY_SECS / 2.0) - (START_HOUR + 12.0)).abs() < 1e-9);
        assert_eq!(clear_sky_w(3.0), 0.0);
        assert!(clear_sky_w(14.0) > 3_500.0);
    }

    #[test]
    fn charges_from_the_start_and_never_exceeds_the_contracted_power() {
        let mut sim = EvSim::new(true, false);
        assert_eq!(sim.charger_state().status, EvChargerStatus::Charging);
        // A whole day, with the washing machine and the oven.
        for _ in 0..(DAY_SECS as usize / 2) {
            sim.tick(2.0, 0.0);
            let grid = sim.grid_state();
            assert!(
                grid.power_w <= CONTRACTED_W,
                "grid {} W at {:.1} h",
                grid.power_w,
                sim.hour()
            );
            let state = sim.charger_state();
            if state.status == EvChargerStatus::Charging {
                let measured = state.phase_current_a.unwrap()[0];
                assert!((HW_MIN_A..=LIMIT_MAX_A).contains(&measured.round()));
                assert!(state.power_w > 0.0);
            } else {
                assert_eq!(state.power_w, 0.0);
            }
        }
    }

    #[test]
    fn the_chargers_power_control_throttles_and_holds_back() {
        let mut sim = EvSim::new(false, false);
        sim.apply(EvChargerCommand::SetCurrent { amps: 25.0 })
            .unwrap();
        // Ramp-up takes a few ticks.
        run_for(&mut sim, 10.0);
        let state = sim.charger_state();
        assert_eq!(state.current_a, 25.0);
        assert_eq!(state.status, EvChargerStatus::Charging);
        // 5750 W contracted, ~500 W of house: about 22 A for the car.
        let measured = state.phase_current_a.unwrap()[0];
        assert!((20.0..25.0).contains(&measured), "{measured}");

        // Washing machine + throttling: never over the contracted power.
        run_for(&mut sim, 20.0);
        assert!(sim.grid_state().power_w <= CONTRACTED_W);
    }

    #[test]
    fn commands_change_the_state_at_once() {
        let mut sim = EvSim::new(true, false);
        sim.apply(EvChargerCommand::Pause).unwrap();
        let state = sim.charger_state();
        assert_eq!(state.status, EvChargerStatus::Paused);
        assert_eq!(state.power_w, 0.0);
        sim.apply(EvChargerCommand::Resume).unwrap();
        assert_eq!(sim.charger_state().status, EvChargerStatus::Charging);

        sim.apply(EvChargerCommand::SetCurrent { amps: 40.0 })
            .unwrap();
        assert_eq!(sim.charger_state().current_a, LIMIT_MAX_A);
        sim.apply(EvChargerCommand::SetCurrent { amps: 9.6 })
            .unwrap();
        assert_eq!(sim.charger_state().current_a, 10.0);
        sim.apply(EvChargerCommand::SetCurrent { amps: f64::NAN })
            .unwrap();
        assert_eq!(sim.charger_state().current_a, HW_MIN_A);

        sim.apply(EvChargerCommand::Lock).unwrap();
        let state = sim.charger_state();
        assert_eq!(state.locked, Some(true));
        assert_eq!(state.status, EvChargerStatus::Connected);
        sim.apply(EvChargerCommand::Unlock).unwrap();
        assert_eq!(sim.charger_state().locked, Some(false));

        assert!(matches!(
            sim.apply(EvChargerCommand::SetMode {
                mode: EvChargeMode::Scheduled
            }),
            Err(DriverError::Unsupported)
        ));
    }

    #[test]
    fn the_schedule_waits_for_the_night() {
        let mut sim = EvSim::new(true, false);
        sim.apply(EvChargerCommand::SetSchedule { enabled: true })
            .unwrap();
        let state = sim.charger_state();
        assert_eq!(state.status, EvChargerStatus::Waiting);
        assert_eq!(state.waiting_for, Some(EvChargerWaitReason::Schedule));
        assert_eq!(state.schedule_enabled, Some(true));
        // 23:00 is 12.5 simulated hours after 10:30.
        run_for(&mut sim, DAY_SECS * 12.6 / 24.0);
        assert!(in_schedule(sim.hour()), "{}", sim.hour());
        assert_eq!(sim.charger_state().status, EvChargerStatus::Charging);
    }

    #[test]
    fn solar_mode_waits_for_the_sun() {
        let mut sim = EvSim::new(true, false);
        sim.apply(EvChargerCommand::SetMode {
            mode: EvChargeMode::Solar,
        })
        .unwrap();
        // Night: no sun at all.
        run_for(&mut sim, DAY_SECS * 13.0 / 24.0);
        let state = sim.charger_state();
        assert_eq!(state.status, EvChargerStatus::Waiting);
        assert_eq!(state.waiting_for, Some(EvChargerWaitReason::Solar));

        sim.apply(EvChargerCommand::SetMode {
            mode: EvChargeMode::SolarGrid,
        })
        .unwrap();
        assert_eq!(sim.charger_state().status, EvChargerStatus::Charging);
    }

    #[test]
    fn meters_add_up() {
        let mut sim = EvSim::new(true, true);
        for _ in 0..100 {
            sim.tick(2.0, 0.3);
            let house = sim.house_state().power_w;
            let sources =
                sim.grid_state().power_w + sim.solar_state().power_w + sim.battery_state().power_w;
            assert!((house - sources).abs() <= 2.0, "{house} vs {sources}");
        }
    }
}
