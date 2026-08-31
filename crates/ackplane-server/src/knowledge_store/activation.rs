use std::time::SystemTime;

use super::{KnowledgeLifecycleState, KnowledgeStore, KnowledgeStoreError};

/// One immutable receipt for an accepted Candidate -> Active transition
/// (ADR-0113 decision 7): the authorization basis, an optional reason, and
/// when. Never updated or deleted once written.
#[derive(Debug, Clone, PartialEq)]
pub struct KnowledgeActivation {
    pub activation_id: String,
    pub authorized_by: String,
    pub reason: Option<String>,
    pub activated_at: SystemTime,
}

impl KnowledgeStore {
    /// Promotes a Candidate to Active. `authorized_by` names the human actor
    /// (or, once a policy-authoring surface exists elsewhere, an adopted
    /// policy id) that authorized it -- refused when empty, matching
    /// decision 1's "refused without a satisfied authorization basis". The
    /// guarded `UPDATE` only matches a row that is still `Candidate` and not
    /// retired, so two concurrent activations of the same statement can
    /// never both land; the loser is told precisely why (already active,
    /// retired, or never recorded), mirroring `DesignStore::record_decision`'s
    /// own compare-and-swap (ADR-0111/ADR-0121 decision 3). Only once that
    /// guarded update lands does the immutable receipt get appended.
    pub async fn activate(
        &self,
        tenant_id: &str,
        repository_id: &str,
        knowledge_id: &str,
        authorized_by: &str,
        reason: Option<&str>,
        now: SystemTime,
    ) -> Result<KnowledgeActivation, KnowledgeStoreError> {
        let authorized_by = authorized_by.trim();
        if authorized_by.is_empty() {
            return Err(KnowledgeStoreError::MissingAuthorizationBasis);
        }
        let activation_id = unique_activation_id();
        let connection = self.connection().await?;
        let row = connection
            .query_opt(
                "WITH promoted AS ( \
                    UPDATE knowledge \
                       SET lifecycle_state = $6 \
                     WHERE tenant_id = $1 AND repository_id = $2 AND knowledge_id = $3 \
                       AND lifecycle_state = $7 AND retired_at IS NULL \
                 RETURNING knowledge_id \
                 ) \
                 INSERT INTO knowledge_activations \
                     (tenant_id, repository_id, knowledge_id, activation_id, authorized_by, reason, activated_at) \
                 SELECT $1, $2, knowledge_id, $4, $5, $8, $9 FROM promoted \
                 RETURNING activation_id, authorized_by, reason, activated_at",
                &[
                    &tenant_id,
                    &repository_id,
                    &knowledge_id,
                    &activation_id,
                    &authorized_by,
                    &(KnowledgeLifecycleState::Active as i16),
                    &(KnowledgeLifecycleState::Candidate as i16),
                    &reason,
                    &now,
                ],
            )
            .await?;
        if let Some(row) = row {
            return Ok(KnowledgeActivation {
                activation_id: row.get("activation_id"),
                authorized_by: row.get("authorized_by"),
                reason: row.get("reason"),
                activated_at: row.get("activated_at"),
            });
        }
        let current = connection
            .query_opt(
                "SELECT lifecycle_state, retired_at FROM knowledge \
                 WHERE tenant_id = $1 AND repository_id = $2 AND knowledge_id = $3",
                &[&tenant_id, &repository_id, &knowledge_id],
            )
            .await?;
        match current {
            None => Err(KnowledgeStoreError::UnknownKnowledge {
                knowledge_id: knowledge_id.to_string(),
            }),
            Some(row) => {
                let retired_at: Option<SystemTime> = row.get("retired_at");
                if retired_at.is_some() {
                    return Err(KnowledgeStoreError::Retired {
                        knowledge_id: knowledge_id.to_string(),
                    });
                }
                // The guarded UPDATE above already excludes "Candidate AND
                // not retired", so failing it while also finding no
                // retirement here leaves exactly one state in this
                // two-variant enum: Active. try_from still guards against a
                // genuinely corrupt numeric value rather than assuming it.
                let value: i16 = row.get("lifecycle_state");
                KnowledgeLifecycleState::try_from(value).map_err(|()| {
                    KnowledgeStoreError::CorruptLifecycleState {
                        knowledge_id: knowledge_id.to_string(),
                        value,
                    }
                })?;
                Err(KnowledgeStoreError::AlreadyActive {
                    knowledge_id: knowledge_id.to_string(),
                })
            }
        }
    }
}

fn unique_activation_id() -> String {
    let mut bytes = [0u8; 16];
    getrandom::getrandom(&mut bytes).expect("the OS random source should be available");
    let hex: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    format!("knowledge-activation-{hex}")
}

#[cfg(test)]
mod tests {
    use super::super::tests::{store, unique_scope};
    use super::*;
    use crate::knowledge_store::RecordKnowledgeRequest;

