use serde::{Deserialize, Serialize};

use super::{EvidenceId, SpanId, Strength, SubitemId};

/// Axis-aligned rectangle in PDF user-space coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct BBox {
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
}

/// One rectangle on one PDF page (MuPDF page space: points, origin top-left).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PdfRegion {
    /// 1-based page number.
    pub page: u32,
    pub bbox: BBox,
}

/// What a span is made of.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SpanKind {
    /// Text taken from the document.
    #[default]
    Text,
    /// A figure/screenshot; `text` is a model-generated description of it.
    Image,
}

impl SpanKind {
    fn is_text(&self) -> bool {
        *self == SpanKind::Text
    }
}

/// A unit of the document that the LLM classifies. This is the single source
/// of truth shared by the LLM prompt and the PDF annotation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextSpan {
    pub id: SpanId,
    #[serde(default, skip_serializing_if = "SpanKind::is_text")]
    pub kind: SpanKind,
    /// Whitespace-normalised text as shown to the LLM.
    pub text: String,
    /// Byte offsets into the document text (`start == end` for images,
    /// marking where the image sits in reading order).
    pub start: usize,
    pub end: usize,
    /// Where the span is on the PDF pages; empty for plain-text input.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub regions: Vec<PdfRegion>,
}

impl TextSpan {
    pub fn text(id: SpanId, text: String, start: usize, end: usize) -> Self {
        Self { id, kind: SpanKind::Text, text, start, end, regions: Vec::new() }
    }

    /// First page the span appears on, if known.
    pub fn page(&self) -> Option<u32> {
        self.regions.first().map(|r| r.page)
    }
}

/// One span's support for one subitem.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Support {
    pub subitem: SubitemId,
    pub strength: Strength,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// A span judged to be rubric evidence, multi-labelled with the subitems it
/// supports. Invariant: at most one `Support` per subitem.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Evidence {
    pub id: EvidenceId,
    pub span: TextSpan,
    pub supports: Vec<Support>,
}

impl Evidence {
    /// Builds evidence and enforces the one-strength-per-subitem invariant.
    pub fn new(id: EvidenceId, span: TextSpan, supports: Vec<Support>) -> Self {
        let mut e = Self { id, span, supports };
        e.dedup_supports();
        e
    }

    /// Collapses duplicate subitems, keeping the strongest. Returns how many
    /// duplicates were removed.
    pub fn dedup_supports(&mut self) -> usize {
        let before = self.supports.len();
        let mut out: Vec<Support> = Vec::with_capacity(before);
        for s in self.supports.drain(..) {
            match out.iter_mut().find(|o| o.subitem == s.subitem) {
                Some(existing) if s.strength > existing.strength => *existing = s,
                Some(_) => {}
                None => out.push(s),
            }
        }
        self.supports = out;
        before - self.supports.len()
    }

    pub fn support_for(&self, subitem: &SubitemId) -> Option<&Support> {
        self.supports.iter().find(|s| &s.subitem == subitem)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn span() -> TextSpan {
        TextSpan::text("S1".into(), "x".into(), 0, 1)
    }

    fn sup(id: &str, strength: Strength) -> Support {
        Support { subitem: id.into(), strength, confidence: None, reason: None }
    }

    #[test]
    fn dedup_keeps_strongest() {
        let e = Evidence::new(
            "E1".into(),
            span(),
            vec![sup("D1", Strength::Weak), sup("E1", Strength::Moderate), sup("D1", Strength::Strong)],
        );
        assert_eq!(e.supports.len(), 2);
        assert_eq!(e.support_for(&"D1".into()).unwrap().strength, Strength::Strong);
    }
}
