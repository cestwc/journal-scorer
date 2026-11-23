//! Core data models.

mod criterion;
mod document;
mod evidence;
mod id;
mod score;
mod strength;
mod subitem;

pub use criterion::Criterion;
pub use document::{Document, Figure};
pub use evidence::{BBox, Evidence, PdfRegion, SpanKind, Support, TextSpan};
pub use id::{CriterionId, EvidenceId, SpanId, SubitemId};
pub use score::{ClassScore, Contribution, CriterionScore, ScoreResult};
pub use strength::{Strength, StrengthWeights};
pub use subitem::{LevelDescriptors, Subitem};
