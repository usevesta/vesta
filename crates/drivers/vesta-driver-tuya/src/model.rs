//! Tuya product categories → brand-free endpoints and capabilities, data
//! points → capability state, and commands → the exact data points to set.
//!
//! | Category | Endpoint (type) | Capabilities | From data points |
//! |---|---|---|---|
//! | `cz` socket, `tdq` on/off switch, `kg` switch | `main` (`outlet`, or `switch` for `kg` and non-metering `tdq`); several channels: `switch_1`, `switch_2`, … | `on_off`, plus `power_meter` (`load`) when it measures | `switch`, `switch_N`; `cur_power`, `cur_voltage`, `cur_current`, `total_forward_energy` |
//! | `sd` robot vacuum | `main` (`robot_vacuum`) | `robot_vacuum`, `battery` | `status`, `fault`, `electricity_left`, `clean_area`, `clean_time`; commands `power_go`, `pause`, `switch_charge` or `mode: chargego` |
//!
//! Anything else exposes nothing (and is logged): VESTA never guesses what
//! an unknown device does. Values are converted with the scale and unit of
//! each data point (e.g. `cur_voltage` 2272 with scale 1 is 227.2 V).

use std::collections::HashMap;

use serde_json::{Map, Value};
use vesta_core::{
    BatteryCapability, BatteryState, Capabilities, CapabilityState, Command, DriverError, Endpoint,
    EndpointId, EndpointType, MeterRole, OnOffCapability, OnOffCommand, OnOffFeatures, OnOffState,
    Percent, PowerMeterCapability, PowerMeterFeatures, PowerMeterState, RobotVacuumCapability,
    RobotVacuumCommand, RobotVacuumFeatures, RobotVacuumState, RobotVacuumStatus,
};

use crate::config::DataPoint;

/// The device's data points, as last reported (DP number → value).
pub type DpValues = HashMap<String, Value>;

/// The robot counts as low on charge at or below this.
const LOW_BATTERY_PERCENT: u8 = 20;

/// A numeric data point and how to turn it into the capability's unit.
#[derive(Debug, Clone, PartialEq)]
struct Measure {
    dp: String,
    /// Reported value × factor = value in the capability's unit.
    factor: f64,
}

impl Measure {
    fn read(&self, values: &DpValues) -> Option<f64> {
        let raw = match values.get(&self.dp)? {
            Value::Number(number) => number.as_f64()?,
            // Some firmware reports numbers as strings.
            Value::String(text) => text.trim().parse().ok()?,
            _ => return None,
        };
        let value = raw * self.factor;
        value.is_finite().then_some(value)
    }
}

/// `10^-scale`, times a unit conversion.
fn factor(point: &DataPoint, unit_factor: f64) -> f64 {
    let scale = i32::try_from(point.scale()).unwrap_or(0);
    10f64.powi(-scale) * unit_factor
}

fn unit(point: &DataPoint) -> String {
    point.unit().unwrap_or_default().to_ascii_lowercase()
}

/// Rounds away floating-point noise from scaling (227.20000000000002).
fn tidy(value: f64, decimals: i32) -> f64 {
    let step = 10f64.powi(decimals);
    (value * step).round() / step
}

#[derive(Debug, Clone, PartialEq)]
struct Channel {
    endpoint: EndpointId,
    name: String,
    dp: String,
}

