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

/// A time of day that is not written `HH:MM` (`00:00`–`23:59`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("a time of day is written HH:MM, from 00:00 to 23:59")]
pub struct InvalidTimeOfDay;

/// A time of day on a 24-hour clock, in the home's local time. On the wire
/// it is `"HH:MM"` (`"00:00"`–`"23:59"`); inside, minutes past midnight
/// (Matter: `MinutesPastMidnight`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, TS)]
pub struct TimeOfDay(#[ts(type = "string")] u16);

impl TimeOfDay {
    pub const MIDNIGHT: Self = Self(0);
    const MINUTES_PER_DAY: u16 = 24 * 60;

    pub const fn new(hour: u8, minute: u8) -> Result<Self, InvalidTimeOfDay> {
        if hour < 24 && minute < 60 {
            Ok(Self(hour as u16 * 60 + minute as u16))
        } else {
            Err(InvalidTimeOfDay)
        }
    }

    /// From minutes past midnight, `0..1440`.
    pub const fn from_minutes(minutes: u16) -> Result<Self, InvalidTimeOfDay> {
        if minutes < Self::MINUTES_PER_DAY {
            Ok(Self(minutes))
        } else {
            Err(InvalidTimeOfDay)
        }
    }

    /// Minutes past midnight, `0..1440`.
    pub const fn minutes(self) -> u16 {
        self.0
    }

    pub const fn hour(self) -> u8 {
        (self.0 / 60) as u8
    }

    pub const fn minute(self) -> u8 {
        (self.0 % 60) as u8
    }
}

impl fmt::Display for TimeOfDay {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:02}:{:02}", self.hour(), self.minute())
    }
}

impl std::str::FromStr for TimeOfDay {
    type Err = InvalidTimeOfDay;

    /// Exactly `HH:MM`, two digits each.
    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let bytes = text.as_bytes();
        if bytes.len() != 5 || bytes.get(2) != Some(&b':') {
            return Err(InvalidTimeOfDay);
        }
        let digit = |i: usize| match bytes.get(i) {
            Some(b) if b.is_ascii_digit() => Ok(b - b'0'),
            _ => Err(InvalidTimeOfDay),
        };
        Self::new(digit(0)? * 10 + digit(1)?, digit(3)? * 10 + digit(4)?)
    }
}

impl Serialize for TimeOfDay {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for TimeOfDay {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = <std::borrow::Cow<'de, str>>::deserialize(deserializer)?;
        text.parse().map_err(de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn time_of_day_is_hh_mm() {
        let time: TimeOfDay = serde_json::from_str("\"22:30\"").unwrap();
        assert_eq!((time.hour(), time.minute(), time.minutes()), (22, 30, 1350));
        assert_eq!(serde_json::to_string(&time).unwrap(), "\"22:30\"");
        assert_eq!(TimeOfDay::new(6, 5).unwrap().to_string(), "06:05");
        assert_eq!("00:00".parse::<TimeOfDay>(), Ok(TimeOfDay::MIDNIGHT));
        assert_eq!("23:59".parse::<TimeOfDay>().unwrap().minutes(), 1439);
        for bad in [
            "24:00",
            "12:60",
            "7:00",
            "07:0",
            "0700",
            "07-00",
            "",
            "ab:cd",
            "07:00:00",
            " 07:00",
            "+1:00",
            "٠٧:٠٠",
        ] {
            assert!(bad.parse::<TimeOfDay>().is_err(), "{bad}");
        }
        assert!(serde_json::from_str::<TimeOfDay>("420").is_err());
        assert!(TimeOfDay::from_minutes(1440).is_err());
        assert!(TimeOfDay::new(24, 0).is_err());
    }

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
