//! Durable ADR-0107 directive and receipt records, and the undelivered-directive
//! read that live NodeSync delivery draws from (ADR-0116 slice 3).

use tokio_postgres::{Client, NoTls};

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
    pub(crate) client: Client,
}

impl DirectiveStore {
    pub async fn connect(database_url: &str) -> Result<Self, tokio_postgres::Error> {
        let (mut client, connection) = tokio_postgres::connect(database_url, NoTls).await?;
        tokio::spawn(async move {
            if let Err(error) = connection.await {
                tracing::error!(%error, "ackplane directive store connection closed with an error");
            }
        });
        migration_lock::migrate_locked(
            &mut client,
            migration_lock::key::SUPERVISOR_SESSION_PROJECTION,
            SUPERVISOR_MIGRATION,
        )
        .await?;
        migration_lock::migrate_locked(&mut client, migration_lock::key::DIRECTIVES, MIGRATION)
            .await?;
        Ok(Self { client })
    }
}

#[cfg(test)]
mod tests;
