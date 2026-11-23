use scorer_llm::LlmError;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum EngineError {
    #[error("input text is empty")]
    EmptyInput,
    #[error(transparent)]
    Llm(#[from] LlmError),
}
