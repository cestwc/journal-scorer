use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "journal-scorer", version, about = "Rubric-based scoring of journal reflections")]
pub struct Cli {
    #[command(flatten)]
    pub global: GlobalArgs,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Args)]
pub struct GlobalArgs {
    /// Config file (default: ./journal-scorer.toml if it exists).
    #[arg(long, global = true, env = "JOURNAL_SCORER_CONFIG")]
    pub config: Option<PathBuf>,
    /// Rubric TOML file (default: built-in AI Journal rubric).
    #[arg(long, global = true)]
    pub rubric: Option<PathBuf>,
    /// OpenAI-compatible base URL, e.g. http://127.0.0.1:8080/v1 (overrides config).
    #[arg(long, global = true)]
    pub base_url: Option<String>,
    /// Model name (overrides config).
    #[arg(long, global = true)]
    pub model: Option<String>,
    /// Environment variable that holds the API key (overrides config).
    #[arg(long, global = true)]
    pub api_key_env: Option<String>,
    /// Use the offline mock provider (returns no evidence).
    #[arg(long, global = true)]
    pub mock: bool,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Score a reflection and print the JSON report.
    Score {
        /// Text file to read, or `-` for stdin.
        #[arg(default_value = "-")]
        input: String,
        /// Include every analysed span in the output.
        #[arg(long)]
        include_spans: bool,
        /// Single-line JSON.
        #[arg(long)]
        compact: bool,
    },
    /// Score a PDF and write an annotated copy with an appended score sheet.
    Annotate {
        /// PDF to score.
        input: PathBuf,
        /// Output PDF (default: <input>-annotated.pdf).
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// Also write the JSON report here.
        #[arg(long)]
        report: Option<PathBuf>,
    },
    /// Start the web UI and HTTP API.
    Serve {
        /// Listen address (overrides config), e.g. 127.0.0.1:3000.
        #[arg(long)]
        addr: Option<String>,
    },
    /// Show how text or a PDF is split into spans (no LLM call).
    Segment {
        /// Text file, PDF, or `-` for stdin.
        #[arg(default_value = "-")]
        input: String,
        #[arg(long)]
        compact: bool,
    },
    /// Print the active rubric as JSON.
    Rubric {
        #[arg(long)]
        compact: bool,
    },
}
