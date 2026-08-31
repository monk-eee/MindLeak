//! Durable ADR-0107 directive and receipt records, and the undelivered-directive
//! read that live NodeSync delivery draws from (ADR-0116 slice 3).

use crate::db_pool::{PgConnection, PgPool};
use crate::migration_lock;

const MIGRATION: &str = include_str!("../../migrations/0030_directives.sql");
const SUPERVISOR_MIGRATION: &str =
    include_str!("../../migrations/0024_supervisor_session_projection.sql");

mod model;
mod read;
mod write;

pub use model::{
    DirectiveReceiptOutcome, DirectiveReceiptRecord, DirectiveRecord, DirectiveStoreError,
    DirectiveWriteOutcome,
};
pub use read::MAX_DELIVERY_BATCH;
pub(crate) use write::enqueue_in_transaction;

/// PostgreSQL persistence for immutable typed directives and their receipts.
pub struct DirectiveStore {
    pool: PgPool,
}

impl DirectiveStore {
    /// Takes a clone of the process's single pool (ADR-0143 decision 1), not a
    /// database URL: a store that resolved its own connection would be exactly
    /// the per-store demand the pool exists to bound.
    pub async fn connect(pool: &PgPool) -> Result<Self, DirectiveStoreError> {
        let mut connection = pool.get().await?;
        migration_lock::migrate_locked(
            &mut connection,
            migration_lock::key::SUPERVISOR_SESSION_PROJECTION,
            SUPERVISOR_MIGRATION,
        )
        .await?;
        migration_lock::migrate_locked(&mut connection, migration_lock::key::DIRECTIVES, MIGRATION)
            .await?;
        Ok(Self { pool: pool.clone() })
    }

    /// One checked-out connection, held only for the call that asked for it.
    ///
    /// A caller that opens a transaction keeps this binding alive for the life
    /// of that transaction, which is the one case where holding a connection
    /// across `.await` points is correct rather than accidental.
    pub(crate) async fn connection(&self) -> Result<PgConnection, DirectiveStoreError> {
        Ok(self.pool.get().await?)
    }
}

#[cfg(test)]
mod tests;
