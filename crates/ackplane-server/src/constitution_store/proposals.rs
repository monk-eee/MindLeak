//! ADR-0126 decisions 1-3: an append-only, immutable-once-authored
//! suggestion for a constitution clause change, originated from the
//! Bridge. Never activates anything -- `mod.rs`'s own header states the
//! authority boundary this table stays on the correct side of, and this
//! table extends it rather than crossing it: nothing here is ever read by
//! `get_active`/`publish`, and adoption is read-only pattern matching over
//! this table and `constitution_publications`, correlated at read time,
//! never a status this table's own writer sets.

use std::time::SystemTime;

use super::{ConstitutionStore, ConstitutionStoreError};

/// Decision 1: what `propose_clause` accepts. The suggested clause change,
/// in the exact `ClauseSnapshot` shape the existing read projection already
/// returns -- no new clause type, so a diff against the active snapshot has
/// nothing new to reconcile.
#[derive(Debug, Clone, PartialEq)]
pub struct ProposeConstitutionClauseRequest {
    pub tenant_id: String,
    pub repository_id: String,
    pub proposal_id: String,
    pub kind: String,
    pub slug: String,
    pub title: String,
    pub statement: String,
    pub consequence: Option<String>,
    pub scope: Option<String>,
    pub rationale: Option<String>,
    pub author: String,
    /// ADR-0142 decision 4: a bounded, optional "who to show in the UI"
    /// string, stored separately from and never substituted for `author`.
    pub display_label: Option<String>,
}

/// One proposal, as `propose_clause`/`list_proposals` return it.
#[derive(Debug, Clone, PartialEq)]
pub struct ConstitutionProposal {
    pub tenant_id: String,
    pub repository_id: String,
    pub proposal_id: String,
    pub kind: String,
    pub slug: String,
    pub title: String,
    pub statement: String,
    pub consequence: Option<String>,
    pub scope: Option<String>,
    pub rationale: Option<String>,
    pub author: String,
    pub display_label: Option<String>,
    pub status: String,
    pub created_at: SystemTime,
}

impl ConstitutionStore {
    /// Decisions 1/3: record a suggested clause change. A byte-identical
    /// retry for the same `(tenant_id, repository_id, proposal_id)`
    /// succeeds idempotently -- but only while that identity is still
    /// `proposed`; any other content under the same identity is refused
    /// rather than silently overwritten, the same immutability contract
    /// `record_publication` already holds, one layer lighter. Once a
    /// proposal has been withdrawn (decision 3's own only mutation), that
    /// identity is terminal: `withdraw_proposal` is the sole writer of
    /// `status`, and nothing here reverses it, byte-identical or not --
    /// `Ok(())` from this method is therefore always a true guarantee that
    /// the durable status is `proposed`, so a caller never has to re-read to
    /// know it.
    pub async fn propose_clause(
        &mut self,
        request: ProposeConstitutionClauseRequest,
    ) -> Result<(), ConstitutionStoreError> {
        if request.proposal_id.trim().is_empty() {
            return Err(ConstitutionStoreError::EmptyProposalId);
        }
        if request.author.trim().is_empty() {
            return Err(ConstitutionStoreError::EmptyAuthor);
        }

        let transaction = self.client.transaction().await?;
        let existing = transaction
            .query_opt(
                "SELECT kind, slug, title, statement, consequence, scope, rationale, author, \
                 display_label, status \
                 FROM constitution_proposals \
                 WHERE tenant_id = $1 AND repository_id = $2 AND proposal_id = $3",
                &[
                    &request.tenant_id,
                    &request.repository_id,
                    &request.proposal_id,
                ],
            )
            .await?;
        if let Some(row) = existing {
            let status: String = row.get(9);
            if status != "proposed" {
                transaction.commit().await?;
                return Err(ConstitutionStoreError::ProposalWithdrawn {
                    proposal_id: request.proposal_id.clone(),
                });
            }
            let matches = row.get::<_, String>(0) == request.kind
                && row.get::<_, String>(1) == request.slug
                && row.get::<_, String>(2) == request.title
                && row.get::<_, String>(3) == request.statement
                && row.get::<_, Option<String>>(4) == request.consequence
                && row.get::<_, Option<String>>(5) == request.scope
                && row.get::<_, Option<String>>(6) == request.rationale
                && row.get::<_, String>(7) == request.author
                && row.get::<_, Option<String>>(8) == request.display_label;
            transaction.commit().await?;
            if matches {
                return Ok(());
            }
            return Err(ConstitutionStoreError::ProposalImmutabilityViolation {
                proposal_id: request.proposal_id.clone(),
            });
        }

        transaction
            .execute(
                "INSERT INTO constitution_proposals \
                 (tenant_id, repository_id, proposal_id, kind, slug, title, statement, \
                  consequence, scope, rationale, author, display_label) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)",
                &[
                    &request.tenant_id,
                    &request.repository_id,
                    &request.proposal_id,
                    &request.kind,
                    &request.slug,
                    &request.title,
                    &request.statement,
                    &request.consequence,
                    &request.scope,
                    &request.rationale,
                    &request.author,
                    &request.display_label,
                ],
            )
            .await?;
        transaction.commit().await?;
        Ok(())
    }

