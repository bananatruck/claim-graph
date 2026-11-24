#!/usr/bin/env bash
# Rebuild the committed Relate stores from the synthetic inputs.
# Run from the repository root. `cargo test` checks that the committed stores hold
# exactly the graph these commands build (the .rl bytes differ on every rebuild).
set -euo pipefail

cargo build --release --locked
bin=./target/release/claim-graph

"$bin" ingest --replace \
  --draft data/synthetic/draft.md \
  --runs data/synthetic/runs.jsonl \
  --store store/claim-graph.rl

"$bin" ingest --replace \
  --draft data/synthetic/messy/draft.md \
  --runs data/synthetic/messy/runs.jsonl \
  --store store/messy.rl
