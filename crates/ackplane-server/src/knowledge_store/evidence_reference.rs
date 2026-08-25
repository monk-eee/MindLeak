use std::time::SystemTime;

use tokio_postgres::Row;

use super::{KnowledgeStore, KnowledgeStoreError};

/// A bounded page of one knowledge statement's evidence trail never returns
/// more than this many rows (ADR-0113 decision 5's "queries have hard
/// result... bounds", applied here to decision 3's evidence references).
const MAX_EVIDENCE_REFERENCES_PAGE: i64 = 100;

/// What kind of outcome a reference cites (ADR-0113 decision 3: "the task,
/// context packet, validation, or receipt that supported a lesson").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KnowledgeEvidenceReferenceKind {
    Task = 1,
    ContextPacket = 2,
    Validation = 3,
    Receipt = 4,
}

impl TryFrom<i16> for KnowledgeEvidenceReferenceKind {
    type Error = ();

    fn try_from(value: i16) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::Task),
            2 => Ok(Self::ContextPacket),
            3 => Ok(Self::Validation),
            4 => Ok(Self::Receipt),
            _ => Err(()),
        }
    }
}

/// Whether a reference supports or undermines the statement it is attached
/// to (ADR-0113 decision 3: "records corroboration and later contradiction
/// as separate facts", not one opaque confidence number).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KnowledgeEvidencePolarity {
    Corroborates = 1,
    Contradicts = 2,
}

impl TryFrom<i16> for KnowledgeEvidencePolarity {
    type Error = ();

    fn try_from(value: i16) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::Corroborates),
            2 => Ok(Self::Contradicts),
            _ => Err(()),
        }
    }
}

/// One durable evidence or outcome reference attached to a knowledge
/// statement. Never updated or deleted once written -- a later contradiction
/// is a NEW reference, not an edit to a corroborating one.
#[derive(Debug, Clone, PartialEq)]
pub struct KnowledgeEvidenceReference {
    pub reference_id: String,
    pub kind: KnowledgeEvidenceReferenceKind,
    pub reference_ref: String,
    pub polarity: KnowledgeEvidencePolarity,
    pub recorded_by: String,
    pub recorded_at: SystemTime,
}

/// Uses a request struct rather than positional parameters for the same
/// reason `record`/`supersede` do: enough fields that a bare parameter list
/// would obscure which is which (exempted from the tenant-id guard's
/// literal-signature check alongside them, in `repository_id_guard.rs`).
#[derive(Debug, Clone, PartialEq)]
pub struct RecordKnowledgeEvidenceReferenceRequest {
    pub tenant_id: String,
    pub repository_id: String,
    pub knowledge_id: String,
    pub kind: KnowledgeEvidenceReferenceKind,
    pub reference_ref: String,
    pub polarity: KnowledgeEvidencePolarity,
    pub recorded_by: String,
}