#[derive(Debug, Clone, PartialEq)]
struct Meter {
    endpoint: EndpointId,
    power: Measure,
    voltage: Option<Measure>,
    current: Option<Measure>,
    energy: Option<Measure>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Switches {
    endpoint_type: EndpointType,
    channels: Vec<Channel>,
    meter: Option<Meter>,
}

#[derive(Debug, Clone, PartialEq)]
enum Dock {
    /// A boolean data point (`switch_charge`).
    Switch(String),
    /// The `mode` data point, set to `chargego`.
    Mode(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Vacuum {
    status: Option<String>,
    fault: Option<String>,
    battery: Option<Measure>,
    area: Option<Measure>,
    /// Minutes, as reported.
    duration: Option<Measure>,
    start: Option<String>,
    pause: Option<String>,
    dock: Option<Dock>,
}

/// What VESTA makes of one device.
#[derive(Debug, Clone, PartialEq)]
pub enum Model {
    Switches(Switches),
    Vacuum(Vacuum),
    /// A category VESTA does not know: nothing is exposed.
    Unsupported,
}

fn find<'a>(mapping: &'a [(String, DataPoint)], codes: &[&str]) -> Option<&'a (String, DataPoint)> {
    codes
        .iter()
        .find_map(|code| mapping.iter().find(|(_, point)| point.code == *code))
}

fn endpoint_id(raw: &str) -> Result<EndpointId, DriverError> {
    EndpointId::new(raw).map_err(DriverError::other)
}

/// `switch`, `switch_1`, `switch_2`, …: the relays (not `switch_led`,
/// `switch_usb1` and friends).
fn channel_number(code: &str) -> Option<u32> {
    if code == "switch" {
        return Some(0);
    }
    code.strip_prefix("switch_")?.parse().ok()
}

impl Model {
    /// Reads the category and data points of a device.
    pub fn new(
        category: &str,
        mapping: &std::collections::BTreeMap<String, DataPoint>,
    ) -> Result<Self, DriverError> {
        let mut points: Vec<(String, DataPoint)> = mapping
            .iter()
            .map(|(dp, point)| (dp.clone(), point.clone()))
            .collect();
        points.sort_by_key(|(dp, _)| dp.parse::<u16>().unwrap_or(u16::MAX));
        Ok(match category {
            "cz" | "kg" | "tdq" => match Self::switches(category, &points)? {
                Some(switches) => Self::Switches(switches),
                None => {
                    tracing::warn!(
                        category,
                        "this Tuya device has no on/off data point; nothing is shown"
                    );
                    Self::Unsupported
                }
            },
            "sd" => Self::Vacuum(Self::vacuum(&points)),
            other => {
                tracing::warn!(
                    category = other,
                    "Tuya category not supported yet; the device is not shown"
                );
                Self::Unsupported
            }
        })
    }

    fn switches(
        category: &str,
        points: &[(String, DataPoint)],
    ) -> Result<Option<Switches>, DriverError> {
        let mut relays: Vec<(u32, &String)> = points
            .iter()
            .filter(|(_, point)| point.is("Boolean"))
            .filter_map(|(dp, point)| channel_number(&point.code).map(|n| (n, dp)))
            .collect();
        relays.sort();
        if relays.is_empty() {
            return Ok(None);
        }
        let single = relays.len() == 1;
        let mut channels = Vec::new();
        for (number, dp) in &relays {
            let (id, name) = if single {
                ("main".to_owned(), "Switch".to_owned())
            } else {
                (format!("switch_{number}"), format!("Switch {number}"))
            };
            channels.push(Channel {
                endpoint: endpoint_id(&id)?,
                name,
                dp: (*dp).clone(),
            });
        }

        let measure = |codes: &[&str], convert: &dyn Fn(&str) -> Option<f64>| {
            find(points, codes).and_then(|(dp, point)| {
                convert(&unit(point)).map(|unit_factor| Measure {
                    dp: dp.clone(),
                    factor: factor(point, unit_factor),
                })
            })
        };
        let power = measure(&["cur_power"], &|unit| match unit {
            "" | "w" => Some(1.0),
            "kw" => Some(1000.0),
            _ => None,
        });
        let meter = match power {
            Some(power) => Some(Meter {
                endpoint: if single {
                    endpoint_id("main")?
                } else {
                    endpoint_id("power")?
                },
                power,
                voltage: measure(&["cur_voltage"], &|unit| {
                    matches!(unit, "" | "v").then_some(1.0)
                }),
                current: measure(&["cur_current"], &|unit| match unit {
                    "" | "ma" => Some(0.001),
                    "a" => Some(1.0),
                    _ => None,
                }),
                // A lifetime counter. `add_ele` is not: it is the energy
                // since the device last told the Tuya cloud, which adds it
                // up; VESTA does not invent a total from it.
                energy: measure(&["total_forward_energy", "forward_energy_total"], &|unit| {
                    match unit {
                        "" | "kwh" | "kw·h" | "kw.h" => Some(1000.0),
                        "wh" | "w·h" => Some(1.0),
                        _ => None,
                    }
                }),
            }),
            None => None,
        };
        let endpoint_type = match category {
            "cz" => EndpointType::Outlet,
            "kg" => EndpointType::Switch,
            _ if meter.is_some() => EndpointType::Outlet,
            _ => EndpointType::Switch,
        };
        Ok(Some(Switches {
            endpoint_type,
            channels,
            meter,
        }))
    }

