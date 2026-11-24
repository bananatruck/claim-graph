//! Writes a parsed draft and run log into a single-file Relate store, and queries it.
//!
//! See SPEC.md, sections "Graph model" and "Queries".

use std::fmt;
use std::path::{Path, PathBuf};

use crate::draft::Draft;
use crate::runlog::RunLog;

/// What `ingest` wrote.
#[derive(Debug, Clone, PartialEq)]
pub struct IngestSummary {
    pub claims: usize,
    pub sources: usize,
    pub experiments: usize,
    pub runs: usize,
    pub metrics: usize,
    /// `(claim id, reference)` for each reference that points at nothing,
    /// written as `@key`, `run:ID` or `exp:ID`.
    pub unresolved: Vec<(String, String)>,
}

/// A claim with no path to a completed run or a listed source.
#[derive(Debug, Clone, PartialEq)]
pub struct UnsupportedClaim {
    pub id: String,
    pub section: String,
    pub text: String,
    pub unresolved: Vec<String>,
}

/// The answer to "which claims are unsupported", read from the store.
#[derive(Debug, Clone, PartialEq)]
pub struct Report {
    pub total_claims: usize,
    pub unsupported: Vec<UnsupportedClaim>,
}

#[derive(Debug)]
pub enum StoreError {
    /// The store path does not end in `.rl`.
    NotSingleFile(PathBuf),
    /// The store already exists and `replace` was not set.
    Exists(PathBuf),
    /// The path is not valid UTF-8, which Relate's open call needs.
    NotUtf8(PathBuf),
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
            StoreError::NotUtf8(p) => write!(f, "store path is not valid UTF-8: {}", p.display()),
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

/// The unsupported-claims query from SPEC.md, verbatim.
pub const UNSUPPORTED_QUERY: &str = "\
MATCH (c:Claim)
WHERE NOT EXISTS {
  MATCH (c)-[:CITES|SUPPORTED_BY|HAS_RUN*1..2]->(t)
  WHERE t:Source OR (t:Run AND t.status = 'completed')
}
RETURN c.id AS id, c.section AS section, c.text AS text, c.unresolved AS unresolved
ORDER BY c.ordinal";

/// Each write statement and the parameter it reads its rows from.
const WRITES: &[(&str, &str)] = &[
    ("draft", "CREATE (:Draft {path: $draft.path, title: $draft.title})"),
    (
        "sources",
        "MATCH (d:Draft) UNWIND $sources AS s \
         CREATE (d)-[:LISTS]->(:Source {key: s.key, entry: s.entry})",
    ),
    (
        "experiments",
        "UNWIND $experiments AS e CREATE (:Experiment {id: e.id, name: e.name})",
    ),
    (
        "runs",
        "UNWIND $runs AS r MATCH (e:Experiment {id: r.experiment}) \
         CREATE (e)-[:HAS_RUN]->(:Run {id: r.id, seed: r.seed, status: r.status})",
    ),
    (
        "metrics",
        "UNWIND $metrics AS m MATCH (r:Run {id: m.run}) \
         CREATE (r)-[:PRODUCED]->(:Metric {run: m.run, name: m.name, value: m.value})",
    ),
    (
        "claims",
        "MATCH (d:Draft) UNWIND $claims AS c \
         CREATE (d)-[:HAS_CLAIM]->(:Claim {id: c.id, ordinal: c.ordinal, section: c.section, \
         text: c.text, unresolved: c.unresolved})",
    ),
    (
        "cites",
        "UNWIND $cites AS x MATCH (c:Claim {id: x.claim}), (s:Source {key: x.key}) \
         CREATE (c)-[:CITES]->(s)",
    ),
    (
        "run_links",
        "UNWIND $run_links AS x MATCH (c:Claim {id: x.claim}), (r:Run {id: x.id}) \
         CREATE (c)-[:SUPPORTED_BY]->(r)",
    ),
    (
        "experiment_links",
        "UNWIND $experiment_links AS x MATCH (c:Claim {id: x.claim}), (e:Experiment {id: x.id}) \
         CREATE (c)-[:SUPPORTED_BY]->(e)",
    ),
];

/// Build the graph in a new store at `path`, in one transaction, then checkpoint and close it.
pub fn ingest(
    path: &Path,
    draft_path: &str,
    draft: &Draft,
    log: &RunLog,
    replace: bool,
) -> Result<IngestSummary, StoreError> {
    let uri = store_uri(path)?;
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
        std::fs::remove_file(path)?;
    }

    let rows = Rows::new(draft_path, draft, log);
    let db = relate::Database::open(uri)?;
    let mut conn = db.connect()?;
    conn.begin()?;
    for (name, statement) in WRITES {
        let params = rows.params(name)?;
        if let Err(e) = conn.query_with_params(statement, params) {
            conn.rollback()?;
            return Err(e.into());
        }
    }
    conn.commit()?;
    drop(conn);
    db.checkpoint()?;
    drop(db);

    Ok(IngestSummary {
        claims: draft.claims.len(),
        sources: draft.sources.len(),
        experiments: log.experiments.len(),
        runs: log.runs.len(),
        metrics: log.metrics.len(),
        unresolved: rows.unresolved,
    })
}

