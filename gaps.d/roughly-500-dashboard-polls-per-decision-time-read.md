- **Roughly 500 dashboard polls per decision-time read — SURFACED, not fixed.**
  Lifetime telemetry: `graph_stats` 16,522 calls and `telemetry_snapshot`
  12,567, against 66 reads that could change a decision. `graph_stats` alone has
  spent 3,405 seconds — 57 minutes of cumulative compute — answering "how many
  nodes are there". The caller is the extension's polling loop, not an agent.
  Impact: wasted compute and a telemetry record whose shape is dominated by
  self-observation, which is what made the retrieval gap hard to see in the
  first place. Fix is a debounce or a push model in the extension; not attempted
  here because it is a separate change in a separate plane.
