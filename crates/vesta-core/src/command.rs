//! Typed commands (`docs/architecture/capabilities.md`, "Commands").
//!
//! A command targets one endpoint and one capability. Wire shape:
//! `{ "capability": "level", "action": "set", "level": 50, "transition_ms": 300 }`.

use std::fmt;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::capability::{
    CapabilityKind, ChargingSchedule, ChargingWindow, EvChargeMode, FanDirection,
};
use crate::device::EndpointType;
use crate::driver::DriverError;
use crate::id::Target;
use crate::units::{Hue, Percent};

/// A command for one capability of one endpoint.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "capability", rename_all = "snake_case")]
pub enum Command {
    OnOff(OnOffCommand),
    Level(LevelCommand),
    ColorTemperature(ColorTemperatureCommand),
    Color(ColorCommand),
    Cover(CoverCommand),
    Fan(FanCommand),
    Lock(LockCommand),
    Trigger(TriggerCommand),
    EvCharger(EvChargerCommand),
    RobotVacuum(RobotVacuumCommand),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum OnOffCommand {
    On,
    Off,
    Toggle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "action", rename_all = "snake_case")]
#[ts(optional_fields)]
pub enum LevelCommand {
    /// Also turns the endpoint on if it is off.
    Set {
        level: Percent,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        transition_ms: Option<u32>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "action", rename_all = "snake_case")]
#[ts(optional_fields)]
pub enum ColorTemperatureCommand {
    Set {
        kelvin: u16,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        transition_ms: Option<u32>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "action", rename_all = "snake_case")]
