use std::sync::Arc;

use scorer_core::scoring;
use scorer_core::segment::{Segmenter, SentenceSegmenter};
use scorer_core::{Document, EvidenceId, Figure, PdfRegion, Rubric, SpanId, SpanKind, TextSpan};
use scorer_llm::json::{parse_json, strip_reasoning};
use scorer_llm::{CompletionRequest, ImageData, LlmProvider, Message, ProviderInfo};
use serde::{Deserialize, Serialize};

use crate::classification::{to_evidence, LlmClassification};
use crate::progress::{no_progress, Progress, ProgressFn, Stage};
use crate::{prompt, EngineError, RunStats, ScoreReport};

/// Longest figure description kept (characters).
const MAX_FIGURE_DESCRIPTION: usize = 1200;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ScorerOptions {
    /// Spans sent per LLM call.
    pub batch_size: usize,
    pub temperature: f32,
    pub max_tokens: Option<u32>,
    pub seed: Option<u64>,
    /// Extra attempts when the model returns unparseable JSON.
    pub max_retries: u32,
    /// Send figures to the (vision) model for description.
    pub describe_figures: bool,
    /// Output budget for one figure description (includes any reasoning).
    pub figure_max_tokens: u32,
}

impl Default for ScorerOptions {
    fn default() -> Self {
        Self {
            batch_size: 15,
            temperature: 0.0,
            max_tokens: Some(16384),
            seed: None,
            max_retries: 1,
            describe_figures: true,
            figure_max_tokens: 4096,
        }
    }
}

/// The scoring pipeline. Cheap to share behind an `Arc`.
pub struct Scorer {
    provider: Arc<dyn LlmProvider>,
    rubric: Arc<Rubric>,
    segmenter: Arc<dyn Segmenter>,
    options: ScorerOptions,
}

impl Scorer {
    pub fn new(provider: Arc<dyn LlmProvider>, rubric: Arc<Rubric>) -> Self {
        Self {
            provider,
            rubric,
            segmenter: Arc::new(SentenceSegmenter::default()),
            options: ScorerOptions::default(),
        }
    }

    pub fn with_segmenter(mut self, segmenter: Arc<dyn Segmenter>) -> Self {
        self.segmenter = segmenter;
        self
    }

    pub fn with_options(mut self, options: ScorerOptions) -> Self {
        self.options = options;
        self
    }

    pub fn rubric(&self) -> &Rubric {
        &self.rubric
    }

    pub fn segmenter(&self) -> &dyn Segmenter {
        self.segmenter.as_ref()
    }

    pub fn provider_info(&self) -> ProviderInfo {
        self.provider.info()
    }

    /// Segments plain text and scores it.
    pub async fn score_text(&self, text: &str) -> Result<ScoreReport, EngineError> {
        if text.trim().is_empty() {
            return Err(EngineError::EmptyInput);
        }
        let doc = Document { text: text.to_owned(), spans: self.segmenter.segment(text), figures: Vec::new() };
        self.score_document(doc, &no_progress).await
    }

    /// Scores a prepared document: describes its figures, then classifies
    /// text and figure spans together in reading order.
    pub async fn score_document(&self, doc: Document, progress: ProgressFn<'_>) -> Result<ScoreReport, EngineError> {
        let Document { text, mut spans, figures } = doc;
        let mut warnings = Vec::new();
        let mut stats = RunStats { figures: figures.len(), ..Default::default() };

        if self.options.describe_figures {
            for (i, fig) in figures.iter().enumerate() {
                progress(Progress::new(Stage::DescribingImages, i, figures.len()));
                stats.llm_calls += 1;
                match self.describe_figure(fig).await {
                    Ok(desc) => spans.push(figure_span(i, fig, &desc)),
                    Err(e) => warnings.push(format!("figure {} (page {}) skipped: {e}", i + 1, fig.page)),
                }
            }
        } else if !figures.is_empty() {
            warnings.push(format!("{} figure(s) not analysed (describe_figures = false)", figures.len()));
        }
        // Reading order; a figure goes before text starting at its anchor.
        spans.sort_by_key(|s| (s.start, s.kind == SpanKind::Text));
        if spans.is_empty() {
            return Err(EngineError::EmptyInput);
        }
        stats.spans = spans.len();

        let system = prompt::system_prompt(&self.rubric);
        let mut evidence = Vec::new();
        let batches: Vec<&[TextSpan]> = spans.chunks(self.options.batch_size.max(1)).collect();
        for (i, batch) in batches.iter().enumerate() {
            progress(Progress::new(Stage::Classifying, i, batches.len()));
            tracing::info!(batch = i + 1, of = batches.len(), spans = batch.len(), "classifying batch");
            let (classification, calls) = self.classify_batch(&system, batch).await?;
            stats.batches += 1;
            stats.llm_calls += calls;
            evidence.extend(to_evidence(&self.rubric, batch, classification, &mut warnings));
        }
        progress(Progress::new(Stage::Classifying, batches.len(), batches.len()));

        for (i, e) in evidence.iter_mut().enumerate() {
            e.id = EvidenceId(format!("E{}", i + 1));
        }
        stats.evidence = evidence.len();

        Ok(ScoreReport {
            score: scoring::score(&self.rubric, &evidence),
            evidence,
            provider: self.provider.info(),
            stats,
            warnings,
            spans,
            source_text: text,
        })
    }

