//! Store tests on real `.rl` files: every answer is read back from a reopened Relate store.

use std::path::{Path, PathBuf};

use claim_graph::answers::{self, Difference, Link, MetricMatch, Mismatch, Stale};
use claim_graph::draft::parse_draft;
use claim_graph::runlog::parse_run_log;
use claim_graph::store::{self, IngestSummary, StoreError};
use relate::Database;

struct Dataset {
    draft_path: &'static str,
    draft: &'static str,
    runs: &'static str,
}

const CLEAN: Dataset = Dataset {
    draft_path: "data/synthetic/draft.md",
    draft: include_str!("../data/synthetic/draft.md"),
    runs: include_str!("../data/synthetic/runs.jsonl"),
};

const MESSY: Dataset = Dataset {
    draft_path: "data/synthetic/messy/draft.md",
    draft: include_str!("../data/synthetic/messy/draft.md"),
    runs: include_str!("../data/synthetic/messy/runs.jsonl"),
};

fn ingest(data: &Dataset, path: &Path, replace: bool) -> Result<IngestSummary, StoreError> {
    let draft = parse_draft(data.draft).unwrap();
    let log = parse_run_log(data.runs).unwrap();
    store::ingest(path, data.draft_path, &draft, &log, replace)
}

fn built(data: &Dataset) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("claims.rl");
    ingest(data, &path, false).unwrap();
    (dir, path)
}

fn count(path: &Path, cypher: &str) -> i64 {
    let db = Database::open_read_only(path.to_str().unwrap()).unwrap();
    let mut conn = db.connect().unwrap();
    let result = conn.query(cypher).unwrap();
    result.rows[0][0].as_int().unwrap()
}

fn ids<T>(items: &[T], id: impl Fn(&T) -> &str) -> Vec<&str> {
    items.iter().map(id).collect()
}

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| s.to_string()).collect()
}

#[test]
fn ingest_counts_what_the_graph_holds() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("claims.rl");
    let summary = ingest(&CLEAN, &path, false).unwrap();
    assert!(path.is_file(), "the store is a single file");
    assert_eq!(
        summary,
        IngestSummary {
            claims: 7,
            prose: 1,
            sources: 2,
            experiments: 3,
            runs: 4,
            metrics: 3,
            figures: 4,
            matches: 1,
            derivations: 1,
            unresolved: vec![("C7".into(), "@okafor2023".into())],
        }
    );
}

#[test]
fn the_reopened_store_holds_the_whole_graph() {
    let (_dir, path) = built(&CLEAN);
    let nodes = [
        ("Draft", 1),
        ("Sentence", 8),
        ("Claim", 7),
        ("Source", 3),
        ("Experiment", 3),
        ("Run", 4),
        ("Metric", 3),
        ("Figure", 4),
        ("Derivation", 1),
    ];
    for (label, expected) in nodes {
        let n = count(&path, &format!("MATCH (n:{label}) RETURN count(n)"));
        assert_eq!(n, expected, "nodes labelled {label}");
    }
    let rels = [
        ("HAS_SENTENCE", 8),
        ("LISTS", 2),
        ("CITES", 2),
        ("SUPPORTED_BY", 4),
        ("HAS_RUN", 4),
        ("PRODUCED", 3),
        ("STATES", 4),
        ("MATCHES", 1),
        ("DERIVED_AS", 1),
        ("MINUEND", 1),
        ("SUBTRAHEND", 1),
        ("SUPERSEDES", 0),
    ];
    for (rel_type, expected) in rels {
        let n = count(
            &path,
            &format!("MATCH ()-[r:{rel_type}]->() RETURN count(r)"),
        );
        assert_eq!(n, expected, "relationships of type {rel_type}");
    }
}

