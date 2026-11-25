#!/usr/bin/env bash
# Rebuild the committed Relate stores from the synthetic inputs.
# Run from the repository root. `cargo test` checks that the committed stores hold
# exactly the graph these commands build (the .rl bytes differ on every rebuild).
# `cargo run` builds and runs the current code wherever CARGO_TARGET_DIR points.
set -euo pipefail

ingest() {
  cargo run --release --locked --quiet -- ingest --replace "$@"
}

ingest \
  --draft data/synthetic/draft.md \
  --runs data/synthetic/runs.jsonl \
  --store store/claim-graph.rl

ingest \
  --draft data/synthetic/messy/draft.md \
  --runs data/synthetic/messy/runs.jsonl \
  --store store/messy.rl
