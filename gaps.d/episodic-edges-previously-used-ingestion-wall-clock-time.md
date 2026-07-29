- **Episodic edges previously used ingestion wall-clock time.** — Delayed passive
  execution/commit ingestion could invert failure/change/success chronology and
  fabricate or hide consequence. — High impact on signal correctness. — Fixed
  this run: execution and commit edges now use authoritative record timestamps,
  with regression tests.
