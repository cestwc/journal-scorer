//! Splitting input text into small spans (sentences, bullets, short
//! paragraphs) that the LLM classifies one by one.

use crate::model::{SpanId, TextSpan};

/// Strategy for turning raw text into spans. The PDF layer runs a segmenter
/// over its extracted text and then fills in `TextSpan::regions`.
pub trait Segmenter: Send + Sync {
    fn segment(&self, text: &str) -> Vec<TextSpan>;
}

/// Rule-based splitter: breaks at sentence ends, blank lines and bullet
/// starts; single line breaks inside a paragraph are treated as wrapping.
#[derive(Debug, Clone)]
pub struct SentenceSegmenter {
    /// Spans with fewer alphanumeric characters than this are dropped.
    pub min_alnum: usize,
}

impl Default for SentenceSegmenter {
    fn default() -> Self {
        Self { min_alnum: 3 }
    }
}

const ABBREVIATIONS: &[&str] = &["e.g.", "i.e.", "etc.", "vs.", "cf.", "fig.", "no.", "mr.", "ms.", "dr."];

impl Segmenter for SentenceSegmenter {
    fn segment(&self, text: &str) -> Vec<TextSpan> {
        split_ranges(text)
            .into_iter()
            .filter_map(|(start, end)| trim_range(text, start, end))
            .filter(|&(s, e)| text[s..e].chars().filter(|c| c.is_alphanumeric()).count() >= self.min_alnum)
            .enumerate()
            .map(|(i, (start, end))| {
                TextSpan::text(SpanId(format!("S{}", i + 1)), normalize_ws(&text[start..end]), start, end)
            })
            .collect()
    }
}

fn split_ranges(text: &str) -> Vec<(usize, usize)> {
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let mut ranges = Vec::new();
    let mut start = 0;
    for (k, &(i, c)) in chars.iter().enumerate() {
        let after = i + c.len_utf8();
        let boundary = match c {
            '.' | '!' | '?' => {
                let next_is_ws = chars.get(k + 1).map_or(false, |&(_, n)| n.is_whitespace());
                let so_far = &text[start..after];
                next_is_ws && !ends_with_abbreviation(so_far) && !is_list_number(so_far)
            }
            '\n' => {
                let rest = text[after..].trim_start_matches([' ', '\t', '\r']);
                rest.starts_with('\n') || starts_with_bullet(rest)
            }
            _ => false,
        };
        if boundary {
            ranges.push((start, after));
            start = after;
        }
    }
    ranges.push((start, text.len()));
    ranges
}

fn ends_with_abbreviation(segment: &str) -> bool {
    let last = segment.split_whitespace().last().unwrap_or("").to_lowercase();
    ABBREVIATIONS.contains(&last.as_str())
}

/// `"  12."` — an ordered-list marker, not a sentence end.
fn is_list_number(segment: &str) -> bool {
    let t = segment.trim();
    t.len() > 1 && t.len() <= 4 && t.ends_with('.') && t[..t.len() - 1].chars().all(|c| c.is_ascii_digit())
}

fn starts_with_bullet(s: &str) -> bool {
    let mut it = s.chars();
    match it.next() {
        Some('-' | '*' | '•' | '–' | '▪' | '◦') => it.next().map_or(false, char::is_whitespace),
        Some(c) if c.is_ascii_digit() => {
            let digits = s.chars().take_while(char::is_ascii_digit).count();
            let rest = &s[digits..];
            (rest.starts_with(". ") || rest.starts_with(") ")) && digits <= 3
        }
        _ => false,
    }
}

fn trim_range(text: &str, start: usize, end: usize) -> Option<(usize, usize)> {
    let slice = &text[start..end];
    let lead = slice.len() - slice.trim_start().len();
    let trail = slice.len() - slice.trim_end().len();
    (lead + trail < slice.len()).then(|| (start + lead, end - trail))
}

fn normalize_ws(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(input: &str) -> Vec<String> {
        SentenceSegmenter::default().segment(input).into_iter().map(|s| s.text).collect()
    }

    #[test]
    fn splits_sentences_and_keeps_abbreviations() {
        let t = texts("We used SDD, e.g. specs first. Then BDD! Did it work? Yes, v1.2 passed.");
        assert_eq!(
            t,
            vec!["We used SDD, e.g. specs first.", "Then BDD!", "Did it work?", "Yes, v1.2 passed."]
        );
    }

    #[test]
    fn splits_paragraphs_and_bullets_but_joins_wrapped_lines() {
        let t = texts("Intro line one\ncontinues here\n\nTasks:\n- AI wrote tests\n- I reviewed the diff\n1. Step one");
        assert_eq!(
            t,
            vec!["Intro line one continues here", "Tasks:", "- AI wrote tests", "- I reviewed the diff", "1. Step one"]
        );
    }

    #[test]
    fn offsets_point_into_original_text() {
        let input = "  First sentence.   Second one here.  ";
        for s in SentenceSegmenter::default().segment(input) {
            assert_eq!(normalize_ws(&input[s.start..s.end]), s.text);
        }
    }

    #[test]
    fn drops_noise_spans() {
        assert_eq!(texts("--\n\n12\n\nReal sentence."), vec!["Real sentence."]);
    }
}
