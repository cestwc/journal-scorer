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
}

/// A chat-completion backend. Implement this to add a provider.
#[async_trait]
pub trait LlmProvider: Send + Sync {
    fn info(&self) -> ProviderInfo;

    async fn complete(&self, request: CompletionRequest) -> Result<CompletionResponse, LlmError>;
}