    fn request(tenant_id: &str, repository_id: &str, content: &str) -> RecordKnowledgeRequest {
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

    #[tokio::test]
    async fn record_defaults_to_candidate_and_stays_out_of_recall_and_the_active_page() {
        let Some(store) = store().await else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let (tenant_id, repository_id) = unique_scope("record-candidate");
        let recorded = store
            .record(request(&tenant_id, &repository_id, "a fresh candidate"))
            .await
            .unwrap();
        assert_eq!(recorded.lifecycle_state, KnowledgeLifecycleState::Candidate);

        let recalled = store
            .recall(&tenant_id, &repository_id, None, 10)
            .await
            .unwrap();
        assert!(recalled.entries.is_empty());

        let page = store
            .active_page(&tenant_id, &repository_id, None, 10)
            .await
            .unwrap();
        assert!(page.entries.is_empty());
    }

    #[tokio::test]
    async fn activate_refuses_an_empty_authorization_basis() {
        let Some(store) = store().await else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let (tenant_id, repository_id) = unique_scope("activate-empty-actor");
        let recorded = store
            .record(request(&tenant_id, &repository_id, "needs an actor"))
            .await
            .unwrap();

        let result = store
            .activate(
                &tenant_id,
                &repository_id,
                &recorded.knowledge_id,
                "   ",
                None,
                SystemTime::now(),
            )
            .await;
        assert!(matches!(
            result,
            Err(KnowledgeStoreError::MissingAuthorizationBasis)
        ));
    }

    #[tokio::test]
    async fn activate_promotes_a_candidate_and_it_becomes_recallable() {
        let Some(store) = store().await else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let (tenant_id, repository_id) = unique_scope("activate-promotes");
        let recorded = store
            .record(request(&tenant_id, &repository_id, "worth activating"))
            .await
            .unwrap();

        let activation = store
            .activate(
                &tenant_id,
                &repository_id,
                &recorded.knowledge_id,
                "human:reviewer-1",
                Some("matches the adopted evidence bar"),
                SystemTime::now(),
            )
            .await
            .unwrap();
        assert_eq!(activation.authorized_by, "human:reviewer-1");
        assert_eq!(
            activation.reason.as_deref(),
            Some("matches the adopted evidence bar")
        );

        let recalled = store
            .recall(&tenant_id, &repository_id, None, 10)
            .await
            .unwrap();
        assert_eq!(recalled.entries.len(), 1);
        assert_eq!(recalled.entries[0].knowledge_id, recorded.knowledge_id);

        let page = store
            .active_page(&tenant_id, &repository_id, None, 10)
            .await
            .unwrap();
        assert_eq!(page.entries.len(), 1);
        assert_eq!(page.entries[0].knowledge_id, recorded.knowledge_id);
    }

    #[tokio::test]
    async fn activate_refuses_an_already_active_statement() {
        let Some(store) = store().await else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let (tenant_id, repository_id) = unique_scope("activate-twice");
        let recorded = store
            .record(request(&tenant_id, &repository_id, "activated once"))
            .await
            .unwrap();
        store
            .activate(
                &tenant_id,
                &repository_id,
                &recorded.knowledge_id,
                "human:reviewer-1",
                None,
                SystemTime::now(),
            )
            .await
            .unwrap();

        let second = store
            .activate(
                &tenant_id,
                &repository_id,
                &recorded.knowledge_id,
                "human:reviewer-2",
                None,
                SystemTime::now(),
            )
            .await;
        assert!(matches!(
            second,
            Err(KnowledgeStoreError::AlreadyActive { knowledge_id }) if knowledge_id == recorded.knowledge_id
        ));
    }

    #[tokio::test]
    async fn activate_refuses_a_retired_statement_even_if_it_was_never_activated() {
        let Some(store) = store().await else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let (tenant_id, repository_id) = unique_scope("activate-retired");
        let recorded = store
            .record(request(&tenant_id, &repository_id, "retired before review"))
            .await
            .unwrap();
        let retired = store
            .retire(
                &tenant_id,
                &repository_id,
                &recorded.knowledge_id,
                "superseded before it was ever reviewed",
                "human:reviewer-1",
            )
            .await
            .unwrap();
        assert!(retired);

        let result = store
            .activate(
                &tenant_id,
                &repository_id,
                &recorded.knowledge_id,
                "human:reviewer-1",
                None,
                SystemTime::now(),
            )
            .await;
        assert!(matches!(
            result,
            Err(KnowledgeStoreError::Retired { knowledge_id }) if knowledge_id == recorded.knowledge_id
        ));
    }

    #[tokio::test]
    async fn activate_refuses_a_knowledge_id_that_was_never_recorded() {
        let Some(store) = store().await else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let (tenant_id, repository_id) = unique_scope("activate-unknown");

        let result = store
            .activate(
                &tenant_id,
                &repository_id,
                "knowledge-never-recorded",
                "human:reviewer-1",
                None,
                SystemTime::now(),
            )
            .await;
        assert!(matches!(
            result,
            Err(KnowledgeStoreError::UnknownKnowledge { knowledge_id }) if knowledge_id == "knowledge-never-recorded"
        ));
    }

    #[tokio::test]
    async fn activation_preserves_a_reconfirmation_recorded_while_still_a_candidate() {
        let Some(store) = store().await else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let (tenant_id, repository_id) = unique_scope("activate-preserves-history");
        let recorded = store
            .record(request(
                &tenant_id,
                &repository_id,
                "corroborated before review",
            ))
            .await
            .unwrap();
        store
            .reconfirm(
                &tenant_id,
                &repository_id,
                &recorded.knowledge_id,
                "seen again in a second incident",
                "human:corroborator",
                SystemTime::now(),
            )
            .await
            .unwrap()
            .expect("a candidate statement is still reconfirmable");

        store
            .activate(
                &tenant_id,
                &repository_id,
                &recorded.knowledge_id,
                "human:reviewer-1",
                None,
                SystemTime::now(),
            )
            .await
            .unwrap();

        let history = store.history(&tenant_id, &repository_id, 10).await.unwrap();
        let entry = history
            .iter()
            .find(|entry| entry.knowledge_id == recorded.knowledge_id)
            .expect("the activated statement stays in history");
        assert_eq!(entry.lifecycle_state, KnowledgeLifecycleState::Active);
        assert_eq!(
            entry.last_reconfirmed_by.as_deref(),
            Some("human:corroborator")
        );
        assert_eq!(
            entry.last_reconfirmation_evidence_ref.as_deref(),
            Some("seen again in a second incident")
        );
    }
}
