# Claim Graph: specification

## The idea in one sentence

Claim Graph reads a research agent's paper draft and its run log into a single-file Relate graph and reports every claim that has no path to a completed run or a cited source, every claim that rests on a superseded experiment, and every number that the runs a claim cites did not produce.

## User and problem

The user is a researcher who lets an AI research agent run experiments and write the first draft of a paper.
The agent writes fluent prose fast, and some of its sentences are not backed by anything it actually ran or read.
A claim can cite an experiment that never ran, a run that crashed, a reference missing from the bibliography, or a pilot that a later experiment replaced, and it still reads well.
A number in the abstract can drift from the number the run produced while the results section states the right one.
Checking this by hand means cross-reading the draft, the bibliography and the run log line by line.
Claim Graph turns that cross-reading into graph paths, so every finding names the path that is missing or the path that contradicts the text.

## How it uses Relate

- Surface: the `relate` Rust crate from crates.io, version 1.2.0, compiled into the binary with `default-features = false` (no Arrow or Parquet; the store uses Relate's JSON snapshot codec, which every Relate surface opens).
- Mode: single file.
  The store is one `.rl` file, which holds Relate's write-ahead log, snapshot and catalog.
- Why this mode: the graph is rebuilt from two input files, one process writes it and then exits, and judges need one artifact they can copy and open.
- Rejected: a directory store with the JSON codec would give a diffable `graph.json`, but it splits the store into a snapshot and a WAL folder; `claim-graph dump` gives a diffable text form instead.
- Rejected: in-memory mode, because nothing would be left for judges to inspect.
- Ingest writes the whole graph in one Relate transaction, and the matching of numbers to metrics runs as Cypher inside that transaction.
- Every answer is a Cypher query against the reopened store, opened read-only; nothing is answered from in-memory Rust structures.
- All values reach Cypher as query parameters, so draft text never becomes part of a statement.

## Inputs

All inputs in this repository are synthetic and say so in their first lines.
There are two datasets: `data/synthetic/` (small and tidy) and `data/synthetic/messy/` (abbreviations, lists, a table, code, rounded and conflicting figures, a superseded experiment, a run the log never recorded, and an out-of-order run log with extra fields).

### Draft (Markdown)

- The first `# ` heading is the draft title.
- A `## ` heading starts a section; the section name is stored on each sentence.
- Lines starting with `>` (notes), `|` (table rows) and `![` (images), deeper headings, and everything inside ```` ``` ```` fences or `$$` blocks are skipped.
- A paragraph is a run of non-blank lines; each list item (`- `, `* `, `1. `, `1) `) is its own paragraph.
- A sentence ends at `.`, `!` or `?` followed by whitespace or the end of the paragraph, so `71.3` does not split a sentence.
- A period after `e.g.`, `i.e.`, `et al.`, `Fig.`, `Figs.`, `Sec.`, `Eq.`, `Eqs.`, `Tab.`, `vs.`, `cf.`, `approx.` or `No.` does not end a sentence.
- `[@key]`, `[@a; @b]` or `[@a, @b]` cites sources.
- `[run:ID]` links a sentence to a run, and `[exp:ID]` links it to an experiment.
- Every non-blank line in the `References` section must be `- [@key] entry text`; anything else is a parse error with its line number.

### The claim rule

A sentence is a checkable claim when it contains a number, a comparison word, a citation, or a run or experiment link.
Every other sentence is prose: it is stored and counted, listed with `--prose`, and never reported as unsupported.
Claims are numbered `C1`, `C2`, ... and prose `P1`, `P2`, ..., each in reading order.
The comparison words are a fixed list in `src/classify.rs` (`better`, `higher`, `than`, `outperforms`, `halves`, `match` and so on), matched as whole words.
The rule sits behind one function, `classify`, so it can be replaced without touching the parser or the store.
It errs towards checking: a sentence such as "Fig. 3 shows the error rate" counts as a claim because it holds a number.

### Figures

A figure is a number in a claim, outside reference markers, and not part of a word (`r2`, `v1.2` and `3rd` are not figures).
`71.3%` is stored as the value 0.713 and also as written, 71.3.
Each figure carries the precision it was written with: half a unit in its last digit.
Only percentages and numbers written with decimals count as measurements; bare integers are treated as counts ("3 seeds", "2024") and never matched to metrics.

### Run log (JSON Lines)

One JSON object per line, with a `kind` field:

- `{"kind":"experiment","id":"E12","name":"...","supersedes":"E11"}`, where `supersedes` is optional.
- `{"kind":"run","id":"m-07","experiment":"E12","seed":1,"status":"completed"}`, where `seed` is optional.
- `{"kind":"metric","run":"m-07","name":"accuracy","value":0.739}`
- `{"kind":"note","text":"..."}`, which is ignored and lets a log carry its own synthetic-data label.

Records may come in any order, and extra fields such as `timestamp` or `git_sha` are ignored.
IDs must be unique, every run must name a defined experiment, every metric a defined run, and `supersedes` another defined experiment.
An unknown `kind`, a missing required field or invalid JSON is an error with its line number.
Only the status `completed` counts as support.

## Graph model

### Node labels

| Label | Properties | Meaning |
| --- | --- | --- |
| `Draft` | `path`, `title` | The draft file that was ingested |
| `Sentence` | `id`, `ordinal`, `section`, `text`, `reasons` | One sentence of the draft; `reasons` says why it is a claim, empty for prose |
| `Claim` | (second label on a `Sentence`) | A checkable claim |
| `Source` | `key`, `entry` | A bibliography entry, or a cited key with no entry |
| `Experiment` | `id`, `name`, `logged` | An experiment; `logged: false` when only the draft names it |
| `Run` | `id`, `seed`, `status`, `logged` | A run; `logged: false` when only the draft names it |
| `Metric` | `run`, `name`, `value` | Evidence: a number a run produced |
| `Figure` | `claim`, `index`, `as_written`, `value`, `tolerance`, `raw`, `raw_tolerance`, `percent`, `measured` | A number as a claim states it |
| `Derivation` | `op` | A figure computed from two metrics; `op` is `difference` |

### Relationship types

| Type | From | To | Meaning |
| --- | --- | --- | --- |
| `HAS_SENTENCE` | `Draft` | `Sentence` | The draft contains the sentence |
| `LISTS` | `Draft` | `Source` | The bibliography lists the source |
| `CITES` | `Claim` | `Source` | The claim cites the source |
| `SUPPORTED_BY` | `Claim` | `Run` or `Experiment` | The claim points at a run or an experiment |
| `HAS_RUN` | `Experiment` | `Run` | The run belongs to the experiment |
| `SUPERSEDES` | `Experiment` | `Experiment` | A later experiment replaces an earlier one |
| `PRODUCED` | `Run` | `Metric` | The run produced the metric |
| `STATES` | `Claim` | `Figure` | The claim states the number |
| `MATCHES` | `Figure` | `Metric` | The metric, rounded to the figure's written precision, gives the figure |
| `DERIVED_AS` | `Figure` | `Derivation` | The figure is the difference of two metrics |
| `MINUEND`, `SUBTRAHEND` | `Derivation` | `Metric` | The two metrics of a difference, from two runs of one experiment |

A reference to something the inputs do not define still becomes a node: a cited key with no `LISTS` relationship, or a run or experiment with `logged: false`.
The gap is a fact in the graph, so the same queries find it.

### How numbers are matched

Matching runs as two Cypher statements at the end of ingest, inside the same transaction.
A measured figure `MATCHES` a metric when the metric is within the figure's tolerance on either scale, so `71.3%` matches 0.713 and 0.7128 but not 0.714.
A measured figure with no direct match gets a `Derivation` when it equals the difference between the same metric in two runs of one experiment, on either scale, so "6.1 points" matches 0.713 minus 0.652.

## Queries

All queries are in `src/queries.rs`.

### Which claims are unsupported

```cypher
MATCH (c:Claim)
WHERE NOT EXISTS {
  MATCH (c)-[:CITES|SUPPORTED_BY|HAS_RUN*1..2]->(t)
  WHERE (t:Source AND EXISTS { MATCH (:Draft)-[:LISTS]->(t) })
     OR (t:Run AND t.status = 'completed')
}
RETURN c.id AS id, c.section AS section, c.text AS text
ORDER BY c.ordinal
```

This catches a claim with no references, a claim whose experiment has no runs, a claim whose only run failed or was never logged, and a claim whose citation is missing from the bibliography.

### Why is this claim unsupported (`explain`)

For each `CITES` or `SUPPORTED_BY` relationship of the claim, one query reads whether the target is listed or logged, its status, and how many of its runs completed; a second query returns every supporting path as the IDs along it.

### Where did this number come from (`trace`)

```cypher
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
```

The answer says whether the claim cites the run that produced its number, and shows what the experiment's other runs measured, which exposes a best seed reported alone.
A second query follows `DERIVED_AS` to the two runs of a difference.
A figure with neither is reported plainly as a number no run produced.

### Which claims rest on superseded experiments (`audit`)

```cypher
MATCH (c:Claim)-[:SUPPORTED_BY]->(x)
MATCH (e:Experiment)
WHERE e = x OR EXISTS { MATCH (e)-[:HAS_RUN]->(x) }
MATCH chain = (newest:Experiment)-[:SUPERSEDES*1..]->(e)
WHERE NOT EXISTS { MATCH (:Experiment)-[:SUPERSEDES]->(newest) }
RETURN c.id AS claim, x.id AS via, [n IN nodes(chain) | n.id] AS chain,
       COLLECT { ... newest experiment's completed results ... } AS newest_results
```

The `SUPERSEDES*1..` chain has no fixed length, which is the part a relational table makes painful.

### Which numbers did the cited runs not produce (`audit`)

For each measured figure in a claim that rests on at least one completed run, the query checks that some metric of those runs `MATCHES` the figure or is part of its `Derivation`.
When none is, it reports what those runs measured and which other claims state those values, which catches an abstract that says 74.2% while the results section says 73.9% for the same run.

## Command-line surface

- `claim-graph ingest --draft FILE --runs FILE --store FILE.rl [--replace]`
- `claim-graph unsupported --store FILE.rl [--prose] [--fail] [--format text|json]`
- `claim-graph explain --store FILE.rl --claim C4 [--format text|json]`
- `claim-graph trace --store FILE.rl (--figure "71.3%" | --claim C1) [--format text|json]`
- `claim-graph audit --store FILE.rl [--fail] [--format text|json]`
- `claim-graph query --store FILE.rl "CYPHER"` runs any statement read-only, so judges can explore the store without installing anything else.
- `claim-graph dump --store FILE.rl` prints every node and relationship as one sorted line.

Exit status: 0 success, 1 runtime or input error, 2 usage error, 3 findings when `--fail` is given.

## Reproducibility

Relate's `.rl` file is not byte-for-byte reproducible: rebuilding from the same inputs gives different bytes and the same graph.
The committed stores are therefore checked by content: `cargo test` rebuilds both from their inputs into a temporary directory and compares the `dump` of each with the `dump` of the committed file.
`scripts/rebuild-stores.sh` regenerates both stores with the documented commands.

## Limits

- The sentence splitter and the claim rule are deterministic heuristics, not language understanding.
- Only the three reference forms above are recognised; `\cite{}`, numeric `[1]` citations and author-year text are not.
- Matching is by value and written precision only; a figure can match an unrelated metric that happens to have the same value, and `trace` shows every match so a reader can judge.
- Bare integers are never matched to metrics.
- Differences are searched only between two runs of the same experiment.
- Claim Graph does not judge whether a cited source says what the claim says, or whether the numbers justify the wording.
- Ingest rebuilds a store; there are no incremental updates.
- No model calls and no network access at run time.

## Test plan

- Draft parser: title, sections, sentence splitting around decimals and abbreviations, notes, tables, images, code and math skipped, all list styles, all reference forms, figures outside markers only, malformed references with their line numbers.
- Claim rule: each reason on its own, plain prose, whole-word matching, numbers inside identifiers.
- Figures: percentages, decimals, integers, thousands separators, multipliers, numbers inside words, unit suffixes.
- Run log: each kind, any order, extra fields, `supersedes`, duplicates, dangling references, invalid JSON.
- Store, on real `.rl` files reopened read-only: node and relationship counts, unsupported claims and prose, a report that changes when the graph is edited directly, `explain` reasons and paths, `trace` matches, differences and the no-source case, stale chains, mismatches, read-only `query`, refusal to overwrite, the path rule, and a missing store.
- The committed stores match a rebuild by content, and are single-file bundles with the JSON codec.
- End to end on the built binary: every command, text and JSON output, `--fail` exit status, usage errors.
- CI runs `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test` and `cargo build --release`.

## Demo

The 1 to 2 minute script is `submission/demo-script.md`.
