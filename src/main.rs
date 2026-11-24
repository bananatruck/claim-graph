use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand, ValueEnum};
use serde::Serialize;

use claim_graph::answers::{self, Audit, Explanation, FigureTrace, Report, Stale, Table};
use claim_graph::draft::parse_draft;
use claim_graph::runlog::parse_run_log;
use claim_graph::store;

/// Exit status when `--fail` is given and the store has findings.
const FINDINGS: u8 = 3;

/// Report every claim in a research draft that has no path to a completed run or a cited source.
///
/// Claim Graph reads a Markdown draft and a JSON Lines run log into a single-file Relate
/// graph store, then answers every question from that store with Cypher.
///
/// Exit status: 0 success, 1 runtime or input error, 2 usage error,
/// 3 findings when --fail is given.
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
    /// Example: claim-graph unsupported --store store/claim-graph.rl --fail
    Unsupported {
        #[command(flatten)]
        common: Common,
        /// Also list the sentences the claim rule treated as prose.
        #[arg(long)]
        prose: bool,
        /// Exit with status 3 when any claim is unsupported.
        #[arg(long)]
        fail: bool,
    },
    /// Explain one sentence: each reference it makes, why it does or does not support it,
    /// every supporting path, and where its numbers came from.
    ///
    /// Example: claim-graph explain --store store/claim-graph.rl --claim C4
    Explain {
        #[command(flatten)]
        common: Common,
        /// Sentence ID, such as C4 or P1.
        #[arg(long)]
        claim: String,
    },
    /// Trace numbers in the draft to the metric, run and experiment that produced them.
    ///
    /// Example: claim-graph trace --store store/claim-graph.rl --figure "71.3%"
    Trace {
        #[command(flatten)]
        common: Common,
        /// The number exactly as written in the draft, such as "71.3%" or "6.1".
        #[arg(long)]
        figure: Option<String>,
        /// Trace every number in one claim instead.
        #[arg(long)]
        claim: Option<String>,
    },
    /// Run every check: unsupported claims, claims on superseded experiments, and
    /// figures that the runs a claim cites did not produce.
    ///
    /// Example: claim-graph audit --store store/messy.rl --fail
    Audit {
        #[command(flatten)]
        common: Common,
        /// Exit with status 3 when there is any finding.
        #[arg(long)]
        fail: bool,
    },
    /// Run one read-only Cypher statement against the store.
    ///
    /// Example: claim-graph query --store store/claim-graph.rl "MATCH (c:Claim) RETURN c.id, c.text"
    Query {
        #[command(flatten)]
        common: Common,
        /// The Cypher statement.
        cypher: String,
    },
    /// Print every node and relationship as one sorted line, to compare stores by content.
    ///
    /// Example: claim-graph dump --store store/claim-graph.rl
    Dump {
        /// Relate store written by `ingest`; opened read-only.
        #[arg(long)]
        store: PathBuf,
    },
}

#[derive(Args)]
struct Common {
    /// Relate store written by `ingest`; opened read-only.
    #[arg(long)]
    store: PathBuf,
    /// Output format.
    #[arg(long, value_enum, default_value_t = Format::Text)]
    format: Format,
}

