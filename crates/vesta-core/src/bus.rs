//! In-process fan-out of device updates, over `tokio::sync::broadcast`.
//!
//! The bus is lossy by design: a slow subscriber (e.g. a WebSocket client on
//! a flaky connection) that falls behind receives [`BusRecv::Resync`] and
//! must be sent a full snapshot instead of the missed updates.

use tokio::sync::broadcast::{self, error::RecvError};

use crate::event::EndpointEvent;
use crate::message::{AvailabilityChange, ServerMessage, StateChange};

/// A device update travelling from drivers to clients.
#[derive(Debug, Clone, PartialEq)]
pub enum BusEvent {
    State(StateChange),
    Event(EndpointEvent),
    Availability(AvailabilityChange),
}

impl From<BusEvent> for ServerMessage {
    fn from(value: BusEvent) -> Self {
        match value {
            BusEvent::State(state) => Self::State(state),
            BusEvent::Event(event) => Self::Event(event),
            BusEvent::Availability(availability) => Self::Availability(availability),
        }
    }
}

/// What a subscriber gets from [`Subscription::recv`].
#[derive(Debug, Clone, PartialEq)]
pub enum BusRecv {
    Event(BusEvent),
    /// Updates were missed: send the client a full snapshot.
    Resync,
}

#[derive(Debug, Clone)]
pub struct EventBus {
    tx: broadcast::Sender<BusEvent>,
}

impl EventBus {
    pub const DEFAULT_CAPACITY: usize = 1024;
    /// Upper bound accepted by [`EventBus::new`].
    pub const MAX_CAPACITY: usize = 1 << 16;

    /// Creates a bus buffering up to `capacity` updates per subscriber
    /// (clamped to `1..=MAX_CAPACITY`).
    pub fn new(capacity: usize) -> Self {
        let (tx, _) = broadcast::channel(capacity.clamp(1, Self::MAX_CAPACITY));
        Self { tx }
    }

    /// Publishes an update to every current subscriber. Returns how many
    /// received it; zero subscribers is not an error.
    pub fn publish(&self, event: BusEvent) -> usize {
        self.tx.send(event).unwrap_or(0)
    }

    pub fn subscribe(&self) -> Subscription {
        Subscription {
            rx: self.tx.subscribe(),
        }
    }

    pub fn subscriber_count(&self) -> usize {
        self.tx.receiver_count()
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new(Self::DEFAULT_CAPACITY)
    }
}

#[derive(Debug)]
pub struct Subscription {
    rx: broadcast::Receiver<BusEvent>,
}

impl Subscription {
    /// Next update, [`BusRecv::Resync`] after lagging, or `None` once the
    /// bus has been dropped.
    pub async fn recv(&mut self) -> Option<BusRecv> {
        match self.rx.recv().await {
            Ok(event) => Some(BusRecv::Event(event)),
            Err(RecvError::Lagged(_)) => Some(BusRecv::Resync),
            Err(RecvError::Closed) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::id::DeviceId;

    fn availability(online: bool) -> BusEvent {
        BusEvent::Availability(AvailabilityChange {
            device_id: DeviceId::new("hall_relay").unwrap(),
            online,
        })
    }

    #[tokio::test]
    async fn delivers_to_every_subscriber() {
        let bus = EventBus::default();
        assert_eq!(bus.publish(availability(true)), 0);

        let mut a = bus.subscribe();
        let mut b = bus.subscribe();
        assert_eq!(bus.subscriber_count(), 2);
        assert_eq!(bus.publish(availability(true)), 2);
        assert_eq!(a.recv().await, Some(BusRecv::Event(availability(true))));
        assert_eq!(b.recv().await, Some(BusRecv::Event(availability(true))));
    }

    #[tokio::test]
    async fn lagging_subscriber_is_told_to_resync() {
        let bus = EventBus::new(1);
        let mut sub = bus.subscribe();
        bus.publish(availability(true));
        bus.publish(availability(false));
        bus.publish(availability(true));

        assert_eq!(sub.recv().await, Some(BusRecv::Resync));
        assert_eq!(sub.recv().await, Some(BusRecv::Event(availability(true))));
    }

    #[tokio::test]
    async fn closed_bus_ends_the_subscription() {
        let bus = EventBus::new(0);
        let mut sub = bus.subscribe();
        drop(bus);
        assert_eq!(sub.recv().await, None);
    }

    #[test]
    fn bus_event_maps_to_server_message() {
        assert!(matches!(
            ServerMessage::from(availability(false)),
            ServerMessage::Availability(AvailabilityChange { online: false, .. })
        ));
    }
}
