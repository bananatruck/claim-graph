//! End-to-end tests of the built binary on the synthetic inputs.

use std::path::Path;
use std::process::{Command, Output};

const BIN: &str = env!("CARGO_BIN_EXE_claim-graph");
const DRAFT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/data/synthetic/draft.md");
const RUNS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/data/synthetic/runs.jsonl");

fn run(args: &[&str]) -> Output {
    Command::new(BIN).args(args).output().unwrap()
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8(bytes.to_vec()).unwrap()
}

fn ingest(store: &Path, extra: &[&str]) -> Output {
    let store = store.to_str().unwrap();
    let mut args = vec!["ingest", "--draft", DRAFT, "--runs", RUNS, "--store", store];
    args.extend_from_slice(extra);
    run(&args)
}

#[test]
fn ingest_then_report_in_text() {
    let dir = tempfile::tempdir().unwrap();
    let store = dir.path().join("claims.rl");

    let out = ingest(&store, &[]);
    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    let stdout = text(&out.stdout);
    assert!(
        stdout.contains("7 claims, 2 sources, 3 experiments, 3 runs, 2 metrics"),
        "{stdout}"
    );
    let stderr = text(&out.stderr);
    assert!(
        stderr.contains("warning: C7 references @okafor2023"),
        "{stderr}"
    );

    let out = run(&["unsupported", "--store", store.to_str().unwrap()]);
    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    let stdout = text(&out.stdout);
    assert!(
        stdout.starts_with("4 of 7 claims have no path to a completed run or a cited source"),
        "{stdout}"
    );
    let ids: Vec<&str> = stdout
        .lines()
        .filter_map(|line| line.split_whitespace().next())
        .filter(|word| word.starts_with('C') && word[1..].chars().all(|c| c.is_ascii_digit()))
        .collect();
    assert_eq!(ids, ["C3", "C4", "C6", "C7"], "{stdout}");
    assert!(stdout.contains("unresolved: @okafor2023"), "{stdout}");
}

#[test]
fn report_in_json() {
    let dir = tempfile::tempdir().unwrap();
    let store = dir.path().join("claims.rl");
    assert!(ingest(&store, &[]).status.success());

    let out = run(&[
        "unsupported",
        "--store",
        store.to_str().unwrap(),
        "--format",
        "json",
    ]);
    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(report["total_claims"], 7);
    let ids: Vec<&str> = report["unsupported"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, ["C3", "C4", "C6", "C7"]);
    assert_eq!(report["unsupported"][3]["unresolved"][0], "@okafor2023");
}

#[test]
fn an_existing_store_needs_replace() {
    let dir = tempfile::tempdir().unwrap();
    let store = dir.path().join("claims.rl");
    assert!(ingest(&store, &[]).status.success());

    let out = ingest(&store, &[]);
    assert_eq!(out.status.code(), Some(1));
    assert!(text(&out.stderr).contains("--replace"));

    let out = ingest(&store, &["--replace"]);
    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
}

#[test]
fn a_bad_input_reports_file_and_line() {
    let dir = tempfile::tempdir().unwrap();
    let bad = dir.path().join("bad.md");
    std::fs::write(&bad, "# T\n\n## References\n\nnot an entry\n").unwrap();
    let store = dir.path().join("claims.rl");
    let out = run(&[
        "ingest",
        "--draft",
        bad.to_str().unwrap(),
        "--runs",
        RUNS,
        "--store",
        store.to_str().unwrap(),
    ]);
    assert_eq!(out.status.code(), Some(1));
    let stderr = text(&out.stderr);
    assert!(stderr.contains("bad.md: line 5"), "{stderr}");
    assert!(!store.exists(), "no store is written for bad input");
}

#[test]
fn missing_arguments_are_a_usage_error() {
    let out = run(&["ingest"]);
    assert_eq!(out.status.code(), Some(2));
}
