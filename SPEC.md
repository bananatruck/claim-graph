# Claim Graph: specification

## The idea in one sentence

Claim Graph reads a research agent's paper draft and its run log into a single-file Relate graph and reports every checkable claim that has no path to a completed run or a listed source, every reference that fails on an otherwise supported claim, every claim that rests on a superseded experiment, and every measured figure (a percentage, a decimal or a number with an exponent) in a claim resting on completed runs that those runs did not produce.
Bare integers, numbers in claims supported only by citations, and sentences the claim rule leaves as prose are not checked against metrics.

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
- Every answer is a Cypher query against the reopened store, opened read-only: which references resolve, whether each supports its claim and why, and which numbers match are decided in Cypher.
  Rust validates the input files before ingest, and sorts and formats query results.
- All values reach Cypher as query parameters, so draft text never becomes part of a statement.

## Inputs

All inputs in this repository are synthetic and say so in their first lines.
There are two datasets: `data/synthetic/` (small and tidy) and `data/synthetic/messy/` (abbreviations, lists, a table, code, rounded and conflicting figures, a superseded experiment, a run the log never recorded, and an out-of-order run log with extra fields).

### Draft (Markdown)

- The first `# ` heading is the draft title; before any section it is only the title, so sentences before the first section have an empty section name.
- A `## ` heading starts a section; the section name is stored on each sentence.
- Lines starting with `>` (notes), `|` (table rows) and `![` (images), and deeper headings (three to six `#` followed by a space or the end of the line) are skipped; other text that starts with `#`, such as `#1 reached 74.2%` or a tab after the `#` run (`##\tResults`), is read as a sentence.
- A line of three or more backticks opens a code fence, closed by a line of at least as many backticks and nothing else; a line that also closes its backticks is inline code, not a fence.
- A line starting with `$$` opens a math block, closed by a line that starts or ends with `$$`; `$$ ... $$` on one line is removed and the rest of the line kept.
- A fence or math block still open at the end of the draft is a parse error naming the line that opened it.
- A number whose value or written precision a 64-bit float cannot hold (`1e-999`, `1e999`) is a parse error naming its line, because as a float it would become 0 or infinity and could match a metric it does not equal.
- A list of numbers written without spaces that reads more than one way (`1,234,5`) is a parse error naming its line (see Figures).
- Any other `# ` heading, or the title when a section has already started, starts a new section and ends the References section.
- A paragraph is a run of non-blank lines; each list item (`- `, `* `, `1. `, `1) `) is its own paragraph.
- A sentence ends at `.`, `!` or `?`, plus any closing brackets or quotes right after it, followed by whitespace or the end of the paragraph, so `71.3` does not split a sentence and `.)` ends one.
- Punctuation inside a reference marker never ends a sentence.
- A period after `e.g.`, `i.e.`, `Fig.`, `Figs.`, `Sec.`, `Eq.`, `Eqs.`, `Tab.`, `vs.`, `cf.` or `approx.` does not end a sentence; nor does `al.` after `et`, or `No.` before a number.
- `[@key]`, `[@a; @b]` or `[@a, @b]` cites sources.
- `[run:ID]` links a sentence to a run, and `[exp:ID]` links it to an experiment.
- Every non-blank line in the `References` section must be `- [@key] entry text`; anything else is a parse error with its line number.

### The claim rule

A sentence is a checkable claim when it contains a number, a comparison word, a citation, or a run or experiment link.
Every other sentence is prose: it is stored and counted, listed with `--prose`, and never reported as unsupported.
Claims are numbered `C1`, `C2`, ... and prose `P1`, `P2`, ..., each in reading order.
The comparison words are a fixed list in `src/classify.rs`: comparatives (`better`, `higher`, `than`), change verbs and their inflections (`outperforms`, `halved`, `dropped`), and result verbs (`reaches`, `achieved`, `confirms`), matched as whole words, where digits and underscores belong to the word (`Match3` is not `match`).
The rule sits behind one function, `classify`, so it can be replaced without touching the parser or the store.
It errs towards checking: a sentence such as "Fig. 3 shows the error rate" counts as a claim because it holds a number.

