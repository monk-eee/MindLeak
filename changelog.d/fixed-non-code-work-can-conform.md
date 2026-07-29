- Conformance no longer fails work whose product is not code (ADR-0060). Evidence
  that touches no code bound to the task goal now resolves to `aligned` with the
  fact recorded as a finding, matching the verdict the same evidence already got
  when no task was attached. Previously the presence of a task made the verdict
  worse, so a task delivering an ADR, documentation, a benchmark or a changelog
  fragment could never reach `aligned` and parked in `in_review` awaiting a human
  who had no queue to watch. Only a positive signal of a problem — drift, a
  `forbid_change` lock, missing provenance, or governed code changed without a
  covering task — may downgrade a verdict. An audit no longer claims "evidence
  covers task goal" when it did not.