#[ts(optional_fields)]
pub enum ColorCommand {
    Set {
        hue: Hue,
        saturation: Percent,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        transition_ms: Option<u32>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum CoverCommand {
    Open,
    Close,
    Stop,
    SetPosition { position: Percent },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum FanCommand {
    SetSpeed { speed: u8 },
    SetDirection { direction: FanDirection },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum LockCommand {
    Lock,
    Unlock,
    Unlatch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum TriggerCommand {
    Fire,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum EvChargerCommand {
    /// Stops charging until `resume` (needs `features.pause`).
    Pause,
    Resume,
    /// The current to offer the car, always clamped to the range the
    /// charger accepts right now (needs `features.adjustable_current`). The
    /// charger's own power control may still lower it.
    SetCurrent {
        amps: f64,
    },
    /// Needs `mode` in `features.modes`.
    SetMode {
        mode: EvChargeMode,
    },
    /// Nobody can charge until `unlock` (needs `features.lock`).
    Lock,
    /// Sensitive: anyone could charge afterwards.
    Unlock,
    /// Switches the charger's own schedule on or off (needs
    /// `features.schedule`).
    SetSchedule {
        enabled: bool,
    },
    /// Stores charging hours in the charger (it keeps following them by
    /// itself) and switches its schedule on or off. Needs
    /// `features.charging_schedule.editable` and windows that fit it
    /// (`ChargingScheduleFeatures::check`). May need the internet
    /// (`features.charging_schedule.saving_needs_internet`), so it may take
    /// longer than other commands ([`Command::is_slow`]).
    SetChargingSchedule {
        enabled: bool,
        windows: Vec<ChargingWindow>,
    },
    /// Reads the charging hours from the charger once, so the screen shows
    /// the ones set on it (e.g. in its own app). Sent when someone asks
    /// (opening the hours editor), never on a timer; the charger may answer
    /// it less often than asked. Needs
    /// `features.charging_schedule.refreshable`; may need the internet, so
    /// it is slow ([`Command::is_slow`]).
    RefreshChargingSchedule,
}

impl EvChargerCommand {
    /// The schedule a `set_charging_schedule` asks for.
    pub fn charging_schedule(&self) -> Option<ChargingSchedule> {
        match self {
            Self::SetChargingSchedule { enabled, windows } => Some(ChargingSchedule {
                enabled: *enabled,
                windows: windows.clone(),
                origin: None,
            }),
            _ => None,
        }
    }
}

/// Robot vacuum commands. Each needs its feature (`features.start`,
/// `features.pause`, `features.return_to_dock`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum RobotVacuumCommand {
    /// Starts cleaning, or carries on after a pause.
    Start,
    Pause,
    /// Sends the robot back to its dock to charge.
    ReturnToDock,
}

impl Command {
    /// The capability this command belongs to.
    pub const fn capability(&self) -> CapabilityKind {
        match self {
            Self::OnOff(_) => CapabilityKind::OnOff,
            Self::Level(_) => CapabilityKind::Level,
            Self::ColorTemperature(_) => CapabilityKind::ColorTemperature,
            Self::Color(_) => CapabilityKind::Color,
            Self::Cover(_) => CapabilityKind::Cover,
            Self::Fan(_) => CapabilityKind::Fan,
            Self::Lock(_) => CapabilityKind::Lock,
            Self::Trigger(_) => CapabilityKind::Trigger,
            Self::EvCharger(_) => CapabilityKind::EvCharger,
            Self::RobotVacuum(_) => CapabilityKind::RobotVacuum,
        }
    }

    /// Whether this command needs an authorized role and explicit user
    /// confirmation (biometric on mobile) before the Hub runs it.
    ///
    /// Sensitive: `lock.unlock`, `lock.unlatch`, `trigger.fire`,
    /// `ev_charger.unlock` (anyone could charge afterwards), and
    /// `cover.open` / `cover.set_position` on endpoints that guard access to
    /// the home (`garage_door`, `gate`, `door`, `lock`). Matches are
    /// exhaustive on purpose: a new command must take a decision here.
    pub const fn is_sensitive(&self, endpoint_type: EndpointType) -> bool {
        match self {
            Self::Lock(LockCommand::Unlock | LockCommand::Unlatch) => true,
            Self::Lock(LockCommand::Lock) => false,
            Self::Trigger(TriggerCommand::Fire) => true,
            Self::Cover(CoverCommand::Open | CoverCommand::SetPosition { .. }) => {
                endpoint_type.guards_access()
            }
            Self::Cover(CoverCommand::Close | CoverCommand::Stop) => false,
            Self::EvCharger(EvChargerCommand::Unlock) => true,
            Self::EvCharger(
                EvChargerCommand::Pause
                | EvChargerCommand::Resume
                | EvChargerCommand::SetCurrent { .. }
                | EvChargerCommand::SetMode { .. }
                | EvChargerCommand::Lock
                | EvChargerCommand::SetSchedule { .. }
                | EvChargerCommand::SetChargingSchedule { .. }
                | EvChargerCommand::RefreshChargingSchedule,
            ) => false,
            Self::OnOff(_)
            | Self::Level(_)
            | Self::ColorTemperature(_)
            | Self::Color(_)
            | Self::Fan(_)
            | Self::RobotVacuum(_) => false,
        }
    }

    /// Whether this command may take longer than the usual few seconds
    /// (the Hub and the screens wait [`SLOW_COMMAND_TIMEOUT`] instead of
    /// 5 s): saving or reading a charger's hours may go through the
    /// internet.
    pub const fn is_slow(&self) -> bool {
        matches!(
            self,
            Self::EvCharger(
                EvChargerCommand::SetChargingSchedule { .. }
                    | EvChargerCommand::RefreshChargingSchedule
            )
        )
    }
}

/// How long a slow command ([`Command::is_slow`]) may take.
pub const SLOW_COMMAND_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(20);

/// Client → Hub request to run a command:
/// `{ "type": "command", "request_id": "c-42", "target": "living_room_fan/light",
///    "command": { ... }, "confirmed": false }`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
pub struct CommandRequest {
    /// Chosen by the client; echoed back in the `command_result`.
    pub request_id: String,
    pub target: Target,
    pub command: Command,
    /// `true` only after the user explicitly confirmed a sensitive command
    /// (biometric on mobile). The Hub rejects sensitive commands without it.
    /// Defaults to `false` when absent.
    #[serde(default)]
    pub confirmed: bool,
}

/// Hub → client outcome of a [`CommandRequest`]. `error` is present exactly
/// when `ok` is `false`. `ok` means the device accepted the command; the
/// pending UI state resolves when the matching `state` message arrives.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(optional_fields)]
pub struct CommandResult {
    pub request_id: String,
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<CommandError>,
}

impl CommandResult {
    pub fn ok(request_id: impl Into<String>) -> Self {
        Self {
            request_id: request_id.into(),
            ok: true,
            error: None,
        }
    }

    pub fn failed(request_id: impl Into<String>, error: CommandError) -> Self {
        Self {
            request_id: request_id.into(),
            ok: false,
            error: Some(error),
        }
    }

    pub fn new(request_id: impl Into<String>, result: Result<(), CommandError>) -> Self {
        match result {
            Ok(()) => Self::ok(request_id),
            Err(error) => Self::failed(request_id, error),
        }
    }

    /// The outcome as a `Result`. A malformed result (`ok: false` without
    /// `error`) reads as a device error.
    pub fn result(&self) -> Result<(), CommandError> {
        if self.ok {
            Ok(())
        } else {
            Err(self
                .error
                .clone()
                .unwrap_or(CommandError::Device { message: None }))
        }
    }
}

/// Why a command failed. Codes are stable; the UI maps them to translated,
/// plain-language messages and never shows `message` raw.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, thiserror::Error)]
#[serde(tag = "code", rename_all = "snake_case")]
#[ts(optional_fields)]
pub enum CommandError {
    /// The endpoint does not have the capability or feature.
    #[error("the endpoint does not support this command")]
    Unsupported,
    #[error("the device is offline")]
    Offline,
    /// No confirmation from the device before the timeout (default 5 s).
    #[error("the device did not confirm the command in time")]
    Timeout,
    #[error("the command was rejected: {reason}")]
    Rejected { reason: RejectReason },
    /// Driver-specific failure. `message` is for diagnostics only and must
    /// never contain hosts, addresses, credentials or driver names.
    #[error("the device reported an error")]
    Device {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        message: Option<String>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum RejectReason {
    /// The client's role does not allow this command on this target (or not
    /// at this time, for time-limited guests).
    NotAllowed,
    /// A sensitive command arrived without `"confirmed": true`. The UI asks
    /// the user to confirm and sends it again.
    ConfirmationRequired,
}

impl fmt::Display for RejectReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::NotAllowed => "not allowed",
            Self::ConfirmationRequired => "confirmation required",
        })
    }
}

impl From<&DriverError> for CommandError {
    /// Driver error details stay on the Hub (they may name hosts or
    /// addresses); clients only get the stable code.
    fn from(error: &DriverError) -> Self {
        match error {
            DriverError::Unsupported | DriverError::ReadOnly | DriverError::UnknownEndpoint(_) => {
                Self::Unsupported
            }
            DriverError::Offline => Self::Offline,
            DriverError::Timeout => Self::Timeout,
            DriverError::InvalidConfig(_)
            | DriverError::Protocol(_)
            | DriverError::Io(_)
            | DriverError::SinkClosed
            | DriverError::Other(_) => Self::Device { message: None },
        }
    }
}

impl From<DriverError> for CommandError {
    fn from(error: DriverError) -> Self {
        Self::from(&error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL_TYPES: [EndpointType; 15] = [
        EndpointType::Light,
        EndpointType::Outlet,
        EndpointType::Switch,
        EndpointType::Fan,
        EndpointType::WindowCovering,
        EndpointType::GarageDoor,
        EndpointType::Gate,
        EndpointType::Door,
        EndpointType::Lock,
        EndpointType::Camera,
        EndpointType::Doorbell,
        EndpointType::Sensor,
        EndpointType::Meter,
        EndpointType::EvCharger,
        EndpointType::RobotVacuum,
    ];

    fn access_types() -> Vec<EndpointType> {
        vec![
            EndpointType::GarageDoor,
            EndpointType::Gate,
            EndpointType::Door,
            EndpointType::Lock,
        ]
    }

    #[test]
    fn lock_and_trigger_sensitivity_is_independent_of_type() {
        for ty in ALL_TYPES {
            assert!(Command::Lock(LockCommand::Unlock).is_sensitive(ty));
            assert!(Command::Lock(LockCommand::Unlatch).is_sensitive(ty));
            assert!(Command::Trigger(TriggerCommand::Fire).is_sensitive(ty));
            assert!(!Command::Lock(LockCommand::Lock).is_sensitive(ty));
        }
    }

    #[test]
    fn opening_covers_is_sensitive_only_on_access_endpoints() {
        let opening = [
            Command::Cover(CoverCommand::Open),
            Command::Cover(CoverCommand::SetPosition {
                position: Percent::saturating(50),
            }),
        ];
        let not_opening = [
            Command::Cover(CoverCommand::Close),
            Command::Cover(CoverCommand::Stop),
        ];
        for ty in ALL_TYPES {
            let access = access_types().contains(&ty);
            for cmd in &opening {
                assert_eq!(cmd.is_sensitive(ty), access, "{cmd:?} on {ty:?}");
            }
            for cmd in &not_opening {
                assert!(!cmd.is_sensitive(ty), "{cmd:?} on {ty:?}");
            }
        }
        assert!(!Command::Cover(CoverCommand::Open).is_sensitive(EndpointType::WindowCovering));
        assert!(Command::Cover(CoverCommand::Open).is_sensitive(EndpointType::GarageDoor));
        assert!(Command::Cover(CoverCommand::Open).is_sensitive(EndpointType::Gate));
    }

    #[test]
    fn everyday_commands_are_not_sensitive() {
        let commands = [
            Command::OnOff(OnOffCommand::Toggle),
            Command::Level(LevelCommand::Set {
                level: Percent::MAX,
                transition_ms: None,
            }),
            Command::ColorTemperature(ColorTemperatureCommand::Set {
                kelvin: 3000,
                transition_ms: None,
            }),
            Command::Fan(FanCommand::SetSpeed { speed: 3 }),
            Command::EvCharger(EvChargerCommand::Pause),
            Command::EvCharger(EvChargerCommand::Resume),
            Command::EvCharger(EvChargerCommand::SetCurrent { amps: 16.0 }),
            Command::EvCharger(EvChargerCommand::SetMode {
                mode: EvChargeMode::Solar,
            }),
            Command::EvCharger(EvChargerCommand::Lock),
            Command::EvCharger(EvChargerCommand::SetSchedule { enabled: false }),
            Command::RobotVacuum(RobotVacuumCommand::Start),
            Command::RobotVacuum(RobotVacuumCommand::Pause),
            Command::RobotVacuum(RobotVacuumCommand::ReturnToDock),
        ];
        for ty in ALL_TYPES {
            for cmd in &commands {
                assert!(!cmd.is_sensitive(ty), "{cmd:?} on {ty:?}");
            }
        }
    }

    #[test]
    fn unlocking_a_charger_is_sensitive() {
        for ty in ALL_TYPES {
            assert!(Command::EvCharger(EvChargerCommand::Unlock).is_sensitive(ty));
        }
    }

    #[test]
    fn driver_errors_do_not_leak_details() {
        let err = DriverError::Protocol("connection refused by 192.0.2.10".to_owned());
        assert_eq!(
            CommandError::from(&err),
            CommandError::Device { message: None }
        );
        assert_eq!(
            CommandError::from(DriverError::Offline),
            CommandError::Offline
        );
        assert_eq!(
            CommandError::from(DriverError::Unsupported),
            CommandError::Unsupported
        );
    }

    #[test]
    fn result_accessors() {
        assert_eq!(CommandResult::ok("c-1").result(), Ok(()));
        assert_eq!(
            CommandResult::new("c-2", Err(CommandError::Timeout)).result(),
            Err(CommandError::Timeout)
        );
        let malformed = CommandResult {
            request_id: "c-3".to_owned(),
            ok: false,
            error: None,
        };
        assert_eq!(
            malformed.result(),
            Err(CommandError::Device { message: None })
        );
    }
}
