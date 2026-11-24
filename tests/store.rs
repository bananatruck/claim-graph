//! Store tests on real `.rl` files: every answer is read back from a reopened Relate store.

use std::path::{Path, PathBuf};

use claim_graph::draft::parse_draft;
use claim_graph::runlog::parse_run_log;
use claim_graph::store::{self, StoreError};
use relate::Database;

const DRAFT: &str = include_str!("../data/synthetic/draft.md");
const RUNS: &str = include_str!("../data/synthetic/runs.jsonl");

fn ingest_synthetic(path: &Path, replace: bool) -> Result<store::IngestSummary, StoreError> {
    let draft = parse_draft(DRAFT).unwrap();
    let log = parse_run_log(RUNS).unwrap();
    store::ingest(path, "data/synthetic/draft.md", &draft, &log, replace)
}

fn temp_store() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("claims.rl");
    (dir, path)
}

fn count(path: &Path, cypher: &str) -> i64 {
    let db = Database::open_read_only(path.to_str().unwrap()).unwrap();
    let mut conn = db.connect().unwrap();
    let result = conn.query(cypher).unwrap();
    result.rows[0][0].as_int().unwrap()
}

#[test]
fn ingest_writes_one_file_and_reports_unresolved_references() {
    let (_dir, path) = temp_store();
    let summary = ingest_synthetic(&path, false).unwrap();
    assert!(path.is_file(), "the store is a single file");
    assert_eq!(summary.claims, 7);
    assert_eq!(
        summary.unresolved,
        [("C7".to_string(), "@okafor2023".to_string())]
    );
}

#[test]
fn the_reopened_store_holds_the_whole_graph() {
    let (_dir, path) = temp_store();
    ingest_synthetic(&path, false).unwrap();
    let nodes = [
        ("Draft", 1),
        ("Claim", 7),
        ("Source", 2),
        ("Experiment", 3),
        ("Run", 3),
        ("Metric", 2),
    ];
    for (label, expected) in nodes {
        let n = count(&path, &format!("MATCH (n:{label}) RETURN count(n)"));
        assert_eq!(n, expected, "nodes labelled {label}");
    }
    let rels = [
        ("HAS_CLAIM", 7),
        ("LISTS", 2),
        ("CITES", 1),
        ("SUPPORTED_BY", 4),
        ("HAS_RUN", 3),
        ("PRODUCED", 2),
    ];
    for (rel_type, expected) in rels {
        let n = count(&path, &format!("MATCH ()-[r:{rel_type}]->() RETURN count(r)"));
        assert_eq!(n, expected, "relationships of type {rel_type}");
    }
}

#[test]
fn unsupported_claims_come_from_the_reopened_store() {
    let (_dir, path) = temp_store();
    ingest_synthetic(&path, false).unwrap();
    let report = store::unsupported(&path).unwrap();
    assert_eq!(report.total_claims, 7);
    let ids: Vec<&str> = report.unsupported.iter().map(|c| c.id.as_str()).collect();
    assert_eq!(ids, ["C3", "C4", "C6", "C7"]);
    let c7 = &report.unsupported[3];
    assert_eq!(c7.section, "Results");
    assert_eq!(c7.unresolved, ["@okafor2023"]);
    assert!(c7.text.starts_with("The gains match"));
    assert!(report.unsupported[0].unresolved.is_empty());
}

#[test]
fn the_report_follows_edits_made_to_the_graph() {
    let (_dir, path) = temp_store();
    ingest_synthetic(&path, false).unwrap();
    {
        // Give C3 a citation directly in the store, outside Claim Graph.
        let db = Database::open(path.to_str().unwrap()).unwrap();
        let mut conn = db.connect().unwrap();
        conn.query(
            "MATCH (c:Claim {id: 'C3'}), (s:Source {key: 'chen2024'}) CREATE (c)-[:CITES]->(s)",
        )
        .unwrap();
    }
    let report = store::unsupported(&path).unwrap();
    let ids: Vec<&str> = report.unsupported.iter().map(|c| c.id.as_str()).collect();
    assert_eq!(ids, ["C4", "C6", "C7"]);
}

#[test]
fn ingest_refuses_an_existing_store_unless_replace_is_given() {
    let (_dir, path) = temp_store();
    ingest_synthetic(&path, false).unwrap();
    let err = ingest_synthetic(&path, false).unwrap_err();
    assert!(matches!(err, StoreError::Exists(_)), "got {err}");
    ingest_synthetic(&path, true).unwrap();
    assert_eq!(count(&path, "MATCH (c:Claim) RETURN count(c)"), 7);
}

#[test]
fn the_store_path_must_end_in_rl() {
    let dir = tempfile::tempdir().unwrap();
    let err = ingest_synthetic(&dir.path().join("claims"), false).unwrap_err();
    assert!(matches!(err, StoreError::NotSingleFile(_)), "got {err}");
}

#[test]
fn reporting_on_a_missing_store_is_an_error_and_creates_nothing() {
    let (_dir, path) = temp_store();
    assert!(store::unsupported(&path).is_err());
    assert!(!path.exists());
}
