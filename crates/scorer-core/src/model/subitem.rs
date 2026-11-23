use serde::{Deserialize, Serialize};

use super::{Strength, SubitemId};

/// What weak / moderate / strong evidence looks like for one subitem.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LevelDescriptors {
    pub weak: String,
    pub moderate: String,
    pub strong: String,
}

impl LevelDescriptors {
    pub fn get(&self, strength: Strength) -> &str {
        match strength {
            Strength::Weak => &self.weak,
            Strength::Moderate => &self.moderate,
            Strength::Strong => &self.strong,
        }
    }
}

/// A rubric support class within a criterion, e.g. "verification behavior".
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Subitem {
    pub id: SubitemId,
    pub name: String,
    #[serde(flatten)]
    pub levels: LevelDescriptors,
}
