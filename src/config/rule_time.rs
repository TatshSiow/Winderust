use std::{fmt, str::FromStr};

use chrono::NaiveTime;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// A By Time rule's minute-precision local clock time.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RuleTime(NaiveTime);

impl RuleTime {
    pub const fn from_hm(hour: u32, minute: u32) -> Option<Self> {
        match NaiveTime::from_hms_opt(hour, minute, 0) {
            Some(time) => Some(Self(time)),
            None => None,
        }
    }

    pub const fn as_naive_time(self) -> NaiveTime {
        self.0
    }
}

impl FromStr for RuleTime {
    type Err = chrono::ParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        NaiveTime::parse_from_str(value, "%H:%M").map(Self)
    }
}

impl fmt::Display for RuleTime {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.format("%H:%M").fmt(formatter)
    }
}

impl Serialize for RuleTime {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for RuleTime {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parsing_preserves_clock_bounds_and_canonicalizes_minutes() {
        for (input, expected) in [("00:00", "00:00"), ("23:59", "23:59"), ("9:5", "09:05")] {
            let time: RuleTime = input.parse().unwrap();
            assert_eq!(time.to_string(), expected);
            assert_eq!(time.as_naive_time().format("%H:%M").to_string(), expected);
        }
        for input in ["", "25:00", "12:60", "12:30:00", "not a time"] {
            assert!(input.parse::<RuleTime>().is_err(), "{input}");
        }
        assert!(RuleTime::from_hm(24, 0).is_none());
        assert!(RuleTime::from_hm(0, 60).is_none());
    }

    #[test]
    fn serde_keeps_hh_mm_strings_and_rejects_invalid_times() {
        #[derive(Debug, Serialize, Deserialize)]
        struct Clock {
            time: RuleTime,
        }
        let clock: Clock = toml::from_str("time = '9:5'").unwrap();
        let saved = toml::to_string(&clock).unwrap();
        assert_eq!(saved, "time = \"09:05\"\n");
        assert_eq!(toml::from_str::<Clock>(&saved).unwrap().time, clock.time);
        assert!(toml::from_str::<Clock>("time = '25:00'").is_err());
    }
}