#[derive(Clone, Copy, PartialEq, ValueEnum)]
enum Format {
    Text,
    Json,
}

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(code) => code,
        Err(message) => {
            eprintln!("error: {message}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> Result<ExitCode, String> {
    let fail_if = |fail: bool, findings: usize| {
        if fail && findings > 0 {
            ExitCode::from(FINDINGS)
        } else {
            ExitCode::SUCCESS
        }
    };
    match cli.command {
        Command::Ingest {
            draft,
            runs,
            store,
            replace,
        } => {
            ingest(&draft, &runs, &store, replace)?;
            Ok(ExitCode::SUCCESS)
        }
        Command::Unsupported {
            common,
            prose,
            fail,
        } => {
            let mut report = answers::unsupported(&common.store).map_err(|e| e.to_string())?;
            let findings = report.unsupported.len();
            if !prose {
                report.prose.clear();
            }
            emit(common.format, &report, render_report)?;
            Ok(fail_if(fail, findings))
        }
        Command::Explain { common, claim } => {
            let explanation = answers::explain(&common.store, &claim).map_err(|e| e.to_string())?;
            emit(common.format, &explanation, render_explanation)?;
            Ok(ExitCode::SUCCESS)
        }
        Command::Trace {
            common,
            figure,
            claim,
        } => {
            if figure.is_none() && claim.is_none() {
                return Err("trace needs --figure or --claim".into());
            }
            let traces = answers::trace(&common.store, figure.as_deref(), claim.as_deref())
                .map_err(|e| e.to_string())?;
            if traces.is_empty() {
                return Err(match (figure, claim) {
                    (Some(f), _) => format!("no claim in the store states {f:?}"),
                    (None, Some(c)) => format!("claim {c} states no numbers"),
                    (None, None) => unreachable!(),
                });
            }
            emit(common.format, &traces, |t| render_traces(t, ""))?;
            Ok(ExitCode::SUCCESS)
        }
        Command::Audit { common, fail } => {
            let audit = answers::audit(&common.store).map_err(|e| e.to_string())?;
            emit(common.format, &audit, render_audit)?;
            Ok(fail_if(fail, audit.findings()))
        }
        Command::Query { common, cypher } => {
            let table = answers::query(&common.store, &cypher).map_err(|e| e.to_string())?;
            emit(common.format, &table, render_table)?;
            Ok(ExitCode::SUCCESS)
        }
        Command::Dump { store } => {
            let mut lines = answers::dump(&store).map_err(|e| e.to_string())?.join("\n");
            lines.push('\n');
            write_stdout(&lines)?;
            Ok(ExitCode::SUCCESS)
        }
    }
}

fn emit<T: Serialize>(
    format: Format,
    value: &T,
    text: impl Fn(&T) -> String,
) -> Result<(), String> {
    let output = match format {
        Format::Text => text(value),
        Format::Json => serde_json::to_string_pretty(value).map_err(|e| e.to_string())? + "\n",
    };
    write_stdout(&output)
}

/// Write to stdout; a reader that stops early (such as `head`) is not an error.
fn write_stdout(output: &str) -> Result<(), String> {
    use std::io::Write;
    let mut stdout = std::io::stdout().lock();
    match stdout
        .write_all(output.as_bytes())
        .and_then(|()| stdout.flush())
    {
        Err(e) if e.kind() != std::io::ErrorKind::BrokenPipe => Err(e.to_string()),
        _ => Ok(()),
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
    let s = store::ingest(
        store_path,
        &draft_path.to_string_lossy(),
        &draft,
        &log,
        replace,
    )
    .map_err(|e| e.to_string())?;
    for (claim, reference) in &s.unresolved {
        eprintln!("warning: {claim} references {reference}, which is not defined");
    }
    println!(
        "wrote {}: {} claims, {} prose sentences, {} sources, {} experiments, {} runs, {} metrics, \
         {} figures, {} figure-metric matches, {} differences",
        store_path.display(),
        s.claims,
        s.prose,
        s.sources,
        s.experiments,
        s.runs,
        s.metrics,
        s.figures,
        s.matches,
        s.derivations
    );
    Ok(())
}

fn section(name: &str) -> &str {
    if name.is_empty() { "-" } else { name }
}

fn render_report(report: &Report) -> String {
    let mut out = String::new();
    if report.unsupported.is_empty() {
        let _ = writeln!(
            out,
            "All {} claims have a path to a completed run or a cited source.",
            report.total_claims
        );
    } else {
        let _ = writeln!(
            out,
            "{} of {} claims have no path to a completed run or a cited source:\n",
            report.unsupported.len(),
            report.total_claims
        );
    }
    for claim in &report.unsupported {
        let _ = writeln!(
            out,
            "{}  [{}]  {}",
            claim.id,
            section(&claim.section),
            claim.text
        );
        if !claim.unresolved.is_empty() {
            let _ = writeln!(out, "    unresolved: {}", claim.unresolved.join(", "));
        }
    }
    if !report.prose.is_empty() {
        let _ = writeln!(
            out,
            "\n{} sentences are prose (no number, comparison or reference) and were not checked:\n",
            report.prose.len()
        );
        for p in &report.prose {
            let _ = writeln!(out, "{}  [{}]  {}", p.id, section(&p.section), p.text);
        }
    }
    out
}

fn render_explanation(e: &Explanation) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "{}  [{}]  {}", e.id, section(&e.section), e.text);
    if !e.is_claim {
        let _ = writeln!(
            out,
            "\nProse: no number, comparison or reference, so it is not checked."
        );
        return out;
    }
    let _ = writeln!(out, "Checked because it has: {}", e.reasons.join(", "));
    let _ = writeln!(out, "\nReferences:");
    if e.links.is_empty() {
        let _ = writeln!(out, "  none");
    }
    for l in &e.links {
        let mark = if l.supports {
            "supports"
        } else {
            "does not support"
        };
        let _ = writeln!(out, "  {} {}: {} ({})", l.kind, l.target, mark, l.reason);
    }
    let _ = writeln!(out, "\nSupporting paths:");
    if e.support_paths.is_empty() {
        let _ = writeln!(
            out,
            "  none: no path reaches a completed run or a listed source, so the claim is unsupported"
        );
    }
    for path in &e.support_paths {
        let _ = writeln!(out, "  {}", path.join(" -> "));
    }
    for s in &e.stale {
        out.push_str(&render_stale(s));
    }
    if !e.figures.is_empty() {
        let _ = writeln!(out, "\nNumbers:");
        out.push_str(&render_traces(&e.figures, "  "));
    }
    out
}

