use std::collections::VecDeque;
use std::sync::Mutex;

use async_trait::async_trait;

use crate::{CompletionRequest, CompletionResponse, LlmError, LlmProvider, ProviderInfo};

const EMPTY_RESPONSE: &str = r#"{"evidence": []}"#;
const IMAGE_RESPONSE: &str = "Mock description of an image.";

/// Returns queued responses in order, then an empty evidence list. Requests
/// containing images get a fixed description and do not consume the queue.
pub struct MockProvider {
    queue: Mutex<VecDeque<String>>,
}

impl MockProvider {
    pub fn new(responses: Vec<String>) -> Self {
        Self { queue: Mutex::new(responses.into()) }
    }
}

#[async_trait]
impl LlmProvider for MockProvider {
    fn info(&self) -> ProviderInfo {
        ProviderInfo::new("mock", "mock")
    }

    async fn complete(&self, request: CompletionRequest) -> Result<CompletionResponse, LlmError> {
        if request.messages.iter().any(|m| !m.images.is_empty()) {
            return Ok(CompletionResponse { content: IMAGE_RESPONSE.into(), model: Some("mock".into()), ..Default::default() });
        }
        let content = self
            .queue
            .lock()
            .expect("mock queue poisoned")
            .pop_front()
            .unwrap_or_else(|| EMPTY_RESPONSE.to_owned());
        Ok(CompletionResponse { content, model: Some("mock".into()), ..Default::default() })
    }
}
