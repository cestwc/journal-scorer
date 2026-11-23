//! Provider-neutral request/response types.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
}

/// An image attached to a message (for vision-capable models).
#[derive(Debug, Clone, PartialEq)]
pub struct ImageData {
    /// MIME type, e.g. `image/png`.
    pub mime: String,
    pub bytes: Vec<u8>,
}

impl ImageData {
    pub fn png(bytes: Vec<u8>) -> Self {
        Self { mime: "image/png".into(), bytes }
    }
}

/// A chat message. Adapters translate it to their wire format.
#[derive(Debug, Clone, PartialEq)]
pub struct Message {
    pub role: Role,
    pub content: String,
    pub images: Vec<ImageData>,
}

impl Message {
    pub fn new(role: Role, content: impl Into<String>) -> Self {
        Self { role, content: content.into(), images: Vec::new() }
    }
    pub fn system(content: impl Into<String>) -> Self {
        Self::new(Role::System, content)
    }
    pub fn user(content: impl Into<String>) -> Self {
        Self::new(Role::User, content)
    }
    pub fn assistant(content: impl Into<String>) -> Self {
        Self::new(Role::Assistant, content)
    }
    pub fn with_image(mut self, image: ImageData) -> Self {
        self.images.push(image);
        self
    }
}

#[derive(Debug, Clone, Default)]
pub struct CompletionRequest {
    pub messages: Vec<Message>,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
    pub seed: Option<u64>,
    /// Ask the provider to constrain output to a JSON object, if it can.
    pub json_output: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Usage {
    pub prompt_tokens: Option<u32>,
    pub completion_tokens: Option<u32>,
}

#[derive(Debug, Clone, Default)]
pub struct CompletionResponse {
    pub content: String,
    pub model: Option<String>,
    pub usage: Option<Usage>,
    pub finish_reason: Option<String>,
}
