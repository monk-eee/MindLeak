# ADR-0145: Recovery rehearsal is real; production recovery execution is single-tenant only

- Status: Accepted
- Date: 2026-08-30
- Deciders: MindLeak maintainers
- Accepted: 2026-08-30 by the repository owner, authorized directly in
  session — attributed human adoption after review.
- Refines: [ADR-0119](0119-industrial-administration-lifecycle-policy.md)
  decision 6
- Depends on:
  [ADR-0134](0134-enrolled-signing-keys-authenticate-lifecycle-purge-confirmations.md)
  (enrolled signing keys authenticate Lifecycle purge confirmations),
  [ADR-0142](0142-loopback-verified-principal-extends-to-work-design-and-constitution.md)
- Related:
  [ADR-0143](0143-postgres-access-goes-through-one-bounded-pool-per-process.md)

## Context

ADR-0119 decision 6 already states the policy this ADR has to satisfy:

> Recovery separates inspection, rehearsal, and execution. Inspection and a
> restore drill run against an isolated target and create durable reports;
> neither mutates the authoritative production database. A production
> recovery execution requires a previously identified artifact or
> point-in-time target, compatibility and integrity evidence, an explicit
> scoped impact plan, verified high-trust authorization, and a final receipt
> that names the old and new authority state.

