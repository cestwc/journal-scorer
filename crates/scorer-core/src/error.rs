use thiserror::Error;

#[derive(Debug, Error)]
pub enum CoreError {
    #[error("failed to parse rubric: {0}")]
    RubricParse(#[from] toml::de::Error),
    #[error("invalid rubric: {0}")]
    RubricInvalid(String),
    #[error("unknown strength `{0}` (expected weak, moderate or strong)")]
    UnknownStrength(String),
}
