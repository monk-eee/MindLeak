use std::time::SystemTime;

use super::reach::validate_reach;
use super::{
    unique_knowledge_id, Knowledge, KnowledgeLifecycleState, KnowledgeStore, KnowledgeStoreError,
};

/// One immutable receipt recording that an active statement was superseded
/// by a newly-recorded replacement (ADR-0113 decisions 1 and 7): the prior
/// statement's own row is never rewritten in place -- it is marked
/// `Superseded` and points at the replacement; this receipt is the durable
/// record of who authorized the change and why the replacement won. Never
/// updated or deleted once written, the same append-only contract as
/// `KnowledgeActivation`/`KnowledgeReconfirmation`.
#[derive(Debug, Clone, PartialEq)]
pub struct KnowledgeSupersession {
    pub supersession_id: String,
    pub new_knowledge_id: String,
    pub authorized_by: String,
    pub reason: String,
    pub superseded_at: SystemTime,
}

/// Everything superseding a prior statement needs: the target being
/// replaced, the replacement's own content, and who authorized the change and
/// why. Uses a request struct rather than positional parameters for the same
/// reason `record`'s own `RecordKnowledgeRequest` does -- enough fields that
/// a bare parameter list would obscure which is which (exempted from the
/// tenant-id guard's literal-signature check alongside `record`, in
/// `repository_id_guard.rs`, for the same reason).
#[derive(Debug, Clone, PartialEq)]
pub struct SupersedeKnowledgeRequest {
    pub tenant_id: String,
    pub repository_id: String,
    /// The prior statement being replaced. Must currently be `Active`.
    pub knowledge_id: String,
    /// The replacement statement's own content -- a genuine revision, not a
    /// copy; unchanged-content corroboration is what `reconfirm` is for.
    pub new_content: String,
    pub new_source_ref: Option<String>,
    pub new_recorded_by: Option<String>,
    pub new_reach_node_ids: Vec<String>,
    pub new_reach_goal_id: Option<String>,
    pub new_half_life_hours: f64,
    /// The human actor (or an adopted policy id) authorizing the
    /// supersession -- refused when empty, matching `activate`'s own
    /// authorization-basis requirement (decision 1).
    pub authorized_by: String,
    /// Why the replacement won. Required and non-empty: unlike activation's
    /// optional reason, decision 1 specifically says supersession "records
    /// why the replacement won" -- there is no supersession without one.
    pub reason: String,
}