Only the first third of that sentence has ever been built. `ackplane-bridge::
administration::recovery` exposes exactly two routes,
`inspect_snapshot`/`latest_snapshot_inspection`, both calling
`snapshot_provider::inspect_snapshot_artifact` — which checks a manifest
digest, whether the artifact decrypts, and whether `pg_restore --list` can
parse the archive. That is a format check against the artifact file. It never
opens a database, so it cannot answer the actual question a recovery drill
exists to answer: does this artifact *restore*, against the schema and
extensions this deployment's migrations have actually produced. Rehearsal —
"a restore drill run against an isolated target" — does not exist as code
anywhere in the workspace (`git grep` for `rehearsal`, `restore_drill`,
`execute_recovery` finds nothing outside this ADR's own commit). Production
recovery execution does not exist either: there is no route that calls
`pg_restore` against the authoritative `ACKPLANE_DATABASE_URL`, and
`SnapshotProviderError::RestoreSpawn` — the one enum variant that mentions
`pg_restore` starting — is reachable only from the archive-validation path
inside `inspect_snapshot_artifact`, not from any execution path.

The reason this was never built casually is visible in this repository's own
architecture notes, and it is a real constraint, not a missing feature:

> [Snapshot] is deliberately platform-scoped only today: Ackplane's schema is
> multi-tenant at the row level, so a `pg_dump` of the whole database is
> never a valid tenant-scoped artifact, and a true tenant-scoped export is
> separate, tracked follow-on work, not this provider relabeled.

The same fact applies to restore with the opposite, more dangerous direction:
a platform Snapshot artifact contains every tenant's rows, so *restoring* it
would overwrite every tenant's data, not just the requesting tenant's. A
recovery execution endpoint that did not confront this would let one tenant's
disaster-recovery request silently roll back every other tenant on the same
Ackplane deployment to an earlier point in time — a correctness and trust
failure far worse than the capability being merely absent. This is the actual
reason "the exact provider integrations... remain implementation work" in
ADR-0119's own consequences section: the mechanism was never the hard part;
the blast radius was.

ADR-0134 already solved the adjacent "verified high-trust authorization"
requirement for Lifecycle purge: a preview and a confirmation, each carrying a
domain-separated signature from an enrolled node's signing key, with
confirmation requiring public-key material distinct from the key that created
the preview. Recovery execution needs the identical property — proof that two
distinct verified credentials authorized an irreversible action — and nothing
about Recovery's threat model differs from Purge's in a way that would justify
a second, competing authorization mechanism.

## Decision

**Rehearsal is a new capability that always runs, produces a durable report,
and never touches production. Production recovery execution reuses ADR-0134's
dual-signing-key preview/confirmation pattern, requires a fresh passing
rehearsal of the exact same artifact, and is refused outright on any
deployment that is not confirmed single-tenant.**

1. **Rehearsal restores into an isolated, ephemeral target, never
   `ACKPLANE_DATABASE_URL`.** A new `snapshot_provider::rehearse_recovery`
   provisions a scratch Postgres database — a fresh, throwaway database
   created and dropped by the rehearsal itself, on the same server or a
   configured rehearsal-only connection string
   (`ACKPLANE_REHEARSAL_DATABASE_URL`), never the authoritative one — runs
   `pg_restore` against it for real, and then runs the same bounded set of
   application-level invariant checks the migration/readiness paths already
   know how to run: migration version matches `migrate_locked`'s expected
   head, and a table-count/row-count reconciliation against the manifest
   `pg_dump` itself recorded. It refuses to reuse `ACKPLANE_DATABASE_URL` as
   its own target under any configuration; a misconfigured rehearsal-only URL
   that resolves to the same database is a fatal, refused configuration, not
   a warning.
2. **A durable `RecoveryRehearsalReport` is the compatibility and integrity
   evidence decision 6 asks for.** It names the artifact's manifest digest,
   restore duration, migration-version match, table/row reconciliation
   result, and pass/fail — following `RecoveryInspection`'s own shape
   (`recovery_model.rs`): random, non-deterministic id, append-only, never
   deduplicated by idempotency key, because two rehearsals of the same
   artifact are legitimately distinct events, not replays.
3. **Production recovery execution requires a rehearsal report for the exact
   same artifact digest inside a bounded freshness window
   (`MAX_REHEARSAL_FRESHNESS`, proposed 24 hours — the same order of
   magnitude as `MAX_CONFIRMATION_WINDOW`).** A rehearsal from before the
   deployment's last migration proves nothing about compatibility with the
   schema production is running today; execution refuses (not merely warns)
   without a passing, fresh rehearsal of that identical digest. There is no
   execution path that accepts an inspected-only artifact — inspection alone
   was always insufficient for this decision, which is exactly what the
   original gap fragment and decision 6 both said.
4. **Execution reuses ADR-0134's preview/confirm signing contract verbatim,
   scoped to Recovery.** A `RecoveryPreviewRequest` carries the same
   domain-separated, node-signed, nonce-consumed shape `PurgePreviewRequest`
   already has (`purge_model.rs`): requesting node id, public key
   fingerprint, and a distinct `MAX_CONFIRMATION_WINDOW`-bounded expiry. A
   `RecoveryConfirmation` must carry a public-key fingerprint distinct from
   the previewing key, exactly as ADR-0134 decision 4 requires for purge —
   the same reasoning applies without modification: identity proof is
   necessary but not sufficient authorization of one exact destructive
   mutation, and a second distinct verified credential is what makes it one.
   No new authorization primitive is introduced.
5. **The preview carries an explicit impact plan, not a free-text
   description.** It names: the artifact id and manifest digest being
   restored; the current authoritative state's own digest, captured by
   triggering a fresh platform Snapshot as part of preview construction (a
   "one before" safety point, so a completed execution is itself always
   recoverable — this Snapshot is not optional and its failure fails the
   preview); the passing rehearsal report id being relied on; and an explicit
   `AdministrationScope::Platform` field, always platform, per decision 6.
   There is no per-tenant or per-repository impact plan variant, because
   decision 6 forbids the endpoint from claiming a narrower blast radius than
   the artifact actually has.
6. **Production execution is refused outright unless the deployment is
   attested single-tenant.** A Bridge instance's Snapshot provider
   configuration gains a required `single_tenant_attested: bool` field,
   defaulting to `false`. Recovery execution refuses with a typed
   `MultiTenantRecoveryUnavailable` error whenever it is `false` — which is
   every deployment until an operator explicitly sets it, and it is never
   inferred from the number of tenant rows observed, because a currently
   single-tenant platform can still onboard a second tenant tomorrow and this
   is a durable configuration decision, not a runtime headcount. This is the
   same shape as decision 6's own admission that "the exact provider
   integrations... remain implementation work": a genuinely safe multi-tenant
   restore needs a distinct, larger capability (selectively restoring one
   tenant's rows out of a whole-database dump, or a tenant-scoped export
   format from the still-not-built tenant-scoped Export) that this ADR does
   not design and does not pretend to design. Recovery execution ships only
   for the deployment shape where "restore everything" and "restore this
   tenant" are the same operation — matching exactly how ADR-0128 scoped its
   verified loopback principal to self-hosted single-tenant deployments.
7. **The receipt names the old and new authority state, not a caller's
   description of them.** `RecoveryExecutionReceipt` records: the pre-restore
   safety Snapshot's manifest digest (the "old" state), the restored
   artifact's manifest digest (the "new" state), the rehearsal report id
   relied on, the verified previewing and confirming node ids and key
   fingerprints, and outcome (`Succeeded`/`Failed`/`Refused`) — following
   `PurgeReceipt`'s `Expired`-vs-`Refused` distinction (a confirmation that
   arrived too late is not the same fact as one a disabled multi-tenant
   deployment blocked outright).