#[test]
fn unsupported_claims_and_prose_come_from_the_reopened_store() {
    let (_dir, path) = built(&CLEAN);
    let report = answers::unsupported(&path).unwrap();
    assert_eq!(report.total_claims, 7);
    assert_eq!(
        ids(&report.unsupported, |c| &c.id),
        ["C3", "C4", "C6", "C7"]
    );
    let c7 = &report.unsupported[3];
    assert_eq!(c7.section, "Results");
    assert_eq!(c7.unresolved, ["@okafor2023"]);
    assert!(report.unsupported[0].unresolved.is_empty());
    assert_eq!(ids(&report.prose, |p| &p.id), ["P1"]);
}

#[test]
fn the_report_follows_edits_made_to_the_graph() {
    let (_dir, path) = built(&CLEAN);
    {
        // Outside Claim Graph: give C3 a citation, and list the missing source.
        let db = Database::open(path.to_str().unwrap()).unwrap();
        let mut conn = db.connect().unwrap();
        conn.query(
            "MATCH (c:Claim {id: 'C3'}), (s:Source {key: 'chen2024'}) CREATE (c)-[:CITES]->(s)",
        )
        .unwrap();
        conn.query("MATCH (d:Draft), (s:Source {key: 'okafor2023'}) CREATE (d)-[:LISTS]->(s)")
            .unwrap();
    }
    let report = answers::unsupported(&path).unwrap();
    assert_eq!(ids(&report.unsupported, |c| &c.id), ["C4", "C6"]);
}

#[test]
fn explain_gives_the_reason_for_each_reference() {
    let (_dir, path) = built(&CLEAN);
    let link = |claim: &str, kind: &str, target: &str, supports: bool, reason: &str| Link {
        claim: claim.into(),
        kind: kind.into(),
        target: target.into(),
        supports,
        reason: reason.into(),
    };

    let c4 = answers::explain(&path, "C4").unwrap();
    assert!(c4.is_claim);
    assert_eq!(
        c4.links,
        [link(
            "C4",
            "experiment",
            "E2",
            false,
            "experiment has no runs"
        )]
    );
    assert!(c4.support_paths.is_empty());

    let c6 = answers::explain(&path, "C6").unwrap();
    assert_eq!(
        c6.links,
        [link("C6", "run", "r-004", false, "run status is failed")]
    );

    let c7 = answers::explain(&path, "C7").unwrap();
    assert_eq!(
        c7.links,
        [link(
            "C7",
            "source",
            "okafor2023",
            false,
            "not listed in References"
        )]
    );

    let c2 = answers::explain(&path, "C2").unwrap();
    assert_eq!(
        c2.links,
        [link(
            "C2",
            "experiment",
            "E1",
            true,
            "3 of its 3 runs completed"
        )]
    );
    assert_eq!(
        c2.support_paths,
        [
            strings(&["C2", "E1", "r-001"]),
            strings(&["C2", "E1", "r-002"]),
            strings(&["C2", "E1", "r-003"]),
        ]
    );
    assert_eq!(c2.figures.len(), 1);
    assert_eq!(c2.reasons, ["number", "comparison", "reference"]);
}

#[test]
fn explain_marks_prose_and_rejects_unknown_ids() {
    let (_dir, path) = built(&CLEAN);
    let p1 = answers::explain(&path, "P1").unwrap();
    assert!(!p1.is_claim);
    assert!(p1.reasons.is_empty());
    assert!(matches!(
        answers::explain(&path, "C99"),
        Err(StoreError::UnknownSentence(_))
    ));
}

#[test]
fn trace_follows_a_figure_to_its_run_and_the_experiment_siblings() {
    let (_dir, path) = built(&CLEAN);
    let traces = answers::trace(&path, Some("71.3%"), None).unwrap();
    assert_eq!(traces.len(), 1);
    assert_eq!(traces[0].claim, "C1");
    assert_eq!(
        traces[0].matches,
        [MetricMatch {
            metric: "success_rate".into(),
            value: 0.713,
            run: "r-002".into(),
            status: "completed".into(),
            experiment: "E1".into(),
            cited: true,
            other_runs: strings(&["r-001 (completed) = 0.652", "r-003 (completed) = 0.689"]),
        }]
    );
    assert!(traces[0].differences.is_empty());
}

