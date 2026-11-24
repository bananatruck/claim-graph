# Demo script (about 100 seconds)

Run from the repository root after `cargo build --release`, with `alias cg=./target/release/claim-graph`.
`demo.sh` in this folder runs the same commands in order (it builds its store under `out/`, which git ignores), and `demo.typescript` with `demo.timing` is a recording of it (replay with `scriptreplay --timing submission/demo.timing submission/demo.typescript`).

## 0:00 The problem (10 s)

Say: "An AI research agent ran these experiments and wrote this draft. Which of its claims are actually backed by something it ran or read?"
Show `data/synthetic/draft.md` and `data/synthetic/runs.jsonl` side by side, pointing at the SYNTHETIC label.

## 0:10 One file, one transaction (15 s)

Run: `mkdir -p out && cg ingest --replace --draft data/synthetic/draft.md --runs data/synthetic/runs.jsonl --store out/claim-graph.rl`
Say: "The draft and the run log become one Relate graph in a single `.rl` file: claims, sources, experiments, runs, metrics, and every number in the text."
Point at the warning: the draft cites a paper its own bibliography does not list.

## 0:25 Unsupported claims (15 s)

Run: `cg unsupported --store out/claim-graph.rl`
Say: "Four of seven claims have no path to a completed run or a listed source."

## 0:40 Why, as paths (15 s)

Run: `cg explain --store out/claim-graph.rl --claim C4`
Say: "C4 points at experiment E2, which has no runs."
Run: `cg explain --store out/claim-graph.rl --claim C2`
Say: "C2 is supported through E1 to three completed runs, and its 6.1 points is the difference between runs r-002 and r-001."

## 0:55 Where did this number come from (15 s)

Run: `cg trace --store out/claim-graph.rl --figure "71.3%"`
Say: "71.3% comes from run r-002, which the claim cites; the graph also shows the other seed scored 0.689."

## 1:10 A messy draft (20 s)

Run: `cg audit --store store/messy.rl`
Say: "On a messier draft the same graph finds a citation missing from the bibliography on a claim that another source supports, a claim resting on a pilot that two later experiments superseded, following the chain to the newest results, and an abstract that says 74.2% while the run it cites produced 73.9%, the value the results section states."

## 1:30 Explore it yourself (10 s)

Run: `cg query --store store/messy.rl "MATCH p = (:Experiment)-[:SUPERSEDES*]->(:Experiment) RETURN [n IN nodes(p) | n.id] AS chain"`
Say: "Every answer is a Cypher query on the store in this repository. Clone it and ask your own."
