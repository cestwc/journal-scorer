use scorer_core::{Evidence, ScoreResult, TextSpan};
use scorer_llm::ProviderInfo;
use serde::Serialize;

#[derive(Debug, Clone, Default, Serialize)]
pub struct RunStats {
    pub spans: usize,
    pub figures: usize,
    pub batches: usize,
    pub llm_calls: u32,
    pub evidence: usize,
}

/// Everything an entry point returns for one scored document.
#[derive(Debug, Clone, Serialize)]
pub struct ScoreReport {
    pub score: ScoreResult,
    pub evidence: Vec<Evidence>,
    pub provider: ProviderInfo,
    pub stats: RunStats,
    /// Non-fatal problems, e.g. the model referencing an unknown subitem.
    pub warnings: Vec<String>,
    /// All analysed spans (text and figures); callers may clear it.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub spans: Vec<TextSpan>,
    /// The document text that span offsets refer to; callers may clear it.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub source_text: String,
}
