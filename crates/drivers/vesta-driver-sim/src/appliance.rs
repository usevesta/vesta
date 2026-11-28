//! Simulated plug with a power meter, and a simulated robot vacuum, on a
//! compressed clock (one tick of 2 s is a minute of cleaning).
//!
//! - `smart_plug`: an `outlet` with `on_off` and a `load` meter. While on it
//!   draws `load_w` (with a little noise) and counts energy.
//! - `robot_vacuum`: `robot_vacuum` and `battery`. Cleans until about 40 m²
//!   or a low battery, goes back to its base, charges, and reports
//!   `docked` when full.
//!
//! With `read_only`, both report no controls and refuse every command, like
//! a real device set up for monitoring only.

use std::time::Duration;

use vesta_core::{
    BatteryCapability, BatteryState, Capabilities, DriverError, Endpoint, EndpointId, EndpointType,
    MeterRole, OnOffCapability, OnOffFeatures, OnOffState, Percent, PowerMeterCapability,
    PowerMeterFeatures, PowerMeterState, RobotVacuumCapability, RobotVacuumCommand,
    RobotVacuumFeatures, RobotVacuumState, RobotVacuumStatus,
};

pub(crate) const TICK: Duration = Duration::from_secs(2);
/// Default draw of the simulated plug's appliance.
pub(crate) const DEFAULT_LOAD_W: f64 = 60.0;
const VOLTS: f64 = 230.0;

/// Cleaning, per tick: one minute, half a square metre, one percent.
const CLEAN_S_PER_TICK: f64 = 60.0;
const CLEAN_M2_PER_TICK: f64 = 0.5;
const DRAIN_PER_TICK: f64 = 1.0;
const CHARGE_PER_TICK: f64 = 2.0;
/// A whole flat, then back home.
const FULL_CLEAN_M2: f64 = 40.0;
/// Goes home by itself at this charge.
const RETURN_AT_PERCENT: f64 = 15.0;
const RETURN_TICKS: u32 = 3;

#[derive(Debug, Clone)]
pub(crate) struct PlugSim {
    pub(crate) on: bool,
    load_w: f64,
    power_w: f64,
    energy_wh: f64,
}

impl PlugSim {
    pub(crate) fn new(load_w: f64) -> Self {
        Self {
            on: true,
            load_w,
            power_w: load_w,
            energy_wh: 18_400.0,
        }
    }

    /// `noise` is in `-1.0..=1.0`.
    pub(crate) fn tick(&mut self, dt: f64, noise: f64) {
        self.power_w = if self.on {
            (self.load_w * noise.mul_add(0.03, 1.0)).max(0.0)
        } else {
            0.0
        };
        self.energy_wh += self.power_w * dt / 3_600.0;
    }

    pub(crate) fn switch(&mut self, on: bool) {
        self.on = on;
        self.power_w = if on { self.load_w } else { 0.0 };
    }