    async fn describe_figure(&self, fig: &Figure) -> Result<String, EngineError> {
        let request = CompletionRequest {
            messages: vec![
                Message::system(prompt::FIGURE_SYSTEM_PROMPT),
                Message::user(prompt::figure_user_prompt(fig.page)).with_image(ImageData::png(fig.png.clone())),
            ],
            temperature: Some(self.options.temperature),
            max_tokens: Some(self.options.figure_max_tokens),
            seed: self.options.seed,
            json_output: false,
        };
        let response = self.provider.complete(request).await?;
        let text: String = strip_reasoning(&response.content).split_whitespace().collect::<Vec<_>>().join(" ");
        if text.is_empty() {
            return Err(EngineError::Llm(scorer_llm::LlmError::Empty));
        }
        Ok(text.chars().take(MAX_FIGURE_DESCRIPTION).collect())
    }

    async fn classify_batch(
        &self,
        system: &str,
        batch: &[TextSpan],
    ) -> Result<(LlmClassification, u32), EngineError> {
        let request = CompletionRequest {
            messages: vec![Message::system(system), Message::user(prompt::user_prompt(batch))],
            temperature: Some(self.options.temperature),
            max_tokens: self.options.max_tokens,
            seed: self.options.seed,
            json_output: true,
        };
        let mut attempts = 0;
        loop {
            attempts += 1;
            let response = self.provider.complete(request.clone()).await?;
            match parse_json::<LlmClassification>(&response.content) {
                Ok(c) => return Ok((c, attempts)),
                Err(e) if attempts <= self.options.max_retries => {
                    tracing::warn!(attempt = attempts, error = %e, "unparseable model output; retrying");
                }
                Err(e) => return Err(e.into()),
            }
        }
    }
}

fn figure_span(index: usize, fig: &Figure, description: &str) -> TextSpan {
    TextSpan {
        id: SpanId(format!("F{}", index + 1)),
        kind: SpanKind::Image,
        text: format!("[Image on page {}] {description}", fig.page),
        start: fig.anchor,
        end: fig.anchor,
        regions: vec![PdfRegion { page: fig.page, bbox: fig.bbox }],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use scorer_llm::providers::MockProvider;

    fn scorer(responses: &[&str]) -> Scorer {
        let provider = Arc::new(MockProvider::new(responses.iter().map(|s| s.to_string()).collect()));
        Scorer::new(provider, Arc::new(Rubric::builtin_ai_journal()))
    }

    const TEXT: &str = "We followed SDD then BDD then TDD for the login feature. \
                        I reviewed every diff Copilot produced. The weather was nice.";

    #[tokio::test]
    async fn end_to_end_with_mock() {
        let reply = r#"<think>hmm</think>```json
        {"evidence": [
          {"span_id": "S1", "supports": [
            {"subitem": "D1", "strength": "strong", "confidence": 0.9, "reason": "names cycle"},
            {"subitem": "s1", "strength": "Moderate"},
            {"subitem": "D1", "strength": "weak"}
          ]},
          {"span_id": "S2", "supports": [{"subitem": "E1", "strength": "weak"}, {"subitem": "Q7", "strength": "weak"}]},
          {"span_id": "S99", "supports": [{"subitem": "M1", "strength": "weak"}]}
        ]}
        ```"#;
        let report = scorer(&[reply]).score_text(TEXT).await.unwrap();

        assert_eq!(report.stats.spans, 3);
        assert_eq!(report.evidence.len(), 2);
        assert_eq!(report.evidence[0].id.as_str(), "E1");
        assert_eq!(report.evidence[0].supports.len(), 2);
        assert_eq!(report.warnings.len(), 3, "{:?}", report.warnings);

        let get = |id: &str| report.score.criteria.iter().find(|c| c.criterion.as_str() == id).unwrap();
        assert_eq!(get("D").score, 1.0); // 5 * 1.0 / 5
        assert_eq!(get("S").score, 0.6); // 5 * 0.6 / 5
        assert_eq!(get("E").score, 0.3125); // 5 * 0.25 / 4
        assert_eq!(report.score.total, 1.9125);
    }

    #[tokio::test]
    async fn retries_on_bad_json_then_fails() {
        let err = scorer(&["nope", "still nope"]).score_text(TEXT).await.unwrap_err();
        assert!(matches!(err, EngineError::Llm(scorer_llm::LlmError::InvalidJson { .. })));

        let ok = scorer(&["nope", r#"{"evidence": []}"#]).score_text(TEXT).await.unwrap();
        assert_eq!(ok.stats.llm_calls, 2);
    }

    #[tokio::test]
    async fn figures_are_described_and_classified_in_order() {
        use scorer_core::BBox;
        let text = "First sentence here. Second sentence here.";
        let spans = SentenceSegmenter::default().segment(text);
        let fig = Figure { page: 2, bbox: BBox { x0: 0.0, y0: 0.0, x1: 10.0, y1: 10.0 }, anchor: 21, png: vec![0] };
        let reply = r#"{"evidence":[{"span_id":"F1","supports":[{"subitem":"S3","strength":"strong"}]}]}"#;
        let progress_calls = std::sync::Mutex::new(Vec::new());
        let report = scorer(&[reply])
            .score_document(
                Document { text: text.into(), spans, figures: vec![fig] },
                &|p| progress_calls.lock().unwrap().push(p.stage),
            )
            .await
            .unwrap();

        let ids: Vec<&str> = report.spans.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids, ["S1", "F1", "S2"]);
        assert_eq!(report.evidence[0].span.kind, SpanKind::Image);
        assert_eq!(report.evidence[0].span.page(), Some(2));
        assert!(report.evidence[0].span.text.starts_with("[Image on page 2] Mock description"));
        assert_eq!(report.stats.llm_calls, 2);
        assert_eq!(progress_calls.lock().unwrap()[0], Stage::DescribingImages);
    }

    #[tokio::test]
    async fn empty_input_rejected() {
        assert!(matches!(scorer(&[]).score_text("   ").await, Err(EngineError::EmptyInput)));
    }
}
