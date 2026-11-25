#!/usr/bin/env bash
# Runs the demo commands in order, for recording. Run from the repository root.
# `cargo run` builds and runs the current code wherever CARGO_TARGET_DIR points.
# The pauses give a viewer time to read.
# The demo builds its own store under out/, so the committed stores are left as they are.
set -euo pipefail

cg=(cargo run --release --locked --quiet --)
pause=${PAUSE:-3}
mkdir -p out

step() {
  # Print the command so it can be copied: plain words as they are, anything else in
  # single quotes.
  printf '\n$ claim-graph'
  for arg in "$@"; do
    if [[ $arg =~ ^[A-Za-z0-9_./:%=-]+$ ]]; then
      printf ' %s' "$arg"
    else
      printf " '%s'" "${arg//\'/\'\\\'\'}"
    fi
  done
  printf '\n'
  "${cg[@]}" "$@"
  sleep "$pause"
}

step ingest --replace --draft data/synthetic/draft.md --runs data/synthetic/runs.jsonl --store out/claim-graph.rl
step unsupported --store out/claim-graph.rl
step explain --store out/claim-graph.rl --claim C4
step explain --store out/claim-graph.rl --claim C2
step trace --store out/claim-graph.rl --figure "71.3%"
step audit --store store/messy.rl
step query --store store/messy.rl "MATCH p = (:Experiment)-[:SUPERSEDES*]->(:Experiment) RETURN [n IN nodes(p) | n.id] AS chain"