#[test]
fn trace_finds_a_figure_that_is_a_difference_of_two_runs() {
    let (_dir, path) = built(&CLEAN);
    let traces = answers::trace(&path, Some("6.1"), None).unwrap();
    assert_eq!(traces.len(), 1);
    assert!(traces[0].matches.is_empty());
    assert_eq!(
        traces[0].differences,
        [Difference {
            metric: "success_rate".into(),
            minuend_run: "r-002".into(),
            minuend: 0.713,
            subtrahend_run: "r-001".into(),
            subtrahend: 0.652,
            experiment: "E1".into(),
            cited: true,
        }]
    );
}

#[test]
fn trace_says_plainly_when_no_run_produced_a_figure() {
    let (_dir, path) = built(&CLEAN);
    let traces = answers::trace(&path, Some("60%"), None).unwrap();
    assert_eq!(traces.len(), 1);
    assert!(traces[0].measured);
    assert!(traces[0].matches.is_empty() && traces[0].differences.is_empty());
    assert!(
        answers::trace(&path, Some("99.9%"), None)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn the_clean_draft_has_no_stale_or_mismatched_claims() {
    let (_dir, path) = built(&CLEAN);
    let audit = answers::audit(&path).unwrap();
    assert!(audit.stale.is_empty(), "{:?}", audit.stale);
    assert!(audit.mismatches.is_empty(), "{:?}", audit.mismatches);
    assert_eq!(audit.findings(), 4);
}

#[test]
fn the_messy_draft_parses_into_claims_and_prose() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("messy.rl");
    let summary = ingest(&MESSY, &path, false).unwrap();
    assert_eq!((summary.claims, summary.prose), (8, 6));
    assert_eq!(
        summary.unresolved,
        [
            ("C6".to_string(), "@ndiaye2022".to_string()),
            ("C7".to_string(), "run:m-99".to_string())
        ]
    );
    let report = answers::unsupported(&path).unwrap();
    assert_eq!(ids(&report.unsupported, |c| &c.id), ["C7", "C8"]);
}

#[test]
fn a_claim_on_a_superseded_experiment_is_stale_through_the_whole_chain() {
    let (_dir, path) = built(&MESSY);
    let audit = answers::audit(&path).unwrap();
    assert_eq!(
        audit.stale,
        [Stale {
            claim: "C5".into(),
            via: "m-01".into(),
            chain: strings(&["E12", "E11", "E10"]),
            newest_results: strings(&[
                "m-06 accuracy = 0.695",
                "m-06 tool_calls_per_question = 6.8",
                "m-07 accuracy = 0.739",
                "m-07 tool_calls_per_question = 4.21",
            ]),
        }]
    );
}

#[test]
fn two_claims_citing_one_run_with_different_values_are_a_mismatch() {
    let (_dir, path) = built(&MESSY);
    let audit = answers::audit(&path).unwrap();
    assert_eq!(
        audit.mismatches,
        [Mismatch {
            claim: "C1".into(),
            figure: "74.2%".into(),
            cited_results: strings(&[
                "m-07 accuracy = 0.739",
                "m-07 tool_calls_per_question = 4.21"
            ]),
            stated_by: strings(&["C3", "C4"]),
        }]
    );
    // 2 unsupported, 1 broken reference, 1 stale, 1 mismatch.
    assert_eq!(audit.findings(), 5);
}

#[test]
fn a_rounded_figure_matches_its_metric() {
    let (_dir, path) = built(&MESSY);
    let traces = answers::trace(&path, Some("71.3%"), None).unwrap();
    assert_eq!(traces[0].matches[0].run, "m-01");
    assert_eq!(traces[0].matches[0].value, 0.7128);
    let derived = answers::trace(&path, Some("4.4"), None).unwrap();
    assert_eq!(derived[0].differences[0].minuend_run, "m-07");
    assert_eq!(derived[0].differences[0].subtrahend_run, "m-06");
}

#[test]
fn query_runs_read_only_cypher() {
    let (_dir, path) = built(&CLEAN);
    let table = answers::query(
        &path,
        "MATCH (c:Claim)-[:SUPPORTED_BY]->(r:Run) RETURN c.id AS claim, r.id AS run ORDER BY claim",
    )
    .unwrap();
    assert_eq!(table.columns, ["claim", "run"]);
    assert_eq!(
        table.rows,
        [strings(&["'C1'", "'r-002'"]), strings(&["'C6'", "'r-004'"])]
    );
    assert!(answers::query(&path, "CREATE (:Claim {id: 'X'})").is_err());
}

#[test]
fn ingest_refuses_an_existing_store_unless_replace_is_given() {
    let (_dir, path) = built(&CLEAN);
    let err = ingest(&CLEAN, &path, false).unwrap_err();
    assert!(matches!(err, StoreError::Exists(_)), "got {err}");
    // Replace with a different graph, so a replacement that kept the old store would fail.
    ingest(&MESSY, &path, true).unwrap();
    let (_messy_dir, messy) = built(&MESSY);
    assert_eq!(
        answers::dump(&path).unwrap(),
        answers::dump(&messy).unwrap()
    );
    assert_ne!(
        answers::dump(&path).unwrap(),
        answers::dump(&built(&CLEAN).1).unwrap()
    );
}

#[test]
fn the_store_path_must_end_in_rl() {
    let dir = tempfile::tempdir().unwrap();
    let err = ingest(&CLEAN, &dir.path().join("claims"), false).unwrap_err();
    assert!(matches!(err, StoreError::NotSingleFile(_)), "got {err}");
}

#[test]
fn reporting_on_a_missing_store_is_an_error_and_creates_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("claims.rl");
    assert!(matches!(
        answers::unsupported(&path),
        Err(StoreError::Missing(_))
    ));
    assert!(!path.exists());
}

