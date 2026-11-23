//! Text and figure extraction with positions.

use mupdf::pdf::PdfDocument;
use mupdf::text_page::TextBlockType;
use mupdf::{Colorspace, Device, IRect, ImageFormat, Matrix, Page, Pixmap, TextPageFlags};
use scorer_core::segment::Segmenter;
use scorer_core::{BBox, Document, Figure, PdfRegion, TextSpan};

use crate::geometry::{from_rect, intersect, quad_bounds, union};
use crate::PdfError;

#[derive(Debug, Clone)]
pub struct ExtractOptions {
    /// Figures smaller than this (points) are ignored as icons/logos.
    pub min_figure_width: f32,
    pub min_figure_height: f32,
    /// At most this many figures per document (in reading order).
    pub max_figures: usize,
    /// Longest side of a rendered figure, in pixels.
    pub figure_max_px: f32,
    /// Maximum render scale (1.0 = 72 dpi).
    pub figure_max_scale: f32,
}

impl Default for ExtractOptions {
    fn default() -> Self {
        Self { min_figure_width: 80.0, min_figure_height: 40.0, max_figures: 40, figure_max_px: 1400.0, figure_max_scale: 3.0 }
    }
}

/// One non-whitespace character with its position.
struct Glyph {
    offset: usize,
    page: u32,
    bbox: BBox,
}

/// How to join a text line to the previous one. Block boundaries are not
/// trusted (some PDFs put every line in its own block), so this uses layout.
fn separator(prev: Option<BBox>, cur: BBox) -> &'static str {
    let Some(prev) = prev else { return "" };
    let (ph, ch) = (prev.y1 - prev.y0, cur.y1 - cur.y0);
    let overlap = prev.y1.min(cur.y1) - prev.y0.max(cur.y0);
    if overlap > 0.5 * ph.min(ch) {
        " " // same row (e.g. a font change split the line)
    } else if cur.y0 < prev.y0 || cur.y0 - prev.y1 > 0.6 * ph {
        "\n\n" // gap or jump to a new column: paragraph boundary
    } else {
        "\n" // wrapped line
    }
}

/// Extracts a scoring [`Document`] from PDF bytes. Text spans carry the
/// page regions of their characters; figures are rendered to PNG.
pub fn extract(pdf: &[u8], segmenter: &dyn Segmenter, opts: &ExtractOptions) -> Result<Document, PdfError> {
    let doc = PdfDocument::from_bytes(pdf)?;
    let page_count = doc.page_count()?;

    let mut text = String::new();
    let mut glyphs: Vec<Glyph> = Vec::new();
    let mut figures: Vec<Figure> = Vec::new();

    for p in 0..page_count {
        let page_no = (p + 1) as u32;
        let page = doc.load_page(p)?;
        let page_box = from_rect(page.bounds()?);
        let tp = page.to_text_page(TextPageFlags::PRESERVE_IMAGES)?;
        let mut prev_line: Option<BBox> = None;
        let page_glyphs_start = glyphs.len();
        let page_figures_start = figures.len();

        for block in tp.blocks() {
            match block.r#type() {
                TextBlockType::Text => {
                    for line in block.lines() {
                        let line_box = from_rect(line.bounds());
                        text.push_str(separator(prev_line, line_box));
                        prev_line = Some(line_box);
                        for ch in line.chars() {
                            let Some(c) = ch.char() else { continue };
                            let offset = text.len();
                            text.push(c);
                            if !c.is_whitespace() {
                                glyphs.push(Glyph { offset, page: page_no, bbox: quad_bounds(&ch.quad()) });
                            }
                        }
                    }
                }
                TextBlockType::Image if figures.len() < opts.max_figures => {
                    let Some(bbox) = intersect(from_rect(block.bounds()), page_box) else { continue };
                    let (w, h) = (bbox.x1 - bbox.x0, bbox.y1 - bbox.y0);
                    let duplicate = figures.iter().any(|f| f.page == page_no && f.bbox == bbox);
                    if w < opts.min_figure_width || h < opts.min_figure_height || duplicate {
                        continue;
                    }
                    match render_region(&page, bbox, opts) {
                        // Anchor is fixed up after the page's text is known.
                        Ok(png) => figures.push(Figure { page: page_no, bbox, anchor: 0, png }),
                        Err(e) => tracing::warn!(page = page_no, error = %e, "could not render figure"),
                    }
                }
                _ => {}
            }
        }
        // MuPDF lists image blocks in content order, not reading order: place
        // each figure before the first text below it on its page.
        for fig in &mut figures[page_figures_start..] {
            fig.anchor = glyphs[page_glyphs_start..]
                .iter()
                .find(|g| g.bbox.y0 >= fig.bbox.y1 - 2.0)
                .map_or(text.len(), |g| g.offset);
        }
        text.push_str("\n\n");
    }

    let spans: Vec<TextSpan> = segmenter
        .segment(&text)
        .into_iter()
        .map(|mut s| {
            s.regions = regions_for(&glyphs, s.start, s.end);
            s
        })
        .collect();

    if spans.is_empty() && figures.is_empty() {
        return Err(PdfError::Empty);
    }
    Ok(Document { text, spans, figures })
}

