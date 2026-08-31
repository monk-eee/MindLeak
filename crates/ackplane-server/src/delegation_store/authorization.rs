//! Live, bounded authorization decisions for one delegated routine action.
//!
//! A grant records a human-approved envelope. This module re-checks that
//! envelope at use time and writes an immutable receipt for both authorizations
//! and refusals. It is server-internal: an authenticated service supplies the
//! already-verified delegatee and policy basis.

mod model;
mod read;
#[cfg(test)]
mod tests;
mod write;

pub use model::{
    DelegationUseError, DelegationUseOutcome, DelegationUseReceipt, DelegationUseReceiptCursor,
    DelegationUseReceiptPage, DelegationUseRefusal, DelegationUseRequest, DelegationUseStatus,
};

use super::DelegationStore;

impl DelegationStore {
    /// Re-checks an active delegation and records the resulting immutable use
    /// receipt. The caller supplies the authoritative server time, never a
    /// browser or agent clock.
    pub async fn authorize_use(
        &self,
        request: DelegationUseRequest,
        now: std::time::SystemTime,
    ) -> Result<DelegationUseOutcome, DelegationUseError> {
        // One connection for the whole held transaction (ADR-0143 decision 4):
        // the `FOR UPDATE` lock on the projection row is session-scoped, and the
        // concurrent-replay test depends on a waiter blocking against a stable
        // connection identity for as long as the holder's transaction is open.
        let mut connection = self.pool.get().await?;
        write::authorize_use(&mut connection, request, now).await
    }

    /// Lists one delegation's use decisions in durable receipt order. The
    /// caller is responsible for checking tenant/repository visibility first.
    pub async fn list_use_receipts(
        &self,
        tenant_id: &str,
        repository_id: &str,
        delegation_id: &str,
        after: Option<&DelegationUseReceiptCursor>,
        requested_limit: i64,
    ) -> Result<DelegationUseReceiptPage, DelegationUseError> {
        let connection = self.pool.get().await?;
        read::list_use_receipts(
            &connection,
            tenant_id,
            repository_id,
            delegation_id,
            after,
            requested_limit,
        )
        .await
    }
}