/// The committed stores must hold exactly the graph the documented commands build.
#[test]
fn committed_stores_match_a_rebuild_from_their_inputs() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    for (data, committed) in [(&CLEAN, "store/claim-graph.rl"), (&MESSY, "store/messy.rl")] {
        let (_dir, rebuilt) = built(data);
        let expected = answers::dump(&rebuilt).unwrap();
        let actual = answers::dump(&root.join(committed)).unwrap();
        assert_eq!(
            actual, expected,
            "{committed} is out of date; run scripts/rebuild-stores.sh"
        );
    }
}

/// Built without the `parquet` feature, the store uses Relate's JSON snapshot codec,
/// which every Relate surface opens.
#[test]
fn the_committed_store_is_a_single_file_bundle_with_the_json_codec() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let info = relate_storage::store_info(&root.join("store/claim-graph.rl")).unwrap();
    assert!(matches!(
        info.layout,
        relate_storage::StoreLayout::Bundle { .. }
    ));
    match info.snapshot {
        relate_storage::StoreSnapshot::Envelope { codec, .. } => {
            assert_eq!(codec, relate::SnapshotCodecKind::Json)
        }
        other => panic!("expected a JSON snapshot, found {other:?}"),
    }
}

// Review round 1: trigger inputs from outside review, pinned as tests.

fn built_from(draft: &str, runs: &str) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("claims.rl");
    let draft = parse_draft(draft).unwrap();
    let log = parse_run_log(runs).unwrap();
    store::ingest(&path, "draft.md", &draft, &log, false).unwrap();
    (dir, path)
}

fn mismatched(path: &Path) -> Vec<(String, String)> {
    answers::audit(path)
        .unwrap()
        .mismatches
        .into_iter()
        .map(|m| (m.claim, m.figure))
        .collect()
}

fn pair(claim: &str, figure: &str) -> (String, String) {
    (claim.to_string(), figure.to_string())
}

const E1_RUNS: &str = r#"{"kind":"experiment","id":"E1","name":"a"}
{"kind":"run","id":"r-1","experiment":"E1","status":"completed"}
{"kind":"run","id":"r-2","experiment":"E1","status":"completed"}
"#;

