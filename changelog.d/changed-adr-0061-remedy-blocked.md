- **ADR-0061's merge queue is not available for this repository, and the ADR now
  says so.** GitHub's merge queue requires an organization-owned repository;
  this one is owned by a user account, so the "Require merge queue" checkbox is
  absent from branch protection rather than merely unticked, and no REST or
  GraphQL field exists behind it. The measurement that motivated the ADR stands —
  65% of CI in twenty-four hours spent re-running unchanged code — but the
  remedy is out of reach, so the status is now `Accepted (remedy blocked)` and
  the three genuinely available options are recorded: move the repository to an
  organisation, accept the churn, or reduce contention by arming fewer branches
  at once. Attempting the change also proved the ADR's own warning that its two
  halves must move together: unticking "require branches to be up to date"
  succeeded while ticking the queue was impossible, leaving `main` briefly able
  to accept two individually-green branches that break it together. The
  protection was restored with required checks unchanged. `merge_group` stays in
  `ci.yml` — inert without a queue, and it makes the organisation option a
  single settings change rather than a prerequisite to rediscover.
