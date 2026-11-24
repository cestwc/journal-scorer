use axum::extract::{Multipart, Path, Query, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use scorer_core::Rubric;
use scorer_engine::ScoreReport;
use scorer_llm::ProviderInfo;
use serde::{Deserialize, Serialize};

use crate::models::{ModelError, ModelList};

use super::error::ApiError;
use super::jobs::{CancelError, JobView};
use super::AppState;

#[derive(Serialize)]
pub struct Health {
    status: &'static str,
    provider: ProviderInfo,
    rubric_id: String,
}

pub async fn health(State(state): State<AppState>) -> Json<Health> {
    let scorer = state.models.default_scorer();
    Json(Health { status: "ok", provider: scorer.provider_info(), rubric_id: scorer.rubric().id.clone() })
}

pub async fn rubric(State(state): State<AppState>) -> Json<Rubric> {
    Json(state.models.default_scorer().rubric().clone())
}

pub async fn models(State(state): State<AppState>) -> Json<ModelList> {
    let models = state.models.clone();
    // Reads OpenCode's files (the catalog is a few MB).
    Json(tokio::task::spawn_blocking(move || models.list()).await.expect("listing models does not panic"))
}

impl From<ModelError> for ApiError {
    fn from(e: ModelError) -> Self {
        ApiError::new(StatusCode::BAD_REQUEST, e.to_string())
    }
}

#[derive(Deserialize)]
pub struct ScoreRequest {
    text: String,
    #[serde(default)]
    include_spans: bool,
    /// `provider/model`; the server's default when absent.
    #[serde(default)]
    model: Option<String>,
}

pub async fn score(
    State(state): State<AppState>,
    Json(req): Json<ScoreRequest>,
) -> Result<Json<ScoreReport>, ApiError> {
    let scorer = state.models.get(req.model.as_deref())?;
    let mut report = scorer.score_text(&req.text).await?;
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

/// Accepts a PDF upload (multipart field `file`, optional field `model`)
/// and starts scoring it.
pub async fn create_job(State(state): State<AppState>, mut form: Multipart) -> Result<(StatusCode, Json<JobCreated>), ApiError> {
    let bad = |m: String| ApiError::new(StatusCode::BAD_REQUEST, m);
    let mut upload = None;
    let mut model = None;
    while let Some(field) = form.next_field().await.map_err(|e| bad(e.to_string()))? {
        match field.name() {
            Some("file") => {
                let file_name = field.file_name().unwrap_or("upload.pdf").to_owned();
                let bytes = field.bytes().await.map_err(|e| bad(e.to_string()))?;
                if !bytes.starts_with(b"%PDF") {
                    return Err(bad(format!("`{file_name}` is not a PDF")));
                }
                upload = Some((file_name, bytes.to_vec()));
            }
            Some("model") => model = Some(field.text().await.map_err(|e| bad(e.to_string()))?),
            _ => {}
        }
    }
    let (file_name, bytes) = upload.ok_or_else(|| bad("missing multipart field `file`".into()))?;
    let scorer = state.models.get(model.as_deref())?;
    let id = state.jobs.submit(scorer, bytes, file_name);
    Ok((StatusCode::ACCEPTED, Json(JobCreated { id })))
}

pub async fn job(State(state): State<AppState>, Path(id): Path<String>) -> Result<Json<JobView>, ApiError> {
    state.jobs.view(&id).map(Json).ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "unknown job"))
}

pub async fn cancel_job(State(state): State<AppState>, Path(id): Path<String>) -> Result<Json<JobView>, ApiError> {
    state.jobs.cancel(&id).map(Json).map_err(|e| match e {
        CancelError::NotFound => ApiError::new(StatusCode::NOT_FOUND, "unknown job"),
        CancelError::Finished(stage) => {
            ApiError::new(StatusCode::CONFLICT, format!("job already finished ({})", stage_name(stage)))
        }
    })
}

fn stage_name(stage: scorer_engine::Stage) -> String {
    serde_json::to_value(stage).ok().and_then(|v| v.as_str().map(str::to_owned)).unwrap_or_default()
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
