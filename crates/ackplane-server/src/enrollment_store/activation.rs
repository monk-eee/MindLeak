use super::*;

impl EnrollmentStore {
    /// Return the currently valid challenge for an approved enrollment, or
    /// record a fresh supplied nonce after the prior challenge expires. The
    /// caller generates the nonce with its operating-system CSPRNG.
    pub async fn issue_challenge(
        &self,
        request: &ActivationChallengeRequest,
        nonce: &[u8],
        now: SystemTime,
    ) -> Result<IssuedActivationChallenge, EnrollmentStoreError> {
        let mut connection = self.connection().await?;
        let transaction = connection.transaction().await?;
        let enrollment = transaction
            .query_opt(
                "SELECT proposed_node_id, public_key_fingerprint, state, expires_at FROM enrollment_requests \
                 WHERE tenant_id = $1 AND repository_id = $2 AND request_id = $3 FOR UPDATE",
                &[&request.tenant_id, &request.repository_id, &request.request_id],
            )
            .await?
            .ok_or_else(|| EnrollmentStoreError::NotFound {
                request_id: request.request_id.clone(),
            })?;
        validate_binding(
            request,
            enrollment.get::<_, String>(0),
            enrollment.get::<_, String>(1),
        )?;
        let request_expires_at: SystemTime = enrollment.get(3);
        if now > request_expires_at {
            expire_enrollment(
                &transaction,
                &request.request_id,
                &request.tenant_id,
                &request.repository_id,
                &request.proposed_node_id,
                &request.public_key_fingerprint,
            )
            .await?;
            transaction.commit().await?;
            return Err(EnrollmentStoreError::RequestExpired {
                request_id: request.request_id.clone(),
            });
        }
        if state_from_i16(enrollment.get(2))? != EnrollmentState::Approved {
            return Err(EnrollmentStoreError::NotApproved {
                request_id: request.request_id.clone(),
            });
        }

        let existing = transaction
            .query_opt(
                "SELECT nonce, issued_at, expires_at FROM activation_challenges \
                 WHERE tenant_id = $1 AND repository_id = $2 AND request_id = $3 FOR UPDATE",
                &[
                    &request.tenant_id,
                    &request.repository_id,
                    &request.request_id,
                ],
            )
            .await?;
        if let Some(challenge) = existing {
            let expires_at: SystemTime = challenge.get(2);
            if expires_at > now {
                transaction.commit().await?;
                return Ok(IssuedActivationChallenge {
                    request: request.clone(),
                    nonce: challenge.get(0),
                    issued_at: challenge.get(1),
                    expires_at,
                    state: EnrollmentState::Approved,
                });
            }
        }

        let expires_at = now + ACTIVATION_CHALLENGE_LIFETIME;
        transaction
            .execute(
                "INSERT INTO activation_challenges (
                     tenant_id, repository_id, request_id, proposed_node_id,
                     public_key_fingerprint, nonce, issued_at, expires_at
                 ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8)
                 ON CONFLICT (tenant_id, repository_id, request_id) DO UPDATE SET
                     proposed_node_id = EXCLUDED.proposed_node_id,
                     public_key_fingerprint = EXCLUDED.public_key_fingerprint,
                     nonce = EXCLUDED.nonce,
                     issued_at = EXCLUDED.issued_at,
                     expires_at = EXCLUDED.expires_at,
                     consumed_at = NULL",
                &[
                    &request.tenant_id,
                    &request.repository_id,
                    &request.request_id,
                    &request.proposed_node_id,
                    &request.public_key_fingerprint,
                    &nonce,
                    &now,
                    &expires_at,
                ],
            )
            .await?;
        transaction.commit().await?;

