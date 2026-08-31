use std::time::SystemTime;

use tokio_postgres::Row;

use super::evidence_reference::KnowledgeEvidencePolarity;
use super::query::{EFFECTIVE_WEIGHT_SQL, LATEST_RECONFIRMATION_JOIN};
use super::{KnowledgeLifecycleState, KnowledgeStore, KnowledgeStoreError};

/// How an active knowledge statement stands against ADR-0113 decision 4's
/// freshness/trust signals, computed at query time from confirmed time,
/// half-life, and any policy-defined revalidation rule -- never written back
/// as a mutable score, matching this crate's [`super::query::EFFECTIVE_WEIGHT_SQL`]
/// and this repository's standing decay invariant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KnowledgeRevalidationClassification {
    /// Not contradicted, not overdue, and within one half-life of `confirmed_at`.
    Current = 1,
    /// At least one half-life has elapsed since `confirmed_at`
    /// (`effective_weight <= 0.5`), with no contradiction and no overdue
    /// policy rule.
    ApproachingExpiry = 2,
    /// At least one `knowledge_evidence_references` row contradicts this
    /// statement (decision 3). Takes precedence over decay and policy: an
    /// evidence-backed refutation is a stronger signal than elapsed time
    /// alone, and can be recorded against a statement long after the fact
    /// without decay ever pushing it here on its own.
    Contradicted = 3,
    /// `revalidate_after_hours` is set and more time has elapsed since
    /// `confirmed_at` than it allows. Never true when no policy rule is
    /// defined (`revalidate_after_hours IS NULL`) -- absence of a policy is
    /// not itself grounds for "overdue".
    OverdueForRevalidation = 4,
}

impl TryFrom<i16> for KnowledgeRevalidationClassification {
    type Error = ();

    fn try_from(value: i16) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::Current),
            2 => Ok(Self::ApproachingExpiry),
            3 => Ok(Self::Contradicted),
            4 => Ok(Self::OverdueForRevalidation),
            _ => Err(()),
        }
    }
}

/// Pure classification mirroring the SQL `CLASSIFICATION_SQL` expression
/// `revalidation_queue`/`revalidation_entry` both evaluate at query time --
/// the same intentional Rust/SQL duplication `EFFECTIVE_WEIGHT_SQL`'s own doc
/// comment already establishes for this crate: SQL needs the expression for
/// filtering/ordering: a bounded review queue must exclude `Current` records
/// and support an allow-listed classification filter entirely in the
/// database, not by over-fetching and filtering in Rust (which would break
/// keyset pagination's page-size guarantee); Rust needs a pure version for
/// direct, DB-free testing of the precedence rule itself. Contradicted
/// outranks overdue, which outranks decay: a contradiction is an
/// evidence-backed refutation, independent of elapsed time; an overdue
/// policy rule is a human-authorized constraint, more specific than the
/// generic half-life curve. `effective_weight <= 0.5` is exactly "at least
/// one half-life has elapsed" (2^-1 == 0.5 exactly).
///
/// Only `#[cfg(test)]` code calls this directly -- production code applies
/// `CLASSIFICATION_SQL` at query time instead (see above). Without the cfg
/// gate this is unreachable in a non-test build and trips `dead_code`.
#[cfg(test)]
fn classify(
    effective_weight: f64,
    revalidation_overdue: bool,
    contradicted: bool,
) -> KnowledgeRevalidationClassification {
    if contradicted {
        KnowledgeRevalidationClassification::Contradicted
    } else if revalidation_overdue {
        KnowledgeRevalidationClassification::OverdueForRevalidation
    } else if effective_weight <= 0.5 {
        KnowledgeRevalidationClassification::ApproachingExpiry
    } else {
        KnowledgeRevalidationClassification::Current
    }
}

/// The SQL mirror of [`classify`]. Each branch value is cast to `smallint`
/// so the computed column's type matches `knowledge.lifecycle_state`'s own
/// column type and this module's `$N::smallint` bind parameters exactly,
/// rather than relying on Postgres's default `integer` inference for a bare
/// literal.
const CLASSIFICATION_SQL: &str = "CASE \
     WHEN contradicted THEN 3::smallint \
     WHEN overdue THEN 4::smallint \
     WHEN effective_weight <= 0.5 THEN 2::smallint \
     ELSE 1::smallint \
     END";

