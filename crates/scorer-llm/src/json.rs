//! Tolerant extraction of a JSON object from model output: strips
//! `<think>` reasoning blocks and Markdown code fences, then falls back to the
//! outermost `{ ... }`.

use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::LlmError;

pub fn parse_json<T: DeserializeOwned>(raw: &str) -> Result<T, LlmError> {
    let value = extract_json(raw)?;
    serde_json::from_value(value).map_err(|e| invalid(raw, e.to_string()))
}

pub fn extract_json(raw: &str) -> Result<Value, LlmError> {
    let text = strip_reasoning(raw);
    let text = text.trim();

    if let Ok(v) = serde_json::from_str(text) {
        return Ok(v);
    }
    if let Some(inner) = fenced_block(text) {
        if let Ok(v) = serde_json::from_str(inner) {
            return Ok(v);
        }
    }
    if let (Some(a), Some(b)) = (text.find('{'), text.rfind('}')) {
        if a < b {
            return serde_json::from_str(&text[a..=b]).map_err(|e| invalid(raw, e.to_string()));
        }
    }
    Err(invalid(raw, "no JSON object found".into()))
}

/// Removes `<think>...</think>` reasoning that some models put before the answer.
pub fn strip_reasoning(raw: &str) -> String {
    // Everything up to the last closing tag is reasoning (handles templates
    // that emit only the closing tag).
    let after = match raw.rfind("</think>") {
        Some(i) => &raw[i + "</think>".len()..],
        None => raw,
    };
    after.to_owned()
}

fn fenced_block(text: &str) -> Option<&str> {
    let start = text.find("```")?;
    let rest = &text[start + 3..];
    let rest = rest.strip_prefix("json").unwrap_or(rest);
    let end = rest.find("```")?;
    Some(rest[..end].trim())
}

fn invalid(raw: &str, reason: String) -> LlmError {
    let snippet: String = raw.chars().take(200).collect();
    LlmError::InvalidJson { reason, snippet }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain() {
        assert_eq!(extract_json(r#"{"a":1}"#).unwrap()["a"], 1);
    }

    #[test]
    fn think_and_fence() {
        let raw = "<think>let me see {not json}</think>\nHere:\n```json\n{\"a\": 2}\n```";
        assert_eq!(extract_json(raw).unwrap()["a"], 2);
    }

    #[test]
    fn surrounded_by_prose() {
        assert_eq!(extract_json("Sure! {\"a\": [3]} Hope that helps.").unwrap()["a"][0], 3);
    }

    #[test]
    fn garbage() {
        assert!(matches!(extract_json("no json here"), Err(LlmError::InvalidJson { .. })));
    }
}
