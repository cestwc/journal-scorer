use serde::{Deserialize, Serialize};

use super::{CriterionId, Subitem};

/// One independently scored rubric criterion (0..=criterion_max).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Criterion {
    pub id: CriterionId,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub subitems: Vec<Subitem>,
}
