//! End-to-end tests of the built binary on the synthetic inputs.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const BIN: &str = env!("CARGO_BIN_EXE_claim-graph");
const ROOT: &str = env!("CARGO_MANIFEST_DIR");

fn run(args: &[&str]) -> Output {
    Command::new(BIN).args(args).output().unwrap()
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8(bytes.to_vec()).unwrap()
}

fn input(relative: &str) -> String {
    format!("{ROOT}/{relative}")
}

fn ingest(store: &Path, draft: &str, runs: &str, extra: &[&str]) -> Output {
    let (draft, runs) = (input(draft), input(runs));
    let mut args = vec![
        "ingest",
        "--draft",
        &draft,
        "--runs",
        &runs,
        "--store",
        store.to_str().unwrap(),
    ];
    args.extend_from_slice(extra);
    run(&args)
}

fn clean_store() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let store = dir.path().join("claims.rl");
    let out = ingest(
        &store,
        "data/synthetic/draft.md",
        "data/synthetic/runs.jsonl",
        &[],
    );
    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    (dir, store)
}

fn messy_store() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let store = dir.path().join("messy.rl");
    let out = ingest(
        &store,
        "data/synthetic/messy/draft.md",
        "data/synthetic/messy/runs.jsonl",
        &[],
    );
    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    (dir, store)
}

fn ok(args: &[&str]) -> String {
    let out = run(args);
    assert!(
        out.status.success(),
        "{args:?} failed: {}",
        text(&out.stderr)
    );
    text(&out.stdout)
}

#[test]
fn ingest_then_report_in_text() {
    let dir = tempfile::tempdir().unwrap();
    let store = dir.path().join("claims.rl");
    let out = ingest(
        &store,
        "data/synthetic/draft.md",
        "data/synthetic/runs.jsonl",
        &[],
    );
    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
    let stdout = text(&out.stdout);
    assert!(
        stdout.contains("7 claims, 1 prose sentences, 2 sources, 3 experiments, 4 runs"),
        "{stdout}"
    );
    let stderr = text(&out.stderr);
    assert!(
        stderr.contains("warning: C7 references @okafor2023"),
        "{stderr}"
    );

    let stdout = ok(&["unsupported", "--store", store.to_str().unwrap()]);
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
    assert!(!stdout.contains("P1"), "prose is listed only with --prose");

    let stdout = ok(&["unsupported", "--store", store.to_str().unwrap(), "--prose"]);
    assert!(
        stdout.contains("P1  [Abstract]  We study a planner"),
        "{stdout}"
    );
}

#[test]
fn report_in_json() {
    let (_dir, store) = clean_store();
    let stdout = ok(&[
        "unsupported",
        "--store",
        store.to_str().unwrap(),
        "--format",
        "json",
    ]);
    let report: serde_json::Value = serde_json::from_str(&stdout).unwrap();
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
fn fail_exits_with_status_3_when_there_are_findings() {
    let (_dir, store) = clean_store();
    let store = store.to_str().unwrap();
    assert_eq!(
        run(&["unsupported", "--store", store, "--fail"])
            .status
            .code(),
        Some(3)
    );
    assert_eq!(
        run(&["audit", "--store", store, "--fail"]).status.code(),
        Some(3)
    );
    assert_eq!(run(&["audit", "--store", store]).status.code(), Some(0));
}

#[test]
fn explain_names_the_broken_link() {
    let (_dir, store) = clean_store();
    let store = store.to_str().unwrap();
    let stdout = ok(&["explain", "--store", store, "--claim", "C4"]);
    assert!(
        stdout.contains("experiment E2: does not support (experiment has no runs)"),
        "{stdout}"
    );
    assert!(stdout.contains("so the claim is unsupported"), "{stdout}");
    let stdout = ok(&["explain", "--store", store, "--claim", "C2"]);
    assert!(stdout.contains("C2 -> E1 -> r-002"), "{stdout}");
    let out = run(&["explain", "--store", store, "--claim", "C99"]);
    assert_eq!(out.status.code(), Some(1));
}

#[test]
fn trace_answers_where_a_number_came_from() {
    let (_dir, store) = clean_store();
    let store = store.to_str().unwrap();
    let stdout = ok(&["trace", "--store", store, "--figure", "71.3%"]);
    assert!(
        stdout.contains(
            "= success_rate 0.713 from run r-002 (completed) in experiment E1, cited by the claim"
        ),
        "{stdout}"
    );
    let stdout = ok(&["trace", "--store", store, "--figure", "6.1"]);
    assert!(
        stdout.contains("difference of success_rate: run r-002 (0.713) minus run r-001 (0.652)"),
        "{stdout}"
    );
    let stdout = ok(&["trace", "--store", store, "--figure", "60%"]);
    assert!(
        stdout.contains("no run in the log produced this number"),
        "{stdout}"
    );
    let out = run(&["trace", "--store", store, "--figure", "99%"]);
    assert_eq!(out.status.code(), Some(1));
}

#[test]
fn audit_reports_stale_and_mismatched_claims_on_the_messy_draft() {
    let (_dir, store) = messy_store();
    let stdout = ok(&["audit", "--store", store.to_str().unwrap()]);
    assert!(
        stdout.contains(
            "Stale: C5 rests on m-01, from experiment E10, which is superseded: E12 supersedes E11 supersedes E10"
        ),
        "{stdout}"
    );
    assert!(
        stdout.contains("C1 states 74.2%, but its cited runs measured: m-07 accuracy = 0.739"),
        "{stdout}"
    );
    assert!(stdout.contains("4 findings."), "{stdout}");
}

#[test]
fn query_and_dump_read_the_store() {
    let (_dir, store) = clean_store();
    let store = store.to_str().unwrap();
    let stdout = ok(&[
        "query",
        "--store",
        store,
        "MATCH (s:Source) WHERE NOT EXISTS { MATCH (:Draft)-[:LISTS]->(s) } RETURN s.key AS key",
    ]);
    assert_eq!(stdout, "key\n'okafor2023'\n(1 rows)\n");
    let stdout = ok(&["dump", "--store", store]);
    assert!(stdout.lines().count() > 40, "{stdout}");
}

#[test]
fn an_existing_store_needs_replace() {
    let (_dir, store) = clean_store();
    let out = ingest(
        &store,
        "data/synthetic/draft.md",
        "data/synthetic/runs.jsonl",
        &[],
    );
    assert_eq!(out.status.code(), Some(1));
    assert!(text(&out.stderr).contains("--replace"));
    let out = ingest(
        &store,
        "data/synthetic/draft.md",
        "data/synthetic/runs.jsonl",
        &["--replace"],
    );
    assert!(out.status.success(), "stderr: {}", text(&out.stderr));
}

#[test]
fn a_bad_input_reports_file_and_line() {
    let dir = tempfile::tempdir().unwrap();
    let bad = dir.path().join("bad.md");
    std::fs::write(&bad, "# T\n\n## References\n\nnot an entry\n").unwrap();
    let store = dir.path().join("claims.rl");
    let runs = input("data/synthetic/runs.jsonl");
    let out = run(&[
        "ingest",
        "--draft",
        bad.to_str().unwrap(),
        "--runs",
        &runs,
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
    assert_eq!(run(&["ingest"]).status.code(), Some(2));
}
