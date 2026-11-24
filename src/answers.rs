//! Questions asked of a reopened store. Every answer comes from a Cypher query in
//! `queries`; nothing here is computed from the input files.

use std::path::Path;

use serde::Serialize;
use serde_json::json;

use crate::queries;
use crate::store::{
    StoreError, float, int, open_read_only, params, string, string_list, unresolved,
};

/// A sentence, as listed in reports.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Listed {
    pub id: String,
    pub section: String,
    pub text: String,
    /// References in this sentence that point at nothing.
    pub unresolved: Vec<String>,
}

/// The answer to "which claims are unsupported".
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Report {
    pub total_claims: i64,
    pub unsupported: Vec<Listed>,
    /// Sentences the claim rule did not check.
    pub prose: Vec<Listed>,
}

pub fn unsupported(path: &Path) -> Result<Report, StoreError> {
    let db = open_read_only(path)?;
    let mut conn = db.connect()?;
    let total = conn.query(queries::CLAIM_TOTAL)?;
    let total_claims = int(first(&total.rows, "claim total")?, "n")?;
    let dangling = unresolved(&mut conn)?;
    let listed = |rows: &[Vec<relate::Value>]| -> Result<Vec<Listed>, StoreError> {
        rows.iter()
            .map(|row| {
                let id = string(&row[0], "id")?;
                Ok(Listed {
                    unresolved: dangling
                        .iter()
                        .filter(|(_, claim, _)| *claim == id)
                        .map(|(_, _, r)| r.clone())
                        .collect(),
                    id,
                    section: string(&row[1], "section")?,
                    text: string(&row[2], "text")?,
                })
            })
            .collect()
    };
    let unsupported = listed(&conn.query(queries::UNSUPPORTED)?.rows)?;
    let prose = listed(&conn.query(queries::PROSE)?.rows)?;
    Ok(Report {
        total_claims,
        unsupported,
        prose,
    })
}

/// One reference a claim makes, and whether it supports the claim.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Link {
    pub kind: String,
    pub target: String,
    pub supports: bool,
    pub reason: String,
}

/// A claim that rests on an experiment a later one supersedes.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Stale {
    pub claim: String,
    /// The run or experiment the claim cites.
    pub via: String,
    /// From the newest experiment down to the one the claim rests on.
    pub chain: Vec<String>,
    /// What the newest experiment's completed runs measured.
    pub newest_results: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Explanation {
    pub id: String,
    pub section: String,
    pub text: String,
    pub is_claim: bool,
    pub reasons: Vec<String>,
    pub links: Vec<Link>,
    /// Each supporting path as the IDs along it, starting with the claim.
    pub support_paths: Vec<Vec<String>>,
    pub stale: Vec<Stale>,
    pub figures: Vec<FigureTrace>,
}

pub fn explain(path: &Path, id: &str) -> Result<Explanation, StoreError> {
    let db = open_read_only(path)?;
    let mut conn = db.connect()?;
    let by_id = params(&json!({"id": id}))?;
    let sentence = conn.query_with_params(queries::SENTENCE, by_id.clone())?;
    let row = sentence
        .rows
        .first()
        .ok_or_else(|| StoreError::UnknownSentence(id.to_string()))?;
    let mut explanation = Explanation {
        id: string(&row[0], "id")?,
        section: string(&row[1], "section")?,
        text: string(&row[2], "text")?,
        reasons: string_list(&row[3], "reasons")?,
        is_claim: int(&row[4], "is_claim")? > 0,
        links: Vec::new(),
        support_paths: Vec::new(),
        stale: Vec::new(),
        figures: Vec::new(),
    };
    if !explanation.is_claim {
        return Ok(explanation);
    }

    for row in &conn.query_with_params(queries::LINKS, by_id.clone())?.rows {
        explanation.links.push(link(row)?);
    }
    for row in &conn
        .query_with_params(queries::SUPPORT_PATHS, by_id.clone())?
        .rows
    {
        explanation
            .support_paths
            .push(string_list(&row[0], "path")?);
    }
    explanation.stale = stale(&mut conn, Some(id))?;
    explanation.figures = traces(&mut conn, None, Some(id))?;
    Ok(explanation)
}

