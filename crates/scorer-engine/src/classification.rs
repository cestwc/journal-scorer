//! The model's raw output format and its validation into domain `Evidence`.
//! Everything from the model is treated as untrusted: unknown spans,
//! subitems and strengths are dropped with a warning.

use std::collections::BTreeMap;

use scorer_core::{Evidence, EvidenceId, Rubric, Strength, Support, TextSpan};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct LlmClassification {
    #[serde(default)]
    pub evidence: Vec<LlmEvidence>,
}

#[derive(Debug, Deserialize)]
pub struct LlmEvidence {
    pub span_id: String,
    #[serde(default)]
    pub supports: Vec<LlmSupport>,
}

#[derive(Debug, Deserialize)]
pub struct LlmSupport {
    pub subitem: String,
    pub strength: String,
    #[serde(default)]
    pub confidence: Option<f32>,
    #[serde(default)]
    pub reason: Option<String>,
}

/// Converts one batch's classification into evidence, in span order.
/// Evidence ids are placeholders; the pipeline renumbers them.
pub fn to_evidence(
    rubric: &Rubric,
    batch: &[TextSpan],
    classification: LlmClassification,
    warnings: &mut Vec<String>,
) -> Vec<Evidence> {
    let mut by_span: BTreeMap<usize, Vec<Support>> = BTreeMap::new();

    for item in classification.evidence {
        let span_id = item.span_id.trim();
        let Some(idx) = batch.iter().position(|s| s.id.as_str().eq_ignore_ascii_case(span_id)) else {
            warnings.push(format!("model referenced unknown span `{span_id}`; ignored"));
            continue;
        };
        for sup in item.supports {
            let Some((_, subitem)) = rubric.resolve_subitem(&sup.subitem) else {
                warnings.push(format!("{span_id}: unknown subitem `{}`; ignored", sup.subitem));
                continue;
            };
            let Ok(strength) = sup.strength.parse::<Strength>() else {
                warnings.push(format!("{span_id}/{}: unknown strength `{}`; ignored", subitem.id, sup.strength));
                continue;
            };
            by_span.entry(idx).or_default().push(Support {
                subitem: subitem.id.clone(),
                strength,
                confidence: sup.confidence.map(|c| c.clamp(0.0, 1.0)),
                reason: sup.reason.filter(|r| !r.trim().is_empty()),
            });
        }
    }

    by_span
        .into_iter()
        .filter(|(_, supports)| !supports.is_empty())
        .map(|(idx, supports)| {
            let span = batch[idx].clone();
            let mut e = Evidence { id: EvidenceId::from("pending"), span, supports };
            if e.dedup_supports() > 0 {
                warnings.push(format!("{}: duplicate subitem labels collapsed to the strongest", e.span.id));
            }
            e
        })
        .collect()
}