    /// Every proposal recorded for this tenant/repository, newest first --
    /// both `proposed` and `withdrawn` (decision 7: never deleted, only
    /// aged out of a caller's own default view, which this method leaves
    /// entirely to its caller).
    pub async fn list_proposals(
        &self,
        tenant_id: &str,
        repository_id: &str,
    ) -> Result<Vec<ConstitutionProposal>, ConstitutionStoreError> {
        let rows = self
            .client
            .query(
                "SELECT proposal_id, kind, slug, title, statement, consequence, scope, \
                 rationale, author, display_label, status, created_at \
                 FROM constitution_proposals \
                 WHERE tenant_id = $1 AND repository_id = $2 \
                 ORDER BY created_at DESC",
                &[&tenant_id, &repository_id],
            )
            .await?;
        Ok(rows
            .into_iter()
            .map(|row| ConstitutionProposal {
                tenant_id: tenant_id.to_string(),
                repository_id: repository_id.to_string(),
                proposal_id: row.get(0),
                kind: row.get(1),
                slug: row.get(2),
                title: row.get(3),
                statement: row.get(4),
                consequence: row.get(5),
                scope: row.get(6),
                rationale: row.get(7),
                author: row.get(8),
                display_label: row.get(9),
                status: row.get(10),
                created_at: row.get(11),
            })
            .collect())
    }