fn render_stale(s: &Stale) -> String {
    let mut out = format!(
        "\nStale: {} rests on {}, from experiment {}, which is superseded: {}\n",
        s.claim,
        s.via,
        s.chain.last().map_or("?", String::as_str),
        s.chain.join(" supersedes ")
    );
    if s.newest_results.is_empty() {
        let _ = writeln!(out, "  the newest experiment has no completed results");
    } else {
        let _ = writeln!(
            out,
            "  newest results ({}): {}",
            s.chain.first().map_or("?", String::as_str),
            s.newest_results.join("; ")
        );
    }
    out
}

fn render_traces(traces: &[FigureTrace], indent: &str) -> String {
    let mut out = String::new();
    for t in traces {
        let _ = writeln!(out, "{indent}{} in {}:", t.figure, t.claim);
        if !t.measured {
            let _ = writeln!(
                out,
                "{indent}  a whole number, treated as a count and not matched to metrics"
            );
            continue;
        }
        for m in &t.matches {
            let cited = if m.cited {
                "cited by the claim"
            } else {
                "NOT cited by the claim"
            };
            let _ = writeln!(
                out,
                "{indent}  = {} {} from run {} ({}) in experiment {}, {}",
                m.metric, m.value, m.run, m.status, m.experiment, cited
            );
            if !m.other_runs.is_empty() {
                let _ = writeln!(
                    out,
                    "{indent}    other runs of {}: {}",
                    m.experiment,
                    m.other_runs.join("; ")
                );
            }
        }
        for d in &t.differences {
            let cited = if d.cited {
                "cited by the claim"
            } else {
                "NOT cited by the claim"
            };
            let _ = writeln!(
                out,
                "{indent}  = difference of {}: run {} ({}) minus run {} ({}) in experiment {}, {}",
                d.metric,
                d.minuend_run,
                d.minuend,
                d.subtrahend_run,
                d.subtrahend,
                d.experiment,
                cited
            );
        }
        if t.matches.is_empty() && t.differences.is_empty() {
            let _ = writeln!(
                out,
                "{indent}  no run in the log produced this number, directly or as a difference"
            );
        }
    }
    out
}

fn render_audit(a: &Audit) -> String {
    let mut out = String::from("Unsupported claims\n\n");
    let mut report = a.report.clone();
    report.prose.clear();
    out.push_str(&render_report(&report));
    let _ = writeln!(out, "\nClaims on superseded experiments");
    if a.stale.is_empty() {
        let _ = writeln!(out, "\nnone");
    }
    for s in &a.stale {
        out.push_str(&render_stale(s));
    }
    let _ = writeln!(out, "\nFigures the cited runs did not produce\n");
    if a.mismatches.is_empty() {
        let _ = writeln!(out, "none");
    }
    for m in &a.mismatches {
        let _ = writeln!(
            out,
            "{} states {}, but its cited runs measured: {}",
            m.claim,
            m.figure,
            m.cited_results.join("; ")
        );
        if !m.stated_by.is_empty() {
            let _ = writeln!(
                out,
                "    the values those runs produced are stated by {}",
                m.stated_by.join(", ")
            );
        }
    }
    let _ = writeln!(out, "\n{} findings.", a.findings());
    out
}

fn render_table(t: &Table) -> String {
    let mut out = t.columns.join("\t");
    out.push('\n');
    for row in &t.rows {
        out.push_str(&row.join("\t"));
        out.push('\n');
    }
    let _ = writeln!(out, "({} rows)", t.rows.len());
    out
}
