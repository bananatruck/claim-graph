//! Every Cypher statement Claim Graph runs, in one place.
//!
//! Writes build the graph inside one transaction; reads answer questions from the
//! reopened store. SPEC.md explains each one.

/// Each write statement and the parameter it reads its rows from (`None` for statements
/// that only read what earlier statements wrote).
pub const WRITES: &[(&str, Option<&str>)] = &[
    (
        "CREATE (:Draft {path: $draft.path, title: $draft.title})",
        Some("draft"),
    ),
    (
        "MATCH (d:Draft) UNWIND $sources AS s \
         CREATE (d)-[:LISTS]->(:Source {key: s.key, entry: s.entry})",
        Some("sources"),
    ),
    (
        "UNWIND $experiments AS e \
         CREATE (:Experiment {id: e.id, name: e.name, logged: true})",
        Some("experiments"),
    ),
    (
        "UNWIND $supersedes AS s \
         MATCH (newer:Experiment {id: s.newer}), (older:Experiment {id: s.older}) \
         CREATE (newer)-[:SUPERSEDES]->(older)",
        Some("supersedes"),
    ),
    (
        "UNWIND $runs AS r MATCH (e:Experiment {id: r.experiment}) \
         CREATE (e)-[:HAS_RUN]->(:Run {id: r.id, seed: r.seed, status: r.status, logged: true})",
        Some("runs"),
    ),
    (
        "UNWIND $metrics AS m MATCH (r:Run {id: m.run}) \
         CREATE (r)-[:PRODUCED]->(:Metric {run: m.run, name: m.name, value: m.value})",
        Some("metrics"),
    ),
    (
        "MATCH (d:Draft) UNWIND $claims AS c \
         CREATE (d)-[:HAS_SENTENCE]->(:Sentence:Claim {id: c.id, ordinal: c.ordinal, \
         section: c.section, text: c.text, reasons: c.reasons})",
        Some("claims"),
    ),
    (
        "MATCH (d:Draft) UNWIND $prose AS p \
         CREATE (d)-[:HAS_SENTENCE]->(:Sentence {id: p.id, ordinal: p.ordinal, \
         section: p.section, text: p.text, reasons: []})",
        Some("prose"),
    ),
    // A citation to a key the References section does not list creates a Source with no
    // LISTS relationship, so the gap is a fact in the graph rather than a warning.
    (
        "UNWIND $cites AS x MATCH (c:Claim {id: x.claim}) \
         MERGE (s:Source {key: x.key}) \
         CREATE (c)-[:CITES]->(s)",
        Some("cites"),
    ),
    (
        "UNWIND $run_links AS x MATCH (c:Claim {id: x.claim}) \
         MERGE (r:Run {id: x.id}) ON CREATE SET r.logged = false \
         CREATE (c)-[:SUPPORTED_BY]->(r)",
        Some("run_links"),
    ),
    (
        "UNWIND $experiment_links AS x MATCH (c:Claim {id: x.claim}) \
         MERGE (e:Experiment {id: x.id}) ON CREATE SET e.logged = false \
         CREATE (c)-[:SUPPORTED_BY]->(e)",
        Some("experiment_links"),
    ),
    (
        "UNWIND $figures AS f MATCH (c:Claim {id: f.claim}) \
         CREATE (c)-[:STATES]->(:Figure {claim: f.claim, index: f.index, \
         as_written: f.as_written, value: f.value, tolerance: f.tolerance, raw: f.raw, \
         raw_tolerance: f.raw_tolerance, percent: f.percent, measured: f.measured})",
        Some("figures"),
    ),
    // A measured figure matches a metric when the metric, rounded to the figure's written
    // precision, gives the figure: 71.3% matches 0.7128 and 0.713, not 0.714.
    (
        "MATCH (f:Figure), (m:Metric) \
         WHERE f.measured \
           AND (abs(m.value - f.value) <= f.tolerance + 1e-9 \
                OR abs(m.value - f.raw) <= f.raw_tolerance + 1e-9) \
         CREATE (f)-[:MATCHES]->(m)",
        None,
    ),
    // A measured figure that matches no metric may be the difference between two runs of
    // one experiment, as in "6.1 points above the baseline".
    (
        "MATCH (f:Figure) WHERE f.measured AND NOT EXISTS { MATCH (f)-[:MATCHES]->() } \
         MATCH (e:Experiment)-[:HAS_RUN]->(r1:Run)-[:PRODUCED]->(m1:Metric), \
               (e)-[:HAS_RUN]->(r2:Run)-[:PRODUCED]->(m2:Metric) \
         WHERE r1 <> r2 AND m1.name = m2.name AND m1.value > m2.value \
           AND (abs((m1.value - m2.value) - f.value) <= f.tolerance + 1e-9 \
                OR abs((m1.value - m2.value) * 100 - f.raw) <= f.raw_tolerance + 1e-9) \
         CREATE (f)-[:DERIVED_AS]->(d:Derivation {op: 'difference'}), \
                (d)-[:MINUEND]->(m1), (d)-[:SUBTRAHEND]->(m2)",
        None,
    ),
];

