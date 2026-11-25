# Claim Graph

Find the claims in a research agent's draft that rest on nothing, on a superseded experiment, or on a number the cited run never produced.

## Use case

AI research agents now run experiments and write the first draft of the paper about them.
The prose is fluent, and some of it is not backed by anything the agent ran or read: an experiment that never ran, a run that crashed, a citation missing from the bibliography, a pilot that a later experiment replaced, or an abstract that says 74.2% when the run produced 73.9%.
The person who signs the paper has to catch these before a reviewer does.

Claim Graph reads the draft (Markdown) and the agent's run log (JSON Lines) into one Relate graph and checks the claims against it.
The relationships that carry the answer are:

- claim to source (`CITES`), and draft to source (`LISTS`), so a citation counts only if the bibliography lists it, and a dangling citation is reported even when another reference supports the claim;
- claim to run or experiment (`SUPPORTED_BY`), and experiment to run (`HAS_RUN`), so a claim counts as supported only through a completed run;
- experiment to experiment (`SUPERSEDES`), followed to any depth, so a claim on a replaced pilot is flagged with the newest results;
- claim to number (`STATES`), number to metric (`MATCHES`) or to a difference of two metrics (`DERIVED_AS`, `MINUEND`, `SUBTRAHEND`), and run to metric (`PRODUCED`), so every number in the text can be walked back to the run that produced it, or shown to have no source.

A sentence is checked when it contains a number, a comparison word, a citation or a run or experiment link; every other sentence is counted as prose and listed only with `--prose`.
The rule is deterministic and lives in one replaceable function (`src/classify.rs`).

## How it uses Relate

- Mode: single file.
- Why this mode: the graph is rebuilt from two input files by one process that then exits, and a single `.rl` file is one artifact a judge can copy, open and query.
  A directory store would split it into a snapshot and a WAL folder; `claim-graph dump` gives a diffable text view instead.
