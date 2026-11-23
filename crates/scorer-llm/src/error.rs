use thiserror::Error;

#[derive(Debug, Error)]
pub enum LlmError {
    #[error("provider configuration error: {0}")]
    Config(String),
    #[error("transport error: {0}")]
    Transport(String),
    #[error("provider returned HTTP {status}: {body}")]
    Status { status: u16, body: String },
    #[error("could not decode provider response: {0}")]
    Decode(String),
    #[error("provider returned an empty completion")]
    Empty,
    #[error("model output is not valid JSON ({reason}); output starts with: {snippet}")]
    InvalidJson { reason: String, snippet: String },
}