impl KnowledgeStore {
    /// Supersedes an active statement with a newly-recorded replacement in
    /// one atomic operation: the replacement is inserted directly as
    /// `Active` (the supersession's own authorization basis already
    /// satisfies decision 1's review gate -- routing it through a second,
    /// separate `activate` call would let an unreviewed candidate silently
    /// stand in for the prior statement in between). The prior statement's
    /// `lifecycle_state` moves `Active -> Superseded` with `superseded_by`
    /// set, and one receipt is appended -- all inside a single `WITH`
    /// statement, so a retired, already-superseded, or still-`Candidate`
    /// prior statement cannot acquire a replacement through an interleaving
    /// write. A guard failure is diagnosed precisely rather than folded into
    /// one generic conflict, mirroring `activate`'s own compare-and-swap
    /// (ADR-0111/ADR-0121 decision 3).
    pub async fn supersede(
        &self,
        request: SupersedeKnowledgeRequest,
        now: SystemTime,
    ) -> Result<(Knowledge, KnowledgeSupersession), KnowledgeStoreError> {
        let authorized_by = request.authorized_by.trim();
        if authorized_by.is_empty() {
            return Err(KnowledgeStoreError::MissingAuthorizationBasis);
        }
        let reason = request.reason.trim();
        if reason.is_empty() {
            return Err(KnowledgeStoreError::MissingSupersessionReason);
        }
        if request.new_content.trim().is_empty() {
            return Err(KnowledgeStoreError::EmptyContent);
        }
        if request.new_half_life_hours <= 0.0 {
            return Err(KnowledgeStoreError::InvalidHalfLife);
        }
        validate_reach(
            &request.new_reach_node_ids,
            request.new_reach_goal_id.as_deref(),
        )?;

        let new_knowledge_id = unique_knowledge_id();
        let supersession_id = unique_supersession_id();
        let row = self
            .connection()
            .await?
            .query_opt(
                "WITH superseded AS ( \
                    UPDATE knowledge \
                       SET lifecycle_state = $6, superseded_by = $4 \
                     WHERE tenant_id = $1 AND repository_id = $2 AND knowledge_id = $3 \
                       AND lifecycle_state = $7 AND retired_at IS NULL \
                 RETURNING knowledge_id \
                 ), replacement AS ( \
                    INSERT INTO knowledge \
                        (tenant_id, repository_id, knowledge_id, content, source_ref, recorded_by, reach_node_ids, reach_goal_id, half_life_hours, confirmed_at, lifecycle_state) \
                    SELECT $1, $2, $4, $8, $9, $10, $11, $12, $13, $14, $7 FROM superseded \
                 RETURNING knowledge_id \
                 ) \
                 INSERT INTO knowledge_supersessions \
                     (tenant_id, repository_id, knowledge_id, supersession_id, new_knowledge_id, authorized_by, reason, superseded_at) \
                 SELECT $1, $2, superseded.knowledge_id, $5, replacement.knowledge_id, $15, $16, $14 \
                   FROM superseded, replacement \
                 RETURNING supersession_id, new_knowledge_id, authorized_by, reason, superseded_at",
                &[
                    &request.tenant_id,                            // $1
                    &request.repository_id,                       // $2
                    &request.knowledge_id,                         // $3
                    &new_knowledge_id,                             // $4
                    &supersession_id,                              // $5
                    &(KnowledgeLifecycleState::Superseded as i16), // $6
                    &(KnowledgeLifecycleState::Active as i16),     // $7 (required prior state; also the replacement's own new state)
                    &request.new_content,                          // $8
                    &request.new_source_ref,                       // $9
                    &request.new_recorded_by,                      // $10
                    &request.new_reach_node_ids,                   // $11
                    &request.new_reach_goal_id,                    // $12
                    &request.new_half_life_hours,                  // $13
                    &now,                                          // $14
                    &authorized_by,                                // $15
                    &reason,                                       // $16
                ],
            )
            .await?;

        let Some(row) = row else {
            return Err(self
                .diagnose_supersede_failure(
                    &request.knowledge_id,
                    &request.tenant_id,
                    &request.repository_id,
                )
                .await?);
        };

        let supersession = KnowledgeSupersession {
            supersession_id: row.get("supersession_id"),
            new_knowledge_id: row.get("new_knowledge_id"),
            authorized_by: row.get("authorized_by"),
            reason: row.get("reason"),
            superseded_at: row.get("superseded_at"),
        };
        let replacement = Knowledge {
            knowledge_id: new_knowledge_id,
            tenant_id: request.tenant_id,
            repository_id: request.repository_id,
            content: request.new_content,
            source_ref: request.new_source_ref,
            recorded_by: request.new_recorded_by,
            reach_node_ids: request.new_reach_node_ids,
            reach_goal_id: request.new_reach_goal_id,
            half_life_hours: request.new_half_life_hours,
            confirmed_at: now,
            lifecycle_state: KnowledgeLifecycleState::Active,
            superseded_by: None,
        };
        Ok((replacement, supersession))
    }

    async fn diagnose_supersede_failure(
        &self,
        knowledge_id: &str,
        tenant_id: &str,
        repository_id: &str,
    ) -> Result<KnowledgeStoreError, KnowledgeStoreError> {
        let current = self
            .connection()
            .await?
            .query_opt(
                "SELECT lifecycle_state, retired_at FROM knowledge \
                 WHERE tenant_id = $1 AND repository_id = $2 AND knowledge_id = $3",
                &[&tenant_id, &repository_id, &knowledge_id],
            )
            .await?;
        Ok(match current {
            None => KnowledgeStoreError::UnknownKnowledge {
                knowledge_id: knowledge_id.to_string(),
            },
            Some(row) => {
                let retired_at: Option<SystemTime> = row.get("retired_at");
                if retired_at.is_some() {
                    return Ok(KnowledgeStoreError::Retired {
                        knowledge_id: knowledge_id.to_string(),
                    });
                }
                let value: i16 = row.get("lifecycle_state");
                match KnowledgeLifecycleState::try_from(value) {
                    Ok(KnowledgeLifecycleState::Superseded) => {
                        KnowledgeStoreError::AlreadySuperseded {
                            knowledge_id: knowledge_id.to_string(),
                        }
                    }
                    Ok(KnowledgeLifecycleState::Candidate) => KnowledgeStoreError::NotActive {
                        knowledge_id: knowledge_id.to_string(),
                    },
                    // The guarded UPDATE above excludes "Active AND not
                    // retired", so an Active-and-not-retired read here means
                    // a concurrent write changed it back between the failed
                    // CAS and this diagnostic read -- a genuine, if narrow,
                    // race rather than corruption. Ask the caller to retry
                    // rather than mislabel it as already-superseded or
                    // not-yet-active.
                    Ok(KnowledgeLifecycleState::Active) => {
                        KnowledgeStoreError::ConcurrentlyModified {
                            knowledge_id: knowledge_id.to_string(),
                        }
                    }
                    Err(()) => KnowledgeStoreError::CorruptLifecycleState {
                        knowledge_id: knowledge_id.to_string(),
                        value,
                    },
                }
            }
        })
    }
}

