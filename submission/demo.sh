#!/usr/bin/env bash
# Runs the demo commands in order, for recording. Run from the repository root
# after `cargo build --release`. The pauses give a viewer time to read.
# The demo builds its own store under out/, so the committed stores are left as they are.
set -euo pipefail

cg=./target/release/claim-graph
pause=${PAUSE:-3}
mkdir -p out

step() {
  printf '\n$ claim-graph'
  printf ' %q' "$@"
  printf '\n'
  "$cg" "$@"
  sleep "$pause"
}

step ingest --replace --draft data/synthetic/draft.md --runs data/synthetic/runs.jsonl --store out/claim-graph.rl
step unsupported --store out/claim-graph.rl
step explain --store out/claim-graph.rl --claim C4
step explain --store out/claim-graph.rl --claim C2
step trace --store out/claim-graph.rl --figure "71.3%"
step audit --store store/messy.rl
step query --store store/messy.rl "MATCH p = (:Experiment)-[:SUPERSEDES*]->(:Experiment) RETURN [n IN nodes(p) | n.id] AS chain"
