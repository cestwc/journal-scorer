use std::time::Duration;

use async_trait::async_trait;
use base64::Engine as _;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use crate::config::OpenAiCompatibleConfig;
use crate::{CompletionRequest, CompletionResponse, LlmError, LlmProvider, Message, ProviderInfo, Role, Usage};

/// Adapter for any OpenAI-style `/chat/completions` endpoint.
pub struct OpenAiCompatibleProvider {
    client: reqwest::Client,
    endpoint: String,
    model: String,
    api_key: Option<String>,
    json_mode: bool,
    extra_body: Map<String, Value>,
    info: Option<ProviderInfo>,
}

impl OpenAiCompatibleProvider {
    pub fn from_config(cfg: &OpenAiCompatibleConfig) -> Result<Self, LlmError> {
        let api_key = match &cfg.api_key_env {
            Some(var) => Some(std::env::var(var).map_err(|_| {
                LlmError::Config(format!("environment variable `{var}` (api_key_env) is not set"))
            })?),
            None => None,
        };
        Self::new(&cfg.base_url, &cfg.model, api_key, cfg.timeout_secs, cfg.json_mode, cfg.extra_body.clone())
    }

    pub fn new(
        base_url: &str,
        model: &str,
        api_key: Option<String>,
        timeout_secs: u64,
        json_mode: bool,
        extra_body: Map<String, Value>,
    ) -> Result<Self, LlmError> {
        if base_url.trim().is_empty() {
            return Err(LlmError::Config("base_url is empty".into()));
        }
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(timeout_secs))
            .build()
            .map_err(|e| LlmError::Config(e.to_string()))?;
        Ok(Self {
            client,
            endpoint: format!("{}/chat/completions", base_url.trim_end_matches('/')),
            model: model.to_owned(),
            api_key: api_key.filter(|k| !k.is_empty()),
            json_mode,
            extra_body,
            info: None,
        })
    }

    /// Reports `info` (e.g. the OpenCode model name and capabilities)
    /// instead of the plain adapter description.
    pub fn with_info(mut self, info: ProviderInfo) -> Self {
        self.info = Some(ProviderInfo { endpoint: Some(self.endpoint.clone()), ..info });
        self
    }
}

/// OpenAI wire format: plain string content, or an array of parts when
/// images are attached.
#[derive(Serialize)]
struct WireMessage {
    role: Role,
    content: Value,
}

impl From<&Message> for WireMessage {
    fn from(m: &Message) -> Self {
        let content = if m.images.is_empty() {
            Value::String(m.content.clone())
        } else {
            let mut parts = vec![json!({"type": "text", "text": m.content})];
            for img in &m.images {
                let b64 = base64::engine::general_purpose::STANDARD.encode(&img.bytes);
                parts.push(json!({"type": "image_url", "image_url": {"url": format!("data:{};base64,{b64}", img.mime)}}));
            }
            Value::Array(parts)
        };
        Self { role: m.role, content }
    }
}

#[derive(Serialize)]
struct ChatBody<'a> {
    model: &'a str,
    messages: Vec<WireMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    seed: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    response_format: Option<Value>,
    #[serde(flatten)]
    extra: &'a Map<String, Value>,
}

#[derive(Deserialize)]
struct ChatResponse {
    model: Option<String>,
    #[serde(default)]
    choices: Vec<Choice>,
    usage: Option<Usage>,
}

#[derive(Deserialize)]
struct Choice {
    message: ChoiceMessage,
    finish_reason: Option<String>,
}

#[derive(Deserialize)]
struct ChoiceMessage {
    content: Option<String>,
}

#[async_trait]
impl LlmProvider for OpenAiCompatibleProvider {
    fn info(&self) -> ProviderInfo {
        self.info.clone().unwrap_or_else(|| ProviderInfo {
            endpoint: Some(self.endpoint.clone()),
            ..ProviderInfo::new("openai_compatible", self.model.clone())
        })
    }

    async fn complete(&self, request: CompletionRequest) -> Result<CompletionResponse, LlmError> {
        let body = ChatBody {
            model: &self.model,
            messages: request.messages.iter().map(WireMessage::from).collect(),
            temperature: request.temperature,
            max_tokens: request.max_tokens,
            seed: request.seed,
            response_format: (request.json_output && self.json_mode).then(|| json!({"type": "json_object"})),
            extra: &self.extra_body,
        };

        let mut http = self.client.post(&self.endpoint).json(&body);
        if let Some(key) = &self.api_key {
            http = http.bearer_auth(key);
        }
        let resp = http.send().await.map_err(|e| LlmError::Transport(e.to_string()))?;
        let status = resp.status();
        let text = resp.text().await.map_err(|e| LlmError::Transport(e.to_string()))?;
        if !status.is_success() {
            // Some servers (e.g. Strata) reject JSON cut off at the limit.
            if text.contains("finish_reason=length") {
                return Err(LlmError::Truncated);
            }
            let body: String = text.chars().take(500).collect();
            return Err(LlmError::Status { status: status.as_u16(), body });
        }

        let parsed: ChatResponse = serde_json::from_str(&text).map_err(|e| LlmError::Decode(e.to_string()))?;
        let choice = parsed.choices.into_iter().next().ok_or(LlmError::Empty)?;
        if request.json_output && choice.finish_reason.as_deref() == Some("length") {
            return Err(LlmError::Truncated);
        }
        let content = choice.message.content.filter(|c| !c.trim().is_empty()).ok_or(LlmError::Empty)?;
        Ok(CompletionResponse {
            content,
            model: parsed.model,
            usage: parsed.usage,
            finish_reason: choice.finish_reason,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ImageData;

    #[test]
    fn wire_format_text_and_image() {
        let plain = serde_json::to_value(WireMessage::from(&Message::user("hi"))).unwrap();
        assert_eq!(plain, json!({"role": "user", "content": "hi"}));

        let with_img = Message::user("look").with_image(ImageData::png(vec![1, 2, 3]));
        let v = serde_json::to_value(WireMessage::from(&with_img)).unwrap();
        assert_eq!(v["content"][0], json!({"type": "text", "text": "look"}));
        assert_eq!(v["content"][1]["image_url"]["url"], "data:image/png;base64,AQID");
    }
}