- What the graph models: `Draft`, `Sentence`/`Claim`, `Source`, `Experiment`, `Run`, `Metric`, `Figure` and `Derivation` nodes, joined by the relationships above (full model in [SPEC.md](SPEC.md#graph-model)).
- Relate is embedded through the `relate` Rust crate, version 1.2.0 from crates.io, built without its default `parquet` feature, so the store uses Relate's JSON snapshot codec inside the `.rl` file.
- Ingest writes everything in one Relate transaction, and the matching of numbers to metrics runs as Cypher inside it.
- Every answer is a Cypher query on the reopened store, opened read-only; all queries are in [`src/queries.rs`](src/queries.rs).
  For example, the unsupported-claims check:

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

Relate capabilities used: embedded Rust API, single-file `.rl` store, explicit transactions, read-only opens, query parameters, `MERGE ... ON CREATE SET`, `EXISTS`/`COUNT`/`COLLECT` subqueries, variable-length paths (`*1..2` and unbounded `SUPERSEDES*1..`), path functions (`nodes(p)`), and `labels`/`properties` for the canonical dump.

## Setup and run

Prerequisites: Rust 1.88 or later with Cargo (`rustup update` if Cargo reports an older compiler), and a network connection for the first build to fetch crates.
The first build compiles the Relate engine from source and takes a few minutes.

```bash
git clone https://github.com/bananatruck/claim-graph.git claim-graph && cd claim-graph
cargo build --release
```

The committed stores are ready to query:

```bash
./target/release/claim-graph unsupported --store store/claim-graph.rl
./target/release/claim-graph explain --store store/claim-graph.rl --claim C2
./target/release/claim-graph trace --store store/claim-graph.rl --figure "71.3%"
./target/release/claim-graph audit --store store/messy.rl
```

Output of the last command:

```text
Unsupported claims

2 of 8 claims have no path to a completed run or a cited source:

C7  [Results]  A second seed confirms the result [run:m-99].
    unresolved: run:m-99
C8  [Results]  Fig. 3 shows the error rate by notebook length.

Broken references on otherwise supported claims

C6 cites source ndiaye2022: not listed in References

Claims on superseded experiments

Stale: C5 rests on m-01, from experiment E10, which is superseded: E12 supersedes E11 supersedes E10
  newest results (E12): m-06 accuracy = 0.695; m-06 tool_calls_per_question = 6.8; m-07 accuracy = 0.739; m-07 tool_calls_per_question = 4.21

Figures the cited runs did not produce

C1 states 74.2%, but its cited runs measured: m-07 accuracy = 0.739; m-07 tool_calls_per_question = 4.21
    the values those runs produced are stated by C3, C4

5 findings.
```

`unsupported` is the structural check alone; `audit` is the full verdict, and the one to gate on.

Explore the graph with any read-only Cypher, without installing anything else:

```bash
./target/release/claim-graph query --store store/messy.rl \
  "MATCH p = (:Experiment)-[:SUPERSEDES*]->(:Experiment) RETURN [n IN nodes(p) | n.id] AS chain"
```

Build a store from the inputs, or from your own draft and run log (formats in [SPEC.md](SPEC.md#inputs)); `out/` is ignored by git, so the committed stores stay as they are:

```bash
mkdir -p out
./target/release/claim-graph ingest \
  --draft data/synthetic/draft.md --runs data/synthetic/runs.jsonl --store out/claim-graph.rl
```

Other options: `--format json` on `unsupported`, `explain`, `trace`, `audit` and `query`; `--prose` on `unsupported`; and `--fail` on `unsupported` and `audit` to exit with status 3 when there are findings, for use in a pipeline.
`claim-graph --help` and `claim-graph <command> --help` list everything.
Run the tests with `cargo test`.

The store is an ordinary Relate store, so the `relate` CLI should open it too (`relate --database store/claim-graph.rl --read-only "MATCH (c:Claim) RETURN c.id, c.text"`); that path has not been tested in this project.

## Where the data lives

- Persistent Relate stores: [`store/claim-graph.rl`](store/claim-graph.rl) (tidy draft) and [`store/messy.rl`](store/messy.rl) (messy draft).
- Their inputs: [`data/synthetic/`](data/synthetic/) and [`data/synthetic/messy/`](data/synthetic/messy/).
  All inputs are synthetic test data written by hand; no experiment in them was run and the cited works do not exist.
- Relate's `.rl` bytes differ on every rebuild, so the stores are checked by content: `cargo test` rebuilds both and compares `claim-graph dump` of each with the committed file.
  `scripts/rebuild-stores.sh` regenerates them with the commands above.

## Limits

- Sentence splitting and the claim rule are deterministic heuristics, not language understanding; they lean towards checking too much (for example "Fig. 3 shows..." counts as a claim because it holds a number).
- A sentence with no number, no reference and no word from the comparison list in `src/classify.rs` is prose and is not checked.
- Table rows, images and code are skipped, so a result stated only in a table is not checked.
- Only `[@key]`, `[run:ID]` and `[exp:ID]` references are recognised.
- Numbers match by value and written precision, so an unrelated metric with the same value can match; `trace` shows every match, with the run's status and whether the claim cites it.
- Bare integers are treated as counts and never matched to metrics, and unit suffixes such as `k` or `ms` are kept in the text but not used to rescale the number.
- A claim that cites several runs is checked against all of them together: which number in the sentence belongs to which run is not parsed.
- Numbers in a claim supported only by citations are not checked, since the cited work is not in the graph.
- A difference is recognised only between two completed runs of one experiment the claim rests on.
- Most checks follow paths of one or two hops; the unbounded traversal is the `SUPERSEDES` chain, and number matching compares every figure with every metric, which is fine for one paper and its log.
- A store holds one draft and is rebuilt on every ingest.
- Ingest without `--replace` publishes the store with a hard link so it can never overwrite one, so the store must go on a file system with hard links (not FAT or exFAT); `--replace` is not a workaround, because it overwrites.
- Claim Graph does not judge whether a source says what the claim says.

## Licence

MIT, see [LICENSE](LICENSE).
Relate is Apache-2.0 and is used as a dependency, unmodified.
