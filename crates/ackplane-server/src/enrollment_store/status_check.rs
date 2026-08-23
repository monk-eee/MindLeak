use super::*;

impl EnrollmentStore {
    /// Find the binding a `CheckEnrollmentStatus` request names, and the
    /// state it is in right now. `None` means no such binding was ever
    /// recorded -- the caller must report that identically to every
    /// verification failure (ADR-0122 decision 5), never as a distinct
    /// answer.
    ///
    /// Checks `signing_keys` first, not `enrollment_requests`: retirement and
    /// revocation are written straight to `signing_keys.retired_at` /
    /// `revoked_at` as they happen, while `enrollment_requests.state` is only
    /// ever advanced as far as `Active` and then left alone, so a fingerprint
    /// that reached `Active` and was later revoked would read back as
    /// `Active` forever if `enrollment_requests` were consulted first. A
    /// `signing_keys` row exists for every fingerprint that ever activated
    /// (activation registers it in the same transaction that marks the
    /// request `Active`, see `activation.rs`), so `enrollment_requests` is
    /// only ever the *only* source of truth for the pre-activation states
    /// (`Pending`, `Approved`, `Activating`) and for a request that was
    /// `Rejected` or expired before it ever got that far.
    pub async fn find_binding(
        &self,
        tenant_id: &str,
        repository_id: &str,
        node_id: &str,
        public_key_fingerprint: &str,
        now: SystemTime,
    ) -> Result<Option<EnrollmentStatusLookup>, EnrollmentStoreError> {
        if let Some(lifecycle) = crate::signing_keys::lifecycle_for_binding(
            &self.client,
            tenant_id,
            repository_id,
            node_id,
            public_key_fingerprint,
        )
        .await?
        {
            return Ok(Some(EnrollmentStatusLookup {
                public_key: lifecycle.record.public_key.clone(),
                state: state_from_signing_key_lifecycle(&lifecycle, now),
            }));
        }

        let Some(row) = self
            .client
            .query_opt(
                "SELECT public_key, state FROM enrollment_requests \
                 WHERE tenant_id = $1 AND repository_id = $2 AND proposed_node_id = $3 \
                 AND public_key_fingerprint = $4 ORDER BY created_at DESC LIMIT 1",
                &[
                    &tenant_id,
                    &repository_id,
                    &node_id,
                    &public_key_fingerprint,
                ],
            )
            .await?
        else {
            return Ok(None);
        };
        Ok(Some(EnrollmentStatusLookup {
            public_key: row.get(0),
            state: state_from_i16(row.get(1))?,
        }))
    }

    /// Consume a (tenant, repository, node, fingerprint, nonce) tuple exactly
    /// once (anti-replay for `CheckEnrollmentStatus`, ADR-0122 decision 3).
    /// Returns true the first time a tuple is seen, false on every later
    /// attempt with the identical tuple -- the insert's own uniqueness is the
    /// enforcement, so this needs no read-then-write race. Its own table
    /// rather than a shared one, for the same reason every other domain's
    /// nonce table is its own: a coincidental collision between two unrelated
    /// domains must never refuse a legitimate request in either.
    pub async fn consume_status_nonce(
        &self,
        tenant_id: &str,
        repository_id: &str,
        node_id: &str,
        public_key_fingerprint: &str,
        nonce: &[u8],
        now: SystemTime,
    ) -> Result<bool, EnrollmentStoreError> {
        let inserted = self
            .client
            .execute(
                "INSERT INTO enrollment_status_authentication_nonces \
                 (tenant_id, repository_id, node_id, public_key_fingerprint, nonce, consumed_at) \
                 VALUES ($1, $2, $3, $4, $5, $6) \
                 ON CONFLICT (tenant_id, repository_id, node_id, public_key_fingerprint, nonce) \
                 DO NOTHING",
                &[
                    &tenant_id,
                    &repository_id,
                    &node_id,
                    &public_key_fingerprint,
                    &nonce,
                    &now,
                ],
            )
            .await?;
        Ok(inserted == 1)
    }
}

/// Judge a signing key's status as of `now`, in the same
/// revoked-then-expired-then-retired precedence `signing_keys::judge` applies
/// as of an envelope's acceptance time. `Retired` collapses to
/// `EnrollmentState::Expired`: rotation ends this exact binding's authority no
/// less than an expiry would, and it is not the security-relevant
/// administrative act that `Revoked` is, so `Expired` is the closer of the two
/// states the wire protocol actually has.
fn state_from_signing_key_lifecycle(
    lifecycle: &crate::signing_keys::SigningKeyLifecycle,
    now: SystemTime,
) -> EnrollmentState {
    if lifecycle.revoked_at.is_some_and(|revoked| now >= revoked) {
        return EnrollmentState::Revoked;
    }
    if lifecycle
        .record
        .expires_at
        .is_some_and(|expires| now >= expires)
    {
        return EnrollmentState::Expired;
    }
    if lifecycle.retired_at.is_some_and(|retired| now >= retired) {
        return EnrollmentState::Expired;
    }
    EnrollmentState::Active
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(seconds: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(seconds)
    }

    fn lifecycle() -> crate::signing_keys::SigningKeyLifecycle {
        crate::signing_keys::SigningKeyLifecycle {
            record: crate::signing_keys::SigningKeyRecord {
                signing_key_id: "signing-key-a".to_owned(),
                tenant_id: "tenant-a".to_owned(),
                repository_id: "repository-a".to_owned(),
                node_id: "node-a".to_owned(),
                public_key: vec![7; 32],
                public_key_fingerprint: "ed25519:7f3a".to_owned(),
                activated_at: at(1_000),
                expires_at: None,
            },
            retired_at: None,
            revoked_at: None,
        }
    }

    #[test]
    fn an_untouched_key_is_active() {
        assert_eq!(
            state_from_signing_key_lifecycle(&lifecycle(), at(2_000)),
            EnrollmentState::Active
        );
    }

    #[test]
    fn a_revoked_key_is_revoked_from_the_moment_of_revocation() {
        let mut lifecycle = lifecycle();
        lifecycle.revoked_at = Some(at(5_000));
        assert_eq!(
            state_from_signing_key_lifecycle(&lifecycle, at(4_999)),
            EnrollmentState::Active
        );
        assert_eq!(
            state_from_signing_key_lifecycle(&lifecycle, at(5_000)),
            EnrollmentState::Revoked
        );
    }

    #[test]
    fn an_expired_key_reports_expired() {
        let mut lifecycle = lifecycle();
        lifecycle.record.expires_at = Some(at(5_000));
        assert_eq!(
            state_from_signing_key_lifecycle(&lifecycle, at(5_000)),
            EnrollmentState::Expired
        );
    }

    #[test]
    fn a_retired_key_reports_expired_not_a_dedicated_state() {
        let mut lifecycle = lifecycle();
        lifecycle.retired_at = Some(at(5_000));
        assert_eq!(
            state_from_signing_key_lifecycle(&lifecycle, at(5_000)),
            EnrollmentState::Expired
        );
    }

    #[test]
    fn revocation_outranks_a_simultaneous_expiry() {
        let mut lifecycle = lifecycle();
        lifecycle.record.expires_at = Some(at(5_000));
        lifecycle.revoked_at = Some(at(5_000));
        assert_eq!(
            state_from_signing_key_lifecycle(&lifecycle, at(5_000)),
            EnrollmentState::Revoked
        );
    }
}