impl KnowledgeStore {
    /// Attaches one durable evidence/outcome reference to a knowledge
    /// statement -- in any lifecycle state: corroboration is how a Candidate
    /// earns activation, and a contradiction can be recorded against Active
    /// (or already-superseded/retired) knowledge long after the fact,
    /// without that alone changing its lifecycle state. The `SELECT ...
    /// FROM knowledge WHERE ...` guard refuses an unknown `knowledge_id`
    /// with a typed error rather than surfacing a raw foreign-key violation.
    pub async fn record_evidence_reference(
        &self,
        request: RecordKnowledgeEvidenceReferenceRequest,
        now: SystemTime,
    ) -> Result<KnowledgeEvidenceReference, KnowledgeStoreError> {
        let reference_ref = request.reference_ref.trim();
        if reference_ref.is_empty() {
            return Err(KnowledgeStoreError::EmptyEvidenceReferenceRef);
        }
        let recorded_by = request.recorded_by.trim();
        if recorded_by.is_empty() {
            return Err(KnowledgeStoreError::EmptyEvidenceReferenceRecordedBy);
        }
        let reference_id = unique_evidence_reference_id();
        let row = self
            .client
            .query_opt(
                "INSERT INTO knowledge_evidence_references \
                     (tenant_id, repository_id, knowledge_id, reference_id, reference_kind, reference_ref, polarity, recorded_by, recorded_at) \
                 SELECT $1, $2, knowledge_id, $4, $5, $6, $7, $8, $9 FROM knowledge \
                  WHERE tenant_id = $1 AND repository_id = $2 AND knowledge_id = $3 \
                 RETURNING reference_id, reference_kind, reference_ref, polarity, recorded_by, recorded_at",
                &[
                    &request.tenant_id,
                    &request.repository_id,
                    &request.knowledge_id,
                    &reference_id,
                    &(request.kind as i16),
                    &reference_ref,
                    &(request.polarity as i16),
                    &recorded_by,
                    &now,
                ],
            )
            .await?;
        match row {
            Some(row) => Ok(evidence_reference_from_row(row)),
            None => Err(KnowledgeStoreError::UnknownKnowledge {
                knowledge_id: request.knowledge_id,
            }),
        }
    }

    /// One knowledge statement's evidence trail, most recent first, hard
    /// bounded regardless of the caller's requested `limit`.
    pub async fn evidence_references(
        &self,
        tenant_id: &str,
        repository_id: &str,
        knowledge_id: &str,
        limit: i64,
    ) -> Result<Vec<KnowledgeEvidenceReference>, KnowledgeStoreError> {
        let limit = bounded_evidence_reference_limit(limit);
        let rows = self
            .client
            .query(
                "SELECT reference_id, reference_kind, reference_ref, polarity, recorded_by, recorded_at \
                 FROM knowledge_evidence_references \
                 WHERE tenant_id = $1 AND repository_id = $2 AND knowledge_id = $3 \
                 ORDER BY recorded_at DESC, reference_id DESC \
                 LIMIT $4",
                &[&tenant_id, &repository_id, &knowledge_id, &limit],
            )
            .await?;
        Ok(rows.into_iter().map(evidence_reference_from_row).collect())
    }
}

fn evidence_reference_from_row(row: Row) -> KnowledgeEvidenceReference {
    let kind: i16 = row.get("reference_kind");
    let polarity: i16 = row.get("polarity");
    KnowledgeEvidenceReference {
        reference_id: row.get("reference_id"),
        // A CHECK constraint keeps these in range at the database layer;
        // this fallback only matters for a row a manual UPDATE corrupted,
        // mirroring `history()`'s own unwrap_or for a bulk-list read where
        // erroring the whole page on one bad row is worse than one
        // mis-grouped entry.
        kind: KnowledgeEvidenceReferenceKind::try_from(kind)
            .unwrap_or(KnowledgeEvidenceReferenceKind::Receipt),
        reference_ref: row.get("reference_ref"),
        polarity: KnowledgeEvidencePolarity::try_from(polarity)
            .unwrap_or(KnowledgeEvidencePolarity::Corroborates),
        recorded_by: row.get("recorded_by"),
        recorded_at: row.get("recorded_at"),
    }
}

fn unique_evidence_reference_id() -> String {
    let mut bytes = [0u8; 16];
    getrandom::getrandom(&mut bytes).expect("the OS random source should be available");
    let hex: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    format!("knowledge-evidence-reference-{hex}")
}

/// A caller's requested page size never overrides decision 5's hard bound,
/// and a non-positive request still returns at least one row rather than
/// zero -- pure and deterministic, so the bound itself is unit-tested
/// directly rather than only indirectly through a real database round trip.
fn bounded_evidence_reference_limit(requested: i64) -> i64 {
    requested.clamp(1, MAX_EVIDENCE_REFERENCES_PAGE)
}

