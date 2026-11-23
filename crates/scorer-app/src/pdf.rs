//! PDF flow shared by the CLI and the web jobs:
//! extract -> score -> annotate. MuPDF work runs on blocking threads.

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use scorer_core::segment::SentenceSegmenter;
use scorer_engine::{Progress, ProgressFn, ScoreReport, Scorer, Stage};
use scorer_pdf::{AnnotateInput, ExtractOptions, SheetMeta};

pub struct PdfOutcome {
    pub report: ScoreReport,
    pub annotated: Vec<u8>,
}

pub async fn score_pdf(scorer: &Scorer, pdf: Arc<Vec<u8>>, file_name: &str, progress: ProgressFn<'_>) -> Result<PdfOutcome> {
    progress(Progress::stage(Stage::Extracting));
    let source = pdf.clone();
    let doc = tokio::task::spawn_blocking(move || {
        scorer_pdf::extract(&source, &SentenceSegmenter::default(), &ExtractOptions::default())
    })
    .await?
    .context("reading PDF")?;
    tracing::info!(spans = doc.spans.len(), figures = doc.figures.len(), "extracted PDF");

    let report = scorer.score_document(doc, progress).await?;

    progress(Progress::stage(Stage::Annotating));
    let rubric = scorer.rubric().clone();
    let evidence = report.evidence.clone();
    let score = report.score.clone();
    let meta = SheetMeta {
        file_name: file_name.to_owned(),
        model: report.provider.model.clone(),
        generated: utc_now(),
    };
    let annotated = tokio::task::spawn_blocking(move || {
        scorer_pdf::annotate(&pdf, &AnnotateInput { rubric: &rubric, evidence: &evidence, score: &score, meta })
    })
    .await?
    .context("writing annotated PDF")?;

    progress(Progress::stage(Stage::Done));
    Ok(PdfOutcome { report, annotated })
}

/// `2026-11-23 09:41 UTC` without a date-time dependency.
pub fn utc_now() -> String {
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0) as i64;
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // Howard Hinnant's civil-from-days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02} {:02}:{:02} UTC", rem / 3600, rem % 3600 / 60)
}