const MAX_REVALIDATION_QUEUE_PAGE_SIZE: i64 = 100;

/// One active knowledge statement's revalidation classification, as surfaced
/// in the Bridge's bounded review queue (ADR-0113 decision 4). Read-only:
/// nothing here writes `revalidate_after_hours`, authors a policy, or
/// triggers a revalidation -- this is a query-time projection only.
#[derive(Debug, Clone, PartialEq)]
pub struct RevalidationQueueEntry {
    pub knowledge_id: String,
    pub content: String,
    pub source_ref: Option<String>,
    pub recorded_by: Option<String>,
    pub reach_node_ids: Vec<String>,
    pub reach_goal_id: Option<String>,
    pub confirmed_at: SystemTime,
    pub half_life_hours: f64,
    pub revalidate_after_hours: Option<f64>,
    pub effective_weight: f64,
    pub last_reconfirmed_at: Option<SystemTime>,
    pub last_reconfirmed_by: Option<String>,
    pub last_reconfirmation_evidence_ref: Option<String>,
    pub classification: KnowledgeRevalidationClassification,
}

/// A stable keyset boundary for a Bridge page of the revalidation queue,
/// ordered oldest-`confirmed_at`-first (the longest-unconfirmed records lead).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RevalidationQueueCursor {
    pub confirmed_at: SystemTime,
    pub knowledge_id: String,
}

/// One bounded page of the revalidation queue.
#[derive(Debug, Clone, PartialEq)]
pub struct RevalidationQueuePage {
    pub entries: Vec<RevalidationQueueEntry>,
    pub next_after: Option<RevalidationQueueCursor>,
}

impl KnowledgeStore {
    /// A bounded, paginated, tenant/repository-scoped page of the
    /// revalidation review queue: every Active, non-retired statement whose
    /// classification is not [`KnowledgeRevalidationClassification::Current`],
    /// oldest-`confirmed_at`-first. `classification_filter` narrows to
    /// exactly one non-current bucket when set. Read-only: this adds no
    /// mutation route, no policy-authoring surface, and no automatic
    /// revalidation trigger -- it only surfaces what already needs review.
    pub async fn revalidation_queue(
        &self,
        tenant_id: &str,
        repository_id: &str,
        classification_filter: Option<KnowledgeRevalidationClassification>,
        after: Option<&RevalidationQueueCursor>,
        page_size: i64,
    ) -> Result<RevalidationQueuePage, KnowledgeStoreError> {
        let page_size = page_size.clamp(1, MAX_REVALIDATION_QUEUE_PAGE_SIZE);
        let after_confirmed_at = after.map(|cursor| cursor.confirmed_at);
        let after_knowledge_id = after
            .map(|cursor| cursor.knowledge_id.as_str())
            .unwrap_or_default();
        let classification_filter = classification_filter.map(|value| value as i16);
        let rows = self
            .connection()
            .await?
            .query(
                &format!(
                    "WITH classified AS ( \
                         SELECT k.knowledge_id, k.content, k.source_ref, k.recorded_by, \
                                k.reach_node_ids, k.reach_goal_id, k.confirmed_at, \
                                k.half_life_hours, k.revalidate_after_hours, \
                                {EFFECTIVE_WEIGHT_SQL} AS effective_weight, \
                                EXISTS ( \
                                    SELECT 1 FROM knowledge_evidence_references e \
                                     WHERE e.tenant_id = k.tenant_id AND e.repository_id = k.repository_id \
                                       AND e.knowledge_id = k.knowledge_id AND e.polarity = {contradicts} \
                                ) AS contradicted, \
                                (k.revalidate_after_hours IS NOT NULL \
                                 AND extract(epoch from (now() - k.confirmed_at)) / 3600.0 >= k.revalidate_after_hours \
                                ) AS overdue, \
                                latest_reconfirmation.last_reconfirmed_at, \
                                latest_reconfirmation.last_reconfirmed_by, \
                                latest_reconfirmation.last_reconfirmation_evidence_ref \
                         FROM knowledge k \
                         {LATEST_RECONFIRMATION_JOIN} \
                         WHERE k.tenant_id = $1 AND k.repository_id = $2 AND k.retired_at IS NULL \
                           AND k.lifecycle_state = {active} \
                     ), \
                     bucketed AS ( \
                         SELECT *, {CLASSIFICATION_SQL} AS classification FROM classified \
                     ) \
                     SELECT * FROM bucketed \
                     WHERE classification <> {current} \
                       AND ($3::smallint IS NULL OR classification = $3) \
                       AND ($4::timestamptz IS NULL \
                            OR confirmed_at > $4 \
                            OR (confirmed_at = $4 AND knowledge_id > $5)) \
                     ORDER BY confirmed_at ASC, knowledge_id ASC \
                     LIMIT $6",
                    contradicts = KnowledgeEvidencePolarity::Contradicts as i16,
                    active = KnowledgeLifecycleState::Active as i16,
                    current = KnowledgeRevalidationClassification::Current as i16,
                ),
                &[
                    &tenant_id,
                    &repository_id,
                    &classification_filter,
                    &after_confirmed_at,
                    &after_knowledge_id,
                    &page_size.saturating_add(1),
                ],
            )
            .await?;
        let has_next_page = rows.len() > page_size as usize;
        let entries = rows
            .into_iter()
            .take(page_size as usize)
            .map(revalidation_entry_from_row)
            .collect::<Vec<_>>();
        let next_after = has_next_page
            .then(|| {
                entries.last().map(|entry| RevalidationQueueCursor {
                    confirmed_at: entry.confirmed_at,
                    knowledge_id: entry.knowledge_id.clone(),
                })
            })
            .flatten();
        Ok(RevalidationQueuePage {
            entries,
            next_after,
        })
    }

