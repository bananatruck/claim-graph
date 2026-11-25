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
    assert!(stdout.contains("5 findings."), "{stdout}");
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

#[test]
fn trace_needs_exactly_one_selector() {
    let (_dir, store) = clean_store();
    let store = store.to_str().unwrap();
    assert_eq!(run(&["trace", "--store", store]).status.code(), Some(2));
    assert_eq!(
        run(&[
            "trace", "--store", store, "--figure", "60%", "--claim", "C1"
        ])
        .status
        .code(),
        Some(2)
    );
}

#[test]
fn trace_names_an_unknown_claim() {
    let (_dir, store) = clean_store();
    let out = run(&[
        "trace",
        "--store",
        store.to_str().unwrap(),
        "--claim",
        "C99",
    ]);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        text(&out.stderr).contains("no sentence with ID C99"),
        "{}",
        text(&out.stderr)
    );
}

#[test]
fn audit_and_explain_show_broken_references_and_mismatches() {
    let (_dir, store) = messy_store();
    let store = store.to_str().unwrap();
    let stdout = ok(&["audit", "--store", store]);
    assert!(
        stdout.contains("C6 cites source ndiaye2022: not listed in References"),
        "{stdout}"
    );
    assert!(stdout.contains("5 findings."), "{stdout}");
    let stdout = ok(&["explain", "--store", store, "--claim", "C1"]);
    assert!(
        stdout.contains("Mismatch: 74.2% is not what its cited runs measured"),
        "{stdout}"
    );
}

#[test]
fn ingest_succeeds_when_the_reader_of_its_summary_has_gone() {
    let dir = tempfile::tempdir().unwrap();
    let store = dir.path().join("out.rl");
    let mut child = Command::new(BIN)
        .args([
            "ingest",
            "--draft",
            &input("data/synthetic/draft.md"),
            "--runs",
            &input("data/synthetic/runs.jsonl"),
            "--store",
            store.to_str().unwrap(),
        ])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    // Close the read end before ingest writes its summary.
    drop(child.stdout.take());
    let output = child.wait_with_output().unwrap();
    assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
    assert!(store.exists());
}

/// `--replace` must never replace one of the ingest's own inputs: not by the same path, a
/// relative path to it, or a symbolic link on either side.
#[cfg(unix)]
#[test]
fn replace_refuses_a_store_path_that_names_an_input() {
    let dir = tempfile::tempdir().unwrap();
    let draft_text = std::fs::read_to_string(input("data/synthetic/draft.md")).unwrap();
    let runs_text = std::fs::read_to_string(input("data/synthetic/runs.jsonl")).unwrap();
    let draft = dir.path().join("paper.rl");
    let runs = dir.path().join("runs.rl");
    std::fs::write(&draft, &draft_text).unwrap();
    std::fs::write(&runs, &runs_text).unwrap();
    std::os::unix::fs::symlink(&draft, dir.path().join("link.rl")).unwrap();
    std::os::unix::fs::symlink(
        dir.path().join("target.rl"),
        dir.path().join("draft-link.md"),
    )
    .unwrap();
    std::fs::write(dir.path().join("target.rl"), &draft_text).unwrap();
    for (draft_arg, store_arg) in [
        ("paper.rl", "paper.rl"),
        ("paper.rl", "./sub/../paper.rl"),
        ("paper.rl", "runs.rl"),
        ("paper.rl", "link.rl"),
        ("draft-link.md", "target.rl"),
    ] {
        std::fs::create_dir_all(dir.path().join("sub")).unwrap();
        let out = Command::new(BIN)
            .current_dir(dir.path())
            .args([
                "ingest",
                "--draft",
                draft_arg,
                "--runs",
                "runs.rl",
                "--store",
                store_arg,
                "--replace",
            ])
            .output()
            .unwrap();
        assert_eq!(
            out.status.code(),
            Some(1),
            "{draft_arg} {store_arg}: {}",
            text(&out.stderr)
        );
        assert!(
            text(&out.stderr).contains("is an input"),
            "{}",
            text(&out.stderr)
        );
        assert_eq!(std::fs::read_to_string(&draft).unwrap(), draft_text);
        assert_eq!(std::fs::read_to_string(&runs).unwrap(), runs_text);
        assert_eq!(
            std::fs::read_to_string(dir.path().join("target.rl")).unwrap(),
            draft_text
        );
    }
    // A store that is not an input is still replaced.
    let out = Command::new(BIN)
        .current_dir(dir.path())
        .args([
            "ingest", "--draft", "paper.rl", "--runs", "runs.rl", "--store", "out.rl",
        ])
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", text(&out.stderr));
    let out = Command::new(BIN)
        .current_dir(dir.path())
        .args([
            "ingest",
            "--draft",
            "paper.rl",
            "--runs",
            "runs.rl",
            "--store",
            "out.rl",
            "--replace",
        ])
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", text(&out.stderr));
}

/// The store's snapshot codec is JSON whatever Relate's environment says.
#[test]
fn the_snapshot_codec_does_not_follow_the_environment() {
    let dir = tempfile::tempdir().unwrap();
    let store = dir.path().join("claims.rl");
    let (draft, runs) = (
        input("data/synthetic/draft.md"),
        input("data/synthetic/runs.jsonl"),
    );
    let out = Command::new(BIN)
        .env("RELATE_SNAPSHOT_FORMAT", "markdown")
        .args([
            "ingest",
            "--draft",
            &draft,
            "--runs",
            &runs,
            "--store",
            store.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", text(&out.stderr));
    match relate_storage::store_info(&store).unwrap().snapshot {
        relate_storage::StoreSnapshot::Envelope { codec, .. } => {
            assert_eq!(codec, relate::SnapshotCodecKind::Json)
        }
        other => panic!("expected a JSON snapshot, found {other:?}"),
    }
}