#[cfg(test)]
mod tests {
    use super::super::tests::{store, unique_scope};
    use super::*;
    use crate::knowledge_store::RecordKnowledgeRequest;

    fn record_request(
        tenant_id: &str,
        repository_id: &str,
        content: &str,
    ) -> RecordKnowledgeRequest {
        RecordKnowledgeRequest {
            tenant_id: tenant_id.to_string(),
            repository_id: repository_id.to_string(),
            content: content.to_string(),
            source_ref: None,
            recorded_by: None,
            reach_node_ids: vec![],
            reach_goal_id: None,
            half_life_hours: 720.0,
            embedding: None,
        }
    }

    fn reference_request(
        tenant_id: &str,
        repository_id: &str,
        knowledge_id: &str,
        reference_ref: &str,
        polarity: KnowledgeEvidencePolarity,
    ) -> RecordKnowledgeEvidenceReferenceRequest {
        RecordKnowledgeEvidenceReferenceRequest {
            tenant_id: tenant_id.to_string(),
            repository_id: repository_id.to_string(),
            knowledge_id: knowledge_id.to_string(),
            kind: KnowledgeEvidenceReferenceKind::Task,
            reference_ref: reference_ref.to_string(),
            polarity,
            recorded_by: "agent:reviewer".to_string(),
        }
    }

    #[tokio::test]
    async fn a_recorded_reference_round_trips_its_kind_and_polarity() {
        let Some(store) = store().await else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let (tenant_id, repository_id) = unique_scope("evidence-reference-round-trip");
        let recorded = store
            .record(record_request(
                &tenant_id,
                &repository_id,
                "a candidate lesson",
            ))
            .await
            .unwrap();

        let reference = store
            .record_evidence_reference(
                reference_request(
                    &tenant_id,
                    &repository_id,
                    &recorded.knowledge_id,
                    "task:abc123",
                    KnowledgeEvidencePolarity::Corroborates,
                ),
                SystemTime::now(),
            )
            .await
            .unwrap();

        assert_eq!(reference.kind, KnowledgeEvidenceReferenceKind::Task);
        assert_eq!(reference.reference_ref, "task:abc123");
        assert_eq!(reference.polarity, KnowledgeEvidencePolarity::Corroborates);
        assert_eq!(reference.recorded_by, "agent:reviewer");
    }

    #[tokio::test]
    async fn corroboration_and_contradiction_are_recorded_as_separate_facts() {
        let Some(store) = store().await else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let (tenant_id, repository_id) = unique_scope("evidence-reference-both-polarities");
        let recorded = store
            .record(record_request(
                &tenant_id,
                &repository_id,
                "a candidate lesson",
            ))
            .await
            .unwrap();
        store
            .record_evidence_reference(
                reference_request(
                    &tenant_id,
                    &repository_id,
                    &recorded.knowledge_id,
                    "task:corroborating-1",
                    KnowledgeEvidencePolarity::Corroborates,
                ),
                SystemTime::now(),
            )
            .await
            .unwrap();
        store
            .record_evidence_reference(
                reference_request(
                    &tenant_id,
                    &repository_id,
                    &recorded.knowledge_id,
                    "task:contradicting-1",
                    KnowledgeEvidencePolarity::Contradicts,
                ),
                SystemTime::now(),
            )
            .await
            .unwrap();

        let references = store
            .evidence_references(&tenant_id, &repository_id, &recorded.knowledge_id, 10)
            .await
            .unwrap();

        assert_eq!(references.len(), 2);
        assert!(references
            .iter()
            .any(|r| r.reference_ref == "task:corroborating-1"
                && r.polarity == KnowledgeEvidencePolarity::Corroborates));
        assert!(references
            .iter()
            .any(|r| r.reference_ref == "task:contradicting-1"
                && r.polarity == KnowledgeEvidencePolarity::Contradicts));
    }

