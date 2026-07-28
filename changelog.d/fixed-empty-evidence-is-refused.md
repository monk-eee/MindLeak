- **`evidence_for` refuses an empty window instead of returning nothing as
  evidence.** Measured on the board: forty audits carried *"evidence contains
  no provenance-bearing mutation"*, and sixteen of them were raised **after**
  the argument guard that was supposed to have fixed that cause — by two
  different agents. Every one of those bundles was completely empty: no
  commits, no changed nodes, no executions, no provenance. The misspelt
  argument was *a* cause, not *the* cause; the dominant one is asking for
  evidence over a window nothing was ingested into, receiving a well-formed
  envelope containing nothing, and submitting it. Conformance then records
  `needs_human`, which reads as "a human must judge this" when in fact nobody
  can — the work was never recorded, and no amount of adjudication will
  conjure it. The call now fails with the window, the agent, and the remedy
  (`ingest_commit` with `changed_files`, or `ingest_execution`) rather than
  succeeding emptily. A window that caught real work is unaffected.