        Ok(IssuedActivationChallenge {
            request: request.clone(),
            nonce: nonce.to_vec(),
            issued_at: now,
            expires_at,
            state: EnrollmentState::Approved,
        })
    }

    /// Verify a proof against the stored approved key, then atomically consume
    /// its challenge, record the activating transition, and mint one receipt.
    /// An exact replay returns that receipt rather than creating fresh authority.
    pub async fn activate(
        &self,
        activation: &EnrollmentActivation,
        enrollment_receipt_id: &str,
        signing_key_id: &str,
        now: SystemTime,
    ) -> Result<EnrollmentActivationResult, EnrollmentStoreError> {
        let request = &activation.request;
        let mut connection = self.connection().await?;
        let transaction = connection.transaction().await?;
        let enrollment = transaction
            .query_opt(
                "SELECT proposed_node_id, public_key_fingerprint, public_key, state, expires_at \
                 FROM enrollment_requests WHERE tenant_id = $1 AND repository_id = $2 \
                 AND request_id = $3 FOR UPDATE",
                &[
                    &request.tenant_id,
                    &request.repository_id,
                    &request.request_id,
                ],
            )
            .await?
            .ok_or_else(|| EnrollmentStoreError::NotFound {
                request_id: request.request_id.clone(),
            })?;
        validate_binding(
            request,
            enrollment.get::<_, String>(0),
            enrollment.get::<_, String>(1),
        )?;
        let public_key: Vec<u8> = enrollment.get(2);
        let state = state_from_i16(enrollment.get(3))?;
        let request_expires_at: SystemTime = enrollment.get(4);
        if now > request_expires_at {
            expire_enrollment(
                &transaction,
                &request.request_id,
                &request.tenant_id,
                &request.repository_id,
                &request.proposed_node_id,
                &request.public_key_fingerprint,
            )
            .await?;
            transaction.commit().await?;
            return Err(EnrollmentStoreError::RequestExpired {
                request_id: request.request_id.clone(),
            });
        }

        let challenge = transaction
            .query_opt(
                "SELECT nonce, expires_at, consumed_at FROM activation_challenges \
                 WHERE tenant_id = $1 AND repository_id = $2 AND request_id = $3 FOR UPDATE",
                &[
                    &request.tenant_id,
                    &request.repository_id,
                    &request.request_id,
                ],
            )
            .await?
            .ok_or_else(|| EnrollmentStoreError::ChallengeExpired {
                request_id: request.request_id.clone(),
            })?;
        let stored_nonce: Vec<u8> = challenge.get(0);
        let expires_at: SystemTime = challenge.get(1);
        let consumed_at: Option<SystemTime> = challenge.get(2);
        let proof_is_valid = stored_nonce == activation.nonce
            && verify_activation_proof(
                &public_key,
                &activation.signature,
                ActivationProofBinding {
                    nonce: &activation.nonce,
                    request_id: &request.request_id,
                    tenant_id: &request.tenant_id,
                    repository_id: &request.repository_id,
                    node_id: &request.proposed_node_id,
                    public_key_fingerprint: &request.public_key_fingerprint,
                },
            );
        if !proof_is_valid {
            return Err(EnrollmentStoreError::InvalidProof {
                request_id: request.request_id.clone(),
            });
        }

        if state == EnrollmentState::Activating && consumed_at.is_some() {
            let receipt = transaction
                .query_one(
                    "SELECT enrollment_receipt_id FROM enrollment_receipts WHERE tenant_id = $1 \
                     AND repository_id = $2 AND request_id = $3",
                    &[
                        &request.tenant_id,
                        &request.repository_id,
                        &request.request_id,
                    ],
                )
                .await?;
            // A replay must return the key actually assigned on the ORIGINAL
            // activation, not the fresh id the caller generated for this
            // retry -- signing_keys has no request_id column, so the same
            // (tenant, repository, node) lookup an external node would have
            // to do resolves it, ordered by recency in case of a later
            // rotation.
            let signing_key = transaction
                .query_one(
                    "SELECT signing_key_id FROM signing_keys WHERE tenant_id = $1 \
                     AND repository_id = $2 AND node_id = $3 ORDER BY activated_at DESC LIMIT 1",
                    &[
                        &request.tenant_id,
                        &request.repository_id,
                        &request.proposed_node_id,
                    ],
                )
                .await?;
            transaction.commit().await?;
            return Ok(EnrollmentActivationResult {
                request_id: request.request_id.clone(),
                state,
                enrollment_receipt_id: receipt.get(0),
                signing_key_id: signing_key.get(0),
            });
        }
        if state != EnrollmentState::Approved {
            return Err(EnrollmentStoreError::NotApproved {
                request_id: request.request_id.clone(),
            });
        }
        if consumed_at.is_some() {
            return Err(EnrollmentStoreError::ChallengeConsumed {
                request_id: request.request_id.clone(),
            });
        }
        if now > expires_at {
            return Err(EnrollmentStoreError::ChallengeExpired {
                request_id: request.request_id.clone(),
            });
        }

        transaction
            .execute(
                "UPDATE activation_challenges SET consumed_at = $4 WHERE tenant_id = $1 \
                 AND repository_id = $2 AND request_id = $3",
                &[
                    &request.tenant_id,
                    &request.repository_id,
                    &request.request_id,
                    &now,
                ],
            )
            .await?;
        transaction
            .execute(
                "UPDATE enrollment_requests SET state = $4 WHERE tenant_id = $1 \
                 AND repository_id = $2 AND request_id = $3",
                &[
                    &request.tenant_id,
                    &request.repository_id,
                    &request.request_id,
                    &state_as_i16(EnrollmentState::Activating),
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
                    &request.tenant_id,
                    &request.repository_id,
                    &request.request_id,
                    &request.proposed_node_id,
                    &request.public_key_fingerprint,
                    &state_as_i16(EnrollmentState::Activating),
                    &"node-proof-of-possession",
                    &"activation proof verified",
                ],
            )
            .await?;
        transaction
            .execute(
                "INSERT INTO enrollment_receipts (
                     enrollment_receipt_id, tenant_id, repository_id, request_id,
                     proposed_node_id, public_key_fingerprint, activated_at
                 ) VALUES ($1,$2,$3,$4,$5,$6,$7)",
                &[
                    &enrollment_receipt_id,
                    &request.tenant_id,
                    &request.repository_id,
                    &request.request_id,
                    &request.proposed_node_id,
                    &request.public_key_fingerprint,
                    &now,
                ],
            )
            .await?;
        // Same transaction as the receipt: a node that is activated but whose
        // key nothing can resolve would sign records no one could verify.
        signing_keys::register(
            &transaction,
            &SigningKeyRecord {
                signing_key_id: signing_key_id.to_owned(),
                tenant_id: request.tenant_id.clone(),
                repository_id: request.repository_id.clone(),
                node_id: request.proposed_node_id.clone(),
                public_key,
                public_key_fingerprint: request.public_key_fingerprint.clone(),
                activated_at: now,
                expires_at: None,
            },
        )
        .await?;
        transaction.commit().await?;

        Ok(EnrollmentActivationResult {
            request_id: request.request_id.clone(),
            state: EnrollmentState::Activating,
            enrollment_receipt_id: enrollment_receipt_id.to_owned(),
            signing_key_id: signing_key_id.to_owned(),
        })
    }
}

