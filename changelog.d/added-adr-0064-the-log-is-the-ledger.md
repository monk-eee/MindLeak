- **ADR-0064 records that the task lifecycle becomes an append-only log, with
  `tasks` as its projection.** The schema had already improvised this primitive
  three times: `claim_lapses` and `unleased_seconds` are aggregates of events
  nobody wrote down, `task_claim_transfers` is a single-verb log with a
  hand-written before-image, and `conformance` is append-only already. The cost
  showed up on 2026-07-29 — diagnosing board growth across 220 tasks produced a
  **wrong** first answer, reading 29 expired-lease tasks as abandoned when four
  agents were actively working them; a sweep on that reading would have stripped
  live work from all four. The same gap makes ADR-0048's `needs_human` cap fire
  on healthy work, because two integers cannot tell "lapsed while idle" from
  "lapsed mid-build with commits landing in the hole", and the 300-second default
  lease is shorter than `cargo test --all`.
  Decision only; no behaviour changes in this commit. Per ADR-0063 the migration
  never rebuilds `tasks` destructively — live claims are not ours to touch — and
  imports each existing task as a genesis event that honestly declares it carries
  no prior history. Verdict recomputation and forking are explicitly deferred,
  and this does **not** shrink the board: that growth is agent fan-out (69
  created against 36 closed in a day), which the log makes legible, not smaller.