    /// One active statement's revalidation classification, regardless of
    /// whether it currently belongs in the queue (unlike
    /// [`Self::revalidation_queue`], this does not exclude `Current`) -- the
    /// Bridge detail view for a single record named from the queue's list.
    pub async fn revalidation_entry(
        &self,
        tenant_id: &str,
        repository_id: &str,
        knowledge_id: &str,
    ) -> Result<Option<RevalidationQueueEntry>, KnowledgeStoreError> {
        let row = self
            .connection()
            .await?
            .query_opt(
                &format!(
                    "WITH classified AS ( \
                         SELECT k.knowledge_id, k.content, k.source_ref, k.recorded_by, \
                                k.reach_node_ids, k.reach_goal_id, k.confirmed_at, \
                                k.half_life_hours, k.revalidate_after_hours, \
                                {EFFECTIVE_WEIGHT_SQL} AS effective_weight, \
                                EXISTS ( \
                                    SELECT 1 FROM knowledge_evidence_references e \
                                     WHERE e.tenant_id = k.tenant_id AND e.repository_id = k.repository_id \
                                       AND e.knowledge_id = k.knowledge_id AND e.polarity = {contradicts} \
                                ) AS contradicted, \
                                (k.revalidate_after_hours IS NOT NULL \
                                 AND extract(epoch from (now() - k.confirmed_at)) / 3600.0 >= k.revalidate_after_hours \
                                ) AS overdue, \
                                latest_reconfirmation.last_reconfirmed_at, \
                                latest_reconfirmation.last_reconfirmed_by, \
                                latest_reconfirmation.last_reconfirmation_evidence_ref \
                         FROM knowledge k \
                         {LATEST_RECONFIRMATION_JOIN} \
                         WHERE k.tenant_id = $1 AND k.repository_id = $2 AND k.knowledge_id = $3 \
                           AND k.retired_at IS NULL AND k.lifecycle_state = {active} \
                     ) \
                     SELECT *, {CLASSIFICATION_SQL} AS classification FROM classified",
                    contradicts = KnowledgeEvidencePolarity::Contradicts as i16,
                    active = KnowledgeLifecycleState::Active as i16,
                ),
                &[&tenant_id, &repository_id, &knowledge_id],
            )
            .await?;
        Ok(row.map(revalidation_entry_from_row))
    }
}

