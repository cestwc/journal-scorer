//! Model selection: one scorer per model, built on first use.
//!
//! With an OpenCode provider, any usable model from the OpenCode setup can
//! be chosen per request (`provider/model`); otherwise the configured model
//! is the only one.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use scorer_core::Rubric;
use scorer_engine::{Scorer, ScorerOptions};
use scorer_llm::opencode::{ModelEntry, OpencodeConfig, OpencodeSetup};
use scorer_llm::{LlmProvider, ProviderConfig};
use serde::Serialize;

use crate::config::{self, AppConfig};

/// Why a model could not be selected.
#[derive(Debug, thiserror::Error)]
pub enum ModelError {
    /// The model name was rejected (unknown, unsupported, fixed server).
    #[error("{0}")]
    Invalid(String),
}

/// Answer of `GET /api/models`.
#[derive(Debug, Serialize)]
pub struct ModelList {
    /// Model used when a request names none.
    pub default: String,
    /// Whether requests may name another model.
    pub switchable: bool,
    /// Usable models, sorted by spec. Contains at least the default.
    pub models: Vec<ModelEntry>,
}

pub struct ModelRegistry {
    default_spec: String,
    default: Arc<Scorer>,
    /// Set when models come from OpenCode and can be switched.
    opencode: Option<OpencodeConfig>,
    rubric: Arc<Rubric>,
    options: ScorerOptions,
    cache: Mutex<HashMap<String, Arc<Scorer>>>,
}

impl ModelRegistry {
    pub fn from_config(cfg: &AppConfig) -> Result<Self> {
        let rubric = Arc::new(config::load_rubric(cfg.rubric.path.as_deref())?);
        let provider = scorer_llm::build_provider(&cfg.llm).context("building LLM provider")?;
        let default = Arc::new(make_scorer(provider, rubric.clone(), cfg.scorer.clone()));
        let opencode = match &cfg.llm {
            ProviderConfig::Opencode(c) => Some(c.clone()),
            _ => None,
        };
        Ok(Self { opencode, rubric, options: cfg.scorer.clone(), ..Self::fixed(default) })
    }

    /// A registry with a single, unswitchable model.
    pub fn fixed(scorer: Arc<Scorer>) -> Self {
        Self {
            default_spec: scorer.provider_info().model,
            rubric: Arc::new(scorer.rubric().clone()),
            options: ScorerOptions::default(),
            default: scorer,
            opencode: None,
            cache: Mutex::default(),
        }
    }

    pub fn default_scorer(&self) -> Arc<Scorer> {
        self.default.clone()
    }

    /// The scorer for `spec`, or the default one when `spec` is empty.
    pub fn get(&self, spec: Option<&str>) -> Result<Arc<Scorer>, ModelError> {
        let spec = match spec.map(str::trim) {
            None | Some("") => return Ok(self.default.clone()),
            Some(s) if s == self.default_spec => return Ok(self.default.clone()),
            Some(s) => s,
        };
        let Some(oc) = &self.opencode else {
            return Err(ModelError::Invalid(format!(
                "this server only uses `{}`; switching models needs `kind = \"opencode\"`",
                self.default_spec
            )));
        };
        if let Some(s) = self.cache.lock().unwrap().get(spec) {
            return Ok(s.clone());
        }
        let cfg = ProviderConfig::Opencode(OpencodeConfig { model: Some(spec.to_owned()), ..oc.clone() });
        let provider = scorer_llm::build_provider(&cfg).map_err(|e| ModelError::Invalid(e.to_string()))?;
        let scorer = Arc::new(make_scorer(provider, self.rubric.clone(), self.options.clone()));
        Ok(self.cache.lock().unwrap().entry(spec.to_owned()).or_insert(scorer).clone())
    }

    /// Re-reads OpenCode's files, so newly added providers and keys show up.
    pub fn list(&self) -> ModelList {
        let mut models: Vec<ModelEntry> = match &self.opencode {
            Some(oc) => match OpencodeSetup::load(oc) {
                Ok(setup) => setup.list().into_iter().filter(|m| m.unusable.is_none()).collect(),
                Err(e) => {
                    tracing::warn!(error = %e, "could not read the OpenCode setup");
                    Vec::new()
                }
            },
            None => Vec::new(),
        };
        if !models.iter().any(|m| m.spec == self.default_spec) {
            let info = self.default.provider_info();
            models.push(ModelEntry {
                spec: self.default_spec.clone(),
                name: info.model.clone(),
                provider: info.kind.clone(),
                vision: info.vision,
                context: None,
                cost: None,
                unusable: None,
            });
        }
        models.sort_by(|a, b| a.spec.cmp(&b.spec));
        ModelList { default: self.default_spec.clone(), switchable: self.opencode.is_some(), models }
    }
}

/// Wraps a provider in a scorer whose options respect the model's limits.
pub fn make_scorer(provider: Arc<dyn LlmProvider>, rubric: Arc<Rubric>, mut options: ScorerOptions) -> Scorer {
    let info = provider.info();
    if let Some(max) = info.max_output {
        options.max_tokens = Some(options.max_tokens.map_or(max, |m| m.min(max)));
        options.figure_max_tokens = options.figure_max_tokens.min(max);
    }
    if info.vision == Some(false) && options.describe_figures {
        tracing::warn!(model = %info.model, "model does not accept images; screenshots will be skipped");
        options.describe_figures = false;
    }
    tracing::info!(provider = %info.kind, model = %info.model, rubric = %rubric.id, "scorer ready");
    Scorer::new(provider, rubric).with_options(options)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{RubricConfig, ServerConfig};

    fn opencode_dir() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("registry-test-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("cfg")).unwrap();
        std::fs::write(
            dir.join("cfg/opencode.json"),
            r#"{"provider": {"local": {"npm": "@ai-sdk/openai-compatible",
                 "options": {"baseURL": "http://127.0.0.1:9/v1"},
                 "models": {"a": {"name": "A"}, "b": {"name": "B", "modalities": {"input": ["text"]}}}}}}"#,
        )
        .unwrap();
        dir
    }

    #[test]
    fn switches_between_opencode_models() {
        let dir = opencode_dir();
        let oc = OpencodeConfig {
            model: Some("local/a".into()),
            config_dir: Some(dir.join("cfg")),
            data_dir: Some(dir.join("data")),
            cache_dir: Some(dir.join("cache")),
            ..Default::default()
        };
        let cfg = AppConfig {
            llm: ProviderConfig::Opencode(oc),
            scorer: ScorerOptions::default(),
            rubric: RubricConfig::default(),
            server: ServerConfig::default(),
        };
        let reg = ModelRegistry::from_config(&cfg).unwrap();

        let list = reg.list();
        assert_eq!((list.default.as_str(), list.switchable), ("local/a", true));
        let specs: Vec<_> = list.models.iter().map(|m| m.spec.as_str()).collect();
        assert_eq!(specs, ["local/a", "local/b"]);

        assert!(Arc::ptr_eq(&reg.get(None).unwrap(), &reg.get(Some("local/a")).unwrap()));
        let b = reg.get(Some("local/b")).unwrap();
        assert_eq!(b.provider_info().model, "local/b");
        assert!(Arc::ptr_eq(&b, &reg.get(Some("local/b")).unwrap()), "scorers are cached");
        assert!(reg.get(Some("local/zzz")).err().unwrap().to_string().contains("no model"));
        std::fs::remove_dir_all(dir).ok();
    }
}
