//! Scoring pipeline shared by every entry point (CLI, web, future editors):
//!
//! document -> figure descriptions (vision) -> spans -> LLM classification (per batch)
//!      -> validated `Evidence` -> deterministic `ScoreResult`
//!
//! The LLM only labels evidence; all arithmetic happens in `scorer_core`.

pub mod classification;
pub mod error;
pub mod pipeline;
pub mod progress;
pub mod prompt;
pub mod report;

pub use error::EngineError;
pub use pipeline::{Scorer, ScorerOptions};
pub use progress::{no_progress, Progress, ProgressFn, Stage};
pub use report::{RunStats, ScoreReport};