/// Open the store read-only and list the claims with no supporting path.
pub fn unsupported(path: &Path) -> Result<Report, StoreError> {
    let uri = store_uri(path)?;
    if !path.is_file() {
        return Err(StoreError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("no store at {}", path.display()),
        )));
    }
    let db = relate::Database::open_read_only(uri)?;
    let mut conn = db.connect()?;

    let total = conn.query("MATCH (c:Claim) RETURN count(c) AS n")?;
    let total_claims = total
        .rows
        .first()
        .and_then(|row| row.first())
        .and_then(relate::Value::as_int)
        .ok_or_else(|| StoreError::Unexpected("claim count".into()))?;

    let result = conn.query(UNSUPPORTED_QUERY)?;
    let mut unsupported = Vec::with_capacity(result.rows.len());
    for row in &result.rows {
        let [id, section, text, unresolved] = row.as_slice() else {
            return Err(StoreError::Unexpected(format!(
                "expected 4 columns, got {}",
                row.len()
            )));
        };
        unsupported.push(UnsupportedClaim {
            id: string(id, "id")?,
            section: string(section, "section")?,
            text: string(text, "text")?,
            unresolved: string_list(unresolved)?,
        });
    }
    Ok(Report {
        total_claims: usize::try_from(total_claims)
            .map_err(|_| StoreError::Unexpected(format!("claim count {total_claims}")))?,
        unsupported,
    })
}

fn store_uri(path: &Path) -> Result<&str, StoreError> {
    if path.extension().is_none_or(|ext| ext != "rl") {
        return Err(StoreError::NotSingleFile(path.to_path_buf()));
    }
    path.to_str()
        .ok_or_else(|| StoreError::NotUtf8(path.to_path_buf()))
}

fn string(value: &relate::Value, column: &str) -> Result<String, StoreError> {
    value
        .as_string()
        .map(str::to_string)
        .ok_or_else(|| StoreError::Unexpected(format!("column {column} is not a string")))
}

fn string_list(value: &relate::Value) -> Result<Vec<String>, StoreError> {
    match value {
        relate::Value::Null => Ok(Vec::new()),
        relate::Value::List(items) => items
            .iter()
            .map(|item| string(item, "unresolved"))
            .collect(),
        _ => Err(StoreError::Unexpected(
            "column unresolved is not a list".into(),
        )),
    }
}

/// The parameter rows for every write, plus the references that resolve to nothing.
struct Rows {
    params: serde_json::Value,
    unresolved: Vec<(String, String)>,
}

impl Rows {
    fn new(draft_path: &str, draft: &Draft, log: &RunLog) -> Self {
        use serde_json::json;
        use std::collections::HashSet;

        let source_keys: HashSet<&str> = draft.sources.iter().map(|s| s.key.as_str()).collect();
        let run_ids: HashSet<&str> = log.runs.iter().map(|r| r.id.as_str()).collect();
        let experiment_ids: HashSet<&str> =
            log.experiments.iter().map(|e| e.id.as_str()).collect();

        let mut unresolved = Vec::new();
        let mut claims = Vec::new();
        let mut cites = Vec::new();
        let mut run_links = Vec::new();
        let mut experiment_links = Vec::new();
        for claim in &draft.claims {
            let mut missing = Vec::new();
            for key in &claim.citations {
                if source_keys.contains(key.as_str()) {
                    cites.push(json!({"claim": claim.id, "key": key}));
                } else {
                    missing.push(format!("@{key}"));
                }
            }
            for id in &claim.runs {
                if run_ids.contains(id.as_str()) {
                    run_links.push(json!({"claim": claim.id, "id": id}));
                } else {
                    missing.push(format!("run:{id}"));
                }
            }
            for id in &claim.experiments {
                if experiment_ids.contains(id.as_str()) {
                    experiment_links.push(json!({"claim": claim.id, "id": id}));
                } else {
                    missing.push(format!("exp:{id}"));
                }
            }
            unresolved.extend(missing.iter().map(|m| (claim.id.clone(), m.clone())));
            claims.push(json!({
                "id": claim.id,
                "ordinal": claim.ordinal,
                "section": claim.section,
                "text": claim.text,
                "unresolved": missing,
            }));
        }

        let params = json!({
            "draft": {"path": draft_path, "title": draft.title},
            "sources": draft.sources.iter()
                .map(|s| json!({"key": s.key, "entry": s.entry})).collect::<Vec<_>>(),
            "experiments": log.experiments.iter()
                .map(|e| json!({"id": e.id, "name": e.name})).collect::<Vec<_>>(),
            "runs": log.runs.iter()
                .map(|r| json!({"id": r.id, "experiment": r.experiment, "seed": r.seed, "status": r.status}))
                .collect::<Vec<_>>(),
            "metrics": log.metrics.iter()
                .map(|m| json!({"run": m.run, "name": m.name, "value": m.value})).collect::<Vec<_>>(),
            "claims": claims,
            "cites": cites,
            "run_links": run_links,
            "experiment_links": experiment_links,
        });
        Rows { params, unresolved }
    }

    /// Parameters for one write: only the named row set, bound under its own name.
    fn params(
        &self,
        name: &str,
    ) -> Result<std::collections::BTreeMap<String, relate::Value>, StoreError> {
        let value = serde_json::json!({ name: self.params[name] });
        relate::params_from_value(&value).map_err(StoreError::Unexpected)
    }
}
