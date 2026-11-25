# Notebook agents that check their own arithmetic

> SYNTHETIC TEST DATA. This draft and its run log were written by hand for Claim Graph's tests and demo.
> It is messy on purpose: abbreviations, lists, a table, code, rounded and conflicting figures, a superseded experiment and a run the log never recorded.
> No experiment described here was run, and the cited works do not exist.

## Abstract

We present a notebook agent that re-derives every number it reports.
On the held-out notebooks it answers 74.2% of questions correctly [run:m-07].
That is an improvement of 4.4 points over the plain agent [exp:E12].

## Method

The agent reads a notebook, e.g. a lab log or a spreadsheet export, and plans its answer.

1. It extracts every table.
2. It recomputes each derived column.
3. It writes the answer with the recomputed values.

```python
accuracy = correct / total  # 0.5 here is code, not a claim.
```

| setting | accuracy |
|---|---|
| plain | 0.695 |

## Results

The final configuration answers 73.9% of questions correctly [run:m-07].
Tool calls fall to 4.2 per question on average [run:m-07].
Our earlier pilot reached 71.3% [run:m-01].
As Ito et al. note, recomputation helps most on long notebooks [@ito2024, @ndiaye2022].
A second seed confirms the result [run:m-99].
Fig. 3 shows the error rate by notebook length.
We thank the anonymous reviewers.

## References

- [@ito2024] Ito and Sato. Long notebooks and where to find them. Journal of Placeholder Results, 2024.
- [@ruiz2025] Ruiz. Agents that show their work. Proceedings of Imaginary Systems, 2025.
