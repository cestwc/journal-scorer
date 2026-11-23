use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::CoreError;

/// How strongly one evidence span supports one subitem.
///
/// Ordered: `Weak < Moderate < Strong`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Strength {
    Weak,
    Moderate,
    Strong,
}

impl Strength {
    pub const ALL: [Strength; 3] = [Strength::Weak, Strength::Moderate, Strength::Strong];

    pub fn as_str(self) -> &'static str {
        match self {
            Strength::Weak => "weak",
            Strength::Moderate => "moderate",
            Strength::Strong => "strong",
        }
    }
}

impl fmt::Display for Strength {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Strength {
    type Err = CoreError;

    /// Case-insensitive; also accepts the abbreviations `w`, `m`, `s`.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "weak" | "w" => Ok(Strength::Weak),
            "moderate" | "medium" | "m" => Ok(Strength::Moderate),
            "strong" | "s" => Ok(Strength::Strong),
            _ => Err(CoreError::UnknownStrength(s.to_owned())),
        }
    }
}

/// Numeric contribution of each strength level to a class score.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct StrengthWeights {
    pub weak: f64,
    pub moderate: f64,
    pub strong: f64,
}

impl Default for StrengthWeights {
    fn default() -> Self {
        Self { weak: 0.25, moderate: 0.60, strong: 1.00 }
    }
}

impl StrengthWeights {
    pub fn weight(&self, strength: Strength) -> f64 {
        match strength {
            Strength::Weak => self.weak,
            Strength::Moderate => self.moderate,
            Strength::Strong => self.strong,
        }
    }

    pub fn validate(&self) -> Result<(), CoreError> {
        for s in Strength::ALL {
            let w = self.weight(s);
            if !(w > 0.0 && w <= 1.0) {
                return Err(CoreError::RubricInvalid(format!(
                    "weight for `{s}` must be in (0, 1], got {w}"
                )));
            }
        }
        if !(self.weak <= self.moderate && self.moderate <= self.strong) {
            return Err(CoreError::RubricInvalid(
                "weights must satisfy weak <= moderate <= strong".into(),
            ));
        }
        Ok(())
    }
}