fn link(row: &[relate::Value]) -> Result<Link, StoreError> {
    let kind = string(&row[0], "kind")?;
    let target = string(&row[1], "target")?;
    let listed = int(&row[2], "listed")? > 0;
    let logged = row[3].as_bool().unwrap_or(true);
    let status = row[4].as_string().map(str::to_string);
    let runs = int(&row[5], "runs")?;
    let completed = int(&row[6], "completed_runs")?;
    let (supports, reason) = match kind.as_str() {
        "source" if listed => (true, "listed in References".to_string()),
        "source" => (false, "not listed in References".to_string()),
        _ if !logged => (false, "not in the run log".to_string()),
        "run" => match status.as_deref() {
            Some("completed") => (true, "run completed".to_string()),
            Some(other) => (false, format!("run status is {other}")),
            None => (false, "run has no status".to_string()),
        },
        _ if runs == 0 => (false, "experiment has no runs".to_string()),
        _ if completed == 0 => (false, format!("none of its {runs} runs completed")),
        _ => (true, format!("{completed} of its {runs} runs completed")),
    };
    Ok(Link {
        kind,
        target,
        supports,
        reason,
    })
}

fn stale(conn: &mut relate::Connection, id: Option<&str>) -> Result<Vec<Stale>, StoreError> {
    let result = conn.query_with_params(queries::STALE, params(&json!({"id": id}))?)?;
    result
        .rows
        .iter()
        .map(|row| {
            Ok(Stale {
                claim: string(&row[0], "claim")?,
                via: string(&row[1], "via")?,
                chain: string_list(&row[2], "chain")?,
                newest_results: string_list(&row[3], "newest_results")?,
            })
        })
        .collect()
}

/// A metric a figure matches, and the run and experiment behind it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MetricMatch {
    pub metric: String,
    pub value: f64,
    pub run: String,
    pub status: String,
    pub experiment: String,
    /// Whether the claim cites this run, directly or through its experiment.
    pub cited: bool,
    /// The same metric from the experiment's other runs.
    pub other_runs: Vec<String>,
}

/// A figure that equals the difference between two runs' values of one metric.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Difference {
    pub metric: String,
    pub minuend_run: String,
    pub minuend: f64,
    pub subtrahend_run: String,
    pub subtrahend: f64,
    pub experiment: String,
    pub cited: bool,
}

/// Where one figure in one claim came from.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FigureTrace {
    pub claim: String,
    pub figure: String,
    /// False for bare integers, which are treated as counts and never matched.
    pub measured: bool,
    pub matches: Vec<MetricMatch>,
    pub differences: Vec<Difference>,
}

pub fn trace(
    path: &Path,
    figure: Option<&str>,
    claim: Option<&str>,
) -> Result<Vec<FigureTrace>, StoreError> {
    let db = open_read_only(path)?;
    let mut conn = db.connect()?;
    traces(&mut conn, figure, claim)
}

fn traces(
    conn: &mut relate::Connection,
    figure: Option<&str>,
    claim: Option<&str>,
) -> Result<Vec<FigureTrace>, StoreError> {
    let found = conn.query_with_params(
        queries::FIGURES,
        params(&json!({"figure": figure, "id": claim}))?,
    )?;
    let mut out = Vec::new();
    for row in &found.rows {
        let claim = string(&row[0], "claim")?;
        let idx = int(&row[1], "idx")?;
        let key = params(&json!({"id": claim, "idx": idx}))?;
        let mut trace = FigureTrace {
            figure: string(&row[2], "figure")?,
            measured: row[3].as_bool().unwrap_or(false),
            claim,
            matches: Vec::new(),
            differences: Vec::new(),
        };
        for m in &conn
            .query_with_params(queries::FIGURE_MATCHES, key.clone())?
            .rows
        {
            trace.matches.push(MetricMatch {
                metric: string(&m[0], "metric")?,
                value: float(&m[1], "value")?,
                run: string(&m[2], "run")?,
                status: string(&m[3], "status")?,
                experiment: string(&m[4], "experiment")?,
                cited: int(&m[5], "cited")? > 0,
                other_runs: string_list(&m[6], "other_runs")?,
            });
        }
        for d in &conn
            .query_with_params(queries::FIGURE_DERIVATIONS, key)?
            .rows
        {
            trace.differences.push(Difference {
                metric: string(&d[0], "metric")?,
                minuend_run: string(&d[1], "minuend_run")?,
                minuend: float(&d[2], "minuend")?,
                subtrahend_run: string(&d[3], "subtrahend_run")?,
                subtrahend: float(&d[4], "subtrahend")?,
                experiment: string(&d[5], "experiment")?,
                cited: int(&d[6], "cited")? > 0,
            });
        }
        out.push(trace);
    }
    Ok(out)
}

