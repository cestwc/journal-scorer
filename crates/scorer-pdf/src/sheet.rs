//! Score-sheet pages appended after the journal: totals, formula, per-class
//! scores with contributing evidence, and the evidence list.

use mupdf::pdf::PdfDocument;
use mupdf::{Buffer, Font};
use scorer_core::Strength;

use crate::style::{criterion_color, subitem_color, Rgb};
use crate::{AnnotateInput, PdfError};

const MARGIN: f32 = 48.0;
const BLACK: Rgb = (0.1, 0.1, 0.12);
const GRAY: Rgb = (0.42, 0.45, 0.5);
const TRACK: Rgb = (0.9, 0.91, 0.93);
const MAX_SNIPPET: usize = 320;

#[derive(Clone, Copy)]
enum Face {
    Regular,
    Bold,
}

impl Face {
    fn resource(self) -> &'static str {
        match self {
            Face::Regular => "F1",
            Face::Bold => "F2",
        }
    }
}

struct Writer {
    fonts: [Font; 2],
    width: f32,
    height: f32,
    pages: Vec<String>,
    cur: String,
    /// Distance from the top of the page.
    y: f32,
}

impl Writer {
    fn new(width: f32, height: f32) -> Result<Self, PdfError> {
        Ok(Self {
            fonts: [Font::new("Helvetica")?, Font::new("Helvetica-Bold")?],
            width,
            height,
            pages: Vec::new(),
            cur: String::new(),
            y: MARGIN,
        })
    }

    fn content_width(&self) -> f32 {
        self.width - 2.0 * MARGIN
    }

    fn text_width(&self, face: Face, size: f32, s: &str) -> f32 {
        let font = &self.fonts[face as usize];
        s.chars()
            .map(|c| {
                font.encode_character(c as i32)
                    .and_then(|g| font.advance_glyph(g))
                    .unwrap_or(0.5)
            })
            .sum::<f32>()
            * size
    }

    fn ensure(&mut self, needed: f32) {
        if self.y + needed > self.height - MARGIN {
            self.pages.push(std::mem::take(&mut self.cur));
            self.y = MARGIN;
        }
    }

    fn gap(&mut self, h: f32) {
        self.y += h;
    }

    /// Draws one line at `x` without wrapping, on the current baseline.
    fn draw(&mut self, x: f32, s: &str, face: Face, size: f32, color: Rgb) {
        let baseline = self.height - (self.y + size);
        self.cur.push_str(&format!(
            "BT /{} {size:.1} Tf {:.3} {:.3} {:.3} rg {x:.2} {baseline:.2} Td ({}) Tj ET\n",
            face.resource(),
            color.0,
            color.1,
            color.2,
            pdf_string(s)
        ));
    }

    /// Wrapped paragraph starting at `indent`.
    fn para(&mut self, s: &str, face: Face, size: f32, color: Rgb, indent: f32) {
        let max = self.content_width() - indent;
        let line_h = size * 1.35;
        for line in self.wrap(s, face, size, max) {
            self.ensure(line_h);
            self.draw(MARGIN + indent, &line, face, size, color);
            self.y += line_h;
        }
    }

    fn wrap(&self, s: &str, face: Face, size: f32, max: f32) -> Vec<String> {
        let mut lines = Vec::new();
        let mut cur = String::new();
        for word in s.split_whitespace() {
            let candidate = if cur.is_empty() { word.to_owned() } else { format!("{cur} {word}") };
            if self.text_width(face, size, &candidate) <= max || cur.is_empty() {
                cur = candidate;
            } else {
                lines.push(std::mem::replace(&mut cur, word.to_owned()));
            }
        }
        if !cur.is_empty() {
            lines.push(cur);
        }
        lines
    }

    fn rect(&mut self, x: f32, top: f32, w: f32, h: f32, color: Rgb) {
        let y = self.height - top - h;
        self.cur.push_str(&format!(
            "{:.3} {:.3} {:.3} rg {x:.2} {y:.2} {w:.2} {h:.2} re f\n",
            color.0, color.1, color.2
        ));
    }

    /// Label on the left, value right-aligned.
    fn row(&mut self, label: &str, value: &str, face: Face, size: f32, color: Rgb, indent: f32) {
        self.ensure(size * 1.4);
        let vw = self.text_width(face, size, value);
        self.draw(MARGIN + indent, label, face, size, color);
        self.draw(self.width - MARGIN - vw, value, face, size, color);
        self.y += size * 1.4;
    }

    fn bar(&mut self, fraction: f64, color: Rgb, indent: f32, h: f32) {
        self.ensure(h + 4.0);
        let w = self.content_width() - indent;
        self.rect(MARGIN + indent, self.y, w, h, TRACK);
        self.rect(MARGIN + indent, self.y, w * fraction.clamp(0.0, 1.0) as f32, h, color);
        self.y += h + 4.0;
    }

    fn finish(mut self) -> Vec<String> {
        if !self.cur.is_empty() {
            self.pages.push(self.cur);
        }
        self.pages
    }
}

