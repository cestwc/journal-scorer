# journal-scorer

Rubric-based scoring of student AI-journal reflections: PDF or text in → evidence labels, scores,
and an annotated PDF with an appended score sheet.

License: **AGPL-3.0-or-later** (the PDF layer links MuPDF, which is AGPL).

The LLM **only labels evidence** (span → subitems → weak/moderate/strong). All arithmetic is
deterministic Rust code (grade.md §6):

- class score = 1 − Π(1 − wᵢ), w ∈ {0.25, 0.60, 1.00}
- criterion score = 5 · Σ class scores / N
- total = Σ criterion scores (0–25)

## Layout

```
crates/
  scorer-core/    domain: models, rubric (TOML, built-in in rubrics/), scoring, segmentation. No I/O.
  scorer-llm/     LlmProvider trait, ProviderConfig, adapters (openai_compatible, mock), JSON extraction
  scorer-engine/  pipeline: figures → vision descriptions; spans → LLM labels → validate → Evidence → ScoreResult
  scorer-pdf/     MuPDF: text + per-character boxes, figures → PNG; highlight/box annotations, margin tags, score sheet
  scorer-app/     binary `journal-scorer`: CLI subcommands + axum HTTP API + browser UI (static/, embedded)
```

Dependency direction: `app → {engine, pdf}`, `engine → {core, llm}`, `pdf → core`.

## PDF flow

1. **Extract** (scorer-pdf): text with a box per character; lines are joined by layout (gap/column),
   so sentences survive PDFs that put every line in its own block. Figures ≥ 80×40 pt are rendered
   to PNG and anchored before the first text below them.
2. **Describe figures**: each screenshot goes to the vision model once; the description becomes a span
   `[Image on page N] …` and is labelled like any sentence.
3. **Label + score**: as for text.
4. **Annotate** (copy of the original): highlight per evidence (colour = criterion, opacity = strength,
   pop-up note with subitem, strength, confidence, reason), box around screenshot evidence, margin tag
   `E6 S4·S C3·M`, then score-sheet pages (formula, class scores with evidence ids, evidence list).

Building needs a C compiler and libclang (MuPDF is compiled from source; first build ~10 min).
Set `LIBCLANG_PATH` in `.cargo/config.toml` for your machine.

| Model | Where | Notes |
|---|---|---|
| `Strength` | core/model/strength.rs | weak < moderate < strong; weights come from the rubric |
| `Criterion`, `Subitem` | core/model/{criterion,subitem}.rs | data-driven, loaded from rubric TOML |
| `Evidence` (+ `TextSpan`, `Support`, `PdfLocation`) | core/model/evidence.rs | one span, multi-label, ≤ 1 strength per subitem; `location` reserved for PDF |
| `ScoreResult` (+ `CriterionScore`, `ClassScore`, `Contribution`) | core/model/score.rs | every number traces back to evidence ids |

## Usage

```bash
cp journal-scorer.example.toml journal-scorer.toml   # edit base_url/model
export LLM_API_KEY=...                               # if the server needs one

cargo run --release -- score reflection.txt          # JSON report to stdout
cat reflection.txt | cargo run -- score -            # stdin
cargo run -- annotate journal.pdf                    # -> journal-annotated.pdf (+ --report r.json)
cargo run -- segment reflection.txt                  # inspect spans, no LLM call (also .pdf)
cargo run -- rubric                                  # print active rubric
cargo run -- serve                                   # HTTP API on 127.0.0.1:2379

# No config file:
cargo run -- --base-url http://127.0.0.1:8080/v1 --model mymodel score r.txt
cargo run -- --mock score r.txt                      # offline
```

Browser UI: run `serve`, open http://localhost:2379/. Open or drop a PDF (background job with
progress → PDF preview, annotated view, score panel, evidence table, annotated-PDF and JSON
downloads), or paste text and press Score. From a laptop: `ssh -N -L 2379:127.0.0.1:2379 <this host>`.

HTTP API:

```bash
curl -s localhost:2379/api/health
curl -s localhost:2379/api/rubric
curl -s localhost:2379/api/score -H 'content-type: application/json' \
     -d '{"text": "We used SDD then TDD ...", "include_spans": false}'
curl -s -F file=@journal.pdf localhost:2379/api/jobs           # -> {"id": "..."}
curl -s localhost:2379/api/jobs/<id>                            # progress, then report
curl -s -o out.pdf 'localhost:2379/api/jobs/<id>/annotated.pdf?download=1'
```

## Extending

- **New provider**: implement `scorer_llm::LlmProvider`, add a `ProviderConfig` variant, wire it in `providers::build_provider`.
- **New rubric**: copy `crates/scorer-core/rubrics/ai_journal_v1.toml`, pass `--rubric`.
- **Other document formats**: produce a `scorer_core::Document` (spans with `regions`, figures) and call `Scorer::score_document`.

## Known MVP limitations

- Spans are classified in batches without surrounding context.
- S5 "evidence distributed across sections" is document-level; per-span labelling only approximates it.
- PDF jobs live in memory (last 30) and run one at a time; a restart forgets them.
- Figures are described without the surrounding text; a figure's evidence box covers the whole image.