/// A measured figure that none of the claim's cited runs produced.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Mismatch {
    pub claim: String,
    pub figure: String,
    /// What the runs the claim rests on actually measured.
    pub cited_results: Vec<String>,
    /// Other claims that state a value those runs produced.
    pub stated_by: Vec<String>,
}

/// Every finding in one pass.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Audit {
    pub report: Report,
    pub stale: Vec<Stale>,
    pub mismatches: Vec<Mismatch>,
}

impl Audit {
    pub fn findings(&self) -> usize {
        self.report.unsupported.len() + self.stale.len() + self.mismatches.len()
    }
}

pub fn audit(path: &Path) -> Result<Audit, StoreError> {
    let report = unsupported(path)?;
    let db = open_read_only(path)?;
    let mut conn = db.connect()?;
    let stale = stale(&mut conn, None)?;
    let mismatches = conn
        .query(queries::MISMATCHES)?
        .rows
        .iter()
        .map(|row| {
            Ok(Mismatch {
                claim: string(&row[0], "claim")?,
                figure: string(&row[1], "figure")?,
                cited_results: string_list(&row[2], "cited_results")?,
                stated_by: string_list(&row[3], "stated_by")?,
            })
        })
        .collect::<Result<_, StoreError>>()?;
    Ok(Audit {
        report,
        stale,
        mismatches,
    })
}

/// The result of a read-only Cypher statement, with each cell as a Cypher literal.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Table {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<String>>,
}

/// Run any statement against the store opened read-only; writes are refused by Relate.
pub fn query(path: &Path, cypher: &str) -> Result<Table, StoreError> {
    let db = open_read_only(path)?;
    let mut conn = db.connect()?;
    let result = conn.query(cypher)?;
    Ok(Table {
        columns: result.header.columns.clone(),
        rows: result
            .rows
            .iter()
            .map(|row| row.iter().map(relate::Value::to_cypher_literal).collect())
            .collect(),
    })
}

/// Every node and relationship as one sorted line each, independent of internal IDs.
/// Two stores with the same dump hold the same graph.
pub fn dump(path: &Path) -> Result<Vec<String>, StoreError> {
    let db = open_read_only(path)?;
    let mut conn = db.connect()?;
    let node = |labels: &relate::Value, props: &relate::Value| -> Result<String, StoreError> {
        let mut labels = string_list(labels, "labels")?;
        labels.sort();
        Ok(format!(
            "(:{} {})",
            labels.join(":"),
            props.to_cypher_literal()
        ))
    };
    let mut lines = Vec::new();
    for row in &conn.query(queries::DUMP_NODES)?.rows {
        lines.push(node(&row[0], &row[1])?);
    }
    for row in &conn.query(queries::DUMP_RELATIONSHIPS)?.rows {
        lines.push(format!(
            "{}-[:{} {}]->{}",
            node(&row[0], &row[1])?,
            string(&row[2], "type")?,
            row[3].to_cypher_literal(),
            node(&row[4], &row[5])?
        ));
    }
    lines.sort();
    Ok(lines)
}

fn first<'a>(rows: &'a [Vec<relate::Value>], what: &str) -> Result<&'a relate::Value, StoreError> {
    rows.first()
        .and_then(|row| row.first())
        .ok_or_else(|| StoreError::Unexpected(what.to_string()))
}
