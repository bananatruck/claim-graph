use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand, ValueEnum};

use claim_graph::draft::parse_draft;
use claim_graph::runlog::parse_run_log;
use claim_graph::store::{self, Report};

/// Report every claim in a research draft that has no path to a completed run or a cited source.
///
/// Claim Graph reads a Markdown draft and a JSON Lines run log into a single-file Relate
/// graph store, then answers questions from that store with Cypher.
#[derive(Parser)]
#[command(version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Build a new store from a draft and a run log.
    ///
    /// Example: claim-graph ingest --draft data/synthetic/draft.md --runs data/synthetic/runs.jsonl --store store/claim-graph.rl
    Ingest {
        /// Markdown draft (see SPEC.md for the format).
        #[arg(long)]
        draft: PathBuf,
        /// JSON Lines run log (see SPEC.md for the format).
        #[arg(long)]
        runs: PathBuf,
        /// Relate store to create; must end in .rl.
        #[arg(long)]
        store: PathBuf,
        /// Delete an existing store file at --store and rebuild it.
        #[arg(long)]
        replace: bool,
    },
    /// List the claims with no path to a completed run or a cited source.
    ///
    /// Example: claim-graph unsupported --store store/claim-graph.rl --format json
    Unsupported {
        /// Relate store written by `ingest`; opened read-only.
        #[arg(long)]
        store: PathBuf,
        /// Output format.
        #[arg(long, value_enum, default_value_t = Format::Text)]
        format: Format,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum Format {
    Text,
    Json,
}

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("error: {message}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> Result<(), String> {
    match cli.command {
        Command::Ingest {
            draft,
            runs,
            store,
            replace,
        } => ingest(&draft, &runs, &store, replace),
        Command::Unsupported { store, format } => {
            let report = store::unsupported(&store).map_err(|e| e.to_string())?;
            match format {
                Format::Text => print!("{}", render_text(&report)),
                Format::Json => println!(
                    "{}",
                    serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?
                ),
            }
            Ok(())
        }
    }
}

fn ingest(
    draft_path: &Path,
    runs_path: &Path,
    store_path: &Path,
    replace: bool,
) -> Result<(), String> {
    let read =
        |path: &Path| std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()));
    let draft =
        parse_draft(&read(draft_path)?).map_err(|e| format!("{}: {e}", draft_path.display()))?;
    let log =
        parse_run_log(&read(runs_path)?).map_err(|e| format!("{}: {e}", runs_path.display()))?;
    let summary = store::ingest(
        store_path,
        &draft_path.to_string_lossy(),
        &draft,
        &log,
        replace,
    )
    .map_err(|e| e.to_string())?;
    for (claim, reference) in &summary.unresolved {
        eprintln!("warning: {claim} references {reference}, which is not defined");
    }
    println!(
        "wrote {}: {} claims, {} sources, {} experiments, {} runs, {} metrics",
        store_path.display(),
        summary.claims,
        summary.sources,
        summary.experiments,
        summary.runs,
        summary.metrics
    );
    Ok(())
}

fn render_text(report: &Report) -> String {
    if report.unsupported.is_empty() {
        return format!(
            "All {} claims have a path to a completed run or a cited source.\n",
            report.total_claims
        );
    }
    let mut out = format!(
        "{} of {} claims have no path to a completed run or a cited source:\n\n",
        report.unsupported.len(),
        report.total_claims
    );
    for claim in &report.unsupported {
        let section = if claim.section.is_empty() {
            "-"
        } else {
            &claim.section
        };
        out.push_str(&format!("{}  [{}]  {}\n", claim.id, section, claim.text));
        if !claim.unresolved.is_empty() {
            out.push_str(&format!(
                "    unresolved: {}\n",
                claim.unresolved.join(", ")
            ));
        }
    }
    out
}
