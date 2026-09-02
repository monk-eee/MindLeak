//! Every schema migration first takes the global lock, then its own file key.
//! The global lock prevents deadlocks between different DDL files that touch
//! related tables; the file key keeps the migration identity explicit for
//! diagnostics and preserves the established migration-number convention.

pub(crate) const GLOBAL_SCHEMA: i64 = 0;
/// `migrations/0001_ledger.sql`
pub(crate) const LEDGER: i64 = 1;
/// `migrations/0002_projection.sql`
pub(crate) const PROJECTION: i64 = 2;
/// `migrations/0003_enrollment.sql`
pub(crate) const ENROLLMENT: i64 = 3;
/// `migrations/0004_signing_keys.sql`
pub(crate) const SIGNING_KEYS: i64 = 4;
/// `migrations/0005_claim_delegation.sql`
pub(crate) const CLAIM_DELEGATION: i64 = 5;
/// `migrations/0006_claim_authentication_nonces.sql`
pub(crate) const CLAIM_AUTHENTICATION_NONCES: i64 = 6;
/// `migrations/0007_knowledge.sql`
pub(crate) const KNOWLEDGE: i64 = 7;
/// `migrations/0008_knowledge_authentication_nonces.sql`
pub(crate) const KNOWLEDGE_AUTHENTICATION_NONCES: i64 = 8;
/// `migrations/0009_constitution.sql`
pub(crate) const CONSTITUTION: i64 = 9;
/// `migrations/0010_constitution_authentication_nonces.sql`
pub(crate) const CONSTITUTION_AUTHENTICATION_NONCES: i64 = 10;
/// `migrations/0011_knowledge_recorded_by.sql`
pub(crate) const KNOWLEDGE_RECORDED_BY: i64 = 11;
/// `migrations/0012_knowledge_reconfirmations.sql`
pub(crate) const KNOWLEDGE_RECONFIRMATIONS: i64 = 12;
/// `migrations/0013_knowledge_reach.sql`
pub(crate) const KNOWLEDGE_REACH: i64 = 13;
/// `migrations/0014_evidence.sql`
pub(crate) const EVIDENCE: i64 = 14;
/// `migrations/0015_evidence_conformance.sql`
pub(crate) const EVIDENCE_CONFORMANCE: i64 = 15;
/// `migrations/0016_telemetry.sql`
pub(crate) const TELEMETRY: i64 = 16;
/// `migrations/0017_telemetry_authentication_nonces.sql`
pub(crate) const TELEMETRY_AUTHENTICATION_NONCES: i64 = 17;
/// `migrations/0018_evidence_review_filter.sql`
pub(crate) const EVIDENCE_REVIEW_FILTER: i64 = 18;
/// `migrations/0020_context_packets.sql`
pub(crate) const CONTEXT_PACKETS: i64 = 20;
/// `migrations/0022_human_delegation.sql`
pub(crate) const HUMAN_DELEGATION: i64 = 22;
/// `migrations/0023_human_delegation_event_payloads.sql`
pub(crate) const HUMAN_DELEGATION_EVENT_PAYLOADS: i64 = 23;
/// `migrations/0024_supervisor_session_projection.sql`
pub(crate) const SUPERVISOR_SESSION_PROJECTION: i64 = 24;
/// `migrations/0025_enrollment_status_authentication_nonces.sql`
pub(crate) const ENROLLMENT_STATUS_AUTHENTICATION_NONCES: i64 = 25;
/// `migrations/0026_constitution_publication_history.sql`
pub(crate) const CONSTITUTION_PUBLICATION_HISTORY: i64 = 26;
/// `migrations/0027_industrial_designs.sql`
pub(crate) const INDUSTRIAL_DESIGNS: i64 = 27;
/// `migrations/0028_work.sql`. Key 27 was deliberately skipped while
/// this was drafted: the shared development database already had it
/// recorded as applied by concurrent, then-not-yet-committed work
/// (observed against `ackplane_schema_migrations` while investigating
/// ADR-0120's own migration-identity gap) -- it landed as
/// `INDUSTRIAL_DESIGNS` above.
pub(crate) const WORK: i64 = 28;
/// `migrations/0029_live_feed.sql`. Renumbered from 27 to 29: 27 was
/// this file's own next-available slot when drafted, but collided with
/// `INDUSTRIAL_DESIGNS`/`WORK` (27/28) landing concurrently.
pub(crate) const LIVE_FEED: i64 = 29;
/// `migrations/0030_directives.sql`
pub(crate) const DIRECTIVES: i64 = 30;
/// `migrations/0031_industrial_design_work_reference.sql`. Key 30 was
/// already recorded as applied in the shared development database by
/// concurrent, then-not-yet-committed work at the time this was drafted.
pub(crate) const INDUSTRIAL_DESIGN_WORK_REFERENCE: i64 = 31;
/// `migrations/0032_industrial_design_materializations.sql`
pub(crate) const INDUSTRIAL_DESIGN_MATERIALIZATIONS: i64 = 32;
/// `migrations/0033_knowledge_active_page_index.sql`
pub(crate) const KNOWLEDGE_ACTIVE_PAGE_INDEX: i64 = 33;
/// `migrations/0034_knowledge_lifecycle.sql`
pub(crate) const KNOWLEDGE_LIFECYCLE: i64 = 34;
/// `migrations/0035_knowledge_supersession_and_evidence.sql`
pub(crate) const KNOWLEDGE_SUPERSESSION_AND_EVIDENCE: i64 = 35;
/// `migrations/0036_knowledge_revalidation_policy.sql`
pub(crate) const KNOWLEDGE_REVALIDATION_POLICY: i64 = 36;
/// `migrations/0037_work_commands.sql`
pub(crate) const WORK_COMMANDS: i64 = 37;
/// `migrations/0038_constitution_proposals.sql`. 36 and 37 were live in
/// the shared development database and/or a concurrent unmerged branch
/// (`feat/work-command-ledger`, PR #733, `0037_work_commands.sql`) at
/// the time this was drafted -- checked via `migration-audit.mjs` and a
/// direct `gh pr view` before picking this number, not guessed.
pub(crate) const CONSTITUTION_PROPOSALS: i64 = 38;
/// `migrations/0039_work_task_command_execution.sql`. Key 38 was already
/// recorded as applied in the shared development database by
/// concurrent, then-not-yet-committed work at the time this was drafted.
pub(crate) const WORK_TASK_COMMAND_EXECUTION: i64 = 39;
/// `migrations/0041_administration.sql`. 39 and 40 (like 19 before them)
/// were already reached by the shared development database from
/// concurrent work nobody had committed yet -- `migration-audit.mjs`
/// checked before picking 41, not guessed.
pub(crate) const ADMINISTRATION: i64 = 41;
/// `migrations/0042_administration_purge.sql`.
pub(crate) const ADMINISTRATION_PURGE: i64 = 42;
/// `migrations/0046_administration_recovery_inspection.sql`. 43, 44, and
/// 45 were each reached by the shared development database from
/// concurrent work nobody had committed yet -- checked directly against
/// `ackplane_schema_migrations` (44 collided with a different session's
/// migration under the *same* key, which `migration-audit.mjs` cannot
/// see because this branch's own source already accounts for that key)
/// before picking 46.
pub(crate) const ADMINISTRATION_RECOVERY_INSPECTION: i64 = 46;
/// `migrations/0047_administration_export.sql`. Checked directly against
/// `ackplane_schema_migrations` (only 46 applied above 42 at the time
/// this was drafted) before picking 47.
pub(crate) const ADMINISTRATION_EXPORT: i64 = 47;
/// `migrations/0049_delegation_use_receipts.sql`. Keys 43, 44, and 48
/// were already applied by concurrent work in the shared development
/// database when this unmerged slice was recovered; `migration-audit --next`
/// selected 49 rather than reusing an ambiguous key.
pub(crate) const DELEGATION_USE_RECEIPTS: i64 = 49;
/// `migrations/0050_administration_purge_confirming_label.sql`. Key 49
/// is already allocated to delegation-use receipts on main, so
/// `migration-audit --next` selected 50 for this recovered purge fix.
pub(crate) const ADMINISTRATION_PURGE_CONFIRMING_LABEL: i64 = 50;
/// `migrations/0051_administration_purge_confirmation_authentication.sql`.
pub(crate) const ADMINISTRATION_PURGE_CONFIRMATION_AUTHENTICATION: i64 = 51;
/// `migrations/0052_administration_purge_confirmation_fingerprint.sql`.
pub(crate) const ADMINISTRATION_PURGE_CONFIRMATION_FINGERPRINT: i64 = 52;
/// `migrations/0053_work_command_directives.sql`. Checked via
/// `migration-audit.mjs` before picking 53: no live discrepancy above 52.
pub(crate) const WORK_COMMAND_DIRECTIVES: i64 = 53;
/// `migrations/0054_human_decision_requests.sql` (ADR-0115 item 5).
/// Key 53 was already applied by concurrent work in the shared
/// development database when this slice was written;
/// `migration-audit --next` selected 54.
pub(crate) const HUMAN_DECISION_REQUESTS: i64 = 54;
/// `migrations/0055_projected_node_embeddings.sql` (ADR-0140 decision 1).
/// `migration-audit --next` selected 55: no live discrepancy above 54.
pub(crate) const PROJECTED_NODE_EMBEDDINGS: i64 = 55;
/// `migrations/0056_supervisor_outbox_positions.sql` (ADR-0146 decision 3).
/// `migration-audit --next` selected 56 from committed source; no live
/// database was reachable to check for a higher applied key, so this is
/// the next free key on `main` rather than a verified-against-live one.
pub(crate) const SUPERVISOR_OUTBOX_POSITIONS: i64 = 56;
/// `migrations/0057_administration_recovery_rehearsal.sql` (ADR-0145
/// slice 1). `migration-audit --next` selected 57 from committed source;
/// no live database was reachable to check for a higher applied key.
pub(crate) const ADMINISTRATION_RECOVERY_REHEARSAL: i64 = 57;
/// `migrations/0058_administration_recovery_execution.sql` (ADR-0145
/// slice 2). `migration-audit --next` selected 58, checked against the
/// shared development database (`ackplane_test`) as well as committed
/// source: no live discrepancy above 57.
pub(crate) const ADMINISTRATION_RECOVERY_EXECUTION: i64 = 58;
/// `migrations/0059_design_constitution_display_label.sql` (ADR-0142
/// decision 4). `migration-audit --next` selected 59 from committed
/// source, but the shared development database (`ackplane_test`) had
/// already applied a different migration's content under key 59 by the
/// time tests ran here -- renumbered to 60 rather than reusing an
/// ambiguous key. `migration-audit.mjs` cannot see this class of
/// collision from committed source alone; only the live database
/// surfaces a same-key-different-content mismatch, and only at
/// connect() time.
///
/// `migrations/0060_constitution_proposals_display_label.sql`, applied by
/// `ConstitutionStore::connect()` only. Originally bundled with the
/// materialization table's ALTER under this one key; split so this
/// store's connect() never touches a table only `MaterializationStore`
/// creates (see key 61's doc comment).
pub(crate) const CONSTITUTION_PROPOSALS_DISPLAY_LABEL: i64 = 60;
/// `migrations/0061_design_materialization_display_label.sql`, applied by
/// `MaterializationStore::connect()` only. On a genuinely fresh database
/// (unlike the long-lived shared dev container, which already had this
/// table from unrelated prior activity), `ConstitutionStore::connect()`
/// can run before `MaterializationStore::connect()` ever creates
/// `industrial_design_materializations` (migration 0032) -- bundling
/// both ALTERs under one key made `ConstitutionStore` fail with
/// "relation industrial_design_materializations does not exist" in CI's
/// clean-container run. Each store now only ever migrates tables it
/// already owns or transitively depends on.
pub(crate) const DESIGN_MATERIALIZATION_DISPLAY_LABEL: i64 = 61;
/// `migrations/0063_administration_recovery_execution_receipt.sql`
/// (ADR-0145 slice 4). Filed as 59, 60, 61, then 62 -- each time the
/// shared development database (`ackplane_test`) had accepted a different
/// migration under that key from a concurrent branch first. Renumbered a
/// fifth time to 63 for a genuinely new reason: CI caught that 62's own
/// content had real foreign keys from `rehearsal_id`/`request_id` to
/// tables a real restore always empties before this row is ever inserted
/// (see the migration's own comment), so the content changed and needed
/// a fresh key rather than reusing 62 under new content.
pub(crate) const ADMINISTRATION_RECOVERY_EXECUTION_RECEIPT: i64 = 63;
/// `migrations/0064_industrial_designs_display_label.sql` -- the third
/// and final table of ADR-0142 decision 4's display_label rollout
/// (`CONSTITUTION_PROPOSALS_DISPLAY_LABEL`/60 and
/// `DESIGN_MATERIALIZATION_DISPLAY_LABEL`/61 closed the other two;
/// this one was deferred while `design_store.rs` carried a live
/// ADR-0143 pool-migration claim).
pub(crate) const INDUSTRIAL_DESIGNS_DISPLAY_LABEL: i64 = 64;
/// `migrations/0065_work_event_positions.sql`
pub(crate) const WORK_EVENT_POSITIONS: i64 = 65;
/// `migrations/0066_delegated_claim_parked.sql` (ADR-0096 clause
/// completion). `migration-audit --next` selected 66 from committed
/// source; no live discrepancy above 65.
pub(crate) const DELEGATED_CLAIM_PARKED: i64 = 66;
/// Not a real schema migration -- reserved so it can never collide with
/// one (every real key above is non-negative, allocated by
/// `migration-audit.mjs --next`). Its presence as a row in
/// `ackplane_schema_migrations` is a database-level marker: this
/// instance has been explicitly designated shared, and `migrate_locked`
/// refuses to apply anything against it without an explicit review
/// acknowledgement (`ACKPLANE_MIGRATE_REVIEWED`). See
/// `mark_shared_database` and
/// gaps.d/unaccepted-work-migration-reaches-shared-db.md, which this
/// exists to close: nothing previously stopped a branch-local,
/// unreviewed migration from reaching a shared database in the first
/// place -- only detected the damage afterwards.
pub(crate) const SHARED_DATABASE_MARKER: i64 = -2;