/// One rectangle per visual row covered by bytes `start..end`.
fn regions_for(glyphs: &[Glyph], start: usize, end: usize) -> Vec<PdfRegion> {
    let from = glyphs.partition_point(|g| g.offset < start);
    let mut out: Vec<PdfRegion> = Vec::new();
    for g in glyphs[from..].iter().take_while(|g| g.offset < end) {
        match out.last_mut() {
            Some(r) if r.page == g.page && same_row(r.bbox, g.bbox) => r.bbox = union(r.bbox, g.bbox),
            _ => out.push(PdfRegion { page: g.page, bbox: g.bbox }),
        }
    }
    out
}

fn same_row(a: BBox, b: BBox) -> bool {
    let overlap = a.y1.min(b.y1) - a.y0.max(b.y0);
    overlap > 0.5 * (a.y1 - a.y0).min(b.y1 - b.y0) && b.x0 >= a.x0 - 1.0
}

/// Renders a page region to PNG (whatever is drawn there, not just the
/// embedded image, so overlaid text is included).
fn render_region(page: &Page, bbox: BBox, opts: &ExtractOptions) -> Result<Vec<u8>, PdfError> {
    let longest = (bbox.x1 - bbox.x0).max(bbox.y1 - bbox.y0);
    let scale = (opts.figure_max_px / longest).min(opts.figure_max_scale);
    let irect = IRect {
        x0: (bbox.x0 * scale).floor() as i32,
        y0: (bbox.y0 * scale).floor() as i32,
        x1: (bbox.x1 * scale).ceil() as i32,
        y1: (bbox.y1 * scale).ceil() as i32,
    };
    let mut pixmap = Pixmap::new_with_rect(&Colorspace::device_rgb(), irect, false)?;
    pixmap.clear_with(255)?;
    {
        let device = Device::from_pixmap(&pixmap)?;
        page.run(&device, &Matrix::new_scale(scale, scale))?;
    }
    let mut png = Vec::new();
    pixmap.write_to(&mut png, ImageFormat::PNG)?;
    Ok(png)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn g(offset: usize, page: u32, line: u32, x: f32) -> Glyph {
        Glyph { offset, page, bbox: BBox { x0: x, y0: 10.0 * line as f32, x1: x + 5.0, y1: 10.0 * line as f32 + 8.0 } }
    }

    fn bb(x0: f32, y0: f32, x1: f32, y1: f32) -> BBox {
        BBox { x0, y0, x1, y1 }
    }

    #[test]
    fn separators_from_layout() {
        let line = bb(50.0, 100.0, 300.0, 110.0);
        assert_eq!(separator(None, line), "");
        assert_eq!(separator(Some(line), bb(310.0, 101.0, 400.0, 110.0)), " ");
        assert_eq!(separator(Some(line), bb(50.0, 112.0, 300.0, 122.0)), "\n");
        assert_eq!(separator(Some(line), bb(50.0, 125.0, 300.0, 135.0)), "\n\n");
        assert_eq!(separator(Some(line), bb(320.0, 40.0, 500.0, 50.0)), "\n\n");
    }

    #[test]
    fn regions_merge_per_line_and_split_across_pages() {
        let glyphs = vec![g(0, 1, 0, 0.0), g(1, 1, 0, 5.0), g(3, 1, 1, 0.0), g(5, 2, 2, 0.0), g(9, 2, 3, 0.0)];
        let r = regions_for(&glyphs, 1, 6);
        assert_eq!(r.len(), 3);
        assert_eq!((r[0].page, r[0].bbox.x0, r[0].bbox.x1), (1, 5.0, 10.0));
        assert_eq!(r[1].page, 1);
        assert_eq!(r[2].page, 2);
    }
}