8. **A failed production restore does not retry with looser checks and does
   not leave the database partially restored without saying so.** Per ADR-0119
   decision 10, a failed `pg_restore` against production is a `Failed`
   receipt naming the failure, never a silent fallback to the pre-restore
   safety Snapshot (that reversal is its own explicit, separately confirmed
   recovery execution against the safety artifact — using the same
   two-key-confirmed workflow this ADR defines, not an automatic action) and
   never a retry with rehearsal or freshness checks relaxed.

## Consequences

- Rehearsal ships first and independently: it never touches production, needs
  no dual-signing-key authorization (decision 6 only requires that for
  execution), and is useful on every deployment shape, single-tenant or not —
  it is real evidence about whether backups actually work, which today this
  repository cannot produce at all.
- Production recovery execution ships only behind `single_tenant_attested`,
  so most deployments gain rehearsal but not execution from this ADR alone.
  That is an intentional, named limitation (ADR-0138), not a deferred gap:
  multi-tenant recovery execution needs a distinct, larger design (per-tenant
  selective restore) this ADR does not attempt.
- An implementing agent's natural slice order: (1) rehearsal against an
  ephemeral target and its report type, provable independently with its own
  integration test; (2) the preview/confirm wire contract, reusing
  ADR-0134's signing helper directly rather than a Recovery-specific copy;
  (3) `single_tenant_attested` configuration and its refusal; (4) the
  execution path itself, gated on all of the above.
- `docs/ARCHITECTURE.md`'s Recovery paragraph needs a follow-up update once
  any of this ships — not part of this ADR, which is design-only.

## Rejected alternatives

**Invent a new signing/confirmation mechanism for Recovery specifically.**
Rejected: ADR-0134 already solved the identical problem (prove two distinct
verified credentials approved one irreversible action) for Purge, and
Recovery's threat model is not different enough to justify a second
mechanism a reviewer would have to learn and audit separately.

**Ship production execution for every deployment and rely on the impact
preview to warn about multi-tenancy.** Rejected: a warning a caller can
dismiss is not a safeguard against overwriting other tenants' data — decision
6 requires this class of action to be gated on verified authorization and an
explicit scope, not on an operator reading a warning correctly under
pressure during an actual incident.

**Skip rehearsal and gate execution on inspection alone.** Rejected: this is
exactly the gap the original fragment identified. `pg_restore --list`
proves the archive is well-formed; it does not prove the schema it targets
still matches the artifact, which is precisely the failure mode a real
incident is likely to expose.

**Attempt a per-tenant selective restore now instead of refusing multi-tenant
execution.** Rejected as out of scope for this ADR: selectively restoring one
tenant's rows out of a whole-database `pg_dump` (or from a not-yet-built
tenant-scoped Export artifact) is a distinct, larger capability with its own
integrity questions (foreign keys across tenant boundaries, sequence/id
collisions with data written after the snapshot). Refusing outright is honest
about what exists today; building it silently narrower than it actually is
would not be.