    #[tokio::test]
    async fn a_reference_can_be_recorded_against_a_still_candidate_statement() {
        let Some(store) = store().await else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let (tenant_id, repository_id) = unique_scope("evidence-reference-candidate");
        let recorded = store
            .record(record_request(
                &tenant_id,
                &repository_id,
                "an unreviewed candidate",
            ))
            .await
            .unwrap();

        let reference = store
            .record_evidence_reference(
                reference_request(
                    &tenant_id,
                    &repository_id,
                    &recorded.knowledge_id,
                    "context_packet:xyz",
                    KnowledgeEvidencePolarity::Corroborates,
                ),
                SystemTime::now(),
            )
            .await
            .unwrap();

        assert_eq!(reference.reference_ref, "context_packet:xyz");
    }

    #[tokio::test]
    async fn recording_a_reference_against_an_unknown_knowledge_id_refuses() {
        let Some(store) = store().await else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let (tenant_id, repository_id) = unique_scope("evidence-reference-unknown");

        let error = store
            .record_evidence_reference(
                reference_request(
                    &tenant_id,
                    &repository_id,
                    "knowledge-does-not-exist",
                    "task:abc123",
                    KnowledgeEvidencePolarity::Corroborates,
                ),
                SystemTime::now(),
            )
            .await
            .unwrap_err();

        assert!(matches!(
            error,
            KnowledgeStoreError::UnknownKnowledge { knowledge_id } if knowledge_id == "knowledge-does-not-exist"
        ));
    }

    #[tokio::test]
    async fn recording_a_reference_refuses_an_empty_reference_ref() {
        let Some(store) = store().await else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let (tenant_id, repository_id) = unique_scope("evidence-reference-empty-ref");
        let recorded = store
            .record(record_request(
                &tenant_id,
                &repository_id,
                "a candidate lesson",
            ))
            .await
            .unwrap();
        let mut request = reference_request(
            &tenant_id,
            &repository_id,
            &recorded.knowledge_id,
            "   ",
            KnowledgeEvidencePolarity::Corroborates,
        );
        request.reference_ref = "   ".to_string();

        let error = store
            .record_evidence_reference(request, SystemTime::now())
            .await
            .unwrap_err();

        assert!(matches!(
            error,
            KnowledgeStoreError::EmptyEvidenceReferenceRef
        ));
    }

    #[tokio::test]
    async fn evidence_references_returns_only_the_requested_page_size() {
        let Some(store) = store().await else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let (tenant_id, repository_id) = unique_scope("evidence-reference-page-size");
        let recorded = store
            .record(record_request(
                &tenant_id,
                &repository_id,
                "a candidate lesson",
            ))
            .await
            .unwrap();
        for reference_ref in ["task:1", "task:2", "task:3"] {
            store
                .record_evidence_reference(
                    reference_request(
                        &tenant_id,
                        &repository_id,
                        &recorded.knowledge_id,
                        reference_ref,
                        KnowledgeEvidencePolarity::Corroborates,
                    ),
                    SystemTime::now(),
                )
                .await
                .unwrap();
        }

        let references = store
            .evidence_references(&tenant_id, &repository_id, &recorded.knowledge_id, 2)
            .await
            .unwrap();

        assert_eq!(references.len(), 2);
    }

    #[test]
    fn the_page_size_bound_clamps_an_oversized_request_down_to_the_hard_limit() {
        assert_eq!(
            bounded_evidence_reference_limit(10_000),
            MAX_EVIDENCE_REFERENCES_PAGE
        );
    }

    #[test]
    fn the_page_size_bound_clamps_a_non_positive_request_up_to_one() {
        assert_eq!(bounded_evidence_reference_limit(0), 1);
        assert_eq!(bounded_evidence_reference_limit(-5), 1);
    }

    #[test]
    fn the_page_size_bound_leaves_an_in_range_request_unchanged() {
        assert_eq!(bounded_evidence_reference_limit(10), 10);
    }
}