    fn vacuum(points: &[(String, DataPoint)]) -> Vacuum {
        let boolean = |code: &str| {
            find(points, &[code])
                .filter(|(_, point)| point.is("Boolean"))
                .map(|(dp, _)| dp.clone())
        };
        let integer = |codes: &[&str], unit_factor: f64| {
            find(points, codes).map(|(dp, point)| Measure {
                dp: dp.clone(),
                factor: factor(point, unit_factor),
            })
        };
        let dock = boolean("switch_charge").map(Dock::Switch).or_else(|| {
            find(points, &["mode"])
                .filter(|(_, point)| point.range().iter().any(|mode| mode == "chargego"))
                .map(|(dp, _)| Dock::Mode(dp.clone()))
        });
        Vacuum {
            status: find(points, &["status"]).map(|(dp, _)| dp.clone()),
            fault: find(points, &["fault"]).map(|(dp, _)| dp.clone()),
            battery: integer(&["electricity_left", "residual_electricity"], 1.0),
            area: integer(&["clean_area"], 1.0),
            duration: integer(&["clean_time"], 1.0),
            start: boolean("power_go"),
            pause: boolean("pause"),
            dock,
        }
    }

    /// Endpoints, types, capabilities and features. With `read_only`, every
    /// feature that would send a command is off.
    pub fn endpoints(&self, read_only: bool) -> Result<Vec<Endpoint>, DriverError> {
        match self {
            Self::Unsupported => Ok(Vec::new()),
            Self::Switches(switches) => {
                let mut endpoints: Vec<Endpoint> = switches
                    .channels
                    .iter()
                    .map(|channel| Endpoint {
                        id: channel.endpoint.clone(),
                        name: channel.name.clone(),
                        endpoint_type: switches.endpoint_type,
                        room: None,
                        capabilities: Capabilities {
                            on_off: Some(OnOffCapability {
                                features: read_only.then_some(OnOffFeatures { read_only: true }),
                                state: None,
                            }),
                            ..Capabilities::default()
                        },
                    })
                    .collect();
                if let Some(meter) = &switches.meter {
                    let capability = PowerMeterCapability {
                        features: PowerMeterFeatures {
                            role: MeterRole::Load,
                            bidirectional: false,
                        },
                        state: None,
                    };
                    match endpoints.iter_mut().find(|e| e.id == meter.endpoint) {
                        Some(endpoint) => endpoint.capabilities.power_meter = Some(capability),
                        None => endpoints.push(Endpoint {
                            id: meter.endpoint.clone(),
                            name: "Power".to_owned(),
                            endpoint_type: EndpointType::Meter,
                            room: None,
                            capabilities: Capabilities {
                                power_meter: Some(capability),
                                ..Capabilities::default()
                            },
                        }),
                    }
                }
                Ok(endpoints)
            }
            Self::Vacuum(vacuum) => {
                let control = !read_only;
                Ok(vec![Endpoint {
                    id: EndpointId::main(),
                    name: "Robot vacuum".to_owned(),
                    endpoint_type: EndpointType::RobotVacuum,
                    room: None,
                    capabilities: Capabilities {
                        robot_vacuum: Some(RobotVacuumCapability {
                            features: RobotVacuumFeatures {
                                start: control && vacuum.start.is_some(),
                                pause: control
                                    && (vacuum.pause.is_some() || vacuum.start.is_some()),
                                return_to_dock: control && vacuum.dock.is_some(),
                            },
                            state: None,
                        }),
                        battery: vacuum
                            .battery
                            .as_ref()
                            .map(|_| BatteryCapability::default()),
                        ..Capabilities::default()
                    },
                }])
            }
        }
    }