    /// Decision 3: the only mutation a proposal ever receives, and only by
    /// its own author. Returns whether this call actually changed anything
    /// -- false for an unknown id, a different author, or a proposal
    /// already withdrawn, so a caller can distinguish "nothing to do" from
    /// a real state change without a separate read first.
    pub async fn withdraw_proposal(
        &self,
        tenant_id: &str,
        repository_id: &str,
        proposal_id: &str,
        author: &str,
    ) -> Result<bool, ConstitutionStoreError> {
        let updated = self
            .client
            .execute(
                "UPDATE constitution_proposals SET status = 'withdrawn' \
                 WHERE tenant_id = $1 AND repository_id = $2 AND proposal_id = $3 \
                 AND author = $4 AND status = 'proposed'",
                &[&tenant_id, &repository_id, &proposal_id, &author],
            )
            .await?;
        Ok(updated == 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constitution_store::tests::{store, unique_scope};

    fn proposal_request(
        tenant_id: &str,
        repository_id: &str,
        proposal_id: &str,
        author: &str,
    ) -> ProposeConstitutionClauseRequest {
        ProposeConstitutionClauseRequest {
            tenant_id: tenant_id.to_string(),
            repository_id: repository_id.to_string(),
            proposal_id: proposal_id.to_string(),
            kind: "constraint".to_string(),
            slug: "suggested-slug".to_string(),
            title: "Suggested title".to_string(),
            statement: "Suggested statement.".to_string(),
            consequence: Some("review".to_string()),
            scope: None,
            rationale: Some("Because the Bridge operator noticed a gap.".to_string()),
            author: author.to_string(),
            display_label: None,
        }
    }

    #[tokio::test]
    async fn a_proposed_clause_reads_back_as_proposed() {
        let Some(mut store) = store().await else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let (tenant_id, repository_id) = unique_scope("proposal-roundtrip");
        let request = proposal_request(&tenant_id, &repository_id, "proposal-1", "bridge-ui");

        store.propose_clause(request.clone()).await.unwrap();

        let proposals = store
            .list_proposals(&tenant_id, &repository_id)
            .await
            .unwrap();
        assert_eq!(proposals.len(), 1);
        assert_eq!(proposals[0].proposal_id, "proposal-1");
        assert_eq!(proposals[0].kind, request.kind);
        assert_eq!(proposals[0].slug, request.slug);
        assert_eq!(proposals[0].title, request.title);
        assert_eq!(proposals[0].statement, request.statement);
        assert_eq!(proposals[0].consequence, request.consequence);
        assert_eq!(proposals[0].scope, request.scope);
        assert_eq!(proposals[0].rationale, request.rationale);
        assert_eq!(proposals[0].author, request.author);
        assert_eq!(proposals[0].display_label, request.display_label);
        assert_eq!(proposals[0].status, "proposed");
    }

    #[tokio::test]
    async fn a_display_label_stores_separately_from_the_authoritative_author() {
        let Some(mut store) = store().await else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let (tenant_id, repository_id) = unique_scope("proposal-display-label");
        let mut request = proposal_request(&tenant_id, &repository_id, "proposal-1", "bridge-ui");
        request.display_label = Some("Jordan (via Bridge)".to_string());

        store.propose_clause(request.clone()).await.unwrap();

        let proposals = store
            .list_proposals(&tenant_id, &repository_id)
            .await
            .unwrap();
        assert_eq!(proposals.len(), 1);
        assert_eq!(
            proposals[0].display_label,
            Some("Jordan (via Bridge)".to_string())
        );
        assert_eq!(proposals[0].author, "bridge-ui");
    }

    #[tokio::test]
    async fn a_retry_with_a_different_display_label_is_an_immutability_violation() {
        let Some(mut store) = store().await else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let (tenant_id, repository_id) = unique_scope("proposal-display-label-conflict");
        let mut request = proposal_request(&tenant_id, &repository_id, "proposal-1", "bridge-ui");
        request.display_label = Some("Jordan".to_string());
        store.propose_clause(request.clone()).await.unwrap();

        request.display_label = Some("Alex".to_string());
        let error = store
            .propose_clause(request)
            .await
            .expect_err("a different display_label under the same identity must conflict");
        assert!(matches!(
            error,
            ConstitutionStoreError::ProposalImmutabilityViolation { .. }
        ));
    }

    #[tokio::test]
    async fn list_proposals_is_empty_for_an_unknown_scope() {
        let Some(store) = store().await else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let (tenant_id, repository_id) = unique_scope("proposal-empty");

        let proposals = store
            .list_proposals(&tenant_id, &repository_id)
            .await
            .unwrap();
        assert_eq!(proposals, Vec::new());
    }

    #[tokio::test]
    async fn proposing_the_same_clause_twice_is_an_idempotent_no_op() {
        let Some(mut store) = store().await else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let (tenant_id, repository_id) = unique_scope("proposal-replay");
        let request = proposal_request(&tenant_id, &repository_id, "proposal-1", "bridge-ui");

        store.propose_clause(request.clone()).await.unwrap();
        store
            .propose_clause(request.clone())
            .await
            .expect("a byte-identical replay must succeed, not error");

        let proposals = store
            .list_proposals(&tenant_id, &repository_id)
            .await
            .unwrap();
        assert_eq!(proposals.len(), 1, "a replay must not duplicate the row");
    }

    #[tokio::test]
    async fn proposing_different_content_under_the_same_id_is_rejected() {
        let Some(mut store) = store().await else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let (tenant_id, repository_id) = unique_scope("proposal-mutate");
        let mut request = proposal_request(&tenant_id, &repository_id, "proposal-1", "bridge-ui");
        store.propose_clause(request.clone()).await.unwrap();

        request.statement = "A different suggestion entirely.".to_string();
        let error = store.propose_clause(request).await.unwrap_err();
        assert!(matches!(
            error,
            ConstitutionStoreError::ProposalImmutabilityViolation { .. }
        ));

        // The original content must survive the rejected mutation attempt.
        let proposals = store
            .list_proposals(&tenant_id, &repository_id)
            .await
            .unwrap();
        assert_eq!(proposals[0].statement, "Suggested statement.");
    }

    #[tokio::test]
    async fn an_empty_proposal_id_is_refused() {
        let Some(mut store) = store().await else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let (tenant_id, repository_id) = unique_scope("proposal-empty-id");
        let request = proposal_request(&tenant_id, &repository_id, "", "bridge-ui");

        let error = store.propose_clause(request).await.unwrap_err();
        assert!(matches!(error, ConstitutionStoreError::EmptyProposalId));
    }

    #[tokio::test]
    async fn an_empty_author_is_refused() {
        let Some(mut store) = store().await else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let (tenant_id, repository_id) = unique_scope("proposal-empty-author");
        let request = proposal_request(&tenant_id, &repository_id, "proposal-1", "");

        let error = store.propose_clause(request).await.unwrap_err();
        assert!(matches!(error, ConstitutionStoreError::EmptyAuthor));
    }

    #[tokio::test]
    async fn its_own_author_can_withdraw_a_proposal() {
        let Some(mut store) = store().await else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let (tenant_id, repository_id) = unique_scope("proposal-withdraw");
        store
            .propose_clause(proposal_request(
                &tenant_id,
                &repository_id,
                "proposal-1",
                "bridge-ui",
            ))
            .await
            .unwrap();

        let withdrawn = store
            .withdraw_proposal(&tenant_id, &repository_id, "proposal-1", "bridge-ui")
            .await
            .unwrap();
        assert!(withdrawn);

        let proposals = store
            .list_proposals(&tenant_id, &repository_id)
            .await
            .unwrap();
        assert_eq!(proposals[0].status, "withdrawn");
    }

    /// Regression for the confirmed defect: replaying a withdrawn proposal
    /// used to succeed with `Ok(())` (the request still byte-matched the
    /// stored content) even though `status` durably stayed `withdrawn` --
    /// the Bridge handler then reported `status: "proposed"` from that
    /// `Ok(())` alone, contradicting `list_proposals`. `propose_clause` must
    /// now refuse the replay outright: withdrawal is this identity's one
    /// terminal mutation (decision 3), so re-proposing under the same id is
    /// rejected rather than silently reviving it, and the durable status
    /// must stay `withdrawn`.
    #[tokio::test]
    async fn replaying_a_withdrawn_proposal_is_refused_and_status_stays_withdrawn() {
        let Some(mut store) = store().await else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let (tenant_id, repository_id) = unique_scope("proposal-replay-withdrawn");
        let request = proposal_request(&tenant_id, &repository_id, "proposal-1", "bridge-ui");
        store.propose_clause(request.clone()).await.unwrap();
        assert!(store
            .withdraw_proposal(&tenant_id, &repository_id, "proposal-1", "bridge-ui")
            .await
            .unwrap());

        let error = store
            .propose_clause(request)
            .await
            .expect_err("a withdrawn proposal must refuse re-proposal under the same identity");
        assert!(matches!(
            error,
            ConstitutionStoreError::ProposalWithdrawn { proposal_id } if proposal_id == "proposal-1"
        ));

        let proposals = store
            .list_proposals(&tenant_id, &repository_id)
            .await
            .unwrap();
        assert_eq!(
            proposals[0].status, "withdrawn",
            "the refused replay must not resurrect the proposal"
        );
    }

    #[tokio::test]
    async fn a_different_author_cannot_withdraw_a_proposal() {
        let Some(mut store) = store().await else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let (tenant_id, repository_id) = unique_scope("proposal-withdraw-wrong-author");
        store
            .propose_clause(proposal_request(
                &tenant_id,
                &repository_id,
                "proposal-1",
                "bridge-ui",
            ))
            .await
            .unwrap();

        let withdrawn = store
            .withdraw_proposal(&tenant_id, &repository_id, "proposal-1", "someone-else")
            .await
            .unwrap();
        assert!(
            !withdrawn,
            "a non-author's withdrawal attempt must not change anything"
        );

        let proposals = store
            .list_proposals(&tenant_id, &repository_id)
            .await
            .unwrap();
        assert_eq!(proposals[0].status, "proposed");
    }

    #[tokio::test]
    async fn withdrawing_an_already_withdrawn_proposal_is_a_no_op_not_an_error() {
        let Some(mut store) = store().await else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let (tenant_id, repository_id) = unique_scope("proposal-double-withdraw");
        store
            .propose_clause(proposal_request(
                &tenant_id,
                &repository_id,
                "proposal-1",
                "bridge-ui",
            ))
            .await
            .unwrap();
        assert!(store
            .withdraw_proposal(&tenant_id, &repository_id, "proposal-1", "bridge-ui")
            .await
            .unwrap());

        let second_attempt = store
            .withdraw_proposal(&tenant_id, &repository_id, "proposal-1", "bridge-ui")
            .await
            .unwrap();
        assert!(
            !second_attempt,
            "a second withdrawal has nothing left to change"
        );
    }

    #[tokio::test]
    async fn withdrawing_an_unknown_proposal_returns_false_not_an_error() {
        let Some(store) = store().await else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let (tenant_id, repository_id) = unique_scope("proposal-withdraw-unknown");

        let withdrawn = store
            .withdraw_proposal(&tenant_id, &repository_id, "no-such-proposal", "bridge-ui")
            .await
            .unwrap();
        assert!(!withdrawn);
    }

    #[tokio::test]
    async fn proposals_are_scoped_to_their_own_tenant_and_repository() {
        let Some(mut store) = store().await else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let (tenant_a, repo_a) = unique_scope("proposal-scope-a");
        let (tenant_b, repo_b) = unique_scope("proposal-scope-b");
        store
            .propose_clause(proposal_request(
                &tenant_a,
                &repo_a,
                "proposal-1",
                "bridge-ui",
            ))
            .await
            .unwrap();
        store
            .propose_clause(proposal_request(
                &tenant_b,
                &repo_b,
                "proposal-1",
                "bridge-ui",
            ))
            .await
            .unwrap();

        let for_a = store.list_proposals(&tenant_a, &repo_a).await.unwrap();
        assert_eq!(for_a.len(), 1);
        let for_b = store.list_proposals(&tenant_b, &repo_b).await.unwrap();
        assert_eq!(for_b.len(), 1);
    }
}
