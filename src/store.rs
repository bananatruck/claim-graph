//! Writes a parsed draft and run log into a single-file Relate store.
//!
//! See SPEC.md, section "Graph model". The questions asked of the store are in `answers`.

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

use serde_json::{Value as Json, json};

use crate::draft::Draft;
use crate::queries;
use crate::runlog::RunLog;

/// What `ingest` wrote, counted from the graph after the commit.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct IngestSummary {
    pub claims: i64,
    pub prose: i64,
    pub sources: i64,
    pub experiments: i64,
    pub runs: i64,
    pub metrics: i64,
    pub figures: i64,
    pub matches: i64,
    pub derivations: i64,
    /// `(claim id, reference)` for each reference that points at nothing,
    /// written as `@key`, `run:ID` or `exp:ID`, in reading order.
    pub unresolved: Vec<(String, String)>,
}

#[derive(Debug)]
pub enum StoreError {
    /// The store path does not end in `.rl`.
    NotSingleFile(PathBuf),
    /// The store already exists and `replace` was not set.
    Exists(PathBuf),
    /// There is no store at the path.
    Missing(PathBuf),
    /// The path is not valid UTF-8, which Relate's open call needs.
    NotUtf8(PathBuf),
    /// No sentence has the requested ID.
    UnknownSentence(String),
    Io(std::io::Error),
    Relate(relate::Error),
    /// The store answered with a shape this program does not expect.
    Unexpected(String),
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StoreError::NotSingleFile(p) => {
                write!(f, "store path must end in .rl: {}", p.display())
            }
            StoreError::Exists(p) => write!(
                f,
                "store already exists: {} (pass --replace to rebuild it)",
                p.display()
            ),
            StoreError::Missing(p) => write!(f, "no store at {}", p.display()),
            StoreError::NotUtf8(p) => write!(f, "store path is not valid UTF-8: {}", p.display()),
            StoreError::UnknownSentence(id) => write!(f, "no sentence with ID {id} in the store"),
            StoreError::Io(e) => write!(f, "{e}"),
            StoreError::Relate(e) => write!(f, "relate: {e}"),
            StoreError::Unexpected(m) => write!(f, "unexpected result from the store: {m}"),
        }
    }
}

impl std::error::Error for StoreError {}

impl From<relate::Error> for StoreError {
    fn from(e: relate::Error) -> Self {
        StoreError::Relate(e)
    }
}

impl From<std::io::Error> for StoreError {
    fn from(e: std::io::Error) -> Self {
        StoreError::Io(e)
    }
}

/// Build the graph in a new store at `path`, in one transaction, then checkpoint and close it.
///
/// The store is built in a temporary file beside `path` and moved into place only when it
/// is complete, so a failed ingest with `replace` leaves the previous store untouched.
pub fn ingest(
    path: &Path,
    draft_path: &str,
    draft: &Draft,
    log: &RunLog,
    replace: bool,
) -> Result<IngestSummary, StoreError> {
    store_uri(path)?;
    if path.exists() {
        if !replace {
            return Err(StoreError::Exists(path.to_path_buf()));
        }
        if !path.is_file() {
            return Err(StoreError::Unexpected(format!(
                "{} exists and is not a file; refusing to replace it",
                path.display()
            )));
        }
    }
    let temporary = temporary_path(path);
    let built = build(&temporary, draft_path, draft, log);
    let result = built.and_then(|summary| {
        std::fs::rename(&temporary, path)?;
        Ok(summary)
    });
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

/// A path in the same directory as `path`, ending in `.rl`, that no other ingest uses.
fn temporary_path(path: &Path) -> PathBuf {
    let stem = path
        .file_stem()
        .map_or_else(|| "store".into(), |s| s.to_string_lossy().into_owned());
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.subsec_nanos());
    path.with_file_name(format!(".{stem}.ingest-{}-{nanos}.rl", std::process::id()))
}

fn build(
    path: &Path,
    draft_path: &str,
    draft: &Draft,
    log: &RunLog,
) -> Result<IngestSummary, StoreError> {
    let uri = store_uri(path)?;
    if path.exists() {
        return Err(StoreError::Exists(path.to_path_buf()));
    }
    let rows = rows(draft_path, draft, log);
    let db = relate::Database::open(uri)?;
    let mut conn = db.connect()?;
    conn.begin()?;
    for (statement, param) in queries::WRITES {
        let params = match param {
            Some(name) => params(&json!({ *name: rows[*name] }))?,
            None => BTreeMap::new(),
        };
        if let Err(e) = conn.query_with_params(statement, params) {
            conn.rollback()?;
            return Err(e.into());
        }
    }
    conn.commit()?;

    let counts = conn.query(queries::COUNTS)?;
    let row = counts
        .rows
        .first()
        .ok_or_else(|| StoreError::Unexpected("counts".into()))?;
    let count = |i: usize| -> Result<i64, StoreError> {
        row.get(i)
            .and_then(relate::Value::as_int)
            .ok_or_else(|| StoreError::Unexpected(format!("count column {i}")))
    };
    let mut summary = IngestSummary {
        claims: count(0)?,
        prose: count(1)?,
        sources: count(2)?,
        experiments: count(3)?,
        runs: count(4)?,
        metrics: count(5)?,
        figures: count(6)?,
        matches: count(7)?,
        derivations: count(8)?,
        unresolved: Vec::new(),
    };
    summary.unresolved = unresolved(&mut conn)?
        .into_iter()
        .map(|(_, claim, reference)| (claim, reference))
        .collect();
    drop(conn);
    db.checkpoint()?;
    drop(db);
    Ok(summary)
}

