use serde::{Deserialize, Serialize};

use super::{CriterionId, EvidenceId, Strength, SubitemId};

/// One evidence item's contribution to a class score.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Contribution {
    pub evidence_id: EvidenceId,
    pub strength: Strength,
    pub weight: f64,
}

/// Saturated score of one subitem, in [0, 1].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ClassScore {
    pub subitem: SubitemId,
    pub name: String,
    pub score: f64,
    pub contributions: Vec<Contribution>,
}

/// Score of one criterion, in [0, max].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CriterionScore {
    pub criterion: CriterionId,
    pub name: String,
    pub score: f64,
    pub max: f64,
    /// Sum of class scores before normalisation.
    pub raw: f64,
    pub classes: Vec<ClassScore>,
    /// Deterministic, evidence-based explanation of the score.
    pub rationale: String,
}

/// Full deterministic scoring outcome for one document.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScoreResult {
    pub rubric_id: String,
    pub rubric_version: String,
    pub criteria: Vec<CriterionScore>,
    pub total: f64,
    pub max_total: f64,
}
