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
    let link = |kind: &str, target: &str, supports: bool, reason: &str| Link {
        kind: kind.into(),
        target: target.into(),
        supports,
        reason: reason.into(),
    };

    let c4 = answers::explain(&path, "C4").unwrap();
    assert!(c4.is_claim);
    assert_eq!(
        c4.links,
        [link("experiment", "E2", false, "experiment has no runs")]
    );
    assert!(c4.support_paths.is_empty());

    let c6 = answers::explain(&path, "C6").unwrap();
    assert_eq!(
        c6.links,
        [link("run", "r-004", false, "run status is failed")]
    );

    let c7 = answers::explain(&path, "C7").unwrap();
    assert_eq!(
        c7.links,
        [link(
            "source",
            "okafor2023",
            false,
            "not listed in References"
        )]
    );

    let c2 = answers::explain(&path, "C2").unwrap();
    assert_eq!(
        c2.links,
        [link("experiment", "E1", true, "3 of its 3 runs completed")]
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
    assert_eq!(audit.findings(), 4);
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
    ingest(&CLEAN, &path, true).unwrap();
    assert_eq!(count(&path, "MATCH (c:Claim) RETURN count(c)"), 7);
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
