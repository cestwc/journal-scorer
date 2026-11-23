use axum::extract::{Multipart, Path, Query, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use scorer_core::Rubric;
use scorer_engine::ScoreReport;
use scorer_llm::ProviderInfo;
use serde::{Deserialize, Serialize};

use super::error::ApiError;
use super::jobs::JobView;
use super::AppState;

#[derive(Serialize)]
pub struct Health {
    status: &'static str,
    provider: ProviderInfo,
    rubric_id: String,
}

pub async fn health(State(state): State<AppState>) -> Json<Health> {
    Json(Health {
        status: "ok",
        provider: state.scorer.provider_info(),
        rubric_id: state.scorer.rubric().id.clone(),
    })
}

pub async fn rubric(State(state): State<AppState>) -> Json<Rubric> {
    Json(state.scorer.rubric().clone())
}

#[derive(Deserialize)]
pub struct ScoreRequest {
    text: String,
    #[serde(default)]
    include_spans: bool,
}

pub async fn score(
    State(state): State<AppState>,
    Json(req): Json<ScoreRequest>,
) -> Result<Json<ScoreReport>, ApiError> {
    let mut report = state.scorer.score_text(&req.text).await?;
    if !req.include_spans {
        report.spans.clear();
    }
    report.source_text.clear();
    Ok(Json(report))
}

#[derive(Serialize)]
pub struct JobCreated {
    id: String,
}

/// Accepts a PDF upload (multipart field `file`) and starts scoring it.
pub async fn create_job(State(state): State<AppState>, mut form: Multipart) -> Result<(StatusCode, Json<JobCreated>), ApiError> {
    let bad = |m: String| ApiError::new(StatusCode::BAD_REQUEST, m);
    while let Some(field) = form.next_field().await.map_err(|e| bad(e.to_string()))? {
        if field.name() != Some("file") {
            continue;
        }
        let file_name = field.file_name().unwrap_or("upload.pdf").to_owned();
        let bytes = field.bytes().await.map_err(|e| bad(e.to_string()))?;
        if !bytes.starts_with(b"%PDF") {
            return Err(bad(format!("`{file_name}` is not a PDF")));
        }
        let id = state.jobs.submit(state.scorer.clone(), bytes.to_vec(), file_name);
        return Ok((StatusCode::ACCEPTED, Json(JobCreated { id })));
    }
    Err(bad("missing multipart field `file`".into()))
}

pub async fn job(State(state): State<AppState>, Path(id): Path<String>) -> Result<Json<JobView>, ApiError> {
    state.jobs.view(&id).map(Json).ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "unknown job"))
}

#[derive(Deserialize)]
pub struct DownloadQuery {
    #[serde(default)]
    download: Option<u8>,
}

pub async fn annotated_pdf(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<DownloadQuery>,
) -> Result<Response, ApiError> {
    let (file_name, bytes) = state
        .jobs
        .annotated(&id)
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "no annotated PDF for this job (yet)"))?;
    let stem = file_name.strip_suffix(".pdf").or_else(|| file_name.strip_suffix(".PDF")).unwrap_or(&file_name);
    let safe: String = stem.chars().map(|c| if c.is_ascii_alphanumeric() || " -_.".contains(c) { c } else { '_' }).collect();
    let disposition = if q.download.unwrap_or(0) == 1 { "attachment" } else { "inline" };
    Ok((
        [
            (header::CONTENT_TYPE, "application/pdf".to_owned()),
            (header::CONTENT_DISPOSITION, format!("{disposition}; filename=\"{safe}-annotated.pdf\"")),
        ],
        bytes.as_ref().clone(),
    )
        .into_response())
}
