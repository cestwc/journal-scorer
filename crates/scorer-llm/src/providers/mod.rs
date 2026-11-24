//! Concrete provider adapters and the factory that selects one.

mod mock;
mod openai_compatible;

use std::sync::Arc;

pub use mock::MockProvider;
pub use openai_compatible::OpenAiCompatibleProvider;

use crate::opencode::OpencodeSetup;
use crate::{LlmError, LlmProvider, ProviderConfig, ProviderInfo};

pub fn build_provider(config: &ProviderConfig) -> Result<Arc<dyn LlmProvider>, LlmError> {
    Ok(match config {
        ProviderConfig::OpenaiCompatible(c) => Arc::new(OpenAiCompatibleProvider::from_config(c)?),
        ProviderConfig::Opencode(c) => {
            let setup = OpencodeSetup::load(c)?;
            let spec = c.model.clone().or_else(|| setup.default_model()).ok_or_else(|| {
                LlmError::Config("no model chosen: set `model` (see `journal-scorer models`)".into())
            })?;
            let m = setup.resolve(&spec)?;
            let info = ProviderInfo {
                kind: "opencode".into(),
                model: m.spec.clone(),
                endpoint: None,
                vision: m.vision,
                max_output: m.max_output,
            };
            Arc::new(OpenAiCompatibleProvider::new(&m.base_url, &m.api_model, m.api_key, c.timeout_secs, true, Default::default())?.with_info(info))
        }
        ProviderConfig::Mock(c) => Arc::new(MockProvider::new(c.responses.clone())),
    })
}