#[test]
fn an_uncited_direct_match_does_not_hide_a_cited_difference() {
    let runs = format!(
        "{}\n{}",
        CLEAN.runs,
        r#"{"kind":"experiment","id":"E9","name":"other"}
{"kind":"run","id":"r-999","experiment":"E9","status":"failed"}
{"kind":"metric","run":"r-999","name":"tool_calls","value":6.1}"#
    );
    let (_dir, path) = built_from(CLEAN.draft, &runs);
    assert!(mismatched(&path).is_empty());
    let traces = answers::trace(&path, Some("6.1"), None).unwrap();
    assert_eq!(traces[0].differences[0].minuend_run, "r-002");
    assert!(traces[0].differences[0].cited);
}

#[test]
fn a_difference_needs_both_runs_completed() {
    let runs = MESSY.runs.replace(
        r#""id":"m-06","experiment":"E12","seed":1,"status":"completed""#,
        r#""id":"m-06","experiment":"E12","seed":1,"status":"crashed""#,
    );
    let (_dir, path) = built_from(MESSY.draft, &runs);
    assert!(mismatched(&path).contains(&pair("C2", "4.4")));
}

#[test]
fn a_difference_needs_both_runs_cited() {
    let draft = MESSY.draft.replace(
        "over the plain agent [exp:E12]",
        "over the plain agent [run:m-07]",
    );
    let (_dir, path) = built_from(&draft, MESSY.runs);
    assert!(mismatched(&path).contains(&pair("C2", "4.4")));
    let traces = answers::trace(&path, Some("4.4"), None).unwrap();
    assert!(traces[0].differences.iter().all(|d| !d.cited));
}

#[test]
fn signs_are_checked() {
    let runs = format!(
        "{E1_RUNS}{}",
        r#"{"kind":"metric","run":"r-1","name":"loss_delta","value":-0.713}
{"kind":"metric","run":"r-2","name":"accuracy","value":0.739}
"#
    );
    let draft =
        "The loss delta is -0.713 [run:r-1].\nAblation changes accuracy by -73.9% [run:r-2].\n";
    let (_dir, path) = built_from(draft, &runs);
    assert_eq!(mismatched(&path), [pair("C2", "-73.9%")]);
}

#[test]
fn percent_and_point_figures_match_on_both_scales() {
    // Metrics on the 0..100 scale, and a percent difference.
    let runs = format!(
        "{E1_RUNS}{}",
        r#"{"kind":"metric","run":"r-1","name":"acc","value":71.3}
{"kind":"metric","run":"r-2","name":"acc","value":65.2}
"#
    );
    let (_dir, path) = built_from("This is 6.1% above the baseline [exp:E1].\n", &runs);
    assert!(mismatched(&path).is_empty());

    // Points against a metric on the 0..1 scale.
    let runs = format!(
        "{E1_RUNS}{}",
        r#"{"kind":"metric","run":"r-1","name":"acc","value":0.713}
"#
    );
    let (_dir, path) = built_from("Accuracy reaches 71.3 points [run:r-1].\n", &runs);
    assert!(mismatched(&path).is_empty());
}

#[test]
fn a_plain_number_is_not_matched_to_a_scaled_difference() {
    let runs = format!(
        "{E1_RUNS}{}",
        r#"{"kind":"metric","run":"r-1","name":"tool_calls","value":4.21}
{"kind":"metric","run":"r-2","name":"tool_calls","value":4.15}
"#
    );
    let (_dir, path) = built_from("Tool calls stay below 6.0 [exp:E1].\n", &runs);
    assert_eq!(mismatched(&path), [pair("C1", "6.0")]);
}

#[test]
fn matching_stays_within_the_written_precision() {
    let runs = format!(
        "{E1_RUNS}{}",
        r#"{"kind":"metric","run":"r-1","name":"error","value":0.0}
"#
    );
    let (_dir, path) = built_from("Error is 0.0000000001 [run:r-1].\n", &runs);
    assert_eq!(mismatched(&path), [pair("C1", "0.0000000001")]);
}

