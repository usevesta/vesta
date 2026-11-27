//! Range-checked units shared by capabilities and commands.
//!
//! Values are validated when deserialized, so a command such as
//! `{ "level": 150 }` never reaches a driver.

use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use ts_rs::TS;

/// A value outside its documented range.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("{value} is outside {min}..={max}")]
pub struct OutOfRange {
    pub value: u32,
    pub min: u32,
    pub max: u32,
}

/// Integer percentage, `0..=100`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, TS)]
pub struct Percent(u8);

impl Percent {
    pub const MIN: Self = Self(0);
    pub const MAX: Self = Self(100);

    pub const fn new(value: u8) -> Result<Self, OutOfRange> {
        if value <= 100 {
            Ok(Self(value))
        } else {
            Err(OutOfRange {
                value: value as u32,
                min: 0,
                max: 100,
            })
        }
    }

    /// Clamps `value` to `0..=100`.
    pub const fn saturating(value: u8) -> Self {
        if value > 100 { Self(100) } else { Self(value) }
    }

    pub const fn get(self) -> u8 {
        self.0
    }
}

impl TryFrom<u8> for Percent {
    type Error = OutOfRange;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<Percent> for u8 {
    fn from(value: Percent) -> Self {
        value.0
    }
}

impl fmt::Display for Percent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}%", self.0)
    }
}

impl Serialize for Percent {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_u8(self.0)
    }
}

impl<'de> Deserialize<'de> for Percent {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::new(u8::deserialize(deserializer)?).map_err(de::Error::custom)
    }
}

/// Hue in degrees, `0..=360`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, TS)]
pub struct Hue(u16);

impl Hue {
    pub const fn new(degrees: u16) -> Result<Self, OutOfRange> {
        if degrees <= 360 {
            Ok(Self(degrees))
        } else {
            Err(OutOfRange {
                value: degrees as u32,
                min: 0,
                max: 360,
            })
        }
    }

    pub const fn get(self) -> u16 {
        self.0
    }
}

impl TryFrom<u16> for Hue {
    type Error = OutOfRange;

    fn try_from(value: u16) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<Hue> for u16 {
    fn from(value: Hue) -> Self {
        value.0
    }
}

impl Serialize for Hue {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_u16(self.0)
    }
}

impl<'de> Deserialize<'de> for Hue {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::new(u16::deserialize(deserializer)?).map_err(de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_range() {
        assert_eq!(Percent::new(0).unwrap().get(), 0);
        assert_eq!(Percent::new(100).unwrap().get(), 100);
        assert!(Percent::new(101).is_err());
        assert_eq!(Percent::saturating(250), Percent::MAX);
        assert!(serde_json::from_str::<Percent>("101").is_err());
        assert!(serde_json::from_str::<Percent>("-1").is_err());
        assert!(serde_json::from_str::<Percent>("50.5").is_err());
        assert_eq!(serde_json::from_str::<Percent>("42").unwrap().get(), 42);
        assert_eq!(
            serde_json::to_string(&Percent::saturating(42)).unwrap(),
            "42"
        );
    }

    #[test]
    fn hue_range() {
        assert!(Hue::new(360).is_ok());
        assert!(Hue::new(361).is_err());
        assert!(serde_json::from_str::<Hue>("361").is_err());
        assert_eq!(serde_json::from_str::<Hue>("200").unwrap().get(), 200);
    }
}
