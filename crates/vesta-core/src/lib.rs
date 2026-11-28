//! VESTA domain model.
//!
//! Hardware is described as **Device → Endpoint → Capability** (see
//! `docs/architecture/capabilities.md`). Everything here is plain, serializable
//! data: capabilities, their static `features` and dynamic `state`, typed
//! commands, events, WebSocket messages and the versioned config/dashboard
//! schemas. Brands never appear in this crate; they live in `crates/drivers/*`.
//!
//! Wire types derive [`ts_rs::TS`] and are exported to
//! `web/src/api/generated/` by `cargo test -p vesta-core`. [`HubConfig`] is
//! deliberately **not** exported: it holds driver config (hosts, keys,
//! passwords) that must never reach a client.

pub mod bus;
pub mod capability;
pub mod command;
pub mod config;
pub mod dashboard;
pub mod device;
pub mod driver;
pub mod event;
pub mod id;
pub mod message;
pub mod units;

pub use async_trait::async_trait;
pub use jiff::Timestamp;

pub use bus::{BusEvent, BusRecv, EventBus, Subscription};
pub use capability::{
    BatteryCapability, BatteryState, CameraCapability, CameraFeatures, CameraState, Capabilities,
    CapabilityKind, CapabilityState, ColorCapability, ColorState, ColorTemperatureCapability,
    ColorTemperatureFeatures, ColorTemperatureState, ContactCapability, ContactState,
    CoverCapability, CoverFeatures, CoverKind, CoverMotion, CoverState, DoorbellCapability,
    EvChargeMode, EvChargerCapability, EvChargerFeatures, EvChargerState, EvChargerStatus,
    EvChargerWaitReason, FanCapability, FanDirection, FanFeatures, FanState, HumidityCapability,
    HumidityState, LevelCapability, LevelState, LockCapability, LockFeatures, LockState,
    LockStatus, MeterRole, OccupancyCapability, OccupancyState, OnOffCapability, OnOffFeatures,
    OnOffState, PowerMeterCapability, PowerMeterFeatures, PowerMeterState, RobotVacuumCapability,
    RobotVacuumFeatures, RobotVacuumState, RobotVacuumStatus, TemperatureCapability,
    TemperatureState, TriggerCapability,
};
pub use command::{
    ColorCommand, ColorTemperatureCommand, Command, CommandError, CommandRequest, CommandResult,
    CoverCommand, EvChargerCommand, FanCommand, LevelCommand, LockCommand, OnOffCommand,
    RejectReason, RobotVacuumCommand, TriggerCommand,
};
pub use config::{
    CONFIG_SCHEMA_VERSION, ConfigError, DeviceConfig, EndpointOverride, Go2rtcSettings, HubConfig,
    HubSettings, NotificationsConfig,
};
pub use dashboard::{
    DASHBOARD_SCHEMA_VERSION, Dashboard, DoorbellOverlay, EventRef, JsonObject, Kiosk,
    MAX_GRID_COLUMNS, MAX_PAGES, MAX_ROW_SPAN, MAX_TITLE_CHARS, MAX_WIDGET_TYPE_LEN,
    MAX_WIDGETS_PER_PAGE, Page, Theme, TouchScale, Widget,
};
pub use device::{Device, DeviceDescriptor, Endpoint, EndpointType};
pub use driver::{DeviceDriver, DriverError, EventSink};
pub use event::{EndpointEvent, EventKind};
pub use id::{DeviceId, EndpointId, IdError, RoomId, Target};
pub use message::{
    AvailabilityChange, ClientMessage, DashboardChanged, ServerMessage, Snapshot, StateChange,
};
pub use units::{Hue, OutOfRange, Percent};
