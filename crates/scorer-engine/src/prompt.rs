//! Prompt construction. The rubric text comes from the `Rubric` data, so a
//! different rubric file changes the prompt without code changes.

use std::fmt::Write;

use scorer_core::{Rubric, Strength, TextSpan};

pub fn system_prompt(rubric: &Rubric) -> String {
    let mut p = String::new();
    let _ = writeln!(
        p,
        "You are a rubric-aware evidence classifier for student reflections ({}).\n\
         You do NOT grade the document. You label evidence; scores are computed by a program.\n\n\
         Task: for each numbered span you are given, decide whether it is evidence for any rubric subitem below.\n\
         Rules:\n\
         - A span may support several subitems (multi-label).\n\
         - For each (span, subitem) pair give exactly ONE strength: weak, moderate or strong, following that subitem's level descriptions.\n\
         - Judge only what the span itself shows. Do not reward length or style.\n\
         - Spans starting with \"[Image on page N]\" are descriptions of screenshots/figures the student included; \
         treat what the image shows as the student's evidence (e.g. artifacts, observable outcomes, precise detail).\n\
         - Omit spans that are not evidence. Use only the span ids and subitem ids listed.\n\
         - confidence is your probability (0-1) that the label is correct. reason is at most 25 words.\n\
         - Output a single JSON object and nothing else.\n",
        rubric.name
    );
    let _ = writeln!(p, "Rubric subitems:");
    for c in &rubric.criteria {
        let _ = writeln!(p, "\n## {} — {}", c.id, c.name);
        for s in &c.subitems {
            let _ = writeln!(p, "- {} {}", s.id, s.name);
            for level in Strength::ALL {
                let _ = writeln!(p, "    {level}: {}", s.levels.get(level));
            }
        }
    }
    let _ = write!(p, "\nOutput schema:\n{}", OUTPUT_SCHEMA_EXAMPLE);
    p
}

pub fn user_prompt(spans: &[TextSpan]) -> String {
    let mut p = String::from("Spans:\n");
    for s in spans {
        let _ = writeln!(p, "[{}] {}", s.id, s.text);
    }
    p.push_str("\nReturn the JSON object now.");
    p
}

const OUTPUT_SCHEMA_EXAMPLE: &str = r#"{"evidence": [
  {"span_id": "S3", "supports": [
    {"subitem": "E1", "strength": "moderate", "confidence": 0.8, "reason": "Describes reviewing the diff before merging."}
  ]}
]}
"#;

pub const FIGURE_SYSTEM_PROMPT: &str = "You transcribe figures from a student's software-engineering reflection journal. \
In at most 120 words: first say what kind of image it is (code, terminal output, test report, diagram, \
chat with an AI assistant, UI screenshot, ...), then transcribe the key visible text verbatim where legible \
(file names, commands, test counts, error messages, numbers). Do not evaluate or grade. Plain text only.";

pub fn figure_user_prompt(page: u32) -> String {
    format!("Describe this image from page {page} of the journal.")
}
