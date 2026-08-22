use super::*;

impl EnrollmentStore {
    /// Apply or reject a rotation, verifying continuity between the current
    /// and successor keys before either is touched (ADR-0085 decision 7).
    /// A rejection is a normal, non-error result: the wire contract carries a
    /// typed outcome precisely so a node can distinguish "try again" from
    /// "stop and re-enrol" without parsing a status message.
    pub async fn rotate_key(
        &mut self,
        rotation: &KeyRotation,
        now: SystemTime,
    ) -> Result<KeyRotationResult, EnrollmentStoreError> {
        let transaction = self.client.transaction().await?;

        let current =
            signing_keys::fetch_lifecycle_for_update(&transaction, &rotation.current_key_id)
                .await?;
        let Some(current) = current else {
            return Ok(rejected(
                rotation,
                KeyRotationRejection::CurrentKeyNotActive,
            ));
        };
        let resolution = signing_keys::judge(
            &current,
            &signing_keys::EnvelopeBinding {
                signing_key_id: &rotation.current_key_id,
                tenant_id: &rotation.tenant_id,
                repository_id: &rotation.repository_id,
                producer_id: &rotation.node_id,
                accepted_at: now,
            },
        );
        let current_public_key = match resolution {
            KeyResolution::Resolved(record) => record.public_key,
            KeyResolution::Revoked => {
                return Ok(rejected(rotation, KeyRotationRejection::NodeRevoked));
            }
            KeyResolution::Unknown
            | KeyResolution::BindingMismatch
            | KeyResolution::NotYetActive
            | KeyResolution::Expired
            | KeyResolution::Retired => {
                return Ok(rejected(
                    rotation,
                    KeyRotationRejection::CurrentKeyNotActive,
                ));
            }
        };

        if public_key_fingerprint(&rotation.successor_public_key)
            != rotation.successor_public_key_fingerprint
        {
            return Ok(rejected(
                rotation,
                KeyRotationRejection::ContinuityProofInvalid,
            ));
        }
        if signing_keys::key_exists(&transaction, &rotation.successor_key_id).await? {
            return Ok(rejected(
                rotation,
                KeyRotationRejection::SuccessorKeyConflict,
            ));
        }

        let statement = key_rotation_bytes(&KeyRotationStatement {
            tenant_id: &rotation.tenant_id,
            repository_id: &rotation.repository_id,
            node_id: &rotation.node_id,
            current_key_id: &rotation.current_key_id,
            successor_key_id: &rotation.successor_key_id,
            successor_public_key_fingerprint: &rotation.successor_public_key_fingerprint,
            successor_public_key: &rotation.successor_public_key,
            requested_overlap_seconds: rotation.requested_overlap_seconds,
        });
        let current_signed = verify_key_rotation_signature(
            &current_public_key,
            &rotation.current_key_signature,
            &statement,
        );
        let successor_signed = verify_key_rotation_signature(
            &rotation.successor_public_key,
            &rotation.successor_key_signature,
            &statement,
        );
        if !current_signed || !successor_signed {
            return Ok(rejected(
                rotation,
                KeyRotationRejection::ContinuityProofInvalid,
            ));
        }

        let overlap =
            Duration::from_secs(rotation.requested_overlap_seconds).min(MAX_ROTATION_OVERLAP);
        let retired_at = now + overlap;
        signing_keys::retire(&transaction, &rotation.current_key_id, retired_at).await?;
        signing_keys::register(
            &transaction,
            &SigningKeyRecord {
                signing_key_id: rotation.successor_key_id.clone(),
                tenant_id: rotation.tenant_id.clone(),
                repository_id: rotation.repository_id.clone(),
                node_id: rotation.node_id.clone(),
                public_key: rotation.successor_public_key.clone(),
                public_key_fingerprint: rotation.successor_public_key_fingerprint.clone(),
                activated_at: now,
                expires_at: None,
            },
        )
        .await?;
        transaction.commit().await?;

        Ok(KeyRotationResult {
            node_id: rotation.node_id.clone(),
            current_key_id: rotation.current_key_id.clone(),
            successor_key_id: rotation.successor_key_id.clone(),
            outcome: KeyRotationOutcome::Applied,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::time::UNIX_EPOCH;

    use ed25519_dalek::{Signer, SigningKey};
    use sha2::{Digest, Sha256};

    use super::*;
    use crate::enrollment::activation_challenge_bytes;
    use crate::enrollment_store::submission::tests::sample_submission_for;

    #[tokio::test]
    async fn a_continuity_proven_rotation_retires_the_old_key_and_activates_the_new_one() {
        let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let now = SystemTime::now();
        let mut store = EnrollmentStore::connect(&database_url)
            .await
            .expect("test database connects");
        let current_key = SigningKey::from_bytes(&[11; 32]);
        let successor_key = SigningKey::from_bytes(&[12; 32]);
        let signing_key_id = format!("signing-key-rotation-current-{}", node_suffix());
        let successor_key_id = format!("signing-key-rotation-successor-{}", node_suffix());
        let enrollment = activated_node(&mut store, &current_key, &signing_key_id, now).await;

        let rotation = signed_rotation(
            &current_key,
            &successor_key,
            KeyRotation {
                tenant_id: enrollment.tenant_id.clone(),
                repository_id: enrollment.repository_id.clone(),
                node_id: enrollment.proposed_node_id.clone(),
                current_key_id: signing_key_id.clone(),
                successor_key_id: successor_key_id.clone(),
                successor_public_key_fingerprint: public_key_fingerprint(
                    &successor_key.verifying_key().to_bytes(),
                ),
                successor_public_key: successor_key.verifying_key().to_bytes().to_vec(),
                current_key_signature: Vec::new(),
                successor_key_signature: Vec::new(),
                requested_overlap_seconds: 3_600,
            },
        );

        let result = store
            .rotate_key(&rotation, now)
            .await
            .expect("rotation with a valid continuity proof applies");

        assert_eq!(
            result,
            KeyRotationResult {
                node_id: enrollment.proposed_node_id,
                current_key_id: signing_key_id.clone(),
                successor_key_id: successor_key_id.clone(),
                outcome: KeyRotationOutcome::Applied,
            }
        );

        let current_after = signing_keys::resolve(
            &store.client,
            &signing_keys::EnvelopeBinding {
                signing_key_id: &signing_key_id,
                tenant_id: &rotation.tenant_id,
                repository_id: &rotation.repository_id,
                producer_id: &rotation.node_id,
                accepted_at: now + Duration::from_secs(7_200),
            },
        )
        .await
        .expect("resolve queries succeed");
        let successor_after = signing_keys::resolve(
            &store.client,
            &signing_keys::EnvelopeBinding {
                signing_key_id: &successor_key_id,
                tenant_id: &rotation.tenant_id,
                repository_id: &rotation.repository_id,
                producer_id: &rotation.node_id,
                accepted_at: now,
            },
        )
        .await
        .expect("resolve queries succeed");

        assert_eq!(current_after, signing_keys::KeyResolution::Retired);
        assert!(matches!(
            successor_after,
            signing_keys::KeyResolution::Resolved(_)
        ));
    }

    /// A rotation whose successor signature does not match the current key's
    /// statement must be rejected rather than applied — otherwise anyone who
    /// merely possesses a *successor* key, without the current key's
    /// authorisation, could rotate a node away from its owner.
    #[tokio::test]
    async fn a_rotation_missing_the_current_keys_authorisation_is_rejected() {
        let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let now = SystemTime::now();
        let mut store = EnrollmentStore::connect(&database_url)
            .await
            .expect("test database connects");
        let current_key = SigningKey::from_bytes(&[13; 32]);
        let successor_key = SigningKey::from_bytes(&[14; 32]);
        let attacker_key = SigningKey::from_bytes(&[15; 32]);
        let signing_key_id = format!("signing-key-rotation-current-{}", node_suffix());
        let successor_key_id = format!("signing-key-rotation-successor-{}", node_suffix());
        let enrollment = activated_node(&mut store, &current_key, &signing_key_id, now).await;

        // Signed by an attacker's key instead of the node's actual current key.
        let rotation = signed_rotation(
            &attacker_key,
            &successor_key,
            KeyRotation {
                tenant_id: enrollment.tenant_id.clone(),
                repository_id: enrollment.repository_id.clone(),
                node_id: enrollment.proposed_node_id.clone(),
                current_key_id: signing_key_id.clone(),
                successor_key_id: successor_key_id.clone(),
                successor_public_key_fingerprint: public_key_fingerprint(
                    &successor_key.verifying_key().to_bytes(),
                ),
                successor_public_key: successor_key.verifying_key().to_bytes().to_vec(),
                current_key_signature: Vec::new(),
                successor_key_signature: Vec::new(),
                requested_overlap_seconds: 3_600,
            },
        );

        let result = store
            .rotate_key(&rotation, now)
            .await
            .expect("rejection is a normal result, not an error");

        assert_eq!(
            result.outcome,
            KeyRotationOutcome::Rejected(KeyRotationRejection::ContinuityProofInvalid)
        );

        let successor_after = signing_keys::resolve(
            &store.client,
            &signing_keys::EnvelopeBinding {
                signing_key_id: &successor_key_id,
                tenant_id: &rotation.tenant_id,
                repository_id: &rotation.repository_id,
                producer_id: &rotation.node_id,
                accepted_at: now,
            },
        )
        .await
        .expect("resolve queries succeed");
        assert_eq!(
            successor_after,
            signing_keys::KeyResolution::Unknown,
            "a rejected rotation must not register the successor key"
        );
    }

    #[tokio::test]
    async fn rotating_an_unknown_current_key_is_rejected_as_not_active() {
        let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let now = SystemTime::now();
        let mut store = EnrollmentStore::connect(&database_url)
            .await
            .expect("test database connects");
        let current_key = SigningKey::from_bytes(&[16; 32]);
        let successor_key = SigningKey::from_bytes(&[17; 32]);

        let rotation = signed_rotation(
            &current_key,
            &successor_key,
            KeyRotation {
                tenant_id: "tenant-test".to_owned(),
                repository_id: "repository-test".to_owned(),
                node_id: format!("node-{}", node_suffix()),
                current_key_id: format!("signing-key-never-registered-{}", node_suffix()),
                successor_key_id: format!("signing-key-rotation-successor-{}", node_suffix()),
                successor_public_key_fingerprint: public_key_fingerprint(
                    &successor_key.verifying_key().to_bytes(),
                ),
                successor_public_key: successor_key.verifying_key().to_bytes().to_vec(),
                current_key_signature: Vec::new(),
                successor_key_signature: Vec::new(),
                requested_overlap_seconds: 3_600,
            },
        );

        let result = store
            .rotate_key(&rotation, now)
            .await
            .expect("rejection is a normal result, not an error");

        assert_eq!(
            result.outcome,
            KeyRotationOutcome::Rejected(KeyRotationRejection::CurrentKeyNotActive)
        );
    }

    /// Submit, approve, challenge and activate one node end to end, leaving it
    /// with a live signing key a rotation test can then act on.
    async fn activated_node(
        store: &mut EnrollmentStore,
        signing_key: &SigningKey,
        signing_key_id: &str,
        now: SystemTime,
    ) -> EnrollmentSubmission {
        let enrollment = sample_submission_for(signing_key);
        store.submit(&enrollment).await.expect("request persists");
        store
            .approve(&EnrollmentApproval {
                request_id: enrollment.request_id.clone(),
                tenant_id: enrollment.tenant_id.clone(),
                repository_id: enrollment.repository_id.clone(),
                public_key_fingerprint: enrollment.public_key_fingerprint.clone(),
                approved_capabilities: enrollment.requested_capabilities.clone(),
                approved_by: "administrator-test".to_owned(),
            })
            .await
            .expect("request is approved");
        let request = ActivationChallengeRequest {
            request_id: enrollment.request_id.clone(),
            tenant_id: enrollment.tenant_id.clone(),
            repository_id: enrollment.repository_id.clone(),
            proposed_node_id: enrollment.proposed_node_id.clone(),
            public_key_fingerprint: enrollment.public_key_fingerprint.clone(),
        };
        let challenge = store
            .issue_challenge(&request, &nonce_for(signing_key_id), now)
            .await
            .expect("approved request receives challenge");
        let signature = signing_key.sign(&activation_challenge_bytes(
            &challenge.nonce,
            &request.request_id,
            &request.tenant_id,
            &request.repository_id,
            &request.proposed_node_id,
            &request.public_key_fingerprint,
        ));
        store
            .activate(
                &EnrollmentActivation {
                    request,
                    nonce: challenge.nonce,
                    signature: signature.to_bytes().to_vec(),
                },
                &format!("receipt-{signing_key_id}"),
                signing_key_id,
                now,
            )
            .await
            .expect("valid proof activates enrollment");
        enrollment
    }

    fn signed_rotation(
        current: &SigningKey,
        successor: &SigningKey,
        rotation: KeyRotation,
    ) -> KeyRotation {
        let statement = key_rotation_bytes(&KeyRotationStatement {
            tenant_id: &rotation.tenant_id,
            repository_id: &rotation.repository_id,
            node_id: &rotation.node_id,
            current_key_id: &rotation.current_key_id,
            successor_key_id: &rotation.successor_key_id,
            successor_public_key_fingerprint: &rotation.successor_public_key_fingerprint,
            successor_public_key: &rotation.successor_public_key,
            requested_overlap_seconds: rotation.requested_overlap_seconds,
        });
        KeyRotation {
            current_key_signature: current.sign(&statement).to_bytes().to_vec(),
            successor_key_signature: successor.sign(&statement).to_bytes().to_vec(),
            ..rotation
        }
    }

    fn node_suffix() -> u128 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock is after the Unix epoch")
            .as_nanos()
    }

    /// `activation_challenges.nonce` is globally unique, so every call to
    /// `activated_node` across every test needs its own nonce rather than a
    /// shared literal.
    fn nonce_for(seed: &str) -> [u8; 32] {
        let digest = Sha256::digest(seed.as_bytes());
        digest.into()
    }
}