fn revalidation_entry_from_row(row: Row) -> RevalidationQueueEntry {
    let classification: i16 = row.get("classification");
    RevalidationQueueEntry {
        knowledge_id: row.get("knowledge_id"),
        content: row.get("content"),
        source_ref: row.get("source_ref"),
        recorded_by: row.get("recorded_by"),
        reach_node_ids: row.get("reach_node_ids"),
        reach_goal_id: row.get("reach_goal_id"),
        confirmed_at: row.get("confirmed_at"),
        half_life_hours: row.get("half_life_hours"),
        revalidate_after_hours: row.get("revalidate_after_hours"),
        effective_weight: row.get("effective_weight"),
        last_reconfirmed_at: row.get("last_reconfirmed_at"),
        last_reconfirmed_by: row.get("last_reconfirmed_by"),
        last_reconfirmation_evidence_ref: row.get("last_reconfirmation_evidence_ref"),
        classification: KnowledgeRevalidationClassification::try_from(classification)
            .unwrap_or(KnowledgeRevalidationClassification::Current),
    }
}

#[cfg(test)]
mod tests {
    use super::super::evidence_reference::{
        KnowledgeEvidenceReferenceKind, RecordKnowledgeEvidenceReferenceRequest,
    };
    use super::super::tests::{store, unique_scope};
    use super::*;
    use crate::knowledge_store::RecordKnowledgeRequest;

    #[test]
    fn classify_prioritises_contradiction_over_overdue_and_decay() {
        assert_eq!(
            classify(0.1, true, true),
            KnowledgeRevalidationClassification::Contradicted
        );
    }

    #[test]
    fn classify_prioritises_overdue_over_decay_when_not_contradicted() {
        assert_eq!(
            classify(1.0, true, false),
            KnowledgeRevalidationClassification::OverdueForRevalidation
        );
    }

    #[test]
    fn classify_reports_approaching_expiry_at_exactly_half_life() {
        assert_eq!(
            classify(0.5, false, false),
            KnowledgeRevalidationClassification::ApproachingExpiry
        );
    }

    #[test]
    fn classify_reports_current_when_fresh_with_no_signals() {
        assert_eq!(
            classify(1.0, false, false),
            KnowledgeRevalidationClassification::Current
        );
    }

    fn record_request(
        tenant_id: &str,
        repository_id: &str,
        content: &str,
        half_life_hours: f64,
    ) -> RecordKnowledgeRequest {
        RecordKnowledgeRequest {
            tenant_id: tenant_id.to_string(),
            repository_id: repository_id.to_string(),
            content: content.to_string(),
            source_ref: None,
            recorded_by: None,
            reach_node_ids: vec![],
            reach_goal_id: None,
            half_life_hours,
            embedding: None,
        }
    }

    async fn active_statement(
        store: &KnowledgeStore,
        tenant_id: &str,
        repository_id: &str,
        content: &str,
        half_life_hours: f64,
    ) -> String {
        let recorded = store
            .record(record_request(
                tenant_id,
                repository_id,
                content,
                half_life_hours,
            ))
            .await
            .unwrap();
        store
            .activate(
                tenant_id,
                repository_id,
                &recorded.knowledge_id,
                "human:reviewer",
                None,
                SystemTime::now(),
            )
            .await
            .unwrap();
        recorded.knowledge_id
    }

    async fn set_confirmed_at_hours_ago(
        store: &KnowledgeStore,
        tenant_id: &str,
        repository_id: &str,
        knowledge_id: &str,
        hours_ago: f64,
    ) {
        store
            .connection()
            .await
            .unwrap()
            .execute(
                "UPDATE knowledge SET confirmed_at = now() - ($1 * interval '1 hour') \
                 WHERE tenant_id = $2 AND repository_id = $3 AND knowledge_id = $4",
                &[&hours_ago, &tenant_id, &repository_id, &knowledge_id],
            )
            .await
            .unwrap();
    }

    async fn set_revalidate_after_hours(
        store: &KnowledgeStore,
        tenant_id: &str,
        repository_id: &str,
        knowledge_id: &str,
        hours: f64,
    ) {
        store
            .connection()
            .await
            .unwrap()
            .execute(
                "UPDATE knowledge SET revalidate_after_hours = $1 \
                 WHERE tenant_id = $2 AND repository_id = $3 AND knowledge_id = $4",
                &[&hours, &tenant_id, &repository_id, &knowledge_id],
            )
            .await
            .unwrap();
    }

