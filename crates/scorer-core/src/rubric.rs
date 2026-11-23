//! Rubric definition, loading and validation.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::model::{Criterion, StrengthWeights, Subitem, SubitemId};
use crate::CoreError;

const BUILTIN_AI_JOURNAL: &str = include_str!("../rubrics/ai_journal_v1.toml");

fn default_criterion_max() -> f64 {
    5.0
}

/// A complete rubric: criteria, their subitems, and scoring constants.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Rubric {
    pub id: String,
    pub name: String,
    pub version: String,
    /// Maximum score of each criterion.
    #[serde(default = "default_criterion_max")]
    pub criterion_max: f64,
    #[serde(default)]
    pub weights: StrengthWeights,
    pub criteria: Vec<Criterion>,
}

impl Rubric {
    /// Parses and validates a rubric from TOML.
    pub fn from_toml_str(s: &str) -> Result<Self, CoreError> {
        let rubric: Rubric = toml::from_str(s)?;
        rubric.validate()?;
        Ok(rubric)
    }

    /// The built-in AI Journal Reflection rubric.
    pub fn builtin_ai_journal() -> Self {
        Self::from_toml_str(BUILTIN_AI_JOURNAL).expect("built-in rubric must be valid")
    }

    pub fn validate(&self) -> Result<(), CoreError> {
        let invalid = |m: String| Err(CoreError::RubricInvalid(m));
        self.weights.validate()?;
        if !(self.criterion_max > 0.0) {
            return invalid("criterion_max must be positive".into());
        }
        if self.criteria.is_empty() {
            return invalid("rubric has no criteria".into());
        }
        let mut criterion_ids = HashSet::new();
        let mut subitem_ids = HashSet::new();
        for c in &self.criteria {
            if !criterion_ids.insert(c.id.as_str().to_ascii_lowercase()) {
                return invalid(format!("duplicate criterion id `{}`", c.id));
            }
            if c.subitems.is_empty() {
                return invalid(format!("criterion `{}` has no subitems", c.id));
            }
            for s in &c.subitems {
                if !subitem_ids.insert(s.id.as_str().to_ascii_lowercase()) {
                    return invalid(format!("duplicate subitem id `{}`", s.id));
                }
            }
        }
        Ok(())
    }

    pub fn find_subitem(&self, id: &SubitemId) -> Option<(&Criterion, &Subitem)> {
        self.resolve_subitem(id.as_str())
    }

    /// Case-insensitive, whitespace-tolerant lookup, for model output.
    pub fn resolve_subitem(&self, raw: &str) -> Option<(&Criterion, &Subitem)> {
        let raw = raw.trim();
        self.criteria.iter().find_map(|c| {
            c.subitems
                .iter()
                .find(|s| s.id.as_str().eq_ignore_ascii_case(raw))
                .map(|s| (c, s))
        })
    }

    pub fn subitem_count(&self) -> usize {
        self.criteria.iter().map(|c| c.subitems.len()).sum()
    }

    pub fn max_total(&self) -> f64 {
        self.criterion_max * self.criteria.len() as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_rubric_shape() {
        let r = Rubric::builtin_ai_journal();
        assert_eq!(r.criteria.len(), 5);
        assert_eq!(r.subitem_count(), 24);
        assert_eq!(r.max_total(), 25.0);
        assert!(r.resolve_subitem(" e1 ").is_some());
    }

    #[test]
    fn rejects_duplicate_subitem() {
        let toml = r#"
            id = "x"
            name = "x"
            version = "1"
            [[criteria]]
            id = "A"
            name = "A"
            [[criteria.subitems]]
            id = "A1"
            name = "a"
            weak = "w"
            moderate = "m"
            strong = "s"
            [[criteria.subitems]]
            id = "a1"
            name = "b"
            weak = "w"
            moderate = "m"
            strong = "s"
        "#;
        assert!(matches!(Rubric::from_toml_str(toml), Err(CoreError::RubricInvalid(_))));
    }
}
