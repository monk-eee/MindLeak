- `ingest_commit` refuses an abbreviated commit hash instead of minting a second
  intent node for a commit already ingested under its full hash. The node id is
  derived from the hash, so `intent:007835a` and `intent:007835a1c979...` were
  two nodes competing to represent one event, inflating commit counts in
  conformance evidence and duplicating provenance edges. Pass all 40 (or 64) hex
  characters; the error names the fix. Hash case is normalised for the same
  reason. Ingestion cannot expand an abbreviation itself, because it never shells
  out to git.
- `MindLeakError::InvalidArgument` distinguishes a caller-supplied argument
  problem from the `Other` catch-all.
