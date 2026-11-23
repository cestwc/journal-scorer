use super::{BBox, TextSpan};

/// A figure (screenshot, diagram) found in a PDF, rendered to PNG.
#[derive(Debug, Clone, PartialEq)]
pub struct Figure {
    /// 1-based page number.
    pub page: u32,
    pub bbox: BBox,
    /// Byte offset in `Document::text` where the figure sits in reading order.
    pub anchor: usize,
    pub png: Vec<u8>,
}

/// Everything the scoring pipeline needs about one input document.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Document {
    /// Plain text of the document (what span offsets refer to).
    pub text: String,
    /// Text spans in reading order.
    pub spans: Vec<TextSpan>,
    pub figures: Vec<Figure>,
}