/// What ingest wrote, counted from the graph.
pub const COUNTS: &str = "\
RETURN COUNT { MATCH (n:Claim) } AS claims,
       COUNT { MATCH (n:Sentence) WHERE NOT n:Claim } AS prose,
       COUNT { MATCH (:Draft)-[:LISTS]->(s:Source) } AS sources,
       COUNT { MATCH (e:Experiment) WHERE e.logged } AS experiments,
       COUNT { MATCH (r:Run) WHERE r.logged } AS runs,
       COUNT { MATCH (m:Metric) } AS metrics,
       COUNT { MATCH (f:Figure) } AS figures,
       COUNT { MATCH (:Figure)-[x:MATCHES]->(:Metric) } AS matches,
       COUNT { MATCH (d:Derivation) } AS derivations";

/// References that point at nothing: a source the bibliography does not list, or a run
/// or experiment the run log does not contain.
pub const UNRESOLVED: &str = "\
MATCH (c:Claim)-[:CITES]->(s:Source)
WHERE NOT EXISTS { MATCH (:Draft)-[:LISTS]->(s) }
RETURN c.ordinal AS ordinal, c.id AS claim, '@' + s.key AS ref
UNION
MATCH (c:Claim)-[:SUPPORTED_BY]->(x)
WHERE x.logged = false
RETURN c.ordinal AS ordinal, c.id AS claim,
       CASE WHEN x:Run THEN 'run:' ELSE 'exp:' END + x.id AS ref";

/// A claim is supported when a path of CITES, SUPPORTED_BY and HAS_RUN leads to a source
/// the draft lists or to a completed run.
pub const UNSUPPORTED: &str = "\
MATCH (c:Claim)
WHERE NOT EXISTS {
  MATCH (c)-[:CITES|SUPPORTED_BY|HAS_RUN*1..2]->(t)
  WHERE (t:Source AND EXISTS { MATCH (:Draft)-[:LISTS]->(t) })
     OR (t:Run AND t.status = 'completed')
}
RETURN c.id AS id, c.section AS section, c.text AS text
ORDER BY c.ordinal";

/// Sentences the claim rule left as prose.
pub const PROSE: &str = "\
MATCH (p:Sentence) WHERE NOT p:Claim
RETURN p.id AS id, p.section AS section, p.text AS text
ORDER BY p.ordinal";

pub const CLAIM_TOTAL: &str = "MATCH (c:Claim) RETURN count(c) AS n";

/// One sentence, for `explain`.
pub const SENTENCE: &str = "\
MATCH (s:Sentence {id: $id})
RETURN s.id AS id, s.section AS section, s.text AS text, s.reasons AS reasons,
       COUNT { MATCH (c:Claim) WHERE c = s } AS is_claim";

/// Every reference a claim makes, with the facts that decide whether it supports.
pub const LINKS: &str = "\
MATCH (c:Claim {id: $id})-[l:CITES|SUPPORTED_BY]->(t)
RETURN CASE WHEN t:Source THEN 'source' WHEN t:Run THEN 'run' ELSE 'experiment' END AS kind,
       coalesce(t.id, t.key) AS target,
       COUNT { MATCH (:Draft)-[:LISTS]->(t) } AS listed,
       coalesce(t.logged, true) AS logged,
       t.status AS status,
       COUNT { MATCH (t)-[:HAS_RUN]->(:Run) } AS runs,
       COUNT { MATCH (t)-[:HAS_RUN]->(r:Run) WHERE r.status = 'completed' } AS completed_runs
ORDER BY kind, target";

/// Every supporting path of one claim, as the IDs along it.
pub const SUPPORT_PATHS: &str = "\
MATCH p = (c:Claim {id: $id})-[:CITES|SUPPORTED_BY|HAS_RUN*1..2]->(t)
WHERE (t:Source AND EXISTS { MATCH (:Draft)-[:LISTS]->(t) })
   OR (t:Run AND t.status = 'completed')
RETURN [n IN nodes(p) | CASE WHEN n:Source THEN '@' + n.key ELSE n.id END] AS path
ORDER BY size(path), path";

/// Claims that rest on an experiment, directly or through one of its runs, that a later
/// experiment supersedes, following SUPERSEDES chains of any length to the newest one.
pub const STALE: &str = "\
MATCH (c:Claim)-[:SUPPORTED_BY]->(x)
WHERE $id IS NULL OR c.id = $id
MATCH (e:Experiment)
WHERE e = x OR EXISTS { MATCH (e)-[:HAS_RUN]->(x) }
MATCH chain = (newest:Experiment)-[:SUPERSEDES*1..]->(e)
WHERE NOT EXISTS { MATCH (:Experiment)-[:SUPERSEDES]->(newest) }
RETURN c.id AS claim, x.id AS via, [n IN nodes(chain) | n.id] AS chain,
       COLLECT {
         MATCH (newest)-[:HAS_RUN]->(r:Run)-[:PRODUCED]->(m:Metric)
         WHERE r.status = 'completed'
         RETURN r.id + ' ' + m.name + ' = ' + toString(m.value) AS result
         ORDER BY result
       } AS newest_results
