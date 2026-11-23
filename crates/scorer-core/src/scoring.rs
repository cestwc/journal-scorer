//! Deterministic aggregation (grade.md §6):
//!
//! * class score     = 1 - Π(1 - w_i)            (saturating)
//! * criterion score = max * Σ class_scores / N
//! * total           = Σ criterion scores

use crate::model::{
    ClassScore, Contribution, Criterion, CriterionScore, Evidence, ScoreResult, Strength, Subitem,
};
use crate::Rubric;

/// Saturating combination of contribution weights; result in [0, 1].
pub fn saturate<I: IntoIterator<Item = f64>>(weights: I) -> f64 {
    1.0 - weights
        .into_iter()
        .fold(1.0, |acc, w| acc * (1.0 - w.clamp(0.0, 1.0)))
}

/// Scores a set of evidence against a rubric. Supports referencing subitems
/// that are not in the rubric are ignored.
pub fn score(rubric: &Rubric, evidence: &[Evidence]) -> ScoreResult {
    let criteria: Vec<CriterionScore> = rubric
        .criteria
        .iter()
        .map(|c| score_criterion(rubric, c, evidence))
        .collect();
    let total = criteria.iter().map(|c| c.score).sum();
    ScoreResult {
        rubric_id: rubric.id.clone(),
        rubric_version: rubric.version.clone(),
        criteria,
        total: round4(total),
        max_total: rubric.max_total(),
    }
}

fn score_criterion(rubric: &Rubric, criterion: &Criterion, evidence: &[Evidence]) -> CriterionScore {
    let classes: Vec<ClassScore> = criterion
        .subitems
        .iter()
        .map(|s| score_class(rubric, s, evidence))
        .collect();
    let raw: f64 = classes.iter().map(|c| c.score).sum();
    let score = rubric.criterion_max * raw / classes.len() as f64;
    CriterionScore {
        criterion: criterion.id.clone(),
        name: criterion.name.clone(),
        score: round4(score),
        max: rubric.criterion_max,
        raw: round4(raw),
        rationale: rationale(&classes),
        classes,
    }
}

fn score_class(rubric: &Rubric, subitem: &Subitem, evidence: &[Evidence]) -> ClassScore {
    // One contribution per evidence item: if an item somehow carries the same
    // subitem twice, only its strongest support counts.
    let contributions: Vec<Contribution> = evidence
        .iter()
        .filter_map(|e| {
            e.supports
                .iter()
                .filter(|s| s.subitem == subitem.id)
                .map(|s| s.strength)
                .max()
                .map(|strength| Contribution {
                    evidence_id: e.id.clone(),
                    strength,
                    weight: rubric.weights.weight(strength),
                })
        })
        .collect();
    ClassScore {
        subitem: subitem.id.clone(),
        name: subitem.name.clone(),
        score: round4(saturate(contributions.iter().map(|c| c.weight))),
        contributions,
    }
}

fn rationale(classes: &[ClassScore]) -> String {
    let (supported, missing): (Vec<&ClassScore>, Vec<&ClassScore>) =
        classes.iter().partition(|c| !c.contributions.is_empty());

    let mut parts = Vec::new();
    if !supported.is_empty() {
        let items: Vec<String> = supported
            .iter()
            .map(|c| {
                let ids: Vec<&str> = c.contributions.iter().map(|x| x.evidence_id.as_str()).collect();
                format!(
                    "{} {} = {:.2} [{}; {}]",
                    c.subitem,
                    c.name,
                    c.score,
                    strength_counts(&c.contributions),
                    ids.join(", ")
                )
            })
            .collect();
        parts.push(format!("Supported: {}.", items.join("; ")));
    }
    if !missing.is_empty() {
        let items: Vec<String> = missing.iter().map(|c| format!("{} {}", c.subitem, c.name)).collect();
        parts.push(format!("No evidence for: {}.", items.join("; ")));
    }
    parts.join(" ")
}

fn strength_counts(contributions: &[Contribution]) -> String {
    Strength::ALL
        .iter()
        .rev()
        .filter_map(|s| {
            let n = contributions.iter().filter(|c| c.strength == *s).count();
            (n > 0).then(|| format!("{n} {s}"))
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn round4(x: f64) -> f64 {
    (x * 10_000.0).round() / 10_000.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Support, TextSpan};

    fn ev(id: &str, supports: &[(&str, Strength)]) -> Evidence {
        Evidence::new(
            id.into(),
            TextSpan::text("S1".into(), String::new(), 0, 0),
            supports
                .iter()
                .map(|(s, st)| Support { subitem: (*s).into(), strength: *st, confidence: None, reason: None })
                .collect(),
        )
    }

    #[test]
    fn saturation_formula() {
        assert_eq!(saturate([]), 0.0);
        assert!((saturate([0.25, 0.25]) - 0.4375).abs() < 1e-12);
        assert!((saturate([0.6, 0.25]) - 0.7).abs() < 1e-12);
        assert_eq!(saturate([1.0, 0.25]), 1.0);
    }

    #[test]
    fn empty_evidence_scores_zero() {
        let r = Rubric::builtin_ai_journal();
        let s = score(&r, &[]);
        assert_eq!(s.total, 0.0);
        assert_eq!(s.max_total, 25.0);
        assert!(s.criteria.iter().all(|c| c.score == 0.0));
    }

    #[test]
    fn criterion_normalises_by_class_count() {
        let r = Rubric::builtin_ai_journal();
        // Criterion E has 4 subitems: one strong -> 5 * 1/4 = 1.25.
        // D1 gets weak + moderate from two items -> 0.7; D has 5 subitems -> 0.7.
        let evidence = vec![
            ev("E1", &[("E1", Strength::Strong), ("D1", Strength::Weak)]),
            ev("E2", &[("D1", Strength::Moderate)]),
        ];
        let s = score(&r, &evidence);
        let get = |id: &str| s.criteria.iter().find(|c| c.criterion.as_str() == id).unwrap();
        assert_eq!(get("E").score, 1.25);
        assert_eq!(get("D").score, 0.7);
        assert_eq!(s.total, 1.95);
        assert!(get("D").rationale.contains("D1 workflow / cycle identification = 0.70"));
    }

    #[test]
    fn unknown_subitems_are_ignored() {
        let r = Rubric::builtin_ai_journal();
        let s = score(&r, &[ev("E1", &[("Z9", Strength::Strong)])]);
        assert_eq!(s.total, 0.0);
    }
}
