//! The single driver trait and the sink drivers publish into.
//!
//! Data flow on the Hub:
//!
//! ```text
//! driver.run(sink) ──EventSink (mpsc)──▶ device runtime ──EventBus (broadcast)──▶ WebSocket clients
//!                                         └ updates the snapshot cache
//! ```
//!
//! The sink is a bounded `mpsc` so state updates are never dropped before the
//! runtime has recorded them (the runtime must not lag: it is the source of
//! truth for snapshots). Fan-out to clients goes through the lossy
//! [`EventBus`](crate::EventBus), where lagging means "resync".

use async_trait::async_trait;
use tokio::sync::mpsc;

use crate::Timestamp;
use crate::bus::BusEvent;
use crate::capability::CapabilityState;
use crate::command::Command;
use crate::device::DeviceDescriptor;
use crate::event::{EndpointEvent, EventKind};
use crate::id::{DeviceId, EndpointId, Target};
use crate::message::{AvailabilityChange, StateChange};

/// One driver instance controls one device.
#[async_trait]
pub trait DeviceDriver: Send + Sync {
    /// Endpoints, types, capabilities and features. Reported by the driver.
    fn describe(&self) -> DeviceDescriptor;

    /// Execute a typed command on one endpoint. Return
    /// [`DriverError::Unsupported`] for commands the hardware cannot run.
    async fn execute(&self, endpoint: &EndpointId, cmd: Command) -> Result<(), DriverError>;

    /// Driver loop (polling or push subscription). Publishes state, events
    /// and availability through `sink`. Returns when the sink closes.
    async fn run(&self, sink: EventSink) -> Result<(), DriverError>;
}

/// Errors raised by drivers. Details stay on the Hub: clients only receive
/// a [`CommandError`](crate::CommandError) code.
#[derive(Debug, thiserror::Error)]
pub enum DriverError {
    #[error("command not supported by this endpoint")]
    Unsupported,
    /// The device is set up for monitoring only (e.g. `read_only` in its
    /// driver config) and never receives commands.
    #[error("this device is set up as read-only: commands are never sent to it")]
    ReadOnly,
    #[error("unknown endpoint `{0}`")]
    UnknownEndpoint(EndpointId),
    #[error("device is offline")]
    Offline,
    #[error("device did not respond in time")]
    Timeout,
    #[error("invalid driver configuration: {0}")]
    InvalidConfig(String),
    #[error("protocol error: {0}")]
    Protocol(String),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    /// The Hub stopped listening; `run` should return.
    #[error("the hub stopped listening for device updates")]
    SinkClosed,
    #[error(transparent)]
    Other(Box<dyn std::error::Error + Send + Sync + 'static>),
}

impl DriverError {
    pub fn other(error: impl std::error::Error + Send + Sync + 'static) -> Self {
        Self::Other(Box::new(error))
    }
}

/// Where a driver publishes what happens on its device. Bound to one
/// device, so a driver can only report on its own endpoints.
#[derive(Debug, Clone)]
pub struct EventSink {
    device: DeviceId,
    tx: mpsc::Sender<BusEvent>,
}

impl EventSink {
    pub fn new(device: DeviceId, tx: mpsc::Sender<BusEvent>) -> Self {
        Self { device, tx }
    }

    pub fn device(&self) -> &DeviceId {
        &self.device
    }

    pub fn target(&self, endpoint: &EndpointId) -> Target {
        Target::new(self.device.clone(), endpoint.clone())
    }

    /// Publishes the new full state of one capability.
    pub async fn state(
        &self,
        endpoint: &EndpointId,
        state: impl Into<CapabilityState>,
    ) -> Result<(), DriverError> {
        self.send(BusEvent::State(StateChange {
            target: self.target(endpoint),
            state: state.into(),
        }))
        .await
    }

    /// Publishes an event that happened now.
    pub async fn event(&self, endpoint: &EndpointId, event: EventKind) -> Result<(), DriverError> {
        self.event_at(endpoint, event, Timestamp::now()).await
    }