pub fn append(doc: &mut PdfDocument, input: &AnnotateInput<'_>, (width, height): (f32, f32)) -> Result<(), PdfError> {
    let mut w = Writer::new(width, height)?;
    let score = input.score;
    let meta = &input.meta;

    w.para("Score sheet", Face::Bold, 18.0, BLACK, 0.0);
    w.gap(2.0);
    for line in [
        format!("File: {}", meta.file_name),
        format!("Rubric: {} (v{})", input.rubric.name, input.rubric.version),
        format!("Model: {}  ·  Generated: {}", meta.model, meta.generated),
    ] {
        w.para(&line, Face::Regular, 9.0, GRAY, 0.0);
    }
    w.gap(10.0);
    w.row(
        "Total",
        &format!("{:.2} / {:.0}", score.total, score.max_total),
        Face::Bold,
        16.0,
        BLACK,
        0.0,
    );
    w.gap(4.0);
    w.para(
        &format!(
            "Class score = 1 − (1 − w1)(1 − w2)…(1 − wn) over its n pieces of evidence, with w = weak {}, moderate {}, strong {}. \
             Criterion score = {} × (sum of class scores) / (number of classes). Total = sum of criterion scores.",
            input.rubric.weights.weak, input.rubric.weights.moderate, input.rubric.weights.strong, input.rubric.criterion_max
        ),
        Face::Regular,
        8.5,
        GRAY,
        0.0,
    );
    w.para(
        "In the journal: highlight colour = criterion, darker = stronger; margin tags list subitem·strength (W/M/S); \
         click a highlight for the model's reasons.",
        Face::Regular,
        8.5,
        GRAY,
        0.0,
    );
    w.gap(12.0);

    for (i, c) in score.criteria.iter().enumerate() {
        let color = criterion_color(i);
        w.ensure(60.0);
        w.rect(MARGIN, w.y + 1.0, 8.0, 8.0, color);
        w.row(&format!("     {}  {}", c.criterion, c.name), &format!("{:.2} / {:.0}", c.score, c.max), Face::Bold, 10.5, BLACK, 0.0);
        w.bar(c.score / c.max, color, 0.0, 4.0);
        for cl in &c.classes {
            let ids: Vec<String> = cl
                .contributions
                .iter()
                .map(|x| format!("{} {}", x.evidence_id, strength_word(x.strength)))
                .collect();
            let tone = if cl.contributions.is_empty() { GRAY } else { BLACK };
            w.row(&format!("{}  {}", cl.subitem, cl.name), &format!("{:.2}", cl.score), Face::Regular, 8.5, tone, 12.0);
            if !ids.is_empty() {
                w.para(&ids.join(", "), Face::Regular, 7.5, GRAY, 28.0);
            }
        }
        w.gap(10.0);
    }

    w.ensure(40.0);
    w.para(&format!("Evidence ({})", input.evidence.len()), Face::Bold, 13.0, BLACK, 0.0);
    w.gap(4.0);
    for ev in input.evidence {
        let page = ev.span.page().map(|p| format!("p.{p}")).unwrap_or_else(|| "–".into());
        let kind = if ev.span.kind == scorer_core::SpanKind::Image { " · image" } else { "" };
        let labels: Vec<String> = ev.supports.iter().map(|s| format!("{} {}", s.subitem, strength_word(s.strength))).collect();
        w.ensure(30.0);
        let color = ev.supports.first().map(|s| subitem_color(input.rubric, &s.subitem)).unwrap_or(GRAY);
        w.rect(MARGIN, w.y + 1.5, 3.0, 8.0, color);
        w.para(&format!("{}  ·  {page}{kind}  ·  {}", ev.id, labels.join(", ")), Face::Bold, 8.5, BLACK, 8.0);
        let mut snippet: String = ev.span.text.chars().take(MAX_SNIPPET).collect();
        if ev.span.text.chars().count() > MAX_SNIPPET {
            snippet.push('…');
        }
        w.para(&snippet, Face::Regular, 8.0, GRAY, 8.0);
        w.gap(5.0);
    }

    let resources = doc.new_object_from_str(
        "<< /Font << /F1 << /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >> \
                     /F2 << /Type /Font /Subtype /Type1 /BaseFont /Helvetica-Bold /Encoding /WinAnsiEncoding >> >> >>",
    )?;
    let resources = doc.add_object(&resources)?;
    for content in w.finish() {
        let page = doc.new_page((width, height))?;
        let stream = doc.add_stream(&Buffer::from_bytes(content.as_bytes())?, None, true)?;
        let mut obj = page.object();
        obj.dict_put("Contents", stream)?;
        obj.dict_put("Resources", resources.clone())?;
    }
    Ok(())
}

fn strength_word(s: Strength) -> &'static str {
    s.as_str()
}

/// Encodes text as a PDF literal string in WinAnsiEncoding.
fn pdf_string(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        let byte: u8 = match c {
            '(' | ')' | '\\' => {
                out.push('\\');
                c as u8
            }
            ' '..='~' => c as u8,
            '\u{a0}'..='\u{ff}' => c as u32 as u8,
            '€' => 0x80,
            '‚' => 0x82,
            '…' => 0x85,
            '‘' => 0x91,
            '’' => 0x92,
            '“' => 0x93,
            '”' => 0x94,
            '•' => 0x95,
            '–' | '−' => 0x96,
            '—' => 0x97,
            '™' => 0x99,
            'Π' => b'P',
            'ᵢ' => b'i',
            'ﬁ' => {
                out.push('f');
                b'i'
            }
            'ﬂ' => {
                out.push('f');
                b'l'
            }
            c if c.is_whitespace() => b' ',
            _ => b'?',
        };
        if byte < 0x80 {
            out.push(byte as char);
        } else {
            out.push_str(&format!("\\{byte:03o}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn winansi_escaping() {
        assert_eq!(pdf_string("a(b)\\"), "a\\(b\\)\\\\");
        assert_eq!(pdf_string("“x” – é"), "\\223x\\224 \\226 \\351");
        assert_eq!(pdf_string("ﬂow 漢"), "flow ?");
    }
}