    pub(crate) fn meter(&self) -> PowerMeterState {
        let power_w = (self.power_w * 10.0).round() / 10.0;
        PowerMeterState {
            energy_wh: Some(self.energy_wh.round()),
            voltage_v: Some(VOLTS),
            current_a: Some((power_w / VOLTS * 1_000.0).round() / 1_000.0),
            ..PowerMeterState::power(power_w)
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct RobotSim {
    pub(crate) status: RobotVacuumStatus,
    battery: f64,
    area_m2: f64,
    duration_s: f64,
    returning_ticks: u32,
}

impl RobotSim {
    pub(crate) fn new() -> Self {
        Self {
            status: RobotVacuumStatus::Docked,
            battery: 100.0,
            area_m2: 22.0,
            duration_s: 1_800.0,
            returning_ticks: 0,
        }
    }

    fn go_home(&mut self) {
        self.status = RobotVacuumStatus::Returning;
        self.returning_ticks = RETURN_TICKS;
    }

    pub(crate) fn apply(&mut self, cmd: RobotVacuumCommand) -> Result<(), DriverError> {
        use RobotVacuumStatus as S;
        match (cmd, self.status) {
            (RobotVacuumCommand::Start, S::Paused) => self.status = S::Cleaning,
            (RobotVacuumCommand::Start, S::Cleaning) => {}
            (RobotVacuumCommand::Start, _) => {
                if self.battery <= RETURN_AT_PERCENT {
                    // Like a real robot: too little charge to set off.
                    return Err(DriverError::Protocol("battery too low".to_owned()));
                }
                self.status = S::Cleaning;
                self.area_m2 = 0.0;
                self.duration_s = 0.0;
            }
            (RobotVacuumCommand::Pause, S::Cleaning | S::Returning) => self.status = S::Paused,
            (RobotVacuumCommand::Pause, _) => {}
            (RobotVacuumCommand::ReturnToDock, S::Docked | S::Charging | S::Returning) => {}
            (RobotVacuumCommand::ReturnToDock, _) => self.go_home(),
        }
        Ok(())
    }

    pub(crate) fn tick(&mut self) {
        use RobotVacuumStatus as S;
        match self.status {
            S::Cleaning => {
                self.duration_s += CLEAN_S_PER_TICK;
                self.area_m2 += CLEAN_M2_PER_TICK;
                self.battery = (self.battery - DRAIN_PER_TICK).max(0.0);
                if self.area_m2 >= FULL_CLEAN_M2 || self.battery <= RETURN_AT_PERCENT {
                    self.go_home();
                }
            }
            S::Returning => {
                self.battery = (self.battery - DRAIN_PER_TICK / 2.0).max(0.0);
                self.returning_ticks = self.returning_ticks.saturating_sub(1);
                if self.returning_ticks == 0 {
                    self.status = S::Charging;
                }
            }
            S::Charging => {
                self.battery = (self.battery + CHARGE_PER_TICK).min(100.0);
                if self.battery >= 100.0 {
                    self.status = S::Docked;
                }
            }
            S::Docked | S::Paused | S::Idle | S::Error => {}
        }
    }

    pub(crate) fn state(&self) -> RobotVacuumState {
        RobotVacuumState {
            status: self.status,
            cleaned_area_m2: Some(self.area_m2.round()),
            cleaning_duration_s: Some(self.duration_s.round() as u64),
        }
    }

    pub(crate) fn battery(&self) -> BatteryState {
        let percent = Percent::saturating(self.battery.round().clamp(0.0, 100.0) as u8);
        BatteryState {
            percent,
            low: percent.get() <= 20,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) enum Appliance {
    Plug(PlugSim),
    Robot(RobotSim),
}

impl Appliance {
    /// The endpoint, with its current state.
    pub(crate) fn endpoint(&self, read_only: bool) -> Endpoint {
        match self {
            Self::Plug(plug) => Endpoint {
                id: EndpointId::main(),
                name: "Plug".to_owned(),
                endpoint_type: EndpointType::Outlet,
                room: None,
                capabilities: Capabilities {
                    on_off: Some(OnOffCapability {
                        features: read_only.then_some(OnOffFeatures { read_only: true }),
                        state: Some(OnOffState { on: plug.on }),
                    }),
                    power_meter: Some(PowerMeterCapability {
                        features: PowerMeterFeatures {
                            role: MeterRole::Load,
                            bidirectional: false,
                        },
                        state: Some(plug.meter()),
                    }),
                    ..Capabilities::default()
                },
            },
            Self::Robot(robot) => Endpoint {
                id: EndpointId::main(),
                name: "Robot vacuum".to_owned(),
                endpoint_type: EndpointType::RobotVacuum,
                room: None,
                capabilities: Capabilities {
                    robot_vacuum: Some(RobotVacuumCapability {
                        features: RobotVacuumFeatures {
                            start: !read_only,
                            pause: !read_only,
                            return_to_dock: !read_only,
                        },
                        state: Some(robot.state()),
                    }),
                    battery: Some(BatteryCapability {
                        state: Some(robot.battery()),
                    }),
                    ..Capabilities::default()
                },
            },
        }
    }

    /// Copies the simulation's readings into `endpoint`.
    pub(crate) fn sync(&self, endpoint: &mut Endpoint) {
        let caps = &mut endpoint.capabilities;
        match self {
            Self::Plug(plug) => {
                if let Some(on_off) = caps.on_off.as_mut() {
                    on_off.state = Some(OnOffState { on: plug.on });
                }
                if let Some(meter) = caps.power_meter.as_mut() {
                    meter.state = Some(plug.meter());
                }
            }
            Self::Robot(robot) => {
                if let Some(vacuum) = caps.robot_vacuum.as_mut() {
                    vacuum.state = Some(robot.state());
                }
                if let Some(battery) = caps.battery.as_mut() {
                    battery.state = Some(robot.battery());
                }
            }
        }
    }

    pub(crate) fn tick(&mut self, dt: f64, noise: f64) {
        match self {
            Self::Plug(plug) => plug.tick(dt, noise),
            Self::Robot(robot) => robot.tick(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plug_draws_only_while_on_and_counts_energy() {
        let mut plug = PlugSim::new(100.0);
        let before = plug.meter().energy_wh.unwrap();
        plug.tick(3_600.0, 0.0);
        let meter = plug.meter();
        assert_eq!(meter.power_w, 100.0);
        assert_eq!(meter.energy_wh, Some(before + 100.0));
        assert_eq!(meter.current_a, Some(0.435));
        plug.switch(false);
        plug.tick(3_600.0, 1.0);
        assert_eq!(plug.meter().power_w, 0.0);
        assert_eq!(plug.meter().energy_wh, Some(before + 100.0));
    }

    #[test]
    fn a_robot_cleans_goes_home_and_charges() {
        use RobotVacuumStatus as S;
        let mut robot = RobotSim::new();
        assert_eq!(robot.state().status, S::Docked);
        robot.apply(RobotVacuumCommand::Start).unwrap();
        assert_eq!(robot.state().cleaned_area_m2, Some(0.0));
        robot.tick();
        assert_eq!(robot.state().status, S::Cleaning);
        assert_eq!(robot.state().cleaning_duration_s, Some(60));
        robot.apply(RobotVacuumCommand::Pause).unwrap();
        robot.tick();
        assert_eq!(robot.state().status, S::Paused);
        assert_eq!(robot.state().cleaning_duration_s, Some(60));
        // Carrying on keeps the clean going.
        robot.apply(RobotVacuumCommand::Start).unwrap();
        assert_eq!(robot.state().cleaning_duration_s, Some(60));
        robot.apply(RobotVacuumCommand::ReturnToDock).unwrap();
        assert_eq!(robot.state().status, S::Returning);
        for _ in 0..RETURN_TICKS {
            robot.tick();
        }
        assert_eq!(robot.state().status, S::Charging);
        for _ in 0..60 {
            robot.tick();
        }
        assert_eq!(robot.state().status, S::Docked);
        assert_eq!(robot.battery().percent.get(), 100);
    }

    #[test]
    fn a_robot_comes_home_by_itself() {
        let mut robot = RobotSim::new();
        robot.apply(RobotVacuumCommand::Start).unwrap();
        for _ in 0..200 {
            robot.tick();
            if robot.state().status != RobotVacuumStatus::Cleaning {
                break;
            }
        }
        assert_eq!(robot.state().status, RobotVacuumStatus::Returning);
        assert!(robot.state().cleaned_area_m2.unwrap() <= FULL_CLEAN_M2);
    }
}
