//! Events: instantaneous facts, not state.

use std::fmt;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::Timestamp;
use crate::capability::CapabilityKind;
use crate::id::Target;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
pub enum EventKind {
    #[serde(rename = "doorbell.pressed")]
    DoorbellPressed,
    #[serde(rename = "occupancy.detected")]
    OccupancyDetected,
}

impl EventKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::DoorbellPressed => "doorbell.pressed",
            Self::OccupancyDetected => "occupancy.detected",
        }
    }

    /// The capability that emits this event.
    pub const fn capability(self) -> CapabilityKind {
        match self {
            Self::DoorbellPressed => CapabilityKind::Doorbell,
            Self::OccupancyDetected => CapabilityKind::Occupancy,
        }
    }
}

impl fmt::Display for EventKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// An event on one endpoint:
/// `{ "target": "front_doorbell/main", "event": "doorbell.pressed", "at": "2026-01-01T12:00:00Z" }`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
pub struct EndpointEvent {
    pub target: Target,
    pub event: EventKind,
    /// RFC 3339 timestamp.
    pub at: Timestamp,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn names_match_serde() {
        for kind in [EventKind::DoorbellPressed, EventKind::OccupancyDetected] {
            assert_eq!(serde_json::to_value(kind).unwrap(), json!(kind.as_str()));
        }
    }

    #[test]
    fn at_is_rfc3339() {
        let event: EndpointEvent = serde_json::from_value(json!({
            "target": "front_doorbell/main",
            "event": "doorbell.pressed",
            "at": "2026-01-01T13:00:00+01:00"
        }))
        .unwrap();
        assert_eq!(
            serde_json::to_value(&event).unwrap()["at"],
            json!("2026-01-01T12:00:00Z")
        );
        assert!(
            serde_json::from_value::<EndpointEvent>(json!({
                "target": "front_doorbell/main",
                "event": "doorbell.pressed",
                "at": "yesterday"
            }))
            .is_err()
        );
    }
}