#[test]
fn equal_runs_support_a_zero_difference() {
    let runs = format!(
        "{E1_RUNS}{}",
        r#"{"kind":"metric","run":"r-1","name":"acc","value":0.5}
{"kind":"metric","run":"r-2","name":"acc","value":0.5}
"#
    );
    let (_dir, path) = built_from("The gap is 0.00 [exp:E1].\n", &runs);
    assert!(mismatched(&path).is_empty());
}

#[test]
fn a_broken_reference_on_a_supported_claim_is_a_finding() {
    let (_dir, path) = built(&MESSY);
    let audit = answers::audit(&path).unwrap();
    assert_eq!(
        audit.broken,
        [Link {
            claim: "C6".into(),
            kind: "source".into(),
            target: "ndiaye2022".into(),
            supports: false,
            reason: "not listed in References".into(),
        }]
    );
    assert_eq!(audit.findings(), 5);

    let draft = "Our planner reaches 99.9% task success [run:r-004] [@lee2025].\n\n## References\n\n- [@lee2025] Lee. A paper. 2025.\n";
    let (_dir, path) = built_from(draft, CLEAN.runs);
    let audit = answers::audit(&path).unwrap();
    assert!(audit.report.unsupported.is_empty());
    assert_eq!(audit.broken.len(), 1);
    assert_eq!(audit.broken[0].reason, "run status is failed");
}

#[test]
fn explain_shows_a_mismatched_figure() {
    let (_dir, path) = built(&MESSY);
    let c1 = answers::explain(&path, "C1").unwrap();
    assert_eq!(ids(&c1.mismatches, |m| &m.figure), ["74.2%"]);
}

#[test]
fn derivations_carry_their_operands_in_the_dump() {
    let (_dir, path) = built(&CLEAN);
    let dump = answers::dump(&path).unwrap();
    assert!(
        dump.iter().any(|line| line
            == "(:Derivation {minuend: 'r-002 success_rate', op: 'difference', subtrahend: 'r-001 success_rate'})"),
        "{dump:#?}"
    );
}

#[test]
fn replace_leaves_no_temporary_files() {
    let (dir, path) = built(&CLEAN);
    ingest(&MESSY, &path, true).unwrap();
    let names: Vec<String> = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, ["claims.rl"]);
}

#[test]
fn the_roundoff_allowance_stays_below_the_written_precision() {
    // Direct match: 1000000000000.0 is written to 0.05, so 1000000000001.0 must not match.
    let runs = format!(
        "{E1_RUNS}{}",
        r#"{"kind":"metric","run":"r-1","name":"count","value":1000000000001.0}
"#
    );
    let (_dir, path) = built_from("We reached 1000000000000.0 steps [run:r-1].\n", &runs);
    assert_eq!(mismatched(&path), [pair("C1", "1000000000000.0")]);

    // Difference: 3000000000001.0 - 2000000000000.0 is 1000000000001.0, not the figure.
    let runs = format!(
        "{E1_RUNS}{}",
        r#"{"kind":"metric","run":"r-1","name":"count","value":3000000000001.0}
{"kind":"metric","run":"r-2","name":"count","value":2000000000000.0}
"#
    );
    let (_dir, path) = built_from("The gap is 1000000000000.0 steps [exp:E1].\n", &runs);
    assert_eq!(mismatched(&path), [pair("C1", "1000000000000.0")]);

    // Direct match with large operands: 1000000000000000.0 is written to 0.05, and the
    // operand-relative allowance alone (about 2.0 here) would accept 1000000000000001.0,
    // so only the cap at a millionth of the written tolerance rejects it.
    let runs = format!(
        "{E1_RUNS}{}",
        r#"{"kind":"metric","run":"r-1","name":"count","value":1000000000000001.0}
"#
    );
    let (_dir, path) = built_from("We reached 1000000000000000.0 steps [run:r-1].\n", &runs);
    assert_eq!(mismatched(&path), [pair("C1", "1000000000000000.0")]);

    // Large operands: the difference is exactly 2.0, and 1.0 is written to 0.05, so the
    // allowance may not grow with the operands past the written precision.
    for (minuend, figure, expected) in [
        ("1000000000000002.0", "1.0", &[pair("C1", "1.0")][..]),
        ("1000000000000001.0", "1.0", &[]),
    ] {
        let runs = format!(
            "{E1_RUNS}{{\"kind\":\"metric\",\"run\":\"r-1\",\"name\":\"count\",\"value\":{minuend}}}\n\
             {{\"kind\":\"metric\",\"run\":\"r-2\",\"name\":\"count\",\"value\":1000000000000000.0}}\n"
        );
        let (_dir, path) = built_from(&format!("The gap is {figure} steps [exp:E1].\n"), &runs);
        assert_eq!(mismatched(&path), expected, "{minuend}");
    }
}