    /// Publishes an event with the device's own timestamp.
    pub async fn event_at(
        &self,
        endpoint: &EndpointId,
        event: EventKind,
        at: Timestamp,
    ) -> Result<(), DriverError> {
        self.send(BusEvent::Event(EndpointEvent {
            target: self.target(endpoint),
            event,
            at,
        }))
        .await
    }

    /// Reports whether the device is reachable.
    pub async fn availability(&self, online: bool) -> Result<(), DriverError> {
        self.send(BusEvent::Availability(AvailabilityChange {
            device_id: self.device.clone(),
            online,
        }))
        .await
    }

    async fn send(&self, event: BusEvent) -> Result<(), DriverError> {
        self.tx
            .send(event)
            .await
            .map_err(|_| DriverError::SinkClosed)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::capability::{Capabilities, OnOffCapability, OnOffState};
    use crate::command::OnOffCommand;
    use crate::device::{Endpoint, EndpointType};

    /// A fake relay: proves the trait is object safe and shows the intended
    /// driver shape.
    struct FakeRelay;

    #[async_trait]
    impl DeviceDriver for FakeRelay {
        fn describe(&self) -> DeviceDescriptor {
            DeviceDescriptor {
                endpoints: vec![Endpoint {
                    id: EndpointId::main(),
                    name: "Relay".to_owned(),
                    endpoint_type: EndpointType::Switch,
                    room: None,
                    capabilities: Capabilities {
                        on_off: Some(OnOffCapability::default()),
                        ..Capabilities::default()
                    },
                }],
            }
        }

        async fn execute(&self, endpoint: &EndpointId, cmd: Command) -> Result<(), DriverError> {
            if endpoint != "main" {
                return Err(DriverError::UnknownEndpoint(endpoint.clone()));
            }
            match cmd {
                Command::OnOff(_) => Ok(()),
                _ => Err(DriverError::Unsupported),
            }
        }

        async fn run(&self, sink: EventSink) -> Result<(), DriverError> {
            sink.availability(true).await?;
            sink.state(&EndpointId::main(), OnOffState { on: true })
                .await?;
            sink.event(&EndpointId::main(), EventKind::OccupancyDetected)
                .await
        }
    }

    #[tokio::test]
    async fn driver_publishes_through_sink() {
        let driver: Arc<dyn DeviceDriver> = Arc::new(FakeRelay);
        let (tx, mut rx) = mpsc::channel(8);
        let sink = EventSink::new(DeviceId::new("hall_relay").unwrap(), tx);

        driver.run(sink).await.unwrap();

        assert_eq!(
            rx.recv().await,
            Some(BusEvent::Availability(AvailabilityChange {
                device_id: DeviceId::new("hall_relay").unwrap(),
                online: true,
            }))
        );
        assert_eq!(
            rx.recv().await,
            Some(BusEvent::State(StateChange {
                target: "hall_relay/main".parse().unwrap(),
                state: OnOffState { on: true }.into(),
            }))
        );
        match rx.recv().await {
            Some(BusEvent::Event(event)) => {
                assert_eq!(event.target.to_string(), "hall_relay/main");
                assert_eq!(event.event, EventKind::OccupancyDetected);
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[tokio::test]
    async fn closed_sink_stops_the_driver() {
        let (tx, rx) = mpsc::channel(1);
        drop(rx);
        let sink = EventSink::new(DeviceId::new("hall_relay").unwrap(), tx);
        assert!(matches!(
            FakeRelay.run(sink).await,
            Err(DriverError::SinkClosed)
        ));
    }

    #[tokio::test]
    async fn execute_routes_by_endpoint() {
        let driver = FakeRelay;
        let main = EndpointId::main();
        assert!(
            driver
                .execute(&main, Command::OnOff(OnOffCommand::On))
                .await
                .is_ok()
        );
        assert!(matches!(
            driver
                .execute(
                    &EndpointId::new("light").unwrap(),
                    Command::OnOff(OnOffCommand::On)
                )
                .await,
            Err(DriverError::UnknownEndpoint(_))
        ));
        assert_eq!(driver.describe().endpoints.len(), 1);
    }
}
