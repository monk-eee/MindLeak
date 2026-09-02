//! Applies every table an Ackplane deployment needs, one store at a time.
//!
//! Extracted from `bin/migrate.rs` (ADR-0088 clause 5's one-shot migration
//! entrypoint) so ADR-0145's recovery rehearsal can run the identical
//! migration sequence against a scratch, ephemeral database and learn
//! whether the restored schema is current -- reusing this exact store list
//! rather than forking a second copy of it (a store this list does not cover
//! is a migration race `bin/migrate.rs` cannot be trusted to have prevented,
//! and a rehearsal check this list does not cover proves nothing about that
//! same store). Keep this list in lockstep with `main.rs`'s own store list.
//!
//! Applies no migration logic of its own: each named store's `connect` runs
//! its own idempotent `CREATE TABLE IF NOT EXISTS` migration as a side effect
//! of connecting.

use crate::claim_store::ClaimStore;
use crate::constitution_store::ConstitutionStore;
use crate::delegation_store::DelegationStore;
use crate::directive_store::DirectiveStore;
use crate::enrollment_store::EnrollmentStore;
use crate::evidence_store::EvidenceStore;
use crate::human_decision_store::HumanDecisionStore;
use crate::knowledge_store::KnowledgeStore;
use crate::ledger::LedgerStore;
use crate::live_feed_store::LiveFeedStore;
use crate::projection::Projector;
use crate::supervisor_store::SupervisorStore;
use crate::telemetry_store::TelemetryStore;
use crate::work_store::WorkStore;

/// Applies every table this deployment needs against `database_url` and
/// returns once every store's schema is current, or the first failure.
pub async fn migrate_all(database_url: &str) -> Result<(), String> {
    // One pool for this process (ADR-0143 decision 1). Stores still on their
    // own `connect(database_url)` take their turn in the migration sequence;
    // none is left half-migrated.
    let pool = crate::db_pool::build_pool(database_url, crate::db_pool::SERVICE_POOL_MAX_SIZE)
        .map_err(|error| format!("building the database pool failed: {error}"))?;
    LedgerStore::connect(&pool)
        .await
        .map_err(|error| format!("ledger schema failed: {error}"))?;
    EnrollmentStore::connect(&pool)
        .await
        .map_err(|error| format!("enrollment schema failed: {error}"))?;
    ClaimStore::connect(&pool)
        .await
        .map_err(|error| format!("claim schema failed: {error}"))?;
    Projector::connect(&pool)
        .await
        .map_err(|error| format!("projection schema failed: {error}"))?;
    KnowledgeStore::connect(&pool)
        .await
        .map_err(|error| format!("knowledge schema failed: {error}"))?;
    EvidenceStore::connect(&pool)
        .await
        .map_err(|error| format!("evidence schema failed: {error}"))?;
    ConstitutionStore::connect(&pool)
        .await
        .map_err(|error| format!("constitution schema failed: {error}"))?;
    TelemetryStore::connect(&pool)
        .await
        .map_err(|error| format!("telemetry schema failed: {error}"))?;
    DelegationStore::connect(&pool)
        .await
        .map_err(|error| format!("delegation schema failed: {error}"))?;
    DirectiveStore::connect(&pool)
        .await
        .map_err(|error| format!("directive schema failed: {error}"))?;
    SupervisorStore::connect(&pool)
        .await
        .map_err(|error| format!("supervisor schema failed: {error}"))?;
    LiveFeedStore::connect(&pool)
        .await
        .map_err(|error| format!("live feed schema failed: {error}"))?;
    WorkStore::connect(&pool)
        .await
        .map_err(|error| format!("work schema failed: {error}"))?;
    HumanDecisionStore::connect(&pool)
        .await
        .map_err(|error| format!("human decision schema failed: {error}"))?;
    Ok(())
}

/// Marks `database_url`'s target shared (ADR unnumbered: the migration-apply
/// gate that closes gaps.d/unaccepted-work-migration-reaches-shared-db.md):
/// a one-time, explicit provisioning action for whoever stands up a shared
/// or persistent Postgres instance. After this, every `migrate_locked` call
/// against it refuses without an explicit `ACKPLANE_MIGRATE_REVIEWED`
/// acknowledgement. Exposed as `ackplane-migrate --mark-shared`.
pub async fn mark_database_shared(database_url: &str) -> Result<(), String> {
    let (client, connection) = tokio_postgres::connect(database_url, tokio_postgres::NoTls)
        .await
        .map_err(|error| format!("connecting to mark the database shared failed: {error}"))?;
    tokio::spawn(async move {
        let _ = connection.await;
    });
    crate::migration_lock::mark_shared_database(&client)
        .await
        .map_err(|error| format!("marking the database shared failed: {error}"))
}

#[cfg(test)]
mod tests {
    const SOURCE: &str = include_str!("schema_migration.rs");

    // Regression: this list previously lived only in `bin/migrate.rs` and
    // drifted from main.rs's (5 of 9 boot-time stores covered), so a fresh
    // deployment could start any of the other four services racing their own
    // first migration. It also names direct-consumer stores whose schema must
    // exist before their later wiring.
    #[test]
    fn migrate_all_covers_every_store_main_rs_connects_at_boot() {
        for store in [
            "LedgerStore",
            "EnrollmentStore",
            "ClaimStore",
            "Projector",
            "KnowledgeStore",
            "EvidenceStore",
            "ConstitutionStore",
            "TelemetryStore",
            "DelegationStore",
            "DirectiveStore",
            "SupervisorStore",
            "LiveFeedStore",
            "WorkStore",
            "HumanDecisionStore",
        ] {
            assert!(
                SOURCE.contains(&format!("{store}::connect")),
                "migrate_all() must call {store}::connect -- main.rs connects it at boot"
            );
        }
    }
}
