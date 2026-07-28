- **The Memory Plane refuses an argument it does not declare, instead of
  dropping it.** The Intent Plane gained this guard when a misspelt
  `lease_seconds` produced a silently defaulted lease; the Memory Plane went
  without it, and the cost was concrete. `ingest_commit` takes `changed_files`; an agent passed `files`; the argument was dropped in silence.
  No `refactored` edges were written, so `evidence_for` counted zero commits, so
  conformance reported "no provenance-bearing mutation", so `complete_task`
  returned `needs_human` and the task never reached `done` — thirteen claims sat
  lapsed-but-still-held on the work board, and nothing in the symptom pointed
  within a mile of the typo. The same mistake on the Intent Plane is caught in
  seconds, because it names the argument, names what the tool actually accepts,
  and says that a misspelt argument is dropped rather than defaulted. Envelope
  keys the server injects, and the `session_id` every client sends on every
  call, are not treated as the caller's mistake.
