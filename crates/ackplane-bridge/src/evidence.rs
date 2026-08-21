//! Tenant-scoped read access for the Industrial Evidence Board.
//!
//! The Bridge deliberately reuses Ackplane's typed Evidence store rather
//! than opening a raw SQL route or copying evidence bodies into browser state.

use std::error::Error;

use ackplane_server::evidence_store::{
    ConformanceCursor, ConformancePage, ConformanceStoreError, EvidenceCursor, EvidencePage,
    EvidenceStore, EvidenceStoreError,
};

pub const DEFAULT_PAGE_SIZE: u32 = 20;
pub const MAX_PAGE_SIZE: u32 = 100;

/// Read-only Evidence Board data source for one Bridge process.
pub struct BridgeEvidenceStore {
    store: EvidenceStore,
}

impl BridgeEvidenceStore {
    pub async fn connect(
        database_url: &str,
    ) -> Result<Self, Box<dyn Error + Send + Sync + 'static>> {
        Ok(Self {
            store: EvidenceStore::connect(database_url).await?,
        })
    }

    /// Returns one stable keyset page of typed evidence for a tenant-owned task.
    pub async fn task_evidence(
        &self,
        tenant_id: &str,
        repository_id: &str,
        task_id: &str,
        cursor: Option<&EvidenceCursor>,
        requested_limit: Option<u32>,
    ) -> Result<EvidencePage, EvidenceStoreError> {
        self.store
            .list_page(
                tenant_id,
                repository_id,
                task_id,
                cursor,
                page_limit(requested_limit),
            )
            .await
    }

    /// Returns one stable keyset page of derived conformance history for the
    /// same tenant-owned task. Findings remain a count and digest, never body text.
    pub async fn task_conformance(
        &self,
        tenant_id: &str,
        repository_id: &str,
        task_id: &str,
        cursor: Option<&ConformanceCursor>,
        requested_limit: Option<u32>,
    ) -> Result<ConformancePage, ConformanceStoreError> {
        self.store
            .list_conformance_page(
                tenant_id,
                repository_id,
                task_id,
                cursor,
                page_limit(requested_limit),
            )
            .await
    }
}

pub fn page_limit(requested_limit: Option<u32>) -> i64 {
    i64::from(
        requested_limit
            .unwrap_or(DEFAULT_PAGE_SIZE)
            .clamp(1, MAX_PAGE_SIZE),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_limit_uses_the_default_and_bounds_requested_values() {
        for (requested, expected) in [
            (None, i64::from(DEFAULT_PAGE_SIZE)),
            (Some(0), 1),
            (Some(42), 42),
            (Some(MAX_PAGE_SIZE + 1), i64::from(MAX_PAGE_SIZE)),
        ] {
            assert_eq!(page_limit(requested), expected);
        }
    }
}
