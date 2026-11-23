//! Runtime provider selection. Deserialises from a `[llm]` table whose
//! `kind` field picks the adapter.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProviderConfig {
    /// Any server exposing `POST {base_url}/chat/completions` in the OpenAI
    /// format (llama.cpp, vLLM, Strata, OpenRouter, OpenAI, ...).
    OpenaiCompatible(OpenAiCompatibleConfig),
    /// Offline provider returning canned responses; for tests and UI work.
    Mock(MockConfig),
}

fn default_timeout() -> u64 {
    300
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OpenAiCompatibleConfig {
    /// e.g. `http://127.0.0.1:8080/v1`
    pub base_url: String,
    pub model: String,
    /// Name of the environment variable holding the API key. The key itself
    /// is never stored in config. Omit for unauthenticated servers.
    #[serde(default)]
    pub api_key_env: Option<String>,
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
    /// Send `response_format = {"type": "json_object"}`.
    #[serde(default = "default_true")]
    pub json_mode: bool,
    /// Extra top-level fields merged into every request body, for
    /// server-specific options (e.g. `chat_template_kwargs`).
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub extra_body: Map<String, Value>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct MockConfig {
    /// Responses returned in order; once exhausted, an empty evidence list.
    #[serde(default)]
    pub responses: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_openai_compatible_from_toml() {
        let cfg: ProviderConfig = toml::from_str(
            r#"
            kind = "openai_compatible"
            base_url = "http://127.0.0.1:8080/v1"
            model = "local"
            api_key_env = "LLM_API_KEY"
            [extra_body.chat_template_kwargs]
            enable_thinking = false
            "#,
        )
        .unwrap();
        let ProviderConfig::OpenaiCompatible(c) = cfg else { panic!("wrong variant") };
        assert_eq!(c.timeout_secs, 300);
        assert!(c.json_mode);
        assert_eq!(c.extra_body["chat_template_kwargs"]["enable_thinking"], false);
    }
}
