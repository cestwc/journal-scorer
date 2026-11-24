//! Reuse an OpenCode installation's model setup: provider addresses and
//! model ids from `opencode.json(c)`, keys from `auth.json` (written by
//! `opencode auth login`), and capabilities/limits from OpenCode's model
//! catalog cache. OpenCode itself is not run; requests go straight to the
//! provider through the OpenAI-compatible adapter.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::LlmError;

/// AI SDK packages whose providers speak OpenAI `/chat/completions`.
const OPENAI_STYLE: &[&str] = &["@ai-sdk/openai-compatible", "@openrouter/ai-sdk-provider", "@ai-sdk/openai"];

fn default_timeout() -> u64 {
    300
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OpencodeConfig {
    /// `provider/model` as listed by `opencode models` (e.g. `strata/strata`).
    /// Default: the `model` set in OpenCode's config.
    #[serde(default)]
    pub model: Option<String>,
    /// Override `~/.config/opencode`.
    #[serde(default)]
    pub config_dir: Option<PathBuf>,
    /// Override `~/.local/share/opencode` (holds `auth.json`).
    #[serde(default)]
    pub data_dir: Option<PathBuf>,
    /// Override `~/.cache/opencode` (holds `models.json`).
    #[serde(default)]
    pub cache_dir: Option<PathBuf>,
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
}

impl Default for OpencodeConfig {
    fn default() -> Self {
        Self { model: None, config_dir: None, data_dir: None, cache_dir: None, timeout_secs: default_timeout() }
    }
}

/// A model resolved to everything needed to call it.
#[derive(Debug, Clone)]
pub struct ResolvedModel {
    /// `provider/model`, as OpenCode names it.
    pub spec: String,
    pub name: String,
    pub base_url: String,
    /// Model id sent to the API.
    pub api_model: String,
    pub api_key: Option<String>,
    /// Accepts image input (`None` = unknown).
    pub vision: Option<bool>,
    /// Hard output-token limit from the catalog (`None` = unknown).
    pub max_output: Option<u32>,
}

/// One row of [`OpencodeSetup::list`].
#[derive(Debug, Clone, Serialize)]
pub struct ModelEntry {
    pub spec: String,
    pub name: String,
    pub vision: Option<bool>,
    pub context: Option<u64>,
    /// USD per million input / output tokens.
    pub cost: Option<(f64, f64)>,
    /// Why the scorer cannot use it, if it cannot.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unusable: Option<String>,
}

/// OpenCode's files, loaded once.
pub struct OpencodeSetup {
    config: Map<String, Value>,
    auth: Map<String, Value>,
    catalog: Map<String, Value>,
    config_dir: PathBuf,
}

impl OpencodeSetup {
    pub fn load(cfg: &OpencodeConfig) -> Result<Self, LlmError> {
        let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
        let xdg = |var: &str, fallback: &str| {
            std::env::var_os(var).map(PathBuf::from).unwrap_or_else(|| home.join(fallback)).join("opencode")
        };
        let config_dir = cfg.config_dir.clone().unwrap_or_else(|| xdg("XDG_CONFIG_HOME", ".config"));
        let data_dir = cfg.data_dir.clone().unwrap_or_else(|| xdg("XDG_DATA_HOME", ".local/share"));
        let cache_dir = cfg.cache_dir.clone().unwrap_or_else(|| xdg("XDG_CACHE_HOME", ".cache"));

        let config = ["opencode.jsonc", "opencode.json", "config.json"]
            .iter()
            .map(|f| config_dir.join(f))
            .find(|p| p.exists())
            .map(|p| read_object(&p, true))
            .transpose()?
            .unwrap_or_default();
        let auth = optional_object(&data_dir.join("auth.json"))?;
        let catalog = optional_object(&cache_dir.join("models.json"))?;
        if config.is_empty() && auth.is_empty() && catalog.is_empty() {
            return Err(LlmError::Config(format!(
                "no OpenCode setup found (looked in {} and {})",
                config_dir.display(),
                data_dir.display()
            )));
        }
        Ok(Self { config, auth, catalog, config_dir })
    }

    /// The `model` set in OpenCode's config, if any.
    pub fn default_model(&self) -> Option<String> {
        self.config.get("model").and_then(Value::as_str).map(str::to_owned)
    }

    pub fn resolve(&self, spec: &str) -> Result<ResolvedModel, LlmError> {
        let cfg_err = |m: String| LlmError::Config(m);
        let (pid, mid) = spec
            .split_once('/')
            .ok_or_else(|| cfg_err(format!("model `{spec}` must be `provider/model` (see `journal-scorer models`)")))?;

        let user = self.config.get("provider").and_then(|p| p.get(pid));
        let cat = self.catalog.get(pid);
        if user.is_none() && cat.is_none() {
            return Err(cfg_err(format!("OpenCode has no provider `{pid}`")));
        }
        let user_model = user.and_then(|p| p.get("models")).and_then(|m| m.get(mid));
        let cat_model = cat.and_then(|p| p.get("models")).and_then(|m| m.get(mid));
        let model = user_model.or(cat_model).ok_or_else(|| cfg_err(format!("provider `{pid}` has no model `{mid}`")))?;

        // Protocol: per-model override, then the provider's package.
        let npm = cat_model
            .and_then(|m| m.pointer("/provider/npm"))
            .or_else(|| user.and_then(|p| p.get("npm")))
            .or_else(|| cat.and_then(|p| p.get("npm")))
            .and_then(Value::as_str)
            .unwrap_or("@ai-sdk/openai-compatible");
        if !OPENAI_STYLE.contains(&npm) {
            return Err(cfg_err(format!(
                "`{spec}` uses the {npm} protocol; only OpenAI-compatible providers are supported so far"
            )));
        }

        let option = |key: &str| user.and_then(|p| p.pointer(&format!("/options/{key}"))).and_then(Value::as_str);
        let base_url = option("baseURL")
            .map(|s| self.substitute(s))
            .transpose()?
            .or_else(|| cat.and_then(|p| p.get("api")).and_then(Value::as_str).map(str::to_owned))
            .or_else(|| (npm == "@ai-sdk/openai").then(|| "https://api.openai.com/v1".to_owned()))
            .ok_or_else(|| cfg_err(format!("no base URL known for provider `{pid}`")))?;

        let api_key = match option("apiKey") {
            Some(k) => Some(self.substitute(k)?),
            None => self.stored_key(pid)?.or_else(|| {
                cat.and_then(|p| p.get("env"))
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .find_map(|var| std::env::var(var).ok())
            }),
        };

        Ok(ResolvedModel {
            spec: spec.to_owned(),
            name: model.get("name").and_then(Value::as_str).unwrap_or(mid).to_owned(),
            base_url,
            api_model: model.get("id").and_then(Value::as_str).unwrap_or(mid).to_owned(),
            api_key,
            vision: vision(model),
            // Only the catalog's limits are model limits; limits in the user's
            // own provider entries are OpenCode agent settings.
            max_output: if user_model.is_none() {
                cat_model.and_then(|m| m.pointer("/limit/output")).and_then(Value::as_u64).map(|v| v as u32)
            } else {
                None
            },
        })
    }

    /// Models from providers the user has configured or holds a key for.
    pub fn list(&self) -> Vec<ModelEntry> {
        let mut pids: Vec<&str> = self
            .config
            .get("provider")
            .and_then(Value::as_object)
            .map(|p| p.keys().map(String::as_str).collect())
            .unwrap_or_default();
        pids.extend(self.auth.keys().map(String::as_str));
        pids.sort_unstable();
        pids.dedup();

        let mut out = Vec::new();
        for pid in pids {
            let mut mids: Vec<String> = [self.config.get("provider").and_then(|p| p.get(pid)), self.catalog.get(pid)]
                .into_iter()
                .flatten()
                .filter_map(|p| p.get("models").and_then(Value::as_object))
                .flat_map(|m| m.keys().cloned())
                .collect();
            mids.sort();
            mids.dedup();
            for mid in mids {
                let spec = format!("{pid}/{mid}");
                let model = self
                    .config
                    .get("provider")
                    .and_then(|p| p.pointer(&format!("/{pid}/models/{}", escape_pointer(&mid))))
                    .or_else(|| self.catalog.get(pid).and_then(|p| p.get("models")).and_then(|m| m.get(&mid)));
                let unusable = self.resolve(&spec).err().map(|e| e.to_string());
                out.push(ModelEntry {
                    name: model.and_then(|m| m.get("name")).and_then(Value::as_str).unwrap_or(&mid).to_owned(),
                    vision: model.and_then(vision),
                    context: model.and_then(|m| m.pointer("/limit/context")).and_then(Value::as_u64),
                    cost: model.and_then(|m| {
                        Some((m.pointer("/cost/input")?.as_f64()?, m.pointer("/cost/output")?.as_f64()?))
                    }),
                    unusable,
                    spec,
                });
            }
        }
        out
    }

    fn stored_key(&self, pid: &str) -> Result<Option<String>, LlmError> {
        let Some(entry) = self.auth.get(pid) else { return Ok(None) };
        match entry.get("type").and_then(Value::as_str) {
            Some("api") => Ok(entry.get("key").and_then(Value::as_str).map(str::to_owned)),
            Some(other) => Err(LlmError::Config(format!(
                "provider `{pid}` is signed in with `{other}` in OpenCode, which only OpenCode itself can use"
            ))),
            None => Ok(None),
        }
    }

    /// OpenCode's `{env:NAME}` and `{file:path}` substitutions.
    fn substitute(&self, s: &str) -> Result<String, LlmError> {
        let mut out = String::new();
        let mut rest = s;
        while let Some(start) = rest.find('{') {
            let Some(len) = rest[start..].find('}') else { break };
            let token = &rest[start + 1..start + len];
            out.push_str(&rest[..start]);
            if let Some(var) = token.strip_prefix("env:") {
                out.push_str(&std::env::var(var).unwrap_or_default());
            } else if let Some(path) = token.strip_prefix("file:") {
                let path = expand_path(path, &self.config_dir);
                let text = std::fs::read_to_string(&path)
                    .map_err(|e| LlmError::Config(format!("{{file:{}}}: {e}", path.display())))?;
                out.push_str(text.trim());
            } else {
                out.push_str(&rest[start..=start + len]);
            }
            rest = &rest[start + len + 1..];
        }
        out.push_str(rest);
        Ok(out)
    }
}

fn vision(model: &Value) -> Option<bool> {
    if let Some(inputs) = model.pointer("/modalities/input").and_then(Value::as_array) {
        return Some(inputs.iter().any(|v| v == "image"));
    }
    model.get("attachment").and_then(Value::as_bool)
}

fn escape_pointer(s: &str) -> String {
    s.replace('~', "~0").replace('/', "~1")
}

fn expand_path(p: &str, base: &Path) -> PathBuf {
    if let Some(rest) = p.strip_prefix("~/") {
        std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default().join(rest)
    } else {
        base.join(p)
    }
}

fn read_object(path: &Path, jsonc: bool) -> Result<Map<String, Value>, LlmError> {
    let text = std::fs::read_to_string(path).map_err(|e| LlmError::Config(format!("{}: {e}", path.display())))?;
    let value: Value = if jsonc {
        json5::from_str(&text).map_err(|e| LlmError::Config(format!("{}: {e}", path.display())))?
    } else {
        serde_json::from_str(&text).map_err(|e| LlmError::Config(format!("{}: {e}", path.display())))?
    };
    match value {
        Value::Object(m) => Ok(m),
        _ => Err(LlmError::Config(format!("{}: expected a JSON object", path.display()))),
    }
}

fn optional_object(path: &Path) -> Result<Map<String, Value>, LlmError> {
    if path.exists() {
        read_object(path, false)
    } else {
        Ok(Map::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup(dir: &Path) -> OpencodeSetup {
        std::fs::create_dir_all(dir.join("cfg")).unwrap();
        std::fs::create_dir_all(dir.join("data")).unwrap();
        std::fs::create_dir_all(dir.join("cache")).unwrap();
        std::fs::write(
            dir.join("cfg/opencode.jsonc"),
            r#"{
              // comment
              "model": "local/qwen",
              "provider": {
                "local": {
                  "npm": "@ai-sdk/openai-compatible",
                  "options": { "baseURL": "http://127.0.0.1:8080/v1", "apiKey": "{file:key.txt}" },
                  "models": { "qwen": { "name": "Qwen", "limit": { "output": 4096 } }, },
                },
              },
            }"#,
        )
        .unwrap();
        std::fs::write(dir.join("cfg/key.txt"), "secret-from-file\n").unwrap();
        std::fs::write(dir.join("data/auth.json"), r#"{"router": {"type": "api", "key": "rk"}, "sub": {"type": "oauth"}}"#).unwrap();
        std::fs::write(
            dir.join("cache/models.json"),
            r#"{
              "router": {"api": "https://router.example/v1", "npm": "@openrouter/ai-sdk-provider", "env": [],
                "models": {"vendor/big": {"name": "Big", "modalities": {"input": ["text"]}, "limit": {"context": 1000, "output": 8000}, "cost": {"input": 1, "output": 2}},
                           "vendor/other": {"name": "Other", "provider": {"npm": "@ai-sdk/anthropic"}}}},
              "sub": {"api": "https://sub.example/v1", "npm": "@ai-sdk/openai-compatible", "models": {"m": {"name": "M"}}}
            }"#,
        )
        .unwrap();
        OpencodeSetup::load(&OpencodeConfig {
            config_dir: Some(dir.join("cfg")),
            data_dir: Some(dir.join("data")),
            cache_dir: Some(dir.join("cache")),
            ..Default::default()
        })
        .unwrap()
    }

    #[test]
    fn resolves_user_and_catalog_models() {
        let dir = std::env::temp_dir().join(format!("oc-test-{}", std::process::id()));
        let s = setup(&dir);
        assert_eq!(s.default_model().as_deref(), Some("local/qwen"));

        let local = s.resolve("local/qwen").unwrap();
        assert_eq!(local.base_url, "http://127.0.0.1:8080/v1");
        assert_eq!(local.api_key.as_deref(), Some("secret-from-file"));
        assert_eq!(local.max_output, None, "user limits are not hard limits");

        let big = s.resolve("router/vendor/big").unwrap();
        assert_eq!((big.base_url.as_str(), big.api_model.as_str()), ("https://router.example/v1", "vendor/big"));
        assert_eq!(big.api_key.as_deref(), Some("rk"));
        assert_eq!((big.vision, big.max_output), (Some(false), Some(8000)));

        assert!(s.resolve("router/vendor/other").unwrap_err().to_string().contains("@ai-sdk/anthropic"));
        assert!(s.resolve("sub/m").unwrap_err().to_string().contains("oauth"));
        assert!(s.resolve("nope").is_err());

        let listed: Vec<String> = s.list().into_iter().map(|m| m.spec).collect();
        assert!(listed.contains(&"router/vendor/big".to_owned()) && listed.contains(&"local/qwen".to_owned()));
        std::fs::remove_dir_all(dir).ok();
    }
}
