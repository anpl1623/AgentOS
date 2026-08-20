//! Risk classification for actions the runtime may take.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::error::CoreError;

/// How consequential an action is.
///
/// Ordering is meaningful: `None < Low < Medium < High < Critical`. The runtime
/// compares a tool's risk against policy ceilings and against the taint floor
/// raised by untrusted input, so the ordering is load-bearing, not cosmetic.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Default,
)]
#[serde(rename_all = "snake_case")]
pub enum RiskLevel {
    /// Pure computation with no observable side effect.
    #[default]
    None,
    /// Reads local, in-scope state.
    Low,
    /// Writes local state, or reads from the network.
    Medium,
    /// Destructive locally, or externally visible (sending, publishing).
    High,
    /// Irreversible and externally visible (payments, production changes).
    Critical,
}

impl RiskLevel {
    /// All levels, ascending.
    pub const ALL: [Self; 5] = [
        Self::None,
        Self::Low,
        Self::Medium,
        Self::High,
        Self::Critical,
    ];

    /// Stable wire representation.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::Critical => "critical",
        }
    }

    /// The higher of two levels.
    #[must_use]
    pub fn max_of(self, other: Self) -> Self {
        if self >= other { self } else { other }
    }
}

impl fmt::Display for RiskLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for RiskLevel {
    type Err = CoreError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "none" => Ok(Self::None),
            "low" => Ok(Self::Low),
            "medium" | "med" => Ok(Self::Medium),
            "high" => Ok(Self::High),
            "critical" | "crit" => Ok(Self::Critical),
            other => Err(CoreError::UnknownVariant {
                kind: "risk level",
                value: other.to_owned(),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn orders_ascending() {
        assert!(RiskLevel::None < RiskLevel::Low);
        assert!(RiskLevel::Low < RiskLevel::Medium);
        assert!(RiskLevel::Medium < RiskLevel::High);
        assert!(RiskLevel::High < RiskLevel::Critical);
    }

    #[test]
    fn max_of_picks_higher() {
        assert_eq!(
            RiskLevel::Low.max_of(RiskLevel::Critical),
            RiskLevel::Critical
        );
        assert_eq!(RiskLevel::High.max_of(RiskLevel::None), RiskLevel::High);
    }

    #[test]
    fn parses_every_variant_it_prints() {
        for level in RiskLevel::ALL {
            assert_eq!(level.as_str().parse::<RiskLevel>().unwrap(), level);
        }
    }
}
