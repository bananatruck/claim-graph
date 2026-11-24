#!/usr/bin/env bash
# Runs the demo commands in order, for recording. Run from the repository root
# after `cargo build --release`. The pauses give a viewer time to read.
set -euo pipefail

cg=./target/release/claim-graph
pause=${PAUSE:-3}

step() {
  printf '\n$ claim-graph %s\n' "$*"
  "$cg" "$@" || true
  sleep "$pause"
}

step ingest --replace --draft data/synthetic/draft.md --runs data/synthetic/runs.jsonl --store store/claim-graph.rl
step unsupported --store store/claim-graph.rl
step explain --store store/claim-graph.rl --claim C4
step explain --store store/claim-graph.rl --claim C2
step trace --store store/claim-graph.rl --figure "71.3%"
step audit --store store/messy.rl
step query --store store/messy.rl "MATCH p = (:Experiment)-[:SUPERSEDES*]->(:Experiment) RETURN [n IN nodes(p) | n.id] AS chain"