/// `(ordinal, claim id, reference)` for every dangling reference, in reading order.
pub(crate) fn unresolved(
    conn: &mut relate::Connection,
) -> Result<Vec<(i64, String, String)>, StoreError> {
    let result = conn.query(queries::UNRESOLVED)?;
    let mut out = Vec::new();
    for row in &result.rows {
        out.push((
            int(&row[0], "ordinal")?,
            string(&row[1], "claim")?,
            string(&row[2], "ref")?,
        ));
    }
    out.sort();
    Ok(out)
}

/// Open an existing store read-only.
pub fn open_read_only(path: &Path) -> Result<relate::Database, StoreError> {
    let uri = store_uri(path)?;
    if !path.is_file() {
        return Err(StoreError::Missing(path.to_path_buf()));
    }
    Ok(relate::Database::open_read_only(uri)?)
}

fn store_uri(path: &Path) -> Result<&str, StoreError> {
    if path.extension().is_none_or(|ext| ext != "rl") {
        return Err(StoreError::NotSingleFile(path.to_path_buf()));
    }
    path.to_str()
        .ok_or_else(|| StoreError::NotUtf8(path.to_path_buf()))
}

pub(crate) fn params(value: &Json) -> Result<BTreeMap<String, relate::Value>, StoreError> {
    relate::params_from_value(value).map_err(StoreError::Unexpected)
}

pub(crate) fn string(value: &relate::Value, column: &str) -> Result<String, StoreError> {
    value
        .as_string()
        .map(str::to_string)
        .ok_or_else(|| StoreError::Unexpected(format!("column {column} is not a string")))
}

pub(crate) fn int(value: &relate::Value, column: &str) -> Result<i64, StoreError> {
    value
        .as_int()
        .ok_or_else(|| StoreError::Unexpected(format!("column {column} is not an integer")))
}

pub(crate) fn float(value: &relate::Value, column: &str) -> Result<f64, StoreError> {
    match value {
        relate::Value::Float(f) => Ok(*f),
        relate::Value::Int(i) => Ok(*i as f64),
        _ => Err(StoreError::Unexpected(format!(
            "column {column} is not a number"
        ))),
    }
}

pub(crate) fn string_list(value: &relate::Value, column: &str) -> Result<Vec<String>, StoreError> {
    match value {
        relate::Value::Null => Ok(Vec::new()),
        relate::Value::List(items) => items.iter().map(|item| string(item, column)).collect(),
        _ => Err(StoreError::Unexpected(format!(
            "column {column} is not a list"
        ))),
    }
}

/// The parameter rows for every write. References are passed as written; whether they
/// resolve is decided by the graph, not here.
fn rows(draft_path: &str, draft: &Draft, log: &RunLog) -> Json {
    let mut claims = Vec::new();
    let mut prose = Vec::new();
    let mut cites = Vec::new();
    let mut run_links = Vec::new();
    let mut experiment_links = Vec::new();
    let mut figures = Vec::new();
    for sentence in &draft.sentences {
        let row = json!({
            "id": sentence.id,
            "ordinal": sentence.ordinal,
            "section": sentence.section,
            "text": sentence.text,
            "reasons": sentence.reasons.iter().map(|r| r.as_str()).collect::<Vec<_>>(),
        });
        if !sentence.is_claim() {
            prose.push(row);
            continue;
        }
        claims.push(row);
        for key in dedup(&sentence.citations) {
            cites.push(json!({"claim": sentence.id, "key": key}));
        }
        for id in dedup(&sentence.runs) {
            run_links.push(json!({"claim": sentence.id, "id": id}));
        }
        for id in dedup(&sentence.experiments) {
            experiment_links.push(json!({"claim": sentence.id, "id": id}));
        }
        for (index, figure) in sentence.figures.iter().enumerate() {
            // Only percentages and numbers written with decimals are treated as
            // measurements; bare integers are usually counts ("3 seeds", "2024").
            let measured = figure.percent || figure.as_written.contains('.');
            figures.push(json!({
                "claim": sentence.id,
                "index": index,
                "as_written": figure.as_written,
                "value": figure.value,
                "tolerance": figure.tolerance,
                "raw": figure.raw,
                "raw_tolerance": figure.raw_tolerance,
                "percent": figure.percent,
                "measured": measured,
            }));
        }
    }

    json!({
        "draft": {"path": draft_path, "title": draft.title},
        "sources": draft.sources.iter()
            .map(|s| json!({"key": s.key, "entry": s.entry})).collect::<Vec<_>>(),
        "experiments": log.experiments.iter()
            .map(|e| json!({"id": e.id, "name": e.name})).collect::<Vec<_>>(),
        "supersedes": log.experiments.iter()
            .filter_map(|e| e.supersedes.as_ref().map(|older| json!({"newer": e.id, "older": older})))
            .collect::<Vec<_>>(),
        "runs": log.runs.iter()
            .map(|r| json!({"id": r.id, "experiment": r.experiment, "seed": r.seed, "status": r.status}))
            .collect::<Vec<_>>(),
        "metrics": log.metrics.iter()
            .map(|m| json!({"run": m.run, "name": m.name, "value": m.value})).collect::<Vec<_>>(),
        "claims": claims,
        "prose": prose,
        "cites": cites,
        "run_links": run_links,
        "experiment_links": experiment_links,
        "figures": figures,
    })
}

fn dedup(items: &[String]) -> Vec<&String> {
    let mut seen = Vec::new();
    for item in items {
        if !seen.contains(&item) {
            seen.push(item);
        }
    }
    seen
}
