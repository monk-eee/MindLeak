- **One commit split into two intent nodes when ingested by an abbreviated
  sha.** — `ingest::git::ingest_commit` built the node id from the sha exactly
  as supplied, so a commit already ingested under its full hash gained a
  *second* node when ingested again by its abbreviation. Observed 2026-07-29:
  an evidence bundle carried both `intent:007835a` and
  `intent:007835a1c979…` for one commit, with duplicated `refactored` edges to
  all four artefacts and `commits=2`. — Medium impact: inflated commit counts in
  conformance evidence, duplicated provenance, and two nodes competing to
  represent one event, with nothing downstream able to tell they are the same
  commit. The commit-level twin of the "one file is one node" defect. — **Fixed
  Jul 2026:** an abbreviation is now refused with
  `MindLeakError::InvalidArgument` naming the fix, and case is normalised;
  ingestion cannot expand an abbreviation itself because it never shells out to
  git (invariant 1). Regression tests
  `an_abbreviated_sha_is_refused_rather_than_creating_a_second_node` and
  `sha_case_does_not_fork_the_commit_into_two_nodes` (Lodestar task
  `task:3767516939a0`).
