use super::*;

impl EnrollmentStore {
    /// Persist a pending request or return its already-recorded state when a
    /// node retries the exact same request id and binding.
    pub async fn submit(
        &self,
        submission: &EnrollmentSubmission,
    ) -> Result<EnrollmentStatus, EnrollmentStoreError> {
        if public_key_fingerprint(&submission.public_key) != submission.public_key_fingerprint {
            return Err(EnrollmentStoreError::PublicKeyFingerprintMismatch {
                request_id: submission.request_id.clone(),
            });
        }
        let mut connection = self.connection().await?;
        let transaction = connection.transaction().await?;
        let existing = transaction
            .query_opt(
                "SELECT proposed_node_id, display_name, public_key, public_key_fingerprint, \
                 requested_capabilities, state FROM enrollment_requests \
                 WHERE tenant_id = $1 AND repository_id = $2 AND request_id = $3 FOR UPDATE",
                &[
                    &submission.tenant_id,
                    &submission.repository_id,
                    &submission.request_id,
                ],
            )
            .await?;

        if let Some(row) = existing {
            let existing_state = state_from_i16(row.get(5))?;
            let matches = row.get::<_, String>(0) == submission.proposed_node_id
                && row.get::<_, String>(1) == submission.display_name
                && row.get::<_, Vec<u8>>(2) == submission.public_key
                && row.get::<_, String>(3) == submission.public_key_fingerprint
                && row.get::<_, Vec<String>>(4) == submission.requested_capabilities;
            if !matches {
                return Err(EnrollmentStoreError::RequestConflict {
                    request_id: submission.request_id.clone(),
                });
            }
            return Ok(EnrollmentStatus {
                request_id: submission.request_id.clone(),
                state: existing_state,
            });
        }

        let created_at =
            parse_rfc3339(&submission.request_id, "created_at", &submission.created_at)?;
        let expires_at =
            parse_rfc3339(&submission.request_id, "expires_at", &submission.expires_at)?;
        transaction
            .execute(
                "INSERT INTO enrollment_requests (
                     tenant_id, repository_id, request_id, proposed_node_id, display_name,
                     public_key, public_key_fingerprint, requested_capabilities, created_at,
                     expires_at, state
                 ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)",
                &[
                    &submission.tenant_id,
                    &submission.repository_id,
                    &submission.request_id,
                    &submission.proposed_node_id,
                    &submission.display_name,
                    &submission.public_key,
                    &submission.public_key_fingerprint,
                    &submission.requested_capabilities,
                    &created_at,
                    &expires_at,
                    &state_as_i16(EnrollmentState::Pending),
                ],
            )
            .await?;
        append_transition(
            &transaction,
            submission,
            EnrollmentState::Pending,
            "unauthenticated-node-request",
            "enrollment requested",
        )
        .await?;
        transaction.commit().await?;

        Ok(EnrollmentStatus {
            request_id: submission.request_id.clone(),
            state: EnrollmentState::Pending,
        })
    }

    /// Record an administrator's approval of the exact fingerprint it reviewed.
    pub async fn approve(
        &self,
        approval: &EnrollmentApproval,
    ) -> Result<EnrollmentStatus, EnrollmentStoreError> {
        let mut connection = self.connection().await?;
        let transaction = connection.transaction().await?;
        let row = transaction
            .query_opt(
                "SELECT proposed_node_id, public_key_fingerprint, state, expires_at FROM enrollment_requests \
                 WHERE tenant_id = $1 AND repository_id = $2 AND request_id = $3 FOR UPDATE",
                &[
                    &approval.tenant_id,
                    &approval.repository_id,
                    &approval.request_id,
                ],
            )
            .await?
            .ok_or_else(|| EnrollmentStoreError::NotFound {
                request_id: approval.request_id.clone(),
            })?;
        let node_id: String = row.get(0);
        let stored_fingerprint: String = row.get(1);
        let stored_state = state_from_i16(row.get(2))?;
        let expires_at: SystemTime = row.get(3);
        if SystemTime::now() > expires_at {
            expire_enrollment(
                &transaction,
                &approval.request_id,
                &approval.tenant_id,
                &approval.repository_id,
                &node_id,
                &stored_fingerprint,
            )
            .await?;
            transaction.commit().await?;
            return Err(EnrollmentStoreError::RequestExpired {
                request_id: approval.request_id.clone(),
            });
        }
        if stored_state != EnrollmentState::Pending {
            return Err(EnrollmentStoreError::NotPending {
                request_id: approval.request_id.clone(),
            });
        }
        if stored_fingerprint != approval.public_key_fingerprint {
            return Err(EnrollmentStoreError::FingerprintMismatch {
                request_id: approval.request_id.clone(),
            });
        }

        transaction
            .execute(
                "UPDATE enrollment_requests SET state = $4, approved_fingerprint = $5, \
                 approved_capabilities = $6, approved_at = now(), approved_by = $7 \
                 WHERE tenant_id = $1 AND repository_id = $2 AND request_id = $3",
                &[
                    &approval.tenant_id,
                    &approval.repository_id,
                    &approval.request_id,
                    &state_as_i16(EnrollmentState::Approved),
                    &approval.public_key_fingerprint,
                    &approval.approved_capabilities,
                    &approval.approved_by,
                ],
            )
            .await?;
        transaction
            .execute(
                "INSERT INTO enrollment_transitions (
                     tenant_id, repository_id, request_id, proposed_node_id,
                     public_key_fingerprint, state, actor, reason
                 ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8)",
                &[
                    &approval.tenant_id,
                    &approval.repository_id,
                    &approval.request_id,
                    &node_id,
                    &approval.public_key_fingerprint,
                    &state_as_i16(EnrollmentState::Approved),
                    &approval.approved_by,
                    &"fingerprint approved",
                ],
            )
            .await?;
        transaction.commit().await?;

        Ok(EnrollmentStatus {
            request_id: approval.request_id.clone(),
            state: EnrollmentState::Approved,
        })
    }
}

