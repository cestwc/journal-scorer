//! Concrete provider adapters and the factory that selects one.

mod mock;
mod openai_compatible;

use std::sync::Arc;

pub use mock::MockProvider;
pub use openai_compatible::OpenAiCompatibleProvider;

use crate::{LlmError, LlmProvider, ProviderConfig};

pub fn build_provider(config: &ProviderConfig) -> Result<Arc<dyn LlmProvider>, LlmError> {
    Ok(match config {
        ProviderConfig::OpenaiCompatible(c) => Arc::new(OpenAiCompatibleProvider::from_config(c)?),
        ProviderConfig::Mock(c) => Arc::new(MockProvider::new(c.responses.clone())),
    })
}
