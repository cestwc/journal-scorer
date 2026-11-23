//! Provider-agnostic LLM layer.
//!
//! Callers depend only on [`LlmProvider`]; concrete adapters live in
//! [`providers`] and are selected at runtime from a [`ProviderConfig`].

pub mod config;
pub mod error;
pub mod json;
pub mod provider;
pub mod providers;
pub mod types;

pub use config::ProviderConfig;
pub use error::LlmError;
pub use provider::{LlmProvider, ProviderInfo};
pub use providers::build_provider;
pub use types::{CompletionRequest, CompletionResponse, ImageData, Message, Role, Usage};
