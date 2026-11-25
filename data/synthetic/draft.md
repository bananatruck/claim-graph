# Retrieval-augmented planning for a toy research agent

> SYNTHETIC TEST DATA. This draft, its references and its run log were written by hand for Claim Graph's tests and demo.
> No experiment described here was run, and the cited works do not exist.

## Abstract

We study a planner for a toy research agent.
Our planner reaches 71.3% task success on the held-out split [run:r-002].
This is 6.1 points above the strongest baseline [exp:E1].
The planner also halves the number of tool calls per task.

## Results

Retrieval helps most on long tasks [exp:E2].
Prior agents plateau below 60% on this benchmark [@lee2025].
Ablating the critic lowers success to 65.0% [run:r-004].
The gains match those reported for larger planners [@okafor2023].

## References

- [@lee2025] Lee and Park. A synthetic benchmark for planning agents. Workshop on Imaginary Results, 2025.
- [@chen2024] Chen. Tool use in toy agents. Proceedings of Placeholder Studies, 2024.