#[cfg(test)]
pub(super) mod tests {
    use std::time::UNIX_EPOCH;

    use ed25519_dalek::SigningKey;

    use super::*;

    #[tokio::test]
    async fn exact_retry_observes_the_approved_enrollment_state() {
        let Some(pool) = crate::test_support::test_pool() else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let store = EnrollmentStore::connect(&pool)
            .await
            .expect("test database connects");
        let enrollment = sample_submission();

        let first = store.submit(&enrollment).await.expect("request persists");
        let retry = store
            .submit(&enrollment)
            .await
            .expect("retry is idempotent");
        let approved = store
            .approve(&EnrollmentApproval {
                request_id: enrollment.request_id.clone(),
                tenant_id: enrollment.tenant_id.clone(),
                repository_id: enrollment.repository_id.clone(),
                public_key_fingerprint: enrollment.public_key_fingerprint.clone(),
                approved_capabilities: enrollment.requested_capabilities.clone(),
                approved_by: "administrator-test".to_owned(),
            })
            .await
            .expect("exact fingerprint approval succeeds");
        let retry_after_approval = store
            .submit(&enrollment)
            .await
            .expect("retry observes current state");

        assert_eq!(
            (first, retry, approved, retry_after_approval),
            (
                EnrollmentStatus {
                    request_id: enrollment.request_id.clone(),
                    state: EnrollmentState::Pending,
                },
                EnrollmentStatus {
                    request_id: enrollment.request_id.clone(),
                    state: EnrollmentState::Pending,
                },
                EnrollmentStatus {
                    request_id: enrollment.request_id.clone(),
                    state: EnrollmentState::Approved,
                },
                EnrollmentStatus {
                    request_id: enrollment.request_id,
                    state: EnrollmentState::Approved,
                },
            )
        );
    }

    pub(in crate::enrollment_store) fn sample_submission() -> EnrollmentSubmission {
        sample_submission_for(&SigningKey::from_bytes(&[7; 32]))
    }

    pub(in crate::enrollment_store) fn sample_submission_for(
        signing_key: &SigningKey,
    ) -> EnrollmentSubmission {
        let unique_suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock is after the Unix epoch")
            .as_nanos();
        EnrollmentSubmission {
            request_id: format!("request-{unique_suffix}"),
            tenant_id: "tenant-test".to_owned(),
            repository_id: "repository-test".to_owned(),
            proposed_node_id: format!("node-{unique_suffix}"),
            display_name: "Node test".to_owned(),
            public_key: signing_key.verifying_key().to_bytes().to_vec(),
            public_key_fingerprint: public_key_fingerprint(&signing_key.verifying_key().to_bytes()),
            requested_capabilities: vec!["synchronize".to_owned()],
            created_at: "2026-01-01T00:00:00Z".to_owned(),
            expires_at: "2030-01-01T00:00:00Z".to_owned(),
        }
    }
}
