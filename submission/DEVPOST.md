# Devpost submission text: Claim Graph

Draft for Kesh to review, edit and paste into the Devpost form.
Lines marked TODO need Kesh.

## Project name

Claim Graph

## Tagline

Find the claims in a research agent's draft that rest on nothing, on a superseded experiment, or on a number the cited run never produced.

## Links

- Repository: TODO (public GitHub repository, tagged with the topics `relate` and `relate-neo4j-hackathon`)
- Demo video: TODO (1 to 2 minutes, viewable without a login)

## Inspiration

AI research agents can now run experiments and write the paper about them, and their prose reads well whether or not anything stands behind it.
A sentence can cite an experiment that never ran, a run that crashed, a reference missing from the bibliography, or a pilot that a later experiment replaced.
An abstract can say 74.2% while the run it cites produced 73.9%.
Catching this by hand means cross-reading the draft, the bibliography and the run log line by line, which is exactly the kind of connected question a graph answers well.

## What it does

Claim Graph is a command-line tool that reads a Markdown draft and a JSON Lines run log into a single-file Relate graph and checks every claim against it.

- `unsupported` lists the claims with no path to a completed run or a source the bibliography lists.
- `explain` shows, for one claim, each reference it makes, why that reference does or does not support it, and every supporting path.
- `trace` walks a number in the text back to the metric, run and experiment that produced it, including a number that is the difference of two runs ("6.1 points above the baseline"), and shows what the experiment's other runs measured.
- `audit` adds two checks: claims resting on an experiment that a chain of later experiments superseded, and numbers that the runs a claim cites did not produce, naming the other claims that state the right value.
- `query` runs any read-only Cypher on the store, and `dump` prints the whole graph as sorted text.
- `--fail` turns findings into exit status 3, so a research pipeline can refuse to ship a draft with unsupported claims.

## How we built it

Claim Graph is written in Rust and embeds Relate through its Rust crate (version 1.2.0 from crates.io).
The store is a single `.rl` file: the graph is rebuilt from two input files by one process, and one file is the easiest artifact for anyone to copy and inspect.
Ingest writes the whole graph in one Relate transaction, and the matching of numbers to metrics runs as Cypher inside that transaction.
Every answer is a Cypher query on the reopened store, opened read-only.
A reference to something the inputs do not define still becomes a node (a cited key with no bibliography entry, or a run with `logged: false`), so the gaps are facts in the graph and the same queries find them.
Parsing is deterministic: a sentence is checked when it holds a number, a comparison word, a citation or a run or experiment link, and other sentences are counted as prose.
The repository includes two synthetic datasets, written by hand and labelled as synthetic, and the stores built from them.

## Relate capabilities used

- Embedded Rust API (`Database::open`, `open_read_only`, `connect`, `query_with_params`).
- Single-file `.rl` store with the JSON snapshot codec.
- Explicit transactions (`begin`, `commit`, `rollback`) and an explicit checkpoint.
- Cypher: `UNWIND` batches with parameters, `MERGE ... ON CREATE SET`, `EXISTS`, `COUNT` and `COLLECT` subqueries, variable-length paths (`*1..2` and unbounded `SUPERSEDES*1..`), path functions, `labels` and `properties`.

## Third-party tools

- Rust crates: `clap`, `serde`, `serde_json`; for tests, `tempfile` and `relate-storage`.
- GitHub Actions for CI, with the `Swatinem/rust-cache` action.

## AI tools

Claim Graph was built by AI coding agents directed by Kesh, and reviewed by independent automated reviewers.
TODO (Kesh): if the form asks for tools by name, list them here.

## Challenges

- Making the graph decide what is missing, instead of resolving references in application code first: dangling references became nodes without the relationship that would make them count.
- Matching numbers honestly: a figure matches a metric only within the precision it was written with, so 71.3% matches 0.7128 but not 0.714, and bare integers are treated as counts.
- Relate's `.rl` bytes change on every rebuild, so the committed stores are checked by content: the tests rebuild them and compare a canonical dump.

## What's next

- Read LaTeX drafts and common experiment-tracker exports.
- Follow claims that depend on other claims, so one broken run marks everything built on it.
- An MCP view, through `relate serve`, so the agent that wrote the draft can query its own claims before it hands the draft over.

## Data

All data in the repository is synthetic and written by hand; no experiment in it was run and no cited work exists.
