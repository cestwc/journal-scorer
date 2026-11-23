//! Writes the scoring result into a copy of the PDF as real annotations.

use std::collections::BTreeMap;

use mupdf::pdf::{PdfDocument, PdfPage};
use mupdf::color::AnnotationColor;
use scorer_core::{BBox, Evidence, PdfRegion, Rubric, ScoreResult, SpanKind, Strength, Support};

use crate::geometry::{from_rect, to_quad, to_rect, union};
use crate::style::{box_width, highlight_opacity, letter, subitem_color, Rgb};
use crate::{sheet, PdfError};

const AUTHOR: &str = "journal-scorer";
const TAG_FONT_SIZE: f32 = 6.0;
const TAG_LINE_HEIGHT: f32 = 7.5;
const TAG_CHAR_WIDTH: f32 = 3.4;
const TAG_MIN_WIDTH: f32 = 34.0;
const TAG_GAP: f32 = 4.0;

/// Information printed on the score sheet.
#[derive(Debug, Clone, Default)]
pub struct SheetMeta {
    pub file_name: String,
    pub model: String,
    pub generated: String,
}

pub struct AnnotateInput<'a> {
    pub rubric: &'a Rubric,
    pub evidence: &'a [Evidence],
    pub score: &'a ScoreResult,
    pub meta: SheetMeta,
}

/// Returns a new PDF: the original pages with annotations, followed by the
/// score sheet.
pub fn annotate(pdf: &[u8], input: &AnnotateInput<'_>) -> Result<Vec<u8>, PdfError> {
    let mut doc = PdfDocument::from_bytes(pdf)?;
    let page_count = doc.page_count()? as u32;

    // Evidence regions grouped by page.
    let mut by_page: BTreeMap<u32, Vec<(&Evidence, Vec<PdfRegion>)>> = BTreeMap::new();
    for ev in input.evidence {
        let mut pages: BTreeMap<u32, Vec<PdfRegion>> = BTreeMap::new();
        for r in &ev.span.regions {
            pages.entry(r.page).or_default().push(*r);
        }
        for (page, regions) in pages {
            if (1..=page_count).contains(&page) {
                by_page.entry(page).or_default().push((ev, regions));
            }
        }
    }

    for (page_no, items) in &by_page {
        let mut page = doc.load_pdf_page(*page_no as i32 - 1)?;
        annotate_page(&mut page, input.rubric, items)?;
        page.update()?;
    }

    let first = doc.load_page(0)?.bounds()?;
    sheet::append(&mut doc, input, (first.x1 - first.x0, first.y1 - first.y0))?;

    let mut out = Vec::new();
    doc.write_to(&mut out)?;
    Ok(out)
}

fn annotate_page(page: &mut PdfPage, rubric: &Rubric, items: &[(&Evidence, Vec<PdfRegion>)]) -> Result<(), PdfError> {
    let page_box = from_rect(page.bounds()?);
    let text_box = items
        .iter()
        .flat_map(|(_, rs)| rs.iter().map(|r| r.bbox))
        .reduce(union)
        .unwrap_or(page_box);
    let mut margins = Margins::new(page_box, text_box);

    let mut items: Vec<&(&Evidence, Vec<PdfRegion>)> = items.iter().collect();
    items.sort_by(|a, b| a.1[0].bbox.y0.total_cmp(&b.1[0].bbox.y0));

    for (ev, regions) in items {
        let Some(best) = strongest(&ev.supports) else { continue };
        let color = subitem_color(rubric, &best.subitem);
        let note = note_text(rubric, ev);

        let mut annot = if ev.span.kind == SpanKind::Image {
            let rect = regions.iter().map(|r| r.bbox).reduce(union).expect("non-empty");
            let mut a = page.add_square_annotation(to_rect(rect))?;
            a.set_border_width(box_width(best.strength))?;
            a
        } else {
            let mut a = page.add_highlight_annotation(regions.iter().map(|r| to_quad(r.bbox)).collect::<Vec<_>>())?;
            a.set_opacity(highlight_opacity(best.strength))?;
            a
        };
        annot.set_color(rgb(color))?;
        annot.set_contents(&note)?;
        annot.set_author(AUTHOR)?;
        annot.update()?;

        let tag = tag_text(ev);
        if let Some(rect) = margins.place(regions[0].bbox.y0, &tag) {
            let mut t = page.add_free_text_annotation(to_rect(rect), &tag)?;
            t.set_default_appearance("Helv", TAG_FONT_SIZE, Some(rgb(color)))?;
            t.set_border_width(0.0)?;
            t.set_contents(&tag)?;
            t.set_author(AUTHOR)?;
            t.update()?;
        }
    }
    Ok(())
}