#[cfg(test)]
mod tests {
    use ed25519_dalek::{Signer, SigningKey};

    use super::*;
    use crate::enrollment::activation_challenge_bytes;
    use crate::enrollment_store::submission::tests::sample_submission_for;

    #[tokio::test]
    async fn activation_reuses_its_live_challenge_and_exact_replay_receipt() {
        let Some(pool) = crate::test_support::test_pool() else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let signing_key = SigningKey::from_bytes(&[8; 32]);
        let enrollment = sample_submission_for(&signing_key);
        let request = ActivationChallengeRequest {
            request_id: enrollment.request_id.clone(),
            tenant_id: enrollment.tenant_id.clone(),
            repository_id: enrollment.repository_id.clone(),
            proposed_node_id: enrollment.proposed_node_id.clone(),
            public_key_fingerprint: enrollment.public_key_fingerprint.clone(),
        };
        let approval = EnrollmentApproval {
            request_id: enrollment.request_id.clone(),
            tenant_id: enrollment.tenant_id.clone(),
            repository_id: enrollment.repository_id.clone(),
            public_key_fingerprint: enrollment.public_key_fingerprint.clone(),
            approved_capabilities: enrollment.requested_capabilities.clone(),
            approved_by: "administrator-test".to_owned(),
        };
        let now = SystemTime::now();
        let store = EnrollmentStore::connect(&pool)
            .await
            .expect("test database connects");
        store.submit(&enrollment).await.expect("request persists");
        store.approve(&approval).await.expect("request is approved");

        let challenge = store
            .issue_challenge(&request, &crate::test_support::unique_nonce(), now)
            .await
            .expect("approved request receives challenge");
        let live_nonce = challenge.nonce.clone();
        let challenge_retry = store
            .issue_challenge(&request, &crate::test_support::unique_nonce(), now)
            .await
            .expect("live challenge is returned on retry");
        let signature = signing_key.sign(&activation_challenge_bytes(
            &challenge.nonce,
            &request.request_id,
            &request.tenant_id,
            &request.repository_id,
            &request.proposed_node_id,
            &request.public_key_fingerprint,
        ));
        let activation = EnrollmentActivation {
            request,
            nonce: challenge.nonce.clone(),
            signature: signature.to_bytes().to_vec(),
        };
        let receipt_id = crate::test_support::unique_id("receipt-original");
        let signing_key_id = crate::test_support::unique_id("signing-key-original");
        let first = store
            .activate(&activation, &receipt_id, &signing_key_id, now)
            .await
            .expect("valid proof activates enrollment");
        let replay = store
            .activate(
                &activation,
                "receipt-replay-must-not-persist",
                "signing-key-replay-must-not-persist",
                now,
            )
            .await
            .expect("exact valid replay returns durable receipt");

        assert_eq!(
            (challenge.nonce, challenge_retry.nonce, first, replay),
            (
                live_nonce.clone(),
                live_nonce,
                EnrollmentActivationResult {
                    request_id: enrollment.request_id.clone(),
                    state: EnrollmentState::Activating,
                    enrollment_receipt_id: receipt_id.clone(),
                    signing_key_id: signing_key_id.clone(),
                },
                EnrollmentActivationResult {
                    request_id: enrollment.request_id,
                    state: EnrollmentState::Activating,
                    enrollment_receipt_id: receipt_id,
                    // Must be the ORIGINAL key, not the replay call's throwaway
                    // value above -- proves the replay resolves the key that
                    // was actually registered, not whatever the caller passed
                    // in on this retry.
                    signing_key_id,
                },
            )
        );
    }
}