/// The matching rule at its edges: the roundoff allowance is 1e-15 of each operand's
/// magnitude, summed term by term so that it cannot overflow, and capped at a millionth of the
/// written tolerance.
#[test]
fn the_roundoff_allowance_holds_at_its_edges() {
    for (figure, metric, matches) in [
        // Exactly half a unit away matches; any visible distance beyond does not.
        ("0.30", "0.305", true),
        ("0.30", "0.3050001", false),
        // Near the largest float: the operand sum must not overflow into accepting anything.
        ("9e307", "9.5e307", true),
        ("9e307", "9.5000001e307", false),
        ("-9e307", "-9.5000001e307", false),
        // Tiny magnitudes: no absolute allowance, so a tiny tolerance stays tiny.
        ("1.0e-300", "1.04e-300", true),
        ("1.0e-300", "1.06e-300", false),
    ] {
        let runs = format!(
            "{E1_RUNS}{{\"kind\":\"metric\",\"run\":\"r-1\",\"name\":\"x\",\"value\":{metric}}}\n"
        );
        let (_dir, path) = built_from(&format!("We reached {figure} loss [run:r-1].\n"), &runs);
        let expected: &[(String, String)] = if matches {
            &[]
        } else {
            &[pair("C1", figure)][..]
        };
        assert_eq!(mismatched(&path), expected, "{figure} vs {metric}");
    }

    // A difference with a zero baseline near the largest float.
    for (minuend, figure, derived) in [
        ("9.5e307", "9e307", true),
        ("9.5000001e307", "9e307", false),
    ] {
        let runs = format!(
            "{E1_RUNS}{{\"kind\":\"metric\",\"run\":\"r-1\",\"name\":\"x\",\"value\":{minuend}}}\n\
             {{\"kind\":\"metric\",\"run\":\"r-2\",\"name\":\"x\",\"value\":0.0}}\n"
        );
        let (_dir, path) = built_from(&format!("The gap is {figure} [exp:E1].\n"), &runs);
        let expected: &[(String, String)] = if derived {
            &[]
        } else {
            &[pair("C1", figure)][..]
        };
        assert_eq!(mismatched(&path), expected, "{minuend} - 0 vs {figure}");
    }
}

/// `audit` reads every finding through one opened store. That is safe only if an opened
/// store keeps reading one version of the graph while a replacement is published by
/// rename, and a second open would see the new version; this pins both.
#[test]
fn one_opened_store_reads_one_version_across_a_replacement() {
    let claims = "MATCH (c:Claim) RETURN count(c)";
    let (_dir, path) = built(&CLEAN);
    let clean_claims = count(&path, claims);
    let db = Database::open_read_only(path.to_str().unwrap()).unwrap();
    let mut conn = db.connect().unwrap();
    ingest(&MESSY, &path, true).unwrap();
    let messy_claims = count(&path, claims);
    assert_ne!(clean_claims, messy_claims);
    let same_handle = conn.query(claims).unwrap().rows[0][0].as_int().unwrap();
    assert_eq!(same_handle, clean_claims);
}