fn strongest(supports: &[Support]) -> Option<&Support> {
    // First of the strongest, so ties keep the model's order.
    supports.iter().fold(None, |best: Option<&Support>, s| match best {
        Some(b) if b.strength >= s.strength => Some(b),
        _ => Some(s),
    })
}

fn rgb((red, green, blue): Rgb) -> AnnotationColor {
    AnnotationColor::Rgb { red, green, blue }
}

/// `E3 E1·M S3·S`
fn tag_text(ev: &Evidence) -> String {
    let codes: Vec<String> = ev.supports.iter().map(|s| format!("{}·{}", s.subitem, letter(s.strength))).collect();
    format!("{} {}", ev.id, codes.join(" "))
}

/// Pop-up note: one line per supported subitem with the model's reason.
fn note_text(rubric: &Rubric, ev: &Evidence) -> String {
    let mut lines = vec![format!("{} — rubric evidence", ev.id)];
    for s in &ev.supports {
        let name = rubric.find_subitem(&s.subitem).map(|(_, sub)| sub.name.as_str()).unwrap_or("");
        let conf = s.confidence.map(|c| format!(" ({:.0}%)", c * 100.0)).unwrap_or_default();
        lines.push(format!("• {} {} — {}{}", s.subitem, name, s.strength, conf));
        if let Some(reason) = &s.reason {
            lines.push(format!("   {reason}"));
        }
    }
    lines.push(format!("Weights: weak {}, moderate {}, strong {}", rubric.weights.weak, rubric.weights.moderate, rubric.weights.strong));
    lines.join("\n")
}

/// Places margin tags beside the text column, stacking them so they never
/// overlap. Uses the wider of the two margins.
struct Margins {
    x0: f32,
    width: f32,
    next_y: f32,
    page_bottom: f32,
}

impl Margins {
    fn new(page: BBox, text: BBox) -> Self {
        let right = page.x1 - text.x1 - 2.0 * TAG_GAP;
        let left = text.x0 - page.x0 - 2.0 * TAG_GAP;
        let (x0, width) = if right >= TAG_MIN_WIDTH || right >= left {
            (text.x1 + TAG_GAP, right.max(TAG_MIN_WIDTH))
        } else {
            (page.x0 + TAG_GAP, left)
        };
        // Keep the tag on the page even when the margin is too narrow.
        let x0 = x0.min(page.x1 - TAG_GAP - width);
        Self { x0, width, next_y: page.y0, page_bottom: page.y1 }
    }

    fn place(&mut self, y: f32, tag: &str) -> Option<BBox> {
        let per_line = ((self.width - 2.0) / TAG_CHAR_WIDTH).floor().max(1.0) as usize;
        let lines = tag.chars().count().div_ceil(per_line).max(1);
        let height = lines as f32 * TAG_LINE_HEIGHT + 2.0;
        let y0 = y.max(self.next_y);
        if y0 + height > self.page_bottom {
            return None;
        }
        self.next_y = y0 + height + 1.0;
        Some(BBox { x0: self.x0, y0, x1: self.x0 + self.width, y1: y0 + height })
    }
}

#[allow(dead_code)]
fn _assert_strength_order() {
    // `strongest` relies on Strength's ordering.
    debug_assert!(Strength::Weak < Strength::Strong);
}
