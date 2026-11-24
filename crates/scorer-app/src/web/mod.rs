//! HTTP server: browser UI at `/` plus the JSON API.
//!
//! * `GET  /`  (and `/app.js`, `/app.css`)
//! * `GET  /api/health`
//! * `GET  /api/rubric`
//! * `GET  /api/models`  default model, whether it can be switched, usable models
//! * `POST /api/score`  `{"text": "...", "include_spans": false, "model": "provider/model"}`
//! * `POST /api/jobs`   multipart `file` = PDF, optional `model`  ->  `{"id": ...}`
//! * `GET  /api/jobs/{id}`  progress, then the report
//! * `POST /api/jobs/{id}/cancel`  stop an unfinished job
//! * `GET  /api/jobs/{id}/annotated.pdf[?download=1]`

mod assets;
mod error;
mod handlers;
mod jobs;

use std::sync::Arc;

use anyhow::Result;
use axum::extract::DefaultBodyLimit;
use axum::routing::{get, post};
use axum::Router;

use crate::models::ModelRegistry;

pub use jobs::JobStore;

/// Largest accepted PDF upload.
const MAX_UPLOAD_BYTES: usize = 64 * 1024 * 1024;

#[derive(Clone)]
pub struct AppState {
    pub models: Arc<ModelRegistry>,
    pub jobs: Arc<JobStore>,
}

impl AppState {
    pub fn new(models: Arc<ModelRegistry>) -> Self {
        Self { models, jobs: Arc::new(JobStore::default()) }
    }
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/", get(assets::index))
        .route("/app.js", get(assets::app_js))
        .route("/app.css", get(assets::app_css))
        .route("/api/health", get(handlers::health))
        .route("/api/rubric", get(handlers::rubric))
        .route("/api/models", get(handlers::models))
        .route("/api/score", post(handlers::score))
        .route("/api/jobs", post(handlers::create_job).layer(DefaultBodyLimit::max(MAX_UPLOAD_BYTES)))
        .route("/api/jobs/{id}", get(handlers::job))
        .route("/api/jobs/{id}/cancel", post(handlers::cancel_job))
        .route("/api/jobs/{id}/annotated.pdf", get(handlers::annotated_pdf))
        .with_state(state)
}

pub async fn serve(models: Arc<ModelRegistry>, addr: &str) -> Result<()> {
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!("listening on http://{}", listener.local_addr()?);
    axum::serve(listener, router(AppState::new(models)))
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use http_body_util::BodyExt;
    use scorer_core::Rubric;
    use scorer_engine::Scorer;
    use scorer_llm::providers::MockProvider;
    use tower::ServiceExt;

    fn app(responses: &[&str]) -> Router {
        let provider = Arc::new(MockProvider::new(responses.iter().map(|s| s.to_string()).collect()));
        let scorer = Scorer::new(provider, Arc::new(Rubric::builtin_ai_journal()));
        router(AppState::new(Arc::new(ModelRegistry::fixed(Arc::new(scorer)))))
    }

    async fn post_score(app: Router, body: &str) -> (StatusCode, serde_json::Value) {
        let req = Request::post("/api/score")
            .header("content-type", "application/json")
            .body(Body::from(body.to_owned()))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        let status = resp.status();
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        (status, serde_json::from_slice(&bytes).unwrap())
    }

    #[tokio::test]
    async fn score_endpoint_returns_report() {
        let reply = r#"{"evidence":[{"span_id":"S1","supports":[{"subitem":"M5","strength":"strong"}]}]}"#;
        let (status, json) = post_score(app(&[reply]), r#"{"text":"Next sprint I will write failing tests first."}"#).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["score"]["total"], 1.0);
        assert_eq!(json["evidence"][0]["supports"][0]["subitem"], "M5");
        assert!(json.get("spans").is_none());
    }

    #[tokio::test]
    async fn serves_ui() {
        for (path, ty) in [("/", "text/html"), ("/app.js", "text/javascript"), ("/app.css", "text/css")] {
            let resp = app(&[]).oneshot(Request::get(path).body(Body::empty()).unwrap()).await.unwrap();
            assert_eq!(resp.status(), StatusCode::OK, "{path}");
            let ct = resp.headers()["content-type"].to_str().unwrap();
            assert!(ct.starts_with(ty), "{path}: {ct}");
        }
    }

    async fn get_json(app: Router, path: &str) -> (StatusCode, serde_json::Value) {
        let resp = app.oneshot(Request::get(path).body(Body::empty()).unwrap()).await.unwrap();
        let status = resp.status();
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        (status, serde_json::from_slice(&bytes).unwrap())
    }

    #[tokio::test]
    async fn fixed_server_lists_its_one_model() {
        let (status, json) = get_json(app(&[]), "/api/models").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["default"], "mock");
        assert_eq!(json["switchable"], false);
        assert_eq!(json["models"][0]["spec"], "mock");
    }

    #[tokio::test]
    async fn model_choice_is_validated() {
        // Naming the default is fine; anything else is refused on a fixed server.
        let reply = r#"{"evidence":[]}"#;
        let (status, json) = post_score(app(&[reply]), r#"{"text":"I wrote tests first.","model":"mock"}"#).await;
        assert_eq!(status, StatusCode::OK, "{json}");
        assert_eq!(json["provider"]["model"], "mock");
        let (status, json) = post_score(app(&[]), r#"{"text":"I wrote tests first.","model":"other/x"}"#).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(json["error"].as_str().unwrap().contains("only uses"));
    }

    #[tokio::test]
    async fn empty_text_is_bad_request() {
        let (status, json) = post_score(app(&[]), r#"{"text":"  "}"#).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(json["error"].as_str().unwrap().contains("empty"));
    }
}
