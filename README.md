# Claim Graph

Find every claim in a research agent's paper draft that has no path to a completed run or a cited source.

Status: walking skeleton.
The `ingest` and `unsupported` commands work end to end on a real Relate store; the rest of the plan is in [SPEC.md](SPEC.md).

## Use case

An AI research agent can run experiments and write a fluent draft about them, and some sentences in that draft are not backed by anything it ran or read.
Claim Graph reads the draft and the agent's run log into one graph and asks a single question of it: which claims have no path to evidence?
The relationships that matter are claim to source (`CITES`), claim to run or experiment (`SUPPORTED_BY`), experiment to run (`HAS_RUN`) and run to metric (`PRODUCED`).
A claim is supported when a path leads from it to a listed source or to a run whose status is `completed`.
That one rule catches a sentence with no reference, a claim about an experiment that never ran, a claim resting on a crashed run, and a citation that is missing from the bibliography.

## How it uses Relate

- Mode: single file.
- Why this mode: the graph is rebuilt from two input files by one process that then exits, and a single `.rl` file is one artifact that can be committed, copied and opened by anyone.
- What the graph models: `Draft`, `Claim`, `Source`, `Experiment`, `Run` and `Metric` nodes, joined by `HAS_CLAIM`, `LISTS`, `CITES`, `SUPPORTED_BY`, `HAS_RUN` and `PRODUCED` relationships (full model in [SPEC.md](SPEC.md#graph-model)).
- Relate is embedded through the `relate` Rust crate (version 1.2.0 from crates.io), built without its default `parquet` feature, so the store uses Relate's JSON snapshot codec inside the `.rl` file.
- `ingest` writes the whole graph in one Relate transaction and checkpoints the store; `unsupported` reopens it read-only and answers with this Cypher query:

```cypher
MATCH (c:Claim)
WHERE NOT EXISTS {
  MATCH (c)-[:CITES|SUPPORTED_BY|HAS_RUN*1..2]->(t)
  WHERE t:Source OR (t:Run AND t.status = 'completed')
}
RETURN c.id AS id, c.section AS section, c.text AS text, c.unresolved AS unresolved
ORDER BY c.ordinal
```

## Setup and run

Prerequisites: Rust 1.88 or later with Cargo.
The first build compiles the Relate engine from source and takes a few minutes.

```bash
cargo build --release
./target/release/claim-graph ingest \
  --draft data/synthetic/draft.md \
  --runs data/synthetic/runs.jsonl \
  --store store/claim-graph.rl --replace
./target/release/claim-graph unsupported --store store/claim-graph.rl
```

Output of the last command on the synthetic data:

```text
4 of 7 claims have no path to a completed run or a cited source:

C3  [Abstract]  The planner also halves the number of tool calls per task.
C4  [Results]  Retrieval helps most on long tasks [exp:E2].
C6  [Results]  Ablating the critic lowers success to 65.0% [run:r-004].
C7  [Results]  The gains match those reported for larger planners [@okafor2023].
    unresolved: @okafor2023
```

Add `--format json` for machine-readable output.
Run the tests with `cargo test`.

## Where the data lives

- Persistent Relate store: [`store/claim-graph.rl`](store/claim-graph.rl), built from the inputs below by the command above.
- Inputs: [`data/synthetic/draft.md`](data/synthetic/draft.md) and [`data/synthetic/runs.jsonl`](data/synthetic/runs.jsonl).
  Both are synthetic test data written by hand; no experiment in them was run and the cited works do not exist.
- Rebuilding the store gives the same graph, but not the same bytes, so the file changes on every rebuild.