fn unique_supersession_id() -> String {
    let mut bytes = [0u8; 16];
    getrandom::getrandom(&mut bytes).expect("the OS random source should be available");
    let hex: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    format!("knowledge-supersession-{hex}")
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

    fn supersede_request(
        tenant_id: &str,
        repository_id: &str,
        knowledge_id: &str,
        new_content: &str,
    ) -> SupersedeKnowledgeRequest {
        SupersedeKnowledgeRequest {
            tenant_id: tenant_id.to_string(),
            repository_id: repository_id.to_string(),
            knowledge_id: knowledge_id.to_string(),
            new_content: new_content.to_string(),
            new_source_ref: None,
            new_recorded_by: None,
            new_reach_node_ids: vec![],
            new_reach_goal_id: None,
            new_half_life_hours: 720.0,
            authorized_by: "human:reviewer".to_string(),
            reason: "the replacement corrects a stale path reference".to_string(),
        }
    }

    async fn active_statement(
        store: &KnowledgeStore,
        tenant_id: &str,
        repository_id: &str,
        content: &str,
    ) -> String {
        let recorded = store
            .record(record_request(tenant_id, repository_id, content))
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

    #[tokio::test]
    async fn supersede_marks_the_prior_statement_superseded_and_activates_the_replacement() {
        let Some(store) = store().await else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let (tenant_id, repository_id) = unique_scope("supersede-happy-path");
        let prior_id =
            active_statement(&store, &tenant_id, &repository_id, "the old guidance").await;

        let (replacement, receipt) = store
            .supersede(
                supersede_request(
                    &tenant_id,
                    &repository_id,
                    &prior_id,
                    "the corrected guidance",
                ),
                SystemTime::now(),
            )
            .await
            .unwrap();

        assert_eq!(replacement.lifecycle_state, KnowledgeLifecycleState::Active);
        assert_eq!(replacement.content, "the corrected guidance");
        assert_eq!(receipt.new_knowledge_id, replacement.knowledge_id);
        assert_eq!(receipt.authorized_by, "human:reviewer");
        assert_eq!(
            receipt.reason,
            "the replacement corrects a stale path reference"
        );

        let history = store.history(&tenant_id, &repository_id, 10).await.unwrap();
        let prior_entry = history
            .iter()
            .find(|entry| entry.knowledge_id == prior_id)
            .unwrap();
        assert_eq!(
            prior_entry.lifecycle_state,
            KnowledgeLifecycleState::Superseded
        );
        assert_eq!(
            prior_entry.superseded_by,
            Some(replacement.knowledge_id.clone())
        );
        assert_eq!(prior_entry.retired_at, None);
        assert_eq!(prior_entry.content, "the old guidance");
    }

    #[tokio::test]
    async fn a_superseded_statement_disappears_from_recall_and_the_replacement_takes_its_place() {
        let Some(store) = store().await else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let (tenant_id, repository_id) = unique_scope("supersede-recall");
        let prior_id =
            active_statement(&store, &tenant_id, &repository_id, "the old guidance").await;

        let (replacement, _) = store
            .supersede(
                supersede_request(
                    &tenant_id,
                    &repository_id,
                    &prior_id,
                    "the corrected guidance",
                ),
                SystemTime::now(),
            )
            .await
            .unwrap();

        let recalled = store
            .recall(&tenant_id, &repository_id, None, 10)
            .await
            .unwrap();
        let recalled_ids: Vec<&str> = recalled
            .entries
            .iter()
            .map(|entry| entry.knowledge_id.as_str())
            .collect();
        assert!(!recalled_ids.contains(&prior_id.as_str()));
        assert!(recalled_ids.contains(&replacement.knowledge_id.as_str()));
    }

    #[tokio::test]
    async fn supersede_refuses_an_empty_authorization_basis() {
        let Some(store) = store().await else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let (tenant_id, repository_id) = unique_scope("supersede-empty-authorized-by");
        let prior_id =
            active_statement(&store, &tenant_id, &repository_id, "the old guidance").await;
        let mut request = supersede_request(
            &tenant_id,
            &repository_id,
            &prior_id,
            "the corrected guidance",
        );
        request.authorized_by = "   ".to_string();

        let error = store
            .supersede(request, SystemTime::now())
            .await
            .unwrap_err();

        assert!(matches!(
            error,
            KnowledgeStoreError::MissingAuthorizationBasis
        ));
    }

    #[tokio::test]
    async fn supersede_refuses_an_empty_reason() {
        let Some(store) = store().await else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let (tenant_id, repository_id) = unique_scope("supersede-empty-reason");
        let prior_id =
            active_statement(&store, &tenant_id, &repository_id, "the old guidance").await;
        let mut request = supersede_request(
            &tenant_id,
            &repository_id,
            &prior_id,
            "the corrected guidance",
        );
        request.reason = "  ".to_string();

        let error = store
            .supersede(request, SystemTime::now())
            .await
            .unwrap_err();

        assert!(matches!(
            error,
            KnowledgeStoreError::MissingSupersessionReason
        ));
    }

    #[tokio::test]
    async fn supersede_refuses_an_unknown_knowledge_id() {
        let Some(store) = store().await else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let (tenant_id, repository_id) = unique_scope("supersede-unknown");

        let error = store
            .supersede(
                supersede_request(
                    &tenant_id,
                    &repository_id,
                    "knowledge-does-not-exist",
                    "new content",
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
    async fn supersede_refuses_a_still_candidate_statement() {
        let Some(store) = store().await else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let (tenant_id, repository_id) = unique_scope("supersede-candidate");
        let recorded = store
            .record(record_request(
                &tenant_id,
                &repository_id,
                "an unreviewed candidate",
            ))
            .await
            .unwrap();

        let error = store
            .supersede(
                supersede_request(
                    &tenant_id,
                    &repository_id,
                    &recorded.knowledge_id,
                    "new content",
                ),
                SystemTime::now(),
            )
            .await
            .unwrap_err();

        assert!(matches!(
            error,
            KnowledgeStoreError::NotActive { knowledge_id } if knowledge_id == recorded.knowledge_id
        ));
    }

    #[tokio::test]
    async fn supersede_refuses_an_already_retired_statement() {
        let Some(store) = store().await else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let (tenant_id, repository_id) = unique_scope("supersede-retired");
        let prior_id =
            active_statement(&store, &tenant_id, &repository_id, "the old guidance").await;
        store
            .retire(
                &tenant_id,
                &repository_id,
                &prior_id,
                "no longer applicable",
                "human:reviewer",
            )
            .await
            .unwrap();

        let error = store
            .supersede(
                supersede_request(&tenant_id, &repository_id, &prior_id, "new content"),
                SystemTime::now(),
            )
            .await
            .unwrap_err();

        assert!(matches!(
            error,
            KnowledgeStoreError::Retired { knowledge_id } if knowledge_id == prior_id
        ));
    }

    #[tokio::test]
    async fn supersede_refuses_an_already_superseded_statement() {
        let Some(store) = store().await else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let (tenant_id, repository_id) = unique_scope("supersede-twice");
        let prior_id =
            active_statement(&store, &tenant_id, &repository_id, "the old guidance").await;
        store
            .supersede(
                supersede_request(
                    &tenant_id,
                    &repository_id,
                    &prior_id,
                    "the first replacement",
                ),
                SystemTime::now(),
            )
            .await
            .unwrap();

        let error = store
            .supersede(
                supersede_request(
                    &tenant_id,
                    &repository_id,
                    &prior_id,
                    "a second replacement",
                ),
                SystemTime::now(),
            )
            .await
            .unwrap_err();

        assert!(matches!(
            error,
            KnowledgeStoreError::AlreadySuperseded { knowledge_id } if knowledge_id == prior_id
        ));
    }
}
