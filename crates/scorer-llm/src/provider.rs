use async_trait::async_trait;
use serde::Serialize;

use crate::{CompletionRequest, CompletionResponse, LlmError};

/// Identifies the provider and model that produced a result.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ProviderInfo {
    pub kind: String,
    pub model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<String>,
    /// Whether the model accepts images (`None` = unknown).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vision: Option<bool>,
    /// Hard output-token limit of the model (`None` = unknown).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_output: Option<u32>,
}

impl ProviderInfo {
    pub fn new(kind: impl Into<String>, model: impl Into<String>) -> Self {
        Self { kind: kind.into(), model: model.into(), endpoint: None, vision: None, max_output: None }
    }
}

/// A chat-completion backend. Implement this to add a provider.
#[async_trait]
pub trait LlmProvider: Send + Sync {
    fn info(&self) -> ProviderInfo;

    async fn complete(&self, request: CompletionRequest) -> Result<CompletionResponse, LlmError>;
}
