# Claim Graph: specification

## The idea in one sentence

Claim Graph reads a research agent's paper draft and its run log into a single-file Relate graph and reports every claim that has no path to a completed run or a cited source.

## User and problem

The user is a researcher who lets an AI research agent run experiments and write the first draft of a paper.
The agent writes fluent prose fast, and some of its sentences are not backed by anything it actually ran or read.
A claim can cite an experiment that never ran, a run that crashed, or a reference that is not in the bibliography, and it still reads well.
Checking this by hand means cross-reading the draft, the bibliography and the run log line by line.
Claim Graph turns that cross-reading into graph paths: a claim is supported when a path leads from it to a completed run or to a listed source, and unsupported when no such path exists.
The same graph answers the follow-up question "where did this number come from" by walking from a figure in the text to the metric, run and experiment that produced it.

## How it uses Relate

- Surface: the `relate` Rust crate from crates.io, version 1.2.0, compiled into the binary with `default-features = false` (no Arrow or Parquet; the store uses Relate's JSON snapshot codec).
- Mode: single file.
  The store is one `.rl` file, which holds Relate's write-ahead log, snapshot and catalog.
- Why this mode: the graph is rebuilt from two input files, one process writes it and then exits, and judges need one artifact they can copy and open with the `relate` CLI or the crate.
- Rejected: a directory store with the JSON codec would give a diffable `graph.json`, but it splits the store into a snapshot and a WAL folder, and diffs of a generated graph are not the point here.
- Rejected: in-memory mode, because nothing would be left for judges to inspect.
- Every query in this spec runs against the reopened store, never against in-memory Rust structures.
- All values reach Cypher as query parameters, so draft text never becomes part of a statement.

## Inputs

Both inputs in this repository are synthetic and say so in their first lines.

### Draft (Markdown)

- The first `# ` heading is the draft title.
- A `## ` heading starts a section; the section name is stored on each claim.
- Lines starting with `>` are notes and are ignored.
- A paragraph is a run of non-blank lines; each list item (`- `) is its own paragraph.
- Each sentence in a paragraph outside the `References` section is a claim.
- A sentence ends at `.`, `!` or `?` followed by whitespace or the end of the paragraph, so `71.3` does not split a sentence.
- Abbreviations such as `e.g.` do split a sentence; this is a known limitation of the deterministic splitter.
- Claims are numbered `C1`, `C2`, ... in reading order.
- `[@key]` or `[@a; @b]` cites a source.
- `[run:ID]` links a claim to a run, and `[exp:ID]` links it to an experiment.
- Every non-blank line in the `References` section must be `- [@key] entry text`; anything else is a parse error with its line number.

### Run log (JSON Lines)

One JSON object per line, with a `kind` field:

- `{"kind":"experiment","id":"E1","name":"..."}`
- `{"kind":"run","id":"r-001","experiment":"E1","seed":1,"status":"completed"}`
- `{"kind":"metric","run":"r-001","name":"success_rate","value":0.652}`
- `{"kind":"note","text":"..."}`, which is ignored and lets a log carry its own synthetic-data label.

A run must name an experiment defined earlier, a metric must name a run defined earlier, IDs must be unique, and an unknown `kind` is an error.
Blank lines are allowed.
Errors carry the line number.

## Graph model

### Node labels

| Label | Properties | Meaning |
| --- | --- | --- |
| `Draft` | `path`, `title` | The draft file that was ingested |
| `Claim` | `id`, `ordinal`, `section`, `text`, `unresolved` | One sentence of the draft; `unresolved` lists references that point at nothing |
| `Source` | `key`, `entry` | One entry of the draft's References section |
| `Experiment` | `id`, `name` | An experiment from the run log |
| `Run` | `id`, `seed`, `status` | One run of an experiment |
| `Metric` | `run`, `name`, `value` | Evidence: a number a run produced |
| `Figure` | `as_written`, `value` | Phase 2: a number as it appears in a claim |

### Relationship types

| Type | From | To | Meaning |
| --- | --- | --- | --- |
| `HAS_CLAIM` | `Draft` | `Claim` | The draft contains the claim |
| `LISTS` | `Draft` | `Source` | The draft's bibliography lists the source |
| `CITES` | `Claim` | `Source` | The claim cites the source |
| `SUPPORTED_BY` | `Claim` | `Run` or `Experiment` | The claim points at a run or an experiment |
| `HAS_RUN` | `Experiment` | `Run` | The run belongs to the experiment |
| `PRODUCED` | `Run` | `Metric` | The run produced the metric |
| `STATES` | `Claim` | `Figure` | Phase 2: the claim states the number |
| `MATCHES` | `Figure` | `Metric` | Phase 2: the stated number equals the metric value, with `71.3%` equal to `0.713` |

A reference to an unknown source key, run ID or experiment ID creates no relationship; the claim keeps it in `unresolved` and ingest prints a warning.

## Queries

### Which claims are unsupported

A claim is supported when a directed path of `CITES`, `SUPPORTED_BY` and `HAS_RUN` relationships leads from it to a `Source`, or to a `Run` whose status is `completed`.

```cypher
MATCH (c:Claim)
WHERE NOT EXISTS {
  MATCH (c)-[:CITES|SUPPORTED_BY|HAS_RUN*1..2]->(t)
  WHERE t:Source OR (t:Run AND t.status = 'completed')
}
RETURN c.id AS id, c.section AS section, c.text AS text, c.unresolved AS unresolved
ORDER BY c.ordinal
```

The bound of two hops covers the longest supporting path, `Claim -> Experiment -> Run`.
This catches a claim with no references, a claim whose experiment has no runs, a claim whose only run failed, and a claim whose citation is missing from the bibliography.

### Where did this number come from (phase 2)

```cypher
MATCH (c:Claim)-[:STATES]->(f:Figure {as_written: $figure})
OPTIONAL MATCH (f)-[:MATCHES]->(m:Metric)<-[:PRODUCED]-(r:Run)
OPTIONAL MATCH (e:Experiment)-[:HAS_RUN]->(r)
RETURN c.id AS claim, f.as_written AS figure, m.name AS metric, m.value AS value, r.id AS run, r.status AS status, e.id AS experiment
ORDER BY claim, run
```

A row with a null `metric` is a number that no run produced.

## Command-line surface

Phase 1, built in the walking skeleton:

- `claim-graph ingest --draft FILE --runs FILE --store FILE.rl [--replace]` parses both inputs, writes the graph in one transaction, checkpoints and closes the store.
  It refuses to overwrite an existing store unless `--replace` is given, and the store path must end in `.rl`.
- `claim-graph unsupported --store FILE.rl [--format text|json]` opens the store read-only and prints the unsupported claims.

Phase 2:

- `claim-graph trace --store FILE.rl --figure "71.3%"` answers where a number came from.
- `claim-graph explain --store FILE.rl --claim C2` prints every supporting path of one claim.
- `claim-graph unsupported --fail` exits with status 3 when any claim is unsupported, so a research pipeline can gate on it.

Exit codes: 0 success, 1 runtime or input error, 2 usage error (from the argument parser).

## Out of scope

- Model calls of any kind, and any network access at run time.
- Judging whether a cited source really says what the claim says.
- Judging whether a run's numbers are good enough for the claim's wording.
- Parsing LaTeX, PDF or real-world run-tracker formats; the inputs are the two formats above.
- Incremental updates to an existing store; ingest rebuilds it.

## Test plan

- Draft parser unit tests: title, sections, sentence splitting around decimals, notes ignored, list items, all three reference kinds, multi-key citations, a malformed References line reported with its line number.
- Run log parser unit tests: each kind, blank lines, unknown kind, duplicate IDs, a run naming an unknown experiment, a metric naming an unknown run, invalid JSON with its line number.
- Store tests on a real `.rl` file in a temporary directory: ingest, drop the database, reopen it read-only, and check node and relationship counts and the exact unsupported claim IDs from the query above.
- Store test that ingest refuses an existing store without `--replace`.
- End-to-end test that runs the built binary on the synthetic inputs and checks its output and exit code.
- CI runs `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test` and `cargo build --release`.

## Demo script (90 seconds)

1. (0:00) "An AI research agent wrote this draft and ran these experiments. Which of its claims are backed by anything?"
   Show `data/synthetic/draft.md` and `data/synthetic/runs.jsonl`, both labelled synthetic.
2. (0:15) Run `claim-graph ingest --draft data/synthetic/draft.md --runs data/synthetic/runs.jsonl --store store/claim-graph.rl --replace`.
   Point at the warning for the citation that is not in the bibliography.
3. (0:30) Run `claim-graph unsupported --store store/claim-graph.rl`.
   Read out the four unsupported claims and why each fails: no reference, an experiment that never ran, a run that crashed, a missing citation.
4. (0:55) Open the same file with the `relate` CLI read-only and run the unsupported-claims Cypher by hand, to show the answer comes from the graph.
5. (1:15) Show the path for one supported claim, `Claim -> Experiment -> Run`, in the CLI output.
6. (1:25) "One file, one query, and every unsupported claim has a name."