### Figures

A figure is a number in a claim, outside reference markers.
The text is read as whole words, and the grammar in `src/figures.rs` decides whether a word is a figure, a range of two figures, a comma list written without spaces, or an identifier holding none.
So `r2`, `v1.2.3`, `1.2.3`, `gpt-3.5-turbo`, `UTF-8`, `x86-64`, `Llama-3.1-8B` and `3rd` are not figures.
In a list written without spaces, each comma is a list comma or a thousands separator, and an item does not start with `0` and another digit.
The one choice that makes every item read is the list: `1,23` is 1 and 23, `1,024.5,2.5` is 1,024.5 and 2.5, and `1.5-2,000.5,3.5` is 1.5, 2,000.5 and 3.5.
A word that reads as one figure or a range is never a list, so `1,500` is 1,500 and `0.5-1,500` is 0.5 and 1,500.
A list with more than one such choice, such as `1,234,5` (1,234 and 5, or 1, 234 and 5), is a parse error naming its line, so no reading is picked that might match a metric; a list with none is an identifier.
A leading minus sign (`-0.25`, `−3.1%`) belongs to the figure; a dash between two figures (`10-20`, `71.3%-73.9%`) is a range.
A leading dot (`.713`) and an exponent (`1.0e-3`) are part of the number; the exponent scales both the value and the written precision.
A unit suffix of up to three letters (`12.5ms`, `10k`, `3x`) is kept as written but does not rescale the number.
`71.3%` and "71.3 points" (or `point`, `pp`, as a whole word, so `3 pp_score` is a count) are on the percent scale: stored as 0.713, and as written, 71.3.
Each figure carries the precision it was written with: half a unit in its last digit.
Value and precision must be finite, and the precision at least the smallest normal 64-bit float (about 2.2e-308) on both scales; otherwise the draft is refused, as above.
Only percentages and numbers written with a decimal point or an exponent (`3e0` included) count as measurements; bare integers are treated as counts ("3 seeds", "2024") and never matched to metrics.

### Run log (JSON Lines)

One JSON object per line, with a `kind` field:

- `{"kind":"experiment","id":"E12","name":"...","supersedes":"E11"}`, where `supersedes` is optional.
- `{"kind":"run","id":"m-07","experiment":"E12","seed":1,"status":"completed"}`, where `seed` is optional.
- `{"kind":"metric","run":"m-07","name":"accuracy","value":0.739}`
- `{"kind":"note","text":"..."}`, which is ignored and lets a log carry its own synthetic-data label.

Records may come in any order, and extra fields such as `timestamp` or `git_sha` are ignored.
IDs must be unique and use only the characters a draft reference can (letters, digits, `-`, `_`, `.`, `:`); every run must name a defined experiment, every metric a defined run, and `supersedes` another defined experiment, with no cycle.
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
| `Derivation` | `op`, `minuend`, `subtrahend` | A figure computed from two metrics; `op` is `difference`, and the operands are named as run and metric |

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
A measured figure `MATCHES` a metric when the metric is within the figure's written tolerance of either the scaled value or the written number, so `71.3%` matches 0.7128, 0.713 and 71.3 but not 0.714, and a plain 6.0 matches only 6.0 within 0.05.
The comparison runs in 64-bit floats, so it allows for their roundoff and nothing more.
A stored value is off from its decimal by at most u times its magnitude (u = 2^-53, about 1.1e-16), and the subtraction adds at most as much again, so a computed difference is within about 2.2e-16 times the sum of the operands' magnitudes of the exact one.
The allowance is 1e-15 times each operand's magnitude, summed term by term so it cannot overflow near the largest float, and capped at a millionth of the written tolerance, so `0.30` matches 0.305 but not 0.3050001 and `9e307` matches 9.5e307 but not 9.5000001e307; a difference of two metrics uses the same rule over its three operands.
`MATCHES` is computed across the whole log, so `trace` can show a run the claim does not cite; only matches with completed runs the claim cites count as support.
A measured figure that no completed run the claim cites produced directly gets a `Derivation` when it equals, by the same rule, the difference between the same metric in two completed runs of an experiment the claim rests on, so "6.1 points" matches 0.713 minus 0.652.
A difference supports the claim only when the claim reaches both runs.

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
The full query in `src/queries.rs` also collects, for each unsupported claim, the references in it that point at nothing.

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

