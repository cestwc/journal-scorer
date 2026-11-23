//! Application configuration: a TOML file plus CLI overrides.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use scorer_core::Rubric;
use scorer_engine::ScorerOptions;
use scorer_llm::config::{MockConfig, OpenAiCompatibleConfig};
use scorer_llm::ProviderConfig;
use serde::Deserialize;

use crate::cli::GlobalArgs;

const DEFAULT_CONFIG_FILE: &str = "journal-scorer.toml";

#[derive(Debug, Deserialize)]
pub struct AppConfig {
    pub llm: ProviderConfig,
    #[serde(default)]
    pub scorer: ScorerOptions,
    #[serde(default)]
    pub rubric: RubricConfig,
    #[serde(default)]
    pub server: ServerConfig,
}

#[derive(Debug, Default, Deserialize)]
pub struct RubricConfig {
    pub path: Option<PathBuf>,
}

#[derive(Debug, Deserialize)]
pub struct ServerConfig {
    pub addr: String,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self { addr: "127.0.0.1:3000".into() }
    }
}

/// Reads the config file if one is given or present, then applies CLI
/// overrides. Without a file, `--mock` or `--base-url` + `--model` suffice.
pub fn load(args: &GlobalArgs) -> Result<AppConfig> {
    let path = args.config.clone().or_else(|| {
        let p = PathBuf::from(DEFAULT_CONFIG_FILE);
        p.exists().then_some(p)
    });
    let file = match &path {
        Some(p) => Some(read_config(p)?),
        None => None,
    };

    let llm = resolve_llm(file.as_ref().map(|f| &f.llm), args)?;
    let mut cfg = match file {
        Some(f) => AppConfig { llm, ..f },
        None => AppConfig {
            llm,
            scorer: ScorerOptions::default(),
            rubric: RubricConfig::default(),
            server: ServerConfig::default(),
        },
    };
    if let Some(r) = &args.rubric {
        cfg.rubric.path = Some(r.clone());
    }
    Ok(cfg)
}

fn read_config(path: &Path) -> Result<AppConfig> {
    let text = std::fs::read_to_string(path).with_context(|| format!("reading config {}", path.display()))?;
    toml::from_str(&text).with_context(|| format!("parsing config {}", path.display()))
}

fn resolve_llm(from_file: Option<&ProviderConfig>, args: &GlobalArgs) -> Result<ProviderConfig> {
    if args.mock {
        return Ok(ProviderConfig::Mock(MockConfig::default()));
    }
    let mut base = match from_file {
        Some(ProviderConfig::OpenaiCompatible(c)) => Some(c.clone()),
        Some(other) if args.base_url.is_none() && args.model.is_none() => return Ok(other.clone()),
        _ => None,
    };
    if base.is_none() {
        let (Some(base_url), Some(model)) = (&args.base_url, &args.model) else {
            bail!(
                "no LLM provider configured: create {DEFAULT_CONFIG_FILE} (see journal-scorer.example.toml), \
                 pass --base-url and --model, or use --mock"
            );
        };
        base = Some(OpenAiCompatibleConfig {
            base_url: base_url.clone(),
            model: model.clone(),
            api_key_env: None,
            timeout_secs: 300,
            json_mode: true,
            extra_body: Default::default(),
        });
    }
    let mut c = base.expect("set above");
    if let Some(v) = &args.base_url {
        c.base_url = v.clone();
    }
    if let Some(v) = &args.model {
        c.model = v.clone();
    }
    if let Some(v) = &args.api_key_env {
        c.api_key_env = Some(v.clone());
    }
    Ok(ProviderConfig::OpenaiCompatible(c))
}

/// Built-in rubric unless a path is given.
pub fn load_rubric(path: Option<&Path>) -> Result<Rubric> {
    match path {
        None => Ok(Rubric::builtin_ai_journal()),
        Some(p) => {
            let text = std::fs::read_to_string(p).with_context(|| format!("reading rubric {}", p.display()))?;
            Rubric::from_toml_str(&text).with_context(|| format!("loading rubric {}", p.display()))
        }
    }
}