ORDER BY c.ordinal, via";

/// Figures stated by claims, optionally narrowed to one figure text or one claim.
pub const FIGURES: &str = "\
MATCH (c:Claim)-[:STATES]->(f:Figure)
WHERE ($figure IS NULL OR f.as_written = $figure) AND ($id IS NULL OR c.id = $id)
RETURN c.id AS claim, f.index AS idx, f.as_written AS figure, f.measured AS measured
ORDER BY c.ordinal, idx";

/// Where one figure came from: each metric it matches, the run and experiment behind it,
/// whether the claim cites that run, and what the experiment's other runs measured.
pub const FIGURE_MATCHES: &str = "\
MATCH (c:Claim {id: $id})-[:STATES]->(f:Figure {index: $idx})
MATCH (f)-[:MATCHES]->(m:Metric)<-[:PRODUCED]-(r:Run)<-[:HAS_RUN]-(e:Experiment)
RETURN m.name AS metric, m.value AS value, r.id AS run, r.status AS status, e.id AS experiment,
       COUNT { MATCH (c)-[:SUPPORTED_BY|HAS_RUN*1..2]->(r) } AS cited,
       COLLECT {
         MATCH (e)-[:HAS_RUN]->(r2:Run)-[:PRODUCED]->(m2:Metric)
         WHERE r2 <> r AND m2.name = m.name
         RETURN r2.id + ' (' + r2.status + ') = ' + toString(m2.value) AS other
         ORDER BY other
       } AS other_runs
ORDER BY run, metric";

/// One figure as the difference of two runs' metrics in the same experiment.
pub const FIGURE_DERIVATIONS: &str = "\
MATCH (c:Claim {id: $id})-[:STATES]->(f:Figure {index: $idx})-[:DERIVED_AS]->(d:Derivation)
MATCH (d)-[:MINUEND]->(m1:Metric)<-[:PRODUCED]-(r1:Run)<-[:HAS_RUN]-(e:Experiment),
      (d)-[:SUBTRAHEND]->(m2:Metric)<-[:PRODUCED]-(r2:Run)
RETURN m1.name AS metric, r1.id AS minuend_run, m1.value AS minuend,
       r2.id AS subtrahend_run, m2.value AS subtrahend, e.id AS experiment,
       COUNT { MATCH (c)-[:SUPPORTED_BY|HAS_RUN*1..2]->(e2) WHERE e2 = e OR e2 = r1 OR e2 = r2 } AS cited
ORDER BY minuend_run, subtrahend_run";

/// Measured figures in claims that rest on completed runs, where none of those runs
/// produced the figure, directly or as a difference. Names the runs that did produce it
/// and the other claims that state those runs' values.
pub const MISMATCHES: &str = "\
MATCH (c:Claim)-[:STATES]->(f:Figure)
WHERE f.measured
  AND EXISTS { MATCH (c)-[:SUPPORTED_BY|HAS_RUN*1..2]->(r:Run) WHERE r.status = 'completed' }
  AND NOT EXISTS {
    MATCH (c)-[:SUPPORTED_BY|HAS_RUN*1..2]->(r:Run)-[:PRODUCED]->(m:Metric)
    WHERE r.status = 'completed'
      AND (EXISTS { MATCH (f)-[:MATCHES]->(m) }
           OR EXISTS { MATCH (f)-[:DERIVED_AS]->(:Derivation)-[:MINUEND|SUBTRAHEND]->(m) })
  }
RETURN c.id AS claim, f.as_written AS figure,
       COLLECT {
         MATCH (c)-[:SUPPORTED_BY|HAS_RUN*1..2]->(r:Run)-[:PRODUCED]->(m:Metric)
         WHERE r.status = 'completed'
         RETURN DISTINCT r.id + ' ' + m.name + ' = ' + toString(m.value) AS result
         ORDER BY result
       } AS cited_results,
       COLLECT {
         MATCH (c)-[:SUPPORTED_BY|HAS_RUN*1..2]->(r:Run)-[:PRODUCED]->(m:Metric)
               <-[:MATCHES]-(:Figure)<-[:STATES]-(other:Claim)
         WHERE r.status = 'completed' AND other <> c
         RETURN DISTINCT other.id AS other
         ORDER BY other
       } AS stated_by
ORDER BY c.ordinal, f.index";

/// Every node, for the canonical dump.
pub const DUMP_NODES: &str = "MATCH (n) RETURN labels(n) AS labels, properties(n) AS props";

/// Every relationship with both endpoints, for the canonical dump.
pub const DUMP_RELATIONSHIPS: &str = "\
MATCH (a)-[r]->(b)
RETURN labels(a) AS from_labels, properties(a) AS from_props, type(r) AS type,
       properties(r) AS props, labels(b) AS to_labels, properties(b) AS to_props";