    async fn contradict(
        store: &KnowledgeStore,
        tenant_id: &str,
        repository_id: &str,
        knowledge_id: &str,
    ) {
        store
            .record_evidence_reference(
                RecordKnowledgeEvidenceReferenceRequest {
                    tenant_id: tenant_id.to_string(),
                    repository_id: repository_id.to_string(),
                    knowledge_id: knowledge_id.to_string(),
                    kind: KnowledgeEvidenceReferenceKind::Validation,
                    reference_ref: "evidence:refuted-by-a-later-run".to_string(),
                    polarity: KnowledgeEvidencePolarity::Contradicts,
                    recorded_by: "human:reviewer".to_string(),
                },
                SystemTime::now(),
            )
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn a_record_at_exactly_one_half_life_is_approaching_expiry() {
        let Some(store) = store().await else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let (tenant_id, repository_id) = unique_scope("revalidation-half-life");
        let knowledge_id = active_statement(
            &store,
            &tenant_id,
            &repository_id,
            "a decaying lesson",
            100.0,
        )
        .await;
        set_confirmed_at_hours_ago(&store, &tenant_id, &repository_id, &knowledge_id, 100.0).await;

        let entry = store
            .revalidation_entry(&tenant_id, &repository_id, &knowledge_id)
            .await
            .unwrap()
            .expect("the active record should be found");

        assert_eq!(
            entry.classification,
            KnowledgeRevalidationClassification::ApproachingExpiry
        );
        assert!(entry.effective_weight <= 0.5);
    }

    #[tokio::test]
    async fn a_contradicted_record_is_flagged_even_while_still_decay_fresh() {
        let Some(store) = store().await else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let (tenant_id, repository_id) = unique_scope("revalidation-contradicted");
        let knowledge_id = active_statement(
            &store,
            &tenant_id,
            &repository_id,
            "a contested lesson",
            100_000.0,
        )
        .await;
        contradict(&store, &tenant_id, &repository_id, &knowledge_id).await;

        let entry = store
            .revalidation_entry(&tenant_id, &repository_id, &knowledge_id)
            .await
            .unwrap()
            .expect("the active record should be found");

        assert_eq!(
            entry.classification,
            KnowledgeRevalidationClassification::Contradicted
        );
        assert!(entry.effective_weight > 0.99);
    }

    #[tokio::test]
    async fn overdue_never_fires_when_no_policy_rule_is_defined() {
        let Some(store) = store().await else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let (tenant_id, repository_id) = unique_scope("revalidation-no-policy");
        let knowledge_id = active_statement(
            &store,
            &tenant_id,
            &repository_id,
            "a lesson with no revalidation policy",
            100_000.0,
        )
        .await;
        set_confirmed_at_hours_ago(&store, &tenant_id, &repository_id, &knowledge_id, 500.0).await;

        let entry = store
            .revalidation_entry(&tenant_id, &repository_id, &knowledge_id)
            .await
            .unwrap()
            .expect("the active record should be found");

        assert_eq!(entry.revalidate_after_hours, None);
        assert_eq!(
            entry.classification,
            KnowledgeRevalidationClassification::Current
        );
    }

    #[tokio::test]
    async fn a_record_past_its_defined_revalidation_policy_is_overdue() {
        let Some(store) = store().await else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let (tenant_id, repository_id) = unique_scope("revalidation-overdue");
        let knowledge_id = active_statement(
            &store,
            &tenant_id,
            &repository_id,
            "a policy-bound lesson",
            100_000.0,
        )
        .await;
        set_revalidate_after_hours(&store, &tenant_id, &repository_id, &knowledge_id, 24.0).await;
        set_confirmed_at_hours_ago(&store, &tenant_id, &repository_id, &knowledge_id, 48.0).await;

        let entry = store
            .revalidation_entry(&tenant_id, &repository_id, &knowledge_id)
            .await
            .unwrap()
            .expect("the active record should be found");

        assert_eq!(
            entry.classification,
            KnowledgeRevalidationClassification::OverdueForRevalidation
        );
    }

    #[tokio::test]
    async fn the_queue_excludes_current_records_and_orders_oldest_confirmed_first() {
        let Some(store) = store().await else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let (tenant_id, repository_id) = unique_scope("revalidation-queue-order");
        let fresh_id = active_statement(
            &store,
            &tenant_id,
            &repository_id,
            "still current",
            100_000.0,
        )
        .await;
        let stale_id = active_statement(&store, &tenant_id, &repository_id, "stale", 10.0).await;
        set_confirmed_at_hours_ago(&store, &tenant_id, &repository_id, &stale_id, 50.0).await;
        let very_stale_id =
            active_statement(&store, &tenant_id, &repository_id, "very stale", 10.0).await;
        set_confirmed_at_hours_ago(&store, &tenant_id, &repository_id, &very_stale_id, 100.0).await;

        let page = store
            .revalidation_queue(&tenant_id, &repository_id, None, None, 10)
            .await
            .unwrap();

        let ids: Vec<&str> = page
            .entries
            .iter()
            .map(|entry| entry.knowledge_id.as_str())
            .collect();
        assert_eq!(ids, vec![very_stale_id.as_str(), stale_id.as_str()]);
        assert!(!ids.contains(&fresh_id.as_str()));
        assert!(page.next_after.is_none());
    }

    #[tokio::test]
    async fn the_queue_filters_to_one_allow_listed_classification() {
        let Some(store) = store().await else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let (tenant_id, repository_id) = unique_scope("revalidation-queue-filter");
        let contradicted_id = active_statement(
            &store,
            &tenant_id,
            &repository_id,
            "contradicted",
            100_000.0,
        )
        .await;
        contradict(&store, &tenant_id, &repository_id, &contradicted_id).await;
        let stale_id = active_statement(&store, &tenant_id, &repository_id, "stale", 10.0).await;
        set_confirmed_at_hours_ago(&store, &tenant_id, &repository_id, &stale_id, 50.0).await;

        let page = store
            .revalidation_queue(
                &tenant_id,
                &repository_id,
                Some(KnowledgeRevalidationClassification::Contradicted),
                None,
                10,
            )
            .await
            .unwrap();

        assert_eq!(page.entries.len(), 1);
        assert_eq!(page.entries[0].knowledge_id, contradicted_id);
    }

    #[tokio::test]
    async fn the_queue_paginates_via_a_stable_keyset_cursor() {
        let Some(store) = store().await else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let (tenant_id, repository_id) = unique_scope("revalidation-queue-page");
        let mut ids = Vec::new();
        for (label, hours_ago) in [("a", 90.0), ("b", 60.0), ("c", 30.0)] {
            let id = active_statement(&store, &tenant_id, &repository_id, label, 10.0).await;
            set_confirmed_at_hours_ago(&store, &tenant_id, &repository_id, &id, hours_ago).await;
            ids.push(id);
        }

        let first_page = store
            .revalidation_queue(&tenant_id, &repository_id, None, None, 2)
            .await
            .unwrap();
        assert_eq!(first_page.entries.len(), 2);
        assert_eq!(first_page.entries[0].knowledge_id, ids[0]);
        assert_eq!(first_page.entries[1].knowledge_id, ids[1]);
        let cursor = first_page.next_after.expect("a third entry remains");

        let second_page = store
            .revalidation_queue(&tenant_id, &repository_id, None, Some(&cursor), 2)
            .await
            .unwrap();
        assert_eq!(second_page.entries.len(), 1);
        assert_eq!(second_page.entries[0].knowledge_id, ids[2]);
        assert!(second_page.next_after.is_none());
    }

    #[tokio::test]
    async fn the_queue_is_isolated_by_tenant() {
        let Some(store) = store().await else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let (tenant_a, repository_id) = unique_scope("revalidation-tenant-a");
        let (tenant_b, _repository_b) = unique_scope("revalidation-tenant-b");
        let id_a = active_statement(&store, &tenant_a, &repository_id, "tenant a", 10.0).await;
        set_confirmed_at_hours_ago(&store, &tenant_a, &repository_id, &id_a, 50.0).await;

        let page_b = store
            .revalidation_queue(&tenant_b, &repository_id, None, None, 10)
            .await
            .unwrap();

        assert!(page_b.entries.is_empty());
    }
}
