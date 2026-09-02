//! Immutable Industrial Work command requests and receipts (ADR-0125).
//!
//! The authoritative command service validates authorization before calling
//! this store. It records intent and outcome references, executes the five
//! server-owned commands' Work/Claim effects, and issues the five
//! supervisor-directed commands' ADR-0107 directives. [`WorkCommandService`]
//! is the one authoritative entry point a Bridge route (or any other typed
//! caller) may use -- a route that reaches [`WorkCommandStore`] directly
//! without it is the contract violation ADR-0125 decision 11 rejects.

use crate::db_pool::{PgConnection, PgPool};
use crate::migration_lock;

const MIGRATION: &str = include_str!("../../migrations/0037_work_commands.sql");
const WORK_MIGRATION: &str = include_str!("../../migrations/0028_work.sql");
const EXECUTION_MIGRATION: &str =
    include_str!("../../migrations/0039_work_task_command_execution.sql");
const DIRECTIVES_MIGRATION: &str =
    include_str!("../../migrations/0053_work_command_directives.sql");
const SUPERVISOR_SESSION_MIGRATION: &str =
    include_str!("../../migrations/0024_supervisor_session_projection.sql");
const AGENT_DIRECTIVES_MIGRATION: &str = include_str!("../../migrations/0030_directives.sql");
const WORK_EVENT_POSITIONS_MIGRATION: &str =
    include_str!("../../migrations/0065_work_event_positions.sql");

mod execute;
mod model;
mod payload;
mod service;
mod write;

use model::{NewWorkCommandReceipt, WorkCommandReceiptWriteOutcome, WorkCommandWriteOutcome};

pub use model::{
    NewWorkCommand, WorkCommand, WorkCommandKind, WorkCommandOutcome, WorkCommandReceipt,
    WorkCommandStoreError,
};
pub use payload::{
    payload_digest, AnswerWaitPayload, AssignPayload, CreateWorkPayload, DirectiveTarget,
    DrainPayload, PausePayload, ReleaseLeasePayload, ResumePayload, ReviewDisposition,
    RouteWorkPayload, SteerPayload, SubmitReviewPayload, WorkCommandPayload,
};
pub use service::{
    VerifiedWorkCommandPrincipal, WorkCommandAuthorization, WorkCommandRefusal, WorkCommandService,
    WorkCommandServiceError, WorkCommandServiceOutcome, AUTHORIZATION_UNAVAILABLE_REASON,
};

/// PostgreSQL persistence for immutable Work command requests and receipts.
pub struct WorkCommandStore {
    pool: PgPool,
}

impl WorkCommandStore {
    /// Takes a clone of the process's single pool (ADR-0143 decision 1), not a
    /// database URL: a store that resolved its own connection would be exactly
    /// the per-store demand the pool exists to bound.
    pub async fn connect(pool: &PgPool) -> Result<Self, WorkCommandStoreError> {
        let mut client = pool.get().await?;
        migration_lock::migrate_locked(&mut client, migration_lock::key::WORK, WORK_MIGRATION)
            .await?;
        migration_lock::migrate_locked(&mut client, migration_lock::key::WORK_COMMANDS, MIGRATION)
            .await?;
        migration_lock::migrate_locked(
            &mut client,
            migration_lock::key::WORK_TASK_COMMAND_EXECUTION,
            EXECUTION_MIGRATION,
        )
        .await?;
        // This store appends lifecycle events to the same per-repository Work
        // stream `WorkStore` creates into, so it owns migrating the positions
        // it writes -- it can legitimately connect before `WorkStore` does.
        migration_lock::migrate_locked(
            &mut client,
            migration_lock::key::WORK_EVENT_POSITIONS,
            WORK_EVENT_POSITIONS_MIGRATION,
        )
        .await?;
        // Supervisor-directed commands (Assign/Steer/Pause/Resume/Drain)
        // issue an ADR-0107 directive on this same connection's transaction
        // (see `execute::supervisor_directives`), so this store also owns
        // migrating the directive tables it writes to -- exactly the same
        // dual-migration pattern `directive_store::DirectiveStore::connect`
        // already uses for its own supervisor-session dependency.
        migration_lock::migrate_locked(
            &mut client,
            migration_lock::key::SUPERVISOR_SESSION_PROJECTION,
            SUPERVISOR_SESSION_MIGRATION,
        )
        .await?;
        migration_lock::migrate_locked(
            &mut client,
            migration_lock::key::DIRECTIVES,
            AGENT_DIRECTIVES_MIGRATION,
        )
        .await?;
        migration_lock::migrate_locked(
            &mut client,
            migration_lock::key::WORK_COMMAND_DIRECTIVES,
            DIRECTIVES_MIGRATION,
        )
        .await?;
        Ok(Self { pool: pool.clone() })
    }

    /// One checked-out connection, held only for the call that asked for it.
    ///
    /// A caller that opens a transaction keeps this binding alive for the life
    /// of that transaction — which matters more here than in most stores: a
    /// supervisor-directed command writes its Work/Claim effect and issues its
    /// ADR-0107 directive on one transaction, so both must land on the same
    /// connection or neither is atomic.
    pub(crate) async fn connection(&self) -> Result<PgConnection, WorkCommandStoreError> {
        Ok(crate::db_pool::checkout(&self.pool).await?)
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod supervisor_directive_tests;
