use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use scorer_engine::EngineError;
use serde_json::json;

/// JSON error body: `{"error": "..."}`.
pub struct ApiError {
    status: StatusCode,
    message: String,
}

impl ApiError {
    pub fn new(status: StatusCode, message: impl Into<String>) -> Self {
        Self { status, message: message.into() }
    }
}

impl From<EngineError> for ApiError {
    fn from(e: EngineError) -> Self {
        let status = match &e {
            EngineError::EmptyInput => StatusCode::BAD_REQUEST,
            EngineError::Llm(_) => StatusCode::BAD_GATEWAY,
        };
        Self { status, message: e.to_string() }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        if self.status.is_server_error() {
            tracing::error!(status = %self.status, error = %self.message, "request failed");
        }
        (self.status, Json(json!({ "error": self.message }))).into_response()
    }
}
