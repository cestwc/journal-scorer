//! Domain layer: rubric models, evidence, deterministic scoring and text
//! segmentation. Contains no I/O and no LLM code.

pub mod error;
pub mod model;
pub mod rubric;
pub mod scoring;
pub mod segment;

pub use error::CoreError;
pub use model::*;
pub use rubric::Rubric;
