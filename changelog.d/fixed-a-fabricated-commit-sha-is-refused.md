### Fixed

- `ingest_commit` now refuses a commit hash that is well formed but names no
  commit in the checkout it serves.

  The existing check only tested the *shape* — forty (or sixty-four) hex
  characters — and a fabricated hash is well formed by construction. On
  2026-07-29 an agent holding the abbreviation `7b17243` composed the remaining
  thirty-three characters; the resulting `intent:` node carried real
  `refactored` edges to real files while naming a commit that has never
  existed. Nothing removes a node once written, so a phantom stands until it
  decays, and afterwards it is indistinguishable from real provenance.

  Only git can tell a plausible object name from a real one, and the memory
  plane does not spawn git (invariant 1). The capability is therefore injected
  — `MindLeak::with_commit_resolver`, mirroring `with_worktree_refresh` — and
  supplied by `mindleak-mcp` from the new `mindleak_storage::commit_exists`.

  An unreachable git answers "unknown", never "no": a missing tool must not be
  able to refuse a commit that is perfectly real, so an unanswerable check
  degrades to exactly the previous behaviour.
