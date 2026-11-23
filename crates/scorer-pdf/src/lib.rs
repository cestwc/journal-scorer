//! PDF layer (MuPDF):
//!
//! * [`extract`]: text with per-character positions -> spans with page
//!   regions, plus figures rendered to PNG.
//! * [`annotate`]: writes highlight/box annotations, margin tags and an
//!   appended score sheet into a copy of the original PDF.
//!
//! All MuPDF objects stay inside these synchronous functions; callers in
//! async code should run them on a blocking thread.

pub mod annotate;
pub mod error;
pub mod extract;
mod geometry;
mod sheet;
mod style;

pub use annotate::{annotate, AnnotateInput, SheetMeta};
pub use error::PdfError;
pub use extract::{extract, ExtractOptions};