    /// Every capability state the reported values make known. A value that
    /// is missing or not understood leaves its state unknown (it is never
    /// invented).
    pub fn states(&self, values: &DpValues) -> Vec<(EndpointId, CapabilityState)> {
        let mut states = Vec::new();
        match self {
            Self::Unsupported => {}
            Self::Switches(switches) => {
                for channel in &switches.channels {
                    if let Some(on) = values.get(&channel.dp).and_then(Value::as_bool) {
                        states.push((channel.endpoint.clone(), OnOffState { on }.into()));
                    }
                }
                if let Some(meter) = &switches.meter
                    && let Some(power) = meter.power.read(values)
                {
                    let state = PowerMeterState {
                        energy_wh: meter
                            .energy
                            .as_ref()
                            .and_then(|m| m.read(values))
                            .map(|wh| tidy(wh, 1)),
                        voltage_v: meter
                            .voltage
                            .as_ref()
                            .and_then(|m| m.read(values))
                            .map(|v| tidy(v, 1)),
                        current_a: meter
                            .current
                            .as_ref()
                            .and_then(|m| m.read(values))
                            .map(|a| tidy(a, 3)),
                        ..PowerMeterState::power(tidy(power, 1))
                    };
                    states.push((meter.endpoint.clone(), state.into()));
                }
            }
            Self::Vacuum(vacuum) => {
                let main = EndpointId::main();
                if let Some(status) = vacuum.status(values) {
                    let state = RobotVacuumState {
                        status,
                        cleaned_area_m2: vacuum
                            .area
                            .as_ref()
                            .and_then(|m| m.read(values))
                            .filter(|area| *area >= 0.0)
                            .map(|area| tidy(area, 1)),
                        cleaning_duration_s: vacuum
                            .duration
                            .as_ref()
                            .and_then(|m| m.read(values))
                            .filter(|minutes| *minutes >= 0.0)
                            // Whole minutes, as reported.
                            .map(|minutes| (minutes * 60.0).round() as u64),
                    };
                    states.push((main.clone(), state.into()));
                }
                if let Some(percent) = vacuum.battery.as_ref().and_then(|m| m.read(values)) {
                    let percent = Percent::saturating(percent.clamp(0.0, 100.0).round() as u8);
                    states.push((
                        main,
                        BatteryState {
                            percent,
                            low: percent.get() <= LOW_BATTERY_PERCENT,
                        }
                        .into(),
                    ));
                }
            }
        }
        states
    }

    /// Whether `endpoint` exists, and whether it takes commands at all.
    pub fn knows(&self, endpoint: &EndpointId) -> bool {
        match self {
            Self::Unsupported => false,
            Self::Switches(switches) => {
                switches.channels.iter().any(|c| &c.endpoint == endpoint)
                    || switches
                        .meter
                        .as_ref()
                        .is_some_and(|m| &m.endpoint == endpoint)
            }
            Self::Vacuum(_) => endpoint.as_str() == "main",
        }
    }

