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
        &mut self,
        request: DelegationUseRequest,
        now: std::time::SystemTime,
    ) -> Result<DelegationUseOutcome, DelegationUseError> {
        write::authorize_use(&mut self.client, request, now).await
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
        read::list_use_receipts(
            &self.client,
            tenant_id,
            repository_id,
            delegation_id,
            after,
            requested_limit,
        )
        .await
    }
}
