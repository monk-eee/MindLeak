- **`silent-knowledge` reports the recorded lessons that can never be read.**
  The conformance advisory matches recorded knowledge on referenced nodes and
  nothing else, so a record whose evidence carries no `nodes` array is stored,
  counted, decayed on schedule, and can reach nobody. Nothing measured that.
  `active_knowledge` reports `surfaces` per record, but only for whatever
  filter you happened to ask for — reading it as a repository-wide number
  required already suspecting the problem and then constructing the query,
  which is why an early spot check of a filtered subset read as "3 of 17" and
  the real figure went unnoticed. Measured across the whole ledger: **65 of 153
  records, 42%, cannot be read.** Among them are the lessons most worth having
  — that testing a facade method proves the logic and says nothing about the
  wiring, which is precisely how `merge_evidence` shipped refusing every
  caller; that a guard asserting over a retired name silently stops guarding;
  and one recording the cost of skipping the mandatory ADR-0029 pre-flight,
  which is exactly the mistake it could not warn anybody about. The audit ranks
  by weight and confirmation so the list is workable rather than a heap, takes
  `--top N`, and `--check` exits non-zero for a hook or a pipeline. It only
  reports: knowledge is append-only and nothing attaches nodes retrospectively,
  so the repair is to re-record the content with an evidence `nodes` array
  after re-verifying that it is still true. Copying a stale claim forward would
  be worse than leaving it silent, which is why this does not attempt to do it
  automatically.