    /// The data points to set for `cmd`: exactly what is sent to the
    /// device, nothing else.
    pub fn plan(
        &self,
        endpoint: &EndpointId,
        cmd: &Command,
        values: &DpValues,
    ) -> Result<Map<String, Value>, DriverError> {
        let mut dps = Map::new();
        match (self, cmd) {
            (Self::Switches(switches), Command::OnOff(action)) => {
                let channel = switches
                    .channels
                    .iter()
                    .find(|channel| &channel.endpoint == endpoint)
                    .ok_or(DriverError::Unsupported)?;
                let on = match action {
                    OnOffCommand::On => true,
                    OnOffCommand::Off => false,
                    // Toggling needs to know the state first.
                    OnOffCommand::Toggle => !values
                        .get(&channel.dp)
                        .and_then(Value::as_bool)
                        .ok_or(DriverError::Offline)?,
                };
                dps.insert(channel.dp.clone(), Value::Bool(on));
            }
            (Self::Vacuum(vacuum), Command::RobotVacuum(action)) => match action {
                RobotVacuumCommand::Start => {
                    let start = vacuum.start.as_ref().ok_or(DriverError::Unsupported)?;
                    match (&vacuum.pause, vacuum.status(values)) {
                        // Carry on where it paused.
                        (Some(pause), Some(RobotVacuumStatus::Paused)) => {
                            dps.insert(pause.clone(), Value::Bool(false));
                        }
                        _ => {
                            dps.insert(start.clone(), Value::Bool(true));
                        }
                    }
                }
                RobotVacuumCommand::Pause => match (&vacuum.pause, &vacuum.start) {
                    (Some(pause), _) => {
                        dps.insert(pause.clone(), Value::Bool(true));
                    }
                    (None, Some(start)) => {
                        dps.insert(start.clone(), Value::Bool(false));
                    }
                    (None, None) => return Err(DriverError::Unsupported),
                },
                RobotVacuumCommand::ReturnToDock => match &vacuum.dock {
                    Some(Dock::Switch(dp)) => {
                        dps.insert(dp.clone(), Value::Bool(true));
                    }
                    Some(Dock::Mode(dp)) => {
                        dps.insert(dp.clone(), Value::String("chargego".to_owned()));
                    }
                    None => return Err(DriverError::Unsupported),
                },
            },
            _ => return Err(DriverError::Unsupported),
        }
        Ok(dps)
    }
}

impl Vacuum {
    /// The robot's status in VESTA's words. Unknown values stay unknown.
    fn status(&self, values: &DpValues) -> Option<RobotVacuumStatus> {
        let faulty = self
            .fault
            .as_ref()
            .and_then(|dp| values.get(dp))
            .is_some_and(|fault| match fault {
                Value::Number(number) => number.as_u64().is_some_and(|code| code != 0),
                Value::Bool(flag) => *flag,
                Value::String(text) => !text.is_empty() && text != "0",
                _ => false,
            });
        if faulty {
            return Some(RobotVacuumStatus::Error);
        }
        let raw = self
            .status
            .as_ref()
            .and_then(|dp| values.get(dp))?
            .as_str()?;
        Some(match raw {
            "charge_done" | "chargecompleted" | "charge_completed" | "fully_charged" => {
                RobotVacuumStatus::Docked
            }
            "charging" => RobotVacuumStatus::Charging,
            "cleaning" | "smart_clean" | "smart" | "zone_clean" | "part_clean" | "spot_clean"
            | "wall_clean" | "edge_clean" | "single_clean" | "goto_pos" | "mop_clean"
            | "select_room" => RobotVacuumStatus::Cleaning,
            "paused" | "pause" => RobotVacuumStatus::Paused,
            "goto_charge" | "go_charging" | "back_charge" | "chargego" => {
                RobotVacuumStatus::Returning
            }
            "standby" | "sleep" | "pos_arrived" | "pos_unarrive" | "idle" | "standy" => {
                RobotVacuumStatus::Idle
            }
            "fault" | "error" | "trapped" | "stuck" => RobotVacuumStatus::Error,
            other => {
                tracing::debug!(status = other, "robot vacuum status not understood");
                return None;
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use serde_json::json;

    use super::*;

    pub(crate) fn mapping(value: Value) -> BTreeMap<String, DataPoint> {
        serde_json::from_value(value).unwrap()
    }

    fn plug() -> BTreeMap<String, DataPoint> {
        mapping(json!({
            "1": { "code": "switch_1", "type": "Boolean", "values": {} },
            "9": { "code": "countdown_1", "type": "Integer", "values": { "unit": "s", "min": 0, "max": 86400, "scale": 0, "step": 1 } },
            "20": { "code": "add_ele", "type": "Integer", "values": { "unit": "kW·h", "min": 0, "max": 50000, "scale": 3, "step": 100 } },
            "21": { "code": "cur_current", "type": "Integer", "values": { "unit": "mA", "min": 0, "max": 30000, "scale": 0, "step": 1 } },
            "22": { "code": "cur_power", "type": "Integer", "values": { "unit": "W", "min": 0, "max": 50000, "scale": 1, "step": 1 } },
            "23": { "code": "cur_voltage", "type": "Integer", "values": { "unit": "V", "min": 0, "max": 5000, "scale": 1, "step": 1 } }
        }))
    }

    fn values(value: Value) -> DpValues {
        serde_json::from_value(value).unwrap()
    }

    #[test]
    fn a_metering_plug_is_an_outlet_with_a_meter() {
        let model = Model::new("tdq", &plug()).unwrap();
        let endpoints = model.endpoints(false).unwrap();
        assert_eq!(endpoints.len(), 1);
        let main = &endpoints[0];
        assert_eq!(main.id, "main");
        assert_eq!(main.endpoint_type, EndpointType::Outlet);
        assert!(!main.capabilities.on_off.as_ref().unwrap().read_only());
        assert_eq!(
            main.capabilities.power_meter.as_ref().unwrap().features,
            PowerMeterFeatures {
                role: MeterRole::Load,
                bidirectional: false
            }
        );

        let states = model.states(&values(json!({
            "1": true, "20": 2, "21": 112, "22": 125, "23": 2272
        })));
        assert_eq!(
            states,
            vec![
                (EndpointId::main(), OnOffState { on: true }.into()),
                (
                    EndpointId::main(),
                    PowerMeterState {
                        voltage_v: Some(227.2),
                        current_a: Some(0.112),
                        ..PowerMeterState::power(12.5)
                    }
                    .into()
                ),
            ]
        );
    }

    #[test]
    fn read_only_switches_say_so() {
        let model = Model::new("cz", &plug()).unwrap();
        let endpoints = model.endpoints(true).unwrap();
        assert!(
            endpoints[0]
                .capabilities
                .on_off
                .as_ref()
                .unwrap()
                .read_only()
        );
        assert!(
            !endpoints[0]
                .capabilities
                .supports(&Command::OnOff(OnOffCommand::On))
        );
    }

    #[test]
    fn categories_and_channels() {
        let relay = mapping(json!({ "1": { "code": "switch_1", "type": "Boolean" } }));
        let types = |category: &str, mapping: &BTreeMap<String, DataPoint>| {
            Model::new(category, mapping)
                .unwrap()
                .endpoints(false)
                .unwrap()[0]
                .endpoint_type
        };
        assert_eq!(types("tdq", &relay), EndpointType::Switch);
        assert_eq!(types("kg", &plug()), EndpointType::Switch);
        assert_eq!(types("cz", &relay), EndpointType::Outlet);

        let two = mapping(json!({
            "1": { "code": "switch_1", "type": "Boolean" },
            "2": { "code": "switch_2", "type": "Boolean" },
            "7": { "code": "switch_led", "type": "Boolean" },
            "8": { "code": "switch_usb1", "type": "Boolean" },
            "22": { "code": "cur_power", "type": "Integer", "values": { "unit": "W", "scale": 1 } }
        }));
        let model = Model::new("cz", &two).unwrap();
        let ids: Vec<String> = model
            .endpoints(false)
            .unwrap()
            .iter()
            .map(|e| e.id.to_string())
            .collect();
        assert_eq!(ids, ["switch_1", "switch_2", "power"]);
        let states = model.states(&values(json!({ "1": false, "2": true, "22": 0 })));
        assert_eq!(states.len(), 3);
        assert_eq!(states[2].0, "power");
    }

    #[test]
    fn unknown_categories_and_relayless_devices_show_nothing() {
        assert_eq!(Model::new("mcs", &plug()).unwrap(), Model::Unsupported);
        assert_eq!(
            Model::new("dcb", &BTreeMap::new()).unwrap(),
            Model::Unsupported
        );
        let no_relay = mapping(json!({ "22": { "code": "cur_power", "type": "Integer" } }));
        assert_eq!(Model::new("cz", &no_relay).unwrap(), Model::Unsupported);
        assert!(Model::Unsupported.endpoints(false).unwrap().is_empty());
        assert!(
            Model::Unsupported
                .states(&values(json!({ "1": true })))
                .is_empty()
        );
    }

    #[test]
    fn energy_only_from_a_lifetime_counter() {
        let mut meter = plug();
        meter.insert(
            "17".to_owned(),
            serde_json::from_value(json!({ "code": "total_forward_energy", "type": "Integer",
                "values": { "unit": "kW·h", "scale": 2 } }))
            .unwrap(),
        );
        let model = Model::new("cz", &meter).unwrap();
        let states = model.states(&values(json!({ "22": 1000, "17": 12345, "20": 3 })));
        let CapabilityState::PowerMeter(state) = &states[0].1 else {
            panic!("no meter");
        };
        assert_eq!(state.power_w, 100.0);
        assert_eq!(state.energy_wh, Some(123_450.0));
        // `add_ele` alone is never turned into a total.
        let states = Model::new("cz", &plug())
            .unwrap()
            .states(&values(json!({ "22": 0, "20": 3 })));
        let CapabilityState::PowerMeter(state) = &states[0].1 else {
            panic!("no meter");
        };
        assert_eq!(state.energy_wh, None);
    }

    #[test]
    fn values_that_are_missing_or_odd_stay_unknown() {
        let model = Model::new("tdq", &plug()).unwrap();
        assert!(model.states(&values(json!({}))).is_empty());
        assert!(
            model
                .states(&values(json!({ "1": "yes", "22": "abc" })))
                .is_empty()
        );
        let states = model.states(&values(json!({ "22": "125" })));
        assert_eq!(states.len(), 1, "numeric strings are read");
    }

    #[test]
    fn switch_commands_set_exactly_one_data_point() {
        let model = Model::new("tdq", &plug()).unwrap();
        let main = EndpointId::main();
        let none = values(json!({}));
        assert_eq!(
            Value::Object(
                model
                    .plan(&main, &Command::OnOff(OnOffCommand::On), &none)
                    .unwrap()
            ),
            json!({ "1": true })
        );
        assert_eq!(
            Value::Object(
                model
                    .plan(&main, &Command::OnOff(OnOffCommand::Off), &none)
                    .unwrap()
            ),
            json!({ "1": false })
        );
        assert!(matches!(
            model.plan(&main, &Command::OnOff(OnOffCommand::Toggle), &none),
            Err(DriverError::Offline)
        ));
        let on = values(json!({ "1": true }));
        assert_eq!(
            Value::Object(
                model
                    .plan(&main, &Command::OnOff(OnOffCommand::Toggle), &on)
                    .unwrap()
            ),
            json!({ "1": false })
        );
        assert!(matches!(
            model.plan(
                &main,
                &Command::RobotVacuum(RobotVacuumCommand::Start),
                &none
            ),
            Err(DriverError::Unsupported)
        ));
    }

    pub(crate) fn vacuum() -> BTreeMap<String, DataPoint> {
        mapping(json!({
            "1": { "code": "power_go", "type": "Boolean", "values": {} },
            "2": { "code": "pause", "type": "Boolean", "values": {} },
            "3": { "code": "switch_charge", "type": "Boolean", "values": {} },
            "4": { "code": "mode", "type": "Enum", "values": { "range": ["smart", "zone", "pose"] } },
            "5": { "code": "status", "type": "Enum", "values": { "range": ["standby", "zone_clean", "part_clean", "cleaning", "paused", "goto_pos", "pos_arrived", "pos_unarrive", "goto_charge", "charging", "charge_done", "sleep"] } },
            "9": { "code": "suction", "type": "Enum", "values": { "range": ["closed", "gentle", "normal", "strong"] } },
            "16": { "code": "clean_time", "type": "Integer", "values": { "unit": "min", "min": 0, "max": 9999, "scale": 0, "step": 1 } },
            "17": { "code": "clean_area", "type": "Integer", "values": { "unit": "㎡", "min": 0, "max": 9999, "scale": 0, "step": 1 } },
            "23": { "code": "electricity_left", "type": "Integer", "values": { "unit": "%", "min": 0, "max": 100, "scale": 0, "step": 1 } }
        }))
    }

    #[test]
    fn a_robot_vacuum_reports_status_battery_and_its_last_clean() {
        let model = Model::new("sd", &vacuum()).unwrap();
        let endpoints = model.endpoints(false).unwrap();
        let main = &endpoints[0];
        assert_eq!(main.endpoint_type, EndpointType::RobotVacuum);
        assert_eq!(
            main.capabilities.robot_vacuum.as_ref().unwrap().features,
            RobotVacuumFeatures {
                start: true,
                pause: true,
                return_to_dock: true
            }
        );
        assert!(main.capabilities.battery.is_some());

        let states = model.states(&values(json!({
            "5": "charging", "23": 64, "16": 25, "17": 14
        })));
        assert_eq!(
            states,
            vec![
                (
                    EndpointId::main(),
                    RobotVacuumState {
                        status: RobotVacuumStatus::Charging,
                        cleaned_area_m2: Some(14.0),
                        cleaning_duration_s: Some(1500),
                    }
                    .into()
                ),
                (
                    EndpointId::main(),
                    BatteryState {
                        percent: Percent::saturating(64),
                        low: false
                    }
                    .into()
                ),
            ]
        );
        let low = model.states(&values(json!({ "23": 12 })));
        assert_eq!(
            low,
            vec![(
                EndpointId::main(),
                BatteryState {
                    percent: Percent::saturating(12),
                    low: true
                }
                .into()
            )]
        );
    }

    #[test]
    fn every_robot_status_in_plain_categories() {
        let model = Model::new("sd", &vacuum()).unwrap();
        let Model::Vacuum(vacuum) = &model else {
            panic!("not a vacuum");
        };
        let cases = [
            ("standby", RobotVacuumStatus::Idle),
            ("sleep", RobotVacuumStatus::Idle),
            ("zone_clean", RobotVacuumStatus::Cleaning),
            ("part_clean", RobotVacuumStatus::Cleaning),
            ("cleaning", RobotVacuumStatus::Cleaning),
            ("goto_pos", RobotVacuumStatus::Cleaning),
            ("pos_arrived", RobotVacuumStatus::Idle),
            ("pos_unarrive", RobotVacuumStatus::Idle),
            ("paused", RobotVacuumStatus::Paused),
            ("goto_charge", RobotVacuumStatus::Returning),
            ("charging", RobotVacuumStatus::Charging),
            ("charge_done", RobotVacuumStatus::Docked),
        ];
        for (raw, expected) in cases {
            assert_eq!(
                vacuum.status(&values(json!({ "5": raw }))),
                Some(expected),
                "{raw}"
            );
        }
        assert_eq!(vacuum.status(&values(json!({ "5": "dancing" }))), None);
        assert_eq!(vacuum.status(&values(json!({}))), None);
    }

    #[test]
    fn a_fault_overrides_the_status() {
        let mut with_fault = vacuum();
        with_fault.insert(
            "18".to_owned(),
            serde_json::from_value(json!({ "code": "fault", "type": "Bitmap" })).unwrap(),
        );
        let model = Model::new("sd", &with_fault).unwrap();
        let states = model.states(&values(json!({ "5": "cleaning", "18": 4 })));
        let CapabilityState::RobotVacuum(state) = &states[0].1 else {
            panic!("no status");
        };
        assert_eq!(state.status, RobotVacuumStatus::Error);
        let states = model.states(&values(json!({ "5": "cleaning", "18": 0 })));
        let CapabilityState::RobotVacuum(state) = &states[0].1 else {
            panic!("no status");
        };
        assert_eq!(state.status, RobotVacuumStatus::Cleaning);
    }

    #[test]
    fn robot_commands_set_exactly_these_data_points() {
        let model = Model::new("sd", &vacuum()).unwrap();
        let main = EndpointId::main();
        let plan = |cmd: RobotVacuumCommand, now: Value| {
            Value::Object(
                model
                    .plan(&main, &Command::RobotVacuum(cmd), &values(now))
                    .unwrap(),
            )
        };
        assert_eq!(
            plan(RobotVacuumCommand::Start, json!({ "5": "charging" })),
            json!({ "1": true })
        );
        assert_eq!(
            plan(RobotVacuumCommand::Start, json!({ "5": "paused" })),
            json!({ "2": false })
        );
        assert_eq!(
            plan(RobotVacuumCommand::Pause, json!({ "5": "cleaning" })),
            json!({ "2": true })
        );
        assert_eq!(
            plan(RobotVacuumCommand::ReturnToDock, json!({})),
            json!({ "3": true })
        );
        assert!(matches!(
            model.plan(&main, &Command::OnOff(OnOffCommand::On), &values(json!({}))),
            Err(DriverError::Unsupported)
        ));
    }

    #[test]
    fn robots_without_some_data_points_offer_less() {
        let minimal = mapping(json!({
            "1": { "code": "power_go", "type": "Boolean" },
            "4": { "code": "mode", "type": "Enum", "values": { "range": ["smart", "chargego"] } },
            "5": { "code": "status", "type": "Enum" }
        }));
        let model = Model::new("sd", &minimal).unwrap();
        let main = &model.endpoints(false).unwrap()[0];
        assert_eq!(
            main.capabilities.robot_vacuum.as_ref().unwrap().features,
            RobotVacuumFeatures {
                start: true,
                pause: true,
                return_to_dock: true
            }
        );
        assert!(main.capabilities.battery.is_none());
        let plan = |cmd| {
            Value::Object(
                model
                    .plan(
                        &EndpointId::main(),
                        &Command::RobotVacuum(cmd),
                        &values(json!({})),
                    )
                    .unwrap(),
            )
        };
        assert_eq!(plan(RobotVacuumCommand::Pause), json!({ "1": false }));
        assert_eq!(
            plan(RobotVacuumCommand::ReturnToDock),
            json!({ "4": "chargego" })
        );

        let watched = model.endpoints(true).unwrap();
        assert_eq!(
            watched[0]
                .capabilities
                .robot_vacuum
                .as_ref()
                .unwrap()
                .features,
            RobotVacuumFeatures {
                start: false,
                pause: false,
                return_to_dock: false
            }
        );
        let blind = Model::new(
            "sd",
            &mapping(json!({ "5": { "code": "status", "type": "Enum" } })),
        )
        .unwrap();
        assert_eq!(
            blind.endpoints(false).unwrap()[0]
                .capabilities
                .robot_vacuum
                .as_ref()
                .unwrap()
                .features,
            RobotVacuumFeatures {
                start: false,
                pause: false,
                return_to_dock: false
            }
        );
    }
}
