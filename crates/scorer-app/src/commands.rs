//! Subcommand implementations.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result};
use scorer_core::segment::{Segmenter, SentenceSegmenter};
use scorer_engine::Scorer;
use scorer_engine::Progress;
use serde::Serialize;

use crate::cli::{Cli, Command};
use crate::config::{self, AppConfig};
use crate::{pdf, web};

pub async fn run(cli: Cli) -> Result<()> {
    match cli.command {
        Command::Models { provider, vision, all, json } => {
            let setup = scorer_llm::opencode::OpencodeSetup::load(&Default::default())?;
            let models: Vec<_> = setup
                .list()
                .into_iter()
                .filter(|m| provider.as_deref().map_or(true, |p| m.spec.split('/').next() == Some(p)))
                .filter(|m| !vision || m.vision == Some(true))
                .filter(|m| all || m.unusable.is_none())
                .collect();
            if json {
                return print_json(&models, false);
            }
            println!("{:<52} {:>6} {:>8} {:>15}  NAME", "MODEL", "IMAGES", "CONTEXT", "$/M IN / OUT");
            for m in &models {
                let images = match m.vision { Some(true) => "yes", Some(false) => "no", None => "?" };
                let context = m.context.map(|c| format!("{}K", c / 1000)).unwrap_or_else(|| "?".into());
                let cost = m.cost.map(|(i, o)| format!("{i} / {o}")).unwrap_or_else(|| "?".into());
                println!("{:<52} {:>6} {:>8} {:>15}  {}", m.spec, images, context, cost, m.name);
                if let Some(why) = &m.unusable {
                    println!("{:<52} unusable: {why}", "");
                }
            }
            if let Some(default) = setup.default_model() {
                println!("\nOpenCode default: {default}");
            }
            Ok(())
        }
        Command::Rubric { compact } => {
            let rubric = config::load_rubric(cli.global.rubric.as_deref())?;
            print_json(&rubric, compact)
        }
        Command::Segment { input, compact } => {
            if is_pdf(Path::new(&input)) {
                let bytes = std::fs::read(&input).with_context(|| format!("reading {input}"))?;
                let doc = scorer_pdf::extract(&bytes, &SentenceSegmenter::default(), &Default::default())?;
                let figures: Vec<_> = doc
                    .figures
                    .iter()
                    .map(|f| serde_json::json!({"page": f.page, "bbox": f.bbox, "png_bytes": f.png.len()}))
                    .collect();
                print_json(&serde_json::json!({"spans": doc.spans, "figures": figures}), compact)
            } else {
                let spans = SentenceSegmenter::default().segment(&read_input(&input)?);
                print_json(&spans, compact)
            }
        }
        Command::Annotate { input, output, report } => {
            let cfg = config::load(&cli.global)?;
            let scorer = build_scorer(&cfg)?;
            let bytes = std::fs::read(&input).with_context(|| format!("reading {}", input.display()))?;
            let name = input.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let log = |p: Progress| tracing::info!(stage = ?p.stage, done = p.done, total = p.total, "progress");
            let outcome = pdf::score_pdf(&scorer, Arc::new(bytes), &name, &log).await?;

            let output = output.unwrap_or_else(|| annotated_path(&input));
            std::fs::write(&output, &outcome.annotated).with_context(|| format!("writing {}", output.display()))?;
            if let Some(path) = report {
                std::fs::write(&path, serde_json::to_vec_pretty(&outcome.report)?)
                    .with_context(|| format!("writing {}", path.display()))?;
            }
            eprintln!(
                "total {:.2} / {:.0}, {} evidence; wrote {}",
                outcome.report.score.total,
                outcome.report.score.max_total,
                outcome.report.evidence.len(),
                output.display()
            );
            Ok(())
        }
        Command::Score { input, include_spans, compact } => {
            let cfg = config::load(&cli.global)?;
            let scorer = build_scorer(&cfg)?;
            let text = read_input(&input)?;
            let mut report = scorer.score_text(&text).await?;
            if !include_spans {
                report.spans.clear();
            }
            report.source_text.clear();
            print_json(&report, compact)
        }
        Command::Serve { addr } => {
            let cfg = config::load(&cli.global)?;
            let scorer = Arc::new(build_scorer(&cfg)?);
            web::serve(scorer, addr.as_deref().unwrap_or(&cfg.server.addr)).await
        }
    }
}

pub fn build_scorer(cfg: &AppConfig) -> Result<Scorer> {
    let rubric = config::load_rubric(cfg.rubric.path.as_deref())?;
    let provider = scorer_llm::build_provider(&cfg.llm).context("building LLM provider")?;
    let info = provider.info();
    let mut options = cfg.scorer.clone();
    // Stay within the model's real limits.
    if let Some(max) = info.max_output {
        options.max_tokens = Some(options.max_tokens.map_or(max, |m| m.min(max)));
        options.figure_max_tokens = options.figure_max_tokens.min(max);
    }
    if info.vision == Some(false) && options.describe_figures {
        tracing::warn!(model = %info.model, "model does not accept images; screenshots will be skipped");
        options.describe_figures = false;
    }
    tracing::info!(provider = %info.kind, model = %info.model, rubric = %rubric.id, "scorer ready");
    Ok(Scorer::new(provider, Arc::new(rubric)).with_options(options))
}

fn is_pdf(path: &Path) -> bool {
    path.extension().is_some_and(|e| e.eq_ignore_ascii_case("pdf"))
}

fn annotated_path(input: &Path) -> PathBuf {
    let stem = input.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "output".into());
    input.with_file_name(format!("{stem}-annotated.pdf"))
}

fn read_input(input: &str) -> Result<String> {
    if input == "-" {
        let mut s = String::new();
        std::io::stdin().read_to_string(&mut s).context("reading stdin")?;
        Ok(s)
    } else {
        std::fs::read_to_string(input).with_context(|| format!("reading {input}"))
    }
}

fn print_json<T: Serialize>(value: &T, compact: bool) -> Result<()> {
    let out = if compact { serde_json::to_string(value)? } else { serde_json::to_string_pretty(value)? };
    println!("{out}");
    Ok(())
}