### Which references fail on a supported claim (`audit`)

The same query that `explain` uses decides, in Cypher, whether each `CITES` or `SUPPORTED_BY` reference supports its claim and why (not listed, not logged, a run that did not complete, an experiment with no completed run).
`audit` lists the failing references of claims that another reference supports, which the unsupported-claims check alone would hide.

### Which numbers did the cited runs not produce (`audit`)

For each measured figure in a claim that rests on at least one completed run, the query checks that a completed run the claim cites produced a metric that `MATCHES` the figure, or that the figure has a `Derivation` whose two runs are both completed and both reached by the claim.
When none is, it reports what those runs measured and which other claims state those values, which catches an abstract that says 74.2% while the results section says 73.9% for the same run.

## Command-line surface

- `claim-graph ingest --draft FILE --runs FILE --store FILE.rl [--replace]`
  The store is built in a temporary file beside `FILE.rl` and published only when complete. Without `--replace`, publication fails if `FILE.rl` exists by then, even when another ingest created it meanwhile; with `--replace`, the new store replaces the old one in one rename.
  Publishing without `--replace` uses a hard link, which fails atomically if `FILE.rl` exists, so `FILE.rl` must be on a file system that supports hard links; on one that does not, such as FAT or exFAT, ingest fails after building the store and leaves no store at `FILE.rl`.
  Choose a destination on a file system with hard links; `--replace` is not an equivalent workaround, because it permits overwriting an existing store.
  Ingest refuses a `--store` that is one of its own inputs, reached by any path, symbolic link or hard link, so `--replace` can never overwrite the draft or the run log.
- `claim-graph unsupported --store FILE.rl [--prose] [--fail] [--format text|json]`
- `claim-graph explain --store FILE.rl --claim C4 [--format text|json]`
- `claim-graph trace --store FILE.rl (--figure "71.3%" | --claim C1) [--format text|json]`, with exactly one of the two selectors.
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
- Bare integers are never matched to metrics, and unit suffixes do not rescale numbers.
- Differences are searched only between two completed runs of an experiment the claim rests on.
- Table rows are skipped, so a result stated only in a table is not checked.
- A claim citing several runs is checked against all of them together; which number belongs to which run is not parsed.
- Numbers in a claim supported only by citations are not checked.
- Number matching compares every figure with every metric; that is fine for one paper and its log.
- Claim Graph does not judge whether a cited source says what the claim says, or whether the numbers justify the wording.
- Ingest rebuilds a store; there are no incremental updates.
- No model calls and no network access at run time.

## Test plan

- Draft parser: title, sections, sentence splitting around decimals and abbreviations, notes, tables, images, code and math skipped, all list styles, all reference forms, figures outside markers only, malformed references with their line numbers.
- Claim rule: each reason on its own, plain prose, whole-word matching, numbers inside identifiers.
- Figures: one table row per grammar rule, plus percentages, points, decimals with and without a leading digit, exponents, integers, signs, ranges, thousands separators, multipliers, unit suffixes, ordinals, dotted and hyphenated identifiers.
- Run log: each kind, any order, extra fields, `supersedes`, cycles, IDs a draft cannot reference, duplicates, dangling references, invalid JSON.
- Store, on real `.rl` files reopened read-only: node and relationship counts, unsupported claims and prose, a report that changes when the graph is edited directly, `explain` reasons and paths, `trace` matches, differences and the no-source case, stale chains, mismatches, broken references on supported claims, each trigger input from the first review round, read-only `query`, refusal to overwrite, replacement without leftovers, the path rule, and a missing store.
- The committed stores match a rebuild by content, and are single-file bundles with the JSON codec.
- End to end on the built binary: every command, text and JSON output, `--fail` exit status, usage errors.
- CI runs `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test` and `cargo build --release`.

## Demo

The 1 to 2 minute script is `submission/demo-script.md`.
